use std::io::IsTerminal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Color {
    Red,
    Green,
    Yellow,
    Magenta,
    Cyan,
    BrightBlack,
    BrightCyan,
    Default,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Style {
    fg: Option<Color>,
    bold: bool,
    dim: bool,
    underline: bool,
}

impl Style {
    fn fg(color: Color) -> Self {
        Self {
            fg: Some(color),
            ..Self::default()
        }
    }

    fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    fn underline(mut self) -> Self {
        self.underline = true;
        self
    }
}

pub fn color_enabled_auto() -> bool {
    std::io::stdout().is_terminal()
}

pub fn color_enabled_from_env() -> bool {
    match std::env::var("SURTR_REPL_COLOR") {
        Ok(value) if value.eq_ignore_ascii_case("always") => true,
        Ok(value) if value.eq_ignore_ascii_case("never") => false,
        _ if std::env::var_os("NO_COLOR").is_some() => false,
        _ => color_enabled_auto(),
    }
}

pub fn repl_result_line(line: &str) -> String {
    if let Some((head, inspected)) = line.split_once(" = ") {
        if let Some((name, ty)) = head.split_once(": ") {
            return concat([
                styled(name, Style::fg(Color::Cyan)),
                styled(": ", Style::fg(Color::Default).dim()),
                type_doc(ty),
                styled(" = ", Style::fg(Color::Default).dim()),
                inspect_doc(inspected),
            ]);
        }
    }

    if is_type_definition_line(line) {
        return styled(line, Style::fg(Color::BrightCyan));
    }

    inspect_doc(line)
}

pub fn doc_symbol(symbol: &str) -> String {
    styled(symbol, Style::default().bold())
}

pub fn doc_signature(signature: &str) -> String {
    concat([
        styled("sig: ", Style::fg(Color::Cyan)),
        signature_doc(signature),
    ])
}

pub fn signature(signature: &str) -> String {
    signature_doc(signature)
}

pub fn info_line(line: &str) -> String {
    if let Some(rest) = line.strip_prefix("## ") {
        return styled(rest, Style::fg(Color::Yellow).bold());
    }
    if let Some(rest) = line.strip_prefix("kind: ") {
        return concat([
            styled("kind", Style::fg(Color::BrightBlack).bold()),
            styled(": ", Style::fg(Color::BrightBlack)),
            styled(rest, Style::fg(Color::Yellow)),
        ]);
    }
    if let Some(rest) = line.strip_prefix("origin: ") {
        return concat([
            styled("origin", Style::fg(Color::BrightBlack).bold()),
            styled(": ", Style::fg(Color::BrightBlack)),
            styled(rest, Style::fg(Color::Green)),
        ]);
    }
    if let Some(rest) = line.strip_prefix("defined: ") {
        return concat([
            styled("defined", Style::fg(Color::BrightBlack).bold()),
            styled(": ", Style::fg(Color::BrightBlack)),
            signature_doc(rest),
        ]);
    }
    if let Some(rest) = line.strip_prefix("specialized: ") {
        return concat([
            styled("specialized", Style::fg(Color::BrightBlack).bold()),
            styled(": ", Style::fg(Color::BrightBlack)),
            source_doc(rest),
        ]);
    }
    if let Some(rest) = line.strip_prefix("type: ") {
        return concat([
            styled("type", Style::fg(Color::BrightBlack).bold()),
            styled(": ", Style::fg(Color::BrightBlack)),
            type_doc(rest),
        ]);
    }
    if let Some(rest) = line.strip_prefix("view result: ") {
        return concat([
            styled("view result", Style::fg(Color::BrightBlack).bold()),
            styled(": ", Style::fg(Color::BrightBlack)),
            type_doc(rest),
        ]);
    }
    if let Some(rest) = line.strip_prefix("full path: ") {
        return concat([
            styled("full path", Style::fg(Color::BrightBlack).bold()),
            styled(": ", Style::fg(Color::BrightBlack)),
            source_doc(rest),
        ]);
    }
    if let Some(rest) = line.strip_prefix("identity: ") {
        return concat([
            styled("identity", Style::fg(Color::BrightBlack).bold()),
            styled(": ", Style::fg(Color::BrightBlack)),
            source_doc(rest),
        ]);
    }
    if let Some(rest) = line.strip_prefix("hop ") {
        return concat([
            styled("hop ", Style::fg(Color::BrightBlack).bold()),
            styled(rest, Style::fg(Color::Cyan).bold()),
        ]);
    }
    if let Some(rest) = line.strip_prefix("relation: ") {
        return concat([
            styled("relation", Style::fg(Color::BrightBlack).bold()),
            styled(": ", Style::fg(Color::BrightBlack)),
            signature_doc(rest),
        ]);
    }
    if let Some(rest) = line.strip_prefix("cumulative: ") {
        return concat([
            styled("cumulative", Style::fg(Color::BrightBlack).bold()),
            styled(": ", Style::fg(Color::BrightBlack)),
            source_doc(rest),
        ]);
    }
    if let Some(rest) = line.strip_prefix("fallible: ") {
        return concat([
            styled("fallible", Style::fg(Color::BrightBlack).bold()),
            styled(": ", Style::fg(Color::BrightBlack)),
            styled(rest, Style::fg(Color::Yellow)),
        ]);
    }
    if let Some(rest) = line.strip_prefix("reason: ") {
        return concat([
            styled("reason", Style::fg(Color::BrightBlack).bold()),
            styled(": ", Style::fg(Color::BrightBlack)),
            source_doc(rest),
        ]);
    }
    if let Some(rest) = line.strip_prefix("stop ") {
        return concat([
            styled("stop ", Style::fg(Color::BrightBlack).bold()),
            styled(rest, Style::fg(Color::Magenta).bold()),
        ]);
    }
    if line == "none" {
        return styled(line, Style::fg(Color::BrightBlack).dim());
    }
    if line.contains("->") && line.contains('(') {
        return signature_doc(line);
    }
    if line.contains("::") || looks_like_source_line(line) {
        return source_doc(line);
    }
    styled(line, Style::default().bold())
}

pub fn doc_signature_banner(symbol: &str, signature: &str) -> String {
    signature_banner(symbol, signature, true)
}

pub fn plain_doc_signature_banner(symbol: &str, signature: &str) -> String {
    signature_banner(symbol, signature, false)
}

pub fn doc_body_lines(text: &str) -> Vec<String> {
    let mut rendered = Vec::new();
    let mut expect_repl_result = false;

    for line in normalized_doc_lines(text) {
        let prompt_line = line.find("xldr(").or_else(|| line.find("xldr>"));
        if prompt_line.is_some() {
            rendered.push(doc_body_line(&line));
            expect_repl_result = true;
            continue;
        }

        if expect_repl_result {
            let indent_len = line.len().saturating_sub(line.trim_start().len());
            rendered.push(format!(
                "{}{}",
                &line[..indent_len],
                repl_result_line(line.trim_start())
            ));
            expect_repl_result = false;
            continue;
        }

        rendered.push(doc_body_line(&line));
        expect_repl_result = false;
    }

    rendered
}

pub fn plain_doc_body_lines(text: &str) -> Vec<String> {
    normalized_doc_lines(text)
}

fn normalized_doc_lines(text: &str) -> Vec<String> {
    let mut lines = text.lines().collect::<Vec<_>>();
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    let common_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| visual_indent(line))
        .min()
        .unwrap_or(0);

    lines
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| strip_visual_indent(line, common_indent).to_string())
        .collect()
}

