use ariadne::{Color, Fmt, Label, Report, ReportKind};
use scar::error::TypeError;
use serde::{Deserialize, Serialize};
use spire::ast::Span;
use std::fmt;
use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    pub id: SourceId,
    pub file_name: String,
    pub source: String,
}

#[derive(Debug, Default, Clone)]
pub struct SourceRegistry {
    entries: Vec<SourceEntry>,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        file_name: impl Into<String>,
        source: impl Into<String>,
    ) -> SourceId {
        let id = SourceId(self.entries.len() as u32);
        self.entries.push(SourceEntry {
            id,
            file_name: file_name.into(),
            source: source.into(),
        });
        id
    }

    pub fn get(&self, source_id: SourceId) -> Option<&SourceEntry> {
        self.entries.get(source_id.0 as usize)
    }

    pub fn file_name(&self, source_id: SourceId) -> Option<&str> {
        self.get(source_id).map(|entry| entry.file_name.as_str())
    }

    pub fn source(&self, source_id: SourceId) -> Option<&str> {
        self.get(source_id).map(|entry| entry.source.as_str())
    }

    pub fn update_source(&mut self, source_id: SourceId, source: impl Into<String>) -> bool {
        if let Some(entry) = self.entries.get_mut(source_id.0 as usize) {
            entry.source = source.into();
            true
        } else {
            false
        }
    }

    pub fn owned_context(&self, source_id: SourceId) -> Option<(String, String)> {
        self.get(source_id)
            .map(|entry| (entry.source.clone(), entry.file_name.clone()))
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializableDiagnostic {
    pub kind: String,
    pub phase: String,
    pub line: u32,
    pub column: u32,
    pub span: [u32; 2],
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub got: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SerializableDiagnosticReport {
    pub errors: Vec<SerializableDiagnostic>,
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

pub fn parse_error_spec(source: &str, message: impl Into<String>, span: Span) -> DiagnosticSpec {
    let message = message.into();
    let mut spec = simple_error("ParseError", message.clone(), span.clone(), None);

    if message.starts_with("Unexpected token:") {
        spec.help = Some(
            "The parser stopped at this token. Check the expression immediately before it.".into(),
        );
    }

    if let Some(token) = message.strip_prefix("Unexpected token: ") {
        if let Some(line_span) = line_span_containing(source, span.start) {
            let line = slice_chars(source, line_span.0, line_span.1);
            let token_hint = match token {
                "RParen" => "unexpected closing parenthesis",
                "RBrace" => "unexpected closing brace",
                "RBracket" => "unexpected closing bracket",
                _ => "unexpected token",
            };
            if !line.trim().is_empty() {
                spec.labels.push(DiagnosticLabel {
                    span,
                    message: token_hint.into(),
                    color: Color::Yellow,
                });
            }
        }
    }

    spec
}

pub fn resolve_error_spec(source: &str, message: impl Into<String>, span: Span) -> DiagnosticSpec {
    let message = message.into();
    let mut spec = simple_error("ResolveError", message.clone(), span.clone(), None);

    if let Some(name) = message.strip_prefix("Undefined variable: ") {
        spec.help = Some(format!(
            "`{}` is not defined in the current scope. Define it or import it before use.",
            name
        ));
        if let Some(name_span) = identifier_span_at(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                span: name_span,
                message: format!("unresolved name `{}`", name),
                color: Color::Yellow,
            });
        }
    }

    spec
}

pub fn type_error_spec(source: &str, error: &TypeError) -> DiagnosticSpec {
    let mut spec = simple_error(
        "TypeError",
        error.message.clone(),
        error.span.clone(),
        error.hint.clone(),
    );

    let replace_help = is_flow_operator_message(&error.message);
    if let Some(template) =
        infer_type_error_template(source, &error.span, &error.message, error.hint.as_deref())
    {
        spec.labels.extend(template.labels);
        if let Some(help) = template.help {
            spec.help = Some(if replace_help {
                help
            } else {
                match spec.help.take() {
                    Some(existing) => format!("{}\n{}", existing, help),
                    None => help,
                }
            });
        }
    }
    if spec.help.is_none() {
        if let (Some(expected), Some(got)) = extract_expected_got(&spec.message) {
            spec.help = Some(format!(
                "This location requires {}, but the expression currently has {}.",
                expected, got
            ));
        }
    }
    if spec
        .help
        .as_deref()
        .is_some_and(|help| callable_definition_signature_from_hint(help).is_some())
    {
        spec.help = None;
    }

    spec
}

pub fn type_error_spec_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    error: &TypeError,
) -> DiagnosticSpec {
    type_error_spec(sources.source(source_id).unwrap_or(""), error)
}

pub fn report_error(file_name: &str, source: &str, spec: DiagnosticSpec) {
    let report = build_report(file_name, source, &spec);
    let cache = ariadne::sources([
        (
            RenderSourceId::Primary(file_name.to_string()),
            source.to_string(),
        ),
        (
            RenderSourceId::Related(file_name.to_string()),
            source.to_string(),
        ),
    ]);

    if let Err(err) = report.eprint(cache) {
        let mut stderr = io::stderr().lock();
        let _ = write_fallback_diagnostic(&mut stderr, file_name, &spec, &err);
    }
}

pub fn render_error(file_name: &str, source: &str, spec: &DiagnosticSpec) -> String {
    let report = build_report(file_name, source, spec);
    let mut buf = Vec::new();
    let cache = ariadne::sources([
        (
            RenderSourceId::Primary(file_name.to_string()),
            source.to_string(),
        ),
        (
            RenderSourceId::Related(file_name.to_string()),
            source.to_string(),
        ),
    ]);

    if let Err(err) = report.write(cache, &mut buf) {
        let _ = write_fallback_diagnostic(&mut buf, file_name, spec, &err);
    }

    String::from_utf8_lossy(&buf).into_owned()
}

pub fn report_error_by_id(sources: &SourceRegistry, source_id: SourceId, spec: DiagnosticSpec) {
    if let Some(entry) = sources.get(source_id) {
        report_error(&entry.file_name, &entry.source, spec);
    } else {
        report_error("<unknown>", "", spec);
    }
}

pub fn render_error_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    spec: &DiagnosticSpec,
) -> String {
    if let Some(entry) = sources.get(source_id) {
        render_error(&entry.file_name, &entry.source, spec)
    } else {
        render_error("<unknown>", "", spec)
    }
}

fn build_report(
    file_name: &str,
    source: &str,
    spec: &DiagnosticSpec,
) -> Report<'static, (RenderSourceId, std::ops::Range<usize>)> {
    let primary = normalized_char_span(source, &spec.primary_span);
    let primary_range = char_span_to_byte_range(source, &primary);
    let lines = line_spans(source);
    let primary_line = line_index_for_span(&lines, primary.start);
    let suppress_primary_label = spec.kind == "TypeError"
        && (is_flow_operator_message(&spec.message) || has_annotation_assignment_labels(spec));
    let primary_source = RenderSourceId::Primary(file_name.to_string());
    let related_source = RenderSourceId::Related(file_name.to_string());
    let mut builder = Report::build(
        ReportKind::Error,
        (primary_source.clone(), primary_range.clone()),
    )
    .with_message(format!("{}: {}", spec.kind, spec.message));

    if !suppress_primary_label {
        builder = builder.with_label(
            Label::new((primary_source.clone(), primary_range))
                .with_message(spec.message.clone())
                .with_color(Color::Red),
        );
    }

    for label in &spec.labels {
        let span = normalized_char_span(source, &label.span);
        let range = char_span_to_byte_range(source, &span);
        let label_source =
            if should_render_related_label_with_own_source(spec, primary_line, &lines, &span) {
                related_source.clone()
            } else {
                primary_source.clone()
            };
        builder = builder.with_label(
            Label::new((label_source, range))
                .with_message(label.message.clone())
                .with_color(label.color),
        );
    }

    if let Some(h) = &spec.help {
        builder = builder.with_help(h.clone());
    }
    if spec.kind == "TypeError" && is_flow_operator_message(&spec.message) {
        builder = builder.with_note(format!("Reason: {}", spec.message));
    }

    builder.finish()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RenderSourceId {
    Primary(String),
    Related(String),
}

impl fmt::Display for RenderSourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderSourceId::Primary(file_name) | RenderSourceId::Related(file_name) => {
                f.write_str(file_name)
            }
        }
    }
}

fn should_render_related_label_with_own_source(
    spec: &DiagnosticSpec,
    primary_line: Option<usize>,
    lines: &[(usize, usize)],
    span: &Span,
) -> bool {
    spec.kind == "TypeError"
        && spec
            .message
            .starts_with("Argument type mismatch: expected ")
        && line_index_for_span(lines, span.start) != primary_line
}

