use serde::{Deserialize, Serialize};
use sindr::primitives::SurtrInt;
use spire::ast::{AstTy, BinOp, Lit, ProcessSpec, Span, Symbol, Visibility};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedDeclAttrs {
    pub doc: Option<String>,
    pub hidden: bool,
    pub readonly: bool,
    pub visibility: Visibility,
    pub user_importable: bool,
    pub user_callable: bool,
}

impl Default for ResolvedDeclAttrs {
    fn default() -> Self {
        Self {
            doc: None,
            hidden: false,
            readonly: false,
            visibility: Visibility::Public,
            user_importable: true,
            user_callable: true,
        }
    }
}

/// A resolved identifier — name + unique id + source location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedId {
    pub name: Symbol,
    pub qualified_name: Option<Symbol>,
    pub unique_id: u32,
    pub compiler_generated: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedProcessSpec {
    pub module_path: String,
    pub process_name: String,
    pub spec: ProcessSpec,
    pub init_uid: u32,
    pub get_uid: u32,
    pub set_uid: Option<u32>,
    pub handler_uids: Vec<ResolvedProcessHandlerUid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedProcessHandlerUid {
    pub internal_name: Symbol,
    pub uid: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ResolvedStructLitField {
    Explicit(Symbol, Resolved),
    Shorthand(Symbol, Resolved),
}

/// Resolved AST — every identifier carries a unique_id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Resolved {
    /// Literal value
    Lit(Span, Lit),

    /// Variable reference (resolved)
    Var(Span, ResolvedId),

    /// Function application
    App(Span, Box<Resolved>, Vec<ResolvedRecordLitArg>),

    /// Block of statements
    Block(Span, Vec<Resolved>),

    /// Binding: `x = expr`
    Bind(Span, ResolvedPattern, Box<Resolved>),

    /// Safe bind: `x =? expr` — unwrap `Ok(x)`, propagate `Err` early
    SafeBind(Span, ResolvedPattern, Box<Resolved>),

    /// Binary operation
    BinOp(Span, BinOp, Box<Resolved>, Box<Resolved>),

    /// Value pipe
    Pipe(Span, Box<Resolved>, Box<Resolved>),

    /// Context map
    ContextMap(Span, Box<Resolved>, Box<Resolved>),

    /// Context bind
    ContextBind(Span, Box<Resolved>, Box<Resolved>),

    /// Plain function composition
    Compose(Span, Box<Resolved>, Box<Resolved>),

    /// Lifted composition
    LiftedCompose(Span, Box<Resolved>, Box<Resolved>),

    /// Kleisli composition
    KleisliCompose(Span, Box<Resolved>, Box<Resolved>),

    /// Empty list literal
    ListNil(Span),

    /// Cons-style list construction
    ListCons(Span, Box<Resolved>, Box<Resolved>),

    /// Fixed list literal
    ListLiteral(Span, Vec<Resolved>),

    /// Inclusive range literal
    RangeLiteral(Span, Box<Resolved>, Box<Resolved>),

    /// Tuple literal
    TupleLiteral(Span, Vec<Resolved>),

    /// Parenthesized expression preserved for operator RHS disambiguation.
    Grouped(Span, Box<Resolved>),

    /// Interpolated string
    InterpolatedStr(Span, Vec<ResolvedInterpolatedPart>),

    /// `dbg!(expr1, expr2, ...)` special form
    Dbg(Span, Vec<Resolved>),

    /// `if(flag, then, else)` / `if_then(flag, then)` special form
    If(Span, Box<Resolved>, Box<Resolved>, Option<Box<Resolved>>),

    /// `assert(flag, err)` special form
    Assert(Span, Box<Resolved>, Box<Resolved>),

    /// `ensure(value, pred, err)` special form
    Ensure(Span, Box<Resolved>, Box<Resolved>, Box<Resolved>),

    /// `Result::map_err(value, err)` special form
    MapErr(Span, Box<Resolved>, Box<Resolved>),

    /// `Result::cause(value, err)` special form
    Cause(Span, Box<Resolved>, Box<Resolved>),

    /// `Result::recover_kind(value, ErrorKind, handler)` special form
    RecoverKind(Span, Box<Resolved>, Box<Resolved>, Box<Resolved>),

    /// Match expression
    Match(Span, Box<Resolved>, Vec<ResolvedMatchArm>),

    /// Field access: `expr.field`
    FieldAccess(Span, Box<Resolved>, Symbol),

    /// Inferred field/facet capture: `_.field` / `_.field.subfield`
    InferredFacetCapture(Span, Vec<Symbol>),

    /// Compiler-managed Facet shorthand capture: `~source.path`
    FacetCapture(Span, Box<Resolved>),

    /// Process-local handler dependency access: `ctx.<slot>`.
    ProcessContextHandler(Span, Symbol),

    /// Struct literal: `User { name: "alice", age, active: is_active }`
    StructLit(Span, ResolvedId, Vec<ResolvedStructLitField>),

    /// Constructor call: `Point(1.0, 2.0)`
    ConstructorCall(Span, ResolvedId, Vec<ResolvedRecordLitArg>),

    /// Compiler-synthesized target-type witness used only for conversion
    /// surfaces such as `from(value, String)`.
    TypeRefWitness(Span, AstTy),

    /// Struct definition (passed through for Scar)
    StructDef(Span, ResolvedId, Vec<ResolvedField>, ResolvedDeclAttrs),

    /// Record definition (passed through for Scar)
    RecordDef(Span, ResolvedId, Vec<ResolvedField>),

    /// Error type definition
    DeferrorDef(Span, ResolvedId, Vec<ResolvedField>, Box<Resolved>),

    /// Enum definition
    EnumDef(
        Span,
        ResolvedId,
        Vec<ResolvedTypeParam>,
        Vec<ResolvedEnumVariant>,
        ResolvedDeclAttrs,
    ),

    /// Function definition
    Def(
        Span,
        ResolvedId,
        Vec<ResolvedTypeParam>,
        Vec<ResolvedFunParam>,
        Option<AstTy>,
        Box<Resolved>,
        ResolvedDeclAttrs,
    ),

    /// Top-level constant definition.
    ConstDef(
        Span,
        ResolvedId,
        Option<AstTy>,
        Box<Resolved>,
        ResolvedDeclAttrs,
    ),

    ExtractorDef(
        Span,
        ResolvedId,
        Vec<ResolvedTypeParam>,
        ResolvedExtractorParam,
        AstTy,
        Box<Resolved>,
        ResolvedDeclAttrs,
    ),

    /// Trait definition
    TraitDef(
        Span,
        ResolvedId,
        Vec<ResolvedTypeParam>,
        Vec<ResolvedTraitMethodSig>,
        ResolvedDeclAttrs,
    ),

    /// Trait impl definition
    TraitImplDef(
        Span,
        ResolvedId,
        Vec<AstTy>,
        AstTy,
        Vec<ResolvedTraitImplMethod>,
    ),

    /// Builtin declaration
    BuiltinDecl(
        Span,
        ResolvedId,
        Vec<ResolvedFunParam>,
        Option<AstTy>,
        ResolvedDeclAttrs,
    ),

    BuiltinExtractorDecl(
        Span,
        ResolvedId,
        ResolvedExtractorParam,
        AstTy,
        ResolvedDeclAttrs,
    ),

    /// Builtin type declaration
    BuiltinTypeDecl(Span, ResolvedId, Vec<Symbol>, ResolvedDeclAttrs),

    /// Declaration-only Result constructor contract from std modules.
    ///
    /// The parser accepts the surface form
    /// `@builtin type Ok(...) -> Result<...>` / `@builtin type Err(...) -> Result<...>`
    /// and normalizes both into this resolved node so later phases do not need
    /// to care about the parser-only spelling trick.
    ResultCtorDecl(Span, ResolvedId, AstTy, AstTy, ResolvedDeclAttrs),

    /// Closure literal
    Closure(
        Span,
        Vec<ResolvedClosureParam>,
        Vec<ResolvedId>,
        Box<Resolved>,
    ),

    /// Captured function value
    Capture(Span, Box<Resolved>, Vec<Resolved>),

    /// Semicolon — explicit Unit coercion
    Semi(Span, Box<Resolved>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedMatchArm {
    pub pattern: ResolvedPattern,
    pub guard: Option<Resolved>,
    pub body: Resolved,
}

/// Interpolated string fragment (resolved).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ResolvedInterpolatedPart {
    Text(String),
    Expr(Box<Resolved>),
}

/// Pattern in a binding (resolved).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ResolvedPattern {
    Var(ResolvedId),
    Annotated(ResolvedId, AstTy),
    Wildcard(Span),
    ListNil(Span),
    ListCons(Box<ResolvedPattern>, Box<ResolvedPattern>),
    IntLit(Span, SurtrInt),
    StrLit(Span, String),
    BoolLit(Span, bool),
    DurationLit(Span, SurtrInt),
    Constructor(ResolvedId, Vec<ResolvedPattern>),
    Extractor(ResolvedId, Vec<ResolvedPattern>),
    Tuple(Vec<ResolvedPattern>),
    Or(Vec<ResolvedPattern>),
    As(Box<ResolvedPattern>, ResolvedId, Option<AstTy>),
}

/// Record literal argument (resolved).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ResolvedRecordLitArg {
    Positional(Resolved),
    Named(Symbol, Resolved),
}

