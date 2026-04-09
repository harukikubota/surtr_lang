use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn surtr_bin() -> String {
    if let Ok(path) = env::var("CARGO_BIN_EXE_surtr") {
        return path;
    }

    let mut path = env::current_exe().expect("failed to locate current test executable");
    path.pop(); // <test-binary>
    path.pop(); // deps
    path.push("surtr");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    assert!(
        path.exists(),
        "surtr binary not found at {}",
        path.display()
    );
    path.to_string_lossy().into_owned()
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();
    let dir = env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), nanos));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

fn write_source(path: &Path, source: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("failed to create parent directory");
    }
    fs::write(path, source).expect("failed to write source file");
}

#[test]
fn test_command_supports_all_module_and_function_selectors() {
    let temp = unique_temp_dir("surtr_test_command_selectors");
    let module_path = temp.join("lib/math.srt");
    write_source(
        &module_path,
        r#"defmod Math {
  @@test add(1, 2) == 3
  @@test add(1, 2) != 4
  def add(x: Int, y: Int) -> Int { x + y }
}
"#,
    );

    let bin = surtr_bin();
    for args in [
        vec!["test"],
        vec!["test", "Math"],
        vec!["test", "Math::add"],
    ] {
        let output = Command::new(&bin)
            .args(args.clone())
            .current_dir(&temp)
            .output()
            .expect("failed to run surtr test command");
        assert!(
            output.status.success(),
            "test command failed for args {:?}\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("[PASS] Math::add"));
        assert!(stdout.contains("test result: passed=2, failed=0, total=2"));
    }

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_reports_failure_details_for_comparison() {
    let temp = unique_temp_dir("surtr_test_command_failure");
    let module_path = temp.join("lib/math.srt");
    write_source(
        &module_path,
        r#"defmod Math {
  @@test add(10, 4) == 6
  def add(x: Int, y: Int) -> Int { x + y }
}
"#,
    );

    let bin = surtr_bin();
    let output = Command::new(&bin)
        .args(["test"])
        .current_dir(&temp)
        .output()
        .expect("failed to run surtr test command");
    assert!(
        !output.status.success(),
        "test command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[FAIL] Math::add (lib/math.srt:2:3)"));
    assert!(stdout.contains("expr: add(10, 4) == 6"));
    assert!(stdout.contains("lhs : add(10, 4) => 14"));
    assert!(stdout.contains("rhs : 6 => 6"));
    assert!(stdout.contains("op  : eq"));
    assert!(stdout.contains("test result: passed=0, failed=1, total=1"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_allows_annotations_and_blank_lines_between_test_and_def() {
    let temp = unique_temp_dir("surtr_test_command_annotation_chain");
    let module_path = temp.join("lib/math.srt");
    write_source(
        &module_path,
        r#"defmod Math {
  @@test add(1, 2) == 3

  @@builtin def to_string(a: $A) -> String

  @@test add(2, 3) == 5

  def add(x: Int, y: Int) -> Int { x + y }
}
"#,
    );

    let bin = surtr_bin();
    let output = Command::new(&bin)
        .args(["test"])
        .current_dir(&temp)
        .output()
        .expect("failed to run surtr test command");
    assert!(
        output.status.success(),
        "test command should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[PASS] Math::add"));
    assert!(stdout.contains("test result: passed=2, failed=0, total=2"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn test_command_surfaces_compile_error_details_for_test_expression() {
    let temp = unique_temp_dir("surtr_test_command_compile_error");
    let module_path = temp.join("lib/math.srt");
    write_source(
        &module_path,
        r#"defmod Math {
  @@test add("bad", 2) == 3
  def add(x: Int, y: Int) -> Int { x + y }
}
"#,
    );

    let bin = surtr_bin();
    let output = Command::new(&bin)
        .args(["test"])
        .current_dir(&temp)
        .output()
        .expect("failed to run surtr test command");
    assert!(
        !output.status.success(),
        "test command should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[FAIL] Math::add"));
    assert!(stdout.contains("note: TypeError at __surtr_test__.srt:2:"));
    assert!(stdout.contains("expected Int, got String"));

    let _ = fs::remove_dir_all(temp);
}
