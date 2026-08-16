use crate::heuristics::{
    apply_runtime_provenance_by_id, infer_runtime_error_template,
    infer_runtime_value_error_template, split_runtime_literal_values,
};
use crate::{simple_error, DiagnosticSpec, RuntimeDiagnosticContext, SourceId, SourceRegistry};
use spire::ast::Span;

pub fn runtime_value_error_spec(
    source: &str,
    kind: impl Into<String>,
    message: impl Into<String>,
    span_start: usize,
    span_end: usize,
    help: Option<String>,
) -> DiagnosticSpec {
    let mut span = Span {
        start: span_start,
        end: span_end,
    };
    if span.end <= span.start {
        span.end = span.start.saturating_add(1);
    }

    let kind = kind.into();
    let raw_message = message.into();
    let (message, literal_values) = split_runtime_literal_values(&raw_message);
    let mut spec = simple_error(kind.clone(), message.clone(), span.clone(), help);
    if let Some(template) =
        infer_runtime_value_error_template(source, &span, &kind, &raw_message, literal_values)
    {
        if let Some(primary) = template
            .labels
            .iter()
            .find(|label| label.message != "SafeBind partial match")
        {
            spec.primary_span = primary.span.clone();
        }
        spec.labels.extend(template.labels);
        spec.notes.extend(template.notes);
        if let Some(help) = template.help {
            spec.help = Some(help);
        }
    }
    spec
}

pub fn runtime_error_spec(
    source: &str,
    message: impl Into<String>,
    span: Span,
    context: &RuntimeDiagnosticContext,
    help: Option<String>,
) -> DiagnosticSpec {
    let message = message.into();
    let mut spec = simple_error("RuntimeError", message.clone(), span.clone(), help);
    if let Some(template) = infer_runtime_error_template(source, &span, &message, context) {
        if let Some(primary) = template
            .labels
            .iter()
            .find(|label| label.message != "call target")
        {
            spec.primary_span = primary.span.clone();
        }
        spec.labels.extend(template.labels);
        spec.notes.extend(template.notes);
        if let Some(help) = template.help {
            spec.help = Some(match spec.help.take() {
                Some(existing) => format!("{}\n{}", existing, help),
                None => help,
            });
        }
    }
    spec
}

pub fn runtime_error_spec_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    message: impl Into<String>,
    span: Span,
    context: &RuntimeDiagnosticContext,
    help: Option<String>,
) -> DiagnosticSpec {
    let source = sources.source(source_id).unwrap_or("");
    let mut spec = runtime_error_spec(source, message, span, context, help);
    apply_runtime_provenance_by_id(sources, source_id, context, &mut spec);
    spec
}
