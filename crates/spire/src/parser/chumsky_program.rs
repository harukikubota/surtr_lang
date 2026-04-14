use chumsky::error::{Rich, RichReason};
use chumsky::prelude::*;
use chumsky::Parser as ChumskyParser;

use crate::ast::{Ast, Span};
use crate::error::ParseError;
use crate::token::{Spanned, Token};

use super::{Parser, ParserContext};

type ProgramExtra<'src> = extra::Err<Rich<'src, Spanned<Token>>>;

pub(super) fn parse_program_with_chumsky(
    tokens: &[Spanned<Token>],
    context: ParserContext,
) -> Result<Vec<Ast>, ParseError> {
    program_parser(context)
        .parse(tokens)
        .into_result()
        .map_err(|errs| map_chumsky_error(tokens, errs))
}

fn program_parser<'src>(
    context: ParserContext,
) -> impl ChumskyParser<'src, &'src [Spanned<Token>], Vec<Ast>, ProgramExtra<'src>> {
    custom(move |inp| {
        let mut stmts = Vec::new();

        loop {
            let before = inp.cursor();
            let remaining: &[Spanned<Token>] = inp.slice_from(&before..);

            if remaining.is_empty() {
                return Err(Rich::custom(
                    inp.span_since(&before),
                    "unexpected end of input",
                ));
            }

            match &remaining[0].token {
                Token::Newline => {
                    let _ = inp.next_ref();
                }
                Token::Eof => {
                    let _ = inp.next_ref();
                    break;
                }
                _ => {
                    let (stmt, consumed) = parse_stmt_prefix(remaining, context.clone())
                        .map_err(|err| Rich::custom(inp.span_since(&before), err.message()))?;

                    for _ in 0..consumed {
                        let _ = inp.next_ref();
                    }

                    while {
                        let tail: &[Spanned<Token>] = inp.slice_from(&inp.cursor()..);
                        matches!(tail.first().map(|tok| &tok.token), Some(Token::Newline))
                    } {
                        let _ = inp.next_ref();
                    }

                    stmts.push(stmt);
                }
            }
        }

        Ok(stmts)
    })
}

fn parse_stmt_prefix(
    tokens: &[Spanned<Token>],
    context: ParserContext,
) -> Result<(Ast, usize), ParseError> {
    let mut parser = Parser::new(tokens.to_vec(), context);
    let stmt = parser.parse_stmt()?;
    parser.ensure_stmt_boundary(&stmt, false)?;
    let synthetic_tokens = parser.tokens.len().saturating_sub(tokens.len());
    Ok((stmt, parser.pos.saturating_sub(synthetic_tokens)))
}

fn map_chumsky_error(
    tokens: &[Spanned<Token>],
    mut errs: Vec<Rich<'_, Spanned<Token>>>,
) -> ParseError {
    let Some(err) = errs.pop() else {
        return ParseError::syntax("unknown parse error", fallback_span(tokens));
    };

    let span = map_input_span(tokens, err.span());
    let message = match err.reason() {
        RichReason::Custom(msg) => msg.clone(),
        RichReason::ExpectedFound { .. } => "unexpected token while parsing program".to_string(),
    };

    if let Some(expected) = message.strip_prefix("Incomplete input: expected ") {
        ParseError::incomplete(expected.to_string(), span)
    } else if message.contains("end of input") || message.contains("unexpected end of input") {
        ParseError::incomplete("input", span)
    } else {
        ParseError::syntax(message, span)
    }
}

fn map_input_span(tokens: &[Spanned<Token>], span: &SimpleSpan<usize>) -> Span {
    if tokens.is_empty() {
        return Span { start: 0, end: 0 };
    }

    let start_idx = span.start.min(tokens.len().saturating_sub(1));
    let end_idx = span
        .end
        .saturating_sub(1)
        .min(tokens.len().saturating_sub(1));

    let start = tokens[start_idx].span.start;
    let end = if span.end == 0 {
        tokens[start_idx].span.end
    } else {
        tokens[end_idx].span.end
    };

    Span { start, end }
}

fn fallback_span(tokens: &[Spanned<Token>]) -> Span {
    tokens
        .last()
        .map(|tok| tok.span.clone())
        .unwrap_or(Span { start: 0, end: 0 })
}
