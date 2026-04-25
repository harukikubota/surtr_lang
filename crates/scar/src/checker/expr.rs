use super::*;

impl Checker {
    pub(super) fn match_result_value_not_allowed_error(&self, span: &Span) -> TypeError {
        TypeError {
            message: "MatchResult values are extractor-only and can only be constructed inside extractor definitions"
                .into(),
            span: span.clone(),
            hint: Some(
                "Use MatchResult::Success, MatchResult::NoMatch, or MatchResult::Err only in a defextractor body. Ordinary APIs should return Result, Option, or a user-defined enum explicitly."
                    .into(),
            ),
        }
    }

    fn parse_standalone_tuple_root_index(name: &str) -> Option<usize> {
        let suffix = name.strip_prefix('_')?;
        if suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        suffix.parse::<usize>().ok()
    }

    fn parse_tuple_segment_index(field: &str) -> Option<usize> {
        let suffix = field.strip_prefix('_')?;
        if suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        suffix.parse::<usize>().ok()
    }

    pub(super) fn check_node(&mut self, node: &Resolved) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Lit(span, lit) => {
                let ty = self.lit_type(lit);
                Ok(TypedNode {
                    ty,
                    span: span.clone(),
                    node: TypedInner::Lit(lit.clone()),
                })
            }

            Resolved::Var(span, id) => {
                if id.qualified_name.as_ref().is_some_and(|qualified_name| {
                    self.trait_methods_by_qualified_name
                        .contains_key(qualified_name)
                }) {
                    return Err(TypeError {
                        message: format!(
                            "Trait helper `{}` cannot be referenced directly",
                            id.name
                        ),
                        span: span.clone(),
                        hint: Some(
                            "Call the helper with arguments so the receiver type can choose an impl."
                                .into(),
                        ),
                    });
                }

                if let Some(stored_ty) = self.env.lookup_var(id.unique_id).cloned() {
                    let ty = match &stored_ty {
                        Ty::BuiltinFunc { .. } | Ty::UserFunc { .. } => {
                            self.instantiate_builtin_ty(&stored_ty)
                        }
                        _ => self.resolve_ty(&stored_ty),
                    };
                    if matches!(ty, Ty::Lens(_, _)) {
                        if let Some(path) = self.lens_bindings.get(&id.unique_id).cloned() {
                            let source_ty = self.resolve_ty(&path.source_ty);
                            let focus_ty = self.resolve_ty(&path.focus_ty);
                            return Ok(TypedNode {
                                ty: Ty::Lens(
                                    Box::new(source_ty.clone()),
                                    Box::new(focus_ty.clone()),
                                ),
                                span: span.clone(),
                                node: TypedInner::LensPath(TypedLensPath {
                                    source_ty,
                                    focus_ty,
                                    may_fail: path.may_fail,
                                    segments: path.segments,
                                }),
                            });
                        }
                        return Err(TypeError {
                            message: "Lens value is not statically resolvable at this usage site"
                                .into(),
                            span: span.clone(),
                            hint: Some(
                                "Use a concrete path expression like User.name or pair._0.".into(),
                            ),
                        });
                    }
                    return Ok(TypedNode {
                        ty,
                        span: span.clone(),
                        node: TypedInner::Var(id.clone()),
                    });
                }

                if let Some(variant) = self.lookup_enum_variant_by_constructor_id(id.unique_id) {
                    let variant = self.instantiate_enum_variant(&variant);
                    if variant.enum_name == "MatchResult" && !self.in_extractor_body {
                        return Err(self.match_result_value_not_allowed_error(span));
                    }
                    if !variant.payload.is_empty() {
                        return Err(TypeError {
                            message: format!(
                                "Enum constructor {} expects {} argument(s)",
                                id.name,
                                variant.payload.len()
                            ),
                            span: span.clone(),
                            hint: Some("Call it as `Enum::Variant(...)`".into()),
                        });
                    }
                    let idx_node = TypedNode {
                        ty: Ty::Int,
                        span: span.clone(),
                        node: TypedInner::Lit(Lit::Int(variant.discriminant.clone())),
                    };
                    return Ok(TypedNode {
                        ty: self.resolve_ty(&variant.enum_ty),
                        span: span.clone(),
                        node: TypedInner::ConstructorCall(variant.tag, vec![idx_node]),
                    });
                }

                if let Some(index) = Self::parse_standalone_tuple_root_index(id.name.as_str()) {
                    return Err(TypeError {
                        message: format!(
                            "Standalone tuple root _{} is not allowed; use tuple access with ._{}",
                            index, index
                        ),
                        span: span.clone(),
                        hint: Some("Tuple elements are accessed as value._0, value._1, ...".into()),
                    });
                }

                Err(TypeError {
                    message: format!("Undefined variable: {}", id.name),
                    span: span.clone(),
                    hint: None,
                })
            }

            Resolved::TypeRefWitness(span, ast_ty) => Ok(TypedNode {
                ty: Ty::TypeRef(Box::new(
                    self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?,
                )),
                span: span.clone(),
                node: TypedInner::Lit(Lit::Unit),
            }),

            Resolved::Bind(span, pat, rhs) => {
                if !Self::is_total_bind_pattern(pat) {
                    return Err(TypeError {
                        message: "Only total MatchBlock patterns can be used with `=`".into(),
                        span: span.clone(),
                        hint: Some(
                            "Use `=?` for partial destructuring and extractor-driven matches."
                                .into(),
                        ),
                    });
                }
                let typed_rhs = if let ResolvedPattern::Annotated(_, ast_ty) = pat {
                    let expected =
                        self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())?;
                    self.check_node_with_expected(rhs, Some(&expected))?
                } else {
                    self.check_node(rhs)?
                };
                let lens_path = if matches!(typed_rhs.ty, Ty::Lens(_, _)) {
                    Some(self.resolve_lens_path_from_node(typed_rhs.clone(), span)?)
                } else {
                    None
                };
                if matches!(typed_rhs.ty, Ty::Error) {
                    return Err(TypeError {
                        message: "Error values must be wrapped with Err(...)".into(),
                        span: typed_rhs.span.clone(),
                        hint: None,
                    });
                }
                let (typed_pat, pat_ty) = self.check_pattern(pat, &typed_rhs.ty, span)?;
                self.ensure_self_rebinding_types(&typed_pat, span)?;

                self.bind_typed_pattern(&typed_pat, &self.resolve_ty(&pat_ty));
                if let Some(path) = &lens_path {
                    self.bind_lens_pattern_bindings(&typed_pat, path, span)?;
                } else {
                    self.clear_lens_pattern_bindings(&typed_pat);
                }
                self.normalize_env_bindings();

