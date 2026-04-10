use spire::token::Token;

mod loader;
pub mod repl;
pub mod tui;

pub use loader::{
    collect_additional_default_std_module_inputs, collect_lib_module_inputs,
    collect_module_sources_with_module_file_stages, collect_module_sources_with_module_stages,
    collect_module_sources_with_modules, collect_module_sources_with_std_module_stages,
    compose_script_compile_sources, derive_primary_module_path, is_default_std_module_file_name,
    is_default_std_module_path, script_pseudo_module_path, CompileSources, LoadError,
    ModuleInput, ModuleSources, SourceDescriptor, SourceKind, StagedModule,
};

pub use repl::logic::core::{EldrLoadError, ReplEngine};
pub use repl::ui::cli::{cli_command, BannerMode, ReplOptions};
use sindr::ir::{DocEntry, DocKind};

// ── Public types used by other crates ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct LoweredModuleAst {
    pub module_path: String,
    pub ast: Vec<spire::ast::Ast>,
    pub declared_span: Option<spire::ast::Span>,
    pub module_doc: Option<String>,
}

fn format_ast_ty(ty: &spire::ast::AstTy) -> String {
    match ty {
        spire::ast::AstTy::Named(_, name) => name.clone(),
        spire::ast::AstTy::Generic(_, name, args) => {
            let args = args
                .iter()
                .map(format_ast_ty)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}<{args}>")
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

fn format_fun_signature(
    name: &str,
    params: &[spire::ast::FunParam],
    ret_ty: &Option<spire::ast::AstTy>,
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
            spire::ast::Ast::Def(_, name, params, ret_ty, _, attrs) => {
                if let Some(doc) = &attrs.doc {
                    out.push(DocEntry {
                        qualified_name: qualified_name(module_path, name),
                        kind: DocKind::Function,
                        module_path: module_path.to_string(),
                        signature: Some(format_fun_signature(name, params, ret_ty)),
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
                        signature: Some(format_fun_signature(name, params, ret_ty)),
                        doc: doc.clone(),
                    });
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

    for stage in module_stages {
        for module in stage {
            if let Some(doc) = &module.module_doc {
                docs.push(DocEntry {
                    qualified_name: module.module_path.clone(),
                    kind: DocKind::Module,
                    module_path: module.module_path.clone(),
                    signature: None,
                    doc: doc.clone(),
                });
            }
            collect_doc_entries_for_ast(&module.ast, &module.module_path, &mut docs);
        }
    }

    collect_doc_entries_for_ast(user_ast, user_module_path.unwrap_or_default(), &mut docs);

    docs
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

pub fn derive_source_rules(
    compile_unit_kind: spire::CompileUnitKind,
    source_kind: SourceKind,
    entrypoint: Option<&spire::EntryPoint>,
) -> spire::SourceRules {
    // SourceKind controls the syntactic boundary (`@@builtin`, top-level expr,
    // etc.), while CompileUnitKind refines runtime-only rules such as where
    // `set_exit_code` is legal in project builds.
    let base = match source_kind {
        SourceKind::Script => spire::SourceRules::script(),
        SourceKind::Module => spire::SourceRules::module(),
        SourceKind::StdModule => spire::SourceRules::std_module(),
        SourceKind::ReplChunk => spire::SourceRules::repl_chunk(),
    };

    let policy = match source_kind {
        SourceKind::Script => spire::SetExitCodePolicy::Anywhere,
        SourceKind::ReplChunk => spire::SetExitCodePolicy::Forbidden,
        SourceKind::Module | SourceKind::StdModule
            if compile_unit_kind == spire::CompileUnitKind::Project =>
        {
            spire::SetExitCodePolicy::EntryOnly
        }
        SourceKind::Module | SourceKind::StdModule => spire::SetExitCodePolicy::Forbidden,
    };

    base.with_set_exit_code_policy(policy, entrypoint)
}

fn erase_non_newline_span(chars: &mut [char], start: usize, end: usize) {
    let len = chars.len();
    let capped_end = end.min(len);
    let capped_start = start.min(len);
    for ch in chars.iter_mut().take(capped_end).skip(capped_start) {
        if *ch != '\n' {
            *ch = ' ';
        }
    }
}

/// Strip `@@test <expr>` annotations while preserving source span offsets.
///
/// The parser does not need to process `@@test` in normal compilation flows.
/// Replacing characters with spaces keeps diagnostics line/column stable.
pub fn strip_test_annotations(source: &str) -> String {
    let tokens = match spire::lexer::tokenize(source) {
        Ok(tokens) => tokens,
        Err(_) => return source.to_string(),
    };

    let mut chars = source.chars().collect::<Vec<_>>();
    let mut i = 0usize;
    while i < tokens.len() {
        if let Token::Annotator(name) = &tokens[i].token {
            if name == "test" {
                let mut j = i + 1;
                while j < tokens.len() && !matches!(tokens[j].token, Token::Newline | Token::Eof) {
                    j += 1;
                }
                let end = if j > i + 1 {
                    tokens[j - 1].span.end
                } else {
                    tokens[i].span.end
                };
                erase_non_newline_span(&mut chars, tokens[i].span.start, end);
                i = j;
                continue;
            }
        }
        i += 1;
    }

    chars.into_iter().collect::<String>()
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
    let mut shared_result_ctor_contracts = Vec::new();

    for stmt in ast {
        match stmt {
            spire::ast::Ast::Defmod(span, module_path, body, attrs) => {
                let mut module_ast = shared_imports.clone();
                module_ast.extend(body);
                lowered.push(LoweredModuleAst {
                    module_path,
                    ast: module_ast,
                    declared_span: Some(span),
                    module_doc: attrs.doc,
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
            spire::ast::Ast::StructDef(_, _, _)
            | spire::ast::Ast::RecordDef(_, _, _)
            | spire::ast::Ast::DeferrorDef(_, _, _, _, _)
            | spire::ast::Ast::EnumDef(_, _, _, _, _)
            | spire::ast::Ast::ImplDef(_, _, _)
            | spire::ast::Ast::BuiltinDecl(_, _, _, _, _)
            | spire::ast::Ast::BuiltinTypeDecl(_, _, _) => {
                // Std-module files are allowed to carry top-level declarations
                // alongside their `defmod`. We deliberately keep these in the
                // global declaration layer so source organization by file does
                // not silently change the public surface from `print(...)` to
                // `Kernel::print(...)`, etc.
                shared_global_defs.push(stmt);
            }
            _ => {
                // Defensive fallback. Parser policy should keep this unreachable for module sources.
                shared_global_defs.push(stmt);
            }
        }
    }

    if !shared_result_ctor_contracts.is_empty() {
        if lowered.len() == 1 {
            let insert_at = lowered[0]
                .ast
                .iter()
                .take_while(|stmt| matches!(stmt, spire::ast::Ast::Import(_, _, _)))
                .count();
            lowered[0]
                .ast
                .splice(insert_at..insert_at, shared_result_ctor_contracts);
        } else {
            let mut shared_ast = shared_imports.clone();
            shared_ast.extend(shared_result_ctor_contracts);
            lowered.push(LoweredModuleAst {
                module_path: fallback_module_path.unwrap_or_default().to_string(),
                ast: shared_ast,
                declared_span: None,
                module_doc: None,
            });
        }
    }

    if !shared_global_defs.is_empty() {
        let mut shared_ast = shared_imports;
        shared_ast.extend(shared_global_defs);
        lowered.push(LoweredModuleAst {
            module_path: fallback_module_path.unwrap_or_default().to_string(),
            ast: shared_ast,
            declared_span: None,
            module_doc: None,
        });
    }

    lowered
}

pub fn parse_module_stages_from_compile_sources(
    compile_sources: &CompileSources,
    compile_unit_kind: spire::CompileUnitKind,
) -> Result<Vec<Vec<sigil::StagedModuleAst>>, ModuleStageParseError> {
    repl::logic::core::parse_module_stages_from_sources(
        &compile_sources.sources,
        &compile_sources.module_stages,
        compile_unit_kind,
    )
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
        .expect("module source should parse");

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
            .any(|stmt| matches!(stmt, spire::ast::Ast::RecordDef(_, _, _))));
    }

    #[test]
    fn lower_module_source_merges_shared_defs_into_single_defmod() {
        let ast = spire::parse_with_context(
            r#"@@builtin type Ok($T) -> Result<$T>

defmod Result {
  def dummy() { () }
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::SourceRules::std_module()),
        )
        .expect("std module source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0].module_path, "Result");
        assert!(lowered[0].ast.iter().any(
            |stmt| matches!(stmt, spire::ast::Ast::ResultCtorDecl(_, name, _, _, _) if name == "Ok")
        ));
        assert!(lowered[0].ast.iter().any(
            |stmt| matches!(stmt, spire::ast::Ast::Def(_, name, _, _, _, _) if name == "dummy")
        ));
    }

    #[test]
    fn lower_module_source_keeps_builtin_decls_global_even_with_single_defmod() {
        let ast = spire::parse_with_context(
            r#"@@builtin type Int
@@builtin def safe_mod(a: Int, b: Int) -> Result<Int>

defmod Int {
  def dummy() { () }
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::SourceRules::std_module()),
        )
        .expect("std module source should parse");

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
    fn collect_doc_entries_includes_deferror_docs() {
        let ast = spire::parse_with_context(
            r#"defmod Bootstrap {
  def dummy() { () }
}

@@doc """Missing value."""
deferror NoneError { "None Value." }"#,
            spire::ParserContext::module(1, None).with_rules(spire::SourceRules::std_module()),
        )
        .expect("std module source should parse");

        let lowered = lower_module_source_ast(ast, Some("Bootstrap"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                module_path: module.module_path,
                ast: module.ast,
                module_doc: module.module_doc,
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

        let err = parse_module_stages_from_compile_sources(
            &compile_sources,
            spire::CompileUnitKind::Script,
        )
        .expect_err("duplicate defmod path must fail");
        assert!(matches!(
            err.kind,
            ModuleStageParseErrorKind::DuplicateModulePath { ref module_path, .. } if module_path == "Shared"
        ));
    }

    #[test]
    fn strip_test_annotations_replaces_annotated_line_with_spaces() {
        let source =
            "defmod M {\n  @@test add(1, 2) == 3\n  def add(x: Int, y: Int) -> Int { x + y }\n}\n";
        let stripped = strip_test_annotations(source);

        assert!(!stripped.contains("@@test"));
        assert!(stripped.contains("def add(x: Int, y: Int) -> Int { x + y }"));
        assert_eq!(source.lines().count(), stripped.lines().count());
    }

    #[test]
    fn strip_test_annotations_is_noop_without_annotations() {
        let source = "defmod Kernel {\n  def add(x: Int, y: Int) -> Int { x + y }\n}\n";
        assert_eq!(strip_test_annotations(source), source);
    }
}
