use serde::{Deserialize, Serialize};

/// Kind of user-defined type at runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeKind {
    Struct,
    Record,
}

/// Runtime metadata for a tagged type — used by Eldr for display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeEntry {
    pub tag: u32,
    pub name: String,
    pub kind: TypeKind,
    pub field_names: Vec<String>,
}

/// Registry of all user-defined types in a compiled program.
/// Forge builds this; Eldr reads it for `to_display_string`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TypeRegistry {
    pub entries: Vec<TypeEntry>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Register a new type entry.
    pub fn register(&mut self, entry: TypeEntry) {
        self.entries.push(entry);
    }

    /// Look up a type entry by tag.
    pub fn lookup(&self, tag: u32) -> Option<&TypeEntry> {
        self.entries.iter().find(|e| e.tag == tag)
    }
}
