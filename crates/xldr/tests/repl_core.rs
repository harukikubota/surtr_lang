use std::fs;
use std::time::Duration;

use xldr::repl::logic::core::{ReplCompletionContext, ReplCompletionKind};
use xldr::repl::logic::{ReplOutput, ReplResult};
use xldr::ReplEngine;

fn engine() -> ReplEngine {
    ReplEngine::new().expect("REPL engine should bootstrap")
}

fn process_engine() -> ReplEngine {
    ReplEngine::from_script_source(
        "process_preload.srt",
        r#"
defagent MyWorker {
  meta {
    instance: Worker
    init_policy: Eager
    state: Int
  }

  @init
  def init(seed: Int) -> Result<Int> { Ok(seed) }

  @get
  def read(state: Int) -> Result<Int> { Ok(state) }

  @set
  def write(_state: Int, next: Int) -> Result<Int> { Ok(next) }

  def hidden_value(_state: Int) -> Result<Int> { Ok(99) }
}

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

  def hidden_size(_state: Int) -> Result<Int> { Ok(0) }
}

defsupervisor MySup {
  meta {
    strategy: OneForOne
    max_restarts: 5
    max_seconds: 10
    child_restart_default: Transient
    allow_adopt: True
  }
}

supervisor_init {
  MyServer {}
  MySup {}
}
"#,
    )
    .expect("process preload should bootstrap")
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

fn visible_text(result: &ReplResult) -> String {
    result
        .stdout
        .iter()
        .chain(rendered(result).iter())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
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

#[test]
fn core_completion_returns_global_candidates_with_details() {
    let mut engine = engine();
    assert!(
        engine.completions("", 0).candidates.is_empty(),
        "empty prompt should not show global completion noise"
    );
    assert!(rendered_text(&engine.handle_line("answer = 42")).contains("answer: Int"));

    let completion = engine.completions("ans", 3);
    let answer = completion
        .candidates
        .iter()
        .find(|candidate| candidate.label == "answer")
        .expect("answer binding should be suggested");
    assert_eq!(answer.kind, ReplCompletionKind::Variable);
    assert_eq!(answer.detail.as_deref(), Some("Int"));

    let print = engine
        .completions("pri", 3)
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "print")
        .expect("pathless function calls should be suggested");
    assert_eq!(print.kind, ReplCompletionKind::FunctionCall);
    assert!(print
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("print(")));
    assert!(print
        .documentation
        .as_deref()
        .is_some_and(|doc| doc.contains("Print a string to stdout")));
    assert!(
        completion.telemetry.completion_compute_ns.is_some(),
        "completion telemetry should record compute time: {:?}",
        completion.telemetry
    );
    assert!(
        completion
            .telemetry
            .completion_compute_ns
            .is_some_and(|value| value > 0),
        "completion telemetry should be positive: {:?}",
        completion.telemetry
    );
}

#[test]
fn core_completion_keeps_all_matching_candidates() {
    let mut engine = engine();
    for idx in 0..6 {
        let result = engine.handle_line(&format!("value_{idx} = {idx}"));
        assert!(
            rendered_text(&result).contains(&format!("value_{idx}: Int")),
            "{}",
            rendered_text(&result)
        );
    }

    let completion = engine.completions("value_", "value_".len());
    assert!(
        completion.candidates.len() >= 6,
        "core completion should retain all matching candidates for paging: {:?}",
        completion.candidates
    );
}

#[test]
fn core_operator_completion_shows_inferred_rhs_signature_without_operator_candidates() {
    let mut engine = engine();
    assert!(rendered_text(&engine.handle_line("answer = 42")).contains("answer: Int"));
    assert!(rendered_text(&engine.handle_line("name = \"surtr\"")).contains("name: String"));

    let completion = engine.completions("1 + ", "1 + ".len());
    let signature = completion
        .signature
        .as_ref()
        .expect("operator rhs should show signature help");
    assert_eq!(signature.lines.join("\n"), "Int + [Int]");

    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert!(
        !labels
            .iter()
            .any(|label| matches!(*label, "+" | "|>" | "++")),
        "operator symbols must not be completion candidates: {labels:?}"
    );
    let answer_pos = labels
        .iter()
        .position(|label| *label == "answer")
        .expect("Int binding should be suggested");
    let name_pos = labels
        .iter()
        .position(|label| *label == "name")
        .expect("String binding should remain available after matching Int candidates");
    assert!(
        answer_pos < name_pos,
        "Int candidates should rank before nonmatching variables: {labels:?}"
    );

    assert!(ReplCompletionContext::should_request("1 + ", "1 + ".len()));

    let partial = engine.completions("1 + an", "1 + an".len());
    assert_eq!(
        partial
            .signature
            .as_ref()
            .expect("operator rhs prefix should keep signature help")
            .lines
            .join("\n"),
        "Int + [Int]"
    );
    let partial_labels = partial
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert!(
        partial_labels.iter().any(|label| *label == "answer"),
        "matching Int variable should remain visible with a RHS prefix: {partial_labels:?}"
    );
}

#[test]
fn core_operator_completion_shows_string_concat_rhs_signature() {
    let mut engine = engine();
    assert!(rendered_text(&engine.handle_line("name = \"surtr\"")).contains("name: String"));

    let completion = engine.completions("name ++ ", "name ++ ".len());
    let signature = completion
        .signature
        .as_ref()
        .expect("concat rhs should show signature help");
    assert_eq!(signature.lines.join("\n"), "String ++ [String]");
}

#[test]
fn core_operator_completion_stages_function_operator_types() {
    let mut engine = engine();
    assert!(rendered_text(&engine.handle_line("x = 1")).contains("x: Int"));

    let first = engine.completions("x |> ", "x |> ".len());
    let first_signature = first
        .signature
        .as_ref()
        .expect("pipe rhs should show callable shape");
    assert_eq!(first_signature.lines.join("\n"), "Int |> [(Int -> _)]");
    let first_labels = first
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert!(
        !first_labels
            .iter()
            .any(|label| matches!(*label, "|>" | "|*>" | "|>=" | ">>" | ">*" | ">=>")),
        "function operator symbols must not be completion candidates: {first_labels:?}"
    );
    let to_string_pos = first_labels
        .iter()
        .position(|label| *label == "to_string")
        .expect("callable RHS candidate should be suggested for pipe");
    let print_pos = first_labels
        .iter()
        .position(|label| *label == "print")
        .expect("nonmatching callable should remain available after matching candidates");
    assert!(
        to_string_pos < print_pos,
        "callable candidates accepting Int should rank first: {:?}",
        first
            .candidates
            .iter()
            .filter(|candidate| matches!(
                candidate.label.as_str(),
                "to_string" | "print" | "Show::to_string" | "Kernel::print"
            ))
            .collect::<Vec<_>>()
    );

    let chained = engine.completions("x |> to_string |> ", "x |> to_string |> ".len());
    let chained_signature = chained
        .signature
        .as_ref()
        .expect("chained pipe rhs should carry staged type");
    assert_eq!(
        chained_signature.lines.join("\n"),
        "Int |> (Int -> String) |> [(String -> _)]"
    );
}

#[test]
fn core_operator_completion_stages_specialized_function_operators() {
    let mut engine = engine();
    assert!(rendered_text(&engine.handle_line("xs = [1, 2]")).contains("xs: List<Int>"));
    assert!(rendered_text(&engine.handle_line("ret = Ok(1)")).contains("ret: Result<Int"));
    assert!(
        rendered_text(&engine.handle_line("to_s = {|n: Int| to_string(n)}"))
            .contains("to_s: (Int -> String)")
    );
    assert!(
        rendered_text(&engine.handle_line("strings = {|n: Int| [to_string(n)]}"))
            .contains("strings: (Int -> List<String>)")
    );
    assert!(
        rendered_text(&engine.handle_line("inc_ok = {|n: Int| Ok(n + 1)}"))
            .contains("inc_ok: (Int -> Result<Int")
    );

    let mapped = engine.completions("xs |*> ", "xs |*> ".len());
    assert_eq!(
        mapped.signature.as_ref().unwrap().lines.join("\n"),
        "List<Int> |*> [(Int -> _)]"
    );

    let bound = engine.completions("ret |>= ", "ret |>= ".len());
    assert_eq!(
        bound.signature.as_ref().unwrap().lines.join("\n"),
        "Result<Int> |>= [(Int -> Result<_>)]"
    );

    let composed = engine.completions("to_s >> ", "to_s >> ".len());
    assert_eq!(
        composed.signature.as_ref().unwrap().lines.join("\n"),
        "(Int -> String) >> [(String -> _)]"
    );

    let lifted = engine.completions("strings >* ", "strings >* ".len());
    assert_eq!(
        lifted.signature.as_ref().unwrap().lines.join("\n"),
        "(Int -> List<String>) >* [(String -> _)]"
    );

    let kleisli = engine.completions("inc_ok >=> ", "inc_ok >=> ".len());
    assert_eq!(
        kleisli.signature.as_ref().unwrap().lines.join("\n"),
        "(Int -> Result<Int>) >=> [(Int -> Result<_>)]"
    );
}

#[test]
fn core_operator_completion_keeps_unknown_lhs_as_placeholder() {
    let engine = engine();
    let completion = engine.completions("missing |> ", "missing |> ".len());
    let signature = completion
        .signature
        .as_ref()
        .expect("unknown lhs should still show staged operator help");

    assert_eq!(signature.lines.join("\n"), "_ |> [(_ -> _)]");
}

#[test]
fn core_exposes_shared_semantic_index_for_repl_and_lsp_lookup() {
    let mut engine = engine();
    assert!(rendered_text(&engine.handle_line("answer = 42")).contains("answer: Int"));

    let index = engine.semantic_index();
    let answer = surtr_analysis::lookup_symbol_at_cursor(&index, "answer", 3)
        .expect("REPL binding should be visible through shared semantic lookup");
    assert_eq!(answer.symbol.label, "answer");
    assert_eq!(answer.symbol.kind, surtr_analysis::CompletionKind::Variable);
    assert_eq!(answer.symbol.detail.as_deref(), Some("Int"));

    let print = index
        .find_symbol("print")
        .expect("stdlib function should be visible through shared semantic index");
    assert_eq!(print.kind, surtr_analysis::CompletionKind::FunctionCall);
    assert!(print
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("print(")));
    assert!(print.documentation.is_some());

    let duration = index
        .find_symbol("Duration")
        .expect("stdlib type should be visible through shared semantic index");
    assert_eq!(
        duration.kind,
        surtr_analysis::CompletionKind::TypeConstructor
    );
    assert!(duration
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("Duration")));
    assert!(
        duration.documentation.is_some(),
        "type constructors should retain shared doc metadata: {duration:?}"
    );
}

#[test]
fn core_exposes_symbol_semantic_infos_before_completion_projection() {
    let mut engine = engine();
    assert!(rendered_text(&engine.handle_line("answer = 42")).contains("answer: Int"));

    let infos = engine.symbol_semantic_infos();

    let answer = infos
        .iter()
        .find(|info| info.surface_name == "answer")
        .expect("REPL binding should be visible as semantic info");
    assert_eq!(answer.kind, surtr_analysis::CompletionKind::Variable);
    assert_eq!(answer.detail.as_deref(), Some("Int"));

    let print = infos
        .iter()
        .find(|info| info.surface_name == "print")
        .expect("stdlib function should be visible as semantic info");
    assert_eq!(print.kind, surtr_analysis::CompletionKind::FunctionCall);
    assert!(print.documentation.is_some());
    assert!(
        print.display_metadata.is_some(),
        "REPL semantic info should retain stdlib display metadata origin: {print:?}"
    );

    let duration = infos
        .iter()
        .find(|info| info.surface_name == "Duration")
        .expect("stdlib type should be visible as semantic info");
    assert_eq!(duration.identity, Some(sindr::names::TypeIdentity::Type));
}

