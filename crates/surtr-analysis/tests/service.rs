use std::path::PathBuf;

use surtr_analysis::{
    resolve_context, AnalysisContextRequest, AnalysisDiagnosticKind, AnalysisMode, AnalysisService,
    CompletionKind, CompletionSymbol, ProjectRunnerSourceInput, RunnerSelection, SelectedContext,
    SemanticIndex, Utf16Position,
};

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
fn analysis_service_completions_use_snapshot_semantic_index_and_utf16_position() {
    let mut service = AnalysisService::new();
    let path = PathBuf::from("/repo/main.srt");
    service.update_document(path.clone(), Some(1), "pri".to_string());
    service.set_semantic_index(SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "print".to_string(),
        replacement: "print".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("print(a: String) -> Unit".to_string()),
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

fn temp_root(name: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("surtr-analysis-service-{name}-{nonce}"))
}
