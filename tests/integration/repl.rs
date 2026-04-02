use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

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

fn run_repl_session(input: &str) -> Output {
    let bin = PathBuf::from(surtr_bin());
    let mut child = Command::new(bin)
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn surtr repl");

    child
        .stdin
        .as_mut()
        .expect("stdin pipe is unavailable")
        .write_all(input.as_bytes())
        .expect("failed to write repl input");

    child.wait_with_output().expect("failed to wait on repl")
}

#[test]
fn repl_quit_exits_cleanly() {
    let output = run_repl_session(":quit\n");
    assert!(
        output.status.success(),
        "repl should exit successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn repl_keeps_bindings_between_inputs() {
    let output = run_repl_session("x = 42\nx\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("x: Int = 42"),
        "expected bind echo in repl output, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("42"),
        "expected expression result in repl output, got:\n{}",
        stdout
    );
}

#[test]
fn repl_infers_closure_argument_type_from_add_constraint() {
    let output = run_repl_session("fun = {|num| num + 5}\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fun: (Int -> Int)"),
        "expected closure argument type to infer as Int, got:\n{}",
        stdout
    );
}

#[test]
fn repl_compile_error_does_not_break_session_state() {
    let output = run_repl_session("x = 1\nbad: Int = \"oops\"\nx\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("expected Int, got String"),
        "expected compile error details in stderr, got:\n{}",
        stderr
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1"),
        "expected previous binding to remain accessible after error, got:\n{}",
        stdout
    );
}

#[test]
fn repl_value_recall_by_line_number() {
    let output = run_repl_session("5\n:v 1\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("surtr(1)>"),
        "expected numbered prompt for first line, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("surtr(2)>"),
        "expected numbered prompt for second line, got:\n{}",
        stdout
    );

    let fives = stdout.matches("> 5").count();
    assert!(
        fives >= 2,
        "expected original value and :v recall output, got:\n{}",
        stdout
    );
}

#[test]
fn repl_deferror_builder_survives_incremental_execution() {
    let output = run_repl_session(
        "deferror PageNotFound(html: String) {\n  \"Page Not Found. #{html}\"\n}\nerr_result: Result<Int> = Err(PageNotFound(\"404\"))\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "err_result: Result<Int, Error> = Err(PageNotFound(\"Page Not Found. 404\"))"
        ),
        "expected incremental err_result binding, got:\n{}",
        stdout
    );
}

#[test]
fn repl_ok_defaults_result_error_type_to_error() {
    let output = run_repl_session("ret = Ok(10)\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ret: Result<Int, Error> = Ok(10)"),
        "expected Ok binding to default err side to Error, got:\n{}",
        stdout
    );
}

#[test]
fn repl_hides_internal_type_var_ids_in_result_display() {
    let output = run_repl_session("ret_e = Err(NoneError)\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ret_e: Result<_, Error> = Err(NoneError(\"None Value.\"))"),
        "expected Result type vars to be hidden in repl output, got:\n{}",
        stdout
    );
}

#[test]
fn repl_duplicate_function_name_is_rejected() {
    let output = run_repl_session("def f() -> Int { 1 }\ndef f() -> Int { 2 }\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Duplicate top-level definition: f"),
        "expected duplicate definition error in stderr, got:\n{}",
        stderr
    );
}

#[test]
fn repl_eprint_reports_generation_site_line() {
    let output = run_repl_session(
        "deferror PageNotFound(html: String) {\n  \"Page Not Found. #{html}\"\n}\nerr_result: Result<Int> = Err(PageNotFound(\"404\"))\nmatch err_result {\n  Ok(num) => print(to_string(num)),\n  Err(e)  => eprint(e)\n}\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("repl:2:"),
        "expected repl error to point at generation site within the current repl chunk, got:\n{}",
        stderr
    );
}
