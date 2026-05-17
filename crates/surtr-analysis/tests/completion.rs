use sigil::{DeclarationEntry, DeclarationIndex, DeclarationKind};
use sindr::ir::{DocEntry, DocKind, SignatureEntry};
use spire::ast::Visibility;
use surtr_analysis::{
    complete_prefix, CompletionKind, CompletionRequest, CompletionSymbol, SemanticIndex,
};

#[test]
fn completion_request_clamps_cursor_to_char_boundary_and_returns_byte_replacement_range() {
    let index = SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "print".to_string(),
        replacement: "print".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("print(a: String) -> Unit".to_string()),
    }]);
    let source = "値.pr";
    let cursor_inside_multibyte = 1;

    let completion = complete_prefix(CompletionRequest {
        index: &index,
        source,
        cursor: cursor_inside_multibyte,
    });

    assert!(completion.candidates.is_empty());
    assert_eq!(completion.replace_start, 0);
    assert_eq!(completion.replace_end, 0);
}

#[test]
fn completion_request_filters_symbols_by_token_prefix() {
    let index = SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "print".to_string(),
            replacement: "print".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("print(a: String) -> Unit".to_string()),
        },
        CompletionSymbol {
            label: "Process::sleep".to_string(),
            replacement: "Process::sleep".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: None,
        },
        CompletionSymbol {
            label: "String".to_string(),
            replacement: "String".to_string(),
            kind: CompletionKind::TypeConstructor,
            detail: None,
        },
    ]);

    let completion = complete_prefix(CompletionRequest {
        index: &index,
        source: "pri",
        cursor: 3,
    });

    assert_eq!(completion.replace_start, 0);
    assert_eq!(completion.replace_end, 3);
    assert_eq!(completion.candidates.len(), 1);
    assert_eq!(completion.candidates[0].label, "print");
}

#[test]
fn completion_request_matches_qualified_symbol_tail_for_unqualified_prefix() {
    let index = SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "Helper::helper".to_string(),
        replacement: "Helper::helper".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: None,
    }]);

    let completion = complete_prefix(CompletionRequest {
        index: &index,
        source: "he",
        cursor: 2,
    });

    assert_eq!(completion.candidates.len(), 1);
    assert_eq!(completion.candidates[0].label, "Helper::helper");
}

#[test]
fn semantic_index_adds_module_owner_symbols_from_declarations() {
    let mut declarations = DeclarationIndex::new();
    declarations.insert(
        "Helper::helper".to_string(),
        declaration_entry(
            "Helper",
            "helper",
            "Helper::helper",
            DeclarationKind::Def,
            true,
            true,
        ),
    );

    let index = SemanticIndex::from_declaration_index(&declarations);
    let completion = complete_prefix(CompletionRequest {
        index: &index,
        source: "He",
        cursor: 2,
    });
    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"Helper"), "labels: {labels:?}");
    assert!(labels.contains(&"Helper::helper"), "labels: {labels:?}");
}

#[test]
fn semantic_index_deduplicates_completion_symbols() {
    let index = SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "print".to_string(),
            replacement: "print".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: None,
        },
        CompletionSymbol {
            label: "print".to_string(),
            replacement: "print".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("duplicate".to_string()),
        },
    ]);

    assert_eq!(index.symbols().len(), 1);
}

fn declaration_entry(
    module_path: &str,
    name: &str,
    fq_name: &str,
    kind: DeclarationKind,
    user_importable: bool,
    user_callable: bool,
) -> DeclarationEntry {
    DeclarationEntry {
        module_path: module_path.to_string(),
        name: name.to_string(),
        fq_name: fq_name.to_string(),
        kind,
        stage_index: 0,
        auto_import: false,
        hidden: false,
        visibility: Visibility::Public,
        user_importable,
        user_callable,
    }
}

#[test]
fn semantic_index_builds_completion_symbols_from_doc_and_signature_metadata() {
    let docs = vec![
        DocEntry {
            qualified_name: "Global::print".to_string(),
            kind: DocKind::Function,
            module_path: "Global".to_string(),
            signature: Some("print(a: String) -> Unit".to_string()),
            doc: "Writes a line.".to_string(),
        },
        DocEntry {
            qualified_name: "Global::String".to_string(),
            kind: DocKind::Type,
            module_path: "Global".to_string(),
            signature: Some("String".to_string()),
            doc: "UTF-8 text.".to_string(),
        },
    ];
    let signatures = vec![SignatureEntry {
        qualified_name: "Global::print".to_string(),
        kind: DocKind::Function,
        module_path: "Global".to_string(),
        signature: "print(a: String) -> Unit".to_string(),
    }];

    let index = SemanticIndex::from_metadata(&docs, &signatures);
    let completion = complete_prefix(CompletionRequest {
        index: &index,
        source: "pri",
        cursor: 3,
    });

    assert_eq!(completion.candidates.len(), 1);
    assert_eq!(completion.candidates[0].label, "print");
    assert_eq!(completion.candidates[0].kind, CompletionKind::FunctionCall);
    assert_eq!(
        completion.candidates[0].detail.as_deref(),
        Some("print(a: String) -> Unit")
    );

    let type_completion = complete_prefix(CompletionRequest {
        index: &index,
        source: "Str",
        cursor: 3,
    });
    assert_eq!(type_completion.candidates.len(), 1);
    assert_eq!(type_completion.candidates[0].label, "String");
    assert_eq!(
        type_completion.candidates[0].kind,
        CompletionKind::TypeConstructor
    );
}

#[test]
fn completion_candidates_are_sorted_by_label_for_stable_lsp_output() {
    let index = SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "atom".to_string(),
            replacement: "atom".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: None,
        },
        CompletionSymbol {
            label: "alpha".to_string(),
            replacement: "alpha".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: None,
        },
    ]);

    let completion = complete_prefix(CompletionRequest {
        index: &index,
        source: "a",
        cursor: 1,
    });

    assert_eq!(completion.candidates[0].label, "alpha");
    assert_eq!(completion.candidates[1].label, "atom");
}
