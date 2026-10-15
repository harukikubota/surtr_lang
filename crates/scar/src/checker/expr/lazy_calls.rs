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
                    ));
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

/// A source Lazy input before its single closure shell is consumed.
/// Shells describe evaluation semantics; they do not each require a runtime allocation.
pub(in crate::checker) struct LazyInput {
    pub(in crate::checker) node: TypedNode,
    /// A direct capture parameter, including its grouping, already denotes a normalized input.
    placeholder: bool,
    /// A bare call contributes one delayed shell regardless of its return type.
    call_shell: bool,
}

impl Checker {
    pub(super) fn trait_parameter_is_lazy(
        &self,
        trait_name: &str,
        method_name: &str,
        index: usize,
    ) -> bool {
        self.traits
            .get(trait_name)
            .filter(|declaration| declaration.standard_lazy_contract)
            .and_then(|declaration| declaration.methods.get(method_name))
            .and_then(|method| method.value_parameters.get(index))
            .is_some_and(|parameter| Self::lazy_parameter_inner(parameter.ty.syntax()).is_some())
    }

    /// Trait bodies receive the same normalized shell as special forms, but
    /// consume it themselves. Hoist an explicit eager boundary before creating it.
    pub(super) fn check_trait_lazy_argument(
        &mut self,
        expression: &Resolved,
        expected: &Ty,
        span: &Span,
    ) -> Result<TypedNode, TypeError> {
        let Ty::Func(parameters, result) = self.resolve_ty(expected) else {
            return Err(TypeError::new(
                "Invalid normalized Lazy parameter contract",
                span.clone(),
            ));
        };
        if !parameters.is_empty() {
            return Err(TypeError::new(
                "Lazy parameter requires a zero-argument callable",
                span.clone(),
            ));
        }
        if matches!(expression, Resolved::Grouped(..)) && !Self::is_lazy_placeholder(expression) {
            if let Some(reference) = sigil::collect_resolved_closure_captures(expression, &[])
                .into_iter()
                .find(|id| id.compiler_generated && id.name.starts_with("__cap_"))
            {
                return Err(TypeError::new(
                    "capture placeholder is not available in a Lazy eager expression",
                    reference.span,
                ));
            }
        }
        let mut input = self.check_lazy_input(expression, Some(&result), span)?;
        let mut eager_binding = None;
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
        if let TypedInner::EagerBoundary(inner) = &input.node.node {
            let id = ResolvedId {
                name: "__lazy_eager".into(),
                qualified_name: None,
                symbol_info: None,
                unique_id: Self::next_synthetic_range_uid(),
                compiler_generated: true,
                span: span.clone(),
            };
            outer_ids.insert(id.unique_id);
            eager_binding = Some(TypedNode {
                ty: Ty::Unit,
                span: input.node.span.clone(),
                node: TypedInner::Bind(
                    TypedPattern::Var(inner.ty.clone(), id.clone()),
                    inner.clone(),
                ),
            });
            input.node.node = TypedInner::Var(id);
        }
        let body = self.normalize_lazy_single(input, &result, span)?;
        let captures = Self::finalized_closure_captures(&body, &outer_ids);
        let closure = TypedNode {
            ty: Ty::Func(vec![], Box::new(body.ty.clone())),
            span: span.clone(),
            node: TypedInner::Closure(vec![], captures, Box::new(body)),
        };
        Ok(match eager_binding {
            Some(binding) => TypedNode {
                ty: closure.ty.clone(),
                span: span.clone(),
                node: TypedInner::Block(vec![binding, closure]),
            },
            None => closure,
        })
    }

    pub(in crate::checker) fn is_lazy_placeholder(arg: &Resolved) -> bool {
        match arg {
            Resolved::Grouped(_, inner) => Self::is_lazy_placeholder(inner),
            Resolved::Var(_, id) => id.compiler_generated && id.name.starts_with("__cap_"),
            _ => false,
        }
    }

