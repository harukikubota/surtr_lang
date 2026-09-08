#[allow(dead_code)]
mod support;
use diagnostics::{DiagnosticOrigin, TypeDiagnosticReason as Reason};

fn error(source: &str) -> scar::error::TypeError {
    support::typecheck(support::resolve_with_builtin_prelude(source)).expect_err(source)
}

#[test]
fn ensure_predicate_relations_are_structured() {
    for (source, expected, actual) in [
        (
            "ensure(1, {|x: String| True}, NoneError)",
            "(Int -> Boolean)",
            "(String -> Boolean)",
        ),
        (
            "ensure(1, {|x: Int| x}, NoneError)",
            "(Int -> Boolean)",
            "(Int -> Int)",
        ),
    ] {
        let err = error(source);
        assert_eq!(err.reason(), Some(Reason::ArgumentTypeMismatch), "{err:?}");
        let diagnostic = err.structured.unwrap();
        assert_eq!(diagnostic.origin, DiagnosticOrigin::Call);
        let data = diagnostic.data_json();
        assert_eq!(data["expected_type"], expected);
        assert_eq!(data["actual_type"], actual);
        assert_eq!(diagnostic.primary.ty.as_deref(), Some(actual));
    }
}

#[test]
fn closure_unit_return_relation_is_structured() {
    let err = error("def take(f: (Int -> Unit)) -> Unit { f(1) }\ntake({|x: Int| x})");
    assert_eq!(err.reason(), Some(Reason::ReturnTypeMismatch), "{err:?}");
    let diagnostic = err.structured.unwrap();
    assert_eq!(diagnostic.origin, DiagnosticOrigin::Return);
    assert_eq!(diagnostic.data_json()["expected_type"], "Unit");
    assert_eq!(diagnostic.data_json()["actual_type"], "Int");
}

#[test]
fn function_on_callbacks_use_typed_relations() {
    for source in [
        "value: (Int, Int -> Boolean) = Function::on({|a: String, b: String| True}, 1)",
        "value: (Int, Int -> Boolean) = Function::on(1, {|x: Int| x})",
    ] {
        let err = error(source);
        assert_eq!(
            err.reason(),
            Some(Reason::ArgumentTypeMismatch),
            "{source}: {err:?}"
        );
        let diagnostic = err.structured.unwrap();
        assert_eq!(diagnostic.origin, DiagnosticOrigin::Call);
        assert!(diagnostic.data_json()["expected_type"].is_string());
        assert_eq!(diagnostic.data_json()["actual_type"], "Int");
    }
}
