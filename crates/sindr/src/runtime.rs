use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::rc::Rc;

use crate::names::surface_path_name;
use crate::primitives::{BuiltinId, FunctionId, RuntimeTag, SurtrInt};

/// Kind of user-defined type at runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeKind {
    Struct,
    Record,
    EnumVariant,
}

/// Runtime metadata for a tagged type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeEntry {
    pub tag: RuntimeTag,
    pub name: String,
    pub kind: TypeKind,
    pub field_names: Vec<String>,
    #[serde(default)]
    pub private_flags: Vec<bool>,
}

/// Registry of all user-defined types in a compiled program.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeRegistry {
    entries: Vec<TypeEntry>,
    tag_to_index: HashMap<RuntimeTag, usize>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            tag_to_index: HashMap::new(),
        }
    }

    pub fn from_entries(entries: Vec<TypeEntry>) -> Self {
        let mut registry = Self::new();
        registry.extend(entries);
        registry
    }

    pub fn register(&mut self, entry: TypeEntry) {
        self.tag_to_index.insert(entry.tag, self.entries.len());
        self.entries.push(entry);
    }

    pub fn extend<I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = TypeEntry>,
    {
        for entry in entries {
            self.register(entry);
        }
    }

    pub fn entries(&self) -> &[TypeEntry] {
        &self.entries
    }

    pub fn truncate(&mut self, len: usize) {
        self.entries.truncate(len);
        self.tag_to_index = self
            .entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| (entry.tag, idx))
            .collect();
    }

    pub fn lookup(&self, tag: RuntimeTag) -> Option<&TypeEntry> {
        self.tag_to_index
            .get(&tag)
            .and_then(|idx| self.entries.get(*idx))
    }

    pub fn lookup_by_name(&self, qualified_name: &str) -> Option<&TypeEntry> {
        let surface_query = surface_path_name(qualified_name);
        self.entries.iter().find(|entry| {
            entry.name == qualified_name || surface_path_name(&entry.name) == surface_query
        })
    }

    pub fn tag_by_name(&self, qualified_name: &str) -> Option<RuntimeTag> {
        self.lookup_by_name(qualified_name).map(|entry| entry.tag)
    }
}

impl Default for TypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Serialize for TypeRegistry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.entries.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TypeRegistry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries = Vec::<TypeEntry>::deserialize(deserializer)?;
        Ok(Self::from_entries(entries))
    }
}

