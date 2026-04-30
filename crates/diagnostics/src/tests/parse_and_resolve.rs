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