                Ok(TypedNode {
                    ty: Ty::Unit,
                    span: span.clone(),
                    node: TypedInner::Bind(typed_pat, Box::new(typed_rhs)),
                })
            }

            Resolved::SafeBind(span, pat, rhs) => self.check_safebind(span, pat, rhs),

            Resolved::App(span, func, args) => self.check_app(span, func, args),

            Resolved::BinOp(span, op, left, right) => self.check_binop(span, op, left, right),
            Resolved::Pipe(span, left, right) => self.check_pipe(span, left, right),
            Resolved::ContextMap(span, left, right) => self.check_context_map(span, left, right),
            Resolved::ContextBind(span, left, right) => self.check_context_bind(span, left, right),
            Resolved::Compose(span, left, right) => self.check_compose(span, left, right),
            Resolved::LiftedCompose(span, left, right) => {
                self.check_lifted_compose(span, left, right)
            }
            Resolved::KleisliCompose(span, left, right) => {
                self.check_kleisli_compose(span, left, right)
            }

            Resolved::ListNil(span) => self.check_list_nil(span),
            Resolved::ListCons(span, head, tail) => self.check_list_cons(span, head, tail),
            Resolved::ListLiteral(span, elems) => self.check_list_literal(span, elems),
            Resolved::TupleLiteral(span, elems) => self.check_tuple_literal(span, elems),
            Resolved::Grouped(span, inner) => {
                let mut typed = self.check_node(inner)?;
                typed.span = span.clone();
                Ok(typed)
            }

            Resolved::InterpolatedStr(span, parts) => self.check_interpolated_str(span, parts),

            Resolved::If(span, cond, then, else_opt) => self.check_if(span, cond, then, else_opt),
            Resolved::Assert(span, cond, err) => self.check_assert(span, cond, err),
            Resolved::Ensure(span, value, pred, err) => self.check_ensure(span, value, pred, err),
            Resolved::RecoverKind(span, value, marker, handler) => {
                self.check_recover_kind(span, value, marker, handler)
            }

            Resolved::Match(span, scrutinee, arms) => self.check_match(span, scrutinee, arms),

            Resolved::FieldAccess(span, expr, field) => self.check_field_access(span, expr, field),

            Resolved::Block(span, stmts) => {
                let mut typed_stmts = Vec::new();
                let mut last_ty = Ty::Unit;
                for s in stmts {
                    let t = self.check_node(s)?;
                    last_ty = t.ty.clone();
                    typed_stmts.push(t);
                }
                Ok(TypedNode {
                    ty: last_ty,
                    span: span.clone(),
                    node: TypedInner::Block(typed_stmts),
                })
            }

            Resolved::Semi(span, inner) => {
                let typed_inner = self.check_node(inner)?;
                Ok(TypedNode {
                    ty: Ty::Unit,
                    span: span.clone(),
                    node: TypedInner::Semi(Box::new(typed_inner)),
                })
            }

            Resolved::StructDef(span, id, fields) => self.check_struct_def(span, id, fields),
            Resolved::RecordDef(span, id, fields) => self.check_record_def(span, id, fields),
            Resolved::EnumDef(span, id, type_params, variants) => {
                self.check_enum_def(span, id, type_params, variants)
            }
            Resolved::StructLit(span, id, field_vals) => {
                self.check_struct_lit(span, id, field_vals)
            }
            Resolved::ConstructorCall(span, id, args) => {
                self.check_constructor_call(span, id, args)
            }
            Resolved::DeferrorDef(span, id, fields, show_expr) => {
                self.check_deferror_def(span, id, fields, show_expr)
            }
            Resolved::Def(span, id, type_params, params, ret_ty, body, attrs) => {
                self.check_def(span, id, type_params, params, ret_ty, body, attrs)
            }
            Resolved::ExtractorDef(span, id, type_params, param, ret_ty, body, attrs) => {
                self.check_extractor_def(span, id, type_params, param, ret_ty, body, attrs)
            }
            Resolved::TraitDef(span, id, _, methods, _) => Ok(TypedNode {
                ty: Ty::Unit,
                span: span.clone(),
                node: TypedInner::TraitDef(
                    self.trait_key(id),
                    methods
                        .iter()
                        .map(|method| method.id.name.clone())
                        .collect(),
                ),
            }),
            Resolved::TraitImplDef(span, trait_id, trait_args, target_ty, methods) => {
                self.check_trait_impl_def(span, trait_id, trait_args, target_ty, methods)
            }
            Resolved::BuiltinDecl(span, id, params, ret_ty, _) => {
                self.check_builtin_decl(span, id, params, ret_ty)
            }
            Resolved::BuiltinExtractorDecl(span, id, param, ret_ty, _) => {
                self.check_builtin_extractor_decl(span, id, param, ret_ty)
            }
            Resolved::BuiltinTypeDecl(span, id, params, attrs) => {
                self.check_builtin_type_decl(span, id, params, attrs)
            }
            Resolved::ResultCtorDecl(span, id, param_ty, ret_ty, attrs) => {
                self.check_result_ctor_decl(span, id, param_ty, ret_ty, attrs)
            }
            Resolved::Closure(span, params, captures, body) => {
                self.check_closure(span, params, captures, body, None)
            }
            Resolved::Capture(span, target, args) => self.check_capture(span, target, args),
        }
    }

    pub(super) fn check_node_with_expected(
        &mut self,
        node: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        match (node, expected) {
            (Resolved::Closure(span, params, captures, body), Some(expected_ty)) => {
                self.check_closure(span, params, captures, body, Some(expected_ty))
            }
            (Resolved::FieldAccess(span, expr, field), expected_ty) => {
                self.check_field_access_with_expected(span, expr, field, expected_ty)
            }
            (_, Some(expected_ty)) => {
                let typed = self.check_node(node)?;
                if matches!(expected_ty, Ty::Error) && self.is_concrete_error_value(&typed) {
                    let call_span = typed.span.clone();
                    return Ok(self.maybe_call_zero_arg_function(typed, call_span));
                }
                Ok(typed)
            }
            _ => self.check_node(node),
        }
    }

    fn bind_lens_pattern_bindings(
        &mut self,
        pattern: &TypedPattern,
        path: &TypedLensPath,
        span: &Span,
    ) -> Result<(), TypeError> {
        match pattern {
            TypedPattern::Var(_, id) => {
                self.lens_bindings.insert(id.unique_id, path.clone());
                Ok(())
            }
            TypedPattern::As(_, inner, alias) => {
                self.bind_lens_pattern_bindings(inner, path, span)?;
                self.lens_bindings.insert(alias.unique_id, path.clone());
                Ok(())
            }
            TypedPattern::Wildcard(_) => Ok(()),
            _ => Err(TypeError {
                message: "Lens values can only be bound to variables or `_` patterns".into(),
                span: span.clone(),
                hint: Some("Use `lens = User.name` or `_ = User.name`.".into()),
            }),
        }
    }

    fn clear_lens_pattern_bindings(&mut self, pattern: &TypedPattern) {
        match pattern {
            TypedPattern::Var(_, id) => {
                self.lens_bindings.remove(&id.unique_id);
            }
            TypedPattern::As(_, inner, alias) => {
                self.clear_lens_pattern_bindings(inner);
                self.lens_bindings.remove(&alias.unique_id);
            }
            TypedPattern::ListCons(_, head, tail) => {
                self.clear_lens_pattern_bindings(head);
                self.clear_lens_pattern_bindings(tail);
            }
            TypedPattern::Tuple(_, items) => {
                for item in items {
                    self.clear_lens_pattern_bindings(item);
                }
            }
            TypedPattern::ResultOk(_, inner) => self.clear_lens_pattern_bindings(inner),
            TypedPattern::Extractor { items, .. } => {
                for item in items {
                    self.clear_lens_pattern_bindings(item);
                }
            }
            TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _) => {}
        }
    }

    pub(super) fn check_safebind(
        &mut self,
        span: &Span,
        pat: &ResolvedPattern,
        rhs: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_rhs = self.check_node(rhs)?;
        if matches!(typed_rhs.ty, Ty::Lens(_, _)) {
            return Err(TypeError {
                message: "Lens values cannot be bound with `=?`".into(),
                span: typed_rhs.span.clone(),
                hint: Some("Use `=` for compile-time Lens bindings.".into()),
            });
        }
        let rhs_ty = self.resolve_ty(&typed_rhs.ty);
        if matches!(&rhs_ty, Ty::Enum(name, _) if name == "Option") {
            return Err(TypeError {
                message: "Option is not a SafeBind target; `=?` propagates Result-style failures, not optional values.".into(),
                span: typed_rhs.span.clone(),
                hint: Some(
                    "Convert explicitly with Option::to_result(value, err) before using `=?`."
                        .into(),
                ),
            });
        }
        let pattern_can_nomatch = !Self::is_total_bind_pattern(pat);
        let (ok_ty, mut propagated_err_tys) = match rhs_ty {
            Ty::Result(ok, err) => {
                let mut err_tys = vec![err.as_ref().clone()];
                if pattern_can_nomatch {
                    err_tys.push(Ty::Error);
                }
                (ok.as_ref().clone(), err_tys)
            }
            other => {
                let err_tys = if pattern_can_nomatch {
                    vec![Ty::Error]
                } else {
                    Vec::new()
                };
                (other, err_tys)
            }
        };

        let (typed_pat, pat_ty) = self.check_pattern(pat, &ok_ty, span)?;
        self.ensure_self_rebinding_types(&typed_pat, span)?;
        if let Some(ret_ty) = self.function_return_ty.clone() {
            let fn_err_ty = match ret_ty {
                Ty::Result(_, fn_err_ty) => fn_err_ty,
                other => {
                    return Err(TypeError {
                        message: format!(
                            "`=?` can only be used in functions returning Result<...>, got {}",
                            self.ty_name(&other)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
            };

            self.collect_pattern_result_error_types(&typed_pat, &mut propagated_err_tys);

            for propagated in propagated_err_tys {
                if !self.types_compatible(fn_err_ty.as_ref(), &propagated) {
                    return Err(TypeError {
                        message: format!(
                            "`=?` error type mismatch: function returns {}, but expression returns {}",
                            self.ty_name(fn_err_ty.as_ref()),
                            self.ty_name(&propagated)
                        ),
                        span: typed_rhs.span.clone(),
                        hint: None,
                    });
                }
            }
        }

        self.bind_typed_pattern(&typed_pat, &pat_ty);

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::SafeBind(typed_pat, Box::new(typed_rhs)),
        })
    }

    pub(super) fn check_compose_callable(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Capture(_, _, _)
            | Resolved::Closure(_, _, _, _)
            | Resolved::Compose(_, _, _)
            | Resolved::LiftedCompose(_, _, _)
            | Resolved::KleisliCompose(_, _, _) => self.check_node(node),
            Resolved::Var(_, _) | Resolved::Grouped(_, _) => {
                self.check_function_value_operand(node, op_name)
            }
            _ => Err(TypeError {
                message: format!(
                    "{} requires a function value",
                    op_name
                ),
                span: self.resolved_span(node).clone(),
                hint: Some("Use `&f`, a closure, a function-typed variable, a grouped function-valued expression, or another compose expression.".into()),
            }),
        }
    }

    pub(super) fn check_operator_compose_callable(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Capture(_, _, _)
            | Resolved::Closure(_, _, _, _)
            | Resolved::Compose(_, _, _)
            | Resolved::LiftedCompose(_, _, _)
            | Resolved::KleisliCompose(_, _, _) => self.check_node(node),
            Resolved::Var(_, _) | Resolved::Grouped(_, _) => {
                self.check_function_value_operand(node, op_name)
            }
            _ => Err(TypeError {
                message: format!("{} requires a function value", op_name),
                span: self.resolved_span(node).clone(),
                hint: Some("Use `&f`, a closure, a function-typed variable, or a parenthesized expression that evaluates to a function value.".into()),
            }),
        }
    }

    pub(super) fn check_apply_callable(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Capture(_, _, _) | Resolved::Closure(_, _, _, _) => self.check_node(node),
            Resolved::Var(_, _) | Resolved::Grouped(_, _) => {
                self.check_function_value_operand(node, op_name)
            }
            Resolved::App(span, func, args) => self.check_injected_call(span, func, args, op_name),
            _ => Err(TypeError {
                message: format!(
                    "{} requires a function value or a function call like `f(...)`",
                    op_name
                ),
                span: self.resolved_span(node).clone(),
                hint: Some("Use `&f`, a closure, a function-typed variable, or wrap a callable-returning call in parentheses.".into()),
            }),
        }
    }

    pub(super) fn check_function_value_operand(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        let typed = self.check_node(node)?;
        if matches!(self.resolve_ty(&typed.ty), Ty::Func(_, _)) {
            Ok(typed)
        } else {
            Err(TypeError {
                message: format!("{} requires a function value", op_name),
                span: typed.span,
                hint: Some("Bare function names are not function values; use `&name`, a closure, a function-typed variable, or `(call_returning_function(...))`.".into()),
            })
        }
    }

    pub(super) fn resolved_span<'a>(&self, node: &'a Resolved) -> &'a Span {
        match node {
            Resolved::Lit(span, _)
            | Resolved::Var(span, _)
            | Resolved::App(span, _, _)
            | Resolved::Block(span, _)
            | Resolved::Bind(span, _, _)
            | Resolved::SafeBind(span, _, _)
            | Resolved::BinOp(span, _, _, _)
            | Resolved::Pipe(span, _, _)
            | Resolved::ContextMap(span, _, _)
            | Resolved::ContextBind(span, _, _)
            | Resolved::Compose(span, _, _)
            | Resolved::LiftedCompose(span, _, _)
            | Resolved::KleisliCompose(span, _, _)
            | Resolved::ListNil(span)
            | Resolved::ListCons(span, _, _)
            | Resolved::ListLiteral(span, _)
            | Resolved::TupleLiteral(span, _)
            | Resolved::Grouped(span, _)
            | Resolved::InterpolatedStr(span, _)
            | Resolved::If(span, _, _, _)
            | Resolved::Assert(span, _, _)
            | Resolved::Ensure(span, _, _, _)
            | Resolved::RecoverKind(span, _, _, _)
            | Resolved::Match(span, _, _)
            | Resolved::FieldAccess(span, _, _)
            | Resolved::StructLit(span, _, _)
            | Resolved::ConstructorCall(span, _, _)
            | Resolved::TypeRefWitness(span, _)
            | Resolved::StructDef(span, _, _)
            | Resolved::RecordDef(span, _, _)
            | Resolved::DeferrorDef(span, _, _, _)
            | Resolved::EnumDef(span, _, _, _)
            | Resolved::Def(span, _, _, _, _, _, _)
            | Resolved::ExtractorDef(span, _, _, _, _, _, _)
            | Resolved::BuiltinDecl(span, _, _, _, _)
            | Resolved::BuiltinExtractorDecl(span, _, _, _, _)
            | Resolved::BuiltinTypeDecl(span, _, _, _)
            | Resolved::ResultCtorDecl(span, _, _, _, _)
            | Resolved::TraitDef(span, _, _, _, _)
            | Resolved::TraitImplDef(span, _, _, _, _)
            | Resolved::Closure(span, _, _, _)
            | Resolved::Capture(span, _, _)
            | Resolved::Semi(span, _) => span,
        }
    }

    pub(super) fn function_parts<'a>(&'a self, ty: &'a Ty) -> Option<(&'a [Ty], &'a Ty)> {
        match ty {
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                Some((params.as_slice(), ret.as_ref()))
            }
            Ty::Func(params, ret) => Some((params.as_slice(), ret.as_ref())),
            _ => None,
        }
    }

    pub(super) fn unary_function_parts(
        &self,
        ty: &Ty,
        op_name: &str,
        span: &Span,
    ) -> Result<(Ty, Ty), TypeError> {
        let Some((params, ret)) = self.function_parts(ty) else {
            return Err(TypeError {
                message: format!("{} expects a function value", op_name),
                span: span.clone(),
                hint: None,
            });
        };
        if params.len() != 1 {
            return Err(TypeError {
                message: format!("{} expects a unary callable", op_name),
                span: span.clone(),
                hint: None,
            });
        }
        Ok((self.resolve_ty(&params[0]), self.resolve_ty(ret)))
    }

    pub(super) fn typed_function_var_by_name(
        &mut self,
        name: &str,
        span: &Span,
    ) -> Result<TypedNode, TypeError> {
        let id = self
            .function_ids_by_name
            .get(name)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Missing helper function: {}", name),
                span: span.clone(),
                hint: None,
            })?;
        let ty = self
            .env
            .lookup_var(id.unique_id)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Missing helper function type: {}", name),
                span: span.clone(),
                hint: None,
            })?;
        let ty = match &ty {
            Ty::BuiltinFunc { .. } | Ty::UserFunc { .. } => self.instantiate_builtin_ty(&ty),
            _ => self.resolve_ty(&ty),
        };
        Ok(TypedNode {
            ty,
            span: span.clone(),
            node: TypedInner::Var(ResolvedId {
                span: span.clone(),
                ..id
            }),
        })
    }

    pub(super) fn build_typed_app(
        &mut self,
        span: &Span,
        func: TypedNode,
        args: Vec<TypedNode>,
    ) -> Result<TypedNode, TypeError> {
        let (params, ret) = match self.resolve_ty(&func.ty) {
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                (params, ret.as_ref().clone())
            }
            Ty::Func(params, ret) => (params, ret.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!("Not a function: {}", self.ty_name(&other)),
                    span: span.clone(),
                    hint: None,
                })
            }
        };
        if params.len() != args.len() {
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
        for (expected, arg) in params.iter().zip(&args) {
            if !self.types_compatible(expected, &arg.ty) {
                return Err(TypeError {
                    message: format!(
                        "Argument type mismatch: expected {}, got {}",
                        self.ty_name(expected),
                        self.ty_name(&arg.ty)
                    ),
                    span: arg.span.clone(),
                    hint: None,
                });
            }
        }
        self.ensure_no_runtime_lens_args(&args, span, "Function application")?;
        Ok(TypedNode {
            ty: self.resolve_ty(&ret),
            span: span.clone(),
            node: TypedInner::App(Box::new(func), args),
        })
    }

    pub(super) fn trait_method_ref<'a>(
        &self,
        func: &'a Resolved,
    ) -> Option<(&'a ResolvedId, String, String)> {
        let Resolved::Var(_, id) = func else {
            return None;
        };
        let qualified_name = id.qualified_name.as_ref()?;
        let (trait_name, method_name) = self
            .trait_methods_by_qualified_name
            .get(qualified_name)?
            .clone();
        Some((id, trait_name, method_name))
    }

    pub(super) fn trait_dispatch_target(
        &self,
        trait_name: &str,
        method_name: &str,
        receiver_ty: &Ty,
    ) -> Option<TraitDispatch> {
        let receiver_ty = self.resolve_ty(receiver_ty);
        match receiver_ty {
            Ty::Var(var) => {
                if self.tyvar_has_bound(var, trait_name)
                    || self.tyvar_satisfies_compiler_trait(var, trait_name)
                {
                    Some(TraitDispatch::Pending)
                } else {
                    None
                }
            }
            concrete => {
                if let Some(target_name) = self.trait_target_name(&concrete) {
                    if let Some(impl_info) = self
                        .trait_impls
                        .get(&(trait_name.into(), target_name.clone()))
                    {
                        let method = impl_info.methods.get(method_name)?;

                        if let Some(dispatch_override) = &method.dispatch_override {
                            return Some(TraitDispatch::Static(dispatch_override.clone()));
                        }
                        let function_key = method
                            .function_id
                            .qualified_name
                            .as_ref()
                            .unwrap_or(&method.function_id.name);
                        let function_id = self.function_ids_by_name.get(function_key)?;
                        let function_ty = self.env.lookup_var(function_id.unique_id)?;
                        let Ty::UserFunc { fun_idx, .. } = function_ty else {
                            return None;
                        };
                        return Some(TraitDispatch::Static(TraitDispatchTarget::UserFunction {
                            name: method.function_id.name.clone(),
                            fun_idx: *fun_idx,
                        }));
                    }
                }
                self.compiler_trait_dispatch_target(trait_name, method_name, &concrete)
                    .map(TraitDispatch::Static)
            }
        }
    }

    fn opposite_conversion_hint(
        &self,
        trait_name: &str,
        method_name: &str,
        receiver_ty: &Ty,
        typed_args: &[TypedNode],
        span: &Span,
    ) -> Option<TypeError> {
        let requested_trait = if self.trait_matches_short_name(trait_name, "From") {
            "From"
        } else if self.trait_matches_short_name(trait_name, "TryFrom") {
            "TryFrom"
        } else {
            return None;
        };
        let opposite_trait = if requested_trait == "From" {
            "TryFrom"
        } else {
            "From"
        };
        let receiver_name = self.trait_target_name(receiver_ty)?;
        let witness_ty = self.resolve_ty(&typed_args.get(1)?.ty);
        let Ty::TypeRef(target_ty) = witness_ty else {
            return None;
        };
        let opposite_trait_key = self.trait_key_by_short_name(opposite_trait)?;
        let opposite_instance_key =
            self.trait_instance_key_from_tys(&opposite_trait_key, &[target_ty.as_ref().clone()]);
        if !self
            .trait_impls
            .contains_key(&(opposite_instance_key, receiver_name.clone()))
        {
            return None;
        }

        let target_name = self.ty_name(&target_ty);
        let opposite_method = if opposite_trait == "From" {
            "from"
        } else {
            "try_from"
        };
        Some(TypeError {
            message: format!(
                "{} -> {} implements {}, not {}. Use {}(value, {}).",
                receiver_name,
                target_name,
                opposite_trait,
                requested_trait,
                opposite_method,
                target_name
            ),
            span: span.clone(),
            hint: Some(format!(
                "{}::{} is not available for this conversion pair.",
                self.trait_display_name(trait_name),
                method_name
            )),
        })
    }

    pub(super) fn check_trait_method_call(
        &mut self,
        span: &Span,
        trait_name: &str,
        method_name: &str,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: format!(
                    "{}::{} does not accept named arguments",
                    trait_name, method_name
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let trait_info = self
            .traits
            .get(trait_name)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Unknown trait: {}", trait_name),
                span: span.clone(),
                hint: None,
            })?;
        let method = trait_info
            .methods
            .get(method_name)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Unknown trait method: {}::{}", trait_name, method_name),
                span: span.clone(),
                hint: None,
            })?;

        let self_ty = self.env.fresh_tyvar();
        let (param_tys, ret_ty, trait_arg_tys) =
            self.resolve_trait_method_signature(&trait_info, &method, &self_ty)?;

        let trait_display_name = self.trait_display_name(trait_name);
        let trait_impl_summary = self.trait_implementation_summary(trait_name);

        if args.len() != param_tys.len() {
            return Err(TypeError {
                message: format!(
                    "{}::{} expects {} argument(s), got {}",
                    trait_name,
                    method_name,
                    param_tys.len(),
                    args.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_args = args
            .iter()
            .zip(param_tys.iter())
            .map(|(arg, expected)| match arg {
                ResolvedRecordLitArg::Positional(expr) => {
                    self.check_node_with_expected(expr, Some(expected))
                }
                ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.ensure_no_runtime_lens_args(&typed_args, span, "Trait method call")?;

        for (idx, (expected, arg)) in param_tys.iter().zip(&typed_args).enumerate() {
            if !self.types_compatible(expected, &arg.ty) {
                if typed_args.len() == 2 {
                    let left_ty = self.ty_name(&typed_args[0].ty);
                    let right_ty = self.ty_name(&typed_args[1].ty);
                    if self.trait_matches_short_name(trait_name, "Eq")
                        || self.trait_matches_short_name(trait_name, "Ord")
                    {
                        return Err(TypeError {
                            message: format!(
                                "Cannot compare {} and {}. {}",
                                left_ty, right_ty, trait_impl_summary
                            ),
                            span: arg.span.clone(),
                            hint: None,
                        });
                    }
                    if self.trait_matches_short_name(trait_name, "Concat") {
                        return Err(TypeError {
                            message: format!(
                                "++ requires (String, String), got ({}, {}). {}",
                                left_ty, right_ty, trait_impl_summary
                            ),
                            span: arg.span.clone(),
                            hint: None,
                        });
                    }
                }
                let receiver_ty = self.resolve_ty(&self_ty);
                if !matches!(receiver_ty, Ty::Var(_))
                    && self.trait_impl_exists(trait_name, &receiver_ty)
                {
                    return Err(TypeError {
                        message: format!(
                            "{}::{} expects argument {} to match receiver type {}, got {}. {}",
                            trait_display_name,
                            method_name,
                            idx + 1,
                            self.ty_name(&receiver_ty),
                            self.ty_name(&arg.ty),
                            trait_impl_summary
                        ),
                        span: arg.span.clone(),
                        hint: None,
                    });
                }
                return Err(TypeError {
                    message: format!(
                        "Argument type mismatch in {}::{}: expected {}, got {}. {}",
                        trait_display_name,
                        method_name,
                        self.ty_name(expected),
                        self.ty_name(&arg.ty),
                        trait_impl_summary
                    ),
                    span: arg.span.clone(),
                    hint: None,
                });
            }
        }

        let trait_call_name = self.trait_instance_key_from_tys(trait_name, &trait_arg_tys);
        let trait_call_display_name = self.trait_display_name(&trait_call_name);
        let trait_call_summary = self.trait_implementation_summary(&trait_call_name);
        let receiver_ty = self.resolve_ty(&self_ty);
        let receiver_span = typed_args
            .first()
            .map(|arg| arg.span.clone())
            .unwrap_or_else(|| span.clone());

        if let Some(err) = self.opposite_conversion_hint(
            &trait_call_name,
            method_name,
            &receiver_ty,
            &typed_args,
            &receiver_span,
        ) {
            if self
                .trait_dispatch_target(&trait_call_name, method_name, &receiver_ty)
                .is_none()
            {
                return Err(err);
            }
        }

        let dispatch = self
            .trait_dispatch_target(&trait_call_name, method_name, &receiver_ty)
            .ok_or_else(|| TypeError {
                message: format!(
                    "{}::{} requires a receiver type implementing {}, got {}. {}",
                    trait_call_display_name,
                    method_name,
                    trait_call_display_name,
                    self.ty_name(&receiver_ty),
                    trait_call_summary
                ),
                span: receiver_span,
                hint: None,
            })?;

        Ok(TypedNode {
            ty: self.resolve_ty(&ret_ty),
            span: span.clone(),
            node: TypedInner::TraitCall {
                trait_name: trait_call_name,
                method_name: method_name.into(),
                receiver_ty,
                dispatch,
                args: typed_args,
            },
        })
    }

    pub(super) fn check_injected_call(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: format!(
                    "{} does not support named arguments on the right-hand side",
                    op_name
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_func = self.check_node(func)?;
        let (params, ret) = match self.resolve_ty(&typed_func.ty) {
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                (params, ret.as_ref().clone())
            }
            Ty::Func(params, ret) => (params, ret.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!(
                        "{} right-hand side is not a function call target: {}",
                        op_name,
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: None,
                })
            }
        };

        if params.len() != args.len() + 1 {
            return Err(TypeError {
                message: format!(
                    "{} injects the left value as the first argument, so the call expects {} explicit argument(s), got {}",
                    op_name,
                    params.len().saturating_sub(1),
                    args.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_args: Vec<TypedNode> = args
            .iter()
            .zip(params.iter().skip(1))
            .map(|(arg, expected)| match arg {
                ResolvedRecordLitArg::Positional(expr) => {
                    self.check_node_with_expected(expr, Some(expected))
                }
                ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.ensure_no_runtime_lens_args(&typed_args, span, op_name)?;

        for (expected, arg) in params.iter().skip(1).zip(&typed_args) {
            if !self.types_compatible(expected, &arg.ty) {
                return Err(TypeError {
                    message: format!(
                        "Argument type mismatch: expected {}, got {}",
                        self.ty_name(expected),
                        self.ty_name(&arg.ty)
                    ),
                    span: arg.span.clone(),
                    hint: None,
                });
            }
        }

        Ok(TypedNode {
            ty: Ty::Func(
                vec![self.resolve_ty(&params[0])],
                Box::new(self.resolve_ty(&ret)),
            ),
            span: span.clone(),
            node: TypedInner::InjectCall(Box::new(typed_func), typed_args),
        })
    }

    pub(super) fn build_injected_app(
        &mut self,
        span: &Span,
        injected_value: TypedNode,
        callable: TypedNode,
    ) -> Result<TypedNode, TypeError> {
        let TypedInner::InjectCall(func, mut args) = callable.node else {
            return Err(TypeError {
                message: "internal error: expected injected call".into(),
                span: span.clone(),
                hint: None,
            });
        };
        let mut full_args = Vec::with_capacity(args.len() + 1);
        full_args.push(injected_value);
        full_args.append(&mut args);
        self.build_typed_app(span, *func, full_args)
    }

    pub(super) fn list_helper_ref_by_name(
        &mut self,
        helper_name: &str,
        span: &Span,
    ) -> Result<ListHelperRef, TypeError> {
        let helper = self.typed_function_var_by_name(helper_name, span)?;
        match helper.ty {
            Ty::UserFunc { fun_idx, .. } => Ok(ListHelperRef::User(fun_idx)),
            Ty::BuiltinFunc { ref name, .. } => {
                let builtin_id =
                    sindr::builtin::builtin_id_by_name(name).ok_or_else(|| TypeError {
                        message: format!("Unknown builtin helper: {}", helper_name),
                        span: span.clone(),
                        hint: None,
                    })?;
                Ok(ListHelperRef::Builtin(builtin_id))
            }
            _ => Err(TypeError {
                message: format!("{} must be a callable helper", helper_name),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn build_list_helper_call(
        &mut self,
        helper_name: &str,
        span: &Span,
        value: TypedNode,
        callable: TypedNode,
    ) -> Result<TypedNode, TypeError> {
        let helper = self.typed_function_var_by_name(helper_name, span)?;
        self.build_typed_app(span, helper, vec![value, callable])
    }

    pub(super) fn ensure_plain_map_output(
        &self,
        output_ty: &Ty,
        op_name: &str,
        span: &Span,
    ) -> Result<(), TypeError> {
        match self.resolve_ty(output_ty) {
            Ty::Result(_, _) | Ty::List(_) => Err(TypeError {
                message: format!(
                    "{} expects a plain function on the right-hand side; use `|>=` for contextual output",
                    op_name
                ),
                span: span.clone(),
                hint: None,
            }),
            _ => Ok(()),
        }
    }

    pub(super) fn check_pipe(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_node(left)?;
        let typed_right = self.check_apply_callable(right, "`|>`")?;
        let (param, ret) = self.unary_function_parts(&typed_right.ty, "`|>`", &typed_right.span)?;
        if !self.types_compatible(&param, &typed_left.ty) {
            return Err(TypeError {
                message: format!(
                    "`|>` type mismatch: expected {}, got {}",
                    self.ty_name(&param),
                    self.ty_name(&typed_left.ty)
                ),
                span: typed_left.span.clone(),
                hint: None,
            });
        }
        match typed_right.node {
            TypedInner::InjectCall(_, _) => self.build_injected_app(span, typed_left, typed_right),
            _ => Ok(TypedNode {
                ty: ret,
                span: span.clone(),
                node: TypedInner::Pipe(Box::new(typed_left), Box::new(typed_right)),
            }),
        }
    }

    pub(super) fn check_context_map(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_right = self.check_apply_callable(right, "`|*>`")?;
        let (rhs_in, rhs_out) =
            self.unary_function_parts(&typed_right.ty, "`|*>`", &typed_right.span)?;
        self.ensure_plain_map_output(&rhs_out, "`|*>`", &typed_right.span)?;

        let typed_left = self.check_node(left)?;
        match self.resolve_ty(&typed_left.ty) {
            Ty::Result(ok, err) => {
                if !self.types_compatible(ok.as_ref(), &rhs_in) {
                    return Err(TypeError {
                        message: format!(
                            "`|*>` type mismatch: expected {}, got {}",
                            self.ty_name(ok.as_ref()),
                            self.ty_name(&rhs_in)
                        ),
                        span: typed_right.span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedNode {
                    ty: Ty::Result(Box::new(rhs_out), Box::new(self.resolve_ty(err.as_ref()))),
                    span: span.clone(),
                    node: TypedInner::ResultMap(Box::new(typed_left), Box::new(typed_right)),
                })
            }
            Ty::List(item) => {
                if !self.types_compatible(item.as_ref(), &rhs_in) {
                    return Err(TypeError {
                        message: format!(
                            "`|*>` type mismatch: expected {}, got {}",
                            self.ty_name(item.as_ref()),
                            self.ty_name(&rhs_in)
                        ),
                        span: typed_right.span.clone(),
                        hint: None,
                    });
                }
                self.build_list_helper_call("List::map", span, typed_left, typed_right)
            }
            other => Err(TypeError {
                message: format!(
                    "`|*>` requires Result or List on the left, got {}",
                    self.ty_name(&other)
                ),
                span: typed_left.span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn check_context_bind(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_right = self.check_apply_callable(right, "`|>=`")?;
        let (rhs_in, rhs_ret) =
            self.unary_function_parts(&typed_right.ty, "`|>=`", &typed_right.span)?;

        let typed_left = self.check_node(left)?;
        match (self.resolve_ty(&typed_left.ty), self.resolve_ty(&rhs_ret)) {
            (Ty::Result(ok, err), Ty::Result(next_ok, next_err)) => {
                if !self.types_compatible(ok.as_ref(), &rhs_in)
                    || !self.types_compatible(err.as_ref(), next_err.as_ref())
                {
                    return Err(TypeError {
                        message: "`|>=` requires matching Result context on both sides".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedNode {
                    ty: Ty::Result(
                        Box::new(self.resolve_ty(next_ok.as_ref())),
                        Box::new(self.resolve_ty(err.as_ref())),
                    ),
                    span: span.clone(),
                    node: TypedInner::ResultBind(Box::new(typed_left), Box::new(typed_right)),
                })
            }
            (Ty::List(item), Ty::List(_)) => {
                if !self.types_compatible(item.as_ref(), &rhs_in) {
                    return Err(TypeError {
                        message: format!(
                            "`|>=` type mismatch: expected {}, got {}",
                            self.ty_name(item.as_ref()),
                            self.ty_name(&rhs_in)
                        ),
                        span: typed_right.span.clone(),
                        hint: None,
                    });
                }
                self.build_list_helper_call("List::flat_map", span, typed_left, typed_right)
            }
            (Ty::Result(_, _), Ty::List(_)) | (Ty::List(_), Ty::Result(_, _)) => Err(TypeError {
                message: "`|>=` cannot mix Result and List context".into(),
                span: span.clone(),
                hint: None,
            }),
            (other, _) => Err(TypeError {
                message: format!(
                    "`|>=` requires Result or List on the left, got {}",
                    self.ty_name(&other)
                ),
                span: typed_left.span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn check_compose(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_operator_compose_callable(left, "`>>`")?;
        let typed_right = self.check_operator_compose_callable(right, "`>>`")?;
        let (left_in, left_out) =
            self.unary_function_parts(&typed_left.ty, "`>>`", &typed_left.span)?;
        let (right_in, right_out) =
            self.unary_function_parts(&typed_right.ty, "`>>`", &typed_right.span)?;
        if !self.types_compatible(&left_out, &right_in) {
            return Err(TypeError {
                message: "`>>` requires the left output type to match the right input type".into(),
                span: span.clone(),
                hint: None,
            });
        }
        Ok(TypedNode {
            ty: Ty::Func(vec![left_in], Box::new(right_out)),
            span: span.clone(),
            node: TypedInner::Compose(
                ComposeFlavor::Plain,
                Box::new(typed_left),
                Box::new(typed_right),
            ),
        })
    }

    pub(super) fn check_lifted_compose(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_operator_compose_callable(left, "`>*`")?;
        let typed_right = self.check_operator_compose_callable(right, "`>*`")?;
        let (left_in, left_out) =
            self.unary_function_parts(&typed_left.ty, "`>*`", &typed_left.span)?;
        let (right_in, right_out) =
            self.unary_function_parts(&typed_right.ty, "`>*`", &typed_right.span)?;
        self.ensure_plain_map_output(&right_out, "`>*`", &typed_right.span)?;
        match self.resolve_ty(&left_out) {
            Ty::Result(ok, err) => {
                if !self.types_compatible(ok.as_ref(), &right_in) {
                    return Err(TypeError {
                        message:
                            "`>*` requires the left contextual output to match the right input type"
                                .into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedNode {
                    ty: Ty::Func(
                        vec![left_in],
                        Box::new(Ty::Result(
                            Box::new(self.resolve_ty(&right_out)),
                            Box::new(self.resolve_ty(err.as_ref())),
                        )),
                    ),
                    span: span.clone(),
                    node: TypedInner::Compose(
                        ComposeFlavor::ResultMap,
                        Box::new(typed_left),
                        Box::new(typed_right),
                    ),
                })
            }
            Ty::List(item) => {
                if !self.types_compatible(item.as_ref(), &right_in) {
                    return Err(TypeError {
                        message:
                            "`>*` requires the left contextual output to match the right input type"
                                .into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedNode {
                    ty: Ty::Func(
                        vec![left_in],
                        Box::new(Ty::List(Box::new(self.resolve_ty(&right_out)))),
                    ),
                    span: span.clone(),
                    node: TypedInner::Compose(
                        ComposeFlavor::ListMap {
                            helper: self.list_helper_ref_by_name("List::map", span)?,
                        },
                        Box::new(typed_left),
                        Box::new(typed_right),
                    ),
                })
            }
            _ => Err(TypeError {
                message: "`>*` requires Result or List on the left-hand side".into(),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn check_kleisli_compose(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_operator_compose_callable(left, "`>=>`")?;
        let typed_right = self.check_operator_compose_callable(right, "`>=>`")?;
        let (left_in, left_out) =
            self.unary_function_parts(&typed_left.ty, "`>=>`", &typed_left.span)?;
        let (right_in, right_out) =
            self.unary_function_parts(&typed_right.ty, "`>=>`", &typed_right.span)?;
        match (self.resolve_ty(&left_out), self.resolve_ty(&right_out)) {
            (Ty::Result(ok, err), Ty::Result(next_ok, next_err)) => {
                if !self.types_compatible(ok.as_ref(), &right_in)
                    || !self.types_compatible(err.as_ref(), next_err.as_ref())
                {
                    return Err(TypeError {
                        message: "`>=>` requires matching Result context on both sides".into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedNode {
                    ty: Ty::Func(
                        vec![left_in],
                        Box::new(Ty::Result(
                            Box::new(self.resolve_ty(next_ok.as_ref())),
                            Box::new(self.resolve_ty(err.as_ref())),
                        )),
                    ),
                    span: span.clone(),
                    node: TypedInner::Compose(
                        ComposeFlavor::ResultBind,
                        Box::new(typed_left),
                        Box::new(typed_right),
                    ),
                })
            }
            (Ty::List(item), Ty::List(next_item)) => {
                if !self.types_compatible(item.as_ref(), &right_in) {
                    return Err(TypeError {
                        message: "`>=>` requires matching List element types across both sides"
                            .into(),
                        span: span.clone(),
                        hint: None,
                    });
                }
                Ok(TypedNode {
                    ty: Ty::Func(
                        vec![left_in],
                        Box::new(Ty::List(Box::new(self.resolve_ty(next_item.as_ref())))),
                    ),
                    span: span.clone(),
                    node: TypedInner::Compose(
                        ComposeFlavor::ListBind {
                            helper: self.list_helper_ref_by_name("List::flat_map", span)?,
                        },
                        Box::new(typed_left),
                        Box::new(typed_right),
                    ),
                })
            }
            _ => Err(TypeError {
                message: "`>=>` requires matching Result or List context on both sides".into(),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn match_result_variant_tags(
        &self,
        span: &Span,
    ) -> Result<(u32, u32, u32), TypeError> {
        let variants = self
            .lookup_enum_variants_of("MatchResult")
            .ok_or_else(|| TypeError {
                message: "MatchResult enum is not available in the current environment".into(),
                span: span.clone(),
                hint: None,
            })?;
        let mut success_tag = None;
        let mut no_match_tag = None;
        let mut err_tag = None;
        for variant in variants {
            match variant.short_name.as_str() {
                "Success" => success_tag = Some(variant.tag),
                "NoMatch" => no_match_tag = Some(variant.tag),
                "Err" => err_tag = Some(variant.tag),
                _ => {}
            }
        }
        match (success_tag, no_match_tag, err_tag) {
            (Some(success), Some(no_match), Some(err)) => Ok((success, no_match, err)),
            _ => Err(TypeError {
                message: "MatchResult enum must define Success, NoMatch, and Err variants".into(),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn extractor_contract(
        &mut self,
        extractor_id: &ResolvedId,
        span: &Span,
    ) -> Result<(Ty, Vec<Ty>, u32, u32, u32), TypeError> {
        let extractor_ty = self
            .env
            .lookup_var(extractor_id.unique_id)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Undefined extractor: {}", extractor_id.name),
                span: span.clone(),
                hint: None,
            })?;
        let extractor_ty = self.instantiate_builtin_ty(&extractor_ty);
        let (params, ret) = match &extractor_ty {
            Ty::BuiltinFunc { params, ret, .. }
            | Ty::UserFunc { params, ret, .. }
            | Ty::Func(params, ret) => (params.clone(), ret.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!(
                        "Extractor {} is not callable (got {})",
                        extractor_id.name,
                        self.ty_name(other)
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }
        };
        if params.len() != 1 {
            return Err(TypeError {
                message: format!(
                    "Extractor {} must accept exactly one input value, got {} parameter(s)",
                    extractor_id.name,
                    params.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }
        let input_ty = params[0].clone();
        let seq_tys =
            self.require_match_result_seq_ty(&self.resolve_ty(&ret), span, &extractor_id.name)?;
        let (success_tag, no_match_tag, err_tag) = self.match_result_variant_tags(span)?;
        Ok((input_ty, seq_tys, success_tag, no_match_tag, err_tag))
    }

    pub(super) fn extractor_callable_ty(
        &mut self,
        extractor_id: &ResolvedId,
        span: &Span,
    ) -> Result<Ty, TypeError> {
        let extractor_ty = self
            .env
            .lookup_var(extractor_id.unique_id)
            .cloned()
            .ok_or_else(|| TypeError {
                message: format!("Undefined extractor: {}", extractor_id.name),
                span: span.clone(),
                hint: None,
            })?;
        Ok(self.instantiate_builtin_ty(&extractor_ty))
    }

    pub(super) fn kernel_uncons_id(&self, span: &Span) -> Result<ResolvedId, TypeError> {
        let id = self
            .function_ids_by_name
            .get("Kernel::uncons")
            .or_else(|| self.function_ids_by_name.get("uncons"))
            .cloned()
            .ok_or_else(|| TypeError {
                message: "Missing helper function: Kernel::uncons".into(),
                span: span.clone(),
                hint: None,
            })?;
        Ok(ResolvedId {
            span: span.clone(),
            ..id
        })
    }

    pub(super) fn uncons_contract_for_input(
        &self,
        observed_ty: &Ty,
        span: &Span,
    ) -> Result<(Ty, Vec<Ty>), TypeError> {
        match self.resolve_ty(observed_ty) {
            Ty::List(inner) => {
                let elem_ty = inner.as_ref().clone();
                let list_ty = Ty::List(Box::new(elem_ty.clone()));
                Ok((list_ty.clone(), vec![elem_ty, list_ty]))
            }
            Ty::Str => Ok((Ty::Str, vec![Ty::Str, Ty::Str])),
            other => Err(TypeError {
                message: format!(
                    "Extractor uncons expects List<...> or String, got {}",
                    self.ty_name(&other)
                ),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn extractor_contract_for_observed_ty(
        &mut self,
        extractor_id: &ResolvedId,
        observed_ty: &Ty,
        span: &Span,
    ) -> Result<(Ty, Ty, Vec<Ty>, u32, u32, u32), TypeError> {
        let extractor_ty = self.extractor_callable_ty(extractor_id, span)?;
        let (input_ty, seq_tys, success_tag, no_match_tag, err_tag) = if matches!(&extractor_ty, Ty::BuiltinFunc { name, .. } if name == "uncons")
        {
            let (input_ty, seq_tys) = self.uncons_contract_for_input(observed_ty, span)?;
            let (success_tag, no_match_tag, err_tag) = self.match_result_variant_tags(span)?;
            (input_ty, seq_tys, success_tag, no_match_tag, err_tag)
        } else {
            self.extractor_contract(extractor_id, span)?
        };
        Ok((
            input_ty,
            self.resolve_ty(&extractor_ty),
            seq_tys,
            success_tag,
            no_match_tag,
            err_tag,
        ))
    }

    pub(super) fn require_match_result_seq_ty(
        &self,
        ty: &Ty,
        span: &Span,
        context: &str,
    ) -> Result<Vec<Ty>, TypeError> {
        match self.resolve_ty(ty) {
            Ty::Enum(name, args) if name == "MatchResult" && args.len() == 1 => match &args[0] {
                Ty::Tuple(items) => Ok(items.clone()),
                other => Ok(vec![other.clone()]),
            },
            other => Err(TypeError {
                message: format!(
                    "{} must return MatchResult<T> or MatchResult<(...)>, got {}",
                    context,
                    self.ty_name(&other)
                ),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn typecheck_user_function_args(
        &mut self,
        span: &Span,
        callee_uid: u32,
        params: &[Ty],
        args: &[ResolvedRecordLitArg],
    ) -> Result<Vec<TypedNode>, TypeError> {
        let has_named = args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)));
        let has_positional = args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Positional(_)));
        if has_named && has_positional {
            return Err(TypeError {
                message: "Cannot mix positional and named arguments".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let param_names = self.user_func_params.get(&callee_uid).cloned();
        let mut typed_args = Vec::with_capacity(params.len());

        if has_named {
            let names = param_names.as_ref().ok_or_else(|| TypeError {
                message: "This function value does not accept named arguments".into(),
                span: span.clone(),
                hint: None,
            })?;

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

            let mut reordered: Vec<Option<&Resolved>> = vec![None; params.len()];
            for arg in args {
                let ResolvedRecordLitArg::Named(name, expr) = arg else {
                    unreachable!("validated argument form above")
                };
                let idx = names
                    .iter()
                    .position(|n| n == name)
                    .ok_or_else(|| TypeError {
                        message: format!("Unknown argument name '{}' for function", name),
                        span: span.clone(),
                        hint: None,
                    })?;
                if reordered[idx].is_some() {
                    return Err(TypeError {
                        message: format!("Duplicate argument '{}'", name),
                        span: span.clone(),
                        hint: None,
                    });
                }
                reordered[idx] = Some(expr);
            }

            for (idx, expected_ty) in params.iter().enumerate() {
                let expr = reordered[idx].ok_or_else(|| TypeError {
                    message: format!("Missing argument '{}'", names[idx]),
                    span: span.clone(),
                    hint: None,
                })?;
                let typed = self.check_node_with_expected(expr, Some(expected_ty))?;
                self.ensure_no_runtime_lens_value(&typed, "Function call arguments")?;
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
            return Ok(typed_args);
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

        for (expected_ty, arg) in params.iter().zip(args) {
            let ResolvedRecordLitArg::Positional(expr) = arg else {
                unreachable!("validated argument form above")
            };
            let typed = self.check_node_with_expected(expr, Some(expected_ty))?;
            self.ensure_no_runtime_lens_value(&typed, "Function call arguments")?;
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

        Ok(typed_args)
    }

    fn lens_intrinsic_kind(&self, func: &Resolved) -> Option<&'static str> {
        let Resolved::Var(_, id) = func else {
            return None;
        };
        if let Some(qualified_name) = id.qualified_name.as_deref() {
            return match qualified_name {
                "Lens::view" => Some("view"),
                "Lens::compose" => Some("compose"),
                "Lens::set" => Some("set"),
                "Lens::over" => Some("over"),
                _ => None,
            };
        }
        match id.name.as_str() {
            // Keep legacy fallback for Stage1 names.
            "view" => Some("view"),
            "compose" => Some("compose"),
            _ => None,
        }
    }

    fn resolve_lens_path_from_node(
        &self,
        typed: TypedNode,
        span: &Span,
    ) -> Result<TypedLensPath, TypeError> {
        if !matches!(typed.ty, Ty::Lens(_, _)) {
            return Err(TypeError {
                message: format!("Expected Lens<...> value, got {}", self.ty_name(&typed.ty)),
                span: typed.span.clone(),
                hint: None,
            });
        }
        match typed.node {
            TypedInner::LensPath(path) => Ok(TypedLensPath {
                source_ty: self.resolve_ty(&path.source_ty),
                focus_ty: self.resolve_ty(&path.focus_ty),
                may_fail: path.may_fail,
                segments: path.segments,
            }),
            _ => Err(TypeError {
                message:
                    "Lens values are compile-time only in Stage1 and cannot be stored or passed around"
                        .into(),
                span: span.clone(),
                hint: Some("Use type-root path expressions inline (e.g. User.name).".into()),
            }),
        }
    }

    fn check_lens_compose_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        if args.len() != 2 {
            return Err(TypeError {
                message: format!("Lens::compose expects 2 argument(s), got {}", args.len()),
                span: span.clone(),
                hint: None,
            });
        }
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: "Lens::compose does not accept named arguments".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let ResolvedRecordLitArg::Positional(left_expr) = &args[0] else {
            unreachable!("validated argument form above")
        };
        let ResolvedRecordLitArg::Positional(right_expr) = &args[1] else {
            unreachable!("validated argument form above")
        };

        let left = self.check_node(left_expr)?;
        let left_path = self.resolve_lens_path_from_node(left, span)?;

        let expected_right_focus = self.env.fresh_tyvar();
        let expected_right_ty = Ty::Lens(
            Box::new(self.resolve_ty(&left_path.focus_ty)),
            Box::new(expected_right_focus),
        );
        let right = self.check_node_with_expected(right_expr, Some(&expected_right_ty))?;
        let right_path = self.resolve_lens_path_from_node(right, span)?;

        if !self.types_compatible(&left_path.focus_ty, &right_path.source_ty) {
            return Err(TypeError {
                message: format!(
                    "Lens::compose source/focus mismatch: left focus is {}, right source is {}",
                    self.ty_name(&left_path.focus_ty),
                    self.ty_name(&right_path.source_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let source_ty = self.resolve_ty(&left_path.source_ty);
        let focus_ty = self.resolve_ty(&right_path.focus_ty);
        let mut segments = left_path.segments;
        segments.extend(right_path.segments);
        let path = TypedLensPath {
            source_ty: source_ty.clone(),
            focus_ty: focus_ty.clone(),
            may_fail: left_path.may_fail || right_path.may_fail,
            segments,
        };
        Ok(TypedNode {
            ty: Ty::Lens(Box::new(source_ty), Box::new(focus_ty)),
            span: span.clone(),
            node: TypedInner::LensPath(path),
        })
    }

    fn check_lens_source_value(
        &mut self,
        op_name: &str,
        source_expr: &Resolved,
    ) -> Result<(TypedNode, bool, Ty), TypeError> {
        let typed_source = self.check_node(source_expr)?;
        if matches!(typed_source.ty, Ty::Lens(_, _)) {
            return Err(TypeError {
                message: format!("{} source value cannot be a Lens", op_name),
                span: typed_source.span.clone(),
                hint: None,
            });
        }

        let (source_is_result, source_value_ty) = match self.resolve_ty(&typed_source.ty) {
            Ty::Result(ok, _) => (true, ok.as_ref().clone()),
            other => (false, other),
        };

        Ok((typed_source, source_is_result, source_value_ty))
    }

    fn check_lens_path_argument(
        &mut self,
        span: &Span,
        op_name: &str,
        path_expr: &Resolved,
        source_value_ty: &Ty,
        source_input_ty: &Ty,
    ) -> Result<TypedLensPath, TypeError> {
        let expected_focus_ty = self.env.fresh_tyvar();
        let expected_path_ty = Ty::Lens(
            Box::new(self.resolve_ty(source_value_ty)),
            Box::new(expected_focus_ty),
        );
        let path_node = self.check_node_with_expected(path_expr, Some(&expected_path_ty))?;
        let path = self.resolve_lens_path_from_node(path_node, span)?;

        if !self.types_compatible(&path.source_ty, source_value_ty) {
            return Err(TypeError {
                message: format!(
                    "{} source type mismatch: lens expects {}, got {}",
                    op_name,
                    self.ty_name(&path.source_ty),
                    self.ty_name(source_input_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        Ok(path)
    }

    fn check_lens_view_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        if args.len() != 2 {
            return Err(TypeError {
                message: format!("Lens::view expects 2 argument(s), got {}", args.len()),
                span: span.clone(),
                hint: None,
            });
        }
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: "Lens::view does not accept named arguments".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let ResolvedRecordLitArg::Positional(path_expr) = &args[0] else {
            unreachable!("validated argument form above")
        };
        let ResolvedRecordLitArg::Positional(source_expr) = &args[1] else {
            unreachable!("validated argument form above")
        };

        let (typed_source, source_is_result, source_value_ty) =
            self.check_lens_source_value("Lens::view", source_expr)?;
        let path = self.check_lens_path_argument(
            span,
            "Lens::view",
            path_expr,
            &source_value_ty,
            &typed_source.ty,
        )?;

        let focus_ty = self.resolve_ty(&path.focus_ty);
        let out_ty = if source_is_result || path.may_fail {
            Ty::Result(Box::new(focus_ty.clone()), Box::new(Ty::Error))
        } else {
            focus_ty
        };

        Ok(TypedNode {
            ty: out_ty,
            span: span.clone(),
            node: TypedInner::LensView {
                source: Box::new(typed_source),
                path,
                source_is_result,
            },
        })
    }

    fn check_lens_set_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        if args.len() != 3 {
            return Err(TypeError {
                message: format!("Lens::set expects 3 argument(s), got {}", args.len()),
                span: span.clone(),
                hint: None,
            });
        }
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: "Lens::set does not accept named arguments".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let ResolvedRecordLitArg::Positional(path_expr) = &args[0] else {
            unreachable!("validated argument form above")
        };
        let ResolvedRecordLitArg::Positional(source_expr) = &args[1] else {
            unreachable!("validated argument form above")
        };
        let ResolvedRecordLitArg::Positional(value_expr) = &args[2] else {
            unreachable!("validated argument form above")
        };

        let (typed_source, source_is_result, source_value_ty) =
            self.check_lens_source_value("Lens::set", source_expr)?;
        let path = self.check_lens_path_argument(
            span,
            "Lens::set",
            path_expr,
            &source_value_ty,
            &typed_source.ty,
        )?;

        let typed_value = self.check_node_with_expected(value_expr, Some(&path.focus_ty))?;
        if !self.types_compatible(&path.focus_ty, &typed_value.ty) {
            return Err(TypeError {
                message: format!(
                    "Lens::set value type mismatch: expected {}, got {}",
                    self.ty_name(&path.focus_ty),
                    self.ty_name(&typed_value.ty)
                ),
                span: typed_value.span.clone(),
                hint: None,
            });
        }

        Ok(TypedNode {
            ty: Ty::Result(
                Box::new(self.resolve_ty(&source_value_ty)),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::LensSet {
                source: Box::new(typed_source),
                path,
                value: Box::new(typed_value),
                source_is_result,
            },
        })
    }

    fn check_lens_over_intrinsic(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        if args.len() != 3 {
            return Err(TypeError {
                message: format!("Lens::over expects 3 argument(s), got {}", args.len()),
                span: span.clone(),
                hint: None,
            });
        }
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                message: "Lens::over does not accept named arguments".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let ResolvedRecordLitArg::Positional(path_expr) = &args[0] else {
            unreachable!("validated argument form above")
        };
        let ResolvedRecordLitArg::Positional(source_expr) = &args[1] else {
            unreachable!("validated argument form above")
        };
        let ResolvedRecordLitArg::Positional(update_expr) = &args[2] else {
            unreachable!("validated argument form above")
        };

        let (typed_source, source_is_result, source_value_ty) =
            self.check_lens_source_value("Lens::over", source_expr)?;
        let path = self.check_lens_path_argument(
            span,
            "Lens::over",
            path_expr,
            &source_value_ty,
            &typed_source.ty,
        )?;

        let typed_update = self.check_node(update_expr)?;
        let (in_ty, out_ty) = self.unary_function_parts(&typed_update.ty, "Lens::over", span)?;
        if !self.types_compatible(&path.focus_ty, &in_ty) {
            return Err(TypeError {
                message: format!(
                    "Lens::over update function input mismatch: expected {}, got {}",
                    self.ty_name(&path.focus_ty),
                    self.ty_name(&in_ty)
                ),
                span: typed_update.span.clone(),
                hint: None,
            });
        }

        let (out_ok, out_err) = match self.resolve_ty(&out_ty) {
            Ty::Result(ok, err) => (ok.as_ref().clone(), err.as_ref().clone()),
            _ => {
                return Err(TypeError {
                    message: format!(
                        "Lens::over update function must return Result<...>, got {}",
                        self.ty_name(&out_ty)
                    ),
                    span: typed_update.span.clone(),
                    hint: None,
                });
            }
        };
        if !self.types_compatible(&path.focus_ty, &out_ok) {
            return Err(TypeError {
                message: format!(
                    "Lens::over update function output mismatch: expected {}, got {}",
                    self.ty_name(&path.focus_ty),
                    self.ty_name(&out_ok)
                ),
                span: typed_update.span.clone(),
                hint: None,
            });
        }
        if !self.types_compatible(&Ty::Error, &out_err) {
            return Err(TypeError {
                message: format!(
                    "Lens::over update function error type must be Error-compatible, got {}",
                    self.ty_name(&out_err)
                ),
                span: typed_update.span.clone(),
                hint: None,
            });
        }

        Ok(TypedNode {
            ty: Ty::Result(
                Box::new(self.resolve_ty(&source_value_ty)),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::LensOver {
                source: Box::new(typed_source),
                path,
                update_fun: Box::new(typed_update),
                source_is_result,
            },
        })
    }

    fn try_check_lens_intrinsic_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        match self.lens_intrinsic_kind(func) {
            Some("view") => Ok(Some(self.check_lens_view_intrinsic(span, args)?)),
            Some("compose") => Ok(Some(self.check_lens_compose_intrinsic(span, args)?)),
            Some("set") => Ok(Some(self.check_lens_set_intrinsic(span, args)?)),
            Some("over") => Ok(Some(self.check_lens_over_intrinsic(span, args)?)),
            _ => Ok(None),
        }
    }

    fn ensure_no_runtime_lens_args(
        &self,
        args: &[TypedNode],
        span: &Span,
        callee: &str,
    ) -> Result<(), TypeError> {
        if args.iter().any(|arg| self.ty_contains_lens(&arg.ty)) {
            return Err(TypeError {
                message: format!(
                    "{} cannot accept Lens values in Stage1 (Lens is compile-time only)",
                    callee
                ),
                span: span.clone(),
                hint: Some("Apply Lens::view(...) before passing the value.".into()),
            });
        }
        Ok(())
    }

    fn ensure_no_runtime_lens_value(
        &self,
        value: &TypedNode,
        context: &str,
    ) -> Result<(), TypeError> {
        if self.ty_contains_lens(&value.ty) {
            return Err(TypeError {
                message: format!(
                    "{} cannot contain Lens values in Stage1 (Lens is compile-time only)",
                    context
                ),
                span: value.span.clone(),
                hint: Some(
                    "Consume Lens with Lens::view/set/over first, then pass the plain value."
                        .into(),
                ),
            });
        }
        Ok(())
    }

    pub(super) fn check_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        if let Some(typed) = self.try_check_lens_intrinsic_app(span, func, args)? {
            return Ok(typed);
        }

        if let Some((_id, trait_name, method_name)) = self.trait_method_ref(func) {
            return self.check_trait_method_call(span, &trait_name, &method_name, args);
        }

        let typed_func = self.check_node(func)?;
        let func_ty = self.resolve_ty(&typed_func.ty);

        match &func_ty {
            Ty::BuiltinFunc { name, params, ret } => {
                if args
                    .iter()
                    .any(|a| matches!(a, ResolvedRecordLitArg::Named(_, _)))
                {
                    return Err(TypeError {
                        message: format!("{} does not accept named arguments", name),
                        span: span.clone(),
                        hint: None,
                    });
                }

                if args.len() != params.len() {
                    return Err(TypeError {
                        message: format!(
                            "{} expects {} argument(s), got {}",
                            name,
                            params.len(),
                            args.len()
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let typed_args: Vec<TypedNode> = args
                    .iter()
                    .zip(params.iter())
                    .map(|(arg, expected)| match arg {
                        ResolvedRecordLitArg::Positional(expr) => {
                            self.check_node_with_expected(expr, Some(expected))
                        }
                        ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                for (param, arg) in params.iter().zip(&typed_args) {
                    if !self.types_compatible(param, &arg.ty) {
                        return Err(TypeError {
                            message: format!(
                                "Argument type mismatch: expected {}, got {}",
                                self.ty_name(param),
                                self.ty_name(&arg.ty)
                            ),
                            span: arg.span.clone(),
                            hint: None,
                        });
                    }
                }
                self.ensure_no_runtime_lens_args(&typed_args, span, name)?;

                if name == "set_exit_code" {
                    match self.runtime_policy.exit_code_policy {
                        ExitCodePolicy::Anywhere => {}
                        ExitCodePolicy::Forbidden => {
                            return Err(TypeError {
                                message: format!(
                                    "set_exit_code is forbidden by source policy ({})",
                                    self.runtime_policy.exit_code_policy.as_str()
                                ),
                                span: span.clone(),
                                hint: Some(
                                    "This source kind does not allow set_exit_code. Use Result-based failure handling instead."
                                        .into(),
                                ),
                            });
                        }
                        ExitCodePolicy::EntryOnly => {
                            let Some(entrypoint) =
                                self.runtime_policy.normalized_entrypoint.as_ref()
                            else {
                                return Err(TypeError {
                                    message:
                                        "set_exit_code requires a normalized entrypoint but none was provided".into(),
                                    span: span.clone(),
                                    hint: Some(
                                        "Configure an entrypoint, or avoid set_exit_code in this compile unit."
                                            .into(),
                                    ),
                                });
                            };
                            if self.current_function_symbol.as_deref() != Some(entrypoint.as_str())
                            {
                                return Err(TypeError {
                                    message: format!(
                                        "set_exit_code is only allowed inside entrypoint `{}` (policy: {})",
                                        entrypoint,
                                        self.runtime_policy.exit_code_policy.as_str()
                                    ),
                                    span: span.clone(),
                                    hint: Some(
                                        "Move set_exit_code into the configured entrypoint function."
                                            .into(),
                                    ),
                                });
                            }
                        }
                    }
                }

                Ok(TypedNode {
                    ty: self.resolve_ty(ret),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            Ty::UserFunc { params, ret, .. } => {
                let has_named = args
                    .iter()
                    .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)));
                let callee_uid = match func {
                    Resolved::Var(_, id) => id.unique_id,
                    _ if !has_named => u32::MAX,
                    _ => {
                        return Err(TypeError {
                            message: "This function value does not accept named arguments".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                };
                let typed_args =
                    self.typecheck_user_function_args(span, callee_uid, params, args)?;
                self.ensure_no_runtime_lens_args(&typed_args, span, "Function call")?;

                Ok(TypedNode {
                    ty: self.resolve_ty(ret),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            Ty::Func(params, ret) => {
                if args
                    .iter()
                    .any(|a| matches!(a, ResolvedRecordLitArg::Named(_, _)))
                {
                    return Err(TypeError {
                        message: "Function values do not accept named arguments".into(),
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
                let typed_args: Vec<TypedNode> = args
                    .iter()
                    .zip(params.iter())
                    .map(|(arg, expected)| match arg {
                        ResolvedRecordLitArg::Positional(expr) => {
                            self.check_node_with_expected(expr, Some(expected))
                        }
                        ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                for (param, arg) in params.iter().zip(&typed_args) {
                    if !self.types_compatible(param, &arg.ty) {
                        return Err(TypeError {
                            message: format!(
                                "Argument type mismatch: expected {}, got {}",
                                self.ty_name(param),
                                self.ty_name(&arg.ty)
                            ),
                            span: arg.span.clone(),
                            hint: None,
                        });
                    }
                }
                self.ensure_no_runtime_lens_args(&typed_args, span, "Function call")?;

                Ok(TypedNode {
                    ty: self.resolve_ty(ret),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(typed_func), typed_args),
                })
            }
            _ => Err(TypeError {
                message: format!("Not a function: {}", self.ty_name(&typed_func.ty)),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    pub(super) fn check_closure(
        &mut self,
        span: &Span,
        params: &[ResolvedClosureParam],
        captures: &[ResolvedId],
        body: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let mut body_checker = self.spawn_child_checker(self.env.clone());
        body_checker.closure_depth = self.closure_depth.saturating_add(1);
        let mut typed_params = Vec::new();
        let param_tys = match expected {
            Some(Ty::Func(expected_params, _)) => {
                if expected_params.len() != params.len() {
                    return Err(TypeError {
                        message: format!(
                            "closure expects {} parameter(s), got {}",
                            expected_params.len(),
                            params.len()
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                expected_params.clone()
            }
            Some(other) => {
                return Err(TypeError {
                    message: format!("Expected function type, got {}", self.ty_name(other)),
                    span: span.clone(),
                    hint: None,
                });
            }
            None => params
                .iter()
                .map(|param| match &param.ty {
                    Some(ast_ty) => body_checker.resolve_ast_ty_in_context(
                        ast_ty,
                        body_checker.local_type_syntax_context(),
                    ),
                    None => Ok(body_checker.env.fresh_tyvar()),
                })
                .collect::<Result<Vec<_>, _>>()?,
        };

        for (param, param_ty) in params.iter().zip(param_tys.iter()) {
            let param_ty = if let Some(ast_ty) = &param.ty {
                let annotated = body_checker
                    .resolve_ast_ty_in_context(ast_ty, body_checker.local_type_syntax_context())?;
                if !body_checker.types_compatible(param_ty, &annotated) {
                    return Err(TypeError {
                        message: format!(
                            "closure parameter `{}` expected {}, got {}",
                            param.id.name,
                            body_checker.ty_name(param_ty),
                            body_checker.ty_name(&annotated)
                        ),
                        span: param.id.span.clone(),
                        hint: None,
                    });
                }
                body_checker.resolve_ty(&annotated)
            } else {
                body_checker.resolve_ty(param_ty)
            };
            body_checker
                .env
                .bind_var(param.id.unique_id, param_ty.clone());
            typed_params.push(TypedClosureParam {
                id: param.id.clone(),
                ty: param_ty,
            });
        }

        for capture in captures {
            if let Some(ty) = self.env.lookup_var(capture.unique_id).cloned() {
                if matches!(ty, Ty::Lens(_, _)) {
                    return Err(TypeError {
                        message: "Lens values are scope-local compile-time capabilities and cannot be captured by closures".into(),
                        span: capture.span.clone(),
                        hint: Some(
                            "Consume the Lens in the current scope with Lens::view/set/over before creating the closure."
                                .into(),
                        ),
                    });
                }
                body_checker
                    .env
                    .bind_var(capture.unique_id, body_checker.resolve_ty(&ty));
            }
        }

        if let Some(Ty::Func(_, expected_ret)) = expected {
            body_checker.function_return_ty = Some(expected_ret.as_ref().clone());
        }
        let typed_body = body_checker.check_node(body)?;
        if matches!(typed_body.ty, Ty::Lens(_, _)) {
            return Err(TypeError {
                message: "Lens is compile-time only in Stage1 and cannot be returned from closures"
                    .into(),
                span: typed_body.span.clone(),
                hint: Some("Use Lens::view(...) inside the closure instead.".into()),
            });
        }
        let typed_body = body_checker.resolve_typed_node(typed_body);
        self.absorb_child_progress(&body_checker);

        let param_tys = typed_params
            .iter()
            .map(|p| body_checker.resolve_ty(&p.ty))
            .collect::<Vec<_>>();
        Ok(TypedNode {
            ty: Ty::Func(param_tys, Box::new(body_checker.resolve_ty(&typed_body.ty))),
            span: span.clone(),
            node: TypedInner::Closure(
                typed_params
                    .into_iter()
                    .map(|param| TypedClosureParam {
                        id: param.id,
                        ty: body_checker.resolve_ty(&param.ty),
                    })
                    .collect(),
                captures.to_vec(),
                Box::new(typed_body),
            ),
        })
    }

    pub(super) fn check_capture(
        &mut self,
        span: &Span,
        target: &Resolved,
        args: &[Resolved],
    ) -> Result<TypedNode, TypeError> {
        let typed_target = self.check_node(target)?;
        let target_ty = self.resolve_ty(&typed_target.ty);
        let (params, ret) = match &target_ty {
            Ty::BuiltinFunc { params, ret, .. } => (params.clone(), ret.as_ref().clone()),
            Ty::UserFunc { params, ret, .. } => (params.clone(), ret.as_ref().clone()),
            Ty::Func(params, ret) => (params.clone(), ret.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!("Not a function: {}", self.ty_name(other)),
                    span: typed_target.span.clone(),
                    hint: None,
                });
            }
        };

        if args.len() > params.len() {
            return Err(TypeError {
                message: format!(
                    "partial application expects at most {} argument(s), got {}",
                    params.len(),
                    args.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_args: Vec<TypedNode> = args
            .iter()
            .zip(params.iter())
            .map(|(arg, expected)| self.check_node_with_expected(arg, Some(expected)))
            .collect::<Result<Vec<_>, _>>()?;

        for (param, arg) in params.iter().zip(&typed_args) {
            if !self.types_compatible(param, &arg.ty) {
                return Err(TypeError {
                    message: format!(
                        "Argument type mismatch: expected {}, got {}",
                        self.ty_name(param),
                        self.ty_name(&arg.ty)
                    ),
                    span: arg.span.clone(),
                    hint: None,
                });
            }
        }
        self.ensure_no_runtime_lens_args(&typed_args, span, "Partial application")?;

        let remaining = params[typed_args.len()..].to_vec();
        Ok(TypedNode {
            ty: Ty::Func(
                remaining
                    .into_iter()
                    .map(|ty| self.resolve_ty(&ty))
                    .collect(),
                Box::new(self.resolve_ty(&ret)),
            ),
            span: span.clone(),
            node: TypedInner::Capture(Box::new(typed_target), typed_args),
        })
    }

    pub(super) fn maybe_call_zero_arg_function(
        &self,
        node: TypedNode,
        _call_span: Span,
    ) -> TypedNode {
        match &node.ty {
            Ty::BuiltinFunc { params, ret, .. }
            | Ty::UserFunc { params, ret, .. }
            | Ty::Func(params, ret)
                if params.is_empty() =>
            {
                TypedNode {
                    ty: ret.as_ref().clone(),
                    span: node.span.clone(),
                    node: TypedInner::App(Box::new(node), Vec::new()),
                }
            }
            _ => node,
        }
    }

    pub(super) fn check_binop(
        &mut self,
        span: &Span,
        op: &BinOp,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_left = self.check_node(left)?;
        let typed_right = self.check_node(right)?;
        let lt = self.resolve_ty(&typed_left.ty);
        let rt = self.resolve_ty(&typed_right.ty);
        let compatibility_checkpoint = self.substitutions.clone();
        let compatible = self.types_compatible(&lt, &rt);

        let make_trait_call = |trait_name: String,
                               method_name: &str,
                               receiver_ty: Ty,
                               dispatch: TraitDispatch,
                               result_ty: Ty,
                               typed_left: TypedNode,
                               typed_right: TypedNode| {
            TypedNode {
                ty: result_ty,
                span: span.clone(),
                node: TypedInner::TraitCall {
                    trait_name,
                    method_name: method_name.into(),
                    receiver_ty,
                    dispatch,
                    args: vec![typed_left, typed_right],
                },
            }
        };

        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul => {
                let method_name = match op {
                    BinOp::Add => "add",
                    BinOp::Sub => "sub",
                    BinOp::Mul => "mul",
                    _ => unreachable!("validated above"),
                };
                if !compatible {
                    self.substitutions = compatibility_checkpoint;
                    return Err(TypeError {
                        message: format!(
                            "Cannot apply {:?} to {} and {}",
                            op,
                            self.ty_name(&lt),
                            self.ty_name(&rt)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let receiver_ty = self.resolve_ty(&lt);
                let numeric_trait =
                    self.trait_key_by_short_name("Numeric")
                        .ok_or_else(|| TypeError {
                            message: "Unknown trait: Numeric".into(),
                            span: span.clone(),
                            hint: None,
                        })?;
                let dispatch = self
                    .trait_dispatch_target(&numeric_trait, method_name, &receiver_ty)
                    .ok_or_else(|| TypeError {
                        message: format!(
                            "Operator {:?} requires both operands to implement Numeric",
                            op
                        ),
                        span: span.clone(),
                        hint: Some("Add a `Numeric` bound or use `Int` / `Float` values.".into()),
                    })?;
                Ok(make_trait_call(
                    numeric_trait,
                    method_name,
                    receiver_ty.clone(),
                    dispatch,
                    receiver_ty,
                    typed_left,
                    typed_right,
                ))
            }
            BinOp::Eq | BinOp::Neq => {
                if !compatible {
                    self.substitutions = compatibility_checkpoint;
                    return Err(TypeError {
                        message: format!(
                            "Cannot compare {} and {}",
                            self.ty_name(&lt),
                            self.ty_name(&rt)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let receiver_ty = self.resolve_ty(&lt);
                let eq_trait = self
                    .trait_key_by_short_name("Eq")
                    .ok_or_else(|| TypeError {
                        message: "Unknown trait: Eq".into(),
                        span: span.clone(),
                        hint: None,
                    })?;
                let method_name = match op {
                    BinOp::Eq => "eq",
                    BinOp::Neq => "neq",
                    _ => unreachable!("validated above"),
                };
                let dispatch = self
                    .trait_dispatch_target(&eq_trait, method_name, &receiver_ty)
                    .ok_or_else(|| TypeError {
                        message: format!(
                            "{} / {} not supported for {}",
                            "==",
                            "!=",
                            self.ty_name(&receiver_ty)
                        ),
                        span: span.clone(),
                        hint: None,
                    })?;
                Ok(make_trait_call(
                    eq_trait,
                    method_name,
                    receiver_ty,
                    dispatch,
                    Ty::Bool,
                    typed_left,
                    typed_right,
                ))
            }
            BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte => {
                if !compatible {
                    self.substitutions = compatibility_checkpoint;
                    return Err(TypeError {
                        message: format!(
                            "Cannot compare {} and {}",
                            self.ty_name(&lt),
                            self.ty_name(&rt)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let receiver_ty = self.resolve_ty(&lt);
                let ord_trait = self
                    .trait_key_by_short_name("Ord")
                    .ok_or_else(|| TypeError {
                        message: "Unknown trait: Ord".into(),
                        span: span.clone(),
                        hint: None,
                    })?;
                let method_name = match op {
                    BinOp::Lt => "lt",
                    BinOp::Gt => "gt",
                    BinOp::Lte => "lte",
                    BinOp::Gte => "gte",
                    _ => unreachable!("validated above"),
                };
                let dispatch = self
                    .trait_dispatch_target(&ord_trait, method_name, &receiver_ty)
                    .ok_or_else(|| TypeError {
                        message: format!(
                            "Cannot compare {} and {}",
                            self.ty_name(&lt),
                            self.ty_name(&rt)
                        ),
                        span: span.clone(),
                        hint: None,
                    })?;
                Ok(make_trait_call(
                    ord_trait,
                    method_name,
                    receiver_ty,
                    dispatch,
                    Ty::Bool,
                    typed_left,
                    typed_right,
                ))
            }
            BinOp::Concat => {
                if !compatible {
                    self.substitutions = compatibility_checkpoint;
                    return Err(TypeError {
                        message: format!(
                            "++ requires (String, String), got ({}, {})",
                            self.ty_name(&lt),
                            self.ty_name(&rt)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let receiver_ty = self.resolve_ty(&lt);
                let concat_trait =
                    self.trait_key_by_short_name("Concat")
                        .ok_or_else(|| TypeError {
                            message: "Unknown trait: Concat".into(),
                            span: span.clone(),
                            hint: None,
                        })?;
                let dispatch = self
                    .trait_dispatch_target(&concat_trait, "concat", &receiver_ty)
                    .ok_or_else(|| TypeError {
                        message: format!(
                            "++ requires values implementing Concat, got ({}, {})",
                            self.ty_name(&lt),
                            self.ty_name(&rt)
                        ),
                        span: span.clone(),
                        hint: None,
                    })?;
                Ok(make_trait_call(
                    concat_trait,
                    "concat",
                    receiver_ty.clone(),
                    dispatch,
                    receiver_ty,
                    typed_left,
                    typed_right,
                ))
            }
        }
    }

    pub(super) fn check_list_nil(&mut self, span: &Span) -> Result<TypedNode, TypeError> {
        let tv = self.env.fresh_tyvar();
        Ok(TypedNode {
            ty: Ty::List(Box::new(tv)),
            span: span.clone(),
            node: TypedInner::ListNil,
        })
    }

    pub(super) fn check_list_cons(
        &mut self,
        span: &Span,
        head: &Resolved,
        tail: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_head = self.check_node(head)?;
        let typed_tail = self.check_node(tail)?;
        self.ensure_no_runtime_lens_value(&typed_head, "List construction")?;
        self.ensure_no_runtime_lens_value(&typed_tail, "List construction")?;
        let tail_elem_ty = match &typed_tail.ty {
            Ty::List(inner) => inner.as_ref().clone(),
            other => {
                return Err(TypeError {
                    message: format!("list tail must be List<...>, got {}", self.ty_name(other)),
                    span: typed_tail.span.clone(),
                    hint: Some("Use `[head, ..tail]` with a list tail value".into()),
                });
            }
        };

        if !self.types_compatible(&typed_head.ty, &tail_elem_ty) {
            return Err(TypeError {
                message: format!(
                    "expected {}, got {}",
                    self.ty_name(&tail_elem_ty),
                    self.ty_name(&typed_head.ty)
                ),
                span: typed_head.span.clone(),
                hint: Some("List head and tail element types must match".into()),
            });
        }

        let elem_ty = self.resolve_ty(&tail_elem_ty);
        Ok(TypedNode {
            ty: Ty::List(Box::new(elem_ty.clone())),
            span: span.clone(),
            node: TypedInner::ListCons(Box::new(typed_head), Box::new(typed_tail)),
        })
    }

    pub(super) fn check_list_literal(
        &mut self,
        span: &Span,
        elems: &[Resolved],
    ) -> Result<TypedNode, TypeError> {
        if elems.is_empty() {
            return self.check_list_nil(span);
        }

        let typed_elems: Vec<TypedNode> = elems
            .iter()
            .map(|e| self.check_node(e))
            .collect::<Result<Vec<_>, _>>()?;
        for typed in &typed_elems {
            self.ensure_no_runtime_lens_value(typed, "List literal")?;
        }

        let elem_ty = typed_elems[0].ty.clone();
        for te in typed_elems.iter().skip(1) {
            if !self.types_compatible(&elem_ty, &te.ty) {
                return Err(TypeError {
                    message: format!(
                        "expected {}, got {}",
                        self.ty_name(&elem_ty),
                        self.ty_name(&te.ty)
                    ),
                    span: te.span.clone(),
                    hint: Some("All list elements must have the same type".into()),
                });
            }
        }

        Ok(TypedNode {
            ty: Ty::List(Box::new(elem_ty)),
            span: span.clone(),
            node: TypedInner::ListLiteral(typed_elems),
        })
    }

    pub(super) fn check_tuple_literal(
        &mut self,
        span: &Span,
        elems: &[Resolved],
    ) -> Result<TypedNode, TypeError> {
        if elems.len() < 2 {
            return Err(TypeError {
                message: "Tuple literals require at least 2 values".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let typed_elems = elems
            .iter()
            .map(|elem| self.check_node(elem))
            .collect::<Result<Vec<_>, _>>()?;
        for typed in &typed_elems {
            self.ensure_no_runtime_lens_value(typed, "Tuple literal")?;
        }
        let item_tys = typed_elems.iter().map(|elem| elem.ty.clone()).collect();

        Ok(TypedNode {
            ty: Ty::Tuple(item_tys),
            span: span.clone(),
            node: TypedInner::TupleLiteral(typed_elems),
        })
    }

    pub(super) fn check_interpolated_str(
        &mut self,
        span: &Span,
        parts: &[ResolvedInterpolatedPart],
    ) -> Result<TypedNode, TypeError> {
        let mut typed_parts = Vec::new();
        for part in parts {
            match part {
                ResolvedInterpolatedPart::Text(s) => {
                    typed_parts.push(TypedInterpolatedPart::Text(s.clone()));
                }
                ResolvedInterpolatedPart::Expr(expr) => {
                    let typed_expr = self.check_node(expr)?;
                    self.ensure_no_runtime_lens_value(&typed_expr, "String interpolation")?;
                    if matches!(typed_expr.ty, Ty::Result(_, _)) {
                        return Err(TypeError {
                            message: "Interpolation does not allow Result type".into(),
                            span: typed_expr.span.clone(),
                            hint: Some(
                                "Unwrap/match the Result first, or convert it to a printable value"
                                    .into(),
                            ),
                        });
                    }
                    typed_parts.push(TypedInterpolatedPart::Expr(Box::new(typed_expr)));
                }
            }
        }

        Ok(TypedNode {
            ty: Ty::Str,
            span: span.clone(),
            node: TypedInner::InterpolatedStr(typed_parts),
        })
    }

    pub(super) fn check_if(
        &mut self,
        span: &Span,
        cond: &Resolved,
        then: &Resolved,
        else_opt: &Option<Box<Resolved>>,
    ) -> Result<TypedNode, TypeError> {
        let typed_cond = self.check_node(cond)?;
        if !self.types_compatible(&Ty::Bool, &typed_cond.ty) {
            return Err(TypeError {
                message: format!(
                    "if condition must be Boolean, got {}",
                    self.ty_name(&typed_cond.ty)
                ),
                span: typed_cond.span.clone(),
                hint: None,
            });
        }

        let typed_then = self.check_node(then)?;

        match else_opt {
            Some(else_branch) => {
                let typed_else = self.check_node(else_branch)?;
                if !self.types_compatible(&typed_then.ty, &typed_else.ty) {
                    return Err(TypeError {
                        message: format!(
                            "if branches have different types: {} and {}",
                            self.ty_name(&typed_then.ty),
                            self.ty_name(&typed_else.ty)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let ty = typed_then.ty.clone();
                Ok(TypedNode {
                    ty,
                    span: span.clone(),
                    node: TypedInner::If(
                        Box::new(typed_cond),
                        Box::new(typed_then),
                        Some(Box::new(typed_else)),
                    ),
                })
            }
            None => Ok(TypedNode {
                ty: Ty::Unit,
                span: span.clone(),
                node: TypedInner::If(Box::new(typed_cond), Box::new(typed_then), None),
            }),
        }
    }

    pub(super) fn check_assert(
        &mut self,
        span: &Span,
        cond: &Resolved,
        err: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_cond = self.check_node(cond)?;
        if !self.types_compatible(&Ty::Bool, &typed_cond.ty) {
            return Err(TypeError {
                message: format!(
                    "assert condition must be Boolean, got {}",
                    self.ty_name(&typed_cond.ty)
                ),
                span: typed_cond.span.clone(),
                hint: None,
            });
        }

        let raw_err = self.check_node(err)?;
        let typed_err = self.maybe_call_zero_arg_function(raw_err, span.clone());
        self.ensure_guard_error_value(&typed_err, "assert")?;

        Ok(TypedNode {
            ty: Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::Assert(Box::new(typed_cond), Box::new(typed_err)),
        })
    }

    pub(super) fn check_ensure(
        &mut self,
        span: &Span,
        value: &Resolved,
        pred: &Resolved,
        err: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let typed_value = self.check_node(value)?;
        let typed_pred = self.check_compose_callable(pred, "ensure")?;
        let (pred_in, pred_out) =
            self.unary_function_parts(&typed_pred.ty, "ensure", &typed_pred.span)?;
        if !self.types_compatible(&pred_in, &typed_value.ty) {
            return Err(TypeError {
                message: format!(
                    "ensure predicate type mismatch: expected {}, got {}",
                    self.ty_name(&typed_value.ty),
                    self.ty_name(&pred_in)
                ),
                span: typed_pred.span.clone(),
                hint: None,
            });
        }
        if !self.types_compatible(&Ty::Bool, &pred_out) {
            return Err(TypeError {
                message: format!(
                    "ensure predicate must return Boolean, got {}",
                    self.ty_name(&pred_out)
                ),
                span: typed_pred.span.clone(),
                hint: None,
            });
        }

        let raw_err = self.check_node(err)?;
        let typed_err = self.maybe_call_zero_arg_function(raw_err, span.clone());
        self.ensure_guard_error_value(&typed_err, "ensure")?;

        Ok(TypedNode {
            ty: Ty::Result(
                Box::new(self.resolve_ty(&typed_value.ty)),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::Ensure(
                Box::new(typed_value),
                Box::new(typed_pred),
                Box::new(typed_err),
            ),
        })
    }

    pub(super) fn check_recover_kind(
        &mut self,
        span: &Span,
        value: &Resolved,
        marker: &ResolvedId,
        handler: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        if !self.env.is_error_constructor(marker.unique_id) {
            return Err(TypeError {
                message: "recover_kind marker must be a concrete deferror name".into(),
                span: marker.span.clone(),
                hint: Some("Pass a deferror constructor name such as Timeout, not a value.".into()),
            });
        }

        let typed_value = self.check_node(value)?;
        let value_ty = self.resolve_ty(&typed_value.ty);
        let Ty::Result(ok_ty, _) = &value_ty else {
            return Err(TypeError {
                message: format!(
                    "recover_kind value must be Result<...>, got {}",
                    self.ty_name(&value_ty)
                ),
                span: typed_value.span.clone(),
                hint: None,
            });
        };
        let ok_ty = ok_ty.as_ref().clone();
        let expected_handler = Ty::Func(
            vec![Ty::Error],
            Box::new(Ty::Result(Box::new(ok_ty.clone()), Box::new(Ty::Error))),
        );
        let typed_handler = self.check_node_with_expected(handler, Some(&expected_handler))?;
        let (handler_in, handler_out) =
            self.unary_function_parts(&typed_handler.ty, "recover_kind", &typed_handler.span)?;
        if !self.types_compatible(&Ty::Error, &handler_in) {
            return Err(TypeError {
                message: format!(
                    "recover_kind handler must accept Error, got {}",
                    self.ty_name(&handler_in)
                ),
                span: typed_handler.span.clone(),
                hint: None,
            });
        }
        if !self.types_compatible(&expected_handler, &typed_handler.ty) {
            return Err(TypeError {
                message: format!(
                    "recover_kind handler must return Result<{}>, got {}",
                    self.ty_name(&ok_ty),
                    self.ty_name(&handler_out)
                ),
                span: typed_handler.span.clone(),
                hint: None,
            });
        }

        Ok(TypedNode {
            ty: Ty::Result(Box::new(ok_ty), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::RecoverKind(
                Box::new(typed_value),
                marker.clone(),
                Box::new(typed_handler),
            ),
        })
    }

    fn resolve_lens_segment_for_source_ty(
        &mut self,
        source_ty: &Ty,
        field: &str,
        span: &Span,
        for_capability: bool,
    ) -> Result<(TypedLensSegment, Ty, bool), TypeError> {
        match self.resolve_ty(source_ty) {
            Ty::Tuple(items) => {
                let index = field
                    .strip_prefix('_')
                    .ok_or_else(|| TypeError {
                        message: "Tuple elements are accessed with ._0, ._1, ...".into(),
                        span: span.clone(),
                        hint: None,
                    })?
                    .parse::<usize>()
                    .map_err(|_| TypeError {
                        message: "Tuple elements are accessed with ._0, ._1, ...".into(),
                        span: span.clone(),
                        hint: None,
                    })?;
                let field_ty = items.get(index).cloned().ok_or_else(|| TypeError {
                    message: format!(
                        "Tuple index ._{} is out of bounds for {}",
                        index,
                        self.ty_name(source_ty)
                    ),
                    span: span.clone(),
                    hint: None,
                })?;
                Ok((
                    TypedLensSegment::Tuple {
                        field_index: index as u32,
                        tuple_len: items.len() as u32,
                    },
                    field_ty,
                    false,
                ))
            }
            Ty::Struct(name, fields) | Ty::Record(name, fields) => {
                if self.env.is_private_field(&name, field) {
                    let outside_impl =
                        self.current_impl_struct_target.as_deref() != Some(name.as_str());
                    if for_capability && outside_impl {
                        return Err(TypeError {
                            message: format!("Field '{}.{}' is private", name, field),
                            span: span.clone(),
                            hint: Some(format!(
                                "Expose the value through a public method on {} instead.",
                                name
                            )),
                        });
                    }
                    if !for_capability && outside_impl && self.closure_depth > 0 {
                        return Err(TypeError {
                            message: format!(
                                "Field '{}.{}' is private and cannot be accessed from closures outside impl {}",
                                name, field, name
                            ),
                            span: span.clone(),
                            hint: Some(format!(
                                "Read {}.{} in the current scope first, then capture the plain value.",
                                name, field
                            )),
                        });
                    }
                }
                let (field_index, field_ty) = fields
                    .iter()
                    .enumerate()
                    .find(|(_, (field_name, _))| field_name == field)
                    .map(|(i, (_, ty))| (i as u32, ty.clone()))
                    .ok_or_else(|| TypeError {
                        message: format!("No field '{}' on {}", field, self.ty_name(source_ty)),
                        span: span.clone(),
                        hint: None,
                    })?;
                Ok((
                    TypedLensSegment::Field {
                        field_name: field.to_string(),
                        field_index,
                        container_field_count: fields.len() as u32,
                    },
                    field_ty,
                    false,
                ))
            }
            Ty::Enum(enum_name, _) => {
                if self.lookup_enum_variants_of(&enum_name).is_none() {
                    return Err(TypeError {
                        message: format!("No variants found for enum {}", enum_name),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let Some(variant) = self.lookup_enum_variant_by_short_name(&enum_name, field)
                else {
                    return Err(TypeError {
                        message: format!(
                            "No variant selector '{}' on {} (use PascalCase constructor names)",
                            field, enum_name
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                };
                let variant = self.instantiate_enum_variant(&variant);
                if !self.types_compatible(&variant.enum_ty, source_ty) {
                    return Err(TypeError {
                        message: format!(
                            "Variant selector {}.{} does not match {}",
                            enum_name,
                            field,
                            self.ty_name(source_ty)
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let payload_arity = variant.payload.len() as u32;
                let focus_ty = match variant.payload.len() {
                    0 => Ty::Unit,
                    1 => variant.payload[0].clone(),
                    _ => Ty::Tuple(variant.payload.clone()),
                };
                Ok((
                    TypedLensSegment::Variant {
                        enum_name,
                        variant_name: variant.short_name,
                        variant_tag: variant.tag,
                        payload_arity,
                    },
                    focus_ty,
                    true,
                ))
            }
            other => {
                let message = if field.starts_with('_')
                    && field
                        .strip_prefix('_')
                        .is_some_and(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
                {
                    format!(
                        "Tuple-style access .{} is only available on tuples, got {}",
                        field,
                        self.ty_name(&other)
                    )
                } else {
                    format!("Cannot access field on {}", self.ty_name(&other))
                };
                Err(TypeError {
                    message,
                    span: span.clone(),
                    hint: None,
                })
            }
        }
    }

    fn try_check_tuple_type_root_lens_path(
        &mut self,
        span: &Span,
        expr: &Resolved,
        field: &str,
        expected: Option<&Ty>,
    ) -> Result<Option<TypedNode>, TypeError> {
        let Resolved::Var(_, id) = expr else {
            return Ok(None);
        };
        if id.name != "Tuple" {
            return Ok(None);
        }
        let Some(index) = Self::parse_tuple_segment_index(field) else {
            return Ok(None);
        };

        let expected_ty = expected.ok_or_else(|| TypeError {
            message: format!(
                "Tuple.{} requires Lens type context (e.g. Lens::view(Tuple.{}, source_tuple))",
                field, field
            ),
            span: span.clone(),
            hint: Some("Use Tuple._N only where a Lens<(...), ...> is expected.".into()),
        })?;
        let expected_ty = self.resolve_ty(expected_ty);
        let (expected_source, expected_focus) = match expected_ty {
            Ty::Lens(source, focus) => (source.as_ref().clone(), focus.as_ref().clone()),
            other => {
                return Err(TypeError {
                    message: format!(
                        "Tuple.{} requires expected Lens<..., ...> context, got {}",
                        field,
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: Some(
                        "Use Tuple._N as a Lens path argument in Lens::view/set/over.".into(),
                    ),
                });
            }
        };

        let tuple_items = match self.resolve_ty(&expected_source) {
            Ty::Tuple(items) => items,
            other => {
                return Err(TypeError {
                    message: format!(
                        "Tuple.{} requires tuple source context, got {}",
                        field,
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: Some("Expected source type like (A, B, ...) for Tuple._N.".into()),
                });
            }
        };

        let focus_ty = tuple_items.get(index).cloned().ok_or_else(|| TypeError {
            message: format!(
                "Tuple index ._{} is out of bounds for ({})",
                index,
                tuple_items
                    .iter()
                    .map(|item| self.ty_name(item))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            span: span.clone(),
            hint: None,
        })?;

        if !self.types_compatible(&focus_ty, &expected_focus) {
            return Err(TypeError {
                message: format!(
                    "Tuple.{} focus type mismatch: expected {}, got {}",
                    field,
                    self.ty_name(&expected_focus),
                    self.ty_name(&focus_ty)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let source_ty = Ty::Tuple(tuple_items);
        let path = TypedLensPath {
            source_ty: source_ty.clone(),
            focus_ty: focus_ty.clone(),
            may_fail: false,
            segments: vec![TypedLensSegment::Tuple {
                field_index: index as u32,
                tuple_len: match &source_ty {
                    Ty::Tuple(items) => items.len() as u32,
                    _ => unreachable!("source_ty is always Tuple here"),
                },
            }],
        };

        Ok(Some(TypedNode {
            ty: Ty::Lens(Box::new(source_ty), Box::new(focus_ty)),
            span: span.clone(),
            node: TypedInner::LensPath(path),
        }))
    }

    fn check_field_access_with_expected(
        &mut self,
        span: &Span,
        expr: &Resolved,
        field: &str,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        if let Some(tuple_root_path) =
            self.try_check_tuple_type_root_lens_path(span, expr, field, expected)?
        {
            return Ok(tuple_root_path);
        }
        let typed_expr = self.check_node(expr)?;

        if matches!(typed_expr.ty, Ty::Lens(_, _)) {
            let path = self.resolve_lens_path_from_node(typed_expr, span)?;
            let (segment, focus_ty, may_fail) =
                self.resolve_lens_segment_for_source_ty(&path.focus_ty, field, span, true)?;
            let source_ty = self.resolve_ty(&path.source_ty);
            let focus_ty = self.resolve_ty(&focus_ty);
            let mut segments = path.segments;
            segments.push(segment);
            let combined = TypedLensPath {
                source_ty: source_ty.clone(),
                focus_ty: focus_ty.clone(),
                may_fail: path.may_fail || may_fail,
                segments,
            };
            return Ok(TypedNode {
                ty: Ty::Lens(Box::new(source_ty), Box::new(focus_ty)),
                span: span.clone(),
                node: TypedInner::LensPath(combined),
            });
        }

        if let TypedInner::Var(id) = &typed_expr.node {
            if self.env.is_type_constructor_id(id.unique_id) {
                let source_ty = self.resolve_ty(&typed_expr.ty);
                let (segment, focus_ty, may_fail) =
                    self.resolve_lens_segment_for_source_ty(&source_ty, field, span, true)?;
                let focus_ty = self.resolve_ty(&focus_ty);
                let path = TypedLensPath {
                    source_ty: source_ty.clone(),
                    focus_ty: focus_ty.clone(),
                    may_fail,
                    segments: vec![segment],
                };
                return Ok(TypedNode {
                    ty: Ty::Lens(Box::new(source_ty), Box::new(focus_ty)),
                    span: span.clone(),
                    node: TypedInner::LensPath(path),
                });
            }
        }

        let (source_is_result, source_focus_ty) = match self.resolve_ty(&typed_expr.ty) {
            Ty::Result(ok, _) => (true, ok.as_ref().clone()),
            other => (false, other),
        };
        let (segment, focus_ty, may_fail) =
            self.resolve_lens_segment_for_source_ty(&source_focus_ty, field, span, false)?;
        let focus_ty = self.resolve_ty(&focus_ty);
        let path = TypedLensPath {
            source_ty: source_focus_ty,
            focus_ty: focus_ty.clone(),
            may_fail,
            segments: vec![segment],
        };
        let out_ty = if source_is_result || path.may_fail {
            Ty::Result(Box::new(focus_ty), Box::new(Ty::Error))
        } else {
            focus_ty
        };

        Ok(TypedNode {
            ty: out_ty,
            span: span.clone(),
            node: TypedInner::LensView {
                source: Box::new(typed_expr),
                path,
                source_is_result,
            },
        })
    }

    pub(super) fn check_field_access(
        &mut self,
        span: &Span,
        expr: &Resolved,
        field: &str,
    ) -> Result<TypedNode, TypeError> {
        self.check_field_access_with_expected(span, expr, field, None)
    }
}
