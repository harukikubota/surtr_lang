use super::*;

impl Checker {
    /// The declaration owns normalization; its ordinary body receives callables.
    /// Keep the input relation shared with the intrinsic consumers still in use.
    pub(super) fn check_lazy_app(
        &mut self,
        span: &Span,
        kind: LazyCaptureKind,
        function: &Resolved,
        args: &[Resolved],
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let mut statements = Vec::new();
        let mut outer_ids = self
            .env
            .vars
            .iter()
            .filter_map(|(id, ty)| {
                (!matches!(ty, Ty::UserFunc { .. } | Ty::BuiltinFunc { .. })
                    && !self.env.type_constructor_ids.contains(id))
                .then_some(*id)
            })
            .collect::<HashSet<_>>();
        let original_outer_ids = outer_ids.clone();
        self.env.push_var_scope();
        let result = (|| {
            let typed_args = match kind {
                LazyCaptureKind::If | LazyCaptureKind::IfThen => {
                    let otherwise =
                        (kind == LazyCaptureKind::If).then(|| Box::new(args[2].clone()));
                    let checked =
                        self.check_if_branches(span, &args[0], &args[1], &otherwise, expected)?;
                    let TypedInner::If(condition, then_branch, else_branch) = checked.node else {
                        unreachable!("checked branch shape")
                    };
                    let then = self.lazy_body_callable(
                        *then_branch,
                        &mut statements,
                        &mut outer_ids,
                        span,
                    );
                    let otherwise = else_branch.map(|body| {
                        self.lazy_body_callable(*body, &mut statements, &mut outer_ids, span)
                    });
                    let condition =
                        self.bind_lazy_call_input(*condition, &mut statements, &mut outer_ids);
                    let mut inputs = vec![condition, then];
                    if let Some(otherwise) = otherwise {
                        inputs.push(otherwise);
                    }
                    inputs
                }
                LazyCaptureKind::Require => {
                    let condition = self.check_node(&args[0])?;
                    if !self.types_compatible(&Ty::Bool, &condition.ty)? {
                        return Err(TypeError::new(
                            format!(
                                "require condition must be Boolean, got {}",
                                self.ty_name(&condition.ty)
                            ),
                            condition.span,
                        ));
                    }
                    let error =
                        self.check_lazy_argument_with_expected(&args[1], &Ty::Error, span)?;
                    self.ensure_guard_error_value(&error, "require")?;
                    let error =
                        self.lazy_body_callable(error, &mut statements, &mut outer_ids, span);
                    let condition =
                        self.bind_lazy_call_input(condition, &mut statements, &mut outer_ids);
                    vec![condition, error]
                }
                LazyCaptureKind::Ensure => {
                    let value = self.check_node(&args[0])?;
                    if matches!(&args[1], Resolved::App(..) | Resolved::LazyApp(..)) {
                        return Err(TypeError {
                        structured: None,
                        message: "ensure requires a closure or capture predicate".into(),
                        span: self.resolved_span(&args[1]).clone(),
                        hint: Some("Use `&predicate` or `{|value| predicate(value) }`; call expressions such as `predicate()` are not accepted here.".into()),
                    });
                    }
                    let expected_pred =
                        Ty::Func(vec![self.resolve_ty(&value.ty)], Box::new(Ty::Bool));
                    let mut predicate = if Self::is_lazy_placeholder(&args[1]) {
                        self.check_node(&args[1])?
                    } else {
                        self.check_compose_callable(&args[1], "ensure")?
                    };
                    self.assert_type_relation(
                        &expected_pred,
                        &predicate.ty,
                        self.type_fact(SourceRole::Expected, span, &expected_pred),
                        self.type_fact(SourceRole::Value, &predicate.span, &predicate.ty),
                        TypeDiagnosticReason::ArgumentTypeMismatch,
                        DiagnosticOrigin::Call,
                        "ensure",
                        1,
                    )?;
                    predicate.ty = self.resolve_ty(&predicate.ty);
                    self.unary_function_parts(&predicate.ty, "ensure", &predicate.span)?;
                    let error =
                        self.check_lazy_argument_with_expected(&args[2], &Ty::Error, span)?;
                    self.ensure_guard_error_value(&error, "ensure")?;
                    let value = self.bind_lazy_call_input(value, &mut statements, &mut outer_ids);
                    let error =
                        self.lazy_body_callable(error, &mut statements, &mut outer_ids, span);
                    let predicate =
                        self.bind_lazy_call_input(predicate, &mut statements, &mut outer_ids);
                    vec![value, predicate, error]
                }
                LazyCaptureKind::MapErr | LazyCaptureKind::OrElse => {
                    let name = if kind == LazyCaptureKind::MapErr {
                        "map_err"
                    } else {
                        "or_else"
                    };
                    let value = self.check_result_value(&args[0], name, None)?;
                    let required = if kind == LazyCaptureKind::MapErr {
                        Ty::Error
                    } else {
                        self.resolve_ty(&value.ty)
                    };
                    let body = self.check_lazy_argument_with_expected(&args[1], &required, span)?;
                    if kind == LazyCaptureKind::MapErr {
                        self.ensure_result_error_arg(&body, name)?;
                    } else {
                        self.assert_type_relation(
                            &required,
                            &body.ty,
                            self.type_fact(SourceRole::Expected, &value.span, &required),
                            self.type_fact(SourceRole::Value, &body.span, &body.ty),
                            TypeDiagnosticReason::ArgumentTypeMismatch,
                            DiagnosticOrigin::Call,
                            "Result::or_else",
                            1,
                        )?;
                    }
                    let value = self.bind_lazy_call_input(value, &mut statements, &mut outer_ids);
                    let next = self.lazy_body_callable(body, &mut statements, &mut outer_ids, span);
                    vec![value, next]
                }
                _ => {
                    return Err(TypeError::new(
                        "Invalid ordinary Lazy call contract",
                        span.clone(),
                    ))
                }
            };
            let call = self.finish_normalized_lazy_call(span, function, typed_args, expected)?;
            self.record_lazy_capture_signature(span, &call.ty);
            if statements.is_empty() {
                Ok(call)
            } else {
                let ty = call.ty.clone();
                statements.push(call);
                Ok(TypedNode {
                    ty,
                    span: span.clone(),
                    node: TypedInner::Block(statements),
                })
            }
        })();
        self.env.pop_var_scope();
        for id in outer_ids.difference(&original_outer_ids) {
            self.constructor_capabilities.remove(id);
        }
        result
    }

