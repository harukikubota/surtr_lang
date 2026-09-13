use crate::{
    simple_error, DiagnosticData, DiagnosticLabel, DiagnosticOrigin, DiagnosticSpec, RuntimeData,
    RuntimeDiagnosticContext, RuntimeDiagnosticReason, SourceFact, SourceId, SourceRegistry,
    SourceRole, StructuredDiagnostic,
};
use sindr::runtime::RuntimeErrorDiagnostic;
use spire::ast::Span;

pub fn runtime_value_error_spec(
    source_id: SourceId,
    kind: impl Into<String>,
    message: impl Into<String>,
    span_start: usize,
    span_end: usize,
    diagnostic: Option<&RuntimeErrorDiagnostic>,
    help: Option<String>,
) -> DiagnosticSpec {
    let span = normalized_span(span_start, span_end);
    let kind = kind.into();
    let message = message.into();
    let (reason, detail, lhs, rhs, labels, notes) = match diagnostic {
        Some(RuntimeErrorDiagnostic::LiteralPatternMismatch { lhs, rhs }) => (
            RuntimeDiagnosticReason::ValuePatternMismatch,
            "literal pattern must equal the SafeBind input".to_string(),
            Some(lhs.clone()),
            Some(rhs.clone()),
            vec![
                DiagnosticLabel {
                    source_id: Some(source_id),
                    span: span.clone(),
                    message: format!("LHS value: {lhs}"),
                    color: None,
                },
                DiagnosticLabel {
                    source_id: Some(source_id),
                    span: span.clone(),
                    message: "SafeBind partial match".into(),
                    color: None,
                },
                DiagnosticLabel {
                    source_id: Some(source_id),
                    span: span.clone(),
                    message: format!("RHS value: {rhs}"),
                    color: None,
                },
            ],
            Vec::new(),
        ),
        Some(RuntimeErrorDiagnostic::SafeBindPatternFailure { rule, input_source }) => {
            let mut notes = vec![format!("expected rule: {rule}")];
            if let Some(input_source) = input_source {
                notes.push(format!("input source: {input_source}"));
            }
            (
                RuntimeDiagnosticReason::ValuePatternMismatch,
                rule.clone(),
                None,
                None,
                vec![DiagnosticLabel {
                    source_id: Some(source_id),
                    span: span.clone(),
                    message: "SafeBind partial match".into(),
                    color: None,
                }],
                notes,
            )
        }
        None => (
            RuntimeDiagnosticReason::ValueFailure,
            message.clone(),
            None,
            None,
            Vec::new(),
            Vec::new(),
        ),
    };
    let failure_kind = kind.clone();
    runtime_spec(
        source_id,
        kind,
        message,
        span,
        reason,
        RuntimeData {
            detail,
            failure_kind: Some(failure_kind),
            opcode: None,
            function: None,
            lhs,
            rhs,
        },
        labels,
        notes,
        help,
    )
}

pub fn runtime_error_spec(
    source_id: SourceId,
    message: impl Into<String>,
    span: Span,
    context: &RuntimeDiagnosticContext,
    help: Option<String>,
) -> DiagnosticSpec {
    let message = message.into();
    let mut notes = Vec::new();
    if let Some(opcode) = &context.opcode {
        notes.push(format!("opcode: {opcode}"));
    }
    if let Some(function) = &context.function {
        notes.push(format!("function: {function}"));
    }
    notes.extend(
        context
            .details
            .iter()
            .map(|detail| format!("detail: {detail}")),
    );
    runtime_spec(
        source_id,
        "RuntimeError".to_string(),
        message.clone(),
        span,
        context.reason,
        RuntimeData {
            detail: message,
            failure_kind: Some(context.reason.as_str().into()),
            opcode: context.opcode.clone(),
            function: context.function.clone(),
            lhs: None,
            rhs: None,
        },
        Vec::new(),
        notes,
        help,
    )
}

pub fn runtime_error_spec_by_id(
    _sources: &SourceRegistry,
    source_id: SourceId,
    message: impl Into<String>,
    span: Span,
    context: &RuntimeDiagnosticContext,
    help: Option<String>,
) -> DiagnosticSpec {
    runtime_error_spec(source_id, message, span, context, help)
}

fn normalized_span(start: usize, end: usize) -> Span {
    Span {
        start,
        end: if end <= start {
            start.saturating_add(1)
        } else {
            end
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn runtime_spec(
    source_id: SourceId,
    kind: String,
    message: String,
    span: Span,
    reason: RuntimeDiagnosticReason,
    data: RuntimeData,
    labels: Vec<DiagnosticLabel>,
    notes: Vec<String>,
    help: Option<String>,
) -> DiagnosticSpec {
    let primary = SourceFact::untyped(SourceRole::Pattern, source_id, span.clone());
    let structured = StructuredDiagnostic {
        reason: reason.into(),
        origin: DiagnosticOrigin::Runtime,
        data: DiagnosticData::Runtime(data),
        primary,
        related: Vec::new(),
        remediation: help.clone().map(|text| crate::Remediation::Help { text }),
    };
    let mut spec = simple_error(kind, message, span, help);
    spec.labels = labels;
    spec.notes = notes;
    spec.structured = Some(structured);
    spec
}