fn normalized_char_span(source: &str, span: &Span) -> Span {
    let source_len = source.chars().count();
    if source_len == 0 {
        return Span { start: 0, end: 0 };
    }

    let mut start = span.start.min(source_len.saturating_sub(1));
    let mut end = span.end.min(source_len);
    if end <= start {
        end = (start + 1).min(source_len);
    }
    if end <= start {
        start = 0;
        end = 1.min(source_len);
    }

    Span { start, end }
}

fn char_span_to_byte_range(source: &str, span: &Span) -> std::ops::Range<usize> {
    let normalized = normalized_char_span(source, span);
    char_offset_to_byte_offset(source, normalized.start)
        ..char_offset_to_byte_offset(source, normalized.end)
}

fn char_offset_to_byte_offset(source: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let char_len = source.chars().count();
    if offset >= char_len {
        return source.len();
    }
    source
        .char_indices()
        .nth(offset)
        .map(|(idx, _)| idx)
        .unwrap_or(source.len())
}

fn write_fallback_diagnostic(
    writer: &mut impl Write,
    file_name: &str,
    spec: &DiagnosticSpec,
    render_err: &io::Error,
) -> io::Result<()> {
    writeln!(writer, "diagnostic rendering failed: {}", render_err)?;
    writeln!(writer, "{}: {}", spec.kind, spec.message)?;
    writeln!(
        writer,
        "--> {}:{}-{}",
        file_name, spec.primary_span.start, spec.primary_span.end
    )?;
    for label in &spec.labels {
        writeln!(
            writer,
            "= note: {} [{}-{}]",
            label.message, label.span.start, label.span.end
        )?;
    }
    if let Some(help) = &spec.help {
        for line in help.lines() {
            writeln!(writer, "= help: {}", line)?;
        }
    }
    Ok(())
}

pub fn serializable_report_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    phase: impl Into<String>,
    spec: &DiagnosticSpec,
) -> SerializableDiagnosticReport {
    SerializableDiagnosticReport {
        errors: vec![serializable_diagnostic_by_id(
            sources, source_id, phase, spec,
        )],
    }
}

pub fn serializable_diagnostic_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    phase: impl Into<String>,
    spec: &DiagnosticSpec,
) -> SerializableDiagnostic {
    let phase = phase.into();
    let source = sources.source(source_id).unwrap_or("");
    let (line, column) = line_column_for_offset(source, spec.primary_span.start);
    let (expected, got) = extract_expected_got(&spec.message);
    SerializableDiagnostic {
        kind: spec.kind.clone(),
        phase,
        line,
        column,
        span: [spec.primary_span.start as u32, spec.primary_span.end as u32],
        message: spec.message.clone(),
        expected,
        got,
        hint: spec.help.clone(),
    }
}

#[derive(Debug, Clone)]
struct TemplateSpec {
    labels: Vec<DiagnosticLabel>,
    help: Option<String>,
}

