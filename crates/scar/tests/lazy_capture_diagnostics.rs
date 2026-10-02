#[allow(dead_code)]
mod support;

use diagnostics::{DiagnosticOrigin, TypeDiagnosticReason};
use sindr::policy::RuntimeSourcePolicy;

fn error(source: &str) -> scar::error::TypeError {
    support::typecheck_with_rules(source, RuntimeSourcePolicy::script()).expect_err(source)
}

#[test]
fn bound_lazy_capture_calls_explain_function_and_actual_parameter() {
    for (source, function, condition, ordinal) in [
        ("f = &and(&1, &2)\nf(True, False)", "and", "True", "2"),
        ("f = &or(&2, &1)\ng = f\ng(False, True)", "or", "False", "1"),
        ("f = &if(True, &1, 0)\nf(1)", "if", "selected", "1"),
        ("f = &if_then(&1, &2)\nf(True, ())", "if_then", "True", "2"),
        ("defenum Choice { Item(Int), Empty }\nf = &if_let(&1, Choice::Item(_), &2, 0)\nf(Choice::Empty, 1)", "if_let", "matches", "2"),
        ("defenum Choice { Item(Int), Empty }\nf = &if_let_then(&1, Choice::Item(_), &2)\nf(Choice::Empty, ())", "if_let_then", "matches", "2"),
    ] {
        let error = error(source);
        assert_eq!(error.reason(), Some(TypeDiagnosticReason::ArgumentTypeMismatch), "{source}: {error:?}");
        let hint = error.hint.as_deref().unwrap_or_else(|| panic!("{source}: {error:?}"));
        assert!(hint.contains(function), "{hint}");
        assert!(hint.contains(condition), "{hint}");
        assert!(hint.contains(&format!("parameter {ordinal}")), "{hint}");
        assert!(hint.contains("signature"), "{hint}");
        assert!(hint.contains("{ ||"), "{hint}");
        assert_eq!(error.structured.as_ref().unwrap().remediation_text().as_deref(), Some(hint));
    }
}

#[test]
fn annotations_and_shared_placeholder_conflicts_have_different_guidance() {
    let annotated = error("f: (Boolean, Boolean -> Boolean) = &and(&1, &2)");
    let hint = annotated
        .hint
        .as_deref()
        .expect("normalized capture signature guidance");
    assert!(
        hint.contains("and") && hint.contains("(-> Boolean)") && hint.contains("signature"),
        "{hint}"
    );
    let conflict = error("f = &and(&1, &1)");
    assert!(
        conflict
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("different placeholder") && !hint.contains("{ ||")),
        "{conflict:?}"
    );
    let unknown = error("f = &if(True, &1, &2)");
    assert!(
        unknown
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("if") && hint.contains("annotation")),
        "{unknown:?}"
    );
}

#[test]
fn signatures_and_identity_recapture_keep_exact_capture_provenance() {
    for source in ["f = &and(&1, &2)\ng = &f\ng(True, False)"] {
        let error = error(source);
        assert!(
            error
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("and Lazy capture parameter 2")),
            "{source}: {error:?}"
        );
    }
    let unknown = error("f: (Boolean, Int, Int -> Int) = &if(&1, &2, &3)");
    assert!(
        unknown
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("(Boolean, (-> Int), (-> Int) -> Int)")),
        "{unknown:?}"
    );
    let wrong_return = error("f: (Boolean, (-> Boolean) -> Int) = &and(&1, &2)");
    assert!(
        wrong_return
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("(Boolean, (-> Boolean) -> Boolean)")),
        "{wrong_return:?}"
    );
    let wrong_known = error("f: ((-> String) -> String) = &if(True, &1, 0)");
    assert!(
        wrong_known
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("((-> Int) -> Int)") && !hint.contains("-> String)")),
        "{wrong_known:?}"
    );
    let conflict = error("f = &and(&1 == True, &1)");
    assert!(
        conflict
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("different placeholder")),
        "{conflict:?}"
    );
}

#[test]
fn ordinary_placeholders_in_consumers_and_constructors_keep_conflict_guidance() {
    for source in [
        "f = &and(is_match(&1, True), &1)",
        "f = &and(Result::is_ok(apply_pattern(&1, True)), &1)",
        "defrecord Flag(value: Boolean)\nf = &and(Flag(&1).value, &1)",
        "impl Boolean { defextractor check(flag: Boolean, value: Boolean) -> MatchResult<Unit> { MatchResult::OK(()) } }\nf = &and(is_match(True, Boolean::check(&1)), &1)",
        "defenum Flag<$A> { Item($A), Empty }\nf = &and(is_match(Flag<Boolean>::Item(&1), Flag::Item(True)), &1)",
    ] {
        let error = error(source);
        assert!(error.hint.as_deref().is_some_and(|hint| hint.contains("different placeholder")), "{source}: {error:?}");
    }
}

