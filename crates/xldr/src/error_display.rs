use diagnostics::{DiagnosticSpec, SourceId, SourceRegistry};
use eldr::builtin::inspect_value;
use eldr::value::Value;
use spire::ast::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorDisplayMode {
    #[default]
    Full,
    Summary,
}

impl ErrorDisplayMode {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "full" => Some(Self::Full),
            "summary" => Some(Self::Summary),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Summary => "summary",
        }
    }
}

fn first_visible_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim_end().to_string())
        .unwrap_or_default()
}

pub fn text_for_mode(text: &str, mode: ErrorDisplayMode) -> String {
    match mode {
        ErrorDisplayMode::Full => text.to_string(),
        ErrorDisplayMode::Summary => first_visible_line(text),
    }
}

pub fn lines_for_mode(text: &str, mode: ErrorDisplayMode) -> Vec<String> {
    text_for_mode(text, mode)
        .lines()
        .map(|line| line.to_string())
        .collect()
}

pub fn emit_text(text: &str, mode: ErrorDisplayMode) {
    let rendered = text_for_mode(text, mode);
    if rendered.is_empty() {
        return;
    }

    if rendered.ends_with('\n') {
        eprint!("{}", rendered);
    } else {
        eprintln!("{}", rendered);
    }
}

pub fn diagnostic_text(file_name: &str, source: &str, spec: &DiagnosticSpec) -> String {
    diagnostics::render_error(file_name, source, spec)
}

pub fn diagnostic_text_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    spec: &DiagnosticSpec,
) -> String {
    diagnostics::render_error_by_id(sources, source_id, spec)
}

pub fn diagnostic_lines(
    file_name: &str,
    source: &str,
    spec: &DiagnosticSpec,
    mode: ErrorDisplayMode,
) -> Vec<String> {
    lines_for_mode(&diagnostic_text(file_name, source, spec), mode)
}

pub fn diagnostic_lines_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    spec: &DiagnosticSpec,
    mode: ErrorDisplayMode,
) -> Vec<String> {
    lines_for_mode(&diagnostic_text_by_id(sources, source_id, spec), mode)
}

pub fn emit_diagnostic(
    file_name: &str,
    source: &str,
    spec: &DiagnosticSpec,
    mode: ErrorDisplayMode,
) {
    emit_text(&diagnostic_text(file_name, source, spec), mode);
}

pub fn emit_diagnostic_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    spec: &DiagnosticSpec,
    mode: ErrorDisplayMode,
) {
    emit_text(&diagnostic_text_by_id(sources, source_id, spec), mode);
}

fn runtime_error_verbose_enabled() -> bool {
    matches!(
        std::env::var("SURTR_VERBOSE_RUNTIME_ERROR").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES")
    )
}

fn runtime_error_help_lines(err: &eldr::RuntimeError) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(pc) = err.context.pc {
        lines.push(format!("pc: {}", pc));
    }
    if let Some(opcode) = err.context.opcode.as_deref() {
        lines.push(format!("opcode: {}", opcode));
    }
    if let Some(function) = err.context.function.as_deref() {
        lines.push(format!("function: {}", function));
    }
    if let Some(location) = err.context.call_site.as_ref() {
        lines.push(format!(
            "call_site: {}:{}:{}",
            location.file, location.line, location.column
        ));
    }
    for detail in &err.context.details {
        lines.push(format!("detail: {}", detail));
    }
    lines
}

