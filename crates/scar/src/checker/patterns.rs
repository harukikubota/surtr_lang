use super::*;
use diagnostics::{PatternKind, TypeDiagnosticReason};

impl Checker {
    pub(super) fn projection_shape_error(
        &self,
        expected: &str,
        actual: impl Into<String>,
        span: &Span,
    ) -> TypeError {
        TypeError::from_structured(diagnostics::StructuredDiagnostic {
            reason: TypeDiagnosticReason::PatternShapeMismatch.into(),
            origin: diagnostics::DiagnosticOrigin::Pattern,
            data: diagnostics::DiagnosticData::Pattern(diagnostics::PatternDiagnosticData {
                pattern_kind: PatternKind::Other,
                name: Some("Projection".into()),
                expected_type: Some(expected.into()),
                actual_type: Some(actual.into()),
                expected_count: None,
                actual_count: None,
                details: Vec::new(),
            }),
            primary: diagnostics::SourceFact::untyped(
                diagnostics::SourceRole::Pattern,
                diagnostics::SourceId(0),
                span.clone(),
            ),
            related: Vec::new(),
            remediation: None,
        })
    }

    /// Resolve only Pattern argument roles. Preargument expressions remain expressions,
    /// including any independent Pattern consumer calls they contain.
    fn select_application_pattern(
        &mut self,
        pattern: &ResolvedPattern,
    ) -> Result<ResolvedPattern, TypeError> {
        Ok(match pattern {
            ResolvedPattern::ExtractorApplication { head, args } => {
                let selected = self.select_extractor_application(head, args)?;
                return self.select_application_pattern(&selected);
            }
            ResolvedPattern::Deferred {
                pattern,
                bindings,
                allow_bindings,
            } => ResolvedPattern::Deferred {
                pattern: Box::new(self.select_application_pattern(pattern)?),
                bindings: bindings.clone(),
                allow_bindings: *allow_bindings,
            },
            ResolvedPattern::Projection {
                index,
                id,
                inner,
                annotation,
            } => ResolvedPattern::Projection {
                index: *index,
                id: id.clone(),
                inner: Box::new(self.select_application_pattern(inner)?),
                annotation: annotation.clone(),
            },
            ResolvedPattern::As(inner, id, annotation) => ResolvedPattern::As(
                Box::new(self.select_application_pattern(inner)?),
                id.clone(),
                annotation.clone(),
            ),
            ResolvedPattern::ListCons(head, tail) => ResolvedPattern::ListCons(
                Box::new(self.select_application_pattern(head)?),
                Box::new(self.select_application_pattern(tail)?),
            ),
            ResolvedPattern::Tuple(items) => ResolvedPattern::Tuple(
                items
                    .iter()
                    .map(|item| self.select_application_pattern(item))
                    .collect::<Result<_, _>>()?,
            ),
            ResolvedPattern::Constructor(id, items) => ResolvedPattern::Constructor(
                id.clone(),
                items
                    .iter()
                    .map(|item| self.select_application_pattern(item))
                    .collect::<Result<_, _>>()?,
            ),
            ResolvedPattern::Extractor(id, args, items) => ResolvedPattern::Extractor(
                id.clone(),
                args.clone(),
                items
                    .iter()
                    .map(|item| self.select_application_pattern(item))
                    .collect::<Result<_, _>>()?,
            ),
            ResolvedPattern::Or(_) => {
                return Err(self.projection_shape_error(
                    "a Pattern without OR in apply_pattern",
                    "OR Pattern",
                    &Self::resolved_pattern_span(pattern),
                ))
            }
            other => other.clone(),
        })
    }

