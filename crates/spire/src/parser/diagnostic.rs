use crate::ast::Span;
use crate::error::ParseError;

use super::error_map;

#[derive(Debug, Clone)]
pub struct ParseDiagnostic {
    pub error: ParseError,
    pub expected_tokens: Vec<String>,
    pub cursor_span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LspDiagnosticSeverity {
    Error = 1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspRelatedInformation {
    pub message: String,
    pub range: LspRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspDiagnostic {
    pub range: LspRange,
    pub severity: LspDiagnosticSeverity,
    pub code: String,
    pub source: String,
    pub message: String,
    pub related_information: Vec<LspRelatedInformation>,
}

impl ParseDiagnostic {
    pub(crate) fn from_parse_error(error: ParseError) -> Self {
        let span = error.span().clone();
        Self {
            error,
            expected_tokens: Vec::new(),
            cursor_span: span,
        }
    }

    pub fn to_lsp(&self, source: &str) -> LspDiagnostic {
        let primary_span = self.error.span();
        let primary_range = span_to_lsp_range(source, primary_span);
        let cursor_range = span_to_lsp_range(source, &self.cursor_span);

        let mut related_information = Vec::new();
        if !self.expected_tokens.is_empty() {
            related_information.push(LspRelatedInformation {
                message: format!("expected one of: {}", self.expected_tokens.join(", ")),
                range: cursor_range,
            });
        } else if let ParseError::Incomplete { expected, .. } = &self.error {
            related_information.push(LspRelatedInformation {
                message: format!("expected: {expected}"),
                range: cursor_range,
            });
        }

        let code = if self.error.is_incomplete() {
            "parse.incomplete"
        } else {
            "parse.syntax"
        };

        LspDiagnostic {
            range: primary_range,
            severity: LspDiagnosticSeverity::Error,
            code: code.to_string(),
            source: "spire".to_string(),
            message: self.error.message(),
            related_information,
        }
    }
}

impl From<ParseError> for ParseDiagnostic {
    fn from(error: ParseError) -> Self {
        Self::from_parse_error(error)
    }
}

impl From<error_map::ParseErrorDiagnostic> for ParseDiagnostic {
    fn from(diag: error_map::ParseErrorDiagnostic) -> Self {
        Self {
            error: diag.error,
            expected_tokens: diag.expected_tokens,
            cursor_span: diag.cursor_span,
        }
    }
}

fn span_to_lsp_range(source: &str, span: &Span) -> LspRange {
    let start = char_offset_to_position(source, span.start);
    let clamped_end = span.end.max(span.start);
    let end = char_offset_to_position(source, clamped_end);
    LspRange { start, end }
}

fn char_offset_to_position(source: &str, offset: usize) -> LspPosition {
    let mut line = 0u32;
    let mut character = 0u32;
    let mut seen = 0usize;
    let limit = offset.min(source.chars().count());

    for ch in source.chars() {
        if seen >= limit {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
        seen += 1;
    }

    LspPosition { line, character }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{parse_with_context_diagnostic, ParserContext};

    #[test]
    fn to_lsp_maps_multiline_span_to_line_and_column() {
        let diag = ParseDiagnostic {
            error: ParseError::syntax("unexpected token", Span { start: 6, end: 9 }),
            expected_tokens: Vec::new(),
            cursor_span: Span { start: 6, end: 9 },
        };

        let lsp = diag.to_lsp("line1\nabc\nz");
        assert_eq!(lsp.range.start.line, 1);
        assert_eq!(lsp.range.start.character, 0);
        assert_eq!(lsp.range.end.line, 1);
        assert_eq!(lsp.range.end.character, 3);
        assert_eq!(lsp.code, "parse.syntax");
        assert!(lsp.related_information.is_empty());
    }

    #[test]
    fn parse_with_context_diagnostic_exposes_expected_tokens_for_incomplete_input() {
        let diag = parse_with_context_diagnostic("def foo(", ParserContext::repl(1))
            .expect_err("should report parse diagnostic");

        assert!(!diag.expected_tokens.is_empty());

        let lsp = diag.to_lsp("def foo(");
        assert!(matches!(lsp.code.as_str(), "parse.incomplete" | "parse.syntax"));
        assert_eq!(lsp.severity, LspDiagnosticSeverity::Error);
        assert!(!lsp.related_information.is_empty());
    }
}
