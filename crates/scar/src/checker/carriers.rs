use super::*;

impl Checker {
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
