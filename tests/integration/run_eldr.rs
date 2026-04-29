use std::fs;
use std::process::Command;

use serde_json::Value;

use crate::common::{surtr_bin, unique_temp_dir, write_source};

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
    assert!(
        run_eldr.stderr.is_empty(),
        "run eldr should not emit stderr on success, got:\n{}",
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
fn run_vm_dump_writes_json_on_success_when_always_enabled() {
    let temp = unique_temp_dir("surtr_vm_dump_success");
    let source_path = temp.join("sample.srt");
    let dump_path = temp.join("vm-dump.json");
    write_source(&source_path, "print(\"ok\")\n");

    let output = Command::new(surtr_bin())
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
            "--vm-dump",
            dump_path.to_str().expect("dump path must be utf-8"),
            "--vm-dump-on",
            "always",
        ])
        .output()
        .expect("failed to run source command");

    assert!(
        output.status.success(),
        "run source should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dump_path.exists(), "vm dump file should exist");

    let dump: Value =
        serde_json::from_slice(&fs::read(&dump_path).expect("vm dump file should be readable"))
            .expect("vm dump should be valid json");
    assert_eq!(dump["result"]["status"], "ok");
    assert_eq!(dump["result"]["exit_code"], 0);
    assert_eq!(dump["vm"]["last_opcode"], "Halt");
    assert!(
        dump["stats"]["executed_opcodes"].as_u64().unwrap_or(0) > 0,
        "expected opcode stats in vm dump: {dump}"
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_vm_dump_skips_success_when_error_mode_is_default() {
    let temp = unique_temp_dir("surtr_vm_dump_skip_success");
    let source_path = temp.join("sample.srt");
    let dump_path = temp.join("vm-dump.json");
    write_source(&source_path, "print(\"ok\")\n");

    let output = Command::new(surtr_bin())
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
            "--vm-dump",
            dump_path.to_str().expect("dump path must be utf-8"),
        ])
        .output()
        .expect("failed to run source command");

    assert!(
        output.status.success(),
        "run source should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !dump_path.exists(),
        "vm dump should not be written for successful run in default error mode"
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_vm_dump_writes_json_for_err_result_in_error_mode() {
    let temp = unique_temp_dir("surtr_vm_dump_err_result");
    let source_path = temp.join("sample.srt");
    let dump_path = temp.join("vm-dump.json");
    write_source(&source_path, "safe_div(1, 0)\n");

    let output = Command::new(surtr_bin())
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
            "--vm-dump",
            dump_path.to_str().expect("dump path must be utf-8"),
        ])
        .output()
        .expect("failed to run source command");

    assert!(
        !output.status.success(),
        "run source should fail for Err result\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dump_path.exists(), "vm dump file should exist");

    let dump: Value =
        serde_json::from_slice(&fs::read(&dump_path).expect("vm dump file should be readable"))
            .expect("vm dump should be valid json");
    assert_eq!(dump["result"]["status"], "result_err");
    assert_eq!(dump["result"]["exit_code"], 0);
    assert_eq!(
        dump["result"]["last_value"],
        "Err(ZeroDivisionError(\"division by zero\"))"
    );
    assert!(dump["result"]["runtime_error"].is_null());

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_allows_explicit_bootstrap_import() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_explicit_bootstrap_import");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"import Bootstrap;

print("ok")"#,
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
        "run source should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        String::from_utf8_lossy(&output.stdout).contains("ok"),
        "expected bootstrap-imported script to print ok\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_rejects_explicit_kernel_import() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_explicit_kernel_import");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"import Kernel;

print(to_string(add(1, 2)))"#,
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
        stderr.contains("Import conflict"),
        "expected import conflict diagnostic, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Kernel"),
        "expected Kernel in diagnostic, got:\n{}",
        stderr
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_include_loads_module_relative_to_script() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_include_relative_module");
    let source_path = temp.join("sample.srt");
    let helper_path = temp.join("Helper.srt");

    write_source(
        &helper_path,
        r#"defmod Helper {
  def add(x: Int, y: Int) -> Int { x + y }
}"#,
    );
    write_source(
        &source_path,
        r#"include 'Helper.srt'
import Helper::add
print(to_string(add(1, 2)))"#,
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
        "run source should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "run source should not emit stderr on success, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "3\n",
        "expected loaded module function output, got:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_include_rejects_non_literal_argument() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_include_non_literal");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"path = "Helper.srt"
