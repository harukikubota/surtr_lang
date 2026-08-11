use std::collections::{HashMap, HashSet};

use scar::typed::*;
use scar::types::{FacetKind, Ty};
use sigil::resolved::ResolvedId;
use sindr::builtin::builtin_id_by_name;
use sindr::ir::{
    validate_chunk_function_table, validate_program_function_table,
    validate_type_registry_append_entries, BootEntrySource, CallableTemplate, CallableTemplateArg,
    CallableTemplateComposeFlavor, CallableTemplateDirectTarget, CallableTemplateKind,
    CallableTemplateMetadata, CompileInfo, DbgArgTemplate, DbgTemplate, DocEntry, FunctionFlags,
    RuntimeBootPlan, RuntimeCallableRef, RuntimeHandlerArg, RuntimeHandlerDependency,
    RuntimeHandlerKind, RuntimeHandlerOverride, RuntimeHandlerSpec, RuntimeHandlerTarget,
    RuntimeInitPolicy, RuntimeInitResultShape, RuntimeInitSpec, RuntimeLifecycleSpec,
    RuntimeProcessDependencies, RuntimeStateSpec, RuntimeSupervisionSpec,
    RuntimeSupervisorOverrideEntry, RuntimeSupervisorPolicy, RuntimeTypeRef, SingletonBootEntry,
};
use sindr::names::{surface_path_name, surface_rendered_name};
use sindr::primitives::{int, SurtrInt};
use sindr::runtime::{quote_surtr_string_literal, CallableOrigin};
use spire::ast::{
    AstTy, BinOp, Lit, ProcessInstance, ProcessKind, ProcessRuntimeHandlerKind, Span,
    SupervisorInitSpec, Visibility,
};

use crate::bytecode::*;
use crate::error::CodegenError;
use crate::opcode::Opcode;
use crate::registry::{TypeEntry, TypeKind, TypeRegistry};

const DYNAMIC_SUPERVISOR_PROCESS_NAME: &str = "DynamicSupervisor";
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
    let bytecode = Bytecode {
        opcodes,
        constants: state.constants,
        num_locals: state.next_slot as usize,
        type_registry: state.type_registry,
        error_templates: state.error_templates,
        dbg_templates: state.dbg_templates,
        callable_templates: state.callable_templates,
        functions: state.functions,
        source_map: None,
        docs: Vec::new(),
        signatures: Vec::new(),
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
    };
    validate_program_function_table(
        &bytecode.opcodes,
        &bytecode.callable_templates,
        &bytecode.runtime_process_specs.entries,
        &bytecode.functions,
    )
    .map_err(codegen_validation_error)?;
    Ok(bytecode)
}

fn codegen_validation_error(error: impl std::fmt::Display) -> CodegenError {
    CodegenError {
        message: error.to_string(),
        span: Span { start: 0, end: 0 },
    }
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
        if matches!(
            spec.spec.kind,
            spire::ast::ProcessKind::Supervisor
                | spire::ast::ProcessKind::DynamicSupervisor
                | spire::ast::ProcessKind::RuntimeSupervisor
        ) {
            for method in ["status", "spawn", "adopt", "workers"] {
                surface_to_process.insert(
                    format!("{}::{method}", spec.process_name),
                    format!("{}::{method}", spec.process_name),
                );
            }
        } else {
            surface_to_process.insert(
                format!("{}::pid", spec.process_name),
                spec.process_name.clone(),
            );
        }
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
        .map(|entry| entry.process_name.clone())
        .collect::<HashSet<_>>();
    let mut available_supervisors = runtime_boot_plan
        .supervisor_overrides
        .iter()
        .map(|entry| entry.process_name.clone())
        .collect::<HashSet<_>>();
    for spec in process_specs {
        if spec.spec.kind == spire::ast::ProcessKind::DynamicSupervisor {
            available_supervisors.insert(spec.process_name.clone());
        }
    }
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
        let message = if process_name.contains("::") {
            format!(
                "supervisor surface `{process_name}` is not available in this compile unit; add the supervisor to supervisor_init"
            )
        } else {
            format!(
                "singleton `{process_name}` is not available in this compile unit; add it to supervisor_init"
            )
        };
        return Err(CodegenError { message, span });
    }

    Ok(())
}

