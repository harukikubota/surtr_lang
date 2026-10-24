#[allow(dead_code)]
mod support;

use diagnostics::{DiagnosticData, ErrorCollectionOperation, SourceRole, TypeDiagnosticReason};
use spire::ast::Span;

fn span_of(source: &str, text: &str) -> Span {
    let byte_start = source
        .rfind(text)
        .expect("test source contains its labeled text");
    let start = source[..byte_start].chars().count();
    Span {
        start,
        end: start + text.chars().count(),
    }
}

fn assert_closed_diagnostic(
    source: &str,
    primary: &str,
    operation: ErrorCollectionOperation,
    concrete_kind: Option<&str>,
) {
    let error = support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect_err("closed EnumError rejects this transport or failure route");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::EnumErrorPolicyViolation),
        "{error:?}"
    );
    let diagnostic = error
        .structured
        .as_ref()
        .expect("producer supplies structured facts");
    assert_eq!(
        diagnostic.primary.span,
        span_of(source, primary),
        "{error:?}"
    );
    assert_eq!(
        diagnostic.primary.role,
        if operation == ErrorCollectionOperation::MonadFail {
            SourceRole::CallTarget
        } else {
            SourceRole::Value
        }
    );
    let declaration = diagnostic
        .related
        .iter()
        .find(|fact| fact.role == SourceRole::Declaration)
        .expect("target collection declaration has a related label");
    assert_eq!(
        declaration.span,
        span_of(source, "defenumerror Closed { NoneError }")
    );
    let DiagnosticData::ErrorCollection(data) = &diagnostic.data else {
        panic!("expected collection facts: {diagnostic:?}");
    };
    assert_eq!(data.operation, operation);
    assert_eq!(data.target, "Closed");
    assert_eq!(data.members, ["Global::NoneError"]);
    assert_eq!(data.concrete_kind.as_deref(), concrete_kind);
    let spec = diagnostics::structured_type_error_spec(diagnostic);
    assert_eq!(spec.labels[0].span, diagnostic.primary.span);
    assert_eq!(spec.labels[1].span, declaration.span);
    assert!(spec.labels[1]
        .message
        .contains("Closed declares [Global::NoneError] and has no Other"));
    let rendered = diagnostics::render_error("enum_error.srt", source, &spec);
    assert!(rendered.contains("Closed declares"), "{rendered}");
    assert!(rendered.contains("Other"), "{rendered}");
}

#[test]
fn enum_error_conversion_labels_point_to_the_value_and_closed_declaration() {
    for (suffix, primary, kind) in [
        ("value: Closed = ZeroDivisionError()", "ZeroDivisionError()", Some("Global::ZeroDivisionError")),
        ("plain: Error = NoneError()\nvalue: Closed = plain", "plain", None),
        ("defenumerror Source { NoneError }\nselected = Source::NoneError()\nvalue: Closed = selected", "selected", None),
    ] {
        let source = format!("defenumerror Closed {{ NoneError }}\n{suffix}");
        assert_closed_diagnostic(&source, primary, ErrorCollectionOperation::Conversion, kind);
    }
}

#[test]
fn closed_monad_fail_labels_point_to_the_call_and_collection_declaration() {
    for target in ["Result<Int, Closed>", "ResultT<Identity, Int, Closed>"] {
        let source = format!(
            "defenumerror Closed {{ NoneError }}\nvalue: {target} = MonadFail::fail(NoneError())"
        );
        assert_closed_diagnostic(
            &source,
            "MonadFail::fail(NoneError())",
            ErrorCollectionOperation::MonadFail,
            None,
        );
    }
}

#[test]
fn enum_error_exhaustiveness_labels_point_to_the_match_and_declaration() {
    let declaration = "defenumerror Open { NoneError, Other(Error) }";
    let inspected = "match value { NoneError => 1 }";
    let source = format!("{declaration}\nvalue = Open::NoneError()\nanswer = {inspected}");
    let error = support::typecheck(support::resolve_with_builtin_prelude(&source)).unwrap_err();
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::NonExhaustiveMatch)
    );
    let diagnostic = error.structured.as_ref().unwrap();
    assert_eq!(diagnostic.primary.span, span_of(&source, inspected));
    let definition = diagnostic
        .related
        .iter()
        .find(|fact| fact.role == SourceRole::Declaration)
        .unwrap();
    assert_eq!(definition.span, span_of(&source, declaration));
    let DiagnosticData::Pattern(data) = &diagnostic.data else {
        panic!("expected missing items");
    };
    assert_eq!(data.details, ["Other"]);
    let spec = diagnostics::structured_type_error_spec(diagnostic);
    assert!(spec
        .labels
        .iter()
        .any(|label| label.span == definition.span));
}

#[test]
fn branch_and_closure_membership_labels_point_to_the_rejected_value() {
    for suffix in [
        "value: Closed = match True { True => NoneError(), False => ZeroDivisionError() }",
        "factory: (-> Closed) = {|| ZeroDivisionError()}",
        "value: Result<Int, Closed> = Err(ZeroDivisionError())",
    ] {
        let source = format!("defenumerror Closed {{ NoneError }}\n{suffix}");
        assert_closed_diagnostic(
            &source,
            "ZeroDivisionError()",
            ErrorCollectionOperation::Conversion,
            Some("Global::ZeroDivisionError"),
        );
    }
}

#[test]
fn nominal_list_mismatch_names_the_distinct_result_t_error_positions() {
    let source = r#"
defenumerror First { NoneError }
defenumerror Second { ZeroDivisionError }
def inconsistent(left: ResultT<Identity, Int, First>, right: ResultT<Identity, Int, Second>) -> Unit {
  values = [left, right]
  ()
}
"#;
    let error = support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect_err("different nominal Error positions cannot share a homogeneous List");
    assert_eq!(
        error.message,
        "expected ResultT<Identity<_>, Int, First>, got ResultT<Identity<_>, Int, Second>",
        "{error:?}"
    );
    assert_eq!(error.span, span_of(source, "right"));
}
