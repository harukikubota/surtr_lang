use super::*;
use diagnostics::{DiagnosticOrigin, SourceRole, TypeDiagnosticReason};

impl Checker {
    fn derive_dependency_for_subject(
        &mut self,
        target_ty: &Ty,
        subject_span: &Span,
        trait_key: &str,
    ) -> Option<diagnostics::TraitDependencyData> {
        use diagnostics::{TraitDependencyStep, TraitDependencyStepKind};

        let root = self.resolve_ty(target_ty);
        let root_type = self.diagnostic_ty_name(&root);
        let (first_step, mut leaf) = match &root {
            Ty::Struct(name, nominal) | Ty::Record(name, nominal) => self
                .env
                .lookup_type_def(name)?
                .field_type_spans
                .iter()
                .position(|span| span == subject_span)
                .and_then(|index| {
                    self.nominal_fields(name, nominal, subject_span)
                        .ok()?
                        .get(index)
                        .cloned()
                })
                .map(|(name, ty)| {
                    let leaf = self.resolve_ty(&ty);
                    (
                        TraitDependencyStep {
                            kind: TraitDependencyStepKind::Field,
                            name: Some(name.clone()),
                            ordinal: None,
                            ty: self.diagnostic_ty_name(&leaf),
                        },
                        leaf,
                    )
                })?,
            Ty::Enum(name, _) => {
                self.lookup_enum_variants_of(name)?
                    .iter()
                    .find_map(|variant| {
                        variant.payload.iter().enumerate().find_map(|(index, ty)| {
                            (variant.payload_type_spans.get(index) == Some(subject_span)).then(
                                || {
                                    let leaf = self.resolve_ty(ty);
                                    (
                                        TraitDependencyStep {
                                            kind: TraitDependencyStepKind::VariantPayload,
                                            name: Some(variant.short_name.clone()),
                                            ordinal: Some(index as u32),
                                            ty: self.diagnostic_ty_name(&leaf),
                                        },
                                        leaf,
                                    )
                                },
                            )
                        })
                    })?
            }
            _ => return None,
        };
        let mut steps = vec![first_step];
        let mut visited = Vec::new();
        loop {
            if visited.contains(&leaf) {
                break;
            }
            visited.push(leaf.clone());
            let next = match &leaf {
                Ty::List(item)
                    if self.trait_impl_requires_children(trait_key, &leaf, &[*item.clone()]) =>
                {
                    Some((
                        TraitDependencyStepKind::ListElement,
                        None,
                        None,
                        *item.clone(),
                    ))
                }
                Ty::Enum(name, args)
                    if sindr::names::builtin_type_name(sindr::names::surface_path_name(name))
                        == Some(sindr::names::TypeName::HashMap)
                        && args.len() == 1
                        && self.trait_impl_requires_children(
                            trait_key,
                            &leaf,
                            &[args[0].clone()],
                        ) =>
                {
                    Some((
                        TraitDependencyStepKind::HashMapValue,
                        None,
                        None,
                        args[0].clone(),
                    ))
                }
                Ty::Tuple(items) if self.trait_impl_requires_children(trait_key, &leaf, items) => {
                    items
                        .iter()
                        .enumerate()
                        .find(|(_, item)| !self.trait_impl_exists_for_args(trait_key, &[], item))
                        .map(|(index, item)| {
                            (
                                TraitDependencyStepKind::TupleElement,
                                None,
                                Some(index as u32),
                                item.clone(),
                            )
                        })
                }
                Ty::Result(ok, _)
                    if self.trait_impl_requires_children(trait_key, &leaf, &[*ok.clone()]) =>
                {
                    Some((
                        TraitDependencyStepKind::ResultSuccess,
                        None,
                        None,
                        *ok.clone(),
                    ))
                }
                _ => None,
            };
            let Some((kind, name, ordinal, next_ty)) = next else {
                break;
            };
            leaf = self.resolve_ty(&next_ty);
            steps.push(TraitDependencyStep {
                kind,
                name,
                ordinal,
                ty: self.diagnostic_ty_name(&leaf),
            });
        }
        let leaf_policy =
            if self.trait_impl_policy_for_ty(&leaf) == sindr::names::TraitImplPolicy::Open {
                diagnostics::TraitDependencyLeafPolicy::TraitImplementationMissing
            } else {
                diagnostics::TraitDependencyLeafPolicy::TraitImplementationForbidden
            };
        Some(diagnostics::TraitDependencyData {
            context: diagnostics::TraitDependencyContext::Derive,
            root_type,
            steps,
            leaf_type: self.diagnostic_ty_name(&leaf),
            leaf_policy,
        })
    }

