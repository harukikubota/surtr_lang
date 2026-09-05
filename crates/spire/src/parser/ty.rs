use crate::ast::*;
use crate::error::ParseError;
use crate::token::Token;

use super::Parser;

/// The syntactic position occupied by a parsed type.
///
/// Type constructor *identity* is not available in Spire, so this tracks only
/// source-determinable structure while parsing a declaration. Identity-aware
/// position checks remain in later phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TypePosition {
    General,
    /// The root position of a trait or impl method signature. Nested types
    /// inherit this position so `Self<$...>` remains available as a
    /// substitution marker throughout that signature.
    DirectSignatureParameter,
    DirectSignatureReturn,
}

#[derive(Debug, Clone)]
pub(super) struct TypeParseContext {
    impl_target: Option<String>,
    position: TypePosition,
}

impl TypeParseContext {
    pub(super) fn general(impl_target: Option<String>) -> Self {
        Self {
            impl_target,
            position: TypePosition::General,
        }
    }

    pub(super) fn direct_signature_parameter(impl_target: Option<String>) -> Self {
        Self {
            impl_target,
            position: TypePosition::DirectSignatureParameter,
        }
    }

    pub(super) fn direct_signature_return(impl_target: Option<String>) -> Self {
        Self {
            impl_target,
            position: TypePosition::DirectSignatureReturn,
        }
    }

    fn nested(&self) -> Self {
        Self {
            impl_target: self.impl_target.clone(),
            position: self.position,
        }
    }

    fn permits_signature_self_application(&self) -> bool {
        matches!(
            self.position,
            TypePosition::DirectSignatureParameter | TypePosition::DirectSignatureReturn
        )
    }

    fn permits_constructor_variable_application(&self) -> bool {
        matches!(
            self.position,
            TypePosition::DirectSignatureParameter | TypePosition::DirectSignatureReturn
        )
    }
}

