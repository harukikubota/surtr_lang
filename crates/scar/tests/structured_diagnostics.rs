#![deny(dead_code)]

#[allow(dead_code)]
mod support;
use diagnostics::{DiagnosticOrigin, TypeDiagnosticReason as Reason};
use std::collections::HashSet;

fn error(source: &str) -> scar::error::TypeError {
    support::typecheck(support::resolve_with_builtin_prelude(source)).expect_err(source)
}

macro_rules! diagnostic_case {
    ($case:ident) => {
        (stringify!($case), $case as fn())
    };
}

const DIAGNOSTIC_CASES: &[(&str, fn())] = &[
    diagnostic_case!(operator_and_trait_helper_share_typed_relation),
    diagnostic_case!(branches_keep_reason_and_source_facts),
    diagnostic_case!(ordinary_call_arguments_have_typed_reasons),
    diagnostic_case!(argument_contract_reasons_are_distinct),
    diagnostic_case!(return_and_annotation_reasons_remain_distinct),
    diagnostic_case!(missing_trait_and_capability_reasons_are_structured),
    diagnostic_case!(contextual_operator_relations_have_complete_facts),
    diagnostic_case!(branch_context_contains_all_bodies_and_guards),
    diagnostic_case!(callable_shape_reasons_are_structured),
    diagnostic_case!(operator_origin_does_not_overwrite_nested_call_failures),
    diagnostic_case!(operator_helpers_validate_explicit_return_type_arguments),
    diagnostic_case!(bare_function_names_are_not_function_values),
    diagnostic_case!(contextual_operator_helper_parity_covers_apply_bind_and_compose),
    diagnostic_case!(constructor_arguments_use_common_call_contracts),
    diagnostic_case!(callable_shape_failures_preserve_operator_and_helper_origins),
    diagnostic_case!(nested_argument_relations_do_not_fall_back_to_prose),
];

#[test]
fn diagnostic_case_inventory_has_unique_names_and_functions() {
    let direct_tests = include_str!("structured_diagnostics.rs")
        .lines()
        .filter(|line| *line == "#[test]")
        .count();
    assert_eq!(
        direct_tests, 1,
        "structured diagnostic cases must be registered instead of using standalone #[test]"
    );

    let mut names = HashSet::new();
    let mut functions = HashSet::new();
    for &(name, case) in DIAGNOSTIC_CASES {
        assert!(names.insert(name), "duplicate diagnostic case name: {name}");
        assert!(
            functions.insert(case as usize),
            "duplicate diagnostic case function: {name}"
        );
    }
}

fn run_diagnostic_bucket(bucket: usize) {
    let mut ran = 0usize;
    for (index, &(name, case)) in DIAGNOSTIC_CASES.iter().enumerate() {
        if index % 4 == bucket {
            eprintln!("structured diagnostic case: {name}");
            case();
            ran += 1;
        }
    }
    assert!(ran > 0, "no structured diagnostic cases in bucket {bucket}");
}

macro_rules! diagnostic_bucket {
    ($name:ident, $bucket:expr) => {
        #[test]
        fn $name() {
            run_diagnostic_bucket($bucket);
        }
    };
}

diagnostic_bucket!(structured_diagnostics_bucket_0, 0);
diagnostic_bucket!(structured_diagnostics_bucket_1, 1);
diagnostic_bucket!(structured_diagnostics_bucket_2, 2);
diagnostic_bucket!(structured_diagnostics_bucket_3, 3);

fn operator_and_trait_helper_share_typed_relation() {
    for (operator, helper) in [
        ("1 + \"x\"", "Add::add(1, \"x\")"),
        ("1 == \"x\"", "Eq::eq(1, \"x\")"),
        ("1 ++ \"x\"", "Concat::concat(1, \"x\")"),
    ] {
        let op = error(operator);
        let call = error(helper);
        assert_eq!(op.reason(), Some(Reason::ArgumentTypeMismatch), "{op:?}");
        assert_eq!(call.reason(), op.reason(), "{call:?}");
        let op = op.structured.unwrap();
        let call = call.structured.unwrap();
        assert!(matches!(op.origin, DiagnosticOrigin::Operator { .. }));
        assert_eq!(call.origin, DiagnosticOrigin::TraitCall);
        assert_eq!(
            op.data.to_json_value()["expected_type"],
            "Int",
            "{operator}: {op:?}"
        );
        assert_eq!(op.data.to_json_value()["actual_type"], "String");
        assert_eq!(call.data.to_json_value()["expected_type"], "Int");
        assert_eq!(call.data.to_json_value()["actual_type"], "String");
        assert!(!op.related.is_empty());
    }
}
fn branches_keep_reason_and_source_facts() {
    for (source, reason) in [
        ("if(True, 1, \"x\")", Reason::IfBranchTypeMismatch),
        (
            "match True { True => 1, False => \"x\" }",
            Reason::MatchArmTypeMismatch,
        ),
        (
            "cond { False => 1, True => \"x\" }",
            Reason::CondBranchTypeMismatch,
        ),
    ] {
        let err = error(source);
        assert_eq!(err.reason(), Some(reason), "{err:?}");
        let data = err.structured.unwrap();
        assert_eq!(data.primary.ty.as_deref(), Some("String"));
        assert!(data
            .related
            .iter()
            .any(|fact| fact.ty.as_deref() == Some("Int")));
        assert_eq!(
            &source[data.primary.span.start..data.primary.span.end],
            "\"x\""
        );
    }
}
fn ordinary_call_arguments_have_typed_reasons() {
    let err = error("def take(value: Int) -> Int { value }\ntake(\"x\")");
    assert_eq!(err.reason(), Some(Reason::ArgumentTypeMismatch), "{err:?}");
}

