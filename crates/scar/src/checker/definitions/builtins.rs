use super::*;

struct SpecialFormContract {
    expected_qname: &'static str,
    expected_signature: &'static str,
    shape_ok: fn(&[ResolvedValueParameter], &Option<AstTy>) -> bool,
}

fn special_form_shape_if(params: &[ResolvedValueParameter], ret_ty: &Option<AstTy>) -> bool {
    params.len() == 3
        && Checker::is_named_type(&params[0].ty, "Boolean")
        && Checker::is_lazy_of_named(&params[1].ty, "$A")
        && Checker::is_lazy_of_named(&params[2].ty, "$A")
        && ret_ty
            .as_ref()
            .is_some_and(|ty| Checker::is_named_type(ty, "$A"))
}

fn special_form_shape_if_then(params: &[ResolvedValueParameter], ret_ty: &Option<AstTy>) -> bool {
    params.len() == 2
        && Checker::is_named_type(&params[0].ty, "Boolean")
        && Checker::is_lazy_of_unit(&params[1].ty)
        && ret_ty.as_ref().is_some_and(Checker::is_unit_type)
}

fn special_form_shape_if_let(params: &[ResolvedValueParameter], ret_ty: &Option<AstTy>) -> bool {
    params.len() == 4
        && Checker::is_named_type(&params[0].ty, "$A")
        && Checker::is_named_type(&params[1].ty, "$Pattern")
        && Checker::is_lazy_of_named(&params[2].ty, "$B")
        && Checker::is_lazy_of_named(&params[3].ty, "$B")
        && ret_ty
            .as_ref()
            .is_some_and(|ty| Checker::is_named_type(ty, "$B"))
}

fn special_form_shape_if_let_then(
    params: &[ResolvedValueParameter],
    ret_ty: &Option<AstTy>,
) -> bool {
    params.len() == 3
        && Checker::is_named_type(&params[0].ty, "$A")
        && Checker::is_named_type(&params[1].ty, "$Pattern")
        && Checker::is_lazy_of_unit(&params[2].ty)
        && ret_ty.as_ref().is_some_and(Checker::is_unit_type)
}

fn special_form_shape_is_match(params: &[ResolvedValueParameter], ret_ty: &Option<AstTy>) -> bool {
    params.len() == 2
        && Checker::is_named_type(&params[0].ty, "$A")
        && Checker::is_named_type(&params[1].ty, "$Pattern")
        && ret_ty
            .as_ref()
            .is_some_and(|ty| Checker::is_named_type(ty, "Boolean"))
}

pub(in crate::checker) fn special_form_shape_apply_pattern(
    params: &[ResolvedValueParameter],
    ret_ty: &Option<AstTy>,
) -> bool {
    params.len() == 2
        && Checker::is_named_type(&params[0].ty, "$Value")
        && Checker::is_named_type(&params[1].ty, "$Pattern")
        && ret_ty
            .as_ref()
            .is_some_and(|ty| Checker::is_result_of_named(ty, "$Return"))
}

fn special_form_shape_assert_error_matches(
    params: &[ResolvedValueParameter],
    ret_ty: &Option<AstTy>,
) -> bool {
    params.len() == 3
        && Checker::is_named_type(&params[0].ty, "Error")
        && Checker::is_named_type(&params[1].ty, "$Pattern")
        && matches!(params[2].ty.syntax(), AstTy::Generic(_, name, args)
            if name == "Lazy" && args.len() == 1 && Checker::is_result_of_named(&args[0], "Unit"))
        && ret_ty
            .as_ref()
            .is_some_and(|ty| Checker::is_result_of_named(ty, "Unit"))
}

fn special_form_shape_require(params: &[ResolvedValueParameter], ret_ty: &Option<AstTy>) -> bool {
    params.len() == 2
        && Checker::is_named_type(&params[0].ty, "Boolean")
        && Checker::is_lazy_of_named(&params[1].ty, "Error")
        && ret_ty
            .as_ref()
            .is_some_and(|ty| Checker::is_result_of_named(ty, "Unit"))
}

fn special_form_shape_ensure(params: &[ResolvedValueParameter], ret_ty: &Option<AstTy>) -> bool {
    params.len() == 3
        && Checker::is_named_type(&params[0].ty, "$A")
        && Checker::is_unary_func_from_named_to_named(&params[1].ty, "$A", "Boolean")
        && Checker::is_lazy_of_named(&params[2].ty, "Error")
        && ret_ty
            .as_ref()
            .is_some_and(|ty| Checker::is_result_of_named(ty, "$A"))
}

