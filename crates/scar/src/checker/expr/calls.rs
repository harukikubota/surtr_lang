use super::*;
use diagnostics::{
    DiagnosticData, DiagnosticOrigin, SourceRole, StructuredDiagnostic, TypeConstructorCarrierData,
    TypeDiagnosticReason,
};

impl Checker {
    fn collect_callable_value_tyvars(ty: &Ty, variables: &mut Vec<u32>) {
        match ty {
            Ty::Func(..) | Ty::ExtractorClosure(_) => Self::collect_ty_vars(ty, variables),
            Ty::List(inner) | Ty::Pid(inner) | Ty::Lazy(inner) | Ty::MatchResult(inner) => {
                Self::collect_callable_value_tyvars(inner, variables);
            }
            Ty::Tuple(items) | Ty::Enum(_, items) | Ty::SelfApp(items) => {
                for item in items {
                    Self::collect_callable_value_tyvars(item, variables);
                }
            }
            Ty::Result(ok, err) => {
                Self::collect_callable_value_tyvars(ok, variables);
                Self::collect_callable_value_tyvars(err, variables);
            }
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => {
                for argument in &nominal.arguments {
                    Self::collect_callable_value_tyvars(argument, variables);
                }
            }
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                for item in [source, focus, update_source, update_focus] {
                    Self::collect_callable_value_tyvars(item, variables);
                }
            }
            Ty::BuiltinFunc { .. }
            | Ty::UserFunc { .. }
            | Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::ProcessMarker(_)
            | Ty::Hole
            | Ty::Var(_)
            | Ty::Error => {}
        }
    }

    pub(super) fn require_concrete_callable_binding(
        &self,
        ty: &Ty,
        span: &Span,
    ) -> Result<(), TypeError> {
        let ty = self.resolve_ty(ty);
        let mut variables = Vec::new();
        Self::collect_callable_value_tyvars(&ty, &mut variables);
        if variables
            .iter()
            .all(|variable| self.rigid_tyvars.contains(variable))
        {
            return Ok(());
        }
        Err(TypeError {
            structured: None,
            message: "Callable binding requires a concrete signature".into(),
            span: span.clone(),
            hint: Some(
                "Add a concrete callable annotation, annotate the literal parameters, or pass the callable directly to a higher-order argument with a known expected type."
                    .into(),
            ),
        })
    }

    pub(in crate::checker) fn constructor_capability_allows(
        &self,
        actual: &str,
        required: &str,
        visiting: &mut HashSet<String>,
    ) -> bool {
        if actual == required || !visiting.insert(actual.to_string()) {
            return actual == required;
        }
        let allowed = self.traits.get(actual).is_some_and(|info| {
            info.parents.iter().any(|parent| {
                let parent_key = parent
                    .trait_id
                    .qualified_name
                    .as_deref()
                    .unwrap_or(parent.trait_id.name.as_str());
                self.constructor_capability_allows(parent_key, required, visiting)
            })
        });
        visiting.remove(actual);
        allowed
    }

    pub(super) fn check_constructor_capability(
        &self,
        required_trait: &str,
        method_name: &str,
        arg: &TypedNode,
        failure_reason: Option<TypeDiagnosticReason>,
    ) -> Result<(), TypeError> {
        let actual = self.constructor_capability_for_node(arg);
        let Some(required_info) = self.traits.get(required_trait) else {
            return Ok(());
        };
        if required_info.constructor_slots.is_empty() {
            return Ok(());
        }
        if let Some(outcome) = self.constructor_provenance_application_outcome(&actual) {
            self.require_constructor_projection_type(
                outcome,
                required_trait,
                &arg.ty,
                &arg.span,
                method_name,
            )?;
        }
        if self
            .constructor_provenance_allows(&actual, required_trait, &arg.ty)
            .map_err(|error| error.at_span(&arg.span))?
        {
            return Ok(());
        }
        Err(self.trait_failure(
            failure_reason.unwrap_or(TypeDiagnosticReason::MissingTypeConstructorCapability),
            required_trait,
            &arg.ty,
            &arg.span,
            DiagnosticOrigin::TraitCall,
        ))
    }

    pub(super) fn constructor_capability_failure_reason(&self, ty: &Ty) -> TypeDiagnosticReason {
        match self.resolve_ty(ty) {
            Ty::SelfApp(items)
                if Self::constructor_application_parts(&items).is_some_and(|(witness, _)| {
                    matches!(self.resolve_ty(witness), Ty::Var(var) if self.rigid_tyvars.contains(&var))
                }) =>
            {
                TypeDiagnosticReason::MissingGenericBound
            }
            _ => TypeDiagnosticReason::MissingTypeConstructorCapability,
        }
    }

    pub(in crate::checker) fn constructor_annotation_compatible(
        &mut self,
        trait_key: &str,
        expected: &Ty,
        actual: &Ty,
    ) -> Result<bool, TypeError> {
        let expected = self.resolve_ty(expected);
        let actual = self.resolve_ty(actual);
        // A concrete value can expose a constructor capability. An abstract
        // caller-owned carrier cannot be chosen by a nominal body annotation.
        let Ty::SelfApp(items) = &expected else {
            return Ok(false);
        };
        let Some((_, abstract_slots)) = Self::constructor_application_parts(items) else {
            return Ok(false);
        };
        let concrete_slots = match self
            .constructor_application_slots_for_trait(trait_key, &actual)
            .into_checked()?
        {
            ConstructorSlotsOutcome::Projected(slots) => slots,
            ConstructorSlotsOutcome::Deferred { .. } | ConstructorSlotsOutcome::Rejected { .. } => {
                return Ok(false);
            }
        };
        if abstract_slots.len() != concrete_slots.len() {
            return Ok(false);
        }

        let substitutions = self.substitutions.clone();
        let slots_ok = try_all(
            abstract_slots.iter().zip(concrete_slots.iter()),
            |(expected_slot, actual_slot)| self.types_compatible(expected_slot, actual_slot),
        );
        self.substitutions = substitutions;
        slots_ok
    }

    pub(super) fn check_expected_constructor_capability(
        &self,
        expected: &Ty,
        callee_label: &str,
        arg: &TypedNode,
    ) -> Result<(), TypeError> {
        self.check_parameter_constructor_provenance(expected, arg, callee_label)
    }

    pub(super) fn check_explicit_return_type_argument_apply(
        &mut self,
        span: &Span,
        target: &Resolved,
        args: &[AstTy],
    ) -> Result<TypedNode, TypeError> {
        if self.trait_method_ref(target).is_some() {
            return Err(TypeError {
                structured: None,
                message: "A specialized trait helper needs call or capture context".into(),
                span: span.clone(),
                hint: Some(
                    "Call it with arguments or capture it with an expected callable type.".into(),
                ),
            });
        }
        let (mut typed, signature, return_type_arguments) =
            self.instantiate_named_callable_signature(span, target, Some(args))?;
        let constraints =
            self.call_constraint_set(signature, return_type_arguments, None, span, &typed.ty);
        self.apply_return_type_argument_constraints(&constraints)?;
        typed.ty = self.callable_ty_from_signature(&typed.ty, &constraints.signature);
        Ok(typed)
    }

    pub(super) fn instantiate_named_callable_signature(
        &mut self,
        span: &Span,
        target: &Resolved,
        explicit: Option<&[AstTy]>,
    ) -> Result<
        (
            TypedNode,
            sindr::signature::CallableSignature<Ty>,
            Vec<super::signatures::TypeConstraint>,
        ),
        TypeError,
    > {
        let Resolved::Var(_, id) = target else {
            return Err(TypeError {
                structured: None,
                message: "Return type arguments require a named callable".into(),
                span: span.clone(),
                hint: None,
            });
        };
        let mut typed = self.check_node(target)?;
        let signature = self
            .callable_signatures
            .get(&id.unique_id)
            .cloned()
            .ok_or_else(|| super::signatures::missing_canonical_callable_signature(id, span))?;
        let (mut signature, call_substitution) =
            super::signatures::instantiate_callable_signature(self, &signature);
        let return_type_arguments =
            self.apply_call_site_return_type_arguments(span, &mut signature, explicit)?;
        if let Ty::UserFunc {
            call_substitution: target,
            ..
        } = &mut typed.ty
        {
            *target = call_substitution;
        }
        typed.ty = self.callable_ty_from_signature(&typed.ty, &signature);
        Ok((typed, signature, return_type_arguments))
    }

    pub(in crate::checker) fn check_registered_callable_application(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        args: &[ResolvedRecordLitArg],
        expected_return: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let target = Resolved::Var(id.span.clone(), id.clone());
        let (mut typed_func, signature, return_type_arguments) =
            self.instantiate_named_callable_signature(span, &target, None)?;
        let signature_params = signature
            .value_parameters
            .iter()
            .map(|parameter| parameter.ty.clone())
            .collect::<Vec<_>>();
        let callable_hint = self.call_target_signature_hint_for_id(
            id,
            &signature_params,
            &signature.return_type.ty,
        );
        let mut constraints = self.call_constraint_set(
            signature,
            return_type_arguments,
            expected_return,
            span,
            &typed_func.ty,
        );
        self.apply_return_type_argument_constraints(&constraints)?;
        self.apply_concrete_expected_call_constraint(&constraints, span, None)?;
        let typed_args = self.check_callable_application(
            id.unique_id,
            &id.name,
            &constraints.signature,
            args,
            span,
            Some(&callable_hint),
        )?;
        let substitution =
            match self.complete_call_constraint_set(&mut constraints, &typed_args, span, None) {
                super::signatures::SolveState::Solved(substitution) => substitution,
                super::signatures::SolveState::Deferred(pending) => pending.substitution,
                super::signatures::SolveState::Failed(error) => {
                    return Err(if error.hint.is_some() {
                        error
                    } else {
                        error.with_hint(callable_hint)
                    });
                }
            };
        if let Ty::UserFunc {
            call_substitution, ..
        } = &mut typed_func.ty
        {
            *call_substitution = substitution;
        }
        typed_func.ty = self.callable_ty_from_signature(&typed_func.ty, &constraints.signature);
        Ok(TypedNode {
            ty: self.resolve_ty(&constraints.signature.return_type.ty),
            span: span.clone(),
            node: TypedInner::App(Box::new(typed_func), typed_args),
        })
    }

    fn apply_call_site_return_type_arguments(
        &mut self,
        span: &Span,
        signature: &mut sindr::signature::CallableSignature<Ty>,
        explicit: Option<&[AstTy]>,
    ) -> Result<Vec<super::signatures::TypeConstraint>, TypeError> {
        let Some(explicit) = explicit else {
            return Ok(signature
                .return_type_arguments
                .iter()
                .map(|_| super::signatures::TypeConstraint::Infer { span: span.clone() })
                .collect());
        };
        if explicit.len() != signature.return_type_arguments.len() {
            return Err(super::signatures::return_type_argument_arity_error(
                &signature.identity.name,
                signature.return_type_arguments.len(),
                explicit.len(),
                span,
            ));
        }

        let mut constraints = Vec::with_capacity(explicit.len());
        for (slot, ast_ty) in signature.return_type_arguments.iter().zip(explicit) {
            let argument_span = Self::ast_ty_span(ast_ty).clone();
            if matches!(ast_ty, AstTy::Named(_, name) if Self::surface_name(name) == "_") {
                constraints.push(super::signatures::TypeConstraint::Infer {
                    span: argument_span,
                });
                continue;
            }
            let explicit_ty = self.resolve_call_site_return_type_argument(&slot.ty, ast_ty)?;
            constraints.push(super::signatures::TypeConstraint::Explicit {
                ty: explicit_ty,
                span: argument_span,
            });
        }
        Ok(constraints)
    }

    pub(super) fn apply_return_type_argument_constraints(
        &mut self,
        constraints: &super::signatures::CallConstraintSet,
    ) -> Result<(), TypeError> {
        for (slot, constraint) in constraints
            .signature
            .return_type_arguments
            .iter()
            .zip(&constraints.return_type_arguments)
        {
            let Some(explicit_ty) = constraint.explicit_ty() else {
                continue;
            };
            let relation_failed = match slot.ty {
                Ty::Var(variable) if self.constructor_witness_traits.contains_key(&variable) => {
                    let checkpoint = self.candidate_probe_checkpoint();
                    let matched = self.match_explicit_constructor_occurrence(variable, explicit_ty);
                    if !matches!(matched, Ok(true)) {
                        self.rollback_candidate_probe(checkpoint);
                    }
                    !matched.map_err(|error| error.at_span(constraint.span()))?
                }
                _ => self
                    .assert_type_relation(
                        &slot.ty,
                        explicit_ty,
                        self.type_fact(SourceRole::Contract, constraint.span(), &slot.ty),
                        self.type_fact(
                            SourceRole::ReturnTypeArgument,
                            constraint.span(),
                            explicit_ty,
                        ),
                        TypeDiagnosticReason::ArgumentTypeMismatch,
                        DiagnosticOrigin::Call,
                        &constraints.signature.identity.name,
                        slot.ordinal,
                    )
                    .is_err(),
            };
            if relation_failed {
                return Err(super::signatures::return_type_argument_mismatch_error(
                    &constraints.signature.identity.name,
                    slot.ordinal,
                    &self.diagnostic_ty_name(&slot.ty),
                    &self.diagnostic_ty_name(explicit_ty),
                    constraint.span(),
                    SourceRole::Contract,
                    constraint.span(),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn callable_ty_from_signature(
        &self,
        original: &Ty,
        signature: &sindr::signature::CallableSignature<Ty>,
    ) -> Ty {
        let params = signature
            .value_parameters
            .iter()
            .map(|parameter| self.resolve_ty(&parameter.ty))
            .collect::<Vec<_>>();
        let ret = Box::new(self.resolve_ty(&signature.return_type.ty));
        match original {
            Ty::BuiltinFunc { name, .. } => Ty::BuiltinFunc {
                name: name.clone(),
                params,
                ret,
            },
            Ty::UserFunc {
                fun_idx,
                type_params,
                call_substitution,
                ..
            } => Ty::UserFunc {
                fun_idx: *fun_idx,
                type_params: type_params.clone(),
                call_substitution: call_substitution
                    .iter()
                    .map(|(var, ty)| (*var, self.resolve_ty(ty)))
                    .collect(),
                params,
                ret,
            },
            _ => Ty::Func(params, ret),
        }
    }

    pub(super) fn call_constraint_set(
        &self,
        signature: sindr::signature::CallableSignature<Ty>,
        return_type_arguments: Vec<super::signatures::TypeConstraint>,
        expected_return: Option<&Ty>,
        span: &Span,
        callable_ty: &Ty,
    ) -> super::signatures::CallConstraintSet {
        let expected_return =
            expected_return.map(|ty| super::signatures::TypeConstraint::Explicit {
                ty: ty.clone(),
                span: span.clone(),
            });
        let mut origins = return_type_arguments
            .iter()
            .map(|constraint| {
                super::signatures::ConstraintOrigin::ReturnTypeArgument(constraint.span().clone())
            })
            .collect::<Vec<_>>();
        if let Some(constraint) = &expected_return {
            origins.push(super::signatures::ConstraintOrigin::ExpectedReturn(
                constraint.span().clone(),
            ));
        }
        origins.extend(
            signature
                .where_constraints
                .constraints
                .iter()
                .map(|constraint| {
                    super::signatures::ConstraintOrigin::Obligation(constraint.origin.clone())
                }),
        );
        super::signatures::CallConstraintSet {
            obligations: signature.where_constraints.constraints.clone(),
            substitution: match callable_ty {
                Ty::UserFunc {
                    call_substitution, ..
                } => call_substitution.clone(),
                _ => Vec::new(),
            },
            signature,
            return_type_arguments,
            value_arguments: Vec::new(),
            expected_return,
            origins,
        }
    }

    pub(super) fn apply_expected_call_constraint(
        &mut self,
        constraints: &super::signatures::CallConstraintSet,
        span: &Span,
        explicit: Option<&[AstTy]>,
    ) -> Result<(), TypeError> {
        let Some(expected) = constraints
            .expected_return
            .as_ref()
            .and_then(super::signatures::TypeConstraint::explicit_ty)
        else {
            return Ok(());
        };
        if self.callable_error_return_widens(expected, &constraints.signature.return_type.ty)? {
            return Ok(());
        }
        self.assert_type_relation(
            expected,
            &constraints.signature.return_type.ty,
            self.type_fact(SourceRole::Expected, span, expected),
            self.type_fact(
                SourceRole::Value,
                span,
                &constraints.signature.return_type.ty,
            ),
            TypeDiagnosticReason::ReturnTypeMismatch,
            DiagnosticOrigin::Return,
            &constraints.signature.identity.name,
            0,
        )
        .map_err(|_| {
            self.call_expected_return_mismatch_error(constraints, expected, span, explicit)
        })
    }

    /// A result context whose variables are fixed by the enclosing declaration
    /// may safely shape closure and constructor arguments before their bodies
    /// are checked.  Fresh inference variables must wait until value arguments
    /// have contributed their constraints, otherwise generic bounds are proved
    /// too early.
    pub(super) fn apply_concrete_expected_call_constraint(
        &mut self,
        constraints: &super::signatures::CallConstraintSet,
        span: &Span,
        explicit: Option<&[AstTy]>,
    ) -> Result<(), TypeError> {
        let Some(expected) = constraints
            .expected_return
            .as_ref()
            .and_then(super::signatures::TypeConstraint::explicit_ty)
        else {
            return Ok(());
        };
        let normalized_expected =
            self.normalize_executable_constructor_identities(&self.resolve_ty(expected));
        let Ok(canonical_expected) = self.canonical_request(&normalized_expected) else {
            return Ok(());
        };
        fn is_stable_context(ty: &CanonicalTy, rigid: &HashSet<u32>) -> bool {
            match ty.head {
                CanonicalTypeHead::Variable(variable) => rigid.contains(&variable),
                CanonicalTypeHead::SelfApplication => false,
                _ => ty
                    .arguments
                    .iter()
                    .all(|argument| is_stable_context(argument, rigid)),
            }
        }
        fn needs_context(ty: &Ty) -> bool {
            match ty {
                Ty::Func(..) | Ty::BuiltinFunc { .. } | Ty::UserFunc { .. } => true,
                Ty::SelfApp(items) => Checker::constructor_application_parts(items).is_some(),
                Ty::List(inner)
                | Ty::MatchResult(inner)
                | Ty::ExtractorClosure(inner)
                | Ty::Lazy(inner)
                | Ty::Pid(inner) => needs_context(inner),
                Ty::Result(ok, error) => needs_context(ok) || needs_context(error),
                Ty::Tuple(items) => items.iter().any(needs_context),
                Ty::Struct(_, nominal) | Ty::Record(_, nominal) => {
                    nominal.arguments.iter().any(needs_context)
                }
                Ty::Enum(_, arguments) => arguments.iter().any(needs_context),
                Ty::Facet(_, source, focus, update_source, update_focus) => {
                    needs_context(source)
                        || needs_context(focus)
                        || needs_context(update_source)
                        || needs_context(update_focus)
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
        if !constraints
            .signature
            .value_parameters
            .iter()
            .any(|parameter| needs_context(&parameter.ty))
        {
            return Ok(());
        }
        if !is_stable_context(&canonical_expected, &self.rigid_tyvars) {
            return Ok(());
        }
        if self.callable_error_return_widens(
            &normalized_expected,
            &constraints.signature.return_type.ty,
        )? {
            return Ok(());
        }
        self.assert_type_relation(
            &normalized_expected,
            &constraints.signature.return_type.ty,
            self.type_fact(SourceRole::Expected, span, &normalized_expected),
            self.type_fact(
                SourceRole::Value,
                span,
                &constraints.signature.return_type.ty,
            ),
            TypeDiagnosticReason::ReturnTypeMismatch,
            DiagnosticOrigin::Return,
            &constraints.signature.identity.name,
            0,
        )
        .map_err(|_| {
            self.call_expected_return_mismatch_error(constraints, expected, span, explicit)
        })
    }

    pub(super) fn complete_call_constraint_set(
        &mut self,
        constraints: &mut super::signatures::CallConstraintSet,
        typed_args: &[TypedNode],
        span: &Span,
        explicit: Option<&[AstTy]>,
    ) -> super::signatures::SolveState<Vec<(u32, Ty)>> {
        constraints.value_arguments = constraints
            .signature
            .value_parameters
            .iter()
            .zip(typed_args)
            .enumerate()
            .map(
                |(ordinal, (parameter, argument))| super::signatures::ValueArgumentConstraint {
                    ordinal: ordinal as u32,
                    expected: parameter.ty.clone(),
                    actual: argument.ty.clone(),
                    span: argument.span.clone(),
                },
            )
            .collect();
        constraints
            .origins
            .extend(constraints.value_arguments.iter().map(|argument| {
                super::signatures::ConstraintOrigin::ValueArgument(argument.span.clone())
            }));

        if let Err(error) = self.apply_expected_call_constraint(constraints, span, explicit) {
            return super::signatures::SolveState::Failed(error);
        }

        for argument in &constraints.value_arguments {
            let relation = self.assert_type_relation(
                &argument.expected,
                &argument.actual,
                self.type_fact(SourceRole::Expected, span, &argument.expected),
                self.type_fact(SourceRole::Value, &argument.span, &argument.actual),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Call,
                &constraints.signature.identity.name,
                argument.ordinal,
            );
            if let Err(error) = relation {
                if error.reason() != Some(TypeDiagnosticReason::ArgumentTypeMismatch) {
                    return super::signatures::SolveState::Failed(error);
                }
                if let Some(ordinal) = self.conflicting_return_type_argument_ordinal(
                    &argument.expected,
                    &argument.actual,
                    &constraints.signature,
                    &constraints.return_type_arguments,
                ) {
                    if let Some((_, ast_ty)) = explicit.and_then(|items| {
                        constraints
                            .signature
                            .return_type_arguments
                            .iter()
                            .position(|slot| slot.ordinal == ordinal)
                            .and_then(|index| items.get(index).map(|item| (index, item)))
                    }) {
                        return super::signatures::SolveState::Failed(
                            super::signatures::return_type_argument_mismatch_error(
                                &constraints.signature.identity.name,
                                ordinal,
                                &self.diagnostic_ty_name(&argument.actual),
                                &Self::surface_ast_ty(ast_ty),
                                Self::ast_ty_span(ast_ty),
                                SourceRole::Value,
                                &argument.span,
                            ),
                        );
                    }
                }
                if explicit.is_none() {
                    if let Ty::SelfApp(items) = &argument.expected {
                        if let Some((witness, _)) = Self::constructor_application_parts(items) {
                            let shared_witness_argument = match witness {
                                Ty::Var(witness_var) => constraints
                                    .value_arguments
                                    .iter()
                                    .take(argument.ordinal as usize)
                                    .find(|previous| {
                                        let Ty::SelfApp(previous_items) = &previous.expected else {
                                            return false;
                                        };
                                        matches!(
                                            Self::constructor_application_parts(previous_items),
                                            Some((Ty::Var(previous_var), _)) if previous_var == witness_var
                                        )
                                    }),
                                _ => None,
                            };
                            let family_trait = self
                                .constructor_capability_for_type(&argument.expected)
                                .expect(
                                    "constructor occurrence retains canonical capability metadata",
                                );
                            let expected_carrier = shared_witness_argument
                                .map(|previous| self.resolve_ty(&previous.actual))
                                .unwrap_or_else(|| self.resolve_ty(witness));
                            let actual_carrier = self.resolve_ty(&argument.actual);
                            let relation = self
                                .constructor_carrier_relation(
                                    &family_trait,
                                    &expected_carrier,
                                    &actual_carrier,
                                )
                                .into_checked();
                            let relation = match relation {
                                Ok(relation) => relation,
                                Err(error) => {
                                    return super::signatures::SolveState::Failed(
                                        error.at_span(&argument.span),
                                    );
                                }
                            };
                            let reason = match relation {
                                ConstructorCarrierRelation::SameCarrier => {
                                    TypeDiagnosticReason::TypePayloadMismatch
                                }
                                ConstructorCarrierRelation::DifferentCarrier => {
                                    TypeDiagnosticReason::TypeConstructorFamilyMismatch
                                }
                                ConstructorCarrierRelation::Deferred { .. }
                                | ConstructorCarrierRelation::Rejected { .. } => {
                                    return super::signatures::SolveState::Failed(error);
                                }
                            };
                            let left_ty = shared_witness_argument
                                .map(|previous| &previous.actual)
                                .unwrap_or(&argument.expected);
                            let left_span = shared_witness_argument
                                .map(|previous| &previous.span)
                                .unwrap_or(span);
                            return super::signatures::SolveState::Failed(
                                TypeError::from_structured(StructuredDiagnostic {
                                    reason: reason.into(),
                                    origin: DiagnosticOrigin::Call,
                                    data: DiagnosticData::TypeConstructorCarrier(
                                        TypeConstructorCarrierData {
                                            left_type: Some(
                                                self.diagnostic_ty_name(&self.resolve_ty(left_ty)),
                                            ),
                                            right_type: Some(
                                                self.diagnostic_ty_name(&actual_carrier),
                                            ),
                                            left_origin: Some(self.type_fact(
                                                SourceRole::LeftValue,
                                                left_span,
                                                left_ty,
                                            )),
                                            right_origin: Some(self.type_fact(
                                                SourceRole::RightValue,
                                                &argument.span,
                                                &actual_carrier,
                                            )),
                                            required_capability: self
                                                .trait_display_name(&family_trait),
                                            family: self.trait_display_name(&family_trait),
                                            family_id: self
                                                .diagnostic_constructor_family_id(&family_trait),
                                            expected_carrier: self.diagnostic_ty_name(
                                                &self.resolve_ty(&argument.expected),
                                            ),
                                            actual_carrier: self
                                                .diagnostic_ty_name(&actual_carrier),
                                        },
                                    ),
                                    primary: self.type_fact(
                                        SourceRole::RightValue,
                                        &argument.span,
                                        &actual_carrier,
                                    ),
                                    related: vec![self.type_fact(
                                        SourceRole::LeftValue,
                                        left_span,
                                        left_ty,
                                    )],
                                    remediation: None,
                                }),
                            );
                        }
                    }
                }
                return super::signatures::SolveState::Failed(error);
            }
        }

        for obligation in &constraints.obligations {
            let subject = self.resolve_ty(&obligation.subject);
            let trait_key = &obligation.trait_name;
            let obligation_satisfied = (|| -> Result<bool, TypeError> {
                let constructor_head_satisfies = matches!(
                    &obligation.subject,
                    Ty::Var(variable) if self.constructor_witness_traits.contains_key(variable)
                ) && matches!(
                    self.constructor_head_projection(trait_key, &subject)
                        .into_checked()?,
                    ConstructorProjectionOutcome::Applicable { .. }
                );
                Ok(constructor_head_satisfies
                    || self.ty_satisfies_bounds(&subject, std::slice::from_ref(trait_key))?)
            })();
            let obligation_satisfied = match obligation_satisfied {
                Ok(satisfied) => satisfied,
                Err(error) => return super::signatures::SolveState::Failed(error.at_span(span)),
            };
            if !obligation_satisfied {
                let reason = if matches!(subject, Ty::Var(var) if self.rigid_tyvars.contains(&var))
                {
                    TypeDiagnosticReason::MissingGenericBound
                } else {
                    TypeDiagnosticReason::MissingTraitCapability
                };
                return super::signatures::SolveState::Failed(self.trait_failure(
                    reason,
                    trait_key,
                    &subject,
                    span,
                    DiagnosticOrigin::Call,
                ));
            }
        }

        // Reading the closed provenance envelope here ensures every call path
        // retains origins for RTA, values, expected return, and obligations.
        for origin in &constraints.origins {
            let _ = (origin.span(), origin.signature_origin());
        }
        let mut unresolved_return_type_arguments = Vec::new();
        for (slot, constraint) in constraints
            .signature
            .return_type_arguments
            .iter()
            .zip(&constraints.return_type_arguments)
        {
            let resolved = self.resolve_ty(&slot.ty);
            if self.return_type_argument_is_unresolved(&slot.ty)
                && constraint.explicit_ty().is_none()
            {
                unresolved_return_type_arguments.push((slot.ordinal, resolved));
            }
        }
        let substitution = constraints
            .substitution
            .iter()
            .map(|(var, ty)| (*var, self.resolve_ty(ty)))
            .collect::<Vec<_>>();
        if unresolved_return_type_arguments.is_empty() {
            super::signatures::SolveState::Solved(substitution)
        } else {
            super::signatures::SolveState::Deferred(super::signatures::PendingConstraints {
                unresolved_return_type_arguments,
                substitution,
            })
        }
    }

    fn return_type_argument_is_unresolved(&self, ty: &Ty) -> bool {
        if let Ty::SelfApp(items) = ty {
            if let Some((witness, _)) = Self::constructor_application_parts(items) {
                let mut unresolved = Vec::new();
                Self::collect_ty_vars(&self.resolve_ty(witness), &mut unresolved);
                return !unresolved.is_empty();
            }
        }
        let mut unresolved = Vec::new();
        Self::collect_ty_vars(&self.resolve_ty(ty), &mut unresolved);
        !unresolved.is_empty()
    }

    fn call_expected_return_mismatch_error(
        &self,
        constraints: &super::signatures::CallConstraintSet,
        expected: &Ty,
        span: &Span,
        explicit: Option<&[AstTy]>,
    ) -> TypeError {
        let signature = &constraints.signature;
        let conflicting_ordinal = self.conflicting_return_type_argument_ordinal(
            &signature.return_type.ty,
            expected,
            signature,
            &constraints.return_type_arguments,
        );
        let explicit_item = explicit.and_then(|items| {
            conflicting_ordinal
                .and_then(|ordinal| {
                    signature
                        .return_type_arguments
                        .iter()
                        .position(|slot| slot.ordinal == ordinal)
                })
                .and_then(|index| items.get(index).map(|item| (index, item)))
        });
        if let Some((slot, ast_ty)) = explicit_item {
            let ordinal = signature
                .return_type_arguments
                .get(slot)
                .map(|argument| argument.ordinal)
                .unwrap_or(slot as u32);
            return super::signatures::return_type_argument_mismatch_error(
                &signature.identity.name,
                ordinal,
                &self.diagnostic_ty_name(expected),
                &Self::surface_ast_ty(ast_ty),
                Self::ast_ty_span(ast_ty),
                SourceRole::Expected,
                span,
            );
        }
        let error = self.type_relation_error(
            expected,
            &signature.return_type.ty,
            self.type_fact(SourceRole::Expected, span, expected),
            self.type_fact(SourceRole::Value, span, &signature.return_type.ty),
            TypeDiagnosticReason::ReturnTypeMismatch,
            DiagnosticOrigin::Return,
            &signature.identity.name,
            0,
        );
        if matches!(self.resolve_ty(&signature.return_type.ty), Ty::Unit) && error.hint.is_none() {
            let params = signature
                .value_parameters
                .iter()
                .map(|parameter| parameter.ty.clone())
                .collect::<Vec<_>>();
            return error.with_hint(format!(
                "{}\nThis call returns Unit; the surrounding context expects {}.",
                self.format_signature(
                    &signature.identity.name,
                    &params,
                    &signature.return_type.ty,
                ),
                self.diagnostic_ty_name(expected),
            ));
        }
        error
    }

    fn conflicting_return_type_argument_ordinal(
        &self,
        actual: &Ty,
        expected: &Ty,
        signature: &sindr::signature::CallableSignature<Ty>,
        constraints: &[super::signatures::TypeConstraint],
    ) -> Option<u32> {
        let explicit_slot_for_var = |var| {
            signature
                .return_type_arguments
                .iter()
                .zip(constraints)
                .find_map(|(slot, constraint)| {
                    constraint.explicit_ty()?;
                    let mut vars = Vec::new();
                    Self::collect_ty_vars(&slot.ty, &mut vars);
                    vars.into_iter()
                        .any(|slot_var| slot_var == var)
                        .then_some(slot.ordinal)
                })
        };

        fn find(
            checker: &Checker,
            actual: &Ty,
            expected: &Ty,
            explicit_slot_for_var: &impl Fn(u32) -> Option<u32>,
        ) -> Option<u32> {
            if let Ty::Var(var) = actual {
                return (checker.resolve_ty(actual) != checker.resolve_ty(expected))
                    .then(|| explicit_slot_for_var(*var))
                    .flatten();
            }
            if let Ty::SelfApp(items) = actual {
                if let Some((witness, _)) = Checker::constructor_application_parts(items) {
                    if checker.resolve_ty(actual) != checker.resolve_ty(expected) {
                        let mut vars = Vec::new();
                        Checker::collect_ty_vars(witness, &mut vars);
                        if let Some(ordinal) = vars.into_iter().find_map(explicit_slot_for_var) {
                            return Some(ordinal);
                        }
                    }
                }
            }
            match (actual, expected) {
                (Ty::MatchResult(left), Ty::MatchResult(right))
                | (Ty::ExtractorClosure(left), Ty::ExtractorClosure(right))
                | (Ty::List(left), Ty::List(right))
                | (Ty::Lazy(left), Ty::Lazy(right))
                | (Ty::Pid(left), Ty::Pid(right)) => {
                    find(checker, left, right, explicit_slot_for_var)
                }
                (Ty::Result(left_ok, left_err), Ty::Result(right_ok, right_err)) => {
                    find(checker, left_ok, right_ok, explicit_slot_for_var)
                        .or_else(|| find(checker, left_err, right_err, explicit_slot_for_var))
                }
                (Ty::Tuple(left), Ty::Tuple(right))
                | (Ty::SelfApp(left), Ty::SelfApp(right))
                | (Ty::Enum(_, left), Ty::Enum(_, right)) => left
                    .iter()
                    .zip(right)
                    .find_map(|(left, right)| find(checker, left, right, explicit_slot_for_var)),
                (Ty::Func(left_params, left_ret), Ty::Func(right_params, right_ret)) => left_params
                    .iter()
                    .zip(right_params)
                    .find_map(|(left, right)| find(checker, left, right, explicit_slot_for_var))
                    .or_else(|| find(checker, left_ret, right_ret, explicit_slot_for_var)),
                (Ty::Struct(_, left), Ty::Struct(_, right))
                | (Ty::Record(_, left), Ty::Record(_, right)) => left
                    .arguments
                    .iter()
                    .zip(&right.arguments)
                    .find_map(|(left, right)| find(checker, left, right, explicit_slot_for_var)),
                _ => None,
            }
        }

        find(self, actual, expected, &explicit_slot_for_var)
    }

    #[inline(never)]
    pub(super) fn check_application_with_expected_type(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
        expected_ty: &Ty,
    ) -> Result<TypedNode, TypeError> {
        // Keep the original contextual dispatch priority: constructors,
        // trait helpers, Function on calls, and then ordinary applications.
        if matches!(func, Resolved::Var(_, id)
            if self.lookup_enum_variant_by_constructor_id(id.unique_id).is_some())
        {
            let Resolved::Var(_, id) = func else {
                unreachable!("constructor guard requires a variable callee")
            };
            self.check_constructor_call(span, id, args, Some(expected_ty))
        } else if self.trait_method_ref(func).is_some() {
            let (id, trait_name, method_name) = self
                .trait_method_ref(func)
                .expect("guard established a trait method reference");
            let receiver_owner_hint = id
                .name
                .strip_suffix(&format!("::{}", method_name))
                .filter(|owner| *owner == "JsonValue");
            let explicit_type_arguments = Self::explicit_type_args(func);
            self.check_trait_method_call(
                span,
                &trait_name,
                &method_name,
                args,
                receiver_owner_hint,
                Some(expected_ty),
                explicit_type_arguments.as_deref(),
            )
        } else if self.is_function_on_callee(func) {
            self.check_function_on_with_expected(span, func, args, expected_ty)
        } else {
            self.check_app_with_expected(span, func, args, Some(expected_ty))
        }
    }

    // Imported references retain their declaration UID; same-named local bindings do not.
    // Select standard type-dependent behavior by that UID, never by call-site spelling.
    fn is_standard_callable(&self, func: &Resolved, qualified_name: &str) -> bool {
        let target = match func {
            Resolved::ReturnTypeArgumentApply(_, target, _) => target.as_ref(),
            _ => func,
        };
        let Resolved::Var(_, id) = target else {
            return false;
        };
        self.function_ids_by_name
            .get(qualified_name)
            .is_some_and(|declaration| declaration.unique_id == id.unique_id)
    }

    fn is_function_on_callee(&self, func: &Resolved) -> bool {
        self.is_standard_callable(func, "Function::on")
    }

    pub(super) fn is_function_curry_callee(&self, func: &Resolved) -> bool {
        self.is_standard_callable(func, "Function::curry")
    }

    fn curry_source_captures(node: &TypedNode, out: &mut Vec<ResolvedId>) {
        let mut add = |id: &ResolvedId| {
            if !out
                .iter()
                .any(|existing| existing.unique_id == id.unique_id)
            {
                out.push(id.clone());
            }
        };
        match &node.node {
            TypedInner::Var(id) => add(id),
            TypedInner::Capture(target, _) => Self::curry_source_captures(target, out),
            TypedInner::Closure(_, captures, _)
            | TypedInner::ExtractorClosure(_, captures, _)
            | TypedInner::CaptureClosure(_, captures, _) => captures.iter().for_each(&mut add),
            TypedInner::CaptureConstructorClosure(_, _, captures, _) => {
                captures.iter().for_each(&mut add)
            }
            _ => {}
        }
    }

    fn check_function_curry(
        &mut self,
        span: &Span,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        let [ResolvedRecordLitArg::Positional(source)] = args else {
            return Err(TypeError {
                structured: None,
                message: "Function::curry expects exactly one positional callable".into(),
                span: span.clone(),
                hint: None,
            });
        };
        if matches!(source, Resolved::App(..)) {
            return Err(TypeError {
                structured: None,
                message: "Function::curry requires a function value, not a function call".into(),
                span: self.resolved_span(source).clone(),
                hint: Some(
                    "Use `&f`, `&Type::method`, a closure, or a bound callable value.".into(),
                ),
            });
        }

        let trait_capture = match source {
            Resolved::Capture(_, target, _) => self.trait_method_ref(target).is_some(),
            _ => false,
        };
        let typed_source = if trait_capture {
            // Trait helper captures need a callable context to resolve their
            // receiver. The current trait-helper surface used by Function is
            // binary; the resulting signature is then validated below.
            let expected = Ty::Func(
                vec![self.env.fresh_tyvar(), self.env.fresh_tyvar()],
                Box::new(self.env.fresh_tyvar()),
            );
            self.check_node_with_expected(source, Some(&expected))?
        } else {
            self.check_node(source)?
        };
        let source_ty = self.resolve_ty(&typed_source.ty);
        let (params, ret) = match &source_ty {
            Ty::Func(params, ret) => (params.clone(), ret.as_ref().clone()),
            // A named function is only a valid curry input through an explicit
            // capture. A bare function name must retain the language's existing
            // function-value restriction.
            Ty::UserFunc { .. } | Ty::BuiltinFunc { .. } => {
                return Err(TypeError {
                    structured: None,
                    message: "Function::curry requires an explicit function capture".into(),
                    span: self.resolved_span(source).clone(),
                    hint: Some(
                        "Use `curry(&function)` or bind a closure/callable value first.".into(),
                    ),
                });
            }
            _ => {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Function::curry requires a function value, got {}",
                        self.ty_name(&source_ty)
                    ),
                    span: typed_source.span.clone(),
                    hint: None,
                });
            }
        };
        if params.len() < 2 {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Function::curry requires a function with at least two arguments, got {}",
                    params.len()
                ),
                span: typed_source.span.clone(),
                hint: None,
            });
        }

        let mut source_captures = Vec::new();
        Self::curry_source_captures(&typed_source, &mut source_captures);
        let mut parameter_ids = Vec::with_capacity(params.len());
        for index in 0..params.len() {
            parameter_ids.push(ResolvedId {
                name: format!("__curry_arg_{index}"),
                qualified_name: None,
                symbol_info: None,
                unique_id: Self::next_synthetic_range_uid(),
                compiler_generated: true,
                span: span.clone(),
            });
        }

        let mut callable = typed_source;
        let mut result_ty = ret;
        for index in (0..params.len()).rev() {
            let mut call_args = Vec::with_capacity(params.len());
            for (arg_index, param_ty) in params.iter().enumerate() {
                let arg_ty = self.resolve_ty(param_ty);
                call_args.push(TypedNode {
                    ty: arg_ty,
                    span: span.clone(),
                    node: TypedInner::Var(parameter_ids[arg_index].clone()),
                });
            }
            if index < params.len() - 1 {
                // The nested closure is already the value produced by the
                // suffix of the chain; only the outermost body calls the
                // original callable.
                call_args.clear();
            }
            let body = if index == params.len() - 1 {
                TypedNode {
                    ty: self.resolve_ty(&result_ty),
                    span: span.clone(),
                    node: TypedInner::App(Box::new(callable.clone()), call_args),
                }
            } else {
                callable
            };
            let mut captures = source_captures.clone();
            captures.extend(parameter_ids[..index].iter().cloned());
            let closure_ty = Ty::Func(
                vec![self.resolve_ty(&params[index])],
                Box::new(self.resolve_ty(&result_ty)),
            );
            callable = TypedNode {
                ty: closure_ty,
                span: span.clone(),
                node: TypedInner::Closure(
                    vec![TypedClosureParam {
                        id: parameter_ids[index].clone(),
                        ty: self.resolve_ty(&params[index]),
                    }],
                    captures,
                    Box::new(body),
                ),
            };
            result_ty = callable.ty.clone();
        }
        Ok(callable)
    }

    fn check_function_on_with_expected(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
        expected_ty: &Ty,
    ) -> Result<TypedNode, TypeError> {
        let expected_ty = self.resolve_ty(expected_ty);
        let Ty::Func(params, ret) = &expected_ty else {
            return self.check_app(span, func, args);
        };
        if params.len() != 2 || !self.types_compatible(&params[0], &params[1])? {
            return self.check_app(span, func, args);
        }
        let [ResolvedRecordLitArg::Positional(compare_expr), ResolvedRecordLitArg::Positional(key_expr)] =
            args
        else {
            return self.check_app(span, func, args);
        };

        let source_ty = self.resolve_ty(&params[0]);
        let key_ty = self.env.fresh_tyvar();
        let key_expected = Ty::Func(vec![source_ty.clone()], Box::new(key_ty.clone()));
        let typed_key = self.check_node_with_expected(key_expr, Some(&key_expected))?;
        self.assert_type_relation(
            &key_expected,
            &typed_key.ty,
            self.type_fact(SourceRole::Expected, span, &key_expected),
            self.type_fact(SourceRole::Value, &typed_key.span, &typed_key.ty),
            TypeDiagnosticReason::ArgumentTypeMismatch,
            DiagnosticOrigin::Call,
            "Function::on",
            1,
        )?;
        let key_ty = self.resolve_ty(&key_ty);
        let compare_expected = Ty::Func(
            vec![self.resolve_ty(&key_ty), self.resolve_ty(&key_ty)],
            Box::new(self.resolve_ty(ret.as_ref())),
        );
        let typed_compare = self.check_node_with_expected(compare_expr, Some(&compare_expected))?;
        self.assert_type_relation(
            &compare_expected,
            &typed_compare.ty,
            self.type_fact(SourceRole::Expected, span, &compare_expected),
            self.type_fact(SourceRole::Value, &typed_compare.span, &typed_compare.ty),
            TypeDiagnosticReason::ArgumentTypeMismatch,
            DiagnosticOrigin::Call,
            "Function::on",
            0,
        )?;

        let typed_func = self.check_node(func)?;
        let result_ty = Ty::Func(
            vec![source_ty.clone(), source_ty],
            Box::new(self.resolve_ty(ret)),
        );
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::App(Box::new(typed_func), vec![typed_compare, typed_key]),
        })
    }

    pub(in crate::checker) fn check_function_value_operand(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        let typed = self.check_node(node)?;
        if matches!(self.resolve_ty(&typed.ty), Ty::Func(_, _)) {
            Ok(typed)
        } else {
            Err(self
                .callable_shape_error(&typed.ty, op_name, &typed.span, Some(1))
                .with_hint(self.compose_function_value_hint(&typed, op_name)))
        }
    }

    fn compose_call_signature_hint(&self, func: &TypedNode) -> Option<String> {
        match (&func.node, self.resolve_ty(&func.ty)) {
            (TypedInner::Var(id), Ty::BuiltinFunc { params, ret, .. })
            | (TypedInner::Var(id), Ty::UserFunc { params, ret, .. }) => {
                Some(self.call_target_signature_hint_for_id(id, &params, ret.as_ref()))
            }
            (_, Ty::BuiltinFunc { name, params, ret }) => {
                Some(self.call_target_signature_hint(&name, &params, ret.as_ref(), None))
            }
            (_, Ty::UserFunc { params, ret, .. }) => {
                self.callable_definition_signature_hint(func, &params, ret.as_ref())
            }
            (_, Ty::Func(params, ret)) => {
                self.callable_signature_hint(&Ty::Func(params.clone(), ret.clone()))
            }
            _ => None,
        }
    }

    fn compose_function_value_hint(&self, typed: &TypedNode, op_name: &str) -> String {
        if let TypedInner::App(func, _) = &typed.node {
            let signature = self
                .compose_call_signature_hint(func)
                .unwrap_or_else(|| "Call target signature: <unknown>".into());
            format!(
                "{}\n`{}` evaluates this call before composition; the result type {} is not a function value.",
                signature,
                op_name,
                self.diagnostic_ty_name(&typed.ty)
            )
        } else {
            format!(
                "{} works on one-argument function values. Bare function names are not function values; use `&name`, a closure, a function-typed variable, or a call that returns a function value.",
                op_name
            )
        }
    }

    pub(in crate::checker) fn resolved_span<'a>(&self, node: &'a Resolved) -> &'a Span {
        match node {
            Resolved::Lit(span, _)
            | Resolved::Var(span, _)
            | Resolved::ErrorKind(span, _)
            | Resolved::App(span, _, _)
            | Resolved::LazyApp(span, _, _, _)
            | Resolved::ReturnTypeArgumentApply(span, _, _)
            | Resolved::Block(span, _)
            | Resolved::Bind(span, _, _)
            | Resolved::ApplyPattern(span, _, _)
            | Resolved::SafeBind(span, _, _, _)
            | Resolved::StatementQuestion(span, _)
            | Resolved::Do(span, _, _, _, _)
            | Resolved::BinOp(span, _, _, _)
            | Resolved::Pipe(span, _, _)
            | Resolved::ContextMap(span, _, _)
            | Resolved::ContextApply(span, _, _)
            | Resolved::ContextBind(span, _, _)
            | Resolved::Compose(span, _, _)
            | Resolved::LiftedCompose(span, _, _)
            | Resolved::KleisliCompose(span, _, _)
            | Resolved::ListNil(span)
            | Resolved::ListCons(span, _, _)
            | Resolved::ListLiteral(span, _)
            | Resolved::HashMapLiteral(span, _)
            | Resolved::RangeLiteral(span, _, _)
            | Resolved::TupleLiteral(span, _)
            | Resolved::Grouped(span, _)
            | Resolved::InterpolatedStr(span, _)
            | Resolved::Dbg(span, _)
            | Resolved::Cond(span, _)
            | Resolved::If(span, _, _, _)
            | Resolved::Cause(span, _, _)
            | Resolved::IfLet(span, _, _, _)
            | Resolved::AssertErrorMatches(span, _, _)
            | Resolved::Match(span, _, _)
            | Resolved::IsMatch(span, _, _)
            | Resolved::FieldAccess(span, _, _)
            | Resolved::FacetSegmentAccess(span, _, _)
            | Resolved::InferredFacetCapture(span, _)
            | Resolved::FacetCapture(span, _)
            | Resolved::ProcessContextHandler(span, _)
            | Resolved::StructLit(span, _, _)
            | Resolved::ConstructorCall(span, _, _)
            | Resolved::EnumErrorConstructorCall(span, _, _, _)
            | Resolved::EnumConstructorCall(span, _, _, _)
            | Resolved::StructDef(span, ..)
            | Resolved::RecordDef(span, _, _, _)
            | Resolved::DeferrorDef(span, _, _, _)
            | Resolved::EnumDef(span, _, _, _, _)
            | Resolved::Def(span, _, _, _, _, _, _, _)
            | Resolved::ConstDef(span, _, _, _, _)
            | Resolved::ExtractorDef(span, _, _, _, _, _, _)
            | Resolved::BuiltinDecl(span, _, _, _, _, _, _)
            | Resolved::BuiltinExtractorDecl(span, _, _, _, _)
            | Resolved::BuiltinTypeDecl(span, _, _, _)
            | Resolved::TypeAlias(span, _, _, _, _)
            | Resolved::TraitDef(span, _, _, _, _, _)
            | Resolved::TraitImplDef(span, _, _, _, _, _, _)
            | Resolved::Closure(span, _, _, _)
            | Resolved::ExtractorClosure(span, _, _, _)
            | Resolved::CaptureClosure(span, _, _, _)
            | Resolved::Capture(span, _, _)
            | Resolved::Semi(span, _) => span,
        }
    }

    pub(in crate::checker) fn function_parts<'a>(
        &'a self,
        ty: &'a Ty,
    ) -> Option<(&'a [Ty], &'a Ty)> {
        match ty {
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                Some((params.as_slice(), ret.as_ref()))
            }
            Ty::Func(params, ret) => Some((params.as_slice(), ret.as_ref())),
            _ => None,
        }
    }

    pub(in crate::checker) fn callable_signature_from_parts(
        &self,
        params: &[Ty],
        ret: &Ty,
    ) -> String {
        let ty_refs = params
            .iter()
            .map(|ty| ty as &Ty)
            .chain(std::iter::once(ret))
            .collect::<Vec<_>>();
        let rendered = self.diagnostic_ty_names(&ty_refs);
        let (rendered_params, rendered_ret) = rendered.split_at(params.len());
        let param_str = rendered_params.join(", ");
        if param_str.is_empty() {
            format!("(-> {})", rendered_ret[0])
        } else {
            format!("({} -> {})", param_str, rendered_ret[0])
        }
    }

    pub(in crate::checker) fn callable_signature_for_ty(&self, ty: &Ty) -> Option<String> {
        self.function_parts(ty)
            .map(|(params, ret)| self.callable_signature_from_parts(params, ret))
    }

    pub(in crate::checker) fn callable_signature_hint(&self, ty: &Ty) -> Option<String> {
        self.callable_signature_for_ty(ty)
            .map(|sig| format!("Callable type signature: {}", sig))
    }

    pub(in crate::checker) fn call_target_signature_hint(
        &self,
        display_name: &str,
        params: &[Ty],
        ret: &Ty,
        first_param_name: Option<&str>,
    ) -> String {
        let ty_refs = params
            .iter()
            .map(|ty| ty as &Ty)
            .chain(std::iter::once(ret))
            .collect::<Vec<_>>();
        let rendered = self.diagnostic_ty_names(&ty_refs);
        let (rendered_params, rendered_ret) = rendered.split_at(params.len());
        let param_list = params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                let name = match (idx, first_param_name) {
                    (0, Some(name)) => name.to_string(),
                    _ => format!("arg{}", idx + 1),
                };
                let rendered_ty = &rendered_params[idx];
                let _ = ty;
                format!("{}: {}", name, rendered_ty)
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "Call target signature: {}({}) -> {}",
            display_name, param_list, rendered_ret[0]
        )
    }

    pub(in crate::checker) fn callable_target_display_name(&self, id: &ResolvedId) -> String {
        if let Some(qualified_name) = id.qualified_name.as_deref() {
            return callable_definition_display_name(qualified_name, &id.name);
        }
        if sindr::builtin::builtin_meta_by_name(&id.name).is_some() {
            return format!("Kernel::{}", id.name);
        }
        id.name.clone()
    }

    pub(in crate::checker) fn call_target_signature_hint_for_id(
        &self,
        id: &ResolvedId,
        params: &[Ty],
        ret: &Ty,
    ) -> String {
        let display_name = self.callable_target_display_name(id);
        self.call_target_signature_hint(&display_name, params, ret, None)
    }

    pub(in crate::checker) fn trait_method_signature_hint(
        &self,
        trait_display_name: &str,
        method_name: &str,
        params: &[Ty],
        ret: &Ty,
    ) -> String {
        self.call_target_signature_hint(
            &format!("{}::{}", trait_display_name, method_name),
            params,
            ret,
            Some("self"),
        )
    }

    pub(in crate::checker) fn callable_definition_signature_hint(
        &self,
        func: &TypedNode,
        params: &[Ty],
        ret: &Ty,
    ) -> Option<String> {
        let TypedInner::Var(id) = &func.node else {
            return None;
        };
        let qualified_name = id.qualified_name.as_deref().unwrap_or(&id.name);
        let display_name = callable_definition_display_name(qualified_name, &id.name);
        let mut hint = format!(
            "Callable definition signature: {}",
            self.callable_definition_signature(&display_name, id.unique_id, params, ret)
        );
        if let Some(def_span) = self
            .function_ids_by_name
            .values()
            .find(|decl| decl.unique_id == id.unique_id)
            .map(|decl| decl.span.clone())
        {
            hint.push_str(&format!(
                "\nCallable definition span: {}..{}",
                def_span.start, def_span.end
            ));
        }
        Some(hint)
    }

    fn callable_definition_signature(
        &self,
        qualified_name: &str,
        uid: u32,
        params: &[Ty],
        ret: &Ty,
    ) -> String {
        let param_names = self.user_func_params.get(&uid);
        let param_list = params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                let name = param_names
                    .and_then(|names| names.get(idx))
                    .map(String::as_str)
                    .unwrap_or("_");
                format!("{}: {}", name, self.ty_name(ty))
            })
            .collect::<Vec<_>>()
            .join(", ");
        let ret = self.ty_name(ret);

        if let Some(display) = trait_impl_signature_display(qualified_name, &param_list, &ret) {
            return display;
        }

        format!("{}({}) -> {}", qualified_name, param_list, ret)
    }

    pub(in crate::checker) fn unary_function_parts(
        &self,
        ty: &Ty,
        callable: &str,
        span: &Span,
    ) -> Result<(Ty, Ty), TypeError> {
        let Some((params, ret)) = self.function_parts(ty) else {
            return Err(self.callable_shape_error(ty, callable, span, Some(1)));
        };
        if params.len() != 1 {
            return Err(self.callable_shape_error(ty, callable, span, Some(1)));
        }
        Ok((self.resolve_ty(&params[0]), self.resolve_ty(ret)))
    }

    pub(in crate::checker) fn trait_method_ref<'a>(
        &self,
        func: &'a Resolved,
    ) -> Option<(&'a ResolvedId, String, String)> {
        let func = match func {
            Resolved::ReturnTypeArgumentApply(_, target, _) => target.as_ref(),
            other => other,
        };
        let Resolved::Var(_, id) = func else {
            return None;
        };
        let qualified_name = id.qualified_name.as_ref()?;
        let (trait_name, method_name) = self
            .trait_methods_by_qualified_name
            .get(qualified_name)?
            .clone();
        Some((id, trait_name, method_name))
    }

    pub(super) fn explicit_type_args(func: &Resolved) -> Option<Vec<AstTy>> {
        match func {
            Resolved::ReturnTypeArgumentApply(_, _, args) => Some(
                args.iter()
                    .map(|argument| argument.ty.syntax.clone())
                    .collect(),
            ),
            _ => None,
        }
    }

    pub(in crate::checker) fn check_injected_call(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArgumentModeMismatch,
                    op_name,
                    None,
                    args.len(),
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: format!(
                    "{} does not support named arguments on the right-hand side",
                    op_name
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let explicit = Self::explicit_type_args(func);
        let named_target = match func {
            Resolved::ReturnTypeArgumentApply(_, target, _) => target.as_ref(),
            other => other,
        };
        let has_named_signature = match named_target {
            Resolved::Var(_, id) => {
                self.callable_signatures.contains_key(&id.unique_id)
                    || self.is_registered_callable_declaration(id.unique_id)
            }
            _ => false,
        };
        let typed_func = if explicit.is_some() || has_named_signature {
            let (mut typed, signature, return_type_arguments) =
                self.instantiate_named_callable_signature(span, named_target, explicit.as_deref())?;
            let constraints =
                self.call_constraint_set(signature, return_type_arguments, None, span, &typed.ty);
            self.apply_return_type_argument_constraints(&constraints)?;
            typed.ty = self.callable_ty_from_signature(&typed.ty, &constraints.signature);
            typed
        } else {
            self.check_node(func)?
        };
        let (params, ret) = match self.resolve_ty(&typed_func.ty) {
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                (params, ret.as_ref().clone())
            }
            Ty::Func(params, ret) => (params, ret.as_ref().clone()),
            other => {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "{} right-hand side is not a function call target: {}",
                        op_name,
                        self.ty_name(&other)
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }
        };

        if params.len() != args.len() + 1 {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{} injects the left value as the first argument, so the call expects {} explicit argument(s), got {}",
                    op_name,
                    params.len().saturating_sub(1),
                    args.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }

        if matches!(self.resolve_ty(&params[0]), Ty::Lazy(_)) {
            return Err(TypeError {
                structured: None,
                message: "pipe injection into a Lazy parameter is not allowed".into(),
                span: span.clone(),
                hint: Some(
                    "Lazy parameters are evaluated under the callee's control. Bind the value first, then pass that binding explicitly, or use a closure to make the evaluation order explicit."
                        .into(),
                ),
            });
        }

        let typed_args: Vec<TypedNode> = args
            .iter()
            .zip(params.iter().skip(1))
            .map(|(arg, expected)| match arg {
                ResolvedRecordLitArg::Positional(expr) => {
                    self.check_node_with_expected(expr, Some(expected))
                }
                ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.ensure_no_runtime_facet_args(&typed_args, span, op_name)?;

        for (ordinal, (expected, arg)) in params.iter().skip(1).zip(&typed_args).enumerate() {
            if !matches!(self.resolve_ty(expected), Ty::Hole) {
                self.assert_type_relation(
                    expected,
                    &arg.ty,
                    self.type_fact(SourceRole::Expected, span, expected),
                    self.type_fact(SourceRole::Value, &arg.span, &arg.ty),
                    TypeDiagnosticReason::ArgumentTypeMismatch,
                    DiagnosticOrigin::Operator {
                        operator: op_name.trim_matches('`').into(),
                    },
                    op_name,
                    (ordinal + 1) as u32,
                )?;
            }
        }

        Ok(TypedNode {
            ty: Ty::Func(
                vec![self.resolve_ty(&params[0])],
                Box::new(self.resolve_ty(&ret)),
            ),
            span: span.clone(),
            node: TypedInner::InjectCall(Box::new(typed_func), typed_args),
        })
    }

    pub(in crate::checker) fn check_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<TypedNode, TypeError> {
        self.check_app_with_expected(span, func, args, None)
    }

    // Keep curry-only temporaries out of the recursive ordinary-call frame.
    #[inline(never)]
    fn check_function_curry_with_expected(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
        expected_return: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let explicit_return = if let Resolved::ReturnTypeArgumentApply(..) = func {
            let specialized = self.check_node(func)?;
            match specialized.ty {
                Ty::BuiltinFunc { ret, .. } => Some(*ret),
                _ => unreachable!("standard curry has a builtin signature"),
            }
        } else {
            None
        };
        let typed = self.check_function_curry(span, args)?;
        if let Some(expected) = explicit_return.as_ref().or(expected_return) {
            self.assert_type_relation(
                expected,
                &typed.ty,
                self.type_fact(SourceRole::Expected, span, expected),
                self.type_fact(SourceRole::Value, &typed.span, &typed.ty),
                TypeDiagnosticReason::ReturnTypeMismatch,
                DiagnosticOrigin::Call,
                "Function::curry",
                0,
            )?;
        }
        Ok(typed)
    }

    fn check_app_with_expected(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
        expected_return: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        if self.is_function_curry_callee(func) {
            return self.check_function_curry_with_expected(span, func, args, expected_return);
        }
        if let Some(typed) = self.try_check_process_intrinsic_app(span, func, args)? {
            return Ok(typed);
        }

        if let Some(typed) = self.try_check_facet_intrinsic_app(span, func, args)? {
            return Ok(typed);
        }

        if let Some((id, trait_name, method_name)) = self.trait_method_ref(func) {
            let receiver_owner_hint = id
                .name
                .strip_suffix(&format!("::{}", method_name))
                .filter(|owner| *owner == "JsonValue");
            let explicit_type_arguments = Self::explicit_type_args(func);
            return self.check_trait_method_call(
                span,
                &trait_name,
                &method_name,
                args,
                receiver_owner_hint,
                expected_return,
                explicit_type_arguments.as_deref(),
            );
        }

        let result = (|| {
            let explicit_type_arguments = Self::explicit_type_args(func);
            let call_target = match func {
                Resolved::ReturnTypeArgumentApply(_, target, _) => target.as_ref(),
                other => other,
            };
            let has_canonical_signature = match call_target {
                Resolved::Var(_, id) => self.callable_signatures.contains_key(&id.unique_id),
                _ => false,
            };
            let mut prepared_signature = None;
            let mut typed_func = if explicit_type_arguments.is_some() || has_canonical_signature {
                let (typed, signature, return_type_arguments) = self
                    .instantiate_named_callable_signature(
                        span,
                        call_target,
                        explicit_type_arguments.as_deref(),
                    )?;
                prepared_signature = Some((signature, return_type_arguments));
                typed
            } else {
                self.check_node(call_target)?
            };
            let func_ty = self.resolve_ty(&typed_func.ty);

            // A closure parameter may only reveal that it is callable when it is
            // first applied (for example `g(x)` inside a higher-order closure).
            // Turn that unconstrained type variable into a callable shape before
            // checking its arguments so inference can continue recursively.
            if let Ty::Var(var) = func_ty {
                if !self.rigid_tyvars.contains(&var) {
                    let params = (0..args.len())
                        .map(|_| self.env.fresh_tyvar())
                        .collect::<Vec<_>>();
                    let ret = self.env.fresh_tyvar();
                    if self.bind_tyvar(var, &Ty::Func(params, Box::new(ret)))? {
                        return self.check_app_with_expected(span, func, args, expected_return);
                    }
                }
            }

            match &func_ty {
                Ty::BuiltinFunc { name, params, ret } => {
                    let builtin_uid = match &typed_func.node {
                        TypedInner::Var(id) => Some(id.unique_id),
                        _ => None,
                    };
                    let callable_hint = if let TypedInner::Var(id) = &typed_func.node {
                        Some(self.call_target_signature_hint_for_id(id, params, ret.as_ref()))
                    } else {
                        Some(self.call_target_signature_hint(name, params, ret.as_ref(), None))
                    };
                    let application_constraints = prepared_signature
                        .or_else(|| {
                            builtin_uid
                                .and_then(|uid| self.callable_signatures.get(&uid).cloned())
                                .map(|signature| {
                                    let (mut signature, _) =
                                        super::signatures::instantiate_callable_signature(
                                            self, &signature,
                                        );
                                    let return_type_arguments = self
                                .apply_call_site_return_type_arguments(span, &mut signature, None)
                                .expect("omitted return type arguments cannot have an arity error");
                                    (signature, return_type_arguments)
                                })
                        })
                        .map(|(signature, return_type_arguments)| {
                            self.call_constraint_set(
                                signature,
                                return_type_arguments,
                                expected_return,
                                span,
                                &typed_func.ty,
                            )
                        });
                    if application_constraints.is_none() {
                        if let TypedInner::Var(id) = &typed_func.node {
                            if self.is_registered_callable_declaration(id.unique_id) {
                                return Err(
                                    super::signatures::missing_canonical_callable_signature(
                                        id, span,
                                    ),
                                );
                            }
                        }
                    }
                    let result = (|| {
                        let application_constraints = application_constraints;
                        if let Some(constraints) = application_constraints.as_ref() {
                            self.apply_return_type_argument_constraints(constraints)?;
                            self.apply_concrete_expected_call_constraint(
                                constraints,
                                span,
                                explicit_type_arguments.as_deref(),
                            )?;
                        }
                        let typed_args = if let (Some(uid), Some(signature)) = (
                            builtin_uid,
                            application_constraints
                                .as_ref()
                                .map(|constraints| &constraints.signature),
                        ) {
                            typed_func.ty = Ty::BuiltinFunc {
                                name: name.clone(),
                                params: signature
                                    .value_parameters
                                    .iter()
                                    .map(|parameter| parameter.ty.clone())
                                    .collect(),
                                ret: Box::new(signature.return_type.ty.clone()),
                            };
                            self.check_callable_application(
                                uid,
                                name,
                                signature,
                                args,
                                span,
                                callable_hint.as_deref(),
                            )?
                        } else {
                            self.typecheck_positional_call_args(
                                span,
                                name,
                                params,
                                args,
                                callable_hint.clone(),
                                format!("{} does not accept named arguments", name),
                            )?
                        };
                        self.ensure_no_runtime_facet_args(&typed_args, span, name)?;
                        let application_signature = match application_constraints {
                            Some(mut constraints) => {
                                let substitution = match self.complete_call_constraint_set(
                                    &mut constraints,
                                    &typed_args,
                                    span,
                                    explicit_type_arguments.as_deref(),
                                ) {
                                    super::signatures::SolveState::Solved(substitution) => {
                                        substitution
                                    }
                                    super::signatures::SolveState::Deferred(pending) => {
                                        let _ = pending.unresolved_return_type_arguments.len();
                                        pending.substitution
                                    }
                                    super::signatures::SolveState::Failed(error) => {
                                        return Err(error);
                                    }
                                };
                                if let Ty::UserFunc {
                                    call_substitution, ..
                                } = &mut typed_func.ty
                                {
                                    *call_substitution = substitution;
                                }
                                Some(constraints.signature)
                            }
                            None => None,
                        };

                        if let Some(signature) = application_signature.as_ref() {
                            typed_func.ty =
                                self.callable_ty_from_signature(&typed_func.ty, signature);
                        }

                        if name == "__process_self" {
                            let Some(process_name) = self.current_process_name() else {
                                return Err(self.policy_error(
                                    TypeDiagnosticReason::ProcessHandlerScope,
                                    diagnostics::TypePolicy::ProcessHandlerScope,
                                    Some("Process::self() is only available inside process handlers".into()),
                                    None,
                                    None,
                                    None,
                                    None,
                                    span,
                                    Some("Call Process::self() inside @init/@get/@set bodies of a defagent.".into()),
                                ));
                            };
                            return Ok(TypedNode {
                                ty: Ty::pid(process_name),
                                span: span.clone(),
                                node: TypedInner::App(Box::new(typed_func), typed_args),
                            });
                        }

                        if name == "set_exit_code" {
                            match self.runtime_policy.exit_code_policy {
                                ExitCodePolicy::Anywhere => {}
                                ExitCodePolicy::Forbidden => {
                                    return Err(self.policy_error(
                                        TypeDiagnosticReason::SourcePolicyViolation,
                                        diagnostics::TypePolicy::SourceExitCode,
                                        Some("set_exit_code".into()),
                                        None,
                                        None,
                                        Some(self.runtime_policy.exit_code_policy.as_str().into()),
                                        None,
                                        span,
                                        Some("This source kind does not allow set_exit_code. Use Result-based failure handling instead.".into()),
                                    ));
                                }
                                ExitCodePolicy::EntryOnly => {
                                    let Some(entrypoint) =
                                        self.runtime_policy.normalized_entrypoint.as_ref()
                                    else {
                                        return Err(self.policy_error(
                                            TypeDiagnosticReason::CompilePolicyViolation,
                                            diagnostics::TypePolicy::EntrypointRequirement,
                                            Some("set_exit_code".into()),
                                            None,
                                            None,
                                            Some("EntryOnly".into()),
                                            None,
                                            span,
                                            Some("Configure an entrypoint, or avoid set_exit_code in this compile unit.".into()),
                                        ));
                                    };
                                    if self.current_function_symbol.as_deref()
                                        != Some(entrypoint.as_str())
                                    {
                                        return Err(self.policy_error(
                                            TypeDiagnosticReason::SourcePolicyViolation,
                                            diagnostics::TypePolicy::EntrypointRequirement,
                                            Some("set_exit_code".into()),
                                            None,
                                            None,
                                            Some(self.runtime_policy.exit_code_policy.as_str().into()),
                                            Some(entrypoint.clone()),
                                            span,
                                            Some("Move set_exit_code into the configured entrypoint function.".into()),
                                        ));
                                    }
                                }
                            }
                        }

                        Ok(TypedNode {
                            ty: application_signature
                                .as_ref()
                                .map(|signature| self.resolve_ty(&signature.return_type.ty))
                                .unwrap_or_else(|| self.resolve_ty(ret)),
                            span: span.clone(),
                            node: TypedInner::App(Box::new(typed_func), typed_args),
                        })
                    })();
                    result
                }
                Ty::UserFunc { params, ret, .. } => {
                    let has_named = args
                        .iter()
                        .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)));
                    let callee_uid = match call_target {
                        Resolved::Var(_, id) => id.unique_id,
                        _ if !has_named => u32::MAX,
                        _ => {
                            return Err(TypeError {
                                structured: None,
                                message: "This function value does not accept named arguments"
                                    .into(),
                                span: span.clone(),
                                hint: None,
                            });
                        }
                    };
                    let callable_hint =
                        self.callable_definition_signature_hint(&typed_func, params, ret.as_ref());
                    let application_constraints = prepared_signature
                        .or_else(|| {
                            self.callable_signatures
                                .get(&callee_uid)
                                .cloned()
                                .map(|signature| {
                                    let (mut signature, _) =
                                        super::signatures::instantiate_callable_signature(
                                            self, &signature,
                                        );
                                    let return_type_arguments = self
                                .apply_call_site_return_type_arguments(span, &mut signature, None)
                                .expect("omitted return type arguments cannot have an arity error");
                                    (signature, return_type_arguments)
                                })
                        })
                        .map(|(signature, return_type_arguments)| {
                            self.call_constraint_set(
                                signature,
                                return_type_arguments,
                                expected_return,
                                span,
                                &typed_func.ty,
                            )
                        });
                    if application_constraints.is_none() {
                        if let TypedInner::Var(id) = &typed_func.node {
                            if self.is_registered_callable_declaration(id.unique_id) {
                                return Err(
                                    super::signatures::missing_canonical_callable_signature(
                                        id, span,
                                    ),
                                );
                            }
                        }
                    }
                    let result = (|| {
                        let application_constraints = application_constraints;
                        if let Some(constraints) = application_constraints.as_ref() {
                            self.apply_return_type_argument_constraints(constraints)?;
                            self.apply_concrete_expected_call_constraint(
                                constraints,
                                span,
                                explicit_type_arguments.as_deref(),
                            )?;
                        }
                        let typed_args = if let Some(signature) = application_constraints
                            .as_ref()
                            .map(|constraints| &constraints.signature)
                        {
                            if let Ty::UserFunc {
                                fun_idx,
                                type_params,
                                call_substitution,
                                ..
                            } = &typed_func.ty
                            {
                                typed_func.ty = Ty::UserFunc {
                                    fun_idx: *fun_idx,
                                    type_params: type_params.clone(),
                                    call_substitution: call_substitution.clone(),
                                    params: signature
                                        .value_parameters
                                        .iter()
                                        .map(|parameter| parameter.ty.clone())
                                        .collect(),
                                    ret: Box::new(signature.return_type.ty.clone()),
                                };
                            }
                            self.check_callable_application(
                                callee_uid,
                                "function",
                                signature,
                                args,
                                span,
                                callable_hint.as_deref(),
                            )?
                        } else {
                            self.typecheck_user_function_args(
                                span,
                                callee_uid,
                                "function",
                                params,
                                args,
                                callable_hint.as_deref(),
                                false,
                            )?
                        };
                        let typed_args = typed_args
                            .into_iter()
                            .map(|arg| {
                                self.concretize_pending_trait_calls(arg)
                                    .map(|node| *node)
                                    .map_err(|error| *error)
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        self.ensure_no_runtime_facet_args(&typed_args, span, "Function call")?;
                        let application_signature = match application_constraints {
                            Some(mut constraints) => {
                                let substitution = match self.complete_call_constraint_set(
                                    &mut constraints,
                                    &typed_args,
                                    span,
                                    explicit_type_arguments.as_deref(),
                                ) {
                                    super::signatures::SolveState::Solved(substitution) => {
                                        substitution
                                    }
                                    super::signatures::SolveState::Deferred(pending) => {
                                        let _ = pending.unresolved_return_type_arguments.len();
                                        pending.substitution
                                    }
                                    super::signatures::SolveState::Failed(error) => {
                                        return Err(error);
                                    }
                                };
                                if let Ty::UserFunc {
                                    call_substitution, ..
                                } = &mut typed_func.ty
                                {
                                    *call_substitution = substitution;
                                }
                                Some(constraints.signature)
                            }
                            None => None,
                        };

                        if let Some(signature) = application_signature.as_ref() {
                            typed_func.ty =
                                self.callable_ty_from_signature(&typed_func.ty, signature);
                        }

                        Ok(TypedNode {
                            ty: application_signature
                                .as_ref()
                                .map(|signature| self.resolve_ty(&signature.return_type.ty))
                                .unwrap_or_else(|| self.resolve_ty(ret)),
                            span: span.clone(),
                            node: TypedInner::App(Box::new(typed_func), typed_args),
                        })
                    })();
                    result
                }
                Ty::Func(params, ret) => {
                    let callable_hint =
                        self.callable_signature_hint(&Ty::Func(params.clone(), ret.clone()));
                    let typed_args = self.typecheck_positional_call_args(
                        span,
                        "function",
                        params,
                        args,
                        callable_hint.clone(),
                        "Function values do not accept named arguments".into(),
                    )?;
                    self.ensure_no_runtime_facet_args(&typed_args, span, "Function call")?;

                    Ok(TypedNode {
                        ty: self.resolve_ty(ret),
                        span: span.clone(),
                        node: TypedInner::App(Box::new(typed_func), typed_args),
                    })
                }
                _ => Err(self.callable_shape_error(
                    &typed_func.ty,
                    "function",
                    &typed_func.span,
                    None,
                )),
            }
        })();
        let result = result.and_then(|typed| {
            if let TypedInner::App(function, arguments) = &typed.node {
                self.check_callable_input_provenance(function, arguments)?;
            }
            Ok(typed)
        });
        result.map_err(|error| self.lazy_call_error(error, span, func, args))
    }

    fn is_registered_callable_declaration(&self, unique_id: u32) -> bool {
        self.function_ids_by_name
            .values()
            .any(|registered| registered.unique_id == unique_id)
    }

    fn check_callable_application(
        &mut self,
        uid: u32,
        callable_name: &str,
        signature: &sindr::signature::CallableSignature<Ty>,
        args: &[ResolvedRecordLitArg],
        span: &Span,
        callable_hint: Option<&str>,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let params = signature
            .value_parameters
            .iter()
            .map(|parameter| parameter.ty.clone())
            .collect::<Vec<_>>();
        let typed_args = self.typecheck_user_function_args(
            span,
            uid,
            callable_name,
            &params,
            args,
            callable_hint,
            true,
        )?;

        Ok(typed_args)
    }
}

