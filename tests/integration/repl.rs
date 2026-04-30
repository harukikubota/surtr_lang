use crate::common::{surtr_command, unique_temp_dir};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn run_repl_session_with_args(args: &[&str], input: &str) -> Output {
    run_repl_session_with_args_in_dir(args, input, None)
}

fn run_repl_session_with_args_in_dir(args: &[&str], input: &str, cwd: Option<&PathBuf>) -> Output {
    let mut command = surtr_command();
    command
        .arg("repl")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    run_repl_command(command, input)
}

fn run_repl_session(input: &str) -> Output {
    run_repl_session_with_args(&[], input)
}

fn run_repl_session_with_color(input: &str) -> Output {
    let mut command = surtr_command();
    command
        .arg("repl")
        .env("SURTR_REPL_COLOR", "always")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    run_repl_command(command, input)
}

fn run_repl_command(mut command: Command, input: &str) -> Output {
    let mut child = command.spawn().expect("failed to spawn surtr repl");
    let mut stdin = child.stdin.take().expect("stdin pipe is unavailable");
    stdin
        .write_all(input.as_bytes())
        .expect("failed to write repl input");
    drop(stdin);

    child.wait_with_output().expect("failed to wait on repl")
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && matches!(chars.peek(), Some('[')) {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }

    out
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
fn repl_exit_exits_cleanly() {
    let output = run_repl_session(":exit\n");
    assert!(
        output.status.success(),
        "repl should exit successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn repl_fails_fast_when_additional_stdlib_bootstrap_fails() {
    let dir = unique_temp_dir("repl-bootstrap-failure");
    let lib_dir = dir.join("lib");
    fs::create_dir_all(&lib_dir).expect("failed to create lib dir");
    fs::write(lib_dir.join("bad.srt"), "defmod Broken { def nope( }")
        .expect("failed to write bad module");

    let output = run_repl_session_with_args_in_dir(&["--quiet"], "", Some(&dir));
    assert!(
        !output.status.success(),
        "repl init should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(stderr.contains("bootstrap failed during parse"));
    assert!(stderr.contains("lib/bad.srt"));
}

#[test]
fn repl_prints_light_banner_by_default() {
    let output = run_repl_session(":quit\n");
    assert!(
        output.status.success(),
        "repl should exit successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Surtr xldr"));
}

#[test]
fn repl_quiet_suppresses_banner() {
    let output = run_repl_session_with_args(&["--quiet"], ":quit\n");
    assert!(
        output.status.success(),
        "repl should exit successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("Surtr xldr"));
}

#[test]
fn repl_banner_flag_prints_detailed_banner() {
    let output = run_repl_session_with_args(&["--banner"], ":quit\n");
    assert!(
        output.status.success(),
        "repl should exit successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("██\\   ██\\ ██\\      ██████\\  ██████\\"));
    assert!(stdout.contains("\\__|  \\__|\\_______|\\______/ \\__|  \\__|"));
}

#[test]
fn repl_version_prints_version_and_exits() {
    let output = run_repl_session_with_args(&["--version"], "");
    assert!(
        output.status.success(),
        "repl should exit successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "xldr 0.1.0");
}

#[test]
fn repl_pipe_stdin_prints_prompts_and_eval_output() {
    let output = run_repl_session("x = 42\nx\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("xldr(1)> "));
    assert!(stdout.contains("xldr(2)> "));
    assert!(stdout.contains("> x: Int = 42"));
    assert!(stdout.contains("> 42"));
}

#[test]
fn repl_colorizes_sig_command_signature() {
    let output = run_repl_session_with_color(":sig print\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\u{1b}["));
    assert!(stdout.contains("\u{1b}[36ma\u{1b}[0m"));
    assert!(
        stdout.contains("\u{1b}[1;96mString\u{1b}[0m")
            && stdout.contains("\u{1b}[1;96mUnit\u{1b}[0m")
    );
    assert!(strip_ansi(&stdout).contains("Kernel::print(a: String) -> Unit"));
}

#[test]
fn repl_colorizes_doc_for_qualified_kernel_if() {
    let output = run_repl_session_with_color(":doc Kernel::if\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\u{1b}["));
    assert!(!stdout.contains("\u{1b}[43m") && !stdout.contains(";43m"));
    assert!(stdout.contains("\u{1b}[36mflag\u{1b}[0m"));
    assert!(stdout.contains("\u{1b}[1;96mBoolean\u{1b}[0m"));
    assert!(stdout.contains("\u{1b}[1;33m$A\u{1b}[0m"));
    assert!(strip_ansi(&stdout).contains("xldr(1)> if(True, \"ok\", \"ng\")"));
}

#[test]
fn repl_error_summary_then_full_changes_diagnostic_detail() {
    let output = run_repl_session(
        ":error summary\nbad: Int = \"oops\"\n:error full\nworse: Int = \"oops\"\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    let first_error_pos = stderr
        .find("Error: TypeError")
        .expect("expected first TypeError headline");
    let second_error_pos = stderr[first_error_pos + 1..]
        .find("Error: TypeError")
        .map(|offset| first_error_pos + 1 + offset)
        .expect("expected second TypeError headline");

    let first_block = &stderr[first_error_pos..second_error_pos];
    let second_block = &stderr[second_error_pos..];

    assert!(!first_block.contains("╭─["));
    assert!(second_block.contains("╭─["));
}

#[test]
fn repl_runtime_diagnostic_points_at_the_full_call() {
    let output = run_repl_session("safe_mod(10, 0)\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(stderr.contains("ZeroDivisionError"));
    assert!(stderr.contains("REPL:1:1"));
    assert!(stderr.contains("safe_mod(10, 0)"));
}

#[test]
fn repl_human_diagnostic_stays_on_stderr() {
    let output = run_repl_session("defstruct User { name: String }\n:quit\n");
    assert!(
        output.status.success(),
        "repl should remain alive after parse error\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(!stdout.contains("This top-level declaration is not allowed"));
    assert!(
        stderr.contains("This top-level declaration is not allowed in the current source policy")
    );
}

#[test]
fn repl_eprint_reports_generation_site_line() {
    let output = run_repl_session(
        "err_result: Result<Int> = Err(NoneError)\nmatch err_result {\n  Ok(num) => print(to_string(num)),\n  Err(e)  => eprint(e)\n}\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("REPL:2:"));
}
