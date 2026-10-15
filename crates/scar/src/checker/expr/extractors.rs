use super::*;

impl Checker {
    pub(in crate::checker) fn match_result_variant_tags(
        &self,
        span: &Span,
    ) -> Result<(u32, u32), TypeError> {
        let variants = self
            .lookup_enum_variants_of("MatchResult")
            .ok_or_else(|| TypeError {
                structured: None,
                message: "MatchResult enum is not available in the current environment".into(),
                span: span.clone(),
                hint: None,
            })?;
        let mut ok_tag = None;
        let mut err_tag = None;
        for variant in variants {
            match variant.short_name.as_str() {
                "Ok" => ok_tag = Some(variant.tag),
                "Err" => err_tag = Some(variant.tag),
                _ => {}
            }
        }
        match (ok_tag, err_tag) {
            (Some(ok), Some(err)) => Ok((ok, err)),
            _ => Err(TypeError {
                structured: None,
                message: "MatchResult enum must define Ok and Err variants".into(),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    pub(in crate::checker) fn extractor_signature_ty(ty: &Ty) -> &Ty {
        match ty {
            Ty::ExtractorClosure(signature) => signature,
            other => other,
        }
    }

    pub(in crate::checker) fn extractor_contract(
        &mut self,
        extractor_id: &ResolvedId,
        extractor_ty: &Ty,
        span: &Span,
    ) -> Result<(Ty, Vec<Ty>, u32, u32), TypeError> {
        let (params, ret) = match Self::extractor_signature_ty(extractor_ty) {
            Ty::BuiltinFunc { params, ret, .. }
            | Ty::UserFunc { params, ret, .. }
            | Ty::Func(params, ret) => (params.clone(), ret.as_ref().clone()),
            other => {
                return Err(TypeError {
                    structured: None,
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
        if params.is_empty() {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Extractor {} must accept at least one input value, got {} parameter(s)",
                    extractor_id.name,
                    params.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }
        let input_ty = params.last().expect("nonempty parameters checked").clone();
        let seq_tys = self.require_extractor_match_result_payload_ty(
            &self.resolve_ty(&ret),
            span,
            &extractor_id.name,
        )?;
        let (success_tag, err_tag) = self.match_result_variant_tags(span)?;
        Ok((input_ty, seq_tys, success_tag, err_tag))
    }

    pub(in crate::checker) fn extractor_callable_ty(
        &mut self,
        extractor_id: &ResolvedId,
        span: &Span,
    ) -> Result<Ty, TypeError> {
        let extractor_ty = self
            .env
            .lookup_var(extractor_id.unique_id)
            .cloned()
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Undefined extractor: {}", extractor_id.name),
                span: span.clone(),
                hint: None,
            })?;
        if matches!(extractor_ty, Ty::ExtractorClosure(_)) {
            Ok(self.resolve_ty(&extractor_ty))
        } else {
            // Named Pattern heads are call sites. Preserve the fresh solver
            // substitution created with their signature for specialization.
            let mut fresh = HashMap::new();
            if let Ty::UserFunc {
                type_params,
                call_substitution,
                ..
            } = &extractor_ty
            {
                if !call_substitution.is_empty() {
                    return Err(self.typecheck_invariant_error(
                        "named Extractor binding must retain its declaration signature",
                        span,
                    ));
                }
                for variable in type_params {
                    self.instantiate_ty_with_fresh(&Ty::Var(*variable), &mut fresh);
                }
            }
            let mut instantiated = self.instantiate_ty_with_fresh(&extractor_ty, &mut fresh);
            if let Ty::UserFunc {
                type_params,
                call_substitution,
                ..
            } = &mut instantiated
            {
                *call_substitution = type_params
                    .iter()
                    .map(|variable| {
                        fresh.get(variable).cloned().map(|ty| (*variable, ty)).ok_or_else(|| {
                            self.typecheck_invariant_error(
                                "named Extractor instantiation must preserve every declaration slot",
                                span,
                            )
                        })
                    })
                    .collect::<Result<_, _>>()?;
                call_substitution.sort_unstable_by_key(|(variable, _)| *variable);
            }
            Ok(instantiated)
        }
    }

    pub(in crate::checker) fn kernel_uncons_id(
        &self,
        span: &Span,
    ) -> Result<ResolvedId, TypeError> {
        let id = self
            .function_ids_by_name
            .get("Kernel::uncons")
            .cloned()
            .ok_or_else(|| TypeError {
                structured: None,
                message: "Missing helper function: Kernel::uncons".into(),
                span: span.clone(),
                hint: None,
            })?;
        Ok(ResolvedId {
            span: span.clone(),
            ..id
        })
    }

    pub(in crate::checker) fn uncons_contract_for_input(
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
                structured: None,
                message: format!(
                    "Extractor uncons expects List<...> or String, got {}",
                    self.ty_name(&other)
                ),
                span: span.clone(),
                hint: None,
            }),
        }
    }

    pub(in crate::checker) fn extractor_contract_for_observed_ty(
        &mut self,
        extractor_id: &ResolvedId,
        observed_ty: &Ty,
        pre_args: &[Resolved],
        span: &Span,
    ) -> Result<(Ty, Ty, Vec<TypedNode>, Vec<Ty>, u32, u32), TypeError> {
        let mut extractor_ty = self.extractor_callable_ty(extractor_id, span)?;
        let params = match Self::extractor_signature_ty(&extractor_ty) {
            Ty::UserFunc { params, .. } | Ty::BuiltinFunc { params, .. } | Ty::Func(params, _) => {
                params.clone()
            }
            _ => {
                return Err(TypeError::new(
                    "Extractor head is not callable",
                    span.clone(),
                ));
            }
        };
        let Some((input, expected_pre_args)) = params.split_last() else {
            return Err(TypeError::new(
                "Extractor requires at least one input",
                span.clone(),
            ));
        };
        if pre_args.len() != expected_pre_args.len() {
            return Err(TypeError::new(
                format!(
                    "Extractor {} requires {} pre-argument(s), got {}",
                    extractor_id.name,
                    expected_pre_args.len(),
                    pre_args.len()
                ),
                span.clone(),
            ));
        }
        self.types_compatible(input, observed_ty)?;
        let pre_args = pre_args
            .iter()
            .cloned()
            .map(ResolvedRecordLitArg::Positional)
            .collect::<Vec<_>>();
        let typed_pre_args = self.typecheck_positional_call_args(
            span,
            &extractor_id.name,
            expected_pre_args,
            &pre_args,
            None,
            "Extractor pre-arguments must be positional".into(),
        )?;
        self.ensure_no_runtime_facet_args(&typed_pre_args, span, "Extractor pre-arguments")?;

        let (input_ty, seq_tys, success_tag, err_tag) = if self
            .function_ids_by_name
            .get("Kernel::uncons")
            .is_some_and(|id| id.unique_id == extractor_id.unique_id)
            && matches!(&extractor_ty, Ty::BuiltinFunc { .. })
        {
            let (input_ty, seq_tys) = self.uncons_contract_for_input(observed_ty, span)?;
            let Ty::BuiltinFunc { params, ret, .. } = &mut extractor_ty else {
                unreachable!()
            };
            *params = vec![input_ty.clone()];
            *ret = Box::new(Ty::MatchResult(Box::new(Ty::Tuple(seq_tys.clone()))));
            let (success_tag, err_tag) = self.match_result_variant_tags(span)?;
            (input_ty, seq_tys, success_tag, err_tag)
        } else {
            // Input and prearguments share one instantiation. Resolve their
            // constraints before choosing UnitOnly, tuple, or single-value shape.
            self.extractor_contract(extractor_id, &extractor_ty, span)?
        };
        Ok((
            self.resolve_ty(&input_ty),
            self.resolve_ty(&extractor_ty),
            typed_pre_args,
            seq_tys,
            success_tag,
            err_tag,
        ))
    }

    pub(in crate::checker) fn require_extractor_match_result_payload_ty(
        &self,
        ty: &Ty,
        span: &Span,
        context: &str,
    ) -> Result<Vec<Ty>, TypeError> {
        match self.resolve_ty(ty) {
            Ty::MatchResult(payload) => match payload.as_ref() {
                Ty::Tuple(items) => Ok(items.clone()),
                other => Ok(vec![other.clone()]),
            },
            other => Err(TypeError {
                structured: None,
                message: format!(
                    "{} must return MatchResult<T>, got {}",
                    context,
                    self.ty_name(&other)
                ),
                span: span.clone(),
                hint: None,
            }),
        }
    }
}
