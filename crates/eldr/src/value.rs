use forge::registry::TypeRegistry;

/// Runtime value in the Surtr VM.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct Callable {
    pub target: CallableTarget,
    pub captured: Vec<Value>,
}

/// Callable target reference.
#[derive(Debug, Clone)]
pub enum CallableTarget {
    Builtin(u16),
    Function(u32),
}

impl Value {
    /// Display string for `to_string` built-in.
    /// Uses TypeRegistry for struct/record formatting.
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
                let inner: Vec<String> = items
                    .iter()
                    .map(|v| v.to_display_string(registry))
                    .collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Tagged { tag, fields } => {
                if let Some(entry) = registry.lookup(*tag) {
                    match entry.kind {
                        forge::registry::TypeKind::Struct => {
                            let pairs: Vec<String> = entry
                                .field_names
                                .iter()
                                .zip(fields.iter())
                                .map(|(name, val)| {
                                    format!("{}: {}", name, val.to_display_string(registry))
                                })
                                .collect();
                            format!("{} {{ {} }}", entry.name, pairs.join(", "))
                        }
                        forge::registry::TypeKind::Record => {
                            let pairs: Vec<String> = entry
                                .field_names
                                .iter()
                                .zip(fields.iter())
                                .map(|(name, val)| {
                                    format!("{}: {}", name, val.to_display_string(registry))
                                })
                                .collect();
                            format!("{}({})", entry.name, pairs.join(", "))
                        }
                    }
                } else {
                    // Fallback for Ok/Err (tag 0/1) without registry entry
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
            Value::Callable(callable) => match &callable.target {
                CallableTarget::Builtin(id) => format!("<builtin:{}>", id),
                CallableTarget::Function(fun_idx) => {
                    format!("<function:{}; captured={}>", fun_idx, callable.captured.len())
                }
            },
            Value::Error(rich) => {
                format!("{}", rich.message)
            }
        }
    }
}

/// Rich error value — produced by `deferror`.
#[derive(Debug, Clone)]
pub struct RichError {
    pub kind: String,
    pub message: String,
    pub location: Location,
}

/// Source location for error reporting.
#[derive(Debug, Clone)]
pub struct Location {
    pub file: String,
    pub func: String,
    pub line: u32,
    pub column: u32,
    pub span_start: u32,
    pub span_end: u32,
}
