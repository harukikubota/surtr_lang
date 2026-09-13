//! Compiler-owned contracts for language intrinsics.
//!
//! Surface declarations are documentation and validation inputs. The types in
//! this module are the canonical, closed representation consumed by compiler
//! phases; they intentionally do not retain or parse raw signature text.

use crate::names::TypeName;
use serde::{Deserialize, Serialize};

/// Canonical identity of a compiler-owned intrinsic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntrinsicId {
    Do,
}

impl IntrinsicId {
    pub const fn surface_name(self) -> &'static str {
        match self {
            Self::Do => "do",
        }
    }
}

/// Canonical standard owner of an intrinsic surface declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntrinsicOwner {
    Bootstrap,
}

impl IntrinsicOwner {
    pub const fn surface_name(self) -> &'static str {
        match self {
            Self::Bootstrap => "Bootstrap",
        }
    }
}

/// Canonical Trait identities referenced by intrinsic contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CanonicalTraitIdentity {
    Monad,
    Alternative,
}

impl CanonicalTraitIdentity {
    pub const fn surface_name(self) -> &'static str {
        match self {
            Self::Monad => "Monad",
            Self::Alternative => "Alternative",
        }
    }
}

/// Canonical Trait method identities used as intrinsic lowering targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CanonicalTraitMethodIdentity {
    MonadBind,
    AlternativeEmpty,
}

impl CanonicalTraitMethodIdentity {
    pub const fn trait_identity(self) -> CanonicalTraitIdentity {
        match self {
            Self::MonadBind => CanonicalTraitIdentity::Monad,
            Self::AlternativeEmpty => CanonicalTraitIdentity::Alternative,
        }
    }

    pub const fn method_name(self) -> &'static str {
        match self {
            Self::MonadBind => "bind",
            Self::AlternativeEmpty => "empty",
        }
    }
}

/// Type variable identities owned by the intrinsic contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntrinsicTypeParameter {
    Result,
}

/// Closed canonical type structure used by an intrinsic signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntrinsicType {
    BuiltinApplication {
        head: TypeName,
        argument: IntrinsicTypeParameter,
    },
    TraitApplication {
        trait_identity: CanonicalTraitIdentity,
        argument: IntrinsicTypeParameter,
    },
}

/// Meaning of one return type argument position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReturnTypeArgumentRole {
    DirectTypeCtorTrait(CanonicalTraitIdentity),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReturnTypeArgumentContract {
    pub position: usize,
    pub role: ReturnTypeArgumentRole,
}

