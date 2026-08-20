use std::path::PathBuf;

use surtr_analysis::{
    CompletionKind, CompletionSymbol, FacetRootKind, ProjectBootSummary, ProjectRunnerPath,
    ProjectRunnerProfile, ProjectRunnerResult, RunnerSelection, SelectedContext, SemanticIndex,
    SymbolCapabilities,
};
use surtr_lsp::{
    completion_items, definition, diagnostics, document_symbols, file_uri_to_path, hover,
    path_to_file_uri, signature_help, CompletionItemKind, DiagnosticSeverity, LspAnalysisHost,
    LspPosition, LspRange,
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
fn diagnostics_publish_parse_ranges_from_tolerant_parse() {
    let workspace = PathBuf::from("/repo");
    let path = workspace.join("main.srt");
    let uri = path_to_file_uri(&path);
    let mut host = LspAnalysisHost::new(workspace);
    let source = "value = \"😀\" )";
    host.did_open(uri.clone(), Some(1), source.to_string());
    host.set_selected_context(Some(SelectedContext::ScriptEntry(path)));

    let diagnostics = diagnostics(&host, &uri);

    let parse = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.source == "surtr:parse")
        .expect("broken source should publish a parse diagnostic");
    assert_eq!(parse.severity, DiagnosticSeverity::Error);
    assert_eq!(parse.range.start.line, 0);
    let paren_byte = source.find(')').expect("broken token should exist");
    let expected_utf16_character = source[..paren_byte].encode_utf16().count() as u32;
    assert_eq!(parse.range.start.character, expected_utf16_character);
    assert!(parse.range.end.character > parse.range.start.character);
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
        documentation: None,
        sort_text: None,
        origin: None,
        capabilities: None,
        definition: None,
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
    assert_eq!(items[0].documentation.as_deref(), None);
    assert_eq!(items[0].sort_text.as_deref(), Some("1:print"));
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
fn completion_hides_bootstrap_module_but_keeps_other_modules_and_members() {
    let workspace = PathBuf::from("/repo");
    let path = workspace.join("main.srt");
    let uri = path_to_file_uri(&path);
    let mut host = LspAnalysisHost::new(workspace);
    host.did_open(uri.clone(), Some(1), "B".to_string());
    host.set_selected_context(Some(SelectedContext::ScriptEntry(path)));
    host.set_semantic_index(SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "Bootstrap".to_string(),
            replacement: "Bootstrap".to_string(),
            kind: CompletionKind::TypePath,
            detail: None,
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
        CompletionSymbol {
            label: "Kernel".to_string(),
            replacement: "Kernel".to_string(),
            kind: CompletionKind::TypePath,
            detail: None,
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
        CompletionSymbol {
            label: "Bootstrap::helper".to_string(),
            replacement: "Bootstrap::helper".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: None,
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
    ]));

    let bootstrap_labels = completion_items(
        &host,
        &uri,
        LspPosition {
            line: 0,
            character: 1,
        },
    )
    .into_iter()
    .map(|item| item.label)
    .collect::<Vec<_>>();

    assert!(
        !bootstrap_labels.iter().any(|label| label == "Bootstrap"),
        "{bootstrap_labels:?}"
    );

    host.did_change(&uri, Some(2), "K".to_string());
    let kernel_labels = completion_items(
        &host,
        &uri,
        LspPosition {
            line: 0,
            character: 1,
        },
    )
    .into_iter()
    .map(|item| item.label)
    .collect::<Vec<_>>();
    assert!(
        kernel_labels.iter().any(|label| label == "Kernel"),
        "{kernel_labels:?}"
    );

    host.did_change(&uri, Some(3), "Bootstrap::h".to_string());
    let member_labels = completion_items(
        &host,
        &uri,
        LspPosition {
            line: 0,
            character: "Bootstrap::h".len() as u32,
        },
    )
    .into_iter()
    .map(|item| item.label)
    .collect::<Vec<_>>();
    assert!(
        member_labels
            .iter()
            .any(|label| label == "Bootstrap::helper"),
        "{member_labels:?}"
    );
}

#[test]
fn completion_uses_facet_api_first_argument_constraints_through_lsp() {
    let workspace = PathBuf::from("/repo");
    let path = workspace.join("main.srt");
    let uri = path_to_file_uri(&path);
    let mut host = LspAnalysisHost::new(workspace);
    host.did_open(uri.clone(), Some(1), "Facet::view(".to_string());
    host.set_selected_context(Some(SelectedContext::ScriptEntry(path)));
    host.set_semantic_index(SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "Facet::view".to_string(),
            replacement: "Facet::view".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some(
                "view(facet: Facet<ReadablePath, $S, $A, _, _>, source: $S) -> Result<$A>"
                    .to_string(),
            ),
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
        CompletionSymbol {
            label: "User".to_string(),
            replacement: "User".to_string(),
            kind: CompletionKind::TypeConstructor,
            detail: Some("defrecord User".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: Some(SymbolCapabilities::new(
                true,
                true,
                true,
                Some(FacetRootKind::TypeRoot),
            )),
            definition: None,
        },
        CompletionSymbol {
            label: "Int".to_string(),
            replacement: "Int".to_string(),
            kind: CompletionKind::TypeConstructor,
            detail: Some("type Int".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
        CompletionSymbol {
            label: "name_path".to_string(),
            replacement: "name_path".to_string(),
            kind: CompletionKind::Variable,
            detail: Some("Facet<InfallibleStructural, User, String, _, _>".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
    ]));

    let items = completion_items(
        &host,
        &uri,
        LspPosition {
            line: 0,
            character: "Facet::view(".len() as u32,
        },
    );
    let labels = items
        .iter()
        .map(|item| item.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"User"), "{labels:?}");
    assert!(labels.contains(&"name_path"), "{labels:?}");
    assert!(!labels.contains(&"Int"), "{labels:?}");
}

#[test]
fn completion_preserves_contextual_sort_text_through_lsp() {
    let workspace = PathBuf::from("/repo");
    let path = workspace.join("main.srt");
    let uri = path_to_file_uri(&path);
    let mut host = LspAnalysisHost::new(workspace);
    host.did_open(uri.clone(), Some(1), "print(value_".to_string());
    host.set_selected_context(Some(SelectedContext::ScriptEntry(path)));
    host.set_semantic_index(SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "print".to_string(),
            replacement: "print".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("print(value: String) -> Unit".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
        CompletionSymbol {
            label: "value_text".to_string(),
            replacement: "value_text".to_string(),
            kind: CompletionKind::Variable,
            detail: Some("String".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
        CompletionSymbol {
            label: "value_count".to_string(),
            replacement: "value_count".to_string(),
            kind: CompletionKind::Variable,
            detail: Some("Int".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
    ]));

    let items = completion_items(
        &host,
        &uri,
        LspPosition {
            line: 0,
            character: "print(value_".len() as u32,
        },
    );

    assert_eq!(
        items
            .iter()
            .map(|item| (item.label.as_str(), item.sort_text.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("value_text", Some("0000:value_text")),
            ("value_count", Some("0001:value_count")),
        ]
    );
}

#[test]
fn completion_exposes_shared_result_ctors_and_bool_variants_through_lsp() {
    let workspace = PathBuf::from("/repo");
    let path = workspace.join("main.srt");
    let uri = path_to_file_uri(&path);
    let mut host = LspAnalysisHost::new(workspace);
    host.did_open(uri.clone(), Some(1), "Ok".to_string());
    host.set_selected_context(Some(SelectedContext::ScriptEntry(path.clone())));
    host.set_semantic_index(SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "Result::Ok".to_string(),
            replacement: "Result::Ok".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("Result::Ok($T) -> Result<$T, Error>".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
        CompletionSymbol {
            label: "Ok".to_string(),
            replacement: "Ok".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("Result::Ok($T) -> Result<$T, Error>".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
        CompletionSymbol {
            label: "Boolean::True".to_string(),
            replacement: "Boolean::True".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("Boolean::True() -> Boolean".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
        CompletionSymbol {
            label: "True".to_string(),
            replacement: "True".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("Boolean::True() -> Boolean".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            capabilities: None,
            definition: None,
        },
    ]));

    let ok_items = completion_items(
        &host,
        &uri,
        LspPosition {
            line: 0,
            character: 2,
        },
    );
    assert!(ok_items.iter().any(|item| item.label == "Ok"));
    assert!(ok_items.iter().any(|item| {
        item.label == "Ok"
            && item
                .detail
                .as_deref()
                .is_some_and(|detail| detail == "Result::Ok($T) -> Result<$T, Error>")
    }));

    host.did_change(&uri, Some(2), "Tr".to_string());
    let true_items = completion_items(
        &host,
        &uri,
        LspPosition {
            line: 0,
            character: 2,
        },
    );
    assert!(true_items.iter().any(|item| item.label == "True"));
    assert!(true_items.iter().any(|item| {
        item.label == "True"
            && item
                .detail
                .as_deref()
                .is_some_and(|detail| detail == "Boolean::True() -> Boolean")
    }));
    assert!(
        !true_items.iter().any(|item| item.label == "true"),
        "lowercase REPL shorthand must not leak into LSP: {true_items:?}"
    );
}

#[test]
fn hover_maps_semantic_detail_and_documentation_to_lsp_dto() {
    let workspace = PathBuf::from("/repo");
    let path = workspace.join("main.srt");
    let uri = path_to_file_uri(&path);
    let mut host = LspAnalysisHost::new(workspace);
    host.did_open(uri.clone(), Some(1), "value = print".to_string());
    host.set_selected_context(Some(SelectedContext::ScriptEntry(path)));
    host.set_semantic_index(SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "print".to_string(),
        replacement: "print".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("print(a: String) -> Unit".to_string()),
        documentation: Some("Writes a line.".to_string()),
        sort_text: None,
        origin: None,
        capabilities: None,
        definition: None,
    }]));

    let hover = hover(
        &host,
        &uri,
        LspPosition {
            line: 0,
            character: "value = pr".len() as u32,
        },
    )
    .expect("semantic hover should be available");

    assert_eq!(hover.contents, "print(a: String) -> Unit\n\nWrites a line.");
    assert_eq!(
        hover.range,
        Some(LspRange {
            start: LspPosition {
                line: 0,
                character: "value = ".len() as u32,
            },
            end: LspPosition {
                line: 0,
                character: "value = print".len() as u32,
            },
        })
    );
}

#[test]
fn signature_help_maps_semantic_call_context_to_lsp_dto() {
    let workspace = PathBuf::from("/repo");
    let path = workspace.join("main.srt");
    let uri = path_to_file_uri(&path);
    let source = "value = print(\"hello\", Tr";
    let mut host = LspAnalysisHost::new(workspace);
    host.did_open(uri.clone(), Some(1), source.to_string());
    host.set_selected_context(Some(SelectedContext::ScriptEntry(path)));
    host.set_semantic_index(SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "print".to_string(),
        replacement: "print".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("print(value: String, newline: Bool) -> Unit".to_string()),
        documentation: Some("Writes a line.".to_string()),
        sort_text: None,
        origin: None,
        capabilities: None,
        definition: None,
    }]));

    let help = signature_help(
        &host,
        &uri,
        LspPosition {
            line: 0,
            character: source.len() as u32,
        },
    )
    .expect("semantic signature help should be available");

    assert_eq!(
        help.signatures,
        vec!["print(value: String, newline: Bool) -> Unit".to_string()]
    );
    assert_eq!(help.active_signature, Some(0));
    assert_eq!(help.active_parameter, Some(1));
}

#[test]
fn definition_maps_analysis_location_to_lsp_dto() {
    let workspace = PathBuf::from("/repo");
    let path = workspace.join("main.srt");
    let uri = path_to_file_uri(&path);
    let source = "def helper() -> Int { 1 }\nvalue = helper()";
    let mut host = LspAnalysisHost::new(workspace);
    host.did_open(uri.clone(), Some(1), source.to_string());
    host.set_selected_context(Some(SelectedContext::ScriptEntry(path)));

    let locations = definition(
        &host,
        &uri,
        LspPosition {
            line: 1,
            character: "value = help".len() as u32,
        },
    );

    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].uri, uri);
    assert_eq!(
        locations[0].range.start,
        LspPosition {
            line: 0,
            character: 0,
        }
    );
    assert!(locations[0].range.end.character > locations[0].range.start.character);
}

#[test]
fn completion_uses_project_stage_declarations_through_lsp_host() {
    let workspace = temp_workspace("project-completion");
    let src = workspace.join("src");
    std::fs::create_dir_all(&src).expect("temporary src dir must be writable");
    let helper_path = src.join("helper.srt");
    let main_path = src.join("main.srt");
    let project_file = workspace.join("project.srt");
    std::fs::write(&helper_path, "defmod Helper { def helper() -> Int { 1 } }")
        .expect("write helper source");

    let uri = path_to_file_uri(&main_path);
    let source = "defmod Main { def main() -> Int { Helper::he } }";
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/helper.srt")
    |> Config::add_path("./src/main.srt")
  })
})
"#;
    let mut host = LspAnalysisHost::new(workspace.clone());
    host.did_open(uri.clone(), Some(1), source.to_string());
    host.set_selected_context(Some(SelectedContext::ProjectProfile {
        project_file: project_file.clone(),
        profile: "dev".to_string(),
    }));
    host.set_runner_selection(Some(RunnerSelection {
        project_file: project_file.clone(),
        selected_profile: "dev".to_string(),
        normalized_args: vec![("profile".to_string(), "dev".to_string())],
        runner_result: None,
        source: Some(surtr_analysis::ProjectRunnerSourceInput {
            project_file,
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            active_file: Some(main_path),
            source: project_source.to_string(),
        }),
    }));

    let items = completion_items(
        &host,
        &uri,
        LspPosition {
            line: 0,
            character: source
                .find("Helper::he }")
                .expect("completion token exists") as u32
                + "Helper::he".len() as u32,
        },
    );

    assert!(
        items.iter().any(|item| item.label == "Helper::helper"
            && item.kind == CompletionItemKind::Function
            && item.text_edit.new_text == "Helper::helper"),
        "project declarations should flow through LSP completion: {items:?}"
    );

    std::fs::remove_dir_all(workspace).expect("temporary workspace must be removable");
}

