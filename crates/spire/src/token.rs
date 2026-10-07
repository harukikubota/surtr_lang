use crate::ast::Span;
use sindr::primitives::SurtrInt;

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub token: T,
    pub span: Span,
}

/// Dedented raw text and the original character position of each retained character.
#[derive(Debug, Clone, PartialEq)]
pub struct RawStringLiteral {
    pub text: String,
    pub(crate) source_positions: Vec<usize>,
    pub(crate) source_end: usize,
}

impl RawStringLiteral {
    pub(crate) fn source_span(&self, span: Span) -> Span {
        let start = if span.start == self.source_positions.len() {
            self.source_end
        } else {
            self.source_positions[span.start]
        };
        let end = if span.start == span.end {
            start
        } else {
            self.source_positions[span.end - 1] + 1
        };
        Span { start, end }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // ── Literals ──
    Int(SurtrInt),
    Float(f64),
    Str(crate::string_literal::StringLiteral),
    DocString(RawStringLiteral),
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
    BulkUpdate,
    Private,
    Public,
    Readonly,
    Const,
    Type,
    Where,

    // ── End of file ──
    Eof,
}
