#[allow(dead_code)]
mod support;

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    support::typecheck(support::resolve_with_builtin_prelude(source))
}

const SOURCE: &str =
    "defenumerror Closed { NoneError }\nselected: Result<Int, Closed> = Err(NoneError())\n";
const RELAY: &str = "def relay(source: $M<$A>) -> Result<$A> where $M: MonadExtractor { value =? source; Ok(value) }\n";

#[test]
fn generic_extraction_accepts_a_closed_result_at_named_and_captured_call_sites() {
    for call in [
        "value = relay(selected)",
        "saved: (Result<Int> -> Result<Int>) = &relay\nvalue = saved(selected)",
    ] {
        check(&format!("{RELAY}{SOURCE}{call}"))
            .unwrap_or_else(|error| panic!("{call}: {error:?}"));
    }
}

#[test]
fn explicit_extraction_checks_a_call_receiver_once_before_selecting_its_carrier() {
    for method in ["extract", "extract_error"] {
        let source = format!(
            "defenumerror Closed {{ NoneError }}\ndef source(error: Closed) -> Result<Int, Closed> {{ Err(error) }}\nselected: Closed = NoneError()\nvalue = MonadExtractor::{method}(source(selected))"
        );
        check(&source).unwrap_or_else(|error| panic!("{method}: {error:?}"));
    }
}

#[test]
fn explicit_extraction_preserves_the_nested_success_error_set() {
    let source = "defenumerror Closed { NoneError }\ndef source() -> Result<Result<Int, Closed>, Closed> { Ok(Err(NoneError())) }\ninner =? MonadExtractor::extract(source())\nretained: Result<Int, Closed> = inner";
    check(source)
        .expect("updating the outer carrier proof preserves the success payload's Error set");
}

#[test]
fn aggregate_error_transport_reconnects_the_projected_carrier_contract() {
    for source in [
        "defenumerror Closed { NoneError }\ndef values(error: Closed) -> List<Result<Int, Closed>> { [Err(error)] }\nconverted: List<Result<Int>> = values(Closed::NoneError())\nvalue = match converted { [head, .._] => MonadExtractor::extract(head), [] => Ok(0) }",
        "defenumerror Closed { NoneError }\ndef values(error: Closed) -> List<Result<Result<Int, Closed>, Closed>> { [Ok(Err(error))] }\nconverted: List<Result<Result<Int, Closed>>> = values(Closed::NoneError())\nretained: Result<Int, Closed> = match converted { [head, .._] => { inner =? MonadExtractor::extract(head); inner }, [] => Ok(0) }",
    ] {
        check(source).expect("a projected converted Result uses its common Error carrier contract and preserves its success payload");
    }
}

#[test]
fn generic_extraction_checks_all_constructor_bounds_on_the_same_carrier() {
    for bounds in ["Monad + MonadExtractor", "MonadExtractor + Monad"] {
        let relay = format!("def relay(source: $M<$A>) -> Result<$A> where $M: {bounds} {{ value =? source; Ok(value) }}");
        for selected in [
            "selected: Result<Int> = Err(NoneError())",
            "defenumerror Closed { NoneError }\nselected: Result<Int, Closed> = Err(NoneError())",
        ] {
            let source = format!("{relay}\n{selected}\nvalue = relay(selected)");
            check(&source).unwrap_or_else(|error| panic!("{bounds}: {error:?}"));
        }
    }
    let rejected = "def relay(source: $M<$A>) -> Result<$A> where $M: Alternative + MonadExtractor { value =? source; Ok(value) }";
    check(&format!("{rejected}\n{SOURCE}value = relay(selected)"))
        .expect_err("common Error transport cannot supply an absent second capability");
}
