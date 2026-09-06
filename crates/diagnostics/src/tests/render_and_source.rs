use super::test_support::*;

#[test]
fn fallback_diagnostic_preserves_core_fields() {
    let spec = DiagnosticSpec {
        kind: "TypeError".into(),
        message: "expected Int, got String".into(),
        primary_span: Span { start: 13, end: 23 },
        labels: vec![DiagnosticLabel {
            source_id: None,
            span: Span { start: 13, end: 23 },
            message: "binding value".into(),
            color: Some(Color::Blue),
        }],
        notes: Vec::new(),
        help: Some("The type annotation requires Int".into()),
        structured: None,
    };

    let mut buf = Vec::new();
    write_fallback_diagnostic(
        &mut buf,
        "main.srt",
        &spec,
        &io::Error::other("broken pipe"),
    )
    .expect("fallback output should succeed");

    let text = String::from_utf8(buf).expect("fallback output must be valid utf-8");
    assert!(text.contains("diagnostic rendering failed: broken pipe"));
    assert!(text.contains("TypeError: expected Int, got String"));
    assert!(text.contains("--> main.srt:13-23"));
    assert!(text.contains("= note: binding value [13-23]"));
    assert!(text.contains("= help: The type annotation requires Int"));
}

#[test]
fn fallback_diagnostic_returns_writer_error() {
    let spec = simple_error(
        "ParseError",
        "unexpected token",
        Span { start: 2, end: 5 },
        None,
    );
    let err = write_fallback_diagnostic(
        &mut FailingWriter,
        "main.srt",
        &spec,
        &io::Error::other("broken pipe"),
    )
    .expect_err("failing writer should propagate io error");

    assert_eq!(err.kind(), io::ErrorKind::Other);
}

#[test]
fn source_registry_registers_and_updates_entries() {
    let mut sources = SourceRegistry::new();
    let src_id = sources.register("main.srt", "x = 1");

    assert_eq!(sources.file_name(src_id), Some("main.srt"));
    assert_eq!(sources.source(src_id), Some("x = 1"));

    assert!(sources.update_source(src_id, "x = 2"));
    assert_eq!(sources.source(src_id), Some("x = 2"));
}

#[test]
fn source_registry_returns_owned_context() {
    let mut sources = SourceRegistry::new();
    let src_id = sources.register("script.srt", "print(\"ok\")");

    let context = sources
        .owned_context(src_id)
        .expect("registered source should exist");
    assert_eq!(context.0, "print(\"ok\")");
    assert_eq!(context.1, "script.srt");
}

#[test]
fn render_error_normalizes_out_of_range_span_to_keep_source_snippet() {
    let spec = simple_error(
        "TypeError",
        "expected Int, got String",
        Span {
            start: 9999,
            end: 10000,
        },
        None,
    );
    let rendered = render_error("main.srt", "bad: Int = \"x\"", &spec);
    assert!(
        rendered.contains("TypeError: expected Int, got String"),
        "expected headline in rendered diagnostic"
    );
    assert!(
        rendered.contains("main.srt"),
        "expected file label in rendered diagnostic"
    );
}

#[test]
fn render_error_normalizes_empty_span_to_single_point_label() {
    let spec = simple_error(
        "ParseError",
        "unexpected token",
        Span { start: 4, end: 4 },
        None,
    );
    let rendered = render_error("main.srt", "abcde", &spec);
    assert!(rendered.contains("ParseError: unexpected token"));
    assert!(rendered.contains("main.srt"));
}

#[test]
fn serializable_report_uses_character_offsets_for_utf8_source() {
    let mut sources = SourceRegistry::new();
    let source_id = sources.register("main.srt", "あx");
    let spec = simple_error(
        "TypeError",
        "expected Int, got String",
        Span { start: 1, end: 2 },
        None,
    );

    let report = serializable_report_by_id(&sources, source_id, "typecheck", &spec);
    assert_eq!(report.errors[0].line, 1);
    assert_eq!(report.errors[0].column, 2);
    assert_eq!(report.errors[0].span, [1, 2]);
}

