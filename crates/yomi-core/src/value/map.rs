//! The contents of a `map` (`MapData`). Insertion order and an index, sharing and copy on write, and incremental tracking
//! of the total size.

use super::*;
use hashbrown::HashTable;

/// The contents of a `map`. Holds pairs of keys and values in insertion order, with an index for looking up keys (key →
/// where the pair is stored).
///
/// - Lookup, update, insertion and removal take time proportional to the size of the key (not a linear search). Keys are
///   compared and hashed as values, without converting them to strings.
/// - Removal only empties the slot of the pair, without shifting the pairs after it. When the holes exceed the number of
///   pairs, they are compacted at once (amortized constant time per removal). [`MapData::entries`] goes through the pairs
///   in insertion order, skipping holes.
/// - Cloning only shares the `Arc` (the first update after cloning copies the contents). Reading a huge `map` does not
///   mean copying the whole thing every time. A clone is independent of the original (value semantics).
/// - The index is not saved. It is rebuilt on restore (`From<Vec<_>>`, `Deserialize`).
/// - Keeps the total size of the pairs (the estimate of [`Value::approx_bytes`]) incrementally, so checking value sizes is
///   not proportional to the size of the `map`. When a value inside nesting is updated, the outer side reports it with
///   `adjust_bytes` (within the crate).
/// - `==` does not compare insertion order (equal if they have the same values for the same keys).
#[derive(Clone, Default)]
pub struct MapData(Arc<MapInner>);

/// Shows only the sequence of pairs (`[(key, value), …]`), without the index or holes (so the output is stable).
impl std::fmt::Debug for MapData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.entries()).finish()
    }
}

/// The contents of a `map` (pairs, index, total size).
#[derive(Clone, Default)]
pub(super) struct MapInner {
    /// The slots of pairs (in insertion order). Removed slots stay as `None`, and are compacted when holes exceed `live`.
    /// The last slot is never a hole (it shrinks when a removal empties the end).
    slots: Vec<Option<Slot>>,
    /// Key hash → index into `slots`. The key itself lives only in `slots`.
    index: HashTable<usize>,
    /// The number of pairs (the number of `Some` in `slots`).
    live: usize,
    /// The total size of the pairs (the sum of [`Value::approx_bytes`] of keys and values, not including the 24 of the `map`
    /// itself). On each update, only the change is added or subtracted (no recounting).
    bytes: usize,
}

#[derive(Clone)]
struct Slot {
    pair: (Value, Value),
    /// The hash of the key. Kept so keys need not be rehashed when compacting.
    hash: u64,
}

/// The iterator returned by [`MapData::entries`] that goes through the pairs in insertion order (skipping holes left by
/// removals).
#[derive(Clone)]
pub struct MapEntries<'a> {
    slots: std::slice::Iter<'a, Option<Slot>>,
    remaining: usize,
}

impl<'a> Iterator for MapEntries<'a> {
    type Item = &'a (Value, Value);
    fn next(&mut self) -> Option<Self::Item> {
        let e = self.slots.by_ref().flatten().next()?;
        self.remaining -= 1;
        Some(&e.pair)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl DoubleEndedIterator for MapEntries<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let e = self.slots.by_ref().rev().flatten().next()?;
        self.remaining -= 1;
        Some(&e.pair)
    }
}

impl ExactSizeIterator for MapEntries<'_> {}

impl std::iter::FusedIterator for MapEntries<'_> {}

/// Shows the sequence of remaining pairs.
impl std::fmt::Debug for MapEntries<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.clone()).finish()
    }
}

impl MapData {
    /// An empty `map`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes one from a sequence of pairs. If a key repeats, the later value updates it (the position stays the first one).
    pub fn from_pairs(pairs: Vec<(Value, Value)>) -> Self {
        let mut inner = MapInner {
            slots: Vec::with_capacity(pairs.len()),
            index: HashTable::with_capacity(pairs.len()),
            live: 0,
            bytes: 0,
        };
        for (k, v) in pairs {
            inner.upsert(k, v);
        }
        Self(Arc::new(inner))
    }

