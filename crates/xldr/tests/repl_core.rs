use std::fs;

use xldr::repl::logic::{ReplOutput, ReplResult};
use xldr::ReplEngine;

fn engine() -> ReplEngine {
    ReplEngine::new().expect("REPL engine should bootstrap")
}

fn rendered(result: &ReplResult) -> &[String] {
    match &result.output {
        ReplOutput::EvalSuccess { rendered, .. }
        | ReplOutput::EvalError { rendered, .. }
        | ReplOutput::CommandOutput { rendered } => rendered,
        ReplOutput::DocResolved {
            symbol,
            signature,
            summary,
            source_snippet,
        } => {
            panic!(
                "expected rendered output, got doc resolved: symbol={symbol}, signature={signature:?}, summary={summary:?}, source_snippet={source_snippet:?}"
            )
        }
        ReplOutput::SigResolved { signature } => {
            panic!("expected rendered output, got signature: {signature}")
        }
        ReplOutput::StatusMessage(message) => {
            panic!("expected rendered output, got status: {message}")
        }
        ReplOutput::EvalStarted { idx, source } => {
            panic!("expected rendered output, got eval start: idx={idx}, source={source}")
        }
    }
}

fn rendered_text(result: &ReplResult) -> String {
    rendered(result).join("\n")
}

fn doc_text(result: &ReplResult) -> String {
    match &result.output {
        ReplOutput::DocResolved {
            symbol,
            signature,
            summary,
            source_snippet,
        } => [
            symbol.clone(),
            signature.clone().unwrap_or_default(),
            summary.clone().unwrap_or_default(),
            source_snippet.clone().unwrap_or_default(),
        ]
        .join("\n"),
        ReplOutput::CommandOutput { rendered } => rendered.join("\n"),
        other => panic!("expected doc output, got {}", output_kind(other)),
    }
}

fn signature_text(result: &ReplResult) -> String {
    match &result.output {
        ReplOutput::SigResolved { signature } => signature.clone(),
        ReplOutput::CommandOutput { rendered } => rendered.join("\n"),
        other => panic!("expected signature output, got {}", output_kind(other)),
    }
}

fn status_text(result: &ReplResult) -> String {
    match &result.output {
        ReplOutput::StatusMessage(message) => message.clone(),
        other => panic!("expected status output, got {}", output_kind(other)),
    }
}

fn output_kind(output: &ReplOutput) -> &'static str {
    match output {
        ReplOutput::EvalStarted { .. } => "EvalStarted",
        ReplOutput::EvalSuccess { .. } => "EvalSuccess",
        ReplOutput::EvalError { .. } => "EvalError",
        ReplOutput::CommandOutput { .. } => "CommandOutput",
        ReplOutput::DocResolved { .. } => "DocResolved",
        ReplOutput::SigResolved { .. } => "SigResolved",
        ReplOutput::StatusMessage(_) => "StatusMessage",
    }
}

#[test]
fn core_keeps_bindings_and_definitions_between_inputs() {
    let mut engine = engine();

    let bind = engine.handle_line("x = 42");
    assert!(!bind.should_exit);
    assert!(rendered_text(&bind).contains("x: Int = 42"));

    let value = engine.handle_line("x");
    assert!(!value.should_exit);
    assert!(rendered_text(&value).contains("42"));

    let def = engine.handle_line("def add_core(x: Int, y: Int) -> Int { x + y }");
    assert!(!def.should_exit);

    let call = engine.handle_line("add_core(1, 2)");
    assert!(!call.should_exit);
    assert!(rendered_text(&call).contains("3"));
}

#[test]
fn core_rolls_back_failed_input_without_losing_previous_state() {
    let mut engine = engine();

    let bind = engine.handle_line("x = 1");
    assert!(rendered_text(&bind).contains("x: Int = 1"));

    let err = engine.handle_line("bad: Int = \"oops\"");
    assert!(!err.should_exit);
    assert!(matches!(err.output, ReplOutput::EvalError { .. }));
    assert!(rendered_text(&err).contains("expected Int, got String"));

    let value = engine.handle_line("x");
    assert!(!value.should_exit);
    assert!(rendered_text(&value).contains("1"));
}

