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
    ResolvedReturnTypeArgument, ResolvedValueParameter, ResolvedWhereClause,
    ResolvedWhereConstraintRhs,
};
use sindr::builtin::{builtin_surface_variant_for_decl, BuiltinSurfaceSignatureMeta};
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
pub(super) struct TypeInputId(String);

#[derive(Debug, Clone)]
pub(super) struct SourceOrigin {
    pub(super) span: Span,
}

#[derive(Debug, Default)]
pub(super) struct SignatureOccurrences {
    pub(super) argument_inputs: BTreeMap<TypeInputId, Vec<SourceOrigin>>,
    pub(super) return_inputs: BTreeMap<TypeInputId, Vec<SourceOrigin>>,
    pub(super) declared_return_type_arguments: BTreeMap<TypeInputId, SourceOrigin>,
}

#[derive(Debug, Default)]
pub(super) struct DirectConstructorInputs {
    witnesses: HashMap<String, Ty>,
}

fn type_input_id(name: &str, constructor_trait_names: &HashSet<String>) -> Option<TypeInputId> {
    (name == "Self" || name.starts_with('$') || constructor_trait_names.contains(name))
        .then(|| TypeInputId(name.to_string()))
}

fn collect_type_inputs(
    ty: &AstTy,
    constructor_trait_names: &HashSet<String>,
    inputs: &mut BTreeMap<TypeInputId, Vec<SourceOrigin>>,
) {
    match ty {
        AstTy::Named(span, name) => {
            if let Some(id) = type_input_id(name, constructor_trait_names) {
                inputs
                    .entry(id)
                    .or_default()
                    .push(SourceOrigin { span: span.clone() });
            }
        }
        AstTy::Generic(span, name, arguments) => {
            if let Some(id) = type_input_id(name, constructor_trait_names) {
                inputs
                    .entry(id)
                    .or_default()
                    .push(SourceOrigin { span: span.clone() });
            }
            for argument in arguments {
                collect_type_inputs(argument, constructor_trait_names, inputs);
            }
        }
        AstTy::Tuple(_, items) => {
            for item in items {
                collect_type_inputs(item, constructor_trait_names, inputs);
            }
        }
        AstTy::Func(_, parameters, return_type) => {
            for parameter in parameters {
                collect_type_inputs(parameter, constructor_trait_names, inputs);
            }
            collect_type_inputs(return_type, constructor_trait_names, inputs);
        }
        AstTy::ImplTrait(_, _) => {}
    }
}

pub(super) fn signature_occurrences(
    return_type_arguments: &[ResolvedReturnTypeArgument],
    value_parameters: &[ResolvedValueParameter],
    return_type: Option<&AstTy>,
    constructor_trait_names: &HashSet<String>,
) -> SignatureOccurrences {
    let mut occurrences = SignatureOccurrences::default();
    for parameter in value_parameters {
        collect_type_inputs(
            &parameter.ty,
            constructor_trait_names,
            &mut occurrences.argument_inputs,
        );
    }
    if let Some(return_type) = return_type {
        collect_type_inputs(
            return_type,
            constructor_trait_names,
            &mut occurrences.return_inputs,
        );
    }
    for argument in return_type_arguments {
        let id = match &argument.ty {
            AstTy::Named(_, name) => type_input_id(name, constructor_trait_names),
            _ => None,
        };
        if let Some(id) = id {
            occurrences.declared_return_type_arguments.insert(
                id,
                SourceOrigin {
                    span: argument.span.clone(),
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
    related: Vec<SourceFact>,
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
                callable: callable.into(),
                ordinal,
                expected_type: input.into(),
                actual_type: input.into(),
            }),
            primary,
            related,
            remediation: Some(Remediation::Help { text: help.into() }),
        }),
    }
}

