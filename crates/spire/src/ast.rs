use serde::{Deserialize, Serialize};
use sindr::primitives::SurtrInt;

/// Source location — attached to every AST node for downstream error reporting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// A plain identifier string. Kept as its own type for readability.
pub type Symbol = String;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    #[default]
    Public,
    Private,
}

/// Attributes attached to a declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclAttrs {
    pub doc: Option<String>,
    pub builtin: bool,
    pub compiler_generated: bool,
    /// Traits requested by a single `@derive` annotation.
    pub derives: Vec<Symbol>,
    /// Compiler declaration for a Facet path kind. These declarations are
    /// accepted only from the canonical standard-library Facet source.
    pub facet_path_kind: Option<Vec<Symbol>>,
    pub auto_import: bool,
    pub hidden: bool,
    pub readonly: bool,
    pub visibility: Visibility,
    pub user_importable: bool,
    pub user_callable: bool,
    /// Impl-method dispatch slots, populated only for trait impl methods.
    pub return_type_arguments: Vec<ReturnTypeArgument>,
}

impl Default for DeclAttrs {
    fn default() -> Self {
        Self {
            doc: None,
            builtin: false,
            compiler_generated: false,
            derives: Vec::new(),
            facet_path_kind: None,
            auto_import: false,
            hidden: false,
            readonly: false,
            visibility: Visibility::Public,
            user_importable: true,
            user_callable: true,
            return_type_arguments: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessKind {
    Agent,
    GenServer,
    Supervisor,
    RuntimeSupervisor,
    DynamicSupervisor,
    Task,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessInstance {
    Singleton,
    Worker,
}

/// Compiler-managed metadata carried by lowered `defagent` modules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub process_name: Symbol,
    pub kind: ProcessKind,
    pub instance: ProcessInstance,
    pub state: AstTy,
    pub boot: bool,
    pub registry: bool,
    pub standby: bool,
    pub handlers: Vec<ProcessHandlerDependency>,
    pub handler_specs: Vec<ProcessRuntimeHandlerSpec>,
    #[serde(default)]
    pub supervisor_policy: Option<SupervisorPolicy>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessHandlerDependency {
    pub slot: Symbol,
    pub capability: Symbol,
    pub default_target: ProcessHandlerTarget,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessHandlerTarget {
    pub name: Symbol,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessRuntimeHandlerKind {
    Init,
    Get,
    Set,
    Call,
    Cast,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessRuntimeHandlerSpec {
    pub name: Symbol,
    #[serde(default)]
    pub internal_name: Symbol,
    pub kind: ProcessRuntimeHandlerKind,
    pub span: Span,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SupervisorInitSpec {
    #[serde(default)]
    pub entries: Vec<SupervisorInitEntry>,
    #[serde(default)]
    pub singletons: Vec<SupervisorInitSingleton>,
    #[serde(default)]
    pub supervisors: Vec<SupervisorInitOverride>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SupervisorInitEntry {
    pub process_name: Symbol,
    pub timeout_ms: Option<u64>,
    pub handlers: Vec<SupervisorInitHandlerOverride>,
    pub overrides: SupervisorPolicyOverride,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SupervisorInitSingleton {
    pub process_name: Symbol,
    pub timeout_ms: Option<u64>,
    pub handlers: Vec<SupervisorInitHandlerOverride>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SupervisorInitHandlerOverride {
    pub slot: Symbol,
    pub target: SupervisorInitHandlerTarget,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SupervisorInitHandlerTarget {
    pub name: Symbol,
    pub named_args: Vec<SupervisorInitHandlerArg>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SupervisorInitHandlerArg {
    pub name: Symbol,
    pub value: String,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupervisorStrategy {
    OneForOne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChildRestartPolicy {
    Permanent,
    Transient,
    Temporary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorPolicy {
    pub strategy: SupervisorStrategy,
    pub max_restarts: u64,
    pub max_seconds: u64,
    pub child_restart_default: ChildRestartPolicy,
    pub allow_adopt: bool,
    #[serde(default)]
    pub shutdown_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorPolicyOverride {
    #[serde(default)]
    pub strategy: Option<SupervisorStrategy>,
    #[serde(default)]
    pub max_restarts: Option<u64>,
    #[serde(default)]
    pub max_seconds: Option<u64>,
    #[serde(default)]
    pub child_restart_default: Option<ChildRestartPolicy>,
    #[serde(default)]
    pub allow_adopt: Option<bool>,
    #[serde(default)]
    pub shutdown_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorInitOverride {
    pub process_name: Symbol,
    pub overrides: SupervisorPolicyOverride,
    pub span: Span,
}

/// Surface builtin type head declaration: `List<$A>`, `Result<$T>`, `Int`, ...
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinTypeHead {
    pub span: Span,
    pub name: Symbol,
    pub params: Vec<Symbol>,
}

// ── Literals ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Lit {
    Int(SurtrInt),
    Float(f64),
    Str(String),
    Bool(bool),
    Unit,
}

// ── Binary operators ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Slash,
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    Concat,
    Choice,
}

// ── Type annotations (surface syntax) ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AstTy {
    /// `Int`, `String`, `Boolean`, `Unit`, `User`, ...
    Named(Span, Symbol),
    /// `impl Describable`
    ImplTrait(Span, Symbol),
    /// `List<T>`, `Result<T, E>`, user-defined generic types, ...
    Generic(Span, Symbol, Vec<AstTy>),
    /// `(A, B, C)`
    Tuple(Span, Vec<AstTy>),
    /// `(-> T)`, `(A -> B)`, `(A, B -> C)`
    Func(Span, Vec<AstTy>, Box<AstTy>),
}

/// A declaration-level `where` clause.
#[derive(Debug, Clone, PartialEq)]
pub struct WhereClause {
    pub constraints: Vec<WhereConstraint>,
    pub span: Span,
}

/// One constrained subject, for example `$A: Eq + Concat`.
#[derive(Debug, Clone, PartialEq)]
pub struct WhereConstraint {
    pub subject: AstTy,
    pub bounds: Vec<WhereConstraintRhs>,
    pub span: Span,
}

/// The right-hand side of a `where` constraint.
#[derive(Debug, Clone, PartialEq)]
pub enum WhereConstraintRhs {
    /// An ordinary bare trait requirement such as `Eq` or `Convert`.
    Trait(Span, Symbol),
    /// A type-constructor shape requirement such as `Type<$A>`.
    TypeConstructor(Span, Vec<AstTy>),
    /// A trait constructor-slot projection such as `Functor.$A`.
    TraitSlot(Span, Symbol, Symbol),
}

// ── Patterns ──

#[derive(Debug, Clone, PartialEq)]
pub enum AstPattern {
    /// `x`
    Var(Span, Symbol),
    /// `x: Int`
    Annotated(Span, Symbol, AstTy),
    /// `^x`
    Pin(Span, Symbol),
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
    /// Duration literal in pattern position, e.g. `20ms`.
    DurationLit(Span, SurtrInt),
    /// `Ok(inner)` / `Color::Red` / `KeyInput::Arrow(dir)` in pattern position.
    Constructor(Span, Symbol, Vec<AstPattern>),
    /// `uncons(head, tail)` / `User(name, age)` in MatchBlock position.
    Call(Span, Symbol, Vec<AstPattern>),
    /// `(head, tail, ...)`
    Tuple(Span, Vec<AstPattern>),
    /// `left | right` inside a pattern.
    Or(Span, Vec<AstPattern>),
    /// `inner @ alias` / `inner @ alias: Ty`
    ///
    /// The final span is the alias identifier token, kept separately from
    /// the full as-pattern span for diagnostics and REPL binding metadata.
    As(Span, Box<AstPattern>, Symbol, Option<AstTy>, Span),
}

/// Match arm: `pattern [when guard] => body`.
#[derive(Debug, Clone, PartialEq)]
pub struct AstMatchArm {
    pub pattern: AstPattern,
    pub guard: Option<Ast>,
    pub body: Ast,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FacetBracketExpr {
    pub expr: Box<Ast>,
    pub display: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FacetPathSegment {
    Field { name: Symbol, optional: bool },
    Bracket(FacetBracketExpr),
}

impl FacetPathSegment {
    pub fn field(name: impl Into<Symbol>) -> Self {
        Self::Field {
            name: name.into(),
            optional: false,
        }
    }

    pub fn optional_field(name: impl Into<Symbol>) -> Self {
        Self::Field {
            name: name.into(),
            optional: true,
        }
    }

    pub fn display_label(&self) -> String {
        match self {
            Self::Field { name, optional } => {
                if *optional {
                    format!("{name}?")
                } else {
                    name.clone()
                }
            }
            Self::Bracket(expr) => format!("[{}]", expr.display),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BulkUpdateEntry {
    pub span: Span,
    pub path: BulkUpdatePath,
    pub kind: BulkUpdateEntryKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BulkUpdatePath {
    Segments(Span, Vec<FacetPathSegment>),
    Pin(Span, Symbol),
    Chain(Span, Box<BulkUpdatePath>, Box<BulkUpdatePath>),
    StripLeft(Span, Box<BulkUpdatePath>, usize),
    StripRight(Span, Box<BulkUpdatePath>, usize),
}

impl BulkUpdatePath {
    pub fn span(&self) -> &Span {
        match self {
            Self::Segments(span, _)
            | Self::Pin(span, _)
            | Self::Chain(span, _, _)
            | Self::StripLeft(span, _, _)
            | Self::StripRight(span, _, _) => span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BulkUpdateEntryKind {
    Set(Ast),
    Over(Ast),
    OverResult(Ast),
    CaseSet(Ast),
    CaseOver(Ast),
    Nested(Vec<BulkUpdateEntry>),
}

// ── Struct / Record fields ──

#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: Symbol,
    pub ty: AstTy,
    pub span: Span,
    pub visibility: Visibility,
    pub readonly: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordField {
    pub name: Symbol,
    pub ty: AstTy,
    pub span: Span,
    pub visibility: Visibility,
    pub readonly: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: Symbol,
    pub payload: Vec<AstTy>,
    pub discriminant: Option<SurtrInt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    pub name: Symbol,
    pub bound: Option<Symbol>,
    pub span: Span,
}

/// A type supplied through declaration or call-site `::<...>` syntax.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnTypeArgument {
    pub ordinal: u32,
    pub ty: AstTy,
    pub span: Span,
}

/// Declaration-side value parameter mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueParameterMode {
    PositionalOrNamed,
    Variadic,
}

/// A declaration-side value parameter written in `(...)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueParameter {
    pub name: Symbol,
    pub mode: ValueParameterMode,
    pub ty: AstTy,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitMethodSig {
    pub name: Symbol,
    /// Explicit dispatch type slots written as `method::<Self, $A>`.
    pub return_type_arguments: Vec<ReturnTypeArgument>,
    pub type_params: Vec<TypeParam>,
    pub value_parameters: Vec<ValueParameter>,
    pub ret_ty: AstTy,
    pub where_clause: Option<WhereClause>,
    pub body: Option<Box<Ast>>,
    pub attrs: DeclAttrs,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractorParam {
    pub name: Symbol,
    pub ty: Option<AstTy>,
    pub span: Span,
}

/// Closure parameter — type is inferred from the expected function signature.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosureParam {
    pub name: Symbol,
    pub ty: Option<AstTy>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DbgArg {
    pub span: Span,
    pub expr: Ast,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HashMapLiteralEntry {
    pub key: Ast,
    pub value: Ast,
}

/// Record literal argument — positional or named.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordLitArg {
    Positional(Ast),
    Named(Symbol, Ast),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructLitField {
    Explicit(Symbol, Ast),
    Shorthand(Symbol),
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

/// Parser-only backtick capture target for operator forms such as `&`+``.
#[derive(Debug, Clone, PartialEq)]
pub struct FuncLiteralRef {
    pub span: Span,
    pub body: Symbol,
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

    /// Compiler-generated hidden builtin reference.
    InternalVar(Span, Symbol),

    /// Qualified path reference: `Kernel::add`
    Path(Span, AstPath),

    /// Parser-only backtick capture target such as `&`+``.
    FuncLiteralRef(Span, FuncLiteralRef),

    /// Function application: `print("hello")`, `to_string(42)`, `add(y: 2, x: 1)`
    App(Span, Box<Ast>, Vec<RecordLitArg>),

    /// Explicit generic-slot application: `identity::<Int>`, `Trait::method::<Int>`
    ReturnTypeArgumentApply(Span, Box<Ast>, Vec<ReturnTypeArgument>),

    /// Statement sequence used by declaration bodies and lowered closure bodies.
    Block(Span, Vec<Ast>),

    /// Binding: `x = 10`, `num: Int = 42`
    Bind(Span, AstPattern, Box<Ast>),

    /// Safe bind: `x =? expr` — unwrap `Ok(x)`, propagate `Err` early
    SafeBind(Span, AstPattern, Box<Ast>),

    /// Binary operation: `a + b`, `x == y`
    BinOp(Span, BinOp, Box<Ast>, Box<Ast>),

    /// Value pipe: `value |> f`
    Pipe(Span, Box<Ast>, Box<Ast>),

    /// Context-preserving map: `value |*> f`
    ContextMap(Span, Box<Ast>, Box<Ast>),

    /// Applicative application: `mapper |*| value`
    ContextApply(Span, Box<Ast>, Box<Ast>),

    /// Context-preserving bind: `value |>= f`
    ContextBind(Span, Box<Ast>, Box<Ast>),

    /// Plain function composition: `f >> g`
    Compose(Span, Box<Ast>, Box<Ast>),

    /// Lifted composition: `f >* g`
    LiftedCompose(Span, Box<Ast>, Box<Ast>),

    /// Kleisli composition: `f >=> g`
    KleisliCompose(Span, Box<Ast>, Box<Ast>),

    /// Empty list literal: `[]`
    ListNil(Span),

    /// Cons-style list construction: `[head, ..tail]`
    ListCons(Span, Box<Ast>, Box<Ast>),

    /// Fixed list literal: `[1, 2, 3]`
    ListLiteral(Span, Vec<Ast>),

    /// Inclusive range literal: `[start..stop]`
    RangeLiteral(Span, Box<Ast>, Box<Ast>),

    /// Tuple literal: `(1, 2, 3)`
    TupleLiteral(Span, Vec<Ast>),

    /// Parenthesized expression preserved for operator RHS disambiguation.
    Grouped(Span, Box<Ast>),

    /// Interpolated string: `"hi #{name}"`
    InterpolatedStr(Span, Vec<InterpolatedPart>),

    /// Debug special form: `dbg!(expr1, expr2, ...)`
    Dbg(Span, Vec<DbgArg>),

    /// Match expression
    Match(Span, Box<Ast>, Vec<AstMatchArm>),

    /// `Facet::bulk_update(source) { ... }` special form.
    BulkUpdate(Span, Box<Ast>, Vec<BulkUpdateEntry>),

    /// Field access: `user.name`, `pair._0`
    FieldAccess(Span, Box<Ast>, Symbol),

    /// Non-identifier Facet path segment, or an identifier segment with an optional marker.
    FacetSegmentAccess(Span, Box<Ast>, FacetPathSegment),

    /// Compiler-managed Facet shorthand capture: `~source.path`
    FacetCapture(Span, Box<Ast>),

    /// Struct definition: `defstruct Box<$A> { value: $A }`
    StructDef(Span, Symbol, Vec<TypeParam>, Vec<StructField>, DeclAttrs),

    /// Record definition: `defrecord Point(x: Float, y: Float)`
    RecordDef(Span, Symbol, Vec<RecordField>, DeclAttrs),

    /// Struct literal: `User { name: "alice", age, active: is_active }`
    StructLit(Span, Symbol, Vec<StructLitField>),

    /// Compiler-generated struct literal used for syntax sugars such as `100ms`.
    InternalStructLit(Span, Symbol, Vec<StructLitField>),

    /// Constructor call: `Point(1.0, 2.0)` or `Point(x: 1.0, y: 2.0)`
    ConstructorCall(Span, Symbol, Vec<RecordLitArg>),

    /// Error type definition: `deferror ParseError(term: String) { "..." }`
    DeferrorDef(Span, Symbol, Vec<RecordField>, Box<Ast>, DeclAttrs),

    /// Enum definition: `defenum Color { Red, Green = 4, Blue(Int) }`
    EnumDef(Span, Symbol, Vec<TypeParam>, Vec<EnumVariant>, DeclAttrs),

    /// Function definition: `def add(x: Int, y: Int) -> Int { x + y }`
    Def(
        Span,
        Symbol,
        Vec<ReturnTypeArgument>,
        Vec<ValueParameter>,
        Option<AstTy>,
        Option<WhereClause>,
        Box<Ast>,
        DeclAttrs,
    ),

    /// Top-level constant definition: `const APP_NAME = "surtr"`
    ConstDef(Span, Symbol, Option<AstTy>, Box<Ast>, DeclAttrs),

    /// Top-level runtime boot configuration block.
    SupervisorInit(Span, SupervisorInitSpec),

    ExtractorDef(
        Span,
        Symbol,
        Vec<TypeParam>,
        ExtractorParam,
        AstTy,
        Box<Ast>,
        DeclAttrs,
    ),

    /// Builtin declaration: `@builtin def print(a: String) -> Unit`
    BuiltinDecl(
        Span,
        Symbol,
        Vec<ReturnTypeArgument>,
        Vec<ValueParameter>,
        Option<AstTy>,
        Option<WhereClause>,
        DeclAttrs,
    ),

    /// Display-only intrinsic declaration: `@intrinsic def dbg!(values: *$A) -> Unit`
    IntrinsicDecl(Span, Symbol, String, DeclAttrs),

    BuiltinExtractorDecl(Span, Symbol, ExtractorParam, AstTy, DeclAttrs),

    /// Builtin type declaration: `@builtin type Int`
    BuiltinTypeDecl(Span, BuiltinTypeHead, DeclAttrs),

    /// Compile-time-only alias for a function signature.
    TypeAlias(Span, Symbol, Vec<TypeParam>, AstTy),

    /// Declaration-only Result constructor contracts used by std modules.
    ///
    /// Surface syntax is intentionally special-cased:
    /// `@builtin type Ok($T) -> Result<$T>`
    /// `@builtin type Err(Error) -> Result<$T>`
    ///
    /// These are not real type declarations, but this syntax keeps them in the
    /// same declaration layer as the other std-module builtin contracts.
    ResultCtorDecl(Span, Symbol, AstTy, AstTy, DeclAttrs),

    /// Module declaration: `defmod Kernel { ... }`
    Defmod(Span, Symbol, Vec<Ast>, DeclAttrs),

    /// Process module declarations: `defagent Counter { ... }`, etc.
    Defagent(Span, Symbol, Vec<Ast>, ProcessSpec, DeclAttrs),
    Defgenserver(Span, Symbol, Vec<Ast>, ProcessSpec, DeclAttrs),
    Defsupervisor(Span, Symbol, Vec<Ast>, ProcessSpec, DeclAttrs),
    DefdynamicSupervisor(Span, Symbol, Vec<Ast>, ProcessSpec, DeclAttrs),

    /// Parser-only namespace declaration: `namespace Auth { ... }`
    Namespace(Span, Symbol, Vec<Ast>),

    /// Impl definition: `impl User { def normalize(self) -> Self { self } }`
    ImplDef(Span, Symbol, Vec<Ast>, DeclAttrs),

    /// Trait definition: `deftrait Add { def add(self: Self, rhs: Self) -> Self }`
    TraitDef(
        Span,
        Symbol,
        Vec<TypeParam>,
        Option<WhereClause>,
        Vec<TraitMethodSig>,
        DeclAttrs,
    ),

    /// Trait impl definition:
    /// `impl Describable for Int { ... }`
    /// `impl From<String> for Int { ... }`
    TraitImplDef(
        Span,
        Symbol,
        Vec<AstTy>,
        AstTy,
        Option<WhereClause>,
        Vec<Ast>,
        DeclAttrs,
    ),

    /// Import declaration
    Import(Span, AstPath, ImportSpec),

    /// Script include directive: `include "./path/to/module.srt"`
    Include(Span, String),

    /// Closure literal: `{|x, y| expr}` / `{|| expr}` / `{ expr }`
    Closure(Span, Vec<ClosureParam>, Box<Ast>),

    /// Captured function / placeholder capture head: `&print` / `&print(&1)`
    Capture(Span, Box<Ast>, Vec<Ast>),

    /// Placeholder inside a capture expression: `&1`, `&2`, ...
    CapturePlaceholder(Span, usize),

    /// Semicolon — explicit Unit coercion marker (wraps the discarded expr)
    Semi(Span, Box<Ast>),

    /// String-keyed HashMap literal: `hash!["key" => value]`
    HashMapLiteral(Span, Vec<HashMapLiteralEntry>),
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
    fn include_form_is_distinct_in_ast() {
        let span = Span { start: 0, end: 20 };
        let include = Ast::Include(span.clone(), "./mylib.srt".to_string());
        assert!(matches!(include, Ast::Include(_, ref path) if path == "./mylib.srt"));
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
