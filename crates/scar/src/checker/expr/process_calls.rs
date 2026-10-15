use super::*;
use diagnostics::{DiagnosticOrigin, SourceRole, TypeDiagnosticReason};

impl Checker {
    pub(in crate::checker) fn current_process_name(&self) -> Option<String> {
        let symbol = self.current_function_symbol.as_deref()?;
        let (module, handler) = symbol.rsplit_once("::")?;
        Self::is_process_handler_name(handler).then(|| module.to_string())
    }

    fn process_policy_error(
        &self,
        subject: impl Into<String>,
        actual: Option<&Ty>,
        span: &Span,
        hint: Option<String>,
    ) -> TypeError {
        self.policy_error(
            TypeDiagnosticReason::ProcessPolicyViolation,
            diagnostics::TypePolicy::ProcessCapabilityPolicy,
            Some(subject.into()),
            None,
            actual,
            None,
            None,
            span,
            hint,
        )
    }

    pub(super) fn try_check_process_intrinsic_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        if let Some(typed) = self.try_check_supervisor_spawn_app(span, func, args)? {
            return Ok(Some(typed));
        }
        if let Some(typed) = self.try_check_supervisor_adopt_app(span, func, args)? {
            return Ok(Some(typed));
        }
        if let Some(typed) = self.try_check_supervisor_status_app(span, func, args)? {
            return Ok(Some(typed));
        }
        if let Some(typed) = self.try_check_supervisor_workers_app(span, func, args)? {
            return Ok(Some(typed));
        }
        if let Some(typed) = self.try_check_worker_message_template_app(span, func, args)? {
            return Ok(Some(typed));
        }
        self.try_check_singleton_explicit_pid_app(span, func, args)
    }

    fn try_check_supervisor_spawn_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        let Some(supervisor_process) = self.supervisor_spawn_target(func) else {
            return Ok(None);
        };
        self.ensure_supervisor_registered_for_surface(span, &supervisor_process, "spawn")?;
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArgumentModeMismatch,
                    &format!("{supervisor_process}::spawn"),
                    None,
                    1,
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: format!("{supervisor_process}::spawn does not accept named arguments"),
                span: span.clone(),
                hint: None,
            });
        }
        if args.len() != 1 {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArityMismatch,
                    &format!("{supervisor_process}::spawn"),
                    None,
                    1,
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: format!(
                    "{}::spawn expects 1 argument(s), got {}",
                    supervisor_process,
                    args.len()
                ),
                span: span.clone(),
                hint: Some(
                    "Pass a worker init route reference like `MyWorker::init(args)`.".into(),
                ),
            });
        }
        if self.in_compiler_generated_supervisor_wrapper(&supervisor_process, "spawn") {
            return Ok(None);
        }
        let ResolvedRecordLitArg::Positional(worker_init) = &args[0] else {
            unreachable!("validated named arguments above")
        };
        if matches!(worker_init, Resolved::Var(_, _)) {
            return Ok(None);
        }
        let (worker_process, typed_init) = self.synthesize_supervisor_worker_init(worker_init)?;
        match self.resolve_ty(&typed_init.ty) {
            Ty::Func(params, _) if params.is_empty() => {}
            other => {
                return Err(self.process_policy_error(
                    format!(
                        "supervisor spawn expects a zero-argument worker init route, got {}",
                        self.ty_name(&other)
                    ),
                    Some(&other),
                    &typed_init.span,
                    Some(
                        "Pass a generated worker init reference like `MyWorker::init(args)`."
                            .into(),
                    ),
                ));
            }
        }

        Ok(Some(TypedNode {
            ty: Ty::Result(
                Box::new(Ty::pid(worker_process.clone())),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::SupervisorSpawn {
                supervisor_process,
                worker_process,
                init: Box::new(typed_init),
            },
        }))
    }

    fn try_check_supervisor_adopt_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        let Some(supervisor_process) = self.supervisor_intrinsic_target(func, "adopt") else {
            return Ok(None);
        };
        self.ensure_supervisor_registered_for_surface(span, &supervisor_process, "adopt")?;
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArgumentModeMismatch,
                    &format!("{supervisor_process}::adopt"),
                    None,
                    1,
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: format!("{supervisor_process}::adopt does not accept named arguments"),
                span: span.clone(),
                hint: None,
            });
        }
        if args.len() != 1 {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArityMismatch,
                    &format!("{supervisor_process}::adopt"),
                    None,
                    1,
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: format!(
                    "{}::adopt expects 1 argument(s), got {}",
                    supervisor_process,
                    args.len()
                ),
                span: span.clone(),
                hint: Some("Pass a worker PID.".into()),
            });
        }
        let ResolvedRecordLitArg::Positional(pid_expr) = &args[0] else {
            unreachable!("validated named arguments above")
        };
        let typed_pid = self.check_node(pid_expr)?;
        let worker_process = match self.resolve_ty(&typed_pid.ty) {
            Ty::Pid(marker) if matches!(marker.as_ref(), Ty::ProcessMarker(_)) => {
                let Ty::ProcessMarker(name) = *marker else {
                    unreachable!()
                };
                name
            }
            other => {
                return Err(self.process_policy_error(
                    format!(
                        "supervisor adopt expects PID<Worker>, got {}",
                        self.ty_name(&other)
                    ),
                    Some(&other),
                    &typed_pid.span,
                    Some("Pass a worker PID returned from a worker init route.".into()),
                ));
            }
        };
        let supervisor_spec = self
            .supervisor_spec_by_name(&supervisor_process)
            .ok_or_else(|| {
                self.typecheck_invariant_error(
                    format!("supervisor process metadata missing for `{supervisor_process}`"),
                    span,
                )
            })?;
        if !supervisor_spec
            .spec
            .supervisor_policy
            .as_ref()
            .map(|policy| policy.allow_adopt)
            .unwrap_or(false)
        {
            return Err(self.process_policy_error(
                format!(
                    "{}::adopt is not available because allow_adopt is False",
                    supervisor_process
                ),
                None,
                span,
                Some("Enable `allow_adopt: True` in the supervisor definition or override.".into()),
            ));
        }

        Ok(Some(TypedNode {
            ty: Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::SupervisorAdopt {
                supervisor_process,
                worker_process,
                pid: Box::new(typed_pid),
            },
        }))
    }

    fn try_check_supervisor_status_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        let Some(supervisor_process) = self.supervisor_intrinsic_target(func, "status") else {
            return Ok(None);
        };
        self.ensure_supervisor_registered_for_surface(span, &supervisor_process, "status")?;
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArgumentModeMismatch,
                    &format!("{supervisor_process}::status"),
                    None,
                    0,
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: format!("{supervisor_process}::status does not accept named arguments"),
                span: span.clone(),
                hint: None,
            });
        }
        if !args.is_empty() {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArityMismatch,
                    &format!("{supervisor_process}::status"),
                    None,
                    0,
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: format!(
                    "{}::status expects 0 argument(s), got {}",
                    supervisor_process,
                    args.len()
                ),
                span: span.clone(),
                hint: None,
            });
        }
        let status_ty = self
            .env
            .lookup_type_def("SupervisorStatus")
            .map(|def| {
                Ty::Struct(
                    def.name.clone(),
                    NominalType::new(
                        def.type_param_vars.iter().copied().map(Ty::Var).collect(),
                        def.fields.clone(),
                    ),
                )
            })
            .ok_or_else(|| {
                self.typecheck_invariant_error("SupervisorStatus type metadata", span)
            })?;
        Ok(Some(TypedNode {
            ty: Ty::Result(Box::new(status_ty), Box::new(Ty::Error)),
            span: span.clone(),
            node: TypedInner::SupervisorStatus { supervisor_process },
        }))
    }

    fn try_check_supervisor_workers_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        let Some(supervisor_process) = self.supervisor_intrinsic_target(func, "workers") else {
            return Ok(None);
        };
        self.ensure_supervisor_registered_for_surface(span, &supervisor_process, "workers")?;
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArgumentModeMismatch,
                    &format!("{supervisor_process}::workers"),
                    None,
                    2,
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: format!("{supervisor_process}::workers does not accept named arguments"),
                span: span.clone(),
                hint: None,
            });
        }
        if args.len() != 2 {
            return Err(TypeError {
                structured: Some(self.argument_contract_diagnostic(
                    TypeDiagnosticReason::ArityMismatch,
                    &format!("{supervisor_process}::workers"),
                    None,
                    2,
                    args.len(),
                    span,
                    DiagnosticOrigin::Call,
                )),
                message: format!(
                    "{}::workers expects 2 argument(s), got {}",
                    supervisor_process,
                    args.len()
                ),
                span: span.clone(),
                hint: Some("Pass a worker init route and WorkerStrategy.".into()),
            });
        }
        if self.in_compiler_generated_supervisor_wrapper(&supervisor_process, "workers") {
            return Ok(None);
        }
        if !self.supervisor_workers_allowed_in_current_context() {
            return Err(self.process_policy_error(
                "supervisor workers can only be called from Singleton GenServer @init",
                None,
                span,
                Some("Create worker sets in the pool Singleton GenServer @init and keep the handle in state.".into()),
            ));
        }
        let ResolvedRecordLitArg::Positional(worker_init) = &args[0] else {
            unreachable!("validated named arguments above")
        };
        let ResolvedRecordLitArg::Positional(strategy_expr) = &args[1] else {
            unreachable!("validated named arguments above")
        };
        let (worker_process, typed_init) = self.synthesize_supervisor_worker_init(worker_init)?;
        match self.resolve_ty(&typed_init.ty) {
            Ty::Func(params, _) if params.is_empty() => {}
            other => {
                return Err(self.process_policy_error(
                    format!(
                        "supervisor workers expects a zero-argument worker init route, got {}",
                        self.ty_name(&other)
                    ),
                    Some(&other),
                    &typed_init.span,
                    Some(
                        "Pass a generated worker init reference like `MyWorker::init(args)`."
                            .into(),
                    ),
                ));
            }
        }
        let strategy_ty = self
            .env
            .lookup_type_def("WorkerStrategy")
            .map(|def| {
                Ty::Struct(
                    def.name.clone(),
                    NominalType::new(
                        def.type_param_vars.iter().copied().map(Ty::Var).collect(),
                        def.fields.clone(),
                    ),
                )
            })
            .ok_or_else(|| self.typecheck_invariant_error("WorkerStrategy type metadata", span))?;
        let typed_strategy = self.check_node_with_expected(strategy_expr, Some(&strategy_ty))?;
        if !self.types_compatible(&strategy_ty, &typed_strategy.ty)? {
            return Err(self.type_relation_error(
                &strategy_ty,
                &typed_strategy.ty,
                self.type_fact(SourceRole::Expected, span, &strategy_ty),
                self.type_fact(SourceRole::Value, &typed_strategy.span, &typed_strategy.ty),
                TypeDiagnosticReason::ArgumentTypeMismatch,
                DiagnosticOrigin::Call,
                &format!("{supervisor_process}::workers"),
                1,
            ));
        }
        Ok(Some(TypedNode {
            ty: Ty::Result(
                Box::new(Ty::Enum(
                    "Workers".into(),
                    vec![Ty::pid(worker_process.clone())],
                )),
                Box::new(Ty::Error),
            ),
            span: span.clone(),
            node: TypedInner::SupervisorWorkers {
                supervisor_process,
                worker_process,
                init: Box::new(typed_init),
                strategy: Box::new(typed_strategy),
            },
        }))
    }

    fn supervisor_workers_allowed_in_current_context(&self) -> bool {
        let Some(spec) = self.current_process_spec() else {
            return false;
        };
        if spec.spec.kind != spire::ast::ProcessKind::GenServer
            || spec.spec.instance != spire::ast::ProcessInstance::Singleton
        {
            return false;
        }
        let Some(symbol) = self.current_function_symbol.as_deref() else {
            return false;
        };
        let Some((_, handler)) = symbol.rsplit_once("::") else {
            return false;
        };
        handler == "__agent_init"
    }

    fn in_compiler_generated_supervisor_wrapper(
        &self,
        supervisor_process: &str,
        method: &str,
    ) -> bool {
        let Some(symbol) = self.current_function_symbol.as_deref() else {
            return false;
        };
        let expected = format!("{}::{method}", Self::surface_name(supervisor_process));
        Self::surface_name(symbol) == expected
    }

    fn ensure_supervisor_registered_for_surface(
        &self,
        span: &Span,
        supervisor_process: &str,
        method: &str,
    ) -> Result<(), TypeError> {
        if self.in_compiler_generated_supervisor_wrapper(supervisor_process, method) {
            return Ok(());
        }
        if Self::surface_name(supervisor_process) == "DynamicSupervisor" {
            return Ok(());
        }
        let registered = self.boot_plan.entries.iter().any(|entry| {
            Self::surface_name(&entry.process_name) == Self::surface_name(supervisor_process)
        }) || self.boot_plan.supervisors.iter().any(|entry| {
            Self::surface_name(&entry.process_name) == Self::surface_name(supervisor_process)
        });
        if registered {
            return Ok(());
        }
        Err(self.policy_error(
            TypeDiagnosticReason::CompilePolicyViolation,
            diagnostics::TypePolicy::CompileUnitAvailability,
            Some(format!(
                "supervisor surface `{}::{method}` is not available in this compile unit; add the supervisor to supervisor_init",
                Self::surface_name(supervisor_process)
            )),
            None,
            None,
            Some("compile unit".into()),
            None,
            span,
            Some("Register custom supervisors in supervisor_init before using their generated supervisor surface.".into()),
        ))
    }

    fn try_check_worker_message_template_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Ok(None);
        }
        let Resolved::Var(_, id) = func else {
            return Ok(None);
        };
        let qualified = id.qualified_name.as_deref().unwrap_or(&id.name);
        let Some((process_name, _method_name)) = qualified.rsplit_once("::") else {
            return Ok(None);
        };
        let Some(process_name) = self
            .process_specs
            .iter()
            .find(|spec| {
                spec.process_name == process_name
                    && spec.spec.instance == spire::ast::ProcessInstance::Worker
            })
            .map(|spec| spec.process_name.clone())
        else {
            return Ok(None);
        };
        let typed_func = self.check_node(func)?;
        let func_ty = self.resolve_ty(&typed_func.ty);
        let (params, ret) = match &func_ty {
            Ty::UserFunc { params, ret, .. } => (params.as_slice(), ret.clone()),
            Ty::BuiltinFunc { params, ret, .. } => (params.as_slice(), ret.clone()),
            _ => return Ok(None),
        };
        let Some((first_param, remaining_params)) = params.split_first() else {
            return Ok(None);
        };
        let Ty::Pid(pid_process) = self.resolve_ty(first_param) else {
            return Ok(None);
        };
        if !matches!(pid_process.as_ref(), Ty::ProcessMarker(name) if name == &process_name) {
            return Ok(None);
        }
        if args.len() != remaining_params.len() {
            return Ok(None);
        }
        let typed_args: Vec<TypedNode> = args
            .iter()
            .zip(remaining_params.iter())
            .map(|(arg, expected)| match arg {
                ResolvedRecordLitArg::Positional(expr) => {
                    self.check_node_with_expected(expr, Some(expected))
                }
                ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (expected, actual) in remaining_params.iter().zip(typed_args.iter()) {
            if !self.types_compatible(expected, &actual.ty)? {
                return Ok(None);
            }
        }
        Ok(Some(TypedNode {
            ty: Ty::Func(vec![Ty::pid(process_name)], ret),
            span: span.clone(),
            node: TypedInner::InjectCall(Box::new(typed_func), typed_args),
        }))
    }

    fn try_check_singleton_explicit_pid_app(
        &mut self,
        span: &Span,
        func: &Resolved,
        args: &[ResolvedRecordLitArg],
    ) -> Result<Option<TypedNode>, TypeError> {
        if args
            .iter()
            .any(|arg| matches!(arg, ResolvedRecordLitArg::Named(_, _)))
        {
            return Ok(None);
        }
        let Resolved::Var(_, id) = func else {
            return Ok(None);
        };
        let qualified = id.qualified_name.as_deref().unwrap_or(&id.name);
        let Some((process_name, method_name)) = qualified.rsplit_once("::") else {
            return Ok(None);
        };
        if method_name == "pid" {
            return Ok(None);
        }
        let Some(process_name) = self
            .process_specs
            .iter()
            .find(|spec| {
                Self::surface_name(&spec.process_name) == Self::surface_name(process_name)
                    && spec.spec.instance == spire::ast::ProcessInstance::Singleton
            })
            .map(|spec| spec.process_name.clone())
        else {
            return Ok(None);
        };

        let typed_func = self.check_node(func)?;
        let func_ty = self.resolve_ty(&typed_func.ty);
        let (params, ret) = match &func_ty {
            Ty::UserFunc { params, ret, .. } => (params.as_slice(), ret.clone()),
            Ty::BuiltinFunc { params, ret, .. } => (params.as_slice(), ret.clone()),
            _ => return Ok(None),
        };
        if args.len() != params.len() + 1 {
            return Ok(None);
        }

        let pid_ty = Ty::pid(process_name.clone());
        let typed_pid = match &args[0] {
            ResolvedRecordLitArg::Positional(expr) => {
                self.check_node_with_expected(expr, Some(&pid_ty))?
            }
            ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
        };
        if !self.types_compatible(&pid_ty, &typed_pid.ty)? {
            return Ok(None);
        }

        let typed_args: Vec<TypedNode> = args[1..]
            .iter()
            .zip(params.iter())
            .map(|(arg, expected)| match arg {
                ResolvedRecordLitArg::Positional(expr) => {
                    self.check_node_with_expected(expr, Some(expected))
                }
                ResolvedRecordLitArg::Named(_, _) => unreachable!("validated above"),
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (expected, actual) in params.iter().zip(typed_args.iter()) {
            if !self.types_compatible(expected, &actual.ty)? {
                return Ok(None);
            }
        }

        let rewritten_call = TypedNode {
            ty: (*ret).clone(),
            span: span.clone(),
            node: TypedInner::App(Box::new(typed_func), typed_args),
        };
        Ok(Some(TypedNode {
            ty: (*ret).clone(),
            span: span.clone(),
            node: TypedInner::Block(vec![typed_pid, rewritten_call]),
        }))
    }

    fn supervisor_spawn_target(&self, func: &Resolved) -> Option<String> {
        self.supervisor_intrinsic_target(func, "spawn")
    }

    fn supervisor_intrinsic_target(&self, func: &Resolved, method: &str) -> Option<String> {
        let Resolved::Var(_, id) = func else {
            return None;
        };
        let qualified = id.qualified_name.as_deref()?;
        let process_name = qualified.strip_suffix(&format!("::{method}"))?;
        if Self::surface_name(process_name) == "Supervisor" {
            return None;
        }
        // DynamicSupervisor is the compiler-managed root supervisor surface.
        // It is supplied by the standard library, but it is not necessarily
        // present in the current suffix's process-spec list. Keep its calls
        // on the intrinsic lowering path so a generic return-only wrapper is
        // never left as an unmaterialized user-function index.
        if Self::surface_name(process_name) == "DynamicSupervisor" {
            return Some(process_name.to_string());
        }
        self.supervisor_spec_by_name(process_name)
            .map(|spec| spec.process_name.clone())
    }

    fn supervisor_spec_by_name(&self, process_name: &str) -> Option<&TypedProcessSpec> {
        self.process_specs.iter().find(|spec| {
            Self::surface_name(&spec.process_name) == Self::surface_name(process_name)
                && matches!(
                    spec.spec.kind,
                    spire::ast::ProcessKind::Supervisor
                        | spire::ast::ProcessKind::DynamicSupervisor
                        | spire::ast::ProcessKind::RuntimeSupervisor
                )
        })
    }

    fn worker_process_spec_for_init_route<'a>(
        &'a self,
        qualified: &str,
    ) -> Option<(
        &'a TypedProcessSpec,
        &'a spire::ast::ProcessRuntimeHandlerSpec,
    )> {
        self.process_specs.iter().find_map(|spec| {
            if spec.spec.instance != spire::ast::ProcessInstance::Worker {
                return None;
            }
            spec.spec
                .handler_specs
                .iter()
                .find(|handler| {
                    handler.kind == spire::ast::ProcessRuntimeHandlerKind::Init
                        && Self::surface_name(qualified)
                            == Self::surface_name(&format!(
                                "{}::{}",
                                spec.process_name, handler.name
                            ))
                })
                .map(|handler| (spec, handler))
        })
    }

    fn worker_process_spec_for_internal_init_uid(&self, uid: u32) -> Option<&TypedProcessSpec> {
        self.process_specs.iter().find(|spec| {
            spec.spec.instance == spire::ast::ProcessInstance::Worker && spec.init_uid == uid
        })
    }

    fn synthesize_supervisor_worker_init(
        &mut self,
        worker_init: &Resolved,
    ) -> Result<(String, TypedNode), TypeError> {
        let span = self.resolved_span(worker_init).clone();
        if let Resolved::Closure(_, params, _, body)
        | Resolved::ExtractorClosure(_, params, _, body)
        | Resolved::CaptureClosure(_, params, _, body) = worker_init
        {
            if params.is_empty() {
                if let Resolved::App(_, func, _) = body.as_ref() {
                    if let Resolved::Var(_, id) = func.as_ref() {
                        if let Some(process_spec) =
                            self.worker_process_spec_for_internal_init_uid(id.unique_id)
                        {
                            let process_name = process_spec.process_name.clone();
                            let typed_init = self.check_node(worker_init)?;
                            return Ok((process_name, typed_init));
                        }
                    }
                }
            }
        }
        let Resolved::App(_, func, args) = worker_init else {
            return Err(self.process_policy_error(
                "supervisor spawn expects a worker init route reference",
                None,
                &span,
                Some("Use `MyWorker::init(args)`.".into()),
            ));
        };
        let Resolved::Var(_, id) = func.as_ref() else {
            return Err(self.process_policy_error(
                "supervisor spawn expects a worker init route reference",
                None,
                &span,
                Some("Use `MyWorker::init(args)`.".into()),
            ));
        };
        let qualified = id.qualified_name.as_deref().unwrap_or(&id.name);
        let Some((process_spec, init_handler)) = self.worker_process_spec_for_init_route(qualified)
        else {
            return Err(self.process_policy_error(
                "supervisor spawn expects a worker init route reference",
                None,
                &span,
                Some("Use `MyWorker::init(args)`.".into()),
            ));
        };
        let process_name = process_spec.process_name.clone();
        let internal_name = init_handler.internal_name.clone();
        let init_uid = process_spec.init_uid;
        let synthetic = Resolved::Closure(
            span.clone(),
            Vec::new(),
            Vec::new(),
            Box::new(Resolved::App(
                span.clone(),
                Box::new(Resolved::Var(
                    span.clone(),
                    ResolvedId {
                        name: internal_name.clone(),
                        qualified_name: Some(format!("{}::{}", process_name, internal_name)),
                        symbol_info: None,
                        unique_id: init_uid,
                        compiler_generated: true,
                        span: span.clone(),
                    },
                )),
                args.clone(),
            )),
        );
        let typed_init = self.check_node(&synthetic)?;
        Ok((process_name, typed_init))
    }

    pub(super) fn check_process_context_handler(
        &mut self,
        span: &Span,
        slot: &str,
    ) -> Result<TypedNode, TypeError> {
        let Some(process_name) = self.current_process_name() else {
            return Err(self.policy_error(
                TypeDiagnosticReason::ProcessHandlerScope,
                diagnostics::TypePolicy::ProcessHandlerScope,
                Some("ctx.<slot> is only available inside process handlers".into()),
                None,
                None,
                None,
                None,
                span,
                Some("Use ctx.<slot> inside @init/@get/@set/@call/@cast bodies.".into()),
            ));
        };
        let Some(capability) = self
            .process_handler_dependencies
            .get(&process_name)
            .and_then(|slots| slots.get(slot))
            .cloned()
        else {
            return Err(self.policy_error(
                TypeDiagnosticReason::ProcessHandlerScope,
                diagnostics::TypePolicy::ProcessHandlerScope,
                Some(format!(
                    "handler slot `{}` is not declared for process `{}`",
                    slot,
                    Self::surface_name(&process_name)
                )),
                None,
                None,
                None,
                None,
                span,
                Some("Declare the slot in meta.handlers before using ctx.<slot>.".into()),
            ));
        };
        Ok(TypedNode {
            ty: Ty::pid(Self::canonical_user_type_name(&capability)),
            span: span.clone(),
            node: TypedInner::ProcessContextHandler {
                process_name,
                slot: slot.to_string(),
            },
        })
    }
}
