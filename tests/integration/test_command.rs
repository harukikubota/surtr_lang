use std::fs;
use std::path::Path;
use std::process::{Command, Output};

mod common;
use common::{surtr_bin, unique_temp_dir, write_source};

fn run_surtr(temp: &Path, args: &[&str]) -> Output {
    Command::new(surtr_bin())
        .args(args)
        .current_dir(temp)
        .output()
        .expect("failed to run surtr command")
}

fn write_math_module(temp: &Path) {
    write_source(
        &temp.join("lib/math.srt"),
        r#"defmod Math {
  def add(x: Int, y: Int) -> Int { x + y }
}
"#,
    );
}

fn write_math_test(temp: &Path, body: &str) {
    write_source(&temp.join("lib/tests/math.srt"), body);
}

#[test]
fn test_command_runs_named_test_scripts() {
    let temp = unique_temp_dir("surtr_test_command_named_scripts");
    write_math_module(&temp);
    write_math_test(
        &temp,
        r#"import Math;
import Test;

test("Math") {
  describe("add") {
    it("adds two numbers") { assert_eq(3, add(1, 2)) }
    it("adds zero") { assert_eq(7, add(7, 0)) }
  }
}
"#,
    );

    for args in [vec!["test", "math"], vec!["test", "math.srt"]] {
        let output = run_surtr(&temp, &args);
        assert!(
            output.status.success(),
            "test command failed for args {:?}\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("[PASS] Math > add > adds two numbers"));
        assert!(stdout.contains("[PASS] Math > add > adds zero"));
        assert!(stdout.contains("test result: passed=2, failed=0, total=2"));
    }

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_reports_assertion_failures_from_it() {
    let temp = unique_temp_dir("surtr_test_command_assertion_failure");
    write_math_module(&temp);
    write_math_test(
        &temp,
        r#"import Math;
import Test;

test("Math") {
  describe("add") {
    it("rejects wrong sum") { assert_eq(6, add(10, 4)) }
  }
}
"#,
    );

    let output = run_surtr(&temp, &["test", "math"]);
    assert!(
        !output.status.success(),
        "test command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[FAIL] Math > add > rejects wrong sum (lib/tests/math.srt)"));
    assert!(stdout.contains("expected 6, got 14"));
    assert!(stdout.contains("test result: passed=0, failed=1, total=1"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_supports_result_pipeline_assertions() {
    let temp = unique_temp_dir("surtr_test_command_pipeline");
    write_source(
        &temp.join("lib/tests/string_pipeline.srt"),
        r#"import String;
import Test;

test("String") {
  describe("TryFrom") {
    it("parses ints through the assertion pipeline") {
      try_from("1", Int) |>= assert_eq(1)
    }
  }
}
"#,
    );

    let output = run_surtr(&temp, &["test", "string_pipeline"]);
    assert!(
        output.status.success(),
        "test command should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[PASS] String > TryFrom > parses ints through the assertion pipeline"));
    assert!(stdout.contains("test result: passed=1, failed=0, total=1"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_creates_and_reuses_test_cache() {
    let temp = unique_temp_dir("surtr_test_command_cache");
    write_math_module(&temp);
    write_math_test(
        &temp,
        r#"import Math;
import Test;

test("Math") {
  describe("add") {
    it("writes cacheable bytecode") { assert_eq(3, add(1, 2)) }
  }
}
"#,
    );

    let first = run_surtr(&temp, &["test", "math"]);
    assert!(
        first.status.success(),
        "first test run should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );

    let cache_dir = temp.join("target/surtr-test-cache/eldr");
    assert!(cache_dir.is_dir(), "cache dir should exist: {}", cache_dir.display());
    let first_files = fs::read_dir(&cache_dir)
        .expect("cache dir should be readable")
        .map(|entry| entry.expect("cache entry should load").path())
        .collect::<Vec<_>>();
    assert_eq!(first_files.len(), 1, "expected exactly one cached test artifact");

    let second = run_surtr(&temp, &["test", "math"]);
    assert!(
        second.status.success(),
        "second test run should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );

    let second_files = fs::read_dir(&cache_dir)
        .expect("cache dir should be readable")
        .map(|entry| entry.expect("cache entry should load").path())
        .collect::<Vec<_>>();
    assert_eq!(second_files.len(), 1, "cache should reuse the same artifact count");
    assert_eq!(first_files, second_files, "cache key should stay stable across identical runs");

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_reports_missing_test_script() {
    let temp = unique_temp_dir("surtr_test_command_missing");

    let output = run_surtr(&temp, &["test", "missing"]);
    assert!(
        !output.status.success(),
        "missing test script should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("test: failed to read lib/tests/missing.srt for selector `missing`"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_nested_lib_tests_are_ignored_by_normal_script_run() {
    let temp = unique_temp_dir("surtr_test_command_nested_lib_tests_ignored");
    write_source(&temp.join("main.srt"), r#"print("ok")"#);
    write_source(
        &temp.join("lib/tests/bad.srt"),
        r#"this is not valid surtr syntax"#,
    );

    let output = run_surtr(&temp, &["run", "main.srt"]);
    assert!(
        output.status.success(),
        "normal script run should ignore lib/tests fixtures\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "ok");

    let _ = fs::remove_dir_all(temp);
}
