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
        | ReplOutput::PlainText { lines: rendered }
        | ReplOutput::StyledDoc { lines: rendered } => rendered,
        ReplOutput::Diagnostic {
            rendered,
            summary_tail: _,
        } => rendered,
        ReplOutput::DocResolved {
            symbol,
            signature,
            summary,
            source_snippet,
            details,
        } => {
            panic!(
                "expected rendered output, got doc resolved: symbol={symbol}, signature={signature:?}, summary={summary:?}, source_snippet={source_snippet:?}, details={details:?}"
            )
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
            details,
        } => [
            symbol.clone(),
            signature.clone().unwrap_or_default(),
            summary.clone().unwrap_or_default(),
            source_snippet.clone().unwrap_or_default(),
            details.join("\n"),
        ]
        .join("\n"),
        ReplOutput::PlainText { lines: rendered } | ReplOutput::StyledDoc { lines: rendered } => {
            rendered.join("\n")
        }
        ReplOutput::Diagnostic {
            rendered,
            summary_tail,
        } => rendered
            .iter()
            .chain(summary_tail.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"),
        other => panic!("expected doc output, got {}", output_kind(other)),
    }
}

fn signature_text(result: &ReplResult) -> String {
    match &result.output {
        ReplOutput::StyledDoc { lines } | ReplOutput::PlainText { lines } => lines.join("\n"),
        ReplOutput::Diagnostic {
            rendered,
            summary_tail,
        } => rendered
            .iter()
            .chain(summary_tail.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"),
        ReplOutput::EvalError { rendered, .. } => {
            panic!(
                "expected signature output, got EvalError:\n{}",
                rendered.join("\n")
            )
        }
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
        ReplOutput::PlainText { .. } => "PlainText",
        ReplOutput::StyledDoc { .. } => "StyledDoc",
        ReplOutput::Diagnostic { .. } => "Diagnostic",
        ReplOutput::DocResolved { .. } => "DocResolved",
        ReplOutput::StatusMessage(_) => "StatusMessage",
    }
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
    assert!(rendered_text(&err).contains("This top-level declaration is not allowed in REPL"));
}

#[test]
fn core_from_script_source_exposes_preloaded_docs_and_keeps_repl_policy() {
    let mut engine = ReplEngine::from_script_source(
        "preload.srt",
        r#"
@doc """
Greets from preload.
"""
def greet() -> String { "hello" }
"#,
    )
    .expect("script preload should bootstrap");

    let doc = engine.handle_line(":doc greet");
    let doc = doc_text(&doc);
    assert!(doc.contains("Greets from preload."), "{doc}");

    let call = engine.handle_line("greet()");
    assert!(rendered_text(&call).contains("hello"));

    let err = engine.handle_line("defstruct User { name: String }");
    assert!(
        rendered_text(&err).contains("REPL"),
        "{}",
        rendered_text(&err)
    );
}

#[test]
fn core_from_script_file_resolves_include_and_executes_preload_before_repl() {
    let dir = tempfile_dir("xldr-repl-core-script-include");
    let module_path = dir.join("m.srt");
    let script_path = dir.join("a.srt");
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

    let mut engine =
        ReplEngine::from_script_file(script_path.to_str().expect("script path must be utf-8"))
            .expect("script preload with include should bootstrap");

    let value = engine.handle_line("answer + 1");
    let value_text = rendered_text(&value);
    assert!(
        value_text.contains("2"),
        "kind={}\nvalue_text={value_text:?}",
        output_kind(&value.output)
    );
}

#[test]
fn core_from_module_source_exposes_preloaded_module_definitions() {
    let mut engine = ReplEngine::from_module_source(
        "math.srt",
        r#"
defmod Math {
  @doc """
  Add two ints.
  """
  def add2(x: Int, y: Int) -> Int { x + y }
}
"#,
    )
    .expect("module preload should bootstrap");

    let doc = engine.handle_line(":doc Math::add2");
    let doc = doc_text(&doc);
    assert!(doc.contains("Add two ints."), "{doc}");

    let imported = engine.handle_line("import Math::add2");
    assert!(rendered_text(&imported).contains("Imported Math::add2"));

    let call = engine.handle_line("add2(1, 2)");
    assert!(rendered_text(&call).contains("3"));
}

#[test]
fn core_from_module_source_rejects_include_directive() {
    let result = ReplEngine::from_module_source(
        "math.srt",
        r#"
defmod Math {
  def add2(x: Int, y: Int) -> Int { x + y }
}

include "./extra.srt"
"#,
    );
    let err = match result {
        Ok(_) => panic!("module preload with include must fail"),
        Err(err) => err,
    };

    let rendered = format!("{err:?}");
    assert!(rendered.contains("include"), "{rendered}");
}

#[test]
fn core_commands_do_not_require_a_cli_process() {
    let mut engine = engine();

    let help = engine.handle_line(":help");
    assert!(rendered_text(&help).contains("REPL commands:"));
    assert!(rendered_text(&help).contains(":type <binding>"));

    let doc = engine.handle_line(":doc print");
    let doc = doc_text(&doc);
    assert!(doc.contains("Kernel::print"));
    assert!(doc.contains("Print a string to stdout."));

    let sig = engine.handle_line(":sig print");
    assert!(signature_text(&sig).contains("Kernel::print(a: String) -> Unit"));

    let missing_sig = engine.handle_line(":sig a");
    let missing_sig = rendered_text(&missing_sig);
    assert!(missing_sig.contains("No signature found for a"));
    assert!(missing_sig.contains(":doc <symbol>") || missing_sig.contains(":sig $a"));

    let unknown = engine.handle_line(":nope");
    assert!(!unknown.should_exit);
    assert!(rendered_text(&unknown).contains("Unknown REPL command: :nope"));
}

#[test]
fn core_reuses_deferred_tuple_lens_bindings_between_inputs() {
    let mut engine = engine();

    let lens = engine.handle_line("a = Tuple._1");
    assert!(rendered_text(&lens).contains("a: Lens<_, _> = Tuple._1"));

    let pair = engine.handle_line("pair = (\"alice\", 2)");
    assert!(rendered_text(&pair).contains("pair: (String, Int) = (\"alice\", 2)"));

    let value = engine.handle_line("Lens::view(a, pair)");
    assert!(rendered_text(&value).contains("2"));
}

#[test]
fn core_static_impl_methods_keep_runtime_arity_in_sync() {
    let mut engine = engine();

    let gen_range = engine.handle_line("Generator::to_list(Generator::range(1, 3))");
    assert!(!gen_range.should_exit);
    assert!(rendered_text(&gen_range).contains("[1, 2, 3]"));

    let codepoints = engine.handle_line("String::codepoints(\"a\", StringEncoding::Ascii)");
    assert!(!codepoints.should_exit);
    assert!(rendered_text(&codepoints).contains("Ok([97])"));
}

#[test]
fn core_renders_top_level_lens_compose_expressions_without_codegen_leak() {
    let mut engine = engine();

    let tuple_lens = engine.handle_line("a = Tuple._1");
    assert!(rendered_text(&tuple_lens).contains("a: Lens<_, _> = Tuple._1"));

    let enum_lens = engine.handle_line("ep = IntBase.Oct");
    assert!(rendered_text(&enum_lens).contains("ep: Lens<IntBase, Unit> = IntBase.Oct"));

    let slash = engine.handle_line("a / ep");
    let slash = rendered_text(&slash);
    assert!(slash.contains("Lens<_, _> = Tuple._1.Oct"), "{slash}");

    let helper = engine.handle_line("Lens::compose(a, ep)");
    let helper = rendered_text(&helper);
    assert!(helper.contains("Lens<_, _> = Tuple._1.Oct"), "{helper}");
}

#[test]
fn core_lens_command_reports_segments_and_stop_points() {
    let mut engine = engine();

    let binding = engine.handle_line("path = Tuple._0");
    assert!(rendered_text(&binding).contains("path: Lens<_, _> = Tuple._0"));

    let lens_info = engine.handle_line(":lens path");
    assert!(matches!(lens_info.output, ReplOutput::StyledDoc { .. }));
    let lens_info = rendered_text(&lens_info);
    assert!(lens_info.contains("## LensPath"), "{lens_info}");
    assert!(lens_info.contains("type: Lens<_, _>"), "{lens_info}");
    assert!(lens_info.contains("view result: _"), "{lens_info}");
    assert!(lens_info.contains("full path: Tuple._0"), "{lens_info}");
    assert!(lens_info.contains("## Flow"), "{lens_info}");
    assert!(lens_info.contains("hop 1: Tuple._0"), "{lens_info}");
    assert!(lens_info.contains("relation: _ -> _"), "{lens_info}");

    let fallible = engine.handle_line(":lens BitWidth.Any");
    let fallible = rendered_text(&fallible);
    assert!(
        fallible.contains("view result: Result<Int, Error>"),
        "{fallible}"
    );
    assert!(fallible.contains("## Stops"), "{fallible}");
    assert!(fallible.contains("stop 1:"), "{fallible}");
    assert!(
        fallible.contains("variant mismatch returns Result"),
        "{fallible}"
    );
}

#[test]
fn core_doc_reports_match_and_cond_from_bootstrap_surface() {
    let mut engine = engine();

    let match_doc = engine.handle_line(":doc match");
    let match_doc = doc_text(&match_doc);
    assert!(match_doc.contains("Bootstrap::match"), "{match_doc}");
    assert!(
        match_doc.contains("match value { pattern => expr, ... } -> $B"),
        "{match_doc}"
    );
    assert!(match_doc.contains("Match special form."), "{match_doc}");
    assert!(
        match_doc.contains("pattern when cond => expr"),
        "{match_doc}"
    );
    assert!(
        match_doc.contains("`match` must be exhaustive"),
        "{match_doc}"
    );
    assert!(
        match_doc.contains("Use `Ok(...)` and `Err(...)`"),
        "{match_doc}"
    );

    let cond_doc = engine.handle_line(":doc cond");
    let cond_doc = doc_text(&cond_doc);
    assert!(cond_doc.contains("Bootstrap::cond"), "{cond_doc}");
    assert!(
        cond_doc.contains("cond { cond1 => expr1, ..., True => exprN } -> $A"),
        "{cond_doc}"
    );
    assert!(cond_doc.contains("Cond special form."), "{cond_doc}");
    assert!(
        cond_doc.contains("cond { cond1 => expr1, ..., True => exprN }"),
        "{cond_doc}"
    );
    assert!(
        cond_doc.contains("final clause must be `True`"),
        "{cond_doc}"
    );
}

#[test]
fn core_type_command_looks_up_visible_bindings_only() {
    let mut engine = engine();

    let list = engine.handle_line("list: List<Int> = [1, 2, 3]");
    let list_text = rendered_text(&list);
    assert!(
        list_text.contains("list: List<Int> = [1, 2, 3]"),
        "{list_text}"
    );

    let list_type = engine.handle_line(":type list");
    let list_type_text = rendered_text(&list_type);
    assert_eq!(
        list_type_text,
        "list\ntype: List<Int>\nidentity: TypeIdentity::Type"
    );

    let closure = engine.handle_line("captured = 1");
    assert!(rendered_text(&closure).contains("captured: Int = 1"));

    let closure_fun = engine.handle_line("pure = {|n: Int| n + 1}");
    let closure_fun_text = rendered_text(&closure_fun);
    assert!(
        closure_fun_text.contains("pure: (Int -> Int) = Closure(Int -> Int)"),
        "{closure_fun_text}"
    );

    let closure_fun_type = engine.handle_line(":type pure");
    let closure_fun_type_text = rendered_text(&closure_fun_type);
    assert_eq!(
        closure_fun_type_text,
        "pure\ntype: (Int -> Int)\nidentity: TypeIdentity::Closure"
    );

    let capture_fun = engine.handle_line("inc = {|n: Int| n + captured}");
    let capture_fun_text = rendered_text(&capture_fun);
    assert!(
        capture_fun_text.contains("inc: (Int -> Int) = Closure(Int -> Int)"),
        "{capture_fun_text}"
    );

    let binary_closure = engine.handle_line("f = {|x: Int, y: Int| x + y}");
    let binary_closure_text = rendered_text(&binary_closure);
    assert!(
        binary_closure_text.contains("f: (Int, Int -> Int) = Closure(Int, Int -> Int)"),
        "{binary_closure_text}"
    );

    let capture_fun_type = engine.handle_line(":type inc");
    let capture_fun_type_text = rendered_text(&capture_fun_type);
    assert_eq!(
        capture_fun_type_text,
        "inc\ntype: (Int -> Int)\nidentity: TypeIdentity::Closure"
    );

    let builtin_capture = engine.handle_line("p = &print");
    let builtin_capture_text = rendered_text(&builtin_capture);
    assert!(
        builtin_capture_text
            .contains("FnCapture(module: Kernel, name: print, sig: print(a: String) -> Unit)"),
        "{builtin_capture_text}"
    );

    let builtin_capture_type = engine.handle_line(":type p");
    let builtin_capture_type_text = rendered_text(&builtin_capture_type);
    assert_eq!(
        builtin_capture_type_text,
        "p\ntype: (String -> Unit)\nidentity: TypeIdentity::Capture"
    );

    let partial_capture = engine.handle_line("f = &Add::add(&1, 4)");
    let partial_capture_text = rendered_text(&partial_capture);
    assert!(
        partial_capture_text.contains("FnCapture(module: Add, name: add, sig: (Int -> Int))"),
        "{partial_capture_text}"
    );

    let partial_capture_type = engine.handle_line(":type f");
    let partial_capture_type_text = rendered_text(&partial_capture_type);
    assert_eq!(
        partial_capture_type_text,
        "f\ntype: (Int -> Int)\nidentity: TypeIdentity::Capture"
    );

    for invalid in [":type if", ":type String::is_empty()"] {
        let result = engine.handle_line(invalid);
        let text = rendered_text(&result);
        assert!(text.contains("Usage: :type <binding>"), "{text}");
    }
}

#[test]
fn core_repl_surfaces_keep_generic_arguments_for_bindings() {
    let mut engine = engine();

    let bind = engine.handle_line("maybe: Option<Int> = Option::Some(1)");
    let bind_text = rendered_text(&bind);
    assert!(
        bind_text.contains("maybe: Option<Int> = Option::Some(1)"),
        "{bind_text}"
    );

    let nested = engine.handle_line("nested: List<Option<Int>> = [Option::Some(1)]");
    let nested_text = rendered_text(&nested);
    assert!(
        nested_text.contains("nested: List<Option<Int>> = [Option::Some(1)]"),
        "{nested_text}"
    );

    let ty = engine.handle_line(":type maybe");
    assert_eq!(
        rendered_text(&ty),
        "maybe\ntype: Option<Int>\nidentity: TypeIdentity::Enum"
    );

    let info = engine.handle_line(":info maybe");
    let info_text = rendered_text(&info);
    assert!(info_text.contains("type: Option<Int>"), "{info_text}");

    let nested_info = engine.handle_line(":info nested");
    let nested_info_text = rendered_text(&nested_info);
    assert!(
        nested_info_text.contains("type: List<Option<Int>>"),
        "{nested_info_text}"
    );
}

#[test]
fn core_help_and_error_commands_return_structured_command_output() {
    let mut engine = engine();

    let help = engine.handle_line(":help");
    let help_text = rendered_text(&help);
    assert!(help_text.contains("REPL commands:"));
    assert!(help_text.contains(":save <path.eldr>"));
    assert!(help_text.contains(":info <query>"));

    let sig_help = engine.handle_line(":h sig");
    assert!(rendered_text(&sig_help).contains("Usage: :sig <function|query>"));

    let info_help = engine.handle_line(":help info");
    assert!(rendered_text(&info_help).contains("Usage: :info <query>"));

    let error_default = engine.handle_line(":error");
    assert!(rendered_text(&error_default).contains("error display mode: full"));

    let error_summary = engine.handle_line(":error summary");
    assert!(rendered_text(&error_summary).contains("error display mode: summary"));

    let error_full = engine.handle_line(":error full");
    assert!(rendered_text(&error_full).contains("error display mode: full"));
}

#[test]
fn core_info_command_reports_queries_and_command_errors() {
    let mut engine = engine();

    let info_usage = engine.handle_line(":info");
    assert!(rendered_text(&info_usage).contains("Usage: :info <query>"));

    let print_info = engine.handle_line(":info print");
    let print_info_text = rendered_text(&print_info);
    assert!(
        print_info_text.contains("Kernel::print"),
        "{print_info_text}"
    );
    assert!(print_info_text.contains("kind:"), "{print_info_text}");
    assert!(print_info_text.contains("origin:"), "{print_info_text}");
    assert!(print_info_text.contains("defined:"), "{print_info_text}");

    let _ = engine.handle_line("ret = Ok(\"3\")");
    let _ = engine.handle_line("up = {|term: String| try_from(term, Int)}");
    let typed_info = engine.handle_line(":info ret |>= up");
    let typed_info_text = rendered_text(&typed_info);
    assert!(typed_info_text.contains("defined:"), "{typed_info_text}");
    assert!(
        typed_info_text.contains("specialized:"),
        "{typed_info_text}"
    );
    assert!(typed_info_text.contains("Result<Int>"), "{typed_info_text}");
}

#[test]
fn core_repl_command_and_query_errors_use_diagnostics() {
    let mut engine = engine();

    let bad_error_mode = engine.handle_line(":error bad");
    let bad_error_mode_text = strip_ansi(&rendered_text(&bad_error_mode));
    assert!(
        bad_error_mode_text.contains("Error: ReplCommandError"),
        "{bad_error_mode_text}"
    );

    let bad_sig_query = engine.handle_line(":sig gt(Int, )");
    let bad_sig_query_text = strip_ansi(&rendered_text(&bad_sig_query));
    assert!(
        bad_sig_query_text.contains("Error: ReplQueryParseError"),
        "{bad_sig_query_text}"
    );

    let info_type_error = engine.handle_line(":info 1 + \"2\"");
    let info_type_error_text = strip_ansi(&rendered_text(&info_type_error));
    assert!(
        info_type_error_text.contains("Error: ReplQueryParseError"),
        "{info_type_error_text}"
    );
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
fn core_immediate_anonymous_callable_calls_show_binding_hint() {
    let mut engine = engine();

    for source in [
        "&add(&1, 10)(4)",
        "(&add(&1, 10))(4)",
        "({|x| x + 1})(4)",
        "(make())(4)",
    ] {
        let result = engine.handle_line(source);
        assert!(!result.should_exit);
        assert!(matches!(result.output, ReplOutput::EvalError { .. }));
        let text = rendered_text(&result);
        assert!(
            text.contains("Immediate calls on anonymous callable expressions are not supported")
        );
        assert!(text.contains("f = &add(&1, 10)"));
        assert!(text.contains("tmp = make()"));
    }
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

    let and_doc = engine.handle_line(":doc &&");
    let and_doc = doc_text(&and_doc);
    assert!(and_doc.contains("Kernel::and"));
    assert!(and_doc.contains("Logical conjunction with short-circuit evaluation."));

    let or_doc = engine.handle_line(":doc ||");
    let or_doc = doc_text(&or_doc);
    assert!(or_doc.contains("Kernel::or"));
    assert!(or_doc.contains("Logical disjunction with short-circuit evaluation."));

    let bind_doc = engine.handle_line(":doc =");
    let bind_doc = doc_text(&bind_doc);
    assert!(bind_doc.contains("Bootstrap::="), "{bind_doc}");
    assert!(
        bind_doc.contains("@intrinsic def =(pattern: $Pattern, value: $A) -> Unit"),
        "{bind_doc}"
    );
    assert!(bind_doc.contains("Bind special form."), "{bind_doc}");

    let safe_bind_doc = engine.handle_line(":doc =?");
    let safe_bind_doc = doc_text(&safe_bind_doc);
    assert!(safe_bind_doc.contains("Bootstrap::=?"), "{safe_bind_doc}");
    assert!(
        safe_bind_doc.contains("@intrinsic def =?(pattern: $Pattern, value: $A) -> Unit"),
        "{safe_bind_doc}"
    );
    assert!(
        safe_bind_doc.contains("SafeBind special form."),
        "{safe_bind_doc}"
    );
    assert!(
        safe_bind_doc
            .contains("It may be used only inside functions whose current evaluation returns"),
        "{safe_bind_doc}"
    );
    assert!(
        safe_bind_doc.contains("`num: Int =? Option::Some(1)` is an error"),
        "{safe_bind_doc}"
    );
    assert!(
        safe_bind_doc.contains(
            "The REPL accepts the syntax, reports the resulting error message, and keeps"
        ),
        "{safe_bind_doc}"
    );

    let typed_sig = engine.handle_line(":sig gt(Int, Int)");
    let typed_sig = signature_text(&typed_sig);
    assert!(typed_sig.contains("impl Gt for Int::gt(self: Self, rhs: Self) -> Boolean"));

    let operator_sig = engine.handle_line(":sig |>");
    let operator_sig = signature_text(&operator_sig);
    assert!(operator_sig.contains("trait PipeApply { pipe_apply(self: Self, value: $A) -> $B }"));

    let slash_doc = engine.handle_line(":doc /");
    let slash_doc = doc_text(&slash_doc);
    assert!(slash_doc.contains("trait Compose"), "{slash_doc}");
    assert!(slash_doc.contains("models the `/` operator"), "{slash_doc}");

    let helper_sig = engine.handle_line(":sig gt");
    let helper_sig = signature_text(&helper_sig);
    assert!(helper_sig.contains("trait Gt { gt(self: Self, rhs: Self) -> Boolean }"));

    let bind_sig = engine.handle_line(":sig =");
    let bind_sig = signature_text(&bind_sig);
    assert_eq!(
        bind_sig.trim(),
        "@intrinsic def =(pattern: $Pattern, value: $A) -> Unit"
    );

    let safe_bind_sig = engine.handle_line(":sig =?");
    let safe_bind_sig = signature_text(&safe_bind_sig);
    assert_eq!(
        safe_bind_sig.trim(),
        "@intrinsic def =?(pattern: $Pattern, value: $A) -> Unit"
    );

    let match_sig = engine.handle_line(":sig match");
    let match_sig = signature_text(&match_sig);
    assert_eq!(
        match_sig.trim(),
        "match value { pattern => expr, ... } -> $B"
    );

    let cond_sig = engine.handle_line(":sig cond");
    let cond_sig = signature_text(&cond_sig);
    assert_eq!(
        cond_sig.trim(),
        "cond { cond1 => expr1, ..., True => exprN } -> $A"
    );

    let typed_doc = engine.handle_line(":doc gt(Int, Int)");
    let typed_doc = doc_text(&typed_doc);
    assert!(typed_doc.contains("impl Gt for Int::gt(self: Self, rhs: Self) -> Boolean"));
    assert!(typed_doc.contains(
        "Return `True` when the left integer is strictly greater than the right integer."
    ));
    assert!(!typed_doc.contains(
        "\n  Return `True` when the left integer is strictly greater than the right integer."
    ));

    let helper_doc = engine.handle_line(":doc gt");
    let helper_doc = doc_text(&helper_doc);
    assert!(helper_doc.contains("trait Gt { gt(self: Self, rhs: Self) -> Boolean }"));

    let constructor_doc = engine.handle_line(":doc Duration(Int)");
    let constructor_doc = doc_text(&constructor_doc);
    assert!(
        constructor_doc.contains("Duration::new(value: Int) -> Result<Self, Error>"),
        "{constructor_doc}"
    );
    assert!(
        constructor_doc.contains("Construct a `Duration` from a millisecond count."),
        "{constructor_doc}"
    );

    let extractor_doc = engine.handle_line(":doc Duration!()");
    let extractor_doc = doc_text(&extractor_doc);
    assert!(
        extractor_doc.contains("Duration::deconstruct(self: Self) -> MatchResult<Int, Error>"),
        "{extractor_doc}"
    );
    assert!(
        extractor_doc
            .contains("Deconstruct a `Duration` into its millisecond count in pattern position."),
        "{extractor_doc}"
    );

    let extractor_sig = engine.handle_line(":sig Duration!()");
    let extractor_sig = signature_text(&extractor_sig);
    assert!(
        extractor_sig
            .contains("defined:\n  Duration::deconstruct(self: Self) -> MatchResult<Int, Error>"),
        "{extractor_sig}"
    );
    assert!(
        extractor_sig.contains("specialized:\n  Duration!() -> MatchResult<Int, Error>"),
        "{extractor_sig}"
    );

    let extractor_sig_no_args = engine.handle_line(":sig Duration!");
    let extractor_sig_no_args = signature_text(&extractor_sig_no_args);
    assert_eq!(extractor_sig_no_args.trim(), extractor_sig.trim());

    let extractor_sig_explicit_self = engine.handle_line(":sig Duration!(Duration)");
    let extractor_sig_explicit_self = signature_text(&extractor_sig_explicit_self);
    assert!(
        extractor_sig_explicit_self
            .contains("defined:\n  Duration::deconstruct(self: Self) -> MatchResult<Int, Error>"),
        "{extractor_sig_explicit_self}"
    );
    assert!(
        extractor_sig_explicit_self
            .contains("specialized:\n  Duration!(Duration) -> MatchResult<Int, Error>"),
        "{extractor_sig_explicit_self}"
    );

    let duration_sig = engine.handle_line(":sig Duration");
    let duration_sig = signature_text(&duration_sig);
    assert_eq!(
        duration_sig.trim(),
        "Duration::new(value: Int) -> Result<Self, Error>"
    );

    let duration_empty_call_sig = engine.handle_line(":sig Duration()");
    let duration_empty_call_sig = signature_text(&duration_empty_call_sig);
    assert_eq!(
        duration_empty_call_sig.trim(),
        "Duration::new(value: Int) -> Result<Self, Error>"
    );

    let unsupported = engine.handle_line(":doc gt(make_value(), Int)");
    assert!(
        rendered_text(&unsupported).contains("Unsupported command query argument `make_value()`"),
        "{}",
        rendered_text(&unsupported)
    );
}

#[test]
fn core_sig_type_owner_falls_back_to_constructor_signatures() {
    let mut engine = engine();

    let option_sig = engine.handle_line(":sig Option");
    let option_sig = rendered_text(&option_sig);
    assert!(option_sig.contains("* Option::Some($T)"), "{option_sig}");
    assert!(option_sig.contains("* Option::None"), "{option_sig}");

    let point_sig = engine.handle_line(":sig StyledDocStyle");
    let point_sig = signature_text(&point_sig);
    assert!(point_sig.contains("StyledDocStyle::new("), "{point_sig}");
    assert!(point_sig.contains("fg: Option"), "{point_sig}");
    assert!(point_sig.contains("italic: Boolean"), "{point_sig}");
    assert!(point_sig.contains("-> Self"), "{point_sig}");
}

#[test]
fn core_sig_enum_rejects_extra_input_with_shared_message() {
    let mut engine = engine();

    let guided = rendered_text(&engine.handle_line(":sig Option(Int)"));
    assert!(guided.contains(":sig Option"), "{guided}");

    let bare_variant = rendered_text(&engine.handle_line(":sig Option::Some"));
    assert!(bare_variant.contains(":sig Option"), "{bare_variant}");

    let variant_call = rendered_text(&engine.handle_line(":sig Option::Some()"));
    assert!(
        variant_call.contains("expects 1 argument(s), got 0")
            || variant_call.contains(":sig Option"),
        "{variant_call}"
    );

    let variant_typed = rendered_text(&engine.handle_line(":sig Option::Some(Int)"));
    assert!(
        variant_typed.contains("Option::Some(Int) -> Option<Int>")
            || variant_typed.contains(":sig Option"),
        "{variant_typed}"
    );
}

#[test]
fn core_doc_command_resolves_closure_type_and_callable_bindings() {
    let mut engine = engine();

    let closure_doc = engine.handle_line(":doc Closure");
    let closure_doc = doc_text(&closure_doc);
    assert!(closure_doc.contains("Closure"), "{closure_doc}");
    assert!(
        closure_doc
            .contains("Compiler-reserved callable category marker for REPL and doc surfaces."),
        "{closure_doc}"
    );

    let closure_binding = engine.handle_line("adder = {|n: Int| n + 1}");
    assert!(rendered_text(&closure_binding).contains("adder: (Int -> Int)"));
    let closure_binding_doc = engine.handle_line(":doc $adder");
    let closure_binding_doc = doc_text(&closure_binding_doc);
    assert!(
        closure_binding_doc.contains("Compiler-reserved callable category marker"),
        "{closure_binding_doc}"
    );
    assert!(
        closure_binding_doc.contains("type: (Int -> Int)"),
        "{closure_binding_doc}"
    );
    assert!(
        closure_binding_doc.contains("example: ret: Int = adder(Int)"),
        "{closure_binding_doc}"
    );
    assert!(
        !closure_binding_doc.contains("captures:"),
        "{closure_binding_doc}"
    );

    let capture_binding = engine.handle_line("printer = &print");
    assert!(rendered_text(&capture_binding).contains("FnCapture(module: Kernel, name: print"));
    let capture_binding_doc = engine.handle_line(":doc $printer");
    let capture_binding_doc = doc_text(&capture_binding_doc);
    assert!(
        capture_binding_doc.contains("Kernel::print"),
        "{capture_binding_doc}"
    );
    assert!(
        capture_binding_doc.contains("binding: printer"),
        "{capture_binding_doc}"
    );
    assert!(
        capture_binding_doc.contains("derived from: Kernel::print"),
        "{capture_binding_doc}"
    );
}

#[test]
fn core_sig_expression_queries_support_operator_forms() {
    let mut engine = engine();

    assert!(rendered_text(&engine.handle_line("ret = Ok(\"3\")"))
        .contains("ret: Result<String, Error> = Ok(\"3\")"));
    assert!(
        rendered_text(&engine.handle_line("up = {|term: String| try_from(term, Int)}"))
            .contains("up: (String -> Result<Int, Error>)")
    );

    let bind_sig = engine.handle_line(":sig ret |>= up");
    let bind_sig = signature_text(&bind_sig);
    assert!(
        bind_sig.contains("defined:\n  Chainable::chain("),
        "{bind_sig}"
    );
    assert!(bind_sig.contains("lhs: Result<String>"), "{bind_sig}");
    assert!(
        bind_sig.contains("rhs: (String -> Result<Int>)"),
        "{bind_sig}"
    );
    assert!(
        bind_sig.contains("specialized:\n  ret |>= up: Result<Int>"),
        "{bind_sig}"
    );

    assert!(rendered_text(&engine.handle_line("value = 3")).contains("value: Int = 3"));
    assert!(
        rendered_text(&engine.handle_line("inc = {|n: Int| n + 1}")).contains("inc: (Int -> Int)")
    );

    let pipe_sig = engine.handle_line(":sig value |> inc");
    let pipe_sig = signature_text(&pipe_sig);
    assert!(
        pipe_sig.contains("defined:\n  PipeApply::pipe_apply("),
        "{pipe_sig}"
    );
    assert!(
        pipe_sig.contains("specialized:\n  value |> inc: Int"),
        "{pipe_sig}"
    );

    assert!(
        rendered_text(&engine.handle_line("inc_ok = {|n: Int| Ok(n + 1)}"))
            .contains("inc_ok: (Int -> Result<Int, Error>)")
    );
    let compose_sig = engine.handle_line(":sig up >=> inc_ok");
    let compose_sig = signature_text(&compose_sig);
    assert!(
        compose_sig.contains("defined:\n  KleisliComposable::kleisli_compose("),
        "{compose_sig}"
    );
    assert!(
        compose_sig.contains("specialized:\n  up >=> inc_ok: (String -> Result<Int>)"),
        "{compose_sig}"
    );
}

#[test]
fn core_sig_expression_queries_reject_non_expressions() {
    let mut engine = engine();

    for source in [":sig a = 1", ":sig import Kernel", ":sig 1\n2"] {
        let result = engine.handle_line(source);
        assert!(!result.should_exit);
        let text = rendered_text(&result);
        assert!(
            text.contains("Unsupported command query form")
                || text.contains("Unsupported command query argument"),
            "{text}"
        );
    }
}

#[test]
fn core_sig_typed_operator_queries_accept_function_types_and_reject_explicit_result_error() {
    let mut engine = engine();

    assert!(rendered_text(&engine.handle_line("num = 3")).contains("num: Int = 3"));

    let sig = engine.handle_line(":sig num |> (Int -> String)");
    let sig = signature_text(&sig);
    assert!(sig.contains("defined:\n  PipeApply::pipe_apply("), "{sig}");
    assert!(sig.contains("rhs: (Int -> String)"), "{sig}");
    assert!(
        sig.contains("specialized:\n  num |> (Int -> String): String"),
        "{sig}"
    );

    let invalid = engine.handle_line(":sig num |> (Int -> Result<String, Error>)");
    let invalid = rendered_text(&invalid);
    assert!(
        invalid.contains("Typed query `Result` should be written as `Result<T>`"),
        "{invalid}"
    );
}

#[test]
fn core_sig_typed_call_queries_specialize_polymorphic_returns() {
    let mut engine = engine();

    let sig = engine.handle_line(":sig id(Int)");
    let sig = signature_text(&sig);
    assert!(
        sig.contains("defined:\n  Kernel::id(value: $A) -> $A"),
        "{sig}"
    );
    assert!(sig.contains("specialized:\n  id(Int) -> Int"), "{sig}");
}

#[test]
fn core_sig_supports_closure_bindings_recapture_and_application() {
    let mut engine = engine();

    let closure = engine.handle_line("a = {|n: Int, m: Int| n + m}");
    let closure_text = rendered_text(&closure);
    assert!(
        closure_text.contains("a: (Int, Int -> Int)"),
        "{closure_text}"
    );

    let closure_sig = engine.handle_line(":sig $a");
    assert_eq!(
        signature_text(&closure_sig),
        "a: (Int, Int -> Int) :: Closure"
    );

    let arity_error = engine.handle_line(":sig a(Int)");
    let arity_text = rendered_text(&arity_error);
    assert!(
        arity_text.contains("function expects 2 argument(s), got 1"),
        "{arity_text}"
    );
    assert!(
        arity_text.contains("Callable type signature: (Int, Int -> Int)"),
        "{arity_text}"
    );

    let applied = engine.handle_line(":sig a(Int, Int)");
    assert_eq!(signature_text(&applied), "a(Int, Int) -> Int");

    let recaptured = engine.handle_line("b = &a(1, &1)");
    let recaptured_text = rendered_text(&recaptured);
    assert!(
        recaptured_text.contains("b: (Int -> Int)"),
        "{recaptured_text}"
    );

    let recaptured_sig = engine.handle_line(":sig $b");
    assert_eq!(
        signature_text(&recaptured_sig),
        "b: (Int -> Int) :: Capture"
    );

    let recapture_query = engine.handle_line(":sig &a(Int, &1)");
    let recapture_query = rendered_text(&recapture_query);
    assert!(
        recapture_query.contains("Invalid typed call query callee `&a`"),
        "{recapture_query}"
    );
}

#[test]
fn core_callable_refs_and_signature_errors_are_ui_independent() {
    let mut engine = engine();

    let builtin_ref = engine.handle_line("&Int::shr");
    assert!(rendered_text(&builtin_ref).contains(
        "FnCapture(module: Int, name: shr, sig: Int::shr(value: Int, bits: Int) -> Result<Int, NegativeShiftCount>)"
    ));

    let partial_capture_ref = engine.handle_line("&Add::add(&1, 1)");
    assert!(rendered_text(&partial_capture_ref)
        .contains("FnCapture(module: Add, name: add, sig: (Int -> Int))"));

    let closure_ref = engine.handle_line("{|x: Int, y: Int| x + y}");
    assert!(rendered_text(&closure_ref).contains("Closure(Int, Int -> Int)"));

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
fn core_partial_capture_chains_preserve_capture_origin_until_a_closure_literal_appears() {
    let mut engine = engine();

    let def = engine.handle_line("def f(a: Int, b: Int, c: Int) -> Int { a + b + c }");
    assert!(!def.should_exit);

    let f3 = engine.handle_line("f3 = &f");
    let f3_text = rendered_text(&f3);
    assert!(f3_text.contains("FnCapture("), "{f3_text}");
    assert!(f3_text.contains("name: f"), "{f3_text}");
    assert!(
        f3_text.contains("sig: f(a: Int, b: Int, c: Int) -> Int"),
        "{f3_text}"
    );

    let f2 = engine.handle_line("f2 = &f3(&1, &2, 3)");
    let f2_text = rendered_text(&f2);
    assert!(f2_text.contains("FnCapture("), "{f2_text}");
    assert!(f2_text.contains("sig: (Int, Int -> Int)"), "{f2_text}");

    let f1 = engine.handle_line("f1 = &f2(&1, 2)");
    let f1_text = rendered_text(&f1);
    assert!(f1_text.contains("FnCapture("), "{f1_text}");
    assert!(f1_text.contains("sig: (Int -> Int)"), "{f1_text}");

    let applied = engine.handle_line("f1(10)");
    assert!(rendered_text(&applied).contains("15"));

    let g3 = engine.handle_line("g3 = {|a: Int, b: Int, c: Int| a + b + c}");
    let g3_text = rendered_text(&g3);
    assert!(
        g3_text.contains("Closure(Int, Int, Int -> Int)"),
        "{g3_text}"
    );

    let g2 = engine.handle_line("g2 = &g3(&1, &2, 3)");
    let g2_text = rendered_text(&g2);
    assert!(
        g2_text.contains("Closure(Int, Int -> Int)")
            || g2_text.contains("FnCapture(") && g2_text.contains("sig: (Int, Int -> Int)"),
        "{g2_text}"
    );
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

#[test]
fn core_dbg_docs_and_signatures_resolve_from_bootstrap_source() {
    let mut engine = engine();

    let doc = engine.handle_line(":doc dbg!");
    let doc = doc_text(&doc);
    assert!(doc.contains("Bootstrap::dbg!"), "{doc}");
    assert!(doc.contains("Debug special form."), "{doc}");

    let sig = engine.handle_line(":sig dbg!");
    let rendered = signature_text(&sig);
    assert!(
        rendered.contains("@intrinsic def dbg!(values: *$A) -> Unit"),
        "{rendered}"
    );
}

#[test]
fn core_dbg_typed_call_queries_use_special_form_pseudo_application() {
    let mut engine = engine();

    let doc = engine.handle_line(":doc dbg!(Int)");
    let doc = doc_text(&doc);
    assert!(doc.contains("Bootstrap::dbg!"), "{doc}");
    assert!(doc.contains("inspect"), "{doc}");

    let sig = engine.handle_line(":sig dbg!(Int, String)");
    let sig = signature_text(&sig);
    assert_eq!(sig.trim(), "@intrinsic def dbg!(values: *$A) -> Unit");
}

#[test]
fn core_doc_reports_tuple_surface_undocumented_types_and_scope_aware_helpers() {
    let mut engine = engine();

    let tuple_doc = engine.handle_line(":doc Tuple");
    let tuple_doc = doc_text(&tuple_doc);
    assert!(tuple_doc.contains("Tuple"), "{tuple_doc}");
    assert!(tuple_doc.contains("Tuple._0"), "{tuple_doc}");
    assert!(tuple_doc.contains("Tuple._1"), "{tuple_doc}");
    assert!(tuple_doc.contains("pair._1"), "{tuple_doc}");
    assert!(
        tuple_doc.contains("Lens::view(Tuple._1, pair)"),
        "{tuple_doc}"
    );
    assert!(
        tuple_doc.contains("Lens::set(Tuple._1, pair, 3)"),
        "{tuple_doc}"
    );

    let tuple_sig = engine.handle_line(":sig Tuple");
    let tuple_sig = rendered_text(&tuple_sig);
    assert!(
        tuple_sig.contains("No signature found for Tuple"),
        "{tuple_sig}"
    );

    let config_doc = engine.handle_line(":doc Config");
    let config_doc = doc_text(&config_doc);
    assert!(config_doc.contains("Config"), "{config_doc}");
    assert!(config_doc.contains("defstruct Config"), "{config_doc}");
    assert!(config_doc.contains("undocumented"), "{config_doc}");
    assert!(config_doc.contains("@doc"), "{config_doc}");

    let style_doc = engine.handle_line(":doc StyledDocStyle");
    let style_doc = doc_text(&style_doc);
    assert!(style_doc.contains("StyledDocStyle"), "{style_doc}");
    assert!(
        style_doc.contains("defrecord StyledDocStyle"),
        "{style_doc}"
    );
    assert!(style_doc.contains("undocumented"), "{style_doc}");

    let helper_before_import = engine.handle_line(":doc add");
    let helper_before_import = rendered_text(&helper_before_import);
    assert!(
        helper_before_import.contains("No docs found for add"),
        "{helper_before_import}"
    );

    let import_add = engine.handle_line("import Add::add");
    let import_add_text = rendered_text(&import_add);
    assert!(
        import_add_text.contains("Imported Add::add"),
        "{import_add_text}"
    );

    let helper_after_import = engine.handle_line(":doc add");
    let helper_after_import = doc_text(&helper_after_import);
    assert!(
        helper_after_import.contains("Add::add"),
        "{helper_after_import}"
    );

    let if_doc = engine.handle_line(":doc if");
    let if_doc = doc_text(&if_doc);
    assert!(if_doc.contains("Kernel::if"), "{if_doc}");
}

#[test]
fn core_doc_typed_call_supports_qualified_inherent_impl_methods() {
    let mut engine = engine();

    let doc = engine.handle_line(":doc Boolean::not(Boolean)");
    let doc = doc_text(&doc);
    assert!(doc.contains("Boolean::not"), "{doc}");
    assert!(doc.contains("logical negation"), "{doc}");
}

#[test]
fn core_sig_supports_tuple_field_sugar_and_lens_expression_queries() {
    let mut engine = engine();

    let pair = engine.handle_line("pair = (\"alice\", 2)");
    let pair_text = rendered_text(&pair);
    assert!(pair_text.contains("pair: (String, Int)"), "{pair_text}");

    let field_sig = engine.handle_line(":sig pair._1");
    let field_sig = signature_text(&field_sig);
    assert!(field_sig.contains("defined:"), "{field_sig}");
    assert!(
        field_sig.contains("Lens::view(Tuple._1, pair)"),
        "{field_sig}"
    );
    assert!(field_sig.contains("specialized:"), "{field_sig}");
    assert!(field_sig.contains("pair._1: Int"), "{field_sig}");

    let view_sig = engine.handle_line(":sig pair._1");
    let view_sig = signature_text(&view_sig);
    assert!(view_sig.contains("defined:"), "{view_sig}");
    assert!(view_sig.contains("Lens::view("), "{view_sig}");
    assert!(view_sig.contains("specialized:"), "{view_sig}");
    assert!(view_sig.contains("pair._1: Int"), "{view_sig}");

    let result_pair = engine.handle_line("result_pair = (Ok(2), \"ok\")");
    let result_pair_text = rendered_text(&result_pair);
    assert!(
        result_pair_text.contains("result_pair"),
        "{result_pair_text}"
    );

    let compose_sig =
        engine.handle_line(":sig Lens::compose(StyledDocSegment.style, StyledDocStyle.bold)");
    let compose_sig = rendered_text(&compose_sig);
    assert!(
        compose_sig.contains("Unsupported command query argument `StyledDocSegment.style`"),
        "{compose_sig}"
    );

    let over_result_sig = engine.handle_line(
        ":sig Lens::over_result(Tuple._0, result_pair, {|value: Result<Int>| Ok(value)})",
    );
    let over_result_sig = rendered_text(&over_result_sig);
    assert!(
        over_result_sig.contains("Unsupported command query argument `Tuple._0`"),
        "{over_result_sig}"
    );

    let slash_sig = engine.handle_line(":sig StyledDocSegment.style / StyledDocStyle.bold");
    let slash_sig = rendered_text(&slash_sig);
    assert!(
        slash_sig.contains("Unsupported command query form")
            || slash_sig.contains("Unsupported command query argument"),
        "{slash_sig}"
    );
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
