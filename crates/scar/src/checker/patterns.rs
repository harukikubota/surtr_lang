use super::*;
use diagnostics::{PatternKind, TypeDiagnosticReason};

impl Checker {
    pub(super) fn eq_dispatch_for_pattern_pin(
        &mut self,
        ty: &Ty,
        span: &Span,
    ) -> Result<TraitDispatch, TypeError> {
        let receiver_ty = self.resolve_ty(ty);
        let eq_trait = self.trait_key_by_short_name("Eq").ok_or_else(|| {
            self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("Eq trait contract".into()),
                None,
                None,
                None,
                None,
                span,
                None,
            )
        })?;
        let trait_info = self.traits.get(&eq_trait).cloned().ok_or_else(|| {
            self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("Eq trait contract".into()),
                None,
                None,
                None,
                None,
                span,
                None,
            )
        })?;
        let method = trait_info.methods.get("eq").ok_or_else(|| {
            self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("Eq::eq method contract".into()),
                None,
                None,
                None,
                None,
                span,
                None,
            )
        })?;
        let (_, result_ty, trait_args, _, _) =
            self.resolve_trait_method_signature(&trait_info, method, &receiver_ty)?;
        self.consume_matching_capability(&receiver_ty, &eq_trait);
        let dispatch = match self.select_trait_method_instantiation(
            &eq_trait,
            "eq",
            &receiver_ty,
            &trait_args,
            &[receiver_ty.clone(), receiver_ty.clone()],
            &result_ty,
        )? {
            CandidateApplicability::Applicable(instantiation) => {
                Some(TraitDispatch::Selected(Box::new(instantiation)))
            }
            CandidateApplicability::Deferred(_) => {
                self.trait_dispatch_target_for_args(&eq_trait, "eq", &receiver_ty, &trait_args)?
            }
            CandidateApplicability::Rejected(_) => None,
        };
        dispatch.ok_or_else(|| {
            self.trait_obligation_failure(
                TypeDiagnosticReason::MissingTraitCapability,
                &eq_trait,
                &[],
                None,
                &receiver_ty,
                span,
                diagnostics::DiagnosticOrigin::Pattern,
            )
        })
    }

    pub(super) fn is_total_bind_pattern(pat: &ResolvedPattern) -> bool {
        match pat {
            ResolvedPattern::Var(_)
            | ResolvedPattern::Annotated(_, _)
            | ResolvedPattern::Wildcard(_) => true,
            ResolvedPattern::As(inner, _, _) => Self::is_total_bind_pattern(inner),
            ResolvedPattern::Tuple(items) => items.iter().all(Self::is_total_bind_pattern),
            ResolvedPattern::Or(_) => false,
            ResolvedPattern::Pin(_)
            | ResolvedPattern::ListNil(_)
            | ResolvedPattern::ListCons(_, _)
            | ResolvedPattern::IntLit(_, _)
            | ResolvedPattern::StrLit(_, _)
            | ResolvedPattern::BoolLit(_, _)
            | ResolvedPattern::DurationLit(_, _)
            | ResolvedPattern::Constructor(_, _)
            | ResolvedPattern::Extractor(_, _) => false,
        }
    }

    pub(super) fn check_pattern(
        &mut self,
        pat: &ResolvedPattern,
        rhs_ty: &Ty,
        span: &Span,
    ) -> Result<(TypedPattern, Ty), TypeError> {
        match pat {
            ResolvedPattern::Var(id) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                Ok((TypedPattern::Var(rhs_ty.clone(), id.clone()), rhs_ty))
            }
            ResolvedPattern::Annotated(id, ast_ty) => {
                let expected =
                    self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())?;
                self.assert_type_relation(
                    &expected,
                    rhs_ty,
                    self.type_fact(
                        diagnostics::SourceRole::Annotation,
                        Self::ast_ty_span(ast_ty),
                        &expected,
                    ),
                    self.type_fact(diagnostics::SourceRole::Value, span, rhs_ty),
                    diagnostics::TypeDiagnosticReason::AnnotationTypeMismatch,
                    diagnostics::DiagnosticOrigin::Annotation,
                    &id.name,
                    0,
                )?;
                let expected = self.resolve_ty(&expected);
                Ok((TypedPattern::Var(expected.clone(), id.clone()), expected))
            }
            ResolvedPattern::Pin(id) => {
                let pinned_ty = self.env.lookup_var(id.unique_id).cloned().ok_or_else(|| {
                    self.policy_error(
                        TypeDiagnosticReason::TypecheckInvariantViolation,
                        diagnostics::TypePolicy::ProducerContract,
                        Some("resolved pinned pattern binding".into()),
                        None,
                        None,
                        None,
                        None,
                        &id.span,
                        None,
                    )
                })?;
                let rhs_ty = self.resolve_ty(rhs_ty);
                let pinned_ty = self.resolve_ty(&pinned_ty);
                if !self.types_compatible(&pinned_ty, &rhs_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Pin,
                        Some(id.name.clone()),
                        Some(self.diagnostic_ty_name(&pinned_ty)),
                        Some(&rhs_ty),
                        None,
                        None,
                        Vec::new(),
                        &id.span,
                    ));
                }
                let dispatch = self.eq_dispatch_for_pattern_pin(&rhs_ty, &id.span)?;
                Ok((
                    TypedPattern::Pin(rhs_ty.clone(), id.clone(), dispatch),
                    rhs_ty,
                ))
            }
            ResolvedPattern::As(inner, alias, alias_ty) => {
                let (typed_inner, inner_ty) = self.check_pattern(inner, rhs_ty, span)?;
                let alias_bind_ty = if let Some(ast_ty) = alias_ty {
                    let expected =
                        self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())?;
                    if !self.types_compatible(&expected, &inner_ty) {
                        return Err(self.pattern_error(
                            TypeDiagnosticReason::PatternTypeMismatch,
                            PatternKind::Other,
                            Some("as-pattern alias".into()),
                            Some(self.diagnostic_ty_name(&expected)),
                            Some(&inner_ty),
                            None,
                            None,
                            Vec::new(),
                            &alias.span,
                        ));
                    }
                    self.resolve_ty(&expected)
                } else {
                    self.resolve_ty(&inner_ty)
                };

                Ok((
                    TypedPattern::As(alias_bind_ty, Box::new(typed_inner), alias.clone()),
                    inner_ty,
                ))
            }
            ResolvedPattern::Wildcard(_) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                Ok((TypedPattern::Wildcard(rhs_ty.clone()), rhs_ty))
            }
            ResolvedPattern::Tuple(items) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                let Ty::Tuple(item_tys) = &rhs_ty else {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternShapeMismatch,
                        PatternKind::Tuple,
                        Some("tuple".into()),
                        Some("tuple".into()),
                        Some(&rhs_ty),
                        None,
                        None,
                        Vec::new(),
                        span,
                    ));
                };
                if items.len() != item_tys.len() {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternArityMismatch,
                        PatternKind::Tuple,
                        Some("tuple".into()),
                        None,
                        Some(&rhs_ty),
                        Some(item_tys.len()),
                        Some(items.len()),
                        Vec::new(),
                        span,
                    ));
                }
                let mut typed_items = Vec::with_capacity(items.len());
                for (item, item_ty) in items.iter().zip(item_tys.iter()) {
                    let (typed_item, _) = self.check_pattern(item, item_ty, span)?;
                    typed_items.push(typed_item);
                }
                Ok((TypedPattern::Tuple(rhs_ty.clone(), typed_items), rhs_ty))
            }
            ResolvedPattern::Or(_) => Err(self.pattern_error(
                TypeDiagnosticReason::PatternShapeMismatch,
                PatternKind::Other,
                Some("pattern alternatives".into()),
                Some("match expression".into()),
                Some(rhs_ty),
                None,
                None,
                vec!["not supported in binding patterns".into()],
                span,
            )),
            ResolvedPattern::ListNil(pspan) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                match rhs_ty {
                    Ty::List(_) => Ok((TypedPattern::ListNil(rhs_ty.clone()), rhs_ty)),
                    Ty::Str => Ok((TypedPattern::StrLit(Ty::Str, String::new()), rhs_ty)),
                    other => Err(self.pattern_error(
                        TypeDiagnosticReason::PatternShapeMismatch,
                        PatternKind::List,
                        Some("empty list".into()),
                        Some("List<...> or String".into()),
                        Some(&other),
                        None,
                        None,
                        Vec::new(),
                        pspan,
                    )),
                }
            }
            ResolvedPattern::ListCons(head, tail) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                match &rhs_ty {
                    Ty::List(inner) => {
                        let elem_ty = inner.as_ref().clone();
                        let (typed_head, _) = self.check_pattern(head, &elem_ty, span)?;
                        let tail_ty = Ty::List(Box::new(elem_ty.clone()));
                        let (typed_tail, _) = self.check_pattern(tail, &tail_ty, span)?;
                        Ok((
                            TypedPattern::ListCons(
                                rhs_ty.clone(),
                                Box::new(typed_head),
                                Box::new(typed_tail),
                            ),
                            rhs_ty,
                        ))
                    }
                    Ty::Str => {
                        let extractor_id = self.kernel_uncons_id(span)?;
                        let (input_ty, extractor_ty, seq_tys, success_tag, no_match_tag, err_tag) =
                            self.extractor_contract_for_observed_ty(&extractor_id, &rhs_ty, span)?;
                        debug_assert_eq!(seq_tys.len(), 2);
                        let (typed_head, _) = self.check_pattern(head, &seq_tys[0], span)?;
                        let (typed_tail, _) = self.check_pattern(tail, &seq_tys[1], span)?;
                        Ok((
                            TypedPattern::Extractor {
                                input_ty,
                                extractor: extractor_id,
                                extractor_ty,
                                success_tag,
                                no_match_tag,
                                err_tag,
                                seq_tys,
                                items: vec![typed_head, typed_tail],
                            },
                            rhs_ty,
                        ))
                    }
                    other => Err(self.pattern_error(
                        TypeDiagnosticReason::PatternShapeMismatch,
                        PatternKind::List,
                        Some("list".into()),
                        Some("List<...> or String".into()),
                        Some(other),
                        None,
                        None,
                        Vec::new(),
                        span,
                    )),
                }
            }
            ResolvedPattern::IntLit(pspan, n) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                if !self.types_compatible(&Ty::Int, &rhs_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Other,
                        Some("integer literal".into()),
                        Some("Int".into()),
                        Some(&rhs_ty),
                        None,
                        None,
                        Vec::new(),
                        pspan,
                    ));
                }
                Ok((TypedPattern::IntLit(Ty::Int, n.clone()), rhs_ty))
            }
            ResolvedPattern::StrLit(pspan, s) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                if !self.types_compatible(&Ty::Str, &rhs_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Other,
                        Some("string literal".into()),
                        Some("String".into()),
                        Some(&rhs_ty),
                        None,
                        None,
                        Vec::new(),
                        pspan,
                    ));
                }
                Ok((TypedPattern::StrLit(Ty::Str, s.clone()), rhs_ty))
            }
            ResolvedPattern::BoolLit(pspan, b) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                if !self.types_compatible(&Ty::Bool, &rhs_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Other,
                        Some("boolean literal".into()),
                        Some("Boolean".into()),
                        Some(&rhs_ty),
                        None,
                        None,
                        Vec::new(),
                        pspan,
                    ));
                }
                Ok((TypedPattern::BoolLit(Ty::Bool, *b), rhs_ty))
            }
            ResolvedPattern::DurationLit(pspan, n) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                if !Self::is_duration_ty(&rhs_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Other,
                        Some("duration literal".into()),
                        Some("Duration".into()),
                        Some(&rhs_ty),
                        None,
                        None,
                        Vec::new(),
                        pspan,
                    ));
                }
                Ok((TypedPattern::DurationLit(rhs_ty.clone(), n.clone()), rhs_ty))
            }
            ResolvedPattern::Constructor(ctor_id, inners) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                if matches!(rhs_ty, Ty::Bool) {
                    let variant = self
                        .lookup_enum_variant_by_constructor_id(ctor_id.unique_id)
                        .ok_or_else(|| {
                            self.typecheck_invariant_error(
                                "resolved enum pattern constructor",
                                &ctor_id.span,
                            )
                        })?
                        .clone();
                    let variant = self.instantiate_enum_variant(&variant);
                    if Self::surface_name(&variant.enum_name) != "Boolean" {
                        return Err(self.pattern_error(
                            TypeDiagnosticReason::PatternShapeMismatch,
                            PatternKind::Constructor,
                            Some(ctor_id.name.clone()),
                            Some("Boolean".into()),
                            Some(&rhs_ty),
                            None,
                            None,
                            Vec::new(),
                            &ctor_id.span,
                        ));
                    }
                    if !inners.is_empty() {
                        return Err(self.pattern_error(
                            TypeDiagnosticReason::PatternArityMismatch,
                            PatternKind::Constructor,
                            Some(ctor_id.name.clone()),
                            None,
                            Some(&rhs_ty),
                            Some(0),
                            Some(inners.len()),
                            Vec::new(),
                            &ctor_id.span,
                        ));
                    }
                    return match variant.short_name.as_str() {
                        "True" => Ok((TypedPattern::BoolLit(Ty::Bool, true), Ty::Bool)),
                        "False" => Ok((TypedPattern::BoolLit(Ty::Bool, false), Ty::Bool)),
                        _ => Err(self.typecheck_invariant_error(
                            "resolved Boolean constructor variant",
                            &ctor_id.span,
                        )),
                    };
                }
                if let Ty::Result(ok_ty, err_ty) = &rhs_ty {
                    let (tag, inner_ty) = match ctor_id.name.as_str() {
                        "Ok" => (0, ok_ty.as_ref().clone()),
                        "Err" => (1, err_ty.as_ref().clone()),
                        _ => {
                            return Err(self.typecheck_invariant_error(
                                "Result pattern constructor",
                                &ctor_id.span,
                            ));
                        }
                    };
                    if inners.len() != 1 {
                        return Err(self.pattern_error(
                            TypeDiagnosticReason::PatternArityMismatch,
                            PatternKind::Constructor,
                            Some(ctor_id.name.clone()),
                            None,
                            Some(&rhs_ty),
                            Some(1),
                            Some(inners.len()),
                            Vec::new(),
                            &ctor_id.span,
                        ));
                    }
                    let (typed_inner, _) = self.check_pattern(&inners[0], &inner_ty, span)?;
                    return Ok((
                        TypedPattern::Constructor {
                            ty: rhs_ty.clone(),
                            tag,
                            field_tys: vec![inner_ty],
                            fields: vec![typed_inner],
                            field_offset: 0,
                        },
                        rhs_ty,
                    ));
                }
                let Ty::Enum(expected_enum_name, _) = &rhs_ty else {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::ConstructorPatternRequiresEnumOrResultRhs,
                        PatternKind::Constructor,
                        Some(ctor_id.name.clone()),
                        Some("enum or Result".into()),
                        Some(&rhs_ty),
                        None,
                        None,
                        Vec::new(),
                        &ctor_id.span,
                    ));
                };
                let variant = self
                    .lookup_enum_variant_by_constructor_id(ctor_id.unique_id)
                    .ok_or_else(|| {
                        self.typecheck_invariant_error(
                            "resolved enum pattern constructor",
                            &ctor_id.span,
                        )
                    })?
                    .clone();
                let variant = self.instantiate_enum_variant(&variant);
                if &variant.enum_name != expected_enum_name {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternShapeMismatch,
                        PatternKind::Constructor,
                        Some(ctor_id.name.clone()),
                        Some(Self::surface_name(expected_enum_name).to_string()),
                        Some(&rhs_ty),
                        None,
                        None,
                        Vec::new(),
                        &ctor_id.span,
                    ));
                }
                if !self.types_compatible(&variant.enum_ty, &rhs_ty) {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternTypeMismatch,
                        PatternKind::Constructor,
                        Some(ctor_id.name.clone()),
                        Some(self.diagnostic_ty_name(&variant.enum_ty)),
                        Some(&rhs_ty),
                        None,
                        None,
                        Vec::new(),
                        &ctor_id.span,
                    ));
                }
                if inners.len() != variant.payload.len() {
                    return Err(self.pattern_error(
                        TypeDiagnosticReason::PatternArityMismatch,
                        PatternKind::Constructor,
                        Some(ctor_id.name.clone()),
                        None,
                        Some(&rhs_ty),
                        Some(variant.payload.len()),
                        Some(inners.len()),
                        Vec::new(),
                        &ctor_id.span,
                    ));
                }
                let mut typed_fields = Vec::with_capacity(inners.len());
                for (inner, field_ty) in inners.iter().zip(variant.payload.iter()) {
                    let field_ty = self.resolve_ty(field_ty);
                    let (typed_inner, _) = self.check_pattern(inner, &field_ty, span)?;
                    typed_fields.push(typed_inner);
                }
                Ok((
                    TypedPattern::Constructor {
                        ty: rhs_ty.clone(),
                        tag: variant.tag,
                        field_tys: variant.payload,
                        fields: typed_fields,
                        field_offset: 1,
                    },
                    rhs_ty,
                ))
            }
            ResolvedPattern::Extractor(extractor_id, items) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                let (input_ty, extractor_ty, seq_tys, success_tag, no_match_tag, err_tag) = self
                    .extractor_contract_for_observed_ty(
                        extractor_id,
                        &rhs_ty,
                        &extractor_id.span,
                    )?;
                if !self.types_compatible(&input_ty, &rhs_ty) {
                    let mut error = self.pattern_error(
                        TypeDiagnosticReason::ExtractorInputTypeMismatch,
                        PatternKind::Extractor,
                        Some(extractor_id.name.clone()),
                        Some(self.diagnostic_ty_name(&input_ty)),
                        Some(&rhs_ty),
                        None,
                        None,
                        vec![self
                            .callable_signature_for_ty(&extractor_ty)
                            .unwrap_or_else(|| self.diagnostic_ty_name(&extractor_ty))],
                        &extractor_id.span,
                    );
                    if let Some(signature) = self.callable_signature_for_ty(&extractor_ty) {
                        error = error.with_hint(format!(
                            "Extractor type signature: {}. RHS type is {}.",
                            signature,
                            self.diagnostic_ty_name(&rhs_ty)
                        ));
                    }
                    return Err(error);
                }
                if items.len() != seq_tys.len() {
                    let success_types = seq_tys
                        .iter()
                        .map(|ty| self.diagnostic_ty_name(ty))
                        .collect::<Vec<_>>();
                    return Err(self
                        .pattern_error(
                            TypeDiagnosticReason::ExtractorArityMismatch,
                            PatternKind::Extractor,
                            Some(extractor_id.name.clone()),
                            None,
                            Some(&rhs_ty),
                            Some(seq_tys.len()),
                            Some(items.len()),
                            success_types.clone(),
                            &extractor_id.span,
                        )
                        .with_hint(format!(
                            "Extractor success value(s): {}.",
                            success_types.join(", ")
                        )));
                }
                let mut typed_items = Vec::with_capacity(items.len());
                for (item, item_ty) in items.iter().zip(seq_tys.iter()) {
                    let (typed_item, _) = self.check_pattern(item, item_ty, span)?;
                    typed_items.push(typed_item);
                }
                Ok((
                    TypedPattern::Extractor {
                        input_ty: rhs_ty.clone(),
                        extractor: extractor_id.clone(),
                        extractor_ty,
                        success_tag,
                        no_match_tag,
                        err_tag,
                        seq_tys,
                        items: typed_items,
                    },
                    rhs_ty,
                ))
            }
        }
    }

    pub(super) fn bind_typed_pattern(&mut self, pat: &TypedPattern, rhs_ty: &Ty) {
        let rhs_ty = self.resolve_ty(rhs_ty);
        match pat {
            TypedPattern::Var(_, id) => {
                self.env.bind_var(id.unique_id, rhs_ty.clone());
                if Self::ty_is_error_observer_callable(&rhs_ty) {
                    self.error_observer_bindings.insert(id.unique_id);
                } else {
                    self.error_observer_bindings.remove(&id.unique_id);
                }
            }
            TypedPattern::As(alias_ty, inner, id) => {
                let alias_ty = self.resolve_ty(alias_ty);
                self.env.bind_var(id.unique_id, alias_ty.clone());
                if Self::ty_is_error_observer_callable(&alias_ty) {
                    self.error_observer_bindings.insert(id.unique_id);
                } else {
                    self.error_observer_bindings.remove(&id.unique_id);
                }
                self.bind_typed_pattern(inner, &rhs_ty);
            }
            TypedPattern::Pin(_, _, _) => {}
            TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => {}
            TypedPattern::Tuple(_, items) => {
                let item_tys = match &rhs_ty {
                    Ty::Tuple(item_tys) => item_tys.clone(),
                    _ => return,
                };
                for (item, item_ty) in items.iter().zip(item_tys.iter()) {
                    self.bind_typed_pattern(item, item_ty);
                }
            }
            TypedPattern::ListCons(_, head, tail) => {
                let elem_ty = match &rhs_ty {
                    Ty::List(inner) => inner.as_ref().clone(),
                    _ => return,
                };
                self.bind_typed_pattern(head, &elem_ty);
                let tail_ty = Ty::List(Box::new(elem_ty));
                self.bind_typed_pattern(tail, &tail_ty);
            }
            TypedPattern::Constructor {
                field_tys, fields, ..
            } => {
                for (field, field_ty) in fields.iter().zip(field_tys) {
                    self.bind_typed_pattern(field, field_ty);
                }
            }
            TypedPattern::Extractor { seq_tys, items, .. } => {
                for (item, item_ty) in items.iter().zip(seq_tys.iter()) {
                    self.bind_typed_pattern(item, item_ty);
                }
            }
        }
    }

    pub(super) fn normalize_env_bindings(&mut self) {
        let profile = self.profiler.start();
        let keys = self.env.vars.keys().copied().collect::<Vec<_>>();
        for key in keys {
            if let Some(ty) = self.env.vars.get(&key).cloned() {
                if matches!(
                    ty,
                    Ty::BuiltinFunc { .. } | Ty::UserFunc { .. } | Ty::Func(_, _)
                ) {
                    continue;
                }
                self.env.vars.insert(key, self.resolve_ty(&ty));
            }
        }
        self.profiler
            .finish(ProfileEvent::NormalizeEnvBindings, profile);
    }

    pub(super) fn collect_pattern_result_error_types(&self, pat: &TypedPattern, out: &mut Vec<Ty>) {
        match pat {
            TypedPattern::Constructor { fields, .. } => {
                for field in fields {
                    self.collect_pattern_result_error_types(field, out);
                }
            }
            TypedPattern::ListCons(_, head, tail) => {
                self.collect_pattern_result_error_types(head, out);
                self.collect_pattern_result_error_types(tail, out);
            }
            TypedPattern::Tuple(_, items) => {
                for item in items {
                    self.collect_pattern_result_error_types(item, out);
                }
            }
            TypedPattern::As(_, inner, _) => {
                self.collect_pattern_result_error_types(inner, out);
            }
            TypedPattern::Extractor { items, .. } => {
                out.push(Ty::Error);
                for item in items {
                    self.collect_pattern_result_error_types(item, out);
                }
            }
            TypedPattern::Var(_, _)
            | TypedPattern::Pin(_, _, _)
            | TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => {}
        }
    }
}