    pub(in crate::checker) fn trait_dependency_for_requirement(
        &self,
        trait_key: &str,
        subject: &Ty,
    ) -> Option<diagnostics::TraitDependencyData> {
        use diagnostics::{TraitDependencyStep, TraitDependencyStepKind};

        let root = self.resolve_ty(subject);
        let root_type = self.diagnostic_ty_name(&root);
        let mut probe = self.spawn_child_checker(self.env.clone());
        let mut leaf = root;
        let mut steps = Vec::new();
        let mut visited = Vec::new();
        loop {
            if visited.contains(&leaf) {
                break;
            }
            visited.push(leaf.clone());
            let next = match &leaf {
                Ty::List(item)
                    if self.trait_impl_requires_children(trait_key, &leaf, &[*item.clone()]) =>
                {
                    Some((TraitDependencyStepKind::ListElement, None, *item.clone()))
                }
                Ty::Enum(name, args)
                    if sindr::names::builtin_type_name(sindr::names::surface_path_name(name))
                        == Some(sindr::names::TypeName::HashMap)
                        && args.len() == 1
                        && self.trait_impl_requires_children(
                            trait_key,
                            &leaf,
                            &[args[0].clone()],
                        ) =>
                {
                    Some((TraitDependencyStepKind::HashMapValue, None, args[0].clone()))
                }
                Ty::Tuple(items) if self.trait_impl_requires_children(trait_key, &leaf, items) => {
                    items
                        .iter()
                        .enumerate()
                        .find(|(_, item)| !probe.trait_impl_exists_for_args(trait_key, &[], item))
                        .map(|(index, item)| {
                            (
                                TraitDependencyStepKind::TupleElement,
                                Some(index as u32),
                                item.clone(),
                            )
                        })
                }
                Ty::Result(ok, _)
                    if self.trait_impl_requires_children(trait_key, &leaf, &[*ok.clone()]) =>
                {
                    Some((TraitDependencyStepKind::ResultSuccess, None, *ok.clone()))
                }
                _ => None,
            };
            let Some((kind, ordinal, next_ty)) = next else {
                break;
            };
            leaf = self.resolve_ty(&next_ty);
            steps.push(TraitDependencyStep {
                kind,
                name: None,
                ordinal,
                ty: self.diagnostic_ty_name(&leaf),
            });
        }
        let leaf_policy =
            if self.trait_impl_policy_for_ty(&leaf) == sindr::names::TraitImplPolicy::Open {
                diagnostics::TraitDependencyLeafPolicy::TraitImplementationMissing
            } else {
                diagnostics::TraitDependencyLeafPolicy::TraitImplementationForbidden
            };
        if steps.is_empty()
            && leaf_policy == diagnostics::TraitDependencyLeafPolicy::TraitImplementationMissing
        {
            return None;
        }
        Some(diagnostics::TraitDependencyData {
            context: diagnostics::TraitDependencyContext::Requirement,
            root_type,
            steps,
            leaf_type: self.diagnostic_ty_name(&leaf),
            leaf_policy,
        })
    }

