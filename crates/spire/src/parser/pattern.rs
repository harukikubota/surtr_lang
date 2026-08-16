use crate::ast::*;
use crate::error::ParseError;
use crate::token::Token;

use super::Parser;

impl Parser<'_> {
    pub(super) fn parse_pattern_bind_stmt(&mut self) -> Result<Ast, ParseError> {
        let pat = self.parse_bind_pattern()?;
        let assign_tok = self.peek().clone();
        if !matches!(assign_tok, Token::Bind | Token::SafeBind) {
            return Err(ParseError::syntax(
                "Pattern destructuring requires assignment operator (`=` or `=?`)",
                self.peek_span(),
            ));
        }
        self.advance();
        let rhs = self.parse_expr()?;
        self.ensure_non_associative_assignment(&rhs)?;
        let span = Span {
            start: super::pattern_span(&pat).start,
            end: rhs.span().end,
        };
        if matches!(assign_tok, Token::Bind) && pattern_contains_pin(&pat) {
            return Err(ParseError::syntax(
                "Pinned patterns are not allowed with =. Use =? or match for value checks.",
                span,
            ));
        }
        Self::assignment_ast(assign_tok, span, pat, rhs)
    }

    pub(super) fn is_pattern_bind_stmt_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::LBrack
                | Token::LParen
                | Token::Unit
                | Token::Ident(_)
                | Token::Caret
                | Token::Int(_)
                | Token::Str(_)
                | Token::True
                | Token::False
                | Token::Minus
        )
    }

    fn parse_list_bind_pattern(&mut self) -> Result<AstPattern, ParseError> {
        let sp = self.peek_span();
        self.with_parse_nesting(sp.clone(), |parser| {
            parser.expect(&Token::LBrack)?;
            parser.skip_newlines();
            if matches!(parser.peek(), Token::RBrack) {
                let end = parser.expect(&Token::RBrack)?;
                return Ok(AstPattern::ListNil(Span {
                    start: sp.start,
                    end: end.end,
                }));
            }

            let first = parser.parse_bind_pattern()?;
            parser.skip_newlines();
            let end = if matches!(parser.peek(), Token::Comma) {
                parser.advance();
                parser.skip_newlines();
                if matches!(parser.peek(), Token::DotDot) {
                    parser.advance();
                    parser.skip_newlines();
                    let tail = parser.parse_bind_pattern()?;
                    parser.skip_newlines();
                    let end = parser.expect(&Token::RBrack)?;
                    return Ok(AstPattern::ListCons(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        Box::new(first),
                        Box::new(tail),
                    ));
                }

                let mut items = vec![first];
                if matches!(parser.peek(), Token::RBrack) {
                    let end = parser.expect(&Token::RBrack)?;
                    return Ok(super::fixed_bind_list_pattern(sp.start, end.end, items));
                }
                items.push(parser.parse_bind_pattern()?);
                while matches!(parser.peek(), Token::Comma) {
                    parser.advance();
                    parser.skip_newlines();
                    if matches!(parser.peek(), Token::RBrack) {
                        break;
                    }
                    items.push(parser.parse_bind_pattern()?);
                }
                parser.skip_newlines();
                let end = parser.expect(&Token::RBrack)?;
                return Ok(super::fixed_bind_list_pattern(sp.start, end.end, items));
            } else {
                parser.expect(&Token::RBrack)?
            };

            Ok(super::fixed_bind_list_pattern(
                sp.start,
                end.end,
                vec![first],
            ))
        })
    }

    fn parse_bind_pattern(&mut self) -> Result<AstPattern, ParseError> {
        let mut alts = vec![self.parse_bind_pattern_atom()?];
        loop {
            self.skip_newlines();
            if !matches!(self.peek(), Token::Pipe) {
                break;
            }
            self.advance();
            self.skip_newlines();
            alts.push(self.parse_bind_pattern_atom()?);
        }
        let mut pat = if let [single] = alts.as_slice() {
            single.clone()
        } else {
            let start = alts
                .first()
                .map(|pat| super::pattern_span(pat).start)
                .unwrap_or_else(|| self.peek_span().start);
            let end = alts
                .last()
                .map(|pat| super::pattern_span(pat).end)
                .unwrap_or(start);
            AstPattern::Or(Span { start, end }, alts)
        };
        loop {
            self.skip_newlines();
            if !matches!(self.peek(), Token::At) {
                break;
            }
            self.advance(); // '@'
            self.skip_newlines();
            if super::pattern_depth(&pat) >= super::MAX_PARSE_NESTING {
                return Err(ParseError::syntax(
                    super::MAX_PARSE_NESTING_MESSAGE,
                    super::pattern_span(&pat).clone(),
                ));
            }
            let (alias, alias_span) = self.expect_ident()?;
            if alias == "self" && self.impl_target_stack.is_empty() {
                return Err(ParseError::syntax(
                    "`self` can only be used inside impl methods",
                    alias_span,
                ));
            }
            self.ensure_non_const_identifier(&alias, alias_span.clone(), "Pattern alias")?;
            self.skip_newlines();
            let alias_ty = if matches!(self.peek(), Token::Colon) {
                self.advance();
                self.skip_newlines();
                Some(self.parse_type()?)
            } else {
                None
            };
            let end = alias_ty
                .as_ref()
                .map(|ty| super::ast_ty_span(ty).end)
                .unwrap_or(alias_span.end);
            let span = Span {
                start: super::pattern_span(&pat).start,
                end,
            };
            pat = AstPattern::As(span, Box::new(pat), alias, alias_ty);
        }
        Ok(pat)
    }

    fn parse_bind_pattern_atom(&mut self) -> Result<AstPattern, ParseError> {
        let sp = self.peek_span();
        match self.peek().clone() {
            Token::Ident(name) if name == "_" => {
                self.advance();
                Ok(AstPattern::Wildcard(sp))
            }
            Token::Caret => {
                self.advance();
                let (name, name_span) = self.expect_ident()?;
                if name == "self" && self.impl_target_stack.is_empty() {
                    return Err(ParseError::syntax(
                        "`self` can only be used inside impl methods",
                        name_span,
                    ));
                }
                Ok(AstPattern::Pin(
                    Span {
                        start: sp.start,
                        end: name_span.end,
                    },
                    name,
                ))
            }
            Token::Int(n) => {
                self.advance();
                if self.is_duration_suffix_here() {
                    let suffix_span = self.advance().span.clone();
                    return Ok(AstPattern::DurationLit(
                        Span {
                            start: sp.start,
                            end: suffix_span.end,
                        },
                        n,
                    ));
                }
                Ok(AstPattern::IntLit(sp, n))
            }
            Token::Minus => {
                self.advance();
                let neg_span = self.peek_span();
                match self.peek().clone() {
                    Token::Int(n) => {
                        self.advance();
                        Ok(AstPattern::IntLit(
                            Span {
                                start: sp.start,
                                end: neg_span.end,
                            },
                            -n,
                        ))
                    }
                    Token::Eof => Err(ParseError::incomplete("integer literal", neg_span)),
                    _ => Err(ParseError::syntax(
                        "Expected integer literal after '-' in pattern",
                        neg_span,
                    )),
                }
            }
            Token::Str(s) => {
                self.advance();
                Ok(AstPattern::StrLit(sp, s))
            }
            Token::True => {
                self.advance();
                Ok(AstPattern::BoolLit(sp, true))
            }
            Token::False => {
                self.advance();
                Ok(AstPattern::BoolLit(sp, false))
            }
            Token::Ident(name) => {
                if name == "self" && self.impl_target_stack.is_empty() {
                    return Err(ParseError::syntax(
                        "`self` can only be used inside impl methods",
                        sp,
                    ));
                }
                self.advance();
                let mut segments = vec![name.clone()];
                let mut path_end = sp.end;
                while self.has_path_separator()
                    && matches!(
                        self.peek_n(2),
                        Some(Token::Ident(_) | Token::True | Token::False)
                    )
                {
                    self.consume_path_separator()?;
                    let (seg, seg_span) = match self.peek().clone() {
                        Token::Ident(_) => self.expect_ident()?,
                        Token::True => {
                            let span = self.advance().span;
                            ("True".into(), span)
                        }
                        Token::False => {
                            let span = self.advance().span;
                            ("False".into(), span)
                        }
                        _ => unreachable!("path segment token was checked before consuming `::`"),
                    };
                    path_end = seg_span.end;
                    segments.push(seg);
                }

                let callee_name = segments.join("::");
                if matches!(self.peek(), Token::LParen) {
                    return self.with_parse_nesting(sp.clone(), |parser| {
                        parser.advance();
                        parser.skip_newlines();
                        let mut inners = Vec::new();
                        if !matches!(parser.peek(), Token::RParen) {
                            inners.push(parser.parse_bind_pattern()?);
                            parser.skip_newlines();
                            while matches!(parser.peek(), Token::Comma) {
                                parser.advance();
                                parser.skip_newlines();
                                if matches!(parser.peek(), Token::RParen) {
                                    break;
                                }
                                inners.push(parser.parse_bind_pattern()?);
                                parser.skip_newlines();
                            }
                        }
                        let end = parser.expect(&Token::RParen)?;
                        Ok(AstPattern::Call(
                            Span {
                                start: sp.start,
                                end: end.end,
                            },
                            callee_name,
                            inners,
                        ))
                    });
                }

                let is_ctor = segments
                    .last()
                    .and_then(|segment| segment.chars().next())
                    .map(|ch| ch.is_uppercase())
                    .unwrap_or(false);
                if is_ctor {
                    let ctor_name = callee_name;
                    if matches!(self.peek(), Token::Unit) {
                        let end = self.advance().span.clone();
                        return Ok(AstPattern::Constructor(
                            Span {
                                start: sp.start,
                                end: end.end,
                            },
                            ctor_name,
                            Vec::new(),
                        ));
                    }
                    return Ok(AstPattern::Constructor(
                        Span {
                            start: sp.start,
                            end: path_end,
                        },
                        ctor_name,
                        Vec::new(),
                    ));
                }

                if segments.len() > 1 {
                    return Err(ParseError::syntax(
                        "Qualified patterns support constructor forms only",
                        Span {
                            start: sp.start,
                            end: path_end,
                        },
                    ));
                }

                self.ensure_non_const_identifier(&name, sp.clone(), "Pattern binding")?;
                Ok(AstPattern::Var(sp, name))
            }
            Token::LBrack => self.parse_list_bind_pattern(),
            Token::Unit => Err(ParseError::syntax(
                "The Unit type has no pattern matching.",
                sp,
            )),
            Token::LParen => {
                self.with_parse_nesting(sp.clone(), |parser| {
                    parser.advance();
                    parser.skip_newlines();
                    let first = parser.parse_bind_pattern()?;
                    parser.skip_newlines();
                    if matches!(parser.peek(), Token::Comma) {
                        parser.advance();
                        parser.skip_newlines();
                        if matches!(parser.peek(), Token::RParen) {
                            return Err(ParseError::syntax(
                                "1-tuple patterns are not supported",
                                Span {
                                    start: sp.start,
                                    end: parser.peek_span().end,
                                },
                            ));
                        }
                        let mut items = vec![first, parser.parse_bind_pattern()?];
                        parser.skip_newlines();
                        while matches!(parser.peek(), Token::Comma) {
                            parser.advance();
                            parser.skip_newlines();
                            if matches!(parser.peek(), Token::RParen) {
                                break;
                            }
                            items.push(parser.parse_bind_pattern()?);
                            parser.skip_newlines();
                        }
                        let end = parser.expect(&Token::RParen)?;
                        Ok(AstPattern::Tuple(
                            Span {
                                start: sp.start,
                                end: end.end,
                            },
                            items,
                        ))
                    } else {
                        parser.expect(&Token::RParen)?;
                        Ok(first)
                    }
                })
            }
            Token::Eof => Err(ParseError::incomplete("list pattern", sp)),
            _ => Err(ParseError::syntax(
                "Pattern supports identifiers, literals, `_`, list patterns, nested `Ok(...)` patterns, and `pattern @ alias`",
                sp,
            )),
        }
    }

    /// Match pattern now reuses the same grammar as bind/safe-bind patterns.
    pub(super) fn parse_match_pattern(&mut self) -> Result<AstPattern, ParseError> {
        self.parse_bind_pattern()
    }
}

