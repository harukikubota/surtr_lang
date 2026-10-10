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
    write_source(&temp.join("lib/tests/local/math.srt"), body);
}

#[test]
fn test_command_runs_actual_test_file_paths() {
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

    let absolute = temp.join("lib/tests/local/math.srt");
    for args in [
        vec!["test", "lib/tests/local/math.srt"],
        vec!["test", "./lib/tests/local/math.srt"],
        vec!["test", absolute.to_str().unwrap()],
    ] {
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
fn test_command_accepts_parent_and_symlink_file_paths() {
    use std::os::unix::fs::symlink;
    let temp = unique_temp_dir("surtr_test_command_file_paths");
    write_source(
        &temp.join("private/helper.srt"),
        "defmod Helper { def value() -> Int { 3 } }\n",
    );
    write_source(
        &temp.join("private/entry.srt"),
        "include \"./helper.srt\"\nimport Test;\nit(\"outside lib tests\") { assert_eq(3, Helper::value()) }\n",
    );
    fs::create_dir_all(temp.join("nested")).unwrap();
    symlink("private/entry.srt", temp.join("alias.srt")).unwrap();
    // Includes stay relative to the entry path: the alias also has its own adjacent helper.
    write_source(
        &temp.join("helper.srt"),
        "defmod Helper { def value() -> Int { 3 } }\n",
    );
    for (cwd, path) in [
        (temp.join("nested"), "../private/entry.srt"),
        (temp.clone(), "alias.srt"),
    ] {
        let output = run_surtr(&cwd, &["test", path]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
        let output = run_surtr(&temp, &["test", "lib/tests/local/math.srt"]);
        let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{kind}: {stdout}\n{stderr}");
        assert!(
            stdout.contains("[FAIL] Sequencing > first failure"),
            "{kind}: {stdout}\n{stderr}"
        );
        assert!(stdout.contains("expected 1, got 2"), "{kind}: {stdout}");
        assert!(
            stdout.contains("[PASS] Sequencing > next it"),
            "{kind}: {stdout}\n{stderr}"
        );
        assert!(
            stdout.contains("test result: passed=1, failed=1, total=2"),
            "{kind}: {stdout}"
        );
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
    let output = run_surtr(&temp, &["test", "lib/tests/local/math.srt"]);
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

    let output = run_surtr(&temp, &["test", "lib/tests/local/math.srt"]);
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
    assert!(stdout.contains("[FAIL] Math > add > rejects wrong sum (lib/tests/local/math.srt)"));
    assert!(stdout.contains("expected 6, got 14"));
    assert!(stdout.contains("test result: passed=0, failed=1, total=1"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_assertion_captions_follow_captures_and_included_helpers() {
    let temp = unique_temp_dir("surtr_test_assertion_call_boundaries");
    let cases = [
        (
            "check: (Int -> Result<()>) = &Test::assert_lt(&1, 2)\n    check(3)",
            "check(3)",
            "assert_lt",
        ),
        (
            "assert_satisfies(predicate: {|n| n > 0}, message: \"positive\", actual: 0)",
            "assert_satisfies(predicate: {|n| n > 0}, message: \"positive\", actual: 0)",
            "assert_satisfies",
        ),
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
        let source = format!(
            "import Test;\ntest(\"boundary\") {{\n  it(\"same\") {{ assert_eq(0, 0) }}\n  it(\"same\") {{\n    {body}\n  }}\n}}\n"
        );
        write_math_test(&temp, &source);
        let prefix = &source[..source.find(call).unwrap()];
        let line = prefix.chars().filter(|ch| *ch == '\n').count() + 1;
        let column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
        let output = run_surtr(&temp, &["test", "lib/tests/local/math.srt"]);
        let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
        assert!(
            stdout.contains(&format!("lib/tests/local/math.srt:{line}:{column}")),
            "{source}\n{stdout}\n{stderr}"
        );
        assert!(stdout.contains(&format!("{assertion} failed:")), "{stdout}");
        if assertion == "assert_eq" && body.contains("actual:") {
            assert!(stdout.contains("LHS term: \"期待\""), "{stdout}");
            assert!(stdout.contains("RHS term: \"実際\""), "{stdout}");
        }
        if body.contains("&Test::") {
            assert!(!stdout.contains("LHS term:"), "{stdout}");
            assert!(!stdout.contains("RHS term:"), "{stdout}");
        }
    }

    let helper =
        "defmod Helper {\n  def check() -> Result<()> {\n    Test::assert_gte(1, 2)\n  }\n}\n";
    write_source(&temp.join("lib/tests/local/helper.srt"), helper);
    write_math_test(
        &temp,
        "include \"./helper.srt\"\nimport Test;\ntest(\"included\") { it(\"failure\") { Helper::check() } }\n",
    );
    let output = run_surtr(&temp, &["test", "lib/tests/local/math.srt"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
    assert!(stdout.contains("helper.srt:3:5"), "{stdout}\n{stderr}");
    assert!(stdout.contains("Test::assert_gte(1, 2)"), "{stdout}");
    assert!(stdout.contains("assert_gte failed:"), "{stdout}");

    // A function with the same short name is not a standard assertion. Keep
    // the Error's construction site, even for TestAssertionFailed itself.
    write_source(
        &temp.join("lib/tests/local/helper.srt"),
        "defmod Helper {\n  def assert_true() -> Result<()> {\n    Err(TestAssertionFailed(\"custom failure\"))\n  }\n}\n",
    );
    write_math_test(
        &temp,
        "include \"./helper.srt\"\nimport Test;\ntest(\"included\") { it(\"failure\") { Helper::assert_true() } }\n",
    );
    let output = run_surtr(&temp, &["test", "lib/tests/local/math.srt"]);
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
    check_assertion_call_sites(&cases);
}

#[test]
fn test_command_validation_assertions_report_public_call_sites() {
    check_assertion_call_sites(&[
        ("assert(False, \"condition\")", "assert"),
        (
            "assert_satisfies(0, \"positive\", {|n| n > 0})",
            "assert_satisfies",
        ),
        ("assert_lt(2, 1)", "assert_lt"),
        ("assert_lte(2, 1)", "assert_lte"),
        ("assert_gt(1, 2)", "assert_gt"),
        ("assert_gte(1, 2)", "assert_gte"),
        (
            "assert_err_message_eq(\"other\", Err(NoneError))",
            "assert_err_message_eq",
        ),
        ("assert_starts_with(\"a\", \"b\")", "assert_starts_with"),
        ("assert_ends_with(\"a\", \"b\")", "assert_ends_with"),
        ("assert_some(Option<Int>::None)", "assert_some"),
    ]);
}

fn check_assertion_call_sites(cases: &[(&str, &str)]) {
    let temp = unique_temp_dir("surtr_test_assertion_captions");
    let mut source = "import Test;\ntest(\"キャプション\") {\n".to_string();
    let mut expected = Vec::new();
    for (case_index, &(assertion, name)) in cases.iter().enumerate() {
        for (wrap_index, (before, after)) in [
            ("", ""),
            (
                "do::<Result> { assert_eq(\"prior\", \"prior\")\n        ",
                " }",
            ),
            ("assert_true(True)?\n      ", "?\n      Ok(())"),
        ]
        .into_iter()
        .enumerate()
        {
            let group = format!("nested {case_index}-{wrap_index}");
            let fragment = format!(
                "  describe(\"{group}\") {{\n    it(\"same name\") {{\n      {before}{assertion}{after}\n    }}\n    it(\"same name\") {{ assert_eq(\"later\", \"later\") }}\n  }}\n",
            );
            let byte_start = fragment.find(assertion).unwrap();
            let prefix = format!("{}{}", source, &fragment[..byte_start]);
            let line = prefix.chars().filter(|ch| *ch == '\n').count() + 1;
            let column = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
            expected.push((assertion, name, group, line, column));
            source.push_str(&fragment);
        }
    }
    source.push_str("}\n");
    write_math_test(&temp, &source);
    let output = run_surtr(&temp, &["test", "lib/tests/local/math.srt"]);
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{source}\n{stdout}\n{stderr}"
    );
    assert!(stderr.is_empty(), "{source}\n{stdout}\n{stderr}");
    assert!(!stdout.contains("LHS term: \"later\""), "{stdout}");

    // Match each diagnostic only inside its own failed case's output block.
    // The next event has the same case name, so scope identity must distinguish it.
    let mut event_starts = Vec::new();
    let mut offset = 0;
    for line in stdout.split_inclusive('\n') {
        if line.starts_with("[FAIL] ") || line.starts_with("[PASS] ") {
            event_starts.push(offset);
        }
        offset += line.len();
    }
    assert_eq!(event_starts.len(), expected.len() * 2, "{source}\n{stdout}");
    for (pair, (assertion, name, group, line, column)) in
        event_starts.chunks_exact(2).zip(&expected)
    {
        let failed = &stdout[pair[0]..pair[1]];
        let passed = &stdout[pair[1]..];
        assert!(
            failed.starts_with(&format!(
                "[FAIL] キャプション > {group} > same name (lib/tests/local/math.srt)\n"
            )),
            "{assertion}: {failed}"
        );
        assert!(
            passed.starts_with(&format!("[PASS] キャプション > {group} > same name\n")),
            "{assertion}: {passed}"
        );
        assert!(
            failed.contains(&format!("lib/tests/local/math.srt:{line}:{column}")),
            "wrong caption for {assertion}:\n{failed}"
        );
        assert!(failed.contains(&format!("{name} failed:")), "{failed}");
        assert!(!failed.contains("LHS term: \"later\""), "{failed}");
    }
    let count = expected.len();
    assert!(
        stdout.contains(&format!(
            "test result: passed={count}, failed={count}, total={}",
            count * 2
        )),
        "{stdout}"
    );
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

    let output = run_surtr(&temp, &["test", "lib/tests/local/math.srt"]);
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
    assert!(stdout.contains("[FAIL] String > repeat > bad (lib/tests/local/math.srt)"));
    assert!(stdout.contains("TestAssertionFailed: expected \"tes\", got \"bad\""));
    assert!(stdout.contains("assert_eq(\"tes\", \"bad\")"));
    assert!(stdout.contains("LHS term: \"tes\""));
    assert!(stdout.contains("RHS term: \"bad\""));
    assert!(stdout.contains("assert_eq failed: expected \"tes\", got \"bad\""));
    assert!(stdout.contains("lib/tests/local/math.srt"));
    assert!(!stdout.contains("note:"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_runs_range_library_tests_with_polymorphic_constructor_calls() {
    let repo = repo_root();
    let output = run_surtr(&repo, &["test", "lib/tests/basic_types/range.srt"]);
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
        &temp.join("lib/tests/local/generic_diag.srt"),
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

    let output = run_surtr(&temp, &["test", "lib/tests/local/generic_diag.srt"]);
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

    let output = run_surtr(&temp, &["test", "--quiet", "lib/tests/local/math.srt"]);
    assert!(
        output.status.success(),
        "quiet test command should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(String::from_utf8_lossy(&output.stdout), "");

    let output = run_surtr(&temp, &["test", "lib/tests/local/math.srt", "-q"]);
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

    let output = run_surtr(&temp, &["test", "-q", "lib/tests/local/math.srt"]);
    assert!(
        !output.status.success(),
        "quiet failing test command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[FAIL] Math > rejects wrong sum (lib/tests/local/math.srt)"));
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

    let output = run_surtr_with_env(
        &temp,
        &["test", "lib/tests/local/math.srt"],
        &[("SURTR_TEST_COLOR", "always")],
    );
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
        &temp.join("lib/tests/local/string_pipeline.srt"),
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

    let output = run_surtr(&temp, &["test", "lib/tests/local/string_pipeline.srt"]);
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
    assert!(stderr.contains("test: failed to read missing:"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_all_runs_lib_test_scripts() {
    let temp = unique_temp_dir("surtr_test_command_all");
    write_source(
        &temp.join("lib/tests/local/alpha.srt"),
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
        &temp.join("lib/tests/local/capture_stdout.srt"),
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
        &temp.join("lib/tests/local/stdin.srt"),
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
        &temp.join("lib/tests/local/capture_stderr.srt"),
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
        &temp.join("lib/tests/local/io_isolation.srt"),
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
        &temp.join("lib/tests/local/bad.srt"),
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
    let target = temp.join("written.txt");
    write_source(
        &temp.join("entry.srt"),
        &format!(
            "import Test;\nit(\"writes a real file\") {{ File::write(\"{}\", \"alpha\") }}\n",
            target.display()
        ),
    );
    let output = run_surtr(&temp, &["test", "entry.srt"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "alpha");
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
    let output = run_surtr(&temp, &["test", "lib/tests/local/math.srt"]);
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("[SKIP]"));
    let output = run_surtr(
        &temp,
        &["test", "lib/tests/local/math.srt", "--include-xit"],
    );
    assert!(!output.status.success());
    let output = run_surtr(
        &temp,
        &[
            "test",
            "lib/tests/local/math.srt",
            "--list",
            "--include-xit",
        ],
    );
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
        &temp.join("lib/tests/local/second.srt"),
        "import Test;\nit(\"top level\") { assert_true(True) }\n",
    );
    for all in [false, true] {
        for list in [false, true] {
            for json in [false, true] {
                let mut args = vec![
                    "test",
                    if all {
                        "--all"
                    } else {
                        "lib/tests/local/math.srt"
                    },
                ];
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
            "lib/tests/local/math.srt",
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
            "lib/tests/local/math.srt",
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
        &temp.join("lib/tests/local/second.srt"),
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
        &[
            "test",
            "lib/tests/local/math.srt",
            "--list",
            "--it=missing",
            "--format=json",
        ],
    );
    let report = test_json(&output);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(report["summary"]["scope_failures"], 1);
    assert_eq!(report["summary"]["failed"], 0);
    assert_eq!(report["summary"]["policy_errors"], 0);
    assert_eq!(report["errors"][0]["kind"], "scope");

    for args in [
        vec!["test", "--bad", "--format=json"],
        vec![
            "test",
            "lib/tests/local/math.srt",
            "--it",
            "--format",
            "json",
        ],
        vec!["test", "--format=json"],
    ] {
        let output = run_surtr(&temp, &args);
        let report = test_json(&output);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(report["errors"][0]["kind"], "usage");
        assert!(report["options"].is_null());
    }
    for args in [
        vec![
            "test",
            "lib/tests/local/math.srt",
            "--format=json",
            "--format=human",
        ],
        vec![
            "test",
            "lib/tests/local/math.srt",
            "--format=json",
            "--format",
        ],
        vec!["test", "lib/tests/local/math.srt", "--format=bad"],
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
            &[
                "test",
                "lib/tests/local/math.srt",
                "--it=missing",
                "--list",
                "--format=json",
            ],
        );
        let report = test_json(&output);
        assert_eq!(output.status.code(), Some(1), "{source}");
        assert_eq!(report["summary"]["script_errors"], 1, "{source}: {report}");
        assert_eq!(report["summary"]["policy_errors"], 0);
    }
    write_math_test(&temp, "import Test;\npend(\"future\", \"later\")");
    write_source(
        &temp.join("lib/tests/local/second.srt"),
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
    let source = "import Test;\ndeferror PayloadFailure(detail: String) { detail }\nassert_cause_chain([\"NoneError\"], Err(NoneError))\n";
    write_math_test(&temp, source);
    let output = run_surtr(
        &temp,
        &["test", "lib/tests/local/math.srt", "--format=json"],
    );
    let report = test_json(&output);
    assert_eq!(output.status.code(), Some(1), "{source}: {report}");
    assert_eq!(report["summary"]["script_errors"], 1);
    assert!(
        report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("concrete deferror"),
        "{source}: {report}"
    );
    let _ = fs::remove_dir_all(temp);
}

fn check_assertion_cases(
    label: &str,
    declarations: &str,
    cases: &[(&str, &str, Option<&str>)],
) -> serde_json::Value {
    let temp = unique_temp_dir(label);
    write_source(
        &temp.join("lib/tests/local/support/assertions.srt"),
        r#"
defmod UserAssertions {
  def assert() -> Result<()> { Ok(()) }
  def check() -> Result<()> { assert() }
  def assert_err_kind(marker: String, result: Result<Int>) -> Result<()> { Ok(()) }
  def assert_cause_chain(markers: List<String>, result: Result<Int>) -> Result<()> { Ok(()) }
}
"#,
    );
    let mut source =
        format!("include \"./support/assertions.srt\"\nimport Test;\n{declarations}\n");
    for (name, body, _) in cases {
        source.push_str(&format!("it(\"{name}\") {{ {body} }}\n"));
    }
    write_math_test(&temp, &source);
    let output = run_surtr(
        &temp,
        &["test", "lib/tests/local/math.srt", "--format=json"],
    );
    let report = test_json(&output);
    let failures = cases
        .iter()
        .filter(|(_, _, detail)| detail.is_some())
        .count();
    assert_eq!(
        output.status.code(),
        Some(i32::from(failures > 0)),
        "{label}: {report}"
    );
    assert_eq!(report["summary"]["script_errors"], 0, "{label}: {report}");
    assert_eq!(
        report["summary"]["passed"],
        cases.len() - failures,
        "{label}: {report}"
    );
    assert_eq!(report["summary"]["failed"], failures, "{label}: {report}");
    assert_eq!(
        report["cases"].as_array().unwrap().len(),
        cases.len(),
        "{label}: {report}"
    );
    for (actual, (name, body, detail)) in report["cases"].as_array().unwrap().iter().zip(cases) {
        assert_eq!(actual["name"], *name, "{label}: {actual}");
        assert_eq!(
            actual["status"],
            if detail.is_some() { "failed" } else { "passed" },
            "{label}/{name}: {body} => {actual}"
        );
        if let Some(detail) = detail {
            assert!(
                actual["detail"].as_str().unwrap().contains(detail),
                "{label}/{name}: {body} => {actual}"
            );
            assert!(
                actual["detail"]
                    .as_str()
                    .unwrap()
                    .contains("TestAssertionFailed"),
                "{label}/{name}: {actual}"
            );
        }
    }
    let _ = fs::remove_dir_all(temp);
    report
}

#[test]
fn test_command_extended_assertions_return_expected_results() {
    check_assertion_cases(
        "surtr_test_assertions",
        "",
        &[
            ("ne passes", "assert_ne(1, 2)", None),
            ("ne fails", "assert_ne(1, 1)", Some("expected unequal")),
            ("ok needs no Eq", "assert_ok(Ok({|x: Int| x}))", None),
            ("err passes", "assert_err(Err(NoneError))", None),
            (
                "ok rejects Err",
                "assert_ok(Err(NoneError))",
                Some("expected Ok"),
            ),
            ("err rejects Ok", "assert_err(Ok(1))", Some("expected Err")),
            ("some equality", "assert_some_eq(2, Option::Some(2))", None),
            (
                "some mismatch",
                "assert_some_eq(2, Option::Some(3))",
                Some("expected"),
            ),
            (
                "some rejects None",
                "assert_some_eq(2, Option::None)",
                Some("None"),
            ),
            (
                "none needs no Eq",
                "value: Option<(Int -> Int)> = Option::None\nassert_none(value)",
                None,
            ),
            (
                "none rejects Some",
                "assert_none(Option::Some(3))",
                Some("Some(3)"),
            ),
            (
                "contains unicode",
                "assert_contains(\"世界\", \"hello 世界\")",
                None,
            ),
            ("contains empty", "assert_contains(\"\", \"text\")", None),
            (
                "contains missing",
                "assert_contains(\"missing\", \"text\")",
                Some("missing"),
            ),
            (
                "explicit failure",
                "fail(\"Keep this detail\")",
                Some("Keep this detail"),
            ),
        ],
    );
}

#[test]
fn test_command_validation_assertions_and_composition() {
    check_assertion_cases(
        "surtr_test_validation_assertions",
        "deferror MessageFailure(detail: String) { detail }",
        &[
            ("assert pass", "assert(True, \"yes\")", None),
            (
                "assert function",
                "check: (Boolean, String -> Result<()>) = &Test::assert\ncheck(True, \"ok\")",
                None,
            ),
            ("assert named", "assert(message: \"ok\", flag: True)", None),
            ("user assert", "UserAssertions::check()", None),
            (
                "assert message",
                "assert(False, \"exact message\")",
                Some("exact message"),
            ),
            (
                "assert empty message",
                "assert_err_message_eq(\"\", assert(False, \"\"))",
                None,
            ),
            (
                "predicate pass",
                "assert_satisfies(3, \"positive\", {|n| n > 0})",
                None,
            ),
            (
                "predicate failure",
                "assert_satisfies(0, \"positive\", {|n| n > 0})",
                Some("positive\\nactual: 0"),
            ),
            (
                "predicate once",
                "do { assert_satisfies(3, \"positive\", {|n| print(\"called\")\nn > 0})\nassert_stdout_eq([\"called\"]) }",
                None,
            ),
            (
                "eager message",
                "message: (-> String) = {|| print(\"message\")\n\"positive\"}\ndo { assert_satisfies(3, message(), {|n| n > 0})\nassert_stdout_eq([\"message\"]) }",
                None,
            ),
            (
                "message ignores cause",
                "assert_err_message_eq(\"outer\", Result::cause(Err(MessageFailure(\"inner\")), MessageFailure(\"outer\")))",
                None,
            ),
            ("lt pass", "assert_lt(1, 2)", None),
            ("lt equal", "assert_lt(2, 2)", Some("2 < 2")),
            ("lte equal", "assert_lte(2, 2)", None),
            ("lte strict", "assert_lte(1, 2)", None),
            ("lte failure", "assert_lte(3, 2)", Some("3 <= 2")),
            ("gt pass", "assert_gt(2, 1)", None),
            ("gt equal", "assert_gt(2, 2)", Some("2 > 2")),
            ("gte equal", "assert_gte(2, 2)", None),
            ("gte strict", "assert_gte(2, 1)", None),
            ("gte failure", "assert_gte(1, 2)", Some("1 >= 2")),
            (
                "message pass",
                "assert_err_message_eq(\"detail\", Err(MessageFailure(\"detail\")))",
                None,
            ),
            (
                "message mismatch",
                "assert_err_message_eq(\"wanted\", Err(MessageFailure(\"actual\")))",
                Some("actual"),
            ),
            (
                "message Ok",
                "assert_err_message_eq(\"wanted\", Ok(42))",
                Some("Ok(42)"),
            ),
            (
                "prefix unicode",
                "assert_starts_with(\"世界\", \"世界 hello\")",
                None,
            ),
            ("prefix empty", "assert_starts_with(\"\", \"text\")", None),
            (
                "prefix mismatch",
                "assert_starts_with(\"Text\", \"text\")",
                Some("Text"),
            ),
            (
                "suffix unicode",
                "assert_ends_with(\"世界\", \"hello 世界\")",
                None,
            ),
            ("suffix empty", "assert_ends_with(\"\", \"text\")", None),
            (
                "suffix mismatch",
                "assert_ends_with(\"Text\", \"text\")",
                Some("Text"),
            ),
            (
                "some no Eq",
                "assert_some(Option::Some({|x: Int| x}))",
                None,
            ),
            (
                "some None",
                "value: Option<Int> = Option::None\nassert_some(value)",
                Some("None"),
            ),
            (
                "bare propagation",
                "do { assert(False, \"stopped\")\nfail(\"unreachable\") }",
                Some("stopped"),
            ),
            (
                "question propagation",
                "assert(False, \"stopped\")?\nfail(\"unreachable\")",
                Some("stopped"),
            ),
            (
                "safe bind propagation",
                "_ =? assert(False, \"stopped\")\nfail(\"unreachable\")",
                Some("stopped"),
            ),
            (
                "explicit discard",
                "assert(False, \"discarded\");\nOk(())",
                None,
            ),
            (
                "do discard",
                "do::<Result> { assert(False, \"discarded\");\nOk(()) }",
                None,
            ),
            (
                "failed predicate once",
                "result = assert_satisfies(0, \"positive\", {|n| print(\"called\")\nn > 0})\ndo { assert_err(result)\nassert_stdout_eq([\"called\"]) }",
                None,
            ),
            (
                "stored functions",
                "checks: List<(Int -> Result<()>)> = [&Test::assert_gt(&1, 0), &Test::assert_lt(&1, 10)]\nList::reduce(checks, Ok(()), {|prior, check| do { prior\ncheck(3) }})",
                None,
            ),
        ],
    );
}

#[test]
fn test_command_approx_assertion_finite_boundaries() {
    let huge = format!("17{}.0", "0".repeat(307));
    let overflow = format!("huge = {huge}\nassert_approx(huge, 0.0 - huge, huge)");
    check_assertion_cases(
        "surtr_test_approx",
        "",
        &[
            ("tolerance boundary", "assert_approx(1.0, 1.25, 0.25)", None),
            ("exact", "assert_approx(1.0, 1.0, 0.0)", None),
            ("opposite signs", "assert_approx(-1.0, 1.0, 2.0)", None),
            (
                "negative tolerance",
                "assert_approx(1.0, 1.0, -0.1)",
                Some("tolerance"),
            ),
            (
                "outside tolerance",
                "assert_approx(1.0, 1.5, 0.25)",
                Some("expected"),
            ),
            ("finite subtraction overflow", &overflow, Some("expected")),
        ],
    );
}

const ERROR_KIND_DECLARATIONS: &str = r#"
deferror PayloadFailure(detail: String) { detail }
deferror OtherFailure { "PayloadFailure" }
namespace First { deferror Same(detail: String) { detail } }
namespace Second { deferror Same(detail: String) { detail } }
def check_payload(result: Result<$A>) -> Result<()> { Test::assert_err_kind(PayloadFailure, result) }
def check_chain(result: Result<$A>) -> Result<()> { Test::assert_cause_chain([PayloadFailure, NoneError], result) }
def make_failure() -> Result<Int> {
  print("evaluated")
  Err(NoneError)
}
"#;

#[test]
fn test_command_error_kind_assertion_uses_declaration_identity() {
    check_assertion_cases(
        "surtr_test_error_kind",
        ERROR_KIND_DECLARATIONS,
        &[
            (
                "payload ignored",
                r#"assert_err_kind(PayloadFailure, Err(PayloadFailure("first")))"#,
                None,
            ),
            (
                "nullary",
                "assert_err_kind(NoneError, Err(NoneError))",
                None,
            ),
            (
                "generic helper",
                r#"check_payload(Err(PayloadFailure("generic")))"#,
                None,
            ),
            (
                "capture",
                r#"captured: (Result<Int> -> Result<()>) = &Test::assert_err_kind(PayloadFailure, &1)
 captured(Err(PayloadFailure("captured")))"#,
                None,
            ),
            (
                "qualified",
                r#"assert_err_kind(First::Same, Err(First::Same("same")))"#,
                None,
            ),
            (
                "different namespace",
                r#"assert_err_kind(First::Same, Err(Second::Same("same")))"#,
                Some("expected error kind First::Same, got Second::Same"),
            ),
            (
                "user function",
                r#"UserAssertions::assert_err_kind("literal", Ok(1))"#,
                None,
            ),
            (
                "Ok rejected",
                "assert_err_kind(PayloadFailure, Ok(3))",
                Some("expected Err"),
            ),
            (
                "different kind",
                "assert_err_kind(PayloadFailure, Err(OtherFailure))",
                Some("expected error kind"),
            ),
            (
                "root ignores cause",
                r#"assert_err_kind(PayloadFailure, Result::cause(Err(NoneError), PayloadFailure("outer")))"#,
                None,
            ),
        ],
    );
}

#[test]
fn test_command_cause_chain_assertion_matches_complete_outer_first_sequence() {
    let report = check_assertion_cases(
        "surtr_test_cause_chain",
        ERROR_KIND_DECLARATIONS,
        &[
            (
                "outer first",
                r#"assert_cause_chain([PayloadFailure, NoneError], Result::cause(Err(NoneError), PayloadFailure("outer")))"#,
                None,
            ),
            (
                "singleton",
                "assert_cause_chain([NoneError], Err(NoneError))",
                None,
            ),
            (
                "repeated kind",
                "assert_cause_chain([NoneError, NoneError], Result::cause(Err(NoneError), NoneError))",
                None,
            ),
            (
                "qualified",
                r#"assert_cause_chain([First::Same, Second::Same], Result::cause(Err(Second::Same("inner")), First::Same("outer")))"#,
                None,
            ),
            (
                "reversed names",
                r#"assert_cause_chain([Second::Same, First::Same], Result::cause(Err(Second::Same("inner")), First::Same("outer")))"#,
                Some("first mismatch at index 0"),
            ),
            (
                "inner mismatch",
                r#"assert_cause_chain([First::Same, First::Same], Result::cause(Err(Second::Same("inner")), First::Same("outer")))"#,
                Some("first mismatch at index 1"),
            ),
            (
                "missing cause",
                "assert_cause_chain([NoneError], Result::cause(Err(NoneError), NoneError))",
                Some("length mismatch: expected 1, got 2"),
            ),
            (
                "extra cause",
                "assert_cause_chain([NoneError, NoneError], Err(NoneError))",
                Some("length mismatch: expected 2, got 1"),
            ),
            (
                "empty Err",
                "assert_cause_chain([], Err(NoneError))",
                Some("expected cause chain [], got [Global::NoneError]"),
            ),
            ("empty Ok", "assert_cause_chain([], Ok(1))", Some("got Ok")),
            (
                "nonempty Ok",
                "assert_cause_chain([NoneError], Ok(1))",
                Some("got Ok"),
            ),
            (
                "Ok needs no Eq",
                "assert_cause_chain([NoneError], Ok({|x: Int| x}))",
                Some("got Ok"),
            ),
            (
                "capture",
                r#"captured: (Result<Int> -> Result<()>) = &Test::assert_cause_chain([PayloadFailure, NoneError], &1)
 captured(Result::cause(Err(NoneError), PayloadFailure("capture")))"#,
                None,
            ),
            (
                "generic Int",
                r#"value: Result<Int> = Result::cause(Err(NoneError), PayloadFailure("int"))
 check_chain(value)"#,
                None,
            ),
            (
                "generic String",
                r#"value: Result<String> = Result::cause(Err(NoneError), PayloadFailure("string"))
 check_chain(value)"#,
                None,
            ),
            (
                "user function",
                r#"UserAssertions::assert_cause_chain(["literal"], Ok(1))"#,
                None,
            ),
            (
                "evaluated once",
                "assert_cause_chain([NoneError], make_failure())",
                None,
            ),
            (
                "three entries",
                r#"assert_cause_chain([PayloadFailure, PayloadFailure, NoneError], Result::cause(Result::cause(Err(NoneError), PayloadFailure("inner")), PayloadFailure("outer")))"#,
                None,
            ),
        ],
    );
    let evaluated = report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "evaluated once")
        .unwrap();
    assert_eq!(
        evaluated["io"]["stdout"],
        serde_json::json!(["evaluated"]),
        "{evaluated}"
    );
}

#[test]
fn test_command_long_do_preserves_order_and_short_circuit_without_stack_overflow() {
    let temp = unique_temp_dir("surtr_test_long_do");
    let steps = (0..40)
        .map(|index| format!("assert_true(True)\nprint(\"step {index}\")\n"))
        .collect::<String>();
    let source = format!(
        "import Test;\nit(\"complete\") {{ do {{\n{steps}Ok(())\n}} }}\n\
         it(\"short circuit\") {{ do {{\n{steps}assert_true(False)\nprint(\"unreachable\")\nOk(())\n}} }}\n"
    );
    write_math_test(&temp, &source);
    let output = run_surtr(
        &temp,
        &["test", "lib/tests/local/math.srt", "--format=json"],
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = test_json(&output);
    assert_eq!(report["summary"]["script_errors"], 0, "{report}");
    assert_eq!(report["summary"]["passed"], 1, "{report}");
    assert_eq!(report["summary"]["failed"], 1, "{report}");
    let expected = serde_json::json!((0..40)
        .map(|index| format!("step {index}"))
        .collect::<Vec<_>>());
    for case in report["cases"].as_array().unwrap() {
        assert_eq!(case["io"]["stdout"], expected, "{case}");
    }
    assert!(report["cases"][1]["detail"]
        .as_str()
        .unwrap()
        .contains("TestAssertionFailed"));
    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_all_selects_only_category_entries_in_path_order() {
    let temp = unique_temp_dir("surtr_test_command_fixed_depth");
    for path in [
        "lib/tests/root.srt",
        "lib/tests/support/shared/defs.srt",
        "lib/tests/z/support/defs.srt",
        "lib/tests/z/deeper/entry.srt",
    ] {
        write_source(
            &temp.join(path),
            "invalid Surtr: depth must exclude this input",
        );
    }
    for path in ["schema/prelude.srt", "a/spec_defs.srt", "z/entry.srt"] {
        write_source(
            &temp.join("lib/tests").join(path),
            "import Test;\nit(\"passes\") { assert_true(True) }\n",
        );
    }
    let output = run_surtr(&temp, &["test", "--all", "--format=json"]);
    let report = test_json(&output);
    assert!(output.status.success(), "{report}");
    let files: Vec<_> = report["scripts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|script| script["file"].as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        [
            "lib/tests/a/spec_defs.srt",
            "lib/tests/schema/prelude.srt",
            "lib/tests/z/entry.srt"
        ]
    );
    assert_eq!(report["summary"]["passed"], 3);
    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_all_rejects_zero_files_but_keeps_zero_case_policy() {
    let temp = unique_temp_dir("surtr_test_command_empty_targets");
    write_source(
        &temp.join("lib/tests/root.srt"),
        "invalid ignored root input",
    );
    write_source(
        &temp.join("lib/tests/support/shared/defs.srt"),
        "invalid ignored support input",
    );
    let output = run_surtr(&temp, &["test", "--all", "--format=json"]);
    let report = test_json(&output);
    assert_eq!(output.status.code(), Some(1), "{report}");
    assert_eq!(report["summary"]["script_errors"], 1);
    assert!(report["errors"][0]["message"]
        .as_str()
        .unwrap()
        .contains("no test files"));
    write_source(&temp.join("lib/tests/local/empty.srt"), "import Test;\n");
    let output = run_surtr(&temp, &["test", "--all", "--format=json"]);
    let report = test_json(&output);
    assert!(output.status.success(), "{report}");
    assert_eq!(report["summary"]["discovered"], 0);
    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_does_not_rewrite_file_paths_or_fall_back_to_selectors() {
    let temp = unique_temp_dir("surtr_test_command_literal_paths");
    write_source(
        &temp.join("lib/tests/local/result.srt"),
        "import Test;\nit(\"library\") { assert_true(True) }\n",
    );
    for path in [
        "result",
        "result.srt",
        "lib\\tests\\local\\result.srt",
        "directory",
    ] {
        fs::create_dir_all(temp.join("directory")).unwrap();
        let output = run_surtr(&temp, &["test", path, "--format=json"]);
        let report = test_json(&output);
        assert_eq!(output.status.code(), Some(1), "{path}: {report}");
        assert_eq!(report["summary"]["script_errors"], 1);
    }
    for path in ["result", " spaced "] {
        write_source(
            &temp.join(path),
            "import Test;\nit(\"literal file\") { assert_true(True) }\n",
        );
        let output = run_surtr(&temp, &["test", path]);
        assert!(
            output.status.success(),
            "{path}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_assert_eq_obeys_custom_eq_instead_of_inspect() {
    check_assertion_cases(
        "surtr_test_custom_eq",
        r#"
defstruct EqByValue { value: Int, description: String }
impl EqByValue { def new(value: Int, description: String) -> Self { EqByValue { value, description } } }
impl Eq for EqByValue { def eq(self: Self, rhs: Self) -> Boolean { Eq::eq(self.value, rhs.value) } }
defstruct AlwaysDifferent { value: Int }
impl AlwaysDifferent { def new(value: Int) -> Self { AlwaysDifferent { value } } }
impl Eq for AlwaysDifferent { def eq(self: Self, rhs: Self) -> Boolean { False } }
"#,
        &[
            (
                "different text equal values",
                "assert_eq(EqByValue(7, \"first\"), EqByValue(7, \"second\"))",
                None,
            ),
            (
                "same text unequal values",
                "assert_eq(AlwaysDifferent(7), AlwaysDifferent(7))",
                Some("expected AlwaysDifferent"),
            ),
        ],
    );
}

#[test]
fn test_command_moved_entries_and_changed_support_invalidate_cached_bytecode() {
    let temp = unique_temp_dir("surtr_test_moved_include_cache");
    let entry = "include \"./support/value.srt\"\nimport Test;\nit(\"included value\") { assert_eq(1, CachedValue::get()) }\n";
    write_source(&temp.join("lib/tests/old/entry.srt"), entry);
    write_source(
        &temp.join("lib/tests/old/support/value.srt"),
        "defmod CachedValue { def get() -> Int { 1 } }\n",
    );
    for _ in 0..2 {
        let output = run_surtr(&temp, &["test", "lib/tests/old/entry.srt", "--format=json"]);
        assert!(output.status.success(), "{}", test_json(&output));
    }
    fs::rename(temp.join("lib/tests/old"), temp.join("lib/tests/new")).unwrap();
    write_source(
        &temp.join("lib/tests/new/support/value.srt"),
        "defmod CachedValue { def get() -> Int { 2 } }\n",
    );
    let output = run_surtr(&temp, &["test", "--all", "--format=json"]);
    let report = test_json(&output);
    assert_eq!(output.status.code(), Some(1), "{report}");
    assert_eq!(report["summary"]["failed"], 1);
    assert_eq!(report["cases"][0]["file"], "lib/tests/new/entry.srt");
    assert!(report["cases"][0]["detail"]
        .as_str()
        .unwrap()
        .contains("expected 1, got 2"));
    // Change only the dependency at the same path: the cached failure must not survive.
    write_source(
        &temp.join("lib/tests/new/support/value.srt"),
        "defmod CachedValue { def get() -> Int { 1 } }\n",
    );
    let output = run_surtr(&temp, &["test", "--all", "--format=json"]);
    assert!(output.status.success(), "{}", test_json(&output));
    let _ = fs::remove_dir_all(temp);
}

#[cfg(unix)]
#[test]
fn test_command_preserves_existing_backslash_names_and_relative_includes() {
    let temp = unique_temp_dir("surtr_test_backslash_names");
    let category = temp.join("lib/tests").join(r"literal\category");
    write_source(
        &category.join("support/value.srt"),
        "defmod LiteralValue { def get() -> Int { 3 } }\n",
    );
    write_source(
        &category.join(r"literal\entry.srt"),
        "include \"./support/value.srt\"\nimport Test;\nit(\"literal path\") { assert_eq(3, LiteralValue::get()) }\n",
    );
    let entry = r"lib/tests/literal\category/literal\entry.srt";
    for target in [entry, "--all"] {
        let output = run_surtr(&temp, &["test", target, "--format=json"]);
        let report = test_json(&output);
        assert!(output.status.success(), "{report}");
        assert_eq!(report["summary"]["passed"], 1);
        assert_eq!(report["scripts"][0]["file"], entry);
    }
    let _ = fs::remove_dir_all(temp);
}

#[cfg(unix)]
#[test]
fn test_command_all_reports_broken_file_links_and_continues() {
    let temp = unique_temp_dir("surtr_test_broken_file_link");
    write_source(
        &temp.join("lib/tests/local/z_good.srt"),
        "import Test;\nit(\"later case\") { assert_true(True) }\n",
    );
    std::os::unix::fs::symlink("missing.srt", temp.join("lib/tests/local/a_broken.srt")).unwrap();
    std::os::unix::fs::symlink(".", temp.join("lib/tests/local/b_directory.srt")).unwrap();
    let output = run_surtr(&temp, &["test", "--all", "--format=json"]);
    let report = test_json(&output);
    assert_eq!(output.status.code(), Some(1), "{report}");
    assert_eq!(report["summary"]["script_errors"], 2);
    assert_eq!(report["summary"]["passed"], 1);
    assert_eq!(report["scripts"][0]["file"], "lib/tests/local/a_broken.srt");
    assert_eq!(
        report["scripts"][1]["file"],
        "lib/tests/local/b_directory.srt"
    );
    assert_eq!(report["scripts"][2]["file"], "lib/tests/local/z_good.srt");
    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_shared_include_prefix_keeps_entries_independent() {
    let temp = unique_temp_dir("surtr_test_shared_include_prefix");
    write_source(
        &temp.join("lib/tests/local/support/shared.srt"),
        "defmod Shared {\n  def value() -> Int { 42 }\n  def check() -> Result<()> {\n    Test::assert_gte(1, 2)\n  }\n}\n",
    );
    for (file, body) in [
        ("a", "expected = 42; it(\"first\") { assert_eq(Shared::value(), expected) }"),
        ("b", "expected = \"second\"; it(\"second\") { assert_eq(Shared::value(), 42); assert_eq(expected, \"second\") }\nit(\"entry failure\") { assert_eq(42, 0) }\nit(\"dependency failure\") { Shared::check() }"),
    ] {
        write_source(
            &temp.join(format!("lib/tests/local/{file}.srt")),
            &format!("include \"./support/shared.srt\"\nimport Test;\n{body}\n"),
        );
    }
    let output = run_surtr(&temp, &["test", "--all", "--quiet", "--format=json"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = test_json(&output);
    assert_eq!(report["summary"]["passed"], 2);
    assert_eq!(report["summary"]["failed"], 2);
    let cases = report["cases"].as_array().unwrap();
    assert!(cases[0]["file"].as_str().unwrap().ends_with("b.srt"));
    assert!(cases[0]["diagnostic"]["file"]
        .as_str()
        .unwrap()
        .ends_with("b.srt"));
    assert!(cases[1]["diagnostic"]["file"]
        .as_str()
        .unwrap()
        .ends_with("support/shared.srt"));
    assert_eq!(cases[1]["diagnostic"]["line"], 4);
    let prefixes = fs::read_dir(temp.join("target/surtr-test-cache/prefix"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "semantic"))
        .count();
    assert_eq!(prefixes, 1, "same include environment must compile once");
    let _ = fs::remove_dir_all(temp);
}

#[cfg(unix)]
fn run_surtr_with_terminal_stderr(temp: &Path, args: &[&str]) -> (Output, String) {
    use std::io::Read;
    use std::os::fd::FromRawFd;
    use std::process::Stdio;
    let mut master = -1;
    let mut slave = -1;
    let mut size = libc::winsize {
        ws_row: 24,
        ws_col: 120,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    assert_eq!(result, 0, "terminal should open");
    let mut master = unsafe { fs::File::from_raw_fd(master) };
    let slave = unsafe { fs::File::from_raw_fd(slave) };
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            match master.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => bytes.extend_from_slice(&buffer[..count]),
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(error) => panic!("terminal read failed: {error}"),
            }
        }
        String::from_utf8(bytes).expect("terminal progress should be UTF-8")
    });
    let mut command = surtr_command();
    command
        .args(args)
        .current_dir(temp)
        .stderr(Stdio::from(slave));
    let output = command.output().expect("test command should run");
    drop(command);
    (output, reader.join().unwrap())
}

#[cfg(unix)]
#[test]
fn test_command_quiet_terminal_progress_preserves_json_and_clears() {
    let temp = unique_temp_dir("surtr_test_terminal_progress");
    write_source(
        &temp.join("lib/tests/local/a.srt"),
        "import Test;\nit(\"first\") { assert_eq(1, 1) }\n",
    );
    write_source(
        &temp.join("lib/tests/local/b.srt"),
        "import Test;\nit(\"second\") { assert_eq(2, 2) }\n",
    );
    let (output, progress) =
        run_surtr_with_terminal_stderr(&temp, &["test", "--all", "--quiet", "--format=json"]);
    assert!(
        output.status.success(),
        "{}\n{progress}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report = test_json(&output);
    assert_eq!(report["summary"]["passed"], 2);
    assert_eq!(
        progress.matches("Preparing standard environment").count(),
        1
    );
    for index in [1, 2] {
        assert!(
            progress.contains(&format!("Compiling [{index}/2]")),
            "{progress}"
        );
        assert!(
            progress.contains(&format!("Running [{index}/2]")),
            "{progress}"
        );
    }
    assert!(progress.find("Running [1/2]").unwrap() < progress.find("Compiling [2/2]").unwrap());
    assert!(progress.ends_with("\r\x1b[2K"), "{progress:?}");
    assert!(!progress.contains('\n'), "{progress:?}");
    let (warm, progress) = run_surtr_with_terminal_stderr(&temp, &["test", "--all", "--quiet"]);
    assert!(warm.status.success());
    assert!(warm.stdout.is_empty());
    assert!(
        !progress.contains("Preparing"),
        "warm bytecode should skip standard preparation"
    );
    assert!(progress.ends_with("\r\x1b[2K"));
    let piped = run_surtr(&temp, &["test", "--all", "--quiet"]);
    assert!(piped.status.success());
    assert!(piped.stdout.is_empty() && piped.stderr.is_empty());
    let _ = fs::remove_dir_all(temp);
}
