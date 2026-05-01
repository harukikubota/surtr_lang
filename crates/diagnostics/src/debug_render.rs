use crate::heuristics::char_span_to_byte_range;
use crate::Color;
use ariadne::{Label, Report, ReportKind};
use spire::ast::Span;
use std::io::{self, Write};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugLabel {
    pub span_start: u32,
    pub span_end: u32,
    pub message: String,
    pub color: Option<Color>,
}

pub fn render_debug_report(
    file_name: &str,
    source: &str,
    kind: &'static str,
    message: &str,
    span_start: u32,
    span_end: u32,
    labels: &[DebugLabel],
) -> String {
    if source.is_empty() {
        return render_fallback(file_name, kind, message, span_start, span_end, labels);
    }

    let report = build_report(file_name, source, kind, message, span_start, span_end, labels);
    let mut buf = Vec::new();
    let cache = ariadne::sources([(file_name.to_string(), source.to_string())]);
    if let Err(err) = report.write(cache, &mut buf) {
        let _ = write_fallback_debug(
            &mut buf, file_name, kind, message, span_start, span_end, labels, &err,
        );
    }

    String::from_utf8_lossy(&buf).into_owned()
}

fn build_report(
    file_name: &str,
    source: &str,
    kind: &'static str,
    message: &str,
    span_start: u32,
    span_end: u32,
    labels: &[DebugLabel],
) -> Report<'static, (String, std::ops::Range<usize>)> {
    let primary_span = Span {
        start: span_start as usize,
        end: span_end as usize,
    };
    let primary_range = char_span_to_byte_range(source, &primary_span);
    let mut builder = Report::build(
        ReportKind::Custom(kind, Color::Cyan),
        (file_name.to_string(), primary_range),
    )
    .with_message(message.to_string());

    for label in labels {
        let range = char_span_to_byte_range(
            source,
            &Span {
                start: label.span_start as usize,
                end: label.span_end as usize,
            },
        );
        builder = builder.with_label(match label.color {
            Some(color) => Label::new((file_name.to_string(), range))
                .with_message(label.message.clone())
                .with_color(color),
            None => Label::new((file_name.to_string(), range)).with_message(label.message.clone()),
        });
    }

    builder.finish()
}

fn render_fallback(
    file_name: &str,
    kind: &'static str,
    message: &str,
    span_start: u32,
    span_end: u32,
    labels: &[DebugLabel],
) -> String {
    let mut buf = Vec::new();
    let _ = write_fallback_lines(
        &mut buf, file_name, kind, message, span_start, span_end, labels,
    );
    String::from_utf8_lossy(&buf).into_owned()
}

fn write_fallback_debug(
    writer: &mut impl Write,
    file_name: &str,
    kind: &'static str,
    message: &str,
    span_start: u32,
    span_end: u32,
    labels: &[DebugLabel],
    render_err: &io::Error,
) -> io::Result<()> {
    writeln!(writer, "debug rendering failed: {}", render_err)?;
    write_fallback_lines(
        writer, file_name, kind, message, span_start, span_end, labels,
    )
}

fn write_fallback_lines(
    writer: &mut impl Write,
    file_name: &str,
    kind: &'static str,
    message: &str,
    span_start: u32,
    span_end: u32,
    labels: &[DebugLabel],
) -> io::Result<()> {
    writeln!(writer, "{}: {}", kind, message)?;
    writeln!(writer, "--> {}:{}-{}", file_name, span_start, span_end)?;
    for label in labels {
        writeln!(
            writer,
            "= note: {} [{}-{}]",
            label.message, label.span_start, label.span_end
        )?;
    }
    Ok(())
}
