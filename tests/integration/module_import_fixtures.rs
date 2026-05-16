use serde_json::Value;
use std::fs;
use std::time::Instant;

use crate::common::{
    assert_compile_error_matches, module_compile_error_fixtures, module_spec_fixtures,
    normalize_text, parse_compile_error_expectation, repo_root, surtr_command, unique_temp_dir,
    ModuleFixtureCase,
};
use crate::support;

fn compile_multi_source_case(
    case: &ModuleFixtureCase,
) -> Result<forge::bytecode::Bytecode, String> {
    support::compile_module_fixture_case(case)
}

fn compile_sources_for_case(case: &ModuleFixtureCase) -> Result<xldr::CompileSources, String> {
    support::compile_sources_for_module_fixture(case)
}

fn check_multi_source_case_phase(case: &ModuleFixtureCase, phase: &str) -> Result<(), String> {
    let compile_sources = compile_sources_for_case(case)?;
    support::check_script_sources_phase(&compile_sources, phase)
}

fn run_multi_source_case(case: &ModuleFixtureCase) -> Result<Vec<String>, String> {
    support::run_module_fixture_case(case)
}

fn run_module_spec_bucket(bucket: usize, bucket_count: usize) {
    let cases = module_spec_fixtures()
        .into_iter()
        .filter(|fixture| {
            support::stable_bucket(&fixture.case.case_dir.to_string_lossy(), bucket_count) == bucket
        })
        .collect::<Vec<_>>();
    assert!(
        !cases.is_empty(),
        "no module spec cases assigned to bucket {} of {}",
        bucket,
        bucket_count
    );

    let timing_enabled = support::test_timing_enabled();
    let _timing_guard = timing_enabled.then(|| {
        support::timing_report_lock()
            .lock()
            .expect("timing report lock poisoned")
    });
    let cache_stats_start = support::cache_stats_snapshot();
    let timing_start = Instant::now();
    let mut slowest = Vec::<support::SlowFixtureTiming>::new();
    let fixture_count = cases.len();

    for fixture in cases {
        let fixture_start = Instant::now();
        let output = run_multi_source_case(&fixture.case).unwrap_or_else(|e| {
            panic!(
                "pipeline failed for {}: {}",
                fixture.case.case_dir.display(),
                e
            )
        });
        let fixture_elapsed = fixture_start.elapsed();
        if timing_enabled {
            slowest.push(support::SlowFixtureTiming {
                path: fixture.case.case_dir.clone(),
                phase: "run".to_string(),
                duration: fixture_elapsed,
            });
        }

        let actual_stdout = output.join("\n");
        assert_eq!(
            normalize_text(&actual_stdout),
            normalize_text(fixture.expected),
            "stdout mismatch for {}",
            fixture.case.case_dir.display()
        );
    }

    if timing_enabled {
        slowest.sort_by(|a, b| {
            b.duration
                .cmp(&a.duration)
                .then_with(|| a.path.cmp(&b.path))
        });
        support::print_timing_report(
            &format!("module pass bucket {bucket}"),
            fixture_count,
            timing_start.elapsed(),
            support::cache_stats_snapshot().saturating_delta_since(&cache_stats_start),
            &slowest,
        );
    }
}

fn run_module_compile_error_bucket(bucket: usize, bucket_count: usize) {
    let cases = module_compile_error_fixtures()
        .into_iter()
        .filter(|fixture| {
            support::stable_bucket(&fixture.case.case_dir.to_string_lossy(), bucket_count) == bucket
        })
        .collect::<Vec<_>>();
    assert!(
        !cases.is_empty(),
        "no module compile-error cases assigned to bucket {} of {}",
        bucket,
        bucket_count
    );

    let timing_enabled = support::test_timing_enabled();
    let _timing_guard = timing_enabled.then(|| {
        support::timing_report_lock()
            .lock()
            .expect("timing report lock poisoned")
    });
    let cache_stats_start = support::cache_stats_snapshot();
    let timing_start = Instant::now();
    let mut slowest = Vec::<support::SlowFixtureTiming>::new();
    let fixture_count = cases.len();

    for fixture in cases {
        let expected = parse_compile_error_expectation(&fixture.error_path);
        let phase_name = expected.phase.as_deref().unwrap_or("compile");
        let fixture_start = Instant::now();
        let result = match expected.phase.as_deref() {
            Some(phase @ ("parse" | "resolve" | "typecheck")) => {
                check_multi_source_case_phase(&fixture.case, phase)
            }
            None | Some(_) => compile_multi_source_case(&fixture.case).map(|_| ()),
        };
        if timing_enabled {
            slowest.push(support::SlowFixtureTiming {
                path: fixture.case.case_dir.clone(),
                phase: phase_name.to_string(),
                duration: fixture_start.elapsed(),
            });
        }
        match result {
            Ok(_) => panic!(
                "expected compile failure but succeeded: {}",
                fixture.case.case_dir.display()
            ),
            Err(msg) => assert_compile_error_matches(&expected, &msg, &fixture.case.case_dir),
        }
    }

    if timing_enabled {
        slowest.sort_by(|a, b| {
            b.duration
                .cmp(&a.duration)
                .then_with(|| a.path.cmp(&b.path))
        });
        support::print_timing_report(
            &format!("module fail bucket {bucket}"),
            fixture_count,
            timing_start.elapsed(),
            support::cache_stats_snapshot().saturating_delta_since(&cache_stats_start),
            &slowest,
        );
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
fn direct_module_file_requires_module_loading_path_instead_of_script_cli_mode() {
    let module_path =
        repo_root().join("tests/fixtures/modules/fail/duplicate_import_all_all/Kernel.srt");
    let output = surtr_command()
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
        !output.status.success(),
        "script-mode CLI should reject direct module file {}\nstdout:\n{}\nstderr:\n{}",
        module_path.display(),
        stdout,
        stderr
    );
    assert!(
        stderr.contains("defmod is not allowed at script top-level"),
        "expected strict script parse failure for {}\nstdout:\n{}\nstderr:\n{}",
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
                == repo_root().join("tests/fixtures/modules/pass/qualified_name_without_import")
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

    let dump = surtr_command()
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
