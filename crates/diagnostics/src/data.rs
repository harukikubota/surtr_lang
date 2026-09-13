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
    UnresolvedEnumConstructorTypeArgument,
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
    SafeBindTotalPatternNonMonadRhs,
    SafeBindTotalPatternNonResultMonadRhs,
    PatternTypeMismatch,
    PatternShapeMismatch,
    PatternArityMismatch,
    NonTotalBindingPattern,
    NestedResultErrorPattern,
    MatchGuardTypeMismatch,
    ConstructorPatternRequiresEnumOrResultRhs,
    ExtractorInputTypeMismatch,
    ExtractorArityMismatch,
    NonExhaustiveMatch,
    SafeBindErrorTypeMismatch,
    SafeBindRequiresResultTarget,
    ErrorValueMustBeWrapped,
    FacetSafeBindForbidden,
    FacetPatternBindingForbidden,
    FacetOperationPolicyViolation,
    FacetCompileTimeOnly,
    ProcessHandlerScope,
    ProcessPolicyViolation,
    SourcePolicyViolation,
    CompilePolicyViolation,
    NominalDeclarationConstraintViolation,
    TraitHelperCaptureNeedsExpectedType,
    TypecheckInvariantViolation,
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
            Self::UnresolvedEnumConstructorTypeArgument => "UnresolvedEnumConstructorTypeArgument",
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
            Self::SafeBindTotalPatternNonMonadRhs => "SafeBindTotalPatternNonMonadRhs",
            Self::SafeBindTotalPatternNonResultMonadRhs => "SafeBindTotalPatternNonResultMonadRhs",
            Self::PatternTypeMismatch => "PatternTypeMismatch",
            Self::PatternShapeMismatch => "PatternShapeMismatch",
            Self::PatternArityMismatch => "PatternArityMismatch",
            Self::NonTotalBindingPattern => "NonTotalBindingPattern",
            Self::NestedResultErrorPattern => "NestedResultErrorPattern",
            Self::MatchGuardTypeMismatch => "MatchGuardTypeMismatch",
            Self::ConstructorPatternRequiresEnumOrResultRhs => {
                "ConstructorPatternRequiresEnumOrResultRhs"
            }
            Self::ExtractorInputTypeMismatch => "ExtractorInputTypeMismatch",
            Self::ExtractorArityMismatch => "ExtractorArityMismatch",
            Self::NonExhaustiveMatch => "NonExhaustiveMatch",
            Self::SafeBindErrorTypeMismatch => "SafeBindErrorTypeMismatch",
            Self::SafeBindRequiresResultTarget => "SafeBindRequiresResultTarget",
            Self::ErrorValueMustBeWrapped => "ErrorValueMustBeWrapped",
            Self::FacetSafeBindForbidden => "FacetSafeBindForbidden",
            Self::FacetPatternBindingForbidden => "FacetPatternBindingForbidden",
            Self::FacetOperationPolicyViolation => "FacetOperationPolicyViolation",
            Self::FacetCompileTimeOnly => "FacetCompileTimeOnly",
            Self::ProcessHandlerScope => "ProcessHandlerScope",
            Self::ProcessPolicyViolation => "ProcessPolicyViolation",
            Self::SourcePolicyViolation => "SourcePolicyViolation",
            Self::CompilePolicyViolation => "CompilePolicyViolation",
            Self::NominalDeclarationConstraintViolation => "NominalDeclarationConstraintViolation",
            Self::TraitHelperCaptureNeedsExpectedType => "TraitHelperCaptureNeedsExpectedType",
            Self::TypecheckInvariantViolation => "TypecheckInvariantViolation",
        }
    }
}

/// Stable runtime failure families selected by the runtime producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeDiagnosticReason {
    ValuePatternMismatch,
    ValueFailure,
    BuiltinContractViolation,
    VmInvariant,
    ProcessInitTimeout,
    ProcessInitFailed,
    TaskTimeout,
    CallTimeout,
    ProcessLifecycleFailed,
}

