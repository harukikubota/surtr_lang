use sigil::{
    DeclarationEntry, DeclarationIndex, DeclarationKind, OwnerEntry, OwnerKind, OwnerRegistry,
};
use sindr::ir::{DocEntry, DocKind, SignatureEntry};
use sindr::names::{FacetRootKind, SymbolCapabilities, TypeIdentity};
use spire::ast::{Span, Visibility};
use surtr_analysis::{
    complete_call_argument, complete_prefix, lookup_symbol_at_cursor,
    rank_completion_candidates_by_expected_type, repl_assist_at_cursor, signature_help_at_cursor,
    CallableSignature, CompletionCandidate, CompletionKind, CompletionOrigin, CompletionRequest,
    CompletionScope, CompletionSymbol, ReplCommandUseSite, ReplCompletionUseSite,
    ReplInputSupportContext, ReplInputSupportUpdate, SemanticIndex, SymbolDisplayMetadata,
};

#[test]
fn completion_request_clamps_cursor_to_char_boundary_and_returns_byte_replacement_range() {
    let index = SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "print".to_string(),
        replacement: "print".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("print(a: String) -> Unit".to_string()),
        documentation: None,
        sort_text: None,
        origin: None,

        definition: None,

        capabilities: None,
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
            documentation: None,
            sort_text: None,
            origin: None,

            definition: None,

            capabilities: None,
        },
        CompletionSymbol {
            label: "Process::sleep".to_string(),
            replacement: "Process::sleep".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: None,
            documentation: None,
            sort_text: None,
            origin: None,

            definition: None,

            capabilities: None,
        },
        CompletionSymbol {
            label: "String".to_string(),
            replacement: "String".to_string(),
            kind: CompletionKind::TypeConstructor,
            detail: None,
            documentation: None,
            sort_text: None,
            origin: None,

            definition: None,

            capabilities: None,
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
fn completion_candidates_preserve_symbol_capabilities() {
    let capabilities = SymbolCapabilities::new(true, true, true, Some(FacetRootKind::TypeRoot));
    let index = SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "User".to_string(),
        replacement: "User".to_string(),
        kind: CompletionKind::TypeConstructor,
        detail: None,
        documentation: None,
        sort_text: None,
        origin: None,
        definition: None,
        capabilities: Some(capabilities),
    }]);

    let completion = complete_prefix(CompletionRequest {
        index: &index,
        source: "Us",
        cursor: 2,
    });

    assert_eq!(
        completion.candidates[0]
            .capabilities
            .as_ref()
            .and_then(|capabilities| capabilities.facet_root_path),
        Some(FacetRootKind::TypeRoot)
    );
}

#[test]
fn rank_completion_candidates_by_expected_type_keeps_nonmatching_candidates_after_matches() {
    let candidates = vec![
        completion_candidate("text", "String"),
        completion_candidate("count", "Int"),
        completion_candidate("other", "String"),
    ];

    let ranked =
        rank_completion_candidates_by_expected_type(candidates, Some("Int"), |expected, actual| {
            expected == actual
        });

    assert_eq!(
        ranked
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["count", "text", "other"]
    );
}

#[test]
fn rank_completion_candidates_by_expected_type_preserves_order_without_expected_type() {
    let candidates = vec![
        completion_candidate("text", "String"),
        completion_candidate("count", "Int"),
    ];

    let ranked =
        rank_completion_candidates_by_expected_type(candidates, None, |expected, actual| {
            expected == actual
        });

    assert_eq!(
        ranked
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["text", "count"]
    );
}

#[test]
fn completion_request_matches_qualified_symbol_tail_for_unqualified_prefix() {
    let index = SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "Helper::helper".to_string(),
        replacement: "Helper::helper".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: None,
        documentation: None,
        sort_text: None,
        origin: None,

        definition: None,

        capabilities: None,
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
fn repl_completion_requires_unqualified_symbols_for_unqualified_prefix() {
    let index = SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "String::repeat".to_string(),
        replacement: "String::repeat".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("String::repeat(self: String, times: Int) -> String".to_string()),
        documentation: Some("Repeat text.".to_string()),
        sort_text: None,
        origin: None,
        definition: None,
        capabilities: None,
    }]);

    let unqualified = surtr_analysis::complete_repl_prefix(
        CompletionRequest {
            index: &index,
            source: "re",
            cursor: 2,
        },
        CompletionScope::All,
    );

    assert!(
        unqualified.candidates.is_empty(),
        "qualified-only symbols must not leak into unqualified REPL completion: {:?}",
        unqualified.candidates
    );

    let index = SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "String::repeat".to_string(),
            replacement: "String::repeat".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("String::repeat(self: String, times: Int) -> String".to_string()),
            documentation: Some("Repeat text.".to_string()),
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "repeat".to_string(),
            replacement: "repeat".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("String::repeat(self: String, times: Int) -> String".to_string()),
            documentation: Some("Repeat text.".to_string()),
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
    ]);

    let unqualified = surtr_analysis::complete_repl_prefix(
        CompletionRequest {
            index: &index,
            source: "re",
            cursor: 2,
        },
        CompletionScope::All,
    );

    assert_eq!(unqualified.candidates.len(), 1);
    assert_eq!(unqualified.candidates[0].label, "repeat");
    assert_eq!(unqualified.candidates[0].replacement, "repeat");
    assert_eq!(unqualified.candidates[0].kind, CompletionKind::FunctionCall);
    assert_eq!(
        unqualified.candidates[0].documentation.as_deref(),
        Some("Repeat text.")
    );

    let qualified = surtr_analysis::complete_repl_prefix(
        CompletionRequest {
            index: &index,
            source: "String::re",
            cursor: "String::re".len(),
        },
        CompletionScope::All,
    );

    assert_eq!(qualified.candidates.len(), 1);
    assert_eq!(qualified.candidates[0].label, "String::repeat");
    assert_eq!(qualified.candidates[0].replacement, "String::repeat");
    assert_eq!(qualified.candidates[0].kind, CompletionKind::TypePath);
}

