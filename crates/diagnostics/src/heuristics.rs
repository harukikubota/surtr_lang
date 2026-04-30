use crate::{
    Color, DiagnosticLabel, DiagnosticSpec, RuntimeDiagnosticContext, SourceId, SourceRegistry,
};
use ariadne::Fmt;
use spire::ast::Span;

pub(crate) fn should_render_related_label_with_own_source(
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

pub(crate) fn has_duplicate_definition_labels(spec: &DiagnosticSpec) -> bool {
    spec.labels
        .iter()
        .any(|label| label.message == "first definition")
        && spec
            .labels
            .iter()
            .any(|label| label.message == "conflicting definition")
}

pub(crate) fn has_missing_trait_method_labels(spec: &DiagnosticSpec) -> bool {
    spec.labels
        .iter()
        .any(|label| label.message == "impl target")
        && spec
            .labels
            .iter()
            .any(|label| label.message == "missing required method")
}

pub(crate) fn has_trait_impl_signature_mismatch_labels(spec: &DiagnosticSpec) -> bool {
    spec.labels
        .iter()
        .any(|label| label.message == "trait declaration")
        && spec
            .labels
            .iter()
            .any(|label| label.message.starts_with("expected "))
        && spec
            .labels
            .iter()
            .any(|label| label.message == "trait impl declaration")
        && spec
            .labels
            .iter()
            .any(|label| label.message.starts_with("actual "))
}

pub(crate) fn has_parse_focus_labels(spec: &DiagnosticSpec) -> bool {
    spec.kind == "ParseError"
        && spec.labels.iter().any(|label| {
            matches!(
                label.message.as_str(),
                "forbidden top-level declaration"
                    | "top-level expression is not allowed here"
                    | "return-position `impl Trait` is not supported"
                    | "`where` clauses are not available yet"
            )
        })
}

pub(crate) fn has_total_bind_pattern_labels(spec: &DiagnosticSpec) -> bool {
    spec.kind == "TypeError"
        && spec
            .labels
            .iter()
            .any(|label| label.message == "LHS pattern: partial MatchBlock pattern")
        && spec.labels.iter().any(|label| {
            label
                .message
                .contains("Use `=?` for partial destructuring and extractor-driven matches.")
        })
}

pub(crate) fn has_runtime_safebind_labels(spec: &DiagnosticSpec) -> bool {
    spec.labels
        .iter()
        .any(|label| label.message == "SafeBind partial match")
}

pub(crate) fn has_runtime_error_focus_labels(spec: &DiagnosticSpec) -> bool {
    spec.kind == "RuntimeError"
        && spec.labels.iter().any(|label| {
            label.message == "call target"
                || label.message.starts_with("opcode:")
                || label.message.starts_with("expected rule:")
                || label.message.starts_with("runtime rule:")
        })
}

pub(crate) fn infer_missing_trait_method_labels(
    source: &str,
    message: &str,
) -> Option<Vec<DiagnosticLabel>> {
    let prefix = "Trait impl ";
    let suffix = " is missing method `";
    let rest = message.strip_prefix(prefix)?;
    let (impl_head, method_part) = rest.split_once(suffix)?;
    let method_name = method_part.strip_suffix('`')?;
    let trait_name = impl_head.split_once(" for ")?.0;
    let impl_pattern = format!("impl {} for ", trait_name);
    let method_pattern = format!("def {}(", method_name);
    let impl_span = line_head_span_with_brace(source, &impl_pattern)?;
    let method_span = line_head_span_with_brace(source, &method_pattern)?;
    Some(vec![
        DiagnosticLabel {
            source_id: None,
            span: impl_span,
            message: "impl target".to_string(),
            color: None,
        },
        DiagnosticLabel {
            source_id: None,
            span: method_span,
            message: "missing required method".to_string(),
            color: Some(Color::Red),
        },
    ])
}

pub(crate) fn infer_trait_impl_signature_mismatch_labels(
    source: &str,
    message: &str,
) -> Option<Vec<DiagnosticLabel>> {
    let prefix = "Trait impl method ";
    let rest = message.strip_prefix(prefix)?;
    let (method_head, detail) = rest.split_once(" has incompatible ")?;
    let (trait_name, method_name) = method_head.split_once("::")?;
    let expected_ty = detail
        .split_once("expected ")
        .and_then(|(_, rest)| {
            rest.split_once(", got ")
                .map(|(expected, _)| expected.trim())
        })
        .filter(|expected| !expected.is_empty())?;
    let got_ty = detail
        .split_once("got ")
        .map(|(_, got)| got.trim())
        .filter(|got| !got.is_empty())?;
    let mismatch_kind = if detail.starts_with("parameter type") {
        "parameter"
    } else if detail.starts_with("return type") {
        "return"
    } else {
        "signature"
    };

    let trait_decl_span = line_head_span_with_brace(source, &format!("deftrait {}", trait_name))?;
    let trait_fn_span = line_head_span_from(
        source,
        &format!("def {}(", method_name),
        trait_decl_span.start,
    )?;
    let impl_decl_span = line_head_span_with_brace(source, &format!("impl {} for ", trait_name))?;
    let impl_fn_line_span = line_head_span_from(
        source,
        &format!("def {}(", method_name),
        impl_decl_span.start,
    )?;
    let impl_fn_error_span = type_token_span_in_line(source, &impl_fn_line_span, got_ty)?;

    Some(vec![
        DiagnosticLabel {
            source_id: None,
            span: trait_decl_span,
            message: "trait declaration".to_string(),
            color: None,
        },
        DiagnosticLabel {
            source_id: None,
            span: trait_fn_span,
            message: format!("expected {} type: {}", mismatch_kind, expected_ty),
            color: None,
        },
        DiagnosticLabel {
            source_id: None,
            span: impl_decl_span,
            message: "trait impl declaration".to_string(),
            color: None,
        },
        DiagnosticLabel {
            source_id: None,
            span: impl_fn_error_span,
            message: format!("actual {} type: {}", mismatch_kind, got_ty),
            color: Some(Color::Red),
        },
    ])
}

pub(crate) fn line_head_span_with_brace(source: &str, needle: &str) -> Option<Span> {
    let byte_start = source.find(needle)?;
    let line_byte_start = source[..byte_start]
        .rfind('\n')
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let line_byte_end = source[byte_start..]
        .find('\n')
        .map(|idx| byte_start + idx)
        .unwrap_or(source.len());
    let line = &source[line_byte_start..line_byte_end];
    let head_byte_start = line
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    let brace_byte_idx = line.find('{').unwrap_or(line.len());
    let trim_end = line[..brace_byte_idx]
        .trim_end_matches(char::is_whitespace)
        .len();
    let start = source[..line_byte_start + head_byte_start].chars().count();
    let end = source[..line_byte_start + trim_end].chars().count();
    Some(Span { start, end })
}

pub(crate) fn line_head_span_from(
    source: &str,
    needle: &str,
    start_char_offset: usize,
) -> Option<Span> {
    let start_byte_offset = char_offset_to_byte_offset(source, start_char_offset);
    let relative_byte_start = source[start_byte_offset..].find(needle)?;
    let byte_start = start_byte_offset + relative_byte_start;
    let line_byte_start = source[..byte_start]
        .rfind('\n')
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let line_byte_end = source[byte_start..]
        .find('\n')
        .map(|idx| byte_start + idx)
        .unwrap_or(source.len());
    let line = &source[line_byte_start..line_byte_end];
    let head_byte_start = line
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    let trim_end = line.trim_end_matches(char::is_whitespace).len();
    let start = source[..line_byte_start + head_byte_start].chars().count();
    let end = source[..line_byte_start + trim_end].chars().count();
    Some(Span { start, end })
}

pub(crate) fn type_token_span_in_line(
    source: &str,
    line_span: &Span,
    ty_name: &str,
) -> Option<Span> {
    let line_start = char_offset_to_byte_offset(source, line_span.start);
    let line_end = char_offset_to_byte_offset(source, line_span.end);
    let line = &source[line_start..line_end];
    let byte_idx = line.rfind(ty_name)?;
    let start = source[..line_start + byte_idx].chars().count();
    let end = start + ty_name.chars().count();
    Some(Span { start, end })
}

pub(crate) fn normalized_char_span(source: &str, span: &Span) -> Span {
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

pub(crate) fn char_span_to_byte_range(source: &str, span: &Span) -> std::ops::Range<usize> {
    let normalized = normalized_char_span(source, span);
    char_offset_to_byte_offset(source, normalized.start)
        ..char_offset_to_byte_offset(source, normalized.end)
}

pub(crate) fn char_offset_to_byte_offset(source: &str, offset: usize) -> usize {
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
pub(crate) struct TemplateSpec {
    pub(crate) labels: Vec<DiagnosticLabel>,
    pub(crate) notes: Vec<String>,
    pub(crate) help: Option<String>,
}

pub(crate) struct FlowOperatorView<'a> {
    lhs_actual: &'a str,
    rhs_actual: &'a str,
    op_rule: String,
    step: String,
    rule_detail: Option<String>,
    reason: String,
    help: String,
}

pub(crate) struct BinaryOperatorView<'a> {
    lhs_actual: &'a str,
    rhs_actual: &'a str,
    op_rule: String,
    step: String,
    reason: String,
    help: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOperatorFailureKind {
    IncompatibleTypes,
    MissingImplementation,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedBinaryOperatorError {
    op_name_hint: Option<&'static str>,
    left_ty: Option<String>,
    right_ty: Option<String>,
    failure_kind: BinaryOperatorFailureKind,
}

pub(crate) fn infer_type_error_template(
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
    if let Some(spec) = infer_plain_rhs_required_flow_template(source, &lines, message) {
        return Some(spec);
    }
    if let Some(spec) = infer_ensure_predicate_template(source, &lines, focus, message) {
        return Some(spec);
    }
    if let Some(spec) = infer_extractor_template(source, &lines, focus, message) {
        return Some(spec);
    }
    if let Some(spec) = infer_total_bind_pattern_template(source, &lines, focus, message, hint) {
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
            source_id: None,
            span: Span {
                start: decl_line.0,
                end: decl_line.1,
            },
            message: decl_text,
            color: Some(Color::Blue),
        }];

        labels.push(DiagnosticLabel {
            source_id: None,
            span: Span {
                start: close_line.0,
                end: close_line.1,
            },
            message: "function body ends here".into(),
            color: Some(Color::Yellow),
        });

        return Some(TemplateSpec {
            labels,
            notes: Vec::new(),
            help: None,
        });
    }

    if let Some(call_name) = call_name_at_span(source, &lines, focus) {
        if let Some(sig_line) = find_function_signature_line(source, &lines, &call_name) {
            let sig_text = slice_chars(source, sig_line.0, sig_line.1)
                .trim()
                .to_string();
            return Some(TemplateSpec {
                labels: vec![DiagnosticLabel {
                    source_id: None,
                    span: Span {
                        start: sig_line.0,
                        end: sig_line.1,
                    },
                    message: sig_text,
                    color: Some(Color::Blue),
                }],
                notes: Vec::new(),
                help: None,
            });
        }
    }

    None
}

pub(crate) fn serializable_callable_hint_from_labels(spec: &DiagnosticSpec) -> Option<String> {
    if spec.kind != "TypeError"
        || !spec
            .message
            .starts_with("Argument type mismatch: expected ")
    {
        return None;
    }

    let signature = spec.labels.first()?.message.trim();
    if signature.is_empty()
        || signature.starts_with("LHS ")
        || signature.starts_with("RHS ")
        || signature.starts_with("OP:")
        || signature.starts_with("operator ")
        || signature.starts_with("left operand:")
        || signature.starts_with("right operand:")
    {
        return None;
    }

    Some(format!("Callable definition signature: {}", signature))
}

pub(crate) fn infer_operator_mismatch_template(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
    message: &str,
) -> Option<TemplateSpec> {
    let parsed = parse_binary_operator_error(message)?;
    let line_idx = line_index_for_span(lines, focus.start)?;
    let (line_start, line_end) = lines[line_idx];
    let line = slice_chars(source, line_start, line_end);
    let chars: Vec<char> = line.chars().collect();
    let focus_col = focus.start.saturating_sub(line_start);
    let op_span = match parsed.op_name_hint {
        Some(op_name) => find_backtick_operator_span(line_start, &chars, focus_col)
            .or_else(|| find_operator_symbol_span(line_start, &chars, op_name, focus_col))?,
        None => find_any_binary_operator_span(line_start, &chars, focus_col)?,
    };
    let op_symbol = slice_chars(source, op_span.start, op_span.end);
    let op_display = binary_operator_display_symbol(&op_symbol);
    let op_name = parsed
        .op_name_hint
        .or_else(|| binary_op_name_from_symbol(&op_display))
        .unwrap_or("Eq");
    let ((left_start, left_end), (right_start, right_end)) =
        find_binary_operand_spans(&chars, op_span.start - line_start, op_span.end - line_start)?;
    let left_ty = parsed.left_ty.as_deref().unwrap_or("unknown");
    let right_ty = parsed.right_ty.as_deref().unwrap_or(left_ty);
    let view =
        build_binary_operator_view(op_name, &op_display, left_ty, right_ty, parsed.failure_kind);
    Some(build_binary_operator_template(
        line_start,
        left_start,
        left_end,
        op_span,
        right_start,
        right_end,
        &view,
    ))
}

pub(crate) fn parse_binary_operator_error(message: &str) -> Option<ParsedBinaryOperatorError> {
    if let Some(tail) = message.strip_prefix("Cannot apply ") {
        let (op_name, types) = tail.split_once(" to ")?;
        let (left_ty, right_ty) = types.split_once(" and ")?;
        return Some(ParsedBinaryOperatorError {
            op_name_hint: Some(binary_canonical_op_name(op_name)?),
            left_ty: Some(left_ty.to_string()),
            right_ty: Some(right_ty.to_string()),
            failure_kind: BinaryOperatorFailureKind::IncompatibleTypes,
        });
    }
    if let Some(tail) = message.strip_prefix("Cannot compare ") {
        let tail = tail.split_once(". ").map(|(head, _)| head).unwrap_or(tail);
        let (left_ty, right_ty) = tail.split_once(" and ")?;
        return Some(ParsedBinaryOperatorError {
            op_name_hint: None,
            left_ty: Some(left_ty.to_string()),
            right_ty: Some(right_ty.to_string()),
            failure_kind: BinaryOperatorFailureKind::IncompatibleTypes,
        });
    }
    if let Some(tail) = message.strip_prefix("++ requires (String, String), got (") {
        let tail = tail
            .split_once(". ")
            .map(|(head, _)| head)
            .unwrap_or(tail)
            .strip_suffix(')')?;
        let (left_ty, right_ty) = tail.split_once(", ")?;
        return Some(ParsedBinaryOperatorError {
            op_name_hint: Some("Concat"),
            left_ty: Some(left_ty.to_string()),
            right_ty: Some(right_ty.to_string()),
            failure_kind: BinaryOperatorFailureKind::IncompatibleTypes,
        });
    }
    if let Some((symbol, trait_name)) = message
        .strip_prefix('`')
        .and_then(|tail| tail.split_once("` requires both operands to implement "))
    {
        return Some(ParsedBinaryOperatorError {
            op_name_hint: Some(
                binary_op_name_from_symbol(symbol)
                    .or_else(|| binary_canonical_op_name(trait_name))?,
            ),
            left_ty: None,
            right_ty: None,
            failure_kind: BinaryOperatorFailureKind::MissingImplementation,
        });
    }
    if let Some(ty) = message.strip_prefix("== / != not supported for ") {
        return Some(ParsedBinaryOperatorError {
            op_name_hint: None,
            left_ty: Some(ty.to_string()),
            right_ty: Some(ty.to_string()),
            failure_kind: BinaryOperatorFailureKind::MissingImplementation,
        });
    }
    if let Some(tail) = message.strip_prefix("++ requires values implementing Concat, got (") {
        let tail = tail.strip_suffix(')')?;
        let (left_ty, right_ty) = tail.split_once(", ")?;
        return Some(ParsedBinaryOperatorError {
            op_name_hint: Some("Concat"),
            left_ty: Some(left_ty.to_string()),
            right_ty: Some(right_ty.to_string()),
            failure_kind: BinaryOperatorFailureKind::MissingImplementation,
        });
    }
    None
}

pub(crate) fn build_binary_operator_view<'a>(
    op_name: &str,
    op_symbol: &str,
    left_ty: &'a str,
    right_ty: &'a str,
    failure_kind: BinaryOperatorFailureKind,
) -> BinaryOperatorView<'a> {
    let (lhs_bad, rhs_bad) =
        binary_operator_mismatch_sides(op_name, left_ty, right_ty, failure_kind);
    let lhs_display = flow_type_display(left_ty, lhs_bad);
    let rhs_display = flow_type_display(right_ty, rhs_bad);
    let result_ty = if matches!(op_name, "Add" | "Sub" | "Mul") {
        "A"
    } else {
        "Boolean"
    };
    match op_name {
        "Add" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: "A + A -> A (where A: Add)".into(),
            step: format!("{lhs_display} + {rhs_display} -> <type error>"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: "Use the same Add type on both sides, for example `Int + Int` or `Float + Float`.".into(),
        },
        "Sub" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: "A - A -> A (where A: Sub)".into(),
            step: format!("{lhs_display} - {rhs_display} -> <type error>"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: "Use the same Sub type on both sides, for example `Int - Int` or `Float - Float`.".into(),
        },
        "Mul" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: "A * A -> A (where A: Mul)".into(),
            step: format!("{lhs_display} * {rhs_display} -> <type error>"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: "Use the same Mul type on both sides, for example `Int * Int` or `Float * Float`.".into(),
        },
        "Eq" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: "A == A -> Boolean".into(),
            step: format!("{lhs_display} == {rhs_display} -> Boolean"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: "Compare two values of the same type, or convert one side before comparing.".into(),
        },
        "Neq" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: "A != A -> Boolean".into(),
            step: format!("{lhs_display} != {rhs_display} -> Boolean"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: "Compare two values of the same type, or convert one side before comparing.".into(),
        },
        "Lt" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: "A < A -> Boolean (where A: Lt)".into(),
            step: format!("{lhs_display} < {rhs_display} -> Boolean"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: "Use the same ordered type on both sides, or convert one side before comparing.".into(),
        },
        "Lte" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: "A <= A -> Boolean (where A: Lte)".into(),
            step: format!("{lhs_display} <= {rhs_display} -> Boolean"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: "Use the same ordered type on both sides, or convert one side before comparing.".into(),
        },
        "Gt" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: "A > A -> Boolean (where A: Gt)".into(),
            step: format!("{lhs_display} > {rhs_display} -> Boolean"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: "Use the same ordered type on both sides, or convert one side before comparing.".into(),
        },
        "Gte" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: "A >= A -> Boolean (where A: Gte)".into(),
            step: format!("{lhs_display} >= {rhs_display} -> Boolean"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: "Use the same ordered type on both sides, or convert one side before comparing.".into(),
        },
        "Concat" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: "String ++ String -> String".into(),
            step: format!("{lhs_display} ++ {rhs_display} -> String"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: "Convert both sides to String, or use an operator/helper that matches the current types.".into(),
        },
        other => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: format!("{other} requires compatible operands"),
            step: format!("{lhs_display} {op_symbol} {rhs_display} -> {result_ty}"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: "Use operand types that match the operator's contract.".into(),
        },
    }
}

