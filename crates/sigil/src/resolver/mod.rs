use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::panic;

use serde::{Deserialize, Serialize};
use sindr::builtin::{builtin_uid, BUILTIN_METAS};
use spire::ast::{
    Ast, AstMatchArm, AstPattern, AstTy, ClosureParam, DeclAttrs, ExtractorParam, FunParam, Lit,
    RecordLitArg, Span, Visibility,
};

use crate::error::{ResolveError, ResolveErrorLabel};
use crate::resolved::*;
use crate::scope::Scope;

mod captures;
mod declarations;
mod expr;
mod imports;
mod patterns;
mod scope_init;
mod session;
mod special_forms;
#[cfg(test)]
mod tests;

pub use self::declarations::{
    precollect_declaration_index, DeclarationEntry, DeclarationIndex, DeclarationKind,
    StagedModuleAst,
};
pub use self::session::{SigilCheckpoint, SigilSession};

use self::declarations::{
    assign_declaration_uids, collect_stage_impl_target_resolutions, declaration_uid_kind_map,
    trait_impl_method_qualified_name, trait_method_qualified_name,
};
use self::imports::{build_global_scope, build_module_scope};

const STAGE_WORKER_STACK_SIZE: usize = 8 * 1024 * 1024;

fn auto_import_module_names(module_stages: &[Vec<StagedModuleAst>]) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for stage in module_stages {
        for module in stage {
            if module.auto_import && seen.insert(module.module_path.clone()) {
                names.push(module.module_path.clone());
            }
        }
    }
    names
}

/// Resolve all identifiers in the AST to unique references.
pub fn resolve(ast: Vec<Ast>) -> Result<Vec<Resolved>, ResolveError> {
    let mut resolver = Resolver::new();
    resolver.resolve_program(ast)
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveResumeState {
    pub next_local_id: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStagedProgram {
    pub resolved: Vec<Resolved>,
    pub resume_state: ResolveResumeState,
}

pub fn resolve_staged_program_with_state(
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: Vec<Ast>,
    declaration_index: &DeclarationIndex,
    user_module_path: Option<String>,
) -> Result<ResolvedStagedProgram, ResolveError> {
    resolve_staged_program_from_state(
        module_stages,
        user_ast,
        declaration_index,
        user_module_path,
        0,
        ResolveResumeState::default(),
    )
}

pub fn resolve_staged_program_from_state(
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: Vec<Ast>,
    declaration_index: &DeclarationIndex,
    user_module_path: Option<String>,
    start_stage_index: usize,
    resume_state: ResolveResumeState,
) -> Result<ResolvedStagedProgram, ResolveError> {
    let declaration_uids = assign_declaration_uids(declaration_index);
    let declaration_uid_kinds = declaration_uid_kind_map(declaration_index, &declaration_uids);
    let global_scope = build_global_scope(declaration_index, &declaration_uids);
    let auto_import_modules = auto_import_module_names(module_stages);
    let mut resolved = Vec::new();
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
            &stage_impl_targets,
        );

        let mut offset = 0u32;
        for result in stage_results {
            let mut result = result?;
            rebase_resolved_nodes(&mut result.resolved, stage_local_base, offset);
            resolved.extend(result.resolved);
            offset = offset.saturating_add(result.local_id_count);
        }
        next_local_id = stage_local_base.saturating_add(offset);
    }

    if !user_ast.is_empty() {
        let mut user_scope = build_module_scope(
            &global_scope,
            &auto_import_modules,
            declaration_index,
            &declaration_uids,
            &declaration_uid_kinds,
            &user_ast,
            user_module_path.as_deref(),
            module_stages.len(),
        )?;
        user_scope.advance_next_id_to(next_local_id);
        let mut user_resolver = Resolver::with_scope(user_scope);
        user_resolver.declaration_uids = declaration_uids;
        user_resolver.declaration_uid_kinds = declaration_uid_kinds;
        user_resolver.current_module_path = user_module_path;
        user_resolver.allow_top_level_shadowing = true;
        resolved.extend(user_resolver.resolve_program(user_ast)?);
        next_local_id = user_resolver.scope.next_id();
    }

    Ok(ResolvedStagedProgram {
        resolved,
        resume_state: ResolveResumeState { next_local_id },
    })
}

