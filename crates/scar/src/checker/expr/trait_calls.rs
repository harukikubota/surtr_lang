use super::*;
use diagnostics::{DiagnosticOrigin, SourceRole, TypeDiagnosticReason};

#[derive(Clone)]
pub(super) struct TraitInvocationContext<'a> {
    pub(super) span: &'a Span,
    pub(super) trait_name: &'a str,
    pub(super) method_name: &'a str,
    pub(super) args: &'a [ResolvedRecordLitArg],
    pub(super) receiver_owner_hint: Option<&'a str>,
    pub(super) expected_ret_ty: Option<&'a Ty>,
    pub(super) expected_ret_relation: Option<(&'a ExpectedTypeRelation, &'a Span)>,
    pub(super) argument_expected_relation: Option<&'a ExpectedTypeRelation>,
    pub(super) constructor_failure_reason: Option<TypeDiagnosticReason>,
    pub(super) explicit_type_args: Option<&'a [AstTy]>,
    pub(super) operator: Option<OperatorTraitOp>,
    pub(super) receiver_hint: Option<&'a Ty>,
    pub(super) defer_incompatible_result_context: bool,
}

pub(super) struct PreparedTraitInvocation {
    param_tys: Vec<Ty>,
    ret_ty: Ty,
    trait_arg_tys: Vec<Ty>,
    self_ty: Ty,
    declared_param_tys: Vec<Ty>,
    constructor_receiver_index: Option<usize>,
    prepared_args: Vec<Option<TypedNode>>,
}

pub(super) enum TraitInvocationPreparation {
    Complete(TypedNode),
    Arguments(PreparedTraitInvocation),
}

impl Checker {
    pub(in crate::checker) fn trait_obligations_depend_on(
        &self,
        node: &TypedNode,
        inputs: &[Ty],
    ) -> bool {
        let mut permitted = Vec::new();
        for input in inputs {
            Self::collect_ty_vars(&self.resolve_ty(input), &mut permitted);
        }
        let mut waiting = Vec::new();
        for obligation in Self::full_trait_obligations(node) {
            for ty in obligation
                .trait_args
                .iter()
                .chain(std::iter::once(&obligation.receiver))
            {
                Self::collect_ty_vars(&self.resolve_ty(ty), &mut waiting);
            }
        }
        !waiting.is_empty() && waiting.iter().all(|var| permitted.contains(var))
    }

    pub(super) fn local_callable_obligations_depend_on(
        &self,
        node: &TypedNode,
        inputs: &[Ty],
    ) -> bool {
        Self::full_trait_obligations(node)
            .iter()
            .all(|obligation| !matches!(self.resolve_ty(&obligation.receiver), Ty::Var(_)))
            && self.trait_obligations_depend_on(node, inputs)
    }

    pub(in crate::checker) fn validate_pending_body_dependencies(
        &self,
        body: &TypedNode,
        inputs: &[Ty],
    ) -> Result<(), TypeError> {
        if self.trait_obligations_depend_on(body, inputs) {
            return Ok(());
        }
        if let Some((trait_name, method, subject, span)) = self.first_pending_trait_helper(body) {
            return Err(self.pending_trait_helper_error(trait_name, method, subject, span));
        }
        Ok(())
    }

    pub(in crate::checker) fn pending_trait_helper_error(
        &self,
        trait_name: &str,
        method_name: &str,
        subject: &Ty,
        span: &Span,
    ) -> TypeError {
        self.trait_dispatch_failure(TypeDiagnosticReason::UnresolvedTraitMethodInstantiation, trait_name, method_name, Some(subject), span)
            .with_hint("Add parameter or binding type annotations, or pass the callable where an expected function type is available.")
    }

    fn check_trait_method_constructor_capabilities(
        &self,
        trait_name: &str,
        method_name: &str,
        param_tys: &[Ty],
        receiver_index: Option<usize>,
        args: &[TypedNode],
        failure_reason: Option<TypeDiagnosticReason>,
    ) -> Result<(), TypeError> {
        for (index, (expected, arg)) in param_tys.iter().zip(args).enumerate() {
            if receiver_index == Some(index) {
                self.check_constructor_capability(trait_name, method_name, arg, failure_reason)?;
                continue;
            }
            if !matches!(expected, Ty::SelfApp(items)
                if Self::constructor_application_parts(items).is_some())
            {
                continue;
            }
            let required_trait = self
                .constructor_capability_for_type(expected)
                .unwrap_or_else(|| trait_name.to_string());
            self.check_constructor_capability(&required_trait, method_name, arg, failure_reason)?;
        }
        Ok(())
    }

    pub(in crate::checker) fn concretize_pending_trait_calls(
        &mut self,
        mut node: TypedNode,
    ) -> Result<Box<TypedNode>, Box<TypeError>> {
        self.concretize_pending_trait_calls_in_place(&mut node)?;
        Ok(Box::new(node))
    }

