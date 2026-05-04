use sindr::runtime::Location;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuntimeErrorContext {
    pub pc: Option<usize>,
    pub opcode: Option<String>,
    pub function: Option<String>,
    pub call_site: Option<Location>,
    pub details: Vec<String>,
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
        self.context = Box::new(context);
        self
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

    lines.join("\n")
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
        runtime_error_verbose_enabled, RuntimeError, RuntimeErrorContext,
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
