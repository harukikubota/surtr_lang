use crate::heuristics::trimmed_line_span_containing;
use crate::{simple_error, Color, DiagnosticLabel, DiagnosticSpec};
use spire::ast::Span;

pub fn repl_query_parse_error_spec(
    source: &str,
    message: impl Into<String>,
    span: Span,
) -> DiagnosticSpec {
    let message = message.into();
    let mut spec = simple_error(
        "ReplQueryParseError",
        message.clone(),
        span.clone(),
        repl_query_help(&message),
    );

    spec.labels.push(DiagnosticLabel {
        source_id: None,
        span: repl_focus_span(source, &span),
        message: repl_query_label(&message).into(),
        color: Some(Color::Red),
    });

    spec
}

pub fn repl_command_parse_error_spec(
    source: &str,
    message: impl Into<String>,
    span: Span,
) -> DiagnosticSpec {
    let message = message.into();
    let mut spec = simple_error(
        "ReplCommandError",
        message.clone(),
        span.clone(),
        repl_command_help(&message),
    );

    spec.labels.push(DiagnosticLabel {
        source_id: None,
        span: repl_focus_span(source, &span),
        message: repl_command_label(&message).into(),
        color: Some(Color::Red),
    });

    spec
}

fn repl_focus_span(source: &str, span: &Span) -> Span {
    if span.start == span.end {
        span.clone()
    } else {
        trimmed_line_span_containing(source, span.start).unwrap_or_else(|| span.clone())
    }
}

fn repl_query_label(message: &str) -> &'static str {
    if message == "REPL query cannot be empty." {
        "query expected here"
    } else if message == "Invalid typed call query: empty argument." {
        "query argument expected here"
    } else if message == "Invalid typed call query: missing closing `)`." {
        "missing closing `)` for this typed call"
    } else if message == "Invalid typed query: `_ :` requires a type." {
        "annotated hole is missing its type"
    } else if message.starts_with("Invalid operator query:") {
        "operator query is missing an operand"
    } else {
        "query parse error"
    }
}

fn repl_query_help(message: &str) -> Option<String> {
    if message == "REPL query cannot be empty." {
        Some("Provide a symbol, typed call, typed operator, or expression query.".into())
    } else if message == "Invalid typed call query: empty argument." {
        Some("Provide an argument after the comma, or remove the trailing comma.".into())
    } else if message == "Invalid typed call query: missing closing `)`." {
        Some("Close the typed call with `)` after the final argument.".into())
    } else if message == "Invalid typed query: `_ :` requires a type." {
        Some("Annotated holes must be written as `_ : Type`.".into())
    } else if message.starts_with("Invalid operator query:") {
        Some("Write operator queries as `<lhs> <operator> <rhs>`.".into())
    } else {
        None
    }
}

fn repl_command_label(message: &str) -> &'static str {
    if message.contains("Unknown REPL command") {
        "unknown REPL command"
    } else if message.contains("Invalid :error mode") {
        "unsupported :error mode"
    } else {
        "command parse error"
    }
}

fn repl_command_help(message: &str) -> Option<String> {
    if message.contains("Unknown REPL command") {
        Some("Type `:help` for the list of available REPL commands.".into())
    } else if message.contains("Invalid :error mode") {
        Some("Use `:error`, `:error summary`, or `:error trace`.".into())
    } else {
        None
    }
}
