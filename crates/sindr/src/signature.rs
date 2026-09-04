//! Canonical callable-signature metadata shared by the compiler phases.
//!
//! This module deliberately contains no parser or checker types. The metadata
//! layer can describe a callable before Scar resolves its surface types, while
//! Scar can instantiate the same structures with its canonical `Ty` type.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable internal identity used when a callable targets a builtin runtime
/// entry. It is distinct from a user-visible Surtr `Int`.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BuiltinId(pub u16);

impl From<u16> for BuiltinId {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl From<BuiltinId> for u16 {
    fn from(value: BuiltinId) -> Self {
        value.0
    }
}

impl fmt::Display for BuiltinId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValueParameterMode {
    PositionalOrNamed,
    Variadic,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SignatureOrigin {
    pub description: String,
}

impl SignatureOrigin {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanonicalReturnTypeArgument<T> {
    pub ordinal: u32,
    pub ty: T,
    pub origin: SignatureOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanonicalValueParameter<T> {
    pub ordinal: u32,
    pub name: String,
    pub mode: ValueParameterMode,
    pub ty: T,
    pub origin: SignatureOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanonicalTypeOccurrence<T> {
    pub ty: T,
    pub origin: SignatureOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanonicalConstraint<T> {
    pub subject: T,
    pub trait_name: String,
    pub origin: SignatureOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanonicalConstraintSet<T> {
    pub constraints: Vec<CanonicalConstraint<T>>,
}

impl<T> Default for CanonicalConstraintSet<T> {
    fn default() -> Self {
        Self {
            constraints: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CallableDeclarationKind {
    Function,
    TraitMethod,
    Builtin,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallableIdentity {
    /// The canonical owner, such as `Int` in `Int::safe_div`.
    pub owner: Option<String>,
    pub name: String,
    pub declaration_kind: CallableDeclarationKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RuntimeTarget {
    UserFunction(u32),
    Builtin(BuiltinId),
    TraitDispatch(u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallableSignature<T> {
    pub identity: CallableIdentity,
    pub return_type_arguments: Vec<CanonicalReturnTypeArgument<T>>,
    pub value_parameters: Vec<CanonicalValueParameter<T>>,
    pub return_type: CanonicalTypeOccurrence<T>,
    pub where_constraints: CanonicalConstraintSet<T>,
    pub runtime_target: RuntimeTarget,
    pub declaration_origins: Vec<SignatureOrigin>,
}

impl<T> CallableSignature<T> {
    pub fn value_arity(&self) -> usize {
        self.value_parameters.len()
    }
}