impl Parser<'_> {
    // ── Type annotation parsing ──

    fn wrap_optional_ty(&self, inner: AstTy, end: usize) -> AstTy {
        let start = super::ast_ty_span(&inner).start;
        AstTy::Generic(Span { start, end }, "Option".to_string(), vec![inner])
    }

    fn parse_optional_type_suffix(&mut self, mut ty: AstTy) -> AstTy {
        while matches!(self.peek(), Token::Question) {
            let end = self.advance().span.end;
            ty = self.wrap_optional_ty(ty, end);
        }
        ty
    }

    pub(super) fn parse_type(&mut self) -> Result<AstTy, ParseError> {
        self.parse_type_in_context(TypeParseContext::general(
            self.impl_target_stack.last().cloned(),
        ))
    }

    pub(super) fn parse_type_in_impl_context(
        &mut self,
        impl_target: Option<String>,
    ) -> Result<AstTy, ParseError> {
        self.parse_type_in_context(TypeParseContext::general(impl_target))
    }

    pub(super) fn parse_direct_signature_parameter_type(
        &mut self,
        impl_target: Option<String>,
    ) -> Result<AstTy, ParseError> {
        self.parse_type_in_context(TypeParseContext::direct_signature_parameter(impl_target))
    }

    pub(super) fn parse_direct_signature_return_type(
        &mut self,
        impl_target: Option<String>,
    ) -> Result<AstTy, ParseError> {
        self.parse_type_in_context(TypeParseContext::direct_signature_return(impl_target))
    }

    pub(super) fn parse_type_in_context(
        &mut self,
        context: TypeParseContext,
    ) -> Result<AstTy, ParseError> {
        self.skip_newlines();
        let sp = self.peek_span();
        let impl_target = context.impl_target.clone();
        let nested_context = context.nested();

        if matches!(self.peek(), Token::LParen) {
            return self.with_parse_nesting(sp.clone(), |parser| {
                parser.advance();
                parser.skip_newlines();
                if matches!(parser.peek(), Token::Arrow) {
                    parser.advance();
                    let ret = parser.parse_type_in_context(nested_context.clone())?;
                    parser.skip_newlines();
                    let end = parser.expect(&Token::RParen)?;
                    return Ok(parser.parse_optional_type_suffix(AstTy::Func(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        Vec::new(),
                        Box::new(ret),
                    )));
                }

                let mut params = Vec::new();
                params.push(parser.parse_type_in_context(nested_context.clone())?);
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
                    params.push(parser.parse_type_in_context(nested_context.clone())?);
                    parser.skip_newlines();
                }
                if matches!(parser.peek(), Token::Arrow) {
                    parser.advance();
                    let ret = parser.parse_type_in_context(nested_context.clone())?;
                    parser.skip_newlines();
                    let end = parser.expect(&Token::RParen)?;
                    return Ok(parser.parse_optional_type_suffix(AstTy::Func(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        params,
                        Box::new(ret),
                    )));
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
                Ok(parser.parse_optional_type_suffix(AstTy::Tuple(
                    Span {
                        start: sp.start,
                        end: end.end,
                    },
                    params,
                )))
            });
        }

        if matches!(self.peek(), Token::Dollar) {
            self.advance();
            let (name, end) = self.expect_ident()?;
            let name = format!("${}", name);
            if name == "$Self" {
                return Err(ParseError::syntax("Invalid type variable name: $Self", sp));
            }
            if matches!(self.peek(), Token::Lt) {
                if !context.permits_constructor_variable_application() {
                    return Err(ParseError::syntax(
                        "type constructor variables may only be applied in callable signature types",
                        sp,
                    ));
                }
                return self.with_parse_nesting(sp.clone(), |parser| {
                    parser.advance();
                    parser.skip_newlines();
                    let mut args = vec![parser.parse_type_in_context(nested_context.clone())?];
                    parser.skip_newlines();
                    while matches!(parser.peek(), Token::Comma) {
                        parser.advance();
                        parser.skip_newlines();
                        args.push(parser.parse_type_in_context(nested_context.clone())?);
                        parser.skip_newlines();
                    }
                    let end = parser.expect_type_gt()?;
                    Ok(parser.parse_optional_type_suffix(AstTy::Generic(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        name,
                        args,
                    )))
                });
            }
            return Ok(self.parse_optional_type_suffix(AstTy::Named(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                name,
            )));
        }

        if matches!(self.peek(), Token::Unit) {
            let end = self.advance().span.clone();
            return Ok(self.parse_optional_type_suffix(AstTy::Named(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                "Unit".to_string(),
            )));
        }

        if matches!(self.peek(), Token::Impl) {
            return Err(ParseError::syntax(
                "Anonymous `impl Trait` types are not supported; introduce a named type slot and constrain it with `where`",
                sp,
            ));
        }

        // Named type, possibly with type args: Result<Int>, List<Int>, Option<Int>, ...
        let (name, name_span) = self.expect_qualified_ident(2, "type")?;
        if name == "Self" {
            if impl_target.is_none() {
                return Err(ParseError::syntax(
                    "`Self` can only be used inside trait or impl declarations",
                    sp,
                ));
            }
            if matches!(self.peek(), Token::Lt) {
                if !context.permits_signature_self_application() {
                    return Err(ParseError::syntax(
                        "`Self<...>` is only allowed inside an impl or trait-method signature",
                        sp,
                    ));
                }
                return self.with_parse_nesting(sp.clone(), |parser| {
                    parser.advance();
                    parser.skip_newlines();
                    let mut args = vec![parser.parse_type_in_context(nested_context.clone())?];
                    parser.skip_newlines();
                    while matches!(parser.peek(), Token::Comma) {
                        parser.advance();
                        parser.skip_newlines();
                        args.push(parser.parse_type_in_context(nested_context.clone())?);
                        parser.skip_newlines();
                    }
                    let end = parser.expect_type_gt()?;
                    Ok(parser.parse_optional_type_suffix(AstTy::Generic(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        "Self".to_string(),
                        args,
                    )))
                });
            }
            return Ok(self.parse_optional_type_suffix(AstTy::Named(
                Span {
                    start: sp.start,
                    end: name_span.end,
                },
                "Self".to_string(),
            )));
        }
        if name == "self" {
            return Err(ParseError::syntax("`self` is not a type name", sp));
        }

        // Check for type parameters: Name<T> or Name<T, E>
        if matches!(self.peek(), Token::Lt) {
            return self.with_parse_nesting(sp.clone(), |parser| {
                parser.advance();
                parser.skip_newlines();
                let mut args = vec![parser.parse_type_in_context(nested_context.clone())?];
                parser.skip_newlines();
                while matches!(parser.peek(), Token::Comma) {
                    parser.advance();
                    parser.skip_newlines();
                    args.push(parser.parse_type_in_context(nested_context.clone())?);
                    parser.skip_newlines();
                }
                let end = parser.expect_type_gt()?;
                Ok(parser.parse_optional_type_suffix(AstTy::Generic(
                    Span {
                        start: sp.start,
                        end: end.end,
                    },
                    name,
                    args,
                )))
            });
        }

        Ok(self.parse_optional_type_suffix(AstTy::Named(
            Span {
                start: sp.start,
                end: name_span.end,
            },
            name,
        )))
    }

    pub(super) fn is_self_type(ty: &AstTy) -> bool {
        matches!(ty, AstTy::Named(_, name) | AstTy::Generic(_, name, _) if name == "Self")
    }

    pub(super) fn is_impl_receiver_type(ty: &AstTy, target: &str) -> bool {
        let same_target = |name: &str| {
            name == target
                || name.rsplit("::").next().unwrap_or(name)
                    == target.rsplit("::").next().unwrap_or(target)
        };
        Self::is_self_type(ty)
            || matches!(ty, AstTy::Named(_, name) | AstTy::Generic(_, name, _) if same_target(name))
    }
}
