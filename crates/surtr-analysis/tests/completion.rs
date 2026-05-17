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
