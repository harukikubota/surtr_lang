use ariadne::{Color, Label, Report, ReportKind, Source};
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
    pub context: RuntimeErrorContext,
}

impl RuntimeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            context: RuntimeErrorContext::default(),
        }
    }

    pub fn with_context(mut self, context: RuntimeErrorContext) -> Self {
        self.context = context;
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

pub fn report_runtime_error(
    err: &RuntimeError,
    source: Option<&str>,
    fallback_file: Option<&str>,
    location: Option<Location>,
) {
    let mut rendered = false;

    if let (Some(source), Some(location)) = (source, location.clone()) {
        let file_name = if location.file.is_empty() {
            fallback_file.unwrap_or("<runtime>")
        } else {
            location.file.as_str()
        };
        let start = location.span_start as usize;
        let end = if location.span_end > location.span_start {
            location.span_end as usize
        } else {
            start.saturating_add(1)
        };

        let report = Report::build(ReportKind::Error, (file_name, start..end))
            .with_message("RuntimeError")
            .with_label(
                Label::new((file_name, start..end))
                    .with_message(&err.message)
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((file_name, Source::from(source)));

        if report.is_ok() {
            rendered = true;
        }
    }

    if !rendered {
        eprintln!(
            "{}",
            format_runtime_error_with_location(err, location.as_ref())
        );
    }

    if runtime_error_verbose_enabled() {
        eprintln!("{}", format_runtime_error_verbose(err));
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format_runtime_error(self))
    }
}

impl std::error::Error for RuntimeError {}

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
        RuntimeError, RuntimeErrorContext,
    };
    use sindr::runtime::Location;

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
}
