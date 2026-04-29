use crate::{Color, DiagnosticSpec};
use ariadne::{Label, Report, ReportKind};
use spire::ast::Span;

pub fn surtr_assert_eq_error_spec(
    kind: impl Into<String>,
    message: impl Into<String>,
    call_span: Span,
    lhs_span: Span,
    rhs_span: Span,
    lhs_term: impl Into<String>,
    rhs_term: impl Into<String>,
) -> DiagnosticSpec {
    let message = message.into();
    DiagnosticSpec {
        kind: kind.into(),
        message: message.clone(),
        primary_span: call_span,
        labels: vec![
            crate::DiagnosticLabel {
                source_id: None,
                span: lhs_span,
                message: format!("LHS term: {}", lhs_term.into()),
                color: Some(Color::Blue),
            },
            crate::DiagnosticLabel {
                source_id: None,
                span: rhs_span,
                message: format!("RHS term: {}", rhs_term.into()),
                color: Some(Color::Yellow),
            },
        ],
        notes: Vec::new(),
        help: Some(format!("assert_eq failed: {}", message)),
    }
}

pub fn render_surtr_code_error(file_name: &str, source: &str, spec: &DiagnosticSpec) -> String {
    let source_id = file_name.to_string();
    let primary = normalized_surtr_span(source, &spec.primary_span);
    let primary_range = primary.start..primary.end;
    let mut builder = Report::build(ReportKind::Error, (source_id.clone(), primary_range))
        .with_message(format!("{}: {}", spec.kind, spec.message));

    for diagnostic_label in &spec.labels {
        let span = normalized_surtr_span(source, &diagnostic_label.span);
        let range = span.start..span.end;
        let label =
            Label::new((source_id.clone(), range)).with_message(diagnostic_label.message.clone());
        builder = match diagnostic_label.color {
            Some(color) => builder.with_label(label.with_color(color)),
            None => builder.with_label(label),
        };
    }

    for note in &spec.notes {
        builder = builder.with_note(note.clone());
    }

    if let Some(help) = &spec.help {
        builder = builder.with_help(help.clone());
    }

    let mut buf = Vec::new();
    let cache = ariadne::sources([(source_id, source.to_string())]);
    if let Err(err) = builder.finish().write(cache, &mut buf) {
        let _ = crate::render::write_fallback_diagnostic(&mut buf, file_name, spec, &err);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn normalized_surtr_span(source: &str, span: &Span) -> Span {
    let source_len = source.chars().count();
    if source_len == 0 {
        return Span { start: 0, end: 0 };
    }
    let start = span.start.min(source_len.saturating_sub(1));
    let mut end = span.end.min(source_len);
    if end <= start {
        end = (start + 1).min(source_len);
    }
    Span { start, end }
}
