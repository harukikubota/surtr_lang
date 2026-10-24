use super::*;
use diagnostics::{
    CandidateFailureData, CandidateSelectionData, DiagnosticData, DiagnosticOrigin, SourceFact,
    SourceRole, StructuredDiagnostic, TypeDiagnosticReason,
};

#[derive(Clone, Copy)]
enum CompositionOperator {
    Plain,
    Lifted,
    Kleisli,
}

#[derive(Clone, Copy)]
pub(super) enum ExpectedCallableSlot {
    Plain,
    Contextual,
}

pub(super) struct ExpectedCallableContract {
    input: Ty,
    ret: Option<Ty>,
    slot: ExpectedCallableSlot,
}

impl Checker {
    pub(in crate::checker) fn check_compose_callable(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Capture(_, _, _)
            | Resolved::Closure(_, _, _, _)
            | Resolved::ExtractorClosure(_, _, _, _)
            | Resolved::CaptureClosure(_, _, _, _)
            | Resolved::Compose(_, _, _)
            | Resolved::LiftedCompose(_, _, _)
            | Resolved::KleisliCompose(_, _, _) => self.check_node(node),
            Resolved::Var(_, _)
            | Resolved::Grouped(_, _)
            | Resolved::ReturnTypeArgumentApply(_, _, _) => {
                self.check_function_value_operand(node, op_name)
            }
            _ => Err(TypeError {
                structured: None,
                message: format!("{} requires a function value", op_name),
                span: self.resolved_span(node).clone(),
                hint: Some(format!(
                    "{} composes one-argument function values. This operand is being parsed as a call/result expression first. Use `&f`, a closure, a function-typed variable, or another compose expression. If you meant to compose a function returned by a call, parenthesize the call like `(make_fn(...)) {} (other_fn(...))`.",
                    op_name, op_name
                )),
            }),
        }
    }

    pub(in crate::checker) fn check_operator_compose_callable(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        self.check_function_value_operand(node, op_name)
    }

    pub(in crate::checker) fn check_apply_callable(
        &mut self,
        node: &Resolved,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        match node {
            Resolved::Capture(_, _, _)
            | Resolved::Closure(_, _, _, _)
            | Resolved::ExtractorClosure(_, _, _, _) | Resolved::CaptureClosure(_, _, _, _) => self.check_node(node),
            Resolved::Var(_, _) | Resolved::Grouped(_, _) | Resolved::ReturnTypeArgumentApply(_, _, _) => {
                self.check_function_value_operand(node, op_name)
            }
            Resolved::App(span, func, args) => {
                self.check_injected_call(span, func, args, op_name)
            }
            _ => Err(TypeError {
                structured: None,
                message: format!(
                    "{} requires a function value or a function call like `f(...)`",
                    op_name
                ),
                span: self.resolved_span(node).clone(),
                hint: Some("Use `&f`, a closure, a function-typed variable, or wrap a callable-returning call in parentheses.".into()),
            }),
        }
    }

    fn can_use_expected_callable_context(node: &Resolved) -> bool {
        matches!(
            node,
            Resolved::InferredFacetCapture(_, _)
                | Resolved::Capture(_, _, _)
                | Resolved::Closure(_, _, _, _)
                | Resolved::ExtractorClosure(_, _, _, _)
                | Resolved::CaptureClosure(_, _, _, _)
                | Resolved::Grouped(_, _)
                | Resolved::ReturnTypeArgumentApply(_, _, _)
        )
    }

    fn expected_callable_ty(&mut self, contract: &ExpectedCallableContract) -> Ty {
        match contract.slot {
            ExpectedCallableSlot::Plain | ExpectedCallableSlot::Contextual => {}
        }
        Ty::Func(
            vec![self.resolve_ty(&contract.input)],
            Box::new(
                contract
                    .ret
                    .as_ref()
                    .map(|ret| self.resolve_ty(ret))
                    .unwrap_or_else(|| self.env.fresh_tyvar()),
            ),
        )
    }

    pub(super) fn callable_contract(
        &mut self,
        input_ty: &Ty,
        ret_ty: Option<Ty>,
        slot: ExpectedCallableSlot,
    ) -> ExpectedCallableContract {
        ExpectedCallableContract {
            input: self.resolve_ty(input_ty),
            ret: ret_ty.map(|ty| self.resolve_ty(&ty)),
            slot,
        }
    }

    pub(super) fn check_apply_callable_with_contract(
        &mut self,
        node: &Resolved,
        contract: &ExpectedCallableContract,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        if Self::can_use_expected_callable_context(node) {
            let expected = self.expected_callable_ty(contract);
            self.check_node_with_expected(node, Some(&expected))
        } else {
            self.check_apply_callable(node, op_name)
        }
    }

    fn check_compose_callable_with_contract(
        &mut self,
        node: &Resolved,
        contract: &ExpectedCallableContract,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        if Self::can_use_expected_callable_context(node) {
            let expected = self.expected_callable_ty(contract);
            let typed = self.check_node_with_expected(node, Some(&expected))?;
            // Connect contextual output variables to the checked callable before
            // its pending method calls are finalized by the enclosing expression.
            // Input compatibility remains the composition operator's obligation.
            if let Some(output) = &contract.ret {
                let (_, actual_output) =
                    self.unary_function_parts(&typed.ty, op_name, &typed.span)?;
                self.assert_type_relation(
                    output,
                    &actual_output,
                    self.type_fact(SourceRole::Expected, &typed.span, output),
                    self.type_fact(SourceRole::Value, &typed.span, &actual_output),
                    TypeDiagnosticReason::ReturnTypeMismatch,
                    DiagnosticOrigin::Return,
                    op_name,
                    0,
                )?;
            }
            Ok(typed)
        } else {
            self.check_operator_compose_callable(node, op_name)
        }
    }

    fn expected_unary_function_parts(&mut self, expected: Option<&Ty>) -> Option<(Ty, Ty)> {
        let expected = self.resolve_ty(expected?);
        let Ty::Func(params, ret) = expected else {
            return None;
        };
        if params.len() == 1 {
            Some((self.resolve_ty(&params[0]), self.resolve_ty(ret.as_ref())))
        } else {
            None
        }
    }

    fn operator_trait_dispatch_for_args(
        &mut self,
        trait_name: &str,
        method_name: &str,
        receiver_ty: &Ty,
        requested_trait_args: &[Ty],
        argument_tys: &[Ty],
        result_ty: &Ty,
    ) -> Result<Option<(TraitDispatch, Vec<Ty>)>, TypeError> {
        let profile = self.profiler.start();
        let dispatch = self.select_method_dispatch(
            trait_name,
            method_name,
            receiver_ty,
            requested_trait_args,
            argument_tys,
            result_ty,
        );
        self.profiler
            .finish(ProfileEvent::OperatorTraitCandidateScan, profile);
        Ok(dispatch?.map(|dispatch| {
            (
                dispatch,
                requested_trait_args
                    .iter()
                    .map(|ty| self.resolve_ty(ty))
                    .collect(),
            )
        }))
    }

    pub(super) fn opposite_conversion_hint(
        &self,
        trait_name: &str,
        method_name: &str,
        receiver_ty: &Ty,
        requested_trait_args: &[Ty],
        span: &Span,
    ) -> Option<TypeError> {
        let requested_trait = if self.trait_matches_short_name(trait_name, "Convert") {
            "Convert"
        } else if self.trait_matches_short_name(trait_name, "TryConvert") {
            "TryConvert"
        } else {
            return None;
        };
        let opposite_trait = if requested_trait == "Convert" {
            "TryConvert"
        } else {
            "Convert"
        };
        let receiver_name = self.trait_target_name(receiver_ty)?;
        let target_ty = self.resolve_ty(requested_trait_args.first()?);
        let opposite_trait_key = self.trait_key_by_short_name(opposite_trait)?;
        let receiver_ty = self.resolve_ty(receiver_ty);
        let has_opposite_impl = self.trait_impls.values().any(|impl_info| {
            self.trait_key(&impl_info.trait_id) == opposite_trait_key
                && impl_info.trait_arg_tys == [target_ty.clone()]
                && impl_info.target_ty == receiver_ty
        });
        if !has_opposite_impl {
            return None;
        }

        let target_name = self.ty_name(&target_ty);
        let opposite_method = if opposite_trait == "Convert" {
            "to"
        } else {
            "try_to"
        };
        Some(TypeError {
            structured: None,
            message: format!(
                "{} -> {} implements {}, not {}. Use {}::<{}>(value).",
                receiver_name,
                target_name,
                opposite_trait,
                requested_trait,
                opposite_method,
                target_name
            ),
            span: span.clone(),
            hint: Some(format!(
                "{}::{} is not available for this conversion pair.",
                self.trait_display_name(trait_name),
                method_name
            )),
        })
    }

    fn check_trait_helper_pipe_callable(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
        input_ty: Ty,
        ret_ty: Option<Ty>,
        op_name: &str,
    ) -> Result<Option<TypedNode>, TypeError> {
        if self.trait_method_ref(func).is_none() {
            return Ok(None);
        }
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{} does not support named arguments on the right-hand side",
                    op_name
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let param_id = ResolvedId {
            name: "__pipe_trait_helper_arg".into(),
            qualified_name: None,
            symbol_info: None,
            unique_id: Self::next_synthetic_range_uid(),
            compiler_generated: true,
            span: span.clone(),
        };
        let mut body_args = Vec::with_capacity(args.len() + 1);
        body_args.push(ResolvedRecordLitArg::Positional(Resolved::Var(
            span.clone(),
            param_id.clone(),
        )));
        body_args.extend(args.iter().cloned());

        let synthetic = Resolved::Closure(
            span.clone(),
            vec![ResolvedClosureParam {
                lazy_capture: None,
                id: param_id,
                ty: None,
            }],
            Vec::new(),
            Box::new(Resolved::App(
                span.clone(),
                Box::new(func.clone()),
                body_args,
            )),
        );
        let contract = self.callable_contract(&input_ty, ret_ty, ExpectedCallableSlot::Contextual);
        let expected = self.expected_callable_ty(&contract);
        self.check_node_with_expected(&synthetic, Some(&expected))
            .map(Some)
    }

    fn flow_operator_trait_call(
        &mut self,
        span: &Span,
        trait_short_name: &str,
        method_name: &str,
        receiver_ty: &Ty,
        requested_trait_args: Vec<Ty>,
        op: OperatorTraitOp,
        args: Vec<TypedNode>,
        result_ty: Ty,
        op_name: &str,
    ) -> Result<TypedNode, TypeError> {
        let trait_key = self
            .trait_key_by_short_name(trait_short_name)
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown trait: {}", trait_short_name),
                span: span.clone(),
                hint: None,
            })?;
        let Some((dispatch, resolved_trait_args)) = self.operator_trait_dispatch_for_args(
            &trait_key,
            method_name,
            receiver_ty,
            &requested_trait_args,
            &args
                .iter()
                .map(|arg| self.resolve_ty(&arg.ty))
                .collect::<Vec<_>>(),
            &result_ty,
        )?
        else {
            return Err(self.trait_obligation_failure(
                TypeDiagnosticReason::NoApplicableTraitImplementation,
                &trait_key,
                &requested_trait_args,
                Some(method_name),
                receiver_ty,
                span,
                DiagnosticOrigin::Operator {
                    operator: op_name.trim_matches('`').into(),
                },
            ));
        };
        let trait_name = self.trait_instance_key_from_tys(&trait_key, &resolved_trait_args);
        let lhs_ty = args
            .first()
            .map(|arg| self.resolve_ty(&arg.ty))
            .unwrap_or_else(|| self.resolve_ty(receiver_ty));
        let rhs_ty = args
            .get(1)
            .map(|arg| self.resolve_ty(&arg.ty))
            .unwrap_or(Ty::Unit);
        Ok(TypedNode {
            ty: self.resolve_ty(&result_ty),
            span: span.clone(),
            node: TypedInner::TraitCall {
                trait_name,
                method_name: method_name.into(),
                receiver_ty: self.resolve_ty(receiver_ty),
                obligation: TraitObligation {
                    trait_id: trait_key,
                    trait_args: resolved_trait_args,
                    receiver: self.resolve_ty(receiver_ty),
                },
                dispatch,
                origin: TraitCallOrigin::Operator { op, lhs_ty, rhs_ty },
                args,
            },
        })
    }

    pub(in crate::checker) fn check_pipe(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_pipe_with_expected(span, left, right, None)
    }

    pub(super) fn check_pipe_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        (|| {
            let typed_left = self.check_node(left)?;
            let rhs_ret_expected = expected.map(|ty| self.resolve_ty(ty));
            let typed_right = match right {
                Resolved::InferredFacetCapture(_, _)
                | Resolved::Capture(_, _, _)
                | Resolved::Closure(_, _, _, _)
                | Resolved::ExtractorClosure(_, _, _, _)
                | Resolved::CaptureClosure(_, _, _, _)
                | Resolved::Grouped(_, _) => {
                    let contract = self.callable_contract(
                        &typed_left.ty,
                        rhs_ret_expected.clone(),
                        ExpectedCallableSlot::Plain,
                    );
                    self.check_apply_callable_with_contract(right, &contract, "`|>`")?
                }
                Resolved::App(call_span, func, args) => {
                    if let Some(typed) = self.check_trait_helper_pipe_callable(
                        call_span,
                        func,
                        args,
                        self.resolve_ty(&typed_left.ty),
                        rhs_ret_expected.clone(),
                        "`|>`",
                    )? {
                        typed
                    } else {
                        self.check_apply_callable(right, "`|>`")?
                    }
                }
                Resolved::ReturnTypeArgumentApply(call_span, _, _) => self
                    .check_trait_helper_pipe_callable(
                        call_span,
                        right,
                        &[],
                        self.resolve_ty(&typed_left.ty),
                        rhs_ret_expected.clone(),
                        "`|>`",
                    )?
                    .ok_or_else(|| TypeError {
                        structured: None,
                        message: "`|>` requires a specialized trait helper on the right".into(),
                        span: call_span.clone(),
                        hint: None,
                    })?,
                _ => self.check_apply_callable(right, "`|>`")?,
            };
            let (param, ret) =
                self.unary_function_parts(&typed_right.ty, "`|>`", &typed_right.span)?;
            self.assert_operand_relation(
                &param,
                &typed_left.ty,
                &typed_left,
                &typed_right,
                TypeDiagnosticReason::ArgumentTypeMismatch,
                "|>",
                "|>",
                None,
                SourceRole::RightValue,
            )?;
            Ok(TypedNode {
                ty: self.resolve_ty(&ret),
                span: span.clone(),
                node: TypedInner::Pipe(Box::new(typed_left), Box::new(typed_right)),
            })
        })()
        .map_err(|error| {
            Self::operator_operand_error(
                error,
                "|>",
                self.resolved_span(left),
                self.resolved_span(right),
            )
        })
    }

    pub(super) fn assert_contextual_callable_argument(
        &mut self,
        expected: &Ty,
        argument: &TypedNode,
        source: &TypedNode,
        capability: &str,
        contextual_return: bool,
        callable: &str,
        ordinal: u32,
    ) -> Result<(), TypeError> {
        let expected = self.resolve_ty(expected);
        let actual = self.resolve_ty(&argument.ty);
        let (Some((inputs, output)), Some((actual_inputs, actual_output))) =
            (self.function_parts(&expected), self.function_parts(&actual))
        else {
            return self.assert_type_relation(
                &expected,
                &actual,
                self.type_fact(SourceRole::LeftValue, &source.span, &source.ty),
                self.type_fact(SourceRole::RightValue, &argument.span, &argument.ty),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::TraitCall,
                callable,
                ordinal,
            );
        };
        if inputs.len() != actual_inputs.len() {
            return Err(self.callable_shape_error(
                &actual,
                callable,
                &argument.span,
                Some(inputs.len()),
            ));
        }
        let inputs = inputs.to_vec();
        let actual_inputs = actual_inputs.to_vec();
        let output = output.clone();
        let actual_output = actual_output.clone();
        for (input, actual_input) in inputs.iter().zip(&actual_inputs) {
            self.assert_operand_relation(
                input,
                actual_input,
                source,
                argument,
                TypeDiagnosticReason::TypePayloadMismatch,
                callable,
                "",
                Some(capability),
                SourceRole::LeftValue,
            )
            .map_err(Self::trait_helper_origin)?;
        }
        if contextual_return {
            self.assert_carrier_relation(
                &output,
                &actual_output,
                source,
                argument,
                capability,
                "",
                SourceRole::LeftValue,
            )
            .map_err(Self::trait_helper_origin)
        } else {
            self.assert_type_relation(
                &output,
                &actual_output,
                self.type_fact(SourceRole::LeftValue, &source.span, &source.ty),
                self.type_fact(SourceRole::RightValue, &argument.span, &argument.ty),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::TraitCall,
                callable,
                ordinal,
            )
        }
    }

    pub(super) fn check_invocation_argument(
        &mut self,
        expr: &Resolved,
        expected: &Ty,
        operator: Option<OperatorTraitOp>,
        index: usize,
        expected_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        if index == 1
            && matches!(
                operator,
                Some(OperatorTraitOp::PipeMap | OperatorTraitOp::PipeBind)
            )
        {
            if let Resolved::ReturnTypeArgumentApply(span, _, _) = expr {
                if let Ty::Func(inputs, output) = self.resolve_ty(expected) {
                    if let [input] = inputs.as_slice() {
                        if let Some(typed) = self.check_trait_helper_pipe_callable(
                            span,
                            expr,
                            &[],
                            input.clone(),
                            Some(*output),
                            "context operator",
                        )? {
                            return Ok(typed);
                        }
                    }
                }
            }

            if let Resolved::App(span, func, args) = expr {
                let resolved = self.resolve_ty(expected);
                if let Ty::Func(inputs, output) = resolved {
                    if let [input] = inputs.as_slice() {
                        if let Some(typed) = self.check_trait_helper_pipe_callable(
                            span,
                            func,
                            args,
                            input.clone(),
                            Some(*output),
                            "context operator",
                        )? {
                            return Ok(typed);
                        }
                    }
                }
                return self.check_apply_callable(expr, "context operator");
            }
        }
        // An inference-only constructor witness is an argument constraint,
        // not a receiver conversion for a nested producer. Check that value
        // once before choosing the declared standard Error boundary. Concrete
        // Result and rigid constructor contexts keep their contextual typing.
        let deferred_constructor = match expected {
            Ty::SelfApp(items) => {
                Self::constructor_application_parts(items).is_some_and(|(witness, _)| {
                    matches!(self.resolve_ty(witness), Ty::Var(variable)
                        if !self.rigid_tyvars.contains(&variable))
                })
            }
            _ => false,
        };
        let mut typed = if deferred_constructor {
            self.check_node(expr)?
        } else {
            self.check_node_with_expected_relation(expr, Some(expected), expected_relation)?
        };
        self.normalize_generic_error_carrier_argument(expected, &mut typed)?;
        Ok(typed)
    }

    fn check_operator_invocation(
        &mut self,
        span: &Span,
        trait_short_name: &str,
        method_name: &str,
        op: OperatorTraitOp,
        token: &str,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let trait_name = self
            .trait_key_by_short_name(trait_short_name)
            .ok_or_else(|| {
                TypeError::new(format!("Unknown trait: {trait_short_name}"), span.clone())
            })?;
        let args = vec![
            ResolvedRecordLitArg::Positional(left.clone()),
            ResolvedRecordLitArg::Positional(right.clone()),
        ];
        let mut typed = self
            .check_trait_invocation(
                span,
                &trait_name,
                method_name,
                &args,
                None,
                expected,
                None,
                None,
                None,
                None,
                Some(op.clone()),
                None,
            )
            .map_err(|mut error| {
                if let Some(diagnostic) = &mut error.structured {
                    if diagnostic.origin == DiagnosticOrigin::TraitCall {
                        diagnostic.origin = DiagnosticOrigin::Operator {
                            operator: token.into(),
                        };
                        diagnostic.map_source_facts(|fact| {
                            if fact.span == *self.resolved_span(left) {
                                fact.role = SourceRole::LeftValue;
                            }
                            if fact.span == *self.resolved_span(right) {
                                fact.role = SourceRole::RightValue;
                            }
                        });
                    }
                }
                Self::operator_operand_error(
                    error,
                    token,
                    self.resolved_span(left),
                    self.resolved_span(right),
                )
            })?;
        if let TypedInner::TraitCall { origin, args, .. } = &mut typed.node {
            *origin = TraitCallOrigin::Operator {
                op,
                lhs_ty: self.resolve_ty(&args[0].ty),
                rhs_ty: self.resolve_ty(&args[1].ty),
            };
        }
        Ok(typed)
    }

    pub(in crate::checker) fn check_context_apply(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_context_apply_with_expected(span, left, right, None)
    }

    pub(super) fn check_context_apply_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        self.check_operator_invocation(
            span,
            "Applicative",
            "ap",
            OperatorTraitOp::ContextApply,
            "|*|",
            left,
            right,
            expected,
        )
    }

    pub(in crate::checker) fn check_context_map(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_context_map_with_expected(span, left, right, None)
    }

    pub(super) fn check_context_map_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        self.check_operator_invocation(
            span,
            "Functor",
            "fmap",
            OperatorTraitOp::PipeMap,
            "|*>",
            left,
            right,
            expected,
        )
    }

    pub(in crate::checker) fn check_context_bind(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_context_bind_with_expected(span, left, right, None)
    }

    /// Roll back speculative checking while returning source type-syntax
    /// failures separately from candidate-dependent invocation failures.
    fn probe_constructor_invocation<T>(
        &mut self,
        probe: impl FnOnce(&mut Self) -> Result<T, TypeError>,
    ) -> Result<Result<T, TypeError>, TypeError> {
        let checkpoint = self.candidate_probe_checkpoint();
        let retained = Rc::new(RefCell::new(None));
        let outer_probe = self.type_syntax_probe_error.replace(retained.clone());
        let result = probe(self);
        self.type_syntax_probe_error = outer_probe;
        self.rollback_candidate_probe(checkpoint);
        if let Some(error) = retained.borrow_mut().take() {
            if let Some(outer) = &self.type_syntax_probe_error {
                let mut retained = outer.borrow_mut();
                if retained.is_none() {
                    *retained = Some(error.clone());
                }
            }
            return Err(error);
        }
        Ok(result)
    }

    /// A receiverless constructor helper exposes its mapped input through its
    /// signature. Probe each carrier with the complete invocation constraints;
    /// only a unique successful carrier may commit its substitutions.
    pub(super) fn try_constructor_invocation_from_arguments(
        &mut self,
        span: &Span,
        trait_name: &str,
        method_name: &str,
        args: &[ResolvedRecordLitArg],
        declared_params: &[Ty],
        expected: Option<&Ty>,
        expected_relation: Option<(&ExpectedTypeRelation, &Span)>,
        argument_expected_relation: Option<&ExpectedTypeRelation>,
        constructor_failure_reason: Option<TypeDiagnosticReason>,
        explicit: Option<&[AstTy]>,
        operator: Option<OperatorTraitOp>,
        receiver_index: usize,
    ) -> Result<Option<TypedNode>, TypeError> {
        let ResolvedRecordLitArg::Positional(Resolved::App(_, helper, helper_args)) =
            &args[receiver_index]
        else {
            return Ok(None);
        };
        let Some((_, helper_trait, helper_method)) = self.trait_method_ref(helper) else {
            return Ok(None);
        };
        let helper_info = self.traits[&helper_trait].clone();
        let helper_method = &helper_info.methods[&helper_method];
        if helper_info.constructor_slots.len() != 1 {
            return Ok(None);
        }
        let [ResolvedRecordLitArg::Positional(value)] = helper_args.as_slice() else {
            return Ok(None);
        };
        let helper_self = self.env.fresh_tyvar();
        let (helper_params, helper_result, _, _, _) =
            self.resolve_trait_method_signature(&helper_info, helper_method, &helper_self)?;
        let (Some(parameter), Ty::SelfApp(slots)) = (helper_params.first(), &helper_result) else {
            return Ok(None);
        };
        if helper_params.len() != 1 || slots.as_slice() != std::slice::from_ref(parameter) {
            return Ok(None);
        }
        let typed_value = self.check_node(value)?;
        let value_ty = self.resolve_ty(&typed_value.ty);
        let mut carrier_evidence = match expected {
            Some(ty) => match self
                .constructor_context_type_for(trait_name, ty, &value_ty)
                .into_checked()
                .map_err(|error| error.at_span(span))?
            {
                ConstructorApplicationOutcome::Applied(context) => Some(context),
                ConstructorApplicationOutcome::Deferred { .. }
                | ConstructorApplicationOutcome::Rejected { .. } => None,
            },
            None => None,
        };
        if carrier_evidence.is_none() && Self::explicit_type_args(helper).is_some() {
            let ResolvedRecordLitArg::Positional(receiver) = &args[receiver_index] else {
                unreachable!()
            };
            let typed = self.check_node(receiver)?;
            carrier_evidence = match self
                .constructor_context_type_for(trait_name, &typed.ty, &value_ty)
                .into_checked()
                .map_err(|error| error.at_span(&typed.span))?
            {
                ConstructorApplicationOutcome::Applied(context) => Some(context),
                ConstructorApplicationOutcome::Deferred { .. }
                | ConstructorApplicationOutcome::Rejected { .. } => None,
            };
        }
        if carrier_evidence.is_none() {
            for (index, (argument, parameter)) in args.iter().zip(declared_params).enumerate() {
                if index == receiver_index {
                    continue;
                }
                let ResolvedRecordLitArg::Positional(argument) = argument else {
                    continue;
                };
                carrier_evidence = self
                    .probe_constructor_invocation(|checker| {
                        let observed = match parameter {
                            Ty::SelfApp(_) => Some(checker.check_node(argument)?.ty),
                            Ty::Func(inputs, output)
                                if inputs.len() == 1
                                    && matches!(output.as_ref(), Ty::SelfApp(_)) =>
                            {
                                let result = checker.env.fresh_tyvar();
                                let callable = Ty::Func(vec![value_ty.clone()], Box::new(result));
                                let typed = checker.check_invocation_argument(
                                    argument,
                                    &callable,
                                    operator.clone(),
                                    index,
                                    None,
                                )?;
                                checker
                                    .function_parts(&typed.ty)
                                    .map(|(_, output)| checker.resolve_ty(output))
                            }
                            _ => None,
                        };
                        Ok(observed.and_then(|observed| {
                            match checker
                                .constructor_context_type_for(trait_name, &observed, &value_ty)
                            {
                                ConstructorApplicationOutcome::Applied(context) => Some(context),
                                ConstructorApplicationOutcome::Deferred { .. }
                                | ConstructorApplicationOutcome::Rejected { .. } => None,
                            }
                        }))
                    })?
                    .ok()
                    .flatten();
                if carrier_evidence.is_some() {
                    break;
                }
            }
        }
        let candidates = match &carrier_evidence {
            Some(carrier) => vec![carrier.clone()],
            None => self.constructor_context_candidates(trait_name, &value_ty)?,
        };
        let mut accepted = Vec::new();
        let mut failures = Vec::new();
        for candidate in candidates {
            let result = self.probe_constructor_invocation(|checker| {
                checker.check_trait_invocation(
                    span,
                    trait_name,
                    method_name,
                    args,
                    None,
                    expected,
                    expected_relation,
                    argument_expected_relation,
                    constructor_failure_reason,
                    explicit,
                    operator.clone(),
                    Some(&candidate),
                )
            })?;
            match result {
                Ok(_) => accepted.push(candidate.clone()),
                Err(error) if carrier_evidence.is_some() => return Err(error),
                Err(error) => failures.push(CandidateFailureData {
                    candidate_type: self.diagnostic_ty_name(&candidate),
                    detail: error.message,
                }),
            }
        }
        if !accepted.is_empty() && carrier_evidence.is_none() {
            return Err(self.ambiguous_constructor_result(trait_name, method_name, span));
        }
        if let [candidate] = accepted.as_slice() {
            return self
                .check_trait_invocation(
                    span,
                    trait_name,
                    method_name,
                    args,
                    None,
                    expected,
                    expected_relation,
                    argument_expected_relation,
                    constructor_failure_reason,
                    explicit,
                    operator,
                    Some(candidate),
                )
                .map(Some);
        }
        if !accepted.is_empty() {
            return Err(self.ambiguous_constructor_result(trait_name, method_name, span));
        }
        Err(TypeError::from_structured(StructuredDiagnostic {
            reason: TypeDiagnosticReason::NoApplicableTraitImplementation.into(),
            origin: DiagnosticOrigin::TraitCall,
            data: DiagnosticData::CandidateSelection(CandidateSelectionData {
                subject_type: Some(self.diagnostic_ty_name(&value_ty)),
                trait_arguments: vec![],
                impl_declaration: None,
                trait_name: self.trait_display_name(trait_name),
                method: method_name.into(),
                failures,
            }),
            primary: self.type_fact(SourceRole::Value, &typed_value.span, &typed_value.ty),
            related: vec![],
            remediation: None,
        }))
    }

    pub(super) fn check_context_bind_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        self.check_operator_invocation(
            span,
            "Monad",
            "bind",
            OperatorTraitOp::PipeBind,
            "|>=",
            left,
            right,
            expected,
        )
    }

    fn prepare_composition_invocation(
        &mut self,
        span: &Span,
        kind: &CompositionOperator,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<(Vec<TypedNode>, Vec<Ty>), TypeError> {
        let (label, capability) = match kind {
            CompositionOperator::Plain => ("Bootstrap::>>", None),
            CompositionOperator::Lifted => ("Bootstrap::>*", Some("Functor")),
            CompositionOperator::Kleisli => ("Bootstrap::>=>", Some("Monad")),
        };
        let expected_parts = self.expected_unary_function_parts(expected);
        let expected_output = expected_parts.as_ref().map(|(_, output)| output.clone());
        let typed_left = if let Some((input, output)) = &expected_parts {
            let output_hint = if let Some(capability) = capability {
                let payload = self.env.fresh_tyvar();
                match self.constructor_context_type_for(capability, output, &payload) {
                    ConstructorApplicationOutcome::Applied(context) => Some(context),
                    ConstructorApplicationOutcome::Deferred { .. }
                    | ConstructorApplicationOutcome::Rejected { .. } => None,
                }
            } else {
                None
            };
            let contract =
                self.callable_contract(input, output_hint, ExpectedCallableSlot::Contextual);
            self.check_compose_callable_with_contract(left, &contract, label)?
        } else {
            self.check_operator_compose_callable(left, label)?
        };
        let (left_input, left_output) =
            self.unary_function_parts(&typed_left.ty, label, &typed_left.span)?;
        let input = if let Some(capability) = capability {
            let outcome = self.constructor_slot_type_for(capability, &left_output);
            self.require_constructor_projection_type(
                outcome,
                capability,
                &left_output,
                &typed_left.span,
                label,
            )
            .map_err(|mut error| {
                if let Some(diagnostic) = &mut error.structured {
                    diagnostic.map_source_facts(|fact| {
                        if fact.span == typed_left.span {
                            fact.ty = Some(self.diagnostic_ty_name(&typed_left.ty));
                            fact.role = SourceRole::LeftValue;
                        }
                    });
                }
                error
            })?
        } else {
            left_output.clone()
        };
        let result_payload = self.env.fresh_tyvar();
        let result_context = if matches!(kind, CompositionOperator::Kleisli) {
            let outcome = self.constructor_context_type_for("Monad", &left_output, &result_payload);
            Some(self.require_constructor_projection_type(
                outcome,
                "Monad",
                &left_output,
                span,
                label,
            )?)
        } else {
            None
        };
        let output_hint = match kind {
            CompositionOperator::Plain => expected_output.clone(),
            CompositionOperator::Lifted => expected_output.as_ref().and_then(|ty| {
                match self.constructor_slot_type_for("Functor", ty) {
                    ConstructorApplicationOutcome::Applied(slot) => Some(slot),
                    ConstructorApplicationOutcome::Deferred { .. }
                    | ConstructorApplicationOutcome::Rejected { .. } => None,
                }
            }),
            CompositionOperator::Kleisli => {
                expected_output.clone().or_else(|| result_context.clone())
            }
        };
        let contract =
            self.callable_contract(&input, output_hint, ExpectedCallableSlot::Contextual);
        let typed_right = self.check_compose_callable_with_contract(right, &contract, label)?;
        let (_, right_output) =
            self.unary_function_parts(&typed_right.ty, label, &typed_right.span)?;
        let trait_args = match kind {
            CompositionOperator::Plain => vec![left_input, left_output, right_output],
            CompositionOperator::Lifted => {
                let outcome =
                    self.constructor_context_type_for("Functor", &left_output, &right_output);
                let mapped = self.require_constructor_projection_type(
                    outcome,
                    "Functor",
                    &left_output,
                    span,
                    label,
                )?;
                vec![left_input, input, right_output, mapped]
            }
            CompositionOperator::Kleisli => vec![
                left_input,
                input,
                result_context.expect("Kleisli contextual result"),
            ],
        };
        Ok((vec![typed_left, typed_right], trait_args))
    }

    pub(in crate::checker) fn check_compose(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_compose_with_expected(span, left, right, None)
    }

    pub(super) fn check_compose_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let (args, _) = self.prepare_composition_invocation(
            span,
            &CompositionOperator::Plain,
            left,
            right,
            expected,
        )?;
        let [typed_left, typed_right] =
            <[TypedNode; 2]>::try_from(args).expect("composition preparation returns two operands");
        let (input, middle) = self.unary_function_parts(&typed_left.ty, ">>", &typed_left.span)?;
        let (next_input, output) =
            self.unary_function_parts(&typed_right.ty, ">>", &typed_right.span)?;
        self.assert_operand_relation(
            &next_input,
            &middle,
            &typed_left,
            &typed_right,
            TypeDiagnosticReason::ArgumentTypeMismatch,
            ">>",
            ">>",
            None,
            SourceRole::RightValue,
        )?;
        Ok(TypedNode {
            ty: Ty::Func(
                vec![self.resolve_ty(&input)],
                Box::new(self.resolve_ty(&output)),
            ),
            span: span.clone(),
            node: TypedInner::Compose(
                ComposeFlavor::Plain,
                Box::new(typed_left),
                Box::new(typed_right),
            ),
        })
    }

    pub(in crate::checker) fn check_lifted_compose(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_lifted_compose_with_expected(span, left, right, None)
    }

    pub(super) fn check_lifted_compose_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        self.check_contextual_compose_with_expected(
            span,
            left,
            right,
            expected,
            CompositionOperator::Lifted,
        )
    }

    pub(in crate::checker) fn check_kleisli_compose(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_kleisli_compose_with_expected(span, left, right, None)
    }

    pub(super) fn check_kleisli_compose_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        self.check_contextual_compose_with_expected(
            span,
            left,
            right,
            expected,
            CompositionOperator::Kleisli,
        )
    }

    fn check_contextual_compose_with_expected(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
        kind: CompositionOperator,
    ) -> Result<TypedNode, TypeError> {
        let (values, trait_args) =
            self.prepare_composition_invocation(span, &kind, left, right, expected)?;
        let [typed_left, typed_right] = <[TypedNode; 2]>::try_from(values)
            .expect("composition preparation returns two operands");
        let (trait_name, method_name, op, token) = match kind {
            CompositionOperator::Lifted => ("Functor", "fmap", OperatorTraitOp::PipeMap, ">*"),
            CompositionOperator::Kleisli => ("Monad", "bind", OperatorTraitOp::PipeBind, ">=>"),
            _ => unreachable!("contextual composition kind"),
        };
        let input_ty = self.resolve_ty(&trait_args[0]);
        let source_ty = self.resolve_ty(&typed_left.ty);
        let (source_input, carrier_ty) =
            self.unary_function_parts(&source_ty, token, &typed_left.span)?;
        self.assert_operand_relation(
            &source_input,
            &input_ty,
            &typed_left,
            &typed_right,
            TypeDiagnosticReason::ArgumentTypeMismatch,
            token,
            token,
            None,
            SourceRole::LeftValue,
        )?;
        let output_ty = match kind {
            CompositionOperator::Lifted => self.resolve_ty(&trait_args[3]),
            CompositionOperator::Kleisli => self.resolve_ty(&trait_args[2]),
            _ => unreachable!(),
        };
        let make_id = |name: &str| ResolvedId {
            name: name.into(),
            qualified_name: None,
            symbol_info: None,
            unique_id: Self::next_synthetic_range_uid(),
            compiler_generated: true,
            span: span.clone(),
        };
        let left_id = make_id("__compose_left");
        let right_id = make_id("__compose_right");
        let input_id = make_id("__compose_input");
        let left_ref = TypedNode {
            ty: source_ty.clone(),
            span: span.clone(),
            node: TypedInner::Var(left_id.clone()),
        };
        let right_ref = TypedNode {
            ty: self.resolve_ty(&typed_right.ty),
            span: span.clone(),
            node: TypedInner::Var(right_id.clone()),
        };
        let input_ref = TypedNode {
            ty: input_ty.clone(),
            span: span.clone(),
            node: TypedInner::Var(input_id.clone()),
        };
        let produced = TypedNode {
            ty: carrier_ty.clone(),
            span: span.clone(),
            node: TypedInner::App(Box::new(left_ref), vec![input_ref]),
        };
        let body = self.flow_operator_trait_call(
            span,
            trait_name,
            method_name,
            &carrier_ty,
            vec![],
            op,
            vec![produced, right_ref],
            output_ty.clone(),
            token,
        )?;
        let closure_ty = Ty::Func(vec![input_ty.clone()], Box::new(output_ty));
        let closure = TypedNode {
            ty: closure_ty.clone(),
            span: span.clone(),
            node: TypedInner::Closure(
                vec![TypedClosureParam {
                    id: input_id,
                    ty: input_ty,
                }],
                vec![left_id.clone(), right_id.clone()],
                Box::new(body),
            ),
        };
        let bind_left = TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Bind(TypedPattern::Var(source_ty, left_id), Box::new(typed_left)),
        };
        let bind_right = TypedNode {
            ty: Ty::Unit,
            span: span.clone(),
            node: TypedInner::Bind(
                TypedPattern::Var(self.resolve_ty(&typed_right.ty), right_id),
                Box::new(typed_right),
            ),
        };
        Ok(TypedNode {
            ty: closure_ty,
            span: span.clone(),
            node: TypedInner::Block(vec![bind_left, bind_right, closure]),
        })
    }

    fn unsupported_binop_type_error(op: &BinOp, span: &Span) -> TypeError {
        TypeError {
            structured: None,
            message: format!("Unsupported binary operator in trait lowering: {op:?}"),
            span: span.clone(),
            hint: None,
        }
    }

    pub(in crate::checker) fn check_binop(
        &mut self,
        span: &Span,
        op: &BinOp,
        left: &Resolved,
        right: &Resolved,
    ) -> Result<TypedNode, TypeError> {
        self.check_binop_with_expected(span, op, left, right, None)
    }

    pub(super) fn check_binop_with_expected(
        &mut self,
        span: &Span,
        op: &BinOp,
        left: &Resolved,
        right: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        if matches!(op, BinOp::FacetChain) {
            return self.check_arrow_chain(span, left, right, expected);
        }

        let (trait_short, method, token) = match op {
            BinOp::Add => ("Add", "add", "+"),
            BinOp::Sub => ("Sub", "sub", "-"),
            BinOp::Mul => ("Mul", "mul", "*"),
            BinOp::Div(_) => ("Div", "safe_div", "/"),
            BinOp::Mod(_) => ("Mod", "safe_mod", "%"),
            BinOp::Eq => ("Eq", "eq", "=="),
            BinOp::Neq => ("Eq", "neq", "!="),
            BinOp::Lt => ("Compare", "lt", "<"),
            BinOp::Lte => ("Compare", "lte", "<="),
            BinOp::Gt => ("Compare", "gt", ">"),
            BinOp::Gte => ("Compare", "gte", ">="),
            BinOp::Concat => ("Concat", "concat", "++"),
            BinOp::Choice => ("Alternative", "choose", "<|>"),
            BinOp::FacetChain => return Err(Self::unsupported_binop_type_error(op, span)),
        };
        let trait_name = self
            .trait_key_by_short_name(trait_short)
            .ok_or_else(|| TypeError::new(format!("Unknown trait: {trait_short}"), span.clone()))?;
        self.check_trait_invocation_with_result_context(
            span,
            &trait_name,
            method,
            &[
                ResolvedRecordLitArg::Positional(left.clone()),
                ResolvedRecordLitArg::Positional(right.clone()),
            ],
            None,
            // A plain outside expectation cannot determine the Self payload of
            // a Result-producing operator. Preserve compatible Result context
            // for generic payload inference; outer assertions report plain mismatch.
            if matches!(op, BinOp::Div(_) | BinOp::Mod(_)) {
                expected.filter(|ty| matches!(self.resolve_ty(ty), Ty::Result(..)))
            } else {
                expected
            },
            None,
            None,
            None,
            None,
            None,
            None,
            matches!(op, BinOp::Div(_) | BinOp::Mod(_)),
        )
        .and_then(|mut typed| {
            if let TypedInner::TraitCall { origin, args, .. } = &mut typed.node {
                let comparison = match op {
                    BinOp::Lt => Some(ComparisonOperator::Lt),
                    BinOp::Lte => Some(ComparisonOperator::Lte),
                    BinOp::Gt => Some(ComparisonOperator::Gt),
                    BinOp::Gte => Some(ComparisonOperator::Gte),
                    _ => None,
                };
                if let Some(op) = comparison {
                    *origin = TraitCallOrigin::Comparison {
                        op,
                        lhs_ty: args[0].ty.clone(),
                        rhs_ty: args[1].ty.clone(),
                    };
                }
            }
            self.record_safe_operator_signature(op, &typed)?;
            Ok(typed)
        })
        .map_err(|mut error| {
            if let Some(diagnostic) = &mut error.structured {
                if diagnostic.origin == DiagnosticOrigin::TraitCall
                    && (diagnostic.primary.span == *self.resolved_span(left)
                        || diagnostic.primary.span == *self.resolved_span(right))
                {
                    diagnostic.origin = DiagnosticOrigin::Operator {
                        operator: token.into(),
                    };
                    diagnostic.map_source_facts(|fact| {
                        if fact.span == *self.resolved_span(left) {
                            fact.role = SourceRole::LeftValue;
                        } else if fact.span == *self.resolved_span(right) {
                            fact.role = SourceRole::RightValue;
                        }
                    });
                }
            }
            error
        })
    }

    /// Attach provenance only after a concrete implementation was selected.
    /// Declaration syntax preserves an authored error contract; value types
    /// deliberately keep their normal Result<T> display.
    fn record_safe_operator_signature(
        &mut self,
        op: &BinOp,
        typed: &TypedNode,
    ) -> Result<(), TypeError> {
        let (symbol, operator_span) = match op {
            BinOp::Div(span) => ("/", span),
            BinOp::Mod(span) => ("%", span),
            _ => return Ok(()),
        };
        let TypedInner::TraitCall {
            dispatch,
            args,
            method_name,
            obligation,
            ..
        } = &typed.node
        else {
            return Err(self.typecheck_invariant_error("safe operator trait call", operator_span));
        };
        let instantiation = match dispatch {
            TraitDispatch::Selected(instantiation) => instantiation,
            // Bounded generics are checked before a concrete implementation is known.
            TraitDispatch::Pending => return Ok(()),
            TraitDispatch::Static(_) => {
                return Err(self.typecheck_invariant_error(
                    "safe operator selected declaration",
                    operator_span,
                ));
            }
        };
        let mut variables = Vec::new();
        Self::collect_ty_vars(&self.resolve_ty(&typed.ty), &mut variables);
        for argument in args {
            Self::collect_ty_vars(&self.resolve_ty(&argument.ty), &mut variables);
        }
        if !variables.is_empty() {
            // A selected generic implementation can still contain unresolved caller
            // inputs; its signature is not a concrete diagnostic fact yet.
            return Ok(());
        }
        let TraitImplementationId::Declared { declaration, .. } = &instantiation.implementation
        else {
            return Err(self.typecheck_invariant_error(
                "safe operator implementation declaration",
                operator_span,
            ));
        };
        let implementation = self
            .trait_impls
            .values()
            .find(|info| info.declaration_key == *declaration)
            .ok_or_else(|| {
                self.typecheck_invariant_error(
                    "safe operator implementation metadata",
                    operator_span,
                )
            })?;
        let method = implementation.methods.get(method_name).ok_or_else(|| {
            self.typecheck_invariant_error("safe operator method metadata", operator_span)
        })?;
        let mut result = self.diagnostic_ty_name(&self.resolve_ty(&typed.ty));
        if let Some(return_syntax) = method.ret_ty.as_ref() {
            if let AstTy::Generic(_, name, arguments) = return_syntax.syntax() {
                if Self::surface_name(name) == "Result" && arguments.len() == 2 {
                    if !matches!(&arguments[1], AstTy::Named(_, name) if self.env.is_declared_error_type_name(name))
                    {
                        self.resolve_error_marker_type(&arguments[1])?;
                    }
                    let AstTy::Named(_, authored_error) = &arguments[1] else {
                        return Err(self.typecheck_invariant_error(
                            "safe operator authored error marker",
                            operator_span,
                        ));
                    };
                    let Ty::Result(ok, _) = self.resolve_ty(&typed.ty) else {
                        return Err(self.typecheck_invariant_error(
                            "safe operator Result signature",
                            operator_span,
                        ));
                    };
                    result = format!(
                        "Result<{}, {}>",
                        self.diagnostic_ty_name(&ok),
                        Self::surface_name(authored_error)
                    );
                }
            }
        }
        let params = args
            .iter()
            .map(|argument| self.diagnostic_ty_name(&self.resolve_ty(&argument.ty)))
            .collect::<Vec<_>>()
            .join(", ");
        let mut fact = SourceFact::typed(
            SourceRole::OperatorSignature,
            diagnostics::SourceId(0),
            operator_span.clone(),
            format!("({params}) -> {result}"),
        );
        fact.declaration_identity = Some(diagnostics::DeclarationIdentity {
            owner: obligation.trait_id.clone(),
            name: symbol.into(),
        });
        self.safe_operator_results
            .insert((typed.span.start, typed.span.end), fact);
        Ok(())
    }

    /// Grouping and the final expression of a block preserve the produced value.
    pub(super) fn propagate_safe_operator_result(&mut self, from: &Span, to: &Span) {
        if let Some(fact) = self
            .safe_operator_results
            .get(&(from.start, from.end))
            .cloned()
        {
            self.safe_operator_results.insert((to.start, to.end), fact);
        }
    }

    fn check_arrow_chain(
        &mut self,
        span: &Span,
        left: &Resolved,
        right: &Resolved,
        _expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        self.check_facet_compose_intrinsic(
            span,
            &[
                ResolvedRecordLitArg::Positional(left.clone()),
                ResolvedRecordLitArg::Positional(right.clone()),
            ],
        )
    }
}
