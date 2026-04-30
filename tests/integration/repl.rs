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

struct SharedReplCase {
    name: &'static str,
    input: &'static str,
    assert_output: fn(&SharedReplCaseOutput<'_>),
}

struct SharedReplCaseOutput<'a> {
    stdout: &'a str,
    stderr: &'a str,
}

fn run_shared_repl_cases(cases: &[SharedReplCase]) {
    let output = run_repl_session(&build_shared_repl_input(cases));
    assert!(
        output.status.success(),
        "shared repl session failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.trim().is_empty(),
        "shared repl cases produced unexpected stderr\nstdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );

    for case in cases {
        let stdout_segment = extract_shared_repl_segment(
            &stdout,
            &shared_repl_marker(case.name, "start"),
            &shared_repl_marker(case.name, "end"),
        );
        (case.assert_output)(&SharedReplCaseOutput {
            stdout: stdout_segment,
            stderr: "",
        });
    }
}

fn build_shared_repl_input(cases: &[SharedReplCase]) -> String {
    let mut input = String::new();

    for case in cases {
        input.push_str(&format!(
            "print(\"{}\")\n",
            shared_repl_marker(case.name, "start")
        ));
        input.push_str(case.input);
        if !case.input.ends_with('\n') {
            input.push('\n');
        }
        input.push_str(&format!(
            "print(\"{}\")\n",
            shared_repl_marker(case.name, "end")
        ));
    }

    input.push_str(":quit\n");
    input
}

fn shared_repl_marker(case_name: &str, edge: &str) -> String {
    format!("__surtr_repl_case__{edge}__{case_name}__")
}

fn extract_shared_repl_segment<'a>(text: &'a str, start_marker: &str, end_marker: &str) -> &'a str {
    let start = text
        .find(start_marker)
        .unwrap_or_else(|| panic!("missing shared REPL start marker `{start_marker}`"))
        + start_marker.len();
    let tail = &text[start..];
    let end = tail
        .find(end_marker)
        .unwrap_or_else(|| panic!("missing shared REPL end marker `{end_marker}`"));
    &tail[..end]
}

fn extract_prompt_numbers(stdout: &str) -> Vec<u32> {
    let mut numbers = Vec::new();
    let mut remaining = stdout;

    while let Some(prompt_start) = remaining.find("xldr(") {
        let digits = &remaining[prompt_start + 5..];
        let Some(prompt_end) = digits.find(")>") else {
            break;
        };
        let number = &digits[..prompt_end];
        if let Ok(value) = number.parse::<u32>() {
            numbers.push(value);
        }
        remaining = &digits[prompt_end + 2..];
    }

    numbers
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
    assert!(
        stdout.contains("Surtr xldr"),
        "expected lightweight banner in repl output, got:\n{}",
        stdout
    );
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
    assert!(
        !stdout.contains("Surtr xldr"),
        "expected quiet mode to suppress the banner, got:\n{}",
        stdout
    );
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
    assert!(
        stdout.contains("██\\   ██\\ ██\\      ██████\\  ██████\\"),
        "expected detailed banner in repl output, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("\\__|  \\__|\\_______|\\______/ \\__|  \\__|"),
        "expected detailed banner command help in repl output, got:\n{}",
        stdout
    );
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
    assert!(
        stdout.trim() == "xldr 0.1.0",
        "expected xldr version output, got:\n{}",
        stdout
    );
}