    pub(in crate::checker) fn check_lazy_input(
        &mut self,
        arg: &Resolved,
        expected: Option<&Ty>,
        call_span: &Span,
    ) -> Result<LazyInput, TypeError> {
        if let Resolved::Var(span, id) = arg {
            if id.name.starts_with("__pipe_slot_") {
                return Err(TypeError {
                    structured: None,
                    message: "pipe injection into a Lazy parameter is not allowed".into(),
                    span: span.clone(),
                    hint: Some("Lazy parameters are evaluated under the callee's control. Bind the value first, then pass that binding explicitly, or use a closure to make the evaluation order explicit.".into()),
                });
            }
        }
        let placeholder = Self::is_lazy_placeholder(arg);
        if let Resolved::Grouped(span, inner) = arg {
            let mut input = self.check_lazy_input(inner, expected, call_span)?;
            // Parentheses around a normalized capture parameter only group it.
            if placeholder {
                input.placeholder = true;
                return Ok(input);
            }
            input.call_shell = false;
            input.node = TypedNode {
                ty: input.node.ty.clone(),
                span: span.clone(),
                node: TypedInner::EagerBoundary(Box::new(input.node)),
            };
            return Ok(input);
        }
        let node = if self.branch_tail_is_return_type_argument_call(arg) && expected.is_some() {
            let checkpoint = self.candidate_probe_checkpoint();
            match self.check_node(arg) {
                Ok(node) => node,
                Err(error)
                    if error.reason()
                        == Some(TypeDiagnosticReason::AmbiguousReturnTypeArgument) =>
                {
                    self.rollback_candidate_probe(checkpoint);
                    let expected = expected.expect("checked expected branch result");
                    match arg {
                        Resolved::Closure(span, params, captures, body)
                        | Resolved::ExtractorClosure(span, params, captures, body)
                        | Resolved::CaptureClosure(span, params, captures, body)
                            if params.is_empty() =>
                        {
                            self.check_closure(
                                span,
                                params,
                                captures,
                                body,
                                Some(&Ty::Func(vec![], Box::new(expected.clone()))),
                                None,
                            )?
                        }
                        _ => self.check_node_with_expected(arg, Some(expected))?,
                    }
                }
                Err(error) => {
                    self.rollback_candidate_probe(checkpoint);
                    return Err(error);
                }
            }
        } else {
            self.check_node(arg)?
        };
        Ok(LazyInput {
            node,
            placeholder,
            call_shell: matches!(
                arg,
                Resolved::App(..)
                    | Resolved::LazyApp(..)
                    | Resolved::ConstructorCall(..)
                    | Resolved::EnumConstructorCall(..)
                    | Resolved::If(..)
                    | Resolved::IfLet(..)
                    | Resolved::Cause(..)
                    | Resolved::ApplyPattern(..)
                    | Resolved::IsMatch(..)
                    | Resolved::Dbg(..)
            ),
        })
    }

    fn lazy_depth(&self, ty: &Ty) -> usize {
        match self.resolve_ty(ty) {
            Ty::Func(params, ret)
            | Ty::UserFunc { params, ret, .. }
            | Ty::BuiltinFunc { params, ret, .. }
                if params.is_empty() =>
            {
                1 + self.lazy_depth(&ret)
            }
            _ => 0,
        }
    }

    fn lazy_shell_type(&self, input: &LazyInput) -> Ty {
        let ty = self.resolve_ty(&input.node.ty);
        if input.call_shell {
            Ty::Func(vec![], Box::new(ty))
        } else {
            ty
        }
    }

    /// Consume exactly one normalized shell, preserving any function in its result.
    fn consume_lazy_input(&self, mut input: LazyInput, wrap: bool, span: &Span) -> TypedNode {
        input.node.ty = self.resolve_ty(&input.node.ty);
        if wrap && input.call_shell {
            // Consuming the extra adjustment shell leaves the original shell
            // around a bare call. That remaining shell must be represented as
            // a real thunk rather than evaluating the call now.
            let outer_ids = self
                .env
                .vars
                .iter()
                .filter_map(|(id, ty)| {
                    (!matches!(ty, Ty::UserFunc { .. } | Ty::BuiltinFunc { .. })
                        && !self.env.type_constructor_ids.contains(id))
                    .then_some(*id)
                })
                .collect::<HashSet<_>>();
            let captures = Self::finalized_closure_captures(&input.node, &outer_ids);
            TypedNode {
                ty: Ty::Func(vec![], Box::new(input.node.ty.clone())),
                span: input.node.span.clone(),
                node: TypedInner::Closure(vec![], captures, Box::new(input.node)),
            }
        } else if wrap || input.call_shell {
            input.node
        } else {
            self.maybe_call_zero_arg_function(input.node, span.clone())
        }
    }