pub(crate) fn build_binary_operator_template(
    line_start: usize,
    left_start: usize,
    left_end: usize,
    op_span: Span,
    right_start: usize,
    right_end: usize,
    view: &BinaryOperatorView<'_>,
) -> TemplateSpec {
    TemplateSpec {
        labels: vec![
            DiagnosticLabel {
                source_id: None,
                span: Span {
                    start: line_start + left_start,
                    end: line_start + left_end,
                },
                message: flow_operator_caption("LHS actual", view.lhs_actual),
                color: Some(Color::Blue),
            },
            DiagnosticLabel {
                source_id: None,
                span: op_span,
                message: flow_operator_caption("OP rule", &view.op_rule),
                color: Some(Color::Magenta),
            },
            DiagnosticLabel {
                source_id: None,
                span: Span {
                    start: line_start + right_start,
                    end: line_start + right_end,
                },
                message: flow_operator_caption("RHS actual", view.rhs_actual),
                color: Some(Color::Yellow),
            },
        ],
        notes: vec![format!("Step: {}", view.step), view.reason.clone()],
        help: Some(view.help.clone()),
    }
}

pub(crate) fn build_function_value_flow_template(
    line_start: usize,
    lhs_start: usize,
    lhs_end: usize,
    op_start: usize,
    op_end: usize,
    rhs_start: usize,
    rhs_end: usize,
    op: &str,
    lhs_expr: &str,
    rhs_expr: &str,
    message: &str,
) -> TemplateSpec {
    let rule = match op {
        ">>" => "(A -> B) >> (B -> C) -> (A -> C)",
        ">*" => "(A -> B) >* (B -> C) -> (A -> C)",
        ">=>" => "compose one-argument Result/List-returning functions",
        _ => "compose function values",
    };

    TemplateSpec {
        labels: vec![
            DiagnosticLabel {
                source_id: None,
                span: Span {
                    start: line_start + lhs_start,
                    end: line_start + lhs_end,
                },
                message: format!("LHS operand: {}", lhs_expr.trim()),
                color: Some(Color::Blue),
            },
            DiagnosticLabel {
                source_id: None,
                span: Span {
                    start: line_start + op_start,
                    end: line_start + op_end,
                },
                message: format!("OP rule: {}", rule),
                color: Some(Color::Magenta),
            },
            DiagnosticLabel {
                source_id: None,
                span: Span {
                    start: line_start + rhs_start,
                    end: line_start + rhs_end,
                },
                message: format!("RHS operand: {}", rhs_expr.trim()),
                color: Some(Color::Yellow),
            },
        ],
        notes: vec![
            format!(
                "These operands are parsed as expressions before `{}` checks whether they are function values.",
                op
            ),
            message.to_string(),
        ],
        help: Some(format!(
            "`{}` works on one-argument function values. Use `&name`, a closure, or a function-valued variable. If a call returns the function you want to compose, parenthesize the call like `(make_fn(...)) {} (other_fn(...))`.",
            op, op
        )),
    }
}

pub(crate) fn build_function_value_flow_template_with_signature(
    line_start: usize,
    lhs_start: usize,
    lhs_end: usize,
    op_start: usize,
    op_end: usize,
    rhs_start: usize,
    rhs_end: usize,
    op: &str,
    focus_is_lhs: bool,
    failing_signature: &str,
    opposite_expr: &str,
    message: &str,
    result_note: Option<&str>,
) -> TemplateSpec {
    let rule = match op {
        ">>" => "(A -> B) >> (B -> C) -> (A -> C)",
        ">*" => "(A -> Result<B> / List<B>) >* (B -> C) -> contextual function",
        ">=>" => "(A -> Result<B> / List<B>) >=> (B -> Result<C> / List<C>) -> contextual function",
        _ => "compose function values",
    };

    let signature_label = if focus_is_lhs {
        "LHS signature"
    } else {
        "RHS signature"
    };
    let opposite_label = if focus_is_lhs {
        "RHS operand"
    } else {
        "LHS operand"
    };

    let signature_span = if focus_is_lhs {
        Span {
            start: line_start + lhs_start,
            end: line_start + lhs_end,
        }
    } else {
        Span {
            start: line_start + rhs_start,
            end: line_start + rhs_end,
        }
    };
    let opposite_span = if focus_is_lhs {
        Span {
            start: line_start + rhs_start,
            end: line_start + rhs_end,
        }
    } else {
        Span {
            start: line_start + lhs_start,
            end: line_start + lhs_end,
        }
    };

    let mut notes = vec![message.to_string()];
    if let Some(note) = result_note {
        notes.insert(0, note.to_string());
    }

    TemplateSpec {
        labels: vec![
            DiagnosticLabel {
                source_id: None,
                span: signature_span,
                message: format!("{}: {}", signature_label, failing_signature),
                color: Some(Color::Blue),
            },
            DiagnosticLabel {
                source_id: None,
                span: Span {
                    start: line_start + op_start,
                    end: line_start + op_end,
                },
                message: format!("OP rule: {}", rule),
                color: Some(Color::Magenta),
            },
            DiagnosticLabel {
                source_id: None,
                span: opposite_span,
                message: format!("{}: {}", opposite_label, opposite_expr.trim()),
                color: Some(Color::Yellow),
            },
        ],
        notes,
        help: Some(format!(
            "`{}` works on one-argument function values. A call operand is typechecked first; only a resulting function value can participate in composition.",
            op
        )),
    }
}

