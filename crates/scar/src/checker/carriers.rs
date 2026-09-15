use super::*;

impl Checker {
    pub(super) fn resolve_result_effect(&self, carrier: &Ty) -> ResultEffectResolution {
        let carrier = self.resolve_ty(carrier);
        if let Ty::Result(_, error_ty) = &carrier {
            return ResultEffectResolution::Preserve(ResultPreserveTarget {
                carrier_ty: carrier.clone(),
                error_ty: error_ty.as_ref().clone(),
                construction: ResultPreserveConstruction::CanonicalResult,
            });
        }
        let Ty::Struct(name, nominal) = &carrier else {
            return if type_contains_unresolved_vars(&carrier) {
                ResultEffectResolution::Deferred
            } else {
                ResultEffectResolution::Unavailable
            };
        };
        let Some(definition) = self.env.lookup_type_def(name) else {
            return ResultEffectResolution::Unavailable;
        };
        let Some(annotation) = definition.result_effect.as_ref() else {
            return ResultEffectResolution::Unavailable;
        };
        let Some(base) = nominal.arguments.get(annotation.base_parameter_index) else {
            return ResultEffectResolution::InvalidMetadata(
                "validated Result effect base argument is missing",
            );
        };
        let base = self.resolve_ty(base);
        if type_contains_unresolved_vars(&base) && !matches!(base, Ty::Result(_, _)) {
            return ResultEffectResolution::Deferred;
        }
        if !matches!(base, Ty::Result(_, _)) {
            return ResultEffectResolution::Unavailable;
        }
        let Some((_, Ty::Result(_, field_error_ty))) = nominal.fields.first() else {
            return ResultEffectResolution::InvalidMetadata(
                "validated Result effect field is not instantiated as canonical Result",
            );
        };
        let error_ty = self.resolve_ty(field_error_ty);
        let tag = definition.tag;
        ResultEffectResolution::Preserve(ResultPreserveTarget {
            carrier_ty: carrier,
            error_ty,
            construction: ResultPreserveConstruction::AnnotatedStruct { tag },
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

    pub(super) fn match_bare_constructor_occurrence(&mut self, occurrence: u32, ty: &Ty) -> bool {
        self.match_constructor_occurrence(occurrence, ty, false)
    }

    pub(super) fn match_explicit_constructor_occurrence(
        &mut self,
        occurrence: u32,
        ty: &Ty,
    ) -> bool {
        self.match_constructor_occurrence(occurrence, ty, true)
    }

    fn match_constructor_occurrence(
        &mut self,
        occurrence: u32,
        ty: &Ty,
        preserve_mapped_arguments: bool,
    ) -> bool {
        let Some(required_trait) = self.constructor_witness_traits.get(&occurrence).cloned() else {
            return false;
        };
        match self.resolve_ty(&Ty::Var(occurrence)) {
            Ty::Var(unbound) => {
                if self.rigid_tyvars.contains(&unbound) {
                    return false;
                }
                match self.constructor_head_projection(&required_trait, ty) {
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
                        identity.is_some_and(|identity| self.bind_tyvar(unbound, &identity))
                    }
                    // Captured arguments may still be selected by a later
                    // expected-result constraint. Keep deferred variables live
                    // until that constraint arrives, but never turn a rejected
                    // constructor proof into an inferred witness.
                    ConstructorProjectionOutcome::Deferred { .. } => self.bind_tyvar(unbound, ty),
                    ConstructorProjectionOutcome::Rejected { .. } => false,
                }
            }
            witness if preserve_mapped_arguments => {
                let Some(identity) =
                    self.normalize_inferred_constructor_identity(&required_trait, ty)
                else {
                    return false;
                };
                let checkpoint = self.candidate_probe_checkpoint();
                let compatible = self.types_compatible(&witness, &identity);
                if !compatible {
                    self.rollback_candidate_probe(checkpoint);
                }
                compatible
            }
            witness => {
                let ConstructorCarrierOutcome::Projected(actual_carrier) =
                    self.canonical_constructor_carrier(&required_trait, ty)
                else {
                    return false;
                };
                match self.canonical_constructor_carrier(&required_trait, &witness) {
                    ConstructorCarrierOutcome::Projected(expected_carrier) => {
                        self.unify_constructor_carriers(&expected_carrier, &actual_carrier)
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
        }
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
    ) -> bool {
        if left.len() != right.len() {
            return false;
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

        left.iter()
            .zip(right)
            .enumerate()
            .all(|(ordinal, (left, right))| {
                if self.resolve_ty(left) == self.resolve_ty(right) {
                    return true;
                }
                let Some(trait_key) = constructor_traits
                    .get(ordinal)
                    .and_then(|trait_key| trait_key.as_deref())
                else {
                    return self.types_compatible(left, right);
                };
                match (self.resolve_ty(left), self.resolve_ty(right)) {
                    (Ty::Var(variable), right) if !self.rigid_tyvars.contains(&variable) => {
                        if let Some(existing) = self.constructor_witness_traits.get(&variable) {
                            if !self.trait_bound_entails(existing, trait_key, &mut HashSet::new()) {
                                return false;
                            }
                        } else {
                            self.constructor_witness_traits
                                .insert(variable, trait_key.to_string());
                        }
                        self.match_bare_constructor_occurrence(variable, &right)
                    }
                    (left, Ty::Var(variable)) if !self.rigid_tyvars.contains(&variable) => {
                        if let Some(existing) = self.constructor_witness_traits.get(&variable) {
                            if !self.trait_bound_entails(existing, trait_key, &mut HashSet::new()) {
                                return false;
                            }
                        } else {
                            self.constructor_witness_traits
                                .insert(variable, trait_key.to_string());
                        }
                        self.match_bare_constructor_occurrence(variable, &left)
                    }
                    (left, right) => {
                        let ConstructorCarrierOutcome::Projected(left) =
                            self.canonical_constructor_carrier(trait_key, &left)
                        else {
                            return false;
                        };
                        let ConstructorCarrierOutcome::Projected(right) =
                            self.canonical_constructor_carrier(trait_key, &right)
                        else {
                            return false;
                        };
                        self.unify_constructor_carriers(&left, &right)
                    }
                }
            })
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
    ) -> bool {
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
            return false;
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
            return false;
        };
        let checkpoint = self.candidate_probe_checkpoint();
        for (expected, actual) in captured {
            if !self.types_compatible(&expected, &actual) {
                self.rollback_candidate_probe(checkpoint);
                return false;
            }
        }
        true
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
        match (
            self.canonical_constructor_carrier(trait_key, left),
            self.canonical_constructor_carrier(trait_key, right),
        ) {
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
