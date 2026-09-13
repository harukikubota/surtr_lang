pub(super) use crate::render::*;
pub(super) use crate::source::{char_span_to_byte_range, slice_chars};
pub(super) use crate::*;
pub(super) use ariadne::Color;
pub(super) use spire::ast::Span;
pub(super) use std::io::{self, Write};

pub(super) struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("writer failed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("writer failed"))
    }
}

pub(super) fn strip_ansi(input: &str) -> String {
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

pub(super) fn labels_text(spec: &DiagnosticSpec) -> String {
    spec.labels
        .iter()
        .map(|label| strip_ansi(&label.message))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn spec_notes_text(spec: &DiagnosticSpec) -> String {
    spec.notes
        .iter()
        .map(|note| strip_ansi(note))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn parser_error_spec(
    source: &str,
    context: Option<spire::ParserContext>,
) -> DiagnosticSpec {
    let error = match context {
        Some(context) => spire::parse_with_context(source, context),
        None => spire::parse(source),
    }
    .expect_err("test source should produce a parser diagnostic");
    crate::parse_error_spec(crate::SourceId(0), source, &error)
}