pub(super) fn pattern_contains_pin(pattern: &AstPattern) -> bool {
    match pattern {
        AstPattern::Pin(_, _) => true,
        AstPattern::As(_, inner, _, _) => pattern_contains_pin(inner),
        AstPattern::ListCons(_, head, tail) => {
            pattern_contains_pin(head) || pattern_contains_pin(tail)
        }
        AstPattern::Constructor(_, _, items)
        | AstPattern::Call(_, _, items)
        | AstPattern::Tuple(_, items)
        | AstPattern::Or(_, items) => items.iter().any(pattern_contains_pin),
        AstPattern::Var(_, _)
        | AstPattern::Annotated(_, _, _)
        | AstPattern::Wildcard(_)
        | AstPattern::ListNil(_)
        | AstPattern::IntLit(_, _)
        | AstPattern::StrLit(_, _)
        | AstPattern::BoolLit(_, _)
        | AstPattern::DurationLit(_, _) => false,
    }
}

pub(super) fn pattern_contains_binding_var(pattern: &AstPattern) -> bool {
    match pattern {
        AstPattern::Var(_, _) | AstPattern::Annotated(_, _, _) | AstPattern::As(_, _, _, _) => true,
        AstPattern::ListCons(_, head, tail) => {
            pattern_contains_binding_var(head) || pattern_contains_binding_var(tail)
        }
        AstPattern::Constructor(_, _, items)
        | AstPattern::Call(_, _, items)
        | AstPattern::Tuple(_, items)
        | AstPattern::Or(_, items) => items.iter().any(pattern_contains_binding_var),
        AstPattern::Wildcard(_)
        | AstPattern::Pin(_, _)
        | AstPattern::ListNil(_)
        | AstPattern::IntLit(_, _)
        | AstPattern::StrLit(_, _)
        | AstPattern::BoolLit(_, _)
        | AstPattern::DurationLit(_, _) => false,
    }
}