fn special_form_shape_map_err_or_cause(
    params: &[ResolvedValueParameter],
    ret_ty: &Option<AstTy>,
) -> bool {
    params.len() == 2
        && Checker::is_result_of_named(&params[0].ty, "$T")
        && Checker::is_lazy_of_named(&params[1].ty, "Error")
        && ret_ty
            .as_ref()
            .is_some_and(|ty| Checker::is_result_of_named(ty, "$T"))
}

fn special_form_shape_or_else(params: &[ResolvedValueParameter], ret_ty: &Option<AstTy>) -> bool {
    params.len() == 2
        && Checker::is_result_of_named(&params[0].ty, "$T")
        && matches!(params[1].ty.syntax(), AstTy::Generic(_, name, args)
            if name == "Lazy" && args.len() == 1
                && Checker::is_result_of_named(&args[0], "$T"))
        && ret_ty
            .as_ref()
            .is_some_and(|ty| Checker::is_result_of_named(ty, "$T"))
}

fn special_form_shape_and_or(params: &[ResolvedValueParameter], ret_ty: &Option<AstTy>) -> bool {
    params.len() == 2
        && Checker::is_named_type(&params[0].ty, "Boolean")
        && Checker::is_lazy_of_named(&params[1].ty, "Boolean")
        && ret_ty
            .as_ref()
            .is_some_and(|ty| Checker::is_named_type(ty, "Boolean"))
}

fn special_form_shape_pair_constructor(
    params: &[ResolvedValueParameter],
    ret_ty: &Option<AstTy>,
) -> bool {
    params.len() == 2
        && Checker::is_named_type(&params[0].ty, "$A")
        && Checker::is_named_type(&params[1].ty, "$B")
        && matches!(
            ret_ty,
            Some(AstTy::Tuple(_, items))
                if matches!(items.as_slice(), [first, second]
                    if Checker::is_named_type(first, "$A") && Checker::is_named_type(second, "$B"))
        )
}

