use std::path::PathBuf;

use surtr_analysis::{
    CompletionKind, CompletionSymbol, RunnerSelection, SelectedContext, SemanticIndex,
};
use surtr_lsp::{
    completion_items, diagnostics, file_uri_to_path, path_to_file_uri, CompletionItemKind,
    DiagnosticSeverity, LspAnalysisHost, LspPosition, LspRange,
};

#[test]
fn file_uri_roundtrip_decodes_percent_escaped_paths() {
    let path = PathBuf::from("/repo/dir with space/main.srt");
    let uri = path_to_file_uri(&path);

    assert_eq!(uri, "file:///repo/dir%20with%20space/main.srt");
    assert_eq!(file_uri_to_path(&uri), Some(path));
}

#[test]
fn diagnostics_maps_analysis_ranges_and_sources_to_lsp_dto() {
    let workspace = PathBuf::from("/repo");
    let path = workspace.join("main.srt");
    let uri = path_to_file_uri(&path);
    let mut host = LspAnalysisHost::new(workspace);
    host.did_open(uri.clone(), Some(1), "value: Int = \"bad\"".to_string());
    host.set_selected_context(Some(SelectedContext::ScriptEntry(path)));

    let diagnostics = diagnostics(&host, &uri);

    let typecheck = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.source == "surtr:typecheck")
        .expect("type mismatch should publish a typecheck diagnostic");
    assert_eq!(typecheck.severity, DiagnosticSeverity::Error);
    assert_eq!(typecheck.range.start.line, 0);
    assert!(typecheck.range.end.character > typecheck.range.start.character);
    assert!(typecheck.message.contains("expected Int"));
}

#[test]
fn completion_maps_utf16_position_to_lsp_text_edits() {
    let workspace = PathBuf::from("/repo");
    let path = workspace.join("main.srt");
    let uri = path_to_file_uri(&path);
    let mut host = LspAnalysisHost::new(workspace);
    host.did_open(uri.clone(), Some(1), "pri".to_string());
    host.set_selected_context(Some(SelectedContext::ScriptEntry(path)));
    host.set_semantic_index(SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "print".to_string(),
        replacement: "print".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("print(a: String) -> Unit".to_string()),
    }]));

    let items = completion_items(
        &host,
        &uri,
        LspPosition {
            line: 0,
            character: 3,
        },
    );

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "print");
    assert_eq!(items[0].kind, CompletionItemKind::Function);
    assert_eq!(items[0].detail.as_deref(), Some("print(a: String) -> Unit"));
    assert_eq!(
        items[0].text_edit.range,
        LspRange {
            start: LspPosition {
                line: 0,
                character: 0,
            },
            end: LspPosition {
                line: 0,
                character: 3,
            },
        }
    );
    assert_eq!(items[0].text_edit.new_text, "print");
}

#[test]
fn project_runner_diagnostics_are_published_as_lsp_diagnostics() {
    let workspace = std::env::temp_dir().join(format!(
        "surtr-lsp-project-runner-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock must be after unix epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).expect("temporary workspace must be writable");
    let project_file = workspace.join("project.srt");
    let uri = path_to_file_uri(&project_file);
    let mut host = LspAnalysisHost::new(workspace.clone());
    host.did_open(
        uri.clone(),
        Some(1),
        "Project::entrypoint(Config::new(), \"dev\", {|c| Config::add_path(c, \"missing*.srt\") })"
            .to_string(),
    );
    host.set_selected_context(Some(SelectedContext::ProjectProfile {
        project_file: project_file.clone(),
        profile: "dev".to_string(),
    }));
    host.set_runner_selection(Some(RunnerSelection {
        project_file: project_file.clone(),
        selected_profile: "dev".to_string(),
        normalized_args: vec![("profile".to_string(), "dev".to_string())],
        source: Some(surtr_analysis::ProjectRunnerSourceInput {
            project_file,
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            active_file: None,
            source: "Project::entrypoint(Config::new(), \"dev\", {|c| Config::add_path(c, \"missing*.srt\") })"
                .to_string(),
        }),
    }));

    let diagnostics = diagnostics(&host, &uri);

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source == "surtr:project-runner"
                && diagnostic.message.contains("did not match")),
        "project runner diagnostics should be mapped separately: {diagnostics:?}"
    );

    std::fs::remove_dir_all(workspace).expect("temporary workspace must be removable");
}