#[test]
fn structured_type_diagnostic_projects_the_same_facts_to_json_and_rendering() {
    let mut sources = SourceRegistry::new();
    let source_id = sources.register("main.srt", "guard::<Option>(value)");
    let input = StructuredDiagnostic {
        reason: TypeDiagnosticReason::ReturnTypeArgumentMismatch,
        origin: DiagnosticOrigin::ReturnTypeArgument { ordinal: 0 },
        data: DiagnosticData::ReturnTypeArgument(ReturnTypeArgumentData {
            callable: "guard".into(),
            ordinal: 0,
            expected_type: "Option".into(),
            actual_type: "List".into(),
        }),
        primary: SourceFact::typed(
            SourceRole::ReturnTypeArgument,
            source_id,
            Span { start: 7, end: 13 },
            "Option",
        ),
        related: vec![SourceFact::typed(
            SourceRole::Value,
            source_id,
            Span { start: 14, end: 19 },
            "List<Int>",
        )],
        remediation: None,
    };

    let spec = structured_type_error_spec(&input);
    let rendered = strip_ansi(&render_error_by_id(&sources, source_id, &spec));
    assert!(rendered.contains("Return type argument 0"));
    assert!(rendered.contains("ReturnTypeArgument: Option"));
    assert!(rendered.contains("Value: List<Int>"));

    let report = serializable_report_by_id(&sources, source_id, "typecheck", &spec);
    let diagnostic = &report.errors[0];
    assert_eq!(
        diagnostic.reason.as_deref(),
        Some("ReturnTypeArgumentMismatch")
    );
    assert_eq!(diagnostic.related.len(), 1);
    assert_eq!(diagnostic.data["ordinal"], 0);
    assert_eq!(diagnostic.expected.as_deref(), Some("Option"));
    assert_eq!(diagnostic.got.as_deref(), Some("List"));
}

#[test]
fn ambiguous_return_type_argument_has_a_distinct_headline() {
    let mut sources = SourceRegistry::new();
    let source_id = sources.register("main.srt", "make()");
    let input = StructuredDiagnostic {
        reason: TypeDiagnosticReason::AmbiguousReturnTypeArgument,
        origin: DiagnosticOrigin::ReturnTypeArgument { ordinal: 0 },
        data: DiagnosticData::ReturnTypeArgument(ReturnTypeArgumentData {
            callable: "make".into(),
            ordinal: 0,
            expected_type: "concrete return type argument".into(),
            actual_type: "List<$A>".into(),
        }),
        primary: SourceFact::typed(
            SourceRole::ReturnTypeArgument,
            source_id,
            Span { start: 0, end: 6 },
            "List<$A>",
        ),
        related: Vec::new(),
        remediation: Some(Remediation::Help {
            text: "Provide an expected result type.".into(),
        }),
    };

    let spec = structured_type_error_spec(&input);
    assert_eq!(
        spec.message,
        "return type arguments for `make` cannot be inferred"
    );
    let report = serializable_report_by_id(&sources, source_id, "typecheck", &spec);
    assert_eq!(
        report.errors[0].reason.as_deref(),
        Some("AmbiguousReturnTypeArgument")
    );
}

#[test]
fn rejected_trait_candidates_preserve_structured_failure_details() {
    let mut sources = SourceRegistry::new();
    let source_id = sources.register("main.srt", "Monad::return(1) |>= {|x| x + 1}");
    let input = StructuredDiagnostic {
        reason: TypeDiagnosticReason::NoApplicableTraitImplementation,
        origin: DiagnosticOrigin::Operator,
        data: DiagnosticData::CandidateSelection(CandidateSelectionData {
            trait_name: "Monad".into(),
            method: "bind".into(),
            failures: vec![CandidateFailureData {
                candidate_type: "Result<Int>".into(),
                detail: "right-hand side returns Int".into(),
            }],
        }),
        primary: SourceFact::typed(
            SourceRole::LeftValue,
            source_id,
            Span { start: 0, end: 34 },
            "Int",
        ),
        related: Vec::new(),
        remediation: Some(Remediation::Candidates {
            items: vec!["Result<Int>".into()],
        }),
    };

    let spec = structured_type_error_spec(&input);
    assert_eq!(
        spec.message,
        "no `Monad` candidate can satisfy `Monad::bind`"
    );
    let report = serializable_report_by_id(&sources, source_id, "typecheck", &spec);
    assert_eq!(
        report.errors[0].reason.as_deref(),
        Some("NoApplicableTraitImplementation")
    );
    assert_eq!(report.errors[0].data["kind"], "CandidateSelection");
    assert_eq!(
        report.errors[0].data["failures"][0]["detail"],
        "right-hand side returns Int"
    );
}

#[test]
fn char_span_to_byte_range_converts_only_at_render_boundary() {
    let range = char_span_to_byte_range("あx", &Span { start: 1, end: 2 });
    assert_eq!(range, "あ".len().."あx".len());
}

#[test]
fn surtr_assert_eq_template_renders_terms_through_ariadne() {
    let source =
        "print(\"あい\")\n\ntest(\"String\") {\n  it(\"bad\") { assert_eq(\"tes\", \"bad\") }\n}\n";
    let call_start = source.find("assert_eq").expect("assert_eq exists");
    let call_end = call_start + "assert_eq(\"tes\", \"bad\")".len();
    let lhs_start = source.find("\"tes\"").expect("lhs exists");
    let lhs_end = lhs_start + "\"tes\"".len();
    let rhs_start = source.find("\"bad\"").expect("rhs exists");
    let rhs_end = rhs_start + "\"bad\"".len();
    let span = |start: usize, end: usize| Span {
        start: source[..start].chars().count(),
        end: source[..end].chars().count(),
    };
    let spec = surtr_assert_eq_error_spec(
        "TestAssertionFailed",
        "expected \"tes\", got \"bad\"",
        span(call_start, call_end),
        span(lhs_start, lhs_end),
        span(rhs_start, rhs_end),
        "\"tes\"",
        "\"bad\"",
    );

    let rendered = strip_ansi(&render_surtr_code_error("main.srt", source, &spec));
    assert!(rendered.contains("TestAssertionFailed: expected \"tes\", got \"bad\""));
    assert!(rendered.contains("main.srt:4:15"));
    assert!(rendered.contains("assert_eq(\"tes\", \"bad\")"));
    assert!(rendered.contains("LHS term: \"tes\""));
    assert!(rendered.contains("RHS term: \"bad\""));
    assert!(rendered.contains("assert_eq failed: expected \"tes\", got \"bad\""));
}

