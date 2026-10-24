use super::*;

impl Checker {
    fn warning_span(span: &Span) -> WarningSpan {
        WarningSpan {
            start: span.start,
            end: span.end,
        }
    }

    fn push_warning(
        &mut self,
        kind: WarningKind,
        phase: WarningPhase,
        span: &Span,
        message: impl Into<String>,
        hint: Option<String>,
    ) {
        self.warnings.push(CompilerWarning::new(
            kind,
            phase,
            Self::warning_span(span),
            message,
            hint,
        ));
    }

    pub(super) fn collect_unused_type_parameter_warnings(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, type_params, fields, _) => {
                    let mut used = HashSet::new();
                    for field in fields {
                        Self::collect_ast_ty_type_params(&field.ty, &mut used);
                    }
                    self.warn_unused_type_params(type_params, &used, &id.name);
                }
                Resolved::EnumDef(_, id, type_params, variants, _) => {
                    let mut used = HashSet::new();
                    for variant in variants {
                        for payload in &variant.payload {
                            Self::collect_ast_ty_type_params(payload, &mut used);
                        }
                    }
                    self.warn_unused_type_params(type_params, &used, &id.name);
                }
                Resolved::Def(_, id, _return_type_arguments, params, ret_ty, _, _, _) => {
                    let used = Self::signature_type_param_uses(
                        params,
                        ret_ty.as_ref().map(|ty| ty.syntax()),
                    );
                    self.warn_unused_type_params(&[], &used, &id.name);
                }
                Resolved::ExtractorDef(_, id, type_params, param, ret_ty, _, _) => {
                    let mut used = HashSet::new();
                    for param in param {
                        if let Some(param_ty) = &param.ty {
                            Self::collect_ast_ty_type_params(param_ty, &mut used);
                        }
                    }
                    Self::collect_ast_ty_type_params(ret_ty, &mut used);
                    self.warn_unused_type_params(type_params, &used, &id.name);
                }
                Resolved::TraitDef(_, id, type_params, _, methods, _) => {
                    let mut trait_used = HashSet::new();
                    for method in methods {
                        Self::validate_trait_method_input_slots(method)?;
                        let mut return_type_argument_slots = HashSet::new();
                        for return_type_argument in &method.return_type_arguments {
                            Self::collect_ast_ty_type_params(
                                &return_type_argument.ty,
                                &mut return_type_argument_slots,
                            );
                        }
                        let mut value_param_slots = HashSet::new();
                        for param in &method.value_parameters {
                            Self::collect_ast_ty_type_params(&param.ty, &mut trait_used);
                            Self::collect_ast_ty_type_params(&param.ty, &mut value_param_slots);
                        }
                        let mut method_input_used = return_type_argument_slots;
                        method_input_used.extend(value_param_slots);
                        let mut method_output_used = HashSet::new();
                        Self::collect_ast_ty_type_params(&method.ret_ty, &mut trait_used);
                        Self::collect_ast_ty_type_params(&method.ret_ty, &mut method_output_used);

                        let mut method_used = method_input_used.clone();
                        method_used.extend(method_output_used.iter().cloned());
                        if method
                            .where_clause
                            .as_ref()
                            .map(|clause| {
                                clause.constraints.iter().any(|constraint| {
                                    let mut vars = HashSet::new();
                                    Self::collect_ast_ty_type_params(
                                        &constraint.subject,
                                        &mut vars,
                                    );
                                    vars.into_iter().any(|var| !method_used.contains(&var))
                                })
                            })
                            .unwrap_or(false)
                        {
                            return Err(TypeError {
                                structured: None,
                                message: format!(
                                    "Trait method {} has a type variable used only by where constraints",
                                    method.id.name
                                ),
                                span: method.span.clone(),
                                hint: Some("Add the type variable to ReturnTypeArguments or a value argument.".into()),
                            });
                        }
                        if id.name == "Default"
                            && method.id.name == "default"
                            && !method.return_type_arguments.iter().any(
                                |param| matches!(&*param.ty, AstTy::Named(_, name) if name == "Self"),
                            )
                            {
                            return Err(TypeError {
                                structured: None,
                                message: "Default::default must declare Self in ReturnTypeArguments".into(),
                                span: method.span.clone(),
                                hint: Some("Use `def default::<Self>() -> Self`.".into()),
                            });
                        }

                        self.warn_unused_type_params(
                            &method.type_params,
                            &method_used,
                            &method.id.name,
                        );
                    }
                    self.warn_unused_type_params(type_params, &trait_used, &id.name);
                }
                Resolved::TraitImplDef(_, _, _, _, _, _, methods) => {
                    for method in methods {
                        let used = Self::signature_type_param_uses(
                            &method.value_parameters,
                            method.ret_ty.as_ref().map(|ty| ty.syntax()),
                        );
                        self.warn_unused_type_params(
                            &method.type_params,
                            &used,
                            &method.function_id.name,
                        );
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn signature_type_param_uses(
        params: &[ResolvedValueParameter],
        ret_ty: Option<&AstTy>,
    ) -> HashSet<String> {
        let mut used = HashSet::new();
        for param in params {
            Self::collect_ast_ty_type_params(&param.ty, &mut used);
        }
        if let Some(ret_ty) = ret_ty {
            Self::collect_ast_ty_type_params(ret_ty, &mut used);
        }
        used
    }

    pub(super) fn collect_ast_ty_type_params(ty: &AstTy, used: &mut HashSet<String>) {
        match ty {
            AstTy::Named(_, name) => {
                if name.starts_with('$') {
                    used.insert(name.clone());
                }
            }
            AstTy::ImplTrait(_, _) => {}
            AstTy::Generic(_, name, args) => {
                if name.starts_with('$') {
                    used.insert(name.clone());
                }
                for arg in args {
                    Self::collect_ast_ty_type_params(arg, used);
                }
            }
            AstTy::Tuple(_, items) => {
                for item in items {
                    Self::collect_ast_ty_type_params(item, used);
                }
            }
            AstTy::Func(_, params, ret) => {
                for param in params {
                    Self::collect_ast_ty_type_params(param, used);
                }
                Self::collect_ast_ty_type_params(ret, used);
            }
        }
    }

    fn warn_unused_type_params(
        &mut self,
        type_params: &[ResolvedTypeParam],
        used: &HashSet<String>,
        owner_name: &str,
    ) {
        for param in type_params {
            if used.contains(&param.name) {
                continue;
            }
            self.push_warning(
                WarningKind::UnusedTypeParameter,
                WarningPhase::Typecheck,
                &param.span,
                format!("unused type parameter `{}` in `{}`", param.name, owner_name),
                Some("Remove the type parameter if it is not part of the signature.".into()),
            );
        }
    }
}
