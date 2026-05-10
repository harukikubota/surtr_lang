use super::scope_init::initialize_scope;
use super::scope_init::is_doc_only_builtin_decl;
use super::*;
use sindr::builtin::{builtin_type_meta_by_name, builtin_type_supports_inherent_impl};

use serde::{Deserialize, Serialize};

fn is_reserved_builtin_type_redefinition(name: &str) -> bool {
    let surface_name = name.strip_prefix("Global::").unwrap_or(name);
    builtin_type_meta_by_name(surface_name).is_some() && surface_name != "ProcessInit"
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

fn entry_visibility(attrs: &DeclAttrs) -> Visibility {
    attrs.visibility
}

fn entry_user_importable(attrs: &DeclAttrs) -> bool {
    attrs.user_importable
}

fn entry_user_callable(attrs: &DeclAttrs) -> bool {
    attrs.user_callable
}

fn normalize_impl_method_name(target: &str, method_name: &str) -> String {
    format!("{}::{}", target, method_name)
}

fn impl_method_module_path(_module_path: &str, target: &str) -> String {
    target.to_string()
}

fn lower_impl_member_name(
    current_module_path: Option<&str>,
    target: &str,
    method_name: &str,
) -> String {
    if current_module_path == Some(target) {
        method_name.to_string()
    } else {
        normalize_impl_method_name(target, method_name)
    }
}

fn type_decl_entry_module_path() -> String {
    String::new()
}

fn surface_name(name: &str) -> &str {
    name.strip_prefix("Global::").unwrap_or(name)
}

fn define_surface_alias(scope: &mut Scope, canonical_name: &str, uid: u32) {
    if surface_name(canonical_name) != canonical_name {
        scope.define_with_id(surface_name(canonical_name), uid);
    }
}

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
                Ast::StructDef(_, name, _, _) => (name, DeclarationKind::Struct),
                Ast::EnumDef(_, name, _, _, _) => (name, DeclarationKind::Enum),
                Ast::RecordDef(_, name, _, _) => (name, DeclarationKind::Record),
                Ast::DeferrorDef(_, name, _, _, _) => (name, DeclarationKind::Deferror),
                _ => continue,
            };
            match resolutions.get(name) {
                None => {
                    resolutions.insert(name.clone(), ImplTargetResolution::Unique(kind.clone()));
                    if let Some(surface_name) = name.strip_prefix("Global::") {
                        resolutions
                            .insert(surface_name.to_string(), ImplTargetResolution::Unique(kind));
                    }
                }
                Some(ImplTargetResolution::Unique(_)) | Some(ImplTargetResolution::Ambiguous) => {
                    resolutions.insert(name.clone(), ImplTargetResolution::Ambiguous);
                    if let Some(surface_name) = name.strip_prefix("Global::") {
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
            let builtin_target = target.strip_prefix("Global::").unwrap_or(target);
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
        Ast::Def(span, name, type_params, params, ret_ty, body, attrs) => Ast::Def(
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
        Ast::FuncLiteralRef(span, func) => Ast::FuncLiteralRef(span, func),
        Ast::CapturePlaceholder(span, index) => Ast::CapturePlaceholder(span, index),
        Ast::Grouped(span, inner) => Ast::Grouped(span, Box::new(rewrite_self_ast(*inner, target))),
        Ast::Semi(span, inner) => Ast::Semi(span, Box::new(rewrite_self_ast(*inner, target))),
        other => other,
    }
}

pub(super) fn assign_declaration_uids(index: &DeclarationIndex) -> HashMap<String, u32> {
    let mut scope = initialize_scope();
    let mut declaration_uids = HashMap::with_capacity(index.len());
    let mut entries = index.values().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.stage_index
            .cmp(&right.stage_index)
            .then_with(|| left.fq_name.cmp(&right.fq_name))
    });
    for entry in entries {
        declaration_uids.insert(entry.fq_name.clone(), scope.reserve_id());
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
/// The index key is fully-qualified name `ModulePath::Name`.
/// Only declaration forms covered by Issue 6 are collected:
/// `def`, `defextractor`, `@builtin def`, `@builtin defextractor`, `@builtin type`,
/// `defstruct`, `defrecord`, `deferror`.
pub fn precollect_declaration_index(
    module_stages: &[Vec<StagedModuleAst>],
) -> Result<DeclarationIndex, ResolveError> {
    let mut index = DeclarationIndex::new();
    let mut seen_impl_targets: HashMap<String, Span> = HashMap::new();
    let mut seen_public_consts: HashMap<String, (usize, String)> = HashMap::new();
    for (stage_index, stage) in module_stages.iter().enumerate() {
        let stage_impl_targets = collect_stage_impl_target_resolutions(stage);
        for module in stage {
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
                                surface_name(&target_fq)
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

                    let method_module_path = impl_method_module_path(&module.module_path, target);
                    for method in methods {
                        let (method_span, method_name, kind, attrs) = match method {
                            Ast::Def(method_span, method_name, _, _, _, _, attrs) => {
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
                            _ => {
                                return Err(ResolveError {
                                    message:
                                        "impl body may only contain `def` / `defextractor` / `@builtin def` / `@builtin defextractor` declarations"
                                            .to_string(),
                                    span: span.clone(),
                                related_labels: Vec::new(),
                                });
                            }
                        };

                        let fq_name = format!("{}::{}", method_module_path, method_name);
                        if let Some(prev) = index.get(&fq_name) {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate fully-qualified declaration: {} (already declared in stage {} module {})",
                                    surface_name(&fq_name), prev.stage_index, prev.module_path
                                ),
                                span: method_span.clone(),
                            related_labels: Vec::new(),
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
                                auto_import: false,
                                hidden: attrs.hidden,
                                visibility: entry_visibility(attrs),
                                user_importable: entry_user_importable(attrs),
                                user_callable: entry_user_callable(attrs),
                            },
                        );
                    }
                    continue;
                }

                if let Ast::TraitDef(span, name, _type_params, methods, attrs) = stmt {
                    let fq_name = if module.module_path.is_empty() {
                        name.clone()
                    } else {
                        format!("{}::{}", module.module_path, name)
                    };
                    if let Some(prev) = index.get(&fq_name) {
                        return Err(ResolveError {
                            message: format!(
                                "Duplicate fully-qualified declaration: {} (already declared in stage {} module {})",
                                surface_name(&fq_name), prev.stage_index, prev.module_path
                            ),
                            span: span.clone(),
                        related_labels: Vec::new(),
                        });
                    }
                    index.insert(
                        fq_name.clone(),
                        DeclarationEntry {
                            module_path: module.module_path.clone(),
                            name: name.clone(),
                            fq_name,
                            kind: DeclarationKind::Trait,
                            stage_index,
                            auto_import: attrs.auto_import,
                            hidden: false,
                            visibility: Visibility::Public,
                            user_importable: true,
                            user_callable: true,
                        },
                    );

                    for method in methods {
                        let method_name = trait_method_qualified_name(name, &method.name);
                        let qualified_trait_name = if module.module_path.is_empty() {
                            name.clone()
                        } else {
                            format!("{}::{}", module.module_path, name)
                        };
                        let method_fq_name =
                            trait_method_qualified_name(&qualified_trait_name, &method.name);
                        if let Some(prev) = index.get(&method_fq_name) {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate fully-qualified declaration: {} (already declared in stage {} module {})",
                                    surface_name(&method_fq_name), prev.stage_index, prev.module_path
                                ),
                                span: method.span.clone(),
                            related_labels: Vec::new(),
                            });
                        }
                        index.insert(
                            method_fq_name.clone(),
                            DeclarationEntry {
                                module_path: module.module_path.clone(),
                                name: method_name,
                                fq_name: method_fq_name,
                                kind: DeclarationKind::TraitMethod,
                                stage_index,
                                auto_import: false,
                                hidden: false,
                                visibility: Visibility::Public,
                                user_importable: true,
                                user_callable: true,
                            },
                        );
                    }
                    continue;
                }

                if let Ast::TraitImplDef(span, _trait_name, _trait_args, _target_ty, methods, _) =
                    stmt
                {
                    for method in methods {
                        let (method_span, method_name) = match method {
                            Ast::Def(method_span, method_name, _, _, _, _, _)
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
                        if let Some(prev) = index.get(&internal_name) {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate fully-qualified declaration: {} (already declared in stage {} module {})",
                                    surface_name(&internal_name), prev.stage_index, prev.module_path
                                ),
                                span: method_span.clone(),
                            related_labels: Vec::new(),
                            });
                        }
                        index.insert(
                            internal_name.clone(),
                            DeclarationEntry {
                                module_path: if module.module_path.is_empty() {
                                    "__traitimpl__".to_string()
                                } else {
                                    format!("{}::__traitimpl__", module.module_path)
                                },
                                name: internal_name.clone(),
                                fq_name: internal_name,
                                kind: DeclarationKind::ImplMethod,
                                stage_index,
                                auto_import: false,
                                hidden: false,
                                visibility: Visibility::Private,
                                user_importable: false,
                                user_callable: false,
                            },
                        );
                    }
                    continue;
                }

                if let Ast::EnumDef(span, name, _, variants, _) = stmt {
                    if is_reserved_builtin_type_redefinition(name) {
                        return Err(ResolveError {
                            message: format!(
                                "Type name `{}` is reserved by a canonical builtin type declaration",
                                name.strip_prefix("Global::").unwrap_or(name)
                            ),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    let fq_name = name.to_string();
                    if let Some(prev) = index.get(&fq_name) {
                        return Err(ResolveError {
                            message: format!(
                                "Duplicate fully-qualified declaration: {} (already declared in stage {} module {})",
                                surface_name(&fq_name), prev.stage_index, prev.module_path
                            ),
                            span: span.clone(),
                        related_labels: Vec::new(),
                        });
                    }
                    index.insert(
                        fq_name.clone(),
                        DeclarationEntry {
                            module_path: type_decl_entry_module_path(),
                            name: name.clone(),
                            fq_name,
                            kind: DeclarationKind::Enum,
                            stage_index,
                            auto_import: false,
                            hidden: false,
                            visibility: Visibility::Public,
                            user_importable: true,
                            user_callable: true,
                        },
                    );

                    for variant in variants {
                        let variant_name = format!("{}::{}", name, variant.name);
                        let variant_fq_name = variant_name.clone();
                        if let Some(prev) = index.get(&variant_fq_name) {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate fully-qualified declaration: {} (already declared in stage {} module {})",
                                    surface_name(&variant_fq_name), prev.stage_index, prev.module_path
                                ),
                                span: variant.span.clone(),
                            related_labels: Vec::new(),
                            });
                        }
                        index.insert(
                            variant_fq_name.clone(),
                            DeclarationEntry {
                                module_path: type_decl_entry_module_path(),
                                name: variant_name,
                                fq_name: variant_fq_name,
                                kind: DeclarationKind::EnumVariant,
                                stage_index,
                                auto_import: false,
                                hidden: false,
                                visibility: Visibility::Public,
                                user_importable: true,
                                user_callable: true,
                            },
                        );
                    }
                    continue;
                }

                let (span, name, kind, visibility, hidden, user_importable, user_callable) =
                    match stmt {
                        Ast::Def(span, name, _, _, _, _, attrs) => (
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
                            Visibility::Public,
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
                        Ast::ImplDef(_, _, _, _)
                        | Ast::TraitDef(_, _, _, _, _)
                        | Ast::TraitImplDef(_, _, _, _, _, _) => continue,
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
                        Ast::StructDef(span, name, _, _) => (
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
                    Ast::StructDef(_, name, _, _)
                        | Ast::RecordDef(_, name, _, _)
                        | Ast::DeferrorDef(_, name, _, _, _)
                        if is_reserved_builtin_type_redefinition(name)
                ) {
                    return Err(ResolveError {
                        message: format!(
                            "Type name `{}` is reserved by a canonical builtin type declaration",
                            name.strip_prefix("Global::").unwrap_or(name)
                        ),
                        span: span.clone(),
                        related_labels: Vec::new(),
                    });
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

                if let Some(prev) = index.get(&fq_name) {
                    return Err(ResolveError {
                        message: format!(
                            "Duplicate fully-qualified declaration: {} (already declared in stage {} module {})",
                            surface_name(&fq_name), prev.stage_index, prev.module_path
                        ),
                        span: span.clone(),
                    related_labels: Vec::new(),
                    });
                }

                index.insert(
                    fq_name.clone(),
                    DeclarationEntry {
                        module_path: if matches!(
                            kind,
                            DeclarationKind::BuiltinType
                                | DeclarationKind::Struct
                                | DeclarationKind::Record
                                | DeclarationKind::Deferror
                        ) {
                            type_decl_entry_module_path()
                        } else {
                            module.module_path.clone()
                        },
                        name: name.to_string(),
                        fq_name,
                        kind,
                        stage_index,
                        auto_import: false,
                        hidden,
                        visibility,
                        user_importable,
                        user_callable,
                    },
                );
            }
        }
    }

    Ok(index)
}

