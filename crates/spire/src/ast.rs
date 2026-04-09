use sindr::primitives::SurtrInt;

/// Source location — attached to every AST node for downstream error reporting.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// A plain identifier string. Kept as its own type for readability.
pub type Symbol = String;

/// Attributes attached to a declaration.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeclAttrs {
    pub doc: Option<String>,
}

/// Surface builtin type head declaration: `List<$A>`, `Result<$T>`, `Int`, ...
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinTypeHead {
    pub span: Span,
    pub name: Symbol,
    pub params: Vec<Symbol>,
}

// ── Literals ──

#[derive(Debug, Clone, PartialEq)]
pub enum Lit {
    Int(SurtrInt),
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
    /// `List<T>`, `Result<T, E>`, user-defined generic types, ...
    Generic(Span, Symbol, Vec<AstTy>),
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
    IntLit(Span, SurtrInt),
    /// String literal in pattern position.
    StrLit(Span, String),
    /// Boolean literal in pattern position.
    BoolLit(Span, bool),
    /// `Ok(inner)` in safe-bind patterns
    Constructor(Span, Symbol, Box<AstPattern>),
    /// `inner @ alias` / `inner @ alias: Ty`
    As(Span, Box<AstPattern>, Symbol, Option<AstTy>),
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
    pub ty: Option<AstTy>,
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

/// Qualified path: `Mod`, `Mod::Type`, `Mod::fun`.
#[derive(Debug, Clone, PartialEq)]
pub struct AstPath {
    pub span: Span,
    pub segments: Vec<Symbol>,
}

/// Import selector.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportSpec {
    /// `import Mod;`
    All,
    /// `import Mod::fun1;`
    Single(Symbol),
    /// `import Mod::{fun1, fun2};`
    List(Vec<Symbol>),
}

// ── AST ──

#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    /// Literal value: `42`, `"hello"`, `True`, `()`
    Lit(Span, Lit),

    /// Variable reference: `x`, `print`
    Var(Span, Symbol),

    /// Qualified path reference: `Kernel::add`
    Path(Span, AstPath),

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
    Match(Span, Box<Ast>, Vec<(AstPattern, Ast)>),

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
    DeferrorDef(Span, Symbol, Vec<RecordField>, Box<Ast>, DeclAttrs),

    /// Function definition: `def add(x: Int, y: Int) -> Int { x + y }`
    Def(
        Span,
        Symbol,
        Vec<FunParam>,
        Option<AstTy>,
        Box<Ast>,
        DeclAttrs,
    ),

    /// Builtin declaration: `@@builtin def print(a: String) -> Unit`
    BuiltinDecl(Span, Symbol, Vec<FunParam>, Option<AstTy>, DeclAttrs),

    /// Builtin type declaration: `@@builtin type Int`
    BuiltinTypeDecl(Span, BuiltinTypeHead, DeclAttrs),

    /// Declaration-only Result constructor contracts used by std modules.
    ///
    /// Surface syntax is intentionally special-cased:
    /// `@@builtin type Ok($T) -> Result<$T>`
    /// `@@builtin type Err(Error) -> Result<$T>`
    ///
    /// These are not real type declarations, but this syntax keeps them in the
    /// same declaration layer as the other std-module builtin contracts.
    ResultCtorDecl(Span, Symbol, AstTy, AstTy, DeclAttrs),

    /// Module declaration: `defmod Kernel { ... }`
    Defmod(Span, Symbol, Vec<Ast>, DeclAttrs),

    /// Import declaration
    Import(Span, AstPath, ImportSpec),

    /// Closure literal: `{|x, y| expr}` / `{|| expr}`
    Closure(Span, Vec<ClosureParam>, Box<Ast>),

    /// Captured function / partial application: `&print` / `&print(x)`
    Capture(Span, Box<Ast>, Vec<Ast>),

    /// Semicolon — explicit Unit coercion marker (wraps the discarded expr)
    Semi(Span, Box<Ast>),
}

#[cfg(test)]
mod tests {
    use super::{Ast, AstPath, DeclAttrs, ImportSpec, Lit, Span};
    use sindr::primitives::int;

    #[test]
    fn import_forms_are_distinct_in_ast() {
        let span = Span { start: 0, end: 12 };
        let mod_path = AstPath {
            span: span.clone(),
            segments: vec!["Kernel".to_string()],
        };
        let all = Ast::Import(span.clone(), mod_path.clone(), ImportSpec::All);
        let single = Ast::Import(
            span.clone(),
            mod_path.clone(),
            ImportSpec::Single("add".to_string()),
        );
        let list = Ast::Import(
            span.clone(),
            mod_path,
            ImportSpec::List(vec!["add".to_string(), "sub".to_string()]),
        );

        assert!(matches!(all, Ast::Import(_, _, ImportSpec::All)));
        assert!(matches!(single, Ast::Import(_, _, ImportSpec::Single(_))));
        assert!(matches!(list, Ast::Import(_, _, ImportSpec::List(_))));
    }

    #[test]
    fn defmod_keeps_body_nodes() {
        let span = Span { start: 0, end: 20 };
        let body = vec![Ast::Lit(span.clone(), Lit::Int(int(1)))];
        let node = Ast::Defmod(
            span,
            "Kernel".to_string(),
            body.clone(),
            DeclAttrs::default(),
        );

        match node {
            Ast::Defmod(_, name, inner, attrs) => {
                assert_eq!(name, "Kernel");
                assert_eq!(inner, body);
                assert_eq!(attrs, DeclAttrs::default());
            }
            _ => panic!("expected defmod"),
        }
    }
}
