use super::test_support::*;

#[test]
fn repl_query_parse_error_spec_renders_precise_query_span() {
    let spec = repl_query_parse_error_spec(
        "gt(Int, )",
        "Invalid typed call query: empty argument.",
        Span { start: 8, end: 8 },
    );

    assert_eq!(spec.kind, "ReplQueryParseError");
    assert_eq!(spec.primary_span, Span { start: 8, end: 8 });
    assert_eq!(
        spec.help.as_deref(),
        Some("Provide an argument after the comma, or remove the trailing comma.")
    );

    let rendered = strip_ansi(&render_error("repl", "gt(Int, )", &spec));
    assert!(rendered.contains("ReplQueryParseError: Invalid typed call query: empty argument."));
    assert!(rendered.contains("query argument expected here"));
}

#[test]
fn repl_command_parse_error_spec_suggests_help_for_unknown_commands() {
    let spec = repl_command_parse_error_spec(
        ":wat",
        "Unknown REPL command `:wat`.",
        Span { start: 0, end: 4 },
    );

    assert_eq!(spec.kind, "ReplCommandError");
    assert_eq!(
        spec.help.as_deref(),
        Some("Type `:help` for the list of available REPL commands.")
    );

    let rendered = strip_ansi(&render_error("repl", ":wat", &spec));
    assert!(rendered.contains("ReplCommandError: Unknown REPL command `:wat`."));
    assert!(rendered.contains("unknown REPL command"));
}