pub(crate) fn infer_flow_operator_template(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
    message: &str,
    hint: Option<&str>,
) -> Option<TemplateSpec> {
    let op = ["|>=", "|*>", "|>", ">>", ">*", ">=>"]
        .into_iter()
        .find(|op| message.contains(&format!("`{}`", op)))?;
    let op_pattern: Vec<char> = op.chars().collect();
    let focus_line_idx = line_index_for_span(lines, focus.start);
    let mut line_indices = focus_line_idx
        .into_iter()
        .chain((0..lines.len()).filter(move |idx| Some(*idx) != focus_line_idx));
    let (line_start, chars, op_start) = line_indices.find_map(|line_idx| {
        let (line_start, line_end) = lines[line_idx];
        let line = slice_chars(source, line_start, line_end);
        let chars: Vec<char> = line.chars().collect();
        let op_start = find_subslice_outside_literals(&chars, &op_pattern, 0)?;
        Some((line_start, chars, op_start))
    })?;
    let op_end = op_start + op.chars().count();
    let lhs_start = find_assignment_eq_before(&chars, op_start)
        .map(|idx| idx + 1)
        .unwrap_or(0)
        .min(op_start);
    let (lhs_start, lhs_end) = trim_char_span(&chars, lhs_start, op_start);
    let (rhs_start, rhs_end) = trim_char_span(&chars, op_end, chars.len());
    let detail = hint.and_then(parse_operator_hint);
    if detail.is_none() && message.contains("requires a function value") {
        let line = chars.iter().collect::<String>();
        let lhs_expr = slice_chars(&line, lhs_start, lhs_end);
        let rhs_expr = slice_chars(&line, rhs_start, rhs_end);
        let op_abs_start = line_start + op_start;
        let focus_is_lhs = focus.start < op_abs_start;
        if let Some(signature) = hint
            .and_then(call_target_signature_from_hint)
            .or_else(|| hint.and_then(callable_definition_signature_from_hint))
            .or_else(|| hint.and_then(callable_type_signature_from_hint))
        {
            let result_note = hint
                .and_then(non_signature_hint_note)
                .filter(|note| !note.is_empty());
            return Some(build_function_value_flow_template_with_signature(
                line_start,
                lhs_start,
                lhs_end,
                op_start,
                op_end,
                rhs_start,
                rhs_end,
                op,
                focus_is_lhs,
                signature,
                if focus_is_lhs { &rhs_expr } else { &lhs_expr },
                message,
                result_note,
            ));
        }
        return Some(build_function_value_flow_template(
            line_start, lhs_start, lhs_end, op_start, op_end, rhs_start, rhs_end, op, &lhs_expr,
            &rhs_expr, message,
        ));
    }
    let lhs_actual = detail
        .as_ref()
        .map(|detail| detail.lhs.as_str())
        .unwrap_or("unknown");
    let rhs_actual = detail
        .as_ref()
        .map(|detail| detail.rhs.as_str())
        .unwrap_or("unknown");
    let (lhs_bad, rhs_bad) = flow_operator_mismatch_sides(op, message);
    let view = FlowOperatorView {
        lhs_actual,
        rhs_actual,
        op_rule: flow_operator_rule_display(op, lhs_actual, rhs_actual),
        step: lowered_flow_operator_rule(op, lhs_actual, rhs_actual, lhs_bad, rhs_bad),
        rule_detail: flow_operator_rule_detail(
            op,
            &flow_operator_rule_display(op, lhs_actual, rhs_actual),
        ),
        reason: flow_operator_reason(op, message, lhs_actual, rhs_actual),
        help: flow_operator_help(
            op,
            message,
            lhs_actual,
            rhs_actual,
            detail.as_ref().and_then(|detail| detail.extra.as_deref()),
        ),
    };

    Some(build_flow_operator_template(
        line_start, lhs_start, lhs_end, op_start, op_end, rhs_start, rhs_end, &view,
    ))
}

pub(crate) fn infer_ensure_predicate_template(
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
            source_id: None,
            span: Span {
                start: line_start + predicate_span.0,
                end: line_start + predicate_span.1,
            },
            message: "predicate must be a closure or capture, not a call result".into(),
            color: Some(Color::Yellow),
        }],
        notes: Vec::new(),
        help: None,
    })
}

pub(crate) fn infer_plain_rhs_required_flow_template(
    source: &str,
    lines: &[(usize, usize)],
    message: &str,
) -> Option<TemplateSpec> {
    let op = if message.starts_with("`|*>` expects a plain function on the right-hand side") {
        "|*>"
    } else if message.starts_with("`>*` expects a plain function on the right-hand side") {
        ">*"
    } else {
        return None;
    };
    let (line_start, chars, op_start, line_idx) =
        lines
            .iter()
            .enumerate()
            .find_map(|(line_idx, (line_start, line_end))| {
                let line = slice_chars(source, *line_start, *line_end);
                let chars: Vec<char> = line.chars().collect();
                let op_start =
                    find_subslice_outside_literals(&chars, &op.chars().collect::<Vec<_>>(), 0)?;
                Some((*line_start, chars, op_start, line_idx))
            })?;
    let op_end = op_start + op.chars().count();
    let lhs_start = find_assignment_eq_before(&chars, op_start)
        .map(|idx| idx + 1)
        .unwrap_or(0)
        .min(op_start);
    let (lhs_start, lhs_end) = trim_char_span(&chars, lhs_start, op_start);
    let (rhs_start, rhs_end) = trim_char_span(&chars, op_end, chars.len());
    let lhs_expr = slice_chars(&chars.iter().collect::<String>(), lhs_start, lhs_end);
    let rhs_expr = slice_chars(&chars.iter().collect::<String>(), rhs_start, rhs_end);
    let lhs_actual = infer_simple_binding_type(source, lines, line_idx, &lhs_expr)
        .unwrap_or_else(|| "unknown".into());
    let rhs_actual =
        infer_simple_callable_type(source, lines, &rhs_expr).unwrap_or_else(|| "unknown".into());
    let op_rule = flow_operator_rule_display(op, &lhs_actual, &rhs_actual);
    let view = FlowOperatorView {
        lhs_actual: &lhs_actual,
        rhs_actual: &rhs_actual,
        rule_detail: flow_operator_rule_detail(op, &op_rule),
        op_rule,
        step: lowered_flow_operator_rule(op, &lhs_actual, &rhs_actual, false, true),
        reason: flow_operator_reason(op, message, &lhs_actual, &rhs_actual),
        help: flow_operator_help(op, message, &lhs_actual, &rhs_actual, None),
    };

    Some(build_flow_operator_template(
        line_start, lhs_start, lhs_end, op_start, op_end, rhs_start, rhs_end, &view,
    ))
}

pub(crate) fn build_flow_operator_template(
    line_start: usize,
    lhs_start: usize,
    lhs_end: usize,
    op_start: usize,
    op_end: usize,
    rhs_start: usize,
    rhs_end: usize,
    view: &FlowOperatorView<'_>,
) -> TemplateSpec {
    let labels = vec![
        DiagnosticLabel {
            source_id: None,
            span: Span {
                start: line_start + lhs_start,
                end: line_start + lhs_end,
            },
            message: flow_operator_caption("LHS actual", view.lhs_actual),
            color: Some(Color::Blue),
        },
        DiagnosticLabel {
            source_id: None,
            span: Span {
                start: line_start + op_start,
                end: line_start + op_end,
            },
            message: flow_operator_caption("OP rule", &view.op_rule),
            color: Some(Color::Yellow),
        },
        DiagnosticLabel {
            source_id: None,
            span: Span {
                start: line_start + rhs_start,
                end: line_start + rhs_end,
            },
            message: flow_operator_caption("RHS actual", view.rhs_actual),
            color: Some(Color::Magenta),
        },
    ];
    let mut notes = vec![format!("Step: {}", view.step)];
    if let Some(rule_detail) = &view.rule_detail {
        notes.push(rule_detail.clone());
    }
    notes.push(view.reason.clone());

    TemplateSpec {
        labels,
        notes,
        help: Some(view.help.clone()),
    }
}

pub(crate) fn flow_operator_caption(prefix: &str, value: &str) -> String {
    const FLOW_PREFIX_WIDTH: usize = 10;
    format!("{prefix:>width$}: {value}", width = FLOW_PREFIX_WIDTH)
}

pub(crate) fn binary_operator_mismatch_sides(
    op_name: &str,
    left_ty: &str,
    right_ty: &str,
    failure_kind: BinaryOperatorFailureKind,
) -> (bool, bool) {
    if failure_kind == BinaryOperatorFailureKind::MissingImplementation {
        return (left_ty != "unknown", right_ty != "unknown");
    }
    match op_name {
        "Add" => {
            let lhs_impl = is_numeric_type(left_ty);
            let rhs_impl = is_numeric_type(right_ty);
            let rhs_differs = left_ty != right_ty;
            (!lhs_impl, !rhs_impl || rhs_differs)
        }
        "Sub" | "Mul" => {
            let lhs_impl = is_numeric_type(left_ty);
            let rhs_impl = is_numeric_type(right_ty);
            let rhs_differs = left_ty != right_ty;
            (!lhs_impl, !rhs_impl || rhs_differs)
        }
        "Eq" | "Neq" | "Lt" | "Lte" | "Gt" | "Gte" => (false, left_ty != right_ty),
        "Concat" => {
            let lhs_impl = left_ty == "String";
            let rhs_impl = right_ty == "String";
            let rhs_differs = left_ty != right_ty;
            (!lhs_impl, !rhs_impl || rhs_differs)
        }
        _ => (false, left_ty != right_ty),
    }
}

pub(crate) fn is_numeric_type(ty: &str) -> bool {
    matches!(ty.trim(), "Int" | "Float")
}

pub(crate) fn binary_canonical_op_name(op_name: &str) -> Option<&'static str> {
    match op_name {
        "Add" => Some("Add"),
        "Sub" => Some("Sub"),
        "Mul" => Some("Mul"),
        "Eq" => Some("Eq"),
        "Neq" => Some("Neq"),
        "Lt" => Some("Lt"),
        "Lte" => Some("Lte"),
        "Gt" => Some("Gt"),
        "Gte" => Some("Gte"),
        "Concat" => Some("Concat"),
        _ => None,
    }
}

pub(crate) fn binary_op_name_from_symbol(symbol: &str) -> Option<&'static str> {
    match symbol {
        "+" => Some("Add"),
        "-" => Some("Sub"),
        "*" => Some("Mul"),
        "==" => Some("Eq"),
        "!=" => Some("Neq"),
        "<" => Some("Lt"),
        "<=" => Some("Lte"),
        ">" => Some("Gt"),
        ">=" => Some("Gte"),
        "++" => Some("Concat"),
        _ => None,
    }
}

pub(crate) fn binary_operator_display_symbol(symbol: &str) -> String {
    symbol
        .strip_prefix('`')
        .and_then(|s| s.strip_suffix('`'))
        .unwrap_or(symbol)
        .to_string()
}