include path
print("ok")"#,
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
        stderr.contains("include expects a string literal path"),
        "expected include argument diagnostic, got:\n{}",
        stderr
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_module_file_rejects_include_directive() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_module_include_forbidden");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"defmod Helper {
  def add(x: Int, y: Int) -> Int { x + y }
}

include './extra.srt'"#,
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
        stderr.contains("This top-level declaration is not allowed in the current source policy"),
        "expected module policy diagnostic, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("include"),
        "expected include in diagnostic, got:\n{}",
        stderr
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
        r#"err_result: Result<Int> = Err(NoneError)
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
        stderr.contains("sample.srt:1:"),
        "expected error to point at generation site on line 1, got:\n{}",
        stderr
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_compile_error_points_to_offending_expression() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_deferror_compile_location");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"def main() -> Result<Int> {
  "bad"
}

main()"#,
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
        stderr.contains("sample.srt:2:"),
        "expected compile error to point at the offending expression on line 2, got:\n{}",
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

#[test]
fn run_source_main_set_exit_code_updates_process_status_and_keeps_running() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_main_set_exit_code");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"def main() -> Result<()> {
  set_exit_code(7)
  print("still running")
  Ok(())
}

main()
"#,
    );

    let output = Command::new(&bin)
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
        ])
        .output()
        .expect("failed to run source command");

    assert_eq!(
        output.status.code(),
        Some(7),
        "expected exit code 7\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "still running\n",
        "expected evaluation to continue after set_exit_code"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).trim().is_empty(),
        "expected no stderr output"
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_main_err_overrides_set_exit_code_with_runtime_error_exit() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_main_err_overrides_exit_code");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"def main() -> Result<()> {
  set_exit_code(7)
  Err(NoneError)
}

main()
"#,
    );

    let output = Command::new(&bin)
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
        ])
        .output()
        .expect("failed to run source command");

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected Err(main()) to force exit code 1\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("NoneError"),
        "expected runtime diagnostic for NoneError, got:\n{}",
        stderr
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_cli_entry_executes_selected_function_only() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_run_cli_entry");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"print("top-level")

def start() -> Result<()> {
  print("start")
  Ok(())
}
"#,
    );

    let output = Command::new(&bin)
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
            "--entry",
            "start",
        ])
        .output()
        .expect("failed to run source command");

    assert!(
        output.status.success(),
        "run source should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "start\n",
        "top-level evaluation must be skipped when --entry is provided"
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_entrypoint_annotation_executes_annotated_function() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_run_annotation_entry");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"print("top-level")

@@entrypoint
def auto_entry() -> Result<()> {
  print("annotated")
  Ok(())
}
"#,
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
        "run source should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "annotated\n",
        "top-level evaluation must be skipped when @@entrypoint is present"
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_cli_entry_overrides_entrypoint_annotation() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_run_entry_precedence");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"@@entrypoint
def auto_entry() -> Result<()> {
  print("auto")
  Ok(())
}

def manual_entry() -> Result<()> {
  print("manual")
  Ok(())
}
"#,
    );

    let output = Command::new(&bin)
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
            "--entry",
            "manual_entry",
        ])
        .output()
        .expect("failed to run source command");

    assert!(
        output.status.success(),
        "run source should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "manual\n");

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_duplicate_entrypoint_annotation_is_compile_error() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_run_duplicate_entrypoint");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"@@entrypoint
def first() -> Result<()> { Ok(()) }
@@entrypoint
def second() -> Result<()> { Ok(()) }
"#,
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
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("multiple @@entrypoint annotations"),
        "expected duplicate @@entrypoint error, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn run_source_entry_signature_is_checked_when_entry_selected() {
    let bin = surtr_bin();
    let temp = unique_temp_dir("surtr_run_entry_signature");
    let source_path = temp.join("sample.srt");
    write_source(
        &source_path,
        r#"def start(code: Int) -> Result<()> {
  Ok(())
}
"#,
    );

    let output = Command::new(&bin)
        .args([
            "run",
            source_path.to_str().expect("source path must be utf-8"),
            "--entry",
            "start",
        ])
        .output()
        .expect("failed to run source command");

    assert!(
        !output.status.success(),
        "run source should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("must have signature () -> Result<()>"),
        "expected entry signature violation, got:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_dir_all(temp);
}