impl Resolver {
    pub(super) fn lower_impl_defs(&self, stmts: Vec<Ast>) -> Result<Vec<Ast>, ResolveError> {
        let local_impl_targets = if self.current_stage_impl_targets.is_none() {
            let mut local_targets = HashMap::new();
            for stmt in &stmts {
                let (name, kind) = match stmt {
                    Ast::StructDef(_, name, _, _) => (name, DeclarationKind::Struct),
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
            Some(local_targets)
        } else {
            None
        };
        let impl_targets = self
            .current_stage_impl_targets
            .as_ref()
            .or(local_impl_targets.as_ref())
            .expect("impl target resolutions must exist");

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
                                surface_name(&target)
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
                            _ => {
                                return Err(ResolveError {
                                    message:
                                        "impl body may only contain `def` / `defextractor` / `@builtin def` / `@builtin defextractor` declarations"
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
                Ast::Def(span, name, _, _, _, _, _) => {
                    let surface = surface_name(name).to_string();
                    if !declared_in_batch.insert(surface.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", surface),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    if !self.allow_top_level_shadowing && self.scope.lookup(name).is_some() {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", surface),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    let qualified_name = self.qualify_current_declaration_name(name);
                    let uid = self
                        .declaration_uids
                        .get(&qualified_name)
                        .copied()
                        .unwrap_or_else(|| {
                            let fresh = self.scope.reserve_id();
                            self.declaration_uids.insert(qualified_name.clone(), fresh);
                            fresh
                        });
                    self.predeclared_ids
                        .entry(name.clone())
                        .or_default()
                        .push_back(uid);
                    self.declaration_uid_kinds.insert(uid, DeclarationKind::Def);
                    // Keep the outer scope at the most recent declaration,
                    // so forward references resolve to the latest top-level definition.
                    self.scope.define_with_id(name, uid);
                    define_surface_alias(&mut self.scope, &qualified_name, uid);
                }
                Ast::ExtractorDef(span, name, _, _, _, _, _) => {
                    let surface = surface_name(name).to_string();
                    if !declared_in_batch.insert(surface.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", surface),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    if !self.allow_top_level_shadowing && self.scope.lookup(name).is_some() {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", surface),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    let qualified_name = self.qualify_current_declaration_name(name);
                    let uid = self
                        .declaration_uids
                        .get(&qualified_name)
                        .copied()
                        .unwrap_or_else(|| {
                            let fresh = self.scope.reserve_id();
                            self.declaration_uids.insert(qualified_name.clone(), fresh);
                            fresh
                        });
                    self.predeclared_ids
                        .entry(name.clone())
                        .or_default()
                        .push_back(uid);
                    self.declaration_uid_kinds
                        .insert(uid, DeclarationKind::Extractor);
                    self.scope.define_with_id(name, uid);
                    define_surface_alias(&mut self.scope, &qualified_name, uid);
                }
                Ast::ConstDef(span, name, _, _, attrs) => {
                    if !declared_in_batch.insert(name.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    if !self.allow_top_level_shadowing && self.scope.lookup(name).is_some() {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    let qualified_name = if attrs.visibility == Visibility::Public {
                        self.qualify_current_declaration_name(name)
                    } else {
                        self.qualify_current_declaration_name(&format!("__const__::{}", name))
                    };
                    let uid = self
                        .declaration_uids
                        .get(&qualified_name)
                        .copied()
                        .unwrap_or_else(|| {
                            let fresh = self.scope.reserve_id();
                            self.declaration_uids.insert(qualified_name.clone(), fresh);
                            fresh
                        });
                    self.predeclared_ids
                        .entry(name.clone())
                        .or_default()
                        .push_back(uid);
                    self.declaration_uid_kinds
                        .insert(uid, DeclarationKind::Const);
                    self.scope.define_with_id(name, uid);
                    define_surface_alias(&mut self.scope, &qualified_name, uid);
                }
                Ast::TraitDef(span, name, _type_params, methods, _) => {
                    if !declared_in_batch.insert(name.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    if !self.allow_top_level_shadowing && self.scope.lookup(name).is_some() {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    let qualified_trait = self.qualify_current_declaration_name(name);
                    let uid = self
                        .declaration_uids
                        .get(&qualified_trait)
                        .copied()
                        .unwrap_or_else(|| {
                            let fresh = self.scope.reserve_id();
                            self.declaration_uids.insert(qualified_trait.clone(), fresh);
                            fresh
                        });
                    self.predeclared_ids
                        .entry(name.clone())
                        .or_default()
                        .push_back(uid);
                    self.declaration_uid_kinds
                        .insert(uid, DeclarationKind::Trait);
                    self.scope.define_with_id(name, uid);

                    for method in methods {
                        let method_alias = trait_method_qualified_name(name, &method.name);
                        let qualified_method = trait_method_qualified_name(
                            &self.qualify_current_declaration_name(name),
                            &method.name,
                        );
                        if !declared_in_batch.insert(method_alias.clone()) {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate top-level definition: {}",
                                    method_alias
                                ),
                                span: method.span.clone(),
                                related_labels: Vec::new(),
                            });
                        }
                        if !self.allow_top_level_shadowing
                            && self.scope.lookup(&method_alias).is_some()
                        {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate top-level definition: {}",
                                    method_alias
                                ),
                                span: method.span.clone(),
                                related_labels: Vec::new(),
                            });
                        }
                        let method_uid = self
                            .declaration_uids
                            .get(&qualified_method)
                            .copied()
                            .unwrap_or_else(|| {
                                let fresh = self.scope.reserve_id();
                                self.declaration_uids
                                    .insert(qualified_method.clone(), fresh);
                                fresh
                            });
                        self.predeclared_ids
                            .entry(method_alias.clone())
                            .or_default()
                            .push_back(method_uid);
                        self.declaration_uid_kinds
                            .insert(method_uid, DeclarationKind::TraitMethod);
                        self.scope.define_with_id(&method_alias, method_uid);
                    }
                }
                Ast::BuiltinDecl(_, name, _, _, _) => {
                    if is_doc_only_builtin_decl(name) {
                        continue;
                    }
                    if !declared_in_batch.insert(name.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: stmt.span().clone(),
                            related_labels: Vec::new(),
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
                    self.declaration_uid_kinds.insert(uid, DeclarationKind::Def);
                    self.scope.define_with_id(name, uid);
                }
                Ast::IntrinsicDecl(_, _, _, _) => continue,
                Ast::BuiltinExtractorDecl(_, name, _, _, _) => {
                    if !declared_in_batch.insert(name.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: stmt.span().clone(),
                            related_labels: Vec::new(),
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
                    self.declaration_uid_kinds
                        .insert(uid, DeclarationKind::Extractor);
                    self.scope.define_with_id(name, uid);
                }
                Ast::ResultCtorDecl(span, name, _, _, _) => {
                    if !declared_in_batch.insert(name.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    if !self.allow_top_level_shadowing && self.scope.lookup(name).is_some() {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: span.clone(),
                            related_labels: Vec::new(),
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
                    self.declaration_uid_kinds
                        .insert(uid, DeclarationKind::ResultCtor);
                    self.scope.define_with_id(name, uid);
                    define_surface_alias(&mut self.scope, name, uid);
                }
                Ast::BuiltinTypeDecl(span, head, _) => {
                    let surface = surface_name(&head.name).to_string();
                    if !declared_in_batch.insert(surface.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", surface),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    if !self.allow_top_level_shadowing && self.scope.lookup(&head.name).is_some() {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", surface),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    let uid = self
                        .declaration_uids
                        .get(&head.name)
                        .copied()
                        .unwrap_or_else(|| {
                            let fresh = self.scope.reserve_id();
                            self.declaration_uids.insert(head.name.clone(), fresh);
                            fresh
                        });
                    self.predeclared_ids
                        .entry(head.name.clone())
                        .or_default()
                        .push_back(uid);
                    self.declaration_uid_kinds
                        .insert(uid, DeclarationKind::BuiltinType);
                    self.scope.define_with_id(&head.name, uid);
                    define_surface_alias(&mut self.scope, &head.name, uid);
                }
                Ast::StructDef(span, name, _, _)
                | Ast::RecordDef(span, name, _, _)
                | Ast::DeferrorDef(span, name, _, _, _) => {
                    let surface = surface_name(name).to_string();
                    if is_reserved_builtin_type_redefinition(name) {
                        return Err(ResolveError {
                            message: format!(
                                "Type name `{}` is reserved by a canonical builtin type declaration",
                                name.strip_prefix("Global::").unwrap_or(name)
                            ),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    if !declared_in_batch.insert(surface.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", surface),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    if !self.allow_top_level_shadowing && self.scope.lookup(name).is_some() {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", surface),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    let qualified_name = name.clone();
                    let uid = self
                        .declaration_uids
                        .get(&qualified_name)
                        .copied()
                        .unwrap_or_else(|| {
                            let fresh = self.scope.reserve_id();
                            self.declaration_uids.insert(qualified_name.clone(), fresh);
                            fresh
                        });
                    self.predeclared_ids
                        .entry(name.clone())
                        .or_default()
                        .push_back(uid);
                    let kind = match stmt {
                        Ast::StructDef(..) => DeclarationKind::Struct,
                        Ast::RecordDef(..) => DeclarationKind::Record,
                        Ast::DeferrorDef(_, _, _, _, _) => DeclarationKind::Deferror,
                        _ => unreachable!(),
                    };
                    self.declaration_uid_kinds.insert(uid, kind);
                    self.scope.define_with_id(name, uid);
                    define_surface_alias(&mut self.scope, name, uid);
                }
                Ast::EnumDef(span, name, _, variants, _) => {
                    let surface = surface_name(name).to_string();
                    if is_reserved_builtin_type_redefinition(name) {
                        return Err(ResolveError {
                            message: format!(
                                "Type name `{}` is reserved by a canonical builtin type declaration",
                                name.strip_prefix("Global::").unwrap_or(name)
                            ),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    if !declared_in_batch.insert(surface.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", surface),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    if !self.allow_top_level_shadowing && self.scope.lookup(name).is_some() {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", surface),
                            span: span.clone(),
                            related_labels: Vec::new(),
                        });
                    }
                    let qualified_enum = name.clone();
                    let uid = self
                        .declaration_uids
                        .get(&qualified_enum)
                        .copied()
                        .unwrap_or_else(|| {
                            let fresh = self.scope.reserve_id();
                            self.declaration_uids.insert(qualified_enum.clone(), fresh);
                            fresh
                        });
                    self.predeclared_ids
                        .entry(name.clone())
                        .or_default()
                        .push_back(uid);
                    self.declaration_uid_kinds
                        .insert(uid, DeclarationKind::Enum);
                    self.scope.define_with_id(name, uid);
                    define_surface_alias(&mut self.scope, name, uid);

                    for variant in variants {
                        let qualified_ctor = format!("{}::{}", name, variant.name);
                        if !declared_in_batch.insert(qualified_ctor.clone()) {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate top-level definition: {}",
                                    qualified_ctor
                                ),
                                span: variant.span.clone(),
                                related_labels: Vec::new(),
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
                                related_labels: Vec::new(),
                            });
                        }
                        let ctor_uid = self
                            .declaration_uids
                            .get(&qualified_ctor)
                            .copied()
                            .unwrap_or_else(|| {
                                let fresh = self.scope.reserve_id();
                                self.declaration_uids.insert(qualified_ctor.clone(), fresh);
                                fresh
                            });
                        self.predeclared_ids
                            .entry(qualified_ctor.clone())
                            .or_default()
                            .push_back(ctor_uid);
                        self.declaration_uid_kinds
                            .insert(ctor_uid, DeclarationKind::EnumVariant);
                        self.scope.define_with_id(&qualified_ctor, ctor_uid);
                        define_surface_alias(&mut self.scope, &qualified_ctor, ctor_uid);
                    }
                }
                Ast::TraitImplDef(_, _, _, _, _, _) => {}
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
