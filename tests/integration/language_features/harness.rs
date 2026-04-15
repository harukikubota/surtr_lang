use crate::support;
use eldr::vm::{VmObservation, VmObservationOptions};
use eldr::VM;

pub fn run_surtr(source: &str) -> Result<Vec<String>, String> {
    support::run_project_script("language_features.srt", source)
}

pub fn run_surtr_with_stderr(source: &str) -> Result<(Vec<String>, Vec<String>), String> {
    support::run_project_script_with_stderr("language_features.srt", source)
}

pub fn observe_surtr(source: &str) -> VmObservation {
    let bytecode = support::compile_project_script("language_features.srt", source)
        .expect("compile should work");
    let mut vm = VM::new(bytecode);
    vm.enable_observation(VmObservationOptions::default());
    vm.run().expect("run should succeed");
    vm.observation().expect("observation should exist")
}

pub fn assert_output(source: &str, expected: &[&str]) {
    let output = run_surtr(source).expect("Pipeline failed");
    assert_eq!(output, expected, "\nSource:\n{}\n", source);
}

pub fn assert_compile_error(source: &str, expected_substr: &str) {
    let result = run_surtr(source);
    match result {
        Err(msg) => assert!(
            msg.contains(expected_substr),
            "Expected error containing '{}', got: {}",
            expected_substr,
            msg
        ),
        Ok(output) => panic!("Expected compile error, got output: {:?}", output),
    }
}
