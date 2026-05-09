use std::collections::HashMap;

use spire::ast::Span;

/// Lexical scope — maps names to unique IDs.
#[derive(Debug, Clone)]
pub struct Scope {
    bindings: HashMap<String, u32>,
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
            bindings: HashMap::new(),
            next_id: 0,
        }
    }

    /// Start IDs from a given offset (e.g. after builtin registration).
    pub fn with_next_id(next_id: u32) -> Self {
        Self {
            bindings: HashMap::new(),
            next_id,
        }
    }

    /// Define a new binding, returning its unique_id.
    /// Shadowing: the old binding is replaced.
    pub fn define(&mut self, name: &str, _span: Span) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.bindings.insert(name.to_string(), id);
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
        self.bindings.insert(name.to_string(), id);
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
        self.bindings.get(name).copied()
    }

    pub fn bindings(&self) -> impl Iterator<Item = (&str, u32)> {
        self.bindings
            .iter()
            .map(|(name, uid)| (name.as_str(), *uid))
    }

    /// Current next_id value (for chaining scopes).
    pub fn next_id(&self) -> u32 {
        self.next_id
    }
}
