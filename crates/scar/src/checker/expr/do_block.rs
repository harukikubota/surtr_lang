use super::*;

impl Checker {
    fn do_failure_carrier_is_rigid_variable(&self, carrier: &Ty) -> bool {
        match self.resolve_ty(carrier) {
            Ty::Var(variable) => self.rigid_tyvars.contains(&variable),
            Ty::SelfApp(items) => Self::constructor_application_parts(&items)
                .is_some_and(|(constructor, _)| {
                    matches!(constructor, Ty::Var(variable) if self.rigid_tyvars.contains(variable))
                }),
            _ => false,
        }
    }

    fn canonical_do_trait_key(
        &self,
        contract: &sigil::resolved::ResolvedDoContract,
        identity: sindr::intrinsic::CanonicalTraitIdentity,
        span: &Span,
    ) -> Result<String, TypeError> {
        let surface = identity.surface_name();
        let resolved_id = match identity {
            sindr::intrinsic::CanonicalTraitIdentity::Monad => &contract.monad_trait,
            sindr::intrinsic::CanonicalTraitIdentity::Alternative => &contract.alternative_trait,
        }
        .as_ref()
        .ok_or_else(|| {
            self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some(format!("canonical intrinsic Trait {surface}")),
                None,
                None,
                None,
                None,
                span,
                None,
            )
        })?;
        let key = self.trait_key(resolved_id);
        let Some(info) = self.traits.get(&key) else {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some(format!("canonical intrinsic Trait {surface}")),
                None,
                None,
                None,
                None,
                span,
                None,
            ));
        };
        if info.id.unique_id != resolved_id.unique_id
            || info.id.name != surface
            || info.constructor_slots.is_empty()
        {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some(format!("canonical intrinsic TypeCtorTrait {surface}")),
                None,
                None,
                None,
                None,
                span,
                None,
            ));
        }
        Ok(key)
    }

    fn partial_do_failure_method(
        &self,
        span: &Span,
    ) -> Result<sindr::intrinsic::CanonicalTraitMethodIdentity, TypeError> {
        let contract = sindr::intrinsic::do_intrinsic_contract();
        let route = contract.routes.iter().find(|route| {
            route.predicate == sindr::intrinsic::DoCapabilityPredicate::HasPartialExtractPattern
                && route.same_carrier == contract.do_local_carrier
        });
        let Some(route) = route else {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do partial-extract capability and lowering contract".into()),
                None,
                None,
                None,
                None,
                span,
                None,
            ));
        };
        let sindr::intrinsic::DoRouteLowering::FailureEffect(failure) = route.lowering else {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do partial-extract lowering route".into()),
                None,
                None,
                None,
                None,
                span,
                None,
            ));
        };
        let sindr::intrinsic::FailureEffectAction::OverrideWith(method) = failure.fallback_action
        else {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do partial-extract fallback action".into()),
                None,
                None,
                None,
                None,
                span,
                None,
            ));
        };
        if failure.fallback_capability != method.trait_identity() {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do partial-extract capability route".into()),
                None,
                None,
                None,
                None,
                span,
                None,
            ));
        }
        Ok(method)
    }

    pub(super) fn check_do(
        &mut self,
        span: &Span,
        intrinsic: sindr::intrinsic::IntrinsicId,
        resolved_contract: &sigil::resolved::ResolvedDoContract,
        return_type_arguments: &[ResolvedReturnTypeArgument],
        statements: &[ResolvedDoStatement],
        expected: Option<&Ty>,
        expected_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        let contract = sindr::intrinsic::do_intrinsic_contract();
        if intrinsic != contract.identity || contract.return_type_arguments.len() != 1 {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("validated do intrinsic contract".into()),
                None,
                None,
                None,
                None,
                span,
                None,
            ));
        }
        if return_type_arguments.len() > 1 {
            return Err(super::signatures::return_type_argument_arity_error(
                intrinsic.surface_name(),
                1,
                return_type_arguments.len(),
                span,
            ));
        }
        let explicit_carrier = return_type_arguments
            .first()
            .map(|argument| {
                self.resolve_type_constructor_return_type_argument(argument.ty.syntax())
                    .map(|ty| (ty, argument))
            })
            .transpose()?;
        let mut carrier_hint = explicit_carrier.as_ref().map(|(ty, _)| ty.clone());
        let mut carrier_relation = explicit_carrier
            .as_ref()
            .map(|(_, argument)| ExpectedTypeRelation::return_type_argument(&argument.span));
        if let Some(expected) = expected {
            let expected = self.resolve_ty(expected);
            if let Some((explicit, argument)) = &explicit_carrier {
                let relation = expected_relation
                    .cloned()
                    .unwrap_or_else(|| ExpectedTypeRelation::contextual(span));
                if self
                    .assert_type_relation(
                        &expected,
                        explicit,
                        self.type_fact(relation.role, &relation.span, &expected),
                        self.type_fact(SourceRole::ReturnTypeArgument, &argument.span, explicit),
                        TypeDiagnosticReason::ReturnTypeArgumentMismatch,
                        DiagnosticOrigin::ReturnTypeArgument { ordinal: 0 },
                        intrinsic.surface_name(),
                        0,
                    )
                    .is_err()
                {
                    return Err(super::signatures::return_type_argument_mismatch_error(
                        intrinsic.surface_name(),
                        0,
                        &self.diagnostic_ty_name(&expected),
                        &self.diagnostic_ty_name(explicit),
                        &argument.span,
                        relation.role,
                        &relation.span,
                    ));
                }
                carrier_hint = Some(self.resolve_ty(explicit));
                carrier_relation = expected_relation
                    .cloned()
                    .or_else(|| Some(ExpectedTypeRelation::contextual(span)));
                if let Some(relation) = &mut carrier_relation {
                    relation.explicit_carrier_span = Some(argument.span.clone());
                }
            } else {
                carrier_hint = Some(expected);
                carrier_relation = expected_relation
                    .cloned()
                    .or_else(|| Some(ExpectedTypeRelation::contextual(span)));
            }
        }

        let result_span = match statements.last() {
            Some(ResolvedDoStatement::Statement(final_expression)) => {
                self.resolved_span(final_expression).clone()
            }
            _ => {
                return Err(self.policy_error(
                    TypeDiagnosticReason::TypecheckInvariantViolation,
                    diagnostics::TypePolicy::ProducerContract,
                    Some("final do expression origin".into()),
                    None,
                    None,
                    None,
                    None,
                    span,
                    None,
                ));
            }
        };

        let saved_facet_bindings = self.facet_bindings.clone();
        let saved_constructor_capabilities = self.constructor_capabilities.clone();
        self.env.push_var_scope();
        let result = self.check_do_statements(
            span,
            intrinsic,
            resolved_contract,
            statements,
            &result_span,
            carrier_hint.as_ref(),
            carrier_relation.as_ref(),
        );
        self.env.pop_var_scope();
        self.facet_bindings = saved_facet_bindings;
        self.constructor_capabilities = saved_constructor_capabilities;
        result
    }

    fn check_do_statements(
        &mut self,
        do_span: &Span,
        intrinsic: sindr::intrinsic::IntrinsicId,
        resolved_contract: &sigil::resolved::ResolvedDoContract,
        statements: &[ResolvedDoStatement],
        result_span: &Span,
        carrier_hint: Option<&Ty>,
        carrier_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        let Some((first, rest)) = statements.split_first() else {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("non-empty resolved do block".into()),
                None,
                None,
                None,
                None,
                do_span,
                None,
            ));
        };
        if rest.is_empty() {
            let ResolvedDoStatement::Statement(final_expression) = first else {
                return Err(self.policy_error(
                    TypeDiagnosticReason::TypecheckInvariantViolation,
                    diagnostics::TypePolicy::ProducerContract,
                    Some("final do expression".into()),
                    None,
                    None,
                    None,
                    None,
                    do_span,
                    None,
                ));
            };
            return self.check_do_final_expression(
                do_span,
                resolved_contract,
                final_expression,
                carrier_hint,
                carrier_relation,
            );
        }

        match first {
            ResolvedDoStatement::Extract {
                span,
                operator_span: _,
                pattern_span,
                pattern,
                rhs,
            } => self.check_do_extract(
                do_span,
                intrinsic,
                resolved_contract,
                span,
                pattern_span,
                pattern,
                rhs,
                rest,
                carrier_hint,
                carrier_relation,
            ),
            ResolvedDoStatement::SafeBind {
                span,
                operator_span,
                pattern_span,
                pattern,
                rhs,
            } => self.check_do_safebind(
                do_span,
                intrinsic,
                resolved_contract,
                span,
                operator_span,
                pattern_span,
                pattern,
                rhs,
                rest,
                result_span,
                carrier_hint,
                carrier_relation,
            ),
            ResolvedDoStatement::Statement(statement)
                if matches!(
                    statement,
                    Resolved::Bind(..)
                        | Resolved::Semi(..)
                        | Resolved::Def(..)
                        | Resolved::ExtractorDef(..)
                        | Resolved::ConstDef(..)
                ) =>
            {
                let inherited_substitutions = self.substitutions.clone();
                let typed_statement = self.check_node(statement)?;
                self.substitutions = inherited_substitutions;
                let typed_rest = self.check_do_statements(
                    do_span,
                    intrinsic,
                    resolved_contract,
                    rest,
                    result_span,
                    carrier_hint,
                    carrier_relation,
                )?;
                Ok(TypedNode {
                    ty: typed_rest.ty.clone(),
                    span: do_span.clone(),
                    node: TypedInner::Block(vec![typed_statement, typed_rest]),
                })
            }
            ResolvedDoStatement::Statement(statement) => self.check_do_bare_or_plain_statement(
                do_span,
                intrinsic,
                resolved_contract,
                statement,
                rest,
                result_span,
                carrier_hint,
                carrier_relation,
            ),
        }
    }

    fn check_do_final_expression(
        &mut self,
        do_span: &Span,
        resolved_contract: &sigil::resolved::ResolvedDoContract,
        expression: &Resolved,
        carrier_hint: Option<&Ty>,
        carrier_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        let typed = match carrier_hint {
            Some(expected) if self.body_tail_is_return_type_argument_call(expression) => {
                self.check_node_with_expected(expression, Some(expected))?
            }
            Some(_) => self.check_node(expression)?,
            None => self.check_node(expression)?,
        };
        if let Some(expected) = carrier_hint {
            let monad = self.canonical_do_trait_key(
                resolved_contract,
                sindr::intrinsic::CanonicalTraitIdentity::Monad,
                do_span,
            )?;
            let compatible = self.types_compatible(expected, &typed.ty);
            let carrier_classification = if compatible {
                None
            } else {
                Some(self.constructor_carrier_relation(&monad, expected, &typed.ty))
            };
            let mut relation = carrier_relation
                .cloned()
                .unwrap_or_else(|| ExpectedTypeRelation::contextual(do_span));
            if matches!(
                carrier_classification,
                Some(ConstructorCarrierRelation::DifferentCarrier)
            ) {
                if let Some(span) = relation.explicit_carrier_span.clone() {
                    relation = ExpectedTypeRelation::return_type_argument(&span);
                }
            }
            let externally_owned_reason = Some(relation.reason).filter(|reason| {
                matches!(
                    reason,
                    TypeDiagnosticReason::ReturnTypeMismatch
                        | TypeDiagnosticReason::AnnotationTypeMismatch
                        | TypeDiagnosticReason::ReturnTypeArgumentMismatch
                )
            });
            let relation_reason = if let Some(reason) = externally_owned_reason {
                reason
            } else if compatible {
                TypeDiagnosticReason::TypePayloadMismatch
            } else {
                match carrier_classification
                    .expect("incompatible do result must classify its constructor relation")
                {
                    ConstructorCarrierRelation::SameCarrier => {
                        TypeDiagnosticReason::TypePayloadMismatch
                    }
                    ConstructorCarrierRelation::DifferentCarrier => {
                        TypeDiagnosticReason::TypeConstructorFamilyMismatch
                    }
                    ConstructorCarrierRelation::Deferred { waiting_on } => {
                        let _ = waiting_on;
                        return Err(self.ambiguous_constructor_result(&monad, "do", do_span));
                    }
                    ConstructorCarrierRelation::Rejected { failures }
                        if Self::constructor_projection_failures_are_metadata(&failures) =>
                    {
                        return Err(signatures::constructor_signature_metadata_error(
                            "do",
                            do_span,
                            Self::constructor_projection_failure_detail(&failures),
                        ));
                    }
                    ConstructorCarrierRelation::Rejected { .. } => {
                        return Err(self.trait_obligation_failure(
                            self.constructor_capability_failure_reason(&typed.ty),
                            &monad,
                            &[],
                            None,
                            &typed.ty,
                            &typed.span,
                            DiagnosticOrigin::Intrinsic,
                        ));
                    }
                }
            };
            self.assert_type_relation(
                expected,
                &typed.ty,
                self.type_fact(relation.role, &relation.span, expected),
                self.type_fact(SourceRole::Value, &typed.span, &typed.ty),
                relation_reason,
                relation.origin,
                relation.callable,
                relation.ordinal,
            )?;
        }
        let carrier = self.resolve_ty(&typed.ty);
        let contract = sindr::intrinsic::do_intrinsic_contract();
        let monad_identity = contract
            .routes
            .iter()
            .find(|route| {
                route.predicate == sindr::intrinsic::DoCapabilityPredicate::Always
                    && route.same_carrier == contract.do_local_carrier
                    && matches!(
                        route.lowering,
                        sindr::intrinsic::DoRouteLowering::Sequence(_)
                    )
            })
            .map(|route| route.capability)
            .ok_or_else(|| {
                self.policy_error(
                    TypeDiagnosticReason::TypecheckInvariantViolation,
                    diagnostics::TypePolicy::ProducerContract,
                    Some("do Monad capability rule".into()),
                    None,
                    None,
                    None,
                    None,
                    do_span,
                    None,
                )
            })?;
        let monad = self.canonical_do_trait_key(resolved_contract, monad_identity, do_span)?;
        match self.constructor_slot_type_for(&monad, &carrier) {
            ConstructorApplicationOutcome::Applied(_) => {
                self.consume_matching_capability(&carrier, &monad);
                Ok(typed)
            }
            ConstructorApplicationOutcome::Deferred { .. } => {
                Err(self.ambiguous_constructor_result(&monad, "do", do_span))
            }
            ConstructorApplicationOutcome::Rejected { failures }
                if Self::constructor_projection_failures_are_metadata(&failures) =>
            {
                Err(signatures::constructor_signature_metadata_error(
                    "do",
                    do_span,
                    Self::constructor_projection_failure_detail(&failures),
                ))
            }
            ConstructorApplicationOutcome::Rejected { .. } => Err(self.trait_obligation_failure(
                self.constructor_capability_failure_reason(&carrier),
                &monad,
                &[],
                None,
                &carrier,
                &typed.span,
                DiagnosticOrigin::Intrinsic,
            )),
        }
    }

    fn synthetic_do_continuation(
        &self,
        span: &Span,
        intrinsic: sindr::intrinsic::IntrinsicId,
        contract: &sigil::resolved::ResolvedDoContract,
        statements: &[ResolvedDoStatement],
    ) -> Resolved {
        Resolved::Do(
            span.clone(),
            intrinsic,
            contract.clone(),
            Vec::new(),
            statements.to_vec(),
        )
    }

    fn check_do_bind_invocation(
        &mut self,
        do_span: &Span,
        resolved_contract: &sigil::resolved::ResolvedDoContract,
        source: &Resolved,
        closure: Resolved,
        carrier_hint: Option<&Ty>,
        carrier_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        let contract = sindr::intrinsic::do_intrinsic_contract();
        let sequence = contract
            .routes
            .iter()
            .find_map(|route| match route.lowering {
                sindr::intrinsic::DoRouteLowering::Sequence(method)
                    if route.predicate == sindr::intrinsic::DoCapabilityPredicate::Always
                        && route.capability == method.trait_identity()
                        && route.same_carrier == contract.do_local_carrier =>
                {
                    Some(method)
                }
                _ => None,
            })
            .ok_or_else(|| {
                self.policy_error(
                    TypeDiagnosticReason::TypecheckInvariantViolation,
                    diagnostics::TypePolicy::ProducerContract,
                    Some("do sequence route".into()),
                    None,
                    None,
                    None,
                    None,
                    do_span,
                    None,
                )
            })?;
        let trait_key =
            self.canonical_do_trait_key(resolved_contract, sequence.trait_identity(), do_span)?;
        let args = vec![
            ResolvedRecordLitArg::Positional(source.clone()),
            ResolvedRecordLitArg::Positional(closure),
        ];
        let explicit_relation = carrier_relation.map(|relation| {
            relation
                .explicit_carrier_span
                .as_ref()
                .map(ExpectedTypeRelation::return_type_argument)
                .unwrap_or_else(|| relation.clone())
        });
        let constructor_failure_reason =
            carrier_hint.map(|carrier| self.constructor_capability_failure_reason(carrier));
        let continuation_relation = carrier_relation.map(ExpectedTypeRelation::for_do_continuation);
        self.check_trait_invocation(
            do_span,
            &trait_key,
            sequence.method_name(),
            &args,
            None,
            carrier_hint,
            explicit_relation
                .as_ref()
                .map(|relation| (relation, self.resolved_span(source))),
            continuation_relation.as_ref(),
            constructor_failure_reason,
            None,
            None,
            None,
        )
    }

    fn check_do_extract(
        &mut self,
        do_span: &Span,
        intrinsic: sindr::intrinsic::IntrinsicId,
        resolved_contract: &sigil::resolved::ResolvedDoContract,
        statement_span: &Span,
        pattern_span: &Span,
        pattern: &ResolvedPattern,
        rhs: &Resolved,
        rest: &[ResolvedDoStatement],
        carrier_hint: Option<&Ty>,
        carrier_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        let parameter_id = ResolvedId {
            name: "__do_payload".into(),
            qualified_name: None,
            symbol_info: None,
            unique_id: Self::next_synthetic_range_uid(),
            compiler_generated: true,
            span: statement_span.clone(),
        };
        let parameters = vec![ResolvedClosureParam {
            id: parameter_id.clone(),
            ty: None,
        }];
        let continuation =
            self.synthetic_do_continuation(do_span, intrinsic, resolved_contract, rest);
        let failure_placeholder = if Self::is_total_bind_pattern(pattern) {
            None
        } else {
            Some(ResolvedId {
                name: "__do_failure_effect".into(),
                qualified_name: None,
                symbol_info: None,
                unique_id: Self::next_synthetic_range_uid(),
                compiler_generated: true,
                span: pattern_span.clone(),
            })
        };
        let body = if Self::is_total_bind_pattern(pattern) {
            Resolved::Block(
                statement_span.clone(),
                vec![
                    Resolved::Bind(
                        statement_span.clone(),
                        pattern.clone(),
                        Box::new(Resolved::Var(statement_span.clone(), parameter_id.clone())),
                    ),
                    continuation,
                ],
            )
        } else {
            let placeholder = failure_placeholder
                .as_ref()
                .expect("partial bind placeholder");
            Resolved::Match(
                statement_span.clone(),
                Box::new(Resolved::Var(statement_span.clone(), parameter_id.clone())),
                vec![
                    ResolvedMatchArm {
                        pattern: pattern.clone(),
                        guard: None,
                        body: continuation,
                    },
                    ResolvedMatchArm {
                        pattern: ResolvedPattern::Wildcard(statement_span.clone()),
                        guard: None,
                        body: Resolved::Var(pattern_span.clone(), placeholder.clone()),
                    },
                ],
            )
        };
        let mut captures = sigil::collect_resolved_closure_captures(&body, &parameters);
        if let Some(placeholder) = &failure_placeholder {
            captures.retain(|capture| capture.unique_id != placeholder.unique_id);
        }
        let closure =
            Resolved::Closure(statement_span.clone(), parameters, captures, Box::new(body));
        if let Some(placeholder) = &failure_placeholder {
            let placeholder_ty = self.env.fresh_tyvar();
            self.env.push_var_scope();
            self.env.bind_var(placeholder.unique_id, placeholder_ty);
        }
        let checked = self.check_do_bind_invocation(
            do_span,
            resolved_contract,
            rhs,
            closure,
            carrier_hint,
            carrier_relation,
        );
        if failure_placeholder.is_some() {
            self.env.pop_var_scope();
        }
        let mut checked = checked?;
        let Some(placeholder) = failure_placeholder else {
            return Ok(checked);
        };
        let carrier_ty = self.resolve_ty(&checked.ty);
        let replacement = match self.resolve_result_effect(&carrier_ty) {
            ResultEffectResolution::Preserve(target) => {
                if !self.types_compatible(&target.error_ty, &Ty::Error) {
                    return Err(self.policy_error(
                        TypeDiagnosticReason::SafeBindErrorTypeMismatch,
                        diagnostics::TypePolicy::SafeBindFailureTarget,
                        Some("do partial pattern".into()),
                        Some(&target.error_ty),
                        Some(&Ty::Error),
                        None,
                        None,
                        pattern_span,
                        None,
                    ));
                }
                TypedNode {
                    ty: target.carrier_ty.clone(),
                    span: pattern_span.clone(),
                    node: TypedInner::ResultEffectFailure(Box::new(target)),
                }
            }
            ResultEffectResolution::Unavailable => {
                let failure = self.partial_do_failure_method(pattern_span)?;
                let trait_key = self.canonical_do_trait_key(
                    resolved_contract,
                    failure.trait_identity(),
                    pattern_span,
                )?;
                self.check_trait_invocation(
                    pattern_span,
                    &trait_key,
                    failure.method_name(),
                    &[],
                    None,
                    Some(&carrier_ty),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )?
            }
            ResultEffectResolution::Deferred => {
                let failure = self.partial_do_failure_method(pattern_span)?;
                let trait_key = self.canonical_do_trait_key(
                    resolved_contract,
                    failure.trait_identity(),
                    pattern_span,
                )?;
                if self.do_failure_carrier_is_rigid_variable(&carrier_ty) {
                    let _ = self.check_trait_invocation(
                        pattern_span,
                        &trait_key,
                        failure.method_name(),
                        &[],
                        None,
                        Some(&carrier_ty),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )?;
                }
                TypedNode {
                    ty: carrier_ty.clone(),
                    span: pattern_span.clone(),
                    node: TypedInner::DeferredDoFailure(Box::new(DeferredDoFailureTarget {
                        carrier_ty: carrier_ty.clone(),
                        alternative_trait_key: trait_key,
                        alternative_method_name: failure.method_name().into(),
                        propagated_error_tys: vec![Ty::Error],
                        failure_span: pattern_span.clone(),
                    })),
                }
            }
            ResultEffectResolution::InvalidMetadata(subject) => {
                return Err(self.policy_error(
                    TypeDiagnosticReason::TypecheckInvariantViolation,
                    diagnostics::TypePolicy::ProducerContract,
                    Some(subject.into()),
                    None,
                    Some(&carrier_ty),
                    None,
                    None,
                    pattern_span,
                    None,
                ));
            }
        };
        let TypedInner::TraitCall { args, .. } = &mut checked.node else {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do partial-bind Monad::bind call".into()),
                None,
                Some(&carrier_ty),
                None,
                None,
                statement_span,
                None,
            ));
        };
        let Some(closure) = args.get_mut(1) else {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do partial-bind mapper closure".into()),
                None,
                Some(&carrier_ty),
                None,
                None,
                statement_span,
                None,
            ));
        };
        let TypedInner::Closure(_, captures, body) = &mut closure.node else {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do partial-bind typed mapper closure".into()),
                None,
                Some(&carrier_ty),
                None,
                None,
                statement_span,
                None,
            ));
        };
        captures.retain(|capture| capture.unique_id != placeholder.unique_id);
        let TypedInner::Match(_, arms) = &mut body.node else {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do partial-bind typed failure match".into()),
                None,
                Some(&carrier_ty),
                None,
                None,
                statement_span,
                None,
            ));
        };
        let Some(failure_arm) = arms.last_mut() else {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do partial-bind failure arm".into()),
                None,
                Some(&carrier_ty),
                None,
                None,
                statement_span,
                None,
            ));
        };
        if !matches!(&failure_arm.body.node, TypedInner::Var(id) if id.unique_id == placeholder.unique_id)
        {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do partial-bind failure placeholder".into()),
                None,
                Some(&carrier_ty),
                None,
                None,
                statement_span,
                None,
            ));
        }
        failure_arm.body = replacement;
        Ok(self.resolve_typed_node(checked))
    }

    fn do_safebind_failure_method(
        &self,
        span: &Span,
    ) -> Result<sindr::intrinsic::CanonicalTraitMethodIdentity, TypeError> {
        let contract = sindr::intrinsic::do_intrinsic_contract();
        let failure = contract
            .routes
            .iter()
            .find_map(|route| match route.lowering {
                sindr::intrinsic::DoRouteLowering::FailureEffect(failure)
                    if route.predicate
                        == sindr::intrinsic::DoCapabilityPredicate::HasLegalSafeBind
                        && route.same_carrier == contract.do_local_carrier =>
                {
                    Some(failure)
                }
                _ => None,
            })
            .ok_or_else(|| {
                self.policy_error(
                    TypeDiagnosticReason::TypecheckInvariantViolation,
                    diagnostics::TypePolicy::ProducerContract,
                    Some("do SafeBind failure route".into()),
                    None,
                    None,
                    None,
                    None,
                    span,
                    None,
                )
            })?;
        let sindr::intrinsic::FailureEffectAction::OverrideWith(method) = failure.fallback_action
        else {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do SafeBind non-Result failure action".into()),
                None,
                None,
                None,
                None,
                span,
                None,
            ));
        };
        if failure.fallback_capability == method.trait_identity() {
            Ok(method)
        } else {
            Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do SafeBind capability and lowering contract".into()),
                None,
                None,
                None,
                None,
                span,
                None,
            ))
        }
    }

    fn check_do_safebind(
        &mut self,
        do_span: &Span,
        intrinsic: sindr::intrinsic::IntrinsicId,
        resolved_contract: &sigil::resolved::ResolvedDoContract,
        statement_span: &Span,
        operator_span: &Span,
        pattern_span: &Span,
        pattern: &ResolvedPattern,
        rhs: &Resolved,
        rest: &[ResolvedDoStatement],
        result_span: &Span,
        carrier_hint: Option<&Ty>,
        carrier_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        let inherited_substitutions = self.substitutions.clone();
        let mut checked = self.check_safebind_input(statement_span, pattern, rhs)?;
        checked.typed_pattern = self.resolve_typed_pattern(checked.typed_pattern);
        checked.pattern_ty = self.resolve_ty(&checked.pattern_ty);
        checked.typed_rhs = self.resolve_typed_node(checked.typed_rhs);
        checked.projection = match checked.projection {
            SafeBindRhsProjection::CanonicalResultOnce {
                payload_ty,
                error_ty,
            } => SafeBindRhsProjection::CanonicalResultOnce {
                payload_ty: self.resolve_ty(&payload_ty),
                error_ty: self.resolve_ty(&error_ty),
            },
            SafeBindRhsProjection::PassThroughNonResultPartial { pattern_input_ty } => {
                SafeBindRhsProjection::PassThroughNonResultPartial {
                    pattern_input_ty: self.resolve_ty(&pattern_input_ty),
                }
            }
        };
        checked.propagated_error_tys = checked
            .propagated_error_tys
            .into_iter()
            .map(|ty| self.resolve_ty(&ty))
            .collect();
        self.substitutions = inherited_substitutions;
        self.bind_typed_pattern(&checked.typed_pattern, &checked.pattern_ty);
        self.bind_constructor_provenance(
            &checked.typed_pattern,
            self.result_constructor_provenance(&checked.typed_rhs),
        );

        let continuation = self.check_do_statements(
            do_span,
            intrinsic,
            resolved_contract,
            rest,
            result_span,
            carrier_hint,
            carrier_relation,
        )?;
        let continuation_ty = self.resolve_ty(&continuation.ty);
        let failure_target = match self.resolve_result_effect(&continuation_ty) {
            ResultEffectResolution::Preserve(target) => {
                self.collect_pattern_result_error_types(
                    &checked.typed_pattern,
                    &mut checked.propagated_error_tys,
                );
                for propagated in &checked.propagated_error_tys {
                    if !self.types_compatible(&target.error_ty, propagated) {
                        return Err(self.policy_error(
                            TypeDiagnosticReason::SafeBindErrorTypeMismatch,
                            diagnostics::TypePolicy::SafeBindFailureTarget,
                            Some("=?".into()),
                            Some(&target.error_ty),
                            Some(propagated),
                            None,
                            None,
                            &checked.typed_rhs.span,
                            None,
                        ));
                    }
                }
                SafeBindFailureTarget::DoResultContext(Box::new(target))
            }
            ResultEffectResolution::Unavailable => {
                let failure = self.do_safebind_failure_method(operator_span)?;
                let trait_key = self.canonical_do_trait_key(
                    resolved_contract,
                    failure.trait_identity(),
                    operator_span,
                )?;
                let empty = self.check_trait_invocation(
                    operator_span,
                    &trait_key,
                    failure.method_name(),
                    &[],
                    None,
                    Some(&continuation_ty),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )?;
                SafeBindFailureTarget::DoAlternative {
                    empty: Box::new(empty),
                }
            }
            ResultEffectResolution::Deferred => {
                self.collect_pattern_result_error_types(
                    &checked.typed_pattern,
                    &mut checked.propagated_error_tys,
                );
                let failure = self.do_safebind_failure_method(operator_span)?;
                let trait_key = self.canonical_do_trait_key(
                    resolved_contract,
                    failure.trait_identity(),
                    operator_span,
                )?;
                if self.do_failure_carrier_is_rigid_variable(&continuation_ty) {
                    let _ = self.check_trait_invocation(
                        operator_span,
                        &trait_key,
                        failure.method_name(),
                        &[],
                        None,
                        Some(&continuation_ty),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )?;
                }
                SafeBindFailureTarget::Deferred(Box::new(DeferredDoFailureTarget {
                    carrier_ty: continuation_ty.clone(),
                    alternative_trait_key: trait_key,
                    alternative_method_name: failure.method_name().into(),
                    propagated_error_tys: checked.propagated_error_tys.clone(),
                    failure_span: operator_span.clone(),
                }))
            }
            ResultEffectResolution::InvalidMetadata(subject) => {
                return Err(self.policy_error(
                    TypeDiagnosticReason::TypecheckInvariantViolation,
                    diagnostics::TypePolicy::ProducerContract,
                    Some(subject.into()),
                    None,
                    Some(&continuation_ty),
                    None,
                    None,
                    operator_span,
                    None,
                ));
            }
        };
        let rhs_span = checked.typed_rhs.span.clone();
        Ok(TypedNode {
            ty: continuation_ty,
            span: statement_span.clone(),
            node: TypedInner::DoSafeBind(Box::new(TypedDoSafeBind {
                pattern: checked.typed_pattern,
                rhs: Box::new(checked.typed_rhs),
                projection: checked.projection,
                failure_target,
                continuation: Box::new(continuation),
                origins: DoSafeBindOrigins {
                    do_span: do_span.clone(),
                    operator_span: operator_span.clone(),
                    pattern_span: pattern_span.clone(),
                    rhs_span,
                    result_span: result_span.clone(),
                },
            })),
        })
    }

    fn check_do_bare_or_plain_statement(
        &mut self,
        do_span: &Span,
        intrinsic: sindr::intrinsic::IntrinsicId,
        resolved_contract: &sigil::resolved::ResolvedDoContract,
        statement: &Resolved,
        rest: &[ResolvedDoStatement],
        result_span: &Span,
        carrier_hint: Option<&Ty>,
        carrier_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        let monad = self.canonical_do_trait_key(
            resolved_contract,
            sindr::intrinsic::CanonicalTraitIdentity::Monad,
            do_span,
        )?;
        let mut checkpoint = self.candidate_probe_checkpoint();
        let probe = match self.check_node(statement) {
            Err(error)
                if error.reason() == Some(TypeDiagnosticReason::AmbiguousReturnTypeArgument)
                    && carrier_hint.is_some() =>
            {
                self.rollback_candidate_probe(checkpoint);
                checkpoint = self.candidate_probe_checkpoint();
                self.check_node_with_expected(statement, carrier_hint)
            }
            result => result,
        };
        let statement_kind = match &probe {
            Ok(typed) => match self.constructor_slot_type_for(&monad, &typed.ty) {
                ConstructorApplicationOutcome::Applied(_)
                | ConstructorApplicationOutcome::Deferred { .. } => DoStatementKind::Monadic,
                ConstructorApplicationOutcome::Rejected { .. } => DoStatementKind::Plain,
            },
            Err(error) => {
                let error = error.clone();
                self.rollback_candidate_probe(checkpoint);
                return Err(error);
            }
        };
        self.rollback_candidate_probe(checkpoint);
        if statement_kind == DoStatementKind::Plain {
            let inherited_substitutions = self.substitutions.clone();
            let typed_statement = self.check_node(statement)?;
            self.substitutions = inherited_substitutions;
            let typed_rest = self.check_do_statements(
                do_span,
                intrinsic,
                resolved_contract,
                rest,
                result_span,
                carrier_hint,
                carrier_relation,
            )?;
            return Ok(TypedNode {
                ty: typed_rest.ty.clone(),
                span: do_span.clone(),
                node: TypedInner::Block(vec![typed_statement, typed_rest]),
            });
        }

        let parameter_id = ResolvedId {
            name: "__do_ignored_payload".into(),
            qualified_name: None,
            symbol_info: None,
            unique_id: Self::next_synthetic_range_uid(),
            compiler_generated: true,
            span: self.resolved_span(statement).clone(),
        };
        let parameters = vec![ResolvedClosureParam {
            id: parameter_id,
            ty: None,
        }];
        let body = self.synthetic_do_continuation(do_span, intrinsic, resolved_contract, rest);
        let captures = sigil::collect_resolved_closure_captures(&body, &parameters);
        let closure = Resolved::Closure(
            self.resolved_span(statement).clone(),
            parameters,
            captures,
            Box::new(body),
        );
        self.check_do_bind_invocation(
            do_span,
            resolved_contract,
            statement,
            closure,
            carrier_hint,
            carrier_relation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trait_id(name: &str, qualified_name: &str, unique_id: u32) -> ResolvedId {
        ResolvedId {
            name: name.into(),
            qualified_name: Some(qualified_name.into()),
            unique_id,
            compiler_generated: false,
            symbol_info: None,
            span: Span { start: 0, end: 1 },
        }
    }

    fn constructor_trait(id: ResolvedId) -> TraitInfo {
        TraitInfo {
            id,
            type_params: Vec::new(),
            where_clause: None,
            constructor_slots: vec!["$A".into()],
            parents: Vec::new(),
            methods: HashMap::new(),
        }
    }

    #[test]
    fn canonical_do_trait_uses_resolved_identity_when_short_names_collide() {
        let mut checker = Checker::new(TypecheckContext::default());
        let canonical = trait_id("Monad", "Monad", 1);
        let shadow = trait_id("Monad", "Shadow::Monad", 2);
        checker
            .traits
            .insert("Monad".into(), constructor_trait(canonical.clone()));
        checker
            .traits
            .insert("Shadow::Monad".into(), constructor_trait(shadow));
        let contract = sigil::resolved::ResolvedDoContract {
            monad_trait: Some(canonical),
            alternative_trait: None,
        };

        assert_eq!(
            checker
                .canonical_do_trait_key(
                    &contract,
                    sindr::intrinsic::CanonicalTraitIdentity::Monad,
                    &Span { start: 0, end: 1 },
                )
                .expect("resolved canonical identity must remain selectable"),
            "Monad"
        );
    }
}