    /// Goes through the pairs in insertion order.
    pub fn entries(&self) -> MapEntries<'_> {
        MapEntries {
            slots: self.0.slots.iter(),
            remaining: self.0.live,
        }
    }

    /// Goes through the pairs in insertion order (same as [`MapData::entries`]).
    pub fn iter(&self) -> MapEntries<'_> {
        self.entries()
    }

    /// The number of pairs.
    pub fn len(&self) -> usize {
        self.0.live
    }

    /// Whether there are no pairs.
    pub fn is_empty(&self) -> bool {
        self.0.live == 0
    }

    /// Takes the pairs out (in insertion order). Does not copy the pairs if not shared.
    pub fn into_entries(self) -> Vec<(Value, Value)> {
        match Arc::try_unwrap(self.0) {
            Ok(inner) => inner.slots.into_iter().flatten().map(|s| s.pair).collect(),
            Err(shared) => shared
                .slots
                .iter()
                .flatten()
                .map(|s| s.pair.clone())
                .collect(),
        }
    }

    /// Whether the key exists.
    pub fn contains_key(&self, key: &Value) -> bool {
        self.slot(key).is_some()
    }

    /// The value of the key.
    pub fn get(&self, key: &Value) -> Option<&Value> {
        self.slot(key).map(|i| &self.0.entry(i).1)
    }

    /// The slot of the key's pair (passed to `value_mut` and `remove_at`; it may change on insertion and removal).
    pub(crate) fn slot(&self, key: &Value) -> Option<usize> {
        self.0.find(hash_of(key), key)
    }

    /// The slot of the key's pair. If missing, returns the hash of the key (passed to [`MapData::push_hashed`], so the hash is
    /// not computed twice).
    pub(crate) fn slot_or_hash(&self, key: &Value) -> Result<usize, u64> {
        let hash = hash_of(key);
        self.0.find(hash, key).ok_or(hash)
    }

    /// Compacts the holes left by removals, if any (copies the contents first if shared). Used for the copy that a `for`
    /// iterates. After compacting, slots match positions in insertion order, so positions do not change across save and load.
    pub(crate) fn compact_for_iteration(&mut self) {
        if self.0.live != self.0.slots.len() {
            Arc::make_mut(&mut self.0).compact();
        }
    }

    /// The pair in slot `i` (`None` if out of range or a hole left by a removal).
    pub(crate) fn entry_at(&self, i: usize) -> Option<&(Value, Value)> {
        self.0.slots.get(i)?.as_ref().map(|s| &s.pair)
    }

    /// Appends a pair to the end when `slot_or_hash` returned a hash (the key is missing).
    pub(crate) fn push_hashed(&mut self, hash: u64, key: Value, value: Value) {
        Arc::make_mut(&mut self.0).push_entry(hash, key, value);
    }

    /// Returns the value in slot `i` in a mutable form (copies the contents first if shared).
    pub(crate) fn value_mut(&mut self, i: usize) -> &mut Value {
        &mut Arc::make_mut(&mut self.0).slots[i]
            .as_mut()
            .expect("組がある")
            .pair
            .1
    }

    /// If the key is missing, appends it and returns `true`; otherwise does nothing and returns `false`.
    pub(crate) fn push_new(&mut self, key: Value, value: Value) -> bool {
        let hash = hash_of(&key);
        if self.0.find(hash, &key).is_some() {
            return false;
        }
        Arc::make_mut(&mut self.0).push_entry(hash, key, value);
        true
    }

    /// The total size of the pairs (not including the 24 of the `map` itself).
    pub(crate) fn content_bytes(&self) -> usize {
        self.0.bytes
    }

    /// After updating a value inside (modified via `value_mut`), corrects the total size of the pairs as having decreased by
    /// `shrink` and increased by `grow`.
    pub(crate) fn adjust_bytes(&mut self, shrink: usize, grow: usize) {
        let inner = Arc::make_mut(&mut self.0);
        inner.bytes = (inner.bytes + grow).saturating_sub(shrink);
    }

    /// Removes the pair in slot `i` (makes it a hole; compacts when there are too many holes).
    pub(crate) fn remove_at(&mut self, i: usize) -> (Value, Value) {
        let inner = Arc::make_mut(&mut self.0);
        let Slot { pair: (k, v), hash } = inner.slots[i].take().expect("組がある");
        inner.live -= 1;
        inner.bytes = inner
            .bytes
            .saturating_sub(k.approx_bytes() + v.approx_bytes());
        inner
            .index
            .find_entry(hash, |&j| j == i)
            .expect("索引にある")
            .remove();
        while matches!(inner.slots.last(), Some(None)) {
            inner.slots.pop();
        }
        if inner.slots.len() - inner.live > inner.live.max(COMPACT_MIN) {
            inner.compact();
        }
        (k, v)
    }

    /// Whether it shares the same `Arc` (to confirm that a clone did not copy the contents; for tests).
    #[doc(hidden)]
    pub fn shares_storage_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Empties it.
    pub(crate) fn clear(&mut self) {
        self.0 = Arc::new(MapInner::default());
    }

    /// Whether the index, the order and the total size are consistent (for tests).
    #[doc(hidden)]
    pub fn is_consistent(&self) -> bool {
        let inner = &*self.0;
        let holes = inner.slots.len() - inner.live;
        inner.bytes
            == self
                .entries()
                .map(|(k, v)| k.approx_bytes() + v.approx_bytes())
                .sum::<usize>()
            && inner.slots.iter().flatten().count() == inner.live
            && inner.index.len() == inner.live
            && !matches!(inner.slots.last(), Some(None))
            && holes <= inner.live.max(COMPACT_MIN)
            && inner.slots.iter().enumerate().all(|(i, e)| {
                e.as_ref()
                    .is_none_or(|s| s.hash == hash_of(&s.pair.0) && self.slot(&s.pair.0) == Some(i))
            })
    }
}

