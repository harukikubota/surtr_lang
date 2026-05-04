#[test]
fn runtime_value_error_spec_splits_literal_safebind_pattern() {
    let source = r#""2" =? "1""#;
    let spec = runtime_value_error_spec(
        source,
        "PatternMismatch",
        "Pattern did not match.\t@@lhs=\"2\"\t@@rhs=\"1\"",
        0,
        1,
        None,
    );

    assert_eq!(
        slice_chars(source, spec.primary_span.start, spec.primary_span.end),
        r#""2""#
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
fn runtime_value_error_spec_splits_head_tail_string_safebind_pattern() {
    let source = r#"[h, ..t] =? """#;
    let spec = runtime_value_error_spec(
        source,
        "PatternMismatch",
        "Pattern did not match.",
        0,
        1,
        None,
    );

    assert!(spec
        .labels
        .iter()
        .any(|label| { label.message == "head-tail list pattern requires a non-empty String" }));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "SafeBind partial match"));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "input source: String"));
}

#[test]
fn runtime_error_spec_splits_builtin_runtime_error() {
    let spec = runtime_error_spec(
        r#"len("oops")"#,
        "len expects List as first argument",
        Span { start: 0, end: 11 },
        &RuntimeDiagnosticContext::default(),
        None,
    );
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "call target"));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "expected rule: List as first argument"));
}

use super::test_support::*;

#[test]
fn runtime_error_spec_splits_builtin_out_of_range_rule() {
    let spec = runtime_error_spec(
        "set_exit_code(999999999999999999999999999999)",
        "set_exit_code out of range for i32: 999999999999999999999999999999",
        Span { start: 0, end: 45 },
        &RuntimeDiagnosticContext::default(),
        None,
    );
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "expected rule: value must fit in i32"));
}

#[test]
fn runtime_error_spec_splits_vm_runtime_error() {
    let spec = runtime_error_spec(
        "bad_jump",
        "JumpIfFalse: expected Bool",
        Span { start: 0, end: 8 },
        &RuntimeDiagnosticContext {
            opcode: Some("JumpIfFalse".into()),
            function: Some("fun#1".into()),
            details: Vec::new(),
        },
        None,
    );
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "opcode: JumpIfFalse"));
    assert!(spec.labels.iter().any(|label| {
        label
            .message
            .starts_with("runtime rule: JumpIfFalse requires Bool")
    }));
}
