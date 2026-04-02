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
    // .../target/debug/deps/<test-binary> -> .../target/debug/surtr
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
    fs::write(path, source).expect("failed to write source file");
}

#[test]
fn run_eldr_matches_run_srt_output() {
    let temp = unique_temp_dir("surtr_step1_roundtrip");
    let source_path = temp.join("sample.srt");
    let eldr_path = temp.join("sample.eldr");

    write_source(
        &source_path,
        "num = 10\nnum2 = 5\nprint(to_string(num + num2))\nprint(\"ok\")\n",
    );

    let bin = surtr_bin();

    let build = Command::new(&bin)
        .args([
            "build",
            source_path.to_str().expect("source path must be utf-8"),
            eldr_path.to_str().expect("eldr path must be utf-8"),
        ])
        .output()
        .expect("failed to run build command");
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );

    let run_srt = Command::new(&bin)
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
        ])
        .output()
        .expect("failed to run source command");
    assert!(
        run_srt.status.success(),
        "run srt failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run_srt.stdout),
        String::from_utf8_lossy(&run_srt.stderr)
    );

    let run_eldr = Command::new(&bin)
        .args(["run", eldr_path.to_str().expect("eldr path must be utf-8")])
        .output()
        .expect("failed to run eldr command");
    assert!(
        run_eldr.status.success(),
        "run eldr failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run_eldr.stdout),
        String::from_utf8_lossy(&run_eldr.stderr)
    );

    assert_eq!(
        String::from_utf8_lossy(&run_srt.stdout),
        String::from_utf8_lossy(&run_eldr.stdout),
        "stdout mismatch between run <.srt> and run <.eldr>"
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_error_points_to_generation_site() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_deferror_location");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"deferror PageNotFound(html: String) {
  "Page Not Found. #{html}"
}

err_result: Result<Int> = Err(PageNotFound("404"))
match err_result {
  Ok(num) => print(to_string(num)),
  Err(e)  => eprint(e)
}"#,
    );
    let output = Command::new(&bin)
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
        ])
        .output()
        .expect("failed to run source command");

    assert!(
        output.status.success(),
        "run source failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("sample.srt:5:"),
        "expected error to point at generation site on line 5, got:\n{}",
        stderr
    );
    assert!(
        !stderr.contains("deferror PageNotFound"),
        "did not expect the definition site to be the primary focus, got:\n{}",
        stderr
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_deferror_compile_error_points_to_definition_site() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_deferror_compile_location");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"deferror BadMessage {
  1
}

Err(BadMessage)"#,
    );
    let output = Command::new(&bin)
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
        ])
        .output()
        .expect("failed to run source command");

    assert!(
        !output.status.success(),
        "run source should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("sample.srt:1:"),
        "expected deferror compile error to point at definition site on line 1, got:\n{}",
        stderr
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_safe_div_zero_returns_err_value() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_safe_div_zero");
    let source_path = temp.join("sample.srt");
    write_source(&source_path, r#"print(inspect(safe_div(1, 0)))"#);
    let output = Command::new(&bin)
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
        ])
        .output()
        .expect("failed to run source command");

    assert!(
        output.status.success(),
        "run source should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Err(ZeroDivisionError(\"division by zero\"))"),
        "expected safe_div zero to return Err value, got:\n{}",
        stdout
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.trim().is_empty(),
        "expected no stderr, got:\n{}",
        stderr
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_safe_mod_zero_returns_err_value_even_with_verbose_runtime_flag() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_safe_mod_zero");
    let source_path = temp.join("sample.srt");
    write_source(&source_path, r#"print(inspect(safe_mod(1, 0)))"#);
    let output = Command::new(&bin)
        .env("SURTR_VERBOSE_RUNTIME_ERROR", "1")
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
        ])
        .output()
        .expect("failed to run source command");

    assert!(
        output.status.success(),
        "run source should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Err(ZeroDivisionError(\"division by zero\"))"),
        "expected safe_mod zero to return Err value, got:\n{}",
        stdout
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.trim().is_empty(),
        "expected no stderr, got:\n{}",
        stderr
    );

    let _ = fs::remove_dir_all(temp);
}