#[test]
fn core_rejects_repl_forbidden_top_level_declarations() {
    let mut engine = engine();

    let err = engine.handle_line("defstruct User { name: String }");
    assert!(!err.should_exit);
    assert!(matches!(err.output, ReplOutput::EvalError { .. }));
    assert!(rendered_text(&err)
        .contains("This top-level declaration is not allowed in the current source policy"));
}

#[test]
fn core_commands_do_not_require_a_cli_process() {
    let mut engine = engine();

    let help = engine.handle_line(":help");
    assert!(rendered_text(&help).contains("REPL commands:"));

    let doc = engine.handle_line(":doc print");
    let doc = doc_text(&doc);
    assert!(doc.contains("Kernel::print"));
    assert!(doc.contains("Print a string to stdout."));

    let sig = engine.handle_line(":sig print");
    assert!(signature_text(&sig).contains("Kernel::print(a: String) -> Unit"));

    let unknown = engine.handle_line(":nope");
    assert!(!unknown.should_exit);
    assert!(rendered_text(&unknown).contains("Unknown REPL command: :nope"));
}

#[test]
fn core_help_and_error_commands_return_structured_command_output() {
    let mut engine = engine();

    let help = engine.handle_line(":help");
    let help_text = rendered_text(&help);
    assert!(help_text.contains("REPL commands:"));
    assert!(help_text.contains(":save <path.eldr>"));

    let sig_help = engine.handle_line(":h sig");
    assert!(rendered_text(&sig_help).contains("Usage: :sig <function>"));

    let error_default = engine.handle_line(":error");
    assert!(rendered_text(&error_default).contains("error display mode: full"));

    let error_summary = engine.handle_line(":error summary");
    assert!(rendered_text(&error_summary).contains("error display mode: summary"));

    let error_full = engine.handle_line(":error full");
    assert!(rendered_text(&error_full).contains("error display mode: full"));
}

#[test]
fn core_value_recall_uses_engine_history_and_prompt_index() {
    let mut engine = engine();
    assert_eq!(engine.prompt(), "xldr(1)> ");

    let first = engine.handle_line("5");
    assert!(rendered_text(&first).contains("5"));
    assert_eq!(engine.prompt(), "xldr(2)> ");

    let recalled = engine.handle_line(":v 1");
    assert!(rendered_text(&recalled).contains("5"));
    assert_eq!(engine.prompt(), "xldr(3)> ");
}

#[test]
fn core_result_error_reports_diagnostic_without_exiting() {
    let mut engine = engine();

    let result_err = engine.handle_line("Err(NoneError)");
    assert!(!result_err.should_exit);
    assert!(matches!(result_err.output, ReplOutput::EvalError { .. }));
    assert!(rendered_text(&result_err).contains("None Value."));

    let safe_mod = engine.handle_line("safe_mod(10, 0)");
    assert!(!safe_mod.should_exit);
    assert!(matches!(safe_mod.output, ReplOutput::EvalError { .. }));
    assert!(rendered_text(&safe_mod).contains("division by zero"));
}

