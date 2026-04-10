use sigil::resolved::ResolvedId;
use sindr::primitives::SurtrInt;
use spire::ast::{BinOp, Lit, Span};

use crate::types::Ty;

/// A fully typed AST node.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedNode {
    pub ty: Ty,
    pub span: Span,
    pub node: TypedInner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListHelperRef {
    Builtin(u16),
    User(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposeFlavor {
    Plain,
    ResultMap,
    ResultBind,
    ListMap { helper: ListHelperRef },
    ListBind { helper: ListHelperRef },
}

/// Inner structure of a typed node.
#[derive(Debug, Clone, PartialEq)]
pub enum TypedInner {
    Lit(Lit),
    Var(ResolvedId),
    App(Box<TypedNode>, Vec<TypedNode>),
    Block(Vec<TypedNode>),
    Bind(TypedPattern, Box<TypedNode>),
    SafeBind(TypedPattern, Box<TypedNode>),
    BinOp(BinOp, Box<TypedNode>, Box<TypedNode>),
    Pipe(Box<TypedNode>, Box<TypedNode>),
    ResultMap(Box<TypedNode>, Box<TypedNode>),
    ResultBind(Box<TypedNode>, Box<TypedNode>),
    Compose(ComposeFlavor, Box<TypedNode>, Box<TypedNode>),
    ListNil,
    ListCons(Box<TypedNode>, Box<TypedNode>),
    ListLiteral(Vec<TypedNode>),
    InterpolatedStr(Vec<TypedInterpolatedPart>),
    If(Box<TypedNode>, Box<TypedNode>, Option<Box<TypedNode>>),
    Match(Box<TypedNode>, Vec<(TypedMatchPattern, TypedNode)>),

    /// Field access — field name resolved to index by Scar
    FieldAccess(Box<TypedNode>, u32),

    /// Struct literal — tag + field values (in definition order)
    StructLit(u32, Vec<TypedNode>),

    /// Constructor call — tag + field values (in definition order)
    ConstructorCall(u32, Vec<TypedNode>),

    /// Error type definition — tag + binding id + params + show expression
    DeferrorDef(u32, u32, ResolvedId, Vec<TypedFunParam>, Box<TypedNode>),

    /// Enum definition — enum type name + variants
    EnumDef(String, Vec<TypedEnumVariantDef>),

    /// Function definition — tag + name + params + return type + body
    Def(u32, ResolvedId, Vec<TypedFunParam>, Ty, Box<TypedNode>),

    /// Closure literal — params + captures + body
    Closure(Vec<TypedClosureParam>, Vec<ResolvedId>, Box<TypedNode>),

    /// Captured function / partial application
    Capture(Box<TypedNode>, Vec<TypedNode>),

    /// Struct definition — tag + name + field names (for TypeRegistry)
    StructDef(u32, String, Vec<String>),

    /// Record definition — tag + name + field names (for TypeRegistry)
    RecordDef(u32, String, Vec<String>),

    /// Semicolon — explicit Unit coercion
    Semi(Box<TypedNode>),
}

/// Interpolated string fragment (typed).
#[derive(Debug, Clone, PartialEq)]
pub enum TypedInterpolatedPart {
    Text(String),
    Expr(Box<TypedNode>),
}

/// Pattern in a binding (typed).
#[derive(Debug, Clone, PartialEq)]
pub enum TypedPattern {
    Var(Ty, ResolvedId),
    As(Ty, Box<TypedPattern>, ResolvedId),
    Wildcard(Ty),
    ListNil(Ty),
    ListCons(Ty, Box<TypedPattern>, Box<TypedPattern>),
    IntLit(Ty, SurtrInt),
    StrLit(Ty, String),
    BoolLit(Ty, bool),
    /// `Ok(inner)` pattern node in safe-bind recursion.
    ResultOk(Ty, Box<TypedPattern>),
}

/// Match pattern (typed).
#[derive(Debug, Clone, PartialEq)]
pub enum TypedMatchPattern {
    Binding(ResolvedId),
    /// `inner @ alias`
    As(Box<TypedMatchPattern>, ResolvedId),
    /// `_`
    Wildcard,
    /// `True` / `False`
    BoolLit(bool),
    /// Integer literal
    IntLit(SurtrInt),
    /// String literal
    StrLit(String),
    /// Constructor tag + field patterns + payload field offset.
    Constructor {
        tag: u32,
        fields: Vec<TypedMatchPattern>,
        field_offset: u32,
    },
    /// `[]`
    ListNil,
    /// `[head, ..tail]`
    ListCons(Box<TypedMatchPattern>, Box<TypedMatchPattern>),
}

/// Function parameter (typed).
#[derive(Debug, Clone, PartialEq)]
pub struct TypedFunParam {
    pub id: ResolvedId,
    pub ty: Ty,
}

/// Typed closure parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedClosureParam {
    pub id: ResolvedId,
    pub ty: Ty,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedEnumVariantDef {
    pub tag: u32,
    pub constructor_name: String,
    pub field_names: Vec<String>,
}
