use super::*;

impl Checker {
    pub(super) fn register_tyvar_bound(&mut self, var: u32, trait_name: &str) {
        let bounds = self.tyvar_bounds.entry(var).or_default();
        if !bounds.iter().any(|bound| bound == trait_name) {
            bounds.push(trait_name.to_string());
            bounds.sort();
        }
    }

    pub(super) fn register_tyvar_bounds(&mut self, var: u32, bounds: &[String]) {
        for bound in bounds {
            self.register_tyvar_bound(var, bound);
        }
    }

    pub(super) fn tyvar_bound_names(&self, var: u32) -> Vec<String> {
        self.tyvar_bounds.get(&var).cloned().unwrap_or_default()
    }

    pub(super) fn tyvar_has_bound(&mut self, var: u32, trait_name: &str) -> bool {
        let requested_family = trait_name;
        let matching_bounds = self
            .tyvar_bounds
            .get(&var)
            .into_iter()
            .flatten()
            .filter(|bound| {
                let bound_family = bound.as_str();
                bound_family == requested_family
                    || self.trait_bound_entails(bound_family, requested_family, &mut HashSet::new())
            })
            .cloned()
            .collect::<Vec<_>>();
        if !matching_bounds.is_empty() {
            for capability in &mut self.active_capabilities {
                let capability_family = &capability.trait_id;
                if matches!(&capability.subject_ty, Ty::Var(subject_var) if *subject_var == var)
                    && matching_bounds.iter().any(|bound| {
                        let bound_family = bound.as_str();
                        capability_family == bound_family
                    })
                {
                    capability.consumed = true;
                }
            }
        }
        !matching_bounds.is_empty()
    }

    pub(super) fn consume_matching_capability(&mut self, receiver: &Ty, trait_name: &str) -> bool {
        let receiver = self.resolve_ty(receiver);
        let requested_family = trait_name;
        let matches = self
            .active_capabilities
            .iter()
            .enumerate()
            .filter_map(|(index, capability)| {
                let subject = self.resolve_ty(&capability.subject_ty);
                let capability_family = &capability.trait_id;
                let family_matches = capability_family == requested_family
                    || self.trait_bound_entails(
                        capability_family,
                        requested_family,
                        &mut HashSet::new(),
                    );
                let receiver_matches =
                    self.capability_receiver_matches(&capability.trait_id, &subject, &receiver);
                (receiver_matches && family_matches).then_some(index)
            })
            .collect::<Vec<_>>();
        let matched = !matches.is_empty();
        for index in matches {
            self.active_capabilities[index].consumed = true;
        }
        matched
    }

    pub(super) fn lit_type(&self, lit: &Lit) -> Ty {
        match lit {
            Lit::Int(_) => Ty::Int,
            Lit::Float(_) => Ty::Float,
            Lit::Str(_) => Ty::Str,
            Lit::Bool(_) => Ty::Bool,
            Lit::Unit => Ty::Unit,
        }
    }

    pub(super) fn contextual_callable_expected(&self, expected: Option<&Ty>) -> Option<Ty> {
        let expected = expected?;
        let resolved = self.resolve_ty(expected);
        let (inputs, _) = self.function_parts(&resolved)?;
        (inputs.len() == 1).then(|| expected.clone())
    }

