use super::test_support::*;

#[test]
fn repl_query_parse_error_spec_renders_precise_query_span() {
    let spec = repl_query_parse_error_spec(
        "compare(Int, )",
        "Unsupported command query form.",
        Span { start: 7, end: 14 },
        ReplDiagnosticReason::QueryUnsupported,
    );

    assert_eq!(spec.kind, "ReplQueryParseError");
    assert_eq!(spec.primary_span, Span { start: 7, end: 14 });
    assert_eq!(
        spec.help.as_deref(),
        Some("Use a name, qualified name, fixed symbol, Facet root, or field path without arguments.")
    );

    let rendered = strip_ansi(&render_error("repl", "compare(Int, )", &spec));
    assert!(rendered.contains("ReplQueryParseError: Unsupported command query form."));
    assert!(rendered.contains("query parse error"));
}

#[test]
fn repl_command_parse_error_spec_suggests_help_for_unknown_commands() {
    let spec = repl_command_parse_error_spec(
        ":wat",
        "Unknown REPL command `:wat`.",
        Span { start: 0, end: 4 },
        ReplDiagnosticReason::CommandUnknown,
    );

    assert_eq!(spec.kind, "ReplCommandError");
    assert_eq!(
        spec.help.as_deref(),
        Some("Type `:help` for the list of available REPL commands.")
    );

    let rendered = strip_ansi(&render_error("repl", ":wat", &spec));
    assert!(rendered.contains("ReplCommandError: Unknown REPL command `:wat`."));
    assert!(rendered.contains("unknown REPL command"));

    let structured = spec.structured.expect("REPL diagnostic must be structured");
    assert_eq!(structured.reason.as_str(), "CommandUnknown");
}