#[test]
fn repl_shared_success_cases_cover_bindings_and_pure_eval() {
    run_shared_repl_cases(&[
        SharedReplCase {
            name: "keeps_bindings_between_inputs",
            input: "x = 42\nx\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("x: Int = 42"),
                    "expected bind echo in repl output, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains("> 42"),
                    "expected expression result in repl output, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "echoes_bindings_even_with_trailing_semicolons",
            input: "n = 1;w = 2; r = 3;\n",
            assert_output: |output| {
                for expected in ["n: Int = 1", "w: Int = 2", "r: Int = 3"] {
                    assert!(
                        output.stdout.contains(expected),
                        "expected semicolon-terminated binding `{}` to be echoed, got:\n{}",
                        expected,
                        output.stdout
                    );
                }
            },
        },
        SharedReplCase {
            name: "infers_closure_argument_type_from_add_constraint",
            input: "fun = {|num| num + 5}\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("fun: (Int -> Int)"),
                    "expected closure argument type to infer as Int, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "displays_const_helper_with_hole_callable_surface",
            input: "always = const(1)\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("always: (_ -> Int)"),
                    "expected const helper to display Hole callable surface, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "auto_imports_concat_trait_helper",
            input: "concat(\"q\", \"q\")\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("qq"),
                    "expected concat helper result in repl output, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "accepts_top_level_function_definition",
            input: "def add_shared_bucket_basic(x: Int, y: Int) -> Int { x + y }\nadd_shared_bucket_basic(1, 2)\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("> 3"),
                    "expected function call result in repl output, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "ok_defaults_result_error_type_to_error",
            input: "ret = Ok(10)\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("ret: Result<Int, Error> = Ok(10)"),
                    "expected Ok binding to default err side to Error, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "hides_internal_type_var_ids_in_result_display",
            input: "ret_e = Err(NoneError)\n",
            assert_output: |output| {
                assert!(
                    output
                        .stdout
                        .contains("ret_e: Result<_, Error> = Err(NoneError(\"None Value.\"))"),
                    "expected Result type vars to be hidden in repl output, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "safebind_constructor_pattern_echoes_binding",
            input: "ret = Ok(1)\nrr = Ok(ret)\nOk(num) =? rr\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("num: Int = 1"),
                    "expected constructor-pattern safebind to echo num binding, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "safebind_list_pattern_echoes_all_bindings",
            input: "rv: Result<List<Int>> = Ok([1, 2, 3])\n[h, ..t] =? rv\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("h: Int = 1"),
                    "expected list-pattern safebind to echo h binding, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains("t: List<Int> = [2, 3]"),
                    "expected list-pattern safebind to echo t binding, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "safebind_list_pattern_accepts_plain_list_rhs",
            input: "li = [1, 2, 3]\n[h, ..t] =? li\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("h: Int = 1"),
                    "expected list-pattern safebind on plain list rhs to echo h binding, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains("t: List<Int> = [2, 3]"),
                    "expected list-pattern safebind on plain list rhs to echo t binding, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "safebind_list_pattern_accepts_nested_constructor_literals",
            input: "lr = [Ok(1), Ok(2), Ok(3)]\n[Ok(1), Ok(2), _] =? lr\n",
            assert_output: |output| {
                assert!(
                    output.stderr.is_empty(),
                    "expected no parse error for nested constructor list pattern, got:\n{}",
                    output.stderr
                );
            },
        },
        SharedReplCase {
            name: "safebind_list_pattern_accepts_nested_constructor_with_tail",
            input: "lr = [Ok(1), Ok(2), Ok(3)]\n[Ok(1), ..tail] =? lr\n",
            assert_output: |output| {
                assert!(
                    output
                        .stdout
                        .contains("tail: List<Result<Int, Error>> = [Ok(2), Ok(3)]"),
                    "expected nested constructor with tail to bind tail, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "safe_xxx_zero_uses_zero_division_error",
            input: "print(inspect(safe_div(1, 0)))\nprint(inspect(safe_mod(1, 0)))\n",
            assert_output: |output| {
                assert!(
                    output
                        .stdout
                        .contains("Err(ZeroDivisionError(\"division by zero\"))"),
                    "expected ZeroDivisionError display in repl output, got:\n{}",
                    output.stdout
                );
                assert_eq!(
                    output
                        .stdout
                        .matches("Err(ZeroDivisionError(\"division by zero\"))")
                        .count(),
                    2,
                    "expected both safe_div and safe_mod to use ZeroDivisionError, got:\n{}",
                    output.stdout
                );
            },
        },
    ]);
}

#[test]
fn repl_rejects_top_level_struct_definition() {
    let output = run_repl_session("defstruct User { name: String }\n:quit\n");
    assert!(
        output.status.success(),
        "repl should remain alive after parse error\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        stderr.contains("This top-level declaration is not allowed in the current source policy"),
        "expected declaration policy parse error, got:\n{}",
        stderr
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
fn repl_shared_success_cases_cover_reference_commands() {
    run_shared_repl_cases(&[
        SharedReplCase {
            name: "doc_command_shows_builtin_docs",
            input: ":doc print\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("Kernel::print"),
                    "expected :doc to resolve the builtin symbol, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains("Kernel::print(a: String) -> Unit"),
                    "expected :doc to print the builtin signature banner, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains("Print a string to stdout."),
                    "expected :doc to print the builtin summary, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "sig_command_shows_builtin_and_local_function_signatures",
            input: ":sig print\ndef add_shared_bucket_sig(x: Int, y: Int) -> Int { x + y }\n:sig add_shared_bucket_sig\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("Kernel::print(a: String) -> Unit"),
                    "expected :sig to print the builtin signature, got:\n{}",
                    output.stdout
                );
                assert!(
                    output
                        .stdout
                        .contains("__Repl::Session::add_shared_bucket_sig(x: Int, y: Int) -> Int"),
                    "expected :sig to print the REPL function signature, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "help_command_lists_available_commands",
            input: ":help\n",
            assert_output: |output| {
                for expected in [
                    "REPL commands:",
                    ":help, :h [command]",
                    ":quit, :exit",
                    ":doc <symbol>",
                    ":sig <symbol>",
                    ":error [full|summary]",
                    ":save <path.eldr>",
                    ":v <line>",
                ] {
                    assert!(
                        output.stdout.contains(expected),
                        "expected help output to contain `{}`, got:\n{}",
                        expected,
                        output.stdout
                    );
                }
            },
        },
        SharedReplCase {
            name: "help_sig_topic_shows_sig_usage",
            input: ":h sig\n:h :sig\n:sig\n",
            assert_output: |output| {
                assert!(
                    output.stdout.matches("Usage: :sig <function>").count() >= 3,
                    "expected sig help forms to show sig usage, got:\n{}",
                    output.stdout
                );
                assert!(
                    output
                        .stdout
                        .contains("Examples: :sig print, :sig Kernel::if, :sig add"),
                    "expected sig help examples, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "help_doc_topic_shows_doc_usage",
            input: ":h doc\n:h :doc\n",
            assert_output: |output| {
                assert!(
                    output.stdout.matches("Usage: :doc <symbol>").count() >= 2,
                    "expected both doc help forms to show doc usage, got:\n{}",
                    output.stdout
                );
                assert!(
                    output
                        .stdout
                        .contains("Examples: :doc print, :doc Kernel::if, :doc Add, :doc +"),
                    "expected doc help examples, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "doc_without_symbol_shows_doc_help",
            input: ":doc\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("Usage: :doc <symbol>"),
                    "expected :doc without a symbol to show doc help, got:\n{}",
                    output.stdout
                );
                assert!(
                    output
                        .stdout
                        .contains("Examples: :doc print, :doc Kernel::if, :doc Add, :doc +"),
                    "expected :doc help examples, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "unknown_command_suggests_help_and_keeps_session_alive",
            input: ":nope\n1\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("Unknown REPL command: :nope"),
                    "expected unknown command message, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains("Type :help for available REPL commands."),
                    "expected help suggestion, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains("> 1"),
                    "expected session to continue after unknown command, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "doc_command_resolves_operator_trait_aliases",
            input: ":doc Add\n:doc +\n:doc |*>\n:doc |>=\n",
            assert_output: |output| {
                assert!(
                    output
                        .stdout
                        .matches("trait Add { add(self: Self, rhs: Self) -> Self }")
                        .count()
                        >= 2,
                    "expected both :doc Add and :doc + to render Add docs, got:\n{}",
                    output.stdout
                );
                assert!(
                    output
                        .stdout
                        .matches("Standard `Add` operator trait declaration.")
                        .count()
                        >= 2,
                    "expected both doc lookups to print the Add summary, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains("Standard `Functor` trait declaration."),
                    "expected :doc |*> to render Functor docs from source, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains("Standard `Chainable` trait declaration."),
                    "expected :doc |>= to render Chainable docs from source, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "doc_command_lists_ambiguous_trait_method_candidates",
            input: ":doc gt\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("gt has multiple docs:"),
                    "expected :doc gt to show candidates, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains("impl Gt for Int::gt")
                        && output.stdout.contains("impl Gt for Float::gt"),
                    "expected Int and Float gt candidates, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "doc_typed_call_resolves_trait_impl_docs_without_evaluation",
            input: ":doc gt(3, 2)\n:doc gt(2.0, 1.5)\n",
            assert_output: |output| {
                assert!(
                    output
                        .stdout
                        .contains("impl Gt for Int::gt(self: Self, rhs: Self) -> Boolean"),
                    "expected Int gt impl docs, got:\n{}",
                    output.stdout
                );
                assert!(
                    output
                        .stdout
                        .contains("impl Gt for Float::gt(self: Self, rhs: Self) -> Boolean"),
                    "expected Float gt impl docs, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "doc_typed_call_accepts_type_placeholders_and_rejects_calls",
            input: ":doc gt(_ : Float, _ : Float)\n:doc gt(Float, Float)\n:doc gt(make_value(), 1)\n",
            assert_output: |output| {
                assert!(
                    output
                        .stdout
                        .matches("impl Gt for Float::gt(self: Self, rhs: Self) -> Boolean")
                        .count()
                        >= 2,
                    "expected Float gt impl docs, got:\n{}",
                    output.stdout
                );
                assert!(
                    output
                        .stdout
                        .contains("Unsupported typed call query argument `make_value()`"),
                    "expected non-evaluating query rejection, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "doc_and_sig_render_without_result_prefix_and_dedent_doc_body",
            input: ":sig gt(Int, Int)\n:doc gt(Int, Int)\n",
            assert_output: |output| {
                assert!(
                    !output.stdout.contains("\n> defined:")
                        && !output.stdout.contains("\n> specialized:")
                        && !output.stdout.contains("\n> Return `True`"),
                    "expected :doc/:sig output without result prefix, got:\n{}",
                    output.stdout
                );
                assert!(
                    output
                        .stdout
                        .contains("\ndefined:\n  impl Gt for Int::gt(self: Self, rhs: Self) -> Boolean")
                        || output.stdout.contains(
                            "> defined:\n  impl Gt for Int::gt(self: Self, rhs: Self) -> Boolean"
                        ),
                    "expected defined signature block, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains(
                        "\nReturn `True` when the left integer is strictly greater than the right integer."
                    ),
                    "expected doc body to be dedented, got:\n{}",
                    output.stdout
                );
                assert!(
                    !output.stdout.contains(
                        "\n  Return `True` when the left integer is strictly greater than the right integer."
                    ),
                    "expected doc body not to keep source indentation, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "doc_command_resolves_flow_operator_trait_aliases",
            input: ":doc |>\n:doc >>\n:doc >*\n:doc >=>\n",
            assert_output: |output| {
                assert!(
                    output
                        .stdout
                        .contains("Standard `PipeApply` flow operator trait declaration."),
                    "expected :doc |> to render PipeApply docs from source, got:\n{}",
                    output.stdout
                );
                assert!(
                    output
                        .stdout
                        .contains("Standard `Composable` flow operator trait declaration."),
                    "expected :doc >> to render Composable docs from source, got:\n{}",
                    output.stdout
                );
                assert!(
                    output
                        .stdout
                        .contains("Standard `LiftComposable` flow operator trait declaration."),
                    "expected :doc >* to render LiftComposable docs from source, got:\n{}",
                    output.stdout
                );
                assert!(
                    output
                        .stdout
                        .contains("Standard `KleisliComposable` flow operator trait declaration."),
                    "expected :doc >=> to render KleisliComposable docs from source, got:\n{}",
                    output.stdout
                );
            },
        },
    ]);
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
    let prompt_numbers = extract_prompt_numbers(&stdout);
    assert!(
        prompt_numbers.starts_with(&[1, 2]),
        "expected numbered prompts for the first two inputs, got:\n{}",
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
fn repl_colorizes_sig_command_signature() {
    let output = run_repl_session_with_color(":sig print\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\u{1b}["),
        "expected ANSI styling for :sig print, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("\u{1b}[36ma\u{1b}[0m"),
        "expected parameter name styling inside :sig output, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("\u{1b}[1;96mString\u{1b}[0m")
            && stdout.contains("\u{1b}[1;96mUnit\u{1b}[0m"),
        "expected type styling inside :sig output, got:\n{}",
        stdout
    );
    assert!(
        strip_ansi(&stdout).contains("Kernel::print(a: String) -> Unit"),
        "expected styled :sig output to preserve plain text, got:\n{}",
        stdout
    );
}

#[test]
fn repl_colorizes_doc_for_qualified_kernel_if() {
    let output = run_repl_session(":doc Kernel::if\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Kernel::if"),
        "expected :doc to resolve Kernel::if, got:\n{}",
        stdout
    );

    let output = run_repl_session_with_color(":doc Kernel::if\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\u{1b}["),
        "expected ANSI styling for :doc Kernel::if, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("\u{1b}[43m") && !stdout.contains(";43m"),
        "expected no background styling for :doc signature banner, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("\u{1b}[36mflag\u{1b}[0m"),
        "expected parameter name styling inside signature banner, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("\u{1b}[1;96mBoolean\u{1b}[0m"),
        "expected type styling inside signature banner, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("\u{1b}[1;33m$A\u{1b}[0m"),
        "expected generic type styling inside signature banner, got:\n{}",
        stdout
    );
    assert!(
        strip_ansi(&stdout).contains("xldr(1)> if(True, \"ok\", \"ng\")"),
        "expected styled doc examples to preserve plain text, got:\n{}",
        stdout
    );
}

#[test]
fn repl_shared_success_cases_cover_callable_and_auxiliary_views() {
    run_shared_repl_cases(&[
        SharedReplCase {
            name: "error_command_switches_display_mode",
            input: ":error\n:error summary\n:error\n:error full\n:error\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("> error display mode: full"),
                    "expected default error display mode to be full, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains("> error display mode: summary"),
                    "expected :error summary to update mode, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "displays_bare_std_callable_refs_with_named_inspect_format",
            input: "&Int::shr\n&Boolean::xor\nprint(inspect(&Boolean::xor))\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains(
                        "FnCapture(module: Int, name: shr, signature: shr(value: Int, bits: Int) -> Result<Int, NegativeShiftCount>)"
                    ),
                    "expected builtin callable inspect format, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains(
                        "FnCapture(module: Boolean, name: xor, signature: xor(left: Boolean, right: Boolean) -> Boolean)"
                    ),
                    "expected function callable inspect format, got:\n{}",
                    output.stdout
                );
                assert_eq!(
                    output
                        .stdout
                        .matches(
                            "FnCapture(module: Boolean, name: xor, signature: xor(left: Boolean, right: Boolean) -> Boolean)"
                        )
                        .count(),
                    2,
                    "expected bare display and inspect(...) to agree for Boolean::xor, got:\n{}",
                    output.stdout
                );
            },
        },
        SharedReplCase {
            name: "concat_helper_works_inside_annotated_closure",
            input: "f = {|x: String, y: String| concat(x,y)}\nf(\"a\",\"b\")\n",
            assert_output: |output| {
                let stdout = strip_ansi(output.stdout);
                assert!(
                    stdout.contains("f: (String, String -> String)"),
                    "expected closure to infer String concat signature, got:\n{}",
                    stdout
                );
                assert!(
                    stdout.contains("> \"ab\"") || stdout.contains("> ab"),
                    "expected closure call to concatenate strings, got:\n{}",
                    stdout
                );
            },
        },
        SharedReplCase {
            name: "displays_local_function_refs_with_named_inspect_format",
            input: "def add_shared_bucket_local_ref(x: Int, y: Int) -> Int { x + y }\n&add_shared_bucket_local_ref\nprint(inspect(&add_shared_bucket_local_ref))\n",
            assert_output: |output| {
                assert!(
                    output.stdout.contains("FnCapture(module:"),
                    "expected named callable inspect output, got:\n{}",
                    output.stdout
                );
                assert!(
                    output.stdout.contains(
                        "name: add_shared_bucket_local_ref, signature: add_shared_bucket_local_ref(x: Int, y: Int) -> Int)"
                    ),
                    "expected local function signature in inspect output, got:\n{}",
                    output.stdout
                );
            },
        },
    ]);
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

    assert!(
        !first_block.contains("╭─["),
        "summary mode should collapse diagnostic to one line, got:\n{}",
        first_block
    );
    assert!(
        second_block.contains("╭─["),
        "full mode should include source snippet block, got:\n{}",
        second_block
    );
}

#[test]
fn repl_rejects_bare_trait_helper_callable_refs() {
    let output = run_repl_session("&concat\n:quit\n");
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
        combined.contains("Trait helper `concat` cannot be referenced directly"),
        "expected bare trait helper ref to be rejected, got:\n{}",
        combined
    );
    assert!(
        !combined.contains("FnCapture(module: Result, name: chain"),
        "bare trait helper ref must not reuse an unrelated function id, got:\n{}",
        combined
    );
}

#[test]
fn repl_save_command_writes_decodable_eldr_snapshot() {
    let dir = unique_temp_dir("repl-save");
    let save_base = dir.join("session");
    let input = format!("x = 1\n:save {}\n:quit\n", save_base.to_string_lossy());
    let output = run_repl_session(&input);
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let saved_path = save_base.with_extension("eldr");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("saved to {}", saved_path.display())),
        "expected :save to report the output path, got:\n{}",
        stdout
    );

    let bytes = fs::read(&saved_path).expect("saved .eldr snapshot should exist");
    forge::bytecode::Bytecode::decode(&bytes).expect("saved .eldr snapshot should decode");

    fs::remove_dir_all(&dir).expect("failed to clean temp dir");
}

#[test]
fn repl_rejects_top_level_deferror_definition() {
    let output = run_repl_session("deferror PageNotFound(html: String) { html }\n:quit\n");
    assert!(
        output.status.success(),
        "repl should remain alive after parse error\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        stderr.contains("This top-level declaration is not allowed in the current source policy"),
        "expected declaration policy parse error, got:\n{}",
        stderr
    );
}

#[test]
fn repl_evaluates_main_result_err_immediately() {
    let output = run_repl_session("Err(NoneError)\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("> Err("),
        "expected Err result to be evaluated immediately instead of echoed, got:\n{}",
        stdout
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("NoneError"),
        "expected evaluated Err to be reported in stderr, got:\n{}",
        stderr
    );
}

#[test]
fn repl_colorizes_plain_list_safebind_bindings() {
    let output = run_repl_session_with_color("[h, ..t] =? [1, 2, 3]\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\u{1b}["),
        "expected ANSI styling for safebind result, got:\n{}",
        stdout
    );

    let plain = strip_ansi(&stdout);
    assert!(
        plain.contains("h: Int = 1"),
        "expected h binding to be preserved, got:\n{}",
        stdout
    );
    assert!(
        plain.contains("t: List<Int> = [2, 3]"),
        "expected t binding to be preserved, got:\n{}",
        stdout
    );
}

#[test]
fn repl_colorizes_constructor_return_without_type_style_bleed() {
    let output = run_repl_session_with_color("Ok(1)\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\u{1b}[1;35mOk\u{1b}[0m"),
        "expected constructor token styling for Ok(1), got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("\u{1b}[96mOk(1)\u{1b}[0m"),
        "expected Ok(1) not to be styled as a type definition line, got:\n{}",
        stdout
    );
}

#[test]
fn repl_safe_mod_runtime_error_highlights_the_full_call() {
    let output = run_repl_session("safe_mod(10, 0)\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        stderr.contains("ZeroDivisionError"),
        "expected zero division error in stderr, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("REPL:1:1"),
        "expected runtime error to point at the start of the call, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("safe_mod(10, 0)"),
        "expected diagnostic to include the full call site, got:\n{}",
        stderr
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
fn repl_add_trait_errors_list_available_implementations() {
    let output = run_repl_session("Add::add(1,False)\nAdd::add(False, True)\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        stderr.contains("Add::add expects argument 2 to match receiver type Int, got Boolean"),
        "expected Add mismatch detail in stderr, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Add::add requires a receiver type implementing Add, got Boolean"),
        "expected missing receiver impl detail in stderr, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Add is implemented for: Float, Int"),
        "expected trait implementation list in stderr, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Help: Call target signature: Add::add("),
        "expected trait call signature help in stderr, got:\n{}",
        stderr
    );
}

#[test]
fn repl_call_errors_show_target_signature() {
    let output = run_repl_session("print()\nprint(1)\nAdd::add(2, \"a\")\n:quit\n");
    assert!(
        output.status.success(),
        "repl failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    assert!(
        stderr.contains("Help: Call target signature: Kernel::print(arg1: String) -> Unit"),
        "expected builtin call signature help in stderr, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Help: Call target signature: Add::add("),
        "expected Add call signature help in stderr, got:\n{}",
        stderr
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
    assert!(
        stderr.contains("REPL:2:"),
        "expected repl error to point at generation site within the current repl chunk, got:\n{}",
        stderr
    );
}
