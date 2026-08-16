use crate::heuristics::{
    line_span_containing, line_spans, rewrite_line_at_span, slice_chars, trimmed_line_span,
    trimmed_line_span_containing,
};
use crate::{simple_error, Color, DiagnosticLabel, DiagnosticSpec};
use spire::ast::Span;

pub fn parse_error_spec(source: &str, message: impl Into<String>, span: Span) -> DiagnosticSpec {
    let message = message.into();
    let mut spec = simple_error("ParseError", message.clone(), span.clone(), None);

    if message == "This top-level declaration is not allowed in the current source policy" {
        spec.help = Some(
            "Move this declaration into a module compile unit, or replace it with an expression that is allowed in this source kind."
                .into(),
        );
        if let Some(line_span) = trimmed_line_span_containing(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: line_span,
                message: "forbidden top-level declaration".into(),
                color: Some(Color::Red),
            });
        }
    }

    if message == "Top-level expressions are not allowed in module compile units" {
        spec.help = Some(
            "Wrap this code in a `def`, `defmod`, or another declaration that is valid at module top level."
                .into(),
        );
        if let Some(line_span) = trimmed_line_span_containing(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: line_span,
                message: "top-level expression is not allowed here".into(),
                color: Some(Color::Red),
            });
        }
    }

    if message == "Top-level expressions are not allowed in this source context" {
        spec.help = Some(
            "This source kind only accepts declarations at the top level. Move the expression into a function or another executable context."
                .into(),
        );
        if let Some(line_span) = trimmed_line_span_containing(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: line_span,
                message: "top-level expression is not allowed here".into(),
                color: Some(Color::Red),
            });
        }
    }

    if message == "The Unit type has no pattern matching." {
        spec.help = Some("Variable bindings and the `_` wildcard pattern are allowed.".into());
    }

    if message
        == "as-pattern alias must be a binding identifier."
    {
        spec.help =
            Some("Replace the wildcard alias with a name, for example `pattern @ value`.".into());
    }

    if message == "Range literals must use bracket syntax" {
        spec.help = Some("Write `[start..stop]`.".into());
    }

    if let Some(operator) = message.strip_prefix("Unquoted operator capture: ") {
        spec.help = Some(format!("Write &`{operator}`."));
    }

    if message.starts_with("return-position `impl Trait` is not supported") {
        spec.help =
            Some("Name the return type parameter explicitly in the function signature.".into());
        if let Some(line_span) = trimmed_line_span_containing(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: line_span,
                message: "return-position `impl Trait` is not supported".into(),
                color: Some(Color::Red),
            });
        }
    }

    if message == "`where` clauses are staged and not implemented yet" {
        spec.help = Some(
            "Rewrite the constraint as explicit type parameters or defer this API shape until `where` clauses are available."
                .into(),
        );
        if let Some(line_span) = trimmed_line_span_containing(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: line_span,
                message: "`where` clauses are not available yet".into(),
                color: Some(Color::Red),
            });
        }
    }

    if message.starts_with("Unexpected token:") {
        spec.help = Some(
            "The parser stopped at this token. Check the expression immediately before it.".into(),
        );
    }

    if message == "meta requires state" {
        spec.help = Some(
            "Add a state declaration inside `meta { ... }`. For example:\n\n  state: Int".into(),
        );
    }

    if message == "meta requires instance" {
        spec.help = Some(
            "Add an instance declaration inside `meta { ... }`. For example:\n\n  instance: Singleton"
                .into(),
        );
    }

    if message == "meta requires state" || message == "meta requires instance" {
        if let Some(process_decl_span) = previous_non_empty_line_span(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: process_decl_span,
                message: "process declaration".into(),
                color: Some(Color::Blue),
            });
        }
    }

    if let Some(token) = message.strip_prefix("Unexpected token: ") {
        if let Some(line_span) = line_span_containing(source, span.start) {
            let line = slice_chars(source, line_span.0, line_span.1);
            let token_hint = match token {
                "RParen" => "unexpected closing parenthesis",
                "RBrace" => "unexpected closing brace",
                "RBracket" => "unexpected closing bracket",
                _ => "unexpected token",
            };
            if !line.trim().is_empty() {
                spec.labels.push(DiagnosticLabel {
                    source_id: None,
                    span: span.clone(),
                    message: token_hint.into(),
                    color: Some(Color::Red),
                });
            }
        }
    }

    if message == "anonymous capture is not supported; use `&id` instead" {
        if let Some(rewrite) = rewrite_line_at_span(source, &span, "&id") {
            spec.help = Some(format!(
                "Replace this anonymous capture with:\n\n  {}",
                rewrite
            ));
        }
    }

    if message
        == "anonymous capture is not supported; extract a named function and capture it like `&fun_name(&1, &2)`"
    {
        if let Some(rewrite) = rewrite_line_at_span(source, &span, "&fun_name(&1, &2)") {
            spec.help = Some(format!(
                "Extract the body into a named helper and replace this capture with:\n\n  {}",
                rewrite
            ));
        }
    }

    if message
        == "Immediate calls on anonymous callable expressions are not supported; bind the callable to a name and call it as `fn(args)`"
    {
        spec.help = Some(
            "Bind the callable to a name before calling it. For example:\n\n  f = &add(&1, 10)\n  f(4)\n\n  f = {|x| x + 1}\n  f(4)\n\n  tmp = make()\n  tmp(4)"
                .into(),
        );
        if let Some(line_span) = trimmed_line_span_containing(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: line_span,
                message: "anonymous callable is followed by an immediate call".into(),
                color: Some(Color::Red),
            });
        }
    }

    spec
}

fn previous_non_empty_line_span(source: &str, pos: usize) -> Option<Span> {
    let lines = line_spans(source);
    let current_idx = lines
        .iter()
        .position(|(start, end)| *start <= pos && pos <= *end)?;
    for idx in (0..current_idx).rev() {
        let Some(span) = trimmed_line_span(source, lines[idx]) else {
            continue;
        };
        if span.start < span.end {
            return Some(span);
        }
    }
    None
}
