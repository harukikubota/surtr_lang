use super::scope_init::initialize_scope;
use super::scope_init::is_doc_only_builtin_decl;
use super::*;
use sindr::builtin::builtin_type_supports_inherent_impl;
use sindr::names::{
    reserved_owner_surface_name_constraint, surface_path_name, ReservedOwnerSurfaceNameKind,
};
use spire::ast::FacetPathSegment;

use serde::{Deserialize, Serialize};

fn reserved_owner_name_error(
    owner_kind: &str,
    name: &str,
    span: &Span,
    allow_canonical_builtin_type: bool,
) -> Option<ResolveError> {
    let constraint = reserved_owner_surface_name_constraint(name)?;
    if allow_canonical_builtin_type
        && matches!(
            constraint.kind,
            ReservedOwnerSurfaceNameKind::CanonicalBuiltinType
        )
    {
        return None;
    }

    Some(ResolveError {
        message: format!(
            "{} `{}` is {}",
            owner_kind,
            constraint.surface_name,
            constraint.kind.diagnostic_suffix()
        ),
        span: span.clone(),
        related_labels: Vec::new(),
    })
}

fn reject_reserved_owner_name(
    owner_kind: &str,
    name: &str,
    span: &Span,
    allow_canonical_builtin_type: bool,
) -> Result<(), ResolveError> {
    if let Some(err) =
        reserved_owner_name_error(owner_kind, name, span, allow_canonical_builtin_type)
    {
        return Err(err);
    }
    Ok(())
}

fn builtin_special_enum_surface_name(name: &str) -> bool {
    matches!(global_surface_name(name), "Result" | "Boolean")
}

fn builtin_special_enum_variant_alias(enum_name: &str, variant_name: &str) -> bool {
    matches!(
        (global_surface_name(enum_name), variant_name),
        ("Result", "Ok" | "Err") | ("Boolean", "True" | "False")
    )
}