fn argument_contract_reasons_are_distinct() {
    for (call, reason) in [
        ("take()", Reason::ArityMismatch),
        ("take(1, value: 2)", Reason::ArgumentModeMismatch),
        ("take(other: 1)", Reason::UnknownNamedArgument),
        ("take(value: 1, value: 2)", Reason::DuplicateArgument),
        ("pair(first: 1)", Reason::MissingArgument),
    ] {
        let err = error(&format!("def take(value: Int) -> Int {{ value }}\ndef pair(first: Int, second: Int) -> Int {{ first + second }}\n{call}"));
        assert_eq!(err.reason(), Some(reason), "{call}: {err:?}");
    }
}

fn return_and_annotation_reasons_remain_distinct() {
    for (source, reason) in [
        ("def wrong() -> Int { \"x\" }", Reason::ReturnTypeMismatch),
        ("value: Int = \"x\"", Reason::AnnotationTypeMismatch),
    ] {
        let err = error(source);
        assert_eq!(err.reason(), Some(reason), "{err:?}");
    }
}

fn missing_trait_and_capability_reasons_are_structured() {
    for (source, reason) in [
        (
            "def bad(value: $A) -> String { Show::to_string(value) }",
            Reason::MissingGenericBound,
        ),
        (
            "defrecord Empty(value: Int)\nAdd::add(Empty(1), Empty(2))",
            Reason::NoApplicableTraitImplementation,
        ),
    ] {
        let err = error(source);
        assert_eq!(err.reason(), Some(reason), "{err:?}");
    }
}

fn contextual_operator_relations_have_complete_facts() {
    for call in ["[1] |*> &wrong", "Functor::fmap([1], &wrong)"] {
        let source = format!("def wrong(value: String) -> String {{ value }}\n{call}");
        let err = error(&source);
        assert_eq!(
            err.reason(),
            Some(Reason::TypePayloadMismatch),
            "{source}: {err:?}"
        );
        let facts = err.structured.unwrap();
        assert!(
            std::iter::once(&facts.primary)
                .chain(facts.related.iter())
                .any(|fact| fact.ty.as_deref() == Some("List<Int>")),
            "{facts:?}"
        );
    }
    let err = error("[1] |>= {|x: Int| Option::Some(x)}");
    assert_eq!(
        err.reason(),
        Some(Reason::TypeConstructorFamilyMismatch),
        "{err:?}"
    );
}

fn branch_context_contains_all_bodies_and_guards() {
    for source in [
        "cond { False => 1, False => \"x\", True => False }",
        "match 0 { 0 => 1, 1 => \"x\", _ => False }",
    ] {
        let diagnostic = error(source).structured.expect("structured branch error");
        let bodies = std::iter::once(&diagnostic.primary)
            .chain(diagnostic.related.iter())
            .filter(|fact| fact.role == diagnostics::SourceRole::Branch)
            .collect::<Vec<_>>();
        assert_eq!(bodies.len(), 3, "{diagnostic:?}");
        for ordinal in 0..3 {
            assert!(
                bodies.iter().any(|fact| fact.ordinal == Some(ordinal)),
                "{diagnostic:?}"
            );
        }
        let data = diagnostic.data_json();
        assert!(data["left_origin"].is_object());
        assert!(data["right_origin"].is_object());
    }
    let diagnostic = error("if_let(Option::Some(1), Option::Some(value), value, \"x\")");
    assert_eq!(
        diagnostic.reason(),
        Some(Reason::IfBranchTypeMismatch),
        "{diagnostic:?}"
    );
}

fn callable_shape_reasons_are_structured() {
    for (source, expected) in [
        ("value = 1\nvalue(2)", Reason::NotCallable),
        ("[1] |*> {|a: Int, b: Int| a + b}", Reason::CallableShapeMismatch),
        ("[1] |*> {|x: Int| Option::Some(x)}", Reason::CallableShapeMismatch),
        ("def left(x: Int) -> List<Int> { [x] }\ndef right(x: String) -> String { x }\n&left >* &right", Reason::TypePayloadMismatch),
        ("def left(x: Int) -> List<Int> { [x] }\ndef right(x: String) -> List<String> { [x] }\n&left >=> &right", Reason::TypePayloadMismatch),
    ] {
        let err = error(source);
        assert_eq!(err.reason(), Some(expected), "{source}: {err:?}");
    }
}

