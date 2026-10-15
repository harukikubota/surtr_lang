use super::*;
use diagnostics::{DiagnosticOrigin, SourceRole, TypeDiagnosticReason};

impl Checker {
    pub(in crate::checker) fn check_struct_lit(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        field_vals: &[ResolvedStructLitField],
    ) -> Result<TypedNode, TypeError> {
        let def = self
            .env
            .lookup_type_def(&id.name)
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown struct type: {}", id.name),
                span: span.clone(),
                hint: None,
            })?
            .clone();

        if !id.compiler_generated {
            let owner_name = Self::surface_name(&def.name);
            if self.current_impl_struct_target.as_deref() != Some(owner_name) {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Struct literal `{}` is only allowed inside `impl {} {{ ... }}` method bodies",
                        id.name, id.name
                    ),
                    span: span.clone(),
                    hint: Some(format!(
                        "Construct `{}` values via `{}(...)` / `{}::new(...)` outside the impl body.",
                        id.name, id.name, id.name
                    )),
                });
            }
        }

        let tag = def.tag;

        let mut seen = HashSet::new();
        for field in field_vals {
            let name = match field {
                ResolvedStructLitField::Explicit(name, _)
                | ResolvedStructLitField::Shorthand(name, _) => name,
            };
            if !def.fields.iter().any(|(field_name, _)| field_name == name) {
                return Err(TypeError {
                    structured: None,
                    message: format!("Unknown field '{}' in {}", name, id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
            if !seen.insert(name.clone()) {
                return Err(TypeError {
                    structured: None,
                    message: format!("Duplicate field '{}' in {}", name, id.name),
                    span: span.clone(),
                    hint: None,
                });
            }
        }

        let mut fresh = HashMap::new();
        for variable in &def.type_param_vars {
            self.instantiate_ty_with_fresh(&Ty::Var(*variable), &mut fresh);
        }
        let fields = def
            .fields
            .iter()
            .map(|(name, ty)| (name.clone(), self.instantiate_ty_with_fresh(ty, &mut fresh)))
            .collect::<Vec<_>>();
        let mut typed_fields = Vec::new();
        for (def_name, def_ty) in &fields {
            let resolved_val = field_vals
                .iter()
                .find_map(|field| match field {
                    ResolvedStructLitField::Explicit(name, resolved_val)
                    | ResolvedStructLitField::Shorthand(name, resolved_val)
                        if name == def_name =>
                    {
                        Some(resolved_val)
                    }
                    _ => None,
                })
                .ok_or_else(|| TypeError {
                    structured: None,
                    message: format!("Missing field '{}' in {}", def_name, id.name),
                    span: span.clone(),
                    hint: None,
                })?;
            let typed_val = self.check_node(resolved_val)?;
            if self.ty_contains_facet(&typed_val.ty) {
                return Err(TypeError {
                    structured: None,
                    message:
                        "Struct literal fields cannot contain Facet values in Stage1 (Facet is compile-time only)"
                            .into(),
                    span: typed_val.span.clone(),
                    hint: Some("Apply Facet::view/set/over before constructing runtime values.".into()),
                });
            }
            if !self.types_compatible(def_ty, &typed_val.ty)? {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Field '{}': expected {}, got {}",
                        def_name,
                        self.ty_name(def_ty),
                        self.ty_name(&typed_val.ty)
                    ),
                    span: typed_val.span.clone(),
                    hint: None,
                });
            }
            typed_fields.push(typed_val);
        }

        let arguments: Vec<Ty> = def
            .type_param_vars
            .iter()
            .map(|variable| {
                self.resolve_ty(
                    fresh
                        .get(variable)
                        .expect("every nominal type parameter is instantiated"),
                )
            })
            .collect();
        let result_fields = self.instantiate_type_def_fields(&def, &arguments);
        let result_ty = Ty::Struct(id.name.clone(), NominalType::new(arguments, result_fields));
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::StructLit(tag, typed_fields),
        })
    }

    pub(in crate::checker) fn check_constructor_call(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        args: &[ResolvedRecordLitArg],
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        // Closure bodies are checked through `check_node`, but an expected
        // closure return type is kept separately while that body is checked.
        // Use it for Result constructors so `Err(error)` can inhabit a nested
        // Result without constructing `Err(Err(error))`.
        // An absent call-site expectation means the constructor is
        // polymorphic, not that it should inherit the enclosing function's
        // return type.  In particular, `Err(...)` inside a list/tuple or a
        // nested call must not become `Result<Unit>` merely because the
        // surrounding function returns `Result<Unit>`.  The fresh Result
        // success slot below is unified later by the surrounding expression
        // or by the function body's final return check.
        let expected = expected.cloned();
        if let Some(variant) = self.lookup_enum_variant_by_constructor_id(id.unique_id) {
            let variant = self.instantiate_enum_variant(&variant);
            let enum_surface_name = Self::surface_name(&variant.enum_name);
            if let Some(sindr::names::SpecialEnumVariantLowering::Boolean(value)) =
                variant.special_variant
            {
                if !args.is_empty() {
                    return Err(TypeError::from_structured(
                        self.argument_contract_diagnostic(
                            TypeDiagnosticReason::ArityMismatch,
                            &id.name,
                            None,
                            0,
                            args.len(),
                            span,
                            DiagnosticOrigin::Call,
                        ),
                    ));
                }
                return Ok(TypedNode {
                    ty: Ty::Bool,
                    span: span.clone(),
                    node: TypedInner::Lit(Lit::Bool(value)),
                });
            }
            if enum_surface_name == "MatchResult" {
                if !matches!(
                    self.callable_context,
                    CallableContext::Extractor | CallableContext::ExtractorClosure
                ) {
                    return Err(TypeError::new(
                        "MatchResult constructors are only allowed in an Extractor body",
                        span.clone(),
                    ));
                }
                if args.len() != 1 {
                    return Err(TypeError::new(
                        "MatchResult constructors require exactly one value",
                        span.clone(),
                    ));
                }
                let ResolvedRecordLitArg::Positional(expr) = &args[0] else {
                    return Err(TypeError::new(
                        "MatchResult constructors require a positional value",
                        span.clone(),
                    ));
                };
                let expected_payload = expected.as_ref().and_then(|ty| match self.resolve_ty(ty) {
                    Ty::MatchResult(payload) => Some(*payload),
                    _ => None,
                });
                let inner = self.check_node_with_expected(
                    expr,
                    if variant.short_name == "Ok" {
                        expected_payload.as_ref()
                    } else {
                        None
                    },
                )?;
                let inner = self.maybe_call_zero_arg_function(inner, span.clone());
                self.ensure_no_runtime_facet_args(
                    std::slice::from_ref(&inner),
                    span,
                    "MatchResult constructor",
                )?;
                let payload = match variant.short_name.as_str() {
                    "Ok" => inner.ty.clone(),
                    "Err" => {
                        if !self.types_compatible(&inner.ty, &Ty::Error)? {
                            let mut error = TypeError::new(
                                "MatchResult::Err requires an Error value",
                                inner.span.clone(),
                            );
                            if let Some(hint) = self.error_kind_error_hint(&inner) {
                                error = error.with_hint(hint);
                            }
                            return Err(error);
                        }
                        expected_payload.unwrap_or_else(|| self.env.fresh_tyvar())
                    }
                    _ => {
                        return Err(TypeError::new(
                            "Invalid canonical MatchResult variant",
                            span.clone(),
                        ))
                    }
                };
                return Ok(TypedNode {
                    ty: Ty::MatchResult(Box::new(payload)),
                    span: span.clone(),
                    node: TypedInner::ConstructorCall(
                        variant.tag,
                        vec![
                            TypedNode {
                                ty: Ty::Int,
                                span: span.clone(),
                                node: TypedInner::Lit(Lit::Int(variant.discriminant)),
                            },
                            inner,
                        ],
                    ),
                });
            }
            if matches!(
                variant.special_variant,
                Some(
                    sindr::names::SpecialEnumVariantLowering::ResultOk
                        | sindr::names::SpecialEnumVariantLowering::ResultErr
                )
            ) {
                if args.len() != 1 {
                    return Err(TypeError::from_structured(
                        self.argument_contract_diagnostic(
                            TypeDiagnosticReason::ArityMismatch,
                            &id.name,
                            None,
                            1,
                            args.len(),
                            span,
                            DiagnosticOrigin::Call,
                        ),
                    ));
                }
                let inner = match &args[0] {
                    ResolvedRecordLitArg::Positional(expr) => {
                        // Only the success payload receives the owner's success
                        // type. Err stores a concrete Error at the current layer.
                        let inner_expected = (variant.special_variant
                            == Some(sindr::names::SpecialEnumVariantLowering::ResultOk))
                        .then(|| {
                            expected
                                .as_ref()
                                .and_then(|expected| match self.resolve_ty(expected) {
                                    Ty::Result(ok, _) => Some(ok.as_ref().clone()),
                                    _ => None,
                                })
                        })
                        .flatten();
                        let typed = self.check_node_with_expected(expr, inner_expected.as_ref())?;
                        self.ensure_no_match_result_value(&typed.ty, &typed.span)?;
                        if self.ty_contains_facet(&typed.ty) {
                            return Err(TypeError {
                                structured: None,
                                message:
                                    "Result constructors cannot contain Facet values in Stage1 (Facet is compile-time only)"
                                        .into(),
                                span: typed.span.clone(),
                                hint: Some(
                                    "Apply Facet::view/set/over before wrapping with Ok(...) or Err(...)."
                                        .into(),
                                ),
                            });
                        }
                        self.maybe_call_zero_arg_function(typed, span.clone())
                    }
                    ResolvedRecordLitArg::Named(_, _) => {
                        return Err(TypeError::from_structured(
                            self.argument_contract_diagnostic(
                                TypeDiagnosticReason::ArgumentModeMismatch,
                                &id.name,
                                None,
                                1,
                                args.len(),
                                span,
                                DiagnosticOrigin::Call,
                            ),
                        ));
                    }
                };
                if variant.special_variant
                    == Some(sindr::names::SpecialEnumVariantLowering::ResultErr)
                {
                    if matches!(self.resolve_ty(&inner.ty), Ty::Result(_, _)) {
                        return Err(TypeError {
                            structured: None,
                            message: "Nested Result errors are not allowed: use Err(error) for the outer failure, or Ok(Err(error)) for an inner failure.".into(),
                            span: inner.span.clone(),
                            hint: Some(
                                "Err(...) is lifted to the expected Result nesting; do not write Err(Err(...)).".into(),
                            ),
                        });
                    }
                    if !matches!(inner.ty, Ty::Error) {
                        return Err(TypeError {
                            structured: None,
                            message: "Err(...) requires an Error value.".into(),
                            span: inner.span.clone(),
                            hint: self.error_kind_error_hint(&inner).or_else(|| Some(
                                "Use an existing Error or an explicit deferror constructor call in Err(...).".into(),
                            )),
                        });
                    }
                    if self.is_abstract_error_marker_value(&inner) {
                        return Err(TypeError {
                            structured: None,
                            message: "Error is abstract and cannot be constructed directly.".into(),
                            span: inner.span.clone(),
                            hint: Some("Use a concrete deferror value in Err(...).".into()),
                        });
                    }
                }
                let result_ty = if variant.special_variant
                    == Some(sindr::names::SpecialEnumVariantLowering::ResultOk)
                {
                    Ty::Result(Box::new(inner.ty.clone()), Box::new(Ty::Error))
                } else {
                    expected
                        .as_ref()
                        .filter(|ty| matches!(self.resolve_ty(ty), Ty::Result(_, _)))
                        .map(|ty| self.resolve_ty(ty))
                        .unwrap_or_else(|| {
                            let ok_var = self.env.fresh_tyvar();
                            Ty::Result(Box::new(ok_var), Box::new(Ty::Error))
                        })
                };
                return Ok(TypedNode {
                    ty: result_ty,
                    span: span.clone(),
                    node: TypedInner::ConstructorCall(variant.tag, vec![inner]),
                });
            }
            if matches!(
                Self::surface_name(&variant.enum_name),
                "StopReply" | "StopReason"
            ) && !self.stop_constructor_allowed()
            {
                return Err(self.stop_constructor_error(span, &variant.enum_name));
            }
            let payload_values = self.typecheck_positional_call_args(
                span,
                &id.name,
                &variant.payload,
                args,
                None,
                "Enum constructors do not accept named arguments".into(),
            )?;
            self.ensure_no_runtime_facet_args(&payload_values, span, "Enum constructor arguments")?;

            let mut fields = Vec::with_capacity(payload_values.len() + 1);
            fields.push(TypedNode {
                ty: Ty::Int,
                span: span.clone(),
                node: TypedInner::Lit(Lit::Int(variant.discriminant)),
            });
            fields.extend(payload_values);

            return Ok(TypedNode {
                ty: self.resolve_ty(&variant.enum_ty),
                span: span.clone(),
                node: TypedInner::ConstructorCall(variant.tag, fields),
            });
        }

        if self.error_definition_uid == Some(id.unique_id) {
            return Err(TypeError::new("Error internal construction is only allowed as the direct terminal expression of its declaration", span.clone()));
        }
        if self.env.is_error_constructor(id.unique_id) {
            let schema = self
                .env
                .error_constructor_inputs
                .get(&id.unique_id)
                .cloned()
                .ok_or_else(|| {
                    TypeError::new("Error constructor input signature is missing", span.clone())
                })?;
            let typed_args = self.check_error_arguments(span, &id.name, &schema, args)?;
            let ty = self.env.lookup_var(id.unique_id).cloned().ok_or_else(|| {
                TypeError::new("Error constructor function is missing", span.clone())
            })?;
            return Ok(TypedNode {
                ty: Ty::Error,
                span: span.clone(),
                node: TypedInner::App(
                    Box::new(TypedNode {
                        ty,
                        span: id.span.clone(),
                        node: TypedInner::Var(id.clone()),
                    }),
                    typed_args,
                ),
            });
        }
        if let Some(ty) = self.env.lookup_var(id.unique_id).cloned() {
            match &ty {
                Ty::BuiltinFunc { params, ret, .. } => {
                    let callable_hint =
                        Some(self.call_target_signature_hint_for_id(id, params, ret.as_ref()));
                    let typed_args = self.typecheck_positional_call_args(
                        span,
                        &id.name,
                        params,
                        args,
                        callable_hint,
                        "Constructor calls do not accept named arguments".into(),
                    )?;
                    self.ensure_no_runtime_facet_args(&typed_args, span, "Constructor arguments")?;

                    return Ok(TypedNode {
                        ty: ret.as_ref().clone(),
                        span: span.clone(),
                        node: TypedInner::App(
                            Box::new(TypedNode {
                                ty: ty.clone(),
                                span: id.span.clone(),
                                node: TypedInner::Var(id.clone()),
                            }),
                            typed_args,
                        ),
                    });
                }
                Ty::UserFunc { params, ret, .. }
                    if self.callable_signatures.contains_key(&id.unique_id) =>
                {
                    return self.check_registered_callable_application(span, id, args, None);
                }
                Ty::UserFunc { params, ret, .. } => {
                    let callable_hint = self.call_target_signature_hint_for_id(id, params, ret);
                    let typed_args = self.typecheck_user_function_args(
                        span,
                        id.unique_id,
                        "function",
                        params,
                        args,
                        Some(callable_hint.as_str()),
                        false,
                    )?;
                    return Ok(TypedNode {
                        ty: ret.as_ref().clone(),
                        span: span.clone(),
                        node: TypedInner::App(
                            Box::new(TypedNode {
                                ty: ty.clone(),
                                span: id.span.clone(),
                                node: TypedInner::Var(id.clone()),
                            }),
                            typed_args,
                        ),
                    });
                }
                Ty::Func(params, ret) => {
                    let callable_hint =
                        self.callable_signature_hint(&Ty::Func(params.clone(), ret.clone()));
                    let typed_args = self.typecheck_positional_call_args(
                        span,
                        &id.name,
                        params,
                        args,
                        callable_hint,
                        "Constructor calls do not accept named arguments".into(),
                    )?;
                    self.ensure_no_runtime_facet_args(&typed_args, span, "Constructor arguments")?;

                    return Ok(TypedNode {
                        ty: ret.as_ref().clone(),
                        span: span.clone(),
                        node: TypedInner::App(
                            Box::new(TypedNode {
                                ty: ty.clone(),
                                span: id.span.clone(),
                                node: TypedInner::Var(id.clone()),
                            }),
                            typed_args,
                        ),
                    });
                }
                _ => {}
            }
        }

        let def = self
            .env
            .lookup_type_def(&id.name)
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown constructor type: {}", id.name),
                span: span.clone(),
                hint: None,
            })?
            .clone();

        if matches!(def.kind, crate::env::TypeKind::Struct) {
            let owner_name = id.qualified_name.as_deref().unwrap_or(id.name.as_str());
            let new_name = format!("{}::new", owner_name);
            let Some(new_uid) = self.impl_method_uids.get(&new_name).copied() else {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Struct `{}` constructor call requires `{}` but no such method was found",
                        id.name, new_name
                    ),
                    span: span.clone(),
                    hint: Some(format!(
                        "Define `impl {} {{ def new(...) -> Self {{ ... }} }}` or `impl {} {{ def new(...) -> Result<Self, Error> {{ ... }} }}`.",
                        id.name, id.name
                    )),
                });
            };
            let new_id = ResolvedId {
                name: new_name.clone(),
                qualified_name: None,
                symbol_info: None,
                unique_id: new_uid,
                compiler_generated: false,
                span: id.span.clone(),
            };
            let typed_call =
                self.check_registered_callable_application(span, &new_id, args, None)?;
            let ret_ty = typed_call.ty.clone();
            let expected_self_ty = Ty::Struct(
                id.name.clone(),
                NominalType::new(
                    def.type_param_vars.iter().copied().map(Ty::Var).collect(),
                    def.fields.clone(),
                ),
            );
            let returns_self = self.types_compatible(&expected_self_ty, &ret_ty)?;
            let returns_result_self = match self.resolve_ty(&ret_ty) {
                Ty::Result(ok, _) => self.types_compatible(&expected_self_ty, ok.as_ref())?,
                _ => false,
            };
            if !(returns_self || returns_result_self) {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "`{}` must return Self ({}) or Result<Self, E>, got {}",
                        new_name,
                        self.ty_name(&expected_self_ty),
                        self.ty_name(&ret_ty)
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }

            return Ok(typed_call);
        }

        if !matches!(def.kind, crate::env::TypeKind::Record) {
            return Err(TypeError {
                structured: None,
                message: format!("{} is not a constructor-call type", id.name),
                span: span.clone(),
                hint: None,
            });
        }

        let tag = def.tag;
        let mut typed_fields = vec![None; def.fields.len()];

        let named = args
            .iter()
            .any(|a| matches!(a, ResolvedRecordLitArg::Named(_, _)));

        if !named {
            if args.len() != def.fields.len() {
                return Err(TypeError::from_structured(
                    self.argument_contract_diagnostic(
                        TypeDiagnosticReason::ArityMismatch,
                        &id.name,
                        None,
                        def.fields.len(),
                        args.len(),
                        span,
                        DiagnosticOrigin::Call,
                    ),
                ));
            }
            for (i, arg) in args.iter().enumerate() {
                if let ResolvedRecordLitArg::Positional(expr) = arg {
                    let typed_val = self.check_node(expr)?;
                    if self.ty_contains_facet(&typed_val.ty) {
                        return Err(TypeError {
                            structured: None,
                            message:
                                "Record constructors cannot contain Facet values in Stage1 (Facet is compile-time only)"
                                    .into(),
                            span: typed_val.span.clone(),
                            hint: Some("Apply Facet::view/set/over before constructing runtime values.".into()),
                        });
                    }
                    let (_, def_ty) = &def.fields[i];
                    self.assert_type_relation(
                        def_ty,
                        &typed_val.ty,
                        self.type_fact(SourceRole::Expected, &id.span, def_ty),
                        self.type_fact(SourceRole::Value, &typed_val.span, &typed_val.ty),
                        TypeDiagnosticReason::ArgumentTypeMismatch,
                        DiagnosticOrigin::Call,
                        &id.name,
                        i as u32,
                    )?;
                    typed_fields[i] = Some(typed_val);
                }
            }
        } else {
            let mut seen = HashSet::new();
            for arg in args {
                if let ResolvedRecordLitArg::Positional(expr) = arg {
                    let mut diagnostic = self.argument_contract_diagnostic(
                        TypeDiagnosticReason::ArgumentModeMismatch,
                        &id.name,
                        None,
                        def.fields.len(),
                        args.len(),
                        self.resolved_span(expr),
                        DiagnosticOrigin::Call,
                    );
                    diagnostic.remediation = Some(diagnostics::Remediation::Help {
                        text: "Named Record arguments require an explicit field name or a bare variable shorthand".into(),
                    });
                    return Err(TypeError::from_structured(diagnostic));
                }
                if let ResolvedRecordLitArg::Named(name, expr) = arg {
                    if !seen.insert(name.clone()) {
                        return Err(TypeError::from_structured(
                            self.argument_contract_diagnostic(
                                TypeDiagnosticReason::DuplicateArgument,
                                &id.name,
                                Some(name),
                                def.fields.len(),
                                args.len(),
                                span,
                                DiagnosticOrigin::Call,
                            ),
                        ));
                    }
                    let idx = def
                        .fields
                        .iter()
                        .position(|(n, _)| n == name)
                        .ok_or_else(|| {
                            TypeError::from_structured(self.argument_contract_diagnostic(
                                TypeDiagnosticReason::UnknownNamedArgument,
                                &id.name,
                                Some(name),
                                def.fields.len(),
                                args.len(),
                                span,
                                DiagnosticOrigin::Call,
                            ))
                        })?;
                    let typed_val = self.check_node(expr)?;
                    if self.ty_contains_facet(&typed_val.ty) {
                        return Err(TypeError {
                            structured: None,
                            message:
                                "Record constructors cannot contain Facet values in Stage1 (Facet is compile-time only)"
                                    .into(),
                            span: typed_val.span.clone(),
                            hint: Some("Apply Facet::view/set/over before constructing runtime values.".into()),
                        });
                    }
                    let (_, def_ty) = &def.fields[idx];
                    self.assert_type_relation(
                        def_ty,
                        &typed_val.ty,
                        self.type_fact(SourceRole::Expected, &id.span, def_ty),
                        self.type_fact(SourceRole::Value, &typed_val.span, &typed_val.ty),
                        TypeDiagnosticReason::ArgumentTypeMismatch,
                        DiagnosticOrigin::Call,
                        &id.name,
                        idx as u32,
                    )?;
                    typed_fields[idx] = Some(typed_val);
                }
            }
        }

        let final_fields: Vec<TypedNode> = typed_fields
            .into_iter()
            .enumerate()
            .map(|(i, f)| {
                f.ok_or_else(|| {
                    TypeError::from_structured(self.argument_contract_diagnostic(
                        TypeDiagnosticReason::MissingArgument,
                        &id.name,
                        Some(&def.fields[i].0),
                        def.fields.len(),
                        args.len(),
                        span,
                        DiagnosticOrigin::Call,
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let result_ty = match def.kind {
            crate::env::TypeKind::Record => Ty::Record(
                id.name.clone(),
                NominalType::monomorphic(def.fields.clone()),
            ),
            crate::env::TypeKind::Struct
            | crate::env::TypeKind::Enum
            | crate::env::TypeKind::ConcreteError => {
                unreachable!("validated above")
            }
        };
        Ok(TypedNode {
            ty: result_ty,
            span: span.clone(),
            node: TypedInner::ConstructorCall(tag, final_fields),
        })
    }

    pub(in crate::checker) fn check_explicit_enum_constructor_call(
        &mut self,
        span: &Span,
        id: &ResolvedId,
        type_args: &[AstTy],
        args: &[ResolvedRecordLitArg],
        expected: Option<&Ty>,
    ) -> Result<TypedNode, TypeError> {
        let variant = self
            .lookup_enum_variant_by_constructor_id(id.unique_id)
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown enum constructor: {}", id.name),
                span: span.clone(),
                hint: None,
            })?;
        let def = self
            .env
            .lookup_type_def(&variant.enum_name)
            .cloned()
            .ok_or_else(|| TypeError {
                structured: None,
                message: format!("Unknown enum type: {}", variant.enum_name),
                span: span.clone(),
                hint: None,
            })?;
        if !matches!(def.kind, crate::env::TypeKind::Enum) {
            return Err(TypeError {
                structured: None,
                message: format!("{} is not an enum type", variant.enum_name),
                span: span.clone(),
                hint: None,
            });
        }
        if type_args.len() != def.type_params.len() {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Enum constructor {} expects {} type argument(s), got {}",
                    variant.enum_name,
                    def.type_params.len(),
                    type_args.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }

        let supplied = type_args
            .iter()
            .map(|ast_ty| {
                if matches!(ast_ty, AstTy::Named(_, name) if Self::surface_name(name) == "_") {
                    Ok(self.env.fresh_tyvar())
                } else {
                    let ty = self.resolve_ast_ty_in_context(ast_ty, TypeSyntaxContext::General)?;
                    Ok(ty)
                }
            })
            .collect::<Result<Vec<_>, TypeError>>()?;
        let declared_arguments = match &variant.enum_ty {
            Ty::Enum(_, arguments) => arguments.clone(),
            Ty::Result(ok, _) => vec![ok.as_ref().clone()],
            Ty::Bool => Vec::new(),
            Ty::MatchResult(payload) => vec![payload.as_ref().clone()],
            other => {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Enum constructor metadata for {} has invalid result type {}",
                        variant.enum_name,
                        self.ty_name(other)
                    ),
                    span: span.clone(),
                    hint: None,
                });
            }
        };
        if declared_arguments.len() != supplied.len() {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "Enum constructor metadata for {} has {} type slot(s), expected {}",
                    variant.enum_name,
                    declared_arguments.len(),
                    supplied.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }
        let mut bindings = HashMap::new();
        for (declared, supplied) in declared_arguments.into_iter().zip(supplied) {
            let Ty::Var(variable) = declared else {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "Enum constructor metadata for {} contains a non-variable type slot",
                        variant.enum_name
                    ),
                    span: span.clone(),
                    hint: None,
                });
            };
            bindings.insert(variable, supplied);
        }
        let enum_ty = self.substitute_type_def_ty(&variant.enum_ty, &bindings);
        let payload = variant
            .payload
            .iter()
            .map(|ty| self.substitute_type_def_ty(ty, &bindings))
            .collect::<Vec<_>>();

        if let Some(expected) = expected {
            self.assert_type_relation(
                expected,
                &enum_ty,
                self.type_fact(SourceRole::Expected, span, expected),
                self.type_fact(SourceRole::Value, span, &enum_ty),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Call,
                "enum constructor",
                0,
            )?;
        }

        if variant.special_variant.is_some() || matches!(variant.enum_ty, Ty::MatchResult(_)) {
            let typed = self.check_constructor_call(span, id, args, Some(&enum_ty))?;
            self.assert_type_relation(
                &enum_ty,
                &typed.ty,
                self.type_fact(SourceRole::Expected, span, &enum_ty),
                self.type_fact(SourceRole::Value, &typed.span, &typed.ty),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Call,
                &id.name,
                0,
            )?;
            return Ok(typed);
        }

        let payload_values = self.typecheck_positional_call_args(
            span,
            &id.name,
            &payload,
            args,
            None,
            "Enum constructors do not accept named arguments".into(),
        )?;
        self.ensure_no_runtime_facet_args(&payload_values, span, "Enum constructor arguments")?;

        let mut fields = Vec::with_capacity(payload_values.len() + 1);
        fields.push(TypedNode {
            ty: Ty::Int,
            span: span.clone(),
            node: TypedInner::Lit(Lit::Int(variant.discriminant)),
        });
        fields.extend(payload_values);
        Ok(TypedNode {
            ty: self.resolve_ty(&enum_ty),
            span: span.clone(),
            node: TypedInner::ConstructorCall(variant.tag, fields),
        })
    }

    pub(in crate::checker) fn check_error_arguments(
        &mut self,
        span: &Span,
        name: &str,
        schema: &[(String, Ty)],
        args: &[ResolvedRecordLitArg],
    ) -> Result<Vec<TypedNode>, TypeError> {
        let named = args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(..)));
        let mut values = vec![None; schema.len()];
        if args.len() != schema.len() {
            return Err(TypeError::new(
                format!(
                    "{} expects {} arguments, got {}",
                    name,
                    schema.len(),
                    args.len()
                ),
                span.clone(),
            ));
        }
        for (position, argument) in args.iter().enumerate() {
            let (index, expression) = if named {
                let (field, expression) = match argument {
                    ResolvedRecordLitArg::Named(field, expression) => (field.as_str(), expression),
                    ResolvedRecordLitArg::Positional(expression) => {
                        match expression {
                            Resolved::Var(_, variable) => (variable.name.as_str(), expression),
                            _ => return Err(TypeError::new(
                                "Error named arguments require an explicit name or a bare variable",
                                span.clone(),
                            )),
                        }
                    }
                };
                let index = schema
                    .iter()
                    .position(|(name, _)| name == field)
                    .ok_or_else(|| {
                        TypeError::new(
                            format!("Unknown Error field or input: {}", field),
                            span.clone(),
                        )
                    })?;
                (index, expression)
            } else {
                let ResolvedRecordLitArg::Positional(expression) = argument else {
                    unreachable!()
                };
                (position, expression)
            };
            if values[index].is_some() {
                return Err(TypeError::new(
                    format!("Duplicate Error field or input: {}", schema[index].0),
                    span.clone(),
                ));
            }
            let value = self.check_node_with_expected(expression, Some(&schema[index].1))?;
            self.assert_type_relation(
                &schema[index].1,
                &value.ty,
                self.type_fact(SourceRole::Expected, span, &schema[index].1),
                self.type_fact(SourceRole::Value, &value.span, &value.ty),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Call,
                name,
                index as u32,
            )?;
            values[index] = Some(value);
        }
        let values = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                value.ok_or_else(|| {
                    TypeError::new(
                        format!("Missing Error field or input: {}", schema[index].0),
                        span.clone(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.ensure_no_runtime_facet_args(&values, span, "Error constructor arguments")?;
        Ok(values)
    }

    pub(in crate::checker) fn error_kind_error_hint(&self, node: &TypedNode) -> Option<String> {
        if !matches!(self.resolve_ty(&node.ty), Ty::Enum(name, args) if name == "ErrorKind" && args.is_empty())
        {
            return None;
        }
        if let TypedInner::ErrorKind(kind) = &node.node {
            let id = self.function_ids_by_name.get(kind)?;
            let inputs = self.env.error_constructor_inputs.get(&id.unique_id)?;
            let name = kind.strip_prefix("Global::").unwrap_or(kind);
            return Some(if inputs.is_empty() {
                format!("Call `{name}()` to construct an Error; the bare name is an ErrorKind.")
            } else {
                format!("Call `{name}(...)` with the declared constructor inputs to construct an Error; the bare name is an ErrorKind.")
            });
        }
        Some("ErrorKind identifies a deferror declaration. Pass an Error value or call a concrete deferror constructor explicitly.".into())
    }

    pub(in crate::checker) fn is_abstract_error_marker_value(&self, node: &TypedNode) -> bool {
        matches!(&node.node, TypedInner::Var(id) if id.name == "Error" && !self.env.is_error_constructor(id.unique_id))
    }

    pub(in crate::checker) fn ensure_guard_error_value(
        &self,
        node: &TypedNode,
        form_name: &str,
    ) -> Result<(), TypeError> {
        if !matches!(node.ty, Ty::Error) {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{} error branch must evaluate to Error, got {}",
                    form_name,
                    self.ty_name(&node.ty)
                ),
                span: node.span.clone(),
                hint: self.error_kind_error_hint(node),
            });
        }
        Ok(())
    }

    pub(in crate::checker) fn ensure_result_error_arg(
        &self,
        node: &TypedNode,
        form_name: &str,
    ) -> Result<(), TypeError> {
        if !matches!(node.ty, Ty::Error) {
            return Err(TypeError {
                structured: None,
                message: format!(
                    "{} error argument must evaluate to Error, got {}",
                    form_name,
                    self.ty_name(&node.ty)
                ),
                span: node.span.clone(),
                hint: self.error_kind_error_hint(node),
            });
        }
        Ok(())
    }
}