fn visual_indent(line: &str) -> usize {
    let mut columns = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => columns += 1,
            '\t' => columns += 4 - (columns % 4),
            _ => break,
        }
    }
    columns
}

fn strip_visual_indent(line: &str, columns_to_strip: usize) -> &str {
    let mut columns = 0usize;
    for (idx, ch) in line.char_indices() {
        let width = match ch {
            ' ' => 1,
            '\t' => 4 - (columns % 4),
            _ => return &line[idx..],
        };
        if columns + width > columns_to_strip {
            return &line[idx..];
        }
        columns += width;
        if columns == columns_to_strip {
            return &line[idx + ch.len_utf8()..];
        }
    }
    ""
}

pub(crate) fn doc_body_line(line: &str) -> String {
    let trimmed = line.trim_start();
    if trimmed.starts_with("##") {
        return styled(line, Style::fg(Color::Yellow).bold());
    }

    if let Some(prompt_start) = line.find("xldr(") {
        if let Some(prompt_end) = line[prompt_start..].find('>') {
            let prompt_end = prompt_start + prompt_end + 1;
            return concat([
                line[..prompt_start].to_string(),
                styled(&line[prompt_start..prompt_end], Style::fg(Color::Cyan)),
                source_doc(&line[prompt_end..]),
            ]);
        }
    }
    if let Some(prompt_start) = line.find("xldr>") {
        let prompt_end = prompt_start + "xldr>".len();
        return concat([
            line[..prompt_start].to_string(),
            styled(&line[prompt_start..prompt_end], Style::fg(Color::Cyan)),
            source_doc(&line[prompt_end..]),
        ]);
    }

    if looks_like_source_line(trimmed) {
        source_doc(line)
    } else {
        markdown_doc_line(line)
    }
}

