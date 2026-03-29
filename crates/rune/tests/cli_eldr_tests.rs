use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

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
        .args(["run", source_path.to_str().expect("source path must be utf-8")])
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
fn build_uses_default_eldr_output_path() {
    let temp = unique_temp_dir("surtr_step1_default_path");
    let source_path = temp.join("default_out.srt");
    let expected_eldr_path = temp.join("default_out.eldr");

    write_source(&source_path, "print(\"hello\")\n");

    let bin = surtr_bin();
    let build = Command::new(&bin)
        .args([
            "build",
            source_path.to_str().expect("source path must be utf-8"),
        ])
        .output()
        .expect("failed to run build command");
    assert!(
        build.status.success(),
        "build failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(
        expected_eldr_path.exists(),
        "default .eldr output not found at {}",
        expected_eldr_path.display()
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn dump_outputs_valid_json_for_jq() {
    let temp = unique_temp_dir("surtr_dump_json");
    let source_path = temp.join("dump_sample.srt");
    let eldr_path = temp.join("dump_sample.eldr");

    write_source(&source_path, "print(\"hello\")\n");

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

    let dump = Command::new(&bin)
        .args([
            "dump",
            eldr_path.to_str().expect("eldr path must be utf-8"),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run dump command");
    assert!(
        dump.status.success(),
        "dump failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&dump.stdout),
        String::from_utf8_lossy(&dump.stderr)
    );

    let json: Value = serde_json::from_slice(&dump.stdout).expect("dump output must be valid json");
    assert_eq!(json["header"]["magic"], "ELDR");
    assert_eq!(json["chunks"][0]["tag"], "Code");
    assert!(json["summary"]["opcode_count"].as_u64().unwrap_or(0) > 0);

    let _ = fs::remove_dir_all(temp);
}
