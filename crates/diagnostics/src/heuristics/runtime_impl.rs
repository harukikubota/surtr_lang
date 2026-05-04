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
    let builtin_needle = format!("@builtin def {}(", builtin_name);
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

