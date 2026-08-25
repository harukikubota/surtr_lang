use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sindr::names::{FacetRootKind, SymbolCapabilities, TypeIdentity};
use surtr_analysis::{
    resolve_context, AnalysisContextRequest, AnalysisDiagnosticKind, AnalysisHost, AnalysisMode,
    AnalysisService, CompletionKind, CompletionSymbol, ProjectRunnerInput,
    ProjectRunnerSourceInput, ReplCompletionUseSite, RunnerContext, RunnerSelection,
    SelectedContext, SemanticIndex, SymbolDisplayMetadata, SymbolSemanticInfo, Utf16Position,
};

#[derive(Debug)]
struct MemoryHost {
    files: HashMap<PathBuf, String>,
}

impl MemoryHost {
    fn new(files: impl IntoIterator<Item = (PathBuf, String)>) -> Self {
        Self {
            files: files.into_iter().collect(),
        }
    }
}

impl AnalysisHost for MemoryHost {
    fn read_to_string(&self, path: &Path) -> Option<String> {
        self.files.get(path).cloned()
    }

    fn resolve_project_runner(&self, input: ProjectRunnerInput) -> RunnerContext {
        surtr_analysis::resolve_project_runner_with(input, |path| self.read_to_string(path))
    }
}

#[test]
fn analysis_service_updates_documents_and_parses_active_context() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    service.update_document(path.clone(), Some(1), "value = 1".to_string());

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);

    assert_eq!(snapshot.context.context.mode, AnalysisMode::Script);
    assert!(snapshot.ast.is_some());
    assert!(service
        .diagnostics(&snapshot)
        .iter()
        .all(|diagnostic| diagnostic.kind != AnalysisDiagnosticKind::Parse));
}

#[test]
fn analysis_service_maps_parse_diagnostics_to_utf16_ranges() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/lib/user.srt");
    service.update_document(
        path.clone(),
        Some(1),
        "@builtin def print() -> Unit".to_string(),
    );

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path,
        selected_context: Some(SelectedContext::DefinitionStandalone),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let diagnostics = service.diagnostics(&snapshot);

    let parse = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.kind == AnalysisDiagnosticKind::Parse)
        .expect("definition source should reject builtin declarations");
    assert!(parse.range.is_some());
    assert!(parse.message.contains("@builtin") || !parse.message.is_empty());
}

#[test]
fn analysis_service_maps_spire_character_spans_to_utf16_ranges() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    let source = "value = \"😀\" )";
    service.update_document(path.clone(), Some(1), source.to_string());

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let diagnostics = service.diagnostics(&snapshot);

    let parse = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.kind == AnalysisDiagnosticKind::Parse)
        .expect("parse error should be reported");
    let range = parse.range.expect("parse diagnostic should have a range");
    let paren_byte = source.find(')').expect("broken token should exist");
    let expected_utf16_character = source[..paren_byte].encode_utf16().count() as u32;
    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, expected_utf16_character);
}

#[test]
fn analysis_service_does_not_resolve_or_typecheck_tolerant_parse_results() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    service.update_document(
        path.clone(),
        Some(1),
        "bad = )\nmissing = missing_name".to_string(),
    );

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let diagnostics = service.diagnostics(&snapshot);

    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == AnalysisDiagnosticKind::Parse));
    assert!(!diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == AnalysisDiagnosticKind::Resolve));
    assert!(!diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == AnalysisDiagnosticKind::Typecheck));
}

#[test]
fn analysis_service_keeps_parameterized_where_rhs_in_parse_category() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/lib/user.srt");
    service.update_document(
        path.clone(),
        Some(1),
        r#"deftrait Marker<$Tag> {
  def mark::<$Tag>(self: Self) -> $Tag
}

def mark(value: $A) -> String
where
  $A: Marker<Int>
{
  Marker::mark::<Int>(value)
}"#
        .to_string(),
    );

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::DefinitionStandalone),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let diagnostics = service.diagnostics(&snapshot);

    let parse = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.kind == AnalysisDiagnosticKind::Parse)
        .expect("parameterized where RHS should remain a parser diagnostic");
    assert!(parse.message.contains("Parameterized trait bounds"));
    assert!(!diagnostics.iter().any(|diagnostic| matches!(
        diagnostic.kind,
        AnalysisDiagnosticKind::Resolve | AnalysisDiagnosticKind::Typecheck
    )));
}

#[test]
fn analysis_service_document_symbols_use_tolerant_outline_when_parse_fails() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/lib/user.srt");
    service.update_document(
        path.clone(),
        Some(1),
        r#"def ok() -> Int { 1 }
broken = )
def next() -> Int { 2 }"#
            .to_string(),
    );

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::DefinitionStandalone),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let symbols = service.document_symbols(&snapshot, &path);

    assert!(service
        .diagnostics(&snapshot)
        .iter()
        .any(|diagnostic| diagnostic.kind == AnalysisDiagnosticKind::Parse));
    assert!(symbols.iter().any(|symbol| symbol.name.ends_with("ok")));
    assert!(symbols.iter().any(|symbol| symbol.name.ends_with("next")));
}

