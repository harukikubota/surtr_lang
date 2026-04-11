use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use sindr::builtin::{builtin_uid, BUILTIN_METAS};
use spire::ast::{
    Ast, AstPattern, AstTy, ClosureParam, DeclAttrs, FunParam, RecordLitArg, Span,
};

use crate::error::ResolveError;
use crate::resolved::*;
use crate::scope::Scope;

const AUTO_IMPORT_MODULES: &[&str] = &["Bootstrap", "Kernel"];

fn initialize_base_scope() -> Scope {
    let mut scope = Scope::new();
    let dummy = Span { start: 0, end: 0 };
    scope.define("Ok", dummy.clone());
    scope.define("Err", dummy);
    scope
}

fn initialize_scope() -> Scope {
    let mut scope = initialize_base_scope();
    for meta in BUILTIN_METAS {
        if is_global_runtime_builtin(meta.name) {
            scope.define_with_id(meta.name, builtin_uid(meta.builtin_id));
        }
    }
    scope
}

fn is_global_runtime_builtin(name: &str) -> bool {
    matches!(
        name,
        "print" | "to_string" | "inspect" | "safe_div" | "safe_mod" | "eprint" | "set_exit_code"
    )
}

fn resolve_decl_attrs(attrs: &DeclAttrs) -> ResolvedDeclAttrs {
    ResolvedDeclAttrs {
        doc: attrs.doc.clone(),
    }
}

fn is_runtime_builtin_decl(name: &str) -> bool {
    BUILTIN_METAS.iter().any(|meta| meta.name == name)
}

fn is_special_form_builtin_decl(name: &str) -> bool {
    matches!(name, "if" | "if_then" | "assert" | "ensure")
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
    pub module_doc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationKind {
    Def,
    Struct,
    Record,
    Deferror,
    Enum,
    EnumVariant,
    ImplMethod,
    ImplCtorNew,
    BuiltinType,
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

fn is_importable_declaration(kind: &DeclarationKind) -> bool {
    !matches!(
        kind,
        DeclarationKind::BuiltinType | DeclarationKind::ImplCtorNew
    )
}

fn normalize_impl_method_name(target: &str, method_name: &str) -> String {
    format!("{}::{}", target, method_name)
}

fn impl_method_module_path(module_path: &str, target: &str) -> String {
    if module_path.is_empty() {
        target.to_string()
    } else {
        format!("{}::{}", module_path, target)
    }
}

fn rewrite_self_type(ty: AstTy, target: &str) -> AstTy {
    match ty {
        AstTy::Named(span, name) => {
            if name == "Self" {
                AstTy::Named(span, target.to_string())
            } else {
                AstTy::Named(span, name)
            }
        }
        AstTy::Generic(span, name, args) => AstTy::Generic(
            span,
            name,
            args.into_iter()
                .map(|arg| rewrite_self_type(arg, target))
                .collect(),
        ),
        AstTy::Func(span, params, ret) => AstTy::Func(
            span,
            params
                .into_iter()
                .map(|param| rewrite_self_type(param, target))
                .collect(),
            Box::new(rewrite_self_type(*ret, target)),
        ),
    }
}

fn rewrite_self_pattern(pat: AstPattern, target: &str) -> AstPattern {
    match pat {
        AstPattern::Annotated(span, name, ty) => {
            AstPattern::Annotated(span, name, rewrite_self_type(ty, target))
        }
        AstPattern::ListCons(span, head, tail) => AstPattern::ListCons(
            span,
            Box::new(rewrite_self_pattern(*head, target)),
            Box::new(rewrite_self_pattern(*tail, target)),
        ),
        AstPattern::Constructor(span, name, inners) => AstPattern::Constructor(
            span,
            name,
            inners
                .into_iter()
                .map(|inner| rewrite_self_pattern(inner, target))
                .collect(),
        ),
        AstPattern::As(span, inner, alias, alias_ty) => AstPattern::As(
            span,
            Box::new(rewrite_self_pattern(*inner, target)),
            alias,
            alias_ty.map(|ty| rewrite_self_type(ty, target)),
        ),
        other => other,
    }
}

fn rewrite_self_ast(node: Ast, target: &str) -> Ast {
    match node {
        Ast::Block(span, stmts) => Ast::Block(
            span,
            stmts
                .into_iter()
                .map(|stmt| rewrite_self_ast(stmt, target))
                .collect(),
        ),
        Ast::Bind(span, pat, rhs) => Ast::Bind(
            span,
            rewrite_self_pattern(pat, target),
            Box::new(rewrite_self_ast(*rhs, target)),
        ),
        Ast::SafeBind(span, pat, rhs) => Ast::SafeBind(
            span,
            rewrite_self_pattern(pat, target),
            Box::new(rewrite_self_ast(*rhs, target)),
        ),
        Ast::BinOp(span, op, left, right) => Ast::BinOp(
            span,
            op,
            Box::new(rewrite_self_ast(*left, target)),
            Box::new(rewrite_self_ast(*right, target)),
        ),
        Ast::ListCons(span, head, tail) => Ast::ListCons(
            span,
            Box::new(rewrite_self_ast(*head, target)),
            Box::new(rewrite_self_ast(*tail, target)),
        ),
        Ast::ListLiteral(span, elems) => Ast::ListLiteral(
            span,
            elems
                .into_iter()
                .map(|elem| rewrite_self_ast(elem, target))
                .collect(),
        ),
        Ast::InterpolatedStr(span, parts) => Ast::InterpolatedStr(
            span,
            parts
                .into_iter()
                .map(|part| match part {
                    spire::ast::InterpolatedPart::Text(text) => {
                        spire::ast::InterpolatedPart::Text(text)
                    }
                    spire::ast::InterpolatedPart::Expr(expr) => spire::ast::InterpolatedPart::Expr(
                        Box::new(rewrite_self_ast(*expr, target)),
                    ),
                })
                .collect(),
        ),
        Ast::Match(span, scrutinee, arms) => Ast::Match(
            span,
            Box::new(rewrite_self_ast(*scrutinee, target)),
            arms.into_iter()
                .map(|(pat, body)| {
                    (
                        rewrite_self_pattern(pat, target),
                        rewrite_self_ast(body, target),
                    )
                })
                .collect(),
        ),
        Ast::FieldAccess(span, expr, field) => {
            Ast::FieldAccess(span, Box::new(rewrite_self_ast(*expr, target)), field)
        }
        Ast::StructLit(span, name, fields) => Ast::StructLit(
            span,
            name,
            fields
                .into_iter()
                .map(|(field_name, expr)| (field_name, rewrite_self_ast(expr, target)))
                .collect(),
        ),
        Ast::ConstructorCall(span, name, args) => Ast::ConstructorCall(
            span,
            name,
            args.into_iter()
                .map(|arg| match arg {
                    RecordLitArg::Positional(expr) => {
                        RecordLitArg::Positional(rewrite_self_ast(expr, target))
                    }
                    RecordLitArg::Named(name, expr) => {
                        RecordLitArg::Named(name, rewrite_self_ast(expr, target))
                    }
                })
                .collect(),
        ),
        Ast::DeferrorDef(span, name, fields, show_expr, attrs) => Ast::DeferrorDef(
            span,
            name,
            fields
                .into_iter()
                .map(|field| spire::ast::RecordField {
                    name: field.name,
                    ty: rewrite_self_type(field.ty, target),
                    span: field.span,
                })
                .collect(),
            Box::new(rewrite_self_ast(*show_expr, target)),
            attrs,
        ),
        Ast::EnumDef(span, name, type_params, variants, attrs) => Ast::EnumDef(
            span,
            name,
            type_params,
            variants
                .into_iter()
                .map(|variant| spire::ast::EnumVariant {
                    name: variant.name,
                    payload: variant
                        .payload
                        .into_iter()
                        .map(|payload_ty| rewrite_self_type(payload_ty, target))
                        .collect(),
                    discriminant: variant.discriminant,
                    span: variant.span,
                })
                .collect(),
            attrs,
        ),
        Ast::Def(span, name, params, ret_ty, body, attrs) => Ast::Def(
            span,
            name,
            params
                .into_iter()
                .map(|param| FunParam {
                    name: param.name,
                    ty: rewrite_self_type(param.ty, target),
                    span: param.span,
                })
                .collect(),
            ret_ty.map(|ret| rewrite_self_type(ret, target)),
            Box::new(rewrite_self_ast(*body, target)),
            attrs,
        ),
        Ast::BuiltinDecl(span, name, params, ret_ty, attrs) => Ast::BuiltinDecl(
            span,
            name,
            params
                .into_iter()
                .map(|param| FunParam {
                    name: param.name,
                    ty: rewrite_self_type(param.ty, target),
                    span: param.span,
                })
                .collect(),
            ret_ty.map(|ret| rewrite_self_type(ret, target)),
            attrs,
        ),
        Ast::BuiltinTypeDecl(span, head, attrs) => Ast::BuiltinTypeDecl(
            span,
            spire::ast::BuiltinTypeHead {
                span: head.span,
                name: head.name,
                params: head.params,
            },
            attrs,
        ),
        Ast::ResultCtorDecl(span, name, param_ty, ret_ty, attrs) => Ast::ResultCtorDecl(
            span,
            name,
            rewrite_self_type(param_ty, target),
            rewrite_self_type(ret_ty, target),
            attrs,
        ),
        Ast::Closure(span, params, body) => Ast::Closure(
            span,
            params
                .into_iter()
                .map(|param| ClosureParam {
                    name: param.name,
                    ty: param.ty.map(|ty| rewrite_self_type(ty, target)),
                    span: param.span,
                })
                .collect(),
            Box::new(rewrite_self_ast(*body, target)),
        ),
        Ast::Capture(span, capture_target, args) => Ast::Capture(
            span,
            Box::new(rewrite_self_ast(*capture_target, target)),
            args.into_iter()
                .map(|arg| rewrite_self_ast(arg, target))
                .collect(),
        ),
        Ast::Semi(span, inner) => Ast::Semi(span, Box::new(rewrite_self_ast(*inner, target))),
        other => other,
    }
}

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
    for (fq_name, entry) in index {
        if entry.kind == DeclarationKind::BuiltinType {
            continue;
        }
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
    let mut import_context = ImportContext {
        declaration_index,
        declaration_uids,
        current_stage_index,
        import_state: &mut import_state,
    };

    for stmt in stmts {
        if let Ast::Import(span, path, spec) = stmt {
            apply_import_to_scope(&mut scope, &mut import_context, path, spec, span.clone())?;
        }
    }

    for auto_import in AUTO_IMPORT_MODULES {
        if current_module_path == Some(*auto_import) {
            continue;
        }
        import_module_into_scope(
            &mut scope,
            &mut import_context,
            auto_import,
            true,
            Span { start: 0, end: 0 },
        )?;
    }

    if let Some(module_path) = current_module_path {
        for entry in declaration_index.values() {
            if entry.module_path == module_path {
                if !is_importable_declaration(&entry.kind) {
                    continue;
                }
                if let Some(uid) = declaration_uids.get(&entry.fq_name) {
                    scope.define_with_id(&entry.name, *uid);
                }
            }
        }
    }

    Ok(scope)
}

struct ImportContext<'a> {
    declaration_index: &'a DeclarationIndex,
    declaration_uids: &'a HashMap<String, u32>,
    current_stage_index: usize,
    import_state: &'a mut ImportState,
}

fn apply_import_to_scope(
    scope: &mut Scope,
    import_context: &mut ImportContext<'_>,
    path: &spire::ast::AstPath,
    spec: &spire::ast::ImportSpec,
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
            import_context,
            &module_name,
            false,
            span,
        ),
        spire::ast::ImportSpec::Single(name) => import_single_into_scope(
            scope,
            import_context,
            &module_name,
            name,
            span,
        ),
        spire::ast::ImportSpec::List(names) => {
            for name in names {
                import_single_into_scope(
                    scope,
                    import_context,
                    &module_name,
                    name,
                    span.clone(),
                )?;
            }
            Ok(())
        }
    }
}

