use super::*;
use diagnostics::TypeDiagnosticReason;

impl Checker {
    pub(super) fn do_failure_carrier_is_rigid_variable(&self, carrier: &Ty) -> bool {
        match self.resolve_ty(carrier) {
            Ty::Var(variable) => self.rigid_tyvars.contains(&variable),
            Ty::SelfApp(items) => Self::constructor_application_parts(&items)
                .is_some_and(|(constructor, _)| {
                    matches!(constructor, Ty::Var(variable) if self.rigid_tyvars.contains(variable))
                }),
            _ => false,
        }
    }

    pub(super) fn resolve_monad_fail(
        &mut self,
        carrier: &Ty,
        span: &Span,
    ) -> Result<MonadFailResolution, TypeError> {
        let carrier = self.resolve_ty(carrier);
        let Some(key) = self
            .traits
            .values()
            .find(|info| info.compiler_owned_failure)
            .map(|info| self.trait_key(&info.id))
        else {
            return Ok(MonadFailResolution::InvalidMetadata(
                "canonical MonadFail declaration is missing",
            ));
        };
        match self.prove_trait_capability(&key, &carrier)? {
            ApplicabilityProof::Unsatisfied => return Ok(MonadFailResolution::Unavailable),
            ApplicabilityProof::Deferred(_) => {
                // The failure capability belongs to the constructor. Its mapped
                // payload may still be inferred from the remainder of a closure;
                // captured parameters and implementation constraints must already
                // be proved by the normal constructor projection contract.
                if !matches!(
                    self.constructor_head_projection(&key, &carrier)
                        .into_checked()
                        .map_err(|error| error.at_span(span))?,
                    ConstructorProjectionOutcome::Applicable { .. }
                ) {
                    return Ok(MonadFailResolution::Deferred);
                }
            }
            ApplicabilityProof::Satisfied(_) => {}
        }
        let error_id = ResolvedId {
            name: "__failure_error".into(),
            qualified_name: None,
            symbol_info: None,
            unique_id: Self::next_synthetic_range_uid(),
            compiler_generated: true,
            span: span.clone(),
        };
        self.env.push_var_scope();
        self.env.bind_var(error_id.unique_id, Ty::Error);
        let call = self.check_trait_invocation(
            span,
            &key,
            sindr::intrinsic::CanonicalTraitMethodIdentity::MonadFailFail.method_name(),
            &[ResolvedRecordLitArg::Positional(Resolved::Var(
                span.clone(),
                error_id.clone(),
            ))],
            None,
            Some(&carrier),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        self.env.pop_var_scope();
        Ok(MonadFailResolution::Preserve(MonadFailTarget {
            carrier_ty: carrier,
            error_id,
            call: Box::new(call?),
        }))
    }

    /// Pattern execution produces an Error; only the enclosing context chooses
    /// how to consume it. A rigid generic context never gains a new capability
    /// from a later concrete instantiation.
    pub(super) fn resolve_pattern_failure_target(
        &mut self,
        carrier: &Ty,
        propagated: &[Ty],
        span: &Span,
        alternative: Option<(&str, &str, &Span)>,
        allow_deferred: bool,
    ) -> Result<SafeBindFailureTarget, TypeError> {
        let carrier = self.resolve_ty(carrier);
        match self.resolve_monad_fail(&carrier, span)? {
            MonadFailResolution::Preserve(target) => {
                for error_ty in propagated {
                    if !self.types_compatible(&Ty::Error, error_ty)? {
                        return Err(self.policy_error(
                            TypeDiagnosticReason::SafeBindErrorTypeMismatch,
                            diagnostics::TypePolicy::SafeBindFailureTarget,
                            Some("Pattern failure".into()),
                            Some(&Ty::Error),
                            Some(error_ty),
                            None,
                            None,
                            span,
                            None,
                        ));
                    }
                }
                return Ok(if alternative.is_some() {
                    SafeBindFailureTarget::DoMonadFail(Box::new(target))
                } else {
                    SafeBindFailureTarget::EnclosingMonadFail(Box::new(target))
                });
            }
            MonadFailResolution::InvalidMetadata(subject) => {
                return Err(self.policy_error(
                    TypeDiagnosticReason::TypecheckInvariantViolation,
                    diagnostics::TypePolicy::ProducerContract,
                    Some(subject.into()),
                    None,
                    Some(&carrier),
                    None,
                    None,
                    span,
                    None,
                ));
            }
            MonadFailResolution::Deferred
                if alternative.is_some()
                    && allow_deferred
                    && !self.do_failure_carrier_is_rigid_variable(&carrier) =>
            {
                let (trait_key, method_name, keyword_span) =
                    alternative.expect("do failure context");
                return Ok(SafeBindFailureTarget::Deferred(Box::new(
                    DeferredDoFailureTarget {
                        carrier_ty: carrier,
                        alternative_trait_key: trait_key.into(),
                        alternative_method_name: method_name.into(),
                        propagated_error_tys: propagated.to_vec(),
                        failure_span: span.clone(),
                        do_keyword_span: keyword_span.clone(),
                    },
                )));
            }
            MonadFailResolution::Deferred if alternative.is_some() && !allow_deferred => {
                return Err(self.policy_error(
                    TypeDiagnosticReason::TypecheckInvariantViolation,
                    diagnostics::TypePolicy::ProducerContract,
                    Some("deferred do failure policy remained unresolved".into()),
                    None,
                    Some(&carrier),
                    None,
                    None,
                    span,
                    None,
                ));
            }
            MonadFailResolution::Unavailable | MonadFailResolution::Deferred => {}
        }
        let Some((trait_key, method_name, keyword_span)) = alternative else {
            if !type_contains_unresolved_vars(&carrier) {
                return Err(self.pattern_failure_error(
                    diagnostics::PatternFailureContext::Callable,
                    &carrier,
                    span,
                    self.function_return_origin.as_ref(),
                ));
            }
            return Err(self.policy_error(
                TypeDiagnosticReason::SafeBindRequiresMonadFailTarget,
                diagnostics::TypePolicy::SafeBindRequiresMonadFailTarget,
                Some("=?".into()),
                None,
                Some(&carrier),
                None,
                None,
                span,
                None,
            ));
        };
        let empty = self.check_trait_invocation(
            span,
            trait_key,
            method_name,
            &[],
            None,
            Some(&carrier),
            None,
            None,
            None,
            None,
            None,
            None,
        ).map_err(|error| {
            if !type_contains_unresolved_vars(&carrier)
                && error.reason() == Some(TypeDiagnosticReason::NoApplicableTraitImplementation)
                && matches!(error.structured.as_ref().map(|value| &value.data),
                    Some(diagnostics::DiagnosticData::TraitDispatch(value)) if value.trait_name == "Alternative")
            {
                self.pattern_failure_error(
                    diagnostics::PatternFailureContext::Do,
                    &carrier,
                    span,
                    Some(keyword_span),
                )
            } else {
                error
            }
        })?;
        Ok(SafeBindFailureTarget::DoAlternative {
            empty: Box::new(empty),
        })
    }

    pub(super) fn pattern_failure_error(
        &self,
        context: diagnostics::PatternFailureContext,
        carrier: &Ty,
        failure_span: &Span,
        target_span: Option<&Span>,
    ) -> TypeError {
        let (reason, target_role) = match context {
            diagnostics::PatternFailureContext::Callable => (
                TypeDiagnosticReason::SafeBindRequiresMonadFailTarget,
                diagnostics::SourceRole::ReturnType,
            ),
            diagnostics::PatternFailureContext::Do => (
                TypeDiagnosticReason::NoApplicableTraitImplementation,
                diagnostics::SourceRole::DoCarrier,
            ),
        };
        TypeError::from_structured(diagnostics::StructuredDiagnostic {
            reason: reason.into(),
            origin: diagnostics::DiagnosticOrigin::Intrinsic,
            data: diagnostics::DiagnosticData::PatternFailure(diagnostics::PatternFailureData {
                context,
                carrier_type: self.diagnostic_ty_name(&self.resolve_ty(carrier)),
            }),
            primary: diagnostics::SourceFact::untyped(
                diagnostics::SourceRole::Pattern,
                diagnostics::SourceId(0),
                failure_span.clone(),
            ),
            related: target_span
                .map(|span| self.type_fact(target_role, span, carrier))
                .into_iter()
                .collect(),
            remediation: None,
        })
    }

    pub(super) fn constructor_projection_failures_are_metadata(
        failures: &[ConstructorProjectionFailure],
    ) -> bool {
        failures.iter().all(|failure| {
            matches!(
                failure,
                ConstructorProjectionFailure::Canonicalization
                    | ConstructorProjectionFailure::MissingImplTargetMetadata
                    | ConstructorProjectionFailure::MissingConstructorSlotMapping { .. }
                    | ConstructorProjectionFailure::MissingWitnessTrait
                    | ConstructorProjectionFailure::SlotCountMismatch { .. }
                    | ConstructorProjectionFailure::InvalidSlotPosition { .. }
                    | ConstructorProjectionFailure::MissingNominalArguments
                    | ConstructorProjectionFailure::UnsupportedConstructor
            )
        })
    }

    pub(super) fn constructor_projection_failure_detail(
        failures: &[ConstructorProjectionFailure],
    ) -> String {
        failures
            .iter()
            .map(|failure| match failure {
                ConstructorProjectionFailure::ProofError(error) => error.message.clone(),
                ConstructorProjectionFailure::Canonicalization => {
                    "constructor type cannot be canonicalized".to_string()
                }
                ConstructorProjectionFailure::MissingImplTargetMetadata => {
                    "implementation target metadata is missing".to_string()
                }
                ConstructorProjectionFailure::UnsatisfiedConstraints => {
                    "implementation constraints are not satisfied".to_string()
                }
                ConstructorProjectionFailure::NoApplicableImplementation => {
                    "no implementation has the required constructor shape".to_string()
                }
                ConstructorProjectionFailure::AmbiguousImplementation => {
                    "more than one implementation has the required constructor shape".to_string()
                }
                ConstructorProjectionFailure::MissingConstructorSlotMapping { .. } => {
                    "a declared constructor slot has no canonical mapping".to_string()
                }
                ConstructorProjectionFailure::MissingWitnessTrait => {
                    "constructor witness capability metadata is missing".to_string()
                }
                ConstructorProjectionFailure::SlotCountMismatch { expected, actual } => {
                    format!("constructor slot count mismatch: expected {expected}, got {actual}")
                }
                ConstructorProjectionFailure::InvalidSlotPosition { position } => {
                    format!("constructor slot position {position} is outside the target type")
                }
                ConstructorProjectionFailure::MissingNominalArguments => {
                    "nominal constructor arguments are missing".to_string()
                }
                ConstructorProjectionFailure::UnsupportedConstructor => {
                    "type is not a supported constructor application".to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Public identity is independent of session-local declaration numbering.
    pub(super) fn diagnostic_constructor_family_id(&self, trait_key: &str) -> String {
        let family = self.constructor_family_key(trait_key);
        let mut identities = family
            .0
            .iter()
            .map(|id| {
                let info = self
                    .traits
                    .values()
                    .find(|info| info.id.unique_id == *id)
                    .expect("family member metadata");
                info.id
                    .qualified_name
                    .as_deref()
                    .unwrap_or(&info.id.name)
                    .to_string()
            })
            .collect::<Vec<_>>();
        identities.sort();
        format!("family:{}", identities.join("+"))
    }

    /// Inheritance is undirected for family identity, directed for capability.
    pub(super) fn constructor_family_key(&self, trait_key: &str) -> TypeCtorTraitFamilyId {
        let start = self
            .traits
            .get(trait_key)
            .expect("resolved constructor Trait");
        assert!(!start.constructor_slots.is_empty());
        let mut component = HashSet::from([start.id.unique_id]);
        loop {
            let before = component.len();
            for info in self
                .traits
                .values()
                .filter(|info| !info.constructor_slots.is_empty())
            {
                for parent in &info.parents {
                    let parent_info = self
                        .traits
                        .values()
                        .find(|info| info.id.unique_id == parent.trait_id.unique_id)
                        .expect("resolved parent Trait metadata");
                    if parent_info.constructor_slots.is_empty() {
                        continue;
                    }
                    if component.contains(&info.id.unique_id)
                        || component.contains(&parent_info.id.unique_id)
                    {
                        component.insert(info.id.unique_id);
                        component.insert(parent_info.id.unique_id);
                    }
                }
            }
            if before == component.len() {
                break;
            }
        }
        let mut identities: Vec<_> = component.into_iter().collect();
        identities.sort_unstable();
        TypeCtorTraitFamilyId(identities)
    }

    /// Two constructor Traits can share one application witness only when
    /// their mapped slots are anchored by a common ancestor. Parent-impl
    /// validation requires every child to preserve that ancestor's slot
    /// positions. A common descendant is insufficient: two unrelated parents
    /// may legally map the same nominal parameters in different orders.
    pub(super) fn constructor_traits_share_mapping(&self, left: &str, right: &str) -> bool {
        self.traits.iter().any(|(candidate, info)| {
            !info.constructor_slots.is_empty()
                && info.type_params.is_empty()
                && self.trait_bound_entails(left, candidate, &mut HashSet::new())
                && self.trait_bound_entails(right, candidate, &mut HashSet::new())
        })
    }

    pub(super) fn match_bare_constructor_occurrence(
        &mut self,
        occurrence: u32,
        ty: &Ty,
    ) -> Result<bool, TypeError> {
        self.match_constructor_occurrence(occurrence, ty, false)
    }

    pub(super) fn match_explicit_constructor_occurrence(
        &mut self,
        occurrence: u32,
        ty: &Ty,
    ) -> Result<bool, TypeError> {
        self.match_constructor_occurrence(occurrence, ty, true)
    }

    fn match_constructor_occurrence(
        &mut self,
        occurrence: u32,
        ty: &Ty,
        preserve_mapped_arguments: bool,
    ) -> Result<bool, TypeError> {
        let Some(required_trait) = self.constructor_witness_traits.get(&occurrence).cloned() else {
            return Ok(false);
        };
        Ok(match self.resolve_ty(&Ty::Var(occurrence)) {
            Ty::Var(unbound) => {
                if self.rigid_tyvars.contains(&unbound) {
                    return Ok(false);
                }
                match self
                    .constructor_head_projection(&required_trait, ty)
                    .into_checked()?
                {
                    ConstructorProjectionOutcome::Applicable { info, mut mapping } => {
                        let identity = if preserve_mapped_arguments {
                            self.normalize_inferred_constructor_identity(&required_trait, ty)
                        } else {
                            // A bare occurrence stores identity and captured
                            // arguments only. Its surrounding application owns
                            // every mapped payload.
                            for slot in &info.constructor_slot_vars {
                                mapping.insert(*slot, Ty::Hole);
                            }
                            Some(self.substitute_ty_with_mapping(&info.target_ty, &mapping))
                        };
                        match identity {
                            Some(identity) => self.bind_tyvar(unbound, &identity)?,
                            None => false,
                        }
                    }
                    // Captured arguments may still be selected by a later
                    // expected-result constraint. Keep deferred variables live
                    // until that constraint arrives, but never turn a rejected
                    // constructor proof into an inferred witness.
                    ConstructorProjectionOutcome::Deferred { .. } => {
                        self.bind_tyvar(unbound, ty)?
                    }
                    ConstructorProjectionOutcome::Rejected { .. } => false,
                }
            }
            witness if preserve_mapped_arguments => {
                let Some(identity) =
                    self.normalize_inferred_constructor_identity(&required_trait, ty)
                else {
                    return Ok(false);
                };
                let checkpoint = self.candidate_probe_checkpoint();
                let compatible = self.types_compatible(&witness, &identity);
                if !matches!(compatible, Ok(true)) {
                    self.rollback_candidate_probe(checkpoint);
                }
                compatible?
            }
            witness => {
                let ConstructorCarrierOutcome::Projected(actual_carrier) = self
                    .canonical_constructor_carrier(&required_trait, ty)
                    .into_checked()?
                else {
                    return Ok(false);
                };
                match self
                    .canonical_constructor_carrier(&required_trait, &witness)
                    .into_checked()?
                {
                    ConstructorCarrierOutcome::Projected(expected_carrier) => {
                        self.unify_constructor_carriers(&expected_carrier, &actual_carrier)?
                    }
                    ConstructorCarrierOutcome::Deferred { waiting_on } => {
                        debug_assert!(!waiting_on.is_empty());
                        false
                    }
                    ConstructorCarrierOutcome::Rejected { failures } => {
                        debug_assert!(!failures.is_empty());
                        false
                    }
                }
            }
        })
    }

    pub(super) fn is_projected_constructor_identity(&self, ty: &Ty) -> bool {
        fn contains_hole(ty: &CanonicalTy) -> bool {
            ty.head == CanonicalTypeHead::Hole || ty.arguments.iter().any(contains_hole)
        }
        self.canonical_request(&self.resolve_ty(ty))
            .ok()
            .is_some_and(|ty| ty.head != CanonicalTypeHead::SelfApplication && contains_hole(&ty))
    }

    /// Compare nominal arguments according to their declaration roles.
    /// Constructor parameters compare the carrier head and captured
    /// arguments; their mapped payload belongs to each application site.
    pub(super) fn nominal_arguments_compatible(
        &mut self,
        name: &str,
        left: &[Ty],
        right: &[Ty],
    ) -> Result<bool, TypeError> {
        if left.len() != right.len() {
            return Ok(false);
        }
        let constructor_traits = self
            .env
            .lookup_type_def(name)
            .map(|definition| {
                definition
                    .type_param_bounds
                    .iter()
                    .map(|bound| {
                        bound
                            .as_deref()
                            .and_then(|bound| self.declaration_constructor_trait_key(bound))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        try_all(
            left.iter().zip(right).enumerate(),
            |(ordinal, (left, right))| {
                if self.resolve_ty(left) == self.resolve_ty(right) {
                    return Ok(true);
                }
                let Some(trait_key) = constructor_traits
                    .get(ordinal)
                    .and_then(|trait_key| trait_key.as_deref())
                else {
                    return self.types_compatible(left, right);
                };
                Ok(match (self.resolve_ty(left), self.resolve_ty(right)) {
                    (Ty::Var(variable), right) if !self.rigid_tyvars.contains(&variable) => {
                        if let Some(existing) = self.constructor_witness_traits.get(&variable) {
                            if !self.trait_bound_entails(existing, trait_key, &mut HashSet::new()) {
                                return Ok(false);
                            }
                        } else {
                            self.constructor_witness_traits
                                .insert(variable, trait_key.to_string());
                        }
                        self.match_bare_constructor_occurrence(variable, &right)?
                    }
                    (left, Ty::Var(variable)) if !self.rigid_tyvars.contains(&variable) => {
                        if let Some(existing) = self.constructor_witness_traits.get(&variable) {
                            if !self.trait_bound_entails(existing, trait_key, &mut HashSet::new()) {
                                return Ok(false);
                            }
                        } else {
                            self.constructor_witness_traits
                                .insert(variable, trait_key.to_string());
                        }
                        self.match_bare_constructor_occurrence(variable, &left)?
                    }
                    (left, right) => {
                        let ConstructorCarrierOutcome::Projected(left) = self
                            .canonical_constructor_carrier(trait_key, &left)
                            .into_checked()?
                        else {
                            return Ok(false);
                        };
                        let ConstructorCarrierOutcome::Projected(right) = self
                            .canonical_constructor_carrier(trait_key, &right)
                            .into_checked()?
                        else {
                            return Ok(false);
                        };
                        self.unify_constructor_carriers(&left, &right)?
                    }
                })
            },
        )
    }

    /// Convert only independent, unresolved mapped slots to the stable
    /// constructor-identity marker.  Concrete mapped arguments from a full
    /// RTA, captured arguments, and correlated variables remain constraints.
    pub(super) fn normalize_inferred_constructor_identity(
        &self,
        trait_key: &str,
        ty: &Ty,
    ) -> Option<Ty> {
        let ConstructorProjectionOutcome::Applicable { info, .. } =
            self.constructor_head_projection(trait_key, ty)
        else {
            return None;
        };
        let mut canonical = self.canonical_request(ty).ok()?;
        let mapped = info
            .constructor_slot_positions
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        fn contains_variable(ty: &CanonicalTy, variable: u32) -> bool {
            ty.head == CanonicalTypeHead::Variable(variable)
                || ty
                    .arguments
                    .iter()
                    .any(|argument| contains_variable(argument, variable))
        }
        for position in mapped {
            let variable = match canonical.arguments.get(position).map(|ty| &ty.head) {
                Some(CanonicalTypeHead::Variable(variable)) => *variable,
                _ => continue,
            };
            if canonical
                .arguments
                .iter()
                .enumerate()
                .any(|(other, argument)| other != position && contains_variable(argument, variable))
            {
                continue;
            }
            canonical.arguments[position] = CanonicalTy {
                head: CanonicalTypeHead::Hole,
                arguments: Vec::new(),
            };
        }
        self.canonical_to_ty(&canonical).ok()
    }

    /// Lower verified constructor parameters to an executable type identity.
    /// Only declaration-marked constructor arguments are projected; ordinary
    /// value payloads keep the usual unresolved-type checks.
    pub(super) fn normalize_executable_constructor_identities(&self, ty: &Ty) -> Ty {
        let normalize = |ty: &Ty| self.normalize_executable_constructor_identities(ty);
        let normalize_nominal_arguments = |name: &str, arguments: &[Ty]| {
            let definition = self.env.lookup_type_def(name);
            arguments
                .iter()
                .enumerate()
                .map(|(ordinal, argument)| {
                    let argument = normalize(argument);
                    let constructor_trait = definition
                        .and_then(|definition| definition.type_param_bounds.get(ordinal))
                        .and_then(|bound| bound.as_deref())
                        .and_then(|bound| self.declaration_constructor_trait_key(bound));
                    constructor_trait
                        .and_then(|trait_key| {
                            self.normalize_inferred_constructor_identity(&trait_key, &argument)
                        })
                        .unwrap_or(argument)
                })
                .collect::<Vec<_>>()
        };

        match self.resolve_ty(ty) {
            Ty::Var(variable) => Ty::Var(variable),
            Ty::ExtractorClosure(inner) => Ty::ExtractorClosure(Box::new(normalize(&inner))),
            Ty::MatchResult(inner) => Ty::MatchResult(Box::new(normalize(&inner))),
            Ty::List(inner) => Ty::List(Box::new(normalize(&inner))),
            Ty::Lazy(inner) => Ty::Lazy(Box::new(normalize(&inner))),
            Ty::Tuple(items) => Ty::Tuple(items.iter().map(normalize).collect()),
            Ty::SelfApp(items) => Ty::SelfApp(items.iter().map(normalize).collect()),
            Ty::Result(ok, error) => {
                Ty::Result(Box::new(normalize(&ok)), Box::new(normalize(&error)))
            }
            Ty::Enum(name, arguments) => {
                Ty::Enum(name.clone(), normalize_nominal_arguments(&name, &arguments))
            }
            Ty::Struct(name, nominal) => {
                let arguments = normalize_nominal_arguments(&name, &nominal.arguments);
                let fields = self
                    .env
                    .lookup_type_def(&name)
                    .map(|definition| self.instantiate_type_def_fields(definition, &arguments))
                    .unwrap_or_else(|| {
                        nominal
                            .fields
                            .iter()
                            .map(|(field, ty)| (field.clone(), normalize(ty)))
                            .collect()
                    });
                Ty::Struct(name, NominalType::new(arguments, fields))
            }
            Ty::Record(name, nominal) => {
                let arguments = normalize_nominal_arguments(&name, &nominal.arguments);
                let fields = self
                    .env
                    .lookup_type_def(&name)
                    .map(|definition| self.instantiate_type_def_fields(definition, &arguments))
                    .unwrap_or_else(|| {
                        nominal
                            .fields
                            .iter()
                            .map(|(field, ty)| (field.clone(), normalize(ty)))
                            .collect()
                    });
                Ty::Record(name, NominalType::new(arguments, fields))
            }
            Ty::Func(parameters, ret) => Ty::Func(
                parameters.iter().map(normalize).collect(),
                Box::new(normalize(&ret)),
            ),
            Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
                name,
                params: params.iter().map(normalize).collect(),
                ret: Box::new(normalize(&ret)),
            },
            Ty::UserFunc {
                fun_idx,
                type_params,
                call_substitution,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx,
                type_params,
                call_substitution: call_substitution
                    .iter()
                    .map(|(variable, ty)| {
                        (
                            *variable,
                            self.normalize_executable_constructor_binding(*variable, ty),
                        )
                    })
                    .collect(),
                params: params.iter().map(normalize).collect(),
                ret: Box::new(normalize(&ret)),
            },
            Ty::Facet(kind, source, focus, update_source, update_focus) => Ty::Facet(
                kind,
                Box::new(normalize(&source)),
                Box::new(normalize(&focus)),
                Box::new(normalize(&update_source)),
                Box::new(normalize(&update_focus)),
            ),
            primitive => primitive,
        }
    }

    pub(super) fn normalize_executable_constructor_binding(&self, variable: u32, ty: &Ty) -> Ty {
        let ty = self.normalize_executable_constructor_identities(ty);
        self.constructor_witness_traits
            .get(&variable)
            .and_then(|trait_key| self.normalize_inferred_constructor_identity(trait_key, &ty))
            .unwrap_or(ty)
    }

    pub(super) fn normalize_executable_trait_argument(
        &self,
        trait_name: &str,
        ordinal: usize,
        ty: &Ty,
    ) -> Ty {
        let ty = self.normalize_executable_constructor_identities(ty);
        let constructor_trait = self
            .traits
            .get(trait_name)
            .and_then(|info| {
                info.type_params
                    .get(ordinal)
                    .map(|parameter| (info, parameter))
            })
            .and_then(|(info, parameter)| {
                self.trait_head_parameter_constructor_bound(info, &parameter.name)
            });
        constructor_trait
            .and_then(|trait_key| self.normalize_inferred_constructor_identity(&trait_key, &ty))
            .unwrap_or(ty)
    }

    fn unify_constructor_carriers(
        &mut self,
        expected: &CanonicalConstructorCarrier,
        actual: &CanonicalConstructorCarrier,
    ) -> Result<bool, TypeError> {
        if expected.family_id != actual.family_id
            || expected.constructor != actual.constructor
            || expected.arity != actual.arity
            || expected.mapped_slots != actual.mapped_slots
            || expected.captured_arguments.len() != actual.captured_arguments.len()
            || expected
                .captured_arguments
                .iter()
                .zip(&actual.captured_arguments)
                .any(|(expected, actual)| expected.position != actual.position)
        {
            return Ok(false);
        }
        let captured = expected
            .captured_arguments
            .iter()
            .zip(&actual.captured_arguments)
            .map(|(expected, actual)| {
                Some((
                    self.canonical_to_ty(&expected.ty).ok()?,
                    self.canonical_to_ty(&actual.ty).ok()?,
                ))
            })
            .collect::<Option<Vec<_>>>();
        let Some(captured) = captured else {
            return Ok(false);
        };
        let checkpoint = self.candidate_probe_checkpoint();
        for (expected, actual) in captured {
            let compatible = self.types_compatible(&expected, &actual);
            if !matches!(compatible, Ok(true)) {
                self.rollback_candidate_probe(checkpoint);
                return compatible;
            }
        }
        Ok(true)
    }

    pub(super) fn canonical_constructor_carrier(
        &self,
        trait_key: &str,
        ty: &Ty,
    ) -> ConstructorCarrierOutcome {
        self.canonical_constructor_carrier_projection(trait_key, ty, false)
    }

    pub(super) fn constructor_carrier_relation(
        &self,
        trait_key: &str,
        left: &Ty,
        right: &Ty,
    ) -> ConstructorCarrierRelation {
        let left = self.canonical_constructor_carrier(trait_key, left);
        let right = self.canonical_constructor_carrier(trait_key, right);
        // A deferred peer must not hide a failed required proof.
        for outcome in [&left, &right] {
            if let ConstructorCarrierOutcome::Rejected { failures } = outcome {
                if ConstructorProjectionFailure::proof_error(failures).is_some() {
                    return ConstructorCarrierRelation::Rejected {
                        failures: failures.clone(),
                    };
                }
            }
        }
        match (left, right) {
            (
                ConstructorCarrierOutcome::Projected(left),
                ConstructorCarrierOutcome::Projected(right),
            ) if left == right => ConstructorCarrierRelation::SameCarrier,
            (ConstructorCarrierOutcome::Projected(_), ConstructorCarrierOutcome::Projected(_)) => {
                ConstructorCarrierRelation::DifferentCarrier
            }
            (
                ConstructorCarrierOutcome::Deferred { mut waiting_on },
                ConstructorCarrierOutcome::Deferred {
                    waiting_on: right_waiting,
                },
            ) => {
                waiting_on.extend(right_waiting);
                waiting_on.sort_unstable();
                waiting_on.dedup();
                ConstructorCarrierRelation::Deferred { waiting_on }
            }
            (ConstructorCarrierOutcome::Deferred { waiting_on }, _)
            | (_, ConstructorCarrierOutcome::Deferred { waiting_on }) => {
                ConstructorCarrierRelation::Deferred { waiting_on }
            }
            (
                ConstructorCarrierOutcome::Rejected { mut failures },
                ConstructorCarrierOutcome::Rejected {
                    failures: right_failures,
                },
            ) => {
                failures.extend(right_failures);
                ConstructorCarrierRelation::Rejected { failures }
            }
            (ConstructorCarrierOutcome::Rejected { failures }, _)
            | (_, ConstructorCarrierOutcome::Rejected { failures }) => {
                ConstructorCarrierRelation::Rejected { failures }
            }
        }
    }

    pub(super) fn contextual_constructor_carrier(
        &self,
        trait_key: &str,
        ty: &Ty,
    ) -> ConstructorCarrierOutcome {
        self.canonical_constructor_carrier_projection(trait_key, ty, true)
    }

    fn canonical_constructor_carrier_projection(
        &self,
        trait_key: &str,
        ty: &Ty,
        contextual: bool,
    ) -> ConstructorCarrierOutcome {
        let projection = if contextual {
            self.constructor_capability_projection(trait_key, ty)
        } else {
            self.constructor_projection(trait_key, ty)
        };
        let implementation = match projection {
            ConstructorProjectionOutcome::Applicable { info, .. } => info,
            ConstructorProjectionOutcome::Deferred { waiting_on } => {
                return ConstructorCarrierOutcome::Deferred { waiting_on };
            }
            ConstructorProjectionOutcome::Rejected { failures } => {
                return ConstructorCarrierOutcome::Rejected { failures };
            }
        };
        let target = match self.canonical_request(ty) {
            Ok(target) => target,
            Err(_) => {
                return ConstructorCarrierOutcome::Rejected {
                    failures: vec![ConstructorProjectionFailure::Canonicalization],
                };
            }
        };
        let family_id = self.constructor_family_key(trait_key);
        let positions = &implementation.constructor_slot_positions;
        if positions.len() != implementation.constructor_slot_vars.len() {
            return ConstructorCarrierOutcome::Rejected {
                failures: vec![ConstructorProjectionFailure::SlotCountMismatch {
                    expected: implementation.constructor_slot_vars.len(),
                    actual: positions.len(),
                }],
            };
        }
        if let Some(position) = positions
            .iter()
            .copied()
            .find(|position| *position >= target.arguments.len())
        {
            return ConstructorCarrierOutcome::Rejected {
                failures: vec![ConstructorProjectionFailure::InvalidSlotPosition { position }],
            };
        }
        if positions.iter().collect::<HashSet<_>>().len() != positions.len() {
            return ConstructorCarrierOutcome::Rejected {
                failures: vec![ConstructorProjectionFailure::InvalidSlotPosition {
                    position: positions
                        .iter()
                        .copied()
                        .find(|position| {
                            positions
                                .iter()
                                .filter(|candidate| *candidate == position)
                                .count()
                                > 1
                        })
                        .unwrap_or(0),
                }],
            };
        }
        ConstructorCarrierOutcome::Projected(CanonicalConstructorCarrier {
            family_id: family_id.clone(),
            constructor: ConstructorHead::Concrete(target.head),
            arity: target.arguments.len() as u32,
            mapped_slots: positions
                .iter()
                .enumerate()
                .map(|(ordinal, position)| CanonicalMappedSlot {
                    slot_id: ConstructorSlotId {
                        family_id: family_id.clone(),
                        ordinal: ordinal as u32,
                    },
                    position: *position as u32,
                })
                .collect(),
            captured_arguments: target
                .arguments
                .into_iter()
                .enumerate()
                .filter(|(position, _)| !positions.contains(position))
                .map(|(position, ty)| CanonicalCapturedArgument {
                    position: position as u32,
                    ty,
                })
                .collect(),
        })
    }
}
