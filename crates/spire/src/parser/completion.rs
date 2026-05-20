use crate::ast::Span;
use crate::error::ParseError;
use crate::lexer::tokenize;
use crate::token::{Spanned, Token};

use super::chumsky_program;
use super::context::DeclLevel;
use super::{Parser, ParserContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionContext {
    ExprContext,
    TypeContext,
    DeclContext,
    ImportPath,
    CallArgName,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IncompleteParseResult {
    pub expected_tokens: Vec<String>,
    pub cursor_span: Span,
    pub context: CompletionContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorCompletionContext {
    pub stages: Vec<OperatorCompletionStage>,
    pub active_stage: usize,
    pub cursor_span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorCompletionStage {
    pub lhs: Span,
    pub operator: String,
    pub operator_span: Span,
    pub rhs: Option<Span>,
}

pub fn parse_incomplete_stmt(
    source: &str,
    context: ParserContext,
) -> Result<IncompleteParseResult, ParseError> {
    let tokens = tokenize(source)?;
    match chumsky_program::parse_program_with_chumsky_diagnostic(source, &tokens, context) {
        Ok(_) => Err(ParseError::syntax(
            "input is already complete",
            completion_span(&tokens),
        )),
        Err(diag) => {
            if !diag.error.is_incomplete() {
                if let Some(expected_tokens) = promote_syntax_to_incomplete(&diag.error, source) {
                    let cursor = source.len();
                    let cursor_span = Span {
                        start: cursor,
                        end: cursor,
                    };
                    return Ok(IncompleteParseResult {
                        context: infer_completion_context(source, cursor, &expected_tokens),
                        expected_tokens,
                        cursor_span,
                    });
                }
                return Err(diag.error);
            }

            let mut expected_tokens = diag.expected_tokens;
            if expected_tokens.is_empty() {
                expected_tokens = expected_from_error(&diag.error);
            }

            Ok(IncompleteParseResult {
                context: infer_completion_context(source, diag.cursor_span.start, &expected_tokens),
                expected_tokens,
                cursor_span: diag.cursor_span,
            })
        }
    }
}

pub fn parse_incomplete_expr(
    source: &str,
    mut context: ParserContext,
) -> Result<IncompleteParseResult, ParseError> {
    let tokens = tokenize(source)?;
    context.level = DeclLevel::Expr;

    let mut parser = Parser::new(source, &tokens, context);
    match parser.parse_expr() {
        Ok(_) => Err(ParseError::syntax(
            "input is already complete",
            parser.peek_span(),
        )),
        Err(error) => {
            if !error.is_incomplete() {
                return Err(error);
            }
            let expected_tokens = expected_from_error(&error);
            let cursor_span = error.span().clone();
            Ok(IncompleteParseResult {
                context: infer_completion_context(source, cursor_span.start, &expected_tokens),
                expected_tokens,
                cursor_span,
            })
        }
    }
}

pub fn parse_operator_completion_context(
    source: &str,
    cursor: usize,
) -> Option<OperatorCompletionContext> {
    let cursor = clamp_to_char_boundary(source, cursor.min(source.len()));
    let before = &source[..cursor];
    let operators = top_level_operators(before)?;
    let active_stage = operators.len().checked_sub(1)?;
    let mut stages = Vec::with_capacity(operators.len());

    for (idx, operator) in operators.iter().enumerate() {
        let next_operator_start = operators
            .get(idx + 1)
            .map(|next| next.start)
            .unwrap_or(cursor);
        let lhs = trim_byte_range_to_span(source, 0, operator.start)?;
        let rhs = if idx == active_stage {
            None
        } else {
            trim_byte_range_to_span(source, operator.end, next_operator_start)
        };
        stages.push(OperatorCompletionStage {
            lhs,
            operator: operator.symbol.to_string(),
            operator_span: byte_range_to_span(source, operator.start, operator.end),
            rhs,
        });
    }

    Some(OperatorCompletionContext {
        stages,
        active_stage,
        cursor_span: byte_range_to_span(source, cursor, cursor),
    })
}

fn completion_span(tokens: &[Spanned<Token>]) -> Span {
    tokens
        .last()
        .map(|token| token.span.clone())
        .unwrap_or(Span { start: 0, end: 0 })
}

fn expected_from_error(error: &ParseError) -> Vec<String> {
    match error {
        ParseError::Incomplete { expected, .. } => vec![expected.clone()],
        ParseError::SyntaxError { .. } => Vec::new(),
    }
}

fn promote_syntax_to_incomplete(error: &ParseError, source: &str) -> Option<Vec<String>> {
    let ParseError::SyntaxError { message, .. } = error else {
        return None;
    };
    let trimmed = source.trim_end();

    if trimmed.ends_with("::")
        || message.contains("Expected identifier or `{` after `::` in import")
    {
        return Some(vec!["identifier".into(), "{".into()]);
    }

    None
}

fn infer_completion_context(
    source: &str,
    cursor_byte: usize,
    expected_tokens: &[String],
) -> CompletionContext {
    let clamped = cursor_byte.min(source.len());
    let prefix = &source[..clamped];
    let trimmed = prefix.trim_end();
    let line = trimmed.rsplit('\n').next().unwrap_or("").trim_start();

    if trimmed.ends_with("::") || line.starts_with("import ") {
        return CompletionContext::ImportPath;
    }

    if line.starts_with("def ")
        || line.starts_with("impl ")
        || line.starts_with("deftrait ")
        || line.starts_with("@@")
    {
        return CompletionContext::DeclContext;
    }

    if looks_like_call_arg_site(trimmed) {
        return CompletionContext::CallArgName;
    }

    let expects_type = expected_tokens.iter().any(|expected| {
        expected.contains("Gt")
            || expected.contains("where")
            || expected.contains("impl Trait")
            || expected.contains("type")
    });
    let has_type_intro = line.contains(':') && !line.contains('=');
    if expects_type || has_type_intro {
        return CompletionContext::TypeContext;
    }

    if trimmed.is_empty() {
        CompletionContext::Unknown
    } else {
        CompletionContext::ExprContext
    }
}

fn looks_like_call_arg_site(prefix: &str) -> bool {
    if prefix.ends_with('(') || prefix.ends_with(',') {
        return true;
    }

    let open_paren = prefix.rfind('(');
    let close_paren = prefix.rfind(')');
    match (open_paren, close_paren) {
        (Some(open), Some(close)) => open > close,
        (Some(_), None) => true,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy)]
struct TopLevelOperator<'a> {
    symbol: &'a str,
    start: usize,
    end: usize,
}

const COMPLETION_OPERATORS: &[&str] = &[
    "|>=", "|*>", ">=>", ">>", ">*", "|>", "++", "==", "!=", "<=", ">=", "&&", "||", "+", "-", "*",
    "/", "<", ">",
];

fn top_level_operators(input: &str) -> Option<Vec<TopLevelOperator<'_>>> {
    let mut out = Vec::new();
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut iter = input.char_indices().peekable();

    while let Some((idx, ch)) = iter.next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                continue;
            }
            '(' => {
                paren_depth += 1;
                continue;
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                continue;
            }
            '[' => {
                bracket_depth += 1;
                continue;
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                continue;
            }
            '{' => {
                brace_depth += 1;
                continue;
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }

        if paren_depth != 0 || bracket_depth != 0 || brace_depth != 0 {
            continue;
        }

        if let Some(symbol) = COMPLETION_OPERATORS
            .iter()
            .find(|symbol| input[idx..].starts_with(**symbol))
        {
            out.push(TopLevelOperator {
                symbol,
                start: idx,
                end: idx + symbol.len(),
            });
            for _ in 1..symbol.chars().count() {
                iter.next();
            }
        }
    }

    (!in_string && !out.is_empty()).then_some(out)
}

