use chumsky::error::{Rich, RichReason};
use chumsky::span::SimpleSpan;

use crate::ast::Span;
use crate::error::ParseError;
use crate::token::{Spanned, Token};

#[derive(Debug, Clone)]
pub(crate) struct ParseErrorDiagnostic {
    pub(crate) error: ParseError,
    pub(crate) expected_tokens: Vec<String>,
    pub(crate) cursor_span: Span,
}

pub(crate) fn map_chumsky_error_with_diagnostic(
    tokens: &[Spanned<Token>],
    mut errs: Vec<Rich<'_, Spanned<Token>>>,
) -> ParseErrorDiagnostic {
    let Some(err) = errs.pop() else {
        return ParseErrorDiagnostic {
            error: ParseError::syntax("unknown parse error", fallback_span(tokens)),
            expected_tokens: Vec::new(),
            cursor_span: fallback_span(tokens),
        };
    };

    let span = map_input_span(tokens, err.span());
    let expected_tokens = extract_expected_tokens(&err);
    let message = match err.reason() {
        RichReason::Custom(msg) => msg.clone(),
        RichReason::ExpectedFound { .. } => "unexpected token while parsing program".to_string(),
    };

    let error = if let Some(expected) = message.strip_prefix("Incomplete input: expected ") {
        ParseError::incomplete(expected.to_string(), span.clone())
    } else if message.contains("end of input") || message.contains("unexpected end of input") {
        ParseError::incomplete("input", span.clone())
    } else if matches!(err.reason(), RichReason::ExpectedFound { .. }) && err.found().is_none() {
        let expected = if expected_tokens.is_empty() {
            "token".to_string()
        } else {
            expected_tokens.join(" | ")
        };
        ParseError::incomplete(expected, span.clone())
    } else {
        ParseError::syntax(message, span.clone())
    };

    ParseErrorDiagnostic {
        error,
        expected_tokens,
        cursor_span: span,
    }
}

fn extract_expected_tokens(err: &Rich<'_, Spanned<Token>>) -> Vec<String> {
    let mut out = Vec::new();

    if let RichReason::Custom(message) = err.reason() {
        if let Some(expected) = message.strip_prefix("Incomplete input: expected ") {
            out.push(expected.to_string());
            return out;
        }
    }

    for expected in err.expected() {
        let label = format!("{expected:?}");
        if !out.contains(&label) {
            out.push(label);
        }
    }

    out
}

fn map_input_span(tokens: &[Spanned<Token>], span: &SimpleSpan<usize>) -> Span {
    if tokens.is_empty() {
        return Span { start: 0, end: 0 };
    }

    let start_idx = span.start.min(tokens.len().saturating_sub(1));
    let end_idx = span
        .end
        .saturating_sub(1)
        .min(tokens.len().saturating_sub(1));

    let start = tokens[start_idx].span.start;
    let end = if span.end == 0 {
        tokens[start_idx].span.end
    } else {
        tokens[end_idx].span.end
    };

    Span { start, end }
}

fn fallback_span(tokens: &[Spanned<Token>]) -> Span {
    tokens
        .last()
        .map(|tok| tok.span.clone())
        .unwrap_or(Span { start: 0, end: 0 })
}
