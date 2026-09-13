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
    let mut spec = simple_error("ResolveError", message.clone(), span.clone(), None);
    spec.structured = Some(StructuredDiagnostic {
        reason: DiagnosticReason::Resolve(reason),
        origin: DiagnosticOrigin::Resolve,
        data: DiagnosticData::Resolve(ResolveDiagnosticData {
            detail: message,
            subject,
        }),
        primary: SourceFact::untyped(SourceRole::Other, source_id, span),
        related: labels
            .iter()
            .map(|(source_id, span, _)| {
                SourceFact::untyped(SourceRole::Other, *source_id, span.clone())
            })
            .collect(),
        remediation: None,
    });
    spec.labels.extend(labels.iter().enumerate().map(
        |(index, (label_source_id, span, message))| DiagnosticLabel {
            source_id: (*label_source_id != source_id).then_some(*label_source_id),
            span: span.clone(),
            message: message.clone(),
            color: Some(resolve_related_label_color(index)),
        },
    ));
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
