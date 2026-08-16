pub(super) use crate::heuristics::*;
pub(super) use crate::render::*;
pub(super) use crate::TypeErrorDiagnostic as TypeError;
pub(super) use crate::*;
pub(super) use ariadne::{Color, Fmt};
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