    /// Known inputs decide the shell depth; an expected type only determines unknown parameters.
    /// Each shallower input receives at most one shell before the ordinary branch relation.
    pub(in crate::checker) fn normalize_lazy_pair(
        &mut self,
        left: LazyInput,
        right: LazyInput,
        expected: Option<&Ty>,
        span: &Span,
    ) -> Result<(TypedNode, TypedNode), TypeError> {
        let mut left_type = self.lazy_shell_type(&left);
        let mut right_type = self.lazy_shell_type(&right);
        let mut wrap_left = false;
        let mut wrap_right = false;
        match (left.placeholder, right.placeholder) {
            (true, true) => {
                let result = expected.cloned().unwrap_or_else(|| self.env.fresh_tyvar());
                let required = Ty::Func(vec![], Box::new(result));
                self.assert_lazy_placeholder(
                    &required,
                    &left.node,
                    span,
                    &[(&left.node, &required), (&right.node, &required)],
                )?;
                self.assert_lazy_placeholder(
                    &required,
                    &right.node,
                    span,
                    &[(&left.node, &required), (&right.node, &required)],
                )?;
            }
            (true, false) => {
                if self.lazy_depth(&right_type) == 0 {
                    wrap_right = true;
                    right_type = Ty::Func(vec![], Box::new(right_type));
                }
                self.assert_lazy_placeholder(
                    &right_type,
                    &left.node,
                    span,
                    &[(&left.node, &right_type)],
                )?;
            }
            (false, true) => {
                if self.lazy_depth(&left_type) == 0 {
                    wrap_left = true;
                    left_type = Ty::Func(vec![], Box::new(left_type));
                }
                self.assert_lazy_placeholder(
                    &left_type,
                    &right.node,
                    span,
                    &[(&right.node, &left_type)],
                )?;
            }
            (false, false) => {
                let left_depth = self.lazy_depth(&left_type);
                let right_depth = self.lazy_depth(&right_type);
                wrap_left = left_depth == 0 && right_depth == 0 || left_depth < right_depth;
                wrap_right = left_depth == 0 && right_depth == 0 || right_depth < left_depth;
            }
        }
        // The ordinary branch relation below checks the resulting values;
        // at most one shell is added, so deeper mismatches remain errors.
        Ok((
            self.consume_lazy_input(left, wrap_left, span),
            self.consume_lazy_input(right, wrap_right, span),
        ))
    }

    /// Keep all requirements known at this normalization step available to diagnostics,
    /// even when the first placeholder relation fails.
    fn assert_lazy_placeholder(
        &mut self,
        required: &Ty,
        node: &TypedNode,
        span: &Span,
        requirements: &[(&TypedNode, &Ty)],
    ) -> Result<(), TypeError> {
        let result = self.assert_type_relation(
            required,
            &node.ty,
            self.type_fact(SourceRole::Expected, span, required),
            self.type_fact(SourceRole::Value, &node.span, &node.ty),
            TypeDiagnosticReason::ArgumentTypeMismatch,
            DiagnosticOrigin::Call,
            "Lazy",
            0,
        );
        result.map_err(|error| self.lazy_placeholder_error(error, required, node, requirements))
    }

    pub(in crate::checker) fn check_lazy_argument(
        &mut self,
        arg: &Resolved,
        call_span: &Span,
    ) -> Result<TypedNode, TypeError> {
        let input = self.check_lazy_input(arg, None, call_span)?;
        if input.placeholder {
            return Err(TypeError::new(
                "Lazy capture parameter requires a known branch or expected type",
                input.node.span.clone(),
            ));
        }
        let wrap = self.lazy_depth(&self.lazy_shell_type(&input)) == 0;
        Ok(self.consume_lazy_input(input, wrap, call_span))
    }

    pub(in crate::checker) fn normalize_lazy_single(
        &mut self,
        input: LazyInput,
        expected: &Ty,
        span: &Span,
    ) -> Result<TypedNode, TypeError> {
        if input.placeholder {
            let required = Ty::Func(vec![], Box::new(expected.clone()));
            self.assert_lazy_placeholder(
                &required,
                &input.node,
                span,
                &[(&input.node, &required)],
            )?;
        }
        let wrap = !input.placeholder && self.lazy_depth(&self.lazy_shell_type(&input)) == 0;
        Ok(self.consume_lazy_input(input, wrap, span))
    }

    pub(in crate::checker) fn check_lazy_argument_with_expected(
        &mut self,
        arg: &Resolved,
        expected: &Ty,
        call_span: &Span,
    ) -> Result<TypedNode, TypeError> {
        let input = self.check_lazy_input(arg, Some(expected), call_span)?;
        self.normalize_lazy_single(input, expected, call_span)
    }
}
