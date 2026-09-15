use crate::{
    simple_error, Color, DiagnosticData, DiagnosticLabel, DiagnosticOrigin, DiagnosticSpec,
    PolicyData, SourceFact, SourceId, StructuredDiagnostic, TypeDiagnosticReason, TypePolicy,
};
use spire::ast::Span;

/// Build the explicit producer-contract failure used when an adapter receives
/// an unstructured Scar type error. Message, hint, and source text are
/// intentionally unavailable at this boundary.
pub fn typecheck_invariant_spec(source_id: SourceId, span: Span) -> DiagnosticSpec {
    let input = StructuredDiagnostic {
        reason: TypeDiagnosticReason::TypecheckInvariantViolation.into(),
        origin: DiagnosticOrigin::Intrinsic,
        data: DiagnosticData::Policy(PolicyData {
            policy: TypePolicy::ProducerContract,
            subject: None,
            expected_type: None,
            actual_type: None,
            stage: None,
            entrypoint: None,
        }),
        primary: SourceFact::untyped(crate::SourceRole::Value, source_id, span),
        related: Vec::new(),
        remediation: None,
    };
    structured_type_error_spec(&input)
}

/// Preserve an unstructured producer's display text while marking the missing
/// typed payload as a producer-contract violation. The text is never inspected
/// to infer a stable reason or typed field.
pub fn typecheck_invariant_spec_with_display(
    source_id: SourceId,
    span: Span,
    message: impl Into<String>,
    hint: Option<String>,
) -> DiagnosticSpec {
    let mut spec = typecheck_invariant_spec(source_id, span);
    spec.message = message.into();
    spec.help = hint;
    spec
}

/// Project a completed structured type diagnostic into the renderer's common
/// representation.
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
    if input.reason.type_reason() == Some(TypeDiagnosticReason::ReservedIntrinsicMarkerUsage) {
        let marker = match &input.data {
            DiagnosticData::Policy(value) => value.subject.as_deref().unwrap_or("intrinsic marker"),
            _ => "intrinsic marker",
        };
        if let Some(primary) = spec.labels.first_mut() {
            primary.message = format!("`{marker}` cannot be used in this type position");
        }
        spec.notes
            .push(format!("`{marker}` is not an ordinary value type"));
    }
    spec
}

/// Explicitly named alias for callers that want to make the structured
/// boundary visible at the call site.
pub fn type_error_spec_from_structured(input: &StructuredDiagnostic) -> DiagnosticSpec {
    structured_type_error_spec(input)
}