    fn lower_pattern_projections(
        &self,
        pattern: ResolvedPattern,
        slots: &mut std::collections::BTreeMap<u8, ResolvedId>,
    ) -> Result<ResolvedPattern, TypeError> {
        Ok(match pattern {
            ResolvedPattern::Projection {
                index,
                id,
                inner,
                annotation,
            } => {
                if !(1..=16).contains(&index) {
                    return Err(self.projection_shape_error(
                        "an index between _1 and _16",
                        format!("_{}", index),
                        &id.span,
                    ));
                }
                if slots.insert(index, id.clone()).is_some() {
                    return Err(self.projection_shape_error(
                        "unique projection slots",
                        format!("duplicate projection slot _{}", index),
                        &id.span,
                    ));
                }
                ResolvedPattern::As(
                    Box::new(self.lower_pattern_projections(*inner, slots)?),
                    id,
                    annotation,
                )
            }
            ResolvedPattern::Deferred {
                pattern,
                bindings,
                allow_bindings,
            } => ResolvedPattern::Deferred {
                pattern: Box::new(self.lower_pattern_projections(*pattern, slots)?),
                bindings,
                allow_bindings,
            },
            ResolvedPattern::As(inner, id, annotation) => ResolvedPattern::As(
                Box::new(self.lower_pattern_projections(*inner, slots)?),
                id,
                annotation,
            ),
            ResolvedPattern::ListCons(head, tail) => ResolvedPattern::ListCons(
                Box::new(self.lower_pattern_projections(*head, slots)?),
                Box::new(self.lower_pattern_projections(*tail, slots)?),
            ),
            ResolvedPattern::Tuple(items) => ResolvedPattern::Tuple(
                items
                    .into_iter()
                    .map(|item| self.lower_pattern_projections(item, slots))
                    .collect::<Result<_, _>>()?,
            ),
            ResolvedPattern::Constructor(id, items) => ResolvedPattern::Constructor(
                id,
                items
                    .into_iter()
                    .map(|item| self.lower_pattern_projections(item, slots))
                    .collect::<Result<_, _>>()?,
            ),
            ResolvedPattern::Extractor(id, args, items) => ResolvedPattern::Extractor(
                id,
                args,
                items
                    .into_iter()
                    .map(|item| self.lower_pattern_projections(item, slots))
                    .collect::<Result<_, _>>()?,
            ),
            other => other,
        })
    }

    fn apply_pattern_input_hint(
        &mut self,
        pattern: &ResolvedPattern,
        projection_types: &HashMap<u32, Ty>,
    ) -> Option<Ty> {
        match pattern {
            ResolvedPattern::Deferred { pattern, .. } => {
                self.apply_pattern_input_hint(pattern, projection_types)
            }
            ResolvedPattern::As(inner, id, annotation) => {
                if let Some(ty) = annotation {
                    self.resolve_ast_ty_in_context(ty, self.local_type_syntax_context())
                        .ok()
                } else {
                    projection_types
                        .get(&id.unique_id)
                        .cloned()
                        .or_else(|| self.apply_pattern_input_hint(inner, projection_types))
                }
            }
            ResolvedPattern::Tuple(items) => Some(Ty::Tuple(
                items
                    .iter()
                    .map(|item| {
                        self.apply_pattern_input_hint(item, projection_types)
                            .unwrap_or_else(|| self.env.fresh_tyvar())
                    })
                    .collect(),
            )),
            ResolvedPattern::Constructor(id, items)
                if matches!(id.name.as_str(), "Ok" | "Result::Ok") && items.len() == 1 =>
            {
                Some(Ty::Result(
                    Box::new(
                        self.apply_pattern_input_hint(&items[0], projection_types)
                            .unwrap_or_else(|| self.env.fresh_tyvar()),
                    ),
                    Box::new(Ty::Error),
                ))
            }
            _ => self.infer_match_pattern_ty(pattern),
        }
    }