pub(super) fn validate_return_type_argument_definition(
    callable: &str,
    return_type_arguments: &[ResolvedReturnTypeArgument],
    value_parameters: &[ResolvedValueParameter],
    return_type: Option<&AstTy>,
    constructor_trait_names: &HashSet<String>,
) -> Result<(), crate::error::TypeError> {
    let occurrences = signature_occurrences(
        return_type_arguments,
        value_parameters,
        return_type,
        constructor_trait_names,
    );

    for (ordinal, argument) in return_type_arguments.iter().enumerate() {
        let Some(input) = (match &argument.ty {
            AstTy::Named(_, name) => type_input_id(name, constructor_trait_names),
            _ => None,
        }) else {
            continue;
        };
        let TypeInputId(name) = &input;
        if let Some(argument_origins) = occurrences.argument_inputs.get(&input) {
            let related = argument_origins
                .first()
                .map(|origin| source_fact(SourceRole::Value, origin.span.clone(), name))
                .into_iter()
                .collect();
            return Err(occurrence_error(
                TypeDiagnosticReason::DuplicateReturnTypeArgumentInput,
                callable,
                name,
                ordinal as u32,
                source_fact(SourceRole::ReturnTypeArgument, argument.span.clone(), name),
                related,
                format!("type input `{name}` is introduced more than once"),
                &format!("remove `{name}` from the return type arguments"),
            ));
        }
        if !occurrences.return_inputs.contains_key(&input) {
            return Err(occurrence_error(
                TypeDiagnosticReason::UnusedReturnTypeArgument,
                callable,
                name,
                ordinal as u32,
                source_fact(SourceRole::ReturnTypeArgument, argument.span.clone(), name),
                Vec::new(),
                format!("return type argument `{name}` does not appear in the return type"),
                "remove the unused return type argument or use it in the return type",
            ));
        }
    }

    for (input, origins) in &occurrences.return_inputs {
        if occurrences.argument_inputs.contains_key(input)
            || occurrences
                .declared_return_type_arguments
                .contains_key(input)
        {
            continue;
        }
        let TypeInputId(name) = input;
        let origin = origins
            .first()
            .expect("a collected type input always has an origin");
        return Err(occurrence_error(
            TypeDiagnosticReason::MissingReturnTypeArgument,
            callable,
            name,
            0,
            source_fact(SourceRole::Expected, origin.span.clone(), name),
            Vec::new(),
            format!("return-only type input `{name}` is not declared"),
            &format!("declare it as `def {callable}::<{name}>(...)`"),
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
    ast_ty: &AstTy,
    resolved: &Ty,
    inputs: &mut DirectConstructorInputs,
) {
    let Some(trait_key) = checker.constructor_trait_key_for_ast_ty(ast_ty) else {
        return;
    };
    let Ty::SelfApp(items) = resolved else {
        return;
    };
    let Some((witness, _)) = Checker::constructor_application_parts(items) else {
        return;
    };
    inputs.witnesses.insert(trait_key, witness.clone());
}

pub(super) fn coalesce_direct_constructor_inputs(
    checker: &Checker,
    ty: Ty,
    inputs: &DirectConstructorInputs,
) -> Ty {
    match ty {
        Ty::SelfApp(mut items) => {
            if let Some((Ty::Var(var), _)) = Checker::constructor_application_parts(&items) {
                if let Some(trait_key) = checker.constructor_witness_traits.get(var) {
                    if let Some(shared) = inputs.witnesses.get(trait_key) {
                        items[1] = shared.clone();
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
            params,
            ret,
        } => Ty::UserFunc {
            fun_idx,
            type_params,
            params: params
                .into_iter()
                .map(|parameter| coalesce_direct_constructor_inputs(checker, parameter, inputs))
                .collect(),
            ret: Box::new(coalesce_direct_constructor_inputs(checker, *ret, inputs)),
        },
        Ty::Struct(name, fields) => Ty::Struct(
            name,
            fields
                .into_iter()
                .map(|(name, ty)| {
                    (
                        name,
                        coalesce_direct_constructor_inputs(checker, ty, inputs),
                    )
                })
                .collect(),
        ),
        Ty::Record(name, fields) => Ty::Record(
            name,
            fields
                .into_iter()
                .map(|(name, ty)| {
                    (
                        name,
                        coalesce_direct_constructor_inputs(checker, ty, inputs),
                    )
                })
                .collect(),
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
) -> CallableSignature<Ty> {
    let mut fresh = HashMap::new();
    CallableSignature {
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
    }
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
}