#[test]
fn core_shared_repl_completion_helper_preserves_repl_visibility_and_presentation() {
    let engine = engine();
    let index = engine.semantic_index();

    let string_repeat = surtr_analysis::complete_repl_prefix(
        surtr_analysis::CompletionRequest {
            index: &index,
            source: "String::re",
            cursor: "String::re".len(),
        },
        surtr_analysis::CompletionScope::All,
    );
    let repeat = string_repeat
        .candidates
        .iter()
        .find(|candidate| candidate.label == "String::repeat")
        .expect("shared REPL completion should expose qualified String helpers");
    assert_eq!(repeat.kind, surtr_analysis::CompletionKind::TypePath);
    assert!(
        repeat
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("String::repeat(")),
        "shared completion should retain signature detail: {repeat:?}"
    );

    let process_init = surtr_analysis::complete_repl_prefix(
        surtr_analysis::CompletionRequest {
            index: &index,
            source: "ProcessInit",
            cursor: "ProcessInit".len(),
        },
        surtr_analysis::CompletionScope::All,
    );
    assert!(
        process_init.candidates.is_empty(),
        "shared REPL completion must preserve xldr hidden-owner filtering: {:?}",
        process_init.candidates
    );
}

#[test]
fn core_completion_returns_type_constructors_and_type_paths() {
    let engine = engine();

    let duration = engine
        .completions("Dur", 3)
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "Duration")
        .expect("type constructor should be suggested");
    assert_eq!(duration.kind, ReplCompletionKind::TypeConstructor);
    assert!(duration
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("Duration")));

    let int_min = engine
        .completions("Int::mi", "Int::mi".len())
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "Int::min")
        .expect("qualified type path should be suggested");
    assert_eq!(int_min.kind, ReplCompletionKind::TypePath);
    assert!(int_min
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("Int::min(")));
}

#[test]
fn core_completion_shows_bare_result_constructors_and_bool_variants() {
    let engine = engine();

    let ok_candidates = engine.completions("Ok", "Ok".len()).candidates;
    let ok_labels = ok_candidates
        .iter()
        .map(|candidate| candidate.label.clone())
        .collect::<Vec<_>>();
    let ok = ok_candidates
        .into_iter()
        .find(|candidate| candidate.label == "Ok")
        .unwrap_or_else(|| panic!("bare Ok constructor should be suggested: {ok_labels:?}"));
    assert_eq!(ok.kind, ReplCompletionKind::FunctionCall);
    assert_eq!(ok.replacement, "Ok");
    assert_eq!(
        ok.detail.as_deref(),
        Some("Result::Ok($T) -> Result<$T, Error>"),
        "Ok completion detail should expose the canonical Result surface: {ok:?}"
    );

    let err_candidates = engine.completions("Err", "Err".len()).candidates;
    let err_labels = err_candidates
        .iter()
        .map(|candidate| candidate.label.clone())
        .collect::<Vec<_>>();
    let err = err_candidates
        .into_iter()
        .find(|candidate| candidate.label == "Err")
        .unwrap_or_else(|| panic!("bare Err constructor should be suggested: {err_labels:?}"));
    assert_eq!(err.kind, ReplCompletionKind::FunctionCall);
    assert_eq!(err.replacement, "Err");
    assert_eq!(
        err.detail.as_deref(),
        Some("Result::Err(Error) -> Result<$T, Error>"),
        "Err completion detail should expose the canonical Result surface: {err:?}"
    );

    let true_candidates = engine.completions("Tr", "Tr".len()).candidates;
    let true_labels = true_candidates
        .iter()
        .map(|candidate| candidate.label.clone())
        .collect::<Vec<_>>();
    let true_variant = true_candidates
        .into_iter()
        .find(|candidate| candidate.label == "True")
        .unwrap_or_else(|| panic!("bare True variant should be suggested: {true_labels:?}"));
    assert_eq!(true_variant.kind, ReplCompletionKind::FunctionCall);
    assert_eq!(true_variant.replacement, "True");
    assert_eq!(
        true_variant.detail.as_deref(),
        Some("Boolean::True() -> Boolean")
    );

    let false_candidates = engine.completions("Fal", "Fal".len()).candidates;
    let false_labels = false_candidates
        .iter()
        .map(|candidate| candidate.label.clone())
        .collect::<Vec<_>>();
    let false_variant = false_candidates
        .into_iter()
        .find(|candidate| candidate.label == "False")
        .unwrap_or_else(|| panic!("bare False variant should be suggested: {false_labels:?}"));
    assert_eq!(false_variant.kind, ReplCompletionKind::FunctionCall);
    assert_eq!(false_variant.replacement, "False");
    assert_eq!(
        false_variant.detail.as_deref(),
        Some("Boolean::False() -> Boolean")
    );
}

#[test]
fn core_completion_accepts_lowercase_bool_aliases() {
    let engine = engine();

    let true_candidates = engine.completions("tru", "tru".len()).candidates;
    let true_labels = true_candidates
        .iter()
        .map(|candidate| format!("{}=>{}", candidate.label, candidate.replacement))
        .collect::<Vec<_>>();
    let true_variant = true_candidates
        .into_iter()
        .find(|candidate| candidate.label == "true")
        .unwrap_or_else(|| panic!("lowercase true alias should be suggested: {true_labels:?}"));
    assert_eq!(true_variant.kind, ReplCompletionKind::FunctionCall);
    assert_eq!(true_variant.replacement, "True");

    let false_candidates = engine.completions("fal", "fal".len()).candidates;
    let false_labels = false_candidates
        .iter()
        .map(|candidate| format!("{}=>{}", candidate.label, candidate.replacement))
        .collect::<Vec<_>>();
    let false_variant = false_candidates
        .into_iter()
        .find(|candidate| candidate.label == "false")
        .unwrap_or_else(|| panic!("lowercase false alias should be suggested: {false_labels:?}"));
    assert_eq!(false_variant.kind, ReplCompletionKind::FunctionCall);
    assert_eq!(false_variant.replacement, "False");
}

#[test]
fn core_completion_hides_lowercase_bool_alias_when_shadowed_by_value_binding() {
    let mut engine = engine();
    let bound = rendered_text(&engine.handle_line("true = 1"));
    assert!(bound.contains("true: Int = 1"), "{bound}");

    let candidates = engine.completions("tru", "tru".len()).candidates;
    let rendered = candidates
        .iter()
        .map(|candidate| format!("{}=>{}", candidate.label, candidate.replacement))
        .collect::<Vec<_>>();
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.label == "true" && candidate.replacement == "True"),
        "special lowercase bool alias should disappear once shadowed: {rendered:?}"
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.label == "true" && candidate.replacement == "true"),
        "shadowing binding should remain visible as the real completion: {rendered:?}"
    );
}

#[test]
fn core_completion_hides_lowercase_bool_alias_when_shadowed_by_import_or_top_level_def() {
    let mut imported_engine = ReplEngine::from_module_source(
        "hoge.srt",
        r#"
defmod Hoge {
  def true() -> Int { 1 }
}
"#,
    )
    .expect("module preload should succeed");

    let imported = rendered_text(&imported_engine.handle_line("import Hoge::true"));
    assert!(imported.contains("Imported Hoge::true"), "{imported}");

    let imported_candidates = imported_engine.completions("tru", "tru".len()).candidates;
    let imported_rendered = imported_candidates
        .iter()
        .map(|candidate| format!("{}=>{}", candidate.label, candidate.replacement))
        .collect::<Vec<_>>();
    assert!(
        !imported_candidates
            .iter()
            .any(|candidate| candidate.label == "true" && candidate.replacement == "True"),
        "imported lowercase symbol should suppress the special bool alias: {imported_rendered:?}"
    );
    assert!(
        imported_candidates
            .iter()
            .any(|candidate| candidate.label == "true" && candidate.replacement == "true"),
        "imported lowercase symbol should stay visible as the actual completion: {imported_rendered:?}"
    );

    let mut live_engine = engine();
    let defined = live_engine.handle_line("def true() -> Int { 1 }");
    assert!(
        !matches!(defined.output, ReplOutput::EvalError { .. }),
        "live top-level def should compile: {}",
        rendered_text(&defined)
    );

    let live_candidates = live_engine.completions("tru", "tru".len()).candidates;
    let live_rendered = live_candidates
        .iter()
        .map(|candidate| format!("{}=>{}", candidate.label, candidate.replacement))
        .collect::<Vec<_>>();
    assert!(
        !live_candidates
            .iter()
            .any(|candidate| candidate.label == "true" && candidate.replacement == "True"),
        "live top-level def should suppress the special bool alias: {live_rendered:?}"
    );
    assert!(
        live_candidates
            .iter()
            .any(|candidate| candidate.label == "true" && candidate.replacement == "true"),
        "live top-level def should remain visible as the actual completion: {live_rendered:?}"
    );
}

#[test]
fn core_completion_only_shows_unqualified_importable_functions_after_import() {
    let mut engine = engine();

    let labels_before = engine
        .completions("a", 1)
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert!(
        !labels_before.iter().any(|label| label == "abs"),
        "Float::abs should not be suggested as a bare call before import: {labels_before:?}"
    );
    assert!(
        engine
            .completions("Float::a", "Float::a".len())
            .candidates
            .iter()
            .any(|candidate| candidate.label == "Float::abs"),
        "qualified Float::abs completion should remain available"
    );

    let imported = rendered_text(&engine.handle_line("import Float::abs"));
    assert!(
        imported.contains("Imported Float::abs"),
        "import should succeed before testing completion: {imported}"
    );

    let labels_after = engine
        .completions("a", 1)
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert!(
        labels_after.iter().any(|label| label == "abs"),
        "Float::abs should be suggested as a bare call after import: {labels_after:?}"
    );
}

#[test]
fn core_completion_and_sig_prefer_authored_signatures_for_imported_helpers() {
    let mut engine = engine();

    let with_completion = engine
        .completions("wi", 2)
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "with")
        .expect("Result::with should be suggested");
    assert_eq!(
        with_completion.detail.as_deref(),
        Some("Result::with(value: Result<$A>, f: Result<($A -> $B)>) -> Result<$B>")
    );

    let list_at_completion = engine
        .completions("List::a", "List::a".len())
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "List::at")
        .expect("List::at should be suggested");
    assert_eq!(
        list_at_completion.detail.as_deref(),
        Some("List::at(values: List<$A>, index: Int) -> Result<$A, IndexOutOfBounds>")
    );

    let imported = rendered_text(&engine.handle_line("import List::{at}"));
    assert!(imported.contains("Imported List::at"), "{imported}");

    let at_completion = engine
        .completions("at", 2)
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "at")
        .expect("imported at helper should be suggested");
    assert_eq!(
        at_completion.detail.as_deref(),
        Some("List::at(values: List<$A>, index: Int) -> Result<$A, IndexOutOfBounds>")
    );

    assert_eq!(
        signature_text(&engine.handle_line(":sig at")).trim(),
        "List::at(values: List<$A>, index: Int) -> Result<$A, IndexOutOfBounds>"
    );
    assert_eq!(
        signature_text(&engine.handle_line(":sig with")).trim(),
        "Result::with(value: Result<$A>, f: Result<($A -> $B)>) -> Result<$B>"
    );
    assert_eq!(
        signature_text(&engine.handle_line(":sig List::at")).trim(),
        "List::at(values: List<$A>, index: Int) -> Result<$A, IndexOutOfBounds>"
    );
    assert_eq!(
        signature_text(&engine.handle_line(":sig Result::with")).trim(),
        "Result::with(value: Result<$A>, f: Result<($A -> $B)>) -> Result<$B>"
    );
}

#[test]
fn core_completion_is_enabled_inside_string_interpolation_only() {
    let engine = engine();
    let string_body = r#""plain Str"#;
    assert!(
        engine
            .completions(string_body, string_body.len())
            .candidates
            .is_empty(),
        "completion should stay disabled in ordinary string text"
    );

    let interpolation = r#""plain #{Str"#;
    let labels = engine
        .completions(interpolation, interpolation.len())
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert!(
        labels.contains(&"String".to_string()),
        "completion should be enabled inside string interpolation: {labels:?}"
    );
}