#[test]
fn analysis_service_maps_resolve_diagnostics_to_utf16_ranges() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    service.update_document(path.clone(), Some(1), "value = missing_name".to_string());

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);

    let resolve = service
        .diagnostics(&snapshot)
        .into_iter()
        .find(|diagnostic| diagnostic.kind == AnalysisDiagnosticKind::Resolve)
        .expect("missing name should produce a resolve diagnostic");
    assert!(resolve.range.is_some());
    assert!(resolve.message.contains("missing_name"));
}

#[test]
fn analysis_service_maps_typecheck_diagnostics_to_utf16_ranges() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    service.update_document(path.clone(), Some(1), "value: Int = \"bad\"".to_string());

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);

    let typecheck = service
        .diagnostics(&snapshot)
        .into_iter()
        .find(|diagnostic| diagnostic.kind == AnalysisDiagnosticKind::Typecheck)
        .expect("type mismatch should produce a typecheck diagnostic");
    assert!(typecheck.range.is_some());
    assert!(typecheck.message.contains("expected Int"));
}

#[test]
fn analysis_service_resolves_load_project_script_context_from_literal_directive() {
    let root = temp_root("load-project");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let script_path = root.join("scripts").join("seed.srt");
    std::fs::create_dir_all(script_path.parent().expect("script parent")).expect("create scripts");
    let project_file = root.join("project.srt");
    let module_path = src.join("main.srt");
    std::fs::write(&module_path, "defmod Main { def main() -> Int { 1 } }").expect("write module");
    std::fs::write(
        &project_file,
        r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::entry_fun(c, "Main::main")
    |> Config::add_path("./src/main.srt")
  })
})
"#,
    )
    .expect("write project");

    let mut service = AnalysisService::new();
    service.update_document(
        script_path.clone(),
        Some(1),
        r#"load_project("../project.srt", profile: "dev")

Seeder::run()
"#
        .to_string(),
    );

    let resolved = service.resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: script_path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(script_path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });

    assert_eq!(resolved.context.mode, AnalysisMode::Script);
    let script_project = resolved
        .script_project
        .expect("script should carry project context");
    assert_eq!(script_project.project_file, Some(project_file));
    assert_eq!(script_project.profile, Some("dev".to_string()));
    assert!(script_project.diagnostics.is_empty());
    assert_eq!(
        script_project
            .project_context
            .as_ref()
            .map(|context| context.selected_profile.as_str()),
        Some("dev")
    );
    let runner = resolved
        .runner
        .expect("load_project should resolve runner context");
    assert_eq!(runner.selected_profile, "dev");
    assert_eq!(runner.resolved_paths[0].literal_or_glob, "./src/main.srt");
    assert_eq!(runner.resolved_paths[0].expanded_files, vec![module_path]);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_load_project_uses_injected_host_sources() {
    let script_path = PathBuf::from("/repo/scripts/seed.srt");
    let project_path = PathBuf::from("/repo/project.srt");
    let module_path = PathBuf::from("/repo/src/main.srt");
    let mut service = AnalysisService::with_host(Arc::new(MemoryHost::new([
        (
            project_path.clone(),
            r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::entry_fun(c, "Main::main")
    |> Config::add_path("./src/main.srt")
  })
})
"#
            .to_string(),
        ),
        (
            module_path.clone(),
            "defmod Main { def main() -> Int { 1 } }".to_string(),
        ),
    ])));
    service.update_document(
        script_path.clone(),
        Some(1),
        r#"load_project("../project.srt", profile: "dev")
Main::ma"#
            .to_string(),
    );

    let context = service.resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: script_path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(script_path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);

    let project_context = snapshot
        .context
        .script_project
        .as_ref()
        .and_then(|project| project.project_context.as_ref())
        .expect("load_project should resolve project context through injected host");
    assert!(
        !project_context.resolved_paths.is_empty(),
        "project context should include resolved paths through injected host"
    );
}

#[test]
fn analysis_service_definition_uses_injected_host_sources_for_line_index() {
    let target_path = PathBuf::from("/repo/lib/helper.srt");
    let mut service = AnalysisService::with_host(Arc::new(MemoryHost::new([(
        target_path.clone(),
        "def helper() -> Int { 1 }\n".to_string(),
    )])));
    let active_path = PathBuf::from("/repo/main.srt");
    service.update_document(active_path.clone(), Some(1), "helper".to_string());

    let index = SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "helper".to_string(),
        replacement: "helper".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("helper() -> Int".to_string()),
        documentation: None,
        sort_text: None,
        origin: None,
        definition: Some(surtr_analysis::SourceLocation {
            path: target_path.clone(),
            start: 4,
            end: 10,
        }),
        capabilities: None,
    }]);
    service.set_semantic_index(index);

    let context = service.resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: active_path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(active_path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let definitions = service.definition(
        &snapshot,
        Utf16Position {
            line: 0,
            character: "helper".encode_utf16().count() as u32,
        },
    );

    let definition = definitions
        .into_iter()
        .find(|location| location.path == target_path)
        .expect("definition should resolve through injected host source");
    assert_eq!(definition.range.start.line, 0);
    assert_eq!(definition.range.start.character, 4);
    assert_eq!(definition.range.end.character, 10);
}

