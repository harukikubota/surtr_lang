#![allow(unused_imports)]

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use sindr::builtin::{builtin_uid, BUILTIN_METAS};
use spire::ast::{Ast, AstPattern, BinOp, ClosureParam, FunParam, Lit, RecordLitArg, Span};

use crate::error::ResolveError;
use crate::resolved::*;
use crate::scope::Scope;

const AUTO_IMPORT_MODULES: &[&str] = &["Bootstrap", "Kernel"];

fn initialize_scope() -> Scope {
    let mut scope = Scope::new();
    let dummy = Span { start: 0, end: 0 };
    scope.define("Ok", dummy.clone());
    scope.define("Err", dummy);
    for meta in BUILTIN_METAS {
        scope.define_with_id(meta.name, builtin_uid(meta.builtin_id));
    }
    scope
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
    let declaration_uids = assign_declaration_uids(declaration_index);
    let global_scope = build_global_scope(declaration_index, &declaration_uids);
    let mut resolved = Vec::new();

    for (stage_index, stage) in module_stages.iter().enumerate() {
        for module in stage {
            let scope = build_module_scope(
                &global_scope,
                declaration_index,
                &declaration_uids,
                &module.ast,
                Some(module.module_path.as_str()),
                stage_index,
            )?;
            let mut resolver = Resolver::with_scope(scope);
            resolver.current_module_path = Some(module.module_path.clone());
            resolver.declaration_uids = declaration_uids.clone();
            resolver.allow_top_level_shadowing = true;
            resolved.extend(resolver.resolve_program(module.ast.clone())?);
        }
    }

    let user_scope = build_module_scope(
        &global_scope,
        declaration_index,
        &declaration_uids,
        &user_ast,
        user_module_path.as_deref(),
        module_stages.len(),
    )?;
    let mut user_resolver = Resolver::with_scope(user_scope);
    user_resolver.declaration_uids = declaration_uids;
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
    let global_scope = build_global_scope(&declaration_index, &declaration_uids);
    build_module_scope(
        &global_scope,
        &declaration_index,
        &declaration_uids,
        &[],
        current_module_path,
        current_stage_index,
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct StagedModuleAst {
    pub module_path: String,
    pub ast: Vec<Ast>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationKind {
    Def,
    Struct,
    Record,
    Deferror,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeclarationEntry {
    pub module_path: String,
    pub name: String,
    pub fq_name: String,
    pub kind: DeclarationKind,
    pub stage_index: usize,
}

pub type DeclarationIndex = BTreeMap<String, DeclarationEntry>;

fn assign_declaration_uids(index: &DeclarationIndex) -> HashMap<String, u32> {
    let mut scope = initialize_scope();
    let mut declaration_uids = HashMap::with_capacity(index.len());
    for fq_name in index.keys() {
        declaration_uids.insert(fq_name.clone(), scope.reserve_id());
    }
    declaration_uids
}

fn build_global_scope(index: &DeclarationIndex, declaration_uids: &HashMap<String, u32>) -> Scope {
    let mut scope = initialize_scope();
    for fq_name in index.keys() {
        if let Some(uid) = declaration_uids.get(fq_name) {
            scope.define_with_id(fq_name, *uid);
        }
    }
    scope
}

fn build_module_scope(
    global_scope: &Scope,
    declaration_index: &DeclarationIndex,
    declaration_uids: &HashMap<String, u32>,
    stmts: &[Ast],
    current_module_path: Option<&str>,
    current_stage_index: usize,
) -> Result<Scope, ResolveError> {
    let mut scope = global_scope.clone();
    let mut import_state = ImportState::default();

    for stmt in stmts {
        if let Ast::Import(span, path, spec) = stmt {
            apply_import_to_scope(
                &mut scope,
                declaration_index,
                declaration_uids,
                current_stage_index,
                path,
                spec,
                &mut import_state,
                span.clone(),
            )?;
        }
    }

    for auto_import in AUTO_IMPORT_MODULES {
        if current_module_path == Some(*auto_import) {
            continue;
        }
        import_module_into_scope(
            &mut scope,
            declaration_index,
            declaration_uids,
            auto_import,
            current_stage_index,
            true,
            &mut import_state,
            Span { start: 0, end: 0 },
        )?;
    }

    if let Some(module_path) = current_module_path {
        for entry in declaration_index.values() {
            if entry.module_path == module_path {
                if let Some(uid) = declaration_uids.get(&entry.fq_name) {
                    scope.define_with_id(&entry.name, *uid);
                }
            }
        }
    }

    Ok(scope)
}

fn apply_import_to_scope(
    scope: &mut Scope,
    declaration_index: &DeclarationIndex,
    declaration_uids: &HashMap<String, u32>,
    current_stage_index: usize,
    path: &spire::ast::AstPath,
    spec: &spire::ast::ImportSpec,
    import_state: &mut ImportState,
    span: Span,
) -> Result<(), ResolveError> {
    let module_name = path.segments.join("::");
    if AUTO_IMPORT_MODULES
        .iter()
        .any(|auto| auto == &module_name.as_str())
    {
        return Err(ResolveError {
            message: format!(
                "Duplicate import: `{}` is auto-imported and cannot be explicitly imported",
                module_name
            ),
            span,
        });
    }
    match spec {
        spire::ast::ImportSpec::All => import_module_into_scope(
            scope,
            declaration_index,
            declaration_uids,
            &module_name,
            current_stage_index,
            false,
            import_state,
            span,
        ),
        spire::ast::ImportSpec::Single(name) => import_single_into_scope(
            scope,
            declaration_index,
            declaration_uids,
            &module_name,
            name,
            current_stage_index,
            import_state,
            span,
        ),
        spire::ast::ImportSpec::List(names) => {
            for name in names {
                import_single_into_scope(
                    scope,
                    declaration_index,
                    declaration_uids,
                    &module_name,
                    name,
                    current_stage_index,
                    import_state,
                    span.clone(),
                )?;
            }
            Ok(())
        }
    }
}

fn import_module_into_scope(
    scope: &mut Scope,
    declaration_index: &DeclarationIndex,
    declaration_uids: &HashMap<String, u32>,
    module_name: &str,
    current_stage_index: usize,
    auto_import: bool,
    import_state: &mut ImportState,
    span: Span,
) -> Result<(), ResolveError> {
    if !auto_import {
        import_state.record_module_import(module_name, &span)?;
    }
    let mut imported_any = false;
    let mut blocked_by_stage = false;
    for entry in declaration_index.values() {
        if entry.module_path != module_name {
            continue;
        }
        if entry.stage_index >= current_stage_index {
            blocked_by_stage = true;
            continue;
        }
        let uid = declaration_uids[&entry.fq_name];
        bind_import_name(
            scope,
            &entry.name,
            uid,
            module_name,
            auto_import,
            span.clone(),
        )?;
        imported_any = true;
    }

    if imported_any {
        Ok(())
    } else if auto_import && AUTO_IMPORT_MODULES.contains(&module_name) {
        Ok(())
    } else if blocked_by_stage {
        Err(ResolveError {
            message: format!(
                "Import target `{}` is not available in the current stage",
                module_name
            ),
            span,
        })
    } else {
        Err(ResolveError {
            message: format!("Unknown module import: {}", module_name),
            span,
        })
    }
}

fn import_single_into_scope(
    scope: &mut Scope,
    declaration_index: &DeclarationIndex,
    declaration_uids: &HashMap<String, u32>,
    module_name: &str,
    name: &str,
    current_stage_index: usize,
    import_state: &mut ImportState,
    span: Span,
) -> Result<(), ResolveError> {
    import_state.record_member_import(module_name, name, &span)?;

    let fq_name = format!("{}::{}", module_name, name);
    let Some(entry) = declaration_index.get(&fq_name) else {
        let module_exists = declaration_index
            .values()
            .any(|entry| entry.module_path == module_name);
        return Err(ResolveError {
            message: if module_exists {
                format!("Unknown import member: {}", fq_name)
            } else {
                format!("Unknown module import: {}", module_name)
            },
            span,
        });
    };

    if entry.stage_index >= current_stage_index {
        return Err(ResolveError {
            message: format!(
                "Import target `{}` is not available in the current stage",
                fq_name
            ),
            span,
        });
    }

    bind_import_name(
        scope,
        &entry.name,
        declaration_uids[&entry.fq_name],
        module_name,
        false,
        span,
    )
}

fn bind_import_name(
    scope: &mut Scope,
    short_name: &str,
    uid: u32,
    module_name: &str,
    auto_import: bool,
    span: Span,
) -> Result<(), ResolveError> {
    if let Some(existing_uid) = scope.lookup(short_name) {
        if existing_uid == uid {
            return Ok(());
        }
        if auto_import {
            return Ok(());
        }
        return Err(ResolveError {
            message: format!(
                "Import conflict for `{}` from module `{}`",
                short_name, module_name
            ),
            span,
        });
    }

    scope.define_with_id(short_name, uid);
    Ok(())
}

#[derive(Debug, Clone, Default)]
struct ImportState {
    imported_modules: HashSet<String>,
    imported_members: HashSet<(String, String)>,
}

impl ImportState {
    fn record_module_import(&mut self, module_name: &str, span: &Span) -> Result<(), ResolveError> {
        if self.imported_modules.contains(module_name)
            || self
                .imported_members
                .iter()
                .any(|(module, _)| module == module_name)
        {
            return Err(ResolveError {
                message: format!("Duplicate import: {}", module_name),
                span: span.clone(),
            });
        }
        self.imported_modules.insert(module_name.to_string());
        Ok(())
    }

    fn record_member_import(
        &mut self,
        module_name: &str,
        name: &str,
        span: &Span,
    ) -> Result<(), ResolveError> {
        let member = (module_name.to_string(), name.to_string());
        if self.imported_modules.contains(module_name) || self.imported_members.contains(&member) {
            return Err(ResolveError {
                message: format!("Duplicate import: {}::{}", module_name, name),
                span: span.clone(),
            });
        }
        self.imported_members.insert(member);
        Ok(())
    }
}

/// Precollect global declaration index from staged module ASTs.
///
/// The index key is fully-qualified name `ModulePath::Name`.
/// Only declaration forms covered by Issue 6 are collected:
/// `def`, `defstruct`, `defrecord`, `deferror`.
pub fn precollect_declaration_index(
    module_stages: &[Vec<StagedModuleAst>],
) -> Result<DeclarationIndex, ResolveError> {
    let mut index = DeclarationIndex::new();

    for (stage_index, stage) in module_stages.iter().enumerate() {
        for module in stage {
            for stmt in &module.ast {
                let (span, name, kind) = match stmt {
                    Ast::Def(span, name, _, _, _) => (span, name.as_str(), DeclarationKind::Def),
                    Ast::StructDef(span, name, _) => (span, name.as_str(), DeclarationKind::Struct),
                    Ast::RecordDef(span, name, _) => (span, name.as_str(), DeclarationKind::Record),
                    Ast::DeferrorDef(span, name, _, _) => {
                        (span, name.as_str(), DeclarationKind::Deferror)
                    }
                    _ => continue,
                };

                let fq_name = if module.module_path.is_empty() {
                    name.to_string()
                } else {
                    format!("{}::{}", module.module_path, name)
                };

                if let Some(prev) = index.get(&fq_name) {
                    return Err(ResolveError {
                        message: format!(
                            "Duplicate fully-qualified declaration: {} (already declared in stage {} module {})",
                            fq_name, prev.stage_index, prev.module_path
                        ),
                        span: span.clone(),
                    });
                }

                index.insert(
                    fq_name.clone(),
                    DeclarationEntry {
                        module_path: module.module_path.clone(),
                        name: name.to_string(),
                        fq_name,
                        kind,
                        stage_index,
                    },
                );
            }
        }
    }

    Ok(index)
}

#[derive(Debug, Clone)]
pub struct SigilCheckpoint {
    scope: Scope,
}

#[derive(Debug, Clone)]
pub struct SigilSession {
    scope: Scope,
    current_module_path: Option<String>,
}

impl SigilSession {
    pub fn new() -> Self {
        Self {
            scope: initialize_scope(),
            current_module_path: None,
        }
    }

    pub fn with_module_path(current_module_path: Option<String>) -> Self {
        Self {
            scope: initialize_scope(),
            current_module_path,
        }
    }

    pub fn resolve(&mut self, ast: Vec<Ast>) -> Result<Vec<Resolved>, ResolveError> {
        let mut resolver = Resolver::with_scope(self.scope.clone());
        resolver.current_module_path = self.current_module_path.clone();
        let resolved = resolver.resolve_program(ast)?;
        self.scope = resolver.into_scope();
        Ok(resolved)
    }

    pub fn checkpoint(&self) -> SigilCheckpoint {
        SigilCheckpoint {
            scope: self.scope.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: SigilCheckpoint) {
        self.scope = checkpoint.scope;
    }

    pub fn replace_scope(&mut self, scope: Scope) {
        self.scope = scope;
    }
}

impl Default for SigilSession {
    fn default() -> Self {
        Self::new()
    }
}

struct Resolver {
    scope: Scope,
    /// Fresh IDs reserved in predeclaration order for each top-level declaration name.
    predeclared_ids: HashMap<String, VecDeque<u32>>,
    declaration_uids: HashMap<String, u32>,
    current_module_path: Option<String>,
    allow_top_level_shadowing: bool,
}

impl Resolver {
    fn new() -> Self {
        Self {
            scope: initialize_scope(),
            predeclared_ids: HashMap::new(),
            declaration_uids: HashMap::new(),
            current_module_path: None,
            allow_top_level_shadowing: false,
        }
    }

    fn with_scope(scope: Scope) -> Self {
        Self {
            scope,
            predeclared_ids: HashMap::new(),
            declaration_uids: HashMap::new(),
            current_module_path: None,
            allow_top_level_shadowing: false,
        }
    }

    fn into_scope(self) -> Scope {
        self.scope
    }

    fn qualify_current_declaration_name(&self, name: &str) -> String {
        match self.current_module_path.as_deref() {
            Some(module_path) if !module_path.is_empty() => format!("{}::{}", module_path, name),
            _ => name.to_string(),
        }
    }

    fn with_child_scope<T>(
        &mut self,
        f: impl FnOnce(&mut Resolver) -> Result<T, ResolveError>,
    ) -> Result<T, ResolveError> {
        let mut child = Resolver::with_scope(self.scope.clone());
        let out = f(&mut child)?;
        self.scope.advance_next_id_to(child.scope.next_id());
        Ok(out)
    }

    fn resolve_program(&mut self, stmts: Vec<Ast>) -> Result<Vec<Resolved>, ResolveError> {
        self.validate_auto_import_conflicts(&stmts)?;
        self.predeclare_functions(&stmts)?;
        let mut resolved = Vec::new();
        for stmt in stmts {
            if matches!(stmt, Ast::Import(_, _, _)) {
                // `import` declarations are consumed by resolver-side module/import handling.
                // Until full module resolution lands, they are intentionally no-op here.
                continue;
            }
            resolved.push(self.resolve_node(stmt)?);
        }
        self.predeclared_ids.clear();
        Ok(resolved)
    }

    fn validate_auto_import_conflicts(&self, stmts: &[Ast]) -> Result<(), ResolveError> {
        for stmt in stmts {
            match stmt {
                Ast::Import(span, path, _) => {
                    let module_name = path.segments.join("::");
                    if AUTO_IMPORT_MODULES
                        .iter()
                        .any(|auto| auto == &module_name.as_str())
                    {
                        return Err(ResolveError {
                            message: format!(
                                "Duplicate import: `{}` is auto-imported and cannot be explicitly imported",
                                module_name
                            ),
                            span: span.clone(),
                        });
                    }
                }
                Ast::Defmod(_, _, body) => self.validate_auto_import_conflicts(body)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn predeclare_functions(&mut self, stmts: &[Ast]) -> Result<(), ResolveError> {
        self.predeclared_ids.clear();
        // Language rule:
        // Top-level names must be unique per module / REPL session.
        // We intentionally enforce the same rule for file execution and REPL.
        let mut declared_in_batch = HashSet::new();
        for stmt in stmts {
            match stmt {
                Ast::Def(span, name, _, _, _) => {
                    if !declared_in_batch.insert(name.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: span.clone(),
                        });
                    }
                    if !self.allow_top_level_shadowing && self.scope.lookup(name).is_some() {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: span.clone(),
                        });
                    }
                    let uid = self
                        .declaration_uids
                        .get(&self.qualify_current_declaration_name(name))
                        .copied()
                        .unwrap_or_else(|| self.scope.reserve_id());
                    self.predeclared_ids
                        .entry(name.clone())
                        .or_default()
                        .push_back(uid);
                    // Keep the outer scope at the most recent declaration,
                    // so forward references resolve to the latest top-level definition.
                    self.scope.define_with_id(name, uid);
                }
                Ast::BuiltinDecl(_, name, _, _) => {
                    if !declared_in_batch.insert(name.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: stmt.span().clone(),
                        });
                    }
                    // Builtins are keyed by fixed IDs from builtin metadata.
                    // Re-declarations should keep that identity stable.
                    let uid = self
                        .scope
                        .lookup(name)
                        .unwrap_or_else(|| self.scope.reserve_id());
                    self.predeclared_ids
                        .entry(name.clone())
                        .or_default()
                        .push_back(uid);
                    self.scope.define_with_id(name, uid);
                }
                Ast::StructDef(span, name, _)
                | Ast::RecordDef(span, name, _)
                | Ast::DeferrorDef(span, name, _, _) => {
                    if !declared_in_batch.insert(name.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: span.clone(),
                        });
                    }
                    if !self.allow_top_level_shadowing && self.scope.lookup(name).is_some() {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: span.clone(),
                        });
                    }
                    let uid = self
                        .declaration_uids
                        .get(&self.qualify_current_declaration_name(name))
                        .copied()
                        .unwrap_or_else(|| self.scope.reserve_id());
                    self.predeclared_ids
                        .entry(name.clone())
                        .or_default()
                        .push_back(uid);
                    self.scope.define_with_id(name, uid);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn take_predeclared_id(&mut self, name: &str) -> Option<u32> {
        self.predeclared_ids
            .get_mut(name)
            .and_then(|ids| ids.pop_front())
    }

    fn resolve_node(&mut self, node: Ast) -> Result<Resolved, ResolveError> {
        match node {
            Ast::Lit(span, lit) => Ok(Resolved::Lit(span, lit)),

            Ast::Var(span, name) => {
                let uid = self.scope.lookup(&name).ok_or_else(|| ResolveError {
                    message: format!("Undefined variable: {}", name),
                    span: span.clone(),
                })?;
                Ok(Resolved::Var(
                    span.clone(),
                    ResolvedId {
                        name,
                        qualified_name: None,
                        unique_id: uid,
                        span,
                    },
                ))
            }
            Ast::Path(span, path) => {
                let name = path.segments.join("::");
                let uid = self.scope.lookup(&name).ok_or_else(|| ResolveError {
                    message: format!("Undefined variable: {}", name),
                    span: span.clone(),
                })?;
                Ok(Resolved::Var(
                    span.clone(),
                    ResolvedId {
                        qualified_name: Some(name.clone()),
                        name,
                        unique_id: uid,
                        span,
                    },
                ))
            }

            Ast::App(span, func, args) => {
                // Check for `if` special form
                if let Ast::Var(_, ref name) = *func {
                    if name == "if" {
                        return self.resolve_if(span, args, IfKind::If3);
                    }
                    if name == "if_then" {
                        return self.resolve_if(span, args, IfKind::IfThen2);
                    }
                }

                let resolved_func = self.resolve_node(*func)?;
                let resolved_args = args
                    .into_iter()
                    .map(|arg| match arg {
                        RecordLitArg::Positional(expr) => {
                            Ok(ResolvedRecordLitArg::Positional(self.resolve_node(expr)?))
                        }
                        RecordLitArg::Named(name, expr) => {
                            Ok(ResolvedRecordLitArg::Named(name, self.resolve_node(expr)?))
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::App(span, Box::new(resolved_func), resolved_args))
            }

            Ast::Bind(span, pat, rhs) => {
                // Resolve RHS first (before defining the new binding for shadowing)
                let resolved_rhs = self.resolve_node(*rhs)?;
                let resolved_pat = self.resolve_pattern(pat)?;
                Ok(Resolved::Bind(span, resolved_pat, Box::new(resolved_rhs)))
            }

            Ast::SafeBind(span, pat, rhs) => {
                // Resolve RHS first (before defining the new binding for shadowing)
                let resolved_rhs = self.resolve_node(*rhs)?;
                let resolved_pat = self.resolve_pattern(pat)?;
                Ok(Resolved::SafeBind(
                    span,
                    resolved_pat,
                    Box::new(resolved_rhs),
                ))
            }

            Ast::BinOp(span, op, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::BinOp(span, op, Box::new(l), Box::new(r)))
            }

            Ast::ListNil(span) => Ok(Resolved::ListNil(span)),

            Ast::ListCons(span, head, tail) => {
                let head = self.resolve_node(*head)?;
                let tail = self.resolve_node(*tail)?;
                Ok(Resolved::ListCons(span, Box::new(head), Box::new(tail)))
            }

            Ast::ListLiteral(span, elems) => {
                let resolved = elems
                    .into_iter()
                    .map(|e| self.resolve_node(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::ListLiteral(span, resolved))
            }

            Ast::InterpolatedStr(span, parts) => {
                let mut resolved_parts = Vec::new();
                for part in parts {
                    match part {
                        spire::ast::InterpolatedPart::Text(s) => {
                            resolved_parts.push(ResolvedInterpolatedPart::Text(s));
                        }
                        spire::ast::InterpolatedPart::Expr(expr) => {
                            let resolved_expr = self.resolve_node(*expr)?;
                            resolved_parts
                                .push(ResolvedInterpolatedPart::Expr(Box::new(resolved_expr)));
                        }
                    }
                }
                Ok(Resolved::InterpolatedStr(span, resolved_parts))
            }

            Ast::FieldAccess(span, expr, field) => {
                let resolved_expr = self.resolve_node(*expr)?;
                Ok(Resolved::FieldAccess(span, Box::new(resolved_expr), field))
            }

            Ast::Block(span, stmts) => {
                let resolved = self.with_child_scope(|child| {
                    stmts
                        .into_iter()
                        .map(|s| child.resolve_node(s))
                        .collect::<Result<Vec<_>, _>>()
                })?;
                Ok(Resolved::Block(span, resolved))
            }

            Ast::Semi(span, inner) => {
                let resolved = self.resolve_node(*inner)?;
                Ok(Resolved::Semi(span, Box::new(resolved)))
            }

            // Struct/Record/Deferror definitions — reuse predeclared IDs
            Ast::StructDef(span, name, fields) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    span: span.clone(),
                };
                let rfields = fields
                    .into_iter()
                    .map(|f| ResolvedField {
                        id: None,
                        name: f.name,
                        ty: f.ty,
                        span: f.span,
                    })
                    .collect();
                Ok(Resolved::StructDef(span, rid, rfields))
            }

            Ast::RecordDef(span, name, fields) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    span: span.clone(),
                };
                let rfields = fields
                    .into_iter()
                    .map(|f| ResolvedField {
                        id: None,
                        name: f.name,
                        ty: f.ty,
                        span: f.span,
                    })
                    .collect();
                Ok(Resolved::RecordDef(span, rid, rfields))
            }

            Ast::DeferrorDef(span, name, fields, show_expr) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    span: span.clone(),
                };
                let mut error_scope = self.scope.clone();
                let mut rfields = Vec::new();
                for f in fields {
                    let uid = error_scope.define(&f.name, f.span.clone());
                    rfields.push(ResolvedField {
                        id: Some(ResolvedId {
                            name: f.name.clone(),
                            qualified_name: None,
                            unique_id: uid,
                            span: f.span.clone(),
                        }),
                        name: f.name,
                        ty: f.ty,
                        span: f.span,
                    });
                }
                let mut show_resolver = Resolver::with_scope(error_scope);
                let resolved_show = show_resolver.resolve_node(*show_expr)?;
                self.scope.advance_next_id_to(show_resolver.scope.next_id());
                Ok(Resolved::DeferrorDef(
                    span,
                    rid,
                    rfields,
                    Box::new(resolved_show),
                ))
            }

            Ast::Def(span, name, params, ret_ty, body) => {
                let fun_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut body_scope = self.scope.clone();
                // Ensure self-recursion inside this definition binds to this declaration,
                // not to a newer same-name declaration predeclared later in the chunk.
                body_scope.define_with_id(&name, fun_uid);
                let mut body_resolver = Resolver::with_scope(body_scope);
                let resolved_params = params
                    .into_iter()
                    .map(|param| body_resolver.resolve_fun_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                let resolved_body = body_resolver.resolve_node(*body)?;

                self.scope.advance_next_id_to(body_resolver.scope.next_id());
                self.scope.define_with_id(&name, fun_uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: fun_uid,
                    span: span.clone(),
                };

                Ok(Resolved::Def(
                    span,
                    rid,
                    resolved_params,
                    ret_ty,
                    Box::new(resolved_body),
                ))
            }

            Ast::BuiltinDecl(span, name, params, ret_ty) => {
                if !BUILTIN_METAS.iter().any(|meta| meta.name == name) {
                    return Err(ResolveError {
                        message: format!("Unknown builtin declaration: {}", name),
                        span,
                    });
                }

                let builtin_uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                let mut decl_resolver = Resolver::with_scope(self.scope.clone());
                let resolved_params = params
                    .into_iter()
                    .map(|param| decl_resolver.resolve_fun_param(param))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                self.scope.advance_next_id_to(decl_resolver.scope.next_id());
                self.scope.define_with_id(&name, builtin_uid);
                let qualified_name = name.clone();
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: builtin_uid,
                    span: span.clone(),
                };
                Ok(Resolved::BuiltinDecl(span, rid, resolved_params, ret_ty))
            }
            Ast::Defmod(span, name, _) => Err(ResolveError {
                message: format!("Module resolution is not implemented yet: {}", name),
                span,
            }),
            Ast::Import(span, _, _) => Err(ResolveError {
                message: "Import resolution is not implemented yet".to_string(),
                span,
            }),

            Ast::Closure(span, params, body) => {
                let mut closure_scope = self.scope.clone();
                let mut resolved_params = Vec::new();
                for param in params {
                    let uid = closure_scope.define(&param.name, param.span.clone());
                    resolved_params.push(ResolvedClosureParam {
                        id: ResolvedId {
                            name: param.name,
                            qualified_name: None,
                            unique_id: uid,
                            span: param.span,
                        },
                    });
                }

                let mut body_resolver = Resolver::with_scope(closure_scope);
                let resolved_body = body_resolver.resolve_node(*body)?;
                self.scope.advance_next_id_to(body_resolver.scope.next_id());

                let captures = collect_captures(&resolved_body, &resolved_params);

                Ok(Resolved::Closure(
                    span,
                    resolved_params,
                    captures,
                    Box::new(resolved_body),
                ))
            }

            Ast::Capture(span, target, args) => {
                let resolved_target = self.resolve_node(*target)?;
                let resolved_args = args
                    .into_iter()
                    .map(|arg| self.resolve_node(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Resolved::Capture(
                    span,
                    Box::new(resolved_target),
                    resolved_args,
                ))
            }

            Ast::StructLit(span, type_name, field_vals) => {
                let uid = self.scope.lookup(&type_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined type: {}", type_name),
                    span: span.clone(),
                })?;
                let rid = ResolvedId {
                    name: type_name,
                    qualified_name: None,
                    unique_id: uid,
                    span: span.clone(),
                };
                let resolved_fields = field_vals
                    .into_iter()
                    .map(|(name, expr)| Ok((name, self.resolve_node(expr)?)))
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::StructLit(span, rid, resolved_fields))
            }

            Ast::ConstructorCall(span, type_name, args) => {
                let uid = self.scope.lookup(&type_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined type: {}", type_name),
                    span: span.clone(),
                })?;
                let rid = ResolvedId {
                    name: type_name,
                    qualified_name: None,
                    unique_id: uid,
                    span: span.clone(),
                };
                let resolved_args = args
                    .into_iter()
                    .map(|arg| match arg {
                        spire::ast::RecordLitArg::Positional(e) => {
                            Ok(ResolvedRecordLitArg::Positional(self.resolve_node(e)?))
                        }
                        spire::ast::RecordLitArg::Named(name, e) => {
                            Ok(ResolvedRecordLitArg::Named(name, self.resolve_node(e)?))
                        }
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::ConstructorCall(span, rid, resolved_args))
            }

            Ast::Match(span, scrutinee, arms) => {
                let resolved_scrut = self.resolve_node(*scrutinee)?;
                let resolved_arms = arms
                    .into_iter()
                    .map(|(pat, body)| {
                        let (rpat, body) = self.resolve_match_arm(pat, body)?;
                        Ok((rpat, body))
                    })
                    .collect::<Result<Vec<_>, ResolveError>>()?;
                Ok(Resolved::Match(
                    span,
                    Box::new(resolved_scrut),
                    resolved_arms,
                ))
            }
        }
    }

    fn resolve_fun_param(&mut self, param: FunParam) -> Result<ResolvedFunParam, ResolveError> {
        let uid = self.scope.define(&param.name, param.span.clone());
        Ok(ResolvedFunParam {
            id: ResolvedId {
                name: param.name,
                qualified_name: None,
                unique_id: uid,
                span: param.span,
            },
            ty: param.ty,
        })
    }

    fn resolve_if(
        &mut self,
        span: Span,
        args: Vec<RecordLitArg>,
        kind: IfKind,
    ) -> Result<Resolved, ResolveError> {
        let (expected_arity, callee_name) = match kind {
            IfKind::If3 => (3usize, "if"),
            IfKind::IfThen2 => (2usize, "if_then"),
        };
        if args.len() != expected_arity {
            return Err(ResolveError {
                message: format!(
                    "{} expects {} arguments, got {}",
                    callee_name,
                    expected_arity,
                    args.len()
                ),
                span,
            });
        }

        let mut positional = Vec::with_capacity(args.len());
        for arg in args {
            match arg {
                RecordLitArg::Positional(expr) => positional.push(expr),
                RecordLitArg::Named(name, _) => {
                    return Err(ResolveError {
                        message: format!(
                            "{} does not accept named argument '{}'",
                            callee_name, name
                        ),
                        span,
                    });
                }
            }
        }

        let mut iter = positional.into_iter();
        let cond = self.resolve_node(iter.next().expect("checked arg length"))?;
        let then = self.resolve_node(iter.next().expect("checked arg length"))?;
        let else_branch = match kind {
            IfKind::If3 => Some(Box::new(
                self.resolve_node(iter.next().expect("checked arg length"))?,
            )),
            IfKind::IfThen2 => None,
        };
        Ok(Resolved::If(
            span,
            Box::new(cond),
            Box::new(then),
            else_branch,
        ))
    }

    fn resolve_pattern(&mut self, pat: AstPattern) -> Result<ResolvedPattern, ResolveError> {
        let mut seen = HashMap::<String, Span>::new();
        self.resolve_pattern_inner(pat, &mut seen)
    }

    fn define_pattern_binding(
        &mut self,
        name: String,
        span: Span,
        seen: &mut HashMap<String, Span>,
    ) -> Result<ResolvedId, ResolveError> {
        if let Some(prev_span) = seen.get(&name) {
            return Err(ResolveError {
                message: format!("Duplicate binding in pattern: {}", name),
                span: Span {
                    start: prev_span.start,
                    end: span.end,
                },
            });
        }
        seen.insert(name.clone(), span.clone());
        let uid = self.scope.define(&name, span.clone());
        Ok(ResolvedId {
            name,
            qualified_name: None,
            unique_id: uid,
            span,
        })
    }

    fn resolve_pattern_inner(
        &mut self,
        pat: AstPattern,
        seen: &mut HashMap<String, Span>,
    ) -> Result<ResolvedPattern, ResolveError> {
        match pat {
            AstPattern::Var(span, name) => Ok(ResolvedPattern::Var(
                self.define_pattern_binding(name, span, seen)?,
            )),
            AstPattern::Annotated(span, name, ty) => Ok(ResolvedPattern::Annotated(
                self.define_pattern_binding(name, span, seen)?,
                ty,
            )),
            AstPattern::Wildcard(span) => Ok(ResolvedPattern::Wildcard(span)),
            AstPattern::ListNil(span) => Ok(ResolvedPattern::ListNil(span)),
            AstPattern::ListCons(_, head, tail) => Ok(ResolvedPattern::ListCons(
                Box::new(self.resolve_pattern_inner(*head, seen)?),
                Box::new(self.resolve_pattern_inner(*tail, seen)?),
            )),
            AstPattern::IntLit(span, n) => Ok(ResolvedPattern::IntLit(span, n)),
            AstPattern::StrLit(span, s) => Ok(ResolvedPattern::StrLit(span, s)),
            AstPattern::BoolLit(span, b) => Ok(ResolvedPattern::BoolLit(span, b)),
            AstPattern::Constructor(span, ctor_name, inner) => {
                let ctor_uid = self.scope.lookup(&ctor_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined constructor: {}", ctor_name),
                    span: span.clone(),
                })?;
                Ok(ResolvedPattern::Constructor(
                    ResolvedId {
                        name: ctor_name,
                        qualified_name: None,
                        unique_id: ctor_uid,
                        span,
                    },
                    Box::new(self.resolve_pattern_inner(*inner, seen)?),
                ))
            }
            AstPattern::As(span, inner, alias, alias_ty) => {
                let resolved_inner = self.resolve_pattern_inner(*inner, seen)?;
                let alias_id = self.define_pattern_binding(alias, span, seen)?;
                Ok(ResolvedPattern::As(
                    Box::new(resolved_inner),
                    alias_id,
                    alias_ty,
                ))
            }
        }
    }

    fn resolve_match_arm(
        &mut self,
        pat: spire::ast::AstMatchPattern,
        body: Ast,
    ) -> Result<(ResolvedMatchPattern, Resolved), ResolveError> {
        self.with_child_scope(|child| match pat {
            spire::ast::AstMatchPattern::Binding(span, name) => {
                let uid = child.scope.define(&name, span.clone());
                let resolved_body = child.resolve_node(body)?;
                Ok((
                    ResolvedMatchPattern::Binding(ResolvedId {
                        name,
                        qualified_name: None,
                        unique_id: uid,
                        span,
                    }),
                    resolved_body,
                ))
            }
            spire::ast::AstMatchPattern::Wildcard(span) => {
                let resolved_body = child.resolve_node(body)?;
                Ok((ResolvedMatchPattern::Wildcard(span), resolved_body))
            }
            spire::ast::AstMatchPattern::BoolLit(span, b) => {
                let resolved_body = child.resolve_node(body)?;
                Ok((ResolvedMatchPattern::BoolLit(span, b), resolved_body))
            }
            spire::ast::AstMatchPattern::IntLit(span, n) => {
                let resolved_body = child.resolve_node(body)?;
                Ok((ResolvedMatchPattern::IntLit(span, n), resolved_body))
            }
            spire::ast::AstMatchPattern::StrLit(span, s) => {
                let resolved_body = child.resolve_node(body)?;
                Ok((ResolvedMatchPattern::StrLit(span, s), resolved_body))
            }
            spire::ast::AstMatchPattern::Constructor(span, ctor_name, inner_name) => {
                let ctor_uid = child.scope.lookup(&ctor_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined constructor: {}", ctor_name),
                    span: span.clone(),
                })?;
                let ctor_id = ResolvedId {
                    name: ctor_name,
                    qualified_name: None,
                    unique_id: ctor_uid,
                    span: span.clone(),
                };
                let inner_id = match inner_name {
                    Some(name) => {
                        let uid = child.scope.define(&name, span.clone());
                        Some(ResolvedId {
                            name,
                            qualified_name: None,
                            unique_id: uid,
                            span: span.clone(),
                        })
                    }
                    None => None,
                };
                let resolved_body = child.resolve_node(body)?;
                Ok((
                    ResolvedMatchPattern::Constructor(span, ctor_id, inner_id),
                    resolved_body,
                ))
            }
            spire::ast::AstMatchPattern::ListNil(span) => {
                let resolved_body = child.resolve_node(body)?;
                Ok((ResolvedMatchPattern::ListNil(span), resolved_body))
            }
            spire::ast::AstMatchPattern::ListCons(_, head, tail) => {
                let head = child.resolve_match_pattern_node(*head)?;
                let tail = child.resolve_match_pattern_node(*tail)?;
                let resolved_body = child.resolve_node(body)?;
                Ok((
                    ResolvedMatchPattern::ListCons(Box::new(head), Box::new(tail)),
                    resolved_body,
                ))
            }
        })
    }

    fn resolve_match_pattern_node(
        &mut self,
        pat: spire::ast::AstMatchPattern,
    ) -> Result<ResolvedMatchPattern, ResolveError> {
        match pat {
            spire::ast::AstMatchPattern::Binding(span, name) => {
                let uid = self.scope.define(&name, span.clone());
                Ok(ResolvedMatchPattern::Binding(ResolvedId {
                    name,
                    qualified_name: None,
                    unique_id: uid,
                    span,
                }))
            }
            spire::ast::AstMatchPattern::Wildcard(span) => Ok(ResolvedMatchPattern::Wildcard(span)),
            spire::ast::AstMatchPattern::BoolLit(span, b) => {
                Ok(ResolvedMatchPattern::BoolLit(span, b))
            }
            spire::ast::AstMatchPattern::IntLit(span, n) => {
                Ok(ResolvedMatchPattern::IntLit(span, n))
            }
            spire::ast::AstMatchPattern::StrLit(span, s) => {
                Ok(ResolvedMatchPattern::StrLit(span, s))
            }
            spire::ast::AstMatchPattern::Constructor(span, ctor_name, inner_name) => {
                let ctor_uid = self.scope.lookup(&ctor_name).ok_or_else(|| ResolveError {
                    message: format!("Undefined constructor: {}", ctor_name),
                    span: span.clone(),
                })?;
                let ctor_id = ResolvedId {
                    name: ctor_name,
                    qualified_name: None,
                    unique_id: ctor_uid,
                    span: span.clone(),
                };
                let inner_id = match inner_name {
                    Some(name) => {
                        let uid = self.scope.define(&name, span.clone());
                        Some(ResolvedId {
                            name,
                            qualified_name: None,
                            unique_id: uid,
                            span,
                        })
                    }
                    None => None,
                };
                Ok(ResolvedMatchPattern::Constructor(
                    ctor_id.span.clone(),
                    ctor_id,
                    inner_id,
                ))
            }
            spire::ast::AstMatchPattern::ListNil(span) => Ok(ResolvedMatchPattern::ListNil(span)),
            spire::ast::AstMatchPattern::ListCons(_, head, tail) => {
                Ok(ResolvedMatchPattern::ListCons(
                    Box::new(self.resolve_match_pattern_node(*head)?),
                    Box::new(self.resolve_match_pattern_node(*tail)?),
                ))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IfKind {
    If3,
    IfThen2,
}

fn collect_captures(body: &Resolved, params: &[ResolvedClosureParam]) -> Vec<ResolvedId> {
    let mut bound = HashSet::new();
    for param in params {
        bound.insert(param.id.unique_id);
    }
    let mut free = Vec::new();
    collect_captures_inner(body, &mut bound, &mut free);
    free
}

fn collect_captures_inner(node: &Resolved, bound: &mut HashSet<u32>, free: &mut Vec<ResolvedId>) {
    match node {
        Resolved::Lit(_, _) => {}
        Resolved::Var(_, id) => {
            if !bound.contains(&id.unique_id)
                && !free.iter().any(|seen| seen.unique_id == id.unique_id)
            {
                free.push(id.clone());
            }
        }
        Resolved::App(_, func, args) => {
            collect_captures_inner(func, bound, free);
            for arg in args {
                match arg {
                    ResolvedRecordLitArg::Positional(expr)
                    | ResolvedRecordLitArg::Named(_, expr) => {
                        collect_captures_inner(expr, bound, free);
                    }
                }
            }
        }
        Resolved::Block(_, stmts) => {
            let mut local_bound = bound.clone();
            for stmt in stmts {
                collect_captures_inner(stmt, &mut local_bound, free);
                match stmt {
                    Resolved::Bind(_, pat, _) | Resolved::SafeBind(_, pat, _) => {
                        collect_bind_pattern_bindings(pat, &mut local_bound);
                    }
                    Resolved::Def(_, id, params, _, _) => {
                        local_bound.insert(id.unique_id);
                        for param in params {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    Resolved::BuiltinDecl(_, id, params, _) => {
                        local_bound.insert(id.unique_id);
                        for param in params {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    Resolved::Closure(_, params, _, _) => {
                        for param in params {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    _ => {}
                }
            }
        }
        Resolved::Bind(_, pat, rhs) => {
            collect_captures_inner(rhs, bound, free);
            collect_bind_pattern_bindings(pat, bound);
        }
        Resolved::SafeBind(_, pat, rhs) => {
            collect_captures_inner(rhs, bound, free);
            collect_bind_pattern_bindings(pat, bound);
        }
        Resolved::BinOp(_, _, left, right) => {
            collect_captures_inner(left, bound, free);
            collect_captures_inner(right, bound, free);
        }
        Resolved::ListNil(_) => {}
        Resolved::ListCons(_, head, tail) => {
            collect_captures_inner(head, bound, free);
            collect_captures_inner(tail, bound, free);
        }
        Resolved::ListLiteral(_, elems) => {
            for elem in elems {
                collect_captures_inner(elem, bound, free);
            }
        }
        Resolved::InterpolatedStr(_, parts) => {
            for part in parts {
                if let ResolvedInterpolatedPart::Expr(expr) = part {
                    collect_captures_inner(expr, bound, free);
                }
            }
        }
        Resolved::If(_, cond, then, else_opt) => {
            collect_captures_inner(cond, bound, free);
            collect_captures_inner(then, bound, free);
            if let Some(else_branch) = else_opt {
                collect_captures_inner(else_branch, bound, free);
            }
        }
        Resolved::Match(_, scrutinee, arms) => {
            collect_captures_inner(scrutinee, bound, free);
            for (pat, body) in arms {
                let mut arm_bound = bound.clone();
                collect_match_pattern_bindings(pat, &mut arm_bound);
                collect_captures_inner(body, &mut arm_bound, free);
            }
        }
        Resolved::FieldAccess(_, expr, _) => collect_captures_inner(expr, bound, free),
        Resolved::StructLit(_, _, fields) => {
            for (_, expr) in fields {
                collect_captures_inner(expr, bound, free);
            }
        }
        Resolved::ConstructorCall(_, _, args) => {
            for arg in args {
                match arg {
                    ResolvedRecordLitArg::Positional(expr) => {
                        collect_captures_inner(expr, bound, free)
                    }
                    ResolvedRecordLitArg::Named(_, expr) => {
                        collect_captures_inner(expr, bound, free)
                    }
                }
            }
        }
        Resolved::StructDef(_, _, _)
        | Resolved::RecordDef(_, _, _)
        | Resolved::DeferrorDef(_, _, _, _)
        | Resolved::BuiltinDecl(_, _, _, _) => {}
        Resolved::Def(_, id, params, _, body) => {
            let mut fun_bound = bound.clone();
            fun_bound.insert(id.unique_id);
            for param in params {
                fun_bound.insert(param.id.unique_id);
            }
            collect_captures_inner(body, &mut fun_bound, free);
        }
        Resolved::Closure(_, _, captures, _) => {
            for cap in captures {
                if !bound.contains(&cap.unique_id)
                    && !free.iter().any(|seen| seen.unique_id == cap.unique_id)
                {
                    free.push(cap.clone());
                }
            }
        }
        Resolved::Capture(_, target, args) => {
            collect_captures_inner(target, bound, free);
            for arg in args {
                collect_captures_inner(arg, bound, free);
            }
        }
        Resolved::Semi(_, inner) => collect_captures_inner(inner, bound, free),
    }
}

fn collect_match_pattern_bindings(pat: &ResolvedMatchPattern, bound: &mut HashSet<u32>) {
    match pat {
        ResolvedMatchPattern::Binding(id) => {
            bound.insert(id.unique_id);
        }
        ResolvedMatchPattern::Constructor(_, _, Some(inner)) => {
            bound.insert(inner.unique_id);
        }
        ResolvedMatchPattern::ListCons(head, tail) => {
            collect_match_pattern_bindings(head, bound);
            collect_match_pattern_bindings(tail, bound);
        }
        _ => {}
    }
}

fn collect_bind_pattern_bindings(pat: &ResolvedPattern, bound: &mut HashSet<u32>) {
    match pat {
        ResolvedPattern::Var(id) | ResolvedPattern::Annotated(id, _) => {
            bound.insert(id.unique_id);
        }
        ResolvedPattern::Constructor(_, inner) => {
            collect_bind_pattern_bindings(inner, bound);
        }
        ResolvedPattern::As(inner, id, _) => {
            bound.insert(id.unique_id);
            collect_bind_pattern_bindings(inner, bound);
        }
        ResolvedPattern::ListCons(head, tail) => {
            collect_bind_pattern_bindings(head, bound);
            collect_bind_pattern_bindings(tail, bound);
        }
        ResolvedPattern::Wildcard(_)
        | ResolvedPattern::ListNil(_)
        | ResolvedPattern::IntLit(_, _)
        | ResolvedPattern::StrLit(_, _)
        | ResolvedPattern::BoolLit(_, _) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_module_ast(src: &str, module_path: &str) -> Vec<Ast> {
        let _ = module_path;
        spire::parse(src).expect("module source should parse")
    }

    fn parse_and_resolve(src: &str) -> Result<Vec<Resolved>, ResolveError> {
        let ast = spire::parse(src).expect("parse failed");
        resolve(ast)
    }

    fn resolve_user_with_modules(
        user_src: &str,
        module_stages: &[Vec<StagedModuleAst>],
    ) -> Result<Vec<Resolved>, ResolveError> {
        let user_ast = spire::parse(user_src).expect("user script should parse");
        let mut full_stages = vec![vec![StagedModuleAst {
            module_path: "Bootstrap".to_string(),
            ast: parse_module_ast(r#"deferror NoneError { "none" }"#, "Bootstrap"),
        }]];
        full_stages.extend(module_stages.iter().cloned());
        let declaration_index =
            precollect_declaration_index(&full_stages).expect("precollect should succeed");
        resolve_staged_program(
            &full_stages,
            user_ast,
            &declaration_index,
            Some("__Script::fixture".to_string()),
        )
    }

    #[test]
    fn test_precollect_declaration_index_succeeds_without_body_resolution() {
        let module_stages = vec![vec![StagedModuleAst {
            module_path: "Bootstrap".to_string(),
            ast: parse_module_ast(
                r#"def to_int(x: String) -> Int { unknown_name }
defrecord Pair(left: Int, right: Int)
deferror Oops(reason: String) { reason }"#,
                "Bootstrap",
            ),
        }]];

        let index =
            precollect_declaration_index(&module_stages).expect("precollect should succeed");
        assert!(index.contains_key("Bootstrap::to_int"));
        assert!(index.contains_key("Bootstrap::Pair"));
        assert!(index.contains_key("Bootstrap::Oops"));
    }

    #[test]
    fn test_precollect_declaration_index_rejects_duplicate_fully_qualified_name() {
        let module_stages = vec![vec![
            StagedModuleAst {
                module_path: "Std::Math".to_string(),
                ast: parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Std::Math"),
            },
            StagedModuleAst {
                module_path: "Std::Math".to_string(),
                ast: parse_module_ast(r#"def add(a: Int, b: Int) -> Int { a + b }"#, "Std::Math"),
            },
        ]];

        let err = precollect_declaration_index(&module_stages)
            .expect_err("duplicate fully-qualified declaration must fail");
        assert!(err
            .message
            .contains("Duplicate fully-qualified declaration: Std::Math::add"));
    }

    #[test]
    fn test_precollect_declaration_index_is_deterministic_when_stage_input_order_changes() {
        let mod_a = StagedModuleAst {
            module_path: "Std::A".to_string(),
            ast: parse_module_ast(r#"def same(x: Int) -> Int { x }"#, "Std::A"),
        };
        let mod_b = StagedModuleAst {
            module_path: "Std::B".to_string(),
            ast: parse_module_ast(r#"def same(x: Int) -> Int { x }"#, "Std::B"),
        };

        let index_first =
            precollect_declaration_index(&[vec![mod_a.clone(), mod_b.clone()]]).unwrap();
        let index_swapped =
            precollect_declaration_index(&[vec![mod_b.clone(), mod_a.clone()]]).unwrap();

        assert_eq!(index_first, index_swapped);
        assert!(index_first.contains_key("Std::A::same"));
        assert!(index_first.contains_key("Std::B::same"));
    }

    #[test]
    fn test_precollect_declaration_index_tracks_bootstrap_std_user_stage_split() {
        let module_stages = vec![
            vec![StagedModuleAst {
                module_path: "Bootstrap".to_string(),
                ast: parse_module_ast(r#"deferror NoneError { "none" }"#, "Bootstrap"),
            }],
            vec![StagedModuleAst {
                module_path: "Std::Math".to_string(),
                ast: parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Std::Math"),
            }],
            vec![StagedModuleAst {
                module_path: "User::Main".to_string(),
                ast: parse_module_ast(r#"def main() -> Int { 1 }"#, "User::Main"),
            }],
        ];

        let index =
            precollect_declaration_index(&module_stages).expect("precollect should succeed");
        assert_eq!(index["Bootstrap::NoneError"].stage_index, 0);
        assert_eq!(index["Std::Math::add"].stage_index, 1);
        assert_eq!(index["User::Main::main"].stage_index, 2);
    }

    #[test]
    fn test_simple_bind() {
        let resolved = parse_and_resolve("x = 10").unwrap();
        assert_eq!(resolved.len(), 1);
        match &resolved[0] {
            Resolved::Bind(_, ResolvedPattern::Var(id), _) => {
                assert_eq!(id.name, "x");
            }
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_builtin_ref() {
        let resolved = parse_and_resolve("print(to_string(42))").unwrap();
        match &resolved[0] {
            Resolved::App(_, func, _) => match func.as_ref() {
                Resolved::Var(_, id) => assert_eq!(id.name, "print"),
                _ => panic!("Expected Var for print"),
            },
            _ => panic!("Expected App"),
        }
    }

    #[test]
    fn test_builtin_decl_resolution() {
        let resolved = parse_and_resolve("@@builtin def print(a: String) -> Unit").unwrap();
        match &resolved[0] {
            Resolved::BuiltinDecl(_, id, params, ret_ty) => {
                assert_eq!(id.name, "print");
                assert_eq!(id.unique_id, 2); // 0=Ok, 1=Err, 2=print
                assert_eq!(params.len(), 1);
                assert!(matches!(
                    ret_ty,
                    Some(spire::ast::AstTy::Named(_, ty)) if ty == "Unit"
                ));
            }
            _ => panic!("Expected BuiltinDecl"),
        }
    }

    #[test]
    fn test_named_args_resolution() {
        let resolved = parse_and_resolve(
            r#"def add(x: Int, y: Int) -> Int { x + y }
result = add(y: 2, x: 1)"#,
        )
        .unwrap();
        match &resolved[1] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::App(_, _, args) => {
                    assert!(matches!(&args[0], ResolvedRecordLitArg::Named(n, _) if n == "y"));
                    assert!(matches!(&args[1], ResolvedRecordLitArg::Named(n, _) if n == "x"));
                }
                _ => panic!("Expected App"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_function_def_resolution() {
        let resolved = parse_and_resolve(
            r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(1))"#,
        )
        .unwrap();
        match &resolved[0] {
            Resolved::Def(_, id, params, ret_ty, body) => {
                assert_eq!(id.name, "add");
                assert_eq!(params.len(), 2);
                assert!(matches!(ret_ty, Some(spire::ast::AstTy::Named(_, ty)) if ty == "Int"));
                assert!(
                    matches!(body.as_ref(), Resolved::Block(_, stmts) if matches!(stmts.as_slice(), [Resolved::BinOp(_, _, _, _)]))
                );
            }
            _ => panic!("Expected Def"),
        }
        match &resolved[1] {
            Resolved::App(_, func, _) => match func.as_ref() {
                Resolved::Var(_, id) => assert_eq!(id.name, "print"),
                _ => panic!("Expected Var"),
            },
            _ => panic!("Expected App"),
        }
    }

    #[test]
    fn test_undefined_var() {
        let result = parse_and_resolve("print(unknown_var)");
        assert!(result.is_err());
    }

    #[test]
    fn test_if_conversion() {
        let resolved = parse_and_resolve("x = if(True, 1, 2)").unwrap();
        match &resolved[0] {
            Resolved::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Resolved::If(_, _, _, Some(_))));
            }
            _ => panic!("Expected Bind with If"),
        }
    }

    #[test]
    fn test_if_then_conversion() {
        let resolved = parse_and_resolve("x = if_then(True, 1)").unwrap();
        match &resolved[0] {
            Resolved::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Resolved::If(_, _, _, None)));
            }
            _ => panic!("Expected Bind with If"),
        }
    }

    #[test]
    fn test_duplicate_top_level_def_is_error() {
        let result = parse_and_resolve("def f() -> Int { 1 }\ndef f() -> Int { 2 }");
        let err = result.expect_err("duplicate def must fail");
        assert!(err.message.contains("Duplicate top-level definition: f"));
    }

    #[test]
    fn test_forward_reference_to_function_resolves_to_same_unique_id() {
        let resolved = parse_and_resolve(
            r#"result = add(1, 2)
def add(x: Int, y: Int) -> Int { x + y }"#,
        )
        .unwrap();

        let call_id = match &resolved[0] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::App(_, func, _) => match func.as_ref() {
                    Resolved::Var(_, id) => id.unique_id,
                    _ => panic!("Expected function variable in App"),
                },
                _ => panic!("Expected App on forward function reference"),
            },
            _ => panic!("Expected Bind"),
        };

        let def_id = match &resolved[1] {
            Resolved::Def(_, id, _, _, _) => id.unique_id,
            _ => panic!("Expected Def"),
        };

        assert_eq!(call_id, def_id);
    }

    #[test]
    fn test_forward_reference_to_struct_literal_resolves_to_same_unique_id() {
        let resolved = parse_and_resolve(
            r#"user = User { name: "alice", age: 30 }
defstruct User {
  name: String,
  age: Int,
}"#,
        )
        .unwrap();

        let lit_id = match &resolved[0] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::StructLit(_, id, _) => id.unique_id,
                _ => panic!("Expected StructLit"),
            },
            _ => panic!("Expected Bind"),
        };

        let def_id = match &resolved[1] {
            Resolved::StructDef(_, id, _) => id.unique_id,
            _ => panic!("Expected StructDef"),
        };

        assert_eq!(lit_id, def_id);
    }

    #[test]
    fn test_forward_reference_to_record_constructor_resolves_to_same_unique_id() {
        let resolved = parse_and_resolve(
            r#"point = Point(1.0, 2.0)
defrecord Point(x: Float, y: Float)"#,
        )
        .unwrap();

        let ctor_id = match &resolved[0] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::ConstructorCall(_, id, _) => id.unique_id,
                _ => panic!("Expected ConstructorCall"),
            },
            _ => panic!("Expected Bind"),
        };

        let def_id = match &resolved[1] {
            Resolved::RecordDef(_, id, _) => id.unique_id,
            _ => panic!("Expected RecordDef"),
        };

        assert_eq!(ctor_id, def_id);
    }

    #[test]
    fn test_forward_reference_to_deferror_constructor_resolves_to_same_unique_id() {
        let resolved = parse_and_resolve(
            r#"err = PageNotFound("404")
deferror PageNotFound(html: String) {
  "Page Not Found. #{html}"
}"#,
        )
        .unwrap();

        let ctor_id = match &resolved[0] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::ConstructorCall(_, id, _) => id.unique_id,
                _ => panic!("Expected ConstructorCall"),
            },
            _ => panic!("Expected Bind"),
        };

        let def_id = match &resolved[1] {
            Resolved::DeferrorDef(_, id, _, _) => id.unique_id,
            _ => panic!("Expected DeferrorDef"),
        };

        assert_eq!(ctor_id, def_id);
    }

    #[test]
    fn test_forward_reference_unique_ids_are_deterministic_across_runs() {
        let source = r#"result = build_user("alice")
point = Point(1, 2)
err = NotFound("404")

def build_user(name: String) -> String { name }
defrecord Point(x: Int, y: Int)
deferror NotFound(code: String) {
  "missing #{code}"
}"#;

        let first = parse_and_resolve(source).unwrap();
        let second = parse_and_resolve(source).unwrap();

        fn collect_top_level_ids(nodes: &[Resolved]) -> Vec<u32> {
            nodes
                .iter()
                .flat_map(|node| match node {
                    Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                        Resolved::App(_, func, _) => match func.as_ref() {
                            Resolved::Var(_, id) => vec![id.unique_id],
                            _ => Vec::new(),
                        },
                        Resolved::ConstructorCall(_, id, _) | Resolved::StructLit(_, id, _) => {
                            vec![id.unique_id]
                        }
                        _ => Vec::new(),
                    },
                    Resolved::Def(_, id, _, _, _)
                    | Resolved::RecordDef(_, id, _)
                    | Resolved::StructDef(_, id, _)
                    | Resolved::DeferrorDef(_, id, _, _) => vec![id.unique_id],
                    _ => Vec::new(),
                })
                .collect()
        }

        assert_eq!(
            collect_top_level_ids(&first),
            collect_top_level_ids(&second)
        );
    }

    #[test]
    fn test_unresolved_forward_constructor_is_error() {
        let result = parse_and_resolve(r#"value = MissingType(1)"#);
        let err = result.expect_err("unknown forward constructor must fail");
        assert!(err.message.contains("Undefined type: MissingType"));
    }

    #[test]
    fn test_duplicate_top_level_struct_is_error() {
        let result = parse_and_resolve(
            r#"defstruct User { name: String }
defstruct User { name: String }"#,
        );
        let err = result.expect_err("duplicate struct must fail");
        assert!(err.message.contains("Duplicate top-level definition: User"));
    }

    #[test]
    fn test_shadowing() {
        let resolved = parse_and_resolve("x = 1\nx = x + 1").unwrap();
        // The second x should have a different unique_id
        match (&resolved[0], &resolved[1]) {
            (
                Resolved::Bind(_, ResolvedPattern::Var(id1), _),
                Resolved::Bind(_, ResolvedPattern::Var(id2), _),
            ) => {
                assert_ne!(id1.unique_id, id2.unique_id);
            }
            _ => panic!("Expected two Binds"),
        }
    }

    #[test]
    fn test_match_wildcard_and_literals() {
        let resolved = parse_and_resolve(
            r#"s = "a"
x = match s {
  "a" => 1,
  2 => 2,
  _ => 0,
}"#,
        )
        .unwrap();
        match &resolved[1] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Match(_, _, arms) => {
                    assert!(matches!(&arms[0].0, ResolvedMatchPattern::StrLit(_, s) if s == "a"));
                    assert!(matches!(&arms[1].0, ResolvedMatchPattern::IntLit(_, 2)));
                    assert!(matches!(&arms[2].0, ResolvedMatchPattern::Wildcard(_)));
                }
                _ => panic!("Expected Match"),
            },
            _ => panic!("Expected Bind with Match"),
        }
    }

    #[test]
    fn test_closure_and_capture_resolution() {
        let resolved = parse_and_resolve(
            r#"x = 1
f = {|y| x + y}
g = &print(1)"#,
        )
        .unwrap();
        match &resolved[1] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Closure(_, params, captures, body) => {
                    assert_eq!(params.len(), 1);
                    assert_eq!(captures.len(), 1);
                    assert!(matches!(
                        body.as_ref(),
                        Resolved::BinOp(_, BinOp::Add, _, _)
                    ));
                }
                _ => panic!("Expected Closure"),
            },
            _ => panic!("Expected Bind"),
        }
        match &resolved[2] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Capture(_, target, args) => {
                    assert_eq!(args.len(), 1);
                    assert!(matches!(target.as_ref(), Resolved::Var(_, id) if id.name == "print"));
                }
                _ => panic!("Expected Capture"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_safebind_resolution() {
        let resolved = parse_and_resolve("num =? Ok(1)").unwrap();
        match &resolved[0] {
            Resolved::SafeBind(_, ResolvedPattern::Var(id), rhs) => {
                assert_eq!(id.name, "num");
                assert!(matches!(rhs.as_ref(), Resolved::ConstructorCall(_, _, _)));
            }
            _ => panic!("Expected SafeBind"),
        }
    }

    #[test]
    fn test_safebind_constructor_pattern_resolution() {
        let resolved = parse_and_resolve(
            r#"value: Result<Result<Int>> = Ok(Ok(1))
Ok(num) =? value"#,
        )
        .unwrap();
        match &resolved[0] {
            Resolved::Bind(_, _, _) => {}
            _ => panic!("Expected prelude bind"),
        }
        match &resolved[1] {
            Resolved::SafeBind(_, ResolvedPattern::Constructor(ctor, inner), rhs) => {
                assert_eq!(ctor.name, "Ok");
                assert!(matches!(inner.as_ref(), ResolvedPattern::Var(id) if id.name == "num"));
                assert!(matches!(rhs.as_ref(), Resolved::Var(_, id) if id.name == "value"));
            }
            _ => panic!("Expected SafeBind with constructor pattern"),
        }
    }

    #[test]
    fn test_safebind_list_with_constructor_literal_pattern_resolution() {
        let resolved = parse_and_resolve(
            r#"lr: Result<List<Result<Int>>> = Ok([Ok(1), Ok(2)])
[Ok(1), ..tail] =? lr"#,
        )
        .unwrap();
        match &resolved[0] {
            Resolved::Bind(_, _, _) => {}
            _ => panic!("Expected prelude bind"),
        }
        match &resolved[1] {
            Resolved::SafeBind(_, ResolvedPattern::ListCons(head, tail), rhs) => {
                assert!(matches!(
                    head.as_ref(),
                    ResolvedPattern::Constructor(ctor, inner)
                        if ctor.name == "Ok"
                        && matches!(inner.as_ref(), ResolvedPattern::IntLit(_, 1))
                ));
                assert!(matches!(tail.as_ref(), ResolvedPattern::Var(id) if id.name == "tail"));
                assert!(matches!(rhs.as_ref(), Resolved::Var(_, id) if id.name == "lr"));
            }
            _ => panic!("Expected SafeBind list constructor pattern"),
        }
    }

    #[test]
    fn test_as_pattern_resolution() {
        let resolved = parse_and_resolve(
            r#"value: Result<List<Int>> = Ok([1, 2, 3])
[head, ..tail] @ list_dup: List<Int> =? value"#,
        )
        .unwrap();
        match &resolved[1] {
            Resolved::SafeBind(_, ResolvedPattern::As(inner, alias, Some(_)), rhs) => {
                assert_eq!(alias.name, "list_dup");
                assert!(matches!(inner.as_ref(), ResolvedPattern::ListCons(_, _)));
                assert!(matches!(rhs.as_ref(), Resolved::Var(_, id) if id.name == "value"));
            }
            _ => panic!("Expected SafeBind with as-pattern"),
        }
    }

    #[test]
    fn test_duplicate_binding_in_pattern_is_error() {
        let err = parse_and_resolve(
            r#"value: Result<List<Int>> = Ok([1, 2, 3])
[head, ..tail] @ head =? value"#,
        )
        .expect_err("duplicate pattern binding should fail");
        assert!(err.message.contains("Duplicate binding in pattern: head"));
    }

    #[test]
    fn test_block_binding_does_not_escape() {
        let result = parse_and_resolve(
            r#"{
  x = 1
  x
}
x"#,
        );
        let err = result.expect_err("block-local binding must not escape");
        assert!(err.message.contains("Undefined variable: x"));
    }

    #[test]
    fn test_match_arm_binding_does_not_escape() {
        let result = parse_and_resolve(
            r#"value: Result<Int> = Ok(1)
match value {
  Ok(x) => x,
  Err(e) => 0,
}
x"#,
        );
        let err = result.expect_err("match-arm binding must not escape");
        assert!(err.message.contains("Undefined variable: x"));
    }

    #[test]
    fn test_match_arm_binding_does_not_leak_to_other_arms() {
        let result = parse_and_resolve(
            r#"value: Result<Int> = Ok(1)
match value {
  Ok(x) => x,
  Err(e) => x,
}"#,
        );
        let err = result.expect_err("match-arm binding must stay within its own arm");
        assert!(err.message.contains("Undefined variable: x"));
    }

    #[test]
    fn test_nested_closure_does_not_overcapture_outer_local() {
        let resolved = parse_and_resolve(r#"f = {|x| {|y| x + y}}"#).unwrap();
        match &resolved[0] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Closure(_, outer_params, outer_captures, outer_body) => {
                    assert_eq!(outer_params.len(), 1);
                    assert!(outer_captures.is_empty());
                    match outer_body.as_ref() {
                        Resolved::Closure(_, inner_params, inner_captures, inner_body) => {
                            assert_eq!(inner_params.len(), 1);
                            assert_eq!(inner_captures.len(), 1);
                            assert_eq!(inner_captures[0].name, "x");
                            assert!(matches!(
                                inner_body.as_ref(),
                                Resolved::BinOp(_, BinOp::Add, _, _)
                            ));
                        }
                        _ => panic!("Expected inner Closure"),
                    }
                }
                _ => panic!("Expected outer Closure"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_explicit_auto_import_is_rejected() {
        let err = parse_and_resolve(
            r#"import Bootstrap;
print("ok")"#,
        )
        .expect_err("explicit auto-import must fail");
        assert!(err.message.contains("Duplicate import"));
        assert!(err.message.contains("Bootstrap"));
    }

    #[test]
    fn test_explicit_kernel_auto_import_is_rejected() {
        let err = parse_and_resolve(
            r#"import Kernel;
print(to_string(add(1, 2)))"#,
        )
        .expect_err("explicit kernel auto-import must fail");
        assert!(err.message.contains("Duplicate import"));
        assert!(err.message.contains("Kernel"));
    }

    #[test]
    fn test_duplicate_module_import_is_rejected() {
        let module_stages = vec![vec![StagedModuleAst {
            module_path: "Helper".to_string(),
            ast: parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
        }]];

        let err = resolve_user_with_modules(
            r#"import Helper;
import Helper;
print(to_string(add(1, 2)))"#,
            &module_stages,
        )
        .expect_err("duplicate module import must fail");
        assert!(
            err.message.contains("Duplicate import"),
            "actual error: {}",
            err.message
        );
        assert!(
            err.message.contains("Helper"),
            "actual error: {}",
            err.message
        );
    }

    #[test]
    fn test_duplicate_module_then_member_import_is_rejected() {
        let module_stages = vec![vec![StagedModuleAst {
            module_path: "Helper".to_string(),
            ast: parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
        }]];

        let err = resolve_user_with_modules(
            r#"import Helper;
import Helper::add;
print(to_string(add(1, 2)))"#,
            &module_stages,
        )
        .expect_err("module followed by member import must fail");
        assert!(
            err.message.contains("Duplicate import"),
            "actual error: {}",
            err.message
        );
        assert!(
            err.message.contains("Helper::add"),
            "actual error: {}",
            err.message
        );
    }

    #[test]
    fn test_duplicate_member_then_module_import_is_rejected() {
        let module_stages = vec![vec![StagedModuleAst {
            module_path: "Helper".to_string(),
            ast: parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
        }]];

        let err = resolve_user_with_modules(
            r#"import Helper::add;
import Helper;
print(to_string(add(1, 2)))"#,
            &module_stages,
        )
        .expect_err("member followed by module import must fail");
        assert!(
            err.message.contains("Duplicate import"),
            "actual error: {}",
            err.message
        );
        assert!(
            err.message.contains("Helper"),
            "actual error: {}",
            err.message
        );
    }

    #[test]
    fn test_duplicate_member_import_is_rejected() {
        let module_stages = vec![vec![StagedModuleAst {
            module_path: "Helper".to_string(),
            ast: parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
        }]];

        let err = resolve_user_with_modules(
            r#"import Helper::add;
import Helper::add;
print(to_string(add(1, 2)))"#,
            &module_stages,
        )
        .expect_err("duplicate member import must fail");
        assert!(
            err.message.contains("Duplicate import"),
            "actual error: {}",
            err.message
        );
        assert!(
            err.message.contains("Helper::add"),
            "actual error: {}",
            err.message
        );
    }

    #[test]
    fn test_explicit_import_shadows_auto_imported_kernel_function() {
        let module_stages = vec![
            vec![StagedModuleAst {
                module_path: "Kernel".to_string(),
                ast: parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Kernel"),
            }],
            vec![StagedModuleAst {
                module_path: "Helper".to_string(),
                ast: parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x - y }"#, "Helper"),
            }],
        ];

        let resolved = resolve_user_with_modules(
            r#"import Helper::add;
print(to_string(add(7, 3)))"#,
            &module_stages,
        )
        .expect("explicit import should shadow auto-imported function");

        let helper_add_uid = resolved
            .iter()
            .find_map(|node| match node {
                Resolved::Def(_, id, _, _, _)
                    if id.qualified_name.as_deref() == Some("Helper::add") =>
                {
                    Some(id.unique_id)
                }
                _ => None,
            })
            .expect("helper add should be resolved");

        let imported_add_uid = resolved
            .iter()
            .find_map(|node| match node {
                Resolved::App(_, print_func, print_args) => {
                    if !matches!(print_func.as_ref(), Resolved::Var(_, id) if id.name == "print") {
                        return None;
                    }
                    let call = match print_args.first()? {
                        ResolvedRecordLitArg::Positional(inner) => inner,
                        _ => return None,
                    };
                    let call = match call {
                        Resolved::App(_, func, args) => {
                            if !matches!(func.as_ref(), Resolved::Var(_, id) if id.name == "to_string")
                            {
                                return None;
                            }
                            match args.first()? {
                                ResolvedRecordLitArg::Positional(inner) => inner,
                                _ => return None,
                            }
                        }
                        _ => return None,
                    };
                    match call {
                        Resolved::App(_, func, _) => match func.as_ref() {
                            Resolved::Var(_, id) if id.name == "add" => Some(id.unique_id),
                            _ => None,
                        },
                        _ => None,
                    }
                }
                _ => None,
            })
            .expect("user call should resolve imported add");

        assert_eq!(imported_add_uid, helper_add_uid);
    }

    #[test]
    fn test_capture_prefers_shadowed_local_function_name() {
        let resolved = parse_and_resolve(
            r#"print = {|x| x}
captured = &print"#,
        )
        .expect("shadowing + capture should resolve");

        let local_print_id = match &resolved[0] {
            Resolved::Bind(_, ResolvedPattern::Var(id), _) => id.unique_id,
            _ => panic!("Expected local print binding"),
        };

        let captured_target_id = match &resolved[1] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Capture(_, target, _) => match target.as_ref() {
                    Resolved::Var(_, id) => id.unique_id,
                    _ => panic!("Expected captured var target"),
                },
                _ => panic!("Expected capture expression"),
            },
            _ => panic!("Expected captured binding"),
        };

        assert_eq!(captured_target_id, local_print_id);
    }
}