fn collect_missing_singleton_calls(
    node: &TypedNode,
    surface_to_process: &HashMap<String, String>,
    available_singletons: &HashSet<String>,
    available_supervisors: &HashSet<String>,
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
        | TypedInner::FacetPath(_)
        | TypedInner::PendingFacetPath(_)
        | TypedInner::EnumDef(_, _)
        | TypedInner::TraitDef(..)
        | TypedInner::TraitImplDef(..)
        | TypedInner::BuiltinExtractorDecl(_, _, _)
        | TypedInner::StructDef(_, _, _, _, _)
        | TypedInner::RecordDef(_, _, _, _, _) => {}
        TypedInner::EagerBoundary(inner) => collect_missing_singleton_calls(
            inner,
            surface_to_process,
            available_singletons,
            available_supervisors,
            first_missing,
        ),
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
            strategy,
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
                strategy,
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
        TypedInner::HashMapLiteral(entries) => {
            for (key, value) in entries {
                collect_missing_singleton_calls(
                    key,
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
        TypedInner::FacetView { source, .. } => collect_missing_singleton_calls(
            source,
            surface_to_process,
            available_singletons,
            available_supervisors,
            first_missing,
        ),
        TypedInner::FacetSet { source, value, .. } => {
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
        TypedInner::FacetOver {
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
        | TypedInner::Def(_, _, _, _, _, _, body, _)
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
    mut chunk: BytecodeChunk,
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
    let type_registry_base = base.type_registry.entries().len();
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
    if chunk.type_registry_base as usize != type_registry_base {
        return Err(CodegenError {
            message: format!(
                "chunk type registry base mismatch: chunk={}, base={}",
                chunk.type_registry_base, type_registry_base
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

    rebase_chunk_callable_template_ids(
        &mut chunk.opcodes,
        &mut chunk.callable_templates,
        base.callable_templates.len(),
    )?;
    rebase_chunk_function_ids(
        &mut chunk.opcodes,
        &mut chunk.callable_templates,
        &mut chunk.functions,
        &mut chunk.runtime_process_specs,
        base.functions.len(),
    )?;
    validate_chunk_function_table(
        &chunk.opcodes,
        &chunk.callable_templates,
        &chunk.runtime_process_specs,
        &chunk.functions,
        base.functions.len(),
        false,
    )
    .map_err(codegen_validation_error)?;
    validate_type_registry_append_entries(base.type_registry.entries(), &chunk.type_entries)
        .map_err(codegen_validation_error)?;

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
    base.type_registry
        .try_extend(chunk.type_entries)
        .map_err(codegen_validation_error)?;
    base.error_templates.extend(chunk.error_templates);
    base.dbg_templates.extend(chunk.dbg_templates);
    base.callable_templates.extend(chunk.callable_templates);
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

fn rebase_chunk_callable_template_ids(
    opcodes: &mut [Opcode],
    templates: &mut [CallableTemplate],
    base_template_len: usize,
) -> Result<(), CodegenError> {
    let Some(template_floor) = templates
        .iter()
        .map(|template| template.template_id as usize)
        .min()
    else {
        return Ok(());
    };
    if template_floor <= base_template_len {
        return Ok(());
    }
    let delta = template_floor - base_template_len;
    for opcode in opcodes.iter_mut() {
        if let Opcode::LoadCallableTemplateRef(template_id) = opcode {
            let template_idx = *template_id as usize;
            if template_idx >= template_floor {
                *template_id = u32::try_from(template_idx - delta).map_err(|_| CodegenError {
                    message: "callable template index exceeds u32 after rebasing".into(),
                    span: Span { start: 0, end: 0 },
                })?;
            }
        }
    }
    for template in templates.iter_mut() {
        let template_idx = template.template_id as usize;
        if template_idx >= template_floor {
            template.template_id =
                u32::try_from(template_idx - delta).map_err(|_| CodegenError {
                    message: "callable template id exceeds u32 after rebasing".into(),
                    span: Span { start: 0, end: 0 },
                })?;
        }
    }
    Ok(())
}

fn rebase_chunk_function_ids(
    opcodes: &mut [Opcode],
    templates: &mut [CallableTemplate],
    functions: &mut [FunctionEntry],
    runtime_process_specs: &mut [RuntimeProcessSpec],
    base_function_len: usize,
) -> Result<(), CodegenError> {
    let Some(function_floor) = functions.iter().map(|entry| entry.fun_idx as usize).min() else {
        return Ok(());
    };
    if function_floor <= base_function_len {
        return Ok(());
    }
    let delta = function_floor - base_function_len;

    let rebase_fun_idx = |fun_idx: &mut u32| -> Result<(), CodegenError> {
        let current = *fun_idx as usize;
        if current >= function_floor {
            *fun_idx = u32::try_from(current - delta).map_err(|_| CodegenError {
                message: "function index exceeds u32 after rebasing".into(),
                span: Span { start: 0, end: 0 },
            })?;
        }
        Ok(())
    };

    for opcode in opcodes.iter_mut() {
        match opcode {
            Opcode::LoadFunctionRef(fun_idx) | Opcode::Call { fun_idx, .. } => {
                rebase_fun_idx(fun_idx)?;
            }
            _ => {}
        }
    }

    for template in templates.iter_mut() {
        match &mut template.kind {
            CallableTemplateKind::PartialDirectCall { target, .. }
            | CallableTemplateKind::InjectDirectCall { target, .. } => {
                if let CallableTemplateDirectTarget::Function(fun_idx) = target {
                    rebase_fun_idx(fun_idx)?;
                }
            }
            CallableTemplateKind::ComposeDirect { .. } => {}
        }
    }

    for entry in functions.iter_mut() {
        rebase_fun_idx(&mut entry.fun_idx)?;
    }

    for spec in runtime_process_specs.iter_mut() {
        rebase_fun_idx(&mut spec.init.callable.fun_idx)?;
        for handler in &mut spec.handlers {
            rebase_fun_idx(&mut handler.fun_idx)?;
        }
    }

    Ok(())
}

fn extend_runtime_boot_plan(base: &mut RuntimeBootPlan, chunk: RuntimeBootPlan) {
    base.singletons.extend(chunk.singletons);
    base.standard_overrides.extend(chunk.standard_overrides);
    base.handler_overrides.extend(chunk.handler_overrides);
    base.supervisor_overrides.extend(chunk.supervisor_overrides);
}

fn build_runtime_boot_plan(
    boot_plan: &SupervisorInitSpec,
    process_specs: &[TypedProcessSpec],
) -> Result<RuntimeBootPlan, CodegenError> {
    let mut runtime = RuntimeBootPlan::default();
    let default_timeout_ms = runtime.runtime_limits.default_init_timeout_ms;

    for entry in &boot_plan.entries {
        let spec =
            match resolve_boot_process_spec(process_specs, &entry.process_name, &entry.span) {
                Ok(spec) => spec,
                Err(err) if entry.process_name == DYNAMIC_SUPERVISOR_PROCESS_NAME => {
                    if entry.timeout_ms.is_some() || !entry.handlers.is_empty() {
                        return Err(CodegenError {
                        message:
                            "supervisor_init supervisor entry does not accept timeout or handlers"
                                .into(),
                        span: entry.span.clone(),
                    });
                    }
                    if runtime.supervisor_overrides.iter().any(|registered| {
                        registered.process_name == DYNAMIC_SUPERVISOR_PROCESS_NAME
                    }) {
                        return Err(CodegenError {
                            message: "supervisor_init entry is duplicated".into(),
                            span: entry.span.clone(),
                        });
                    }
                    let base_policy = default_dynamic_supervisor_policy();
                    runtime
                        .supervisor_overrides
                        .push(RuntimeSupervisorOverrideEntry {
                            process_name: DYNAMIC_SUPERVISOR_PROCESS_NAME.into(),
                            policy: runtime_supervisor_policy_from_effective(
                                &base_policy,
                                &entry.overrides,
                            ),
                        });
                    let _ = err;
                    continue;
                }
                Err(err) => return Err(err),
            };
        match spec.spec.instance {
            ProcessInstance::Worker => {
                return Err(CodegenError {
                    message: "worker process cannot appear in supervisor_init".into(),
                    span: entry.span.clone(),
                });
            }
            ProcessInstance::Singleton
                if matches!(
                    spec.spec.kind,
                    spire::ast::ProcessKind::Supervisor
                        | spire::ast::ProcessKind::DynamicSupervisor
                        | spire::ast::ProcessKind::RuntimeSupervisor
                ) =>
            {
                if entry.timeout_ms.is_some() || !entry.handlers.is_empty() {
                    return Err(CodegenError {
                        message:
                            "supervisor_init supervisor entry does not accept timeout or handlers"
                                .into(),
                        span: entry.span.clone(),
                    });
                }
                let Some(base_policy) = &spec.spec.supervisor_policy else {
                    return Err(CodegenError {
                        message: "supervisor process is missing a policy definition".into(),
                        span: entry.span.clone(),
                    });
                };
                if runtime
                    .supervisor_overrides
                    .iter()
                    .any(|registered| registered.process_name == spec.process_name)
                {
                    return Err(CodegenError {
                        message: "supervisor_init entry is duplicated".into(),
                        span: entry.span.clone(),
                    });
                }
                runtime
                    .supervisor_overrides
                    .push(RuntimeSupervisorOverrideEntry {
                        process_name: runtime_supervisor_process_name(spec),
                        policy: runtime_supervisor_policy_from_effective(
                            base_policy,
                            &entry.overrides,
                        ),
                    });
            }
            ProcessInstance::Singleton => {
                if entry.overrides != Default::default() {
                    return Err(CodegenError {
                        message:
                            "supervisor_init singleton entry does not accept supervisor policy keys"
                                .into(),
                        span: entry.span.clone(),
                    });
                }
                add_runtime_singleton_entry(
                    &mut runtime,
                    spec,
                    entry.timeout_ms,
                    &entry.handlers,
                    &entry.span,
                    default_timeout_ms,
                )?;
            }
        }
    }

    for singleton in &boot_plan.singletons {
        let spec =
            resolve_boot_process_spec(process_specs, &singleton.process_name, &singleton.span)?;
        if spec.spec.instance != ProcessInstance::Singleton {
            return Err(CodegenError {
                message: "only Singleton process can appear in singleton boot entry".into(),
                span: singleton.span.clone(),
            });
        }
        add_runtime_singleton_entry(
            &mut runtime,
            spec,
            singleton.timeout_ms,
            &singleton.handlers,
            &singleton.span,
            default_timeout_ms,
        )?;
    }

    for supervisor in &boot_plan.supervisors {
        let spec =
            resolve_boot_process_spec(process_specs, &supervisor.process_name, &supervisor.span)?;
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
                process_name: runtime_supervisor_process_name(spec),
                policy: runtime_supervisor_policy_from_effective(
                    base_policy,
                    &supervisor.overrides,
                ),
            });
    }

    Ok(runtime)
}

fn runtime_supervisor_process_name(spec: &TypedProcessSpec) -> String {
    if spec.spec.kind == ProcessKind::DynamicSupervisor {
        surface_path_name(&spec.process_name).to_string()
    } else {
        spec.process_name.clone()
    }
}

fn add_runtime_singleton_entry(
    runtime: &mut RuntimeBootPlan,
    spec: &TypedProcessSpec,
    timeout_ms: Option<u64>,
    handlers: &[spire::ast::SupervisorInitHandlerOverride],
    span: &Span,
    default_timeout_ms: u64,
) -> Result<(), CodegenError> {
    if runtime
        .singletons
        .iter()
        .any(|entry| entry.process_name == spec.process_name)
    {
        return Err(CodegenError {
            message: "singleton boot entry is duplicated".into(),
            span: span.clone(),
        });
    }

    runtime.singletons.push(SingletonBootEntry {
        process_name: spec.process_name.clone(),
        init_timeout_ms: timeout_ms.unwrap_or(default_timeout_ms),
        source: BootEntrySource::ExplicitConfig,
    });
    for handler in handlers {
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
    Ok(())
}

fn resolve_boot_process_spec<'a>(
    process_specs: &'a [TypedProcessSpec],
    requested_name: &str,
    span: &Span,
) -> Result<&'a TypedProcessSpec, CodegenError> {
    let exact = process_specs
        .iter()
        .filter(|spec| spec.process_name == requested_name)
        .collect::<Vec<_>>();
    let matches = if exact.is_empty() {
        process_specs
            .iter()
            .filter(|spec| {
                spec.process_name
                    .rsplit("::")
                    .next()
                    .is_some_and(|short| short == requested_name)
            })
            .collect::<Vec<_>>()
    } else {
        exact
    };
    match matches.as_slice() {
        [spec] => Ok(spec),
        [] => Err(CodegenError {
            message: format!("process `{requested_name}` is not defined or not visible"),
            span: span.clone(),
        }),
        _ => Err(CodegenError {
            message: format!("process name `{requested_name}` is ambiguous"),
            span: span.clone(),
        }),
    }
}

fn default_dynamic_supervisor_policy() -> spire::ast::SupervisorPolicy {
    spire::ast::SupervisorPolicy {
        strategy: spire::ast::SupervisorStrategy::OneForOne,
        max_restarts: 10,
        max_seconds: 5,
        child_restart_default: spire::ast::ChildRestartPolicy::Transient,
        allow_adopt: true,
        shutdown_timeout_ms: None,
    }
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
        TypedInner::Def(_, id, _, _, ret_ty, _, _, _) if id.unique_id == uid => Some(ret_ty),
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
        Ty::Enum(name, args) if name == "StandbyInit" || name.ends_with("::StandbyInit") => {
            args.first().cloned().ok_or_else(|| CodegenError {
                message: format!(
                    "Standby @init for process `{process_name}` must return Result<StandbyInit<State>>"
                ),
                span: Span { start: 0, end: 0 },
            })
        }
        _ => Err(CodegenError {
            message: format!(
                "Standby @init for process `{process_name}` must return Result<StandbyInit<State>>"
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

fn runtime_type_ref_from_ast(ty: &AstTy) -> RuntimeTypeRef {
    fn ast_ty_to_string(ty: &AstTy) -> String {
        match ty {
            AstTy::Named(_, name) | AstTy::ImplTrait(_, name) => name.clone(),
            AstTy::Generic(_, name, args) => format!(
                "{}<{}>",
                name,
                args.iter()
                    .map(ast_ty_to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Tuple(_, items) => format!(
                "({})",
                items
                    .iter()
                    .map(ast_ty_to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Func(_, params, ret) => {
                let params = params
                    .iter()
                    .map(ast_ty_to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                if params.is_empty() {
                    format!("(-> {})", ast_ty_to_string(ret))
                } else {
                    format!("({} -> {})", params, ast_ty_to_string(ret))
                }
            }
        }
    }

    RuntimeTypeRef {
        name: ast_ty_to_string(ty),
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
            TypedInner::Def(_, id, _, _, _, _, _, _)
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
        let mut set_name = None;
        if let Some(set_uid) = spec.set_uid {
            let lowered_set_name = qualified_names.get(&set_uid).ok_or_else(|| CodegenError {
                message: format!(
                    "missing lowered set handler metadata for process `{}`",
                    spec.process_name
                ),
                span: Span { start: 0, end: 0 },
            })?;
            let bytecode_set_entry =
                function_entries
                    .get(lowered_set_name)
                    .ok_or_else(|| CodegenError {
                        message: format!(
                            "missing bytecode set handler for process `{}`",
                            spec.process_name
                        ),
                        span: Span { start: 0, end: 0 },
                    })?;
            set_name = Some(lowered_set_name.clone());
            set_entry = Some(bytecode_set_entry);
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
                let set_name = set_name.ok_or_else(|| CodegenError {
                    message: format!(
                        "missing lowered set handler metadata for process `{}`",
                        spec.process_name
                    ),
                    span: Span { start: 0, end: 0 },
                })?;
                handler_specs.push(RuntimeHandlerSpec {
                    handler_id: 2,
                    name: set_name,
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
        let _state_ty = init_state_ty(init_ret_ty, spec.spec.standby, &spec.process_name)?;
        let state_type = runtime_type_ref_from_ast(&spec.spec.state);
        let result_type = runtime_type_ref(init_ret_ty);
        let init_policy = if spec.spec.standby {
            RuntimeInitPolicy::Standby
        } else {
            RuntimeInitPolicy::Eager
        };
        let result_shape = if spec.spec.standby {
            RuntimeInitResultShape::StandbyProcessInit {
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
            Opcode::JumpIfLocalTagEq { target_pc, .. }
            | Opcode::JumpIfLocalTagNe { target_pc, .. }
                if *target_pc as usize >= insertion_pc =>
            {
                *target_pc = add_u32(*target_pc, inserted_len, "base jump relocation")?;
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
            Opcode::JumpIfLocalTagEq {
                tag_const_idx,
                target_pc,
                ..
            }
            | Opcode::JumpIfLocalTagNe {
                tag_const_idx,
                target_pc,
                ..
            } => {
                *target_pc = map_chunk_pc(*target_pc, chunk_halt, base_top_len, chunk_func_base)?;
                *tag_const_idx =
                    tag_const_idx
                        .checked_add(const_base)
                        .ok_or_else(|| CodegenError {
                            message: "chunk const relocation overflow".into(),
                            span: Span { start: 0, end: 0 },
                        })?;
            }
            Opcode::Jump(addr) | Opcode::JumpIfFalse(addr) | Opcode::JumpIfTrue(addr) => {
                *addr = map_chunk_pc(*addr, chunk_halt, base_top_len, chunk_func_base)?;
            }
            Opcode::LoadConst(idx)
            | Opcode::StoreConstLocal { const_idx: idx, .. }
            | Opcode::EqLocalTag {
                tag_const_idx: idx, ..
            } => {
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
    pub facet_info: Option<ReplFacetInfo>,
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
pub struct ReplFacetSegmentInfo {
    pub label: String,
    pub kind: String,
    pub source_ty: String,
    pub focus_ty: String,
    pub fallible: bool,
    pub reason: String,
    pub policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplFacetInfo {
    pub ty: String,
    pub stage: ReplFacetStage,
    pub path_kind: String,
    pub source_ty: String,
    pub focus_ty: String,
    pub update_source_ty: String,
    pub update_focus_ty: String,
    pub api_eligibility: Vec<String>,
    pub view_result_ty: String,
    pub full_path: String,
    pub segments: Vec<ReplFacetSegmentInfo>,
    pub stop_points: Vec<String>,
    pub operation: Option<ReplFacetOperation>,
    pub root_policy: String,
    pub available_in_current_scope: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplFacetOperation {
    pub name: String,
    pub kind_constraint: String,
    pub result_ty: String,
    pub replacement_ty: Option<String>,
    pub mapper_ty: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplFacetStage {
    Template,
    Pending,
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
    pub result_facet_info: Option<ReplFacetInfo>,
    pub type_defs: Vec<TypeDefDisplay>,
    pub function_defs: Vec<String>,
    pub docs: Vec<DocEntry>,
}

#[derive(Debug, Clone)]
struct CodegenState {
    constants: Vec<Constant>,
    slot_map: HashMap<u32, u32>, // unique_id → local slot
    callable_defs: HashMap<u32, DirectCallableTarget>, // unique_id -> direct callable target
    callable_names: HashMap<String, DirectCallableTarget>, // qualified/bare name -> direct target
    next_slot: u32,
    next_fun_idx: u32,
    type_registry: TypeRegistry,
    error_templates: Vec<ErrTemplate>,
    dbg_templates: Vec<DbgTemplate>,
    callable_templates: Vec<CallableTemplate>,
    functions: Vec<FunctionEntry>,
    error_ctor_funs: HashMap<String, (u32, u8)>, // error kind -> (fun_idx, arity)
}

impl CodegenState {
    fn new() -> Self {
        Self {
            constants: Vec::new(),
            slot_map: HashMap::new(),
            callable_defs: HashMap::new(),
            callable_names: HashMap::new(),
            next_slot: 0,
            next_fun_idx: 0,
            type_registry: TypeRegistry::new(),
            error_templates: Vec::new(),
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
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
                callable_defs: HashMap::new(),
                callable_names: HashMap::new(),
                next_slot: bytecode.num_locals as u32,
                next_fun_idx,
                type_registry: bytecode.type_registry.clone(),
                error_templates: bytecode.error_templates.clone(),
                dbg_templates: bytecode.dbg_templates.clone(),
                callable_templates: bytecode.callable_templates.clone(),
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
        let base_function_len = self.state.functions.len().saturating_sub(functions.len());
        let chunk = BytecodeChunk {
            runtime_process_specs,
            runtime_boot_plan,
            ..chunk
        };
        validate_chunk_function_table(
            &chunk.opcodes,
            &chunk.callable_templates,
            &chunk.runtime_process_specs,
            &chunk.functions,
            base_function_len,
            false,
        )
        .map_err(codegen_validation_error)?;
        Ok((chunk, meta))
    }

    fn codegen_chunk_nodes_with_options(
        &mut self,
        typed: Vec<TypedNode>,
        top_level_returns_result: bool,
    ) -> Result<(BytecodeChunk, ChunkMeta, Vec<FunctionEntry>), CodegenError> {
        let before = self.state.clone();
        let typed_for_meta = typed.clone();
        let const_base = before.constants.len();
        let type_registry_base = before.type_registry.entries().len();
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
            after.type_registry.entries()[before.type_registry.entries().len()..].to_vec();
        let error_templates = after.error_templates[before.error_templates.len()..].to_vec();
        let dbg_templates = after.dbg_templates[before.dbg_templates.len()..].to_vec();
        let callable_templates =
            after.callable_templates[before.callable_templates.len()..].to_vec();
        let meta = collect_chunk_meta(&typed_for_meta, &after.slot_map);
        let functions = after.functions[before.functions.len()..].to_vec();

        self.state = after;

        let const_base = u32::try_from(const_base).map_err(|_| CodegenError {
            message: "constant base exceeds u32".into(),
            span: Span { start: 0, end: 0 },
        })?;
        let type_registry_base = u32::try_from(type_registry_base).map_err(|_| CodegenError {
            message: "type registry base exceeds u32".into(),
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
                type_registry_base,
                type_entries,
                error_template_base,
                error_templates,
                dbg_template_base,
                dbg_templates,
                callable_templates,
                functions: functions.clone(),
                docs: Vec::new(),
                signatures: Vec::new(),
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
            Opcode::JumpIfLocalTagEq { tag_const_idx, .. }
            | Opcode::JumpIfLocalTagNe { tag_const_idx, .. } => {
                let idx_usize = *tag_const_idx as usize;
                if idx_usize < const_base {
                    return Err(CodegenError {
                        message: format!(
                            "chunk constant index {} is below base {}",
                            idx_usize, const_base
                        ),
                        span: Span { start: 0, end: 0 },
                    });
                }
                *tag_const_idx -= const_base as u32;
            }
            Opcode::LoadConst(idx)
            | Opcode::StoreConstLocal { const_idx: idx, .. }
            | Opcode::EqLocalTag {
                tag_const_idx: idx, ..
            } => {
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
    use super::{
        compose_bytecode_with_chunk, format_function_signature, localize_chunk_indices,
        ty_to_string, Codegen, ForgeSession, MatchPatternDecomp, MatchPatternDecompChild,
        PatternDecomp, PatternDecompChild,
    };
    use crate::bytecode::{Bytecode, BytecodeChunk, CompileInfo, Constant, ErrTemplate};
    use crate::opcode::Opcode;
    use scar::typed::TypedProcessHandlerUid;
    use scar::typed::{
        ComposeFlavor, TypedDbgArg, TypedFacetPath, TypedFacetPathKind, TypedFacetSegment,
        TypedFunParam, TypedInner, TypedMatchArm, TypedMatchPattern, TypedNode, TypedPattern,
        TypedProcessSpec, TypedProgram, TypedTypeParam,
    };
    use scar::types::Ty;
    use sigil::resolved::ResolvedId;
    use sindr::ir::{
        BootEntrySource, CallableTemplate, CallableTemplateComposeFlavor,
        CallableTemplateDirectTarget, CallableTemplateKind, DbgTemplate, DocEntry, DocKind,
        FunctionEntry, FunctionFlags, RuntimeBootPlan, RuntimeCallableRef, RuntimeInitPolicy,
        RuntimeInitResultShape, RuntimeInitSpec, RuntimeProcessInstance, RuntimeProcessKind,
        RuntimeProcessSpec, RuntimeProcessSpecTable, RuntimeStateSpec, RuntimeSupervisionSpec,
        RuntimeTypeRef, SingletonBootEntry,
    };
    use sindr::runtime::{TypeEntry, TypeKind, TypeRegistry};
    use spire::ast::{
        AstTy, BinOp, Lit, ProcessInstance, ProcessKind, ProcessRuntimeHandlerSpec, ProcessSpec,
        Span, SupervisorInitEntry, SupervisorInitSpec, Visibility,
    };

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

    fn resolved_id(name: &str, qualified_name: Option<&str>, unique_id: u32) -> ResolvedId {
        ResolvedId {
            name: name.into(),
            qualified_name: qualified_name.map(str::to_string),
            unique_id,
            compiler_generated: false,
            symbol_info: None,
            span: span(0, 0),
        }
    }

    fn typed_fun_param(name: &str, unique_id: u32, ty: Ty) -> TypedFunParam {
        TypedFunParam {
            id: resolved_id(name, None, unique_id),
            ty,
        }
    }

    #[test]
    fn ty_to_string_uses_surface_names_for_runtime_display_types() {
        let user_ty = Ty::Struct("Global::User".into(), Vec::new());
        assert_eq!(ty_to_string(&user_ty), "User");

        let option_ty = Ty::Enum("Global::Option".into(), vec![user_ty.clone()]);
        assert_eq!(ty_to_string(&option_ty), "Option<User>");

        let pid_ty = Ty::Pid("Global::Worker".into());
        assert_eq!(ty_to_string(&pid_ty), "PID<Worker>");
    }

    #[test]
    fn format_function_signature_preserves_generic_surface_names() {
        let type_params = vec![TypedTypeParam {
            name: "$A".into(),
            ty_var: 42,
            bound: None,
        }];
        let range_ty = Ty::Struct(
            "Global::Range".into(),
            vec![("min".into(), Ty::Var(42)), ("max".into(), Ty::Var(42))],
        );
        let params = vec![
            typed_fun_param("min", 1, Ty::Var(42)),
            typed_fun_param("max", 2, Ty::Var(42)),
        ];

        assert_eq!(
            format_function_signature("new", &type_params, &params, &range_ty),
            "new<$A>(min: $A, max: $A) -> Range<$A>"
        );
    }

    fn local_var(name: &str, unique_id: u32, ty: Ty) -> TypedNode {
        TypedNode {
            ty,
            span: span(0, 0),
            node: TypedInner::Var(resolved_id(name, None, unique_id)),
        }
    }

    fn qualified_var(name: &str, qualified_name: &str, unique_id: u32, ty: Ty) -> TypedNode {
        TypedNode {
            ty,
            span: span(0, 0),
            node: TypedInner::Var(resolved_id(name, Some(qualified_name), unique_id)),
        }
    }

    fn function_entry(fun_idx: u32, entry_pc: u32, end_pc: u32) -> FunctionEntry {
        FunctionEntry {
            fun_idx,
            entry_pc,
            num_locals: 0,
            arity: 0,
            qualified_name: Some(format!("Global::f{fun_idx}")),
            signature: None,
            end_pc,
            span_start: 0,
            span_end: 0,
            flags: FunctionFlags::default(),
        }
    }

    fn err_template(id: u32, kind: &str) -> ErrTemplate {
        ErrTemplate {
            id,
            kind: kind.into(),
            span_start: 0,
            span_end: 0,
            line: 0,
            column: 0,
            format: "{message}".into(),
            num_params: 0,
        }
    }

    fn dbg_template(id: u32) -> DbgTemplate {
        DbgTemplate {
            id,
            span_start: 0,
            span_end: 0,
            source_name: None,
            args: Vec::new(),
        }
    }

    fn doc_entry(qualified_name: &str) -> DocEntry {
        DocEntry {
            qualified_name: qualified_name.into(),
            kind: DocKind::Function,
            module_path: "Global".into(),
            signature: Some("() -> Unit".into()),
            doc: format!("doc for {qualified_name}"),
        }
    }

    fn type_entry(tag: u32, name: &str) -> TypeEntry {
        TypeEntry {
            tag,
            name: name.into(),
            kind: TypeKind::Struct,
            field_names: vec!["value".into()],
            private_flags: vec![false],
        }
    }

    fn runtime_process_spec(process_name: &str, fun_idx: u32) -> RuntimeProcessSpec {
        RuntimeProcessSpec {
            process_id: 0,
            type_name: process_name.into(),
            kind: RuntimeProcessKind::Agent,
            instance: RuntimeProcessInstance::Singleton,
            state: RuntimeStateSpec {
                state_type: RuntimeTypeRef { name: "Int".into() },
            },
            init: RuntimeInitSpec {
                callable: RuntimeCallableRef { fun_idx },
                policy: RuntimeInitPolicy::Eager,
                result_shape: RuntimeInitResultShape::EagerState {
                    result_type: RuntimeTypeRef { name: "Int".into() },
                },
                state_type: RuntimeTypeRef { name: "Int".into() },
                init_route: None,
            },
            handlers: Vec::new(),
            dependencies: Default::default(),
            lifecycle: Default::default(),
            supervision: RuntimeSupervisionSpec {
                parent: Some("RuntimeSupervisor".into()),
                children: Vec::new(),
                policy: None,
            },
        }
    }

    fn singleton_boot(name: &str) -> SingletonBootEntry {
        SingletonBootEntry {
            process_name: name.into(),
            init_timeout_ms: RuntimeBootPlan::default()
                .runtime_limits
                .default_init_timeout_ms,
            source: BootEntrySource::ExplicitConfig,
        }
    }

    fn base_bytecode() -> Bytecode {
        Bytecode {
            opcodes: vec![
                Opcode::LoadConst(0),
                Opcode::Jump(5),
                Opcode::JumpIfFalse(5),
                Opcode::JumpIfLocalTagEq {
                    local_idx: 0,
                    tag_const_idx: 1,
                    target_pc: 5,
                },
                Opcode::LoadConst(1),
                Opcode::Halt,
                Opcode::LoadConst(0),
                Opcode::Return,
                Opcode::LoadConst(1),
                Opcode::Return,
            ],
            constants: vec![Constant::Bool(true), Constant::Tag(7)],
            num_locals: 2,
            type_registry: TypeRegistry::from_entries(vec![type_entry(2, "Global::BaseType")]),
            error_templates: vec![err_template(0, "Global::BaseError")],
            dbg_templates: vec![dbg_template(0)],
            callable_templates: Vec::new(),
            functions: vec![function_entry(0, 6, 8), function_entry(1, 8, 10)],
            source_map: None,
            docs: vec![doc_entry("Global::base_doc")],
            signatures: Vec::new(),
            compile_info: CompileInfo::default(),
            labels: Vec::new(),
            imports: Vec::new(),
            exports: Vec::new(),
            literals: Vec::new(),
            lines: Vec::new(),
            spans: Vec::new(),
            sources: Vec::new(),
            pc_spans: Vec::new(),
            runtime_process_specs: RuntimeProcessSpecTable {
                entries: vec![runtime_process_spec("Global::BaseAgent", 0)],
            },
            runtime_boot_plan: RuntimeBootPlan {
                singletons: vec![singleton_boot("Global::BaseAgent")],
                ..RuntimeBootPlan::default()
            },
        }
    }

    fn relocatable_chunk() -> BytecodeChunk {
        BytecodeChunk {
            opcodes: vec![
                Opcode::LoadConst(0),
                Opcode::JumpIfTrue(4),
                Opcode::JumpIfLocalTagNe {
                    local_idx: 0,
                    tag_const_idx: 1,
                    target_pc: 4,
                },
                Opcode::LoadConst(1),
                Opcode::Halt,
                Opcode::MakeError { template_id: 0 },
                Opcode::Dbg {
                    template_id: 0,
                    arg_count: 0,
                },
                Opcode::LoadConst(2),
                Opcode::MakeErrorLiteral {
                    kind_const_idx: 3,
                    message_const_idx: 4,
                },
                Opcode::Return,
                Opcode::LoadConst(1),
                Opcode::Return,
            ],
            source_map: None,
            const_base: 2,
            constants: vec![
                Constant::Int(5.into()),
                Constant::Tag(9),
                Constant::Str("payload".into()),
                Constant::Str("ChunkError".into()),
                Constant::Str("boom".into()),
            ],
            new_locals: 3,
            type_registry_base: 1,
            type_entries: vec![type_entry(3, "Global::ChunkType")],
            error_template_base: 1,
            error_templates: vec![err_template(1, "Global::ChunkError")],
            dbg_template_base: 1,
            dbg_templates: vec![dbg_template(1)],
            callable_templates: Vec::new(),
            functions: vec![function_entry(2, 5, 10), function_entry(3, 10, 12)],
            docs: vec![
                doc_entry("Global::base_doc"),
                doc_entry("Global::chunk_doc"),
            ],
            signatures: Vec::new(),
            runtime_process_specs: vec![runtime_process_spec("Global::ChunkAgent", 2)],
            runtime_boot_plan: RuntimeBootPlan {
                singletons: vec![singleton_boot("Global::ChunkAgent")],
                ..RuntimeBootPlan::default()
            },
        }
    }

    fn singleton_process_spec(name: &str) -> TypedProcessSpec {
        TypedProcessSpec {
            module_path: "Global".into(),
            process_name: format!("Global::{name}"),
            spec: ProcessSpec {
                process_name: format!("Global::{name}"),
                kind: ProcessKind::Agent,
                instance: ProcessInstance::Singleton,
                state: AstTy::Named(span(0, 0), "Int".into()),
                boot: false,
                registry: false,
                standby: false,
                handlers: Vec::new(),
                handler_specs: Vec::<ProcessRuntimeHandlerSpec>::new(),
                supervisor_policy: None,
            },
            init_uid: 101,
            get_uid: 102,
            set_uid: None,
            handler_uids: Vec::<TypedProcessHandlerUid>::new(),
        }
    }

    fn singleton_process_program(name: &str) -> TypedProgram {
        let result_ty = Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error));
        let init_id = resolved_id("init", Some(&format!("Global::{name}::init")), 101);
        let get_id = resolved_id("get", Some(&format!("Global::{name}::get")), 102);

        TypedProgram {
            nodes: vec![
                TypedNode {
                    ty: Ty::Unit,
                    span: span(0, 0),
                    node: TypedInner::Def(
                        0,
                        init_id,
                        Vec::new(),
                        Vec::new(),
                        result_ty.clone(),
                        None,
                        Box::new(lit_node(Ty::Int, Lit::Int(0.into()), span(0, 0))),
                        Visibility::Public,
                    ),
                },
                TypedNode {
                    ty: Ty::Unit,
                    span: span(0, 0),
                    node: TypedInner::Def(
                        1,
                        get_id,
                        Vec::new(),
                        vec![typed_fun_param("state", 201, Ty::Int)],
                        result_ty,
                        None,
                        Box::new(lit_node(Ty::Int, Lit::Int(1.into()), span(0, 0))),
                        Visibility::Public,
                    ),
                },
            ],
            process_specs: vec![singleton_process_spec(name)],
            boot_plan: SupervisorInitSpec {
                entries: vec![SupervisorInitEntry {
                    process_name: format!("Global::{name}"),
                    timeout_ms: None,
                    handlers: Vec::new(),
                    overrides: Default::default(),
                    span: span(0, 0),
                }],
                ..SupervisorInitSpec::default()
            },
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
    fn emit_assert_literal_bool_folds_to_single_result_path() {
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
                        symbol_info: None,
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
            .any(|opcode| matches!(opcode, Opcode::MakeOk)));
        assert!(!opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::JumpIfFalse(_))));
        assert!(!opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::MakeErr)));
    }

    #[test]
    fn emit_ensure_stores_value_and_calls_predicate_once() {
        let mut gene = Codegen::new();
        let pred_id = sigil::resolved::ResolvedId {
            name: "is_even".into(),
            qualified_name: None,
            unique_id: 7,
            compiler_generated: false,
            symbol_info: None,
            span: span(8, 16),
        };
        let err_id = sigil::resolved::ResolvedId {
            name: "NoneError".into(),
            qualified_name: Some("NoneError".into()),
            unique_id: 8,
            compiler_generated: false,
            symbol_info: None,
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

        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::StoreLocal(_) | Opcode::StoreConstLocal { .. }
        )));
        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::Call {
                fun_idx: 3,
                arity: 1,
                ..
            }
        )));
        assert!(!opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::CallClosure { arity: 1, .. })));
    }

    #[test]
    fn emit_ensure_direct_user_capture_lowers_without_callclosure() {
        let mut gene = Codegen::new();
        let pred_id = sigil::resolved::ResolvedId {
            name: "is_even".into(),
            qualified_name: None,
            unique_id: 70,
            compiler_generated: false,
            symbol_info: None,
            span: span(8, 16),
        };
        let err_id = sigil::resolved::ResolvedId {
            name: "NoneError".into(),
            qualified_name: Some("NoneError".into()),
            unique_id: 71,
            compiler_generated: false,
            symbol_info: None,
            span: span(18, 27),
        };
        gene.state.slot_map.insert(err_id.unique_id, 0);
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

        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::Call {
                fun_idx: 3,
                arity: 1,
                ..
            }
        )));
        assert!(!opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::CallClosure { arity: 1, .. })));
    }

    #[test]
    fn emit_interpolated_str_coalesces_adjacent_text_parts() {
        let mut gene = Codegen::new();
        let node = TypedNode {
            ty: Ty::Str,
            span: span(1, 20),
            node: TypedInner::InterpolatedStr(vec![
                scar::typed::TypedInterpolatedPart::Text("a".into()),
                scar::typed::TypedInterpolatedPart::Text("b".into()),
                scar::typed::TypedInterpolatedPart::Expr(Box::new(lit_node(
                    Ty::Int,
                    Lit::Int(1.into()),
                    span(6, 7),
                ))),
                scar::typed::TypedInterpolatedPart::Text("c".into()),
                scar::typed::TypedInterpolatedPart::Text("d".into()),
            ]),
        };

        gene.emit_node(&node)
            .expect("interpolated string emission should succeed");
        let (opcodes, state) = gene.finalize().expect("labels should resolve");

        assert!(state
            .constants
            .iter()
            .any(|constant| matches!(constant, Constant::Str(value) if value == "ab")));
        assert!(state
            .constants
            .iter()
            .any(|constant| matches!(constant, Constant::Str(value) if value == "cd")));
        for part in ["a", "b", "c", "d"] {
            assert!(!state
                .constants
                .iter()
                .any(|constant| matches!(constant, Constant::Str(value) if value == part)));
        }
        assert_eq!(
            opcodes
                .iter()
                .filter(|opcode| matches!(opcode, Opcode::ConcatStr))
                .count(),
            2
        );
    }

    #[test]
    fn emit_tuple_bind_reuses_test_field_slots() {
        let mut gene = Codegen::new();
        let tuple_ty = Ty::Tuple(vec![Ty::Int, Ty::Int]);
        gene.state.slot_map.insert(82, 0);
        gene.state.next_slot = 1;

        let node = TypedNode {
            ty: Ty::Unit,
            span: span(1, 18),
            node: TypedInner::Bind(
                TypedPattern::Tuple(
                    tuple_ty.clone(),
                    vec![
                        TypedPattern::Var(Ty::Int, resolved_id("left", None, 83)),
                        TypedPattern::Var(Ty::Int, resolved_id("right", None, 84)),
                    ],
                ),
                Box::new(local_var("pair", 82, tuple_ty)),
            ),
        };

        gene.emit_node(&node)
            .expect("tuple bind emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");

        assert_eq!(
            opcodes
                .iter()
                .filter(|opcode| matches!(opcode, Opcode::GetTupleField { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn stale_tuple_bind_decomp_returns_codegen_error() {
        let mut gene = Codegen::new();
        let tuple_ty = Ty::Tuple(vec![Ty::Int, Ty::Int]);
        let pat = TypedPattern::Tuple(
            tuple_ty,
            vec![
                TypedPattern::Var(Ty::Int, resolved_id("left", None, 183)),
                TypedPattern::Var(Ty::Int, resolved_id("right", None, 184)),
            ],
        );
        let decomp = PatternDecomp::Tuple(vec![PatternDecompChild {
            slot: 4,
            decomp: PatternDecomp::None,
        }]);

        let err = gene
            .emit_pattern_bind_from_local(&pat, 0, Some(decomp), &span(1, 10))
            .expect_err("stale tuple decomp should become CodegenError");
        assert!(err.message.contains("tuple pattern decomp arity mismatch"));
    }

    #[test]
    fn emit_list_cons_bind_reuses_test_head_tail_slots() {
        let mut gene = Codegen::new();
        let list_ty = Ty::List(Box::new(Ty::Int));
        gene.state.slot_map.insert(85, 0);
        gene.state.next_slot = 1;

        let node = TypedNode {
            ty: Ty::Unit,
            span: span(1, 18),
            node: TypedInner::Bind(
                TypedPattern::ListCons(
                    list_ty.clone(),
                    Box::new(TypedPattern::Var(Ty::Int, resolved_id("head", None, 86))),
                    Box::new(TypedPattern::Var(
                        list_ty.clone(),
                        resolved_id("tail", None, 87),
                    )),
                ),
                Box::new(local_var("items", 85, list_ty)),
            ),
        };

        gene.emit_node(&node)
            .expect("list cons bind emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");

        assert_eq!(
            opcodes
                .iter()
                .filter(|opcode| matches!(opcode, Opcode::ListHead))
                .count(),
            1
        );
        assert_eq!(
            opcodes
                .iter()
                .filter(|opcode| matches!(opcode, Opcode::ListTail))
                .count(),
            1
        );
    }

    #[test]
    fn emit_extractor_bind_invokes_user_extractor_once() {
        let mut gene = Codegen::new();
        gene.state.slot_map.insert(88, 0);
        gene.state.next_slot = 1;

        let node = TypedNode {
            ty: Ty::Unit,
            span: span(1, 22),
            node: TypedInner::Bind(
                TypedPattern::Extractor {
                    input_ty: Ty::Int,
                    extractor: resolved_id("extract", None, 89),
                    extractor_ty: Ty::UserFunc {
                        fun_idx: 12,
                        type_params: vec![],
                        params: vec![Ty::Int],
                        ret: Box::new(Ty::Int),
                    },
                    success_tag: 0,
                    no_match_tag: 1,
                    err_tag: 2,
                    seq_tys: vec![Ty::Int],
                    items: vec![TypedPattern::Var(Ty::Int, resolved_id("value", None, 90))],
                },
                Box::new(local_var("source", 88, Ty::Int)),
            ),
        };

        gene.emit_node(&node)
            .expect("extractor bind emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");

        assert_eq!(
            opcodes
                .iter()
                .filter(|opcode| matches!(
                    opcode,
                    Opcode::Call {
                        fun_idx: 12,
                        arity: 1,
                        ..
                    }
                ))
                .count(),
            1
        );
    }

    #[test]
    fn emit_match_tuple_bind_reuses_test_field_slots() {
        let mut gene = Codegen::new();
        let tuple_ty = Ty::Tuple(vec![Ty::Int, Ty::Int]);
        gene.state.slot_map.insert(91, 0);
        gene.state.next_slot = 1;

        gene.emit_match(
            &local_var("pair", 91, tuple_ty),
            &[TypedMatchArm {
                pattern: TypedMatchPattern::Tuple(vec![
                    TypedMatchPattern::Binding(resolved_id("left", None, 92)),
                    TypedMatchPattern::Binding(resolved_id("right", None, 93)),
                ]),
                guard: None,
                body: lit_node(Ty::Int, Lit::Int(1.into()), span(12, 13)),
            }],
        )
        .expect("match emission should succeed");

        let (opcodes, _) = gene.finalize().expect("labels should resolve");

        assert_eq!(
            opcodes
                .iter()
                .filter(|opcode| matches!(opcode, Opcode::GetTupleField { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn stale_match_tuple_decomp_returns_codegen_error() {
        let mut gene = Codegen::new();
        let pat = TypedMatchPattern::Tuple(vec![
            TypedMatchPattern::Binding(resolved_id("left", None, 192)),
            TypedMatchPattern::Binding(resolved_id("right", None, 193)),
        ]);
        let decomp = MatchPatternDecomp::Tuple(vec![MatchPatternDecompChild {
            slot: 4,
            decomp: MatchPatternDecomp::None,
        }]);

        let err = gene
            .emit_match_pattern_bind(&pat, 0, Some(decomp), &span(1, 10))
            .expect_err("stale tuple match decomp should become CodegenError");
        assert!(err.message.contains("tuple match decomp arity mismatch"));
    }

    #[test]
    fn stale_constructor_match_decomp_returns_codegen_error() {
        let mut gene = Codegen::new();
        let pat = TypedMatchPattern::Constructor {
            tag: 9,
            field_offset: 0,
            fields: vec![
                TypedMatchPattern::Binding(resolved_id("left", None, 194)),
                TypedMatchPattern::Binding(resolved_id("right", None, 195)),
            ],
        };
        let decomp = MatchPatternDecomp::Constructor(vec![MatchPatternDecompChild {
            slot: 4,
            decomp: MatchPatternDecomp::None,
        }]);

        let err = gene
            .emit_match_pattern_bind(&pat, 0, Some(decomp), &span(1, 10))
            .expect_err("stale constructor match decomp should become CodegenError");
        assert!(err
            .message
            .contains("constructor match decomp arity mismatch"));
    }

    #[test]
    fn malformed_facet_segment_slots_returns_codegen_error() {
        let mut gene = Codegen::new();
        let index = lit_node(Ty::Int, Lit::Int(0.into()), span(5, 6));
        let path = TypedFacetPath {
            source_ty: Ty::List(Box::new(Ty::Int)),
            focus_ty: Ty::Int,
            update_source_ty: Ty::Hole,
            update_focus_ty: Ty::Hole,
            path_kind: TypedFacetPathKind::InfallibleStructural,
            may_fail: false,
            source_readonly_root: false,
            segments: vec![TypedFacetSegment::ListIndex {
                index: Box::new(index),
                display: "0".into(),
                literal_index: Some(0.into()),
                focus_readonly_root: false,
                focus_type_name: None,
            }],
        };
        let mismatch_end = gene.fresh_label();

        let err = gene
            .emit_facet_segments_from_local(0, &path, &[], &span(1, 6), Some(mismatch_end))
            .expect_err("missing facet segment slots should become CodegenError");
        assert!(err
            .message
            .contains("missing precomputed slot metadata for segment 1"));
    }

    #[test]
    fn emit_if_literal_true_skips_branch_opcodes() {
        let mut gene = Codegen::new();
        let node = TypedNode {
            ty: Ty::Int,
            span: span(1, 18),
            node: TypedInner::If(
                Box::new(lit_node(Ty::Bool, Lit::Bool(true), span(1, 5))),
                Box::new(lit_node(Ty::Int, Lit::Int(11.into()), span(9, 11))),
                Some(Box::new(lit_node(
                    Ty::Int,
                    Lit::Int(22.into()),
                    span(15, 17),
                ))),
            ),
        };

        gene.emit_node(&node).expect("if emission should succeed");
        let (opcodes, state) = gene.finalize().expect("labels should resolve");

        assert!(!opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::JumpIfFalse(_) | Opcode::Jump(_))));
        assert!(state
            .constants
            .iter()
            .any(|constant| matches!(constant, Constant::Int(value) if *value == 11.into())));
        assert!(!state
            .constants
            .iter()
            .any(|constant| matches!(constant, Constant::Int(value) if *value == 22.into())));
    }

    #[test]
    fn emit_literal_bind_uses_store_const_local_opcode() {
        let mut gene = Codegen::new();
        let node = TypedNode {
            ty: Ty::Unit,
            span: span(1, 9),
            node: TypedInner::Bind(
                TypedPattern::Wildcard(Ty::Int),
                Box::new(lit_node(Ty::Int, Lit::Int(42.into()), span(5, 7))),
            ),
        };

        gene.emit_node(&node)
            .expect("literal bind emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");

        assert!(opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::StoreConstLocal { .. })));
        assert!(!opcodes
            .windows(2)
            .any(|window| matches!(window, [Opcode::LoadConst(_), Opcode::StoreLocal(_)])));
    }

    #[test]
    fn emit_local_bind_uses_copy_local_opcode() {
        let mut gene = Codegen::new();
        let source_id = sigil::resolved::ResolvedId {
            name: "source".into(),
            qualified_name: None,
            unique_id: 10,
            compiler_generated: false,
            symbol_info: None,
            span: span(1, 7),
        };
        gene.state.slot_map.insert(source_id.unique_id, 0);
        gene.state.next_slot = 1;

        let node = TypedNode {
            ty: Ty::Unit,
            span: span(1, 16),
            node: TypedInner::Bind(
                TypedPattern::Wildcard(Ty::Int),
                Box::new(TypedNode {
                    ty: Ty::Int,
                    span: span(11, 16),
                    node: TypedInner::Var(source_id),
                }),
            ),
        };

        gene.emit_node(&node)
            .expect("local bind emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");

        assert!(opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::CopyLocal { .. })));
        assert!(!opcodes
            .windows(2)
            .any(|window| matches!(window, [Opcode::LoadLocal(_), Opcode::StoreLocal(_)])));
    }

    #[test]
    fn emit_local_tag_compare_uses_eq_local_tag_opcode() {
        let mut gene = Codegen::new();
        let tag_const = gene.add_constant(Constant::Tag(1));

        gene.emit(Opcode::LoadLocal(2));
        gene.emit(Opcode::GetTag);
        gene.emit(Opcode::LoadConst(tag_const));
        gene.emit(Opcode::EqTag);

        let (opcodes, _) = gene.finalize().expect("labels should resolve");

        assert_eq!(
            opcodes,
            vec![Opcode::EqLocalTag {
                local_idx: 2,
                tag_const_idx: tag_const,
            }]
        );
    }

    #[test]
    fn emit_local_tag_branch_fuses_to_dedicated_jump_opcodes() {
        let mut gene = Codegen::new();
        let tag_const = gene.add_constant(Constant::Tag(1));
        let false_label = gene.fresh_label();
        let true_label = gene.fresh_label();

        gene.emit(Opcode::LoadLocal(2));
        gene.emit(Opcode::GetTag);
        gene.emit(Opcode::LoadConst(tag_const));
        gene.emit(Opcode::EqTag);
        gene.emit_jump_if_false(false_label);
        gene.patch_label(false_label);

        gene.emit(Opcode::LoadLocal(3));
        gene.emit(Opcode::GetTag);
        gene.emit(Opcode::LoadConst(tag_const));
        gene.emit(Opcode::EqTag);
        gene.emit_jump_if_true(true_label);
        gene.patch_label(true_label);

        let (opcodes, _) = gene.finalize().expect("labels should resolve");

        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::JumpIfLocalTagNe {
                local_idx: 2,
                tag_const_idx,
                ..
            } if *tag_const_idx == tag_const
        )));
        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::JumpIfLocalTagEq {
                local_idx: 3,
                tag_const_idx,
                ..
            } if *tag_const_idx == tag_const
        )));
        assert!(!opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::EqLocalTag { .. })));
        assert!(!opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::JumpIfFalse(_) | Opcode::JumpIfTrue(_))));
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

    #[test]
    fn emit_map_err_routes_error_branch_through_builtin() {
        let mut gene = Codegen::new();
        gene.state.slot_map.insert(10, 0);
        gene.state.slot_map.insert(11, 1);
        gene.state.next_slot = 2;

        let node = TypedNode {
            ty: Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error)),
            span: span(1, 20),
            node: TypedInner::MapErr(
                Box::new(local_var(
                    "value",
                    10,
                    Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error)),
                )),
                Box::new(local_var("err", 11, Ty::Error)),
            ),
        };

        gene.emit_node(&node)
            .expect("map_err emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");
        let map_err_id = Codegen::builtin_id("map_err").expect("map_err builtin must exist");

        assert!(opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::StoreLocal(_) | Opcode::CopyLocal { .. })));
        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::CallBuiltin {
                builtin_id,
                arity: 2,
                ..
            } if *builtin_id == map_err_id
        )));
        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::JumpIfTrue(_) | Opcode::JumpIfLocalTagEq { .. }
        )));
    }

    #[test]
    fn emit_cause_routes_error_branch_through_builtin() {
        let mut gene = Codegen::new();
        gene.state.slot_map.insert(20, 0);
        gene.state.slot_map.insert(21, 1);
        gene.state.next_slot = 2;

        let node = TypedNode {
            ty: Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error)),
            span: span(1, 18),
            node: TypedInner::Cause(
                Box::new(local_var(
                    "value",
                    20,
                    Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error)),
                )),
                Box::new(local_var("err", 21, Ty::Error)),
            ),
        };

        gene.emit_node(&node)
            .expect("cause emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");
        let cause_id = Codegen::builtin_id("cause").expect("cause builtin must exist");

        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::CallBuiltin {
                builtin_id,
                arity: 2,
                ..
            } if *builtin_id == cause_id
        )));
        assert!(
            opcodes
                .iter()
                .filter(|opcode| matches!(opcode, Opcode::LoadLocal(_)))
                .count()
                >= 2
        );
    }

    #[test]
    fn emit_recover_kind_checks_error_kind_and_calls_handler() {
        let mut gene = Codegen::new();
        gene.state.slot_map.insert(30, 0);
        gene.state
            .callable_names
            .insert("MyError".into(), super::DirectCallableTarget::User(11));
        gene.state.callable_names.insert(
            "Global::MyError".into(),
            super::DirectCallableTarget::User(11),
        );
        gene.state.next_slot = 1;

        let handler = TypedNode {
            ty: Ty::Func(
                vec![Ty::Error],
                Box::new(Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error))),
            ),
            span: span(10, 20),
            node: TypedInner::Capture(
                Box::new(TypedNode {
                    ty: Ty::UserFunc {
                        fun_idx: 7,
                        type_params: vec![],
                        params: vec![Ty::Error],
                        ret: Box::new(Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error))),
                    },
                    span: span(10, 20),
                    node: TypedInner::Var(resolved_id("handler", None, 31)),
                }),
                vec![],
            ),
        };

        let node = TypedNode {
            ty: Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error)),
            span: span(1, 30),
            node: TypedInner::RecoverKind(
                Box::new(local_var(
                    "value",
                    30,
                    Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error)),
                )),
                Box::new(qualified_var("MyError", "Global::MyError", 32, Ty::Error)),
                Box::new(handler),
            ),
        };

        gene.emit_node(&node)
            .expect("recover_kind emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");
        let recover_kind_id =
            Codegen::builtin_id("__recover_kind").expect("__recover_kind builtin must exist");

        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::CallBuiltin {
                builtin_id,
                arity: 3,
                ..
            } if *builtin_id == recover_kind_id
        )));
        assert!(opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::LoadFunctionRef(7))));
        assert!(!opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::CallBuiltin {
                builtin_id,
                arity: 1,
                ..
            } if *builtin_id == Codegen::builtin_id("kind").expect("kind builtin must exist")
        )));
        assert!(!opcodes.iter().any(|opcode| matches!(opcode, Opcode::EqStr)));
        assert!(!opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::CallClosure { arity: 1, .. })));
    }

    #[test]
    fn emit_inject_call_records_direct_call_template() {
        let mut gene = Codegen::new();
        let node = TypedNode {
            ty: Ty::Func(vec![Ty::Int], Box::new(Ty::Int)),
            span: span(1, 12),
            node: TypedInner::InjectCall(
                Box::new(TypedNode {
                    ty: Ty::UserFunc {
                        fun_idx: 7,
                        type_params: vec![],
                        params: vec![Ty::Int, Ty::Int],
                        ret: Box::new(Ty::Int),
                    },
                    span: span(1, 4),
                    node: TypedInner::Var(resolved_id("add", None, 41)),
                }),
                vec![lit_node(Ty::Int, Lit::Int(1.into()), span(9, 10))],
            ),
        };

        gene.emit_node(&node)
            .expect("inject call emission should succeed");
        let (_, state) = gene.finalize().expect("labels should resolve");

        assert!(matches!(
            state.callable_templates.as_slice(),
            [CallableTemplate {
                kind: CallableTemplateKind::InjectDirectCall {
                    target: CallableTemplateDirectTarget::Function(7),
                    bound_arg_count: 1,
                },
                ..
            }]
        ));
    }

    #[test]
    fn emit_compose_records_template_for_template_compatible_operands() {
        let mut gene = Codegen::new();
        let node = TypedNode {
            ty: Ty::Func(
                vec![Ty::Str],
                Box::new(Ty::Result(Box::new(Ty::Str), Box::new(Ty::Error))),
            ),
            span: span(1, 20),
            node: TypedInner::Compose(
                ComposeFlavor::ResultBind,
                Box::new(TypedNode {
                    ty: Ty::Func(
                        vec![Ty::Str],
                        Box::new(Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error))),
                    ),
                    span: span(1, 7),
                    node: TypedInner::Capture(
                        Box::new(TypedNode {
                            ty: Ty::UserFunc {
                                fun_idx: 11,
                                type_params: vec![],
                                params: vec![Ty::Str],
                                ret: Box::new(Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error))),
                            },
                            span: span(1, 7),
                            node: TypedInner::Var(resolved_id("parse", None, 51)),
                        }),
                        vec![],
                    ),
                }),
                Box::new(TypedNode {
                    ty: Ty::Func(
                        vec![Ty::Int],
                        Box::new(Ty::Result(Box::new(Ty::Str), Box::new(Ty::Error))),
                    ),
                    span: span(11, 20),
                    node: TypedInner::Capture(
                        Box::new(TypedNode {
                            ty: Ty::UserFunc {
                                fun_idx: 12,
                                type_params: vec![],
                                params: vec![Ty::Int],
                                ret: Box::new(Ty::Result(Box::new(Ty::Str), Box::new(Ty::Error))),
                            },
                            span: span(11, 20),
                            node: TypedInner::Var(resolved_id("render", None, 52)),
                        }),
                        vec![],
                    ),
                }),
            ),
        };

        gene.emit_node(&node)
            .expect("compose emission should succeed");
        let (_, state) = gene.finalize().expect("labels should resolve");

        assert!(state.callable_templates.iter().any(|template| {
            matches!(
                template.kind,
                CallableTemplateKind::ComposeDirect {
                    flavor: CallableTemplateComposeFlavor::ResultBind,
                }
            )
        }));
    }

    #[test]
    fn emit_safebind_result_wildcard_propagates_err_and_returns_unit_on_success() {
        let mut gene = Codegen::new();
        gene.state.slot_map.insert(40, 0);
        gene.state.next_slot = 1;

        let node = TypedNode {
            ty: Ty::Unit,
            span: span(1, 12),
            node: TypedInner::SafeBind(
                TypedPattern::Wildcard(Ty::Int),
                Box::new(local_var(
                    "value",
                    40,
                    Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error)),
                )),
            ),
        };

        gene.emit_node(&node)
            .expect("safebind result emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");
        let eprint_id = Codegen::builtin_id("eprint").expect("eprint builtin must exist");

        assert!(opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::StoreLocal(_) | Opcode::CopyLocal { .. })));
        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::JumpIfFalse(_) | Opcode::JumpIfLocalTagNe { .. }
        )));
        assert!(opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::GetField { field_index: 0 })));
        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::CallBuiltin {
                builtin_id,
                arity: 1,
                ..
            } if *builtin_id == eprint_id
        )));
        assert!(opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::LoadConst(_))));
    }

    #[test]
    fn emit_exact_list_safebind_long_failure_uses_list_len_opcode() {
        let mut gene = Codegen::new();
        gene.state.slot_map.insert(81, 0);
        gene.state.next_slot = 1;

        let list_ty = Ty::List(Box::new(Ty::Int));
        let pattern = TypedPattern::ListCons(
            list_ty.clone(),
            Box::new(TypedPattern::IntLit(Ty::Int, 1.into())),
            Box::new(TypedPattern::ListCons(
                list_ty.clone(),
                Box::new(TypedPattern::IntLit(Ty::Int, 2.into())),
                Box::new(TypedPattern::ListNil(list_ty.clone())),
            )),
        );

        let node = TypedNode {
            ty: Ty::Unit,
            span: span(1, 16),
            node: TypedInner::SafeBind(pattern, Box::new(local_var("values", 81, list_ty))),
        };

        gene.emit_node(&node)
            .expect("safebind emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");

        assert!(opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::ListLen)));
    }

    #[test]
    fn compose_bytecode_with_chunk_relocates_and_merges_artifact_state() {
        let bytecode =
            compose_bytecode_with_chunk(base_bytecode(), relocatable_chunk()).expect("compose");

        assert_eq!(bytecode.opcodes[1], Opcode::Jump(9));
        assert_eq!(bytecode.opcodes[2], Opcode::JumpIfFalse(9));
        assert_eq!(
            bytecode.opcodes[3],
            Opcode::JumpIfLocalTagEq {
                local_idx: 0,
                tag_const_idx: 1,
                target_pc: 9,
            }
        );
        assert!(matches!(bytecode.opcodes[5], Opcode::LoadConst(2)));
        assert_eq!(bytecode.opcodes[6], Opcode::JumpIfTrue(9));
        assert_eq!(
            bytecode.opcodes[7],
            Opcode::JumpIfLocalTagNe {
                local_idx: 0,
                tag_const_idx: 3,
                target_pc: 9,
            }
        );
        assert!(matches!(bytecode.opcodes[8], Opcode::LoadConst(3)));
        assert!(matches!(
            bytecode.opcodes[14],
            Opcode::MakeError { template_id: 1 }
        ));
        assert!(matches!(
            bytecode.opcodes[15],
            Opcode::Dbg {
                template_id: 1,
                arg_count: 0
            }
        ));
        assert!(matches!(bytecode.opcodes[16], Opcode::LoadConst(4)));
        assert!(matches!(
            bytecode.opcodes[17],
            Opcode::MakeErrorLiteral {
                kind_const_idx: 5,
                message_const_idx: 6
            }
        ));
        assert_eq!(
            bytecode
                .opcodes
                .iter()
                .filter(|op| matches!(op, Opcode::Halt))
                .count(),
            1
        );
        assert!(matches!(bytecode.opcodes[9], Opcode::Halt));

        assert_eq!(bytecode.functions.len(), 4);
        assert_eq!(bytecode.functions[0].fun_idx, 0);
        assert_eq!(bytecode.functions[0].entry_pc, 10);
        assert_eq!(bytecode.functions[0].end_pc, 12);
        assert_eq!(bytecode.functions[1].fun_idx, 1);
        assert_eq!(bytecode.functions[1].entry_pc, 12);
        assert_eq!(bytecode.functions[1].end_pc, 14);
        assert_eq!(bytecode.functions[2].fun_idx, 2);
        assert_eq!(bytecode.functions[2].entry_pc, 14);
        assert_eq!(bytecode.functions[2].end_pc, 19);
        assert_eq!(bytecode.functions[3].fun_idx, 3);
        assert_eq!(bytecode.functions[3].entry_pc, 19);
        assert_eq!(bytecode.functions[3].end_pc, 21);

        assert_eq!(bytecode.constants.len(), 7);
        assert_eq!(bytecode.error_templates.len(), 2);
        assert_eq!(bytecode.dbg_templates.len(), 2);
        assert_eq!(bytecode.type_registry.entries().len(), 2);
        assert_eq!(bytecode.num_locals, 5);
        assert_eq!(bytecode.docs.len(), 2);
        assert_eq!(bytecode.runtime_process_specs.entries.len(), 2);
        assert_eq!(bytecode.runtime_boot_plan.singletons.len(), 2);
        assert_eq!(bytecode.docs[0].qualified_name, "Global::base_doc");
        assert_eq!(bytecode.docs[1].qualified_name, "Global::chunk_doc");
    }

    #[test]
    fn compose_bytecode_with_chunk_rejects_base_without_top_level_halt() {
        let mut base = base_bytecode();
        base.opcodes.retain(|op| !matches!(op, Opcode::Halt));

        let err = compose_bytecode_with_chunk(base, relocatable_chunk())
            .expect_err("base bytecode without top-level halt must fail");

        assert!(err
            .message
            .contains("precompiled bytecode has no top-level Halt"));
    }

    #[test]
    fn compose_bytecode_with_chunk_rejects_chunk_without_top_level_halt() {
        let mut chunk = relocatable_chunk();
        chunk.opcodes.retain(|op| !matches!(op, Opcode::Halt));

        let err = compose_bytecode_with_chunk(base_bytecode(), chunk)
            .expect_err("chunk without top-level halt must fail");

        assert!(err.message.contains("compiled chunk has no top-level Halt"));
    }

    #[test]
    fn compose_bytecode_with_chunk_rejects_mismatched_chunk_bases() {
        let base = base_bytecode();

        let mut const_chunk = relocatable_chunk();
        const_chunk.const_base += 1;
        let err = compose_bytecode_with_chunk(base.clone(), const_chunk)
            .expect_err("const base mismatch must fail");
        assert!(err.message.contains("chunk constant base mismatch"));

        let mut err_chunk = relocatable_chunk();
        err_chunk.error_template_base += 1;
        let err = compose_bytecode_with_chunk(base.clone(), err_chunk)
            .expect_err("error template base mismatch must fail");
        assert!(err.message.contains("chunk error template base mismatch"));

        let mut dbg_chunk = relocatable_chunk();
        dbg_chunk.dbg_template_base += 1;
        let err = compose_bytecode_with_chunk(base.clone(), dbg_chunk)
            .expect_err("dbg template base mismatch must fail");
        assert!(err.message.contains("chunk dbg template base mismatch"));

        let mut type_chunk = relocatable_chunk();
        type_chunk.type_registry_base += 1;
        let err = compose_bytecode_with_chunk(base, type_chunk)
            .expect_err("type registry base mismatch must fail");
        assert!(err.message.contains("chunk type registry base mismatch"));
    }

    #[test]
    fn compose_bytecode_with_chunk_rejects_invalid_chunk_function_index() {
        let mut chunk = relocatable_chunk();
        chunk.functions[1].fun_idx = 9;

        let err = compose_bytecode_with_chunk(base_bytecode(), chunk)
            .expect_err("invalid function index must fail");

        assert!(err
            .message
            .to_ascii_lowercase()
            .contains("function table invariant violated in chunk"));
    }

    #[test]
    fn localize_chunk_indices_rebases_all_chunk_local_indices() {
        let mut opcodes = vec![
            Opcode::LoadConst(3),
            Opcode::StoreConstLocal {
                const_idx: 4,
                local_idx: 1,
            },
            Opcode::EqLocalTag {
                local_idx: 2,
                tag_const_idx: 5,
            },
            Opcode::JumpIfLocalTagEq {
                local_idx: 3,
                tag_const_idx: 6,
                target_pc: 7,
            },
            Opcode::JumpIfLocalTagNe {
                local_idx: 4,
                tag_const_idx: 7,
                target_pc: 8,
            },
            Opcode::MakeError { template_id: 4 },
            Opcode::Dbg {
                template_id: 5,
                arg_count: 0,
            },
            Opcode::MakeErrorLiteral {
                kind_const_idx: 8,
                message_const_idx: 9,
            },
        ];

        localize_chunk_indices(&mut opcodes, 3, 4, 5).expect("localize chunk indices");

        assert_eq!(opcodes[0], Opcode::LoadConst(0));
        assert_eq!(
            opcodes[1],
            Opcode::StoreConstLocal {
                const_idx: 1,
                local_idx: 1,
            }
        );
        assert_eq!(
            opcodes[2],
            Opcode::EqLocalTag {
                local_idx: 2,
                tag_const_idx: 2,
            }
        );
        assert_eq!(
            opcodes[3],
            Opcode::JumpIfLocalTagEq {
                local_idx: 3,
                tag_const_idx: 3,
                target_pc: 7,
            }
        );
        assert_eq!(
            opcodes[4],
            Opcode::JumpIfLocalTagNe {
                local_idx: 4,
                tag_const_idx: 4,
                target_pc: 8,
            }
        );
        assert_eq!(opcodes[5], Opcode::MakeError { template_id: 0 });
        assert_eq!(
            opcodes[6],
            Opcode::Dbg {
                template_id: 0,
                arg_count: 0,
            }
        );
        assert_eq!(
            opcodes[7],
            Opcode::MakeErrorLiteral {
                kind_const_idx: 5,
                message_const_idx: 6,
            }
        );
    }

    #[test]
    fn forge_session_codegen_chunk_uses_chunk_local_indices_from_existing_bases() {
        let mut base = Bytecode::default();
        base.constants = vec![Constant::Int(10.into())];
        base.error_templates = vec![err_template(0, "Global::OldError")];
        base.dbg_templates = vec![dbg_template(0)];

        let mut session = ForgeSession::from_bytecode(&base);
        let node = TypedNode {
            ty: Ty::Unit,
            span: span(1, 18),
            node: TypedInner::Dbg(vec![TypedDbgArg {
                span: span(6, 7),
                ty_name: "Int".into(),
                expr: lit_node(Ty::Int, Lit::Int(1.into()), span(6, 7)),
            }]),
        };

        let (chunk, _) = session
            .codegen_chunk(vec![node])
            .expect("codegen chunk should succeed");

        assert_eq!(chunk.const_base, 1);
        assert_eq!(chunk.error_template_base, 1);
        assert_eq!(chunk.dbg_template_base, 1);
        assert_eq!(chunk.constants, vec![Constant::Int(1.into())]);
        assert!(chunk.opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::Dbg {
                template_id: 0,
                arg_count: 1
            }
        )));
    }

    #[test]
    fn forge_session_codegen_chunk_typed_program_embeds_runtime_metadata() {
        let mut session = ForgeSession::new();
        let (chunk, _) = session
            .codegen_chunk_typed_program(singleton_process_program("ChunkedLogger"))
            .expect("typed program chunk should succeed");

        assert_eq!(chunk.runtime_process_specs.len(), 1);
        assert_eq!(
            chunk.runtime_process_specs[0].type_name,
            "Global::ChunkedLogger"
        );
        assert_eq!(chunk.runtime_process_specs[0].init.callable.fun_idx, 0);
        assert_eq!(chunk.runtime_boot_plan.singletons.len(), 1);
        assert_eq!(
            chunk.runtime_boot_plan.singletons[0].process_name,
            "Global::ChunkedLogger"
        );
    }

    #[test]
    fn forge_session_codegen_chunk_repl_result_uses_result_halt_path_for_failures() {
        let mut session = ForgeSession::new();
        let node = TypedNode {
            ty: Ty::Unit,
            span: span(1, 8),
            node: TypedInner::Bind(
                TypedPattern::ListCons(
                    Ty::List(Box::new(Ty::Int)),
                    Box::new(TypedPattern::Wildcard(Ty::Int)),
                    Box::new(TypedPattern::Wildcard(Ty::List(Box::new(Ty::Int)))),
                ),
                Box::new(TypedNode {
                    ty: Ty::List(Box::new(Ty::Int)),
                    span: span(5, 7),
                    node: TypedInner::ListNil,
                }),
            ),
        };

        let (chunk, _) = session
            .codegen_chunk_repl_result(vec![node])
            .expect("repl result chunk should succeed");

        assert!(chunk
            .opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::StructNew { field_count: 1 })));
        assert!(!chunk
            .opcodes
            .iter()
            .any(|opcode| matches!(opcode, Opcode::CallBuiltin { .. })));
        assert!(matches!(chunk.opcodes.last(), Some(Opcode::Halt)));
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
        result_facet_info: top_level_result_facet_info(typed),
        type_defs,
        function_defs,
        docs: Vec::new(),
    }
}