#[test]
fn extractor_preargument_variables_do_not_make_success_a_binding_branch() {
    let prefix = "impl Boolean { defextractor check(flag: Boolean, value: Boolean) -> MatchResult<Unit> { MatchResult::OK(()) } }\nflag = True\n";
    for (capture, call, function) in [
        (
            "&if_let(True, Boolean::check(flag), &1, 0)",
            "f(1)",
            "if_let",
        ),
        (
            "&if_let_then(True, Boolean::check(flag), &1)",
            "f(())",
            "if_let_then",
        ),
    ] {
        let error = error(&format!("{prefix}f = {capture}\n{call}"));
        assert_eq!(
            error.reason(),
            Some(TypeDiagnosticReason::ArgumentTypeMismatch)
        );
        assert!(
            error.hint.as_deref().is_some_and(|hint| hint
                .contains(&format!("{function} Lazy capture parameter 1"))
                && hint.contains("matches")
                && hint.contains("{ ||")),
            "{error:?}"
        );
    }
}

#[test]
fn ordinary_functions_inner_errors_and_direct_expression_do_not_get_lazy_help() {
    for source in [
        "def f(a: Boolean, b: (-> Boolean)) -> Boolean { b() }\nf(True, False)",
        "def broken(x: String) -> Boolean { True }\nf = &and(&1, &2)\nf(True, broken(1))",
        "def broken(x: String) -> Boolean { True }\nf = &and(True, broken(&1))\nf(1)",
        "defenum Choice { Item(Int), Empty }\nf: (Choice, (-> Int) -> Int) = &if_let(&1, Choice::Item(x), &2, 0)",
    ] {
        let error = error(source);
        assert!(!error.hint.as_deref().is_some_and(|hint| hint.contains("Lazy capture") || hint.contains("{ ||")), "{source}: {error:?}");
    }
    let unknown_ordinary =
        error("def identity(x: $A) -> $A { x }\nf = &if(True, identity(&1), identity(&2))");
    assert!(
        !unknown_ordinary
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("zero-argument")),
        "{unknown_ordinary:?}"
    );
    support::typecheck_with_rules("f = &and(and(True, &1), &1)", RuntimeSourcePolicy::script())
        .expect("nested Lazy uses are compatible");
    let fixed_error = error("f: (Error -> Result<Unit>) = &assert(&1, NoneError)");
    assert!(
        !fixed_error.hint.as_deref().is_some_and(
            |hint| hint.contains("error placeholder") || hint.contains("inside the capture")
        ),
        "{fixed_error:?}"
    );
    let ordinary = error("f = &and(&1, False)\nf(1)");
    assert_eq!(
        ordinary.structured.as_ref().unwrap().origin,
        DiagnosticOrigin::Call
    );
    assert!(
        !ordinary
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("{ ||")),
        "{ordinary:?}"
    );
}

#[test]
fn error_lazy_capture_explains_existing_transport_restriction() {
    for (source, function) in [
        ("f = &assert(&1, &2)\nf(True, 1)", "assert"),
        ("f = &ensure(1, &1, &2)\nf({|x| True}, 1)", "ensure"),
        ("f = &Result::map_err(Ok(1), &1)\nf(1)", "Result::map_err"),
        ("f = &Result::cause(Ok(1), &1)\nf(1)", "Result::cause"),
    ] {
        let error = error(source);
        assert_eq!(
            error.message,
            "Error observer closure can only be passed to Error-observation APIs"
        );
        assert_eq!(error.reason(), None);
        let hint = error.hint.as_deref().expect("Error capture guide");
        assert!(
            hint.contains(function)
                && hint.contains("(-> Error)")
                && hint.contains("signature")
                && hint.contains("inside the capture"),
            "{hint}"
        );
        assert!(!hint.contains("{ ||"), "{hint}");
    }
    let error = error("f: (Boolean, (-> Error) -> Result<Unit>) = &assert(&1, &2)");
    assert!(
        error
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("assert") && hint.contains("inside the capture")),
        "{error:?}"
    );
}
