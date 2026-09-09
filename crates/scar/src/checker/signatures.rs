//! Canonical callable-signature adapters.
//!
//! User functions, trait helpers, and builtin surface declarations all enter
//! the checker through this role-bearing shape.

use super::Checker;
use crate::error::TypeError;
use crate::types::Ty;
use diagnostics::{
    CallableSignatureData, ConstraintSubjectData, DiagnosticData, DiagnosticOrigin, Remediation,
    ReturnTypeArgumentData, SourceFact, SourceId, SourceRole, StructuredDiagnostic,
    TypeDiagnosticReason,
};
use sigil::resolved::{
    ResolvedReturnTypeArgument, ResolvedSignatureTy, ResolvedValueParameter, ResolvedWhereClause,
    ResolvedWhereConstraintRhs,
};
use sindr::builtin::{
    builtin_meta_by_id, builtin_surface_variant_for_decl, BuiltinSurfaceSignatureMeta,
    BuiltinTraitMethodMeta,
};
use sindr::signature::{
    CallableDeclarationKind, CallableIdentity, CallableSignature, CanonicalConstraint,
    CanonicalConstraintSet, CanonicalReturnTypeArgument, CanonicalTypeOccurrence,
    CanonicalValueParameter, RuntimeTarget, SignatureOrigin,
    ValueParameterMode as CanonicalValueParameterMode,
};
use spire::ast::ValueParameterMode;
use spire::ast::{AstTy, Span};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TypeInputId {
    Named(String),
    ConstructorTrait(u32),
}

#[derive(Debug, Clone)]
pub(super) struct SourceOrigin {
    pub(super) span: Span,
    pub(super) display_name: String,
}

#[derive(Debug, Default)]
pub(super) struct SignatureOccurrences {
    pub(super) argument_inputs: BTreeMap<TypeInputId, Vec<SourceOrigin>>,
    pub(super) return_inputs: BTreeMap<TypeInputId, Vec<SourceOrigin>>,
    pub(super) declared_return_type_arguments: BTreeMap<TypeInputId, SourceOrigin>,
}

#[derive(Debug, Default)]
pub(super) struct DirectConstructorInputs {
    // Only ReturnTypeArgument declarations establish an anonymous direct
    // constructor identity that another signature position may reuse. Direct
    // value parameters and direct returns are independent occurrences.
    witnesses: HashMap<String, Ty>,
}

#[derive(Debug, Clone)]
pub(super) enum TypeConstraint {
    Infer { span: Span },
    Explicit { ty: Ty, span: Span },
}

impl TypeConstraint {
    pub(super) fn span(&self) -> &Span {
        match self {
            Self::Infer { span } | Self::Explicit { span, .. } => span,
        }
    }