fn top_level_result_facet_info(typed: &[TypedNode]) -> Option<ReplFacetInfo> {
    typed
        .iter()
        .rev()
        .find(|stmt| {
            !matches!(
                stmt.node,
                TypedInner::Def(..) | TypedInner::ExtractorDef(..) | TypedInner::DeferrorDef(..)
            )
        })
        .and_then(repl_facet_info_for_node)
}

/// Produces the shared REPL inspection view from Scar's typed Facet metadata.
/// Both the compiled REPL chunk and ad-hoc `:facet` queries use this function
/// so their displayed slot and API state cannot drift.
pub fn repl_facet_info_for_node(node: &TypedNode) -> Option<ReplFacetInfo> {
    let (path, source_is_result, operation) =
        match &node.node {
            TypedInner::FacetPath(_) | TypedInner::PendingFacetPath(_) => {
                return facet_info_for_node(node)
            }
            TypedInner::FacetView {
                path,
                source_is_result,
                ..
            } => (
                path,
                *source_is_result,
                ReplFacetOperation {
                    name: "Facet::view".into(),
                    kind_constraint: "ReadablePath".into(),
                    result_ty: ty_to_string(&node.ty),
                    replacement_ty: None,
                    mapper_ty: None,
                },
            ),
            TypedInner::FacetSet {
                path,
                source_is_result,
                value,
                mode,
                ..
            } => (
                path,
                *source_is_result,
                ReplFacetOperation {
                    name: match mode {
                        TypedFacetSetMode::Exact => "Facet::set",
                        TypedFacetSetMode::CaseSet => "Facet::case_set",
                    }
                    .into(),
                    kind_constraint: match mode {
                        TypedFacetSetMode::Exact => "WritablePath",
                        TypedFacetSetMode::CaseSet => "CasePath",
                    }
                    .into(),
                    result_ty: ty_to_string(&node.ty),
                    replacement_ty: Some(ty_to_string(&value.ty)),
                    mapper_ty: None,
                },
            ),
            TypedInner::FacetOver {
                path,
                source_is_result,
                update_fun,
                mode,
                ..
            } => (
                path,
                *source_is_result,
                ReplFacetOperation {
                    name: match mode {
                        TypedFacetOverMode::FocusValue => "Facet::over",
                        TypedFacetOverMode::FocusResult => "Facet::over_result",
                        TypedFacetOverMode::CaseFocusValue
                        | TypedFacetOverMode::CaseFocusResult => "Facet::case_over",
                    }
                    .into(),
                    kind_constraint: match mode {
                        TypedFacetOverMode::CaseFocusValue
                        | TypedFacetOverMode::CaseFocusResult => "CasePath",
                        _ => "WritablePath",
                    }
                    .into(),
                    result_ty: ty_to_string(&node.ty),
                    replacement_ty: Some(ty_to_string(&path.update_focus_ty)),
                    mapper_ty: Some(ty_to_string(&update_fun.ty)),
                },
            ),
            _ => return None,
        };
    let facet_ty = Ty::Facet(
        match path.path_kind {
            TypedFacetPathKind::InfallibleStructural => FacetKind::InfallibleStructural,
            TypedFacetPathKind::FallibleStructural => FacetKind::FallibleStructural,
            TypedFacetPathKind::VariantPath => FacetKind::VariantPath,
        },
        Box::new(path.source_ty.clone()),
        Box::new(path.focus_ty.clone()),
        Box::new(path.update_source_ty.clone()),
        Box::new(path.update_focus_ty.clone()),
    );
    let template = TypedNode {
        ty: facet_ty,
        span: node.span.clone(),
        node: TypedInner::FacetPath(path.clone()),
    };
    let mut info = facet_info_for_node(&template)?;
    if source_is_result {
        info.stop_points
            .insert(0, "source - input already starts in Result context".into());
    }
    info.operation = Some(operation);
    Some(info)
}