/// The single carrier source shared by every rule in a `do` expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntrinsicCarrierSource {
    ReturnTypeArgument(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SafeBindRejection {
    NonMonad,
    NonResultMonad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SafeBindInputMatcher {
    CanonicalType(TypeName),
    OtherwisePartialPattern,
    OtherwiseTotalPattern,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SafeBindInputAction {
    UnwrapOneLayer,
    PassThroughToPattern,
    RejectAfterPatternTypeCheck(&'static [SafeBindRejection]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SafeBindInputRule {
    pub matcher: SafeBindInputMatcher,
    pub action: SafeBindInputAction,
}

/// Closed predicates which can activate a capability obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DoCapabilityPredicate {
    Always,
    HasPartialExtractPattern,
    HasLegalSafeBindAndCarrierIsNot(TypeName),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DoCapabilityRule {
    pub predicate: DoCapabilityPredicate,
    pub capability: CanonicalTraitIdentity,
    pub same_carrier: IntrinsicCarrierSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SafeBindFailureAction {
    PreserveExistingSafeBindFailure,
    OverrideWith(CanonicalTraitMethodIdentity),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SafeBindFailureContract {
    pub canonical_result: TypeName,
    pub canonical_result_action: SafeBindFailureAction,
    pub otherwise_action: SafeBindFailureAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DoLoweringContract {
    pub sequence: CanonicalTraitMethodIdentity,
    pub partial_failure: CanonicalTraitMethodIdentity,
    pub safe_bind_failure: SafeBindFailureContract,
}

/// Canonical compiler-owned `do` contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DoIntrinsicContract {
    pub identity: IntrinsicId,
    pub owner: IntrinsicOwner,
    pub return_type_arguments: &'static [ReturnTypeArgumentContract],
    pub value_parameters: &'static [IntrinsicType],
    pub return_type: IntrinsicType,
    pub do_local_carrier: IntrinsicCarrierSource,
    pub safe_bind_input: &'static [SafeBindInputRule],
    pub capability_rules: &'static [DoCapabilityRule],
    pub lowering: DoLoweringContract,
}

const DO_CARRIER: IntrinsicCarrierSource = IntrinsicCarrierSource::ReturnTypeArgument(0);

const DO_RETURN_TYPE_ARGUMENTS: &[ReturnTypeArgumentContract] = &[ReturnTypeArgumentContract {
    position: 0,
    role: ReturnTypeArgumentRole::DirectTypeCtorTrait(CanonicalTraitIdentity::Monad),
}];

const DO_VALUE_PARAMETERS: &[IntrinsicType] = &[IntrinsicType::BuiltinApplication {
    head: TypeName::DoBlock,
    argument: IntrinsicTypeParameter::Result,
}];

const DO_SAFE_BIND_REJECTIONS: &[SafeBindRejection] = &[
    SafeBindRejection::NonMonad,
    SafeBindRejection::NonResultMonad,
];

const DO_SAFE_BIND_INPUT: &[SafeBindInputRule] = &[
    SafeBindInputRule {
        matcher: SafeBindInputMatcher::CanonicalType(TypeName::Result),
        action: SafeBindInputAction::UnwrapOneLayer,
    },
    SafeBindInputRule {
        matcher: SafeBindInputMatcher::OtherwisePartialPattern,
        action: SafeBindInputAction::PassThroughToPattern,
    },
    SafeBindInputRule {
        matcher: SafeBindInputMatcher::OtherwiseTotalPattern,
        action: SafeBindInputAction::RejectAfterPatternTypeCheck(DO_SAFE_BIND_REJECTIONS),
    },
];

const DO_CAPABILITY_RULES: &[DoCapabilityRule] = &[
    DoCapabilityRule {
        predicate: DoCapabilityPredicate::Always,
        capability: CanonicalTraitIdentity::Monad,
        same_carrier: DO_CARRIER,
    },
    DoCapabilityRule {
        predicate: DoCapabilityPredicate::HasPartialExtractPattern,
        capability: CanonicalTraitIdentity::Alternative,
        same_carrier: DO_CARRIER,
    },
    DoCapabilityRule {
        predicate: DoCapabilityPredicate::HasLegalSafeBindAndCarrierIsNot(TypeName::Result),
        capability: CanonicalTraitIdentity::Alternative,
        same_carrier: DO_CARRIER,
    },
];

pub const DO_INTRINSIC_CONTRACT: DoIntrinsicContract = DoIntrinsicContract {
    identity: IntrinsicId::Do,
    owner: IntrinsicOwner::Bootstrap,
    return_type_arguments: DO_RETURN_TYPE_ARGUMENTS,
    value_parameters: DO_VALUE_PARAMETERS,
    return_type: IntrinsicType::TraitApplication {
        trait_identity: CanonicalTraitIdentity::Monad,
        argument: IntrinsicTypeParameter::Result,
    },
    do_local_carrier: DO_CARRIER,
    safe_bind_input: DO_SAFE_BIND_INPUT,
    capability_rules: DO_CAPABILITY_RULES,
    lowering: DoLoweringContract {
        sequence: CanonicalTraitMethodIdentity::MonadBind,
        partial_failure: CanonicalTraitMethodIdentity::AlternativeEmpty,
        safe_bind_failure: SafeBindFailureContract {
            canonical_result: TypeName::Result,
            canonical_result_action: SafeBindFailureAction::PreserveExistingSafeBindFailure,
            otherwise_action: SafeBindFailureAction::OverrideWith(
                CanonicalTraitMethodIdentity::AlternativeEmpty,
            ),
        },
    },
};

pub const fn do_intrinsic_contract() -> &'static DoIntrinsicContract {
    &DO_INTRINSIC_CONTRACT
}
