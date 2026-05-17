use sindr::policy::{CompileUnitKind, SourceKind};
use std::path::PathBuf;
use surtr_analysis::{
    parse_document, AnalysisCacheInput, AnalysisCacheKey, AnalysisContext, AnalysisContextRequest,
    AnalysisMode, DocumentVersion, ModuleFileFingerprint, RunnerSelection, SelectedContext,
};

#[test]
fn parse_document_uses_std_definition_rules_for_builtin_declarations() {
    let ast = parse_document(
        "@builtin def print(a: String) -> Unit",
        0,
        SourceKind::StdDefinitionSource,
        CompileUnitKind::DefinitionCheck,
        Some("Kernel".to_string()),
    )
    .expect("std definition source should accept builtin declaration");

    assert_eq!(ast.len(), 1);
}

#[test]
fn parse_document_uses_definition_rules_for_user_modules() {
    let err = parse_document(
        "@builtin def print(a: String) -> Unit",
        0,
        SourceKind::DefinitionSource,
        CompileUnitKind::DefinitionCheck,
        Some("Kernel".to_string()),
    )
    .expect_err("user definition source must reject builtin declaration");

    assert!(!err.message().is_empty());
}

#[test]
fn parse_document_uses_project_context_for_project_sources() {
    let ast = parse_document(
        "Project::config()",
        0,
        SourceKind::DefinitionSource,
        CompileUnitKind::Project,
        None,
    )
    .expect("project context should accept top-level project expressions");

    assert_eq!(ast.len(), 1);
}

#[test]
fn analysis_context_request_preserves_explicit_selected_context() {
    let request = AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: PathBuf::from("/repo/src/user.srt"),
        selected_context: Some(SelectedContext::ScriptEntry(PathBuf::from(
            "/repo/main.srt",
        ))),
        runner_selection: None,
        open_documents: vec![DocumentVersion {
            path: PathBuf::from("/repo/src/user.srt"),
            version: Some(7),
            content_hash: "active-hash".to_string(),
        }],
    };

    assert!(matches!(
        request.selected_context,
        Some(SelectedContext::ScriptEntry(ref path)) if path == &PathBuf::from("/repo/main.srt")
    ));
    assert_eq!(request.open_documents[0].version, Some(7));
}

#[test]
fn analysis_cache_key_is_stable_for_unordered_runner_args_and_external_inputs() {
    let context = AnalysisContext {
        workspace_root: PathBuf::from("/repo"),
        mode: AnalysisMode::Project,
        entry_file: Some(PathBuf::from("/repo/project.srt")),
        active_file: PathBuf::from("/repo/src/user.srt"),
        source_kind: SourceKind::DefinitionSource,
    };
    let modules = vec![vec![ModuleFileFingerprint {
        path: PathBuf::from("/repo/src/user.srt"),
        source_kind: SourceKind::DefinitionSource,
        content_hash: "module-hash".to_string(),
    }]];

    let left = AnalysisCacheKey::new(AnalysisCacheInput {
        context: context.clone(),
        active_document_hash: "active".to_string(),
        stdlib_hash: "stdlib".to_string(),
        include_graph_hash: Some("includes".to_string()),
        module_stages: modules.clone(),
        runner_selection: Some(RunnerSelection {
            project_file: PathBuf::from("/repo/project.srt"),
            selected_profile: "test".to_string(),
            normalized_args: vec![
                ("profile".to_string(), "test".to_string()),
                ("env".to_string(), "ci".to_string()),
            ],
        }),
        project_runner_hash: Some("project-hash".to_string()),
        project_path_hashes: vec![("src/*.srt".to_string(), "paths".to_string())],
        boot_summary_hash: Some("boot".to_string()),
        external_inputs: vec![
            ("seed".to_string(), "ok".to_string()),
            ("config".to_string(), "missing".to_string()),
        ],
        load_project_hash: None,
        active_file_profiles: vec!["test".to_string(), "dev".to_string()],
    });

    let right = AnalysisCacheKey::new(AnalysisCacheInput {
        context,
        active_document_hash: "active".to_string(),
        stdlib_hash: "stdlib".to_string(),
        include_graph_hash: Some("includes".to_string()),
        module_stages: modules,
        runner_selection: Some(RunnerSelection {
            project_file: PathBuf::from("/repo/project.srt"),
            selected_profile: "test".to_string(),
            normalized_args: vec![
                ("env".to_string(), "ci".to_string()),
                ("profile".to_string(), "test".to_string()),
            ],
        }),
        project_runner_hash: Some("project-hash".to_string()),
        project_path_hashes: vec![("src/*.srt".to_string(), "paths".to_string())],
        boot_summary_hash: Some("boot".to_string()),
        external_inputs: vec![
            ("config".to_string(), "missing".to_string()),
            ("seed".to_string(), "ok".to_string()),
        ],
        load_project_hash: None,
        active_file_profiles: vec!["dev".to_string(), "test".to_string()],
    });

    assert_eq!(left, right);
    assert!(left
        .fields()
        .iter()
        .any(|field| field.name == "selected_profile" && field.value == "test"));
    assert!(left
        .fields()
        .iter()
        .any(|field| field.name == "boot_summary_hash" && field.value == "boot"));
}
