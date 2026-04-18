use crate::support;

#[test]
#[ignore = "pending: stronger closure inference for expected=None without let-generalization"]
fn pending_closure_without_expected_type_can_be_reused_polymorphically() {
    let source = r#"id = {|value| value}
left: Int = id(1)
right: String = id("ok")"#;
    let bytecode = support::compile_script("pending_closure_poly.srt", source)
        .expect("closure should typecheck once expected=None inference is strengthened");
    assert!(!bytecode.opcodes.is_empty());
}

#[test]
#[ignore = "pending: runtime fuel budget should stop non-terminating execution with a stable reason"]
fn pending_runtime_fuel_budget_stops_recursive_loop() {
    let source = r#"def loop() -> Result<()> {
  loop()
}

def main() -> Result<()> {
  loop()
}"#;
    let err = support::run_script("pending_fuel_budget.srt", source)
        .expect_err("fuel-based execution should stop runaway programs once implemented");
    assert!(
        err.contains("fuel") || err.contains("budget") || err.contains("step limit"),
        "future fuel error should mention the stop reason: {err}"
    );
}

#[test]
#[ignore = "pending: host-dependent OOM policy and reporting contract are not fixed yet"]
fn pending_host_dependent_oom_policy_is_surfaced_consistently() {
    let _message = "future OOM policy should decide whether allocation failure is a runtime error, process failure, or host abort";
}

#[test]
#[ignore = "pending: std-module @@builtin and @@test coexistence is not implemented yet"]
fn pending_std_module_builtin_and_test_annotations_can_coexist() {
    let module_source = r#"defmod Bootstrap {
  @@builtin
  type Int

  @@builtin
  def print(a: String) -> Unit

  @@test 1 == 1
  def smoke() -> Boolean { True }
}"#;
    assert!(module_source.contains("@@builtin"));
}

#[test]
#[ignore = "pending: Float NaN/Infinity contract is not fixed yet"]
fn pending_float_non_finite_contract() {
    let source = r#"value = safe_div(0.0, 0.0)
print(to_string(value))"#;
    let (stdout, _stderr) = support::run_script_with_stderr("pending_float_contract.srt", source)
        .expect("float contract should be decidable once specified");
    assert!(
        stdout
            .iter()
            .any(|line| line.contains("NaN") || line.contains("ZeroDivisionError")),
        "future Float contract test should assert one precise behavior"
    );
}