/// Field definition (resolved) — used in StructDef / RecordDef / DeferrorDef.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedField {
    pub id: Option<ResolvedId>,
    pub name: Symbol,
    pub ty: AstTy,
    pub span: Span,
    pub visibility: Visibility,
    pub readonly: bool,
}

/// Function parameter (resolved).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedFunParam {
    pub id: ResolvedId,
    pub ty: AstTy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedExtractorParam {
    pub id: ResolvedId,
    pub ty: Option<AstTy>,
}

/// Closure parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedClosureParam {
    pub id: ResolvedId,
    pub ty: Option<AstTy>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedEnumVariant {
    pub id: ResolvedId,
    pub payload: Vec<AstTy>,
    pub discriminant: Option<SurtrInt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedTraitMethodSig {
    pub id: ResolvedId,
    pub type_params: Vec<ResolvedTypeParam>,
    pub params: Vec<ResolvedFunParam>,
    pub ret_ty: AstTy,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedTraitImplMethod {
    pub method_name: Symbol,
    pub function_id: ResolvedId,
    pub type_params: Vec<ResolvedTypeParam>,
    pub params: Vec<ResolvedFunParam>,
    pub ret_ty: Option<AstTy>,
    pub body: Box<Resolved>,
    pub attrs: ResolvedDeclAttrs,
    pub span: Span,
    pub is_builtin: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedTypeParam {
    pub name: Symbol,
    pub bound: Option<Symbol>,
    pub span: Span,
}
