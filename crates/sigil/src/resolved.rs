use sindr::primitives::SurtrInt;
use spire::ast::{AstTy, BinOp, Lit, Span, Symbol};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedDeclAttrs {
    pub doc: Option<String>,
}

/// A resolved identifier — name + unique id + source location.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedId {
    pub name: Symbol,
    pub qualified_name: Option<Symbol>,
    pub unique_id: u32,
    pub span: Span,
}

/// Resolved AST — every identifier carries a unique_id.
#[derive(Debug, Clone, PartialEq)]
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

    /// Kleisli composition
    KleisliCompose(Span, Box<Resolved>, Box<Resolved>),

    /// Empty list literal
    ListNil(Span),

    /// Cons-style list construction
    ListCons(Span, Box<Resolved>, Box<Resolved>),

    /// Fixed list literal
    ListLiteral(Span, Vec<Resolved>),

    /// Tuple literal
    TupleLiteral(Span, Vec<Resolved>),

    /// Interpolated string
    InterpolatedStr(Span, Vec<ResolvedInterpolatedPart>),

    /// `if(flag, then, else)` / `if_then(flag, then)` special form
    If(Span, Box<Resolved>, Box<Resolved>, Option<Box<Resolved>>),

    /// `assert(flag, err)` special form
    Assert(Span, Box<Resolved>, Box<Resolved>),

    /// `ensure(value, pred, err)` special form
    Ensure(Span, Box<Resolved>, Box<Resolved>, Box<Resolved>),

    /// Match expression
    Match(Span, Box<Resolved>, Vec<(ResolvedPattern, Resolved)>),

    /// Field access: `expr.field`
    FieldAccess(Span, Box<Resolved>, Symbol),

    /// Struct literal: `User { name: "alice", age: 30 }`
    StructLit(Span, ResolvedId, Vec<(Symbol, Resolved)>),

    /// Constructor call: `Point(1.0, 2.0)`
    ConstructorCall(Span, ResolvedId, Vec<ResolvedRecordLitArg>),

    /// Compiler-synthesized target-type witness used only for conversion
    /// surfaces such as `from(value, String)`.
    TypeRefWitness(Span, AstTy),

    /// Struct definition (passed through for Scar)
    StructDef(Span, ResolvedId, Vec<ResolvedField>),

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
    TraitImplDef(Span, ResolvedId, Vec<AstTy>, AstTy, Vec<ResolvedTraitImplMethod>),

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
    /// `@@builtin type Ok(...) -> Result<...>` / `@@builtin type Err(...) -> Result<...>`
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

    /// Captured function / partial application
    Capture(Span, Box<Resolved>, Vec<Resolved>),

    /// Semicolon — explicit Unit coercion
    Semi(Span, Box<Resolved>),
}

/// Interpolated string fragment (resolved).
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedInterpolatedPart {
    Text(String),
    Expr(Box<Resolved>),
}

/// Pattern in a binding (resolved).
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedPattern {
    Var(ResolvedId),
    Annotated(ResolvedId, AstTy),
    Wildcard(Span),
    ListNil(Span),
    ListCons(Box<ResolvedPattern>, Box<ResolvedPattern>),
    IntLit(Span, SurtrInt),
    StrLit(Span, String),
    BoolLit(Span, bool),
    Constructor(ResolvedId, Vec<ResolvedPattern>),
    Extractor(ResolvedId, Vec<ResolvedPattern>),
    Tuple(Vec<ResolvedPattern>),
    As(Box<ResolvedPattern>, ResolvedId, Option<AstTy>),
}

/// Record literal argument (resolved).
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedRecordLitArg {
    Positional(Resolved),
    Named(Symbol, Resolved),
}

/// Field definition (resolved) — used in StructDef / RecordDef / DeferrorDef.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedField {
    pub id: Option<ResolvedId>,
    pub name: Symbol,
    pub ty: AstTy,
    pub span: Span,
}

/// Function parameter (resolved).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFunParam {
    pub id: ResolvedId,
    pub ty: AstTy,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedExtractorParam {
    pub id: ResolvedId,
    pub ty: Option<AstTy>,
}

/// Closure parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedClosureParam {
    pub id: ResolvedId,
    pub ty: Option<AstTy>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEnumVariant {
    pub id: ResolvedId,
    pub payload: Vec<AstTy>,
    pub discriminant: Option<SurtrInt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTraitMethodSig {
    pub id: ResolvedId,
    pub type_params: Vec<ResolvedTypeParam>,
    pub params: Vec<ResolvedFunParam>,
    pub ret_ty: AstTy,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTraitImplMethod {
    pub method_name: Symbol,
    pub function_id: ResolvedId,
    pub type_params: Vec<ResolvedTypeParam>,
    pub params: Vec<ResolvedFunParam>,
    pub ret_ty: Option<AstTy>,
    pub body: Box<Resolved>,
    pub attrs: ResolvedDeclAttrs,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTypeParam {
    pub name: Symbol,
    pub bound: Option<Symbol>,
    pub span: Span,
}