    pub(super) fn error_positions_agree(&self, expected: &Ty, actual: &Ty) -> bool {
        let expected = self.resolve_ty(expected);
        let actual = self.resolve_ty(actual);
        let inferable =
            |ty: &Ty| matches!(ty, Ty::Var(variable) if !self.rigid_tyvars.contains(variable));
        if self.is_error_slot_ty(&expected) && self.is_error_slot_ty(&actual) {
            return inferable(&expected) || inferable(&actual) || expected == actual;
        }
        let function = |ty: Ty| match ty {
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                Ty::Func(params, ret)
            }
            other => other,
        };
        let expected = function(expected);
        let actual = function(actual);
        match (&expected, &actual) {
            (Ty::Result(a, e), Ty::Result(b, f)) => {
                self.error_positions_agree(a, b) && self.error_positions_agree(e, f)
            }
            (Ty::List(a), Ty::List(b))
            | (Ty::Lazy(a), Ty::Lazy(b))
            | (Ty::MatchResult(a), Ty::MatchResult(b))
            | (Ty::ExtractorClosure(a), Ty::ExtractorClosure(b))
            | (Ty::Pid(a), Ty::Pid(b)) => self.error_positions_agree(a, b),
            (Ty::Tuple(a), Ty::Tuple(b))
            | (Ty::Enum(_, a), Ty::Enum(_, b))
            | (Ty::SelfApp(a), Ty::SelfApp(b)) => a
                .iter()
                .zip(b)
                .all(|(a, b)| self.error_positions_agree(a, b)),
            (Ty::Struct(_, a), Ty::Struct(_, b)) | (Ty::Record(_, a), Ty::Record(_, b)) => a
                .arguments
                .iter()
                .zip(&b.arguments)
                .all(|(a, b)| self.error_positions_agree(a, b)),
            (Ty::Func(a, r), Ty::Func(b, s)) => {
                a.iter()
                    .zip(b)
                    .all(|(a, b)| self.error_positions_agree(a, b))
                    && self.error_positions_agree(r, s)
            }
            (Ty::Facet(_, s, f, u, v), Ty::Facet(_, t, g, w, x)) => {
                [(&**s, &**t), (&**f, &**g), (&**u, &**w), (&**v, &**x)]
                    .into_iter()
                    .all(|(a, b)| self.error_positions_agree(a, b))
            }
            _ => true,
        }
    }

    pub(super) fn types_compatible(&mut self, expected: &Ty, got: &Ty) -> Result<bool, TypeError> {
        let profile = self.profiler.start();
        let result = (|| -> Result<bool, TypeError> {
            let expected_bare_occurrence = Self::bare_constructor_occurrence(expected);
            let got_bare_occurrence = Self::bare_constructor_occurrence(got);
            let expected_function = match expected {
                Ty::Func(parameters, output) => Some((parameters.as_slice(), output.as_ref())),
                _ => None,
            };
            let got_function = match got {
                Ty::Func(parameters, output) => Some((parameters.as_slice(), output.as_ref())),
                _ => None,
            };
            let expected = self.resolve_ty(expected);
            let got = self.resolve_ty(got);
            let result = match (&expected, &got) {
                (Ty::Hole, Ty::Hole) => true,
                (Ty::Error, Ty::Var(var))
                    if self.error_slot_tyvars.contains(var) && self.rigid_tyvars.contains(var) =>
                {
                    true
                }
                (Ty::Var(var), Ty::Error)
                    if self.error_slot_tyvars.contains(var) && self.rigid_tyvars.contains(var) =>
                {
                    true
                }

                (Ty::Var(left), Ty::Var(right)) => match (
                    self.rigid_tyvars.contains(left),
                    self.rigid_tyvars.contains(right),
                ) {
                    (true, true) => left == right,
                    (true, false) => self.bind_tyvar(*right, &Ty::Var(*left))?,
                    (false, true) => self.bind_tyvar(*left, &Ty::Var(*right))?,
                    (false, false) => self.bind_tyvar(*left, &Ty::Var(*right))?,
                },
                (Ty::Var(var), _) if self.rigid_tyvars.contains(var) => false,
                (_, Ty::Var(var)) if self.rigid_tyvars.contains(var) => false,
                (Ty::Var(var), ty) | (ty, Ty::Var(var)) => self.bind_tyvar(*var, ty)?,
                (Ty::Int, Ty::Int)
                | (Ty::Float, Ty::Float)
                | (Ty::Str, Ty::Str)
                | (Ty::Bool, Ty::Bool)
                | (Ty::Unit, Ty::Unit)
                | (Ty::Error, Ty::Error) => true,
                (Ty::Error, ty) if self.enum_error_name(ty).is_some() => true,
                (ty, got)
                    if self.enum_error_name(ty).is_some()
                        && self.is_error_value_ty(got)
                        && ty != got =>
                {
                    self.env
                        .enum_error_other
                        .contains(&self.enum_error_name(ty).expect("guarded EnumError"))
                }
                (Ty::MatchResult(a), Ty::MatchResult(b))
                | (Ty::ExtractorClosure(a), Ty::ExtractorClosure(b))
                | (Ty::List(a), Ty::List(b)) => self.types_compatible(a, b)?,
                (Ty::Lazy(a), Ty::Lazy(b)) => self.types_compatible(a, b)?,
                (Ty::ProcessMarker(a), Ty::ProcessMarker(b)) => a == b,
                (Ty::Pid(a), Ty::Pid(b)) => self.types_compatible(a, b)?,
                (Ty::Pid(expected_process), Ty::Enum(name, args))
                    if name == "WorkerLease" && args.len() == 1 =>
                {
                    match args.first() {
                        Some(Ty::Pid(actual_process)) => {
                            self.types_compatible(expected_process, actual_process)?
                        }
                        _ => false,
                    }
                }
                (
                    Ty::Facet(kind_a, src_a, focus_a, update_src_a, update_focus_a),
                    Ty::Facet(kind_b, src_b, focus_b, update_src_b, update_focus_b),
                ) => {
                    kind_a.accepts(*kind_b)
                        && self.types_compatible(src_a, src_b)?
                        && self.types_compatible(focus_a, focus_b)?
                        && self.types_compatible(update_src_a, update_src_b)?
                        && self.types_compatible(update_focus_a, update_focus_b)?
                }
                (Ty::Tuple(a), Ty::Tuple(b)) => {
                    a.len() == b.len()
                        && try_all(a.iter().zip(b.iter()), |(left, right)| {
                            self.types_compatible(left, right)
                        })?
                }
                (Ty::SelfApp(a), Ty::SelfApp(b))
                    if Self::constructor_application_parts(a).is_some()
                        && Self::constructor_application_parts(b).is_some() =>
                {
                    let (left_witness, left_slots) =
                        Self::constructor_application_parts(a).expect("checked above");
                    let (right_witness, right_slots) =
                        Self::constructor_application_parts(b).expect("checked above");
                    left_slots.len() == right_slots.len()
                        && self.types_compatible(left_witness, right_witness)?
                        && try_all(
                            left_slots.iter().zip(right_slots.iter()),
                            |(left, right)| self.types_compatible(left, right),
                        )?
                }
                (Ty::SelfApp(a), other) if Self::constructor_application_parts(a).is_some() => {
                    let (witness, expected_slots) =
                        Self::constructor_application_parts(a).expect("checked above");
                    if expected_slots.is_empty() {
                        match expected_bare_occurrence {
                            Some(occurrence) => {
                                self.match_bare_constructor_occurrence(occurrence, other)?
                            }
                            None => self.types_compatible(witness, other)?,
                        }
                    } else if !match witness {
                        Ty::Var(occurrence)
                            if self.constructor_witness_traits.contains_key(occurrence)
                                && (matches!(self.resolve_ty(witness), Ty::Var(_))
                                    || self.is_projected_constructor_identity(witness)) =>
                        {
                            self.match_bare_constructor_occurrence(*occurrence, other)?
                        }
                        _ => self.types_compatible(witness, other)?,
                    } {
                        false
                    } else {
                        match self
                            .constructor_application_slots_for_witness(
                                witness,
                                expected_slots.len(),
                                other,
                            )
                            .into_checked()?
                        {
                            ConstructorSlotsOutcome::Projected(actual_slots) => {
                                actual_slots.len() == expected_slots.len()
                                    && try_all(
                                        expected_slots.iter().zip(actual_slots.iter()),
                                        |(expected, actual)| {
                                            self.types_compatible(expected, actual)
                                        },
                                    )?
                            }
                            ConstructorSlotsOutcome::Deferred { .. }
                            | ConstructorSlotsOutcome::Rejected { .. } => false,
                        }
                    }
                }
                (other, Ty::SelfApp(b)) if Self::constructor_application_parts(b).is_some() => {
                    let (_, slots) = Self::constructor_application_parts(b).expect("checked above");
                    if slots.is_empty() {
                        match got_bare_occurrence {
                            Some(occurrence) => {
                                self.match_bare_constructor_occurrence(occurrence, other)?
                            }
                            None => self.types_compatible(&Ty::SelfApp(b.clone()), other)?,
                        }
                    } else {
                        self.types_compatible(&Ty::SelfApp(b.clone()), other)?
                    }
                }
                (Ty::SelfApp(a), Ty::SelfApp(b)) => {
                    a.len() == b.len()
                        && try_all(a.iter().zip(b.iter()), |(left, right)| {
                            self.types_compatible(left, right)
                        })?
                }
                (Ty::Func(a_params, a_ret), Ty::Func(b_params, b_ret)) => {
                    // Preserve declaration-owned constructor occurrences while
                    // comparing every input and output with the ordinary relation.
                    let (a_params, a_ret) =
                        expected_function.unwrap_or((a_params.as_slice(), a_ret.as_ref()));
                    let (b_params, b_ret) =
                        got_function.unwrap_or((b_params.as_slice(), b_ret.as_ref()));
                    a_params.len() == b_params.len()
                        && self.error_positions_agree(
                            &Ty::Func(a_params.to_vec(), Box::new(a_ret.clone())),
                            &Ty::Func(b_params.to_vec(), Box::new(b_ret.clone())),
                        )
                        && try_all(a_params.iter().zip(b_params.iter()), |(a, b)| {
                            self.types_compatible(a, b)
                        })?
                        && self.types_compatible(a_ret, b_ret)?
                        && self.error_positions_agree(
                            &Ty::Func(a_params.to_vec(), Box::new(a_ret.clone())),
                            &Ty::Func(b_params.to_vec(), Box::new(b_ret.clone())),
                        )
                }
                (Ty::Result(ok1, err1), Ty::Result(ok2, err2)) => {
                    self.types_compatible(ok1, ok2)? && self.types_compatible(err1, err2)?
                }
                (Ty::Struct(n1, fields1), Ty::Struct(n2, fields2)) => {
                    Self::canonical_user_type_name(n1) == Self::canonical_user_type_name(n2)
                        && self.nominal_arguments_compatible(
                            n1,
                            &fields1.arguments,
                            &fields2.arguments,
                        )?
                }
                (Ty::Record(n1, fields1), Ty::Record(n2, fields2)) => {
                    Self::canonical_user_type_name(n1) == Self::canonical_user_type_name(n2)
                        && self.nominal_arguments_compatible(
                            n1,
                            &fields1.arguments,
                            &fields2.arguments,
                        )?
                }
                (Ty::Enum(n1, args1), Ty::Enum(n2, args2)) => {
                    Self::canonical_user_type_name(n1) == Self::canonical_user_type_name(n2)
                        && self.nominal_arguments_compatible(n1, args1, args2)?
                }
                _ => false,
            };
            Ok(result)
        })();
        self.profiler.finish(ProfileEvent::TypesCompatible, profile);
        result
    }

    /// A contextual trait capability applies to every value in its declared
    /// constructor slot(s), while captured nominal arguments must still agree.
    /// This is stricter than nominal-head comparison but permits a Factory
    /// capability on Boxed<$A> to prove a receiverless Factory::make() result
    /// of Boxed<Int>.
    pub(super) fn capability_receiver_matches(
        &self,
        capability_trait: &str,
        subject: &Ty,
        receiver: &Ty,
    ) -> bool {
        if self.resolve_ty(subject) == self.resolve_ty(receiver) {
            return true;
        }
        let canonical_subject = match self.contextual_constructor_carrier(capability_trait, subject)
        {
            ConstructorCarrierOutcome::Projected(carrier) => carrier,
            ConstructorCarrierOutcome::Deferred { waiting_on } => {
                debug_assert!(!waiting_on.is_empty());
                return false;
            }
            ConstructorCarrierOutcome::Rejected { failures } => {
                debug_assert!(!failures.is_empty());
                return false;
            }
        };
        let canonical_receiver =
            match self.contextual_constructor_carrier(capability_trait, receiver) {
                ConstructorCarrierOutcome::Projected(carrier) => carrier,
                ConstructorCarrierOutcome::Deferred { waiting_on } => {
                    debug_assert!(!waiting_on.is_empty());
                    return false;
                }
                ConstructorCarrierOutcome::Rejected { failures } => {
                    debug_assert!(!failures.is_empty());
                    return false;
                }
            };
        canonical_subject == canonical_receiver
    }

    pub(super) fn signature_tyvar_ids(tyvars: &HashMap<String, Ty>) -> HashSet<u32> {
        tyvars
            .values()
            .filter_map(|ty| match ty {
                Ty::Var(var) => Some(*var),
                _ => None,
            })
            .collect()
    }

    pub(super) fn types_compatible_with_rigid(
        &mut self,
        expected: &Ty,
        got: &Ty,
        rigid_tyvars: &HashSet<u32>,
    ) -> Result<bool, TypeError> {
        let saved = std::mem::replace(&mut self.rigid_tyvars, rigid_tyvars.clone());
        let compatible = self.types_compatible(expected, got);
        self.rigid_tyvars = saved;
        compatible
    }

    pub(super) fn bind_tyvar(&mut self, var: u32, ty: &Ty) -> Result<bool, TypeError> {
        let profile = self.profiler.start();
        let result = (|| -> Result<bool, TypeError> {
            let ty = self.resolve_ty(ty);
            if !self.validate_error_slot_binding(var, &ty)? {
                return Ok(false);
            }
            if self.process_marker_tyvars.contains(&var)
                && !matches!(ty, Ty::Var(_) | Ty::ProcessMarker(_))
            {
                return Ok(false);
            }
            let result = if ty == Ty::Var(var) {
                true
            } else if self.ty_contains_var(&ty, var) {
                false
            } else {
                let var_bounds = self.tyvar_bound_names(var);
                let pending_obligations = self
                    .pending_trait_obligations
                    .get(&var)
                    .cloned()
                    .unwrap_or_default();
                match &ty {
                    Ty::Var(other) => {
                        if self.rigid_tyvars.contains(other)
                            && self.process_marker_tyvars.contains(&var)
                            && !self.process_marker_tyvars.contains(other)
                        {
                            return Ok(false);
                        }
                        if self.process_marker_tyvars.contains(&var)
                            || self.process_marker_tyvars.contains(other)
                        {
                            self.process_marker_tyvars.insert(var);
                            self.process_marker_tyvars.insert(*other);
                        }
                        if self.rigid_tyvars.contains(other)
                            && !var_bounds
                                .iter()
                                .all(|bound| self.tyvar_has_bound(*other, bound))
                        {
                            return Ok(false);
                        }
                        let mut combined = var_bounds;
                        for bound in self.tyvar_bound_names(*other) {
                            if !combined.iter().any(|existing| existing == &bound) {
                                combined.push(bound);
                            }
                        }
                        combined.sort();
                        self.tyvar_bounds.insert(var, combined.clone());
                        self.tyvar_bounds.insert(*other, combined);
                        let binding = HashMap::from([(var, Ty::Var(*other))]);
                        for mut obligation in pending_obligations {
                            obligation.receiver = self.substitute_ty_with_mapping(
                                &self.resolve_ty(&obligation.receiver),
                                &binding,
                            );
                            obligation.args = obligation
                                .args
                                .iter()
                                .map(|arg| {
                                    self.substitute_ty_with_mapping(&self.resolve_ty(arg), &binding)
                                })
                                .collect();
                            let pending = self.pending_trait_obligations.entry(*other).or_default();
                            if !pending.contains(&obligation) {
                                pending.push(obligation);
                            }
                        }
                    }
                    _ => {
                        let has_constructor_identity_marker =
                            self.canonical_request(&ty).ok().is_some_and(|canonical| {
                                fn contains_hole(ty: &CanonicalTy) -> bool {
                                    ty.head == CanonicalTypeHead::Hole
                                        || ty.arguments.iter().any(contains_hole)
                                }
                                canonical.head != CanonicalTypeHead::SelfApplication
                                    && contains_hole(&canonical)
                            });
                        let bounds_satisfied = try_all(var_bounds.iter(), |bound| {
                            let constructor_bound =
                                (self.constructor_witness_traits.contains_key(&var)
                                    && has_constructor_identity_marker)
                                    .then(|| self.declaration_constructor_trait_key(bound))
                                    .flatten();
                            Ok(match constructor_bound {
                                Some(trait_key) => matches!(
                                    self.constructor_head_projection(&trait_key, &ty)
                                        .into_checked()?,
                                    ConstructorProjectionOutcome::Applicable { .. }
                                ),
                                None => {
                                    self.ty_satisfies_bounds(&ty, std::slice::from_ref(bound))?
                                }
                            })
                        })?;
                        if !bounds_satisfied {
                            return Ok(false);
                        }
                        if !try_all(pending_obligations.iter(), |obligation| {
                            let binding = HashMap::from([(var, ty.clone())]);
                            let receiver = self.substitute_ty_with_mapping(
                                &self.resolve_ty(&obligation.receiver),
                                &binding,
                            );
                            let arguments = obligation
                                .args
                                .iter()
                                .map(|arg| {
                                    self.substitute_ty_with_mapping(&self.resolve_ty(arg), &binding)
                                })
                                .collect::<Vec<_>>();
                            Ok(matches!(
                                self.probe_trait_head(&obligation.trait_id, &arguments, &receiver)?,
                                ApplicabilityProof::Satisfied(_)
                            ))
                        })? {
                            return Ok(false);
                        }
                    }
                }
                self.substitutions.insert(var, ty);
                self.pending_trait_obligations.remove(&var);
                true
            };
            Ok(result)
        })();
        self.profiler.finish(ProfileEvent::BindTyVar, profile);
        result
    }

    pub(super) fn ty_satisfies_bounds(
        &mut self,
        ty: &Ty,
        bounds: &[String],
    ) -> Result<bool, TypeError> {
        if bounds.is_empty() {
            return Ok(true);
        }

        match self.resolve_ty(ty) {
            Ty::Var(var) => Ok(bounds.iter().all(|bound| self.tyvar_has_bound(var, bound))),
            concrete => try_all(bounds.iter(), |bound| {
                Ok(matches!(
                    self.prove_trait_capability(bound, &concrete)?,
                    ApplicabilityProof::Satisfied(_)
                ))
            }),
        }
    }

    pub(super) fn validate_nominal_type_arguments(
        &mut self,
        def: &crate::env::TypeDefInfo,
        arguments: &[Ty],
        span: &Span,
        defer_unresolved: bool,
    ) -> Result<(), TypeError> {
        if def.type_params.len() != arguments.len()
            || def.type_param_bounds.len() != arguments.len()
        {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Internal consistency error: type {} has incomplete declaration constraint metadata",
                    Self::surface_name(&def.name)
                ),
                span: span.clone(),
                hint: None,
            });
        }

        if let Some(pending) = self.pending_nominal_arguments.as_mut() {
            pending.push((
                def.name.clone(),
                arguments.to_vec(),
                span.clone(),
                defer_unresolved,
            ));
            return Ok(());
        }

        for (variable, argument) in def.type_param_vars.iter().zip(arguments) {
            if self.error_slot_tyvars.contains(variable) {
                self.require_error_slot(argument, span)?;
            }
        }
        for ((parameter, bound), argument) in def
            .type_params
            .iter()
            .zip(&def.type_param_bounds)
            .zip(arguments)
        {
            let Some(bound) = bound else {
                continue;
            };
            let resolved = self.resolve_ty(argument);
            if defer_unresolved && matches!(resolved, Ty::Var(_)) {
                continue;
            }
            let satisfied = if let Ty::Var(var) = &resolved {
                self.tyvar_has_bound(*var, bound)
            } else if let Some(trait_key) = self.declaration_constructor_trait_key(bound) {
                matches!(
                    self.constructor_projection(&trait_key, &resolved)
                        .into_checked()
                        .map_err(|error| error.at_span(span))?,
                    ConstructorProjectionOutcome::Applicable { .. }
                )
            } else {
                self.ty_satisfies_bounds(&resolved, std::slice::from_ref(bound))?
            };
            if !satisfied {
                let actual_type = self.ty_name(&resolved);
                let error = TypeError::from_structured(diagnostics::StructuredDiagnostic {
                    reason: diagnostics::TypeDiagnosticReason::NominalDeclarationConstraintViolation
                        .into(),
                    origin: diagnostics::DiagnosticOrigin::Annotation,
                    data: diagnostics::DiagnosticData::Policy(diagnostics::PolicyData {
                        policy: diagnostics::TypePolicy::NominalDeclarationConstraint,
                        subject: Some(parameter.clone()),
                        expected_type: Some(self.trait_display_name(bound)),
                        actual_type: Some(actual_type.clone()),
                        stage: Some(Self::surface_name(&def.name).to_string()),
                        entrypoint: None,
                    }),
                    primary: diagnostics::SourceFact::typed(
                        diagnostics::SourceRole::Value,
                        diagnostics::SourceId(0),
                        span.clone(),
                        actual_type,
                    ),
                    related: Vec::new(),
                    remediation: None,
                })
                .with_hint(
                        "Use a constructor head with the declared capability, or add the same explicit bound to the rigid type variable."
                );
                return Err(error);
            }
        }
        Ok(())
    }

    pub(super) fn validate_nominal_type_well_formed(
        &mut self,
        ty: &Ty,
        span: &Span,
        defer_unresolved: bool,
    ) -> Result<(), TypeError> {
        let resolved = self.resolve_ty(ty);
        match &resolved {
            Ty::Struct(name, nominal) | Ty::Record(name, nominal) => {
                let def = self
                    .env
                    .lookup_type_def(name)
                    .cloned()
                    .ok_or_else(|| TypeError {
                        structured: None,
                        message: format!("Unknown nominal type: {}", Self::surface_name(name)),
                        span: span.clone(),
                        hint: None,
                    })?;
                self.validate_nominal_type_arguments(
                    &def,
                    &nominal.arguments,
                    span,
                    defer_unresolved,
                )?;
                for argument in &nominal.arguments {
                    self.validate_nominal_type_well_formed(argument, span, defer_unresolved)?;
                }
            }
            Ty::Enum(name, arguments) => {
                if let Some(def) = self.env.lookup_type_def(name).cloned() {
                    self.validate_nominal_type_arguments(&def, arguments, span, defer_unresolved)?;
                }
                for argument in arguments {
                    self.validate_nominal_type_well_formed(argument, span, defer_unresolved)?;
                }
            }
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner) => {
                self.validate_nominal_type_well_formed(inner, span, defer_unresolved)?;
            }
            Ty::Pid(marker) => self.constrain_pid_marker_type(marker, span, false)?,
            Ty::Result(ok, err) => {
                self.validate_nominal_type_well_formed(ok, span, defer_unresolved)?;
                self.validate_nominal_type_well_formed(err, span, defer_unresolved)?;
            }
            Ty::Tuple(items) | Ty::SelfApp(items) => {
                for item in items {
                    self.validate_nominal_type_well_formed(item, span, defer_unresolved)?;
                }
            }
            Ty::Func(params, ret) | Ty::BuiltinFunc { params, ret, .. } => {
                for param in params {
                    self.validate_nominal_type_well_formed(param, span, defer_unresolved)?;
                }
                self.validate_nominal_type_well_formed(ret, span, defer_unresolved)?;
            }
            Ty::UserFunc { params, ret, .. } => {
                for param in params {
                    self.validate_nominal_type_well_formed(param, span, defer_unresolved)?;
                }
                self.validate_nominal_type_well_formed(ret, span, defer_unresolved)?;
            }
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                self.validate_nominal_type_well_formed(source, span, defer_unresolved)?;
                self.validate_nominal_type_well_formed(focus, span, defer_unresolved)?;
                self.validate_nominal_type_well_formed(update_source, span, defer_unresolved)?;
                self.validate_nominal_type_well_formed(update_focus, span, defer_unresolved)?;
            }
            Ty::Var(_)
            | Ty::Hole
            | Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Error
            | Ty::ProcessMarker(_) => {}
        }
        Ok(())
    }

    pub(super) fn nominal_type_uses_capability(
        &self,
        ty: &Ty,
        subject: &Ty,
        capability: &str,
    ) -> bool {
        let ty = self.resolve_ty(ty);
        let subject = self.resolve_ty(subject);
        let argument_uses = |checker: &Self, name: &str, arguments: &[Ty]| {
            checker.env.lookup_type_def(name).is_some_and(|def| {
                def.type_param_bounds
                    .iter()
                    .zip(arguments)
                    .any(|(required, argument)| {
                        required.as_ref().is_some_and(|required| {
                            checker.resolve_ty(argument) == subject
                                && checker.trait_bound_entails(
                                    capability,
                                    required,
                                    &mut HashSet::new(),
                                )
                        })
                    })
            })
        };
        match &ty {
            Ty::Struct(name, nominal) | Ty::Record(name, nominal) => {
                argument_uses(self, name, &nominal.arguments)
                    || nominal.arguments.iter().any(|argument| {
                        self.nominal_type_uses_capability(argument, &subject, capability)
                    })
            }
            Ty::Enum(name, arguments) => {
                argument_uses(self, name, arguments)
                    || arguments.iter().any(|argument| {
                        self.nominal_type_uses_capability(argument, &subject, capability)
                    })
            }
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner)
            | Ty::Pid(inner) => self.nominal_type_uses_capability(inner, &subject, capability),
            Ty::Result(ok, err) => {
                self.nominal_type_uses_capability(ok, &subject, capability)
                    || self.nominal_type_uses_capability(err, &subject, capability)
            }
            Ty::Tuple(items) | Ty::SelfApp(items) => items
                .iter()
                .any(|item| self.nominal_type_uses_capability(item, &subject, capability)),
            Ty::Func(params, ret) | Ty::BuiltinFunc { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| self.nominal_type_uses_capability(param, &subject, capability))
                    || self.nominal_type_uses_capability(ret, &subject, capability)
            }
            Ty::UserFunc { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| self.nominal_type_uses_capability(param, &subject, capability))
                    || self.nominal_type_uses_capability(ret, &subject, capability)
            }
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                self.nominal_type_uses_capability(source, &subject, capability)
                    || self.nominal_type_uses_capability(focus, &subject, capability)
                    || self.nominal_type_uses_capability(update_source, &subject, capability)
                    || self.nominal_type_uses_capability(update_focus, &subject, capability)
            }
            Ty::Var(_)
            | Ty::Hole
            | Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Error
            | Ty::ProcessMarker(_) => false,
        }
    }

    pub(super) fn ty_contains_var(&self, ty: &Ty, needle: u32) -> bool {
        match self.resolve_ty(ty) {
            Ty::Var(var) => var == needle,
            Ty::Hole => false,
            Ty::MatchResult(inner) | Ty::ExtractorClosure(inner) | Ty::List(inner) => {
                self.ty_contains_var(&inner, needle)
            }
            Ty::Lazy(inner) => self.ty_contains_var(&inner, needle),
            Ty::Pid(inner) => self.ty_contains_var(&inner, needle),
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                self.ty_contains_var(&source, needle)
                    || self.ty_contains_var(&focus, needle)
                    || self.ty_contains_var(&update_source, needle)
                    || self.ty_contains_var(&update_focus, needle)
            }
            Ty::Tuple(items) | Ty::SelfApp(items) => {
                items.iter().any(|item| self.ty_contains_var(item, needle))
            }
            Ty::Func(params, ret) => {
                params
                    .iter()
                    .any(|param| self.ty_contains_var(param, needle))
                    || self.ty_contains_var(&ret, needle)
            }
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| self.ty_contains_var(param, needle))
                    || self.ty_contains_var(&ret, needle)
            }
            Ty::Result(ok, err) => {
                self.ty_contains_var(&ok, needle) || self.ty_contains_var(&err, needle)
            }
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => nominal
                .arguments
                .iter()
                .any(|argument| self.ty_contains_var(argument, needle)),
            Ty::Enum(_, args) => args.iter().any(|arg| self.ty_contains_var(arg, needle)),
            _ => false,
        }
    }

    pub(super) fn resolve_ty(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::Var(var) => match self.substitutions.get(var) {
                Some(bound) => self.resolve_ty(bound),
                None => Ty::Var(*var),
            },
            Ty::ExtractorClosure(inner) => Ty::ExtractorClosure(Box::new(self.resolve_ty(inner))),
            Ty::MatchResult(inner) => Ty::MatchResult(Box::new(self.resolve_ty(inner))),
            Ty::List(inner) => Ty::List(Box::new(self.resolve_ty(inner))),
            Ty::Hole => Ty::Hole,
            Ty::Lazy(inner) => Ty::Lazy(Box::new(self.resolve_ty(inner))),
            Ty::Pid(marker) => Ty::Pid(Box::new(self.resolve_ty(marker))),
            Ty::Facet(kind, source, focus, update_source, update_focus) => Ty::Facet(
                *kind,
                Box::new(self.resolve_ty(source)),
                Box::new(self.resolve_ty(focus)),
                Box::new(self.resolve_ty(update_source)),
                Box::new(self.resolve_ty(update_focus)),
            ),
            Ty::Tuple(items) => Ty::Tuple(items.iter().map(|item| self.resolve_ty(item)).collect()),
            Ty::SelfApp(items) => {
                let source_witness =
                    Self::constructor_application_parts(items).map(|(witness, _)| witness.clone());
                let mut resolved = items
                    .iter()
                    .map(|item| self.resolve_ty(item))
                    .collect::<Vec<_>>();
                if let Some((witness, slots)) = Self::constructor_application_parts(&resolved) {
                    if let Some(source) = source_witness.as_ref() {
                        if let ConstructorApplicationOutcome::Applied(applied) =
                            self.apply_constructor_application(source, witness, slots)
                        {
                            return self.resolve_ty(&applied);
                        }
                    }
                }
                // An unresolved application still needs its declaration's
                // mapped-slot metadata after call-site constraints arrive.
                if let Some(source) = source_witness.filter(|_| resolved.len() > 2) {
                    let shares_constructor_metadata = match (&source, &resolved[1]) {
                        (Ty::Var(source_var), Ty::Var(resolved_var)) => self
                            .constructor_witness_traits
                            .get(source_var)
                            .zip(self.constructor_witness_traits.get(resolved_var))
                            .is_some_and(|(source_trait, resolved_trait)| {
                                self.constructor_traits_share_mapping(source_trait, resolved_trait)
                            }),
                        _ => false,
                    };
                    if !shares_constructor_metadata {
                        resolved[1] = source;
                    }
                }
                Ty::SelfApp(resolved)
            }
            Ty::Func(params, ret) => Ty::Func(
                params.iter().map(|param| self.resolve_ty(param)).collect(),
                Box::new(self.resolve_ty(ret)),
            ),
            Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
                name: name.clone(),
                params: params.iter().map(|param| self.resolve_ty(param)).collect(),
                ret: Box::new(self.resolve_ty(ret)),
            },
            Ty::UserFunc {
                fun_idx,
                type_params,
                call_substitution,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx: *fun_idx,
                type_params: type_params.clone(),
                call_substitution: call_substitution
                    .iter()
                    .map(|(var, ty)| (*var, self.resolve_ty(ty)))
                    .collect(),
                params: params.iter().map(|param| self.resolve_ty(param)).collect(),
                ret: Box::new(self.resolve_ty(ret)),
            },
            Ty::Struct(name, fields) => Ty::Struct(
                name.clone(),
                NominalType::new(
                    fields
                        .arguments
                        .iter()
                        .map(|argument| self.resolve_ty(argument))
                        .collect(),
                ),
            ),
            Ty::Record(name, fields) => Ty::Record(
                name.clone(),
                NominalType::new(
                    fields
                        .arguments
                        .iter()
                        .map(|argument| self.resolve_ty(argument))
                        .collect(),
                ),
            ),
            Ty::Enum(name, args) => Ty::Enum(
                name.clone(),
                args.iter().map(|arg| self.resolve_ty(arg)).collect(),
            ),
            Ty::Result(ok, err) => Ty::Result(
                Box::new(self.resolve_ty(ok)),
                Box::new(self.resolve_ty(err)),
            ),
            other => other.clone(),
        }
    }

    pub(super) fn instantiate_ty_with_fresh(
        &mut self,
        ty: &Ty,
        fresh: &mut HashMap<u32, Ty>,
    ) -> Ty {
        let profile = self.profiler.start();
        let result = match ty {
            Ty::Var(var) => {
                if let Some(instantiated) = fresh.get(var) {
                    instantiated.clone()
                } else {
                    let instantiated = self.env.fresh_tyvar();
                    if let Ty::Var(new_var) = instantiated {
                        self.inherit_error_slot_variable(*var, new_var);
                        let bounds = self.tyvar_bound_names(*var);
                        self.register_tyvar_bounds(new_var, &bounds);
                        if self.process_marker_tyvars.contains(var) {
                            self.process_marker_tyvars.insert(new_var);
                        }
                        if let Some(trait_key) = self.constructor_witness_traits.get(var).cloned() {
                            self.constructor_witness_traits.insert(new_var, trait_key);
                        }
                    }
                    fresh.insert(*var, instantiated.clone());
                    instantiated
                }
            }
            Ty::ExtractorClosure(inner) => {
                Ty::ExtractorClosure(Box::new(self.instantiate_ty_with_fresh(inner, fresh)))
            }
            Ty::MatchResult(inner) => {
                Ty::MatchResult(Box::new(self.instantiate_ty_with_fresh(inner, fresh)))
            }
            Ty::List(inner) => Ty::List(Box::new(self.instantiate_ty_with_fresh(inner, fresh))),
            Ty::Hole => Ty::Hole,
            Ty::Lazy(inner) => Ty::Lazy(Box::new(self.instantiate_ty_with_fresh(inner, fresh))),
            Ty::Pid(marker) => Ty::Pid(Box::new(self.instantiate_ty_with_fresh(marker, fresh))),
            Ty::Facet(kind, source, focus, update_source, update_focus) => Ty::Facet(
                *kind,
                Box::new(self.instantiate_ty_with_fresh(source, fresh)),
                Box::new(self.instantiate_ty_with_fresh(focus, fresh)),
                Box::new(self.instantiate_ty_with_fresh(update_source, fresh)),
                Box::new(self.instantiate_ty_with_fresh(update_focus, fresh)),
            ),
            Ty::Tuple(items) => Ty::Tuple(
                items
                    .iter()
                    .map(|item| self.instantiate_ty_with_fresh(item, fresh))
                    .collect(),
            ),
            Ty::SelfApp(items) => Ty::SelfApp(
                items
                    .iter()
                    .map(|item| self.instantiate_ty_with_fresh(item, fresh))
                    .collect(),
            ),
            Ty::Func(params, ret) => Ty::Func(
                params
                    .iter()
                    .map(|param| self.instantiate_ty_with_fresh(param, fresh))
                    .collect(),
                Box::new(self.instantiate_ty_with_fresh(ret, fresh)),
            ),
            Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
                name: name.clone(),
                params: params
                    .iter()
                    .map(|param| self.instantiate_ty_with_fresh(param, fresh))
                    .collect(),
                ret: Box::new(self.instantiate_ty_with_fresh(ret, fresh)),
            },
            Ty::UserFunc {
                fun_idx,
                type_params,
                call_substitution,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx: *fun_idx,
                type_params: type_params.clone(),
                call_substitution: call_substitution
                    .iter()
                    .map(|(var, ty)| (*var, self.instantiate_ty_with_fresh(ty, fresh)))
                    .collect(),
                params: params
                    .iter()
                    .map(|param| self.instantiate_ty_with_fresh(param, fresh))
                    .collect(),
                ret: Box::new(self.instantiate_ty_with_fresh(ret, fresh)),
            },
            Ty::Struct(name, fields) => Ty::Struct(
                name.clone(),
                NominalType::new(
                    fields
                        .arguments
                        .iter()
                        .map(|argument| self.instantiate_ty_with_fresh(argument, fresh))
                        .collect(),
                ),
            ),
            Ty::Record(name, fields) => Ty::Record(
                name.clone(),
                NominalType::new(
                    fields
                        .arguments
                        .iter()
                        .map(|argument| self.instantiate_ty_with_fresh(argument, fresh))
                        .collect(),
                ),
            ),
            Ty::Enum(name, args) => Ty::Enum(
                name.clone(),
                args.iter()
                    .map(|arg| self.instantiate_ty_with_fresh(arg, fresh))
                    .collect(),
            ),
            Ty::Result(ok, err) => Ty::Result(
                Box::new(self.instantiate_ty_with_fresh(ok, fresh)),
                Box::new(self.instantiate_ty_with_fresh(err, fresh)),
            ),
            other => other.clone(),
        };
        self.profiler
            .finish(ProfileEvent::InstantiateTyWithFresh, profile);
        result
    }

    /// Instantiates declaration-owned callable generics for one use site.
    /// Every lookup gets an independent fresh map so substitutions cannot
    /// leak between calls to the same function.
    pub(super) fn instantiate_callable_ty(&mut self, ty: &Ty) -> Ty {
        let mut fresh = HashMap::new();
        self.instantiate_ty_with_fresh(ty, &mut fresh)
    }

    pub(super) fn substitute_type_def_ty(&self, ty: &Ty, bindings: &HashMap<u32, Ty>) -> Ty {
        match ty {
            Ty::Var(var) => bindings.get(var).cloned().unwrap_or(Ty::Var(*var)),
            Ty::ExtractorClosure(inner) => {
                Ty::ExtractorClosure(Box::new(self.substitute_type_def_ty(inner, bindings)))
            }
            Ty::MatchResult(inner) => {
                Ty::MatchResult(Box::new(self.substitute_type_def_ty(inner, bindings)))
            }
            Ty::List(inner) => Ty::List(Box::new(self.substitute_type_def_ty(inner, bindings))),
            Ty::Hole => Ty::Hole,
            Ty::Lazy(inner) => Ty::Lazy(Box::new(self.substitute_type_def_ty(inner, bindings))),
            Ty::Pid(marker) => Ty::Pid(Box::new(self.substitute_type_def_ty(marker, bindings))),
            Ty::Facet(kind, source, focus, update_source, update_focus) => Ty::Facet(
                *kind,
                Box::new(self.substitute_type_def_ty(source, bindings)),
                Box::new(self.substitute_type_def_ty(focus, bindings)),
                Box::new(self.substitute_type_def_ty(update_source, bindings)),
                Box::new(self.substitute_type_def_ty(update_focus, bindings)),
            ),
            Ty::Tuple(items) => Ty::Tuple(
                items
                    .iter()
                    .map(|item| self.substitute_type_def_ty(item, bindings))
                    .collect(),
            ),
            Ty::SelfApp(items) => {
                let source_witness =
                    Self::constructor_application_parts(items).map(|(witness, _)| witness.clone());
                let substituted = items
                    .iter()
                    .map(|item| self.substitute_type_def_ty(item, bindings))
                    .collect::<Vec<_>>();
                if let Some((witness, slots)) = Self::constructor_application_parts(&substituted) {
                    if let Some(source) = source_witness.as_ref() {
                        if let ConstructorApplicationOutcome::Applied(applied) =
                            self.apply_constructor_application(source, witness, slots)
                        {
                            return applied;
                        }
                    }
                }
                Ty::SelfApp(substituted)
            }
            Ty::Func(params, ret) => Ty::Func(
                params
                    .iter()
                    .map(|param| self.substitute_type_def_ty(param, bindings))
                    .collect(),
                Box::new(self.substitute_type_def_ty(ret, bindings)),
            ),
            Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
                name: name.clone(),
                params: params
                    .iter()
                    .map(|param| self.substitute_type_def_ty(param, bindings))
                    .collect(),
                ret: Box::new(self.substitute_type_def_ty(ret, bindings)),
            },
            Ty::UserFunc {
                fun_idx,
                type_params,
                call_substitution,
                params,
                ret,
            } => Ty::UserFunc {
                fun_idx: *fun_idx,
                type_params: type_params.clone(),
                call_substitution: call_substitution
                    .iter()
                    .map(|(var, ty)| (*var, self.substitute_type_def_ty(ty, bindings)))
                    .collect(),
                params: params
                    .iter()
                    .map(|param| self.substitute_type_def_ty(param, bindings))
                    .collect(),
                ret: Box::new(self.substitute_type_def_ty(ret, bindings)),
            },
            Ty::Struct(name, fields) => Ty::Struct(
                name.clone(),
                NominalType::new(
                    fields
                        .arguments
                        .iter()
                        .map(|argument| self.substitute_type_def_ty(argument, bindings))
                        .collect(),
                ),
            ),
            Ty::Record(name, fields) => Ty::Record(
                name.clone(),
                NominalType::new(
                    fields
                        .arguments
                        .iter()
                        .map(|argument| self.substitute_type_def_ty(argument, bindings))
                        .collect(),
                ),
            ),
            Ty::Enum(name, args) => Ty::Enum(
                name.clone(),
                args.iter()
                    .map(|arg| self.substitute_type_def_ty(arg, bindings))
                    .collect(),
            ),
            Ty::Result(ok, err) => Ty::Result(
                Box::new(self.substitute_type_def_ty(ok, bindings)),
                Box::new(self.substitute_type_def_ty(err, bindings)),
            ),
            other => other.clone(),
        }
    }

    pub(super) fn nominal_fields(
        &self,
        name: &str,
        nominal: &NominalType,
        span: &Span,
    ) -> Result<Vec<(String, Ty)>, TypeError> {
        let definition = self.env.lookup_type_def(name).ok_or_else(|| {
            self.typecheck_invariant_error("nominal application has no declaration", span)
        })?;
        if definition.state != crate::env::TypeDefState::SignatureResolved {
            return Err(self.typecheck_invariant_error(
                "nominal field access requires a resolved declaration schema",
                span,
            ));
        }
        if definition.type_param_vars.len() != nominal.arguments.len() {
            return Err(self.typecheck_invariant_error(
                "nominal application has inconsistent type argument arity",
                span,
            ));
        }
        Ok(self.instantiate_type_def_fields(definition, &nominal.arguments))
    }

    fn instantiate_type_def_fields(
        &self,
        def: &crate::env::TypeDefInfo,
        args: &[Ty],
    ) -> Vec<(String, Ty)> {
        let bindings = def
            .type_param_vars
            .iter()
            .copied()
            .zip(args.iter().cloned())
            .collect::<HashMap<_, _>>();
        def.fields
            .iter()
            .map(|(field, field_ty)| {
                (
                    field.clone(),
                    self.substitute_type_def_ty(field_ty, &bindings),
                )
            })
            .collect()
    }

    pub(super) fn instantiate_enum_variant(
        &mut self,
        variant: &crate::env::EnumVariantInfo,
    ) -> crate::env::EnumVariantInfo {
        let profile = self.profiler.start();
        let mut fresh = HashMap::new();
        let instantiated = crate::env::EnumVariantInfo {
            enum_error: variant.enum_error,
            error_identity: variant.error_identity.clone(),
            special_variant: variant.special_variant,
            constructor_name: variant.constructor_name.clone(),
            short_name: variant.short_name.clone(),
            enum_name: variant.enum_name.clone(),
            enum_ty: self.instantiate_ty_with_fresh(&variant.enum_ty, &mut fresh),
            tag: variant.tag,
            payload: variant
                .payload
                .iter()
                .map(|ty| self.instantiate_ty_with_fresh(ty, &mut fresh))
                .collect(),
            payload_type_spans: variant.payload_type_spans.clone(),
            discriminant: variant.discriminant.clone(),
        };
        self.profiler
            .finish(ProfileEvent::InstantiateEnumVariant, profile);
        instantiated
    }

    pub(super) fn ty_contains_facet(&self, ty: &Ty) -> bool {
        match self.resolve_ty(ty) {
            Ty::Facet(..) => true,
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner)
            | Ty::Pid(inner) => self.ty_contains_facet(inner.as_ref()),
            Ty::Tuple(items) | Ty::SelfApp(items) => {
                items.iter().any(|item| self.ty_contains_facet(item))
            }
            Ty::Func(params, ret) => {
                params.iter().any(|param| self.ty_contains_facet(param))
                    || self.ty_contains_facet(ret.as_ref())
            }
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                params.iter().any(|param| self.ty_contains_facet(param))
                    || self.ty_contains_facet(ret.as_ref())
            }
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => nominal
                .arguments
                .iter()
                .any(|argument| self.ty_contains_facet(argument)),
            Ty::Enum(_, args) => args.iter().any(|arg| self.ty_contains_facet(arg)),
            Ty::Result(ok, err) => {
                self.ty_contains_facet(ok.as_ref()) || self.ty_contains_facet(err.as_ref())
            }
            Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Var(_)
            | Ty::Error
            | Ty::Hole
            | Ty::ProcessMarker(_) => false,
        }
    }
}