pub(crate) fn binary_operator_reason(
    op_name: &str,
    op_symbol: &str,
    lhs_display: &str,
    rhs_display: &str,
    failure_kind: BinaryOperatorFailureKind,
) -> String {
    match failure_kind {
        BinaryOperatorFailureKind::IncompatibleTypes => match op_name {
            "Add" | "Sub" | "Mul" => {
                if lhs_display == rhs_display {
                    format!(
                "Reason: `{}` requires an operator trait implementation, but both sides are {}.",
                        op_symbol, lhs_display
                    )
                } else {
                    format!(
                        "Reason: `{}` requires the same operator trait type on both sides, but got {} and {}.",
                        op_symbol, lhs_display, rhs_display
                    )
                }
            }
            "Eq" | "Neq" => format!(
                "Reason: `{}` compares two values of the same type, but got {} and {}.",
                op_symbol, lhs_display, rhs_display
            ),
            "Lt" | "Lte" | "Gt" | "Gte" => format!(
                "Reason: `{}` compares two ordered values of the same type, but got {} and {}.",
                op_symbol, lhs_display, rhs_display
            ),
            "Concat" => format!(
                "Reason: `++` is string concatenation, but got {} and {}.",
                lhs_display, rhs_display
            ),
            _ => format!(
                "Reason: operator `{}` cannot combine {} and {}.",
                op_symbol, lhs_display, rhs_display
            ),
        },
        BinaryOperatorFailureKind::MissingImplementation => match op_name {
            "Add" | "Sub" | "Mul" => format!(
                "Reason: {} does not implement {}, so `{}` is not available.",
                lhs_display, op_name, op_symbol
            ),
            "Eq" | "Neq" => format!(
                "Reason: {} does not implement Eq, so `{}` is not available.",
                lhs_display, op_symbol
            ),
            "Lt" | "Lte" | "Gt" | "Gte" => format!(
                "Reason: {} does not implement {}, so `{}` is not available.",
                lhs_display, op_name, op_symbol
            ),
            "Concat" => format!(
                "Reason: {} does not implement Concat, so `++` is not available.",
                lhs_display
            ),
            _ => format!(
                "Reason: {} does not implement the trait required by `{}`.",
                lhs_display, op_symbol
            ),
        },
    }
}

pub(crate) fn infer_simple_binding_type(
    source: &str,
    lines: &[(usize, usize)],
    current_line_idx: usize,
    expr: &str,
) -> Option<String> {
    let ident = expr.trim();
    if ident.is_empty()
        || !ident
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    for idx in (0..current_line_idx).rev() {
        let (line_start, line_end) = lines[idx];
        let line = slice_chars(source, line_start, line_end);
        let trimmed = line.trim();
        let prefix = format!("{}:", ident);
        if !trimmed.starts_with(&prefix) {
            continue;
        }
        let rest = trimmed[prefix.len()..].trim_start();
        let ty = rest.split('=').next()?.trim();
        if !ty.is_empty() {
            return Some(ty.to_string());
        }
    }
    None
}

pub(crate) fn infer_simple_callable_type(
    source: &str,
    lines: &[(usize, usize)],
    expr: &str,
) -> Option<String> {
    let trimmed = expr.trim();
    let name = trimmed.strip_suffix("()")?.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    for (line_start, line_end) in lines {
        let line = slice_chars(source, *line_start, *line_end);
        let trimmed = line.trim();
        let prefix = format!("def {}(", name);
        if !trimmed.starts_with(&prefix) {
            continue;
        }
        let rest = &trimmed[prefix.len()..];
        let (params, ret) = rest.split_once(") -> ")?;
        let param_ty = params.split(':').nth(1)?.trim().trim_end_matches(',');
        let ret_ty = ret.split_whitespace().next()?.trim_end_matches('{').trim();
        return Some(format!("({} -> {})", param_ty, ret_ty));
    }
    None
}

pub(crate) fn infer_extractor_template(
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
    let (pattern_span, caption) = if let Some(pattern_span) = extract_match_pattern_span(&chars) {
        (
            pattern_span,
            "extractor pattern checked against the match scrutinee",
        )
    } else if let Some(pattern_span) = extract_safebind_pattern_span(&chars) {
        (
            pattern_span,
            "extractor pattern checked against the SafeBind RHS",
        )
    } else {
        return None;
    };

    Some(TemplateSpec {
        labels: vec![DiagnosticLabel {
            source_id: None,
            span: Span {
                start: line_start + pattern_span.0,
                end: line_start + pattern_span.1,
            },
            message: caption.into(),
            color: Some(Color::Yellow),
        }],
        notes: Vec::new(),
        help: None,
    })
}

pub(crate) fn infer_total_bind_pattern_template(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
    message: &str,
    hint: Option<&str>,
) -> Option<TemplateSpec> {
    if message != "Only total MatchBlock patterns can be used with `=`" {
        return None;
    }
    let line_idx = line_index_for_span(lines, focus.start)?;
    let (line_start, line_end) = lines[line_idx];
    let line = slice_chars(source, line_start, line_end);
    let chars: Vec<char> = line.chars().collect();
    let eq_col = find_assignment_eq_before(&chars, chars.len())?;
    let lhs = trim_char_span(&chars, 0, eq_col);
    let rhs = trim_char_span(&chars, eq_col + 1, chars.len());
    let op_span = Span {
        start: line_start + eq_col,
        end: line_start + eq_col + 1,
    };

    Some(TemplateSpec {
        labels: vec![
            DiagnosticLabel {
                source_id: None,
                span: Span {
                    start: line_start + lhs.0,
                    end: line_start + lhs.1,
                },
                message: "LHS pattern: partial MatchBlock pattern".into(),
                color: Some(Color::Red),
            },
            DiagnosticLabel {
                source_id: None,
                span: op_span,
                message: hint
                    .unwrap_or("Use `=?` for partial destructuring and extractor-driven matches.")
                    .to_string(),
                color: Some(Color::Yellow),
            },
            DiagnosticLabel {
                source_id: None,
                span: Span {
                    start: line_start + rhs.0,
                    end: line_start + rhs.1,
                },
                message: "RHS value".into(),
                color: None,
            },
        ],
        notes: Vec::new(),
        help: None,
    })
}

pub(crate) fn infer_runtime_value_error_template(
    source: &str,
    focus: &Span,
    kind: &str,
    message: &str,
    literal_values: Option<(&str, &str)>,
) -> Option<TemplateSpec> {
    if !matches!(kind, "PatternMismatch" | "EmptyList" | "IndexOutOfBounds") {
        return None;
    }

    let lines = line_spans(source);
    let focus_line = line_index_for_span(&lines, focus.start)?;
    let (assignment_line_idx, bind_col) = find_safebind_assignment(&lines, focus_line, source)?;
    let (line_start, line_end) = lines[assignment_line_idx];
    let line = slice_chars(source, line_start, line_end);
    let chars: Vec<char> = line.chars().collect();
    let lhs = trim_char_span(&chars, 0, bind_col);
    let lhs_span = Span {
        start: line_start + lhs.0,
        end: line_start + lhs.1,
    };
    let op_span = Span {
        start: line_start + bind_col,
        end: line_start + bind_col + 2,
    };
    let rhs_span = safebind_terminal_rhs_span(source, &lines, (assignment_line_idx, bind_col))?;
    let lhs_text = slice_chars(source, lhs_span.start, lhs_span.end);
    let rhs_text = slice_chars(source, rhs_span.start, rhs_span.end);
    let input_source = classify_runtime_input_source(rhs_text.trim());

    if let Some((lhs_value, rhs_value)) = literal_values {
        return Some(TemplateSpec {
            labels: vec![
                DiagnosticLabel {
                    source_id: None,
                    span: lhs_span,
                    message: format!("LHS value: {}", lhs_value),
                    color: Some(Color::Red),
                },
                DiagnosticLabel {
                    source_id: None,
                    span: op_span,
                    message: "SafeBind partial match".into(),
                    color: Some(Color::Yellow),
                },
                DiagnosticLabel {
                    source_id: None,
                    span: rhs_span,
                    message: format!("RHS value: {}", rhs_value),
                    color: None,
                },
            ],
            notes: Vec::new(),
            help: None,
        });
    }

    let lhs_message =
        describe_runtime_pattern_failure(lhs_text.trim(), input_source, kind, message);

    Some(TemplateSpec {
        labels: vec![
            DiagnosticLabel {
                source_id: None,
                span: lhs_span,
                message: lhs_message,
                color: Some(Color::Red),
            },
            DiagnosticLabel {
                source_id: None,
                span: op_span,
                message: "SafeBind partial match".into(),
                color: Some(Color::Yellow),
            },
            DiagnosticLabel {
                source_id: None,
                span: rhs_span,
                message: format!("input source: {}", input_source),
                color: None,
            },
        ],
        notes: Vec::new(),
        help: None,
    })
}

pub(crate) fn classify_runtime_input_source(text: &str) -> &'static str {
    if is_string_literal(text) {
        "String"
    } else if text.starts_with('[') && text.ends_with(']') {
        "List"
    } else if matches!(text, "True" | "False") {
        "Boolean"
    } else if is_int_literal(text) {
        "Int"
    } else if text.starts_with('(') && text.ends_with(')') {
        "Tuple"
    } else {
        "value"
    }
}

pub(crate) fn describe_runtime_pattern_failure(
    pattern: &str,
    input_source: &str,
    kind: &str,
    _message: &str,
) -> String {
    if pattern.starts_with('[') && pattern.ends_with(']') {
        if pattern == "[]" {
            return format!("empty list pattern requires {}.len == 0", input_source);
        }
        if pattern.contains("..") {
            return format!(
                "head-tail list pattern requires a non-empty {}",
                input_source
            );
        }
        if kind == "PatternMismatch" {
            return "fixed-length list pattern item did not match the input source".into();
        }
        return format!(
            "fixed-length list pattern requires {}.len to match the pattern arity",
            input_source
        );
    }

    if is_literal_pattern(pattern) {
        return "literal pattern did not match the input source".into();
    }

    if pattern.starts_with('(') && pattern.ends_with(')') {
        return "tuple pattern did not match the input source".into();
    }

    if is_constructor_like_pattern(pattern) {
        return "constructor pattern did not match the input source".into();
    }

    "pattern did not match the input source".into()
}

pub(crate) fn is_literal_pattern(pattern: &str) -> bool {
    is_string_literal(pattern) || matches!(pattern, "True" | "False") || is_int_literal(pattern)
}

pub(crate) fn is_string_literal(text: &str) -> bool {
    (text.starts_with('"') && text.ends_with('"'))
        || (text.starts_with('\'') && text.ends_with('\''))
}

pub(crate) fn is_int_literal(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit())
}

pub(crate) fn is_constructor_like_pattern(pattern: &str) -> bool {
    let Some(head) = pattern.chars().next() else {
        return false;
    };
    head.is_ascii_uppercase()
}

pub(crate) fn split_runtime_literal_values(message: &str) -> (String, Option<(&str, &str)>) {
    let Some((base, rest)) = message.split_once("\t@@lhs=") else {
        return (message.to_string(), None);
    };
    let Some((lhs, rhs)) = rest.split_once("\t@@rhs=") else {
        return (message.to_string(), None);
    };
    (base.to_string(), Some((lhs, rhs)))
}

pub(crate) fn infer_runtime_error_template(
    source: &str,
    focus: &Span,
    message: &str,
    context: &RuntimeDiagnosticContext,
) -> Option<TemplateSpec> {
    let lines = line_spans(source);
    let call_name = call_name_at_span(source, &lines, focus);
    if let Some(builtin_name) = runtime_builtin_name_from_message(message, call_name.as_deref()) {
        return infer_builtin_runtime_error_template(source, &lines, focus, message, &builtin_name);
    }

    infer_vm_runtime_error_template(source, focus, message, context)
}

pub(crate) fn infer_builtin_runtime_error_template(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
    message: &str,
    builtin_name: &str,
) -> Option<TemplateSpec> {
    let (call_span, arg_spans) = find_call_site_and_args(source, lines, focus, builtin_name)?;
    let arg_span = runtime_builtin_argument_span(message, &arg_spans).unwrap_or(call_span.clone());
    let rule = runtime_builtin_expected_rule(message);

    Some(TemplateSpec {
        labels: vec![
            DiagnosticLabel {
                source_id: None,
                span: arg_span,
                message: message.to_string(),
                color: Some(Color::Red),
            },
            DiagnosticLabel {
                source_id: None,
                span: call_span.clone(),
                message: "call target".into(),
                color: Some(Color::Yellow),
            },
            DiagnosticLabel {
                source_id: None,
                span: call_span,
                message: format!("expected rule: {}", rule),
                color: Some(Color::Magenta),
            },
        ],
        notes: Vec::new(),
        help: None,
    })
}

