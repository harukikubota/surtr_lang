use crate::ast::*;
use crate::error::ParseError;
use crate::token::Token;

use super::Parser;

impl Parser<'_> {
    // ── Type annotation parsing ──

    pub(super) fn parse_type(&mut self) -> Result<AstTy, ParseError> {
        self.parse_type_in_impl_context(self.impl_target_stack.last().cloned())
    }

    pub(super) fn parse_type_in_impl_context(
        &mut self,
        impl_target: Option<String>,
    ) -> Result<AstTy, ParseError> {
        self.skip_newlines();
        let sp = self.peek_span();

        if matches!(self.peek(), Token::LParen) {
            return self.with_parse_nesting(sp.clone(), |parser| {
                parser.advance();
                parser.skip_newlines();
                if matches!(parser.peek(), Token::Arrow) {
                    parser.advance();
                    let ret = parser.parse_type_in_impl_context(impl_target.clone())?;
                    parser.skip_newlines();
                    let end = parser.expect(&Token::RParen)?;
                    return Ok(AstTy::Func(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        Vec::new(),
                        Box::new(ret),
                    ));
                }

                let mut params = Vec::new();
                params.push(parser.parse_type_in_impl_context(impl_target.clone())?);
                parser.skip_newlines();
                while matches!(parser.peek(), Token::Comma) {
                    parser.advance();
                    parser.skip_newlines();
                    if matches!(parser.peek(), Token::RParen) {
                        return Err(ParseError::syntax(
                            "1-tuple types are not supported",
                            Span {
                                start: sp.start,
                                end: parser.peek_span().end,
                            },
                        ));
                    }
                    params.push(parser.parse_type_in_impl_context(impl_target.clone())?);
                    parser.skip_newlines();
                }
                if matches!(parser.peek(), Token::Arrow) {
                    parser.advance();
                    let ret = parser.parse_type_in_impl_context(impl_target.clone())?;
                    parser.skip_newlines();
                    let end = parser.expect(&Token::RParen)?;
                    return Ok(AstTy::Func(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        params,
                        Box::new(ret),
                    ));
                }

                let end = parser.expect(&Token::RParen)?;
                if params.len() == 1 {
                    parser.skip_newlines();
                    let message = if matches!(parser.peek(), Token::Arrow) {
                        "Parenthesized type signatures must choose tuple or function syntax after the first element: use `,` and another type for a tuple, or put `->` before `)` for a function type (for example, `(Int -> String)`, not `(Int) -> String`)."
                    } else {
                        "Parenthesized type annotations with one element are not supported: use the type without parentheses, `(T, U)` for a tuple, or `(T -> R)` for a function type."
                    };
                    return Err(ParseError::syntax(
                        message,
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                    ));
                }
                Ok(AstTy::Tuple(
                    Span {
                        start: sp.start,
                        end: end.end,
                    },
                    params,
                ))
            });
        }

        if matches!(self.peek(), Token::Dollar) {
            self.advance();
            let (name, end) = self.expect_ident()?;
            let name = format!("${}", name);
            if name == "$Self" {
                return Err(ParseError::syntax("Invalid type variable name: $Self", sp));
            }
            return Ok(AstTy::Named(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                name,
            ));
        }

        if matches!(self.peek(), Token::Unit) {
            let end = self.advance().span.clone();
            return Ok(AstTy::Named(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                "Unit".to_string(),
            ));
        }

        if matches!(self.peek(), Token::Impl) {
            self.advance();
            self.skip_newlines();
        let (trait_name, trait_span) = self.expect_qualified_ident(2, "trait")?;
        return Ok(AstTy::ImplTrait(
            Span {
                start: sp.start,
                    end: trait_span.end,
                },
                trait_name,
            ));
        }

        // Named type, possibly with type args: Result<Int>, List<Int>, Option<Int>, ...
        let (name, name_span) = self.expect_qualified_ident(2, "type")?;
        if name == "Self" {
            if impl_target.is_some() {
                return Ok(AstTy::Named(
                    Span {
                        start: sp.start,
                        end: name_span.end,
                    },
                    "Self".to_string(),
                ));
            }
            return Err(ParseError::syntax(
                "`Self` can only be used inside impl methods",
                sp,
            ));
        }
        if name == "self" {
            return Err(ParseError::syntax("`self` is not a type name", sp));
        }

        // Check for type parameters: Name<T> or Name<T, E>
        if matches!(self.peek(), Token::Lt) {
            return self.with_parse_nesting(sp.clone(), |parser| {
                parser.advance();
                parser.skip_newlines();
                let mut args = vec![parser.parse_type_in_impl_context(impl_target.clone())?];
                parser.skip_newlines();
                while matches!(parser.peek(), Token::Comma) {
                    parser.advance();
                    parser.skip_newlines();
                    args.push(parser.parse_type_in_impl_context(impl_target.clone())?);
                    parser.skip_newlines();
                }
                let end = parser.expect_type_gt()?;
                Ok(AstTy::Generic(
                    Span {
                        start: sp.start,
                        end: end.end,
                    },
                    name,
                    args,
                ))
            });
        }

        Ok(AstTy::Named(
            Span {
                start: sp.start,
                end: name_span.end,
            },
            name,
        ))
    }

    pub(super) fn is_self_type(ty: &AstTy) -> bool {
        matches!(ty, AstTy::Named(_, name) if name == "Self")
    }
}