impl Checker {
    pub(in crate::checker) fn check_builtin_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        return_type_arguments: &[ResolvedReturnTypeArgument],
        params: &[ResolvedValueParameter],
        ret_ty: &Option<sigil::resolved::ResolvedSignatureTy>,
        where_clause: Option<&ResolvedWhereClause>,
    ) -> Result<TypedNode, TypeError> {
        if Self::is_compiler_flow_operator_decl(id) {
            if !crate::checker::signatures::builtin_surface_matches(
                id,
                return_type_arguments,
                params,
                ret_ty.as_ref(),
                where_clause,
            ) {
                return Err(TypeError::new(
                    format!(
                        "Builtin {} does not match its canonical surface signature",
                        id.name
                    ),
                    span.clone(),
                ));
            }
            return Ok(TypedNode {
                ty: Ty::Unit,
                span: span.clone(),
                node: TypedInner::Lit(Lit::Unit),
            });
        }
        let declared_name = id.name.rsplit("::").next().unwrap_or(&id.name);
        let is_special_form = Self::is_special_form_builtin_decl_name(declared_name);
        if is_special_form {
            let syntax_ret_ty = ret_ty.as_ref().map(|ty| ty.syntax.clone());
            return self.check_special_form_builtin_decl(span, id, params, &syntax_ret_ty);
        }

        let builtin_name =
            sindr::builtin::builtin_runtime_name(declared_name, id.qualified_name.as_deref());
        let meta = sindr::builtin::builtin_meta_by_name(builtin_name).ok_or_else(|| TypeError {
            structured: None,
            message: format!("Unknown builtin declaration: {}", id.name),
            span: span.clone(),
            hint: None,
        })?;
        if params.len() != usize::from(meta.runtime_arity()) {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Builtin {} arity mismatch: expected {}, got {}",
                    id.name,
                    meta.runtime_arity(),
                    params.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }

        if !crate::checker::signatures::builtin_surface_matches(
            id,
            return_type_arguments,
            params,
            ret_ty.as_ref(),
            where_clause,
        ) {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Builtin {} does not match its canonical surface signature",
                    id.name
                ),
                span: span.clone(),
                hint: Some(
                    "Update the @builtin declaration to match BUILTIN_METAS, including its owner and type shape."
                        .into(),
                ),
            });
        }

        let mut tyvars = HashMap::new();
        let return_type_argument_tys = return_type_arguments
            .iter()
            .map(|argument| {
                self.resolve_builtin_signature_ty_in_context(
                    &argument.ty,
                    TypeSyntaxContext::General,
                    &mut tyvars,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let param_tys = params
            .iter()
            .map(|param| {
                self.resolve_builtin_signature_ty_in_context(
                    &param.ty,
                    TypeSyntaxContext::StdBuiltinParameter,
                    &mut tyvars,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ret = match ret_ty {
            Some(ty) => self.resolve_builtin_signature_ty_in_context(
                ty,
                TypeSyntaxContext::FunctionReturn,
                &mut tyvars,
            )?,
            None => Ty::Unit,
        };

        let canonical_where_constraints = crate::checker::signatures::canonical_where_constraints(
            self,
            where_clause,
            &mut tyvars,
        )?;
        self.callable_signatures.insert(
            id.unique_id,
            Arc::new(crate::checker::signatures::canonical_callable_signature(
                self,
                id,
                return_type_arguments,
                params,
                return_type_argument_tys.as_slice(),
                param_tys.as_slice(),
                ret.clone(),
                canonical_where_constraints,
                sindr::signature::RuntimeTarget::Builtin(meta.builtin_id()),
                sindr::signature::CallableDeclarationKind::Builtin,
            )?),
        );

        self.env.bind_var(
            id.unique_id,
            Ty::BuiltinFunc {
                name: meta.name.into(),
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

    pub(in crate::checker) fn check_special_form_builtin_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        params: &[ResolvedValueParameter],
        ret_ty: &Option<AstTy>,
    ) -> Result<TypedNode, TypeError> {
        let contract = Self::special_form_contract(id.name.as_str());

        if Self::surface_qualified_name(id.qualified_name.as_deref())
            != Some(contract.expected_qname)
        {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Special-form declaration `{}` is only allowed at `{}`.",
                    id.name, contract.expected_qname
                ),
                span: span.clone(),
                hint: None,
            });
        }

        if !(contract.shape_ok)(params, ret_ty) {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Special-form declaration must match the canonical contract: {}",
                    contract.expected_signature
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let mut tyvars = HashMap::new();
        let param_tys = params
            .iter()
            .enumerate()
            .map(|(_index, param)| self.resolve_builtin_ast_ty(&param.ty, &mut tyvars))
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

    pub(in crate::checker) fn check_builtin_type_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        params: &[String],
        attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
        if let Some(members) = &attrs.facet_path_kind {
            if attrs.builtin {
                return Err(TypeError {
                    structured: None,
                    message: "@FacetPathKind Type declarations are only allowed in canonical lib/facet.srt".into(),
                    span: span.clone(),
                    hint: None,
                });
            }
            if !params.is_empty() {
                return Err(TypeError {
                    structured: None,
                    message: "Facet path kind types cannot have generic parameters".into(),
                    span: span.clone(),
                    hint: None,
                });
            }
            let atomic = matches!(
                id.name.as_str(),
                "InfallibleStructural" | "FallibleStructural" | "VariantPath"
            );
            if members.is_empty() {
                if !atomic {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Facet path kind `{}` must be an alias or one of the compiler-derived atomic kinds",
                            id.name
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
            } else {
                if atomic {
                    return Err(TypeError {
                        structured: None,
                        message: format!(
                            "Atomic Facet path kind `{}` cannot declare an alias RHS",
                            id.name
                        ),
                        span: span.clone(),
                        hint: None,
                    });
                }
                for member in members {
                    if !self.facet_path_kind_decls.contains_key(member) {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "Facet path kind alias `{}` must reference a previously declared kind; `{member}` is not available",
                                id.name
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                }
            }
            if self
                .facet_path_kind_decls
                .insert(id.name.clone(), members.clone())
                .is_some()
            {
                return Err(TypeError {
                    structured: None,
                    message: format!("Duplicate Facet path kind declaration: {}", id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
            return Ok(TypedNode {
                ty: Ty::Unit,
                span: span.clone(),
                node: TypedInner::Lit(Lit::Unit),
            });
        }
        let Some(meta) = builtin_type_meta_by_name(Self::surface_name(&id.name)) else {
            return Err(TypeError {
                structured: None,
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
                structured: None,
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
                    structured: None,
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

    pub(in crate::checker) fn require_concrete_extractor_target_head(
        &self,
        target_ty: &Ty,
        span: &Span,
        extractor_name: &str,
    ) -> Result<(), TypeError> {
        match self.resolve_ty(target_ty) {
            Ty::Var(_) | Ty::SelfApp(_) | Ty::Hole => Err(TypeError {
                structured: None,
                message: format!(
                    "Extractor target type must have a concrete head: {extractor_name}"
                ),
                span: span.clone(),
                hint: Some(
                    "Use a concrete target such as Int, String, List<$T>, or Result<$T>. Put type-wide or Trait-constrained computation in a normal function."
                        .into(),
                ),
            }),
            _ => Ok(()),
        }
    }

    pub(in crate::checker) fn check_builtin_extractor_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        params: &[ResolvedExtractorParam],
        ret_ty: &AstTy,
    ) -> Result<TypedNode, TypeError> {
        let mut tyvars = HashMap::new();
        let param_tys = params
            .iter()
            .map(|param| match &param.ty {
                Some(ty) => self.resolve_builtin_ast_ty_in_context(
                    ty,
                    TypeSyntaxContext::General,
                    &mut tyvars,
                ),
                None => Ok(self.env.fresh_tyvar()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if param_tys.is_empty() {
            return Err(TypeError::new(
                "Extractor requires at least one input",
                span.clone(),
            ));
        }
        let target_param = params.last().expect("non-empty Extractor parameters");
        let target_span = target_param
            .ty
            .as_ref()
            .map(Self::ast_ty_span)
            .unwrap_or(&target_param.id.span);
        self.require_concrete_extractor_target_head(
            param_tys
                .last()
                .expect("non-empty Extractor parameter types"),
            target_span,
            &id.name,
        )?;
        let ret = self.resolve_builtin_ast_ty_in_context(
            ret_ty,
            TypeSyntaxContext::ExtractorReturn,
            &mut tyvars,
        )?;
        self.require_extractor_match_result_payload_ty(
            &ret,
            span,
            &format!("Extractor {}", id.name),
        )?;

        self.env.bind_var(
            id.unique_id,
            Ty::BuiltinFunc {
                name: id.name.clone(),
                params: param_tys.clone(),
                ret: Box::new(ret.clone()),
            },
        );

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::BuiltinExtractorDecl(id.clone(), param_tys, ret),
        })
    }

    pub(in crate::checker) fn is_named_type(ast_ty: &AstTy, expected_name: &str) -> bool {
        matches!(ast_ty, AstTy::Named(_, name) if name == expected_name)
    }

    pub(in crate::checker) fn is_unit_type(ast_ty: &AstTy) -> bool {
        Self::is_named_type(ast_ty, "Unit")
    }

    pub(in crate::checker) fn is_lazy_of_named(ast_ty: &AstTy, expected_name: &str) -> bool {
        matches!(
            ast_ty,
            AstTy::Generic(_, name, args)
                if name == "Lazy"
                    && args.len() == 1
                    && matches!(&args[0], AstTy::Named(_, param_name) if param_name == expected_name)
        )
    }

    pub(in crate::checker) fn is_lazy_of_unit(ast_ty: &AstTy) -> bool {
        Self::is_lazy_of_named(ast_ty, "Unit")
    }

    pub(in crate::checker) fn is_unary_func_from_named_to_named(
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

    pub(in crate::checker) fn is_special_form_builtin_decl_name(name: &str) -> bool {
        matches!(
            name,
            "if_let"
                | "if_let_then"
                | "is_match"
                | "apply_pattern"
                | "assert_error_matches"
                | "cause"
                | "and"
                | "or"
                | "eq"
                | "neq"
                | "concat"
                | "(,)"
        )
    }

    pub(in crate::checker) fn validate_standard_lazy_function_decl(
        &self,
        span: &Span,
        id: &ResolvedId,
        params: &[ResolvedValueParameter],
        ret_ty: &Option<sigil::resolved::ResolvedSignatureTy>,
    ) -> Result<(), TypeError> {
        if !self.enforce_builtin_type_contracts {
            return Ok(());
        }
        let qname = Self::surface_qualified_name(id.qualified_name.as_deref());
        let name = match qname {
            Some("Kernel::if") => "if",
            Some("Kernel::if_then") => "if_then",
            Some("Kernel::require") => "require",
            Some("Kernel::ensure") => "ensure",
            Some("Result::map_err") => "map_err",
            Some("Result::or_else") => "or_else",
            _ => return Ok(()),
        };
        let contract = Self::special_form_contract(name);
        let annotation = ret_ty.as_ref().map(|ty| ty.syntax().clone());
        if !(contract.shape_ok)(params, &annotation) {
            return Err(TypeError::new(
                format!(
                    "Standard Lazy declaration must match the canonical contract: {}",
                    contract.expected_signature
                ),
                span.clone(),
            ));
        }
        Ok(())
    }

    pub(in crate::checker) fn is_compiler_flow_operator_decl(id: &ResolvedId) -> bool {
        matches!(id.name.as_str(), "|>" | ">>" | ">*" | ">=>")
            && Self::surface_qualified_name(id.qualified_name.as_deref())
                == Some(format!("Bootstrap::{}", id.name).as_str())
    }

    fn special_form_contract(name: &str) -> SpecialFormContract {
        match name {
            "if" => SpecialFormContract {
                expected_qname: "Kernel::if",
                expected_signature: "def if(flag: Boolean, then_branch: Lazy<$A>, else_branch: Lazy<$A>) -> $A",
                shape_ok: special_form_shape_if,
            },
            "if_then" => SpecialFormContract {
                expected_qname: "Kernel::if_then",
                expected_signature: "def if_then(flag: Boolean, then_branch: Lazy<Unit>) -> Unit",
                shape_ok: special_form_shape_if_then,
            },
            "if_let" => SpecialFormContract {
                expected_qname: "Kernel::if_let",
                expected_signature: "@builtin def if_let(value: $A, pattern: $Pattern, then_branch: Lazy<$B>, else_branch: Lazy<$B>) -> $B",
                shape_ok: special_form_shape_if_let,
            },
            "if_let_then" => SpecialFormContract {
                expected_qname: "Kernel::if_let_then",
                expected_signature: "@builtin def if_let_then(value: $A, pattern: $Pattern, then_branch: Lazy<Unit>) -> Unit",
                shape_ok: special_form_shape_if_let_then,
            },
            "apply_pattern" => SpecialFormContract {
                expected_qname: "Kernel::apply_pattern",
                expected_signature: "@builtin def apply_pattern(value: $Value, pattern: $Pattern) -> Result<$Return>",
                shape_ok: special_form_shape_apply_pattern,
            },
            "assert_error_matches" => SpecialFormContract {
                expected_qname: "Test::assert_error_matches",
                expected_signature: "@builtin def assert_error_matches(error: Error, pattern: $Pattern, check: Lazy<Result<()>>) -> Result<()>",
                shape_ok: special_form_shape_assert_error_matches,
            },
            "is_match" => SpecialFormContract {
                expected_qname: "Kernel::is_match",
                expected_signature: "@builtin def is_match(value: $A, pattern: $Pattern) -> Boolean",
                shape_ok: special_form_shape_is_match,
            },
            "require" => SpecialFormContract {
                expected_qname: "Kernel::require",
                expected_signature: "def require(flag: Boolean, err: Lazy<Error>) -> Result<Unit>",
                shape_ok: special_form_shape_require,
            },
            "ensure" => SpecialFormContract {
                expected_qname: "Kernel::ensure",
                expected_signature: "def ensure(value: $A, pred: ($A -> Boolean), err: Lazy<Error>) -> Result<$A>",
                shape_ok: special_form_shape_ensure,
            },
            "map_err" => SpecialFormContract {
                expected_qname: "Result::map_err",
                expected_signature: "def map_err(result: Result<$T>, err: Lazy<Error>) -> Result<$T>",
                shape_ok: special_form_shape_map_err_or_cause,
            },
            "cause" => SpecialFormContract {
                expected_qname: "Result::cause",
                expected_signature: "@builtin def cause(result: Result<$T>, err: Lazy<Error>) -> Result<$T>",
                shape_ok: special_form_shape_map_err_or_cause,
            },
            "or_else" => SpecialFormContract {
                expected_qname: "Result::or_else",
                expected_signature: "def or_else(value: Result<$T>, next: Lazy<Result<$T>>) -> Result<$T>",
                shape_ok: special_form_shape_or_else,
            },
            "and" => SpecialFormContract {
                expected_qname: "Kernel::and",
                expected_signature: "@builtin def and(left: Boolean, right: Lazy<Boolean>) -> Boolean",
                shape_ok: special_form_shape_and_or,
            },
            "or" => SpecialFormContract {
                expected_qname: "Kernel::or",
                expected_signature: "@builtin def or(left: Boolean, right: Lazy<Boolean>) -> Boolean",
                shape_ok: special_form_shape_and_or,
            },
            "(,)" => SpecialFormContract {
                expected_qname: "Bootstrap::(,)",
                expected_signature: "@builtin def (,)(lhs: $A, rhs: $B) -> ($A, $B)",
                shape_ok: special_form_shape_pair_constructor,
            },
            _ => unreachable!(),
        }
    }

    pub(in crate::checker) fn is_result_of_named(ast_ty: &AstTy, expected_name: &str) -> bool {
        matches!(ast_ty, AstTy::Generic(_, name, args)
            if name == "Result"
                && (args.len() == 1 || (args.len() == 2 && matches!(&args[1], AstTy::Named(_, error) if error == "Error")))
                && matches!(&args[0], AstTy::Named(_, param_name) if param_name == expected_name))
    }

    pub(in crate::checker) fn ensure_builtin_type_contracts(&self) -> Result<(), TypeError> {
        if !self.enforce_builtin_type_contracts {
            return Ok(());
        }

        for meta in builtin_type_head_metas() {
            if !self.seen_builtin_type_decls.contains_key(meta.name) {
                return Err(TypeError {
                    structured: None,
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

    pub(in crate::checker) fn validate_builtin_enum_shape(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        type_params: &[ResolvedTypeParam],
        variants: &[ResolvedEnumVariant],
        attrs: &ResolvedDeclAttrs,
    ) -> Result<(), TypeError> {
        if attrs.builtin {
            match Self::surface_name(&id.name) {
                "Result" => {
                    if type_params.len() != 1
                        || variants.len() != 2
                        || variants[0].id.name.rsplit("::").next() != Some("Ok")
                        || variants[1].id.name.rsplit("::").next() != Some("Err")
                        || variants[0].payload.len() != 1
                        || variants[1].payload.len() != 1
                        || !matches!(&variants[0].payload[0], AstTy::Named(_, name) if name == "$T")
                        || !matches!(&variants[1].payload[0], AstTy::Named(_, name) if name == "Error")
                    {
                        return Err(TypeError {
                            structured: None,
                            message: "Builtin Result enum must match `defenum Result<$T> { Ok($T), Err(Error) }`.".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    self.seen_builtin_type_decls
                        .insert("Result".into(), (vec!["$T".into()], span.clone()));
                }
                "MatchResult" => {
                    if type_params.len() != 1
                        || variants.len() != 2
                        || variants[0].id.name.rsplit("::").next() != Some("Ok")
                        || variants[1].id.name.rsplit("::").next() != Some("Err")
                        || variants[0].payload.len() != 1
                        || variants[1].payload.len() != 1
                        || !matches!(&variants[0].payload[0], AstTy::Named(_, name) if name == &type_params[0].name)
                        || !matches!(&variants[1].payload[0], AstTy::Named(_, name) if name == "Error")
                    {
                        return Err(TypeError::new(
                            "Builtin MatchResult must define Ok($Value) and Err(Error)",
                            span.clone(),
                        ));
                    }
                    self.seen_builtin_type_decls.insert(
                        "MatchResult".into(),
                        (vec![type_params[0].name.clone()], span.clone()),
                    );
                }
                "Boolean" => {
                    if !type_params.is_empty()
                        || variants.len() != 2
                        || variants[0].id.name.rsplit("::").next() != Some("True")
                        || variants[1].id.name.rsplit("::").next() != Some("False")
                        || !variants.iter().all(|variant| variant.payload.is_empty())
                    {
                        return Err(TypeError {
                            structured: None,
                            message:
                                "Builtin Boolean enum must match `defenum Boolean { True, False }`."
                                    .into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    self.seen_builtin_type_decls
                        .insert("Boolean".into(), (Vec::new(), span.clone()));
                }
                _ => {}
            }
        }

        Ok(())
    }
}
