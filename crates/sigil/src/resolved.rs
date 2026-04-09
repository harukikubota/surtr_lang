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

    /// Empty list literal
    ListNil(Span),

    /// Cons-style list construction
    ListCons(Span, Box<Resolved>, Box<Resolved>),

    /// Fixed list literal
    ListLiteral(Span, Vec<Resolved>),

    /// Interpolated string
    InterpolatedStr(Span, Vec<ResolvedInterpolatedPart>),

    /// `if(cond, then, else)` / `if_then(cond, then)` special form
    If(Span, Box<Resolved>, Box<Resolved>, Option<Box<Resolved>>),

    /// Match expression
    Match(Span, Box<Resolved>, Vec<(ResolvedPattern, Resolved)>),

    /// Field access: `expr.field`
    FieldAccess(Span, Box<Resolved>, Symbol),

    /// Struct literal: `User { name: "alice", age: 30 }`
    StructLit(Span, ResolvedId, Vec<(Symbol, Resolved)>),

    /// Constructor call: `Point(1.0, 2.0)`
    ConstructorCall(Span, ResolvedId, Vec<ResolvedRecordLitArg>),

    /// Struct definition (passed through for Scar)
    StructDef(Span, ResolvedId, Vec<ResolvedField>),

    /// Record definition (passed through for Scar)
    RecordDef(Span, ResolvedId, Vec<ResolvedField>),

    /// Error type definition
    DeferrorDef(Span, ResolvedId, Vec<ResolvedField>, Box<Resolved>),

    /// Function definition
    Def(
        Span,
        ResolvedId,
        Vec<ResolvedFunParam>,
        Option<AstTy>,
        Box<Resolved>,
        ResolvedDeclAttrs,
    ),

    /// Builtin declaration
    BuiltinDecl(
        Span,
        ResolvedId,
        Vec<ResolvedFunParam>,
        Option<AstTy>,
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
    Constructor(ResolvedId, Box<ResolvedPattern>),
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

/// Closure parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedClosureParam {
    pub id: ResolvedId,
    pub ty: Option<AstTy>,
}
