/// Source location — attached to every AST node for downstream error reporting.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// A plain identifier string. Kept as its own type for readability.
pub type Symbol = String;

// ── Literals ──

#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Unit,
}

// ── Binary operators ──

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    Concat,
}

// ── Type annotations (surface syntax) ──

#[derive(Debug, Clone, PartialEq)]
pub enum AstTy {
    /// `Int`, `String`, `Boolean`, `Unit`, `User`, ...
    Named(Span, Symbol),
    /// `[Int]`, `[String]`, ...
    ListOf(Span, Box<AstTy>),
    /// `Result<Int>` or `Result<Int, ParseError>`
    ResultOf(Span, Box<AstTy>, Option<Box<AstTy>>),
}

// ── Patterns ──

#[derive(Debug, Clone, PartialEq)]
pub enum AstPattern {
    /// `x`
    Var(Span, Symbol),
    /// `x: Int`
    Annotated(Span, Symbol, AstTy),
    /// `_`
    Wildcard(Span),
}

// ── Match patterns ──

#[derive(Debug, Clone, PartialEq)]
pub enum AstMatchPattern {
    /// `True` / `False`
    BoolLit(Span, bool),
    /// `Ok(val)` / `Err(e)` — constructor with optional inner binding
    Constructor(Span, Symbol, Option<Symbol>),
}

// ── Struct / Record fields ──

#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: Symbol,
    pub ty: AstTy,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordField {
    pub name: Symbol,
    pub ty: AstTy,
    pub span: Span,
}

/// Record literal argument — positional or named.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordLitArg {
    Positional(Ast),
    Named(Symbol, Ast),
}

// ── AST ──

#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    /// Literal value: `42`, `"hello"`, `True`, `()`
    Lit(Span, Lit),

    /// Variable reference: `x`, `print`
    Var(Span, Symbol),

    /// Function application: `print("hello")`, `to_string(42)`
    App(Span, Box<Ast>, Vec<Ast>),

    /// Block of statements (implicit in top-level, explicit in `{}`)
    Block(Span, Vec<Ast>),

    /// Binding: `x = 10`, `num: Int = 42`
    Bind(Span, AstPattern, Box<Ast>),

    /// Binary operation: `a + b`, `x == y`
    BinOp(Span, BinOp, Box<Ast>, Box<Ast>),

    /// List literal: `[1, 2, 3]`, `[]`
    List(Span, Vec<Ast>),

    /// Match expression
    Match(Span, Box<Ast>, Vec<(AstMatchPattern, Ast)>),

    /// Field access: `user.name`
    FieldAccess(Span, Box<Ast>, Symbol),

    /// Struct definition: `defstruct User { name: String, age: Int }`
    StructDef(Span, Symbol, Vec<StructField>),

    /// Record definition: `defrecord Point(x: Float, y: Float)`
    RecordDef(Span, Symbol, Vec<RecordField>),

    /// Struct literal: `User { name: "alice", age: 30 }`
    StructLit(Span, Symbol, Vec<(Symbol, Ast)>),

    /// Record literal: `Point(1.0, 2.0)` or `Point(x: 1.0, y: 2.0)`
    RecordLit(Span, Symbol, Vec<RecordLitArg>),

    /// Error type definition: `deferror ParseError(term: String) { "..." }`
    DeferrorDef(Span, Symbol, Vec<RecordField>, Box<Ast>),

    /// Semicolon — explicit Unit coercion marker (wraps the discarded expr)
    Semi(Span, Box<Ast>),
}
