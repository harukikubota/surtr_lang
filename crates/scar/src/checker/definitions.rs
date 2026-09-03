use super::*;

struct SpecialFormBuiltinContract {
    expected_qname: &'static str,
    expected_signature: &'static str,
    shape_ok: fn(&[ResolvedValueParameter], &Option<AstTy>) -> bool,
}

#[cfg(test)]
mod contextual_capability_tests {
    use super::*;

    #[test]
    fn declared_method_generic_absent_from_signature_still_tracks_unused_capability() {
        let span = Span { start: 10, end: 16 };
        let mut checker = Checker::new(TypecheckContext::default());
        let declared = vec![ResolvedTypeParam {
            name: "$T".into(),
            bound: None,
            span: span.clone(),
        }];
        let mut method_tyvars = HashMap::new();
        checker.seed_missing_method_type_params(&declared, &mut method_tyvars);
        let marker_id = ResolvedId {
            name: "Marker".into(),
            qualified_name: Some("Marker".into()),
            unique_id: 1,
            compiler_generated: false,
            symbol_info: None,
            span: span.clone(),
        };
        let where_clause = ResolvedWhereClause {
            constraints: vec![ResolvedWhereConstraint {
                subject: AstTy::Named(span.clone(), "$T".into()),
                bounds: vec![ResolvedWhereConstraintRhs::Trait {
                    trait_id: marker_id,
                }],
                span: span.clone(),
            }],
            span: span.clone(),
        };
        let capabilities = checker
            .resolved_capability_uses(Some(&where_clause), &method_tyvars)
            .expect("declared method capability should resolve");

        let err = checker
            .check_body_in_isolated_scope(
                &[],
                &[],
                &capabilities,
                &mut [],
                method_tyvars.clone(),
                Checker::signature_tyvar_ids(&method_tyvars),
                Ty::Unit,
                "unused".into(),
                None,
                false,
                &Resolved::Lit(span.clone(), Lit::Unit),
            )
            .expect_err("a where-only declared method generic must remain an unused capability");

        assert!(err.message.contains("UnusedTraitConstraint"), "{err:?}");
        assert_eq!(err.span, span);
    }
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

fn special_form_shape_assert(params: &[ResolvedValueParameter], ret_ty: &Option<AstTy>) -> bool {
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

fn special_form_shape_recover_kind(
    params: &[ResolvedValueParameter],
    ret_ty: &Option<AstTy>,
) -> bool {
    params.len() == 3
        && Checker::is_result_of_named(&params[0].ty, "$A")
        && Checker::is_lazy_of_named(&params[1].ty, "Error")
        && Checker::is_unary_func_from_named_to_result(&params[2].ty, "Error", "$A")
        && ret_ty
            .as_ref()
            .is_some_and(|ty| Checker::is_result_of_named(ty, "$A"))
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
    fn bare_return_typevar_result_mismatch(
        &self,
        expected_ret: &Ty,
        actual_ret: &Ty,
        span: &Span,
    ) -> Option<TypeError> {
        match (self.resolve_ty(expected_ret), self.resolve_ty(actual_ret)) {
            (Ty::Var(_), Ty::Result(_, _)) => Some(TypeError {
                message: format!(
                    "expected {}, got {}",
                    self.ty_name(expected_ret),
                    self.ty_name(actual_ret)
                ),
                span: span.clone(),
                hint: Some(
                    "A plain return type variable cannot be satisfied by Err(...). Declare Result<$T> when the function propagates failures."
                        .into(),
                ),
            }),
            _ => None,
        }
    }

    pub(super) fn check_builtin_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        params: &[ResolvedValueParameter],
        ret_ty: &Option<AstTy>,
        where_clause: Option<&ResolvedWhereClause>,
    ) -> Result<TypedNode, TypeError> {
        let is_kernel_is_match = id.name == "is_match"
            && Self::surface_qualified_name(id.qualified_name.as_deref())
                == Some("Kernel::is_match");
        let is_special_form = if id.name == "is_match" {
            is_kernel_is_match
        } else {
            Self::is_special_form_builtin_decl_name(&id.name)
        };
        if is_special_form {
            return self.check_special_form_builtin_decl(span, id, params, ret_ty);
        }

        let builtin_name =
            sindr::builtin::builtin_runtime_name(&id.name, id.qualified_name.as_deref());
        let meta = sindr::builtin::builtin_meta_by_name(builtin_name).ok_or_else(|| TypeError {
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

        if let Some(clause) = where_clause {
            self.builtin_contracts.insert(
                id.unique_id,
                BuiltinContract {
                    where_clause: TypedWhereClause::from(clause),
                    type_vars: tyvars.clone(),
                    param_tys: param_tys.clone(),
                },
            );
        }

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

    pub(super) fn check_special_form_builtin_decl(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        params: &[ResolvedValueParameter],
        ret_ty: &Option<AstTy>,
    ) -> Result<TypedNode, TypeError> {
        let contract = Self::special_form_builtin_contract(id.name.as_str());

        if Self::surface_qualified_name(id.qualified_name.as_deref())
            != Some(contract.expected_qname)
        {
            return Err(TypeError {
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
        attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
        if let Some(members) = &attrs.facet_path_kind {
            if attrs.builtin {
                return Err(TypeError {
                    message: "@FacetPathKind Type declarations are only allowed in canonical lib/facet.srt".into(),
                    span: span.clone(),
                    hint: None,
                });
            }
            if !params.is_empty() {
                return Err(TypeError {
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
                        message: format!("Facet path kind `{}` must be an alias or one of the compiler-derived atomic kinds", id.name),
                        span: span.clone(),
                        hint: None,
                    });
                }
            } else {
                if atomic {
                    return Err(TypeError {
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
                            message: format!("Facet path kind alias `{}` must reference a previously declared kind; `{member}` is not available", id.name),
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
            TypeSyntaxContext::ExtractorReturn,
            &mut tyvars,
        )?;
        self.require_extractor_option_payload_ty(
            &ret,
            &param.id.span,
            &format!("Extractor {}", id.name),
        )?;

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

        if Self::surface_qualified_name(id.qualified_name.as_deref()) != Some(expected_qname) {
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
                "Ok" => "@builtin type Ok($T) -> Result<$T>",
                "Err" => "@builtin type Err(Error) -> Result<$T>",
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

    pub(super) fn is_lazy_of_named(ast_ty: &AstTy, expected_name: &str) -> bool {
        matches!(
            ast_ty,
            AstTy::Generic(_, name, args)
                if name == "Lazy"
                    && args.len() == 1
                    && matches!(&args[0], AstTy::Named(_, param_name) if param_name == expected_name)
        )
    }

    pub(super) fn is_lazy_of_unit(ast_ty: &AstTy) -> bool {
        Self::is_lazy_of_named(ast_ty, "Unit")
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

    pub(super) fn is_unary_func_from_named_to_result(
        ast_ty: &AstTy,
        expected_param_name: &str,
        expected_result_name: &str,
    ) -> bool {
        matches!(
            ast_ty,
            AstTy::Func(_, params, ret)
                if params.len() == 1
                    && matches!(&params[0], AstTy::Named(_, name) if name == expected_param_name)
                    && Self::is_result_of_named(ret.as_ref(), expected_result_name)
        )
    }

    pub(super) fn is_special_form_builtin_decl_name(name: &str) -> bool {
        matches!(
            name,
            "if" | "if_then"
                | "if_let"
                | "if_let_then"
                | "is_match"
                | "assert"
                | "ensure"
                | "map_err"
                | "cause"
                | "recover_kind"
                | "and"
                | "or"
                | "eq"
                | "neq"
                | "concat"
                | "(,)"
        )
    }

    fn special_form_builtin_contract(name: &str) -> SpecialFormBuiltinContract {
        match name {
            "if" => SpecialFormBuiltinContract {
                expected_qname: "Kernel::if",
                expected_signature:
                    "@builtin def if(flag: Boolean, then_branch: Lazy<$A>, else_branch: Lazy<$A>) -> $A",
                shape_ok: special_form_shape_if,
            },
            "if_then" => SpecialFormBuiltinContract {
                expected_qname: "Kernel::if_then",
                expected_signature:
                    "@builtin def if_then(flag: Boolean, then_branch: Lazy<Unit>) -> Unit",
                shape_ok: special_form_shape_if_then,
            },
            "if_let" => SpecialFormBuiltinContract {
                expected_qname: "Kernel::if_let",
                expected_signature:
                    "@builtin def if_let(value: $A, pattern: $Pattern, then_branch: Lazy<$B>, else_branch: Lazy<$B>) -> $B",
                shape_ok: special_form_shape_if_let,
            },
            "if_let_then" => SpecialFormBuiltinContract {
                expected_qname: "Kernel::if_let_then",
                expected_signature:
                    "@builtin def if_let_then(value: $A, pattern: $Pattern, then_branch: Lazy<Unit>) -> Unit",
                shape_ok: special_form_shape_if_let_then,
            },
            "is_match" => SpecialFormBuiltinContract {
                expected_qname: "Kernel::is_match",
                expected_signature:
                    "@builtin def is_match(value: $A, pattern: $Pattern) -> Boolean",
                shape_ok: special_form_shape_is_match,
            },
            "assert" => SpecialFormBuiltinContract {
                expected_qname: "Kernel::assert",
                expected_signature:
                    "@builtin def assert(flag: Boolean, err: Lazy<Error>) -> Result<Unit>",
                shape_ok: special_form_shape_assert,
            },
            "ensure" => SpecialFormBuiltinContract {
                expected_qname: "Kernel::ensure",
                expected_signature:
                    "@builtin def ensure(value: $A, pred: ($A -> Boolean), err: Lazy<Error>) -> Result<$A>",
                shape_ok: special_form_shape_ensure,
            },
            "map_err" => SpecialFormBuiltinContract {
                expected_qname: "Result::map_err",
                expected_signature:
                    "@builtin def map_err(result: Result<$T>, err: Lazy<Error>) -> Result<$T>",
                shape_ok: special_form_shape_map_err_or_cause,
            },
            "cause" => SpecialFormBuiltinContract {
                expected_qname: "Result::cause",
                expected_signature:
                    "@builtin def cause(result: Result<$T>, err: Lazy<Error>) -> Result<$T>",
                shape_ok: special_form_shape_map_err_or_cause,
            },
            "recover_kind" => SpecialFormBuiltinContract {
                expected_qname: "Result::recover_kind",
                expected_signature:
                    "@builtin def recover_kind(value: Result<$A>, marker: Lazy<Error>, handler: (Error -> Result<$A>)) -> Result<$A>",
                shape_ok: special_form_shape_recover_kind,
            },
            "and" => SpecialFormBuiltinContract {
                expected_qname: "Kernel::and",
                expected_signature:
                    "@builtin def and(left: Boolean, right: Lazy<Boolean>) -> Boolean",
                shape_ok: special_form_shape_and_or,
            },
            "or" => SpecialFormBuiltinContract {
                expected_qname: "Kernel::or",
                expected_signature:
                    "@builtin def or(left: Boolean, right: Lazy<Boolean>) -> Boolean",
                shape_ok: special_form_shape_and_or,
            },
            "(,)" => SpecialFormBuiltinContract {
                expected_qname: "Bootstrap::(,)",
                expected_signature: "@builtin def (,)(lhs: $A, rhs: $B) -> ($A, $B)",
                shape_ok: special_form_shape_pair_constructor,
            },
            _ => unreachable!(),
        }
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

        for meta in builtin_type_head_metas() {
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

    pub(super) fn resolved_capability_uses(
        &mut self,
        where_clause: Option<&ResolvedWhereClause>,
        tyvars: &HashMap<String, Ty>,
    ) -> Result<Vec<CapabilityUse>, TypeError> {
        let Some(where_clause) = where_clause else {
            return Ok(Vec::new());
        };
        let mut uses = Vec::new();
        for constraint in &where_clause.constraints {
            let subject = match &constraint.subject {
                AstTy::Named(_, name) | AstTy::Generic(_, name, _) if tyvars.contains_key(name) => {
                    tyvars.get(name).cloned().expect("checked above")
                }
                _ => continue,
            };
            let subject_ty = self.resolve_ty(&subject);
            let subject_name = Self::surface_ast_ty(&constraint.subject);
            for bound in &constraint.bounds {
                if let ResolvedWhereConstraintRhs::Trait { trait_id } = bound {
                    uses.push(CapabilityUse {
                        subject_ty: subject_ty.clone(),
                        subject_name: subject_name.clone(),
                        trait_id: self.trait_key(trait_id),
                        span: trait_id.span.clone(),
                        consumed: false,
                    });
                }
            }
        }
        Ok(uses)
    }

    fn typed_capability_uses(
        &mut self,
        where_clause: Option<&TypedWhereClause>,
        tyvars: &HashMap<String, Ty>,
    ) -> Vec<CapabilityUse> {
        let Some(where_clause) = where_clause else {
            return Vec::new();
        };
        let mut uses = Vec::new();
        for constraint in &where_clause.constraints {
            let subject = match &constraint.subject {
                AstTy::Named(_, name) | AstTy::Generic(_, name, _) if tyvars.contains_key(name) => {
                    tyvars.get(name).cloned().expect("checked above")
                }
                _ => continue,
            };
            let subject_ty = self.resolve_ty(&subject);
            let subject_name = Self::surface_ast_ty(&constraint.subject);
            for bound in &constraint.bounds {
                if let TypedWhereConstraintRhs::Trait { trait_id } = bound {
                    uses.push(CapabilityUse {
                        subject_ty: subject_ty.clone(),
                        subject_name: subject_name.clone(),
                        trait_id: self.trait_key(trait_id),
                        span: trait_id.span.clone(),
                        consumed: false,
                    });
                }
            }
        }
        uses
    }

    pub(super) fn seed_missing_method_type_params(
        &mut self,
        type_params: &[ResolvedTypeParam],
        tyvars: &mut HashMap<String, Ty>,
    ) {
        let missing = type_params
            .iter()
            .filter(|param| !tyvars.contains_key(&param.name))
            .cloned()
            .collect::<Vec<_>>();
        self.seed_signature_type_params(&missing, tyvars);
    }

    fn collect_declared_type_var_instances(
        pattern: &Ty,
        actual: &Ty,
        declared_vars: &HashSet<u32>,
        instances: &mut HashMap<u32, Ty>,
    ) {
        match (pattern, actual) {
            (Ty::Var(var), actual) if declared_vars.contains(var) => {
                instances.entry(*var).or_insert_with(|| actual.clone());
            }
            (Ty::List(pattern), Ty::List(actual)) | (Ty::Lazy(pattern), Ty::Lazy(actual)) => {
                Self::collect_declared_type_var_instances(
                    pattern,
                    actual,
                    declared_vars,
                    instances,
                );
            }
            (Ty::Result(pattern_ok, pattern_err), Ty::Result(actual_ok, actual_err)) => {
                Self::collect_declared_type_var_instances(
                    pattern_ok,
                    actual_ok,
                    declared_vars,
                    instances,
                );
                Self::collect_declared_type_var_instances(
                    pattern_err,
                    actual_err,
                    declared_vars,
                    instances,
                );
            }
            (Ty::Tuple(patterns), Ty::Tuple(actuals))
            | (Ty::SelfApp(patterns), Ty::SelfApp(actuals))
            | (Ty::Enum(_, patterns), Ty::Enum(_, actuals)) => {
                for (pattern, actual) in patterns.iter().zip(actuals.iter()) {
                    Self::collect_declared_type_var_instances(
                        pattern,
                        actual,
                        declared_vars,
                        instances,
                    );
                }
            }
            (Ty::Struct(_, patterns), Ty::Struct(_, actuals))
            | (Ty::Record(_, patterns), Ty::Record(_, actuals)) => {
                for ((_, pattern), (_, actual)) in patterns.iter().zip(actuals.iter()) {
                    Self::collect_declared_type_var_instances(
                        pattern,
                        actual,
                        declared_vars,
                        instances,
                    );
                }
            }
            (Ty::Func(pattern_params, pattern_ret), Ty::Func(actual_params, actual_ret)) => {
                for (pattern, actual) in pattern_params.iter().zip(actual_params.iter()) {
                    Self::collect_declared_type_var_instances(
                        pattern,
                        actual,
                        declared_vars,
                        instances,
                    );
                }
                Self::collect_declared_type_var_instances(
                    pattern_ret,
                    actual_ret,
                    declared_vars,
                    instances,
                );
            }
            _ => {}
        }
    }

    pub(super) fn resolved_named_type_args(
        &self,
        type_name: &str,
        resolved_ty: &Ty,
    ) -> Option<Vec<Ty>> {
        let def = self.env.lookup_type_def(type_name)?;
        let resolved_fields = match resolved_ty {
            Ty::Struct(_, fields) | Ty::Record(_, fields) => fields,
            _ => return None,
        };
        let declared_vars = def.type_param_vars.iter().copied().collect::<HashSet<_>>();
        let mut instances = HashMap::new();
        for ((_, pattern), (_, actual)) in def.fields.iter().zip(resolved_fields.iter()) {
            Self::collect_declared_type_var_instances(
                pattern,
                actual,
                &declared_vars,
                &mut instances,
            );
        }
        def.type_param_vars
            .iter()
            .map(|var| instances.get(var).cloned())
            .collect()
    }

    pub(super) fn collect_signature_ty_bindings(
        &self,
        ast_ty: &AstTy,
        resolved_ty: &Ty,
        bindings: &mut HashMap<String, Ty>,
    ) {
        match (ast_ty, resolved_ty) {
            (AstTy::Named(_, name), ty) if name.starts_with('$') => {
                bindings.insert(name.clone(), ty.clone());
            }
            (AstTy::Generic(_, name, args), Ty::SelfApp(items))
                if name != "Self" && Self::constructor_application_parts(items).is_some() =>
            {
                let (_, slots) = Self::constructor_application_parts(items).expect("checked above");
                for (arg, slot) in args.iter().zip(slots.iter()) {
                    self.collect_signature_ty_bindings(arg, slot, bindings);
                }
            }
            (AstTy::Generic(_, name, args), Ty::Struct(_, _) | Ty::Record(_, _)) => {
                if let Some(resolved_args) = self.resolved_named_type_args(name, resolved_ty) {
                    for (arg, resolved) in args.iter().zip(resolved_args.iter()) {
                        self.collect_signature_ty_bindings(arg, resolved, bindings);
                    }
                }
            }
            (AstTy::Generic(_, _, args), Ty::List(inner)) if args.len() == 1 => {
                self.collect_signature_ty_bindings(&args[0], inner, bindings);
            }
            (AstTy::Generic(_, _, args), Ty::Result(ok, err)) => {
                if let Some(arg) = args.first() {
                    self.collect_signature_ty_bindings(arg, ok, bindings);
                }
                if let Some(arg) = args.get(1) {
                    self.collect_signature_ty_bindings(arg, err, bindings);
                }
            }
            (AstTy::Generic(_, _, args), Ty::Enum(_, resolved_args)) => {
                for (arg, resolved) in args.iter().zip(resolved_args.iter()) {
                    self.collect_signature_ty_bindings(arg, resolved, bindings);
                }
            }
            (AstTy::Tuple(_, items), Ty::Tuple(resolved_items)) => {
                for (item, resolved) in items.iter().zip(resolved_items.iter()) {
                    self.collect_signature_ty_bindings(item, resolved, bindings);
                }
            }
            (AstTy::Func(_, params, ret), Ty::Func(resolved_params, resolved_ret)) => {
                for (param, resolved) in params.iter().zip(resolved_params.iter()) {
                    self.collect_signature_ty_bindings(param, resolved, bindings);
                }
                self.collect_signature_ty_bindings(ret, resolved_ret, bindings);
            }
            _ => {}
        }
    }

    pub(super) fn resolve_contextual_return_body(
        &mut self,
        return_ast: &AstTy,
        expected_ret: &Ty,
        typed_body: &TypedNode,
        rigid_tyvars: &HashSet<u32>,
    ) -> Result<Option<Ty>, TypeError> {
        let Some(trait_key) = self.constructor_trait_key_for_ast_ty(return_ast) else {
            return Ok(None);
        };
        let actual = self.resolve_ty(&typed_body.ty);
        let has_concrete_constructor_shape = Self::constructor_application_slots(&actual).is_some();
        let expected_parts = match expected_ret {
            Ty::SelfApp(items) => Self::constructor_application_parts(items),
            _ => None,
        };
        let Some((witness, expected_slots)) = expected_parts else {
            unreachable!("constructor result annotation must lower to a contextual application")
        };
        if matches!(actual, Ty::Var(_) | Ty::SelfApp(_)) || !has_concrete_constructor_shape {
            return Err(TypeError {
                message: format!(
                    "UnresolvedConstructorResult: {} must resolve to one concrete constructor",
                    Self::surface_ast_ty(return_ast)
                ),
                span: self.return_mismatch_span(typed_body),
                hint: Some(
                    "Return a concrete constructor value from every branch of this function."
                        .into(),
                ),
            });
        }
        if !self.trait_impl_exists(&trait_key, &actual) {
            return Err(TypeError {
                message: format!(
                    "{} does not implement constructor trait {}",
                    self.ty_name(&actual),
                    Self::surface_name(&trait_key)
                ),
                span: self.return_mismatch_span(typed_body),
                hint: None,
            });
        }
        let concrete_slots = self
            .constructor_application_slots_for_trait(&trait_key, &actual)
            .expect("a validated constructor-trait impl must expose its declared slots");
        if expected_slots.len() != concrete_slots.len()
            || !expected_slots
                .iter()
                .zip(concrete_slots.iter())
                .all(|(expected, actual)| {
                    self.types_compatible_with_rigid(expected, actual, rigid_tyvars)
                })
            || !self.bind_tyvar(
                match witness {
                    Ty::Var(var) => *var,
                    _ => unreachable!("fresh constructor witness must be a type variable"),
                },
                &actual,
            )
        {
            return Err(TypeError {
                message: format!(
                    "expected {}, got {}",
                    self.ty_name(expected_ret),
                    self.ty_name(&actual)
                ),
                span: self.return_mismatch_span(typed_body),
                hint: None,
            });
        }
        Ok(Some(actual))
    }

    pub(super) fn check_body_in_isolated_scope(
        &mut self,
        local_bindings: &[(u32, Ty)],
        local_capabilities: &[(u32, String)],
        declaration_capabilities: &[CapabilityUse],
        deferred_capabilities: &mut [CapabilityUse],
        local_annotation_tyvars: HashMap<String, Ty>,
        rigid_tyvars: HashSet<u32>,
        function_return_ty: Ty,
        function_symbol: String,
        impl_target: Option<String>,
        in_extractor_body: bool,
        body: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        let saved_function_return_ty = self.function_return_ty.clone();
        let saved_local_annotation_tyvars = self.local_annotation_tyvars.clone();
        let saved_rigid_tyvars = self.rigid_tyvars.clone();
        let saved_current_function_symbol = self.current_function_symbol.clone();
        let saved_current_impl_struct_target = self.current_impl_struct_target.clone();
        let saved_in_extractor_body = self.in_extractor_body;
        let saved_closure_depth = self.closure_depth;
        let saved_facet_bindings = self.facet_bindings.clone();
        let saved_constructor_capabilities = self.constructor_capabilities.clone();
        let saved_active_capabilities = self.active_capabilities.clone();

        self.env.push_var_scope();
        self.function_return_ty = Some(function_return_ty.clone());
        self.local_annotation_tyvars = local_annotation_tyvars;
        self.rigid_tyvars = rigid_tyvars;
        self.current_function_symbol = Some(function_symbol);
        self.current_impl_struct_target = impl_target;
        self.in_extractor_body = in_extractor_body;
        self.active_capabilities = declaration_capabilities
            .iter()
            .chain(deferred_capabilities.iter())
            .cloned()
            .collect();
        for (unique_id, ty) in local_bindings {
            self.env.bind_var(*unique_id, ty.clone());
        }
        for (unique_id, capability) in local_capabilities {
            self.constructor_capabilities
                .insert(*unique_id, capability.clone());
        }

        // Function and trait method bodies are embedded in a top-level typed
        // definition. Check the body now, but defer subtree normalization to the
        // single resolve_typed_node pass in check_program.
        let profile = self.profiler.start();
        let result = if self.body_tail_is_receiverless_trait_call(body) {
            self.check_node_with_expected(body, Some(&function_return_ty))
        } else {
            self.check_node(body)
        };
        for (deferred, checked) in deferred_capabilities.iter_mut().zip(
            self.active_capabilities
                .iter()
                .skip(declaration_capabilities.len()),
        ) {
            deferred.consumed |= checked.consumed;
        }
        let unused_capability = result.as_ref().ok().and_then(|_| {
            self.active_capabilities
                .iter()
                .take(declaration_capabilities.len())
                .find(|capability| !capability.consumed)
                .cloned()
        });
        self.profiler
            .finish(ProfileEvent::CheckBodyIsolated, profile);

        self.env.pop_var_scope();
        self.function_return_ty = saved_function_return_ty;
        self.local_annotation_tyvars = saved_local_annotation_tyvars;
        self.rigid_tyvars = saved_rigid_tyvars;
        self.current_function_symbol = saved_current_function_symbol;
        self.current_impl_struct_target = saved_current_impl_struct_target;
        self.in_extractor_body = saved_in_extractor_body;
        self.closure_depth = saved_closure_depth;
        self.facet_bindings = saved_facet_bindings;
        self.constructor_capabilities = saved_constructor_capabilities;
        self.active_capabilities = saved_active_capabilities;

        if let Some(unused) = unused_capability {
            return Err(TypeError {
                message: format!(
                    "UnusedTraitConstraint: {}: {} is never consumed",
                    unused.subject_name,
                    Self::surface_name(&unused.trait_id)
                ),
                span: unused.span,
                hint: Some(
                    "Remove this bound, or use it in a trait call or generic proof forwarding expression."
                        .into(),
                ),
            });
        }
        result
    }

    fn body_tail_is_receiverless_trait_call(&self, body: &Resolved) -> bool {
        match body {
            Resolved::Block(_, statements) => statements
                .last()
                .is_some_and(|tail| self.body_tail_is_receiverless_trait_call(tail)),
            Resolved::Grouped(_, inner) => self.body_tail_is_receiverless_trait_call(inner),
            Resolved::App(_, function, _) => self
                .trait_method_ref(function)
                .and_then(|(_, trait_name, method_name)| {
                    self.traits
                        .get(&trait_name)
                        .and_then(|trait_info| trait_info.methods.get(&method_name))
                })
                .is_some_and(|method| {
                    !method
                        .value_parameters
                        .iter()
                        .any(|param| Self::ast_ty_mentions_self(&param.ty))
                }),
            _ => false,
        }
    }

    fn ast_ty_mentions_self(ty: &AstTy) -> bool {
        match ty {
            AstTy::Named(_, name) | AstTy::ImplTrait(_, name) => name == "Self",
            AstTy::Generic(_, name, args) => {
                name == "Self" || args.iter().any(Self::ast_ty_mentions_self)
            }
            AstTy::Tuple(_, items) => items.iter().any(Self::ast_ty_mentions_self),
            AstTy::Func(_, params, ret) => {
                params.iter().any(Self::ast_ty_mentions_self) || Self::ast_ty_mentions_self(ret)
            }
        }
    }

    pub(super) fn check_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        return_type_arguments: &[ResolvedReturnTypeArgument],
        params: &[ResolvedValueParameter],
        ret_ty: &Option<AstTy>,
        where_clause: Option<&ResolvedWhereClause>,
        body: &Resolved,
        attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
        let mut typed_params = Vec::new();
        let mut local_bindings = Vec::new();
        let mut local_capabilities = Vec::new();
        let mut tyvars = HashMap::new();
        let typed_return_type_arguments = return_type_arguments
            .iter()
            .map(|argument| {
                Ok(TypedReturnTypeArgument {
                    ordinal: argument.ordinal,
                    ty: self.resolve_def_signature_ast_ty_in_context(
                        id,
                        &argument.ty,
                        TypeSyntaxContext::General,
                        &mut tyvars,
                    )?,
                    span: argument.span.clone(),
                })
            })
            .collect::<Result<Vec<_>, TypeError>>()?;

        for param in params {
            let param_ty = self.resolve_def_signature_ast_ty_in_context(
                id,
                &param.ty,
                TypeSyntaxContext::General,
                &mut tyvars,
            )?;
            if self.ty_contains_process_init(&param_ty) {
                return Err(TypeError {
                    message: "StandbyInit<T> is only allowed as Standby @init return type".into(),
                    span: param.id.span.clone(),
                    hint: None,
                });
            }
            if !self.allow_error_function_params
                && !Self::allows_std_error_function_param_exception(id)
                && Self::ty_exposes_error_value(&param_ty)
            {
                return Err(
                    self.error_function_param_not_allowed_error(Self::ast_ty_span(&param.ty))
                );
            }
            if self.ty_contains_facet(&param_ty) {
                return Err(TypeError {
                    message:
                        "Facet is compile-time only in Stage1 and cannot appear in function parameter types"
                            .into(),
                    span: param.id.span.clone(),
                    hint: None,
                });
            }
            local_bindings.push((param.id.unique_id, param_ty.clone()));
            if let Some(capability) = self.constructor_trait_key_for_ast_ty(&param.ty) {
                local_capabilities.push((param.id.unique_id, capability));
            }
            typed_params.push(TypedValueParameter {
                id: param.id.clone(),
                mode: param.mode,
                ty: param_ty.clone(),
                span: param.span.clone(),
            });
        }

        let mut expected_ret = match ret_ty {
            Some(ty) => self.resolve_def_signature_ast_ty_in_context(
                id,
                ty,
                TypeSyntaxContext::FunctionReturn,
                &mut tyvars,
            )?,
            None => Ty::Unit,
        };
        self.apply_resolved_where_trait_bounds(where_clause, &tyvars, None)?;
        let declaration_capabilities = self.resolved_capability_uses(where_clause, &tyvars)?;
        if self.ty_contains_facet(&expected_ret) {
            return Err(TypeError {
                message:
                    "Facet is compile-time only in Stage1 and cannot appear in function return types"
                        .into(),
                span: span.clone(),
                hint: None,
            });
        }

        let current_symbol = id.qualified_name.clone().unwrap_or_else(|| id.name.clone());
        if self.process_handler_return_exposes_context_pid(&current_symbol, &expected_ret) {
            return Err(TypeError {
                message: "handler dependency cannot be returned from process handlers".into(),
                span: span.clone(),
                hint: Some("Keep ctx.<slot> usage inside the process handler body.".into()),
            });
        }
        let is_entrypoint = self
            .runtime_policy
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
                        .runtime_policy
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

        let impl_target = Self::split_impl_method_id(id).and_then(|(impl_target, _method)| {
            self.env
                .lookup_type_def(&impl_target)
                .is_some_and(|def| def.kind == crate::env::TypeKind::Struct)
                .then_some(Self::surface_name(&impl_target).to_string())
        });
        let typed_body = self.check_body_in_isolated_scope(
            &local_bindings,
            &local_capabilities,
            &declaration_capabilities,
            &mut [],
            tyvars.clone(),
            Self::signature_tyvar_ids(&tyvars),
            expected_ret.clone(),
            current_symbol,
            impl_target,
            false,
            body,
        )?;

        if let Some(return_ast) = ret_ty.as_ref() {
            if let Some(concrete) = self.resolve_contextual_return_body(
                return_ast,
                &expected_ret,
                &typed_body,
                &Self::signature_tyvar_ids(&tyvars),
            )? {
                expected_ret = concrete;
            }
        }

        let actual_ret = self.resolve_ty(&typed_body.ty);
        if let Some(err) = self.bare_return_typevar_result_mismatch(
            &expected_ret,
            &actual_ret,
            &self.return_mismatch_span(&typed_body),
        ) {
            return Err(err);
        }
        let rigid_tyvars = Self::signature_tyvar_ids(&tyvars);
        let return_constructor_coercion = ret_ty
            .as_ref()
            .and_then(|ast_ty| self.constructor_trait_key_for_ast_ty(ast_ty))
            .is_some_and(|trait_key| {
                self.constructor_annotation_compatible(
                    &trait_key,
                    &expected_ret,
                    &typed_body.ty,
                    None,
                )
            });
        if !self.types_compatible_with_rigid(&expected_ret, &typed_body.ty, &rigid_tyvars)
            && !return_constructor_coercion
        {
            if let Some(err) = self.facet_replace_result_context_error(
                &typed_body,
                &expected_ret,
                &self.return_mismatch_span(&typed_body),
            ) {
                return Err(err);
            }
            if let Some(err) = self.plain_value_result_context_error(
                &expected_ret,
                &typed_body.ty,
                &self.return_mismatch_span(&typed_body),
            ) {
                return Err(err);
            }
            let hint = if matches!(actual_ret, Ty::Unit) {
                self.describe_unit_return_hint(&typed_body)
            } else {
                None
            };
            return Err(TypeError {
                message: if ret_ty.is_some() {
                    format!(
                        "expected {}, got {}",
                        self.ty_name(&expected_ret),
                        self.ty_name(&actual_ret)
                    )
                } else {
                    format!(
                        "def {} without an explicit return type must return Unit, got {}",
                        id.name,
                        self.ty_name(&actual_ret)
                    )
                },
                span: self.return_mismatch_span(&typed_body),
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
        let checked_params = typed_params
            .iter()
            .map(|param| param.ty.clone())
            .collect::<Vec<_>>();
        let mut checked_type_params = Vec::new();
        for argument in &typed_return_type_arguments {
            Self::collect_ty_vars(&argument.ty, &mut checked_type_params);
        }
        for param in &checked_params {
            Self::collect_ty_vars(param, &mut checked_type_params);
        }
        Self::collect_ty_vars(&expected_ret, &mut checked_type_params);
        self.env.bind_var(
            id.unique_id,
            Ty::UserFunc {
                fun_idx,
                type_params: checked_type_params,
                params: checked_params,
                ret: Box::new(expected_ret.clone()),
            },
        );
        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Def(
                fun_idx,
                id.clone(),
                typed_return_type_arguments,
                typed_params,
                expected_ret,
                where_clause.map(TypedWhereClause::from),
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
        if self.ty_contains_facet(&param_ty) {
            return Err(TypeError {
                message:
                    "Facet is compile-time only in Stage1 and cannot appear in extractor parameter types"
                        .into(),
                span: param.id.span.clone(),
                hint: None,
            });
        }
        let local_bindings = vec![(param.id.unique_id, param_ty.clone())];
        let typed_param = TypedValueParameter {
            id: param.id.clone(),
            mode: spire::ast::ValueParameterMode::PositionalOrNamed,
            ty: param_ty,
            span: param.id.span.clone(),
        };

        let expected_ret = self.resolve_signature_ast_ty_in_context(
            ret_ty,
            TypeSyntaxContext::ExtractorReturn,
            &mut tyvars,
        )?;
        if self.ty_contains_facet(&expected_ret) {
            return Err(TypeError {
                message:
                    "Facet is compile-time only in Stage1 and cannot appear in extractor return types"
                        .into(),
                span: span.clone(),
                hint: None,
            });
        }
        self.require_extractor_option_payload_ty(
            &expected_ret,
            &param.id.span,
            &format!("Extractor {}", id.name),
        )?;

        let current_symbol = id.qualified_name.clone().unwrap_or_else(|| id.name.clone());
        let impl_target = Self::split_impl_method_id(id).and_then(|(impl_target, _method)| {
            self.env
                .lookup_type_def(&impl_target)
                .is_some_and(|def| def.kind == crate::env::TypeKind::Struct)
                .then_some(Self::surface_name(&impl_target).to_string())
        });
        let typed_body = self.check_body_in_isolated_scope(
            &local_bindings,
            &[],
            &[],
            &mut [],
            tyvars.clone(),
            Self::signature_tyvar_ids(&tyvars),
            expected_ret.clone(),
            current_symbol,
            impl_target,
            true,
            body,
        )?;

        let rigid_tyvars = Self::signature_tyvar_ids(&tyvars);
        if !self.types_compatible_with_rigid(&expected_ret, &typed_body.ty, &rigid_tyvars) {
            let actual_ret = self.resolve_ty(&typed_body.ty);
            let hint = if matches!(actual_ret, Ty::Unit) {
                self.describe_unit_return_hint(&typed_body)
            } else {
                None
            };
            return Err(TypeError {
                message: format!(
                    "expected {}, got {}",
                    self.ty_name(&expected_ret),
                    self.ty_name(&actual_ret)
                ),
                span: self.return_mismatch_span(&typed_body),
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
                TypedValueParameter {
                    id: typed_param.id,
                    mode: typed_param.mode,
                    ty: self.resolve_ty(&typed_param.ty),
                    span: typed_param.span,
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
        where_clause: Option<&ResolvedWhereClause>,
        resolved_methods: &[ResolvedTraitImplMethod],
    ) -> Result<Vec<TypedNode>, TypeError> {
        let (_, target_ty, _, _) = self.resolve_trait_impl_head_tys(trait_args, target_ast_ty)?;
        let target_name = self
            .trait_target_name(&target_ty)
            .ok_or_else(|| TypeError {
                message:
                    "trait impl target must be a concrete named type, tuple type, or function type"
                        .into(),
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
        let impl_info = self
            .trait_impl_for_head(trait_id, trait_args, target_ast_ty)
            .ok_or_else(|| TypeError {
                message: format!("Unknown trait impl {} for {}", trait_id.name, target_name),
                span: span.clone(),
                hint: None,
            })?;
        let mut impl_tyvars = impl_info
            .type_param_vars_by_name
            .iter()
            .map(|(name, var)| (name.clone(), Ty::Var(*var)))
            .collect::<HashMap<_, _>>();
        impl_tyvars.insert("Self".into(), target_ty.clone());
        let mut block_capabilities = self.resolved_capability_uses(where_clause, &impl_tyvars)?;
        let mut typed_nodes = vec![TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::TraitImplDef(
                self.trait_instance_key(trait_id, trait_args),
                target_name.clone(),
                where_clause.map(TypedWhereClause::from),
            ),
        }];

        let mut methods = impl_info.methods.into_values().collect::<Vec<_>>();
        methods.sort_by(|left, right| left.method_name.cmp(&right.method_name));

        for method in methods {
            let resolved_method = resolved_methods
                .iter()
                .find(|candidate| candidate.function_id.unique_id == method.function_id.unique_id);
            if resolved_method.is_none() && !method.function_id.compiler_generated {
                return Err(TypeError {
                    message: format!("Unknown resolved impl method {}", method.method_name),
                    span: method.span.clone(),
                    hint: None,
                });
            }
            let trait_method =
                trait_info
                    .methods
                    .get(&method.method_name)
                    .ok_or_else(|| TypeError {
                        message: format!(
                            "Trait impl {} for {} defines unknown method `{}`",
                            trait_id.name, target_name, method.method_name
                        ),
                        span: method.span.clone(),
                        hint: None,
                    })?;
            let (param_tys, mut expected_ret, type_params, return_type_argument_tys) = self
                .resolve_trait_impl_method_signature(
                    &trait_info,
                    trait_args,
                    &method,
                    target_ast_ty,
                    &trait_method.ret_ty,
                    impl_info.where_clause.as_ref(),
                )?;

            let mut typed_params = Vec::new();
            let mut local_bindings = Vec::new();
            let mut method_tyvars = impl_tyvars.clone();
            for (param, param_ty) in method.value_parameters.iter().zip(param_tys.iter()) {
                local_bindings.push((param.id.unique_id, param_ty.clone()));
                let binding_source = match &param.ty {
                    AstTy::Named(_, name) if name == "Self" => target_ast_ty,
                    _ => &param.ty,
                };
                self.collect_signature_ty_bindings(binding_source, param_ty, &mut method_tyvars);
                typed_params.push(TypedValueParameter {
                    id: param.id.clone(),
                    mode: param.mode,
                    ty: param_ty.clone(),
                    span: param.span.clone(),
                });
            }

            if method.is_builtin {
                continue;
            }
            self.collect_signature_ty_bindings(
                method.ret_ty.as_ref().unwrap_or(&trait_method.ret_ty),
                &expected_ret,
                &mut method_tyvars,
            );
            self.seed_missing_method_type_params(&method.type_params, &mut method_tyvars);
            let method_capabilities = match resolved_method {
                Some(resolved_method) => self.resolved_capability_uses(
                    resolved_method.where_clause.as_ref(),
                    &method_tyvars,
                )?,
                None => self.typed_capability_uses(method.where_clause.as_ref(), &method_tyvars),
            };
            let mut method_block_capabilities =
                self.resolved_capability_uses(where_clause, &method_tyvars)?;
            for capability in &mut method_block_capabilities {
                capability.consumed = block_capabilities.iter().any(|existing| {
                    existing.consumed
                        && existing.subject_name == capability.subject_name
                        && Self::base_trait_key(&existing.trait_id)
                            == Self::base_trait_key(&capability.trait_id)
                });
            }

            let impl_target = self
                .env
                .lookup_type_def(&target_name)
                .is_some_and(|def| def.kind == crate::env::TypeKind::Struct)
                .then_some(Self::surface_name(&target_name).to_string());
            let typed_body = self.check_body_in_isolated_scope(
                &local_bindings,
                &[],
                &method_capabilities,
                &mut method_block_capabilities,
                method_tyvars.clone(),
                type_params.iter().copied().collect(),
                expected_ret.clone(),
                method.function_id.name.clone(),
                impl_target,
                false,
                &method.body,
            )?;
            let body_obligations = Self::full_trait_obligations(&typed_body);
            for registered_impl in self.trait_impls.values_mut() {
                if let Some(registered_method) =
                    registered_impl.methods.get_mut(&method.method_name)
                {
                    if registered_method.function_id.unique_id == method.function_id.unique_id {
                        registered_method.body_obligations = body_obligations.clone();
                        break;
                    }
                }
            }
            for checked in method_block_capabilities {
                if checked.consumed {
                    if let Some(existing) = block_capabilities.iter_mut().find(|existing| {
                        existing.subject_name == checked.subject_name
                            && Self::base_trait_key(&existing.trait_id)
                                == Self::base_trait_key(&checked.trait_id)
                    }) {
                        existing.consumed = true;
                    }
                }
            }

            let rigid_tyvars = type_params.iter().copied().collect::<HashSet<_>>();
            let return_ast = method.ret_ty.as_ref().unwrap_or(&trait_method.ret_ty);
            if let Some(concrete) = self.resolve_contextual_return_body(
                return_ast,
                &expected_ret,
                &typed_body,
                &rigid_tyvars,
            )? {
                expected_ret = concrete;
            }

            let actual_ret = self.resolve_ty(&typed_body.ty);
            if let Some(err) = self.bare_return_typevar_result_mismatch(
                &expected_ret,
                &actual_ret,
                &self.return_mismatch_span(&typed_body),
            ) {
                return Err(err);
            }
            if !self.types_compatible_with_rigid(&expected_ret, &typed_body.ty, &rigid_tyvars) {
                if let Some(err) = self.facet_replace_result_context_error(
                    &typed_body,
                    &expected_ret,
                    &self.return_mismatch_span(&typed_body),
                ) {
                    return Err(err);
                }
                if let Some(err) = self.plain_value_result_context_error(
                    &expected_ret,
                    &typed_body.ty,
                    &self.return_mismatch_span(&typed_body),
                ) {
                    return Err(err);
                }
                let hint = if matches!(actual_ret, Ty::Unit) {
                    self.describe_unit_return_hint(&typed_body)
                } else {
                    None
                };
                return Err(TypeError {
                    message: format!(
                        "expected {}, got {}",
                        self.ty_name(&expected_ret),
                        self.ty_name(&actual_ret)
                    ),
                    span: self.return_mismatch_span(&typed_body),
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
            let typed_return_type_arguments = method
                .return_type_arguments
                .iter()
                .zip(return_type_argument_tys.iter())
                .map(|(argument, ty)| TypedReturnTypeArgument {
                    ordinal: argument.ordinal,
                    ty: ty.clone(),
                    span: argument.span.clone(),
                })
                .collect::<Vec<_>>();
            typed_nodes.push(TypedNode {
                ty: Ty::Unit,
                span: method.span.clone(),
                node: TypedInner::Def(
                    fun_idx,
                    method.function_id.clone(),
                    typed_return_type_arguments,
                    typed_params,
                    expected_ret,
                    method.where_clause.clone(),
                    Box::new(typed_body),
                    method.attrs.visibility,
                ),
            });
        }

        if let Some(unused) = block_capabilities
            .into_iter()
            .find(|capability| !capability.consumed)
        {
            return Err(TypeError {
                message: format!(
                    "UnusedTraitConstraint: {}: {} is never consumed by this impl block",
                    unused.subject_name,
                    Self::surface_name(&unused.trait_id)
                ),
                span: unused.span,
                hint: Some(
                    "Remove this impl bound, or consume it from at least one implementation method."
                        .into(),
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
        where_clause: Option<&ResolvedWhereClause>,
        _methods: &[ResolvedTraitImplMethod],
    ) -> Result<TypedNode, TypeError> {
        let (_, target_ty, _, _) = self.resolve_trait_impl_head_tys(trait_args, target_ast_ty)?;
        let target_name = self
            .trait_target_name(&target_ty)
            .ok_or_else(|| TypeError {
                message:
                    "trait impl target must be a concrete named type, tuple type, or function type"
                        .into(),
                span: Self::ast_ty_span(target_ast_ty).clone(),
                hint: None,
            })?;
        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::TraitImplDef(
                self.trait_instance_key(trait_id, trait_args),
                target_name,
                where_clause.map(TypedWhereClause::from),
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
        type_params: &[ResolvedTypeParam],
        fields: &[ResolvedField],
    ) -> Result<TypedNode, TypeError> {
        let mut tyvars = HashMap::new();
        self.seed_signature_type_params(type_params, &mut tyvars);
        let ty_fields: Vec<(String, Ty)> = fields
            .iter()
            .map(|f| {
                Ok((
                    f.name.clone(),
                    self.resolve_signature_ast_ty_in_context(
                        &f.ty,
                        TypeSyntaxContext::General,
                        &mut tyvars,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, TypeError>>()?;
        let private_fields = fields
            .iter()
            .filter(|field| field.visibility == spire::ast::Visibility::Private)
            .map(|field| field.name.clone())
            .collect::<HashSet<_>>();

        let readonly_fields = fields
            .iter()
            .filter(|field| field.readonly)
            .map(|field| field.name.clone())
            .collect::<HashSet<_>>();
        let readonly_root = self
            .env
            .lookup_type_def(&id.name)
            .is_some_and(|def| def.readonly_root);
        let type_param_vars = type_params
            .iter()
            .filter_map(|param| match tyvars.get(&param.name) {
                Some(Ty::Var(var)) => Some(*var),
                _ => None,
            })
            .collect::<Vec<_>>();

        let tag = self
            .env
            .resolve_type_def_signature(
                &id.name,
                ty_fields.clone(),
                type_param_vars,
                private_fields,
                readonly_fields,
                readonly_root,
            )
            .ok_or_else(|| TypeError {
                message: format!("Unknown struct type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

        self.env
            .bind_var(id.unique_id, Ty::Struct(id.name.clone(), ty_fields.clone()));

        let field_names: Vec<String> = ty_fields.iter().map(|(n, _)| n.clone()).collect();
        let field_policies = fields
            .iter()
            .map(|field| crate::typed::TypedFieldPolicy {
                private: field.visibility == spire::ast::Visibility::Private,
                readonly: field.readonly,
            })
            .collect();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::StructDef(
                tag,
                id.name.clone(),
                field_names,
                field_policies,
                readonly_root,
            ),
        })
    }

    pub(super) fn check_enum_def(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        type_params: &[ResolvedTypeParam],
        variants: &[ResolvedEnumVariant],
        attrs: &ResolvedDeclAttrs,
    ) -> Result<TypedNode, TypeError> {
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
                            message: "Builtin Result enum must match `defenum Result<$T> { Ok($T), Err(Error) }`.".into(),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                    self.seen_builtin_type_decls
                        .insert("Result".into(), (vec!["$T".into()], span.clone()));
                }
                "Boolean" => {
                    if !type_params.is_empty()
                        || variants.len() != 2
                        || variants[0].id.name.rsplit("::").next() != Some("True")
                        || variants[1].id.name.rsplit("::").next() != Some("False")
                        || !variants.iter().all(|variant| variant.payload.is_empty())
                    {
                        return Err(TypeError {
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

        let enum_variants = self
            .lookup_enum_variants_of(&id.name)
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
            .iter()
            .map(|variant| TypedEnumVariantDef {
                tag: variant.tag,
                constructor_name: variant.constructor_name.clone(),
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

        let readonly_fields = fields
            .iter()
            .filter(|field| field.readonly)
            .map(|field| field.name.clone())
            .collect::<HashSet<_>>();
        let readonly_root = false;

        let tag = self
            .env
            .resolve_type_def_signature(
                &id.name,
                ty_fields.clone(),
                Vec::new(),
                private_fields,
                readonly_fields,
                readonly_root,
            )
            .ok_or_else(|| TypeError {
                message: format!("Unknown record type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

        self.env
            .bind_var(id.unique_id, Ty::Record(id.name.clone(), ty_fields.clone()));

        let field_names: Vec<String> = ty_fields.iter().map(|(n, _)| n.clone()).collect();
        let field_policies = fields
            .iter()
            .map(|field| crate::typed::TypedFieldPolicy {
                private: field.visibility == spire::ast::Visibility::Private,
                readonly: field.readonly,
            })
            .collect();

        Ok(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::RecordDef(
                tag,
                id.name.clone(),
                field_names,
                field_policies,
                readonly_root,
            ),
        })
    }

    pub(super) fn check_struct_lit(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        field_vals: &[ResolvedStructLitField],
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

        if !id.compiler_generated {
            let owner_name = Self::surface_name(&def.name);
            if self.current_impl_struct_target.as_deref() != Some(owner_name) {
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
        }

        let tag = def.tag;

        let mut seen = HashSet::new();
        for field in field_vals {
            let name = match field {
                ResolvedStructLitField::Explicit(name, _)
                | ResolvedStructLitField::Shorthand(name, _) => name,
            };
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
            let resolved_val = field_vals
                .iter()
                .find_map(|field| match field {
                    ResolvedStructLitField::Explicit(name, resolved_val)
                    | ResolvedStructLitField::Shorthand(name, resolved_val)
                        if name == def_name =>
                    {
                        Some(resolved_val)
                    }
                    _ => None,
                })
                .ok_or_else(|| TypeError {
                    message: format!("Missing field '{}' in {}", def_name, id.name),
                    span: span.clone(),
                    hint: None,
                })?;
            let typed_val = self.check_node(resolved_val)?;
            if self.ty_contains_facet(&typed_val.ty) {
                return Err(TypeError {
                    message:
                        "Struct literal fields cannot contain Facet values in Stage1 (Facet is compile-time only)"
                            .into(),
                    span: typed_val.span.clone(),
                    hint: Some("Apply Facet::view/set/over before constructing runtime values.".into()),
                });
            }
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
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        // Closure bodies are checked through `check_node`, but an expected
        // closure return type is kept separately while that body is checked.
        // Use it for Result constructors so `Err(error)` can inhabit a nested
        // Result without constructing `Err(Err(error))`.
        // An absent call-site expectation means the constructor is
        // polymorphic, not that it should inherit the enclosing function's
        // return type.  In particular, `Err(...)` inside a list/tuple or a
        // nested call must not become `Result<Unit>` merely because the
        // surrounding function returns `Result<Unit>`.  The fresh Result
        // success slot below is unified later by the surrounding expression
        // or by the function body's final return check.
        let expected = expected.cloned();
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
                    let inner_expected =
                        expected
                            .as_ref()
                            .and_then(|expected| match self.resolve_ty(expected) {
                                Ty::Result(ok, _) => Some(ok.as_ref().clone()),
                                _ => None,
                            });
                    let typed = self.check_node_with_expected(expr, inner_expected.as_ref())?;
                    if self.ty_contains_facet(&typed.ty) {
                        return Err(TypeError {
                            message:
                                "Result constructors cannot contain Facet values in Stage1 (Facet is compile-time only)"
                                    .into(),
                            span: typed.span.clone(),
                            hint: Some(
                                "Apply Facet::view/set/over before wrapping with Ok(...) or Err(...)."
                                    .into(),
                            ),
                        });
                    }
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
                if matches!(self.resolve_ty(&inner.ty), Ty::Result(_, _)) {
                    return Err(TypeError {
                        message: "Nested Result errors are not allowed: use Err(ConcreteError) for the outer failure, or Ok(Err(ConcreteError)) for an inner failure.".into(),
                        span: inner.span.clone(),
                        hint: Some(
                            "Err(...) is lifted to the expected Result nesting; do not write Err(Err(...)).".into(),
                        ),
                    });
                }
                if !matches!(inner.ty, Ty::Error) {
                    return Err(TypeError {
                        message: "Err(...) requires a concrete deferror value.".into(),
                        span: inner.span.clone(),
                        hint: Some(
                            "Use a deferror-defined value in Err(...), not a plain value.".into(),
                        ),
                    });
                }
                if self.is_abstract_error_marker_value(&inner) {
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
                let result_ty = expected
                    .as_ref()
                    .filter(|ty| matches!(self.resolve_ty(ty), Ty::Result(_, _)))
                    .map(|ty| self.resolve_ty(ty))
                    .unwrap_or_else(|| {
                        let ok_var = self.env.fresh_tyvar();
                        Ty::Result(Box::new(ok_var), Box::new(Ty::Error))
                    });
                (1u32, result_ty)
            };
            return Ok(TypedNode {
                ty: result_ty,
                span: span.clone(),
                node: TypedInner::ConstructorCall(tag, vec![inner]),
            });
        }

        if let Some(variant) = self.lookup_enum_variant_by_constructor_id(id.unique_id) {
            let variant = self.instantiate_enum_variant(&variant);
            let enum_surface_name = Self::surface_name(&variant.enum_name);
            if enum_surface_name == "Boolean" {
                if !args.is_empty() {
                    return Err(TypeError {
                        message: format!("{} expects 0 argument(s), got {}", id.name, args.len()),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let value = match variant.short_name.as_str() {
                    "True" => true,
                    "False" => false,
                    _ => {
                        return Err(TypeError {
                            message: format!(
                                "Unknown builtin Boolean variant: {}",
                                variant.short_name
                            ),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                };
                return Ok(TypedNode {
                    ty: Ty::Bool,
                    span: span.clone(),
                    node: TypedInner::Lit(Lit::Bool(value)),
                });
            }
            if enum_surface_name == "Result" {
                if args.len() != 1 {
                    return Err(TypeError {
                        message: format!("{} expects 1 argument(s), got {}", id.name, args.len()),
                        span: span.clone(),
                        hint: None,
                    });
                }
                let inner = match &args[0] {
                    ResolvedRecordLitArg::Positional(expr) => {
                        // Preserve the expected Result payload when this
                        // constructor was resolved through enum metadata
                        // (the qualified `Result::Ok` path).  Applicative
                        // chains rely on this context to infer nested
                        // closures left-to-right: the first `|*|` fixes the
                        // mapper input, which then constrains the next one.
                        let inner_expected = expected.as_ref().and_then(|expected| {
                            match self.resolve_ty(expected) {
                                Ty::Result(ok, _) => Some(ok.as_ref().clone()),
                                _ => None,
                            }
                        });
                        let typed = self.check_node_with_expected(expr, inner_expected.as_ref())?;
                        if self.ty_contains_facet(&typed.ty) {
                            return Err(TypeError {
                                message:
                                    "Result constructors cannot contain Facet values in Stage1 (Facet is compile-time only)"
                                        .into(),
                                span: typed.span.clone(),
                                hint: Some(
                                    "Apply Facet::view/set/over before wrapping with Ok(...) or Err(...)."
                                        .into(),
                                ),
                            });
                        }
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
                if variant.short_name == "Err" {
                    if matches!(self.resolve_ty(&inner.ty), Ty::Result(_, _)) {
                        return Err(TypeError {
                            message: "Nested Result errors are not allowed: use Err(ConcreteError) for the outer failure, or Ok(Err(ConcreteError)) for an inner failure.".into(),
                            span: inner.span.clone(),
                            hint: Some(
                                "Err(...) is lifted to the expected Result nesting; do not write Err(Err(...)).".into(),
                            ),
                        });
                    }
                    if !matches!(inner.ty, Ty::Error) {
                        return Err(TypeError {
                            message: "Err(...) requires a concrete deferror value.".into(),
                            span: inner.span.clone(),
                            hint: Some(
                                "Use a deferror-defined value in Err(...), not a plain value."
                                    .into(),
                            ),
                        });
                    }
                    if self.is_abstract_error_marker_value(&inner) {
                        return Err(TypeError {
                            message: "Error is abstract and cannot be constructed directly.".into(),
                            span: inner.span.clone(),
                            hint: Some("Use a concrete deferror value in Err(...).".into()),
                        });
                    }
                }
                let result_ty = if variant.short_name == "Ok" {
                    Ty::Result(Box::new(inner.ty.clone()), Box::new(Ty::Error))
                } else {
                    expected
                        .as_ref()
                        .filter(|ty| matches!(self.resolve_ty(ty), Ty::Result(_, _)))
                        .map(|ty| self.resolve_ty(ty))
                        .unwrap_or_else(|| {
                            let ok_var = self.env.fresh_tyvar();
                            Ty::Result(Box::new(ok_var), Box::new(Ty::Error))
                        })
                };
                return Ok(TypedNode {
                    ty: result_ty,
                    span: span.clone(),
                    node: TypedInner::ConstructorCall(variant.tag, vec![inner]),
                });
            }
            if matches!(
                Self::surface_name(&variant.enum_name),
                "StopReply" | "StopReason"
            ) && !self.stop_constructor_allowed()
            {
                return Err(self.stop_constructor_error(span, &variant.enum_name));
            }
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
                if self.ty_contains_facet(&typed.ty) {
                    return Err(TypeError {
                        message:
                            "Enum constructors cannot contain Facet values in Stage1 (Facet is compile-time only)"
                                .into(),
                        span: typed.span.clone(),
                        hint: Some("Apply Facet::view/set/over before constructing runtime values.".into()),
                    });
                }
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
                    let callable_hint =
                        Some(self.call_target_signature_hint_for_id(id, params, ret.as_ref()));
                    if args.len() != params.len() {
                        return Err(TypeError {
                            message: format!(
                                "function expects {} argument(s), got {}",
                                params.len(),
                                args.len()
                            ),
                            span: span.clone(),
                            hint: callable_hint.clone(),
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
                        if self.ty_contains_facet(&typed_val.ty) {
                            return Err(TypeError {
                                message:
                                    "Constructor arguments cannot contain Facet values in Stage1 (Facet is compile-time only)"
                                        .into(),
                                span: typed_val.span.clone(),
                                hint: Some(
                                    "Apply Facet::view/set/over before passing constructor arguments."
                                        .into(),
                                ),
                            });
                        }
                        if !self.types_compatible(param_ty, &typed_val.ty) {
                            return Err(TypeError {
                                message: format!(
                                    "Argument type mismatch: expected {}, got {}",
                                    self.ty_name(param_ty),
                                    self.ty_name(&typed_val.ty)
                                ),
                                span: typed_val.span.clone(),
                                hint: callable_hint.clone(),
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
                    let callable_hint = self.call_target_signature_hint_for_id(id, params, ret);
                    let typed_args = self.typecheck_user_function_args(
                        span,
                        id.unique_id,
                        params,
                        args,
                        Some(callable_hint.as_str()),
                        false,
                    )?;
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
                    let callable_hint =
                        self.callable_signature_hint(&Ty::Func(params.clone(), ret.clone()));
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
                            hint: callable_hint.clone(),
                        });
                    }

                    let mut typed_args = Vec::with_capacity(params.len());
                    for (expected_ty, arg) in params.iter().zip(args) {
                        let ResolvedRecordLitArg::Positional(expr) = arg else {
                            unreachable!("validated argument form above")
                        };
                        let typed = self.check_node(expr)?;
                        if self.ty_contains_facet(&typed.ty) {
                            return Err(TypeError {
                                message:
                                    "Constructor arguments cannot contain Facet values in Stage1 (Facet is compile-time only)"
                                        .into(),
                                span: typed.span.clone(),
                                hint: Some(
                                    "Apply Facet::view/set/over before passing constructor arguments."
                                        .into(),
                                ),
                            });
                        }
                        if !self.types_compatible(expected_ty, &typed.ty) {
                            return Err(TypeError {
                                message: format!(
                                    "Argument type mismatch: expected {}, got {}",
                                    self.ty_name(expected_ty),
                                    self.ty_name(&typed.ty)
                                ),
                                span: typed.span.clone(),
                                hint: callable_hint.clone(),
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
                        "Define `impl {} {{ def new(...) -> Self {{ ... }} }}` or `impl {} {{ def new(...) -> Result<Self, Error> {{ ... }} }}`.",
                        id.name, id.name
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
            let new_ty = match new_ty {
                Ty::BuiltinFunc { .. } | Ty::UserFunc { .. } => {
                    self.instantiate_callable_ty(&new_ty)
                }
                other => other,
            };
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

            let typed_args =
                self.typecheck_user_function_args(span, new_uid, &params, args, None, false)?;
            let expected_self_ty = Ty::Struct(id.name.clone(), def.fields.clone());
            let returns_self = self.types_compatible(&expected_self_ty, &ret_ty);
            let returns_result_self = match self.resolve_ty(&ret_ty) {
                Ty::Result(ok, _) => self.types_compatible(&expected_self_ty, ok.as_ref()),
                _ => false,
            };
            if !(returns_self || returns_result_self) {
                return Err(TypeError {
                    message: format!(
                        "`{}` must return Self ({}) or Result<Self, E>, got {}",
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
                            symbol_info: None,
                            unique_id: new_uid,
                            compiler_generated: false,
                            span: id.span.clone(),
                        }),
                    }),
                    typed_args,
                ),
            });
        }

        if !matches!(
            def.kind,
            crate::env::TypeKind::Record | crate::env::TypeKind::ConcreteError
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
                    if self.ty_contains_facet(&typed_val.ty) {
                        return Err(TypeError {
                            message:
                                "Record constructors cannot contain Facet values in Stage1 (Facet is compile-time only)"
                                    .into(),
                            span: typed_val.span.clone(),
                            hint: Some("Apply Facet::view/set/over before constructing runtime values.".into()),
                        });
                    }
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
                    if self.ty_contains_facet(&typed_val.ty) {
                        return Err(TypeError {
                            message:
                                "Record constructors cannot contain Facet values in Stage1 (Facet is compile-time only)"
                                    .into(),
                            span: typed_val.span.clone(),
                            hint: Some("Apply Facet::view/set/over before constructing runtime values.".into()),
                        });
                    }
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
            crate::env::TypeKind::ConcreteError => Ty::Error,
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
                Vec::new(),
                fields
                    .iter()
                    .filter(|field| field.visibility == spire::ast::Visibility::Private)
                    .map(|field| field.name.clone())
                    .collect(),
                HashSet::new(),
                false,
            )
            .ok_or_else(|| TypeError {
                message: format!("Unknown error type declaration: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;

        let mut show_env = self.env.clone();
        let typed_params: Vec<TypedValueParameter> = ty_fields
            .iter()
            .map(|(ty, resolved_id)| {
                show_env.bind_var(resolved_id.unique_id, ty.clone());
                TypedValueParameter {
                    id: resolved_id.clone(),
                    mode: spire::ast::ValueParameterMode::PositionalOrNamed,
                    ty: ty.clone(),
                    span: resolved_id.span.clone(),
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
            TypedInner::App(func, args) if args.is_empty() => match &func.node {
                TypedInner::Var(id) => self.env.is_error_constructor(id.unique_id),
                TypedInner::Closure(_, _, body) => self.is_concrete_error_value(body),
                _ => false,
            },
            TypedInner::App(func, _) => {
                matches!(&func.node, TypedInner::Var(id) if self.env.is_error_constructor(id.unique_id))
            }
            _ => false,
        }
    }

    pub(super) fn is_abstract_error_marker_value(&self, node: &TypedNode) -> bool {
        matches!(&node.node, TypedInner::Var(id) if id.name == "Error" && !self.env.is_error_constructor(id.unique_id))
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
        Ok(())
    }

    pub(super) fn ensure_result_error_arg(
        &self,
        node: &TypedNode,
        form_name: &str,
    ) -> Result<(), TypeError> {
        if !matches!(node.ty, Ty::Error) {
            return Err(TypeError {
                message: format!(
                    "{} error argument must evaluate to Error, got {}",
                    form_name,
                    self.ty_name(&node.ty)
                ),
                span: node.span.clone(),
                hint: None,
            });
        }
        Ok(())
    }

    pub(super) fn ensure_recover_kind_marker(&self, node: &TypedNode) -> Result<(), TypeError> {
        if !matches!(node.ty, Ty::Error) {
            return Err(TypeError {
                message: format!(
                    "recover_kind marker must evaluate to Error, got {}",
                    self.ty_name(&node.ty)
                ),
                span: node.span.clone(),
                hint: None,
            });
        }
        if !self.is_concrete_error_value(node) {
            return Err(TypeError {
                message: "recover_kind marker must be a concrete deferror name or constructor"
                    .into(),
                span: node.span.clone(),
                hint: Some("Pass a deferror name like Timeout or a constructor call like Timeout(\"detail\").".into()),
            });
        }
        Ok(())
    }
}
