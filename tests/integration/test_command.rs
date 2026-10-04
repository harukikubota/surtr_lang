use std::fs;
use std::path::Path;
use std::process::Output;

use crate::common::{repo_root, surtr_command, unique_temp_dir, write_source};

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

fn contains_numeric_tyvar(input: &str) -> bool {
    let chars: Vec<char> = input.chars().collect();
    for idx in 0..chars.len() {
        if chars[idx] != '$' {
            continue;
        }
        if chars.get(idx + 1).is_some_and(|next| next.is_ascii_digit()) {
            return true;
        }
    }
    false
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
        assert_eq!(
            output.status.code(),
            Some(0),
            "successful test command should exit with status 0"
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("[PASS] Math > add > adds two numbers"));
        assert!(stdout.contains("[PASS] Math > add > adds zero"));
        assert!(stdout.contains("test result: passed=2, failed=0, total=2"));
    }

    let _ = fs::remove_dir_all(temp);
}

#[cfg(unix)]
#[test]
fn test_command_rejects_test_file_symlink_outside_lib_tests() {
    use std::os::unix::fs::symlink;

    let temp = unique_temp_dir("surtr_test_command_symlink_escape");
    write_source(&temp.join("private/secret.srt"), "not valid Surtr source");
    fs::create_dir_all(temp.join("lib/tests")).expect("create test directory");
    symlink(
        "../../private/secret.srt",
        temp.join("lib/tests/escape.srt"),
    )
    .expect("create escaping test symlink");

    let output = run_surtr(&temp, &["test", "escape"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("test: selector must stay within lib/tests"),
        "unexpected diagnostic: {stderr}"
    );

    write_source(
        &temp.join("lib/tests/inside.srt"),
        "import Test;\ntest(\"Inside\") { describe(\"link\") { it(\"passes\") { assert_eq(1, 1) } } }\n",
    );
    symlink("inside.srt", temp.join("lib/tests/alias.srt"))
        .expect("create test symlink within lib/tests");
    let output = run_surtr(&temp, &["test", "alias"]);
    assert!(
        output.status.success(),
        "symlink within lib/tests should run\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_short_circuits_multiple_assertions_and_continues_next_it() {
    let temp = unique_temp_dir("surtr_test_statement_question_short_circuit");
    for (body, kind) in [
        (
            "assert_eq(1, 2)?\n    print(\"after-failure\")\n    assert_eq(3, 3)",
            "question",
        ),
        (
            "do::<Result> {\n      assert_eq(1, 2)\n      print(\"after-failure\")\n      assert_eq(3, 3)\n    }",
            "do",
        ),
    ] {
        write_math_test(
            &temp,
            &format!(
                r#"import Test;
test("Sequencing") {{
  it("first failure") {{
    print("before-failure")
    {body}
  }}
  it("next it") {{
    assert_stdout_eq([])?
    print("next-only")
    assert_stdout_eq(["next-only"])
  }}
}}
"#
            ),
        );
        let output = run_surtr(&temp, &["test", "math"]);
        let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{kind}: {stdout}\n{stderr}");
        assert!(stdout.contains("[FAIL] Sequencing > first failure"), "{kind}: {stdout}\n{stderr}");
        assert!(stdout.contains("expected 1, got 2"), "{kind}: {stdout}");
        assert!(stdout.contains("[PASS] Sequencing > next it"), "{kind}: {stdout}\n{stderr}");
        assert!(stdout.contains("test result: passed=1, failed=1, total=2"), "{kind}: {stdout}");
        assert!(!stdout.contains("after-failure"), "{kind}: {stdout}");
    }
    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_statement_question_requires_explicit_result_tail() {
    let temp = unique_temp_dir("surtr_test_statement_question_unit_tail");
    write_math_test(
        &temp,
        r#"import Test;
test("Unit tail") {
  it("requires Result") { assert_true(True)? }
}
"#,
    );
    let output = run_surtr(&temp, &["test", "math"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "Unit tail must not become Ok implicitly"
    );
    assert!(
        stderr.contains("TypeError")
            && stderr.contains("expected (-> Result<Unit>), got (-> Unit)"),
        "{stderr}"
    );
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
    assert_eq!(
        output.status.code(),
        Some(1),
        "test command failure should exit with status 1"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[FAIL] Math > add > rejects wrong sum (lib/tests/math.srt)"));
    assert!(stdout.contains("expected 6, got 14"));
    assert!(stdout.contains("test result: passed=0, failed=1, total=1"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_assertion_captions_follow_captures_and_included_helpers() {
    let temp = unique_temp_dir("surtr_test_assertion_call_boundaries");
    let cases = [
        (
            "check: (Boolean -> Result<()>) = &Test::assert_true\n    check(False)",
            "check(False)",
            "assert_true",
        ),
        (
            "check: (Int -> Result<()>) = &Test::assert_eq(1, &1)\n    check(2)",
            "check(2)",
            "assert_eq",
        ),
        (
            "check: (Int, Int -> Result<()>) = &Test::assert_eq(&2, &1)\n    check(1, 2)",
            "check(1, 2)",
            "assert_eq",
        ),
        (
            "assert_eq(actual: \"実際\", expected: \"期待\")",
            "assert_eq(actual: \"実際\", expected: \"期待\")",
            "assert_eq",
        ),
        (
            "do::<Result> {\n      assert_eq(0, 0)\n      assert_eq(1, 2)\n    }",
            "assert_eq(1, 2)",
            "assert_eq",
        ),
    ];
    for (body, call, assertion) in cases {
        let source = format!("import Test;\ntest(\"boundary\") {{\n  it(\"same\") {{ assert_eq(0, 0) }}\n  it(\"same\") {{\n    {body}\n  }}\n}}\n");
        write_math_test(&temp, &source);
        let prefix = &source[..source.find(call).unwrap()];
        let line = prefix.chars().filter(|ch| *ch == '\n').count() + 1;
        let column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
        let output = run_surtr(&temp, &["test", "math"]);
        let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
        assert!(
            stdout.contains(&format!("lib/tests/math.srt:{line}:{column}")),
            "{source}\n{stdout}\n{stderr}"
        );
        assert!(stdout.contains(&format!("{assertion} failed:")), "{stdout}");
        if body.contains("actual:") {
            assert!(stdout.contains("LHS term: \"期待\""), "{stdout}");
            assert!(stdout.contains("RHS term: \"実際\""), "{stdout}");
        }
        if body.contains("&Test::") {
            assert!(!stdout.contains("LHS term:"), "{stdout}");
            assert!(!stdout.contains("RHS term:"), "{stdout}");
        }
    }

    let helper =
        "defmod Helper {\n  def check() -> Result<()> {\n    Test::assert_false(True)\n  }\n}\n";
    write_source(&temp.join("lib/tests/helper.srt"), helper);
    write_math_test(&temp, "include \"./helper.srt\"\nimport Test;\ntest(\"included\") { it(\"failure\") { Helper::check() } }\n");
    let output = run_surtr(&temp, &["test", "math"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
    assert!(stdout.contains("helper.srt:3:5"), "{stdout}\n{stderr}");
    assert!(stdout.contains("Test::assert_false(True)"), "{stdout}");
    assert!(stdout.contains("assert_false failed:"), "{stdout}");

    // A function with the same short name is not a standard assertion. Keep
    // the Error's construction site, even for TestAssertionFailed itself.
    write_source(&temp.join("lib/tests/helper.srt"), "defmod Helper {\n  def assert_true() -> Result<()> {\n    Err(TestAssertionFailed(\"custom failure\"))\n  }\n}\n");
    write_math_test(&temp, "include \"./helper.srt\"\nimport Test;\ntest(\"included\") { it(\"failure\") { Helper::assert_true() } }\n");
    let output = run_surtr(&temp, &["test", "math"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
    assert!(stdout.contains("helper.srt:3:9"), "{stdout}\n{stderr}");
    assert!(stdout.contains("custom failure"), "{stdout}");
    assert!(!stdout.contains("assert_true failed:"), "{stdout}");
    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_assertion_captions_use_the_executed_call_site() {
    let temp = unique_temp_dir("surtr_test_assertion_captions");
    let cases = [
        ("assert_true(False)", "assert_true"),
        ("assert_false(True)", "assert_false"),
        ("assert_eq(1, 2)", "assert_eq"),
        ("assert_ok_eq(1, Ok(2))", "assert_ok_eq"),
        ("assert_ok_eq(1, Err(NoneError))", "assert_ok_eq"),
        (
            "assert_err_contains(\"missing\", Err(NoneError))",
            "assert_err_contains",
        ),
        (
            "assert_err_contains(\"missing\", Ok(1))",
            "assert_err_contains",
        ),
        ("assert_stdout_eq([\"missing\"])", "assert_stdout_eq"),
        ("assert_stderr_eq([\"missing\"])", "assert_stderr_eq"),
        (
            "assert_doc_plain_eq(\"expected\", StyledDoc::text(\"actual\"))",
            "assert_doc_plain_eq",
        ),
        (
            "assert_doc_ansi_eq(\"expected\", StyledDoc::text(\"actual\"))",
            "assert_doc_ansi_eq",
        ),
    ];
    for (assertion, name) in cases {
        for (before, after) in [
            ("", ""),
            (
                "do::<Result> { assert_eq(\"prior\", \"prior\")\n        ",
                " }",
            ),
            ("assert_true(True)?\n      ", "?\n      Ok(())"),
        ] {
            let source = format!(
                "import Test;\ntest(\"キャプション\") {{\n  describe(\"nested\") {{\n    it(\"same name\") {{\n      {before}{assertion}{after}\n    }}\n    it(\"same name\") {{ assert_eq(\"later\", \"later\") }}\n  }}\n}}\n",
            );
            write_math_test(&temp, &source);
            let byte_start = source.find(assertion).unwrap();
            let prefix = &source[..byte_start];
            let line = prefix.chars().filter(|ch| *ch == '\n').count() + 1;
            let column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
            let output = run_surtr(&temp, &["test", "math"]);
            let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(
                output.status.code(),
                Some(1),
                "{source}\n{stdout}\n{stderr}"
            );
            assert!(
                stdout.contains(&format!("lib/tests/math.srt:{line}:{column}")),
                "wrong caption for {assertion}:\n{stdout}\n{stderr}"
            );
            assert!(stdout.contains(&format!("{name} failed:")), "{stdout}");
            assert!(!stdout.contains("LHS term: \"later\""), "{stdout}");
            assert!(
                stdout.contains("test result: passed=1, failed=1, total=2"),
                "{stdout}"
            );
        }
    }
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
    assert_eq!(
        output.status.code(),
        Some(1),
        "test command failure should exit with status 1"
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
fn test_command_runs_range_library_tests_with_polymorphic_constructor_calls() {
    let repo = repo_root();
    let output = run_surtr(&repo, &["test", "range"]);
    assert!(
        output.status.success(),
        "range test command should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[PASS] Range > construction helpers > keeps constructor polymorphism across different endpoint types in one scope"));
    assert!(stdout.contains("test result: passed=6, failed=0, total=6"));
}

#[test]
fn test_command_type_errors_hide_numeric_inference_variables_in_diagnostics() {
    let temp = unique_temp_dir("surtr_test_command_hidden_numeric_tyvars");
    write_source(
        &temp.join("lib/tests/generic_diag.srt"),
        r#"import Test;

defstruct Box<$A> {
  value: $A,
}

impl Box {
  def new(value: $A) -> Box<$A> where $A: Compare {
    match Compare::compare(value, value) {
      Ordering::Less => Box { value },
      Ordering::Equal => Box { value },
      Ordering::Greater => Box { value },
    }
  }
}

test("Diagnostics") {
  it("renders user-facing placeholders") {
    bad = Box(Option::Some(1))
    assert_eq("unreachable", inspect(bad))
  }
}
"#,
    );

    let output = run_surtr(&temp, &["test", "generic_diag"]);
    assert!(
        !output.status.success(),
        "generic diagnostic test should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        stderr.contains("MissingTraitCapability: Option<Int> must implement Compare"),
        "{stderr}"
    );
    assert!(
        stderr.contains("Call target signature: Box::new(arg1: $A) -> Box"),
        "{stderr}"
    );
    assert!(!contains_numeric_tyvar(&stderr), "{stderr}");

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
  describe("TryConvert") {
    it("parses ints through the assertion pipeline") {
      try_to::<Int>("1") |>= {|value| assert_eq(1, value)}
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
    assert!(
        stdout.contains("[PASS] String > TryConvert > parses ints through the assertion pipeline")
    );
    assert!(stdout.contains("test result: passed=1, failed=0, total=1"));

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
        &temp.join("lib/tests/capture_stdout.srt"),
        r#"import Test;

test("Capture stdout") {
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
    write_source(
        &temp.join("lib/tests/capture_stderr.srt"),
        r#"import Test;

test("Capture stderr") {
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
    assert!(stdout.contains("[PASS] Capture stdout > asserts captured print lines"));
    assert!(stdout.contains("[PASS] Stdin > reads pushed stdin lines through IO"));
    assert!(stdout.contains("[PASS] Stdin > reads pushed stdin chars through IO"));
    assert!(stdout.contains("[PASS] Capture stderr > asserts captured eprint fallback lines"));
    assert!(stdout.contains("[PASS] IO isolation > leaves unread io behind"));
    assert!(stdout.contains("[PASS] IO isolation > starts with fresh io buffers"));
    assert!(stdout.contains("test result: passed=8, failed=0, total=8"));

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
fn normal_script_run_rejects_import_test() {
    let temp = unique_temp_dir("surtr_run_rejects_test_module");
    write_source(
        &temp.join("main.srt"),
        r#"import Test;

print("ok")
"#,
    );

    let output = run_surtr(&temp, &["run", "main.srt"]);
    assert!(
        !output.status.success(),
        "normal script run should reject Test module\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(stderr.contains("Unknown module import: Test"), "{stderr}");

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_runs_file_module_tests_and_writes_real_files() {
    let temp = unique_temp_dir("surtr_test_command_file_module");
    let sandbox_dir = temp.join("tmp/sandbox");
    fs::create_dir_all(&sandbox_dir).expect("sandbox dir should be creatable");

    let repo_file_test = repo_root().join("lib/tests/file.srt");
    write_source(
        &temp.join("lib/tests/file.srt"),
        &fs::read_to_string(&repo_file_test).expect("repo file test fixture should exist"),
    );

    let repo_file_module = repo_root().join("lib/file.srt");
    if repo_file_module.exists() {
        write_source(
            &temp.join("lib/file.srt"),
            &fs::read_to_string(&repo_file_module).expect("repo file module should be readable"),
        );
    }

    let output = run_surtr(&temp, &["test", "file"]);
    assert!(
        output.status.success(),
        "file test command should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[PASS] File > writes and reads text"));
    assert!(stdout.contains("[PASS] File > appends text"));
    assert!(stdout.contains("[PASS] File > checks exists and delete"));
    assert!(stdout.contains("[PASS] File > writes and flushes through with_open"));
    assert!(stdout.contains("[PASS] File > reads chunks until eof"));
    assert!(stdout.contains("[PASS] File > reports missing path"));
    assert!(stdout.contains("test result: passed=6, failed=0, total=6"));

    assert_eq!(
        fs::read_to_string(sandbox_dir.join("write_read.txt"))
            .expect("write_read file should exist after test"),
        "alpha"
    );
    assert_eq!(
        fs::read_to_string(sandbox_dir.join("append.txt"))
            .expect("append file should exist after test"),
        "onetwo"
    );
    assert!(
        !sandbox_dir.join("delete.txt").exists(),
        "delete.txt should have been removed by the File test"
    );
    assert_eq!(
        fs::read_to_string(sandbox_dir.join("with_open_write.txt"))
            .expect("with_open_write file should exist after test"),
        "chunk-a"
    );
    assert_eq!(
        fs::read_to_string(sandbox_dir.join("read_chunk.txt"))
            .expect("read_chunk file should exist after test"),
        "abcdef"
    );
    assert!(
        !sandbox_dir.join("missing/nope.txt").exists(),
        "missing-path assertion should not materialize the absent file"
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_extension_case_selection() {
    let temp = unique_temp_dir("surtr_test_extension_selection");
    write_math_test(
        &temp,
        r#"import Test;
test("suite") {
  it("active") { assert_true(True) }
  xit("paused", "repairing") { assert_true(False) }
  pend("future", "later")
}
"#,
    );
    let output = run_surtr(&temp, &["test", "math"]);
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("[SKIP]"));
    let output = run_surtr(&temp, &["test", "math", "--include-xit"]);
    assert!(!output.status.success());
    let output = run_surtr(&temp, &["test", "math", "--list", "--include-xit"]);
    assert!(output.status.success());
    let _ = fs::remove_dir_all(temp);
}

fn test_json(output: &Output) -> serde_json::Value {
    assert!(
        output.stderr.is_empty(),
        "JSON stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stdout)))
}

fn assert_test_counts(report: &serde_json::Value) {
    let s = &report["summary"];
    let n = |key: &str| s[key].as_u64().unwrap();
    assert_eq!(n("discovered"), n("selected") + n("filtered"));
    assert_eq!(n("executed"), n("passed") + n("failed"));
    assert_eq!(
        n("selected"),
        if report["mode"] == "list" {
            n("runnable")
        } else {
            n("executed")
        } + n("skipped")
            + n("pending")
    );
}

#[test]
fn test_command_extension_eight_output_boundaries() {
    let temp = unique_temp_dir("surtr_test_extension_outputs");
    write_math_test(
        &temp,
        r#"import Test;
print("walk top")
test("suite") {
  print("walk suite")
  it("duplicate") { print("case output"); assert_true(True) }
  it("duplicate") { assert_stdout_eq([]) }
  xit("paused", "repairing") { print("paused body"); assert_true(False) }
  pend("future", "later")
}
"#,
    );
    write_source(
        &temp.join("lib/tests/second.srt"),
        "import Test;\nit(\"top level\") { assert_true(True) }\n",
    );
    for all in [false, true] {
        for list in [false, true] {
            for json in [false, true] {
                let mut args = vec!["test", if all { "--all" } else { "math" }];
                if list {
                    args.push("--list");
                }
                if json {
                    args.extend(["--format=json"]);
                }
                let output = run_surtr(&temp, &args);
                assert!(
                    output.status.success(),
                    "{args:?}: {}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                if json {
                    let report = test_json(&output);
                    assert_test_counts(&report);
                    assert_eq!(report["summary"]["discovered"], if all { 5 } else { 4 });
                    assert_eq!(report["summary"]["pending"], 1);
                    assert_eq!(report["summary"]["skipped"], 1);
                    assert_eq!(report["summary"]["failed"], 0);
                    assert_eq!(
                        report["scripts"][0]["io"]["stdout"],
                        serde_json::json!(["walk top", "walk suite"])
                    );
                    assert_eq!(report["cases"][0]["case_index"], 0);
                    assert_eq!(report["cases"][1]["case_index"], 1);
                    assert_eq!(report["cases"][0]["name"], report["cases"][1]["name"]);
                    assert_eq!(
                        report["cases"][0]["status"],
                        if list { "runnable" } else { "passed" }
                    );
                    assert!(report["duration_ns"].is_null());
                    for case in report["cases"].as_array().unwrap() {
                        assert!(case["duration_ns"].is_null());
                        if list {
                            assert!(case["io"].is_null());
                        }
                    }
                    if !list {
                        assert_eq!(
                            report["cases"][0]["io"]["stdout"],
                            serde_json::json!(["case output"])
                        );
                    }
                } else {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    assert!(stdout.contains(if list { "[LIST]" } else { "[PASS]" }));
                    assert!(stdout.contains("repairing"));
                    assert!(stdout.contains("later"));
                    assert!(!stdout.contains("paused body"));
                }
            }
        }
    }
    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_extension_combined_policies_and_timings() {
    let temp = unique_temp_dir("surtr_test_extension_policies");
    write_math_test(
        &temp,
        r#"import Test;
test("outer") {
  test("suite") {
    describe("outer group") {
      describe("group") {
        it("selected active") { assert_stdout_eq([]) }
        xit("selected paused", "repair") { print("only once"); assert_stdout_eq(["only once"]) }
        pend("selected future", "later")
        pend("excluded future", "filtered reason")
      }
    }
  }
}
it("selected top") { assert_true(True) }
"#,
    );
    for list in [false, true] {
        let mut args = vec![
            "test",
            "--all",
            "--test=suite",
            "--describe",
            "group",
            "--it",
            "selected",
            "--include-xit",
            "--deny-pending",
            "--quiet",
            "--timings",
            "--format=json",
        ];
        if list {
            args.push("--list");
        }
        let output = run_surtr(&temp, &args);
        assert_eq!(output.status.code(), Some(1));
        let report = test_json(&output);
        assert_test_counts(&report);
        assert_eq!(report["summary"]["selected"], 3);
        assert_eq!(report["summary"]["filtered"], 2);
        assert_eq!(report["summary"]["policy_errors"], 1);
        assert!(report["duration_ns"].as_u64().is_some());
        assert_eq!(
            report["options"]["filters"],
            serde_json::json!({"test":"suite","describe":"group","it":"selected"})
        );
        for flag in ["include_xit", "deny_pending", "quiet", "timings"] {
            assert_eq!(report["options"][flag], true);
        }
        let cases = report["cases"].as_array().unwrap();
        assert_eq!(cases.len(), if list { 3 } else { 1 });
        assert_eq!(cases.last().unwrap()["reason"], "later");
        assert!(cases.last().unwrap()["duration_ns"].is_null());
        if list {
            assert_eq!(cases[1]["status"], "runnable");
            assert!(cases[1]["io"].is_null());
        }
    }
    let output = run_surtr(
        &temp,
        &[
            "test",
            "math",
            "--include-xit",
            "--timings",
            "--it=paused",
            "--format=json",
        ],
    );
    let report = test_json(&output);
    assert!(output.status.success());
    assert_test_counts(&report);
    let case = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["selected"] == true)
        .unwrap();
    assert_eq!(case["status"], "passed");
    assert_eq!(case["declaration"], "xit");
    assert_eq!(case["reason"], "repair");
    assert!(case["duration_ns"].as_u64().is_some());
    let output = run_surtr(
        &temp,
        &[
            "test",
            "math",
            "--deny-pending",
            "--it=active",
            "--quiet",
            "--format=json",
        ],
    );
    let report = test_json(&output);
    assert!(output.status.success());
    assert_eq!(report["summary"]["policy_errors"], 0);
    assert_eq!(report["summary"]["passed"], 1);
    assert_eq!(report["cases"], serde_json::json!([]));
    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_extension_errors_and_usage_json() {
    let temp = unique_temp_dir("surtr_test_extension_errors");
    write_math_test(
        &temp,
        r#"import Test;
it("interrupted") { it("nested") { assert_true(True) } }
it("not reached") { assert_true(True) }
"#,
    );
    write_source(
        &temp.join("lib/tests/second.srt"),
        "import Test;\nit(\"next file\") { assert_true(True) }\n",
    );
    let output = run_surtr(&temp, &["test", "--all", "--format=json", "--timings"]);
    let report = test_json(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_test_counts(&report);
    assert_eq!(report["summary"]["discovered"], 2);
    assert_eq!(report["summary"]["failed"], 1);
    assert_eq!(report["summary"]["passed"], 1);
    assert_eq!(report["summary"]["script_errors"], 1);
    assert_eq!(report["scripts"][0]["status"], "aborted");
    assert_eq!(report["scripts"][1]["status"], "completed");
    assert!(report["cases"][0]["duration_ns"].as_u64().is_some());
    assert!(report["errors"][0]["diagnostic"]["message"]
        .as_str()
        .unwrap()
        .contains("inside a test case"));

    write_math_test(
        &temp,
        "import Test;\ntest(\"broken scope\") { Err(NoneError) }\n",
    );
    let output = run_surtr(
        &temp,
        &["test", "math", "--list", "--it=missing", "--format=json"],
    );
    let report = test_json(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report["summary"]["scope_failures"], 1);
    assert_eq!(report["summary"]["failed"], 0);
    assert_eq!(report["summary"]["policy_errors"], 0);
    assert_eq!(report["errors"][0]["kind"], "scope");

    for args in [
        vec!["test", "--bad", "--format=json"],
        vec!["test", "math", "--it", "--format", "json"],
        vec!["test", "--format=json"],
    ] {
        let output = run_surtr(&temp, &args);
        let report = test_json(&output);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(report["errors"][0]["kind"], "usage");
        assert!(report["options"].is_null());
    }
    for args in [
        vec!["test", "math", "--format=json", "--format=human"],
        vec!["test", "math", "--format=json", "--format"],
        vec!["test", "math", "--format=bad"],
    ] {
        let output = run_surtr(&temp, &args);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
    }
    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_extension_scan_and_declaration_failures_are_not_filtered() {
    let temp = unique_temp_dir("surtr_test_extension_scan_errors");
    for source in [
        "import Test;\nxit(\"hidden\", \"  \" ) { assert_true(True) }",
        "import Test;\npend(\"hidden\", \"\")",
        "import Test;\nxit(\"hidden\", \"repair\") { assert_true(1) }",
        "import Test;\npend(\"hidden\", \"later\") { assert_true(True) }",
    ] {
        write_math_test(&temp, source);
        let output = run_surtr(
            &temp,
            &["test", "math", "--it=missing", "--list", "--format=json"],
        );
        let report = test_json(&output);
        assert_eq!(output.status.code(), Some(1), "{source}");
        assert_eq!(report["summary"]["script_errors"], 1, "{source}: {report}");
        assert_eq!(report["summary"]["policy_errors"], 0);
    }
    write_math_test(&temp, "import Test;\npend(\"future\", \"later\")");
    write_source(
        &temp.join("lib/tests/second.srt"),
        "import Test;\nxit(\"paused\", \"repair\") { assert_true(True) }",
    );
    for name in ["future", "paused"] {
        let output = run_surtr(&temp, &["test", "--all", "--it", name, "--format=json"]);
        let report = test_json(&output);
        assert!(output.status.success());
        assert_eq!(report["summary"]["selected"], 1);
        assert_eq!(report["summary"]["policy_errors"], 0);
    }
    let output = run_surtr(&temp, &["test", "--all", "--it=missing", "--format=json"]);
    let report = test_json(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report["summary"]["policy_errors"], 1);
    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_extension_assertion_type_boundaries() {
    let temp = unique_temp_dir("surtr_test_extension_assertion_types");
    for (source, expected) in [
        ("assert_ne({|x: Int| x}, {|x: Int| x})", "Eq"),
        (
            "assert_some_eq({|x: Int| x}, Option::Some({|x: Int| x}))",
            "Eq",
        ),
        ("assert_approx(1, 1.0, 0.0)", "Float"),
        (
            "assert_err_kind(\"NoneError\", Err(NoneError))",
            "concrete deferror",
        ),
        (
            "assert_err_kind(PayloadFailure(\"x\"), Err(NoneError))",
            "concrete deferror",
        ),
        (
            "assert_err_kind(Int, Err(NoneError))",
            "Undefined variable: Int",
        ),
        (
            "&Test::assert_err_kind(&1, Err(NoneError))",
            "concrete deferror",
        ),
        (
            "assert_err_kind(Error, Err(NoneError))",
            "concrete deferror",
        ),
        (
            "marker = NoneError\nassert_err_kind(marker, Err(NoneError))",
            "concrete deferror",
        ),
        (
            "def forward(marker: ErrorKind) -> Result<()> { Ok(()) }\nOk(())",
            "ErrorKind is reserved",
        ),
    ] {
        write_math_test(
            &temp,
            &format!(
                "import Test;\ndeferror PayloadFailure(detail: String) {{ detail }}\n{source}\n"
            ),
        );
        let output = run_surtr(&temp, &["test", "math", "--format=json"]);
        let report = test_json(&output);
        assert_eq!(output.status.code(), Some(1), "{source}: {report}");
        assert_eq!(report["summary"]["script_errors"], 1);
        assert!(
            report["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains(expected),
            "{source}: {report}"
        );
    }
    let _ = fs::remove_dir_all(temp);
}
