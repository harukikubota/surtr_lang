use super::*;

impl Checker {
    fn warning_span(span: &Span) -> WarningSpan {
        WarningSpan {
            start: span.start,
            end: span.end,
        }
    }

    fn push_warning(
        &mut self,
        kind: WarningKind,
        phase: WarningPhase,
        span: &Span,
        message: impl Into<String>,
        hint: Option<String>,
    ) {
        self.warnings.push(CompilerWarning::new(
            kind,
            phase,
            Self::warning_span(span),
            message,
            hint,
        ));
    }

    pub(super) fn collect_unused_type_parameter_warnings(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, type_params, fields, _) => {
                    let mut used = HashSet::new();
                    for field in fields {
                        Self::collect_ast_ty_type_params(&field.ty, &mut used);
                    }
                    self.warn_unused_type_params(type_params, &used, &id.name);
                }
                Resolved::EnumDef(_, id, type_params, variants, _) => {
                    let mut used = HashSet::new();
                    for variant in variants {
                        for payload in &variant.payload {
                            Self::collect_ast_ty_type_params(payload, &mut used);
                        }
                    }
                    self.warn_unused_type_params(type_params, &used, &id.name);
                }
                Resolved::Def(_, id, _return_type_arguments, params, ret_ty, _, _, _) => {
                    let used = Self::signature_type_param_uses(
                        params,
                        ret_ty.as_ref().map(|ty| ty.syntax()),
                    );
                    self.warn_unused_type_params(&[], &used, &id.name);
                }
                Resolved::ExtractorDef(_, id, type_params, param, ret_ty, _, _) => {
                    let mut used = HashSet::new();
                    for param in param {
                        if let Some(param_ty) = &param.ty {
                            Self::collect_ast_ty_type_params(param_ty, &mut used);
                        }
                    }
                    Self::collect_ast_ty_type_params(ret_ty, &mut used);
                    self.warn_unused_type_params(type_params, &used, &id.name);
                }
                Resolved::TraitDef(_, id, type_params, _, methods, _) => {
                    let mut trait_used = HashSet::new();
                    for method in methods {
                        Self::validate_trait_method_input_slots(method)?;
                        let mut return_type_argument_slots = HashSet::new();
                        for return_type_argument in &method.return_type_arguments {
                            Self::collect_ast_ty_type_params(
                                &return_type_argument.ty,
                                &mut return_type_argument_slots,
                            );
                        }
                        let mut value_param_slots = HashSet::new();
                        for param in &method.value_parameters {
                            Self::collect_ast_ty_type_params(&param.ty, &mut trait_used);
                            Self::collect_ast_ty_type_params(&param.ty, &mut value_param_slots);
                        }
                        let mut method_input_used = return_type_argument_slots;
                        method_input_used.extend(value_param_slots);
                        let mut method_output_used = HashSet::new();
                        Self::collect_ast_ty_type_params(&method.ret_ty, &mut trait_used);
                        Self::collect_ast_ty_type_params(&method.ret_ty, &mut method_output_used);

                        let mut method_used = method_input_used.clone();
                        method_used.extend(method_output_used.iter().cloned());
                        if method
                            .where_clause
                            .as_ref()
                            .map(|clause| {
                                clause.constraints.iter().any(|constraint| {
                                    let mut vars = HashSet::new();
                                    Self::collect_ast_ty_type_params(
                                        &constraint.subject,
                                        &mut vars,
                                    );
                                    vars.into_iter().any(|var| !method_used.contains(&var))
                                })
                            })
                            .unwrap_or(false)
                        {
                            return Err(TypeError {
                                structured: None,
                                message: format!(
                                    "Trait method {} has a type variable used only by where constraints",
                                    method.id.name
                                ),
                                span: method.span.clone(),
                                hint: Some("Add the type variable to ReturnTypeArguments or a value argument.".into()),
                            });
                        }
                        if id.name == "Default"
                            && method.id.name == "default"
                            && !method.return_type_arguments.iter().any(
                                |param| matches!(&*param.ty, AstTy::Named(_, name) if name == "Self"),
                            )
                            {
                            return Err(TypeError {
                                structured: None,
                                message: "Default::default must declare Self in ReturnTypeArguments".into(),
                                span: method.span.clone(),
                                hint: Some("Use `def default::<Self>() -> Self`.".into()),
                            });
                        }

                        self.warn_unused_type_params(
                            &method.type_params,
                            &method_used,
                            &method.id.name,
                        );
                    }
                    self.warn_unused_type_params(type_params, &trait_used, &id.name);
                }
                Resolved::TraitImplDef(_, _, _, _, _, _, methods) => {
                    for method in methods {
                        let used = Self::signature_type_param_uses(
                            &method.value_parameters,
                            method.ret_ty.as_ref().map(|ty| ty.syntax()),
                        );
                        self.warn_unused_type_params(
                            &method.type_params,
                            &used,
                            &method.function_id.name,
                        );
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn signature_type_param_uses(
        params: &[ResolvedValueParameter],
        ret_ty: Option<&AstTy>,
    ) -> HashSet<String> {
        let mut used = HashSet::new();
        for param in params {
            Self::collect_ast_ty_type_params(&param.ty, &mut used);
        }
        if let Some(ret_ty) = ret_ty {
            Self::collect_ast_ty_type_params(ret_ty, &mut used);
        }
        used
    }

    pub(super) fn collect_ast_ty_type_params(ty: &AstTy, used: &mut HashSet<String>) {
        match ty {
            AstTy::Named(_, name) => {
                if name.starts_with('$') {
                    used.insert(name.clone());
                }
            }
            AstTy::ImplTrait(_, _) => {}
            AstTy::Generic(_, name, args) => {
                if name.starts_with('$') {
                    used.insert(name.clone());
                }
                for arg in args {
                    Self::collect_ast_ty_type_params(arg, used);
                }
            }
            AstTy::Tuple(_, items) => {
                for item in items {
                    Self::collect_ast_ty_type_params(item, used);
                }
            }
            AstTy::Func(_, params, ret) => {
                for param in params {
                    Self::collect_ast_ty_type_params(param, used);
                }
                Self::collect_ast_ty_type_params(ret, used);
            }
        }
    }

    fn warn_unused_type_params(
        &mut self,
        type_params: &[ResolvedTypeParam],
        used: &HashSet<String>,
        owner_name: &str,
    ) {
        for param in type_params {
            if used.contains(&param.name) {
                continue;
            }
            self.push_warning(
                WarningKind::UnusedTypeParameter,
                WarningPhase::Typecheck,
                &param.span,
                format!("unused type parameter `{}` in `{}`", param.name, owner_name),
                Some("Remove the type parameter if it is not part of the signature.".into()),
            );
        }
    }

    pub(super) fn collect_unused_value_warnings_in_sequence(&mut self, nodes: &[TypedNode]) {
        for (idx, node) in nodes.iter().enumerate() {
            if idx + 1 < nodes.len()
                && !matches!(node.ty, Ty::Unit)
                && !Self::is_explicit_discard(node)
            {
                self.push_warning(
                    WarningKind::UnusedValue,
                    WarningPhase::Typecheck,
                    &node.span,
                    "unused value",
                    Some("Use `;` if this value is intentionally discarded.".into()),
                );
            }
            self.collect_unused_value_warnings_in_node(node);
        }
    }

    fn collect_unused_value_warnings_in_node(&mut self, node: &TypedNode) {
        for expr in Self::pattern_expression_nodes(node) {
            self.collect_unused_value_warnings_in_node(expr);
        }
        match &node.node {
            TypedInner::ErrorConstruct {
                message, payload, ..
            } => {
                self.collect_unused_value_warnings_in_node(message);
                for value in payload {
                    self.collect_unused_value_warnings_in_node(value);
                }
            }
            TypedInner::Block(stmts) => self.collect_unused_value_warnings_in_sequence(stmts),
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                self.collect_unused_value_warnings_in_node(func);
                for arg in args {
                    self.collect_unused_value_warnings_in_node(arg);
                }
            }
            TypedInner::TraitCall { args, .. }
            | TypedInner::ListLiteral(args)
            | TypedInner::TupleLiteral(args)
            | TypedInner::StructLit(_, args)
            | TypedInner::ConstructorCall(_, args) => {
                for arg in args {
                    self.collect_unused_value_warnings_in_node(arg);
                }
            }
            TypedInner::Bind(_, rhs)
            | TypedInner::ApplyPattern { value: rhs, .. }
            | TypedInner::SafeBind(_, rhs, _, _)
            | TypedInner::FieldAccess(rhs, _)
            | TypedInner::Semi(rhs) => self.collect_unused_value_warnings_in_node(rhs),
            TypedInner::DoSafeBind(control) => {
                self.collect_unused_value_warnings_in_node(&control.rhs);
                if let SafeBindFailureTarget::DoAlternative { empty } = &control.failure_target {
                    self.collect_unused_value_warnings_in_node(empty);
                }
                self.collect_unused_value_warnings_in_node(&control.continuation);
            }
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right) => {
                self.collect_unused_value_warnings_in_node(left);
                self.collect_unused_value_warnings_in_node(right);
            }
            TypedInner::InterpolatedStr(parts) => {
                for part in parts {
                    if let TypedInterpolatedPart::Expr(expr) = part {
                        self.collect_unused_value_warnings_in_node(expr);
                    }
                }
            }
            TypedInner::Dbg(args) => {
                for arg in args {
                    self.collect_unused_value_warnings_in_node(&arg.expr);
                }
            }
            TypedInner::EagerBoundary(inner) => self.collect_unused_value_warnings_in_node(inner),
            TypedInner::If(cond, then_branch, else_branch) => {
                self.collect_unused_value_warnings_in_node(cond);
                self.collect_unused_value_warnings_in_node(then_branch);
                if let Some(else_branch) = else_branch {
                    self.collect_unused_value_warnings_in_node(else_branch);
                }
            }
            TypedInner::Cause(cond, err) => {
                self.collect_unused_value_warnings_in_node(cond);
                self.collect_unused_value_warnings_in_node(err);
            }
            TypedInner::Match(scrutinee, arms) => {
                self.collect_unused_value_warnings_in_node(scrutinee);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.collect_unused_value_warnings_in_node(guard);
                    }
                    self.collect_unused_value_warnings_in_node(&arm.body);
                }
            }
            TypedInner::SupervisorSpawn { init, .. }
            | TypedInner::SupervisorWorkers { init, .. } => {
                self.collect_unused_value_warnings_in_node(init);
            }
            TypedInner::SupervisorAdopt { pid, .. } => {
                self.collect_unused_value_warnings_in_node(pid);
            }
            TypedInner::FacetView { source, .. } => {
                self.collect_unused_value_warnings_in_node(source);
            }
            TypedInner::FacetSet { source, value, .. } => {
                self.collect_unused_value_warnings_in_node(source);
                self.collect_unused_value_warnings_in_node(value);
            }
            TypedInner::FacetOver {
                source, update_fun, ..
            } => {
                self.collect_unused_value_warnings_in_node(source);
                self.collect_unused_value_warnings_in_node(update_fun);
            }
            TypedInner::DeferrorDef(_, _, _, _, show)
            | TypedInner::Def(_, _, _, _, _, _, show, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, show, _)
            | TypedInner::Closure(_, _, show)
            | TypedInner::ExtractorClosure(_, _, show)
            | TypedInner::CaptureClosure(_, _, show) => {
                self.collect_unused_value_warnings_in_node(show);
            }
            TypedInner::CaptureConstructorClosure(_, _, _, show) => {
                self.collect_unused_value_warnings_in_node(show);
            }
            TypedInner::HashMapLiteral(entries) => {
                for (key, value) in entries {
                    self.collect_unused_value_warnings_in_node(key);
                    self.collect_unused_value_warnings_in_node(value);
                }
            }
            TypedInner::Lit(_)
            | TypedInner::ErrorKind(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil
            | TypedInner::ProcessContextHandler { .. }
            | TypedInner::SupervisorStatus { .. }
            | TypedInner::FacetPath(_)
            | TypedInner::PendingFacetPath(_)
            | TypedInner::EnumDef(_, _)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::BuiltinExtractorDecl(_, _, _)
            | TypedInner::StructDef(_, _, _, _)
            | TypedInner::RecordDef(_, _, _, _) => {}
        }
    }

    fn is_explicit_discard(node: &TypedNode) -> bool {
        matches!(node.node, TypedInner::Semi(_))
    }
}
