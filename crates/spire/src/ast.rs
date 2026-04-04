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
    /// `List<Int>`, `List<String>`, ...
    ListOf(Span, Box<AstTy>),
    /// `Result<Int>` or `Result<Int, ParseError>`
    ResultOf(Span, Box<AstTy>, Option<Box<AstTy>>),
    /// `(-> T)`, `(A -> B)`, `(A, B -> C)`
    Func(Span, Vec<AstTy>, Box<AstTy>),
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
    /// `[]`
    ListNil(Span),
    /// `[head, ..tail]`
    ListCons(Span, Box<AstPattern>, Box<AstPattern>),
    /// Integer literal in pattern position.
    IntLit(Span, i64),
    /// String literal in pattern position.
    StrLit(Span, String),
    /// Boolean literal in pattern position.
    BoolLit(Span, bool),
    /// `Ok(inner)` in safe-bind patterns
    Constructor(Span, Symbol, Box<AstPattern>),
}

// ── Match patterns ──

#[derive(Debug, Clone, PartialEq)]
pub enum AstMatchPattern {
    /// `x` inside pattern substructure
    Binding(Span, Symbol),
    /// `_`
    Wildcard(Span),
    /// `True` / `False`
    BoolLit(Span, bool),
    /// Integer literal
    IntLit(Span, i64),
    /// String literal
    StrLit(Span, String),
    /// `Ok(val)` / `Err(e)` — constructor with optional inner binding
    Constructor(Span, Symbol, Option<Symbol>),
    /// `[]`
    ListNil(Span),
    /// `[head, ..tail]`
    ListCons(Span, Box<AstMatchPattern>, Box<AstMatchPattern>),
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

/// Function parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct FunParam {
    pub name: Symbol,
    pub ty: AstTy,
    pub span: Span,
}

/// Closure parameter — type is inferred from the expected function signature.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosureParam {
    pub name: Symbol,
    pub span: Span,
}

/// Record literal argument — positional or named.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordLitArg {
    Positional(Ast),
    Named(Symbol, Ast),
}

/// Interpolated string fragment.
#[derive(Debug, Clone, PartialEq)]
pub enum InterpolatedPart {
    Text(String),
    Expr(Box<Ast>),
}

// ── AST ──

#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    /// Literal value: `42`, `"hello"`, `True`, `()`
    Lit(Span, Lit),

    /// Variable reference: `x`, `print`
    Var(Span, Symbol),

    /// Function application: `print("hello")`, `to_string(42)`, `add(y: 2, x: 1)`
    App(Span, Box<Ast>, Vec<RecordLitArg>),

    /// Block of statements (implicit in top-level, explicit in `{}`)
    Block(Span, Vec<Ast>),

    /// Binding: `x = 10`, `num: Int = 42`
    Bind(Span, AstPattern, Box<Ast>),

    /// Safe bind: `x =? expr` — unwrap `Ok(x)`, propagate `Err` early
    SafeBind(Span, AstPattern, Box<Ast>),

    /// Binary operation: `a + b`, `x == y`
    BinOp(Span, BinOp, Box<Ast>, Box<Ast>),

    /// Empty list literal: `[]`
    ListNil(Span),

    /// Cons-style list construction: `[head, ..tail]`
    ListCons(Span, Box<Ast>, Box<Ast>),

    /// Fixed list literal: `[1, 2, 3]`
    ListLiteral(Span, Vec<Ast>),

    /// Interpolated string: `"hi #{name}"`
    InterpolatedStr(Span, Vec<InterpolatedPart>),

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

    /// Constructor call: `Point(1.0, 2.0)` or `Point(x: 1.0, y: 2.0)`
    ConstructorCall(Span, Symbol, Vec<RecordLitArg>),

    /// Error type definition: `deferror ParseError(term: String) { "..." }`
    DeferrorDef(Span, Symbol, Vec<RecordField>, Box<Ast>),

    /// Function definition: `def add(x: Int, y: Int) -> Int { x + y }`
    Def(Span, Symbol, Vec<FunParam>, Option<AstTy>, Box<Ast>),

    /// Builtin declaration: `@@builtin def print(a: String) -> Unit`
    BuiltinDecl(Span, Symbol, Vec<FunParam>, Option<AstTy>),

    /// Closure literal: `{|x, y| expr}` / `{|| expr}`
    Closure(Span, Vec<ClosureParam>, Box<Ast>),

    /// Captured function / partial application: `&print` / `&print(x)`
    Capture(Span, Box<Ast>, Vec<Ast>),

    /// Semicolon — explicit Unit coercion marker (wraps the discarded expr)
    Semi(Span, Box<Ast>),
}
