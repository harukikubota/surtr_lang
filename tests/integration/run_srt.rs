use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use sindr::policy::CompileUnitKind;

use crate::common::{
    compile_error_fixtures, extract_phase_tag, normalize_text, parse_compile_error_expectation,
    spec_fixtures, unique_temp_dir,
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

fn timing_breakdown_enabled() -> bool {
    support::env_flag_enabled(env::var("SURTR_TEST_TIMING").ok().as_deref())
}

fn print_timing_report(
    group: &str,
    fixture_count: usize,
    total: Duration,
    cache: support::CacheStatsSnapshot,
    slowest: Vec<support::SlowFixtureTiming>,
) {
    eprintln!(
        "{}",
        support::format_timing_report(&support::TimingReportInput {
            group,
            fixture_count,
            total,
            cache,
            slowest: &slowest,
        })
    );
}

fn timing_report_lock() -> &'static Mutex<()> {
    static TIMING_REPORT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TIMING_REPORT_LOCK.get_or_init(|| Mutex::new(()))
}

fn semantic_prefix_cache_lock() -> &'static Mutex<()> {
    static SEMANTIC_PREFIX_CACHE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    SEMANTIC_PREFIX_CACHE_LOCK.get_or_init(|| Mutex::new(()))
}

fn remove_semantic_prefix_cache_entry(cache_path: &PathBuf) {
    let _ = fs::remove_file(cache_path);
    if let Some(parent) = cache_path.parent() {
        let is_empty = fs::read_dir(parent)
            .ok()
            .and_then(|mut entries| entries.next().transpose().ok())
            .flatten()
            .is_none();
        if is_empty {
            let _ = fs::remove_dir(parent);
        }
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

    let timing_enabled = timing_breakdown_enabled();
    let _timing_guard = timing_enabled.then(|| {
        timing_report_lock()
            .lock()
            .expect("timing report lock poisoned")
    });
    let cache_stats_start = support::cache_stats_snapshot();
    let timing_start = Instant::now();
    let mut slowest = Vec::<support::SlowFixtureTiming>::new();
    let fixture_count = sources.len();

    for fixture in sources {
        let fixture_start = Instant::now();
        let output = run_surtr(fixture.source).unwrap_or_else(|e| {
            panic!(
                "pipeline failed for {}: {}",
                fixture.source_path.display(),
                e
            )
        });
        let fixture_elapsed = fixture_start.elapsed();
        if timing_enabled {
            slowest.push(support::SlowFixtureTiming {
                path: fixture.source_path.clone(),
                phase: "run".to_string(),
                duration: fixture_elapsed,
            });
        }

        let actual_stdout = output.join("\n");
        assert_eq!(
            normalize_text(&actual_stdout),
            normalize_text(fixture.expected),
            "stdout mismatch for {}",
            fixture.source_path.display()
        );
    }

    if timing_enabled {
        slowest.sort_by(|a, b| {
            b.duration
                .cmp(&a.duration)
                .then_with(|| a.path.cmp(&b.path))
        });
        print_timing_report(
            &format!("script pass bucket {bucket}"),
            fixture_count,
            timing_start.elapsed(),
            support::cache_stats_snapshot().saturating_delta_since(&cache_stats_start),
            slowest,
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
    let _timing_guard = timing_enabled.then(|| {
        timing_report_lock()
            .lock()
            .expect("timing report lock poisoned")
    });
    let cache_stats_start = support::cache_stats_snapshot();
    let timing_start = Instant::now();
    let mut slowest = Vec::<support::SlowFixtureTiming>::new();
    let fixture_count = sources.len();

    for fixture in sources {
        let expected = parse_compile_error_expectation(&fixture.error_path);

        let phase_name = expected.phase.as_deref().unwrap_or("unknown").to_string();
        let fixture_start = Instant::now();
        let result = check_compile_phase(fixture.source, expected.phase.as_deref());
        let fixture_elapsed = fixture_start.elapsed();

        if timing_enabled {
            slowest.push(support::SlowFixtureTiming {
                path: fixture.source_path.clone(),
                phase: phase_name,
                duration: fixture_elapsed,
            });
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
        slowest.sort_by(|a, b| {
            b.duration
                .cmp(&a.duration)
                .then_with(|| a.path.cmp(&b.path))
        });
        print_timing_report(
            &format!("script fail bucket {bucket}"),
            fixture_count,
            timing_start.elapsed(),
            support::cache_stats_snapshot().saturating_delta_since(&cache_stats_start),
            slowest,
        );
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
    let _cache_guard = semantic_prefix_cache_lock()
        .lock()
        .expect("semantic prefix cache lock poisoned");
    let prefix_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-fixture-cache/prefix");

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
    let cache_key = xldr::test_semantic_prefix_cache_key(CompileUnitKind::Script, &compile_sources)
        .expect("semantic prefix key should build");
    let cache_path = prefix_dir.join(format!("{cache_key}.semantic"));
    let _ = fs::remove_file(&cache_path);
    fs::create_dir_all(&prefix_dir).expect("prefix cache dir should be creatable");
    let unrelated_path = prefix_dir.join("preserve-me.semantic");
    fs::write(&unrelated_path, b"existing-prefix-entry")
        .expect("unrelated prefix cache entry should be writable");
    let err = support::check_script_sources_phase(&compile_sources, "typecheck")
        .expect_err("type mismatch should fail in the typecheck phase");

    assert!(
        err.contains("expected Int, got String"),
        "unexpected compile failure: {err}"
    );
    assert!(
        cache_path.is_file(),
        "semantic prefix cache file should exist: {}",
        cache_path.display()
    );
    assert!(
        unrelated_path.is_file(),
        "compile-error path should not clear unrelated prefix cache entries: {}",
        unrelated_path.display()
    );

    remove_semantic_prefix_cache_entry(&cache_path);
}

#[test]
fn semantic_prefix_cache_cleanup_keeps_unrelated_entries() {
    let _cache_guard = semantic_prefix_cache_lock()
        .lock()
        .expect("semantic prefix cache lock poisoned");
    let prefix_dir = unique_temp_dir("surtr_semantic_prefix_cleanup");
    fs::create_dir_all(&prefix_dir).expect("prefix cache dir should be creatable");

    let target_path = prefix_dir.join("target.semantic");
    let unrelated_path = prefix_dir.join("unrelated.semantic");
    fs::write(&target_path, b"target").expect("target cache entry should be writable");
    fs::write(&unrelated_path, b"unrelated").expect("unrelated cache entry should be writable");

    remove_semantic_prefix_cache_entry(&target_path);

    assert!(
        !target_path.exists(),
        "cleanup should remove the targeted semantic prefix entry: {}",
        target_path.display()
    );
    assert!(
        unrelated_path.exists(),
        "cleanup should preserve unrelated semantic prefix entries: {}",
        unrelated_path.display()
    );

    let _ = fs::remove_dir_all(&prefix_dir);
}
