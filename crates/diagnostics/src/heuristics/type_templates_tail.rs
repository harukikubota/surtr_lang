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