#[test]
fn core_doc_and_sig_commands_resolve_aliases_and_typed_queries() {
    let mut engine = engine();

    let builtin_doc = engine.handle_line(":doc print");
    let builtin_doc = doc_text(&builtin_doc);
    assert!(builtin_doc.contains("Kernel::print"));
    assert!(builtin_doc.contains("Print a string to stdout."));

    let alias_doc = engine.handle_line(":doc +");
    let alias_doc = doc_text(&alias_doc);
    assert!(alias_doc.contains("trait Add { add(self: Self, rhs: Self) -> Self }"));
    assert!(alias_doc.contains("Standard `Add` operator trait declaration."));

    let typed_sig = engine.handle_line(":sig gt(Int, Int)");
    let typed_sig = signature_text(&typed_sig);
    assert!(typed_sig.contains("impl Gt for Int::gt(self: Self, rhs: Self) -> Boolean"));

    let typed_doc = engine.handle_line(":doc gt(Int, Int)");
    let typed_doc = doc_text(&typed_doc);
    assert!(typed_doc.contains("impl Gt for Int::gt(self: Self, rhs: Self) -> Boolean"));
    assert!(typed_doc.contains(
        "Return `True` when the left integer is strictly greater than the right integer."
    ));
    assert!(!typed_doc.contains(
        "\n  Return `True` when the left integer is strictly greater than the right integer."
    ));

    let ambiguous = engine.handle_line(":doc gt");
    let ambiguous = rendered_text(&ambiguous);
    assert!(ambiguous.contains("gt has multiple docs:"));
    assert!(ambiguous.contains("impl Gt for Int::gt"));
    assert!(ambiguous.contains("impl Gt for Float::gt"));

    let unsupported = engine.handle_line(":doc gt(make_value(), 1)");
    assert!(rendered_text(&unsupported)
        .contains("Unsupported typed call query argument `make_value()`"));
}

#[test]
fn core_callable_refs_and_signature_errors_are_ui_independent() {
    let mut engine = engine();

    let builtin_ref = engine.handle_line("&Int::shr");
    assert!(rendered_text(&builtin_ref).contains(
        "FnCapture(module: Int, name: shr, signature: shr(value: Int, bits: Int) -> Result<Int, NegativeShiftCount>)"
    ));

    let trait_helper = engine.handle_line("&concat");
    assert!(!trait_helper.should_exit);
    assert!(matches!(trait_helper.output, ReplOutput::EvalError { .. }));
    assert!(rendered_text(&trait_helper)
        .contains("Trait helper `concat` cannot be referenced directly"));

    let builtin_call = engine.handle_line("print()");
    assert!(!builtin_call.should_exit);
    assert!(rendered_text(&builtin_call).contains("print expects 1 argument(s), got 0"));

    let add_call = engine.handle_line("Add::add(False, True)");
    assert!(!add_call.should_exit);
    let add_text = rendered_text(&add_call);
    assert!(add_text.contains("Add::add requires a receiver type implementing Add, got Boolean"));
}

#[test]
fn core_duplicate_defs_and_runtime_result_errors_keep_the_session_alive() {
    let mut engine = engine();

    let first_def = engine.handle_line("def f() -> Int { 1 }");
    assert!(!first_def.should_exit);

    let duplicate = engine.handle_line("def f() -> Int { 2 }");
    assert!(!duplicate.should_exit);
    assert!(matches!(duplicate.output, ReplOutput::EvalError { .. }));
    assert!(rendered_text(&duplicate).contains("Duplicate top-level definition: f"));

    let err_value = engine.handle_line("Err(NoneError)");
    assert!(!err_value.should_exit);
    assert!(matches!(err_value.output, ReplOutput::EvalError { .. }));
    assert!(rendered_text(&err_value).contains("None Value."));

    let still_alive = engine.handle_line("1");
    assert!(!still_alive.should_exit);
    assert!(rendered_text(&still_alive).contains("1"));
}

#[test]
fn core_save_writes_decodable_eldr_snapshot() {
    let mut engine = engine();
    let dir = tempfile_dir("xldr-repl-core-save");
    let path = dir.join("session.eldr");

    let bind = engine.handle_line("answer = 42");
    assert!(rendered_text(&bind).contains("answer: Int = 42"));

    let save = engine.handle_line(&format!(":save {}", path.display()));
    assert!(rendered_text(&save).contains("saved to"));

    let bytes = fs::read(&path).expect("saved .eldr should exist");
    let bytecode = sindr::ir::Bytecode::decode(&bytes).expect("saved .eldr should decode");
    assert!(!bytecode.opcodes.is_empty());
}

#[test]
fn core_quit_command_sets_exit_without_ui_work() {
    let mut engine = engine();

    let result = engine.handle_line(":quit");
    assert!(result.should_exit);
    assert_eq!(status_text(&result), "quit");
}

fn tempfile_dir(prefix: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("temp dir should be created");
    dir
}
