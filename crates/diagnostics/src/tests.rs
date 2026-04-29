use super::*;
use crate::heuristics::*;
use crate::render::write_fallback_diagnostic;
use ariadne::Fmt;
use scar::error::TypeError;
use spire::ast::Span;
use std::io::{self, Write};

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("writer failed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("writer failed"))
    }
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
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
fn fallback_diagnostic_preserves_core_fields() {
    let spec = DiagnosticSpec {
        kind: "TypeError".into(),
        message: "expected Int, got String".into(),
        primary_span: Span { start: 13, end: 23 },
        labels: vec![DiagnosticLabel {
            source_id: None,
            span: Span { start: 13, end: 23 },
            message: "binding value".into(),
            color: Some(Color::Blue),
        }],
        notes: Vec::new(),
        help: Some("The type annotation requires Int".into()),
    };

    let mut buf = Vec::new();
    write_fallback_diagnostic(
        &mut buf,
        "main.srt",
        &spec,
        &io::Error::other("broken pipe"),
    )
    .expect("fallback output should succeed");

    let text = String::from_utf8(buf).expect("fallback output must be valid utf-8");
    assert!(text.contains("diagnostic rendering failed: broken pipe"));
    assert!(text.contains("TypeError: expected Int, got String"));
    assert!(text.contains("--> main.srt:13-23"));
    assert!(text.contains("= note: binding value [13-23]"));
    assert!(text.contains("= help: The type annotation requires Int"));
}

#[test]
fn fallback_diagnostic_returns_writer_error() {
    let spec = simple_error(
        "ParseError",
        "unexpected token",
        Span { start: 2, end: 5 },
        None,
    );
    let err = write_fallback_diagnostic(
        &mut FailingWriter,
        "main.srt",
        &spec,
        &io::Error::other("broken pipe"),
    )
    .expect_err("failing writer should propagate io error");

    assert_eq!(err.kind(), io::ErrorKind::Other);
}

#[test]
fn source_registry_registers_and_updates_entries() {
    let mut sources = SourceRegistry::new();
    let src_id = sources.register("main.srt", "x = 1");

    assert_eq!(sources.file_name(src_id), Some("main.srt"));
    assert_eq!(sources.source(src_id), Some("x = 1"));

    assert!(sources.update_source(src_id, "x = 2"));
    assert_eq!(sources.source(src_id), Some("x = 2"));
}

#[test]
fn source_registry_returns_owned_context() {
    let mut sources = SourceRegistry::new();
    let src_id = sources.register("script.srt", "print(\"ok\")");

    let context = sources
        .owned_context(src_id)
        .expect("registered source should exist");
    assert_eq!(context.0, "print(\"ok\")");
    assert_eq!(context.1, "script.srt");
}

#[test]
fn render_error_normalizes_out_of_range_span_to_keep_source_snippet() {
    let spec = simple_error(
        "TypeError",
        "expected Int, got String",
        Span {
            start: 9999,
            end: 10000,
        },
        None,
    );
    let rendered = render_error("main.srt", "bad: Int = \"x\"", &spec);
    assert!(
        rendered.contains("TypeError: expected Int, got String"),
        "expected headline in rendered diagnostic"
    );
    assert!(
        rendered.contains("main.srt"),
        "expected file label in rendered diagnostic"
    );
}

#[test]
fn render_error_normalizes_empty_span_to_single_point_label() {
    let spec = simple_error(
        "ParseError",
        "unexpected token",
        Span { start: 4, end: 4 },
        None,
    );
    let rendered = render_error("main.srt", "abcde", &spec);
    assert!(rendered.contains("ParseError: unexpected token"));
    assert!(rendered.contains("main.srt"));
}

#[test]
fn serializable_report_uses_character_offsets_for_utf8_source() {
    let mut sources = SourceRegistry::new();
    let source_id = sources.register("main.srt", "あx");
    let spec = simple_error(
        "TypeError",
        "expected Int, got String",
        Span { start: 1, end: 2 },
        None,
    );

    let report = serializable_report_by_id(&sources, source_id, "typecheck", &spec);
    assert_eq!(report.errors[0].line, 1);
    assert_eq!(report.errors[0].column, 2);
    assert_eq!(report.errors[0].span, [1, 2]);
}

#[test]
fn char_span_to_byte_range_converts_only_at_render_boundary() {
    let range = char_span_to_byte_range("あx", &Span { start: 1, end: 2 });
    assert_eq!(range, "あ".len().."あx".len());
}

