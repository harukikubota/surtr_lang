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

pub(crate) fn has_duplicate_pattern_binding_labels(spec: &DiagnosticSpec) -> bool {
    spec.message.starts_with("Duplicate binding in pattern: ")
        && spec.labels.iter().any(|label| label.message == "first")
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
        && spec.labels.iter().any(|label| label.message == "RHS value")
}

pub(crate) fn has_runtime_safebind_labels(spec: &DiagnosticSpec) -> bool {
    spec.labels
        .iter()
        .any(|label| label.message == "SafeBind partial match")
}

pub(crate) fn has_runtime_error_focus_labels(spec: &DiagnosticSpec) -> bool {
    spec.kind == "RuntimeError" && !spec.labels.is_empty()
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
