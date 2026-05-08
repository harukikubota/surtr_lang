use std::collections::{HashMap, HashSet};

use scar::typed::*;
use scar::types::Ty;
use sigil::resolved::ResolvedId;
use sindr::builtin::builtin_id_by_name;
use sindr::ir::{
    BootEntrySource, CompileInfo, DbgArgTemplate, DbgTemplate, DocEntry, FunctionFlags,
    RuntimeBootPlan, RuntimeCallableRef, RuntimeHandlerArg, RuntimeHandlerDependency,
    RuntimeHandlerKind, RuntimeHandlerOverride, RuntimeHandlerSpec, RuntimeHandlerTarget,
    RuntimeInitPolicy, RuntimeInitResultShape, RuntimeInitSpec, RuntimeLifecycleSpec,
    RuntimeProcessDependencies, RuntimeStateSpec, RuntimeSupervisionSpec,
    RuntimeSupervisorOverrideEntry, RuntimeSupervisorPolicy, RuntimeTypeRef, SingletonBootEntry,
};
use sindr::primitives::int;
use spire::ast::{
    BinOp, Lit, ProcessInstance, ProcessRuntimeHandlerKind, Span, SupervisorInitSpec, Visibility,
};

use crate::bytecode::*;
use crate::error::CodegenError;
use crate::opcode::Opcode;
use crate::registry::{TypeEntry, TypeKind, TypeRegistry};

/// Lower the typed AST to bytecode.
pub fn codegen(typed: Vec<TypedNode>) -> Result<Bytecode, CodegenError> {
    codegen_typed_program(TypedProgram {
        nodes: typed,
        process_specs: Vec::new(),
        boot_plan: SupervisorInitSpec::default(),
    })
}

/// Lower a typed program, including runtime process metadata, to bytecode.
pub fn codegen_typed_program(typed: TypedProgram) -> Result<Bytecode, CodegenError> {
    let TypedProgram {
        nodes,
        process_specs,
        boot_plan,
    } = typed;
    let mut gene = Codegen::new();
    gene.emit_program(nodes.clone())?;
    let (opcodes, state) = gene.finalize()?;
    let runtime_process_specs =
        build_runtime_process_specs(&process_specs, &nodes, &state.functions)?;
    let runtime_boot_plan = build_runtime_boot_plan(&boot_plan, &process_specs)?;
    validate_required_singletons(&nodes, &process_specs, &runtime_boot_plan)?;
    Ok(Bytecode {
        opcodes,
        constants: state.constants,
        num_locals: state.next_slot as usize,
        type_registry: state.type_registry,
        error_templates: state.error_templates,
        dbg_templates: state.dbg_templates,
        functions: state.functions,
        source_map: None,
        docs: Vec::new(),
        compile_info: CompileInfo::default(),
        labels: Vec::new(),
        imports: Vec::new(),
        exports: Vec::new(),
        literals: Vec::new(),
        lines: Vec::new(),
        spans: Vec::new(),
        sources: Vec::new(),
        pc_spans: Vec::new(),
        runtime_process_specs,
        runtime_boot_plan,
    })
}

fn validate_required_singletons(
    nodes: &[TypedNode],
    process_specs: &[TypedProcessSpec],
    runtime_boot_plan: &RuntimeBootPlan,
) -> Result<(), CodegenError> {
    let mut surface_to_process = HashMap::new();
    for spec in process_specs {
        if spec.spec.instance != ProcessInstance::Singleton {
            continue;
        }
        surface_to_process.insert(
            format!("{}::pid", spec.process_name),
            spec.process_name.clone(),
        );
        for handler in &spec.spec.handler_specs {
            if handler.kind == ProcessRuntimeHandlerKind::Init {
                continue;
            }
            surface_to_process.insert(
                format!("{}::{}", spec.process_name, handler.name),
                spec.process_name.clone(),
            );
        }
    }

    if surface_to_process.is_empty() {
        return Ok(());
    }

    let available_singletons = runtime_boot_plan
        .singletons
        .iter()
        .map(|entry| entry.process_name.as_str())
        .collect::<HashSet<_>>();
    let available_supervisors = runtime_boot_plan
        .supervisor_overrides
        .iter()
        .map(|entry| entry.process_name.as_str())
        .collect::<HashSet<_>>();
    let mut first_missing: HashMap<String, Span> = HashMap::new();
    for node in nodes {
        collect_missing_singleton_calls(
            node,
            &surface_to_process,
            &available_singletons,
            &available_supervisors,
            &mut first_missing,
        );
    }

    if let Some((process_name, span)) = first_missing.into_iter().min_by(|left, right| {
        left.1
            .start
            .cmp(&right.1.start)
            .then_with(|| left.0.cmp(&right.0))
    }) {
        return Err(CodegenError {
            message: format!(
                "singleton `{process_name}` is not available in this compile unit; add it to supervisor_init"
            ),
            span,
        });
    }

    Ok(())
}