#[test]
fn type_error_spec_labels_backtick_operator_operands() {
    let source = "bad = 1 `+` \"oops\"";
    let err = TypeError {
            message: "Cannot apply Add to Int and String".into(),
            span: Span { start: 6, end: 18 },
            hint: Some(
                "Operator `Add` requires compatible operand types. Left operand is Int, right operand is String."
                    .into(),
            ),
        };

    let spec = type_error_spec(source, &err);
    let notes_text = spec
        .notes
        .iter()
        .map(|note| strip_ansi(note))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(spec
        .labels
        .iter()
        .any(|label| strip_ansi(&label.message) == "LHS actual: Int"));
    assert!(spec
        .labels
        .iter()
        .any(|label| strip_ansi(&label.message) == "   OP rule: A + A -> A (where A: Add)"));
    assert!(spec
        .labels
        .iter()
        .any(|label| strip_ansi(&label.message) == "RHS actual: String"));
    assert!(notes_text.contains("Step: Int + String -> <type error>"));
    assert!(notes_text.contains(
        "Reason: `+` requires the same operator trait type on both sides, but got Int and String."
    ));
    assert!(spec
        .help
        .as_deref()
        .is_some_and(|help| help.contains("same Add type")));
}

#[test]
fn type_error_spec_picks_symbol_operator_outside_literals() {
    let source = r#""+" + "value""#;
    let rhs_start = source.rfind("\"value\"").expect("rhs literal");
    let err = TypeError {
            message: "Cannot apply Add to String and String".into(),
            span: Span {
                start: rhs_start,
                end: source.chars().count(),
            },
            hint: Some(
                "Operator `Add` requires compatible operand types. Left operand is String, right operand is String."
                    .into(),
            ),
        };

    let spec = type_error_spec(source, &err);
    let op = spec
        .labels
        .iter()
        .find(|label| strip_ansi(&label.message) == "   OP rule: A + A -> A (where A: Add)")
        .expect("operator label");
    let lhs = spec
        .labels
        .iter()
        .find(|label| strip_ansi(&label.message) == "LHS actual: String")
        .expect("lhs label");

    assert_eq!(slice_chars(source, op.span.start, op.span.end), "+");
    assert_eq!(slice_chars(source, lhs.span.start, lhs.span.end), r#""+""#);
}

#[test]
fn type_error_spec_formats_eq_operator_with_three_captions() {
    let source = "print(to_string(1 == True))";
    let err = TypeError {
        message: "Cannot compare Int and Boolean".into(),
        span: Span { start: 16, end: 25 },
        hint: None,
    };

    let spec = type_error_spec(source, &err);
    let notes_text = spec
        .notes
        .iter()
        .map(|note| strip_ansi(note))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(spec
        .labels
        .iter()
        .any(|label| strip_ansi(&label.message) == "LHS actual: Int"));
    assert!(spec
        .labels
        .iter()
        .any(|label| strip_ansi(&label.message) == "   OP rule: A == A -> Boolean"));
    assert!(spec
        .labels
        .iter()
        .any(|label| strip_ansi(&label.message) == "RHS actual: Boolean"));
    assert!(notes_text.contains("Step: Int == Boolean -> Boolean"));
    assert!(notes_text
        .contains("Reason: `==` compares two values of the same type, but got Int and Boolean."));
}

#[test]
fn type_error_spec_distinguishes_neq_operator_from_source() {
    let source = "print(to_string(1 != True))";
    let err = TypeError {
        message: "Cannot compare Int and Boolean".into(),
        span: Span { start: 16, end: 25 },
        hint: None,
    };

    let spec = type_error_spec(source, &err);
    let notes_text = spec
        .notes
        .iter()
        .map(|note| strip_ansi(note))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(spec
        .labels
        .iter()
        .any(|label| strip_ansi(&label.message) == "   OP rule: A != A -> Boolean"));
    assert!(notes_text.contains("Step: Int != Boolean -> Boolean"));
    assert!(notes_text
        .contains("Reason: `!=` compares two values of the same type, but got Int and Boolean."));
}

#[test]
fn type_error_spec_distinguishes_lt_operator_from_source() {
    let source = "print(to_string(1 < True))";
    let err = TypeError {
        message: "Cannot compare Int and Boolean".into(),
        span: Span { start: 16, end: 24 },
        hint: None,
    };

    let spec = type_error_spec(source, &err);
    let notes_text = spec
        .notes
        .iter()
        .map(|note| strip_ansi(note))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(spec
        .labels
        .iter()
        .any(|label| strip_ansi(&label.message) == "   OP rule: A < A -> Boolean (where A: Lt)"));
    assert!(notes_text.contains("Step: Int < Boolean -> Boolean"));
    assert!(notes_text.contains(
        "Reason: `<` compares two ordered values of the same type, but got Int and Boolean."
    ));
}

#[test]
fn type_error_spec_formats_concat_operator_with_three_captions() {
    let source = "print(1 ++ \"x\")";
    let err = TypeError {
        message: "++ requires (String, String), got (Int, String)".into(),
        span: Span { start: 6, end: 14 },
        hint: None,
    };

    let spec = type_error_spec(source, &err);
    let notes_text = spec
        .notes
        .iter()
        .map(|note| strip_ansi(note))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(spec
        .labels
        .iter()
        .any(|label| strip_ansi(&label.message) == "LHS actual: Int"));
    assert!(spec
        .labels
        .iter()
        .any(|label| strip_ansi(&label.message) == "   OP rule: String ++ String -> String"));
    assert!(spec
        .labels
        .iter()
        .any(|label| strip_ansi(&label.message) == "RHS actual: String"));
    assert!(notes_text.contains("Step: Int ++ String -> String"));
    assert!(notes_text.contains("Reason: `++` is string concatenation, but got Int and String."));
}

#[test]
fn type_error_spec_colors_generic_binary_operator_note_only() {
    let source = "bad = 1 `*` \"oops\"";
    let err = TypeError {
        message: "Cannot apply Mul to Int and String".into(),
        span: Span { start: 6, end: 18 },
        hint: None,
    };

    let spec = type_error_spec(source, &err);
    let labels_text = spec
        .labels
        .iter()
        .map(|label| strip_ansi(&label.message))
        .collect::<Vec<_>>()
        .join("\n");
    let notes_text = spec.notes.join("\n");

    assert!(labels_text.contains("LHS actual: Int"));
    assert!(labels_text.contains("RHS actual: String"));
    assert!(!labels_text.contains("\u{1b}[31m"));
    assert!(notes_text.contains(&"String".fg(Color::Red).to_string()));
}

#[test]
fn type_error_spec_labels_flow_operator_parts() {
    let source = "bad = parse(1) |> &inc";
    let err = TypeError {
            message: "`|>` type mismatch: expected Int, got Result<Int>".into(),
            span: Span { start: 6, end: 14 },
            hint: Some(
                "`|>` signature rule: LHS: A; RHS: (A -> B); result: B. LHS: Result<Int>. RHS: (Int -> Int)."
                    .into(),
            ),
        };

    let spec = type_error_spec(source, &err);
    let label_text = spec
        .labels
        .iter()
        .map(|label| strip_ansi(&label.message))
        .collect::<Vec<_>>()
        .join("\n");
    let notes_text = spec
        .notes
        .iter()
        .map(|note| strip_ansi(note))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(label_text.contains("LHS actual: Result<Int>"));
    assert!(label_text.contains("OP rule: A |> (A -> B) -> B"));
    assert!(label_text.contains("RHS actual: (Int -> Int)"));
    assert!(notes_text.contains("Step: Result<Int> |> (Int -> Int) -> Int"));
    assert!(notes_text.contains("Reason: RHS expects Int, but LHS is Result<Int>."));
    assert!(spec
        .help
        .as_deref()
        .is_some_and(|help| help.contains("Use `|*>`")));
}

#[test]
fn type_error_spec_splits_annotation_mismatch_on_same_line() {
    let source = "result: Int = make_text(1)";
    let err = TypeError {
        message: "expected Int, got String".into(),
        span: Span {
            start: 0,
            end: source.chars().count(),
        },
        hint: None,
    };

    let spec = type_error_spec(source, &err);

    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "LHS annotation: Int"));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "RHS expression: String"));

    let rendered = strip_ansi(&render_error("main.srt", source, &spec));
    assert!(rendered.contains("LHS annotation: Int"));
    assert!(rendered.contains("RHS expression: String"));
    assert!(!rendered.contains("╰── expected Int, got String"));
}

