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
        Ok(match assign_tok {
            Token::Bind => Ast::Bind(span, pat, Box::new(rhs)),
            Token::SafeBind => Ast::SafeBind(span, pat, Box::new(rhs)),
            _ => unreachable!("validated assignment token"),
        })
    }

    pub(super) fn is_pattern_bind_stmt_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::LBrack
                | Token::LParen
                | Token::Ident(_)
                | Token::Int(_)
                | Token::Str(_)
                | Token::True
                | Token::False
                | Token::Minus
        )
    }

    fn parse_list_bind_pattern(&mut self) -> Result<AstPattern, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::LBrack)?;
        self.skip_newlines();
        if matches!(self.peek(), Token::RBrack) {
            let end = self.expect(&Token::RBrack)?;
            return Ok(AstPattern::ListNil(Span {
                start: sp.start,
                end: end.end,
            }));
        }

        let first = self.parse_bind_pattern()?;
        self.skip_newlines();
        let end = if matches!(self.peek(), Token::Comma) {
            self.advance();
            self.skip_newlines();
            if matches!(self.peek(), Token::DotDot) {
                self.advance();
                self.skip_newlines();
                let tail = self.parse_bind_pattern()?;
                self.skip_newlines();
                let end = self.expect(&Token::RBrack)?;
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
            items.push(self.parse_bind_pattern()?);
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::RBrack) {
                    break;
                }
                items.push(self.parse_bind_pattern()?);
            }
            self.skip_newlines();
            let end = self.expect(&Token::RBrack)?;
            return Ok(super::fixed_bind_list_pattern(sp.start, end.end, items));
        } else {
            self.expect(&Token::RBrack)?
        };

        Ok(super::fixed_bind_list_pattern(
            sp.start,
            end.end,
            vec![first],
        ))
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
        let mut pat = if alts.len() == 1 {
            alts.pop().expect("one pattern alternative")
        } else {
            let start = super::pattern_span(alts.first().expect("pattern alternative")).start;
            let end = super::pattern_span(alts.last().expect("pattern alternative")).end;
            AstPattern::Or(Span { start, end }, alts)
        };
        loop {
            self.skip_newlines();
            if !matches!(self.peek(), Token::At) {
                break;
            }
            self.advance(); // '@'
            self.skip_newlines();
            let (alias, alias_span) = self.expect_ident()?;
            if alias == "self" && self.impl_target_stack.is_empty() {
                return Err(ParseError::syntax(
                    "`self` can only be used inside impl methods",
                    alias_span,
                ));
            }
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
            Token::Int(n) => {
                self.advance();
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
                while self.has_path_separator() && matches!(self.peek_n(2), Some(Token::Ident(_))) {
                    self.consume_path_separator()?;
                    let (seg, seg_span) = self.expect_ident()?;
                    path_end = seg_span.end;
                    segments.push(seg);
                }

                let callee_name = segments.join("::");
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    self.skip_newlines();
                    let mut inners = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        inners.push(self.parse_bind_pattern()?);
                        self.skip_newlines();
                        while matches!(self.peek(), Token::Comma) {
                            self.advance();
                            self.skip_newlines();
                            if matches!(self.peek(), Token::RParen) {
                                break;
                            }
                            inners.push(self.parse_bind_pattern()?);
                            self.skip_newlines();
                        }
                    }
                    let end = self.expect(&Token::RParen)?;
                    return Ok(AstPattern::Call(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        callee_name,
                        inners,
                    ));
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

                Ok(AstPattern::Var(sp, name))
            }
            Token::LBrack => self.parse_list_bind_pattern(),
            Token::LParen => {
                self.advance();
                self.skip_newlines();
                let first = self.parse_bind_pattern()?;
                self.skip_newlines();
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                    if matches!(self.peek(), Token::RParen) {
                        return Err(ParseError::syntax(
                            "1-tuple patterns are not supported",
                            Span {
                                start: sp.start,
                                end: self.peek_span().end,
                            },
                        ));
                    }
                    let mut items = vec![first, self.parse_bind_pattern()?];
                    self.skip_newlines();
                    while matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                        items.push(self.parse_bind_pattern()?);
                        self.skip_newlines();
                    }
                    let end = self.expect(&Token::RParen)?;
                    Ok(AstPattern::Tuple(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        items,
                    ))
                } else {
                    self.expect(&Token::RParen)?;
                    Ok(first)
                }
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
