use serde::{Deserialize, Serialize};

/// Kind of user-defined type at runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeKind {
    Struct,
    Record,
}

/// Runtime metadata for a tagged type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeEntry {
    pub tag: u32,
    pub name: String,
    pub kind: TypeKind,
    pub field_names: Vec<String>,
}

/// Registry of all user-defined types in a compiled program.
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

    pub fn register(&mut self, entry: TypeEntry) {
        self.entries.push(entry);
    }

    pub fn lookup(&self, tag: u32) -> Option<&TypeEntry> {
        self.entries.iter().find(|entry| entry.tag == tag)
    }
}

/// Runtime value in the Surtr VM.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Unit,
    List(Vec<Value>),
    Tagged { tag: u32, fields: Vec<Value> },
    Callable(Callable),
    Error(Box<RichError>),
}

/// Callable runtime value.
#[derive(Debug, Clone, PartialEq)]
pub struct Callable {
    pub target: CallableTarget,
    pub captured: Vec<Value>,
}

/// Callable target reference.
#[derive(Debug, Clone, PartialEq)]
pub enum CallableTarget {
    Builtin(u16),
    Function(u32),
}

impl Value {
    /// Display string for `to_string` built-in.
    pub fn to_display_string(&self, registry: &TypeRegistry) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Float(f) => {
                let s = format!("{}", f);
                if s.contains('.') {
                    s
                } else {
                    format!("{}.0", s)
                }
            }
            Value::Str(s) => s.clone(),
            Value::Bool(b) => {
                if *b {
                    "True".to_string()
                } else {
                    "False".to_string()
                }
            }
            Value::Unit => "()".to_string(),
            Value::List(items) => {
                let inner = items
                    .iter()
                    .map(|item| item.to_display_string(registry))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{}]", inner)
            }
            Value::Tagged { tag, fields } => {
                if let Some(entry) = registry.lookup(*tag) {
                    let pairs = entry
                        .field_names
                        .iter()
                        .zip(fields.iter())
                        .map(|(name, val)| format!("{}: {}", name, val.to_display_string(registry)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    match entry.kind {
                        TypeKind::Struct => format!("{} {{ {} }}", entry.name, pairs),
                        TypeKind::Record => format!("{}({})", entry.name, pairs),
                    }
                } else {
                    // Fallback for reserved tags and unknown runtime tags.
                    match *tag {
                        0 => format!(
                            "Ok({})",
                            fields
                                .first()
                                .map(|v| v.to_display_string(registry))
                                .unwrap_or_default()
                        ),
                        1 => format!(
                            "Err({})",
                            fields
                                .first()
                                .map(|v| v.to_display_string(registry))
                                .unwrap_or_default()
                        ),
                        _ => format!("Tagged({}, {:?})", tag, fields),
                    }
                }
            }
            Value::Callable(callable) => match callable.target {
                CallableTarget::Builtin(id) => format!("<builtin:{}>", id),
                CallableTarget::Function(fun_idx) => {
                    format!(
                        "<function:{}; captured={}>",
                        fun_idx,
                        callable.captured.len()
                    )
                }
            },
            Value::Error(rich) => rich.message.clone(),
        }
    }
}

/// Rich error value produced by `deferror`.
#[derive(Debug, Clone, PartialEq)]
pub struct RichError {
    pub kind: String,
    pub message: String,
    pub location: Location,
}

/// Source location for error reporting.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub file: String,
    pub func: String,
    pub line: u32,
    pub column: u32,
    pub span_start: u32,
    pub span_end: u32,
}

#[cfg(test)]
mod tests {
    use super::{Location, RichError, TypeEntry, TypeKind, TypeRegistry, Value};

    #[test]
    fn display_for_reserved_result_tags() {
        let registry = TypeRegistry::new();
        let ok = Value::Tagged {
            tag: 0,
            fields: vec![Value::Int(42)],
        };
        let err = Value::Tagged {
            tag: 1,
            fields: vec![Value::Str("bad".into())],
        };
        assert_eq!(ok.to_display_string(&registry), "Ok(42)");
        assert_eq!(err.to_display_string(&registry), "Err(bad)");
    }

    #[test]
    fn display_for_registered_struct_and_record() {
        let mut registry = TypeRegistry::new();
        registry.register(TypeEntry {
            tag: 10,
            name: "User".into(),
            kind: TypeKind::Struct,
            field_names: vec!["name".into(), "age".into()],
        });
        registry.register(TypeEntry {
            tag: 11,
            name: "Pair".into(),
            kind: TypeKind::Record,
            field_names: vec!["left".into(), "right".into()],
        });

        let user = Value::Tagged {
            tag: 10,
            fields: vec![Value::Str("alice".into()), Value::Int(20)],
        };
        let pair = Value::Tagged {
            tag: 11,
            fields: vec![Value::Int(1), Value::Int(2)],
        };

        assert_eq!(
            user.to_display_string(&registry),
            "User { name: alice, age: 20 }"
        );
        assert_eq!(pair.to_display_string(&registry), "Pair(left: 1, right: 2)");
    }

    #[test]
    fn display_for_rich_error_uses_message() {
        let registry = TypeRegistry::new();
        let value = Value::Error(Box::new(RichError {
            kind: "TestError".into(),
            message: "boom".into(),
            location: Location {
                file: "<repl>".into(),
                func: "f".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 1,
            },
        }));
        assert_eq!(value.to_display_string(&registry), "boom");
    }
}