fn signature_banner(symbol: &str, signature: &str, color: bool) -> String {
    let label = qualified_signature(symbol, signature);
    let width = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(96)
        .max(label.chars().count() + 4);
    let total_pad = width.saturating_sub(label.chars().count());
    let left = total_pad / 2;
    let right = total_pad - left;

    if !color {
        return format!("{}{}{}", " ".repeat(left), label, " ".repeat(right));
    }

    concat([" ".repeat(left), signature_doc(&label), " ".repeat(right)])
}

fn qualified_signature(symbol: &str, signature: &str) -> String {
    if let Some((module, tail)) = symbol.rsplit_once("::") {
        if signature == tail || signature.starts_with(&format!("{tail}(")) {
            return format!("{module}::{signature}");
        }
    }
    signature.to_string()
}

fn inspect_doc(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if ch.is_ascii_whitespace() {
            out.push(ch);
            continue;
        }

        if ch == '"' {
            let end = consume_string(idx, &mut chars, text);
            out.push_str(&styled(&text[idx..end], Style::fg(Color::Green)));
            continue;
        }

        if ch.is_ascii_digit()
            || (ch == '-' && chars.peek().is_some_and(|(_, next)| next.is_ascii_digit()))
        {
            let mut end = consume_while(idx, &mut chars, text, |c| {
                c.is_ascii_digit() || c == '_' || c == '.'
            });
            if text[end..].starts_with("ms") {
                end += "ms".len();
                chars.next();
                chars.next();
            }
            out.push_str(&styled(&text[idx..end], Style::fg(Color::Yellow)));
            continue;
        }

        if is_ident_start(ch) {
            let end = consume_while(idx, &mut chars, text, is_ident_continue);
            let word = &text[idx..end];
            let style = if matches!(word, "True" | "False") {
                Style::fg(Color::Yellow)
            } else if matches!(word, "NoneError" | "SomeError" | "ParseError" | "TypeError") {
                Style::fg(Color::Red)
            } else if word.chars().next().is_some_and(char::is_uppercase) {
                Style::fg(Color::Magenta).bold()
            } else {
                Style::default()
            };
            out.push_str(&styled(word, style));
            continue;
        }

        if is_operator_char(ch) {
            let end = consume_while(idx, &mut chars, text, is_operator_char);
            out.push_str(&styled(&text[idx..end], Style::fg(Color::Cyan)));
            continue;
        }

        out.push_str(&styled(
            &text[idx..idx + ch.len_utf8()],
            Style::fg(Color::Default).dim(),
        ));
    }

    out
}

