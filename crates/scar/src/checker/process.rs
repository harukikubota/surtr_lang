use super::*;

impl Checker {
    pub(super) fn set_process_handler_dependencies(&mut self, process_specs: &[TypedProcessSpec]) {
        self.process_specs = process_specs.to_vec();
        self.process_handler_dependencies = process_specs
            .iter()
            .map(|spec| {
                let slots = spec
                    .spec
                    .handlers
                    .iter()
                    .map(|handler| (handler.slot.clone(), handler.capability.clone()))
                    .collect::<HashMap<_, _>>();
                (spec.process_name.clone(), slots)
            })
            .collect();
    }

    pub(super) fn is_lazy_init_function_symbol(&self, symbol: &str) -> bool {
        self.process_specs.iter().any(|spec| {
            spec.spec.standby
                && self
                    .function_ids_by_name
                    .get(symbol)
                    .is_some_and(|id| id.unique_id == spec.init_uid)
        })
    }

    pub(super) fn ty_contains_process_init(&self, ty: &Ty) -> bool {
        match self.resolve_ty(ty) {
            Ty::Enum(name, args) => {
                Self::builtin_type_is_process_boundary_only(&name)
                    || args.iter().any(|arg| self.ty_contains_process_init(arg))
            }
            Ty::Result(ok, err) => {
                self.ty_contains_process_init(&ok) || self.ty_contains_process_init(&err)
            }
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner)
            | Ty::Pid(inner) => self.ty_contains_process_init(&inner),
            Ty::Tuple(items) | Ty::SelfApp(items) => {
                items.iter().any(|item| self.ty_contains_process_init(item))
            }
            Ty::Func(params, ret) => {
                params
                    .iter()
                    .any(|param| self.ty_contains_process_init(param))
                    || self.ty_contains_process_init(&ret)
            }
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => {
                nominal
                    .arguments
                    .iter()
                    .any(|argument| self.ty_contains_process_init(argument))
                    || nominal
                        .iter()
                        .any(|(_, field_ty)| self.ty_contains_process_init(field_ty))
            }
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| self.ty_contains_process_init(param))
                    || self.ty_contains_process_init(&ret)
            }
            Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::ProcessMarker(_)
            | Ty::Hole
            | Ty::Var(_)
            | Ty::Error
            | Ty::Facet(..) => false,
        }
    }

    fn process_init_state_ty(&self, ty: &Ty) -> Option<Ty> {
        match self.resolve_ty(ty) {
            Ty::Enum(name, args) if Self::builtin_type_is_process_boundary_only(&name) => {
                args.into_iter().next()
            }
            _ => None,
        }
    }

    fn builtin_type_is_process_boundary_only(name: &str) -> bool {
        builtin_type_usage_policy(Self::surface_name(name)).is_some_and(|policy| {
            policy.process_boundary_allowed && !policy.type_annotation_allowed
        })
    }

    fn process_handler_function_ty(&self, uid: u32) -> Option<(Vec<Ty>, Ty)> {
        match self.env.lookup_var(uid)? {
            Ty::UserFunc { params, ret, .. } | Ty::BuiltinFunc { params, ret, .. } => {
                Some((params.clone(), ret.as_ref().clone()))
            }
            _ => None,
        }
    }

    fn process_result_ok_ty(&self, ty: &Ty) -> Option<Ty> {
        match self.resolve_ty(ty) {
            Ty::Result(ok, _) => Some(ok.as_ref().clone()),
            _ => None,
        }
    }

    fn process_handler_public_name(process: &TypedProcessSpec, handler_name: &str) -> String {
        format!(
            "{}::{}",
            Self::surface_name(&process.process_name),
            handler_name
        )
    }

    fn validate_handler_first_param_state(
        &self,
        process: &TypedProcessSpec,
        handler_kind: &str,
        handler_name: &str,
        params: &[Ty],
        state_ty: &Ty,
        state_name: &str,
        span: Span,
    ) -> Result<(), TypeError> {
        let state_param = match params.first() {
            Some(first)
                if matches!(
                    self.resolve_ty(first),
                    Ty::Pid(marker)
                        if matches!(marker.as_ref(), Ty::ProcessMarker(name) if name == &process.process_name)
                ) =>
            {
                params.get(1)
            }
            _ => params.first(),
        };
        if state_param.is_some_and(|param| self.resolve_ty(param) == *state_ty) {
            return Ok(());
        }

        Err(TypeError {
            structured: None,
            message: format!(
                "@{} handler `{}` first parameter must match process state type `{}`",
                handler_kind,
                Self::process_handler_public_name(process, handler_name),
                Self::surface_name(state_name)
            ),
            span,
            hint: None,
        })
    }

    fn validate_handler_result_ok_state(
        &self,
        process: &TypedProcessSpec,
        handler_kind: &str,
        handler_name: &str,
        ret: &Ty,
        state_ty: &Ty,
        state_name: &str,
        span: Span,
    ) -> Result<(), TypeError> {
        if self
            .process_result_ok_ty(ret)
            .is_some_and(|ok| self.resolve_ty(&ok) == *state_ty)
        {
            return Ok(());
        }

        Err(TypeError {
            structured: None,
            message: format!(
                "@{} handler `{}` Result ok type must match process state type `{}`",
                handler_kind,
                Self::process_handler_public_name(process, handler_name),
                Self::surface_name(state_name)
            ),
            span,
            hint: None,
        })
    }

    fn validate_call_handler_result_state(
        &self,
        process: &TypedProcessSpec,
        handler_name: &str,
        ret: &Ty,
        state_ty: &Ty,
        state_name: &str,
        span: Span,
    ) -> Result<(), TypeError> {
        let has_state = self.process_result_ok_ty(ret).is_some_and(|ok| {
            matches!(self.resolve_ty(&ok), Ty::Enum(name, items)
                if Self::surface_name(&name) == "CallResult"
                    && items.len() == 2
                    && self.resolve_ty(&items[1]) == *state_ty)
        });
        if has_state {
            return Ok(());
        }

        Err(TypeError {
            structured: None,
            message: format!(
                "@call handler `{}` Result ok type must be CallResult<Reply, {}>",
                Self::process_handler_public_name(process, handler_name),
                Self::surface_name(state_name)
            ),
            span,
            hint: None,
        })
    }

    fn validate_cast_handler_result_state(
        &self,
        process: &TypedProcessSpec,
        handler_name: &str,
        ret: &Ty,
        state_ty: &Ty,
        state_name: &str,
        span: Span,
    ) -> Result<(), TypeError> {
        let has_state = self.process_result_ok_ty(ret).is_some_and(|ok| {
            matches!(self.resolve_ty(&ok), Ty::Enum(name, items)
                if Self::surface_name(&name) == "CastResult"
                    && items.len() == 1
                    && self.resolve_ty(&items[0]) == *state_ty)
        });
        if has_state {
            return Ok(());
        }

        Err(TypeError {
            structured: None,
            message: format!(
                "@cast handler `{}` Result ok type must be CastResult<{}>",
                Self::process_handler_public_name(process, handler_name),
                Self::surface_name(state_name)
            ),
            span,
            hint: None,
        })
    }

    pub(super) fn validate_process_state_contracts(&mut self) -> Result<(), TypeError> {
        let process_specs = self.process_specs.clone();
        for process in &process_specs {
            if !matches!(
                process.spec.kind,
                spire::ast::ProcessKind::Agent | spire::ast::ProcessKind::GenServer
            ) {
                continue;
            }
            let state_ty =
                self.resolve_ast_ty_in_context(&process.spec.state, TypeSyntaxContext::General)?;
            let Some(Ty::UserFunc { ret, .. } | Ty::BuiltinFunc { ret, .. }) =
                self.env.lookup_var(process.init_uid)
            else {
                continue;
            };
            let init_ok_ty = match self.resolve_ty(ret) {
                Ty::Result(ok, _) => ok.as_ref().clone(),
                other => other,
            };
            let init_state_ty = if process.spec.standby {
                self.process_init_state_ty(&init_ok_ty)
                    .ok_or_else(|| TypeError {
                        structured: None,
                        message: format!(
                            "Standby @init for process `{}` must return Result<StandbyInit<State>>",
                            process.process_name
                        ),
                        span: Span { start: 0, end: 0 },
                        hint: None,
                    })?
            } else {
                if self.ty_contains_process_init(&init_ok_ty) {
                    return Err(TypeError {
                        structured: None,
                        message: "StandbyInit<T> is only allowed as Standby @init return type"
                            .into(),
                        span: Span { start: 0, end: 0 },
                        hint: None,
                    });
                }
                init_ok_ty
            };

            let init_state_ty = self.resolve_ty(&init_state_ty);
            let state_ty = self.resolve_ty(&state_ty);
            let state_name = Self::surface_ast_ty(&process.spec.state);
            if init_state_ty != state_ty {
                return Err(TypeError {
                    structured: None,
                    message: format!(
                        "@init handler `{}::init` Result ok type must match process state type `{}`",
                        Self::surface_name(&process.process_name),
                        state_name,
                    ),
                    span: Span { start: 0, end: 0 },
                    hint: None,
                });
            }
            let handler_uids = process
                .handler_uids
                .iter()
                .map(|handler| (handler.internal_name.as_str(), handler.uid))
                .collect::<HashMap<_, _>>();
            for handler in &process.spec.handler_specs {
                let Some(uid) = handler_uids.get(handler.internal_name.as_str()).copied() else {
                    continue;
                };
                let Some((params, ret)) = self.process_handler_function_ty(uid) else {
                    continue;
                };
                match handler.kind {
                    spire::ast::ProcessRuntimeHandlerKind::Init => {}
                    spire::ast::ProcessRuntimeHandlerKind::Get => {
                        self.validate_handler_first_param_state(
                            process,
                            "get",
                            &handler.name,
                            &params,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                    }
                    spire::ast::ProcessRuntimeHandlerKind::Set => {
                        self.validate_handler_first_param_state(
                            process,
                            "set",
                            &handler.name,
                            &params,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                        self.validate_handler_result_ok_state(
                            process,
                            "set",
                            &handler.name,
                            &ret,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                    }
                    spire::ast::ProcessRuntimeHandlerKind::Call => {
                        self.validate_handler_first_param_state(
                            process,
                            "call",
                            &handler.name,
                            &params,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                        self.validate_call_handler_result_state(
                            process,
                            &handler.name,
                            &ret,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                    }
                    spire::ast::ProcessRuntimeHandlerKind::Cast => {
                        self.validate_handler_first_param_state(
                            process,
                            "cast",
                            &handler.name,
                            &params,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                        self.validate_cast_handler_result_state(
                            process,
                            &handler.name,
                            &ret,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn process_handler_return_exposes_context_pid(&self, symbol: &str, ty: &Ty) -> bool {
        let Some((process_name, handler)) = symbol.rsplit_once("::") else {
            return false;
        };
        if !Self::is_process_handler_name(handler) {
            return false;
        }
        let Some(slots) = self.process_handler_dependencies.get(process_name) else {
            return false;
        };
        self.ty_contains_handler_capability_pid(ty, slots)
    }

    pub(super) fn is_process_handler_name(handler: &str) -> bool {
        matches!(handler, "__agent_init" | "__agent_get" | "__agent_set")
            || handler.starts_with("__agent_call_")
            || handler.starts_with("__agent_cast_")
    }

    pub(super) fn current_process_spec(&self) -> Option<&TypedProcessSpec> {
        let symbol = self.current_function_symbol.as_deref()?;
        let (module, _) = symbol.rsplit_once("::")?;
        self.process_specs
            .iter()
            .find(|spec| spec.module_path == module || spec.process_name == module)
    }

    pub(super) fn stop_constructor_allowed(&self) -> bool {
        let Some(spec) = self.current_process_spec() else {
            return false;
        };
        if spec.spec.kind != spire::ast::ProcessKind::GenServer
            || spec.spec.instance != spire::ast::ProcessInstance::Worker
        {
            return false;
        }
        let Some(symbol) = self.current_function_symbol.as_deref() else {
            return false;
        };
        let Some((_, function_name)) = symbol.rsplit_once("::") else {
            return false;
        };
        function_name != "__agent_init"
    }

    pub(super) fn stop_constructor_error(&self, span: &Span, enum_name: &str) -> TypeError {
        TypeError {
            structured: None,
            message: format!(
                "{} can only be used inside Worker GenServer @call/@cast handlers or local helper functions",
                enum_name
            ),
            span: span.clone(),
            hint: Some(
                "Use Stop(...) only from Worker defgenserver handlers or helper defs in the same process block."
                    .into(),
            ),
        }
    }

    fn ty_contains_handler_capability_pid(&self, ty: &Ty, slots: &HashMap<String, String>) -> bool {
        match self.resolve_ty(ty) {
            Ty::Pid(marker) => {
                matches!(marker.as_ref(), Ty::ProcessMarker(name) if slots.values().any(|capability| Self::canonical_user_type_name(capability) == *name))
            }
            Ty::Result(ok, err) => {
                self.ty_contains_handler_capability_pid(&ok, slots)
                    || self.ty_contains_handler_capability_pid(&err, slots)
            }
            Ty::List(inner)
            | Ty::MatchResult(inner)
            | Ty::ExtractorClosure(inner)
            | Ty::Lazy(inner) => self.ty_contains_handler_capability_pid(&inner, slots),
            Ty::Tuple(items) | Ty::SelfApp(items) => items
                .iter()
                .any(|item| self.ty_contains_handler_capability_pid(item, slots)),
            Ty::Func(params, ret) => {
                params
                    .iter()
                    .any(|param| self.ty_contains_handler_capability_pid(param, slots))
                    || self.ty_contains_handler_capability_pid(&ret, slots)
            }
            Ty::Struct(_, nominal) | Ty::Record(_, nominal) => {
                nominal
                    .arguments
                    .iter()
                    .any(|argument| self.ty_contains_handler_capability_pid(argument, slots))
                    || nominal.iter().any(|(_, field_ty)| {
                        self.ty_contains_handler_capability_pid(field_ty, slots)
                    })
            }
            Ty::Enum(_, args) => args
                .iter()
                .any(|arg| self.ty_contains_handler_capability_pid(arg, slots)),
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| self.ty_contains_handler_capability_pid(param, slots))
                    || self.ty_contains_handler_capability_pid(&ret, slots)
            }
            Ty::Var(_)
            | Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Error
            | Ty::Hole
            | Ty::ProcessMarker(_) => false,
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                self.ty_contains_handler_capability_pid(&source, slots)
                    || self.ty_contains_handler_capability_pid(&focus, slots)
                    || self.ty_contains_handler_capability_pid(&update_source, slots)
                    || self.ty_contains_handler_capability_pid(&update_focus, slots)
            }
        }
    }
}

#[cfg(test)]
mod process_boundary_policy_tests {
    use super::*;
    use sindr::policy::{CompileUnitKind, ExitCodePolicy, SourceKind};

    #[test]
    fn process_boundary_only_type_query_uses_builtin_usage_policy() {
        assert!(Checker::builtin_type_is_process_boundary_only(
            "StandbyInit"
        ));
        assert!(Checker::builtin_type_is_process_boundary_only(
            "Global::StandbyInit"
        ));
        assert!(!Checker::builtin_type_is_process_boundary_only("PID"));
        assert!(!Checker::builtin_type_is_process_boundary_only("String"));
    }

    #[test]
    fn typecheck_context_can_be_derived_from_source_policy() {
        let source_policy = SourceKind::DefinitionSource.policy(CompileUnitKind::Project, None);
        let context = TypecheckContext::from_source_policy(source_policy);

        assert_eq!(
            context.runtime_policy.exit_code_policy,
            ExitCodePolicy::EntryOnly
        );
        assert!(!context.enforce_builtin_type_contracts);
    }
}
