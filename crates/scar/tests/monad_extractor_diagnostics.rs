#[allow(dead_code)]
mod support;

use diagnostics::{DiagnosticData, SourceRole, TypeDiagnosticReason};
use scar::error::TypeError;

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, TypeError> {
    support::typecheck(support::resolve_with_builtin_prelude(source))
}

fn missing(source: &str, payload: &str) {
    let error = check(source).expect_err(source);
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::SafeBindMissingMonadExtractor),
        "{source}\n{error:?}"
    );
    let data = match &error
        .structured
        .as_ref()
        .expect("structured extraction error")
        .data
    {
        DiagnosticData::SafeBindExtraction(data) => data,
        other => panic!("expected extraction data, got {other:?}"),
    };
    assert_eq!(
        data.rhs_type,
        format!("Identity<{payload}>"),
        "{source}\n{error:?}"
    );
    assert_eq!(data.payload_type, payload, "{source}\n{error:?}");
    assert!(!data.lhs_input.is_empty(), "{source}\n{error:?}");
    assert_eq!(data.extractor_capability, "unsatisfied");

    let diagnostic = error.structured.as_ref().unwrap();
    let operator = source.rfind("=?").expect("SafeBind operator");
    let line_start = source[..operator].rfind('\n').map_or(0, |index| index + 1);
    let lhs = source[line_start..operator].trim();
    // Bindings retain their head location; aliases retain the inner location.
    // Structural Patterns retain their complete, already resolved source range.
    let pattern_source = match lhs {
        "value: Int" | "value @ whole" => "value",
        "_: Int" => "_",
        _ => lhs,
    };
    let rhs = source[operator + 2..].trim();
    assert_eq!(diagnostic.primary.role, SourceRole::Value, "{source}");
    assert_eq!(
        &source[diagnostic.primary.span.start..diagnostic.primary.span.end],
        rhs,
        "the primary location must retain the original RHS: {source}"
    );
    let pattern = diagnostic
        .related
        .iter()
        .find(|fact| fact.role == SourceRole::Pattern)
        .expect("the direct LHS input is a related source fact");
    assert_eq!(
        &source[pattern.span.start..pattern.span.end],
        pattern_source,
        "the related Pattern location must exclude the operator and RHS: {source}"
    );
}

#[test]
fn missing_extraction_classifies_total_bindings_and_annotations() {
    for lhs in [
        "value",
        "_",
        "_discard",
        "value @ whole",
        "value: Int",
        "_: Int",
    ] {
        missing(&format!("{lhs} =? Identity::new(10)"), "Int");
    }
    for source in [
        "value: Identity<Int> =? Identity::new(10)",
        "_: Identity<Int> =? Identity::new(10)",
        "value @ whole: Identity<Int> =? Identity::new(10)",
        "value =? 10",
        "value: Int =? 10",
        "hash![] =? hash![\"key\" => 1]",
        "(a, b) =? (1, 2)",
    ] {
        let error = check(source).expect_err(source);
        assert_eq!(
            error.reason(),
            Some(TypeDiagnosticReason::SafeBindTotalPatternNonExtractingRhs),
            "{source}\n{error:?}"
        );
    }
    for source in ["value: String =? Identity::new(10)", "value: String =? 10"] {
        let error = check(source).expect_err(source);
        assert_eq!(
            error.reason(),
            Some(TypeDiagnosticReason::AnnotationTypeMismatch),
            "{source}\n{error:?}"
        );
    }
}

#[test]
fn missing_extraction_classifies_direct_literal_pin_and_structural_inputs() {
    for (source, payload) in [
        ("10 =? Identity::new(10)", "Int"),
        ("expected = 10\n^expected =? Identity::new(10)", "Int"),
        ("[value] =? Identity::new([1])", "List<Int>"),
        ("[head, ..tail] =? Identity::new(\"text\")", "String"),
        ("(a, b) =? Identity::new((1, 2))", "(Int, Int)"),
        ("defrecord User(name: String)\nUser(name) =? Identity::new(User(\"a\"))", "User"),
        ("hash![\"key\" => value] =? Identity::new(hash![\"key\" => 1])", "HashMap<Int>"),
        ("hash![] =? Identity::new(hash![\"key\" => 1])", "HashMap<Int>"),
        ("defenum Choice { Selected(Int), Empty }\nChoice::Selected(value) =? Identity::new(Choice::Selected(1))", "Choice"),
    ] {
        missing(source, payload);
    }
}