#[test]
fn type_error_spec_splits_multiline_annotation_mismatch() {
    let source = "ret: String =\n    fun1()\n    |> fun2()\n    |> fun3()";
    let fun3_start = source.find("fun3").expect("source has final rhs call");
    let err = TypeError {
        message: "expected String, got Int".into(),
        span: Span {
            start: fun3_start,
            end: fun3_start + "fun3()".len(),
        },
        hint: None,
    };

    let spec = type_error_spec(source, &err);
    let lhs = spec
        .labels
        .iter()
        .find(|label| label.message == "LHS annotation: String")
        .expect("lhs label");
    let rhs = spec
        .labels
        .iter()
        .find(|label| label.message == "RHS expression: Int")
        .expect("rhs label");

    assert_eq!(
        slice_chars(source, lhs.span.start, lhs.span.end),
        "ret: String ="
    );
    assert_eq!(slice_chars(source, rhs.span.start, rhs.span.end), "fun3()");
}

#[test]
fn render_flow_operator_error_keeps_actual_types_out_of_help() {
    let source = "bad = 1 |>= &inc";
    let err = TypeError {
            message: "`|>=` requires Result or List on the left, got Int".into(),
            span: Span { start: 6, end: 7 },
            hint: Some(
                "`|>=` signature rule: LHS: Result<A, E> or List<A>; RHS: (A -> Result<B, E>) or (A -> List<B>); result: Result<B, E> or List<B>. LHS: Int. RHS: (Int -> Result<Int>). Operators share precedence and resolve left-to-right, so LHS is the type produced so far."
                    .into(),
            ),
        };

    let spec = type_error_spec(source, &err);
    let rendered = render_error("main.srt", source, &spec);
    let rendered_plain = strip_ansi(&rendered);

    assert!(rendered_plain.contains("LHS actual: Int"));
    assert!(rendered_plain.contains("OP rule: Result<A> |>= (A -> Result<B>) -> Result<B>"));
    assert!(rendered_plain.contains("Step: Int |>= (Int -> Result<Int>) -> Result<Int>"));
    assert!(rendered_plain.contains("Reason: LHS is Int, but `|>=` requires Result<A> or List<A>."));
    assert_eq!(
        spec.help.as_deref(),
        Some("Use `|>` for a plain value, or make the LHS Result/List.")
    );
    assert!(!spec.help.as_deref().unwrap_or("").contains("Int"));
}