#[test]
fn core_completion_shows_builtin_owner_surfaces_and_hides_special_types() {
    let engine = engine();
    let completion_context = engine.completion_context();

    for (prefix, expected) in [
        ("Str", "String"),
        ("Lis", "List"),
        ("Fac", "Facet"),
        ("IO", "IO"),
        ("Jso", "Json"),
    ] {
        let candidate = completion_context
            .completions(prefix, prefix.len())
            .candidates
            .into_iter()
            .find(|candidate| candidate.label == expected)
            .unwrap_or_else(|| panic!("{expected} should be suggested for prefix {prefix}"));
        assert_eq!(candidate.kind, ReplCompletionKind::TypeConstructor);
    }

    let string_repeat = completion_context
        .completions("String::re", "String::re".len())
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "String::repeat")
        .expect("qualified String helper should be suggested");
    assert_eq!(string_repeat.kind, ReplCompletionKind::TypePath);

    let facet_view = completion_context
        .completions("Facet::v", "Facet::v".len())
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "Facet::view")
        .expect("qualified Facet helper should be suggested");
    assert_eq!(facet_view.kind, ReplCompletionKind::TypePath);

    let all_labels = completion_context
        .completions("M", 1)
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert!(
        !all_labels.iter().any(|label| label == "MatchResult"),
        "MatchResult should not be suggested: {all_labels:?}"
    );

    let excluded = [
        "MatchArms",
        "CondClauses",
        "BulkUpdateEntries",
        "Hole",
        "Lazy",
        "TypeRef",
        "ProcessInit",
        "Closure",
    ];
    for name in excluded {
        let labels = completion_context
            .completions(name, name.len())
            .candidates
            .into_iter()
            .map(|candidate| candidate.label)
            .collect::<Vec<_>>();
        assert!(
            !labels.iter().any(|label| label == name),
            "{name} should not be suggested: {labels:?}"
        );
    }
}

#[test]
fn core_completion_keeps_type_owners_ahead_of_members_for_pascal_case_prefix() {
    let engine = engine();
    let labels = engine
        .completions("Int", "Int".len())
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .take(6)
        .collect::<Vec<_>>();

    assert_eq!(
        labels,
        vec![
            "Int".to_string(),
            "IntBase".to_string(),
            "Int::abs".to_string(),
            "Int::bit_and".to_string(),
            "Int::bit_not".to_string(),
            "Int::bit_not_in".to_string(),
        ]
    );
}

#[test]
fn core_completion_shows_user_defined_module_owners() {
    let engine = ReplEngine::from_module_source(
        "demo_module.srt",
        r#"
defmod Demo {
  @doc """
  Say hi.
  """
  def hello() -> String { "hi" }
}
"#,
    )
    .expect("module preload should bootstrap");

    let demo = engine
        .completions("Dem", 3)
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "Demo")
        .expect("user-defined module owner should be suggested");
    assert_eq!(demo.kind, ReplCompletionKind::TypeConstructor);

    let hello = engine
        .completions("Demo::h", "Demo::h".len())
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "Demo::hello")
        .expect("user-defined module member should be suggested");
    assert_eq!(hello.kind, ReplCompletionKind::TypePath);

    let mut engine = engine;
    let sig = signature_text(&engine.handle_line(":sig Demo::hello"));
    assert!(sig.contains("Demo::hello() -> String"), "{sig}");
    assert!(!sig.contains("Global::"), "{sig}");
}

#[test]
fn core_completion_shows_script_preload_owner_and_members_without_docs() {
    let engine = ReplEngine::from_script_source(
        "tmp/user.srt",
        r#"
defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name, age }
  }

  def birthday(self) -> Self {
    put(~self.age, self.age + 1)
  }
}
"#,
    )
    .expect("script preload should bootstrap");

    let user = engine
        .completions("U", "U".len())
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "User")
        .expect("script preload owner should be suggested without docs");
    assert_eq!(user.kind, ReplCompletionKind::TypeConstructor);

    let ctor = engine
        .completions("User::n", "User::n".len())
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "User::new")
        .expect("undocumented impl ctor should be suggested");
    assert_eq!(ctor.kind, ReplCompletionKind::TypePath);
    assert!(
        ctor.detail
            .as_deref()
            .is_some_and(|detail| detail.contains("User::new(name: String, age: Int) -> User")),
        "constructor completion detail should use surface names: {ctor:?}"
    );
    assert!(
        ctor.detail
            .as_deref()
            .is_none_or(|detail| !detail.contains("Global::")),
        "constructor completion detail must not expose Global: {ctor:?}"
    );

    let method = engine
        .completions("User::b", "User::b".len())
        .candidates
        .into_iter()
        .find(|candidate| candidate.label == "User::birthday")
        .expect("undocumented impl method should be suggested");
    assert_eq!(method.kind, ReplCompletionKind::TypePath);
    assert!(
        method
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("User::birthday(self: User) -> User")),
        "method completion detail should use surface names: {method:?}"
    );
    assert!(
        method
            .detail
            .as_deref()
            .is_none_or(|detail| !detail.contains("Global::")),
        "method completion detail must not expose Global: {method:?}"
    );
}

#[test]
fn core_script_top_level_defs_are_signature_and_completion_surfaces_without_docs() {
    let mut engine = ReplEngine::from_script_source(
        "tmp/top_level.srt",
        r#"
def greet(name: String) -> String { name }
"#,
    )
    .expect("script preload should bootstrap");

    let sig = signature_text(&engine.handle_line(":sig greet"));
    assert!(
        sig.contains("greet(name: String) -> String"),
        "script top-level def should have a signature surface: {sig}"
    );
    assert!(!sig.contains("Global::"), "{sig}");

    let completion = engine.completions("gre", "gre".len());
    let greet = completion
        .candidates
        .iter()
        .find(|candidate| candidate.label == "greet")
        .expect("script top-level def should be suggested by bare name");
    assert_eq!(greet.kind, ReplCompletionKind::FunctionCall);
    assert!(
        greet
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("greet(name: String) -> String")),
        "completion detail should mirror :sig: {greet:?}"
    );

    let docs = rendered_text(&engine.handle_line(":doc greet"));
    assert!(
        docs.contains("No docs found for greet"),
        "top-level script defs do not get doc support in this change: {docs}"
    );
}

#[test]
fn core_typed_sig_query_uses_impl_signatures_without_docs() {
    let mut engine = ReplEngine::from_script_source(
        "tmp/no_doc_impl.srt",
        r#"
deftrait Pairwise {
  def pair(self: Self, rhs: Self) -> Self
}

defstruct Duo {
  value: Int,
}

impl Duo {
  def new(value: Int) -> Self {
    Duo { value }
  }
}

impl Pairwise for Duo {
  def pair(self: Self, rhs: Self) -> Self {
    Duo { value: self.value + rhs.value }
  }
}
"#,
    )
    .expect("script preload should bootstrap");

    let doc = rendered_text(&engine.handle_line(":doc pair(Duo, Duo)"));
    assert!(
        doc.contains("No docs found for pair(Duo, Duo)"),
        "typed doc query should still require @doc: {doc}"
    );

    let sig = signature_text(&engine.handle_line(":sig pair(Duo, Duo)"));
    assert!(
        sig.contains("defined:\n  impl Pairwise for Duo::pair(self: Duo, rhs: Duo) -> Duo"),
        "{sig}"
    );
    assert!(
        sig.contains("specialized:\n  pair(Duo, Duo) -> Duo"),
        "{sig}"
    );
}

#[test]
fn core_live_repl_top_level_defs_are_signature_and_completion_surfaces() {
    let mut engine = engine();
    let def = engine.handle_line("def local(x: Int) -> Int { x + 1 }");
    assert!(
        !matches!(def.output, ReplOutput::EvalError { .. }),
        "live top-level def should compile: {}",
        rendered_text(&def)
    );

    let sig = signature_text(&engine.handle_line(":sig local"));
    assert!(
        sig.contains("local(x: Int) -> Int"),
        "live REPL top-level def should have a signature surface: {sig}"
    );

    let completion = engine.completions("loc", "loc".len());
    let local = completion
        .candidates
        .iter()
        .find(|candidate| candidate.label == "local")
        .expect("live REPL top-level def should be suggested by bare name");
    assert_eq!(local.kind, ReplCompletionKind::FunctionCall);
    assert!(
        local
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("local(x: Int) -> Int")),
        "completion detail should mirror :sig: {local:?}"
    );
}

#[test]
fn core_completion_hides_global_noise_for_empty_constructor_call_arguments() {
    let engine = engine();
    let completion = engine.completions("Duration(", "Duration(".len());

    let signature = completion
        .signature
        .as_ref()
        .expect("constructor call should still show signature help");
    assert_eq!(signature.active_parameter, Some(0));
    assert!(
        signature.lines.join("\n").contains("Duration::new("),
        "constructor signature should remain visible: {:?}",
        signature.lines
    );
    assert!(
        signature.lines.join("\n").contains("[Int]"),
        "active constructor parameter should be highlighted: {:?}",
        signature.lines
    );
    assert!(
        completion.candidates.is_empty(),
        "empty constructor argument position should not show unrelated global candidates: {:?}",
        completion.candidates
    );
}

#[test]
fn core_completion_shows_script_preload_constructor_signature_without_docs() {
    let engine = ReplEngine::from_script_source(
        "tmp/user.srt",
        r#"
defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> User {
    User { name, age }
  }
}
"#,
    )
    .expect("script preload should bootstrap");

    let completion = engine.completions("User(", "User(".len());
    let signature = completion
        .signature
        .as_ref()
        .expect("script preload constructor should show signature help");
    assert_eq!(signature.active_parameter, Some(0));
    let rendered = signature.lines.join("\n");
    assert!(
        rendered.contains("User::new("),
        "constructor signature should use owner surface: {rendered:?}"
    );
    assert!(
        rendered.contains("name: [String]"),
        "first constructor parameter should be highlighted: {rendered:?}"
    );
    assert!(
        rendered.contains("-> User"),
        "constructor signature should follow the actual new return type: {rendered:?}"
    );
    assert!(
        !rendered.contains("-> Self"),
        "constructor signature must not fall back to synthesized Self when new returns User: {rendered:?}"
    );
}

#[test]
fn core_completion_constructor_signature_follows_result_self_new_signature() {
    let engine = ReplEngine::from_script_source(
        "tmp/user_result.srt",
        r#"
defstruct User {
  name: String,
}

impl User {
  def new(name: String) -> Result<Self> {
    Ok(User { name })
  }
}
"#,
    )
    .expect("script preload should bootstrap");

    let completion = engine.completions("User(", "User(".len());
    let signature = completion
        .signature
        .as_ref()
        .expect("script preload constructor should show signature help");
    let rendered = signature.lines.join("\n");
    assert!(
        rendered.contains("User::new(name: [String]) -> Result<User, Error>"),
        "constructor signature should follow Result<Self> from new after type normalization: {rendered:?}"
    );
    assert!(
        !rendered.contains("-> Self"),
        "constructor signature must not use synthesized Self fallback when new returns Result<Self>: {rendered:?}"
    );
}

#[test]
fn core_completion_hides_enum_variants_until_owner_path_is_confirmed() {
    let engine = engine();

    let bare_labels = engine
        .completions("IntB", "IntB".len())
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert_eq!(
        bare_labels,
        vec![
            "IntBase",
            "IntBase::label",
            "IntBase::prefix",
            "IntBase::radix"
        ]
    );

    let bool_labels = engine
        .completions("Bool", "Bool".len())
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert_eq!(
        bool_labels,
        vec![
            "Boolean",
            "Boolean::eqv",
            "Boolean::implies",
            "Boolean::not",
            "Boolean::xor"
        ]
    );

    let qualified_labels = engine
        .completions("IntBase::", "IntBase::".len())
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();
    assert!(qualified_labels.contains(&"IntBase::Bin".to_string()));
    assert!(qualified_labels.contains(&"IntBase::Dec".to_string()));
    assert!(qualified_labels.contains(&"IntBase::Hex".to_string()));
    assert!(qualified_labels.contains(&"IntBase::Oct".to_string()));

    let boolean_candidates = engine
        .completions("Boolean::", "Boolean::".len())
        .candidates;
    let boolean_labels = boolean_candidates
        .iter()
        .map(|candidate| candidate.label.clone())
        .collect::<Vec<_>>();
    assert!(boolean_labels.contains(&"Boolean::True".to_string()));
    assert!(boolean_labels.contains(&"Boolean::False".to_string()));

    let true_variant = boolean_candidates
        .into_iter()
        .find(|candidate| candidate.label == "Boolean::True")
        .expect("qualified Boolean variant should be suggested");
    assert_eq!(true_variant.kind, ReplCompletionKind::TypePath);
    assert_eq!(
        true_variant.detail.as_deref(),
        Some("Boolean::True() -> Boolean")
    );
}

