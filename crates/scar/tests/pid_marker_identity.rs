#[allow(dead_code)]
mod support;

fn worker(name: &str) -> String {
    format!(
        "defgenserver {name} {{\n  meta {{ instance: Worker init_policy: Eager state: Int }}\n  @init def init() -> Result<Int> {{ Ok(0) }}\n  @call def read(state: Int) -> Result<CallResult<Int, Int>> {{ Ok(CallResult::Reply(state, state)) }}\n}}"
    )
}

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, String> {
    support::typecheck_module_source_result(source)
}

#[test]
fn pid_marker_rejects_unknown_names_and_ordinary_types() {
    for (declaration, marker) in [
        ("", "NotAProcess"),
        ("", "Int"),
        ("defenum Ordinary { First }", "Ordinary"),
        ("defmod Ordinary {}", "Ordinary"),
    ] {
        let source = format!(
            "{declaration}\ndefmod Probe {{\n def fake(value: PID<{marker}>) -> PID<{marker}> {{ value }}\n}}"
        );
        let error = check(&source).expect_err("a PID marker must identify a declared capability");
        assert!(error.contains(marker), "{source}: {error}");
    }
}

#[test]
fn pid_generic_cannot_be_returned_as_a_different_process() {
    let source = format!(
        "{}\ndefmod Probe {{\n def forget(value: PID<$P>) -> PID<Other> {{ value }}\n}}",
        worker("Other")
    );
    let error = check(&source).expect_err("declaration-owned PID markers are rigid");
    assert!(error.contains("PID"), "{error}");
}

#[test]
fn pid_generic_repeated_inputs_share_one_marker_binding() {
    let source = format!(
        "{}\n{}\ndefmod Probe {{\n def take(first: PID<$P>, second: PID<$P>) -> PID<$P> {{ first }}\ndef mix(first: PID<Counter>, second: PID<Other>) -> PID<Counter> {{ take(first, second) }}\n}}",
        worker("Counter"),
        worker("Other")
    );
    let error = check(&source).expect_err("one PID marker variable cannot denote two processes");
    assert!(error.contains("PID"), "{error}");
}

#[test]
fn pid_marker_identity_is_preserved_across_namespaces() {
    let source = format!(
        "{}\n{}\ndefmod Probe {{\n def leak(value: PID<Left::Counter>) -> PID<Right::Counter> {{ value }}\n}}",
        worker("Left::Counter"),
        worker("Right::Counter")
    );
    let error = check(&source).expect_err("same short names do not identify the same process");
    assert!(error.contains("PID"), "{error}");
}

#[test]
fn pid_generics_preserve_same_marker_and_standard_handler_capabilities() {
    let source = format!(
        "{}\n{}\ndefmod Probe {{\n def take(first: PID<$P>, second: PID<$P>) -> PID<$P> {{ selected: PID<$P> = first\n selected }}\ndef use_worker(value: PID<Counter>) -> PID<Counter> {{ take(value, value) }}\ndef use_singleton(value: PID<Env>) -> PID<Env> {{ take(value, value) }}\ndef use_output(value: PID<OutHandler>) -> PID<OutHandler> {{ take(value, value) }}\ndef use_input(value: PID<InHandler>) -> PID<InHandler> {{ take(value, value) }}\n}}",
        worker("Counter"),
        worker("Env").replace("instance: Worker", "instance: Singleton")
    );
    check(&source).expect("valid markers and repeated generic inputs must remain valid");
}

#[test]
fn standard_handler_pid_still_has_no_eq_capability() {
    let error = check(
        "defmod Probe {\n def compare(value: PID<OutHandler>) -> Boolean { value == value }\n}",
    )
    .expect_err("handler capability PIDs do not implement Eq");
    assert!(error.contains("Eq"), "{error}");
}

#[test]
fn pid_marker_generic_cannot_be_specialized_to_an_ordinary_value_type() {
    let error = check(
        "defmod Probe {
 def make::<$P>() -> Result<PID<$P>> { Err(NoneError) }
 def invalid() -> Unit { make::<Int>(); () }
}",
    )
    .expect_err("a PID marker type variable cannot bind an ordinary value type");
    assert!(error.contains("Int"), "{error}");
}

#[test]
fn pid_marker_generic_is_distinct_from_the_pid_value_type() {
    let source = format!(
        "{}\ndefmod Probe {{\n def take(pid: PID<$P>, marker: $P) -> PID<$P> {{ pid }}\n def invalid(pid: PID<Counter>) -> PID<Counter> {{ take(pid, pid) }}\n}}",
        worker("Counter")
    );
    let error = check(&source).expect_err("the marker is distinct from a PID value");
    assert!(error.contains("PID"), "{error}");
}

#[test]
fn pid_marker_return_type_argument_preserves_a_declared_process() {
    let source = format!(
        "{}\ndefmod Probe {{\n def make::<$P>() -> Result<PID<$P>> {{ Err(NoneError) }}\n def valid() -> Result<PID<Counter>> {{ make::<Counter>() }}\n}}",
        worker("Counter")
    );
    check(&source).expect("marker return type arguments must resolve the declared process");
}

#[test]
fn trait_candidate_cannot_specialize_a_pid_marker_to_a_value_type() {
    let source = "deftrait Fixture {
 def marker(self: Self, value: $P) -> Result<PID<$P>>
}

impl Fixture for Int {
 def marker(self: Self, value: $P) -> Result<PID<$P>> { Err(NoneError) }
}
defmod Probe {
 def invalid() -> Unit { Fixture::marker(0, 1); () }
}";
    let error = check(source).expect_err("trait candidates must preserve PID marker domains");
    assert!(
        error.contains("Argument type mismatch") && error.contains("Int"),
        "{error}"
    );
}

#[test]
fn local_pid_annotation_cannot_strengthen_an_ordinary_rigid_generic() {
    let source = "defmod Probe {
 def ignore(value: $A) -> Unit {
  pending: Result<PID<$A>> = Err(NoneError)
  ()
 }
}";
    let error = check(source).expect_err("local PID annotations require a declared marker domain");
    assert!(error.contains("PID"), "{error}");
}

#[test]
fn nominal_pid_field_cannot_specialize_its_marker_to_int() {
    let source = "defstruct PidBox<$P> { pid: PID<$P> }
defmod Probe {
 def invalid(value: PidBox<Int>) -> PidBox<Int> { value }
}";
    let error = check(source).expect_err("nominal type arguments must preserve PID marker domains");
    assert!(error.contains("PID") || error.contains("marker"), "{error}");
}

#[test]
fn nominal_pid_marker_arguments_preserve_declared_process_identity() {
    let source = format!(
        "{}\ndefstruct PidBox<$P> {{ pid: PID<$P> }}\nimpl PidBox {{\n def new(pid: PID<$P>) -> PidBox<$P> {{ PidBox {{ pid: pid }} }}\n}}\ndefmod Probe {{\n def valid(pid: PID<Counter>) -> PidBox<Counter> {{ PidBox::new(pid) }}\n}}",
        worker("Counter")
    );
    check(&source).expect("a nominal marker parameter must resolve the declared process");
}