#[test]
fn analysis_service_analyzes_load_project_script_body_under_project_context() {
    let root = temp_root("load-project-script-body");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let script_path = root.join("scripts").join("seed.srt");
    std::fs::create_dir_all(script_path.parent().expect("script parent")).expect("create scripts");
    let project_file = root.join("project.srt");
    let module_path = src.join("main.srt");
    std::fs::write(&module_path, "defmod Main { def main() -> Int { 1 } }").expect("write module");
    std::fs::write(
        &project_file,
        r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::entry_fun(c, "Main::main")
    |> Config::add_path("./src/main.srt")
  })
})
"#,
    )
    .expect("write project");

    let mut service = AnalysisService::new();
    service.update_document(
        script_path.clone(),
        Some(1),
        r#"load_project("../project.srt", profile: "dev")

value = missing_name
"#
        .to_string(),
    );

    let context = service.resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: script_path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(script_path.clone())),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let diagnostics = service.diagnostics(&snapshot);

    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == AnalysisDiagnosticKind::Resolve
                && diagnostic.path == script_path
                && diagnostic.message.contains("missing_name")
        }),
        "script body should be resolved under load_project context: {diagnostics:?}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_indexes_active_load_project_script_owner_identities() {
    let root = temp_root("load-project-script-owner-identities");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let script_path = root.join("scripts").join("seed.srt");
    std::fs::create_dir_all(script_path.parent().expect("script parent")).expect("create scripts");
    let project_file = root.join("project.srt");
    let module_path = src.join("main.srt");
    std::fs::write(&module_path, "defmod Main { def main() -> Int { 1 } }").expect("write module");
    std::fs::write(
        &project_file,
        r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::entry_fun(c, "Main::main")
    |> Config::add_path("./src/main.srt")
  })
})
"#,
    )
    .expect("write project");

    let mut service = AnalysisService::new();
    service.update_document(
        script_path.clone(),
        Some(1),
        r#"load_project("../project.srt", profile: "dev")"#.to_string(),
    );
    let context = service.resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: script_path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(script_path.clone())),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    service.update_document(
        script_path.clone(),
        Some(2),
        r#"type Alias = (Int -> Int)

deftrait Show {
  def show(self: Self) -> String
}
"#
        .to_string(),
    );

    let snapshot = service.analyze(context);
    let diagnostics = service.diagnostics(&snapshot);
    assert!(
        diagnostics.is_empty(),
        "active project script should analyze: {diagnostics:?}"
    );

    for (name, identity, kind) in [
        ("Alias", TypeIdentity::Sig, CompletionKind::TypePath),
        ("Show", TypeIdentity::Trait, CompletionKind::TypePath),
        (
            "Show::show",
            TypeIdentity::Trait,
            CompletionKind::FunctionCall,
        ),
    ] {
        let matching = snapshot
            .semantic_index
            .symbol_semantic_infos()
            .iter()
            .filter(|info| info.canonical_name == name && info.kind == kind)
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "{name}: {matching:?}");
        assert_eq!(matching[0].identity, Some(identity), "{name}");
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_project_context_resolves_symbols_from_runner_module_stage() {
    let root = temp_root("project-stage");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let helper_path = src.join("helper.srt");
    let main_path = src.join("main.srt");
    let project_file = root.join("project.srt");
    std::fs::write(&helper_path, "defmod Helper { def helper() -> Int { 1 } }")
        .expect("write helper");
    std::fs::write(
        &main_path,
        "import Helper::helper\ndefmod Main { def main() -> Int { helper() } }",
    )
    .expect("write main");
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/helper.srt")
    |> Config::add_path("./src/main.srt")
  })
})
"#;

    let mut service = AnalysisService::new();
    service.update_document(
        main_path.clone(),
        Some(1),
        "import Helper::helper\ndefmod Main { def main() -> Int { helper() } }".to_string(),
    );
    let context = resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: main_path.clone(),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: project_file.clone(),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            runner_result: None,
            source: Some(ProjectRunnerSourceInput {
                project_file,
                selected_profile: "dev".to_string(),
                normalized_args: vec![("profile".to_string(), "dev".to_string())],
                active_file: Some(main_path),
                source: project_source.to_string(),
            }),
        }),
        open_documents: service.document_store().open_document_versions(),
    });

    let snapshot = service.analyze(context);
    let diagnostics = service.diagnostics(&snapshot);

    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.kind != AnalysisDiagnosticKind::Resolve),
        "project module stage should satisfy helper reference: {diagnostics:?}"
    );
    assert!(
        snapshot.resolved.is_some(),
        "project context should produce resolved nodes: {diagnostics:?}"
    );
    assert!(
        snapshot.typed.is_some(),
        "project context should produce typed nodes: {diagnostics:?}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_project_context_lowers_const_only_file_under_file_module_path() {
    let root = temp_root("project-const-only-module");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let config_path = src.join("Config.srt");
    let main_path = src.join("main.srt");
    let project_file = root.join("project.srt");
    std::fs::write(&config_path, "const VERSION: Int = 1").expect("write config");
    let main_source = "defmod Main { def main() -> Int { Config::VERSION } }";
    std::fs::write(&main_path, main_source).expect("write main");
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/Config.srt")
    |> Config::add_path("./src/main.srt")
  })
})
"#;

    let mut service = AnalysisService::new();
    service.update_document(main_path.clone(), Some(1), main_source.to_string());
    let context = resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: main_path.clone(),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: project_file.clone(),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            runner_result: None,
            source: Some(ProjectRunnerSourceInput {
                project_file,
                selected_profile: "dev".to_string(),
                normalized_args: vec![("profile".to_string(), "dev".to_string())],
                active_file: Some(main_path),
                source: project_source.to_string(),
            }),
        }),
        open_documents: service.document_store().open_document_versions(),
    });

    let snapshot = service.analyze(context);
    let diagnostics = service.diagnostics(&snapshot);

    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.kind != AnalysisDiagnosticKind::Resolve),
        "const-only project file should lower under Config module path: {diagnostics:?}"
    );
    assert!(
        snapshot.typed.is_some(),
        "const-only project stage should typecheck: {diagnostics:?}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_project_context_builds_completion_index_from_runner_module_stage() {
    let root = temp_root("project-completion");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let helper_path = src.join("helper.srt");
    let main_path = src.join("main.srt");
    let project_file = root.join("project.srt");
    std::fs::write(&helper_path, "defmod Helper { def helper() -> Int { 1 } }")
        .expect("write helper");
    std::fs::write(&main_path, "import Helper::he").expect("write main");
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/helper.srt")
    |> Config::add_path("./src/main.srt")
  })
})
"#;

    let mut service = AnalysisService::new();
    service.update_document(main_path.clone(), Some(1), "import Helper::he".to_string());
    let context = resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: main_path.clone(),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: project_file.clone(),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            runner_result: None,
            source: Some(ProjectRunnerSourceInput {
                project_file,
                selected_profile: "dev".to_string(),
                normalized_args: vec![("profile".to_string(), "dev".to_string())],
                active_file: Some(main_path),
                source: project_source.to_string(),
            }),
        }),
        open_documents: service.document_store().open_document_versions(),
    });

    let snapshot = service.analyze(context);
    let completion = service.completions(
        &snapshot,
        Utf16Position {
            line: 0,
            character: "import Helper::he".len() as u32,
        },
    );

    assert!(
        completion
            .candidates
            .iter()
            .any(|candidate| candidate.label == "Helper::helper"),
        "project declarations should populate completion candidates: {completion:?}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_project_completion_excludes_private_module_declarations() {
    let root = temp_root("project-completion-private-hidden");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let helper_path = src.join("helper.srt");
    let main_path = src.join("main.srt");
    let project_file = root.join("project.srt");
    std::fs::write(
        &helper_path,
        "defmod Helper { defp secret() -> Int { 1 } def public() -> Int { 2 } }",
    )
    .expect("write helper");
    std::fs::write(&main_path, "sec").expect("write main");
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/helper.srt")
    |> Config::add_path("./src/main.srt")
  })
})
"#;

    let mut service = AnalysisService::new();
    service.update_document(main_path.clone(), Some(1), "sec".to_string());
    let context = resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: main_path.clone(),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: project_file.clone(),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            runner_result: None,
            source: Some(ProjectRunnerSourceInput {
                project_file,
                selected_profile: "dev".to_string(),
                normalized_args: vec![("profile".to_string(), "dev".to_string())],
                active_file: Some(main_path),
                source: project_source.to_string(),
            }),
        }),
        open_documents: service.document_store().open_document_versions(),
    });

    let snapshot = service.analyze(context);
    let completion = service.completions(
        &snapshot,
        Utf16Position {
            line: 0,
            character: 3,
        },
    );

    assert!(
        completion
            .candidates
            .iter()
            .all(|candidate| candidate.label != "Helper::secret"),
        "private declarations must not leak through raw compile metadata: {completion:?}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_project_completion_excludes_unimported_module_members() {
    let root = temp_root("project-completion-unimported-hidden");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let helper_path = src.join("helper.srt");
    let main_path = src.join("main.srt");
    let project_file = root.join("project.srt");
    std::fs::write(&helper_path, "defmod Helper { def helper() -> Int { 1 } }")
        .expect("write helper");
    std::fs::write(&main_path, "he").expect("write main");
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/helper.srt")
    |> Config::add_path("./src/main.srt")
  })
})
"#;

    let mut service = AnalysisService::new();
    service.update_document(main_path.clone(), Some(1), "he".to_string());
    let context = resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: main_path.clone(),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: project_file.clone(),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            runner_result: None,
            source: Some(ProjectRunnerSourceInput {
                project_file,
                selected_profile: "dev".to_string(),
                normalized_args: vec![("profile".to_string(), "dev".to_string())],
                active_file: Some(main_path),
                source: project_source.to_string(),
            }),
        }),
        open_documents: service.document_store().open_document_versions(),
    });

    let snapshot = service.analyze(context);
    let completion = service.completions(
        &snapshot,
        Utf16Position {
            line: 0,
            character: 2,
        },
    );

    assert!(
        completion
            .candidates
            .iter()
            .all(|candidate| candidate.label != "Helper::helper"),
        "unimported module members must not leak through raw compile metadata: {completion:?}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_project_context_indexes_effective_imported_short_name() {
    let root = temp_root("project-imported-short-index");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let helper_path = src.join("helper.srt");
    let main_path = src.join("main.srt");
    let project_file = root.join("project.srt");
    std::fs::write(&helper_path, "defmod Helper { def helper() -> Int { 1 } }")
        .expect("write helper");
    std::fs::write(&main_path, "import Helper::helper").expect("write main");
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/helper.srt")
    |> Config::add_path("./src/main.srt")
  })
})
"#;

    let mut service = AnalysisService::new();
    service.update_document(
        main_path.clone(),
        Some(1),
        "import Helper::helper".to_string(),
    );
    let context = resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: main_path.clone(),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: project_file.clone(),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            runner_result: None,
            source: Some(ProjectRunnerSourceInput {
                project_file,
                selected_profile: "dev".to_string(),
                normalized_args: vec![("profile".to_string(), "dev".to_string())],
                active_file: Some(main_path),
                source: project_source.to_string(),
            }),
        }),
        open_documents: service.document_store().open_document_versions(),
    });

    let snapshot = service.analyze(context);
    let helper_symbols = snapshot
        .semantic_index
        .symbols()
        .iter()
        .filter(|symbol| symbol.label == "helper")
        .cloned()
        .collect::<Vec<_>>();

    assert!(
        helper_symbols.iter().any(|symbol| {
            matches!(
                symbol.origin.as_ref(),
                Some(surtr_analysis::CompletionOrigin::Declaration { qualified_name, .. })
                    if qualified_name == "Global::Helper::helper"
            )
        }),
        "effective import should expose short declaration symbol: {helper_symbols:?}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_project_context_preserves_existing_semantic_infos() {
    let root = temp_root("project-existing-semantic-infos");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let main_path = src.join("main.srt");
    let project_file = root.join("project.srt");
    std::fs::write(&main_path, "defmod Main { def main() -> Int { 1 } }").expect("write main");
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/main.srt")
  })
})
"#;

    let mut service = AnalysisService::new();
    service.set_semantic_index(SemanticIndex::from_symbol_semantic_infos(vec![
        SymbolSemanticInfo {
            canonical_name: "Global::External::helper".to_string(),
            surface_name: "External::helper".to_string(),
            replacement: "External::helper".to_string(),
            kind: CompletionKind::FunctionCall,
            identity: None,
            detail: Some("External::helper() -> Int".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
            display_metadata: Some(SymbolDisplayMetadata {
                qualified_name: "Global::External::helper".to_string(),
                module_path: "Global::External".to_string(),
                has_doc: false,
                has_signature: true,
            }),
        },
    ]));
    service.update_document(
        main_path.clone(),
        Some(1),
        "defmod Main { def main() -> Int { 1 } }".to_string(),
    );

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: main_path.clone(),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: project_file.clone(),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            runner_result: None,
            source: Some(ProjectRunnerSourceInput {
                project_file,
                selected_profile: "dev".to_string(),
                normalized_args: vec![("profile".to_string(), "dev".to_string())],
                active_file: Some(main_path),
                source: project_source.to_string(),
            }),
        }),
        open_documents: service.document_store().open_document_versions(),
    });

    let snapshot = service.analyze(context);
    let external = snapshot
        .semantic_index
        .symbol_semantic_infos()
        .iter()
        .find(|info| info.surface_name == "External::helper")
        .expect("existing semantic info should survive project context enrichment");
    assert_eq!(
        external
            .display_metadata
            .as_ref()
            .map(|metadata| (metadata.qualified_name.as_str(), metadata.has_signature)),
        Some(("Global::External::helper", true))
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_project_context_imported_short_name_inherits_compile_metadata() {
    let root = temp_root("project-imported-short-metadata");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let helper_path = src.join("helper.srt");
    let main_path = src.join("main.srt");
    let project_file = root.join("project.srt");
    let helper_source = r#"defmod Helper {
  @doc """
  Increment a number.
  """
  def helper(value: Int) -> Int { value + 1 }
}"#;
    let main_source = "import Helper::helper\ndefmod Main { def main() -> Int { helper(1) } }";
    std::fs::write(&helper_path, helper_source).expect("write helper");
    std::fs::write(&main_path, main_source).expect("write main");
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/helper.srt")
    |> Config::add_path("./src/main.srt")
  })
})
"#;

    let mut service = AnalysisService::new();
    service.update_document(main_path.clone(), Some(1), main_source.to_string());
    let context = resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: main_path.clone(),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: project_file.clone(),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            runner_result: None,
            source: Some(ProjectRunnerSourceInput {
                project_file,
                selected_profile: "dev".to_string(),
                normalized_args: vec![("profile".to_string(), "dev".to_string())],
                active_file: Some(main_path.clone()),
                source: project_source.to_string(),
            }),
        }),
        open_documents: service.document_store().open_document_versions(),
    });

    let snapshot = service.analyze(context);
    let helper_symbol = snapshot
        .semantic_index
        .symbols()
        .iter()
        .find(|symbol| symbol.label == "helper")
        .expect("effective import should expose helper");
    assert_eq!(
        helper_symbol.detail.as_deref(),
        Some("helper(value: Int) -> Int")
    );
    assert_eq!(
        helper_symbol.documentation.as_deref().map(str::trim),
        Some("Increment a number.")
    );

    let hover_line = main_source.lines().nth(1).expect("call line");
    let hover_column = hover_line.find("helper").expect("helper call exists") as u32 + 3;
    let hover = service
        .hover(
            &snapshot,
            Utf16Position {
                line: 1,
                character: hover_column,
            },
        )
        .expect("imported helper should produce hover");
    assert!(
        hover.contents.contains("helper(value: Int) -> Int"),
        "{hover:?}"
    );
    assert!(hover.contents.contains("Increment a number."), "{hover:?}");

    let signature_column = hover_line.find("helper(1").expect("call exists") + "helper(1".len();
    let signature_column = signature_column as u32;
    let help = service
        .signature_help(
            &snapshot,
            Utf16Position {
                line: 1,
                character: signature_column,
            },
        )
        .expect("imported helper should produce signature help");
    assert_eq!(
        help.signatures,
        vec!["helper(value: Int) -> Int".to_string()]
    );
    assert_eq!(help.active_parameter, Some(0));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_project_context_rejects_set_exit_code_outside_entrypoint() {
    let root = temp_root("project-set-exit-code");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let helper_path = src.join("helper.srt");
    let main_path = src.join("main.srt");
    let project_file = root.join("project.srt");
    std::fs::write(
        &helper_path,
        "defmod Helper { def helper() -> Unit { set_exit_code(9) } }",
    )
    .expect("write helper");
    std::fs::write(&main_path, "defmod Main { def main() -> Int { 1 } }").expect("write main");
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::entry_fun(c, "Main::main")
    |> Config::add_path("./src/helper.srt")
    |> Config::add_path("./src/main.srt")
  })
})
"#;

    let mut service = AnalysisService::new();
    service.update_document(
        helper_path.clone(),
        Some(1),
        "defmod Helper { def helper() -> Unit { set_exit_code(9) } }".to_string(),
    );
    let context = resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: helper_path.clone(),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: project_file.clone(),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            runner_result: None,
            source: Some(ProjectRunnerSourceInput {
                project_file,
                selected_profile: "dev".to_string(),
                normalized_args: vec![("profile".to_string(), "dev".to_string())],
                active_file: Some(helper_path.clone()),
                source: project_source.to_string(),
            }),
        }),
        open_documents: service.document_store().open_document_versions(),
    });

    let snapshot = service.analyze(context);
    let diagnostics = service.diagnostics(&snapshot);

    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == AnalysisDiagnosticKind::Typecheck
                && diagnostic.path == helper_path
                && diagnostic
                    .message
                    .contains("set_exit_code is only allowed inside entrypoint")
        }),
        "project context should reject set_exit_code outside entrypoint: {diagnostics:?}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_completions_use_snapshot_semantic_index_and_utf16_position() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    service.update_document(path.clone(), Some(1), "pri".to_string());
    service.set_semantic_index(SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "print".to_string(),
        replacement: "print".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("print(a: String) -> Unit".to_string()),
        documentation: None,
        sort_text: None,
        origin: None,

        definition: None,

        capabilities: None,
    }]));

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let completion = service.completions(
        &snapshot,
        Utf16Position {
            line: 0,
            character: 3,
        },
    );

    assert_eq!(completion.candidates.len(), 1);
    assert_eq!(completion.candidates[0].label, "print");
    assert_eq!(completion.replace_start, 0);
    assert_eq!(completion.replace_end, 3);
}

