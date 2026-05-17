use crate::ast::BinOp;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FuncLiteralOperator {
    pub(crate) body: &'static str,
    pub(crate) binop: BinOp,
    pub(crate) tier: FuncLiteralOperatorTier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FuncLiteralOperatorTier {
    Expr,
    Logical,
}

const FUNC_LITERAL_OPERATORS: &[FuncLiteralOperator] = &[
    FuncLiteralOperator {
        body: "+",
        binop: BinOp::Add,
        tier: FuncLiteralOperatorTier::Expr,
    },
    FuncLiteralOperator {
        body: "-",
        binop: BinOp::Sub,
        tier: FuncLiteralOperatorTier::Expr,
    },
    FuncLiteralOperator {
        body: "*",
        binop: BinOp::Mul,
        tier: FuncLiteralOperatorTier::Expr,
    },
    FuncLiteralOperator {
        body: "/",
        binop: BinOp::Slash,
        tier: FuncLiteralOperatorTier::Expr,
    },
    FuncLiteralOperator {
        body: "++",
        binop: BinOp::Concat,
        tier: FuncLiteralOperatorTier::Expr,
    },
    FuncLiteralOperator {
        body: "==",
        binop: BinOp::Eq,
        tier: FuncLiteralOperatorTier::Logical,
    },
    FuncLiteralOperator {
        body: "!=",
        binop: BinOp::Neq,
        tier: FuncLiteralOperatorTier::Logical,
    },
    FuncLiteralOperator {
        body: "<",
        binop: BinOp::Lt,
        tier: FuncLiteralOperatorTier::Logical,
    },
    FuncLiteralOperator {
        body: ">",
        binop: BinOp::Gt,
        tier: FuncLiteralOperatorTier::Logical,
    },
    FuncLiteralOperator {
        body: "<=",
        binop: BinOp::Lte,
        tier: FuncLiteralOperatorTier::Logical,
    },
    FuncLiteralOperator {
        body: ">=",
        binop: BinOp::Gte,
        tier: FuncLiteralOperatorTier::Logical,
    },
];

pub(crate) fn func_literal_operator(body: &str) -> Option<FuncLiteralOperator> {
    FUNC_LITERAL_OPERATORS
        .iter()
        .find(|operator| operator.body == body)
        .cloned()
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
