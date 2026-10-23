use super::*;
use diagnostics::{DiagnosticData, StructuredDiagnostic};

impl Checker {
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
            sindr::intrinsic::CanonicalTraitIdentity::MonadFail => &contract.monad_fail_trait,
            sindr::intrinsic::CanonicalTraitIdentity::MonadExtractor => {
                return Err(TypeError::new(
                    "MonadExtractor is an input capability, not a do carrier contract",
                    span.clone(),
                ));
            }
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

    fn resolve_do_pattern_failure_target(
        &mut self,
        contract: &sigil::resolved::ResolvedDoContract,
        carrier: &Ty,
        propagated: &[Ty],
        span: &Span,
    ) -> Result<SafeBindFailureTarget, TypeError> {
        use sindr::intrinsic::{CanonicalTraitIdentity, CanonicalTraitMethodIdentity};
        let fail_key =
            self.canonical_do_trait_key(contract, CanonicalTraitIdentity::MonadFail, span)?;
        if !self
            .traits
            .get(&fail_key)
            .is_some_and(|info| info.compiler_owned_failure)
        {
            return Err(self.policy_error(
                TypeDiagnosticReason::TypecheckInvariantViolation,
                diagnostics::TypePolicy::ProducerContract,
                Some("do failure contract does not reference canonical MonadFail".into()),
                None,
                Some(carrier),
                None,
                None,
                span,
                None,
            ));
        }
        let empty = CanonicalTraitMethodIdentity::AlternativeEmpty;
        let alternative_key =
            self.canonical_do_trait_key(contract, empty.trait_identity(), span)?;
        self.resolve_pattern_failure_target(
            carrier,
            propagated,
            span,
            Some((
                &alternative_key,
                empty.method_name(),
                &contract.keyword_span,
            )),
            true,
        )
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
        let mut statements = statements;
        let mut prefix = Vec::new();
        let tail = loop {
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
                break self.check_do_final_expression(
                    do_span,
                    resolved_contract,
                    final_expression,
                    carrier_hint,
                    carrier_relation,
                )?;
            }

            if let ResolvedDoStatement::Statement(statement) = first {
                if Self::statement_question_parts(statement).is_none() {
                    let plain = matches!(
                        statement,
                        Resolved::Bind(..)
                            | Resolved::Semi(..)
                            | Resolved::Def(..)
                            | Resolved::ExtractorDef(..)
                            | Resolved::ConstDef(..)
                    ) || self.classify_do_statement(
                        do_span,
                        resolved_contract,
                        statement,
                        carrier_hint,
                    )? == DoStatementKind::Plain;
                    if plain {
                        let inherited_substitutions = self.substitutions.clone();
                        let typed = self.check_node(statement)?;
                        self.substitutions = inherited_substitutions;
                        prefix.push(typed);
                        statements = rest;
                        continue;
                    }
                }
            }

            break match first {
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
                    false,
                    rest,
                    result_span,
                    carrier_hint,
                    carrier_relation,
                ),
                ResolvedDoStatement::Statement(statement)
                    if Self::statement_question_parts(statement).is_some() =>
                {
                    let (span, rhs) = Self::statement_question_parts(statement)
                        .expect("matched a statement question");
                    let operator_span = Span {
                        start: span.end - 1,
                        end: span.end,
                    };
                    self.check_do_safebind(
                        do_span,
                        intrinsic,
                        resolved_contract,
                        span,
                        &operator_span,
                        span,
                        &ResolvedPattern::Wildcard(span.clone()),
                        rhs,
                        true,
                        rest,
                        result_span,
                        carrier_hint,
                        carrier_relation,
                    )
                }
                ResolvedDoStatement::Statement(statement) => self.check_do_monadic_statement(
                    do_span,
                    intrinsic,
                    resolved_contract,
                    statement,
                    rest,
                    carrier_hint,
                    carrier_relation,
                ),
            }?;
        };
        if prefix.is_empty() {
            Ok(tail)
        } else {
            let ty = tail.ty.clone();
            prefix.push(tail);
            Ok(TypedNode {
                ty,
                span: do_span.clone(),
                node: TypedInner::Block(prefix),
            })
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
            let compatible = self.types_compatible(expected, &typed.ty)?;
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
        match self
            .constructor_slot_type_for(&monad, &carrier)
            .into_checked()
            .map_err(|error| error.at_span(&typed.span))?
        {
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
            lazy_capture: None,
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
        let failure_target = self.resolve_do_pattern_failure_target(
            resolved_contract,
            &carrier_ty,
            &[Ty::Error],
            pattern_span,
        )?;
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
        let TypedInner::Match(value, arms) = &mut body.node else {
            unreachable!("partial mapper Match was validated above");
        };
        let (typed_pattern, _) = self.check_pattern(pattern, &value.ty, pattern_span)?;
        let continuation = arms.remove(0).body;
        body.node = TypedInner::DoSafeBind(Box::new(TypedDoSafeBind {
            pattern: typed_pattern,
            rhs: value.clone(),
            projection: SafeBindRhsProjection::PatternInput {
                pattern_input_ty: value.ty.clone(),
            },
            failure_target,
            continuation: Box::new(continuation),
            origins: DoSafeBindOrigins {
                do_span: do_span.clone(),
                operator_span: statement_span.clone(),
                pattern_span: pattern_span.clone(),
                rhs_span: statement_span.clone(),
                result_span: do_span.clone(),
            },
        }));
        Ok(*self.resolve_typed_node(checked))
    }

    fn statement_question_parts(statement: &Resolved) -> Option<(&Span, &Resolved)> {
        match statement {
            Resolved::StatementQuestion(span, rhs) => Some((span, rhs)),
            Resolved::Semi(_, inner) => Self::statement_question_parts(inner),
            _ => None,
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
        unit_success_only: bool,
        rest: &[ResolvedDoStatement],
        result_span: &Span,
        carrier_hint: Option<&Ty>,
        carrier_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        let inherited_substitutions = self.substitutions.clone();
        let mut checked =
            self.check_safebind_input(statement_span, pattern, rhs, unit_success_only)?;
        checked.typed_pattern = self.resolve_typed_pattern(checked.typed_pattern);
        checked.pattern_ty = self.resolve_ty(&checked.pattern_ty);
        checked.typed_rhs = self.finalize_safebind_node(checked.typed_rhs)?;
        checked.projection = match checked.projection {
            SafeBindRhsProjection::CanonicalResultOnce {
                payload_ty,
                error_ty,
            } => SafeBindRhsProjection::CanonicalResultOnce {
                payload_ty: self.resolve_ty(&payload_ty),
                error_ty: self.resolve_ty(&error_ty),
            },
            SafeBindRhsProjection::PatternInput { pattern_input_ty } => {
                SafeBindRhsProjection::PatternInput {
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
        self.collect_pattern_result_error_types(
            &checked.typed_pattern,
            &mut checked.propagated_error_tys,
        );
        let failure_target = self.resolve_do_pattern_failure_target(
            resolved_contract,
            &continuation_ty,
            &checked.propagated_error_tys,
            operator_span,
        )?;
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

    fn classify_do_statement(
        &mut self,
        do_span: &Span,
        resolved_contract: &sigil::resolved::ResolvedDoContract,
        statement: &Resolved,
        carrier_hint: Option<&Ty>,
    ) -> Result<DoStatementKind, TypeError> {
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
        Ok(statement_kind)
    }

    fn check_do_monadic_statement(
        &mut self,
        do_span: &Span,
        intrinsic: sindr::intrinsic::IntrinsicId,
        resolved_contract: &sigil::resolved::ResolvedDoContract,
        statement: &Resolved,
        rest: &[ResolvedDoStatement],
        carrier_hint: Option<&Ty>,
        carrier_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        let parameter_id = ResolvedId {
            name: "__do_ignored_payload".into(),
            qualified_name: None,
            symbol_info: None,
            unique_id: Self::next_synthetic_range_uid(),
            compiler_generated: true,
            span: self.resolved_span(statement).clone(),
        };
        let parameters = vec![ResolvedClosureParam {
            lazy_capture: None,
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

    fn constructor_trait(id: ResolvedId) -> Arc<TraitInfo> {
        Arc::new(TraitInfo {
            id,
            compiler_owned_equality: false,
            compiler_owned_failure: false,
            compiler_owned_extraction: false,
            standard_lazy_contract: false,
            type_params: Vec::new(),
            where_clause: None,
            constructor_slots: vec!["$A".into()],
            parents: Vec::new(),
            methods: HashMap::new(),
        })
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
            keyword_span: Span { start: 0, end: 2 },
            monad_fail_trait: None,
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

    #[test]
    fn canonical_extraction_requires_trusted_declaration_authority() {
        let mut checker = Checker::new(TypecheckContext::default());
        let span = Span { start: 0, end: 1 };
        let error = checker.canonical_monad_extractor_key(&span).unwrap_err();
        assert_eq!(
            error.reason(),
            Some(TypeDiagnosticReason::TypecheckInvariantViolation)
        );
        let impostor = trait_id("MonadExtractor", "MonadExtractor", 2);
        checker
            .traits
            .insert("MonadExtractor".into(), constructor_trait(impostor));
        let error = checker.canonical_monad_extractor_key(&span).unwrap_err();
        assert_eq!(
            error.reason(),
            Some(TypeDiagnosticReason::TypecheckInvariantViolation)
        );
    }
}

struct CheckedSafeBindInput {
    typed_pattern: TypedPattern,
    pattern_ty: Ty,
    typed_rhs: TypedNode,
    projection: SafeBindRhsProjection,
    propagated_error_tys: Vec<Ty>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DoStatementKind {
    Monadic,
    Plain,
}

impl Checker {
    /// Store statement-local inference in every IR component before the block
    /// checker resets its substitutions for the next statement.
    fn finalize_safebind_node(&mut self, node: TypedNode) -> Result<TypedNode, TypeError> {
        let node = *self
            .concretize_pending_trait_calls(node)
            .map_err(|error| *error)?;
        Ok(self.substitute_typed_node_with_mapping(node, &self.substitutions))
    }

    pub(in crate::checker) fn canonical_monad_extractor_key(
        &self,
        span: &Span,
    ) -> Result<String, TypeError> {
        let info = self
            .traits
            .values()
            .find(|info| info.compiler_owned_extraction)
            .ok_or_else(|| {
                self.typecheck_invariant_error(
                    "canonical MonadExtractor declaration is missing",
                    span,
                )
            })?;
        let method = info.methods.get("extract").ok_or_else(|| {
            self.typecheck_invariant_error("canonical MonadExtractor::extract contract", span)
        })?;
        let parameter = method
            .value_parameters
            .first()
            .map(|parameter| parameter.ty.syntax());
        let slot = match parameter {
            Some(AstTy::Generic(_, head, arguments)) if head == "Self" && arguments.len() == 1 => {
                match &arguments[0] {
                    AstTy::Named(_, slot) if slot.starts_with('$') => Some(slot),
                    _ => None,
                }
            }
            _ => None,
        };
        let result_matches = match (slot, method.ret_ty.syntax()) {
            (Some(slot), AstTy::Generic(_, head, arguments))
                if Self::surface_name(head) == "Result" =>
            {
                matches!(arguments.as_slice(), [AstTy::Named(_, result)] if result == slot)
            }
            _ => false,
        };
        let key = self.trait_key(&info.id);
        self.canonical_extraction_parent_key(&key, span)?;
        if !result_matches
            || method.value_parameters.len() != 1
            || !method.return_type_arguments.is_empty()
            || !info.type_params.is_empty()
            || info.constructor_slots.len() != 1
            || method.where_clause.is_some()
        {
            return Err(
                self.typecheck_invariant_error("canonical MonadExtractor::extract contract", span)
            );
        }
        Ok(key)
    }

    fn canonical_extraction_parent_key(
        &self,
        extraction_key: &str,
        span: &Span,
    ) -> Result<String, TypeError> {
        let parent_key = self
            .traits
            .get(extraction_key)
            .filter(|info| info.compiler_owned_extraction)
            .and_then(|info| {
                info.parents.iter().find_map(|parent| {
                    let key = self.trait_key(&parent.trait_id);
                    self.traits
                        .get(&key)
                        .filter(|info| {
                            key == "Monad"
                                && info.standard_lazy_contract
                                && info.id.unique_id == parent.trait_id.unique_id
                        })
                        .map(|_| key)
                })
            });
        parent_key.ok_or_else(|| {
            self.typecheck_invariant_error("canonical MonadExtractor parent Monad identity", span)
        })
    }

    fn extract_safebind_rhs(&mut self, key: &str, rhs: TypedNode) -> Result<TypedNode, TypeError> {
        let span = rhs.span.clone();
        let input = ResolvedId {
            name: "__extract_input".into(),
            qualified_name: None,
            symbol_info: None,
            unique_id: Self::next_synthetic_range_uid(),
            compiler_generated: true,
            span: span.clone(),
        };
        self.env.push_var_scope();
        self.env.bind_var(input.unique_id, rhs.ty.clone());
        let checked = self.check_trait_invocation(
            &span,
            key,
            sindr::intrinsic::CanonicalTraitMethodIdentity::MonadExtractorExtract.method_name(),
            &[ResolvedRecordLitArg::Positional(Resolved::Var(
                span.clone(),
                input.clone(),
            ))],
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        self.env.pop_var_scope();
        let mut call = checked?;
        let TypedInner::TraitCall { args, .. } = &mut call.node else {
            return Err(self.typecheck_invariant_error(
                "MonadExtractor::extract resolved ordinary Trait call",
                &span,
            ));
        };
        if args.len() != 1
            || !matches!(&args[0].node, TypedInner::Var(id) if id.unique_id == input.unique_id)
        {
            return Err(self.typecheck_invariant_error(
                "resolved MonadExtractor::extract argument contract",
                &span,
            ));
        }
        args[0] = rhs;
        if !matches!(self.resolve_ty(&call.ty), Ty::Result(_, _)) {
            return Err(self.typecheck_invariant_error(
                "MonadExtractor::extract canonical Result return",
                &span,
            ));
        }
        Ok(call)
    }

    /// Resolve every layer before lowering, so an unknown terminal payload is
    /// never inferred as Unit merely because the statement discards its value.
    fn check_statement_question_input(
        &mut self,
        span: &Span,
        pat: &ResolvedPattern,
        rhs: TypedNode,
    ) -> Result<CheckedSafeBindInput, TypeError> {
        let extraction_key = self.canonical_monad_extractor_key(span)?;
        let monad = self.canonical_extraction_parent_key(&extraction_key, span)?;
        let rhs_ty = self.resolve_ty(&rhs.ty);
        let mut current = rhs_ty.clone();
        let mut layers = Vec::new();
        let mut seen = Vec::new();
        let mut propagated_error_tys = Vec::new();
        loop {
            if current == Ty::Unit && !layers.is_empty() {
                break;
            }
            if seen.contains(&current) {
                return Err(TypeError::new(
                    format!("Statement `?` requires a finite MonadExtractor success chain ending at Unit; repeated {}.", self.ty_name(&current)),
                    span.clone(),
                ));
            }
            seen.push(current.clone());
            let (payload, error) = match &current {
                Ty::Result(payload, error) => (self.resolve_ty(payload), self.resolve_ty(error)),
                _ => match self.constructor_slot_type_for(&monad, &current).into_checked()? {
                    ConstructorApplicationOutcome::Applied(payload) => (self.resolve_ty(&payload), Ty::Error),
                    _ => return Err(TypeError::new(
                        format!("Statement `?` requires a MonadExtractor RHS with terminal success type Unit; got {} in {}.", self.ty_name(&current), self.ty_name(&rhs_ty)),
                        span.clone(),
                    )),
                },
            };
            propagated_error_tys.push(error);
            layers.push(payload.clone());
            current = payload;
        }

        // Ordinary calls and matches keep inference, specialization and codegen
        // on their existing paths. Only the final Result reaches SafeBind's
        // failure target; inner failures retain their original Error value.
        let mut input = rhs;
        let mut calls = Vec::new();
        for payload_ty in layers {
            let call = self.extract_safebind_rhs(&extraction_key, input)?;
            let payload_id = ResolvedId {
                name: "__question_payload".into(),
                qualified_name: None,
                symbol_info: None,
                unique_id: Self::next_synthetic_range_uid(),
                compiler_generated: true,
                span: span.clone(),
            };
            input = TypedNode {
                ty: payload_ty,
                span: span.clone(),
                node: TypedInner::Var(payload_id.clone()),
            };
            calls.push((call, payload_id));
        }
        let (mut result, _) = calls.pop().ok_or_else(|| {
            self.typecheck_invariant_error("statement `?` nonempty extraction chain", span)
        })?;
        for (call, payload_id) in calls.into_iter().rev() {
            let Ty::Result(_, error_ty) = self.resolve_ty(&call.ty) else {
                return Err(self.typecheck_invariant_error("statement `?` extraction Result", span));
            };
            let error_id = ResolvedId {
                name: "__question_error".into(),
                qualified_name: None,
                symbol_info: None,
                unique_id: Self::next_synthetic_range_uid(),
                compiler_generated: true,
                span: span.clone(),
            };
            let failure = TypedNode {
                ty: result.ty.clone(),
                span: span.clone(),
                node: TypedInner::ConstructorCall(
                    1,
                    vec![TypedNode {
                        ty: *error_ty,
                        span: span.clone(),
                        node: TypedInner::Var(error_id.clone()),
                    }],
                ),
            };
            result = TypedNode {
                ty: result.ty.clone(),
                span: span.clone(),
                node: TypedInner::Match(
                    Box::new(call),
                    vec![
                        TypedMatchArm {
                            direct_expression: false,
                            pattern: TypedMatchPattern::Constructor {
                                tag: 0,
                                fields: vec![TypedMatchPattern::Binding(payload_id)],
                                field_offset: 0,
                            },
                            guard: None,
                            body: result,
                        },
                        TypedMatchArm {
                            direct_expression: false,
                            pattern: TypedMatchPattern::Constructor {
                                tag: 1,
                                fields: vec![TypedMatchPattern::Binding(error_id)],
                                field_offset: 0,
                            },
                            guard: None,
                            body: failure,
                        },
                    ],
                ),
            };
        }
        let Ty::Result(payload_ty, error_ty) = self.resolve_ty(&result.ty) else {
            return Err(self.typecheck_invariant_error("statement `?` terminal Result", span));
        };
        if *payload_ty != Ty::Unit {
            return Err(self.typecheck_invariant_error("statement `?` terminal Unit", span));
        }
        let (typed_pattern, pattern_ty) = self.check_pattern(pat, &Ty::Unit, span)?;
        Ok(CheckedSafeBindInput {
            typed_pattern,
            pattern_ty,
            typed_rhs: result,
            projection: SafeBindRhsProjection::CanonicalResultOnce {
                payload_ty: *payload_ty,
                error_ty: *error_ty,
            },
            propagated_error_tys,
        })
    }

    fn check_safebind_input(
        &mut self,
        span: &Span,
        pat: &ResolvedPattern,
        rhs: &Resolved,
        unit_success_only: bool,
    ) -> Result<CheckedSafeBindInput, TypeError> {
        let mut typed_rhs = self.check_node(rhs)?;
        if unit_success_only {
            return self.check_statement_question_input(span, pat, typed_rhs);
        }
        if matches!(typed_rhs.ty, Ty::Facet(..)) {
            return Err(self.policy_error(
                TypeDiagnosticReason::FacetSafeBindForbidden,
                diagnostics::TypePolicy::SafeBindFacet,
                Some("Facet value".into()),
                None,
                Some(&typed_rhs.ty),
                Some("Stage1".into()),
                None,
                &typed_rhs.span,
                Some("Use `=` for compile-time Facet bindings.".into()),
            ));
        }
        let rhs_ty = self.resolve_ty(&typed_rhs.ty);
        let extraction_key = self.canonical_monad_extractor_key(span)?;
        let extraction_proof = self.prove_trait_capability(&extraction_key, &rhs_ty)?;
        let extracts = match extraction_proof {
            ApplicabilityProof::Satisfied(_) => true,
            ApplicabilityProof::Deferred(_) | ApplicabilityProof::Unsatisfied => {
                match self
                    .constructor_capability_projection(&extraction_key, &rhs_ty)
                    .into_checked()?
                {
                    // Capability proofs may defer just because a payload is
                    // unknown. Only the declared constructor heads decide
                    // whether an extraction implementation can still apply.
                    // Matching heads retain normal constraint/dispatch errors.
                    ConstructorProjectionOutcome::Applicable { .. } => true,
                    ConstructorProjectionOutcome::Deferred { .. } => {
                        !self.do_failure_carrier_is_rigid_variable(&rhs_ty)
                    }
                    ConstructorProjectionOutcome::Rejected { failures }
                        if failures.iter().all(|failure| {
                            matches!(
                                failure,
                                ConstructorProjectionFailure::NoApplicableImplementation
                            )
                        }) =>
                    {
                        false
                    }
                    ConstructorProjectionOutcome::Rejected { failures }
                        if Self::constructor_projection_failures_are_metadata(&failures) =>
                    {
                        return Err(self.typecheck_invariant_error(
                            "MonadExtractor constructor projection metadata",
                            span,
                        ));
                    }
                    ConstructorProjectionOutcome::Rejected { .. } => true,
                }
            }
        };
        if extracts {
            typed_rhs = self.extract_safebind_rhs(&extraction_key, typed_rhs)?;
        } else {
            let monad = self.canonical_extraction_parent_key(&extraction_key, span)?;
            if let ConstructorApplicationOutcome::Applied(payload_ty) = self
                .constructor_slot_type_for(&monad, &rhs_ty)
                .into_checked()?
            {
                if let Some(lhs_input) =
                    self.safebind_missing_extraction_input(pat, &rhs_ty, &payload_ty)?
                {
                    if self.do_failure_carrier_is_rigid_variable(&rhs_ty) {
                        return Err(self.trait_obligation_failure(
                            TypeDiagnosticReason::MissingGenericBound,
                            &extraction_key,
                            &[],
                            None,
                            &rhs_ty,
                            &typed_rhs.span,
                            DiagnosticOrigin::Intrinsic,
                        ));
                    }
                    return Err(TypeError::from_structured(StructuredDiagnostic {
                        reason: TypeDiagnosticReason::SafeBindMissingMonadExtractor.into(),
                        origin: DiagnosticOrigin::Intrinsic,
                        data: DiagnosticData::SafeBindExtraction(
                            diagnostics::SafeBindExtractionData {
                                rhs_type: self.diagnostic_ty_name(&rhs_ty),
                                payload_type: self.diagnostic_ty_name(&payload_ty),
                                lhs_input,
                                extractor_capability: "unsatisfied".into(),
                            },
                        ),
                        primary: self.type_fact(SourceRole::Value, &typed_rhs.span, &rhs_ty),
                        related: vec![self.type_fact(
                            SourceRole::Pattern,
                            &Self::resolved_pattern_span(pat)?,
                            &payload_ty,
                        )],
                        remediation: None,
                    }));
                }
            }
        }
        let (ok_ty, mut propagated_err_tys, rhs_projection) =
            match (extracts, self.resolve_ty(&typed_rhs.ty)) {
                (true, Ty::Result(ok, err)) => {
                    let payload_ty = ok.as_ref().clone();
                    let error_ty = err.as_ref().clone();
                    (
                        payload_ty.clone(),
                        vec![error_ty.clone()],
                        SafeBindRhsProjection::CanonicalResultOnce {
                            payload_ty,
                            error_ty,
                        },
                    )
                }
                (false, other) => (
                    other.clone(),
                    Vec::new(),
                    SafeBindRhsProjection::PatternInput {
                        pattern_input_ty: other,
                    },
                ),
                (true, _) => unreachable!("extract return contract checked above"),
            };

        let (typed_pat, pat_ty) = self.check_pattern(pat, &ok_ty, span)?;
        let pattern_can_nomatch = !Self::is_total_bind_pattern(pat);
        if pattern_can_nomatch {
            propagated_err_tys.push(Ty::Error);
        } else if !extracts {
            if matches!(&rhs_ty, Ty::Var(variable) if self.rigid_tyvars.contains(variable)) {
                return Err(self.trait_obligation_failure(
                    TypeDiagnosticReason::MissingGenericBound,
                    &extraction_key,
                    &[],
                    None,
                    &rhs_ty,
                    &typed_rhs.span,
                    DiagnosticOrigin::Intrinsic,
                ));
            }
            let rhs_name = self.diagnostic_ty_name(&ok_ty);
            return Err(TypeError::from_structured(StructuredDiagnostic {
                reason: TypeDiagnosticReason::SafeBindTotalPatternNonExtractingRhs.into(),
                origin: DiagnosticOrigin::Intrinsic,
                data: DiagnosticData::SafeBindRelation(diagnostics::SafeBindRelationData {
                    lhs_type: self.diagnostic_ty_name(&pat_ty),
                    rhs_type: rhs_name,
                    lhs_is_total: true,
                }),
                primary: self.type_fact(SourceRole::Value, &typed_rhs.span, &ok_ty),
                related: vec![self.type_fact(SourceRole::Pattern, span, &pat_ty)],
                remediation: None,
            }));
        }
        self.ensure_self_rebinding_types(&typed_pat, span)?;
        Ok(CheckedSafeBindInput {
            typed_pattern: typed_pat,
            pattern_ty: pat_ty,
            typed_rhs,
            projection: rhs_projection,
            propagated_error_tys: propagated_err_tys,
        })
    }

    pub(super) fn check_safebind(
        &mut self,
        span: &Span,
        operator_span: &Span,
        pat: &ResolvedPattern,
        rhs: &Resolved,
        unit_success_only: bool,
    ) -> Result<TypedNode, TypeError> {
        let mut checked = self.check_safebind_input(span, pat, rhs, unit_success_only)?;
        let failure_target = if matches!(
            self.callable_context,
            CallableContext::Extractor | CallableContext::ExtractorClosure
        ) {
            let (_, err_tag) = self.match_result_variant_tags(span)?;
            SafeBindFailureTarget::EnclosingMatchResultContext { err_tag }
        } else if let Some(ret_ty) = self.function_return_ty.clone() {
            self.collect_pattern_result_error_types(
                &checked.typed_pattern,
                &mut checked.propagated_error_tys,
            );
            self.resolve_pattern_failure_target(
                &ret_ty,
                &checked.propagated_error_tys,
                operator_span,
                None,
                false,
            )?
        } else {
            SafeBindFailureTarget::TopLevel
        };

        self.bind_typed_pattern(&checked.typed_pattern, &checked.pattern_ty);
        self.bind_constructor_provenance(
            &checked.typed_pattern,
            self.result_constructor_provenance(&checked.typed_rhs),
        );

        self.finalize_safebind_node(TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::SafeBind(
                checked.typed_pattern,
                Box::new(checked.typed_rhs),
                checked.projection,
                failure_target,
            ),
        })
    }
}