    fn enrich_generated_derive_failure(
        &mut self,
        mut error: TypeError,
        generated: bool,
        target_ty: &Ty,
        trait_key: &str,
        declaration_span: &Span,
    ) -> TypeError {
        if !generated {
            return error;
        }
        let Some(structured) = error.structured.as_mut() else {
            return error;
        };
        let diagnostics::DiagnosticData::TraitDispatch(dispatch) = &mut structured.data else {
            return error;
        };
        if dispatch.trait_name != trait_key {
            return error;
        }
        let Some(dependency) =
            self.derive_dependency_for_subject(target_ty, &error.span, trait_key)
        else {
            return error;
        };
        if dependency.leaf_policy
            == diagnostics::TraitDependencyLeafPolicy::TraitImplementationForbidden
        {
            structured.remediation = Some(diagnostics::Remediation::Help {
                text: if trait_key.ends_with("Show") {
                    "Remove this derive or write Show explicitly, using inspect for the forbidden field when needed."
                        .into()
                } else {
                    "Remove this derive or compare a supported value extracted from the type."
                        .into()
                },
            });
        }
        dispatch.dependency = Some(dependency);
        structured.related.push(self.type_fact(
            SourceRole::Declaration,
            declaration_span,
            target_ty,
        ));
        TypeError::from_structured(structured.clone())
    }