#[test]
fn completion_uses_injected_project_runner_executor() {
    let workspace = temp_workspace("project-completion-vm");
    let src = workspace.join("src");
    std::fs::create_dir_all(&src).expect("temporary src dir must be writable");
    let helper_path = src.join("helper.srt");
    let main_path = src.join("main.srt");
    let project_file = workspace.join("project.srt");
    std::fs::write(&helper_path, "defmod Helper { def helper() -> Int { 1 } }")
        .expect("write helper source");

    let uri = path_to_file_uri(&main_path);
    let source = "Helper::he";
    let mut host = LspAnalysisHost::new(workspace.clone());
    let helper_path_for_executor = helper_path.clone();
    host.set_project_runner_executor(Some(
        move |input: surtr_analysis::ProjectRunnerSourceInput| {
            assert_eq!(input.selected_profile, "dev");
            Ok(ProjectRunnerResult {
                profiles: vec![ProjectRunnerProfile {
                    name: "dev".to_string(),
                    entrypoint: "Main::main".to_string(),
                    paths: vec![ProjectRunnerPath {
                        declared_by: input.project_file,
                        literal_or_glob: helper_path_for_executor.to_string_lossy().into_owned(),
                        declaration_span: None,
                    }],
                }],
                boot_summary: ProjectBootSummary::default(),
                external_inputs: Vec::new(),
            })
        },
    ));
    host.did_open(uri.clone(), Some(1), source.to_string());
    host.set_selected_context(Some(SelectedContext::ProjectProfile {
        project_file: project_file.clone(),
        profile: "dev".to_string(),
    }));
    host.set_runner_selection(Some(RunnerSelection {
        project_file: project_file.clone(),
        selected_profile: "dev".to_string(),
        normalized_args: vec![("profile".to_string(), "dev".to_string())],
        runner_result: None,
        source: Some(surtr_analysis::ProjectRunnerSourceInput {
            project_file,
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            active_file: Some(main_path.clone()),
            source: "computed project source".to_string(),
        }),
    }));

    let items = completion_items(
        &host,
        &uri,
        LspPosition {
            line: 0,
            character: "Helper::he".len() as u32,
        },
    );

    assert!(
        items.iter().any(|item| item.label == "Helper::helper"
            && item.kind == CompletionItemKind::Function),
        "VM-executed project runner result should flow through LSP completion: {items:?}"
    );

    std::fs::remove_dir_all(workspace).expect("temporary workspace must be removable");
}