fn infer_type_error_template(
    source: &str,
    focus: &Span,
    message: &str,
    hint: Option<&str>,
) -> Option<TemplateSpec> {
    let lines = line_spans(source);
    let focus_line = line_index_for_span(&lines, focus.start)?;

    if let Some(spec) = infer_if_branch_mismatch_template(source, &lines, focus, message) {
        return Some(spec);
    }
    if let Some(spec) = infer_match_arm_mismatch_template(source, focus, message) {
        return Some(spec);
    }
    if let Some(spec) = infer_operator_mismatch_template(source, &lines, focus, message) {
        return Some(spec);
    }
    if let Some(spec) = infer_flow_operator_template(source, &lines, focus, message, hint) {
        return Some(spec);
    }
    if let Some(spec) = infer_ensure_predicate_template(source, &lines, focus, message) {
        return Some(spec);
    }
    if let Some(spec) = infer_extractor_template(source, &lines, focus, message) {
        return Some(spec);
    }
    if let Some(spec) = infer_argument_mismatch_template(source, &lines, focus, message, hint) {
        return Some(spec);
    }
    if let Some(spec) = infer_annotation_assignment_template(source, &lines, focus, message) {
        return Some(spec);
    }

    if let Some((decl_line, close_line)) = enclosing_def_lines(source, &lines, focus_line) {
        let decl_text = slice_chars(source, decl_line.0, decl_line.1)
            .trim()
            .to_string();
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
            let sig_text = slice_chars(source, sig_line.0, sig_line.1)
                .trim()
                .to_string();
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

fn infer_operator_mismatch_template(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
    message: &str,
) -> Option<TemplateSpec> {
    let tail = message.strip_prefix("Cannot apply ")?;
    let (op_name, types) = tail.split_once(" to ")?;
    let (left_ty, right_ty) = types.split_once(" and ")?;
    let line_idx = line_index_for_span(lines, focus.start)?;
    let (line_start, line_end) = lines[line_idx];
    let line = slice_chars(source, line_start, line_end);
    let chars: Vec<char> = line.chars().collect();
    let focus_col = focus.start.saturating_sub(line_start);
    let op_span = find_backtick_operator_span(line_start, &chars, focus_col)
        .or_else(|| find_operator_symbol_span(line_start, &chars, op_name, focus_col))?;
    let lhs_start = find_assignment_eq_before(&chars, op_span.start - line_start)
        .map(|idx| idx + 1)
        .unwrap_or(0)
        .min(op_span.start - line_start);
    let (left_start, left_end) = trim_char_span(&chars, lhs_start, op_span.start - line_start);
    let (right_start, right_end) = trim_char_span(&chars, op_span.end - line_start, chars.len());

    Some(TemplateSpec {
        labels: vec![
            DiagnosticLabel {
                span: Span {
                    start: line_start + left_start,
                    end: line_start + left_end,
                },
                message: format!("left operand: {}", left_ty),
                color: Color::Blue,
            },
            DiagnosticLabel {
                span: op_span,
                message: format!("operator `{}`", op_name),
                color: Color::Magenta,
            },
            DiagnosticLabel {
                span: Span {
                    start: line_start + right_start,
                    end: line_start + right_end,
                },
                message: format!("right operand: {}", right_ty),
                color: Color::Yellow,
            },
        ],
        help: None,
    })
}

fn infer_flow_operator_template(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
    message: &str,
    hint: Option<&str>,
) -> Option<TemplateSpec> {
    let op = ["|>=", "|*>", "|>", ">>", ">*", ">=>"]
        .into_iter()
        .find(|op| message.contains(&format!("`{}`", op)))?;
    let line_idx = line_index_for_span(lines, focus.start)?;
    let (line_start, line_end) = lines[line_idx];
    let line = slice_chars(source, line_start, line_end);
    let chars: Vec<char> = line.chars().collect();
    let op_pattern: Vec<char> = op.chars().collect();
    let op_start = find_subslice_outside_literals(&chars, &op_pattern, 0)?;
    let op_end = op_start + op.chars().count();
    let lhs_start = find_assignment_eq_before(&chars, op_start)
        .map(|idx| idx + 1)
        .unwrap_or(0)
        .min(op_start);
    let (lhs_start, lhs_end) = trim_char_span(&chars, lhs_start, op_start);
    let (rhs_start, rhs_end) = trim_char_span(&chars, op_end, chars.len());
    let detail = hint.and_then(parse_operator_hint);
    let lhs_actual = detail
        .as_ref()
        .map(|detail| detail.lhs.as_str())
        .unwrap_or("unknown");
    let rhs_actual = detail
        .as_ref()
        .map(|detail| detail.rhs.as_str())
        .unwrap_or("unknown");
    let (lhs_expected, rhs_expected) = flow_operator_display_expectations(op, message);
    let (lhs_bad, rhs_bad) = flow_operator_mismatch_sides(op, message);
    let lowered_rule = lowered_flow_operator_rule(op, lhs_actual, rhs_actual, lhs_bad, rhs_bad);

    Some(TemplateSpec {
        labels: vec![
            DiagnosticLabel {
                span: Span {
                    start: line_start + lhs_start,
                    end: line_start + lhs_end,
                },
                message: flow_operand_label("LHS", lhs_actual, &lhs_expected, lhs_bad, message),
                color: Color::Blue,
            },
            DiagnosticLabel {
                span: Span {
                    start: line_start + op_start,
                    end: line_start + op_end,
                },
                message: format!("OP: {}", lowered_rule),
                color: Color::Yellow,
            },
            DiagnosticLabel {
                span: Span {
                    start: line_start + rhs_start,
                    end: line_start + rhs_end,
                },
                message: flow_operand_label("RHS", rhs_actual, &rhs_expected, rhs_bad, message),
                color: Color::Magenta,
            },
        ],
        help: Some(flow_operator_help(op).into()),
    })
}

fn infer_ensure_predicate_template(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
    message: &str,
) -> Option<TemplateSpec> {
    if !message.starts_with("ensure requires a closure or capture predicate") {
        return None;
    }
    let line_idx = line_index_for_span(lines, focus.start)?;
    let (line_start, line_end) = lines[line_idx];
    let line = slice_chars(source, line_start, line_end);
    let chars: Vec<char> = line.chars().collect();
    let call_start =
        find_subslice_outside_literals(&chars, &['e', 'n', 's', 'u', 'r', 'e', '('], 0)?;
    let arg_spans = collect_call_argument_spans(&chars, call_start + "ensure(".len())?;
    let predicate_span = arg_spans.get(1)?;

    Some(TemplateSpec {
        labels: vec![DiagnosticLabel {
            span: Span {
                start: line_start + predicate_span.0,
                end: line_start + predicate_span.1,
            },
            message: "predicate must be a closure or capture, not a call result".into(),
            color: Color::Yellow,
        }],
        help: None,
    })
}

fn infer_extractor_template(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
    message: &str,
) -> Option<TemplateSpec> {
    if !message.starts_with("Extractor ") {
        return None;
    }
    let line_idx = line_index_for_span(lines, focus.start)?;
    let (line_start, line_end) = lines[line_idx];
    let line = slice_chars(source, line_start, line_end);
    let chars: Vec<char> = line.chars().collect();
    let pattern_span = extract_match_pattern_span(&chars)?;

    Some(TemplateSpec {
        labels: vec![DiagnosticLabel {
            span: Span {
                start: line_start + pattern_span.0,
                end: line_start + pattern_span.1,
            },
            message: "extractor pattern checked against the match scrutinee".into(),
            color: Color::Yellow,
        }],
        help: None,
    })
}

fn infer_argument_mismatch_template(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
    message: &str,
    hint: Option<&str>,
) -> Option<TemplateSpec> {
    if !message.starts_with("Argument type mismatch: expected ") {
        return None;
    }
    let mut labels = Vec::new();
    if let Some((sig_text, sig_span)) = hint.and_then(callable_definition_from_hint) {
        let label_span = line_span_containing(source, sig_span.start)
            .map(|(start, end)| Span { start, end })
            .unwrap_or(sig_span);
        labels.push(DiagnosticLabel {
            span: label_span,
            message: sig_text.to_string(),
            color: Color::Blue,
        });
    } else if let Some(call_name) = call_name_at_span(source, lines, focus) {
        if let Some(sig_line) = find_function_signature_line(source, lines, &call_name) {
            if let Some(sig_text) = source_signature_caption(source, lines, sig_line, &call_name) {
                labels.push(DiagnosticLabel {
                    span: Span {
                        start: sig_line.0,
                        end: sig_line.1,
                    },
                    message: sig_text,
                    color: Color::Blue,
                });
            }
        }
    }
    if labels.is_empty() {
        return None;
    }
    Some(TemplateSpec { labels, help: None })
}

fn infer_annotation_assignment_template(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
    message: &str,
) -> Option<TemplateSpec> {
    if !message.starts_with("expected ") {
        return None;
    }
    let (Some(expected), Some(got)) = extract_expected_got(message) else {
        return None;
    };
    let focus_line = line_index_for_span(lines, focus.start)?;
    let assignment = find_annotated_assignment_line(source, lines, focus_line)?;
    let rhs_span = assignment_rhs_span(source, lines, focus_line, &assignment)?;

    Some(TemplateSpec {
        labels: vec![
            DiagnosticLabel {
                span: assignment.lhs_span,
                message: format!("LHS annotation: {}", expected),
                color: Color::Blue,
            },
            DiagnosticLabel {
                span: rhs_span,
                message: format!("RHS expression: {}", got),
                color: Color::Yellow,
            },
        ],
        help: None,
    })
}

#[derive(Debug, Clone)]
struct OperatorHintParts {
    lhs: String,
    rhs: String,
}

fn is_flow_operator_message(message: &str) -> bool {
    ["`|>`", "`|*>`", "`|>=`", "`>>`", "`>*`", "`>=>`"]
        .into_iter()
        .any(|op| message.contains(op))
}

fn has_annotation_assignment_labels(spec: &DiagnosticSpec) -> bool {
    let has_lhs = spec
        .labels
        .iter()
        .any(|label| label.message.starts_with("LHS annotation:"));
    let has_rhs = spec
        .labels
        .iter()
        .any(|label| label.message.starts_with("RHS expression:"));
    has_lhs && has_rhs
}

fn parse_operator_hint(hint: &str) -> Option<OperatorHintParts> {
    let (_, rest) = hint.split_once(". LHS: ")?;
    let (lhs, rest) = rest.split_once(". RHS: ")?;
    let rhs = rest
        .split_once(". Operators share precedence")
        .map(|(rhs, _)| rhs)
        .unwrap_or(rest.trim_end_matches('.'));
    Some(OperatorHintParts {
        lhs: lhs.trim().to_string(),
        rhs: rhs.trim().to_string(),
    })
}

fn flow_operator_expected_parts(op: &str) -> (&'static str, &'static str) {
    match op {
        "|>" => ("Evaluated", "Callable"),
        "|*>" => ("Container<A>", "(A -> B)"),
        "|>=" => ("Container<A>", "(A -> Container<B>)"),
        ">>" => ("(A -> B)", "(B -> C)"),
        ">*" => ("(A -> Container<B>)", "(B -> C)"),
        ">=>" => ("(A -> Container<B>)", "(B -> Container<C>)"),
        _ => ("LHS", "RHS"),
    }
}

fn flow_operator_display_expectations(op: &str, message: &str) -> (String, String) {
    let (lhs_default, rhs_default) = flow_operator_expected_parts(op);
    let (Some(expected), Some(_got)) = extract_expected_got(message) else {
        return (lhs_default.into(), rhs_default.into());
    };

    match op {
        "|>" => (expected, rhs_default.into()),
        "|*>" | "|>=" => (lhs_default.into(), format!("input {}", expected)),
        _ => (lhs_default.into(), rhs_default.into()),
    }
}

fn flow_operand_label(
    side: &str,
    actual: &str,
    expected: &str,
    is_mismatch: bool,
    message: &str,
) -> String {
    if !is_mismatch {
        return format!("{} actual: {} (expected {})", side, actual, expected);
    }

    let prefix = if message.contains("requires Result or List")
        || message.contains("requires matching Result or List")
    {
        "Container required"
    } else if message.contains("requires Result")
        || message.contains("requires matching Result")
        || message.contains("return Result")
    {
        "Result required"
    } else {
        "TypeMismatch"
    };
    format!(
        "{}: {} actual: {} (expected {})",
        prefix,
        side,
        actual.fg(Color::Red),
        expected.fg(Color::Red)
    )
}

fn lowered_flow_operator_rule(
    op: &str,
    lhs_actual: &str,
    rhs_actual: &str,
    lhs_bad: bool,
    rhs_bad: bool,
) -> String {
    let lhs_display = flow_type_display(lhs_actual, lhs_bad);
    let rhs_display = flow_type_display(rhs_actual, rhs_bad);
    let fallback = || format!("{} `{}` {} -> <type error>", lhs_display, op, rhs_display);

    match op {
        "|>" => {
            let out = unary_function_parts_display(rhs_actual)
                .map(|(_, out)| out)
                .unwrap_or_else(|| "Evaluated".into());
            format!("{} |> {} -> {}", lhs_display, rhs_display, out)
        }
        "|*>" => {
            let out = unary_function_parts_display(rhs_actual)
                .map(|(_, out)| out)
                .unwrap_or_else(|| "B".into());
            let result = map_container_output_display(lhs_actual, &out)
                .unwrap_or_else(|| "Evaluated".into());
            format!("{} |*> {} -> {}", lhs_display, rhs_display, result)
        }
        "|>=" => {
            let out = unary_function_parts_display(rhs_actual)
                .map(|(_, out)| out)
                .unwrap_or_else(|| "Container<B>".into());
            format!("{} |>= {} -> {}", lhs_display, rhs_display, out)
        }
        ">>" => {
            let Some((lhs_in, _lhs_out)) = unary_function_parts_display(lhs_actual) else {
                return fallback();
            };
            let Some((_rhs_in, rhs_out)) = unary_function_parts_display(rhs_actual) else {
                return fallback();
            };
            format!(
                "{} >> {} -> ({} -> {})",
                lhs_display, rhs_display, lhs_in, rhs_out
            )
        }
        ">*" => {
            let Some((lhs_in, lhs_out)) = unary_function_parts_display(lhs_actual) else {
                return fallback();
            };
            let Some((_rhs_in, rhs_out)) = unary_function_parts_display(rhs_actual) else {
                return fallback();
            };
            let result = map_container_output_display(&lhs_out, &rhs_out)
                .unwrap_or_else(|| "Container<C>".into());
            format!(
                "{} >* {} -> ({} -> {})",
                lhs_display, rhs_display, lhs_in, result
            )
        }
        ">=>" => {
            let Some((lhs_in, _lhs_out)) = unary_function_parts_display(lhs_actual) else {
                return fallback();
            };
            let Some((_rhs_in, rhs_out)) = unary_function_parts_display(rhs_actual) else {
                return fallback();
            };
            format!(
                "{} >=> {} -> ({} -> {})",
                lhs_display, rhs_display, lhs_in, rhs_out
            )
        }
        _ => fallback(),
    }
}

fn flow_type_display(ty: &str, is_mismatch: bool) -> String {
    if is_mismatch {
        ty.fg(Color::Red).to_string()
    } else {
        ty.to_string()
    }
}

fn unary_function_parts_display(ty: &str) -> Option<(String, String)> {
    let trimmed = ty.trim();
    let inner = trimmed.strip_prefix('(')?.strip_suffix(')')?;
    let (input, output) = inner.split_once(" -> ")?;
    Some((input.trim().to_string(), output.trim().to_string()))
}

fn map_container_output_display(container_ty: &str, new_inner: &str) -> Option<String> {
    if container_ty.starts_with("Result<") && container_ty.ends_with('>') {
        Some(format!("Result<{}>", new_inner))
    } else if container_ty.starts_with("List<") && container_ty.ends_with('>') {
        Some(format!("List<{}>", new_inner))
    } else {
        None
    }
}

fn flow_operator_mismatch_sides(op: &str, message: &str) -> (bool, bool) {
    if message.contains("on the left") || message.contains("left-hand side") {
        return (true, false);
    }
    if message.contains("right-hand side") {
        return (false, true);
    }
    if message.contains("both sides")
        || message.contains("cannot mix")
        || message.contains("left output type to match the right input type")
        || message.contains("left contextual output to match the right input type")
    {
        return (true, true);
    }
    if message.contains("type mismatch") {
        return match op {
            "|>" => (true, false),
            "|*>" | "|>=" => (false, true),
            _ => (true, true),
        };
    }
    (false, false)
}

fn flow_operator_help(op: &str) -> &'static str {
    match op {
        "|>" => "`|>` passes the whole left-hand value into the callable. The RHS must accept the LHS type.",
        "|*>" => "`|*>` maps a plain function over Result/List. The right side must return a plain value.",
        "|>=" => "`|>=` binds a Result/List value. The right side must return the same container family.",
        ">>" => "`>>` composes plain functions. Use `>*` or `>=>` when the left function returns Result/List.",
        ">*" => "`>*` composes a contextual function with a plain function. The left side must return Result/List.",
        ">=>" => "`>=>` composes contextual functions. Both sides must return the same container family.",
        _ => "Check the function operator rule against the LHS and RHS types.",
    }
}

fn line_column_for_offset(source: &str, offset: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut column = 1u32;
    let limit = offset.min(source.chars().count());
    for ch in source.chars().take(limit) {
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn extract_expected_got(message: &str) -> (Option<String>, Option<String>) {
    let Some(expected_start) = message.find("expected ") else {
        return (None, None);
    };
    let expected_part = &message[expected_start + "expected ".len()..];
    let Some((expected, got)) = expected_part.split_once(", got ") else {
        return (None, None);
    };
    (
        Some(expected.trim().to_string()),
        Some(got.trim().to_string()),
    )
}

fn infer_if_branch_mismatch_template(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
    message: &str,
) -> Option<TemplateSpec> {
    let msg_tail = message.strip_prefix("if branches have different types: ")?;
    let (then_ty, else_ty) = msg_tail.split_once(" and ")?;
    let line_idx = line_index_for_span(lines, focus.start)?;
    let (line_start, line_end) = lines[line_idx];
    let line = slice_chars(source, line_start, line_end);
    let chars: Vec<char> = line.chars().collect();
    let if_start = find_subslice_outside_literals(&chars, &['i', 'f', '('], 0)?;
    let mut paren_depth = 1usize;
    let mut first_comma = None;
    let mut second_comma = None;
    let mut close_paren = None;
    let mut idx = if_start + 3;

    while idx < chars.len() {
        if is_quote_char(chars[idx]) {
            idx = skip_quoted_literal(&chars, idx);
            continue;
        }
        match chars[idx] {
            '(' => paren_depth += 1,
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                if paren_depth == 0 {
                    close_paren = Some(idx);
                    break;
                }
            }
            ',' if paren_depth == 1 => {
                if first_comma.is_none() {
                    first_comma = Some(idx);
                } else if second_comma.is_none() {
                    second_comma = Some(idx);
                }
            }
            _ => {}
        }
        idx += 1;
    }

    let first_comma = first_comma?;
    let second_comma = second_comma?;
    let close_paren = close_paren?;

    let then_span = trimmed_span_from_line(line_start, &chars, first_comma + 1, second_comma);
    let else_span = trimmed_span_from_line(line_start, &chars, second_comma + 1, close_paren);

    Some(TemplateSpec {
        labels: vec![
            DiagnosticLabel {
                span: then_span,
                message: format!("then branch: {}", then_ty),
                color: Color::Blue,
            },
            DiagnosticLabel {
                span: else_span,
                message: format!("else branch: {}", else_ty),
                color: Color::Yellow,
            },
        ],
        help: Some(
            "if/3 requires both branches to return the same type. Use if_then/2 when only side effects are needed."
                .into(),
        ),
    })
}

fn infer_match_arm_mismatch_template(
    source: &str,
    focus: &Span,
    message: &str,
) -> Option<TemplateSpec> {
    let msg_tail = message.strip_prefix("Match arm type mismatch: expected ")?;
    let (expected_ty, got_ty) = msg_tail.split_once(", got ")?;
    let chars: Vec<char> = source.chars().collect();
    let (match_start, open_brace, close_brace) = find_enclosing_match_block(&chars, focus.start)?;
    let arm_spans = collect_match_arm_body_spans(&chars, open_brace + 1, close_brace);
    if arm_spans.len() < 2 {
        return None;
    }

    let focus_idx = arm_spans
        .iter()
        .position(|(s, e)| focus.start >= *s && focus.start <= *e)
        .unwrap_or(0);

    let palette = [
        Color::Blue,
        Color::Yellow,
        Color::Cyan,
        Color::Magenta,
        Color::Green,
    ];

    let mut labels = Vec::new();
    labels.push(DiagnosticLabel {
        span: Span {
            start: match_start,
            end: (match_start + 5).min(chars.len()),
        },
        message: format!("match expression expects {}", expected_ty),
        color: Color::Magenta,
    });

    for (idx, (start, end)) in arm_spans.iter().enumerate() {
        let color = palette[idx % palette.len()];
        let actual_ty = if idx == focus_idx {
            got_ty
        } else {
            expected_ty
        };
        let message = format!(
            "arm #{} returns {} (expected {})",
            idx + 1,
            actual_ty,
            expected_ty
        );
        labels.push(DiagnosticLabel {
            span: Span {
                start: *start,
                end: *end,
            },
            message,
            color,
        });
    }

    Some(TemplateSpec {
        labels,
        help: Some("All match arms must return the same type.".into()),
    })
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
    for (idx, line) in lines.iter().enumerate().skip(focus_line + 1) {
        let text = slice_chars(source, line.0, line.1);
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

fn callable_definition_signature_from_hint(hint: &str) -> Option<&str> {
    hint.strip_prefix("Callable definition signature: ")
        .map(|sig| sig.lines().next().unwrap_or(sig))
}

fn callable_definition_span_from_hint(hint: &str) -> Option<Span> {
    let line = hint
        .lines()
        .find_map(|line| line.strip_prefix("Callable definition span: "))?;
    let (start, end) = line.split_once("..")?;
    Some(Span {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
    })
}

fn callable_definition_from_hint(hint: &str) -> Option<(&str, Span)> {
    Some((
        callable_definition_signature_from_hint(hint)?,
        callable_definition_span_from_hint(hint)?,
    ))
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

fn source_signature_caption(
    source: &str,
    lines: &[(usize, usize)],
    sig_line: (usize, usize),
    name: &str,
) -> Option<String> {
    let sig_text = slice_chars(source, sig_line.0, sig_line.1);
    let def_sig = def_signature_from_line(&sig_text)?;
    if let Some(trait_impl) = enclosing_trait_impl_header(source, lines, sig_line.0) {
        return Some(format!("{} {{ def {} }}", trait_impl, def_sig));
    }
    if let Some(impl_target) = enclosing_impl_target(source, lines, sig_line.0) {
        let method = def_sig.strip_prefix(name).unwrap_or(&def_sig);
        return Some(format!("{}::{}{}", impl_target, name, method));
    }
    if let Some(module_path) = enclosing_defmod_path(source, lines, sig_line.0) {
        return Some(format!("{}::{}", module_path, def_sig));
    }
    Some(def_sig)
}

fn def_signature_from_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let after_def = trimmed.strip_prefix("def ")?;
    let before_body = after_def
        .split_once('{')
        .map(|(sig, _)| sig)
        .unwrap_or(after_def)
        .trim();
    Some(before_body.to_string())
}

fn enclosing_trait_impl_header(
    source: &str,
    lines: &[(usize, usize)],
    sig_start: usize,
) -> Option<String> {
    let sig_idx = line_index_for_span(lines, sig_start)?;
    for idx in (0..sig_idx).rev() {
        let text = slice_chars(source, lines[idx].0, lines[idx].1);
        let trimmed = text.trim();
        if trimmed.starts_with("impl ") && trimmed.contains(" for ") {
            return Some(trimmed.trim_end_matches('{').trim().to_string());
        }
        if trimmed.starts_with("def ") || trimmed.starts_with("defmod ") {
            break;
        }
    }
    None
}

fn enclosing_impl_target(
    source: &str,
    lines: &[(usize, usize)],
    sig_start: usize,
) -> Option<String> {
    let sig_idx = line_index_for_span(lines, sig_start)?;
    for idx in (0..sig_idx).rev() {
        let text = slice_chars(source, lines[idx].0, lines[idx].1);
        let trimmed = text.trim();
        if trimmed.starts_with("impl ") && !trimmed.contains(" for ") {
            let target = trimmed
                .strip_prefix("impl ")?
                .trim_end_matches('{')
                .trim()
                .to_string();
            return (!target.is_empty()).then_some(target);
        }
        if trimmed.starts_with("def ") || trimmed.starts_with("defmod ") {
            break;
        }
    }
    None
}

fn enclosing_defmod_path(
    source: &str,
    lines: &[(usize, usize)],
    sig_start: usize,
) -> Option<String> {
    let sig_idx = line_index_for_span(lines, sig_start)?;
    for idx in (0..sig_idx).rev() {
        let text = slice_chars(source, lines[idx].0, lines[idx].1);
        let trimmed = text.trim();
        if trimmed.starts_with("defmod ") {
            let name = trimmed
                .strip_prefix("defmod ")?
                .trim_end_matches('{')
                .trim()
                .to_string();
            return (!name.is_empty()).then_some(name);
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

fn trimmed_span_from_line(line_start: usize, chars: &[char], start: usize, end: usize) -> Span {
    let mut s = start.min(chars.len());
    let mut e = end.min(chars.len());

    while s < e && chars[s].is_ascii_whitespace() {
        s += 1;
    }
    while e > s && chars[e - 1].is_ascii_whitespace() {
        e -= 1;
    }

    Span {
        start: line_start + s,
        end: line_start + e,
    }
}

fn find_enclosing_match_block(chars: &[char], focus_pos: usize) -> Option<(usize, usize, usize)> {
    let mut i = focus_pos.min(chars.len());
    while i > 0 {
        i -= 1;
        if !is_match_keyword_at(chars, i) {
            continue;
        }
        let mut j = i + 5;
        while j < chars.len() && chars[j].is_ascii_whitespace() {
            j += 1;
        }
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_open = None;
        while j < chars.len() {
            if is_quote_char(chars[j]) {
                j = skip_quoted_literal(chars, j);
                continue;
            }
            match chars[j] {
                '(' => paren_depth += 1,
                ')' => paren_depth = paren_depth.saturating_sub(1),
                '[' => bracket_depth += 1,
                ']' => bracket_depth = bracket_depth.saturating_sub(1),
                '{' if paren_depth == 0 && bracket_depth == 0 => {
                    brace_open = Some(j);
                    break;
                }
                '\n' if paren_depth == 0 && bracket_depth == 0 => {}
                _ => {}
            }
            j += 1;
        }
        let open = brace_open?;
        let mut depth = 1usize;
        let mut k = open + 1;
        while k < chars.len() {
            if is_quote_char(chars[k]) {
                k = skip_quoted_literal(chars, k);
                continue;
            }
            match chars[k] {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        if focus_pos >= open && focus_pos <= k {
                            return Some((i, open, k));
                        }
                        break;
                    }
                }
                _ => {}
            }
            k += 1;
        }
    }
    None
}

fn collect_match_arm_body_spans(
    chars: &[char],
    block_start: usize,
    block_end: usize,
) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut i = block_start;
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;

    while i + 1 < block_end {
        if is_quote_char(chars[i]) {
            i = skip_quoted_literal(chars, i);
            continue;
        }
        match chars[i] {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '=' if chars[i + 1] == '>'
                && brace_depth == 0
                && paren_depth == 0
                && bracket_depth == 0 =>
            {
                let body_start = i + 2;
                let mut j = body_start;
                let mut b_depth = 0usize;
                let mut p_depth = 0usize;
                let mut br_depth = 0usize;
                while j < block_end {
                    if is_quote_char(chars[j]) {
                        j = skip_quoted_literal(chars, j);
                        continue;
                    }
                    match chars[j] {
                        '{' => b_depth += 1,
                        '}' => {
                            if b_depth == 0 && p_depth == 0 && br_depth == 0 {
                                break;
                            }
                            b_depth = b_depth.saturating_sub(1);
                        }
                        '(' => p_depth += 1,
                        ')' => p_depth = p_depth.saturating_sub(1),
                        '[' => br_depth += 1,
                        ']' => br_depth = br_depth.saturating_sub(1),
                        ',' if b_depth == 0 && p_depth == 0 && br_depth == 0 => break,
                        _ => {}
                    }
                    j += 1;
                }
                let (start, end) = trim_char_span(chars, body_start, j);
                if start < end {
                    spans.push((start, end));
                }
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }

    spans
}

fn is_match_keyword_at(chars: &[char], idx: usize) -> bool {
    if idx + 5 > chars.len() {
        return false;
    }
    if chars[idx..idx + 5] != ['m', 'a', 't', 'c', 'h'] {
        return false;
    }
    let prev_ok = idx == 0 || !(chars[idx - 1].is_ascii_alphanumeric() || chars[idx - 1] == '_');
    let next_ok = idx + 5 >= chars.len()
        || !(chars[idx + 5].is_ascii_alphanumeric() || chars[idx + 5] == '_');
    prev_ok && next_ok
}

fn trim_char_span(chars: &[char], start: usize, end: usize) -> (usize, usize) {
    let mut s = start.min(chars.len());
    let mut e = end.min(chars.len());
    while s < e && chars[s].is_ascii_whitespace() {
        s += 1;
    }
    while e > s && chars[e - 1].is_ascii_whitespace() {
        e -= 1;
    }
    (s, e)
}

fn find_assignment_eq_before(chars: &[char], limit: usize) -> Option<usize> {
    let limit = limit.min(chars.len());
    let mut idx = 0usize;
    while idx < limit {
        if is_quote_char(chars[idx]) {
            idx = skip_quoted_literal(chars, idx);
            continue;
        }
        if chars[idx] == '=' {
            let prev = idx.checked_sub(1).and_then(|i| chars.get(i)).copied();
            let next = chars.get(idx + 1).copied();
            let looks_like_operator = matches!(prev, Some('<' | '>' | '!' | '=' | '|' | '-'))
                || matches!(next, Some('=' | '>'));
            if !looks_like_operator {
                return Some(idx);
            }
        }
        idx += 1;
    }
    None
}

fn line_span_containing(source: &str, pos: usize) -> Option<(usize, usize)> {
    let lines = line_spans(source);
    let idx = line_index_for_span(&lines, pos)?;
    Some(lines[idx])
}

#[derive(Debug, Clone)]
struct AnnotatedAssignment {
    line_idx: usize,
    eq_col: usize,
    lhs_span: Span,
}

fn find_annotated_assignment_line(
    source: &str,
    lines: &[(usize, usize)],
    focus_line: usize,
) -> Option<AnnotatedAssignment> {
    for idx in (0..=focus_line).rev() {
        let (line_start, line_end) = lines[idx];
        let line = slice_chars(source, line_start, line_end);
        if line.trim().is_empty() {
            break;
        }
        let chars: Vec<char> = line.chars().collect();
        let Some(colon) = find_char_outside_literals(&chars, ':', 0) else {
            continue;
        };
        let Some(eq) = find_assignment_eq_before(&chars, chars.len()) else {
            continue;
        };
        if colon >= eq
            || line.trim_start().starts_with("def ")
            || find_subslice_outside_literals(&chars, &['=', '>'], 0).is_some()
        {
            continue;
        }
        let (lhs_start, lhs_end) = trim_char_span(&chars, 0, eq + 1);
        if lhs_start >= lhs_end {
            continue;
        }
        return Some(AnnotatedAssignment {
            line_idx: idx,
            eq_col: eq,
            lhs_span: Span {
                start: line_start + lhs_start,
                end: line_start + lhs_end,
            },
        });
    }
    None
}

fn assignment_rhs_span(
    source: &str,
    lines: &[(usize, usize)],
    focus_line: usize,
    assignment: &AnnotatedAssignment,
) -> Option<Span> {
    let (line_start, line_end) = lines[assignment.line_idx];
    let line = slice_chars(source, line_start, line_end);
    let chars: Vec<char> = line.chars().collect();
    let same_line_rhs =
        trimmed_span_from_line(line_start, &chars, assignment.eq_col + 1, chars.len());
    if same_line_rhs.start < same_line_rhs.end {
        return Some(same_line_rhs);
    }

    if focus_line > assignment.line_idx {
        if let Some(span) = trimmed_rhs_focus_line_span(source, lines[focus_line]) {
            return Some(span);
        }
    }

    let mut last = None;
    for &(start, end) in lines.iter().skip(assignment.line_idx + 1) {
        let Some(span) = trimmed_line_span(source, (start, end)) else {
            break;
        };
        last = Some(span);
    }
    last
}

fn trimmed_rhs_focus_line_span(source: &str, line: (usize, usize)) -> Option<Span> {
    let text = slice_chars(source, line.0, line.1);
    let chars: Vec<char> = text.chars().collect();
    let trimmed = trim_char_span(&chars, 0, chars.len());
    if trimmed.0 >= trimmed.1 {
        return None;
    }

    for op in ["|>=", "|*>", "|>", ">>", ">*", ">=>"] {
        let op_len = op.chars().count();
        if trimmed.0 + op_len <= trimmed.1
            && chars[trimmed.0..trimmed.0 + op_len]
                .iter()
                .copied()
                .eq(op.chars())
        {
            let rhs = trim_char_span(&chars, trimmed.0 + op_len, trimmed.1);
            return (rhs.0 < rhs.1).then_some(Span {
                start: line.0 + rhs.0,
                end: line.0 + rhs.1,
            });
        }
    }

    Some(Span {
        start: line.0 + trimmed.0,
        end: line.0 + trimmed.1,
    })
}

fn trimmed_line_span(source: &str, line: (usize, usize)) -> Option<Span> {
    let text = slice_chars(source, line.0, line.1);
    let chars: Vec<char> = text.chars().collect();
    let span = trimmed_span_from_line(line.0, &chars, 0, chars.len());
    (span.start < span.end).then_some(span)
}

fn identifier_span_at(source: &str, pos: usize) -> Option<Span> {
    let chars: Vec<char> = source.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let mut start = pos.min(chars.len().saturating_sub(1));
    while start > 0 && is_ident_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = pos.min(chars.len());
    while end < chars.len() && is_ident_char(chars[end]) {
        end += 1;
    }
    if start < end {
        Some(Span { start, end })
    } else {
        None
    }
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn find_backtick_operator_span(
    line_start: usize,
    chars: &[char],
    focus_col: usize,
) -> Option<Span> {
    let mut best = None;
    let mut idx = 0usize;
    while idx < chars.len() {
        if is_quote_char(chars[idx]) {
            idx = skip_quoted_literal(chars, idx);
            continue;
        }
        if chars[idx] != '`' {
            idx += 1;
            continue;
        }
        let first = idx;
        idx += 1;
        while idx < chars.len() {
            if is_quote_char(chars[idx]) {
                idx = skip_quoted_literal(chars, idx);
                continue;
            }
            if chars[idx] == '`' {
                let end = idx + 1;
                if first <= focus_col {
                    best = Some((first, end));
                } else if best.is_none() {
                    best = Some((first, end));
                }
                idx = end;
                break;
            }
            idx += 1;
        }
    }
    let (first, end) = best?;
    Some(Span {
        start: line_start + first,
        end: line_start + end,
    })
}

fn find_operator_symbol_span(
    line_start: usize,
    chars: &[char],
    op_name: &str,
    focus_col: usize,
) -> Option<Span> {
    let op = match op_name {
        "Add" => "+",
        "Sub" => "-",
        "Mul" => "*",
        "Div" => "/",
        "Mod" => "%",
        "Eq" => "==",
        "Neq" => "!=",
        "Lt" => "<",
        "Lte" => "<=",
        "Gt" => ">",
        "Gte" => ">=",
        _ => return None,
    };
    let pattern: Vec<char> = op.chars().collect();
    let mut best = None;
    let mut search_from = 0usize;
    while let Some(start) = find_subslice_outside_literals(chars, &pattern, search_from) {
        let end = start + pattern.len();
        if start <= focus_col {
            best = Some(start);
        } else if best.is_none() {
            best = Some(start);
            break;
        }
        search_from = end.max(search_from + 1);
    }
    let start = best?;
    Some(Span {
        start: line_start + start,
        end: line_start + start + pattern.len(),
    })
}

fn collect_call_argument_spans(chars: &[char], args_start: usize) -> Option<Vec<(usize, usize)>> {
    let mut spans = Vec::new();
    let mut start = args_start;
    let mut paren_depth = 1usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut idx = args_start;

    while idx < chars.len() {
        if is_quote_char(chars[idx]) {
            idx = skip_quoted_literal(chars, idx);
            continue;
        }
        match chars[idx] {
            '(' => paren_depth += 1,
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                if paren_depth == 0 {
                    let span = trim_char_span(chars, start, idx);
                    if span.0 < span.1 {
                        spans.push(span);
                    }
                    return Some(spans);
                }
            }
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if paren_depth == 1 && bracket_depth == 0 && brace_depth == 0 => {
                let span = trim_char_span(chars, start, idx);
                if span.0 < span.1 {
                    spans.push(span);
                }
                start = idx + 1;
            }
            _ => {}
        }
        idx += 1;
    }

    None
}

fn extract_match_pattern_span(chars: &[char]) -> Option<(usize, usize)> {
    let arrow = find_subslice_outside_literals(chars, &['=', '>'], 0)?;
    trim_after_line_indent(chars, 0, arrow)
}

fn trim_after_line_indent(chars: &[char], start: usize, end: usize) -> Option<(usize, usize)> {
    let span = trim_char_span(chars, start, end);
    (span.0 < span.1).then_some(span)
}

fn is_quote_char(ch: char) -> bool {
    matches!(ch, '"' | '\'')
}

fn skip_quoted_literal(chars: &[char], start: usize) -> usize {
    let Some(&quote) = chars.get(start) else {
        return start;
    };
    let mut idx = start + 1;
    let mut escaped = false;
    while idx < chars.len() {
        let ch = chars[idx];
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return idx + 1;
        }
        idx += 1;
    }
    chars.len()
}

fn find_char_outside_literals(chars: &[char], target: char, start: usize) -> Option<usize> {
    let mut idx = start.min(chars.len());
    while idx < chars.len() {
        if is_quote_char(chars[idx]) {
            idx = skip_quoted_literal(chars, idx);
            continue;
        }
        if chars[idx] == target {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

fn find_subslice_outside_literals(chars: &[char], needle: &[char], start: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(start.min(chars.len()));
    }

    let mut idx = start.min(chars.len());
    while idx + needle.len() <= chars.len() {
        if is_quote_char(chars[idx]) {
            idx = skip_quoted_literal(chars, idx);
            continue;
        }
        if chars[idx..idx + needle.len()] == *needle {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("writer failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("writer failed"))
        }
    }

    fn strip_ansi(input: &str) -> String {
        let mut out = String::new();
        let mut chars = input.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' && chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
                continue;
            }
            out.push(ch);
        }
        out
    }

    #[test]
    fn fallback_diagnostic_preserves_core_fields() {
        let spec = DiagnosticSpec {
            kind: "TypeError".into(),
            message: "expected Int, got String".into(),
            primary_span: Span { start: 13, end: 23 },
            labels: vec![DiagnosticLabel {
                span: Span { start: 13, end: 23 },
                message: "binding value".into(),
                color: Color::Blue,
            }],
            help: Some("The type annotation requires Int".into()),
        };

        let mut buf = Vec::new();
        write_fallback_diagnostic(
            &mut buf,
            "main.srt",
            &spec,
            &io::Error::other("broken pipe"),
        )
        .expect("fallback output should succeed");

        let text = String::from_utf8(buf).expect("fallback output must be valid utf-8");
        assert!(text.contains("diagnostic rendering failed: broken pipe"));
        assert!(text.contains("TypeError: expected Int, got String"));
        assert!(text.contains("--> main.srt:13-23"));
        assert!(text.contains("= note: binding value [13-23]"));
        assert!(text.contains("= help: The type annotation requires Int"));
    }

    #[test]
    fn fallback_diagnostic_returns_writer_error() {
        let spec = simple_error(
            "ParseError",
            "unexpected token",
            Span { start: 2, end: 5 },
            None,
        );
        let err = write_fallback_diagnostic(
            &mut FailingWriter,
            "main.srt",
            &spec,
            &io::Error::other("broken pipe"),
        )
        .expect_err("failing writer should propagate io error");

        assert_eq!(err.kind(), io::ErrorKind::Other);
    }

    #[test]
    fn source_registry_registers_and_updates_entries() {
        let mut sources = SourceRegistry::new();
        let src_id = sources.register("main.srt", "x = 1");

        assert_eq!(sources.file_name(src_id), Some("main.srt"));
        assert_eq!(sources.source(src_id), Some("x = 1"));

        assert!(sources.update_source(src_id, "x = 2"));
        assert_eq!(sources.source(src_id), Some("x = 2"));
    }

    #[test]
    fn source_registry_returns_owned_context() {
        let mut sources = SourceRegistry::new();
        let src_id = sources.register("script.srt", "print(\"ok\")");

        let context = sources
            .owned_context(src_id)
            .expect("registered source should exist");
        assert_eq!(context.0, "print(\"ok\")");
        assert_eq!(context.1, "script.srt");
    }

    #[test]
    fn render_error_normalizes_out_of_range_span_to_keep_source_snippet() {
        let spec = simple_error(
            "TypeError",
            "expected Int, got String",
            Span {
                start: 9999,
                end: 10000,
            },
            None,
        );
        let rendered = render_error("main.srt", "bad: Int = \"x\"", &spec);
        assert!(
            rendered.contains("TypeError: expected Int, got String"),
            "expected headline in rendered diagnostic"
        );
        assert!(
            rendered.contains("main.srt"),
            "expected file label in rendered diagnostic"
        );
    }

    #[test]
    fn render_error_normalizes_empty_span_to_single_point_label() {
        let spec = simple_error(
            "ParseError",
            "unexpected token",
            Span { start: 4, end: 4 },
            None,
        );
        let rendered = render_error("main.srt", "abcde", &spec);
        assert!(rendered.contains("ParseError: unexpected token"));
        assert!(rendered.contains("main.srt"));
    }

    #[test]
    fn serializable_report_uses_character_offsets_for_utf8_source() {
        let mut sources = SourceRegistry::new();
        let source_id = sources.register("main.srt", "あx");
        let spec = simple_error(
            "TypeError",
            "expected Int, got String",
            Span { start: 1, end: 2 },
            None,
        );

        let report = serializable_report_by_id(&sources, source_id, "typecheck", &spec);
        assert_eq!(report.errors[0].line, 1);
        assert_eq!(report.errors[0].column, 2);
        assert_eq!(report.errors[0].span, [1, 2]);
    }

    #[test]
    fn char_span_to_byte_range_converts_only_at_render_boundary() {
        let range = char_span_to_byte_range("あx", &Span { start: 1, end: 2 });
        assert_eq!(range, "あ".len().."あx".len());
    }

    #[test]
    fn type_error_spec_labels_backtick_operator_operands() {
        let source = "bad = 1 `+` \"oops\"";
        let err = TypeError {
            message: "Cannot apply Add to Int and String".into(),
            span: Span { start: 6, end: 18 },
            hint: Some(
                "Operator `Add` requires compatible operand types. Left operand is Int, right operand is String."
                    .into(),
            ),
        };

        let spec = type_error_spec(source, &err);

        assert!(spec
            .labels
            .iter()
            .any(|label| label.message == "left operand: Int"));
        assert!(spec
            .labels
            .iter()
            .any(|label| label.message == "operator `Add`"));
        assert!(spec
            .labels
            .iter()
            .any(|label| label.message == "right operand: String"));
        assert!(spec
            .help
            .as_deref()
            .is_some_and(|help| help.contains("Operator `Add` requires")));
    }

    #[test]
    fn type_error_spec_picks_symbol_operator_outside_literals() {
        let source = r#""+" + "value""#;
        let rhs_start = source.rfind("\"value\"").expect("rhs literal");
        let err = TypeError {
            message: "Cannot apply Add to String and String".into(),
            span: Span {
                start: rhs_start,
                end: source.chars().count(),
            },
            hint: Some(
                "Operator `Add` requires compatible operand types. Left operand is String, right operand is String."
                    .into(),
            ),
        };

        let spec = type_error_spec(source, &err);
        let op = spec
            .labels
            .iter()
            .find(|label| label.message == "operator `Add`")
            .expect("operator label");
        let lhs = spec
            .labels
            .iter()
            .find(|label| label.message == "left operand: String")
            .expect("lhs label");

        assert_eq!(slice_chars(source, op.span.start, op.span.end), "+");
        assert_eq!(slice_chars(source, lhs.span.start, lhs.span.end), r#""+""#);
    }

    #[test]
    fn type_error_spec_labels_flow_operator_parts() {
        let source = "bad = parse(1) |> &inc";
        let err = TypeError {
            message: "`|>` type mismatch: expected Int, got Result<Int>".into(),
            span: Span { start: 6, end: 14 },
            hint: Some(
                "`|>` signature rule: LHS: A; RHS: (A -> B); result: B. LHS: Result<Int>. RHS: (Int -> Int)."
                    .into(),
            ),
        };

        let spec = type_error_spec(source, &err);
        let label_text = spec
            .labels
            .iter()
            .map(|label| strip_ansi(&label.message))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(label_text.contains("TypeMismatch: LHS actual: Result<Int> (expected Int)"));
        assert!(label_text.contains("OP: Result<Int> |> (Int -> Int) -> Int"));
        assert!(label_text.contains("RHS actual: (Int -> Int) (expected Callable)"));
        assert!(spec
            .help
            .as_deref()
            .is_some_and(|help| help.contains("The RHS must accept the LHS type")));
    }

    #[test]
    fn type_error_spec_splits_annotation_mismatch_on_same_line() {
        let source = "result: Int = make_text(1)";
        let err = TypeError {
            message: "expected Int, got String".into(),
            span: Span {
                start: 0,
                end: source.chars().count(),
            },
            hint: None,
        };

        let spec = type_error_spec(source, &err);

        assert!(spec
            .labels
            .iter()
            .any(|label| label.message == "LHS annotation: Int"));
        assert!(spec
            .labels
            .iter()
            .any(|label| label.message == "RHS expression: String"));

        let rendered = strip_ansi(&render_error("main.srt", source, &spec));
        assert!(rendered.contains("LHS annotation: Int"));
        assert!(rendered.contains("RHS expression: String"));
        assert!(!rendered.contains("╰── expected Int, got String"));
    }

    #[test]
    fn type_error_spec_splits_multiline_annotation_mismatch() {
        let source = "ret: String =\n    fun1()\n    |> fun2()\n    |> fun3()";
        let fun3_start = source.find("fun3").expect("source has final rhs call");
        let err = TypeError {
            message: "expected String, got Int".into(),
            span: Span {
                start: fun3_start,
                end: fun3_start + "fun3()".len(),
            },
            hint: None,
        };

        let spec = type_error_spec(source, &err);
        let lhs = spec
            .labels
            .iter()
            .find(|label| label.message == "LHS annotation: String")
            .expect("lhs label");
        let rhs = spec
            .labels
            .iter()
            .find(|label| label.message == "RHS expression: Int")
            .expect("rhs label");

        assert_eq!(
            slice_chars(source, lhs.span.start, lhs.span.end),
            "ret: String ="
        );
        assert_eq!(slice_chars(source, rhs.span.start, rhs.span.end), "fun3()");
    }

    #[test]
    fn render_flow_operator_error_keeps_actual_types_out_of_help() {
        let source = "bad = 1 |>= &inc";
        let err = TypeError {
            message: "`|>=` requires Result or List on the left, got Int".into(),
            span: Span { start: 6, end: 7 },
            hint: Some(
                "`|>=` signature rule: LHS: Result<A, E> or List<A>; RHS: (A -> Result<B, E>) or (A -> List<B>); result: Result<B, E> or List<B>. LHS: Int. RHS: (Int -> Result<Int>). Operators share precedence and resolve left-to-right, so LHS is the type produced so far."
                    .into(),
            ),
        };

        let spec = type_error_spec(source, &err);
        let rendered = render_error("main.srt", source, &spec);
        let rendered_plain = strip_ansi(&rendered);

        assert!(rendered_plain.contains("Container required: LHS actual: Int"));
        assert!(rendered_plain.contains("OP: Int |>= (Int -> Result<Int>) -> Result<Int>"));
        assert!(
            rendered_plain.contains("Reason: `|>=` requires Result or List on the left, got Int")
        );
        assert_eq!(
            spec.help.as_deref(),
            Some(
                "`|>=` binds a Result/List value. The right side must return the same container family."
            )
        );
        assert!(!spec.help.as_deref().unwrap_or("").contains("Int"));
    }

    #[test]
    fn type_error_spec_labels_repl_flow_operator_lhs_before_pipe_bind() {
        let source = r#"re"^a$" |>= Regex::is_match("a")"#;
        let rhs_start = source.find("Regex::is_match").expect("rhs call");
        let err = TypeError {
            message: "`|>=` requires the right-hand side to return Result, got Boolean".into(),
            span: Span {
                start: rhs_start,
                end: source.chars().count(),
            },
            hint: Some(
                "`|>=` signature rule: LHS: Result<A, E>; RHS: (A -> Result<B, E>); result: Result<B, E>. LHS: Result<Regex>. RHS: (Regex -> Boolean). Operators share precedence and resolve left-to-right, so LHS is the type produced so far."
                    .into(),
            ),
        };

        let spec = type_error_spec(source, &err);
        let lhs = spec
            .labels
            .iter()
            .find(|label| strip_ansi(&label.message).contains("LHS actual: Result<Regex>"))
            .expect("lhs label");
        let rhs = spec
            .labels
            .iter()
            .find(|label| strip_ansi(&label.message).contains("RHS actual: (Regex -> Boolean)"))
            .expect("rhs label");

        assert_eq!(
            slice_chars(source, lhs.span.start, lhs.span.end),
            r#"re"^a$""#
        );
        assert_eq!(
            slice_chars(source, rhs.span.start, rhs.span.end),
            r#"Regex::is_match("a")"#
        );
    }

    #[test]
    fn type_error_spec_labels_ensure_predicate_call() {
        let source = "guard = ensure(4, is_even(), NoneError)";
        let err = TypeError {
            message: "ensure requires a closure or capture predicate".into(),
            span: Span { start: 18, end: 27 },
            hint: Some(
                "Use `&predicate` or `{|value| predicate(value) }`; call expressions such as `predicate()` are not accepted here."
                    .into(),
            ),
        };

        let spec = type_error_spec(source, &err);

        assert!(spec.labels.iter().any(|label| label
            .message
            .contains("predicate must be a closure or capture")));
        assert!(spec
            .help
            .as_deref()
            .is_some_and(|help| help.contains("Use `&predicate`")));
    }

    #[test]
    fn type_error_spec_labels_ensure_predicate_call_with_string_comma() {
        let source = r#"guard = ensure(4, invalid("a,b"), NoneError)"#;
        let err = TypeError {
            message: "ensure requires a closure or capture predicate".into(),
            span: Span { start: 18, end: 32 },
            hint: None,
        };

        let spec = type_error_spec(source, &err);
        let predicate = spec
            .labels
            .iter()
            .find(|label| {
                label
                    .message
                    .contains("predicate must be a closure or capture")
            })
            .expect("predicate label");

        assert_eq!(
            slice_chars(source, predicate.span.start, predicate.span.end),
            r#"invalid("a,b")"#
        );
    }

    #[test]
    fn collect_match_arm_body_spans_ignores_literals() {
        let source = r#"match value { Left("=>") => "a,b", Right(x) => x }"#;
        let chars: Vec<char> = source.chars().collect();
        let (_, open_brace, close_brace) =
            find_enclosing_match_block(&chars, source.find("Right").expect("focus"))
                .expect("match block");
        let spans = collect_match_arm_body_spans(&chars, open_brace + 1, close_brace);

        assert_eq!(spans.len(), 2);
        assert_eq!(slice_chars(source, spans[0].0, spans[0].1), r#""a,b""#);
        assert_eq!(slice_chars(source, spans[1].0, spans[1].1), "x");
    }

    #[test]
    fn extract_match_pattern_span_ignores_literal_arrow() {
        let source = r#"Capture("=>") => value"#;
        let chars: Vec<char> = source.chars().collect();
        let span = extract_match_pattern_span(&chars).expect("pattern span");

        assert_eq!(slice_chars(source, span.0, span.1), r#"Capture("=>")"#);
    }

    #[test]
    fn type_error_spec_uses_callable_definition_signature_hint() {
        let source = r#"def add(x: Int, y: Int) -> Int {
  x + y
}
bad = &add("oops")"#;
        let err = TypeError {
            message: "Argument type mismatch: expected Int, got String".into(),
            span: Span { start: 56, end: 62 },
            hint: Some(
                "Callable definition signature: __Script::fixture::add(x: Int, y: Int) -> Int\nCallable definition span: 0..32"
                    .into(),
            ),
        };

        let spec = type_error_spec(source, &err);
        assert!(spec.help.is_none());
        assert!(spec
            .labels
            .iter()
            .any(|label| label.message == "__Script::fixture::add(x: Int, y: Int) -> Int"));
        let def_label = spec
            .labels
            .iter()
            .find(|label| label.message == "__Script::fixture::add(x: Int, y: Int) -> Int")
            .expect("definition label");
        assert_eq!(def_label.span.start, 0);

        let rendered = strip_ansi(&render_error("main.srt", source, &spec));
        assert_eq!(
            rendered.matches("def add(x: Int, y: Int) -> Int {").count(),
            1
        );
        assert!(rendered.contains("__Script::fixture::add(x: Int, y: Int) -> Int"));
    }

    #[test]
    fn source_signature_caption_handles_defmod_and_impls() {
        let module_source = r#"defmod Math {
  def add(x: Int, y: Int) -> Int {
    x + y
  }
}"#;
        let module_lines = line_spans(module_source);
        let module_sig = find_function_signature_line(module_source, &module_lines, "add")
            .expect("module def line");
        assert_eq!(
            source_signature_caption(module_source, &module_lines, module_sig, "add").as_deref(),
            Some("Math::add(x: Int, y: Int) -> Int")
        );

        let impl_source = r#"impl User {
  def normalize(self: Self) -> Self {
    self
  }
}"#;
        let impl_lines = line_spans(impl_source);
        let impl_sig = find_function_signature_line(impl_source, &impl_lines, "normalize")
            .expect("impl def line");
        assert_eq!(
            source_signature_caption(impl_source, &impl_lines, impl_sig, "normalize").as_deref(),
            Some("User::normalize(self: Self) -> Self")
        );

        let trait_impl_source = r#"impl From<String> for Int {
  def from(self: Self, to: TypeRef<String>) -> String {
    inspect(self)
  }
}"#;
        let trait_impl_lines = line_spans(trait_impl_source);
        let trait_impl_sig =
            find_function_signature_line(trait_impl_source, &trait_impl_lines, "from")
                .expect("trait impl def line");
        assert_eq!(
            source_signature_caption(trait_impl_source, &trait_impl_lines, trait_impl_sig, "from")
                .as_deref(),
            Some(
                "impl From<String> for Int { def from(self: Self, to: TypeRef<String>) -> String }"
            )
        );
    }

    #[test]
    fn parse_error_spec_adds_unexpected_token_help() {
        let spec = parse_error_spec(
            "x = )",
            "Unexpected token: RParen",
            Span { start: 4, end: 5 },
        );

        assert!(spec
            .labels
            .iter()
            .any(|label| label.message.contains("unexpected closing parenthesis")));
        assert!(spec
            .help
            .as_deref()
            .is_some_and(|help| help.contains("parser stopped")));
    }

    #[test]
    fn resolve_error_spec_labels_undefined_name() {
        let spec = resolve_error_spec(
            "unknown(1)",
            "Undefined variable: unknown",
            Span { start: 0, end: 7 },
        );

        assert!(spec
            .labels
            .iter()
            .any(|label| label.message.contains("unresolved name `unknown`")));
        assert!(spec
            .help
            .as_deref()
            .is_some_and(|help| help.contains("not defined in the current scope")));
    }
}
