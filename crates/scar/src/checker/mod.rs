#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use sigil::resolved::*;
use sindr::builtin::{
    builtin_function_metas, builtin_type_head_metas, builtin_type_meta_by_name, BuiltinMeta,
};
use sindr::names::builtin_type_usage_policy;
use sindr::policy::{ExitCodePolicy, RuntimeSourcePolicy};
use sindr::warning::{
    CompilerWarning, PhaseOutput, WarningBuffer, WarningKind, WarningPhase, WarningSpan,
};
use spire::ast::{AstTy, BinOp, Lit, Span};

use crate::env::{TypeEnv, TypeKind};
use crate::error::TypeError;
use crate::typed::*;
use crate::types::{NominalType, Ty};

mod captures;
mod carriers;
mod definitions;
mod expr;
mod handler_captures;
mod lazy_diagnostics;
mod matching;
use lazy_diagnostics::{ActiveLazyCapture, LazyCaptureDiagnostic};
mod inference;
mod patterns;
mod predeclare;
mod provenance;
mod relations;
mod resolve_typed;
mod signatures;
mod specialize;
mod trait_selection;
mod type_display;
mod type_syntax;
use trait_selection::{ApplicabilityProof, MethodTypeEnvironment};

mod audit;
mod bootstrap;
mod process;
mod profile;
mod program;
mod session;
mod state;
mod warnings;