fn collect_missing_singleton_calls(
    node: &TypedNode,
    surface_to_process: &HashMap<String, String>,
    available_singletons: &HashSet<&str>,
    available_supervisors: &HashSet<&str>,
    first_missing: &mut HashMap<String, Span>,
) {
    if let Some(process_name) = singleton_required_by_call(node, surface_to_process) {
        if !available_singletons.contains(process_name.as_str()) {
            first_missing
                .entry(process_name)
                .or_insert_with(|| node.span.clone());
        }
    }

    match &node.node {
        TypedInner::Lit(_)
        | TypedInner::Var(_)
        | TypedInner::ListNil
        | TypedInner::ProcessContextHandler { .. }
        | TypedInner::LensPath(_)
        | TypedInner::PendingLensPath(_)
        | TypedInner::EnumDef(_, _)
        | TypedInner::TraitDef(_, _)
        | TypedInner::TraitImplDef(_, _)
        | TypedInner::BuiltinExtractorDecl(_, _, _)
        | TypedInner::StructDef(_, _, _, _, _)
        | TypedInner::RecordDef(_, _, _, _, _) => {}
        TypedInner::SupervisorSpawn {
            supervisor_process,
            init,
            ..
        } => {
            if !available_supervisors.contains(supervisor_process.as_str()) {
                first_missing
                    .entry(format!("{}::spawn", supervisor_process))
                    .or_insert_with(|| node.span.clone());
            }
            collect_missing_singleton_calls(
                init,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
        }
        TypedInner::SupervisorAdopt {
            supervisor_process,
            pid,
            ..
        } => {
            if !available_supervisors.contains(supervisor_process.as_str()) {
                first_missing
                    .entry(format!("{}::adopt", supervisor_process))
                    .or_insert_with(|| node.span.clone());
            }
            collect_missing_singleton_calls(
                pid,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
        }
        TypedInner::SupervisorStatus { supervisor_process } => {
            if !available_supervisors.contains(supervisor_process.as_str()) {
                first_missing
                    .entry(format!("{}::status", supervisor_process))
                    .or_insert_with(|| node.span.clone());
            }
        }
        TypedInner::SupervisorWorkers {
            supervisor_process,
            init,
            size,
            ..
        } => {
            if !available_supervisors.contains(supervisor_process.as_str()) {
                first_missing
                    .entry(format!("{}::workers", supervisor_process))
                    .or_insert_with(|| node.span.clone());
            }
            collect_missing_singleton_calls(
                init,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
            collect_missing_singleton_calls(
                size,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
        }
        TypedInner::App(func, args)
        | TypedInner::InjectCall(func, args)
        | TypedInner::Capture(func, args) => {
            collect_missing_singleton_calls(
                func,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
            for arg in args {
                collect_missing_singleton_calls(
                    arg,
                    surface_to_process,
                    available_singletons,
                    available_supervisors,
                    first_missing,
                );
            }
        }
        TypedInner::TraitCall { args, .. }
        | TypedInner::ListLiteral(args)
        | TypedInner::TupleLiteral(args)
        | TypedInner::StructLit(_, args)
        | TypedInner::ConstructorCall(_, args)
        | TypedInner::Block(args) => {
            for arg in args {
                collect_missing_singleton_calls(
                    arg,
                    surface_to_process,
                    available_singletons,
                    available_supervisors,
                    first_missing,
                );
            }
        }
        TypedInner::Bind(_, rhs)
        | TypedInner::SafeBind(_, rhs)
        | TypedInner::FieldAccess(rhs, _)
        | TypedInner::Semi(rhs) => collect_missing_singleton_calls(
            rhs,
            surface_to_process,
            available_singletons,
            available_supervisors,
            first_missing,
        ),
        TypedInner::BinOp(_, left, right)
        | TypedInner::Pipe(left, right)
        | TypedInner::Compose(_, left, right)
        | TypedInner::ListCons(left, right)
        | TypedInner::MapErr(left, right)
        | TypedInner::Cause(left, right) => {
            collect_missing_singleton_calls(
                left,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
            collect_missing_singleton_calls(
                right,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
        }
        TypedInner::InterpolatedStr(parts) => {
            for part in parts {
                if let scar::typed::TypedInterpolatedPart::Expr(expr) = part {
                    collect_missing_singleton_calls(
                        expr,
                        surface_to_process,
                        available_singletons,
                        available_supervisors,
                        first_missing,
                    );
                }
            }
        }
        TypedInner::Dbg(args) => {
            for arg in args {
                collect_missing_singleton_calls(
                    &arg.expr,
                    surface_to_process,
                    available_singletons,
                    available_supervisors,
                    first_missing,
                );
            }
        }
        TypedInner::If(cond, then_node, else_node) => {
            collect_missing_singleton_calls(
                cond,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
            collect_missing_singleton_calls(
                then_node,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
            if let Some(else_node) = else_node {
                collect_missing_singleton_calls(
                    else_node,
                    surface_to_process,
                    available_singletons,
                    available_supervisors,
                    first_missing,
                );
            }
        }
        TypedInner::Assert(left, right)
        | TypedInner::Ensure(left, right, _)
        | TypedInner::RecoverKind(left, right, _) => {
            collect_missing_singleton_calls(
                left,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
            collect_missing_singleton_calls(
                right,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
            match &node.node {
                TypedInner::Ensure(_, _, third) | TypedInner::RecoverKind(_, _, third) => {
                    collect_missing_singleton_calls(
                        third,
                        surface_to_process,
                        available_singletons,
                        available_supervisors,
                        first_missing,
                    );
                }
                _ => {}
            }
        }
        TypedInner::Match(scrutinee, arms) => {
            collect_missing_singleton_calls(
                scrutinee,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_missing_singleton_calls(
                        guard,
                        surface_to_process,
                        available_singletons,
                        available_supervisors,
                        first_missing,
                    );
                }
                collect_missing_singleton_calls(
                    &arm.body,
                    surface_to_process,
                    available_singletons,
                    available_supervisors,
                    first_missing,
                );
            }
        }
        TypedInner::LensView { source, .. } => collect_missing_singleton_calls(
            source,
            surface_to_process,
            available_singletons,
            available_supervisors,
            first_missing,
        ),
        TypedInner::LensSet { source, value, .. } => {
            collect_missing_singleton_calls(
                source,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
            collect_missing_singleton_calls(
                value,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
        }
        TypedInner::LensOver {
            source, update_fun, ..
        } => {
            collect_missing_singleton_calls(
                source,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
            collect_missing_singleton_calls(
                update_fun,
                surface_to_process,
                available_singletons,
                available_supervisors,
                first_missing,
            );
        }
        TypedInner::DeferrorDef(_, _, _, _, body)
        | TypedInner::Closure(_, _, body)
        | TypedInner::Def(_, _, _, _, _, body, _)
        | TypedInner::ExtractorDef(_, _, _, _, _, body, _) => collect_missing_singleton_calls(
            body,
            surface_to_process,
            available_singletons,
            available_supervisors,
            first_missing,
        ),
    }
}

fn singleton_required_by_call(
    node: &TypedNode,
    surface_to_process: &HashMap<String, String>,
) -> Option<String> {
    let func = match &node.node {
        TypedInner::App(func, _)
        | TypedInner::InjectCall(func, _)
        | TypedInner::Capture(func, _) => func,
        _ => return None,
    };
    let TypedInner::Var(id) = &func.node else {
        return None;
    };
    id.qualified_name
        .as_ref()
        .and_then(|name| surface_to_process.get(name))
        .or_else(|| surface_to_process.get(&id.name))
        .cloned()
}

/// Compose a complete executable bytecode artifact from a precompiled prefix
/// and a chunk produced by `ForgeSession::codegen_chunk`.
///
/// REPL chunks are normally appended and executed from their appended base PC.
/// A `.eldr` artifact, however, starts at PC 0, so the chunk top-level opcodes
/// must be inserted before the prefix `Halt` while function bodies remain after
/// the single top-level halt.
pub fn compose_bytecode_with_chunk(
    mut base: Bytecode,
    chunk: BytecodeChunk,
) -> Result<Bytecode, CodegenError> {
    let base_halt = base
        .opcodes
        .iter()
        .position(|op| matches!(op, Opcode::Halt))
        .ok_or_else(|| CodegenError {
            message: "precompiled bytecode has no top-level Halt".into(),
            span: Span { start: 0, end: 0 },
        })?;
    let chunk_halt = chunk
        .opcodes
        .iter()
        .position(|op| matches!(op, Opcode::Halt))
        .ok_or_else(|| CodegenError {
            message: "compiled chunk has no top-level Halt".into(),
            span: Span { start: 0, end: 0 },
        })?;

    let const_base = base.constants.len();
    let error_template_base = base.error_templates.len();
    let dbg_template_base = base.dbg_templates.len();
    if chunk.const_base as usize != const_base {
        return Err(CodegenError {
            message: format!(
                "chunk constant base mismatch: chunk={}, base={}",
                chunk.const_base, const_base
            ),
            span: Span { start: 0, end: 0 },
        });
    }
    if chunk.error_template_base as usize != error_template_base {
        return Err(CodegenError {
            message: format!(
                "chunk error template base mismatch: chunk={}, base={}",
                chunk.error_template_base, error_template_base
            ),
            span: Span { start: 0, end: 0 },
        });
    }
    if chunk.dbg_template_base as usize != dbg_template_base {
        return Err(CodegenError {
            message: format!(
                "chunk dbg template base mismatch: chunk={}, base={}",
                chunk.dbg_template_base, dbg_template_base
            ),
            span: Span { start: 0, end: 0 },
        });
    }

    let base_top_len = base_halt;
    let chunk_top_len = chunk_halt;
    let base_func_len = base.opcodes.len().saturating_sub(base_halt + 1);
    let final_halt = base_top_len + chunk_top_len;
    let base_func_base = final_halt + 1;
    let chunk_func_base = base_func_base + base_func_len;

    let mut base_ops = base.opcodes;
    relocate_base_ops_for_insert(&mut base_ops, base_halt, chunk_top_len)?;

    let mut chunk_ops = chunk.opcodes;
    relocate_chunk_ops_for_artifact(
        &mut chunk_ops,
        chunk_halt,
        base_top_len,
        chunk_func_base,
        const_base,
        error_template_base,
        dbg_template_base,
    )?;

    let mut opcodes = Vec::with_capacity(base_ops.len() + chunk_ops.len().saturating_sub(1));
    opcodes.extend_from_slice(&base_ops[..base_halt]);
    opcodes.extend_from_slice(&chunk_ops[..chunk_halt]);
    opcodes.push(Opcode::Halt);
    opcodes.extend_from_slice(&base_ops[base_halt + 1..]);
    opcodes.extend_from_slice(&chunk_ops[chunk_halt + 1..]);

    for entry in &mut base.functions {
        relocate_function_entry(entry, base_halt, chunk_top_len)?;
    }

    let mut functions = base.functions;
    for mut entry in chunk.functions {
        let mapped_entry = map_chunk_pc(entry.entry_pc, chunk_halt, base_top_len, chunk_func_base)?;
        entry.entry_pc = mapped_entry;
        if entry.end_pc != 0 {
            entry.end_pc = map_chunk_pc(entry.end_pc, chunk_halt, base_top_len, chunk_func_base)?;
        }
        let idx = entry.fun_idx as usize;
        if idx == functions.len() {
            functions.push(entry);
        } else if idx < functions.len() {
            functions[idx] = entry;
        } else {
            return Err(CodegenError {
                message: format!(
                    "function table invariant violated in chunk: fun_idx {} > len {}",
                    idx,
                    functions.len()
                ),
                span: Span { start: 0, end: 0 },
            });
        }
    }

    base.opcodes = opcodes;
    base.constants.extend(chunk.constants);
    base.type_registry.entries.extend(chunk.type_entries);
    base.error_templates.extend(chunk.error_templates);
    base.dbg_templates.extend(chunk.dbg_templates);
    base.num_locals = base.num_locals.saturating_add(chunk.new_locals);
    base.functions = functions;
    base.source_map = None;
    extend_docs_unique(&mut base.docs, chunk.docs);
    base.runtime_process_specs
        .entries
        .extend(chunk.runtime_process_specs);
    extend_runtime_boot_plan(&mut base.runtime_boot_plan, chunk.runtime_boot_plan);
    Ok(base)
}

fn extend_runtime_boot_plan(base: &mut RuntimeBootPlan, chunk: RuntimeBootPlan) {
    base.singletons.extend(chunk.singletons);
    base.standard_overrides.extend(chunk.standard_overrides);
    base.handler_overrides.extend(chunk.handler_overrides);
}

fn build_runtime_boot_plan(
    boot_plan: &SupervisorInitSpec,
    process_specs: &[TypedProcessSpec],
) -> Result<RuntimeBootPlan, CodegenError> {
    let mut runtime = RuntimeBootPlan::default();
    let default_timeout_ms = runtime.runtime_limits.default_init_timeout_ms;

    for singleton in &boot_plan.singletons {
        let Some(spec) = resolve_boot_process_spec(process_specs, &singleton.process_name) else {
            return Err(CodegenError {
                message: "singleton process is not defined or not visible".into(),
                span: singleton.span.clone(),
            });
        };
        if spec.spec.instance != ProcessInstance::Singleton {
            return Err(CodegenError {
                message: "only Singleton process can appear in singleton boot entry".into(),
                span: singleton.span.clone(),
            });
        }
        if runtime
            .singletons
            .iter()
            .any(|entry| entry.process_name == spec.process_name)
        {
            return Err(CodegenError {
                message: "singleton boot entry is duplicated".into(),
                span: singleton.span.clone(),
            });
        }

        runtime.singletons.push(SingletonBootEntry {
            process_name: spec.process_name.clone(),
            init_timeout_ms: singleton.timeout_ms.unwrap_or(default_timeout_ms),
            source: BootEntrySource::ExplicitConfig,
        });
        for handler in &singleton.handlers {
            let Some(dependency) = spec
                .spec
                .handlers
                .iter()
                .find(|dependency| dependency.slot == handler.slot)
            else {
                return Err(CodegenError {
                    message: "handler slot is not declared by the target process".into(),
                    span: handler.span.clone(),
                });
            };
            validate_runtime_handler_target(dependency, &handler.target)?;
            runtime.handler_overrides.push(RuntimeHandlerOverride {
                target_process: spec.process_name.clone(),
                slot: handler.slot.clone(),
                handler_target: RuntimeHandlerTarget {
                    name: handler.target.name.clone(),
                    named_args: handler
                        .target
                        .named_args
                        .iter()
                        .map(|arg| RuntimeHandlerArg {
                            name: arg.name.clone(),
                            value: arg.value.clone(),
                        })
                        .collect(),
                },
            });
        }
    }

    for supervisor in &boot_plan.supervisors {
        let Some(spec) = resolve_boot_process_spec(process_specs, &supervisor.process_name) else {
            return Err(CodegenError {
                message: "supervisor process is not defined or not visible".into(),
                span: supervisor.span.clone(),
            });
        };
        if !matches!(
            spec.spec.kind,
            spire::ast::ProcessKind::Supervisor
                | spire::ast::ProcessKind::DynamicSupervisor
                | spire::ast::ProcessKind::RuntimeSupervisor
        ) {
            return Err(CodegenError {
                message: "supervisor override target must be a supervisor process".into(),
                span: supervisor.span.clone(),
            });
        }
        let Some(base_policy) = &spec.spec.supervisor_policy else {
            return Err(CodegenError {
                message: "supervisor process is missing a policy definition".into(),
                span: supervisor.span.clone(),
            });
        };
        runtime
            .supervisor_overrides
            .push(RuntimeSupervisorOverrideEntry {
                process_name: spec.process_name.clone(),
                policy: runtime_supervisor_policy_from_effective(
                    base_policy,
                    &supervisor.overrides,
                ),
            });
    }

    Ok(runtime)
}

fn resolve_boot_process_spec<'a>(
    process_specs: &'a [TypedProcessSpec],
    requested_name: &str,
) -> Option<&'a TypedProcessSpec> {
    process_specs
        .iter()
        .find(|spec| spec.process_name == requested_name)
        .or_else(|| {
            process_specs.iter().find(|spec| {
                spec.process_name
                    .rsplit("::")
                    .next()
                    .is_some_and(|short| short == requested_name)
            })
        })
}

fn runtime_supervisor_policy_from_effective(
    base: &spire::ast::SupervisorPolicy,
    overrides: &spire::ast::SupervisorPolicyOverride,
) -> RuntimeSupervisorPolicy {
    let effective_strategy = overrides.strategy.unwrap_or(base.strategy);
    let effective_restart_default = overrides
        .child_restart_default
        .unwrap_or(base.child_restart_default);
    RuntimeSupervisorPolicy {
        strategy: match effective_strategy {
            spire::ast::SupervisorStrategy::OneForOne => "OneForOne".into(),
        },
        max_restarts: overrides.max_restarts.unwrap_or(base.max_restarts),
        max_seconds: overrides.max_seconds.unwrap_or(base.max_seconds),
        child_restart_default: match effective_restart_default {
            spire::ast::ChildRestartPolicy::Permanent => "Permanent".into(),
            spire::ast::ChildRestartPolicy::Transient => "Transient".into(),
            spire::ast::ChildRestartPolicy::Temporary => "Temporary".into(),
        },
        allow_adopt: overrides.allow_adopt.unwrap_or(base.allow_adopt),
        shutdown_timeout_ms: overrides.shutdown_timeout_ms.or(base.shutdown_timeout_ms),
    }
}

fn validate_runtime_handler_target(
    dependency: &spire::ast::ProcessHandlerDependency,
    target: &spire::ast::SupervisorInitHandlerTarget,
) -> Result<(), CodegenError> {
    match dependency.capability.as_str() {
        "OutHandler" => match target.name.as_str() {
            "StdOut" | "StdErr" | "NullOutHandler" => {
                if !target.named_args.is_empty() {
                    return Err(CodegenError {
                        message: format!("{} does not accept handler arguments", target.name),
                        span: target.span.clone(),
                    });
                }
            }
            "FileOutHandler" => {
                let has_path = target.named_args.iter().any(|arg| arg.name == "path");
                if !has_path {
                    return Err(CodegenError {
                        message: "FileOutHandler requires named argument `path`".into(),
                        span: target.span.clone(),
                    });
                }
                if target.named_args.iter().any(|arg| arg.name != "path") {
                    return Err(CodegenError {
                        message: "FileOutHandler only accepts named argument `path`".into(),
                        span: target.span.clone(),
                    });
                }
            }
            _ => {
                return Err(CodegenError {
                    message: format!(
                        "handler target `{}` does not satisfy capability OutHandler",
                        target.name
                    ),
                    span: target.span.clone(),
                });
            }
        },
        capability => {
            return Err(CodegenError {
                message: format!(
                    "handler capability `{capability}` is not supported by supervisor_init override validation"
                ),
                span: dependency.span.clone(),
            });
        }
    }
    Ok(())
}

fn typed_def_return_ty(nodes: &[TypedNode], uid: u32) -> Option<&Ty> {
    nodes.iter().find_map(|node| match &node.node {
        TypedInner::Def(_, id, _, _, ret_ty, _, _) if id.unique_id == uid => Some(ret_ty),
        _ => None,
    })
}

fn init_state_ty(ret_ty: &Ty, lazy: bool, process_name: &str) -> Result<Ty, CodegenError> {
    let ok_ty = match ret_ty {
        Ty::Result(ok, _) => ok.as_ref(),
        other => other,
    };
    if !lazy {
        return Ok(ok_ty.clone());
    }
    match ok_ty {
        Ty::Enum(name, args) if name == "ProcessInit" || name.ends_with("::ProcessInit") => {
            args.first().cloned().ok_or_else(|| CodegenError {
                message: format!(
                    "Lazy @init for process `{process_name}` must return Result<ProcessInit<State>>"
                ),
                span: Span { start: 0, end: 0 },
            })
        }
        _ => Err(CodegenError {
            message: format!(
                "Lazy @init for process `{process_name}` must return Result<ProcessInit<State>>"
            ),
            span: Span { start: 0, end: 0 },
        }),
    }
}

fn runtime_type_ref(ty: &Ty) -> RuntimeTypeRef {
    RuntimeTypeRef {
        name: ty_to_string(ty),
    }
}

fn build_runtime_process_specs(
    process_specs: &[TypedProcessSpec],
    nodes: &[TypedNode],
    functions: &[FunctionEntry],
) -> Result<RuntimeProcessSpecTable, CodegenError> {
    let qualified_names = nodes
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Def(_, id, _, _, _, _, _)
            | TypedInner::ExtractorDef(_, id, _, _, _, _, _) => Some((
                id.unique_id,
                id.qualified_name.clone().unwrap_or_else(|| id.name.clone()),
            )),
            _ => None,
        })
        .collect::<HashMap<_, _>>();

    let function_ids = functions
        .iter()
        .filter_map(|entry| {
            entry
                .qualified_name
                .as_ref()
                .map(|name| (name.clone(), entry.fun_idx))
        })
        .collect::<HashMap<_, _>>();
    let function_entries = functions
        .iter()
        .filter_map(|entry| {
            entry
                .qualified_name
                .as_ref()
                .map(|name| (name.clone(), entry.clone()))
        })
        .collect::<HashMap<_, _>>();

    let mut entries = Vec::with_capacity(process_specs.len());
    for (process_id, spec) in process_specs.iter().enumerate() {
        let process_kind = match spec.spec.kind {
            spire::ast::ProcessKind::Agent => RuntimeProcessKind::Agent,
            spire::ast::ProcessKind::GenServer => RuntimeProcessKind::GenServer,
            spire::ast::ProcessKind::Supervisor => RuntimeProcessKind::Supervisor,
            spire::ast::ProcessKind::RuntimeSupervisor => RuntimeProcessKind::RuntimeSupervisor,
            spire::ast::ProcessKind::DynamicSupervisor => RuntimeProcessKind::DynamicSupervisor,
            spire::ast::ProcessKind::Task => RuntimeProcessKind::Task,
        };
        let init_name = qualified_names
            .get(&spec.init_uid)
            .ok_or_else(|| CodegenError {
                message: format!(
                    "missing lowered init handler metadata for process `{}`",
                    spec.process_name
                ),
                span: Span { start: 0, end: 0 },
            })?;
        let get_name = qualified_names
            .get(&spec.get_uid)
            .ok_or_else(|| CodegenError {
                message: format!(
                    "missing lowered get handler metadata for process `{}`",
                    spec.process_name
                ),
                span: Span { start: 0, end: 0 },
            })?;
        let _set_fun_idx = spec
            .set_uid
            .map(|uid| {
                let set_name = qualified_names.get(&uid).ok_or_else(|| CodegenError {
                    message: format!(
                        "missing lowered set handler metadata for process `{}`",
                        spec.process_name
                    ),
                    span: Span { start: 0, end: 0 },
                })?;
                function_ids
                    .get(set_name)
                    .copied()
                    .ok_or_else(|| CodegenError {
                        message: format!(
                            "missing bytecode set handler for process `{}`",
                            spec.process_name
                        ),
                        span: Span { start: 0, end: 0 },
                    })
            })
            .transpose()?;
        let mut handler_specs = Vec::new();
        let init_entry = function_entries
            .get(init_name)
            .ok_or_else(|| CodegenError {
                message: format!(
                    "missing bytecode init handler for process `{}`",
                    spec.process_name
                ),
                span: Span { start: 0, end: 0 },
            })?;
        let get_entry = function_entries.get(get_name).ok_or_else(|| CodegenError {
            message: format!(
                "missing bytecode get handler for process `{}`",
                spec.process_name
            ),
            span: Span { start: 0, end: 0 },
        })?;
        let mut set_entry = None;
        if let Some(set_uid) = spec.set_uid {
            let set_name = qualified_names.get(&set_uid).ok_or_else(|| CodegenError {
                message: format!(
                    "missing lowered set handler metadata for process `{}`",
                    spec.process_name
                ),
                span: Span { start: 0, end: 0 },
            })?;
            set_entry = Some(function_entries.get(set_name).ok_or_else(|| CodegenError {
                message: format!(
                    "missing bytecode set handler for process `{}`",
                    spec.process_name
                ),
                span: Span { start: 0, end: 0 },
            })?);
        }
        if spec.spec.handler_specs.is_empty() {
            handler_specs.push(RuntimeHandlerSpec {
                handler_id: 0,
                name: init_name.clone(),
                kind: RuntimeHandlerKind::Init,
                fun_idx: init_entry.fun_idx,
                arity: init_entry.arity,
            });
            handler_specs.push(RuntimeHandlerSpec {
                handler_id: 1,
                name: get_name.clone(),
                kind: if process_kind == RuntimeProcessKind::GenServer {
                    RuntimeHandlerKind::Call
                } else {
                    RuntimeHandlerKind::Get
                },
                fun_idx: get_entry.fun_idx,
                arity: get_entry.arity,
            });
            if let Some(set_entry) = set_entry {
                handler_specs.push(RuntimeHandlerSpec {
                    handler_id: 2,
                    name: qualified_names
                        .get(&spec.set_uid.expect("set entry exists"))
                        .cloned()
                        .unwrap_or_default(),
                    kind: if process_kind == RuntimeProcessKind::GenServer {
                        RuntimeHandlerKind::Cast
                    } else {
                        RuntimeHandlerKind::Set
                    },
                    fun_idx: set_entry.fun_idx,
                    arity: set_entry.arity,
                });
            }
        } else {
            for (handler_id, handler) in spec.spec.handler_specs.iter().enumerate() {
                let internal_name = if handler.internal_name.is_empty() {
                    None
                } else {
                    Some(format!("{}::{}", spec.module_path, handler.internal_name))
                };
                let entry_for_internal = internal_name
                    .as_ref()
                    .and_then(|name| function_entries.get(name));
                let (kind, entry) = match handler.kind {
                    ProcessRuntimeHandlerKind::Init => (
                        RuntimeHandlerKind::Init,
                        entry_for_internal.unwrap_or(init_entry),
                    ),
                    ProcessRuntimeHandlerKind::Get => (
                        RuntimeHandlerKind::Get,
                        entry_for_internal.unwrap_or(get_entry),
                    ),
                    ProcessRuntimeHandlerKind::Set => {
                        let Some(entry) = entry_for_internal.or(set_entry) else {
                            return Err(CodegenError {
                                message: format!(
                                    "handler metadata references @set for process `{}` but no set handler was lowered",
                                    spec.process_name
                                ),
                                span: handler.span.clone(),
                            });
                        };
                        (RuntimeHandlerKind::Set, entry)
                    }
                    ProcessRuntimeHandlerKind::Call => (
                        RuntimeHandlerKind::Call,
                        entry_for_internal.unwrap_or(get_entry),
                    ),
                    ProcessRuntimeHandlerKind::Cast => {
                        let Some(entry) = entry_for_internal.or(set_entry) else {
                            return Err(CodegenError {
                                message: format!(
                                    "handler metadata references @cast for process `{}` but no cast handler was lowered",
                                    spec.process_name
                                ),
                                span: handler.span.clone(),
                            });
                        };
                        (RuntimeHandlerKind::Cast, entry)
                    }
                };
                handler_specs.push(RuntimeHandlerSpec {
                    handler_id: handler_id as u32,
                    name: handler.name.clone(),
                    kind,
                    fun_idx: entry.fun_idx,
                    arity: entry.arity,
                });
            }
        }

        let init_ret_ty =
            typed_def_return_ty(nodes, spec.init_uid).ok_or_else(|| CodegenError {
                message: format!(
                    "missing typed init handler return type for process `{}`",
                    spec.process_name
                ),
                span: Span { start: 0, end: 0 },
            })?;
        let state_ty = init_state_ty(init_ret_ty, spec.spec.lazy, &spec.process_name)?;
        let state_type = runtime_type_ref(&state_ty);
        let result_type = runtime_type_ref(init_ret_ty);
        let init_policy = if spec.spec.lazy {
            RuntimeInitPolicy::Lazy
        } else {
            RuntimeInitPolicy::Eager
        };
        let result_shape = if spec.spec.lazy {
            RuntimeInitResultShape::LazyProcessInit {
                result_type: result_type.clone(),
            }
        } else {
            RuntimeInitResultShape::EagerState {
                result_type: result_type.clone(),
            }
        };

        entries.push(RuntimeProcessSpec {
            process_id: process_id as u32,
            type_name: spec.process_name.clone(),
            kind: process_kind,
            instance: match spec.spec.instance {
                spire::ast::ProcessInstance::Singleton => RuntimeProcessInstance::Singleton,
                spire::ast::ProcessInstance::Worker => RuntimeProcessInstance::Worker,
            },
            state: RuntimeStateSpec {
                state_type: state_type.clone(),
                owner_process: Some(spec.process_name.clone()),
            },
            init: RuntimeInitSpec {
                callable: RuntimeCallableRef {
                    fun_idx: init_entry.fun_idx,
                },
                policy: init_policy,
                result_shape,
                state_type,
                init_route: None,
            },
            handlers: handler_specs,
            dependencies: RuntimeProcessDependencies {
                handlers: spec
                    .spec
                    .handlers
                    .iter()
                    .map(|handler| RuntimeHandlerDependency {
                        slot: handler.slot.clone(),
                        capability: handler.capability.clone(),
                        default_target: RuntimeHandlerTarget {
                            name: handler.default_target.name.clone(),
                            named_args: Vec::new(),
                        },
                    })
                    .collect(),
            },
            lifecycle: RuntimeLifecycleSpec::default(),
            supervision: RuntimeSupervisionSpec {
                parent: match spec.spec.kind {
                    spire::ast::ProcessKind::Supervisor
                    | spire::ast::ProcessKind::DynamicSupervisor
                    | spire::ast::ProcessKind::RuntimeSupervisor => Some("RootSupervisor".into()),
                    _ if spec.spec.instance == ProcessInstance::Singleton => {
                        Some("RuntimeSupervisor".into())
                    }
                    _ => None,
                },
                children: Vec::new(),
                policy: spec.spec.supervisor_policy.as_ref().map(|policy| {
                    runtime_supervisor_policy_from_effective(
                        policy,
                        &spire::ast::SupervisorPolicyOverride::default(),
                    )
                }),
            },
        });
    }

    Ok(RuntimeProcessSpecTable { entries })
}

fn relocate_base_ops_for_insert(
    opcodes: &mut [Opcode],
    insertion_pc: usize,
    inserted_len: usize,
) -> Result<(), CodegenError> {
    for op in opcodes {
        match op {
            Opcode::Jump(addr) | Opcode::JumpIfFalse(addr) | Opcode::JumpIfTrue(addr)
                if *addr as usize >= insertion_pc =>
            {
                *addr = add_u32(*addr, inserted_len, "base jump relocation")?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn relocate_function_entry(
    entry: &mut FunctionEntry,
    insertion_pc: usize,
    inserted_len: usize,
) -> Result<(), CodegenError> {
    if entry.entry_pc as usize > insertion_pc {
        entry.entry_pc = add_u32(
            entry.entry_pc,
            inserted_len,
            "base function entry relocation",
        )?;
    }
    if entry.end_pc as usize > insertion_pc {
        entry.end_pc = add_u32(entry.end_pc, inserted_len, "base function end relocation")?;
    }
    Ok(())
}

fn relocate_chunk_ops_for_artifact(
    opcodes: &mut [Opcode],
    chunk_halt: usize,
    base_top_len: usize,
    chunk_func_base: usize,
    const_base: usize,
    error_template_base: usize,
    dbg_template_base: usize,
) -> Result<(), CodegenError> {
    let const_base = u32::try_from(const_base).map_err(|_| CodegenError {
        message: "constant base exceeds u32".into(),
        span: Span { start: 0, end: 0 },
    })?;
    let error_template_base = u32::try_from(error_template_base).map_err(|_| CodegenError {
        message: "error template base exceeds u32".into(),
        span: Span { start: 0, end: 0 },
    })?;
    let dbg_template_base = u32::try_from(dbg_template_base).map_err(|_| CodegenError {
        message: "dbg template base exceeds u32".into(),
        span: Span { start: 0, end: 0 },
    })?;
    for op in opcodes {
        match op {
            Opcode::Jump(addr) | Opcode::JumpIfFalse(addr) | Opcode::JumpIfTrue(addr) => {
                *addr = map_chunk_pc(*addr, chunk_halt, base_top_len, chunk_func_base)?;
            }
            Opcode::LoadConst(idx) => {
                *idx = idx.checked_add(const_base).ok_or_else(|| CodegenError {
                    message: "chunk const relocation overflow".into(),
                    span: Span { start: 0, end: 0 },
                })?;
            }
            Opcode::MakeError { template_id } => {
                *template_id = template_id
                    .checked_add(error_template_base)
                    .ok_or_else(|| CodegenError {
                        message: "chunk error template relocation overflow".into(),
                        span: Span { start: 0, end: 0 },
                    })?;
            }
            Opcode::Dbg { template_id, .. } => {
                *template_id =
                    template_id
                        .checked_add(dbg_template_base)
                        .ok_or_else(|| CodegenError {
                            message: "chunk dbg template relocation overflow".into(),
                            span: Span { start: 0, end: 0 },
                        })?;
            }
            Opcode::MakeErrorLiteral {
                kind_const_idx,
                message_const_idx,
            } => {
                *kind_const_idx =
                    kind_const_idx
                        .checked_add(const_base)
                        .ok_or_else(|| CodegenError {
                            message: "chunk error literal kind relocation overflow".into(),
                            span: Span { start: 0, end: 0 },
                        })?;
                *message_const_idx =
                    message_const_idx
                        .checked_add(const_base)
                        .ok_or_else(|| CodegenError {
                            message: "chunk error literal message relocation overflow".into(),
                            span: Span { start: 0, end: 0 },
                        })?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn map_chunk_pc(
    pc: u32,
    chunk_halt: usize,
    base_top_len: usize,
    chunk_func_base: usize,
) -> Result<u32, CodegenError> {
    let pc = pc as usize;
    let mapped = if pc <= chunk_halt {
        base_top_len + pc
    } else {
        chunk_func_base + pc.saturating_sub(chunk_halt + 1)
    };
    u32::try_from(mapped).map_err(|_| CodegenError {
        message: "chunk pc relocation exceeds u32".into(),
        span: Span { start: 0, end: 0 },
    })
}

fn add_u32(value: u32, add: usize, label: &str) -> Result<u32, CodegenError> {
    let add = u32::try_from(add).map_err(|_| CodegenError {
        message: format!("{label} offset exceeds u32"),
        span: Span { start: 0, end: 0 },
    })?;
    value.checked_add(add).ok_or_else(|| CodegenError {
        message: format!("{label} overflow"),
        span: Span { start: 0, end: 0 },
    })
}

fn extend_docs_unique(docs: &mut Vec<DocEntry>, new_docs: Vec<DocEntry>) {
    for doc in new_docs {
        let exists = docs.iter().any(|existing| {
            existing.qualified_name == doc.qualified_name
                && existing.kind == doc.kind
                && existing.signature == doc.signature
        });
        if !exists {
            docs.push(doc);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplTypeKind {
    Struct,
    Record,
    Error,
    Enum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplCallableKind {
    Closure,
    Capture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingInfo {
    pub name: String,
    pub ty: String,
    pub slot_id: u32,
    pub callable_kind: Option<ReplCallableKind>,
    pub callable_display: Option<ReplCallableDisplay>,
    pub callable_captures: Vec<String>,
    pub lens_info: Option<ReplLensInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplCallableDisplay {
    FnCapture {
        module: String,
        name: String,
        sig: String,
    },
    Closure {
        sig: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplLensSegmentInfo {
    pub label: String,
    pub kind: String,
    pub source_ty: String,
    pub focus_ty: String,
    pub fallible: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplLensInfo {
    pub ty: String,
    pub view_result_ty: String,
    pub full_path: String,
    pub segments: Vec<ReplLensSegmentInfo>,
    pub stop_points: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDefDisplay {
    pub name: String,
    pub kind: ReplTypeKind,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkMeta {
    pub bindings: Vec<BindingInfo>,
    pub result_lens_info: Option<ReplLensInfo>,
    pub type_defs: Vec<TypeDefDisplay>,
    pub function_defs: Vec<String>,
    pub docs: Vec<DocEntry>,
}

#[derive(Debug, Clone)]
struct CodegenState {
    constants: Vec<Constant>,
    slot_map: HashMap<u32, u32>, // unique_id → local slot
    next_slot: u32,
    next_fun_idx: u32,
    type_registry: TypeRegistry,
    error_templates: Vec<ErrTemplate>,
    dbg_templates: Vec<DbgTemplate>,
    functions: Vec<FunctionEntry>,
    error_ctor_funs: HashMap<String, (u32, u8)>, // error kind -> (fun_idx, arity)
}

impl CodegenState {
    fn new() -> Self {
        Self {
            constants: Vec::new(),
            slot_map: HashMap::new(),
            next_slot: 0,
            next_fun_idx: 0,
            type_registry: TypeRegistry::new(),
            error_templates: Vec::new(),
            dbg_templates: Vec::new(),
            functions: Vec::new(),
            error_ctor_funs: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ForgeCheckpoint {
    state: CodegenState,
}

#[derive(Debug, Clone)]
pub struct ForgeSession {
    state: CodegenState,
}

impl ForgeSession {
    pub fn new() -> Self {
        Self {
            state: CodegenState::new(),
        }
    }

    pub fn checkpoint(&self) -> ForgeCheckpoint {
        ForgeCheckpoint {
            state: self.state.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: ForgeCheckpoint) {
        self.state = checkpoint.state;
    }

    pub fn type_registry(&self) -> TypeRegistry {
        self.state.type_registry.clone()
    }

    /// Restore a `ForgeSession` from an already-executed `Bytecode`.
    ///
    /// This is used when the TUI (or REPL) loads a pre-compiled `.eldr` file.
    /// The restored session can generate new REPL chunks that append on top of
    /// the loaded bytecode.  It cannot recover `slot_map` (let-binding
    /// unique_id → slot), so previous REPL local variables are not accessible
    /// by name in new input.  Function definitions and the type registry are
    /// fully restored.
    pub fn from_bytecode(bytecode: &sindr::ir::Bytecode) -> Self {
        let next_fun_idx = bytecode
            .functions
            .iter()
            .map(|f| f.fun_idx + 1)
            .max()
            .unwrap_or(0);

        // Reconstruct error_ctor_funs: ErrTemplate.kind → (fun_idx, num_params).
        let mut error_ctor_funs = HashMap::new();
        for template in &bytecode.error_templates {
            if let Some(fun_entry) = bytecode
                .functions
                .iter()
                .find(|f| f.qualified_name.as_deref() == Some(&template.kind))
            {
                error_ctor_funs.insert(
                    template.kind.clone(),
                    (fun_entry.fun_idx, template.num_params),
                );
            }
        }

        Self {
            state: CodegenState {
                constants: bytecode.constants.clone(),
                slot_map: HashMap::new(),
                next_slot: bytecode.num_locals as u32,
                next_fun_idx,
                type_registry: bytecode.type_registry.clone(),
                error_templates: bytecode.error_templates.clone(),
                dbg_templates: bytecode.dbg_templates.clone(),
                functions: bytecode.functions.clone(),
                error_ctor_funs,
            },
        }
    }

    pub fn codegen_chunk(
        &mut self,
        typed: Vec<TypedNode>,
    ) -> Result<(BytecodeChunk, ChunkMeta), CodegenError> {
        self.codegen_chunk_typed_program(TypedProgram {
            nodes: typed,
            process_specs: Vec::new(),
            boot_plan: SupervisorInitSpec::default(),
        })
    }

    pub fn codegen_chunk_typed_program(
        &mut self,
        typed: TypedProgram,
    ) -> Result<(BytecodeChunk, ChunkMeta), CodegenError> {
        self.codegen_chunk_typed_program_with_options(typed, false)
    }

    pub fn codegen_chunk_repl_result(
        &mut self,
        typed: Vec<TypedNode>,
    ) -> Result<(BytecodeChunk, ChunkMeta), CodegenError> {
        self.codegen_chunk_typed_program_with_options(
            TypedProgram {
                nodes: typed,
                process_specs: Vec::new(),
                boot_plan: SupervisorInitSpec::default(),
            },
            true,
        )
    }

    fn codegen_chunk_typed_program_with_options(
        &mut self,
        typed: TypedProgram,
        top_level_returns_result: bool,
    ) -> Result<(BytecodeChunk, ChunkMeta), CodegenError> {
        let TypedProgram {
            nodes,
            process_specs,
            boot_plan,
        } = typed;
        let typed_for_meta = nodes.clone();
        let (chunk, meta, functions) =
            self.codegen_chunk_nodes_with_options(nodes, top_level_returns_result)?;
        let runtime_process_specs =
            build_runtime_process_specs(&process_specs, &typed_for_meta, &functions)?.entries;
        let runtime_boot_plan = build_runtime_boot_plan(&boot_plan, &process_specs)?;
        Ok((
            BytecodeChunk {
                runtime_process_specs,
                runtime_boot_plan,
                ..chunk
            },
            meta,
        ))
    }

    fn codegen_chunk_nodes_with_options(
        &mut self,
        typed: Vec<TypedNode>,
        top_level_returns_result: bool,
    ) -> Result<(BytecodeChunk, ChunkMeta, Vec<FunctionEntry>), CodegenError> {
        let before = self.state.clone();
        let typed_for_meta = typed.clone();
        let const_base = before.constants.len();
        let error_template_base = before.error_templates.len();
        let dbg_template_base = before.dbg_templates.len();

        let mut gene = Codegen::from_state(before.clone());
        gene.set_chunk_constant_dedup_start(const_base);
        gene.set_top_level_returns_result(top_level_returns_result);
        gene.emit_program_chunk(typed)?;
        let (mut opcodes, after) = gene.finalize()?;
        localize_chunk_indices(
            &mut opcodes,
            const_base,
            error_template_base,
            dbg_template_base,
        )?;

        let new_constants = after.constants[before.constants.len()..].to_vec();
        let new_locals = after.next_slot.saturating_sub(before.next_slot) as usize;
        let type_entries =
            after.type_registry.entries[before.type_registry.entries.len()..].to_vec();
        let error_templates = after.error_templates[before.error_templates.len()..].to_vec();
        let dbg_templates = after.dbg_templates[before.dbg_templates.len()..].to_vec();
        let meta = collect_chunk_meta(&typed_for_meta, &after.slot_map);
        let functions = after.functions[before.functions.len()..].to_vec();

        self.state = after;

        let const_base = u32::try_from(const_base).map_err(|_| CodegenError {
            message: "constant base exceeds u32".into(),
            span: Span { start: 0, end: 0 },
        })?;
        let error_template_base = u32::try_from(error_template_base).map_err(|_| CodegenError {
            message: "error template base exceeds u32".into(),
            span: Span { start: 0, end: 0 },
        })?;
        let dbg_template_base = u32::try_from(dbg_template_base).map_err(|_| CodegenError {
            message: "dbg template base exceeds u32".into(),
            span: Span { start: 0, end: 0 },
        })?;

        Ok((
            BytecodeChunk {
                opcodes,
                source_map: None,
                const_base,
                constants: new_constants,
                new_locals,
                type_entries,
                error_template_base,
                error_templates,
                dbg_template_base,
                dbg_templates,
                functions: functions.clone(),
                docs: Vec::new(),
                runtime_process_specs: Vec::new(),
                runtime_boot_plan: Default::default(),
            },
            meta,
            functions,
        ))
    }
}

fn localize_chunk_indices(
    opcodes: &mut [Opcode],
    const_base: usize,
    error_template_base: usize,
    dbg_template_base: usize,
) -> Result<(), CodegenError> {
    for op in opcodes.iter_mut() {
        match op {
            Opcode::LoadConst(idx) => {
                let idx_usize = *idx as usize;
                if idx_usize < const_base {
                    return Err(CodegenError {
                        message: format!(
                            "chunk constant index {} is below base {}",
                            idx_usize, const_base
                        ),
                        span: Span { start: 0, end: 0 },
                    });
                }
                *idx = (idx_usize - const_base) as u32;
            }
            Opcode::MakeError { template_id } => {
                let id_usize = *template_id as usize;
                if id_usize < error_template_base {
                    return Err(CodegenError {
                        message: format!(
                            "chunk error template index {} is below base {}",
                            id_usize, error_template_base
                        ),
                        span: Span { start: 0, end: 0 },
                    });
                }
                *template_id = (id_usize - error_template_base) as u32;
            }
            Opcode::Dbg { template_id, .. } => {
                let id_usize = *template_id as usize;
                if id_usize < dbg_template_base {
                    return Err(CodegenError {
                        message: format!(
                            "chunk dbg template index {} is below base {}",
                            id_usize, dbg_template_base
                        ),
                        span: Span { start: 0, end: 0 },
                    });
                }
                *template_id = (id_usize - dbg_template_base) as u32;
            }
            Opcode::MakeErrorLiteral {
                kind_const_idx,
                message_const_idx,
            } => {
                let kind_idx = *kind_const_idx as usize;
                if kind_idx < const_base {
                    return Err(CodegenError {
                        message: format!(
                            "chunk error literal kind index {} is below base {}",
                            kind_idx, const_base
                        ),
                        span: Span { start: 0, end: 0 },
                    });
                }
                let message_idx = *message_const_idx as usize;
                if message_idx < const_base {
                    return Err(CodegenError {
                        message: format!(
                            "chunk error literal message index {} is below base {}",
                            message_idx, const_base
                        ),
                        span: Span { start: 0, end: 0 },
                    });
                }
                *kind_const_idx = (kind_idx - const_base) as u32;
                *message_const_idx = (message_idx - const_base) as u32;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Codegen;
    use crate::opcode::Opcode;
    use scar::typed::{TypedDbgArg, TypedInner, TypedMatchArm, TypedMatchPattern, TypedNode};
    use scar::types::Ty;
    use spire::ast::{BinOp, Lit, Span};

    fn span(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    fn lit_node(ty: Ty, node: Lit, span: Span) -> TypedNode {
        TypedNode {
            ty,
            span: span.clone(),
            node: TypedInner::Lit(node),
        }
    }

    #[test]
    fn finalize_rejects_unresolved_labels() {
        let mut gene = Codegen::new();
        let label = gene.fresh_label();
        gene.emit_jump(label);

        let err = gene.finalize().expect_err("unresolved label must fail");
        assert!(err.message.contains("unresolved jump label"));
    }

    #[test]
    fn emit_match_routes_last_failure_through_pattern_mismatch_path() {
        let mut gene = Codegen::new();
        let scrutinee = lit_node(Ty::Bool, Lit::Bool(false), span(1, 6));
        let body = lit_node(Ty::Bool, Lit::Bool(true), span(10, 14));

        gene.emit_match(
            &scrutinee,
            &[TypedMatchArm {
                pattern: TypedMatchPattern::BoolLit(true),
                guard: None,
                body,
            }],
        )
        .expect("match emission should succeed");

        let (opcodes, _) = gene.finalize().expect("labels should resolve");
        let eprint_id = Codegen::builtin_id("eprint").expect("eprint builtin must exist");
        assert!(opcodes.iter().any(|opcode| {
            matches!(
                opcode,
                Opcode::CallBuiltin {
                    builtin_id,
                    arity: 1,
                    ..
                } if *builtin_id == eprint_id
            )
        }));
        assert!(matches!(opcodes.last(), Some(Opcode::Halt)));
    }

    #[test]
    fn unsupported_binop_preserves_original_span() {
        let gene = Codegen::new();
        let err = gene
            .binop_to_opcode(&BinOp::Add, &Ty::Bool, &span(23, 31))
            .expect_err("bool addition must fail");

        assert_eq!(err.span, span(23, 31));
        assert!(err.message.contains("Unsupported binop"));
    }

    #[test]
    fn emit_assert_builds_result_without_new_opcode() {
        let mut gene = Codegen::new();
        let node = TypedNode {
            ty: Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Error)),
            span: span(1, 24),
            node: TypedInner::Assert(
                Box::new(lit_node(Ty::Bool, Lit::Bool(true), span(1, 5))),
                Box::new(TypedNode {
                    ty: Ty::Error,
                    span: span(10, 19),
                    node: TypedInner::Var(sigil::resolved::ResolvedId {
                        name: "NoneError".into(),
                        qualified_name: Some("NoneError".into()),
                        unique_id: 99,
                        compiler_generated: false,
                        span: span(10, 19),
                    }),
                }),
            ),
        };
        gene.state.slot_map.insert(99, 0);
        gene.state.next_slot = 1;

        gene.emit_node(&node)
            .expect("assert emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");

        assert!(opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::JumpIfFalse(_))));
        assert!(
            opcodes
                .iter()
                .filter(|opcode| matches!(opcode, Opcode::StructNew { field_count: 1 }))
                .count()
                >= 2
        );
    }

    #[test]
    fn emit_ensure_stores_value_and_calls_predicate_once() {
        let mut gene = Codegen::new();
        let pred_id = sigil::resolved::ResolvedId {
            name: "is_even".into(),
            qualified_name: None,
            unique_id: 7,
            compiler_generated: false,
            span: span(8, 16),
        };
        let err_id = sigil::resolved::ResolvedId {
            name: "NoneError".into(),
            qualified_name: Some("NoneError".into()),
            unique_id: 8,
            compiler_generated: false,
            span: span(18, 27),
        };
        gene.state.slot_map.insert(8, 0);
        gene.state.next_slot = 1;

        let node = TypedNode {
            ty: Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error)),
            span: span(1, 27),
            node: TypedInner::Ensure(
                Box::new(lit_node(Ty::Int, Lit::Int(4.into()), span(1, 2))),
                Box::new(TypedNode {
                    ty: Ty::Func(vec![Ty::Int], Box::new(Ty::Bool)),
                    span: span(8, 16),
                    node: TypedInner::Capture(
                        Box::new(TypedNode {
                            ty: Ty::UserFunc {
                                fun_idx: 3,
                                type_params: vec![],
                                params: vec![Ty::Int],
                                ret: Box::new(Ty::Bool),
                            },
                            span: span(8, 16),
                            node: TypedInner::Var(pred_id),
                        }),
                        vec![],
                    ),
                }),
                Box::new(TypedNode {
                    ty: Ty::Error,
                    span: span(18, 27),
                    node: TypedInner::Var(err_id),
                }),
            ),
        };

        gene.emit_node(&node)
            .expect("ensure emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");

        assert!(opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::StoreLocal(_))));
        assert!(opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::CallClosure { arity: 1, .. })));
    }

    #[test]
    fn emit_dbg_uses_dedicated_opcode() {
        let mut gene = Codegen::new();
        let node = TypedNode {
            ty: Ty::Unit,
            span: span(1, 11),
            node: TypedInner::Dbg(vec![
                TypedDbgArg {
                    span: span(6, 7),
                    ty_name: "Int".into(),
                    expr: lit_node(Ty::Int, Lit::Int(1.into()), span(6, 7)),
                },
                TypedDbgArg {
                    span: span(9, 10),
                    ty_name: "Int".into(),
                    expr: lit_node(Ty::Int, Lit::Int(2.into()), span(9, 10)),
                },
            ]),
        };

        gene.emit_node(&node).expect("dbg emission should succeed");
        let (opcodes, state) = gene.finalize().expect("finalize should succeed");

        assert!(matches!(
            opcodes.last(),
            Some(Opcode::Dbg { arg_count: 2, .. })
        ));
        assert_eq!(state.dbg_templates.len(), 1);
    }
}

impl Default for ForgeSession {
    fn default() -> Self {
        Self::new()
    }
}

fn collect_chunk_meta(typed: &[TypedNode], slot_map: &HashMap<u32, u32>) -> ChunkMeta {
    let mut bindings = Vec::new();
    let mut type_defs = Vec::new();
    let mut function_defs = Vec::new();

    for stmt in typed {
        collect_stmt_meta(
            stmt,
            slot_map,
            &mut bindings,
            &mut type_defs,
            &mut function_defs,
        );
    }

    ChunkMeta {
        bindings,
        result_lens_info: top_level_result_lens_info(typed),
        type_defs,
        function_defs,
        docs: Vec::new(),
    }
}

fn top_level_result_lens_info(typed: &[TypedNode]) -> Option<ReplLensInfo> {
    typed
        .iter()
        .rev()
        .find(|stmt| {
            !matches!(
                stmt.node,
                TypedInner::Def(..) | TypedInner::ExtractorDef(..) | TypedInner::DeferrorDef(..)
            )
        })
        .and_then(lens_info_for_node)
}

fn lens_segment_label(segment: &TypedLensSegment) -> String {
    match segment {
        TypedLensSegment::Field { field_name, .. } => field_name.clone(),
        TypedLensSegment::Tuple { field_index, .. } => format!("_{field_index}"),
        TypedLensSegment::Variant { variant_name, .. } => variant_name.clone(),
    }
}

fn lens_path_full_path(path: &TypedLensPath) -> String {
    let mut rendered = String::new();
    for segment in &path.segments {
        match segment {
            TypedLensSegment::Tuple { field_index, .. } => {
                if rendered.is_empty() {
                    rendered.push_str("Tuple");
                }
                rendered.push_str(&format!("._{field_index}"));
            }
            other => {
                if rendered.is_empty() {
                    rendered.push_str(&ty_to_string(&path.source_ty));
                }
                if !rendered.is_empty() {
                    rendered.push('.');
                }
                rendered.push_str(&lens_segment_label(other));
            }
        }
    }
    if rendered.is_empty() {
        "<lens>".to_string()
    } else {
        rendered
    }
}

fn lens_info_for_node(node: &TypedNode) -> Option<ReplLensInfo> {
    match &node.node {
        TypedInner::LensPath(path) => {
            let mut current_source = path.source_ty.clone();
            let mut segments = Vec::with_capacity(path.segments.len());
            let mut stop_points = Vec::new();
            let mut path_is_fallible = false;
            let mut prefix = String::new();
            for segment in &path.segments {
                let label = lens_segment_label(segment);
                let focus_ty = match segment {
                    TypedLensSegment::Field { .. } | TypedLensSegment::Tuple { .. } => {
                        match &current_source {
                            Ty::Tuple(items) => match segment {
                                TypedLensSegment::Tuple { field_index, .. } => items
                                    .get(*field_index as usize)
                                    .cloned()
                                    .unwrap_or(Ty::Unit),
                                _ => Ty::Unit,
                            },
                            Ty::Struct(_, fields) | Ty::Record(_, fields) => match segment {
                                TypedLensSegment::Field { field_index, .. } => fields
                                    .get(*field_index as usize)
                                    .map(|(_, ty)| ty.clone())
                                    .unwrap_or(Ty::Unit),
                                _ => Ty::Unit,
                            },
                            _ => Ty::Unit,
                        }
                    }
                    TypedLensSegment::Variant {
                        payload_arity,
                        variant_name,
                        ..
                    } => {
                        stop_points.push(format!(
                            "{}.{} - variant mismatch returns Result",
                            ty_to_string(&current_source),
                            variant_name
                        ));
                        if *payload_arity == 0 {
                            Ty::Unit
                        } else {
                            path.focus_ty.clone()
                        }
                    }
                };
                if !prefix.is_empty() && !matches!(segment, TypedLensSegment::Tuple { .. }) {
                    prefix.push('.');
                }
                match segment {
                    TypedLensSegment::Tuple { field_index, .. } => {
                        if prefix.is_empty() {
                            prefix.push_str("Tuple");
                        }
                        prefix.push_str(&format!("._{field_index}"));
                    }
                    _ => prefix.push_str(&label),
                }
                let (kind, fallible, reason) = match segment {
                    TypedLensSegment::Field { .. } => ("field", false, "field access"),
                    TypedLensSegment::Tuple { .. } => ("tuple", false, "tuple index access"),
                    TypedLensSegment::Variant { .. } => {
                        path_is_fallible = true;
                        ("variant", true, "variant mismatch returns Result")
                    }
                };
                segments.push(ReplLensSegmentInfo {
                    label: prefix.clone(),
                    kind: kind.to_string(),
                    source_ty: ty_to_string(&current_source),
                    focus_ty: ty_to_string(&focus_ty),
                    fallible,
                    reason: reason.to_string(),
                });
                current_source = focus_ty;
            }
            Some(ReplLensInfo {
                ty: ty_to_string(&node.ty),
                view_result_ty: if path_is_fallible {
                    format!("Result<{}, Error>", ty_to_string(&path.focus_ty))
                } else {
                    ty_to_string(&path.focus_ty)
                },
                full_path: lens_path_full_path(path),
                segments,
                stop_points,
            })
        }
        TypedInner::PendingLensPath(path) => Some(ReplLensInfo {
            ty: ty_to_string(&node.ty),
            view_result_ty: "_".to_string(),
            full_path: if path.segments.is_empty() {
                "<lens>".to_string()
            } else {
                let mut rendered = String::new();
                for (index, segment) in path.segments.iter().enumerate() {
                    if index == 0 && segment.starts_with('_') {
                        rendered.push_str("Tuple");
                    } else if !rendered.is_empty() && !segment.starts_with('_') {
                        rendered.push('.');
                    }
                    if segment.starts_with('_') {
                        rendered.push('.');
                    }
                    rendered.push_str(segment);
                }
                rendered
            },
            segments: path
                .segments
                .iter()
                .map(|segment| ReplLensSegmentInfo {
                    label: if segment.starts_with('_') {
                        format!("Tuple.{segment}")
                    } else {
                        segment.clone()
                    },
                    kind: if segment.starts_with('_') {
                        "tuple".to_string()
                    } else {
                        "field".to_string()
                    },
                    source_ty: "_".to_string(),
                    focus_ty: "_".to_string(),
                    fallible: false,
                    reason: "requires Lens context to specialize".to_string(),
                })
                .collect(),
            stop_points: Vec::new(),
        }),
        _ => None,
    }
}

fn collect_stmt_meta(
    stmt: &TypedNode,
    slot_map: &HashMap<u32, u32>,
    bindings: &mut Vec<BindingInfo>,
    type_defs: &mut Vec<TypeDefDisplay>,
    function_defs: &mut Vec<String>,
) {
    match &stmt.node {
        TypedInner::Bind(pat, _) | TypedInner::SafeBind(pat, _) => {
            let rhs = match &stmt.node {
                TypedInner::Bind(_, rhs) | TypedInner::SafeBind(_, rhs) => rhs.as_ref(),
                _ => unreachable!(),
            };
            collect_pattern_binding_infos(
                pat,
                slot_map,
                bindings,
                callable_kind_for_node(rhs),
                callable_display_for_node(rhs),
                &callable_capture_names(rhs),
                lens_info_for_node(rhs),
            );
        }
        TypedInner::StructDef(_, name, field_names, _, _) => {
            type_defs.push(TypeDefDisplay {
                name: name.clone(),
                kind: ReplTypeKind::Struct,
                fields: field_names
                    .iter()
                    .map(|field| (field.clone(), String::new()))
                    .collect(),
            });
        }
        TypedInner::RecordDef(_, name, field_names, _, _) => {
            type_defs.push(TypeDefDisplay {
                name: name.clone(),
                kind: ReplTypeKind::Record,
                fields: field_names
                    .iter()
                    .map(|field| (field.clone(), String::new()))
                    .collect(),
            });
        }
        TypedInner::DeferrorDef(_, _, id, _, _) => {
            type_defs.push(TypeDefDisplay {
                name: id.name.clone(),
                kind: ReplTypeKind::Error,
                fields: Vec::new(),
            });
            function_defs.push(id.name.clone());
        }
        TypedInner::EnumDef(name, variants) => {
            type_defs.push(TypeDefDisplay {
                name: name.clone(),
                kind: ReplTypeKind::Enum,
                fields: variants
                    .iter()
                    .map(|variant| (variant.constructor_name.clone(), String::new()))
                    .collect(),
            });
        }
        TypedInner::Def(_, id, _, _, _, _, _) => {
            function_defs.push(id.name.clone());
        }
        TypedInner::ExtractorDef(_, id, _, _, _, _, _) => {
            function_defs.push(id.name.clone());
        }
        // `;` keeps Unit as expression result, but for REPL metadata we still
        // want to surface bindings/type defs introduced by the wrapped statement.
        TypedInner::Semi(inner) => {
            collect_stmt_meta(inner, slot_map, bindings, type_defs, function_defs);
        }
        _ => {}
    }
}

fn collect_pattern_binding_infos(
    pat: &TypedPattern,
    slot_map: &HashMap<u32, u32>,
    out: &mut Vec<BindingInfo>,
    callable_kind: Option<ReplCallableKind>,
    callable_display: Option<ReplCallableDisplay>,
    callable_captures: &[String],
    lens_info: Option<ReplLensInfo>,
) {
    match pat {
        TypedPattern::Var(ty, id) => {
            if let Some(slot_id) = slot_map.get(&id.unique_id) {
                out.push(BindingInfo {
                    name: id.name.clone(),
                    ty: ty_to_string(ty),
                    slot_id: *slot_id,
                    callable_kind,
                    callable_display: callable_display.clone(),
                    callable_captures: callable_captures.to_vec(),
                    lens_info: lens_info.clone(),
                });
            }
        }
        TypedPattern::As(ty, inner, id) => {
            if let Some(slot_id) = slot_map.get(&id.unique_id) {
                out.push(BindingInfo {
                    name: id.name.clone(),
                    ty: ty_to_string(ty),
                    slot_id: *slot_id,
                    callable_kind,
                    callable_display: callable_display.clone(),
                    callable_captures: callable_captures.to_vec(),
                    lens_info: lens_info.clone(),
                });
            }
            collect_pattern_binding_infos(
                inner,
                slot_map,
                out,
                callable_kind,
                callable_display,
                callable_captures,
                lens_info,
            );
        }
        TypedPattern::Wildcard(_)
        | TypedPattern::ListNil(_)
        | TypedPattern::IntLit(_, _)
        | TypedPattern::StrLit(_, _)
        | TypedPattern::BoolLit(_, _)
        | TypedPattern::DurationLit(_, _) => {}
        TypedPattern::Tuple(_, items) => {
            for item in items {
                collect_pattern_binding_infos(
                    item,
                    slot_map,
                    out,
                    callable_kind,
                    callable_display.clone(),
                    callable_captures,
                    lens_info.clone(),
                );
            }
        }
        TypedPattern::ListCons(_, head, tail) => {
            collect_pattern_binding_infos(
                head,
                slot_map,
                out,
                callable_kind,
                callable_display.clone(),
                callable_captures,
                lens_info.clone(),
            );
            collect_pattern_binding_infos(
                tail,
                slot_map,
                out,
                callable_kind,
                callable_display,
                callable_captures,
                lens_info,
            );
        }
        TypedPattern::ResultOk(_, inner) => {
            collect_pattern_binding_infos(
                inner,
                slot_map,
                out,
                callable_kind,
                callable_display,
                callable_captures,
                lens_info,
            );
        }
        TypedPattern::Extractor { items, .. } => {
            for item in items {
                collect_pattern_binding_infos(
                    item,
                    slot_map,
                    out,
                    callable_kind,
                    callable_display.clone(),
                    callable_captures,
                    lens_info.clone(),
                );
            }
        }
    }
}

fn callable_kind_for_node(node: &TypedNode) -> Option<ReplCallableKind> {
    match &node.node {
        TypedInner::Closure(params, _, _)
            if params
                .iter()
                .all(|param| param.id.name.starts_with("__cap_")) =>
        {
            Some(ReplCallableKind::Capture)
        }
        TypedInner::Closure(..) => Some(ReplCallableKind::Closure),
        TypedInner::Capture(..) | TypedInner::InjectCall(..) => Some(ReplCallableKind::Capture),
        TypedInner::Semi(inner) => callable_kind_for_node(inner),
        _ => None,
    }
}

fn callable_display_for_node(node: &TypedNode) -> Option<ReplCallableDisplay> {
    match &node.node {
        TypedInner::InjectCall(func, _) => {
            let (module, name) = callable_head_for_node(func.as_ref())?;
            Some(ReplCallableDisplay::FnCapture {
                module,
                name,
                sig: ty_to_string(&node.ty),
            })
        }
        TypedInner::Closure(params, _, body)
            if params
                .iter()
                .all(|param| param.id.name.starts_with("__cap_")) =>
        {
            let (module, name) = callable_head_for_invocation(body.as_ref())?;
            Some(ReplCallableDisplay::FnCapture {
                module,
                name,
                sig: ty_to_string(&node.ty),
            })
        }
        TypedInner::Closure(..) => Some(ReplCallableDisplay::Closure {
            sig: ty_to_string(&node.ty),
        }),
        TypedInner::Semi(inner) => callable_display_for_node(inner),
        _ => None,
    }
}

fn callable_capture_names(node: &TypedNode) -> Vec<String> {
    match &node.node {
        TypedInner::Closure(_, captures, _) => captures
            .iter()
            .map(|capture| capture.name.to_string())
            .collect(),
        TypedInner::Semi(inner) => callable_capture_names(inner),
        _ => Vec::new(),
    }
}

fn callable_head_for_node(node: &TypedNode) -> Option<(String, String)> {
    match &node.node {
        TypedInner::Var(id) => {
            let qualified = id.qualified_name.as_deref().unwrap_or(id.name.as_str());
            let (module, name) = qualified.rsplit_once("::")?;
            Some((module.to_string(), name.to_string()))
        }
        _ => None,
    }
}

fn callable_head_for_invocation(node: &TypedNode) -> Option<(String, String)> {
    match &node.node {
        TypedInner::App(func, _) => callable_head_for_node(func),
        TypedInner::TraitCall {
            trait_name,
            method_name,
            origin: TraitCallOrigin::Explicit,
            ..
        } => Some((
            trait_short_name(trait_name).to_string(),
            method_name.clone(),
        )),
        _ => None,
    }
}

fn trait_short_name(trait_name: &str) -> &str {
    trait_name
        .split_once('<')
        .map(|(name, _)| name)
        .or_else(|| trait_name.split_once(" for ").map(|(name, _)| name))
        .unwrap_or(trait_name)
}

fn ty_to_string(ty: &Ty) -> String {
    match ty {
        Ty::Int => "Int".into(),
        Ty::Float => "Float".into(),
        Ty::Str => "String".into(),
        Ty::Bool => "Boolean".into(),
        Ty::Unit => "Unit".into(),
        Ty::Hole => "_".into(),
        Ty::List(inner) => format!("List<{}>", ty_to_string(inner)),
        Ty::Lazy(inner) => format!("Lazy<{}>", ty_to_string(inner)),
        Ty::TypeRef(inner) => format!("TypeRef<{}>", ty_to_string(inner)),
        Ty::Pid(name) => format!("PID<{}>", name),
        Ty::Lens(source, focus) => {
            format!("Lens<{}, {}>", ty_to_string(source), ty_to_string(focus))
        }
        Ty::Tuple(items) => format!(
            "({})",
            items
                .iter()
                .map(ty_to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Ty::Result(ok, err) => format!("Result<{}, {}>", ty_to_string(ok), ty_to_string(err)),
        Ty::Struct(name, _) | Ty::Record(name, _) => name.clone(),
        Ty::Enum(name, args) => {
            if args.is_empty() {
                name.clone()
            } else {
                format!(
                    "{}<{}>",
                    name,
                    args.iter().map(ty_to_string).collect::<Vec<_>>().join(", ")
                )
            }
        }
        Ty::Error => "Error".into(),
        // Hide internal type-variable IDs from REPL output.
        Ty::Var(_id) => "_".into(),
        Ty::Func(params, ret) => {
            let param_str = params
                .iter()
                .map(ty_to_string)
                .collect::<Vec<_>>()
                .join(", ");
            if param_str.is_empty() {
                format!("(-> {})", ty_to_string(ret))
            } else {
                format!("({} -> {})", param_str, ty_to_string(ret))
            }
        }
        Ty::BuiltinFunc { name, .. } => format!("Builtin({})", name),
        Ty::UserFunc { .. } => "UserFunc".into(),
    }
}

fn format_function_signature(name: &str, params: &[TypedFunParam], ret_ty: &Ty) -> String {
    let params = params
        .iter()
        .map(|param| format!("{}: {}", param.id.name, ty_to_string(&param.ty)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}({params}) -> {}", ty_to_string(ret_ty))
}

fn format_error_constructor_signature(name: &str, params: &[TypedFunParam]) -> String {
    let params = params
        .iter()
        .map(|param| format!("{}: {}", param.id.name, ty_to_string(&param.ty)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}({params}) -> {name}")
}

// ── IR with labels (resolved to absolute addresses at the end) ──

#[derive(Debug, Clone)]
enum IrOp {
    Op(Opcode),
    /// Jump to label (resolved later)
    JumpLabel(Label),
    /// Jump-if-false to label
    JumpIfFalseLabel(Label),
    /// Jump-if-true to label
    JumpIfTrueLabel(Label),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Label(u32);

#[derive(Debug, Clone)]
struct PendingClosure {
    fun_idx: u32,
    captures: Vec<ResolvedId>,
    params: Vec<TypedClosureParam>,
    body: Box<TypedNode>,
    display: Option<ReplCallableDisplay>,
    signature: String,
}

#[derive(Debug, Clone)]
struct PendingCompose {
    fun_idx: u32,
    flavor: ComposeFlavor,
    span: Span,
}

#[derive(Debug, Clone)]
struct PendingInjectCall {
    fun_idx: u32,
    extra_arg_count: usize,
    span: Span,
    display: Option<ReplCallableDisplay>,
    signature: String,
}

#[derive(Debug, Clone, Copy)]
enum LensUpdateLeaf {
    Set {
        value_slot: u32,
        wrap_plain_result: bool,
    },
    Over {
        update_fun_slot: u32,
        mode: TypedLensOverMode,
        focus_is_result: bool,
    },
}

struct Codegen {
    ir: Vec<IrOp>,
    state: CodegenState,
    next_label: u32,
    label_positions: HashMap<Label, usize>, // label → IR index it points to
    pending_closures: Vec<PendingClosure>,
    pending_composes: Vec<PendingCompose>,
    pending_inject_calls: Vec<PendingInjectCall>,
    in_function: bool,
    top_level_returns_result: bool,
    constant_dedup_start: usize,
}

impl Codegen {
    fn new() -> Self {
        Self::from_state(CodegenState::new())
    }

    fn from_state(state: CodegenState) -> Self {
        Self {
            ir: Vec::new(),
            state,
            next_label: 0,
            label_positions: HashMap::new(),
            pending_closures: Vec::new(),
            pending_composes: Vec::new(),
            pending_inject_calls: Vec::new(),
            in_function: false,
            top_level_returns_result: false,
            constant_dedup_start: 0,
        }
    }

    fn set_chunk_constant_dedup_start(&mut self, start: usize) {
        self.constant_dedup_start = start;
    }

    fn set_top_level_returns_result(&mut self, enabled: bool) {
        self.top_level_returns_result = enabled;
    }

    fn fresh_label(&mut self) -> Label {
        let l = Label(self.next_label);
        self.next_label += 1;
        l
    }

    fn alloc_slot(&mut self, unique_id: u32) -> u32 {
        if let Some(&slot) = self.state.slot_map.get(&unique_id) {
            return slot;
        }
        let slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.state.slot_map.insert(unique_id, slot);
        slot
    }

    fn reserve_fun_idx(&mut self) -> u32 {
        let fun_idx = self.state.next_fun_idx;
        self.state.next_fun_idx += 1;
        fun_idx
    }

    fn builtin_id(name: &str) -> Option<u16> {
        let short_name = name.rsplit("::").next().unwrap_or(name);
        builtin_id_by_name(short_name)
    }

    fn direct_builtin_opcode(name: &str, arity: usize) -> Option<Opcode> {
        match name.rsplit("::").next().unwrap_or(name) {
            "bit_not" if arity == 1 => Some(Opcode::BitNotInt),
            "bit_and" if arity == 2 => Some(Opcode::BitAndInt),
            "bit_or" if arity == 2 => Some(Opcode::BitOrInt),
            "bit_xor" if arity == 2 => Some(Opcode::BitXorInt),
            _ => None,
        }
    }

    fn emit_callable_ref(&mut self, node: &TypedNode) -> Result<(), CodegenError> {
        match &node.node {
            TypedInner::Var(id) => match &node.ty {
                Ty::BuiltinFunc { name, .. } => {
                    let builtin_id = Self::builtin_id(name).ok_or_else(|| CodegenError {
                        message: format!("Unknown builtin: {}", name),
                        span: node.span.clone(),
                    })?;
                    self.emit(Opcode::LoadBuiltinRef(builtin_id));
                }
                Ty::UserFunc { fun_idx, .. } => {
                    self.emit(Opcode::LoadFunctionRef(*fun_idx));
                }
                Ty::Func(_, _) => {
                    let slot = self.alloc_slot(id.unique_id);
                    self.emit(Opcode::LoadLocal(slot));
                }
                _ => {
                    return Err(CodegenError {
                        message: "Not a callable value".into(),
                        span: node.span.clone(),
                    });
                }
            },
            _ => {
                self.emit_node(node)?;
            }
        }
        Ok(())
    }

    fn emit_closure_function(
        &mut self,
        fun_idx: u32,
        params: &[TypedClosureParam],
        captures: &[ResolvedId],
        body: &TypedNode,
        display: Option<&ReplCallableDisplay>,
        signature: &str,
    ) -> Result<(), CodegenError> {
        let saved_slot_map = self.state.slot_map.clone();
        let saved_next_slot = self.state.next_slot;

        self.state.slot_map = HashMap::new();
        self.state.next_slot = 0;

        let mut slot = 0u32;
        for capture in captures {
            self.state.slot_map.insert(capture.unique_id, slot);
            slot += 1;
        }
        for param in params {
            self.state.slot_map.insert(param.id.unique_id, slot);
            slot += 1;
        }
        self.state.next_slot = slot;

        let entry_pc = self.current_pos() as u32;
        let total_arity = captures.len() + params.len();
        let prev_in_function = self.in_function;
        self.in_function = true;
        self.emit_tail_node(body)?;
        self.in_function = prev_in_function;

        let (qualified_name, signature) = match display {
            Some(ReplCallableDisplay::FnCapture { module, name, sig }) => {
                (Some(format!("{module}::{name}")), Some(sig.clone()))
            }
            Some(ReplCallableDisplay::Closure { sig }) => (None, Some(sig.clone())),
            None => (None, Some(signature.to_string())),
        };

        self.state.functions.push(FunctionEntry {
            fun_idx,
            entry_pc,
            num_locals: self.state.next_slot,
            arity: total_arity as u8,
            qualified_name,
            signature,
            end_pc: 0,
            span_start: body.span.start as u32,
            span_end: body.span.end as u32,
            flags: FunctionFlags {
                public: false,
                closure: true,
                partial_apply_wrapper: false,
                builtin_wrapper: false,
                tail_entry: false,
                generated: true,
            },
        });

        self.state.slot_map = saved_slot_map;
        self.state.next_slot = saved_next_slot;
        Ok(())
    }

    fn emit_pending_closures(&mut self) -> Result<(), CodegenError> {
        while !self.pending_closures.is_empty() {
            let pending = std::mem::take(&mut self.pending_closures);
            for closure in pending {
                self.emit_closure_function(
                    closure.fun_idx,
                    &closure.params,
                    &closure.captures,
                    &closure.body,
                    closure.display.as_ref(),
                    &closure.signature,
                )?;
            }
        }
        Ok(())
    }

    fn emit_compose_function(
        &mut self,
        fun_idx: u32,
        flavor: &ComposeFlavor,
        span: &Span,
    ) -> Result<(), CodegenError> {
        let saved_slot_map = self.state.slot_map.clone();
        let saved_next_slot = self.state.next_slot;

        self.state.slot_map = HashMap::new();
        self.state.next_slot = 3;

        let lhs_slot = 0u32;
        let rhs_slot = 1u32;
        let input_slot = 2u32;
        let entry_pc = self.current_pos() as u32;
        let prev_in_function = self.in_function;
        self.in_function = true;

        match flavor {
            ComposeFlavor::Plain => {
                self.emit(Opcode::LoadLocal(rhs_slot));
                self.emit(Opcode::LoadLocal(lhs_slot));
                self.emit(Opcode::LoadLocal(input_slot));
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: span.start as u32,
                    span_end: span.end as u32,
                });
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: span.start as u32,
                    span_end: span.end as u32,
                });
                self.emit(Opcode::Return);
            }
            ComposeFlavor::ResultMap | ComposeFlavor::ResultBind => {
                self.emit(Opcode::LoadLocal(lhs_slot));
                self.emit(Opcode::LoadLocal(input_slot));
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: span.start as u32,
                    span_end: span.end as u32,
                });
                let result_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(result_slot));

                self.emit(Opcode::LoadLocal(result_slot));
                self.emit(Opcode::GetTag);
                let err_tag = self.add_constant(Constant::Tag(1));
                self.emit(Opcode::LoadConst(err_tag));
                self.emit(Opcode::EqTag);

                let ok_path = self.fresh_label();
                self.emit_jump_if_false(ok_path);
                self.emit(Opcode::LoadLocal(result_slot));
                self.emit(Opcode::Return);

                self.patch_label(ok_path);
                match flavor {
                    ComposeFlavor::ResultMap => {
                        let ok_tag = self.add_constant(Constant::Tag(0));
                        self.emit(Opcode::LoadConst(ok_tag));
                        self.emit(Opcode::LoadLocal(rhs_slot));
                        self.emit(Opcode::LoadLocal(result_slot));
                        self.emit(Opcode::GetField { field_index: 0 });
                        self.emit(Opcode::CallClosure {
                            arity: 1,
                            span_start: span.start as u32,
                            span_end: span.end as u32,
                        });
                        self.emit(Opcode::StructNew { field_count: 1 });
                        self.emit(Opcode::Return);
                    }
                    ComposeFlavor::ResultBind => {
                        self.emit(Opcode::LoadLocal(rhs_slot));
                        self.emit(Opcode::LoadLocal(result_slot));
                        self.emit(Opcode::GetField { field_index: 0 });
                        self.emit(Opcode::CallClosure {
                            arity: 1,
                            span_start: span.start as u32,
                            span_end: span.end as u32,
                        });
                        self.emit(Opcode::Return);
                    }
                    _ => unreachable!(),
                }
            }
            ComposeFlavor::ListMap { helper } | ComposeFlavor::ListBind { helper } => {
                self.emit(Opcode::LoadLocal(lhs_slot));
                self.emit(Opcode::LoadLocal(input_slot));
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: span.start as u32,
                    span_end: span.end as u32,
                });
                self.emit(Opcode::LoadLocal(rhs_slot));
                match helper {
                    ListHelperRef::Builtin(builtin_id) => self.emit(Opcode::CallBuiltin {
                        builtin_id: *builtin_id,
                        arity: 2,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    }),
                    ListHelperRef::User(fun_idx) => self.emit(Opcode::Call {
                        fun_idx: *fun_idx,
                        arity: 2,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    }),
                }
                self.emit(Opcode::Return);
            }
        }

        self.in_function = prev_in_function;
        self.state.functions.push(FunctionEntry {
            fun_idx,
            entry_pc,
            num_locals: self.state.next_slot,
            arity: 3,
            qualified_name: None,
            signature: None,
            end_pc: 0,
            span_start: span.start as u32,
            span_end: span.end as u32,
            flags: FunctionFlags {
                public: false,
                closure: false,
                partial_apply_wrapper: false,
                builtin_wrapper: false,
                tail_entry: false,
                generated: true,
            },
        });

        self.state.slot_map = saved_slot_map;
        self.state.next_slot = saved_next_slot;
        Ok(())
    }

    fn emit_inject_call_function(
        &mut self,
        fun_idx: u32,
        extra_arg_count: usize,
        span: &Span,
        display: Option<&ReplCallableDisplay>,
        signature: &str,
    ) -> Result<(), CodegenError> {
        let saved_slot_map = self.state.slot_map.clone();
        let saved_next_slot = self.state.next_slot;

        self.state.slot_map = HashMap::new();
        let func_slot = 0u32;
        let input_slot = (extra_arg_count + 1) as u32;
        self.state.next_slot = input_slot + 1;

        let entry_pc = self.current_pos() as u32;
        let prev_in_function = self.in_function;
        self.in_function = true;

        self.emit(Opcode::LoadLocal(func_slot));
        self.emit(Opcode::LoadLocal(input_slot));
        for offset in 0..extra_arg_count {
            self.emit(Opcode::LoadLocal((offset + 1) as u32));
        }
        self.emit(Opcode::CallClosure {
            arity: (extra_arg_count + 1) as u8,
            span_start: span.start as u32,
            span_end: span.end as u32,
        });
        self.in_function = prev_in_function;
        self.emit(Opcode::Return);

        let (qualified_name, signature) = match display {
            Some(ReplCallableDisplay::FnCapture { module, name, sig }) => {
                (Some(format!("{module}::{name}")), Some(sig.clone()))
            }
            Some(ReplCallableDisplay::Closure { sig }) => (None, Some(sig.clone())),
            None => (None, Some(signature.to_string())),
        };

        self.state.functions.push(FunctionEntry {
            fun_idx,
            entry_pc,
            num_locals: self.state.next_slot,
            arity: (extra_arg_count + 2) as u8,
            qualified_name,
            signature,
            end_pc: 0,
            span_start: span.start as u32,
            span_end: span.end as u32,
            flags: FunctionFlags {
                public: false,
                closure: false,
                partial_apply_wrapper: true,
                builtin_wrapper: false,
                tail_entry: false,
                generated: true,
            },
        });

        self.state.slot_map = saved_slot_map;
        self.state.next_slot = saved_next_slot;
        Ok(())
    }

    fn emit_pending_callables(&mut self) -> Result<(), CodegenError> {
        while !self.pending_closures.is_empty()
            || !self.pending_composes.is_empty()
            || !self.pending_inject_calls.is_empty()
        {
            if !self.pending_closures.is_empty() {
                self.emit_pending_closures()?;
            }
            if !self.pending_composes.is_empty() {
                let pending = std::mem::take(&mut self.pending_composes);
                for compose in pending {
                    self.emit_compose_function(compose.fun_idx, &compose.flavor, &compose.span)?;
                }
            }
            if !self.pending_inject_calls.is_empty() {
                let pending = std::mem::take(&mut self.pending_inject_calls);
                for inject_call in pending {
                    self.emit_inject_call_function(
                        inject_call.fun_idx,
                        inject_call.extra_arg_count,
                        &inject_call.span,
                        inject_call.display.as_ref(),
                        &inject_call.signature,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn add_constant(&mut self, c: Constant) -> u32 {
        // Check for existing identical constant
        for (i, existing) in self
            .state
            .constants
            .iter()
            .enumerate()
            .skip(self.constant_dedup_start)
        {
            if existing == &c {
                return i as u32;
            }
        }
        let idx = self.state.constants.len() as u32;
        self.state.constants.push(c);
        idx
    }

    fn add_dbg_template(&mut self, span: Span, args: &[TypedDbgArg]) -> u32 {
        let id = self.state.dbg_templates.len() as u32;
        self.state.dbg_templates.push(DbgTemplate {
            id,
            span_start: span.start as u32,
            span_end: span.end as u32,
            source_name: None,
            args: args
                .iter()
                .map(|arg| DbgArgTemplate {
                    span_start: arg.span.start as u32,
                    span_end: arg.span.end as u32,
                    ty_name: arg.ty_name.clone(),
                })
                .collect(),
        });
        id
    }

    fn emit(&mut self, op: Opcode) {
        self.ir.push(IrOp::Op(op));
    }

    fn emit_jump(&mut self, label: Label) {
        self.ir.push(IrOp::JumpLabel(label));
    }

    fn emit_jump_if_false(&mut self, label: Label) {
        self.ir.push(IrOp::JumpIfFalseLabel(label));
    }

    fn emit_jump_if_true(&mut self, label: Label) {
        self.ir.push(IrOp::JumpIfTrueLabel(label));
    }

    fn current_pos(&self) -> usize {
        self.ir.len()
    }

    fn emit_unit_const(&mut self) {
        let unit_idx = self.add_constant(Constant::Unit);
        self.emit(Opcode::LoadConst(unit_idx));
    }

    // ── Program ──

    fn emit_program(&mut self, stmts: Vec<TypedNode>) -> Result<(), CodegenError> {
        self.emit_program_with_functions(stmts, false)
    }

    fn emit_program_chunk(&mut self, stmts: Vec<TypedNode>) -> Result<(), CodegenError> {
        self.emit_program_with_functions(stmts, false)
    }

    fn emit_program_with_functions(
        &mut self,
        stmts: Vec<TypedNode>,
        pop_last: bool,
    ) -> Result<(), CodegenError> {
        // Contract with VM::push_atomic():
        // - Main/top-level statements are emitted first.
        // - A single Halt terminates top-level execution.
        // - Function bodies are emitted strictly after Halt and are entered only via Call/CallClosure.
        // - Top-level duplicate function names are rejected earlier in Sigil.
        let mut defs = Vec::new();
        let mut main_stmts = Vec::new();
        let max_def_fun_idx = stmts
            .iter()
            .filter_map(|stmt| match &stmt.node {
                TypedInner::Def(fun_idx, _, _, _, _, _, _) => Some(*fun_idx),
                TypedInner::ExtractorDef(fun_idx, _, _, _, _, _, _) => Some(*fun_idx),
                TypedInner::DeferrorDef(_, fun_idx, _, _, _) => Some(*fun_idx),
                _ => None,
            })
            .max()
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let existing_fun_idx = self
            .state
            .functions
            .iter()
            .map(|entry| entry.fun_idx)
            .max()
            .map(|idx| idx + 1)
            .unwrap_or(0);
        self.state.next_fun_idx = self
            .state
            .next_fun_idx
            .max(max_def_fun_idx)
            .max(existing_fun_idx);

        for stmt in &stmts {
            if let TypedInner::DeferrorDef(_, fun_idx, id, params, _) = &stmt.node {
                self.state
                    .error_ctor_funs
                    .insert(id.name.clone(), (*fun_idx, params.len() as u8));
            }
            match &stmt.node {
                TypedInner::Def(..)
                | TypedInner::ExtractorDef(..)
                | TypedInner::DeferrorDef(..) => defs.push(stmt),
                _ => main_stmts.push(stmt),
            }
        }
        defs.sort_by_key(|stmt| match &stmt.node {
            TypedInner::Def(fun_idx, _, _, _, _, _, _) => *fun_idx,
            TypedInner::ExtractorDef(fun_idx, _, _, _, _, _, _) => *fun_idx,
            TypedInner::DeferrorDef(_, fun_idx, _, _, _) => *fun_idx,
            _ => u32::MAX,
        });

        for (i, stmt) in main_stmts.iter().enumerate() {
            if self.top_level_returns_result
                && matches!(
                    stmt.node,
                    TypedInner::LensPath(_) | TypedInner::PendingLensPath(_)
                )
            {
                // REPL chunks may end with a LensPath expression so the session can
                // inspect the canonical path without materializing a runtime value.
                self.emit_unit_const();
            } else {
                self.emit_node(stmt)?;
            }
            if pop_last || i + 1 < main_stmts.len() {
                self.emit(Opcode::Pop);
            }
        }

        self.emit(Opcode::Halt);

        for def in defs {
            match &def.node {
                TypedInner::Def(..) => self.emit_function_def(def)?,
                TypedInner::ExtractorDef(..) => self.emit_extractor_def(def)?,
                TypedInner::DeferrorDef(..) => self.emit_error_def(def)?,
                _ => unreachable!(),
            }
        }

        self.emit_pending_callables()?;
        self.normalize_function_table()?;

        Ok(())
    }

    fn emit_function_def(&mut self, node: &TypedNode) -> Result<(), CodegenError> {
        let (fun_idx, id, params, ret_ty, body, visibility) = match &node.node {
            TypedInner::Def(fun_idx, id, _type_params, params, ret_ty, body, visibility) => {
                (fun_idx, id, params, ret_ty, body, visibility)
            }
            _ => {
                return Err(CodegenError {
                    message: "expected function definition".into(),
                    span: node.span.clone(),
                });
            }
        };

        let saved_slot_map = self.state.slot_map.clone();
        let saved_next_slot = self.state.next_slot;

        self.state.slot_map = HashMap::new();
        self.state.next_slot = 0;

        for (slot, param) in params.iter().enumerate() {
            self.state.slot_map.insert(param.id.unique_id, slot as u32);
        }
        self.state.next_slot = params.len() as u32;

        let entry_pc = self.current_pos() as u32;
        let prev_in_function = self.in_function;
        self.in_function = true;
        self.emit_tail_node(body)?;
        self.in_function = prev_in_function;

        let num_locals = self.state.next_slot;
        self.state.functions.push(FunctionEntry {
            fun_idx: *fun_idx,
            entry_pc,
            num_locals,
            arity: params.len() as u8,
            qualified_name: id.qualified_name.clone().or_else(|| Some(id.name.clone())),
            signature: Some(format_function_signature(&id.name, params, ret_ty)),
            end_pc: 0,
            span_start: node.span.start as u32,
            span_end: node.span.end as u32,
            flags: FunctionFlags {
                public: *visibility == Visibility::Public,
                closure: false,
                partial_apply_wrapper: false,
                builtin_wrapper: false,
                tail_entry: false,
                generated: false,
            },
        });
        self.state.next_fun_idx = self.state.next_fun_idx.max(*fun_idx + 1);

        self.state.slot_map = saved_slot_map;
        self.state.next_slot = saved_next_slot;

        let _ = id;
        Ok(())
    }

    fn emit_error_def(&mut self, node: &TypedNode) -> Result<(), CodegenError> {
        let (_tag, fun_idx, id, params, body) = match &node.node {
            TypedInner::DeferrorDef(tag, fun_idx, id, params, body) => {
                (tag, fun_idx, id, params, body)
            }
            _ => {
                return Err(CodegenError {
                    message: "expected error definition".into(),
                    span: node.span.clone(),
                });
            }
        };

        let template_id = self.state.error_templates.len() as u32;
        self.state.error_templates.push(ErrTemplate {
            id: template_id,
            kind: id.name.clone(),
            span_start: id.span.start as u32,
            span_end: id.span.end as u32,
            line: 0,
            column: 0,
            format: String::new(),
            num_params: params.len() as u8,
        });

        let saved_slot_map = self.state.slot_map.clone();
        let saved_next_slot = self.state.next_slot;

        self.state.slot_map = HashMap::new();
        self.state.next_slot = 0;

        for (slot, param) in params.iter().enumerate() {
            self.state.slot_map.insert(param.id.unique_id, slot as u32);
        }
        self.state.next_slot = params.len() as u32;

        let entry_pc = self.current_pos() as u32;
        let prev_in_function = self.in_function;
        self.in_function = true;
        self.emit_node(body)?;
        self.in_function = prev_in_function;
        self.emit(Opcode::MakeError { template_id });
        self.emit(Opcode::Return);

        let num_locals = self.state.next_slot;
        self.state.functions.push(FunctionEntry {
            fun_idx: *fun_idx,
            entry_pc,
            num_locals,
            arity: params.len() as u8,
            qualified_name: id.qualified_name.clone().or_else(|| Some(id.name.clone())),
            signature: Some(format_error_constructor_signature(&id.name, params)),
            end_pc: 0,
            span_start: node.span.start as u32,
            span_end: node.span.end as u32,
            flags: FunctionFlags {
                public: true,
                closure: false,
                partial_apply_wrapper: false,
                builtin_wrapper: false,
                tail_entry: false,
                generated: false,
            },
        });
        self.state
            .error_ctor_funs
            .insert(id.name.clone(), (*fun_idx, params.len() as u8));

        self.state.slot_map = saved_slot_map;
        self.state.next_slot = saved_next_slot;
        Ok(())
    }

    fn emit_extractor_def(&mut self, node: &TypedNode) -> Result<(), CodegenError> {
        let (fun_idx, id, param, ret_ty, body, visibility) = match &node.node {
            TypedInner::ExtractorDef(
                fun_idx,
                id,
                _type_params,
                param,
                ret_ty,
                body,
                visibility,
            ) => (fun_idx, id, param, ret_ty, body, visibility),
            _ => {
                return Err(CodegenError {
                    message: "expected extractor definition".into(),
                    span: node.span.clone(),
                });
            }
        };

        let saved_slot_map = self.state.slot_map.clone();
        let saved_next_slot = self.state.next_slot;

        self.state.slot_map = HashMap::new();
        self.state.next_slot = 0;
        self.state.slot_map.insert(param.id.unique_id, 0);
        self.state.next_slot = 1;

        let entry_pc = self.current_pos() as u32;
        let prev_in_function = self.in_function;
        self.in_function = true;
        self.emit_tail_node(body)?;
        self.in_function = prev_in_function;

        let num_locals = self.state.next_slot as u16;
        self.state.functions.push(FunctionEntry {
            fun_idx: *fun_idx,
            entry_pc,
            num_locals: num_locals.into(),
            arity: 1,
            qualified_name: id.qualified_name.clone().or_else(|| Some(id.name.clone())),
            signature: Some(format!(
                "{}({}: {}) -> {}",
                id.name,
                param.id.name,
                ty_to_string(&param.ty),
                ty_to_string(ret_ty)
            )),
            end_pc: 0,
            span_start: node.span.start as u32,
            span_end: node.span.end as u32,
            flags: FunctionFlags {
                public: *visibility == Visibility::Public,
                closure: false,
                partial_apply_wrapper: false,
                builtin_wrapper: false,
                tail_entry: false,
                generated: false,
            },
        });

        self.state.slot_map = saved_slot_map;
        self.state.next_slot = saved_next_slot;
        Ok(())
    }

    fn emit_node(&mut self, node: &TypedNode) -> Result<(), CodegenError> {
        match &node.node {
            TypedInner::Lit(lit) => {
                let c = self.lit_to_constant(lit);
                let idx = self.add_constant(c);
                self.emit(Opcode::LoadConst(idx));
            }

            TypedInner::Var(id) => {
                if matches!(node.ty, Ty::BuiltinFunc { .. } | Ty::UserFunc { .. }) {
                    return Err(CodegenError {
                        message: "Function values must be captured explicitly".into(),
                        span: node.span.clone(),
                    });
                }
                let slot = self.alloc_slot(id.unique_id);
                self.emit(Opcode::LoadLocal(slot));
            }

            TypedInner::Bind(pat, rhs) => {
                if matches!(rhs.ty, Ty::Lens(_, _)) {
                    self.reserve_pattern_slots_for_lens_bind(pat);
                    let unit_idx = self.add_constant(Constant::Unit);
                    self.emit(Opcode::LoadConst(unit_idx));
                    return Ok(());
                }
                self.emit_node(rhs)?;
                let payload_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(payload_slot));

                let fail_label = self.fresh_label();
                self.emit_pattern_test_from_local_for_bind(
                    pat,
                    payload_slot,
                    fail_label,
                    &rhs.span,
                )?;
                self.emit_pattern_bind_from_local(pat, payload_slot)?;

                let success_label = self.fresh_label();
                self.emit_jump(success_label);

                self.patch_label(fail_label);
                self.emit_pattern_mismatch_failure(rhs.span.clone())?;

                self.patch_label(success_label);
                // Bind produces Unit
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::SafeBind(pat, rhs) => {
                self.emit_safebind(pat, rhs)?;
            }

            TypedInner::SupervisorSpawn {
                supervisor_process,
                worker_process,
                init,
            } => {
                let supervisor_idx = self.add_constant(Constant::Str(supervisor_process.clone()));
                self.emit(Opcode::LoadConst(supervisor_idx));
                let worker_idx = self.add_constant(Constant::Str(worker_process.clone()));
                self.emit(Opcode::LoadConst(worker_idx));
                self.emit_node(init)?;
                let builtin_id =
                    Self::builtin_id("__supervisor_spawn").ok_or_else(|| CodegenError {
                        message: "Unknown builtin: __supervisor_spawn".into(),
                        span: node.span.clone(),
                    })?;
                self.emit(Opcode::CallBuiltin {
                    builtin_id,
                    arity: 3,
                    span_start: node.span.start as u32,
                    span_end: node.span.end as u32,
                });
            }

            TypedInner::SupervisorAdopt {
                supervisor_process,
                pid,
                ..
            } => {
                let supervisor_idx = self.add_constant(Constant::Str(supervisor_process.clone()));
                self.emit(Opcode::LoadConst(supervisor_idx));
                self.emit_node(pid)?;
                let builtin_id =
                    Self::builtin_id("__supervisor_adopt").ok_or_else(|| CodegenError {
                        message: "Unknown builtin: __supervisor_adopt".into(),
                        span: node.span.clone(),
                    })?;
                self.emit(Opcode::CallBuiltin {
                    builtin_id,
                    arity: 2,
                    span_start: node.span.start as u32,
                    span_end: node.span.end as u32,
                });
            }

            TypedInner::SupervisorStatus { supervisor_process } => {
                let supervisor_idx = self.add_constant(Constant::Str(supervisor_process.clone()));
                self.emit(Opcode::LoadConst(supervisor_idx));
                let builtin_id =
                    Self::builtin_id("__supervisor_status").ok_or_else(|| CodegenError {
                        message: "Unknown builtin: __supervisor_status".into(),
                        span: node.span.clone(),
                    })?;
                self.emit(Opcode::CallBuiltin {
                    builtin_id,
                    arity: 1,
                    span_start: node.span.start as u32,
                    span_end: node.span.end as u32,
                });
            }

            TypedInner::SupervisorWorkers {
                supervisor_process,
                worker_process,
                init,
                size,
            } => {
                let supervisor_idx = self.add_constant(Constant::Str(supervisor_process.clone()));
                self.emit(Opcode::LoadConst(supervisor_idx));
                let worker_idx = self.add_constant(Constant::Str(worker_process.clone()));
                self.emit(Opcode::LoadConst(worker_idx));
                self.emit_node(init)?;
                self.emit_node(size)?;
                let builtin_id =
                    Self::builtin_id("__supervisor_workers").ok_or_else(|| CodegenError {
                        message: "Unknown builtin: __supervisor_workers".into(),
                        span: node.span.clone(),
                    })?;
                self.emit(Opcode::CallBuiltin {
                    builtin_id,
                    arity: 4,
                    span_start: node.span.start as u32,
                    span_end: node.span.end as u32,
                });
            }

            TypedInner::App(func, args) => {
                self.emit_app(node.span.clone(), func, args)?;
            }

            TypedInner::TraitCall {
                dispatch,
                receiver_ty,
                args,
                ..
            } => match dispatch {
                TraitDispatch::Pending => {
                    return Err(CodegenError {
                        message: "bounded trait call must be specialized before codegen".into(),
                        span: node.span.clone(),
                    });
                }
                TraitDispatch::Static(TraitDispatchTarget::BinOp(op)) => {
                    if args.len() != 2 {
                        return Err(CodegenError {
                            message: format!(
                                "trait binop dispatch expects 2 args, got {}",
                                args.len()
                            ),
                            span: node.span.clone(),
                        });
                    }
                    if matches!(op, BinOp::Eq | BinOp::Neq) && matches!(receiver_ty, Ty::Enum(_, _))
                    {
                        self.emit_enum_eq(op, &args[0], &args[1])?;
                        return Ok(());
                    }
                    self.emit_node(&args[0])?;
                    self.emit_node(&args[1])?;
                    let opcode = self.binop_to_opcode(op, receiver_ty, &node.span)?;
                    self.emit(opcode);
                }
                TraitDispatch::Static(TraitDispatchTarget::Builtin(name)) => {
                    for arg in args {
                        self.emit_node(arg)?;
                    }
                    if let Some(opcode) = Self::direct_builtin_opcode(name, args.len()) {
                        self.emit(opcode);
                    } else {
                        let builtin_id = Self::builtin_id(name).ok_or_else(|| CodegenError {
                            message: format!("Unknown builtin: {}", name),
                            span: node.span.clone(),
                        })?;
                        self.emit(Opcode::CallBuiltin {
                            builtin_id,
                            arity: args.len() as u8,
                            span_start: node.span.start as u32,
                            span_end: node.span.end as u32,
                        });
                    }
                }
                TraitDispatch::Static(TraitDispatchTarget::UserFunction { fun_idx, .. }) => {
                    for arg in args {
                        self.emit_node(arg)?;
                    }
                    self.emit(Opcode::Call {
                        fun_idx: *fun_idx,
                        arity: args.len() as u8,
                        span_start: node.span.start as u32,
                        span_end: node.span.end as u32,
                    });
                }
            },

            TypedInner::InjectCall(func, args) => {
                let fun_idx = self.reserve_fun_idx();
                self.pending_inject_calls.push(PendingInjectCall {
                    fun_idx,
                    extra_arg_count: args.len(),
                    span: node.span.clone(),
                    display: callable_display_for_node(node),
                    signature: ty_to_string(&node.ty),
                });
                self.emit(Opcode::LoadFunctionRef(fun_idx));
                self.emit_callable_ref(func)?;
                for arg in args {
                    self.emit_node(arg)?;
                }
                self.emit(Opcode::CaptureClosure((args.len() + 1) as u8));
            }

            TypedInner::BinOp(op, left, right) => {
                if matches!(op, BinOp::Eq | BinOp::Neq) && matches!(left.ty, Ty::Enum(_, _)) {
                    self.emit_enum_eq(op, left, right)?;
                } else {
                    self.emit_node(left)?;
                    self.emit_node(right)?;
                    let opcode = self.binop_to_opcode(op, &left.ty, &node.span)?;
                    self.emit(opcode);
                }
            }

            TypedInner::Pipe(left, right) => {
                self.emit_callable_ref(right)?;
                self.emit_node(left)?;
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: node.span.start as u32,
                    span_end: node.span.end as u32,
                });
            }

            TypedInner::Compose(flavor, left, right) => {
                let fun_idx = self.reserve_fun_idx();
                self.pending_composes.push(PendingCompose {
                    fun_idx,
                    flavor: flavor.clone(),
                    span: node.span.clone(),
                });
                self.emit(Opcode::LoadFunctionRef(fun_idx));
                self.emit_callable_ref(left)?;
                self.emit_callable_ref(right)?;
                self.emit(Opcode::CaptureClosure(2));
            }

            TypedInner::ListNil => self.emit(Opcode::ListNil),
            TypedInner::ListCons(_, _) => self.emit_list_cons_chain(node)?,
            TypedInner::ListLiteral(elems) => {
                for elem in elems {
                    self.emit_node(elem)?;
                }
                self.emit(Opcode::ListFromItems {
                    len: elems.len() as u32,
                });
            }
            TypedInner::TupleLiteral(elems) => {
                for elem in elems {
                    self.emit_node(elem)?;
                }
                self.emit(Opcode::TupleNew {
                    len: elems.len() as u32,
                });
            }

            TypedInner::InterpolatedStr(parts) => {
                self.emit_interpolated_str(parts)?;
            }

            TypedInner::Dbg(args) => {
                for arg in args {
                    self.emit_node(&arg.expr)?;
                }
                let template_id = self.add_dbg_template(node.span.clone(), args);
                self.emit(Opcode::Dbg {
                    template_id,
                    arg_count: args.len() as u8,
                });
            }

            TypedInner::If(cond, then, else_opt) => {
                self.emit_if(cond, then, else_opt)?;
            }
            TypedInner::Assert(cond, err) => {
                self.emit_assert(node, cond, err)?;
            }
            TypedInner::Ensure(value, pred, err) => {
                self.emit_ensure(node, value, pred, err)?;
            }
            TypedInner::MapErr(value, err) => {
                self.emit_result_error_transform(node, value, err, "map_err")?;
            }
            TypedInner::Cause(value, err) => {
                self.emit_result_error_transform(node, value, err, "cause")?;
            }
            TypedInner::RecoverKind(value, marker, handler) => {
                self.emit_recover_kind(node, value, marker, handler)?;
            }

            TypedInner::Match(scrutinee, arms) => {
                self.emit_match(scrutinee, arms)?;
            }

            TypedInner::FieldAccess(expr, idx) => {
                self.emit_node(expr)?;
                match &expr.ty {
                    Ty::Tuple(_) => self.emit(Opcode::GetTupleField { field_index: *idx }),
                    _ => self.emit(Opcode::GetField { field_index: *idx }),
                }
            }
            TypedInner::ProcessContextHandler { process_name, slot } => {
                let process_idx = self.add_constant(Constant::Str(process_name.clone()));
                self.emit(Opcode::LoadConst(process_idx));
                let slot_idx = self.add_constant(Constant::Str(slot.clone()));
                self.emit(Opcode::LoadConst(slot_idx));
                let builtin_id =
                    Self::builtin_id("__process_context_handler").ok_or_else(|| CodegenError {
                        message: "Unknown builtin: __process_context_handler".into(),
                        span: node.span.clone(),
                    })?;
                self.emit(Opcode::CallBuiltin {
                    builtin_id,
                    arity: 2,
                    span_start: node.span.start as u32,
                    span_end: node.span.end as u32,
                });
            }

            TypedInner::LensPath(_) | TypedInner::PendingLensPath(_) => {
                return Err(CodegenError {
                    message:
                        "Lens path value leaked to codegen; Lens is compile-time only in Stage1"
                            .into(),
                    span: node.span.clone(),
                });
            }

            TypedInner::LensView {
                source,
                path,
                source_is_result,
            } => {
                self.emit_lens_view(node, source, path, *source_is_result)?;
            }
            TypedInner::LensSet {
                source,
                path,
                value,
                source_is_result,
                mode,
            } => {
                self.emit_lens_set(node, source, path, value, *source_is_result, *mode)?;
            }
            TypedInner::LensOver {
                source,
                path,
                update_fun,
                source_is_result,
                mode,
            } => {
                self.emit_lens_over(node, source, path, update_fun, *source_is_result, *mode)?;
            }

            TypedInner::StructLit(tag, fields) => {
                // Push tag first, then fields
                let tag_const = self.add_constant(Constant::Tag(*tag));
                self.emit(Opcode::LoadConst(tag_const));
                for field in fields {
                    self.emit_node(field)?;
                }
                // StructNew expects tag + n fields on stack
                self.emit(Opcode::StructNew {
                    field_count: fields.len() as u32,
                });
            }

            TypedInner::ConstructorCall(tag, fields) => {
                let tag_const = self.add_constant(Constant::Tag(*tag));
                self.emit(Opcode::LoadConst(tag_const));
                for field in fields {
                    self.emit_node(field)?;
                }
                self.emit(Opcode::StructNew {
                    field_count: fields.len() as u32,
                });
            }

            TypedInner::Block(stmts) => {
                for (i, s) in stmts.iter().enumerate() {
                    self.emit_node(s)?;
                    if i < stmts.len() - 1 {
                        self.emit(Opcode::Pop);
                    }
                }
            }

            TypedInner::Semi(inner) => {
                self.emit_node(inner)?;
                self.emit(Opcode::Pop);
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::DeferrorDef(_, _, _, _, _) => {
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::Def(_fun_idx, _id, _type_params, _params, _ret_ty, _body, _) => {
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }
            TypedInner::ExtractorDef(..) | TypedInner::BuiltinExtractorDecl(..) => {
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }
            TypedInner::TraitDef(..) | TypedInner::TraitImplDef(..) => {
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::Closure(params, captures, body) => {
                let filtered_captures: Vec<ResolvedId> = captures
                    .iter()
                    .filter(|id| self.state.slot_map.contains_key(&id.unique_id))
                    .cloned()
                    .collect();
                let fun_idx = self.reserve_fun_idx();
                self.pending_closures.push(PendingClosure {
                    fun_idx,
                    captures: filtered_captures.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    display: callable_display_for_node(node),
                    signature: ty_to_string(&node.ty),
                });
                self.emit(Opcode::LoadFunctionRef(fun_idx));
                for capture in &filtered_captures {
                    let slot = self.alloc_slot(capture.unique_id);
                    self.emit(Opcode::LoadLocal(slot));
                }
                self.emit(Opcode::CaptureClosure(filtered_captures.len() as u8));
            }

            TypedInner::Capture(target, args) => {
                if !args.is_empty() {
                    return Err(CodegenError {
                        message: "capture calls with arguments should be lowered before codegen"
                            .into(),
                        span: node.span.clone(),
                    });
                }
                self.emit_callable_ref(target)?;
            }

            TypedInner::StructDef(tag, name, field_names, field_policies, _) => {
                self.state.type_registry.register(TypeEntry {
                    tag: *tag,
                    name: name.clone(),
                    kind: TypeKind::Struct,
                    field_names: field_names.clone(),
                    private_flags: field_policies.iter().map(|policy| policy.private).collect(),
                });
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::RecordDef(tag, name, field_names, field_policies, _) => {
                self.state.type_registry.register(TypeEntry {
                    tag: *tag,
                    name: name.clone(),
                    kind: TypeKind::Record,
                    field_names: field_names.clone(),
                    private_flags: field_policies.iter().map(|policy| policy.private).collect(),
                });
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::EnumDef(_, variants) => {
                for variant in variants {
                    self.state.type_registry.register(TypeEntry {
                        tag: variant.tag,
                        name: variant.constructor_name.clone(),
                        kind: TypeKind::EnumVariant,
                        field_names: variant.field_names.clone(),
                        private_flags: vec![false; variant.field_names.len()],
                    });
                }
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }
        }
        Ok(())
    }

    fn emit_list_cons_chain(&mut self, node: &TypedNode) -> Result<(), CodegenError> {
        let mut heads = Vec::new();
        let mut tail = node;

        while let TypedInner::ListCons(head, next_tail) = &tail.node {
            heads.push(head.as_ref());
            tail = next_tail;
        }

        for head in &heads {
            self.emit_node(head)?;
        }
        self.emit_node(tail)?;
        for _ in heads.iter().rev() {
            self.emit(Opcode::ListCons);
        }
        Ok(())
    }

    fn emit_lens_view(
        &mut self,
        node: &TypedNode,
        source: &TypedNode,
        path: &TypedLensPath,
        source_is_result: bool,
    ) -> Result<(), CodegenError> {
        let returns_result = matches!(node.ty, Ty::Result(_, _));

        if source_is_result {
            self.emit_node(source)?;
            let result_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::StoreLocal(result_slot));

            self.emit(Opcode::LoadLocal(result_slot));
            self.emit(Opcode::GetTag);
            let err_tag = self.add_constant(Constant::Tag(1));
            self.emit(Opcode::LoadConst(err_tag));
            self.emit(Opcode::EqTag);

            let ok_label = self.fresh_label();
            let end_label = self.fresh_label();
            self.emit_jump_if_false(ok_label);
            self.emit(Opcode::LoadLocal(result_slot));
            self.emit_jump(end_label);

            self.patch_label(ok_label);

            self.emit(Opcode::LoadLocal(result_slot));
            self.emit(Opcode::GetField { field_index: 0 });
            let current_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::StoreLocal(current_slot));

            self.emit_lens_segments_from_local(current_slot, path, &node.span, Some(end_label))?;

            let ok_tag = self.add_constant(Constant::Tag(0));
            self.emit(Opcode::LoadConst(ok_tag));
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::StructNew { field_count: 1 });

            self.patch_label(end_label);
            return Ok(());
        }

        self.emit_node(source)?;
        let current_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(current_slot));

        if returns_result {
            let end_label = self.fresh_label();
            self.emit_lens_segments_from_local(current_slot, path, &node.span, Some(end_label))?;

            let ok_tag = self.add_constant(Constant::Tag(0));
            self.emit(Opcode::LoadConst(ok_tag));
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::StructNew { field_count: 1 });

            self.patch_label(end_label);
        } else {
            self.emit_lens_segments_from_local(current_slot, path, &node.span, None)?;
            self.emit(Opcode::LoadLocal(current_slot));
        }

        Ok(())
    }

    fn emit_lens_set(
        &mut self,
        node: &TypedNode,
        source: &TypedNode,
        path: &TypedLensPath,
        value: &TypedNode,
        source_is_result: bool,
        mode: TypedLensSetMode,
    ) -> Result<(), CodegenError> {
        self.emit_node(source)?;
        let source_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(source_slot));

        self.emit_node(value)?;
        let value_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(value_slot));

        self.emit_lens_update_from_source_slot(
            node,
            source_slot,
            path,
            source_is_result,
            LensUpdateLeaf::Set {
                value_slot,
                wrap_plain_result: matches!(mode, TypedLensSetMode::WrapPlainResult),
            },
        )
    }

    fn emit_lens_over(
        &mut self,
        node: &TypedNode,
        source: &TypedNode,
        path: &TypedLensPath,
        update_fun: &TypedNode,
        source_is_result: bool,
        mode: TypedLensOverMode,
    ) -> Result<(), CodegenError> {
        self.emit_node(source)?;
        let source_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(source_slot));

        self.emit_callable_ref(update_fun)?;
        let update_fun_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(update_fun_slot));

        self.emit_lens_update_from_source_slot(
            node,
            source_slot,
            path,
            source_is_result,
            LensUpdateLeaf::Over {
                update_fun_slot,
                mode,
                focus_is_result: matches!(path.focus_ty, Ty::Result(_, _)),
            },
        )
    }

    fn emit_lens_update_from_source_slot(
        &mut self,
        node: &TypedNode,
        source_slot: u32,
        path: &TypedLensPath,
        source_is_result: bool,
        leaf: LensUpdateLeaf,
    ) -> Result<(), CodegenError> {
        if !matches!(node.ty, Ty::Result(_, _)) {
            return Err(CodegenError {
                message: "Internal invariant broken: Lens::set/over must return Result".into(),
                span: node.span.clone(),
            });
        }

        let end_label = self.fresh_label();
        let root_slot = if source_is_result {
            self.emit(Opcode::LoadLocal(source_slot));
            self.emit(Opcode::GetTag);
            let err_tag = self.add_constant(Constant::Tag(1));
            self.emit(Opcode::LoadConst(err_tag));
            self.emit(Opcode::EqTag);

            let ok_label = self.fresh_label();
            self.emit_jump_if_false(ok_label);
            self.emit(Opcode::LoadLocal(source_slot));
            self.emit_jump(end_label);
            self.patch_label(ok_label);

            let root_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(source_slot));
            self.emit(Opcode::GetField { field_index: 0 });
            self.emit(Opcode::StoreLocal(root_slot));
            root_slot
        } else {
            source_slot
        };

        self.emit_lens_update_at_path(root_slot, path, 0, leaf, &node.span, end_label)?;

        let ok_tag = self.add_constant(Constant::Tag(0));
        self.emit(Opcode::LoadConst(ok_tag));
        self.emit(Opcode::LoadLocal(root_slot));
        self.emit(Opcode::StructNew { field_count: 1 });

        self.patch_label(end_label);
        Ok(())
    }

    fn emit_lens_update_at_path(
        &mut self,
        current_slot: u32,
        path: &TypedLensPath,
        segment_idx: usize,
        leaf: LensUpdateLeaf,
        span: &Span,
        failure_end: Label,
    ) -> Result<(), CodegenError> {
        if segment_idx == path.segments.len() {
            return self.emit_lens_leaf_update(current_slot, leaf, span, failure_end);
        }

        match &path.segments[segment_idx] {
            TypedLensSegment::Field {
                field_index,
                container_field_count,
                ..
            } => {
                let focus_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(current_slot));
                self.emit(Opcode::GetField {
                    field_index: *field_index,
                });
                self.emit(Opcode::StoreLocal(focus_slot));

                self.emit_lens_update_at_path(
                    focus_slot,
                    path,
                    segment_idx + 1,
                    leaf,
                    span,
                    failure_end,
                )?;

                self.emit(Opcode::LoadLocal(current_slot));
                self.emit(Opcode::GetTag);
                for index in 0..*container_field_count {
                    if index == *field_index {
                        self.emit(Opcode::LoadLocal(focus_slot));
                    } else {
                        self.emit(Opcode::LoadLocal(current_slot));
                        self.emit(Opcode::GetField { field_index: index });
                    }
                }
                self.emit(Opcode::StructNew {
                    field_count: *container_field_count,
                });
                self.emit(Opcode::StoreLocal(current_slot));
            }
            TypedLensSegment::Tuple {
                field_index,
                tuple_len,
                ..
            } => {
                let focus_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(current_slot));
                self.emit(Opcode::GetTupleField {
                    field_index: *field_index,
                });
                self.emit(Opcode::StoreLocal(focus_slot));

                self.emit_lens_update_at_path(
                    focus_slot,
                    path,
                    segment_idx + 1,
                    leaf,
                    span,
                    failure_end,
                )?;

                for index in 0..*tuple_len {
                    if index == *field_index {
                        self.emit(Opcode::LoadLocal(focus_slot));
                    } else {
                        self.emit(Opcode::LoadLocal(current_slot));
                        self.emit(Opcode::GetTupleField { field_index: index });
                    }
                }
                self.emit(Opcode::TupleNew { len: *tuple_len });
                self.emit(Opcode::StoreLocal(current_slot));
            }
            TypedLensSegment::Variant {
                enum_name,
                variant_name,
                variant_tag,
                payload_arity,
                ..
            } => {
                self.emit(Opcode::LoadLocal(current_slot));
                self.emit(Opcode::GetTag);
                let expected_tag = self.add_constant(Constant::Tag(*variant_tag));
                self.emit(Opcode::LoadConst(expected_tag));
                self.emit(Opcode::EqTag);

                let mismatch_label = self.fresh_label();
                let continue_label = self.fresh_label();
                self.emit_jump_if_false(mismatch_label);

                let focus_slot = self.state.next_slot;
                self.state.next_slot += 1;
                match *payload_arity {
                    0 => {
                        let unit_idx = self.add_constant(Constant::Unit);
                        self.emit(Opcode::LoadConst(unit_idx));
                        self.emit(Opcode::StoreLocal(focus_slot));
                    }
                    1 => {
                        self.emit(Opcode::LoadLocal(current_slot));
                        self.emit(Opcode::GetField { field_index: 1 });
                        self.emit(Opcode::StoreLocal(focus_slot));
                    }
                    n => {
                        for index in 0..n {
                            self.emit(Opcode::LoadLocal(current_slot));
                            self.emit(Opcode::GetField {
                                field_index: index + 1,
                            });
                        }
                        self.emit(Opcode::TupleNew { len: n });
                        self.emit(Opcode::StoreLocal(focus_slot));
                    }
                }

                self.emit_lens_update_at_path(
                    focus_slot,
                    path,
                    segment_idx + 1,
                    leaf,
                    span,
                    failure_end,
                )?;

                self.emit(Opcode::LoadLocal(current_slot));
                self.emit(Opcode::GetTag);
                self.emit(Opcode::LoadLocal(current_slot));
                self.emit(Opcode::GetField { field_index: 0 });
                match *payload_arity {
                    0 => {
                        self.emit(Opcode::StructNew { field_count: 1 });
                    }
                    1 => {
                        self.emit(Opcode::LoadLocal(focus_slot));
                        self.emit(Opcode::StructNew { field_count: 2 });
                    }
                    n => {
                        for index in 0..n {
                            self.emit(Opcode::LoadLocal(focus_slot));
                            self.emit(Opcode::GetTupleField { field_index: index });
                        }
                        self.emit(Opcode::StructNew { field_count: n + 1 });
                    }
                }
                self.emit(Opcode::StoreLocal(current_slot));
                self.emit_jump(continue_label);

                self.patch_label(mismatch_label);
                let detail = format!(
                    "Variant mismatch at segment {} ({}) in lens path: expected variant {}::{}, but got a different variant",
                    segment_idx + 1,
                    Self::lens_segment_display(&path.segments[segment_idx]),
                    enum_name,
                    variant_name
                );
                self.emit_variant_mismatch_result(&detail, span);
                self.emit_jump(failure_end);

                self.patch_label(continue_label);
            }
        }
        Ok(())
    }

    fn emit_lens_leaf_update(
        &mut self,
        current_slot: u32,
        leaf: LensUpdateLeaf,
        span: &Span,
        failure_end: Label,
    ) -> Result<(), CodegenError> {
        match leaf {
            LensUpdateLeaf::Set {
                value_slot,
                wrap_plain_result,
            } => {
                if wrap_plain_result {
                    let ok_tag = self.add_constant(Constant::Tag(0));
                    self.emit(Opcode::LoadConst(ok_tag));
                    self.emit(Opcode::LoadLocal(value_slot));
                    self.emit(Opcode::StructNew { field_count: 1 });
                } else {
                    self.emit(Opcode::LoadLocal(value_slot));
                }
                self.emit(Opcode::StoreLocal(current_slot));
            }
            LensUpdateLeaf::Over {
                update_fun_slot,
                mode,
                focus_is_result,
            } => match (mode, focus_is_result) {
                (TypedLensOverMode::FocusValue, true) => {
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::GetTag);
                    let err_tag = self.add_constant(Constant::Tag(1));
                    self.emit(Opcode::LoadConst(err_tag));
                    self.emit(Opcode::EqTag);

                    let ok_label = self.fresh_label();
                    let continue_label = self.fresh_label();
                    self.emit_jump_if_false(ok_label);
                    self.emit_jump(continue_label);

                    self.patch_label(ok_label);
                    self.emit(Opcode::LoadLocal(update_fun_slot));
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::GetField { field_index: 0 });
                    self.emit(Opcode::CallClosure {
                        arity: 1,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                    let update_result_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::StoreLocal(update_result_slot));

                    self.emit(Opcode::LoadLocal(update_result_slot));
                    self.emit(Opcode::GetTag);
                    self.emit(Opcode::LoadConst(err_tag));
                    self.emit(Opcode::EqTag);

                    let update_ok_label = self.fresh_label();
                    self.emit_jump_if_false(update_ok_label);
                    self.emit(Opcode::LoadLocal(update_result_slot));
                    self.emit_jump(failure_end);

                    self.patch_label(update_ok_label);
                    let ok_tag = self.add_constant(Constant::Tag(0));
                    self.emit(Opcode::LoadConst(ok_tag));
                    self.emit(Opcode::LoadLocal(update_result_slot));
                    self.emit(Opcode::GetField { field_index: 0 });
                    self.emit(Opcode::StructNew { field_count: 1 });
                    self.emit(Opcode::StoreLocal(current_slot));
                    self.patch_label(continue_label);
                }
                _ => {
                    self.emit(Opcode::LoadLocal(update_fun_slot));
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::CallClosure {
                        arity: 1,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                    let update_result_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::StoreLocal(update_result_slot));

                    self.emit(Opcode::LoadLocal(update_result_slot));
                    self.emit(Opcode::GetTag);
                    let err_tag = self.add_constant(Constant::Tag(1));
                    self.emit(Opcode::LoadConst(err_tag));
                    self.emit(Opcode::EqTag);

                    let ok_label = self.fresh_label();
                    self.emit_jump_if_false(ok_label);
                    self.emit(Opcode::LoadLocal(update_result_slot));
                    self.emit_jump(failure_end);

                    self.patch_label(ok_label);
                    self.emit(Opcode::LoadLocal(update_result_slot));
                    self.emit(Opcode::GetField { field_index: 0 });
                    self.emit(Opcode::StoreLocal(current_slot));
                }
            },
        }
        Ok(())
    }

    fn emit_lens_segments_from_local(
        &mut self,
        current_slot: u32,
        path: &TypedLensPath,
        span: &Span,
        mismatch_end: Option<Label>,
    ) -> Result<(), CodegenError> {
        for (segment_idx, segment) in path.segments.iter().enumerate() {
            match segment {
                TypedLensSegment::Field { field_index, .. } => {
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::GetField {
                        field_index: *field_index,
                    });
                    self.emit(Opcode::StoreLocal(current_slot));
                }
                TypedLensSegment::Tuple { field_index, .. } => {
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::GetTupleField {
                        field_index: *field_index,
                    });
                    self.emit(Opcode::StoreLocal(current_slot));
                }
                TypedLensSegment::Variant {
                    enum_name,
                    variant_name,
                    variant_tag,
                    payload_arity,
                    ..
                } => {
                    let Some(end_label) = mismatch_end else {
                        return Err(CodegenError {
                            message:
                                "Internal invariant broken: variant lens segment in plain context"
                                    .into(),
                            span: span.clone(),
                        });
                    };

                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::GetTag);
                    let expected_tag = self.add_constant(Constant::Tag(*variant_tag));
                    self.emit(Opcode::LoadConst(expected_tag));
                    self.emit(Opcode::EqTag);

                    let mismatch_label = self.fresh_label();
                    let continue_label = self.fresh_label();
                    self.emit_jump_if_false(mismatch_label);

                    self.emit_variant_payload_extract_to_local(current_slot, *payload_arity);
                    self.emit_jump(continue_label);

                    self.patch_label(mismatch_label);
                    let detail = format!(
                        "Variant mismatch at segment {} ({}) in lens path: expected variant {}::{}, but got a different variant",
                        segment_idx + 1,
                        Self::lens_segment_display(segment),
                        enum_name,
                        variant_name
                    );
                    self.emit_variant_mismatch_result(&detail, span);
                    self.emit_jump(end_label);

                    self.patch_label(continue_label);
                }
            }
        }
        Ok(())
    }

    fn emit_variant_payload_extract_to_local(&mut self, current_slot: u32, payload_arity: u32) {
        match payload_arity {
            0 => {
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
                self.emit(Opcode::StoreLocal(current_slot));
            }
            1 => {
                self.emit(Opcode::LoadLocal(current_slot));
                self.emit(Opcode::GetField { field_index: 1 });
                self.emit(Opcode::StoreLocal(current_slot));
            }
            n => {
                for index in 0..n {
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::GetField {
                        field_index: index + 1,
                    });
                }
                self.emit(Opcode::TupleNew { len: n });
                self.emit(Opcode::StoreLocal(current_slot));
            }
        }
    }

    fn lens_segment_display(segment: &TypedLensSegment) -> String {
        match segment {
            TypedLensSegment::Field { field_name, .. } => format!(".{}", field_name),
            TypedLensSegment::Tuple { field_index, .. } => format!("._{}", field_index),
            TypedLensSegment::Variant { variant_name, .. } => format!(".{}", variant_name),
        }
    }

    fn emit_variant_mismatch_result(&mut self, detail: &str, span: &Span) {
        let err_tag = self.add_constant(Constant::Tag(1));
        self.emit(Opcode::LoadConst(err_tag));
        self.emit_error_value("VariantMismatch", detail, span);
        self.emit(Opcode::StructNew { field_count: 1 });
    }

    fn emit_safebind(&mut self, pat: &TypedPattern, rhs: &TypedNode) -> Result<(), CodegenError> {
        if !matches!(rhs.ty, Ty::Result(_, _)) {
            if matches!(rhs.ty, Ty::List(_)) {
                return self.emit_safebind_from_list(pat, rhs);
            }

            self.emit_node(rhs)?;
            let payload_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::StoreLocal(payload_slot));

            let pattern_fail = self.fresh_label();
            self.emit_pattern_test_from_local(pat, payload_slot, pattern_fail, &rhs.span)?;
            self.emit_pattern_bind_from_local(pat, payload_slot)?;
            let success_label = self.fresh_label();
            self.emit_jump(success_label);

            self.patch_label(pattern_fail);
            self.emit_safebind_pattern_failure(pat, payload_slot, rhs.span.clone())?;

            self.patch_label(success_label);
            let unit_idx = self.add_constant(Constant::Unit);
            self.emit(Opcode::LoadConst(unit_idx));
            return Ok(());
        }

        self.emit_node(rhs)?;

        // Preserve the Result value for tag check and payload extraction.
        let result_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(result_slot));

        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetTag);
        let err_tag = self.add_constant(Constant::Tag(1));
        self.emit(Opcode::LoadConst(err_tag));
        self.emit(Opcode::EqTag);

        let ok_path = self.fresh_label();
        self.emit_jump_if_false(ok_path);

        self.emit_propagate_result_from_local(result_slot, rhs.span.clone())?;

        self.patch_label(ok_path);

        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetField { field_index: 0 });
        let payload_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(payload_slot));

        if let Some(items) = Self::collect_exact_list_pattern_items(pat) {
            let lhs_len = items.len();
            let fail_shorts = (0..lhs_len).map(|_| self.fresh_label()).collect::<Vec<_>>();
            let fail_long = self.fresh_label();
            let fail_mismatch = self.fresh_label();
            let rest_slot = self.emit_exact_list_pattern_test_from_local(
                &items,
                payload_slot,
                &fail_shorts,
                fail_long,
                fail_mismatch,
                &rhs.span,
            )?;
            self.emit_pattern_bind_from_local(pat, payload_slot)?;
            let success_label = self.fresh_label();
            self.emit_jump(success_label);

            for (rhs_len, fail_short) in fail_shorts.into_iter().enumerate() {
                self.patch_label(fail_short);
                self.emit_list_len_mismatch_failure_concrete(
                    lhs_len,
                    rhs_len,
                    ">",
                    rhs.span.clone(),
                )?;
            }

            self.patch_label(fail_long);
            self.emit_list_len_mismatch_failure_rhs_long(lhs_len, rest_slot, rhs.span.clone())?;

            self.patch_label(fail_mismatch);
            self.emit_pattern_mismatch_failure(rhs.span.clone())?;

            self.patch_label(success_label);
            let unit_idx = self.add_constant(Constant::Unit);
            self.emit(Opcode::LoadConst(unit_idx));
            return Ok(());
        }

        let pattern_fail = self.fresh_label();
        self.emit_pattern_test_from_local(pat, payload_slot, pattern_fail, &rhs.span)?;
        self.emit_pattern_bind_from_local(pat, payload_slot)?;
        let success_label = self.fresh_label();
        self.emit_jump(success_label);

        self.patch_label(pattern_fail);
        self.emit_safebind_pattern_failure(pat, payload_slot, rhs.span.clone())?;

        self.patch_label(success_label);
        if matches!(pat, TypedPattern::Wildcard(_)) {
            // no-op
        }
        let unit_idx = self.add_constant(Constant::Unit);
        self.emit(Opcode::LoadConst(unit_idx));
        Ok(())
    }

    fn emit_safebind_from_list(
        &mut self,
        pat: &TypedPattern,
        rhs: &TypedNode,
    ) -> Result<(), CodegenError> {
        self.emit_node(rhs)?;

        let list_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(list_slot));

        if let Some(items) = Self::collect_exact_list_pattern_items(pat) {
            let lhs_len = items.len();
            let fail_shorts = (0..lhs_len).map(|_| self.fresh_label()).collect::<Vec<_>>();
            let fail_long = self.fresh_label();
            let fail_mismatch = self.fresh_label();
            let rest_slot = self.emit_exact_list_pattern_test_from_local(
                &items,
                list_slot,
                &fail_shorts,
                fail_long,
                fail_mismatch,
                &rhs.span,
            )?;
            self.emit_pattern_bind_from_local(pat, list_slot)?;
            let success_label = self.fresh_label();
            self.emit_jump(success_label);

            for (rhs_len, fail_short) in fail_shorts.into_iter().enumerate() {
                self.patch_label(fail_short);
                self.emit_list_len_mismatch_failure_concrete(
                    lhs_len,
                    rhs_len,
                    ">",
                    rhs.span.clone(),
                )?;
            }

            self.patch_label(fail_long);
            self.emit_list_len_mismatch_failure_rhs_long(lhs_len, rest_slot, rhs.span.clone())?;

            self.patch_label(fail_mismatch);
            self.emit_pattern_mismatch_failure(rhs.span.clone())?;

            self.patch_label(success_label);
            let unit_idx = self.add_constant(Constant::Unit);
            self.emit(Opcode::LoadConst(unit_idx));
            return Ok(());
        }

        let pattern_fail = self.fresh_label();
        self.emit_pattern_test_from_local(pat, list_slot, pattern_fail, &rhs.span)?;
        self.emit_pattern_bind_from_local(pat, list_slot)?;
        let success_label = self.fresh_label();
        self.emit_jump(success_label);

        self.patch_label(pattern_fail);
        self.emit_safebind_pattern_failure(pat, list_slot, rhs.span.clone())?;

        self.patch_label(success_label);
        let unit_idx = self.add_constant(Constant::Unit);
        self.emit(Opcode::LoadConst(unit_idx));
        Ok(())
    }

    fn emit_empty_list_failure(&mut self, span: Span) -> Result<(), CodegenError> {
        self.emit_pattern_failure("EmptyList", "Empty List.", span)
    }

    fn emit_safebind_pattern_failure(
        &mut self,
        pat: &TypedPattern,
        value_slot: u32,
        span: Span,
    ) -> Result<(), CodegenError> {
        match pat {
            TypedPattern::As(_, inner, _) => {
                self.emit_safebind_pattern_failure(inner, value_slot, span)
            }
            TypedPattern::ListNil(_) | TypedPattern::ListCons(_, _, _) => {
                self.emit_empty_list_failure(span)
            }
            TypedPattern::Extractor {
                input_ty,
                extractor_ty,
                ..
            } if matches!(extractor_ty, Ty::BuiltinFunc { name, .. } if name == "uncons")
                && matches!(input_ty, Ty::List(_)) =>
            {
                self.emit_empty_list_failure(span)
            }
            TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => {
                self.emit_literal_pattern_mismatch_failure(pat, value_slot, span)
            }
            _ => self.emit_pattern_mismatch_failure(span),
        }
    }

    fn emit_literal_pattern_mismatch_failure(
        &mut self,
        pat: &TypedPattern,
        value_slot: u32,
        span: Span,
    ) -> Result<(), CodegenError> {
        let Some(lhs_value) = literal_pattern_display(pat) else {
            return self.emit_pattern_mismatch_failure(span);
        };

        let prefix_idx = self.add_constant(Constant::Str("Pattern did not match.\t@@lhs=".into()));
        self.emit(Opcode::LoadConst(prefix_idx));
        let lhs_idx = self.add_constant(Constant::Str(lhs_value));
        self.emit(Opcode::LoadConst(lhs_idx));
        self.emit(Opcode::ConcatStr);

        let rhs_prefix_idx = self.add_constant(Constant::Str("\t@@rhs=".into()));
        self.emit(Opcode::LoadConst(rhs_prefix_idx));
        self.emit(Opcode::ConcatStr);

        self.emit(Opcode::LoadLocal(value_slot));
        let inspect_id = Self::builtin_id("inspect").ok_or_else(|| CodegenError {
            message: "Unknown builtin: inspect".into(),
            span: span.clone(),
        })?;
        self.emit(Opcode::CallBuiltin {
            builtin_id: inspect_id,
            arity: 1,
            span_start: span.start as u32,
            span_end: span.end as u32,
        });
        self.emit(Opcode::ConcatStr);

        self.emit_pattern_failure_from_message_stack("PatternMismatch", span)
    }

    fn emit_list_len_mismatch_failure_concrete(
        &mut self,
        lhs_len: usize,
        rhs_len: usize,
        op: &str,
        span: Span,
    ) -> Result<(), CodegenError> {
        self.emit_pattern_failure(
            "IndexOutOfBounds",
            &format!("LHS.len({}) {} RHS.len({})", lhs_len, op, rhs_len),
            span,
        )
    }

    fn emit_list_len_mismatch_failure_rhs_long(
        &mut self,
        lhs_len: usize,
        remainder_slot: u32,
        span: Span,
    ) -> Result<(), CodegenError> {
        let iter_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::LoadLocal(remainder_slot));
        self.emit(Opcode::StoreLocal(iter_slot));

        let rem_count_slot = self.state.next_slot;
        self.state.next_slot += 1;
        let zero_idx = self.add_constant(Constant::Int(int(0)));
        self.emit(Opcode::LoadConst(zero_idx));
        self.emit(Opcode::StoreLocal(rem_count_slot));

        let loop_head = self.fresh_label();
        let loop_done = self.fresh_label();
        self.patch_label(loop_head);
        self.emit(Opcode::LoadLocal(iter_slot));
        self.emit(Opcode::ListIsEmpty);
        self.emit_jump_if_true(loop_done);

        self.emit(Opcode::LoadLocal(rem_count_slot));
        let one_idx = self.add_constant(Constant::Int(int(1)));
        self.emit(Opcode::LoadConst(one_idx));
        self.emit(Opcode::AddInt);
        self.emit(Opcode::StoreLocal(rem_count_slot));

        self.emit(Opcode::LoadLocal(iter_slot));
        self.emit(Opcode::ListTail);
        self.emit(Opcode::StoreLocal(iter_slot));
        self.emit_jump(loop_head);
        self.patch_label(loop_done);

        let rhs_total_slot = self.state.next_slot;
        self.state.next_slot += 1;
        let lhs_idx = self.add_constant(Constant::Int(int(lhs_len as u64)));
        self.emit(Opcode::LoadConst(lhs_idx));
        self.emit(Opcode::LoadLocal(rem_count_slot));
        self.emit(Opcode::AddInt);
        self.emit(Opcode::StoreLocal(rhs_total_slot));

        let prefix_idx =
            self.add_constant(Constant::Str(format!("LHS.len({}) < RHS.len(", lhs_len)));
        self.emit(Opcode::LoadConst(prefix_idx));
        self.emit(Opcode::LoadLocal(rhs_total_slot));
        let to_string_id = Self::builtin_id("to_string").ok_or_else(|| CodegenError {
            message: "Unknown builtin: to_string".into(),
            span: span.clone(),
        })?;
        self.emit(Opcode::CallBuiltin {
            builtin_id: to_string_id,
            arity: 1,
            span_start: span.start as u32,
            span_end: span.end as u32,
        });
        self.emit(Opcode::ConcatStr);
        let suffix_idx = self.add_constant(Constant::Str(")".into()));
        self.emit(Opcode::LoadConst(suffix_idx));
        self.emit(Opcode::ConcatStr);

        self.emit_pattern_failure_from_message_stack("IndexOutOfBounds", span)
    }

    fn emit_pattern_mismatch_failure(&mut self, span: Span) -> Result<(), CodegenError> {
        self.emit_pattern_failure("PatternMismatch", "Pattern did not match.", span)
    }

    fn emit_pattern_failure(
        &mut self,
        kind: &str,
        message: &str,
        span: Span,
    ) -> Result<(), CodegenError> {
        if self.in_function {
            let tag_const = self.add_constant(Constant::Tag(1));
            self.emit(Opcode::LoadConst(tag_const));
            self.emit_error_value(kind, message, &span);
            self.emit(Opcode::StructNew { field_count: 1 });
            self.emit(Opcode::Return);
        } else if self.top_level_returns_result {
            let tag_const = self.add_constant(Constant::Tag(1));
            self.emit(Opcode::LoadConst(tag_const));
            self.emit_error_value(kind, message, &span);
            self.emit(Opcode::StructNew { field_count: 1 });
            self.emit(Opcode::Halt);
        } else {
            self.emit_error_value(kind, message, &span);
            let eprint_id = Self::builtin_id("eprint").ok_or_else(|| CodegenError {
                message: "Unknown builtin: eprint".into(),
                span: span.clone(),
            })?;
            self.emit(Opcode::CallBuiltin {
                builtin_id: eprint_id,
                arity: 1,
                span_start: span.start as u32,
                span_end: span.end as u32,
            });
            self.emit(Opcode::Halt);
        }
        Ok(())
    }

    fn emit_pattern_failure_from_message_stack(
        &mut self,
        kind: &str,
        span: Span,
    ) -> Result<(), CodegenError> {
        if self.in_function {
            let msg_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::StoreLocal(msg_slot));

            let tag_const = self.add_constant(Constant::Tag(1));
            self.emit(Opcode::LoadConst(tag_const));
            self.emit(Opcode::LoadLocal(msg_slot));
            self.emit_error_value_from_stack(kind, &span);
            self.emit(Opcode::StructNew { field_count: 1 });
            self.emit(Opcode::Return);
        } else if self.top_level_returns_result {
            let msg_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::StoreLocal(msg_slot));

            let tag_const = self.add_constant(Constant::Tag(1));
            self.emit(Opcode::LoadConst(tag_const));
            self.emit(Opcode::LoadLocal(msg_slot));
            self.emit_error_value_from_stack(kind, &span);
            self.emit(Opcode::StructNew { field_count: 1 });
            self.emit(Opcode::Halt);
        } else {
            self.emit_error_value_from_stack(kind, &span);
            let eprint_id = Self::builtin_id("eprint").ok_or_else(|| CodegenError {
                message: "Unknown builtin: eprint".into(),
                span: span.clone(),
            })?;
            self.emit(Opcode::CallBuiltin {
                builtin_id: eprint_id,
                arity: 1,
                span_start: span.start as u32,
                span_end: span.end as u32,
            });
            self.emit(Opcode::Halt);
        }
        Ok(())
    }

    fn emit_error_value(&mut self, kind: &str, message: &str, span: &Span) {
        if let Some((fun_idx, arity)) = self.state.error_ctor_funs.get(kind).copied() {
            match arity {
                0 => {
                    self.emit(Opcode::Call {
                        fun_idx,
                        arity: 0,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                    return;
                }
                1 => {
                    let message_idx = self.add_constant(Constant::Str(message.into()));
                    self.emit(Opcode::LoadConst(message_idx));
                    self.emit(Opcode::Call {
                        fun_idx,
                        arity: 1,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                    return;
                }
                _ => {
                    // fall back to literal when constructor arity is not supported here
                }
            }
        }

        let kind_idx = self.add_constant(Constant::Str(kind.into()));
        let message_idx = self.add_constant(Constant::Str(message.into()));
        self.emit(Opcode::MakeErrorLiteral {
            kind_const_idx: kind_idx,
            message_const_idx: message_idx,
        });
    }

    fn emit_error_value_from_stack(&mut self, kind: &str, span: &Span) {
        if let Some((fun_idx, arity)) = self.state.error_ctor_funs.get(kind).copied() {
            match arity {
                1 => {
                    self.emit(Opcode::Call {
                        fun_idx,
                        arity: 1,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                    return;
                }
                0 => {
                    self.emit(Opcode::Pop);
                    self.emit(Opcode::Call {
                        fun_idx,
                        arity: 0,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                    return;
                }
                _ => {
                    // fall through to literal fallback
                }
            }
        }

        let template_id = self.state.error_templates.len() as u32;
        self.state.error_templates.push(ErrTemplate {
            id: template_id,
            kind: kind.into(),
            span_start: span.start as u32,
            span_end: span.end as u32,
            line: 0,
            column: 0,
            format: String::new(),
            num_params: 1,
        });
        self.emit(Opcode::MakeError { template_id });
    }

    fn collect_exact_list_pattern_items(pat: &TypedPattern) -> Option<Vec<&TypedPattern>> {
        fn walk<'a>(pat: &'a TypedPattern, out: &mut Vec<&'a TypedPattern>) -> bool {
            match pat {
                TypedPattern::As(_, inner, _) => walk(inner.as_ref(), out),
                TypedPattern::ListNil(_) => true,
                TypedPattern::ListCons(_, head, tail) => {
                    out.push(head.as_ref());
                    walk(tail.as_ref(), out)
                }
                _ => false,
            }
        }

        let mut out = Vec::new();
        if walk(pat, &mut out) {
            Some(out)
        } else {
            None
        }
    }

    fn emit_exact_list_pattern_test_from_local(
        &mut self,
        items: &[&TypedPattern],
        list_slot: u32,
        fail_shorts: &[Label],
        fail_long: Label,
        fail_mismatch: Label,
        err_span: &Span,
    ) -> Result<u32, CodegenError> {
        if fail_shorts.len() != items.len() {
            return Err(CodegenError {
                message: "internal error: fail_short label count mismatch".into(),
                span: err_span.clone(),
            });
        }

        let mut current_slot = list_slot;

        for (idx, item) in items.iter().enumerate() {
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListIsEmpty);
            self.emit_jump_if_true(fail_shorts[idx]);

            let head_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListHead);
            self.emit(Opcode::StoreLocal(head_slot));
            self.emit_pattern_test_from_local(item, head_slot, fail_mismatch, err_span)?;

            let next_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListTail);
            self.emit(Opcode::StoreLocal(next_slot));
            current_slot = next_slot;
        }

        self.emit(Opcode::LoadLocal(current_slot));
        self.emit(Opcode::ListIsEmpty);
        self.emit_jump_if_false(fail_long);
        Ok(current_slot)
    }

    fn emit_pattern_test_from_local(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
        fail_label: Label,
        err_span: &Span,
    ) -> Result<(), CodegenError> {
        self.emit_pattern_test_from_local_with_mode(pat, slot, fail_label, err_span, true)
    }

    fn emit_pattern_test_from_local_for_bind(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
        fail_label: Label,
        err_span: &Span,
    ) -> Result<(), CodegenError> {
        self.emit_pattern_test_from_local_with_mode(pat, slot, fail_label, err_span, false)
    }

    fn emit_pattern_test_from_local_with_mode(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
        fail_label: Label,
        err_span: &Span,
        propagate_result_error: bool,
    ) -> Result<(), CodegenError> {
        match pat {
            TypedPattern::Var(_, _) | TypedPattern::Wildcard(_) => {}
            TypedPattern::As(_, inner, _) => {
                self.emit_pattern_test_from_local_with_mode(
                    inner,
                    slot,
                    fail_label,
                    err_span,
                    propagate_result_error,
                )?;
            }
            TypedPattern::IntLit(_, n) => {
                self.emit(Opcode::LoadLocal(slot));
                let n_const = self.add_constant(Constant::Int(n.clone()));
                self.emit(Opcode::LoadConst(n_const));
                self.emit(Opcode::EqInt);
                self.emit_jump_if_false(fail_label);
            }
            TypedPattern::Tuple(_, items) => {
                let mut item_slots = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let item_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(slot));
                    self.emit(Opcode::GetTupleField {
                        field_index: index as u32,
                    });
                    self.emit(Opcode::StoreLocal(item_slot));
                    item_slots.push((item, item_slot));
                }
                for (item, item_slot) in item_slots {
                    self.emit_pattern_test_from_local_with_mode(
                        item,
                        item_slot,
                        fail_label,
                        err_span,
                        propagate_result_error,
                    )?;
                }
            }
            TypedPattern::StrLit(_, s) => {
                self.emit(Opcode::LoadLocal(slot));
                let s_const = self.add_constant(Constant::Str(s.clone()));
                self.emit(Opcode::LoadConst(s_const));
                self.emit(Opcode::EqStr);
                self.emit_jump_if_false(fail_label);
            }
            TypedPattern::BoolLit(_, b) => {
                self.emit(Opcode::LoadLocal(slot));
                let b_const = self.add_constant(Constant::Bool(*b));
                self.emit(Opcode::LoadConst(b_const));
                self.emit(Opcode::EqBool);
                self.emit_jump_if_false(fail_label);
            }
            TypedPattern::DurationLit(_, n) => {
                self.emit_duration_lit_pattern_test(slot, n, fail_label);
            }
            TypedPattern::ListNil(_) => {
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListIsEmpty);
                self.emit_jump_if_false(fail_label);
            }
            TypedPattern::ListCons(_, _, _) => {
                self.emit_list_cons_pattern_test_from_local(
                    pat,
                    slot,
                    fail_label,
                    err_span,
                    propagate_result_error,
                )?;
            }
            TypedPattern::ResultOk(_, inner) => {
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::GetTag);
                let expected_tag = if propagate_result_error { 1 } else { 0 };
                let tag_const = self.add_constant(Constant::Tag(expected_tag));
                self.emit(Opcode::LoadConst(tag_const));
                self.emit(Opcode::EqTag);

                if propagate_result_error {
                    let inner_ok = self.fresh_label();
                    self.emit_jump_if_false(inner_ok);
                    self.emit_propagate_result_from_local(slot, err_span.clone())?;
                    self.patch_label(inner_ok);
                } else {
                    self.emit_jump_if_false(fail_label);
                }

                let inner_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::GetField { field_index: 0 });
                self.emit(Opcode::StoreLocal(inner_slot));
                self.emit_pattern_test_from_local_with_mode(
                    inner,
                    inner_slot,
                    fail_label,
                    err_span,
                    propagate_result_error,
                )?;
            }
            TypedPattern::Extractor {
                input_ty,
                extractor,
                extractor_ty,
                success_tag,
                no_match_tag,
                err_tag,
                seq_tys,
                items,
                ..
            } => {
                let item_slots = self.emit_extractor_item_slots_from_local(
                    input_ty,
                    extractor,
                    extractor_ty,
                    *success_tag,
                    *no_match_tag,
                    *err_tag,
                    seq_tys.len(),
                    slot,
                    fail_label,
                    err_span,
                )?;
                for (item, item_slot) in items.iter().zip(item_slots.iter()) {
                    self.emit_pattern_test_from_local_with_mode(
                        item,
                        *item_slot,
                        fail_label,
                        err_span,
                        propagate_result_error,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn emit_pattern_bind_from_local(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
    ) -> Result<(), CodegenError> {
        match pat {
            TypedPattern::Var(_, id) => {
                let bind_slot = self.alloc_slot(id.unique_id);
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::StoreLocal(bind_slot));
            }
            TypedPattern::As(_, inner, id) => {
                let bind_slot = self.alloc_slot(id.unique_id);
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::StoreLocal(bind_slot));
                self.emit_pattern_bind_from_local(inner, slot)?;
            }
            TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => {}
            TypedPattern::Tuple(_, items) => {
                for (index, item) in items.iter().enumerate() {
                    let item_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(slot));
                    self.emit(Opcode::GetTupleField {
                        field_index: index as u32,
                    });
                    self.emit(Opcode::StoreLocal(item_slot));
                    self.emit_pattern_bind_from_local(item, item_slot)?;
                }
            }
            TypedPattern::ListCons(_, _, _) => {
                self.emit_list_cons_pattern_bind_from_local(pat, slot)?;
            }
            TypedPattern::ResultOk(_, inner) => {
                let inner_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::GetField { field_index: 0 });
                self.emit(Opcode::StoreLocal(inner_slot));
                self.emit_pattern_bind_from_local(inner, inner_slot)?;
            }
            TypedPattern::Extractor {
                input_ty,
                extractor,
                extractor_ty,
                success_tag,
                no_match_tag,
                err_tag,
                seq_tys,
                items,
                ..
            } => {
                let impossible_no_match = self.fresh_label();
                let done = self.fresh_label();
                let item_slots = self.emit_extractor_item_slots_from_local(
                    input_ty,
                    extractor,
                    extractor_ty,
                    *success_tag,
                    *no_match_tag,
                    *err_tag,
                    seq_tys.len(),
                    slot,
                    impossible_no_match,
                    &extractor.span,
                )?;
                for (item, item_slot) in items.iter().zip(item_slots.iter()) {
                    self.emit_pattern_bind_from_local(item, *item_slot)?;
                }
                self.emit_jump(done);
                self.patch_label(impossible_no_match);
                self.emit_pattern_mismatch_failure(extractor.span.clone())?;
                self.patch_label(done);
            }
        }
        Ok(())
    }

    fn emit_list_cons_pattern_test_from_local(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
        fail_label: Label,
        err_span: &Span,
        propagate_result_error: bool,
    ) -> Result<(), CodegenError> {
        let mut current_pat = pat;
        let mut current_slot = slot;

        while let TypedPattern::ListCons(_, head, tail) = current_pat {
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListIsEmpty);
            self.emit_jump_if_true(fail_label);

            let head_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListHead);
            self.emit(Opcode::StoreLocal(head_slot));
            self.emit_pattern_test_from_local_with_mode(
                head,
                head_slot,
                fail_label,
                err_span,
                propagate_result_error,
            )?;

            let tail_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListTail);
            self.emit(Opcode::StoreLocal(tail_slot));

            current_pat = tail;
            current_slot = tail_slot;
        }

        self.emit_pattern_test_from_local_with_mode(
            current_pat,
            current_slot,
            fail_label,
            err_span,
            propagate_result_error,
        )
    }

    fn emit_list_cons_pattern_bind_from_local(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
    ) -> Result<(), CodegenError> {
        let mut current_pat = pat;
        let mut current_slot = slot;

        while let TypedPattern::ListCons(_, head, tail) = current_pat {
            let head_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListHead);
            self.emit(Opcode::StoreLocal(head_slot));
            self.emit_pattern_bind_from_local(head, head_slot)?;

            let tail_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListTail);
            self.emit(Opcode::StoreLocal(tail_slot));

            current_pat = tail;
            current_slot = tail_slot;
        }

        self.emit_pattern_bind_from_local(current_pat, current_slot)
    }

    fn reserve_pattern_slots_for_lens_bind(&mut self, pat: &TypedPattern) {
        match pat {
            TypedPattern::Var(_, id) => {
                self.alloc_slot(id.unique_id);
            }
            TypedPattern::As(_, inner, alias) => {
                self.alloc_slot(alias.unique_id);
                self.reserve_pattern_slots_for_lens_bind(inner);
            }
            TypedPattern::Wildcard(_) => {}
            _ => {}
        }
    }

    fn emit_propagate_result_from_local(
        &mut self,
        result_slot: u32,
        span: Span,
    ) -> Result<(), CodegenError> {
        self.emit(Opcode::LoadLocal(result_slot));
        if self.in_function {
            self.emit(Opcode::Return);
        } else if self.top_level_returns_result {
            self.emit(Opcode::Halt);
        } else {
            self.emit(Opcode::GetField { field_index: 0 });
            let eprint_id = Self::builtin_id("eprint").ok_or_else(|| CodegenError {
                message: "Unknown builtin: eprint".into(),
                span: span.clone(),
            })?;
            self.emit(Opcode::CallBuiltin {
                builtin_id: eprint_id,
                arity: 1,
                span_start: span.start as u32,
                span_end: span.end as u32,
            });
            self.emit(Opcode::Halt);
        }
        Ok(())
    }

    fn emit_propagate_error_from_local(
        &mut self,
        error_slot: u32,
        span: Span,
    ) -> Result<(), CodegenError> {
        if self.in_function {
            let tag_const = self.add_constant(Constant::Tag(1));
            self.emit(Opcode::LoadConst(tag_const));
            self.emit(Opcode::LoadLocal(error_slot));
            self.emit(Opcode::StructNew { field_count: 1 });
            self.emit(Opcode::Return);
        } else if self.top_level_returns_result {
            let tag_const = self.add_constant(Constant::Tag(1));
            self.emit(Opcode::LoadConst(tag_const));
            self.emit(Opcode::LoadLocal(error_slot));
            self.emit(Opcode::StructNew { field_count: 1 });
            self.emit(Opcode::Halt);
        } else {
            self.emit(Opcode::LoadLocal(error_slot));
            let eprint_id = Self::builtin_id("eprint").ok_or_else(|| CodegenError {
                message: "Unknown builtin: eprint".into(),
                span: span.clone(),
            })?;
            self.emit(Opcode::CallBuiltin {
                builtin_id: eprint_id,
                arity: 1,
                span_start: span.start as u32,
                span_end: span.end as u32,
            });
            self.emit(Opcode::Halt);
        }
        Ok(())
    }

    fn emit_unpack_seq_payload_from_local(
        &mut self,
        tuple_slot: u32,
        arity: usize,
        _span: &Span,
    ) -> Result<Vec<u32>, CodegenError> {
        if arity == 1 {
            return Ok(vec![tuple_slot]);
        }

        let mut item_slots = Vec::with_capacity(arity);

        for index in 0..arity {
            let item_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(tuple_slot));
            self.emit(Opcode::GetTupleField {
                field_index: index as u32,
            });
            self.emit(Opcode::StoreLocal(item_slot));
            item_slots.push(item_slot);
        }

        Ok(item_slots)
    }

    fn emit_extractor_item_slots_from_local(
        &mut self,
        input_ty: &Ty,
        extractor: &ResolvedId,
        extractor_ty: &Ty,
        success_tag: u32,
        no_match_tag: u32,
        err_tag: u32,
        seq_len: usize,
        input_slot: u32,
        no_match_label: Label,
        span: &Span,
    ) -> Result<Vec<u32>, CodegenError> {
        if let Ty::BuiltinFunc { name, .. } = extractor_ty {
            match name.as_str() {
                "Ok" => {
                    if seq_len != 1 {
                        return Err(CodegenError {
                            message: "Ok extractor must produce exactly one value".into(),
                            span: extractor.span.clone(),
                        });
                    }
                    self.emit(Opcode::LoadLocal(input_slot));
                    self.emit(Opcode::GetTag);
                    let ok_tag = self.add_constant(Constant::Tag(0));
                    self.emit(Opcode::LoadConst(ok_tag));
                    self.emit(Opcode::EqTag);
                    self.emit_jump_if_false(no_match_label);

                    let item_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(input_slot));
                    self.emit(Opcode::GetField { field_index: 0 });
                    self.emit(Opcode::StoreLocal(item_slot));
                    return Ok(vec![item_slot]);
                }
                "Err" => {
                    if seq_len != 1 {
                        return Err(CodegenError {
                            message: "Err extractor must produce exactly one value".into(),
                            span: extractor.span.clone(),
                        });
                    }
                    self.emit(Opcode::LoadLocal(input_slot));
                    self.emit(Opcode::GetTag);
                    let err_tag = self.add_constant(Constant::Tag(1));
                    self.emit(Opcode::LoadConst(err_tag));
                    self.emit(Opcode::EqTag);
                    self.emit_jump_if_false(no_match_label);

                    let item_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(input_slot));
                    self.emit(Opcode::GetField { field_index: 0 });
                    self.emit(Opcode::StoreLocal(item_slot));
                    return Ok(vec![item_slot]);
                }
                "uncons" => {
                    if seq_len != 2 {
                        return Err(CodegenError {
                            message: "uncons extractor must produce exactly two values".into(),
                            span: extractor.span.clone(),
                        });
                    }
                    match input_ty {
                        Ty::List(_) => {
                            self.emit(Opcode::LoadLocal(input_slot));
                            self.emit(Opcode::ListIsEmpty);
                            self.emit_jump_if_true(no_match_label);

                            let head_slot = self.state.next_slot;
                            self.state.next_slot += 1;
                            self.emit(Opcode::LoadLocal(input_slot));
                            self.emit(Opcode::ListHead);
                            self.emit(Opcode::StoreLocal(head_slot));

                            let tail_slot = self.state.next_slot;
                            self.state.next_slot += 1;
                            self.emit(Opcode::LoadLocal(input_slot));
                            self.emit(Opcode::ListTail);
                            self.emit(Opcode::StoreLocal(tail_slot));
                            return Ok(vec![head_slot, tail_slot]);
                        }
                        Ty::Str => {
                            self.emit(Opcode::LoadLocal(input_slot));
                            self.emit(Opcode::StringIsEmpty);
                            self.emit_jump_if_true(no_match_label);

                            let head_slot = self.state.next_slot;
                            self.state.next_slot += 1;
                            self.emit(Opcode::LoadLocal(input_slot));
                            self.emit(Opcode::StringHead);
                            self.emit(Opcode::StoreLocal(head_slot));

                            let tail_slot = self.state.next_slot;
                            self.state.next_slot += 1;
                            self.emit(Opcode::LoadLocal(input_slot));
                            self.emit(Opcode::StringTail);
                            self.emit(Opcode::StoreLocal(tail_slot));
                            return Ok(vec![head_slot, tail_slot]);
                        }
                        other => {
                            return Err(CodegenError {
                                message: format!(
                                    "uncons extractor expects List<...> or String, got {}",
                                    ty_to_string(other)
                                ),
                                span: extractor.span.clone(),
                            });
                        }
                    }
                }
                other => {
                    return Err(CodegenError {
                        message: format!("Unknown builtin extractor: {}", other),
                        span: extractor.span.clone(),
                    });
                }
            }
        }

        let fun_idx = match extractor_ty {
            Ty::UserFunc { fun_idx, .. } => *fun_idx,
            other => {
                return Err(CodegenError {
                    message: format!(
                        "Extractor {} is not codegen-callable: {}",
                        extractor.name,
                        ty_to_string(other)
                    ),
                    span: extractor.span.clone(),
                });
            }
        };

        self.emit(Opcode::LoadLocal(input_slot));
        self.emit(Opcode::Call {
            fun_idx,
            arity: 1,
            span_start: extractor.span.start as u32,
            span_end: extractor.span.end as u32,
        });
        let result_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(result_slot));

        let success_label = self.fresh_label();
        let check_no_match_label = self.fresh_label();
        let check_err_label = self.fresh_label();
        let end_label = self.fresh_label();

        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetTag);
        let success_tag_const = self.add_constant(Constant::Tag(success_tag));
        self.emit(Opcode::LoadConst(success_tag_const));
        self.emit(Opcode::EqTag);
        self.emit_jump_if_false(check_no_match_label);
        self.patch_label(success_label);

        let payload_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetField { field_index: 1 });
        self.emit(Opcode::StoreLocal(payload_slot));
        let item_slots = self.emit_unpack_seq_payload_from_local(payload_slot, seq_len, span)?;
        self.emit_jump(end_label);

        self.patch_label(check_no_match_label);
        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetTag);
        let no_match_tag_const = self.add_constant(Constant::Tag(no_match_tag));
        self.emit(Opcode::LoadConst(no_match_tag_const));
        self.emit(Opcode::EqTag);
        self.emit_jump_if_false(check_err_label);
        self.emit_jump(no_match_label);

        self.patch_label(check_err_label);
        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetTag);
        let err_tag_const = self.add_constant(Constant::Tag(err_tag));
        self.emit(Opcode::LoadConst(err_tag_const));
        self.emit(Opcode::EqTag);
        let invalid_outcome_label = self.fresh_label();
        self.emit_jump_if_false(invalid_outcome_label);

        let error_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetField { field_index: 1 });
        self.emit(Opcode::StoreLocal(error_slot));
        self.emit_propagate_error_from_local(error_slot, span.clone())?;

        self.patch_label(invalid_outcome_label);
        self.emit_pattern_failure(
            "InvalidMatchResult",
            "Extractor returned an unknown MatchResult tag.",
            span.clone(),
        )?;

        self.patch_label(end_label);
        Ok(item_slots)
    }

    fn normalize_function_table(&mut self) -> Result<(), CodegenError> {
        // Invariant: functions[idx].fun_idx == idx.
        // VM relies on O(1) array lookup by fun_idx and fails fast if this invariant is broken.
        self.state.functions.sort_by_key(|entry| entry.fun_idx);
        let mut remap = HashMap::new();
        for (idx, entry) in self.state.functions.iter().enumerate() {
            let old_idx = entry.fun_idx;
            let new_idx = idx as u32;
            if remap.insert(old_idx, new_idx).is_some() {
                return Err(CodegenError {
                    message: format!("Duplicate function index detected: {}", old_idx),
                    span: Span { start: 0, end: 0 },
                });
            }
        }

        for entry in &mut self.state.functions {
            if let Some(new_idx) = remap.get(&entry.fun_idx) {
                entry.fun_idx = *new_idx;
            }
        }

        for ir in &mut self.ir {
            if let IrOp::Op(Opcode::LoadFunctionRef(fun_idx) | Opcode::Call { fun_idx, .. }) = ir {
                if let Some(new_idx) = remap.get(fun_idx) {
                    *fun_idx = *new_idx;
                }
            }
        }

        self.state.next_fun_idx =
            u32::try_from(self.state.functions.len()).map_err(|_| CodegenError {
                message: "function table length exceeds u32".into(),
                span: Span { start: 0, end: 0 },
            })?;

        Ok(())
    }

    // ── Function application ──

    fn emit_app(
        &mut self,
        call_span: Span,
        func: &TypedNode,
        args: &[TypedNode],
    ) -> Result<(), CodegenError> {
        match &func.ty {
            Ty::BuiltinFunc { name, .. } => {
                for arg in args {
                    self.emit_node(arg)?;
                }
                if let Some(opcode) = Self::direct_builtin_opcode(name, args.len()) {
                    self.emit(opcode);
                } else {
                    let builtin_id = Self::builtin_id(name).ok_or_else(|| CodegenError {
                        message: format!("Unknown builtin: {}", name),
                        span: func.span.clone(),
                    })?;
                    self.emit(Opcode::CallBuiltin {
                        builtin_id,
                        arity: args.len() as u8,
                        span_start: call_span.start as u32,
                        span_end: call_span.end as u32,
                    });
                }
            }
            Ty::UserFunc {
                fun_idx, params, ..
            } => {
                if args.len() != params.len() {
                    return Err(CodegenError {
                        message: format!(
                            "function expects {} argument(s), got {}",
                            params.len(),
                            args.len()
                        ),
                        span: func.span.clone(),
                    });
                }
                for arg in args {
                    self.emit_node(arg)?;
                }
                self.emit(Opcode::Call {
                    fun_idx: *fun_idx,
                    arity: args.len() as u8,
                    span_start: call_span.start as u32,
                    span_end: call_span.end as u32,
                });
            }
            Ty::Func(params, _) => {
                if args.len() != params.len() {
                    return Err(CodegenError {
                        message: format!(
                            "function expects {} argument(s), got {}",
                            params.len(),
                            args.len()
                        ),
                        span: func.span.clone(),
                    });
                }
                self.emit_node(func)?;
                for arg in args {
                    self.emit_node(arg)?;
                }
                self.emit(Opcode::CallClosure {
                    arity: args.len() as u8,
                    span_start: call_span.start as u32,
                    span_end: call_span.end as u32,
                });
            }
            _ => {
                return Err(CodegenError {
                    message: "Non-function value in call position".into(),
                    span: func.span.clone(),
                });
            }
        }
        Ok(())
    }

    fn emit_tail_node(&mut self, node: &TypedNode) -> Result<(), CodegenError> {
        let mut tail_node = node;

        loop {
            match &tail_node.node {
                TypedInner::Block(stmts) => {
                    if let Some((last, prefix)) = stmts.split_last() {
                        for stmt in prefix {
                            self.emit_node(stmt)?;
                            self.emit(Opcode::Pop);
                        }
                        tail_node = last;
                        continue;
                    }
                    self.emit_unit_const();
                    self.emit(Opcode::Return);
                    return Ok(());
                }
                _ => break,
            }
        }

        match &tail_node.node {
            TypedInner::If(cond, then, else_opt) => {
                self.emit_node(cond)?;
                match else_opt {
                    Some(else_branch) => {
                        let else_label = self.fresh_label();
                        self.emit_jump_if_false(else_label);
                        self.emit_tail_node(then)?;
                        self.patch_label(else_label);
                        self.emit_tail_node(else_branch)?;
                    }
                    None => {
                        let end_label = self.fresh_label();
                        self.emit_jump_if_false(end_label);
                        self.emit_node(then)?;
                        self.emit(Opcode::Pop);
                        self.patch_label(end_label);
                        self.emit_unit_const();
                        self.emit(Opcode::Return);
                    }
                }
            }
            TypedInner::Match(scrutinee, arms) => {
                if arms.is_empty() {
                    self.emit_pattern_mismatch_failure(scrutinee.span.clone())?;
                    return Ok(());
                }

                self.emit_node(scrutinee)?;

                let scrut_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(scrut_slot));

                let mismatch_label = self.fresh_label();
                let mut arm_labels = Vec::with_capacity(arms.len());
                for _ in arms {
                    arm_labels.push(self.fresh_label());
                }

                for (i, arm) in arms.iter().enumerate() {
                    let next_arm = if i + 1 < arms.len() {
                        arm_labels[i + 1]
                    } else {
                        mismatch_label
                    };

                    let pat = &arm.pattern;
                    self.emit_match_pattern_test(pat, scrut_slot, next_arm)?;
                    self.emit_match_pattern_bind(pat, scrut_slot)?;
                    if let Some(guard) = &arm.guard {
                        self.emit_node(guard)?;
                        self.emit_jump_if_false(next_arm);
                    }
                    self.emit_tail_node(&arm.body)?;

                    if i + 1 < arms.len() {
                        self.patch_label(arm_labels[i + 1]);
                    }
                }

                self.patch_label(mismatch_label);
                self.emit_pattern_mismatch_failure(scrutinee.span.clone())?;
            }
            TypedInner::Semi(inner) => {
                self.emit_node(inner)?;
                self.emit(Opcode::Pop);
                self.emit_unit_const();
                self.emit(Opcode::Return);
            }
            TypedInner::Def(..)
            | TypedInner::ExtractorDef(..)
            | TypedInner::BuiltinExtractorDecl(..)
            | TypedInner::DeferrorDef(..) => {
                self.emit_unit_const();
                self.emit(Opcode::Return);
            }
            _ => {
                self.emit_node(tail_node)?;
                self.emit(Opcode::Return);
            }
        }
        Ok(())
    }

    // ── If ──

    fn emit_if(
        &mut self,
        cond: &TypedNode,
        then: &TypedNode,
        else_opt: &Option<Box<TypedNode>>,
    ) -> Result<(), CodegenError> {
        self.emit_node(cond)?;

        match else_opt {
            Some(else_branch) => {
                let else_label = self.fresh_label();
                let end_label = self.fresh_label();

                self.emit_jump_if_false(else_label);
                self.emit_node(then)?;
                self.emit_jump(end_label);

                // Patch else label to current position
                self.patch_label(else_label);
                self.emit_node(else_branch)?;

                self.patch_label(end_label);
            }
            None => {
                let end_label = self.fresh_label();
                self.emit_jump_if_false(end_label);
                self.emit_node(then)?;
                self.emit(Opcode::Pop); // discard then result for if_then/2
                self.patch_label(end_label);
                // Push Unit
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }
        }
        Ok(())
    }

    fn emit_assert(
        &mut self,
        _node: &TypedNode,
        cond: &TypedNode,
        err: &TypedNode,
    ) -> Result<(), CodegenError> {
        self.emit_node(cond)?;
        let fail_label = self.fresh_label();
        let end_label = self.fresh_label();
        self.emit_jump_if_false(fail_label);
        self.emit_ok_unit_result()?;
        self.emit_jump(end_label);

        self.patch_label(fail_label);
        self.emit_err_result_value(err)?;

        self.patch_label(end_label);
        Ok(())
    }

    fn emit_ensure(
        &mut self,
        node: &TypedNode,
        value: &TypedNode,
        pred: &TypedNode,
        err: &TypedNode,
    ) -> Result<(), CodegenError> {
        self.emit_node(value)?;
        let value_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(value_slot));

        self.emit_callable_ref(pred)?;
        self.emit(Opcode::LoadLocal(value_slot));
        self.emit(Opcode::CallClosure {
            arity: 1,
            span_start: node.span.start as u32,
            span_end: node.span.end as u32,
        });

        let fail_label = self.fresh_label();
        let end_label = self.fresh_label();
        self.emit_jump_if_false(fail_label);
        self.emit_ok_result_local(value_slot)?;
        self.emit_jump(end_label);

        self.patch_label(fail_label);
        self.emit_err_result_value(err)?;

        self.patch_label(end_label);
        Ok(())
    }

    fn emit_recover_kind(
        &mut self,
        node: &TypedNode,
        value: &TypedNode,
        marker: &TypedNode,
        handler: &TypedNode,
    ) -> Result<(), CodegenError> {
        self.emit_node(value)?;
        let result_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(result_slot));

        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetTag);
        let err_tag = self.add_constant(Constant::Tag(1));
        self.emit(Opcode::LoadConst(err_tag));
        self.emit(Opcode::EqTag);

        let err_path = self.fresh_label();
        let end_label = self.fresh_label();
        self.emit_jump_if_true(err_path);
        self.emit(Opcode::LoadLocal(result_slot));
        self.emit_jump(end_label);

        self.patch_label(err_path);
        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetField { field_index: 0 });
        let err_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(err_slot));

        let mismatch_label = self.fresh_label();
        let marker_kind = Self::recover_kind_marker_kind(marker).ok_or_else(|| CodegenError {
            message: "recover_kind marker must resolve to a deferror constructor".into(),
            span: marker.span.clone(),
        })?;
        self.emit_error_kind_test_from_local(err_slot, marker_kind, mismatch_label)?;
        self.emit_callable_ref(handler)?;
        self.emit(Opcode::LoadLocal(err_slot));
        self.emit(Opcode::CallClosure {
            arity: 1,
            span_start: node.span.start as u32,
            span_end: node.span.end as u32,
        });
        self.emit_jump(end_label);

        self.patch_label(mismatch_label);
        self.emit(Opcode::LoadLocal(result_slot));

        self.patch_label(end_label);
        Ok(())
    }

    fn emit_result_error_transform(
        &mut self,
        node: &TypedNode,
        value: &TypedNode,
        err: &TypedNode,
        builtin_name: &str,
    ) -> Result<(), CodegenError> {
        self.emit_node(value)?;
        let result_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(result_slot));

        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetTag);
        let err_tag = self.add_constant(Constant::Tag(1));
        self.emit(Opcode::LoadConst(err_tag));
        self.emit(Opcode::EqTag);

        let err_path = self.fresh_label();
        let end_label = self.fresh_label();
        self.emit_jump_if_true(err_path);
        self.emit(Opcode::LoadLocal(result_slot));
        self.emit_jump(end_label);

        self.patch_label(err_path);
        self.emit(Opcode::LoadLocal(result_slot));
        self.emit_node(err)?;
        let builtin_id = Self::builtin_id(builtin_name).ok_or_else(|| CodegenError {
            message: format!("Unknown builtin: {}", builtin_name),
            span: node.span.clone(),
        })?;
        self.emit(Opcode::CallBuiltin {
            builtin_id,
            arity: 2,
            span_start: node.span.start as u32,
            span_end: node.span.end as u32,
        });

        self.patch_label(end_label);
        Ok(())
    }

    fn recover_kind_marker_kind(marker: &TypedNode) -> Option<&str> {
        match &marker.node {
            TypedInner::Var(id) => Some(id.name.rsplit("::").next().unwrap_or(&id.name)),
            TypedInner::App(func, _) => match &func.node {
                TypedInner::Var(id) => Some(id.name.rsplit("::").next().unwrap_or(&id.name)),
                _ => None,
            },
            _ => None,
        }
    }

    fn emit_error_kind_test_from_local(
        &mut self,
        slot: u32,
        expected_kind: &str,
        fail_label: Label,
    ) -> Result<(), CodegenError> {
        self.emit(Opcode::LoadLocal(slot));
        let kind_id = Self::builtin_id("kind").ok_or_else(|| CodegenError {
            message: "Unknown builtin: kind".into(),
            span: Span { start: 0, end: 0 },
        })?;
        self.emit(Opcode::CallBuiltin {
            builtin_id: kind_id,
            arity: 1,
            span_start: 0,
            span_end: 0,
        });
        let kind_const = self.add_constant(Constant::Str(expected_kind.to_string()));
        self.emit(Opcode::LoadConst(kind_const));
        self.emit(Opcode::EqStr);
        self.emit_jump_if_false(fail_label);
        Ok(())
    }

    fn emit_ok_unit_result(&mut self) -> Result<(), CodegenError> {
        let ok_tag = self.add_constant(Constant::Tag(0));
        let unit_idx = self.add_constant(Constant::Unit);
        self.emit(Opcode::LoadConst(ok_tag));
        self.emit(Opcode::LoadConst(unit_idx));
        self.emit(Opcode::StructNew { field_count: 1 });
        Ok(())
    }

    fn emit_ok_result_local(&mut self, slot: u32) -> Result<(), CodegenError> {
        let ok_tag = self.add_constant(Constant::Tag(0));
        self.emit(Opcode::LoadConst(ok_tag));
        self.emit(Opcode::LoadLocal(slot));
        self.emit(Opcode::StructNew { field_count: 1 });
        Ok(())
    }

    fn emit_err_result_value(&mut self, err: &TypedNode) -> Result<(), CodegenError> {
        let err_tag = self.add_constant(Constant::Tag(1));
        self.emit(Opcode::LoadConst(err_tag));
        self.emit_node(err)?;
        self.emit(Opcode::StructNew { field_count: 1 });
        Ok(())
    }

    fn emit_interpolated_str(
        &mut self,
        parts: &[TypedInterpolatedPart],
    ) -> Result<(), CodegenError> {
        if parts.is_empty() {
            let empty = self.add_constant(Constant::Str(String::new()));
            self.emit(Opcode::LoadConst(empty));
            return Ok(());
        }

        let mut first = true;
        for part in parts {
            match part {
                TypedInterpolatedPart::Text(s) => {
                    let idx = self.add_constant(Constant::Str(s.clone()));
                    self.emit(Opcode::LoadConst(idx));
                }
                TypedInterpolatedPart::Expr(expr) => {
                    self.emit_node(expr)?;
                    let to_string_id =
                        Self::builtin_id("to_string").ok_or_else(|| CodegenError {
                            message: "Unknown builtin: to_string".into(),
                            span: expr.span.clone(),
                        })?;
                    self.emit(Opcode::CallBuiltin {
                        builtin_id: to_string_id,
                        arity: 1,
                        span_start: expr.span.start as u32,
                        span_end: expr.span.end as u32,
                    });
                }
            }

            if first {
                first = false;
            } else {
                self.emit(Opcode::ConcatStr);
            }
        }
        Ok(())
    }

    // ── Match ──

    fn emit_match(
        &mut self,
        scrutinee: &TypedNode,
        arms: &[TypedMatchArm],
    ) -> Result<(), CodegenError> {
        if arms.is_empty() {
            return self.emit_pattern_mismatch_failure(scrutinee.span.clone());
        }

        self.emit_node(scrutinee)?;

        let scrut_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(scrut_slot));

        let end_label = self.fresh_label();
        let mismatch_label = self.fresh_label();
        let mut arm_labels: Vec<Label> = Vec::new();

        for _ in arms {
            arm_labels.push(self.fresh_label());
        }

        for (i, arm) in arms.iter().enumerate() {
            let next_arm = if i + 1 < arms.len() {
                arm_labels[i + 1]
            } else {
                mismatch_label
            };

            let pat = &arm.pattern;
            self.emit_match_pattern_test(pat, scrut_slot, next_arm)?;
            self.emit_match_pattern_bind(pat, scrut_slot)?;
            if let Some(guard) = &arm.guard {
                self.emit_node(guard)?;
                self.emit_jump_if_false(next_arm);
            }

            // Emit body
            self.emit_node(&arm.body)?;
            self.emit_jump(end_label);

            // Patch next arm label
            if i + 1 < arms.len() {
                self.patch_label(arm_labels[i + 1]);
            }
        }

        self.patch_label(mismatch_label);
        self.emit_pattern_mismatch_failure(scrutinee.span.clone())?;
        self.patch_label(end_label);
        Ok(())
    }

    fn emit_match_pattern_test(
        &mut self,
        pat: &TypedMatchPattern,
        slot: u32,
        fail_label: Label,
    ) -> Result<(), CodegenError> {
        match pat {
            TypedMatchPattern::Binding(_) | TypedMatchPattern::Wildcard => {}
            TypedMatchPattern::As(inner, _) => {
                self.emit_match_pattern_test(inner, slot, fail_label)?;
            }
            TypedMatchPattern::BoolLit(b) => {
                self.emit(Opcode::LoadLocal(slot));
                let bool_const = self.add_constant(Constant::Bool(*b));
                self.emit(Opcode::LoadConst(bool_const));
                self.emit(Opcode::EqBool);
                self.emit_jump_if_false(fail_label);
            }
            TypedMatchPattern::IntLit(n) => {
                self.emit(Opcode::LoadLocal(slot));
                let int_const = self.add_constant(Constant::Int(n.clone()));
                self.emit(Opcode::LoadConst(int_const));
                self.emit(Opcode::EqInt);
                self.emit_jump_if_false(fail_label);
            }
            TypedMatchPattern::StrLit(s) => {
                self.emit(Opcode::LoadLocal(slot));
                let str_const = self.add_constant(Constant::Str(s.clone()));
                self.emit(Opcode::LoadConst(str_const));
                self.emit(Opcode::EqStr);
                self.emit_jump_if_false(fail_label);
            }
            TypedMatchPattern::DurationLit(n) => {
                self.emit_duration_lit_pattern_test(slot, n, fail_label);
            }
            TypedMatchPattern::ErrorKind(kind) => {
                self.emit_error_kind_test_from_local(slot, kind, fail_label)?;
            }
            TypedMatchPattern::Or(items) => {
                let success_label = self.fresh_label();
                for item in items {
                    let next_label = self.fresh_label();
                    self.emit_match_pattern_test(item, slot, next_label)?;
                    self.emit_jump(success_label);
                    self.patch_label(next_label);
                }
                self.emit_jump(fail_label);
                self.patch_label(success_label);
            }
            TypedMatchPattern::Tuple(items) => {
                let mut item_slots = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let item_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(slot));
                    self.emit(Opcode::GetTupleField {
                        field_index: index as u32,
                    });
                    self.emit(Opcode::StoreLocal(item_slot));
                    item_slots.push((item, item_slot));
                }
                for (item, item_slot) in item_slots {
                    self.emit_match_pattern_test(item, item_slot, fail_label)?;
                }
            }
            TypedMatchPattern::Constructor {
                tag,
                fields,
                field_offset,
            } => {
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::GetTag);
                let tag_const = self.add_constant(Constant::Tag(*tag));
                self.emit(Opcode::LoadConst(tag_const));
                self.emit(Opcode::EqTag);
                self.emit_jump_if_false(fail_label);

                for (idx, field_pat) in fields.iter().enumerate() {
                    let inner_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(slot));
                    self.emit(Opcode::GetField {
                        field_index: *field_offset + idx as u32,
                    });
                    self.emit(Opcode::StoreLocal(inner_slot));
                    self.emit_match_pattern_test(field_pat, inner_slot, fail_label)?;
                }
            }
            TypedMatchPattern::ListNil => {
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListIsEmpty);
                self.emit_jump_if_false(fail_label);
            }
            TypedMatchPattern::ListCons(_, _) => {
                self.emit_list_cons_match_pattern_test(pat, slot, fail_label)?;
            }
            TypedMatchPattern::Extractor {
                input_ty,
                extractor,
                extractor_ty,
                success_tag,
                no_match_tag,
                err_tag,
                seq_tys,
                items,
            } => {
                let item_slots = self.emit_extractor_item_slots_from_local(
                    input_ty,
                    extractor,
                    extractor_ty,
                    *success_tag,
                    *no_match_tag,
                    *err_tag,
                    seq_tys.len(),
                    slot,
                    fail_label,
                    &extractor.span,
                )?;
                for (item, item_slot) in items.iter().zip(item_slots.iter()) {
                    self.emit_match_pattern_test(item, *item_slot, fail_label)?;
                }
            }
        }
        Ok(())
    }

    fn emit_match_pattern_bind(
        &mut self,
        pat: &TypedMatchPattern,
        slot: u32,
    ) -> Result<(), CodegenError> {
        match pat {
            TypedMatchPattern::Binding(id) => {
                let bind_slot = self.alloc_slot(id.unique_id);
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::StoreLocal(bind_slot));
            }
            TypedMatchPattern::As(inner, alias) => {
                let bind_slot = self.alloc_slot(alias.unique_id);
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::StoreLocal(bind_slot));
                self.emit_match_pattern_bind(inner, slot)?;
            }
            TypedMatchPattern::Wildcard
            | TypedMatchPattern::BoolLit(_)
            | TypedMatchPattern::IntLit(_)
            | TypedMatchPattern::StrLit(_)
            | TypedMatchPattern::DurationLit(_)
            | TypedMatchPattern::ErrorKind(_)
            | TypedMatchPattern::Or(_)
            | TypedMatchPattern::ListNil => {}
            TypedMatchPattern::Tuple(items) => {
                for (index, item) in items.iter().enumerate() {
                    let item_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(slot));
                    self.emit(Opcode::GetTupleField {
                        field_index: index as u32,
                    });
                    self.emit(Opcode::StoreLocal(item_slot));
                    self.emit_match_pattern_bind(item, item_slot)?;
                }
            }
            TypedMatchPattern::Constructor {
                fields,
                field_offset,
                ..
            } => {
                for (idx, field_pat) in fields.iter().enumerate() {
                    let inner_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(slot));
                    self.emit(Opcode::GetField {
                        field_index: *field_offset + idx as u32,
                    });
                    self.emit(Opcode::StoreLocal(inner_slot));
                    self.emit_match_pattern_bind(field_pat, inner_slot)?;
                }
            }
            TypedMatchPattern::ListCons(_, _) => {
                self.emit_list_cons_match_pattern_bind(pat, slot)?;
            }
            TypedMatchPattern::Extractor {
                input_ty,
                extractor,
                extractor_ty,
                success_tag,
                no_match_tag,
                err_tag,
                seq_tys,
                items,
            } => {
                let impossible_no_match = self.fresh_label();
                let done = self.fresh_label();
                let item_slots = self.emit_extractor_item_slots_from_local(
                    input_ty,
                    extractor,
                    extractor_ty,
                    *success_tag,
                    *no_match_tag,
                    *err_tag,
                    seq_tys.len(),
                    slot,
                    impossible_no_match,
                    &extractor.span,
                )?;
                for (item, item_slot) in items.iter().zip(item_slots.iter()) {
                    self.emit_match_pattern_bind(item, *item_slot)?;
                }
                self.emit_jump(done);
                self.patch_label(impossible_no_match);
                self.emit_pattern_mismatch_failure(extractor.span.clone())?;
                self.patch_label(done);
            }
        }
        Ok(())
    }

    fn emit_list_cons_match_pattern_test(
        &mut self,
        pat: &TypedMatchPattern,
        slot: u32,
        fail_label: Label,
    ) -> Result<(), CodegenError> {
        let mut current_pat = pat;
        let mut current_slot = slot;

        while let TypedMatchPattern::ListCons(head, tail) = current_pat {
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListIsEmpty);
            self.emit_jump_if_true(fail_label);

            let head_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListHead);
            self.emit(Opcode::StoreLocal(head_slot));
            self.emit_match_pattern_test(head, head_slot, fail_label)?;

            let tail_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListTail);
            self.emit(Opcode::StoreLocal(tail_slot));

            current_pat = tail;
            current_slot = tail_slot;
        }

        self.emit_match_pattern_test(current_pat, current_slot, fail_label)
    }

    fn emit_list_cons_match_pattern_bind(
        &mut self,
        pat: &TypedMatchPattern,
        slot: u32,
    ) -> Result<(), CodegenError> {
        let mut current_pat = pat;
        let mut current_slot = slot;

        while let TypedMatchPattern::ListCons(head, tail) = current_pat {
            let head_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListHead);
            self.emit(Opcode::StoreLocal(head_slot));
            self.emit_match_pattern_bind(head, head_slot)?;

            let tail_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListTail);
            self.emit(Opcode::StoreLocal(tail_slot));

            current_pat = tail;
            current_slot = tail_slot;
        }

        self.emit_match_pattern_bind(current_pat, current_slot)
    }

    // ── Label resolution ──

    /// Mark a label as pointing to the current IR position.
    fn patch_label(&mut self, label: Label) {
        self.label_positions.insert(label, self.ir.len());
    }

    // ── Helpers ──

    fn lit_to_constant(&self, lit: &Lit) -> Constant {
        match lit {
            Lit::Int(n) => Constant::Int(n.clone()),
            Lit::Float(f) => Constant::Float(*f),
            Lit::Str(s) => Constant::Str(s.clone()),
            Lit::Bool(b) => Constant::Bool(*b),
            Lit::Unit => Constant::Unit,
        }
    }

    fn emit_enum_eq(
        &mut self,
        op: &BinOp,
        left: &TypedNode,
        right: &TypedNode,
    ) -> Result<(), CodegenError> {
        self.emit_node(left)?;
        let left_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(left_slot));

        self.emit_node(right)?;
        let right_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(right_slot));

        self.emit(Opcode::LoadLocal(left_slot));
        self.emit(Opcode::GetField { field_index: 0 });
        self.emit(Opcode::LoadLocal(right_slot));
        self.emit(Opcode::GetField { field_index: 0 });
        self.emit(Opcode::EqInt);
        if matches!(op, BinOp::Neq) {
            self.emit(Opcode::NotBool);
        }
        Ok(())
    }

    fn emit_duration_payload_from_local(&mut self, slot: u32) {
        self.emit(Opcode::LoadLocal(slot));
        self.emit(Opcode::GetField { field_index: 0 });
    }

    fn emit_duration_lit_pattern_test(
        &mut self,
        slot: u32,
        millis: &sindr::primitives::SurtrInt,
        fail_label: Label,
    ) {
        self.emit_duration_payload_from_local(slot);
        let millis_const = self.add_constant(Constant::Int(millis.clone()));
        self.emit(Opcode::LoadConst(millis_const));
        self.emit(Opcode::EqInt);
        self.emit_jump_if_false(fail_label);
    }

    fn binop_to_opcode(
        &self,
        op: &BinOp,
        left_ty: &Ty,
        span: &Span,
    ) -> Result<Opcode, CodegenError> {
        match (op, left_ty) {
            (BinOp::Add, Ty::Int) => Ok(Opcode::AddInt),
            (BinOp::Sub, Ty::Int) => Ok(Opcode::SubInt),
            (BinOp::Mul, Ty::Int) => Ok(Opcode::MulInt),
            (BinOp::Add, Ty::Float) => Ok(Opcode::AddFloat),
            (BinOp::Sub, Ty::Float) => Ok(Opcode::SubFloat),
            (BinOp::Mul, Ty::Float) => Ok(Opcode::MulFloat),
            (BinOp::Eq, Ty::Int) => Ok(Opcode::EqInt),
            (BinOp::Neq, Ty::Int) => Ok(Opcode::NeqInt),
            (BinOp::Lt, Ty::Int) => Ok(Opcode::LtInt),
            (BinOp::Gt, Ty::Int) => Ok(Opcode::GtInt),
            (BinOp::Lte, Ty::Int) => Ok(Opcode::LteInt),
            (BinOp::Gte, Ty::Int) => Ok(Opcode::GteInt),
            (BinOp::Eq, Ty::Float) => Ok(Opcode::EqFloat),
            (BinOp::Neq, Ty::Float) => Ok(Opcode::NeqFloat),
            (BinOp::Lt, Ty::Float) => Ok(Opcode::LtFloat),
            (BinOp::Gt, Ty::Float) => Ok(Opcode::GtFloat),
            (BinOp::Lte, Ty::Float) => Ok(Opcode::LteFloat),
            (BinOp::Gte, Ty::Float) => Ok(Opcode::GteFloat),
            (BinOp::Eq, Ty::Str) => Ok(Opcode::EqStr),
            (BinOp::Neq, Ty::Str) => Ok(Opcode::NeqStr),
            (BinOp::Eq, Ty::Bool) => Ok(Opcode::EqBool),
            (BinOp::Neq, Ty::Bool) => Ok(Opcode::NeqBool),
            (BinOp::Concat, Ty::Str) => Ok(Opcode::ConcatStr),
            _ => Err(CodegenError {
                message: format!("Unsupported binop {:?} for type", op),
                span: span.clone(),
            }),
        }
    }

    // ── Finish: resolve labels → absolute addresses ──

    fn finalize(self) -> Result<(Vec<Opcode>, CodegenState), CodegenError> {
        // Resolve labels to absolute IR indices → opcode positions.
        // IR ops map 1:1 to opcodes, so IR index == opcode index.
        let mut opcodes = Vec::new();
        for ir_op in &self.ir {
            match ir_op {
                IrOp::Op(op) => opcodes.push(op.clone()),
                IrOp::JumpLabel(label) => {
                    let pos =
                        self.label_positions
                            .get(label)
                            .copied()
                            .ok_or_else(|| CodegenError {
                                message: format!("unresolved jump label {:?}", label),
                                span: Span { start: 0, end: 0 },
                            })? as u32;
                    opcodes.push(Opcode::Jump(pos));
                }
                IrOp::JumpIfFalseLabel(label) => {
                    let pos =
                        self.label_positions
                            .get(label)
                            .copied()
                            .ok_or_else(|| CodegenError {
                                message: format!("unresolved jump-if-false label {:?}", label),
                                span: Span { start: 0, end: 0 },
                            })? as u32;
                    opcodes.push(Opcode::JumpIfFalse(pos));
                }
                IrOp::JumpIfTrueLabel(label) => {
                    let pos =
                        self.label_positions
                            .get(label)
                            .copied()
                            .ok_or_else(|| CodegenError {
                                message: format!("unresolved jump-if-true label {:?}", label),
                                span: Span { start: 0, end: 0 },
                            })? as u32;
                    opcodes.push(Opcode::JumpIfTrue(pos));
                }
            }
        }
        Ok((opcodes, self.state))
    }
}

