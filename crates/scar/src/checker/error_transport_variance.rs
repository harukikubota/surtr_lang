use super::*;

/// Constructor profiles contain only mapped-slot invariance, so recursive
/// declarations have a finite state space even when their type arguments grow.
#[derive(Clone, PartialEq, Eq, Hash)]
struct TransportState {
    owner: String,
    constructors: Vec<Option<Vec<bool>>>,
}

type TransportSummaries = HashMap<TransportState, Vec<bool>>;
type ConstructorProfiles = HashMap<u32, Option<Vec<bool>>>;

impl Checker {
    pub(super) fn nominal_error_transport_variance(
        &self,
        name: &str,
        arguments: &[Ty],
        span: &Span,
    ) -> Result<Vec<bool>, TypeError> {
        let mut summaries = HashMap::new();
        let mut root =
            self.transport_state(name, arguments, &HashMap::new(), &mut summaries, span)?;
        loop {
            let count = summaries.len();
            let mut changed = false;
            let states = summaries.keys().cloned().collect::<Vec<_>>();
            for state in states {
                let definition = self.env.lookup_type_def(&state.owner).ok_or_else(|| {
                    TypeError::new("Error transport declaration is missing", span.clone())
                })?;
                let profiles = definition
                    .type_param_vars
                    .iter()
                    .copied()
                    .zip(state.constructors.iter().cloned())
                    .collect();
                let mut invariant = HashSet::new();
                match definition.kind {
                    TypeKind::Enum | TypeKind::EnumError => {
                        let variants =
                            self.lookup_enum_variants_of(&state.owner).ok_or_else(|| {
                                TypeError::new("Error transport variants are missing", span.clone())
                            })?;
                        for variant in variants {
                            for payload in &variant.payload {
                                self.transport_invariant_variables(
                                    payload,
                                    false,
                                    &profiles,
                                    &mut summaries,
                                    &mut invariant,
                                    span,
                                )?;
                            }
                        }
                    }
                    _ => {
                        for (_, field) in &definition.fields {
                            self.transport_invariant_variables(
                                field,
                                false,
                                &profiles,
                                &mut summaries,
                                &mut invariant,
                                span,
                            )?;
                        }
                    }
                }
                let policy = summaries
                    .get_mut(&state)
                    .expect("registered transport state");
                for (index, variable) in definition.type_param_vars.iter().enumerate() {
                    if invariant.contains(variable) && !policy[index] {
                        policy[index] = true;
                        changed = true;
                    }
                }
            }
            let updated_root =
                self.transport_state(name, arguments, &HashMap::new(), &mut summaries, span)?;
            changed |= root != updated_root;
            root = updated_root;
            if !changed && count == summaries.len() {
                break;
            }
        }
        Ok(summaries.remove(&root).expect("root transport state"))
    }

    fn transport_state(
        &self,
        owner: &str,
        arguments: &[Ty],
        profiles: &ConstructorProfiles,
        summaries: &mut TransportSummaries,
        span: &Span,
    ) -> Result<TransportState, TypeError> {
        let definition = self.env.lookup_type_def(owner).ok_or_else(|| {
            TypeError::new(
                format!("Error transport declaration is missing: {owner}"),
                span.clone(),
            )
        })?;
        if arguments.len() != definition.type_param_vars.len() {
            return Err(TypeError::new(
                "Error transport argument metadata mismatch",
                span.clone(),
            ));
        }
        let mut constructors = Vec::new();
        for (argument, bound) in arguments.iter().zip(&definition.type_param_bounds) {
            let profile = if let Some(trait_key) = bound
                .as_ref()
                .and_then(|bound| self.declaration_constructor_trait_key(bound))
            {
                self.transport_constructor_profile(argument, &trait_key, profiles, summaries, span)?
            } else {
                None
            };
            constructors.push(profile);
        }
        let state = TransportState {
            owner: owner.to_string(),
            constructors,
        };
        summaries
            .entry(state.clone())
            .or_insert_with(|| vec![false; arguments.len()]);
        Ok(state)
    }