pub(crate) fn infer_vm_runtime_error_template(
    source: &str,
    focus: &Span,
    message: &str,
    context: &RuntimeDiagnosticContext,
) -> Option<TemplateSpec> {
    let line_span =
        trimmed_line_span_containing(source, focus.start).unwrap_or_else(|| focus.clone());
    let rule = runtime_vm_rule_from_message(message)?;
    let mut labels = vec![
        DiagnosticLabel {
            source_id: None,
            span: line_span.clone(),
            message: message.to_string(),
            color: Some(Color::Red),
        },
        DiagnosticLabel {
            source_id: None,
            span: line_span.clone(),
            message: format!("runtime rule: {}", rule),
            color: Some(Color::Magenta),
        },
    ];
    if let Some(opcode) = context.opcode.as_deref() {
        labels.push(DiagnosticLabel {
            source_id: None,
            span: line_span,
            message: format!("opcode: {}", opcode),
            color: Some(Color::Yellow),
        });
    }
    Some(TemplateSpec {
        labels,
        notes: Vec::new(),
        help: None,
    })
}

pub(crate) fn runtime_builtin_name_from_message<'a>(
    message: &'a str,
    fallback: Option<&'a str>,
) -> Option<String> {
    if let Some((name, _)) = message.split_once(" expects ") {
        if is_identifier_like(name) {
            return Some(name.to_string());
        }
    }
    fallback
        .filter(|name| is_identifier_like(name))
        .map(|name| name.to_string())
}

pub(crate) fn runtime_builtin_expected_rule(message: &str) -> String {
    if let Some((_, expected)) = message.split_once(" expects ") {
        expected.to_string()
    } else if let Some((_, tail)) = message.split_once(" out of range for ") {
        let ty = tail.split(':').next().unwrap_or(tail).trim();
        format!("value must fit in {}", ty)
    } else {
        message.to_string()
    }
}

pub(crate) fn runtime_vm_rule_from_message(message: &str) -> Option<String> {
    if let Some(expected) = message.strip_prefix("JumpIfFalse: expected ") {
        return Some(format!(
            "JumpIfFalse requires {}, got a different stack value",
            expected
        ));
    }
    if let Some(expected) = message.strip_prefix("Expected ") {
        return Some(format!("opcode expected {}", expected));
    }
    if message == "GetField on non-tagged value" {
        return Some("GetField requires a tagged runtime value".into());
    }
    if message == "CallClosure expects a callable value" {
        return Some("CallClosure requires a callable value on the stack".into());
    }
    if message == "Stack underflow" {
        return Some("opcode attempted to pop more values than were available".into());
    }
    if message == "Frame stack underflow" {
        return Some("call/return machinery expected an active frame".into());
    }
    if message.starts_with("Invalid jump target: ") {
        return Some("jump target must point inside the active bytecode chunk".into());
    }
    if message.starts_with("LoadConst index out of bounds: ")
        || message.starts_with("LoadLocal out of bounds: ")
        || message.starts_with("StoreLocal out of bounds: ")
    {
        return Some("opcode referenced storage outside the current runtime bounds".into());
    }
    None
}

pub(crate) fn runtime_builtin_argument_span(message: &str, arg_spans: &[Span]) -> Option<Span> {
    if arg_spans.is_empty() {
        return None;
    }
    if message.contains(" as first argument")
        || message.contains(" as kind")
        || message.contains(" as pattern")
        || message.contains(" as idx")
        || message.contains(" as input")
        || message.contains(" as text")
        || message.contains(" as name")
    {
        return arg_spans.first().cloned();
    }
    if message.contains(" as second argument") || message.contains(" as replacement") {
        return arg_spans.get(1).cloned();
    }
    if let Some(arg_name) = message.split(" as ").nth(1) {
        let arg_name = arg_name.split(',').next().unwrap_or(arg_name);
        if arg_name == "detail" {
            return arg_spans.first().cloned();
        }
    }
    if message.contains(", got (") && arg_spans.len() >= 2 {
        return Some(Span {
            start: arg_spans.first()?.start,
            end: arg_spans.last()?.end,
        });
    }
    arg_spans.first().cloned()
}

pub(crate) fn find_call_site_and_args(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
    name: &str,
) -> Option<(Span, Vec<Span>)> {
    let line_idx = line_index_for_span(lines, focus.start)?;
    let (line_start, line_end) = lines[line_idx];
    let line = slice_chars(source, line_start, line_end);
    let chars: Vec<char> = line.chars().collect();
    let needle: Vec<char> = format!("{}(", name).chars().collect();
    let call_start = find_subslice_outside_literals(&chars, &needle, 0)?;
    let open = call_start + needle.len() - 1;
    let close = find_matching_paren(&chars, open)?;
    let call_span = Span {
        start: line_start + call_start,
        end: line_start + close + 1,
    };
    let arg_spans = split_call_argument_spans(line_start, &chars, open + 1, close);
    Some((call_span, arg_spans))
}

pub(crate) fn split_call_argument_spans(
    line_start: usize,
    chars: &[char],
    start: usize,
    end: usize,
) -> Vec<Span> {
    let mut args = Vec::new();
    let mut depth_paren = 0usize;
    let mut depth_bracket = 0usize;
    let mut arg_start = start;
    let mut idx = start;
    while idx < end {
        if is_quote_char(chars[idx]) {
            idx = skip_quoted_literal(chars, idx);
            continue;
        }
        match chars[idx] {
            '(' => depth_paren += 1,
            ')' => depth_paren = depth_paren.saturating_sub(1),
            '[' => depth_bracket += 1,
            ']' => depth_bracket = depth_bracket.saturating_sub(1),
            ',' if depth_paren == 0 && depth_bracket == 0 => {
                let span = trimmed_span_from_line(line_start, chars, arg_start, idx);
                if span.start < span.end {
                    args.push(span);
                }
                arg_start = idx + 1;
            }
            _ => {}
        }
        idx += 1;
    }
    let span = trimmed_span_from_line(line_start, chars, arg_start, end);
    if span.start < span.end {
        args.push(span);
    }
    args
}

pub(crate) fn find_matching_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut idx = open;
    while idx < chars.len() {
        if is_quote_char(chars[idx]) {
            idx = skip_quoted_literal(chars, idx);
            continue;
        }
        match chars[idx] {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
        idx += 1;
    }
    None
}

pub(crate) fn is_identifier_like(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

pub(crate) fn apply_runtime_provenance_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    context: &RuntimeDiagnosticContext,
    spec: &mut DiagnosticSpec,
) {
    let Some(source) = sources.source(source_id) else {
        return;
    };
    let Some(name) = runtime_builtin_name_from_message(
        &spec.message,
        call_name_at_span(source, &line_spans(source), &spec.primary_span).as_deref(),
    ) else {
        return;
    };
    if let Some((def_source_id, def_span, def_label)) =
        find_runtime_builtin_definition_label(sources, &name)
    {
        spec.labels.push(DiagnosticLabel {
            source_id: Some(def_source_id),
            span: def_span,
            message: def_label,
            color: Some(Color::Blue),
        });
    }
    if context.function.is_some() {
        // kept for future expansion; function context is already available in help/details.
    }
}

pub(crate) fn find_runtime_builtin_definition_label(
    sources: &SourceRegistry,
    builtin_name: &str,
) -> Option<(SourceId, Span, String)> {
    let builtin_needle = format!("@@builtin def {}(", builtin_name);
    let user_needle = format!("def {}(", builtin_name);
    for entry in sources.entries() {
        let lines = line_spans(&entry.source);
        if let Some(sig_line) = find_function_signature_line(&entry.source, &lines, builtin_name) {
            let text = slice_chars(&entry.source, sig_line.0, sig_line.1);
            if text.contains(&builtin_needle) || text.trim_start().starts_with(&user_needle) {
                return Some((
                    entry.id,
                    Span {
                        start: sig_line.0,
                        end: sig_line.1,
                    },
                    source_signature_caption(&entry.source, &lines, sig_line, builtin_name)
                        .unwrap_or_else(|| text.trim().to_string()),
                ));
            }
        }
        if let Some(span) = line_head_span_with_brace(&entry.source, &builtin_needle) {
            return Some((
                entry.id,
                span.clone(),
                slice_chars(&entry.source, span.start, span.end),
            ));
        }
    }
    None
}

pub(crate) fn extractor_input_context(
    source: &str,
    lines: &[(usize, usize)],
    error_line_idx: usize,
    message: &str,
) -> Option<(Span, String)> {
    let chars: Vec<char> = source.chars().collect();
    let focus_pos = lines
        .get(error_line_idx)
        .map(|(start, _)| *start)
        .unwrap_or(0);
    let observed_ty = extractor_observed_type_from_message(message)?;

    if let Some((match_start, open_brace, _)) = find_enclosing_match_block(&chars, focus_pos) {
        let scrutinee_span = match_scrutinee_span(&chars, match_start, open_brace)?;
        return Some((scrutinee_span, observed_ty));
    }

    let assignment = find_safebind_assignment(lines, error_line_idx, source)?;
    let rhs_span = safebind_terminal_rhs_span(source, lines, assignment)?;
    Some((rhs_span, observed_ty))
}

pub(crate) fn extractor_error_locus_span(
    source: &str,
    lines: &[(usize, usize)],
    error_line_idx: usize,
) -> Option<Span> {
    let (line_start, line_end) = *lines.get(error_line_idx)?;
    let line = slice_chars(source, line_start, line_end);
    let chars: Vec<char> = line.chars().collect();
    if let Some(pattern_span) = extract_match_pattern_span(&chars) {
        return Some(Span {
            start: line_start + pattern_span.0,
            end: line_start + pattern_span.1,
        });
    }
    let pattern_span = extract_safebind_pattern_span(&chars)?;
    Some(Span {
        start: line_start + pattern_span.0,
        end: line_start + pattern_span.1,
    })
}

pub(crate) fn extractor_name_and_rule(message: &str) -> Option<(String, String)> {
    let tail = message.strip_prefix("Extractor ")?;
    let (name, _) = tail.split_once(" expects ")?;
    let rule = if name == "uncons" {
        "uncons(head, tail) only matches List<$A> or String".to_string()
    } else {
        format!(
            "{}(...) only matches values accepted by its extractor input contract",
            name
        )
    };
    Some((name.to_string(), rule))
}

pub(crate) fn find_extractor_definition_label(
    sources: &SourceRegistry,
    extractor_name: &str,
) -> Option<(SourceId, Span, String)> {
    let builtin_needle = format!("@@builtin defextractor {}(", extractor_name);
    let user_needle = format!("defextractor {}(", extractor_name);

    for entry in sources.entries() {
        if let Some(span) = line_head_span_with_brace(&entry.source, &builtin_needle) {
            return Some((
                entry.id,
                span.clone(),
                slice_chars(&entry.source, span.start, span.end),
            ));
        }
        if let Some(span) = line_head_span_with_brace(&entry.source, &user_needle) {
            return Some((
                entry.id,
                span.clone(),
                slice_chars(&entry.source, span.start, span.end),
            ));
        }
    }

    None
}

pub(crate) fn extractor_observed_type_from_message(message: &str) -> Option<String> {
    let (_, got) = message.split_once(", got ")?;
    Some(got.trim().to_string())
}

pub(crate) fn infer_argument_mismatch_template(
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
            source_id: None,
            span: label_span,
            message: sig_text.to_string(),
            color: Some(Color::Blue),
        });
    } else if let Some(call_name) = call_name_at_span(source, lines, focus) {
        if let Some(sig_line) = find_function_signature_line(source, lines, &call_name) {
            if let Some(sig_text) = source_signature_caption(source, lines, sig_line, &call_name) {
                labels.push(DiagnosticLabel {
                    source_id: None,
                    span: Span {
                        start: sig_line.0,
                        end: sig_line.1,
                    },
                    message: sig_text,
                    color: Some(Color::Blue),
                });
            }
        }
    }
    if labels.is_empty() {
        return None;
    }
    Some(TemplateSpec {
        labels,
        notes: Vec::new(),
        help: None,
    })
}

