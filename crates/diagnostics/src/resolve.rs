use crate::heuristics::{
    extract_backticked_target, identifier_span_at, suggested_pipe_slot_rewrite,
    trimmed_line_span_containing,
};
use crate::{simple_error, Color, DiagnosticLabel, DiagnosticSpec};
use spire::ast::Span;

pub fn resolve_error_spec(source: &str, message: impl Into<String>, span: Span) -> DiagnosticSpec {
    let message = message.into();
    let mut spec = simple_error("ResolveError", message.clone(), span.clone(), None);

    if let Some(name) = message.strip_prefix("Undefined variable: ") {
        spec.help = Some(format!(
            "`{}` is not defined in the current scope. Define it or import it before use.",
            name
        ));
        if let Some(name_span) = identifier_span_at(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: name_span,
                message: format!("unresolved name `{}`", name),
                color: Some(Color::Red),
            });
        }
    }

    if let Some(name) = message.strip_prefix("Undefined variable or function: ") {
        spec.help = Some(format!(
            "`{}` is not defined in the current scope. Define it before use, or check that the function name is imported correctly.",
            name
        ));
        if let Some(name_span) = identifier_span_at(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: name_span,
                message: format!("unresolved callable `{}`", name),
                color: Some(Color::Red),
            });
        }
    }

    if let Some(name_arity) = message.strip_prefix("Undefined function ") {
        spec.help = Some(format!(
            "No callable named `{}` is available in this call position. Check the argument count, or capture/pass a function value explicitly.",
            name_arity
        ));
        if let Some(name_span) = identifier_span_at(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: name_span,
                message: format!("unresolved call target `{}`", name_arity),
                color: Some(Color::Red),
            });
        }
    }

    if let Some(name) = message.strip_prefix("Unknown module import: ") {
        spec.help = Some(format!(
            "`{}` is not a known module or trait import target in this compilation context. Check the name, or ensure the defining file is loaded before this import.",
            name
        ));
        if let Some(line_span) = trimmed_line_span_containing(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: line_span,
                message: format!("unknown import target `{}`", name),
                color: Some(Color::Red),
            });
        }
    }

    if let Some(name) = message.strip_prefix("Unknown import member: ") {
        spec.help = Some(format!(
            "`{}` is not exported by the imported module or trait. Check the member name and the import list.",
            name
        ));
        if let Some(line_span) = trimmed_line_span_containing(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: line_span,
                message: format!("unknown import member `{}`", name),
                color: Some(Color::Red),
            });
        }
    }

    if let Some(name) = message.strip_prefix("Duplicate import: ") {
        spec.help = Some(format!(
            "`{}` is imported more than once in the same scope. Keep one import form and remove the duplicate.",
            name
        ));
        if let Some(line_span) = trimmed_line_span_containing(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: line_span,
                message: "duplicate import".into(),
                color: Some(Color::Red),
            });
        }
    }

    if let Some(target) =
        extract_backticked_target(&message, "Import target `", "` is not importable")
    {
        spec.help = Some(format!(
            "`{}` exists, but it cannot be imported directly in this position. Import its module/trait surface instead, or refer to the type by name where that is supported.",
            target
        ));
        if let Some(line_span) = trimmed_line_span_containing(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: line_span,
                message: format!("import target `{}` is not importable", target),
                color: Some(Color::Red),
            });
        }
    }

    if let Some(target) = extract_backticked_target(
        &message,
        "Import target `",
        "` is not available in the current stage",
    ) {
        spec.help = Some(format!(
            "`{}` is declared later than this import can see. Move the import after the definition stage, or restructure the source so the target is available earlier.",
            target
        ));
        if let Some(line_span) = trimmed_line_span_containing(source, span.start) {
            spec.labels.push(DiagnosticLabel {
                source_id: None,
                span: line_span,
                message: format!("import target `{}` is not available yet", target),
                color: Some(Color::Red),
            });
        }
    }

    if message == "pipe placeholder `_1` cannot be used as an expression" {
        if let Some(rewrite) = suggested_pipe_slot_rewrite(source, span.start) {
            spec.help = Some(format!(
                "Move the `_1` transformation into the previous pipe step:\n\n  {}",
                rewrite
            ));
        }
    }

    spec
}