#[test]
fn missing_extraction_distinguishes_extractor_payload_and_whole_input() {
    missing(
        "payload = *{|value: Int| MatchResult::Ok(value)}\npayload(selected) =? Identity::new(10)",
        "Int",
    );
    check("whole = *{|value: Identity<Int>| MatchResult::Ok(Identity::run(value))}\nwhole(selected) =? Identity::new(10)").unwrap();
    for source in [
        "[value] =? [1]",
        "[head, ..tail] =? \"text\"",
        "hash![\"key\" => value] =? hash![\"key\" => 1]",
        "defenum Choice { Selected(Int), Empty }\nChoice::Selected(value) =? Choice::Selected(1)",
        "(1, value) =? (1, 2)",
        "defrecord User(name: String)\nUser(\"a\") =? User(\"a\")",
    ] {
        check(source).unwrap_or_else(|error| panic!("{source}\n{error:?}"));
    }
}

#[test]
fn missing_extraction_does_not_replace_child_or_post_extraction_errors() {
    for source in [
        "[value: Int] =? [\"a\"]",
        "(1, value: Int) =? (1, \"a\")",
        "defrecord User(name: String)\nUser(name: value: Int) =? User(\"a\")",
        "hash![\"key\" => value: String] =? hash![\"key\" => 1]",
        "value: Int =? Option::Some(Identity::new(10))",
        "payload = *{|value: Identity<Int>| MatchResult::Ok(Identity::run(value))}\npayload(selected: String) =? Identity::new(10)",
    ] {
        let error = check(source).expect_err(source);
        assert_ne!(error.reason(), Some(TypeDiagnosticReason::SafeBindMissingMonadExtractor), "{source}\n{error:?}");
        assert_ne!(error.reason(), Some(TypeDiagnosticReason::SafeBindTotalPatternNonExtractingRhs), "{source}\n{error:?}");
    }
}

#[test]
fn missing_extraction_preserves_whole_structural_contract_errors() {
    for source in [
        "defrecord User(name: String)\nUser(unknown: _) =? User(\"a\")",
        "defrecord User(name: String, age: Int)\nUser(name: _) =? User(\"a\", 1)",
        "defrecord User(name: String)\nUser(name: _, name: _) =? User(\"a\")",
        "(a, b, c) =? (1, 2)",
        "hash![1 => value] =? hash![\"key\" => 1]",
        "expected = {|value: Int| value}\n^expected =? expected",
    ] {
        let error = check(source).expect_err(source);
        assert_ne!(
            error.reason(),
            Some(TypeDiagnosticReason::SafeBindMissingMonadExtractor),
            "{source}\n{error:?}"
        );
    }
}

#[test]
fn missing_extraction_does_not_require_extractor_bound_for_whole_rigid_partial() {
    let error = check(
        r#"
def accept(target: $F<$T>, expected: $F<$T>) -> Result<Unit>
where $F: Monad + Eq
{
  ^expected =? target
  Ok(())
}
"#,
    )
    .expect_err("the existing Eq capability requirement is still checked");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::MissingTraitCapability),
        "{error:?}"
    );
    let data = match &error.structured.as_ref().unwrap().data {
        DiagnosticData::TraitObligation(data) => data,
        other => panic!("expected Eq obligation, got {other:?}"),
    };
    assert_eq!(
        data.trait_name, "Eq",
        "whole-input matching must not add a MonadExtractor bound"
    );
}

#[test]
fn missing_extraction_guarantees_only_payload_entry_not_pattern_success() {
    for (source, payload) in [
        ("defrecord User(name: String)\nUser(unknown: _) =? Identity::new(User(\"a\"))", "User"),
        ("defrecord User(name: String, age: Int)\nUser(name: _) =? Identity::new(User(\"a\", 1))", "User"),
        ("defrecord User(name: String)\nUser(name: _, name: _) =? Identity::new(User(\"a\"))", "User"),
        ("(a, b, c) =? Identity::new((1, 2))", "(Int, Int)"),
        ("hash![1 => value] =? Identity::new(hash![\"key\" => 1])", "HashMap<Int>"),
        ("expected = {|value: Int| value}\n^expected =? Identity::new(expected)", "(Int -> Int)"),
    ] {
        // No alternate Pattern check is required to prove the extraction entry.
        // These contracts are checked normally after extraction is implemented.
        missing(source, payload);
    }
}