pub(crate) fn infer_annotation_assignment_template(
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
                source_id: None,
                span: assignment.lhs_span,
                message: format!("LHS annotation: {}", expected),
                color: Some(Color::Blue),
            },
            DiagnosticLabel {
                source_id: None,
                span: rhs_span,
                message: format!("RHS expression: {}", got),
                color: Some(Color::Yellow),
            },
        ],
        notes: Vec::new(),
        help: None,
    })
}

#[derive(Debug, Clone)]
pub(crate) struct OperatorHintParts {
    lhs: String,
    rhs: String,
    extra: Option<String>,
}

pub(crate) fn is_flow_operator_message(message: &str) -> bool {
    ["`|>`", "`|*>`", "`|>=`", "`>>`", "`>*`", "`>=>`"]
        .into_iter()
        .any(|op| message.contains(op))
}

pub(crate) fn has_annotation_assignment_labels(spec: &DiagnosticSpec) -> bool {
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

pub(crate) fn parse_operator_hint(hint: &str) -> Option<OperatorHintParts> {
    let (_rule, rest) = hint.split_once(". LHS: ")?;
    let (lhs, rest) = rest.split_once(". RHS: ")?;
    let precedence = ". Operators share precedence and resolve left-to-right, so LHS is the type produced so far.";
    let (rhs, extra) = if let Some((rhs, rest)) = rest.split_once(precedence) {
        let extra = rest.trim();
        (
            rhs,
            if extra.is_empty() {
                None
            } else {
                Some(extra.to_string())
            },
        )
    } else {
        (rest.trim_end_matches('.'), None)
    };
    Some(OperatorHintParts {
        lhs: lhs.trim().to_string(),
        rhs: rhs.trim().to_string(),
        extra,
    })
}

pub(crate) fn flow_family_from_type(ty: &str) -> Option<&'static str> {
    let trimmed = ty.trim();
    if trimmed.starts_with("Result<") {
        Some("Result")
    } else if trimmed.starts_with("List<") {
        Some("List")
    } else if trimmed.starts_with("Option<") {
        Some("Option")
    } else {
        None
    }
}

pub(crate) fn flow_family_from_callable_output(ty: &str) -> Option<&'static str> {
    let (_input, output) = unary_function_parts_display(ty)?;
    flow_family_from_type(&output)
}

pub(crate) fn flow_operator_rule_display(op: &str, lhs_actual: &str, rhs_actual: &str) -> String {
    match op {
        "|>" => "A |> (A -> B) -> B".into(),
        ">>" => "(A -> B) >> (B -> C) -> (A -> C)".into(),
        "|*>" => match flow_family_from_type(lhs_actual) {
            Some("Result") => "Result<A> |*> (A -> B) -> Result<B>".into(),
            Some("List") => "List<A> |*> (A -> B) -> List<B>".into(),
            Some("Option") => "Option<A> |*> (A -> B) -> Option<B>".into(),
            _ => "Result/List/Option map".into(),
        },
        "|>=" => match flow_family_from_type(lhs_actual)
            .or_else(|| flow_family_from_callable_output(rhs_actual))
        {
            Some("Result") => "Result<A> |>= (A -> Result<B>) -> Result<B>".into(),
            Some("List") => "List<A> |>= (A -> List<B>) -> List<B>".into(),
            Some("Option") => "Option<A> |>= (A -> Option<B>) -> Option<B>".into(),
            _ => "Result/List/Option bind".into(),
        },
        ">*" => match flow_family_from_callable_output(lhs_actual) {
            Some("Result") => "(A -> Result<B>) >* (B -> C) -> (A -> Result<C>)".into(),
            Some("List") => "(A -> List<B>) >* (B -> C) -> (A -> List<C>)".into(),
            _ => "Result/List lifted compose".into(),
        },
        ">=>" => match flow_family_from_callable_output(lhs_actual)
            .or_else(|| flow_family_from_callable_output(rhs_actual))
        {
            Some("Result") => "(A -> Result<B>) >=> (B -> Result<C>) -> (A -> Result<C>)".into(),
            Some("List") => "(A -> List<B>) >=> (B -> List<C>) -> (A -> List<C>)".into(),
            _ => "Result/List Kleisli compose".into(),
        },
        _ => format!("{} rule", op),
    }
}

pub(crate) fn flow_operator_rule_detail(op: &str, summary: &str) -> Option<String> {
    match (op, summary) {
        ("|*>", "Result/List/Option map") => Some(
            "Rule: Result<A> |*> (A -> B)      -> Result<B>\n      List<A>   |*> (A -> B)      -> List<B>\n      Option<A> |*> (A -> B)      -> Option<B>"
                .into(),
        ),
        ("|>=", "Result/List/Option bind") => Some(
            "Rule: Result<A> |>= (A -> Result<B>) -> Result<B>\n      List<A>   |>= (A -> List<B>)   -> List<B>\n      Option<A> |>= (A -> Option<B>) -> Option<B>"
                .into(),
        ),
        (">*", "Result/List lifted compose") => Some(
            "Rule: (A -> Result<B>) >* (B -> C) -> (A -> Result<C>)\n      (A -> List<B>)   >* (B -> C) -> (A -> List<C>)"
                .into(),
        ),
        (">=>", "Result/List Kleisli compose") => Some(
            "Rule: (A -> Result<B>) >=> (B -> Result<C>) -> (A -> Result<C>)\n      (A -> List<B>)   >=> (B -> List<C>)   -> (A -> List<C>)"
                .into(),
        ),
        _ => None,
    }
}

pub(crate) fn lowered_flow_operator_rule(
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

pub(crate) fn flow_type_display(ty: &str, is_mismatch: bool) -> String {
    if is_mismatch {
        ty.fg(Color::Red).to_string()
    } else {
        ty.to_string()
    }
}

pub(crate) fn unary_function_parts_display(ty: &str) -> Option<(String, String)> {
    let trimmed = ty.trim();
    let inner = trimmed.strip_prefix('(')?.strip_suffix(')')?;
    let (input, output) = inner.split_once(" -> ")?;
    Some((input.trim().to_string(), output.trim().to_string()))
}

pub(crate) fn map_container_output_display(container_ty: &str, new_inner: &str) -> Option<String> {
    if container_ty.starts_with("Result<") && container_ty.ends_with('>') {
        Some(format!("Result<{}>", new_inner))
    } else if container_ty.starts_with("List<") && container_ty.ends_with('>') {
        Some(format!("List<{}>", new_inner))
    } else if container_ty.starts_with("Option<") && container_ty.ends_with('>') {
        Some(format!("Option<{}>", new_inner))
    } else {
        None
    }
}

pub(crate) fn flow_operator_mismatch_sides(op: &str, message: &str) -> (bool, bool) {
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

pub(crate) fn flow_operator_reason(
    op: &str,
    message: &str,
    lhs_actual: &str,
    rhs_actual: &str,
) -> String {
    match op {
        "|>" => {
            if let (Some(expected), Some(got)) = extract_expected_got(message) {
                format!("Reason: RHS expects {}, but LHS is {}.", expected, got)
            } else {
                format!("Reason: {}", message)
            }
        }
        "|*>" => {
            if let Some(got) = message
                .strip_prefix("`|*>` requires Functor implementation on the left, got ")
                .or_else(|| message.strip_prefix("`|*>` requires Result or List on the left, got "))
            {
                format!(
                    "Reason: LHS is {}, but `|*>` maps over a Functor such as Result<A>, List<A>, or Option<A>.",
                    got
                )
            } else if let Some((_prefix, got)) = message.split_once(
                "expects a plain function on the right-hand side; use `|>=` for contextual output",
            ) {
                let _ = got;
                if let Some((_input, output)) = unary_function_parts_display(rhs_actual) {
                    format!(
                        "Reason: RHS returns {}, but `|*>` maps with a plain function.",
                        output
                    )
                } else {
                    format!("Reason: {}", message)
                }
            } else if let (Some(expected), Some(got)) = extract_expected_got(message) {
                format!(
                    "Reason: LHS contains {}, but RHS expects {}.",
                    expected, got
                )
            } else {
                format!("Reason: {}", message)
            }
        }
        "|>=" => {
            if let Some(got) = message
                .strip_prefix("`|>=` requires Chainable implementation on the left, got ")
                .or_else(|| message.strip_prefix("`|>=` requires Result or List on the left, got "))
            {
                format!(
                    "Reason: LHS is {}, but `|>=` requires a Chainable such as Result<A>, List<A>, or Option<A>.",
                    got
                )
            } else if let Some(got) =
                message.strip_prefix("`|>=` requires the right-hand side to return Result, got ")
            {
                format!("Reason: RHS returns {}, but `|>=` requires Result<B>.", got)
            } else if let Some(got) =
                message.strip_prefix("`|>=` requires the right-hand side to return List, got ")
            {
                format!("Reason: RHS returns {}, but `|>=` requires List<B>.", got)
            } else if let Some(got) =
                message.strip_prefix("`|>=` requires the right-hand side to return Option, got ")
            {
                format!("Reason: RHS returns {}, but `|>=` requires Option<B>.", got)
            } else if message.contains("cannot use Option as a standard failure container for Result bind")
            {
                "Reason: LHS is Option, but Result bind in Surtr uses Result as the standard failure container.".into()
            } else if message.contains("cannot switch from Result into Option bind context") {
                "Reason: LHS is Result, but the RHS returns Option and `|>=` does not switch failure-container families implicitly.".into()
            } else if message.contains("cannot mix Result, List, and Option context") {
                let lhs_family = flow_family_from_type(lhs_actual).unwrap_or("Result/List/Option");
                let rhs_family =
                    flow_family_from_callable_output(rhs_actual).unwrap_or("Result/List/Option");
                format!(
                    "Reason: LHS is {}, but RHS returns {}.",
                    lhs_family, rhs_family
                )
            } else if let (Some(expected), Some(got)) = extract_expected_got(message) {
                format!(
                    "Reason: LHS contains {}, but RHS expects {}.",
                    expected, got
                )
            } else {
                format!("Reason: {}", message)
            }
        }
        ">>" => {
            if message.contains("left output type to match the right input type") {
                let lhs_out = unary_function_parts_display(lhs_actual)
                    .map(|(_, out)| out)
                    .unwrap_or_else(|| "unknown".into());
                let rhs_in = unary_function_parts_display(rhs_actual)
                    .map(|(input, _)| input)
                    .unwrap_or_else(|| "unknown".into());
                format!(
                    "Reason: left output is {}, but right input is {}.",
                    lhs_out, rhs_in
                )
            } else {
                format!("Reason: {}", message)
            }
        }
        ">*" => {
            if message.contains("requires Result or List on the left-hand side") {
                let lhs_out = unary_function_parts_display(lhs_actual)
                    .map(|(_, out)| out)
                    .unwrap_or_else(|| lhs_actual.to_string());
                format!(
                    "Reason: LHS returns {}, but `>*` expects Result<B> or List<B>.",
                    lhs_out
                )
            } else if message.contains("left contextual output to match the right input type") {
                let lhs_out = unary_function_parts_display(lhs_actual)
                    .map(|(_, out)| out)
                    .unwrap_or_else(|| "unknown".into());
                let rhs_in = unary_function_parts_display(rhs_actual)
                    .map(|(input, _)| input)
                    .unwrap_or_else(|| "unknown".into());
                format!(
                    "Reason: left contextual output is {}, but right input is {}.",
                    lhs_out, rhs_in
                )
            } else {
                format!("Reason: {}", message)
            }
        }
        ">=>" => format!("Reason: {}", message),
        _ => format!("Reason: {}", message),
    }
}

pub(crate) fn flow_operator_help(
    op: &str,
    message: &str,
    lhs_actual: &str,
    _rhs_actual: &str,
    extra: Option<&str>,
) -> String {
    match op {
        "|>" if flow_family_from_type(lhs_actual) == Some("Result") => {
            "Use `|*>` to map over the Ok value, or `|>=` if the RHS returns Result.".into()
        }
        "|>" if flow_family_from_type(lhs_actual) == Some("List") => {
            "Use `|*>` to map over each List element, or use a function that accepts the whole List.".into()
        }
        "|>" => {
            if let Some(expected) = extract_expected_got(message).0 {
                format!("Change the LHS to {}, or use a function that accepts {}.", expected, lhs_actual)
            } else {
                "Change the LHS value, or use a function that accepts the current LHS type.".into()
            }
        }
        "|*>" if message.contains("requires Functor implementation")
            || message.contains("requires Result or List on the left") =>
        {
            "Use `|>` for a plain value, or make the LHS Result/List/Option.".into()
        }
        "|*>" if message.contains("plain function on the right-hand side") => {
            "Use `|>=` to bind a function that already returns Result/List/Option.".into()
        }
        "|*>" => "Keep the RHS plain, or switch to `|>=` if it already returns Result/List/Option.".into(),
        "|>=" if message.contains("requires Chainable implementation")
            || message.contains("requires Result or List on the left") =>
        {
            "Use `|>` for a plain value, or make the LHS Result/List/Option.".into()
        }
        "|>=" if message.contains("right-hand side to return Result") => {
            "Use `|*>` to map over the Result value, or change the RHS to return Result.".into()
        }
        "|>=" if message.contains("right-hand side to return List") => {
            "Use `|*>` to map over the List value, or change the RHS to return List.".into()
        }
        "|>=" if message.contains("right-hand side to return Option") => {
            "Use `|*>` to map over the Option value, or change the RHS to return Option.".into()
        }
        "|>=" if message.contains("cannot use Option as a standard failure container for Result bind") => {
            "Convert the Option value explicitly with `from(value, Result)` before binding.".into()
        }
        "|>=" if message.contains("cannot switch from Result into Option bind context") => {
            "Wrap the RHS so it converts Option to Result explicitly with `from(value, Result)`.".into()
        }
        "|>=" if message.contains("cannot mix Result, List, and Option context") => {
            "Keep the same container family across bind.".into()
        }
        "|>=" => "Make the RHS input and container family match the LHS.".into(),
        ">>" => {
            let lhs_out = unary_function_parts_display(lhs_actual)
                .map(|(_, out)| out)
                .unwrap_or_else(|| "the left output".into());
            format!(
                "Change the RHS to accept {}, or insert a conversion function.",
                lhs_out
            )
        }
        ">*" if message.contains("requires Result or List on the left-hand side") => {
            extra.unwrap_or("Use `>>` for plain composition, or make the left function return Result/List.").into()
        }
        ">*" => "Keep the left side contextual and make the RHS accept its success value.".into(),
        ">=>" => extra.unwrap_or("Keep the same container family across both functions.").into(),
        _ => extra.unwrap_or("Check the operator rule against the LHS and RHS types.").into(),
    }
}

pub(crate) fn line_column_for_offset(source: &str, offset: usize) -> (u32, u32) {
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

pub(crate) fn extract_expected_got(message: &str) -> (Option<String>, Option<String>) {
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

pub(crate) fn infer_if_branch_mismatch_template(
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
                source_id: None,
                span: then_span,
                message: format!("then branch: {}", then_ty),
                color: Some(Color::Blue),
            },
            DiagnosticLabel {
                source_id: None,
                span: else_span,
                message: format!("else branch: {}", else_ty),
                color: Some(Color::Yellow),
            },
        ],
        notes: Vec::new(),
        help: Some(
            "if/3 requires both branches to return the same type. Use if_then/2 when only side effects are needed."
                .into(),
        ),
    })
}

