use sindr::runtime::{Location, RuntimeStackFrame};

const RUNTIME_ERROR_KIND_DETAIL_PREFIX: &str = "__surtr_runtime_error_kind=";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeErrorKind {
    Generic,
    ProcessInitTimeout,
    ProcessInitFailed,
    TaskTimeout,
    CallTimeout,
    ProcessLifecycleFailed,
}

impl RuntimeErrorKind {
    fn as_detail_value(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::ProcessInitTimeout => "process_init_timeout",
            Self::ProcessInitFailed => "process_init_failed",
            Self::TaskTimeout => "task_timeout",
            Self::CallTimeout => "call_timeout",
            Self::ProcessLifecycleFailed => "process_lifecycle_failed",
        }
    }

    fn from_detail_value(value: &str) -> Option<Self> {
        match value {
            "generic" => Some(Self::Generic),
            "process_init_timeout" => Some(Self::ProcessInitTimeout),
            "process_init_failed" => Some(Self::ProcessInitFailed),
            "task_timeout" => Some(Self::TaskTimeout),
            "call_timeout" => Some(Self::CallTimeout),
            "process_lifecycle_failed" => Some(Self::ProcessLifecycleFailed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuntimeErrorContext {
    pub pc: Option<usize>,
    pub opcode: Option<String>,
    pub function: Option<String>,
    pub call_site: Option<Location>,
    pub details: Vec<String>,
    pub stack_trace: Vec<RuntimeStackFrame>,
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
    pub context: Box<RuntimeErrorContext>,
}

impl RuntimeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            context: Box::new(RuntimeErrorContext::default()),
        }
    }

    pub fn with_context(mut self, context: RuntimeErrorContext) -> Self {
        let kind = self.kind();
        self.context = Box::new(context);
        self.set_kind(kind);
        self
    }

    pub fn with_kind(message: impl Into<String>, kind: RuntimeErrorKind) -> Self {
        let mut err = Self::new(message);
        err.set_kind(kind);
        err
    }

    pub fn process_init_timeout(message: impl Into<String>) -> Self {
        Self::with_kind(message, RuntimeErrorKind::ProcessInitTimeout)
    }

    pub fn process_init_failed(message: impl Into<String>) -> Self {
        Self::with_kind(message, RuntimeErrorKind::ProcessInitFailed)
    }

    pub fn task_timeout(message: impl Into<String>) -> Self {
        Self::with_kind(message, RuntimeErrorKind::TaskTimeout)
    }

    pub fn call_timeout(message: impl Into<String>) -> Self {
        Self::with_kind(message, RuntimeErrorKind::CallTimeout)
    }

    pub fn process_lifecycle_failed(message: impl Into<String>) -> Self {
        Self::with_kind(message, RuntimeErrorKind::ProcessLifecycleFailed)
    }

    pub fn kind(&self) -> RuntimeErrorKind {
        self.context
            .details
            .iter()
            .find_map(|detail| {
                detail
                    .strip_prefix(RUNTIME_ERROR_KIND_DETAIL_PREFIX)
                    .and_then(RuntimeErrorKind::from_detail_value)
            })
            .unwrap_or(RuntimeErrorKind::Generic)
    }

    fn set_kind(&mut self, kind: RuntimeErrorKind) {
        self.context
            .details
            .retain(|detail| !detail.starts_with(RUNTIME_ERROR_KIND_DETAIL_PREFIX));

        if kind != RuntimeErrorKind::Generic {
            self.context.details.push(format!(
                "{}{}",
                RUNTIME_ERROR_KIND_DETAIL_PREFIX,
                kind.as_detail_value()
            ));
        }
    }
}

pub fn format_runtime_error(err: &RuntimeError) -> String {
    format!("RuntimeError: {}", err.message)
}

pub fn format_runtime_error_with_location(
    err: &RuntimeError,
    location: Option<&Location>,
) -> String {
    match location {
        Some(location) => format!(
            "{} ({}:{}:{})",
            format_runtime_error(err),
            location.file,
            location.line,
            location.column
        ),
        None => format_runtime_error(err),
    }
}

pub fn format_runtime_error_verbose(err: &RuntimeError) -> String {
    let mut lines = vec![format_runtime_error(err)];

    if let Some(pc) = err.context.pc {
        lines.push(format!("  pc: {}", pc));
    }
    if let Some(opcode) = err.context.opcode.as_deref() {
        lines.push(format!("  opcode: {}", opcode));
    }
    if let Some(function) = err.context.function.as_deref() {
        lines.push(format!("  function: {}", function));
    }
    if let Some(location) = err.context.call_site.as_ref() {
        lines.push(format!(
            "  call_site: {}:{}:{}",
            location.file, location.line, location.column
        ));
    }
    for detail in &err.context.details {
        lines.push(format!("  detail: {}", detail));
    }
    if !err.context.stack_trace.is_empty() {
        lines.push("Stack trace:".into());
        for (idx, frame) in err.context.stack_trace.iter().enumerate() {
            lines.push(format!("  {}: {}", idx, format_stack_frame(frame)));
        }
    }

    lines.join("\n")
}

pub fn format_stack_frame(frame: &RuntimeStackFrame) -> String {
    let mut rendered = frame
        .function
        .as_deref()
        .unwrap_or("<top-level>")
        .to_string();
    if let Some(location) = frame.location.as_ref() {
        rendered.push_str(&format!(
            " at {}:{}:{}",
            location.file, location.line, location.column
        ));
    }
    if frame.tco {
        rendered.push_str(" tail-call");
    }
    rendered
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format_runtime_error(self))
    }
}

