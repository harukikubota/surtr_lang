pub mod error_display;
mod loader;
pub mod repl;
pub mod tui;

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub use error_display::ErrorDisplayMode;
pub use loader::{
    collect_additional_default_std_module_inputs, collect_lib_module_inputs,
    collect_module_sources_with_extra_std_sources, collect_module_sources_with_module_file_stages,
    collect_module_sources_with_module_stages, collect_module_sources_with_modules,
    compose_script_compile_sources, derive_primary_module_path, is_default_std_module_file_name,
    is_default_std_module_path, script_pseudo_module_path, CompileSources, LoadError, ModuleInput,
    ModuleSources, SourceDescriptor, SourceKind, StagedModule,
};

use diagnostics::SourceId;
pub use repl::logic::core::{EldrLoadError, ReplEngine, ReplLoadError};
pub use repl::ui::cli::{cli_command, BannerMode, ReplOptions};
use serde::{Deserialize, Serialize};
use sindr::builtin::{BUILTIN_METAS, BUILTIN_TYPE_METAS};
use sindr::ir::{stable_hash_hex, Bytecode, DocEntry, DocKind};
use sindr::policy::{CompileUnitKind, EntryPoint, ExitCodePolicy, RuntimeSourcePolicy};

pub const MODULE_SPAN_STRIDE: usize = 1_000_000;

fn stable_hash_bytes(bytes: &[u8]) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

pub fn module_span_base_for_source(source_id: SourceId) -> usize {
    (source_id.0 as usize + 1) * MODULE_SPAN_STRIDE
}

pub fn rebase_module_ast_spans(
    ast: Vec<spire::ast::Ast>,
    source_id: SourceId,
) -> Vec<spire::ast::Ast> {
    spire::rebase_ast_spans(ast, module_span_base_for_source(source_id))
}

pub fn decode_rebased_module_span(span: &spire::ast::Span) -> Option<(SourceId, spire::ast::Span)> {
    if span.start < MODULE_SPAN_STRIDE {
        return None;
    }
    let bucket = span.start / MODULE_SPAN_STRIDE;
    if bucket == 0 {
        return None;
    }
    let base = bucket * MODULE_SPAN_STRIDE;
    Some((
        SourceId((bucket - 1) as u32),
        spire::ast::Span {
            start: span.start.saturating_sub(base),
            end: span.end.saturating_sub(base),
        },
    ))
}

// ── Public types used by other crates ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct LoweredModuleAst {
    pub module_path: String,
    pub doc_module_path: Option<String>,
    pub ast: Vec<spire::ast::Ast>,
    pub declared_span: Option<spire::ast::Span>,
    pub module_doc: Option<String>,
    pub auto_import: bool,
    pub process_spec: Option<spire::ast::ProcessSpec>,
}

pub(crate) fn lowered_module_is_impl_owner(lowered: &LoweredModuleAst) -> bool {
    matches!(
        lowered
            .ast
            .iter()
            .find(|stmt| !matches!(stmt, spire::ast::Ast::Import(_, _, _))),
        Some(
            spire::ast::Ast::ImplDef(_, _, _, _) | spire::ast::Ast::TraitImplDef(_, _, _, _, _, _)
        )
    )
}

fn partition_nested_imports(
    body: Vec<spire::ast::Ast>,
) -> (Vec<spire::ast::Ast>, Vec<spire::ast::Ast>) {
    let mut imports = Vec::new();
    let mut rest = Vec::new();
    for stmt in body {
        if matches!(stmt, spire::ast::Ast::Import(_, _, _)) {
            imports.push(stmt);
        } else {
            rest.push(stmt);
        }
    }
    (imports, rest)
}

fn first_non_import_index(ast: &[spire::ast::Ast]) -> usize {
    ast.iter()
        .take_while(|stmt| matches!(stmt, spire::ast::Ast::Import(_, _, _)))
        .count()
}