#[test]
fn trait_method_type_list_preserves_path_and_both_origins() {
    use crate::{TraitMethodTypeListData, TypeListRole};
    let input = StructuredDiagnostic {
        reason: TypeDiagnosticReason::TraitMethodTypeListMismatch,
        origin: DiagnosticOrigin::Declaration,
        data: DiagnosticData::TraitMethodTypeList(TraitMethodTypeListData {
            method_name: "Build::build".into(),
            role: TypeListRole::ReturnType,
            ordinal: 0,
            nested_path: vec![0],
            expected_type: "Box<Int>".into(),
            actual_type: "Box<String>".into(),
            expected_count: None,
            actual_count: None,
        }),
        primary: SourceFact::typed(
            SourceRole::Impl,
            SourceId(1),
            Span { start: 10, end: 20 },
            "Box<String>",
        ),
        related: vec![SourceFact::typed(
            SourceRole::Contract,
            SourceId(0),
            Span { start: 3, end: 9 },
            "Box<Int>",
        )],
        remediation: None,
    };
    let spec = structured_type_error_spec(&input);
    assert!(spec.message.contains("Build::build"));
    assert!(spec.message.contains("ReturnType"));
    assert!(spec.message.contains("type argument path: 0"));
    assert_eq!(spec.labels[0].source_id, Some(SourceId(1)));
    assert_eq!(spec.labels[0].message, "Impl: Box<String>");
    assert_eq!(spec.labels[1].source_id, Some(SourceId(0)));
    assert_eq!(spec.labels[1].message, "Contract: Box<Int>");
    let data = input.data.to_json_value();
    assert_eq!(data["kind"], "TraitMethodTypeList");
    assert_eq!(data["role"], "ReturnType");
    assert_eq!(data["ordinal"], 0);
    assert_eq!(data["nested_path"], serde_json::json!([0]));
    assert_eq!(data["expected_type"], "Box<Int>");
    assert_eq!(data["actual_type"], "Box<String>");
}

#[test]
fn trait_method_constraints_preserve_expected_and_actual_sets() {
    use crate::TraitMethodConstraintData;
    let input = StructuredDiagnostic {
        reason: TypeDiagnosticReason::TraitMethodConstraintMismatch,
        origin: DiagnosticOrigin::Declaration,
        data: DiagnosticData::TraitMethodConstraint(TraitMethodConstraintData {
            method_name: "Display::show".into(),
            expected_constraints: vec!["$0: Eq".into()],
            actual_constraints: vec!["$0: Compare".into()],
        }),
        primary: SourceFact::untyped(SourceRole::Impl, SourceId(0), Span { start: 0, end: 1 }),
        related: vec![],
        remediation: None,
    };
    let spec = structured_type_error_spec(&input);
    assert!(spec.message.contains("Display::show"));
    assert!(spec.message.contains("incompatible trait constraints"));
    let data = input.data.to_json_value();
    assert_eq!(data["kind"], "TraitMethodConstraint");
    assert_eq!(data["expected_constraints"], serde_json::json!(["$0: Eq"]));
    assert_eq!(
        data["actual_constraints"],
        serde_json::json!(["$0: Compare"])
    );
}

#[test]
fn trait_method_arity_displays_expected_and_actual_counts() {
    use crate::{TraitMethodTypeListData, TypeListRole};
    let input = StructuredDiagnostic {
        reason: TypeDiagnosticReason::TraitMethodTypeListArityMismatch,
        origin: DiagnosticOrigin::Declaration,
        data: DiagnosticData::TraitMethodTypeList(TraitMethodTypeListData {
            method_name: "Make::make".into(),
            role: TypeListRole::ReturnTypeArgument,
            ordinal: 0,
            nested_path: vec![],
            expected_type: String::new(),
            actual_type: String::new(),
            expected_count: Some(1),
            actual_count: Some(0),
        }),
        primary: SourceFact::untyped(SourceRole::Impl, SourceId(0), Span { start: 0, end: 1 }),
        related: vec![],
        remediation: None,
    };
    let spec = structured_type_error_spec(&input);
    assert!(spec.message.contains("ReturnTypeArgument"));
    assert!(spec.message.contains("expected 1, got 0"));
}