fn operator_origin_does_not_overwrite_nested_call_failures() {
    let err = error("def take(value: Int) -> Int { value }\n1 + take(\"x\")");
    let diagnostic = err.structured.expect("structured nested argument failure");
    assert_eq!(diagnostic.origin, DiagnosticOrigin::Call, "{diagnostic:?}");
}

fn operator_helpers_validate_explicit_return_type_arguments() {
    for source in [
        "Functor::fmap::<Int, String, Boolean>([1], {|x: Int| x})",
        "Monad::bind::<Int, String, Boolean>([1], {|x: Int| [x]})",
        "Applicative::ap::<Int, String, Boolean>([{|x: Int| x}], [1])",
    ] {
        let err = error(source);
        assert_eq!(
            err.reason(),
            Some(Reason::ReturnTypeArgumentArityMismatch),
            "{source}: {err:?}"
        );
    }
}

fn bare_function_names_are_not_function_values() {
    let err = error("def inc(x: Int) -> Int { x + 1 }\n1 |> inc");
    assert_eq!(err.reason(), Some(Reason::CallableShapeMismatch), "{err:?}");
}

fn contextual_operator_helper_parity_covers_apply_bind_and_compose() {
    for (prefix, operator, helper) in [
        (
            "",
            "[{|x: String| x}] |*| [1]",
            "Applicative::ap([{|x: String| x}], [1])",
        ),
        (
            "",
            "[1] |>= {|x: String| [x]}",
            "Monad::bind([1], {|x: String| [x]})",
        ),
        (
            "",
            "{|x: Int| x} >> {|x: String| x}",
            "Composable::compose({|x: Int| x}, {|x: String| x})",
        ),
        (
            "",
            "{|x: Int| [x]} >* {|x: String| x}",
            "LiftComposable::lift_compose({|x: Int| [x]}, {|x: String| x})",
        ),
        (
            "",
            "{|x: Int| [x]} >=> {|x: String| [x]}",
            "KleisliComposable::kleisli_compose({|x: Int| [x]}, {|x: String| [x]})",
        ),
    ] {
        let op = error(&format!("{prefix}\n{operator}"));
        let call = error(&format!("{prefix}\n{helper}"));
        assert_eq!(
            op.reason(),
            call.reason(),
            "{operator}: {op:?}\n{helper}: {call:?}"
        );
        assert!(op.structured.is_some() && call.structured.is_some());
    }
}

fn constructor_arguments_use_common_call_contracts() {
    for (call, reason) in [
        ("Item()", Reason::ArityMismatch),
        ("Item(1, value: 2)", Reason::ArgumentModeMismatch),
        ("Item(other: 1)", Reason::UnknownNamedArgument),
        ("Item(value: 1, value: 2)", Reason::DuplicateArgument),
        ("Pair(first: 1)", Reason::MissingArgument),
        ("Item(\"x\")", Reason::ArgumentTypeMismatch),
        ("Item(value: \"x\")", Reason::ArgumentTypeMismatch),
        ("Ok()", Reason::ArityMismatch),
        ("Ok(value: 1)", Reason::ArgumentModeMismatch),
    ] {
        let err = error(&format!(
            "defrecord Item(value: Int)\ndefrecord Pair(first: Int, second: Int)\n{call}"
        ));
        assert_eq!(err.reason(), Some(reason), "{call}: {err:?}");
    }
}

fn callable_shape_failures_preserve_operator_and_helper_origins() {
    let operator = error("[1] |*> {|a: Int, b: Int| a}").structured.unwrap();
    let helper = error("Functor::fmap([1], {|a: Int, b: Int| a})")
        .structured
        .unwrap();
    assert_eq!(operator.reason, Reason::CallableShapeMismatch);
    assert_eq!(helper.reason, operator.reason);
    assert!(
        matches!(operator.origin, DiagnosticOrigin::Operator { operator } if operator == "|*>")
    );
    assert_eq!(helper.origin, DiagnosticOrigin::TraitCall);
}

fn nested_argument_relations_do_not_fall_back_to_prose() {
    for source in [
        "def take(value: List<Int>) -> Int { 1 }\ntake([\"x\"])",
        "def take(value: (Int, String)) -> Int { 1 }\ntake((1, False))",
    ] {
        assert_eq!(
            error(source).reason(),
            Some(Reason::ArgumentTypeMismatch),
            "{source}"
        );
    }
    assert_eq!(
        error("def take(value: Int) -> Int { value }\ntake({|x: Int| x})").reason(),
        Some(Reason::CallableShapeMismatch)
    );
}
