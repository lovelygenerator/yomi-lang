//! The contents of a `list` (`ListData`). Sharing, copy on write, and incremental tracking of the total size.

use super::*;

/// The contents of a `list`. Holds the elements in an `Arc`, so cloning only shares them (the same approach as
/// [`MapData`]).
///
/// - Cloning is O(1). The first update after cloning copies the contents if shared (copy on write). A clone is
///   independent of the original (value semantics; updating one does not change the other).
/// - If not shared, `push`, `insert`, `remove` and replacing elements are done in place (`push` is amortized O(1)).
/// - Keeps the total size of the elements (the estimate of [`Value::approx_bytes`]) incrementally, so checking value
///   sizes is not proportional to the length of the `list`. When a value inside nesting is updated, the outer side
///   reports it with `adjust_bytes` (within the crate).
/// - Can be read as `[Value]` (`Deref`). Converts from `Vec<Value>` with `From`, and to `Vec<Value>` with
///   [`ListData::into_vec`]. `Debug` and serialization are the same as `Vec<Value>` (the save format does not change).
#[derive(Clone, Default)]
pub struct ListData(Arc<ListInner>);

/// The contents of a `list` (elements and the total size).
#[derive(Clone, Default)]
pub(super) struct ListInner {
    items: Vec<Value>,
    /// The total size of the elements (not including the 24 of the `list` itself).
    bytes: usize,
}

/// Shows only the sequence of elements (`[…]`), the same as `Vec<Value>` (the output does not change).
impl std::fmt::Debug for ListData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.items.fmt(f)
    }
}

/// Only when the length is at most this, recount on every update to check that the total size is correct (with
/// `debug_assertions`; so that debug tests do not take quadratic time with large `list`s).
pub(super) const CONSISTENCY_CHECK_MAX_LEN: usize = 256;

impl ListData {
    /// An empty `list`.
    pub fn new() -> Self {
        Self::default()
    }

    /// The elements.
    pub fn as_slice(&self) -> &[Value] {
        &self.0.items
    }

    /// Takes the elements out. Does not copy if not shared.
    pub fn into_vec(self) -> Vec<Value> {
        match Arc::try_unwrap(self.0) {
            Ok(inner) => inner.items,
            Err(shared) => shared.items.clone(),
        }
    }

    /// Appends to the end (copies the contents first if shared).
    pub fn push(&mut self, value: Value) {
        let inner = Arc::make_mut(&mut self.0);
        inner.bytes += value.approx_bytes();
        inner.items.push(value);
        inner.debug_check();
    }

    /// Inserts at position `at` (`at <= len()`; shifts the rest; proportional to the number of elements).
    pub fn insert(&mut self, at: usize, value: Value) {
        let inner = Arc::make_mut(&mut self.0);
        inner.bytes += value.approx_bytes();
        inner.items.insert(at, value);
        inner.debug_check();
    }

    /// Removes and returns the element at position `at` (`at < len()`; shifts the rest; proportional to the number of
    /// elements).
    pub fn remove(&mut self, at: usize) -> Value {
        let inner = Arc::make_mut(&mut self.0);
        let v = inner.items.remove(at);
        inner.bytes = inner.bytes.saturating_sub(v.approx_bytes());
        inner.debug_check();
        v
    }

    /// Swaps the elements at positions `a` and `b` (copies the contents first if shared; the total size is unchanged).
    pub fn swap(&mut self, a: usize, b: usize) {
        if a != b {
            Arc::make_mut(&mut self.0).items.swap(a, b);
        }
    }

    /// Empties it.
    pub fn clear(&mut self) {
        self.0 = Arc::new(ListInner::default());
    }

    /// The total size of the elements (not including the 24 of the `list` itself).
    pub(crate) fn content_bytes(&self) -> usize {
        self.0.bytes
    }

    /// Returns the element at position `i` in a mutable form (copies the contents first if shared). After modifying the
    /// element, report the change in size with [`ListData::adjust_bytes`].
    pub(crate) fn item_mut(&mut self, i: usize) -> Option<&mut Value> {
        if i >= self.0.items.len() {
            return None;
        }
        Arc::make_mut(&mut self.0).items.get_mut(i)
    }

    /// After modifying elements (including inside nesting), corrects the total size as having decreased by `shrink` and
    /// increased by `grow`.
    pub(crate) fn adjust_bytes(&mut self, shrink: usize, grow: usize) {
        let inner = Arc::make_mut(&mut self.0);
        inner.bytes = (inner.bytes + grow).saturating_sub(shrink);
        inner.debug_check();
    }

    /// Whether the total size matches the value recounted from the elements (for tests).
    #[doc(hidden)]
    pub fn is_consistent(&self) -> bool {
        self.0.bytes == self.0.recount()
    }

    /// Whether it shares the same `Arc` (to confirm that a clone did not copy the contents; for tests).
    #[doc(hidden)]
    pub fn shares_storage_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl ListInner {
    /// Recounts the sizes of the elements.
    fn recount(&self) -> usize {
        self.items.iter().map(Value::approx_bytes).sum()
    }

    /// Checks the consistency of the total size (in debug, only for short `list`s).
    fn debug_check(&self) {
        debug_assert!(
            self.items.len() > CONSISTENCY_CHECK_MAX_LEN || self.bytes == self.recount(),
            "list の大きさの合計が合っていない（実装の不具合）"
        );
    }
}

impl std::ops::Deref for ListData {
    type Target = [Value];
    fn deref(&self) -> &Self::Target {
        &self.0.items
    }
}

impl AsRef<[Value]> for ListData {
    fn as_ref(&self) -> &[Value] {
        &self.0.items
    }
}

impl<'a> IntoIterator for &'a ListData {
    type Item = &'a Value;
    type IntoIter = std::slice::Iter<'a, Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.items.iter()
    }
}

impl IntoIterator for ListData {
    type Item = Value;
    type IntoIter = std::vec::IntoIter<Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl ListData {
    /// The sequence of integers `lo..hi` (`range`). The total size counts 8 for each (values are not recounted).
    pub(crate) fn range(lo: i64, hi: i64) -> Self {
        let items: Vec<Value> = (lo..hi).map(Value::Int).collect();
        let bytes = items.len() * 8;
        Self(Arc::new(ListInner { items, bytes }))
    }
}

impl From<Vec<Value>> for ListData {
    fn from(items: Vec<Value>) -> Self {
        let bytes = items.iter().map(Value::approx_bytes).sum();
        Self(Arc::new(ListInner { items, bytes }))
    }
}

impl From<ListData> for Vec<Value> {
    fn from(list: ListData) -> Self {
        list.into_vec()
    }
}

impl FromIterator<Value> for ListData {
    fn from_iter<I: IntoIterator<Item = Value>>(iter: I) -> Self {
        iter.into_iter().collect::<Vec<_>>().into()
    }
}

impl PartialEq for ListData {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.items == other.0.items
    }
}

impl PartialEq<Vec<Value>> for ListData {
    fn eq(&self, other: &Vec<Value>) -> bool {
        self.0.items == *other
    }
}

impl Serialize for ListData {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.0.items.serialize(ser)
    }
}

impl<'de> Deserialize<'de> for ListData {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Ok(Vec::<Value>::deserialize(de)?.into())
    }
}
