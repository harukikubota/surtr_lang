#[test]
fn type_error_spec_labels_backtick_operator_operands() {
    let source = "bad = 1 `+` \"oops\"";
    let err = TypeError {
            message: "`+` requires the same type on both sides, but got Int and String".into(),
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
        .any(|label| strip_ansi(&label.message) == "   OP rule: A + A -> A"));
    assert!(spec
        .labels
        .iter()
        .any(|label| strip_ansi(&label.message) == "RHS actual: String"));
    assert!(notes_text.contains("Step: Int + String -> <type error>"));
    assert!(notes_text
        .contains("Reason: `+` requires the same type on both sides, but got Int and String."));
    assert!(spec
        .help
        .as_deref()
        .is_some_and(|help| help.contains("same type on both sides")));
}

#[test]
fn type_error_spec_picks_symbol_operator_outside_literals() {
    let source = r#""+" + "value""#;
    let rhs_start = source.rfind("\"value\"").expect("rhs literal");
    let err = TypeError {
            message: "`+` is not defined for String".into(),
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
        .find(|label| strip_ansi(&label.message) == "   OP rule: A + A -> A")
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
        message: "`==` requires the same type on both sides, but got Int and Boolean".into(),
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
        message: "`!=` requires the same type on both sides, but got Int and Boolean".into(),
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
        message: "`<` requires the same type on both sides, but got Int and Boolean".into(),
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
        .any(|label| strip_ansi(&label.message) == "   OP rule: A < A -> Boolean"));
    assert!(notes_text.contains("Step: Int < Boolean -> Boolean"));
    assert!(notes_text.contains(
        "Reason: `<` compares two ordered values of the same type, but got Int and Boolean."
    ));
}

#[test]
fn type_error_spec_distinguishes_same_type_but_undefined_open_operator() {
    let source = "bad = False + True";
    let err = TypeError {
        message: "`+` is not defined for Boolean".into(),
        span: Span { start: 6, end: 18 },
        hint: Some("Add is implemented for: Duration, Float, Int.".into()),
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
        .any(|label| strip_ansi(&label.message) == "   OP rule: A + A -> A"));
    assert!(notes_text.contains("Reason: `+` is not defined for Boolean."));
    assert!(spec
        .help
        .as_deref()
        .is_some_and(|help| help.contains("Add is implemented for: Duration, Float, Int.")));
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
fn type_error_spec_canonicalizes_eq_helper_as_operator_surface() {
    let source = "print(to_string(eq(1, True)))";
    let true_start = source.find("True").expect("rhs arg");
    let err = TypeError {
        message: "Eq::eq helper cannot compare Int and Boolean".into(),
        span: Span {
            start: true_start,
            end: true_start + "True".chars().count(),
        },
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
    let op = spec
        .labels
        .iter()
        .find(|label| strip_ansi(&label.message) == "   OP rule: A == A -> Boolean")
        .expect("operator label");
    assert_eq!(slice_chars(source, op.span.start, op.span.end), "eq");
}

#[test]
fn type_error_spec_colors_generic_binary_operator_note_only() {
    let source = "bad = 1 `*` \"oops\"";
    let err = TypeError {
        message: "`*` requires the same type on both sides, but got Int and String".into(),
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
            message: "`|>=` requires Chainable implementation on the left, got Int".into(),
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
    assert!(rendered_plain.contains(
        "Reason: LHS is Int, but `|>=` requires a Chainable such as Result<A>, List<A>, or Option<A>."
    ));
    assert_eq!(
        spec.help.as_deref(),
        Some("Use `|>` for a plain value, or make the LHS Result/List/Option.")
    );
    assert!(!spec.help.as_deref().unwrap_or("").contains("Int"));
}

#[test]
fn binary_operator_error_preserves_trait_implementation_hint_in_help() {
    let source = "bad = False + True";
    let err = TypeError {
        message: "`+` is not defined for Boolean".into(),
        span: Span { start: 6, end: 18 },
        hint: Some("Add is implemented for: Duration, Float, Int.".into()),
    };

    let spec = type_error_spec(source, &err);
    assert!(spec
        .help
        .as_deref()
        .is_some_and(|help| help.contains("Add is implemented for: Duration, Float, Int.")));
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

use super::test_support::*;

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
