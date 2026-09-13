use crate::source::trimmed_line_span_containing;
use crate::{
    simple_error, Color, DiagnosticData, DiagnosticLabel, DiagnosticOrigin, DiagnosticSpec,
    ReplDiagnosticData, ReplDiagnosticReason, SourceFact, SourceId, SourceRole,
    StructuredDiagnostic,
};
use spire::ast::Span;

pub fn repl_query_parse_error_spec(
    source: &str,
    message: impl Into<String>,
    span: Span,
    reason: ReplDiagnosticReason,
) -> DiagnosticSpec {
    let message = message.into();
    let mut spec = structured_repl_error(
        "ReplQueryParseError",
        message,
        span.clone(),
        reason,
        repl_query_help(reason),
    );

    spec.labels.push(DiagnosticLabel {
        source_id: None,
        span: repl_focus_span(source, &span),
        message: repl_query_label(reason).into(),
        color: Some(Color::Red),
    });

    spec
}

pub fn repl_command_parse_error_spec(
    source: &str,
    message: impl Into<String>,
    span: Span,
    reason: ReplDiagnosticReason,
) -> DiagnosticSpec {
    let message = message.into();
    let mut spec = structured_repl_error(
        "ReplCommandError",
        message,
        span.clone(),
        reason,
        repl_command_help(reason),
    );

    spec.labels.push(DiagnosticLabel {
        source_id: None,
        span: repl_focus_span(source, &span),
        message: repl_command_label(reason).into(),
        color: Some(Color::Red),
    });

    spec
}

fn structured_repl_error(
    kind: &'static str,
    message: String,
    span: Span,
    reason: ReplDiagnosticReason,
    help: Option<String>,
) -> DiagnosticSpec {
    let mut spec = simple_error(kind, message.clone(), span.clone(), help);
    spec.structured = Some(StructuredDiagnostic {
        reason: reason.into(),
        origin: DiagnosticOrigin::Parse,
        data: DiagnosticData::Repl(ReplDiagnosticData { detail: message }),
        primary: SourceFact::untyped(SourceRole::Other, SourceId(0), span),
        related: Vec::new(),
        remediation: None,
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

fn repl_query_label(reason: ReplDiagnosticReason) -> &'static str {
    match reason {
        ReplDiagnosticReason::QueryEmpty => "query expected here",
        ReplDiagnosticReason::TypedCallEmptyArgument => "query argument expected here",
        ReplDiagnosticReason::TypedCallMissingClosingParen => {
            "missing closing `)` for this typed call"
        }
        ReplDiagnosticReason::QueryTypeInvalid => "annotated query has an invalid type",
        ReplDiagnosticReason::OperatorMissingTarget => "operator query is missing a target",
        _ => "query parse error",
    }
}

fn repl_query_help(reason: ReplDiagnosticReason) -> Option<String> {
    match reason {
        ReplDiagnosticReason::QueryEmpty => {
            Some("Provide a symbol, typed call, typed operator, or expression query.".into())
        }
        ReplDiagnosticReason::TypedCallEmptyArgument => {
            Some("Provide an argument after the comma, or remove the trailing comma.".into())
        }
        ReplDiagnosticReason::TypedCallMissingClosingParen => {
            Some("Close the typed call with `)` after the final argument.".into())
        }
        ReplDiagnosticReason::QueryTypeInvalid => {
            Some("Use a complete type expression for the query argument.".into())
        }
        ReplDiagnosticReason::OperatorMissingTarget => {
            Some("Write operator queries as `<operator> <target>` after the REPL command.".into())
        }
        _ => None,
    }
}

fn repl_command_label(reason: ReplDiagnosticReason) -> &'static str {
    match reason {
        ReplDiagnosticReason::CommandUnknown => "unknown REPL command",
        ReplDiagnosticReason::CommandArgumentInvalid => "invalid REPL command argument",
        _ => "command parse error",
    }
}

fn repl_command_help(reason: ReplDiagnosticReason) -> Option<String> {
    match reason {
        ReplDiagnosticReason::CommandUnknown => {
            Some("Type `:help` for the list of available REPL commands.".into())
        }
        _ => None,
    }
}