/// With fewer holes than this, do not compact even if there are few pairs (so small `map`s do not compact repeatedly).
const COMPACT_MIN: usize = 8;

impl MapInner {
    fn entry(&self, i: usize) -> &(Value, Value) {
        &self.slots[i].as_ref().expect("組がある").pair
    }

    fn find(&self, hash: u64, key: &Value) -> Option<usize> {
        let slots = &self.slots;
        self.index
            .find(hash, |&i| {
                slots[i]
                    .as_ref()
                    .is_some_and(|s| s.hash == hash && key_eq(&s.pair.0, key))
            })
            .copied()
    }

    /// If the key exists, replaces the value; otherwise appends it (also updates the total size).
    fn upsert(&mut self, key: Value, value: Value) {
        let hash = hash_of(&key);
        match self.find(hash, &key) {
            Some(i) => {
                let slot = &mut self.slots[i].as_mut().expect("組がある").pair.1;
                self.bytes =
                    (self.bytes + value.approx_bytes()).saturating_sub(slot.approx_bytes());
                *slot = value;
            }
            None => self.push_entry(hash, key, value),
        }
    }

    /// Appends the pair of a new key to the end (the caller makes sure the key is missing).
    fn push_entry(&mut self, hash: u64, key: Value, value: Value) {
        self.bytes += key.approx_bytes() + value.approx_bytes();
        let slots = &self.slots;
        self.index.insert_unique(hash, slots.len(), |&i| {
            slots[i].as_ref().expect("組がある").hash
        });
        self.slots.push(Some(Slot {
            pair: (key, value),
            hash,
        }));
        self.live += 1;
    }

    /// Compacts the holes and rebuilds the index (keeps the order; uses the stored hashes without rehashing keys).
    fn compact(&mut self) {
        self.slots.retain(Option::is_some);
        self.index.clear();
        let slots = &self.slots;
        for (i, s) in slots.iter().flatten().enumerate() {
            self.index
                .insert_unique(s.hash, i, |&j| slots[j].as_ref().expect("組がある").hash);
        }
    }
}

fn hash_of(key: &Value) -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = rustc_hash::FxBuildHasher.build_hasher();
    hash_key(key, &mut h);
    h.finish()
}

/// The hash of a key. Keys that are equal by [`key_eq`] have the same hash.
fn hash_key<H: std::hash::Hasher>(v: &Value, h: &mut H) {
    use std::hash::Hash;
    match v {
        Value::Int(n) => (0u8, n).hash(h),
        Value::Bool(b) => (1u8, b).hash(h),
        Value::Str(t) => (2u8, &**t).hash(h),
        Value::Identity(i) => (3u8, &i.id, &i.name).hash(h),
        Value::Struct(st) => {
            (4u8, &*st.ty, st.fields.len()).hash(h);
            for (_, f) in st.fields.iter() {
                hash_key(f, h);
            }
        }
        Value::Enum(e) => {
            (5u8, &*e.ty, &*e.variant, e.args.len()).hash(h);
            for a in e.args.iter() {
                hash_key(a, h);
            }
        }
        // Types that cannot be keys (rejected by type checking). Uses the same equivalence as `key_repr`
        other => (6u8, other.key_repr()).hash(h),
    }
}

/// Whether two keys are equal (the same as equal `key_repr`; for `struct`s, field names are not compared).
fn key_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Identity(x), Value::Identity(y)) => x.id == y.id && x.name == y.name,
        (Value::Struct(s1), Value::Struct(s2)) => {
            s1.ty == s2.ty
                && s1.fields.len() == s2.fields.len()
                && s1
                    .fields
                    .iter()
                    .zip(s2.fields.iter())
                    .all(|((_, x), (_, y))| key_eq(x, y))
        }
        (Value::Enum(e1), Value::Enum(e2)) => {
            e1.ty == e2.ty
                && e1.variant == e2.variant
                && e1.args.len() == e2.args.len()
                && e1
                    .args
                    .iter()
                    .zip(e2.args.iter())
                    .all(|(x, y)| key_eq(x, y))
        }
        (
            Value::Int(_)
            | Value::Bool(_)
            | Value::Str(_)
            | Value::Identity(_)
            | Value::Struct(_)
            | Value::Enum(_),
            _,
        )
        | (
            _,
            Value::Int(_)
            | Value::Bool(_)
            | Value::Str(_)
            | Value::Identity(_)
            | Value::Struct(_)
            | Value::Enum(_),
        ) => false,
        _ => a.key_repr() == b.key_repr(),
    }
}

