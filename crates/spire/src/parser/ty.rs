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
            self.advance();
            self.skip_newlines();
            if matches!(self.peek(), Token::Arrow) {
                self.advance();
                let ret = self.parse_type_in_impl_context(impl_target.clone())?;
                self.skip_newlines();
                let end = self.expect(&Token::RParen)?;
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
            params.push(self.parse_type_in_impl_context(impl_target.clone())?);
            self.skip_newlines();
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::RParen) {
                    return Err(ParseError::syntax(
                        "1-tuple types are not supported",
                        Span {
                            start: sp.start,
                            end: self.peek_span().end,
                        },
                    ));
                }
                params.push(self.parse_type_in_impl_context(impl_target.clone())?);
                self.skip_newlines();
            }
            if matches!(self.peek(), Token::Arrow) {
                self.advance();
                let ret = self.parse_type_in_impl_context(impl_target.clone())?;
                self.skip_newlines();
                let end = self.expect(&Token::RParen)?;
                return Ok(AstTy::Func(
                    Span {
                        start: sp.start,
                        end: end.end,
                    },
                    params,
                    Box::new(ret),
                ));
            }

            let end = self.expect(&Token::RParen)?;
            if params.len() == 1 {
                self.skip_newlines();
                let message = if matches!(self.peek(), Token::Arrow) {
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
            return Ok(AstTy::Tuple(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                params,
            ));
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
            let (trait_name, trait_span) = self.expect_ident()?;
            return Ok(AstTy::ImplTrait(
                Span {
                    start: sp.start,
                    end: trait_span.end,
                },
                trait_name,
            ));
        }

        // Named type, possibly with type args: Result<Int>, List<Int>, Option<Int>, ...
        let (name, _) = self.expect_ident()?;
        if name == "Self" {
            if impl_target.is_some() {
                return Ok(AstTy::Named(
                    Span {
                        start: sp.start,
                        end: sp.end,
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
            self.advance();
            self.skip_newlines();
            let mut args = vec![self.parse_type_in_impl_context(impl_target.clone())?];
            self.skip_newlines();
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                args.push(self.parse_type_in_impl_context(impl_target.clone())?);
                self.skip_newlines();
            }
            let end = self.expect_type_gt()?;
            return Ok(AstTy::Generic(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                name,
                args,
            ));
        }

        Ok(AstTy::Named(
            Span {
                start: sp.start,
                end: sp.end,
            },
            name,
        ))
    }

    pub(super) fn is_self_type(ty: &AstTy) -> bool {
        matches!(ty, AstTy::Named(_, name) if name == "Self")
    }
}
