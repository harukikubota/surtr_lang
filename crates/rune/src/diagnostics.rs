use ariadne::{Color, Label, Report, ReportKind, Source};
use scar::error::TypeError;
use spire::ast::Span;

#[derive(Debug, Clone)]
pub struct DiagnosticSpec {
    pub kind: String,
    pub message: String,
    pub primary_span: Span,
    pub labels: Vec<DiagnosticLabel>,
    pub help: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DiagnosticLabel {
    pub span: Span,
    pub message: String,
    pub color: Color,
}

pub fn simple_error(
    kind: impl Into<String>,
    message: impl Into<String>,
    span: Span,
    help: Option<String>,
) -> DiagnosticSpec {
    DiagnosticSpec {
        kind: kind.into(),
        message: message.into(),
        primary_span: span,
        labels: Vec::new(),
        help,
    }
}

pub fn type_error_spec(source: &str, error: &TypeError) -> DiagnosticSpec {
    let mut spec = simple_error(
        "TypeError",
        error.message.clone(),
        error.span.clone(),
        error.hint.clone(),
    );

    if let Some(template) = infer_type_error_template(source, &error.span) {
        spec.labels.extend(template.labels);
        if let Some(help) = template.help {
            spec.help = Some(match spec.help.take() {
                Some(existing) => format!("{}\n{}", existing, help),
                None => help,
            });
        }
    }

    spec
}

pub fn report_error(file_name: &str, source: &str, spec: DiagnosticSpec) {
    let mut builder = Report::build(
        ReportKind::Error,
        (file_name, spec.primary_span.start..spec.primary_span.end),
    )
    .with_message(format!("{}: {}", spec.kind, spec.message))
    .with_label(
        Label::new((file_name, spec.primary_span.start..spec.primary_span.end))
            .with_message(&spec.message)
            .with_color(Color::Red),
    );

    for label in spec.labels {
        builder = builder.with_label(
            Label::new((file_name, label.span.start..label.span.end))
                .with_message(label.message)
                .with_color(label.color),
        );
    }

    if let Some(h) = spec.help {
        builder = builder.with_help(h);
    }

    builder
        .finish()
        .eprint((file_name, Source::from(source)))
        .unwrap();
}

#[derive(Debug, Clone)]
struct TemplateSpec {
    labels: Vec<DiagnosticLabel>,
    help: Option<String>,
}

fn infer_type_error_template(source: &str, focus: &Span) -> Option<TemplateSpec> {
    let lines = line_spans(source);
    let focus_line = line_index_for_span(&lines, focus.start)?;

    if let Some((decl_line, close_line)) = enclosing_def_lines(source, &lines, focus_line) {
        let decl_text = slice_chars(source, decl_line.0, decl_line.1).trim().to_string();
        let mut labels = vec![DiagnosticLabel {
            span: Span {
                start: decl_line.0,
                end: decl_line.1,
            },
            message: decl_text,
            color: Color::Blue,
        }];

        labels.push(DiagnosticLabel {
            span: Span {
                start: close_line.0,
                end: close_line.1,
            },
            message: "function body ends here".into(),
            color: Color::Yellow,
        });

        return Some(TemplateSpec { labels, help: None });
    }

    if let Some(call_name) = call_name_at_span(source, &lines, focus) {
        if let Some(sig_line) = find_function_signature_line(source, &lines, &call_name) {
            let sig_text = slice_chars(source, sig_line.0, sig_line.1).trim().to_string();
            return Some(TemplateSpec {
                labels: vec![DiagnosticLabel {
                    span: Span {
                        start: sig_line.0,
                        end: sig_line.1,
                    },
                    message: sig_text,
                    color: Color::Blue,
                }],
                help: None,
            });
        }
    }

    None
}

fn enclosing_def_lines(
    source: &str,
    lines: &[(usize, usize)],
    focus_line: usize,
) -> Option<((usize, usize), (usize, usize))> {
    let mut decl_idx = None;
    for idx in (0..=focus_line).rev() {
        let text = slice_chars(source, lines[idx].0, lines[idx].1);
        if text.trim_start().starts_with("def ") {
            decl_idx = Some(idx);
            break;
        }
    }
    let decl_idx = decl_idx?;

    let mut close_idx = None;
    for idx in (focus_line + 1)..lines.len() {
        let text = slice_chars(source, lines[idx].0, lines[idx].1);
        let trimmed = text.trim();
        if trimmed.starts_with("def ") {
            break;
        }
        if trimmed == "}" {
            close_idx = Some(idx);
        }
    }
    let close_idx = close_idx?;

    Some((lines[decl_idx], lines[close_idx]))
}

fn call_name_at_span(source: &str, lines: &[(usize, usize)], focus: &Span) -> Option<String> {
    let line_idx = line_index_for_span(lines, focus.start)?;
    let (line_start, line_end) = lines[line_idx];
    let line = slice_chars(source, line_start, line_end);
    let open_paren = line.rfind('(')?;
    let before = line[..open_paren].trim_end();
    let mut chars = before.chars().rev();
    let mut name_rev = String::new();

    for ch in chars.by_ref() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            name_rev.push(ch);
        } else if !name_rev.is_empty() {
            break;
        }
    }

    if name_rev.is_empty() {
        return None;
    }

    Some(name_rev.chars().rev().collect())
}

fn find_function_signature_line(
    source: &str,
    lines: &[(usize, usize)],
    name: &str,
) -> Option<(usize, usize)> {
    let needle = format!("def {}(", name);
    for &(start, end) in lines {
        let text = slice_chars(source, start, end);
        if text.trim_start().starts_with(&needle) {
            return Some((start, end));
        }
    }
    None
}

fn line_index_for_span(lines: &[(usize, usize)], pos: usize) -> Option<usize> {
    lines
        .iter()
        .position(|(start, end)| pos >= *start && pos <= *end)
}

fn line_spans(source: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = source.chars().collect();
    let mut spans = Vec::new();
    let mut start = 0usize;

    for (idx, ch) in chars.iter().enumerate() {
        if *ch == '\n' {
            spans.push((start, idx));
            start = idx + 1;
        }
    }

    spans.push((start, chars.len()));
    spans
}

fn slice_chars(source: &str, start: usize, end: usize) -> String {
    source
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}
