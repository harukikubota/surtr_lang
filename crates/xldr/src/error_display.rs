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

pub fn runtime_error_text(
    err: &eldr::RuntimeError,
    source: Option<&str>,
    fallback_file: Option<&str>,
    location: Option<sindr::runtime::Location>,
) -> String {
    eldr::render_runtime_error_report(
        err,
        source,
        fallback_file,
        location,
        runtime_error_verbose_enabled(),
    )
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
        None,
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
