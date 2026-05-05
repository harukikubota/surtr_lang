use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::common::{
    compile_error_fixtures, extract_phase_tag, normalize_text, parse_compile_error_expectation,
    spec_fixtures,
};
use crate::support;

const SPEC_FIXTURE_BUCKETS: usize = 4;
const COMPILE_ERROR_FIXTURE_BUCKETS: usize = 4;

fn stable_bucket(key: &str, bucket_count: usize) -> usize {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    (hash as usize) % bucket_count
}

fn compile_surtr(source: &str) -> Result<forge::bytecode::Bytecode, String> {
    support::compile_script("fixture.srt", source)
}

fn check_compile_phase(source: &str, phase: Option<&str>) -> Result<(), String> {
    match phase {
        Some(phase) => support::check_script_phase("fixture.srt", source, phase),
        None => compile_surtr(source).map(|_| ()),
    }
}

fn run_surtr(source: &str) -> Result<Vec<String>, String> {
    support::run_script("fixture.srt", source)
}

#[derive(Debug)]
struct PhaseTiming {
    phase: String,
    duration: Duration,
}

fn timing_breakdown_enabled() -> bool {
    matches!(
        env::var("SURTR_TEST_TIMING").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

fn print_timing_breakdown(
    total: Duration,
    phase_totals: &[PhaseTiming],
    slowest: &[(PathBuf, String, Duration)],
) {
    eprintln!("compile_error timing total: {:.3}s", total.as_secs_f64());

    for phase in phase_totals {
        eprintln!(
            "phase {} total: {:.3}s",
            phase.phase,
            phase.duration.as_secs_f64()
        );
    }

    for (path, phase, duration) in slowest.iter().take(10) {
        eprintln!(
            "slow fixture {:.3}s [{}] {}",
            duration.as_secs_f64(),
            phase,
            path.display()
        );
    }
}

fn run_spec_fixture_bucket(bucket: usize, bucket_count: usize) {
    let sources = spec_fixtures()
        .into_iter()
        .filter(|fixture| {
            stable_bucket(&fixture.source_path.to_string_lossy(), bucket_count) == bucket
        })
        .collect::<Vec<_>>();
    assert!(
        !sources.is_empty(),
        "no spec fixtures assigned to bucket {} of {}",
        bucket,
        bucket_count
    );

    for fixture in sources {
        let output = run_surtr(fixture.source).unwrap_or_else(|e| {
            panic!(
                "pipeline failed for {}: {}",
                fixture.source_path.display(),
                e
            )
        });

        let actual_stdout = output.join("\n");
        assert_eq!(
            normalize_text(&actual_stdout),
            normalize_text(fixture.expected),
            "stdout mismatch for {}",
            fixture.source_path.display()
        );
    }
}

#[test]
fn spec_fixtures_bucket_0() {
    run_spec_fixture_bucket(0, SPEC_FIXTURE_BUCKETS);
}

#[test]
fn spec_fixtures_bucket_1() {
    run_spec_fixture_bucket(1, SPEC_FIXTURE_BUCKETS);
}

#[test]
fn spec_fixtures_bucket_2() {
    run_spec_fixture_bucket(2, SPEC_FIXTURE_BUCKETS);
}

#[test]
fn spec_fixtures_bucket_3() {
    run_spec_fixture_bucket(3, SPEC_FIXTURE_BUCKETS);
}

fn run_compile_error_fixture_bucket(bucket: usize, bucket_count: usize) {
    let sources = compile_error_fixtures()
        .into_iter()
        .filter(|fixture| {
            stable_bucket(&fixture.source_path.to_string_lossy(), bucket_count) == bucket
        })
        .collect::<Vec<_>>();
    assert!(
        !sources.is_empty(),
        "no compile error fixtures assigned to bucket {} of {}",
        bucket,
        bucket_count
    );

    let timing_enabled = timing_breakdown_enabled();
    let timing_start = Instant::now();
    let mut phase_totals = HashMap::<String, Duration>::new();
    let mut slowest = Vec::<(PathBuf, String, Duration)>::new();

    for fixture in sources {
        let expected = parse_compile_error_expectation(&fixture.error_path);

        let phase_name = expected.phase.as_deref().unwrap_or("unknown").to_string();
        let fixture_start = Instant::now();
        let result = check_compile_phase(fixture.source, expected.phase.as_deref());
        let fixture_elapsed = fixture_start.elapsed();

        if timing_enabled {
            *phase_totals.entry(phase_name.clone()).or_default() += fixture_elapsed;
            slowest.push((fixture.source_path.clone(), phase_name, fixture_elapsed));
        }

        match result {
            Ok(_) => panic!(
                "expected compile failure but succeeded: {}",
                fixture.source_path.display()
            ),
            Err(msg) => {
                if let Some(expected_phase) = expected.phase.as_deref() {
                    let actual_phase = extract_phase_tag(&msg).unwrap_or("unknown");
                    assert_eq!(
                        actual_phase,
                        expected_phase,
                        "phase mismatch for {}",
                        fixture.source_path.display()
                    );
                }
                for needle in &expected.contains {
                    assert!(
                        msg.contains(needle),
                        "expected '{}' in error for {}\nactual: {}",
                        needle,
                        fixture.source_path.display(),
                        msg
                    );
                }
            }
        }
    }

    if timing_enabled {
        let mut phase_totals = phase_totals
            .into_iter()
            .map(|(phase, duration)| PhaseTiming { phase, duration })
            .collect::<Vec<_>>();
        phase_totals.sort_by(|a, b| {
            b.duration
                .cmp(&a.duration)
                .then_with(|| a.phase.cmp(&b.phase))
        });

        slowest.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        print_timing_breakdown(timing_start.elapsed(), &phase_totals, &slowest);
    }
}

#[test]
fn compile_error_fixtures_bucket_0() {
    run_compile_error_fixture_bucket(0, COMPILE_ERROR_FIXTURE_BUCKETS);
}

#[test]
fn compile_error_fixtures_bucket_1() {
    run_compile_error_fixture_bucket(1, COMPILE_ERROR_FIXTURE_BUCKETS);
}

#[test]
fn compile_error_fixtures_bucket_2() {
    run_compile_error_fixture_bucket(2, COMPILE_ERROR_FIXTURE_BUCKETS);
}

#[test]
fn compile_error_fixtures_bucket_3() {
    run_compile_error_fixture_bucket(3, COMPILE_ERROR_FIXTURE_BUCKETS);
}

#[test]
fn script_mode_rejects_definition_after_top_level_expression_without_compatibility_fallback() {
    let err = support::compile_script(
        "fixture.srt",
        r#"print("start")

def helper() -> Unit { () }"#,
    )
    .expect_err("legacy script ordering should fail under strict script parsing");

    assert_eq!(extract_phase_tag(&err), Some("parse"));
    assert!(
        err.contains("top-level definition cannot appear after top-level expression"),
        "unexpected error: {err}"
    );
}

#[test]
fn compile_error_phase_primes_semantic_prefix_cache_without_final_bytecode_cache() {
    let prefix_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-fixture-cache/prefix");
    let _ = fs::remove_dir_all(&prefix_dir);

    let module_sources = support::collect_module_sources(&[vec![xldr::ModuleInput {
        file_name: "Helper.srt".into(),
        source: "defmod Helper {\n  def id(x: Int) -> Int { x }\n}\n".into(),
        module_path: "Helper".into(),
    }]])
    .expect("module sources should load");
    let compile_sources = support::compose_script_sources(
        "fixture.srt",
        "import Helper;\nbad: Int = \"bad type\"\n",
        module_sources,
    );
    let err = support::check_script_sources_phase(&compile_sources, "typecheck")
        .expect_err("type mismatch should fail in the typecheck phase");

    assert!(
        err.contains("expected Int, got String"),
        "unexpected compile failure: {err}"
    );
    assert!(
        prefix_dir.is_dir(),
        "semantic prefix cache dir should exist: {}",
        prefix_dir.display()
    );
    let prefix_files = fs::read_dir(&prefix_dir)
        .expect("prefix cache dir should be readable")
        .map(|entry| entry.expect("prefix cache entry should load").path())
        .collect::<Vec<_>>();
    assert!(
        !prefix_files.is_empty(),
        "expected at least one cached semantic prefix after compile-error path"
    );

    let _ = fs::remove_dir_all(&prefix_dir);
}