#[test]
fn core_completion_shows_tuple_variant_signature_help() {
    let engine = engine();
    let completion = engine.completions("BitWidth::Any(", "BitWidth::Any(".len());

    let signature = completion
        .signature
        .as_ref()
        .expect("tuple variant call should show signature help");
    assert_eq!(signature.active_parameter, Some(0));
    let rendered = signature.lines.join("\n");
    assert!(
        rendered.contains("BitWidth::Any([Int]) -> BitWidth"),
        "tuple variant signature should expose constructor call shape: {rendered:?}"
    );
}

#[test]
fn core_completion_ranks_constructor_arguments_by_expected_parameter_type() {
    let mut engine = engine();
    assert!(rendered_text(&engine.handle_line("n = 3")).contains("n: Int"));
    assert!(rendered_text(&engine.handle_line(r#"s = "text""#)).contains("s: String"));

    let completion = engine.completions("Duration(", "Duration(".len());
    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(
        labels.contains(&"n"),
        "Int binding should be suggested for constructor argument: {labels:?}"
    );
    assert!(
        labels.contains(&"s"),
        "String binding should remain available but ranked lower: {labels:?}"
    );
    assert!(
        labels.iter().position(|label| label == &"n")
            < labels.iter().position(|label| label == &"s"),
        "Int binding should rank before String binding for Int constructor argument: {labels:?}"
    );
}

#[test]
fn core_completion_hides_trait_impl_members_from_qualified_type_paths() {
    let engine = engine();
    let labels = engine
        .completions("Boolean::", "Boolean::".len())
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();

    for expected in [
        "Boolean::not",
        "Boolean::xor",
        "Boolean::eqv",
        "Boolean::implies",
    ] {
        assert!(
            labels.iter().any(|label| label == expected),
            "owner method should be suggested: {expected}; labels={labels:?}"
        );
    }

    for hidden in [
        "Boolean::impl Show for Boolean::to_string",
        "Boolean::impl Eq for Boolean::eq",
        "Boolean::impl Neq for Boolean::neq",
        "Boolean::impl From<String> for Boolean::from",
        "Boolean::impl From<Boolean> for Boolean::from",
    ] {
        assert!(
            labels.iter().all(|label| label != hidden),
            "trait impl member should not be suggested: {hidden}; labels={labels:?}"
        );
    }
}

#[test]
fn core_completion_hides_trait_impl_roots_from_expression_candidates() {
    let engine = engine();
    let labels = engine
        .completions("i", "i".len())
        .candidates
        .into_iter()
        .map(|candidate| candidate.label)
        .collect::<Vec<_>>();

    assert!(
        labels.iter().all(|label| !label.starts_with("impl ")),
        "trait impl roots should not be suggested as expression candidates: {labels:?}"
    );
}

#[test]
fn core_completion_shows_facet_path_candidates_for_type_root() {
    let engine = ReplEngine::from_script_source(
        "tmp/user.srt",
        r#"
defrecord User(name: String, age: Int)
"#,
    )
    .expect("script preload should bootstrap");

    let completion = engine.completions("User.", "User.".len());
    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert!(
        labels.contains(&"name"),
        "type-root facet completion should include record fields: {labels:?}"
    );
    assert!(
        labels.contains(&"age"),
        "type-root facet completion should include record fields: {labels:?}"
    );
    let signature = completion
        .signature
        .as_ref()
        .expect("type-root facet completion should show signature help");
    let rendered = signature.lines.join("\n");
    assert!(
        rendered.contains("User.[field] -> Facet<User, _>"),
        "unexpected facet signature help: {rendered:?}"
    );
}

#[test]
fn core_completion_shows_facet_path_candidates_for_value_root() {
    let mut engine = ReplEngine::from_script_source(
        "tmp/user.srt",
        r#"
defrecord User(name: String, age: Int)
"#,
    )
    .expect("script preload should bootstrap");
    assert!(
        rendered_text(&engine.handle_line(r#"user = User("alice", 42)"#)).contains("user: User")
    );

    let completion = engine.completions("user.", "user.".len());
    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert!(
        labels.contains(&"name"),
        "value-root facet completion should include record fields: {labels:?}"
    );
    assert!(
        labels.contains(&"age"),
        "value-root facet completion should include record fields: {labels:?}"
    );
    let signature = completion
        .signature
        .as_ref()
        .expect("value-root facet completion should show signature help");
    let rendered = signature.lines.join("\n");
    assert!(
        rendered.contains("user.[field] -> _"),
        "unexpected value-root facet signature help: {rendered:?}"
    );
}

#[test]
fn core_completion_shows_facet_view_closure_candidates() {
    let engine = ReplEngine::from_script_source(
        "tmp/user.srt",
        r#"
defrecord User(name: String, age: Int)
"#,
    )
    .expect("script preload should bootstrap");

    let completion = engine.completions("&User.", "&User.".len());
    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert!(
        labels.contains(&"name"),
        "facet view-closure completion should include record fields: {labels:?}"
    );
    assert!(
        labels.contains(&"age"),
        "facet view-closure completion should include record fields: {labels:?}"
    );
    let signature = completion
        .signature
        .as_ref()
        .expect("facet view-closure completion should show signature help");
    let rendered = signature.lines.join("\n");
    assert!(
        rendered.contains("&User.[field] -> (User -> _)"),
        "unexpected facet view-closure signature help: {rendered:?}"
    );
}

#[test]
fn core_completion_shows_result_focus_api_help_for_facet_paths() {
    let engine = ReplEngine::from_script_source(
        "tmp/user.srt",
        r#"
defrecord User(score: Result<Int>)
"#,
    )
    .expect("script preload should bootstrap");

    let completion = engine.completions("User.score.", "User.score.".len());
    let rendered = completion
        .signature
        .as_ref()
        .expect("result focus facet completion should show signature help")
        .lines
        .join("\n");
    assert!(
        rendered.contains("Facet::view(User.score, User) -> Result<Int>"),
        "result focus should use view-based return help: {rendered:?}"
    );
    assert!(
        rendered.contains(
            "Facet::over_result(User.score, User, (Result<Int> -> Result<Result<Int>>)) -> Result<User>"
        ),
        "result focus should expose over_result help: {rendered:?}"
    );
}

#[test]
fn core_completion_shows_combined_list_tuple_result_help() {
    let engine = ReplEngine::from_script_source(
        "tmp/user.srt",
        r#"
defrecord User(scores: List<(String, Result<Int>)>)
"#,
    )
    .expect("script preload should bootstrap");

    let completion = engine.completions("User.scores.[0]._1.", "User.scores.[0]._1.".len());
    let rendered = completion
        .signature
        .as_ref()
        .expect("combined facet path completion should show signature help")
        .lines
        .join("\n");
    assert!(
        rendered.contains("Facet::view(User.scores.[0]._1, User) -> Result<Result<Int>>"),
        "combined facet path should keep list-index fallibility in view help: {rendered:?}"
    );
    assert!(
        rendered.contains("Facet::over_result(User.scores.[0]._1, User, (Result<Int> -> Result<Result<Int>>)) -> Result<User>"),
        "combined facet path should preserve result-focus API help: {rendered:?}"
    );
}

#[test]
fn core_completion_suggests_facet_path_roots_for_facet_api_first_argument() {
    let mut engine = ReplEngine::from_script_source(
        "tmp/user.srt",
        r#"
defrecord User(name: String, age: Int)
defenum Slot { Some(String), None }
"#,
    )
    .expect("script preload should bootstrap");
    assert!(
        rendered_text(&engine.handle_line("name_path = User.name")).contains("Facet<User, String>")
    );

    for api in [
        "view",
        "preview",
        "put",
        "set",
        "over",
        "over_result",
        "case_set",
        "case_over",
    ] {
        let input = format!("Facet::{api}(");
        let completion = engine.completions(&input, input.len());
        let labels = completion
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>();

        assert!(
            labels.contains(&"User"),
            "{api} first argument should suggest path-constructable record roots: {labels:?}"
        );
        assert!(
            labels.contains(&"Slot"),
            "{api} first argument should suggest path-constructable enum roots: {labels:?}"
        );
        assert!(
            labels.contains(&"Boolean"),
            "{api} first argument should suggest Boolean variant root: {labels:?}"
        );
        assert!(
            labels.contains(&"name_path"),
            "{api} first argument should suggest Facet bindings: {labels:?}"
        );
        for primitive in ["String", "Int", "Float", "Function"] {
            assert!(
                !labels.contains(&primitive),
                "{api} first argument should not suggest primitive root {primitive}: {labels:?}"
            );
        }
    }

    for api in [
        "view",
        "preview",
        "put",
        "set",
        "over",
        "over_result",
        "case_set",
        "case_over",
    ] {
        let input = format!("{api}(");
        let completion = engine.completions(&input, input.len());
        let labels = completion
            .candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>();

        assert!(
            labels.contains(&"User"),
            "{api} short-form first argument should suggest path roots: {labels:?}"
        );
        assert!(
            labels.contains(&"name_path"),
            "{api} short-form first argument should suggest Facet bindings: {labels:?}"
        );
        assert!(
            !labels.contains(&"String"),
            "{api} short-form first argument should not suggest primitive roots: {labels:?}"
        );
    }
}

#[test]
fn core_completion_derives_facet_segments_from_facet_binding_focus() {
    let mut engine = ReplEngine::from_script_source(
        "tmp/user.srt",
        r#"
defrecord Profile(first: String, last: String)
defrecord User(profile: Profile, age: Int)
"#,
    )
    .expect("script preload should bootstrap");
    assert!(rendered_text(&engine.handle_line("p = User.profile")).contains("Facet<User, Profile>"));

    let completion = engine.completions("p.", "p.".len());
    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(
        labels.contains(&"first"),
        "Facet binding focus should expose Profile fields: {labels:?}"
    );
    assert!(
        labels.contains(&"last"),
        "Facet binding focus should expose Profile fields: {labels:?}"
    );
    assert!(
        !labels.contains(&"age"),
        "Facet binding completion should use focus type, not original source type: {labels:?}"
    );
}

#[test]
fn core_completion_respects_shadowed_facet_api_names() {
    let mut engine = ReplEngine::from_script_source(
        "tmp/user.srt",
        r#"
defrecord User(name: String)
def view(value: Int) -> Int { value }
"#,
    )
    .expect("script preload should bootstrap");
    assert!(
        rendered_text(&engine.handle_line("name_path = User.name")).contains("Facet<User, String>")
    );
    assert!(rendered_text(&engine.handle_line("n = 1")).contains("n: Int"));

    let completion = engine.completions("view(", "view(".len());
    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(
        !labels.contains(&"User"),
        "shadowed view(Int) should not trigger Facet path roots: {labels:?}"
    );
    assert!(
        labels.contains(&"n"),
        "ordinary argument inference should still offer Int variables: {labels:?}"
    );
}

#[test]
fn core_completion_uses_argument_position_for_variable_candidates_and_signature_help() {
    let mut engine = engine();
    assert!(rendered_text(&engine.handle_line("n = 3")).contains("n: Int"));
    assert!(rendered_text(&engine.handle_line(r#"s = "text""#)).contains("s: String"));

    let input = "Int::min(";
    let completion = engine.completions(input, input.len());
    let signature = completion
        .signature
        .as_ref()
        .expect("signature help should be available at call argument position");
    assert_eq!(signature.active_parameter, Some(0));
    let signature_text = signature.lines.join("\n");
    assert!(signature_text.contains("Int::min("), "{signature_text}");
    assert!(
        signature_text.contains("[Int]"),
        "active parameter type should be highlighted: {signature_text}"
    );

    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert!(
        labels.contains(&"n"),
        "Int binding should be suggested: {labels:?}"
    );
    assert!(
        labels.contains(&"s"),
        "String binding should remain available but ranked lower: {labels:?}"
    );
    assert!(
        labels.iter().position(|label| label == &"n")
            < labels.iter().position(|label| label == &"s"),
        "Int binding should rank before String binding for Int argument: {labels:?}"
    );
}

#[test]
fn core_completion_shows_nested_if_and_string_contains_signatures() {
    let mut engine = engine();
    assert!(rendered_text(&engine.handle_line(r#"word = "Hello""#)).contains("word: String"));
    assert!(rendered_text(&engine.handle_line(r#"needle = "ll""#)).contains("needle: String"));
    assert!(rendered_text(&engine.handle_line("width = 2")).contains("width: Int"));

    let input = "if(String::contains(w";
    let completion = engine.completions(input, input.len());
    let signature = completion
        .signature
        .as_ref()
        .expect("nested call signature help should be visible");
    assert_eq!(signature.active_parameter, Some(0));
    assert_eq!(signature.lines.len(), 2, "{signature:?}");
    assert!(
        signature.lines[0].contains("if(flag: [Boolean]"),
        "{signature:?}"
    );
    assert!(
        signature.lines[1].contains("  String::contains(value: [String], needle: String)"),
        "{signature:?}"
    );

    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert!(
        labels.contains(&"word"),
        "inner prefix should suggest word: {labels:?}"
    );
    assert!(
        labels.contains(&"width"),
        "nonmatching w-prefix candidates should remain visible after ranked String candidates: {labels:?}"
    );
    assert!(
        labels.iter().position(|label| label == &"word")
            < labels.iter().position(|label| label == &"width"),
        "String candidates should rank before nonmatching w-prefix candidates: {labels:?}"
    );
}

#[test]
fn core_completion_keeps_path_candidates_while_typing_if_condition_call() {
    let engine = engine();
    let input = "if(String::c";
    let completion = engine.completions(input, input.len());
    let signature = completion
        .signature
        .as_ref()
        .expect("if signature help should stay visible while typing condition");
    assert!(
        signature
            .lines
            .iter()
            .any(|line| line.contains("if(flag: [Boolean]")),
        "{signature:?}"
    );

    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert!(
        labels.contains(&"String::contains"),
        "String path candidates should be visible inside if condition: {labels:?}"
    );
}

#[test]
fn core_completion_keeps_type_candidates_while_typing_if_condition_prefix() {
    let engine = engine();
    let input = "if(S";
    let completion = engine.completions(input, input.len());
    let signature = completion
        .signature
        .as_ref()
        .expect("if signature help should stay visible while typing condition");
    assert!(
        signature
            .lines
            .iter()
            .any(|line| line.contains("if(flag: [Boolean]")),
        "{signature:?}"
    );

    let labels = completion
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();
    assert!(
        labels.contains(&"String"),
        "String type candidate should be visible inside if condition: {labels:?}"
    );
}

#[test]
fn core_completion_prefers_trait_surface_for_neq_helper_details() {
    let engine = engine();

    let bare = engine.completions("neq", "neq".len());
    let neq = bare
        .candidates
        .iter()
        .find(|candidate| candidate.label == "neq")
        .expect("neq helper should be suggested");
    let bare_detail = neq
        .detail
        .as_deref()
        .expect("neq helper should include detail");
    assert_eq!(
        bare_detail,
        "trait Neq { neq(self: Self, rhs: Self) -> Boolean }"
    );

    let call = engine.completions("neq(", "neq(".len());
    let call_signature = call
        .signature
        .as_ref()
        .expect("neq call-site should show signature help");
    assert_eq!(call_signature.active_parameter, Some(0));
    let call_text = call_signature.lines.join("\n");
    assert_eq!(
        call_text.trim(),
        "trait Neq { neq(self: [Self], rhs: Self) -> Boolean }"
    );

    let inferred = engine.completions("neq(1", "neq(1".len());
    let inferred_signature = inferred
        .signature
        .as_ref()
        .expect("neq(1 should keep signature help visible");
    assert_eq!(inferred_signature.active_parameter, Some(0));
    let inferred_text = inferred_signature.lines.join("\n");
    assert_eq!(
        inferred_text.trim(),
        "trait Neq { neq(self: [Self], rhs: Self) -> Boolean }"
    );

    let second_arg = engine.completions("neq(,)", "neq(,)".len() - 1);
    let second_signature = second_arg
        .signature
        .as_ref()
        .expect("neq(,) should keep signature help visible");
    assert_eq!(second_signature.active_parameter, Some(1));
    let second_text = second_signature.lines.join("\n");
    assert_eq!(
        second_text.trim(),
        "trait Neq { neq(self: Self, rhs: [Self]) -> Boolean }"
    );
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

    let def = engine.handle_line("def add_core(x: Int, y: Int) -> Int { x + y }");
    assert!(!def.should_exit);

    let call = engine.handle_line("add_core(1, 2)");
    assert!(!call.should_exit);
    assert!(rendered_text(&call).contains("3"));

    let value = engine.handle_line("x");
    assert!(!value.should_exit);
    assert!(rendered_text(&value).contains("42"));
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
fn core_rebinding_uses_latest_value_and_grows_snapshot_locals() {
    let mut engine = engine();
    let dir = tempfile_dir("xldr-repl-core-rebind");
    let first_path = dir.join("after-first.eldr");
    let path = dir.join("session.eldr");

    let first = engine.handle_line("x = 1");
    assert!(rendered_text(&first).contains("x: Int = 1"));
    assert_eq!(engine.prompt(), "xldr(2)> ");

    let first_save = engine.handle_line(&format!(":save {}", first_path.display()));
    assert!(rendered_text(&first_save).contains("saved to"));
    let first_bytes = fs::read(&first_path).expect("first .eldr should exist");
    let first_snapshot =
        sindr::ir::Bytecode::decode(&first_bytes).expect("first .eldr should decode");

    let second = engine.handle_line("x = 2");
    assert!(rendered_text(&second).contains("x: Int = 2"));

    let value = engine.handle_line("x");
    assert!(rendered_text(&value).contains("2"));

    let recalled = engine.handle_line(":v 1");
    assert!(
        rendered_text(&recalled).contains("1"),
        "kind={} text={}",
        output_kind(&recalled.output),
        rendered_text(&recalled)
    );

    let save = engine.handle_line(&format!(":save {}", path.display()));
    assert!(rendered_text(&save).contains("saved to"));
    let bytes = fs::read(&path).expect("saved .eldr should exist");
    let snapshot = sindr::ir::Bytecode::decode(&bytes).expect("saved .eldr should decode");
    assert!(
        snapshot.num_locals >= first_snapshot.num_locals + 1,
        "num_locals did not grow: before={}, after={}",
        first_snapshot.num_locals,
        snapshot.num_locals
    );
}

#[test]
fn core_rejects_top_level_def_capturing_session_value_binding() {
    let mut engine = engine();

    let bind = engine.handle_line("x = 1");
    assert!(rendered_text(&bind).contains("x: Int = 1"));

    let def = engine.handle_line("def f() -> Int { x }");
    assert!(!def.should_exit);
    assert!(
        matches!(def.output, ReplOutput::EvalError { .. }),
        "kind={} text={}",
        output_kind(&def.output),
        rendered_text(&def)
    );
    assert!(
        rendered_text(&def).contains("Top-level definition `f` cannot reference value binding `x`")
    );
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
fn core_routes_print_side_effects_into_repl_result_lines() {
    let mut engine = engine();

    let result = engine.handle_line(r#"print("hello from repl")"#);

    assert!(!result.should_exit);
    assert_eq!(visible_text(&result), "hello from repl");
    assert!(result.stderr.is_empty());
}

#[test]
fn core_routes_eprint_side_effects_into_repl_stderr_lines() {
    let mut engine = engine();

    let result = engine.handle_line("eprint(NoneError)");

    assert!(!result.should_exit);
    assert_eq!(rendered_text(&result), "");
    assert!(
        result.stderr.iter().any(|line| line.contains("REPL:")),
        "{:?}",
        result.stderr
    );
}

#[test]
fn core_routes_background_prints_into_pump_result_lines() {
    let mut engine = engine();

    let launched = engine.handle_line(
        r#"_ =? Task::launch({||
  _ =? Process::sleep(5ms)
  print("hello from background")
  Ok(())
})"#,
    );
    assert!(!launched.should_exit, "{}", rendered_text(&launched));

    let background = engine.advance_background_time(Duration::from_millis(5));

    assert!(!background.should_exit);
    assert_eq!(visible_text(&background), "hello from background");
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
fn core_from_project_module_stages_exposes_compiled_project_definitions() {
    let mut engine = ReplEngine::from_project_module_stages(&[vec![xldr::ModuleInput {
        file_name: "math.srt".into(),
        source: r#"
defmod Math {
  def add2(x: Int, y: Int) -> Int { x + y }
}
"#
        .into(),
        module_path: "Math".into(),
    }]])
    .expect("project preload should bootstrap");

    let imported = engine.handle_line("import Math::add2");
    assert!(rendered_text(&imported).contains("Imported Math::add2"));

    let call = engine.handle_line("add2(20, 22)");
    assert!(rendered_text(&call).contains("42"));
}

#[test]
fn core_from_project_runner_source_exposes_selected_profile_definitions() {
    let root = tempfile_dir("xldr-project-runner-repl");
    let src = root.join("src");
    fs::create_dir_all(&src).expect("src dir should be created");
    let project_file = root.join("project.srt");
    let helper_file = src.join("helper.srt");
    fs::write(
        &helper_file,
        r#"
defmod Helper {
  def add2(x: Int, y: Int) -> Int { x + y }
}
"#,
    )
    .expect("helper source should be writable");
    let project_source = r#"
def profile_name() -> String { "dev" }

Project::config({|project|
  Project::entrypoint(project, profile_name(), {|config|
    Config::add_path(config, "./src/helper.srt")
  })
})
"#;

    let mut engine =
        ReplEngine::from_project_runner_source(surtr_analysis::ProjectRunnerSourceInput {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: vec![("profile".to_string(), "dev".to_string())],
            active_file: None,
            source: project_source.to_string(),
        })
        .expect("project runner source should preload REPL context");

    let imported = engine.handle_line("import Helper::add2");
    assert!(rendered_text(&imported).contains("Imported Helper::add2"));

    let call = engine.handle_line("add2(20, 22)");
    assert!(rendered_text(&call).contains("42"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn core_from_project_runner_source_exposes_const_only_file_by_stem_module() {
    let root = tempfile_dir("xldr-project-runner-repl-const-only");
    let src = root.join("src");
    fs::create_dir_all(&src).expect("src dir should be created");
    let project_file = root.join("project.srt");
    let config_file = src.join("AppConfig.srt");
    fs::write(
        &config_file,
        r#"
const APP_NAME = "surtr"
"#,
    )
    .expect("config source should be writable");
    let project_source = r#"
Project::config({|project|
  Project::entrypoint(project, "dev", {|config|
    Config::add_path(config, "./src/AppConfig.srt")
  })
})
"#;

    let mut engine =
        ReplEngine::from_project_runner_source(surtr_analysis::ProjectRunnerSourceInput {
            project_file: project_file.clone(),
            selected_profile: "dev".to_string(),
            normalized_args: Vec::new(),
            active_file: None,
            source: project_source.to_string(),
        })
        .expect("project runner source should preload const-only file");

    let output = engine.handle_line("AppConfig::APP_NAME");
    assert!(rendered_text(&output).contains("surtr"));

    let _ = fs::remove_dir_all(root);
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
fn core_reuses_deferred_tuple_facet_bindings_between_inputs() {
    let mut engine = engine();

    let facet = engine.handle_line("a = Tuple._1");
    assert!(rendered_text(&facet).contains("a: Facet<_, _> = Tuple._1"));

    let pair = engine.handle_line("pair = (\"alice\", 2)");
    assert!(rendered_text(&pair).contains("pair: (String, Int) = (\"alice\", 2)"));

    let value = engine.handle_line("Facet::view(a, pair)");
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
fn core_range_bindings_keep_constructor_and_compare_fun_indices_in_sync() {
    let mut engine = engine();

    let a = engine.handle_line("a = 20ms");
    assert!(!a.should_exit);
    assert!(rendered_text(&a).contains("a: Duration = 20ms"));

    let a_typed = engine.handle_line("a =? 20ms");
    assert!(!a_typed.should_exit);
    assert!(rendered_text(&a_typed).contains("a: Duration = 20ms"));

    let b_typed = engine.handle_line("b =? 10ms");
    assert!(!b_typed.should_exit);
    assert!(rendered_text(&b_typed).contains("b: Duration = 10ms"));

    let range = engine.handle_line("Range(a,b)");
    assert!(!range.should_exit);
    assert!(rendered_text(&range).contains("Range(min: 20ms, max: 10ms)"));

    let normalized = engine.handle_line("Range::normalized(a,b)");
    assert!(!normalized.should_exit);
    assert!(rendered_text(&normalized).contains("Range(min: 10ms, max: 20ms)"));

    let neq = engine.handle_line("Range(b,a) != Range(b, 100ms)");
    assert!(!neq.should_exit);
    let text = rendered_text(&neq);
    assert!(text.contains("True"), "{text}");
    assert!(!text.contains("Unknown function index"), "{text}");
    assert!(!text.contains("Call arity mismatch"), "{text}");
}

#[test]
fn core_range_generic_helpers_survive_sig_doc_interleaving() {
    let mut engine = engine();

    let a = engine.handle_line("a = 20ms");
    assert!(!a.should_exit);
    assert!(rendered_text(&a).contains("a: Duration = 20ms"));

    let sig = signature_text(&engine.handle_line(":sig compare(Duration, Duration)"));
    assert!(sig.contains("compare(Duration, Duration)"), "{sig}");

    let doc = doc_text(&engine.handle_line(":doc Range(Int, Int)"));
    assert!(!doc.contains("Unknown function index"), "{doc}");
    assert!(!doc.contains("Call arity mismatch"), "{doc}");

    let b = engine.handle_line("b =? 10ms");
    assert!(!b.should_exit);
    assert!(rendered_text(&b).contains("b: Duration = 10ms"));

    let neq = engine.handle_line("Range(a,b) != Range(b, 100ms)");
    assert!(!neq.should_exit);
    let text = rendered_text(&neq);
    assert!(text.contains("True"), "{text}");
    assert!(!text.contains("Unknown function index"), "{text}");
    assert!(!text.contains("Call arity mismatch"), "{text}");
}

#[test]
fn core_range_generic_helpers_survive_runtime_error_rollback() {
    let mut engine = engine();

    let a = engine.handle_line("a = 20ms");
    assert!(!a.should_exit);
    assert!(rendered_text(&a).contains("a: Duration = 20ms"));

    let b = engine.handle_line("b =? 10ms");
    assert!(!b.should_exit);
    assert!(rendered_text(&b).contains("b: Duration = 10ms"));

    let runtime_error = engine.handle_line("Process::self()");
    assert!(matches!(runtime_error.output, ReplOutput::EvalError { .. }));
    assert!(rendered_text(&runtime_error).contains("Process::self"));

    let neq = engine.handle_line("Range(a,b) != Range(b, 100ms)");
    assert!(!neq.should_exit);
    let text = rendered_text(&neq);
    assert!(text.contains("True"), "{text}");
    assert!(!text.contains("Unknown function index"), "{text}");
    assert!(!text.contains("Call arity mismatch"), "{text}");
}

#[test]
fn core_renders_top_level_facet_chain_expressions_without_codegen_leak() {
    let mut engine = engine();

    let tuple_facet = engine.handle_line("a = Tuple._1");
    assert!(rendered_text(&tuple_facet).contains("a: Facet<_, _> = Tuple._1"));

    let enum_facet = engine.handle_line("ep = IntBase.Oct");
    assert!(rendered_text(&enum_facet).contains("ep: Facet<IntBase, Unit> = IntBase.Oct"));

    let slash = engine.handle_line("a / ep");
    let slash = rendered_text(&slash);
    assert!(slash.contains("Facet<_, _> = Tuple._1.Oct"), "{slash}");

    let helper = engine.handle_line("Facet::chain(a, ep)");
    let helper = rendered_text(&helper);
    assert!(helper.contains("Facet<_, _> = Tuple._1.Oct"), "{helper}");
}

#[test]
fn core_facet_command_reports_kind_apis_segments_and_stop_points() {
    let mut engine = engine();

    let binding = engine.handle_line("path = Tuple._0");
    assert!(rendered_text(&binding).contains("path: Facet<_, _> = Tuple._0"));

    let facet_info = engine.handle_line(":facet path");
    assert!(matches!(facet_info.output, ReplOutput::StyledDoc { .. }));
    let facet_info = rendered_text(&facet_info);
    assert!(facet_info.contains("## FacetPath"), "{facet_info}");
    assert!(facet_info.contains("type: Facet<_, _>"), "{facet_info}");
    assert!(facet_info.contains("kind: structural"), "{facet_info}");
    assert!(facet_info.contains("view API: Facet::view"), "{facet_info}");
    assert!(
        facet_info.contains("preview API: unavailable"),
        "{facet_info}"
    );
    assert!(facet_info.contains("view result: _"), "{facet_info}");
    assert!(facet_info.contains("full path: Tuple._0"), "{facet_info}");
    assert!(facet_info.contains("## Flow"), "{facet_info}");
    assert!(facet_info.contains("hop 1: Tuple._0"), "{facet_info}");
    assert!(facet_info.contains("relation: _ -> _"), "{facet_info}");

    let legacy = engine.handle_line(":lens path");
    assert!(rendered_text(&legacy).contains("Unknown REPL command: :lens"));

    let fallible = engine.handle_line(":facet BitWidth.Any");
    let fallible = rendered_text(&fallible);
    assert!(fallible.contains("kind: variant"), "{fallible}");
    assert!(
        fallible.contains("preview API: Facet::preview"),
        "{fallible}"
    );
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
fn core_renders_negative_and_range_list_facets() {
    let mut engine = engine();

    let last = engine.handle_line("last = List.[-1]");
    assert!(rendered_text(&last).contains("last: Facet<_, _> = List.[-1]"));

    let window = engine.handle_line("window = List.[1..-1]");
    assert!(rendered_text(&window).contains("window: Facet<_, _> = List.[1..-1]"));

    let info = engine.handle_line(":facet window");
    let info = rendered_text(&info);
    assert!(info.contains("full path: List.[1..-1]"), "{info}");
    assert!(info.contains("fallible: yes"), "{info}");
}

#[test]
fn core_doc_reports_match_and_cond_from_bootstrap_surface() {
    let mut engine = engine();

    let match_doc = engine.handle_line(":doc match");
    let match_doc = doc_text(&match_doc);
    assert!(match_doc.contains("Bootstrap::match"), "{match_doc}");
    assert!(
        match_doc.contains("@intrinsic def match(value: $A, arms: MatchArms<$A, $B>) -> $B"),
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
        cond_doc.contains("@intrinsic def cond(clauses: CondClauses<$A>) -> $A"),
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
        assert!(
            text.contains("Usage: :type <binding|singleton-owner> or :type $<binding>"),
            "{text}"
        );
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
    assert!(help_text.contains(":vars"));
    assert!(help_text.contains(":history [selector]"));
    assert!(help_text.contains(":reload [all|defs]"));
    assert!(help_text.contains(":clear"));

    let sig_help = engine.handle_line(":h sig");
    assert!(rendered_text(&sig_help).contains("Usage: :sig <function|query>"));

    let info_help = engine.handle_line(":help info");
    assert!(rendered_text(&info_help).contains("Usage: :info <query>"));

    let history_help = engine.handle_line(":help history");
    assert!(rendered_text(&history_help).contains("Usage: :history [selector]"));

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

    let bad_sig_query = engine.handle_line(":sig compare(Int, )");
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
fn core_session_listing_commands_render_current_state() {
    let mut engine = ReplEngine::from_preload_sources(
        Some((
            "math.srt",
            r#"
defmod Math {
  def add2(x: Int, y: Int) -> Int { x + y }
}
"#,
        )),
        Some((
            "preload.srt",
            r#"
def greet() -> String { "hi" }
import Math::add2
"#,
        )),
    )
    .expect("preload should bootstrap");

    let _ = engine.handle_line("answer = add2(1, 2)");
    let _ = engine.handle_line("def local_twice(x: Int) -> Int { add2(x, x) }");

    let vars = rendered_text(&engine.handle_line(":vars"));
    assert!(vars.contains("line"), "{vars}");
    assert!(vars.contains("answer"), "{vars}");
    assert!(vars.contains("Int"), "{vars}");

    let imported = rendered_text(&engine.handle_line(":imported"));
    assert!(imported.contains("auto"), "{imported}");
    assert!(imported.contains("Kernel"), "{imported}");
    assert!(imported.contains("Show::to_string"), "{imported}");
    assert!(imported.contains("Math::add2"), "{imported}");

    let defs = rendered_text(&engine.handle_line(":defs"));
    assert!(defs.contains("greet"), "{defs}");
    assert!(defs.contains("local_twice"), "{defs}");

    let history = rendered_text(&engine.handle_line(":history"));
    assert!(history.contains("line"), "{history}");
    assert!(history.contains("answer = add2(1, 2)"), "{history}");
    assert!(history.contains("def local_twice"), "{history}");

    let selected = rendered_text(&engine.handle_line(":history 1, 2"));
    assert!(selected.contains("1:"), "{selected}");
    assert!(selected.contains("2:"), "{selected}");
}

#[test]
fn core_reload_and_clear_commands_preserve_only_requested_state() {
    let mut engine = engine();

    let _ = engine.handle_line("seed = 41");
    let _ = engine.handle_line("def keep() -> Int { 42 }");

    let cleared = rendered_text(&engine.handle_line(":clear"));
    assert!(
        cleared.contains("clear") || cleared.contains("Clear") || cleared.contains("not available"),
        "{cleared}"
    );
    let after_clear = rendered_text(&engine.handle_line("seed"));
    assert!(after_clear.contains("41"), "{after_clear}");

    let reloaded = rendered_text(&engine.handle_line(":reload"));
    assert!(reloaded.contains("reload"), "{reloaded}");

    let keep_after_reload = rendered_text(&engine.handle_line("keep()"));
    assert!(keep_after_reload.contains("42"), "{keep_after_reload}");

    let seed_after_reload = rendered_text(&engine.handle_line("seed"));
    assert!(
        seed_after_reload.contains("not found")
            || seed_after_reload.contains("Unknown symbol")
            || seed_after_reload.contains("Undefined variable"),
        "{seed_after_reload}"
    );

    let _ = engine.handle_line("def drop_me() -> Int { 7 }");
    let reload_defs = rendered_text(&engine.handle_line(":reload defs"));
    assert!(reload_defs.contains("reload"), "{reload_defs}");

    let dropped = rendered_text(&engine.handle_line("drop_me()"));
    assert!(
        dropped.contains("not found")
            || dropped.contains("Unknown symbol")
            || dropped.contains("Undefined variable")
            || dropped.contains("Undefined function"),
        "{dropped}"
    );
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

    let typed_sig = engine.handle_line(":sig compare(Int, Int)");
    let typed_sig = signature_text(&typed_sig);
    assert!(typed_sig.contains("impl Compare for Int::compare(self: Int, rhs: Int) -> Ordering"));

    let helper_sig = engine.handle_line(":sig compare");
    let helper_sig = signature_text(&helper_sig);
    assert_eq!(
        helper_sig.trim(),
        "Compare::compare(self: Self, rhs: Self) -> Ordering"
    );

    let less_than_sig = engine.handle_line(":sig <");
    let less_than_sig = signature_text(&less_than_sig);
    assert_eq!(
        less_than_sig.trim(),
        "Compare::lt(self: Self, rhs: Self) -> Boolean"
    );

    let default_less_than_sig = engine.handle_line(":sig lt");
    let default_less_than_sig = signature_text(&default_less_than_sig);
    assert_eq!(
        default_less_than_sig.trim(),
        "Compare::lt(self: Self, rhs: Self) -> Boolean"
    );

    let neq_helper_sig = engine.handle_line(":sig neq");
    let neq_helper_sig = signature_text(&neq_helper_sig);
    assert_eq!(
        neq_helper_sig.trim(),
        "trait Neq { neq(self: Self, rhs: Self) -> Boolean }"
    );

    let typed_less_than_sig = engine.handle_line(":sig lt(Int, Int)");
    let typed_less_than_sig = signature_text(&typed_less_than_sig);
    assert!(
        typed_less_than_sig
            .contains("defined:\n  impl Compare for Int::lt(self: Int, rhs: Int) -> Boolean"),
        "{typed_less_than_sig}"
    );
    assert!(
        typed_less_than_sig.contains("specialized:\n  lt(Int, Int) -> Boolean"),
        "{typed_less_than_sig}"
    );

    let typed_neq_sig = engine.handle_line(":sig neq(Int, Int)");
    let typed_neq_sig = signature_text(&typed_neq_sig);
    assert!(
        typed_neq_sig.contains("defined:\n  impl Neq for Int::neq(self: Int, rhs: Int) -> Boolean"),
        "{typed_neq_sig}"
    );
    assert!(
        typed_neq_sig.contains("specialized:\n  neq(Int, Int) -> Boolean"),
        "{typed_neq_sig}"
    );

    let operator_sig = engine.handle_line(":sig |>");
    let operator_sig = signature_text(&operator_sig);
    assert!(operator_sig.contains("trait PipeApply { pipe_apply(self: Self, value: $A) -> $B }"));

    let slash_doc = engine.handle_line(":doc /");
    let slash_doc = doc_text(&slash_doc);
    assert!(slash_doc.contains("trait Compose"), "{slash_doc}");
    assert!(slash_doc.contains("models the `/` operator"), "{slash_doc}");

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
        "@intrinsic def match(value: $A, arms: MatchArms<$A, $B>) -> $B"
    );

    let cond_sig = engine.handle_line(":sig cond");
    let cond_sig = signature_text(&cond_sig);
    assert_eq!(
        cond_sig.trim(),
        "@intrinsic def cond(clauses: CondClauses<$A>) -> $A"
    );

    let typed_doc = engine.handle_line(":doc compare(Int, Int)");
    let typed_doc = doc_text(&typed_doc);
    assert!(typed_doc.contains("impl Compare for Int::compare(self: Int, rhs: Int) -> Ordering"));
    assert!(typed_doc.contains("Return the three-way ordering between the two integer values."));
    assert!(
        !typed_doc.contains("\n  Return the three-way ordering between the two integer values.")
    );

    let helper_doc = engine.handle_line(":doc compare");
    let helper_doc = doc_text(&helper_doc);
    assert!(helper_doc.contains("Compare::compare"));
    assert!(helper_doc.contains("Standard `Compare` trait declaration."));

    let neq_helper_doc = engine.handle_line(":doc neq");
    let neq_helper_doc = doc_text(&neq_helper_doc);
    assert!(neq_helper_doc.contains("trait Neq { neq(self: Self, rhs: Self) -> Boolean }"));
    assert!(neq_helper_doc.contains("Standard `Neq` operator trait declaration."));

    let operator_doc = engine.handle_line(":doc <");
    let operator_doc = doc_text(&operator_doc);
    assert!(operator_doc.contains("Compare::lt"));
    assert!(!operator_doc.contains("trait Compare {"), "{operator_doc}");

    let default_less_than_doc = engine.handle_line(":doc lt");
    let default_less_than_doc = doc_text(&default_less_than_doc);
    assert!(default_less_than_doc.contains("Compare::lt"));
    assert!(
        !default_less_than_doc.contains("trait Compare {"),
        "{default_less_than_doc}"
    );

    let typed_less_than_doc = engine.handle_line(":doc lt(Int, Int)");
    let typed_less_than_doc = doc_text(&typed_less_than_doc);
    assert!(
        typed_less_than_doc.contains("impl Compare for Int::lt(self: Int, rhs: Int) -> Boolean"),
        "{typed_less_than_doc}"
    );
    assert!(
        typed_less_than_doc.contains(
            "Return `True` when the left integer is strictly less than the right integer."
        ),
        "{typed_less_than_doc}"
    );

    let typed_neq_doc = engine.handle_line(":doc neq(Int, Int)");
    let typed_neq_doc = doc_text(&typed_neq_doc);
    assert!(
        typed_neq_doc.contains("impl Neq for Int::neq(self: Int, rhs: Int) -> Boolean"),
        "{typed_neq_doc}"
    );
    assert!(
        typed_neq_doc.contains("Return `True` when the integer values differ."),
        "{typed_neq_doc}"
    );

    let constructor_doc = engine.handle_line(":doc Duration(Int)");
    let constructor_doc = doc_text(&constructor_doc);
    assert!(
        constructor_doc.contains("Duration::new(value: Int) -> Result<Duration, Error>"),
        "{constructor_doc}"
    );
    assert!(
        constructor_doc.contains("Construct a `Duration` from a millisecond count."),
        "{constructor_doc}"
    );

    let extractor_doc = engine.handle_line(":doc Duration!()");
    let extractor_doc = doc_text(&extractor_doc);
    assert!(
        extractor_doc.contains("Duration::deconstruct(self: Duration) -> MatchResult<Int, Error>"),
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
        extractor_sig.contains(
            "defined:\n  Duration::deconstruct(self: Duration) -> MatchResult<Int, Error>"
        ),
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
        extractor_sig_explicit_self.contains(
            "defined:\n  Duration::deconstruct(self: Duration) -> MatchResult<Int, Error>"
        ),
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
        "Duration::new(value: Int) -> Result<Duration, Error>"
    );

    let duration_empty_call_sig = engine.handle_line(":sig Duration()");
    let duration_empty_call_sig = signature_text(&duration_empty_call_sig);
    assert_eq!(
        duration_empty_call_sig.trim(),
        "Duration::new(value: Int) -> Result<Duration, Error>"
    );

    let unsupported = engine.handle_line(":doc compare(make_value(), Int)");
    assert!(
        rendered_text(&unsupported).contains("Unsupported command query argument `make_value()`"),
        "{}",
        rendered_text(&unsupported)
    );
}

#[test]
fn core_compare_typed_queries_fall_back_to_trait_default_methods_when_impl_override_is_missing() {
    let mut engine = ReplEngine::from_script_source(
        "compare_default.srt",
        r#"
defstruct Ranked {
  weight: Int,
}

impl Ranked {
  def new(weight: Int) -> Self {
    Ranked { weight: weight }
  }
}

impl Compare for Ranked {
  @doc """Compare ranked values by weight."""
  def compare(self: Self, rhs: Self) -> Ordering {
    Compare::compare(self.weight, rhs.weight)
  }
}
"#,
    )
    .expect("compare default preload should bootstrap");

    let sig = engine.handle_line(":sig lt(Ranked, Ranked)");
    let sig = signature_text(&sig);
    assert!(
        sig.contains("defined:\n  Compare::lt(self: Self, rhs: Self) -> Boolean"),
        "{sig}"
    );
    assert!(
        sig.contains("specialized:\n  lt(Ranked, Ranked) -> Boolean"),
        "{sig}"
    );

    let doc = engine.handle_line(":doc lt(Ranked, Ranked)");
    let doc = doc_text(&doc);
    assert!(
        doc.contains("Compare::lt(self: Self, rhs: Self) -> Boolean"),
        "{doc}"
    );
    assert!(doc.contains("Compare::lt"), "{doc}");
    assert!(!doc.contains("trait Compare {"), "{doc}");
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
    assert!(point_sig.contains("-> StyledDocStyle"), "{point_sig}");

    let style = rendered_text(&engine.handle_line(
        "style = StyledDocStyle::new(Option::None, Option::None, True, False, False, False)",
    ));
    assert!(style.contains("style: StyledDocStyle"), "{style}");
}

#[test]
fn core_sig_record_owner_uses_record_constructor_surface() {
    let mut engine = ReplEngine::from_script_source(
        "record_sig.srt",
        r#"
defrecord ScoreFixture(scores: List<Int>, score: HashMap<Int>)
"#,
    )
    .expect("record preload should bootstrap");

    let sig = signature_text(&engine.handle_line(":sig ScoreFixture"));
    assert_eq!(
        sig.trim(),
        "ScoreFixture(scores: List<Int>, score: HashMap<Int>) -> ScoreFixture"
    );
}

#[test]
fn core_range_constructor_and_extractor_queries_use_repl_docs_and_signature_fallbacks() {
    let mut engine = engine();

    let constructor_doc = doc_text(&engine.handle_line(":doc Range(Int, Int)"));
    assert!(constructor_doc.contains("Range::new"), "{constructor_doc}");
    assert!(constructor_doc.contains("min: $A"), "{constructor_doc}");
    assert!(constructor_doc.contains("max: $A"), "{constructor_doc}");
    assert!(
        constructor_doc.contains("-> Range<$A>"),
        "{constructor_doc}"
    );
    assert!(
        constructor_doc.contains("Construct a range while preserving the input order."),
        "{constructor_doc}"
    );

    let range_sig = signature_text(&engine.handle_line(":sig Range"));
    assert!(range_sig.contains("Range::new"), "{range_sig}");
    assert!(range_sig.contains("min: $A"), "{range_sig}");
    assert!(range_sig.contains("max: $A"), "{range_sig}");
    assert!(range_sig.contains("-> Range<$A>"), "{range_sig}");

    let range_empty_call_sig = signature_text(&engine.handle_line(":sig Range()"));
    assert_eq!(range_empty_call_sig.trim(), range_sig.trim());

    let extractor_doc = doc_text(&engine.handle_line(":doc Range!()"));
    assert!(
        extractor_doc.contains("Range::deconstruct"),
        "{extractor_doc}"
    );
    assert!(
        extractor_doc.contains("MatchResult<($A, $A), Error>"),
        "{extractor_doc}"
    );
    assert!(
        extractor_doc.contains("Deconstruct a `Range` into `(min, max)` in pattern position."),
        "{extractor_doc}"
    );

    let extractor_sig = signature_text(&engine.handle_line(":sig Range!()"));
    assert!(
        extractor_sig.contains(
            "defined:\n  Range::deconstruct<$A>(self: Range<$A>) -> MatchResult<($A, $A), Error>"
        ),
        "{extractor_sig}"
    );
    assert!(
        extractor_sig.contains("specialized:\n  Range!() -> MatchResult<($A, $A), Error>"),
        "{extractor_sig}"
    );

    let extractor_sig_no_args = signature_text(&engine.handle_line(":sig Range!"));
    assert_eq!(extractor_sig_no_args.trim(), extractor_sig.trim());
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
fn core_process_doc_and_sig_support_hidden_and_concrete_surfaces() {
    let mut engine = process_engine();

    let hidden_doc = doc_text(&engine.handle_line(":doc GenServer::spawn"));
    assert!(hidden_doc.contains("GenServer::spawn"), "{hidden_doc}");
    assert!(
        hidden_doc.contains("Compiler-managed lower target for GenServer worker spawn."),
        "{hidden_doc}"
    );

    let concrete_doc = doc_text(&engine.handle_line(":doc MySup::status"));
    assert!(concrete_doc.contains("MySup::status"), "{concrete_doc}");
    assert!(
        concrete_doc.contains("Compiler-managed lower target for reading supervisor status."),
        "{concrete_doc}"
    );
    assert!(
        !concrete_doc.contains("Supervisor::status(supervisor:"),
        "{concrete_doc}"
    );

    let hidden_sig = signature_text(&engine.handle_line(":sig Supervisor::status"));
    assert!(
        hidden_sig
            .contains("Supervisor::status(supervisor: $Supervisor) -> Result<SupervisorStatus>"),
        "{hidden_sig}"
    );

    let concrete_sig = signature_text(&engine.handle_line(":sig MySup::status"));
    assert!(
        concrete_sig.contains("MySup::status() -> Result<SupervisorStatus, Error>"),
        "{concrete_sig}"
    );

    let hidden_pid_doc = doc_text(&engine.handle_line(":doc Agent::pid"));
    assert!(hidden_pid_doc.contains("Agent::pid"), "{hidden_pid_doc}");
    assert!(
        hidden_pid_doc.contains("Compiler-managed lower target for Agent singleton PID lookup."),
        "{hidden_pid_doc}"
    );

    let concrete_pid_doc = doc_text(&engine.handle_line(":doc MyServer::pid"));
    assert!(
        concrete_pid_doc.contains("MyServer::pid"),
        "{concrete_pid_doc}"
    );
    assert!(
        concrete_pid_doc
            .contains("Compiler-managed lower target for GenServer singleton PID lookup."),
        "{concrete_pid_doc}"
    );
    assert!(
        !concrete_pid_doc.contains("GenServer::pid(owner:"),
        "{concrete_pid_doc}"
    );

    let hidden_pid_sig = signature_text(&engine.handle_line(":sig GenServer::pid"));
    assert!(
        hidden_pid_sig
            .contains("GenServer::pid(owner: $Owner, init: (-> Result<$State>)) -> PID<$Process>"),
        "{hidden_pid_sig}"
    );

    let concrete_pid_sig = signature_text(&engine.handle_line(":sig MyServer::pid"));
    assert!(
        concrete_pid_sig.contains("MyServer::pid() -> PID<MyServer>"),
        "{concrete_pid_sig}"
    );
}

#[test]
fn core_process_public_surface_respects_annotations() {
    let mut engine = process_engine();

    let public_sig = signature_text(&engine.handle_line(":sig MyServer::size"));
    assert!(
        public_sig.contains("MyServer::size() -> Result<Int, Error>"),
        "{public_sig}"
    );

    let public_doc = doc_text(&engine.handle_line(":doc MyWorker::read"));
    assert!(public_doc.contains("MyWorker::read"), "{public_doc}");

    let private_sig = rendered_text(&engine.handle_line(":sig MyServer::hidden_size"));
    assert!(
        private_sig
            .contains("`MyServer::hidden_size` is private and cannot be queried with `:sig`."),
        "{private_sig}"
    );
    assert!(
        private_sig.contains("Only public declarations are visible to REPL signature lookup."),
        "{private_sig}"
    );

    let private_doc = rendered_text(&engine.handle_line(":doc MyWorker::hidden_value"));
    assert!(
        private_doc
            .contains("`MyWorker::hidden_value` is private and cannot be queried with `:doc`."),
        "{private_doc}"
    );
    assert!(
        private_doc.contains("Add `@doc` only to public declarations."),
        "{private_doc}"
    );

    let annotation_query = rendered_text(&engine.handle_line(":sig @call"));
    assert!(
        annotation_query.contains("No signature found"),
        "{annotation_query}"
    );
}

#[test]
fn core_process_sig_owner_summary_includes_init_pid_and_messages() {
    let mut engine = process_engine();

    let owner_sig = signature_text(&engine.handle_line(":sig MyServer"));
    assert!(owner_sig.contains("GenServer MyServer"), "{owner_sig}");
    assert!(
        owner_sig.contains("@init init() -> Result<PID<MyServer>>"),
        "{owner_sig}"
    );
    assert!(
        owner_sig.contains("@pid pid() -> PID<MyServer>"),
        "{owner_sig}"
    );
    assert!(
        owner_sig.contains("@call size(pid: PID<MyServer>) -> Result<Int, Error>"),
        "{owner_sig}"
    );
}

#[test]
fn core_process_sig_worker_owner_summary_includes_init_and_messages() {
    let mut engine = process_engine();

    let owner_sig = signature_text(&engine.handle_line(":sig MyWorker"));
    assert!(owner_sig.contains("Agent MyWorker"), "{owner_sig}");
    assert!(
        owner_sig.contains("@init init(seed: Int) -> Result<PID<MyWorker>, Error>"),
        "{owner_sig}"
    );
    assert!(
        owner_sig.contains("@get read(pid: PID<MyWorker>) -> Result<Int, Error>"),
        "{owner_sig}"
    );
    assert!(
        owner_sig.contains("@set write(pid: PID<MyWorker>, next: Int) -> Result<Unit, Error>"),
        "{owner_sig}"
    );
    assert!(!owner_sig.contains("@pid"), "{owner_sig}");
}

#[test]
fn core_process_sig_pid_binding_lists_available_messages() {
    let mut engine = process_engine();

    let bind = engine.handle_line("server = MyServer::pid()");
    assert!(rendered_text(&bind).contains("server: PID<MyServer>"));

    let pid_sig = signature_text(&engine.handle_line(":sig $server"));
    assert!(pid_sig.contains("PID<MyServer> messaging"), "{pid_sig}");
    assert!(
        pid_sig.contains("@call size(pid: PID<MyServer>) -> Result<Int, Error>"),
        "{pid_sig}"
    );
    assert!(!pid_sig.contains("@init"), "{pid_sig}");
    assert!(!pid_sig.contains("@pid"), "{pid_sig}");
}

#[test]
fn core_process_type_and_info_support_singletons_and_worker_pids() {
    let mut engine = process_engine();

    let singleton_type = rendered_text(&engine.handle_line(":type MyServer"));
    assert!(
        singleton_type.contains("type: PID<MyServer>"),
        "{singleton_type}"
    );
    assert!(
        !singleton_type.contains("<pid>") && !singleton_type.contains("pid:"),
        "{singleton_type}"
    );

    let singleton_info = rendered_text(&engine.handle_line(":info MyServer"));
    assert!(
        singleton_info.contains("defined: PID<MyServer>"),
        "{singleton_info}"
    );
    assert!(
        singleton_info.contains("instance: Singleton"),
        "{singleton_info}"
    );

    let spawn = engine.handle_line("pid =? MyWorker::init(1)");
    let spawn_text = rendered_text(&spawn);
    assert!(spawn_text.contains("pid: PID<MyWorker>"), "{spawn_text}");
    assert!(!spawn_text.contains("PID<$Process>"), "{spawn_text}");

    let worker_type = rendered_text(&engine.handle_line(":type pid"));
    assert!(worker_type.contains("type: PID<MyWorker>"), "{worker_type}");
    assert!(
        !worker_type.contains("<pid>") && !worker_type.contains("pid: <"),
        "{worker_type}"
    );

    let worker_info = rendered_text(&engine.handle_line(":info pid"));
    assert!(
        worker_info.contains("defined: PID<MyWorker>"),
        "{worker_info}"
    );
    assert!(worker_info.contains("instance: Worker"), "{worker_info}");
    assert!(worker_info.contains("runtime kind:"), "{worker_info}");
    assert!(!worker_info.contains("process:"), "{worker_info}");
    assert!(!worker_info.contains("<pid>"), "{worker_info}");

    let singleton_pid_binding = rendered_text(&engine.handle_line("server = MyServer::pid()"));
    assert!(
        singleton_pid_binding.contains("server: PID<MyServer>"),
        "{singleton_pid_binding}"
    );

    let singleton_pid_info = rendered_text(&engine.handle_line(":info server"));
    assert!(
        singleton_pid_info.contains("kind: process pid"),
        "{singleton_pid_info}"
    );
    assert!(
        singleton_pid_info.contains("defined: PID<MyServer>"),
        "{singleton_pid_info}"
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
        sig.contains("defined:\n  Function::id(value: $A) -> $A"),
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
fn core_completion_shows_signature_for_callable_binding_calls() {
    let mut engine = engine();

    let closure = engine.handle_line("formatter = {|value: Int| to_string(value)}");
    let closure_text = rendered_text(&closure);
    assert!(
        closure_text.contains("formatter: (Int -> String)"),
        "{closure_text}"
    );

    let completion = engine.completions("formatter(", "formatter(".len());
    let signature = completion
        .signature
        .as_ref()
        .expect("callable binding call-site should show signature help");
    assert_eq!(signature.active_parameter, Some(0));
    assert_eq!(signature.lines.join("\n"), "formatter([Int]) -> String");
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
    assert!(rendered_text(&trait_helper).contains(
        "Trait helper `concat` needs expected callable type or same-expression inference evidence"
    ));

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
fn core_eldr_sig_queries_do_not_depend_on_docs_chunk() {
    let mut engine = engine();
    let dir = tempfile_dir("xldr-repl-core-eldr-sig-without-docs");
    let path = dir.join("session.eldr");

    let save = engine.handle_line(&format!(":save {}", path.display()));
    assert!(rendered_text(&save).contains("saved to"));

    let bytes = fs::read(&path).expect("saved .eldr should exist");
    let mut bytecode = sindr::ir::Bytecode::decode(&bytes).expect("saved .eldr should decode");
    bytecode.docs.clear();
    let bytes = bytecode.encode().expect("modified .eldr should encode");

    let mut restored = ReplEngine::from_eldr(&bytes).expect("restored engine should load");

    let doc = rendered_text(&restored.handle_line(":doc compare(Int, Int)"));
    assert!(
        doc.contains("No docs found for compare(Int, Int)"),
        "doc query should read only Docs chunk: {doc}"
    );

    let sig = signature_text(&restored.handle_line(":sig compare(Int, Int)"));
    assert!(
        sig.contains("impl Compare for Int::compare(self: Int, rhs: Int) -> Ordering"),
        "{sig}"
    );
    assert!(
        sig.contains("specialized:\n  compare(Int, Int) -> Ordering"),
        "{sig}"
    );
}

#[test]
fn core_eldr_restore_reports_partial_semantic_restore_notice() {
    let mut engine = engine();
    let dir = tempfile_dir("xldr-repl-core-eldr-partial-restore");
    let path = dir.join("session.eldr");

    let save = engine.handle_line(&format!(":save {}", path.display()));
    assert!(rendered_text(&save).contains("saved to"));

    let bytes = fs::read(&path).expect("saved .eldr should exist");
    let mut restored = ReplEngine::from_eldr(&bytes).expect("restored engine should load");
    let startup = restored.take_startup_results();
    assert!(
        startup
            .iter()
            .any(|result| rendered_text(result).contains("compile semantic metadata")),
        "startup results should report partial semantic restore: {:?}",
        startup.iter().map(rendered_text).collect::<Vec<_>>()
    );
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
        tuple_doc.contains("Facet::view(Tuple._1, pair)"),
        "{tuple_doc}"
    );
    assert!(
        tuple_doc.contains("Facet::set(Tuple._1, pair, 3)"),
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
        style_doc.contains("defstruct StyledDocStyle"),
        "{style_doc}"
    );
    assert!(style_doc.contains("StyledDocStyle.bold"), "{style_doc}");
    assert!(style_doc.contains("lines.[0]"), "{style_doc}");

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
fn core_sig_supports_tuple_field_sugar_and_facet_expression_queries() {
    let mut engine = engine();

    let pair = engine.handle_line("pair = (\"alice\", 2)");
    let pair_text = rendered_text(&pair);
    assert!(pair_text.contains("pair: (String, Int)"), "{pair_text}");

    let field_sig = engine.handle_line(":sig pair._1");
    let field_sig = signature_text(&field_sig);
    assert!(field_sig.contains("defined:"), "{field_sig}");
    assert!(
        field_sig.contains("Facet::view(Tuple._1, pair)"),
        "{field_sig}"
    );
    assert!(field_sig.contains("specialized:"), "{field_sig}");
    assert!(field_sig.contains("pair._1: Int"), "{field_sig}");

    let view_sig = engine.handle_line(":sig pair._1");
    let view_sig = signature_text(&view_sig);
    assert!(view_sig.contains("defined:"), "{view_sig}");
    assert!(view_sig.contains("Facet::view("), "{view_sig}");
    assert!(view_sig.contains("specialized:"), "{view_sig}");
    assert!(view_sig.contains("pair._1: Int"), "{view_sig}");

    let result_pair = engine.handle_line("result_pair = (Ok(2), \"ok\")");
    let result_pair_text = rendered_text(&result_pair);
    assert!(
        result_pair_text.contains("result_pair"),
        "{result_pair_text}"
    );

    let chain_sig =
        engine.handle_line(":sig Facet::chain(StyledDocSegment.style, StyledDocStyle.bold)");
    let chain_sig = rendered_text(&chain_sig);
    assert!(
        chain_sig.contains("Unsupported command query argument `StyledDocSegment.style`"),
        "{chain_sig}"
    );

    let over_result_sig = engine.handle_line(
        ":sig Facet::over_result(Tuple._0, result_pair, {|value: Result<Int>| Ok(value)})",
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
