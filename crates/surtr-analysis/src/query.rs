use spire::ast::{Ast, AstPattern, AstTy, Span};
use std::ops::Deref;

const QUERY_OPERATORS: &[&str] = &[
    "|>=", "|*>", "|>", ">=>", ">*", ">>", "+", "-", "*", "&&", "||", "==", "!=", "<", "<=", ">",
    ">=", "->", "/", "%", "++", "=", "=?", "(,)", "|*|", "<|>",
];

#[derive(Debug, Clone, PartialEq)]
pub enum CommandQuery {
    Symbol(SymbolQuery),
    FacetRootDoc(SymbolQuery),
    FieldPath(SymbolQuery),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SymbolQuery {
    pub source: String,
    pub span: Span,
}

impl Deref for SymbolQuery {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandQueryParseError {
    reason: CommandQueryParseErrorReason,
    message: String,
    span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandQueryParseErrorReason {
    Empty,
    UnsupportedSymbol,
    UnsupportedForm,
}

impl CommandQueryParseError {
    fn new(reason: CommandQueryParseErrorReason, message: impl Into<String>, span: Span) -> Self {
        Self {
            reason,
            message: message.into(),
            span,
        }
    }

    pub fn reason(&self) -> CommandQueryParseErrorReason {
        self.reason
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    #[allow(dead_code)]
    pub fn span(&self) -> Span {
        self.span.clone()
    }
}

pub fn parse_command_query(input: &str) -> Result<CommandQuery, CommandQueryParseError> {
    let Some((trim_start, trim_end)) = trimmed_byte_bounds(input) else {
        let pos = input.chars().count();
        return Err(CommandQueryParseError::new(
            CommandQueryParseErrorReason::Empty,
            "REPL query cannot be empty.",
            Span {
                start: pos,
                end: pos,
            },
        ));
    };
    let trimmed = &input[trim_start..trim_end];
    let ctx = ParseContext {
        source: trimmed,
        base_char: input[..trim_start].chars().count(),
    };

    if let Some(type_ref) = trimmed.strip_prefix("Facet.") {
        if is_type_ref(type_ref) {
            return Ok(CommandQuery::FacetRootDoc(SymbolQuery {
                source: type_ref.to_string(),
                span: ctx.full_span(),
            }));
        }
    }

    if is_field_path_ref(trimmed) {
        return Ok(CommandQuery::FieldPath(SymbolQuery {
            source: trimmed.to_string(),
            span: ctx.full_span(),
        }));
    }

    if is_symbol_ref(trimmed) {
        return Ok(CommandQuery::Symbol(SymbolQuery {
            source: trimmed.to_string(),
            span: ctx.full_span(),
        }));
    }

    let (reason, message) = if trimmed.chars().any(char::is_whitespace) {
        (CommandQueryParseErrorReason::UnsupportedForm,
         "Unsupported command query form. Use a name or a public symbol; arguments are not supported.".to_string())
    } else {
        (
            CommandQueryParseErrorReason::UnsupportedSymbol,
            format!("Unsupported command query symbol `{trimmed}`."),
        )
    };
    Err(CommandQueryParseError::new(
        reason,
        message,
        ctx.full_span(),
    ))
}

#[derive(Debug, Clone, Copy)]
struct ParseContext<'a> {
    source: &'a str,
    base_char: usize,
}

impl ParseContext<'_> {
    fn full_span(&self) -> Span {
        self.span_for_local_bytes(0, self.source.len())
    }

    fn span_for_local_bytes(&self, start: usize, end: usize) -> Span {
        Span {
            start: self.base_char + self.source[..start].chars().count(),
            end: self.base_char + self.source[..end].chars().count(),
        }
    }
}

fn trimmed_byte_bounds(input: &str) -> Option<(usize, usize)> {
    let start = input
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, _)| idx)?;
    let end = input
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(start);
    Some((start, end))
}

fn parse_query_type(input: &str) -> Option<AstTy> {
    let source = format!("_: {input} = ()");
    let ast = spire::parse_with_context(
        &source,
        spire::ParserContext::repl(0).with_rules(spire::ParseRules::repl_chunk()),
    )
    .ok()?;
    match ast.as_slice() {
        [Ast::Bind(_, AstPattern::AnnotatedWildcard(_, ty), _)] => Some(ty.clone()),
        _ => None,
    }
}

pub fn parse_signature_type(input: &str) -> Option<AstTy> {
    parse_query_type(input)
}

pub fn parse_binding_query_type(input: &str) -> Option<AstTy> {
    parse_query_type(input).map(|ty| normalize_binding_query_type(&ty))
}

fn normalize_binding_query_type(ty: &AstTy) -> AstTy {
    match ty {
        AstTy::Named(_, _) | AstTy::ImplTrait(_, _) => ty.clone(),
        AstTy::Generic(span, name, args) if name == "Result" && args.len() == 2 => AstTy::Generic(
            span.clone(),
            name.clone(),
            vec![normalize_binding_query_type(&args[0])],
        ),
        AstTy::Generic(span, name, args) => AstTy::Generic(
            span.clone(),
            name.clone(),
            args.iter().map(normalize_binding_query_type).collect(),
        ),
        AstTy::Tuple(span, items) => AstTy::Tuple(
            span.clone(),
            items.iter().map(normalize_binding_query_type).collect(),
        ),
        AstTy::Func(span, params, ret) => AstTy::Func(
            span.clone(),
            params.iter().map(normalize_binding_query_type).collect(),
            Box::new(normalize_binding_query_type(ret)),
        ),
    }
}

