use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

/// A checkpoint shares an owned container. Mutable access detaches it so that
/// saved state cannot change. Replacing the wrapper does not copy the old value.
#[derive(Debug)]
pub(super) struct CheckpointValue<T>(Rc<T>);

impl<T> Clone for CheckpointValue<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T: Default> Default for CheckpointValue<T> {
    fn default() -> Self {
        Self::from(T::default())
    }
}

impl<T> From<T> for CheckpointValue<T> {
    fn from(value: T) -> Self {
        Self(Rc::new(value))
    }
}

impl<A, T: FromIterator<A>> FromIterator<A> for CheckpointValue<T> {
    fn from_iter<I: IntoIterator<Item = A>>(iter: I) -> Self {
        Self::from(T::from_iter(iter))
    }
}

impl<T> Deref for CheckpointValue<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: Clone> DerefMut for CheckpointValue<T> {
    fn deref_mut(&mut self) -> &mut T {
        Rc::make_mut(&mut self.0)
    }
}

impl<K: Ord + Clone, V: Clone> CheckpointValue<BTreeMap<K, V>> {
    pub(super) fn get_mut<Q: Ord + ?Sized>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
    {
        if Rc::strong_count(&self.0) > 1 && !self.contains_key(key) {
            return None;
        }
        Rc::make_mut(&mut self.0).get_mut(key)
    }

    pub(super) fn remove<Q: Ord + ?Sized>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
    {
        if Rc::strong_count(&self.0) > 1 && !self.contains_key(key) {
            return None;
        }
        Rc::make_mut(&mut self.0).remove(key)
    }
}

impl<T: Ord + Clone> CheckpointValue<BTreeSet<T>> {
    pub(super) fn remove<Q: Ord + ?Sized>(&mut self, value: &Q) -> bool
    where
        T: Borrow<Q>,
    {
        if Rc::strong_count(&self.0) > 1 && !self.contains(value) {
            return false;
        }
        Rc::make_mut(&mut self.0).remove(value)
    }
}

impl<T: Clone> CheckpointValue<VecDeque<T>> {
    pub(super) fn pop_front(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        Rc::make_mut(&mut self.0).pop_front()
    }
}