fn find_result_owner_module(lowered: &[LoweredModuleAst]) -> Option<usize> {
    lowered.iter().position(|module| {
        module.module_path == "Result"
            && matches!(
                module
                    .ast
                    .iter()
                    .find(|stmt| !matches!(stmt, spire::ast::Ast::Import(_, _, _))),
                Some(spire::ast::Ast::ImplDef(_, target, _, _)) if target == "Result"
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

fn format_ast_ty(ty: &spire::ast::AstTy) -> String {
    match ty {
        spire::ast::AstTy::Named(_, name) => name.clone(),
        spire::ast::AstTy::ImplTrait(_, name) => format!("impl {name}"),
        spire::ast::AstTy::Generic(_, name, args) => {
            let args = args
                .iter()
                .map(format_ast_ty)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}<{args}>")
        }
        spire::ast::AstTy::Tuple(_, items) => {
            let items = items
                .iter()
                .map(format_ast_ty)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({items})")
        }
        spire::ast::AstTy::Func(_, params, ret) => {
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

fn format_type_params(type_params: &[spire::ast::TypeParam]) -> String {
    if type_params.is_empty() {
        String::new()
    } else {
        let params = type_params
            .iter()
            .map(|param| match &param.bound {
                Some(bound) => format!("${}: {}", param.name, bound),
                None => format!("${}", param.name),
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("<{params}>")
    }
}

fn format_fun_signature(
    name: &str,
    type_params: &[spire::ast::TypeParam],
    params: &[spire::ast::FunParam],
    ret_ty: &Option<spire::ast::AstTy>,
) -> String {
    let type_params = format_type_params(type_params);
    let params = params
        .iter()
        .map(|param| format!("{}: {}", param.name, format_ast_ty(&param.ty)))
        .collect::<Vec<_>>()
        .join(", ");
    match ret_ty {
        Some(ret) => format!("{name}{type_params}({params}) -> {}", format_ast_ty(ret)),
        None => format!("{name}{type_params}({params})"),
    }
}

fn format_extractor_signature(
    name: &str,
    type_params: &[spire::ast::TypeParam],
    param: &spire::ast::ExtractorParam,
    ret_ty: &spire::ast::AstTy,
) -> String {
    let type_params = format_type_params(type_params);
    let param = match &param.ty {
        Some(ty) => format!("{}: {}", param.name, format_ast_ty(ty)),
        None => param.name.clone(),
    };
    format!("{name}{type_params}({param}) -> {}", format_ast_ty(ret_ty))
}

fn format_result_ctor_signature(
    name: &str,
    param_ty: &spire::ast::AstTy,
    ret_ty: &spire::ast::AstTy,
) -> String {
    format!(
        "{name}({}) -> {}",
        format_ast_ty(param_ty),
        format_ast_ty(ret_ty)
    )
}

fn format_builtin_type_signature(head: &spire::ast::BuiltinTypeHead) -> String {
    if head.params.is_empty() {
        format!("type {}", head.name)
    } else {
        format!("type {}<{}>", head.name, head.params.join(", "))
    }
}

fn format_deferror_signature(name: &str, fields: &[spire::ast::RecordField]) -> String {
    if fields.is_empty() {
        format!("deferror {name}")
    } else {
        let fields = fields
            .iter()
            .map(|field| format!("{}: {}", field.name, format_ast_ty(&field.ty)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("deferror {name}({fields})")
    }
}

fn format_defenum_signature(name: &str, variants: &[spire::ast::EnumVariant]) -> String {
    if variants.is_empty() {
        return format!("defenum {name}");
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
                format!("{}({})", variant.name, payload)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("defenum {name} {{ {variants} }}")
}

fn format_struct_signature(name: &str) -> String {
    format!("defstruct {name}")
}

fn format_record_signature(name: &str) -> String {
    format!("defrecord {name}")
}

fn format_impl_method_signature(
    target: &str,
    name: &str,
    type_params: &[spire::ast::TypeParam],
    params: &[spire::ast::FunParam],
    ret_ty: &Option<spire::ast::AstTy>,
) -> String {
    let signature = format_fun_signature(name, type_params, params, ret_ty);
    if let Some(rest) = signature.strip_prefix(name) {
        format!("{target}::{name}{rest}")
    } else {
        signature
    }
}

fn format_impl_extractor_signature(
    target: &str,
    name: &str,
    type_params: &[spire::ast::TypeParam],
    param: &spire::ast::ExtractorParam,
    ret_ty: &spire::ast::AstTy,
) -> String {
    let signature = format_extractor_signature(name, type_params, param, ret_ty);
    if let Some(rest) = signature.strip_prefix(name) {
        format!("{target}::{name}{rest}")
    } else {
        signature
    }
}

fn format_trait_impl_signature(
    trait_name: &str,
    trait_args: &[spire::ast::AstTy],
    target_ty: &spire::ast::AstTy,
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
    trait_args: &[spire::ast::AstTy],
    target_ty: &spire::ast::AstTy,
    method_name: &str,
    type_params: &[spire::ast::TypeParam],
    params: &[spire::ast::FunParam],
    ret_ty: &Option<spire::ast::AstTy>,
) -> String {
    let method_sig = format_fun_signature(method_name, type_params, params, ret_ty);
    let impl_sig = format_trait_impl_signature(trait_name, trait_args, target_ty);
    format!("{impl_sig}::{method_sig}")
}

fn qualified_name(module_path: &str, name: &str) -> String {
    if module_path.is_empty() {
        name.to_string()
    } else {
        format!("{module_path}::{name}")
    }
}

fn collect_doc_entries_for_ast(
    ast: &[spire::ast::Ast],
    module_path: &str,
    out: &mut Vec<DocEntry>,
) {
    for stmt in ast {
        match stmt {
            spire::ast::Ast::Def(_, name, type_params, params, ret_ty, _, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: module_path.to_string(),
                        signature: Some(format_fun_signature(name, type_params, params, ret_ty)),
                        doc: doc.clone(),
                    });
                }
            }
            spire::ast::Ast::BuiltinDecl(_, name, params, ret_ty, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: module_path.to_string(),
                        signature: Some(format_fun_signature(name, &[], params, ret_ty)),
                        doc: doc.clone(),
                    });
                }
            }
            spire::ast::Ast::IntrinsicDecl(_, name, signature, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: module_path.to_string(),
                        signature: Some(signature.clone()),
                        doc: doc.clone(),
                    });
                }
            }
            spire::ast::Ast::ExtractorDef(_, name, type_params, param, ret_ty, _, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: module_path.to_string(),
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
            spire::ast::Ast::BuiltinExtractorDecl(_, name, param, ret_ty, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: module_path.to_string(),
                        signature: Some(format_extractor_signature(name, &[], param, ret_ty)),
                        doc: doc.clone(),
                    });
                }
            }
            spire::ast::Ast::TraitDef(_, name, _type_params, methods, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Type,
                        module_path: module_path.to_string(),
                        signature: Some(format!(
                            "trait {} {{ {} }}",
                            name,
                            methods
                                .iter()
                                .map(|method| format!(
                                    "{}",
                                    format_fun_signature(
                                        &method.name,
                                        &method.type_params,
                                        &method.params,
                                        &Some(method.ret_ty.clone()),
                                    )
                                ))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                        doc: doc.clone(),
                    });
                }
            }
            spire::ast::Ast::StructDef(_, name, _, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Type,
                        module_path: module_path.to_string(),
                        signature: Some(format_struct_signature(name)),
                        doc: doc.clone(),
                    });
                }
            }
            spire::ast::Ast::RecordDef(_, name, _, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Type,
                        module_path: module_path.to_string(),
                        signature: Some(format_record_signature(name)),
                        doc: doc.clone(),
                    });
                }
            }
            spire::ast::Ast::ImplDef(_, target, methods, _attrs) => {
                for method in methods {
                    match method {
                        spire::ast::Ast::Def(_, name, type_params, params, ret_ty, _, attrs) => {
                            if let Some(doc) = &attrs.doc {
                                let qualified_method_name = if module_path == target {
                                    format!("{target}::{name}")
                                } else {
                                    qualified_name(module_path, &format!("{target}::{name}"))
                                };
                                out.push(DocEntry {
                                    qualified_name: qualified_method_name,
                                    kind: DocKind::Function,
                                    module_path: module_path.to_string(),
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
                        spire::ast::Ast::BuiltinDecl(_, name, params, ret_ty, attrs) => {
                            if let Some(doc) = &attrs.doc {
                                let qualified_method_name = if module_path == target {
                                    format!("{target}::{name}")
                                } else {
                                    qualified_name(module_path, &format!("{target}::{name}"))
                                };
                                out.push(DocEntry {
                                    qualified_name: qualified_method_name,
                                    kind: DocKind::Function,
                                    module_path: module_path.to_string(),
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
                        spire::ast::Ast::ExtractorDef(
                            _,
                            name,
                            type_params,
                            param,
                            ret_ty,
                            _,
                            attrs,
                        ) => {
                            if let Some(doc) = &attrs.doc {
                                let qualified_method_name = if module_path == target {
                                    format!("{target}::{name}")
                                } else {
                                    qualified_name(module_path, &format!("{target}::{name}"))
                                };
                                out.push(DocEntry {
                                    qualified_name: qualified_method_name,
                                    kind: DocKind::Function,
                                    module_path: module_path.to_string(),
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
                        spire::ast::Ast::BuiltinExtractorDecl(_, name, param, ret_ty, attrs) => {
                            if let Some(doc) = &attrs.doc {
                                let qualified_method_name = if module_path == target {
                                    format!("{target}::{name}")
                                } else {
                                    qualified_name(module_path, &format!("{target}::{name}"))
                                };
                                out.push(DocEntry {
                                    qualified_name: qualified_method_name,
                                    kind: DocKind::Function,
                                    module_path: module_path.to_string(),
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
            spire::ast::Ast::TraitImplDef(_, trait_name, trait_args, target_ty, methods, attrs) => {
                if let Some(doc) = &attrs.doc {
                    let rendered = format_trait_impl_signature(trait_name, trait_args, target_ty);
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, &rendered),
                        kind: DocKind::Type,
                        module_path: module_path.to_string(),
                        signature: Some(rendered),
                        doc: doc.clone(),
                    });
                }
                for method in methods {
                    let method_parts = match method {
                        spire::ast::Ast::Def(
                            _,
                            name,
                            type_params,
                            params,
                            ret_ty,
                            _,
                            method_attrs,
                        ) => Some((
                            name,
                            type_params.as_slice(),
                            params.as_slice(),
                            ret_ty,
                            method_attrs,
                        )),
                        spire::ast::Ast::BuiltinDecl(_, name, params, ret_ty, method_attrs) => {
                            Some((name, [].as_slice(), params.as_slice(), ret_ty, method_attrs))
                        }
                        _ => None,
                    };
                    if let Some((name, type_params, params, ret_ty, method_attrs)) = method_parts {
                        if let Some(doc) = &method_attrs.doc {
                            let rendered = format_trait_impl_method_signature(
                                trait_name,
                                trait_args,
                                target_ty,
                                name,
                                type_params,
                                params,
                                ret_ty,
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
                                module_path: module_path.to_string(),
                                signature: Some(rendered),
                                doc: doc.clone(),
                            });
                        }
                    }
                }
            }
            spire::ast::Ast::BuiltinTypeDecl(_, head, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, &head.name),
                        kind: DocKind::Type,
                        module_path: module_path.to_string(),
                        signature: Some(format_builtin_type_signature(head)),
                        doc: doc.clone(),
                    });
                }
            }
            spire::ast::Ast::ResultCtorDecl(_, name, param_ty, ret_ty, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: module_path.to_string(),
                        signature: Some(format_result_ctor_signature(name, param_ty, ret_ty)),
                        doc: doc.clone(),
                    });
                }
            }
            spire::ast::Ast::DeferrorDef(_, name, fields, _, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Type,
                        module_path: module_path.to_string(),
                        signature: Some(format_deferror_signature(name, fields)),
                        doc: doc.clone(),
                    });
                }
            }
            spire::ast::Ast::EnumDef(_, name, _, variants, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Type,
                        module_path: module_path.to_string(),
                        signature: Some(format_defenum_signature(name, variants)),
                        doc: doc.clone(),
                    });
                }
            }
            _ => {}
        }
    }
}