#[test]
fn repl_completion_prefers_type_owners_before_members_for_pascal_case_prefix() {
    let index = SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "Int".to_string(),
            replacement: "Int".to_string(),
            kind: CompletionKind::TypeConstructor,
            detail: Some("type Int".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "IntBase".to_string(),
            replacement: "IntBase".to_string(),
            kind: CompletionKind::TypeConstructor,
            detail: Some("defenum IntBase".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "IntBase::label".to_string(),
            replacement: "IntBase::label".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("IntBase::label(self: IntBase) -> String".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
    ]);

    let completion = surtr_analysis::complete_repl_prefix(
        CompletionRequest {
            index: &index,
            source: "Int",
            cursor: 3,
        },
        CompletionScope::All,
    );

    assert_eq!(
        completion
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Int", "IntBase", "IntBase::label"]
    );
}

#[test]
fn repl_completion_hides_qualified_enum_variants_until_owner_path_is_confirmed() {
    let index = SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "IntBase".to_string(),
            replacement: "IntBase".to_string(),
            kind: CompletionKind::TypeConstructor,
            detail: Some("defenum IntBase".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "IntBase::Bin".to_string(),
            replacement: "IntBase::Bin".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("IntBase::Bin -> IntBase".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "IntBase::Dec".to_string(),
            replacement: "IntBase::Dec".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("IntBase::Dec -> IntBase".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
    ]);

    let bare = surtr_analysis::complete_repl_prefix(
        CompletionRequest {
            index: &index,
            source: "IntB",
            cursor: 4,
        },
        CompletionScope::All,
    );
    assert_eq!(
        bare.candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["IntBase"]
    );

    let qualified = surtr_analysis::complete_repl_prefix(
        CompletionRequest {
            index: &index,
            source: "IntBase::",
            cursor: "IntBase::".len(),
        },
        CompletionScope::All,
    );
    assert_eq!(
        qualified
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["IntBase::Bin", "IntBase::Dec"]
    );
}

#[test]
fn completion_request_injects_shared_result_ctors_and_bool_variants() {
    let index = SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "Result::Ok".to_string(),
            replacement: "Result::Ok".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("Result::Ok($T) -> Result<$T, Error>".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "Ok".to_string(),
            replacement: "Ok".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("Result::Ok($T) -> Result<$T, Error>".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "Result::Err".to_string(),
            replacement: "Result::Err".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("Result::Err(Error) -> Result<$T, Error>".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "Err".to_string(),
            replacement: "Err".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("Result::Err(Error) -> Result<$T, Error>".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "Boolean::True".to_string(),
            replacement: "Boolean::True".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("Boolean::True() -> Boolean".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "True".to_string(),
            replacement: "True".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("Boolean::True() -> Boolean".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "Boolean::False".to_string(),
            replacement: "Boolean::False".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("Boolean::False() -> Boolean".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "False".to_string(),
            replacement: "False".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("Boolean::False() -> Boolean".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
    ]);

    let ok = complete_prefix(CompletionRequest {
        index: &index,
        source: "Ok",
        cursor: 2,
    });
    assert_eq!(
        ok.candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Ok"]
    );
    assert_eq!(ok.candidates[0].kind, CompletionKind::FunctionCall);
    assert_eq!(ok.candidates[0].replacement, "Ok");
    assert_eq!(
        ok.candidates[0].detail.as_deref(),
        Some("Result::Ok($T) -> Result<$T, Error>")
    );

    let err = complete_prefix(CompletionRequest {
        index: &index,
        source: "Err",
        cursor: 3,
    });
    assert_eq!(
        err.candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Err"]
    );
    assert_eq!(
        err.candidates[0].detail.as_deref(),
        Some("Result::Err(Error) -> Result<$T, Error>")
    );

    let true_completion = complete_prefix(CompletionRequest {
        index: &index,
        source: "Tr",
        cursor: 2,
    });
    assert_eq!(
        true_completion
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["True"]
    );
    assert_eq!(true_completion.candidates[0].replacement, "True");
    assert_eq!(
        true_completion.candidates[0].detail.as_deref(),
        Some("Boolean::True() -> Boolean")
    );

    let false_completion = complete_prefix(CompletionRequest {
        index: &index,
        source: "Fal",
        cursor: 3,
    });
    assert_eq!(
        false_completion
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["False"]
    );
    assert_eq!(false_completion.candidates[0].replacement, "False");
    assert_eq!(
        false_completion.candidates[0].detail.as_deref(),
        Some("Boolean::False() -> Boolean")
    );
}

#[test]
fn repl_completion_scope_can_limit_candidates_to_variables() {
    let index = SemanticIndex::from_symbols(vec![
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
        CompletionSymbol {
            label: "normalize".to_string(),
            replacement: "normalize".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("normalize(value: String) -> String".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
    ]);

    let completion = surtr_analysis::complete_repl_prefix(
        CompletionRequest {
            index: &index,
            source: "n",
            cursor: 1,
        },
        CompletionScope::VariablesOnly,
    );

    assert_eq!(
        completion
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["name"]
    );
}

#[test]
fn repl_variable_scope_allows_empty_prefix_for_call_argument_completion() {
    let index = SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "name".to_string(),
        replacement: "name".to_string(),
        kind: CompletionKind::Variable,
        detail: Some("String".to_string()),
        documentation: None,
        sort_text: None,
        origin: None,
        definition: None,
        capabilities: None,
    }]);

    let completion = surtr_analysis::complete_repl_prefix(
        CompletionRequest {
            index: &index,
            source: "print(",
            cursor: "print(".len(),
        },
        CompletionScope::VariablesOnly,
    );

    assert_eq!(completion.candidates.len(), 1);
    assert_eq!(completion.candidates[0].label, "name");
    assert_eq!(completion.replace_start, "print(".len());
    assert_eq!(completion.replace_end, "print(".len());
}

#[test]
fn semantic_index_finds_symbol_at_cursor_for_shared_hover_and_completion_detail() {
    let index = SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "Helper::helper".to_string(),
        replacement: "Helper::helper".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("helper() -> Int".to_string()),
        documentation: Some("Returns the helper value.".to_string()),
        sort_text: None,
        origin: None,

        definition: None,

        capabilities: None,
    }]);

    let lookup = lookup_symbol_at_cursor(&index, "value = helper()", "value = hel".len())
        .expect("unique qualified tail should resolve from token under cursor");

    assert_eq!(lookup.start, "value = ".len());
    assert_eq!(lookup.end, "value = helper".len());
    assert_eq!(lookup.symbol.label, "Helper::helper");
    assert_eq!(lookup.symbol.detail.as_deref(), Some("helper() -> Int"));
    assert_eq!(
        lookup.symbol.documentation.as_deref(),
        Some("Returns the helper value.")
    );
}

#[test]
fn semantic_index_returns_signature_help_from_call_context() {
    let index = SemanticIndex::from_symbols(vec![CompletionSymbol {
        label: "print".to_string(),
        replacement: "print".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: Some("print(value: String, newline: Bool) -> Unit".to_string()),
        documentation: Some("Writes a line.".to_string()),
        sort_text: None,
        origin: None,

        definition: None,

        capabilities: None,
    }]);

    let help = signature_help_at_cursor(&index, "print(\"hello\", Tr", "print(\"hello\", Tr".len())
        .expect("call context should resolve signature help");

    assert_eq!(
        help.signature,
        "print(value: String, newline: Bool) -> Unit"
    );
    assert_eq!(help.active_parameter, 1);
    assert_eq!(help.callee_start, 0);
    assert_eq!(help.callee_end, "print".len());
}

#[test]
fn repl_assist_combines_call_signature_and_argument_variable_completion() {
    let index = SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "print".to_string(),
            replacement: "print".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("print(value: String, newline: Bool) -> Unit".to_string()),
            documentation: Some("Writes a line.".to_string()),
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
        CompletionSymbol {
            label: "normalize".to_string(),
            replacement: "normalize".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("normalize(value: String) -> String".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
    ]);

    let assist = repl_assist_at_cursor(
        CompletionRequest {
            index: &index,
            source: "print(",
            cursor: "print(".len(),
        },
        ReplCompletionUseSite::Input,
    );

    assert_eq!(assist.active_parameter, Some(0));
    assert_eq!(
        assist
            .signature
            .as_ref()
            .map(|signature| signature.signature.as_str()),
        Some("print(value: String, newline: Bool) -> Unit")
    );
    assert_eq!(
        assist
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["name"]
    );
    assert_eq!(assist.replace_start, "print(".len());
    assert_eq!(assist.replace_end, "print(".len());
}

#[test]
fn repl_assist_preserves_repl_tail_presentation() {
    let index = SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "String::repeat".to_string(),
            replacement: "String::repeat".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("String::repeat(self: String, times: Int) -> String".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
        CompletionSymbol {
            label: "repeat".to_string(),
            replacement: "repeat".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("String::repeat(self: String, times: Int) -> String".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        },
    ]);

    let assist = repl_assist_at_cursor(
        CompletionRequest {
            index: &index,
            source: "re",
            cursor: "re".len(),
        },
        ReplCompletionUseSite::Input,
    );

    assert_eq!(assist.signature, None);
    assert_eq!(assist.candidates.len(), 1);
    assert_eq!(assist.candidates[0].label, "repeat");
    assert_eq!(assist.candidates[0].replacement, "repeat");
}

#[test]
fn repl_input_support_context_accepts_session_updates() {
    let mut context = ReplInputSupportContext::default();
    context.apply_update(ReplInputSupportUpdate {
        symbols: vec![CompletionSymbol {
            label: "value".to_string(),
            replacement: "value".to_string(),
            kind: CompletionKind::Variable,
            detail: Some("Int".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        }],
        callable_signatures: vec![CallableSignature {
            label: "fresh".to_string(),
            qualified_name: "Fresh::fresh".to_string(),
            signature: "fresh(value: String) -> Unit".to_string(),
        }],
    });

    let completion = context.input_support("val", 3, ReplCompletionUseSite::Input);
    assert_eq!(completion.candidates.len(), 1);
    assert_eq!(completion.candidates[0].label, "value");
    assert_eq!(completion.candidates[0].detail.as_deref(), Some("Int"));

    let support = context.input_support("fresh(", "fresh(".len(), ReplCompletionUseSite::Input);
    let signature = support
        .signature
        .expect("call signature should be produced by input support core");
    assert_eq!(
        signature.lines,
        vec!["Fresh::fresh(value: [String]) -> Unit".to_string()]
    );
    assert_eq!(signature.active_parameter, Some(0));
}

#[test]
fn repl_input_support_context_filters_type_command_candidates_by_use_site() {
    let context = ReplInputSupportContext::from_update(ReplInputSupportUpdate {
        symbols: vec![
            CompletionSymbol {
                label: "count".to_string(),
                replacement: "count".to_string(),
                kind: CompletionKind::Variable,
                detail: Some("Int".to_string()),
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities: None,
            },
            CompletionSymbol {
                label: "count_up".to_string(),
                replacement: "count_up".to_string(),
                kind: CompletionKind::FunctionCall,
                detail: Some("count_up(value: Int) -> Int".to_string()),
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities: None,
            },
            CompletionSymbol {
                label: "Counter".to_string(),
                replacement: "Counter".to_string(),
                kind: CompletionKind::TypeConstructor,
                detail: Some("Counter".to_string()),
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities: None,
            },
        ],
        callable_signatures: Vec::new(),
    });

    let input_labels = context
        .input_support("co", 2, ReplCompletionUseSite::Input)
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert!(
        input_labels.contains(&"count".to_string())
            && input_labels.contains(&"count_up".to_string()),
        "{input_labels:?}"
    );

    let type_labels = context
        .input_support(
            ":type co",
            ":type co".len(),
            ReplCompletionUseSite::Command(ReplCommandUseSite::Type),
        )
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert_eq!(type_labels, vec!["count".to_string()]);

    let owner_labels = context
        .input_support(
            ":type Cou",
            ":type Cou".len(),
            ReplCompletionUseSite::Command(ReplCommandUseSite::Type),
        )
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert_eq!(owner_labels, vec!["Counter".to_string()]);

    let special_labels = context
        .input_support(
            ":type tr",
            ":type tr".len(),
            ReplCompletionUseSite::Command(ReplCommandUseSite::Type),
        )
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert!(
        !special_labels
            .iter()
            .any(|label| label == "true" || label == "false"),
        "{special_labels:?}"
    );
}

#[test]
fn repl_input_support_context_distinguishes_expression_sig_and_type_use_sites() {
    let context = ReplInputSupportContext::from_update(ReplInputSupportUpdate {
        symbols: vec![
            CompletionSymbol {
                label: "count".to_string(),
                replacement: "count".to_string(),
                kind: CompletionKind::Variable,
                detail: Some("Int".to_string()),
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities: None,
            },
            CompletionSymbol {
                label: "count_up".to_string(),
                replacement: "count_up".to_string(),
                kind: CompletionKind::FunctionCall,
                detail: Some("count_up(value: Int) -> Int".to_string()),
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities: None,
            },
        ],
        callable_signatures: Vec::new(),
    });

    let expr_labels = context
        .input_support("co", 2, ReplCompletionUseSite::Input)
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert!(
        expr_labels.contains(&"count".to_string()) && expr_labels.contains(&"count_up".to_string()),
        "{expr_labels:?}"
    );

    let sig_labels = context
        .input_support(
            ":sig co",
            ":sig co".len(),
            ReplCompletionUseSite::Command(ReplCommandUseSite::Sig),
        )
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert!(
        sig_labels.contains(&"count".to_string()) && sig_labels.contains(&"count_up".to_string()),
        "{sig_labels:?}"
    );

    let doc_labels = context
        .input_support(
            ":doc co",
            ":doc co".len(),
            ReplCompletionUseSite::Command(ReplCommandUseSite::Doc),
        )
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert!(
        doc_labels.contains(&"count".to_string()) && doc_labels.contains(&"count_up".to_string()),
        "{doc_labels:?}"
    );

    let info_labels = context
        .input_support(
            ":info co",
            ":info co".len(),
            ReplCompletionUseSite::Command(ReplCommandUseSite::Info),
        )
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert!(
        info_labels.contains(&"count".to_string()) && info_labels.contains(&"count_up".to_string()),
        "{info_labels:?}"
    );

    let type_labels = context
        .input_support(
            ":type co",
            ":type co".len(),
            ReplCompletionUseSite::Command(ReplCommandUseSite::Type),
        )
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert_eq!(type_labels, vec!["count".to_string()]);
}

#[test]
fn repl_input_support_context_completes_command_heads_and_qualified_callables() {
    let context = ReplInputSupportContext::from_update(ReplInputSupportUpdate {
        symbols: vec![
            CompletionSymbol {
                label: "print".to_string(),
                replacement: "print".to_string(),
                kind: CompletionKind::Variable,
                detail: Some("String".to_string()),
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities: None,
            },
            CompletionSymbol {
                label: "Kernel::print".to_string(),
                replacement: "Kernel::print".to_string(),
                kind: CompletionKind::FunctionCall,
                detail: Some("Kernel::print(value: String) -> Unit".to_string()),
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities: None,
            },
        ],
        callable_signatures: Vec::new(),
    });

    assert!(ReplInputSupportContext::should_request(":si", ":si".len()));
    let head_labels = context
        .input_support(":si", ":si".len(), ReplCompletionUseSite::CommandHead)
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert!(head_labels.contains(&":sig".to_string()), "{head_labels:?}");

    let qualified_labels = context
        .input_support(
            ":sig Kernel::pr",
            ":sig Kernel::pr".len(),
            ReplCompletionUseSite::Command(ReplCommandUseSite::Sig),
        )
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert_eq!(qualified_labels, vec!["Kernel::print".to_string()]);
}

#[test]
fn repl_input_support_context_shows_nested_call_signatures_and_inner_candidates() {
    let context = ReplInputSupportContext::from_update(ReplInputSupportUpdate {
        symbols: vec![
            CompletionSymbol {
                label: "word".to_string(),
                replacement: "word".to_string(),
                kind: CompletionKind::Variable,
                detail: Some("String".to_string()),
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities: None,
            },
            CompletionSymbol {
                label: "width".to_string(),
                replacement: "width".to_string(),
                kind: CompletionKind::Variable,
                detail: Some("Int".to_string()),
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities: None,
            },
        ],
        callable_signatures: vec![
            CallableSignature {
                label: "if".to_string(),
                qualified_name: "if".to_string(),
                signature: "if(flag: Boolean, then_branch: Lazy<$A>, else_branch: Lazy<$A>) -> $A"
                    .to_string(),
            },
            CallableSignature {
                label: "String::contains".to_string(),
                qualified_name: "String::contains".to_string(),
                signature: "contains(value: String, needle: String) -> Boolean".to_string(),
            },
        ],
    });

    let support = context.input_support(
        "if(String::contains(w",
        "if(String::contains(w".len(),
        ReplCompletionUseSite::Input,
    );
    let signature = support
        .signature
        .expect("nested call signature help should be produced");
    assert_eq!(
        signature.lines,
        vec![
            "if(flag: [Boolean], then_branch: Lazy<$A>, else_branch: Lazy<$A>) -> $A".to_string(),
            "  String::contains(value: [String], needle: String) -> Boolean".to_string(),
        ]
    );
    assert_eq!(signature.active_parameter, Some(0));

    let labels = support
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(labels, vec!["word", "width"]);
}

#[test]
fn repl_input_support_context_drops_inner_signature_after_inner_call_closes() {
    let context = ReplInputSupportContext::from_update(ReplInputSupportUpdate {
        symbols: Vec::new(),
        callable_signatures: vec![
            CallableSignature {
                label: "if".to_string(),
                qualified_name: "if".to_string(),
                signature: "if(flag: Boolean, then_branch: Lazy<$A>, else_branch: Lazy<$A>) -> $A"
                    .to_string(),
            },
            CallableSignature {
                label: "String::contains".to_string(),
                qualified_name: "String::contains".to_string(),
                signature: "contains(value: String, needle: String) -> Boolean".to_string(),
            },
        ],
    });

    let input = "if(String::contains(word, needle), ";
    let support = context.input_support(input, input.len(), ReplCompletionUseSite::Input);
    let signature = support
        .signature
        .expect("outer call signature help should remain after inner call closes");
    assert_eq!(
        signature.lines,
        vec!["if(flag: Boolean, then_branch: [Lazy<$A>], else_branch: Lazy<$A>) -> $A".to_string()]
    );
    assert_eq!(signature.active_parameter, Some(1));
}

#[test]
fn repl_input_support_context_keeps_path_candidates_inside_outer_call_arguments() {
    let context = ReplInputSupportContext::from_update(ReplInputSupportUpdate {
        symbols: vec![CompletionSymbol {
            label: "String::contains".to_string(),
            replacement: "String::contains".to_string(),
            kind: CompletionKind::TypePath,
            detail: Some("String::contains(value: String, needle: String) -> Boolean".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        }],
        callable_signatures: vec![
            CallableSignature {
                label: "if".to_string(),
                qualified_name: "if".to_string(),
                signature: "if(flag: Boolean, then_branch: Lazy<$A>, else_branch: Lazy<$A>) -> $A"
                    .to_string(),
            },
            CallableSignature {
                label: "String::contains".to_string(),
                qualified_name: "String::contains".to_string(),
                signature: "contains(value: String, needle: String) -> Boolean".to_string(),
            },
        ],
    });

    let input = "if(String::c";
    let support = context.input_support(input, input.len(), ReplCompletionUseSite::Input);
    assert_eq!(
        support
            .signature
            .as_ref()
            .expect("outer call signature should remain visible")
            .lines,
        vec!["if(flag: [Boolean], then_branch: Lazy<$A>, else_branch: Lazy<$A>) -> $A".to_string()]
    );
    assert!(
        support
            .candidates
            .iter()
            .any(|candidate| candidate.label == "String::contains"),
        "path candidates should remain available inside call arguments: {:?}",
        support.candidates
    );
}

#[test]
fn repl_input_support_context_limits_nested_signature_display_to_two_levels() {
    let context = ReplInputSupportContext::from_update(ReplInputSupportUpdate {
        symbols: Vec::new(),
        callable_signatures: vec![
            CallableSignature {
                label: "if".to_string(),
                qualified_name: "if".to_string(),
                signature: "if(flag: Boolean, then_branch: Lazy<$A>, else_branch: Lazy<$A>) -> $A"
                    .to_string(),
            },
            CallableSignature {
                label: "wrap".to_string(),
                qualified_name: "wrap".to_string(),
                signature: "wrap(value: Boolean) -> Boolean".to_string(),
            },
            CallableSignature {
                label: "String::contains".to_string(),
                qualified_name: "String::contains".to_string(),
                signature: "contains(value: String, needle: String) -> Boolean".to_string(),
            },
        ],
    });

    let input = "if(wrap(String::contains(w";
    let support = context.input_support(input, input.len(), ReplCompletionUseSite::Input);
    let signature = support
        .signature
        .expect("nested call signature help should be produced");
    assert_eq!(
        signature.lines,
        vec![
            "wrap(value: [Boolean]) -> Boolean".to_string(),
            "  String::contains(value: [String], needle: String) -> Boolean".to_string(),
        ]
    );
}

#[test]
fn repl_input_support_context_produces_operator_assist_and_ranked_candidates() {
    let context = ReplInputSupportContext::from_update(ReplInputSupportUpdate {
        symbols: vec![
            CompletionSymbol {
                label: "answer".to_string(),
                replacement: "answer".to_string(),
                kind: CompletionKind::Variable,
                detail: Some("Int".to_string()),
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
        ],
        callable_signatures: Vec::new(),
    });

    assert!(ReplInputSupportContext::should_request(
        "1 + ",
        "1 + ".len()
    ));

    let support = context.input_support("1 + ", "1 + ".len(), ReplCompletionUseSite::Input);
    let signature = support
        .signature
        .expect("operator rhs should show signature help");
    assert_eq!(signature.lines, vec!["Int + [Int]".to_string()]);

    assert_eq!(
        support
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["answer", "name"]
    );
}

#[test]
fn semantic_index_adds_module_owner_symbols_from_declarations() {
    let owners = owner_registry(&[("Helper", TypeIdentity::Mod, OwnerKind::Mod)]);
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

    let index = SemanticIndex::from_declaration_index(&owners, &declarations);
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
    let helper = completion
        .candidates
        .iter()
        .find(|candidate| candidate.label == "Helper::helper")
        .expect("helper completion should exist");
    assert_eq!(
        helper.origin,
        Some(CompletionOrigin::Declaration {
            qualified_name: "Helper::helper".to_string(),
            module_path: "Helper".to_string(),
            name: "helper".to_string(),
            stage_index: 0,
            auto_import: false,
            visibility: Visibility::Public,
            user_importable: true,
            user_callable: true,
            via_import: false,
            via_auto_import: false,
            shadowed_auto_import: false,
        })
    );
}

#[test]
fn semantic_index_deduplicates_completion_symbols() {
    let index = SemanticIndex::from_symbols(vec![
        CompletionSymbol {
            label: "print".to_string(),
            replacement: "print".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: None,
            documentation: None,
            sort_text: None,
            origin: None,

            definition: None,

            capabilities: None,
        },
        CompletionSymbol {
            label: "print".to_string(),
            replacement: "print".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: Some("duplicate".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,

            definition: None,

            capabilities: None,
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

fn owner_registry(entries: &[(&str, TypeIdentity, OwnerKind)]) -> OwnerRegistry {
    let mut registry = OwnerRegistry::default();
    for (canonical_key, identity, kind) in entries {
        registry
            .register(OwnerEntry {
                canonical_key: (*canonical_key).to_string(),
                identity: *identity,
                kind: *kind,
                span: Span { start: 0, end: 1 },
                stage_index: 0,
                module_path: matches!(kind, OwnerKind::Mod | OwnerKind::Supervisor)
                    .then(|| (*canonical_key).to_string()),
            })
            .expect("test owner keys should be distinct");
    }
    registry
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
    assert_eq!(
        completion.candidates[0].documentation.as_deref(),
        Some("Writes a line.")
    );
    assert_eq!(
        completion.candidates[0].sort_text.as_deref(),
        Some("1:print")
    );
    assert_eq!(
        completion.candidates[0].origin,
        Some(CompletionOrigin::Metadata {
            qualified_name: "Global::print".to_string(),
            module_path: "Global".to_string(),
        })
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
    assert_eq!(
        type_completion.candidates[0].documentation.as_deref(),
        Some("UTF-8 text.")
    );
}

#[test]
fn semantic_index_builds_from_symbol_semantic_infos() {
    let index =
        SemanticIndex::from_symbol_semantic_infos(vec![surtr_analysis::SymbolSemanticInfo {
            canonical_name: "Global::Helper::helper".to_string(),
            surface_name: "Helper::helper".to_string(),
            replacement: "Helper::helper".to_string(),
            kind: CompletionKind::FunctionCall,
            identity: None,
            detail: Some("Helper::helper(value: Int) -> Int".to_string()),
            documentation: Some("Increment a number.".to_string()),
            sort_text: Some("1:Helper::helper".to_string()),
            origin: Some(CompletionOrigin::Declaration {
                qualified_name: "Global::Helper::helper".to_string(),
                module_path: "Global::Helper".to_string(),
                name: "helper".to_string(),
                stage_index: 1,
                auto_import: false,
                visibility: spire::ast::Visibility::Public,
                user_importable: true,
                user_callable: true,
                via_import: false,
                via_auto_import: false,
                shadowed_auto_import: false,
            }),
            definition: None,
            capabilities: None,
            display_metadata: Some(SymbolDisplayMetadata {
                qualified_name: "Global::Helper::helper".to_string(),
                module_path: "Global::Helper".to_string(),
                has_doc: true,
                has_signature: true,
            }),
        }]);

    let symbol = index
        .symbols()
        .iter()
        .find(|symbol| symbol.label == "Helper::helper")
        .expect("semantic info should project to completion symbol");
    let info = index
        .symbol_semantic_infos()
        .iter()
        .find(|info| info.surface_name == "Helper::helper")
        .expect("semantic index should preserve aggregate info");

    assert_eq!(
        symbol.detail.as_deref(),
        Some("Helper::helper(value: Int) -> Int")
    );
    assert_eq!(symbol.documentation.as_deref(), Some("Increment a number."));
    assert_eq!(
        info.display_metadata.as_ref().map(|metadata| (
            metadata.qualified_name.as_str(),
            metadata.has_doc,
            metadata.has_signature
        )),
        Some(("Global::Helper::helper", true, true))
    );
}

#[test]
fn semantic_index_upsert_preserves_existing_symbol_semantic_info() {
    let mut index =
        SemanticIndex::from_symbol_semantic_infos(vec![surtr_analysis::SymbolSemanticInfo {
            canonical_name: "Global::Helper::helper".to_string(),
            surface_name: "helper".to_string(),
            replacement: "helper".to_string(),
            kind: CompletionKind::FunctionCall,
            identity: None,
            detail: Some("helper() -> Int".to_string()),
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
            display_metadata: Some(SymbolDisplayMetadata {
                qualified_name: "Global::Helper::helper".to_string(),
                module_path: "Global::Helper".to_string(),
                has_doc: false,
                has_signature: true,
            }),
        }]);

    index.upsert_symbol(CompletionSymbol {
        label: "helper".to_string(),
        replacement: "helper".to_string(),
        kind: CompletionKind::FunctionCall,
        detail: None,
        documentation: Some("Imported helper.".to_string()),
        sort_text: None,
        origin: None,
        definition: None,
        capabilities: None,
    });

    let info = index
        .symbol_semantic_infos()
        .iter()
        .find(|info| info.surface_name == "helper")
        .expect("upserted helper should remain visible");
    assert_eq!(info.documentation.as_deref(), Some("Imported helper."));
    assert_eq!(
        info.display_metadata.as_ref().map(|metadata| (
            metadata.qualified_name.as_str(),
            metadata.has_doc,
            metadata.has_signature
        )),
        Some(("Global::Helper::helper", false, true))
    );
}

#[test]
fn semantic_index_compile_metadata_joins_declaration_docs_and_signatures() {
    let owners = owner_registry(&[("User", TypeIdentity::Struct, OwnerKind::Struct)]);
    let mut declarations = DeclarationIndex::new();
    declarations.insert(
        "Global::Helper::User".to_string(),
        declaration_entry(
            "Global::Helper",
            "User",
            "Global::Helper::User",
            DeclarationKind::Struct,
            true,
            false,
        ),
    );
    let docs = vec![DocEntry {
        qualified_name: "Global::Helper::User".to_string(),
        kind: DocKind::Type,
        module_path: "Global::Helper".to_string(),
        signature: Some("User(name: String)".to_string()),
        doc: "User type.".to_string(),
    }];
    let signatures = vec![SignatureEntry {
        qualified_name: "Global::Helper::User".to_string(),
        kind: DocKind::Type,
        module_path: "Global::Helper".to_string(),
        signature: "User(name: String)".to_string(),
    }];

    let index = SemanticIndex::from_compile_metadata(&owners, &declarations, &docs, &signatures);
    let symbol = index
        .symbols()
        .iter()
        .find(|symbol| symbol.label == "Helper::User")
        .expect("user symbol should exist");

    assert_eq!(symbol.detail.as_deref(), Some("User(name: String)"));
    assert_eq!(symbol.documentation.as_deref(), Some("User type."));
    assert!(matches!(
        symbol.origin.as_ref(),
        Some(CompletionOrigin::Declaration { qualified_name, .. })
            if qualified_name == "Global::Helper::User"
    ));
    assert!(
        symbol.capabilities.is_some(),
        "joined declaration metadata should preserve capabilities: {symbol:?}"
    );
}

#[test]
fn semantic_index_enriches_symbols_with_compile_metadata_aggregate() {
    let owners = owner_registry(&[("User", TypeIdentity::Struct, OwnerKind::Struct)]);
    let mut declarations = DeclarationIndex::new();
    declarations.insert(
        "Global::Helper::User".to_string(),
        declaration_entry(
            "Global::Helper",
            "User",
            "Global::Helper::User",
            DeclarationKind::Struct,
            true,
            false,
        ),
    );
    let docs = vec![DocEntry {
        qualified_name: "Global::Helper::User".to_string(),
        kind: DocKind::Type,
        module_path: "Global::Helper".to_string(),
        signature: Some("User(name: String)".to_string()),
        doc: "User type.".to_string(),
    }];
    let signatures = vec![SignatureEntry {
        qualified_name: "Global::Helper::User".to_string(),
        kind: DocKind::Type,
        module_path: "Global::Helper".to_string(),
        signature: "User(name: String)".to_string(),
    }];
    let index = SemanticIndex::enrich_symbols_with_compile_metadata(
        vec![CompletionSymbol {
            label: "Helper::User".to_string(),
            replacement: "Helper::User".to_string(),
            kind: CompletionKind::TypeConstructor,
            detail: None,
            documentation: None,
            sort_text: None,
            origin: None,
            definition: None,
            capabilities: None,
        }],
        &owners,
        &declarations,
        &docs,
        &signatures,
    );

    let info = index
        .symbol_semantic_infos()
        .iter()
        .find(|info| info.surface_name == "Helper::User")
        .expect("enriched symbol should keep semantic info");

    assert_eq!(info.detail.as_deref(), Some("User(name: String)"));
    assert_eq!(
        info.display_metadata.as_ref().map(|metadata| (
            metadata.qualified_name.as_str(),
            metadata.has_doc,
            metadata.has_signature
        )),
        Some(("Global::Helper::User", true, true))
    );
}

#[test]
fn compile_metadata_exposes_symbol_semantic_info_before_completion_projection() {
    let owners = owner_registry(&[("User", TypeIdentity::Struct, OwnerKind::Struct)]);
    let mut declarations = DeclarationIndex::new();
    declarations.insert(
        "Global::Helper::User".to_string(),
        declaration_entry(
            "Global::Helper",
            "User",
            "Global::Helper::User",
            DeclarationKind::Struct,
            true,
            false,
        ),
    );
    let docs = vec![DocEntry {
        qualified_name: "Global::Helper::User".to_string(),
        kind: DocKind::Type,
        module_path: "Global::Helper".to_string(),
        signature: Some("User(name: String)".to_string()),
        doc: "User type.".to_string(),
    }];
    let signatures = vec![SignatureEntry {
        qualified_name: "Global::Helper::User".to_string(),
        kind: DocKind::Type,
        module_path: "Global::Helper".to_string(),
        signature: "User(name: String)".to_string(),
    }];

    let infos = surtr_analysis::symbol_semantic_infos_from_compile_metadata(
        &owners,
        &declarations,
        &docs,
        &signatures,
    );
    let info = infos
        .iter()
        .find(|info| info.canonical_name == "Global::Helper::User")
        .expect("semantic info should preserve canonical symbol identity");

    assert_eq!(info.surface_name, "Helper::User");
    assert_eq!(info.kind, CompletionKind::TypeConstructor);
    assert_eq!(info.identity, Some(TypeIdentity::Struct));
    assert_eq!(info.detail.as_deref(), Some("User(name: String)"));
    assert_eq!(info.documentation.as_deref(), Some("User type."));
    let display_metadata = info
        .display_metadata
        .as_ref()
        .expect("compile semantic info should retain display metadata origin");
    let _: &SymbolDisplayMetadata = display_metadata;
    assert_eq!(display_metadata.qualified_name, "Global::Helper::User");
    assert!(display_metadata.has_doc);
    assert!(display_metadata.has_signature);
    assert!(
        info.capabilities.is_some(),
        "semantic info should preserve capabilities before completion projection: {info:?}"
    );
}

#[test]
fn shared_builtin_surface_capability_query_excludes_runtime_aliases() {
    let string_caps =
        surtr_analysis::symbol_capabilities_for_builtin_surface("String").expect("String caps");
    assert!(string_caps.type_annotation);
    assert!(string_caps.module_owner);
    assert!(string_caps.impl_target);
    assert_eq!(string_caps.facet_root_path, None);

    let boolean_caps =
        surtr_analysis::symbol_capabilities_for_builtin_surface("Boolean").expect("Boolean caps");
    assert!(boolean_caps.type_annotation);
    assert!(boolean_caps.module_owner);
    assert!(boolean_caps.impl_target);
    assert_eq!(boolean_caps.facet_root_path, Some(FacetRootKind::TypeRoot));

    assert!(
        surtr_analysis::symbol_capabilities_for_builtin_surface("String::len").is_none(),
        "runtime builtin aliases are not compile-space symbol surfaces"
    );
    assert_eq!(
        surtr_analysis::facet_type_root_capabilities().facet_root_path,
        Some(FacetRootKind::TypeRoot)
    );
}

#[test]
fn semantic_index_uses_registry_for_complete_owner_and_member_identities() {
    let owners = owner_registry(&[
        ("Hoge", TypeIdentity::Mod, OwnerKind::Mod),
        ("Worker", TypeIdentity::Mod, OwnerKind::Mod),
        ("RootSup", TypeIdentity::Supervisor, OwnerKind::Supervisor),
        ("Alias", TypeIdentity::Sig, OwnerKind::Sig),
        ("Flag", TypeIdentity::Const, OwnerKind::Const),
        ("Show", TypeIdentity::Trait, OwnerKind::Trait),
        ("Functor", TypeIdentity::TypeConstructor, OwnerKind::Trait),
    ]);
    let mut declarations = DeclarationIndex::new();
    declarations.insert(
        "Hoge::test".to_string(),
        declaration_entry(
            "Hoge",
            "test",
            "Hoge::test",
            DeclarationKind::Def,
            true,
            true,
        ),
    );
    declarations.insert(
        "Flag".to_string(),
        declaration_entry("", "Flag", "Flag", DeclarationKind::Const, true, true),
    );
    declarations.insert(
        "Show".to_string(),
        declaration_entry("", "Show", "Show", DeclarationKind::Trait, true, true),
    );
    declarations.insert(
        "Functor".to_string(),
        declaration_entry("", "Functor", "Functor", DeclarationKind::Trait, true, true),
    );

    let index = SemanticIndex::from_declaration_index(&owners, &declarations);
    for (name, identity, kind) in [
        ("Hoge", TypeIdentity::Mod, CompletionKind::TypePath),
        ("Worker", TypeIdentity::Mod, CompletionKind::TypePath),
        (
            "RootSup",
            TypeIdentity::Supervisor,
            CompletionKind::TypePath,
        ),
        ("Alias", TypeIdentity::Sig, CompletionKind::TypePath),
        ("Flag", TypeIdentity::Const, CompletionKind::Variable),
        ("Show", TypeIdentity::Trait, CompletionKind::TypePath),
        (
            "Functor",
            TypeIdentity::TypeConstructor,
            CompletionKind::TypePath,
        ),
    ] {
        let info = index
            .symbol_semantic_infos()
            .iter()
            .find(|info| info.canonical_name == name && info.kind == kind)
            .unwrap_or_else(|| panic!("missing semantic owner info for {name}"));
        assert_eq!(info.identity, Some(identity), "{name}");
    }

    let member = declarations.get("Hoge::test").expect("member declaration");
    assert_eq!(
        surtr_analysis::symbol_identity_for_declaration_entry(&owners, member),
        Some(TypeIdentity::Mod)
    );
    let member_info = index
        .symbol_semantic_infos()
        .iter()
        .find(|info| info.canonical_name == "Hoge::test")
        .expect("member semantic info");
    assert_eq!(member_info.identity, Some(TypeIdentity::Mod));
    assert_eq!(member_info.kind, CompletionKind::FunctionCall);
}

#[test]
fn semantic_identity_does_not_fall_back_to_declaration_shape() {
    let owners = OwnerRegistry::default();
    let ghost_member = declaration_entry(
        "Ghost",
        "test",
        "Ghost::test",
        DeclarationKind::Def,
        true,
        true,
    );
    let orphan_const = declaration_entry("", "Flag", "Flag", DeclarationKind::Const, true, true);
    let declarations = DeclarationIndex::from([
        ("Ghost::test".to_string(), ghost_member.clone()),
        ("Flag".to_string(), orphan_const.clone()),
    ]);

    assert_eq!(
        surtr_analysis::symbol_identity_for_declaration_entry(&owners, &ghost_member),
        None
    );
    assert_eq!(
        surtr_analysis::symbol_identity_for_declaration_entry(&owners, &orphan_const),
        None
    );
    let infos =
        surtr_analysis::symbol_semantic_infos_from_declaration_index(&owners, &declarations);
    assert!(!infos.iter().any(|info| info.canonical_name == "Ghost"));
}

#[test]
fn shared_declaration_capability_query_handles_user_and_builtin_surfaces() {
    let owners = owner_registry(&[("User", TypeIdentity::Struct, OwnerKind::Struct)]);
    let user = declaration_entry(
        "Global",
        "User",
        "Global::User",
        DeclarationKind::Struct,
        true,
        false,
    );
    let user_caps = surtr_analysis::symbol_capabilities_for_declaration_entry(&owners, &user)
        .expect("user caps");
    assert_eq!(user_caps.facet_root_path, Some(FacetRootKind::TypeRoot));
    assert_eq!(
        surtr_analysis::symbol_identity_for_declaration_entry(&owners, &user),
        Some(TypeIdentity::Struct)
    );

    let builtin = declaration_entry(
        "Global",
        "String",
        "Global::String",
        DeclarationKind::BuiltinType,
        true,
        false,
    );
    let builtin_caps = surtr_analysis::symbol_capabilities_for_declaration_entry(
        &OwnerRegistry::default(),
        &builtin,
    )
    .expect("builtin caps");
    assert!(builtin_caps.type_annotation);
    assert_eq!(builtin_caps.facet_root_path, None);
    assert_eq!(
        surtr_analysis::symbol_identity_for_declaration_entry(&OwnerRegistry::default(), &builtin),
        Some(TypeIdentity::Type)
    );
}

#[test]
fn effective_visible_entry_semantic_info_reuses_qualified_symbol_metadata() {
    let source_location = surtr_analysis::SourceLocation {
        path: "/repo/helper.srt".into(),
        start: 4,
        end: 10,
    };
    let visible = sigil::EffectiveVisibleEntry {
        visible_name: "helper".to_string(),
        via_import: true,
        via_auto_import: false,
        shadowed_auto_import: false,
        importable: true,
        callable: true,
        entry: declaration_entry(
            "Global::Helper",
            "helper",
            "Global::Helper::helper",
            DeclarationKind::Def,
            true,
            true,
        ),
    };

    let projected = surtr_analysis::symbol_semantic_info_for_effective_visible_entry(
        &OwnerRegistry::default(),
        &[surtr_analysis::SymbolSemanticInfo {
            canonical_name: "Global::Helper::helper".to_string(),
            surface_name: "Helper::helper".to_string(),
            replacement: "Helper::helper".to_string(),
            kind: CompletionKind::FunctionCall,
            identity: None,
            detail: Some("Helper::helper(value: Int) -> Int".to_string()),
            documentation: Some("Increment a number.".to_string()),
            sort_text: Some("1:Helper::helper".to_string()),
            origin: None,
            definition: Some(source_location.clone()),
            capabilities: Some(SymbolCapabilities::new(
                true,
                true,
                true,
                Some(FacetRootKind::TypeRoot),
            )),
            display_metadata: Some(SymbolDisplayMetadata {
                qualified_name: "Global::Helper::helper".to_string(),
                module_path: "Global::Helper".to_string(),
                has_doc: true,
                has_signature: true,
            }),
        }],
        &visible,
    )
    .expect("visible helper should project");

    assert_eq!(projected.canonical_name, "Global::Helper::helper");
    assert_eq!(projected.surface_name, "helper");
    assert_eq!(projected.replacement, "helper");
    assert_eq!(
        projected.detail.as_deref(),
        Some("Helper::helper(value: Int) -> Int")
    );
    assert_eq!(
        projected.documentation.as_deref(),
        Some("Increment a number.")
    );
    assert_eq!(projected.sort_text.as_deref(), Some("1:Helper::helper"));
    assert_eq!(projected.definition, Some(source_location));
    assert!(matches!(
        projected.origin.as_ref(),
        Some(CompletionOrigin::Declaration {
            qualified_name,
            via_import,
            via_auto_import,
            shadowed_auto_import,
            ..
        }) if qualified_name == "Global::Helper::helper"
            && *via_import
            && !*via_auto_import
            && !*shadowed_auto_import
    ));
    assert_eq!(
        projected.capabilities,
        Some(SymbolCapabilities::new(
            true,
            true,
            true,
            Some(FacetRootKind::TypeRoot),
        ))
    );
    assert_eq!(
        projected
            .display_metadata
            .as_ref()
            .map(|metadata| (metadata.qualified_name.as_str(), metadata.has_doc)),
        Some(("Global::Helper::helper", true))
    );
}

#[test]
fn call_argument_completion_ranks_self_trait_constraint_candidates_from_impl_signatures() {
    let index = SemanticIndex::from_symbols(vec![
        completion_symbol(
            "compare",
            CompletionKind::FunctionCall,
            "Compare::compare(self: Self, rhs: Self) -> Ordering",
        ),
        completion_symbol(
            "impl Compare for Int",
            CompletionKind::TypeConstructor,
            "impl Compare for Int",
        ),
        completion_symbol("count", CompletionKind::Variable, "Int"),
        completion_symbol("flag", CompletionKind::Variable, "Boolean"),
    ]);

    let completion = complete_call_argument(CompletionRequest {
        index: &index,
        source: "compare(",
        cursor: "compare(".len(),
    })
    .expect("compare call argument should use signature context");

    assert_eq!(
        completion
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["count", "flag"]
    );
    assert_eq!(
        completion
            .candidates
            .iter()
            .map(|candidate| candidate.sort_text.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("0000:count"), Some("0001:flag")]
    );
}

#[test]
fn call_argument_completion_uses_trait_impl_signature_not_builtin_type_whitelist() {
    let index = SemanticIndex::from_symbols(vec![
        completion_symbol(
            "compare",
            CompletionKind::FunctionCall,
            "Compare::compare(self: Self, rhs: Self) -> Ordering",
        ),
        completion_symbol(
            "impl Compare for UserRank",
            CompletionKind::TypeConstructor,
            "impl Compare for UserRank",
        ),
        completion_symbol("rank", CompletionKind::Variable, "UserRank"),
        completion_symbol("name", CompletionKind::Variable, "String"),
    ]);

    let completion = complete_call_argument(CompletionRequest {
        index: &index,
        source: "compare(",
        cursor: "compare(".len(),
    })
    .expect("compare call argument should use signature context");

    assert_eq!(
        completion
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["rank", "name"]
    );
}

#[test]
fn facet_arg_completion_includes_capability_declared_roots_without_detail_heuristics() {
    let owners = owner_registry(&[
        ("User", TypeIdentity::Struct, OwnerKind::Struct),
        ("Config", TypeIdentity::Record, OwnerKind::Record),
        ("Choice", TypeIdentity::Enum, OwnerKind::Enum),
        ("Problem", TypeIdentity::Error, OwnerKind::Error),
    ]);
    let mut declarations = DeclarationIndex::new();
    declarations.insert(
        "User".to_string(),
        declaration_entry("", "User", "User", DeclarationKind::Struct, true, true),
    );
    declarations.insert(
        "Config".to_string(),
        declaration_entry("", "Config", "Config", DeclarationKind::Record, true, true),
    );
    declarations.insert(
        "Choice".to_string(),
        declaration_entry("", "Choice", "Choice", DeclarationKind::Enum, true, true),
    );
    declarations.insert(
        "Problem".to_string(),
        declaration_entry(
            "",
            "Problem",
            "Problem",
            DeclarationKind::Deferror,
            true,
            true,
        ),
    );

    let mut symbols = vec![completion_symbol(
        "Facet::view",
        CompletionKind::FunctionCall,
        "Facet::view(path: Facet<ReadablePath, $S, $A, _, _>, source: $S) -> Result<$A>",
    )];
    symbols.extend(
        SemanticIndex::from_declaration_index(&owners, &declarations)
            .symbols()
            .iter()
            .cloned(),
    );
    let index = SemanticIndex::from_symbols(symbols);

    let completion = complete_call_argument(CompletionRequest {
        index: &index,
        source: "Facet::view(",
        cursor: "Facet::view(".len(),
    })
    .expect("Facet first argument should use facet-root capabilities");

    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(labels, vec!["Choice", "Config", "User"]);
}

#[test]
fn facet_arg_completion_includes_builtin_path_roots_and_excludes_plain_builtin_types() {
    let docs = [
        ("Global::Tuple", "Tuple path root."),
        ("Global::List", "List path root."),
        ("Global::HashMap", "HashMap path root."),
        ("Global::Boolean", "Boolean variant path root."),
        ("Global::String", "Plain string type."),
        ("Global::Result", "Plain result type."),
        ("Global::Facet", "Facet type."),
    ]
    .into_iter()
    .map(|(qualified_name, doc)| DocEntry {
        qualified_name: qualified_name.to_string(),
        kind: DocKind::Type,
        module_path: "Global".to_string(),
        signature: Some(format!(
            "type {}",
            qualified_name.rsplit("::").next().unwrap()
        )),
        doc: doc.to_string(),
    })
    .collect::<Vec<_>>();
    let mut symbols = vec![completion_symbol(
        "Facet::view",
        CompletionKind::FunctionCall,
        "Facet::view(path: Facet<ReadablePath, $S, $A, _, _>, source: $S) -> Result<$A>",
    )];
    symbols.extend(
        SemanticIndex::from_metadata(&docs, &[])
            .symbols()
            .iter()
            .cloned(),
    );
    let index = SemanticIndex::from_symbols(symbols);

    let completion = complete_call_argument(CompletionRequest {
        index: &index,
        source: "Facet::view(",
        cursor: "Facet::view(".len(),
    })
    .expect("Facet first argument should include builtin facet path roots");

    assert_eq!(
        completion
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Boolean", "HashMap", "List", "Tuple"]
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
            documentation: None,
            sort_text: None,
            origin: None,

            definition: None,

            capabilities: None,
        },
        CompletionSymbol {
            label: "alpha".to_string(),
            replacement: "alpha".to_string(),
            kind: CompletionKind::FunctionCall,
            detail: None,
            documentation: None,
            sort_text: None,
            origin: None,

            definition: None,

            capabilities: None,
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

#[test]
fn expected_type_ranking_updates_sort_text_for_lsp_clients() {
    let candidates = vec![
        completion_candidate("text", "String"),
        completion_candidate("count", "Int"),
    ];

    let ranked =
        rank_completion_candidates_by_expected_type(candidates, Some("Int"), |expected, actual| {
            expected == actual
        });

    assert_eq!(
        ranked
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        vec!["count", "text"]
    );
    assert_eq!(ranked[0].sort_text.as_deref(), Some("0000:count"));
    assert_eq!(ranked[1].sort_text.as_deref(), Some("0001:text"));
}

fn completion_symbol(label: &str, kind: CompletionKind, detail: &str) -> CompletionSymbol {
    CompletionSymbol {
        label: label.to_string(),
        replacement: label.to_string(),
        kind,
        detail: Some(detail.to_string()),
        documentation: None,
        sort_text: None,
        origin: None,
        definition: None,
        capabilities: None,
    }
}

fn completion_candidate(label: &str, ty: &str) -> CompletionCandidate {
    CompletionCandidate {
        label: label.to_string(),
        replacement: label.to_string(),
        kind: CompletionKind::Variable,
        detail: Some(ty.to_string()),
        documentation: None,
        sort_text: None,
        origin: None,
        capabilities: None,
        replace_start: 0,
        replace_end: 0,
    }
}
