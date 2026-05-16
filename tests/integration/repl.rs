use crate::common::{surtr_command, unique_temp_dir};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};

struct ChildGuard {
    child: Option<Child>,
}

impl ChildGuard {
    fn spawn(command: &mut Command) -> Self {
        Self {
            child: Some(command.spawn().expect("failed to spawn surtr repl")),
        }
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("child should still be owned")
    }

    fn wait_with_output(mut self) -> Output {
        self.child
            .take()
            .expect("child should still be owned")
            .wait_with_output()
            .expect("failed to wait on repl")
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };

        let _ = child.kill();
        let _ = child.wait();
    }
}

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
    let mut child = ChildGuard::spawn(&mut command);
    let mut stdin = child
        .child_mut()
        .stdin
        .take()
        .expect("stdin pipe is unavailable");
    stdin
        .write_all(input.as_bytes())
        .expect("failed to write repl input");
    drop(stdin);

    child.wait_with_output()
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
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("bootstrap failed during parse"));
    assert!(stderr.contains("lib/bad.srt"));
}

#[test]
fn repl_preload_diagnostic_stays_on_stderr_and_exits_non_zero() {
    let dir = unique_temp_dir("repl-preload-diagnostic");
    let script_path = dir.join("bad.srt");
    fs::write(&script_path, "defmod Broken { }\n").expect("failed to write bad preload script");

    let output = run_repl_session_with_args(
        &[
            "--quiet",
            "--script",
            script_path.to_string_lossy().as_ref(),
        ],
        "",
    );
    assert!(
        !output.status.success(),
        "repl preload should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("ParseError"), "{stderr}");
    assert!(
        stderr.contains("defmod is not allowed at script top-level"),
        "{stderr}"
    );
    assert!(stderr.contains("bad.srt"), "{stderr}");
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
    assert!(stdout.contains("xldr(1)> x: Int = 42"));
    assert!(stdout.contains("xldr(2)> 42"));
}

#[test]
fn repl_static_impl_methods_keep_declared_arity() {
    let output = run_repl_session(
        "print(to_string(Generator::to_list(Generator::range(1, 3))))\nprint(to_string(String::codepoints(\"a\", StringEncoding::Ascii)))\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("[1, 2, 3]"), "{stdout}");
    assert!(stdout.contains("Ok([97])"), "{stdout}");
    assert!(!stdout.contains("Call arity mismatch"), "{stdout}");
}

#[test]
fn repl_range_duration_comparisons_execute_without_arity_mismatch() {
    let output = run_repl_session(
        "print(to_string(compare(Range(10ms, 20ms), Range(10ms, 30ms))))\nprint(to_string(Range(10ms, 20ms) == Range(10ms, 20ms)))\nprint(to_string(Range(10ms, 20ms) != Range(10ms, 30ms)))\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("Ordering::Less"), "{stdout}");
    assert!(stdout.contains("True"), "{stdout}");
    assert!(!stdout.contains("Call arity mismatch"), "{stdout}");
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
fn repl_sig_expression_query_flows_through_cli_presentation() {
    let output = run_repl_session(
        "ret = Ok(\"3\")\nup = {|term: String| try_from(term, Int)}\n:sig ret |>= up\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("defined:"));
    assert!(stdout.contains("Chainable::chain("));
    assert!(stdout.contains("specialized:"));
    assert!(stdout.contains("ret |>= up: Result<Int>"));
}

#[test]
fn repl_rejects_persisting_unresolved_result_callable_binding() {
    let output = run_repl_session("todo = {|| Err(NoneError)}\n:quit\n");
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    let combined = format!("{stdout}\n{stderr}");

    assert!(
        combined.contains("Cannot persist binding with unresolved type variable."),
        "{combined}"
    );
    assert!(
        combined.contains(
            "Add a type annotation or use the value in a context that determines the success type."
        ),
        "{combined}"
    );
    assert!(
        !combined.contains("todo: (-> Result<_, Error>)"),
        "{combined}"
    );
}

#[test]
fn repl_rejects_persisting_unresolved_result_value_binding() {
    let output = run_repl_session("todo = {|| Err(NoneError)}\nret = todo()\n:quit\n");
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    let combined = format!("{stdout}\n{stderr}");

    assert!(
        combined.contains("Cannot persist binding with unresolved type variable."),
        "{combined}"
    );
    assert!(combined.contains("ret = todo()"), "{combined}");
    assert!(!combined.contains("ret: Result<_, Error>"), "{combined}");
}

#[test]
fn repl_accepts_explicitly_constrained_result_binding() {
    let output = run_repl_session("todo: (-> Result<Int>) = {|| Err(NoneError)}\nret: Result<Int> = todo()\n:type ret\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("ret: Result<Int, Error> = Err(NoneError"),
        "{stdout}"
    );
    assert!(stdout.contains("type: Result<Int, Error>"), "{stdout}");
}

#[test]
fn repl_accepts_result_mapping_when_chunk_constrains_type() {
    let output = run_repl_session(
        "todo: (-> Result<Int>) = {|| Err(NoneError)}\nmapped = todo() |*> inspect()\n:type mapped\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("mapped: Result<String, Error> = Err(NoneError"),
        "{stdout}"
    );
    assert!(stdout.contains("type: Result<String, Error>"), "{stdout}");
}

#[test]
fn repl_sig_symbolic_operator_and_polymorphic_query_render_through_cli() {
    let output = run_repl_session(":sig |>\n:sig id(Int)\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("trait PipeApply { pipe_apply(self: Self, value: $A) -> $B }"));
    assert!(stdout.contains("specialized:"));
    assert!(stdout.contains("id(Int) -> Int"));
}

#[test]
fn repl_sig_type_owner_constructor_fallback_renders_through_cli() {
    let output = run_repl_session(":sig Duration\n:sig Duration()\n:sig Option\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("Duration::new(value: Int) -> Result<Self, Error>"),
        "{stdout}"
    );
    assert!(
        stdout
            .matches("Duration::new(value: Int) -> Result<Self, Error>")
            .count()
            >= 2,
        "{stdout}"
    );
    assert!(stdout.contains("* Option::Some"), "{stdout}");
    assert!(stdout.contains("* Option::None"), "{stdout}");
}

#[test]
fn repl_doc_type_owner_prefers_canonical_type_docs() {
    let output = run_repl_session(":doc Option\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("defenum Option"), "{stdout}");
    assert!(stdout.contains("Standard `Option` enum."), "{stdout}");
    assert!(!stdout.contains("status: undocumented"), "{stdout}");
}

#[test]
fn repl_sig_attached_extractor_owner_query_matches_zero_arg_form() {
    let output =
        run_repl_session(":sig Duration!\n:sig Duration!()\n:sig Duration!(Duration)\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout
            .matches("Duration::deconstruct(self: Self) -> MatchResult<Int, Error>")
            .count()
            >= 3,
        "{stdout}"
    );
    assert!(
        stdout.contains("specialized:\n  Duration!() -> MatchResult<Int, Error>"),
        "{stdout}"
    );
    assert!(
        stdout.contains("specialized:\n  Duration!(Duration) -> MatchResult<Int, Error>"),
        "{stdout}"
    );
}

#[test]
fn repl_range_constructor_and_extractor_queries_render_through_cli() {
    let output =
        run_repl_session(":doc Range(Int, Int)\n:sig Range\n:sig Range()\n:doc Range!()\n:sig Range!\n:sig Range!()\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("Range::new"), "{stdout}");
    assert!(stdout.contains("min: $A"), "{stdout}");
    assert!(stdout.contains("max: $A"), "{stdout}");
    assert!(stdout.contains("-> Range<$A>"), "{stdout}");
    assert!(
        stdout.contains("Construct a range while preserving the input order."),
        "{stdout}"
    );
    assert!(stdout.contains("Range::deconstruct"), "{stdout}");
    assert!(stdout.contains("MatchResult<($A, $A), Error>"), "{stdout}");
    assert!(
        stdout.contains("Deconstruct a `Range` into `(min, max)` in pattern position."),
        "{stdout}"
    );
    assert!(
        stdout.contains("specialized:\n  Range!() -> MatchResult<($A, $A), Error>"),
        "{stdout}"
    );
}

#[test]
fn repl_sig_enum_rejects_extra_input_with_shared_message() {
    let output = run_repl_session(
        ":sig Option(Int)\n:sig Option::Some\n:sig Option::Some()\n:sig Option::Some(1)\n:sig Option::Some(Int)\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.matches(
            "Enum signatures are only available for bare type owners: use `:sig Option` instead"
        )
        .count()
            >= 4,
        "{stdout}"
    );
    assert!(stdout.contains("xldr(1)> xldr(1)>"), "{stdout}");
}

#[test]
fn repl_info_renders_styled_summary_for_queries() {
    let output = run_repl_session_with_color(
        "ret = Ok(\"3\")\nup = {|term: String| try_from(term, Int)}\n:info ret |>= up\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\u{1b}["));
    let plain = strip_ansi(&stdout);
    assert!(plain.contains("kind:"), "{plain}");
    assert!(plain.contains("defined:"), "{plain}");
    assert!(plain.contains("specialized:"), "{plain}");
    assert!(plain.contains("Result<Int>"), "{plain}");
}

#[test]
fn repl_supports_session_listing_and_reload_commands() {
    let output = run_repl_session(
        "seed = 41\ndef keep() -> Int { 42 }\n:vars\n:defs\n:history\n:clear\n:reload\nkeep()\nseed\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("line | name | type"), "{stdout}");
    assert!(stdout.contains("seed"), "{stdout}");
    assert!(stdout.contains("line | name | arity"), "{stdout}");
    assert!(stdout.contains("keep/0"), "{stdout}");
    assert!(stdout.contains("line | input"), "{stdout}");
    assert!(stdout.contains("seed = 41"), "{stdout}");
    assert!(
        stdout.contains("clear is not available in this host"),
        "{stdout}"
    );
    assert!(stdout.contains("reload complete: all"), "{stdout}");
    assert!(stdout.contains("42"), "{stdout}");
    assert!(stderr.contains("Undefined variable: seed"), "{stderr}");
}

#[test]
fn repl_sig_missing_symbol_prints_guidance() {
    let output = run_repl_session(":sig a\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("No signature found for a"));
    assert!(stdout.contains(":sig $a") || stdout.contains(":doc <symbol>"));
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
fn repl_keeps_print_output_plain_while_coloring_bindings_and_values() {
    let output = run_repl_session_with_color(
        "print(\"tick 1\")\nprint(inspect(Ok(True)))\nx = 1\nStringEncoding::Utf8\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    let print_line = lines.get(1).expect("expected first print output line");
    assert_eq!(strip_ansi(print_line), "xldr(1)> tick 1", "{stdout}");
    assert!(!print_line.contains("\u{1b}["), "{stdout}");

    let inspect_line = lines.get(2).expect("expected inspect output line");
    assert_eq!(strip_ansi(inspect_line), "xldr(2)> Ok(True)", "{stdout}");
    assert!(!inspect_line.contains("\u{1b}["), "{stdout}");

    assert!(stdout.contains("xldr(3)> \u{1b}[36mx\u{1b}[0m"), "{stdout}");
    assert!(
        stdout.contains("xldr(4)> \u{1b}[96mStringEncoding::Utf8\u{1b}[0m"),
        "{stdout}"
    );
}

#[test]
fn repl_error_summary_then_full_changes_diagnostic_detail() {
    let output =
        run_repl_session(":error summary\n:error bad\n:error full\nworse: Int = \"oops\"\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    let first_error_pos = stderr
        .find("Error: ReplCommandError")
        .expect("expected first ReplCommandError headline");
    let second_error_pos = stderr[first_error_pos + 1..]
        .find("Error: TypeError")
        .map(|offset| first_error_pos + 1 + offset)
        .expect("expected second TypeError headline");

    let first_block = &stderr[first_error_pos..second_error_pos];
    let second_block = &stderr[second_error_pos..];

    assert!(!first_block.contains("╭─["));
    assert!(
        first_block.contains("Use `:error full` or `:error summary`."),
        "{first_block}"
    );
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
    assert!(stderr.contains("This top-level declaration is not allowed in REPL chunks"));
}

#[test]
fn repl_script_preload_flag_exposes_preloaded_docs_and_defs() {
    let temp = unique_temp_dir("repl-script-preload");
    let source_path = temp.join("preload.srt");
    fs::write(
        &source_path,
        r#"
@doc """
Greets from preload.
"""
def greet() -> String { "hello" }
"#,
    )
    .expect("failed to write preload script");

    let output = run_repl_session_with_args(
        &[
            "--script",
            source_path.to_str().expect("source path must be utf-8"),
        ],
        ":doc greet\ngreet()\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("Greets from preload."), "{stdout}");
    assert!(stdout.contains("hello"), "{stdout}");

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn repl_script_preload_flag_prints_preload_runtime_output_before_prompt() {
    let temp = unique_temp_dir("repl-script-preload-output");
    let source_path = temp.join("preload_output.srt");
    fs::write(
        &source_path,
        r#"
print("boot message")
value = 42
"#,
    )
    .expect("failed to write preload script");

    let output = run_repl_session_with_args(
        &[
            "--quiet",
            "--script",
            source_path.to_str().expect("source path must be utf-8"),
        ],
        ":quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("boot message"), "{stdout}");
    assert!(stdout.contains("value: Int = 42"), "{stdout}");

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn repl_script_preload_flag_resolves_include_and_keeps_preloaded_binding() {
    let temp = unique_temp_dir("repl-script-preload-include");
    let module_path = temp.join("m.srt");
    let script_path = temp.join("a.srt");
    fs::write(
        &module_path,
        r#"
defmod M {
  def one() -> Int { 1 }
}
"#,
    )
    .expect("failed to write preload module");
    fs::write(
        &script_path,
        r#"
include "./m.srt"
import M::one
answer = one()
"#,
    )
    .expect("failed to write preload script");

    let output = run_repl_session_with_args(
        &[
            "--script",
            script_path.to_str().expect("script path must be utf-8"),
        ],
        "answer\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("1"), "{stdout}");

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn repl_script_preload_top_level_sig_has_surface_name_without_docs() {
    let temp = unique_temp_dir("repl-script-top-level-sig");
    let script_path = temp.join("top_level.srt");
    fs::write(
        &script_path,
        r#"
def greet(name: String) -> String { name }
"#,
    )
    .expect("failed to write preload script");

    let output = run_repl_session_with_args(
        &[
            "--script",
            script_path.to_str().expect("script path must be utf-8"),
        ],
        ":sig greet\n:doc greet\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("greet(name: String) -> String"), "{stdout}");
    assert!(stdout.contains("No docs found for greet"), "{stdout}");
    assert!(!stdout.contains("Global::"), "{stdout}");

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn repl_process_script_preload_survives_live_repl_process_declaration_rejection() {
    let temp = unique_temp_dir("repl-process-script-preload");
    let script_path = temp.join("process_preload.srt");
    fs::write(
        &script_path,
        r#"
defgenserver MyServer {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(1) }

  @call
  def size(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }
}

supervisor_init {
  MyServer {}
}
"#,
    )
    .expect("failed to write process preload script");

    let output = run_repl_session_with_args(
        &[
            "--script",
            script_path.to_str().expect("script path must be utf-8"),
        ],
        ":sig MyServer::size\nsupervisor_init { MyServer {} }\n:sig MyServer::size\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        stdout
            .matches("MyServer::size() -> Result<Int, Error>")
            .count(),
        2,
        "{stdout}"
    );
    assert!(
        stderr.contains("This top-level declaration is not allowed in REPL chunks"),
        "{stderr}"
    );

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn repl_process_pid_queries_cover_hidden_and_concrete_singleton_surfaces() {
    let temp = unique_temp_dir("repl-process-pid-query");
    let script_path = temp.join("process_pid_query.srt");
    fs::write(
        &script_path,
        r#"
defgenserver MyServer {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(1) }

  @call
  def size(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }
}

supervisor_init {
  MyServer {}
}
"#,
    )
    .expect("failed to write process preload script");

    let output = run_repl_session_with_args(
        &[
            "--script",
            script_path.to_str().expect("script path must be utf-8"),
        ],
        ":doc MyServer::pid\n:sig MyServer::pid\n:sig MyServer\nserver = MyServer::pid()\n:sig $server\n:type server\n:info server\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("MyServer::pid"), "{stdout}");
    assert!(
        stdout.contains("Compiler-managed lower target for GenServer singleton PID lookup."),
        "{stdout}"
    );
    assert!(
        stdout.contains("MyServer::pid() -> PID<MyServer>"),
        "{stdout}"
    );
    assert!(stdout.contains("GenServer MyServer"), "{stdout}");
    assert!(
        stdout.contains("@init init() -> Result<PID<MyServer>>"),
        "{stdout}"
    );
    assert!(stdout.contains("@pid pid() -> PID<MyServer>"), "{stdout}");
    assert!(
        stdout.contains("@call size(pid: PID<MyServer>) -> Result<Int, Error>"),
        "{stdout}"
    );
    assert!(stdout.contains("PID<MyServer> messaging"), "{stdout}");
    assert!(stdout.contains("server: PID<MyServer>"), "{stdout}");
    assert!(stdout.contains("type: PID<MyServer>"), "{stdout}");
    assert!(stdout.contains("kind: process pid"), "{stdout}");

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn repl_module_and_script_preload_flags_share_one_compile_unit() {
    let temp = unique_temp_dir("repl-module-script-preload");
    let module_path = temp.join("helper.srt");
    let script_path = temp.join("main.srt");
    fs::write(
        &module_path,
        r#"
defmod Helper {
  def inc(x: Int) -> Int { x + 1 }
}
"#,
    )
    .expect("failed to write preload module");
    fs::write(
        &script_path,
        r#"
import Helper::inc

def from_script() -> Int { inc(1) }
"#,
    )
    .expect("failed to write preload script");

    let output = run_repl_session_with_args(
        &[
            "--module",
            module_path.to_str().expect("module path must be utf-8"),
            "--script",
            script_path.to_str().expect("script path must be utf-8"),
        ],
        ":sig Helper::inc\n:sig from_script\nfrom_script()\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("Helper::inc(x: Int) -> Int"), "{stdout}");
    assert!(stdout.contains("from_script() -> Int"), "{stdout}");
    assert!(stdout.contains("2"), "{stdout}");
    assert!(!stdout.contains("Global::"), "{stdout}");

    let _ = fs::remove_dir_all(temp);
}

#[test]
fn repl_doc_and_sig_cover_tuple_scope_and_lens_queries() {
    let output = run_repl_session(
        ":doc Tuple\n:sig Tuple\n:doc Config\n:doc StyledDocStyle\n:doc add\nimport Add::add\n:doc add\npair = (\"alice\", 2)\nresult_pair = (Ok(2), \"ok\")\n:sig pair._1\n:sig Facet::over_result(Tuple._0, result_pair, {|value: Result<Int>| Ok(value)})\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("Tuple._0"), "{stdout}");
    assert!(stdout.contains("Tuple._1"), "{stdout}");
    assert!(stdout.contains("No signature found for Tuple"), "{stdout}");
    assert!(stdout.contains("defstruct Config"), "{stdout}");
    assert!(stdout.contains("defrecord StyledDocStyle"), "{stdout}");
    assert!(stdout.contains("No docs found for add"), "{stdout}");
    assert!(stdout.contains("Imported Add::add"), "{stdout}");
    assert!(stdout.contains("Add::add"), "{stdout}");
    assert!(stdout.contains("pair._1: Int"), "{stdout}");
    assert!(
        stderr.contains("Unsupported command query argument `Tuple._0`"),
        "{stderr}"
    );
}

#[test]
fn repl_colorizes_closure_doc_footer_and_type_output() {
    let output =
        run_repl_session_with_color("c = {|x: Int, y: Int| x + y}\n:doc $c\n:type c\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let plain = strip_ansi(&stdout);
    assert!(stdout.contains("\u{1b}["), "{stdout}");
    assert!(plain.contains("type: (Int, Int -> Int)"), "{plain}");
    assert!(plain.contains("example: ret: Int = c(Int, Int)"), "{plain}");
    assert!(plain.contains("identity: TypeIdentity::Closure"), "{plain}");
}

#[test]
fn repl_supports_deferred_lens_bindings_and_lens_command() {
    let output = run_repl_session(
        "a = Tuple._1\npair = (\"alice\", 2)\nFacet::view(a, pair)\n:facet a\n:facet BitWidth.Any\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(stdout.contains("a: Facet<_, _> = Tuple._1"), "{stdout}");
    assert!(stdout.contains("2"), "{stdout}");
    assert!(stdout.contains("FacetPath"), "{stdout}");
    assert!(stdout.contains("view result: _"), "{stdout}");
    assert!(stdout.contains("full path: Tuple._1"), "{stdout}");
    assert!(stdout.contains("Flow"), "{stdout}");
    assert!(stdout.contains("hop 1: Tuple._1"), "{stdout}");
    assert!(stdout.contains("Stops"), "{stdout}");
    assert!(stdout.contains("stop 1:"), "{stdout}");
    assert!(
        stdout.contains("view result: Result<Int, Error>"),
        "{stdout}"
    );
    assert!(
        stdout.contains("variant mismatch returns Result"),
        "{stdout}"
    );
}

#[test]
fn repl_renders_top_level_lens_chain_expressions() {
    let output =
        run_repl_session("ep = IntBase.Oct\na = Tuple._1\na / ep\nFacet::chain(a, ep)\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("ep: Facet<IntBase, Unit> = IntBase.Oct"),
        "{stdout}"
    );
    assert!(stdout.contains("a: Facet<_, _> = Tuple._1"), "{stdout}");
    assert!(stdout.contains("Facet<_, _> = Tuple._1.Oct"), "{stdout}");
}

#[test]
fn repl_reports_return_mismatch_for_concretized_trait_helper_closure() {
    let output = run_repl_session(
        "f: (String, String -> Unit) = {|x: String, y: String| concat(x, y)}\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = strip_ansi(&combined);
    assert!(
        combined.contains("Argument type mismatch: expected Unit, got String"),
        "expected return mismatch, got:\n{}",
        combined
    );
    assert!(
        !combined.contains("could not use the current closure constraints"),
        "trait helper should have concretized before return mismatch:\n{}",
        combined
    );
}

#[test]
fn repl_allows_trait_helper_capture_with_expected_callable_annotation() {
    let output = run_repl_session(
        "cmp: (Int, Int -> Ordering) = &compare\njoin: (String, String -> String) = &concat\ncmp(1, 2)\njoin(\"sur\", \"tr\")\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("cmp: (Int, Int -> Ordering) = Closure(Int, Int -> Ordering)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("join: (String, String -> String) = Closure(String, String -> String)"),
        "{stdout}"
    );
    assert!(stdout.contains("Ordering::Less"), "{stdout}");
    assert!(stdout.contains("\"surtr\""), "{stdout}");
}

#[test]
fn repl_allows_trait_helper_capture_when_function_on_supplies_same_expression_evidence() {
    let output = run_repl_session(
        "by_len = &compare `Function::on` &String::len\nby_len(\"a\", \"abcd\")\n:quit\n",
    );
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("by_len: (String, String -> Ordering) = Closure(_, _ -> _)"),
        "{stdout}"
    );
    assert!(stdout.contains("Ordering::Less"), "{stdout}");
}

#[test]
fn repl_rejects_function_on_inferred_facet_capture_without_source_evidence() {
    let output = run_repl_session("by_age = &compare `Function::on` _.age\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = strip_ansi(&combined);
    assert!(
        combined.contains("Cannot access field on"),
        "expected missing source evidence diagnostic, got:\n{}",
        combined
    );
}

#[test]
fn repl_keeps_bare_trait_helper_capture_unresolved_without_same_expression_evidence() {
    let output = run_repl_session("cmp = &compare\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = strip_ansi(&combined);
    assert!(
        combined.contains("Trait helper `compare` needs expected callable type or same-expression inference evidence"),
        "expected unresolved capture diagnostic, got:\n{}",
        combined
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
