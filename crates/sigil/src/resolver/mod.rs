use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::panic;

use serde::{Deserialize, Serialize};
use sindr::builtin::{builtin_function_metas, builtin_uid};
use sindr::names::{
    builtin_symbol_identity_info, FacetRootKind, SymbolCapabilities, SymbolIdentityInfo,
    TypeIdentity,
};
use sindr::warning::PhaseOutput;
use spire::ast::{
    Ast, AstMatchArm, AstPattern, AstTy, ClosureParam, DeclAttrs, ExtractorParam, FunParam, Lit,
    RecordLitArg, Span, StructLitField, SupervisorInitSpec, Visibility,
};

use crate::error::{ResolveError, ResolveErrorLabel};
use crate::resolved::*;
use crate::scope::Scope;

mod captures;
mod declarations;
mod derive;
mod expr;
mod imports;
mod patterns;
mod scope_init;
mod session;
mod special_forms;
#[cfg(test)]
mod tests;
mod warnings;

pub use self::declarations::{
    const_only_fallback_module_path, declaration_stage_ordering, declaration_uid_order,
    extract_process_modules_from_user_ast, lower_module_source_ast, lowered_module_is_impl_owner,
    precollect_declaration_index, staged_modules_from_source_ast, DeclarationEntry,
    DeclarationIndex, DeclarationKind, DeclarationOrdering, LoweredModuleAst,
    StageOrderedDeclaration, StagedModuleAst,
};
pub use self::session::{SigilCheckpoint, SigilSession};

use self::declarations::{
    assign_declaration_uids, collect_stage_impl_target_resolutions, declaration_uid_kind_map,
    trait_impl_method_qualified_name, trait_method_qualified_name, validate_unique_callable_names,
};
use self::expr::validate_trait_impl_pairs_in_nodes;
use self::imports::{build_global_scope, build_module_scope, build_module_scope_with_imports};
use self::warnings::collect_resolution_warnings;

const STAGE_WORKER_STACK_SIZE: usize = 8 * 1024 * 1024;

fn surface_module_name(module_path: &str) -> String {
    module_path
        .strip_prefix("Global::")
        .unwrap_or(module_path)
        .to_string()
}

fn global_surface_name(name: &str) -> &str {
    name.strip_prefix("Global::").unwrap_or(name)
}

fn define_global_surface_alias(scope: &mut Scope, canonical_name: &str, uid: u32) {
    let surface_name = global_surface_name(canonical_name);
    if surface_name != canonical_name {
        scope.define_with_id(surface_name, uid);
    }
}

pub fn user_type_symbol_identity_info(kind: &DeclarationKind) -> Option<SymbolIdentityInfo> {
    let (identity, capabilities) = match kind {
        DeclarationKind::Struct => (
            TypeIdentity::Struct,
            SymbolCapabilities::new(true, true, true, Some(FacetRootKind::TypeRoot)),
        ),
        DeclarationKind::Record => (
            TypeIdentity::Record,
            SymbolCapabilities::new(true, true, true, Some(FacetRootKind::TypeRoot)),
        ),
        DeclarationKind::Enum => (
            TypeIdentity::Enum,
            SymbolCapabilities::new(true, true, true, Some(FacetRootKind::TypeRoot)),
        ),
        DeclarationKind::Deferror => (
            TypeIdentity::ConcreteError,
            SymbolCapabilities::new(true, false, false, None),
        ),
        _ => return None,
    };
    Some(SymbolIdentityInfo::new(identity, capabilities))
}

pub fn declaration_symbol_identity_info(
    name: &str,
    kind: &DeclarationKind,
) -> Option<SymbolIdentityInfo> {
    if matches!(kind, DeclarationKind::BuiltinType) {
        builtin_symbol_identity_info(global_surface_name(name))
    } else {
        user_type_symbol_identity_info(kind)
    }
}

fn auto_import_module_names(module_stages: &[Vec<StagedModuleAst>]) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for stage in module_stages {
        for module in stage {
            let module_name = surface_module_name(&module.module_path);
            if module.auto_import && seen.insert(module_name.clone()) {
                names.push(module_name);
            }
        }
    }
    names
}

