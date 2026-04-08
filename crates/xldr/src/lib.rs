use spire::token::Token;

mod loader;
pub mod repl;
pub mod tui;

pub use loader::{
    collect_module_sources_with_module_file_stages, collect_module_sources_with_module_stages,
    collect_module_sources_with_modules, collect_module_sources_with_std_module_stages,
    compose_script_compile_sources, script_pseudo_module_path, CompileSources, LoadError,
    ModuleInput, ModuleSources, SourceDescriptor, SourceKind, StagedModule,
};

pub use repl::logic::core::{EldrLoadError, ReplEngine};
pub use repl::ui::cli::{cli_command, BannerMode, ReplOptions};

// ── Public types used by other crates ────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct LoweredModuleAst {
    pub module_path: String,
    pub ast: Vec<spire::ast::Ast>,
    pub declared_span: Option<spire::ast::Span>,
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
    let mut shared_non_module_defs = Vec::new();

    for stmt in ast {
        match stmt {
            spire::ast::Ast::Defmod(span, module_path, body) => {
                let mut module_ast = shared_imports.clone();
                module_ast.extend(body);
                lowered.push(LoweredModuleAst {
                    module_path,
                    ast: module_ast,
                    declared_span: Some(span),
                });
            }
            spire::ast::Ast::Import(_, _, _) => {}
            spire::ast::Ast::StructDef(_, _, _)
            | spire::ast::Ast::RecordDef(_, _, _)
            | spire::ast::Ast::DeferrorDef(_, _, _, _)
            | spire::ast::Ast::BuiltinDecl(_, _, _, _) => {
                shared_non_module_defs.push(stmt);
            }
            _ => {
                // Defensive fallback. Parser policy should keep this unreachable for module sources.
                shared_non_module_defs.push(stmt);
            }
        }
    }

    if !shared_non_module_defs.is_empty() {
        let mut shared_ast = shared_imports;
        shared_ast.extend(shared_non_module_defs);
        lowered.push(LoweredModuleAst {
            module_path: fallback_module_path.unwrap_or_default().to_string(),
            ast: shared_ast,
            declared_span: None,
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
