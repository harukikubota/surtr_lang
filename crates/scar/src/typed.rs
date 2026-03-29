use sigil::resolved::ResolvedId;
use spire::ast::{BinOp, Lit, Span};

use crate::types::Ty;

/// A fully typed AST node.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedNode {
    pub ty: Ty,
    pub span: Span,
    pub node: TypedInner,
}

/// Inner structure of a typed node.
#[derive(Debug, Clone, PartialEq)]
pub enum TypedInner {
    Lit(Lit),
    Var(ResolvedId),
    App(Box<TypedNode>, Vec<TypedNode>),
    Block(Vec<TypedNode>),
    Bind(TypedPattern, Box<TypedNode>),
    BinOp(BinOp, Box<TypedNode>, Box<TypedNode>),
    List(Vec<TypedNode>),
    If(Box<TypedNode>, Box<TypedNode>, Option<Box<TypedNode>>),
    Match(Box<TypedNode>, Vec<(TypedMatchPattern, TypedNode)>),

    /// Field access — field name resolved to index by Scar
    FieldAccess(Box<TypedNode>, u32),

    /// Struct literal — tag + field values (in definition order)
    StructLit(u32, Vec<TypedNode>),

    /// Constructor call — tag + field values (in definition order)
    ConstructorCall(u32, Vec<TypedNode>),

    /// Error type definition — tag + binding id + show expression
    DeferrorDef(u32, ResolvedId, Box<TypedNode>),

    /// Struct definition — tag + name + field names (for TypeRegistry)
    StructDef(u32, String, Vec<String>),

    /// Record definition — tag + name + field names (for TypeRegistry)
    RecordDef(u32, String, Vec<String>),

    /// Semicolon — explicit Unit coercion
    Semi(Box<TypedNode>),
}

/// Pattern in a binding (typed).
#[derive(Debug, Clone, PartialEq)]
pub enum TypedPattern {
    Var(Ty, ResolvedId),
    Wildcard(Ty),
}

/// Match pattern (typed).
#[derive(Debug, Clone, PartialEq)]
pub enum TypedMatchPattern {
    /// `_`
    Wildcard,
    /// `True` / `False`
    BoolLit(bool),
    /// Integer literal
    IntLit(i64),
    /// String literal
    StrLit(String),
    /// Constructor tag + optional inner binding
    Constructor(u32, Option<ResolvedId>),
}
