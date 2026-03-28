use std::collections::HashMap;

use spire::ast::Span;

/// Lexical scope — maps names to unique IDs.
#[derive(Debug, Clone)]
pub struct Scope {
    bindings: HashMap<String, u32>,
    next_id: u32,
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

    /// Look up a name, returning its unique_id if found.
    pub fn lookup(&self, name: &str) -> Option<u32> {
        self.bindings.get(name).copied()
    }

    /// Current next_id value (for chaining scopes).
    pub fn next_id(&self) -> u32 {
        self.next_id
    }
}