impl RuntimeDiagnosticReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ValuePatternMismatch => "ValuePatternMismatch",
            Self::ValueFailure => "ValueFailure",
            Self::BuiltinContractViolation => "BuiltinContractViolation",
            Self::VmInvariant => "VmInvariant",
            Self::ProcessInitTimeout => "ProcessInitTimeout",
            Self::ProcessInitFailed => "ProcessInitFailed",
            Self::TaskTimeout => "TaskTimeout",
            Self::CallTimeout => "CallTimeout",
            Self::ProcessLifecycleFailed => "ProcessLifecycleFailed",
        }
    }
}

/// Stable parser failure families. The producer selects a reason from parser
/// state; diagnostic adapters never recover it from rendered text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseDiagnosticReason {
    IncompleteInput,
    UnexpectedToken,
    DeclarationSyntax,
    ExpressionSyntax,
    StatementSyntax,
    PatternSyntax,
    TypeSyntax,
    LiteralSyntax,
    PositionRule,
    SourcePolicy,
    InterpolationSyntax,
    CompilerInvariant,
}

impl ParseDiagnosticReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IncompleteInput => "IncompleteInput",
            Self::UnexpectedToken => "UnexpectedToken",
            Self::DeclarationSyntax => "DeclarationSyntax",
            Self::ExpressionSyntax => "ExpressionSyntax",
            Self::StatementSyntax => "StatementSyntax",
            Self::PatternSyntax => "PatternSyntax",
            Self::TypeSyntax => "TypeSyntax",
            Self::LiteralSyntax => "LiteralSyntax",
            Self::PositionRule => "PositionRule",
            Self::SourcePolicy => "SourcePolicy",
            Self::InterpolationSyntax => "InterpolationSyntax",
            Self::CompilerInvariant => "CompilerInvariant",
        }
    }
}

/// Stable resolver failure families selected at the resolution producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveDiagnosticReason {
    NameResolution,
    Namespace,
    Visibility,
    Import,
    Capture,
    Pattern,
    Declaration,
    SpecialForm,
    SourcePolicy,
    CompilerInvariant,
}

/// Stable REPL command/query failure families selected by the UI producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplDiagnosticReason {
    QueryEmpty,
    QueryUnsupported,
    TypedCallMissingClosingParen,
    TypedCallMissingCallee,
    TypedCallInvalidCallee,
    TypedCallEmptyArgument,
    OperatorMissingTarget,
    QueryArgumentUnsupported,
    QueryArgumentListUnterminated,
    QueryTypeInvalid,
    QueryEvaluationFailed,
    CommandUnknown,
    CommandArgumentInvalid,
}

impl ReplDiagnosticReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QueryEmpty => "QueryEmpty",
            Self::QueryUnsupported => "QueryUnsupported",
            Self::TypedCallMissingClosingParen => "TypedCallMissingClosingParen",
            Self::TypedCallMissingCallee => "TypedCallMissingCallee",
            Self::TypedCallInvalidCallee => "TypedCallInvalidCallee",
            Self::TypedCallEmptyArgument => "TypedCallEmptyArgument",
            Self::OperatorMissingTarget => "OperatorMissingTarget",
            Self::QueryArgumentUnsupported => "QueryArgumentUnsupported",
            Self::QueryArgumentListUnterminated => "QueryArgumentListUnterminated",
            Self::QueryTypeInvalid => "QueryTypeInvalid",
            Self::QueryEvaluationFailed => "QueryEvaluationFailed",
            Self::CommandUnknown => "CommandUnknown",
            Self::CommandArgumentInvalid => "CommandArgumentInvalid",
        }
    }
}

impl ResolveDiagnosticReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NameResolution => "NameResolution",
            Self::Namespace => "Namespace",
            Self::Visibility => "Visibility",
            Self::Import => "Import",
            Self::Capture => "Capture",
            Self::Pattern => "Pattern",
            Self::Declaration => "Declaration",
            Self::SpecialForm => "SpecialForm",
            Self::SourcePolicy => "SourcePolicy",
            Self::CompilerInvariant => "CompilerInvariant",
        }
    }
}