    pub(super) fn check_apply_pattern(
        &mut self,
        span: &Span,
        value: &Resolved,
        pattern: &ResolvedPattern,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let selected = self.select_application_pattern(pattern)?;
        let mut slots = std::collections::BTreeMap::new();
        let selected = self.lower_pattern_projections(selected, &mut slots)?;
        for (offset, (index, id)) in slots.iter().enumerate() {
            if usize::from(*index) != offset + 1 {
                return Err(self.projection_shape_error(
                    "projection slots contiguous from _1",
                    format!("_{} before missing _{}", index, offset + 1),
                    &id.span,
                ));
            }
        }
        self.env.push_var_scope();
        let saved_aliases = self.pattern_binding_aliases.clone();
        let result = (|| {
            let mut projection_types = HashMap::new();
            if let Some(Ty::Result(payload, _)) = expected.map(|ty| self.resolve_ty(ty)) {
                let expected_slots = if slots.len() == 1 {
                    vec![*payload]
                } else if let Ty::Tuple(items) = *payload {
                    items
                } else {
                    Vec::new()
                };
                if expected_slots.len() == slots.len() {
                    for (id, ty) in slots.values().zip(expected_slots) {
                        projection_types.insert(id.unique_id, ty);
                    }
                }
            }
            let input_hint = self.apply_pattern_input_hint(&selected, &projection_types);
            let value = self.check_node_with_expected(value, input_hint.as_ref())?;
            self.ensure_no_runtime_facet_args(std::slice::from_ref(&value), span, "apply_pattern")?;
            let (pattern, _) = self.check_pattern(&selected, &value.ty, span)?;
            self.bind_typed_pattern(&pattern, &value.ty);
            let projections = slots
                .into_values()
                .map(|id| {
                    let ty = self.env.lookup_var(id.unique_id).cloned().ok_or_else(|| {
                        self.typecheck_invariant_error("Projection binding has no type", &id.span)
                    })?;
                    Ok((id, self.resolve_ty(&ty)))
                })
                .collect::<Result<Vec<_>, TypeError>>()?;
            let payload = match projections.as_slice() {
                [] => Ty::Unit,
                [(_, ty)] => ty.clone(),
                _ => Ty::Tuple(projections.iter().map(|(_, ty)| ty.clone()).collect()),
            };
            let ty = Ty::Result(Box::new(payload), Box::new(Ty::Error));
            if let Some(expected) = expected {
                self.assert_type_relation(
                    expected,
                    &ty,
                    self.type_fact(diagnostics::SourceRole::Expected, span, expected),
                    self.type_fact(diagnostics::SourceRole::Value, span, &ty),
                    TypeDiagnosticReason::AnnotationTypeMismatch,
                    diagnostics::DiagnosticOrigin::Annotation,
                    "apply_pattern",
                    0,
                )?;
            }
            Ok(TypedNode {
                ty: self.resolve_ty(&ty),
                span: span.clone(),
                node: TypedInner::ApplyPattern {
                    value: Box::new(value),
                    pattern,
                    projections,
                },
            })
        })();
        self.env.pop_var_scope();
        self.pattern_binding_aliases = saved_aliases;
        result
    }

    pub(super) fn deferred_pattern_error(
        message: impl Into<String>,
        id: &ResolvedId,
        reason: diagnostics::ResolveDiagnosticReason,
    ) -> TypeError {
        let spec = diagnostics::resolve_error_spec(
            diagnostics::SourceId(0),
            message,
            id.span.clone(),
            reason,
            Some(id.name.clone()),
            &[],
        );
        TypeError {
            message: spec.message,
            span: id.span.clone(),
            hint: None,
            structured: spec.structured,
        }
    }

    pub(super) fn selected_candidate_error(error: &sigil::error::ResolveError) -> TypeError {
        use diagnostics::ResolveDiagnosticReason as D;
        use sigil::error::ResolveErrorReason as R;
        let reason = match &error.diagnostic.reason {
            R::DeferredParse(parse) => {
                let spec = diagnostics::parse_error_spec(diagnostics::SourceId(0), "", parse);
                return TypeError {
                    message: spec.message,
                    span: error.span.clone(),
                    hint: None,
                    structured: spec.structured,
                };
            }
            R::NameResolution => D::NameResolution,
            R::Namespace => D::Namespace,
            R::Visibility => D::Visibility,
            R::Import => D::Import,
            R::Capture => D::Capture,
            R::Pattern => D::Pattern,
            R::Declaration => D::Declaration,
            R::SpecialForm => D::SpecialForm,
            R::SourcePolicy => D::SourcePolicy,
            R::InvalidIntrinsicSurfaceContract => D::InvalidIntrinsicSurfaceContract,
            R::ReservedIntrinsicMarkerDeclaration => D::ReservedIntrinsicMarkerDeclaration,
            R::ReservedIntrinsicMarkerImpl => D::ReservedIntrinsicMarkerImpl,
            R::CompilerInvariant => D::CompilerInvariant,
        };
        let labels = error
            .related_labels
            .iter()
            .map(|label| {
                (
                    diagnostics::SourceId(0),
                    label.span.clone(),
                    label.message.clone(),
                )
            })
            .collect::<Vec<_>>();
        let spec = diagnostics::resolve_error_spec(
            diagnostics::SourceId(0),
            &error.message,
            error.span.clone(),
            reason,
            error.diagnostic.subject.clone(),
            &labels,
        );
        TypeError {
            message: spec.message,
            span: error.span.clone(),
            hint: None,
            structured: spec.structured,
        }
    }

    pub(super) fn canonical_pattern_id(&self, id: &ResolvedId) -> Result<ResolvedId, TypeError> {
        let mut resolved = id.clone();
        while let Some(outer) = self.pattern_binding_aliases.get(&resolved.unique_id) {
            resolved = outer.clone().ok_or_else(|| {
                Self::deferred_pattern_error(
                    format!("Undefined variable: {}", id.name),
                    id,
                    diagnostics::ResolveDiagnosticReason::NameResolution,
                )
            })?;
        }
        resolved.span = id.span.clone();
        Ok(resolved)
    }

