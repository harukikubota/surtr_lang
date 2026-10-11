use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use spire::ast::Span;

/// Lexical scope — maps names to unique IDs.
#[derive(Debug, Clone)]
pub struct Scope {
    bindings: Arc<HashMap<String, u32>>,
    parent: Option<Arc<Scope>>,
    next_id: u32,
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

impl Scope {
    pub fn new() -> Self {
        Self {
            bindings: Arc::new(HashMap::new()),
            parent: None,
            next_id: 0,
        }
    }

    /// Start IDs from a given offset (e.g. after builtin registration).
    pub fn with_next_id(next_id: u32) -> Self {
        Self {
            bindings: Arc::new(HashMap::new()),
            parent: None,
            next_id,
        }
    }

    /// Start a lexical child with local writes over an immutable parent snapshot.
    pub(crate) fn child(&self) -> Self {
        if self.bindings.is_empty() {
            return self.clone();
        }
        Self {
            bindings: Arc::new(HashMap::new()),
            parent: Some(Arc::new(self.clone())),
            next_id: self.next_id,
        }
    }

    /// Define a new binding, returning its unique_id.
    /// Shadowing: the old binding is replaced.
    pub fn define(&mut self, name: &str, _span: Span) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        Arc::make_mut(&mut self.bindings).insert(name.to_string(), id);
        id
    }

    /// Reserve the next unique id without binding a name yet.
    pub fn reserve_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Bind a name to a pre-reserved id.
    pub fn define_with_id(&mut self, name: &str, id: u32) {
        Arc::make_mut(&mut self.bindings).insert(name.to_string(), id);
        if self.next_id <= id {
            self.next_id = id + 1;
        }
    }

    /// Advance the next id if the provided value is larger.
    pub fn advance_next_id_to(&mut self, next_id: u32) {
        if self.next_id < next_id {
            self.next_id = next_id;
        }
    }

    /// Look up a name, returning its unique_id if found.
    pub fn lookup(&self, name: &str) -> Option<u32> {
        std::iter::successors(Some(self), |scope| scope.parent.as_deref())
            .find_map(|scope| scope.bindings.get(name).copied())
    }

    pub fn bindings(&self) -> impl Iterator<Item = (&str, u32)> {
        let mut seen = HashSet::new();
        std::iter::successors(Some(self), |scope| scope.parent.as_deref())
            .flat_map(|scope| scope.bindings.iter())
            .map(|(name, uid)| (name.as_str(), *uid))
            .filter(move |(name, _)| seen.insert(*name))
    }

    /// Current next_id value (for chaining scopes).
    pub fn next_id(&self) -> u32 {
        self.next_id
    }
}

#[cfg(test)]
mod child_scope_tests {
    use super::*;

    #[test]
    fn snapshots_and_nested_scopes_keep_shadowing_local() {
        let mut root = Scope::new();
        root.define_with_id("name", 1);
        root.define_with_id("root_only", 2);
        let snapshot = root.clone();
        root.define_with_id("name", 3);
        assert_eq!(snapshot.lookup("name"), Some(1));
        let mut child = root.child();
        child.define_with_id("name", 4);
        child.define_with_id("child_only", 5);
        let mut nested = child.child();
        nested.define_with_id("name", 6);
        assert_eq!(nested.lookup("root_only"), Some(2));
        assert_eq!(nested.lookup("child_only"), Some(5));
        assert_eq!(nested.lookup("name"), Some(6));
        assert_eq!(child.lookup("name"), Some(4));
        assert_eq!(root.lookup("name"), Some(3));
        assert_eq!(root.lookup("child_only"), None);
        let bindings = nested.bindings().collect::<HashMap<_, _>>();
        assert_eq!(bindings.len(), 3);
        assert_eq!(nested.bindings().count(), 3);
        assert_eq!(bindings.get("name"), Some(&6));
        let empty_child = nested.child().child();
        assert_eq!(empty_child.lookup("name"), Some(6));
        assert_eq!(empty_child.next_id(), 7);
    }
}
