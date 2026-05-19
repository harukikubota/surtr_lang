use sigil::{DeclarationEntry, DeclarationIndex, DeclarationKind};
use sindr::ir::{DocEntry, DocKind, SignatureEntry};
use sindr::names::{FacetRootKind, SymbolCapabilities};
use spire::ast::Visibility;
use surtr_analysis::{
    complete_call_argument, complete_prefix, lookup_symbol_at_cursor,
    rank_completion_candidates_by_expected_type, repl_assist_at_cursor, signature_help_at_cursor,
    CallableSignature, CompletionCandidate, CompletionKind, CompletionOrigin, CompletionRequest,
    CompletionScope, CompletionSymbol, ReplInputSupportContext, ReplInputSupportUpdate,
    SemanticIndex,
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
        CompletionScope::VariablesOnly,
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
        CompletionScope::All,
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

    let completion = context.input_support("val", 3, CompletionScope::All);
    assert_eq!(completion.candidates.len(), 1);
    assert_eq!(completion.candidates[0].label, "value");
    assert_eq!(completion.candidates[0].detail.as_deref(), Some("Int"));

    let support = context.input_support("fresh(", "fresh(".len(), CompletionScope::All);
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

    let support = context.input_support("1 + ", "1 + ".len(), CompletionScope::All);
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
fn semantic_index_compile_metadata_joins_declaration_docs_and_signatures() {
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

    let index = SemanticIndex::from_compile_metadata(&declarations, &docs, &signatures);
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
fn compile_metadata_exposes_symbol_semantic_info_before_completion_projection() {
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
    assert_eq!(info.detail.as_deref(), Some("User(name: String)"));
    assert_eq!(info.documentation.as_deref(), Some("User type."));
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
fn shared_declaration_capability_query_handles_user_and_builtin_surfaces() {
    let user = declaration_entry(
        "Global",
        "User",
        "Global::User",
        DeclarationKind::Struct,
        true,
        false,
    );
    let user_caps =
        surtr_analysis::symbol_capabilities_for_declaration_entry(&user).expect("user caps");
    assert_eq!(user_caps.facet_root_path, Some(FacetRootKind::TypeRoot));

    let builtin = declaration_entry(
        "Global",
        "String",
        "Global::String",
        DeclarationKind::BuiltinType,
        true,
        false,
    );
    let builtin_caps =
        surtr_analysis::symbol_capabilities_for_declaration_entry(&builtin).expect("builtin caps");
    assert!(builtin_caps.type_annotation);
    assert_eq!(builtin_caps.facet_root_path, None);
}

#[test]
fn effective_visible_entry_projection_reuses_qualified_symbol_metadata() {
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
    let projected = surtr_analysis::semantic::completion_symbol_for_effective_visible_entry(
        &[CompletionSymbol {
            label: "Helper::helper".to_string(),
            replacement: "Helper::helper".to_string(),
            kind: CompletionKind::FunctionCall,
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
        }],
        &visible,
    )
    .expect("visible helper should project");

    assert_eq!(projected.label, "helper");
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
        Some(CompletionOrigin::Declaration { qualified_name, .. })
            if qualified_name == "Global::Helper::helper"
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
        "Facet::view(path: Facet<$S, $A>, source: $S) -> $A",
    )];
    symbols.extend(
        SemanticIndex::from_declaration_index(&declarations)
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
        "Facet::view(path: Facet<$S, $A>, source: $S) -> $A",
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
        vec!["HashMap", "List", "Tuple"]
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