struct StageModuleResolveResult {
    resolved: Vec<Resolved>,
    local_id_count: u32,
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
    stage_impl_targets: &HashMap<String, declarations::ImplTargetResolution>,
) -> Vec<Result<StageModuleResolveResult, ResolveError>> {
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(stage.len());
        for module in stage {
            handles.push(
                std::thread::Builder::new()
                    .stack_size(STAGE_WORKER_STACK_SIZE)
                    .spawn_scoped(scope, move || {
                        let mut module_scope = build_module_scope(
                            global_scope,
                            auto_import_modules,
                            declaration_index,
                            declaration_uids,
                            declaration_uid_kinds,
                            &module.ast,
                            Some(module.module_path.as_str()),
                            stage_index,
                        )?;
                        module_scope.advance_next_id_to(stage_local_base);
                        let mut resolver = Resolver::with_scope(module_scope);
                        resolver.current_module_path = Some(module.module_path.clone());
                        resolver.declaration_uids = declaration_uids.clone();
                        resolver.declaration_uid_kinds = declaration_uid_kinds.clone();
                        resolver.current_stage_impl_targets = Some(stage_impl_targets.clone());
                        resolver.allow_top_level_shadowing = true;
                        let resolved = resolver.resolve_program(module.ast.clone())?;
                        let local_id_count =
                            resolver.scope.next_id().saturating_sub(stage_local_base);
                        Ok(StageModuleResolveResult {
                            resolved,
                            local_id_count,
                        })
                    })
                    .expect("stage resolver worker thread should spawn"),
            );
        }

        handles
            .into_iter()
            .map(|handle| match handle.join() {
                Ok(result) => result,
                Err(payload) => panic::resume_unwind(payload),
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

fn rebase_resolved_node(node: &mut Resolved, base: u32, offset: u32) {
    match node {
        Resolved::Lit(..) | Resolved::ListNil(_) | Resolved::TypeRefWitness(..) => {}
        Resolved::Var(_, id) => rebase_resolved_id(id, base, offset),
        Resolved::App(_, func, args) => {
            rebase_resolved_node(func, base, offset);
            for arg in args {
                rebase_record_arg(arg, base, offset);
            }
        }
        Resolved::Block(_, nodes)
        | Resolved::ListLiteral(_, nodes)
        | Resolved::TupleLiteral(_, nodes) => {
            rebase_resolved_nodes(nodes, base, offset);
        }
        Resolved::Bind(_, pattern, rhs) | Resolved::SafeBind(_, pattern, rhs) => {
            rebase_pattern(pattern, base, offset);
            rebase_resolved_node(rhs, base, offset);
        }
        Resolved::BinOp(_, _, left, right)
        | Resolved::Pipe(_, left, right)
        | Resolved::ContextMap(_, left, right)
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
        | Resolved::Semi(_, inner) => {
            rebase_resolved_node(inner, base, offset);
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
        Resolved::RecoverKind(_, value, id, handler) => {
            rebase_resolved_node(value, base, offset);
            rebase_resolved_id(id, base, offset);
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
            for (_, expr) in fields {
                rebase_resolved_node(expr, base, offset);
            }
        }
        Resolved::ConstructorCall(_, id, args) => {
            rebase_resolved_id(id, base, offset);
            for arg in args {
                rebase_record_arg(arg, base, offset);
            }
        }
        Resolved::StructDef(_, id, fields) | Resolved::RecordDef(_, id, fields) => {
            rebase_resolved_id(id, base, offset);
            rebase_fields(fields, base, offset);
        }
        Resolved::DeferrorDef(_, id, fields, show_expr) => {
            rebase_resolved_id(id, base, offset);
            rebase_fields(fields, base, offset);
            rebase_resolved_node(show_expr, base, offset);
        }
        Resolved::EnumDef(_, id, type_params, variants) => {
            rebase_resolved_id(id, base, offset);
            rebase_type_params(type_params, base, offset);
            for variant in variants {
                rebase_resolved_id(&mut variant.id, base, offset);
            }
        }
        Resolved::Def(_, id, type_params, params, _, body, _) => {
            rebase_resolved_id(id, base, offset);
            rebase_type_params(type_params, base, offset);
            rebase_fun_params(params, base, offset);
            rebase_resolved_node(body, base, offset);
        }
        Resolved::ExtractorDef(_, id, type_params, param, _, body, _) => {
            rebase_resolved_id(id, base, offset);
            rebase_type_params(type_params, base, offset);
            rebase_extractor_param(param, base, offset);
            rebase_resolved_node(body, base, offset);
        }
        Resolved::TraitDef(_, id, type_params, methods, _) => {
            rebase_resolved_id(id, base, offset);
            rebase_type_params(type_params, base, offset);
            for method in methods {
                rebase_resolved_id(&mut method.id, base, offset);
                rebase_type_params(&mut method.type_params, base, offset);
                rebase_fun_params(&mut method.params, base, offset);
            }
        }
        Resolved::TraitImplDef(_, id, _, _, methods) => {
            rebase_resolved_id(id, base, offset);
            for method in methods {
                rebase_resolved_id(&mut method.function_id, base, offset);
                rebase_type_params(&mut method.type_params, base, offset);
                rebase_fun_params(&mut method.params, base, offset);
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
        ResolvedPattern::Var(id) | ResolvedPattern::Annotated(id, _) => {
            rebase_resolved_id(id, base, offset);
        }
        ResolvedPattern::Wildcard(_)
        | ResolvedPattern::ListNil(_)
        | ResolvedPattern::IntLit(..)
        | ResolvedPattern::StrLit(..)
        | ResolvedPattern::BoolLit(..) => {}
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

struct Resolver {
    scope: Scope,
    /// Fresh IDs reserved in predeclaration order for each top-level declaration name.
    predeclared_ids: HashMap<String, VecDeque<u32>>,
    declaration_uids: HashMap<String, u32>,
    declaration_uid_kinds: HashMap<u32, DeclarationKind>,
    current_module_path: Option<String>,
    current_stage_impl_targets: Option<HashMap<String, declarations::ImplTargetResolution>>,
    allow_top_level_shadowing: bool,
}
