use sindr::policy::{CompileUnitKind, SourceKind};
use std::path::PathBuf;
use surtr_analysis::{
    parse_document, resolve_context, AnalysisCacheInput, AnalysisCacheKey, AnalysisContext,
    AnalysisContextRequest, AnalysisContextStatus, AnalysisMode, ContextDiagnosticKind,
    DocumentVersion, ModuleFileFingerprint, ProjectRunnerSourceInput, RunnerSelection,
    SelectedContext,
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
        SourceKind::ProjectConfigSource,
        CompileUnitKind::Project,
        None,
    )
    .expect("project context should accept top-level project expressions");

    assert_eq!(ast.len(), 1);
}

#[test]
fn project_config_source_is_not_a_std_definition_source() {
    let err = parse_document(
        "@builtin def print(a: String) -> Unit",
        0,
        SourceKind::ProjectConfigSource,
        CompileUnitKind::Project,
        None,
    )
    .expect_err("project config source must not accept std builtin declarations");

    assert!(!err.message().is_empty());
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
fn resolve_context_uses_explicit_script_entry_and_marks_included_files_as_definitions() {
    let resolved = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: PathBuf::from("/repo/src/user.srt"),
        selected_context: Some(SelectedContext::ScriptEntry(PathBuf::from(
            "/repo/main.srt",
        ))),
        runner_selection: None,
        open_documents: Vec::new(),
    });

    assert_eq!(resolved.status, AnalysisContextStatus::Ready);
    assert_eq!(resolved.context.mode, AnalysisMode::Script);
    assert_eq!(
        resolved.context.entry_file,
        Some(PathBuf::from("/repo/main.srt"))
    );
    assert_eq!(resolved.context.source_kind, SourceKind::DefinitionSource);
}

#[test]
fn resolve_context_auto_selects_stdlib_development_for_lib_sources() {
    let resolved = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: PathBuf::from("/repo/lib/kernel.srt"),
        selected_context: None,
        runner_selection: None,
        open_documents: Vec::new(),
    });

    assert_eq!(resolved.status, AnalysisContextStatus::Ready);
    assert_eq!(resolved.context.mode, AnalysisMode::DefinitionCheck);
    assert_eq!(
        resolved.context.source_kind,
        SourceKind::StdDefinitionSource
    );
}

#[test]
fn resolve_context_auto_selects_script_fixture_entries() {
    let resolved = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: PathBuf::from("/repo/tests/fixtures/script/pass/basic.srt"),
        selected_context: None,
        runner_selection: None,
        open_documents: Vec::new(),
    });

    assert_eq!(resolved.status, AnalysisContextStatus::Ready);
    assert_eq!(resolved.context.mode, AnalysisMode::Script);
    assert_eq!(
        resolved.context.entry_file,
        Some(PathBuf::from("/repo/tests/fixtures/script/pass/basic.srt"))
    );
    assert_eq!(resolved.context.source_kind, SourceKind::Script);
}

#[test]
fn resolve_context_builds_runner_context_from_selected_project_profile() {
    let resolved = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: PathBuf::from("/repo/src/user.srt"),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: PathBuf::from("/repo/project.srt"),
            profile: "test".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: PathBuf::from("/repo/project.srt"),
            selected_profile: "test".to_string(),
            normalized_args: vec![("env".to_string(), "ci".to_string())],
            source: None,
        }),
        open_documents: Vec::new(),
    });

    let runner = resolved
        .runner
        .expect("project context should include runner");
    assert_eq!(resolved.context.mode, AnalysisMode::Project);
    assert_eq!(
        resolved.context.entry_file,
        Some(PathBuf::from("/repo/project.srt"))
    );
    assert_eq!(runner.selected_profile, "test");
    assert_eq!(
        runner.normalized_args,
        vec![("env".to_string(), "ci".to_string())]
    );
}

#[test]
fn resolve_context_marks_selected_project_file_as_project_config_source() {
    let resolved = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: PathBuf::from("/repo/project.srt"),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: PathBuf::from("/repo/project.srt"),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: PathBuf::from("/repo/project.srt"),
            selected_profile: "dev".to_string(),
            normalized_args: Vec::new(),
            source: None,
        }),
        open_documents: Vec::new(),
    });

    assert_eq!(resolved.status, AnalysisContextStatus::Ready);
    assert_eq!(resolved.context.mode, AnalysisMode::Project);
    assert_eq!(
        resolved.context.source_kind,
        SourceKind::ProjectConfigSource
    );
}

#[test]
fn resolve_context_extracts_project_runner_source_when_available() {
    let resolved = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: PathBuf::from("/repo/src/user.srt"),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: PathBuf::from("/repo/project.srt"),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: PathBuf::from("/repo/project.srt"),
            selected_profile: "dev".to_string(),
            normalized_args: Vec::new(),
            source: Some(ProjectRunnerSourceInput {
                project_file: PathBuf::from("/repo/project.srt"),
                selected_profile: "dev".to_string(),
                normalized_args: Vec::new(),
                active_file: None,
                source: r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/user.srt")
  })
})
"#
                .to_string(),
            }),
        }),
        open_documents: Vec::new(),
    });

    let runner = resolved
        .runner
        .expect("project context should include runner");
    assert_eq!(resolved.status, AnalysisContextStatus::Ready);
    assert_eq!(runner.resolved_paths.len(), 1);
    assert_eq!(runner.resolved_paths[0].literal_or_glob, "./src/user.srt");
    assert_eq!(runner.active_file_profiles, vec!["dev"]);
}

#[test]
fn resolve_context_reports_project_profile_mismatch_without_guessing() {
    let resolved = resolve_context(AnalysisContextRequest {
        workspace_root: PathBuf::from("/repo"),
        active_file: PathBuf::from("/repo/src/user.srt"),
        selected_context: Some(SelectedContext::ProjectProfile {
            project_file: PathBuf::from("/repo/project.srt"),
            profile: "dev".to_string(),
        }),
        runner_selection: Some(RunnerSelection {
            project_file: PathBuf::from("/repo/project.srt"),
            selected_profile: "test".to_string(),
            normalized_args: Vec::new(),
            source: None,
        }),
        open_documents: Vec::new(),
    });

    assert_eq!(resolved.status, AnalysisContextStatus::NeedsSelection);
    assert!(resolved
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == ContextDiagnosticKind::ProjectProfileMismatch));
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
            source: None,
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
            source: None,
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