    // Borrow children in place: rebuilding the large enum at every level keeps
    // all variant temporaries live on the stack of long continuation chains.
    fn concretize_pending_trait_calls_in_place(
        &mut self,
        node: &mut TypedNode,
    ) -> Result<(), Box<TypeError>> {
        for expr in Self::pattern_expression_nodes_mut(node) {
            self.concretize_pending_trait_calls_in_place(expr)?;
        }
        match &mut node.node {
            TypedInner::ErrorConstruct {
                message, payload, ..
            } => {
                self.concretize_pending_trait_calls_in_place(message)?;
                for value in payload {
                    self.concretize_pending_trait_calls_in_place(value)?;
                }
            }
            TypedInner::SafeBind(
                _,
                _,
                _,
                SafeBindFailureTarget::EnclosingMonadFail(target)
                | SafeBindFailureTarget::DoMonadFail(target),
            ) => self.concretize_pending_trait_calls_in_place(&mut target.call)?,
            TypedInner::DoSafeBind(control) => {
                if let SafeBindFailureTarget::DoMonadFail(target) = &mut control.failure_target {
                    self.concretize_pending_trait_calls_in_place(&mut target.call)?;
                }
            }
            _ => {}
        }
        node.ty = self.resolve_ty(&node.ty);
        match &mut node.node {
            TypedInner::TraitCall {
                receiver_ty,
                obligation,
                args,
                dispatch,
                method_name,
                ..
            } => {
                *receiver_ty = self.resolve_ty(receiver_ty);
                obligation.trait_args = obligation
                    .trait_args
                    .iter()
                    .map(|ty| self.resolve_ty(ty))
                    .collect();
                obligation.receiver = self.resolve_ty(&obligation.receiver);
                for arg in args.iter_mut() {
                    self.concretize_pending_trait_calls_in_place(arg)?;
                }
                self.concretize_trait_dispatch(
                    &node.span,
                    &node.ty,
                    method_name,
                    obligation,
                    args,
                    dispatch,
                )?;
            }
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                self.concretize_pending_trait_calls_in_place(func)?;
                for arg in args {
                    self.concretize_pending_trait_calls_in_place(arg)?;
                }
            }
            TypedInner::Block(items)
            | TypedInner::TupleLiteral(items)
            | TypedInner::ListLiteral(items)
            | TypedInner::ConstructorCall(_, items)
            | TypedInner::StructLit(_, items) => {
                for item in items {
                    self.concretize_pending_trait_calls_in_place(item)?;
                }
            }
            TypedInner::Bind(_, rhs)
            | TypedInner::ApplyPattern { value: rhs, .. }
            | TypedInner::SafeBind(_, rhs, _, _)
            | TypedInner::EagerBoundary(rhs)
            | TypedInner::FieldAccess(rhs, _)
            | TypedInner::Semi(rhs)
            | TypedInner::SupervisorSpawn { init: rhs, .. }
            | TypedInner::SupervisorAdopt { pid: rhs, .. }
            | TypedInner::FacetView { source: rhs, .. }
            | TypedInner::Closure(_, _, rhs)
            | TypedInner::ExtractorClosure(_, _, rhs)
            | TypedInner::CaptureClosure(_, _, rhs)
            | TypedInner::DeferrorDef(_, _, _, _, rhs)
            | TypedInner::Def(_, _, _, _, _, _, rhs, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, rhs, _) => {
                self.concretize_pending_trait_calls_in_place(rhs)?;
            }
            TypedInner::DoSafeBind(control) => {
                self.concretize_pending_trait_calls_in_place(&mut control.rhs)?;
                match &mut control.failure_target {
                    SafeBindFailureTarget::DoAlternative { empty } => {
                        self.concretize_pending_trait_calls_in_place(empty)?
                    }
                    SafeBindFailureTarget::Deferred(deferred) => {
                        deferred.carrier_ty = self.resolve_ty(&deferred.carrier_ty);
                        deferred.propagated_error_tys = deferred
                            .propagated_error_tys
                            .iter()
                            .map(|ty| self.resolve_ty(ty))
                            .collect();
                    }
                    _ => {}
                }
                self.concretize_pending_trait_calls_in_place(&mut control.continuation)?;
            }
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right)
            | TypedInner::Cause(left, right)
            | TypedInner::SupervisorWorkers {
                init: left,
                strategy: right,
                ..
            }
            | TypedInner::FacetSet {
                source: left,
                value: right,
                ..
            }
            | TypedInner::FacetOver {
                source: left,
                update_fun: right,
                ..
            } => {
                self.concretize_pending_trait_calls_in_place(left)?;
                self.concretize_pending_trait_calls_in_place(right)?;
            }
            TypedInner::HashMapLiteral(entries) => {
                for (key, value) in entries {
                    self.concretize_pending_trait_calls_in_place(key)?;
                    self.concretize_pending_trait_calls_in_place(value)?;
                }
            }
            TypedInner::InterpolatedStr(parts) => {
                for part in parts {
                    if let crate::typed::TypedInterpolatedPart::Expr(expr) = part {
                        self.concretize_pending_trait_calls_in_place(expr)?;
                    }
                }
            }
            TypedInner::Dbg(args) => {
                for arg in args {
                    self.concretize_pending_trait_calls_in_place(&mut arg.expr)?;
                }
            }
            TypedInner::If(condition, then_branch, else_branch) => {
                self.concretize_pending_trait_calls_in_place(condition)?;
                self.concretize_pending_trait_calls_in_place(then_branch)?;
                if let Some(branch) = else_branch {
                    self.concretize_pending_trait_calls_in_place(branch)?;
                }
            }
            TypedInner::Match(scrutinee, arms) => {
                self.concretize_pending_trait_calls_in_place(scrutinee)?;
                for arm in arms {
                    if let Some(guard) = &mut arm.guard {
                        self.concretize_pending_trait_calls_in_place(guard)?;
                    }
                    self.concretize_pending_trait_calls_in_place(&mut arm.body)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn concretize_trait_dispatch(
        &mut self,
        span: &Span,
        ty: &Ty,
        method_name: &str,
        obligation: &TraitObligation,
        args: &[TypedNode],
        dispatch: &mut TraitDispatch,
    ) -> Result<(), Box<TypeError>> {
        let abstract_constructor_receiver = match &obligation.receiver {
            Ty::SelfApp(items) => Self::constructor_application_parts(items)
                .map(|(witness, _)| {
                    self.constructor_witness_supports_trait(witness, &obligation.trait_id)
                })
                .unwrap_or(false),
            _ => false,
        };
        let mut unresolved = Vec::new();
        for ty in obligation
            .trait_args
            .iter()
            .chain(std::iter::once(&obligation.receiver))
        {
            Self::collect_ty_vars(ty, &mut unresolved);
        }
        if matches!(dispatch, TraitDispatch::Pending)
            && unresolved.is_empty()
            && !abstract_constructor_receiver
        {
            let argument_tys = args
                .iter()
                .map(|arg| self.resolve_ty(&arg.ty))
                .collect::<Vec<_>>();
            *dispatch = match self.select_trait_method_instantiation(
                &obligation.trait_id,
                method_name,
                &obligation.receiver,
                &obligation.trait_args,
                &argument_tys,
                ty,
            )? {
                CandidateApplicability::Applicable(instantiation) => {
                    TraitDispatch::Selected(Box::new(instantiation))
                }
                CandidateApplicability::Deferred(_) => TraitDispatch::Pending,
                CandidateApplicability::Rejected(_) => {
                    return Err(Box::new(self.trait_failure(
                        TypeDiagnosticReason::NoApplicableTraitImplementation,
                        &obligation.trait_id,
                        &obligation.receiver,
                        span,
                        DiagnosticOrigin::TraitCall,
                    )));
                }
            };
        }
        Ok(())
    }

    pub(in crate::checker) fn trait_dispatch_target_for_args(
        &mut self,
        trait_name: &str,
        _method_name: &str,
        receiver_ty: &Ty,
        requested_trait_args: &[Ty],
    ) -> Result<Option<TraitDispatch>, TypeError> {
        let receiver_ty = self.resolve_ty(receiver_ty);
        Ok(
            match self.probe_trait_head(trait_name, requested_trait_args, &receiver_ty)? {
                ApplicabilityProof::Satisfied(evidence) => {
                    for index in evidence {
                        self.active_capabilities[index].consumed = true;
                    }
                    Some(TraitDispatch::Pending)
                }
                ApplicabilityProof::Deferred(waiting_on) => {
                    for var in waiting_on {
                        let pending = self.pending_trait_obligations.entry(var).or_default();
                        let obligation = PendingTraitObligation {
                            trait_id: trait_name.into(),
                            args: requested_trait_args.to_vec(),
                            receiver: receiver_ty.clone(),
                        };
                        if !pending.contains(&obligation) {
                            pending.push(obligation);
                        }
                    }
                    Some(TraitDispatch::Pending)
                }
                ApplicabilityProof::Unsatisfied => None,
            },
        )
    }

    fn constructor_target_dispatch(
        &mut self,
        trait_name: &str,
        method_name: &str,
        expected_ty: &Ty,
    ) -> Result<Option<TraitDispatch>, TypeError> {
        if let Ty::Var(var) = self.resolve_ty(expected_ty) {
            return Ok(
                if !self.rigid_tyvars.contains(&var)
                    || self.tyvar_has_bound(var, trait_name)
                    || self.tyvar_satisfies_compiler_trait(var, trait_name)
                {
                    Some(TraitDispatch::Pending)
                } else {
                    None
                },
            );
        }
        let expected_ty = self.resolve_ty(expected_ty);
        if let Ty::SelfApp(items) = &expected_ty {
            if let Some((witness, _)) = Self::constructor_application_parts(items) {
                return Ok(match self.resolve_ty(witness) {
                    Ty::Var(_) if self.constructor_witness_supports_trait(witness, trait_name) => {
                        Some(TraitDispatch::Pending)
                    }
                    _ => None,
                });
            }
            // A plain Self application is the declaration-owned abstract
            // carrier used while checking a generic constructor helper body.
            // Its dispatch must remain pending until the enclosing call site
            // supplies a concrete carrier.
            return Ok(Some(TraitDispatch::Pending));
        }
        self.select_method_dispatch(
            trait_name,
            method_name,
            &expected_ty,
            &[],
            &[],
            &expected_ty,
        )
    }

    fn constructor_witness_supports_trait(&mut self, witness: &Ty, trait_name: &str) -> bool {
        let Ty::Var(var) = self.resolve_ty(witness) else {
            return false;
        };
        if self.tyvar_has_bound(var, trait_name)
            || self.tyvar_satisfies_compiler_trait(var, trait_name)
        {
            return true;
        }
        let Some(source_trait) = self.constructor_witness_traits.get(&var).cloned() else {
            return false;
        };
        self.constructor_capability_allows(&source_trait, trait_name, &mut HashSet::new())
    }

    pub(super) fn constructor_slot_type_for(
        &mut self,
        trait_name: &str,
        container_ty: &Ty,
    ) -> ConstructorApplicationOutcome {
        if let Ty::SelfApp(items) = self.resolve_ty(container_ty) {
            return match Self::constructor_application_parts(&items) {
                Some((witness, [slot]))
                    if self.constructor_witness_supports_trait(witness, trait_name) =>
                {
                    ConstructorApplicationOutcome::Applied(slot.clone())
                }
                Some((_, slots)) if slots.len() != 1 => ConstructorApplicationOutcome::Rejected {
                    failures: vec![ConstructorProjectionFailure::SlotCountMismatch {
                        expected: 1,
                        actual: slots.len(),
                    }],
                },
                Some(_) => ConstructorApplicationOutcome::Rejected {
                    failures: vec![ConstructorProjectionFailure::NoApplicableImplementation],
                },
                None => ConstructorApplicationOutcome::Rejected {
                    failures: vec![ConstructorProjectionFailure::UnsupportedConstructor],
                },
            };
        }
        match self.constructor_projection(trait_name, container_ty) {
            ConstructorProjectionOutcome::Applicable { info, mapping } => {
                let [slot] = info.constructor_slot_vars.as_slice() else {
                    return ConstructorApplicationOutcome::Rejected {
                        failures: vec![ConstructorProjectionFailure::SlotCountMismatch {
                            expected: 1,
                            actual: info.constructor_slot_vars.len(),
                        }],
                    };
                };
                match mapping.get(slot).cloned() {
                    Some(slot) => ConstructorApplicationOutcome::Applied(slot),
                    None => ConstructorApplicationOutcome::Rejected {
                        failures: vec![
                            ConstructorProjectionFailure::MissingConstructorSlotMapping {
                                variable: *slot,
                            },
                        ],
                    },
                }
            }
            ConstructorProjectionOutcome::Deferred { waiting_on } => {
                ConstructorApplicationOutcome::Deferred { waiting_on }
            }
            ConstructorProjectionOutcome::Rejected { failures } => {
                ConstructorApplicationOutcome::Rejected { failures }
            }
        }
    }

    pub(super) fn constructor_context_type_for(
        &mut self,
        trait_name: &str,
        container_ty: &Ty,
        slot_ty: &Ty,
    ) -> ConstructorApplicationOutcome {
        if let Ty::SelfApp(items) = self.resolve_ty(container_ty) {
            return match Self::constructor_application_parts(&items) {
                Some((witness, [_]))
                    if self.constructor_witness_supports_trait(witness, trait_name) =>
                {
                    ConstructorApplicationOutcome::Applied(Ty::SelfApp(vec![
                        Ty::Hole,
                        witness.clone(),
                        slot_ty.clone(),
                    ]))
                }
                Some((_, slots)) if slots.len() != 1 => ConstructorApplicationOutcome::Rejected {
                    failures: vec![ConstructorProjectionFailure::SlotCountMismatch {
                        expected: 1,
                        actual: slots.len(),
                    }],
                },
                Some(_) => ConstructorApplicationOutcome::Rejected {
                    failures: vec![ConstructorProjectionFailure::NoApplicableImplementation],
                },
                None => ConstructorApplicationOutcome::Rejected {
                    failures: vec![ConstructorProjectionFailure::UnsupportedConstructor],
                },
            };
        }
        match self.constructor_projection(trait_name, container_ty) {
            ConstructorProjectionOutcome::Applicable { info, mut mapping } => {
                let [slot] = info.constructor_slot_vars.as_slice() else {
                    return ConstructorApplicationOutcome::Rejected {
                        failures: vec![ConstructorProjectionFailure::SlotCountMismatch {
                            expected: 1,
                            actual: info.constructor_slot_vars.len(),
                        }],
                    };
                };
                mapping.insert(*slot, slot_ty.clone());
                ConstructorApplicationOutcome::Applied(
                    self.resolve_ty(&self.substitute_ty_with_mapping(&info.target_ty, &mapping)),
                )
            }
            ConstructorProjectionOutcome::Deferred { waiting_on } => {
                ConstructorApplicationOutcome::Deferred { waiting_on }
            }
            ConstructorProjectionOutcome::Rejected { failures } => {
                ConstructorApplicationOutcome::Rejected { failures }
            }
        }
    }

    pub(in crate::checker) fn require_constructor_projection_type(
        &self,
        outcome: ConstructorApplicationOutcome,
        trait_name: &str,
        subject: &Ty,
        span: &Span,
        operation: &str,
    ) -> Result<Ty, TypeError> {
        match outcome
            .into_checked()
            .map_err(|error| error.at_span(span))?
        {
            ConstructorApplicationOutcome::Applied(ty) => Ok(ty),
            ConstructorApplicationOutcome::Deferred { waiting_on } => {
                let mut error = self.ambiguous_constructor_result(trait_name, operation, span);
                error.hint = Some(format!(
                    "Resolve all {} constructor input(s) before using this operation.",
                    waiting_on.len()
                ));
                Err(error)
            }
            ConstructorApplicationOutcome::Rejected { failures }
                if Self::constructor_projection_failures_are_metadata(&failures) =>
            {
                Err(signatures::constructor_signature_metadata_error(
                    operation,
                    span,
                    Self::constructor_projection_failure_detail(&failures),
                ))
            }
            ConstructorApplicationOutcome::Rejected { .. } => Err(self.trait_failure(
                TypeDiagnosticReason::NoApplicableTraitImplementation,
                trait_name,
                subject,
                span,
                DiagnosticOrigin::TraitCall,
            )),
        }
    }

    /// Enumerate the concrete constructor contexts which can carry `slot_ty`.
    /// This is used when a constructor helper appears on the opposite side of
    /// an operator and the other operand must provide the missing `Self`.
    pub(super) fn constructor_context_candidates(
        &mut self,
        trait_name: &str,
        slot_ty: &Ty,
    ) -> Result<Vec<Ty>, TypeError> {
        let mut candidates = Vec::new();
        for impl_key in self.trait_impl_candidate_keys(trait_name) {
            let Some(impl_info) = self.trait_impls.get(&impl_key).cloned() else {
                continue;
            };
            if impl_info.constructor_slot_vars.len() != 1 {
                continue;
            }
            let mut fresh = HashMap::new();
            let _target = self.instantiate_ty_with_fresh(&impl_info.target_ty, &mut fresh);
            let slot = impl_info.constructor_slot_vars[0];
            let mapping = impl_info
                .type_param_vars
                .iter()
                .copied()
                .map(|var| {
                    let replacement = if var == slot {
                        slot_ty.clone()
                    } else {
                        fresh
                            .get(&var)
                            .map(|ty| self.resolve_ty(ty))
                            .ok_or_else(|| {
                                TypeError::new(
                                    "Constructor candidate lacks fresh type input metadata",
                                    impl_info.trait_id.span.clone(),
                                )
                            })?
                    };
                    Ok((var, replacement))
                })
                .collect::<Result<HashMap<_, _>, TypeError>>()?;
            candidates.push(
                self.resolve_ty(&self.substitute_ty_with_mapping(&impl_info.target_ty, &mapping)),
            );
        }
        Ok(candidates)
    }

    pub(in crate::checker) fn check_trait_method_call(
        &mut self,
        span: &Span,
        trait_name: &str,
        method_name: &str,
        args: &[ResolvedRecordLitArg],
        receiver_owner_hint: Option<&str>,
        expected_ret_ty: Option<&Ty>,
        explicit_type_args: Option<&[AstTy]>,
    ) -> Result<TypedNode, TypeError> {
        self.check_trait_invocation(
            span,
            trait_name,
            method_name,
            args,
            receiver_owner_hint,
            expected_ret_ty,
            None,
            None,
            None,
            explicit_type_args,
            None,
            None,
        )
    }

    fn constructor_trait_arguments_shared_with_target(info: &TraitImplInfo) -> Vec<usize> {
        let mut target_variables = Vec::new();
        Self::collect_ty_vars(&info.target_ty, &mut target_variables);
        let target_variables = target_variables.into_iter().collect::<HashSet<_>>();
        info.trait_arg_tys
            .iter()
            .enumerate()
            .filter_map(|(index, argument)| {
                let mut argument_variables = Vec::new();
                Self::collect_ty_vars(argument, &mut argument_variables);
                let argument_variables = argument_variables.into_iter().collect::<HashSet<_>>();
                (!argument_variables.is_empty() && argument_variables.is_subset(&target_variables))
                    .then_some(index)
            })
            .collect()
    }

    fn apply_receiverless_target_candidate(
        &mut self,
        implementation: &TraitImplInfo,
        sourceable_trait_arguments: &[usize],
        span: &Span,
        trait_name: &str,
        method_name: &str,
        args: &[ResolvedRecordLitArg],
        declared_params: &[Ty],
        trait_args: &[Ty],
        self_ty: &Ty,
        expected: &Ty,
        operator: Option<OperatorTraitOp>,
    ) -> Result<Option<Vec<TypedNode>>, TypeError> {
        let mut fresh = HashMap::new();
        let target = self.instantiate_ty_with_fresh(&implementation.target_ty, &mut fresh);
        if self
            .assert_type_relation(
                expected,
                &target,
                self.type_fact(SourceRole::Expected, span, expected),
                self.type_fact(SourceRole::Contract, &implementation.trait_id.span, &target),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::TraitCall,
                &format!("{trait_name}::{method_name}"),
                0,
            )
            .is_err()
        {
            return Ok(None);
        }
        self.assert_type_relation(
            self_ty,
            expected,
            self.type_fact(SourceRole::Expected, span, expected),
            self.type_fact(SourceRole::Contract, &implementation.trait_id.span, self_ty),
            TypeDiagnosticReason::ReturnTypeMismatch,
            DiagnosticOrigin::TraitCall,
            &format!("{trait_name}::{method_name}"),
            0,
        )?;
        if implementation.trait_arg_tys.len() != trait_args.len() {
            return Err(TypeError::new(
                "Trait implementation argument metadata mismatch",
                implementation.trait_id.span.clone(),
            ));
        }
        for index in sourceable_trait_arguments {
            let inferred =
                self.instantiate_ty_with_fresh(&implementation.trait_arg_tys[*index], &mut fresh);
            self.assert_type_relation(
                &trait_args[*index],
                &inferred,
                self.type_fact(
                    SourceRole::Contract,
                    &implementation.trait_id.span,
                    &trait_args[*index],
                ),
                self.type_fact(
                    SourceRole::Expected,
                    &implementation.trait_id.span,
                    &inferred,
                ),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::TraitCall,
                &format!("{trait_name}::{method_name}"),
                *index as u32,
            )?;
        }

        let mut typed_args = Vec::with_capacity(args.len());
        for (index, (argument, parameter)) in args.iter().zip(declared_params).enumerate() {
            let ResolvedRecordLitArg::Positional(argument) = argument else {
                return Ok(None);
            };
            let parameter = self.resolve_ty(parameter);
            let typed = self.check_invocation_argument(
                argument,
                &parameter,
                operator.clone(),
                index,
                None,
            )?;
            self.assert_type_relation(
                &parameter,
                &typed.ty,
                self.type_fact(SourceRole::Expected, &typed.span, &parameter),
                self.type_fact(SourceRole::Value, &typed.span, &typed.ty),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::TraitCall,
                &format!("{trait_name}::{method_name}"),
                index as u32,
            )?;
            typed_args.push(typed);
        }
        self.check_trait_method_constructor_capabilities(
            trait_name,
            method_name,
            declared_params,
            None,
            &typed_args,
            None,
        )?;

        let resolved_trait_args = trait_args
            .iter()
            .map(|argument| self.resolve_ty(argument))
            .collect::<Vec<_>>();
        let argument_types = typed_args
            .iter()
            .map(|argument| self.resolve_ty(&argument.ty))
            .collect::<Vec<_>>();
        match self.select_trait_method_instantiation(
            trait_name,
            method_name,
            expected,
            &resolved_trait_args,
            &argument_types,
            expected,
        )? {
            CandidateApplicability::Applicable(instantiation)
                if matches!(
                    instantiation.implementation,
                    TraitImplementationId::Declared { ref declaration, .. }
                        if declaration == &implementation.declaration_key
                ) =>
            {
                Ok(Some(typed_args))
            }
            CandidateApplicability::Applicable(_) => Ok(None),
            CandidateApplicability::Deferred(_) => Err(self.trait_dispatch_failure(
                TypeDiagnosticReason::UnresolvedTraitMethodInstantiation,
                trait_name,
                method_name,
                Some(expected),
                &implementation.trait_id.span,
            )),
            CandidateApplicability::Rejected(_) => Err(self.trait_dispatch_failure(
                TypeDiagnosticReason::NoApplicableTraitImplementation,
                trait_name,
                method_name,
                Some(expected),
                &implementation.trait_id.span,
            )),
        }
    }

    fn contextual_receiverless_target_arguments(
        &mut self,
        trait_name: &str,
        method_name: &str,
        span: &Span,
        args: &[ResolvedRecordLitArg],
        declared_params: &[Ty],
        trait_args: &[Ty],
        self_ty: &Ty,
        expected: &Ty,
        operator: Option<OperatorTraitOp>,
    ) -> Result<Option<Vec<TypedNode>>, TypeError> {
        let mut accepted = Vec::new();
        let mut failures = Vec::new();
        for key in self.trait_impl_candidate_keys(trait_name) {
            let Some(implementation) = self.trait_impls.get(&key).cloned() else {
                continue;
            };
            let sourceable = Self::constructor_trait_arguments_shared_with_target(&implementation);
            if sourceable.len() != trait_args.len() || sourceable.is_empty() {
                continue;
            }
            let checkpoint = self.candidate_probe_checkpoint();
            let result = self.apply_receiverless_target_candidate(
                &implementation,
                &sourceable,
                span,
                trait_name,
                method_name,
                args,
                declared_params,
                trait_args,
                self_ty,
                expected,
                operator.clone(),
            );
            match result {
                Ok(Some(_)) => accepted.push((implementation, sourceable)),
                Ok(None) => {}
                Err(error) => failures.push(error),
            }
            self.rollback_candidate_probe(checkpoint);
        }
        match accepted.as_slice() {
            [(implementation, sourceable)] => self
                .apply_receiverless_target_candidate(
                    implementation,
                    sourceable,
                    span,
                    trait_name,
                    method_name,
                    args,
                    declared_params,
                    trait_args,
                    self_ty,
                    expected,
                    operator,
                )
                .map(|arguments| Some(arguments.unwrap_or_default())),
            [] => match failures.into_iter().next() {
                Some(error) => Err(error),
                None => Ok(None),
            },
            _ => Err(self.ambiguous_constructor_result(trait_name, method_name, span)),
        }
    }

    fn assert_trait_return_context(
        &mut self,
        expected: &Ty,
        ret_ty: &Ty,
        span: &Span,
        trait_name: &str,
        expected_ret_relation: Option<(&ExpectedTypeRelation, &Span)>,
    ) -> Result<(), TypeError> {
        let (expected_fact, actual_fact, reason, origin, callable, ordinal) =
            match expected_ret_relation {
                Some((relation, actual_span)) => (
                    self.type_fact(relation.role, &relation.span, expected),
                    self.type_fact(SourceRole::Value, actual_span, &ret_ty),
                    relation.reason,
                    relation.origin.clone(),
                    relation.callable,
                    relation.ordinal,
                ),
                None => (
                    self.type_fact(SourceRole::Expected, span, expected),
                    self.type_fact(SourceRole::Value, span, &ret_ty),
                    TypeDiagnosticReason::ReturnTypeMismatch,
                    DiagnosticOrigin::TraitCall,
                    trait_name,
                    0,
                ),
            };
        let relation_result = self.assert_type_relation(
            expected,
            &ret_ty,
            expected_fact,
            actual_fact,
            reason,
            origin,
            callable,
            ordinal,
        );
        if relation_result.is_err() && reason == TypeDiagnosticReason::ReturnTypeArgumentMismatch {
            let (_, actual_span) =
                expected_ret_relation.expect("return-type-argument relation has source origins");
            return Err(super::signatures::return_type_argument_mismatch_error(
                callable,
                ordinal,
                &self.diagnostic_ty_name(&ret_ty),
                &self.diagnostic_ty_name(expected),
                &expected_ret_relation
                    .expect("return-type-argument relation has explicit origin")
                    .0
                    .span,
                SourceRole::Value,
                actual_span,
            ));
        }
        relation_result?;
        Ok(())
    }

    pub(in crate::checker) fn check_trait_invocation(
        &mut self,
        span: &Span,
        trait_name: &str,
        method_name: &str,
        args: &[ResolvedRecordLitArg],
        receiver_owner_hint: Option<&str>,
        expected_ret_ty: Option<&Ty>,
        expected_ret_relation: Option<(&ExpectedTypeRelation, &Span)>,
        argument_expected_relation: Option<&ExpectedTypeRelation>,
        constructor_failure_reason: Option<TypeDiagnosticReason>,
        explicit_type_args: Option<&[AstTy]>,
        operator: Option<OperatorTraitOp>,
        receiver_hint: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        self.check_trait_invocation_with_result_context(
            span,
            trait_name,
            method_name,
            args,
            receiver_owner_hint,
            expected_ret_ty,
            expected_ret_relation,
            argument_expected_relation,
            constructor_failure_reason,
            explicit_type_args,
            operator,
            receiver_hint,
            false,
        )
    }

    pub(super) fn check_trait_invocation_with_result_context(
        &mut self,
        span: &Span,
        trait_name: &str,
        method_name: &str,
        args: &[ResolvedRecordLitArg],
        receiver_owner_hint: Option<&str>,
        expected_ret_ty: Option<&Ty>,
        expected_ret_relation: Option<(&ExpectedTypeRelation, &Span)>,
        argument_expected_relation: Option<&ExpectedTypeRelation>,
        constructor_failure_reason: Option<TypeDiagnosticReason>,
        explicit_type_args: Option<&[AstTy]>,
        operator: Option<OperatorTraitOp>,
        receiver_hint: Option<&Ty>,
        defer_incompatible_result_context: bool,
    ) -> Result<TypedNode, TypeError> {
        let call = TraitInvocationContext {
            span,
            trait_name,
            method_name,
            args,
            receiver_owner_hint,
            expected_ret_ty,
            expected_ret_relation,
            argument_expected_relation,
            constructor_failure_reason,
            explicit_type_args,
            operator,
            receiver_hint,
            defer_incompatible_result_context,
        };
        // Release signature/projection temporaries before recursively checking
        // argument bodies (notably the next continuation in a do block).
        let mut prepared = match self.prepare_trait_invocation(&call)? {
            TraitInvocationPreparation::Complete(typed) => return Ok(typed),
            TraitInvocationPreparation::Arguments(prepared) => prepared,
        };
        let typed_args = self.check_prepared_trait_arguments(&call, &mut prepared)?;
        self.finish_trait_invocation(&call, prepared, typed_args)
    }

    #[inline(never)]
    pub(super) fn prepare_trait_invocation(
        &mut self,
        call: &TraitInvocationContext<'_>,
    ) -> Result<TraitInvocationPreparation, TypeError> {
        let TraitInvocationContext {
            span,
            trait_name,
            method_name,
            args,
            expected_ret_ty,
            expected_ret_relation,
            argument_expected_relation,
            constructor_failure_reason,
            explicit_type_args,
            operator,
            receiver_hint,
            defer_incompatible_result_context,
            ..
        } = call.clone();
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError::from_structured(
                self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArgumentModeMismatch,
                    &format!("{trait_name}::{method_name}"),
                    None,
                    0,
                    args.len(),
                    span,
                    DiagnosticOrigin::TraitCall,
                ),
            ));
        }

