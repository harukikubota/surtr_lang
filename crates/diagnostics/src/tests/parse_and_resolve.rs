use super::test_support::*;

#[test]
fn parse_error_spec_adds_unexpected_token_help() {
    let spec = parser_error_spec("x = )", None);

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
fn do_carrier_parser_errors_keep_typed_reasons_and_rewrite_help() {
    let arity = parser_error_spec("do::<Option, List> { finish() }", None);
    assert_eq!(
        arity
            .structured
            .as_ref()
            .map(|structured| structured.reason),
        Some(DiagnosticReason::Parse(
            ParseDiagnosticReason::ReturnTypeArgumentArityMismatch
        ))
    );

    for source in ["do<Option> { finish() }", "do::<$F> { finish() }"] {
        let spec = parser_error_spec(source, None);
        assert_eq!(
            spec.structured.as_ref().map(|structured| structured.reason),
            Some(DiagnosticReason::Parse(
                ParseDiagnosticReason::InvalidDoCarrierReturnTypeArgument
            )),
            "{source}"
        );
        let help = spec.help.as_deref().unwrap_or_default();
        if source.starts_with("do<") {
            assert!(help.contains("do::<Option>"), "{source}: {spec:?}");
            assert_eq!(
                &source[spec.primary_span.start..spec.primary_span.end],
                "<Option>"
            );
        } else {
            assert!(help.contains("do::<Either>"), "{source}: {spec:?}");
            assert!(spec
                .labels
                .iter()
                .any(|label| label.message.contains("outer constructor variable")));
            assert!(spec
                .notes
                .iter()
                .any(|note| note.contains("captured and fixed arguments")));
        }
    }

    let utf8_source = "prefix = \"日本語\"\ndo<Option> { finish() }";
    let utf8 = parser_error_spec(utf8_source, None);
    assert_eq!(
        slice_chars(utf8_source, utf8.primary_span.start, utf8.primary_span.end),
        "<Option>"
    );
    assert!(utf8
        .help
        .as_deref()
        .is_some_and(|help| help.contains("do::<Option>")));
}

#[test]
fn resolve_error_spec_projects_explicit_reason_and_related_source_facts() {
    let primary = Span { start: 12, end: 20 };
    let related_span = Span { start: 0, end: 8 };
    let spec = resolve_error_spec(
        SourceId(3),
        "Undefined variable: stale display text",
        primary.clone(),
        ResolveDiagnosticReason::Namespace,
        Some("Global::Module".into()),
        &[(
            SourceId(2),
            related_span.clone(),
            "first declaration".into(),
        )],
    );

    let structured = spec.structured.expect("resolver diagnostic is structured");
    assert_eq!(
        structured.reason,
        DiagnosticReason::Resolve(ResolveDiagnosticReason::Namespace)
    );
    assert_eq!(structured.origin, DiagnosticOrigin::Resolve);
    assert_eq!(structured.primary.source_id, SourceId(3));
    assert_eq!(structured.primary.span, primary);
    assert_eq!(structured.related[0].source_id, SourceId(2));
    assert_eq!(structured.related[0].span, related_span);
    let DiagnosticData::Resolve(data) = structured.data else {
        panic!("expected resolver payload");
    };
    assert_eq!(data.subject.as_deref(), Some("Global::Module"));
}

#[test]
fn resolve_error_spec_with_labels_cycles_duplicate_binding_colors() {
    let labels = [
        (SourceId(0), Span { start: 0, end: 1 }, "first".to_string()),
        (SourceId(0), Span { start: 2, end: 3 }, "second".to_string()),
        (SourceId(0), Span { start: 4, end: 5 }, "third".to_string()),
        (SourceId(0), Span { start: 6, end: 7 }, "fourth".to_string()),
        (SourceId(0), Span { start: 8, end: 9 }, "fifth".to_string()),
        (
            SourceId(0),
            Span { start: 10, end: 11 },
            "first".to_string(),
        ),
    ];
    let spec = resolve_error_spec(
        SourceId(0),
        "Duplicate binding in pattern: x",
        Span { start: 0, end: 1 },
        ResolveDiagnosticReason::Pattern,
        None,
        &labels,
    );

    assert_eq!(
        spec.labels
            .iter()
            .map(|label| label.color)
            .collect::<Vec<_>>(),
        vec![
            Some(Color::Red),
            Some(Color::Yellow),
            Some(Color::Blue),
            Some(Color::Magenta),
            Some(Color::Cyan),
            Some(Color::Red),
        ]
    );
}

#[test]
fn duplicate_top_level_owner_diagnostic_preserves_each_contract_role() {
    let first_span = Span { start: 10, end: 14 };
    let conflicting_span = Span { start: 30, end: 34 };
    let spec = resolve_error_spec(
        SourceId(0),
        "Duplicate top-level owner: Hoge",
        conflicting_span.clone(),
        ResolveDiagnosticReason::Namespace,
        Some("Hoge".into()),
        &[
            (
                SourceId(0),
                first_span.clone(),
                "first Record declaration".into(),
            ),
            (
                SourceId(0),
                conflicting_span.clone(),
                "conflicting Mod declaration".into(),
            ),
        ],
    );

    assert_eq!(spec.message, "Duplicate top-level owner: Hoge");
    assert_eq!(spec.primary_span, conflicting_span);
    assert!(spec
        .labels
        .iter()
        .any(|label| { label.span == first_span && label.message == "first Record declaration" }));
    assert!(spec.labels.iter().any(|label| {
        label.span == spec.primary_span && label.message == "conflicting Mod declaration"
    }));
    let structured = spec.structured.expect("resolver diagnostic is structured");
    assert_eq!(
        structured.reason,
        DiagnosticReason::Resolve(ResolveDiagnosticReason::Namespace)
    );
    assert_eq!(structured.related.len(), 2);
}

#[test]
fn parse_error_spec_uses_help_for_unit_pattern_guidance() {
    let source = "() = ()";
    let spec = parser_error_spec(source, None);

    assert_eq!(
        spec.help.as_deref(),
        Some("Variable bindings and the `_` wildcard pattern are allowed.")
    );
    assert!(!labels_text(&spec).contains("Help:"));
}

#[test]
fn parse_error_spec_guides_wildcard_as_pattern_aliases() {
    let source = "(left, right) @ _ = (1, 2)";
    let spec = parser_error_spec(source, None);

    assert_eq!(
        spec.help.as_deref(),
        Some("Replace the wildcard alias with a name, for example `pattern @ value`.")
    );
}

#[test]
fn parse_error_spec_rewrites_identity_anonymous_capture() {
    let source = "f = &(&1)";
    let spec = parser_error_spec(source, None);

    assert_eq!(
        spec.help.as_deref(),
        Some("Replace this anonymous capture with:\n\n  f = &id")
    );
}

#[test]
fn parse_error_spec_rewrites_anonymous_capture_to_named_helper_shape() {
    let source = "f = &(&1 + &2)";
    let spec = parser_error_spec(source, None);

    assert_eq!(
        spec.help.as_deref(),
        Some(
            "Extract the body into a named helper and replace this capture with:\n\n  f = &fun_name(&1, &2)"
        )
    );
}

#[test]
fn parse_error_spec_explains_immediate_anonymous_callable_calls() {
    let source = "f = &add(&1, 10)(4)";
    let spec = parser_error_spec(source, None);

    assert!(spec.labels.iter().any(|label| {
        label.message == "anonymous callable is followed by an immediate call"
            && label.color == Some(Color::Red)
    }));
    assert_eq!(
        spec.help.as_deref(),
        Some(
            "Bind the callable to a name before calling it. For example:\n\n  f = &add(&1, 10)\n  f(4)\n\n  f = {|x| x + 1}\n  f(4)\n\n  tmp = make()\n  tmp(4)"
        )
    );
}

#[test]
fn parse_error_spec_puts_range_literal_rewrite_in_help() {
    let spec = parser_error_spec("2..8", None);

    assert_eq!(spec.message, "Range literals must use bracket syntax");
    assert_eq!(spec.help.as_deref(), Some("Write `[start..stop]`."));
    assert!(!spec.message.contains("[start..stop]"));
}

#[test]
fn parse_error_spec_puts_bare_operator_capture_rewrite_in_help() {
    let source = "List::reduce([1, 2], 0, &+)";
    let spec = parser_error_spec(source, None);

    assert_eq!(spec.message, "Unquoted operator capture: +");
    assert_eq!(spec.help.as_deref(), Some("Write &`+`."));
    assert!(!spec.message.contains("Write &`+`."));
}

#[test]
fn parse_error_spec_puts_bare_pair_constructor_rewrite_in_help() {
    let source = "pair = &(,)";
    let spec = parser_error_spec(source, None);

    assert_eq!(spec.message, "bare `(,)` is only valid in infix position");
    assert_eq!(
        spec.help.as_deref(),
        Some("Write &`(,)` for a capture, or use `(,)`(right) as a pipeline RHS.")
    );
    assert!(!spec.message.contains("Write &`(,)`"));
}

#[test]
fn parse_error_spec_labels_source_policy_violation() {
    let source = "defstruct User {\n  name: String,\n}";
    let spec = parser_error_spec(source, Some(spire::ParserContext::repl(0)));

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
    let source = "def echo(x: String) -> impl Show { x }";
    let spec = parser_error_spec(source, None);

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
    let source = "defextractor copy(value: Int) -> Int where $T: Show";
    let spec = parser_error_spec(source, None);

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
fn parse_error_spec_guides_missing_process_state_with_concrete_meta_entry() {
    let source = "defgenserver Ticker {\n  meta {\n    instance: Singleton\n  }\n}";
    let spec = parser_error_spec(source, None);

    assert_eq!(
        spec.help.as_deref(),
        Some("Add a state declaration inside `meta { ... }`. For example:\n\n  state: Int")
    );
    assert!(spec
        .labels
        .iter()
        .any(|label| label.message == "process declaration"));
}
