use super::*;

impl Checker {
    pub(super) fn rewrite_do_safebind_specializations(
        &mut self,
        control: Box<TypedDoSafeBind>,
        span: &Span,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<Box<TypedDoSafeBind>, Box<TypeError>> {
        let TypedDoSafeBind {
            pattern,
            rhs,
            projection,
            failure_target,
            continuation,
            origins,
        } = *control;
        let rhs = self.rewrite_specializations_in_node(
            *rhs,
            defs_by_fun_idx,
            bound_tyvars_by_fun_idx,
            needs_specialization,
            specialization_fun_idxs,
            generated_defs,
        )?;
        let pattern_input_ty = match &projection {
            SafeBindRhsProjection::CanonicalResultOnce { payload_ty, .. } => payload_ty,
            SafeBindRhsProjection::CanonicalResultUnitChain { .. } => &Ty::Unit,
            SafeBindRhsProjection::PatternInput { pattern_input_ty } => pattern_input_ty,
        };
        let pattern = self.concretize_specialized_typed_pattern(
            pattern,
            Some(pattern_input_ty),
            span,
            &mut SpecializationContext {
                defs_by_fun_idx,
                bound_tyvars_by_fun_idx,
                needs_specialization,
                specialization_fun_idxs,
                generated_defs,
            },
        )?;
        let failure_target = match failure_target {
            SafeBindFailureTarget::DoMonadFail(mut target) => {
                target.call = self.rewrite_specializations_in_node(
                    *target.call,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?;
                SafeBindFailureTarget::DoMonadFail(target)
            }
            SafeBindFailureTarget::EnclosingMonadFail(mut target) => {
                target.call = self.rewrite_specializations_in_node(
                    *target.call,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?;
                SafeBindFailureTarget::EnclosingMonadFail(target)
            }
            SafeBindFailureTarget::DoAlternative { empty } => {
                SafeBindFailureTarget::DoAlternative {
                    empty: self.rewrite_specializations_in_node(
                        *empty,
                        defs_by_fun_idx,
                        bound_tyvars_by_fun_idx,
                        needs_specialization,
                        specialization_fun_idxs,
                        generated_defs,
                    )?,
                }
            }
            SafeBindFailureTarget::Deferred(deferred) => {
                let deferred = DeferredDoFailureTarget {
                    carrier_ty: deferred.carrier_ty,
                    alternative_trait_key: deferred.alternative_trait_key,
                    alternative_method_name: deferred.alternative_method_name,
                    propagated_error_tys: deferred.propagated_error_tys,
                    failure_span: deferred.failure_span,
                    do_keyword_span: deferred.do_keyword_span,
                };
                match self.resolve_deferred_do_failure(deferred)? {
                    ResolvedDeferredDoFailure::Result(mut target) => {
                        target.call = self.rewrite_specializations_in_node(
                            *target.call,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )?;
                        SafeBindFailureTarget::DoMonadFail(Box::new(target))
                    }
                    ResolvedDeferredDoFailure::Alternative(empty) => {
                        let empty = self.rewrite_specializations_in_node(
                            empty,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )?;
                        SafeBindFailureTarget::DoAlternative { empty }
                    }
                }
            }
            other => other,
        };
        let continuation = self.rewrite_specializations_in_node(
            *continuation,
            defs_by_fun_idx,
            bound_tyvars_by_fun_idx,
            needs_specialization,
            specialization_fun_idxs,
            generated_defs,
        )?;
        Ok(Box::new(TypedDoSafeBind {
            pattern,
            rhs,
            projection,
            failure_target,
            continuation,
            origins,
        }))
    }

    fn rewrite_bind_specializations(
        &mut self,
        pattern: TypedPattern,
        rhs: Box<TypedNode>,
        span: &Span,
        context: &mut SpecializationContext<'_>,
    ) -> Result<Box<TypedInner>, Box<TypeError>> {
        let rhs = self.rewrite_specializations_in_node(
            *rhs,
            context.defs_by_fun_idx,
            context.bound_tyvars_by_fun_idx,
            context.needs_specialization,
            context.specialization_fun_idxs,
            context.generated_defs,
        )?;
        let pattern =
            self.concretize_specialized_typed_pattern(pattern, Some(&rhs.ty), span, context)?;
        Ok(Box::new(TypedInner::Bind(pattern, rhs)))
    }

    // Keep the large Pattern result and error temporaries out of every recursive
    // specialization frame, including trees which never contain apply_pattern.
    fn rewrite_apply_pattern_specializations(
        &mut self,
        value: Box<TypedNode>,
        pattern: TypedPattern,
        projections: Vec<(ResolvedId, Ty)>,
        span: &Span,
        context: &mut SpecializationContext<'_>,
    ) -> Result<Box<TypedInner>, Box<TypeError>> {
        let value = self.rewrite_specializations_in_node(
            *value,
            context.defs_by_fun_idx,
            context.bound_tyvars_by_fun_idx,
            context.needs_specialization,
            context.specialization_fun_idxs,
            context.generated_defs,
        )?;
        let pattern =
            self.concretize_specialized_typed_pattern(pattern, Some(&value.ty), span, context)?;
        Ok(Box::new(TypedInner::ApplyPattern {
            value,
            pattern,
            projections: projections
                .into_iter()
                .map(|(id, ty)| (id, self.resolve_ty(&ty)))
                .collect(),
        }))
    }

    // Retain a small dispatcher while descending structural expression trees.
    // Specialization temporaries belong to separate helper frames; both sides
    // of the recursive result remain pointer-sized.
    pub(super) fn rewrite_specializations_in_node(
        &mut self,
        node: TypedNode,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<Box<TypedNode>, Box<TypeError>> {
        match &node.node {
            TypedInner::App(..) => self.rewrite_app_specializations_in_node(
                node,
                defs_by_fun_idx,
                bound_tyvars_by_fun_idx,
                needs_specialization,
                specialization_fun_idxs,
                generated_defs,
            ),
            TypedInner::Block(..) => self.rewrite_block_specializations_in_node(
                node,
                defs_by_fun_idx,
                bound_tyvars_by_fun_idx,
                needs_specialization,
                specialization_fun_idxs,
                generated_defs,
            ),
            TypedInner::Match(..) => self.rewrite_match_specializations_in_node(
                node,
                defs_by_fun_idx,
                bound_tyvars_by_fun_idx,
                needs_specialization,
                specialization_fun_idxs,
                generated_defs,
            ),
            TypedInner::Closure(..)
            | TypedInner::ExtractorClosure(..)
            | TypedInner::CaptureClosure(..)
            | TypedInner::CaptureConstructorClosure(..) => self
                .rewrite_closure_specializations_in_node(
                    node,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                ),
            TypedInner::Def(..) | TypedInner::ExtractorDef(..) => self
                .rewrite_callable_definition_specializations_in_node(
                    node,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                ),
            TypedInner::Lit(..)
            | TypedInner::ErrorKind(_)
            | TypedInner::Var(..)
            | TypedInner::SupervisorSpawn { .. }
            | TypedInner::SupervisorAdopt { .. }
            | TypedInner::SupervisorStatus { .. }
            | TypedInner::SupervisorWorkers { .. }
            | TypedInner::TraitCall { .. }
            | TypedInner::InjectCall(..)
            | TypedInner::Bind(..)
            | TypedInner::ApplyPattern { .. }
            | TypedInner::SafeBind(..)
            | TypedInner::DoSafeBind(..)
            | TypedInner::BinOp(..)
            | TypedInner::Pipe(..)
            | TypedInner::Compose(..)
            | TypedInner::ListNil
            | TypedInner::ListCons(..)
            | TypedInner::ListLiteral(..)
            | TypedInner::HashMapLiteral(..)
            | TypedInner::TupleLiteral(..)
            | TypedInner::InterpolatedStr(..)
            | TypedInner::Dbg(..)
            | TypedInner::EagerBoundary(..)
            | TypedInner::If(..)
            | TypedInner::Cause(..)
            | TypedInner::FieldAccess(..)
            | TypedInner::ProcessContextHandler { .. }
            | TypedInner::FacetPath(..)
            | TypedInner::PendingFacetPath(..)
            | TypedInner::FacetView { .. }
            | TypedInner::FacetSet { .. }
            | TypedInner::FacetOver { .. }
            | TypedInner::StructLit(..)
            | TypedInner::ConstructorCall(..)
            | TypedInner::DeferrorDef(..)
            | TypedInner::ErrorConstruct { .. }
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::Capture(..)
            | TypedInner::StructDef(..)
            | TypedInner::RecordDef(..)
            | TypedInner::EnumDef(..)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::Semi(..) => self.rewrite_other_specializations_in_node(
                node,
                defs_by_fun_idx,
                bound_tyvars_by_fun_idx,
                needs_specialization,
                specialization_fun_idxs,
                generated_defs,
            ),
        }
    }

    #[inline(never)]
    fn rewrite_app_specializations_in_node(
        &mut self,
        node: TypedNode,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<Box<TypedNode>, Box<TypeError>> {
        let span = node.span.clone();
        let mut ty = node.ty.clone();
        let node = match node.node {
            TypedInner::App(func, args) => {
                let func = *self.rewrite_specializations_in_node(
                    *func,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?;
                let args = args
                    .into_iter()
                    .map(|arg| {
                        self.rewrite_specializations_in_node(
                            arg,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                        .map(|node| *node)
                    })
                    .collect::<Result<Vec<_>, _>>()?;

                if let Ty::UserFunc { fun_idx, .. } = &func.ty {
                    if needs_specialization.contains(fun_idx) {
                        let original_def =
                            defs_by_fun_idx.get(fun_idx).ok_or_else(|| TypeError {
                                structured: None,
                                message: format!(
                                    "Missing generic definition for fun_idx {}",
                                    fun_idx
                                ),
                                span: span.clone(),
                                hint: None,
                            })?;
                        let requirements = bound_tyvars_by_fun_idx
                            .get(fun_idx)
                            .expect("specialization requirements exist for every definition");
                        let bound_tyvars = &requirements.tyvars;
                        let mapping = self.infer_specialization_mapping(
                            original_def,
                            &args,
                            Some(&func.ty),
                            false,
                            &bound_tyvars,
                        )?;
                        let fully_concrete = mapping.len() == bound_tyvars.len()
                            && bound_tyvars.iter().all(|var| {
                                mapping
                                    .get(var)
                                    .is_some_and(Self::specialization_type_is_concrete)
                            });
                        // Type variables are erased at runtime. A definition
                        // without pending trait dispatch can therefore be
                        // materialized safely even while a callable result is
                        // still waiting for a later type witness. This keeps a
                        // valid function-table entry instead of leaking the
                        // pre-normalization generic index into Forge.
                        if fully_concrete || !requirements.requires_concrete {
                            let concrete_tys = bound_tyvars
                                .iter()
                                .map(|var| mapping.get(var).cloned().unwrap_or(Ty::Var(*var)))
                                .collect::<Vec<_>>();
                            let specialized_fun_idx = self.ensure_specialized_def(
                                *fun_idx,
                                &concrete_tys,
                                &mapping,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )?;
                            let specialized_func_ty = match func.ty.clone() {
                                Ty::UserFunc {
                                    type_params,
                                    call_substitution,
                                    params,
                                    ret,
                                    ..
                                } => {
                                    let call_site_mapping = call_substitution
                                        .iter()
                                        .map(|(variable, ty)| (*variable, self.resolve_ty(ty)))
                                        .collect::<HashMap<_, _>>();
                                    let (params, ret) = if fully_concrete {
                                        match &original_def.node {
                                            TypedInner::Def(
                                                _,
                                                _,
                                                _,
                                                declared_params,
                                                declared_ret,
                                                ..,
                                            ) => (
                                                declared_params
                                                    .iter()
                                                    .map(|parameter| {
                                                        self.substitute_ty_with_mapping(
                                                            &parameter.ty,
                                                            &call_site_mapping,
                                                        )
                                                    })
                                                    .collect(),
                                                Box::new(self.substitute_ty_with_mapping(
                                                    declared_ret,
                                                    &call_site_mapping,
                                                )),
                                            ),
                                            TypedInner::ExtractorDef(
                                                _,
                                                _,
                                                _,
                                                declared_param,
                                                declared_ret,
                                                ..,
                                            ) => (
                                                declared_param
                                                    .iter()
                                                    .map(|param| {
                                                        self.substitute_ty_with_mapping(
                                                            &param.ty,
                                                            &call_site_mapping,
                                                        )
                                                    })
                                                    .collect(),
                                                Box::new(self.substitute_ty_with_mapping(
                                                    declared_ret,
                                                    &call_site_mapping,
                                                )),
                                            ),
                                            _ => (params, ret),
                                        }
                                    } else {
                                        (params, ret)
                                    };
                                    Ty::UserFunc {
                                        fun_idx: specialized_fun_idx,
                                        type_params,
                                        call_substitution: if fully_concrete {
                                            Vec::new()
                                        } else {
                                            call_substitution
                                        },
                                        params,
                                        ret,
                                    }
                                }
                                other => other,
                            };
                            if let Ty::UserFunc { ret, .. } = &specialized_func_ty {
                                ty = ret.as_ref().clone();
                            }
                            let specialized_func = TypedNode {
                                ty: specialized_func_ty,
                                span: func.span.clone(),
                                node: func.node,
                            };
                            TypedInner::App(Box::new(specialized_func), args)
                        } else {
                            TypedInner::App(Box::new(func), args)
                        }
                    } else {
                        TypedInner::App(Box::new(func), args)
                    }
                } else {
                    TypedInner::App(Box::new(func), args)
                }
            }
            _ => unreachable!("specialization dispatcher selected a non-app node"),
        };

        Ok(Box::new(TypedNode { ty, span, node }))
    }

    #[inline(never)]
    fn rewrite_block_specializations_in_node(
        &mut self,
        node: TypedNode,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<Box<TypedNode>, Box<TypeError>> {
        let span = node.span.clone();
        let mut ty = node.ty.clone();
        let node = match node.node {
            TypedInner::Block(stmts) => {
                let stmts = stmts
                    .into_iter()
                    .map(|stmt| {
                        self.rewrite_specializations_in_node(
                            stmt,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                        .map(|node| *node)
                    })
                    .collect::<Result<Vec<_>, Box<TypeError>>>()?;
                if let Some(last) = stmts.last() {
                    ty = last.ty.clone();
                }
                TypedInner::Block(stmts)
            }
            _ => unreachable!("specialization dispatcher selected a non-block node"),
        };

        Ok(Box::new(TypedNode { ty, span, node }))
    }

    #[inline(never)]
    fn rewrite_match_specializations_in_node(
        &mut self,
        node: TypedNode,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<Box<TypedNode>, Box<TypeError>> {
        let span = node.span.clone();
        let ty = node.ty.clone();
        let node = match node.node {
            TypedInner::Match(scrutinee, arms) => TypedInner::Match(
                self.rewrite_specializations_in_node(
                    *scrutinee,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                arms.into_iter()
                    .map(|arm| {
                        Ok(TypedMatchArm {
                            direct_expression: arm.direct_expression,
                            pattern: self.concretize_specialized_match_pattern(
                                arm.pattern,
                                &span,
                                &mut SpecializationContext {
                                    defs_by_fun_idx,
                                    bound_tyvars_by_fun_idx,
                                    needs_specialization,
                                    specialization_fun_idxs,
                                    generated_defs,
                                },
                            )?,
                            guard: arm
                                .guard
                                .map(|guard| {
                                    self.rewrite_specializations_in_node(
                                        guard,
                                        defs_by_fun_idx,
                                        bound_tyvars_by_fun_idx,
                                        needs_specialization,
                                        specialization_fun_idxs,
                                        generated_defs,
                                    )
                                })
                                .transpose()?
                                .map(|node| *node),
                            body: *self.rewrite_specializations_in_node(
                                arm.body,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )?,
                        })
                    })
                    .collect::<Result<Vec<_>, Box<TypeError>>>()?,
            ),
            _ => unreachable!("specialization dispatcher selected a non-match node"),
        };

        Ok(Box::new(TypedNode { ty, span, node }))
    }

    #[inline(never)]
    fn rewrite_closure_specializations_in_node(
        &mut self,
        node: TypedNode,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<Box<TypedNode>, Box<TypeError>> {
        let span = node.span.clone();
        let ty = node.ty.clone();
        let node = match node.node {
            TypedInner::Closure(params, captures, body) => TypedInner::Closure(
                params,
                captures,
                self.rewrite_specializations_in_node(
                    *body,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
            ),
            TypedInner::ExtractorClosure(params, captures, body) => TypedInner::ExtractorClosure(
                params,
                captures,
                self.rewrite_specializations_in_node(
                    *body,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
            ),
            TypedInner::CaptureClosure(params, captures, body) => TypedInner::CaptureClosure(
                params,
                captures,
                self.rewrite_specializations_in_node(
                    *body,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
            ),
            TypedInner::CaptureConstructorClosure(id, params, captures, body) => {
                TypedInner::CaptureConstructorClosure(
                    id,
                    params,
                    captures,
                    self.rewrite_specializations_in_node(
                        *body,
                        defs_by_fun_idx,
                        bound_tyvars_by_fun_idx,
                        needs_specialization,
                        specialization_fun_idxs,
                        generated_defs,
                    )?,
                )
            }
            _ => unreachable!("specialization dispatcher selected a non-closure node"),
        };

        Ok(Box::new(TypedNode { ty, span, node }))
    }

    // Generic forwarding retains this frame while specializing the next callable.
    // Keep it separate from the temporaries of unrelated expression variants.
    #[inline(never)]
    fn rewrite_callable_definition_specializations_in_node(
        &mut self,
        node: TypedNode,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<Box<TypedNode>, Box<TypeError>> {
        let TypedNode { ty, span, node } = node;
        let node = match node {
            TypedInner::Def(
                fun_idx,
                id,
                type_params,
                params,
                ret_ty,
                where_clause,
                body,
                visibility,
            ) => TypedInner::Def(
                fun_idx,
                id,
                type_params,
                params,
                ret_ty,
                where_clause,
                self.rewrite_specializations_in_node(
                    *body,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                visibility,
            ),
            TypedInner::ExtractorDef(fun_idx, id, type_params, param, ret_ty, body, visibility) => {
                TypedInner::ExtractorDef(
                    fun_idx,
                    id,
                    type_params,
                    param,
                    ret_ty,
                    self.rewrite_specializations_in_node(
                        *body,
                        defs_by_fun_idx,
                        bound_tyvars_by_fun_idx,
                        needs_specialization,
                        specialization_fun_idxs,
                        generated_defs,
                    )?,
                    visibility,
                )
            }
            _ => unreachable!("specialization dispatcher selected a non-callable definition"),
        };
        Ok(Box::new(TypedNode { ty, span, node }))
    }

    #[inline(never)]
    fn rewrite_other_specializations_in_node(
        &mut self,
        node: TypedNode,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<Box<TypedNode>, Box<TypeError>> {
        let span = node.span.clone();
        let mut ty = node.ty.clone();
        let node = match node.node {
            TypedInner::Lit(lit) => TypedInner::Lit(lit),
            TypedInner::ErrorKind(kind) => TypedInner::ErrorKind(kind),
            TypedInner::Var(id) => TypedInner::Var(id),
            TypedInner::SupervisorSpawn {
                supervisor_process,
                worker_process,
                init,
            } => TypedInner::SupervisorSpawn {
                supervisor_process,
                worker_process,
                init: self.rewrite_specializations_in_node(
                    *init,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
            },
            TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid,
            } => TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid: self.rewrite_specializations_in_node(
                    *pid,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
            },
            TypedInner::SupervisorStatus { supervisor_process } => {
                TypedInner::SupervisorStatus { supervisor_process }
            }
            TypedInner::SupervisorWorkers {
                supervisor_process,
                worker_process,
                init,
                strategy,
            } => TypedInner::SupervisorWorkers {
                supervisor_process,
                worker_process,
                init: self.rewrite_specializations_in_node(
                    *init,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                strategy: self.rewrite_specializations_in_node(
                    *strategy,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
            },
            TypedInner::TraitCall {
                trait_name,
                method_name,
                receiver_ty,
                obligation,
                dispatch,
                origin,
                args,
            } => {
                let mut args = args
                    .into_iter()
                    .map(|arg| {
                        self.rewrite_specializations_in_node(
                            arg,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                        .map(|node| *node)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                for argument in &mut args {
                    argument.ty = self.normalize_executable_constructor_identities(&argument.ty);
                }
                let receiver_ty = self.resolve_ty(&receiver_ty);
                let obligation = TraitObligation {
                    trait_id: obligation.trait_id,
                    trait_args: obligation
                        .trait_args
                        .iter()
                        .map(|arg| self.resolve_ty(arg))
                        .collect(),
                    receiver: self.resolve_ty(&obligation.receiver),
                };
                let instantiation = if let TraitDispatch::Selected(instantiation) = &dispatch {
                    Some(instantiation.as_ref().clone())
                } else if matches!(dispatch, TraitDispatch::Pending)
                    || matches!(&dispatch,
                        TraitDispatch::Static(TraitDispatchTarget::UserFunction { fun_idx, .. })
                            if needs_specialization.contains(fun_idx))
                {
                    let argument_tys = args
                        .iter()
                        .map(|arg| self.resolve_ty(&arg.ty))
                        .collect::<Vec<_>>();
                    Some(
                        match self.select_trait_method_instantiation(
                            &obligation.trait_id,
                            &method_name,
                            &obligation.receiver,
                            &obligation.trait_args,
                            &argument_tys,
                            &self.resolve_ty(&ty),
                        )? {
                            CandidateApplicability::Applicable(instantiation) => instantiation,
                            CandidateApplicability::Deferred(pending) => {
                                let mut error = self.ambiguous_constructor_result(
                                    &trait_name,
                                    &method_name,
                                    &span,
                                );
                                error.hint = Some(format!(
                                    "Add enough type information to choose among {} remaining candidate(s) and resolve {} constructor input(s).",
                                    pending.candidates.len(),
                                    pending.waiting_on.len()
                                ));
                                return Err(Box::new(error));
                            }
                            CandidateApplicability::Rejected(_) => {
                                return Err(Box::new(self.trait_dispatch_failure(
                                    TypeDiagnosticReason::NoApplicableTraitImplementation,
                                    &trait_name,
                                    &method_name,
                                    Some(&obligation.receiver),
                                    &span,
                                )));
                            }
                        },
                    )
                } else {
                    None
                };
                let dispatch = if let Some(instantiation) = instantiation {
                    self.materialize_trait_method_instantiation(
                        instantiation,
                        &span,
                        &mut SpecializationContext {
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        },
                    )?
                } else {
                    dispatch
                };
                let receiver_ty = self.normalize_executable_constructor_identities(&receiver_ty);
                let obligation = TraitObligation {
                    trait_id: obligation.trait_id.clone(),
                    trait_args: obligation
                        .trait_args
                        .iter()
                        .enumerate()
                        .map(|(ordinal, argument)| {
                            self.normalize_executable_trait_argument(
                                &obligation.trait_id,
                                ordinal,
                                argument,
                            )
                        })
                        .collect(),
                    receiver: self
                        .normalize_executable_constructor_identities(&obligation.receiver),
                };
                ty = self.normalize_executable_constructor_identities(&ty);
                TypedInner::TraitCall {
                    trait_name,
                    method_name,
                    receiver_ty,
                    obligation,
                    dispatch,
                    origin,
                    args,
                }
            }
            TypedInner::InjectCall(func, args) => {
                let mut target = *self.rewrite_specializations_in_node(
                    *func,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?;
                let args = args
                    .into_iter()
                    .map(|arg| {
                        self.rewrite_specializations_in_node(
                            arg,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                        .map(|node| *node)
                    })
                    .collect::<Result<Vec<_>, Box<TypeError>>>()?;
                self.specialize_callable_reference(
                    &mut target,
                    &mut SpecializationContext {
                        defs_by_fun_idx,
                        bound_tyvars_by_fun_idx,
                        needs_specialization,
                        specialization_fun_idxs,
                        generated_defs,
                    },
                )?;
                TypedInner::InjectCall(Box::new(target), args)
            }
            TypedInner::Bind(pattern, rhs) => *self.rewrite_bind_specializations(
                pattern,
                rhs,
                &span,
                &mut SpecializationContext {
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                },
            )?,
            TypedInner::ApplyPattern {
                value,
                pattern,
                projections,
            } => *self.rewrite_apply_pattern_specializations(
                value,
                pattern,
                projections,
                &span,
                &mut SpecializationContext {
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                },
            )?,
            TypedInner::SafeBind(pattern, rhs, projection, failure_target) => {
                let rhs = self.rewrite_specializations_in_node(
                    *rhs,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?;
                let pattern_input_ty = match &projection {
                    SafeBindRhsProjection::CanonicalResultOnce { payload_ty, .. } => payload_ty,
                    SafeBindRhsProjection::CanonicalResultUnitChain { .. } => &Ty::Unit,
                    SafeBindRhsProjection::PatternInput { pattern_input_ty } => pattern_input_ty,
                };
                let pattern = self.concretize_specialized_typed_pattern(
                    pattern,
                    Some(pattern_input_ty),
                    &span,
                    &mut SpecializationContext {
                        defs_by_fun_idx,
                        bound_tyvars_by_fun_idx,
                        needs_specialization,
                        specialization_fun_idxs,
                        generated_defs,
                    },
                )?;
                let failure_target = match failure_target {
                    SafeBindFailureTarget::EnclosingMonadFail(mut target) => {
                        target.call = self.rewrite_specializations_in_node(
                            *target.call,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )?;
                        SafeBindFailureTarget::EnclosingMonadFail(target)
                    }
                    other => other,
                };
                TypedInner::SafeBind(pattern, rhs, projection, failure_target)
            }
            TypedInner::DoSafeBind(control) => {
                TypedInner::DoSafeBind(self.rewrite_do_safebind_specializations(
                    control,
                    &span,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?)
            }
            TypedInner::BinOp(op, left, right) => TypedInner::BinOp(
                op,
                self.rewrite_specializations_in_node(
                    *left,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                self.rewrite_specializations_in_node(
                    *right,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
            ),
            TypedInner::Pipe(left, right) => TypedInner::Pipe(
                self.rewrite_specializations_in_node(
                    *left,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                self.rewrite_specializations_in_node(
                    *right,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
            ),
            TypedInner::Compose(flavor, left, right) => TypedInner::Compose(
                flavor,
                self.rewrite_specializations_in_node(
                    *left,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                self.rewrite_specializations_in_node(
                    *right,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
            ),
            TypedInner::ListNil => TypedInner::ListNil,
            TypedInner::ListCons(head, tail) => TypedInner::ListCons(
                self.rewrite_specializations_in_node(
                    *head,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                self.rewrite_specializations_in_node(
                    *tail,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
            ),
            TypedInner::ListLiteral(items) => TypedInner::ListLiteral(
                items
                    .into_iter()
                    .map(|item| {
                        self.rewrite_specializations_in_node(
                            item,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                        .map(|node| *node)
                    })
                    .collect::<Result<Vec<_>, Box<TypeError>>>()?,
            ),
            TypedInner::HashMapLiteral(entries) => TypedInner::HashMapLiteral(
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        Ok((
                            self.rewrite_specializations_in_node(
                                key,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )
                            .map(|node| *node)?,
                            self.rewrite_specializations_in_node(
                                value,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )
                            .map(|node| *node)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, Box<TypeError>>>()?,
            ),
            TypedInner::ErrorConstruct {
                kind,
                message,
                payload,
                payload_fields,
            } => TypedInner::ErrorConstruct {
                kind,
                message: self.rewrite_specializations_in_node(
                    *message,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                payload: payload
                    .into_iter()
                    .map(|value| {
                        self.rewrite_specializations_in_node(
                            value,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                        .map(|node| *node)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                payload_fields,
            },
            TypedInner::TupleLiteral(items) => TypedInner::TupleLiteral(
                items
                    .into_iter()
                    .map(|item| {
                        self.rewrite_specializations_in_node(
                            item,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                        .map(|node| *node)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::InterpolatedStr(parts) => TypedInner::InterpolatedStr(
                parts
                    .into_iter()
                    .map(|part| match part {
                        TypedInterpolatedPart::Text(text) => Ok(TypedInterpolatedPart::Text(text)),
                        TypedInterpolatedPart::Expr(expr) => Ok(TypedInterpolatedPart::Expr(
                            self.rewrite_specializations_in_node(
                                *expr,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )?,
                        )),
                    })
                    .collect::<Result<Vec<_>, Box<TypeError>>>()?,
            ),
            TypedInner::Dbg(args) => TypedInner::Dbg(
                args.into_iter()
                    .map(|arg| {
                        Ok(TypedDbgArg {
                            span: arg.span,
                            ty_name: arg.ty_name,
                            expr: *self.rewrite_specializations_in_node(
                                arg.expr,
                                defs_by_fun_idx,
                                bound_tyvars_by_fun_idx,
                                needs_specialization,
                                specialization_fun_idxs,
                                generated_defs,
                            )?,
                        })
                    })
                    .collect::<Result<Vec<_>, Box<TypeError>>>()?,
            ),
            TypedInner::EagerBoundary(inner) => {
                TypedInner::EagerBoundary(self.rewrite_specializations_in_node(
                    *inner,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?)
            }
            TypedInner::If(cond, then_branch, else_branch) => TypedInner::If(
                self.rewrite_specializations_in_node(
                    *cond,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                self.rewrite_specializations_in_node(
                    *then_branch,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                else_branch
                    .map(|branch| {
                        self.rewrite_specializations_in_node(
                            *branch,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                    })
                    .transpose()?,
            ),
            TypedInner::Cause(value, err) => TypedInner::Cause(
                self.rewrite_specializations_in_node(
                    *value,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                self.rewrite_specializations_in_node(
                    *err,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
            ),
            TypedInner::FieldAccess(expr, index) => TypedInner::FieldAccess(
                self.rewrite_specializations_in_node(
                    *expr,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                index,
            ),
            TypedInner::ProcessContextHandler { process_name, slot } => {
                TypedInner::ProcessContextHandler { process_name, slot }
            }
            TypedInner::FacetPath(path) => {
                TypedInner::FacetPath(self.rewrite_specializations_in_facet_path(
                    path,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?)
            }
            TypedInner::PendingFacetPath(path) => {
                TypedInner::PendingFacetPath(self.rewrite_specializations_in_pending_facet_path(
                    path,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?)
            }
            TypedInner::FacetView {
                api,
                source,
                path,
                source_is_result,
            } => TypedInner::FacetView {
                api,
                source: self.rewrite_specializations_in_node(
                    *source,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                path: self.rewrite_specializations_in_facet_path(
                    path,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                source_is_result,
            },
            TypedInner::FacetSet {
                api,
                source,
                path,
                value,
                source_is_result,
                mode,
            } => TypedInner::FacetSet {
                api,
                source: self.rewrite_specializations_in_node(
                    *source,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                path: self.rewrite_specializations_in_facet_path(
                    path,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                value: self.rewrite_specializations_in_node(
                    *value,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                source_is_result,
                mode,
            },
            TypedInner::FacetOver {
                api,
                source,
                path,
                update_fun,
                source_is_result,
                mode,
            } => TypedInner::FacetOver {
                api,
                source: self.rewrite_specializations_in_node(
                    *source,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                path: self.rewrite_specializations_in_facet_path(
                    path,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                update_fun: self.rewrite_specializations_in_node(
                    *update_fun,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                source_is_result,
                mode,
            },
            TypedInner::StructLit(tag, fields) => TypedInner::StructLit(
                tag,
                fields
                    .into_iter()
                    .map(|field| {
                        self.rewrite_specializations_in_node(
                            field,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                        .map(|node| *node)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::ConstructorCall(tag, fields) => TypedInner::ConstructorCall(
                tag,
                fields
                    .into_iter()
                    .map(|field| {
                        self.rewrite_specializations_in_node(
                            field,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                        .map(|node| *node)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            TypedInner::DeferrorDef(tag, binding, id, params, show) => TypedInner::DeferrorDef(
                tag,
                binding,
                id,
                params,
                self.rewrite_specializations_in_node(
                    *show,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
            ),
            TypedInner::BuiltinExtractorDecl(id, param_ty, ret_ty) => {
                TypedInner::BuiltinExtractorDecl(id, param_ty, ret_ty)
            }
            TypedInner::Capture(target, args) => {
                let mut target = *self.rewrite_specializations_in_node(
                    *target,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?;
                let args = args
                    .into_iter()
                    .map(|arg| {
                        self.rewrite_specializations_in_node(
                            arg,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )
                        .map(|node| *node)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if args.is_empty() {
                    self.specialize_callable_reference(
                        &mut target,
                        &mut SpecializationContext {
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        },
                    )?;
                }
                TypedInner::Capture(Box::new(target), args)
            }
            TypedInner::StructDef(tag, name, field_names, field_policies) => {
                TypedInner::StructDef(tag, name, field_names, field_policies)
            }
            TypedInner::RecordDef(tag, name, field_names, field_policies) => {
                TypedInner::RecordDef(tag, name, field_names, field_policies)
            }
            TypedInner::EnumDef(name, variants) => TypedInner::EnumDef(name, variants),
            TypedInner::TraitDef(name, where_clause, methods) => {
                TypedInner::TraitDef(name, where_clause, methods)
            }
            TypedInner::TraitImplDef(trait_name, target_name, where_clause) => {
                TypedInner::TraitImplDef(trait_name, target_name, where_clause)
            }
            TypedInner::Semi(inner) => TypedInner::Semi(self.rewrite_specializations_in_node(
                *inner,
                defs_by_fun_idx,
                bound_tyvars_by_fun_idx,
                needs_specialization,
                specialization_fun_idxs,
                generated_defs,
            )?),
            _ => unreachable!("specialization dispatcher selected a non-other node"),
        };

        Ok(Box::new(TypedNode { ty, span, node }))
    }

    fn rewrite_specializations_in_facet_path(
        &mut self,
        mut path: TypedFacetPath,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<TypedFacetPath, Box<TypeError>> {
        path.segments = path
            .segments
            .into_iter()
            .map(|segment| {
                self.rewrite_specializations_in_facet_segment(
                    segment,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(path)
    }

    fn rewrite_specializations_in_facet_segment(
        &mut self,
        segment: TypedFacetSegment,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<TypedFacetSegment, Box<TypeError>> {
        Ok(match segment {
            TypedFacetSegment::ListIndex {
                index,
                display,
                literal_index,
            } => TypedFacetSegment::ListIndex {
                index: self.rewrite_specializations_in_node(
                    *index,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                display,
                literal_index,
            },
            TypedFacetSegment::ListRange {
                start,
                end,
                display,
                literal_start,
                literal_end,
            } => TypedFacetSegment::ListRange {
                start: self.rewrite_specializations_in_node(
                    *start,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                end: self.rewrite_specializations_in_node(
                    *end,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                display,
                literal_start,
                literal_end,
            },
            TypedFacetSegment::MapKey {
                key,
                display,
                literal_key,
            } => TypedFacetSegment::MapKey {
                key: self.rewrite_specializations_in_node(
                    *key,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )?,
                display,
                literal_key,
            },
            other => other,
        })
    }

    fn rewrite_specializations_in_pending_facet_path(
        &mut self,
        mut path: PendingFacetPath,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<PendingFacetPath, Box<TypeError>> {
        path.segments = path
            .segments
            .into_iter()
            .map(|segment| {
                self.rewrite_specializations_in_pending_facet_segment(
                    segment,
                    defs_by_fun_idx,
                    bound_tyvars_by_fun_idx,
                    needs_specialization,
                    specialization_fun_idxs,
                    generated_defs,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(path)
    }

    fn rewrite_specializations_in_pending_facet_segment(
        &mut self,
        segment: PendingFacetSegment,
        defs_by_fun_idx: &SpecializableDefinitions,
        bound_tyvars_by_fun_idx: &HashMap<u32, CallableSpecializationRequirements>,
        needs_specialization: &HashSet<u32>,
        specialization_fun_idxs: &mut HashMap<CallableInstantiationKey, u32>,
        generated_defs: &mut Vec<TypedNode>,
    ) -> Result<PendingFacetSegment, Box<TypeError>> {
        let mut rewrite_expr =
            |this: &mut Self, expr| -> Result<PendingFacetExpr, Box<TypeError>> {
                match expr {
                    PendingFacetExpr::Resolved(expr) => Ok(PendingFacetExpr::Resolved(expr)),
                    PendingFacetExpr::Typed(expr) => Ok(PendingFacetExpr::Typed(
                        this.rewrite_specializations_in_node(
                            *expr,
                            defs_by_fun_idx,
                            bound_tyvars_by_fun_idx,
                            needs_specialization,
                            specialization_fun_idxs,
                            generated_defs,
                        )?,
                    )),
                }
            };
        Ok(match segment {
            PendingFacetSegment::Bracket { expr, display } => PendingFacetSegment::Bracket {
                expr: rewrite_expr(self, expr)?,
                display,
            },
            PendingFacetSegment::RangeBracket {
                start,
                end,
                display,
            } => PendingFacetSegment::RangeBracket {
                start: rewrite_expr(self, start)?,
                end: rewrite_expr(self, end)?,
                display,
            },
            other => other,
        })
    }

    pub(super) fn specialize_callable_reference(
        &mut self,
        target: &mut TypedNode,
        context: &mut SpecializationContext<'_>,
    ) -> Result<(), TypeError> {
        if let Ty::UserFunc {
            fun_idx,
            type_params,
            call_substitution,
            params,
            ret,
        } = target.ty.clone()
        {
            if context.needs_specialization.contains(&fun_idx) {
                let original_def =
                    context
                        .defs_by_fun_idx
                        .get(&fun_idx)
                        .ok_or_else(|| TypeError {
                            structured: None,
                            message: format!("Missing generic definition for fun_idx {}", fun_idx),
                            span: target.span.clone(),
                            hint: None,
                        })?;
                let requirements = context
                    .bound_tyvars_by_fun_idx
                    .get(&fun_idx)
                    .expect("specialization requirements exist for every definition");
                let bound_tyvars = &requirements.tyvars;
                // The callable signature already carries the declaration's substitution.
                // InjectCall's bound arguments omit the injected value, so they are
                // not a complete argument list for ordinary call specialization.
                let mapping = self.infer_specialization_mapping(
                    original_def,
                    &[],
                    Some(&target.ty),
                    true,
                    &bound_tyvars,
                )?;
                let fully_concrete = mapping.len() == bound_tyvars.len()
                    && bound_tyvars.iter().all(|var| {
                        mapping
                            .get(var)
                            .is_some_and(Self::specialization_type_is_concrete)
                    });
                if fully_concrete || !requirements.requires_concrete {
                    let concrete_tys = bound_tyvars
                        .iter()
                        .map(|var| mapping.get(var).cloned().unwrap_or(Ty::Var(*var)))
                        .collect::<Vec<_>>();
                    let specialized_fun_idx = self.ensure_specialized_def(
                        fun_idx,
                        &concrete_tys,
                        &mapping,
                        context.defs_by_fun_idx,
                        context.bound_tyvars_by_fun_idx,
                        context.needs_specialization,
                        context.specialization_fun_idxs,
                        context.generated_defs,
                    )?;
                    target.ty = Ty::UserFunc {
                        fun_idx: specialized_fun_idx,
                        type_params,
                        call_substitution: if fully_concrete {
                            Vec::new()
                        } else {
                            call_substitution
                        },
                        params,
                        ret,
                    };
                }
            }
        }
        Ok(())
    }
}
