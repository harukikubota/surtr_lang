use sindr::warning::{
    CompilerWarning, PhaseOutput, WarningBuffer, WarningKind, WarningPhase, WarningSpan,
};

fn span(start: usize, end: usize) -> WarningSpan {
    WarningSpan { start, end }
}

#[test]
fn warning_buffer_push_extend_and_take_preserves_order() {
    let first = CompilerWarning::new(
        WarningKind::UnusedVariable,
        WarningPhase::Resolve,
        span(1, 2),
        "unused variable `x`",
        Some("Use `_` if the binding is intentionally ignored.".to_string()),
    );
    let second = CompilerWarning::new(
        WarningKind::UnusedValue,
        WarningPhase::Typecheck,
        span(3, 7),
        "unused value",
        None,
    );

    let mut buffer = WarningBuffer::default();
    assert!(buffer.is_empty());

    buffer.push(first.clone());
    buffer.extend(vec![second.clone()]);

    assert_eq!(buffer.as_slice(), &[first.clone(), second.clone()]);
    assert!(!buffer.is_empty());

    assert_eq!(buffer.take(), vec![first, second]);
    assert!(buffer.is_empty());
}

#[test]
fn phase_output_carries_value_and_warnings() {
    let warning = CompilerWarning::new(
        WarningKind::UnusedTypeParameter,
        WarningPhase::Typecheck,
        span(10, 12),
        "unused type parameter `$A`",
        Some("Remove `$A` from the declaration.".to_string()),
    );

    let output = PhaseOutput::new("typed", vec![warning.clone()]);

    assert_eq!(output.value, "typed");
    assert_eq!(output.warnings, vec![warning]);
}
