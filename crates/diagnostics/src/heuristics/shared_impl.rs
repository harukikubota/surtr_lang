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
    pub(crate) line_idx: usize,
    pub(crate) eq_col: usize,
    pub(crate) lhs_span: Span,
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
