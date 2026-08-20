use crate::ast::BinOp;
use crate::token::Token;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FuncLiteralOperator {
    pub(crate) body: &'static str,
    pub(crate) kind: FuncLiteralOperatorKind,
    pub(crate) tier: FuncLiteralOperatorTier,
}

/// Compiler-owned descriptors for quoted FuncLiteral operators.
/// Pair construction never becomes a `BinOp`: it lowers directly to the
/// existing tuple-literal AST and therefore avoids trait and runtime dispatch.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FuncLiteralOperatorKind {
    BinOp(BinOp),
    PairConstructor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FuncLiteralOperatorTier {
    Expr,
    Pair,
    Logical,
}

const FUNC_LITERAL_OPERATORS: &[FuncLiteralOperator] = &[
    FuncLiteralOperator {
        body: "+",
        kind: FuncLiteralOperatorKind::BinOp(BinOp::Add),
        tier: FuncLiteralOperatorTier::Expr,
    },
    FuncLiteralOperator {
        body: "-",
        kind: FuncLiteralOperatorKind::BinOp(BinOp::Sub),
        tier: FuncLiteralOperatorTier::Expr,
    },
    FuncLiteralOperator {
        body: "*",
        kind: FuncLiteralOperatorKind::BinOp(BinOp::Mul),
        tier: FuncLiteralOperatorTier::Expr,
    },
    FuncLiteralOperator {
        body: "/",
        kind: FuncLiteralOperatorKind::BinOp(BinOp::Slash),
        tier: FuncLiteralOperatorTier::Expr,
    },
    FuncLiteralOperator {
        body: "++",
        kind: FuncLiteralOperatorKind::BinOp(BinOp::Concat),
        tier: FuncLiteralOperatorTier::Expr,
    },
    FuncLiteralOperator {
        body: "(,)",
        kind: FuncLiteralOperatorKind::PairConstructor,
        tier: FuncLiteralOperatorTier::Pair,
    },
    FuncLiteralOperator {
        body: "==",
        kind: FuncLiteralOperatorKind::BinOp(BinOp::Eq),
        tier: FuncLiteralOperatorTier::Logical,
    },
    FuncLiteralOperator {
        body: "!=",
        kind: FuncLiteralOperatorKind::BinOp(BinOp::Neq),
        tier: FuncLiteralOperatorTier::Logical,
    },
    FuncLiteralOperator {
        body: "<",
        kind: FuncLiteralOperatorKind::BinOp(BinOp::Lt),
        tier: FuncLiteralOperatorTier::Logical,
    },
    FuncLiteralOperator {
        body: ">",
        kind: FuncLiteralOperatorKind::BinOp(BinOp::Gt),
        tier: FuncLiteralOperatorTier::Logical,
    },
    FuncLiteralOperator {
        body: "<=",
        kind: FuncLiteralOperatorKind::BinOp(BinOp::Lte),
        tier: FuncLiteralOperatorTier::Logical,
    },
    FuncLiteralOperator {
        body: ">=",
        kind: FuncLiteralOperatorKind::BinOp(BinOp::Gte),
        tier: FuncLiteralOperatorTier::Logical,
    },
];

pub(crate) fn func_literal_operator(body: &str) -> Option<FuncLiteralOperator> {
    FUNC_LITERAL_OPERATORS
        .iter()
        .find(|operator| operator.body == body)
        .cloned()
}

/// Returns the quoted spelling for a token that is supported as a FuncLiteral
/// operator. This keeps bare capture guidance aligned with `` &`operator` ``.
pub(crate) fn func_literal_operator_token(token: &Token) -> Option<&'static str> {
    let body = match token {
        Token::Plus => "+",
        Token::Minus => "-",
        Token::Star => "*",
        Token::Slash => "/",
        Token::Concat => "++",
        Token::EqEq => "==",
        Token::BangEq => "!=",
        Token::Lt => "<",
        Token::Gt => ">",
        Token::LtEq => "<=",
        Token::GtEq => ">=",
        _ => return None,
    };
    debug_assert!(func_literal_operator(body).is_some());
    Some(body)
}

pub(crate) fn is_func_literal_ident(body: &str) -> bool {
    let mut chars = body.chars();
    matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

pub(crate) fn parse_func_literal_path(body: &str) -> Option<Vec<String>> {
    if !body.contains("::") {
        return None;
    }
    let segments = body.split("::").collect::<Vec<_>>();
    if segments.len() < 2
        || !segments
            .iter()
            .all(|segment| is_func_literal_ident(segment))
    {
        return None;
    }
    Some(segments.into_iter().map(str::to_string).collect())
}

pub(crate) fn is_valid_func_literal_body(body: &str) -> bool {
    is_func_literal_ident(body)
        || parse_func_literal_path(body).is_some()
        || func_literal_operator(body).is_some()
}