fn type_doc(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if ch.is_ascii_whitespace() {
            out.push(ch);
            continue;
        }

        if is_ident_start(ch) {
            let end = consume_while(idx, &mut chars, text, is_ident_continue);
            let word = &text[idx..end];
            let style = if word.starts_with('$') {
                Style::fg(Color::Yellow).bold()
            } else {
                Style::fg(Color::BrightCyan)
            };
            out.push_str(&styled(word, style));
            continue;
        }

        if ch == '$' {
            let end = consume_while(idx, &mut chars, text, is_ident_continue);
            out.push_str(&styled(&text[idx..end], Style::fg(Color::Yellow).bold()));
            continue;
        }

        let style = if matches!(ch, '<' | '>' | ',' | ':' | '(' | ')' | '-') {
            Style::fg(Color::BrightBlack)
        } else {
            Style::fg(Color::Cyan)
        };
        out.push_str(&styled(&text[idx..idx + ch.len_utf8()], style));
    }

    out
}

fn source_doc(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if ch.is_ascii_whitespace() {
            out.push(ch);
            continue;
        }

        if ch == '"' {
            let end = consume_string(idx, &mut chars, text);
            out.push_str(&styled(&text[idx..end], Style::fg(Color::Green)));
            continue;
        }

        if ch.is_ascii_digit()
            || (ch == '-' && chars.peek().is_some_and(|(_, next)| next.is_ascii_digit()))
        {
            let mut end = consume_while(idx, &mut chars, text, |c| {
                c.is_ascii_digit() || c == '_' || c == '.'
            });
            if text[end..].starts_with("ms") {
                end += "ms".len();
                chars.next();
                chars.next();
            }
            out.push_str(&styled(&text[idx..end], Style::fg(Color::Yellow)));
            continue;
        }

        if is_ident_start(ch) {
            let end = consume_while(idx, &mut chars, text, is_ident_continue);
            let word = &text[idx..end];
            let style = if matches!(
                word,
                "def"
                    | "defmod"
                    | "defenum"
                    | "defrecord"
                    | "let"
                    | "match"
                    | "if"
                    | "else"
                    | "import"
                    | "type"
                    | "return"
            ) {
                Style::fg(Color::Magenta).bold()
            } else if matches!(word, "True" | "False") {
                Style::fg(Color::Yellow)
            } else if word.chars().next().is_some_and(char::is_uppercase) {
                Style::fg(Color::BrightCyan)
            } else {
                Style::default()
            };
            out.push_str(&styled(word, style));
            continue;
        }

        if is_operator_char(ch) {
            let end = consume_while(idx, &mut chars, text, is_operator_char);
            out.push_str(&styled(&text[idx..end], Style::fg(Color::Cyan)));
            continue;
        }

        let style = if ch == '@' {
            Style::fg(Color::Red).underline()
        } else {
            Style::fg(Color::BrightBlack)
        };
        out.push_str(&styled(&text[idx..idx + ch.len_utf8()], style));
    }

    out
}

fn signature_doc(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.char_indices().peekable();
    let mut after_colon = false;

    while let Some((idx, ch)) = chars.next() {
        if ch.is_ascii_whitespace() {
            out.push(ch);
            continue;
        }

        if is_ident_start(ch) {
            let end = consume_while(idx, &mut chars, text, is_ident_continue);
            let word = &text[idx..end];
            let next = text[end..].chars().find(|c| !c.is_ascii_whitespace());
            let style = if word.starts_with('$') {
                Style::fg(Color::Yellow).bold()
            } else if word.chars().next().is_some_and(char::is_uppercase) {
                Style::fg(Color::BrightCyan).bold()
            } else if next == Some(':') && !text[end..].starts_with("::") {
                Style::fg(Color::Cyan)
            } else {
                Style::fg(Color::Magenta).bold()
            };
            out.push_str(&styled(word, style));
            continue;
        }

        if ch == '$' {
            let end = consume_while(idx, &mut chars, text, is_ident_continue);
            out.push_str(&styled(&text[idx..end], Style::fg(Color::Yellow).bold()));
            continue;
        }

        if is_operator_char(ch) {
            let end = consume_while(idx, &mut chars, text, is_operator_char);
            let piece = &text[idx..end];
            after_colon = piece.ends_with(':');
            out.push_str(&styled(piece, Style::fg(Color::Cyan)));
            continue;
        }

        let piece = &text[idx..idx + ch.len_utf8()];
        let style = if after_colon {
            Style::fg(Color::BrightBlack)
        } else {
            Style::fg(Color::BrightBlack)
        };
        after_colon = false;
        out.push_str(&styled(piece, style));
    }

    out
}

