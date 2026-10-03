use std::time::Instant;

use crate::common::{
    assert_compile_error_matches, compile_error_fixtures, extract_phase_tag, normalize_text,
    parse_compile_error_expectation, spec_fixtures,
};
use crate::support;

const SCRIPT_FIXTURE_BUCKETS: usize = 8;

fn compile_surtr(source: &str) -> Result<forge::bytecode::Bytecode, String> {
    support::compile_script("fixture.srt", source)
}

fn check_compile_phase(
    source: &str,
    phase: Option<&str>,
) -> Result<(), support::CompilePhaseFailure> {
    match phase {
        Some(phase) => support::check_script_phase("fixture.srt", source, phase),
        None => compile_surtr(source).map(|_| ()).map_err(Into::into),
    }
}

fn run_surtr(source: &str) -> Result<Vec<String>, String> {
    support::run_script("fixture.srt", source)
}

fn run_spec_fixture_bucket(bucket: usize, bucket_count: usize) {
    let sources = spec_fixtures()
        .into_iter()
        .filter(|fixture| {
            support::stable_bucket(&fixture.source_path.to_string_lossy(), bucket_count) == bucket
        })
        .collect::<Vec<_>>();
    assert!(
        !sources.is_empty(),
        "no spec fixtures assigned to bucket {} of {}",
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
        support::print_timing_report(
            &format!("script pass bucket {bucket}"),
            fixture_count,
            timing_start.elapsed(),
            support::cache_stats_snapshot().saturating_delta_since(&cache_stats_start),
            &slowest,
        );
    }
}

fn run_compile_error_fixture_bucket(bucket: usize, bucket_count: usize) {
    let sources = compile_error_fixtures()
        .into_iter()
        .filter(|fixture| {
            support::stable_bucket(&fixture.source_path.to_string_lossy(), bucket_count) == bucket
        })
        .collect::<Vec<_>>();
    assert!(
        !sources.is_empty(),
        "no compile error fixtures assigned to bucket {} of {}",
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
            Err(msg) => assert_compile_error_matches(&expected, &msg, &fixture.source_path),
        }
    }

    if timing_enabled {
        slowest.sort_by(|a, b| {
            b.duration
                .cmp(&a.duration)
                .then_with(|| a.path.cmp(&b.path))
        });
        support::print_timing_report(
            &format!("script fail bucket {bucket}"),
            fixture_count,
            timing_start.elapsed(),
            support::cache_stats_snapshot().saturating_delta_since(&cache_stats_start),
            &slowest,
        );
    }
}

macro_rules! script_fixture_bucket_test {
    ($name:ident, $bucket:expr) => {
        #[test]
        fn $name() {
            run_spec_fixture_bucket($bucket, SCRIPT_FIXTURE_BUCKETS);
            run_compile_error_fixture_bucket($bucket, SCRIPT_FIXTURE_BUCKETS);
        }
    };
}

script_fixture_bucket_test!(script_fixtures_bucket_0, 0);
script_fixture_bucket_test!(script_fixtures_bucket_1, 1);
script_fixture_bucket_test!(script_fixtures_bucket_2, 2);
script_fixture_bucket_test!(script_fixtures_bucket_3, 3);
script_fixture_bucket_test!(script_fixtures_bucket_4, 4);
script_fixture_bucket_test!(script_fixtures_bucket_5, 5);
script_fixture_bucket_test!(script_fixtures_bucket_6, 6);
script_fixture_bucket_test!(script_fixtures_bucket_7, 7);

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