#[test]
fn type_error_spec_labels_repl_flow_operator_lhs_before_pipe_bind() {
    let source = r#"re"^a$" |>= Regex::is_match("a")"#;
    let rhs_start = source.find("Regex::is_match").expect("rhs call");
    let err = TypeError {
            message: "`|>=` requires the right-hand side to return Result, got Boolean".into(),
            span: Span {
                start: rhs_start,
                end: source.chars().count(),
            },
            hint: Some(
                "`|>=` signature rule: LHS: Result<A, E>; RHS: (A -> Result<B, E>); result: Result<B, E>. LHS: Result<Regex>. RHS: (Regex -> Boolean). Operators share precedence and resolve left-to-right, so LHS is the type produced so far."
                    .into(),
            ),
        };

    let spec = type_error_spec(source, &err);
    let lhs = spec
        .labels
        .iter()
        .find(|label| strip_ansi(&label.message).contains("LHS actual: Result<Regex>"))
        .expect("lhs label");
    let rhs = spec
        .labels
        .iter()
        .find(|label| strip_ansi(&label.message).contains("RHS actual: (Regex -> Boolean)"))
        .expect("rhs label");

    assert_eq!(
        slice_chars(source, lhs.span.start, lhs.span.end),
        r#"re"^a$""#
    );
    assert_eq!(
        slice_chars(source, rhs.span.start, rhs.span.end),
        r#"Regex::is_match("a")"#
    );
}

#[test]
fn type_error_spec_labels_ensure_predicate_call() {
    let source = "guard = ensure(4, is_even(), NoneError)";
    let err = TypeError {
            message: "ensure requires a closure or capture predicate".into(),
            span: Span { start: 18, end: 27 },
            hint: Some(
                "Use `&predicate` or `{|value| predicate(value) }`; call expressions such as `predicate()` are not accepted here."
                    .into(),
            ),
        };

    let spec = type_error_spec(source, &err);

    assert!(spec.labels.iter().any(|label| label
        .message
        .contains("predicate must be a closure or capture")));
    assert!(spec
        .help
        .as_deref()
        .is_some_and(|help| help.contains("Use `&predicate`")));
}

#[test]
fn type_error_spec_labels_ensure_predicate_call_with_string_comma() {
    let source = r#"guard = ensure(4, invalid("a,b"), NoneError)"#;
    let err = TypeError {
        message: "ensure requires a closure or capture predicate".into(),
        span: Span { start: 18, end: 32 },
        hint: None,
    };

    let spec = type_error_spec(source, &err);
    let predicate = spec
        .labels
        .iter()
        .find(|label| {
            label
                .message
                .contains("predicate must be a closure or capture")
        })
        .expect("predicate label");

    assert_eq!(
        slice_chars(source, predicate.span.start, predicate.span.end),
        r#"invalid("a,b")"#
    );
}

#[test]
fn type_error_spec_labels_compose_call_operands_without_unknown_types() {
    let source = "bad = inc(1) >> inc(1)";
    let err = TypeError {
            message: "`>>` requires a function value".into(),
            span: Span { start: 6, end: 12 },
            hint: Some(
                "Call target signature: __Script::fixture::inc(arg1: Int) -> Int\n`>>` evaluates this call before composition; the result type Int is not a function value."
                    .into(),
            ),
        };

    let spec = type_error_spec(source, &err);
    let rendered = strip_ansi(&render_error("main.srt", source, &spec));

    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "LHS signature: __Script::fixture::inc(arg1: Int) -> Int"));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "RHS operand: inc(1)"));
    assert!(!rendered.contains("unknown"));
    assert!(rendered.contains("result type Int is not a function value"));
}

#[test]
fn type_error_spec_labels_trait_impl_signature_mismatch_show_expected_and_actual_types() {
    let source = r#"deftrait Summable {
  def add(self: Self, rhs: Self) -> Self
}

