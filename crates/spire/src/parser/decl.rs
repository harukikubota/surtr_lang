use crate::ast::*;
use crate::error::ParseError;
use crate::token::Token;

use super::ast_ty_span;
use super::context::{DeclLevel, TopLevelDeclKind};
use super::Parser;

impl Parser {
    pub(super) fn parse_field_visibility(&mut self) -> Visibility {
        if matches!(self.peek(), Token::Private) {
            self.advance();
            self.skip_newlines();
            Visibility::Private
        } else {
            Visibility::Public
        }
    }

    pub(super) fn parse_import_selector_list(&mut self) -> Result<(Vec<Symbol>, Span), ParseError> {
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut names = Vec::new();
        loop {
            if matches!(self.peek(), Token::RBrace) {
                break;
            }
            let (name, _span) = self.expect_ident()?;
            names.push(name);
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::RBrace) {
                    break;
                }
                continue;
            }
            break;
        }

        if names.is_empty() {
            return Err(ParseError::syntax(
                "Import list requires at least one symbol",
                self.peek_span(),
            ));
        }

        let end = self.expect(&Token::RBrace)?;
        Ok((names, end))
    }

    pub(super) fn parse_import(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Import)?;
        let (first_seg, first_span) = self.expect_ident()?;
        let path_start = first_span.start;
        let mut qualified = vec![(first_seg, first_span)];
        let mut saw_separator = false;

        while self.has_path_separator() && matches!(self.peek_n(2), Some(Token::Ident(_))) {
            saw_separator = true;
            self.consume_path_separator()?;
            let (seg, seg_span) = self.expect_ident()?;
            qualified.push((seg, seg_span));
        }

        let (module_segments, module_end, spec, mut stmt_end) =
            if self.has_path_separator() && matches!(self.peek_n(2), Some(Token::LBrace)) {
                self.consume_path_separator()?;
                let (names, end) = self.parse_import_selector_list()?;
                (
                    qualified.iter().map(|(name, _)| name.clone()).collect(),
                    qualified.last().expect("non-empty path").1.end,
                    ImportSpec::List(names),
                    end.end,
                )
            } else if self.has_path_separator() {
                return Err(ParseError::syntax(
                    "Expected identifier or `{` after `::` in import",
                    self.peek_span(),
                ));
            } else if saw_separator {
                let (name, selected_span) = qualified
                    .pop()
                    .expect("qualified import with separator has at least 2 segments");
                (
                    qualified.iter().map(|(module, _)| module.clone()).collect(),
                    qualified.last().expect("module path is non-empty").1.end,
                    ImportSpec::Single(name),
                    selected_span.end,
                )
            } else {
                (
                    qualified.iter().map(|(name, _)| name.clone()).collect(),
                    qualified.last().expect("non-empty path").1.end,
                    ImportSpec::All,
                    qualified.last().expect("non-empty path").1.end,
                )
            };

        if matches!(self.peek(), Token::Semicolon) {
            stmt_end = self.advance().span.end;
        }

        let path = AstPath {
            span: Span {
                start: path_start,
                end: module_end,
            },
            segments: module_segments,
        };

        Ok(Ast::Import(
            Span {
                start: sp.start,
                end: stmt_end,
            },
            path,
            spec,
        ))
    }

    pub(super) fn parse_include(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Include)?;
        self.skip_newlines();
        let (path, mut stmt_end) = match self.peek().clone() {
            Token::Str(path) => {
                let str_span = self.advance().span.clone();
                (path, str_span.end)
            }
            _ => {
                return Err(ParseError::syntax(
                    "include expects a string literal path",
                    self.peek_span(),
                ))
            }
        };

        if matches!(self.peek(), Token::Semicolon) {
            stmt_end = self.advance().span.end;
        }

        Ok(Ast::Include(
            Span {
                start: sp.start,
                end: stmt_end,
            },
            path,
        ))
    }

    pub(super) fn parse_defmod(&mut self) -> Result<Ast, ParseError> {
        self.parse_defmod_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_trait_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_trait_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_trait_impl_head(&mut self) -> Result<(Symbol, Vec<AstTy>), ParseError> {
        let (trait_name, _) = self.expect_ident()?;
        let trait_args = if matches!(self.peek(), Token::Lt) {
            self.advance();
            self.skip_newlines();
            let mut args = Vec::new();
            if !matches!(self.peek(), Token::Gt) {
                loop {
                    args.push(self.parse_type_in_impl_context(None)?);
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        continue;
                    }
                    break;
                }
            }
            self.expect_type_gt()?;
            args
        } else {
            Vec::new()
        };
        Ok((trait_name, trait_args))
    }

    pub(super) fn parse_impl_def(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Impl)?;
        let (head, trait_args) = self.parse_trait_impl_head()?;
        self.skip_newlines();

        if matches!(self.peek(), Token::For) {
            self.advance();
            self.skip_newlines();
            let target_ty = self.parse_type_in_impl_context(None)?;
            let self_target = self.trait_impl_self_target_name(&target_ty)?;
            self.skip_newlines();
            self.expect(&Token::LBrace)?;
            self.skip_newlines();

            let mut methods = Vec::new();
            while !matches!(self.peek(), Token::RBrace) {
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete("}", self.peek_span()));
                }
                if !matches!(self.peek(), Token::Def) {
                    return Err(ParseError::syntax(
                        "trait impl body may only contain `def` declarations",
                        self.peek_span(),
                    ));
                }
                let method = self.parse_impl_method(&self_target)?;
                self.ensure_stmt_boundary(&method, true)?;
                methods.push(method);
                while matches!(self.peek(), Token::Newline) {
                    self.advance();
                }
            }

            let end = self.expect(&Token::RBrace)?;
            return Ok(Ast::TraitImplDef(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                head,
                trait_args,
                target_ty,
                methods,
            ));
        }

        if !trait_args.is_empty() {
            return Err(ParseError::syntax(
                "Plain `impl Type { ... }` does not accept trait-style type arguments",
                self.peek_span(),
            ));
        }

        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut methods = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            if !matches!(self.peek(), Token::Def | Token::Defp) {
                return Err(ParseError::syntax(
                    "impl body may only contain `def` / `defp` declarations",
                    self.peek_span(),
                ));
            }
            let method = self.parse_impl_method(&head)?;
            self.ensure_stmt_boundary(&method, true)?;
            methods.push(method);
            while matches!(self.peek(), Token::Newline) {
                self.advance();
            }
        }

        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::ImplDef(
            Span {
                start: sp.start,
                end: end.end,
            },
            head,
            methods,
        ))
    }

    pub(super) fn parse_impl_method(&mut self, target: &str) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        let visibility = match self.peek() {
            Token::Def => {
                self.advance();
                Visibility::Public
            }
            Token::Defp => {
                self.advance();
                Visibility::Private
            }
            _ => {
                return Err(ParseError::syntax(
                    "Expected `def` or `defp`",
                    self.peek_span(),
                ));
            }
        };
        let (name, _) = self.expect_ident()?;
        let mut params = Vec::new();

        if matches!(self.peek(), Token::Unit) {
            self.advance();
        } else {
            self.expect(&Token::LParen)?;
            self.skip_newlines();
            let mut first_param = true;
            if !matches!(self.peek(), Token::RParen) {
                loop {
                    if matches!(self.peek(), Token::Eof) {
                        return Err(ParseError::incomplete(")", self.peek_span()));
                    }
                    self.skip_newlines();
                    let (param_name, param_span) = self.expect_ident()?;

                    let param_ty = if param_name == "self" {
                        if !first_param {
                            return Err(ParseError::syntax(
                                "`self` is only allowed as the first parameter of impl methods",
                                param_span,
                            ));
                        }
                        if matches!(self.peek(), Token::Colon) {
                            self.advance();
                            self.skip_newlines();
                            let ty = self.parse_type_in_impl_context(Some(target.to_string()))?;
                            if !Self::is_self_type(&ty) {
                                return Err(ParseError::syntax(
                                    "`self` receiver type must be `Self`",
                                    ast_ty_span(&ty).clone(),
                                ));
                            }
                            ty
                        } else {
                            AstTy::Named(param_span.clone(), "Self".to_string())
                        }
                    } else {
                        self.expect(&Token::Colon)?;
                        self.skip_newlines();
                        self.parse_type_in_impl_context(Some(target.to_string()))?
                    };

                    params.push(FunParam {
                        name: param_name,
                        ty: param_ty,
                        span: param_span,
                    });
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                    first_param = false;
                }
            }
            self.expect(&Token::RParen)?;
        }

        let ret_ty = if matches!(self.peek(), Token::Arrow) {
            self.advance();
            self.skip_newlines();
            Some(self.parse_type_in_impl_context(Some(target.to_string()))?)
        } else {
            None
        };

        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.impl_target_stack.push(target.to_string());
        let body_stmts = self.parse_block_stmts();
        self.impl_target_stack.pop();
        let body_stmts = body_stmts?;
        if body_stmts.is_empty() {
            return Err(ParseError::syntax(
                "Function body must not be empty",
                self.peek_span(),
            ));
        }
        let end = self.expect(&Token::RBrace)?;
        let body = Ast::Block(
            Span {
                start: sp.start,
                end: end.end,
            },
            body_stmts,
        );

        Ok(Ast::Def(
            Span {
                start: sp.start,
                end: end.end,
            },
            name,
            Vec::new(),
            params,
            ret_ty,
            Box::new(body),
            DeclAttrs {
                visibility,
                ..DeclAttrs::default()
            },
        ))
    }

    pub(super) fn trait_impl_self_target_name(&self, ty: &AstTy) -> Result<String, ParseError> {
        match ty {
            AstTy::Named(_, name) => Ok(name.clone()),
            AstTy::Generic(_, name, args) => {
                if args.is_empty() {
                    Ok(name.clone())
                } else {
                    Err(ParseError::syntax(
                        "trait impl target must be a concrete named type in V1",
                        ast_ty_span(ty).clone(),
                    ))
                }
            }
            _ => Err(ParseError::syntax(
                "trait impl target must be a concrete named type in V1",
                ast_ty_span(ty).clone(),
            )),
        }
    }

    pub(super) fn parse_defmod_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        if self.context.module_path.is_some() {
            return Err(ParseError::syntax(
                "Nested module declarations are not allowed",
                sp,
            ));
        }
        self.expect(&Token::Defmod)?;
        let (name, _) = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body = self.parse_module_body_stmts(Some(name.clone()))?;
        let end = self.expect(&Token::RBrace)?;

        Ok(Ast::Defmod(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            body,
            attrs,
        ))
    }

    pub(super) fn parse_trait_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Deftrait)?;
        let (name, _) = self.expect_ident()?;
        let type_params = self.parse_decl_type_params()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut methods = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            if !matches!(self.peek(), Token::Def) {
                return Err(ParseError::syntax(
                    "trait body may only contain `def` signatures",
                    self.peek_span(),
                ));
            }
            let method = self.parse_trait_method_sig()?;
            methods.push(method);
            while matches!(self.peek(), Token::Newline) {
                self.advance();
            }
        }

        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::TraitDef(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            type_params,
            methods,
            attrs,
        ))
    }

    pub(super) fn parse_trait_method_sig(&mut self) -> Result<TraitMethodSig, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Def)?;
        let (name, _) = self.expect_ident()?;
        let type_params = self.parse_decl_type_params()?;
        let mut params = Vec::new();
        let self_context = Some("Self".to_string());

        if matches!(self.peek(), Token::Unit) {
            self.advance();
        } else {
            self.expect(&Token::LParen)?;
            self.skip_newlines();

            if !matches!(self.peek(), Token::RParen) {
                loop {
                    if matches!(self.peek(), Token::Eof) {
                        return Err(ParseError::incomplete(")", self.peek_span()));
                    }
                    self.skip_newlines();
                    params.push(
                        self.parse_trait_method_param(params.is_empty(), self_context.clone())?,
                    );
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            self.expect(&Token::RParen)?;
        }

        self.skip_newlines();
        self.expect(&Token::Arrow)?;
        self.skip_newlines();
        let ret_ty = self.parse_type_in_impl_context(self_context)?;
        if matches!(ret_ty, AstTy::ImplTrait(_, _)) {
            return Err(ParseError::syntax(
                "return-position `impl Trait` is not supported; name the type parameter explicitly",
                ast_ty_span(&ret_ty).clone(),
            ));
        }
        self.reject_where_clause()?;

        let mut lookahead = self.pos;
        while matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::Newline)
        ) {
            lookahead += 1;
        }

        if matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::LBrace)
        ) {
            return Err(ParseError::syntax(
                "trait method declarations must not have a body",
                self.tokens[lookahead].span.clone(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            sp.end
        };

        Ok(TraitMethodSig {
            name,
            type_params,
            params,
            ret_ty,
            span: Span {
                start: sp.start,
                end,
            },
        })
    }

    pub(super) fn parse_trait_method_param(
        &mut self,
        is_first_param: bool,
        self_context: Option<String>,
    ) -> Result<FunParam, ParseError> {
        let (name, span) = self.expect_ident()?;
        if name == "self" {
            if !is_first_param {
                return Err(ParseError::syntax(
                    "`self` is only allowed as the first parameter of trait methods",
                    span,
                ));
            }

            let ty = if matches!(self.peek(), Token::Colon) {
                self.advance();
                self.skip_newlines();
                let ty = self.parse_type_in_impl_context(self_context)?;
                if !Self::is_self_type(&ty) {
                    return Err(ParseError::syntax(
                        "`self` receiver type must be `Self`",
                        ast_ty_span(&ty).clone(),
                    ));
                }
                ty
            } else {
                AstTy::Named(span.clone(), "Self".to_string())
            };
            return Ok(FunParam { name, ty, span });
        }

        self.expect(&Token::Colon)?;
        let ty = self.parse_type_in_impl_context(self_context)?;
        Ok(FunParam { name, ty, span })
    }

    // ── Data definitions (step 7, 9) ──

    /// `defstruct Name { field: Type, ... }`
    pub(super) fn parse_struct_def(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Defstruct)?;
        let (name, _) = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut fields = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            self.skip_newlines();
            let visibility = self.parse_field_visibility();
            let (fname, fspan) = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let fty = self.parse_type()?;
            fields.push(StructField {
                name: fname,
                ty: fty,
                span: fspan,
                visibility,
            });
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::StructDef(
            Span {
                start: sp.start,
                end: end.end,
            },
            name,
            fields,
        ))
    }

    /// `defrecord Name(field: Type, ...)`
    pub(super) fn parse_record_def(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Defrecord)?;
        let (name, _) = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        self.skip_newlines();

        let mut fields = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete(")", self.peek_span()));
                }
                self.skip_newlines();
                let visibility = self.parse_field_visibility();
                let (fname, fspan) = self.expect_ident()?;
                self.expect(&Token::Colon)?;
                let fty = self.parse_type()?;
                fields.push(RecordField {
                    name: fname,
                    ty: fty,
                    span: fspan,
                    visibility,
                });
                self.skip_newlines();
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                    if matches!(self.peek(), Token::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        let end = self.expect(&Token::RParen)?;
        Ok(Ast::RecordDef(
            Span {
                start: sp.start,
                end: end.end,
            },
            name,
            fields,
        ))
    }

    pub(super) fn parse_enum_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_enum_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_enum_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        start_override: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Defenum)?;
        let (name, _name_span) = self.expect_ident()?;
        let type_params = self.parse_decl_type_params()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut variants = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            self.skip_newlines();
            let variant_start = self.peek_span().start;
            let (variant_name, _) = self.expect_ident()?;
            let mut payload = Vec::new();

            if matches!(self.peek(), Token::LParen) {
                self.advance();
                self.skip_newlines();
                if !matches!(self.peek(), Token::RParen) {
                    payload.push(self.parse_type()?);
                    self.skip_newlines();
                    while matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                        payload.push(self.parse_type()?);
                        self.skip_newlines();
                    }
                }
                self.expect(&Token::RParen)?;
            }

            let discriminant = if matches!(self.peek(), Token::Bind) {
                self.advance();
                self.skip_newlines();
                Some(self.parse_enum_discriminant()?)
            } else {
                None
            };

            let variant_end = if self.pos > 0 {
                self.tokens[self.pos - 1].span.end
            } else {
                variant_start
            };
            variants.push(EnumVariant {
                name: variant_name,
                payload,
                discriminant,
                span: Span {
                    start: variant_start,
                    end: variant_end,
                },
            });

            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                continue;
            }
        }

        if variants.is_empty() {
            return Err(ParseError::syntax(
                "Enum definition requires at least one variant",
                Span {
                    start: sp.start,
                    end: sp.end,
                },
            ));
        }

        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::EnumDef(
            Span {
                start: start_override.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            type_params
                .into_iter()
                .map(|param| TypeParam {
                    name: param.name,
                    bound: param.bound,
                    span: param.span,
                })
                .collect(),
            variants,
            attrs,
        ))
    }

    pub(super) fn parse_decl_type_params(&mut self) -> Result<Vec<TypeParam>, ParseError> {
        if !matches!(self.peek(), Token::Lt) {
            return Ok(Vec::new());
        }

        self.advance();
        self.skip_newlines();

        let mut params = Vec::new();
        loop {
            let param_span = self.peek_span();
            self.expect(&Token::Dollar)?;
            let (param_name, _) = self.expect_ident()?;
            let bound = if matches!(self.peek(), Token::Colon) {
                self.advance();
                self.skip_newlines();
                let (bound_name, _) = self.expect_ident()?;
                Some(bound_name)
            } else {
                None
            };
            params.push(TypeParam {
                name: format!("${}", param_name),
                bound,
                span: param_span,
            });
            self.skip_newlines();

            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                continue;
            }

            if matches!(self.peek(), Token::Gt) {
                self.expect(&Token::Gt)?;
                break;
            }

            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete(">", self.peek_span()));
            }

            return Err(ParseError::syntax(
                "Expected `,` or `>` in declaration type parameter list",
                self.peek_span(),
            ));
        }

        Ok(params)
    }

    pub(super) fn parse_enum_discriminant(
        &mut self,
    ) -> Result<sindr::primitives::SurtrInt, ParseError> {
        let span = self.peek_span();
        if matches!(self.peek(), Token::Minus) {
            self.advance();
            let int_span = self.peek_span();
            let Token::Int(n) = self.peek().clone() else {
                return Err(ParseError::syntax(
                    "Expected integer literal after '-' in enum discriminant",
                    int_span,
                ));
            };
            self.advance();
            return Ok(-n);
        }
        match self.peek().clone() {
            Token::Int(n) => {
                self.advance();
                Ok(n)
            }
            Token::Eof => Err(ParseError::incomplete("integer literal", span)),
            _ => Err(ParseError::syntax(
                "Enum discriminant must be an integer literal",
                span,
            )),
        }
    }

    /// `deferror Name { expr }` or `deferror Name(fields) { expr }`
    pub(super) fn parse_deferror_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_deferror_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_deferror_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        start_override: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Deferror)?;
        let (name, _) = self.expect_ident()?;

        // Optional fields: (field: Type, ...)
        let mut fields = Vec::new();
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            self.skip_newlines();
            if !matches!(self.peek(), Token::RParen) {
                loop {
                    if matches!(self.peek(), Token::Eof) {
                        return Err(ParseError::incomplete(")", self.peek_span()));
                    }
                    self.skip_newlines();
                    let visibility = self.parse_field_visibility();
                    let (fname, fspan) = self.expect_ident()?;
                    self.expect(&Token::Colon)?;
                    let fty = self.parse_type()?;
                    fields.push(RecordField {
                        name: fname,
                        ty: fty,
                        span: fspan,
                        visibility,
                    });
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            self.expect(&Token::RParen)?;
        }

        // Show block: { expr }
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();
        let show_expr = self.parse_expr()?;
        self.skip_newlines();
        let end = self.expect(&Token::RBrace)?;

        Ok(Ast::DeferrorDef(
            Span {
                start: start_override.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            fields,
            Box::new(show_expr),
            attrs,
        ))
    }

    pub(super) fn parse_def_signature(
        &mut self,
    ) -> Result<
        (
            Span,
            Symbol,
            Vec<TypeParam>,
            Vec<FunParam>,
            Option<AstTy>,
            Visibility,
        ),
        ParseError,
    > {
        self.parse_def_signature_with_name_mode(false)
    }

    pub(super) fn parse_def_signature_with_name_mode(
        &mut self,
        allow_builtin_keyword_name: bool,
    ) -> Result<
        (
            Span,
            Symbol,
            Vec<TypeParam>,
            Vec<FunParam>,
            Option<AstTy>,
            Visibility,
        ),
        ParseError,
    > {
        let sp = self.peek_span();
        let visibility = match self.peek() {
            Token::Def => {
                self.advance();
                Visibility::Public
            }
            Token::Defp => {
                self.advance();
                Visibility::Private
            }
            _ => {
                return Err(ParseError::syntax(
                    "Expected `def` or `defp`",
                    self.peek_span(),
                ));
            }
        };
        let (name, _) = if allow_builtin_keyword_name {
            self.expect_builtin_decl_name()?
        } else {
            self.expect_ident()?
        };
        let type_params = self.parse_decl_type_params()?;
        let mut params = Vec::new();
        if matches!(self.peek(), Token::Unit) {
            self.advance();
        } else {
            self.expect(&Token::LParen)?;
            self.skip_newlines();

            if !matches!(self.peek(), Token::RParen) {
                loop {
                    if matches!(self.peek(), Token::Eof) {
                        return Err(ParseError::incomplete(")", self.peek_span()));
                    }
                    self.skip_newlines();
                    params.push(self.parse_fun_param()?);
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            self.expect(&Token::RParen)?;
        }

        let ret_ty = if matches!(self.peek(), Token::Arrow) {
            self.advance();
            self.skip_newlines();
            let ret_ty = self.parse_type()?;
            if matches!(ret_ty, AstTy::ImplTrait(_, _)) {
                return Err(ParseError::syntax(
                    "return-position `impl Trait` is not supported; name the type parameter explicitly",
                    ast_ty_span(&ret_ty).clone(),
                ));
            }
            Some(ret_ty)
        } else {
            None
        };

        self.reject_where_clause()?;

        Ok((sp, name, type_params, params, ret_ty, visibility))
    }

    pub(super) fn parse_extractor_signature(
        &mut self,
    ) -> Result<(Span, Symbol, Vec<TypeParam>, ExtractorParam, AstTy), ParseError> {
        self.parse_extractor_signature_with_name_mode(false)
    }

    pub(super) fn parse_extractor_signature_with_name_mode(
        &mut self,
        allow_builtin_keyword_name: bool,
    ) -> Result<(Span, Symbol, Vec<TypeParam>, ExtractorParam, AstTy), ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Defextractor)?;
        let (name, name_span) = if allow_builtin_keyword_name {
            self.expect_builtin_decl_name()?
        } else {
            self.expect_ident()?
        };
        let type_params = self.parse_decl_type_params()?;
        if Self::is_constructor_style_name(&name) {
            return Err(ParseError::syntax(
                format!(
                    "Extractor names must not use constructor-style names like `{}`; implement `{}`::deconstruct(...) instead",
                    name, name
                ),
                name_span,
            ));
        }
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        self.skip_newlines();
        let (param_name, param_span) = self.expect_ident()?;
        self.skip_newlines();
        let param_ty = if matches!(self.peek(), Token::Colon) {
            self.advance();
            self.skip_newlines();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.skip_newlines();
        self.expect(&Token::RParen)?;
        self.skip_newlines();
        self.expect(&Token::Arrow)?;
        self.skip_newlines();
        let ret_ty = self.parse_type()?;
        if matches!(ret_ty, AstTy::ImplTrait(_, _)) {
            return Err(ParseError::syntax(
                "return-position `impl Trait` is not supported; name the type parameter explicitly",
                ast_ty_span(&ret_ty).clone(),
            ));
        }
        self.reject_where_clause()?;
        Ok((
            sp,
            name,
            type_params,
            ExtractorParam {
                name: param_name,
                ty: param_ty,
                span: param_span,
            },
            ret_ty,
        ))
    }

    pub(super) fn reject_where_clause(&self) -> Result<(), ParseError> {
        if matches!(self.peek(), Token::Where) {
            return Err(ParseError::syntax(
                "`where` clauses are staged and not implemented yet",
                self.peek_span(),
            ));
        }
        Ok(())
    }

    pub(super) fn is_constructor_style_name(name: &str) -> bool {
        name.chars().next().is_some_and(|ch| ch.is_uppercase())
    }

    pub(super) fn parse_annotated_decl(&mut self) -> Result<Ast, ParseError> {
        let mut attrs = DeclAttrs::default();
        let mut saw_builtin = false;
        let mut start_span: Option<Span> = None;

        while let Token::Annotator(name) = self.peek().clone() {
            let annotator_span = self.peek_span();
            if start_span.is_none() {
                start_span = Some(annotator_span.clone());
            }
            self.advance();
            self.skip_newlines();
            match name.as_str() {
                "builtin" => {
                    if saw_builtin {
                        return Err(ParseError::syntax(
                            "@@builtin may only appear once before a declaration",
                            annotator_span,
                        ));
                    }
                    saw_builtin = true;
                }
                "doc" => {
                    if attrs.doc.is_some() {
                        return Err(ParseError::syntax(
                            "@@doc may only appear once before a declaration",
                            annotator_span,
                        ));
                    }
                    let token = self.peek().clone();
                    match token {
                        Token::DocString(text) => {
                            self.advance();
                            attrs.doc = Some(text);
                        }
                        Token::Eof => {
                            return Err(ParseError::incomplete("doc string", self.peek_span()));
                        }
                        _ => {
                            return Err(ParseError::syntax(
                                "@@doc expects a triple-quoted doc string",
                                self.peek_span(),
                            ));
                        }
                    }
                }
                "autoimport" => {
                    if attrs.auto_import {
                        return Err(ParseError::syntax(
                            "@@autoimport may only appear once before a declaration",
                            annotator_span,
                        ));
                    }
                    attrs.auto_import = true;
                }
                _ => {
                    return Err(ParseError::syntax(
                        format!("Unknown annotator: @@{}", name),
                        annotator_span,
                    ));
                }
            }
            self.skip_newlines();
        }

        let start = start_span
            .map(|span| span.start)
            .unwrap_or_else(|| self.peek_span().start);

        if saw_builtin {
            match self.peek() {
                Token::Def => self.parse_builtin_decl(start, attrs),
                Token::Defextractor => self.parse_builtin_extractor_decl(start, attrs),
                Token::Type => self.parse_builtin_type_decl(start, attrs),
                _ => Err(ParseError::syntax(
                    "Expected `def`, `defextractor`, or `type` after @@builtin",
                    self.peek_span(),
                )),
            }
        } else {
            match self.peek() {
                Token::Def => self.parse_def_with_attrs(attrs, Some(start)),
                Token::Defmod => self.parse_defmod_with_attrs(attrs, Some(start)),
                Token::Deftrait => self.parse_trait_def_with_attrs(attrs, Some(start)),
                Token::Deferror => self.parse_deferror_def_with_attrs(attrs, Some(start)),
                Token::Defenum => self.parse_enum_def_with_attrs(attrs, Some(start)),
                Token::Defextractor => self.parse_extractor_def_with_attrs(attrs, Some(start)),
                Token::Eof => Err(ParseError::incomplete("declaration", self.peek_span())),
                _ => Err(ParseError::syntax(
                    "@@doc / @@autoimport must annotate `def`, `defmod`, `deftrait`, `deferror`, `defenum`, `defextractor`, or `@@builtin type/def/defextractor`",
                    self.peek_span(),
                )),
            }
        }
    }

    pub(super) fn parse_builtin_decl(
        &mut self,
        start: usize,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        let (_def_span, name, _type_params, params, ret_ty, _visibility) =
            self.parse_def_signature_with_name_mode(true)?;

        let mut lookahead = self.pos;
        while matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::Newline)
        ) {
            lookahead += 1;
        }

        if matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::LBrace)
        ) {
            return Err(ParseError::syntax(
                "@@builtin declaration must not have a function body",
                self.tokens[lookahead].span.clone(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };

        Ok(Ast::BuiltinDecl(
            Span { start, end },
            name,
            params,
            ret_ty,
            attrs,
        ))
    }

    pub(super) fn parse_builtin_extractor_decl(
        &mut self,
        start: usize,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        let (_sp, name, _type_params, param, ret_ty) =
            self.parse_extractor_signature_with_name_mode(true)?;

        let mut lookahead = self.pos;
        while matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::Newline)
        ) {
            lookahead += 1;
        }

        if matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::LBrace)
        ) {
            return Err(ParseError::syntax(
                "@@builtin extractor declaration must not have a function body",
                self.tokens[lookahead].span.clone(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };

        Ok(Ast::BuiltinExtractorDecl(
            Span { start, end },
            name,
            param,
            ret_ty,
            attrs,
        ))
    }

    pub(super) fn parse_builtin_type_decl(
        &mut self,
        start: usize,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        self.expect(&Token::Type)?;
        self.skip_newlines();
        let (name, name_span) = self.expect_ident()?;

        // `Result` keeps `Ok` / `Err` as declaration-only constructor
        // contracts. They intentionally live behind `@@builtin type ...` so
        // the std-module declaration layer stays visually uniform, even though
        // the payload that follows is function-shaped rather than type-shaped.
        if (name == "Ok" || name == "Err") && matches!(self.peek(), Token::LParen) {
            return self.parse_result_ctor_builtin_type_decl(start, name, attrs);
        }

        let mut params = Vec::new();
        if matches!(self.peek(), Token::Lt) {
            self.advance();
            self.skip_newlines();
            loop {
                self.expect(&Token::Dollar)?;
                let (param_name, _) = self.expect_ident()?;
                params.push(format!("${}", param_name));
                self.skip_newlines();
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                    continue;
                }
                if matches!(self.peek(), Token::Gt) {
                    let gt = self.expect(&Token::Gt)?;
                    let end = if self.pos > 0 {
                        self.tokens[self.pos - 1].span.end
                    } else {
                        gt.end
                    };
                    return Ok(Ast::BuiltinTypeDecl(
                        Span { start, end },
                        BuiltinTypeHead {
                            span: Span {
                                start: name_span.start,
                                end,
                            },
                            name,
                            params,
                        },
                        attrs,
                    ));
                }
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete(">", self.peek_span()));
                }
                return Err(ParseError::syntax(
                    "Expected `,` or `>` in builtin type parameter list",
                    self.peek_span(),
                ));
            }
        }
        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };

        Ok(Ast::BuiltinTypeDecl(
            Span { start, end },
            BuiltinTypeHead {
                span: Span { start, end },
                name,
                params,
            },
            attrs,
        ))
    }

    pub(super) fn parse_result_ctor_builtin_type_decl(
        &mut self,
        start: usize,
        name: Symbol,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        self.skip_newlines();
        let param_ty = self.parse_type()?;
        self.skip_newlines();
        self.expect(&Token::RParen)?;
        self.skip_newlines();
        self.expect(&Token::Arrow)?;
        self.skip_newlines();
        let ret_ty = self.parse_type()?;

        if matches!(self.peek(), Token::LBrace) {
            return Err(ParseError::syntax(
                "Result constructor builtin contracts in std modules must not have a function body",
                self.peek_span(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };

        Ok(Ast::ResultCtorDecl(
            Span { start, end },
            name,
            param_ty,
            ret_ty,
            attrs,
        ))
    }

    /// `def name(arg: Type, ...) -> Type { expr }`
    pub(super) fn parse_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_extractor_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_extractor_def_with_attrs(DeclAttrs::default(), None)
    }

    pub(super) fn parse_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        if self.should_parse_result_ctor_decl() {
            return self.parse_result_ctor_decl_with_attrs(attrs, annotator_start);
        }

        let (sp, name, type_params, params, ret_ty, visibility) = self.parse_def_signature()?;
        let mut attrs = attrs;
        attrs.visibility = visibility;

        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body_stmts = self.parse_block_stmts()?;
        if body_stmts.is_empty() {
            return Err(ParseError::syntax(
                "Function body must not be empty",
                self.peek_span(),
            ));
        }
        let end = self.expect(&Token::RBrace)?;
        let body = Ast::Block(
            Span {
                start: sp.start,
                end: end.end,
            },
            body_stmts,
        );

        Ok(Ast::Def(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            type_params,
            params,
            ret_ty,
            Box::new(body),
            attrs,
        ))
    }

    pub(super) fn parse_extractor_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let (sp, name, type_params, param, ret_ty) = self.parse_extractor_signature()?;

        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body_stmts = self.parse_block_stmts()?;
        if body_stmts.is_empty() {
            return Err(ParseError::syntax(
                "Extractor body must not be empty",
                self.peek_span(),
            ));
        }
        let end = self.expect(&Token::RBrace)?;
        let body = Ast::Block(
            Span {
                start: sp.start,
                end: end.end,
            },
            body_stmts,
        );

        Ok(Ast::ExtractorDef(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            type_params,
            param,
            ret_ty,
            Box::new(body),
            attrs,
        ))
    }

    pub(super) fn should_parse_result_ctor_decl(&self) -> bool {
        if self.context.level != DeclLevel::Top {
            return false;
        }
        if self.context.module_path.is_some() {
            return false;
        }
        if !self
            .context
            .parse_rules
            .allowed_top_level_decl_kinds
            .allows(TopLevelDeclKind::BuiltinDecl)
        {
            return false;
        }
        if !matches!(self.peek(), Token::Def) {
            return false;
        }
        matches!(
            self.tokens.get(self.pos + 1).map(|sp| &sp.token),
            Some(Token::Ident(name)) if name == "Ok" || name == "Err"
        )
    }

    pub(super) fn parse_result_ctor_decl_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Def)?;
        let (name, _) = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        self.skip_newlines();
        let param_ty = self.parse_type()?;
        self.skip_newlines();
        self.expect(&Token::RParen)?;
        self.skip_newlines();
        self.expect(&Token::Arrow)?;
        self.skip_newlines();
        let ret_ty = self.parse_type()?;

        if matches!(self.peek(), Token::LBrace) {
            return Err(ParseError::syntax(
                "Result constructor declarations in std modules must not have a function body",
                self.peek_span(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            sp.start
        };

        Ok(Ast::ResultCtorDecl(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end,
            },
            name,
            param_ty,
            ret_ty,
            attrs,
        ))
    }

    pub(super) fn parse_fun_param(&mut self) -> Result<FunParam, ParseError> {
        let (name, span) = self.expect_ident()?;
        if name == "self" {
            return Err(ParseError::syntax(
                "`self` is only allowed as the first parameter of impl methods",
                span,
            ));
        }
        self.expect(&Token::Colon)?;
        let ty = self.parse_type()?;
        Ok(FunParam { name, ty, span })
    }
}