fn markdown_doc_line(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;

    while let Some(start) = rest.find('`') {
        out.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('`') else {
            out.push_str(&styled("`", Style::fg(Color::BrightBlack)));
            rest = after_start;
            continue;
        };

        let code = &after_start[..end];
        out.push_str(&styled("`", Style::fg(Color::BrightBlack)));
        out.push_str(&code_literal_doc(code));
        out.push_str(&styled("`", Style::fg(Color::BrightBlack)));
        rest = &after_start[end + 1..];
    }

    out.push_str(rest);
    out
}

fn consume_string(
    start: usize,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    text: &str,
) -> usize {
    let mut escaped = false;
    let mut end = text.len();
    for (idx, ch) in chars.by_ref() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            end = idx + ch.len_utf8();
            break;
        }
    }
    end.max(start + 1)
}

fn consume_while(
    start: usize,
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    text: &str,
    pred: impl Fn(char) -> bool,
) -> usize {
    let mut end = text[start..]
        .chars()
        .next()
        .map(|c| start + c.len_utf8())
        .unwrap_or(start);
    while let Some(&(idx, ch)) = chars.peek() {
        if !pred(ch) {
            break;
        }
        chars.next();
        end = idx + ch.len_utf8();
    }
    end
}

fn styled(text: &str, style: Style) -> String {
    if style == Style::default() || text.is_empty() {
        return text.to_string();
    }

    let mut codes = Vec::new();
    if style.bold {
        codes.push("1".to_string());
    }
    if style.dim {
        codes.push("2".to_string());
    }
    if style.underline {
        codes.push("4".to_string());
    }
    if let Some(fg) = style.fg {
        codes.push(color_code(fg).to_string());
    }
    format!("\x1b[{}m{}\x1b[0m", codes.join(";"), text)
}

fn color_code(color: Color) -> u8 {
    let base = 30;
    match color {
        Color::Red => base + 1,
        Color::Green => base + 2,
        Color::Yellow => base + 3,
        Color::Magenta => base + 5,
        Color::Cyan => base + 6,
        Color::BrightBlack => 90,
        Color::BrightCyan => 96,
        Color::Default => 39,
    }
}

fn concat(parts: impl IntoIterator<Item = String>) -> String {
    parts.into_iter().collect()
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    is_ident_start(ch) || ch.is_ascii_digit()
}

fn is_operator_char(ch: char) -> bool {
    matches!(
        ch,
        '+' | '-' | '*' | '/' | '%' | '=' | '?' | '!' | '<' | '>' | '&' | '|' | ':' | '.'
    )
}

fn is_type_definition_line(line: &str) -> bool {
    line.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
        && line
            .chars()
            .all(|ch| is_ident_continue(ch) || matches!(ch, ':' | '<' | '>' | ',' | '$'))
}

fn code_literal_doc(code: &str) -> String {
    if looks_like_signature_literal(code) {
        signature_doc(code)
    } else {
        source_doc(code)
    }
}

fn looks_like_signature_literal(code: &str) -> bool {
    code.contains("->")
        || code.contains(": ")
        || (code.contains('(')
            && code.contains(')')
            && code.chars().any(|ch| ch.is_ascii_uppercase() || ch == '$'))
}

