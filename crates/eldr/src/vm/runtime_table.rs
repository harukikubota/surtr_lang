use std::collections::BTreeMap;
use std::rc::Rc;

/// Checkpoints share the table and its entries. A mutation copies the index,
/// then only the entry whose owned state is about to change.
#[derive(Debug, Clone)]
pub(super) struct RuntimeTable<T> {
    entries: Rc<BTreeMap<u64, Rc<T>>>,
}

impl<T> Default for RuntimeTable<T> {
    fn default() -> Self {
        Self {
            entries: Rc::new(BTreeMap::new()),
        }
    }
}

impl<T> RuntimeTable<T> {
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(test)]
    pub(super) fn contains_key(&self, id: &u64) -> bool {
        self.entries.contains_key(id)
    }

    pub(super) fn get(&self, id: &u64) -> Option<&T> {
        self.entries.get(id).map(Rc::as_ref)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&u64, &T)> {
        self.entries.iter().map(|(id, entry)| (id, entry.as_ref()))
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &T> {
        self.entries.values().map(Rc::as_ref)
    }

    pub(super) fn insert(&mut self, id: u64, value: T) {
        Rc::make_mut(&mut self.entries).insert(id, Rc::new(value));
    }

    pub(super) fn remove(&mut self, id: &u64) {
        if self.entries.contains_key(id) {
            Rc::make_mut(&mut self.entries).remove(id);
        }
    }
}

impl<T: Clone> RuntimeTable<T> {
    pub(super) fn get_mut(&mut self, id: &u64) -> Option<&mut T> {
        if !self.entries.contains_key(id) {
            return None;
        }
        Rc::make_mut(&mut self.entries)
            .get_mut(id)
            .map(Rc::make_mut)
    }

    pub(super) fn take(&mut self, id: &u64) -> Option<T> {
        if !self.entries.contains_key(id) {
            return None;
        }
        Rc::make_mut(&mut self.entries)
            .remove(id)
            .map(Rc::unwrap_or_clone)
    }
}

#[cfg(test)]
impl<T> std::ops::Index<&u64> for RuntimeTable<T> {
    type Output = T;

    fn index(&self, id: &u64) -> &Self::Output {
        self.entries[id].as_ref()
    }
}