    pub(in crate::checker) fn check_trait_impl_items(
        &mut self,
        span: &Span,
        declaration_id: u32,
        trait_id: &ResolvedId,
        trait_args: &[AstTy],
        target_ast_ty: &AstTy,
        where_clause: Option<&ResolvedWhereClause>,
        resolved_methods: &[ResolvedTraitImplMethod],
    ) -> Result<Vec<TypedNode>, TypeError> {
        let trait_key = self.trait_key(trait_id);
        let trait_info = self
            .traits
            .get(&trait_key)
            .cloned()
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown trait: {}", trait_id.name),
                span: span.clone(),
                hint: None,
            })?;
        let (_, target_ty, _, _) =
            self.resolve_trait_impl_head_tys(&trait_info, trait_args, target_ast_ty)?;
        let target_name = self
            .trait_target_name(&target_ty)
            .ok_or_else(|| TypeError {
                structured: None,
                message:
                    "trait impl target must be a concrete named type, tuple type, or function type"
                        .into(),
                span: Self::ast_ty_span(target_ast_ty).clone(),
                hint: None,
            })?;
        let impl_info = self
            .trait_impl_for_declaration(declaration_id)
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown trait impl {} for {}", trait_id.name, target_name),
                span: span.clone(),
                hint: None,
            })?;
        let target_ty = impl_info.target_ty.clone();
        let mut impl_tyvars = impl_info
            .type_param_vars_by_name
            .iter()
            .map(|(name, var)| (name.clone(), Ty::Var(*var)))
            .collect::<HashMap<_, _>>();
        impl_tyvars.insert("Self".into(), target_ty.clone());
        let mut block_capabilities = if impl_info.generated_derive {
            self.generated_derive_capability_uses(where_clause, &impl_tyvars, &target_ty)?
        } else {
            self.resolved_capability_uses(where_clause, &impl_tyvars)?
        };
        for capability in &mut block_capabilities {
            if self.nominal_type_uses_capability(
                &target_ty,
                &capability.subject_ty,
                &capability.trait_id,
            ) {
                capability.consumed = true;
            }
        }
        let mut typed_nodes = vec![TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::TraitImplDef(
                self.trait_instance_key(trait_id, trait_args),
                target_name.clone(),
                where_clause.map(TypedWhereClause::from),
            ),
        }];

        let mut methods = impl_info.methods.values().collect::<Vec<_>>();
        methods.sort_by_key(|method| method.function_id.unique_id);

        for method in methods {
            let resolved_method = resolved_methods
                .iter()
                .find(|candidate| candidate.function_id.unique_id == method.function_id.unique_id);
            if resolved_method.is_none() && !method.function_id.compiler_generated {
                return Err(TypeError {
                    structured: None,
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
                        structured: None,
                        message: format!(
                            "Trait impl {} for {} defines unknown method `{}`",
                            trait_id.name, target_name, method.method_name
                        ),
                        span: method.span.clone(),
                        hint: None,
                    })?;
            let ResolvedImplMethodSignature {
                params: param_tys,
                result: mut expected_ret,
                type_params,
                return_type_arguments: return_type_argument_tys,
                environment: raw_environment,
            } = method.resolved_signature.clone().ok_or_else(|| {
                TypeError::new(
                    "Validated impl method is missing its declaration signature",
                    method.span.clone(),
                )
            })?;

            let mut typed_params = Vec::new();
            let mut local_bindings = Vec::new();
            let mut method_tyvars = raw_environment.head_bindings.clone();
            method_tyvars.extend(raw_environment.bindings.clone());
            method_tyvars.insert("Self".into(), raw_environment.self_ty.clone());
            for (param, param_ty) in method.value_parameters.iter().zip(param_tys.iter()) {
                local_bindings.push((param.id.unique_id, param_ty.clone()));
                let binding_source = match &*param.ty {
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
            let mut method_capabilities = match resolved_method {
                Some(resolved_method) => self.resolved_capability_uses(
                    resolved_method.where_clause.as_ref(),
                    &method_tyvars,
                )?,
                None => self.typed_capability_uses(method.where_clause.as_ref(), &method_tyvars),
            };
            self.consume_signature_constructor_capabilities(
                &mut method_capabilities,
                &method.return_type_arguments,
                &method.value_parameters,
                Some(method.ret_ty.as_ref().unwrap_or(&trait_method.ret_ty)),
            );
            let mut method_block_capabilities = if impl_info.generated_derive {
                self.generated_derive_capability_uses(where_clause, &method_tyvars, &target_ty)?
            } else {
                self.resolved_capability_uses(where_clause, &method_tyvars)?
            };
            self.consume_signature_constructor_capabilities(
                &mut method_block_capabilities,
                &method.return_type_arguments,
                &method.value_parameters,
                Some(method.ret_ty.as_ref().unwrap_or(&trait_method.ret_ty)),
            );
            let signature_types = return_type_argument_tys
                .iter()
                .chain(param_tys.iter())
                .chain(std::iter::once(&expected_ret))
                .collect::<Vec<_>>();
            for capability in &mut method_block_capabilities {
                if signature_types.iter().any(|ty| {
                    self.nominal_type_uses_capability(
                        ty,
                        &capability.subject_ty,
                        &capability.trait_id,
                    )
                }) {
                    capability.consumed = true;
                }
            }
            for capability in &mut method_block_capabilities {
                capability.consumed = block_capabilities.iter().any(|existing| {
                    existing.consumed
                        && existing.subject_name == capability.subject_name
                        && &existing.trait_id == &capability.trait_id
                });
            }

            let impl_target = self
                .env
                .lookup_type_def(&target_name)
                .is_some_and(|def| def.kind == crate::env::TypeKind::Struct)
                .then_some(Self::surface_name(&target_name).to_string());
            // Derive bodies originate in the owning type declaration. A
            // handwritten trait implementation does not have owner authority.
            let private_field_owner = impl_info
                .generated_derive
                .then(|| impl_target.clone())
                .flatten();
            let typed_body = self
                .check_body_in_isolated_scope(
                    &local_bindings,
                    &[],
                    &method_capabilities,
                    &mut method_block_capabilities,
                    method_tyvars.clone(),
                    type_params.iter().copied().collect(),
                    expected_ret.clone(),
                    method
                        .ret_ty
                        .as_ref()
                        .map(|ty| Self::ast_ty_span(ty.syntax()))
                        .unwrap_or_else(|| Self::ast_ty_span(trait_method.ret_ty.syntax())),
                    method
                        .ret_ty
                        .as_ref()
                        .map(|ty| Self::ast_ty_span(ty.syntax())),
                    method.function_id.name.clone(),
                    impl_target,
                    private_field_owner,
                    false,
                    &method.body,
                )
                .map_err(|error| {
                    self.enrich_generated_derive_failure(
                        error,
                        method.function_id.compiler_generated,
                        &target_ty,
                        &trait_key,
                        span,
                    )
                })?;
            for checked in method_block_capabilities {
                if checked.consumed {
                    if let Some(existing) = block_capabilities.iter_mut().find(|existing| {
                        existing.subject_name == checked.subject_name
                            && &existing.trait_id == &checked.trait_id
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
            let saved_rigid = std::mem::replace(&mut self.rigid_tyvars, rigid_tyvars.clone());
            let relation = self.assert_type_relation(
                &expected_ret,
                &typed_body.ty,
                self.type_fact(
                    SourceRole::Expected,
                    Self::ast_ty_span(return_ast),
                    &expected_ret,
                ),
                self.type_fact(
                    SourceRole::Value,
                    &self.return_mismatch_span(&typed_body),
                    &actual_ret,
                ),
                TypeDiagnosticReason::ReturnTypeMismatch,
                DiagnosticOrigin::Return,
                &method.method_name,
                0,
            );
            self.rigid_tyvars = saved_rigid;
            relation?;

            let typed_body = *self
                .concretize_pending_trait_calls(typed_body)
                .map_err(|error| *error)?;
            let signature_inputs = local_bindings
                .iter()
                .map(|(_, ty)| ty.clone())
                .chain(std::iter::once(expected_ret.clone()))
                .collect::<Vec<_>>();
            self.validate_pending_body_dependencies(&typed_body, &signature_inputs)?;

            let body_obligations = Self::full_trait_obligations(&typed_body)
                .into_iter()
                .map(|obligation| TraitObligation {
                    trait_id: obligation.trait_id,
                    trait_args: obligation
                        .trait_args
                        .iter()
                        .map(|ty| self.resolve_ty(ty))
                        .collect(),
                    receiver: self.resolve_ty(&obligation.receiver),
                })
                .collect::<Vec<_>>();
            let registered_impl_key = self.trait_impls.iter().find_map(|(key, implementation)| {
                implementation
                    .methods
                    .get(&method.method_name)
                    .filter(|registered| {
                        registered.function_id.unique_id == method.function_id.unique_id
                    })
                    .map(|_| key.clone())
            });
            if let Some(key) = registered_impl_key {
                let implementation = Arc::make_mut(
                    self.trait_impls
                        .get_mut(&key)
                        .expect("matched implementation remains registered"),
                );
                implementation
                    .methods
                    .get_mut(&method.method_name)
                    .expect("matched implementation method remains registered")
                    .body_obligations = body_obligations;
            }
            let fun_idx = match self.env.lookup_var(method.function_id.unique_id) {
                Some(Ty::UserFunc { fun_idx, .. }) => *fun_idx,
                _ => {
                    return Err(TypeError {
                        structured: None,
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
                structured: None,
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

    pub(in crate::checker) fn check_trait_impl_def(
        &mut self,
        span: &Span,
        trait_id: &ResolvedId,
        trait_args: &[AstTy],
        target_ast_ty: &AstTy,
        where_clause: Option<&ResolvedWhereClause>,
        _methods: &[ResolvedTraitImplMethod],
    ) -> Result<TypedNode, TypeError> {
        let trait_key = self.trait_key(trait_id);
        let trait_info = self
            .traits
            .get(&trait_key)
            .cloned()
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown trait: {}", trait_id.name),
                span: span.clone(),
                hint: None,
            })?;
        let (_, target_ty, _, _) =
            self.resolve_trait_impl_head_tys(&trait_info, trait_args, target_ast_ty)?;
        let target_name = self
            .trait_target_name(&target_ty)
            .ok_or_else(|| TypeError {
                structured: None,
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
}
