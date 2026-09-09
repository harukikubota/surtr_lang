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
        TypeDiagnosticReason::ConcreteReturnTypeArgumentInDefinition => {
            return "Definition return type arguments must introduce type inputs".into()
        }
        TypeDiagnosticReason::InlineReturnTypeArgumentConstraint => {
            return "Return type argument constraints belong in the where clause".into()
        }
        TypeDiagnosticReason::ReturnTypeArgumentArityMismatch => {
            if let DiagnosticData::ReturnTypeArgument(value) = &input.data {
                return format!(
                    "{} expects {} return type argument(s), got {}",
                    value.callable,
                    value.expected_count.expect("arity requires expected count"),
                    value.actual_count.expect("arity requires actual count")
                );
            }
        }
        TypeDiagnosticReason::DuplicateReturnTypeArgumentInput => {
            if let DiagnosticData::ReturnTypeArgument(value) = &input.data {
                return format!(
                    "type input `{}` is introduced more than once",
                    value
                        .expected_type
                        .as_deref()
                        .expect("definition input type")
                );
            }
        }
        TypeDiagnosticReason::MissingReturnTypeArgument => {
            if let DiagnosticData::ReturnTypeArgument(value) = &input.data {
                return format!(
                    "return-only type input `{}` is not declared",
                    value
                        .expected_type
                        .as_deref()
                        .expect("definition input type")
                );
            }
        }
        TypeDiagnosticReason::UnusedReturnTypeArgument => {
            if let DiagnosticData::ReturnTypeArgument(value) = &input.data {
                return format!(
                    "return type argument `{}` does not appear in the return type",
                    value
                        .expected_type
                        .as_deref()
                        .expect("definition input type")
                );
            }
        }
        TypeDiagnosticReason::AmbiguousReturnTypeArgument => {
            if let DiagnosticData::ReturnTypeArgument(value) = &input.data {
                return format!(
                    "return type arguments for `{}` cannot be inferred",
                    value.callable
                );
            }
        }
        TypeDiagnosticReason::UnresolvedEnumConstructorTypeArgument => {
            if let DiagnosticData::EnumConstructorTypeArgument(value) = &input.data {
                return format!(
                    "type argument {} for enum constructor `{}` cannot be inferred",
                    value.ordinal, value.constructor
                );
            }
        }
        TypeDiagnosticReason::CallableSignatureMetadataMismatch => {
            if let DiagnosticData::CallableSignature(value) = &input.data {
                return format!(
                    "canonical callable signature for `{}` is inconsistent",
                    value.callable
                );
            }
        }
        TypeDiagnosticReason::NoApplicableTraitImplementation => {
            if let DiagnosticData::CandidateSelection(value) = &input.data {
                return format!(
                    "no `{}` candidate can satisfy `{}::{}`",
                    value.trait_name, value.trait_name, value.method
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
        DiagnosticData::TraitMethodConstraint(value) => format!(
            "Trait impl method `{}` has incompatible trait constraints",
            value.method_name
        ),
        DiagnosticData::TraitMethodTypeList(value) => match input.reason {
            TypeDiagnosticReason::TraitMethodTypeListArityMismatch => format!(
                "Trait impl method `{}` has incompatible {} arity: expected {}, got {}",
                value.method_name,
                value.role.as_str(),
                value
                    .expected_count
                    .expect("arity diagnostic carries expected count"),
                value
                    .actual_count
                    .expect("arity diagnostic carries actual count")
            ),
            _ => {
                let mut message = format!(
                    "Trait impl method `{}` has an incompatible signature at {} {}",
                    value.method_name,
                    value.role.as_str(),
                    value.ordinal
                );
                if !value.nested_path.is_empty() {
                    message.push_str(&format!(
                        " (type argument path: {})",
                        value
                            .nested_path
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(" → ")
                    ));
                }
                message
            }
        },
        DiagnosticData::ReturnTypeArgument(value) => format!(
            "Return type argument {} for `{}` does not match the callable signature",
            value.ordinal.expect("type argument mismatch ordinal"), value.callable
        ),
        DiagnosticData::CallableShape(value) => {
            let actual = value.actual_type.clone().unwrap_or_else(|| format!("closure with {} parameter(s)", value.actual_arity.expect("closure shape carries its arity")));
            match input.reason {
                TypeDiagnosticReason::NotCallable => format!("Not a function: {actual}"),
                TypeDiagnosticReason::CallableShapeMismatch => match value.return_shape {
                    crate::CallableReturnShape::Plain => format!("{} expects a plain function return, got {actual}", value.callable),
                    crate::CallableReturnShape::Any => match value.expected_arity {
                        Some(arity) => format!("{} expects a callable with {arity} argument(s), got {actual}", value.callable),
                        None => {
                            let expected = input.related.iter().find(|fact| fact.role == crate::SourceRole::Expected).and_then(|fact| fact.ty.as_deref()).expect("non-callable expected shape carries its type fact");
                            format!("{} does not match expected type {expected}", value.callable)
                        }
                    },
                },
                _ => unreachable!("callable shape requires callable reason"),
            }
        },
        DiagnosticData::ArgumentContract(value) => match input.reason {
            TypeDiagnosticReason::ArityMismatch => format!("{} expects {} argument(s), got {}", value.callable, value.expected_count, value.actual_count),
            TypeDiagnosticReason::ArgumentModeMismatch => "Cannot mix positional and named arguments, or use named arguments with this callable".into(),
            TypeDiagnosticReason::UnknownNamedArgument => format!("Unknown argument name '{}' for function", value.name.as_deref().expect("named argument diagnostic requires name")),
            TypeDiagnosticReason::DuplicateArgument => format!("Duplicate argument '{}'", value.name.as_deref().expect("duplicate argument requires name")),
            TypeDiagnosticReason::MissingArgument => format!("Missing argument '{}'", value.name.as_deref().expect("missing argument requires name")),
            _ => unreachable!("argument contract requires a contract reason"),
        },
        DiagnosticData::ArgumentRelation(value) => {
            let expected = value.expected_type.as_deref();
            let actual = value.actual_type.as_deref();
            let label = match input.reason {
                TypeDiagnosticReason::ReturnTypeMismatch => "Return type mismatch",
                TypeDiagnosticReason::AnnotationTypeMismatch => "Annotation type mismatch",
                TypeDiagnosticReason::CallableShapeMismatch => "Callable shape mismatch",
                _ => "Argument type mismatch",
            };
            match (expected, actual) {
                (Some(expected), Some(actual)) => format!("{label}: expected {expected}, got {actual}"),
                _ => label.into(),
            }
        },
        DiagnosticData::TraitObligation(value) => {
            let prefix = match input.reason {
                TypeDiagnosticReason::MissingGenericBound => "MissingGenericBound",
                TypeDiagnosticReason::MissingTraitCapability => "MissingTraitCapability",
                TypeDiagnosticReason::MissingTypeConstructorCapability => "MissingTypeConstructorCapability",
                TypeDiagnosticReason::NoApplicableTraitImplementation => "NoApplicableTraitImplementation",
                _ => unreachable!("trait obligation requires a capability or applicability reason"),
            };
            format!("{prefix}: {} must implement {}{}", value.subject_type, value.trait_name,
                if value.trait_arguments.is_empty() { String::new() } else { format!("<{}>", value.trait_arguments.join(", ")) })
        },
        DiagnosticData::TraitDispatch(value) => {
            let obligation = if value.trait_arguments.is_empty() { value.trait_name.clone() } else { format!("{}<{}>", value.trait_name, value.trait_arguments.join(", ")) };
            match input.reason {
                TypeDiagnosticReason::NoApplicableTraitImplementation => format!("No implementation satisfies {} for {}", obligation, value.subject_type.as_deref().expect("concrete obligation has a subject")),
                TypeDiagnosticReason::UnresolvedTraitMethodInstantiation => format!("{}::{} requires a concrete method instantiation", obligation, value.method.as_deref().expect("method instantiation has a method")),
                TypeDiagnosticReason::MissingTraitDispatchTarget => format!("{}::{} has no concrete dispatch target", obligation, value.method.as_deref().expect("method instantiation has a method")),
                _ => unreachable!("dispatch diagnostic requires dispatch reason"),
            }
        },
        DiagnosticData::TypeConstructorCarrier(value) => match input.reason {
            TypeDiagnosticReason::TypePayloadMismatch => format!("Type payload mismatch: expected {}, got {}", value.expected_carrier, value.actual_carrier),
            TypeDiagnosticReason::TypeConstructorFamilyMismatch => format!("Type constructor family mismatch: expected {}, got {}", value.expected_carrier, value.actual_carrier),
            TypeDiagnosticReason::MissingTypeConstructorCapability => format!("Type constructor occurrence requires {}", value.family),
            _ => unreachable!("carrier diagnostic requires carrier reason"),
        }
        DiagnosticData::BranchAssertion(value) => match input.reason {
            TypeDiagnosticReason::IfBranchTypeMismatch => format!("if branches have different types: {} and {}", value.expected_type, value.actual_type),
            TypeDiagnosticReason::MatchArmTypeMismatch => format!("Match arm type mismatch: expected {}, got {}", value.expected_type, value.actual_type),
            TypeDiagnosticReason::CondBranchTypeMismatch => format!("cond branches have different types: {} and {}", value.expected_type, value.actual_type),
            _ => unreachable!("branch assertions require a branch reason"),
        },
        _ => match input.reason {
            TypeDiagnosticReason::ArityMismatch
            | TypeDiagnosticReason::ReturnTypeArgumentArityMismatch
            | TypeDiagnosticReason::TraitMethodTypeListArityMismatch => {
                "Callable arity does not match".into()
            }
            TypeDiagnosticReason::ReturnTypeMismatch => "Return type does not match".into(),
            TypeDiagnosticReason::AnnotationTypeMismatch => "Annotation type does not match".into(),
            TypeDiagnosticReason::ConcreteReturnTypeArgumentInDefinition => "Definition return type arguments must introduce type inputs".into(),
            TypeDiagnosticReason::InlineReturnTypeArgumentConstraint => "Return type argument constraints belong in the where clause".into(),
            reason => reason.as_str().to_string(),
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
