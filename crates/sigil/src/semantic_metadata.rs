use sindr::ir::{DocEntry, DocKind, SignatureEntry};
use sindr::names::surface_path_name;
use spire::ast::{
    Ast, AstTy, BuiltinTypeHead, EnumVariant, ExtractorParam, FunParam, RecordField,
    TraitMethodSig, TypeParam, WhereClause, WhereConstraintRhs,
};

use crate::StagedModuleAst;

fn format_ast_ty(ty: &AstTy) -> String {
    match ty {
        AstTy::Named(_, name) => surface_path_name(name).to_string(),
        AstTy::ImplTrait(_, name) => format!("impl {}", surface_path_name(name)),
        AstTy::Generic(_, name, args) => {
            let args = args
                .iter()
                .map(format_ast_ty)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}<{args}>", surface_path_name(name))
        }
        AstTy::Tuple(_, items) => {
            let items = items
                .iter()
                .map(format_ast_ty)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({items})")
        }
        AstTy::Func(_, params, ret) => {
            if params.is_empty() {
                format!("(-> {})", format_ast_ty(ret))
            } else {
                let params = params
                    .iter()
                    .map(format_ast_ty)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({params} -> {})", format_ast_ty(ret))
            }
        }
    }
}

fn rewrite_self_ast_ty(ty: &AstTy, self_ty: &AstTy) -> AstTy {
    match ty {
        AstTy::Named(_, name) if name == "Self" => self_ty.clone(),
        AstTy::Named(_, _) | AstTy::ImplTrait(_, _) => ty.clone(),
        AstTy::Generic(span, name, args) => AstTy::Generic(
            span.clone(),
            name.clone(),
            args.iter()
                .map(|arg| rewrite_self_ast_ty(arg, self_ty))
                .collect(),
        ),
        AstTy::Tuple(span, items) => AstTy::Tuple(
            span.clone(),
            items
                .iter()
                .map(|item| rewrite_self_ast_ty(item, self_ty))
                .collect(),
        ),
        AstTy::Func(span, params, ret) => AstTy::Func(
            span.clone(),
            params
                .iter()
                .map(|param| rewrite_self_ast_ty(param, self_ty))
                .collect(),
            Box::new(rewrite_self_ast_ty(ret, self_ty)),
        ),
    }
}

