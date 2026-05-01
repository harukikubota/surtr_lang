use chumsky::error::Rich;
use chumsky::prelude::*;
use chumsky::span::SimpleSpan;
use chumsky::Parser as ChumskyParser;

use crate::ast::Ast;
use crate::error::ParseError;
use crate::token::{Spanned, Token};

use super::error_map::{self, ParseErrorDiagnostic};
use super::{Parser, ParserContext};

type ProgramExtra<'src> = extra::Err<Rich<'src, Spanned<Token>>>;

pub(super) fn parse_program_with_chumsky(
    source: &str,
    tokens: &[Spanned<Token>],
    context: ParserContext,
) -> Result<Vec<Ast>, ParseError> {
    parse_program_with_chumsky_diagnostic(source, tokens, context).map_err(|diag| diag.error)
}

pub(super) fn parse_program_with_chumsky_diagnostic(
    source: &str,
    tokens: &[Spanned<Token>],
    context: ParserContext,
) -> Result<Vec<Ast>, ParseErrorDiagnostic> {
    program_parser(source, context)
        .parse(tokens)
        .into_result()
        .map_err(|errs| error_map::map_chumsky_error_with_diagnostic(tokens, errs))
}

fn program_parser<'src>(
    source: &'src str,
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
                    let base_span: SimpleSpan<usize> = inp.span_since(&before);
                    let (stmt, consumed) =
                        parse_stmt_prefix(source, remaining, context.clone()).map_err(|err| {
                            Rich::custom(
                                token_span_for_parse_error(base_span.start, remaining, err.span()),
                                err.message(),
                            )
                        })?;

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
    source: &str,
    tokens: &[Spanned<Token>],
    context: ParserContext,
) -> Result<(Ast, usize), ParseError> {
    let mut parser = Parser::new(source, tokens, context);
    let stmt = parser.parse_stmt()?;
    parser.ensure_stmt_boundary(&stmt, false)?;
    Ok((stmt, parser.pos))
}

fn token_span_for_parse_error(
    base_index: usize,
    tokens: &[Spanned<Token>],
    error_span: &crate::ast::Span,
) -> SimpleSpan<usize> {
    let first = tokens
        .iter()
        .position(|token| char_spans_overlap(&token.span, error_span))
        .or_else(|| {
            tokens
                .iter()
                .position(|token| token.span.start >= error_span.start)
        })
        .unwrap_or(0);
    let last = tokens
        .iter()
        .rposition(|token| char_spans_overlap(&token.span, error_span))
        .unwrap_or(first);

    SimpleSpan {
        start: base_index + first,
        end: base_index + last + 1,
        context: (),
    }
}

fn char_spans_overlap(a: &crate::ast::Span, b: &crate::ast::Span) -> bool {
    let a_end = a.end.max(a.start + 1);
    let b_end = b.end.max(b.start + 1);
    a.start < b_end && b.start < a_end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Span;
    use crate::lexer::tokenize;

    #[test]
    fn unexpected_token_error_uses_offending_token_span() {
        let err = parse_program_with_chumsky(
            "x = )",
            &tokenize("x = )").expect("source should tokenize"),
            ParserContext::default(),
        )
        .expect_err("source should fail to parse");

        assert_eq!(err.span(), &Span { start: 4, end: 5 });
    }
}