fn facet_segment_label(segment: &TypedFacetSegment) -> String {
    match segment {
        TypedFacetSegment::Field { field_name, .. } => field_name.clone(),
        TypedFacetSegment::Tuple { field_index, .. } => format!("_{field_index}"),
        TypedFacetSegment::Variant { variant_name, .. } => variant_name.clone(),
        TypedFacetSegment::ListIndex { display, .. }
        | TypedFacetSegment::ListRange { display, .. }
        | TypedFacetSegment::MapKey { display, .. } => format!("[{display}]"),
    }
}

fn pending_facet_segment_label(segment: &PendingFacetSegment) -> String {
    match segment {
        PendingFacetSegment::Field { name, optional } => {
            if *optional {
                format!("{name}?")
            } else {
                name.clone()
            }
        }
        PendingFacetSegment::Bracket { display, .. }
        | PendingFacetSegment::RangeBracket { display, .. } => format!("[{display}]"),
    }
}

fn pending_facet_segment_kind(segment: &PendingFacetSegment) -> &'static str {
    match segment {
        PendingFacetSegment::Field { name, .. } if name.starts_with('_') => "tuple",
        PendingFacetSegment::Field { .. } => "field",
        PendingFacetSegment::Bracket { .. } | PendingFacetSegment::RangeBracket { .. } => {
            "container segment"
        }
    }
}

fn facet_path_full_path(path: &TypedFacetPath) -> String {
    let mut rendered = String::new();
    for segment in &path.segments {
        match segment {
            TypedFacetSegment::Tuple { field_index, .. } => {
                if rendered.is_empty() {
                    rendered.push_str("Tuple");
                }
                rendered.push_str(&format!("._{field_index}"));
            }
            TypedFacetSegment::ListIndex { display, .. }
            | TypedFacetSegment::ListRange { display, .. } => {
                if rendered.is_empty() {
                    rendered.push_str("List");
                }
                rendered.push_str(&format!(".[{display}]"));
            }
            TypedFacetSegment::MapKey { display, .. } => {
                if rendered.is_empty() {
                    rendered.push_str("HashMap");
                }
                rendered.push_str(&format!(".[{display}]"));
            }
            other => {
                if rendered.is_empty() {
                    rendered.push_str(&ty_to_string(&path.source_ty));
                }
                if !rendered.is_empty() {
                    rendered.push('.');
                }
                rendered.push_str(&facet_segment_label(other));
            }
        }
    }
    if rendered.is_empty() {
        "<facet>".to_string()
    } else {
        rendered
    }
}