impl<'a> IntoIterator for &'a MapData {
    type Item = &'a (Value, Value);
    type IntoIter = MapEntries<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.entries()
    }
}

impl From<Vec<(Value, Value)>> for MapData {
    fn from(pairs: Vec<(Value, Value)>) -> Self {
        Self::from_pairs(pairs)
    }
}

impl PartialEq for MapData {
    fn eq(&self, other: &Self) -> bool {
        if Arc::ptr_eq(&self.0, &other.0) {
            return true;
        }
        // Insertion order is not compared (keys do not repeat, so they are equal if every pair of one is in the other)
        self.len() == other.len()
            && self
                .iter()
                .all(|(k, v)| other.get(k).is_some_and(|v2| v == v2))
    }
}

impl Serialize for MapData {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.collect_seq(self.entries())
    }
}

impl<'de> Deserialize<'de> for MapData {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Ok(Self::from_pairs(Vec::<(Value, Value)>::deserialize(de)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(m: &MapData) -> Vec<Value> {
        m.entries().map(|(k, _)| k.clone()).collect()
    }

    #[test]
    fn iteration_and_lookup_work_in_insertion_order_after_removal() {
        let mut m = MapData::new();
        for i in 0..10 {
            assert!(m.push_new(Value::Int(i), Value::Int(i * 10)));
        }
        for i in [0, 5, 9, 3] {
            let p = m.slot(&Value::Int(i)).unwrap();
            m.remove_at(p);
            assert!(m.is_consistent());
        }
        assert_eq!(keys(&m), [1, 2, 4, 6, 7, 8].map(Value::Int).to_vec());
        assert_eq!(m.len(), 6);
        assert_eq!(m.entries().len(), 6);
        assert_eq!(m.entries().next_back().map(|e| &e.0), Some(&Value::Int(8)));
        assert_eq!(m.get(&Value::Int(7)), Some(&Value::Int(70)));
        assert!(!m.contains_key(&Value::Int(5)));
        m.push_new(Value::Int(5), Value::Int(0));
        assert_eq!(keys(&m).last(), Some(&Value::Int(5)));
        assert!(m.is_consistent());
    }

    #[test]
    fn compacts_when_holes_grow() {
        let mut m = MapData::new();
        for i in 0..100 {
            m.push_new(Value::Int(i), Value::Int(i));
        }
        for i in 0..90 {
            let p = m.slot(&Value::Int(i)).unwrap();
            m.remove_at(p);
            assert!(m.is_consistent(), "{i}");
        }
        assert!(m.0.slots.len() <= 10 + COMPACT_MIN);
        assert_eq!(keys(&m), (90..100).map(Value::Int).collect::<Vec<_>>());
        for i in 90..100 {
            let p = m.slot(&Value::Int(i)).unwrap();
            m.remove_at(p);
        }
        assert!(m.is_empty() && m.0.slots.is_empty() && m.is_consistent());
    }

    #[test]
    fn removing_from_shared_clone_does_not_affect_original() {
        let mut a = MapData::from_pairs((0..20).map(|i| (Value::Int(i), Value::Int(i))).collect());
        let b = a.clone();
        let p = a.slot(&Value::Int(3)).unwrap();
        a.remove_at(p);
        assert_eq!(b.len(), 20);
        assert!(b.contains_key(&Value::Int(3)) && !a.contains_key(&Value::Int(3)));
        assert!(a.is_consistent() && b.is_consistent());
    }

    #[test]
    fn key_equality_matches_string_representation() {
        let s = |t: &str| Value::Str(t.into());
        let st = |name: &str, v: i64| {
            Value::Struct(std::sync::Arc::new(StructValue {
                ty: "点".into(),
                fields: vec![(name.into(), Value::Int(v))],
            }))
        };
        let vals = [
            Value::Int(1),
            Value::Bool(true),
            s("1"),
            s("i1;"),
            st("x", 1),
            st("y", 1),
            st("x", 2),
            Value::Float(1.0),
        ];
        for a in &vals {
            for b in &vals {
                let same = a.key_repr() == b.key_repr();
                assert_eq!(key_eq(a, b), same, "{a:?} と {b:?}");
                if same {
                    assert_eq!(hash_of(a), hash_of(b), "{a:?} と {b:?}");
                }
            }
        }
    }
}