pub(crate) fn infer_match_arm_mismatch_template(
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
        source_id: None,
        span: Span {
            start: match_start,
            end: (match_start + 5).min(chars.len()),
        },
        message: format!("match expression expects {}", expected_ty),
        color: Some(Color::Magenta),
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
            source_id: None,
            span: Span {
                start: *start,
                end: *end,
            },
            message,
            color: Some(color),
        });
    }

    Some(TemplateSpec {
        labels,
        notes: Vec::new(),
        help: Some("All match arms must return the same type.".into()),
    })
}

pub(crate) fn enclosing_def_lines(
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

pub(crate) fn call_name_at_span(
    source: &str,
    lines: &[(usize, usize)],
    focus: &Span,
) -> Option<String> {
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

pub(crate) fn callable_definition_signature_from_hint(hint: &str) -> Option<&str> {
    hint.strip_prefix("Callable definition signature: ")
        .map(|sig| sig.lines().next().unwrap_or(sig))
}

pub(crate) fn call_target_signature_from_hint(hint: &str) -> Option<&str> {
    hint.strip_prefix("Call target signature: ")
        .map(|sig| sig.lines().next().unwrap_or(sig))
}

pub(crate) fn callable_type_signature_from_hint(hint: &str) -> Option<&str> {
    hint.strip_prefix("Callable type signature: ")
        .map(|sig| sig.lines().next().unwrap_or(sig))
}

pub(crate) fn non_signature_hint_note(hint: &str) -> Option<&str> {
    hint.lines().find(|line| {
        !line.starts_with("Call target signature: ")
            && !line.starts_with("Callable definition signature: ")
            && !line.starts_with("Callable definition span: ")
            && !line.starts_with("Callable type signature: ")
    })
}

pub(crate) fn callable_definition_span_from_hint(hint: &str) -> Option<Span> {
    let line = hint
        .lines()
        .find_map(|line| line.strip_prefix("Callable definition span: "))?;
    let (start, end) = line.split_once("..")?;
    Some(Span {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
    })
}

pub(crate) fn callable_definition_from_hint(hint: &str) -> Option<(&str, Span)> {
    Some((
        callable_definition_signature_from_hint(hint)?,
        callable_definition_span_from_hint(hint)?,
    ))
}

pub(crate) fn find_function_signature_line(
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

pub(crate) fn source_signature_caption(
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

pub(crate) fn def_signature_from_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let after_def = trimmed.strip_prefix("def ")?;
    let before_body = after_def
        .split_once('{')
        .map(|(sig, _)| sig)
        .unwrap_or(after_def)
        .trim();
    Some(before_body.to_string())
}

pub(crate) fn enclosing_trait_impl_header(
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

pub(crate) fn enclosing_impl_target(
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

pub(crate) fn enclosing_defmod_path(
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

pub(crate) fn line_index_for_span(lines: &[(usize, usize)], pos: usize) -> Option<usize> {
    lines
        .iter()
        .position(|(start, end)| pos >= *start && pos <= *end)
}

pub(crate) fn line_spans(source: &str) -> Vec<(usize, usize)> {
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

pub(crate) fn slice_chars(source: &str, start: usize, end: usize) -> String {
    source
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

pub(crate) fn rewrite_line_at_span(source: &str, span: &Span, replacement: &str) -> Option<String> {
    let line = line_span_containing(source, span.start)?;
    let line_start = line.0;
    let line_end = line.1;
    if span.start < line_start || span.end > line_end || span.end < span.start {
        return None;
    }
    let before = slice_chars(source, line_start, span.start);
    let after = slice_chars(source, span.end, line_end);
    Some(
        format!("{}{}{}", before, replacement, after)
            .trim()
            .to_string(),
    )
}

const MAX_PIPE_SLOT_REWRITE_DEPTH: usize = 3;

#[derive(Debug, Clone)]
pub(crate) struct PipeCallFrame {
    callee: String,
    args: Vec<String>,
    slot_arg_index: usize,
}

pub(crate) fn split_top_level_call_args(call: &str) -> Option<(String, Vec<String>)> {
    let call = call.trim();
    if !call.ends_with(')') {
        return None;
    }
    let open = call.find('(')?;
    let close = call.rfind(')')?;
    if close != call.len().saturating_sub(1) {
        return None;
    }
    if close <= open {
        return None;
    }
    let callee = call[..open].trim().to_string();
    let body = &call[open + 1..close];
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in body.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                args.push(body[start..idx].trim().to_string());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = body[start..].trim();
    if !tail.is_empty() {
        args.push(tail.to_string());
    }
    Some((callee, args))
}

pub(crate) fn suggested_pipe_slot_rewrite(source: &str, pos: usize) -> Option<String> {
    let line = line_span_containing(source, pos)?;
    let line_text = slice_chars(source, line.0, line.1);
    let (pipe_idx, operator) = last_pipe_operator_in_line(&line_text, pos.saturating_sub(line.0))?;
    let lhs = slice_chars(&line_text, 0, pipe_idx);
    let rhs = slice_chars(
        &line_text,
        pipe_idx + operator.chars().count(),
        line_text.chars().count(),
    );
    let lhs = lhs.trim_end();
    let rhs = rhs.trim_start();
    match build_pipe_slot_rewrite_steps(rhs)? {
        PipeSlotRewrite::Expanded(steps) => Some(format_pipe_rewrite(lhs.trim(), operator, &steps)),
        PipeSlotRewrite::Closure(closure) => {
            Some(format!("{}\n  {} {}", lhs.trim(), operator, closure))
        }
    }
}

pub(crate) fn last_pipe_operator_in_line(
    line: &str,
    max_index: usize,
) -> Option<(usize, &'static str)> {
    let chars = line.chars().collect::<Vec<_>>();
    let limit = max_index.min(chars.len());
    let mut last = None;

    for idx in 0..limit {
        if chars[idx] != '|' {
            continue;
        }

        let operator = if chars.get(idx + 1) == Some(&'*') && chars.get(idx + 2) == Some(&'>') {
            Some("|*>")
        } else if chars.get(idx + 1) == Some(&'>') && chars.get(idx + 2) == Some(&'=') {
            Some("|>=")
        } else if chars.get(idx + 1) == Some(&'>') {
            Some("|>")
        } else {
            None
        };

        if let Some(operator) = operator {
            last = Some((idx, operator));
        }
    }

    last
}

#[derive(Debug, Clone)]
pub(crate) enum PipeSlotRewrite {
    Expanded(Vec<String>),
    Closure(String),
}

pub(crate) fn build_pipe_slot_rewrite_steps(rhs: &str) -> Option<PipeSlotRewrite> {
    let frames = collect_pipe_slot_frames(rhs)?;
    if frames.len() > MAX_PIPE_SLOT_REWRITE_DEPTH {
        let closure_body = replace_first_standalone_pipe_slot(rhs, "term")?;
        return Some(PipeSlotRewrite::Closure(format!(
            "{{|term| {}}}",
            closure_body.trim()
        )));
    }

    let mut steps = Vec::with_capacity(frames.len());
    let leaf = frames.last()?;
    steps.push(render_call(&leaf.callee, &leaf.args));

    for frame in frames.iter().rev().skip(1) {
        let step = if frame.slot_arg_index == 0 {
            let remaining_args = frame.args[1..].to_vec();
            render_call(&frame.callee, &remaining_args)
        } else {
            let mut args = frame.args.clone();
            args[frame.slot_arg_index] = "_1".into();
            render_call(&frame.callee, &args)
        };
        steps.push(step);
    }

    Some(PipeSlotRewrite::Expanded(steps))
}

pub(crate) fn collect_pipe_slot_frames(rhs: &str) -> Option<Vec<PipeCallFrame>> {
    let mut frames = Vec::new();
    let mut current = rhs.trim().to_string();

    loop {
        let (callee, args) = split_top_level_call_args(&current)?;
        let slot_indices = args
            .iter()
            .enumerate()
            .filter_map(|(idx, arg)| arg.contains("_1").then_some(idx))
            .collect::<Vec<_>>();
        if slot_indices.len() != 1 {
            return None;
        }

        let slot_arg_index = slot_indices[0];
        let nested = args.get(slot_arg_index)?.trim().to_string();
        frames.push(PipeCallFrame {
            callee,
            args,
            slot_arg_index,
        });

        if nested == "_1" {
            return Some(frames);
        }

        if frames.len() > MAX_PIPE_SLOT_REWRITE_DEPTH {
            return Some(frames);
        }

        current = nested;
    }
}

pub(crate) fn render_call(callee: &str, args: &[String]) -> String {
    if args.is_empty() {
        format!("{}()", callee)
    } else {
        format!("{}({})", callee, args.join(", "))
    }
}

pub(crate) fn format_pipe_rewrite(lhs: &str, operator: &str, steps: &[String]) -> String {
    let mut rendered = lhs.to_string();
    for step in steps {
        rendered.push_str("\n  ");
        rendered.push_str(operator);
        rendered.push(' ');
        rendered.push_str(step);
    }
    rendered
}

pub(crate) fn replace_first_standalone_pipe_slot(expr: &str, replacement: &str) -> Option<String> {
    let chars = expr.chars().collect::<Vec<_>>();
    for idx in 0..chars.len().saturating_sub(1) {
        if chars[idx] != '_' || chars[idx + 1] != '1' {
            continue;
        }

        let prev = idx
            .checked_sub(1)
            .and_then(|prev_idx| chars.get(prev_idx))
            .copied();
        let next = chars.get(idx + 2).copied();
        if prev == Some('.')
            || prev.is_some_and(is_identifier_char)
            || next.is_some_and(is_identifier_char)
        {
            continue;
        }

        let before = chars[..idx].iter().collect::<String>();
        let after = chars[idx + 2..].iter().collect::<String>();
        return Some(format!("{}{}{}", before, replacement, after));
    }

    None
}

pub(crate) fn is_identifier_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

pub(crate) fn trimmed_span_from_line(
    line_start: usize,
    chars: &[char],
    start: usize,
    end: usize,
) -> Span {
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

pub(crate) fn find_enclosing_match_block(
    chars: &[char],
    focus_pos: usize,
) -> Option<(usize, usize, usize)> {
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

pub(crate) fn match_scrutinee_span(
    chars: &[char],
    match_start: usize,
    open_brace: usize,
) -> Option<Span> {
    let start = match_start + "match".chars().count();
    let (scrutinee_start, scrutinee_end) = trim_char_span(chars, start, open_brace);
    (scrutinee_start < scrutinee_end).then_some(Span {
        start: scrutinee_start,
        end: scrutinee_end,
    })
}

pub(crate) fn collect_match_arm_body_spans(
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

pub(crate) fn is_match_keyword_at(chars: &[char], idx: usize) -> bool {
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

pub(crate) fn trim_char_span(chars: &[char], start: usize, end: usize) -> (usize, usize) {
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

pub(crate) fn find_assignment_eq_before(chars: &[char], limit: usize) -> Option<usize> {
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

pub(crate) fn line_span_containing(source: &str, pos: usize) -> Option<(usize, usize)> {
    let lines = line_spans(source);
    let idx = line_index_for_span(&lines, pos)?;
    Some(lines[idx])
}

pub(crate) fn trimmed_line_span_containing(source: &str, pos: usize) -> Option<Span> {
    trimmed_line_span(source, line_span_containing(source, pos)?)
}

#[derive(Debug, Clone)]
pub(crate) struct AnnotatedAssignment {
    line_idx: usize,
    eq_col: usize,
    lhs_span: Span,
}

pub(crate) fn find_annotated_assignment_line(
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

pub(crate) fn find_safebind_assignment(
    lines: &[(usize, usize)],
    focus_line: usize,
    source: &str,
) -> Option<(usize, usize)> {
    for idx in (0..=focus_line).rev() {
        let (line_start, line_end) = lines[idx];
        let line = slice_chars(source, line_start, line_end);
        let chars: Vec<char> = line.chars().collect();
        if let Some(bind) = find_subslice_outside_literals(&chars, &['=', '?'], 0) {
            return Some((idx, bind));
        }
    }
    None
}

pub(crate) fn assignment_rhs_span(
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

pub(crate) fn safebind_terminal_rhs_span(
    source: &str,
    lines: &[(usize, usize)],
    assignment: (usize, usize),
) -> Option<Span> {
    let (assignment_line_idx, bind_col) = assignment;
    let mut last_non_empty = None;
    for idx in assignment_line_idx + 1..lines.len() {
        let text = slice_chars(source, lines[idx].0, lines[idx].1);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            break;
        }
        if !starts_with_flow_operator(trimmed) {
            break;
        }
        last_non_empty = Some(idx);
    }

    if let Some(last_line_idx) = last_non_empty {
        return trimmed_rhs_focus_line_span(source, lines[last_line_idx]);
    }

    let (line_start, line_end) = lines[assignment_line_idx];
    let line = slice_chars(source, line_start, line_end);
    let chars: Vec<char> = line.chars().collect();
    let rhs = trimmed_span_from_line(line_start, &chars, bind_col + 2, chars.len());
    if rhs.start >= rhs.end {
        return None;
    }
    if let Some(op_rhs) = terminal_operator_rhs_span(line_start, &chars, bind_col + 2, chars.len())
    {
        return Some(op_rhs);
    }
    Some(rhs)
}

pub(crate) fn trimmed_rhs_focus_line_span(source: &str, line: (usize, usize)) -> Option<Span> {
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

pub(crate) fn terminal_operator_rhs_span(
    line_start: usize,
    chars: &[char],
    start: usize,
    end: usize,
) -> Option<Span> {
    let mut idx = start.min(chars.len());
    let limit = end.min(chars.len());
    let mut last_rhs = None;
    while idx < limit {
        if is_quote_char(chars[idx]) {
            idx = skip_quoted_literal(chars, idx);
            continue;
        }
        if let Some(op_len) = flow_operator_length_at(chars, idx) {
            let rhs_start = idx + op_len;
            let rhs = trim_char_span(chars, rhs_start, limit);
            if rhs.0 < rhs.1 {
                last_rhs = Some(Span {
                    start: line_start + rhs.0,
                    end: line_start + rhs.1,
                });
            }
            idx += op_len;
            continue;
        }
        idx += 1;
    }
    last_rhs
}

pub(crate) fn starts_with_flow_operator(trimmed: &str) -> bool {
    ["|>=", "|*>", "|>", ">>", ">*", ">=>"]
        .iter()
        .any(|op| trimmed.starts_with(op))
}

pub(crate) fn flow_operator_length_at(chars: &[char], idx: usize) -> Option<usize> {
    for op in ["|>=", "|*>", "|>", ">>", ">*", ">=>"] {
        let op_chars = op.chars().collect::<Vec<_>>();
        if idx + op_chars.len() <= chars.len() && chars[idx..idx + op_chars.len()] == op_chars[..] {
            return Some(op_chars.len());
        }
    }
    None
}

pub(crate) fn trimmed_line_span(source: &str, line: (usize, usize)) -> Option<Span> {
    let text = slice_chars(source, line.0, line.1);
    let chars: Vec<char> = text.chars().collect();
    let span = trimmed_span_from_line(line.0, &chars, 0, chars.len());
    (span.start < span.end).then_some(span)
}

pub(crate) fn identifier_span_at(source: &str, pos: usize) -> Option<Span> {
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

pub(crate) fn extract_backticked_target<'a>(
    message: &'a str,
    prefix: &str,
    suffix: &str,
) -> Option<&'a str> {
    let tail = message.strip_prefix(prefix)?;
    tail.strip_suffix(suffix)
}

pub(crate) fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

pub(crate) fn find_backtick_operator_span(
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

pub(crate) fn find_operator_symbol_span(
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
        "Concat" => "++",
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

pub(crate) fn find_any_binary_operator_span(
    line_start: usize,
    chars: &[char],
    focus_col: usize,
) -> Option<Span> {
    for op_name in [
        "Concat", "Eq", "Neq", "Lte", "Gte", "Lt", "Gt", "Add", "Sub", "Mul",
    ] {
        if let Some(span) = find_operator_symbol_span(line_start, chars, op_name, focus_col) {
            return Some(span);
        }
    }
    None
}

pub(crate) fn collect_call_argument_spans(
    chars: &[char],
    args_start: usize,
) -> Option<Vec<(usize, usize)>> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NestingDepth {
    paren: usize,
    bracket: usize,
    brace: usize,
}

pub(crate) fn find_binary_operand_spans(
    chars: &[char],
    op_start: usize,
    op_end: usize,
) -> Option<((usize, usize), (usize, usize))> {
    let target = nesting_depth_before(chars, op_start);
    let mut current = NestingDepth {
        paren: 0,
        bracket: 0,
        brace: 0,
    };
    let mut left_boundary = 0usize;
    let mut idx = 0usize;

    while idx < op_start.min(chars.len()) {
        if is_quote_char(chars[idx]) {
            idx = skip_quoted_literal(chars, idx);
            continue;
        }
        match chars[idx] {
            '(' => {
                current.paren += 1;
                if current == target {
                    left_boundary = idx + 1;
                }
            }
            '[' => {
                current.bracket += 1;
                if current == target {
                    left_boundary = idx + 1;
                }
            }
            '{' => {
                current.brace += 1;
                if current == target {
                    left_boundary = idx + 1;
                }
            }
            ')' => current.paren = current.paren.saturating_sub(1),
            ']' => current.bracket = current.bracket.saturating_sub(1),
            '}' => current.brace = current.brace.saturating_sub(1),
            '=' | ',' | ';' if current == target => {
                left_boundary = idx + 1;
            }
            _ => {}
        }
        idx += 1;
    }

    let left = trim_char_span(chars, left_boundary.min(op_start), op_start);
    if left.0 >= left.1 {
        return None;
    }

    let mut right_boundary = chars.len();
    current = nesting_depth_before(chars, op_end);
    idx = op_end.min(chars.len());

    while idx < chars.len() {
        if is_quote_char(chars[idx]) {
            idx = skip_quoted_literal(chars, idx);
            continue;
        }
        match chars[idx] {
            '(' => current.paren += 1,
            '[' => current.bracket += 1,
            '{' => current.brace += 1,
            ')' | ']' | '}' | ',' | ';' if current == target => {
                right_boundary = idx;
                break;
            }
            ')' => current.paren = current.paren.saturating_sub(1),
            ']' => current.bracket = current.bracket.saturating_sub(1),
            '}' => current.brace = current.brace.saturating_sub(1),
            _ => {}
        }
        idx += 1;
    }

    let right = trim_char_span(chars, op_end, right_boundary);
    if right.0 >= right.1 {
        return None;
    }

    Some((left, right))
}

pub(crate) fn nesting_depth_before(chars: &[char], limit: usize) -> NestingDepth {
    let mut depth = NestingDepth {
        paren: 0,
        bracket: 0,
        brace: 0,
    };
    let mut idx = 0usize;
    while idx < limit.min(chars.len()) {
        if is_quote_char(chars[idx]) {
            idx = skip_quoted_literal(chars, idx);
            continue;
        }
        match chars[idx] {
            '(' => depth.paren += 1,
            ')' => depth.paren = depth.paren.saturating_sub(1),
            '[' => depth.bracket += 1,
            ']' => depth.bracket = depth.bracket.saturating_sub(1),
            '{' => depth.brace += 1,
            '}' => depth.brace = depth.brace.saturating_sub(1),
            _ => {}
        }
        idx += 1;
    }
    depth
}

pub(crate) fn extract_match_pattern_span(chars: &[char]) -> Option<(usize, usize)> {
    let arrow = find_subslice_outside_literals(chars, &['=', '>'], 0)?;
    trim_after_line_indent(chars, 0, arrow)
}

pub(crate) fn extract_safebind_pattern_span(chars: &[char]) -> Option<(usize, usize)> {
    let bind = find_subslice_outside_literals(chars, &['=', '?'], 0)?;
    trim_after_line_indent(chars, 0, bind)
}

pub(crate) fn trim_after_line_indent(
    chars: &[char],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let span = trim_char_span(chars, start, end);
    (span.0 < span.1).then_some(span)
}

pub(crate) fn is_quote_char(ch: char) -> bool {
    matches!(ch, '"' | '\'')
}

pub(crate) fn skip_quoted_literal(chars: &[char], start: usize) -> usize {
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

pub(crate) fn find_char_outside_literals(
    chars: &[char],
    target: char,
    start: usize,
) -> Option<usize> {
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

pub(crate) fn find_subslice_outside_literals(
    chars: &[char],
    needle: &[char],
    start: usize,
) -> Option<usize> {
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