fn format_type_params(type_params: &[TypeParam]) -> String {
    if type_params.is_empty() {
        String::new()
    } else {
        let params = type_params
            .iter()
            .map(|param| match &param.bound {
                Some(bound) => format!("{}: {}", param.name, bound),
                None => param.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("<{params}>")
    }
}

fn format_fun_signature(
    name: &str,
    _type_params: &[TypeParam],
    params: &[FunParam],
    ret_ty: &Option<AstTy>,
) -> String {
    let params = params
        .iter()
        .map(|param| format!("{}: {}", param.name, format_ast_ty(&param.ty)))
        .collect::<Vec<_>>()
        .join(", ");
    match ret_ty {
        Some(ret) => format!("{name}({params}) -> {}", format_ast_ty(ret)),
        None => format!("{name}({params})"),
    }
}

fn format_extractor_signature(
    name: &str,
    _type_params: &[TypeParam],
    param: &ExtractorParam,
    ret_ty: &AstTy,
) -> String {
    let param = match &param.ty {
        Some(ty) => format!("{}: {}", param.name, format_ast_ty(ty)),
        None => param.name.clone(),
    };
    format!("{name}({param}) -> {}", format_ast_ty(ret_ty))
}

fn format_result_ctor_signature(name: &str, param_ty: &AstTy, ret_ty: &AstTy) -> String {
    format!(
        "{name}({}) -> {}",
        format_ast_ty(param_ty),
        format_ast_ty(ret_ty)
    )
}

fn format_builtin_type_signature(head: &BuiltinTypeHead) -> String {
    if head.params.is_empty() {
        format!("type {}", surface_path_name(&head.name))
    } else {
        format!(
            "type {}<{}>",
            surface_path_name(&head.name),
            head.params.join(", ")
        )
    }
}

fn format_deferror_signature(name: &str, fields: &[RecordField]) -> String {
    if fields.is_empty() {
        format!("deferror {}", surface_path_name(name))
    } else {
        let fields = fields
            .iter()
            .map(|field| format!("{}: {}", field.name, format_ast_ty(&field.ty)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("deferror {}({fields})", surface_path_name(name))
    }
}

fn format_defenum_signature(name: &str, variants: &[EnumVariant]) -> String {
    if variants.is_empty() {
        return format!("defenum {}", surface_path_name(name));
    }
    let variants = variants
        .iter()
        .map(|variant| {
            if variant.payload.is_empty() {
                variant.name.clone()
            } else {
                let payload = variant
                    .payload
                    .iter()
                    .map(format_ast_ty)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({payload})", variant.name)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("defenum {} {{ {variants} }}", surface_path_name(name))
}

fn builtin_special_enum_variant_signature(
    enum_name: &str,
    type_params: &[TypeParam],
    variant: &EnumVariant,
) -> Option<String> {
    match (surface_path_name(enum_name), variant.name.as_str()) {
        ("Result", "Ok") => {
            let ok_ty = variant
                .payload
                .first()
                .map(format_ast_ty)
                .unwrap_or_else(|| "$T".to_string());
            Some(format!("Ok({ok_ty}) -> Result<{ok_ty}, Error>"))
        }
        ("Result", "Err") => Some("Err(Error) -> Result<$T, Error>".to_string()),
        ("Boolean", "True") if type_params.is_empty() && variant.payload.is_empty() => {
            Some("True() -> Boolean".to_string())
        }
        ("Boolean", "False") if type_params.is_empty() && variant.payload.is_empty() => {
            Some("False() -> Boolean".to_string())
        }
        _ => None,
    }
}

fn format_generic_struct_signature(name: &str, type_params: &[TypeParam]) -> String {
    let type_params = format_type_params(type_params);
    format!("defstruct {}{type_params}", surface_path_name(name))
}

fn format_record_signature(name: &str) -> String {
    format!("defrecord {}", surface_path_name(name))
}

fn format_impl_method_signature(
    target: &str,
    name: &str,
    type_params: &[TypeParam],
    params: &[FunParam],
    ret_ty: &Option<AstTy>,
) -> String {
    let self_ty = AstTy::Named(spire::ast::Span { start: 0, end: 0 }, target.to_string());
    let params = params
        .iter()
        .map(|param| {
            format!(
                "{}: {}",
                param.name,
                format_ast_ty(&rewrite_self_ast_ty(&param.ty, &self_ty))
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let type_params = format_type_params(type_params);
    let signature = match ret_ty {
        Some(ret) => format!(
            "{name}{type_params}({params}) -> {}",
            format_ast_ty(&rewrite_self_ast_ty(ret, &self_ty))
        ),
        None => format!("{name}{type_params}({params})"),
    };
    if let Some(rest) = signature.strip_prefix(name) {
        format!("{}::{name}{rest}", surface_path_name(target))
    } else {
        signature
    }
}

fn format_impl_extractor_signature(
    target: &str,
    name: &str,
    type_params: &[TypeParam],
    param: &ExtractorParam,
    ret_ty: &AstTy,
) -> String {
    let self_ty = AstTy::Named(spire::ast::Span { start: 0, end: 0 }, target.to_string());
    let type_params = format_type_params(type_params);
    let param = match &param.ty {
        Some(ty) => format!(
            "{}: {}",
            param.name,
            format_ast_ty(&rewrite_self_ast_ty(ty, &self_ty))
        ),
        None => param.name.clone(),
    };
    let signature = format!(
        "{name}{type_params}({param}) -> {}",
        format_ast_ty(&rewrite_self_ast_ty(ret_ty, &self_ty))
    );
    if let Some(rest) = signature.strip_prefix(name) {
        format!("{}::{name}{rest}", surface_path_name(target))
    } else {
        signature
    }
}

fn format_trait_method_signature(trait_name: &str, method: &TraitMethodSig) -> String {
    let signature = format_fun_signature(
        &method.name,
        &method.type_params,
        &method.params,
        &Some(method.ret_ty.clone()),
    );
    if let Some(rest) = signature.strip_prefix(&method.name) {
        format!("{}::{}{}", surface_path_name(trait_name), method.name, rest)
    } else {
        signature
    }
}

fn format_trait_impl_signature(
    trait_name: &str,
    trait_args: &[AstTy],
    target_ty: &AstTy,
) -> String {
    if trait_args.is_empty() {
        format!("impl {trait_name} for {}", format_ast_ty(target_ty))
    } else {
        let args = trait_args
            .iter()
            .map(format_ast_ty)
            .collect::<Vec<_>>()
            .join(", ");
        format!("impl {trait_name}<{args}> for {}", format_ast_ty(target_ty))
    }
}

fn format_trait_impl_method_signature(
    trait_name: &str,
    trait_args: &[AstTy],
    target_ty: &AstTy,
    method_name: &str,
    type_params: &[TypeParam],
    params: &[FunParam],
    ret_ty: &Option<AstTy>,
    where_clause: Option<&WhereClause>,
) -> String {
    let params = params
        .iter()
        .map(|param| {
            format!(
                "{}: {}",
                param.name,
                format_ast_ty(&rewrite_self_ast_ty(&param.ty, target_ty))
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let type_params = format_type_params(type_params);
    let method_sig = match ret_ty {
        Some(ret) => format!(
            "{method_name}{type_params}({params}) -> {}",
            format_ast_ty(&rewrite_self_ast_ty(ret, target_ty))
        ),
        None => format!("{method_name}{type_params}({params})"),
    };
    let impl_sig = format_trait_impl_signature(trait_name, trait_args, target_ty);
    format!(
        "{impl_sig}::{method_sig}{}",
        format_where_clause(where_clause)
    )
}

fn format_where_clause(where_clause: Option<&WhereClause>) -> String {
    let Some(where_clause) = where_clause else {
        return String::new();
    };
    let constraints = where_clause
        .constraints
        .iter()
        .map(|constraint| {
            let bounds = constraint
                .bounds
                .iter()
                .map(|bound| match bound {
                    WhereConstraintRhs::Trait(_, name) => name.clone(),
                    WhereConstraintRhs::TypeConstructor(_, slots) => format!(
                        "Type<{}>",
                        slots
                            .iter()
                            .map(format_ast_ty)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    WhereConstraintRhs::TraitSlot(_, owner, slot) => {
                        format!("{owner}.{slot}")
                    }
                })
                .collect::<Vec<_>>()
                .join(" + ");
            format!("{}: {bounds}", format_ast_ty(&constraint.subject))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(" where {constraints}")
}

fn qualified_name(module_path: &str, name: &str) -> String {
    let module_path = surface_path_name(module_path);
    let name = surface_path_name(name);
    if module_path.is_empty() {
        name.to_string()
    } else if name
        .strip_prefix(module_path)
        .is_some_and(|rest| rest.starts_with("::"))
    {
        name.to_string()
    } else {
        format!("{module_path}::{name}")
    }
}

fn collect_doc_entries_for_ast(ast: &[Ast], module_path: &str, out: &mut Vec<DocEntry>) {
    for stmt in ast {
        match stmt {
            Ast::Def(_, name, type_params, params, ret_ty, _, _, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(format_fun_signature(name, type_params, params, ret_ty)),
                        doc: doc.clone(),
                    });
                }
            }
            Ast::BuiltinDecl(_, name, params, ret_ty, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(format_fun_signature(name, &[], params, ret_ty)),
                        doc: doc.clone(),
                    });
                }
            }
            Ast::IntrinsicDecl(_, name, signature, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(signature.clone()),
                        doc: doc.clone(),
                    });
                }
            }
            Ast::ExtractorDef(_, name, type_params, param, ret_ty, _, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(format_extractor_signature(
                            name,
                            type_params,
                            param,
                            ret_ty,
                        )),
                        doc: doc.clone(),
                    });
                }
            }
            Ast::BuiltinExtractorDecl(_, name, param, ret_ty, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(format_extractor_signature(name, &[], param, ret_ty)),
                        doc: doc.clone(),
                    });
                }
            }
            Ast::TraitDef(_, name, _type_params, _, methods, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Type,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(format!(
                            "trait {} {{ {} }}",
                            surface_path_name(name),
                            methods
                                .iter()
                                .map(|method| format_fun_signature(
                                    &method.name,
                                    &method.type_params,
                                    &method.params,
                                    &Some(method.ret_ty.clone()),
                                ))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                        doc: doc.clone(),
                    });
                    for method in methods {
                        out.push(DocEntry {
                            qualified_name: qualified_name(
                                module_path,
                                &format!("{name}::{}", method.name),
                            ),
                            kind: DocKind::Function,
                            module_path: surface_path_name(module_path).to_string(),
                            signature: Some(format_trait_method_signature(name, method)),
                            doc: doc.clone(),
                        });
                    }
                }
            }
            Ast::StructDef(_, name, type_params, _fields, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Type,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(format_generic_struct_signature(name, type_params)),
                        doc: doc.clone(),
                    });
                }
            }
            Ast::RecordDef(_, name, _, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Type,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(format_record_signature(name)),
                        doc: doc.clone(),
                    });
                }
            }
            Ast::ImplDef(_, target, methods, _attrs) => {
                for method in methods {
                    match method {
                        Ast::Def(_, name, type_params, params, ret_ty, _, _, attrs) => {
                            if let Some(doc) = &attrs.doc {
                                let qualified_method_name = if surface_path_name(module_path)
                                    == surface_path_name(target)
                                {
                                    format!("{}::{name}", surface_path_name(target))
                                } else {
                                    qualified_name(module_path, &format!("{target}::{name}"))
                                };
                                out.push(DocEntry {
                                    qualified_name: qualified_method_name,
                                    kind: DocKind::Function,
                                    module_path: surface_path_name(module_path).to_string(),
                                    signature: Some(format_impl_method_signature(
                                        target,
                                        name,
                                        type_params,
                                        params,
                                        ret_ty,
                                    )),
                                    doc: doc.clone(),
                                });
                            }
                        }
                        Ast::BuiltinDecl(_, name, params, ret_ty, attrs) => {
                            if let Some(doc) = &attrs.doc {
                                let qualified_method_name = if surface_path_name(module_path)
                                    == surface_path_name(target)
                                {
                                    format!("{}::{name}", surface_path_name(target))
                                } else {
                                    qualified_name(module_path, &format!("{target}::{name}"))
                                };
                                out.push(DocEntry {
                                    qualified_name: qualified_method_name,
                                    kind: DocKind::Function,
                                    module_path: surface_path_name(module_path).to_string(),
                                    signature: Some(format_impl_method_signature(
                                        target,
                                        name,
                                        &[],
                                        params,
                                        ret_ty,
                                    )),
                                    doc: doc.clone(),
                                });
                            }
                        }
                        Ast::ExtractorDef(_, name, type_params, param, ret_ty, _, attrs) => {
                            if let Some(doc) = &attrs.doc {
                                let qualified_method_name = if surface_path_name(module_path)
                                    == surface_path_name(target)
                                {
                                    format!("{}::{name}", surface_path_name(target))
                                } else {
                                    qualified_name(module_path, &format!("{target}::{name}"))
                                };
                                out.push(DocEntry {
                                    qualified_name: qualified_method_name,
                                    kind: DocKind::Function,
                                    module_path: surface_path_name(module_path).to_string(),
                                    signature: Some(format_impl_extractor_signature(
                                        target,
                                        name,
                                        type_params,
                                        param,
                                        ret_ty,
                                    )),
                                    doc: doc.clone(),
                                });
                            }
                        }
                        Ast::BuiltinExtractorDecl(_, name, param, ret_ty, attrs) => {
                            if let Some(doc) = &attrs.doc {
                                let qualified_method_name = if surface_path_name(module_path)
                                    == surface_path_name(target)
                                {
                                    format!("{}::{name}", surface_path_name(target))
                                } else {
                                    qualified_name(module_path, &format!("{target}::{name}"))
                                };
                                out.push(DocEntry {
                                    qualified_name: qualified_method_name,
                                    kind: DocKind::Function,
                                    module_path: surface_path_name(module_path).to_string(),
                                    signature: Some(format_impl_extractor_signature(
                                        target,
                                        name,
                                        &[],
                                        param,
                                        ret_ty,
                                    )),
                                    doc: doc.clone(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ast::TraitImplDef(_, trait_name, trait_args, target_ty, _, methods, attrs) => {
                if let Some(doc) = &attrs.doc {
                    let rendered = format_trait_impl_signature(trait_name, trait_args, target_ty);
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, &rendered),
                        kind: DocKind::Type,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(rendered),
                        doc: doc.clone(),
                    });
                }
                for method in methods {
                    let method_parts = match method {
                        Ast::Def(
                            _,
                            name,
                            type_params,
                            params,
                            ret_ty,
                            where_clause,
                            _,
                            method_attrs,
                        ) => Some((
                            name,
                            type_params.as_slice(),
                            params.as_slice(),
                            ret_ty,
                            where_clause.as_ref(),
                            method_attrs,
                        )),
                        Ast::BuiltinDecl(_, name, params, ret_ty, method_attrs) => Some((
                            name,
                            [].as_slice(),
                            params.as_slice(),
                            ret_ty,
                            None,
                            method_attrs,
                        )),
                        _ => None,
                    };
                    if let Some((name, type_params, params, ret_ty, where_clause, method_attrs)) =
                        method_parts
                    {
                        if let Some(doc) = &method_attrs.doc {
                            let rendered = format_trait_impl_method_signature(
                                trait_name,
                                trait_args,
                                target_ty,
                                name,
                                type_params,
                                params,
                                ret_ty,
                                where_clause,
                            );
                            out.push(DocEntry {
                                qualified_name: qualified_name(
                                    module_path,
                                    &format!(
                                        "{}::{}",
                                        format_trait_impl_signature(
                                            trait_name, trait_args, target_ty,
                                        ),
                                        name
                                    ),
                                ),
                                kind: DocKind::Function,
                                module_path: surface_path_name(module_path).to_string(),
                                signature: Some(rendered),
                                doc: doc.clone(),
                            });
                        }
                    }
                }
            }
            Ast::BuiltinTypeDecl(_, head, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, &head.name),
                        kind: DocKind::Type,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(format_builtin_type_signature(head)),
                        doc: doc.clone(),
                    });
                }
            }
            Ast::ResultCtorDecl(_, name, param_ty, ret_ty, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(format_result_ctor_signature(name, param_ty, ret_ty)),
                        doc: doc.clone(),
                    });
                }
            }
            Ast::DeferrorDef(_, name, fields, _, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Type,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(format_deferror_signature(name, fields)),
                        doc: doc.clone(),
                    });
                }
            }
            Ast::EnumDef(_, name, type_params, variants, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Type,
                        module_path: surface_path_name(module_path).to_string(),
                        signature: Some(format_defenum_signature(name, variants)),
                        doc: doc.clone(),
                    });
                }
                if attrs.builtin {
                    for variant in variants {
                        let Some(signature) =
                            builtin_special_enum_variant_signature(name, type_params, variant)
                        else {
                            continue;
                        };
                        out.push(DocEntry {
                            qualified_name: qualified_name(
                                module_path,
                                &format!("{name}::{}", variant.name),
                            ),
                            kind: DocKind::Function,
                            module_path: surface_path_name(module_path).to_string(),
                            signature: Some(signature),
                            doc: String::new(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

fn push_signature_entry(
    out: &mut Vec<SignatureEntry>,
    module_path: &str,
    qualified_name: String,
    kind: DocKind,
    signature: String,
) {
    out.push(SignatureEntry {
        qualified_name,
        kind,
        module_path: surface_path_name(module_path).to_string(),
        signature,
    });
}

fn collect_signature_entries_for_ast(
    ast: &[Ast],
    module_path: &str,
    out: &mut Vec<SignatureEntry>,
) {
    for stmt in ast {
        match stmt {
            Ast::Def(_, name, type_params, params, ret_ty, _, _, _) => {
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, name),
                    DocKind::Function,
                    format_fun_signature(name, type_params, params, ret_ty),
                );
            }
            Ast::BuiltinDecl(_, name, params, ret_ty, _) => {
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, name),
                    DocKind::Function,
                    format_fun_signature(name, &[], params, ret_ty),
                );
            }
            Ast::IntrinsicDecl(_, name, signature, _) => {
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, name),
                    DocKind::Function,
                    signature.clone(),
                );
            }
            Ast::ExtractorDef(_, name, type_params, param, ret_ty, _, _) => {
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, name),
                    DocKind::Function,
                    format_extractor_signature(name, type_params, param, ret_ty),
                );
            }
            Ast::BuiltinExtractorDecl(_, name, param, ret_ty, _) => {
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, name),
                    DocKind::Function,
                    format_extractor_signature(name, &[], param, ret_ty),
                );
            }
            Ast::TraitDef(_, name, _type_params, _, methods, _) => {
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, name),
                    DocKind::Type,
                    format!(
                        "trait {} {{ {} }}",
                        surface_path_name(name),
                        methods
                            .iter()
                            .map(|method| format_fun_signature(
                                &method.name,
                                &method.type_params,
                                &method.params,
                                &Some(method.ret_ty.clone()),
                            ))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
                for method in methods {
                    push_signature_entry(
                        out,
                        module_path,
                        qualified_name(module_path, &format!("{name}::{}", method.name)),
                        DocKind::Function,
                        format_trait_method_signature(name, method),
                    );
                }
            }
            Ast::StructDef(_, name, type_params, _fields, _) => {
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, name),
                    DocKind::Type,
                    format_generic_struct_signature(name, type_params),
                );
            }
            Ast::RecordDef(_, name, _, _) => {
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, name),
                    DocKind::Type,
                    format_record_signature(name),
                );
            }
            Ast::ImplDef(_, target, methods, _) => {
                for method in methods {
                    match method {
                        Ast::Def(_, name, type_params, params, ret_ty, _, _, _) => {
                            let qualified_method_name =
                                if surface_path_name(module_path) == surface_path_name(target) {
                                    format!("{}::{name}", surface_path_name(target))
                                } else {
                                    qualified_name(module_path, &format!("{target}::{name}"))
                                };
                            push_signature_entry(
                                out,
                                module_path,
                                qualified_method_name,
                                DocKind::Function,
                                format_impl_method_signature(
                                    target,
                                    name,
                                    type_params,
                                    params,
                                    ret_ty,
                                ),
                            );
                        }
                        Ast::BuiltinDecl(_, name, params, ret_ty, _) => {
                            let qualified_method_name =
                                if surface_path_name(module_path) == surface_path_name(target) {
                                    format!("{}::{name}", surface_path_name(target))
                                } else {
                                    qualified_name(module_path, &format!("{target}::{name}"))
                                };
                            push_signature_entry(
                                out,
                                module_path,
                                qualified_method_name,
                                DocKind::Function,
                                format_impl_method_signature(target, name, &[], params, ret_ty),
                            );
                        }
                        Ast::ExtractorDef(_, name, type_params, param, ret_ty, _, _) => {
                            let qualified_method_name =
                                if surface_path_name(module_path) == surface_path_name(target) {
                                    format!("{}::{name}", surface_path_name(target))
                                } else {
                                    qualified_name(module_path, &format!("{target}::{name}"))
                                };
                            push_signature_entry(
                                out,
                                module_path,
                                qualified_method_name,
                                DocKind::Function,
                                format_impl_extractor_signature(
                                    target,
                                    name,
                                    type_params,
                                    param,
                                    ret_ty,
                                ),
                            );
                        }
                        Ast::BuiltinExtractorDecl(_, name, param, ret_ty, _) => {
                            let qualified_method_name =
                                if surface_path_name(module_path) == surface_path_name(target) {
                                    format!("{}::{name}", surface_path_name(target))
                                } else {
                                    qualified_name(module_path, &format!("{target}::{name}"))
                                };
                            push_signature_entry(
                                out,
                                module_path,
                                qualified_method_name,
                                DocKind::Function,
                                format_impl_extractor_signature(target, name, &[], param, ret_ty),
                            );
                        }
                        _ => {}
                    }
                }
            }
            Ast::TraitImplDef(_, trait_name, trait_args, target_ty, _, methods, _) => {
                let rendered = format_trait_impl_signature(trait_name, trait_args, target_ty);
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, &rendered),
                    DocKind::Type,
                    rendered.clone(),
                );
                for method in methods {
                    let method_parts = match method {
                        Ast::Def(_, name, type_params, params, ret_ty, where_clause, _, _) => {
                            Some((
                                name,
                                type_params.as_slice(),
                                params.as_slice(),
                                ret_ty,
                                where_clause.as_ref(),
                            ))
                        }
                        Ast::BuiltinDecl(_, name, params, ret_ty, _) => {
                            Some((name, [].as_slice(), params.as_slice(), ret_ty, None))
                        }
                        _ => None,
                    };
                    if let Some((name, type_params, params, ret_ty, where_clause)) = method_parts {
                        let rendered = format_trait_impl_method_signature(
                            trait_name,
                            trait_args,
                            target_ty,
                            name,
                            type_params,
                            params,
                            ret_ty,
                            where_clause,
                        );
                        push_signature_entry(
                            out,
                            module_path,
                            qualified_name(
                                module_path,
                                &format!(
                                    "{}::{}",
                                    format_trait_impl_signature(trait_name, trait_args, target_ty),
                                    name
                                ),
                            ),
                            DocKind::Function,
                            rendered,
                        );
                    }
                }
            }
            Ast::BuiltinTypeDecl(_, head, _) => {
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, &head.name),
                    DocKind::Type,
                    format_builtin_type_signature(head),
                );
            }
            Ast::ResultCtorDecl(_, name, param_ty, ret_ty, _) => {
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, name),
                    DocKind::Function,
                    format_result_ctor_signature(name, param_ty, ret_ty),
                );
            }
            Ast::DeferrorDef(_, name, fields, _, _) => {
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, name),
                    DocKind::Type,
                    format_deferror_signature(name, fields),
                );
            }
            Ast::EnumDef(_, name, type_params, variants, attrs) => {
                push_signature_entry(
                    out,
                    module_path,
                    qualified_name(module_path, name),
                    DocKind::Type,
                    format_defenum_signature(name, variants),
                );
                if attrs.builtin {
                    for variant in variants {
                        let Some(signature) =
                            builtin_special_enum_variant_signature(name, type_params, variant)
                        else {
                            continue;
                        };
                        push_signature_entry(
                            out,
                            module_path,
                            qualified_name(module_path, &format!("{name}::{}", variant.name)),
                            DocKind::Function,
                            signature,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_doc_entries_into(
    docs: &mut Vec<DocEntry>,
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: &[Ast],
    user_module_path: Option<&str>,
) {
    for stage in module_stages {
        for module in stage {
            let doc_module_path = module
                .doc_module_path
                .as_deref()
                .unwrap_or(module.module_path.as_str());
            if let Some(doc) = &module.module_doc {
                docs.push(DocEntry {
                    qualified_name: doc_module_path.to_string(),
                    kind: DocKind::Module,
                    module_path: doc_module_path.to_string(),
                    signature: None,
                    doc: doc.clone(),
                });
            }
            collect_doc_entries_for_ast(&module.ast, doc_module_path, docs);
        }
    }

    collect_doc_entries_for_ast(user_ast, user_module_path.unwrap_or_default(), docs);
}

fn collect_signature_entries_into(
    signatures: &mut Vec<SignatureEntry>,
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: &[Ast],
    user_module_path: Option<&str>,
) {
    for stage in module_stages {
        for module in stage {
            let doc_module_path = module
                .doc_module_path
                .as_deref()
                .unwrap_or(module.module_path.as_str());
            collect_signature_entries_for_ast(&module.ast, doc_module_path, signatures);
        }
    }

    collect_signature_entries_for_ast(user_ast, user_module_path.unwrap_or_default(), signatures);
}

pub fn collect_doc_entries(
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: &[Ast],
    user_module_path: Option<&str>,
) -> Vec<DocEntry> {
    let mut docs = Vec::new();
    collect_doc_entries_into(&mut docs, module_stages, user_ast, user_module_path);
    docs
}

pub fn collect_signature_entries(
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: &[Ast],
    user_module_path: Option<&str>,
) -> Vec<SignatureEntry> {
    let mut signatures = Vec::new();
    collect_signature_entries_into(&mut signatures, module_stages, user_ast, user_module_path);
    signatures
}

pub fn collect_doc_entries_with_base(
    base_docs: &[DocEntry],
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: &[Ast],
    user_module_path: Option<&str>,
) -> Vec<DocEntry> {
    let mut docs = base_docs.to_vec();
    collect_doc_entries_into(&mut docs, module_stages, user_ast, user_module_path);
    docs
}

pub fn collect_signature_entries_with_base(
    base_signatures: &[SignatureEntry],
    module_stages: &[Vec<StagedModuleAst>],
    user_ast: &[Ast],
    user_module_path: Option<&str>,
) -> Vec<SignatureEntry> {
    let mut signatures = base_signatures.to_vec();
    collect_signature_entries_into(&mut signatures, module_stages, user_ast, user_module_path);
    signatures
}
