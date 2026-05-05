use std::fs;
use std::path::Path;
use std::process::Output;

use crate::common::{surtr_command, unique_temp_dir, write_source};

fn run_surtr(temp: &Path, args: &[&str]) -> Output {
    surtr_command()
        .args(args)
        .current_dir(temp)
        .output()
        .expect("failed to run surtr command")
}

fn run_surtr_with_env(temp: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut command = surtr_command();
    command.args(args).current_dir(temp);
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("failed to run surtr command")
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
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
fn test_command_reports_assertion_failure_source_diagnostic() {
    let temp = unique_temp_dir("surtr_test_command_assertion_source_diagnostic");
    write_math_test(
        &temp,
        r#"import Test;

test("String") {
  describe("repeat") {
    it("bad") { assert_eq("tes", "bad") }
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

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("[FAIL] String > repeat > bad (lib/tests/math.srt)"));
    assert!(stdout.contains("TestAssertionFailed: expected \"tes\", got \"bad\""));
    assert!(stdout.contains("assert_eq(\"tes\", \"bad\")"));
    assert!(stdout.contains("LHS term: \"tes\""));
    assert!(stdout.contains("RHS term: \"bad\""));
    assert!(stdout.contains("assert_eq failed: expected \"tes\", got \"bad\""));
    assert!(stdout.contains("lib/tests/math.srt"));
    assert!(!stdout.contains("note:"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_quiet_suppresses_success_output() {
    let temp = unique_temp_dir("surtr_test_command_quiet_success");
    write_math_module(&temp);
    write_math_test(
        &temp,
        r#"import Math;
import Test;

test("Math") {
  it("adds two numbers") { assert_eq(3, add(1, 2)) }
}
"#,
    );

    let output = run_surtr(&temp, &["test", "--quiet", "math"]);
    assert!(
        output.status.success(),
        "quiet test command should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(String::from_utf8_lossy(&output.stdout), "");

    let output = run_surtr(&temp, &["test", "math", "-q"]);
    assert!(
        output.status.success(),
        "quiet test command should accept trailing flag\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_quiet_keeps_failure_output() {
    let temp = unique_temp_dir("surtr_test_command_quiet_failure");
    write_math_module(&temp);
    write_math_test(
        &temp,
        r#"import Math;
import Test;

test("Math") {
  it("rejects wrong sum") { assert_eq(6, add(10, 4)) }
}
"#,
    );

    let output = run_surtr(&temp, &["test", "-q", "math"]);
    assert!(
        !output.status.success(),
        "quiet failing test command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[FAIL] Math > rejects wrong sum (lib/tests/math.srt)"));
    assert!(stdout.contains("expected 6, got 14"));
    assert!(stdout.contains("test result: passed=0, failed=1, total=1"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_colors_suite_lines_and_summary_when_requested() {
    let temp = unique_temp_dir("surtr_test_command_color");
    write_math_module(&temp);
    write_math_test(
        &temp,
        r#"import Math;
import Test;

test("Math") {
  describe("add") {
    it("adds two numbers") { assert_eq(3, add(1, 2)) }
  }
}
"#,
    );

    let output = run_surtr_with_env(&temp, &["test", "math"], &[("SURTR_TEST_COLOR", "always")]);
    assert!(
        output.status.success(),
        "test command should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\x1b[32m[PASS]\x1b[0m Math > add > adds two numbers"));
    assert!(stdout.contains(
        "test result: \x1b[32mpassed=1\x1b[0m, \x1b[32mfailed=0\x1b[0m, \x1b[36mtotal=1\x1b[0m"
    ));

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
    let prefix_dir = temp.join("target/surtr-test-cache/prefix");
    assert!(
        cache_dir.is_dir(),
        "cache dir should exist: {}",
        cache_dir.display()
    );
    assert!(
        prefix_dir.is_dir(),
        "prefix cache dir should exist: {}",
        prefix_dir.display()
    );
    let first_files = fs::read_dir(&cache_dir)
        .expect("cache dir should be readable")
        .map(|entry| entry.expect("cache entry should load").path())
        .collect::<Vec<_>>();
    let first_prefix_files = fs::read_dir(&prefix_dir)
        .expect("prefix cache dir should be readable")
        .map(|entry| entry.expect("prefix cache entry should load").path())
        .collect::<Vec<_>>();
    assert_eq!(
        first_files.len(),
        1,
        "expected exactly one cached test artifact"
    );
    assert_eq!(
        first_prefix_files.len(),
        1,
        "expected exactly one cached semantic prefix"
    );

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
    let second_prefix_files = fs::read_dir(&prefix_dir)
        .expect("prefix cache dir should be readable")
        .map(|entry| entry.expect("prefix cache entry should load").path())
        .collect::<Vec<_>>();
    assert_eq!(
        second_files.len(),
        1,
        "cache should reuse the same artifact count"
    );
    assert_eq!(
        second_prefix_files.len(),
        1,
        "prefix cache should reuse the same artifact count"
    );
    assert_eq!(
        first_files, second_files,
        "cache key should stay stable across identical runs"
    );
    assert_eq!(
        first_prefix_files, second_prefix_files,
        "prefix cache key should stay stable across identical runs"
    );

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
fn test_command_all_runs_lib_test_scripts() {
    let temp = unique_temp_dir("surtr_test_command_all");
    write_source(
        &temp.join("lib/tests/alpha.srt"),
        r#"import Test;

test("Alpha") {
  it("passes") { assert_eq(1, 1) }
}
"#,
    );
    write_source(
        &temp.join("lib/tests/nested/beta.srt"),
        r#"import Test;

test("Beta") {
  it("passes") { assert_true(True) }
}
"#,
    );
    write_source(
        &temp.join("lib/tests/prelude.srt"),
        r#"this file is intentionally ignored by --all"#,
    );

    let output = run_surtr(&temp, &["test", "--all"]);
    assert!(
        output.status.success(),
        "test --all should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[PASS] Alpha > passes"));
    assert!(stdout.contains("[PASS] Beta > passes"));
    assert!(stdout.contains("test result: passed=2, failed=0, total=2"));

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

#[test]
fn test_command_allows_asserting_captured_stdout_in_surtr_test_code() {
    let temp = unique_temp_dir("surtr_test_command_capture_stdout_assert");
    write_source(
        &temp.join("lib/tests/capture_stdout.srt"),
        r#"import Test;

test("Capture") {
  it("asserts captured print lines") {
    print("first")
    assert_eq(["first"], capture_stdout())

    print("second")
    print("third")
    assert_stdout_eq(["second", "third"])
  }
}
"#,
    );

    let output = run_surtr(&temp, &["test", "capture_stdout"]);
    assert!(
        output.status.success(),
        "test command should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[PASS] Capture > asserts captured print lines"));
    assert!(stdout.contains("test result: passed=1, failed=0, total=1"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_allows_pushing_stdin_from_surtr_test_code() {
    let temp = unique_temp_dir("surtr_test_command_push_stdin");
    write_source(
        &temp.join("lib/tests/stdin.srt"),
        r#"import IO;
import Test;

test("Stdin") {
  it("reads pushed stdin lines through IO") {
    push_stdin("alpha\nbeta\n")
    assert_ok_eq("alpha", IO::get_line(""))
    assert_ok_eq("beta", IO::get_line(""))
  }

  it("reads pushed stdin chars through IO") {
    push_stdin("xy")
    assert_ok_eq("x", IO::get(""))
    assert_ok_eq("y", IO::get(""))
  }
}
"#,
    );

    let output = run_surtr(&temp, &["test", "stdin"]);
    assert!(
        output.status.success(),
        "test command should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[PASS] Stdin > reads pushed stdin lines through IO"));
    assert!(stdout.contains("[PASS] Stdin > reads pushed stdin chars through IO"));
    assert!(stdout.contains("test result: passed=2, failed=0, total=2"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_allows_asserting_captured_stderr_in_surtr_test_code() {
    let temp = unique_temp_dir("surtr_test_command_capture_stderr_assert");
    write_source(
        &temp.join("lib/tests/capture_stderr.srt"),
        r#"import Test;

test("Capture") {
  it("asserts captured eprint fallback lines") {
    value: Result<Int> = Err(NoneError)
    match value {
      Ok(_) => (),
      Err(err) => eprint(err),
    }
    assert_stderr_eq(["Error: NoneError: None Value."])
  }
}
"#,
    );

    let output = run_surtr(&temp, &["test", "capture_stderr"]);
    assert!(
        output.status.success(),
        "test command should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[PASS] Capture > asserts captured eprint fallback lines"));
    assert!(stdout.contains("test result: passed=1, failed=0, total=1"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_isolates_test_io_between_it_blocks() {
    let temp = unique_temp_dir("surtr_test_command_io_isolation");
    write_source(
        &temp.join("lib/tests/io_isolation.srt"),
        r#"import IO;
import Test;

test("IO isolation") {
  it("leaves unread io behind") {
    print("stdout-leak")
    value: Result<Int> = Err(NoneError)
    match value {
      Ok(_) => (),
      Err(err) => eprint(err),
    }
    push_stdin("stale")
  }

  it("starts with fresh io buffers") {
    assert_stdout_eq([])
    assert_stderr_eq([])
    push_stdin("fresh\n")
    assert_ok_eq("fresh", IO::get_line(""))
  }
}
"#,
    );

    let output = run_surtr(&temp, &["test", "io_isolation"]);
    assert!(
        output.status.success(),
        "test command should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[PASS] IO isolation > leaves unread io behind"));
    assert!(stdout.contains("[PASS] IO isolation > starts with fresh io buffers"));
    assert!(stdout.contains("test result: passed=2, failed=0, total=2"));

    let _ = fs::remove_dir_all(temp);
}
