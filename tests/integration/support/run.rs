use forge::bytecode::Bytecode;

use crate::common::ModuleFixtureCase;

use super::compile::{compile_module_fixture_case, compile_project_script, compile_script};

#[allow(dead_code)]
pub fn run_script(source_name: &str, source: &str) -> Result<Vec<String>, String> {
    let (stdout, _stderr) = run_script_with_stderr(source_name, source)?;
    Ok(stdout)
}

#[allow(dead_code)]
pub fn run_project_script(source_name: &str, source: &str) -> Result<Vec<String>, String> {
    let (stdout, _stderr) = run_project_script_with_stderr(source_name, source)?;
    Ok(stdout)
}

#[allow(dead_code)]
pub fn run_module_fixture_case(case: &ModuleFixtureCase) -> Result<Vec<String>, String> {
    let bytecode = compile_module_fixture_case(case)?;
    let (stdout, _stderr) = run_bytecode_with_stderr(bytecode)?;
    Ok(stdout)
}

#[allow(dead_code)]
pub fn run_script_with_stderr(
    source_name: &str,
    source: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let bytecode = compile_script(source_name, source)?;
    run_bytecode_with_stderr(bytecode)
}

#[allow(dead_code)]
pub fn run_project_script_with_stderr(
    source_name: &str,
    source: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let bytecode = compile_project_script(source_name, source)?;
    run_bytecode_with_stderr(bytecode)
}

#[allow(dead_code)]
pub fn run_project_script_with_input(
    source_name: &str,
    source: &str,
    input: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let bytecode = compile_project_script(source_name, source)?;
    run_bytecode_with_input(bytecode, input)
}

fn run_bytecode_with_stderr(bytecode: Bytecode) -> Result<(Vec<String>, Vec<String>), String> {
    let mut vm = eldr::VM::new(bytecode)
        .with_output_capture()
        .with_error_capture();
    vm.run()
        .map_err(|e| format!("phase=runtime; message={}", e))?;
    Ok((
        vm.output.unwrap_or_default(),
        vm.error_output.unwrap_or_default(),
    ))
}

fn run_bytecode_with_input(
    bytecode: Bytecode,
    input: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let mut vm = eldr::VM::new(bytecode)
        .with_output_capture()
        .with_error_capture()
        .with_stdin_input(input);
    vm.run()
        .map_err(|e| format!("phase=runtime; message={}", e))?;
    Ok((
        vm.output.unwrap_or_default(),
        vm.error_output.unwrap_or_default(),
    ))
}
