use crate::ast::Span;
use sindr::primitives::SurtrInt;

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub token: T,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // ── Literals ──
    Int(SurtrInt),
    Float(f64),
    Str(crate::string_literal::StringLiteral),
    DocString(String),
    True,
    False,
    Unit, // ()

    // ── Identifier ──
    Ident(String),
    ReservedCallName(sindr::names::ReservedCallName),
    PatternConsumer(sindr::pattern::PatternConsumer),
    NumberedPlaceholder(String),
    FuncLiteral(String),

    // ── Arithmetic operators ──
    Plus,    // +
    Minus,   // -
    Star,    // *
    Slash,   // /
    Percent, // %
    Concat,  // ++

    // ── Comparison / equality ──
    EqEq,   // ==
    BangEq, // !=
    Bang,   // !
    Lt,     // <
    Gt,     // >
    LtEq,   // <=
    GtEq,   // >=
    AndAnd, // &&
    OrOr,   // ||

    // ── Assignment ──
    Bind,     // =
    SafeBind, // =?

    // ── Delimiters ──
    LParen, // (
    RParen, // )
    LBrack, // [
    RBrack, // ]
    LBrace, // {
    RBrace, // }

    // ── Punctuation ──
    Comma,            // ,
    Colon,            // :
    At,               // @
    Dot,              // .
    Question,         // ?
    DotDot,           // ..
    FatArrow,         // =>
    Arrow,            // ->
    LeftArrow,        // <-
    Semicolon,        // ;
    Pipe,             // |
    PipeApply,        // |>
    PipeMap,          // |*>
    PipeApplyContext, // |*|
    PipeBind,         // |>=
    Choice,           // <|>
    Compose,          // >>
    LiftCompose,      // >*
    KleisliCompose,   // >=>
    Amp,              // &
    Tilde,            // ~
    Dollar,           // $
    Caret,            // ^

    // ── Statement separators ──
    Newline,

    // ── Keywords ──
    Def,
    Defp,
    Defagent,
    Defgenserver,
    Defsupervisor,
    DefdynamicSupervisor,
    SupervisorInit,
    Defmod,
    Namespace,
    Deftrait,
    Import,
    Include,
    /// Generic annotator token: `@builtin`, `@foo`, ...
    Annotator(String),
    Defstruct,
    Defrecord,
    Deferror,
    Defenum,
    Defextractor,
    Impl,
    For,
    Match,
    Do,
    When,
    Cond,
    Private,
    Public,
    Readonly,
    Const,
    Type,
    Where,

    // ── End of file ──
    Eof,
}