impl Summable for Int {
  def add(self: Self, rhs: String) -> Self {
    self
  }
}"#;
    let string_start = source.find("String").expect("impl type");
    let err = TypeError {
            message:
                "Trait impl method Summable::add has incompatible parameter type: expected Int, got String"
                    .into(),
            span: Span {
                start: string_start,
                end: string_start + "String".len(),
            },
            hint: None,
        };

    let spec = type_error_spec(source, &err);
    let expected = spec
        .labels
        .iter()
        .find(|label| label.message == "expected parameter type: Int")
        .expect("expected label");
    let actual = spec
        .labels
        .iter()
        .find(|label| label.message == "actual parameter type: String")
        .expect("actual label");

    assert_eq!(
        slice_chars(source, expected.span.start, expected.span.end),
        "def add(self: Self, rhs: Self) -> Self"
    );
    assert_eq!(
        slice_chars(source, actual.span.start, actual.span.end),
        "String"
    );
}

#[test]
fn collect_match_arm_body_spans_ignores_literals() {
    let source = r#"match value { Left("=>") => "a,b", Right(x) => x }"#;
    let chars: Vec<char> = source.chars().collect();
    let (_, open_brace, close_brace) =
        find_enclosing_match_block(&chars, source.find("Right").expect("focus"))
            .expect("match block");
    let spans = collect_match_arm_body_spans(&chars, open_brace + 1, close_brace);

    assert_eq!(spans.len(), 2);
    assert_eq!(slice_chars(source, spans[0].0, spans[0].1), r#""a,b""#);
    assert_eq!(slice_chars(source, spans[1].0, spans[1].1), "x");
}

#[test]
fn extract_match_pattern_span_ignores_literal_arrow() {
    let source = r#"Capture("=>") => value"#;
    let chars: Vec<char> = source.chars().collect();
    let span = extract_match_pattern_span(&chars).expect("pattern span");

    assert_eq!(slice_chars(source, span.0, span.1), r#"Capture("=>")"#);
}

#[test]
fn type_error_spec_uses_callable_definition_signature_hint() {
    let source = r#"def add(x: Int, y: Int) -> Int {
  x + y
}
bad = &add(&1, "oops")"#;
    let err = TypeError {
            message: "Argument type mismatch: expected Int, got String".into(),
            span: Span { start: 60, end: 66 },
            hint: Some(
                "Callable definition signature: __Script::fixture::add(x: Int, y: Int) -> Int\nCallable definition span: 0..32"
                    .into(),
            ),
        };

    let spec = type_error_spec(source, &err);
    assert!(spec.help.is_none());
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "__Script::fixture::add(x: Int, y: Int) -> Int"));
    let def_label = spec
        .labels
        .iter()
        .find(|label| label.message == "__Script::fixture::add(x: Int, y: Int) -> Int")
        .expect("definition label");
    assert_eq!(def_label.span.start, 0);

    let rendered = strip_ansi(&render_error("main.srt", source, &spec));
    assert_eq!(
        rendered.matches("def add(x: Int, y: Int) -> Int {").count(),
        1
    );
    assert!(rendered.contains("__Script::fixture::add(x: Int, y: Int) -> Int"));
}

#[test]
fn serializable_report_preserves_callable_definition_signature_hint() {
    let mut sources = SourceRegistry::new();
    let source = r#"def add(x: Int, y: Int) -> Int {
  x + y
}
bad = add("oops", 1)"#;
    let source_id = sources.register("main.srt", source);
    let err = TypeError {
            message: "Argument type mismatch: expected Int, got String".into(),
            span: Span { start: 52, end: 58 },
            hint: Some(
                "Callable definition signature: __Script::fixture::add(x: Int, y: Int) -> Int\nCallable definition span: 0..32"
                    .into(),
            ),
        };

    let spec = type_error_spec(source, &err);
    let report = serializable_report_by_id(&sources, source_id, "typecheck", &spec);
    let hint = report.errors[0]
        .hint
        .as_deref()
        .expect("serialized callable signature hint");

    assert_eq!(
        hint,
        "Callable definition signature: __Script::fixture::add(x: Int, y: Int) -> Int"
    );
}

#[test]
fn source_signature_caption_handles_defmod_and_impls() {
    let module_source = r#"defmod Math {
  def add(x: Int, y: Int) -> Int {
    x + y
  }
}"#;
    let module_lines = line_spans(module_source);
    let module_sig =
        find_function_signature_line(module_source, &module_lines, "add").expect("module def line");
    assert_eq!(
        source_signature_caption(module_source, &module_lines, module_sig, "add").as_deref(),
        Some("Math::add(x: Int, y: Int) -> Int")
    );

    let impl_source = r#"impl User {
  def normalize(self: Self) -> Self {
    self
  }
}"#;
    let impl_lines = line_spans(impl_source);
    let impl_sig =
        find_function_signature_line(impl_source, &impl_lines, "normalize").expect("impl def line");
    assert_eq!(
        source_signature_caption(impl_source, &impl_lines, impl_sig, "normalize").as_deref(),
        Some("User::normalize(self: Self) -> Self")
    );

    let trait_impl_source = r#"impl From<String> for Int {
  def from(self: Self, to: TypeRef<String>) -> String {
    inspect(self)
  }
}"#;
    let trait_impl_lines = line_spans(trait_impl_source);
    let trait_impl_sig = find_function_signature_line(trait_impl_source, &trait_impl_lines, "from")
        .expect("trait impl def line");
    assert_eq!(
        source_signature_caption(trait_impl_source, &trait_impl_lines, trait_impl_sig, "from")
            .as_deref(),
        Some("impl From<String> for Int { def from(self: Self, to: TypeRef<String>) -> String }")
    );
}

