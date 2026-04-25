use chumsky::error::Rich;
use chumsky::prelude::*;
use chumsky::Parser as ChumskyParser;

use crate::ast::Ast;
use crate::error::ParseError;
use crate::token::{Spanned, Token};

use super::error_map::{self, ParseErrorDiagnostic};
use super::{Parser, ParserContext};

type ProgramExtra<'src> = extra::Err<Rich<'src, Spanned<Token>>>;

pub(super) fn parse_program_with_chumsky(
    tokens: &[Spanned<Token>],
    context: ParserContext,
) -> Result<Vec<Ast>, ParseError> {
    parse_program_with_chumsky_diagnostic(tokens, context).map_err(|diag| diag.error)
}

pub(super) fn parse_program_with_chumsky_diagnostic(
    tokens: &[Spanned<Token>],
    context: ParserContext,
) -> Result<Vec<Ast>, ParseErrorDiagnostic> {
    program_parser(context)
        .parse(tokens)
        .into_result()
        .map_err(|errs| error_map::map_chumsky_error_with_diagnostic(tokens, errs))
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
    let mut parser = Parser::new(tokens, context);
    let stmt = parser.parse_stmt()?;
    parser.ensure_stmt_boundary(&stmt, false)?;
    Ok((stmt, parser.pos))
}