/// Runtime value in the Surtr VM.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(SurtrInt),
    Tag(RuntimeTag),
    Float(f64),
    Str(String),
    Bool(bool),
    Unit,
    List(ListHandle),
    HashMap(HashMapHandle),
    Tuple(Vec<Value>),
    Tagged { tag: u32, fields: Vec<Value> },
    Callable(Callable),
    Error(Box<RichError>),
    Regex(RegexHandle),
    RegexCaptures(RegexCapturesHandle),
    RegexMatch(RegexMatchHandle),
    RandomGenerator(RandomGeneratorHandle),
    Pid(PidHandle),
    FileHandle(FileHandleValue),
    Workers(WorkersHandle),
    WorkerLease(WorkerLeaseHandle),
    TaskHandle(u64),
    PendingFuture(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegexHandle {
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegexCapturesHandle {
    pub input: String,
    pub groups: Vec<Option<(usize, usize)>>,
    pub name_to_index: HashMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegexMatchHandle {
    pub input: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomGeneratorHandle {
    pub state: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PidHandle {
    pub id: u64,
    pub process_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHandleValue {
    pub id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkersHandle {
    pub id: u64,
    pub process_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerLeaseHandle {
    pub workers_id: u64,
    pub pid: PidHandle,
}

/// Shared runtime handle for immutable string-keyed maps.
#[derive(Debug, Clone, PartialEq)]
pub struct HashMapHandle {
    pub entries: HashMap<String, Value>,
}

pub type ListRef = Option<Rc<ListNode>>;

/// Shared runtime handle for persistent cons-list values.
///
/// Invariant:
/// - `len == 0` if and only if `head == None`
/// - `len > 0` if and only if `head` points at the first cons cell
#[derive(Debug, Clone, PartialEq)]
pub struct ListHandle {
    pub head: ListRef,
    pub len: usize,
}

/// Persistent list node for O(1) cons/uncons sharing.
#[derive(Debug, Clone, PartialEq)]
pub struct ListNode {
    pub value: Value,
    pub next: ListRef,
}

/// Callable runtime value.
#[derive(Debug, Clone, PartialEq)]
pub struct Callable {
    pub target: CallableTarget,
    pub lexical_captures: Vec<Value>,
    pub metadata: CallableMetadata,
}

/// Callable target reference.
#[derive(Debug, Clone, PartialEq)]
pub enum CallableTarget {
    Builtin(BuiltinId),
    Function(FunctionId),
    Template(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallableMetadata {
    pub origin: CallableOrigin,
    pub module: Option<String>,
    pub name: Option<String>,
    pub full_signature: Option<String>,
    pub applied_args: usize,
}

impl Default for CallableMetadata {
    fn default() -> Self {
        Self {
            origin: CallableOrigin::Unknown,
            module: None,
            name: None,
            full_signature: None,
            applied_args: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallableOrigin {
    Unknown,
    Closure,
    Capture,
}

impl Value {
    fn render_named_value(
        type_name: &str,
        field_names: &[String],
        private_flags: &[bool],
        fields: &[Value],
        registry: &TypeRegistry,
    ) -> String {
        let hidden_field_count = private_flags.iter().filter(|flag| **flag).count();
        let mut parts = field_names
            .iter()
            .zip(
                private_flags
                    .iter()
                    .copied()
                    .chain(std::iter::repeat(false)),
            )
            .zip(fields.iter())
            .filter_map(|((name, is_private), val)| {
                (!is_private).then(|| format!("{}: {}", name, val.to_display_string(registry)))
            })
            .collect::<Vec<_>>();

        if hidden_field_count > 0 {
            parts.push("..private".to_string());
        }

        format!("{}({})", surface_path_name(type_name), parts.join(", "))
    }

    /// Display string for `to_string` built-in.
    pub fn to_display_string(&self, registry: &TypeRegistry) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Tag(tag) => format!("<tag:{}>", tag),
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
            Value::List(handle) => {
                let inner = handle
                    .iter()
                    .map(|item| item.to_display_string(registry))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{}]", inner)
            }
            Value::HashMap(handle) => {
                if handle.entries.is_empty() {
                    return "HashMap()".to_string();
                }
                let inner = handle
                    .sorted_entries()
                    .into_iter()
                    .map(|(key, value)| {
                        format!(
                            "{} => {}",
                            quote_surtr_string_literal(&key),
                            value.to_display_string(registry)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("HashMap({inner})")
            }
            Value::Tuple(items) => {
                let inner = items
                    .iter()
                    .map(|item| item.to_display_string(registry))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({inner})")
            }
            Value::Tagged { tag, fields } => {
                if let Some(entry) = registry.lookup(*tag) {
                    if surface_path_name(&entry.name) == "Duration" {
                        if let Some(Value::Int(ms)) = fields.first() {
                            return format!("{ms}ms");
                        }
                    }
                    match entry.kind {
                        TypeKind::Struct | TypeKind::Record => Self::render_named_value(
                            &entry.name,
                            &entry.field_names,
                            &entry.private_flags,
                            fields,
                            registry,
                        ),
                        TypeKind::EnumVariant => {
                            let payload = fields
                                .iter()
                                .skip(1)
                                .map(|val| val.to_display_string(registry))
                                .collect::<Vec<_>>()
                                .join(", ");
                            if payload.is_empty() {
                                surface_path_name(&entry.name).to_string()
                            } else {
                                format!("{}({})", surface_path_name(&entry.name), payload)
                            }
                        }
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
                            "{}",
                            fields
                                .first()
                                .map(|v| match v {
                                    Value::Error(rich) => rich.to_result_display_string(),
                                    _ => format!("Err({})", v.to_display_string(registry)),
                                })
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
                        "<function:{}; lexical_captures={}>",
                        fun_idx,
                        callable.lexical_captures.len()
                    )
                }
                CallableTarget::Template(template_id) => {
                    format!(
                        "<template:{}; lexical_captures={}>",
                        template_id,
                        callable.lexical_captures.len()
                    )
                }
            },
            Value::Error(rich) => rich.to_display_string(),
            Value::Regex(handle) => format!("Regex({:?})", handle.pattern),
            Value::RegexCaptures(handle) => {
                format!("RegexCaptures(groups: {})", handle.groups.len())
            }
            Value::RegexMatch(handle) => format!("RegexMatch({}..{})", handle.start, handle.end),
            Value::RandomGenerator(_) => "RandomGenerator(<opaque>)".to_string(),
            Value::Pid(handle) => format!(
                "PID({}#{})",
                surface_path_name(&handle.process_name),
                handle.id
            ),
            Value::FileHandle(handle) => format!("FileHandle#{}", handle.id),
            Value::Workers(handle) => format!("Workers<{}>#{}", handle.process_name, handle.id),
            Value::WorkerLease(handle) => {
                format!("WorkerLease<{}>#{}", handle.pid.process_name, handle.pid.id)
            }
            Value::TaskHandle(future_id) => format!("TaskHandle#{future_id}"),
            Value::PendingFuture(future_id) => format!("<pending:future#{future_id}>"),
        }
    }
}

pub fn quote_surtr_string_literal(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn visible_runtime_error_message(message: &str) -> &str {
    message
        .split_once("\t@@lhs=")
        .map(|(head, _)| head)
        .unwrap_or(message)
}

fn split_runtime_error_diagnostic(
    kind: &str,
    message: &str,
) -> (String, Option<RuntimeErrorDiagnostic>) {
    if kind != "PatternMismatch" {
        return (message.to_string(), None);
    }
    let Some((base, rest)) = message.split_once("\t@@lhs=") else {
        return (message.to_string(), None);
    };
    let Some((lhs, rhs)) = rest.split_once("\t@@rhs=") else {
        return (message.to_string(), None);
    };
    (
        base.to_string(),
        Some(RuntimeErrorDiagnostic::LiteralPatternMismatch {
            lhs: lhs.to_string(),
            rhs: rhs.to_string(),
        }),
    )
}

impl HashMapHandle {
    pub fn empty() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn from_entries(entries: Vec<(String, Value)>) -> Self {
        let mut map = Self::empty();
        for (key, value) in entries {
            map = map.insert(key, value);
        }
        map
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.entries.get(key).cloned()
    }

    pub fn insert(&self, key: String, value: Value) -> Self {
        let mut entries = self.entries.clone();
        entries.insert(key, value);
        Self { entries }
    }

    pub fn remove(&self, key: &str) -> Self {
        if !self.entries.contains_key(key) {
            return self.clone();
        }
        let mut entries = self.entries.clone();
        entries.remove(key);
        Self { entries }
    }

    pub fn keys(&self) -> Vec<String> {
        self.sorted_entries()
            .into_iter()
            .map(|(key, _)| key)
            .collect()
    }

    pub fn values(&self) -> Vec<Value> {
        self.sorted_entries()
            .into_iter()
            .map(|(_, value)| value)
            .collect()
    }

    pub fn sorted_entries(&self) -> Vec<(String, Value)> {
        let mut entries = self
            .entries
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        entries
    }
}

impl ListHandle {
    pub fn empty() -> Self {
        Self { head: None, len: 0 }
    }

    pub fn cons(head: Value, tail: &ListHandle) -> Self {
        Self {
            head: Some(Rc::new(ListNode {
                value: head,
                next: tail.head.clone(),
            })),
            len: tail.len + 1,
        }
    }

    pub fn from_items(items: Vec<Value>) -> Self {
        let mut list = Self::empty();
        for item in items.into_iter().rev() {
            list = Self::cons(item, &list);
        }
        list
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn non_empty_head(&self) -> Option<&Rc<ListNode>> {
        self.head.as_ref()
    }

    pub fn head_value(&self) -> Option<Value> {
        self.non_empty_head().map(|node| node.value.clone())
    }

    pub fn tail_handle(&self) -> Option<Self> {
        self.non_empty_head().map(|node| Self {
            head: node.next.clone(),
            len: self.len.saturating_sub(1),
        })
    }

    pub fn iter(&self) -> ListIter {
        ListIter {
            next: self.head.clone(),
        }
    }
}

pub struct ListIter {
    next: ListRef,
}

impl Iterator for ListIter {
    type Item = Value;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next.clone()?;
        self.next = current.next.clone();
        Some(current.value.clone())
    }
}

/// Rich error value produced by `deferror`.
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeErrorDiagnostic {
    LiteralPatternMismatch { lhs: String, rhs: String },
}

/// Rich error value produced by `deferror`.
#[derive(Debug, Clone, PartialEq)]
pub struct RichError {
    pub kind: String,
    pub message: String,
    pub location: Location,
    pub cause: Option<Box<RichError>>,
    pub diagnostic: Option<RuntimeErrorDiagnostic>,
}

impl RichError {
    pub fn new(
        kind: impl Into<String>,
        message: impl Into<String>,
        location: Location,
        cause: Option<Box<RichError>>,
    ) -> Self {
        let kind = kind.into();
        let (message, diagnostic) = split_runtime_error_diagnostic(&kind, &message.into());
        Self {
            kind,
            message,
            location,
            cause,
            diagnostic,
        }
    }

    pub fn visible_message(&self) -> &str {
        visible_runtime_error_message(&self.message)
    }

    pub fn to_display_string(&self) -> String {
        self.to_display_lines().join("\n")
    }

    pub fn to_result_display_string(&self) -> String {
        let lines = self.to_display_lines();
        let Some((head, tail)) = lines.split_first() else {
            return "Err()".to_string();
        };

        let mut rendered = format!("Err({})", head);
        for line in tail {
            rendered.push('\n');
            rendered.push_str(line);
        }
        rendered
    }

    pub fn to_eprint_lines(&self) -> Vec<String> {
        let display_kind = surface_path_name(&self.kind);
        let mut lines = vec![format!(
            "Error: {}: {}",
            display_kind,
            self.visible_message()
        )];
        let mut next = self.cause.as_deref();
        while let Some(cause) = next {
            let cause_kind = surface_path_name(&cause.kind);
            lines.push(format!(
                "Caused by: {}: {}",
                cause_kind,
                cause.visible_message()
            ));
            next = cause.cause.as_deref();
        }
        lines
    }

    pub fn append_cause_tail(&mut self, cause: RichError) {
        match self.cause.as_mut() {
            Some(existing) => existing.append_cause_tail(cause),
            None => self.cause = Some(Box::new(cause)),
        }
    }

    fn to_display_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        self.push_display_lines(&mut lines, "", "");
        lines
    }

    fn push_display_lines(&self, lines: &mut Vec<String>, first_prefix: &str, child_prefix: &str) {
        lines.push(format!(
            "{}{}({:?})",
            first_prefix,
            surface_path_name(&self.kind),
            self.visible_message()
        ));
        if let Some(cause) = self.cause.as_deref() {
            cause.push_display_lines(
                lines,
                &format!("{}|_ ", child_prefix),
                &format!("{}   ", child_prefix),
            );
        }
    }
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
    use super::{
        Callable, CallableMetadata, CallableTarget, HashMapHandle, ListHandle, Location, RichError,
        TypeEntry, TypeKind, TypeRegistry, Value,
    };
    use crate::primitives::int;

    #[test]
    fn display_for_reserved_result_tags() {
        let registry = TypeRegistry::new();
        let ok = Value::Tagged {
            tag: 0,
            fields: vec![Value::Int(int(42))],
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
            private_flags: vec![false, false],
        });
        registry.register(TypeEntry {
            tag: 11,
            name: "Pair".into(),
            kind: TypeKind::Record,
            field_names: vec!["left".into(), "right".into()],
            private_flags: vec![false, false],
        });
        registry.register(TypeEntry {
            tag: 12,
            name: "SecretUser".into(),
            kind: TypeKind::Struct,
            field_names: vec!["name".into(), "password".into()],
            private_flags: vec![false, true],
        });

        let user = Value::Tagged {
            tag: 10,
            fields: vec![Value::Str("alice".into()), Value::Int(int(20))],
        };
        let pair = Value::Tagged {
            tag: 11,
            fields: vec![Value::Int(int(1)), Value::Int(int(2))],
        };
        let secret_user = Value::Tagged {
            tag: 12,
            fields: vec![Value::Str("alice".into()), Value::Str("s3cr3t".into())],
        };

        assert_eq!(
            user.to_display_string(&registry),
            "User(name: alice, age: 20)"
        );
        assert_eq!(pair.to_display_string(&registry), "Pair(left: 1, right: 2)");
        assert_eq!(
            secret_user.to_display_string(&registry),
            "SecretUser(name: alice, ..private)"
        );
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
            cause: None,
            diagnostic: None,
        }));
        assert_eq!(value.to_display_string(&registry), "TestError(\"boom\")");
    }

    #[test]
    fn display_for_rich_error_renders_tree_when_causes_exist() {
        let registry = TypeRegistry::new();
        let mut value = RichError {
            kind: "Outer".into(),
            message: "outer".into(),
            location: Location {
                file: "<repl>".into(),
                func: "f".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 1,
            },
            cause: None,
            diagnostic: None,
        };
        value.append_cause_tail(RichError {
            kind: "Inner".into(),
            message: "inner".into(),
            location: Location {
                file: "<repl>".into(),
                func: "f".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 1,
            },
            cause: None,
            diagnostic: None,
        });
        value.append_cause_tail(RichError {
            kind: "Leaf".into(),
            message: "leaf".into(),
            location: Location {
                file: "<repl>".into(),
                func: "f".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 1,
            },
            cause: None,
            diagnostic: None,
        });

        let rendered = Value::Error(Box::new(value));
        assert_eq!(
            rendered.to_display_string(&registry),
            "Outer(\"outer\")\n|_ Inner(\"inner\")\n   |_ Leaf(\"leaf\")"
        );
    }

    #[test]
    fn list_display_uses_cons_handle_shape() {
        let registry = TypeRegistry::new();
        let value = Value::List(ListHandle::from_items(vec![
            Value::Int(int(1)),
            Value::Int(int(2)),
            Value::Int(int(3)),
        ]));
        assert_eq!(value.to_display_string(&registry), "[1, 2, 3]");
    }

    #[test]
    fn hash_map_handle_insert_overwrite_and_remove_use_sorted_key_order() {
        let empty = HashMapHandle::empty();
        let with_b = empty.insert("b".into(), Value::Int(int(2)));
        let with_ba = with_b.insert("a".into(), Value::Int(int(1)));
        let with_overwrite = with_ba.insert("a".into(), Value::Int(int(3)));

        assert_eq!(with_overwrite.len(), 2);
        assert_eq!(
            with_overwrite.keys(),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            with_overwrite.values(),
            vec![Value::Int(int(3)), Value::Int(int(2))]
        );
        assert_eq!(with_overwrite.get("a"), Some(Value::Int(int(3))));

        let removed = with_overwrite.remove("b");
        assert_eq!(removed.keys(), vec!["a".to_string()]);
        let no_op = removed.remove("missing");
        assert_eq!(no_op, removed);
    }

    #[test]
    fn hash_map_display_uses_named_shape_and_key_escaping() {
        let registry = TypeRegistry::new();
        let value = Value::HashMap(HashMapHandle::from_entries(vec![
            ("line\nfeed".into(), Value::Int(int(1))),
            ("path\\to".into(), Value::Int(int(2))),
            ("say\"hi".into(), Value::Int(int(3))),
            ("tab\tchar".into(), Value::Int(int(4))),
        ]));
        assert_eq!(
            value.to_display_string(&registry),
            "HashMap(\"line\\nfeed\" => 1, \"path\\\\to\" => 2, \"say\\\"hi\" => 3, \"tab\\tchar\" => 4)"
        );
    }

    #[test]
    fn type_registry_lookup_supports_sparse_tags() {
        let registry = TypeRegistry::from_entries(vec![
            TypeEntry {
                tag: 10,
                name: "Global::User".into(),
                kind: TypeKind::Struct,
                field_names: vec!["name".into()],
                private_flags: vec![false],
            },
            TypeEntry {
                tag: 42,
                name: "Global::Profile".into(),
                kind: TypeKind::Struct,
                field_names: vec!["id".into()],
                private_flags: vec![false],
            },
        ]);

        assert_eq!(
            registry.lookup(10).map(|entry| entry.name.as_str()),
            Some("Global::User")
        );
        assert_eq!(
            registry.lookup(42).map(|entry| entry.name.as_str()),
            Some("Global::Profile")
        );
        assert_eq!(registry.lookup(11), None);
    }

    #[test]
    fn type_registry_extend_preserves_existing_and_new_lookups() {
        let mut registry = TypeRegistry::from_entries(vec![TypeEntry {
            tag: 10,
            name: "Global::User".into(),
            kind: TypeKind::Struct,
            field_names: vec!["name".into()],
            private_flags: vec![false],
        }]);

        registry.extend(vec![TypeEntry {
            tag: 42,
            name: "Global::Profile".into(),
            kind: TypeKind::Struct,
            field_names: vec!["id".into()],
            private_flags: vec![false],
        }]);

        assert_eq!(registry.entries().len(), 2);
        assert_eq!(
            registry.lookup(10).map(|entry| entry.name.as_str()),
            Some("Global::User")
        );
        assert_eq!(
            registry.lookup(42).map(|entry| entry.name.as_str()),
            Some("Global::Profile")
        );
    }

    #[test]
    fn empty_list_head_and_tail_return_none() {
        let list = ListHandle::empty();
        assert_eq!(list.head_value(), None);
        assert_eq!(list.tail_handle(), None);
    }

    #[test]
    fn single_item_list_head_and_tail_preserve_non_empty_contract() {
        let list = ListHandle::from_items(vec![Value::Int(int(7))]);

        assert_eq!(list.len, 1);
        assert_eq!(list.head_value(), Some(Value::Int(int(7))));

        let tail = list.tail_handle().expect("single-item list has empty tail");
        assert_eq!(tail.len, 0);
        assert_eq!(tail.head_value(), None);
        assert!(tail.is_empty());
    }

    #[test]
    fn multi_item_list_head_and_tail_walk_shared_chain() {
        let list = ListHandle::from_items(vec![
            Value::Int(int(1)),
            Value::Int(int(2)),
            Value::Int(int(3)),
        ]);

        assert_eq!(list.len, 3);
        assert_eq!(list.head_value(), Some(Value::Int(int(1))));

        let tail = list.tail_handle().expect("non-empty list has tail handle");
        assert_eq!(tail.len, 2);
        assert_eq!(tail.head_value(), Some(Value::Int(int(2))));

        let tail_tail = tail.tail_handle().expect("tail stays non-empty");
        assert_eq!(tail_tail.len, 1);
        assert_eq!(tail_tail.head_value(), Some(Value::Int(int(3))));
    }

    #[test]
    fn cons_and_tail_update_len_without_mutating_shared_tail() {
        let tail = ListHandle::from_items(vec![Value::Int(int(2)), Value::Int(int(3))]);
        let list = ListHandle::cons(Value::Int(int(1)), &tail);

        assert_eq!(list.len, 3);
        assert_eq!(list.head_value(), Some(Value::Int(int(1))));

        let derived_tail = list.tail_handle().expect("cons list has tail");
        assert_eq!(derived_tail.len, 2);
        assert_eq!(derived_tail.head_value(), Some(Value::Int(int(2))));

        assert_eq!(tail.len, 2);
        assert_eq!(tail.head_value(), Some(Value::Int(int(2))));
    }

    #[test]
    fn display_for_callable_shows_target_and_capture_counts() {
        let registry = TypeRegistry::new();
        let builtin = Value::Callable(Callable {
            target: CallableTarget::Builtin(3),
            lexical_captures: Vec::new(),
            metadata: CallableMetadata::default(),
        });
        let function = Value::Callable(Callable {
            target: CallableTarget::Function(7),
            lexical_captures: vec![Value::Unit],
            metadata: CallableMetadata::default(),
        });
        let template = Value::Callable(Callable {
            target: CallableTarget::Template(11),
            lexical_captures: vec![Value::Unit, Value::Bool(true)],
            metadata: CallableMetadata::default(),
        });
        assert_eq!(builtin.to_display_string(&registry), "<builtin:3>");
        assert_eq!(
            function.to_display_string(&registry),
            "<function:7; lexical_captures=1>"
        );
        assert_eq!(
            template.to_display_string(&registry),
            "<template:11; lexical_captures=2>"
        );
    }

    #[test]
    fn display_result_err_with_rich_error_uses_error_constructor_shape() {
        let registry = TypeRegistry::new();
        let value = Value::Tagged {
            tag: 1,
            fields: vec![Value::Error(Box::new(RichError {
                kind: "NoneError".into(),
                message: "null".into(),
                location: Location {
                    file: "<repl>".into(),
                    func: "f".into(),
                    line: 1,
                    column: 1,
                    span_start: 0,
                    span_end: 1,
                },
                cause: None,
                diagnostic: None,
            }))],
        };
        assert_eq!(
            value.to_display_string(&registry),
            "Err(NoneError(\"null\"))"
        );
    }

    #[test]
    fn display_result_err_with_rich_error_preserves_tree_shape() {
        let registry = TypeRegistry::new();
        let mut rich = RichError {
            kind: "Higher".into(),
            message: "higher".into(),
            location: Location {
                file: "<repl>".into(),
                func: "f".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 1,
            },
            cause: None,
            diagnostic: None,
        };
        rich.append_cause_tail(RichError {
            kind: "Lower".into(),
            message: "lower".into(),
            location: Location {
                file: "<repl>".into(),
                func: "f".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 1,
            },
            cause: None,
            diagnostic: None,
        });

        let value = Value::Tagged {
            tag: 1,
            fields: vec![Value::Error(Box::new(rich))],
        };
        assert_eq!(
            value.to_display_string(&registry),
            "Err(Higher(\"higher\"))\n|_ Lower(\"lower\")"
        );
    }

    #[test]
    fn rich_error_eprint_lines_follow_linear_chain_order() {
        let mut rich = RichError {
            kind: "Higher".into(),
            message: "higher".into(),
            location: Location {
                file: "<repl>".into(),
                func: "f".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 1,
            },
            cause: None,
            diagnostic: None,
        };
        rich.append_cause_tail(RichError {
            kind: "Lower".into(),
            message: "lower".into(),
            location: Location {
                file: "<repl>".into(),
                func: "f".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 1,
            },
            cause: None,
            diagnostic: None,
        });

        assert_eq!(
            rich.to_eprint_lines(),
            vec![
                "Error: Higher: higher".to_string(),
                "Caused by: Lower: lower".to_string(),
            ]
        );
    }

    #[test]
    fn rich_error_eprint_lines_hide_runtime_literal_metadata() {
        let rich = RichError {
            kind: "PatternMismatch".into(),
            message: "Pattern did not match.\t@@lhs=\"1\"\t@@rhs=\"2\"".into(),
            location: Location {
                file: "<repl>".into(),
                func: "f".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 1,
            },
            cause: None,
            diagnostic: None,
        };

        assert_eq!(
            rich.to_eprint_lines(),
            vec!["Error: PatternMismatch: Pattern did not match.".to_string()]
        );
    }
}
