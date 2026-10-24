#[allow(dead_code)]
mod support;

use diagnostics::{DiagnosticData, SourceRole};
use scar::error::TypeError;

const WHOLE_INPUT: &str = "SafeBindWholeInputAfterExtraction";

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, TypeError> {
    support::typecheck(support::resolve_with_builtin_prelude(source))
}

fn whole_input(source: &str, rhs: &str, payload: Option<&str>) -> TypeError {
    let error = check(source).expect_err(source);
    assert_eq!(
        error.reason().map(|reason| reason.as_str()),
        Some(WHOLE_INPUT),
        "{source}\n{error:?}"
    );
    let diagnostic = error
        .structured
        .as_ref()
        .expect("structured input diagnostic");
    let DiagnosticData::SafeBindExtraction(data) = &diagnostic.data else {
        panic!("expected extraction input facts: {error:?}");
    };
    assert_eq!(data.rhs_type, rhs, "{source}");
    if let Some(payload) = payload {
        assert_eq!(data.payload_type, payload, "{source}");
    }
    assert_eq!(
        data.lhs_input, rhs,
        "the input requirement must retain resolved type arguments: {source}"
    );
    assert_eq!(data.extractor_capability, "satisfied", "{source}");
    assert_eq!(diagnostic.primary.role, SourceRole::Pattern, "{source}");
    let rhs_fact = diagnostic
        .related
        .iter()
        .find(|fact| fact.role == SourceRole::Value)
        .expect("original RHS is retained as a related fact");
    let operator = diagnostic
        .related
        .iter()
        .find(|fact| fact.role == SourceRole::Other)
        .expect("the SafeBind operator is retained as a related fact");
    assert_eq!(
        &source[operator.span.start..operator.span.end],
        "=?",
        "{source}"
    );
    assert_eq!(
        &source[rhs_fact.span.start..rhs_fact.span.end],
        source[source.rfind("=?").unwrap() + 2..].trim(),
        "{source}"
    );
    error
}

fn ordinary(source: &str) -> TypeError {
    let error = check(source).expect_err(source);
    assert_ne!(
        error.reason().map(|reason| reason.as_str()),
        Some(WHOLE_INPUT),
        "{source}\n{error:?}"
    );
    error
}

#[test]
fn result_constructors_share_whole_input_reason_and_preserve_unknown_success() {
    let mut messages = Vec::new();
    for pattern in ["Ok(value)", "Err(error)"] {
        let source = format!("target: Result<Int> = Ok(1)\n{pattern} =? target");
        let error = whole_input(&source, "Result<Int>", Some("Int"));
        assert_eq!(&source[error.span.start..error.span.end], pattern);
        messages.push(error.message);
    }
    assert_eq!(
        messages[0], messages[1],
        "Ok and Err must share the same explanation"
    );
    let source = "deferror Oops { \"oops\" }\nErr(error) =? Err(Oops())";
    let error = check(source).expect_err(source);
    assert_eq!(
        error.reason().map(|reason| reason.as_str()),
        Some(WHOLE_INPUT),
        "{error:?}"
    );
}

#[test]
fn standard_and_user_extracting_carriers_use_the_same_diagnostic() {
    for source in [
        "Option::Some(value) =? Option::Some(1)",
        "Option::None =? Option::Some(1)",
    ] {
        whole_input(source, "Option<Int>", Some("Int"));
    }
    for pattern in ["Either::Right(value)", "Either::Left(error)"] {
        whole_input(
            &format!("{pattern} =? Either<Error, Int>::Right(1)"),
            "Either<Error, Int>",
            Some("Int"),
        );
    }
    let definitions = include_str!("../../../lib/tests/monads/support/traced_extraction.srt");
    whole_input(&format!("{definitions}\nTracedExtraction::Continue(value, label) =? TracedExtraction::Continue(1, \"one\")"), "TracedExtraction<Int>", Some("Int"));
}

