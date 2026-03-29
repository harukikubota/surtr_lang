use ariadne::{Color, Label, Report, ReportKind, Source};
use spire::ast::Span;

/// Render a source-attached diagnostic to stderr.
pub fn report_error(
    file_name: &str,
    source: &str,
    kind: &str,
    message: &str,
    span: &Span,
    hint: Option<&str>,
) {
    let mut builder = Report::build(ReportKind::Error, (file_name, span.start..span.end))
        .with_message(format!("{}: {}", kind, message))
        .with_label(
            Label::new((file_name, span.start..span.end))
                .with_message(message)
                .with_color(Color::Red),
        );

    if let Some(h) = hint {
        builder = builder.with_help(h);
    }

    builder
        .finish()
        .eprint((file_name, Source::from(source)))
        .unwrap();
}