fn collect_staged_trait_constructor_slots(
    module_stages: &[Vec<StagedModuleAst>],
    declaration_uids: &HashMap<String, u32>,
) -> HashMap<u32, Vec<String>> {
    let mut result = HashMap::new();
    let mut parents = Vec::new();
    let lookup_uid = |name: &str, module_path: &str| {
        declaration_uids
            .get(name)
            .or_else(|| declaration_uids.get(&format!("{}::{}", module_path, name)))
            .copied()
            .or_else(|| {
                let suffix = format!("::{name}");
                let mut matches = declaration_uids
                    .iter()
                    .filter(|(fq_name, _)| fq_name.ends_with(&suffix))
                    .map(|(_, uid)| *uid);
                let first = matches.next()?;
                matches.next().is_none().then_some(first)
            })
    };
    for module in module_stages.iter().flatten() {
        for stmt in &module.ast {
            let Ast::TraitDef(_, name, _, Some(clause), _, _) = stmt else {
                continue;
            };
            let fq_name = if module.module_path.is_empty() {
                name.clone()
            } else {
                format!("{}::{}", module.module_path, name)
            };
            let Some(uid) = declaration_uids
                .get(&fq_name)
                .or_else(|| declaration_uids.get(name))
                .copied()
            else {
                continue;
            };
            for constraint in &clause.constraints {
                if !matches!(&constraint.subject, AstTy::Named(_, subject) if subject == "Self") {
                    continue;
                }
                for bound in &constraint.bounds {
                    match bound {
                        spire::ast::WhereConstraintRhs::TypeConstructor(_, slots) => {
                            result.insert(
                                uid,
                                slots
                                    .iter()
                                    .filter_map(|slot| match slot {
                                        AstTy::Named(_, name) => Some(name.clone()),
                                        _ => None,
                                    })
                                    .collect(),
                            );
                        }
                        spire::ast::WhereConstraintRhs::Trait(_, parent_name, _) => {
                            if let Some(parent_uid) = lookup_uid(parent_name, &module.module_path) {
                                parents.push((uid, parent_uid));
                            }
                        }
                        spire::ast::WhereConstraintRhs::TraitSlot(..) => {}
                    }
                }
            }
        }
    }
    loop {
        let mut changed = false;
        for (child, parent) in &parents {
            if result.contains_key(child) {
                continue;
            }
            if let Some(slots) = result.get(parent).cloned() {
                result.insert(*child, slots);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    result
}

/// Resolve all identifiers in the AST to unique references.
pub fn resolve(ast: Vec<Ast>) -> Result<Vec<Resolved>, ResolveError> {
    resolve_with_warnings(ast).map(|output| output.value)
}

pub fn resolve_with_warnings(ast: Vec<Ast>) -> Result<PhaseOutput<Vec<Resolved>>, ResolveError> {
    let mut resolver = Resolver::new();
    let resolved = resolver.resolve_program(ast)?;
    let warnings = collect_resolution_warnings(&resolved, &[]);
    Ok(PhaseOutput::new(resolved, warnings))
}

pub fn resolve_staged_program(
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: Vec<Ast>,
    declaration_index: &DeclarationIndex,
    user_module_path: Option<String>,
) -> Result<Vec<Resolved>, ResolveError> {
    resolve_staged_program_from_state(
        module_stages,
        user_ast,
        declaration_index,
        user_module_path,
        0,
        ResolveResumeState::default(),
    )
    .map(|resolved| resolved.resolved)
}

pub fn resolve_staged_program_with_warnings(
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: Vec<Ast>,
    declaration_index: &DeclarationIndex,
    user_module_path: Option<String>,
) -> Result<PhaseOutput<Vec<Resolved>>, ResolveError> {
    resolve_staged_program_from_state_with_warnings(
        module_stages,
        user_ast,
        declaration_index,
        user_module_path,
        0,
        ResolveResumeState::default(),
    )
    .map(|output| {
        let mut program = output.value;
        let resolved = std::mem::take(&mut program.resolved);
        PhaseOutput::new(resolved, output.warnings)
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveResumeState {
    pub next_local_id: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStagedProgram {
    pub resolved: Vec<Resolved>,
    pub process_specs: Vec<ResolvedProcessSpec>,
    pub boot_plan: SupervisorInitSpec,
    pub resume_state: ResolveResumeState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExplicitFunctionImport {
    pub uid: u32,
    pub alias: String,
    pub fq_name: String,
    pub span: Span,
    pub kind: DeclarationKind,
}

pub fn resolve_staged_program_with_state(
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: Vec<Ast>,
    declaration_index: &DeclarationIndex,
    user_module_path: Option<String>,
) -> Result<ResolvedStagedProgram, ResolveError> {
    resolve_staged_program_from_state_with_warnings(
        module_stages,
        user_ast,
        declaration_index,
        user_module_path,
        0,
        ResolveResumeState::default(),
    )
    .map(|output| output.value)
}

pub fn resolve_staged_program_from_state(
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: Vec<Ast>,
    declaration_index: &DeclarationIndex,
    user_module_path: Option<String>,
    start_stage_index: usize,
    resume_state: ResolveResumeState,
) -> Result<ResolvedStagedProgram, ResolveError> {
    resolve_staged_program_from_state_with_warnings(
        module_stages,
        user_ast,
        declaration_index,
        user_module_path,
        start_stage_index,
        resume_state,
    )
    .map(|output| output.value)
}

pub fn resolve_staged_program_with_state_with_warnings(
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: Vec<Ast>,
    declaration_index: &DeclarationIndex,
    user_module_path: Option<String>,
) -> Result<PhaseOutput<ResolvedStagedProgram>, ResolveError> {
    resolve_staged_program_from_state_with_warnings(
        module_stages,
        user_ast,
        declaration_index,
        user_module_path,
        0,
        ResolveResumeState::default(),
    )
}

pub fn resolve_staged_program_from_state_with_warnings(
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: Vec<Ast>,
    declaration_index: &DeclarationIndex,
    user_module_path: Option<String>,
    start_stage_index: usize,
    resume_state: ResolveResumeState,
) -> Result<PhaseOutput<ResolvedStagedProgram>, ResolveError> {
    let declaration_uids = assign_declaration_uids(declaration_index);
    let declaration_uid_kinds = declaration_uid_kind_map(declaration_index, &declaration_uids);
    let trait_constructor_slots =
        collect_staged_trait_constructor_slots(module_stages, &declaration_uids);
    let declaration_hidden_by_uid = declaration_index
        .iter()
        .filter_map(|(fq_name, entry)| {
            declaration_uids
                .get(fq_name)
                .copied()
                .map(|uid| (uid, entry.hidden))
        })
        .collect::<HashMap<_, _>>();
    let global_scope = build_global_scope(declaration_index, &declaration_uids);
    let auto_import_modules = auto_import_module_names(module_stages);
    let mut resolved = Vec::new();
    let mut explicit_function_imports = Vec::new();
    let mut process_specs = Vec::new();
    let mut boot_plan = SupervisorInitSpec::default();
    let mut next_local_id = declaration_uids
        .values()
        .copied()
        .max()
        .map(|uid| uid + 1)
        .unwrap_or_else(|| global_scope.next_id());
    next_local_id = next_local_id.max(global_scope.next_id());
    next_local_id = next_local_id.max(resume_state.next_local_id);

    for (stage_index, stage) in module_stages.iter().enumerate().skip(start_stage_index) {
        let stage_impl_targets = collect_stage_impl_target_resolutions(stage);
        let stage_local_base = next_local_id;
        let stage_results = resolve_stage_modules_parallel(
            stage,
            stage_index,
            stage_local_base,
            &global_scope,
            &auto_import_modules,
            declaration_index,
            &declaration_uids,
            &declaration_uid_kinds,
            &declaration_hidden_by_uid,
            &trait_constructor_slots,
            &stage_impl_targets,
        );

        let mut offset = 0u32;
        for result in stage_results {
            let mut result = result?;
            rebase_resolved_nodes(&mut result.resolved, stage_local_base, offset);
            resolved.extend(result.resolved);
            explicit_function_imports.extend(result.explicit_function_imports);
            offset = offset.saturating_add(result.local_id_count);
        }
        next_local_id = stage_local_base.saturating_add(offset);

        for module in stage {
            collect_supervisor_init_specs(&module.ast, &mut boot_plan);
            if let Some(spec) = &module.process_spec {
                let init_fq = format!("{}::__agent_init", module.module_path);
                let get_fq = format!("{}::__agent_get", module.module_path);
                let set_fq = format!("{}::__agent_set", module.module_path);
                let init_uid = *declaration_uids.get(&init_fq).ok_or_else(|| ResolveError {
                    message: format!("missing lowered init handler `{init_fq}`"),
                    span: Span { start: 0, end: 0 },
                    related_labels: Vec::new(),
                })?;
                let get_uid = *declaration_uids.get(&get_fq).ok_or_else(|| ResolveError {
                    message: format!("missing lowered get handler `{get_fq}`"),
                    span: Span { start: 0, end: 0 },
                    related_labels: Vec::new(),
                })?;
                let set_uid = declaration_uids.get(&set_fq).copied();
                let handler_uids = spec
                    .handler_specs
                    .iter()
                    .map(|handler| {
                        let fq_name = if handler.internal_name.is_empty() {
                            format!("{}::{}", module.module_path, handler.name)
                        } else {
                            format!("{}::{}", module.module_path, handler.internal_name)
                        };
                        declaration_uids
                            .get(&fq_name)
                            .copied()
                            .map(|uid| ResolvedProcessHandlerUid {
                                internal_name: handler.internal_name.clone(),
                                uid,
                            })
                            .ok_or_else(|| ResolveError {
                                message: format!("missing lowered process handler `{fq_name}`"),
                                span: handler.span.clone(),
                                related_labels: Vec::new(),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                process_specs.push(ResolvedProcessSpec {
                    module_path: module.module_path.clone(),
                    process_name: spec.process_name.clone(),
                    spec: spec.clone(),
                    init_uid,
                    get_uid,
                    set_uid,
                    handler_uids,
                });
            }
        }
    }

    if !user_ast.is_empty() {
        collect_supervisor_init_specs(&user_ast, &mut boot_plan);
        let user_scope_build = build_module_scope_with_imports(
            &global_scope,
            &auto_import_modules,
            declaration_index,
            &declaration_uids,
            &declaration_uid_kinds,
            &user_ast,
            user_module_path.as_deref(),
            module_stages.len(),
        )?;
        explicit_function_imports.extend(user_scope_build.explicit_function_imports);
        let mut user_scope = user_scope_build.scope;
        user_scope.advance_next_id_to(next_local_id);
        let mut user_resolver = Resolver::with_scope(user_scope);
        user_resolver.declaration_entries = declaration_index.clone().into_iter().collect();
        user_resolver.declaration_uids = declaration_uids;
        user_resolver.declaration_uid_kinds = declaration_uid_kinds;
        user_resolver.declaration_hidden_by_uid = declaration_hidden_by_uid;
        user_resolver.trait_constructor_slots = trait_constructor_slots;
        user_resolver.current_module_path = user_module_path;
        user_resolver.allow_top_level_shadowing = true;
        resolved.extend(user_resolver.resolve_program(user_ast)?);
        next_local_id = user_resolver.scope.next_id();
    }

    validate_trait_impl_pairs_in_nodes(&resolved)?;
    let warnings = collect_resolution_warnings(&resolved, &explicit_function_imports);

    Ok(PhaseOutput::new(
        ResolvedStagedProgram {
            resolved,
            process_specs,
            boot_plan,
            resume_state: ResolveResumeState { next_local_id },
        },
        warnings,
    ))
}

fn collect_supervisor_init_specs(stmts: &[Ast], boot_plan: &mut SupervisorInitSpec) {
    for stmt in stmts {
        match stmt {
            Ast::SupervisorInit(_, spec) => {
                boot_plan.entries.extend(spec.entries.clone());
                boot_plan.singletons.extend(spec.singletons.clone());
                boot_plan.supervisors.extend(spec.supervisors.clone());
            }
            Ast::Namespace(_, _, body) | Ast::Defmod(_, _, body, _) => {
                collect_supervisor_init_specs(body, boot_plan);
            }
            _ => {}
        }
    }
}

struct StageModuleResolveResult {
    resolved: Vec<Resolved>,
    local_id_count: u32,
    explicit_function_imports: Vec<ExplicitFunctionImport>,
}

fn resolve_stage_modules_parallel(
    stage: &[StagedModuleAst],
    stage_index: usize,
    stage_local_base: u32,
    global_scope: &Scope,
    auto_import_modules: &[String],
    declaration_index: &DeclarationIndex,
    declaration_uids: &HashMap<String, u32>,
    declaration_uid_kinds: &HashMap<u32, DeclarationKind>,
    declaration_hidden_by_uid: &HashMap<u32, bool>,
    trait_constructor_slots: &HashMap<u32, Vec<String>>,
    stage_impl_targets: &HashMap<String, declarations::ImplTargetResolution>,
) -> Vec<Result<StageModuleResolveResult, ResolveError>> {
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(stage.len());
        for module in stage {
            let handle = std::thread::Builder::new()
                .stack_size(STAGE_WORKER_STACK_SIZE)
                .spawn_scoped(scope, move || {
                    let module_scope_build = build_module_scope_with_imports(
                        global_scope,
                        auto_import_modules,
                        declaration_index,
                        declaration_uids,
                        declaration_uid_kinds,
                        &module.ast,
                        Some(module.module_path.as_str()),
                        stage_index,
                    )?;
                    let mut module_scope = module_scope_build.scope;
                    module_scope.advance_next_id_to(stage_local_base);
                    let mut resolver = Resolver::with_scope(module_scope);
                    resolver.current_module_path = Some(module.module_path.clone());
                    resolver.declaration_entries = declaration_index.clone().into_iter().collect();
                    resolver.declaration_uids = declaration_uids.clone();
                    resolver.declaration_uid_kinds = declaration_uid_kinds.clone();
                    resolver.declaration_hidden_by_uid = declaration_hidden_by_uid.clone();
                    resolver.trait_constructor_slots = trait_constructor_slots.clone();
                    resolver.current_stage_impl_targets = Some(stage_impl_targets.clone());
                    resolver.allow_top_level_shadowing = true;
                    let resolved = resolver.resolve_program(module.ast.clone())?;
                    let local_id_count = resolver.scope.next_id().saturating_sub(stage_local_base);
                    Ok(StageModuleResolveResult {
                        resolved,
                        local_id_count,
                        explicit_function_imports: module_scope_build.explicit_function_imports,
                    })
                });
            handles.push(handle.map_err(|err| ResolveError {
                message: format!("failed to spawn stage resolver worker: {}", err),
                span: Span { start: 0, end: 0 },
                related_labels: Vec::new(),
            }));
        }

        handles
            .into_iter()
            .map(|handle| match handle {
                Ok(handle) => match handle.join() {
                    Ok(result) => result,
                    Err(payload) => panic::resume_unwind(payload),
                },
                Err(err) => Err(err),
            })
            .collect()
    })
}

fn rebase_resolved_id(id: &mut ResolvedId, base: u32, offset: u32) {
    if id.unique_id >= base {
        id.unique_id = id.unique_id.saturating_add(offset);
    }
}

fn rebase_resolved_nodes(nodes: &mut [Resolved], base: u32, offset: u32) {
    if offset == 0 {
        return;
    }
    for node in nodes {
        rebase_resolved_node(node, base, offset);
    }
}

fn rebase_where_clause(clause: &mut ResolvedWhereClause, base: u32, offset: u32) {
    for constraint in &mut clause.constraints {
        for bound in &mut constraint.bounds {
            match bound {
                ResolvedWhereConstraintRhs::Trait { trait_id, .. } => {
                    rebase_resolved_id(trait_id, base, offset)
                }
                ResolvedWhereConstraintRhs::TraitSlot { trait_id, .. } => {
                    rebase_resolved_id(trait_id, base, offset)
                }
                ResolvedWhereConstraintRhs::TypeConstructor { .. } => {}
            }
        }
    }
}

fn rebase_resolved_node(node: &mut Resolved, base: u32, offset: u32) {
    match node {
        Resolved::Lit(..) | Resolved::ListNil(_) => {}
        Resolved::Var(_, id) => rebase_resolved_id(id, base, offset),
        Resolved::App(_, func, args) => {
            rebase_resolved_node(func, base, offset);
            for arg in args {
                rebase_record_arg(arg, base, offset);
            }
        }
        Resolved::TypeApply(_, target, _) => rebase_resolved_node(target, base, offset),
        Resolved::Block(_, nodes)
        | Resolved::ListLiteral(_, nodes)
        | Resolved::TupleLiteral(_, nodes) => {
            rebase_resolved_nodes(nodes, base, offset);
        }
        Resolved::HashMapLiteral(_, entries) => {
            for entry in entries {
                rebase_resolved_node(&mut entry.key, base, offset);
                rebase_resolved_node(&mut entry.value, base, offset);
            }
        }
        Resolved::RangeLiteral(_, start, stop) => {
            rebase_resolved_node(start, base, offset);
            rebase_resolved_node(stop, base, offset);
        }
        Resolved::Bind(_, pattern, rhs) | Resolved::SafeBind(_, pattern, rhs) => {
            rebase_pattern(pattern, base, offset);
            rebase_resolved_node(rhs, base, offset);
        }
        Resolved::BinOp(_, _, left, right)
        | Resolved::Pipe(_, left, right)
        | Resolved::ContextMap(_, left, right)
        | Resolved::ContextApply(_, left, right)
        | Resolved::ContextBind(_, left, right)
        | Resolved::Compose(_, left, right)
        | Resolved::LiftedCompose(_, left, right)
        | Resolved::KleisliCompose(_, left, right)
        | Resolved::ListCons(_, left, right) => {
            rebase_resolved_node(left, base, offset);
            rebase_resolved_node(right, base, offset);
        }
        Resolved::Grouped(_, inner)
        | Resolved::FieldAccess(_, inner, _)
        | Resolved::FacetSegmentAccess(_, inner, _)
        | Resolved::FacetCapture(_, inner)
        | Resolved::Semi(_, inner) => {
            rebase_resolved_node(inner, base, offset);
        }
        Resolved::InferredFacetCapture(_, _) => {}
        Resolved::ProcessContextHandler(_, _) => {}
        Resolved::Dbg(_, nodes) => {
            rebase_resolved_nodes(nodes, base, offset);
        }
        Resolved::InterpolatedStr(_, parts) => {
            for part in parts {
                if let ResolvedInterpolatedPart::Expr(expr) = part {
                    rebase_resolved_node(expr, base, offset);
                }
            }
        }
        Resolved::If(_, cond, then_branch, else_branch) => {
            rebase_resolved_node(cond, base, offset);
            rebase_resolved_node(then_branch, base, offset);
            if let Some(else_branch) = else_branch {
                rebase_resolved_node(else_branch, base, offset);
            }
        }
        Resolved::Assert(_, flag, err) => {
            rebase_resolved_node(flag, base, offset);
            rebase_resolved_node(err, base, offset);
        }
        Resolved::Ensure(_, value, pred, err) => {
            rebase_resolved_node(value, base, offset);
            rebase_resolved_node(pred, base, offset);
            rebase_resolved_node(err, base, offset);
        }
        Resolved::MapErr(_, value, err) | Resolved::Cause(_, value, err) => {
            rebase_resolved_node(value, base, offset);
            rebase_resolved_node(err, base, offset);
        }
        Resolved::RecoverKind(_, value, marker, handler) => {
            rebase_resolved_node(value, base, offset);
            rebase_resolved_node(marker, base, offset);
            rebase_resolved_node(handler, base, offset);
        }
        Resolved::Match(_, scrutinee, arms) => {
            rebase_resolved_node(scrutinee, base, offset);
            for arm in arms {
                rebase_pattern(&mut arm.pattern, base, offset);
                if let Some(guard) = &mut arm.guard {
                    rebase_resolved_node(guard, base, offset);
                }
                rebase_resolved_node(&mut arm.body, base, offset);
            }
        }
        Resolved::StructLit(_, id, fields) => {
            rebase_resolved_id(id, base, offset);
            for field in fields {
                match field {
                    ResolvedStructLitField::Explicit(_, expr)
                    | ResolvedStructLitField::Shorthand(_, expr) => {
                        rebase_resolved_node(expr, base, offset);
                    }
                }
            }
        }
        Resolved::ConstructorCall(_, id, args) => {
            rebase_resolved_id(id, base, offset);
            for arg in args {
                rebase_record_arg(arg, base, offset);
            }
        }
        Resolved::StructDef(_, id, _, fields, _) | Resolved::RecordDef(_, id, fields, _) => {
            rebase_resolved_id(id, base, offset);
            rebase_fields(fields, base, offset);
        }
        Resolved::DeferrorDef(_, id, fields, show_expr) => {
            rebase_resolved_id(id, base, offset);
            rebase_fields(fields, base, offset);
            rebase_resolved_node(show_expr, base, offset);
        }
        Resolved::EnumDef(_, id, type_params, variants, _) => {
            rebase_resolved_id(id, base, offset);
            rebase_type_params(type_params, base, offset);
            for variant in variants {
                rebase_resolved_id(&mut variant.id, base, offset);
            }
        }
        Resolved::Def(_, id, type_params, params, _, where_clause, body, _) => {
            rebase_resolved_id(id, base, offset);
            rebase_type_params(type_params, base, offset);
            rebase_fun_params(params, base, offset);
            if let Some(clause) = where_clause {
                rebase_where_clause(clause, base, offset);
            }
            rebase_resolved_node(body, base, offset);
        }
        Resolved::ConstDef(_, id, _, value, _) => {
            rebase_resolved_id(id, base, offset);
            rebase_resolved_node(value, base, offset);
        }
        Resolved::ExtractorDef(_, id, type_params, param, _, body, _) => {
            rebase_resolved_id(id, base, offset);
            rebase_type_params(type_params, base, offset);
            rebase_extractor_param(param, base, offset);
            rebase_resolved_node(body, base, offset);
        }
        Resolved::TraitDef(_, id, type_params, where_clause, methods, _) => {
            rebase_resolved_id(id, base, offset);
            rebase_type_params(type_params, base, offset);
            if let Some(clause) = where_clause {
                rebase_where_clause(clause, base, offset);
            }
            for method in methods {
                rebase_resolved_id(&mut method.id, base, offset);
                rebase_type_params(&mut method.type_params, base, offset);
                rebase_fun_params(&mut method.params, base, offset);
                if let Some(clause) = &mut method.where_clause {
                    rebase_where_clause(clause, base, offset);
                }
                if let Some(body) = &mut method.body {
                    rebase_resolved_node(body, base, offset);
                }
            }
        }
        Resolved::TraitImplDef(_, id, _, _, where_clause, methods) => {
            rebase_resolved_id(id, base, offset);
            if let Some(clause) = where_clause {
                rebase_where_clause(clause, base, offset);
            }
            for method in methods {
                rebase_resolved_id(&mut method.function_id, base, offset);
                rebase_type_params(&mut method.type_params, base, offset);
                rebase_fun_params(&mut method.params, base, offset);
                if let Some(clause) = &mut method.where_clause {
                    rebase_where_clause(clause, base, offset);
                }
                rebase_resolved_node(&mut method.body, base, offset);
            }
        }
        Resolved::BuiltinDecl(_, id, params, _, _) => {
            rebase_resolved_id(id, base, offset);
            rebase_fun_params(params, base, offset);
        }
        Resolved::BuiltinExtractorDecl(_, id, param, _, _) => {
            rebase_resolved_id(id, base, offset);
            rebase_extractor_param(param, base, offset);
        }
        Resolved::BuiltinTypeDecl(_, id, _, _) => rebase_resolved_id(id, base, offset),
        Resolved::TypeAlias(_, _, _, _) => {}
        Resolved::ResultCtorDecl(_, id, _, _, _) => rebase_resolved_id(id, base, offset),
        Resolved::Closure(_, params, captures, body) => {
            for param in params {
                rebase_resolved_id(&mut param.id, base, offset);
            }
            for capture in captures {
                rebase_resolved_id(capture, base, offset);
            }
            rebase_resolved_node(body, base, offset);
        }
        Resolved::Capture(_, target, args) => {
            rebase_resolved_node(target, base, offset);
            rebase_resolved_nodes(args, base, offset);
        }
    }
}

fn rebase_record_arg(arg: &mut ResolvedRecordLitArg, base: u32, offset: u32) {
    match arg {
        ResolvedRecordLitArg::Positional(expr) | ResolvedRecordLitArg::Named(_, expr) => {
            rebase_resolved_node(expr, base, offset);
        }
    }
}

fn rebase_pattern(pattern: &mut ResolvedPattern, base: u32, offset: u32) {
    match pattern {
        ResolvedPattern::Var(id) | ResolvedPattern::Annotated(id, _) | ResolvedPattern::Pin(id) => {
            rebase_resolved_id(id, base, offset);
        }
        ResolvedPattern::Wildcard(_)
        | ResolvedPattern::ListNil(_)
        | ResolvedPattern::IntLit(..)
        | ResolvedPattern::StrLit(..)
        | ResolvedPattern::BoolLit(..)
        | ResolvedPattern::DurationLit(..) => {}
        ResolvedPattern::ListCons(head, tail) => {
            rebase_pattern(head, base, offset);
            rebase_pattern(tail, base, offset);
        }
        ResolvedPattern::Constructor(id, inners) | ResolvedPattern::Extractor(id, inners) => {
            rebase_resolved_id(id, base, offset);
            for inner in inners {
                rebase_pattern(inner, base, offset);
            }
        }
        ResolvedPattern::Tuple(inners) | ResolvedPattern::Or(inners) => {
            for inner in inners {
                rebase_pattern(inner, base, offset);
            }
        }
        ResolvedPattern::As(inner, id, _) => {
            rebase_pattern(inner, base, offset);
            rebase_resolved_id(id, base, offset);
        }
    }
}

fn rebase_fields(fields: &mut [ResolvedField], base: u32, offset: u32) {
    for field in fields {
        if let Some(id) = &mut field.id {
            rebase_resolved_id(id, base, offset);
        }
    }
}

fn rebase_fun_params(params: &mut [ResolvedFunParam], base: u32, offset: u32) {
    for param in params {
        rebase_resolved_id(&mut param.id, base, offset);
    }
}

fn rebase_extractor_param(param: &mut ResolvedExtractorParam, base: u32, offset: u32) {
    rebase_resolved_id(&mut param.id, base, offset);
}

fn rebase_type_params(_params: &mut [ResolvedTypeParam], _base: u32, _offset: u32) {}

pub fn build_scope_for_module(
    module_stages: &[Vec<StagedModuleAst>],
    current_module_path: Option<&str>,
    current_stage_index: usize,
) -> Result<Scope, ResolveError> {
    let declaration_index = precollect_declaration_index(module_stages)?;
    let declaration_uids = assign_declaration_uids(&declaration_index);
    let declaration_uid_kinds = declaration_uid_kind_map(&declaration_index, &declaration_uids);
    let global_scope = build_global_scope(&declaration_index, &declaration_uids);
    let auto_import_modules = auto_import_module_names(module_stages);
    build_module_scope(
        &global_scope,
        &auto_import_modules,
        &declaration_index,
        &declaration_uids,
        &declaration_uid_kinds,
        &[],
        current_module_path,
        current_stage_index,
    )
}

pub fn effective_auto_import_entries(
    module_stages: &[Vec<StagedModuleAst>],
    current_module_path: Option<&str>,
    current_stage_index: usize,
) -> Result<Vec<DeclarationEntry>, ResolveError> {
    let declaration_index = precollect_declaration_index(module_stages)?;
    let declaration_uids = assign_declaration_uids(&declaration_index);
    let declaration_uid_kinds = declaration_uid_kind_map(&declaration_index, &declaration_uids);
    let global_scope = build_global_scope(&declaration_index, &declaration_uids);
    let auto_import_modules = auto_import_module_names(module_stages);
    let build = build_module_scope_with_imports(
        &global_scope,
        &auto_import_modules,
        &declaration_index,
        &declaration_uids,
        &declaration_uid_kinds,
        &[],
        current_module_path,
        current_stage_index,
    )?;
    Ok(build
        .effective_auto_import_fq_names
        .into_iter()
        .filter_map(|fq_name| declaration_index.get(&fq_name).cloned())
        .collect())
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveVisibleEntry {
    pub visible_name: String,
    pub entry: DeclarationEntry,
    pub via_import: bool,
    pub via_auto_import: bool,
    pub shadowed_auto_import: bool,
    pub importable: bool,
    pub callable: bool,
}

fn collect_effective_visible_entries(
    scope: &Scope,
    entries_by_uid: &HashMap<u32, DeclarationEntry>,
    explicit_imports: &[ExplicitFunctionImport],
    effective_auto_import_fq_names: &[String],
    shadowed_auto_import_bindings: &[(String, u32)],
) -> Vec<EffectiveVisibleEntry> {
    let mut visible = Vec::new();
    let mut seen = HashSet::new();
    for (name, uid) in scope.bindings() {
        let Some(entry) = entries_by_uid.get(&uid) else {
            continue;
        };
        if entry.hidden || (!entry.user_importable && !entry.user_callable) {
            continue;
        }
        let visible_name = global_surface_name(name).to_string();
        if !seen.insert((visible_name.clone(), entry.fq_name.clone())) {
            continue;
        }
        let via_import = explicit_imports
            .iter()
            .any(|import| import.uid == uid && import.alias == visible_name);
        let via_auto_import = !via_import
            && effective_auto_import_fq_names
                .iter()
                .any(|fq_name| fq_name == &entry.fq_name)
            && visible_name == global_surface_name(&entry.name);
        let shadowed_auto_import = shadowed_auto_import_bindings
            .iter()
            .any(|(shadow_name, shadow_uid)| shadow_name == &visible_name && *shadow_uid == uid);
        visible.push(EffectiveVisibleEntry {
            visible_name,
            via_import,
            via_auto_import,
            shadowed_auto_import,
            importable: entry.user_importable,
            callable: entry.user_callable,
            entry: entry.clone(),
        });
    }
    visible.sort_by(|left, right| {
        left.visible_name
            .cmp(&right.visible_name)
            .then_with(|| left.entry.fq_name.cmp(&right.entry.fq_name))
    });
    visible
}

pub fn effective_visible_entries(
    module_stages: &[Vec<StagedModuleAst>],
    stmts: &[Ast],
    current_module_path: Option<&str>,
    current_stage_index: usize,
) -> Result<Vec<EffectiveVisibleEntry>, ResolveError> {
    let declaration_index = precollect_declaration_index(module_stages)?;
    let declaration_uids = assign_declaration_uids(&declaration_index);
    let declaration_uid_kinds = declaration_uid_kind_map(&declaration_index, &declaration_uids);
    let global_scope = build_global_scope(&declaration_index, &declaration_uids);
    let auto_import_modules = auto_import_module_names(module_stages);
    let build = build_module_scope_with_imports(
        &global_scope,
        &auto_import_modules,
        &declaration_index,
        &declaration_uids,
        &declaration_uid_kinds,
        stmts,
        current_module_path,
        current_stage_index,
    )?;
    let entries_by_uid = declaration_uids
        .iter()
        .filter_map(|(fq_name, uid)| {
            declaration_index
                .get(fq_name)
                .cloned()
                .map(|entry| (*uid, entry))
        })
        .collect::<HashMap<_, _>>();
    Ok(collect_effective_visible_entries(
        &build.scope,
        &entries_by_uid,
        &build.explicit_function_imports,
        &build.effective_auto_import_fq_names,
        &build.shadowed_auto_import_bindings,
    ))
}

struct Resolver {
    scope: Scope,
    /// Fresh IDs reserved in predeclaration order for each top-level declaration name.
    predeclared_ids: HashMap<String, VecDeque<u32>>,
    declaration_entries: HashMap<String, DeclarationEntry>,
    declaration_uids: HashMap<String, u32>,
    declaration_uid_kinds: HashMap<u32, DeclarationKind>,
    declaration_hidden_by_uid: HashMap<u32, bool>,
    trait_constructor_slots: HashMap<u32, Vec<String>>,
    explicit_module_imports: HashSet<String>,
    current_module_path: Option<String>,
    current_stage_impl_targets: Option<HashMap<String, declarations::ImplTargetResolution>>,
    allow_top_level_shadowing: bool,
    forbidden_top_level_value_bindings: HashMap<u32, String>,
    current_top_level_def_name: Option<String>,
}
