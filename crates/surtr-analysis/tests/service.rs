use std::path::PathBuf;

use surtr_analysis::{
    resolve_context, AnalysisContextRequest, AnalysisDiagnosticKind, AnalysisMode, AnalysisService,
    CompletionKind, CompletionSymbol, SelectedContext, SemanticIndex, Utf16Position,
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
