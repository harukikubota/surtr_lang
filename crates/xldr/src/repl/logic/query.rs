use spire::ast::{Ast, AstPattern, AstTy, Span};

use crate::{derive_parse_rules, SourceKind};

const QUERY_OPERATORS: &[&str] = &["|>=", "|*>", "|>", ">=>", ">*", ">>"];

#[derive(Debug, Clone, PartialEq)]
pub enum ReplQuery {
    Symbol(String),
    Expr(String),
    TypedCall(TypedCallQuery),
    TypedOperator(TypedOperatorQuery),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedCallQuery {
    pub callee: String,
    pub args: Vec<QueryArg>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedOperatorQuery {
    pub lhs: QueryArg,
    pub rhs: QueryArg,
    pub operator: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryArg {
    pub source: String,
    pub kind: QueryArgKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryArgKind {
    Binding(String),
    Literal(QueryLiteral),
    AnnotatedHole(AstTy),
    TypeExpr(AstTy),
    Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryLiteral {
    Unit,
    Boolean,
    String,
    Int,
    Float,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplQueryParseError {
    message: String,
}

impl ReplQueryParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub fn parse_repl_query(input: &str) -> Result<ReplQuery, ReplQueryParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ReplQueryParseError::new("REPL query cannot be empty."));
    }

    if QUERY_OPERATORS.contains(&trimmed) {
        return Ok(ReplQuery::Symbol(trimmed.to_string()));
    }

    if let Some(operator_query) = parse_typed_operator_query(trimmed)? {
        return Ok(ReplQuery::TypedOperator(operator_query));
    }

    if let Some(call_query) = parse_typed_call_query(trimmed)? {
        return Ok(ReplQuery::TypedCall(call_query));
    }

    if trimmed.split_whitespace().count() == 1 {
        return Ok(ReplQuery::Symbol(trimmed.to_string()));
    }

    Ok(ReplQuery::Expr(trimmed.to_string()))
}

fn parse_typed_call_query(input: &str) -> Result<Option<TypedCallQuery>, ReplQueryParseError> {
    let Some(open) = input.find('(') else {
        return Ok(None);
    };
    if !input.ends_with(')') {
        return Err(ReplQueryParseError::new(
            "Invalid typed call query: missing closing `)`.",
        ));
    }
    let callee = input[..open].trim();
    if callee.is_empty() || callee.chars().any(char::is_whitespace) {
        return Err(ReplQueryParseError::new(
            "Invalid typed call query: missing callee.",
        ));
    }

    let args_src = &input[open + 1..input.len() - 1];
    let args = split_top_level_commas(args_src)?;
    let mut parsed_args = Vec::with_capacity(args.len());
    for arg in args {
        let arg = arg.trim();
        if arg.is_empty() {
            return Err(ReplQueryParseError::new(
                "Invalid typed call query: empty argument.",
            ));
        }
        parsed_args.push(parse_query_arg(arg)?);
    }
    Ok(Some(TypedCallQuery {
        callee: callee.to_string(),
        args: parsed_args,
    }))
}

fn parse_typed_operator_query(
    input: &str,
) -> Result<Option<TypedOperatorQuery>, ReplQueryParseError> {
    let Some((lhs, operator, rhs)) = split_top_level_operator_query(input) else {
        return Ok(None);
    };
    let lhs = lhs.trim();
    let rhs = rhs.trim();
    if lhs.is_empty() || rhs.is_empty() {
        return Err(ReplQueryParseError::new(format!(
            "Invalid operator query: `{operator}` requires both left and right operands."
        )));
    }
    Ok(Some(TypedOperatorQuery {
        lhs: parse_query_arg(lhs)?,
        rhs: parse_query_arg(rhs)?,
        operator,
    }))
}

fn parse_query_arg(input: &str) -> Result<QueryArg, ReplQueryParseError> {
    let kind = if let Some(ty) = parse_annotated_hole(input)? {
        QueryArgKind::AnnotatedHole(ty)
    } else if input == "()" {
        QueryArgKind::Literal(QueryLiteral::Unit)
    } else if matches!(input, "True" | "False") {
        QueryArgKind::Literal(QueryLiteral::Boolean)
    } else if is_string_literal(input) {
        QueryArgKind::Literal(QueryLiteral::String)
    } else if is_float_literal(input) {
        QueryArgKind::Literal(QueryLiteral::Float)
    } else if is_int_literal(input) {
        QueryArgKind::Literal(QueryLiteral::Int)
    } else if looks_like_type_expr(input) {
        if let Some(ty) = parse_user_query_type_loose(input).map_err(ReplQueryParseError::new)? {
            QueryArgKind::TypeExpr(ty)
        } else if is_simple_name(input) {
            QueryArgKind::Binding(input.to_string())
        } else {
            QueryArgKind::Expr
        }
    } else if is_simple_name(input) {
        QueryArgKind::Binding(input.to_string())
    } else if let Some(ty) = parse_user_query_type_loose(input).map_err(ReplQueryParseError::new)? {
        QueryArgKind::TypeExpr(ty)
    } else {
        QueryArgKind::Expr
    };

    Ok(QueryArg {
        source: input.to_string(),
        kind,
    })
}

fn parse_annotated_hole(input: &str) -> Result<Option<AstTy>, ReplQueryParseError> {
    let Some((name, ty)) = input.split_once(':') else {
        return Ok(None);
    };
    if name.trim() != "_" {
        return Ok(None);
    }
    let ty = ty.trim();
    if ty.is_empty() {
        return Err(ReplQueryParseError::new(
            "Invalid typed query: `_ :` requires a type.",
        ));
    }
    let ty = parse_user_query_type(ty).map_err(ReplQueryParseError::new)?;
    Ok(Some(ty))
}

fn split_top_level_commas(input: &str) -> Result<Vec<&str>, ReplQueryParseError> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut paren_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in input.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            ',' if paren_depth == 0 && angle_depth == 0 => {
                parts.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    if paren_depth != 0 || angle_depth != 0 || in_string {
        return Err(ReplQueryParseError::new(
            "Invalid typed call query: unterminated argument list.",
        ));
    }

    let tail = input[start..].trim();
    if !tail.is_empty() || !input.trim().is_empty() {
        parts.push(tail);
    }
    Ok(parts)
}

fn split_top_level_operator_query(input: &str) -> Option<(&str, &'static str, &str)> {
    let mut paren_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in input.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            _ if paren_depth == 0 && angle_depth == 0 => {
                for operator in QUERY_OPERATORS {
                    if input[idx..].starts_with(operator) {
                        let lhs = &input[..idx];
                        let rhs = &input[idx + operator.len()..];
                        return Some((lhs, *operator, rhs));
                    }
                }
            }
            _ => {}
        }
    }

    None
}

fn parse_query_type(input: &str) -> Option<AstTy> {
    let source = format!("__query__: {input} = ()");
    let ast = spire::parse_with_context(
        &source,
        spire::ParserContext::repl(0).with_rules(derive_parse_rules(SourceKind::ReplChunk)),
    )
    .ok()?;
    match ast.as_slice() {
        [Ast::Bind(_, AstPattern::Annotated(_, _, ty), _)] => Some(ty.clone()),
        _ => None,
    }
}

pub fn parse_signature_type(input: &str) -> Option<AstTy> {
    parse_query_type(input)
}

pub fn parse_user_query_type(input: &str) -> Result<AstTy, String> {
    let ty = parse_query_type(input).ok_or_else(|| {
        format!("Unsupported query type `{input}`. Use a valid Surtr type expression.")
    })?;
    validate_user_query_type(&ty)?;
    Ok(ty)
}

pub fn parse_user_query_type_loose(input: &str) -> Result<Option<AstTy>, String> {
    let Some(ty) = parse_query_type(input) else {
        return Ok(None);
    };
    validate_user_query_type(&ty)?;
    Ok(Some(ty))
}

pub fn parse_binding_query_type(input: &str) -> Option<AstTy> {
    parse_query_type(input).map(|ty| normalize_binding_query_type(&ty))
}

fn validate_user_query_type(ty: &AstTy) -> Result<(), String> {
    match ty {
        AstTy::Generic(_, name, args) if name == "Result" && args.len() == 2 => Err(
            "Typed query `Result` should be written as `Result<T>`; do not specify the `Error` parameter."
                .to_string(),
        ),
        AstTy::Generic(_, _, args) | AstTy::Tuple(_, args) => {
            for arg in args {
                validate_user_query_type(arg)?;
            }
            Ok(())
        }
        AstTy::Func(_, params, ret) => {
            for param in params {
                validate_user_query_type(param)?;
            }
            validate_user_query_type(ret)
        }
        _ => Ok(()),
    }
}

fn normalize_binding_query_type(ty: &AstTy) -> AstTy {
    match ty {
        AstTy::Named(_, _) | AstTy::ImplTrait(_, _) => ty.clone(),
        AstTy::Generic(span, name, args) if name == "Result" && args.len() == 2 => AstTy::Generic(
            span.clone(),
            name.clone(),
            vec![normalize_binding_query_type(&args[0])],
        ),
        AstTy::Generic(span, name, args) => AstTy::Generic(
            span.clone(),
            name.clone(),
            args.iter().map(normalize_binding_query_type).collect(),
        ),
        AstTy::Tuple(span, items) => AstTy::Tuple(
            span.clone(),
            items.iter().map(normalize_binding_query_type).collect(),
        ),
        AstTy::Func(span, params, ret) => AstTy::Func(
            span.clone(),
            params.iter().map(normalize_binding_query_type).collect(),
            Box::new(normalize_binding_query_type(ret)),
        ),
    }
}

pub fn format_query_ty(ty: &AstTy) -> String {
    match ty {
        AstTy::Named(_, name) => name.clone(),
        AstTy::ImplTrait(_, name) => format!("impl {name}"),
        AstTy::Generic(_, name, args) => {
            let args = args
                .iter()
                .map(format_query_ty)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}<{args}>")
        }
        AstTy::Tuple(_, items) => format!(
            "({})",
            items
                .iter()
                .map(format_query_ty)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        AstTy::Func(_, params, ret) => {
            if params.is_empty() {
                format!("(-> {})", format_query_ty(ret))
            } else {
                format!(
                    "({} -> {})",
                    params
                        .iter()
                        .map(format_query_ty)
                        .collect::<Vec<_>>()
                        .join(", "),
                    format_query_ty(ret)
                )
            }
        }
    }
}

pub fn ast_ty_from_query_arg(arg: &QueryArg) -> Option<AstTy> {
    match &arg.kind {
        QueryArgKind::Literal(QueryLiteral::Unit) => {
            Some(AstTy::Named(Span { start: 0, end: 0 }, "Unit".to_string()))
        }
        QueryArgKind::Literal(QueryLiteral::Boolean) => Some(AstTy::Named(
            Span { start: 0, end: 0 },
            "Boolean".to_string(),
        )),
        QueryArgKind::Literal(QueryLiteral::String) => Some(AstTy::Named(
            Span { start: 0, end: 0 },
            "String".to_string(),
        )),
        QueryArgKind::Literal(QueryLiteral::Int) => {
            Some(AstTy::Named(Span { start: 0, end: 0 }, "Int".to_string()))
        }
        QueryArgKind::Literal(QueryLiteral::Float) => {
            Some(AstTy::Named(Span { start: 0, end: 0 }, "Float".to_string()))
        }
        QueryArgKind::AnnotatedHole(ty) | QueryArgKind::TypeExpr(ty) => Some(ty.clone()),
        QueryArgKind::Binding(_) | QueryArgKind::Expr => None,
    }
}

fn is_simple_name(input: &str) -> bool {
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn looks_like_type_expr(input: &str) -> bool {
    input.starts_with('(')
        || input.starts_with("impl ")
        || input.starts_with('$')
        || input.contains('<')
        || input.contains("->")
        || input
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn is_string_literal(input: &str) -> bool {
    input.len() >= 2 && input.starts_with('"') && input.ends_with('"')
}

fn is_int_literal(input: &str) -> bool {
    let digits = input.strip_prefix('-').unwrap_or(input);
    !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit())
}

fn is_float_literal(input: &str) -> bool {
    let digits = input.strip_prefix('-').unwrap_or(input);
    let Some((lhs, rhs)) = digits.split_once('.') else {
        return false;
    };
    !lhs.is_empty()
        && !rhs.is_empty()
        && lhs.chars().all(|ch| ch.is_ascii_digit())
        && rhs.chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_typed_call_query() {
        let query = parse_repl_query("gt(Int, Int)").expect("query should parse");
        assert!(matches!(
            query,
            ReplQuery::TypedCall(TypedCallQuery { callee, args })
            if callee == "gt"
                && matches!(args[0].kind, QueryArgKind::TypeExpr(_))
                && matches!(args[1].kind, QueryArgKind::TypeExpr(_))
        ));
    }

    #[test]
    fn parse_typed_operator_query_with_bindings() {
        let query = parse_repl_query("ret |>= up").expect("query should parse");
        assert!(matches!(
            query,
            ReplQuery::TypedOperator(TypedOperatorQuery {
                operator: "|>=",
                lhs: QueryArg { kind: QueryArgKind::Binding(ref name), .. },
                rhs: QueryArg { kind: QueryArgKind::Binding(ref other), .. },
            }) if name == "ret" && other == "up"
        ));
    }

    #[test]
    fn parse_typed_operator_query_with_function_type_rhs() {
        let query = parse_repl_query("num |> (Int -> String)").expect("query should parse");
        assert!(matches!(
            query,
            ReplQuery::TypedOperator(TypedOperatorQuery {
                operator: "|>",
                rhs: QueryArg {
                    kind: QueryArgKind::TypeExpr(_),
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn parse_annotated_hole_arg() {
        let query = parse_repl_query("gt(_ : Float, _ : Float)").expect("query should parse");
        assert!(matches!(
            query,
            ReplQuery::TypedCall(TypedCallQuery { args, .. })
            if matches!(args[0].kind, QueryArgKind::AnnotatedHole(_))
                && matches!(args[1].kind, QueryArgKind::AnnotatedHole(_))
        ));
    }

    #[test]
    fn reject_empty_argument() {
        let err = parse_repl_query("gt(Int, )").expect_err("query should fail");
        assert_eq!(err.message(), "Invalid typed call query: empty argument.");
    }

    #[test]
    fn reject_missing_closing_paren() {
        let err = parse_repl_query("gt(Int, Int").expect_err("query should fail");
        assert_eq!(
            err.message(),
            "Invalid typed call query: missing closing `)`."
        );
    }

    #[test]
    fn unsupported_arg_becomes_expr_query_arg() {
        let query = parse_repl_query("gt(make_value(), 1)").expect("query should parse");
        assert!(matches!(
            query,
            ReplQuery::TypedCall(TypedCallQuery { args, .. })
            if matches!(args[0].kind, QueryArgKind::Expr)
                && matches!(args[1].kind, QueryArgKind::Literal(QueryLiteral::Int))
        ));
    }
}
