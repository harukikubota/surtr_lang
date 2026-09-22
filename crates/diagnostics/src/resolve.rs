use crate::{
    simple_error, Color, DiagnosticData, DiagnosticLabel, DiagnosticOrigin, DiagnosticReason,
    DiagnosticSpec, ResolveDiagnosticData, ResolveDiagnosticReason, SourceFact, SourceId,
    SourceRole, StructuredDiagnostic,
};
use spire::ast::Span;

/// Projects resolver-owned facts without recovering meaning from rendered text
/// or inspecting the source file.
pub fn resolve_error_spec(
    source_id: SourceId,
    message: impl Into<String>,
    span: Span,
    reason: ResolveDiagnosticReason,
    subject: Option<String>,
    labels: &[(SourceId, Span, String)],
) -> DiagnosticSpec {
    let message = message.into();
    let diagnostic = StructuredDiagnostic {
        reason: DiagnosticReason::Resolve(reason),
        origin: DiagnosticOrigin::Resolve,
        data: DiagnosticData::Resolve(ResolveDiagnosticData {
            detail: message,
            subject,
            related_labels: labels
                .iter()
                .map(|(_, _, message)| message.clone())
                .collect(),
        }),
        primary: SourceFact::untyped(SourceRole::Other, source_id, span),
        related: labels
            .iter()
            .map(|(source_id, span, _)| {
                SourceFact::untyped(SourceRole::Other, *source_id, span.clone())
            })
            .collect(),
        remediation: None,
    };
    structured_resolve_error_spec(&diagnostic)
}

pub(crate) fn structured_resolve_error_spec(input: &StructuredDiagnostic) -> DiagnosticSpec {
    let DiagnosticData::Resolve(data) = &input.data else {
        panic!("resolve renderer requires resolver diagnostic data");
    };
    assert_eq!(
        input.related.len(),
        data.related_labels.len(),
        "resolver label metadata must be complete"
    );
    let source_id = input.primary.source_id;
    let mut spec = simple_error(
        "ResolveError",
        data.detail.clone(),
        input.primary.span.clone(),
        None,
    );
    spec.structured = Some(input.clone());
    spec.labels.extend(
        input
            .related
            .iter()
            .zip(&data.related_labels)
            .enumerate()
            .map(|(index, (fact, message))| DiagnosticLabel {
                source_id: (fact.source_id != source_id).then_some(fact.source_id),
                span: fact.span.clone(),
                message: message.clone(),
                color: Some(resolve_related_label_color(index)),
            }),
    );
    spec
}

/// Stable label palette based only on producer-supplied label ordering.
pub fn resolve_related_label_color(index: usize) -> Color {
    match index % 5 {
        0 => Color::Red,
        1 => Color::Yellow,
        2 => Color::Blue,
        3 => Color::Magenta,
        _ => Color::Cyan,
    }
}
