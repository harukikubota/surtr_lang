//! Structured diagnostic data shared by phase adapters and renderers.
//!
//! The compiler phases keep their own error types.  This module only defines
//! the closed, phase-neutral envelope used once a phase has enough typed facts
//! to describe a diagnostic without asking a renderer to parse prose.

use serde::{Deserialize, Serialize, Serializer};
use serde_json::{json, Value};
use spire::ast::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeDiagnosticReason {
    ArityMismatch,
    ArgumentModeMismatch,
    UnknownNamedArgument,
    DuplicateArgument,
    MissingArgument,
    ArgumentTypeMismatch,
    ReturnTypeMismatch,
    AnnotationTypeMismatch,
    NotCallable,
    CallableShapeMismatch,
    CallableSignatureMetadataMismatch,
    ReturnTypeArgumentArityMismatch,
    ReturnTypeArgumentMismatch,
    AmbiguousReturnTypeArgument,
    InvalidTraitConstraintSubject,
    MissingGenericBound,
    MissingTraitCapability,
    NoApplicableTraitImplementation,
    UnresolvedTraitMethodInstantiation,
    MissingTraitDispatchTarget,
    MissingTypeConstructorConstraint,
    TraitMethodTypeListMismatch,
    TraitMethodTypeListArityMismatch,
    TraitMethodConstraintMismatch,
    TypeConstructorFamilyMismatch,
    TypePayloadMismatch,
    MissingTypeConstructorCapability,
    DuplicateReturnTypeArgumentInput,
    MissingReturnTypeArgument,
    UnusedReturnTypeArgument,
    ConcreteReturnTypeArgumentInDefinition,
    InlineReturnTypeArgumentConstraint,
    IfBranchTypeMismatch,
    MatchArmTypeMismatch,
    CondBranchTypeMismatch,
}

