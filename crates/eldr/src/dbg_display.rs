use diagnostics::{render_debug_report, Color, DebugLabel};
use sindr::ir::DbgTemplate;

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
    let colors = [
        Color::Blue,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::Cyan,
    ];
    let labels = args
        .iter()
        .enumerate()
        .map(|(index, arg)| DebugLabel {
            span_start: arg.span_start,
            span_end: arg.span_end,
            message: arg.label.clone(),
            color: Some(colors[index % colors.len()]),
        })
        .collect::<Vec<_>>();

    render_debug_report(
        file_name,
        source,
        "Debug",
        "inspect values.",
        template.span_start,
        template.span_end,
        &labels,
    )
}