/// Closed cross-phase reason wrapper. Each phase retains its own reason enum
/// so parser, resolver, and typechecker vocabularies stay independent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticReason {
    Type(TypeDiagnosticReason),
    Parse(ParseDiagnosticReason),
    Resolve(ResolveDiagnosticReason),
    Runtime(RuntimeDiagnosticReason),
    Repl(ReplDiagnosticReason),
}

impl Serialize for DiagnosticReason {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl DiagnosticReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Type(reason) => reason.as_str(),
            Self::Parse(reason) => reason.as_str(),
            Self::Resolve(reason) => reason.as_str(),
            Self::Runtime(reason) => reason.as_str(),
            Self::Repl(reason) => reason.as_str(),
        }
    }

    pub const fn type_reason(self) -> Option<TypeDiagnosticReason> {
        match self {
            Self::Type(reason) => Some(reason),
            _ => None,
        }
    }
}

impl From<TypeDiagnosticReason> for DiagnosticReason {
    fn from(value: TypeDiagnosticReason) -> Self {
        Self::Type(value)
    }
}

impl From<ParseDiagnosticReason> for DiagnosticReason {
    fn from(value: ParseDiagnosticReason) -> Self {
        Self::Parse(value)
    }
}

impl From<ResolveDiagnosticReason> for DiagnosticReason {
    fn from(value: ResolveDiagnosticReason) -> Self {
        Self::Resolve(value)
    }
}

impl From<RuntimeDiagnosticReason> for DiagnosticReason {
    fn from(value: RuntimeDiagnosticReason) -> Self {
        Self::Runtime(value)
    }
}

impl From<ReplDiagnosticReason> for DiagnosticReason {
    fn from(value: ReplDiagnosticReason) -> Self {
        Self::Repl(value)
    }
}

impl PartialEq<TypeDiagnosticReason> for DiagnosticReason {
    fn eq(&self, other: &TypeDiagnosticReason) -> bool {
        *self == Self::Type(*other)
    }
}