fn looks_like_source_line(trimmed: &str) -> bool {
    trimmed.starts_with("@@") || trimmed.starts_with("def ") || trimmed.starts_with("defmod ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_ansi(text: &str) -> bool {
        text.contains("\x1b[")
    }

    #[test]
    fn styles_binding_name_type_and_inspected_value() {
        let rendered = repl_result_line("t: List<Int> = [2, 3]");
        assert!(has_ansi(&rendered));
        assert!(rendered.contains("t"));
        assert!(rendered.contains("Int"));
        assert!(
            rendered.contains("\x1b[36mt\x1b[0m"),
            "binding names should match signature parameter cyan, got {rendered:?}"
        );
        assert!(
            !rendered.contains("\x1b[1;36mt\x1b[0m"),
            "binding names should not be bolder than signature parameters, got {rendered:?}"
        );
        assert!(
            rendered.contains("\x1b[96mList\x1b[0m"),
            "type names should use bright cyan, got {rendered:?}"
        );
        assert!(
            rendered.contains("\x1b[90m<\x1b[0m") && rendered.contains("\x1b[90m>\x1b[0m"),
            "type angle brackets should use gray, got {rendered:?}"
        );
        assert!(
            rendered.contains("\x1b[33m2\x1b[0m"),
            "binding values should keep inspect coloring, got {rendered:?}"
        );
    }

    #[test]
    fn constructor_return_value_is_not_treated_as_type_definition_line() {
        let rendered = repl_result_line("Ok(1)");
        assert!(has_ansi(&rendered));
        assert!(
            rendered.contains("\x1b[1;35mOk\x1b[0m"),
            "constructor should be styled independently, got {rendered:?}"
        );
        assert!(
            !rendered.starts_with("\x1b[96mOk(1)"),
            "constructor call must not inherit type-definition styling"
        );
    }

    #[test]
    fn duration_literals_keep_single_literal_coloring_in_results_and_source() {
        let result = repl_result_line("250ms");
        let source = doc_body_line("xldr(1)> Process::sleep(250ms)");
        assert!(result.contains("\x1b[33m250ms\x1b[0m"));
        assert!(source.contains("\x1b[33m250ms\x1b[0m"));
    }

    #[test]
    fn styles_doc_repl_prompt_and_source() {
        let rendered = doc_body_line("  xldr(1)> if(True, \"ok\", \"ng\")");
        assert!(has_ansi(&rendered));
        assert!(rendered.contains("xldr(1)>"));
        assert!(rendered.contains("True"));
    }

    #[test]
    fn doc_signature_banner_qualifies_and_styles_signature_parts() {
        let rendered = doc_signature_banner(
            "Kernel::if",
            "if(flag: Boolean, then_branch: (-> $A), else_branch: (-> $A)) -> $A",
        );
        assert!(rendered.contains("Kernel"));
        assert!(rendered.contains("if"));
        assert!(rendered.contains("\x1b[36mflag\x1b[0m"));
        assert!(rendered.contains("\x1b[1;96mBoolean\x1b[0m"));
        assert!(rendered.contains("\x1b[1;33m$A\x1b[0m"));
        assert!(
            !rendered.contains("43m"),
            "signature header should not use background color, got {rendered:?}"
        );
    }

    #[test]
    fn doc_body_lines_styles_repl_result_after_prompt() {
        let rendered = doc_body_lines("  xldr> Ok(1)\n  Ok(1)");
        assert_eq!(rendered.len(), 2);
        assert!(!rendered[0].starts_with("  "));
        assert!(rendered[0].contains("xldr>"));
        assert!(rendered[1].contains("\x1b[1;35mOk\x1b[0m"));
    }

    #[test]
    fn plain_doc_body_lines_dedents_common_space_and_tab_indent() {
        let rendered = plain_doc_body_lines("\n\t  First line\n\t  ## Heading\n\t    Nested\n");
        assert_eq!(
            rendered,
            vec![
                "First line".to_string(),
                "## Heading".to_string(),
                "  Nested".to_string(),
            ]
        );
    }

    #[test]
    fn markdown_code_literals_use_signature_coloring_for_signature_shapes() {
        let rendered = markdown_doc_line("  `if(Boolean, (-> $A), (-> $A)) -> $A`");
        assert!(rendered.contains("\x1b[1;96mBoolean\x1b[0m"));
        assert!(rendered.contains("\x1b[1;33m$A\x1b[0m"));
    }
}