impl Checker {
    pub(super) fn lookup_enum_variant_by_constructor_id(
        &self,
        unique_id: u32,
    ) -> Option<crate::env::EnumVariantInfo> {
        let profile = self.profiler.start();
        let variant = self.env.enum_variant_by_constructor_id(unique_id).cloned();
        self.profiler
            .finish(ProfileEvent::EnumVariantCtorLookup, profile);
        variant
    }

    pub(super) fn lookup_enum_variants_of<'a>(
        &'a self,
        enum_name: &str,
    ) -> Option<&'a Vec<crate::env::EnumVariantInfo>> {
        let profile = self.profiler.start();
        let variants = self.env.enum_variants_of(enum_name);
        self.profiler
            .finish(ProfileEvent::EnumVariantsLookup, profile);
        variants
    }

    pub(super) fn lookup_enum_variant_by_short_name(
        &self,
        enum_name: &str,
        short_name: &str,
    ) -> Option<crate::env::EnumVariantInfo> {
        let profile = self.profiler.start();
        let variant = self
            .env
            .enum_variants_of(enum_name)
            .and_then(|variants| {
                variants
                    .iter()
                    .find(|candidate| candidate.short_name == short_name)
            })
            .cloned();
        self.profiler
            .finish(ProfileEvent::EnumVariantSelectorLookup, profile);
        variant
    }
}