use bootstrap::{builtin_ty_from_meta, format_builtin_type_param_suffix, initialize_env};
use profile::{ProfileEvent, TypecheckProfiler};
use session::PersistentCheckerState;
pub use session::{
    type_contains_unresolved_vars, typecheck, typecheck_staged_program,
    typecheck_staged_program_with_context, typecheck_staged_program_with_context_with_warnings,
    typecheck_staged_program_with_warnings, typecheck_with_context,
    typecheck_with_context_with_warnings, typecheck_with_warnings, ScarCheckpoint, ScarSession,
    TypecheckContext,
};
use state::Checker;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum MonadFailResolution {
    Preserve(MonadFailTarget),
    Unavailable,
    Deferred,
    InvalidMetadata(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallableContext {
    Function,
    Closure,
    Extractor,
    ExtractorClosure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeSyntaxContext {
    General,
    StdBuiltinParameter,
    BindingAnnotation,
    ConcreteErrorLocal,
    FunctionReturn,
    HoleClosureParam,
    FacetDeferredSlot,
    ExtractorReturn,
    ExtractorClosureReturn,
    ExtractorBody,
    ErrorMarker,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraitMethodInfo {
    id: ResolvedId,
    return_type_arguments: Vec<ResolvedReturnTypeArgument>,
    type_params: Vec<ResolvedTypeParam>,
    value_parameters: Vec<ResolvedValueParameter>,
    ret_ty: sigil::resolved::ResolvedSignatureTy,
    where_clause: Option<TypedWhereClause>,
    attrs: ResolvedDeclAttrs,
    body: Option<Box<Resolved>>,
    span: Span,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraitInfo {
    id: ResolvedId,
    /// Set only while checking the trusted standard definition stage.
    compiler_owned_equality: bool,
    compiler_owned_failure: bool,
    /// Declaration authority persists across user impls and cached standard stages.
    standard_lazy_contract: bool,
    type_params: Vec<ResolvedTypeParam>,
    where_clause: Option<TypedWhereClause>,
    constructor_slots: Vec<String>,
    parents: Vec<TraitParent>,
    methods: HashMap<String, TraitMethodInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraitParent {
    trait_id: ResolvedId,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraitImplMethodInfo {
    method_name: String,
    function_id: ResolvedId,
    return_type_arguments: Vec<ResolvedReturnTypeArgument>,
    type_params: Vec<ResolvedTypeParam>,
    value_parameters: Vec<ResolvedValueParameter>,
    ret_ty: Option<sigil::resolved::ResolvedSignatureTy>,
    where_clause: Option<TypedWhereClause>,
    body: Box<Resolved>,
    attrs: ResolvedDeclAttrs,
    span: Span,
    display_name_override: Option<String>,
    dispatch_override: Option<TraitDispatchTarget>,
    is_builtin: bool,
    #[serde(default)]
    body_obligations: Vec<TraitObligation>,
    instantiation_contract: Option<ImplMethodInstantiationContract>,
    resolved_signature: Option<ResolvedImplMethodSignature>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraitImplInfo {
    declaration_key: TraitImplDeclarationKey,
    head_type_list: ImplHeadTypeList,
    impl_constraints: CanonicalConstraintSet,
    method_signature_lists: HashMap<String, MethodSignatureTypeList>,
    trait_id: ResolvedId,
    trait_args: Vec<AstTy>,
    trait_arg_tys: Vec<Ty>,
    target_name: String,
    target_ast_ty: AstTy,
    target_ty: Ty,
    where_clause: Option<TypedWhereClause>,
    #[serde(default)]
    generated_derive: bool,
    type_param_vars: Vec<u32>,
    type_param_vars_by_name: HashMap<String, u32>,
    constructor_slot_vars: Vec<u32>,
    constructor_slot_positions: Vec<usize>,
    methods: HashMap<String, TraitImplMethodInfo>,
}

/// Declaration binders are allocated once and reused by the method body.
/// Calls selected before body checking must reference this same namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResolvedImplMethodSignature {
    params: Vec<Ty>,
    result: Ty,
    type_params: Vec<u32>,
    return_type_arguments: Vec<Ty>,
    environment: MethodTypeEnvironment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ImplMethodInstantiationContract {
    head: ImplHeadTypeList,
    signature: MethodSignatureTypeList,
    impl_constraints: CanonicalConstraintSet,
}

pub(super) struct PendingTraitCandidate {
    pub waiting_on: Vec<u32>,
    pub candidates: Vec<TraitImplDeclarationKey>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CandidateFailureKind {
    TraitImplHeadMismatch,
    TraitImplWhereUnsatisfied,
    TraitMethodInvocationMismatch,
}
#[derive(Debug, Clone)]
pub(super) struct CandidateFailure {
    pub declaration: TraitImplDeclarationKey,
    pub kind: CandidateFailureKind,
    pub span: Span,
}
#[derive(Debug, Clone)]
pub(super) struct CandidateRejection {
    pub failures: Vec<CandidateFailure>,
}
pub(super) enum CandidateApplicability {
    Applicable(TraitMethodInstantiation),
    Deferred(PendingTraitCandidate),
    Rejected(CandidateRejection),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ConstructorProjectionFailure {
    Canonicalization,
    MissingImplTargetMetadata,
    #[serde(skip)]
    PendingImplMapping,
    UnsatisfiedConstraints,
    NoApplicableImplementation,
    AmbiguousImplementation,
    MissingConstructorSlotMapping {
        variable: u32,
    },
    MissingWitnessTrait,
    SlotCountMismatch {
        expected: usize,
        actual: usize,
    },
    InvalidSlotPosition {
        position: usize,
    },
    MissingNominalArguments,
    UnsupportedConstructor,
    // A failed proof is terminal and must never be written into a successful checkpoint.
    #[serde(skip)]
    ProofError(Box<TypeError>),
}

#[derive(Debug, Clone)]
enum ConstructorProjectionOutcome {
    Applicable {
        info: Arc<TraitImplInfo>,
        mapping: HashMap<u32, Ty>,
    },
    Deferred {
        waiting_on: Vec<u32>,
    },
    Rejected {
        failures: Vec<ConstructorProjectionFailure>,
    },
}

#[derive(Debug, Clone)]
enum ConstructorCarrierOutcome {
    Projected(CanonicalConstructorCarrier),
    Deferred {
        waiting_on: Vec<u32>,
    },
    Rejected {
        failures: Vec<ConstructorProjectionFailure>,
    },
}

#[derive(Debug, Clone)]
enum ConstructorCarrierRelation {
    SameCarrier,
    DifferentCarrier,
    Deferred {
        waiting_on: Vec<u32>,
    },
    Rejected {
        failures: Vec<ConstructorProjectionFailure>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum ConstructorApplicationOutcome {
    Applied(Ty),
    Deferred {
        waiting_on: Vec<u32>,
    },
    Rejected {
        failures: Vec<ConstructorProjectionFailure>,
    },
}

#[derive(Debug, Clone)]
enum ConstructorSlotsOutcome {
    Projected(Vec<Ty>),
    Deferred {
        waiting_on: Vec<u32>,
    },
    Rejected {
        failures: Vec<ConstructorProjectionFailure>,
    },
}

impl ConstructorProjectionFailure {
    fn proof_error(failures: &[Self]) -> Option<TypeError> {
        failures.iter().find_map(|failure| match failure {
            Self::ProofError(error) => Some((**error).clone()),
            _ => None,
        })
    }
}

macro_rules! checked_constructor_outcome {
    ($($outcome:ty),+ $(,)?) => {$(
        impl $outcome {
            fn into_checked(self) -> Result<Self, TypeError> {
                if let Self::Rejected { failures } = &self {
                    if let Some(error) = ConstructorProjectionFailure::proof_error(failures) {
                        return Err(error);
                    }
                }
                Ok(self)
            }
        }
    )+};
}
checked_constructor_outcome!(
    ConstructorProjectionOutcome,
    ConstructorCarrierOutcome,
    ConstructorCarrierRelation,
    ConstructorApplicationOutcome,
    ConstructorSlotsOutcome,
);

fn try_all<T>(
    values: impl IntoIterator<Item = T>,
    mut check: impl FnMut(T) -> Result<bool, TypeError>,
) -> Result<bool, TypeError> {
    for value in values {
        if !check(value)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A trait requirement kept in inference state.  Keep the trait identity and
/// its type arguments separate so parameterized bounds are not collapsed into
/// a display string while an inference variable remains unresolved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PendingTraitObligation {
    trait_id: String,
    args: Vec<Ty>,
    receiver: Ty,
}

#[derive(Debug, Clone)]
struct CapabilityUse {
    subject_ty: Ty,
    subject_name: String,
    trait_id: String,
    span: Span,
    consumed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SignatureAliasInfo {
    params: Vec<ResolvedTypeParam>,
    rhs: AstTy,
    span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ConstKind {
    PrimitiveLiteral,
    FacetPath,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum StoredConstValue {
    Literal(Lit),
    FacetPath(TypedFacetPath),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum StoredFacetPath {
    Concrete(TypedFacetPath),
    Pending(PendingFacetPath),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConstMeta {
    name: String,
    visibility: spire::ast::Visibility,
    ty: Ty,
    kind: ConstKind,
    value: StoredConstValue,
    span: Span,
}

type TraitImplKey = CanonicalTraitImplPatternKey;
type TraitImplIndex = HashMap<u32, Vec<TraitImplKey>>;
// Immutable definitions and declaration metadata are shared by snapshots.
// Mutation must detach the affected value with Arc::make_mut.
type SpecializableDefinitions = HashMap<u32, Arc<TypedNode>>;
type TraitImplementations = HashMap<TraitImplKey, Arc<TraitImplInfo>>;
type TraitDefinitions = HashMap<String, Arc<TraitInfo>>;
type CallableSignatures = HashMap<u32, Arc<sindr::signature::CallableSignature<Ty>>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallableInstantiationKey {
    callable: MethodDeclarationId,
    implementation: Option<TraitImplementationId>,
    trait_inputs: Vec<(TypeListRole, u32, CanonicalTy)>,
    type_args: Vec<CanonicalTyKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum CanonicalTyKey {
    Int,
    Float,
    String,
    Boolean,
    Unit,
    Error,
    Hole,
    Var(u32),
    SelfApp(Vec<CanonicalTyKey>),
    List(Box<CanonicalTyKey>),
    MatchResult(Box<CanonicalTyKey>),
    ExtractorClosure(Box<CanonicalTyKey>),
    Tuple(Vec<CanonicalTyKey>),
    Func {
        params: Vec<CanonicalTyKey>,
        ret: Box<CanonicalTyKey>,
    },
    Lazy(Box<CanonicalTyKey>),
    Facet {
        kind: crate::types::FacetKind,
        source: Box<CanonicalTyKey>,
        focus: Box<CanonicalTyKey>,
        update_source: Box<CanonicalTyKey>,
        update_focus: Box<CanonicalTyKey>,
    },
    Pid(Box<CanonicalTyKey>),
    ProcessMarker(String),
    BuiltinFunc {
        name: String,
        params: Vec<CanonicalTyKey>,
        ret: Box<CanonicalTyKey>,
    },
    UserFunc {
        type_params: Vec<u32>,
        params: Vec<CanonicalTyKey>,
        ret: Box<CanonicalTyKey>,
    },
    Struct {
        name: String,
        args: Vec<CanonicalTyKey>,
        fields: Vec<(String, CanonicalTyKey)>,
    },
    Record {
        name: String,
        args: Vec<CanonicalTyKey>,
        fields: Vec<(String, CanonicalTyKey)>,
    },
    Enum {
        name: String,
        args: Vec<CanonicalTyKey>,
    },
    Result {
        ok: Box<CanonicalTyKey>,
        err: Box<CanonicalTyKey>,
    },
}

/// Solver identity is structural. In particular, two applications of the
/// same generic struct with different field types must not collapse to the
/// display name used in diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ObligationKey {
    trait_name: String,
    trait_args: Vec<CanonicalTyKey>,
    target: CanonicalTyKey,
}

/// Declaration-only dependency discovery. A proof which requests unfinished
/// metadata is rolled back and retried after those declarations are resolved.
#[derive(Default)]
struct ConstructorMappingResolution {
    pending: HashSet<TraitImplKey>,
    resolving: HashSet<TraitImplKey>,
    requests: RefCell<Option<HashSet<TraitImplKey>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum ConstructorCapabilityProvenance {
    /// No abstract guarantee: concrete constructor applicability must be proved.
    RequiresProof,
    Constrained(BTreeSet<String>),
    /// The outer carrier is fixed by a declared return capability while its
    /// representation still carries value-derived provenance for projections.
    ConstrainedTemplate {
        capabilities: BTreeSet<String>,
        source: Box<(ConstructorCapabilityProvenance, Ty)>,
    },
    /// Every source must justify the requested capability at the use site.
    Intersection(Vec<(ConstructorCapabilityProvenance, Ty)>),
    Fields(Vec<(ConstructorCapabilityProvenance, Ty)>),
    Variants(Vec<(u32, Vec<(ConstructorCapabilityProvenance, Ty)>)>),
    Sequence(Vec<(ConstructorCapabilityProvenance, Ty)>),
    Callable {
        parameters: Vec<u32>,
        result: Box<(ConstructorCapabilityProvenance, Ty)>,
        /// Calls depending on inferred inputs preserve their declaration
        /// contracts independently of the callable's nominal function type.
        calls: Vec<(
            (ConstructorCapabilityProvenance, Ty),
            Vec<(ConstructorCapabilityProvenance, Ty)>,
        )>,
    },
    Parameter(u32),
    DeclaredCallable(u32),
    Injected {
        function: Box<(ConstructorCapabilityProvenance, Ty)>,
        arguments: Vec<(ConstructorCapabilityProvenance, Ty)>,
    },
    Call {
        function: Box<(ConstructorCapabilityProvenance, Ty)>,
        arguments: Vec<(ConstructorCapabilityProvenance, Ty)>,
    },
    Projection {
        source: Box<(ConstructorCapabilityProvenance, Ty)>,
        projection: provenance::Projection,
    },
    Template {
        ty: Ty,
        variables: HashMap<u32, (ConstructorCapabilityProvenance, Ty)>,
    },
    ConstructorApplication(ConstructorApplicationOutcome),
}

impl ConstructorCapabilityProvenance {
    fn constrained(trait_key: String) -> Self {
        Self::Constrained(BTreeSet::from([trait_key]))
    }
}
