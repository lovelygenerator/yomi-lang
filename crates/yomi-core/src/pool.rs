//! Tables indexed by position (arenas) and typed indices.

use std::collections::HashMap;
use std::fmt;
use std::hash::{BuildHasher, Hash, RandomState};
use std::marker::PhantomData;

/// An index pointing to an element of a [`Pool<T>`]. Mixing it up with an index of a table of another type is a compile
/// error.
pub(crate) struct Idx<T> {
    raw: u32,
    _ty: PhantomData<fn() -> T>,
}

impl<T> Idx<T> {
    fn new(raw: u32) -> Self {
        Self {
            raw,
            _ty: PhantomData,
        }
    }

    /// The value of the index.
    pub(crate) fn index(self) -> usize {
        self.raw as usize
    }
}

// Implemented by hand so that `T` does not need `Clone` and so on
impl<T> Clone for Idx<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Idx<T> {}

impl<T> PartialEq for Idx<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for Idx<T> {}

impl<T> std::hash::Hash for Idx<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl<T> fmt::Debug for Idx<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.raw)
    }
}

/// A table that returns an [`Idx<T>`] when an element is added. Elements are never removed.
#[derive(Clone)]
pub(crate) struct Pool<T> {
    items: Vec<T>,
    /// An index from the hash of a value to its position, for elements added with `intern` (only tables that use `intern`
    /// have it).
    index: HashMap<u64, Vec<u32>>,
    hasher: RandomState,
}

// The index is an auxiliary that can be rebuilt from the elements, so it is not compared
impl<T: PartialEq> PartialEq for Pool<T> {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
    }
}

impl<T> Pool<T> {
    pub(crate) fn new() -> Self {
        Self {
            items: Vec::new(),
            index: HashMap::new(),
            hasher: RandomState::new(),
        }
    }

    /// If an equal value exists, returns its index. Otherwise, appends it (merges equal values into one). Searches among the
    /// elements added with `intern` (does not merge with elements added with `push`).
    pub(crate) fn intern(&mut self, item: T) -> Idx<T>
    where
        T: PartialEq + Hash,
    {
        let h = self.hasher.hash_one(&item);
        if let Some(found) = self
            .index
            .get(&h)
            .and_then(|is| is.iter().copied().find(|&i| self.items[i as usize] == item))
        {
            return Idx::new(found);
        }
        let idx = self.push(item);
        self.index.entry(h).or_default().push(idx.raw);
        idx
    }

    /// Appends and returns its index.
    pub(crate) fn push(&mut self, item: T) -> Idx<T> {
        let raw = u32::try_from(self.items.len()).expect("表の要素は u32 に収まる");
        self.items.push(item);
        Idx::new(raw)
    }
}

impl<T> Default for Pool<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> std::ops::Index<Idx<T>> for Pool<T> {
    type Output = T;

    fn index(&self, i: Idx<T>) -> &T {
        &self.items[i.index()]
    }
}

impl<T> std::ops::IndexMut<Idx<T>> for Pool<T> {
    fn index_mut(&mut self, i: Idx<T>) -> &mut T {
        &mut self.items[i.index()]
    }
}

impl<T: fmt::Debug> fmt::Debug for Pool<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.items).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices_follow_insertion_order_and_lookup_by_index() {
        let mut p: Pool<&str> = Pool::new();
        let a = p.push("a");
        let b = p.push("b");
        assert_eq!((a.index(), b.index()), (0, 1));
        assert_eq!((p[a], p[b]), ("a", "b"));
        assert_ne!(a, b);
    }

    #[test]
    fn intern_merges_equal_values_into_same_index() {
        let mut p: Pool<String> = Pool::new();
        let a = p.intern("x".to_string());
        let b = p.intern("y".to_string());
        let c = p.intern("x".to_string());
        assert_eq!(a, c);
        assert_ne!(a, b);
        assert_eq!(p[b], "y");
    }
}
