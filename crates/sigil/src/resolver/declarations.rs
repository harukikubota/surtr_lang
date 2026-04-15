use super::scope_init::initialize_scope;
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct StagedModuleAst {
    pub module_path: String,
    pub ast: Vec<Ast>,
    pub module_doc: Option<String>,
    pub auto_import: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    ResultCtor,
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
    pub auto_import: bool,
    pub visibility: Visibility,
}

pub type DeclarationIndex = BTreeMap<String, DeclarationEntry>;

pub(super) fn is_module_visible_declaration(kind: &DeclarationKind) -> bool {
    !matches!(kind, DeclarationKind::BuiltinType)
}

pub(super) fn is_importable_declaration(kind: &DeclarationKind) -> bool {
    !matches!(
        kind,
        DeclarationKind::BuiltinType | DeclarationKind::ImplCtorNew | DeclarationKind::Struct
    )
}

fn entry_visibility(attrs: &DeclAttrs) -> Visibility {
    attrs.visibility
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

fn ast_ty_key(ty: &AstTy) -> String {
    match ty {
        AstTy::Named(_, name) | AstTy::ImplTrait(_, name) => name.clone(),
        AstTy::Generic(_, name, args) => format!(
            "{}<{}>",
            name,
            args.iter().map(ast_ty_key).collect::<Vec<_>>().join(", ")
        ),
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
                    visibility: field.visibility,
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
        Ast::Semi(span, inner) => Ast::Semi(span, Box::new(rewrite_self_ast(*inner, target))),
        other => other,
    }
}

pub(super) fn assign_declaration_uids(index: &DeclarationIndex) -> HashMap<String, u32> {
    let mut scope = initialize_scope();
    let mut declaration_uids = HashMap::with_capacity(index.len());
    for fq_name in index.keys() {
        declaration_uids.insert(fq_name.clone(), scope.reserve_id());
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
/// `def`, `defextractor`, `@@builtin def`, `@@builtin defextractor`, `@@builtin type`,
/// `defstruct`, `defrecord`, `deferror`.
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
                        let Ast::Def(method_span, method_name, _, _, _, _, attrs) = method else {
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
                                auto_import: false,
                                visibility: entry_visibility(attrs),
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
                            kind: DeclarationKind::Trait,
                            stage_index,
                            auto_import: attrs.auto_import,
                            visibility: Visibility::Public,
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
                                    method_fq_name, prev.stage_index, prev.module_path
                                ),
                                span: method.span.clone(),
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
                                visibility: Visibility::Public,
                            },
                        );
                    }
                    continue;
                }

                if let Ast::TraitImplDef(span, _trait_name, _trait_args, _target_ty, methods) = stmt
                {
                    for method in methods {
                        let Ast::Def(method_span, method_name, _, _, _, _, _) = method else {
                            return Err(ResolveError {
                                message: "trait impl body may only contain `def` declarations"
                                    .to_string(),
                                span: span.clone(),
                            });
                        };
                        let internal_name = trait_impl_method_qualified_name(
                            Some(module.module_path.as_str()),
                            _trait_name,
                            _trait_args,
                            _target_ty,
                            method_name,
                            method_span.start,
                        );
                        if let Some(prev) = index.get(&internal_name) {
                            return Err(ResolveError {
                                message: format!(
                                    "Duplicate fully-qualified declaration: {} (already declared in stage {} module {})",
                                    internal_name, prev.stage_index, prev.module_path
                                ),
                                span: method_span.clone(),
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
                                visibility: Visibility::Private,
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
                            auto_import: false,
                            visibility: Visibility::Public,
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
                                auto_import: false,
                                visibility: Visibility::Public,
                            },
                        );
                    }
                    continue;
                }

                let (span, name, kind, visibility) = match stmt {
                    Ast::Def(span, name, _, _, _, _, attrs) => (
                        span,
                        name.as_str(),
                        DeclarationKind::Def,
                        entry_visibility(attrs),
                    ),
                    Ast::ExtractorDef(span, name, _, _, _, _, attrs) => (
                        span,
                        name.as_str(),
                        DeclarationKind::Extractor,
                        entry_visibility(attrs),
                    ),
                    Ast::BuiltinDecl(span, name, _, _, _) => (
                        span,
                        name.as_str(),
                        DeclarationKind::Def,
                        Visibility::Public,
                    ),
                    Ast::BuiltinExtractorDecl(span, name, _, _, _) => (
                        span,
                        name.as_str(),
                        DeclarationKind::Extractor,
                        Visibility::Public,
                    ),
                    Ast::ImplDef(_, _, _)
                    | Ast::TraitDef(_, _, _, _, _)
                    | Ast::TraitImplDef(_, _, _, _, _) => continue,
                    Ast::ResultCtorDecl(span, name, _, _, _) => (
                        span,
                        name.as_str(),
                        DeclarationKind::ResultCtor,
                        Visibility::Public,
                    ),
                    Ast::BuiltinTypeDecl(span, head, _) => (
                        span,
                        head.name.as_str(),
                        DeclarationKind::BuiltinType,
                        Visibility::Public,
                    ),
                    Ast::StructDef(span, name, _) => (
                        span,
                        name.as_str(),
                        DeclarationKind::Struct,
                        Visibility::Public,
                    ),
                    Ast::RecordDef(span, name, _) => (
                        span,
                        name.as_str(),
                        DeclarationKind::Record,
                        Visibility::Public,
                    ),
                    Ast::DeferrorDef(span, name, _, _, _) => (
                        span,
                        name.as_str(),
                        DeclarationKind::Deferror,
                        Visibility::Public,
                    ),
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
                        auto_import: false,
                        visibility,
                    },
                );
            }
        }
    }

    Ok(index)
}