        let trait_info = self
            .traits
            .get(trait_name)
            .cloned()
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown trait: {}", trait_name),
                span: span.clone(),
                hint: None,
            })?;
        let method = trait_info
            .methods
            .get(method_name)
            .cloned()
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown trait method: {}::{}", trait_name, method_name),
                span: span.clone(),
                hint: None,
            })?;

        let positional_args = args
            .iter()
            .filter_map(|arg| match arg {
                ResolvedRecordLitArg::Positional(expr) => Some(expr),
                ResolvedRecordLitArg::Named(_, _) => None,
            })
            .collect::<Vec<_>>();
        // Generic zero-argument constructor helpers (for example
        // `Monoid::mempty()`) use the same expected-result dispatch.  Keep
        // this signature-driven rather than growing another trait-name list.
        if positional_args.is_empty() && method.value_parameters.is_empty() {
            let probe_self = self.env.fresh_tyvar();
            let (_, probe_ret, _, probe_slots, _) =
                self.resolve_trait_method_signature(&trait_info, &method, &probe_self)?;
            if let Some(arguments) = explicit_type_args {
                if arguments.len() != probe_slots.len() {
                    return Err(super::signatures::return_type_argument_arity_error(
                        &format!("{trait_name}::{method_name}"),
                        probe_slots.len(),
                        arguments.len(),
                        span,
                    ));
                }
            }
            if self.resolve_ty(&probe_ret) == self.resolve_ty(&probe_self)
                || matches!(probe_ret, Ty::SelfApp(_))
                || (self.trait_matches_short_name(trait_name, "Default")
                    && method_name == "default"
                    && (explicit_type_args.is_some() || expected_ret_ty.is_some()))
            {
                let explicit_target = match explicit_type_args.and_then(|arguments| {
                    probe_slots
                        .iter()
                        .position(|slot| slot == &probe_self)
                        .map(|position| &arguments[position])
                }) {
                    Some(argument) if !trait_info.constructor_slots.is_empty() => {
                        Some(self.resolve_type_constructor_return_type_argument(argument)?)
                    }
                    Some(argument) => Some(
                        self.resolve_trait_call_site_return_type_argument(&probe_self, argument)?,
                    ),
                    None => None,
                };
                let expected = explicit_target
                    .as_ref()
                    .or(expected_ret_ty)
                    .ok_or_else(|| {
                        self.ambiguous_constructor_result(trait_name, method_name, span)
                    })?
                    .clone();
                if let Some(expected_ret_ty) = expected_ret_ty {
                    self.assert_type_relation(
                        expected_ret_ty,
                        &expected,
                        self.type_fact(SourceRole::Expected, span, expected_ret_ty),
                        self.type_fact(SourceRole::ReturnTypeArgument, span, &expected),
                        TypeDiagnosticReason::ReturnTypeMismatch,
                        DiagnosticOrigin::Return,
                        &format!("{trait_name}::{method_name}"),
                        0,
                    )?;
                }
                let expected = self.resolve_ty(&expected);
                let mut unresolved = Vec::new();
                Self::collect_ty_vars(&expected, &mut unresolved);
                if explicit_target.is_some()
                    && expected_ret_ty.is_none()
                    && unresolved
                        .iter()
                        .any(|variable| !self.rigid_tyvars.contains(variable))
                {
                    return Err(self.ambiguous_constructor_result(trait_name, method_name, span));
                }
                if !self.trait_matches_short_name(trait_name, "Default") {
                    if let Some(arguments) = explicit_type_args {
                        if let ConstructorProjectionOutcome::Applicable {
                            info: implementation,
                            mapping,
                        } = self.constructor_projection(trait_name, &expected)
                        {
                            let expanded = self.expand_trait_self_apps(
                                probe_ret.clone(),
                                &implementation.target_ty,
                                &implementation.constructor_slot_vars,
                            )?;
                            let return_type = self.substitute_ty_with_mapping(&expanded, &mapping);
                            self.assert_type_relation(
                                &expected,
                                &return_type,
                                self.type_fact(SourceRole::Expected, span, &expected),
                                self.type_fact(SourceRole::Value, span, &return_type),
                                TypeDiagnosticReason::ReturnTypeMismatch,
                                DiagnosticOrigin::Return,
                                &format!("{trait_name}::{method_name}"),
                                0,
                            )?;
                        }
                        for (ordinal, (slot, argument)) in
                            probe_slots.iter().zip(arguments).enumerate()
                        {
                            let explicit = if slot == &probe_self
                                && !trait_info.constructor_slots.is_empty()
                            {
                                self.resolve_type_constructor_return_type_argument(argument)?
                            } else {
                                self.resolve_trait_call_site_return_type_argument(slot, argument)?
                            };
                            let target = if slot == &probe_self { &expected } else { slot };
                            if self
                                .assert_type_relation(
                                    target,
                                    &explicit,
                                    self.type_fact(SourceRole::Expected, span, target),
                                    self.type_fact(
                                        SourceRole::ReturnTypeArgument,
                                        Self::ast_ty_span(argument),
                                        &explicit,
                                    ),
                                    TypeDiagnosticReason::ArgumentTypeMismatch,
                                    DiagnosticOrigin::TraitCall,
                                    &format!("{trait_name}::{method_name}"),
                                    ordinal as u32,
                                )
                                .is_err()
                            {
                                return Err(
                                    super::signatures::return_type_argument_mismatch_error(
                                        &format!("{trait_name}::{method_name}"),
                                        ordinal as u32,
                                        &self.diagnostic_ty_name(target),
                                        &Self::surface_ast_ty(argument),
                                        Self::ast_ty_span(argument),
                                        SourceRole::Expected,
                                        span,
                                    ),
                                );
                            }
                        }
                    }
                }
                let dispatch = self
                    .constructor_target_dispatch(trait_name, method_name, &expected)?
                    .ok_or_else(|| {
                        if self.constructor_capability_failure_reason(&expected)
                            == TypeDiagnosticReason::MissingGenericBound
                        {
                            self.trait_obligation_failure(
                                TypeDiagnosticReason::MissingGenericBound,
                                trait_name,
                                &[],
                                Some(method_name),
                                &expected,
                                span,
                                DiagnosticOrigin::TraitCall,
                            )
                        } else {
                            self.trait_dispatch_failure(
                                TypeDiagnosticReason::NoApplicableTraitImplementation,
                                trait_name,
                                method_name,
                                Some(&expected),
                                span,
                            )
                        }
                    })?;
                let typed = TypedNode {
                    ty: expected.clone(),
                    span: span.clone(),
                    node: TypedInner::TraitCall {
                        trait_name: trait_name.to_string(),
                        method_name: method_name.to_string(),
                        receiver_ty: expected.clone(),
                        obligation: TraitObligation {
                            trait_id: trait_name.to_string(),
                            trait_args: Vec::new(),
                            receiver: expected.clone(),
                        },
                        dispatch,
                        origin: TraitCallOrigin::Explicit,
                        args: Vec::new(),
                    },
                };
                return Ok(TraitInvocationPreparation::Complete(typed));
            }
        }
        let self_ty = if method.attrs.visibility == spire::ast::Visibility::Private {
            self.local_annotation_tyvars
                .get("Self")
                .cloned()
                .ok_or_else(|| {
                    TypeError::new(
                        "Internal error: private trait helper has no lexical Self owner",
                        span.clone(),
                    )
                })?
        } else {
            self.env.fresh_tyvar()
        };
        let (mut param_tys, mut ret_ty, trait_arg_tys, explicit_slots, _) =
            self.resolve_trait_method_signature(&trait_info, &method, &self_ty)?;
        if let Some(explicit_args) = explicit_type_args {
            if explicit_args.len() != explicit_slots.len() {
                return Err(super::signatures::return_type_argument_arity_error(
                    &format!("{trait_name}::{method_name}"),
                    explicit_slots.len(),
                    explicit_args.len(),
                    span,
                ));
            }
        }
        if let Some(explicit_args) = explicit_type_args {
            for (ordinal, (slot, arg)) in explicit_slots.iter().zip(explicit_args).enumerate() {
                let explicit_ty = if slot == &self_ty && !trait_info.constructor_slots.is_empty() {
                    self.resolve_type_constructor_return_type_argument(arg)?
                } else {
                    self.resolve_trait_call_site_return_type_argument(slot, arg)?
                };
                if self
                    .assert_type_relation(
                        slot,
                        &explicit_ty,
                        self.type_fact(SourceRole::Contract, span, slot),
                        self.type_fact(
                            SourceRole::ReturnTypeArgument,
                            Self::ast_ty_span(arg),
                            &explicit_ty,
                        ),
                        TypeDiagnosticReason::ArgumentTypeMismatch,
                        DiagnosticOrigin::TraitCall,
                        &format!("{trait_name}::{method_name}"),
                        ordinal as u32,
                    )
                    .is_err()
                {
                    return Err(super::signatures::return_type_argument_mismatch_error(
                        &format!("{trait_name}::{method_name}"),
                        ordinal as u32,
                        &self.diagnostic_ty_name(slot),
                        &self.diagnostic_ty_name(&explicit_ty),
                        Self::ast_ty_span(arg),
                        SourceRole::Contract,
                        span,
                    ));
                }
            }
        }

        // Return-only inputs cannot be inferred from value arguments. Report
        // that missing context before dispatch so branch checking can supply its
        // expected result through the ordinary ReturnTypeArgument protocol.
        if explicit_type_args.is_none()
            && expected_ret_ty.is_none()
            && explicit_slots.iter().any(|slot| {
                let mut variables = Vec::new();
                Self::collect_ty_vars(&self.resolve_ty(slot), &mut variables);
                variables
                    .iter()
                    .any(|variable| !self.rigid_tyvars.contains(variable))
            })
        {
            return Err(self.ambiguous_constructor_result(trait_name, method_name, span));
        }

        let declared_param_tys = param_tys.clone();
        let mut constructor_receiver_index = None;
        let mut prepared_args = vec![None; args.len()];
        let declared_receiver_index = param_tys.iter().enumerate().rev().find_map(|(index, ty)| {
            matches!(ty, Ty::SelfApp(items) if Self::constructor_application_parts(items).is_none())
                .then_some(index)
        });
        fn contains_trait_constructor_input(ty: &Ty, trait_args: &[Ty]) -> bool {
            match ty {
                Ty::SelfApp(items) => {
                    Checker::constructor_application_parts(items).is_some_and(|(witness, _)| {
                        trait_args.iter().any(|argument| argument == witness)
                    }) || items
                        .iter()
                        .any(|item| contains_trait_constructor_input(item, trait_args))
                }
                Ty::List(inner)
                | Ty::MatchResult(inner)
                | Ty::ExtractorClosure(inner)
                | Ty::Lazy(inner)
                | Ty::Pid(inner) => contains_trait_constructor_input(inner, trait_args),
                Ty::Result(ok, error) => {
                    contains_trait_constructor_input(ok, trait_args)
                        || contains_trait_constructor_input(error, trait_args)
                }
                Ty::Tuple(items) | Ty::Enum(_, items) => items
                    .iter()
                    .any(|item| contains_trait_constructor_input(item, trait_args)),
                Ty::Struct(_, nominal) | Ty::Record(_, nominal) => nominal
                    .arguments
                    .iter()
                    .any(|item| contains_trait_constructor_input(item, trait_args)),
                Ty::Func(inputs, output)
                | Ty::BuiltinFunc {
                    params: inputs,
                    ret: output,
                    ..
                }
                | Ty::UserFunc {
                    params: inputs,
                    ret: output,
                    ..
                } => {
                    inputs
                        .iter()
                        .any(|item| contains_trait_constructor_input(item, trait_args))
                        || contains_trait_constructor_input(output, trait_args)
                }
                Ty::Facet(_, source, focus, update_source, update_focus) => [
                    source.as_ref(),
                    focus.as_ref(),
                    update_source.as_ref(),
                    update_focus.as_ref(),
                ]
                .into_iter()
                .any(|item| contains_trait_constructor_input(item, trait_args)),
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
        // When a receiverless constructor method's value parameter carries a
        // trait-head constructor (for example MonadT<$M>::lift($M<$A>)), let
        // concrete value arguments constrain the trait head before projecting
        // the result carrier. Projecting from the result alone can otherwise
        // choose an implementation and feed its input type back into an
        // underconstrained argument.
        let direct_self_result = match &ret_ty {
            Ty::Var(variable) => matches!(
                self.resolve_ty(&self_ty),
                Ty::Var(self_variable) if self_variable == *variable
            ),
            Ty::SelfApp(items) => Self::constructor_application_parts(items).is_none(),
            _ => false,
        };
        let receiverless_head_input = explicit_type_args.is_none()
            && declared_receiver_index.is_none()
            && !trait_info.constructor_slots.is_empty()
            && direct_self_result
            && declared_param_tys
                .iter()
                .any(|parameter| contains_trait_constructor_input(parameter, &trait_arg_tys));
        if receiverless_head_input && args.len() == declared_param_tys.len() {
            if expected_ret_ty.is_none() {
                return Err(self.ambiguous_constructor_result(trait_name, method_name, span));
            }
            for (param, argument) in trait_info.type_params.iter().zip(&trait_arg_tys) {
                let constructor_trait =
                    self.trait_head_parameter_constructor_bound(&trait_info, &param.name);
                if let (Some(constructor_trait), Ty::Var(variable)) =
                    (constructor_trait, self.resolve_ty(argument))
                {
                    if !self.rigid_tyvars.contains(&variable) {
                        self.constructor_witness_traits
                            .entry(variable)
                            .or_insert(constructor_trait);
                    }
                }
            }
            if let Some(expected) = expected_ret_ty {
                let normalized = self.resolve_ty(expected);
                let mut variables = Vec::new();
                Self::collect_ty_vars(&normalized, &mut variables);
                let stable_context = variables
                    .iter()
                    .all(|variable| self.rigid_tyvars.contains(variable));
                let contextual_args = if stable_context {
                    self.contextual_receiverless_target_arguments(
                        trait_name,
                        method_name,
                        span,
                        args,
                        &declared_param_tys,
                        &trait_arg_tys,
                        &self_ty,
                        &normalized,
                        operator.clone(),
                    )?
                } else {
                    None
                };
                if let Some(contextual_args) = contextual_args {
                    prepared_args = contextual_args.into_iter().map(Some).collect();
                } else {
                    for (index, (argument, parameter)) in
                        args.iter().zip(&declared_param_tys).enumerate()
                    {
                        if !contains_trait_constructor_input(parameter, &trait_arg_tys) {
                            continue;
                        }
                        let ResolvedRecordLitArg::Positional(argument) = argument else {
                            continue;
                        };
                        let typed = self.check_node(argument)?;
                        self.assert_type_relation(
                            parameter,
                            &typed.ty,
                            self.type_fact(SourceRole::Contract, span, parameter),
                            self.type_fact(SourceRole::Value, &typed.span, &typed.ty),
                            TypeDiagnosticReason::ArgumentTypeMismatch,
                            DiagnosticOrigin::TraitCall,
                            &format!("{trait_name}::{method_name}"),
                            index as u32,
                        )?;
                        prepared_args[index] = Some(typed);
                    }
                    self.assert_type_relation(
                        &self_ty,
                        &normalized,
                        self.type_fact(SourceRole::Expected, span, &normalized),
                        self.type_fact(SourceRole::Contract, span, &self_ty),
                        TypeDiagnosticReason::ReturnTypeMismatch,
                        DiagnosticOrigin::TraitCall,
                        &format!("{trait_name}::{method_name}"),
                        0,
                    )?;
                }
                ret_ty = self.resolve_ty(expected);
            }
        }
        if !trait_info.constructor_slots.is_empty()
            && args.len() == param_tys.len()
            && !receiverless_head_input
        {
            let receiver_index = declared_receiver_index;
            let typed_receiver = if let Some(index) = receiver_index {
                constructor_receiver_index = Some(index);
                if receiver_hint.is_none() {
                    if let Some(typed) = self.try_constructor_invocation_from_arguments(
                        span,
                        trait_name,
                        method_name,
                        args,
                        &declared_param_tys,
                        expected_ret_ty,
                        expected_ret_relation,
                        argument_expected_relation,
                        constructor_failure_reason,
                        explicit_type_args,
                        operator.clone(),
                        index,
                    )? {
                        return Ok(TraitInvocationPreparation::Complete(typed));
                    }
                }
                Some(self.check_node_with_expected(positional_args[index], receiver_hint)?)
            } else {
                None
            };
            let receiver_ty = if let Some(typed) = &typed_receiver {
                self.resolve_ty(&typed.ty)
            } else if receiverless_head_input {
                let contextual = expected_ret_ty
                    .map(|ty| self.resolve_ty(ty))
                    .unwrap_or_else(|| self.resolve_ty(&self_ty));
                if matches!(contextual, Ty::Var(_) | Ty::Hole) {
                    return Err(self.ambiguous_constructor_result(trait_name, method_name, span));
                }
                contextual
            } else if matches!(ret_ty, Ty::SelfApp(_)) {
                let contextual = expected_ret_ty
                    .map(|ty| self.resolve_ty(ty))
                    .unwrap_or_else(|| self.resolve_ty(&self_ty));
                if matches!(contextual, Ty::Var(_) | Ty::Hole) {
                    return Err(self.ambiguous_constructor_result(trait_name, method_name, span));
                }
                contextual
            } else {
                self.resolve_ty(&self_ty)
            };
            if !matches!(receiver_ty, Ty::Var(_) | Ty::Hole) {
                let receiver_span = typed_receiver
                    .as_ref()
                    .map(|typed| &typed.span)
                    .unwrap_or(span);
                let (target, slots, captured, implementation_trait_args) =
                    if let Ty::SelfApp(items) = &receiver_ty {
                        let (witness, payloads) = Self::constructor_application_parts(items)
                            .ok_or_else(|| {
                                TypeError::new(
                                    "Constructor application has no witness",
                                    receiver_span.clone(),
                                )
                            })?;
                        let mut target = vec![Ty::Hole, witness.clone()];
                        let mut slots = Vec::new();
                        for _ in payloads {
                            let slot = self.env.fresh_tyvar();
                            let Ty::Var(var) = slot else { unreachable!() };
                            slots.push(var);
                            target.push(slot);
                        }
                        (Ty::SelfApp(target), slots, HashMap::new(), None)
                    } else {
                        let (implementation, mut captured) = match self
                            .constructor_capability_projection(trait_name, &receiver_ty)
                        {
                            ConstructorProjectionOutcome::Applicable { info, mapping } => {
                                (info, mapping)
                            }
                            ConstructorProjectionOutcome::Deferred { .. } => {
                                return Err(self.trait_dispatch_failure(
                                    TypeDiagnosticReason::UnresolvedTraitMethodInstantiation,
                                    trait_name,
                                    method_name,
                                    Some(&receiver_ty),
                                    receiver_span,
                                ));
                            }
                            ConstructorProjectionOutcome::Rejected { failures } => {
                                if Self::constructor_projection_failures_are_metadata(&failures) {
                                    return Err(signatures::constructor_signature_metadata_error(
                                        method_name,
                                        receiver_span,
                                        Self::constructor_projection_failure_detail(&failures),
                                    ));
                                }
                                if let Some(reason) = constructor_failure_reason {
                                    return Err(self.trait_obligation_failure(
                                        reason,
                                        trait_name,
                                        &[],
                                        None,
                                        &receiver_ty,
                                        receiver_span,
                                        DiagnosticOrigin::TraitCall,
                                    ));
                                }
                                return Err(self.trait_dispatch_failure(
                                    TypeDiagnosticReason::NoApplicableTraitImplementation,
                                    trait_name,
                                    method_name,
                                    Some(&receiver_ty),
                                    receiver_span,
                                ));
                            }
                        };
                        let implementation_trait_args = implementation
                            .trait_arg_tys
                            .iter()
                            .map(|argument| {
                                self.resolve_ty(
                                    &self.substitute_ty_with_mapping(argument, &captured),
                                )
                            })
                            .collect::<Vec<_>>();
                        for slot in &implementation.constructor_slot_vars {
                            captured.remove(slot);
                        }
                        (
                            implementation.target_ty.clone(),
                            implementation.constructor_slot_vars.clone(),
                            captured,
                            Some(implementation_trait_args),
                        )
                    };
                if let Some(implementation_trait_args) = implementation_trait_args {
                    if implementation_trait_args.len() != trait_arg_tys.len() {
                        return Err(TypeError::new(
                            "Trait implementation argument metadata mismatch",
                            receiver_span.clone(),
                        ));
                    }
                    for (ordinal, (declared, inferred)) in trait_arg_tys
                        .iter()
                        .zip(&implementation_trait_args)
                        .enumerate()
                    {
                        let constructor_trait = self
                            .traits
                            .get(trait_name)
                            .and_then(|info| {
                                info.type_params.get(ordinal).map(|param| (info, param))
                            })
                            .and_then(|(info, param)| {
                                self.trait_head_parameter_constructor_bound(info, &param.name)
                            });
                        if let (Some(constructor_trait), Ty::Var(variable)) =
                            (constructor_trait, self.resolve_ty(declared))
                        {
                            if !self.rigid_tyvars.contains(&variable) {
                                self.constructor_witness_traits
                                    .entry(variable)
                                    .or_insert(constructor_trait);
                            }
                        }
                        let declared =
                            self.normalize_executable_trait_argument(trait_name, ordinal, declared);
                        let inferred =
                            self.normalize_executable_trait_argument(trait_name, ordinal, inferred);
                        self.assert_type_relation(
                            &declared,
                            &inferred,
                            self.type_fact(SourceRole::Contract, span, &declared),
                            self.type_fact(SourceRole::Value, receiver_span, &inferred),
                            TypeDiagnosticReason::ArgumentTypeMismatch,
                            DiagnosticOrigin::TraitCall,
                            &format!("{trait_name}::{method_name}"),
                            0,
                        )?;
                    }
                }
                if let Some(typed) = &typed_receiver {
                    self.check_constructor_capability(
                        trait_name,
                        method_name,
                        typed,
                        constructor_failure_reason,
                    )?;
                }
                let expand = |ty| {
                    self.expand_trait_self_apps(ty, &target, &slots)
                        .map(|ty| self.substitute_ty_with_mapping(&ty, &captured))
                };
                param_tys = param_tys
                    .into_iter()
                    .map(&expand)
                    .collect::<Result<_, _>>()?;
                ret_ty = expand(ret_ty)?;
                self.assert_type_relation(
                    &self_ty,
                    &receiver_ty,
                    self.type_fact(SourceRole::Expected, span, &self_ty),
                    self.type_fact(SourceRole::Value, receiver_span, &receiver_ty),
                    TypeDiagnosticReason::ArgumentTypeMismatch,
                    DiagnosticOrigin::TraitCall,
                    &format!("{trait_name}::{method_name}"),
                    receiver_index.unwrap_or(0) as u32,
                )?;
                if let (Some(index), Some(typed)) = (receiver_index, typed_receiver) {
                    self.assert_type_relation(
                        &param_tys[index],
                        &typed.ty,
                        self.type_fact(SourceRole::Expected, span, &param_tys[index]),
                        self.type_fact(SourceRole::Value, &typed.span, &typed.ty),
                        TypeDiagnosticReason::ArgumentTypeMismatch,
                        DiagnosticOrigin::TraitCall,
                        &format!("{trait_name}::{method_name}"),
                        index as u32,
                    )?;
                    prepared_args[index] = Some(typed);
                } else {
                    self.assert_type_relation(
                        &self_ty,
                        &ret_ty,
                        self.type_fact(SourceRole::Expected, span, &self_ty),
                        self.type_fact(SourceRole::Value, span, &ret_ty),
                        TypeDiagnosticReason::ReturnTypeMismatch,
                        DiagnosticOrigin::TraitCall,
                        &format!("{trait_name}::{method_name}"),
                        0,
                    )?;
                }
            }
        }
        if !defer_incompatible_result_context {
            if let Some(expected) = expected_ret_ty {
                self.assert_trait_return_context(
                    expected,
                    &ret_ty,
                    span,
                    trait_name,
                    expected_ret_relation,
                )?;
            }
        }

        let trait_display_name = self.trait_display_name(trait_name);
        let trait_signature_hint = |checker: &Self| {
            let params = param_tys
                .iter()
                .map(|ty| checker.resolve_ty(ty))
                .collect::<Vec<_>>();
            let ret = checker.resolve_ty(&ret_ty);
            checker.trait_method_signature_hint(&trait_display_name, method_name, &params, &ret)
        };

        if args.len() != param_tys.len() {
            return Err(
                TypeError::from_structured(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArityMismatch,
                    &format!("{trait_name}::{method_name}"),
                    None,
                    param_tys.len(),
                    args.len(),
                    span,
                    DiagnosticOrigin::TraitCall,
                ))
                .with_hint(trait_signature_hint(self)),
            );
        }

        Ok(TraitInvocationPreparation::Arguments(
            PreparedTraitInvocation {
                param_tys,
                ret_ty,
                trait_arg_tys,
                self_ty,
                declared_param_tys,
                constructor_receiver_index,
                prepared_args,
            },
        ))
    }

    pub(super) fn check_prepared_trait_arguments(
        &mut self,
        call: &TraitInvocationContext<'_>,
        prepared: &mut PreparedTraitInvocation,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let mut remaining = prepared
            .prepared_args
            .iter()
            .filter(|arg| arg.is_none())
            .count();
        while remaining > 0 {
            let before = remaining;
            for (index, (argument, expected)) in
                call.args.iter().zip(&prepared.param_tys).enumerate()
            {
                if prepared.prepared_args[index].is_some() {
                    continue;
                }
                let ResolvedRecordLitArg::Positional(expr) = argument else {
                    unreachable!("named arguments rejected");
                };
                let inputs = if Self::has_contextual_callback_parameters(expr) {
                    self.callback_input_provenance(
                        &prepared.param_tys,
                        &prepared.prepared_args,
                        index,
                        self.resolved_span(expr),
                    )?
                } else {
                    Some(Vec::new())
                };
                let Some(inputs) = inputs else {
                    continue;
                };
                let typed = self
                    .with_callback_parameter_provenance(expr, &inputs, |checker| {
                        if checker.trait_parameter_is_lazy(call.trait_name, call.method_name, index)
                        {
                            return checker.check_trait_lazy_argument(expr, expected, call.span);
                        }
                        checker.check_invocation_argument(
                            expr,
                            expected,
                            call.operator.clone(),
                            index,
                            call.argument_expected_relation.filter(|_| index == 1),
                        )
                    })
                    .map_err(|mut error| {
                        if let Some(diagnostic) = &mut error.structured {
                            if diagnostic.origin == DiagnosticOrigin::Call
                                && diagnostic.primary.span == *self.resolved_span(expr)
                            {
                                diagnostic.origin = DiagnosticOrigin::TraitCall;
                            }
                        }
                        error
                    })?;
                prepared.prepared_args[index] = Some(typed);
                remaining -= 1;
            }
            if remaining == before {
                return Err(self.unresolved_callback_provenance_error(call.span));
            }
        }
        let typed_args = prepared
            .prepared_args
            .iter_mut()
            .map(|arg| arg.take().expect("all trait arguments were checked"))
            .collect::<Vec<_>>();
        self.check_callback_argument_provenance(&prepared.param_tys, &typed_args)?;
        Ok(typed_args)
    }

    pub(super) fn has_contextual_callback_parameters(expression: &Resolved) -> bool {
        match expression {
            Resolved::Grouped(_, inner) => Self::has_contextual_callback_parameters(inner),
            Resolved::Closure(_, parameters, _, _)
            | Resolved::CaptureClosure(_, parameters, _, _)
            | Resolved::ExtractorClosure(_, parameters, _, _) => parameters
                .iter()
                .any(|parameter| parameter.ty.is_none() || parameter.id.compiler_generated),
            _ => false,
        }
    }

    /// Install input views by resolved parameter identity, before checking a
    /// contextual closure body. Declared nominal parameters retain their own
    /// contract; inferred and compiler-generated parameters inherit the source.
    pub(super) fn with_callback_parameter_provenance<T>(
        &mut self,
        expression: &Resolved,
        inputs: &[(ConstructorCapabilityProvenance, Ty)],
        check: impl FnOnce(&mut Self) -> Result<T, TypeError>,
    ) -> Result<T, TypeError> {
        let mut expression = expression;
        while let Resolved::Grouped(_, inner) = expression {
            expression = inner;
        }
        let parameters = match expression {
            Resolved::Closure(_, parameters, _, _)
            | Resolved::CaptureClosure(_, parameters, _, _)
            | Resolved::ExtractorClosure(_, parameters, _, _) => parameters.as_slice(),
            _ => &[],
        };
        let saved = parameters
            .iter()
            .zip(inputs)
            .filter_map(|(parameter, source)| {
                (parameter.ty.is_none() || parameter.id.compiler_generated).then(|| {
                    let id = parameter.id.unique_id;
                    (
                        id,
                        self.constructor_capabilities.insert(id, source.0.clone()),
                    )
                })
            })
            .collect::<Vec<_>>();
        let result = check(self);
        for (id, previous) in saved {
            if let Some(previous) = previous {
                self.constructor_capabilities.insert(id, previous);
            } else {
                self.constructor_capabilities.remove(&id);
            }
        }
        result
    }

    #[inline(never)]
    pub(super) fn finish_trait_invocation(
        &mut self,
        call: &TraitInvocationContext<'_>,
        prepared: PreparedTraitInvocation,
        typed_args: Vec<TypedNode>,
    ) -> Result<TypedNode, TypeError> {
        let TraitInvocationContext {
            span,
            trait_name,
            method_name,
            receiver_owner_hint,
            expected_ret_ty,
            expected_ret_relation,
            constructor_failure_reason,
            defer_incompatible_result_context,
            ..
        } = call.clone();
        let PreparedTraitInvocation {
            param_tys,
            ret_ty,
            trait_arg_tys,
            self_ty,
            declared_param_tys,
            constructor_receiver_index,
            ..
        } = prepared;
        self.ensure_no_runtime_facet_args(&typed_args, span, "Trait method call")?;

        self.check_trait_method_constructor_capabilities(
            trait_name,
            method_name,
            &declared_param_tys,
            constructor_receiver_index,
            &typed_args,
            constructor_failure_reason,
        )?;

        if let Some(owner_hint) = receiver_owner_hint {
            if let Some(receiver) = typed_args.first() {
                let receiver_ty = self.resolve_ty(&receiver.ty);
                if let Some(receiver_name) = self.trait_target_name(&receiver_ty) {
                    if Self::surface_name(&receiver_name) != owner_hint {
                        return Err(TypeError {
                            structured: None,
                            message: format!(
                                "{}::{} helper requires receiver type {}, got {}",
                                owner_hint,
                                method_name,
                                owner_hint,
                                self.ty_name(&receiver_ty)
                            ),
                            span: receiver.span.clone(),
                            hint: Some(format!(
                                "Use {}::{} only for {} values.",
                                owner_hint, method_name, owner_hint
                            )),
                        });
                    }
                }
            }
        }

        for (idx, (expected, arg)) in param_tys.iter().zip(&typed_args).enumerate() {
            let contextual_policy = if constructor_receiver_index.is_some()
                && matches!(declared_param_tys[idx], Ty::Func(..))
            {
                Some((
                    trait_name,
                    matches!(&declared_param_tys[idx], Ty::Func(_, output) if matches!(output.as_ref(), Ty::SelfApp(_))),
                ))
            } else {
                None
            };
            if let Some((capability, contextual_return)) = contextual_policy {
                let source_index = constructor_receiver_index.unwrap_or(0);
                self.assert_contextual_callable_argument(
                    expected,
                    arg,
                    &typed_args[source_index],
                    capability,
                    contextual_return,
                    &format!("{trait_name}::{method_name}"),
                    idx as u32,
                )?;
            } else {
                let expected_fact = if idx > 0 {
                    self.type_fact(
                        SourceRole::LeftValue,
                        &typed_args[0].span,
                        &typed_args[0].ty,
                    )
                } else {
                    self.type_fact(SourceRole::Expected, span, expected)
                };
                self.assert_type_relation(
                    expected,
                    &arg.ty,
                    expected_fact,
                    self.type_fact(
                        if idx > 0 {
                            SourceRole::RightValue
                        } else {
                            SourceRole::Value
                        },
                        &arg.span,
                        &arg.ty,
                    ),
                    TypeDiagnosticReason::ArgumentTypeMismatch,
                    DiagnosticOrigin::TraitCall,
                    &format!("{trait_name}::{method_name}"),
                    idx as u32,
                )?;
            }
        }

        if defer_incompatible_result_context {
            if let Some(expected) = expected_ret_ty {
                // A Result context may finish payload inference after the operands
                // establish Self. Probe without committing failed substitutions.
                // An incompatible context is necessarily rejected by the outside
                // annotation, return, or argument relation; it is not accepted here.
                let compatible = self
                    .with_type_relation_probe(&[expected, &ret_ty], |checker| {
                        checker.types_compatible(expected, &ret_ty)
                    })?;
                if compatible {
                    self.assert_trait_return_context(
                        expected,
                        &ret_ty,
                        span,
                        trait_name,
                        expected_ret_relation,
                    )?;
                }
            }
        }

        let trait_call_name = self.trait_instance_key_from_tys(trait_name, &trait_arg_tys);
        let receiver_ty = self.resolve_ty(&self_ty);
        if let Ty::Var(var) = receiver_ty {
            if self.rigid_tyvars.contains(&var)
                && !self.tyvar_has_bound(var, trait_name)
                && !self.tyvar_satisfies_compiler_trait(var, trait_name)
            {
                return Err(self.trait_obligation_failure(
                    TypeDiagnosticReason::MissingGenericBound,
                    trait_name,
                    &trait_arg_tys,
                    Some(method_name),
                    &receiver_ty,
                    &typed_args
                        .first()
                        .map(|arg| arg.span.clone())
                        .unwrap_or_else(|| span.clone()),
                    DiagnosticOrigin::TraitCall,
                ));
            }
            if !self.rigid_tyvars.contains(&var) {
                let obligations = self.pending_trait_obligations.entry(var).or_default();
                let obligation = PendingTraitObligation {
                    trait_id: trait_name.to_string(),
                    args: trait_arg_tys.clone(),
                    receiver: Ty::Var(var),
                };
                if !obligations.contains(&obligation) {
                    obligations.push(obligation);
                }
            }
        }
        let receiver_span = typed_args
            .first()
            .map(|arg| arg.span.clone())
            .unwrap_or_else(|| span.clone());

        if let Some(err) = self.opposite_conversion_hint(
            trait_name,
            method_name,
            &receiver_ty,
            &trait_arg_tys,
            &receiver_span,
        ) {
            if self
                .trait_dispatch_target_for_args(
                    trait_name,
                    method_name,
                    &receiver_ty,
                    &trait_arg_tys,
                )?
                .is_none()
            {
                return Err(self
                    .trait_obligation_failure(
                        TypeDiagnosticReason::NoApplicableTraitImplementation,
                        trait_name,
                        &trait_arg_tys,
                        Some(method_name),
                        &receiver_ty,
                        &receiver_span,
                        DiagnosticOrigin::TraitCall,
                    )
                    .with_hint(err.message));
            }
        }

        let receiver_ty = self.resolve_ty(&self_ty);
        self.consume_matching_capability(&receiver_ty, trait_name);
        let argument_tys = typed_args
            .iter()
            .map(|arg| self.resolve_ty(&arg.ty))
            .collect::<Vec<_>>();
        let mut rejection_note = None;
        let mut rejection_facts = Vec::new();
        let abstract_constructor = match &receiver_ty {
            Ty::SelfApp(items) => Self::constructor_application_parts(items)
                .map(|(witness, _)| self.constructor_witness_supports_trait(witness, trait_name))
                .unwrap_or(false),
            _ => false,
        };
        let dispatch = if abstract_constructor {
            Some(TraitDispatch::Pending)
        } else {
            match self.select_trait_method_instantiation(
                trait_name,
                method_name,
                &receiver_ty,
                &trait_arg_tys,
                &argument_tys,
                &ret_ty,
            )? {
                CandidateApplicability::Applicable(instantiation) => {
                    Some(TraitDispatch::Selected(Box::new(instantiation)))
                }
                CandidateApplicability::Deferred(_) => self.trait_dispatch_target_for_args(
                    trait_name,
                    method_name,
                    &receiver_ty,
                    &trait_arg_tys,
                )?,
                CandidateApplicability::Rejected(rejection) => {
                    rejection_note = self.candidate_rejection_note(&rejection);
                    rejection_facts = self.candidate_rejection_facts(&rejection);
                    self.consume_matching_capability(&receiver_ty, trait_name)
                        .then_some(TraitDispatch::Pending)
                }
            }
        }
        .ok_or_else(|| {
            let mut error = self.trait_obligation_failure(
                TypeDiagnosticReason::NoApplicableTraitImplementation,
                trait_name,
                &trait_arg_tys,
                Some(method_name),
                &receiver_ty,
                &receiver_span,
                DiagnosticOrigin::TraitCall,
            );
            if let Some(diagnostic) = &mut error.structured {
                diagnostic.related.extend(rejection_facts);
            }
            let forbidden_dependency = error.structured.as_ref().is_some_and(|diagnostic| {
                matches!(
                    &diagnostic.data,
                    diagnostics::DiagnosticData::TraitDispatch(dispatch)
                        if dispatch.dependency.as_ref().is_some_and(|dependency| {
                            dependency.leaf_policy
                                == diagnostics::TraitDependencyLeafPolicy::TraitImplementationForbidden
                        })
                )
            });
            if let Some(note) = rejection_note.filter(|_| !forbidden_dependency) {
                error = error.with_hint(note);
            }
            error
        })?;

        Ok(TypedNode {
            ty: self.resolve_ty(&ret_ty),
            span: span.clone(),
            node: TypedInner::TraitCall {
                trait_name: trait_call_name,
                method_name: method_name.into(),
                receiver_ty: receiver_ty.clone(),
                obligation: TraitObligation {
                    trait_id: trait_name.to_string(),
                    trait_args: trait_arg_tys.iter().map(|ty| self.resolve_ty(ty)).collect(),
                    receiver: receiver_ty,
                },
                dispatch,
                origin: TraitCallOrigin::Explicit,
                args: typed_args,
            },
        })
    }
}