#[test]
fn analysis_service_completions_use_facet_api_first_argument_constraints() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    service.update_document(path.clone(), Some(1), "Facet::set(".to_string());
    service.set_semantic_index(SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "Facet::set".to_string(),
            replacement: "Facet::set".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some(
                "set(facet: Facet<WritablePath, $S, $A, $T, $B>, source: $S, value: $B) -> Result<$T>".to_string(),
            ),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "User".to_string(),
            replacement: "User".to_string(),
            kind: CompletionKind::TypeConstructor,
            detail: None,
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: Some(SymbolCapabilities::new(
                true,
                true,
                true,
                Some(FacetRootKind::TypeRoot),
            )),
        },
        CompletionSymbol {
            label: "String".to_string(),
            replacement: "String".to_string(),
            kind: CompletionKind::TypeConstructor,
            detail: Some("type String".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "name_path".to_string(),
            replacement: "name_path".to_string(),
            kind: CompletionKind::Variable,
            detail: Some("Facet<InfallibleStructural, User, String, _, _>".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "user".to_string(),
            replacement: "user".to_string(),
            kind: CompletionKind::Variable,
            detail: Some("User".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
    ]));

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let completion = service.completions(
        &snapshot,
        Utf16Position {
            line: 0,
            character: "Facet::set(".len() as u32,
        },
    );
    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"User"), "{labels:?}");
    assert!(labels.contains(&"name_path"), "{labels:?}");
    assert!(!labels.contains(&"String"), "{labels:?}");
    assert!(!labels.contains(&"user"), "{labels:?}");
}