impl Resolver {
    pub(super) fn lower_impl_defs(&self, stmts: Vec<Ast>) -> Result<Vec<Ast>, ResolveError> {
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
                        let Ast::Def(
                            method_span,
                            method_name,
                            type_params,
                            params,
                            ret_ty,
                            body,
                            attrs,
                        ) = method
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
                            type_params,
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
                }
                Ast::ExtractorDef(span, name, _, _, _, _, _) => {
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
                }
                Ast::TraitDef(span, name, _type_params, methods, _) => {
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
                    self.declaration_uid_kinds.insert(uid, DeclarationKind::Def);
                    self.scope.define_with_id(name, uid);
                }
                Ast::BuiltinExtractorDecl(_, name, _, _, _) => {
                    if !declared_in_batch.insert(name.clone()) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: stmt.span().clone(),
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
                    self.declaration_uid_kinds
                        .insert(uid, DeclarationKind::ResultCtor);
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
                    self.declaration_uid_kinds
                        .insert(uid, DeclarationKind::BuiltinType);
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
                    let kind = match stmt {
                        Ast::StructDef(_, _, _) => DeclarationKind::Struct,
                        Ast::RecordDef(_, _, _) => DeclarationKind::Record,
                        Ast::DeferrorDef(_, _, _, _, _) => DeclarationKind::Deferror,
                        _ => unreachable!(),
                    };
                    self.declaration_uid_kinds.insert(uid, kind);
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
                    let qualified_enum = self.qualify_current_declaration_name(name);
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
                            .unwrap_or_else(|| {
                                let fresh = self.scope.reserve_id();
                                let qualified_ctor_name =
                                    self.qualify_current_declaration_name(&qualified_ctor);
                                self.declaration_uids.insert(qualified_ctor_name, fresh);
                                fresh
                            });
                        self.predeclared_ids
                            .entry(qualified_ctor.clone())
                            .or_default()
                            .push_back(ctor_uid);
                        self.declaration_uid_kinds
                            .insert(ctor_uid, DeclarationKind::EnumVariant);
                        self.scope.define_with_id(&qualified_ctor, ctor_uid);
                    }
                }
                Ast::TraitImplDef(_, _, _, _, _) => {}
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