#[test]
fn named_and_closure_extractors_request_the_original_input() {
    whole_input("impl Result { defextractor whole(target: Result<$T>) -> MatchResult<Result<$T>> { MatchResult::Ok(target) } }\nResult::whole(value) =? Ok(1)", "Result<Int>", Some("Int"));
    whole_input(
        "whole = *{|target: Result<Int>| MatchResult::Ok(target)}\nwhole(value) =? Ok(1)",
        "Result<Int>",
        Some("Int"),
    );
    whole_input("whole = *{|expected: Int, target: Result<Int>| MatchResult::Ok(target)}\nwhole(1, value) =? Ok(1)", "Result<Int>", Some("Int"));
}

#[test]
fn aliases_preserve_the_direct_constructor_input() {
    whole_input("Ok(value) @ whole =? Ok(1)", "Result<Int>", Some("Int"));
    ordinary("Ok(value) @ whole: Result<Int> =? Ok(1)");
}

#[test]
fn callable_extractor_body_and_do_use_the_same_input_diagnostic() {
    for source in [
        "def inspect() -> Result<Int> { Ok(value) =? Ok(1); Ok(value) }",
        "impl Int { defextractor inspect(target: Int) -> MatchResult<Int> { Ok(value) =? Ok(target); MatchResult::Ok(value) } }",
        "result: Result<Int> = do::<Result> { Ok(value) =? Ok(1); Ok(value) }",
    ] {
        let error = check(source).expect_err(source);
        assert_eq!(error.reason().map(|reason| reason.as_str()), Some(WHOLE_INPUT), "{source}\n{error:?}");
        let operator = error.structured.as_ref().unwrap().related.iter()
            .find(|fact| fact.role == SourceRole::Other).unwrap();
        assert_eq!(&source[operator.span.start..operator.span.end], "=?");
    }
}

#[test]
fn explicit_outer_result_preserves_the_original_value_for_patterns() {
    for source in [
        "Ok(value) =? Ok(Ok(1))",
        "Option::Some(value) =? Ok(Option::Some(1))",
        "Either::Right(value) =? Ok(Either<Error, Int>::Right(1))",
        "whole = *{|target: Result<Int>| MatchResult::Ok(target)}\nwhole(value) =? Ok(Ok(1))",
    ] {
        check(source).unwrap_or_else(|error| panic!("{source}\n{error:?}"));
    }
}

#[test]
fn child_and_unrelated_input_mismatches_keep_their_normal_diagnostics() {
    for source in [
        "Ok(value: String) =? Ok(Ok(1))",
        "(Ok(value), tail) =? Ok((1, 2))",
        "whole = *{|target: Result<Int>| MatchResult::Ok(target)}\nwhole(value: String) =? Ok(Ok(1))",
        "whole = *{|target: Result<Int>| MatchResult::Ok(target)}\nwhole(value) =? Option::Some(1)",
        "whole = *{|target: Result<Int>| MatchResult::Ok(target)}\nwhole(value) =? Ok(Ok(\"wrong\"))",
        "Option::Some(value) =? Ok(1)",
        "value: Result<Int> =? Ok(1)",
    ] {
        ordinary(source);
    }
}

#[test]
fn invalid_extractor_prearguments_and_arity_keep_contract_diagnostics() {
    for source in [
        "whole = *{|expected: Int, target: Result<Int>| MatchResult::Ok(target)}\nwhole(\"wrong\", value) =? Ok(1)",
        "whole = *{|target: Result<Int>| MatchResult::Ok(target)}\nwhole(first, second) =? Ok(Ok(1))",
        "whole = *{|target: Result<Int>| MatchResult::Ok(target)}\nwhole() =? Ok(Ok(1))",
        "impl Result { defextractor same(expected: Result<$T>, target: Result<$T>) -> MatchResult<Result<$T>> { MatchResult::Ok(target) } }\nResult::same(Ok(\"wrong\"), value) =? Ok(1)",
        "Ok(first, second) =? Ok(Ok(1))",
    ] {
        ordinary(source);
    }
}

#[test]
fn missing_capability_and_other_pattern_consumers_keep_their_diagnostics() {
    for source in [
        "value =? Identity::new(1)",
        "def relay(target: $F<Int>) -> Result<Int> where $F: Monad { value =? target; Ok(value) }",
        "match 1 { Ok(value) => value, _ => 0 }",
        "whole = *{|target: Result<Int>| MatchResult::Ok(target)}\napply_pattern(1, whole(value))",
    ] {
        ordinary(source);
    }
}
