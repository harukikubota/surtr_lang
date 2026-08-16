use sindr::operator_diagnostics::{
    operator_profile_by_name, operator_profile_by_symbol, OperatorFamily, BIND_RULE_TEXT,
};

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
    source_name_hint: Option<&'static str>,
    display_symbol_hint: Option<&'static str>,
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
    let (line_idx, line_start) = if let Some(call_name) = parsed.source_name_hint {
        let (call_span, _) = find_call_site_and_args(source, lines, focus, call_name)?;
        let line_idx = line_index_for_span(lines, call_span.start)?;
        (line_idx, lines[line_idx].0)
    } else {
        let line_idx = line_index_for_span(lines, focus.start)?;
        (line_idx, lines[line_idx].0)
    };
    let (left_start, left_end, right_start, right_end, op_span, op_display, op_name) =
        if let Some(call_name) = parsed.source_name_hint {
            let (call_span, arg_spans) = find_call_site_and_args(source, lines, focus, call_name)?;
            let [left_arg, right_arg] = arg_spans.as_slice() else {
                return None;
            };
            let op_span = Span {
                start: call_span.start,
                end: call_span.start + call_name.chars().count(),
            };
            (
                left_arg.start - line_start,
                left_arg.end - line_start,
                right_arg.start - line_start,
                right_arg.end - line_start,
                op_span,
                parsed
                    .display_symbol_hint
                    .unwrap_or(call_name)
                    .to_string(),
                parsed.op_name_hint.unwrap_or("Eq"),
            )
        } else {
            let (_, line_end) = lines[line_idx];
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
            let ((left_start, left_end), (right_start, right_end)) = find_binary_operand_spans(
                &chars,
                op_span.start - line_start,
                op_span.end - line_start,
            )?;
            (
                left_start,
                left_end,
                right_start,
                right_end,
                op_span,
                op_display,
                op_name,
            )
        };
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
            source_name_hint: None,
            display_symbol_hint: None,
            left_ty: Some(left_ty.to_string()),
            right_ty: Some(right_ty.to_string()),
            failure_kind: BinaryOperatorFailureKind::IncompatibleTypes,
        });
    }
    if let Some((symbol, rest)) = message
        .strip_prefix('`')
        .and_then(|tail| tail.split_once("` requires the same type on both sides, but got "))
    {
        let (left_ty, right_ty) = rest.split_once(" and ")?;
        return Some(ParsedBinaryOperatorError {
            op_name_hint: Some(binary_op_name_from_symbol(symbol)?),
            source_name_hint: None,
            display_symbol_hint: None,
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
            source_name_hint: None,
            display_symbol_hint: None,
            left_ty: Some(left_ty.to_string()),
            right_ty: Some(right_ty.to_string()),
            failure_kind: BinaryOperatorFailureKind::IncompatibleTypes,
        });
    }
    if let Some(tail) = message.strip_prefix("Eq::eq helper cannot compare ") {
        let (left_ty, right_ty) = tail.split_once(" and ")?;
        return Some(ParsedBinaryOperatorError {
            op_name_hint: Some("Eq"),
            source_name_hint: Some("eq"),
            display_symbol_hint: Some("=="),
            left_ty: Some(left_ty.to_string()),
            right_ty: Some(right_ty.to_string()),
            failure_kind: BinaryOperatorFailureKind::IncompatibleTypes,
        });
    }
    if let Some(tail) = message.strip_prefix("Eq::neq helper cannot compare ") {
        let (left_ty, right_ty) = tail.split_once(" and ")?;
        return Some(ParsedBinaryOperatorError {
            op_name_hint: Some("Neq"),
            source_name_hint: Some("neq"),
            display_symbol_hint: Some("!="),
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
            source_name_hint: None,
            display_symbol_hint: None,
            left_ty: Some(left_ty.to_string()),
            right_ty: Some(right_ty.to_string()),
            failure_kind: BinaryOperatorFailureKind::IncompatibleTypes,
        });
    }
    if let Some((left_ty, right_ty)) = message
        .strip_prefix("Concat::concat helper requires String on both sides, but got ")
        .and_then(|tail| tail.split_once(" and "))
    {
        return Some(ParsedBinaryOperatorError {
            op_name_hint: Some("Concat"),
            source_name_hint: Some("concat"),
            display_symbol_hint: Some("++"),
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
            source_name_hint: None,
            display_symbol_hint: None,
            left_ty: None,
            right_ty: None,
            failure_kind: BinaryOperatorFailureKind::MissingImplementation,
        });
    }
    if let Some((symbol, ty)) = message
        .strip_prefix('`')
        .and_then(|tail| tail.split_once("` is not defined for "))
    {
        return Some(ParsedBinaryOperatorError {
            op_name_hint: Some(binary_op_name_from_symbol(symbol)?),
            source_name_hint: None,
            display_symbol_hint: None,
            left_ty: Some(ty.to_string()),
            right_ty: Some(ty.to_string()),
            failure_kind: BinaryOperatorFailureKind::MissingImplementation,
        });
    }
    if let Some(ty) = message.strip_prefix("== / != not supported for ") {
        return Some(ParsedBinaryOperatorError {
            op_name_hint: None,
            source_name_hint: None,
            display_symbol_hint: None,
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
            source_name_hint: None,
            display_symbol_hint: None,
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
    let profile = operator_profile_by_name(op_name)
        .or_else(|| operator_profile_by_symbol(op_symbol))
        .copied();
    match op_name {
        "Add" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: profile
                .map(|profile| profile.canonical_rule)
                .unwrap_or("A + A -> A")
                .into(),
            step: format!("{lhs_display} + {rhs_display} -> <type error>"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: binary_operator_help(profile, failure_kind),
        },
        "Sub" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: profile
                .map(|profile| profile.canonical_rule)
                .unwrap_or("A - A -> A")
                .into(),
            step: format!("{lhs_display} - {rhs_display} -> <type error>"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: binary_operator_help(profile, failure_kind),
        },
        "Mul" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: profile
                .map(|profile| profile.canonical_rule)
                .unwrap_or("A * A -> A")
                .into(),
            step: format!("{lhs_display} * {rhs_display} -> <type error>"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: binary_operator_help(profile, failure_kind),
        },
        "Eq" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: profile
                .map(|profile| profile.canonical_rule)
                .unwrap_or("A == A -> Boolean")
                .into(),
            step: format!("{lhs_display} == {rhs_display} -> Boolean"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: binary_operator_help(profile, failure_kind),
        },
        "Neq" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: profile
                .map(|profile| profile.canonical_rule)
                .unwrap_or("A != A -> Boolean")
                .into(),
            step: format!("{lhs_display} != {rhs_display} -> Boolean"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: binary_operator_help(profile, failure_kind),
        },
        "Lt" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: profile
                .map(|profile| profile.canonical_rule)
                .unwrap_or("A < A -> Boolean")
                .into(),
            step: format!("{lhs_display} < {rhs_display} -> Boolean"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: binary_operator_help(profile, failure_kind),
        },
        "Lte" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: profile
                .map(|profile| profile.canonical_rule)
                .unwrap_or("A <= A -> Boolean")
                .into(),
            step: format!("{lhs_display} <= {rhs_display} -> Boolean"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: binary_operator_help(profile, failure_kind),
        },
        "Gt" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: profile
                .map(|profile| profile.canonical_rule)
                .unwrap_or("A > A -> Boolean")
                .into(),
            step: format!("{lhs_display} > {rhs_display} -> Boolean"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: binary_operator_help(profile, failure_kind),
        },
        "Gte" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: profile
                .map(|profile| profile.canonical_rule)
                .unwrap_or("A >= A -> Boolean")
                .into(),
            step: format!("{lhs_display} >= {rhs_display} -> Boolean"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: binary_operator_help(profile, failure_kind),
        },
        "Concat" => BinaryOperatorView {
            lhs_actual: left_ty,
            rhs_actual: right_ty,
            op_rule: profile
                .map(|profile| profile.canonical_rule)
                .unwrap_or("String ++ String -> String")
                .into(),
            step: format!("{lhs_display} ++ {rhs_display} -> String"),
            reason: binary_operator_reason(
                op_name,
                op_symbol,
                &lhs_display,
                &rhs_display,
                failure_kind,
            ),
            help: binary_operator_help(profile, failure_kind),
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
            line_start,
            lhs_start,
            lhs_end,
            op_start,
            op_end,
            rhs_start,
            rhs_end,
            op,
            &lhs_expr,
            &rhs_expr,
            message,
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
        line_start,
        lhs_start,
        lhs_end,
        op_start,
        op_end,
        rhs_start,
        rhs_end,
        &view,
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
        line_start,
        lhs_start,
        lhs_end,
        op_start,
        op_end,
        rhs_start,
        rhs_end,
        &view,
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
                "Reason: `{}` requires a visible implementation for {}.",
                        op_symbol, lhs_display
                    )
                } else {
                    format!(
                        "Reason: `{}` requires the same type on both sides, but got {} and {}.",
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
            "Add" | "Sub" | "Mul" | "Eq" | "Neq" | "Lt" | "Lte" | "Gt" | "Gte" | "Concat" =>
                format!("Reason: `{}` is not defined for {}.", op_symbol, lhs_display),
            _ => format!(
                "Reason: {} does not implement the trait required by `{}`.",
                lhs_display, op_symbol
            ),
        },
    }
}

fn binary_operator_help(
    profile: Option<sindr::operator_diagnostics::OperatorProfile>,
    failure_kind: BinaryOperatorFailureKind,
) -> String {
    match profile.map(|profile| profile.family) {
        Some(OperatorFamily::OpenSameTypeSelf) => match failure_kind {
            BinaryOperatorFailureKind::IncompatibleTypes => {
                "Use the same type on both sides.".into()
            }
            BinaryOperatorFailureKind::MissingImplementation => {
                "Use a type with a visible implementation for this operator.".into()
            }
        },
        Some(OperatorFamily::OpenSameTypeBoolean) => {
            "Compare two values of the same type, or convert one side before comparing.".into()
        }
        Some(OperatorFamily::OpenDedicatedReturn) => {
            "Convert both sides to String, or use an operator/helper that matches the current types.".into()
        }
        _ => "Use operand types that match the operator's contract.".into(),
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
                span: Span {
                    start: line_start + rhs.0,
                    end: line_start + rhs.1,
                },
                message: "RHS value".into(),
                color: None,
            },
        ],
        notes: vec![BIND_RULE_TEXT.to_string()],
        help: Some(
            hint.unwrap_or("Use `=?` for partial destructuring and extractor-driven matches.")
                .to_string(),
        ),
    })
}