impl TypeDiagnosticReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArityMismatch => "ArityMismatch",
            Self::ArgumentModeMismatch => "ArgumentModeMismatch",
            Self::UnknownNamedArgument => "UnknownNamedArgument",
            Self::DuplicateArgument => "DuplicateArgument",
            Self::MissingArgument => "MissingArgument",
            Self::ArgumentTypeMismatch => "ArgumentTypeMismatch",
            Self::ReturnTypeMismatch => "ReturnTypeMismatch",
            Self::AnnotationTypeMismatch => "AnnotationTypeMismatch",
            Self::NotCallable => "NotCallable",
            Self::CallableShapeMismatch => "CallableShapeMismatch",
            Self::CallableSignatureMetadataMismatch => "CallableSignatureMetadataMismatch",
            Self::ReturnTypeArgumentArityMismatch => "ReturnTypeArgumentArityMismatch",
            Self::ReturnTypeArgumentMismatch => "ReturnTypeArgumentMismatch",
            Self::AmbiguousReturnTypeArgument => "AmbiguousReturnTypeArgument",
            Self::InvalidTraitConstraintSubject => "InvalidTraitConstraintSubject",
            Self::MissingGenericBound => "MissingGenericBound",
            Self::MissingTraitCapability => "MissingTraitCapability",
            Self::NoApplicableTraitImplementation => "NoApplicableTraitImplementation",
            Self::UnresolvedTraitMethodInstantiation => "UnresolvedTraitMethodInstantiation",
            Self::MissingTraitDispatchTarget => "MissingTraitDispatchTarget",
            Self::MissingTypeConstructorConstraint => "MissingTypeConstructorConstraint",
            Self::TraitMethodTypeListMismatch => "TraitMethodTypeListMismatch",
            Self::TraitMethodTypeListArityMismatch => "TraitMethodTypeListArityMismatch",
            Self::TraitMethodConstraintMismatch => "TraitMethodConstraintMismatch",
            Self::TypeConstructorFamilyMismatch => "TypeConstructorFamilyMismatch",
            Self::TypePayloadMismatch => "TypePayloadMismatch",
            Self::MissingTypeConstructorCapability => "MissingTypeConstructorCapability",
            Self::DuplicateReturnTypeArgumentInput => "DuplicateReturnTypeArgumentInput",
            Self::MissingReturnTypeArgument => "MissingReturnTypeArgument",
            Self::UnusedReturnTypeArgument => "UnusedReturnTypeArgument",
            Self::ConcreteReturnTypeArgumentInDefinition => {
                "ConcreteReturnTypeArgumentInDefinition"
            }
            Self::InlineReturnTypeArgumentConstraint => "InlineReturnTypeArgumentConstraint",
            Self::IfBranchTypeMismatch => "IfBranchTypeMismatch",
            Self::MatchArmTypeMismatch => "MatchArmTypeMismatch",
            Self::CondBranchTypeMismatch => "CondBranchTypeMismatch",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticOrigin {
    Call,
    TraitCall,
    Operator,
    Annotation,
    Return,
    Branch,
    Pattern,
    Declaration,
    Intrinsic,
    Runtime,
    ReturnTypeArgument { ordinal: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SourceRole {
    Value,
    ReturnTypeArgument,
    Annotation,
    Expected,
    Contract,
    Impl,
    Trait,
    LeftValue,
    RightValue,
    Branch,
    Pattern,
    Declaration,
    CallTarget,
    Other,
}

impl SourceRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Value => "Value",
            Self::ReturnTypeArgument => "ReturnTypeArgument",
            Self::Annotation => "Annotation",
            Self::Expected => "Expected",
            Self::Contract => "Contract",
            Self::Impl => "Impl",
            Self::Trait => "Trait",
            Self::LeftValue => "LeftValue",
            Self::RightValue => "RightValue",
            Self::Branch => "Branch",
            Self::Pattern => "Pattern",
            Self::Declaration => "Declaration",
            Self::CallTarget => "CallTarget",
            Self::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclarationIdentity {
    pub owner: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceFact {
    pub role: SourceRole,
    pub source_id: crate::SourceId,
    pub span: Span,
    pub ty: Option<String>,
    pub declaration_identity: Option<DeclarationIdentity>,
}

impl SourceFact {
    pub fn typed(
        role: SourceRole,
        source_id: crate::SourceId,
        span: Span,
        ty: impl Into<String>,
    ) -> Self {
        Self {
            role,
            source_id,
            span,
            ty: Some(ty.into()),
            declaration_identity: None,
        }
    }

    pub fn untyped(role: SourceRole, source_id: crate::SourceId, span: Span) -> Self {
        Self {
            role,
            source_id,
            span,
            ty: None,
            declaration_identity: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArgumentRelationData {
    pub callable: String,
    pub ordinal: u32,
    pub expected_type: Option<String>,
    pub actual_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReturnTypeArgumentData {
    pub callable: String,
    pub ordinal: u32,
    pub expected_type: String,
    pub actual_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallableSignatureData {
    pub callable: String,
    pub role: String,
    pub expected_count: Option<u32>,
    pub actual_count: Option<u32>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConstraintSubjectData {
    pub subject: String,
    pub constraint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraitObligationData {
    pub trait_name: String,
    pub subject_type: String,
    pub position: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraitDispatchData {
    pub trait_name: String,
    pub method: String,
    pub subject_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateFailureData {
    pub candidate_type: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateSelectionData {
    pub trait_name: String,
    pub method: String,
    pub failures: Vec<CandidateFailureData>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeConstructorCarrierData {
    pub family: String,
    pub expected_carrier: String,
    pub actual_carrier: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BranchAssertionData {
    pub expected_type: String,
    pub actual_type: String,
    pub branch: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SafeBindRelationData {
    pub lhs_type: String,
    pub rhs_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyData {
    pub policy: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeData {
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticData {
    ArgumentRelation(ArgumentRelationData),
    ReturnTypeArgument(ReturnTypeArgumentData),
    CallableSignature(CallableSignatureData),
    ConstraintSubject(ConstraintSubjectData),
    TraitObligation(TraitObligationData),
    TraitDispatch(TraitDispatchData),
    CandidateSelection(CandidateSelectionData),
    TypeConstructorCarrier(TypeConstructorCarrierData),
    BranchAssertion(BranchAssertionData),
    SafeBindRelation(SafeBindRelationData),
    Policy(PolicyData),
    Runtime(RuntimeData),
}

impl DiagnosticData {
    /// Serialize the payload as a flat object.  The variant discriminator is
    /// retained as `kind`, while fields such as `ordinal` remain directly
    /// addressable to keep the JSON contract useful to clients.
    pub fn to_json_value(&self) -> Value {
        let (kind, payload) = match self {
            Self::ArgumentRelation(value) => ("ArgumentRelation", serde_json::to_value(value)),
            Self::ReturnTypeArgument(value) => ("ReturnTypeArgument", serde_json::to_value(value)),
            Self::CallableSignature(value) => ("CallableSignature", serde_json::to_value(value)),
            Self::ConstraintSubject(value) => ("ConstraintSubject", serde_json::to_value(value)),
            Self::TraitObligation(value) => ("TraitObligation", serde_json::to_value(value)),
            Self::TraitDispatch(value) => ("TraitDispatch", serde_json::to_value(value)),
            Self::CandidateSelection(value) => ("CandidateSelection", serde_json::to_value(value)),
            Self::TypeConstructorCarrier(value) => {
                ("TypeConstructorCarrier", serde_json::to_value(value))
            }
            Self::BranchAssertion(value) => ("BranchAssertion", serde_json::to_value(value)),
            Self::SafeBindRelation(value) => ("SafeBindRelation", serde_json::to_value(value)),
            Self::Policy(value) => ("Policy", serde_json::to_value(value)),
            Self::Runtime(value) => ("Runtime", serde_json::to_value(value)),
        };
        let mut object = match payload.expect("diagnostic data is serializable") {
            Value::Object(object) => object,
            _ => unreachable!("diagnostic data payload must be an object"),
        };
        object.insert("kind".into(), json!(kind));
        Value::Object(object)
    }
}

impl Serialize for DiagnosticData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_json_value().serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Remediation {
    Help { text: String },
    Candidates { items: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StructuredDiagnostic {
    pub reason: TypeDiagnosticReason,
    pub origin: DiagnosticOrigin,
    pub data: DiagnosticData,
    pub primary: SourceFact,
    pub related: Vec<SourceFact>,
    pub remediation: Option<Remediation>,
}

impl StructuredDiagnostic {
    pub fn primary_type(&self) -> Option<&str> {
        self.primary.ty.as_deref()
    }

    pub fn remediation_text(&self) -> Option<String> {
        match self.remediation.as_ref()? {
            Remediation::Help { text } => Some(text.clone()),
            Remediation::Candidates { items } if items.is_empty() => None,
            Remediation::Candidates { items } => Some(items.join("\n")),
        }
    }
}