    pub(super) fn explicit_ty(&self) -> Option<&Ty> {
        match self {
            Self::Explicit { ty, .. } => Some(ty),
            Self::Infer { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ValueArgumentConstraint {
    pub(super) ordinal: u32,
    pub(super) expected: Ty,
    pub(super) actual: Ty,
    pub(super) span: Span,
}

#[derive(Debug, Clone)]
pub(super) enum ConstraintOrigin {
    ReturnTypeArgument(Span),
    ValueArgument(Span),
    ExpectedReturn(Span),
    Obligation(SignatureOrigin),
}

impl ConstraintOrigin {
    pub(super) fn span(&self) -> Option<&Span> {
        match self {
            Self::ReturnTypeArgument(span)
            | Self::ValueArgument(span)
            | Self::ExpectedReturn(span) => Some(span),
            Self::Obligation(_) => None,
        }
    }

    pub(super) fn signature_origin(&self) -> Option<&SignatureOrigin> {
        match self {
            Self::Obligation(origin) => Some(origin),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct CallConstraintSet {
    pub(super) signature: CallableSignature<Ty>,
    pub(super) substitution: Vec<(u32, Ty)>,
    pub(super) return_type_arguments: Vec<TypeConstraint>,
    pub(super) value_arguments: Vec<ValueArgumentConstraint>,
    pub(super) expected_return: Option<TypeConstraint>,
    pub(super) obligations: Vec<CanonicalConstraint<Ty>>,
    pub(super) origins: Vec<ConstraintOrigin>,
}

#[derive(Debug, Clone)]
pub(super) struct PendingConstraints {
    pub(super) unresolved_return_type_arguments: Vec<(u32, Ty)>,
    pub(super) substitution: Vec<(u32, Ty)>,
}

#[derive(Debug)]
pub(super) enum SolveState<T> {
    Solved(T),
    Deferred(PendingConstraints),
    Failed(TypeError),
}

pub(super) fn return_type_argument_arity_error(
    callable: &str,
    expected: usize,
    actual: usize,
    span: &Span,
) -> TypeError {
    TypeError {
        message: format!("{callable} expects {expected} return type argument(s), got {actual}"),
        span: span.clone(),
        hint: None,
        structured: Some(StructuredDiagnostic {
            reason: TypeDiagnosticReason::ReturnTypeArgumentArityMismatch,
            origin: DiagnosticOrigin::Call,
            data: DiagnosticData::ReturnTypeArgument(ReturnTypeArgumentData {
                callable: callable.into(),
                ordinal: None,
                expected_type: None,
                actual_type: None,
                expected_count: Some(expected as u32),
                actual_count: Some(actual as u32),
                declared_origin: None,
                value_parameter_origin: None,
                return_origin: None,
                left_origin: None,
                right_origin: Some(SourceFact::untyped(
                    SourceRole::CallTarget,
                    SourceId(0),
                    span.clone(),
                )),
                required_trait: None,
            }),
            primary: SourceFact::untyped(SourceRole::CallTarget, SourceId(0), span.clone()),
            related: Vec::new(),
            remediation: None,
        }),
    }
}

pub(super) fn return_type_argument_mismatch_error(
    callable: &str,
    ordinal: u32,
    expected: &str,
    actual: &str,
    explicit_span: &Span,
    related_role: SourceRole,
    related_span: &Span,
) -> TypeError {
    TypeError {
        message: format!(
            "return type argument {} for `{}` does not match: expected {}, got {}",
            ordinal, callable, expected, actual
        ),
        span: explicit_span.clone(),
        hint: None,
        structured: Some(StructuredDiagnostic {
            reason: TypeDiagnosticReason::ReturnTypeArgumentMismatch,
            origin: DiagnosticOrigin::ReturnTypeArgument { ordinal },
            data: DiagnosticData::ReturnTypeArgument(ReturnTypeArgumentData {
                declared_origin: (related_role == SourceRole::Declaration).then(|| {
                    SourceFact::typed(related_role, SourceId(0), related_span.clone(), expected)
                }),
                value_parameter_origin: (related_role == SourceRole::Value).then(|| {
                    SourceFact::typed(related_role, SourceId(0), related_span.clone(), expected)
                }),
                return_origin: (related_role == SourceRole::Expected).then(|| {
                    SourceFact::typed(related_role, SourceId(0), related_span.clone(), expected)
                }),
                left_origin: Some(SourceFact::typed(
                    related_role,
                    SourceId(0),
                    related_span.clone(),
                    expected,
                )),
                right_origin: Some(SourceFact::typed(
                    SourceRole::ReturnTypeArgument,
                    SourceId(0),
                    explicit_span.clone(),
                    actual,
                )),
                required_trait: None,
                expected_count: None,
                actual_count: None,
                callable: callable.into(),
                ordinal: Some(ordinal),
                expected_type: Some(expected.into()),
                actual_type: Some(actual.into()),
            }),
            primary: SourceFact::typed(
                SourceRole::ReturnTypeArgument,
                SourceId(0),
                explicit_span.clone(),
                actual,
            ),
            related: vec![SourceFact::typed(
                related_role,
                SourceId(0),
                related_span.clone(),
                expected,
            )],
            remediation: None,
        }),
    }
}

fn type_input_id(name: &str) -> Option<TypeInputId> {
    (name == "Self" || name.starts_with('$')).then(|| TypeInputId::Named(name.to_string()))
}

fn collect_type_inputs(ty: &AstTy, inputs: &mut BTreeMap<TypeInputId, Vec<SourceOrigin>>) {
    match ty {
        AstTy::Named(span, name) => {
            if let Some(id) = type_input_id(name) {
                inputs.entry(id).or_default().push(SourceOrigin {
                    span: span.clone(),
                    display_name: name.clone(),
                });
            }
        }
        AstTy::Generic(span, name, arguments) => {
            if let Some(id) = type_input_id(name) {
                inputs.entry(id).or_default().push(SourceOrigin {
                    span: span.clone(),
                    display_name: name.clone(),
                });
            }
            for argument in arguments {
                collect_type_inputs(argument, inputs);
            }
        }
        AstTy::Tuple(_, items) => {
            for item in items {
                collect_type_inputs(item, inputs);
            }
        }
        AstTy::Func(_, parameters, return_type) => {
            for parameter in parameters {
                collect_type_inputs(parameter, inputs);
            }
            collect_type_inputs(return_type, inputs);
        }
        AstTy::ImplTrait(_, _) => {}
    }
}

fn direct_signature_input(signature_ty: &ResolvedSignatureTy) -> Option<(TypeInputId, String)> {
    if let Some(trait_id) = &signature_ty.direct_constructor_trait {
        return Some((
            TypeInputId::ConstructorTrait(trait_id.unique_id),
            trait_id.name.clone(),
        ));
    }
    match signature_ty.syntax() {
        AstTy::Named(_, name) | AstTy::Generic(_, name, _) => {
            type_input_id(name).map(|id| (id, name.clone()))
        }
        AstTy::ImplTrait(_, _) | AstTy::Tuple(_, _) | AstTy::Func(_, _, _) => None,
    }
}

fn collect_signature_type_inputs(
    signature_ty: &ResolvedSignatureTy,
    inputs: &mut BTreeMap<TypeInputId, Vec<SourceOrigin>>,
) {
    if let Some(trait_id) = &signature_ty.direct_constructor_trait {
        inputs
            .entry(TypeInputId::ConstructorTrait(trait_id.unique_id))
            .or_default()
            .push(SourceOrigin {
                span: Checker::ast_ty_span(signature_ty.syntax()).clone(),
                display_name: trait_id.name.clone(),
            });
    }
    // The direct constructor identity is carried separately. Syntax
    // recursion collects only named `Self` / `$T` inputs from its slots and
    // nested ordinary types; it never rediscovers Traits by display name.
    collect_type_inputs(signature_ty.syntax(), inputs);
}

pub(super) fn signature_occurrences(
    return_type_arguments: &[ResolvedReturnTypeArgument],
    value_parameters: &[ResolvedValueParameter],
    return_type: Option<&ResolvedSignatureTy>,
) -> SignatureOccurrences {
    let mut occurrences = SignatureOccurrences::default();
    for parameter in value_parameters {
        collect_signature_type_inputs(&parameter.ty, &mut occurrences.argument_inputs);
    }
    if let Some(return_type) = return_type {
        collect_signature_type_inputs(return_type, &mut occurrences.return_inputs);
    }
    for argument in return_type_arguments {
        if let Some((id, display_name)) = direct_signature_input(&argument.ty) {
            occurrences.declared_return_type_arguments.insert(
                id,
                SourceOrigin {
                    span: argument.span.clone(),
                    display_name,
                },
            );
        }
    }
    occurrences
}

fn source_fact(role: SourceRole, span: Span, ty: &str) -> SourceFact {
    SourceFact::typed(role, SourceId(0), span, ty)
}

fn occurrence_error(
    reason: TypeDiagnosticReason,
    callable: &str,
    input: &str,
    ordinal: u32,
    primary: SourceFact,
    value_parameter_origin: Option<SourceFact>,
    message: String,
    help: &str,
) -> crate::error::TypeError {
    let span = primary.span.clone();
    crate::error::TypeError {
        message,
        span,
        hint: Some(help.into()),
        structured: Some(StructuredDiagnostic {
            reason,
            origin: DiagnosticOrigin::ReturnTypeArgument { ordinal },
            data: DiagnosticData::ReturnTypeArgument(ReturnTypeArgumentData {
                declared_origin: (reason != TypeDiagnosticReason::MissingReturnTypeArgument)
                    .then(|| primary.clone()),
                value_parameter_origin: value_parameter_origin.clone(),
                return_origin: (reason == TypeDiagnosticReason::MissingReturnTypeArgument)
                    .then(|| primary.clone()),
                left_origin: None,
                right_origin: Some(primary.clone()),
                required_trait: None,
                expected_count: None,
                actual_count: None,
                callable: callable.into(),
                ordinal: (reason != TypeDiagnosticReason::MissingReturnTypeArgument)
                    .then_some(ordinal),
                expected_type: Some(input.into()),
                actual_type: None,
            }),
            primary,
            related: value_parameter_origin.into_iter().collect(),
            remediation: Some(Remediation::Help { text: help.into() }),
        }),
    }
}

fn duplicate_return_type_argument_error(
    callable: &str,
    input: &str,
    ordinal: u32,
    current: SourceFact,
    previous: SourceFact,
) -> crate::error::TypeError {
    let message = format!("return type argument `{input}` is introduced more than once");
    let help = format!("remove the duplicate `{input}` return type argument");
    crate::error::TypeError {
        message,
        span: current.span.clone(),
        hint: Some(help.clone()),
        structured: Some(StructuredDiagnostic {
            reason: TypeDiagnosticReason::DuplicateReturnTypeArgumentInput,
            origin: DiagnosticOrigin::ReturnTypeArgument { ordinal },
            data: DiagnosticData::ReturnTypeArgument(ReturnTypeArgumentData {
                declared_origin: Some(current.clone()),
                value_parameter_origin: None,
                return_origin: None,
                left_origin: Some(previous.clone()),
                right_origin: Some(current.clone()),
                required_trait: None,
                expected_count: None,
                actual_count: None,
                callable: callable.into(),
                ordinal: Some(ordinal),
                expected_type: Some(input.into()),
                actual_type: None,
            }),
            primary: current,
            related: vec![previous],
            remediation: Some(Remediation::Help { text: help }),
        }),
    }
}

pub(super) fn validate_return_type_argument_definition(
    callable: &str,
    return_type_arguments: &[ResolvedReturnTypeArgument],
    value_parameters: &[ResolvedValueParameter],
    return_type: Option<&ResolvedSignatureTy>,
) -> Result<(), crate::error::TypeError> {
    let occurrences = signature_occurrences(return_type_arguments, value_parameters, return_type);
    let mut declared_inputs = BTreeMap::new();

    for (ordinal, argument) in return_type_arguments.iter().enumerate() {
        let Some((input, name)) = direct_signature_input(&argument.ty) else {
            continue;
        };
        let origin = SourceOrigin {
            span: argument.span.clone(),
            display_name: name.clone(),
        };
        if let Some(previous) = declared_inputs.insert(input.clone(), origin) {
            return Err(duplicate_return_type_argument_error(
                callable,
                &name,
                ordinal as u32,
                source_fact(SourceRole::ReturnTypeArgument, argument.span.clone(), &name),
                source_fact(
                    SourceRole::ReturnTypeArgument,
                    previous.span,
                    &previous.display_name,
                ),
            ));
        }
        if !matches!(input, TypeInputId::ConstructorTrait(_)) {
            if let Some(argument_origins) = occurrences.argument_inputs.get(&input) {
                let related = argument_origins.first().map(|origin| {
                    source_fact(SourceRole::Value, origin.span.clone(), &origin.display_name)
                });
                return Err(occurrence_error(
                    TypeDiagnosticReason::DuplicateReturnTypeArgumentInput,
                    callable,
                    &name,
                    ordinal as u32,
                    source_fact(SourceRole::ReturnTypeArgument, argument.span.clone(), &name),
                    related,
                    format!("type input `{name}` is introduced more than once"),
                    &format!("remove `{name}` from the return type arguments"),
                ));
            }
        }
        if !occurrences.return_inputs.contains_key(&input) {
            return Err(occurrence_error(
                TypeDiagnosticReason::UnusedReturnTypeArgument,
                callable,
                &name,
                ordinal as u32,
                source_fact(SourceRole::ReturnTypeArgument, argument.span.clone(), &name),
                None,
                format!("return type argument `{name}` does not appear in the return type"),
                "remove the unused return type argument or use it in the return type",
            ));
        }
    }

    for (input, origins) in &occurrences.return_inputs {
        // A direct TypeCtorTrait return is an anonymous result carrier chosen
        // by the function body. It is not a named input introduced by a direct
        // value parameter and therefore needs no ReturnTypeArgument unless the
        // declaration explicitly exposes one.
        if matches!(input, TypeInputId::ConstructorTrait(_)) {
            continue;
        }
        if occurrences.argument_inputs.contains_key(input)
            || occurrences
                .declared_return_type_arguments
                .contains_key(input)
        {
            continue;
        }
        let origin = origins
            .first()
            .expect("a collected type input always has an origin");
        let name = &origin.display_name;
        return Err(occurrence_error(
            TypeDiagnosticReason::MissingReturnTypeArgument,
            callable,
            name,
            0,
            source_fact(SourceRole::Expected, origin.span.clone(), name),
            None,
            format!("return-only type input `{name}` is not declared"),
            &format!(
                "declare `{name}` in the return type argument list: `def {}::<{name}>(...)`",
                callable.rsplit("::").next().expect("callable surface name")
            ),
        ));
    }
    Ok(())
}

fn collect_constructor_variable_applications(ty: &AstTy, out: &mut Vec<(String, Span)>) {
    match ty {
        AstTy::Generic(span, name, arguments) => {
            if name.starts_with('$') {
                out.push((name.clone(), span.clone()));
            }
            for argument in arguments {
                collect_constructor_variable_applications(argument, out);
            }
        }
        AstTy::Tuple(_, items) => {
            for item in items {
                collect_constructor_variable_applications(item, out);
            }
        }
        AstTy::Func(_, parameters, return_type) => {
            for parameter in parameters {
                collect_constructor_variable_applications(parameter, out);
            }
            collect_constructor_variable_applications(return_type, out);
        }
        AstTy::Named(_, _) | AstTy::ImplTrait(_, _) => {}
    }
}

pub(super) fn validate_constructor_variable_constraints(
    _callable: &str,
    return_type_arguments: &[ResolvedReturnTypeArgument],
    value_parameters: &[ResolvedValueParameter],
    return_type: Option<&AstTy>,
    where_clause: Option<&ResolvedWhereClause>,
    constructor_trait_ids: &HashSet<u32>,
) -> Result<(), crate::error::TypeError> {
    let constrained = where_clause
        .into_iter()
        .flat_map(|clause| clause.constraints.iter())
        .filter_map(|constraint| match &constraint.subject {
            AstTy::Named(_, name)
                if name.starts_with('$')
                    && constraint.bounds.iter().any(|bound| {
                        matches!(bound, ResolvedWhereConstraintRhs::Trait { trait_id }
                            if constructor_trait_ids.contains(&trait_id.unique_id))
                    }) =>
            {
                Some(name.clone())
            }
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut applications = Vec::new();
    for argument in return_type_arguments {
        collect_constructor_variable_applications(&argument.ty, &mut applications);
    }
    for parameter in value_parameters {
        collect_constructor_variable_applications(&parameter.ty, &mut applications);
    }
    if let Some(return_type) = return_type {
        collect_constructor_variable_applications(return_type, &mut applications);
    }
    let Some((name, span)) = applications
        .into_iter()
        .find(|(name, _)| !constrained.contains(name))
    else {
        return Ok(());
    };
    let message = format!("type constructor variable `{name}` requires a TypeCtorTrait constraint");
    let help = format!("add a TypeCtorTrait constraint such as `where {name}: Functor`");
    Err(crate::error::TypeError {
        message,
        span: span.clone(),
        hint: Some(help.clone()),
        structured: Some(StructuredDiagnostic {
            reason: TypeDiagnosticReason::MissingTypeConstructorConstraint,
            origin: DiagnosticOrigin::Declaration,
            data: DiagnosticData::ConstraintSubject(ConstraintSubjectData {
                subject_origin: Some(source_fact(SourceRole::Declaration, span.clone(), &name)),
                required_trait: Some("TypeCtorTrait".into()),
                suggested_type_variable: Some(name.clone()),
                subject: name.clone(),
                constraint: "TypeCtorTrait".into(),
            }),
            primary: source_fact(SourceRole::Declaration, span, &name),
            related: Vec::new(),
            remediation: Some(Remediation::Help { text: help }),
        }),
    })
}

pub(super) fn invalid_trait_constraint_subject_error(
    subject: &str,
    span: Span,
) -> crate::error::TypeError {
    let message = format!("trait `{subject}` cannot be used as a constraint subject");
    let help = format!("introduce a type variable and write `where $F: {subject} + RequiredTrait`");
    crate::error::TypeError {
        message,
        span: span.clone(),
        hint: Some(help.clone()),
        structured: Some(StructuredDiagnostic {
            reason: TypeDiagnosticReason::InvalidTraitConstraintSubject,
            origin: DiagnosticOrigin::Declaration,
            data: DiagnosticData::ConstraintSubject(ConstraintSubjectData {
                subject_origin: Some(source_fact(SourceRole::Trait, span.clone(), subject)),
                required_trait: None,
                suggested_type_variable: Some("$F".into()),
                subject: subject.into(),
                constraint: "trait constraint subject".into(),
            }),
            primary: source_fact(SourceRole::Trait, span, subject),
            related: Vec::new(),
            remediation: Some(Remediation::Help { text: help }),
        }),
    }
}

pub(super) fn remember_direct_constructor_input(
    checker: &Checker,
    signature_ty: &ResolvedSignatureTy,
    resolved: &Ty,
    inputs: &mut DirectConstructorInputs,
) {
    let Some(trait_key) = checker.constructor_trait_key_for_signature_ty(signature_ty) else {
        return;
    };
    let Ty::SelfApp(items) = resolved else {
        return;
    };
    let Some((witness, _)) = Checker::constructor_application_parts(items) else {
        return;
    };
    inputs
        .witnesses
        .entry(trait_key)
        .or_insert_with(|| witness.clone());
}

pub(super) fn coalesce_direct_constructor_inputs(
    checker: &mut Checker,
    ty: Ty,
    inputs: &DirectConstructorInputs,
) -> Ty {
    match ty {
        Ty::SelfApp(mut items) => {
            if let Some((Ty::Var(var), _)) = Checker::constructor_application_parts(&items) {
                if let Some(trait_key) = checker.constructor_witness_traits.get(var) {
                    if let Some(shared) = inputs.witnesses.get(trait_key) {
                        if let Ty::Var(shared) = shared {
                            if shared != var {
                                items[1] = Ty::Var(*shared);
                            }
                        }
                    }
                }
            }
            Ty::SelfApp(
                items
                    .into_iter()
                    .map(|item| coalesce_direct_constructor_inputs(checker, item, inputs))
                    .collect(),
            )
        }
        Ty::List(inner) => Ty::List(Box::new(coalesce_direct_constructor_inputs(
            checker, *inner, inputs,
        ))),
        Ty::Tuple(items) => Ty::Tuple(
            items
                .into_iter()
                .map(|item| coalesce_direct_constructor_inputs(checker, item, inputs))
                .collect(),
        ),
        Ty::Func(parameters, return_type) => Ty::Func(
            parameters
                .into_iter()
                .map(|parameter| coalesce_direct_constructor_inputs(checker, parameter, inputs))
                .collect(),
            Box::new(coalesce_direct_constructor_inputs(
                checker,
                *return_type,
                inputs,
            )),
        ),
        Ty::Lazy(inner) => Ty::Lazy(Box::new(coalesce_direct_constructor_inputs(
            checker, *inner, inputs,
        ))),
        Ty::Facet(kind, source, focus, update_source, update_focus) => Ty::Facet(
            kind,
            Box::new(coalesce_direct_constructor_inputs(checker, *source, inputs)),
            Box::new(coalesce_direct_constructor_inputs(checker, *focus, inputs)),
            Box::new(coalesce_direct_constructor_inputs(
                checker,
                *update_source,
                inputs,
            )),
            Box::new(coalesce_direct_constructor_inputs(
                checker,
                *update_focus,
                inputs,
            )),
        ),
        Ty::BuiltinFunc { name, params, ret } => Ty::BuiltinFunc {
            name,
            params: params
                .into_iter()
                .map(|parameter| coalesce_direct_constructor_inputs(checker, parameter, inputs))
                .collect(),
            ret: Box::new(coalesce_direct_constructor_inputs(checker, *ret, inputs)),
        },
        Ty::UserFunc {
            fun_idx,
            type_params,
            call_substitution,
            params,
            ret,
        } => Ty::UserFunc {
            fun_idx,
            type_params,
            call_substitution: call_substitution
                .into_iter()
                .map(|(var, ty)| (var, coalesce_direct_constructor_inputs(checker, ty, inputs)))
                .collect(),
            params: params
                .into_iter()
                .map(|parameter| coalesce_direct_constructor_inputs(checker, parameter, inputs))
                .collect(),
            ret: Box::new(coalesce_direct_constructor_inputs(checker, *ret, inputs)),
        },
        Ty::Struct(name, fields) => Ty::Struct(
            name,
            fields.map_types(|ty| coalesce_direct_constructor_inputs(checker, ty.clone(), inputs)),
        ),
        Ty::Record(name, fields) => Ty::Record(
            name,
            fields.map_types(|ty| coalesce_direct_constructor_inputs(checker, ty.clone(), inputs)),
        ),
        Ty::Enum(name, arguments) => Ty::Enum(
            name,
            arguments
                .into_iter()
                .map(|argument| coalesce_direct_constructor_inputs(checker, argument, inputs))
                .collect(),
        ),
        Ty::Result(ok, error) => Ty::Result(
            Box::new(coalesce_direct_constructor_inputs(checker, *ok, inputs)),
            Box::new(coalesce_direct_constructor_inputs(checker, *error, inputs)),
        ),
        other => other,
    }
}

pub(super) fn canonical_callable_signature(
    id: &sigil::resolved::ResolvedId,
    return_type_arguments: &[ResolvedReturnTypeArgument],
    value_parameters: &[ResolvedValueParameter],
    return_type_arguments_tys: &[Ty],
    value_parameter_tys: &[Ty],
    return_ty: Ty,
    where_constraints: CanonicalConstraintSet<Ty>,
    runtime_target: RuntimeTarget,
    declaration_kind: CallableDeclarationKind,
) -> Result<CallableSignature<Ty>, TypeError> {
    validate_canonical_role_list(
        id,
        "return type argument",
        return_type_arguments.len(),
        return_type_arguments_tys.len(),
    )?;
    validate_canonical_role_list(
        id,
        "value parameter",
        value_parameters.len(),
        value_parameter_tys.len(),
    )?;
    validate_return_type_argument_ordinals(
        id,
        return_type_arguments
            .iter()
            .map(|argument| argument.ordinal),
    )?;

    let origin = |span: &spire::ast::Span, role: &str, ordinal: usize| {
        SignatureOrigin::new(format!("{role} {ordinal} at {}..{}", span.start, span.end))
    };
    let return_type_arguments = return_type_arguments
        .iter()
        .zip(return_type_arguments_tys.iter())
        .map(|(argument, ty)| CanonicalReturnTypeArgument {
            ordinal: argument.ordinal,
            ty: ty.clone(),
            origin: origin(
                &argument.span,
                "return type argument",
                argument.ordinal as usize,
            ),
        })
        .collect();
    let value_parameters = value_parameters
        .iter()
        .zip(value_parameter_tys.iter())
        .enumerate()
        .map(|(ordinal, (parameter, ty))| CanonicalValueParameter {
            ordinal: ordinal as u32,
            name: parameter.id.name.clone(),
            mode: canonical_parameter_mode(parameter.mode),
            ty: ty.clone(),
            origin: origin(&parameter.span, "value parameter", ordinal),
        })
        .collect();
    Ok(CallableSignature {
        identity: CallableIdentity {
            owner: id
                .qualified_name
                .as_deref()
                .and_then(|name| name.rsplit_once("::").map(|(owner, _)| owner.to_string())),
            name: id.name.clone(),
            declaration_kind,
        },
        return_type_arguments,
        value_parameters,
        return_type: CanonicalTypeOccurrence {
            ty: return_ty,
            origin: SignatureOrigin::new(format!(
                "return type at {}..{}",
                id.span.start, id.span.end
            )),
        },
        where_constraints,
        runtime_target,
        declaration_origins: vec![SignatureOrigin::new(format!("declaration {}", id.name))],
    })
}

fn validate_return_type_argument_ordinals(
    id: &sigil::resolved::ResolvedId,
    ordinals: impl IntoIterator<Item = u32>,
) -> Result<(), TypeError> {
    for (expected, ordinal) in ordinals.into_iter().enumerate() {
        if ordinal != expected as u32 {
            return Err(canonical_signature_consistency_error(
                id,
                "return type argument",
                None,
                None,
                format!(
                    "return type argument ordinal {} appears at position {expected}",
                    ordinal
                ),
            ));
        }
    }
    Ok(())
}

fn validate_canonical_role_list(
    id: &sigil::resolved::ResolvedId,
    role: &str,
    source_len: usize,
    resolved_len: usize,
) -> Result<(), TypeError> {
    if source_len == resolved_len {
        return Ok(());
    }
    Err(canonical_signature_consistency_error(
        id,
        role,
        u32::try_from(source_len).ok(),
        u32::try_from(resolved_len).ok(),
        format!("{role} count is {source_len} in source but {resolved_len} after resolution"),
    ))
}

fn canonical_signature_consistency_error(
    id: &sigil::resolved::ResolvedId,
    role: &str,
    expected_count: Option<u32>,
    actual_count: Option<u32>,
    detail: String,
) -> TypeError {
    let message = format!(
        "canonical callable signature for `{}` is inconsistent: {detail}",
        id.name
    );
    TypeError {
        message: message.clone(),
        span: id.span.clone(),
        hint: None,
        structured: Some(StructuredDiagnostic {
            reason: TypeDiagnosticReason::CallableSignatureMetadataMismatch,
            origin: DiagnosticOrigin::Declaration,
            data: DiagnosticData::CallableSignature(CallableSignatureData {
                callable: id.name.clone(),
                role: role.into(),
                expected_count,
                actual_count,
                detail,
            }),
            primary: SourceFact::untyped(SourceRole::Declaration, SourceId(0), id.span.clone()),
            related: Vec::new(),
            remediation: None,
        }),
    }
}

pub(super) fn missing_canonical_callable_signature(
    id: &sigil::resolved::ResolvedId,
    span: &Span,
) -> TypeError {
    let detail = "registered callable has no canonical signature".to_string();
    let message = format!(
        "canonical callable signature for `{}` is missing from the registry",
        id.name
    );
    TypeError {
        message,
        span: span.clone(),
        hint: None,
        structured: Some(StructuredDiagnostic {
            reason: TypeDiagnosticReason::CallableSignatureMetadataMismatch,
            origin: DiagnosticOrigin::Call,
            data: DiagnosticData::CallableSignature(CallableSignatureData {
                callable: id.name.clone(),
                role: "registry".into(),
                expected_count: None,
                actual_count: None,
                detail,
            }),
            primary: SourceFact::untyped(SourceRole::CallTarget, SourceId(0), span.clone()),
            related: Vec::new(),
            remediation: None,
        }),
    }
}

pub(super) fn constructor_signature_metadata_error(
    callable: &str,
    span: &Span,
    detail: String,
) -> TypeError {
    let message =
        format!("canonical constructor signature for `{callable}` is inconsistent: {detail}");
    TypeError {
        message,
        span: span.clone(),
        hint: None,
        structured: Some(StructuredDiagnostic {
            reason: TypeDiagnosticReason::CallableSignatureMetadataMismatch,
            origin: DiagnosticOrigin::Declaration,
            data: DiagnosticData::CallableSignature(CallableSignatureData {
                callable: callable.into(),
                role: "constructor_application".into(),
                expected_count: None,
                actual_count: None,
                detail,
            }),
            primary: SourceFact::untyped(SourceRole::Declaration, SourceId(0), span.clone()),
            related: Vec::new(),
            remediation: None,
        }),
    }
}

pub(super) fn canonical_where_constraints(
    checker: &mut Checker,
    where_clause: Option<&ResolvedWhereClause>,
    tyvars: &mut HashMap<String, Ty>,
) -> Result<CanonicalConstraintSet<Ty>, crate::error::TypeError> {
    let mut constraints = Vec::new();
    let Some(where_clause) = where_clause else {
        return Ok(CanonicalConstraintSet { constraints });
    };

    for constraint in &where_clause.constraints {
        let subject = checker.resolve_builtin_ast_ty_in_context(
            &constraint.subject,
            super::TypeSyntaxContext::General,
            tyvars,
        )?;
        for bound in &constraint.bounds {
            let ResolvedWhereConstraintRhs::Trait { trait_id } = bound else {
                // Constructor-slot constraints are consumed by the trait
                // declaration/selection machinery. They are not ordinary
                // callable obligations.
                continue;
            };
            constraints.push(CanonicalConstraint {
                subject: subject.clone(),
                trait_name: checker.trait_key(trait_id),
                origin: SignatureOrigin::new(format!(
                    "where constraint at {}..{}",
                    constraint.span.start, constraint.span.end
                )),
            });
        }
    }

    Ok(CanonicalConstraintSet { constraints })
}

/// Instantiate every type occurrence in a callable with one shared fresh map.
/// This preserves repeated-variable relationships across RTA, value, return,
/// and where positions.
pub(super) fn instantiate_callable_signature(
    checker: &mut Checker,
    signature: &CallableSignature<Ty>,
) -> (CallableSignature<Ty>, Vec<(u32, Ty)>) {
    let mut fresh = HashMap::new();
    let instantiated = CallableSignature {
        identity: signature.identity.clone(),
        return_type_arguments: signature
            .return_type_arguments
            .iter()
            .map(|argument| CanonicalReturnTypeArgument {
                ordinal: argument.ordinal,
                ty: checker.instantiate_ty_with_fresh(&argument.ty, &mut fresh),
                origin: argument.origin.clone(),
            })
            .collect(),
        value_parameters: signature
            .value_parameters
            .iter()
            .map(|parameter| CanonicalValueParameter {
                ordinal: parameter.ordinal,
                name: parameter.name.clone(),
                mode: parameter.mode,
                ty: checker.instantiate_ty_with_fresh(&parameter.ty, &mut fresh),
                origin: parameter.origin.clone(),
            })
            .collect(),
        return_type: CanonicalTypeOccurrence {
            ty: checker.instantiate_ty_with_fresh(&signature.return_type.ty, &mut fresh),
            origin: signature.return_type.origin.clone(),
        },
        where_constraints: CanonicalConstraintSet {
            constraints: signature
                .where_constraints
                .constraints
                .iter()
                .map(|constraint| CanonicalConstraint {
                    subject: checker.instantiate_ty_with_fresh(&constraint.subject, &mut fresh),
                    trait_name: constraint.trait_name.clone(),
                    origin: constraint.origin.clone(),
                })
                .collect(),
        },
        runtime_target: signature.runtime_target.clone(),
        declaration_origins: signature.declaration_origins.clone(),
    };
    let mut substitution = fresh.into_iter().collect::<Vec<_>>();
    substitution.sort_by_key(|(var, _)| *var);
    (instantiated, substitution)
}

fn canonical_parameter_mode(mode: ValueParameterMode) -> CanonicalValueParameterMode {
    match mode {
        ValueParameterMode::PositionalOrNamed => CanonicalValueParameterMode::PositionalOrNamed,
        ValueParameterMode::Variadic => CanonicalValueParameterMode::Variadic,
    }
}

pub(super) fn builtin_surface_signature(
    id: &sigil::resolved::ResolvedId,
) -> Option<BuiltinSurfaceSignatureMeta> {
    builtin_surface_variant_for_decl(&id.name, id.qualified_name.as_deref())
}

/// Validate the source declaration's callable shape against metadata. Explicit
/// metadata parameter names and modes are part of the contract; generated
/// `argN` names denote runtime-only entries whose surface name is supplied by
/// the declaration.
pub(super) fn builtin_surface_matches(
    id: &sigil::resolved::ResolvedId,
    params: &[ResolvedValueParameter],
    ret_ty: Option<&spire::ast::AstTy>,
) -> bool {
    let runtime_name = sindr::builtin::builtin_runtime_name(&id.name, id.qualified_name.as_deref());
    if runtime_name.starts_with("__") {
        return true;
    }
    let Some(variant) = builtin_surface_signature(id) else {
        return false;
    };
    if variant.value_parameters.len() != params.len() {
        return false;
    }
    if variant
        .value_parameters
        .iter()
        .zip(params)
        .any(|(expected, actual)| {
            (!expected.name.starts_with("arg") && expected.name != actual.id.name)
                || expected.mode != canonical_parameter_mode(actual.mode)
        })
    {
        return false;
    }
    let actual_return = ret_ty
        .map(Checker::surface_ast_ty)
        .unwrap_or_else(|| "Unit".to_string());
    let actual_types = params
        .iter()
        .map(|param| Checker::surface_ast_ty(&param.ty))
        .chain(std::iter::once(actual_return))
        .collect::<Vec<_>>();
    let expected_types = variant
        .value_parameters
        .iter()
        .map(|parameter| parameter.ty.clone())
        .chain(std::iter::once(variant.return_type.ty.clone()))
        .collect::<Vec<_>>();
    let mut actual_variables = HashMap::new();
    let mut expected_variables = HashMap::new();
    let actual_types = actual_types
        .iter()
        .map(|ty| normalize_surface_type(ty, &mut actual_variables))
        .collect::<Vec<_>>();
    let expected_types = expected_types
        .iter()
        .map(|ty| normalize_surface_type(ty, &mut expected_variables))
        .collect::<Vec<_>>();
    if actual_types != expected_types {
        return false;
    }
    true
}

/// Validate an `@builtin` Trait implementation against the runtime entry, not
/// merely against its source Trait declaration. Both source declarations can
/// drift together, so the builtin id is attached only after this independent
/// metadata check succeeds.
pub(super) fn builtin_trait_surface_matches(
    checker: &Checker,
    metadata: &BuiltinTraitMethodMeta,
    target: &Ty,
    value_parameters: &[ResolvedValueParameter],
    params: &[Ty],
    ret_ty: &Ty,
) -> bool {
    let Some(runtime) = builtin_meta_by_id(metadata.builtin_id.0) else {
        return false;
    };
    if params.len() != usize::from(runtime.runtime_arity()) {
        return false;
    }
    let Some(signature) = runtime.surface_variants().into_iter().next() else {
        return false;
    };
    if signature.value_parameters.len() != value_parameters.len()
        || signature
            .value_parameters
            .iter()
            .zip(value_parameters)
            .any(|(expected, actual)| expected.mode != canonical_parameter_mode(actual.mode))
    {
        return false;
    }

    let mut env = crate::env::TypeEnv::new();
    let Ty::BuiltinFunc {
        params: expected_params,
        ret: expected_ret,
        ..
    } = super::builtin_ty_from_meta(runtime, &mut env)
    else {
        unreachable!("builtin metadata always parses to a builtin function")
    };
    if expected_params.len() != params.len() {
        return false;
    }

    let Some(actual_receiver) = params.first() else {
        return false;
    };
    let target = checker.resolve_ty(target);
    let mut target_variables = HashMap::new();
    if !builtin_runtime_type_matches(checker, &target, actual_receiver, &mut target_variables) {
        return false;
    }

    let mut expected_variables = HashMap::new();
    expected_params
        .iter()
        .zip(params)
        .all(|(expected, actual)| {
            builtin_runtime_type_matches(checker, expected, actual, &mut expected_variables)
        })
        && builtin_runtime_type_matches(checker, &expected_ret, ret_ty, &mut expected_variables)
}

fn builtin_runtime_type_matches(
    checker: &Checker,
    expected: &Ty,
    actual: &Ty,
    expected_variables: &mut HashMap<u32, Ty>,
) -> bool {
    // Expected metadata uses an isolated TypeEnv, so its variable ids must
    // never be resolved through the active checker's substitution namespace.
    let expected = expected.clone();
    let actual = checker.resolve_ty(actual);
    if matches!(&expected, Ty::Enum(name, arguments) if name == "_" && arguments.is_empty()) {
        return true;
    }
    if let Ty::Var(variable) = expected {
        return match expected_variables.get(&variable) {
            Some(bound) => checker.resolve_ty(bound) == actual,
            None => {
                expected_variables.insert(variable, actual);
                true
            }
        };
    }

    match (expected, actual) {
        (Ty::Int, Ty::Int)
        | (Ty::Float, Ty::Float)
        | (Ty::Str, Ty::Str)
        | (Ty::Bool, Ty::Bool)
        | (Ty::Unit, Ty::Unit)
        | (Ty::Error, Ty::Error)
        | (Ty::Hole, Ty::Hole) => true,
        (Ty::List(expected), Ty::List(actual)) | (Ty::Lazy(expected), Ty::Lazy(actual)) => {
            builtin_runtime_type_matches(checker, &expected, &actual, expected_variables)
        }
        (Ty::Tuple(expected), Ty::Tuple(actual)) => {
            expected.len() == actual.len()
                && expected.iter().zip(&actual).all(|(expected, actual)| {
                    builtin_runtime_type_matches(checker, expected, actual, expected_variables)
                })
        }
        (Ty::Func(expected_params, expected_ret), Ty::Func(actual_params, actual_ret)) => {
            expected_params.len() == actual_params.len()
                && expected_params
                    .iter()
                    .zip(&actual_params)
                    .all(|(expected, actual)| {
                        builtin_runtime_type_matches(checker, expected, actual, expected_variables)
                    })
                && builtin_runtime_type_matches(
                    checker,
                    &expected_ret,
                    &actual_ret,
                    expected_variables,
                )
        }
        (
            Ty::Facet(
                expected_kind,
                expected_source,
                expected_focus,
                expected_update_source,
                expected_update_focus,
            ),
            Ty::Facet(
                actual_kind,
                actual_source,
                actual_focus,
                actual_update_source,
                actual_update_focus,
            ),
        ) => {
            expected_kind == actual_kind
                && builtin_runtime_type_matches(
                    checker,
                    &expected_source,
                    &actual_source,
                    expected_variables,
                )
                && builtin_runtime_type_matches(
                    checker,
                    &expected_focus,
                    &actual_focus,
                    expected_variables,
                )
                && builtin_runtime_type_matches(
                    checker,
                    &expected_update_source,
                    &actual_update_source,
                    expected_variables,
                )
                && builtin_runtime_type_matches(
                    checker,
                    &expected_update_focus,
                    &actual_update_focus,
                    expected_variables,
                )
        }
        (Ty::Pid(expected), Ty::Pid(actual)) => expected == actual,
        (Ty::Struct(expected_name, expected_fields), Ty::Struct(actual_name, actual_fields))
        | (Ty::Record(expected_name, expected_fields), Ty::Record(actual_name, actual_fields)) => {
            Checker::surface_name(&expected_name) == Checker::surface_name(&actual_name)
                && expected_fields.arguments.len() == actual_fields.arguments.len()
                && expected_fields
                    .arguments
                    .iter()
                    .zip(&actual_fields.arguments)
                    .all(|(expected, actual)| {
                        builtin_runtime_type_matches(checker, expected, actual, expected_variables)
                    })
                && expected_fields.len() == actual_fields.len()
                && expected_fields.iter().zip(&actual_fields).all(
                    |((expected_name, expected_ty), (actual_name, actual_ty))| {
                        expected_name == actual_name
                            && builtin_runtime_type_matches(
                                checker,
                                expected_ty,
                                actual_ty,
                                expected_variables,
                            )
                    },
                )
        }
        (Ty::Enum(expected_name, expected_args), Ty::Enum(actual_name, actual_args)) => {
            Checker::surface_name(&expected_name) == Checker::surface_name(&actual_name)
                && expected_args.len() == actual_args.len()
                && expected_args
                    .iter()
                    .zip(&actual_args)
                    .all(|(expected, actual)| {
                        builtin_runtime_type_matches(checker, expected, actual, expected_variables)
                    })
        }
        (Ty::Result(expected_ok, expected_err), Ty::Result(actual_ok, actual_err)) => {
            builtin_runtime_type_matches(checker, &expected_ok, &actual_ok, expected_variables)
                && builtin_runtime_type_matches(
                    checker,
                    &expected_err,
                    &actual_err,
                    expected_variables,
                )
        }
        (
            Ty::BuiltinFunc {
                name: expected_name,
                params: expected_params,
                ret: expected_ret,
            },
            Ty::BuiltinFunc {
                name: actual_name,
                params: actual_params,
                ret: actual_ret,
            },
        ) => {
            expected_name == actual_name
                && expected_params.len() == actual_params.len()
                && expected_params
                    .iter()
                    .zip(&actual_params)
                    .all(|(expected, actual)| {
                        builtin_runtime_type_matches(checker, expected, actual, expected_variables)
                    })
                && builtin_runtime_type_matches(
                    checker,
                    &expected_ret,
                    &actual_ret,
                    expected_variables,
                )
        }
        _ => false,
    }
}

fn normalize_surface_type(ty: &str, variables: &mut HashMap<String, String>) -> String {
    let ty = ty.replace("<()>", "<Unit>");
    let ty = if ty.trim() == "()" {
        "Unit".to_string()
    } else {
        ty
    };
    let mut normalized = String::with_capacity(ty.len());
    let mut chars = ty.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '$' {
            normalized.push(character);
            continue;
        }
        let mut name = String::from('$');
        while let Some(next) = chars.peek().copied() {
            if !(next.is_ascii_alphanumeric() || next == '_') {
                break;
            }
            name.push(next);
            chars.next();
        }
        let next_index = variables.len();
        let canonical = variables
            .entry(name)
            .or_insert_with(|| format!("${next_index}"));
        normalized.push_str(canonical);
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved_id() -> sigil::resolved::ResolvedId {
        sigil::resolved::ResolvedId {
            name: "sample".into(),
            qualified_name: None,
            unique_id: 1,
            compiler_generated: false,
            symbol_info: None,
            span: Span { start: 4, end: 10 },
        }
    }

    #[test]
    fn canonical_role_validation_rejects_truncated_resolved_lists() {
        let error = validate_canonical_role_list(&resolved_id(), "value parameter", 2, 1)
            .expect_err("role list length drift must fail closed");
        assert_eq!(
            error.reason(),
            Some(TypeDiagnosticReason::CallableSignatureMetadataMismatch)
        );
        assert!(error.message.contains("2 in source but 1 after resolution"));
    }

    #[test]
    fn canonical_role_validation_rejects_non_contiguous_ordinals() {
        let error = validate_return_type_argument_ordinals(&resolved_id(), [0, 2])
            .expect_err("ordinal drift must fail closed");
        assert_eq!(
            error.reason(),
            Some(TypeDiagnosticReason::CallableSignatureMetadataMismatch)
        );
        assert!(error.message.contains("ordinal 2 appears at position 1"));
    }

    #[test]
    fn return_type_argument_validation_uses_canonical_constructor_trait_identity() {
        let canonical_trait = sigil::resolved::ResolvedId {
            name: "VisibleContext".into(),
            qualified_name: Some("Imported::Context".into()),
            unique_id: 41,
            compiler_generated: false,
            symbol_info: None,
            span: Span { start: 0, end: 14 },
        };
        let return_type_arguments = vec![ResolvedReturnTypeArgument {
            ordinal: 0,
            ty: ResolvedSignatureTy {
                syntax: AstTy::Named(Span { start: 0, end: 14 }, "VisibleContext".into()),
                direct_constructor_trait: Some(canonical_trait.clone()),
            },
            span: Span { start: 0, end: 14 },
        }];
        let return_type = ResolvedSignatureTy {
            syntax: AstTy::Generic(
                Span { start: 20, end: 39 },
                "DifferentDisplay".into(),
                vec![AstTy::Named(Span { start: 37, end: 38 }, "Int".into())],
            ),
            direct_constructor_trait: Some(canonical_trait),
        };

        validate_return_type_argument_definition(
            "guard",
            &return_type_arguments,
            &[],
            Some(&return_type),
        )
        .expect("RTA and return occurrences are matched by resolved Trait identity");
    }

    #[test]
    fn duplicate_direct_constructor_return_type_arguments_are_rejected() {
        let canonical_trait = sigil::resolved::ResolvedId {
            name: "Alternative".into(),
            qualified_name: Some("Global::Alternative".into()),
            unique_id: 73,
            compiler_generated: false,
            symbol_info: None,
            span: Span { start: 0, end: 11 },
        };
        let argument = |start, end| ResolvedReturnTypeArgument {
            ordinal: 0,
            ty: ResolvedSignatureTy {
                syntax: AstTy::Named(Span { start, end }, "Alternative".into()),
                direct_constructor_trait: Some(canonical_trait.clone()),
            },
            span: Span { start, end },
        };
        let return_type_arguments = vec![argument(0, 11), argument(13, 24)];
        let return_type = ResolvedSignatureTy {
            syntax: AstTy::Generic(
                Span { start: 30, end: 47 },
                "Alternative".into(),
                vec![AstTy::Named(Span { start: 42, end: 46 }, "Unit".into())],
            ),
            direct_constructor_trait: Some(canonical_trait),
        };

        let error = validate_return_type_argument_definition(
            "guard",
            &return_type_arguments,
            &[],
            Some(&return_type),
        )
        .expect_err("one return carrier cannot silently select the first duplicate RTA");
        assert_eq!(
            error.reason(),
            Some(TypeDiagnosticReason::DuplicateReturnTypeArgumentInput)
        );
        let diagnostic = error.structured.expect("structured duplicate diagnostic");
        assert_eq!(diagnostic.primary.span, Span { start: 13, end: 24 });
        assert!(diagnostic
            .related
            .iter()
            .any(|fact| fact.span == Span { start: 0, end: 11 }));
        let DiagnosticData::ReturnTypeArgument(data) = &diagnostic.data else {
            panic!("duplicate RTA must use ReturnTypeArgument diagnostic data")
        };
        assert!(data.value_parameter_origin.is_none());
        assert_eq!(
            data.left_origin.as_ref().map(|fact| fact.role),
            Some(SourceRole::ReturnTypeArgument)
        );
        assert_eq!(
            data.right_origin.as_ref().map(|fact| fact.role),
            Some(SourceRole::ReturnTypeArgument)
        );
    }
}
