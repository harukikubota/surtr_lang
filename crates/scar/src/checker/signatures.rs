//! Canonical callable-signature adapters.
//!
//! The checker still has legacy storage for a few declaration-only contracts
//! while the remaining phases migrate.  All new callable metadata enters
//! through this module, so user functions, trait helpers, and builtin surface
//! declarations share the same role-bearing shape.

use super::Checker;
use crate::types::Ty;
use sigil::resolved::{ResolvedReturnTypeArgument, ResolvedValueParameter};
use sindr::builtin::{builtin_surface_variant_for_decl, BuiltinSurfaceSignatureMeta};
use sindr::signature::{
    CallableDeclarationKind, CallableIdentity, CallableSignature, CanonicalConstraintSet,
    CanonicalReturnTypeArgument, CanonicalTypeOccurrence, CanonicalValueParameter, RuntimeTarget,
    SignatureOrigin, ValueParameterMode as CanonicalValueParameterMode,
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
        where_constraints: CanonicalConstraintSet::default(),
        runtime_target,
        declaration_origins: vec![SignatureOrigin::new(format!("declaration {}", id.name))],
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

/// Validate the source declaration's type shape against metadata. Parameter
/// names and modes remain part of the canonical metadata; this adapter checks
/// the source type contract here and leaves name/mode policy to the parser and
/// named-argument checker until those routes consume the shared signature.
pub(super) fn builtin_surface_matches(
    id: &sigil::resolved::ResolvedId,
    params: &[ResolvedValueParameter],
    ret_ty: Option<&spire::ast::AstTy>,
) -> bool {
    // The current runtime table contains a number of implementation-only
    // entries whose source declaration is an owner method (for example
    // `Workers::submit`). Their complete owner-specific surface contracts are
    // migrated incrementally; they still use the shared runtime target, but
    // must not be compared with an unqualified fallback variant.
    let qualified_owner = id
        .qualified_name
        .as_deref()
        .and_then(|name| {
            name.strip_prefix("Global::")
                .unwrap_or(name)
                .rsplit_once("::")
        })
        .map(|(owner, _)| owner);
    let runtime_name = sindr::builtin::builtin_runtime_name(&id.name, id.qualified_name.as_deref());
    if runtime_name.starts_with("__") {
        return true;
    }
    let Some(variant) = builtin_surface_signature(id) else {
        // Runtime-only entries without a migrated source surface variant are
        // validated by the existing declaration contract for now. They must
        // not be treated as a mismatched canonical surface.
        return true;
    };
    if qualified_owner.is_some() && variant.identity.owner.is_none() {
        return true;
    }
    if variant.value_parameters.len() != params.len() {
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
