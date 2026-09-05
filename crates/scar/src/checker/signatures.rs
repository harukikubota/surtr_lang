//! Canonical callable-signature adapters.
//!
//! User functions, trait helpers, and builtin surface declarations all enter
//! the checker through this role-bearing shape.

use super::Checker;
use crate::types::Ty;
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
use std::collections::HashMap;

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
) -> CallableSignature<Ty> {
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
    CallableSignature {
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
