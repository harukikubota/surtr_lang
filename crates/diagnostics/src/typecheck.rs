use crate::heuristics::{
    callable_definition_signature_from_hint, extract_expected_got, extractor_error_locus_span,
    extractor_input_context, extractor_name_and_rule, find_extractor_definition_label,
    has_missing_trait_method_labels, has_trait_impl_signature_mismatch_labels,
    infer_missing_trait_method_labels, infer_trait_impl_signature_mismatch_labels,
    infer_type_error_template, is_flow_operator_message, line_index_for_span, line_spans,
    parse_binary_operator_error,
};
use crate::{simple_error, Color, DiagnosticLabel, DiagnosticSpec, SourceId, SourceRegistry};
use scar::error::TypeError;

pub fn type_error_spec(source: &str, error: &TypeError) -> DiagnosticSpec {
    let mut spec = simple_error(
        "TypeError",
        error.message.clone(),
        error.span.clone(),
        error.hint.clone(),
    );

    if let Some(labels) = infer_trait_impl_signature_mismatch_labels(source, &error.message) {
        if let Some(primary) = labels
            .iter()
            .find(|label| label.message.starts_with("actual "))
        {
            spec.primary_span = primary.span.clone();
        }
        spec.labels.extend(labels);
        spec.help = None;
        return spec;
    }

    if let Some(labels) = infer_missing_trait_method_labels(source, &error.message) {
        if let Some(first) = labels.first() {
            spec.primary_span = first.span.clone();
        }
        spec.labels.extend(labels);
    }

    let replace_help = is_flow_operator_message(&error.message)
        || parse_binary_operator_error(&error.message).is_some();
    let inferred_template =
        infer_type_error_template(source, &error.span, &error.message, error.hint.as_deref());

    if let Some(template) = inferred_template {
        spec.labels.extend(template.labels);
        spec.notes.extend(template.notes);
        if let Some(help) = template.help {
            spec.help = Some(if replace_help {
                help
            } else {
                match spec.help.take() {
                    Some(existing) => format!("{}\n{}", existing, help),
                    None => help,
                }
            });
        }
    }
    if error.message == "Only total MatchBlock patterns can be used with `=`" {
        spec.help = None;
    }
    if spec.help.is_none() {
        if let (Some(expected), Some(got)) = extract_expected_got(&spec.message) {
            spec.help = Some(format!(
                "This location requires {}, but the expression currently has {}.",
                expected, got
            ));
        }
    }
    if spec
        .help
        .as_deref()
        .is_some_and(|help| callable_definition_signature_from_hint(help).is_some())
    {
        spec.help = None;
    }

    spec
}

pub fn type_error_spec_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    error: &TypeError,
) -> DiagnosticSpec {
    let mut spec = type_error_spec(sources.source(source_id).unwrap_or(""), error);
    apply_extractor_context_by_id(sources, source_id, error, &mut spec);
    if has_missing_trait_method_labels(&spec) || has_trait_impl_signature_mismatch_labels(&spec) {
        for label in &mut spec.labels {
            if matches!(
                label.message.as_str(),
                "impl target"
                    | "missing required method"
                    | "trait declaration"
                    | "trait impl declaration"
            ) {
                label.source_id = Some(source_id);
            } else if label.message.starts_with("expected ") || label.message.starts_with("actual ")
            {
                label.source_id = Some(source_id);
            }
        }
    }
    spec
}

fn apply_extractor_context_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    error: &TypeError,
    spec: &mut DiagnosticSpec,
) {
    if !error.message.starts_with("Extractor ") {
        return;
    }
    spec.labels.retain(|label| {
        !matches!(
            label.message.as_str(),
            "extractor pattern checked against the match scrutinee"
                | "extractor pattern checked against the SafeBind RHS"
        )
    });
    let Some(source) = sources.source(source_id) else {
        return;
    };
    let lines = line_spans(source);
    let Some(error_line_idx) = line_index_for_span(&lines, error.span.start) else {
        return;
    };

    if let Some((context_span, context_ty)) =
        extractor_input_context(source, &lines, error_line_idx, &error.message)
    {
        spec.labels.push(DiagnosticLabel {
            source_id: Some(source_id),
            span: context_span,
            message: format!("input source: {}", context_ty),
            color: Some(Color::Yellow),
        });
    }

    if let Some(pattern_span) = extractor_error_locus_span(source, &lines, error_line_idx) {
        spec.primary_span = pattern_span;
    }

    if let Some((extractor_name, _rule_text)) = extractor_name_and_rule(&error.message) {
        if let Some((def_source_id, def_span, def_label)) =
            find_extractor_definition_label(sources, &extractor_name)
        {
            spec.labels.push(DiagnosticLabel {
                source_id: Some(def_source_id),
                span: def_span,
                message: format!("Extractor definition: {}", def_label),
                color: Some(Color::Blue),
            });
        }
    }
}
