use crate::ast::*;
use crate::error::ParseError;
use crate::token::Token;

use super::Parser;

impl Parser<'_> {
    pub(super) fn parse_do_pattern_statement(&mut self) -> Result<AstDoStatement, ParseError> {
        let pat = self.parse_pattern()?;
        let operator = self.peek().clone();
        if !matches!(operator, Token::LeftArrow | Token::SafeBind) {
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PatternSyntax,
                "A do pattern statement requires `<-` or `=?`",
                self.peek_span(),
            ));
        }
        self.reject_binding_or(&pat)?;
        let operator_span = self.advance().span;
        let rhs = self.parse_expr()?;
        self.ensure_non_associative_assignment(&rhs)?;
        let span = Span {
            start: super::pattern_span(&pat).start,
            end: rhs.span().end,
        };
        Ok(match operator {
            Token::LeftArrow => AstDoStatement::Extract {
                span,
                operator_span,
                pattern: pat,
                rhs,
            },
            Token::SafeBind => AstDoStatement::SafeBind {
                span,
                operator_span,
                pattern: pat,
                rhs,
            },
            _ => unreachable!("do pattern operator was checked before consuming"),
        })
    }

    pub(super) fn parse_pattern_bind_stmt(&mut self) -> Result<Ast, ParseError> {
        let pat = self.parse_pattern()?;
        let assign_tok = self.peek().clone();
        // Recognize the full LHS before applying the binding restriction.
        // `<-` also commits an OR rejection if do lookahead stopped at a newline.
        if matches!(assign_tok, Token::Bind | Token::SafeBind | Token::LeftArrow) {
            self.reject_binding_or(&pat)?;
        }
        if !matches!(assign_tok, Token::Bind | Token::SafeBind) {
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PatternSyntax,
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
                crate::error::ParseErrorReason::PatternSyntax,
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
                | Token::ReservedCallName(_)
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

            let first = parser.parse_pattern()?;
            parser.skip_newlines();
            let end = if matches!(parser.peek(), Token::Comma) {
                parser.advance();
                parser.skip_newlines();
                if matches!(parser.peek(), Token::DotDot) {
                    parser.advance();
                    parser.skip_newlines();
                    let tail = parser.parse_pattern()?;
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
                items.push(parser.parse_pattern()?);
                while matches!(parser.peek(), Token::Comma) {
                    parser.advance();
                    parser.skip_newlines();
                    if matches!(parser.peek(), Token::RBrack) {
                        break;
                    }
                    items.push(parser.parse_pattern()?);
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

    pub(super) fn parse_pattern_argument(&mut self) -> Result<AstPatternArgument, ParseError> {
        let start = self.peek_span().start;
        let mut named_parser = self.clone();
        let named_pattern = (|| {
            let (name, _) = named_parser.expect_ident()?;
            named_parser.expect(&Token::Colon)?;
            named_parser.skip_newlines();
            let pattern = named_parser.parse_pattern()?;
            named_parser.skip_newlines();
            if !matches!(named_parser.peek(), Token::Comma | Token::RParen) {
                return Err(ParseError::syntax(
                    crate::error::ParseErrorReason::PatternSyntax,
                    "Expected the end of a named Pattern argument",
                    named_parser.peek_span(),
                ));
            }
            Ok((name, Box::new(pattern)))
        })();
        let mut expression_parser = self.clone();
        let expression = expression_parser.parse_expr().and_then(|value| {
            expression_parser.skip_newlines();
            if matches!(expression_parser.peek(), Token::Comma | Token::RParen) {
                Ok(value)
            } else {
                Err(ParseError::syntax(
                    crate::error::ParseErrorReason::ExpressionSyntax,
                    "Expected the end of an application argument",
                    expression_parser.peek_span(),
                ))
            }
        });
        let mut pattern_parser = self.clone();
        let pattern = pattern_parser.parse_pattern().and_then(|value| {
            pattern_parser.skip_newlines();
            if matches!(pattern_parser.peek(), Token::Comma | Token::RParen) {
                Ok(value)
            } else {
                Err(ParseError::syntax(
                    crate::error::ParseErrorReason::PatternSyntax,
                    "Expected the end of a Pattern argument",
                    pattern_parser.peek_span(),
                ))
            }
        });
        if expression.is_err() && pattern.is_err() && named_pattern.is_err() {
            return Err(pattern.unwrap_err());
        }
        if expression.is_ok() && pattern.is_ok() && expression_parser.pos != pattern_parser.pos {
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PatternSyntax,
                "Application argument has inconsistent syntax boundaries",
                self.peek_span(),
            ));
        }
        let next = if pattern.is_ok() {
            pattern_parser
        } else if named_pattern.is_ok() {
            named_parser
        } else {
            expression_parser
        };
        let end = next.tokens[next.pos - 1].span.end;
        *self = next;
        Ok(AstPatternArgument {
            span: Span { start, end },
            expression_error: expression.as_ref().err().cloned(),
            pattern_error: pattern.as_ref().err().cloned(),
            expression: expression.ok().map(Box::new),
            pattern: pattern.ok().map(Box::new),
            named_pattern: named_pattern.ok(),
        })
    }

    pub(super) fn parse_pattern(&mut self) -> Result<AstPattern, ParseError> {
        let mut alts = vec![self.parse_bind_pattern_atom()?];
        loop {
            if !matches!(self.peek(), Token::Pipe) {
                break;
            }
            self.advance();
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
        let compact_alias = matches!(self.peek(), Token::Annotator(_));
        if compact_alias || matches!(self.peek(), Token::At) {
            if super::pattern_depth(&pat) >= super::MAX_PARSE_NESTING {
                return Err(ParseError::syntax(
                    crate::error::ParseErrorReason::PatternSyntax,
                    super::MAX_PARSE_NESTING_MESSAGE,
                    super::pattern_span(&pat).clone(),
                ));
            }
            let (alias, alias_span) = if compact_alias {
                let spanned = self.advance();
                let Token::Annotator(alias) = spanned.token else {
                    unreachable!("compact as-pattern alias was checked before consuming")
                };
                (
                    alias,
                    Span {
                        start: spanned.span.start + 1,
                        end: spanned.span.end,
                    },
                )
            } else {
                self.advance(); // '@'
                self.skip_newlines();
                if let Token::NumberedPlaceholder(digits) = self.peek().clone() {
                    let span = self.advance().span;
                    (format!("_{digits}"), span)
                } else {
                    self.expect_ident()?
                }
            };
            if sindr::names::is_reserved_value_name(&alias) {
                return Err(ParseError::syntax(
                    crate::error::ParseErrorReason::PatternSyntax,
                    if sindr::pattern::PatternConsumer::from_name(&alias).is_some() {
                        "Pattern consumer names cannot be bound or shadowed"
                    } else {
                        "Reserved call names cannot be bound or shadowed"
                    },
                    alias_span,
                ));
            }
            let projection = alias.strip_prefix('_').filter(|digits| {
                !digits.is_empty() && digits.bytes().all(|ch| ch.is_ascii_digit())
            });
            if alias.starts_with('_') && projection.is_none() {
                return Err(ParseError::syntax(
                    crate::error::ParseErrorReason::PatternSyntax,
                    "as-pattern alias must be a binding identifier.",
                    alias_span,
                )
                .with_guidance(crate::error::ParseErrorGuidance::AsPatternAlias));
            }
            if alias == "self" && self.impl_target_stack.is_empty() {
                return Err(ParseError::syntax(
                    crate::error::ParseErrorReason::PatternSyntax,
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
            pat = if let Some(digits) = projection {
                AstPattern::Projection {
                    index: Self::numbered_placeholder_index(digits, alias_span.clone())?,
                    span: alias_span,
                    inner: Box::new(pat),
                    annotation: alias_ty,
                }
            } else {
                AstPattern::As(span, Box::new(pat), alias, alias_ty, alias_span)
            };
        }
        if matches!(self.peek(), Token::At | Token::Annotator(_)) {
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PatternSyntax,
                "A Pattern allows only one as-pattern alias at each level",
                self.peek_span(),
            ));
        }
        Ok(pat)
    }

    fn parse_bind_pattern_atom(&mut self) -> Result<AstPattern, ParseError> {
        let sp = self.peek_span();
        match self.peek().clone() {
            Token::NumberedPlaceholder(digits) => {
                self.advance();
                let index = Self::numbered_placeholder_index(&digits, sp.clone())?;
                let annotation = if matches!(self.peek(), Token::Colon) { self.advance(); self.skip_newlines(); Some(self.parse_type()?) } else { None };
                Ok(AstPattern::Projection { span: sp.clone(), index, inner: Box::new(AstPattern::Wildcard(sp)), annotation })
            }
            Token::Ident(name) if name.starts_with('_') => {
                self.advance();
                if matches!(self.peek(), Token::Colon) {
                    self.advance();
                    return Ok(AstPattern::AnnotatedWildcard(sp, self.parse_type()?));
                }
                Ok(AstPattern::Wildcard(sp))
            }
            Token::Caret => {
                self.advance();
                let (name, name_span) = self.expect_ident()?;
                if name == "self" && self.impl_target_stack.is_empty() {
                    return Err(ParseError::syntax(
                        crate::error::ParseErrorReason::PatternSyntax,
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
                        crate::error::ParseErrorReason::PatternSyntax,
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
            Token::Ident(name) => self.parse_named_pattern_atom(name, sp),
            Token::ReservedCallName(kind) => self.parse_named_pattern_atom(kind.name().to_string(), sp),
            Token::LBrack => self.parse_list_bind_pattern(),
            Token::Unit => Err(ParseError::syntax(
                crate::error::ParseErrorReason::PatternSyntax,
                "The Unit type has no pattern matching.",
                sp,
            )
            .with_guidance(crate::error::ParseErrorGuidance::UnitPattern)),
            Token::LParen => self.with_parse_nesting(sp.clone(), |parser| {
                parser.advance();
                parser.skip_newlines();
                let first = parser.parse_pattern()?;
                parser.skip_newlines();
                if matches!(parser.peek(), Token::Comma) {
                    parser.advance();
                    parser.skip_newlines();
                    if matches!(parser.peek(), Token::RParen) {
                        return Err(ParseError::syntax(
                            crate::error::ParseErrorReason::PatternSyntax,
                            "1-tuple patterns are not supported",
                            Span {
                                start: sp.start,
                                end: parser.peek_span().end,
                            },
                        ));
                    }
                    let mut items = vec![first, parser.parse_pattern()?];
                    parser.skip_newlines();
                    while matches!(parser.peek(), Token::Comma) {
                        parser.advance();
                        parser.skip_newlines();
                        if matches!(parser.peek(), Token::RParen) {
                            break;
                        }
                        items.push(parser.parse_pattern()?);
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
            }),
            Token::Eof => Err(ParseError::incomplete("list pattern", sp)),
            _ => Err(ParseError::syntax(
                crate::error::ParseErrorReason::PatternSyntax,
                "Pattern supports identifiers, literals, `_`, list patterns, nested `Ok(...)` patterns, and `pattern @ alias`",
                sp,
            )),
        }
    }

    pub(super) fn reject_binding_or(&self, pattern: &AstPattern) -> Result<(), ParseError> {
        let Some(or_span) = pattern_or_span(pattern) else {
            return Ok(());
        };
        let pipe = self
            .tokens
            .iter()
            .find(|sp| {
                matches!(sp.token, Token::Pipe)
                    && or_span.start <= sp.span.start
                    && sp.span.end <= or_span.end
            })
            .ok_or_else(|| {
                ParseError::syntax(
                    crate::error::ParseErrorReason::CompilerInvariant,
                    "OR Pattern has no source pipe token",
                    or_span.clone(),
                )
            })?;
        Err(ParseError::syntax(
            crate::error::ParseErrorReason::PatternSyntax,
            "OR patterns are not allowed in binding patterns",
            pipe.span.clone(),
        ))
    }
    fn parse_named_pattern_atom(
        &mut self,
        name: Symbol,
        sp: Span,
    ) -> Result<AstPattern, ParseError> {
        if name == "self" && self.impl_target_stack.is_empty() {
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PatternSyntax,
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
                Some(Token::Ident(_) | Token::ReservedCallName(_) | Token::True | Token::False)
            )
        {
            self.consume_path_separator()?;
            let (seg, seg_span) = match self.peek().clone() {
                Token::Ident(_) | Token::ReservedCallName(_) => self.expect_callable_ident()?,
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
                    inners.push(parser.parse_pattern_argument()?);
                    parser.skip_newlines();
                    while matches!(parser.peek(), Token::Comma) {
                        parser.advance();
                        parser.skip_newlines();
                        if matches!(parser.peek(), Token::RParen) {
                            break;
                        }
                        inners.push(parser.parse_pattern_argument()?);
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
        // The lexer emits adjacent `()` as Unit. In a head application
        // this denotes an empty argument list, not a Unit pattern.
        if matches!(self.peek(), Token::Unit) {
            let end = self.advance().span;
            return Ok(AstPattern::Call(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                callee_name,
                Vec::new(),
            ));
        }
        if is_ctor {
            let ctor_name = callee_name;
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
                crate::error::ParseErrorReason::PatternSyntax,
                "Qualified patterns support constructor forms only",
                Span {
                    start: sp.start,
                    end: path_end,
                },
            ));
        }

        if sindr::names::is_reserved_value_name(&name) {
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PatternSyntax,
                "Reserved call names cannot be bound or shadowed",
                sp,
            ));
        }
        self.ensure_non_const_identifier(&name, sp.clone(), "Pattern binding")?;
        if matches!(self.peek(), Token::Colon) {
            self.advance();
            return Ok(AstPattern::Annotated(sp, name, self.parse_type()?));
        }
        Ok(AstPattern::Var(sp, name))
    }
}

fn pattern_or_span(pattern: &AstPattern) -> Option<&Span> {
    match pattern {
        AstPattern::Call(_, _, args) => args
            .iter()
            .filter_map(|arg| arg.pattern.as_deref())
            .find_map(pattern_or_span),
        AstPattern::Or(span, _) => Some(span),
        AstPattern::As(_, inner, _, _, _) | AstPattern::Projection { inner, .. } => {
            pattern_or_span(inner)
        }
        AstPattern::ListCons(_, head, tail) => {
            pattern_or_span(head).or_else(|| pattern_or_span(tail))
        }
        AstPattern::Constructor(_, _, items) | AstPattern::Tuple(_, items) => {
            items.iter().find_map(pattern_or_span)
        }
        AstPattern::Var(_, _)
        | AstPattern::Annotated(_, _, _)
        | AstPattern::Wildcard(_)
        | AstPattern::AnnotatedWildcard(_, _)
        | AstPattern::Pin(_, _)
        | AstPattern::ListNil(_)
        | AstPattern::IntLit(_, _)
        | AstPattern::StrLit(_, _)
        | AstPattern::BoolLit(_, _)
        | AstPattern::DurationLit(_, _) => None,
    }
}

pub(super) fn pattern_contains_pin(pattern: &AstPattern) -> bool {
    match pattern {
        AstPattern::Call(_, _, args) => args
            .iter()
            .filter_map(|arg| arg.pattern.as_deref())
            .any(pattern_contains_pin),
        AstPattern::Pin(_, _) => true,
        AstPattern::As(_, inner, _, _, _) | AstPattern::Projection { inner, .. } => {
            pattern_contains_pin(inner)
        }
        AstPattern::ListCons(_, head, tail) => {
            pattern_contains_pin(head) || pattern_contains_pin(tail)
        }
        AstPattern::Constructor(_, _, items)
        | AstPattern::Tuple(_, items)
        | AstPattern::Or(_, items) => items.iter().any(pattern_contains_pin),
        AstPattern::Var(_, _)
        | AstPattern::Annotated(_, _, _)
        | AstPattern::Wildcard(_)
        | AstPattern::AnnotatedWildcard(_, _)
        | AstPattern::ListNil(_)
        | AstPattern::IntLit(_, _)
        | AstPattern::StrLit(_, _)
        | AstPattern::BoolLit(_, _)
        | AstPattern::DurationLit(_, _) => false,
    }
}
