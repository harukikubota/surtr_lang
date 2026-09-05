use crate::heuristics::{
    callable_definition_signature_from_hint, extract_expected_got, extractor_error_locus_span,
    extractor_input_context, extractor_name_and_rule, find_extractor_definition_label,
    has_missing_trait_method_labels, has_trait_impl_signature_mismatch_labels,
    infer_missing_trait_method_labels, infer_trait_impl_signature_mismatch_labels,
    infer_type_error_template, is_flow_operator_message, line_index_for_span, line_spans,
    parse_binary_operator_error,
};
use crate::{
    simple_error, Color, DiagnosticData, DiagnosticLabel, DiagnosticSpec, SourceFact, SourceId,
    SourceRegistry, StructuredDiagnostic, TypeDiagnosticReason,
};
use spire::ast::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct TypeErrorDiagnostic {
    pub message: String,
    pub span: Span,
    pub hint: Option<String>,
}

impl TypeErrorDiagnostic {
    pub fn new(message: impl Into<String>, span: Span, hint: Option<String>) -> Self {
        Self {
            message: message.into(),
            span,
            hint,
        }
    }
}

pub fn type_error_spec(source: &str, error: &TypeErrorDiagnostic) -> DiagnosticSpec {
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

    let binary_operator_error = parse_binary_operator_error(&error.message).is_some();
    let replace_help = is_flow_operator_message(&error.message) || binary_operator_error;
    let inferred_template =
        infer_type_error_template(source, &error.span, &error.message, error.hint.as_deref());

    if let Some(template) = inferred_template {
        spec.labels.extend(template.labels);
        spec.notes.extend(template.notes);
        if let Some(help) = template.help {
            spec.help = Some(if replace_help {
                match spec.help.take() {
                    Some(existing)
                        if binary_operator_error && existing.contains("is implemented for:") =>
                    {
                        format!("{}\n{}", existing, help)
                    }
                    Some(existing) if existing == help => existing,
                    _ => help,
                }
            } else {
                match spec.help.take() {
                    Some(existing) if existing == help => existing,
                    Some(existing) => format!("{}\n{}", existing, help),
                    None => help,
                }
            });
        }
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
    error: &TypeErrorDiagnostic,
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

/// Project a completed structured type diagnostic into the renderer's common
/// representation.  This is deliberately separate from `type_error_spec`,
/// whose two-argument form remains the compatibility boundary for checker
/// errors that have not migrated off their legacy message payload yet.
pub fn structured_type_error_spec(input: &StructuredDiagnostic) -> DiagnosticSpec {
    let mut spec = simple_error(
        "TypeError",
        structured_headline(input),
        input.primary.span.clone(),
        input.remediation_text(),
    );
    spec.structured = Some(input.clone());
    spec.labels = std::iter::once(&input.primary)
        .chain(input.related.iter())
        .map(source_fact_label)
        .collect();
    spec
}

/// Explicitly named alias for callers that want to make the structured
/// boundary visible at the call site.
pub fn type_error_spec_from_structured(input: &StructuredDiagnostic) -> DiagnosticSpec {
    structured_type_error_spec(input)
}

fn structured_headline(input: &StructuredDiagnostic) -> String {
    match input.reason {
        TypeDiagnosticReason::DuplicateReturnTypeArgumentInput => {
            if let DiagnosticData::ReturnTypeArgument(value) = &input.data {
                return format!(
                    "type input `{}` is introduced more than once",
                    value.expected_type
                );
            }
        }
        TypeDiagnosticReason::MissingReturnTypeArgument => {
            if let DiagnosticData::ReturnTypeArgument(value) = &input.data {
                return format!(
                    "return-only type input `{}` is not declared",
                    value.expected_type
                );
            }
        }
        TypeDiagnosticReason::UnusedReturnTypeArgument => {
            if let DiagnosticData::ReturnTypeArgument(value) = &input.data {
                return format!(
                    "return type argument `{}` does not appear in the return type",
                    value.expected_type
                );
            }
        }
        TypeDiagnosticReason::InvalidTraitConstraintSubject => {
            if let DiagnosticData::ConstraintSubject(value) = &input.data {
                return format!(
                    "trait `{}` cannot be used as a constraint subject",
                    value.subject
                );
            }
        }
        TypeDiagnosticReason::MissingTypeConstructorConstraint => {
            if let DiagnosticData::ConstraintSubject(value) = &input.data {
                return format!(
                    "type constructor variable `{}` requires a TypeCtorTrait constraint",
                    value.subject
                );
            }
        }
        _ => {}
    }
    match &input.data {
        DiagnosticData::ReturnTypeArgument(value) => format!(
            "Return type argument {} for `{}` does not match the callable signature",
            value.ordinal, value.callable
        ),
        DiagnosticData::ArgumentRelation(value) => format!(
            "Argument {} does not match the callable signature `{}`",
            value.ordinal, value.callable
        ),
        DiagnosticData::TypeConstructorCarrier(_value) => {
            "Type constructor carrier does not match the required family".into()
        }
        DiagnosticData::BranchAssertion(_) => "Branch types do not match".into(),
        _ => match input.reason {
            TypeDiagnosticReason::ArityMismatch
            | TypeDiagnosticReason::ReturnTypeArgumentArityMismatch
            | TypeDiagnosticReason::TraitMethodTypeListArityMismatch => {
                "Callable arity does not match".into()
            }
            TypeDiagnosticReason::ReturnTypeMismatch => "Return type does not match".into(),
            TypeDiagnosticReason::AnnotationTypeMismatch => "Annotation type does not match".into(),
            _ => "Type checking failed".into(),
        },
    }
}

fn source_fact_label(fact: &SourceFact) -> DiagnosticLabel {
    let message = match fact.ty.as_deref() {
        Some(ty) => format!("{}: {}", fact.role.as_str(), ty),
        None => fact.role.as_str().to_string(),
    };
    DiagnosticLabel {
        source_id: Some(fact.source_id),
        span: fact.span.clone(),
        message,
        color: Some(Color::Blue),
    }
}

fn apply_extractor_context_by_id(
    sources: &SourceRegistry,
    source_id: SourceId,
    error: &TypeErrorDiagnostic,
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

    if let Some((_context_span, context_ty)) =
        extractor_input_context(source, &lines, error_line_idx, &error.message)
    {
        spec.notes.push(format!("input source: {}", context_ty));
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
