use super::*;

impl Checker {
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

    pub(super) fn constructor_family_witness_root(&self, mut witness: u32) -> Option<u32> {
        let mut visited = HashSet::new();
        while let Some(shared) = self.constructor_family_witnesses.get(&witness).copied() {
            if !visited.insert(witness) {
                return None;
            }
            witness = shared;
        }
        Some(witness)
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

    // Occurrence witnesses retain their own capability while aliases share only
    // the family carrier. In particular, payload order cannot erase slot identity.
    pub(super) fn constructor_occurrence_matches_family(&self, occurrence: u32, ty: &Ty) -> bool {
        let Some(shared) = self.constructor_family_witnesses.get(&occurrence) else {
            return true;
        };
        let (Some(required), Some(family)) = (
            self.constructor_witness_traits.get(&occurrence),
            self.constructor_witness_traits.get(shared),
        ) else {
            return false;
        };
        let shared_ty = self.resolve_ty(&Ty::Var(*shared));
        let shared_ty = if matches!(shared_ty, Ty::Var(_)) {
            ty
        } else {
            &shared_ty
        };
        match (
            self.canonical_constructor_carrier(required, ty),
            self.canonical_constructor_carrier(family, shared_ty),
        ) {
            (Some(required), Some(family)) => {
                required == family && self.constructor_occurrence_matches_family(*shared, ty)
            }
            _ => false,
        }
    }

    pub(super) fn match_bare_constructor_occurrence(&mut self, occurrence: u32, ty: &Ty) -> bool {
        let Some(required_trait) = self.constructor_witness_traits.get(&occurrence).cloned() else {
            return false;
        };
        let Some(root) = self.constructor_family_witness_root(occurrence) else {
            return false;
        };
        let Some(root_trait) = self.constructor_witness_traits.get(&root).cloned() else {
            return false;
        };
        match self.resolve_ty(&Ty::Var(root)) {
            Ty::Var(unbound) => self.bind_tyvar(unbound, ty),
            witness => {
                let Some(actual_carrier) = self.canonical_constructor_carrier(&required_trait, ty)
                else {
                    return false;
                };
                self.canonical_constructor_carrier(&root_trait, &witness)
                    .is_some_and(|root_carrier| {
                        self.unify_constructor_carriers(&root_carrier, &actual_carrier)
                    })
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
    ) -> Option<CanonicalConstructorCarrier> {
        let (implementation, _) = self.constructor_projection(trait_key, ty)?;
        let target = self.canonical_request(ty).ok()?;
        let family_id = self.constructor_family_key(trait_key);
        let positions = &implementation.constructor_slot_positions;
        if positions.len() != implementation.constructor_slot_vars.len()
            || positions
                .iter()
                .any(|position| *position >= target.arguments.len())
            || positions.iter().collect::<HashSet<_>>().len() != positions.len()
        {
            return None;
        }
        Some(CanonicalConstructorCarrier {
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