#[test]
fn analysis_service_facet_arg_completion_uses_source_location_root_capabilities() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    let source = "defrecord User(name: String)\nFacet::view(User.name, user)";
    service.update_document(path.clone(), Some(1), source.to_string());
    service.set_semantic_index(SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "Facet::view".to_string(),
        replacement: "Facet::view".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some(
            "view(facet: Facet<ReadablePath, $S, $A, _, _>, source: $S) -> Result<$A>".to_string(),
        ),
        documentation: None,
        sort_text: None,
        origin: None,
        definition: None,
        capabilities: None,
    }]));

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let completion = service.completions(
        &snapshot,
        Utf16Position {
            line: 1,
            character: "Facet::view(".len() as u32,
        },
    );
    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"User"), "{labels:?}");
}

#[test]
fn analysis_service_facet_arg_completion_uses_call_signature_not_name() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    service.update_document(path.clone(), Some(1), "view(".to_string());
    service.set_semantic_index(SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "view".to_string(),
            replacement: "view".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("view(value: Int) -> Int".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "User".to_string(),
            replacement: "User".to_string(),
            kind: CompletionKind::TypeConstructor,
            detail: Some("defrecord User".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "name_path".to_string(),
            replacement: "name_path".to_string(),
            kind: CompletionKind::Variable,
            detail: Some("Facet<InfallibleStructural, User, String, _, _>".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
    ]));

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let completion = service.completions(
        &snapshot,
        Utf16Position {
            line: 0,
            character: "view(".len() as u32,
        },
    );
    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(!labels.contains(&"User"), "{labels:?}");
    assert!(!labels.contains(&"name_path"), "{labels:?}");
}

