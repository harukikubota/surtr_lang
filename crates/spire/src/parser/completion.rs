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

pub fn parse_incomplete_stmt(
    source: &str,
    context: ParserContext,
) -> Result<IncompleteParseResult, ParseError> {
    let tokens = tokenize(source)?;
    match chumsky_program::parse_program_with_chumsky_diagnostic(&tokens, context) {
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

    let mut parser = Parser::new(&tokens, context);
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
}