#[test]
fn completion_uses_load_project_context_for_operational_script() {
    let workspace = temp_workspace("load-project-completion");
    let src = workspace.join("src");
    let scripts = workspace.join("scripts");
    std::fs::create_dir_all(&src).expect("temporary src dir must be writable");
    std::fs::create_dir_all(&scripts).expect("temporary scripts dir must be writable");
    let helper_path = src.join("helper.srt");
    let script_path = scripts.join("seed.srt");
    let project_file = workspace.join("project.srt");
    std::fs::write(&helper_path, "defmod Helper { def helper() -> Int { 1 } }")
        .expect("write helper source");
    std::fs::write(
        &project_file,
        r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/helper.srt")
  })
})
"#,
    )
    .expect("write project source");

    let uri = path_to_file_uri(&script_path);
    let source = r#"load_project("../project.srt", profile: "dev")

Helper::he
"#;
    let mut host = LspAnalysisHost::new(workspace.clone());
    host.did_open(uri.clone(), Some(1), source.to_string());
    host.set_selected_context(Some(SelectedContext::ScriptEntry(script_path)));

    let items = completion_items(
        &host,
        &uri,
        LspPosition {
            line: 2,
            character: "Helper::he".len() as u32,
        },
    );

    assert!(
        items.iter().any(|item| item.label == "Helper::helper"
            && item.kind == CompletionItemKind::Function),
        "load_project project declarations should flow through LSP completion: {items:?}"
    );

    std::fs::remove_dir_all(workspace).expect("temporary workspace must be removable");
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
        runner_result: None,
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

