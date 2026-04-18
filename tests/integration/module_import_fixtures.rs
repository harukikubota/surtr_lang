use std::fs;
use std::process::Command;

use serde_json::Value;

use crate::common::{
    extract_phase_tag, module_compile_error_fixtures, module_spec_fixtures, normalize_text,
    parse_compile_error_expectation, repo_root, surtr_bin, unique_temp_dir, ModuleFixtureCase,
};
use crate::support;

fn compile_multi_source_case(
    case: &ModuleFixtureCase,
) -> Result<forge::bytecode::Bytecode, String> {
    let module_sources = support::collect_module_sources(&case.module_stages)?;
    let compile_sources = support::compose_script_sources(
        &case.entry_path.to_string_lossy(),
        case.entry_source,
        module_sources,
    );

    support::compile_script_sources(&compile_sources)
}

fn run_multi_source_case(case: &ModuleFixtureCase) -> Result<Vec<String>, String> {
    let bytecode = compile_multi_source_case(case)?;
    let mut vm = eldr::VM::new(bytecode).with_output_capture();
    vm.run()
        .map_err(|e| format!("phase=runtime; message={}", e))?;
    Ok(vm.output.unwrap_or_default())
}

fn run_module_spec_bucket(bucket: usize, bucket_count: usize) {
    let cases = module_spec_fixtures()
        .into_iter()
        .enumerate()
        .filter(|(index, _)| index % bucket_count == bucket)
        .map(|(_, fixture)| fixture)
        .collect::<Vec<_>>();
    assert!(
        !cases.is_empty(),
        "no module spec cases assigned to bucket {} of {}",
        bucket,
        bucket_count
    );

    for fixture in cases {
        let output = run_multi_source_case(&fixture.case).unwrap_or_else(|e| {
            panic!(
                "pipeline failed for {}: {}",
                fixture.case.case_dir.display(),
                e
            )
        });
        let actual_stdout = output.join("\n");
        assert_eq!(
            normalize_text(&actual_stdout),
            normalize_text(fixture.expected),
            "stdout mismatch for {}",
            fixture.case.case_dir.display()
        );
    }
}

fn run_module_compile_error_bucket(bucket: usize, bucket_count: usize) {
    let cases = module_compile_error_fixtures()
        .into_iter()
        .enumerate()
        .filter(|(index, _)| index % bucket_count == bucket)
        .map(|(_, fixture)| fixture)
        .collect::<Vec<_>>();
    assert!(
        !cases.is_empty(),
        "no module compile-error cases assigned to bucket {} of {}",
        bucket,
        bucket_count
    );

    for fixture in cases {
        let expected = parse_compile_error_expectation(&fixture.error_path);
        let result = compile_multi_source_case(&fixture.case);
        match result {
            Ok(_) => panic!(
                "expected compile failure but succeeded: {}",
                fixture.case.case_dir.display()
            ),
            Err(msg) => {
                if let Some(expected_phase) = expected.phase.as_deref() {
                    let actual_phase = extract_phase_tag(&msg).unwrap_or("unknown");
                    assert_eq!(
                        actual_phase,
                        expected_phase,
                        "phase mismatch for {}",
                        fixture.case.case_dir.display()
                    );
                }
                for needle in &expected.contains {
                    assert!(
                        msg.contains(needle),
                        "expected '{}' in error for {}\nactual: {}",
                        needle,
                        fixture.case.case_dir.display(),
                        msg
                    );
                }
            }
        }
    }
}

#[test]
fn module_spec_fixtures_bucket_0() {
    run_module_spec_bucket(0, 4);
}

#[test]
fn module_spec_fixtures_bucket_1() {
    run_module_spec_bucket(1, 4);
}

#[test]
fn module_spec_fixtures_bucket_2() {
    run_module_spec_bucket(2, 4);
}

#[test]
fn module_spec_fixtures_bucket_3() {
    run_module_spec_bucket(3, 4);
}

#[test]
fn module_compile_error_fixtures_bucket_0() {
    run_module_compile_error_bucket(0, 4);
}

#[test]
fn module_compile_error_fixtures_bucket_1() {
    run_module_compile_error_bucket(1, 4);
}

#[test]
fn module_compile_error_fixtures_bucket_2() {
    run_module_compile_error_bucket(2, 4);
}

#[test]
fn module_compile_error_fixtures_bucket_3() {
    run_module_compile_error_bucket(3, 4);
}

#[test]
fn direct_module_file_compiles_without_module_resolution_stub_error() {
    let module_path =
        repo_root().join("tests/compile_errors/modules/duplicate_import_all_all/Kernel.srt");
    let output = Command::new(surtr_bin())
        .args([
            "check",
            module_path.to_str().expect("module path must be utf-8"),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run surtr check");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "direct module compile should succeed for {}\nstdout:\n{}\nstderr:\n{}",
        module_path.display(),
        stdout,
        stderr
    );
}

#[test]
fn dump_includes_qualified_function_names_for_module_defined_functions() {
    let fixture = module_spec_fixtures()
        .into_iter()
        .find(|fixture| {
            fixture.case.case_dir
                == repo_root().join("tests/spec/modules/qualified_name_without_import")
        })
        .expect("qualified_name_without_import fixture should exist");
    let bytecode = compile_multi_source_case(&fixture.case).unwrap_or_else(|e| {
        panic!(
            "pipeline failed for {}: {}",
            fixture.case.case_dir.display(),
            e
        )
    });

    let temp = unique_temp_dir("surtr_dump_module_qualified_names");
    let eldr_path = temp.join("module_sample.eldr");
    let bytes = bytecode.encode().expect("encode should succeed");
    fs::write(&eldr_path, bytes).expect("failed to write eldr file");

    let dump = Command::new(surtr_bin())
        .args([
            "dump",
            eldr_path.to_str().expect("eldr path must be utf-8"),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run dump command");
    assert!(
        dump.status.success(),
        "dump failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&dump.stdout),
        String::from_utf8_lossy(&dump.stderr)
    );

    let json: Value = serde_json::from_slice(&dump.stdout).expect("dump output must be valid json");
    let functions = json["bytecode"]["functions"]
        .as_array()
        .expect("bytecode.functions must be an array");
    assert!(
        functions
            .iter()
            .any(|entry| entry["qualified_name"] == "Helper::add"),
        "expected dump to include qualified_name=Helper::add, got:\n{}",
        String::from_utf8_lossy(&dump.stdout)
    );

    let _ = fs::remove_dir_all(temp);
}