fn runtime_error_help(err: &eldr::RuntimeError) -> Option<String> {
    let lines = runtime_error_help_lines(err);
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn runtime_error_spec(
    err: &eldr::RuntimeError,
    location: &sindr::runtime::Location,
    include_help: bool,
) -> DiagnosticSpec {
    let start = location.span_start as usize;
    let mut end = location.span_end as usize;
    if end <= start {
        end = start.saturating_add(1);
    }

    diagnostics::simple_error(
        "RuntimeError",
        err.message.clone(),
        Span { start, end },
        if include_help {
            runtime_error_help(err)
        } else {
            None
        },
    )
}

fn runtime_file_name(location: &sindr::runtime::Location, fallback_file: Option<&str>) -> String {
    if location.file.is_empty() {
        fallback_file.unwrap_or("<runtime>").to_string()
    } else {
        location.file.clone()
    }
}

fn fallback_span_from_source(source: &str) -> Span {
    let len = source.chars().count();
    if len == 0 {
        Span { start: 0, end: 0 }
    } else {
        Span { start: 0, end: 1 }
    }
}

fn invalid_result_spec(span: Span) -> DiagnosticSpec {
    diagnostics::simple_error(
        "InvalidResult",
        "missing Err payload",
        span,
        Some("Result::Err must carry one payload value.".into()),
    )
}

pub fn runtime_error_text(
    err: &eldr::RuntimeError,
    source: Option<&str>,
    fallback_file: Option<&str>,
    location: Option<sindr::runtime::Location>,
) -> String {
    let verbose = runtime_error_verbose_enabled();
    match (source, location.as_ref()) {
        (Some(source), Some(location)) => {
            let file_name = runtime_file_name(location, fallback_file);
            let spec = runtime_error_spec(err, location, verbose);
            diagnostics::render_error(&file_name, source, &spec)
        }
        _ => {
            let mut rendered = eldr::format_runtime_error_with_location(err, location.as_ref());
            if verbose {
                if let Some(help) = runtime_error_help(err) {
                    if !rendered.ends_with('\n') {
                        rendered.push('\n');
                    }
                    for line in help.lines() {
                        rendered.push_str("help: ");
                        rendered.push_str(line);
                        rendered.push('\n');
                    }
                }
            }
            rendered
        }
    }
}

fn runtime_value_cause_help(value: &sindr::runtime::RichError) -> Option<String> {
    let mut lines = Vec::new();
    let mut next = value.cause.as_deref();
    while let Some(cause) = next {
        lines.push(format!("Caused by: {}: {}", cause.kind, cause.message));
        next = cause.cause.as_deref();
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

pub fn runtime_error_lines(
    err: &eldr::RuntimeError,
    source: Option<&str>,
    fallback_file: Option<&str>,
    location: Option<sindr::runtime::Location>,
    mode: ErrorDisplayMode,
) -> Vec<String> {
    lines_for_mode(
        &runtime_error_text(err, source, fallback_file, location),
        mode,
    )
}

pub fn emit_runtime_error(
    err: &eldr::RuntimeError,
    source: Option<&str>,
    fallback_file: Option<&str>,
    location: Option<sindr::runtime::Location>,
    mode: ErrorDisplayMode,
) {
    emit_text(
        &runtime_error_text(err, source, fallback_file, location),
        mode,
    );
}

pub fn invalid_result_missing_payload_text(
    source: Option<&str>,
    fallback_file: Option<&str>,
    location: Option<sindr::runtime::Location>,
) -> String {
    match source {
        Some(source) => {
            let (file_name, span) = match location.as_ref() {
                Some(location) => {
                    let start = location.span_start as usize;
                    let end = if location.span_end > location.span_start {
                        location.span_end as usize
                    } else {
                        start.saturating_add(1)
                    };
                    (
                        runtime_file_name(location, fallback_file),
                        Span { start, end },
                    )
                }
                None => (
                    fallback_file.unwrap_or("<runtime>").to_string(),
                    fallback_span_from_source(source),
                ),
            };
            diagnostics::render_error(&file_name, source, &invalid_result_spec(span))
        }
        None => "Error: InvalidResult: missing Err payload".to_string(),
    }
}

pub fn emit_invalid_result_missing_payload(
    source: Option<&str>,
    fallback_file: Option<&str>,
    location: Option<sindr::runtime::Location>,
    mode: ErrorDisplayMode,
) {
    emit_text(
        &invalid_result_missing_payload_text(source, fallback_file, location),
        mode,
    );
}

fn error_spec_from_value_error(value: &sindr::runtime::RichError) -> DiagnosticSpec {
    let start = value.location.span_start as usize;
    let mut end = value.location.span_end as usize;
    if end <= start {
        end = start.saturating_add(1);
    }

    diagnostics::simple_error(
        value.kind.clone(),
        value.message.clone(),
        Span { start, end },
        runtime_value_cause_help(value),
    )
}

pub fn runtime_value_error_text_from_vm(vm: &eldr::VM, value: &Value) -> String {
    match value {
        Value::Error(rich) => {
            if let (Some(source), Some(file_name)) = (vm.source(), vm.source_file()) {
                let spec = error_spec_from_value_error(rich);
                diagnostic_text(file_name, source, &spec)
            } else {
                format!("Error: {}: {}", rich.kind, rich.message)
            }
        }
        other => format!("Error: {}", inspect_value(vm, other)),
    }
}

pub fn runtime_value_error_text_with_registry(
    vm: &eldr::VM,
    value: &Value,
    sources: &SourceRegistry,
    source_id: SourceId,
) -> String {
    match value {
        Value::Error(rich) => {
            let spec = error_spec_from_value_error(rich);
            diagnostic_text_by_id(sources, source_id, &spec)
        }
        other => format!("Error: {}", inspect_value(vm, other)),
    }
}

pub fn runtime_value_error_lines_from_vm(
    vm: &eldr::VM,
    value: &Value,
    mode: ErrorDisplayMode,
) -> Vec<String> {
    lines_for_mode(&runtime_value_error_text_from_vm(vm, value), mode)
}

pub fn runtime_value_error_lines_with_registry(
    vm: &eldr::VM,
    value: &Value,
    sources: &SourceRegistry,
    source_id: SourceId,
    mode: ErrorDisplayMode,
) -> Vec<String> {
    lines_for_mode(
        &runtime_value_error_text_with_registry(vm, value, sources, source_id),
        mode,
    )
}

pub fn emit_runtime_value_error_from_vm(vm: &eldr::VM, value: &Value, mode: ErrorDisplayMode) {
    emit_text(&runtime_value_error_text_from_vm(vm, value), mode);
}

pub fn emit_runtime_value_error_with_registry(
    vm: &eldr::VM,
    value: &Value,
    sources: &SourceRegistry,
    source_id: SourceId,
    mode: ErrorDisplayMode,
) {
    emit_text(
        &runtime_value_error_text_with_registry(vm, value, sources, source_id),
        mode,
    );
}

#[cfg(test)]
mod tests {
    use super::{
        invalid_result_missing_payload_text, runtime_error_text, runtime_value_error_text_from_vm,
    };
    use eldr::{error::RuntimeErrorContext, VM};
    use sindr::ir::Bytecode;
    use sindr::runtime::{Location, RichError, Value};

    #[test]
    fn runtime_error_text_uses_runtimeerror_headline_with_message() {
        let err = eldr::RuntimeError::new("division by zero");
        let text = runtime_error_text(
            &err,
            Some("safe_mod(10, 0)"),
            Some("main.srt"),
            Some(Location {
                file: "main.srt".into(),
                func: "<runtime>".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 14,
            }),
        );
        assert!(text.contains("RuntimeError: division by zero"));
        assert!(text.contains("main.srt"));
    }

    #[test]
    fn runtime_value_error_text_includes_cause_chain_as_help() {
        let vm = VM::new(Bytecode::default()).with_source("main()".into(), "main.srt".into());
        let value = Value::Error(Box::new(RichError {
            kind: "Higher".into(),
            message: "higher".into(),
            location: Location {
                file: "main.srt".into(),
                func: "<runtime>".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 6,
            },
            cause: Some(Box::new(RichError {
                kind: "Lower".into(),
                message: "lower".into(),
                location: Location {
                    file: "main.srt".into(),
                    func: "<runtime>".into(),
                    line: 1,
                    column: 1,
                    span_start: 0,
                    span_end: 6,
                },
                cause: None,
            })),
        }));

        let text = runtime_value_error_text_from_vm(&vm, &value);
        assert!(text.contains("Higher: higher"));
        assert!(text.contains("Caused by: Lower: lower"));
    }

    #[test]
    fn runtime_error_text_fallback_keeps_context_help_when_verbose_is_enabled() {
        static ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        let env_lock = ENV_LOCK.get_or_init(|| std::sync::Mutex::new(()));
        let _guard = env_lock.lock().expect("lock should succeed");
        let previous = std::env::var("SURTR_VERBOSE_RUNTIME_ERROR").ok();
        std::env::set_var("SURTR_VERBOSE_RUNTIME_ERROR", "1");

        let err = eldr::RuntimeError::new("boom").with_context(RuntimeErrorContext {
            pc: Some(9),
            opcode: Some("AddInt".into()),
            function: Some("fun#1".into()),
            call_site: Some(Location {
                file: "main.srt".into(),
                func: "<runtime>".into(),
                line: 2,
                column: 3,
                span_start: 3,
                span_end: 5,
            }),
            details: vec!["stack_depth=1".into()],
        });
        let text = runtime_error_text(&err, None, None, None);
        assert!(text.contains("RuntimeError: boom"));
        assert!(text.contains("help: pc: 9"));
        assert!(text.contains("help: opcode: AddInt"));

        match previous {
            Some(value) => std::env::set_var("SURTR_VERBOSE_RUNTIME_ERROR", value),
            None => std::env::remove_var("SURTR_VERBOSE_RUNTIME_ERROR"),
        }
    }

    #[test]
    fn invalid_result_missing_payload_uses_diagnostic_shape_when_source_exists() {
        let text = invalid_result_missing_payload_text(
            Some("main()"),
            Some("main.srt"),
            Some(Location {
                file: "main.srt".into(),
                func: "<runtime>".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 6,
            }),
        );
        assert!(text.contains("InvalidResult: missing Err payload"));
        assert!(text.contains("main.srt"));
    }
}