    pub(super) fn select_extractor_application(
        &mut self,
        head: &ResolvedId,
        args: &[ResolvedPatternArgument],
    ) -> Result<ResolvedPattern, TypeError> {
        let head = self.canonical_pattern_id(head)?;
        let ty = self
            .env
            .lookup_var(head.unique_id)
            .cloned()
            .map(|ty| self.resolve_ty(&ty))
            .ok_or_else(|| {
                Self::deferred_pattern_error(
                    format!("Undefined variable: {}", head.name),
                    &head,
                    diagnostics::ResolveDiagnosticReason::NameResolution,
                )
            })?;
        let Ty::ExtractorClosure(signature) = ty else {
            return Err(TypeError::new(
                format!("Pattern head {} must have ExtractorClosure type", head.name),
                head.span.clone(),
            ));
        };
        let Ty::Func(params, _) = signature.as_ref() else {
            return Err(TypeError::new(
                "ExtractorClosure signature is unresolved",
                head.span.clone(),
            ));
        };
        let pre_arity = params.len().checked_sub(1).ok_or_else(|| {
            TypeError::new(
                "ExtractorClosure requires at least one input",
                head.span.clone(),
            )
        })?;
        if args.len() < pre_arity {
            return Err(TypeError::new(
                format!(
                    "Extractor {} requires {} pre-argument(s), got {}",
                    head.name,
                    pre_arity,
                    args.len()
                ),
                head.span.clone(),
            ));
        }
        let pre_args = args[..pre_arity]
            .iter()
            .map(|arg| {
                arg.expr
                    .as_ref()
                    .map(|expr| *expr.clone())
                    .map_err(Self::selected_candidate_error)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let items = args[pre_arity..]
            .iter()
            .map(|arg| {
                arg.pattern
                    .as_ref()
                    .map(|pat| *pat.clone())
                    .map_err(Self::selected_candidate_error)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let selected = ResolvedPattern::Extractor(head, pre_args, items);
        // Constructor-trait position checks run only for the chosen syntax roles.
        let constructor_traits = self.contextual_constructor_trait_names(&[]);
        self.validate_constructor_pattern(&selected, &constructor_traits)?;
        Ok(selected)
    }

    pub(super) fn finalize_pattern_bindings(
        &mut self,
        bindings: &[ResolvedPatternBinding],
        selected: &HashSet<u32>,
        allow_bindings: bool,
    ) -> Result<(), TypeError> {
        for binding in bindings {
            if selected.contains(&binding.proxy.unique_id) {
                if !allow_bindings {
                    return Err(Self::deferred_pattern_error(
                        "is_match pattern cannot bind variables",
                        &binding.proxy,
                        diagnostics::ResolveDiagnosticReason::SpecialForm,
                    ));
                }
            } else {
                self.pattern_binding_aliases
                    .insert(binding.proxy.unique_id, binding.outer.clone());
            }
        }
        Ok(())
    }

    fn insert_pattern_binding(out: &mut HashSet<u32>, id: &ResolvedId) -> Result<(), TypeError> {
        if out.insert(id.unique_id) {
            Ok(())
        } else {
            Err(Self::deferred_pattern_error(
                format!("Duplicate pattern binding: {}", id.name),
                id,
                diagnostics::ResolveDiagnosticReason::Pattern,
            ))
        }
    }

    pub(super) fn typed_pattern_bindings(
        pattern: &TypedPattern,
        out: &mut HashSet<u32>,
    ) -> Result<(), TypeError> {
        match pattern {
            TypedPattern::Var(_, id) => Self::insert_pattern_binding(out, id)?,
            TypedPattern::As(_, inner, id) => {
                Self::typed_pattern_bindings(inner, out)?;
                Self::insert_pattern_binding(out, id)?;
            }
            TypedPattern::Tuple(_, items)
            | TypedPattern::Constructor { fields: items, .. }
            | TypedPattern::Extractor { items, .. } => {
                for item in items {
                    Self::typed_pattern_bindings(item, out)?;
                }
            }
            TypedPattern::ListCons(_, head, tail) => {
                Self::typed_pattern_bindings(head, out)?;
                Self::typed_pattern_bindings(tail, out)?;
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn typed_match_pattern_bindings(
        pattern: &TypedMatchPattern,
        out: &mut HashSet<u32>,
    ) -> Result<(), TypeError> {
        match pattern {
            TypedMatchPattern::Binding(id) => Self::insert_pattern_binding(out, id)?,
            TypedMatchPattern::As(inner, id) => {
                Self::typed_match_pattern_bindings(inner, out)?;
                Self::insert_pattern_binding(out, id)?;
            }
            TypedMatchPattern::Tuple(items)
            | TypedMatchPattern::Constructor { fields: items, .. }
            | TypedMatchPattern::Extractor { items, .. } => {
                for item in items {
                    Self::typed_match_pattern_bindings(item, out)?;
                }
            }
            TypedMatchPattern::ListCons(head, tail) => {
                Self::typed_match_pattern_bindings(head, out)?;
                Self::typed_match_pattern_bindings(tail, out)?;
            }
            TypedMatchPattern::Or(items) => {
                // Each alternative has already been checked for identical bindings.
                if let Some(first) = items.first() {
                    Self::typed_match_pattern_bindings(first, out)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Expression subtrees owned by patterns, including nested occurrences.
    /// Expression visitors must include these as well as ordinary Expr children.
    pub(super) fn pattern_expression_nodes(node: &TypedNode) -> Vec<&TypedNode> {
        fn binding<'a>(pat: &'a TypedPattern, out: &mut Vec<&'a TypedNode>) {
            match pat {
                TypedPattern::Extractor {
                    pre_args, items, ..
                } => {
                    out.extend(pre_args);
                    for item in items {
                        binding(item, out);
                    }
                }
                TypedPattern::As(_, inner, _) => binding(inner, out),
                TypedPattern::ListCons(_, head, tail) => {
                    binding(head, out);
                    binding(tail, out);
                }
                TypedPattern::Tuple(_, items) | TypedPattern::Constructor { fields: items, .. } => {
                    for item in items {
                        binding(item, out);
                    }
                }
                _ => {}
            }
        }
        fn matching<'a>(pat: &'a TypedMatchPattern, out: &mut Vec<&'a TypedNode>) {
            match pat {
                TypedMatchPattern::Extractor {
                    pre_args, items, ..
                } => {
                    out.extend(pre_args);
                    for item in items {
                        matching(item, out);
                    }
                }
                TypedMatchPattern::As(inner, _) => matching(inner, out),
                TypedMatchPattern::ListCons(head, tail) => {
                    matching(head, out);
                    matching(tail, out);
                }
                TypedMatchPattern::Tuple(items)
                | TypedMatchPattern::Or(items)
                | TypedMatchPattern::Constructor { fields: items, .. } => {
                    for item in items {
                        matching(item, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        match &node.node {
            TypedInner::Bind(pattern, _)
            | TypedInner::SafeBind(pattern, ..)
            | TypedInner::ApplyPattern { pattern, .. } => binding(pattern, &mut out),
            TypedInner::DoSafeBind(control) => binding(&control.pattern, &mut out),
            TypedInner::Match(_, arms) => {
                for arm in arms {
                    matching(&arm.pattern, &mut out);
                }
            }
            _ => {}
        }
        out
    }

    /// Expression subtrees owned by patterns, including nested occurrences.
    /// Expression visitors must include these as well as ordinary Expr children.
    pub(super) fn pattern_expression_nodes_mut(node: &mut TypedNode) -> Vec<&mut TypedNode> {
        fn binding<'a>(pat: &'a mut TypedPattern, out: &mut Vec<&'a mut TypedNode>) {
            match pat {
                TypedPattern::Extractor {
                    pre_args, items, ..
                } => {
                    out.extend(pre_args);
                    for item in items {
                        binding(item, out);
                    }
                }
                TypedPattern::As(_, inner, _) => binding(inner, out),
                TypedPattern::ListCons(_, head, tail) => {
                    binding(head, out);
                    binding(tail, out);
                }
                TypedPattern::Tuple(_, items) | TypedPattern::Constructor { fields: items, .. } => {
                    for item in items {
                        binding(item, out);
                    }
                }
                _ => {}
            }
        }
        fn matching<'a>(pat: &'a mut TypedMatchPattern, out: &mut Vec<&'a mut TypedNode>) {
            match pat {
                TypedMatchPattern::Extractor {
                    pre_args, items, ..
                } => {
                    out.extend(pre_args);
                    for item in items {
                        matching(item, out);
                    }
                }
                TypedMatchPattern::As(inner, _) => matching(inner, out),
                TypedMatchPattern::ListCons(head, tail) => {
                    matching(head, out);
                    matching(tail, out);
                }
                TypedMatchPattern::Tuple(items)
                | TypedMatchPattern::Or(items)
                | TypedMatchPattern::Constructor { fields: items, .. } => {
                    for item in items {
                        matching(item, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        match &mut node.node {
            TypedInner::Bind(pattern, _)
            | TypedInner::SafeBind(pattern, ..)
            | TypedInner::ApplyPattern { pattern, .. } => binding(pattern, &mut out),
            TypedInner::DoSafeBind(control) => binding(&mut control.pattern, &mut out),
            TypedInner::Match(_, arms) => {
                for arm in arms {
                    matching(&mut arm.pattern, &mut out);
                }
            }
            _ => {}
        }
        out
    }

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
            ResolvedPattern::Deferred { pattern, .. } => Self::is_total_bind_pattern(pattern),
            ResolvedPattern::ExtractorApplication { .. } | ResolvedPattern::Projection { .. } => {
                false
            }
            ResolvedPattern::Var(_)
            | ResolvedPattern::Annotated(_, _)
            | ResolvedPattern::AnnotatedWildcard(_, _)
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
            | ResolvedPattern::Extractor(_, _, _) => false,
        }
    }

    pub(super) fn check_pattern(
        &mut self,
        pat: &ResolvedPattern,
        rhs_ty: &Ty,
        span: &Span,
    ) -> Result<(TypedPattern, Ty), TypeError> {
        self.ensure_no_match_result_value(rhs_ty, span)?;
        if let ResolvedPattern::Pin(id) = pat {
            let canonical = self.canonical_pattern_id(id)?;
            if canonical.unique_id != id.unique_id {
                return self.check_pattern(&ResolvedPattern::Pin(canonical), rhs_ty, span);
            }
        }
        match pat {
            ResolvedPattern::Projection { id, .. } => Err(self.projection_shape_error(
                "the apply_pattern consumer",
                "a projection outside apply_pattern",
                &id.span,
            )),
            ResolvedPattern::Deferred {
                pattern,
                bindings,
                allow_bindings,
            } => {
                let result = self.check_pattern(pattern, rhs_ty, span)?;
                let mut selected = HashSet::new();
                Self::typed_pattern_bindings(&result.0, &mut selected)?;
                self.finalize_pattern_bindings(bindings, &selected, *allow_bindings)?;
                Ok(result)
            }
            ResolvedPattern::ExtractorApplication { head, args } => {
                let selected = self.select_extractor_application(head, args)?;
                self.check_pattern(&selected, rhs_ty, span)
            }

            ResolvedPattern::Var(id) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                Ok((TypedPattern::Var(rhs_ty.clone(), id.clone()), rhs_ty))
            }
            ResolvedPattern::AnnotatedWildcard(pattern_span, ast_ty) => {
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
                    self.type_fact(diagnostics::SourceRole::Value, pattern_span, rhs_ty),
                    TypeDiagnosticReason::AnnotationTypeMismatch,
                    diagnostics::DiagnosticOrigin::Annotation,
                    "_",
                    0,
                )?;
                let expected = self.resolve_ty(&expected);
                Ok((TypedPattern::Wildcard(expected.clone()), expected))
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
                        let (input_ty, extractor_ty, pre_args, seq_tys, success_tag, err_tag) =
                            self.extractor_contract_for_observed_ty(
                                &extractor_id,
                                &rhs_ty,
                                &[],
                                span,
                            )?;
                        debug_assert_eq!(seq_tys.len(), 2);
                        let (typed_head, _) = self.check_pattern(head, &seq_tys[0], span)?;
                        let (typed_tail, _) = self.check_pattern(tail, &seq_tys[1], span)?;
                        Ok((
                            TypedPattern::Extractor {
                                input_ty,
                                extractor: extractor_id,
                                extractor_ty,
                                pre_args,
                                success_tag,
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
            ResolvedPattern::Extractor(extractor_id, pre_args, items) => {
                let rhs_ty = self.resolve_ty(rhs_ty);
                let (input_ty, extractor_ty, pre_args, seq_tys, success_tag, err_tag) = self
                    .extractor_contract_for_observed_ty(
                        extractor_id,
                        &rhs_ty,
                        pre_args,
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
                if items.len() != seq_tys.len() && !(items.is_empty() && seq_tys == [Ty::Unit]) {
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
                        input_ty,
                        extractor: extractor_id.clone(),
                        extractor_ty,
                        pre_args,
                        success_tag,
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