#[test]
fn analysis_service_repl_assist_uses_repl_scope_and_signature_help() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    service.update_document(path.clone(), Some(1), "print(".to_string());
    service.set_semantic_index(SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "print".to_string(),
            replacement: "print".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("print(value: String) -> Unit".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "name".to_string(),
            replacement: "name".to_string(),
            kind: CompletionKind::Variable,
            detail: Some("String".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
    ]));

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let assist = service.repl_assist(
        &snapshot,
        Utf16Position {
            line: 0,
            character: "print(".len() as u32,
        },
        ReplCompletionUseSite::Input,
    );

    assert_eq!(
        assist
            .signature
            .as_ref()
            .map(|signature| signature.signature.as_str()),
        Some("print(value: String) -> Unit")
    );
    assert_eq!(assist.active_parameter, Some(0));
    assert_eq!(assist.candidates.len(), 1);
    assert_eq!(assist.candidates[0].label, "name");
}

#[test]
fn analysis_service_hover_uses_snapshot_semantic_index_and_token_range() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    service.update_document(path.clone(), Some(1), "value = print".to_string());
    service.set_semantic_index(SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "print".to_string(),
        replacement: "print".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("print(a: String) -> Unit".to_string()),
        documentation: Some("Writes a line.".to_string()),
        sort_text: None,
        origin: None,

        definition: None,

        capabilities: None,
    }]));

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let hover = service
        .hover(
            &snapshot,
            Utf16Position {
                line: 0,
                character: "value = pr".len() as u32,
            },
        )
        .expect("semantic symbol should produce hover");

    assert_eq!(hover.contents, "print(a: String) -> Unit\n\nWrites a line.");
    let range = hover.range.expect("hover should include token range");
    assert_eq!(range.start.character, "value = ".len() as u32);
    assert_eq!(range.end.character, "value = print".len() as u32);
}