fn clamp_to_char_boundary(input: &str, mut cursor: usize) -> usize {
    cursor = cursor.min(input.len());
    while cursor > 0 && !input.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn trim_byte_range_to_span(source: &str, start: usize, end: usize) -> Option<Span> {
    let trimmed = source[start..end].trim();
    if trimmed.is_empty() {
        return None;
    }
    let leading = source[start..end].len() - source[start..end].trim_start().len();
    let trailing = source[start..end].trim_end().len();
    Some(byte_range_to_span(
        source,
        start + leading,
        start + trailing,
    ))
}

fn byte_range_to_span(source: &str, start: usize, end: usize) -> Span {
    Span {
        start: source[..start].chars().count(),
        end: source[..end].chars().count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_completion_context_detects_import_path() {
        let context = infer_completion_context("import Kernel::", 15, &[]);
        assert_eq!(context, CompletionContext::ImportPath);
    }

    #[test]
    fn infer_completion_context_detects_call_arg_site() {
        let context = infer_completion_context("print(", 6, &[]);
        assert_eq!(context, CompletionContext::CallArgName);
    }

    #[test]
    fn parse_incomplete_stmt_reports_expected_token_and_context() {
        let result =
            parse_incomplete_stmt("import Kernel::", ParserContext::repl(1)).expect("incomplete");
        assert_eq!(result.context, CompletionContext::ImportPath);
        assert!(!result.expected_tokens.is_empty());
    }

    #[test]
    fn parse_incomplete_expr_reports_call_argument_context() {
        let result = parse_incomplete_expr("print(", ParserContext::repl(1)).expect("incomplete");
        assert_eq!(result.context, CompletionContext::CallArgName);
        assert!(!result.expected_tokens.is_empty());
    }

    #[test]
    fn operator_completion_context_detects_empty_rhs() {
        let context = parse_operator_completion_context("1 + ", 4).expect("operator context");

        assert_eq!(context.active_stage, 0);
        assert_eq!(context.stages.len(), 1);
        assert_eq!(context.stages[0].lhs, Span { start: 0, end: 1 });
        assert_eq!(context.stages[0].operator, "+");
        assert_eq!(context.stages[0].operator_span, Span { start: 2, end: 3 });
        assert_eq!(context.stages[0].rhs, None);
        assert_eq!(context.cursor_span, Span { start: 4, end: 4 });
    }

    #[test]
    fn operator_completion_context_treats_rhs_prefix_as_active() {
        let context = parse_operator_completion_context("1 + ans", 7).expect("operator context");

        assert_eq!(context.active_stage, 0);
        assert_eq!(context.stages.len(), 1);
        assert_eq!(context.stages[0].lhs, Span { start: 0, end: 1 });
        assert_eq!(context.stages[0].operator, "+");
        assert_eq!(context.stages[0].rhs, None);
        assert_eq!(context.cursor_span, Span { start: 7, end: 7 });
    }

    #[test]
    fn operator_completion_context_tracks_function_operator_chain() {
        let context =
            parse_operator_completion_context("x |> f |> ", 10).expect("operator context");

        assert_eq!(context.active_stage, 1);
        assert_eq!(context.stages.len(), 2);
        assert_eq!(context.stages[0].operator, "|>");
        assert_eq!(context.stages[0].lhs, Span { start: 0, end: 1 });
        assert_eq!(context.stages[0].rhs, Some(Span { start: 5, end: 6 }));
        assert_eq!(context.stages[1].operator, "|>");
        assert_eq!(context.stages[1].lhs, Span { start: 0, end: 6 });
        assert_eq!(context.stages[1].rhs, None);
    }

    #[test]
    fn operator_completion_context_ignores_nested_operators_and_strings() {
        let context = parse_operator_completion_context("wrap(1 + 2) |> ", 15)
            .expect("top-level operator context");

        assert_eq!(context.stages.len(), 1);
        assert_eq!(context.stages[0].operator, "|>");
        assert_eq!(context.stages[0].lhs, Span { start: 0, end: 11 });

        assert!(parse_operator_completion_context("\"1 + \"", 6).is_none());
    }

    #[test]
    fn operator_completion_context_rejects_non_operator_prefix() {
        assert!(parse_operator_completion_context("print(", 6).is_none());
        assert!(parse_operator_completion_context("name", 4).is_none());
    }
}
