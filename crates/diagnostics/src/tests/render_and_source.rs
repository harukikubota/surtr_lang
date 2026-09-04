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
