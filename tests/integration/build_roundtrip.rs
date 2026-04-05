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
fn build_produces_identical_bytecode_for_same_input() {
    let temp = unique_temp_dir("surtr_step1_deterministic_build");
    let source_path = temp.join("deterministic.srt");
    let first_eldr_path = temp.join("first.eldr");
    let second_eldr_path = temp.join("second.eldr");

    write_source(
        &source_path,
        r#"nums = [1, 2, 3]
print(inspect(nums))
print(to_string(10 + 20))"#,
    );

    let bin = surtr_bin();
    for eldr_path in [&first_eldr_path, &second_eldr_path] {
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
    }

    let first = fs::read(&first_eldr_path).expect("failed to read first build output");
    let second = fs::read(&second_eldr_path).expect("failed to read second build output");
    assert_eq!(first, second, "same input should produce identical .eldr bytes");

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

#[test]
fn dump_supports_entry_srt_and_traces_normalized_entrypoint() {
    let temp = unique_temp_dir("surtr_dump_entry_srt");
    let source_path = temp.join("entry_trace.srt");

    write_source(
        &source_path,
        r#"@@entrypoint
def auto() -> Result<()> { Ok(()) }

def launch() -> Result<()> { Ok(()) }
"#,
    );

    let bin = surtr_bin();
    let dump = Command::new(&bin)
        .args([
            "dump",
            source_path.to_str().expect("source path must be utf-8"),
            "--entry",
            "launch",
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
    assert_eq!(json["entrypoint_trace"]["source"], "entry_file");
    assert_eq!(json["entrypoint_trace"]["selected_entry_name"], "launch");
    let normalized = json["entrypoint_trace"]["normalized_entrypoint"]
        .as_str()
        .expect("normalized entrypoint must be a string");
    assert!(normalized.starts_with("__Script::"));
    assert!(normalized.ends_with("::launch"));

    let _ = fs::remove_dir_all(temp);
}