fn type_declaration_kind(stmt: &Ast) -> Option<DeclarationKind> {
    match stmt {
        Ast::StructDef(..) => Some(DeclarationKind::Struct),
        Ast::RecordDef(..) => Some(DeclarationKind::Record),
        Ast::DeferrorDef(..) => Some(DeclarationKind::Deferror),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StagedModuleAst {
    pub module_path: String,
    pub doc_module_path: Option<String>,
    pub ast: Vec<Ast>,
    pub module_doc: Option<String>,
    pub auto_import: bool,
    pub process_spec: Option<spire::ast::ProcessSpec>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoweredModuleAst {
    pub module_path: String,
    pub doc_module_path: Option<String>,
    pub ast: Vec<Ast>,
    pub declared_span: Option<Span>,
    pub module_doc: Option<String>,
    pub auto_import: bool,
    pub process_spec: Option<spire::ast::ProcessSpec>,
}

impl From<LoweredModuleAst> for StagedModuleAst {
    fn from(lowered: LoweredModuleAst) -> Self {
        Self {
            module_path: lowered.module_path,
            doc_module_path: lowered.doc_module_path,
            ast: lowered.ast,
            module_doc: lowered.module_doc,
            auto_import: lowered.auto_import,
            process_spec: lowered.process_spec,
        }
    }
}

pub fn lower_module_source_ast(
    ast: Vec<Ast>,
    fallback_module_path: Option<&str>,
) -> Vec<LoweredModuleAst> {
    let shared_imports = ast
        .iter()
        .filter_map(|stmt| match stmt {
            Ast::Import(_, _, _) => Some(stmt.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut lowered = Vec::new();
    let mut shared_global_defs = Vec::new();
    let mut shared_namespace_consts = Vec::new();
    let mut shared_result_ctor_contracts = Vec::new();

    for stmt in ast {
        match stmt {
            Ast::Defmod(span, module_path, body, attrs) => {
                let mut module_ast = shared_imports.clone();
                module_ast.extend(body);
                lowered.push(LoweredModuleAst {
                    module_path,
                    doc_module_path: None,
                    ast: module_ast,
                    declared_span: Some(span),
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                    process_spec: None,
                });
            }
            Ast::Defagent(span, module_path, body, process_spec, attrs)
            | Ast::Defgenserver(span, module_path, body, process_spec, attrs)
            | Ast::Defsupervisor(span, module_path, body, process_spec, attrs)
            | Ast::DefdynamicSupervisor(span, module_path, body, process_spec, attrs) => {
                let mut module_ast = shared_imports.clone();
                module_ast.extend(body);
                lowered.push(LoweredModuleAst {
                    module_path,
                    doc_module_path: None,
                    ast: module_ast,
                    declared_span: Some(span),
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                    process_spec: Some(process_spec),
                });
            }
            Ast::ImplDef(span, target, methods, attrs) => {
                let declared_span = span.clone();
                let module_path = target.clone();
                let mut module_ast = shared_imports.clone();
                let (local_imports, methods) = partition_nested_imports(methods);
                module_ast.extend(local_imports);
                module_ast.push(Ast::ImplDef(span, target, methods, attrs.clone()));
                lowered.push(LoweredModuleAst {
                    module_path,
                    doc_module_path: None,
                    ast: module_ast,
                    declared_span: Some(declared_span),
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                    process_spec: None,
                });
            }
            Ast::TraitImplDef(
                span,
                trait_name,
                trait_args,
                target_ty,
                where_clause,
                methods,
                attrs,
            ) => {
                let declared_span = span.clone();
                let module_path = match &target_ty {
                    AstTy::Named(_, name)
                    | AstTy::ImplTrait(_, name)
                    | AstTy::Generic(_, name, _) => name.clone(),
                    _ => fallback_module_path.unwrap_or_default().to_string(),
                };
                let mut module_ast = shared_imports.clone();
                let (local_imports, methods) = partition_nested_imports(methods);
                module_ast.extend(local_imports);
                module_ast.push(Ast::TraitImplDef(
                    span,
                    trait_name,
                    trait_args,
                    target_ty,
                    where_clause,
                    methods,
                    attrs.clone(),
                ));
                lowered.push(LoweredModuleAst {
                    module_path,
                    doc_module_path: fallback_module_path.map(str::to_string),
                    ast: module_ast,
                    declared_span: Some(declared_span),
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                    process_spec: None,
                });
            }
            Ast::Import(_, _, _) => {}
            Ast::ResultCtorDecl(_, _, _, _, _) => {
                shared_result_ctor_contracts.push(stmt);
            }
            Ast::ConstDef(_, _, _, _, _) => {
                shared_namespace_consts.push(stmt);
            }
            Ast::StructDef(..)
            | Ast::RecordDef(..)
            | Ast::DeferrorDef(_, _, _, _, _)
            | Ast::EnumDef(_, _, _, _, _)
            | Ast::BuiltinDecl(_, _, _, _, _)
            | Ast::IntrinsicDecl(_, _, _, _)
            | Ast::BuiltinTypeDecl(_, _, _) => {
                shared_global_defs.push(stmt);
            }
            _ => {
                shared_global_defs.push(stmt);
            }
        }
    }

    if !shared_namespace_consts.is_empty() {
        if let Some(idx) = find_fallback_namespace_module(&lowered, fallback_module_path)
            .or_else(|| (lowered.len() == 1).then_some(0))
        {
            let insert_at = first_non_import_index(&lowered[idx].ast);
            lowered[idx]
                .ast
                .splice(insert_at..insert_at, shared_namespace_consts);
        } else {
            let mut shared_ast = shared_imports.clone();
            shared_ast.extend(shared_namespace_consts);
            lowered.push(LoweredModuleAst {
                module_path: fallback_module_path.unwrap_or_default().to_string(),
                doc_module_path: None,
                ast: shared_ast,
                declared_span: None,
                module_doc: None,
                auto_import: false,
                process_spec: None,
            });
        }
    }

    if !shared_result_ctor_contracts.is_empty() {
        if let Some(idx) =
            find_result_owner_module(&lowered).or_else(|| (lowered.len() == 1).then_some(0))
        {
            let insert_at = first_non_import_index(&lowered[idx].ast);
            lowered[idx]
                .ast
                .splice(insert_at..insert_at, shared_result_ctor_contracts);
        } else {
            let mut shared_ast = shared_imports.clone();
            shared_ast.extend(shared_result_ctor_contracts);
            lowered.push(LoweredModuleAst {
                module_path: fallback_module_path.unwrap_or_default().to_string(),
                doc_module_path: None,
                ast: shared_ast,
                declared_span: None,
                module_doc: None,
                auto_import: false,
                process_spec: None,
            });
        }
    }

    if !shared_global_defs.is_empty() {
        let mut shared_ast = shared_imports;
        shared_ast.extend(shared_global_defs);
        lowered.push(LoweredModuleAst {
            module_path: fallback_module_path.unwrap_or_default().to_string(),
            doc_module_path: None,
            ast: shared_ast,
            declared_span: None,
            module_doc: None,
            auto_import: false,
            process_spec: None,
        });
    }

    lowered
}

pub fn staged_modules_from_source_ast(
    ast: Vec<Ast>,
    fallback_module_path: Option<&str>,
) -> Vec<StagedModuleAst> {
    lower_module_source_ast(ast, fallback_module_path)
        .into_iter()
        .map(StagedModuleAst::from)
        .collect()
}

pub fn extract_process_modules_from_user_ast(ast: Vec<Ast>) -> (Vec<StagedModuleAst>, Vec<Ast>) {
    let shared_imports = ast
        .iter()
        .filter_map(|stmt| match stmt {
            Ast::Import(_, _, _) => Some(stmt.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut process_modules = Vec::new();
    let mut remaining_ast = Vec::new();

    for stmt in ast {
        match stmt {
            Ast::Defagent(_, module_path, body, process_spec, attrs)
            | Ast::Defgenserver(_, module_path, body, process_spec, attrs)
            | Ast::Defsupervisor(_, module_path, body, process_spec, attrs)
            | Ast::DefdynamicSupervisor(_, module_path, body, process_spec, attrs) => {
                let mut module_ast = shared_imports.clone();
                module_ast.extend(body);
                process_modules.push(StagedModuleAst {
                    module_path,
                    doc_module_path: None,
                    ast: module_ast,
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                    process_spec: Some(process_spec),
                });
            }
            other => remaining_ast.push(other),
        }
    }

    (process_modules, remaining_ast)
}

pub fn const_only_fallback_module_path<'a>(
    ast: &[Ast],
    fallback_module_path: Option<&'a str>,
) -> Option<&'a str> {
    let has_const = ast
        .iter()
        .any(|stmt| matches!(stmt, Ast::ConstDef(_, _, _, _, _)));
    let const_only = ast
        .iter()
        .all(|stmt| matches!(stmt, Ast::Import(_, _, _) | Ast::ConstDef(_, _, _, _, _)));
    (has_const && const_only)
        .then_some(fallback_module_path)
        .flatten()
}

pub fn lowered_module_is_impl_owner(lowered: &LoweredModuleAst) -> bool {
    matches!(
        lowered
            .ast
            .iter()
            .find(|stmt| !matches!(stmt, Ast::Import(_, _, _))),
        Some(Ast::ImplDef(_, _, _, _) | Ast::TraitImplDef(..))
    )
}

fn staged_module_is_impl_owner(module: &StagedModuleAst) -> bool {
    matches!(
        module
            .ast
            .iter()
            .find(|stmt| !matches!(stmt, Ast::Import(_, _, _))),
        Some(Ast::ImplDef(_, _, _, _) | Ast::TraitImplDef(..))
    )
}

fn module_owner_fallback_span(module: &StagedModuleAst) -> Span {
    module
        .ast
        .iter()
        .find(|stmt| !matches!(stmt, Ast::Import(_, _, _)))
        .map(Ast::span)
        .cloned()
        .unwrap_or(Span { start: 0, end: 0 })
}

fn partition_nested_imports(body: Vec<Ast>) -> (Vec<Ast>, Vec<Ast>) {
    let mut imports = Vec::new();
    let mut rest = Vec::new();
    for stmt in body {
        if matches!(stmt, Ast::Import(_, _, _)) {
            imports.push(stmt);
        } else {
            rest.push(stmt);
        }
    }
    (imports, rest)
}

fn first_non_import_index(ast: &[Ast]) -> usize {
    ast.iter()
        .take_while(|stmt| matches!(stmt, Ast::Import(_, _, _)))
        .count()
}

fn find_result_owner_module(lowered: &[LoweredModuleAst]) -> Option<usize> {
    lowered.iter().position(|module| {
        surface_path_name(&module.module_path) == "Result"
            && matches!(
                module
                    .ast
                    .iter()
                    .find(|stmt| !matches!(stmt, Ast::Import(_, _, _))),
                Some(Ast::ImplDef(_, target, _, _)) if surface_path_name(target) == "Result"
            )
    })
}

fn find_fallback_namespace_module(
    lowered: &[LoweredModuleAst],
    fallback_module_path: Option<&str>,
) -> Option<usize> {
    let fallback = fallback_module_path?;
    lowered
        .iter()
        .position(|module| module.module_path == fallback && !lowered_module_is_impl_owner(module))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeclarationKind {
    Def,
    Extractor,
    Trait,
    TraitMethod,
    Struct,
    Record,
    Deferror,
    Enum,
    EnumVariant,
    Const,
    ResultCtor,
    ImplMethod,
    ImplCtorNew,
    BuiltinType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeclarationEntry {
    pub module_path: String,
    pub name: String,
    pub fq_name: String,
    pub kind: DeclarationKind,
    pub stage_index: usize,
    pub auto_import: bool,
    pub hidden: bool,
    pub visibility: Visibility,
    pub user_importable: bool,
    pub user_callable: bool,
}

pub type DeclarationIndex = BTreeMap<String, DeclarationEntry>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageOrderedDeclaration {
    pub stage_index: usize,
    pub fq_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclarationOrdering {
    entries: Vec<StageOrderedDeclaration>,
}

impl DeclarationOrdering {
    pub fn entries(&self) -> &[StageOrderedDeclaration] {
        &self.entries
    }

    pub fn fq_names(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| entry.fq_name.clone())
            .collect()
    }
}

fn duplicate_fq_declaration_error(
    fq_name: &str,
    prev: &DeclarationEntry,
    span: &Span,
) -> ResolveError {
    ResolveError {
        message: format!(
            "Duplicate fully-qualified declaration: {} (already declared in stage {} module {})",
            global_surface_name(fq_name),
            prev.stage_index,
            prev.module_path
        ),
        span: span.clone(),
        related_labels: Vec::new(),
    }
}

fn declaration_entry(
    module_path: impl Into<String>,
    name: impl Into<String>,
    fq_name: impl Into<String>,
    kind: DeclarationKind,
    stage_index: usize,
    auto_import: bool,
    hidden: bool,
    visibility: Visibility,
    user_importable: bool,
    user_callable: bool,
) -> DeclarationEntry {
    DeclarationEntry {
        module_path: module_path.into(),
        name: name.into(),
        fq_name: fq_name.into(),
        kind,
        stage_index,
        auto_import,
        hidden,
        visibility,
        user_importable,
        user_callable,
    }
}

fn insert_declaration_entry(
    index: &mut DeclarationIndex,
    entry: DeclarationEntry,
    span: &Span,
) -> Result<(), ResolveError> {
    if let Some(prev) = index.get(&entry.fq_name) {
        return Err(duplicate_fq_declaration_error(&entry.fq_name, prev, span));
    }

    index.insert(entry.fq_name.clone(), entry);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ImplTargetResolution {
    Unique(DeclarationKind),
    Ambiguous,
}

pub(super) fn is_module_visible_declaration(kind: &DeclarationKind) -> bool {
    !matches!(kind, DeclarationKind::BuiltinType)
}

pub(super) fn is_importable_declaration(kind: &DeclarationKind) -> bool {
    !matches!(
        kind,
        DeclarationKind::BuiltinType
            | DeclarationKind::ImplCtorNew
            | DeclarationKind::Struct
            | DeclarationKind::Const
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ImportSurfaceStatus {
    Importable,
    NonImportableKind,
    Restricted,
    Hidden,
    Private,
    FutureStage,
}

pub(super) fn declaration_import_surface_status(
    entry: &DeclarationEntry,
    current_stage_index: usize,
) -> ImportSurfaceStatus {
    if !is_importable_declaration(&entry.kind) {
        return ImportSurfaceStatus::NonImportableKind;
    }
    if !entry.user_importable {
        return ImportSurfaceStatus::Restricted;
    }
    if entry.hidden {
        return ImportSurfaceStatus::Hidden;
    }
    if entry.visibility != Visibility::Public {
        return ImportSurfaceStatus::Private;
    }
    if entry.stage_index > current_stage_index {
        return ImportSurfaceStatus::FutureStage;
    }
    ImportSurfaceStatus::Importable
}

fn entry_visibility(attrs: &DeclAttrs) -> Visibility {
    attrs.visibility
}

fn entry_user_importable(attrs: &DeclAttrs) -> bool {
    attrs.user_importable
}

fn entry_user_callable(attrs: &DeclAttrs) -> bool {
    attrs.user_callable
}

#[cfg(test)]
mod declaration_surface_tests {
    use super::*;

    fn entry(
        kind: DeclarationKind,
        stage_index: usize,
        user_importable: bool,
        hidden: bool,
        visibility: Visibility,
    ) -> DeclarationEntry {
        declaration_entry(
            "Test",
            "name",
            "Test::name",
            kind,
            stage_index,
            false,
            hidden,
            visibility,
            user_importable,
            true,
        )
    }

    #[test]
    fn declaration_import_surface_status_classifies_effective_user_import_policy() {
        assert_eq!(
            declaration_import_surface_status(
                &entry(DeclarationKind::Struct, 0, true, false, Visibility::Public),
                0
            ),
            ImportSurfaceStatus::NonImportableKind
        );
        assert_eq!(
            declaration_import_surface_status(
                &entry(DeclarationKind::Def, 0, false, false, Visibility::Public),
                0
            ),
            ImportSurfaceStatus::Restricted
        );
        assert_eq!(
            declaration_import_surface_status(
                &entry(DeclarationKind::Def, 0, true, true, Visibility::Public),
                0
            ),
            ImportSurfaceStatus::Hidden
        );
        assert_eq!(
            declaration_import_surface_status(
                &entry(DeclarationKind::Def, 0, true, false, Visibility::Private),
                0
            ),
            ImportSurfaceStatus::Private
        );
        assert_eq!(
            declaration_import_surface_status(
                &entry(DeclarationKind::Def, 1, true, false, Visibility::Public),
                0
            ),
            ImportSurfaceStatus::FutureStage
        );
        assert_eq!(
            declaration_import_surface_status(
                &entry(DeclarationKind::Def, 0, true, false, Visibility::Public),
                0
            ),
            ImportSurfaceStatus::Importable
        );
    }
}

fn normalize_impl_method_name(target: &str, method_name: &str) -> String {
    format!("{}::{}", target, method_name)
}

fn impl_owner_module_path(target: &str) -> String {
    target.to_string()
}

fn lower_impl_member_name(
    current_module_path: Option<&str>,
    target: &str,
    method_name: &str,
) -> String {
    if current_module_path
        .map(surface_path_name)
        .is_some_and(|module_path| module_path == surface_path_name(target))
    {
        method_name.to_string()
    } else {
        normalize_impl_method_name(target, method_name)
    }
}

const TYPE_DECL_ENTRY_MODULE_PATH: &str = "";

pub(super) fn trait_method_qualified_name(trait_name: &str, method_name: &str) -> String {
    format!("{}::{}", trait_name, method_name)
}

pub(super) fn trait_instance_key(trait_name: &str, trait_args: &[AstTy]) -> String {
    if trait_args.is_empty() {
        trait_name.to_string()
    } else {
        format!(
            "{}<{}>",
            trait_name,
            trait_args
                .iter()
                .map(ast_ty_key)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

pub(super) fn ast_ty_key(ty: &AstTy) -> String {
    match ty {
        AstTy::Named(_, name) | AstTy::ImplTrait(_, name) => name.clone(),
        AstTy::Generic(_, name, args) => format!(
            "{}<{}>",
            name,
            args.iter().map(ast_ty_key).collect::<Vec<_>>().join(", ")
        ),
        AstTy::Tuple(_, items) if items.len() >= 2 => format!("Tuple{}", items.len()),
        AstTy::Tuple(_, items) => format!(
            "({})",
            items.iter().map(ast_ty_key).collect::<Vec<_>>().join(", ")
        ),
        AstTy::Func(_, params, ret) => format!(
            "({} -> {})",
            params.iter().map(ast_ty_key).collect::<Vec<_>>().join(", "),
            ast_ty_key(ret)
        ),
    }
}

pub(super) fn trait_impl_method_qualified_name(
    module_path: Option<&str>,
    trait_name: &str,
    trait_args: &[AstTy],
    target: &AstTy,
    method_name: &str,
    span_start: usize,
) -> String {
    let private_module = match module_path {
        Some(module_path) if !module_path.is_empty() => format!("{}::__traitimpl__", module_path),
        _ => "__traitimpl__".to_string(),
    };
    format!(
        "{}::{}::{}::{}::{}",
        private_module,
        trait_instance_key(trait_name, trait_args),
        ast_ty_key(target),
        method_name,
        span_start
    )
}

pub(super) fn collect_stage_impl_target_resolutions(
    stage: &[StagedModuleAst],
) -> HashMap<String, ImplTargetResolution> {
    let mut resolutions = HashMap::new();
    for module in stage {
        for stmt in &module.ast {
            let (name, kind) = match stmt {
                Ast::StructDef(_, name, ..) => (name, DeclarationKind::Struct),
                Ast::EnumDef(_, name, _, _, _) => (name, DeclarationKind::Enum),
                Ast::RecordDef(_, name, _, _) => (name, DeclarationKind::Record),
                Ast::DeferrorDef(_, name, _, _, _) => (name, DeclarationKind::Deferror),
                _ => continue,
            };
            match resolutions.get(name) {
                None => {
                    resolutions.insert(name.clone(), ImplTargetResolution::Unique(kind.clone()));
                    let surface_name = global_surface_name(name);
                    if surface_name != name {
                        resolutions
                            .insert(surface_name.to_string(), ImplTargetResolution::Unique(kind));
                    }
                }
                Some(ImplTargetResolution::Unique(_)) | Some(ImplTargetResolution::Ambiguous) => {
                    resolutions.insert(name.clone(), ImplTargetResolution::Ambiguous);
                    let surface_name = global_surface_name(name);
                    if surface_name != name {
                        resolutions
                            .insert(surface_name.to_string(), ImplTargetResolution::Ambiguous);
                    }
                }
            }
        }
    }
    resolutions
}

fn resolve_impl_target_kind(
    target: &str,
    span: &Span,
    targets: &HashMap<String, ImplTargetResolution>,
) -> Result<DeclarationKind, ResolveError> {
    match targets.get(target) {
        Some(ImplTargetResolution::Unique(kind)) => Ok(kind.clone()),
        Some(ImplTargetResolution::Ambiguous) => Err(ResolveError {
            message: format!(
                "impl target `{}` is ambiguous within the current stage",
                target
            ),
            span: span.clone(),
            related_labels: Vec::new(),
        }),
        None => {
            let builtin_target = global_surface_name(target);
            if builtin_type_supports_inherent_impl(builtin_target) {
                Ok(DeclarationKind::BuiltinType)
            } else {
                Err(ResolveError {
                    message: format!(
                        "impl target `{}` must be a standard type owner or a struct/enum defined in the current stage",
                        target
                    ),
                    span: span.clone(),
                    related_labels: Vec::new(),
                })
            }
        }
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
        AstTy::ImplTrait(span, name) => AstTy::ImplTrait(span, name),
        AstTy::Generic(span, name, args) => AstTy::Generic(
            span,
            name,
            args.into_iter()
                .map(|arg| rewrite_self_type(arg, target))
                .collect(),
        ),
        AstTy::Tuple(span, items) => AstTy::Tuple(
            span,
            items
                .into_iter()
                .map(|item| rewrite_self_type(item, target))
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

fn rewrite_self_where_clause(
    clause: spire::ast::WhereClause,
    target: &str,
) -> spire::ast::WhereClause {
    spire::ast::WhereClause {
        constraints: clause
            .constraints
            .into_iter()
            .map(|constraint| spire::ast::WhereConstraint {
                subject: rewrite_self_type(constraint.subject, target),
                bounds: constraint
                    .bounds
                    .into_iter()
                    .map(|bound| match bound {
                        spire::ast::WhereConstraintRhs::Trait(span, name) => {
                            spire::ast::WhereConstraintRhs::Trait(span, name)
                        }
                        spire::ast::WhereConstraintRhs::TypeConstructor(span, slots) => {
                            spire::ast::WhereConstraintRhs::TypeConstructor(
                                span,
                                slots
                                    .into_iter()
                                    .map(|slot| rewrite_self_type(slot, target))
                                    .collect(),
                            )
                        }
                        spire::ast::WhereConstraintRhs::TraitSlot(span, owner, slot) => {
                            spire::ast::WhereConstraintRhs::TraitSlot(span, owner, slot)
                        }
                    })
                    .collect(),
                span: constraint.span,
            })
            .collect(),
        span: clause.span,
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
        AstPattern::Call(span, name, inners) => AstPattern::Call(
            span,
            name,
            inners
                .into_iter()
                .map(|inner| rewrite_self_pattern(inner, target))
                .collect(),
        ),
        AstPattern::Tuple(span, items) => AstPattern::Tuple(
            span,
            items
                .into_iter()
                .map(|item| rewrite_self_pattern(item, target))
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
        Ast::HashMapLiteral(span, entries) => Ast::HashMapLiteral(
            span,
            entries
                .into_iter()
                .map(|entry| spire::ast::HashMapLiteralEntry {
                    key: rewrite_self_ast(entry.key, target),
                    value: rewrite_self_ast(entry.value, target),
                })
                .collect(),
        ),
        Ast::RangeLiteral(span, start, stop) => Ast::RangeLiteral(
            span,
            Box::new(rewrite_self_ast(*start, target)),
            Box::new(rewrite_self_ast(*stop, target)),
        ),
        Ast::TupleLiteral(span, elems) => Ast::TupleLiteral(
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
                .map(|arm| AstMatchArm {
                    pattern: rewrite_self_pattern(arm.pattern, target),
                    guard: arm.guard.map(|guard| rewrite_self_ast(guard, target)),
                    body: rewrite_self_ast(arm.body, target),
                })
                .collect(),
        ),
        Ast::FieldAccess(span, expr, field) => {
            Ast::FieldAccess(span, Box::new(rewrite_self_ast(*expr, target)), field)
        }
        Ast::FacetSegmentAccess(span, expr, segment) => {
            let segment = match segment {
                FacetPathSegment::Field { .. } => segment,
                FacetPathSegment::Bracket(expr) => {
                    FacetPathSegment::Bracket(spire::ast::FacetBracketExpr {
                        expr: Box::new(rewrite_self_ast(*expr.expr, target)),
                        display: expr.display,
                    })
                }
            };
            Ast::FacetSegmentAccess(span, Box::new(rewrite_self_ast(*expr, target)), segment)
        }
        Ast::StructLit(span, name, fields) => Ast::StructLit(
            span,
            name,
            fields
                .into_iter()
                .map(|field| match field {
                    StructLitField::Explicit(field_name, expr) => {
                        StructLitField::Explicit(field_name, rewrite_self_ast(expr, target))
                    }
                    StructLitField::Shorthand(field_name) => StructLitField::Shorthand(field_name),
                })
                .collect(),
        ),
        Ast::InternalStructLit(span, name, fields) => Ast::InternalStructLit(
            span,
            name,
            fields
                .into_iter()
                .map(|field| match field {
                    StructLitField::Explicit(field_name, expr) => {
                        StructLitField::Explicit(field_name, rewrite_self_ast(expr, target))
                    }
                    StructLitField::Shorthand(field_name) => StructLitField::Shorthand(field_name),
                })
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
                    visibility: field.visibility,
                    readonly: field.readonly,
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
        Ast::Def(span, name, type_params, params, ret_ty, where_clause, body, attrs) => Ast::Def(
            span,
            name,
            type_params,
            params
                .into_iter()
                .map(|param| FunParam {
                    name: param.name,
                    ty: rewrite_self_type(param.ty, target),
                    span: param.span,
                })
                .collect(),
            ret_ty.map(|ret| rewrite_self_type(ret, target)),
            where_clause.map(|clause| rewrite_self_where_clause(clause, target)),
            Box::new(rewrite_self_ast(*body, target)),
            attrs,
        ),
        Ast::ConstDef(span, name, ty, value, attrs) => Ast::ConstDef(
            span,
            name,
            ty.map(|ty| rewrite_self_type(ty, target)),
            Box::new(rewrite_self_ast(*value, target)),
            attrs,
        ),
        Ast::ExtractorDef(span, name, type_params, param, ret_ty, body, attrs) => {
            Ast::ExtractorDef(
                span,
                name,
                type_params,
                ExtractorParam {
                    name: param.name,
                    ty: param.ty.map(|ty| rewrite_self_type(ty, target)),
                    span: param.span,
                },
                rewrite_self_type(ret_ty, target),
                Box::new(rewrite_self_ast(*body, target)),
                attrs,
            )
        }
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
        Ast::IntrinsicDecl(span, name, signature, attrs) => {
            Ast::IntrinsicDecl(span, name, signature, attrs)
        }
        Ast::BuiltinExtractorDecl(span, name, param, ret_ty, attrs) => Ast::BuiltinExtractorDecl(
            span,
            name,
            ExtractorParam {
                name: param.name,
                ty: param.ty.map(|ty| rewrite_self_type(ty, target)),
                span: param.span,
            },
            rewrite_self_type(ret_ty, target),
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
        Ast::TypeApply(span, target_expr, args) => Ast::TypeApply(
            span,
            Box::new(rewrite_self_ast(*target_expr, target)),
            args.into_iter()
                .map(|arg| rewrite_self_type(arg, target))
                .collect(),
        ),
        Ast::FuncLiteralRef(span, func) => Ast::FuncLiteralRef(span, func),
        Ast::CapturePlaceholder(span, index) => Ast::CapturePlaceholder(span, index),
        Ast::Grouped(span, inner) => Ast::Grouped(span, Box::new(rewrite_self_ast(*inner, target))),
        Ast::Semi(span, inner) => Ast::Semi(span, Box::new(rewrite_self_ast(*inner, target))),
        other => other,
    }
}

pub fn declaration_stage_ordering(index: &DeclarationIndex) -> DeclarationOrdering {
    let mut entries = index.values().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.stage_index
            .cmp(&right.stage_index)
            .then_with(|| left.fq_name.cmp(&right.fq_name))
    });
    DeclarationOrdering {
        entries: entries
            .into_iter()
            .map(|entry| StageOrderedDeclaration {
                stage_index: entry.stage_index,
                fq_name: entry.fq_name.clone(),
            })
            .collect(),
    }
}

pub fn declaration_uid_order(index: &DeclarationIndex) -> Vec<String> {
    declaration_stage_ordering(index).fq_names()
}

pub(super) fn assign_declaration_uids(index: &DeclarationIndex) -> HashMap<String, u32> {
    let mut scope = initialize_scope();
    let mut declaration_uids = HashMap::with_capacity(index.len());
    for fq_name in declaration_uid_order(index) {
        declaration_uids.insert(fq_name, scope.reserve_id());
    }
    declaration_uids
}

pub(super) fn declaration_uid_kind_map(
    index: &DeclarationIndex,
    declaration_uids: &HashMap<String, u32>,
) -> HashMap<u32, DeclarationKind> {
    let mut out = HashMap::new();
    out.insert(0, DeclarationKind::ResultCtor);
    out.insert(1, DeclarationKind::ResultCtor);
    for (fq_name, entry) in index {
        if let Some(uid) = declaration_uids.get(fq_name) {
            out.insert(*uid, entry.kind.clone());
        }
    }
    out
}

/// Precollect global declaration index from staged module ASTs.
///
/// The index key is the fully-qualified declaration name. This pass does not
/// resolve declaration bodies; it records the stable metadata needed for
/// staged visibility, imports, and deterministic `unique_id` assignment.
///
/// Collected declaration forms:
/// `def`, `defextractor`, `@builtin def`, `@builtin defextractor`, `@builtin type`,
/// `defstruct`, `defrecord`, `deferror`, `defenum`, `deftrait`, `impl`, and
/// `impl Trait for Type` members.
pub fn precollect_declaration_index(
    module_stages: &[Vec<StagedModuleAst>],
) -> Result<DeclarationIndex, ResolveError> {
    let mut index = DeclarationIndex::new();
    let mut seen_impl_targets: HashMap<String, Span> = HashMap::new();
    let mut seen_public_consts: HashMap<String, (usize, String)> = HashMap::new();
    for (stage_index, stage) in module_stages.iter().enumerate() {
        let stage_impl_targets = collect_stage_impl_target_resolutions(stage);
        for module in stage {
            if !module.module_path.is_empty()
                && !staged_module_is_impl_owner(module)
                && reserved_owner_surface_name_constraint(&module.module_path).is_some_and(
                    |constraint| {
                        matches!(
                            constraint.kind,
                            ReservedOwnerSurfaceNameKind::BuiltinSpecialEnumVariantAlias
                        )
                    },
                )
            {
                reject_reserved_owner_name(
                    "Module name",
                    &module.module_path,
                    &module_owner_fallback_span(module),
                    true,
                )?;
            }

            for stmt in &module.ast {
                if let Ast::ImplDef(span, target, methods, _) = stmt {
                    let target_kind = resolve_impl_target_kind(target, span, &stage_impl_targets)?;
                    if !matches!(
                        target_kind,
                        DeclarationKind::Struct
                            | DeclarationKind::Enum
                            | DeclarationKind::BuiltinType
                    ) {
                        return Err(ResolveError {
                            message: format!(
                                "impl target `{}` must be a standard type, struct, or enum (record is not supported)",
                                target
                            ),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }

                    let target_fq = target.clone();
                    if let Some(first_span) = seen_impl_targets.get(&target_fq) {
                        return Err(ResolveError {
                            message: format!(
                                "Multiple impl blocks for `{}` are not allowed",
                                global_surface_name(&target_fq)
                            ),
                            span: span.clone(),
                            related_labels: vec![
                                ResolveErrorLabel {
                                    span: first_span.clone(),
                                    message: "first definition".to_string(),
                                },
                                ResolveErrorLabel {
                                    span: span.clone(),
                                    message: "conflicting definition".to_string(),
                                },
                            ],
                        });
                    } else {
                        seen_impl_targets.insert(target_fq.clone(), span.clone());
                    }

                    let method_module_path = impl_owner_module_path(target);
                    for method in methods {
                        let (method_span, method_name, kind, attrs) = match method {
                            Ast::Def(method_span, method_name, _, _, _, _, _, attrs) => {
                                let kind = if method_name == "new" {
                                    if !matches!(target_kind, DeclarationKind::Struct) {
                                        return Err(ResolveError {
                                            message: "`new` is only allowed in impl blocks for struct types"
                                                .to_string(),
                                            span: method_span.clone(),
                                        related_labels: Vec::new(),
                                        });
                                    }
                                    DeclarationKind::ImplCtorNew
                                } else {
                                    DeclarationKind::ImplMethod
                                };
                                (method_span, method_name, kind, attrs)
                            }
                            Ast::BuiltinDecl(method_span, method_name, _, _, attrs) => {
                                (method_span, method_name, DeclarationKind::Def, attrs)
                            }
                            Ast::ExtractorDef(method_span, method_name, _, _, _, _, attrs) => {
                                (method_span, method_name, DeclarationKind::Extractor, attrs)
                            }
                            Ast::BuiltinExtractorDecl(method_span, method_name, _, _, attrs) => {
                                (method_span, method_name, DeclarationKind::Extractor, attrs)
                            }
                            Ast::IntrinsicDecl(method_span, method_name, _, attrs) => {
                                (method_span, method_name, DeclarationKind::Def, attrs)
                            }
                            _ => {
                                return Err(ResolveError {
                                    message:
                                        "impl body may only contain `def` / `defextractor` / `@builtin def` / `@builtin defextractor` / `@intrinsic def` declarations"
                                            .to_string(),
                                    span: span.clone(),
                                related_labels: Vec::new(),
                                });
                            }
                        };

                        let fq_name = format!("{}::{}", method_module_path, method_name);
                        insert_declaration_entry(
                            &mut index,
                            declaration_entry(
                                method_module_path.clone(),
                                method_name.clone(),
                                fq_name,
                                kind,
                                stage_index,
                                false,
                                attrs.hidden,
                                entry_visibility(attrs),
                                entry_user_importable(attrs),
                                entry_user_callable(attrs),
                            ),
                            method_span,
                        )?;
                    }
                    continue;
                }

                if let Ast::TraitDef(span, name, _type_params, _, methods, attrs) = stmt {
                    reject_reserved_owner_name("Owner name", name, span, false)?;
                    let fq_name = if module.module_path.is_empty() {
                        name.clone()
                    } else {
                        format!("{}::{}", module.module_path, name)
                    };
                    insert_declaration_entry(
                        &mut index,
                        declaration_entry(
                            module.module_path.clone(),
                            name.clone(),
                            fq_name,
                            DeclarationKind::Trait,
                            stage_index,
                            attrs.auto_import,
                            false,
                            Visibility::Public,
                            true,
                            true,
                        ),
                        span,
                    )?;

                    for method in methods {
                        let method_name = trait_method_qualified_name(name, &method.name);
                        let qualified_trait_name = if module.module_path.is_empty() {
                            name.clone()
                        } else {
                            format!("{}::{}", module.module_path, name)
                        };
                        let method_fq_name =
                            trait_method_qualified_name(&qualified_trait_name, &method.name);
                        insert_declaration_entry(
                            &mut index,
                            declaration_entry(
                                module.module_path.clone(),
                                method_name,
                                method_fq_name,
                                DeclarationKind::TraitMethod,
                                stage_index,
                                false,
                                false,
                                Visibility::Public,
                                true,
                                true,
                            ),
                            &method.span,
                        )?;
                    }
                    continue;
                }

                if let Ast::TraitImplDef(
                    span,
                    _trait_name,
                    _trait_args,
                    _target_ty,
                    _,
                    methods,
                    _,
                ) = stmt
                {
                    for method in methods {
                        let (method_span, method_name) = match method {
                            Ast::Def(method_span, method_name, _, _, _, _, _, _)
                            | Ast::BuiltinDecl(method_span, method_name, _, _, _) => {
                                (method_span, method_name)
                            }
                            _ => {
                                return Err(ResolveError {
                                    message:
                                        "trait impl body may only contain `def` / `@builtin def` declarations"
                                            .to_string(),
                                    span: span.clone(),
                                    related_labels: Vec::new(),
                                });
                            }
                        };
                        let internal_name = trait_impl_method_qualified_name(
                            Some(module.module_path.as_str()),
                            _trait_name,
                            _trait_args,
                            _target_ty,
                            &method_name,
                            method_span.start,
                        );
                        insert_declaration_entry(
                            &mut index,
                            declaration_entry(
                                if module.module_path.is_empty() {
                                    "__traitimpl__".to_string()
                                } else {
                                    format!("{}::__traitimpl__", module.module_path)
                                },
                                internal_name.clone(),
                                internal_name,
                                DeclarationKind::ImplMethod,
                                stage_index,
                                false,
                                false,
                                Visibility::Private,
                                false,
                                false,
                            ),
                            method_span,
                        )?;
                    }
                    continue;
                }

                if let Ast::EnumDef(span, name, _, variants, attrs) = stmt {
                    reject_reserved_owner_name("Type name", name, span, attrs.builtin)?;
                    let fq_name = name.to_string();
                    insert_declaration_entry(
                        &mut index,
                        declaration_entry(
                            TYPE_DECL_ENTRY_MODULE_PATH,
                            name.clone(),
                            fq_name,
                            DeclarationKind::Enum,
                            stage_index,
                            false,
                            false,
                            Visibility::Public,
                            true,
                            true,
                        ),
                        span,
                    )?;

                    for variant in variants {
                        let variant_name = format!("{}::{}", name, variant.name);
                        let variant_fq_name = variant_name.clone();
                        insert_declaration_entry(
                            &mut index,
                            declaration_entry(
                                TYPE_DECL_ENTRY_MODULE_PATH,
                                variant_name,
                                variant_fq_name,
                                DeclarationKind::EnumVariant,
                                stage_index,
                                false,
                                false,
                                Visibility::Public,
                                true,
                                true,
                            ),
                            &variant.span,
                        )?;
                    }
                    continue;
                }

                let (span, name, kind, visibility, hidden, user_importable, user_callable) =
                    match stmt {
                        Ast::Def(span, name, _, _, _, _, _, attrs) => (
                            span,
                            name.as_str(),
                            DeclarationKind::Def,
                            entry_visibility(attrs),
                            false,
                            entry_user_importable(attrs),
                            entry_user_callable(attrs),
                        ),
                        Ast::ExtractorDef(span, name, _, _, _, _, attrs) => (
                            span,
                            name.as_str(),
                            DeclarationKind::Extractor,
                            entry_visibility(attrs),
                            false,
                            entry_user_importable(attrs),
                            entry_user_callable(attrs),
                        ),
                        Ast::ConstDef(span, name, _, _, attrs) => (
                            span,
                            name.as_str(),
                            DeclarationKind::Const,
                            entry_visibility(attrs),
                            false,
                            entry_user_importable(attrs),
                            entry_user_callable(attrs),
                        ),
                        Ast::BuiltinDecl(span, name, _, _, attrs) => (
                            span,
                            name.as_str(),
                            DeclarationKind::Def,
                            entry_visibility(attrs),
                            attrs.hidden,
                            entry_user_importable(attrs),
                            entry_user_callable(attrs),
                        ),
                        Ast::IntrinsicDecl(_, _, _, _) => continue,
                        Ast::BuiltinExtractorDecl(span, name, _, _, attrs) => (
                            span,
                            name.as_str(),
                            DeclarationKind::Extractor,
                            Visibility::Public,
                            attrs.hidden,
                            entry_user_importable(attrs),
                            entry_user_callable(attrs),
                        ),
                        Ast::ImplDef(_, _, _, _) | Ast::TraitDef(..) | Ast::TraitImplDef(..) => {
                            continue
                        }
                        Ast::ResultCtorDecl(span, name, _, _, attrs) => (
                            span,
                            name.as_str(),
                            DeclarationKind::ResultCtor,
                            Visibility::Public,
                            attrs.hidden,
                            entry_user_importable(attrs),
                            entry_user_callable(attrs),
                        ),
                        Ast::BuiltinTypeDecl(span, head, attrs) => (
                            span,
                            head.name.as_str(),
                            DeclarationKind::BuiltinType,
                            Visibility::Public,
                            attrs.hidden,
                            entry_user_importable(attrs),
                            entry_user_callable(attrs),
                        ),
                        Ast::StructDef(span, name, ..) => (
                            span,
                            name.as_str(),
                            DeclarationKind::Struct,
                            Visibility::Public,
                            false,
                            true,
                            true,
                        ),
                        Ast::RecordDef(span, name, _, _) => (
                            span,
                            name.as_str(),
                            DeclarationKind::Record,
                            Visibility::Public,
                            false,
                            true,
                            true,
                        ),
                        Ast::DeferrorDef(span, name, _, _, _) => (
                            span,
                            name.as_str(),
                            DeclarationKind::Deferror,
                            Visibility::Public,
                            false,
                            true,
                            true,
                        ),
                        _ => continue,
                    };

                if matches!(stmt, Ast::BuiltinDecl(_, name, _, _, _) if is_doc_only_builtin_decl(name))
                {
                    continue;
                }

                if matches!(
                    stmt,
                    Ast::StructDef(_, ..)
                        | Ast::RecordDef(_, _, _, _)
                        | Ast::DeferrorDef(_, _, _, _, _)
                ) {
                    reject_reserved_owner_name("Type name", name, span, false)?;
                }

                let fq_name = if kind == DeclarationKind::Const {
                    if visibility == Visibility::Public {
                        if let Some((prev_stage, prev_module)) =
                            seen_public_consts.get(name).cloned()
                        {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate public const: {} (already declared in stage {} module {})",
                                    name, prev_stage, prev_module
                                ),
                                span: span.clone(),
                                related_labels: Vec::new(),
                            });
                        }
                        seen_public_consts
                            .insert(name.to_string(), (stage_index, module.module_path.clone()));
                        if module.module_path.is_empty() {
                            name.to_string()
                        } else {
                            format!("{}::{}", module.module_path, name)
                        }
                    } else if module.module_path.is_empty() {
                        format!("__const__::{}", name)
                    } else {
                        format!("{}::__const__::{}", module.module_path, name)
                    }
                } else if matches!(
                    kind,
                    DeclarationKind::BuiltinType
                        | DeclarationKind::Struct
                        | DeclarationKind::Record
                        | DeclarationKind::Deferror
                ) {
                    name.to_string()
                } else if module.module_path.is_empty() {
                    name.to_string()
                } else {
                    format!("{}::{}", module.module_path, name)
                };

                let entry_module_path = if matches!(
                    kind,
                    DeclarationKind::BuiltinType
                        | DeclarationKind::Struct
                        | DeclarationKind::Record
                        | DeclarationKind::Deferror
                ) {
                    TYPE_DECL_ENTRY_MODULE_PATH.to_string()
                } else {
                    module.module_path.clone()
                };
                insert_declaration_entry(
                    &mut index,
                    declaration_entry(
                        entry_module_path,
                        name.to_string(),
                        fq_name,
                        kind,
                        stage_index,
                        false,
                        hidden,
                        visibility,
                        user_importable,
                        user_callable,
                    ),
                    span,
                )?;
            }
        }
    }

    Ok(index)
}

impl Resolver {
    fn duplicate_top_level_definition_error(surface: &str, span: &Span) -> ResolveError {
        ResolveError {
            message: format!("Duplicate top-level definition: {}", surface),
            span: span.clone(),
            related_labels: Vec::new(),
        }
    }

    fn reject_duplicate_top_level_declaration(
        &self,
        declared_in_batch: &mut HashSet<String>,
        surface: &str,
        lookup_name: &str,
        span: &Span,
    ) -> Result<(), ResolveError> {
        if !declared_in_batch.insert(surface.to_string()) {
            return Err(Self::duplicate_top_level_definition_error(surface, span));
        }
        if !self.allow_top_level_shadowing && self.scope.lookup(lookup_name).is_some() {
            return Err(Self::duplicate_top_level_definition_error(surface, span));
        }
        Ok(())
    }

    fn reserve_declaration_uid(&mut self, qualified_name: &str) -> u32 {
        self.declaration_uids
            .get(qualified_name)
            .copied()
            .unwrap_or_else(|| {
                let fresh = self.scope.reserve_id();
                self.declaration_uids
                    .insert(qualified_name.to_string(), fresh);
                fresh
            })
    }

    fn reserve_scope_uid(&mut self, name: &str) -> u32 {
        self.scope
            .lookup(name)
            .unwrap_or_else(|| self.scope.reserve_id())
    }

    fn record_predeclared_uid(&mut self, name: &str, uid: u32, kind: DeclarationKind) {
        self.predeclared_ids
            .entry(name.to_string())
            .or_default()
            .push_back(uid);
        self.declaration_uid_kinds.insert(uid, kind);
    }

    fn predeclare_scope_binding(&mut self, name: &str, uid: u32, alias: Option<&str>) {
        self.scope.define_with_id(name, uid);
        if let Some(alias) = alias {
            define_global_surface_alias(&mut self.scope, alias, uid);
        }
    }

    pub(super) fn lower_impl_defs(&self, stmts: Vec<Ast>) -> Result<Vec<Ast>, ResolveError> {
        let local_impl_targets;
        let impl_targets = if let Some(stage_targets) = self.current_stage_impl_targets.as_ref() {
            stage_targets
        } else {
            let mut local_targets = HashMap::new();
            for stmt in &stmts {
                let (name, kind) = match stmt {
                    Ast::StructDef(_, name, ..) => (name, DeclarationKind::Struct),
                    Ast::EnumDef(_, name, _, _, _) => (name, DeclarationKind::Enum),
                    Ast::RecordDef(_, name, _, _) => (name, DeclarationKind::Record),
                    Ast::DeferrorDef(_, name, _, _, _) => (name, DeclarationKind::Deferror),
                    _ => continue,
                };
                match local_targets.get(name) {
                    None => {
                        local_targets.insert(name.clone(), ImplTargetResolution::Unique(kind));
                    }
                    Some(ImplTargetResolution::Unique(_))
                    | Some(ImplTargetResolution::Ambiguous) => {
                        local_targets.insert(name.clone(), ImplTargetResolution::Ambiguous);
                    }
                }
            }
            local_impl_targets = local_targets;
            &local_impl_targets
        };

        let mut lowered = Vec::new();
        let mut seen_impl_targets: HashMap<String, Span> = HashMap::new();

        for stmt in stmts {
            match stmt {
                Ast::ImplDef(span, target, methods, _attrs) => {
                    let target_kind = resolve_impl_target_kind(&target, &span, impl_targets)?;
                    if !matches!(
                        target_kind,
                        DeclarationKind::Struct
                            | DeclarationKind::Enum
                            | DeclarationKind::BuiltinType
                    ) {
                        return Err(ResolveError {
                            message: format!(
                                "impl target `{}` must be a standard type, struct, or enum (record is not supported)",
                                target
                            ),
                            span,
                            related_labels: Vec::new(),
                        });
                    }
                    if let Some(first_span) = seen_impl_targets.get(&target) {
                        return Err(ResolveError {
                            message: format!(
                                "Multiple impl blocks for `{}` are not allowed",
                                global_surface_name(&target)
                            ),
                            span: span.clone(),
                            related_labels: vec![
                                ResolveErrorLabel {
                                    span: first_span.clone(),
                                    message: "first definition".to_string(),
                                },
                                ResolveErrorLabel {
                                    span: span.clone(),
                                    message: "conflicting definition".to_string(),
                                },
                            ],
                        });
                    } else {
                        seen_impl_targets.insert(target.clone(), span.clone());
                    }

                    let lowered_module_path = self.current_module_path.as_deref();
                    for method in methods {
                        match method {
                            Ast::Def(
                                method_span,
                                method_name,
                                type_params,
                                params,
                                ret_ty,
                                where_clause,
                                body,
                                attrs,
                            ) => {
                                if method_name == "new"
                                    && !matches!(target_kind, DeclarationKind::Struct)
                                {
                                    return Err(ResolveError {
                                        message:
                                            "`new` is only allowed in impl blocks for struct types"
                                                .to_string(),
                                        span: method_span,
                                        related_labels: Vec::new(),
                                    });
                                }

                                let lowered_name = lower_impl_member_name(
                                    lowered_module_path,
                                    &target,
                                    &method_name,
                                );
                                let lowered_params = params
                                    .into_iter()
                                    .map(|param| FunParam {
                                        name: param.name,
                                        ty: rewrite_self_type(param.ty, &target),
                                        span: param.span,
                                    })
                                    .collect::<Vec<_>>();
                                let lowered_ret_ty =
                                    ret_ty.map(|ty| rewrite_self_type(ty, &target));
                                let lowered_body = rewrite_self_ast(*body, &target);

                                lowered.push(Ast::Def(
                                    method_span,
                                    lowered_name,
                                    type_params,
                                    lowered_params,
                                    lowered_ret_ty,
                                    where_clause
                                        .map(|clause| rewrite_self_where_clause(clause, &target)),
                                    Box::new(lowered_body),
                                    attrs,
                                ));
                            }
                            Ast::ExtractorDef(
                                method_span,
                                method_name,
                                type_params,
                                param,
                                ret_ty,
                                body,
                                attrs,
                            ) => {
                                let lowered_name = lower_impl_member_name(
                                    lowered_module_path,
                                    &target,
                                    &method_name,
                                );
                                let lowered_param = ExtractorParam {
                                    name: param.name,
                                    ty: param.ty.map(|ty| rewrite_self_type(ty, &target)),
                                    span: param.span,
                                };
                                let lowered_ret_ty = rewrite_self_type(ret_ty, &target);
                                let lowered_body = rewrite_self_ast(*body, &target);

                                lowered.push(Ast::ExtractorDef(
                                    method_span,
                                    lowered_name,
                                    type_params,
                                    lowered_param,
                                    lowered_ret_ty,
                                    Box::new(lowered_body),
                                    attrs,
                                ));
                            }
                            Ast::BuiltinDecl(method_span, method_name, params, ret_ty, attrs) => {
                                let lowered_name = lower_impl_member_name(
                                    lowered_module_path,
                                    &target,
                                    &method_name,
                                );
                                let lowered_params = params
                                    .into_iter()
                                    .map(|param| FunParam {
                                        name: param.name,
                                        ty: rewrite_self_type(param.ty, &target),
                                        span: param.span,
                                    })
                                    .collect::<Vec<_>>();
                                let lowered_ret_ty =
                                    ret_ty.map(|ty| rewrite_self_type(ty, &target));

                                lowered.push(Ast::BuiltinDecl(
                                    method_span,
                                    lowered_name,
                                    lowered_params,
                                    lowered_ret_ty,
                                    attrs,
                                ));
                            }
                            Ast::BuiltinExtractorDecl(
                                method_span,
                                method_name,
                                param,
                                ret_ty,
                                attrs,
                            ) => {
                                let lowered_name = lower_impl_member_name(
                                    lowered_module_path,
                                    &target,
                                    &method_name,
                                );
                                let lowered_param = ExtractorParam {
                                    name: param.name,
                                    ty: param.ty.map(|ty| rewrite_self_type(ty, &target)),
                                    span: param.span,
                                };
                                let lowered_ret_ty = rewrite_self_type(ret_ty, &target);

                                lowered.push(Ast::BuiltinExtractorDecl(
                                    method_span,
                                    lowered_name,
                                    lowered_param,
                                    lowered_ret_ty,
                                    attrs,
                                ));
                            }
                            Ast::IntrinsicDecl(method_span, method_name, signature, attrs) => {
                                let lowered_name = lower_impl_member_name(
                                    lowered_module_path,
                                    &target,
                                    &method_name,
                                );
                                lowered.push(Ast::IntrinsicDecl(
                                    method_span,
                                    lowered_name,
                                    signature,
                                    attrs,
                                ));
                            }
                            _ => {
                                return Err(ResolveError {
                                    message:
                                        "impl body may only contain `def` / `defextractor` / `@builtin def` / `@builtin defextractor` / `@intrinsic def` declarations"
                                            .to_string(),
                                    span: span.clone(),
                                related_labels: Vec::new(),
                                });
                            }
                        }
                    }
                }
                other => lowered.push(other),
            }
        }

        Ok(lowered)
    }

    pub(super) fn validate_auto_import_conflicts(&self, stmts: &[Ast]) -> Result<(), ResolveError> {
        for stmt in stmts {
            match stmt {
                Ast::Import(_, _, _) => {}
                Ast::Defmod(_, _, body, _) => self.validate_auto_import_conflicts(body)?,
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn predeclare_functions(&mut self, stmts: &[Ast]) -> Result<(), ResolveError> {
        self.predeclared_ids.clear();
        // Language rule:
        // Top-level names must be unique per module / REPL session.
        // We intentionally enforce the same rule for file execution and REPL.
        let mut declared_in_batch = HashSet::new();
        for stmt in stmts {
            match stmt {
                Ast::Def(span, name, _, _, _, _, _, _) => {
                    let surface = global_surface_name(name).to_string();
                    self.reject_duplicate_top_level_declaration(
                        &mut declared_in_batch,
                        &surface,
                        name,
                        span,
                    )?;
                    let qualified_name = self.qualify_current_declaration_name(name);
                    let uid = self.reserve_declaration_uid(&qualified_name);
                    self.record_predeclared_uid(name, uid, DeclarationKind::Def);
                    // Keep the outer scope at the most recent declaration,
                    // so forward references resolve to the latest top-level definition.
                    self.predeclare_scope_binding(name, uid, Some(&qualified_name));
                }
                Ast::ExtractorDef(span, name, _, _, _, _, _) => {
                    let surface = global_surface_name(name).to_string();
                    self.reject_duplicate_top_level_declaration(
                        &mut declared_in_batch,
                        &surface,
                        name,
                        span,
                    )?;
                    let qualified_name = self.qualify_current_declaration_name(name);
                    let uid = self.reserve_declaration_uid(&qualified_name);
                    self.record_predeclared_uid(name, uid, DeclarationKind::Extractor);
                    self.predeclare_scope_binding(name, uid, Some(&qualified_name));
                }
                Ast::ConstDef(span, name, _, _, attrs) => {
                    self.reject_duplicate_top_level_declaration(
                        &mut declared_in_batch,
                        name,
                        name,
                        span,
                    )?;
                    let qualified_name = if attrs.visibility == Visibility::Public {
                        self.qualify_current_declaration_name(name)
                    } else {
                        self.qualify_current_declaration_name(&format!("__const__::{}", name))
                    };
                    let uid = self.reserve_declaration_uid(&qualified_name);
                    self.record_predeclared_uid(name, uid, DeclarationKind::Const);
                    self.predeclare_scope_binding(name, uid, Some(&qualified_name));
                }
                Ast::TraitDef(span, name, _type_params, where_clause, methods, _) => {
                    reject_reserved_owner_name("Owner name", name, span, false)?;
                    self.reject_duplicate_top_level_declaration(
                        &mut declared_in_batch,
                        name,
                        name,
                        span,
                    )?;
                    let qualified_trait = self.qualify_current_declaration_name(name);
                    let uid = self.reserve_declaration_uid(&qualified_trait);
                    self.record_predeclared_uid(name, uid, DeclarationKind::Trait);
                    self.predeclare_scope_binding(name, uid, None);
                    if let Some(clause) = where_clause {
                        for constraint in &clause.constraints {
                            if !matches!(&constraint.subject, AstTy::Named(_, subject) if subject == "Self")
                            {
                                continue;
                            }
                            for bound in &constraint.bounds {
                                if let spire::ast::WhereConstraintRhs::TypeConstructor(_, slots) =
                                    bound
                                {
                                    self.trait_constructor_slots.insert(
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
                            }
                        }
                    }

                    for method in methods {
                        let method_alias = trait_method_qualified_name(name, &method.name);
                        let qualified_method = trait_method_qualified_name(
                            &self.qualify_current_declaration_name(name),
                            &method.name,
                        );
                        self.reject_duplicate_top_level_declaration(
                            &mut declared_in_batch,
                            &method_alias,
                            &method_alias,
                            &method.span,
                        )?;
                        let method_uid = self.reserve_declaration_uid(&qualified_method);
                        self.record_predeclared_uid(
                            &method_alias,
                            method_uid,
                            DeclarationKind::TraitMethod,
                        );
                        self.predeclare_scope_binding(&method_alias, method_uid, None);
                    }
                }
                Ast::BuiltinDecl(_, name, _, _, _) => {
                    if is_doc_only_builtin_decl(name) {
                        continue;
                    }
                    if !declared_in_batch.insert(name.clone()) {
                        return Err(Self::duplicate_top_level_definition_error(
                            name,
                            stmt.span(),
                        ));
                    }
                    // Builtins are keyed by fixed IDs from builtin metadata.
                    // Re-declarations should keep that identity stable.
                    let uid = self.reserve_scope_uid(name);
                    self.record_predeclared_uid(name, uid, DeclarationKind::Def);
                    self.predeclare_scope_binding(name, uid, None);
                }
                Ast::IntrinsicDecl(_, _, _, _) => continue,
                Ast::BuiltinExtractorDecl(_, name, _, _, _) => {
                    if !declared_in_batch.insert(name.clone()) {
                        return Err(Self::duplicate_top_level_definition_error(
                            name,
                            stmt.span(),
                        ));
                    }
                    let uid = self.reserve_scope_uid(name);
                    self.record_predeclared_uid(name, uid, DeclarationKind::Extractor);
                    self.predeclare_scope_binding(name, uid, None);
                }
                Ast::ResultCtorDecl(span, name, _, _, _) => {
                    self.reject_duplicate_top_level_declaration(
                        &mut declared_in_batch,
                        name,
                        name,
                        span,
                    )?;
                    let uid = self.reserve_scope_uid(name);
                    self.record_predeclared_uid(name, uid, DeclarationKind::ResultCtor);
                    self.predeclare_scope_binding(name, uid, Some(name));
                }
                Ast::BuiltinTypeDecl(span, head, _) => {
                    let surface = global_surface_name(&head.name).to_string();
                    self.reject_duplicate_top_level_declaration(
                        &mut declared_in_batch,
                        &surface,
                        &head.name,
                        span,
                    )?;
                    let uid = self.reserve_declaration_uid(&head.name);
                    self.record_predeclared_uid(&head.name, uid, DeclarationKind::BuiltinType);
                    self.predeclare_scope_binding(&head.name, uid, Some(&head.name));
                }
                Ast::StructDef(span, name, ..)
                | Ast::RecordDef(span, name, _, _)
                | Ast::DeferrorDef(span, name, _, _, _) => {
                    let surface = global_surface_name(name).to_string();
                    reject_reserved_owner_name("Type name", name, span, false)?;
                    self.reject_duplicate_top_level_declaration(
                        &mut declared_in_batch,
                        &surface,
                        name,
                        span,
                    )?;
                    let qualified_name = name.clone();
                    let uid = self.reserve_declaration_uid(&qualified_name);
                    let Some(kind) = type_declaration_kind(stmt) else {
                        continue;
                    };
                    self.record_predeclared_uid(name, uid, kind);
                    self.predeclare_scope_binding(name, uid, Some(name));
                }
                Ast::EnumDef(span, name, _, variants, attrs) => {
                    let surface = global_surface_name(name).to_string();
                    reject_reserved_owner_name("Type name", name, span, attrs.builtin)?;
                    self.reject_duplicate_top_level_declaration(
                        &mut declared_in_batch,
                        &surface,
                        name,
                        span,
                    )?;
                    let qualified_enum = name.clone();
                    let uid = self.reserve_declaration_uid(&qualified_enum);
                    self.record_predeclared_uid(name, uid, DeclarationKind::Enum);
                    self.predeclare_scope_binding(name, uid, Some(name));

                    for variant in variants {
                        let qualified_ctor = format!("{}::{}", name, variant.name);
                        self.reject_duplicate_top_level_declaration(
                            &mut declared_in_batch,
                            &qualified_ctor,
                            &qualified_ctor,
                            &variant.span,
                        )?;
                        let ctor_uid = self.reserve_declaration_uid(&qualified_ctor);
                        self.record_predeclared_uid(
                            &qualified_ctor,
                            ctor_uid,
                            DeclarationKind::EnumVariant,
                        );
                        self.predeclare_scope_binding(
                            &qualified_ctor,
                            ctor_uid,
                            Some(&qualified_ctor),
                        );
                        if attrs.builtin
                            && builtin_special_enum_surface_name(name)
                            && builtin_special_enum_variant_alias(name, &variant.name)
                        {
                            self.record_predeclared_uid(
                                &variant.name,
                                ctor_uid,
                                DeclarationKind::EnumVariant,
                            );
                            self.predeclare_scope_binding(
                                &variant.name,
                                ctor_uid,
                                Some(&qualified_ctor),
                            );
                        }
                    }
                }
                Ast::TraitImplDef(..) => {}
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn take_predeclared_id(&mut self, name: &str) -> Option<u32> {
        self.predeclared_ids
            .get_mut(name)
            .and_then(|ids| ids.pop_front())
    }
}