/// Render completion signatures without declaration-only error contracts.
/// The stored declaration text remains available to REPL commands.
pub fn format_completion_signature(signature: &str) -> String {
    let chars = signature.chars().collect::<Vec<_>>();
    let mut hidden = vec![false; chars.len()];
    let mut delimiters: Vec<(char, bool, Option<usize>)> = Vec::new();
    for (index, &ch) in chars.iter().enumerate() {
        match ch {
            '<' => {
                let mut end = index;
                while end > 0 && chars[end - 1].is_whitespace() {
                    end -= 1;
                }
                let mut start = end;
                while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
                    start -= 1;
                }
                let name = chars[start..end].iter().collect::<String>();
                delimiters.push(('<', matches!(name.as_str(), "Result" | "MatchResult"), None));
            }
            '(' | '[' | '{' => delimiters.push((ch, false, None)),
            ',' => {
                if let Some(('<', true, error_start)) = delimiters.last_mut() {
                    error_start.get_or_insert(index);
                }
            }
            '>' if index == 0 || chars[index - 1] != '-' => {
                if matches!(delimiters.last(), Some(('<', _, _))) {
                    if let Some(('<', true, Some(start))) = delimiters.pop() {
                        hidden[start..index].fill(true);
                    }
                }
            }
            ')' | ']' | '}' => {
                let opening = match ch {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                if delimiters
                    .last()
                    .is_some_and(|(delimiter, _, _)| *delimiter == opening)
                {
                    delimiters.pop();
                }
            }
            _ => {}
        }
    }
    chars
        .into_iter()
        .zip(hidden)
        .filter_map(|(ch, hidden)| (!hidden).then_some(ch))
        .collect()
}

pub fn format_query_ty(ty: &AstTy) -> String {
    match ty {
        AstTy::Named(_, name) => name.clone(),
        AstTy::ImplTrait(_, name) => format!("impl {name}"),
        AstTy::Generic(_, name, args) => {
            let args = args
                .iter()
                .map(format_query_ty)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}<{args}>")
        }
        AstTy::Tuple(_, items) => format!(
            "({})",
            items
                .iter()
                .map(format_query_ty)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        AstTy::Func(_, params, ret) => {
            if params.is_empty() {
                format!("(-> {})", format_query_ty(ret))
            } else {
                format!(
                    "({} -> {})",
                    params
                        .iter()
                        .map(format_query_ty)
                        .collect::<Vec<_>>()
                        .join(", "),
                    format_query_ty(ret)
                )
            }
        }
    }
}

fn is_lexical_identifier(input: &str) -> bool {
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_simple_name(input: &str) -> bool {
    is_lexical_identifier(input)
        && input != "_"
        && !input.strip_prefix('_').is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|ch| ch.is_ascii_digit())
        })
}

fn is_type_ref(input: &str) -> bool {
    input
        .split("::")
        .all(|segment| !segment.is_empty() && is_simple_name(segment))
        && input
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn is_field_path_ref(input: &str) -> bool {
    let Some((root, segments)) = input.split_once('.') else {
        return false;
    };
    root.split("::").all(is_simple_name)
        && segments
            .split('.')
            .all(|segment| !segment.is_empty() && is_lexical_identifier(segment))
}

fn is_symbol_ref(input: &str) -> bool {
    let mut segments = input.rsplit("::");
    let member = segments.next().expect("split yields at least one segment");
    let valid_member = QUERY_OPERATORS.contains(&member)
        || member
            .strip_suffix(['?', '!'])
            .map_or_else(|| is_simple_name(member), is_simple_name);
    valid_member && segments.all(is_simple_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_signature_normalizes_nested_carriers_without_changing_declarations() {
        for (source, expected) in [
            ("read(value: Int) -> Result<Int, NoneError>", "read(value: Int) -> Result<Int>"),
            ("extract(value: Int) -> MatchResult<(Int, Int), Error>", "extract(value: Int) -> MatchResult<(Int, Int)>"),
            ("make() -> Result<(Int -> Result<String, Error>), NoneError>", "make() -> Result<(Int -> Result<String>)>"),
            ("型(value: List<Result<Int, Error>>) -> ExtractorClosure<(Int -> MatchResult<(Int, String), Error>)>", "型(value: List<Result<Int>>) -> ExtractorClosure<(Int -> MatchResult<(Int, String)>)>"),
            ("OtherResult<Int, Error>", "OtherResult<Int, Error>"),
        ] {
            assert_eq!(format_completion_signature(source), expected);
        }
    }

    #[test]
    fn parse_symbol_query_tracks_char_span() {
        let query = parse_command_query("  value  ").expect("query should parse");
        assert!(matches!(
            query,
            CommandQuery::Symbol(SymbolQuery { ref source, span })
            if source == "value" && span == Span { start: 2, end: 7 }
        ));
    }

    #[test]
    fn parse_facet_root_and_field_path_queries() {
        let facet_root = parse_command_query("  Facet.User  ").expect("query should parse");
        assert!(matches!(
            facet_root,
            CommandQuery::FacetRootDoc(SymbolQuery { ref source, span })
                if source == "User" && span == Span { start: 2, end: 12 }
        ));

        let field_path = parse_command_query("User.password").expect("query should parse");
        assert!(matches!(
            field_path,
            CommandQuery::FieldPath(SymbolQuery { ref source, span })
                if source == "User.password" && span == Span { start: 0, end: 13 }
        ));
    }

    #[test]
    fn signature_type_helpers_remain_independent_from_command_query_syntax() {
        let signature = parse_signature_type("Result<Int, Error>").expect("declaration type");
        assert_eq!(format_query_ty(&signature), "Result<Int, Error>");
        let binding = parse_binding_query_type("Result<Int, Error>").expect("binding type");
        assert_eq!(format_query_ty(&binding), "Result<Int>");
        assert!(parse_command_query("Result<Int, Error>").is_err());
    }
}
