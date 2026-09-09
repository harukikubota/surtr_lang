use super::*;

impl Checker {
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

    pub(super) fn match_bare_constructor_occurrence(&mut self, occurrence: u32, ty: &Ty) -> bool {
        let Some(required_trait) = self.constructor_witness_traits.get(&occurrence).cloned() else {
            return false;
        };
        match self.resolve_ty(&Ty::Var(occurrence)) {
            Ty::Var(unbound) => self.bind_tyvar(unbound, ty),
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