    fn bind_lazy_call_input(
        &mut self,
        input: TypedNode,
        statements: &mut Vec<TypedNode>,
        outer_ids: &mut HashSet<u32>,
    ) -> TypedNode {
        let id = ResolvedId {
            name: "__lazy_input".into(),
            qualified_name: None,
            symbol_info: None,
            unique_id: Self::next_synthetic_range_uid(),
            compiler_generated: true,
            span: input.span.clone(),
        };
        outer_ids.insert(id.unique_id);
        let result = TypedNode {
            ty: input.ty.clone(),
            span: input.span.clone(),
            node: TypedInner::Var(id.clone()),
        };
        let pattern = TypedPattern::Var(input.ty.clone(), id);
        let provenance = self.constructor_capability_for_node(&input);
        self.bind_typed_pattern(&pattern, &self.resolve_ty(&input.ty));
        self.bind_constructor_provenance(&pattern, (provenance, input.ty.clone()));
        statements.push(TypedNode {
            ty: Ty::Unit,
            span: input.span.clone(),
            node: TypedInner::Bind(pattern, Box::new(input)),
        });
        result
    }

    fn lazy_body_callable(
        &mut self,
        mut body: TypedNode,
        statements: &mut Vec<TypedNode>,
        outer_ids: &mut HashSet<u32>,
        span: &Span,
    ) -> TypedNode {
        // Only the root grouping is an eager boundary of this call. Nested Lazy
        // calls materialize their own inputs when their containing body runs.
        match &mut body.node {
            TypedInner::EagerBoundary(inner) => {
                body = self.bind_lazy_call_input(*inner.clone(), statements, outer_ids)
            }
            TypedInner::App(target, args) if args.is_empty() => {
                if let TypedInner::EagerBoundary(inner) = &target.node {
                    *target =
                        Box::new(self.bind_lazy_call_input(*inner.clone(), statements, outer_ids));
                }
            }
            _ => {}
        }
        let captures = Self::finalized_closure_captures(&body, outer_ids);
        TypedNode {
            ty: Ty::Func(vec![], Box::new(body.ty.clone())),
            span: span.clone(),
            node: TypedInner::Closure(vec![], captures, Box::new(body)),
        }
    }

    fn finish_normalized_lazy_call(
        &mut self,
        span: &Span,
        function: &Resolved,
        args: Vec<TypedNode>,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let explicit = Self::explicit_type_args(function);
        let target = match function {
            Resolved::ReturnTypeArgumentApply(_, target, _) => target.as_ref(),
            target => target,
        };
        let (mut typed_function, signature, return_type_arguments) =
            self.instantiate_named_callable_signature(span, target, explicit.as_deref())?;
        if signature.value_parameters.len() != args.len() {
            return Err(TypeError::new(
                "Invalid ordinary Lazy call arity",
                span.clone(),
            ));
        }
        let mut constraints = self.call_constraint_set(
            signature,
            return_type_arguments,
            expected,
            span,
            &typed_function.ty,
        );
        self.apply_return_type_argument_constraints(&constraints)?;
        self.apply_concrete_expected_call_constraint(&constraints, span, explicit.as_deref())?;
        for (ordinal, (parameter, argument)) in constraints
            .signature
            .value_parameters
            .iter()
            .zip(&args)
            .enumerate()
        {
            self.check_call_argument_relation(
                span,
                "Lazy",
                &parameter.ty,
                argument,
                None,
                ordinal,
            )?;
        }
        self.ensure_no_runtime_facet_args(&args, span, "Function call")?;
        let parameters = constraints
            .signature
            .value_parameters
            .iter()
            .map(|parameter| parameter.ty.clone())
            .collect::<Vec<_>>();
        self.check_callback_argument_provenance(&parameters, &args)?;
        let substitution = match self.complete_call_constraint_set(
            &mut constraints,
            &args,
            span,
            explicit.as_deref(),
        ) {
            super::super::signatures::SolveState::Solved(substitution) => substitution,
            super::super::signatures::SolveState::Deferred(pending) => pending.substitution,
            super::super::signatures::SolveState::Failed(error) => return Err(error),
        };
        if let Ty::UserFunc {
            call_substitution, ..
        } = &mut typed_function.ty
        {
            *call_substitution = substitution;
        } else {
            return Err(TypeError::new(
                "Ordinary Lazy declaration requires a user function body",
                span.clone(),
            ));
        }
        typed_function.ty =
            self.callable_ty_from_signature(&typed_function.ty, &constraints.signature);
        self.check_callable_input_provenance(&typed_function, &args)?;
        Ok(TypedNode {
            ty: self.resolve_ty(&constraints.signature.return_type.ty),
            span: span.clone(),
            node: TypedInner::App(Box::new(typed_function), args),
        })
    }
}