fn facet_info_for_node(node: &TypedNode) -> Option<ReplFacetInfo> {
    match &node.node {
        TypedInner::FacetPath(path) => {
            let mut current_source = path.source_ty.clone();
            let mut segments = Vec::with_capacity(path.segments.len());
            let mut stop_points = Vec::new();
            let mut path_is_fallible = false;
            let mut prefix = String::new();
            for segment in &path.segments {
                let label = facet_segment_label(segment);
                let focus_ty = match segment {
                    TypedFacetSegment::Field { .. } | TypedFacetSegment::Tuple { .. } => {
                        match &current_source {
                            Ty::Tuple(items) => match segment {
                                TypedFacetSegment::Tuple { field_index, .. } => items
                                    .get(*field_index as usize)
                                    .cloned()
                                    .unwrap_or(Ty::Unit),
                                _ => Ty::Unit,
                            },
                            Ty::Struct(_, fields) | Ty::Record(_, fields) => match segment {
                                TypedFacetSegment::Field { field_index, .. } => fields
                                    .get(*field_index as usize)
                                    .map(|(_, ty)| ty.clone())
                                    .unwrap_or(Ty::Unit),
                                _ => Ty::Unit,
                            },
                            _ => Ty::Unit,
                        }
                    }
                    TypedFacetSegment::Variant {
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
                    TypedFacetSegment::ListIndex { .. } => match &current_source {
                        Ty::List(inner) => inner.as_ref().clone(),
                        _ => path.focus_ty.clone(),
                    },
                    TypedFacetSegment::ListRange { .. } => match &current_source {
                        Ty::List(inner) => Ty::List(Box::new(inner.as_ref().clone())),
                        _ => path.focus_ty.clone(),
                    },
                    TypedFacetSegment::MapKey { .. } => match &current_source {
                        Ty::Enum(name, args)
                            if name.rsplit("::").next().unwrap_or(name) == "HashMap"
                                && args.len() == 1 =>
                        {
                            args[0].clone()
                        }
                        _ => path.focus_ty.clone(),
                    },
                };
                if !prefix.is_empty() && !matches!(segment, TypedFacetSegment::Tuple { .. }) {
                    prefix.push('.');
                }
                match segment {
                    TypedFacetSegment::Tuple { field_index, .. } => {
                        if prefix.is_empty() {
                            prefix.push_str("Tuple");
                        }
                        prefix.push_str(&format!("._{field_index}"));
                    }
                    TypedFacetSegment::ListIndex { display, .. }
                    | TypedFacetSegment::ListRange { display, .. } => {
                        if prefix.is_empty() {
                            prefix.push_str("List.");
                        }
                        prefix.push_str(&format!("[{display}]"));
                    }
                    TypedFacetSegment::MapKey { display, .. } => {
                        if prefix.is_empty() {
                            prefix.push_str("HashMap.");
                        }
                        prefix.push_str(&format!("[{display}]"));
                    }
                    _ => prefix.push_str(&label),
                }
                let (kind, fallible, reason, policy) = match segment {
                    TypedFacetSegment::Field {
                        readonly, private, ..
                    } => (
                        "field",
                        false,
                        "field access",
                        match (*private, *readonly) {
                            (true, true) => "private readonly",
                            (true, false) => "private",
                            (false, true) => "readonly",
                            (false, false) => "public",
                        },
                    ),
                    TypedFacetSegment::Tuple { .. } => {
                        ("tuple", false, "tuple index access", "public")
                    }
                    TypedFacetSegment::Variant { .. } => {
                        path_is_fallible = true;
                        ("variant", true, "variant mismatch returns Result", "public")
                    }
                    TypedFacetSegment::ListIndex { .. } => {
                        path_is_fallible = true;
                        ("list index", true, "index miss returns Result", "public")
                    }
                    TypedFacetSegment::ListRange { .. } => {
                        path_is_fallible = true;
                        ("list range", true, "range miss returns Result", "public")
                    }
                    TypedFacetSegment::MapKey { .. } => {
                        path_is_fallible = true;
                        ("map key", true, "key miss returns Result", "public")
                    }
                };
                segments.push(ReplFacetSegmentInfo {
                    label: prefix.clone(),
                    kind: kind.to_string(),
                    source_ty: ty_to_string(&current_source),
                    focus_ty: ty_to_string(&focus_ty),
                    fallible,
                    reason: reason.to_string(),
                    policy: policy.to_string(),
                });
                current_source = focus_ty;
            }
            Some(ReplFacetInfo {
                ty: ty_to_string(&node.ty),
                stage: ReplFacetStage::Template,
                path_kind: path.path_kind.as_str().to_string(),
                source_ty: ty_to_string(&path.source_ty),
                focus_ty: ty_to_string(&path.focus_ty),
                update_source_ty: ty_to_string(&path.update_source_ty),
                update_focus_ty: ty_to_string(&path.update_focus_ty),
                api_eligibility: facet_api_eligibility(path),
                view_result_ty: if path_is_fallible || path.may_fail {
                    format!("Result<{}, Error>", ty_to_string(&path.focus_ty))
                } else {
                    ty_to_string(&path.focus_ty)
                },
                full_path: facet_path_full_path(path),
                segments,
                stop_points,
                operation: None,
                root_policy: if path.source_readonly_root {
                    "readonly"
                } else {
                    "public"
                }
                .to_string(),
                available_in_current_scope: !path.segments.iter().any(|segment| {
                    matches!(segment, TypedFacetSegment::Field { private: true, .. })
                }),
            })
        }
        TypedInner::PendingFacetPath(path) => Some(ReplFacetInfo {
            ty: ty_to_string(&node.ty),
            stage: ReplFacetStage::Pending,
            path_kind: "pending".to_string(),
            source_ty: "_".to_string(),
            focus_ty: "_".to_string(),
            update_source_ty: "_".to_string(),
            update_focus_ty: "_".to_string(),
            api_eligibility: vec!["pending specialization".to_string()],
            view_result_ty: "_".to_string(),
            full_path: if path.segments.is_empty() {
                "<facet>".to_string()
            } else {
                let mut rendered = path.root_path_name.clone().unwrap_or_default();
                for segment in &path.segments {
                    let label = pending_facet_segment_label(segment);
                    if rendered.is_empty() {
                        if matches!(
                            segment,
                            PendingFacetSegment::Field { name, .. } if name.starts_with('_')
                        ) {
                            rendered.push_str("Tuple");
                            rendered.push('.');
                        }
                    } else {
                        rendered.push('.');
                    }
                    rendered.push_str(&label);
                }
                rendered
            },
            segments: path
                .segments
                .iter()
                .map(|segment| ReplFacetSegmentInfo {
                    label: if matches!(
                        segment,
                        PendingFacetSegment::Field { name, .. } if name.starts_with('_')
                    ) {
                        format!("Tuple.{}", pending_facet_segment_label(segment))
                    } else {
                        pending_facet_segment_label(segment)
                    },
                    kind: pending_facet_segment_kind(segment).to_string(),
                    source_ty: "_".to_string(),
                    focus_ty: "_".to_string(),
                    fallible: matches!(
                        segment,
                        PendingFacetSegment::Bracket { .. }
                            | PendingFacetSegment::RangeBracket { .. }
                    ),
                    reason: "requires Facet context to specialize".to_string(),
                    policy: "pending".to_string(),
                })
                .collect(),
            stop_points: Vec::new(),
            operation: None,
            root_policy: "public".to_string(),
            available_in_current_scope: true,
        }),
        _ => None,
    }
}

fn facet_api_eligibility(path: &TypedFacetPath) -> Vec<String> {
    let deferred = matches!(
        (&path.update_source_ty, &path.update_focus_ty),
        (Ty::Hole, Ty::Hole)
    );
    let mut apis = Vec::new();
    if deferred {
        apis.push("view: available".to_string());
        if path.has_variant_segment() {
            apis.push("preview: available".to_string());
        }
    } else {
        apis.push("view: unavailable (concrete update slots)".to_string());
        apis.push("preview: unavailable (concrete update slots)".to_string());
    }
    let readonly_boundary = path.source_readonly_root
        || path
            .segments
            .iter()
            .any(|segment| matches!(segment, TypedFacetSegment::Field { readonly: true, .. }));
    if path.is_infallible_structural() && !readonly_boundary {
        apis.push("put: available when replacement B derives T".to_string());
    }
    if readonly_boundary {
        apis.push("set: unavailable (readonly boundary)".to_string());
        apis.push("over: unavailable (readonly boundary)".to_string());
    } else {
        apis.push("set: available".to_string());
        apis.push("over: available".to_string());
    }
    if matches!(path.focus_ty, Ty::Result(_, _)) {
        apis.push("over_result: available".to_string());
    }
    if path.has_variant_segment() {
        apis.push("case_over: available".to_string());
        if path.final_segment_is_variant() {
            apis.push("case_set: available".to_string());
        }
    }
    apis
}

fn collect_stmt_meta(
    stmt: &TypedNode,
    slot_map: &HashMap<u32, u32>,
    bindings: &mut Vec<BindingInfo>,
    type_defs: &mut Vec<TypeDefDisplay>,
    function_defs: &mut Vec<String>,
) {
    match &stmt.node {
        TypedInner::Bind(pat, rhs) | TypedInner::SafeBind(pat, rhs) => {
            let rhs = rhs.as_ref();
            collect_pattern_binding_infos(
                pat,
                slot_map,
                bindings,
                callable_kind_for_node(rhs),
                callable_display_for_node(rhs),
                &callable_capture_names(rhs),
                facet_info_for_node(rhs),
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
        TypedInner::Def(_, id, _, _, _, _, _, _) => {
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
    facet_info: Option<ReplFacetInfo>,
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
                    facet_info: facet_info.clone(),
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
                    facet_info: facet_info.clone(),
                });
            }
            collect_pattern_binding_infos(
                inner,
                slot_map,
                out,
                callable_kind,
                callable_display,
                callable_captures,
                facet_info,
            );
        }
        TypedPattern::Wildcard(_)
        | TypedPattern::Pin(_, _, _)
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
                    facet_info.clone(),
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
                facet_info.clone(),
            );
            collect_pattern_binding_infos(
                tail,
                slot_map,
                out,
                callable_kind,
                callable_display,
                callable_captures,
                facet_info,
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
                facet_info,
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
                    facet_info.clone(),
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
    ty_to_string_with_type_params(ty, &[])
}

fn ty_to_string_with_type_params(ty: &Ty, type_params: &[TypedTypeParam]) -> String {
    match ty {
        Ty::Int => "Int".into(),
        Ty::Float => "Float".into(),
        Ty::Str => "String".into(),
        Ty::Bool => "Boolean".into(),
        Ty::Unit => "Unit".into(),
        Ty::Hole => "_".into(),
        Ty::List(inner) => format!(
            "List<{}>",
            ty_to_string_with_type_params(inner, type_params)
        ),
        Ty::Lazy(inner) => format!(
            "Lazy<{}>",
            ty_to_string_with_type_params(inner, type_params)
        ),
        Ty::TypeRef(inner) => format!(
            "TypeRef<{}>",
            ty_to_string_with_type_params(inner, type_params)
        ),
        Ty::Pid(name) => format!("PID<{}>", surface_rendered_name(name)),
        Ty::Facet(kind, source, focus, update_source, update_focus) => {
            format!(
                "Facet<{}, {}, {}, {}, {}>",
                kind.as_str(),
                ty_to_string_with_type_params(source, type_params),
                ty_to_string_with_type_params(focus, type_params),
                ty_to_string_with_type_params(update_source, type_params),
                ty_to_string_with_type_params(update_focus, type_params)
            )
        }
        Ty::Tuple(items) => format!(
            "({})",
            items
                .iter()
                .map(|item| ty_to_string_with_type_params(item, type_params))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Ty::SelfApp(args) => format!(
            "Self<{}>",
            args.iter()
                .map(|arg| ty_to_string_with_type_params(arg, type_params))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Ty::Result(ok, err) => format!(
            "Result<{}, {}>",
            ty_to_string_with_type_params(ok, type_params),
            ty_to_string_with_type_params(err, type_params)
        ),
        Ty::Struct(name, fields) | Ty::Record(name, fields) => {
            let name = surface_path_name(name);
            let args = type_params
                .iter()
                .filter(|param| {
                    fields
                        .iter()
                        .any(|(_, field_ty)| ty_contains_var(field_ty, param.ty_var))
                })
                .map(|param| param.name.clone())
                .collect::<Vec<_>>();
            if args.is_empty() {
                name.to_string()
            } else {
                format!("{}<{}>", name, args.join(", "))
            }
        }
        Ty::Enum(name, args) => {
            let name = surface_path_name(name);
            if args.is_empty() {
                name.to_string()
            } else {
                format!(
                    "{}<{}>",
                    name,
                    args.iter()
                        .map(|arg| ty_to_string_with_type_params(arg, type_params))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        Ty::Error => "Error".into(),
        Ty::Var(id) => type_params
            .iter()
            .find(|param| param.ty_var == *id)
            .map(|param| param.name.clone())
            .unwrap_or_else(|| "_".into()),
        Ty::Func(params, ret) => {
            let param_str = params
                .iter()
                .map(|param| ty_to_string_with_type_params(param, type_params))
                .collect::<Vec<_>>()
                .join(", ");
            if param_str.is_empty() {
                format!("(-> {})", ty_to_string_with_type_params(ret, type_params))
            } else {
                format!(
                    "({} -> {})",
                    param_str,
                    ty_to_string_with_type_params(ret, type_params)
                )
            }
        }
        Ty::BuiltinFunc { name, .. } => format!("Builtin({})", name),
        Ty::UserFunc { .. } => "UserFunc".into(),
    }
}

fn ty_contains_var(ty: &Ty, needle: u32) -> bool {
    match ty {
        Ty::Var(var) => *var == needle,
        Ty::List(inner) | Ty::Lazy(inner) | Ty::TypeRef(inner) => ty_contains_var(inner, needle),
        Ty::Tuple(items) | Ty::SelfApp(items) => {
            items.iter().any(|item| ty_contains_var(item, needle))
        }
        Ty::Func(params, ret) => {
            params.iter().any(|param| ty_contains_var(param, needle))
                || ty_contains_var(ret, needle)
        }
        Ty::Facet(_, source, focus, update_source, update_focus) => {
            ty_contains_var(source, needle)
                || ty_contains_var(focus, needle)
                || ty_contains_var(update_source, needle)
                || ty_contains_var(update_focus, needle)
        }
        Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
            params.iter().any(|param| ty_contains_var(param, needle))
                || ty_contains_var(ret, needle)
        }
        Ty::Struct(_, fields) | Ty::Record(_, fields) => fields
            .iter()
            .any(|(_, field_ty)| ty_contains_var(field_ty, needle)),
        Ty::Enum(_, args) => args.iter().any(|arg| ty_contains_var(arg, needle)),
        Ty::Result(ok, err) => ty_contains_var(ok, needle) || ty_contains_var(err, needle),
        Ty::Int | Ty::Float | Ty::Str | Ty::Bool | Ty::Unit | Ty::Pid(_) | Ty::Hole | Ty::Error => {
            false
        }
    }
}

fn format_function_signature(
    name: &str,
    type_params: &[TypedTypeParam],
    params: &[TypedFunParam],
    ret_ty: &Ty,
) -> String {
    let type_params_surface = if type_params.is_empty() {
        String::new()
    } else {
        format!(
            "<{}>",
            type_params
                .iter()
                .map(|param| match &param.bound {
                    Some(bound) => format!("{}: {}", param.name, bound),
                    None => param.name.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let params = params
        .iter()
        .map(|param| {
            format!(
                "{}: {}",
                param.id.name,
                ty_to_string_with_type_params(&param.ty, type_params)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{name}{type_params_surface}({params}) -> {}",
        ty_to_string_with_type_params(ret_ty, type_params)
    )
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
    JumpIfLocalTagEqLabel {
        local_idx: u32,
        tag_const_idx: u32,
        label: Label,
    },
    JumpIfLocalTagNeLabel {
        local_idx: u32,
        tag_const_idx: u32,
        label: Label,
    },
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
    direct_targets: Option<(DirectCallableTarget, DirectCallableTarget)>,
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
enum FacetUpdateLeaf {
    Set {
        value_slot: u32,
        wrap_plain_result: bool,
    },
    CaseSet {
        value_slot: u32,
    },
    Over {
        update_fun_slot: u32,
        mode: TypedFacetOverMode,
        focus_is_result: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectCallableTarget {
    Builtin(u16),
    User(u32),
}

#[derive(Debug)]
struct PatternDecompChild {
    slot: u32,
    decomp: PatternDecomp,
}

#[derive(Debug)]
enum PatternDecomp {
    None,
    Tuple(Vec<PatternDecompChild>),
    ResultOk(Box<PatternDecompChild>),
    ListCons {
        head: Box<PatternDecompChild>,
        tail: Box<PatternDecompChild>,
    },
    Extractor(Vec<PatternDecompChild>),
}

#[derive(Debug)]
struct ExactListPatternTestOutcome {
    rest_slot: u32,
    decomp: PatternDecomp,
}

#[derive(Debug)]
struct MatchPatternDecompChild {
    slot: u32,
    decomp: MatchPatternDecomp,
}

#[derive(Debug)]
enum MatchPatternDecomp {
    None,
    Tuple(Vec<MatchPatternDecompChild>),
    Constructor(Vec<MatchPatternDecompChild>),
    ListCons {
        head: Box<MatchPatternDecompChild>,
        tail: Box<MatchPatternDecompChild>,
    },
    Extractor(Vec<MatchPatternDecompChild>),
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

    fn existing_slot_for_id(&self, id: &ResolvedId, span: &Span) -> Result<u32, CodegenError> {
        self.state
            .slot_map
            .get(&id.unique_id)
            .copied()
            .ok_or_else(|| CodegenError {
                message: format!(
                    "Pinned value `{}` is not available in the local scope",
                    id.name
                ),
                span: span.clone(),
            })
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

    fn emit_internal_builtin_call(
        &mut self,
        name: &str,
        arity: u8,
        span: &Span,
    ) -> Result<(), CodegenError> {
        let builtin_id = Self::builtin_id(name).ok_or_else(|| CodegenError {
            message: format!("Missing internal builtin {name}"),
            span: span.clone(),
        })?;
        self.emit(Opcode::CallBuiltin {
            builtin_id,
            arity,
            span_start: span.start as u32,
            span_end: span.end as u32,
        });
        Ok(())
    }

    fn emit_unwrap_result_to_local_or_jump(
        &mut self,
        result_slot: u32,
        target_slot: u32,
        failure_end: Label,
    ) {
        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetTag);
        let err_tag = self.add_constant(Constant::Tag(1));
        self.emit(Opcode::LoadConst(err_tag));
        self.emit(Opcode::EqTag);

        let ok_label = self.fresh_label();
        self.emit_jump_if_false(ok_label);
        self.emit(Opcode::LoadLocal(result_slot));
        self.emit_jump(failure_end);

        self.patch_label(ok_label);
        self.emit(Opcode::LoadLocal(result_slot));
        self.emit(Opcode::GetField { field_index: 0 });
        self.emit(Opcode::StoreLocal(target_slot));
    }

    fn literal_bool_value(node: &TypedNode) -> Option<bool> {
        match &node.node {
            TypedInner::Lit(Lit::Bool(value)) => Some(*value),
            _ => None,
        }
    }

    fn direct_builtin_opcode(name: &str, arity: usize) -> Option<Opcode> {
        match name.rsplit("::").next().unwrap_or(name) {
            "shl" if arity == 2 => Some(Opcode::ShlInt),
            "shr" if arity == 2 => Some(Opcode::ShrInt),
            "bit_not" if arity == 1 => Some(Opcode::BitNotInt),
            "bit_and" if arity == 2 => Some(Opcode::BitAndInt),
            "bit_or" if arity == 2 => Some(Opcode::BitOrInt),
            "bit_xor" if arity == 2 => Some(Opcode::BitXorInt),
            "test_bit" if arity == 2 => Some(Opcode::TestBitInt),
            "set_bit" if arity == 2 => Some(Opcode::SetBitInt),
            "clear_bit" if arity == 2 => Some(Opcode::ClearBitInt),
            "toggle_bit" if arity == 2 => Some(Opcode::ToggleBitInt),
            "string_len" if arity == 1 => Some(Opcode::StringLen),
            "len" if arity == 1 => Some(Opcode::ListLen),
            "safe_mod" if arity == 2 => Some(Opcode::SafeModInt),
            "string_contains" if arity == 2 => Some(Opcode::StringContains),
            "string_starts_with" if arity == 2 => Some(Opcode::StringStartsWith),
            "string_ends_with" if arity == 2 => Some(Opcode::StringEndsWith),
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
                    let target = if self.state.slot_map.contains_key(&id.unique_id) {
                        None
                    } else {
                        self.state
                            .callable_defs
                            .get(&id.unique_id)
                            .copied()
                            .or_else(|| {
                                id.qualified_name
                                    .as_ref()
                                    .and_then(|name| self.state.callable_names.get(name).copied())
                            })
                            .or_else(|| self.state.callable_names.get(&id.name).copied())
                    };
                    if let Some(target) = target {
                        self.emit_direct_callable_ref(target);
                    } else {
                        let slot = self.alloc_slot(id.unique_id);
                        self.emit(Opcode::LoadLocal(slot));
                    }
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

    fn direct_callable_target_for_ref(
        &self,
        node: &TypedNode,
    ) -> Result<Option<DirectCallableTarget>, CodegenError> {
        match (&node.node, &node.ty) {
            (TypedInner::Var(_), Ty::BuiltinFunc { name, .. }) => {
                let builtin_id = Self::builtin_id(name).ok_or_else(|| CodegenError {
                    message: format!("Unknown builtin: {}", name),
                    span: node.span.clone(),
                })?;
                Ok(Some(DirectCallableTarget::Builtin(builtin_id)))
            }
            (TypedInner::Var(_), Ty::UserFunc { fun_idx, .. }) => {
                Ok(Some(DirectCallableTarget::User(*fun_idx)))
            }
            (TypedInner::Var(id), Ty::Func(_, _))
                if !self.state.slot_map.contains_key(&id.unique_id) =>
            {
                Ok(self
                    .state
                    .callable_defs
                    .get(&id.unique_id)
                    .copied()
                    .or_else(|| {
                        id.qualified_name
                            .as_ref()
                            .and_then(|name| self.state.callable_names.get(name).copied())
                    })
                    .or_else(|| self.state.callable_names.get(&id.name).copied()))
            }
            _ => Ok(None),
        }
    }

    fn direct_callable_target_for_capture(
        &self,
        node: &TypedNode,
    ) -> Result<Option<DirectCallableTarget>, CodegenError> {
        match &node.node {
            TypedInner::Capture(target, args) if args.is_empty() => {
                self.direct_callable_target_for_ref(target)
            }
            _ => Ok(None),
        }
    }

    fn emit_direct_callable_ref(&mut self, target: DirectCallableTarget) {
        match target {
            DirectCallableTarget::Builtin(builtin_id) => {
                self.emit(Opcode::LoadBuiltinRef(builtin_id))
            }
            DirectCallableTarget::User(fun_idx) => self.emit(Opcode::LoadFunctionRef(fun_idx)),
        }
    }

    fn direct_callable_target_for_id(&self, id: &ResolvedId) -> Option<DirectCallableTarget> {
        self.state
            .callable_defs
            .get(&id.unique_id)
            .copied()
            .or_else(|| {
                id.qualified_name
                    .as_ref()
                    .and_then(|name| self.state.callable_names.get(name).copied())
            })
            .or_else(|| self.state.callable_names.get(&id.name).copied())
    }

    fn direct_callable_target_for_marker_node(
        &self,
        node: &TypedNode,
    ) -> Option<DirectCallableTarget> {
        match &node.node {
            TypedInner::Var(id) => self
                .direct_callable_target_for_ref(node)
                .ok()
                .flatten()
                .or_else(|| self.direct_callable_target_for_id(id)),
            TypedInner::App(func, _)
            | TypedInner::Capture(func, _)
            | TypedInner::Semi(func)
            | TypedInner::EagerBoundary(func) => self.direct_callable_target_for_marker_node(func),
            _ => None,
        }
    }

    fn emit_recover_kind_marker_ref(&mut self, marker: &TypedNode) -> Result<(), CodegenError> {
        let target = self
            .direct_callable_target_for_marker_node(marker)
            .ok_or_else(|| CodegenError {
                message: "recover_kind marker must resolve to a deferror constructor".into(),
                span: marker.span.clone(),
            })?;
        self.emit_direct_callable_ref(target);
        Ok(())
    }

    fn callable_template_target(target: DirectCallableTarget) -> CallableTemplateDirectTarget {
        match target {
            DirectCallableTarget::Builtin(builtin_id) => {
                CallableTemplateDirectTarget::Builtin(builtin_id)
            }
            DirectCallableTarget::User(fun_idx) => CallableTemplateDirectTarget::Function(fun_idx),
        }
    }

    fn callable_template_compose_flavor(
        flavor: &ComposeFlavor,
    ) -> Option<CallableTemplateComposeFlavor> {
        match flavor {
            ComposeFlavor::Plain => Some(CallableTemplateComposeFlavor::Plain),
            ComposeFlavor::ResultMap => Some(CallableTemplateComposeFlavor::ResultMap),
            ComposeFlavor::ResultBind => Some(CallableTemplateComposeFlavor::ResultBind),
            ComposeFlavor::ListMap { .. } => Some(CallableTemplateComposeFlavor::ListMap),
            ComposeFlavor::ListBind { .. } => Some(CallableTemplateComposeFlavor::ListBind),
        }
    }

    fn operator_compose_template_flavor(
        op: &OperatorTraitOp,
        lhs_ty: &Ty,
    ) -> Option<CallableTemplateComposeFlavor> {
        match op {
            OperatorTraitOp::Compose => Some(CallableTemplateComposeFlavor::Plain),
            OperatorTraitOp::LiftCompose | OperatorTraitOp::KleisliCompose => {
                let Ty::Func(_, ret) = lhs_ty else {
                    return None;
                };
                match (op, ret.as_ref()) {
                    (OperatorTraitOp::LiftCompose, Ty::Result(_, _)) => {
                        Some(CallableTemplateComposeFlavor::ResultMap)
                    }
                    (OperatorTraitOp::LiftCompose, Ty::List(_)) => {
                        Some(CallableTemplateComposeFlavor::ListMap)
                    }
                    (OperatorTraitOp::KleisliCompose, Ty::Result(_, _)) => {
                        Some(CallableTemplateComposeFlavor::ResultBind)
                    }
                    (OperatorTraitOp::KleisliCompose, Ty::List(_)) => {
                        Some(CallableTemplateComposeFlavor::ListBind)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn callable_template_metadata(
        display: Option<&ReplCallableDisplay>,
        fallback_signature: Option<&str>,
    ) -> CallableTemplateMetadata {
        match display {
            Some(ReplCallableDisplay::FnCapture { module, name, sig }) => {
                CallableTemplateMetadata {
                    origin: CallableOrigin::Capture,
                    module: Some(module.clone()),
                    name: Some(name.clone()),
                    full_signature: Some(sig.clone()),
                }
            }
            Some(ReplCallableDisplay::Closure { sig }) => CallableTemplateMetadata {
                origin: CallableOrigin::Closure,
                module: None,
                name: None,
                full_signature: Some(sig.clone()),
            },
            None => CallableTemplateMetadata {
                origin: if fallback_signature.is_some() {
                    CallableOrigin::Closure
                } else {
                    CallableOrigin::Unknown
                },
                module: None,
                name: None,
                full_signature: fallback_signature.map(str::to_string),
            },
        }
    }

    fn add_callable_template(
        &mut self,
        kind: CallableTemplateKind,
        metadata: CallableTemplateMetadata,
    ) -> u32 {
        let template_id = self.state.callable_templates.len() as u32;
        self.state.callable_templates.push(CallableTemplate {
            template_id,
            kind,
            metadata,
        });
        template_id
    }

    fn emit_callable_template_ref(&mut self, template_id: u32) {
        self.emit(Opcode::LoadCallableTemplateRef(template_id));
    }

    fn record_inject_direct_call_template(
        &mut self,
        func: &TypedNode,
        bound_arg_count: usize,
        display: Option<&ReplCallableDisplay>,
        signature: &str,
    ) -> Result<Option<u32>, CodegenError> {
        let Some(target) = self.direct_callable_target_for_ref(func)? else {
            return Ok(None);
        };
        let bound_arg_count = u8::try_from(bound_arg_count).map_err(|_| CodegenError {
            message: "inject direct-call template bound_arg_count exceeds u8".into(),
            span: func.span.clone(),
        })?;
        Ok(Some(self.add_callable_template(
            CallableTemplateKind::InjectDirectCall {
                target: Self::callable_template_target(target),
                bound_arg_count,
            },
            Self::callable_template_metadata(display, Some(signature)),
        )))
    }

    fn callable_template_arg_for_node(
        arg: &TypedNode,
        params: &[TypedClosureParam],
        captures: &[ResolvedId],
    ) -> Option<CallableTemplateArg> {
        let TypedInner::Var(id) = &arg.node else {
            return None;
        };
        if let Some(index) = params
            .iter()
            .position(|param| param.id.unique_id == id.unique_id)
        {
            let index = u8::try_from(index).ok()?;
            return Some(CallableTemplateArg::Runtime(index));
        }
        captures
            .iter()
            .position(|capture| capture.unique_id == id.unique_id)
            .and_then(|index| u8::try_from(index).ok())
            .map(CallableTemplateArg::Bound)
    }

    fn partial_direct_call_template_for_closure(
        &self,
        params: &[TypedClosureParam],
        captures: &[ResolvedId],
        body: &TypedNode,
    ) -> Result<Option<CallableTemplateKind>, CodegenError> {
        let (target, args): (DirectCallableTarget, &[TypedNode]) = match &body.node {
            TypedInner::App(func, args) => {
                let Some(target) = self.direct_callable_target_for_ref(func)? else {
                    return Ok(None);
                };
                (target, args)
            }
            TypedInner::TraitCall { dispatch, args, .. } => match dispatch {
                TraitDispatch::Static(TraitDispatchTarget::Builtin(name)) => (
                    DirectCallableTarget::Builtin(Self::builtin_id(name).ok_or_else(|| {
                        CodegenError {
                            message: format!("Unknown builtin: {}", name),
                            span: body.span.clone(),
                        }
                    })?),
                    args,
                ),
                TraitDispatch::Static(TraitDispatchTarget::UserFunction { fun_idx, .. }) => {
                    (DirectCallableTarget::User(*fun_idx), args)
                }
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };

        let mut arg_sources = Vec::with_capacity(args.len());
        for arg in args {
            let Some(source) = Self::callable_template_arg_for_node(arg, params, captures) else {
                return Ok(None);
            };
            arg_sources.push(source);
        }

        Ok(Some(CallableTemplateKind::PartialDirectCall {
            target: Self::callable_template_target(target),
            arg_sources,
        }))
    }

    fn template_compatible_callable(&self, node: &TypedNode) -> Result<bool, CodegenError> {
        if self.direct_callable_target_for_capture(node)?.is_some() {
            return Ok(true);
        }
        match &node.node {
            TypedInner::InjectCall(func, _) => {
                Ok(self.direct_callable_target_for_ref(func)?.is_some())
            }
            TypedInner::Closure(params, captures, body) => {
                let filtered_captures: Vec<ResolvedId> = captures
                    .iter()
                    .filter(|id| self.state.slot_map.contains_key(&id.unique_id))
                    .cloned()
                    .collect();
                Ok(self
                    .partial_direct_call_template_for_closure(params, &filtered_captures, body)?
                    .is_some())
            }
            TypedInner::Semi(inner) => self.template_compatible_callable(inner),
            _ => Ok(false),
        }
    }

    fn emit_direct_call(&mut self, target: DirectCallableTarget, arity: u8, span: &Span) {
        match target {
            DirectCallableTarget::Builtin(builtin_id) => self.emit(Opcode::CallBuiltin {
                builtin_id,
                arity,
                span_start: span.start as u32,
                span_end: span.end as u32,
            }),
            DirectCallableTarget::User(fun_idx) => self.emit(Opcode::Call {
                fun_idx,
                arity,
                span_start: span.start as u32,
                span_end: span.end as u32,
            }),
        }
    }

    fn emit_callable_invoke(
        &mut self,
        callable: &TypedNode,
        arity: u8,
        span: &Span,
    ) -> Result<(), CodegenError> {
        if let Some(target) = self.direct_callable_target_for_ref(callable)? {
            self.emit_direct_call(target, arity, span);
            return Ok(());
        }
        if let Some(target) = self.direct_callable_target_for_capture(callable)? {
            self.emit_direct_call(target, arity, span);
            return Ok(());
        }
        let mut arg_slots = Vec::with_capacity(arity as usize);
        for _ in 0..arity {
            let arg_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::StoreLocal(arg_slot));
            arg_slots.push(arg_slot);
        }
        self.emit_callable_ref(callable)?;
        for arg_slot in arg_slots.iter().rev() {
            self.emit(Opcode::LoadLocal(*arg_slot));
        }
        self.emit(Opcode::CallClosure {
            arity,
            span_start: span.start as u32,
            span_end: span.end as u32,
        });
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
        direct_targets: Option<(DirectCallableTarget, DirectCallableTarget)>,
    ) -> Result<(), CodegenError> {
        let saved_slot_map = self.state.slot_map.clone();
        let saved_next_slot = self.state.next_slot;

        self.state.slot_map = HashMap::new();
        let (lhs_slot, rhs_slot, input_slot, arity) = if direct_targets.is_some() {
            self.state.next_slot = 1;
            (0u32, 0u32, 0u32, 1u8)
        } else {
            self.state.next_slot = 3;
            (0u32, 1u32, 2u32, 3u8)
        };
        let entry_pc = self.current_pos() as u32;
        let prev_in_function = self.in_function;
        self.in_function = true;

        match flavor {
            ComposeFlavor::Plain => {
                self.emit(Opcode::LoadLocal(input_slot));
                if let Some((lhs, rhs)) = direct_targets {
                    self.emit_direct_call(lhs, 1, span);
                    self.emit_direct_call(rhs, 1, span);
                } else {
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
                }
                self.emit(Opcode::Return);
            }
            ComposeFlavor::ResultMap | ComposeFlavor::ResultBind => {
                self.emit(Opcode::LoadLocal(input_slot));
                if let Some((lhs, _)) = direct_targets {
                    self.emit_direct_call(lhs, 1, span);
                } else {
                    self.emit(Opcode::LoadLocal(lhs_slot));
                    self.emit(Opcode::LoadLocal(input_slot));
                    self.emit(Opcode::CallClosure {
                        arity: 1,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                }
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
                        if let Some((_, rhs)) = direct_targets {
                            self.emit(Opcode::LoadLocal(result_slot));
                            self.emit(Opcode::GetField { field_index: 0 });
                            self.emit_direct_call(rhs, 1, span);
                        } else {
                            self.emit(Opcode::LoadLocal(rhs_slot));
                            self.emit(Opcode::LoadLocal(result_slot));
                            self.emit(Opcode::GetField { field_index: 0 });
                            self.emit(Opcode::CallClosure {
                                arity: 1,
                                span_start: span.start as u32,
                                span_end: span.end as u32,
                            });
                        }
                        self.emit(Opcode::StructNew { field_count: 1 });
                        self.emit(Opcode::Return);
                    }
                    ComposeFlavor::ResultBind => {
                        if let Some((_, rhs)) = direct_targets {
                            self.emit(Opcode::LoadLocal(result_slot));
                            self.emit(Opcode::GetField { field_index: 0 });
                            self.emit_direct_call(rhs, 1, span);
                        } else {
                            self.emit(Opcode::LoadLocal(rhs_slot));
                            self.emit(Opcode::LoadLocal(result_slot));
                            self.emit(Opcode::GetField { field_index: 0 });
                            self.emit(Opcode::CallClosure {
                                arity: 1,
                                span_start: span.start as u32,
                                span_end: span.end as u32,
                            });
                        }
                        self.emit(Opcode::Return);
                    }
                    _ => unreachable!(),
                }
            }
            ComposeFlavor::ListMap { helper } | ComposeFlavor::ListBind { helper } => {
                self.emit(Opcode::LoadLocal(input_slot));
                if let Some((lhs, rhs)) = direct_targets {
                    self.emit_direct_call(lhs, 1, span);
                    self.emit_direct_callable_ref(rhs);
                } else {
                    self.emit(Opcode::LoadLocal(lhs_slot));
                    self.emit(Opcode::LoadLocal(input_slot));
                    self.emit(Opcode::CallClosure {
                        arity: 1,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                    self.emit(Opcode::LoadLocal(rhs_slot));
                }
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
            arity,
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
                    self.emit_compose_function(
                        compose.fun_idx,
                        &compose.flavor,
                        &compose.span,
                        compose.direct_targets,
                    )?;
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
        if let Opcode::Return = op {
            if self
                .label_positions
                .values()
                .all(|position| *position != self.ir.len())
            {
                if let Some(IrOp::Op(Opcode::CallClosure {
                    arity,
                    span_start,
                    span_end,
                })) = self.ir.last()
                {
                    let arity = *arity;
                    let span_start = *span_start;
                    let span_end = *span_end;
                    self.ir.pop();
                    self.ir.push(IrOp::Op(Opcode::TailCallClosure {
                        arity,
                        span_start,
                        span_end,
                    }));
                    return;
                }
            }
        }

        if matches!(op, Opcode::EqTag) && self.ir.len() >= 3 {
            let start = self.ir.len() - 3;
            let current = self.ir.len();
            if self
                .label_positions
                .values()
                .all(|position| !((start + 1)..=current).contains(position))
            {
                if let [IrOp::Op(Opcode::LoadLocal(local_idx)), IrOp::Op(Opcode::GetTag), IrOp::Op(Opcode::LoadConst(tag_const_idx))] =
                    &self.ir[start..]
                {
                    let local_idx = *local_idx;
                    let tag_const_idx = *tag_const_idx;
                    self.ir.truncate(start);
                    self.ir.push(IrOp::Op(Opcode::EqLocalTag {
                        local_idx,
                        tag_const_idx,
                    }));
                    return;
                }
            }
        }

        if let Opcode::StoreLocal(local_idx) = op {
            if self
                .label_positions
                .values()
                .all(|position| *position != self.ir.len())
            {
                if let Some(IrOp::Op(Opcode::LoadConst(const_idx))) = self.ir.last() {
                    let const_idx = *const_idx;
                    self.ir.pop();
                    self.ir.push(IrOp::Op(Opcode::StoreConstLocal {
                        const_idx,
                        local_idx,
                    }));
                    return;
                }
                if let Some(IrOp::Op(Opcode::LoadLocal(src_local_idx))) = self.ir.last() {
                    let src_local_idx = *src_local_idx;
                    self.ir.pop();
                    self.ir.push(IrOp::Op(Opcode::CopyLocal {
                        src_local_idx,
                        dst_local_idx: local_idx,
                    }));
                    return;
                }
                if let Some(IrOp::Op(Opcode::LoadLocal(src_local_idx))) = self.ir.last() {
                    let src_local_idx = *src_local_idx;
                    self.ir.pop();
                    self.ir.push(IrOp::Op(Opcode::CopyLocal {
                        src_local_idx,
                        dst_local_idx: local_idx,
                    }));
                    return;
                }
            }
        }
        self.ir.push(IrOp::Op(op));
    }

    fn emit_jump(&mut self, label: Label) {
        self.ir.push(IrOp::JumpLabel(label));
    }

    fn emit_jump_if_false(&mut self, label: Label) {
        if let Some(IrOp::Op(Opcode::EqLocalTag {
            local_idx,
            tag_const_idx,
        })) = self.ir.last()
        {
            let local_idx = *local_idx;
            let tag_const_idx = *tag_const_idx;
            self.ir.pop();
            self.ir.push(IrOp::JumpIfLocalTagNeLabel {
                local_idx,
                tag_const_idx,
                label,
            });
            return;
        }
        self.ir.push(IrOp::JumpIfFalseLabel(label));
    }

    fn emit_jump_if_true(&mut self, label: Label) {
        if let Some(IrOp::Op(Opcode::EqLocalTag {
            local_idx,
            tag_const_idx,
        })) = self.ir.last()
        {
            let local_idx = *local_idx;
            let tag_const_idx = *tag_const_idx;
            self.ir.pop();
            self.ir.push(IrOp::JumpIfLocalTagEqLabel {
                local_idx,
                tag_const_idx,
                label,
            });
            return;
        }
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
        // Contract with interactive chunk execution:
        // - InteractiveVm::push_chunk(...) and VM::push_atomic(...) both expect
        //   top-level code first and callable bodies only after the top-level Halt.
        // - Main/top-level statements are emitted first.
        // - A single Halt terminates top-level execution.
        // - Function bodies are emitted strictly after Halt and are entered only via Call/CallClosure.
        // - Top-level duplicate function names are rejected earlier in Sigil.
        let mut defs = Vec::new();
        let mut main_stmts = Vec::new();
        let max_def_fun_idx = stmts
            .iter()
            .filter_map(|stmt| match &stmt.node {
                TypedInner::Def(fun_idx, _, _, _, _, _, _, _) => Some(*fun_idx),
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
                TypedInner::Def(fun_idx, id, ..)
                | TypedInner::ExtractorDef(fun_idx, id, ..)
                | TypedInner::DeferrorDef(_, fun_idx, id, ..) => {
                    let target = DirectCallableTarget::User(*fun_idx);
                    self.state.callable_defs.insert(id.unique_id, target);
                    self.state.callable_names.insert(id.name.clone(), target);
                    if let Some(qualified_name) = &id.qualified_name {
                        self.state
                            .callable_names
                            .insert(qualified_name.clone(), target);
                    }
                }
                _ => {}
            }
            match &stmt.node {
                TypedInner::Def(..)
                | TypedInner::ExtractorDef(..)
                | TypedInner::DeferrorDef(..) => defs.push(stmt),
                _ => main_stmts.push(stmt),
            }
        }
        defs.sort_by_key(|stmt| match &stmt.node {
            TypedInner::Def(fun_idx, _, _, _, _, _, _, _) => *fun_idx,
            TypedInner::ExtractorDef(fun_idx, _, _, _, _, _, _) => *fun_idx,
            TypedInner::DeferrorDef(_, fun_idx, _, _, _) => *fun_idx,
            _ => u32::MAX,
        });

        for (i, stmt) in main_stmts.iter().enumerate() {
            if self.top_level_returns_result
                && matches!(
                    stmt.node,
                    TypedInner::FacetPath(_) | TypedInner::PendingFacetPath(_)
                )
            {
                // REPL chunks may end with a FacetPath expression so the session can
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
        let (fun_idx, id, type_params, params, ret_ty, body, visibility) = match &node.node {
            TypedInner::Def(
                fun_idx,
                id,
                type_params,
                params,
                ret_ty,
                _where_clause,
                body,
                visibility,
            ) => (fun_idx, id, type_params, params, ret_ty, body, visibility),
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
            signature: Some(format_function_signature(
                &id.name,
                type_params,
                params,
                ret_ty,
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

            TypedInner::EagerBoundary(inner) => self.emit_node(inner)?,

            TypedInner::Bind(pat, rhs) => {
                if matches!(rhs.ty, Ty::Facet(..)) {
                    self.reserve_pattern_slots_for_facet_bind(pat);
                    let unit_idx = self.add_constant(Constant::Unit);
                    self.emit(Opcode::LoadConst(unit_idx));
                    return Ok(());
                }
                self.emit_node(rhs)?;
                let payload_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(payload_slot));

                let fail_label = self.fresh_label();
                let decomp = self.emit_pattern_test_from_local_for_bind(
                    pat,
                    payload_slot,
                    fail_label,
                    &rhs.span,
                )?;
                self.emit_pattern_bind_from_local(pat, payload_slot, Some(decomp), &rhs.span)?;

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
                init,
                ..
            } => {
                let supervisor_idx = self.add_constant(Constant::Str(supervisor_process.clone()));
                self.emit(Opcode::LoadConst(supervisor_idx));
                self.emit_node(init)?;
                let builtin_id =
                    Self::builtin_id("__supervisor_spawn").ok_or_else(|| CodegenError {
                        message: "Unknown builtin: __supervisor_spawn".into(),
                        span: node.span.clone(),
                    })?;
                self.emit(Opcode::CallBuiltin {
                    builtin_id,
                    arity: 2,
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
                init,
                strategy,
                ..
            } => {
                let supervisor_idx = self.add_constant(Constant::Str(supervisor_process.clone()));
                self.emit(Opcode::LoadConst(supervisor_idx));
                self.emit_node(init)?;
                self.emit_node(strategy)?;
                let builtin_id =
                    Self::builtin_id("__supervisor_workers").ok_or_else(|| CodegenError {
                        message: "Unknown builtin: __supervisor_workers".into(),
                        span: node.span.clone(),
                    })?;
                self.emit(Opcode::CallBuiltin {
                    builtin_id,
                    arity: 3,
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
                origin,
                args,
                ..
            } => {
                if let TraitCallOrigin::Comparison { op, .. } = origin {
                    self.emit_compare_operator_trait_call(
                        *op,
                        dispatch,
                        receiver_ty,
                        args,
                        &node.span,
                    )?;
                    return Ok(());
                }
                if let TraitCallOrigin::Operator { op, lhs_ty, .. } = origin {
                    if args.len() == 2
                        && self.template_compatible_callable(&args[0])?
                        && self.template_compatible_callable(&args[1])?
                    {
                        if let Some(flavor) = Self::operator_compose_template_flavor(op, lhs_ty) {
                            let template_id = self.add_callable_template(
                                CallableTemplateKind::ComposeDirect { flavor },
                                CallableTemplateMetadata {
                                    origin: CallableOrigin::Closure,
                                    module: None,
                                    name: None,
                                    full_signature: Some(ty_to_string(&node.ty)),
                                },
                            );
                            self.emit_callable_template_ref(template_id);
                            self.emit_callable_ref(&args[0])?;
                            self.emit_callable_ref(&args[1])?;
                            self.emit(Opcode::CaptureClosure(2));
                            return Ok(());
                        }
                    }
                }
                match dispatch {
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
                        if matches!(op, BinOp::Eq | BinOp::Neq)
                            && matches!(receiver_ty, Ty::Enum(_, _))
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
                            let builtin_id =
                                Self::builtin_id(name).ok_or_else(|| CodegenError {
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
                    TraitDispatch::Static(TraitDispatchTarget::UserFunction {
                        fun_idx, ..
                    }) => {
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
                }
            }

            TypedInner::InjectCall(func, args) => {
                let display = callable_display_for_node(node);
                let signature = ty_to_string(&node.ty);
                let capture_count = if let Some(template_id) = self
                    .record_inject_direct_call_template(
                        func,
                        args.len(),
                        display.as_ref(),
                        &signature,
                    )? {
                    self.emit_callable_template_ref(template_id);
                    args.len()
                } else {
                    let fun_idx = self.reserve_fun_idx();
                    self.pending_inject_calls.push(PendingInjectCall {
                        fun_idx,
                        extra_arg_count: args.len(),
                        span: node.span.clone(),
                        display: display.clone(),
                        signature: signature.clone(),
                    });
                    self.emit(Opcode::LoadFunctionRef(fun_idx));
                    self.emit_callable_ref(func)?;
                    args.len() + 1
                };
                for arg in args {
                    self.emit_node(arg)?;
                }
                self.emit(Opcode::CaptureClosure(capture_count as u8));
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
                if let Some(target) = self.direct_callable_target_for_capture(right)? {
                    self.emit_node(left)?;
                    self.emit_direct_call(target, 1, &node.span);
                    return Ok(());
                }
                if let TypedInner::InjectCall(func, args) = &right.node {
                    if let Some(target) = self.direct_callable_target_for_ref(func)? {
                        self.emit_node(left)?;
                        for arg in args {
                            self.emit_node(arg)?;
                        }
                        self.emit_direct_call(target, (args.len() + 1) as u8, &node.span);
                        return Ok(());
                    }
                }
                self.emit_callable_ref(right)?;
                self.emit_node(left)?;
                self.emit(Opcode::CallClosure {
                    arity: 1,
                    span_start: node.span.start as u32,
                    span_end: node.span.end as u32,
                });
            }

            TypedInner::Compose(flavor, left, right) => {
                if self.template_compatible_callable(left)?
                    && self.template_compatible_callable(right)?
                {
                    if let Some(flavor) = Self::callable_template_compose_flavor(flavor) {
                        let template_id = self.add_callable_template(
                            CallableTemplateKind::ComposeDirect { flavor },
                            CallableTemplateMetadata {
                                origin: CallableOrigin::Closure,
                                module: None,
                                name: None,
                                full_signature: Some(ty_to_string(&node.ty)),
                            },
                        );
                        self.emit_callable_template_ref(template_id);
                        self.emit_callable_ref(left)?;
                        self.emit_callable_ref(right)?;
                        self.emit(Opcode::CaptureClosure(2));
                        return Ok(());
                    }
                }
                let fun_idx = self.reserve_fun_idx();
                let direct_targets = match (
                    self.direct_callable_target_for_capture(left)?,
                    self.direct_callable_target_for_capture(right)?,
                ) {
                    (Some(left), Some(right)) => Some((left, right)),
                    _ => None,
                };
                self.pending_composes.push(PendingCompose {
                    fun_idx,
                    flavor: flavor.clone(),
                    span: node.span.clone(),
                    direct_targets,
                });
                self.emit(Opcode::LoadFunctionRef(fun_idx));
                if direct_targets.is_none() {
                    self.emit_callable_ref(left)?;
                    self.emit_callable_ref(right)?;
                    self.emit(Opcode::CaptureClosure(2));
                }
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
            TypedInner::HashMapLiteral(entries) => {
                for (key, value) in entries {
                    self.emit_node(key)?;
                    self.emit_node(value)?;
                    self.emit(Opcode::TupleNew { len: 2 });
                }
                self.emit(Opcode::ListFromItems {
                    len: entries.len() as u32,
                });
                let builtin_id =
                    Self::builtin_id("map_from_entries").ok_or_else(|| CodegenError {
                        message: "Unknown builtin: map_from_entries".into(),
                        span: node.span.clone(),
                    })?;
                self.emit(Opcode::CallBuiltin {
                    builtin_id,
                    arity: 1,
                    span_start: node.span.start as u32,
                    span_end: node.span.end as u32,
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

            TypedInner::FacetPath(_) | TypedInner::PendingFacetPath(_) => {
                return Err(CodegenError {
                    message:
                        "Facet path value leaked to codegen; Facet is compile-time only in Stage1"
                            .into(),
                    span: node.span.clone(),
                });
            }

            TypedInner::FacetView {
                source,
                path,
                source_is_result,
            } => {
                self.emit_facet_view(node, source, path, *source_is_result)?;
            }
            TypedInner::FacetSet {
                source,
                path,
                value,
                source_is_result,
                mode,
            } => {
                self.emit_facet_set(node, source, path, value, *source_is_result, *mode)?;
            }
            TypedInner::FacetOver {
                source,
                path,
                update_fun,
                source_is_result,
                mode,
            } => {
                self.emit_facet_over(node, source, path, update_fun, *source_is_result, *mode)?;
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

            TypedInner::Def(
                _fun_idx,
                _id,
                _type_params,
                _params,
                _ret_ty,
                _where_clause,
                _body,
                _,
            ) => {
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
                let display = callable_display_for_node(node);
                let signature = ty_to_string(&node.ty);
                if let Some(kind) =
                    self.partial_direct_call_template_for_closure(params, &filtered_captures, body)?
                {
                    let template_id = self.add_callable_template(
                        kind,
                        Self::callable_template_metadata(display.as_ref(), Some(&signature)),
                    );
                    self.emit_callable_template_ref(template_id);
                    for capture in &filtered_captures {
                        let slot = self.alloc_slot(capture.unique_id);
                        self.emit(Opcode::LoadLocal(slot));
                    }
                    if !filtered_captures.is_empty() {
                        self.emit(Opcode::CaptureClosure(filtered_captures.len() as u8));
                    }
                    return Ok(());
                }
                let fun_idx = self.reserve_fun_idx();
                self.pending_closures.push(PendingClosure {
                    fun_idx,
                    captures: filtered_captures.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    display,
                    signature,
                });
                self.emit(Opcode::LoadFunctionRef(fun_idx));
                for capture in &filtered_captures {
                    let slot = self.alloc_slot(capture.unique_id);
                    self.emit(Opcode::LoadLocal(slot));
                }
                if !filtered_captures.is_empty() {
                    self.emit(Opcode::CaptureClosure(filtered_captures.len() as u8));
                }
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
                self.state
                    .type_registry
                    .try_register(TypeEntry {
                        tag: *tag,
                        name: name.clone(),
                        kind: TypeKind::Struct,
                        field_names: field_names.clone(),
                        private_flags: field_policies.iter().map(|policy| policy.private).collect(),
                    })
                    .map_err(|err| CodegenError {
                        message: err.to_string(),
                        span: node.span.clone(),
                    })?;
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::RecordDef(tag, name, field_names, field_policies, _) => {
                self.state
                    .type_registry
                    .try_register(TypeEntry {
                        tag: *tag,
                        name: name.clone(),
                        kind: TypeKind::Record,
                        field_names: field_names.clone(),
                        private_flags: field_policies.iter().map(|policy| policy.private).collect(),
                    })
                    .map_err(|err| CodegenError {
                        message: err.to_string(),
                        span: node.span.clone(),
                    })?;
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }

            TypedInner::EnumDef(_, variants) => {
                for variant in variants {
                    if matches!(
                        sindr::names::surface_rendered_name(&variant.constructor_name).as_str(),
                        "Result::Ok" | "Result::Err" | "Boolean::True" | "Boolean::False"
                    ) {
                        continue;
                    }
                    self.state
                        .type_registry
                        .try_register(TypeEntry {
                            tag: variant.tag,
                            name: variant.constructor_name.clone(),
                            kind: TypeKind::EnumVariant,
                            field_names: variant.field_names.clone(),
                            private_flags: vec![false; variant.field_names.len()],
                        })
                        .map_err(|err| CodegenError {
                            message: err.to_string(),
                            span: node.span.clone(),
                        })?;
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

    fn emit_facet_view(
        &mut self,
        node: &TypedNode,
        source: &TypedNode,
        path: &TypedFacetPath,
        source_is_result: bool,
    ) -> Result<(), CodegenError> {
        let returns_result = matches!(node.ty, Ty::Result(_, _));
        let segment_slots = self.precompute_facet_segment_slots(path)?;

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

            self.emit_facet_segments_from_local(
                current_slot,
                path,
                &segment_slots,
                &node.span,
                Some(end_label),
            )?;

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
            self.emit_facet_segments_from_local(
                current_slot,
                path,
                &segment_slots,
                &node.span,
                Some(end_label),
            )?;

            let ok_tag = self.add_constant(Constant::Tag(0));
            self.emit(Opcode::LoadConst(ok_tag));
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::StructNew { field_count: 1 });

            self.patch_label(end_label);
        } else {
            self.emit_facet_segments_from_local(
                current_slot,
                path,
                &segment_slots,
                &node.span,
                None,
            )?;
            self.emit(Opcode::LoadLocal(current_slot));
        }

        Ok(())
    }

    fn precompute_facet_segment_slots(
        &mut self,
        path: &TypedFacetPath,
    ) -> Result<Vec<[Option<u32>; 2]>, CodegenError> {
        let mut slots = Vec::with_capacity(path.segments.len());
        for segment in &path.segments {
            match segment {
                TypedFacetSegment::ListIndex {
                    index,
                    literal_index,
                    ..
                } => {
                    if literal_index.is_some() {
                        slots.push([None, None]);
                    } else {
                        self.emit_node(index)?;
                        let slot = self.state.next_slot;
                        self.state.next_slot += 1;
                        self.emit(Opcode::StoreLocal(slot));
                        slots.push([Some(slot), None]);
                    }
                }
                TypedFacetSegment::ListRange {
                    start,
                    end,
                    literal_start,
                    literal_end,
                    ..
                } => {
                    let start_slot = if literal_start.is_some() {
                        None
                    } else {
                        self.emit_node(start)?;
                        let slot = self.state.next_slot;
                        self.state.next_slot += 1;
                        self.emit(Opcode::StoreLocal(slot));
                        Some(slot)
                    };
                    let end_slot = if literal_end.is_some() {
                        None
                    } else {
                        self.emit_node(end)?;
                        let slot = self.state.next_slot;
                        self.state.next_slot += 1;
                        self.emit(Opcode::StoreLocal(slot));
                        Some(slot)
                    };
                    slots.push([start_slot, end_slot]);
                }
                TypedFacetSegment::MapKey {
                    key, literal_key, ..
                } => {
                    if literal_key.is_some() {
                        slots.push([None, None]);
                    } else {
                        self.emit_node(key)?;
                        let slot = self.state.next_slot;
                        self.state.next_slot += 1;
                        self.emit(Opcode::StoreLocal(slot));
                        slots.push([Some(slot), None]);
                    }
                }
                _ => slots.push([None, None]),
            }
        }
        Ok(slots)
    }

    fn emit_facet_segment_argument(
        &mut self,
        segment: &TypedFacetSegment,
        slot: Option<u32>,
        literal_index: Option<&SurtrInt>,
        literal_key: Option<&String>,
    ) -> Result<(), CodegenError> {
        if let Some(slot) = slot {
            self.emit(Opcode::LoadLocal(slot));
            return Ok(());
        }
        if let Some(index) = literal_index {
            let index_const = self.add_constant(Constant::Int(index.clone()));
            self.emit(Opcode::LoadConst(index_const));
            return Ok(());
        }
        if let Some(key) = literal_key {
            let key_const = self.add_constant(Constant::Str(key.clone()));
            self.emit(Opcode::LoadConst(key_const));
            return Ok(());
        }

        match segment {
            TypedFacetSegment::ListIndex { index, .. } => self.emit_node(index),
            TypedFacetSegment::MapKey { key, .. } => self.emit_node(key),
            _ => Ok(()),
        }
    }

    fn emit_facet_list_range_arguments(
        &mut self,
        segment: &TypedFacetSegment,
        slots: [Option<u32>; 2],
        literal_start: Option<&SurtrInt>,
        literal_end: Option<&SurtrInt>,
    ) -> Result<(), CodegenError> {
        match slots[0] {
            Some(slot) => self.emit(Opcode::LoadLocal(slot)),
            None => {
                if let Some(value) = literal_start {
                    let index_const = self.add_constant(Constant::Int(value.clone()));
                    self.emit(Opcode::LoadConst(index_const));
                } else if let TypedFacetSegment::ListRange { start, .. } = segment {
                    self.emit_node(start)?;
                }
            }
        }

        match slots[1] {
            Some(slot) => self.emit(Opcode::LoadLocal(slot)),
            None => {
                if let Some(value) = literal_end {
                    let index_const = self.add_constant(Constant::Int(value.clone()));
                    self.emit(Opcode::LoadConst(index_const));
                } else if let TypedFacetSegment::ListRange { end, .. } = segment {
                    self.emit_node(end)?;
                }
            }
        }

        Ok(())
    }

    fn emit_facet_set(
        &mut self,
        node: &TypedNode,
        source: &TypedNode,
        path: &TypedFacetPath,
        value: &TypedNode,
        source_is_result: bool,
        mode: TypedFacetSetMode,
    ) -> Result<(), CodegenError> {
        let segment_slots = self.precompute_facet_segment_slots(path)?;
        self.emit_node(source)?;
        let source_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(source_slot));

        self.emit_node(value)?;
        let value_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(value_slot));

        let leaf = if matches!(mode, TypedFacetSetMode::CaseSet) {
            FacetUpdateLeaf::CaseSet { value_slot }
        } else {
            FacetUpdateLeaf::Set {
                value_slot,
                wrap_plain_result: false,
            }
        };

        self.emit_facet_update_from_source_slot(
            node,
            source_slot,
            path,
            &segment_slots,
            source_is_result,
            leaf,
        )
    }

    fn emit_facet_over(
        &mut self,
        node: &TypedNode,
        source: &TypedNode,
        path: &TypedFacetPath,
        update_fun: &TypedNode,
        source_is_result: bool,
        mode: TypedFacetOverMode,
    ) -> Result<(), CodegenError> {
        let segment_slots = self.precompute_facet_segment_slots(path)?;
        self.emit_node(source)?;
        let source_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(source_slot));

        self.emit_callable_ref(update_fun)?;
        let update_fun_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(update_fun_slot));

        let normalized_mode = match mode {
            TypedFacetOverMode::CaseFocusValue => TypedFacetOverMode::FocusValue,
            TypedFacetOverMode::CaseFocusResult => TypedFacetOverMode::FocusResult,
            other => other,
        };

        self.emit_facet_update_from_source_slot(
            node,
            source_slot,
            path,
            &segment_slots,
            source_is_result,
            FacetUpdateLeaf::Over {
                update_fun_slot,
                mode: normalized_mode,
                focus_is_result: matches!(path.focus_ty, Ty::Result(_, _)),
            },
        )
    }

    fn emit_facet_update_from_source_slot(
        &mut self,
        node: &TypedNode,
        source_slot: u32,
        path: &TypedFacetPath,
        segment_slots: &[[Option<u32>; 2]],
        source_is_result: bool,
        leaf: FacetUpdateLeaf,
    ) -> Result<(), CodegenError> {
        let returns_result = matches!(node.ty, Ty::Result(_, _));
        if source_is_result && !returns_result {
            return Err(CodegenError {
                message:
                    "Internal invariant broken: plain facet update cannot start from Result source"
                        .into(),
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

        self.emit_facet_update_at_path(
            root_slot,
            path,
            segment_slots,
            0,
            leaf,
            &node.span,
            end_label,
        )?;

        if returns_result {
            let ok_tag = self.add_constant(Constant::Tag(0));
            self.emit(Opcode::LoadConst(ok_tag));
            self.emit(Opcode::LoadLocal(root_slot));
            self.emit(Opcode::StructNew { field_count: 1 });
        } else {
            self.emit(Opcode::LoadLocal(root_slot));
        }

        self.patch_label(end_label);
        Ok(())
    }

    fn emit_variant_from_value_slot(
        &mut self,
        variant_tag: u32,
        discriminant: &SurtrInt,
        payload_arity: u32,
        value_slot: u32,
    ) {
        let tag_const = self.add_constant(Constant::Tag(variant_tag));
        self.emit(Opcode::LoadConst(tag_const));
        let discriminant_const = self.add_constant(Constant::Int(discriminant.clone()));
        self.emit(Opcode::LoadConst(discriminant_const));
        match payload_arity {
            0 => {
                self.emit(Opcode::StructNew { field_count: 1 });
            }
            1 => {
                self.emit(Opcode::LoadLocal(value_slot));
                self.emit(Opcode::StructNew { field_count: 2 });
            }
            n => {
                for index in 0..n {
                    self.emit(Opcode::LoadLocal(value_slot));
                    self.emit(Opcode::GetTupleField { field_index: index });
                }
                self.emit(Opcode::StructNew { field_count: n + 1 });
            }
        }
    }

    fn emit_facet_update_at_path(
        &mut self,
        current_slot: u32,
        path: &TypedFacetPath,
        segment_slots: &[[Option<u32>; 2]],
        segment_idx: usize,
        leaf: FacetUpdateLeaf,
        span: &Span,
        failure_end: Label,
    ) -> Result<(), CodegenError> {
        if segment_idx == path.segments.len() {
            return self.emit_facet_leaf_update(current_slot, leaf, span, failure_end);
        }

        match &path.segments[segment_idx] {
            TypedFacetSegment::Field {
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

                self.emit_facet_update_at_path(
                    focus_slot,
                    path,
                    segment_slots,
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
            TypedFacetSegment::Tuple {
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

                self.emit_facet_update_at_path(
                    focus_slot,
                    path,
                    segment_slots,
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
            TypedFacetSegment::ListIndex { literal_index, .. } => {
                let slots = Self::facet_segment_slots(path, segment_slots, segment_idx, span)?;
                let focus_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(current_slot));
                self.emit_facet_segment_argument(
                    &path.segments[segment_idx],
                    slots[0],
                    literal_index.as_ref(),
                    None,
                )?;
                self.emit_internal_builtin_call("__facet_list_get", 2, span)?;
                let get_result_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(get_result_slot));
                self.emit_unwrap_result_to_local_or_jump(get_result_slot, focus_slot, failure_end);

                self.emit_facet_update_at_path(
                    focus_slot,
                    path,
                    segment_slots,
                    segment_idx + 1,
                    leaf,
                    span,
                    failure_end,
                )?;

                self.emit(Opcode::LoadLocal(current_slot));
                self.emit_facet_segment_argument(
                    &path.segments[segment_idx],
                    slots[0],
                    literal_index.as_ref(),
                    None,
                )?;
                self.emit(Opcode::LoadLocal(focus_slot));
                self.emit_internal_builtin_call("__facet_list_set", 3, span)?;
                let set_result_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(set_result_slot));
                self.emit_unwrap_result_to_local_or_jump(
                    set_result_slot,
                    current_slot,
                    failure_end,
                );
            }
            TypedFacetSegment::ListRange {
                literal_start,
                literal_end,
                ..
            } => {
                let slots = Self::facet_segment_slots(path, segment_slots, segment_idx, span)?;
                let focus_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(current_slot));
                self.emit_facet_list_range_arguments(
                    &path.segments[segment_idx],
                    slots,
                    literal_start.as_ref(),
                    literal_end.as_ref(),
                )?;
                self.emit_internal_builtin_call("__facet_list_slice_get", 3, span)?;
                let get_result_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(get_result_slot));
                self.emit_unwrap_result_to_local_or_jump(get_result_slot, focus_slot, failure_end);

                self.emit_facet_update_at_path(
                    focus_slot,
                    path,
                    segment_slots,
                    segment_idx + 1,
                    leaf,
                    span,
                    failure_end,
                )?;

                self.emit(Opcode::LoadLocal(current_slot));
                self.emit_facet_list_range_arguments(
                    &path.segments[segment_idx],
                    slots,
                    literal_start.as_ref(),
                    literal_end.as_ref(),
                )?;
                self.emit(Opcode::LoadLocal(focus_slot));
                self.emit_internal_builtin_call("__facet_list_slice_set", 4, span)?;
                let set_result_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(set_result_slot));
                self.emit_unwrap_result_to_local_or_jump(
                    set_result_slot,
                    current_slot,
                    failure_end,
                );
            }
            TypedFacetSegment::MapKey { literal_key, .. } => {
                let slots = Self::facet_segment_slots(path, segment_slots, segment_idx, span)?;
                let focus_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::LoadLocal(current_slot));
                self.emit_facet_segment_argument(
                    &path.segments[segment_idx],
                    slots[0],
                    None,
                    literal_key.as_ref(),
                )?;
                self.emit_internal_builtin_call("__facet_map_get", 2, span)?;
                let get_result_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(get_result_slot));
                self.emit_unwrap_result_to_local_or_jump(get_result_slot, focus_slot, failure_end);

                self.emit_facet_update_at_path(
                    focus_slot,
                    path,
                    segment_slots,
                    segment_idx + 1,
                    leaf,
                    span,
                    failure_end,
                )?;

                self.emit(Opcode::LoadLocal(current_slot));
                self.emit_facet_segment_argument(
                    &path.segments[segment_idx],
                    slots[0],
                    None,
                    literal_key.as_ref(),
                )?;
                self.emit(Opcode::LoadLocal(focus_slot));
                self.emit_internal_builtin_call("__facet_map_set_existing", 3, span)?;
                let set_result_slot = self.state.next_slot;
                self.state.next_slot += 1;
                self.emit(Opcode::StoreLocal(set_result_slot));
                self.emit_unwrap_result_to_local_or_jump(
                    set_result_slot,
                    current_slot,
                    failure_end,
                );
            }
            TypedFacetSegment::Variant {
                enum_name,
                variant_name,
                variant_tag,
                discriminant,
                payload_arity,
                optional,
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

                if let FacetUpdateLeaf::CaseSet { value_slot } = leaf {
                    if segment_idx + 1 == path.segments.len() {
                        self.emit_variant_from_value_slot(
                            *variant_tag,
                            discriminant,
                            *payload_arity,
                            value_slot,
                        );
                        self.emit(Opcode::StoreLocal(current_slot));
                        self.emit_jump(continue_label);

                        self.patch_label(mismatch_label);
                        if *optional {
                            self.emit_jump(continue_label);
                        } else {
                            self.emit_variant_from_value_slot(
                                *variant_tag,
                                discriminant,
                                *payload_arity,
                                value_slot,
                            );
                            self.emit(Opcode::StoreLocal(current_slot));
                        }
                        self.patch_label(continue_label);
                        return Ok(());
                    }
                }

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

                self.emit_facet_update_at_path(
                    focus_slot,
                    path,
                    segment_slots,
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
                if *optional {
                    self.emit_jump(continue_label);
                } else {
                    let detail = format!(
                        "Variant mismatch at segment {} ({}) in facet path: expected variant {}::{}, but got a different variant",
                        segment_idx + 1,
                        Self::facet_segment_display(&path.segments[segment_idx]),
                        enum_name,
                        variant_name
                    );
                    self.emit_variant_mismatch_result(&detail, span);
                    self.emit_jump(failure_end);
                }

                self.patch_label(continue_label);
            }
        }
        Ok(())
    }

    fn emit_facet_leaf_update(
        &mut self,
        current_slot: u32,
        leaf: FacetUpdateLeaf,
        span: &Span,
        failure_end: Label,
    ) -> Result<(), CodegenError> {
        match leaf {
            FacetUpdateLeaf::Set {
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
            FacetUpdateLeaf::CaseSet { .. } => {
                return Err(CodegenError {
                    message: "Internal invariant broken: Facet::case_set leaf must be handled at final enum segment".into(),
                    span: span.clone(),
                });
            }
            FacetUpdateLeaf::Over {
                update_fun_slot,
                mode,
                focus_is_result,
            } => match (mode, focus_is_result) {
                (TypedFacetOverMode::FocusValue, true) => {
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

    fn emit_facet_segments_from_local(
        &mut self,
        current_slot: u32,
        path: &TypedFacetPath,
        segment_slots: &[[Option<u32>; 2]],
        span: &Span,
        mismatch_end: Option<Label>,
    ) -> Result<(), CodegenError> {
        for (segment_idx, segment) in path.segments.iter().enumerate() {
            match segment {
                TypedFacetSegment::Field { field_index, .. } => {
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::GetField {
                        field_index: *field_index,
                    });
                    self.emit(Opcode::StoreLocal(current_slot));
                }
                TypedFacetSegment::Tuple { field_index, .. } => {
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::GetTupleField {
                        field_index: *field_index,
                    });
                    self.emit(Opcode::StoreLocal(current_slot));
                }
                TypedFacetSegment::ListIndex { literal_index, .. } => {
                    let Some(end_label) = mismatch_end else {
                        return Err(CodegenError {
                            message:
                                "Internal invariant broken: fallible list facet segment in plain context"
                                    .into(),
                            span: span.clone(),
                        });
                    };
                    let slots = Self::facet_segment_slots(path, segment_slots, segment_idx, span)?;
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit_facet_segment_argument(
                        segment,
                        slots[0],
                        literal_index.as_ref(),
                        None,
                    )?;
                    self.emit_internal_builtin_call("__facet_list_get", 2, span)?;
                    let result_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::StoreLocal(result_slot));
                    self.emit_unwrap_result_to_local_or_jump(result_slot, current_slot, end_label);
                }
                TypedFacetSegment::ListRange {
                    literal_start,
                    literal_end,
                    ..
                } => {
                    let Some(end_label) = mismatch_end else {
                        return Err(CodegenError {
                            message:
                                "Internal invariant broken: fallible list facet range in plain context"
                                    .into(),
                            span: span.clone(),
                        });
                    };
                    let slots = Self::facet_segment_slots(path, segment_slots, segment_idx, span)?;
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit_facet_list_range_arguments(
                        segment,
                        slots,
                        literal_start.as_ref(),
                        literal_end.as_ref(),
                    )?;
                    self.emit_internal_builtin_call("__facet_list_slice_get", 3, span)?;
                    let result_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::StoreLocal(result_slot));
                    self.emit_unwrap_result_to_local_or_jump(result_slot, current_slot, end_label);
                }
                TypedFacetSegment::MapKey { literal_key, .. } => {
                    let Some(end_label) = mismatch_end else {
                        return Err(CodegenError {
                            message:
                                "Internal invariant broken: fallible map facet segment in plain context"
                                    .into(),
                            span: span.clone(),
                        });
                    };
                    let slots = Self::facet_segment_slots(path, segment_slots, segment_idx, span)?;
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit_facet_segment_argument(
                        segment,
                        slots[0],
                        None,
                        literal_key.as_ref(),
                    )?;
                    self.emit_internal_builtin_call("__facet_map_get", 2, span)?;
                    let result_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::StoreLocal(result_slot));
                    self.emit_unwrap_result_to_local_or_jump(result_slot, current_slot, end_label);
                }
                TypedFacetSegment::Variant {
                    enum_name,
                    variant_name,
                    variant_tag,
                    payload_arity,
                    ..
                } => {
                    let Some(end_label) = mismatch_end else {
                        return Err(CodegenError {
                            message:
                                "Internal invariant broken: variant facet segment in plain context"
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
                        "Variant mismatch at segment {} ({}) in facet path: expected variant {}::{}, but got a different variant",
                        segment_idx + 1,
                        Self::facet_segment_display(segment),
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

    fn facet_segment_display(segment: &TypedFacetSegment) -> String {
        match segment {
            TypedFacetSegment::Field { field_name, .. } => format!(".{}", field_name),
            TypedFacetSegment::Tuple { field_index, .. } => format!("._{}", field_index),
            TypedFacetSegment::Variant { variant_name, .. } => format!(".{}", variant_name),
            TypedFacetSegment::ListIndex { display, .. }
            | TypedFacetSegment::ListRange { display, .. }
            | TypedFacetSegment::MapKey { display, .. } => format!(".[{display}]"),
        }
    }

    fn facet_segment_slots(
        path: &TypedFacetPath,
        segment_slots: &[[Option<u32>; 2]],
        segment_idx: usize,
        span: &Span,
    ) -> Result<[Option<u32>; 2], CodegenError> {
        segment_slots.get(segment_idx).copied().ok_or_else(|| {
            let display = path
                .segments
                .get(segment_idx)
                .map(Self::facet_segment_display)
                .unwrap_or_else(|| "<missing>".into());
            CodegenError {
                message: format!(
                    "Malformed facet path: missing precomputed slot metadata for segment {} ({})",
                    segment_idx + 1,
                    display
                ),
                span: span.clone(),
            }
        })
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
            let decomp =
                self.emit_pattern_test_from_local(pat, payload_slot, pattern_fail, &rhs.span)?;
            self.emit_pattern_bind_from_local(pat, payload_slot, Some(decomp), &rhs.span)?;
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
            let outcome = self.emit_exact_list_pattern_test_from_local(
                &items,
                payload_slot,
                &fail_shorts,
                fail_long,
                fail_mismatch,
                &rhs.span,
            )?;
            self.emit_pattern_bind_from_local(pat, payload_slot, Some(outcome.decomp), &rhs.span)?;
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
            self.emit_list_len_mismatch_failure_rhs_long(
                lhs_len,
                outcome.rest_slot,
                rhs.span.clone(),
            )?;

            self.patch_label(fail_mismatch);
            self.emit_pattern_mismatch_failure(rhs.span.clone())?;

            self.patch_label(success_label);
            let unit_idx = self.add_constant(Constant::Unit);
            self.emit(Opcode::LoadConst(unit_idx));
            return Ok(());
        }

        let pattern_fail = self.fresh_label();
        let decomp =
            self.emit_pattern_test_from_local(pat, payload_slot, pattern_fail, &rhs.span)?;
        self.emit_pattern_bind_from_local(pat, payload_slot, Some(decomp), &rhs.span)?;
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
            let outcome = self.emit_exact_list_pattern_test_from_local(
                &items,
                list_slot,
                &fail_shorts,
                fail_long,
                fail_mismatch,
                &rhs.span,
            )?;
            self.emit_pattern_bind_from_local(pat, list_slot, Some(outcome.decomp), &rhs.span)?;
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
            self.emit_list_len_mismatch_failure_rhs_long(
                lhs_len,
                outcome.rest_slot,
                rhs.span.clone(),
            )?;

            self.patch_label(fail_mismatch);
            self.emit_pattern_mismatch_failure(rhs.span.clone())?;

            self.patch_label(success_label);
            let unit_idx = self.add_constant(Constant::Unit);
            self.emit(Opcode::LoadConst(unit_idx));
            return Ok(());
        }

        let pattern_fail = self.fresh_label();
        let decomp = self.emit_pattern_test_from_local(pat, list_slot, pattern_fail, &rhs.span)?;
        self.emit_pattern_bind_from_local(pat, list_slot, Some(decomp), &rhs.span)?;
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
        let rem_count_slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::LoadLocal(remainder_slot));
        self.emit(Opcode::ListLen);
        self.emit(Opcode::StoreLocal(rem_count_slot));

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
    ) -> Result<ExactListPatternTestOutcome, CodegenError> {
        if fail_shorts.len() != items.len() {
            return Err(CodegenError {
                message: "internal error: fail_short label count mismatch".into(),
                span: err_span.clone(),
            });
        }

        let mut current_slot = list_slot;
        let mut links = Vec::with_capacity(items.len());

        for (idx, item) in items.iter().enumerate() {
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListIsEmpty);
            self.emit_jump_if_true(fail_shorts[idx]);

            let head_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListHead);
            self.emit(Opcode::StoreLocal(head_slot));
            let head_decomp =
                self.emit_pattern_test_from_local(item, head_slot, fail_mismatch, err_span)?;

            let next_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListTail);
            self.emit(Opcode::StoreLocal(next_slot));
            links.push((head_slot, head_decomp, next_slot));
            current_slot = next_slot;
        }

        self.emit(Opcode::LoadLocal(current_slot));
        self.emit(Opcode::ListIsEmpty);
        self.emit_jump_if_false(fail_long);
        let decomp = links.into_iter().rev().fold(
            PatternDecomp::None,
            |tail, (head_slot, head_decomp, tail_slot)| PatternDecomp::ListCons {
                head: Box::new(PatternDecompChild {
                    slot: head_slot,
                    decomp: head_decomp,
                }),
                tail: Box::new(PatternDecompChild {
                    slot: tail_slot,
                    decomp: tail,
                }),
            },
        );
        Ok(ExactListPatternTestOutcome {
            rest_slot: current_slot,
            decomp,
        })
    }

    fn emit_pattern_test_from_local(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
        fail_label: Label,
        err_span: &Span,
    ) -> Result<PatternDecomp, CodegenError> {
        self.emit_pattern_test_from_local_with_mode(pat, slot, fail_label, err_span, true)
    }

    fn emit_pattern_test_from_local_for_bind(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
        fail_label: Label,
        err_span: &Span,
    ) -> Result<PatternDecomp, CodegenError> {
        self.emit_pattern_test_from_local_with_mode(pat, slot, fail_label, err_span, false)
    }

    fn emit_pattern_test_from_local_with_mode(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
        fail_label: Label,
        err_span: &Span,
        propagate_result_error: bool,
    ) -> Result<PatternDecomp, CodegenError> {
        let decomp = match pat {
            TypedPattern::Var(_, _) | TypedPattern::Wildcard(_) => PatternDecomp::None,
            TypedPattern::Pin(ty, id, dispatch) => {
                let pinned_slot = self.existing_slot_for_id(id, err_span)?;
                self.emit_eq_dispatch_from_slots(dispatch, ty, slot, pinned_slot, err_span)?;
                self.emit_jump_if_false(fail_label);
                PatternDecomp::None
            }
            TypedPattern::As(_, inner, _) => self.emit_pattern_test_from_local_with_mode(
                inner,
                slot,
                fail_label,
                err_span,
                propagate_result_error,
            )?,
            TypedPattern::IntLit(_, n) => {
                self.emit(Opcode::LoadLocal(slot));
                let n_const = self.add_constant(Constant::Int(n.clone()));
                self.emit(Opcode::LoadConst(n_const));
                self.emit(Opcode::EqInt);
                self.emit_jump_if_false(fail_label);
                PatternDecomp::None
            }
            TypedPattern::Tuple(_, items) => {
                let mut children = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let item_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(slot));
                    self.emit(Opcode::GetTupleField {
                        field_index: index as u32,
                    });
                    self.emit(Opcode::StoreLocal(item_slot));
                    let item_decomp = self.emit_pattern_test_from_local_with_mode(
                        item,
                        item_slot,
                        fail_label,
                        err_span,
                        propagate_result_error,
                    )?;
                    children.push(PatternDecompChild {
                        slot: item_slot,
                        decomp: item_decomp,
                    });
                }
                PatternDecomp::Tuple(children)
            }
            TypedPattern::StrLit(_, s) => {
                self.emit(Opcode::LoadLocal(slot));
                let s_const = self.add_constant(Constant::Str(s.clone()));
                self.emit(Opcode::LoadConst(s_const));
                self.emit(Opcode::EqStr);
                self.emit_jump_if_false(fail_label);
                PatternDecomp::None
            }
            TypedPattern::BoolLit(_, b) => {
                self.emit(Opcode::LoadLocal(slot));
                let b_const = self.add_constant(Constant::Bool(*b));
                self.emit(Opcode::LoadConst(b_const));
                self.emit(Opcode::EqBool);
                self.emit_jump_if_false(fail_label);
                PatternDecomp::None
            }
            TypedPattern::DurationLit(_, n) => {
                self.emit_duration_lit_pattern_test(slot, n, fail_label);
                PatternDecomp::None
            }
            TypedPattern::ListNil(_) => {
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListIsEmpty);
                self.emit_jump_if_false(fail_label);
                PatternDecomp::None
            }
            TypedPattern::ListCons(_, _, _) => self.emit_list_cons_pattern_test_from_local(
                pat,
                slot,
                fail_label,
                err_span,
                propagate_result_error,
            )?,
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
                let inner_decomp = self.emit_pattern_test_from_local_with_mode(
                    inner,
                    inner_slot,
                    fail_label,
                    err_span,
                    propagate_result_error,
                )?;
                PatternDecomp::ResultOk(Box::new(PatternDecompChild {
                    slot: inner_slot,
                    decomp: inner_decomp,
                }))
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
                let mut children = Vec::with_capacity(items.len());
                for (item, item_slot) in items.iter().zip(item_slots.iter()) {
                    let item_decomp = self.emit_pattern_test_from_local_with_mode(
                        item,
                        *item_slot,
                        fail_label,
                        err_span,
                        propagate_result_error,
                    )?;
                    children.push(PatternDecompChild {
                        slot: *item_slot,
                        decomp: item_decomp,
                    });
                }
                PatternDecomp::Extractor(children)
            }
        };
        Ok(decomp)
    }

    fn emit_pattern_bind_from_local(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
        decomp: Option<PatternDecomp>,
        err_span: &Span,
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
                self.emit_pattern_bind_from_local(inner, slot, decomp, err_span)?;
            }
            TypedPattern::Wildcard(_)
            | TypedPattern::Pin(_, _, _)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => {}
            TypedPattern::Tuple(_, items) => {
                let mut cached_children = match decomp {
                    Some(PatternDecomp::Tuple(children)) => Some(children.into_iter()),
                    _ => None,
                };
                for (index, item) in items.iter().enumerate() {
                    let (item_slot, item_decomp) = if let Some(children) = cached_children.as_mut()
                    {
                        let child = children.next().ok_or_else(|| CodegenError {
                            message:
                                "Internal invariant broken: tuple pattern decomp arity mismatch"
                                    .into(),
                            span: err_span.clone(),
                        })?;
                        (child.slot, Some(child.decomp))
                    } else {
                        let item_slot = self.state.next_slot;
                        self.state.next_slot += 1;
                        self.emit(Opcode::LoadLocal(slot));
                        self.emit(Opcode::GetTupleField {
                            field_index: index as u32,
                        });
                        self.emit(Opcode::StoreLocal(item_slot));
                        (item_slot, None)
                    };
                    self.emit_pattern_bind_from_local(item, item_slot, item_decomp, err_span)?;
                }
            }
            TypedPattern::ListCons(_, _, _) => {
                self.emit_list_cons_pattern_bind_from_local(pat, slot, decomp, err_span)?;
            }
            TypedPattern::ResultOk(_, inner) => {
                let (inner_slot, inner_decomp) = match decomp {
                    Some(PatternDecomp::ResultOk(child)) => (child.slot, Some(child.decomp)),
                    _ => {
                        let inner_slot = self.state.next_slot;
                        self.state.next_slot += 1;
                        self.emit(Opcode::LoadLocal(slot));
                        self.emit(Opcode::GetField { field_index: 0 });
                        self.emit(Opcode::StoreLocal(inner_slot));
                        (inner_slot, None)
                    }
                };
                self.emit_pattern_bind_from_local(inner, inner_slot, inner_decomp, err_span)?;
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
                let cached_children = match decomp {
                    Some(PatternDecomp::Extractor(children)) => Some(children),
                    _ => None,
                };
                if let Some(children) = cached_children {
                    for (item, child) in items.iter().zip(children.into_iter()) {
                        self.emit_pattern_bind_from_local(
                            item,
                            child.slot,
                            Some(child.decomp),
                            err_span,
                        )?;
                    }
                } else {
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
                        self.emit_pattern_bind_from_local(item, *item_slot, None, err_span)?;
                    }
                    self.emit_jump(done);
                    self.patch_label(impossible_no_match);
                    self.emit_pattern_mismatch_failure(extractor.span.clone())?;
                    self.patch_label(done);
                }
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
    ) -> Result<PatternDecomp, CodegenError> {
        let mut current_pat = pat;
        let mut current_slot = slot;
        let mut links = Vec::new();

        while let TypedPattern::ListCons(_, head, tail) = current_pat {
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListIsEmpty);
            self.emit_jump_if_true(fail_label);

            let head_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListHead);
            self.emit(Opcode::StoreLocal(head_slot));
            let head_decomp = self.emit_pattern_test_from_local_with_mode(
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
            links.push((head_slot, head_decomp, tail_slot));

            current_pat = tail;
            current_slot = tail_slot;
        }

        let tail_decomp = self.emit_pattern_test_from_local_with_mode(
            current_pat,
            current_slot,
            fail_label,
            err_span,
            propagate_result_error,
        )?;
        let decomp = links.into_iter().rev().fold(
            tail_decomp,
            |tail, (head_slot, head_decomp, tail_slot)| PatternDecomp::ListCons {
                head: Box::new(PatternDecompChild {
                    slot: head_slot,
                    decomp: head_decomp,
                }),
                tail: Box::new(PatternDecompChild {
                    slot: tail_slot,
                    decomp: tail,
                }),
            },
        );
        Ok(decomp)
    }

    fn emit_list_cons_pattern_bind_from_local(
        &mut self,
        pat: &TypedPattern,
        slot: u32,
        decomp: Option<PatternDecomp>,
        err_span: &Span,
    ) -> Result<(), CodegenError> {
        let mut current_pat = pat;
        let mut current_slot = slot;
        let mut current_decomp = decomp;

        while let TypedPattern::ListCons(_, head, tail) = current_pat {
            let (head_slot, head_decomp, tail_slot, tail_decomp) = match current_decomp {
                Some(PatternDecomp::ListCons { head, tail }) => {
                    (head.slot, Some(head.decomp), tail.slot, Some(tail.decomp))
                }
                _ => {
                    let head_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::ListHead);
                    self.emit(Opcode::StoreLocal(head_slot));

                    let tail_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::ListTail);
                    self.emit(Opcode::StoreLocal(tail_slot));
                    (head_slot, None, tail_slot, None)
                }
            };
            self.emit_pattern_bind_from_local(head, head_slot, head_decomp, err_span)?;

            current_pat = tail;
            current_slot = tail_slot;
            current_decomp = tail_decomp;
        }

        self.emit_pattern_bind_from_local(current_pat, current_slot, current_decomp, err_span)
    }

    fn reserve_pattern_slots_for_facet_bind(&mut self, pat: &TypedPattern) {
        match pat {
            TypedPattern::Var(_, id) => {
                self.alloc_slot(id.unique_id);
            }
            TypedPattern::As(_, inner, alias) => {
                self.alloc_slot(alias.unique_id);
                self.reserve_pattern_slots_for_facet_bind(inner);
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
        _err_tag: u32,
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
        let invalid_outcome_label = self.fresh_label();
        self.emit_jump_if_false(invalid_outcome_label);
        self.emit_jump(no_match_label);

        self.patch_label(invalid_outcome_label);
        self.emit_pattern_failure(
            "InvalidMatchResult",
            "Extractor returned an unknown Option tag.",
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

        for template in &mut self.state.callable_templates {
            match &mut template.kind {
                CallableTemplateKind::PartialDirectCall { target, .. }
                | CallableTemplateKind::InjectDirectCall { target, .. } => {
                    if let CallableTemplateDirectTarget::Function(fun_idx) = target {
                        if let Some(new_idx) = remap.get(fun_idx) {
                            *fun_idx = *new_idx;
                        }
                    }
                }
                CallableTemplateKind::ComposeDirect { .. } => {}
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
                if let Some(cond_value) = Self::literal_bool_value(cond) {
                    if cond_value {
                        self.emit_tail_node(then)?;
                    } else if let Some(else_branch) = else_opt {
                        self.emit_tail_node(else_branch)?;
                    } else {
                        self.emit_unit_const();
                        self.emit(Opcode::Return);
                    }
                    return Ok(());
                }

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
                    let decomp =
                        self.emit_match_pattern_test(pat, scrut_slot, next_arm, &scrutinee.span)?;
                    self.emit_match_pattern_bind(pat, scrut_slot, Some(decomp), &scrutinee.span)?;
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

    /// Materialize an explicit eager lazy argument exactly once. The returned
    /// slot is deliberately consumed by the enclosing special form; ordinary
    /// `EagerBoundary` emission simply evaluates its inner expression.
    fn materialize_eager_boundary(
        &mut self,
        node: &TypedNode,
    ) -> Result<Option<u32>, CodegenError> {
        let TypedInner::EagerBoundary(inner) = &node.node else {
            return Ok(None);
        };
        self.emit_node(inner)?;
        let slot = self.state.next_slot;
        self.state.next_slot += 1;
        self.emit(Opcode::StoreLocal(slot));
        Ok(Some(slot))
    }

    fn emit_lazy_argument(
        &mut self,
        node: &TypedNode,
        eager_slot: Option<u32>,
    ) -> Result<(), CodegenError> {
        if let Some(slot) = eager_slot {
            self.emit(Opcode::LoadLocal(slot));
        } else {
            self.emit_node(node)?;
        }
        Ok(())
    }

    fn emit_if(
        &mut self,
        cond: &TypedNode,
        then: &TypedNode,
        else_opt: &Option<Box<TypedNode>>,
    ) -> Result<(), CodegenError> {
        let then_eager = self.materialize_eager_boundary(then)?;
        let else_eager = match else_opt {
            Some(branch) => self.materialize_eager_boundary(branch)?,
            None => None,
        };
        if let Some(cond_value) = Self::literal_bool_value(cond) {
            if cond_value {
                self.emit_lazy_argument(then, then_eager)?;
            } else if let Some(else_branch) = else_opt {
                self.emit_lazy_argument(else_branch, else_eager)?;
            } else {
                let unit_idx = self.add_constant(Constant::Unit);
                self.emit(Opcode::LoadConst(unit_idx));
            }
            return Ok(());
        }

        self.emit_node(cond)?;

        match else_opt {
            Some(else_branch) => {
                let else_label = self.fresh_label();
                let end_label = self.fresh_label();

                self.emit_jump_if_false(else_label);
                self.emit_lazy_argument(then, then_eager)?;
                self.emit_jump(end_label);

                // Patch else label to current position
                self.patch_label(else_label);
                self.emit_lazy_argument(else_branch, else_eager)?;

                self.patch_label(end_label);
            }
            None => {
                let end_label = self.fresh_label();
                self.emit_jump_if_false(end_label);
                self.emit_lazy_argument(then, then_eager)?;
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
        let err_eager = self.materialize_eager_boundary(err)?;
        if let Some(cond_value) = Self::literal_bool_value(cond) {
            if cond_value {
                self.emit_ok_unit_result()?;
            } else {
                self.emit_lazy_argument(err, err_eager)?;
                self.emit(Opcode::MakeErr);
            }
            return Ok(());
        }

        self.emit_node(cond)?;
        let fail_label = self.fresh_label();
        let end_label = self.fresh_label();
        self.emit_jump_if_false(fail_label);
        self.emit_ok_unit_result()?;
        self.emit_jump(end_label);

        self.patch_label(fail_label);
        self.emit_lazy_argument(err, err_eager)?;
        self.emit(Opcode::MakeErr);

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

        let err_eager = self.materialize_eager_boundary(err)?;

        self.emit(Opcode::LoadLocal(value_slot));
        self.emit_callable_invoke(pred, 1, &node.span)?;

        let fail_label = self.fresh_label();
        let end_label = self.fresh_label();
        self.emit_jump_if_false(fail_label);
        self.emit_ok_result_local(value_slot)?;
        self.emit_jump(end_label);

        self.patch_label(fail_label);
        self.emit_lazy_argument(err, err_eager)?;
        self.emit(Opcode::MakeErr);

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
        let marker_eager = self.materialize_eager_boundary(marker)?;
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
        // The eager value is deliberately materialized before the result tag
        // check, but recover_kind's runtime ABI still consumes the static
        // constructor reference used as its kind marker.
        let _ = marker_eager;
        self.emit_recover_kind_marker_ref(marker)?;
        self.emit_callable_ref(handler)?;
        let builtin_id = Self::builtin_id("__recover_kind").ok_or_else(|| CodegenError {
            message: "Unknown builtin: __recover_kind".into(),
            span: node.span.clone(),
        })?;
        self.emit(Opcode::CallBuiltin {
            builtin_id,
            arity: 3,
            span_start: node.span.start as u32,
            span_end: node.span.end as u32,
        });
        self.emit_jump(end_label);

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

        let err_eager = self.materialize_eager_boundary(err)?;

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
        self.emit_lazy_argument(err, err_eager)?;
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
        let unit_idx = self.add_constant(Constant::Unit);
        self.emit(Opcode::LoadConst(unit_idx));
        self.emit(Opcode::MakeOk);
        Ok(())
    }

    fn emit_ok_result_local(&mut self, slot: u32) -> Result<(), CodegenError> {
        self.emit(Opcode::LoadLocal(slot));
        self.emit(Opcode::MakeOk);
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

        let mut normalized = Vec::with_capacity(parts.len());
        for part in parts {
            match part {
                TypedInterpolatedPart::Text(text) => match normalized.last_mut() {
                    Some(TypedInterpolatedPart::Text(existing)) => existing.push_str(text),
                    _ => normalized.push(TypedInterpolatedPart::Text(text.clone())),
                },
                TypedInterpolatedPart::Expr(expr) => {
                    normalized.push(TypedInterpolatedPart::Expr(expr.clone()));
                }
            }
        }

        let mut first = true;
        for part in &normalized {
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
            let decomp =
                self.emit_match_pattern_test(pat, scrut_slot, next_arm, &scrutinee.span)?;
            self.emit_match_pattern_bind(pat, scrut_slot, Some(decomp), &scrutinee.span)?;
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
        err_span: &Span,
    ) -> Result<MatchPatternDecomp, CodegenError> {
        let decomp = match pat {
            TypedMatchPattern::Binding(_) | TypedMatchPattern::Wildcard => MatchPatternDecomp::None,
            TypedMatchPattern::Pin { id, ty, dispatch } => {
                let pinned_slot = self.existing_slot_for_id(id, err_span)?;
                self.emit_eq_dispatch_from_slots(dispatch, ty, slot, pinned_slot, err_span)?;
                self.emit_jump_if_false(fail_label);
                MatchPatternDecomp::None
            }
            TypedMatchPattern::As(inner, _) => {
                self.emit_match_pattern_test(inner, slot, fail_label, err_span)?
            }
            TypedMatchPattern::BoolLit(b) => {
                self.emit(Opcode::LoadLocal(slot));
                let bool_const = self.add_constant(Constant::Bool(*b));
                self.emit(Opcode::LoadConst(bool_const));
                self.emit(Opcode::EqBool);
                self.emit_jump_if_false(fail_label);
                MatchPatternDecomp::None
            }
            TypedMatchPattern::IntLit(n) => {
                self.emit(Opcode::LoadLocal(slot));
                let int_const = self.add_constant(Constant::Int(n.clone()));
                self.emit(Opcode::LoadConst(int_const));
                self.emit(Opcode::EqInt);
                self.emit_jump_if_false(fail_label);
                MatchPatternDecomp::None
            }
            TypedMatchPattern::StrLit(s) => {
                self.emit(Opcode::LoadLocal(slot));
                let str_const = self.add_constant(Constant::Str(s.clone()));
                self.emit(Opcode::LoadConst(str_const));
                self.emit(Opcode::EqStr);
                self.emit_jump_if_false(fail_label);
                MatchPatternDecomp::None
            }
            TypedMatchPattern::DurationLit(n) => {
                self.emit_duration_lit_pattern_test(slot, n, fail_label);
                MatchPatternDecomp::None
            }
            TypedMatchPattern::ErrorKind(kind) => {
                self.emit_error_kind_test_from_local(slot, kind, fail_label)?;
                MatchPatternDecomp::None
            }
            TypedMatchPattern::Or(items) => {
                let success_label = self.fresh_label();
                for item in items {
                    let next_label = self.fresh_label();
                    self.emit_match_pattern_test(item, slot, next_label, err_span)?;
                    self.emit_jump(success_label);
                    self.patch_label(next_label);
                }
                self.emit_jump(fail_label);
                self.patch_label(success_label);
                MatchPatternDecomp::None
            }
            TypedMatchPattern::Tuple(items) => {
                let mut children = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let item_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(slot));
                    self.emit(Opcode::GetTupleField {
                        field_index: index as u32,
                    });
                    self.emit(Opcode::StoreLocal(item_slot));
                    let item_decomp =
                        self.emit_match_pattern_test(item, item_slot, fail_label, err_span)?;
                    children.push(MatchPatternDecompChild {
                        slot: item_slot,
                        decomp: item_decomp,
                    });
                }
                MatchPatternDecomp::Tuple(children)
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

                let mut children = Vec::with_capacity(fields.len());
                for (idx, field_pat) in fields.iter().enumerate() {
                    let inner_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(slot));
                    self.emit(Opcode::GetField {
                        field_index: *field_offset + idx as u32,
                    });
                    self.emit(Opcode::StoreLocal(inner_slot));
                    let field_decomp =
                        self.emit_match_pattern_test(field_pat, inner_slot, fail_label, err_span)?;
                    children.push(MatchPatternDecompChild {
                        slot: inner_slot,
                        decomp: field_decomp,
                    });
                }
                MatchPatternDecomp::Constructor(children)
            }
            TypedMatchPattern::ListNil => {
                self.emit(Opcode::LoadLocal(slot));
                self.emit(Opcode::ListIsEmpty);
                self.emit_jump_if_false(fail_label);
                MatchPatternDecomp::None
            }
            TypedMatchPattern::ListCons(_, _) => {
                self.emit_list_cons_match_pattern_test(pat, slot, fail_label)?
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
                let mut children = Vec::with_capacity(items.len());
                for (item, item_slot) in items.iter().zip(item_slots.iter()) {
                    let item_decomp =
                        self.emit_match_pattern_test(item, *item_slot, fail_label, err_span)?;
                    children.push(MatchPatternDecompChild {
                        slot: *item_slot,
                        decomp: item_decomp,
                    });
                }
                MatchPatternDecomp::Extractor(children)
            }
        };
        Ok(decomp)
    }

    fn emit_match_pattern_bind(
        &mut self,
        pat: &TypedMatchPattern,
        slot: u32,
        decomp: Option<MatchPatternDecomp>,
        err_span: &Span,
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
                self.emit_match_pattern_bind(inner, slot, decomp, err_span)?;
            }
            TypedMatchPattern::Wildcard
            | TypedMatchPattern::Pin { .. }
            | TypedMatchPattern::BoolLit(_)
            | TypedMatchPattern::IntLit(_)
            | TypedMatchPattern::StrLit(_)
            | TypedMatchPattern::DurationLit(_)
            | TypedMatchPattern::ErrorKind(_)
            | TypedMatchPattern::Or(_)
            | TypedMatchPattern::ListNil => {}
            TypedMatchPattern::Tuple(items) => {
                let mut cached_children = match decomp {
                    Some(MatchPatternDecomp::Tuple(children)) => Some(children.into_iter()),
                    _ => None,
                };
                for (index, item) in items.iter().enumerate() {
                    let (item_slot, item_decomp) = if let Some(children) = cached_children.as_mut()
                    {
                        let child = children.next().ok_or_else(|| CodegenError {
                            message: "Internal invariant broken: tuple match decomp arity mismatch"
                                .into(),
                            span: err_span.clone(),
                        })?;
                        (child.slot, Some(child.decomp))
                    } else {
                        let item_slot = self.state.next_slot;
                        self.state.next_slot += 1;
                        self.emit(Opcode::LoadLocal(slot));
                        self.emit(Opcode::GetTupleField {
                            field_index: index as u32,
                        });
                        self.emit(Opcode::StoreLocal(item_slot));
                        (item_slot, None)
                    };
                    self.emit_match_pattern_bind(item, item_slot, item_decomp, err_span)?;
                }
            }
            TypedMatchPattern::Constructor {
                fields,
                field_offset,
                ..
            } => {
                let mut cached_children = match decomp {
                    Some(MatchPatternDecomp::Constructor(children)) => Some(children.into_iter()),
                    _ => None,
                };
                for (idx, field_pat) in fields.iter().enumerate() {
                    let (inner_slot, inner_decomp) = if let Some(children) =
                        cached_children.as_mut()
                    {
                        let child = children.next().ok_or_else(|| CodegenError {
                            message:
                                "Internal invariant broken: constructor match decomp arity mismatch"
                                    .into(),
                            span: err_span.clone(),
                        })?;
                        (child.slot, Some(child.decomp))
                    } else {
                        let inner_slot = self.state.next_slot;
                        self.state.next_slot += 1;
                        self.emit(Opcode::LoadLocal(slot));
                        self.emit(Opcode::GetField {
                            field_index: *field_offset + idx as u32,
                        });
                        self.emit(Opcode::StoreLocal(inner_slot));
                        (inner_slot, None)
                    };
                    self.emit_match_pattern_bind(field_pat, inner_slot, inner_decomp, err_span)?;
                }
            }
            TypedMatchPattern::ListCons(_, _) => {
                self.emit_list_cons_match_pattern_bind(pat, slot, decomp, err_span)?;
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
                let cached_children = match decomp {
                    Some(MatchPatternDecomp::Extractor(children)) => Some(children),
                    _ => None,
                };
                if let Some(children) = cached_children {
                    for (item, child) in items.iter().zip(children.into_iter()) {
                        self.emit_match_pattern_bind(
                            item,
                            child.slot,
                            Some(child.decomp),
                            err_span,
                        )?;
                    }
                } else {
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
                        self.emit_match_pattern_bind(item, *item_slot, None, err_span)?;
                    }
                    self.emit_jump(done);
                    self.patch_label(impossible_no_match);
                    self.emit_pattern_mismatch_failure(extractor.span.clone())?;
                    self.patch_label(done);
                }
            }
        }
        Ok(())
    }

    fn emit_list_cons_match_pattern_test(
        &mut self,
        pat: &TypedMatchPattern,
        slot: u32,
        fail_label: Label,
    ) -> Result<MatchPatternDecomp, CodegenError> {
        let mut current_pat = pat;
        let mut current_slot = slot;
        let mut links = Vec::new();

        while let TypedMatchPattern::ListCons(head, tail) = current_pat {
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListIsEmpty);
            self.emit_jump_if_true(fail_label);

            let head_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListHead);
            self.emit(Opcode::StoreLocal(head_slot));
            let head_decomp = self.emit_match_pattern_test(
                head,
                head_slot,
                fail_label,
                &Span { start: 0, end: 0 },
            )?;

            let tail_slot = self.state.next_slot;
            self.state.next_slot += 1;
            self.emit(Opcode::LoadLocal(current_slot));
            self.emit(Opcode::ListTail);
            self.emit(Opcode::StoreLocal(tail_slot));
            links.push((head_slot, head_decomp, tail_slot));

            current_pat = tail;
            current_slot = tail_slot;
        }

        let tail_decomp = self.emit_match_pattern_test(
            current_pat,
            current_slot,
            fail_label,
            &Span { start: 0, end: 0 },
        )?;
        let decomp = links.into_iter().rev().fold(
            tail_decomp,
            |tail, (head_slot, head_decomp, tail_slot)| MatchPatternDecomp::ListCons {
                head: Box::new(MatchPatternDecompChild {
                    slot: head_slot,
                    decomp: head_decomp,
                }),
                tail: Box::new(MatchPatternDecompChild {
                    slot: tail_slot,
                    decomp: tail,
                }),
            },
        );
        Ok(decomp)
    }

    fn emit_list_cons_match_pattern_bind(
        &mut self,
        pat: &TypedMatchPattern,
        slot: u32,
        decomp: Option<MatchPatternDecomp>,
        err_span: &Span,
    ) -> Result<(), CodegenError> {
        let mut current_pat = pat;
        let mut current_slot = slot;
        let mut current_decomp = decomp;

        while let TypedMatchPattern::ListCons(head, tail) = current_pat {
            let (head_slot, head_decomp, tail_slot, tail_decomp) = match current_decomp {
                Some(MatchPatternDecomp::ListCons { head, tail }) => {
                    (head.slot, Some(head.decomp), tail.slot, Some(tail.decomp))
                }
                _ => {
                    let head_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::ListHead);
                    self.emit(Opcode::StoreLocal(head_slot));

                    let tail_slot = self.state.next_slot;
                    self.state.next_slot += 1;
                    self.emit(Opcode::LoadLocal(current_slot));
                    self.emit(Opcode::ListTail);
                    self.emit(Opcode::StoreLocal(tail_slot));
                    (head_slot, None, tail_slot, None)
                }
            };
            self.emit_match_pattern_bind(head, head_slot, head_decomp, err_span)?;

            current_pat = tail;
            current_slot = tail_slot;
            current_decomp = tail_decomp;
        }

        self.emit_match_pattern_bind(current_pat, current_slot, current_decomp, err_span)
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

    fn emit_enum_eq_from_slots(&mut self, op: &BinOp, left_slot: u32, right_slot: u32) {
        self.emit(Opcode::LoadLocal(left_slot));
        self.emit(Opcode::GetField { field_index: 0 });
        self.emit(Opcode::LoadLocal(right_slot));
        self.emit(Opcode::GetField { field_index: 0 });
        self.emit(Opcode::EqInt);
        if matches!(op, BinOp::Neq) {
            self.emit(Opcode::NotBool);
        }
    }

    fn emit_eq_dispatch_from_slots(
        &mut self,
        dispatch: &TraitDispatch,
        receiver_ty: &Ty,
        left_slot: u32,
        right_slot: u32,
        span: &Span,
    ) -> Result<(), CodegenError> {
        match dispatch {
            TraitDispatch::Pending => Err(CodegenError {
                message: "bounded trait call must be specialized before codegen".into(),
                span: span.clone(),
            }),
            TraitDispatch::Static(TraitDispatchTarget::BinOp(op))
                if matches!(op, BinOp::Eq | BinOp::Neq)
                    && matches!(receiver_ty, Ty::Enum(_, _)) =>
            {
                self.emit_enum_eq_from_slots(op, left_slot, right_slot);
                Ok(())
            }
            TraitDispatch::Static(TraitDispatchTarget::BinOp(op)) => {
                self.emit(Opcode::LoadLocal(left_slot));
                self.emit(Opcode::LoadLocal(right_slot));
                let opcode = self.binop_to_opcode(op, receiver_ty, span)?;
                self.emit(opcode);
                Ok(())
            }
            TraitDispatch::Static(TraitDispatchTarget::Builtin(name)) => {
                self.emit(Opcode::LoadLocal(left_slot));
                self.emit(Opcode::LoadLocal(right_slot));
                if let Some(opcode) = Self::direct_builtin_opcode(name, 2) {
                    self.emit(opcode);
                } else {
                    let builtin_id = Self::builtin_id(name).ok_or_else(|| CodegenError {
                        message: format!("Unknown builtin: {}", name),
                        span: span.clone(),
                    })?;
                    self.emit(Opcode::CallBuiltin {
                        builtin_id,
                        arity: 2,
                        span_start: span.start as u32,
                        span_end: span.end as u32,
                    });
                }
                Ok(())
            }
            TraitDispatch::Static(TraitDispatchTarget::UserFunction { fun_idx, .. }) => {
                self.emit(Opcode::LoadLocal(left_slot));
                self.emit(Opcode::LoadLocal(right_slot));
                self.emit(Opcode::Call {
                    fun_idx: *fun_idx,
                    arity: 2,
                    span_start: span.start as u32,
                    span_end: span.end as u32,
                });
                Ok(())
            }
        }
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

    fn emit_compare_operator_trait_call(
        &mut self,
        _op: ComparisonOperator,
        dispatch: &TraitDispatch,
        receiver_ty: &Ty,
        args: &[TypedNode],
        span: &Span,
    ) -> Result<(), CodegenError> {
        if args.len() != 2 {
            return Err(CodegenError {
                message: format!(
                    "comparison trait dispatch expects 2 args, got {}",
                    args.len()
                ),
                span: span.clone(),
            });
        }

        match dispatch {
            TraitDispatch::Pending => {
                return Err(CodegenError {
                    message: "bounded trait call must be specialized before codegen".into(),
                    span: span.clone(),
                });
            }
            TraitDispatch::Static(TraitDispatchTarget::BinOp(binop)) => {
                self.emit_node(&args[0])?;
                self.emit_node(&args[1])?;
                let opcode = self.binop_to_opcode(binop, receiver_ty, span)?;
                self.emit(opcode);
                return Ok(());
            }
            TraitDispatch::Static(TraitDispatchTarget::Builtin(name)) => {
                for arg in args {
                    self.emit_node(arg)?;
                }
                let builtin_id = Self::builtin_id(name).ok_or_else(|| CodegenError {
                    message: format!("Unknown builtin: {}", name),
                    span: span.clone(),
                })?;
                self.emit(Opcode::CallBuiltin {
                    builtin_id,
                    arity: args.len() as u8,
                    span_start: span.start as u32,
                    span_end: span.end as u32,
                });
            }
            TraitDispatch::Static(TraitDispatchTarget::UserFunction { fun_idx, .. }) => {
                for arg in args {
                    self.emit_node(arg)?;
                }
                self.emit(Opcode::Call {
                    fun_idx: *fun_idx,
                    arity: args.len() as u8,
                    span_start: span.start as u32,
                    span_end: span.end as u32,
                });
            }
        }
        Ok(())
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

    fn finalize(mut self) -> Result<(Vec<Opcode>, CodegenState), CodegenError> {
        let mut fuse_tail_call = vec![false; self.ir.len()];
        let mut skip_ir = vec![false; self.ir.len()];
        for idx in 0..self.ir.len().saturating_sub(1) {
            if self
                .label_positions
                .values()
                .any(|position| *position == idx + 1)
            {
                continue;
            }
            if matches!(
                (&self.ir[idx], &self.ir[idx + 1]),
                (
                    IrOp::Op(Opcode::CallClosure { .. }),
                    IrOp::Op(Opcode::Return)
                )
            ) {
                fuse_tail_call[idx] = true;
                skip_ir[idx + 1] = true;
            }
        }

        let mut ir_to_pc = vec![0usize; self.ir.len() + 1];
        let mut pc = 0usize;
        for idx in 0..self.ir.len() {
            ir_to_pc[idx] = pc;
            if !skip_ir[idx] {
                pc += 1;
            }
        }
        ir_to_pc[self.ir.len()] = pc;

        for entry in &mut self.state.functions {
            if let Some(pc) = ir_to_pc.get(entry.entry_pc as usize) {
                entry.entry_pc = *pc as u32;
            }
        }

        let resolve_label_pc = |label| -> Result<u32, CodegenError> {
            let ir_pos = self
                .label_positions
                .get(&label)
                .copied()
                .ok_or_else(|| CodegenError {
                    message: format!("unresolved label {:?}", label),
                    span: Span { start: 0, end: 0 },
                })?;
            ir_to_pc
                .get(ir_pos)
                .copied()
                .map(|pc| pc as u32)
                .ok_or_else(|| CodegenError {
                    message: format!("label position out of bounds: {}", ir_pos),
                    span: Span { start: 0, end: 0 },
                })
        };

        // Resolve labels to opcode positions after final IR peepholes.
        let mut opcodes = Vec::new();
        for (idx, ir_op) in self.ir.iter().enumerate() {
            if skip_ir[idx] {
                continue;
            }
            match ir_op {
                IrOp::Op(Opcode::CallClosure {
                    arity,
                    span_start,
                    span_end,
                }) if fuse_tail_call[idx] => opcodes.push(Opcode::TailCallClosure {
                    arity: *arity,
                    span_start: *span_start,
                    span_end: *span_end,
                }),
                IrOp::Op(op) => opcodes.push(op.clone()),
                IrOp::JumpLabel(label) => {
                    let pos = resolve_label_pc(*label).map_err(|mut err| {
                        err.message = format!("unresolved jump label {:?}", label);
                        err
                    })?;
                    opcodes.push(Opcode::Jump(pos));
                }
                IrOp::JumpIfFalseLabel(label) => {
                    let pos = resolve_label_pc(*label).map_err(|mut err| {
                        err.message = format!("unresolved jump-if-false label {:?}", label);
                        err
                    })?;
                    opcodes.push(Opcode::JumpIfFalse(pos));
                }
                IrOp::JumpIfTrueLabel(label) => {
                    let pos = resolve_label_pc(*label).map_err(|mut err| {
                        err.message = format!("unresolved jump-if-true label {:?}", label);
                        err
                    })?;
                    opcodes.push(Opcode::JumpIfTrue(pos));
                }
                IrOp::JumpIfLocalTagEqLabel {
                    local_idx,
                    tag_const_idx,
                    label,
                } => {
                    let pos = resolve_label_pc(*label).map_err(|mut err| {
                        err.message = format!("unresolved jump-if-local-tag-eq label {:?}", label);
                        err
                    })?;
                    opcodes.push(Opcode::JumpIfLocalTagEq {
                        local_idx: *local_idx,
                        tag_const_idx: *tag_const_idx,
                        target_pc: pos,
                    });
                }
                IrOp::JumpIfLocalTagNeLabel {
                    local_idx,
                    tag_const_idx,
                    label,
                } => {
                    let pos = resolve_label_pc(*label).map_err(|mut err| {
                        err.message = format!("unresolved jump-if-local-tag-ne label {:?}", label);
                        err
                    })?;
                    opcodes.push(Opcode::JumpIfLocalTagNe {
                        local_idx: *local_idx,
                        tag_const_idx: *tag_const_idx,
                        target_pc: pos,
                    });
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

#[cfg(test)]
mod process_runtime_v2_tests {
    use super::*;
    use sigil::resolved::ResolvedId;
    use spire::ast::{
        ChildRestartPolicy, ProcessKind, ProcessRuntimeHandlerSpec, ProcessSpec,
        SupervisorInitEntry, SupervisorPolicy, SupervisorStrategy,
    };

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
                state: AstTy::Named(span(0, 0), "Int".to_string()),
                boot: false,
                registry: false,
                standby: false,
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

    fn worker_process_spec(name: &str) -> TypedProcessSpec {
        let mut spec = singleton_process_spec(name);
        spec.spec.instance = ProcessInstance::Worker;
        spec
    }

    fn supervisor_policy() -> SupervisorPolicy {
        SupervisorPolicy {
            strategy: SupervisorStrategy::OneForOne,
            max_restarts: 5,
            max_seconds: 10,
            child_restart_default: ChildRestartPolicy::Transient,
            allow_adopt: true,
            shutdown_timeout_ms: None,
        }
    }

    fn supervisor_process_spec(name: &str, kind: ProcessKind) -> TypedProcessSpec {
        TypedProcessSpec {
            module_path: name.to_string(),
            process_name: name.to_string(),
            spec: ProcessSpec {
                process_name: name.to_string(),
                kind,
                instance: ProcessInstance::Singleton,
                state: AstTy::Named(span(0, 0), "Unit".to_string()),
                boot: false,
                registry: false,
                standby: false,
                handlers: Vec::new(),
                handler_specs: Vec::new(),
                supervisor_policy: Some(supervisor_policy()),
            },
            init_uid: 1,
            get_uid: 2,
            set_uid: None,
            handler_uids: Vec::new(),
        }
    }

    fn supervisor_init_entry(name: &str) -> SupervisorInitEntry {
        SupervisorInitEntry {
            process_name: name.into(),
            timeout_ms: None,
            handlers: Vec::new(),
            overrides: Default::default(),
            span: span(0, 0),
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
                        symbol_info: None,
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

    fn supervisor_status_call(process_name: &str) -> TypedNode {
        TypedNode {
            ty: Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Error)),
            span: span(10, 21),
            node: TypedInner::SupervisorStatus {
                supervisor_process: process_name.into(),
            },
        }
    }

    fn result_int_ty() -> Ty {
        Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error))
    }

    fn worker_init_callable() -> TypedNode {
        let ret_ty = result_int_ty();
        let fun = TypedNode {
            ty: Ty::UserFunc {
                fun_idx: 7,
                type_params: Vec::new(),
                params: Vec::new(),
                ret: Box::new(ret_ty.clone()),
            },
            span: span(30, 42),
            node: TypedInner::Var(ResolvedId {
                name: "init".into(),
                qualified_name: Some("MyWorker::init".into()),
                unique_id: 701,
                compiler_generated: true,
                symbol_info: None,
                span: span(30, 42),
            }),
        };
        TypedNode {
            ty: Ty::Func(Vec::new(), Box::new(ret_ty.clone())),
            span: span(30, 42),
            node: TypedInner::Closure(
                Vec::new(),
                Vec::new(),
                Box::new(TypedNode {
                    ty: ret_ty,
                    span: span(30, 42),
                    node: TypedInner::App(Box::new(fun), Vec::new()),
                }),
            ),
        }
    }

    fn worker_strategy_value() -> TypedNode {
        TypedNode {
            ty: Ty::Struct("WorkerStrategy".into(), Vec::new()),
            span: span(50, 58),
            node: TypedInner::Var(ResolvedId {
                name: "strategy".into(),
                qualified_name: None,
                unique_id: 702,
                compiler_generated: false,
                symbol_info: None,
                span: span(50, 58),
            }),
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

    #[test]
    fn build_runtime_boot_plan_classifies_unified_entries_by_process_kind() {
        let boot_plan = SupervisorInitSpec {
            entries: vec![
                supervisor_init_entry("Logger"),
                supervisor_init_entry("ImageWorkerSupervisor"),
            ],
            ..SupervisorInitSpec::default()
        };

        let runtime = build_runtime_boot_plan(
            &boot_plan,
            &[
                singleton_process_spec("Logger"),
                supervisor_process_spec("ImageWorkerSupervisor", ProcessKind::Supervisor),
            ],
        )
        .expect("unified entries should lower by process kind");

        assert_eq!(runtime.singletons.len(), 1);
        assert_eq!(runtime.singletons[0].process_name, "Logger");
        assert_eq!(runtime.supervisor_overrides.len(), 1);
        assert_eq!(
            runtime.supervisor_overrides[0].process_name,
            "ImageWorkerSupervisor"
        );
    }

    #[test]
    fn build_runtime_boot_plan_rejects_worker_entry() {
        let boot_plan = SupervisorInitSpec {
            entries: vec![supervisor_init_entry("MyWorker")],
            ..SupervisorInitSpec::default()
        };

        let err = build_runtime_boot_plan(&boot_plan, &[worker_process_spec("MyWorker")])
            .expect_err("workers must not appear in supervisor_init");

        assert!(err.message.contains("worker process cannot appear"));
    }

    #[test]
    fn validate_required_singletons_accepts_dynamic_supervisor_without_dsl_entry() {
        validate_required_singletons(
            &[supervisor_status_call("DynamicSupervisor")],
            &[supervisor_process_spec(
                "DynamicSupervisor",
                ProcessKind::DynamicSupervisor,
            )],
            &RuntimeBootPlan::default(),
        )
        .expect("DynamicSupervisor is implicitly registered");
    }

    #[test]
    fn supervisor_spawn_lowers_to_metadata_arity_shape() {
        let mut gene = Codegen::new();
        let node = TypedNode {
            ty: Ty::Result(Box::new(Ty::Pid("MyWorker".into())), Box::new(Ty::Error)),
            span: span(1, 48),
            node: TypedInner::SupervisorSpawn {
                supervisor_process: "MySup".into(),
                worker_process: "MyWorker".into(),
                init: Box::new(worker_init_callable()),
            },
        };

        gene.emit_node(&node)
            .expect("supervisor spawn emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");
        let builtin_id =
            Codegen::builtin_id("__supervisor_spawn").expect("__supervisor_spawn exists");

        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::CallBuiltin {
                builtin_id: actual,
                arity: 2,
                ..
            } if *actual == builtin_id
        )));
        assert!(!opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::CallBuiltin {
                builtin_id: actual,
                arity: 3,
                ..
            } if *actual == builtin_id
        )));
    }

    #[test]
    fn supervisor_workers_lowers_to_metadata_arity_shape() {
        let mut gene = Codegen::new();
        let node = TypedNode {
            ty: Ty::Result(
                Box::new(Ty::Enum("Workers".into(), vec![Ty::Pid("MyWorker".into())])),
                Box::new(Ty::Error),
            ),
            span: span(1, 64),
            node: TypedInner::SupervisorWorkers {
                supervisor_process: "MySup".into(),
                worker_process: "MyWorker".into(),
                init: Box::new(worker_init_callable()),
                strategy: Box::new(worker_strategy_value()),
            },
        };

        gene.emit_node(&node)
            .expect("supervisor workers emission should succeed");
        let (opcodes, _) = gene.finalize().expect("labels should resolve");
        let builtin_id =
            Codegen::builtin_id("__supervisor_workers").expect("__supervisor_workers exists");

        assert!(opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::CallBuiltin {
                builtin_id: actual,
                arity: 3,
                ..
            } if *actual == builtin_id
        )));
        assert!(!opcodes.iter().any(|opcode| matches!(
            opcode,
            Opcode::CallBuiltin {
                builtin_id: actual,
                arity: 4,
                ..
            } if *actual == builtin_id
        )));
    }
}