fn structured_headline(input: &StructuredDiagnostic) -> String {
    let reason = input
        .reason
        .type_reason()
        .expect("typecheck renderer requires a type diagnostic reason");
    match reason {
        TypeDiagnosticReason::SafeBindTotalPatternNonMonadRhs => {
            if let DiagnosticData::SafeBindRelation(value) = &input.data {
                return format!(
                    "{} is not a SafeBind target; it is not a Monad, and only a Result RHS can be decomposed by `=?`.",
                    value.rhs_type
                );
            }
        }
        TypeDiagnosticReason::SafeBindTotalPatternNonResultMonadRhs => {
            if let DiagnosticData::SafeBindRelation(value) = &input.data {
                return format!(
                    "{} is not a SafeBind target; `=?` propagates Result-style failures, not values from another Monad. Only a Result RHS can be decomposed by `=?`.",
                    value.rhs_type
                );
            }
        }
        TypeDiagnosticReason::ConcreteReturnTypeArgumentInDefinition => {
            return "Definition return type arguments must introduce type inputs".into();
        }
        TypeDiagnosticReason::InlineReturnTypeArgumentConstraint => {
            return "Return type argument constraints belong in the where clause".into();
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
        TypeDiagnosticReason::PatternTypeMismatch => {
            if let DiagnosticData::Pattern(value) = &input.data {
                return format!(
                    "Pattern type mismatch: expected {}, got {}",
                    value
                        .expected_type
                        .as_deref()
                        .expect("pattern expected type"),
                    value.actual_type.as_deref().expect("pattern actual type")
                );
            }
        }
        TypeDiagnosticReason::PatternShapeMismatch => {
            if let DiagnosticData::Pattern(value) = &input.data {
                return format!(
                    "{} pattern requires {}, got {}",
                    value.name.as_deref().expect("pattern shape name"),
                    value
                        .expected_type
                        .as_deref()
                        .expect("pattern expected shape"),
                    value.actual_type.as_deref().expect("pattern actual shape")
                );
            }
        }
        TypeDiagnosticReason::PatternArityMismatch => {
            if let DiagnosticData::Pattern(value) = &input.data {
                return format!(
                    "{} pattern expects {} value(s), got {}",
                    value.name.as_deref().expect("pattern arity name"),
                    value.expected_count.expect("pattern expected arity"),
                    value.actual_count.expect("pattern actual arity")
                );
            }
        }
        TypeDiagnosticReason::NonTotalBindingPattern => {
            return "Only total MatchBlock patterns can be used with `=`".into();
        }
        TypeDiagnosticReason::NestedResultErrorPattern => {
            return "Nested Result errors are not allowed in match patterns: use Err(error) for the outer failure, or Ok(Err(error)) for an inner failure.".into();
        }
        TypeDiagnosticReason::MatchGuardTypeMismatch => {
            if let DiagnosticData::Pattern(value) = &input.data {
                return format!(
                    "match guard must be Boolean, got {}",
                    value.actual_type.as_deref().expect("match guard type")
                );
            }
        }
        TypeDiagnosticReason::ConstructorPatternRequiresEnumOrResultRhs => {
            if let DiagnosticData::Pattern(value) = &input.data {
                return format!(
                    "Constructor pattern requires an enum or Result RHS, got {}",
                    value
                        .actual_type
                        .as_deref()
                        .expect("constructor pattern RHS type")
                );
            }
        }
        TypeDiagnosticReason::ExtractorInputTypeMismatch => {
            if let DiagnosticData::Pattern(value) = &input.data {
                return format!(
                    "Extractor {} expects {}, got {}",
                    value.name.as_deref().expect("extractor name"),
                    value
                        .expected_type
                        .as_deref()
                        .expect("extractor expected type"),
                    value.actual_type.as_deref().expect("extractor actual type")
                );
            }
        }
        TypeDiagnosticReason::ExtractorArityMismatch => {
            if let DiagnosticData::Pattern(value) = &input.data {
                return format!(
                    "Extractor {} returns {} value(s), but pattern expects {}",
                    value.name.as_deref().expect("extractor name"),
                    value.expected_count.expect("extractor success arity"),
                    value.actual_count.expect("pattern arity")
                );
            }
        }
        TypeDiagnosticReason::NonExhaustiveMatch => {
            if let DiagnosticData::Pattern(value) = &input.data {
                return format!(
                    "Non-exhaustive match. Missing: {}",
                    value.details.join(", ")
                );
            }
        }
        TypeDiagnosticReason::SafeBindErrorTypeMismatch => {
            if let DiagnosticData::Policy(value) = &input.data {
                return format!(
                    "`=?` error type mismatch: function returns {}, but expression returns {}",
                    value
                        .expected_type
                        .as_deref()
                        .expect("failure target error type"),
                    value.actual_type.as_deref().expect("propagated error type")
                );
            }
        }
        TypeDiagnosticReason::SafeBindRequiresResultTarget => {
            if let DiagnosticData::Policy(value) = &input.data {
                return format!(
                    "`=?` requires an enclosing ResultContext return type (canonical Result or a valid @result_effect carrier), got {}",
                    value
                        .actual_type
                        .as_deref()
                        .expect("SafeBind enclosing return type")
                );
            }
        }
        TypeDiagnosticReason::ErrorValueMustBeWrapped => {
            return "Error values must be wrapped with Err(...)".into();
        }
        TypeDiagnosticReason::FacetSafeBindForbidden => {
            return "Facet values cannot be bound with `=?`".into();
        }
        TypeDiagnosticReason::FacetPatternBindingForbidden => {
            return "Facet values can only be bound to variables or `_` patterns".into();
        }
        TypeDiagnosticReason::FacetCompileTimeOnly => {
            if let DiagnosticData::Policy(value) = &input.data {
                return value.subject.clone().expect("Facet policy subject");
            }
        }
        TypeDiagnosticReason::FacetOperationPolicyViolation => {
            if let DiagnosticData::Policy(value) = &input.data {
                return value.subject.clone().expect("Facet operation policy");
            }
        }
        TypeDiagnosticReason::ProcessHandlerScope => {
            if let DiagnosticData::Policy(value) = &input.data {
                return value.subject.clone().unwrap_or_else(|| {
                    "Process::self() is only available inside process handlers".into()
                });
            }
        }
        TypeDiagnosticReason::ProcessPolicyViolation => {
            if let DiagnosticData::Policy(value) = &input.data {
                return value.subject.clone().expect("process policy subject");
            }
        }
        TypeDiagnosticReason::SourcePolicyViolation => {
            if let DiagnosticData::Policy(value) = &input.data {
                let subject = value.subject.as_deref().expect("source policy operation");
                return match value.policy {
                    TypePolicy::EntrypointRequirement => format!(
                        "{} is only allowed inside entrypoint `{}` (policy: {})",
                        subject,
                        value.entrypoint.as_deref().expect("configured entrypoint"),
                        value.stage.as_deref().expect("entrypoint-only policy")
                    ),
                    TypePolicy::SourceExitCode => format!(
                        "{} is forbidden by source policy ({})",
                        subject,
                        value.stage.as_deref().expect("source policy mode")
                    ),
                    _ => format!("{} is forbidden by source policy", subject),
                };
            }
        }
        TypeDiagnosticReason::CompilePolicyViolation => {
            if let DiagnosticData::Policy(value) = &input.data {
                let subject = value.subject.as_deref().expect("compile policy subject");
                return match value.policy {
                    TypePolicy::EntrypointRequirement => {
                        "set_exit_code requires a normalized entrypoint but none was provided"
                            .into()
                    }
                    TypePolicy::CompileUnitAvailability | TypePolicy::ProcessCapabilityPolicy => {
                        subject.to_string()
                    }
                    _ => format!("{} is not allowed in this compile unit", subject),
                };
            }
        }
        TypeDiagnosticReason::NominalDeclarationConstraintViolation => {
            if let DiagnosticData::Policy(value) = &input.data {
                return format!(
                    "Type argument {} for {} does not satisfy declaration constraint {} on {}",
                    value.actual_type.as_deref().expect("nominal type argument"),
                    value.subject.as_deref().expect("nominal type parameter"),
                    value
                        .expected_type
                        .as_deref()
                        .expect("nominal declaration bound"),
                    value.stage.as_deref().expect("nominal type name")
                );
            }
        }
        TypeDiagnosticReason::InvalidResultEffectAnnotation => {
            if let DiagnosticData::Policy(value) = &input.data {
                return value
                    .subject
                    .clone()
                    .expect("result effect annotation failure");
            }
        }
        TypeDiagnosticReason::TraitHelperCaptureNeedsExpectedType => {
            if let DiagnosticData::Policy(value) = &input.data {
                return value.subject.as_deref().map_or_else(
                    || {
                        "Trait helper capture needs expected callable type or same-expression inference evidence"
                            .into()
                    },
                    |helper| {
                        format!(
                            "Trait helper `{helper}` needs expected callable type or same-expression inference evidence"
                        )
                    },
                );
            }
        }
        TypeDiagnosticReason::ReservedIntrinsicMarkerUsage => {
            if let DiagnosticData::Policy(value) = &input.data {
                let marker = value.subject.as_deref().unwrap_or("intrinsic marker");
                let intrinsic = value.entrypoint.as_deref().unwrap_or("intrinsic");
                return format!(
                    "`{marker}` is reserved for the compiler-owned `{intrinsic}` signature"
                );
            }
        }
        TypeDiagnosticReason::TypecheckInvariantViolation => {
            return "typechecker failed to provide structured diagnostic data".into();
        }
        _ => {}
    }
    match &input.data {
        DiagnosticData::TraitMethodConstraint(value) => format!(
            "Trait impl method `{}` has incompatible trait constraints",
            value.method_name
        ),
        DiagnosticData::TraitMethodTypeList(value) => match reason {
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
            match reason {
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
        DiagnosticData::ArgumentContract(value) => match reason {
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
            let label = match reason {
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
            let prefix = match reason {
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
            match reason {
                TypeDiagnosticReason::NoApplicableTraitImplementation => format!("No implementation satisfies {} for {}", obligation, value.subject_type.as_deref().expect("concrete obligation has a subject")),
                TypeDiagnosticReason::UnresolvedTraitMethodInstantiation => format!("{}::{} requires a concrete method instantiation", obligation, value.method.as_deref().expect("method instantiation has a method")),
                TypeDiagnosticReason::MissingTraitDispatchTarget => format!("{}::{} has no concrete dispatch target", obligation, value.method.as_deref().expect("method instantiation has a method")),
                _ => unreachable!("dispatch diagnostic requires dispatch reason"),
            }
        },
        DiagnosticData::TypeConstructorCarrier(value) => match reason {
            TypeDiagnosticReason::TypePayloadMismatch => format!("Type payload mismatch: expected {}, got {}", value.expected_carrier, value.actual_carrier),
            TypeDiagnosticReason::TypeConstructorFamilyMismatch => format!("Type constructor family mismatch: expected {}, got {}", value.expected_carrier, value.actual_carrier),
            TypeDiagnosticReason::MissingTypeConstructorCapability => format!("Type constructor occurrence requires {}", value.family),
            _ => unreachable!("carrier diagnostic requires carrier reason"),
        }
        DiagnosticData::BranchAssertion(value) => match reason {
            TypeDiagnosticReason::IfBranchTypeMismatch => format!("if branches have different types: {} and {}", value.expected_type, value.actual_type),
            TypeDiagnosticReason::MatchArmTypeMismatch => format!("Match arm type mismatch: expected {}, got {}", value.expected_type, value.actual_type),
            TypeDiagnosticReason::CondBranchTypeMismatch => format!("cond branches have different types: {} and {}", value.expected_type, value.actual_type),
            _ => unreachable!("branch assertions require a branch reason"),
        },
        _ => match reason {
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