#[test]
fn parse_error_spec_adds_unexpected_token_help() {
    let spec = parse_error_spec(
        "x = )",
        "Unexpected token: RParen",
        Span { start: 4, end: 5 },
    );

    assert!(spec
        .labels
        .iter()
        .any(|label| label.message.contains("unexpected closing parenthesis")));
    assert!(spec
        .help
        .as_deref()
        .is_some_and(|help| help.contains("parser stopped")));
}

#[test]
fn parse_error_spec_rewrites_identity_anonymous_capture() {
    let source = "f = &(&1)";
    let spec = parse_error_spec(
        source,
        "anonymous capture is not supported; use `&id` instead",
        Span { start: 4, end: 9 },
    );

    assert_eq!(
        spec.help.as_deref(),
        Some("Replace this anonymous capture with:\n\n  f = &id")
    );
}

#[test]
fn parse_error_spec_rewrites_anonymous_capture_to_named_helper_shape() {
    let source = "f = &(&1 + &2)";
    let spec = parse_error_spec(
            source,
            "anonymous capture is not supported; extract a named function and capture it like `&fun_name(&1, &2)`",
            Span { start: 4, end: 14 },
        );

    assert_eq!(
            spec.help.as_deref(),
            Some(
                "Extract the body into a named helper and replace this capture with:\n\n  f = &fun_name(&1, &2)"
            )
        );
}

#[test]
fn type_error_spec_labels_extractor_pattern_for_safebind_rhs() {
    let source = "uncons(head, tail) =? True";
    let err = TypeError {
        message: "Extractor uncons expects List<...> or String, got Boolean".into(),
        span: Span { start: 0, end: 6 },
        hint: None,
    };

    let spec = type_error_spec(source, &err);

    assert!(spec.labels.iter().any(|label| {
        label.message == "extractor pattern checked against the SafeBind RHS"
            && slice_chars(source, label.span.start, label.span.end) == "uncons(head, tail)"
    }));
}

#[test]
fn type_error_spec_splits_total_bind_pattern_error_into_lhs_op_rhs() {
    let source = "[h, ..t] = [1]";
    let err = TypeError {
        message: "Only total MatchBlock patterns can be used with `=`".into(),
        span: Span {
            start: 0,
            end: source.chars().count(),
        },
        hint: Some("Use `=?` for partial destructuring and extractor-driven matches.".into()),
    };

    let spec = type_error_spec(source, &err);

    assert!(spec.labels.iter().any(|label| {
        label.message == "LHS pattern: partial MatchBlock pattern"
            && label.color == Some(Color::Red)
            && slice_chars(source, label.span.start, label.span.end) == "[h, ..t]"
    }));
    assert!(spec.labels.iter().any(|label| {
        label.message == "Use `=?` for partial destructuring and extractor-driven matches."
            && label.color == Some(Color::Yellow)
            && slice_chars(source, label.span.start, label.span.end) == "="
    }));
    assert!(spec.labels.iter().any(|label| {
        label.message == "RHS value"
            && label.color.is_none()
            && slice_chars(source, label.span.start, label.span.end) == "[1]"
    }));
    assert!(spec.help.is_none());
}

#[test]
fn type_error_spec_by_id_adds_extractor_context_blocks() {
    let mut sources = SourceRegistry::new();
    let main_source = "print(match True {\n  uncons(head, tail) => head,\n  _ => 0,\n})";
    let main_id = sources.register("main.srt", main_source);
    let kernel_id = sources.register(
        "lib/kernel.srt",
        "@@builtin defextractor uncons(term) -> MatchResult<($Head, $Tail), Error>",
    );
    let err = TypeError {
        message: "Extractor uncons expects List<...> or String, got Boolean".into(),
        span: Span { start: 22, end: 28 },
        hint: None,
    };

    let spec = type_error_spec_by_id(&sources, main_id, &err);

    assert!(spec
        .labels
        .iter()
        .any(|label| label.source_id == Some(main_id) && label.message == "input source: Boolean"));
    assert!(spec.labels.iter().any(|label| {
        label.source_id == Some(kernel_id)
            && label
                .message
                .contains("Extractor definition: @@builtin defextractor uncons(term)")
    }));
}

