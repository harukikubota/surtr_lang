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
    let builtin_needle = format!("@builtin defextractor {}(", extractor_name);
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
            } else if message
                .contains("cannot use Option as a standard failure container for Result bind")
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

