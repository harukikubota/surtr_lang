use crate::common::{surtr_command, unique_temp_dir, write_source};
use serde_json::Value;
use std::fs;

#[test]
fn build_uses_default_eldr_output_path() {
    let temp = unique_temp_dir("surtr_step1_default_path");
    let source_path = temp.join("default_out.srt");
    let expected_eldr_path = temp.join("default_out.eldr");

    write_source(&source_path, "print(\"hello\")\n");
    let build = surtr_command()
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
    for eldr_path in [&first_eldr_path, &second_eldr_path] {
        let build = surtr_command()
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
    assert_eq!(
        first, second,
        "same input should produce identical .eldr bytes"
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn dump_outputs_valid_json_for_jq() {
    let temp = unique_temp_dir("surtr_dump_json");
    let source_path = temp.join("dump_sample.srt");
    let eldr_path = temp.join("dump_sample.eldr");

    write_source(&source_path, "print(\"hello\")\n");
    let build = surtr_command()
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

    let dump = surtr_command()
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
    let chunk_tags = json["chunks"]
        .as_array()
        .expect("chunks must be an array")
        .iter()
        .filter_map(|chunk| chunk["tag"].as_str())
        .collect::<Vec<_>>();
    assert!(chunk_tags.contains(&"Docs"));
    assert!(chunk_tags.contains(&"Func"));
    assert!(chunk_tags.contains(&"ImpT"));
    assert!(chunk_tags.contains(&"ExpT"));
    assert!(chunk_tags.contains(&"LitT"));
    assert!(json["summary"]["opcode_count"].as_u64().unwrap_or(0) > 0);
    assert!(json["summary"]["doc_count"].as_u64().unwrap_or(0) > 0);
    assert!(json["summary"]["function_count"].as_u64().unwrap_or(0) > 0);
    assert!(json["summary"]["label_count"].as_u64().unwrap_or(0) > 0);

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
    let dump = surtr_command()
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
    assert!(json["summary"]["doc_count"].as_u64().unwrap_or(0) > 0);
    let normalized = json["entrypoint_trace"]["normalized_entrypoint"]
        .as_str()
        .expect("normalized entrypoint must be a string");
    assert!(normalized.starts_with("__Script::"));
    assert!(normalized.ends_with("::launch"));

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn check_outputs_machine_readable_json_for_success_and_failure() {
    let temp = unique_temp_dir("surtr_check_json");
    let ok_source_path = temp.join("ok.srt");
    let bad_source_path = temp.join("bad.srt");

    write_source(&ok_source_path, "print(\"hello\")\n");
    write_source(&bad_source_path, "bad: Int = \"oops\"\n");
    let ok = surtr_command()
        .args([
            "check",
            ok_source_path.to_str().expect("source path must be utf-8"),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run check command");
    assert!(
        ok.status.success(),
        "check failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&ok.stdout),
        String::from_utf8_lossy(&ok.stderr)
    );
    let ok_json: Value =
        serde_json::from_slice(&ok.stdout).expect("check success output must be valid json");
    assert_eq!(
        ok_json["errors"]
            .as_array()
            .map(|items| items.len())
            .unwrap_or(1),
        0
    );

    let bad = surtr_command()
        .args([
            "check",
            bad_source_path.to_str().expect("source path must be utf-8"),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run check command");
    assert!(
        !bad.status.success(),
        "check should fail for type error\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&bad.stdout),
        String::from_utf8_lossy(&bad.stderr)
    );
    let bad_json: Value =
        serde_json::from_slice(&bad.stdout).expect("check failure output must be valid json");
    let first = bad_json["errors"][0].clone();
    assert_eq!(first["phase"], "typecheck");
    assert_eq!(first["kind"], "TypeError");
    assert_eq!(first["expected"], "Int");
    assert_eq!(first["got"], "String");
    assert!(first["hint"].is_string() || first["hint"].is_null());
    assert!(first["line"].as_u64().unwrap_or(0) >= 1);

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn dump_outputs_viewer_json() {
    let temp = unique_temp_dir("surtr_dump_viewer_json");
    let source_path = temp.join("viewer_sample.srt");

    write_source(&source_path, "print(\"hello\")\n");
    let dump = surtr_command()
        .args([
            "dump",
            source_path.to_str().expect("source path must be utf-8"),
            "--format",
            "viewer-json",
        ])
        .output()
        .expect("failed to run dump command");
    assert!(
        dump.status.success(),
        "viewer dump failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&dump.stdout),
        String::from_utf8_lossy(&dump.stderr)
    );

    let json: Value =
        serde_json::from_slice(&dump.stdout).expect("viewer dump output must be valid json");
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["format"], "eldr_viewer");
    assert!(json["functions"].as_array().is_some());
    assert!(json["opcodes"].as_array().is_some());
    assert!(json["sources"].as_array().is_some());
    assert!(json["errors"].as_array().is_some());

    let _ = fs::remove_dir_all(temp);
}
