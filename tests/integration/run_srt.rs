use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod support;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("failed to resolve repository root")
}

fn collect_files_with_extension(root: &Path, ext: &str) -> Vec<PathBuf> {
    fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, ext, out);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e == ext)
            {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    walk(root, ext, &mut files);
    files.sort();
    files
}

fn is_multi_source_module_fixture(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized.contains("/tests/spec/modules/")
        || normalized.contains("/tests/compile_errors/modules/")
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

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_string()
}

#[derive(Debug)]
struct CompileErrorExpectation {
    phase: Option<String>,
    contains: Vec<String>,
}

#[derive(Debug)]
struct PhaseTiming {
    phase: String,
    duration: Duration,
}

fn parse_compile_error_expectation(path: &Path) -> CompileErrorExpectation {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    let mut phase = None;
    let mut contains = Vec::new();

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("phase:") {
            phase = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("contains:") {
            contains.push(rest.trim().to_string());
            continue;
        }
        panic!(
            "invalid compile error expectation line in {}: {}",
            path.display(),
            line
        );
    }

    CompileErrorExpectation { phase, contains }
}

fn extract_phase_tag(message: &str) -> Option<&str> {
    message
        .strip_prefix("phase=")
        .and_then(|rest| rest.split_once(';').map(|(phase, _)| phase))
}

fn timing_breakdown_enabled() -> bool {
    matches!(
        env::var("SURTR_TEST_TIMING").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

fn compile_error_sources() -> Vec<PathBuf> {
    let error_root = repo_root().join("tests/compile_errors");
    let sources = collect_files_with_extension(&error_root, "srt")
        .into_iter()
        .filter(|source_path| !is_multi_source_module_fixture(source_path))
        .filter(|source_path| source_path.with_extension("error").exists())
        .collect::<Vec<_>>();
    assert!(
        !sources.is_empty(),
        "no compile error fixtures found under {}",
        error_root.display()
    );
    sources
}

fn spec_sources() -> Vec<PathBuf> {
    let spec_root = repo_root().join("tests/spec");
    let sources = collect_files_with_extension(&spec_root, "srt")
        .into_iter()
        .filter(|source_path| !is_multi_source_module_fixture(source_path))
        .filter(|source_path| source_path.with_extension("expected").exists())
        .collect::<Vec<_>>();
    assert!(
        !sources.is_empty(),
        "no spec fixtures found under {}",
        spec_root.display()
    );
    sources
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
    let sources = spec_sources()
        .into_iter()
        .enumerate()
        .filter(|(index, _)| index % bucket_count == bucket)
        .map(|(_, path)| path)
        .collect::<Vec<_>>();
    assert!(
        !sources.is_empty(),
        "no spec fixtures assigned to bucket {} of {}",
        bucket,
        bucket_count
    );

    for source_path in sources {
        let expected_path = source_path.with_extension("expected");
        assert!(
            expected_path.exists(),
            "missing .expected for {}",
            source_path.display()
        );

        let source = fs::read_to_string(&source_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", source_path.display(), e));
        let expected = fs::read_to_string(&expected_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", expected_path.display(), e));

        let output = run_surtr(&source)
            .unwrap_or_else(|e| panic!("pipeline failed for {}: {}", source_path.display(), e));

        let actual_stdout = output.join("\n");
        assert_eq!(
            normalize_text(&actual_stdout),
            normalize_text(&expected),
            "stdout mismatch for {}",
            source_path.display()
        );
    }
}

#[test]
fn spec_fixtures_bucket_0() {
    run_spec_fixture_bucket(0, 4);
}

#[test]
fn spec_fixtures_bucket_1() {
    run_spec_fixture_bucket(1, 4);
}

#[test]
fn spec_fixtures_bucket_2() {
    run_spec_fixture_bucket(2, 4);
}

#[test]
fn spec_fixtures_bucket_3() {
    run_spec_fixture_bucket(3, 4);
}

fn run_compile_error_fixture_bucket(bucket: usize, bucket_count: usize) {
    let sources = compile_error_sources()
        .into_iter()
        .enumerate()
        .filter(|(index, _)| index % bucket_count == bucket)
        .map(|(_, path)| path)
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

    for source_path in sources {
        let error_path = source_path.with_extension("error");
        assert!(
            error_path.exists(),
            "missing .error for {}",
            source_path.display()
        );

        let source = fs::read_to_string(&source_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", source_path.display(), e));
        let expected = parse_compile_error_expectation(&error_path);

        let phase_name = expected.phase.as_deref().unwrap_or("unknown").to_string();
        let fixture_start = Instant::now();
        let result = check_compile_phase(&source, expected.phase.as_deref());
        let fixture_elapsed = fixture_start.elapsed();

        if timing_enabled {
            *phase_totals.entry(phase_name.clone()).or_default() += fixture_elapsed;
            slowest.push((source_path.clone(), phase_name, fixture_elapsed));
        }

        match result {
            Ok(_) => panic!(
                "expected compile failure but succeeded: {}",
                source_path.display()
            ),
            Err(msg) => {
                if let Some(expected_phase) = expected.phase.as_deref() {
                    let actual_phase = extract_phase_tag(&msg).unwrap_or("unknown");
                    assert_eq!(
                        actual_phase,
                        expected_phase,
                        "phase mismatch for {}",
                        source_path.display()
                    );
                }
                for needle in &expected.contains {
                    assert!(
                        msg.contains(needle),
                        "expected '{}' in error for {}\nactual: {}",
                        needle,
                        source_path.display(),
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
    run_compile_error_fixture_bucket(0, 4);
}

#[test]
fn compile_error_fixtures_bucket_1() {
    run_compile_error_fixture_bucket(1, 4);
}

#[test]
fn compile_error_fixtures_bucket_2() {
    run_compile_error_fixture_bucket(2, 4);
}

#[test]
fn compile_error_fixtures_bucket_3() {
    run_compile_error_fixture_bucket(3, 4);
}