    fn transport_constructor_profile(
        &self,
        constructor: &Ty,
        trait_key: &str,
        profiles: &ConstructorProfiles,
        summaries: &mut TransportSummaries,
        span: &Span,
    ) -> Result<Option<Vec<bool>>, TypeError> {
        let constructor = self.resolve_ty(constructor);
        let variable = match &constructor {
            Ty::Var(variable) => Some(*variable),
            Ty::SelfApp(items) => {
                Self::constructor_application_parts(items).and_then(|(head, slots)| {
                    match (head, slots) {
                        (Ty::Var(variable), []) => Some(*variable),
                        _ => None,
                    }
                })
            }
            _ => None,
        };
        if let Some(variable) = variable {
            // The original generic declaration defers this profile to its
            // concrete specialization; fixed invariants are still checked now.
            return Ok(profiles.get(&variable).cloned().flatten());
        }
        match &constructor {
            Ty::List(_) => return Ok(Some(vec![false])),
            Ty::Result(_, _) => return Ok(Some(vec![true])),
            _ => {}
        }
        let (owner, arguments) = match &constructor {
            Ty::Enum(owner, arguments) => (owner, arguments),
            Ty::Struct(owner, nominal) | Ty::Record(owner, nominal) => (owner, &nominal.arguments),
            _ => {
                return Err(TypeError::new(
                    "Error transport constructor profile has no constructor identity",
                    span.clone(),
                ))
            }
        };
        let (implementation, _) = match self.constructor_head_projection(trait_key, &constructor) {
            ConstructorProjectionOutcome::Applicable { info, mapping } => (info, mapping),
            ConstructorProjectionOutcome::Deferred { .. } => return Ok(None),
            ConstructorProjectionOutcome::Rejected { failures } => {
                return Err(TypeError::new(
                    format!(
                        "Error transport constructor projection failed: {}",
                        Self::constructor_projection_failure_detail(&failures)
                    ),
                    span.clone(),
                ))
            }
        };
        let policy = if Self::canonical_user_type_name(owner) == "Global::ResultT" {
            vec![true, true, false]
        } else if matches!(
            owner.as_str(),
            "HashMap" | "Generator" | "InfiniteGenerator"
        ) {
            vec![false; arguments.len()]
        } else {
            let state = self.transport_state(owner, arguments, profiles, summaries, span)?;
            summaries[&state].clone()
        };
        implementation
            .constructor_slot_positions
            .iter()
            .map(|position| {
                policy.get(*position).copied().ok_or_else(|| {
                    TypeError::new(
                        "Error transport constructor slot metadata mismatch",
                        span.clone(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }

    fn transport_invariant_variables(
        &self,
        ty: &Ty,
        forced: bool,
        profiles: &ConstructorProfiles,
        summaries: &mut TransportSummaries,
        invariant: &mut HashSet<u32>,
        span: &Span,
    ) -> Result<(), TypeError> {
        if forced {
            let mut variables = Vec::new();
            Self::collect_ty_vars(ty, &mut variables);
            invariant.extend(variables);
            return Ok(());
        }
        match ty {
            Ty::Result(ok, error) => {
                self.transport_invariant_variables(ok, true, profiles, summaries, invariant, span)?;
                self.transport_invariant_variables(
                    error, false, profiles, summaries, invariant, span,
                )?;
            }
            Ty::Func(_, _) | Ty::UserFunc { .. } | Ty::BuiltinFunc { .. } | Ty::Facet(..) => {
                self.transport_invariant_variables(ty, true, profiles, summaries, invariant, span)?;
            }
            Ty::List(inner)
            | Ty::Lazy(inner)
            | Ty::Pid(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner) => {
                self.transport_invariant_variables(
                    inner, false, profiles, summaries, invariant, span,
                )?;
            }
            Ty::Tuple(items) => {
                for item in items {
                    self.transport_invariant_variables(
                        item, false, profiles, summaries, invariant, span,
                    )?;
                }
            }
            Ty::SelfApp(items) => {
                let (head, slots) =
                    Self::constructor_application_parts(items).ok_or_else(|| {
                        TypeError::new(
                            "Error transport constructor application metadata is missing",
                            span.clone(),
                        )
                    })?;
                let Ty::Var(variable) = head else {
                    return Err(TypeError::new("Error transport constructor application head must be a declaration witness", span.clone()));
                };
                invariant.insert(*variable);
                let profile = profiles.get(variable).cloned().flatten();
                if profile
                    .as_ref()
                    .is_some_and(|policy| policy.len() != slots.len())
                {
                    return Err(TypeError::new(
                        "Error transport constructor application arity mismatch",
                        span.clone(),
                    ));
                }
                for (index, slot) in slots.iter().enumerate() {
                    self.transport_invariant_variables(
                        slot,
                        profile.as_ref().is_some_and(|policy| policy[index]),
                        profiles,
                        summaries,
                        invariant,
                        span,
                    )?;
                }
            }
            Ty::Enum(owner, arguments) => self.transport_nominal_variables(
                owner, arguments, profiles, summaries, invariant, span,
            )?,
            Ty::Struct(owner, nominal) | Ty::Record(owner, nominal) => self
                .transport_nominal_variables(
                    owner,
                    &nominal.arguments,
                    profiles,
                    summaries,
                    invariant,
                    span,
                )?,
            _ => {}
        }
        Ok(())
    }

    fn transport_nominal_variables(
        &self,
        owner: &str,
        arguments: &[Ty],
        profiles: &ConstructorProfiles,
        summaries: &mut TransportSummaries,
        invariant: &mut HashSet<u32>,
        span: &Span,
    ) -> Result<(), TypeError> {
        let policy = if Self::canonical_user_type_name(owner) == "Global::ResultT" {
            vec![true, true, false]
        } else if matches!(owner, "HashMap" | "Generator" | "InfiniteGenerator") {
            vec![false; arguments.len()]
        } else {
            let state = self.transport_state(owner, arguments, profiles, summaries, span)?;
            summaries[&state].clone()
        };
        for (argument, forced) in arguments.iter().zip(policy) {
            self.transport_invariant_variables(
                argument, forced, profiles, summaries, invariant, span,
            )?;
        }
        Ok(())
    }
}
