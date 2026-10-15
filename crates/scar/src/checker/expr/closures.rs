use super::*;
use diagnostics::{
    DiagnosticData, DiagnosticOrigin, SourceFact, SourceId, SourceRole, StructuredDiagnostic,
    TypeDiagnosticReason,
};
use sindr::names::ConstructorCapturePolicy;

impl Checker {
    #[inline(never)]
    pub(super) fn check_literal_closure_with_expected_relation(
        &mut self,
        span: &Span,
        params: &[ResolvedClosureParam],
        captures: &[ResolvedId],
        body: &Resolved,
        expected_ty: &Ty,
        expected_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        let expected_ty = self.resolve_ty(expected_ty);
        if matches!(expected_ty, Ty::Var(var) if !self.rigid_tyvars.contains(&var)) {
            let typed = self.check_closure(span, params, captures, body, None, None)?;
            self.assert_type_relation(
                &expected_ty,
                &typed.ty,
                self.type_fact(SourceRole::Expected, span, &expected_ty),
                self.type_fact(SourceRole::Value, &typed.span, &typed.ty),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Call,
                "closure",
                0,
            )?;
            Ok(typed)
        } else {
            self.check_closure(
                span,
                params,
                captures,
                body,
                Some(&expected_ty),
                expected_relation,
            )
        }
    }

    #[inline(never)]
    pub(super) fn check_capture_closure_with_expected_type(
        &mut self,
        span: &Span,
        params: &[ResolvedClosureParam],
        captures: &[ResolvedId],
        body: &Resolved,
        expected_ty: &Ty,
    ) -> Result<TypedNode, TypeError> {
        let expected_ty = self.resolve_ty(expected_ty);
        if matches!(expected_ty, Ty::Var(var) if !self.rigid_tyvars.contains(&var)) {
            let typed = self.check_capture_closure(span, params, captures, body, None)?;
            self.assert_type_relation(
                &expected_ty,
                &typed.ty,
                self.type_fact(SourceRole::Expected, span, &expected_ty),
                self.type_fact(SourceRole::Value, &typed.span, &typed.ty),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Call,
                "closure",
                0,
            )?;
            Ok(typed)
        } else {
            self.check_capture_closure(span, params, captures, body, Some(&expected_ty))
        }
    }

    pub(super) fn inferred_capture_body(
        span: &Span,
        param_id: &ResolvedId,
        segments: &[ResolvedFacetPathSegment],
    ) -> Resolved {
        segments.iter().fold(
            Resolved::Var(span.clone(), param_id.clone()),
            |expr, segment| match segment {
                ResolvedFacetPathSegment::Field {
                    name,
                    optional: false,
                } => Resolved::FieldAccess(span.clone(), Box::new(expr), name.clone()),
                other => Resolved::FacetSegmentAccess(span.clone(), Box::new(expr), other.clone()),
            },
        )
    }

