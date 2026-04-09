use ariadne::{Color, Label, Report, ReportKind, Source};
use sindr::runtime::Location;
use std::io::{self, Write};

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
    let mut stderr = io::stderr().lock();
    let _ = report_runtime_error_to(
        &mut stderr,
        err,
        source,
        fallback_file,
        location,
        runtime_error_verbose_enabled(),
    );
}

fn report_runtime_error_to(
    writer: &mut impl Write,
    err: &RuntimeError,
    source: Option<&str>,
    fallback_file: Option<&str>,
    location: Option<Location>,
    verbose: bool,
) -> io::Result<()> {
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
            .finish();

        if report
            .write((file_name, Source::from(source)), &mut *writer)
            .is_ok()
        {
            rendered = true;
        }
    }

    if !rendered {
        writeln!(
            writer,
            "{}",
            format_runtime_error_with_location(err, location.as_ref())
        )?;
    }

    if verbose {
        writeln!(writer, "{}", format_runtime_error_verbose(err))?;
    }

    Ok(())
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
        report_runtime_error_to, runtime_error_verbose_enabled, RuntimeError, RuntimeErrorContext,
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
    fn report_runtime_error_to_falls_back_without_source() {
        let err = RuntimeError::new("boom");
        let location = Location {
            file: "main.srt".into(),
            func: "<runtime>".into(),
            line: 3,
            column: 7,
            span_start: 10,
            span_end: 14,
        };
        let mut buf = Vec::new();
        report_runtime_error_to(&mut buf, &err, None, None, Some(location), false)
            .expect("fallback rendering should succeed");
        let rendered = String::from_utf8(buf).expect("stderr should be utf-8");
        assert!(rendered.contains("RuntimeError: boom (main.srt:3:7)"));
    }

    #[test]
    fn report_runtime_error_to_renders_source_report_and_verbose_block() {
        let err = RuntimeError::new("boom").with_context(RuntimeErrorContext {
            pc: Some(9),
            opcode: Some("AddInt".into()),
            function: None,
            call_site: None,
            details: vec!["stack_depth=1".into()],
        });
        let location = Location {
            file: "main.srt".into(),
            func: "<runtime>".into(),
            line: 1,
            column: 1,
            span_start: 0,
            span_end: 4,
        };
        let mut buf = Vec::new();
        report_runtime_error_to(&mut buf, &err, Some("boom"), None, Some(location), true)
            .expect("report rendering should succeed");
        let rendered = String::from_utf8(buf).expect("stderr should be utf-8");
        assert!(rendered.contains("RuntimeError"));
        assert!(rendered.contains("boom"));
        assert!(rendered.contains("pc: 9"));
        assert!(rendered.contains("opcode: AddInt"));
        assert!(rendered.contains("detail: stack_depth=1"));
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