impl std::error::Error for RuntimeError {}

#[cfg(test)]
fn runtime_error_verbose_enabled() -> bool {
    matches!(
        std::env::var("SURTR_VERBOSE_RUNTIME_ERROR").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES")
    )
}

#[cfg(test)]
mod tests {
    use super::{
        format_runtime_error, format_runtime_error_verbose, format_runtime_error_with_location,
        runtime_error_verbose_enabled, RuntimeError, RuntimeErrorContext, RuntimeErrorKind,
    };
    use sindr::runtime::Location;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn format_runtime_error_uses_shared_shape() {
        let err = RuntimeError::new("boom");
        assert_eq!(format_runtime_error(&err), "RuntimeError: boom");
    }

    #[test]
    fn runtime_error_new_defaults_to_generic_kind_and_preserves_formatting() {
        let err = RuntimeError::new("boom");

        assert_eq!(err.kind(), RuntimeErrorKind::Generic);
        assert_eq!(format_runtime_error(&err), "RuntimeError: boom");
        assert_eq!(err.to_string(), "RuntimeError: boom");
    }

    #[test]
    fn typed_runtime_error_constructors_store_kind_and_preserve_formatting() {
        let cases = [
            (
                RuntimeError::process_init_timeout("init timed out"),
                RuntimeErrorKind::ProcessInitTimeout,
            ),
            (
                RuntimeError::process_init_failed("init failed"),
                RuntimeErrorKind::ProcessInitFailed,
            ),
            (
                RuntimeError::task_timeout("task timed out"),
                RuntimeErrorKind::TaskTimeout,
            ),
            (
                RuntimeError::call_timeout("call timed out"),
                RuntimeErrorKind::CallTimeout,
            ),
            (
                RuntimeError::process_lifecycle_failed("lifecycle failed"),
                RuntimeErrorKind::ProcessLifecycleFailed,
            ),
        ];

        for (err, expected_kind) in cases {
            assert_eq!(err.kind(), expected_kind);
            assert_eq!(
                format_runtime_error(&err),
                format!("RuntimeError: {}", err.message)
            );
        }
    }

    #[test]
    fn typed_runtime_error_kind_survives_context_enrichment() {
        let err = RuntimeError::task_timeout("task timed out").with_context(RuntimeErrorContext {
            pc: Some(3),
            opcode: Some("CallBuiltin".into()),
            function: None,
            call_site: None,
            details: vec!["task_id=7".into()],
            stack_trace: Vec::new(),
        });

        assert_eq!(err.kind(), RuntimeErrorKind::TaskTimeout);
        assert!(err
            .context
            .details
            .iter()
            .any(|detail| detail == "task_id=7"));
    }

    #[test]
    fn format_runtime_error_with_location_appends_file_line_column() {
        let err = RuntimeError::new("boom");
        let location = Location {
            file: "main.srt".into(),
            func: "<runtime>".into(),
            line: 3,
            column: 7,
            span_start: 10,
            span_end: 14,
        };
        assert_eq!(
            format_runtime_error_with_location(&err, Some(&location)),
            "RuntimeError: boom (main.srt:3:7)"
        );
    }

    #[test]
    fn format_runtime_error_verbose_includes_context_fields() {
        let err = RuntimeError::new("boom").with_context(RuntimeErrorContext {
            pc: Some(42),
            opcode: Some("DivInt".into()),
            function: Some("fun#1".into()),
            call_site: Some(Location {
                file: "main.srt".into(),
                func: "<runtime>".into(),
                line: 5,
                column: 9,
                span_start: 20,
                span_end: 24,
            }),
            details: vec!["stack_depth=2".into(), "locals_len=1".into()],
            stack_trace: Vec::new(),
        });

        let formatted = format_runtime_error_verbose(&err);
        assert!(formatted.contains("RuntimeError: boom"));
        assert!(formatted.contains("pc: 42"));
        assert!(formatted.contains("opcode: DivInt"));
        assert!(formatted.contains("function: fun#1"));
        assert!(formatted.contains("call_site: main.srt:5:9"));
        assert!(formatted.contains("detail: stack_depth=2"));
        assert!(formatted.contains("detail: locals_len=1"));
    }

    #[test]
    fn runtime_error_verbose_enabled_accepts_expected_values() {
        let _guard = env_lock().lock().expect("env lock");
        let previous = std::env::var("SURTR_VERBOSE_RUNTIME_ERROR").ok();

        for (value, expected) in [
            (None, false),
            (Some("0"), false),
            (Some("1"), true),
            (Some("true"), true),
            (Some("TRUE"), true),
            (Some("yes"), true),
            (Some("YES"), true),
            (Some("no"), false),
        ] {
            match value {
                Some(value) => std::env::set_var("SURTR_VERBOSE_RUNTIME_ERROR", value),
                None => std::env::remove_var("SURTR_VERBOSE_RUNTIME_ERROR"),
            }
            assert_eq!(runtime_error_verbose_enabled(), expected, "value={value:?}");
        }

        match previous {
            Some(value) => std::env::set_var("SURTR_VERBOSE_RUNTIME_ERROR", value),
            None => std::env::remove_var("SURTR_VERBOSE_RUNTIME_ERROR"),
        }
    }
}