/// Collect doc metadata from lowered std/user modules so it can be attached to
/// REPL chunks and serialized `.eldr` artifacts.
pub fn collect_doc_entries(
    module_stages: &[Vec<sigil::StagedModuleAst>],
    user_ast: &[spire::ast::Ast],
    user_module_path: Option<&str>,
) -> Vec<DocEntry> {
    let mut docs = Vec::new();
    collect_doc_entries_into(&mut docs, module_stages, user_ast, user_module_path);
    docs
}

/// Collect doc metadata while reusing already-collected prefix docs, such as
/// the default stdlib docs stored in the semantic snapshot.
pub fn collect_doc_entries_with_base(
    base_docs: &[DocEntry],
    module_stages: &[Vec<sigil::StagedModuleAst>],
    user_ast: &[spire::ast::Ast],
    user_module_path: Option<&str>,
) -> Vec<DocEntry> {
    let mut docs = base_docs.to_vec();
    collect_doc_entries_into(&mut docs, module_stages, user_ast, user_module_path);
    docs
}

fn collect_doc_entries_into(
    docs: &mut Vec<DocEntry>,
    module_stages: &[Vec<sigil::StagedModuleAst>],
    user_ast: &[spire::ast::Ast],
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

#[derive(Debug, Clone, PartialEq)]
pub enum ModuleStageParseErrorKind {
    Parse {
        message: String,
        span: spire::ast::Span,
    },
    DuplicateModulePath {
        module_path: String,
        first_file_name: String,
        second_file_name: String,
        span: spire::ast::Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleStageParseError {
    pub source_id: diagnostics::SourceId,
    pub kind: ModuleStageParseErrorKind,
}

impl ModuleStageParseError {
    pub fn message(&self) -> String {
        match &self.kind {
            ModuleStageParseErrorKind::Parse { message, .. } => message.clone(),
            ModuleStageParseErrorKind::DuplicateModulePath {
                module_path,
                first_file_name,
                second_file_name,
                ..
            } => format!(
                "duplicate module path `{}` in `{}` and `{}`",
                module_path, first_file_name, second_file_name
            ),
        }
    }

    pub fn span(&self) -> spire::ast::Span {
        match &self.kind {
            ModuleStageParseErrorKind::Parse { span, .. } => span.clone(),
            ModuleStageParseErrorKind::DuplicateModulePath { span, .. } => span.clone(),
        }
    }
}

pub fn derive_parse_rules(source_kind: SourceKind) -> spire::ParseRules {
    match source_kind {
        SourceKind::Script => spire::ParseRules::script(),
        SourceKind::DefinitionSource => spire::ParseRules::module(),
        SourceKind::StdDefinitionSource => spire::ParseRules::std_module(),
        SourceKind::ReplChunk => spire::ParseRules::repl_chunk(),
    }
}

pub fn derive_runtime_policy(
    compile_unit_kind: CompileUnitKind,
    source_kind: SourceKind,
    entrypoint: Option<&EntryPoint>,
) -> RuntimeSourcePolicy {
    let base = match source_kind {
        SourceKind::Script => RuntimeSourcePolicy::script(),
        SourceKind::DefinitionSource => RuntimeSourcePolicy::module(),
        SourceKind::StdDefinitionSource => RuntimeSourcePolicy::std_module(),
        SourceKind::ReplChunk => RuntimeSourcePolicy::repl_chunk(),
    };

    let policy = match source_kind {
        SourceKind::Script => ExitCodePolicy::Anywhere,
        SourceKind::ReplChunk => ExitCodePolicy::Forbidden,
        SourceKind::DefinitionSource | SourceKind::StdDefinitionSource
            if compile_unit_kind == CompileUnitKind::Project =>
        {
            ExitCodePolicy::EntryOnly
        }
        SourceKind::DefinitionSource | SourceKind::StdDefinitionSource => ExitCodePolicy::Forbidden,
    };

    base.with_exit_code_policy(policy, entrypoint)
}

pub fn lower_module_source_ast(
    ast: Vec<spire::ast::Ast>,
    fallback_module_path: Option<&str>,
) -> Vec<LoweredModuleAst> {
    let shared_imports = ast
        .iter()
        .filter_map(|stmt| match stmt {
            spire::ast::Ast::Import(_, _, _) => Some(stmt.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut lowered = Vec::new();
    let mut shared_global_defs = Vec::new();
    let mut shared_namespace_consts = Vec::new();
    let mut shared_result_ctor_contracts = Vec::new();

    for stmt in ast {
        match stmt {
            spire::ast::Ast::Defmod(span, module_path, body, attrs) => {
                let mut module_ast = shared_imports.clone();
                module_ast.extend(body);
                lowered.push(LoweredModuleAst {
                    module_path,
                    doc_module_path: None,
                    ast: module_ast,
                    declared_span: Some(span),
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                    process_spec: attrs.process_spec,
                });
            }
            spire::ast::Ast::ImplDef(span, target, methods, attrs) => {
                let declared_span = span.clone();
                let module_path = target.clone();
                let mut module_ast = shared_imports.clone();
                let (local_imports, methods) = partition_nested_imports(methods);
                module_ast.extend(local_imports);
                module_ast.push(spire::ast::Ast::ImplDef(
                    span,
                    target,
                    methods,
                    attrs.clone(),
                ));
                lowered.push(LoweredModuleAst {
                    module_path,
                    doc_module_path: None,
                    ast: module_ast,
                    declared_span: Some(declared_span),
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                    process_spec: attrs.process_spec,
                });
            }
            spire::ast::Ast::TraitImplDef(
                span,
                trait_name,
                trait_args,
                target_ty,
                methods,
                attrs,
            ) => {
                let declared_span = span.clone();
                let module_path = match &target_ty {
                    spire::ast::AstTy::Named(_, name)
                    | spire::ast::AstTy::ImplTrait(_, name)
                    | spire::ast::AstTy::Generic(_, name, _) => name.clone(),
                    _ => fallback_module_path.unwrap_or_default().to_string(),
                };
                let mut module_ast = shared_imports.clone();
                let (local_imports, methods) = partition_nested_imports(methods);
                module_ast.extend(local_imports);
                module_ast.push(spire::ast::Ast::TraitImplDef(
                    span,
                    trait_name,
                    trait_args,
                    target_ty,
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
                    process_spec: attrs.process_spec,
                });
            }
            spire::ast::Ast::Import(_, _, _) => {}
            // `Ok` / `Err` are the one top-level std declaration we want to
            // associate with the `Result` module proper. They are surface
            // contracts for the runtime constructors, so keeping them under the
            // `Result` module path lets later phases validate
            // `Result::Ok` / `Result::Err` explicitly.
            spire::ast::Ast::ResultCtorDecl(_, _, _, _, _) => {
                shared_result_ctor_contracts.push(stmt);
            }
            spire::ast::Ast::ConstDef(_, _, _, _, _) => {
                shared_namespace_consts.push(stmt);
            }
            spire::ast::Ast::StructDef(..)
            | spire::ast::Ast::RecordDef(..)
            | spire::ast::Ast::DeferrorDef(_, _, _, _, _)
            | spire::ast::Ast::EnumDef(_, _, _, _, _)
            | spire::ast::Ast::BuiltinDecl(_, _, _, _, _)
            | spire::ast::Ast::IntrinsicDecl(_, _, _, _)
            | spire::ast::Ast::BuiltinTypeDecl(_, _, _) => {
                // Std-module files are allowed to carry top-level declarations
                // alongside their `defmod`. We deliberately keep these in the
                // global declaration layer so source organization by file does
                // not silently change the public surface from `print(...)` to
                // `Kernel::print(...)`, etc.
                shared_global_defs.push(stmt);
            }
            _ => {
                // Defensive fallback. Parser policy should keep this unreachable for definition sources.
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

pub fn parse_module_stages_from_compile_sources(
    compile_sources: &CompileSources,
    compile_unit_kind: CompileUnitKind,
) -> Result<Vec<Vec<sigil::StagedModuleAst>>, ModuleStageParseError> {
    repl::logic::core::parse_module_stages_from_sources(
        &compile_sources.sources,
        &compile_sources.module_stages,
        compile_unit_kind,
    )
}

pub fn parse_module_stages_from_compile_sources_suffix(
    compile_sources: &CompileSources,
    compile_unit_kind: CompileUnitKind,
    start_stage_index: usize,
) -> Result<Vec<Vec<sigil::StagedModuleAst>>, ModuleStageParseError> {
    let suffix = compile_sources
        .module_stages
        .iter()
        .skip(start_stage_index)
        .cloned()
        .collect::<Vec<_>>();
    repl::logic::core::parse_module_stages_from_sources(
        &compile_sources.sources,
        &suffix,
        compile_unit_kind,
    )
}

#[derive(Debug, Clone)]
pub struct DefaultStdlibSnapshot {
    pub module_stages: Vec<Vec<sigil::StagedModuleAst>>,
    pub declaration_index: sigil::DeclarationIndex,
    pub resolve_state: sigil::ResolveResumeState,
    pub scar_checkpoint: scar::ScarCheckpoint,
    pub bytecode: forge::bytecode::Bytecode,
    pub docs: Vec<DocEntry>,
    pub auto_import_modules: BTreeSet<String>,
    pub default_stage_count: usize,
}

const STDLIB_SEMANTIC_CACHE_SCHEMA: u32 = 3;
const TEST_SEMANTIC_PREFIX_CACHE_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedStdlibSemanticEnvelope {
    schema: u32,
    key: String,
    payload: CachedStdlibSemanticPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedStdlibSemanticPayload {
    declaration_index: sigil::DeclarationIndex,
    resolve_state: sigil::ResolveResumeState,
    scar_checkpoint: scar::ScarCheckpoint,
    bytecode: Bytecode,
    docs: Vec<DocEntry>,
    auto_import_modules: BTreeSet<String>,
    default_stage_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedTestSemanticPrefixEnvelope {
    schema: u32,
    key: String,
    payload: CachedTestSemanticPrefixPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedTestSemanticPrefixPayload {
    pub declaration_index: sigil::DeclarationIndex,
    pub resolve_state: sigil::ResolveResumeState,
    pub scar_checkpoint: scar::ScarCheckpoint,
    pub bytecode: Bytecode,
}

pub fn cached_lib_module_inputs() -> Result<Vec<ModuleInput>, LoadError> {
    static CACHE: OnceLock<Result<Vec<ModuleInput>, LoadError>> = OnceLock::new();
    CACHE.get_or_init(collect_lib_module_inputs).clone()
}

pub fn cached_additional_default_std_module_inputs() -> Result<Vec<ModuleInput>, LoadError> {
    static CACHE: OnceLock<Result<Vec<ModuleInput>, LoadError>> = OnceLock::new();
    CACHE
        .get_or_init(collect_additional_default_std_module_inputs)
        .clone()
}

pub fn current_exe_fingerprint() -> Result<String, String> {
    static FINGERPRINT: OnceLock<Result<String, String>> = OnceLock::new();
    FINGERPRINT
        .get_or_init(|| {
            let exe =
                env::current_exe().map_err(|e| format!("failed to locate current exe: {}", e))?;
            let bytes =
                fs::read(&exe).map_err(|e| format!("failed to read {}: {}", exe.display(), e))?;
            Ok(stable_hash_bytes(&bytes))
        })
        .clone()
}

pub fn test_semantic_prefix_cache_key(
    compile_unit_kind: CompileUnitKind,
    compile_sources: &CompileSources,
) -> Result<String, String> {
    let fingerprint = current_exe_fingerprint()?;
    Ok(test_semantic_prefix_cache_key_with_fingerprint(
        &fingerprint,
        compile_unit_kind,
        compile_sources,
    ))
}

pub fn test_semantic_prefix_cache_key_with_fingerprint(
    current_exe_fingerprint: &str,
    compile_unit_kind: CompileUnitKind,
    compile_sources: &CompileSources,
) -> String {
    let user_file_name = compile_sources
        .sources
        .file_name(compile_sources.user_source_id)
        .unwrap_or("<unknown>");
    let user_source = compile_sources
        .sources
        .source(compile_sources.user_source_id)
        .unwrap_or("");

    let mut key = String::new();
    key.push_str("surtr-test-semantic-prefix-v");
    key.push_str(&TEST_SEMANTIC_PREFIX_CACHE_SCHEMA.to_string());
    key.push('\x1f');
    key.push_str(current_exe_fingerprint);
    key.push('\x1f');
    key.push_str(&STDLIB_SEMANTIC_CACHE_SCHEMA.to_string());
    key.push('\x1f');
    key.push_str(match compile_unit_kind {
        CompileUnitKind::Script => "script",
        CompileUnitKind::DefinitionCheck => "definition-check",
        CompileUnitKind::Project => "project",
        CompileUnitKind::Repl => "repl",
    });
    key.push('\x1f');
    key.push_str(user_file_name);
    key.push('\x1f');
    key.push_str(&compile_sources.user_module_path);
    key.push('\x1f');
    key.push_str(&stable_hash_hex(user_source));

    for stage in &compile_sources.module_stages {
        key.push('|');
        for module in stage {
            let file_name = compile_sources
                .sources
                .file_name(module.source_id)
                .unwrap_or("<unknown>");
            let source = compile_sources
                .sources
                .source(module.source_id)
                .unwrap_or("");
            key.push_str(file_name);
            key.push('\x1e');
            key.push_str(&module.module_path);
            key.push('\x1e');
            key.push_str(source_kind_key(module.source_kind));
            key.push('\x1e');
            key.push_str(&stable_hash_hex(source));
            key.push('\x1f');
        }
    }

    stable_hash_hex(&key)
}

pub fn load_cached_test_semantic_prefix(
    cache_path: &Path,
    expected_key: &str,
) -> Option<CachedTestSemanticPrefixPayload> {
    let bytes = fs::read(cache_path).ok()?;
    let envelope: CachedTestSemanticPrefixEnvelope = bincode::deserialize(&bytes).ok()?;
    if envelope.schema != TEST_SEMANTIC_PREFIX_CACHE_SCHEMA || envelope.key != expected_key {
        return None;
    }
    Some(envelope.payload)
}

pub fn store_cached_test_semantic_prefix(
    cache_path: &Path,
    key: &str,
    payload: CachedTestSemanticPrefixPayload,
) {
    let Some(parent) = cache_path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let envelope = CachedTestSemanticPrefixEnvelope {
        schema: TEST_SEMANTIC_PREFIX_CACHE_SCHEMA,
        key: key.to_string(),
        payload,
    };
    let Ok(bytes) = bincode::serialize(&envelope) else {
        return;
    };
    let temp_path = cache_path.with_extension(format!("{}.tmp", std::process::id()));
    if fs::write(&temp_path, bytes).is_err() {
        let _ = fs::remove_file(&temp_path);
        return;
    }
    if fs::rename(&temp_path, cache_path).is_err() {
        if fs::copy(&temp_path, cache_path).is_err() {
            let _ = fs::remove_file(&temp_path);
            return;
        }
        let _ = fs::remove_file(&temp_path);
    }
}

pub fn default_stdlib_semantic_snapshot() -> Result<Arc<DefaultStdlibSnapshot>, LoadError> {
    static SNAPSHOT: OnceLock<Result<Arc<DefaultStdlibSnapshot>, LoadError>> = OnceLock::new();
    SNAPSHOT
        .get_or_init(|| build_default_stdlib_snapshot().map(Arc::new))
        .clone()
}

fn build_default_stdlib_snapshot() -> Result<DefaultStdlibSnapshot, LoadError> {
    let module_sources = collect_module_sources_with_module_stages(&[])?;
    let cache_key = stdlib_semantic_cache_key(&module_sources);
    let module_stages = repl::logic::core::parse_module_stages_from_sources(
        &module_sources.sources,
        &module_sources.module_stages,
        CompileUnitKind::Script,
    )
    .map_err(|e| LoadError::BootstrapFailed {
        phase: "parse".into(),
        file_name: module_sources
            .sources
            .file_name(e.source_id)
            .unwrap_or("<stdlib>")
            .to_string(),
        message: e.message(),
    })?;
    let docs = collect_doc_entries(&module_stages, &[], None);
    let auto_import_modules = module_stages
        .iter()
        .flat_map(|stage| stage.iter())
        .filter(|module| module.auto_import)
        .map(|module| module.module_path.clone())
        .collect::<BTreeSet<_>>();
    let default_stage_count = module_stages.len();

    if let Some(payload) =
        load_cached_stdlib_semantic_snapshot(&stdlib_semantic_cache_path(), &cache_key)
    {
        if payload.default_stage_count == default_stage_count {
            return Ok(DefaultStdlibSnapshot {
                module_stages,
                declaration_index: payload.declaration_index,
                resolve_state: payload.resolve_state,
                scar_checkpoint: payload.scar_checkpoint,
                bytecode: payload.bytecode,
                docs: payload.docs,
                auto_import_modules: payload.auto_import_modules,
                default_stage_count: payload.default_stage_count,
            });
        }
    }

    let declaration_index = sigil::precollect_declaration_index(&module_stages).map_err(|e| {
        LoadError::BootstrapFailed {
            phase: "resolve".into(),
            file_name: "<stdlib>".into(),
            message: e.message,
        }
    })?;
    let resolved = sigil::resolve_staged_program_with_state(
        &module_stages,
        Vec::new(),
        &declaration_index,
        None,
    )
    .map_err(|e| LoadError::BootstrapFailed {
        phase: "resolve".into(),
        file_name: "<stdlib>".into(),
        message: e.message,
    })?;
    let resume_state = resolved.resume_state;
    let mut scar_session = scar::ScarSession::new();
    let typed = scar_session
        .typecheck_staged_program_with_context(
            resolved,
            scar::TypecheckContext {
                runtime_policy: RuntimeSourcePolicy::std_module(),
                enforce_builtin_type_contracts: true,
                allow_error_function_params: true,
            },
        )
        .map_err(|e| LoadError::BootstrapFailed {
            phase: "typecheck".into(),
            file_name: "<stdlib>".into(),
            message: e.message,
        })?;
    let mut bytecode =
        forge::codegen_typed_program(typed).map_err(|e| LoadError::BootstrapFailed {
            phase: "codegen".into(),
            file_name: "<stdlib>".into(),
            message: e.message,
        })?;
    scar_session.reconcile_function_indices(bytecode.functions.iter().filter_map(|entry| {
        entry
            .qualified_name
            .as_deref()
            .map(|qualified_name| (qualified_name, entry.fun_idx))
    }));
    bytecode.docs = docs.clone();
    let next_fun_idx = bytecode
        .functions
        .iter()
        .map(|entry| entry.fun_idx.saturating_add(1))
        .max()
        .unwrap_or(0);
    let resolve_state = sigil::ResolveResumeState {
        next_local_id: resume_state.next_local_id.max(next_fun_idx),
    };

    let snapshot = DefaultStdlibSnapshot {
        default_stage_count,
        declaration_index,
        resolve_state,
        scar_checkpoint: scar_session.checkpoint(),
        bytecode,
        docs,
        auto_import_modules,
        module_stages,
    };
    store_cached_stdlib_semantic_snapshot(
        &stdlib_semantic_cache_path(),
        &cache_key,
        CachedStdlibSemanticPayload {
            declaration_index: snapshot.declaration_index.clone(),
            resolve_state: snapshot.resolve_state,
            scar_checkpoint: snapshot.scar_checkpoint.clone(),
            bytecode: snapshot.bytecode.clone(),
            docs: snapshot.docs.clone(),
            auto_import_modules: snapshot.auto_import_modules.clone(),
            default_stage_count: snapshot.default_stage_count,
        },
    );
    Ok(snapshot)
}

fn load_cached_stdlib_semantic_snapshot(
    cache_path: &Path,
    expected_key: &str,
) -> Option<CachedStdlibSemanticPayload> {
    let bytes = fs::read(cache_path).ok()?;
    let envelope: CachedStdlibSemanticEnvelope = bincode::deserialize(&bytes).ok()?;
    if envelope.schema != STDLIB_SEMANTIC_CACHE_SCHEMA || envelope.key != expected_key {
        return None;
    }
    Some(envelope.payload)
}

fn store_cached_stdlib_semantic_snapshot(
    cache_path: &Path,
    key: &str,
    payload: CachedStdlibSemanticPayload,
) {
    let Some(parent) = cache_path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let envelope = CachedStdlibSemanticEnvelope {
        schema: STDLIB_SEMANTIC_CACHE_SCHEMA,
        key: key.to_string(),
        payload,
    };
    let Ok(bytes) = bincode::serialize(&envelope) else {
        return;
    };
    let temp_path = cache_path.with_extension(format!("{}.tmp", std::process::id()));
    if fs::write(&temp_path, bytes).is_err() {
        let _ = fs::remove_file(&temp_path);
        return;
    }
    if fs::rename(&temp_path, cache_path).is_err() {
        if fs::copy(&temp_path, cache_path).is_err() {
            let _ = fs::remove_file(&temp_path);
            return;
        }
        let _ = fs::remove_file(&temp_path);
    }
}

fn stdlib_semantic_cache_path() -> PathBuf {
    if let Some(path) = env::var_os("SURTR_STDLIB_CACHE_DIR") {
        return PathBuf::from(path).join("std.semantic");
    }
    target_root_from_current_exe()
        .map(|root| root.join("surtr-stdlib-cache").join("std.semantic"))
        .unwrap_or_else(|| {
            env::temp_dir()
                .join("surtr-stdlib-cache")
                .join("std.semantic")
        })
}

fn target_root_from_current_exe() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let mut current = exe.parent()?;
    while let Some(name) = current.file_name().and_then(|name| name.to_str()) {
        if name == "debug" || name == "release" {
            return current.parent().map(Path::to_path_buf);
        }
        current = current.parent()?;
    }
    None
}

fn stdlib_semantic_cache_key(module_sources: &ModuleSources) -> String {
    let mut key = String::new();
    key.push_str("surtr-stdlib-semantic-cache-v");
    key.push_str(&STDLIB_SEMANTIC_CACHE_SCHEMA.to_string());
    key.push('\x1f');
    key.push_str(env!("CARGO_PKG_VERSION"));
    key.push('\x1f');
    for meta in BUILTIN_METAS {
        key.push_str(meta.name);
        key.push('\x1e');
        key.push_str(meta.sig_str);
        key.push('\x1e');
        key.push_str(&meta.arity.to_string());
        key.push('\x1f');
    }
    key.push('\x1d');
    for meta in BUILTIN_TYPE_METAS {
        key.push_str(meta.name);
        key.push('\x1e');
        key.push_str(&meta.params.join(","));
        key.push('\x1f');
    }
    key.push('\x1d');
    for stage in &module_sources.module_stages {
        key.push('|');
        for module in stage {
            let file_name = module_sources
                .sources
                .file_name(module.source_id)
                .unwrap_or("<unknown>");
            let source = module_sources
                .sources
                .source(module.source_id)
                .unwrap_or("");
            key.push_str(file_name);
            key.push('\x1e');
            key.push_str(&module.module_path);
            key.push('\x1e');
            key.push_str(source_kind_key(module.source_kind));
            key.push('\x1e');
            key.push_str(&stable_hash_hex(source));
            key.push('\x1f');
        }
    }
    stable_hash_hex(&key)
}

fn source_kind_key(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Script => "script",
        // Keep cache key strings stable for backward compatibility with existing cache entries.
        SourceKind::DefinitionSource => "module",
        SourceKind::StdDefinitionSource => "std",
        SourceKind::ReplChunk => "repl",
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_module_source_extracts_defmods_and_shared_defs() {
        let ast = spire::parse_with_context(
            r#"import Other::f;

defmod A {
  def fa() -> Int { 1 }
}

defrecord Pair(left: Int, right: Int)

defmod B {
  def fb() -> Int { f() }
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("definition source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 3);
        assert_eq!(lowered[0].module_path, "A");
        assert_eq!(lowered[1].module_path, "B");
        assert_eq!(lowered[2].module_path, "");
        assert!(matches!(
            lowered[0].ast[0],
            spire::ast::Ast::Import(_, _, spire::ast::ImportSpec::Single(_))
        ));
        assert!(lowered[2]
            .ast
            .iter()
            .any(|stmt| matches!(stmt, spire::ast::Ast::RecordDef(..))));
    }

    #[test]
    fn default_stdlib_snapshot_contains_only_default_stages() {
        let snapshot =
            default_stdlib_semantic_snapshot().expect("default stdlib snapshot should build");

        assert_eq!(snapshot.default_stage_count, snapshot.module_stages.len());
        assert!(snapshot
            .declaration_index
            .values()
            .any(|entry| entry.fq_name == "Kernel::print"));
        assert!(!snapshot
            .declaration_index
            .values()
            .any(|entry| entry.module_path == "TestOnly"));
    }

    #[test]
    fn stdlib_semantic_cache_rejects_corrupt_file() {
        let cache_path = std::env::temp_dir().join(format!(
            "surtr-corrupt-stdlib-cache-{}.semantic",
            std::process::id()
        ));
        std::fs::write(&cache_path, b"not a semantic cache").expect("write corrupt cache");

        let loaded = load_cached_stdlib_semantic_snapshot(&cache_path, "expected-key");

        assert!(loaded.is_none());
        let _ = std::fs::remove_file(cache_path);
    }

    #[test]
    fn test_semantic_prefix_cache_roundtrips_payload() {
        let cache_path = std::env::temp_dir().join(format!(
            "surtr-test-prefix-cache-{}.semantic",
            std::process::id()
        ));
        let payload = CachedTestSemanticPrefixPayload {
            declaration_index: sigil::DeclarationIndex::new(),
            resolve_state: sigil::ResolveResumeState { next_local_id: 7 },
            scar_checkpoint: scar::ScarSession::new().checkpoint(),
            bytecode: forge::bytecode::Bytecode::default(),
        };

        store_cached_test_semantic_prefix(&cache_path, "expected-key", payload.clone());

        let loaded = load_cached_test_semantic_prefix(&cache_path, "expected-key")
            .expect("payload should roundtrip");

        assert_eq!(loaded.resolve_state.next_local_id, 7);
        assert_eq!(loaded.declaration_index, payload.declaration_index);
        let _ = std::fs::remove_file(cache_path);
    }

    #[test]
    fn test_semantic_prefix_cache_rejects_corrupt_file() {
        let cache_path = std::env::temp_dir().join(format!(
            "surtr-corrupt-test-prefix-cache-{}.semantic",
            std::process::id()
        ));
        std::fs::write(&cache_path, b"not a semantic prefix cache").expect("write corrupt cache");

        let loaded = load_cached_test_semantic_prefix(&cache_path, "expected-key");

        assert!(loaded.is_none());
        let _ = std::fs::remove_file(cache_path);
    }

    #[test]
    fn lower_module_source_merges_result_ctors_into_single_impl_owner() {
        let ast = spire::parse_with_context(
            r#"@builtin type Ok($T) -> Result<$T>

impl Result {
  def dummy() { () }
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0].module_path, "Result");
        assert!(lowered[0].ast.iter().any(
            |stmt| matches!(stmt, spire::ast::Ast::ResultCtorDecl(_, name, _, _, _) if name == "Ok")
        ));
        assert!(lowered[0].ast.iter().any(
            |stmt| matches!(stmt, spire::ast::Ast::ImplDef(_, target, methods, _) if target == "Result"
                && methods.iter().any(|method| matches!(method, spire::ast::Ast::Def(_, name, _, _, _, _, _) if name == "dummy")))
        ));
    }

    #[test]
    fn lower_module_source_keeps_builtin_decls_global_even_with_single_impl_owner() {
        let ast = spire::parse_with_context(
            r#"@builtin type Int
@builtin def safe_mod(a: Int, b: Int) -> Result<Int, ZeroDivisionError>

impl Int {
  def dummy() { () }
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 2);
        assert_eq!(lowered[0].module_path, "Int");
        assert_eq!(lowered[1].module_path, "");
        assert!(lowered[1]
            .ast
            .iter()
            .any(|stmt| matches!(stmt, spire::ast::Ast::BuiltinTypeDecl(_, _, _))));
        assert!(lowered[1]
            .ast
            .iter()
            .any(|stmt| matches!(stmt, spire::ast::Ast::BuiltinDecl(_, name, _, _, _) if name == "safe_mod")));
    }

    #[test]
    fn lower_module_source_attaches_top_level_consts_to_namespace_module() {
        let ast = spire::parse_with_context(
            r#"const APP_NAME = "surtr"

defmod AppConfig {
  def label() -> String { APP_NAME }
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::module()),
        )
        .expect("definition source should parse");

        let lowered = lower_module_source_ast(ast, Some("AppConfig"));
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0].module_path, "AppConfig");
        assert!(lowered[0].ast.iter().any(
            |stmt| matches!(stmt, spire::ast::Ast::ConstDef(_, name, _, _, _) if name == "APP_NAME")
        ));
    }

    #[test]
    fn lower_module_source_keeps_defmod_local_imports_in_that_module() {
        let ast = spire::parse_with_context(
            r#"defmod Parser {
  import String;
  def parse(line: String) -> String { trim(line) }
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("definition source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0].module_path, "Parser");
        assert!(matches!(
            lowered[0].ast.as_slice(),
            [
                spire::ast::Ast::Import(_, _, spire::ast::ImportSpec::All),
                spire::ast::Ast::Def(_, name, _, _, _, _, _)
            ] if name == "parse"
        ));
    }

    #[test]
    fn lower_module_source_hoists_impl_local_imports_into_impl_owner_module() {
        let ast = spire::parse_with_context(
            r#"impl User {
  def normalize(self: Self, name: String) -> String { trim(name) }
  import String;
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("definition source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0].module_path, "User");
        assert!(matches!(
            lowered[0].ast.as_slice(),
            [
                spire::ast::Ast::Import(_, _, spire::ast::ImportSpec::All),
                spire::ast::Ast::ImplDef(_, target, methods, _)
            ] if target == "User"
                && matches!(methods.as_slice(), [spire::ast::Ast::Def(_, name, _, _, _, _, _)] if name == "normalize")
        ));
    }

    #[test]
    fn lower_module_source_hoists_trait_impl_local_imports_into_trait_impl_module() {
        let ast = spire::parse_with_context(
            r#"impl Show for User {
  def to_string(self: Self) -> String { trim("x") }
  import String;
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("definition source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0].module_path, "User");
        assert!(matches!(
            lowered[0].ast.as_slice(),
            [
                spire::ast::Ast::Import(_, _, spire::ast::ImportSpec::All),
                spire::ast::Ast::TraitImplDef(_, trait_name, _, spire::ast::AstTy::Named(_, target), methods, _)
            ] if trait_name == "Show"
                && target == "User"
                && matches!(methods.as_slice(), [spire::ast::Ast::Def(_, name, _, _, _, _, _)] if name == "to_string")
        ));
    }

    #[test]
    fn collect_doc_entries_includes_deferror_docs() {
        let ast = spire::parse_with_context(
            r#"defmod Bootstrap {
  def dummy() { () }
}

@doc """Missing value."""
deferror NoneError { "None Value." }"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, Some("Bootstrap"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Bootstrap::NoneError"
                && entry.signature.as_deref() == Some("deferror NoneError")
                && entry.doc == "Missing value."
        }));
    }

    #[test]
    fn collect_doc_entries_includes_special_closure_type_docs() {
        let ast = spire::parse_with_context(
            r#"@doc """Closure docs."""
@builtin type Closure"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, Some("SpecialTypes"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.kind == DocKind::Type
                && entry.signature.as_deref() == Some("type Closure")
                && entry.doc == "Closure docs."
        }));
    }

    #[test]
    fn bundled_bootstrap_source_parses_in_std_module_context() {
        let ast = spire::parse_with_context(
            include_str!("../../../lib/bootstrap.srt"),
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("bootstrap source should parse as a std module");
        assert!(ast.iter().any(|stmt| matches!(
            stmt,
            spire::ast::Ast::Defmod(_, name, body, _)
                if name == "Bootstrap"
                && body.iter().any(|stmt| matches!(
                    stmt,
                    spire::ast::Ast::BuiltinDecl(_, builtin_name, _, _, _) if builtin_name == "import"
                ))
        )));
    }

    #[test]
    fn bundled_kernel_source_marks_kernel_module_autoimport() {
        let ast = spire::parse_with_context(
            include_str!("../../../lib/kernel.srt"),
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("kernel source should parse as a std module");

        let lowered = lower_module_source_ast(ast, None);
        assert!(lowered
            .iter()
            .any(|module| module.module_path == "Kernel" && module.auto_import));
    }

    #[test]
    fn bundled_special_types_source_declares_lazy_builtin_type() {
        let ast = spire::parse_with_context(
            include_str!("../../../lib/types/special_types.srt"),
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("special types source should parse as a std module");

        let lowered = lower_module_source_ast(ast, Some("SpecialTypes"));
        assert!(lowered.iter().any(|module| {
            module.module_path == "SpecialTypes"
                && module.ast.iter().any(|stmt| {
                    matches!(
                        stmt,
                        spire::ast::Ast::BuiltinTypeDecl(_, head, _)
                            if head.name == "Lazy"
                    )
                })
        }));
    }

    #[test]
    fn collect_doc_entries_includes_bootstrap_import_docs() {
        let ast = spire::parse_with_context(
            r#"defmod Bootstrap {
  @doc """Language-provided import macro function."""
  @builtin def import() -> Unit
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, Some("Bootstrap"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Bootstrap::import"
                && entry.kind == DocKind::Function
                && entry.signature.as_deref() == Some("import() -> Unit")
                && entry.doc == "Language-provided import macro function."
        }));
    }

    #[test]
    fn collect_doc_entries_keeps_single_bootstrap_dbg_intrinsic_doc() {
        let ast = spire::parse_with_context(
            r#"defmod Bootstrap {
  @doc """Debug special form."""
  @intrinsic def dbg!(values: *$A) -> Unit
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, Some("Bootstrap"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        let dbg_docs = docs
            .iter()
            .filter(|entry| entry.qualified_name == "Bootstrap::dbg!")
            .collect::<Vec<_>>();
        assert_eq!(dbg_docs.len(), 1, "{dbg_docs:?}");
        assert_eq!(
            dbg_docs[0].signature.as_deref(),
            Some("@intrinsic def dbg!(values: *$A) -> Unit")
        );
    }

    #[test]
    fn lower_module_source_ast_keeps_process_spec_on_lowered_module() {
        let ast = spire::parse_with_context(
            r#"@agent(kind: State, instance: Singleton, boot: true, lazy: false, registry: true)
defagent Counter {
  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("defagent source should parse");

        let lowered = lower_module_source_ast(ast, Some("Counter"));
        assert_eq!(lowered.len(), 1);
        let process_spec = lowered[0]
            .process_spec
            .as_ref()
            .expect("lowered module should keep process spec");
        assert_eq!(process_spec.process_name, "Counter");
        assert!(process_spec.boot);
        assert!(matches!(
            process_spec.kind,
            spire::ast::ProcessKind::StateAgent
        ));
    }

    #[test]
    fn collect_doc_entries_keeps_bootstrap_bind_intrinsic_docs() {
        let ast = spire::parse_with_context(
            r#"defmod Bootstrap {
  @doc """Bind special form."""
  @intrinsic def =(pattern: $Pattern, value: $A) -> Unit

  @doc """SafeBind special form."""
  @intrinsic def =?(pattern: $Pattern, value: $A) -> Unit
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, Some("Bootstrap"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        let bind_docs = docs
            .iter()
            .filter(|entry| entry.qualified_name == "Bootstrap::=")
            .collect::<Vec<_>>();
        assert_eq!(bind_docs.len(), 1, "{bind_docs:?}");
        assert_eq!(
            bind_docs[0].signature.as_deref(),
            Some("@intrinsic def =(pattern: $Pattern, value: $A) -> Unit")
        );

        let safe_bind_docs = docs
            .iter()
            .filter(|entry| entry.qualified_name == "Bootstrap::=?")
            .collect::<Vec<_>>();
        assert_eq!(safe_bind_docs.len(), 1, "{safe_bind_docs:?}");
        assert_eq!(
            safe_bind_docs[0].signature.as_deref(),
            Some("@intrinsic def =?(pattern: $Pattern, value: $A) -> Unit")
        );
    }

    #[test]
    fn collect_doc_entries_includes_impl_and_trait_docs() {
        let ast = spire::parse_with_context(
            r#"@doc """Trait docs."""
deftrait Numeric {
  def add(self: Self, rhs: Self) -> Self
}

defstruct User {
  name: String,
}

@doc """Numeric Int docs."""
impl Numeric for Int {
  def add(self: Self, rhs: Self) -> Self {
    self + rhs
  }
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("annotated trait and impl docs should parse");

        let lowered = lower_module_source_ast(ast, Some("Sample"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::Numeric"
                && entry.kind == DocKind::Type
                && entry.doc == "Trait docs."
        }));
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::impl Numeric for Int"
                && entry.kind == DocKind::Type
                && entry.signature.as_deref() == Some("impl Numeric for Int")
                && entry.doc == "Numeric Int docs."
        }));
    }

    #[test]
    fn collect_doc_entries_excludes_impl_owner_docs() {
        let ast = spire::parse_with_context(
            r#"defstruct User {
  name: String,
}

@autoimport
impl User {
  def new(name: String) -> Self {
    User { name: name }
  }
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("impl owner annotations should parse");

        let lowered = lower_module_source_ast(ast, Some("Sample"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(!docs.iter().any(|entry| {
            entry.qualified_name == "User" && entry.signature.as_deref() == Some("impl User")
        }));
    }

    #[test]
    fn collect_doc_entries_includes_impl_method_docs() {
        let ast = spire::parse_with_context(
            r#"defstruct User {
  name: String,
}

impl User {
  @doc """Construct a new user value."""
  def new(name: String) -> Self {
    User { name: name }
  }

  @doc """Deconstruct a user value for pattern matching."""
  defextractor deconstruct(self: Self) -> MatchResult<String, Error> {
    MatchResult::Success(self.name)
  }
}

@doc """String conversion for `Int`."""
impl Show for Int {
  @doc """Render `Int` through the standard display surface."""
  def to_string(self: Self) -> String {
    inspect(self)
  }
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("annotated impl method docs should parse");

        let lowered = lower_module_source_ast(ast, Some("Sample"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "User::new"
                && entry.kind == DocKind::Function
                && entry.signature.as_deref() == Some("User::new(name: String) -> Self")
                && entry.doc == "Construct a new user value."
        }));
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "User::deconstruct"
                && entry.kind == DocKind::Function
                && entry.signature.as_deref()
                    == Some("User::deconstruct(self: Self) -> MatchResult<String, Error>")
                && entry.doc == "Deconstruct a user value for pattern matching."
        }));
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::impl Show for Int::to_string"
                && entry.kind == DocKind::Function
                && entry.signature.as_deref()
                    == Some("impl Show for Int::to_string(self: Self) -> String")
                && entry.doc == "Render `Int` through the standard display surface."
        }));
    }

    #[test]
    fn collect_doc_entries_include_struct_and_record_docs_with_head_only_signatures() {
        let ast = spire::parse_with_context(
            r#"@doc """User docs."""
defstruct User {
  name: String,
}

@doc """Point docs."""
defrecord Point(x: Float, y: Float)"#,
            spire::ParserContext::module(1, None),
        )
        .expect("annotated struct and record docs should parse");

        let lowered = lower_module_source_ast(ast, Some("Sample"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::User"
                && entry.kind == DocKind::Type
                && entry.signature.as_deref() == Some("defstruct User")
                && entry.doc == "User docs."
        }));
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::Point"
                && entry.kind == DocKind::Type
                && entry.signature.as_deref() == Some("defrecord Point")
                && entry.doc == "Point docs."
        }));
    }

    #[test]
    fn collect_doc_entries_qualify_builtin_impl_method_signatures() {
        let ast = spire::parse_with_context(
            r#"defstruct User {
  name: String,
}

impl User {
  @doc """Builtin helper doc."""
  @builtin def inspect_name(user: User) -> String
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("annotated builtin impl method docs should parse");

        let lowered = lower_module_source_ast(ast, Some("Sample"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "User::inspect_name"
                && entry.kind == DocKind::Function
                && entry.signature.as_deref() == Some("User::inspect_name(user: User) -> String")
                && entry.doc == "Builtin helper doc."
        }));
    }

    #[test]
    fn parse_module_stages_detects_duplicate_defmod_paths() {
        let module_sources = collect_module_sources_with_module_stages(&[vec![
            ModuleInput {
                file_name: "a.srt".into(),
                source: "defmod Shared { def a() -> Int { 1 } }".into(),
                module_path: "A".into(),
            },
            ModuleInput {
                file_name: "b.srt".into(),
                source: "defmod Shared { def b() -> Int { 2 } }".into(),
                module_path: "B".into(),
            },
        ]])
        .expect("module collection should succeed");
        let compile_sources =
            compose_script_compile_sources("entry.srt", "print(\"hi\")", module_sources);

        let err =
            parse_module_stages_from_compile_sources(&compile_sources, CompileUnitKind::Script)
                .expect_err("duplicate defmod path must fail");
        assert!(matches!(
            err.kind,
            ModuleStageParseErrorKind::DuplicateModulePath { ref module_path, .. } if module_path == "Shared"
        ));
    }

    #[test]
    fn parse_module_stages_preserves_same_stage_file_order_after_parallel_parse() {
        let module_sources = collect_module_sources_with_module_stages(&[vec![
            ModuleInput {
                file_name: "a.srt".into(),
                source: "defmod First { def value() -> Int { 1 } }".into(),
                module_path: "First".into(),
            },
            ModuleInput {
                file_name: "b.srt".into(),
                source: "defmod Second { def value() -> Int { 2 } }".into(),
                module_path: "Second".into(),
            },
        ]])
        .expect("module collection should succeed");
        let compile_sources =
            compose_script_compile_sources("entry.srt", "print(\"hi\")", module_sources);
        let parsed =
            parse_module_stages_from_compile_sources(&compile_sources, CompileUnitKind::Script)
                .expect("module stages should parse");
        let user_stage = parsed.last().expect("user module stage should exist");

        assert_eq!(user_stage[0].module_path, "First");
        assert_eq!(user_stage[1].module_path, "Second");
    }
}