fn trait_impl_signature_display(
    qualified_name: &str,
    param_list: &str,
    ret: &str,
) -> Option<String> {
    let (_, rest) = qualified_name.split_once("::__traitimpl__::")?;
    let mut parts = rest.rsplitn(4, "::").collect::<Vec<_>>();
    if parts.len() != 4 {
        return None;
    }
    parts.reverse();
    let trait_name = parts[0];
    let target = parts[1];
    let method = parts[2];
    Some(format!(
        "impl {} for {} {{ def {}({}) -> {} }}",
        trait_name, target, method, param_list, ret
    ))
}

pub(in crate::checker) fn callable_definition_display_name(
    qualified_name: &str,
    local_name: &str,
) -> String {
    let local_tail = local_name.rsplit("::").next().unwrap_or(local_name);
    if let Some((_prefix, rest)) = qualified_name.split_once("::__traitimpl__::") {
        let mut parts = rest.rsplitn(4, "::").collect::<Vec<_>>();
        if parts.len() == 4 {
            parts.reverse();
            if parts[2] == local_tail {
                let display_name = local_name
                    .strip_prefix("Global::")
                    .unwrap_or(local_name)
                    .replace("::Global::", "::");
                return trim_script_qualified_display_name(&display_name);
            }
        }
        return Checker::surface_name(local_name).to_string();
    }

    let display_name = if qualified_name
        .rsplit("::")
        .next()
        .is_some_and(|tail| tail == local_tail)
    {
        Checker::surface_name(qualified_name).to_string()
    } else {
        Checker::surface_name(local_name).to_string()
    };
    trim_script_qualified_display_name(&display_name)
}

fn trim_script_qualified_display_name(qualified_name: &str) -> String {
    let Some(rest) = qualified_name.strip_prefix("__Script::") else {
        return qualified_name.to_string();
    };
    let segments = rest.split("::").collect::<Vec<_>>();
    if segments.len() < 2 {
        return qualified_name.to_string();
    }

    let name = segments[segments.len() - 1];
    let parent = segments[segments.len() - 2];
    if parent
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
    {
        format!("{}::{}", parent, name)
    } else {
        name.to_string()
    }
}
