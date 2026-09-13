#[test]
fn runtime_value_error_spec_uses_literal_safebind_metadata() {
    let source = r#""2" =? "1""#;
    let diagnostic = sindr::runtime::RuntimeErrorDiagnostic::LiteralPatternMismatch {
        lhs: "\"2\"".into(),
        rhs: "\"1\"".into(),
    };
    let spec = runtime_value_error_spec(
        SourceId(0),
        "PatternMismatch",
        "Pattern did not match.",
        0,
        1,
        Some(&diagnostic),
        None,
    );

    assert_eq!(
        slice_chars(source, spec.primary_span.start, spec.primary_span.end),
        r#"""#
    );
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "LHS value: \"2\""));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "SafeBind partial match"));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "RHS value: \"1\""));
    assert_eq!(spec.message, "Pattern did not match.");
}

#[test]
fn runtime_value_error_spec_uses_head_tail_string_metadata() {
    let diagnostic = sindr::runtime::RuntimeErrorDiagnostic::SafeBindPatternFailure {
        rule: "head-tail list pattern requires a non-empty String".into(),
        input_source: Some("String".into()),
    };
    let spec = runtime_value_error_spec(
        SourceId(0),
        "PatternMismatch",
        "Pattern did not match.",
        0,
        1,
        Some(&diagnostic),
        None,
    );

    assert!(spec_notes_text(&spec).contains("head-tail list pattern requires a non-empty String"));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "SafeBind partial match"));
    assert!(spec_notes_text(&spec).contains("input source: String"));
    assert!(!labels_text(&spec).contains("input source:"));
}

#[test]
fn runtime_error_spec_splits_builtin_runtime_error() {
    let spec = runtime_error_spec(
        SourceId(0),
        "len expects List as first argument",
        Span { start: 0, end: 11 },
        &RuntimeDiagnosticContext {
            reason: RuntimeDiagnosticReason::BuiltinContractViolation,
            opcode: Some("CallBuiltin".into()),
            function: Some("len".into()),
            details: vec!["expected rule: List as first argument".into()],
        },
        None,
    );
    assert!(spec_notes_text(&spec).contains("expected rule: List as first argument"));
    assert!(!labels_text(&spec).contains("expected rule:"));
}

use super::test_support::*;

#[test]
fn runtime_error_spec_splits_builtin_out_of_range_rule() {
    let spec = runtime_error_spec(
        SourceId(0),
        "set_exit_code out of range for i32: 999999999999999999999999999999",
        Span { start: 0, end: 45 },
        &RuntimeDiagnosticContext {
            reason: RuntimeDiagnosticReason::BuiltinContractViolation,
            opcode: Some("CallBuiltin".into()),
            function: Some("set_exit_code".into()),
            details: vec!["expected rule: value must fit in i32".into()],
        },
        None,
    );
    assert!(spec_notes_text(&spec).contains("expected rule: value must fit in i32"));
    assert!(!labels_text(&spec).contains("expected rule:"));
}

#[test]
fn runtime_error_spec_splits_vm_runtime_error() {
    let spec = runtime_error_spec(
        SourceId(0),
        "JumpIfFalse: expected Bool",
        Span { start: 0, end: 8 },
        &RuntimeDiagnosticContext {
            reason: RuntimeDiagnosticReason::VmInvariant,
            opcode: Some("JumpIfFalse".into()),
            function: Some("fun#1".into()),
            details: vec!["runtime rule: JumpIfFalse requires Bool".into()],
        },
        None,
    );
    assert!(spec_notes_text(&spec).contains("opcode: JumpIfFalse"));
    assert!(spec_notes_text(&spec).contains("runtime rule: JumpIfFalse requires Bool"));
    assert!(!labels_text(&spec).contains("opcode:"));
    assert!(!labels_text(&spec).contains("runtime rule:"));
}