#[test]
fn analysis_service_signature_help_uses_snapshot_semantic_index() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    let source = "value = print(\"hello\", Tr";
    service.update_document(path.clone(), Some(1), source.to_string());
    service.set_semantic_index(SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "print".to_string(),
        replacement: "print".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("print(value: String, newline: Bool) -> Unit".to_string()),
        documentation: Some("Writes a line.".to_string()),
        sort_text: None,
        origin: None,

        definition: None,

        capabilities: None,
    }]));

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path)),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let help = service
        .signature_help(
            &snapshot,
            Utf16Position {
                line: 0,
                character: source.len() as u32,
            },
        )
        .expect("semantic symbol should produce signature help");

    assert_eq!(
        help.signatures,
        vec!["print(value: String, newline: Bool) -> Unit".to_string()]
    );
    assert_eq!(help.active_signature, Some(0));
    assert_eq!(help.active_parameter, Some(1));
}

#[test]
fn analysis_service_definition_uses_active_document_semantic_locations() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    let source = "def helper() -> Int { 1 }\nvalue = helper()";
    service.update_document(path.clone(), Some(1), source.to_string());

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: path.clone(),
        selected_context: Some(SelectedContext::ScriptEntry(path.clone())),
        runner_selection: None,
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let locations = service.definition(
        &snapshot,
        Utf16Position {
            line: 1,
            character: "value = help".len() as u32,
        },
    );

    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].path, path);
    assert_eq!(locations[0].range.start.line, 0);
    assert_eq!(locations[0].range.start.character, 0);
    assert!(locations[0].range.end.character > locations[0].range.start.character);
}

