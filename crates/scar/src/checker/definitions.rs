use super::*;

impl Checker {
    pub(super) fn check_builtin_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        params: &[ResolvedFunParam],
        ret_ty: &Option<AstTy>,
    ) -> Result<TypedNode, TypeError> {
        if Self::is_special_form_builtin_decl_name(&id.name) {
            return self.check_special_form_builtin_decl(span, id, params, ret_ty);
        }

        let meta = builtin_meta_by_name(&id.name).ok_or_else(|| TypeError {
            message: format!("Unknown builtin declaration: {}", id.name),
            span: span.clone(),
            hint: None,
        })?;
        if params.len() != usize::from(meta.arity) {
            return Err(TypeError {
                message: format!(
                    "Builtin {} arity mismatch: expected {}, got {}",
                    id.name,
                    meta.arity,
                    params.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let mut tyvars = HashMap::new();
        let param_tys = params
            .iter()
            .map(|param| self.resolve_builtin_ast_ty(&param.ty, &mut tyvars))
            .collect::<Result<Vec<_>, _>>()?;
        let ret = match ret_ty {
            Some(ty) => self.resolve_builtin_ast_ty_in_context(
                ty,
                TypeSyntaxContext::FunctionReturn,
                &mut tyvars,
            )?,
            None => Ty::Unit,
        };

        self.env.bind_var(
            id.unique_id,
            Ty::BuiltinFunc {
                name: id.name.clone(),
                params: param_tys,
                ret: Box::new(ret),
            },
        );

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Lit(Lit::Unit),
        })
    }

    pub(super) fn check_special_form_builtin_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        params: &[ResolvedFunParam],
        ret_ty: &Option<AstTy>,
    ) -> Result<TypedNode, TypeError> {
        let expected_qname = match id.name.as_str() {
            "if" => "Kernel::if",
            "if_then" => "Kernel::if_then",
            "assert" => "Kernel::assert",
            "ensure" => "Kernel::ensure",
            "and" => "Kernel::and",
            "or" => "Kernel::or",
            _ => unreachable!(),
        };

        if id.qualified_name.as_deref() != Some(expected_qname) {
            return Err(TypeError {
                message: format!(
                    "Special-form declaration `{}` is only allowed in std module `Kernel`.",
                    id.name
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let shape_ok = match id.name.as_str() {
            "if" => {
                params.len() == 3
                    && Self::is_named_type(&params[0].ty, "Boolean")
                    && Self::is_zero_arg_func_to_named(&params[1].ty, "$A")
                    && Self::is_zero_arg_func_to_named(&params[2].ty, "$A")
                    && ret_ty
                        .as_ref()
                        .is_some_and(|ty| Self::is_named_type(ty, "$A"))
            }
            "if_then" => {
                params.len() == 2
                    && Self::is_named_type(&params[0].ty, "Boolean")
                    && Self::is_zero_arg_func_to_unit(&params[1].ty)
                    && ret_ty.as_ref().is_some_and(Self::is_unit_type)
            }
            "assert" => {
                params.len() == 2
                    && Self::is_named_type(&params[0].ty, "Boolean")
                    && Self::is_named_type(&params[1].ty, "Error")
                    && ret_ty
                        .as_ref()
                        .is_some_and(|ty| Self::is_result_of_named(ty, "Unit"))
            }
            "ensure" => {
                params.len() == 3
                    && Self::is_named_type(&params[0].ty, "$A")
                    && Self::is_unary_func_from_named_to_named(&params[1].ty, "$A", "Boolean")
                    && Self::is_named_type(&params[2].ty, "Error")
                    && ret_ty
                        .as_ref()
                        .is_some_and(|ty| Self::is_result_of_named(ty, "$A"))
            }
            "and" | "or" => {
                params.len() == 2
                    && Self::is_named_type(&params[0].ty, "Boolean")
                    && Self::is_named_type(&params[1].ty, "Boolean")
                    && ret_ty
                        .as_ref()
                        .is_some_and(|ty| Self::is_named_type(ty, "Boolean"))
            }
            _ => false,
        };

        if !shape_ok {
            let expected = match id.name.as_str() {
                "if" => "@@builtin def if(flag: Boolean, then_branch: (-> $A), else_branch: (-> $A)) -> $A",
                "if_then" => "@@builtin def if_then(flag: Boolean, then_branch: (-> ())) -> ()",
                "assert" => "@@builtin def assert(flag: Boolean, err: Error) -> Result<Unit>",
                "ensure" => "@@builtin def ensure(value: $A, pred: ($A -> Boolean), err: Error) -> Result<$A>",
                "and" => "@@builtin def and(left: Boolean, right: Boolean) -> Boolean",
                "or" => "@@builtin def or(left: Boolean, right: Boolean) -> Boolean",
                _ => unreachable!(),
            };
            return Err(TypeError {
                message: format!(
                    "Special-form declaration must match the canonical contract: {}",
                    expected
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let mut tyvars = HashMap::new();
        let param_tys = params
            .iter()
            .map(|param| self.resolve_builtin_ast_ty(&param.ty, &mut tyvars))
            .collect::<Result<Vec<_>, _>>()?;
        let ret = match ret_ty {
            Some(ty) => self.resolve_builtin_ast_ty_in_context(
                ty,
                TypeSyntaxContext::FunctionReturn,
                &mut tyvars,
            )?,
            None => Ty::Unit,
        };

        self.env.bind_var(
            id.unique_id,
            Ty::BuiltinFunc {
                name: id.name.clone(),
                params: param_tys,
                ret: Box::new(ret),
            },
        );

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Lit(Lit::Unit),
        })
    }

    pub(super) fn check_builtin_type_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        params: &[String],
        _attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
        let Some(meta) = builtin_type_meta_by_name(&id.name) else {
            return Err(TypeError {
                message: format!("Unknown builtin type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            });
        };

        let exact_params_match = params.len() == meta.params.len()
            && params
                .iter()
                .zip(meta.params.iter())
                .all(|(actual, expected)| actual == expected);
        if !exact_params_match {
            return Err(TypeError {
                message: format!(
                    "Builtin type {} must be declared as {}{}",
                    id.name,
                    id.name,
                    format_builtin_type_param_suffix(meta.params)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        if self.enforce_builtin_type_contracts {
            if let Some((_, first_span)) = self.seen_builtin_type_decls.get(&id.name) {
                return Err(TypeError {
                    message: format!("Duplicate builtin type declaration: {}", id.name),
                    span: span.clone(),
                    hint: Some(format!(
                        "Already declared at {}..{}",
                        first_span.start, first_span.end
                    )),
                });
            }
            self.seen_builtin_type_decls
                .insert(id.name.clone(), (params.to_vec(), span.clone()));
        }

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Lit(Lit::Unit),
        })
    }

    pub(super) fn check_builtin_extractor_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        param: &ResolvedExtractorParam,
        ret_ty: &AstTy,
    ) -> Result<TypedNode, TypeError> {
        let mut tyvars = HashMap::new();
        let param_ty = match &param.ty {
            Some(ty) => {
                self.resolve_builtin_ast_ty_in_context(ty, TypeSyntaxContext::General, &mut tyvars)?
            }
            None => self.env.fresh_tyvar(),
        };
        let ret = self.resolve_builtin_ast_ty_in_context(
            ret_ty,
            TypeSyntaxContext::FunctionReturn,
            &mut tyvars,
        )?;
        self.require_match_result_seq_ty(&ret, &param.id.span, &format!("Extractor {}", id.name))?;

        self.env.bind_var(
            id.unique_id,
            Ty::BuiltinFunc {
                name: id.name.clone(),
                params: vec![param_ty.clone()],
                ret: Box::new(ret.clone()),
            },
        );

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::BuiltinExtractorDecl(id.clone(), param_ty, ret),
        })
    }

    pub(super) fn check_result_ctor_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        param_ty: &AstTy,
        ret_ty: &AstTy,
        _attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
        let expected_qname = match id.name.as_str() {
            "Ok" => "Result::Ok",
            "Err" => "Result::Err",
            other => {
                return Err(TypeError {
                    message: format!(
                        "Unknown Result constructor declaration: {}. Only Ok and Err are supported.",
                        other
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }
        };

        if id.qualified_name.as_deref() != Some(expected_qname) {
            return Err(TypeError {
                message: format!(
                    "Result constructor declaration `{}` is only allowed in std module `Result`.",
                    id.name
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let shape_ok = match id.name.as_str() {
            "Ok" => Self::is_named_type(param_ty, "$T") && Self::is_result_of_named(ret_ty, "$T"),
            "Err" => {
                Self::is_named_type(param_ty, "Error") && Self::is_result_of_named(ret_ty, "$T")
            }
            _ => false,
        };

        if !shape_ok {
            let expected = match id.name.as_str() {
                "Ok" => "@@builtin type Ok($T) -> Result<$T>",
                "Err" => "@@builtin type Err(Error) -> Result<$T>",
                _ => unreachable!(),
            };
            return Err(TypeError {
                message: format!(
                    "Result constructor declaration must match the canonical contract: {}",
                    expected
                ),
                span: span.clone(),
                hint: None,
            });
        }

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Lit(Lit::Unit),
        })
    }

    pub(super) fn is_named_type(ast_ty: &AstTy, expected_name: &str) -> bool {
        matches!(ast_ty, AstTy::Named(_, name) if name == expected_name)
    }

    pub(super) fn is_unit_type(ast_ty: &AstTy) -> bool {
        Self::is_named_type(ast_ty, "Unit")
    }

    pub(super) fn is_zero_arg_func_to_named(ast_ty: &AstTy, expected_name: &str) -> bool {
        matches!(
            ast_ty,
            AstTy::Func(_, params, ret)
                if params.is_empty()
                    && matches!(ret.as_ref(), AstTy::Named(_, name) if name == expected_name)
        )
    }

    pub(super) fn is_zero_arg_func_to_unit(ast_ty: &AstTy) -> bool {
        Self::is_zero_arg_func_to_named(ast_ty, "Unit")
    }

    pub(super) fn is_unary_func_from_named_to_named(
        ast_ty: &AstTy,
        expected_param_name: &str,
        expected_ret_name: &str,
    ) -> bool {
        matches!(
            ast_ty,
            AstTy::Func(_, params, ret)
                if params.len() == 1
                    && matches!(&params[0], AstTy::Named(_, name) if name == expected_param_name)
                    && matches!(ret.as_ref(), AstTy::Named(_, name) if name == expected_ret_name)
        )
    }

    pub(super) fn is_special_form_builtin_decl_name(name: &str) -> bool {
        matches!(
            name,
            "if" | "if_then"
                | "assert"
                | "ensure"
                | "and"
                | "or"
                | "eq"
                | "neq"
                | "lt"
                | "lte"
                | "gt"
                | "gte"
                | "concat"
        )
    }

    pub(super) fn is_result_of_named(ast_ty: &AstTy, expected_name: &str) -> bool {
        matches!(
            ast_ty,
            AstTy::Generic(_, name, args)
                if name == "Result"
                    && args.len() == 1
                    && matches!(&args[0], AstTy::Named(_, param_name) if param_name == expected_name)
        )
    }

    pub(super) fn ensure_builtin_type_contracts(&self) -> Result<(), TypeError> {
        if !self.enforce_builtin_type_contracts {
            return Ok(());
        }

        for meta in BUILTIN_TYPE_METAS {
            if !self.seen_builtin_type_decls.contains_key(meta.name) {
                return Err(TypeError {
                    message: format!(
                        "Missing builtin type declaration: {}{}",
                        meta.name,
                        format_builtin_type_param_suffix(meta.params)
                    ),
                    span: Span { start: 0, end: 0 },
                    hint: None,
                });
            }
        }

        Ok(())
    }

    pub(super) fn check_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        type_params: &[ResolvedTypeParam],
        params: &[ResolvedFunParam],
        ret_ty: &Option<AstTy>,
        body: &Resolved,
        attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
        let mut fun_env = self.env.clone();
        let mut typed_params = Vec::new();
        let mut tyvars = HashMap::new();
        self.seed_signature_type_params(type_params, &mut tyvars);

        for param in params {
            let param_ty = self.resolve_signature_ast_ty_in_context(
                &param.ty,
                TypeSyntaxContext::General,
                &mut tyvars,
            )?;
            if matches!(param_ty, Ty::Lens(_, _)) {
                return Err(TypeError {
                    message:
                        "Lens is compile-time only in Stage1 and cannot be used as a function parameter type"
                            .into(),
                    span: param.id.span.clone(),
                    hint: None,
                });
            }
            fun_env.bind_var(param.id.unique_id, param_ty.clone());
            typed_params.push(TypedFunParam {
                id: param.id.clone(),
                ty: param_ty.clone(),
            });
        }

        let expected_ret = match ret_ty {
            Some(ty) => self.resolve_signature_ast_ty_in_context(
                ty,
                TypeSyntaxContext::FunctionReturn,
                &mut tyvars,
            )?,
            None => Ty::Unit,
        };
        if matches!(expected_ret, Ty::Lens(_, _)) {
            return Err(TypeError {
                message:
                    "Lens is compile-time only in Stage1 and cannot be used as a function return type"
                        .into(),
                span: span.clone(),
                hint: None,
            });
        }

        let current_symbol = id.qualified_name.clone().unwrap_or_else(|| id.name.clone());
        let is_entrypoint = self
            .source_rules
            .normalized_entrypoint
            .as_deref()
            .is_some_and(|entry| entry == current_symbol);
        if is_entrypoint {
            if !params.is_empty() {
                return Err(TypeError {
                    message: format!(
                        "entrypoint `{}` must have signature () -> Result<()>",
                        current_symbol
                    ),
                    span: span.clone(),
                    hint: Some("Remove entrypoint parameters and return Result<()>.".into()),
                });
            }
            if !Self::is_main_result_unit_ty(&expected_ret) {
                let legacy_main = current_symbol == "main"
                    && self
                        .source_rules
                        .normalized_entrypoint
                        .as_deref()
                        .is_some_and(|entry| entry == "main");
                return Err(TypeError {
                    message: if legacy_main {
                        "main must declare return type Result<()>".into()
                    } else {
                        format!(
                            "entrypoint `{}` must declare return type Result<()>",
                            current_symbol
                        )
                    },
                    span: span.clone(),
                    hint: Some(
                        "Define entrypoint as `def <name>() -> Result<()> { ... }` and return Ok(()) or Err(error)."
                            .into(),
                    ),
                });
            }
        }

        let mut body_checker = self.spawn_child_checker(fun_env);
        if let Some((impl_target, _method)) = Self::split_impl_method_name(&id.name) {
            if self
                .env
                .lookup_type_def(&impl_target)
                .is_some_and(|def| def.kind == crate::env::TypeKind::Struct)
            {
                body_checker.current_impl_struct_target = Some(impl_target);
            }
        }
        body_checker.function_return_ty = Some(expected_ret.clone());
        body_checker.current_function_symbol = Some(current_symbol);
        let typed_body = body_checker.check_node(body)?;
        let typed_body = body_checker.resolve_typed_node(typed_body);
        self.absorb_child_progress(&body_checker);

        if !self.types_compatible(&expected_ret, &typed_body.ty) {
            let hint = if matches!(typed_body.ty, Ty::Unit) {
                body_checker.describe_unit_return_hint(&typed_body)
            } else {
                None
            };
            return Err(TypeError {
                message: if ret_ty.is_some() {
                    format!(
                        "expected {}, got {}",
                        self.ty_name(&expected_ret),
                        self.ty_name(&typed_body.ty)
                    )
                } else {
                    format!(
                        "def {} without an explicit return type must return Unit, got {}",
                        id.name,
                        self.ty_name(&typed_body.ty)
                    )
                },
                span: body_checker.return_mismatch_span(&typed_body),
                hint,
            });
        }

        let fun_idx = match self.env.lookup_var(id.unique_id) {
            Some(Ty::UserFunc { fun_idx, .. }) => *fun_idx,
            _ => {
                return Err(TypeError {
                    message: format!("Undefined function: {}", id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
        };
        let typed_type_params = type_params
            .iter()
            .filter_map(|param| match tyvars.get(&param.name) {
                Some(Ty::Var(var)) => Some(TypedTypeParam {
                    name: param.name.clone(),
                    ty_var: *var,
                    bound: param.bound.clone(),
                }),
                _ => None,
            })
            .collect::<Vec<_>>();
        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Def(
                fun_idx,
                id.clone(),
                typed_type_params,
                typed_params,
                expected_ret,
                Box::new(typed_body),
                attrs.visibility,
            ),
        })
    }

    pub(super) fn check_extractor_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        type_params: &[ResolvedTypeParam],
        param: &ResolvedExtractorParam,
        ret_ty: &AstTy,
        body: &Resolved,
        attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
        let mut fun_env = self.env.clone();
        let mut tyvars = HashMap::new();
        self.seed_signature_type_params(type_params, &mut tyvars);

        let param_ty = match &param.ty {
            Some(ty) => self.resolve_signature_ast_ty_in_context(
                ty,
                TypeSyntaxContext::General,
                &mut tyvars,
            )?,
            None => self.env.fresh_tyvar(),
        };
        if matches!(param_ty, Ty::Lens(_, _)) {
            return Err(TypeError {
                message:
                    "Lens is compile-time only in Stage1 and cannot be used as an extractor parameter type"
                        .into(),
                span: param.id.span.clone(),
                hint: None,
            });
        }
        fun_env.bind_var(param.id.unique_id, param_ty.clone());
        let typed_param = TypedFunParam {
            id: param.id.clone(),
            ty: param_ty,
        };

        let expected_ret = self.resolve_signature_ast_ty_in_context(
            ret_ty,
            TypeSyntaxContext::FunctionReturn,
            &mut tyvars,
        )?;
        if matches!(expected_ret, Ty::Lens(_, _)) {
            return Err(TypeError {
                message:
                    "Lens is compile-time only in Stage1 and cannot be used as an extractor return type"
                        .into(),
                span: span.clone(),
                hint: None,
            });
        }
        self.require_match_result_seq_ty(
            &expected_ret,
            &param.id.span,
            &format!("Extractor {}", id.name),
        )?;

        let current_symbol = id.qualified_name.clone().unwrap_or_else(|| id.name.clone());
        let mut body_checker = self.spawn_child_checker(fun_env);
        body_checker.function_return_ty = Some(expected_ret.clone());
        body_checker.current_function_symbol = Some(current_symbol);
        let typed_body = body_checker.check_node(body)?;
        let typed_body = body_checker.resolve_typed_node(typed_body);
        self.absorb_child_progress(&body_checker);

        if !self.types_compatible(&expected_ret, &typed_body.ty) {
            let hint = if matches!(typed_body.ty, Ty::Unit) {
                body_checker.describe_unit_return_hint(&typed_body)
            } else {
                None
            };
            return Err(TypeError {
                message: format!(
                    "expected {}, got {}",
                    self.ty_name(&expected_ret),
                    self.ty_name(&typed_body.ty)
                ),
                span: body_checker.return_mismatch_span(&typed_body),
                hint,
            });
        }

        let fun_idx = match self.env.lookup_var(id.unique_id) {
            Some(Ty::UserFunc { fun_idx, .. }) => *fun_idx,
            _ => {
                return Err(TypeError {
                    message: format!("Undefined extractor: {}", id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
        };

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::ExtractorDef(
                fun_idx,
                id.clone(),
                type_params
                    .iter()
                    .filter_map(|param| match tyvars.get(&param.name) {
                        Some(Ty::Var(var)) => Some(TypedTypeParam {
                            name: param.name.clone(),
                            ty_var: *var,
                            bound: param.bound.clone(),
                        }),
                        _ => None,
                    })
                    .collect(),
                TypedFunParam {
                    id: typed_param.id,
                    ty: self.resolve_ty(&typed_param.ty),
                },
                self.resolve_ty(&expected_ret),
                Box::new(typed_body),
                attrs.visibility,
            ),
        })
    }

    pub(super) fn check_trait_impl_items(
        &mut self,
        span: &Span,
        trait_id: &ResolvedId,
        trait_args: &[AstTy],
        target_ast_ty: &AstTy,
        methods: &[ResolvedTraitImplMethod],
    ) -> Result<Vec<TypedNode>, TypeError> {
        let target_ty =
            self.resolve_ast_ty_in_context(target_ast_ty, TypeSyntaxContext::General)?;
        let target_name = self
            .trait_target_name(&target_ty)
            .ok_or_else(|| TypeError {
                message: "trait impl target must be a concrete named type".into(),
                span: Self::ast_ty_span(target_ast_ty).clone(),
                hint: None,
            })?;
        let trait_key = self.trait_key(trait_id);
        let trait_info = self
            .traits
            .get(&trait_key)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Unknown trait: {}", trait_id.name),
                span: span.clone(),
                hint: None,
            })?;
        let mut typed_nodes = vec![TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::TraitImplDef(
                self.trait_instance_key(trait_id, trait_args),
                target_name.clone(),
            ),
        }];

        for method in methods {
            let trait_method = trait_info
                .methods
                .get(&method.method_name)
                .cloned()
                .ok_or_else(|| TypeError {
                    message: format!(
                        "Trait impl {} for {} defines unknown method `{}`",
                        trait_id.name, target_name, method.method_name
                    ),
                    span: method.span.clone(),
                    hint: None,
                })?;

            let inline_method = TraitImplMethodInfo {
                method_name: method.method_name.clone(),
                function_id: method.function_id.clone(),
                type_params: method.type_params.clone(),
                params: method.params.clone(),
                ret_ty: method.ret_ty.clone(),
                body: method.body.clone(),
                attrs: method.attrs.clone(),
                span: method.span.clone(),
                dispatch_override: None,
            };
            let (param_tys, expected_ret, type_params) = self.resolve_trait_impl_method_signature(
                &trait_info,
                trait_args,
                &inline_method,
                &target_ty,
                &trait_method.ret_ty,
            )?;

            let mut fun_env = self.env.clone();
            let mut typed_params = Vec::new();
            for (param, param_ty) in method.params.iter().zip(param_tys.iter()) {
                fun_env.bind_var(param.id.unique_id, param_ty.clone());
                typed_params.push(TypedFunParam {
                    id: param.id.clone(),
                    ty: param_ty.clone(),
                });
            }

            let mut body_checker = self.spawn_child_checker(fun_env);
            if self
                .env
                .lookup_type_def(&target_name)
                .is_some_and(|def| def.kind == crate::env::TypeKind::Struct)
            {
                body_checker.current_impl_struct_target = Some(target_name.clone());
            }
            body_checker.function_return_ty = Some(expected_ret.clone());
            body_checker.current_function_symbol = Some(method.function_id.name.clone());
            let typed_body = body_checker.check_node(&method.body)?;
            let typed_body = body_checker.resolve_typed_node(typed_body);
            self.absorb_child_progress(&body_checker);

            if !self.types_compatible(&expected_ret, &typed_body.ty) {
                let hint = if matches!(typed_body.ty, Ty::Unit) {
                    body_checker.describe_unit_return_hint(&typed_body)
                } else {
                    None
                };
                return Err(TypeError {
                    message: format!(
                        "expected {}, got {}",
                        self.ty_name(&expected_ret),
                        self.ty_name(&typed_body.ty)
                    ),
                    span: body_checker.return_mismatch_span(&typed_body),
                    hint,
                });
            }

            let fun_idx = match self.env.lookup_var(method.function_id.unique_id) {
                Some(Ty::UserFunc { fun_idx, .. }) => *fun_idx,
                _ => {
                    return Err(TypeError {
                        message: format!("Undefined function: {}", method.function_id.name),
                        span: method.span.clone(),
                        hint: None,
                    });
                }
            };
            let typed_type_params = method
                .type_params
                .iter()
                .zip(type_params.iter())
                .map(|(param, ty_var)| TypedTypeParam {
                    name: param.name.clone(),
                    ty_var: *ty_var,
                    bound: param.bound.clone(),
                })
                .collect::<Vec<_>>();
            typed_nodes.push(TypedNode {
                ty: Ty::Unit,
                span: method.span.clone(),
                node: TypedInner::Def(
                    fun_idx,
                    method.function_id.clone(),
                    typed_type_params,
                    typed_params,
                    expected_ret,
                    Box::new(typed_body),
                    method.attrs.visibility,
                ),
            });
        }

        Ok(typed_nodes)
    }

    pub(super) fn check_trait_impl_def(
        &mut self,
        span: &Span,
        trait_id: &ResolvedId,
        trait_args: &[AstTy],
        target_ast_ty: &AstTy,
        _methods: &[ResolvedTraitImplMethod],
    ) -> Result<TypedNode, TypeError> {
        let target_ty =
            self.resolve_ast_ty_in_context(target_ast_ty, TypeSyntaxContext::General)?;
        let target_name = self
            .trait_target_name(&target_ty)
            .ok_or_else(|| TypeError {
                message: "trait impl target must be a concrete named type".into(),
                span: Self::ast_ty_span(target_ast_ty).clone(),
                hint: None,
            })?;
        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::TraitImplDef(
                self.trait_instance_key(trait_id, trait_args),
                target_name,
            ),
        })
    }

    pub(super) fn is_main_result_unit_ty(ty: &Ty) -> bool {
        matches!(
            ty,
            Ty::Result(ok, err)
                if matches!(ok.as_ref(), Ty::Unit) && matches!(err.as_ref(), Ty::Error)
        )
    }

    pub(super) fn check_struct_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        fields: &[ResolvedField],
    ) -> Result<TypedNode, TypeError> {
        let ty_fields: Vec<(String, Ty)> = fields
            .iter()
            .map(|f| {
                Ok((
                    f.name.clone(),
                    self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?,
                ))
            })
            .collect::<Result<Vec<_>, TypeError>>()?;
        let private_fields = fields
            .iter()
            .filter(|field| field.visibility == spire::ast::Visibility::Private)
            .map(|field| field.name.clone())
            .collect::<HashSet<_>>();

        let tag = self
            .env
            .resolve_type_def_signature(&id.name, ty_fields.clone(), private_fields)
            .ok_or_else(|| TypeError {
                message: format!("Unknown struct type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

        self.env
            .bind_var(id.unique_id, Ty::Struct(id.name.clone(), ty_fields.clone()));

        let field_names: Vec<String> = ty_fields.iter().map(|(n, _)| n.clone()).collect();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::StructDef(tag, id.name.clone(), field_names),
        })
    }

    pub(super) fn check_enum_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        _type_params: &[ResolvedTypeParam],
        variants: &[ResolvedEnumVariant],
    ) -> Result<TypedNode, TypeError> {
        let enum_variants = self
            .env
            .enum_variants_of(&id.name)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Unknown enum type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

        if enum_variants.len() != variants.len() {
            return Err(TypeError {
                message: format!("Enum variant metadata mismatch: {}", id.name),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_variants = enum_variants
            .into_iter()
            .map(|variant| TypedEnumVariantDef {
                tag: variant.tag,
                constructor_name: variant.constructor_name,
                field_names: variant
                    .payload
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| format!("_{}", idx))
                    .collect(),
            })
            .collect::<Vec<_>>();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::EnumDef(id.name.clone(), typed_variants),
        })
    }

    pub(super) fn check_record_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        fields: &[ResolvedField],
    ) -> Result<TypedNode, TypeError> {
        let ty_fields: Vec<(String, Ty)> = fields
            .iter()
            .map(|f| {
                Ok((
                    f.name.clone(),
                    self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?,
                ))
            })
            .collect::<Result<Vec<_>, TypeError>>()?;
        let private_fields = fields
            .iter()
            .filter(|field| field.visibility == spire::ast::Visibility::Private)
            .map(|field| field.name.clone())
            .collect::<HashSet<_>>();

        let tag = self
            .env
            .resolve_type_def_signature(&id.name, ty_fields.clone(), private_fields)
            .ok_or_else(|| TypeError {
                message: format!("Unknown record type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

        self.env
            .bind_var(id.unique_id, Ty::Record(id.name.clone(), ty_fields.clone()));

        let field_names: Vec<String> = ty_fields.iter().map(|(n, _)| n.clone()).collect();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::RecordDef(tag, id.name.clone(), field_names),
        })
    }

    pub(super) fn check_struct_lit(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        field_vals: &[(String, Resolved)],
    ) -> Result<TypedNode, TypeError> {
        let def = self
            .env
            .lookup_type_def(&id.name)
            .ok_or_else(|| TypeError {
                message: format!("Unknown struct type: {}", id.name),
                span: span.clone(),
                hint: None,
            })?
            .clone();

        if self.current_impl_struct_target.as_deref() != Some(id.name.as_str()) {
            return Err(TypeError {
                message: format!(
                    "Struct literal `{}` is only allowed inside `impl {} {{ ... }}` method bodies",
                    id.name, id.name
                ),
                span: span.clone(),
                hint: Some(format!(
                    "Construct `{}` values via `{}(...)` / `{}::new(...)` outside the impl body.",
                    id.name, id.name, id.name
                )),
            });
        }

        let tag = def.tag;

        let mut seen = HashSet::new();
        for (name, _) in field_vals {
            if !def.fields.iter().any(|(field_name, _)| field_name == name) {
                return Err(TypeError {
                    message: format!("Unknown field '{}' in {}", name, id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
            if !seen.insert(name.clone()) {
                return Err(TypeError {
                    message: format!("Duplicate field '{}' in {}", name, id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
        }

        let mut typed_fields = Vec::new();
        for (def_name, def_ty) in &def.fields {
            let (_, resolved_val) =
                field_vals
                    .iter()
                    .find(|(n, _)| n == def_name)
                    .ok_or_else(|| TypeError {
                        message: format!("Missing field '{}' in {}", def_name, id.name),
                        span: span.clone(),
                        hint: None,
                    })?;
            let typed_val = self.check_node(resolved_val)?;
            if !self.types_compatible(def_ty, &typed_val.ty) {
                return Err(TypeError {
                    message: format!(
                        "Field '{}': expected {}, got {}",
                        def_name,
                        self.ty_name(def_ty),
                        self.ty_name(&typed_val.ty)
                    ),
                    span: typed_val.span.clone(),
                    hint: None,
                });
            }
            typed_fields.push(typed_val);
        }

        let result_ty = Ty::Struct(id.name.clone(), def.fields.clone());
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::StructLit(tag, typed_fields),
        })
    }

    pub(super) fn check_constructor_call(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        if id.name == "Ok" || id.name == "Err" {
            if args.len() != 1 {
                return Err(TypeError {
                    message: format!("{} expects 1 argument(s), got {}", id.name, args.len()),
                    span: span.clone(),
                    hint: None,
                });
            }
            let inner = match &args[0] {
                ResolvedRecordLitArg::Positional(expr) => {
                    let typed = self.check_node(expr)?;
                    self.maybe_call_zero_arg_function(typed, span.clone())
                }
                ResolvedRecordLitArg::Named(_, _) => {
                    return Err(TypeError {
                        message: format!("{} does not accept named arguments", id.name),
                        span: span.clone(),
                        hint: None,
                    });
                }
            };
            if id.name == "Err" {
                if !matches!(inner.ty, Ty::Error) {
                    return Err(TypeError {
                        message: "Err(...) requires a concrete deferror value.".into(),
                        span: inner.span.clone(),
                        hint: Some(
                            "Use a deferror-defined value in Err(...), not a plain value.".into(),
                        ),
                    });
                }
                if !self.is_concrete_error_value(&inner) {
                    return Err(TypeError {
                        message: "Error is abstract and cannot be constructed directly.".into(),
                        span: inner.span.clone(),
                        hint: Some("Use a concrete deferror value in Err(...).".into()),
                    });
                }
            }
            let (tag, result_ty) = if id.name == "Ok" {
                (
                    0u32,
                    Ty::Result(Box::new(inner.ty.clone()), Box::new(Ty::Error)),
                )
            } else {
                let ok_var = self.env.fresh_tyvar();
                (1u32, Ty::Result(Box::new(ok_var), Box::new(Ty::Error)))
            };
            return Ok(TypedNode {
                ty: result_ty,
                span: span.clone(),
                node: TypedInner::ConstructorCall(tag, vec![inner]),
            });
        }

        if let Some(variant) = self
            .env
            .enum_variant_by_constructor_id(id.unique_id)
            .cloned()
        {
            let variant = self.instantiate_enum_variant(&variant);
            if args.len() != variant.payload.len() {
                return Err(TypeError {
                    message: format!(
                        "{} expects {} argument(s), got {}",
                        id.name,
                        variant.payload.len(),
                        args.len()
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }
            let mut payload_values = Vec::new();
            for (idx, arg) in args.iter().enumerate() {
                let expected = &variant.payload[idx];
                let typed = match arg {
                    ResolvedRecordLitArg::Positional(expr) => self.check_node(expr)?,
                    ResolvedRecordLitArg::Named(_, _) => {
                        return Err(TypeError {
                            message: "Enum constructors do not accept named arguments".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                };
                if !self.types_compatible(expected, &typed.ty) {
                    return Err(TypeError {
                        message: format!(
                            "Argument type mismatch: expected {}, got {}",
                            self.ty_name(expected),
                            self.ty_name(&typed.ty)
                        ),
                        span: typed.span.clone(),
                        hint: None,
                    });
                }
                payload_values.push(typed);
            }

            let mut fields = Vec::with_capacity(payload_values.len() + 1);
            fields.push(TypedNode {
                ty: Ty::Int,
                span: span.clone(),
                node: TypedInner::Lit(Lit::Int(variant.discriminant)),
            });
            fields.extend(payload_values);

            return Ok(TypedNode {
                ty: self.resolve_ty(&variant.enum_ty),
                span: span.clone(),
                node: TypedInner::ConstructorCall(variant.tag, fields),
            });
        }

        if let Some(ty) = self.env.lookup_var(id.unique_id).cloned() {
            match &ty {
                Ty::BuiltinFunc { params, ret, .. } => {
                    if args.len() != params.len() {
                        return Err(TypeError {
                            message: format!(
                                "function expects {} argument(s), got {}",
                                params.len(),
                                args.len()
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }

                    let mut typed_args = Vec::new();
                    for (param_ty, arg) in params.iter().zip(args) {
                        let typed_val = match arg {
                            ResolvedRecordLitArg::Positional(expr) => self.check_node(expr)?,
                            ResolvedRecordLitArg::Named(_, _) => {
                                return Err(TypeError {
                                    message: "Function calls do not accept named arguments".into(),
                                    span: span.clone(),
                                    hint: None,
                                });
                            }
                        };
                        if !self.types_compatible(param_ty, &typed_val.ty) {
                            return Err(TypeError {
                                message: format!(
                                    "Argument type mismatch: expected {}, got {}",
                                    self.ty_name(param_ty),
                                    self.ty_name(&typed_val.ty)
                                ),
                                span: typed_val.span.clone(),
                                hint: None,
                            });
                        }
                        typed_args.push(typed_val);
                    }

                    return Ok(TypedNode {
                        ty: ret.as_ref().clone(),
                        span: span.clone(),
                        node: TypedInner::App(
                            Box::new(TypedNode {
                                ty: ty.clone(),
                                span: id.span.clone(),
                                node: TypedInner::Var(id.clone()),
                            }),
                            typed_args,
                        ),
                    });
                }
                Ty::UserFunc { params, ret, .. } => {
                    let typed_args =
                        self.typecheck_user_function_args(span, id.unique_id, params, args)?;
                    return Ok(TypedNode {
                        ty: ret.as_ref().clone(),
                        span: span.clone(),
                        node: TypedInner::App(
                            Box::new(TypedNode {
                                ty: ty.clone(),
                                span: id.span.clone(),
                                node: TypedInner::Var(id.clone()),
                            }),
                            typed_args,
                        ),
                    });
                }
                Ty::Func(params, ret) => {
                    if args
                        .iter()
                        .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
                    {
                        return Err(TypeError {
                            message: "Function calls do not accept named arguments".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    if args.len() != params.len() {
                        return Err(TypeError {
                            message: format!(
                                "function expects {} argument(s), got {}",
                                params.len(),
                                args.len()
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }

                    let mut typed_args = Vec::with_capacity(params.len());
                    for (expected_ty, arg) in params.iter().zip(args) {
                        let ResolvedRecordLitArg::Positional(expr) = arg else {
                            unreachable!("validated argument form above")
                        };
                        let typed = self.check_node(expr)?;
                        if !self.types_compatible(expected_ty, &typed.ty) {
                            return Err(TypeError {
                                message: format!(
                                    "Argument type mismatch: expected {}, got {}",
                                    self.ty_name(expected_ty),
                                    self.ty_name(&typed.ty)
                                ),
                                span: typed.span.clone(),
                                hint: None,
                            });
                        }
                        typed_args.push(typed);
                    }

                    return Ok(TypedNode {
                        ty: ret.as_ref().clone(),
                        span: span.clone(),
                        node: TypedInner::App(
                            Box::new(TypedNode {
                                ty: ty.clone(),
                                span: id.span.clone(),
                                node: TypedInner::Var(id.clone()),
                            }),
                            typed_args,
                        ),
                    });
                }
                _ => {}
            }
        }

        let def = self
            .env
            .lookup_type_def(&id.name)
            .ok_or_else(|| TypeError {
                message: format!("Unknown constructor type: {}", id.name),
                span: span.clone(),
                hint: None,
            })?
            .clone();

        if matches!(def.kind, crate::env::TypeKind::Struct) {
            let new_name = format!("{}::new", id.name);
            let Some(new_uid) = self.impl_method_uids.get(&new_name).copied() else {
                return Err(TypeError {
                    message: format!(
                        "Struct `{}` constructor call requires `{}` but no such method was found",
                        id.name, new_name
                    ),
                    span: span.clone(),
                    hint: Some(format!(
                        "Define `impl {} {{ def new(...) -> Self {{ ... }} }}`.",
                        id.name
                    )),
                });
            };
            let new_ty = self
                .env
                .lookup_var(new_uid)
                .cloned()
                .ok_or_else(|| TypeError {
                    message: format!("Undefined function: {}", new_name),
                    span: span.clone(),
                    hint: None,
                })?;
            let (params, ret_ty) = match new_ty.clone() {
                Ty::UserFunc { params, ret, .. }
                | Ty::BuiltinFunc { params, ret, .. }
                | Ty::Func(params, ret) => (params, *ret),
                other => {
                    return Err(TypeError {
                        message: format!(
                            "`{}` is not callable (got {})",
                            new_name,
                            self.ty_name(&other)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
            };

            let typed_args = self.typecheck_user_function_args(span, new_uid, &params, args)?;
            let expected_self_ty = Ty::Struct(id.name.clone(), def.fields.clone());
            if !self.types_compatible(&expected_self_ty, &ret_ty) {
                return Err(TypeError {
                    message: format!(
                        "`{}` must return Self ({}), got {}",
                        new_name,
                        self.ty_name(&expected_self_ty),
                        self.ty_name(&ret_ty)
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }

            return Ok(TypedNode {
                ty: ret_ty.clone(),
                span: span.clone(),
                node: TypedInner::App(
                    Box::new(TypedNode {
                        ty: new_ty,
                        span: id.span.clone(),
                        node: TypedInner::Var(ResolvedId {
                            name: new_name,
                            qualified_name: None,
                            unique_id: new_uid,
                            span: id.span.clone(),
                        }),
                    }),
                    typed_args,
                ),
            });
        }

        if !matches!(
            def.kind,
            crate::env::TypeKind::Record | crate::env::TypeKind::Error
        ) {
            return Err(TypeError {
                message: format!("{} is not a constructor-call type", id.name),
                span: span.clone(),
                hint: None,
            });
        }

        let tag = def.tag;
        let mut typed_fields = vec![None; def.fields.len()];

        let all_positional = args
            .iter()
            .all(|a| matches!(a, ResolvedRecordLitArg::Positional(_)));
        let all_named = args
            .iter()
            .all(|a| matches!(a, ResolvedRecordLitArg::Named(_, _)));

        if all_positional {
            if args.len() != def.fields.len() {
                return Err(TypeError {
                    message: format!(
                        "{} expects {} field(s), got {}",
                        id.name,
                        def.fields.len(),
                        args.len()
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }
            for (i, arg) in args.iter().enumerate() {
                if let ResolvedRecordLitArg::Positional(expr) = arg {
                    let typed_val = self.check_node(expr)?;
                    let (_, def_ty) = &def.fields[i];
                    if !self.types_compatible(def_ty, &typed_val.ty) {
                        return Err(TypeError {
                            message: format!(
                                "Field '{}': expected {}, got {}",
                                def.fields[i].0,
                                self.ty_name(def_ty),
                                self.ty_name(&typed_val.ty)
                            ),
                            span: typed_val.span.clone(),
                            hint: None,
                        });
                    }
                    typed_fields[i] = Some(typed_val);
                }
            }
        } else if all_named {
            let mut seen = HashSet::new();
            for arg in args {
                if let ResolvedRecordLitArg::Named(name, expr) = arg {
                    if !seen.insert(name.clone()) {
                        return Err(TypeError {
                            message: format!("Duplicate field '{}' in {}", name, id.name),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    let idx = def
                        .fields
                        .iter()
                        .position(|(n, _)| n == name)
                        .ok_or_else(|| TypeError {
                            message: format!("Unknown field '{}' in {}", name, id.name),
                            span: span.clone(),
                            hint: None,
                        })?;
                    let typed_val = self.check_node(expr)?;
                    let (_, def_ty) = &def.fields[idx];
                    if !self.types_compatible(def_ty, &typed_val.ty) {
                        return Err(TypeError {
                            message: format!(
                                "Field '{}': expected {}, got {}",
                                name,
                                self.ty_name(def_ty),
                                self.ty_name(&typed_val.ty)
                            ),
                            span: typed_val.span.clone(),
                            hint: None,
                        });
                    }
                    typed_fields[idx] = Some(typed_val);
                }
            }
        } else {
            return Err(TypeError {
                message: "Cannot mix positional and named arguments".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let final_fields: Vec<TypedNode> = typed_fields
            .into_iter()
            .enumerate()
            .map(|(i, f)| {
                f.ok_or_else(|| TypeError {
                    message: format!("Missing field '{}' in {}", def.fields[i].0, id.name),
                    span: span.clone(),
                    hint: None,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let result_ty = match def.kind {
            crate::env::TypeKind::Record => Ty::Record(id.name.clone(), def.fields.clone()),
            crate::env::TypeKind::Error => Ty::Error,
            crate::env::TypeKind::Struct | crate::env::TypeKind::Enum => {
                unreachable!("validated above")
            }
        };
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::ConstructorCall(tag, final_fields),
        })
    }

    pub(super) fn check_deferror_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        fields: &[ResolvedField],
        show_expr: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let ty_fields: Vec<(Ty, ResolvedId)> = fields
            .iter()
            .map(|f| {
                let ty = self.resolve_ast_ty_in_context(&f.ty, TypeSyntaxContext::General)?;
                let id = f.id.clone().ok_or_else(|| TypeError {
                    message: format!("Missing resolved field id for {}", f.name),
                    span: f.span.clone(),
                    hint: None,
                })?;
                Ok((ty, id))
            })
            .collect::<Result<Vec<_>, TypeError>>()?;

        let tag = self
            .env
            .resolve_type_def_signature(
                &id.name,
                ty_fields
                    .iter()
                    .map(|(ty, rid)| (rid.name.clone(), ty.clone()))
                    .collect(),
                fields
                    .iter()
                    .filter(|field| field.visibility == spire::ast::Visibility::Private)
                    .map(|field| field.name.clone())
                    .collect(),
            )
            .ok_or_else(|| TypeError {
                message: format!("Unknown error type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

        let mut show_env = self.env.clone();
        let typed_params: Vec<TypedFunParam> = ty_fields
            .iter()
            .map(|(ty, resolved_id)| {
                show_env.bind_var(resolved_id.unique_id, ty.clone());
                TypedFunParam {
                    id: resolved_id.clone(),
                    ty: ty.clone(),
                }
            })
            .collect();

        let fun_idx = match self.env.lookup_var(id.unique_id) {
            Some(Ty::UserFunc { fun_idx, .. }) => *fun_idx,
            _ => {
                return Err(TypeError {
                    message: format!("Undefined function: {}", id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
        };
        self.env.bind_var(
            id.unique_id,
            Ty::UserFunc {
                fun_idx,
                type_params: Vec::new(),
                params: typed_params.iter().map(|p| p.ty.clone()).collect(),
                ret: Box::new(Ty::Error),
            },
        );
        self.env.register_error_constructor(id.unique_id);

        for (ty, resolved_id) in &ty_fields {
            show_env.bind_var(resolved_id.unique_id, ty.clone());
        }
        let mut show_checker = self.spawn_child_checker(show_env);
        show_checker.function_return_ty = Some(Ty::Str);
        let typed_show = show_checker
            .check_node(show_expr)
            .map_err(|err| TypeError {
                message: err.message,
                span: err.span,
                hint: err.hint,
            })?;
        let typed_show = show_checker.resolve_typed_node(typed_show);
        self.absorb_child_progress(&show_checker);
        if !self.types_compatible(&Ty::Str, &typed_show.ty) {
            return Err(TypeError {
                message: format!(
                    "deferror show block must return String, got {}",
                    self.ty_name(&typed_show.ty)
                ),
                span: typed_show.span.clone(),
                hint: None,
            });
        }

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::DeferrorDef(
                tag,
                fun_idx,
                id.clone(),
                typed_params,
                Box::new(typed_show),
            ),
        })
    }

    pub(super) fn is_concrete_error_value(&self, node: &TypedNode) -> bool {
        match &node.node {
            TypedInner::Var(id) => self.env.is_error_constructor(id.unique_id),
            TypedInner::App(func, _) => match &func.node {
                TypedInner::Var(id) => self.env.is_error_constructor(id.unique_id),
                _ => false,
            },
            _ => false,
        }
    }

    pub(super) fn ensure_guard_error_value(
        &self,
        node: &TypedNode,
        form_name: &str,
    ) -> Result<(), TypeError> {
        if !matches!(node.ty, Ty::Error) {
            return Err(TypeError {
                message: format!(
                    "{} error branch must evaluate to Error, got {}",
                    form_name,
                    self.ty_name(&node.ty)
                ),
                span: node.span.clone(),
                hint: None,
            });
        }
        if !self.is_concrete_error_value(node) {
            return Err(TypeError {
                message: format!(
                    "{} error branch must be a concrete deferror value.",
                    form_name
                ),
                span: node.span.clone(),
                hint: Some("Use a deferror constructor or value, not a plain expression.".into()),
            });
        }
        Ok(())
    }
}
