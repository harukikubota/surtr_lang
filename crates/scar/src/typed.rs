use serde::{Deserialize, Serialize};
use sigil::resolved::{Resolved, ResolvedId, ResolvedProcessHandlerUid, ResolvedProcessSpec};
use sindr::primitives::SurtrInt;
use spire::ast::{BinOp, Lit, ProcessSpec, Span, SupervisorInitSpec, Visibility};

use crate::types::Ty;

/// A fully typed AST node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedNode {
    pub ty: Ty,
    pub span: Span,
    pub node: TypedInner,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedProgram {
    pub nodes: Vec<TypedNode>,
    pub process_specs: Vec<TypedProcessSpec>,
    pub boot_plan: SupervisorInitSpec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedProcessSpec {
    pub module_path: String,
    pub process_name: String,
    pub spec: ProcessSpec,
    pub init_uid: u32,
    pub get_uid: u32,
    pub set_uid: Option<u32>,
    pub handler_uids: Vec<TypedProcessHandlerUid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedProcessHandlerUid {
    pub internal_name: String,
    pub uid: u32,
}

impl From<ResolvedProcessHandlerUid> for TypedProcessHandlerUid {
    fn from(value: ResolvedProcessHandlerUid) -> Self {
        Self {
            internal_name: value.internal_name,
            uid: value.uid,
        }
    }
}

impl From<ResolvedProcessSpec> for TypedProcessSpec {
    fn from(value: ResolvedProcessSpec) -> Self {
        Self {
            module_path: value.module_path,
            process_name: value.process_name,
            spec: value.spec,
            init_uid: value.init_uid,
            get_uid: value.get_uid,
            set_uid: value.set_uid,
            handler_uids: value.handler_uids.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedDbgArg {
    pub span: Span,
    pub ty_name: String,
    pub expr: TypedNode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListHelperRef {
    Builtin(u16),
    User(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComposeFlavor {
    Plain,
    ResultMap,
    ResultBind,
    ListMap { helper: ListHelperRef },
    ListBind { helper: ListHelperRef },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedTypeParam {
    pub name: String,
    pub ty_var: u32,
    pub bound: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TraitDispatch {
    Pending,
    Static(TraitDispatchTarget),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TraitDispatchTarget {
    BinOp(BinOp),
    Builtin(String),
    UserFunction { name: String, fun_idx: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TraitCallOrigin {
    Explicit,
    Operator {
        op: OperatorTraitOp,
        lhs_ty: Ty,
        rhs_ty: Ty,
    },
    Comparison {
        op: ComparisonOperator,
        lhs_ty: Ty,
        rhs_ty: Ty,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorTraitOp {
    PipeApply,
    PipeMap,
    PipeBind,
    SlashCompose,
    Compose,
    LiftCompose,
    KleisliCompose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComparisonOperator {
    Lt,
    Lte,
    Gt,
    Gte,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypedFacetSegment {
    Field {
        field_name: String,
        field_index: u32,
        container_field_count: u32,
        container_type_name: String,
        readonly: bool,
        focus_readonly_root: bool,
        focus_type_name: Option<String>,
    },
    Tuple {
        field_index: u32,
        tuple_len: u32,
        focus_readonly_root: bool,
        focus_type_name: Option<String>,
    },
    Variant {
        enum_name: String,
        variant_name: String,
        variant_tag: u32,
        discriminant: SurtrInt,
        payload_arity: u32,
        optional: bool,
        focus_readonly_root: bool,
        focus_type_name: Option<String>,
    },
    ListIndex {
        index: Box<TypedNode>,
        display: String,
        literal_index: Option<SurtrInt>,
        focus_readonly_root: bool,
        focus_type_name: Option<String>,
    },
    ListRange {
        start: Box<TypedNode>,
        end: Box<TypedNode>,
        display: String,
        literal_start: Option<SurtrInt>,
        literal_end: Option<SurtrInt>,
        focus_readonly_root: bool,
        focus_type_name: Option<String>,
    },
    MapKey {
        key: Box<TypedNode>,
        display: String,
        literal_key: Option<String>,
        focus_readonly_root: bool,
        focus_type_name: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedFieldPolicy {
    pub private: bool,
    pub readonly: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypedFacetPathKind {
    Structural,
    Variant,
}

impl TypedFacetPathKind {
    pub fn from_may_fail(may_fail: bool) -> Self {
        if may_fail {
            Self::Variant
        } else {
            Self::Structural
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Structural => "structural",
            Self::Variant => "variant",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedFacetPath {
    pub source_ty: Ty,
    pub focus_ty: Ty,
    pub path_kind: TypedFacetPathKind,
    pub may_fail: bool,
    pub source_readonly_root: bool,
    pub segments: Vec<TypedFacetSegment>,
}

impl TypedFacetPath {
    pub fn has_variant_segment(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| matches!(segment, TypedFacetSegment::Variant { .. }))
    }

    pub fn final_segment_is_variant(&self) -> bool {
        self.segments
            .last()
            .is_some_and(|segment| matches!(segment, TypedFacetSegment::Variant { .. }))
    }

    pub fn is_infallible_structural(&self) -> bool {
        !self.may_fail && !self.has_variant_segment()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingFacetPath {
    pub root_path_name: Option<String>,
    pub source_ty_hint: Option<Ty>,
    pub segments: Vec<PendingFacetSegment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PendingFacetExpr {
    Resolved(Box<Resolved>),
    Typed(Box<TypedNode>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PendingFacetSegment {
    Field {
        name: String,
        optional: bool,
    },
    Bracket {
        expr: PendingFacetExpr,
        display: String,
    },
    RangeBracket {
        start: PendingFacetExpr,
        end: PendingFacetExpr,
        display: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypedFacetSetMode {
    Exact,
    WrapPlainResult,
    CaseSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypedFacetOverMode {
    FocusValue,
    FocusResult,
    CaseFocusValue,
    CaseFocusResult,
}

/// Inner structure of a typed node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypedInner {
    Lit(Lit),
    Var(ResolvedId),
    App(Box<TypedNode>, Vec<TypedNode>),
    TraitCall {
        trait_name: String,
        method_name: String,
        receiver_ty: Ty,
        dispatch: TraitDispatch,
        origin: TraitCallOrigin,
        args: Vec<TypedNode>,
    },
    /// Unary callable synthesized from `f(...)` for apply-style operators.
    InjectCall(Box<TypedNode>, Vec<TypedNode>),
    Block(Vec<TypedNode>),
    Bind(TypedPattern, Box<TypedNode>),
    SafeBind(TypedPattern, Box<TypedNode>),
    BinOp(BinOp, Box<TypedNode>, Box<TypedNode>),
    Pipe(Box<TypedNode>, Box<TypedNode>),
    Compose(ComposeFlavor, Box<TypedNode>, Box<TypedNode>),
    ListNil,
    ListCons(Box<TypedNode>, Box<TypedNode>),
    ListLiteral(Vec<TypedNode>),
    TupleLiteral(Vec<TypedNode>),
    InterpolatedStr(Vec<TypedInterpolatedPart>),
    Dbg(Vec<TypedDbgArg>),
    If(Box<TypedNode>, Box<TypedNode>, Option<Box<TypedNode>>),
    Assert(Box<TypedNode>, Box<TypedNode>),
    Ensure(Box<TypedNode>, Box<TypedNode>, Box<TypedNode>),
    MapErr(Box<TypedNode>, Box<TypedNode>),
    Cause(Box<TypedNode>, Box<TypedNode>),
    RecoverKind(Box<TypedNode>, Box<TypedNode>, Box<TypedNode>),
    Match(Box<TypedNode>, Vec<TypedMatchArm>),

    /// Field access — field name resolved to index by Scar
    FieldAccess(Box<TypedNode>, u32),

    /// Process-local handler dependency access lowered from `ctx.<slot>`.
    ProcessContextHandler {
        process_name: String,
        slot: String,
    },

    /// Supervisor-driven worker spawn lowered from `DynSup::spawn(...)` or
    /// `MySup::spawn(...)`.
    SupervisorSpawn {
        supervisor_process: String,
        worker_process: String,
        init: Box<TypedNode>,
    },

    SupervisorAdopt {
        supervisor_process: String,
        worker_process: String,
        pid: Box<TypedNode>,
    },

    SupervisorStatus {
        supervisor_process: String,
    },

    SupervisorWorkers {
        supervisor_process: String,
        worker_process: String,
        init: Box<TypedNode>,
        strategy: Box<TypedNode>,
    },

    /// Compile-time facet constant path value. Stage 1 does not allow
    /// first-class runtime transport of facet values.
    FacetPath(TypedFacetPath),

    /// Deferred compile-time facet path value. Used for path bindings that need
    /// later source/focus context before they can be fully specialized.
    PendingFacetPath(PendingFacetPath),

    /// Facet view application with compile-time path metadata.
    FacetView {
        source: Box<TypedNode>,
        path: TypedFacetPath,
        source_is_result: bool,
    },

    /// Facet set application with compile-time path metadata.
    FacetSet {
        source: Box<TypedNode>,
        path: TypedFacetPath,
        value: Box<TypedNode>,
        source_is_result: bool,
        mode: TypedFacetSetMode,
    },

    /// Facet over application with compile-time path metadata.
    FacetOver {
        source: Box<TypedNode>,
        path: TypedFacetPath,
        update_fun: Box<TypedNode>,
        source_is_result: bool,
        mode: TypedFacetOverMode,
    },

    /// Struct literal — tag + field values (in definition order)
    StructLit(u32, Vec<TypedNode>),

    /// Constructor call — tag + field values (in definition order)
    ConstructorCall(u32, Vec<TypedNode>),

    /// Error type definition — tag + binding id + params + show expression
    DeferrorDef(u32, u32, ResolvedId, Vec<TypedFunParam>, Box<TypedNode>),

    /// Enum definition — enum type name + variants
    EnumDef(String, Vec<TypedEnumVariantDef>),

    /// Function definition — tag + name + params + return type + body
    Def(
        u32,
        ResolvedId,
        Vec<TypedTypeParam>,
        Vec<TypedFunParam>,
        Ty,
        Box<TypedNode>,
        Visibility,
    ),

    /// Extractor definition — function-shaped runtime entry with MatchResult return type.
    ExtractorDef(
        u32,
        ResolvedId,
        Vec<TypedTypeParam>,
        TypedFunParam,
        Ty,
        Box<TypedNode>,
        Visibility,
    ),

    /// Trait definition metadata.
    TraitDef(String, Vec<String>),

    /// Trait impl metadata.
    TraitImplDef(String, String),

    /// Builtin extractor declaration.
    BuiltinExtractorDecl(ResolvedId, Ty, Ty),

    /// Closure literal — params + captures + body
    Closure(Vec<TypedClosureParam>, Vec<ResolvedId>, Box<TypedNode>),

    /// Captured function value
    Capture(Box<TypedNode>, Vec<TypedNode>),

    /// Struct definition — tag + name + field names + field policies + readonly-root flag
    StructDef(u32, String, Vec<String>, Vec<TypedFieldPolicy>, bool),

    /// Record definition — tag + name + field names + field policies + readonly-root flag
    RecordDef(u32, String, Vec<String>, Vec<TypedFieldPolicy>, bool),

    /// Semicolon — explicit Unit coercion
    Semi(Box<TypedNode>),

    HashMapLiteral(Vec<(TypedNode, TypedNode)>),
}

/// Interpolated string fragment (typed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypedInterpolatedPart {
    Text(String),
    Expr(Box<TypedNode>),
}

/// Pattern in a binding (typed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypedPattern {
    Var(Ty, ResolvedId),
    Pin(Ty, ResolvedId, TraitDispatch),
    As(Ty, Box<TypedPattern>, ResolvedId),
    Wildcard(Ty),
    ListNil(Ty),
    ListCons(Ty, Box<TypedPattern>, Box<TypedPattern>),
    IntLit(Ty, SurtrInt),
    StrLit(Ty, String),
    BoolLit(Ty, bool),
    DurationLit(Ty, SurtrInt),
    Tuple(Ty, Vec<TypedPattern>),
    /// `Ok(inner)` pattern node in safe-bind recursion.
    ResultOk(Ty, Box<TypedPattern>),
    Extractor {
        input_ty: Ty,
        extractor: ResolvedId,
        extractor_ty: Ty,
        success_tag: u32,
        no_match_tag: u32,
        err_tag: u32,
        seq_tys: Vec<Ty>,
        items: Vec<TypedPattern>,
    },
}

/// Match pattern (typed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypedMatchPattern {
    Binding(ResolvedId),
    Pin {
        id: ResolvedId,
        ty: Ty,
        dispatch: TraitDispatch,
    },
    /// `inner @ alias`
    As(Box<TypedMatchPattern>, ResolvedId),
    /// `_`
    Wildcard,
    /// `True` / `False`
    BoolLit(bool),
    /// Integer literal
    IntLit(SurtrInt),
    /// String literal
    StrLit(String),
    /// Duration literal, e.g. `20ms`.
    DurationLit(SurtrInt),
    /// Concrete `deferror` kind pattern for abstract Error values.
    ErrorKind(String),
    /// Pattern alternative. Alternatives are tests only and do not bind names.
    Or(Vec<TypedMatchPattern>),
    Tuple(Vec<TypedMatchPattern>),
    /// Constructor tag + field patterns + payload field offset.
    Constructor {
        tag: u32,
        fields: Vec<TypedMatchPattern>,
        field_offset: u32,
    },
    /// `[]`
    ListNil,
    /// `[head, ..tail]`
    ListCons(Box<TypedMatchPattern>, Box<TypedMatchPattern>),
    Extractor {
        input_ty: Ty,
        extractor: ResolvedId,
        extractor_ty: Ty,
        success_tag: u32,
        no_match_tag: u32,
        err_tag: u32,
        seq_tys: Vec<Ty>,
        items: Vec<TypedMatchPattern>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedMatchArm {
    pub pattern: TypedMatchPattern,
    pub guard: Option<TypedNode>,
    pub body: TypedNode,
}

/// Function parameter (typed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedFunParam {
    pub id: ResolvedId,
    pub ty: Ty,
}

/// Typed closure parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedClosureParam {
    pub id: ResolvedId,
    pub ty: Ty,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypedEnumVariantDef {
    pub tag: u32,
    pub constructor_name: String,
    pub field_names: Vec<String>,
}
