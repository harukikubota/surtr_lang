use crate::ast::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub token: T,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // ── Literals ──
    Int(i64),
    Float(f64),
    Str(String),
    True,
    False,
    Unit, // ()

    // ── Identifier ──
    Ident(String),

    // ── Arithmetic operators ──
    Plus,   // +
    Minus,  // -
    Star,   // *
    Concat, // ++

    // ── Comparison / equality ──
    EqEq,   // ==
    BangEq, // !=
    Lt,     // <
    Gt,     // >
    LtEq,   // <=
    GtEq,   // >=

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
    Comma,     // ,
    Colon,     // :
    At,        // @
    Dot,       // .
    DotDot,    // ..
    FatArrow,  // =>
    Arrow,     // ->
    Semicolon, // ;
    Pipe,      // |
    Amp,       // &
    Dollar,    // $

    // ── Statement separators ──
    Newline,

    // ── Keywords ──
    Def,
    /// Generic annotator token: `@@builtin`, `@@foo`, ...
    Annotator(String),
    Defstruct,
    Defrecord,
    Deferror,
    Match,

    // ── End of file ──
    Eof,
}