#[test]
fn analysis_service_definition_resolves_project_stage_source_locations() {
    let root = temp_root("project-definition");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let helper_path = src.join("helper.srt");
    let main_path = src.join("main.srt");
    let project_file = root.join("project.srt");
    let helper_source = "defmod Helper { def helper() -> Int { 1 } }";
    let main_source = "import Helper::helper\ndefmod Main { def main() -> Int { helper() } }";
    std::fs::write(&helper_path, helper_source).expect("write helper");
    std::fs::write(&main_path, main_source).expect("write main");
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/helper.srt")
    |> Config::add_path("./src/main.srt")
  })
})
"#;

    let mut service = AnalysisService::new();
    service.update_document(main_path.clone(), Some(1), main_source.to_string());
    let context = resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: main_path.clone(),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: project_file.clone(),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            runner_result: None,
            source: Some(ProjectRunnerSourceInput {
                project_file,
                selected_profile: "dev".to_string(),
                normalized_args: vec![("profile".to_string(), "dev".to_string())],
                active_file: Some(main_path),
                source: project_source.to_string(),
            }),
        }),
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let call_column = main_source
        .lines()
        .nth(1)
        .and_then(|line| line.find("helper()"))
        .expect("helper call exists") as u32
        + 3;
    let locations = service.definition(
        &snapshot,
        Utf16Position {
            line: 1,
            character: call_column,
        },
    );

    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].path, helper_path);
    assert_eq!(locations[0].range.start.line, 0);
    assert_eq!(
        locations[0].range.start.character,
        helper_source.find("def helper").expect("helper def exists") as u32
    );
    assert!(locations[0].range.end.character > locations[0].range.start.character);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_document_symbols_flatten_active_declarations() {
    let root = temp_root("document-symbols");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let main_path = src.join("main.srt");
    let project_file = root.join("project.srt");
    let source = "defmod Main { def helper() -> Int { 1 } }";
    std::fs::write(&main_path, source).expect("write main");
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/main.srt")
  })
})
"#;

    let mut service = AnalysisService::new();
    service.update_document(main_path.clone(), Some(1), source.to_string());

    let context = resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: main_path.clone(),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: project_file.clone(),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            runner_result: None,
            source: Some(ProjectRunnerSourceInput {
                project_file,
                selected_profile: "dev".to_string(),
                normalized_args: vec![("profile".to_string(), "dev".to_string())],
                active_file: Some(main_path.clone()),
                source: project_source.to_string(),
            }),
        }),
        open_documents: service.document_store().open_document_versions(),
    });
    let snapshot = service.analyze(context);
    let symbols = service.document_symbols(&snapshot, &main_path);
    let names = symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["Main", "Main::helper"]);
    assert!(symbols
        .iter()
        .all(|symbol| symbol.range.end.character > symbol.range.start.character));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn analysis_service_owner_collision_uses_each_project_source_provenance() {
    let root = temp_root("project-owner-collision-provenance");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src dir");
    let first_path = src.join("a_first.srt");
    let collision_path = src.join("b_collision.srt");
    let active_path = src.join("c_active.srt");
    let project_file = root.join("project.srt");
    let first_source = "defrecord Shared(first: Int)";
    let collision_source = "deferror Shared { \"😀\" }";
    let active_source = "defmod Main { def main() -> Int { 1 } }";
    std::fs::write(&first_path, first_source).expect("write first owner source");
    std::fs::write(&collision_path, collision_source).expect("write conflicting owner source");
    std::fs::write(&active_path, active_source).expect("write active source");
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::entry_fun(c, "Main::main")
    |> Config::add_path("./src/a_first.srt")
    |> Config::add_path("./src/b_collision.srt")
    |> Config::add_path("./src/c_active.srt")
  })
})
"#;

    let mut service = AnalysisService::new();
    service.update_document(active_path.clone(), Some(1), active_source.to_string());
    let context = resolve_context(AnalysisContextRequest {
        workspace_root: root.clone(),
        active_file: active_path.clone(),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: project_file.clone(),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            runner_result: None,
            source: Some(ProjectRunnerSourceInput {
                project_file,
                selected_profile: "dev".to_string(),
                normalized_args: vec![("profile".to_string(), "dev".to_string())],
                active_file: Some(active_path.clone()),
                source: project_source.to_string(),
            }),
        }),
        open_documents: service.document_store().open_document_versions(),
    });

    let snapshot = service.analyze(context);
    let collision = service
        .diagnostics(&snapshot)
        .into_iter()
        .find(|diagnostic| diagnostic.message == "Duplicate top-level owner: Shared")
        .expect("owner collision should be reported");

    assert_eq!(collision.path, collision_path);
    let range = collision
        .range
        .expect("collision should have a local range");
    assert_eq!(
        range.start,
        Utf16Position {
            line: 0,
            character: 0
        }
    );
    assert_eq!(
        range.end,
        Utf16Position {
            line: 0,
            character: collision_source.encode_utf16().count() as u32,
        }
    );
    assert_eq!(collision.related.len(), 1);
    assert_eq!(collision.related[0].path, first_path);
    assert_eq!(
        collision.related[0].range.start,
        Utf16Position {
            line: 0,
            character: 0
        }
    );
    assert_eq!(
        collision.related[0].range.end,
        Utf16Position {
            line: 0,
            character: first_source.encode_utf16().count() as u32,
        }
    );
    assert_eq!(collision.related[0].message, "first Record declaration");

    let _ = std::fs::remove_dir_all(root);
}

fn temp_root(name: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("surtr-analysis-service-{name}-{nonce}"))
}