fn import_module_into_scope(
    scope: &mut Scope,
    import_context: &mut ImportContext<'_>,
    module_name: &str,
    auto_import: bool,
    span: Span,
) -> Result<(), ResolveError> {
    if !auto_import {
        import_context
            .import_state
            .record_module_import(module_name, &span)?;
    }
    let mut imported_any = false;
    let mut blocked_by_stage = false;
    for entry in import_context.declaration_index.values() {
        if entry.module_path != module_name {
            continue;
        }
        if !is_importable_declaration(&entry.kind) {
            continue;
        }
        if entry.stage_index >= import_context.current_stage_index {
            blocked_by_stage = true;
            continue;
        }
        let uid = import_context.declaration_uids[&entry.fq_name];
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

    if imported_any || (auto_import && AUTO_IMPORT_MODULES.contains(&module_name)) {
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
    import_context: &mut ImportContext<'_>,
    module_name: &str,
    name: &str,
    span: Span,
) -> Result<(), ResolveError> {
    import_context
        .import_state
        .record_member_import(module_name, name, &span)?;

    let fq_name = format!("{}::{}", module_name, name);
    let Some(entry) = import_context.declaration_index.get(&fq_name) else {
        let module_exists = import_context
            .declaration_index
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

    if !is_importable_declaration(&entry.kind) {
        return Err(ResolveError {
            message: format!("Import target `{}` is not importable", fq_name),
            span,
        });
    }

    if entry.stage_index >= import_context.current_stage_index {
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
        import_context.declaration_uids[&entry.fq_name],
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
/// `def`, `@@builtin def`, `@@builtin type`, `defstruct`, `defrecord`, `deferror`.
pub fn precollect_declaration_index(
    module_stages: &[Vec<StagedModuleAst>],
) -> Result<DeclarationIndex, ResolveError> {
    let mut index = DeclarationIndex::new();
    let mut seen_impl_targets = HashSet::new();

    for (stage_index, stage) in module_stages.iter().enumerate() {
        for module in stage {
            let mut local_types: HashMap<String, DeclarationKind> = HashMap::new();
            for stmt in &module.ast {
                match stmt {
                    Ast::StructDef(_, name, _) => {
                        local_types.insert(name.clone(), DeclarationKind::Struct);
                    }
                    Ast::EnumDef(_, name, _, _, _) => {
                        local_types.insert(name.clone(), DeclarationKind::Enum);
                    }
                    Ast::RecordDef(_, name, _) => {
                        local_types.insert(name.clone(), DeclarationKind::Record);
                    }
                    Ast::DeferrorDef(_, name, _, _, _) => {
                        local_types.insert(name.clone(), DeclarationKind::Deferror);
                    }
                    _ => {}
                }
            }

            for stmt in &module.ast {
                if let Ast::ImplDef(span, target, methods) = stmt {
                    let Some(target_kind) = local_types.get(target) else {
                        return Err(ResolveError {
                            message: format!(
                                "impl target `{}` must be a locally defined struct or enum",
                                target
                            ),
                            span: span.clone(),
                        });
                    };
                    if !matches!(
                        target_kind,
                        &DeclarationKind::Struct | &DeclarationKind::Enum
                    ) {
                        return Err(ResolveError {
                            message: format!(
                                "impl target `{}` must be struct or enum (record is not supported)",
                                target
                            ),
                            span: span.clone(),
                        });
                    }

                    let target_fq = if module.module_path.is_empty() {
                        target.clone()
                    } else {
                        format!("{}::{}", module.module_path, target)
                    };
                    if !seen_impl_targets.insert(target_fq.clone()) {
                        return Err(ResolveError {
                            message: format!(
                                "Multiple impl blocks for `{}` are not allowed",
                                target_fq
                            ),
                            span: span.clone(),
                        });
                    }

                    let method_module_path = impl_method_module_path(&module.module_path, target);
                    for method in methods {
                        let Ast::Def(method_span, method_name, _, _, _, _) = method else {
                            return Err(ResolveError {
                                message: "impl body may only contain `def` declarations"
                                    .to_string(),
                                span: span.clone(),
                            });
                        };

                        let kind = if method_name == "new" {
                            if !matches!(target_kind, &DeclarationKind::Struct) {
                                return Err(ResolveError {
                                    message:
                                        "`new` is only allowed in impl blocks for struct types"
                                            .to_string(),
                                    span: method_span.clone(),
                                });
                            }
                            DeclarationKind::ImplCtorNew
                        } else {
                            DeclarationKind::ImplMethod
                        };

                        let fq_name = format!("{}::{}", method_module_path, method_name);
                        if let Some(prev) = index.get(&fq_name) {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate fully-qualified declaration: {} (already declared in stage {} module {})",
                                    fq_name, prev.stage_index, prev.module_path
                                ),
                                span: method_span.clone(),
                            });
                        }

                        index.insert(
                            fq_name.clone(),
                            DeclarationEntry {
                                module_path: method_module_path.clone(),
                                name: method_name.clone(),
                                fq_name,
                                kind,
                                stage_index,
                            },
                        );
                    }
                    continue;
                }

                if let Ast::EnumDef(span, name, _, variants, _) = stmt {
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
                            name: name.clone(),
                            fq_name,
                            kind: DeclarationKind::Enum,
                            stage_index,
                        },
                    );

                    for variant in variants {
                        let variant_name = format!("{}::{}", name, variant.name);
                        let variant_fq_name = if module.module_path.is_empty() {
                            variant_name.clone()
                        } else {
                            format!("{}::{}", module.module_path, variant_name)
                        };
                        if let Some(prev) = index.get(&variant_fq_name) {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate fully-qualified declaration: {} (already declared in stage {} module {})",
                                    variant_fq_name, prev.stage_index, prev.module_path
                                ),
                                span: variant.span.clone(),
                            });
                        }
                        index.insert(
                            variant_fq_name.clone(),
                            DeclarationEntry {
                                module_path: module.module_path.clone(),
                                name: variant_name,
                                fq_name: variant_fq_name,
                                kind: DeclarationKind::EnumVariant,
                                stage_index,
                            },
                        );
                    }
                    continue;
                }

                let (span, name, kind) = match stmt {
                    Ast::Def(span, name, _, _, _, _) => (span, name.as_str(), DeclarationKind::Def),
                    Ast::BuiltinDecl(span, name, _, _, _) => {
                        (span, name.as_str(), DeclarationKind::Def)
                    }
                    Ast::ImplDef(_, _, _) => continue,
                    Ast::ResultCtorDecl(_, _, _, _, _) => continue,
                    Ast::BuiltinTypeDecl(span, head, _) => {
                        (span, head.name.as_str(), DeclarationKind::BuiltinType)
                    }
                    Ast::StructDef(span, name, _) => (span, name.as_str(), DeclarationKind::Struct),
                    Ast::RecordDef(span, name, _) => (span, name.as_str(), DeclarationKind::Record),
                    Ast::DeferrorDef(span, name, _, _, _) => {
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

    pub fn lookup_uid(&self, name: &str) -> Option<u32> {
        self.scope.lookup(name)
    }

    pub fn define_with_id(&mut self, name: &str, id: u32) {
        self.scope.define_with_id(name, id);
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
        let stmts = self.lower_impl_defs(stmts)?;
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

    fn lower_impl_defs(&self, stmts: Vec<Ast>) -> Result<Vec<Ast>, ResolveError> {
        let mut local_types: HashMap<String, DeclarationKind> = HashMap::new();
        for stmt in &stmts {
            match stmt {
                Ast::StructDef(_, name, _) => {
                    local_types.insert(name.clone(), DeclarationKind::Struct);
                }
                Ast::EnumDef(_, name, _, _, _) => {
                    local_types.insert(name.clone(), DeclarationKind::Enum);
                }
                Ast::RecordDef(_, name, _) => {
                    local_types.insert(name.clone(), DeclarationKind::Record);
                }
                Ast::DeferrorDef(_, name, _, _, _) => {
                    local_types.insert(name.clone(), DeclarationKind::Deferror);
                }
                _ => {}
            }
        }

        let mut lowered = Vec::new();
        let mut seen_impl_targets = HashSet::new();

        for stmt in stmts {
            match stmt {
                Ast::ImplDef(span, target, methods) => {
                    let Some(target_kind) = local_types.get(&target) else {
                        return Err(ResolveError {
                            message: format!(
                                "impl target `{}` must be a locally defined struct or enum",
                                target
                            ),
                            span,
                        });
                    };
                    if !matches!(
                        target_kind,
                        &DeclarationKind::Struct | &DeclarationKind::Enum
                    ) {
                        return Err(ResolveError {
                            message: format!(
                                "impl target `{}` must be struct or enum (record is not supported)",
                                target
                            ),
                            span,
                        });
                    }
                    if !seen_impl_targets.insert(target.clone()) {
                        return Err(ResolveError {
                            message: format!(
                                "Multiple impl blocks for `{}` are not allowed",
                                target
                            ),
                            span,
                        });
                    }

                    for method in methods {
                        let Ast::Def(method_span, method_name, params, ret_ty, body, attrs) =
                            method
                        else {
                            return Err(ResolveError {
                                message: "impl body may only contain `def` declarations"
                                    .to_string(),
                                span: span.clone(),
                            });
                        };

                        if method_name == "new" && !matches!(target_kind, &DeclarationKind::Struct)
                        {
                            return Err(ResolveError {
                                message: "`new` is only allowed in impl blocks for struct types"
                                    .to_string(),
                                span: method_span,
                            });
                        }

                        let lowered_name = normalize_impl_method_name(&target, &method_name);
                        let lowered_params = params
                            .into_iter()
                            .map(|param| FunParam {
                                name: param.name,
                                ty: rewrite_self_type(param.ty, &target),
                                span: param.span,
                            })
                            .collect::<Vec<_>>();
                        let lowered_ret_ty = ret_ty.map(|ty| rewrite_self_type(ty, &target));
                        let lowered_body = rewrite_self_ast(*body, &target);

                        lowered.push(Ast::Def(
                            method_span,
                            lowered_name,
                            lowered_params,
                            lowered_ret_ty,
                            Box::new(lowered_body),
                            attrs,
                        ));
                    }
                }
                other => lowered.push(other),
            }
        }

        Ok(lowered)
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
                Ast::Defmod(_, _, body, _) => self.validate_auto_import_conflicts(body)?,
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
                Ast::Def(span, name, _, _, _, _) => {
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
                Ast::BuiltinDecl(_, name, _, _, _) => {
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
                Ast::ResultCtorDecl(span, name, _, _, _) => {
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
                        .scope
                        .lookup(name)
                        .unwrap_or_else(|| self.scope.reserve_id());
                    self.predeclared_ids
                        .entry(name.clone())
                        .or_default()
                        .push_back(uid);
                    self.scope.define_with_id(name, uid);
                }
                Ast::BuiltinTypeDecl(span, head, _) => {
                    if !declared_in_batch.insert(head.name.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", head.name),
                            span: span.clone(),
                        });
                    }
                    if !self.allow_top_level_shadowing && self.scope.lookup(&head.name).is_some() {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", head.name),
                            span: span.clone(),
                        });
                    }
                    let uid = self.scope.reserve_id();
                    self.predeclared_ids
                        .entry(head.name.clone())
                        .or_default()
                        .push_back(uid);
                }
                Ast::StructDef(span, name, _)
                | Ast::RecordDef(span, name, _)
                | Ast::DeferrorDef(span, name, _, _, _) => {
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
                Ast::EnumDef(span, name, _, variants, _) => {
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

                    for variant in variants {
                        let qualified_ctor = format!("{}::{}", name, variant.name);
                        if !declared_in_batch.insert(qualified_ctor.clone()) {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate top-level definition: {}",
                                    qualified_ctor
                                ),
                                span: variant.span.clone(),
                            });
                        }
                        if !self.allow_top_level_shadowing
                            && self.scope.lookup(&qualified_ctor).is_some()
                        {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate top-level definition: {}",
                                    qualified_ctor
                                ),
                                span: variant.span.clone(),
                            });
                        }
                        let ctor_uid = self
                            .declaration_uids
                            .get(&self.qualify_current_declaration_name(&qualified_ctor))
                            .copied()
                            .unwrap_or_else(|| self.scope.reserve_id());
                        self.predeclared_ids
                            .entry(qualified_ctor.clone())
                            .or_default()
                            .push_back(ctor_uid);
                        self.scope.define_with_id(&qualified_ctor, ctor_uid);
                    }
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
                // Check for special forms
                if let Ast::Var(_, ref name) = *func {
                    if name == "if" {
                        return self.resolve_if(span, args, IfKind::If3);
                    }
                    if name == "if_then" {
                        return self.resolve_if(span, args, IfKind::IfThen2);
                    }
                    if name == "assert" {
                        return self.resolve_assert(span, args);
                    }
                    if name == "ensure" {
                        return self.resolve_ensure(span, args);
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

            Ast::Pipe(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::Pipe(span, Box::new(l), Box::new(r)))
            }

            Ast::ContextMap(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::ContextMap(span, Box::new(l), Box::new(r)))
            }

            Ast::ContextBind(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::ContextBind(span, Box::new(l), Box::new(r)))
            }

            Ast::Compose(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::Compose(span, Box::new(l), Box::new(r)))
            }

            Ast::KleisliCompose(span, left, right) => {
                let l = self.resolve_node(*left)?;
                let r = self.resolve_node(*right)?;
                Ok(Resolved::KleisliCompose(span, Box::new(l), Box::new(r)))
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

            Ast::DeferrorDef(span, name, fields, show_expr, _) => {
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

            Ast::EnumDef(span, name, type_params, variants, _) => {
                let uid = self
                    .take_predeclared_id(&name)
                    .or_else(|| self.scope.lookup(&name))
                    .unwrap_or_else(|| self.scope.reserve_id());
                self.scope.define_with_id(&name, uid);
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name: name.clone(),
                    qualified_name: Some(qualified_name),
                    unique_id: uid,
                    span: span.clone(),
                };
                let resolved_type_params = type_params
                    .into_iter()
                    .map(|param| ResolvedTypeParam {
                        name: param.name,
                        span: param.span,
                    })
                    .collect::<Vec<_>>();

                let mut resolved_variants = Vec::new();
                for variant in variants {
                    let ctor_name = format!("{}::{}", name, variant.name);
                    let ctor_uid = self
                        .take_predeclared_id(&ctor_name)
                        .or_else(|| self.scope.lookup(&ctor_name))
                        .unwrap_or_else(|| self.scope.reserve_id());
                    self.scope.define_with_id(&ctor_name, ctor_uid);
                    let qualified_ctor_name = self.qualify_current_declaration_name(&ctor_name);
                    resolved_variants.push(ResolvedEnumVariant {
                        id: ResolvedId {
                            name: ctor_name,
                            qualified_name: Some(qualified_ctor_name),
                            unique_id: ctor_uid,
                            span: variant.span.clone(),
                        },
                        payload: variant.payload,
                        discriminant: variant.discriminant,
                        span: variant.span,
                    });
                }

                Ok(Resolved::EnumDef(
                    span,
                    rid,
                    resolved_type_params,
                    resolved_variants,
                ))
            }

            Ast::Def(span, name, params, ret_ty, body, attrs) => {
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
                    resolve_decl_attrs(&attrs),
                ))
            }

            Ast::BuiltinDecl(span, name, params, ret_ty, attrs) => {
                if !is_runtime_builtin_decl(&name) && !is_special_form_builtin_decl(&name) {
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
                let qualified_name = self.qualify_current_declaration_name(&name);
                let rid = ResolvedId {
                    name,
                    qualified_name: Some(qualified_name),
                    unique_id: builtin_uid,
                    span: span.clone(),
                };
                Ok(Resolved::BuiltinDecl(
                    span,
                    rid,
                    resolved_params,
                    ret_ty,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::BuiltinTypeDecl(span, head, attrs) => {
                let builtin_type_uid = self
                    .take_predeclared_id(&head.name)
                    .unwrap_or_else(|| self.scope.reserve_id());
                let qualified_name = self.qualify_current_declaration_name(&head.name);
                let rid = ResolvedId {
                    name: head.name,
                    qualified_name: Some(qualified_name),
                    unique_id: builtin_type_uid,
                    span: span.clone(),
                };
                Ok(Resolved::BuiltinTypeDecl(
                    span,
                    rid,
                    head.params,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::ResultCtorDecl(span, name, param_ty, ret_ty, attrs) => {
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
                Ok(Resolved::ResultCtorDecl(
                    span,
                    rid,
                    param_ty,
                    ret_ty,
                    resolve_decl_attrs(&attrs),
                ))
            }
            Ast::Defmod(span, name, _, _) => Err(ResolveError {
                message: format!("Module resolution is not implemented yet: {}", name),
                span,
            }),
            Ast::Import(span, _, _) => Err(ResolveError {
                message: "Import resolution is not implemented yet".to_string(),
                span,
            }),
            Ast::ImplDef(span, target, _) => Err(ResolveError {
                message: format!("impl lowering failed for target `{}`", target),
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
                        ty: param.ty,
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
                let normalized_name = {
                    let sugared = format!("{}::new", type_name);
                    if self.scope.lookup(&sugared).is_some() {
                        sugared
                    } else {
                        type_name
                    }
                };
                let uid = self
                    .scope
                    .lookup(&normalized_name)
                    .ok_or_else(|| ResolveError {
                        message: format!("Undefined type: {}", normalized_name),
                        span: span.clone(),
                    })?;
                let rid = ResolvedId {
                    name: normalized_name,
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

    fn resolve_assert(&mut self, span: Span, args: Vec<RecordLitArg>) -> Result<Resolved, ResolveError> {
        if args.len() != 2 {
            return Err(ResolveError {
                message: format!("assert expects 2 arguments, got {}", args.len()),
                span,
            });
        }

        let mut positional = Vec::with_capacity(args.len());
        for arg in args {
            match arg {
                RecordLitArg::Positional(expr) => positional.push(expr),
                RecordLitArg::Named(name, _) => {
                    return Err(ResolveError {
                        message: format!("assert does not accept named argument '{}'", name),
                        span,
                    });
                }
            }
        }

        let mut iter = positional.into_iter();
        let cond = self.resolve_node(iter.next().expect("checked arg length"))?;
        let err = self.resolve_node(iter.next().expect("checked arg length"))?;
        Ok(Resolved::Assert(span, Box::new(cond), Box::new(err)))
    }

    fn resolve_ensure(&mut self, span: Span, args: Vec<RecordLitArg>) -> Result<Resolved, ResolveError> {
        if args.len() != 3 {
            return Err(ResolveError {
                message: format!("ensure expects 3 arguments, got {}", args.len()),
                span,
            });
        }

        let mut positional = Vec::with_capacity(args.len());
        for arg in args {
            match arg {
                RecordLitArg::Positional(expr) => positional.push(expr),
                RecordLitArg::Named(name, _) => {
                    return Err(ResolveError {
                        message: format!("ensure does not accept named argument '{}'", name),
                        span,
                    });
                }
            }
        }

        let mut iter = positional.into_iter();
        let value = self.resolve_node(iter.next().expect("checked arg length"))?;
        let pred = self.resolve_node(iter.next().expect("checked arg length"))?;
        let err = self.resolve_node(iter.next().expect("checked arg length"))?;
        Ok(Resolved::Ensure(
            span,
            Box::new(value),
            Box::new(pred),
            Box::new(err),
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
            AstPattern::Constructor(span, ctor_name, inners) => {
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
                    inners
                        .into_iter()
                        .map(|inner| self.resolve_pattern_inner(inner, seen))
                        .collect::<Result<Vec<_>, _>>()?,
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
        pat: AstPattern,
        body: Ast,
    ) -> Result<(ResolvedPattern, Resolved), ResolveError> {
        self.with_child_scope(|child| {
            let resolved_pat = child.resolve_pattern(pat)?;
            let resolved_body = child.resolve_node(body)?;
            Ok((resolved_pat, resolved_body))
        })
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
                    Resolved::Def(_, id, params, _, _, _) => {
                        local_bound.insert(id.unique_id);
                        for param in params {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    Resolved::BuiltinDecl(_, id, params, _, _) => {
                        local_bound.insert(id.unique_id);
                        for param in params {
                            local_bound.insert(param.id.unique_id);
                        }
                    }
                    Resolved::BuiltinTypeDecl(_, _, _, _) => {}
                    Resolved::ResultCtorDecl(_, _, _, _, _) => {}
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
        Resolved::Pipe(_, left, right)
        | Resolved::ContextMap(_, left, right)
        | Resolved::ContextBind(_, left, right)
        | Resolved::Compose(_, left, right)
        | Resolved::KleisliCompose(_, left, right) => {
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
        Resolved::Assert(_, cond, err) => {
            collect_captures_inner(cond, bound, free);
            collect_captures_inner(err, bound, free);
        }
        Resolved::Ensure(_, value, pred, err) => {
            collect_captures_inner(value, bound, free);
            collect_captures_inner(pred, bound, free);
            collect_captures_inner(err, bound, free);
        }
        Resolved::Match(_, scrutinee, arms) => {
            collect_captures_inner(scrutinee, bound, free);
            for (pat, body) in arms {
                let mut arm_bound = bound.clone();
                collect_bind_pattern_bindings(pat, &mut arm_bound);
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
        | Resolved::EnumDef(_, _, _, _)
        | Resolved::BuiltinDecl(_, _, _, _, _)
        | Resolved::BuiltinTypeDecl(_, _, _, _)
        | Resolved::ResultCtorDecl(_, _, _, _, _) => {}
        Resolved::Def(_, id, params, _, body, _) => {
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

fn collect_bind_pattern_bindings(pat: &ResolvedPattern, bound: &mut HashSet<u32>) {
    match pat {
        ResolvedPattern::Var(id) | ResolvedPattern::Annotated(id, _) => {
            bound.insert(id.unique_id);
        }
        ResolvedPattern::Constructor(_, inners) => {
            for inner in inners {
                collect_bind_pattern_bindings(inner, bound);
            }
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
    use sindr::primitives::int;
    use spire::ast::{AstTy, BinOp, Lit};

    fn parse_module_ast(src: &str, module_path: &str) -> Vec<Ast> {
        let _ = module_path;
        spire::parse(src).expect("module source should parse")
    }

    fn parse_and_resolve(src: &str) -> Result<Vec<Resolved>, ResolveError> {
        let ast = spire::parse(src).expect("parse failed");
        resolve(ast)
    }

    fn staged_module(module_path: &str, ast: Vec<Ast>) -> StagedModuleAst {
        StagedModuleAst {
            module_path: module_path.to_string(),
            ast,
            module_doc: None,
        }
    }

    fn resolve_user_with_modules(
        user_src: &str,
        module_stages: &[Vec<StagedModuleAst>],
    ) -> Result<Vec<Resolved>, ResolveError> {
        let user_ast = spire::parse(user_src).expect("user script should parse");
        let mut full_stages = vec![vec![staged_module(
            "Bootstrap",
            parse_module_ast(
                r#"@@builtin def print(a: String) -> Unit
@@builtin def to_string(a: $A) -> String
@@builtin def inspect(a: $A) -> String
@@builtin def safe_div(a: $A, b: $A) -> Result<$A, ZeroDivisionError>
@@builtin def safe_mod(a: Int, b: Int) -> Result<Int, ZeroDivisionError>
@@builtin def eprint(err: Error) -> Unit
@@builtin def set_exit_code(code: Int) -> Unit
deferror NoneError { "none" }
deferror ZeroDivisionError { "division by zero" }
deferror EmptyList { "Empty List." }
deferror IndexOutOfBounds(detail: String) { detail }"#,
                "Bootstrap",
            ),
        )]];
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
        let module_stages = vec![vec![staged_module(
            "Bootstrap",
            parse_module_ast(
                r#"def to_int(x: String) -> Int { unknown_name }
defrecord Pair(left: Int, right: Int)
deferror Oops(reason: String) { reason }"#,
                "Bootstrap",
            ),
        )]];

        let index =
            precollect_declaration_index(&module_stages).expect("precollect should succeed");
        assert!(index.contains_key("Bootstrap::to_int"));
        assert!(index.contains_key("Bootstrap::Pair"));
        assert!(index.contains_key("Bootstrap::Oops"));
    }

    #[test]
    fn test_precollect_builtin_decl_in_module() {
        let module_stages = vec![vec![staged_module(
            "Int",
            parse_module_ast(
                r#"@@builtin def shl(value: Int, bits: Int) -> Result<Int, NegativeShiftCount>"#,
                "Int",
            ),
        )]];

        let index =
            precollect_declaration_index(&module_stages).expect("precollect should succeed");
        assert!(index.contains_key("Int::shl"));
    }

    #[test]
    fn test_precollect_declaration_index_rejects_duplicate_fully_qualified_name() {
        let module_stages = vec![vec![
            staged_module(
                "Std::Math",
                parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Std::Math"),
            ),
            staged_module(
                "Std::Math",
                parse_module_ast(r#"def add(a: Int, b: Int) -> Int { a + b }"#, "Std::Math"),
            ),
        ]];

        let err = precollect_declaration_index(&module_stages)
            .expect_err("duplicate fully-qualified declaration must fail");
        assert!(err
            .message
            .contains("Duplicate fully-qualified declaration: Std::Math::add"));
    }

    #[test]
    fn test_precollect_declaration_index_is_deterministic_when_stage_input_order_changes() {
        let mod_a = staged_module(
            "Std::A",
            parse_module_ast(r#"def same(x: Int) -> Int { x }"#, "Std::A"),
        );
        let mod_b = staged_module(
            "Std::B",
            parse_module_ast(r#"def same(x: Int) -> Int { x }"#, "Std::B"),
        );

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
            vec![staged_module(
                "Bootstrap",
                parse_module_ast(r#"deferror NoneError { "none" }"#, "Bootstrap"),
            )],
            vec![staged_module(
                "Std::Math",
                parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Std::Math"),
            )],
            vec![staged_module(
                "User::Main",
                parse_module_ast(r#"def main() -> Int { 1 }"#, "User::Main"),
            )],
        ];

        let index =
            precollect_declaration_index(&module_stages).expect("precollect should succeed");
        assert_eq!(index["Bootstrap::NoneError"].stage_index, 0);
        assert_eq!(index["Std::Math::add"].stage_index, 1);
        assert_eq!(index["User::Main::main"].stage_index, 2);
    }

    #[test]
    fn test_precollect_impl_methods_as_type_namespace_members() {
        let module_stages = vec![vec![staged_module(
            "",
            parse_module_ast(
                r#"defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }

  def normalize(self) -> Self {
    self
  }
}"#,
                "",
            ),
        )]];

        let index =
            precollect_declaration_index(&module_stages).expect("precollect should succeed");
        let ctor = index.get("User::new").expect("new should be indexed");
        assert_eq!(ctor.module_path, "User");
        assert_eq!(ctor.name, "new");
        assert_eq!(ctor.kind, DeclarationKind::ImplCtorNew);

        let normalize = index
            .get("User::normalize")
            .expect("normalize should be indexed");
        assert_eq!(normalize.module_path, "User");
        assert_eq!(normalize.name, "normalize");
        assert_eq!(normalize.kind, DeclarationKind::ImplMethod);
    }

    #[test]
    fn test_precollect_rejects_multiple_impl_blocks_for_same_type() {
        let module_stages = vec![vec![staged_module(
            "",
            parse_module_ast(
                r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
    User { name: name }
  }
}
impl User {
  def normalize(self) -> Self {
    self
  }
}"#,
                "",
            ),
        )]];

        let err =
            precollect_declaration_index(&module_stages).expect_err("duplicate impl must fail");
        assert!(err
            .message
            .contains("Multiple impl blocks for `User` are not allowed"));
    }

    #[test]
    fn test_precollect_rejects_impl_target_for_record() {
        let module_stages = vec![vec![staged_module(
            "",
            parse_module_ast(
                r#"defrecord Pair(first: Int, second: Int)
impl Pair {
  def new(first: Int, second: Int) -> Self {
    Pair(first, second)
  }
}"#,
                "",
            ),
        )]];

        let err = precollect_declaration_index(&module_stages)
            .expect_err("record impl should be rejected");
        assert!(err
            .message
            .contains("impl target `Pair` must be struct or enum"));
    }

    #[test]
    fn test_import_new_from_impl_is_rejected() {
        let module_stages = vec![vec![staged_module(
            "",
            parse_module_ast(
                r#"defstruct User {
  name: String,
}
impl User {
  def new(name: String) -> Self {
    User { name: name }
  }
  def normalize(self) -> Self {
    self
  }
}"#,
                "",
            ),
        )]];

        let err = resolve_user_with_modules(
            r#"import User::new
User("alice")"#,
            &module_stages,
        )
        .expect_err("new import should fail");
        assert!(err.message.contains("is not importable"));
    }

    #[test]
    fn test_constructor_call_sugars_to_type_new_resolution() {
        let resolved = parse_and_resolve(
            r#"defstruct User {
  name: String,
  age: Int,
}
impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
}
user = User("alice", 30)"#,
        )
        .expect("source should resolve");

        let constructor_name = resolved.iter().find_map(|node| match node {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::ConstructorCall(_, rid, _) => Some(rid.name.clone()),
                _ => None,
            },
            _ => None,
        });
        assert_eq!(constructor_name.as_deref(), Some("User::new"));
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
            Resolved::BuiltinDecl(_, id, params, ret_ty, attrs) => {
                assert_eq!(id.name, "print");
                assert_eq!(id.unique_id, 2); // 0=Ok, 1=Err, 2=print
                assert_eq!(params.len(), 1);
                assert_eq!(attrs, &ResolvedDeclAttrs::default());
                assert!(matches!(
                    ret_ty,
                    Some(spire::ast::AstTy::Named(_, ty)) if ty == "Unit"
                ));
            }
            _ => panic!("Expected BuiltinDecl"),
        }
    }

    #[test]
    fn test_builtin_type_decl_resolution() {
        let ast = spire::parse_with_context(
            "@@builtin type Int",
            spire::ParserContext::module(0, Some("Bootstrap".into()))
                .with_rules(spire::SourceRules::std_module()),
        )
        .expect("std module should parse builtin type declarations");
        let mut resolver = Resolver::new();
        let resolved = resolver
            .resolve_program(ast)
            .expect("builtin type declaration should resolve");
        match &resolved[0] {
            Resolved::BuiltinTypeDecl(_, id, params, attrs) => {
                assert_eq!(id.name, "Int");
                assert!(params.is_empty());
                assert_eq!(attrs, &ResolvedDeclAttrs::default());
            }
            _ => panic!("Expected BuiltinTypeDecl"),
        }
    }

    #[test]
    fn test_module_builtin_can_be_resolved_by_qualified_name() {
        let module_stages = vec![vec![staged_module(
            "Int",
            parse_module_ast(
                r#"@@builtin def shl(value: Int, bits: Int) -> Result<Int, NegativeShiftCount>"#,
                "Int",
            ),
        )]];

        let resolved = resolve_user_with_modules("value = Int::shl(2, 3)", &module_stages)
            .expect("qualified builtin should resolve");
        let bind = resolved
            .iter()
            .find(|node| matches!(node, Resolved::Bind(_, _, _)))
            .expect("expected bind in resolved output");
        match bind {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::App(_, func, _) => match func.as_ref() {
                    Resolved::Var(_, id) => {
                        assert_eq!(id.name, "Int::shl");
                        assert_eq!(id.qualified_name.as_deref(), Some("Int::shl"));
                    }
                    _ => panic!("Expected builtin var"),
                },
                _ => panic!("Expected app"),
            },
            _ => panic!("Expected bind"),
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
            Resolved::Def(_, id, params, ret_ty, body, attrs) => {
                assert_eq!(id.name, "add");
                assert_eq!(params.len(), 2);
                assert_eq!(attrs, &ResolvedDeclAttrs::default());
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
    fn test_assert_conversion() {
        let resolved = parse_and_resolve(
            r#"deferror SomeError { "boom" }
x = assert(True, SomeError)"#,
        )
        .unwrap();
        match &resolved[1] {
            Resolved::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Resolved::Assert(_, _, _)));
            }
            _ => panic!("Expected Bind with Assert"),
        }
    }

    #[test]
    fn test_ensure_conversion() {
        let resolved = parse_and_resolve(
            r#"def is_even(n: Int) -> Boolean { True }
deferror SomeError { "boom" }
x = ensure(1, &is_even, SomeError)"#,
        )
        .unwrap();
        match &resolved[2] {
            Resolved::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Resolved::Ensure(_, _, _, _)));
            }
            _ => panic!("Expected Bind with Ensure"),
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
            Resolved::Def(_, id, _, _, _, _) => id.unique_id,
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
                    Resolved::Def(_, id, _, _, _, _)
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
                    assert!(matches!(&arms[0].0, ResolvedPattern::StrLit(_, s) if s == "a"));
                    assert!(matches!(&arms[1].0, ResolvedPattern::IntLit(_, n) if n == &int(2)));
                    assert!(matches!(&arms[2].0, ResolvedPattern::Wildcard(_)));
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
                assert!(matches!(inner.as_slice(), [ResolvedPattern::Var(id)] if id.name == "num"));
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
                        && matches!(inner.as_slice(), [ResolvedPattern::IntLit(_, n)] if n == &int(1))
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
    fn test_closure_param_annotations_are_preserved() {
        let resolved = parse_and_resolve(r#"f = {|x: Int, y| x}"#).unwrap();
        match &resolved[0] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Closure(_, params, captures, _) => {
                    assert!(captures.is_empty());
                    assert_eq!(params.len(), 2);
                    assert!(matches!(
                        params[0].ty.as_ref(),
                        Some(AstTy::Named(_, name)) if name == "Int"
                    ));
                    assert_eq!(params[1].ty, None);
                }
                _ => panic!("Expected Closure"),
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
        let module_stages = vec![vec![staged_module(
            "Helper",
            parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
        )]];

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
        let module_stages = vec![vec![staged_module(
            "Helper",
            parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
        )]];

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
        let module_stages = vec![vec![staged_module(
            "Helper",
            parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
        )]];

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
        let module_stages = vec![vec![staged_module(
            "Helper",
            parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
        )]];

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
            vec![staged_module(
                "Kernel",
                parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Kernel"),
            )],
            vec![staged_module(
                "Helper",
                parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x - y }"#, "Helper"),
            )],
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
                Resolved::Def(_, id, _, _, _, _)
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

    // --- SigilSession tests ---

    #[test]
    fn test_sigil_session_basic_resolve() {
        let mut session = SigilSession::new();
        let ast = spire::parse("x = 1").expect("parse failed");
        let resolved = session.resolve(ast).expect("resolve failed");
        assert_eq!(resolved.len(), 1);
        assert!(
            matches!(&resolved[0], Resolved::Bind(_, ResolvedPattern::Var(id), _) if id.name == "x")
        );
    }

    #[test]
    fn test_sigil_session_scope_persists_across_calls() {
        let mut session = SigilSession::new();

        let ast1 = spire::parse("x = 1").expect("parse failed");
        session.resolve(ast1).expect("first resolve failed");

        // x must be in scope for the second call
        let ast2 = spire::parse("y = x + 1").expect("parse failed");
        let resolved = session.resolve(ast2).expect("second resolve failed");
        assert!(
            matches!(&resolved[0], Resolved::Bind(_, ResolvedPattern::Var(id), _) if id.name == "y")
        );
    }

    #[test]
    fn test_sigil_session_lookup_uid_returns_bound_id() {
        let mut session = SigilSession::new();
        let ast = spire::parse("answer = 42").expect("parse failed");
        let resolved = session.resolve(ast).expect("resolve failed");

        let expected_id = match &resolved[0] {
            Resolved::Bind(_, ResolvedPattern::Var(id), _) => id.unique_id,
            _ => panic!("Expected Bind"),
        };

        assert_eq!(session.lookup_uid("answer"), Some(expected_id));
    }

    #[test]
    fn test_sigil_session_checkpoint_rollback_removes_later_bindings() {
        let mut session = SigilSession::new();

        // Define x
        let ast1 = spire::parse("x = 1").expect("parse failed");
        session.resolve(ast1).expect("first resolve failed");
        let x_id = session
            .lookup_uid("x")
            .expect("x should be defined after first resolve");

        // Save checkpoint before defining y
        let checkpoint = session.checkpoint();

        // Define y
        let ast2 = spire::parse("y = 2").expect("parse failed");
        session.resolve(ast2).expect("second resolve failed");
        assert!(
            session.lookup_uid("y").is_some(),
            "y should be visible before rollback"
        );

        // Rollback to before y was added
        session.rollback(checkpoint);

        assert!(
            session.lookup_uid("y").is_none(),
            "y should be gone after rollback"
        );
        assert_eq!(
            session.lookup_uid("x"),
            Some(x_id),
            "x should remain after rollback"
        );
    }

    #[test]
    fn test_sigil_session_failed_resolve_does_not_pollute_scope() {
        let mut session = SigilSession::new();

        // Define x
        let ast1 = spire::parse("x = 1").expect("parse failed");
        session.resolve(ast1).expect("first resolve failed");
        let x_id = session.lookup_uid("x").expect("x should be defined");

        // Attempt to resolve something with an undefined variable — must fail
        let ast_fail = spire::parse("y = undefined_name + 1").expect("parse failed");
        assert!(
            session.resolve(ast_fail).is_err(),
            "resolve of undefined var must fail"
        );

        // x should survive; y must not be committed to scope
        assert_eq!(
            session.lookup_uid("x"),
            Some(x_id),
            "x should remain after failed resolve"
        );
        assert!(
            session.lookup_uid("y").is_none(),
            "y must not be in scope after a failed resolve"
        );
    }

    // --- Expression resolution tests ---

    #[test]
    fn test_interpolated_string_resolves_embedded_variable() {
        let resolved = parse_and_resolve(
            r#"name = "alice"
greeting = "Hello #{name}!""#,
        )
        .unwrap();

        match &resolved[1] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::InterpolatedStr(_, parts) => {
                    let has_text = parts.iter().any(
                        |p| matches!(p, ResolvedInterpolatedPart::Text(s) if s.contains("Hello")),
                    );
                    let has_name_var = parts.iter().any(|p| {
                        matches!(p, ResolvedInterpolatedPart::Expr(e)
                            if matches!(e.as_ref(), Resolved::Var(_, id) if id.name == "name"))
                    });
                    assert!(
                        has_text,
                        "expected 'Hello' text part in interpolated string"
                    );
                    assert!(
                        has_name_var,
                        "expected resolved `name` variable in interpolated string"
                    );
                }
                _ => panic!("Expected InterpolatedStr, got {:?}", rhs),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_field_access_resolves_correct_target() {
        let resolved = parse_and_resolve(
            r#"defstruct Point { x: Int, y: Int }
p = Point { x: 1, y: 2 }
val = p.x"#,
        )
        .unwrap();

        match &resolved[2] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::FieldAccess(_, expr, field) => {
                    assert_eq!(field, "x");
                    assert!(
                        matches!(expr.as_ref(), Resolved::Var(_, id) if id.name == "p"),
                        "field access target should be `p`"
                    );
                }
                _ => panic!("Expected FieldAccess"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_list_literal_resolves_all_elements() {
        let resolved = parse_and_resolve("items = [1, 2, 3]").unwrap();
        match &resolved[0] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::ListLiteral(_, elems) => {
                    assert_eq!(elems.len(), 3);
                    assert!(matches!(&elems[0], Resolved::Lit(_, Lit::Int(n)) if n == &int(1)));
                    assert!(matches!(&elems[1], Resolved::Lit(_, Lit::Int(n)) if n == &int(2)));
                    assert!(matches!(&elems[2], Resolved::Lit(_, Lit::Int(n)) if n == &int(3)));
                }
                _ => panic!("Expected ListLiteral"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_semicolon_expression_wraps_inner_node() {
        let resolved = parse_and_resolve(r#"print("hello");"#).unwrap();
        match &resolved[0] {
            Resolved::Semi(_, inner) => match inner.as_ref() {
                Resolved::App(_, func, _) => match func.as_ref() {
                    Resolved::Var(_, id) => assert_eq!(id.name, "print"),
                    _ => panic!("Expected Var(print) inside Semi"),
                },
                _ => panic!("Expected App inside Semi"),
            },
            _ => panic!("Expected Semi at top level"),
        }
    }

    // --- Import error tests ---

    #[test]
    fn test_unknown_import_member_is_error() {
        let module_stages = vec![vec![staged_module(
            "Helper",
            parse_module_ast(r#"def add(x: Int, y: Int) -> Int { x + y }"#, "Helper"),
        )]];

        let err = resolve_user_with_modules(
            r#"import Helper::nonexistent;
print("ok")"#,
            &module_stages,
        )
        .expect_err("importing a non-existent member must fail");

        assert!(
            err.message.contains("Unknown import member"),
            "actual error: {}",
            err.message
        );
        assert!(
            err.message.contains("Helper::nonexistent"),
            "actual error: {}",
            err.message
        );
    }

    // --- Match arm binding tests ---

    #[test]
    fn test_match_arm_constructor_binding_resolves_to_same_uid_in_body() {
        let resolved = parse_and_resolve(
            r#"value: Result<Int> = Ok(42)
result = match value {
  Ok(x) => x,
  Err(e) => 0,
}"#,
        )
        .unwrap();

        match &resolved[1] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Match(_, _, arms) => {
                    match &arms[0] {
                        (ResolvedPattern::Constructor(ctor_id, inner), body) => {
                            assert_eq!(ctor_id.name, "Ok");
                            let binding_id = match inner.as_slice() {
                                [ResolvedPattern::Var(binding_id)] => binding_id,
                                _ => panic!("Expected constructor inner var binding"),
                            };
                            assert_eq!(binding_id.name, "x");
                            // The arm body `x` must refer to the same uid as the pattern binding
                            match body {
                                Resolved::Var(_, var_id) => {
                                    assert_eq!(
                                        var_id.unique_id, binding_id.unique_id,
                                        "body var uid must match pattern binding uid"
                                    );
                                }
                                _ => panic!("Expected Var as match arm body"),
                            }
                        }
                        _ => panic!("Expected Constructor arm pattern with binding"),
                    }
                }
                _ => panic!("Expected Match"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_match_first_binding_pattern_binds_and_is_visible_in_body() {
        let resolved = parse_and_resolve(
            r#"result = match 42 {
  fallback => fallback,
  _ => 0,
}"#,
        )
        .unwrap();

        match &resolved[0] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Match(_, _, arms) => match &arms[0] {
                    (ResolvedPattern::Var(binding_id), Resolved::Var(_, body_id)) => {
                        assert_eq!(binding_id.name, "fallback");
                        assert_eq!(binding_id.unique_id, body_id.unique_id);
                    }
                    _ => panic!("Expected first arm to be a binding pattern"),
                },
                _ => panic!("Expected Match"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_match_as_pattern_and_annotation_resolve_end_to_end() {
        let resolved = parse_and_resolve(
            r#"value = [1, 2]
result = match value {
  [head, ..tail] @ whole: List<Int> => head,
  _ => 0,
}"#,
        )
        .unwrap();

        match &resolved[1] {
            Resolved::Bind(_, _, rhs) => match rhs.as_ref() {
                Resolved::Match(_, _, arms) => match &arms[0] {
                    (
                        ResolvedPattern::As(
                            inner,
                            alias,
                            Some(AstTy::Generic(_, ty_name, ty_args)),
                        ),
                        body,
                    ) => {
                        assert_eq!(alias.name, "whole");
                        assert_eq!(ty_name, "List");
                        assert_eq!(ty_args.len(), 1);
                        assert!(matches!(inner.as_ref(), ResolvedPattern::ListCons(_, _)));
                        assert!(matches!(body, Resolved::Var(_, id) if id.name == "head"));
                    }
                    _ => panic!("Expected as-pattern with generic annotation"),
                },
                _ => panic!("Expected Match"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    // --- build_scope_for_module tests ---

    #[test]
    fn test_build_scope_for_module_includes_prior_stage_declarations() {
        let module_stages = vec![
            vec![staged_module(
                "Util",
                parse_module_ast(r#"def helper(x: Int) -> Int { x }"#, "Util"),
            )],
            vec![staged_module(
                "App",
                parse_module_ast(r#"def main() -> Int { 0 }"#, "App"),
            )],
        ];

        // Stage index 1 (App) — Util::helper from stage 0 should appear by fully-qualified name
        let scope = build_scope_for_module(&module_stages, Some("App"), 1)
            .expect("build_scope_for_module should succeed");

        assert!(
            scope.lookup("Util::helper").is_some(),
            "Util::helper should be accessible by qualified name in App's scope"
        );
    }
}
