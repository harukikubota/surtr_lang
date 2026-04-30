use super::*;
use crate::heuristics::*;
use crate::render::write_fallback_diagnostic;
use ariadne::Fmt;
use spire::ast::Span;
use std::io::{self, Write};
use crate::TypeErrorDiagnostic as TypeError;

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("writer failed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("writer failed"))
    }
}

fn strip_ansi(input: &str) -> String {
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

mod parse_and_resolve {
    use super::*;

    include!("tests/parse_and_resolve.rs");
}

mod render_and_source {
    use super::*;

    include!("tests/render_and_source.rs");
}

mod runtime {
    use super::*;

    include!("tests/runtime.rs");
}

mod typecheck {
    use super::*;

    include!("tests/typecheck.rs");
}
