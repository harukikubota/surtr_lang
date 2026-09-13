use super::test_support::*;

#[test]
fn typecheck_invariant_helper_produces_a_structured_contract_violation() {
    let spec = typecheck_invariant_spec(SourceId(0), Span { start: 2, end: 5 });
    let structured = spec
        .structured
        .expect("fallback is explicit and structured");

    assert_eq!(
        structured.reason.type_reason(),
        Some(TypeDiagnosticReason::TypecheckInvariantViolation)
    );
    assert_eq!(
        spec.message,
        "typechecker failed to provide structured diagnostic data"
    );
    assert!(spec.help.is_none(), "legacy hint must not be interpreted");
    assert_eq!(spec.primary_span, Span { start: 2, end: 5 });
}

#[test]
fn typecheck_invariant_helper_has_no_message_or_type_inference_fields() {
    let spec = typecheck_invariant_spec(SourceId(7), Span { start: 8, end: 13 });
    let structured = spec.structured.expect("structured contract");
    let DiagnosticData::Policy(data) = structured.data else {
        panic!("producer contract data is required");
    };

    assert_eq!(data.policy, TypePolicy::ProducerContract);
    assert!(data.subject.is_none());
    assert!(data.expected_type.is_none());
    assert!(data.actual_type.is_none());
    assert!(structured.related.is_empty());
}

#[test]
fn typecheck_invariant_display_preserves_text_without_classifying_it() {
    let spec = typecheck_invariant_spec_with_display(
        SourceId(7),
        Span { start: 8, end: 13 },
        "Cannot access field on $A",
        Some("Provide source evidence.".into()),
    );
    let structured = spec.structured.expect("structured producer contract");

    assert_eq!(spec.message, "Cannot access field on $A");
    assert_eq!(spec.help.as_deref(), Some("Provide source evidence."));
    assert_eq!(
        structured.reason.type_reason(),
        Some(TypeDiagnosticReason::TypecheckInvariantViolation)
    );
    let DiagnosticData::Policy(data) = structured.data else {
        panic!("producer contract data is required");
    };
    assert!(data.subject.is_none());
    assert!(data.expected_type.is_none());
    assert!(data.actual_type.is_none());
}

#[test]
fn typecheck_invariant_is_serialized_as_a_reason_with_producer_contract_data() {
    let mut sources = SourceRegistry::new();
    let source_id = sources.register("main.srt", "x = f(1)");
    let spec = typecheck_invariant_spec(source_id, Span { start: 4, end: 5 });

    let report = serializable_report_by_id(&sources, source_id, "typecheck", &spec);
    assert_eq!(
        report.errors[0].reason.as_deref(),
        Some("TypecheckInvariantViolation")
    );
    assert_eq!(report.errors[0].data["policy"], "producer_contract");
}