#[test]
fn safebind_terminal_rhs_span_picks_last_pipeline_rhs() {
    let source = "uncons(head, tail) =? seed\n  |> step1()\n  |> finalize()";
    let lines = line_spans(source);
    let assignment = find_safebind_assignment(&lines, 0, source).expect("safebind assignment");
    let span = safebind_terminal_rhs_span(source, &lines, assignment).expect("terminal rhs span");

    assert_eq!(slice_chars(source, span.start, span.end), "finalize()");
}

#[test]
fn parse_error_spec_labels_source_policy_violation() {
    let source = "defstruct User {\n  name: String,\n}";
    let spec = parse_error_spec(
        source,
        "This top-level declaration is not allowed in the current source policy",
        Span { start: 0, end: 17 },
    );

    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "forbidden top-level declaration"));
    assert_eq!(
            spec.help.as_deref(),
            Some(
                "Move this declaration into a module compile unit, or replace it with an expression that is allowed in this source kind."
            )
        );
}

#[test]
fn parse_error_spec_labels_return_position_impl_trait() {
    let source = "def echo(x: impl Numeric) -> impl Numeric { x }";
    let spec = parse_error_spec(
        source,
        "return-position `impl Trait` is not supported; name the type parameter explicitly",
        Span { start: 29, end: 41 },
    );

    assert!(spec.labels.iter().any(|label| {
        label.message == "return-position `impl Trait` is not supported"
            && label.color == Some(Color::Red)
    }));
    assert_eq!(
        spec.help.as_deref(),
        Some("Name the return type parameter explicitly in the function signature.")
    );
}

#[test]
fn parse_error_spec_labels_where_clause_staging() {
    let source = "def double<$N>(x: $N) -> $N where $N: Numeric { x + x }";
    let spec = parse_error_spec(
        source,
        "`where` clauses are staged and not implemented yet",
        Span { start: 29, end: 46 },
    );

    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "`where` clauses are not available yet"));
    assert!(spec
        .help
        .as_deref()
        .is_some_and(|help| help.contains("explicit type parameters")));
}

#[test]
fn resolve_error_spec_labels_undefined_name() {
    let spec = resolve_error_spec(
        "unknown(1)",
        "Undefined variable: unknown",
        Span { start: 0, end: 7 },
    );

    assert!(spec
        .labels
        .iter()
        .any(|label| label.message.contains("unresolved name `unknown`")));
    assert!(spec
        .help
        .as_deref()
        .is_some_and(|help| help.contains("not defined in the current scope")));
}

#[test]
fn resolve_error_spec_labels_undefined_callable() {
    let spec = resolve_error_spec(
        "unknown(1)",
        "Undefined function unknown/1",
        Span { start: 0, end: 7 },
    );

    assert!(spec
        .labels
        .iter()
        .any(|label| label.message.contains("unresolved call target `unknown/1`")));
    assert!(spec
        .help
        .as_deref()
        .is_some_and(|help| help.contains("Check the argument count")));
}

#[test]
fn resolve_error_spec_labels_unknown_module_import() {
    let spec = resolve_error_spec(
        "import Missing",
        "Unknown module import: Missing",
        Span { start: 0, end: 14 },
    );

    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "unknown import target `Missing`"));
    assert!(spec
        .help
        .as_deref()
        .is_some_and(|help| help.contains("loaded before this import")));
}

#[test]
fn resolve_error_spec_labels_non_importable_target() {
    let spec = resolve_error_spec(
        "import User",
        "Import target `User` is not importable",
        Span { start: 0, end: 11 },
    );

    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "import target `User` is not importable"));
    assert!(spec
        .help
        .as_deref()
        .is_some_and(|help| help.contains("cannot be imported directly")));
}

#[test]
fn resolve_error_spec_rewrites_nested_pipe_slot_into_previous_pipe_step() {
    let source = "value |> f(add(10, _1))";
    let spec = resolve_error_spec(
        source,
        "pipe placeholder `_1` cannot be used as an expression",
        Span { start: 20, end: 22 },
    );

    assert_eq!(
            spec.help.as_deref(),
            Some(
                "Move the `_1` transformation into the previous pipe step:\n\n  value\n  |> add(10, _1)\n  |> f()"
            )
        );
}

#[test]
fn resolve_error_spec_recursively_rewrites_nested_pipe_slot_up_to_depth_three() {
    let source = "value |> f(g(add(10, _1)))";
    let spec = resolve_error_spec(
        source,
        "pipe placeholder `_1` cannot be used as an expression",
        Span { start: 22, end: 24 },
    );

    assert_eq!(
            spec.help.as_deref(),
            Some(
                "Move the `_1` transformation into the previous pipe step:\n\n  value\n  |> add(10, _1)\n  |> g()\n  |> f()"
            )
        );
}

#[test]
fn resolve_error_spec_falls_back_to_closure_for_deeper_nested_pipe_slot() {
    let source = "value |> f(g(h(add(10, _1))))";
    let spec = resolve_error_spec(
        source,
        "pipe placeholder `_1` cannot be used as an expression",
        Span { start: 24, end: 26 },
    );

    assert_eq!(
            spec.help.as_deref(),
            Some(
                "Move the `_1` transformation into the previous pipe step:\n\n  value\n  |> {|term| f(g(h(add(10, term))))}"
            )
        );
}

