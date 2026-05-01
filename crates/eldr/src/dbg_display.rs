use ariadne::{Color, Label, Report, ReportKind};
use sindr::ir::DbgTemplate;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DbgRenderArg {
    pub(crate) span_start: u32,
    pub(crate) span_end: u32,
    pub(crate) label: String,
}

pub(crate) fn render_dbg_report(
    file_name: &str,
    source: &str,
    template: &DbgTemplate,
    args: &[DbgRenderArg],
) -> String {
    if source.is_empty() {
        return render_fallback(file_name, template, args);
    }

    let source_id = file_name.to_string();
    let primary_range = normalized_byte_range(
        source,
        template.span_start as usize,
        template.span_end as usize,
    );
    let mut builder = Report::build(
        ReportKind::Custom("Debug", Color::Cyan),
        (source_id.clone(), primary_range.clone()),
    )
    .with_message("inspect values.");

    let colors = [
        Color::Blue,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::Cyan,
    ];
    for (index, arg) in args.iter().enumerate() {
        let range = normalized_byte_range(source, arg.span_start as usize, arg.span_end as usize);
        builder = builder.with_label(
            Label::new((source_id.clone(), range))
                .with_message(arg.label.clone())
                .with_color(colors[index % colors.len()]),
        );
    }

    let mut buf = Vec::new();
    let cache = ariadne::sources([(source_id, source.to_string())]);
    if builder.finish().write(cache, &mut buf).is_err() {
        return render_fallback(file_name, template, args);
    }

    String::from_utf8_lossy(&buf).into_owned()
}

fn render_fallback(file_name: &str, template: &DbgTemplate, args: &[DbgRenderArg]) -> String {
    let mut lines = vec![format!(
        "[{file_name}:{}:{}]",
        template.span_start.saturating_add(1),
        template.span_end.saturating_add(1)
    )];
    lines.extend(args.iter().map(|arg| arg.label.clone()));
    lines.join("\n")
}

fn normalized_byte_range(source: &str, start: usize, end: usize) -> Range<usize> {
    let source_len = source.chars().count();
    if source_len == 0 {
        return 0..0;
    }

    let clamped_start = start.min(source_len.saturating_sub(1));
    let mut clamped_end = end.min(source_len);
    if clamped_end <= clamped_start {
        clamped_end = (clamped_start + 1).min(source_len);
    }

    char_span_to_byte_range(source, clamped_start, clamped_end)
}

fn char_span_to_byte_range(source: &str, start: usize, end: usize) -> Range<usize> {
    let start_byte = char_offset_to_byte(source, start);
    let end_byte = char_offset_to_byte(source, end);
    start_byte..end_byte.max(start_byte + 1).min(source.len())
}

fn char_offset_to_byte(source: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }

    source
        .char_indices()
        .nth(offset)
        .map(|(idx, _)| idx)
        .unwrap_or(source.len())
}