#[test]
fn document_symbols_map_analysis_ranges_to_lsp_dto() {
    let workspace = temp_workspace("document-symbols");
    let src = workspace.join("src");
    std::fs::create_dir_all(&src).expect("temporary src dir must be writable");
    let path = src.join("main.srt");
    let project_file = workspace.join("project.srt");
    let uri = path_to_file_uri(&path);
    let source = "defmod Main { def helper() -> Int { 1 } }";
    let project_source = r#"
Project::config({|config|
  Project::entrypoint(config, "dev", {|c|
    Config::add_path(c, "./src/main.srt")
  })
})
"#;

    let mut host = LspAnalysisHost::new(workspace.clone());
    host.did_open(uri.clone(), Some(1), source.to_string());
    host.set_selected_context(Some(SelectedContext::ProjectProfile {
        project_file: project_file.clone(),
        profile: "dev".to_string(),
    }));
    host.set_runner_selection(Some(RunnerSelection {
        project_file: project_file.clone(),
        selected_profile: "dev".to_string(),
        normalized_args: vec![("profile".to_string(), "dev".to_string())],
        runner_result: None,
        source: Some(surtr_analysis::ProjectRunnerSourceInput {
            project_file,
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            active_file: Some(path),
            source: project_source.to_string(),
        }),
    }));

    let symbols = document_symbols(&host, &uri);

    assert!(symbols.iter().any(|symbol| symbol.name == "Main"));
    assert!(symbols.iter().any(|symbol| symbol.name == "Main::helper"));
    assert!(symbols
        .iter()
        .all(|symbol| symbol.range.end.character > symbol.range.start.character));

    std::fs::remove_dir_all(workspace).expect("temporary workspace must be removable");
}

#[test]
fn document_symbols_use_tolerant_outline_when_parse_fails() {
    let workspace = PathBuf::from("/repo");
    let path = workspace.join("main.srt");
    let uri = path_to_file_uri(&path);
    let source = "def ok() -> Int { 1 }\nbroken = )\ndef next() -> Int { 2 }";
    let mut host = LspAnalysisHost::new(workspace);
    host.did_open(uri.clone(), Some(1), source.to_string());
    host.set_selected_context(Some(SelectedContext::ScriptEntry(path)));

    let diagnostics = diagnostics(&host, &uri);
    let symbols = document_symbols(&host, &uri);

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source == "surtr:parse"),
        "broken source should still publish parse diagnostics: {diagnostics:?}"
    );
    assert!(
        symbols.iter().any(|symbol| symbol.name == "ok")
            && symbols.iter().any(|symbol| symbol.name == "next"),
        "tolerant outline should keep declaration symbols after parse errors: {symbols:?}"
    );
    assert!(symbols
        .iter()
        .all(|symbol| symbol.range.end.character > symbol.range.start.character));
}

fn temp_workspace(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "surtr-lsp-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock must be after unix epoch")
            .as_nanos()
    ))
}