    pub(super) fn check_extractor_closure(
        &mut self,
        span: &Span,
        params: &[ResolvedClosureParam],
        captures: &[ResolvedId],
        body: &Resolved,
        expected: Option<&Ty>,
        expected_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        if params.is_empty() {
            return Err(TypeError::new(
                "ExtractorClosure requires at least one input",
                span.clone(),
            ));
        }
        let signature = match expected.map(|ty| self.resolve_ty(ty)) {
            Some(Ty::ExtractorClosure(signature)) => *signature,
            Some(Ty::Var(_)) | None => Ty::Func(
                params
                    .iter()
                    .map(|param| match &param.ty {
                        Some(ty) => {
                            self.resolve_ast_ty_in_context(ty, self.local_type_syntax_context())
                        }
                        None => Ok(self.env.fresh_tyvar()),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                Box::new(Ty::MatchResult(Box::new(self.env.fresh_tyvar()))),
            ),
            Some(other) => {
                return Err(TypeError::new(
                    format!("Expected {}, got ExtractorClosure", self.ty_name(&other)),
                    span.clone(),
                ));
            }
        };
        let Ty::Func(_, ret) = &signature else {
            return Err(TypeError::new(
                "ExtractorClosure requires a function signature",
                span.clone(),
            ));
        };
        if !matches!(ret.as_ref(), Ty::MatchResult(_)) {
            return Err(TypeError::new(
                "ExtractorClosure must return MatchResult",
                span.clone(),
            ));
        }
        self.check_closure_with_kind(
            span,
            params,
            captures,
            body,
            Some(&signature),
            expected_relation,
            CallableContext::ExtractorClosure,
        )
    }

    pub(super) fn check_closure(
        &mut self,
        span: &Span,
        params: &[ResolvedClosureParam],
        captures: &[ResolvedId],
        body: &Resolved,
        expected: Option<&Ty>,
        expected_relation: Option<&ExpectedTypeRelation>,
    ) -> Result<TypedNode, TypeError> {
        let saved_capture_kind = self.is_capture_closure;
        self.is_capture_closure = false;
        let result = self.check_closure_with_kind(
            span,
            params,
            captures,
            body,
            expected,
            expected_relation,
            CallableContext::Closure,
        );
        self.is_capture_closure = saved_capture_kind;
        result
    }

    fn check_closure_with_kind(
        &mut self,
        span: &Span,
        params: &[ResolvedClosureParam],
        captures: &[ResolvedId],
        body: &Resolved,
        expected: Option<&Ty>,
        expected_relation: Option<&ExpectedTypeRelation>,
        kind: CallableContext,
    ) -> Result<TypedNode, TypeError> {
        let saved_function_return_ty = self.function_return_ty.clone();
        let saved_function_return_origin = self.function_return_origin.take();
        let saved_current_function_symbol = self.current_function_symbol.clone();
        let saved_current_impl_struct_target = self.current_impl_struct_target.clone();
        let saved_current_private_field_owner = self.current_private_field_owner.clone();
        let saved_callable_context = self.callable_context;
        self.callable_context = kind;
        self.function_return_ty = Some(match expected {
            Some(Ty::Func(_, expected_ret)) => expected_ret.as_ref().clone(),
            _ => self.env.fresh_tyvar(),
        });
        let saved_closure_depth = self.closure_depth;
        let saved_facet_bindings = self.facet_bindings.clone();

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
        self.env.push_var_scope();
        self.closure_depth = self.closure_depth.saturating_add(1);
        let result = (|| -> Result<TypedNode, TypeError> {
            let mut typed_params = Vec::new();
            let param_tys = match expected {
                Some(Ty::Func(expected_params, _)) => {
                    if expected_params.len() != params.len() {
                        return Err(self.closure_arity_error(
                            expected_params.len(),
                            params.len(),
                            span,
                        ));
                    }
                    expected_params.clone()
                }
                Some(other) => {
                    return Err(TypeError::from_structured(StructuredDiagnostic {
                        reason: TypeDiagnosticReason::CallableShapeMismatch.into(),
                        origin: DiagnosticOrigin::Call,
                        data: DiagnosticData::CallableShape(diagnostics::CallableShapeData {
                            callable: "closure".into(),
                            actual_type: None,
                            expected_arity: None,
                            actual_arity: Some(params.len() as u32),
                        }),
                        primary: SourceFact::untyped(
                            SourceRole::CallTarget,
                            SourceId(0),
                            span.clone(),
                        ),
                        related: vec![self.type_fact(SourceRole::Expected, span, other)],
                        remediation: None,
                    }));
                }
                None => params
                    .iter()
                    .map(|param| match &param.ty {
                        Some(ast_ty) => {
                            self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())
                        }
                        None => Ok(self.env.fresh_tyvar()),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            };

            for (param, param_ty) in params.iter().zip(param_tys.iter()) {
                let param_ty = if let Some(ast_ty) = &param.ty {
                    let annotated =
                        self.resolve_ast_ty_in_context(ast_ty, self.local_type_syntax_context())?;
                    self.assert_type_relation(
                        param_ty,
                        &annotated,
                        self.type_fact(SourceRole::Expected, span, param_ty),
                        self.type_fact(
                            SourceRole::Annotation,
                            Self::ast_ty_span(ast_ty),
                            &annotated,
                        ),
                        TypeDiagnosticReason::AnnotationTypeMismatch,
                        DiagnosticOrigin::Annotation,
                        &param.id.name,
                        typed_params.len() as u32,
                    )?;
                    self.resolve_ty(&annotated)
                } else {
                    self.resolve_ty(param_ty)
                };
                if !param.id.compiler_generated
                    && (param.ty.is_some()
                        || expected_relation.is_some_and(|relation| {
                            matches!(
                                relation.origin,
                                DiagnosticOrigin::Annotation | DiagnosticOrigin::Return
                            )
                        }))
                {
                    self.explicit_closure_parameters
                        .insert(param.id.unique_id, param_ty.clone());
                }
                self.env.bind_var(param.id.unique_id, param_ty.clone());
                typed_params.push(TypedClosureParam {
                    id: param.id.clone(),
                    ty: param_ty,
                });
            }

            for capture in captures {
                if self
                    .pattern_binding_aliases
                    .contains_key(&capture.unique_id)
                {
                    continue;
                }
                if let Some(ty) = self.env.lookup_var(capture.unique_id).cloned() {
                    let resolved_ty = self.resolve_ty(&ty);
                    self.env.bind_var(capture.unique_id, resolved_ty);
                }
            }

            let profile = self.profiler.start();
            let body_is_result_constructor = match body {
                Resolved::ConstructorCall(_, _, _) | Resolved::EnumConstructorCall(_, _, _, _) => {
                    true
                }
                Resolved::App(_, func, _) => {
                    matches!(func.as_ref(), Resolved::Var(_, id) if self.lookup_enum_variant_by_constructor_id(id.unique_id).is_some())
                }
                _ => false,
            };
            let typed_body = if kind == CallableContext::ExtractorClosure
                || body_is_result_constructor
                || matches!(
                    body,
                    Resolved::If(..)
                        | Resolved::IfLet(..)
                        | Resolved::LazyApp(_, LazyCaptureKind::If | LazyCaptureKind::IfThen, _, _)
                )
                || self.body_tail_is_return_type_argument_call(body)
            {
                if let Some(Ty::Func(_, expected_ret)) = expected {
                    self.check_node_with_expected_relation(
                        body,
                        Some(expected_ret.as_ref()),
                        expected_relation,
                    )?
                } else {
                    self.check_node(body)?
                }
            } else {
                self.check_node(body)?
            };
            self.profiler.finish(ProfileEvent::ClosureBody, profile);
            let typed_body = *self
                .concretize_pending_trait_calls(typed_body)
                .map_err(|error| *error)?;
            if expected.is_none()
                && self.current_function_symbol.is_none()
                && !self.local_callable_obligations_depend_on(&typed_body, &param_tys)
            {
                if let Some((trait_name, method_name, subject, pending_span)) =
                    self.first_pending_trait_helper(&typed_body)
                {
                    return Err(self.pending_trait_helper_error(
                        trait_name,
                        method_name,
                        subject,
                        pending_span,
                    ));
                }
            }
            if matches!(typed_body.ty, Ty::Facet(..)) {
                return Err(self.policy_error(
                    TypeDiagnosticReason::FacetCompileTimeOnly,
                    diagnostics::TypePolicy::FacetStageRestriction,
                    Some(
                        "Facet is compile-time only in Stage1 and cannot be returned from closures"
                            .into(),
                    ),
                    None,
                    Some(&typed_body.ty),
                    Some("Stage1".into()),
                    None,
                    &typed_body.span,
                    Some("Use Facet::view(...) inside the closure instead.".into()),
                ));
            }
            // The closure result type is needed immediately for inference, but
            // the body tree itself is normalized by the enclosing typed node.
            let body_ty = self.resolve_ty(&typed_body.ty);
            if let Some(Ty::Func(_, expected_ret)) = expected {
                let expected_ret = self.resolve_ty(expected_ret);
                if kind == CallableContext::ExtractorClosure || matches!(expected_ret, Ty::Unit) {
                    self.assert_type_relation(
                        &expected_ret,
                        &body_ty,
                        self.type_fact(SourceRole::Expected, span, &expected_ret),
                        self.type_fact(SourceRole::Value, &typed_body.span, &body_ty),
                        TypeDiagnosticReason::ReturnTypeMismatch,
                        DiagnosticOrigin::Return,
                        "closure",
                        0,
                    )?;
                }
            }

            let param_tys = typed_params
                .iter()
                .map(|p| self.resolve_ty(&p.ty))
                .collect::<Vec<_>>();
            Ok(TypedNode {
                ty: if kind == CallableContext::ExtractorClosure {
                    Ty::ExtractorClosure(Box::new(Ty::Func(param_tys, Box::new(body_ty))))
                } else {
                    Ty::Func(param_tys, Box::new(body_ty))
                },
                span: span.clone(),
                node: (if kind == CallableContext::ExtractorClosure {
                    TypedInner::ExtractorClosure
                } else {
                    TypedInner::Closure
                })(
                    typed_params
                        .into_iter()
                        .map(|param| TypedClosureParam {
                            id: param.id,
                            ty: self.resolve_ty(&param.ty),
                        })
                        .collect(),
                    Self::finalized_closure_captures(&typed_body, &outer_ids),
                    Box::new(typed_body),
                ),
            })
        })();

        self.env.pop_var_scope();
        self.function_return_ty = saved_function_return_ty;
        self.function_return_origin = saved_function_return_origin;
        self.current_function_symbol = saved_current_function_symbol;
        self.current_impl_struct_target = saved_current_impl_struct_target;
        self.current_private_field_owner = saved_current_private_field_owner;
        self.callable_context = saved_callable_context;
        self.closure_depth = saved_closure_depth;
        self.facet_bindings = saved_facet_bindings;
        result
    }

    pub(super) fn check_capture_closure(
        &mut self,
        span: &Span,
        params: &[ResolvedClosureParam],
        captures: &[ResolvedId],
        body: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let saved_capture = self.active_lazy_capture.take();
        self.active_lazy_capture =
            ActiveLazyCapture::new(params, expected, self.resolved_span(body).clone());
        let saved_capture_kind = self.is_capture_closure;
        self.is_capture_closure = true;
        let result = self
            .check_closure_with_kind(
                span,
                params,
                captures,
                body,
                expected,
                None,
                CallableContext::Closure,
            )
            .map_err(|error| self.lazy_capture_annotation_error(error));
        self.is_capture_closure = saved_capture_kind;
        self.active_lazy_capture = saved_capture;
        let mut typed = result?;
        let TypedInner::Closure(params, captures, typed_body) = typed.node else {
            return Err(TypeError {
                structured: None,
                message: "Capture closure did not produce a closure node".into(),
                span: span.clone(),
                hint: None,
            });
        };
        let constructor_id = match body {
            Resolved::ConstructorCall(_, id, _) | Resolved::EnumConstructorCall(_, id, _, _)
                if id
                    .symbol_info
                    .as_ref()
                    .and_then(|info| info.capabilities.constructor_capture)
                    == Some(ConstructorCapturePolicy::Ordinary) =>
            {
                Some(id.clone())
            }
            _ => None,
        };
        typed.node = match constructor_id {
            Some(id) => TypedInner::CaptureConstructorClosure(id, params, captures, typed_body),
            None => TypedInner::CaptureClosure(params, captures, typed_body),
        };
        Ok(typed)
    }

    fn check_constructor_capture(
        &mut self,
        span: &Span,
        target: &Resolved,
        expected: Option<&Ty>,
    ) -> Result<Option<TypedNode>, TypeError> {
        let policy = match target {
            Resolved::Var(_, id) | Resolved::EnumConstructorCall(_, id, _, _) => id
                .symbol_info
                .as_ref()
                .and_then(|info| info.capabilities.constructor_capture),
            _ => None,
        };
        let Some(policy) = policy else {
            return Ok(None);
        };
        match policy {
            ConstructorCapturePolicy::CompilerManaged => return Ok(None),
            ConstructorCapturePolicy::Forbidden => {
                let name = match target {
                    Resolved::Var(_, id) | Resolved::EnumConstructorCall(_, id, _, _) => {
                        id.name.clone()
                    }
                    _ => unreachable!("constructor policy was read from a constructor target"),
                };
                return Err(TypeError {
                    structured: None,
                    message: format!("constructor capture is forbidden for `{name}`"),
                    span: span.clone(),
                    hint: None,
                });
            }
            ConstructorCapturePolicy::Ordinary => {}
        }

        let (body, arity) = match target {
            Resolved::EnumConstructorCall(_, id, type_args, _) => {
                let variant = self
                    .lookup_enum_variant_by_constructor_id(id.unique_id)
                    .ok_or_else(|| TypeError {
                        structured: None,
                        message: format!("Unknown enum constructor: {}", id.name),
                        span: span.clone(),
                        hint: None,
                    })?;
                (
                    Resolved::EnumConstructorCall(
                        span.clone(),
                        id.clone(),
                        type_args.clone(),
                        Vec::new(),
                    ),
                    variant.payload.len(),
                )
            }
            Resolved::Var(_, id) => {
                if let Some(variant) = self.lookup_enum_variant_by_constructor_id(id.unique_id) {
                    (
                        Resolved::ConstructorCall(span.clone(), id.clone(), Vec::new()),
                        variant.payload.len(),
                    )
                } else {
                    let def = self
                        .env
                        .lookup_type_def(&id.name)
                        .ok_or_else(|| TypeError {
                            structured: None,
                            message: format!("Unknown constructor type: {}", id.name),
                            span: span.clone(),
                            hint: None,
                        })?;
                    let arity = match def.kind {
                        crate::env::TypeKind::Record => def.fields.len(),
                        crate::env::TypeKind::Struct => {
                            let owner_name =
                                id.qualified_name.as_deref().unwrap_or(id.name.as_str());
                            let new_name = format!("{}::new", owner_name);
                            let new_uid = self.impl_method_uids.get(&new_name).copied().ok_or_else(|| {
                                TypeError {
                                    structured: None,
                                    message: format!(
                                        "Struct `{}` constructor capture requires `{}` but no such method was found",
                                        id.name, new_name
                                    ),
                                    span: span.clone(),
                                    hint: None,
                                }
                            })?;
                            self.callable_signatures
                                .get(&new_uid)
                                .map(|signature| signature.value_arity())
                                .ok_or_else(|| TypeError {
                                    structured: None,
                                    message: format!(
                                        "Struct `{}` constructor capture has no callable signature for `{}`",
                                        id.name, new_name
                                    ),
                                    span: span.clone(),
                                    hint: None,
                                })?
                        }
                        crate::env::TypeKind::Enum => {
                            return Err(TypeError {
                                structured: None,
                                message: format!(
                                    "Enum type `{}` is not a constructor capture target; capture a variant such as `&{}::Variant`",
                                    id.name, id.name
                                ),
                                span: span.clone(),
                                hint: None,
                            });
                        }
                        crate::env::TypeKind::ConcreteError => self
                            .env
                            .error_constructor_inputs
                            .get(&id.unique_id)
                            .ok_or_else(|| {
                                TypeError::new(
                                    "Error constructor input signature is missing",
                                    span.clone(),
                                )
                            })?
                            .len(),
                    };
                    (
                        Resolved::ConstructorCall(span.clone(), id.clone(), Vec::new()),
                        arity,
                    )
                }
            }
            _ => return Ok(None),
        };

        let mut params = Vec::with_capacity(arity);
        let mut body_args = Vec::with_capacity(arity);
        for index in 0..arity {
            let id = ResolvedId {
                name: format!("__constructor_capture_arg_{index}"),
                qualified_name: None,
                symbol_info: None,
                unique_id: Self::next_synthetic_range_uid(),
                compiler_generated: true,
                span: span.clone(),
            };
            body_args.push(ResolvedRecordLitArg::Positional(Resolved::Var(
                span.clone(),
                id.clone(),
            )));
            params.push(ResolvedClosureParam {
                lazy_capture: None,
                id,
                ty: None,
            });
        }

        let body = match body {
            Resolved::ConstructorCall(body_span, id, _) => {
                Resolved::ConstructorCall(body_span, id, body_args)
            }
            Resolved::EnumConstructorCall(body_span, id, type_args, _) => {
                Resolved::EnumConstructorCall(body_span, id, type_args, body_args)
            }
            _ => unreachable!("constructor capture body must be a constructor call"),
        };
        Ok(Some(self.check_capture_closure(
            span,
            &params,
            &[],
            &body,
            expected,
        )?))
    }

    pub(in crate::checker) fn check_capture(
        &mut self,
        span: &Span,
        target: &Resolved,
        args: &[Resolved],
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        if !args.is_empty() {
            return Err(TypeError {
                structured: None,
                message: "capture calls with arguments must be lowered before type checking".into(),
                span: span.clone(),
                hint: None,
            });
        }

        if self.is_function_curry_callee(target) {
            return Err(TypeError::new(
                "Function::curry cannot be captured as a function value; call it with a callable argument",
                span.clone(),
            ));
        }

        if let Some(mut typed) = self.check_constructor_capture(span, target, expected)? {
            let constructor_id = match target {
                Resolved::Var(_, id) | Resolved::EnumConstructorCall(_, id, _, _) => id.clone(),
                _ => unreachable!("constructor capture target has no constructor identity"),
            };
            match typed.node {
                TypedInner::CaptureConstructorClosure(..) => return Ok(typed),
                TypedInner::CaptureClosure(params, captures, body) => {
                    typed.node = TypedInner::CaptureConstructorClosure(
                        constructor_id,
                        params,
                        captures,
                        body,
                    );
                    return Ok(typed);
                }
                _ => {
                    return Err(TypeError::new(
                        "constructor capture did not produce a capture closure",
                        span.clone(),
                    ));
                }
            }
        }

        if let Some((_, trait_name, method_name)) = self.trait_method_ref(target) {
            let declaration = &self.traits[&trait_name];
            if declaration.standard_lazy_contract
                && declaration.methods[&method_name]
                    .value_parameters
                    .iter()
                    .any(|parameter| Self::lazy_parameter_inner(parameter.ty.syntax()).is_some())
            {
                return Err(TypeError::new(
                    format!(
                        "{trait_name}::{method_name} has Lazy arguments and cannot be captured without explicit arguments"
                    ),
                    span.clone(),
                ));
            }
            let Some(expected_ty) = expected else {
                if let Resolved::Var(_, id) = target {
                    return Err(self.policy_error(
                        TypeDiagnosticReason::TraitHelperCaptureNeedsExpectedType,
                        diagnostics::TypePolicy::TraitHelperCaptureInference,
                        Some(id.name.clone()),
                        None,
                        None,
                        None,
                        None,
                        span,
                        Some(
                            "Add a callable annotation such as `cmp: (Int, Int -> Ordering) = &compare`, or use the capture inside an expression like `&compare `Function::on` _.field`."
                                .into(),
                        ),
                    ));
                }
                return Err(self.policy_error(
                    TypeDiagnosticReason::TraitHelperCaptureNeedsExpectedType,
                    diagnostics::TypePolicy::TraitHelperCaptureInference,
                    None,
                    None,
                    None,
                    None,
                    None,
                    span,
                    Some(
                        "Add a callable annotation or use the capture where the receiver type can be inferred."
                            .into(),
                    ),
                ));
            };
            let Ty::Func(params, _) = self.resolve_ty(expected_ty) else {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Expected function type, got {}",
                        self.diagnostic_ty_name(&self.resolve_ty(expected_ty))
                    ),
                    span: span.clone(),
                    hint: None,
                });
            };
            let mut closure_params = Vec::with_capacity(params.len());
            let mut body_args = Vec::with_capacity(params.len());
            for index in 0..params.len() {
                let param_id = ResolvedId {
                    name: format!("__trait_helper_arg_{}", index),
                    qualified_name: None,
                    symbol_info: None,
                    unique_id: Self::next_synthetic_range_uid(),
                    compiler_generated: true,
                    span: span.clone(),
                };
                body_args.push(ResolvedRecordLitArg::Positional(Resolved::Var(
                    span.clone(),
                    param_id.clone(),
                )));
                closure_params.push(ResolvedClosureParam {
                    lazy_capture: None,
                    id: param_id,
                    ty: None,
                });
            }
            let synthetic = Resolved::CaptureClosure(
                span.clone(),
                closure_params,
                Vec::new(),
                Box::new(Resolved::App(
                    span.clone(),
                    Box::new(target.clone()),
                    body_args,
                )),
            );
            return self.check_node_with_expected(&synthetic, Some(expected_ty));
        }

        if Self::capture_target_is_facet_path(target) {
            if self.has_concrete_error_facet_root(target) {
                return Err(TypeError::new(
                    "Concrete Error Facet paths cannot be captured",
                    span.clone(),
                ));
            }
            if let Some(expected_ty) = expected {
                let expected_ty_resolved = self.resolve_ty(expected_ty);
                if let Ty::Func(params, _) = &expected_ty_resolved {
                    if params.len() == 1 {
                        let view_id = self.runtime_helper_id("Facet::view", span)?;
                        let param_uid = Self::next_synthetic_range_uid();
                        let param_id = ResolvedId {
                            name: "__facet_capture_arg".to_string(),
                            qualified_name: None,
                            symbol_info: None,
                            unique_id: param_uid,
                            compiler_generated: true,
                            span: span.clone(),
                        };
                        let synthetic = Resolved::CaptureClosure(
                            span.clone(),
                            vec![ResolvedClosureParam {
                                lazy_capture: None,
                                id: param_id.clone(),
                                ty: None,
                            }],
                            Vec::new(),
                            Box::new(Resolved::App(
                                span.clone(),
                                Box::new(Resolved::Var(span.clone(), view_id)),
                                vec![
                                    ResolvedRecordLitArg::Positional(target.clone()),
                                    ResolvedRecordLitArg::Positional(Resolved::Var(
                                        span.clone(),
                                        param_id,
                                    )),
                                ],
                            )),
                        );
                        return self.check_node_with_expected(&synthetic, Some(expected_ty));
                    }
                }
            }
        }

        let explicit_return_type_arguments = Self::explicit_type_args(target);
        let named_target = match target {
            Resolved::ReturnTypeArgumentApply(_, inner, _) => inner.as_ref(),
            other => other,
        };
        let has_callable_signature = match named_target {
            Resolved::Var(_, id) => self.callable_signatures.contains_key(&id.unique_id),
            _ => false,
        };
        let (typed_target, capture_signature) = if has_callable_signature {
            let (mut typed, signature, return_type_arguments) = self
                .instantiate_named_callable_signature(
                    span,
                    named_target,
                    explicit_return_type_arguments.as_deref(),
                )?;
            let expected_return = explicit_return_type_arguments
                .as_ref()
                .and_then(|_| expected)
                .and_then(|expected| match self.resolve_ty(expected) {
                    Ty::Func(_, ret) | Ty::BuiltinFunc { ret, .. } | Ty::UserFunc { ret, .. } => {
                        Some(*ret)
                    }
                    _ => None,
                });
            let constraints = self.call_constraint_set(
                signature,
                return_type_arguments,
                expected_return.as_ref(),
                span,
                &typed.ty,
            );
            self.apply_return_type_argument_constraints(&constraints)?;
            self.apply_expected_call_constraint(
                &constraints,
                span,
                explicit_return_type_arguments.as_deref(),
            )?;
            typed.ty = self.callable_ty_from_signature(&typed.ty, &constraints.signature);
            (typed, Some(constraints.signature))
        } else {
            (self.check_node(target)?, None)
        };
        self.ensure_no_match_result_value(&typed_target.ty, span)?;
        let mut target_ty = self.resolve_ty(&typed_target.ty);
        if let Some(expected_ty) = expected {
            let checkpoint = self.candidate_probe_checkpoint();
            let callable_shape = match &target_ty {
                Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                    Some(Ty::Func(params.clone(), ret.clone()))
                }
                Ty::Func(..) => Some(target_ty.clone()),
                _ => None,
            };
            let compatible = callable_shape
                .as_ref()
                .map(|shape| self.types_compatible(shape, expected_ty))
                .transpose();
            if matches!(compatible, Ok(Some(true))) {
                target_ty = self.resolve_ty(&target_ty);
            } else {
                // Operator checking supplies a provisional callable shape so
                // it can report the mismatch with both operands. A rejected
                // capture probe must not replace that operator diagnostic or
                // leak partial type bindings.
                self.rollback_candidate_probe(checkpoint);
            }
            compatible?;
        }
        if let Ty::Facet(_, source_ty, focus_ty, ..) = &target_ty {
            let view_id = self.runtime_helper_id("Facet::view", span)?;
            let param_uid = Self::next_synthetic_range_uid();
            let param_name = "__facet_capture_arg".to_string();
            let param_id = ResolvedId {
                name: param_name.clone(),
                qualified_name: None,
                symbol_info: None,
                unique_id: param_uid,
                compiler_generated: true,
                span: span.clone(),
            };
            let captures = match target {
                Resolved::Var(_, id) => vec![id.clone()],
                _ => Vec::new(),
            };
            let body = Resolved::App(
                span.clone(),
                Box::new(Resolved::Var(span.clone(), view_id)),
                vec![
                    ResolvedRecordLitArg::Positional(target.clone()),
                    ResolvedRecordLitArg::Positional(Resolved::Var(span.clone(), param_id.clone())),
                ],
            );
            let ret_ty = match &typed_target.node {
                TypedInner::FacetPath(path) if path.may_fail => Ty::Result(
                    Box::new(self.resolve_ty(focus_ty.as_ref())),
                    Box::new(Ty::Error),
                ),
                _ => self.resolve_ty(focus_ty.as_ref()),
            };
            let synthetic = Resolved::CaptureClosure(
                span.clone(),
                vec![ResolvedClosureParam {
                    lazy_capture: None,
                    id: param_id,
                    ty: None,
                }],
                captures,
                Box::new(body),
            );
            let expected = Ty::Func(vec![self.resolve_ty(source_ty.as_ref())], Box::new(ret_ty));
            return self.check_node_with_expected(&synthetic, Some(&expected));
        }
        let (params, ret) = match &target_ty {
            Ty::BuiltinFunc { params, ret, .. } => (params.clone(), ret.as_ref().clone()),
            Ty::UserFunc { params, ret, .. } => (params.clone(), ret.as_ref().clone()),
            Ty::Func(params, ret) => (params.clone(), ret.as_ref().clone()),
            other => {
                return Err(self.callable_shape_error(other, "capture", &typed_target.span, None));
            }
        };
        if let Some(signature) = &capture_signature {
            if let Some(slot) = signature.return_type_arguments.iter().find(|slot| {
                let mut variables = Vec::new();
                match &slot.ty {
                    Ty::SelfApp(items) => {
                        let Some((witness, _)) = Self::constructor_application_parts(items) else {
                            Self::collect_ty_vars(&self.resolve_ty(&slot.ty), &mut variables);
                            return variables.iter().any(|var| !self.rigid_tyvars.contains(var));
                        };
                        let resolved_witness = self.resolve_ty(witness);
                        let carrier = match witness {
                            Ty::Var(var) => {
                                self.constructor_witness_traits.get(var).map(|trait_key| {
                                    self.canonical_constructor_carrier(trait_key, &resolved_witness)
                                })
                            }
                            _ => None,
                        };
                        match carrier {
                            Some(ConstructorCarrierOutcome::Projected(carrier)) => {
                                for captured in carrier.captured_arguments {
                                    self.canonical_waiting_variables(&captured.ty, &mut variables);
                                }
                            }
                            Some(ConstructorCarrierOutcome::Deferred { waiting_on }) => {
                                variables.extend(waiting_on);
                            }
                            Some(ConstructorCarrierOutcome::Rejected { failures }) => {
                                debug_assert!(!failures.is_empty());
                                Self::collect_ty_vars(&resolved_witness, &mut variables);
                            }
                            None => {
                                Self::collect_ty_vars(&resolved_witness, &mut variables);
                            }
                        }
                    }
                    _ => Self::collect_ty_vars(&self.resolve_ty(&slot.ty), &mut variables),
                }
                // The enclosing signature witnesses its own generic inputs;
                // only fresh carrier or captured variables remain ambiguous.
                // Mapped payload variables are ordinary invocation inputs and
                // do not make an otherwise concrete constructor head pending.
                variables.iter().any(|var| !self.rigid_tyvars.contains(var))
            }) {
                let unresolved_ty = self.diagnostic_ty_name(&self.resolve_ty(&slot.ty));
                return Err(TypeError {
                    message: format!(
                        "return type arguments for `{}` cannot be inferred",
                        signature.identity.name
                    ),
                    span: span.clone(),
                    hint: Some(
                        "Provide concrete return type arguments or an expected callable type."
                            .into(),
                    ),
                    structured: Some(StructuredDiagnostic {
                        reason: TypeDiagnosticReason::AmbiguousReturnTypeArgument.into(),
                        origin: DiagnosticOrigin::ReturnTypeArgument {
                            ordinal: slot.ordinal,
                        },
                        data: DiagnosticData::ReturnTypeArgument(
                            diagnostics::ReturnTypeArgumentData {
                                declared_origin: None,
                                value_parameter_origin: None,
                                return_origin: None,
                                left_origin: None,
                                right_origin: Some(SourceFact::typed(
                                    SourceRole::ReturnTypeArgument,
                                    SourceId(0),
                                    span.clone(),
                                    unresolved_ty.clone(),
                                )),
                                required_trait: None,
                                expected_count: None,
                                actual_count: None,
                                callable: signature.identity.name.clone(),
                                ordinal: Some(slot.ordinal),
                                expected_type: Some("concrete return type argument".into()),
                                actual_type: Some(unresolved_ty.clone()),
                            },
                        ),
                        primary: SourceFact::typed(
                            SourceRole::ReturnTypeArgument,
                            SourceId(0),
                            span.clone(),
                            unresolved_ty,
                        ),
                        related: Vec::new(),
                        remediation: None,
                    }),
                });
            }
        }
        Ok(TypedNode {
            ty: Ty::Func(
                params.into_iter().map(|ty| self.resolve_ty(&ty)).collect(),
                Box::new(self.resolve_ty(&ret)),
            ),
            span: span.clone(),
            node: TypedInner::Capture(Box::new(typed_target), Vec::new()),
        })
    }

    pub(in crate::checker) fn maybe_call_zero_arg_function(
        &self,
        node: TypedNode,
        _call_span: Span,
    ) -> TypedNode {
        match &node.ty {
            Ty::BuiltinFunc { params, ret, .. }
            | Ty::UserFunc { params, ret, .. }
            | Ty::Func(params, ret)
                if params.is_empty() =>
            {
                TypedNode {
                    ty: ret.as_ref().clone(),
                    span: node.span.clone(),
                    node: TypedInner::App(Box::new(node), Vec::new()),
                }
            }
            _ => node,
        }
    }

    pub(super) fn capture_target_is_facet_path(target: &Resolved) -> bool {
        matches!(
            target,
            Resolved::FieldAccess(_, _, _) | Resolved::FacetSegmentAccess(_, _, _)
        )
    }
}
