use sigil::resolved::ResolvedId;
use sindr::primitives::SurtrInt;
use spire::ast::{BinOp, Lit, Span, Visibility};

use crate::types::Ty;

/// A fully typed AST node.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedNode {
    pub ty: Ty,
    pub span: Span,
    pub node: TypedInner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListHelperRef {
    Builtin(u16),
    User(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeFlavor {
    Plain,
    ResultMap,
    ResultBind,
    ListMap { helper: ListHelperRef },
    ListBind { helper: ListHelperRef },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedTypeParam {
    pub name: String,
    pub ty_var: u32,
    pub bound: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TraitDispatch {
    Pending,
    Static(TraitDispatchTarget),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TraitDispatchTarget {
    BinOp(BinOp),
    Builtin(String),
    UserFunction { name: String, fun_idx: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypedLensSegment {
    Field {
        field_name: String,
        field_index: u32,
    },
    Tuple {
        field_index: u32,
    },
    Variant {
        enum_name: String,
        variant_name: String,
        variant_tag: u32,
        payload_arity: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedLensPath {
    pub source_ty: Ty,
    pub focus_ty: Ty,
    pub may_fail: bool,
    pub segments: Vec<TypedLensSegment>,
}

/// Inner structure of a typed node.
#[derive(Debug, Clone, PartialEq)]
pub enum TypedInner {
    Lit(Lit),
    Var(ResolvedId),
    App(Box<TypedNode>, Vec<TypedNode>),
    TraitCall {
        trait_name: String,
        method_name: String,
        receiver_ty: Ty,
        dispatch: TraitDispatch,
        args: Vec<TypedNode>,
    },
    /// Unary callable synthesized from `f(...)` for apply-style operators.
    InjectCall(Box<TypedNode>, Vec<TypedNode>),
    Block(Vec<TypedNode>),
    Bind(TypedPattern, Box<TypedNode>),
    SafeBind(TypedPattern, Box<TypedNode>),
    BinOp(BinOp, Box<TypedNode>, Box<TypedNode>),
    Pipe(Box<TypedNode>, Box<TypedNode>),
    ResultMap(Box<TypedNode>, Box<TypedNode>),
    ResultBind(Box<TypedNode>, Box<TypedNode>),
    Compose(ComposeFlavor, Box<TypedNode>, Box<TypedNode>),
    ListNil,
    ListCons(Box<TypedNode>, Box<TypedNode>),
    ListLiteral(Vec<TypedNode>),
    TupleLiteral(Vec<TypedNode>),
    InterpolatedStr(Vec<TypedInterpolatedPart>),
    If(Box<TypedNode>, Box<TypedNode>, Option<Box<TypedNode>>),
    Assert(Box<TypedNode>, Box<TypedNode>),
    Ensure(Box<TypedNode>, Box<TypedNode>, Box<TypedNode>),
    Match(Box<TypedNode>, Vec<(TypedMatchPattern, TypedNode)>),

    /// Field access — field name resolved to index by Scar
    FieldAccess(Box<TypedNode>, u32),

    /// Compile-time lens constant path value. Stage 1 does not allow
    /// first-class runtime transport of lens values.
    LensPath(TypedLensPath),

    /// Lens view application with compile-time path metadata.
    LensView {
        source: Box<TypedNode>,
        path: TypedLensPath,
        source_is_result: bool,
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

    /// Captured function / partial application
    Capture(Box<TypedNode>, Vec<TypedNode>),

    /// Struct definition — tag + name + field names (for TypeRegistry)
    StructDef(u32, String, Vec<String>),

    /// Record definition — tag + name + field names (for TypeRegistry)
    RecordDef(u32, String, Vec<String>),

    /// Semicolon — explicit Unit coercion
    Semi(Box<TypedNode>),
}

/// Interpolated string fragment (typed).
#[derive(Debug, Clone, PartialEq)]
pub enum TypedInterpolatedPart {
    Text(String),
    Expr(Box<TypedNode>),
}

/// Pattern in a binding (typed).
#[derive(Debug, Clone, PartialEq)]
pub enum TypedPattern {
    Var(Ty, ResolvedId),
    As(Ty, Box<TypedPattern>, ResolvedId),
    Wildcard(Ty),
    ListNil(Ty),
    ListCons(Ty, Box<TypedPattern>, Box<TypedPattern>),
    IntLit(Ty, SurtrInt),
    StrLit(Ty, String),
    BoolLit(Ty, bool),
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
#[derive(Debug, Clone, PartialEq)]
pub enum TypedMatchPattern {
    Binding(ResolvedId),
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

/// Function parameter (typed).
#[derive(Debug, Clone, PartialEq)]
pub struct TypedFunParam {
    pub id: ResolvedId,
    pub ty: Ty,
}

/// Typed closure parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedClosureParam {
    pub id: ResolvedId,
    pub ty: Ty,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedEnumVariantDef {
    pub tag: u32,
    pub constructor_name: String,
    pub field_names: Vec<String>,
}