impl PartialEq<DiagnosticReason> for TypeDiagnosticReason {
    fn eq(&self, other: &DiagnosticReason) -> bool {
        DiagnosticReason::Type(*self) == *other
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParseDiagnosticData {
    pub detail: String,
    pub expected_tokens: Vec<String>,
    pub cursor_span: Span,
    pub guidance: Option<ParseDiagnosticGuidance>,
    pub token_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum ParseDiagnosticGuidance {
    UnexpectedToken,
    TopLevelDeclaration,
    TopLevelExpression,
    UnitPattern,
    AsPatternAlias,
    RangeLiteral,
    OperatorCapture(String),
    PairConstructorCapture,
    ReturnPositionImplTrait,
    WhereClause,
    MissingMetaState,
    MissingMetaInstance,
    AnonymousCaptureIdentity,
    AnonymousCaptureRequiresHelper,
    ImmediateAnonymousCall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolveDiagnosticData {
    pub detail: String,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplDiagnosticData {
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum DiagnosticOrigin {
    Call,
    TraitCall,
    Operator { operator: String },
    Annotation,
    Return,
    Branch { form: BranchForm, ordinal: u32 },
    Pattern,
    Declaration,
    Intrinsic,
    Runtime,
    Parse,
    Resolve,
    ReturnTypeArgument { ordinal: u32 },
    EnumConstructor { ordinal: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BranchForm {
    If,
    IfLet,
    Match,
    Cond,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
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
    Guard,
    Pattern,
    Declaration,
    CallTarget,
    Other,
}

impl SourceRole {
    pub const fn json_name(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::ReturnTypeArgument => "return_type_argument",
            Self::Annotation => "annotation",
            Self::Expected => "expected",
            Self::Contract => "contract",
            Self::Impl => "impl",
            Self::Trait => "trait",
            Self::LeftValue => "left_value",
            Self::RightValue => "right_value",
            Self::Branch => "branch",
            Self::Guard => "guard",
            Self::Pattern => "pattern",
            Self::Declaration => "declaration",
            Self::CallTarget => "call_target",
            Self::Other => "other",
        }
    }

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
            Self::Guard => "Guard",
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
    pub ordinal: Option<u32>,
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
            ordinal: None,
            source_id,
            span,
            ty: Some(ty.into()),
            declaration_identity: None,
        }
    }

    pub fn untyped(role: SourceRole, source_id: crate::SourceId, span: Span) -> Self {
        Self {
            role,
            ordinal: None,
            source_id,
            span,
            ty: None,
            declaration_identity: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CallableReturnShape {
    Any,
    Plain,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallableShapeData {
    pub callable: String,
    pub actual_type: Option<String>,
    pub expected_arity: Option<u32>,
    pub actual_arity: Option<u32>,
    pub return_shape: CallableReturnShape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArgumentContractData {
    pub callable: String,
    pub name: Option<String>,
    pub expected_count: u32,
    pub actual_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArgumentRelationData {
    #[serde(skip)]
    pub expected_origin: Option<SourceFact>,
    #[serde(skip)]
    pub actual_origin: Option<SourceFact>,

    pub callable: String,
    pub ordinal: u32,
    pub expected_type: Option<String>,
    pub actual_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReturnTypeArgumentData {
    pub declared_origin: Option<SourceFact>,
    pub value_parameter_origin: Option<SourceFact>,
    pub return_origin: Option<SourceFact>,
    #[serde(skip)]
    pub left_origin: Option<SourceFact>,
    #[serde(skip)]
    pub right_origin: Option<SourceFact>,
    #[serde(skip)]
    pub required_trait: Option<String>,
    pub expected_count: Option<u32>,
    pub actual_count: Option<u32>,

    pub callable: String,
    pub ordinal: Option<u32>,
    pub expected_type: Option<String>,
    pub actual_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnumConstructorTypeArgumentData {
    pub enum_name: String,
    pub constructor: String,
    pub ordinal: u32,
    pub constraint_status: EnumConstructorConstraintStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EnumConstructorConstraintStatus {
    Insufficient,
}

/// Stable signature-list roles shared by the checker and diagnostic consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TypeListRole {
    TraitArgument,
    ImplTarget,
    ReturnTypeArgument,
    ValueParameter,
    ReturnType,
}

impl TypeListRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TraitArgument => "TraitArgument",
            Self::ImplTarget => "ImplTarget",
            Self::ReturnTypeArgument => "ReturnTypeArgument",
            Self::ValueParameter => "ValueParameter",
            Self::ReturnType => "ReturnType",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraitDiagnosticIdentity {
    pub trait_id: String,
    pub trait_arguments: Vec<String>,
    pub subject_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraitMethodTypeListData {
    pub impl_declaration: Option<SourceFact>,

    pub identity: Option<TraitDiagnosticIdentity>,
    pub method_name: String,
    pub role: TypeListRole,
    pub ordinal: u32,
    pub nested_path: Vec<u32>,
    pub expected_type: Option<String>,
    pub actual_type: Option<String>,
    pub expected_count: Option<u32>,
    pub actual_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraitMethodConstraintData {
    pub impl_declaration: Option<SourceFact>,

    pub identity: Option<TraitDiagnosticIdentity>,
    pub method_name: String,
    pub expected_constraints: Vec<String>,
    pub actual_constraints: Vec<String>,
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
    #[serde(skip)]
    pub subject_origin: Option<SourceFact>,
    #[serde(skip)]
    pub required_trait: Option<String>,
    #[serde(skip)]
    pub suggested_type_variable: Option<String>,

    pub subject: String,
    pub constraint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraitObligationData {
    #[serde(skip)]
    pub obligation_origin: Option<SourceFact>,

    pub trait_name: String,
    pub trait_arguments: Vec<String>,
    pub subject_type: String,
    pub position: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraitDispatchData {
    pub impl_declaration: Option<SourceFact>,

    pub trait_name: String,
    pub trait_arguments: Vec<String>,
    pub method: Option<String>,
    pub subject_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateFailureData {
    pub candidate_type: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateSelectionData {
    pub subject_type: Option<String>,
    pub trait_arguments: Vec<String>,
    pub impl_declaration: Option<SourceFact>,

    pub trait_name: String,
    pub method: String,
    pub failures: Vec<CandidateFailureData>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeConstructorCarrierData {
    #[serde(skip)]
    pub left_type: Option<String>,
    #[serde(skip)]
    pub right_type: Option<String>,
    #[serde(skip)]
    pub left_origin: Option<SourceFact>,
    #[serde(skip)]
    pub right_origin: Option<SourceFact>,
    #[serde(skip)]
    pub required_capability: String,

    pub family: String,
    pub family_id: String,
    pub expected_carrier: String,
    pub actual_carrier: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BranchAssertionData {
    #[serde(skip)]
    pub form: BranchForm,
    #[serde(skip)]
    pub left_ordinal: Option<u32>,
    #[serde(skip)]
    pub right_ordinal: Option<u32>,
    #[serde(skip)]
    pub left_origin: Option<SourceFact>,
    #[serde(skip)]
    pub right_origin: Option<SourceFact>,

    pub expected_type: String,
    pub actual_type: String,
    pub branch: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SafeBindRelationData {
    pub lhs_type: String,
    pub rhs_type: String,
    pub lhs_is_total: bool,
    pub rhs_is_canonical_result: bool,
    pub monad_capability: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    Variable,
    Tuple,
    List,
    Constructor,
    Extractor,
    Match,
    Pin,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PatternDiagnosticData {
    pub pattern_kind: PatternKind,
    pub name: Option<String>,
    pub expected_type: Option<String>,
    pub actual_type: Option<String>,
    pub expected_count: Option<usize>,
    pub actual_count: Option<usize>,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TypePolicy {
    ErrorValuePlacement,
    SafeBindFacet,
    FacetPatternBinding,
    FacetOperation,
    SafeBindRequiresResultTarget,
    SafeBindFailureTarget,
    FacetStageRestriction,
    ProcessHandlerScope,
    ProcessCapabilityPolicy,
    CompileUnitAvailability,
    SourceExitCode,
    EntrypointRequirement,
    NominalDeclarationConstraint,
    TraitHelperCaptureInference,
    ProducerContract,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyData {
    pub policy: TypePolicy,
    pub subject: Option<String>,
    pub expected_type: Option<String>,
    pub actual_type: Option<String>,
    pub stage: Option<String>,
    pub entrypoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeData {
    pub detail: String,
    pub failure_kind: Option<String>,
    pub opcode: Option<String>,
    pub function: Option<String>,
    pub lhs: Option<String>,
    pub rhs: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticData {
    CallableShape(CallableShapeData),
    ArgumentContract(ArgumentContractData),
    ArgumentRelation(ArgumentRelationData),
    ReturnTypeArgument(ReturnTypeArgumentData),
    EnumConstructorTypeArgument(EnumConstructorTypeArgumentData),
    CallableSignature(CallableSignatureData),
    TraitMethodTypeList(TraitMethodTypeListData),
    TraitMethodConstraint(TraitMethodConstraintData),
    ConstraintSubject(ConstraintSubjectData),
    TraitObligation(TraitObligationData),
    TraitDispatch(TraitDispatchData),
    CandidateSelection(CandidateSelectionData),
    TypeConstructorCarrier(TypeConstructorCarrierData),
    BranchAssertion(BranchAssertionData),
    SafeBindRelation(SafeBindRelationData),
    Pattern(PatternDiagnosticData),
    Policy(PolicyData),
    Runtime(RuntimeData),
    Parse(ParseDiagnosticData),
    Resolve(ResolveDiagnosticData),
    Repl(ReplDiagnosticData),
}

impl DiagnosticData {
    /// Serialize the payload as a flat object.  The variant discriminator is
    /// retained as `kind`, while fields such as `ordinal` remain directly
    /// addressable to keep the JSON contract useful to clients.
    pub fn to_json_value(&self) -> Value {
        self.project_json()
    }

    pub(crate) fn raw_json_value(&self) -> Value {
        let (kind, payload) = match self {
            Self::CallableShape(value) => ("CallableShape", serde_json::to_value(value)),
            Self::ArgumentContract(value) => ("ArgumentContract", serde_json::to_value(value)),
            Self::ArgumentRelation(value) => ("ArgumentRelation", serde_json::to_value(value)),
            Self::ReturnTypeArgument(value) => ("ReturnTypeArgument", serde_json::to_value(value)),
            Self::EnumConstructorTypeArgument(value) => {
                ("EnumConstructorTypeArgument", serde_json::to_value(value))
            }
            Self::CallableSignature(value) => ("CallableSignature", serde_json::to_value(value)),
            Self::TraitMethodTypeList(value) => {
                ("TraitMethodTypeList", serde_json::to_value(value))
            }
            Self::TraitMethodConstraint(value) => {
                ("TraitMethodConstraint", serde_json::to_value(value))
            }
            Self::ConstraintSubject(value) => ("ConstraintSubject", serde_json::to_value(value)),
            Self::TraitObligation(value) => ("TraitObligation", serde_json::to_value(value)),
            Self::TraitDispatch(value) => ("TraitDispatch", serde_json::to_value(value)),
            Self::CandidateSelection(value) => ("CandidateSelection", serde_json::to_value(value)),
            Self::TypeConstructorCarrier(value) => {
                ("TypeConstructorCarrier", serde_json::to_value(value))
            }
            Self::BranchAssertion(value) => ("BranchAssertion", serde_json::to_value(value)),
            Self::SafeBindRelation(value) => ("SafeBindRelation", serde_json::to_value(value)),
            Self::Pattern(value) => ("Pattern", serde_json::to_value(value)),
            Self::Policy(value) => ("Policy", serde_json::to_value(value)),
            Self::Runtime(value) => ("Runtime", serde_json::to_value(value)),
            Self::Parse(value) => ("Parse", serde_json::to_value(value)),
            Self::Resolve(value) => ("Resolve", serde_json::to_value(value)),
            Self::Repl(value) => ("Repl", serde_json::to_value(value)),
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
    pub reason: DiagnosticReason,
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

impl StructuredDiagnostic {
    pub fn map_source_locations(
        mut self,
        mut map: impl FnMut(&Span) -> (crate::SourceId, Span),
    ) -> Self {
        self.map_source_facts(|fact| {
            let (source_id, span) = map(&fact.span);
            fact.source_id = source_id;
            fact.span = span;
        });
        self
    }

    /// Apply phase source adaptation to both semantic origins and display labels.
    pub fn map_source_facts(&mut self, mut map: impl FnMut(&mut SourceFact)) {
        for fact in std::iter::once(&mut self.primary).chain(self.related.iter_mut()) {
            map(fact);
        }
        let origins: Vec<&mut Option<SourceFact>> = match &mut self.data {
            DiagnosticData::ArgumentRelation(v) => {
                vec![&mut v.expected_origin, &mut v.actual_origin]
            }
            DiagnosticData::ReturnTypeArgument(v) => vec![
                &mut v.declared_origin,
                &mut v.value_parameter_origin,
                &mut v.return_origin,
                &mut v.left_origin,
                &mut v.right_origin,
            ],
            DiagnosticData::ConstraintSubject(v) => vec![&mut v.subject_origin],
            DiagnosticData::TraitObligation(v) => vec![&mut v.obligation_origin],
            DiagnosticData::TraitDispatch(v) => vec![&mut v.impl_declaration],
            DiagnosticData::TraitMethodTypeList(v) => vec![&mut v.impl_declaration],
            DiagnosticData::TraitMethodConstraint(v) => vec![&mut v.impl_declaration],
            DiagnosticData::CandidateSelection(v) => vec![&mut v.impl_declaration],
            DiagnosticData::TypeConstructorCarrier(v) => {
                vec![&mut v.left_origin, &mut v.right_origin]
            }
            DiagnosticData::BranchAssertion(v) => vec![&mut v.left_origin, &mut v.right_origin],
            DiagnosticData::Repl(_) => Vec::new(),
            _ => vec![],
        };
        for origin in origins.into_iter().flatten() {
            map(origin);
        }
    }
}