fn literal_pattern_display(pat: &TypedPattern) -> Option<String> {
    match pat {
        TypedPattern::As(_, inner, _) => literal_pattern_display(inner),
        TypedPattern::IntLit(_, value) => Some(value.to_string()),
        TypedPattern::StrLit(_, value) => Some(quote_surtr_string_literal(value)),
        TypedPattern::BoolLit(_, value) => Some(if *value {
            "True".to_string()
        } else {
            "False".to_string()
        }),
        TypedPattern::DurationLit(_, value) => Some(format!("{value}ms")),
        _ => None,
    }
}

fn quote_surtr_string_literal(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod process_runtime_v2_tests {
    use super::*;
    use sigil::resolved::ResolvedId;
    use spire::ast::{ProcessKind, ProcessRuntimeHandlerSpec, ProcessSpec};

    fn span(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    fn singleton_process_spec(name: &str) -> TypedProcessSpec {
        TypedProcessSpec {
            module_path: name.to_string(),
            process_name: name.to_string(),
            spec: ProcessSpec {
                process_name: name.to_string(),
                kind: ProcessKind::Agent,
                instance: ProcessInstance::Singleton,
                boot: false,
                registry: false,
                lazy: false,
                handlers: Vec::new(),
                handler_specs: vec![
                    ProcessRuntimeHandlerSpec {
                        name: "init".into(),
                        internal_name: "__agent_init".into(),
                        kind: ProcessRuntimeHandlerKind::Init,
                        span: span(0, 0),
                    },
                    ProcessRuntimeHandlerSpec {
                        name: "log".into(),
                        internal_name: "__agent_get".into(),
                        kind: ProcessRuntimeHandlerKind::Get,
                        span: span(0, 0),
                    },
                ],
                supervisor_policy: None,
            },
            init_uid: 1,
            get_uid: 2,
            set_uid: None,
            handler_uids: Vec::new(),
        }
    }

    fn singleton_surface_call(qualified_name: &str) -> TypedNode {
        TypedNode {
            ty: Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Error)),
            span: span(10, 21),
            node: TypedInner::App(
                Box::new(TypedNode {
                    ty: Ty::UserFunc {
                        fun_idx: 2,
                        type_params: Vec::new(),
                        params: vec![Ty::Str],
                        ret: Box::new(Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Error))),
                    },
                    span: span(10, 21),
                    node: TypedInner::Var(ResolvedId {
                        name: qualified_name.to_string(),
                        qualified_name: Some(qualified_name.to_string()),
                        unique_id: 2,
                        compiler_generated: true,
                        span: span(10, 21),
                    }),
                }),
                vec![TypedNode {
                    ty: Ty::Str,
                    span: span(22, 27),
                    node: TypedInner::Lit(Lit::Str("hello".into())),
                }],
            ),
        }
    }

    #[test]
    fn validate_required_singletons_rejects_direct_call_when_absent_from_boot_plan() {
        let err = validate_required_singletons(
            &[singleton_surface_call("Logger::log")],
            &[singleton_process_spec("Logger")],
            &RuntimeBootPlan::default(),
        )
        .expect_err("direct singleton surface call should require supervisor_init");

        assert!(err.message.contains("singleton `Logger` is not available"));
        assert!(err.message.contains("supervisor_init"));
        assert_eq!(err.span, span(10, 21));
    }

    #[test]
    fn validate_required_singletons_accepts_direct_call_when_booted() {
        let mut boot_plan = RuntimeBootPlan::default();
        boot_plan.singletons.push(SingletonBootEntry {
            process_name: "Logger".into(),
            init_timeout_ms: boot_plan.runtime_limits.default_init_timeout_ms,
            source: BootEntrySource::ExplicitConfig,
        });

        validate_required_singletons(
            &[singleton_surface_call("Logger::log")],
            &[singleton_process_spec("Logger")],
            &boot_plan,
        )
        .expect("supervisor_init singleton should satisfy direct singleton call");
    }
}
