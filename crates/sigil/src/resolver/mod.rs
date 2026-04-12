use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use sindr::builtin::{builtin_uid, BUILTIN_METAS};
use spire::ast::{
    Ast, AstPattern, AstTy, BinOp, ClosureParam, DeclAttrs, ExtractorParam, FunParam, Lit,
    RecordLitArg, Span,
};

use crate::error::ResolveError;
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
    assign_declaration_uids, declaration_uid_kind_map, trait_method_qualified_name,
};
use self::imports::{build_global_scope, build_module_scope};

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
    let declaration_uids = assign_declaration_uids(declaration_index);
    let declaration_uid_kinds = declaration_uid_kind_map(declaration_index, &declaration_uids);
    let global_scope = build_global_scope(declaration_index, &declaration_uids);
    let mut resolved = Vec::new();

    for (stage_index, stage) in module_stages.iter().enumerate() {
        for module in stage {
            let scope = build_module_scope(
                &global_scope,
                declaration_index,
                &declaration_uids,
                &declaration_uid_kinds,
                &module.ast,
                Some(module.module_path.as_str()),
                stage_index,
            )?;
            let mut resolver = Resolver::with_scope(scope);
            resolver.current_module_path = Some(module.module_path.clone());
            resolver.declaration_uids = declaration_uids.clone();
            resolver.declaration_uid_kinds = declaration_uid_kinds.clone();
            resolver.allow_top_level_shadowing = true;
            resolved.extend(resolver.resolve_program(module.ast.clone())?);
        }
    }

    let user_scope = build_module_scope(
        &global_scope,
        declaration_index,
        &declaration_uids,
        &declaration_uid_kinds,
        &user_ast,
        user_module_path.as_deref(),
        module_stages.len(),
    )?;
    let mut user_resolver = Resolver::with_scope(user_scope);
    user_resolver.declaration_uids = declaration_uids;
    user_resolver.declaration_uid_kinds = declaration_uid_kinds;
    user_resolver.current_module_path = user_module_path;
    user_resolver.allow_top_level_shadowing = true;
    resolved.extend(user_resolver.resolve_program(user_ast)?);
    Ok(resolved)
}

pub fn build_scope_for_module(
    module_stages: &[Vec<StagedModuleAst>],
    current_module_path: Option<&str>,
    current_stage_index: usize,
) -> Result<Scope, ResolveError> {
    let declaration_index = precollect_declaration_index(module_stages)?;
    let declaration_uids = assign_declaration_uids(&declaration_index);
    let declaration_uid_kinds = declaration_uid_kind_map(&declaration_index, &declaration_uids);
    let global_scope = build_global_scope(&declaration_index, &declaration_uids);
    build_module_scope(
        &global_scope,
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
    allow_top_level_shadowing: bool,
}
