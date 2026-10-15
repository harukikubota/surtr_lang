use super::*;
use diagnostics::{DiagnosticOrigin, SourceRole, TypeDiagnosticReason};

enum OrderedCallArgument<'a> {
    Present(&'a Resolved),
    Missing(&'a str),
}

#[derive(Clone, Copy)]
enum CallArgumentMode {
    User {
        defer_constructor_conflicts: bool,
        callee_uid: u32,
    },
    Positional,
}

impl Checker {
    pub(in crate::checker) fn typecheck_user_function_args(
        &mut self,
        span: &Span,
        callee_uid: u32,
        callee_label: &str,
        params: &[Ty],
        args: &[ResolvedRecordLitArg],
        callable_hint: Option<&str>,
        defer_constructor_conflicts: bool,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let has_named = args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)));
        let has_positional = args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Positional(_)));
        if has_named && has_positional {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArgumentModeMismatch,
                    callee_label,
                    None,
                    params.len(),
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: "Cannot mix positional and named arguments".into(),
                span: span.clone(),
                hint: None,
            });
        }

        let param_names = self
            .callable_signatures
            .get(&callee_uid)
            .map(|signature| {
                signature
                    .value_parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect::<Vec<_>>()
            })
            .or_else(|| self.user_func_params.get(&callee_uid).cloned());

        if has_named {
            let names = param_names.as_ref().ok_or_else(|| TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArgumentModeMismatch,
                    callee_label,
                    None,
                    params.len(),
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: "This function value does not accept named arguments".into(),
                span: span.clone(),
                hint: None,
            })?;

            let mut reordered: Vec<Option<&Resolved>> = vec![None; params.len()];
            for arg in args {
                let ResolvedRecordLitArg::Named(name, expr) = arg else {
                    return Err(TypeError {
                        structured: Some(self.argument_contract_diagnostic(
                            TypeDiagnosticReason::ArgumentModeMismatch,
                            callee_label,
                            None,
                            params.len(),
                            args.len(),
                            span,
                            DiagnosticOrigin::Call,
                        )),
                        message: "Cannot mix positional and named arguments".into(),
                        span: span.clone(),
                        hint: None,
                    });
                };
                let idx = names
                    .iter()
                    .position(|n| n == name)
                    .ok_or_else(|| TypeError {
                        structured: Some(self.argument_contract_diagnostic(
                            TypeDiagnosticReason::UnknownNamedArgument,
                            callee_label,
                            Some(name),
                            params.len(),
                            args.len(),
                            span,
                            DiagnosticOrigin::Call,
                        )),
                        message: format!("Unknown argument name '{}' for function", name),
                        span: span.clone(),
                        hint: None,
                    })?;
                if reordered[idx].is_some() {
                    return Err(TypeError {
                        structured: Some(self.argument_contract_diagnostic(
                            TypeDiagnosticReason::DuplicateArgument,
                            callee_label,
                            Some(name),
                            params.len(),
                            args.len(),
                            span,
                            DiagnosticOrigin::Call,
                        )),
                        message: format!("Duplicate argument '{}'", name),
                        span: span.clone(),
                        hint: None,
                    });
                }
                reordered[idx] = Some(expr);
            }

            let ordered = reordered
                .into_iter()
                .zip(names)
                .map(|(expr, name)| match expr {
                    Some(expr) => OrderedCallArgument::Present(expr),
                    None => OrderedCallArgument::Missing(name),
                })
                .collect::<Vec<_>>();
            return self.typecheck_ordered_call_args(
                span,
                callee_label,
                params,
                &ordered,
                callable_hint,
                CallArgumentMode::User {
                    defer_constructor_conflicts,
                    callee_uid,
                },
            );
        }

        if args.len() != params.len() {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArityMismatch,
                    callee_label,
                    None,
                    params.len(),
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: format!(
                    "{} expects {} argument(s), got {}",
                    callee_label,
                    params.len(),
                    args.len()
                ),
                span: span.clone(),
                hint: callable_hint.map(str::to_string),
            });
        }

        let ordered = args
            .iter()
            .map(|arg| match arg {
                ResolvedRecordLitArg::Positional(expr) => OrderedCallArgument::Present(expr),
                ResolvedRecordLitArg::Named(_, _) => unreachable!("argument modes were validated"),
            })
            .collect::<Vec<_>>();
        self.typecheck_ordered_call_args(
            span,
            callee_label,
            params,
            &ordered,
            callable_hint,
            CallArgumentMode::User {
                defer_constructor_conflicts,
                callee_uid,
            },
        )
    }

    fn typecheck_ordered_call_args(
        &mut self,
        span: &Span,
        callee_label: &str,
        params: &[Ty],
        args: &[OrderedCallArgument<'_>],
        callable_hint: Option<&str>,
        mode: CallArgumentMode,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let mut typed_args = vec![None; params.len()];
        let mut remaining = params.len();
        loop {
            let before = remaining;
            let mut checked_this_pass = Vec::new();
            for (ordinal, (expected_ty, arg)) in params.iter().zip(args).enumerate() {
                if typed_args[ordinal].is_some() {
                    continue;
                }
                let expr = match arg {
                    OrderedCallArgument::Present(expr) => *expr,
                    OrderedCallArgument::Missing(name) => {
                        return Err(TypeError {
                            structured: Some(
                                self.argument_contract_diagnostic(
                                    TypeDiagnosticReason::MissingArgument,
                                    callee_label,
                                    Some(name),
                                    params.len(),
                                    args.iter()
                                        .filter(|arg| {
                                            matches!(arg, OrderedCallArgument::Present(_))
                                        })
                                        .count(),
                                    span,
                                    DiagnosticOrigin::Call,
                                ),
                            ),
                            message: format!("Missing argument '{}'", name),
                            span: span.clone(),
                            hint: None,
                        });
                    }
                };
                if self
                    .pending_facet_capture_source(expr, expected_ty)
                    .is_some()
                {
                    continue;
                }
                let callback_inputs = if Self::has_contextual_callback_parameters(expr) {
                    self.callback_input_provenance(
                        params,
                        &typed_args,
                        ordinal,
                        self.resolved_span(expr),
                    )?
                } else {
                    Some(Vec::new())
                };
                let Some(callback_inputs) = callback_inputs else {
                    continue;
                };
                let lazy_parameter = match mode {
                    CallArgumentMode::User { callee_uid, .. } => self
                        .lazy_function_parameters
                        .get(&callee_uid)
                        .is_some_and(|positions| positions.contains(&ordinal)),
                    CallArgumentMode::Positional => false,
                };
                let (typed, defer) =
                    self.with_callback_parameter_provenance(expr, &callback_inputs, |checker| {
                        if lazy_parameter {
                            checker
                                .check_trait_lazy_argument(expr, expected_ty, span)
                                .map(|typed| (typed, false))
                        } else {
                            checker.check_ordered_call_argument(
                                expr,
                                expected_ty,
                                callee_label,
                                mode,
                            )
                        }
                    })?;
                if !defer {
                    if matches!(mode, CallArgumentMode::Positional) {
                        checked_this_pass.push(ordinal);
                    } else {
                        self.check_call_argument_relation(
                            span,
                            callee_label,
                            expected_ty,
                            &typed,
                            callable_hint,
                            ordinal,
                        )?;
                    }
                }
                typed_args[ordinal] = Some(typed);
                remaining -= 1;
            }
            // Function-value/positional calls historically check expressions
            // before relating their types. Preserve that diagnostic order while
            // making their constraints available to source-dependent captures.
            for ordinal in checked_this_pass {
                self.check_call_argument_relation(
                    span,
                    callee_label,
                    &params[ordinal],
                    typed_args[ordinal].as_ref().expect("argument was checked"),
                    callable_hint,
                    ordinal,
                )?;
            }
            if remaining == 0 {
                let typed_args = typed_args
                    .into_iter()
                    .map(|arg| arg.expect("all call arguments were checked"))
                    .collect::<Vec<_>>();
                self.check_callback_argument_provenance(params, &typed_args)?;
                return Ok(typed_args);
            }
            if remaining == before {
                let ordinal = typed_args
                    .iter()
                    .position(Option::is_none)
                    .expect("remaining call argument exists");
                let OrderedCallArgument::Present(expr) = args[ordinal] else {
                    unreachable!("missing arguments were rejected")
                };
                let Some(source) = self.pending_facet_capture_source(expr, &params[ordinal]) else {
                    return Err(self.unresolved_callback_provenance_error(self.resolved_span(expr)));
                };
                return Err(TypeError {
                    structured: None,
                    message: format!("FacetPath capture has unresolved source type: {}",
                        self.diagnostic_ty_name(&source)),
                    span: self.resolved_span(expr).clone(),
                    hint: Some("Provide the source type through another argument or a callable annotation.".into()),
                });
            }
        }
    }

    fn check_ordered_call_argument(
        &mut self,
        expr: &Resolved,
        expected_ty: &Ty,
        callee_label: &str,
        mode: CallArgumentMode,
    ) -> Result<(TypedNode, bool), TypeError> {
        Ok(match mode {
            CallArgumentMode::User {
                defer_constructor_conflicts,
                ..
            } => {
                let defer = defer_constructor_conflicts
                    && self.constructor_argument_needs_deferred_check(expected_ty);
                let typed = if defer {
                    self.check_node(expr)?
                } else {
                    self.check_node_with_expected(expr, Some(expected_ty))?
                };
                self.ensure_no_runtime_facet_value(&typed, "Function call arguments")?;
                self.check_expected_constructor_capability(expected_ty, callee_label, &typed)?;
                (typed, defer)
            }
            CallArgumentMode::Positional => (
                if matches!(self.resolve_ty(expected_ty), Ty::Hole) {
                    self.check_node(expr)?
                } else {
                    self.check_node_with_expected(expr, Some(expected_ty))?
                },
                false,
            ),
        })
    }

    pub(super) fn check_call_argument_relation(
        &mut self,
        span: &Span,
        callee_label: &str,
        expected: &Ty,
        typed: &TypedNode,
        callable_hint: Option<&str>,
        ordinal: usize,
    ) -> Result<(), TypeError> {
        if matches!(self.resolve_ty(expected), Ty::Hole) {
            return Ok(());
        }
        self.assert_type_relation(
            expected,
            &typed.ty,
            self.type_fact(SourceRole::Expected, span, expected),
            self.type_fact(SourceRole::Value, &typed.span, &typed.ty),
            TypeDiagnosticReason::ArgumentTypeMismatch,
            DiagnosticOrigin::Call,
            callee_label,
            ordinal as u32,
        )
        .map_err(|error| match callable_hint {
            Some(hint) => error.with_hint(hint),
            None => error,
        })
    }

    fn constructor_argument_needs_deferred_check(&self, expected_ty: &Ty) -> bool {
        let Ty::SelfApp(items) = expected_ty else {
            return false;
        };
        let Some((witness, _)) = Self::constructor_application_parts(items) else {
            return false;
        };
        let resolved = self.resolve_ty(witness);
        if matches!(&resolved, Ty::Var(variable) if !self.rigid_tyvars.contains(variable)) {
            return true;
        }
        let Ty::Var(variable) = witness else {
            return false;
        };
        if self.constructor_witness_traits.contains_key(variable) {
            if let Ok(canonical) = self.canonical_request(&resolved) {
                fn contains_inference_variable(ty: &CanonicalTy, rigid: &HashSet<u32>) -> bool {
                    matches!(ty.head, CanonicalTypeHead::Variable(variable) if !rigid.contains(&variable))
                        || ty
                            .arguments
                            .iter()
                            .any(|argument| contains_inference_variable(argument, rigid))
                }
                if contains_inference_variable(&canonical, &self.rigid_tyvars) {
                    return true;
                }
            }
        }
        self.constructor_witness_traits
            .get(variable)
            .and_then(|trait_key| {
                self.normalize_inferred_constructor_identity(trait_key, &resolved)
            })
            .is_some_and(|identity| identity != resolved)
    }

    pub(in crate::checker) fn typecheck_positional_call_args(
        &mut self,
        span: &Span,
        callee_name: &str,
        params: &[Ty],
        args: &[ResolvedRecordLitArg],
        callable_hint: Option<String>,
        named_arg_error: String,
    ) -> Result<Vec<TypedNode>, TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArgumentModeMismatch,
                    callee_name,
                    None,
                    params.len(),
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: named_arg_error,
                span: span.clone(),
                hint: None,
            });
        }

        if args.len() != params.len() {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArityMismatch,
                    callee_name,
                    None,
                    params.len(),
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: format!(
                    "{} expects {} argument(s), got {}",
                    callee_name,
                    params.len(),
                    args.len()
                ),
                span: span.clone(),
                hint: callable_hint.clone(),
            });
        }

        let ordered = args
            .iter()
            .map(|arg| match arg {
                ResolvedRecordLitArg::Positional(expr) => OrderedCallArgument::Present(expr),
                ResolvedRecordLitArg::Named(_, _) => unreachable!("named arguments were rejected"),
            })
            .collect::<Vec<_>>();
        let typed_args = self.typecheck_ordered_call_args(
            span,
            callee_name,
            params,
            &ordered,
            callable_hint.as_deref(),
            CallArgumentMode::Positional,
        )?;

        typed_args
            .into_iter()
            .map(|arg| {
                self.concretize_pending_trait_calls(arg)
                    .map(|node| *node)
                    .map_err(|error| *error)
            })
            .collect::<Result<Vec<_>, _>>()
    }
}