#[test]
fn resolve_error_spec_rewrites_nested_context_map_slot_into_previous_pipe_step() {
    let source = "value |*> f(add(10, _1))";
    let spec = resolve_error_spec(
        source,
        "pipe placeholder `_1` cannot be used as an expression",
        Span { start: 21, end: 23 },
    );

    assert_eq!(
            spec.help.as_deref(),
            Some(
                "Move the `_1` transformation into the previous pipe step:\n\n  value\n  |*> add(10, _1)\n  |*> f()"
            )
        );
}

#[test]
fn resolve_error_spec_preserves_pipe_slot_position_when_rewriting_nested_calls() {
    let source = "value |> f(1, g(2, add(10, _1)))";
    let spec = resolve_error_spec(
        source,
        "pipe placeholder `_1` cannot be used as an expression",
        Span { start: 28, end: 30 },
    );

    assert_eq!(
            spec.help.as_deref(),
            Some(
                "Move the `_1` transformation into the previous pipe step:\n\n  value\n  |> add(10, _1)\n  |> g(2, _1)\n  |> f(1, _1)"
            )
        );
}

#[test]
fn resolve_error_spec_rewrites_nested_context_bind_slot_into_previous_pipe_step() {
    let source = "value |>= f(add(10, _1))";
    let spec = resolve_error_spec(
        source,
        "pipe placeholder `_1` cannot be used as an expression",
        Span { start: 21, end: 23 },
    );

    assert_eq!(
            spec.help.as_deref(),
            Some(
                "Move the `_1` transformation into the previous pipe step:\n\n  value\n  |>= add(10, _1)\n  |>= f()"
            )
        );
}

#[test]
fn resolve_error_spec_uses_closure_fallback_for_deep_context_bind_rewrite() {
    let source = "value |>= f(g(h(add(10, _1))))";
    let spec = resolve_error_spec(
        source,
        "pipe placeholder `_1` cannot be used as an expression",
        Span { start: 25, end: 27 },
    );

    assert_eq!(
            spec.help.as_deref(),
            Some(
                "Move the `_1` transformation into the previous pipe step:\n\n  value\n  |>= {|term| f(g(h(add(10, term))))}"
            )
        );
}

#[test]
fn runtime_value_error_spec_splits_literal_safebind_pattern() {
    let source = r#""2" =? "1""#;
    let spec = runtime_value_error_spec(
        source,
        "PatternMismatch",
        "Pattern did not match.\t@@lhs=\"2\"\t@@rhs=\"1\"",
        0,
        1,
        None,
    );

    assert_eq!(
        slice_chars(source, spec.primary_span.start, spec.primary_span.end),
        r#""2""#
    );
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "LHS value: \"2\""));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "SafeBind partial match"));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "RHS value: \"1\""));
    assert_eq!(spec.message, "Pattern did not match.");
}

#[test]
fn runtime_value_error_spec_splits_head_tail_string_safebind_pattern() {
    let source = r#"[h, ..t] =? """#;
    let spec = runtime_value_error_spec(
        source,
        "PatternMismatch",
        "Pattern did not match.",
        0,
        1,
        None,
    );

    assert!(spec
        .labels
        .iter()
        .any(|label| { label.message == "head-tail list pattern requires a non-empty String" }));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "SafeBind partial match"));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "input source: String"));
}

#[test]
fn runtime_error_spec_splits_builtin_runtime_error() {
    let spec = runtime_error_spec(
        r#"len("oops")"#,
        "len expects List as first argument",
        Span { start: 0, end: 11 },
        &RuntimeDiagnosticContext::default(),
        None,
    );
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "call target"));
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "expected rule: List as first argument"));
}

#[test]
fn runtime_error_spec_splits_builtin_out_of_range_rule() {
    let spec = runtime_error_spec(
        "set_exit_code(999999999999999999999999999999)",
        "set_exit_code out of range for i32: 999999999999999999999999999999",
        Span { start: 0, end: 45 },
        &RuntimeDiagnosticContext::default(),
        None,
    );
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "expected rule: value must fit in i32"));
}

#[test]
fn runtime_error_spec_splits_vm_runtime_error() {
    let spec = runtime_error_spec(
        "bad_jump",
        "JumpIfFalse: expected Bool",
        Span { start: 0, end: 8 },
        &RuntimeDiagnosticContext {
            opcode: Some("JumpIfFalse".into()),
            function: Some("fun#1".into()),
            details: Vec::new(),
        },
        None,
    );
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "opcode: JumpIfFalse"));
    assert!(spec.labels.iter().any(|label| {
        label
            .message
            .starts_with("runtime rule: JumpIfFalse requires Bool")
    }));
}
