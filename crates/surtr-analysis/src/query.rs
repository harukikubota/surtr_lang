use spire::ast::{Ast, AstPattern, AstTy, Span};
use std::ops::{Deref, Range};

const QUERY_OPERATORS: &[&str] = &[
    "|>=", "|*>", "|>", ">=>", ">*", ">>", "+", "-", "*", "&&", "||", "==", "!=", "<", "<=",
    ">", ">=", "/", "++",
];

#[derive(Debug, Clone, PartialEq)]
pub enum CommandQuery {
    Symbol(SymbolQuery),
    TypedCall(ParsedTypedCallQuery),
    TypedOperator(ParsedTypedOperatorQuery),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SymbolQuery {
    pub source: String,
    pub span: Span,
}

impl Deref for SymbolQuery {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedCallQuery {
    pub callee: String,
    pub args: Vec<QueryArg>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTypedCallQuery {
    pub query: TypedCallQuery,
    pub callee_span: Span,
    pub span: Span,
}

impl Deref for ParsedTypedCallQuery {
    type Target = TypedCallQuery;

    fn deref(&self) -> &Self::Target {
        &self.query
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypedOperatorQuery {
    pub operator: &'static str,
    pub target: QueryArg,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTypedOperatorQuery {
    pub query: TypedOperatorQuery,
    pub operator_span: Span,
    pub span: Span,
}

impl Deref for ParsedTypedOperatorQuery {
    type Target = TypedOperatorQuery;

    fn deref(&self) -> &Self::Target {
        &self.query
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryArg {
    pub source: String,
    pub span: Span,
    pub kind: QueryArgKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryArgKind {
    Binding(String),
    TypeExpr(AstTy),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandQueryParseError {
    message: String,
    span: Span,
}

impl CommandQueryParseError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    #[allow(dead_code)]
    pub fn span(&self) -> Span {
        self.span.clone()
    }
}

pub fn parse_command_query(input: &str) -> Result<CommandQuery, CommandQueryParseError> {
    let Some((trim_start, trim_end)) = trimmed_byte_bounds(input) else {
        let pos = input.chars().count();
        return Err(CommandQueryParseError::new(
            "REPL query cannot be empty.",
            Span {
                start: pos,
                end: pos,
            },
        ));
    };
    let trimmed = &input[trim_start..trim_end];
    let ctx = ParseContext {
        source: trimmed,
        base_char: input[..trim_start].chars().count(),
    };

    if QUERY_OPERATORS.contains(&trimmed) {
        return Ok(CommandQuery::Symbol(SymbolQuery {
            source: trimmed.to_string(),
            span: ctx.full_span(),
        }));
    }

    if let Some(operator_query) = parse_typed_operator_query(&ctx)? {
        return Ok(CommandQuery::TypedOperator(operator_query));
    }

    if let Some(call_query) = parse_typed_call_query(&ctx)? {
        return Ok(CommandQuery::TypedCall(call_query));
    }

    if trimmed.split_whitespace().count() == 1 {
        return Ok(CommandQuery::Symbol(SymbolQuery {
            source: trimmed.to_string(),
            span: ctx.full_span(),
        }));
    }

    Err(CommandQueryParseError::new(
        "Unsupported command query form. Use a symbol, typed call, or typed operator.",
        ctx.full_span(),
    ))
}

#[derive(Debug, Clone, Copy)]
struct ParseContext<'a> {
    source: &'a str,
    base_char: usize,
}

impl ParseContext<'_> {
    fn full_span(&self) -> Span {
        self.span_for_local_bytes(0, self.source.len())
    }

    fn span_for_local_bytes(&self, start: usize, end: usize) -> Span {
        Span {
            start: self.base_char + self.source[..start].chars().count(),
            end: self.base_char + self.source[..end].chars().count(),
        }
    }

    fn point_span_for_local_byte(&self, byte: usize) -> Span {
        let pos = self.base_char + self.source[..byte].chars().count();
        Span {
            start: pos,
            end: pos,
        }
    }
}

fn parse_typed_call_query(
    ctx: &ParseContext<'_>,
) -> Result<Option<ParsedTypedCallQuery>, CommandQueryParseError> {
    parse_typed_call_query_inner(ctx, 0..ctx.source.len())
}

fn parse_typed_call_query_inner(
    ctx: &ParseContext<'_>,
    range: Range<usize>,
) -> Result<Option<ParsedTypedCallQuery>, CommandQueryParseError> {
    let input = &ctx.source[range.clone()];
    let Some(rel_open) = input.find('(') else {
        return Ok(None);
    };
    let open = range.start + rel_open;
    if !input.ends_with(')') {
        return Err(CommandQueryParseError::new(
            "Invalid typed call query: missing closing `)`.",
            ctx.span_for_local_bytes(open, range.end),
        ));
    }
    let callee_range = trim_byte_range(ctx.source, range.start..open);
    let callee = &ctx.source[callee_range.clone()];
    if callee.is_empty() || callee.chars().any(char::is_whitespace) {
        let span = if callee_range.is_empty() {
            ctx.point_span_for_local_byte(open)
        } else {
            ctx.span_for_local_bytes(callee_range.start, callee_range.end)
        };
        return Err(CommandQueryParseError::new(
            "Invalid typed call query: missing callee.",
            span,
        ));
    }
    if !is_callable_ref(callee) {
        return Err(CommandQueryParseError::new(
            format!("Invalid typed call query callee `{callee}`."),
            ctx.span_for_local_bytes(callee_range.start, callee_range.end),
        ));
    }

    let close = range.end - 1;
    let args = split_top_level_commas(ctx, open + 1, close)?;
    let mut parsed_args = Vec::with_capacity(args.len());
    for arg_range in args {
        let trimmed_range = trim_byte_range(ctx.source, arg_range.clone());
        if trimmed_range.is_empty() {
            return Err(CommandQueryParseError::new(
                "Invalid typed call query: empty argument.",
                empty_argument_span(ctx, &arg_range, close),
            ));
        }
        parsed_args.push(parse_query_arg(ctx, trimmed_range)?);
    }
    Ok(Some(ParsedTypedCallQuery {
        query: TypedCallQuery {
            callee: callee.to_string(),
            args: parsed_args,
        },
        callee_span: ctx.span_for_local_bytes(callee_range.start, callee_range.end),
        span: ctx.span_for_local_bytes(range.start, range.end),
    }))
}

fn parse_typed_operator_query(
    ctx: &ParseContext<'_>,
) -> Result<Option<ParsedTypedOperatorQuery>, CommandQueryParseError> {
    let Some((operator, target_range, operator_range)) = split_operator_target_query(ctx.source)
    else {
        return Ok(None);
    };
    let target = trim_byte_range(ctx.source, target_range.clone());
    if target.is_empty() {
        return Err(CommandQueryParseError::new(
            format!("Invalid operator target query: `{operator}` requires a target."),
            ctx.point_span_for_local_byte(operator_range.end),
        ));
    }
    let target = parse_query_arg(ctx, target)?;
    Ok(Some(ParsedTypedOperatorQuery {
        query: TypedOperatorQuery {
            operator,
            target,
        },
        operator_span: ctx.span_for_local_bytes(operator_range.start, operator_range.end),
        span: ctx.full_span(),
    }))
}

fn parse_query_arg(
    ctx: &ParseContext<'_>,
    range: Range<usize>,
) -> Result<QueryArg, CommandQueryParseError> {
    let input = &ctx.source[range.clone()];
    let kind = if input.starts_with('$') || input.starts_with('&') || input == "_1" {
        return Err(CommandQueryParseError::new(
            format!("Unsupported command query argument `{input}`."),
            ctx.span_for_local_bytes(range.start, range.end),
        ));
    } else if looks_like_type_expr(input) {
        if let Some(ty) = parse_user_query_type_loose_in_span(ctx, input, &range)? {
            QueryArgKind::TypeExpr(ty)
        } else {
            return Err(CommandQueryParseError::new(
                format!("Unsupported command query argument `{input}`."),
                ctx.span_for_local_bytes(range.start, range.end),
            ));
        }
    } else if is_simple_name(input) {
        QueryArgKind::Binding(input.to_string())
    } else {
        return Err(CommandQueryParseError::new(
            format!("Unsupported command query argument `{input}`."),
            ctx.span_for_local_bytes(range.start, range.end),
        ));
    };

    Ok(QueryArg {
        source: input.to_string(),
        span: ctx.span_for_local_bytes(range.start, range.end),
        kind,
    })
}

fn split_top_level_commas(
    ctx: &ParseContext<'_>,
    start: usize,
    end: usize,
) -> Result<Vec<Range<usize>>, CommandQueryParseError> {
    let mut parts = Vec::new();
    let mut part_start = start;
    let mut paren_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut paren_stack = Vec::new();
    let mut angle_stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut string_start = None;
    let input = ctx.source;

    for (rel_idx, ch) in input[start..end].char_indices() {
        let idx = start + rel_idx;
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
                string_start = None;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                string_start = Some(idx);
            }
            '(' => {
                paren_depth += 1;
                paren_stack.push(idx);
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                let _ = paren_stack.pop();
            }
            '<' => {
                angle_depth += 1;
                angle_stack.push(idx);
            }
            '>' => {
                angle_depth = angle_depth.saturating_sub(1);
                let _ = angle_stack.pop();
            }
            ',' if paren_depth == 0 && angle_depth == 0 => {
                parts.push(part_start..idx);
                part_start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    if paren_depth != 0 || angle_depth != 0 || in_string {
        let error_start = string_start
            .or_else(|| paren_stack.last().copied())
            .or_else(|| angle_stack.last().copied())
            .unwrap_or(part_start);
        return Err(CommandQueryParseError::new(
            "Invalid typed call query: unterminated argument list.",
            ctx.span_for_local_bytes(error_start, end),
        ));
    }

    if part_start != end || start != end {
        parts.push(part_start..end);
    }
    Ok(parts)
}

fn split_operator_target_query(input: &str) -> Option<(&'static str, Range<usize>, Range<usize>)> {
    let (operator, _rest) = input.split_once(char::is_whitespace)?;
    let operator = QUERY_OPERATORS.iter().find(|candidate| **candidate == operator)?;
    let operator_end = operator.len();
    let target_start = input[operator_end..]
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, _)| operator_end + idx)?;
    Some((*operator, target_start..input.len(), 0..operator_end))
}

fn parse_user_query_type_loose_in_span(
    ctx: &ParseContext<'_>,
    input: &str,
    range: &Range<usize>,
) -> Result<Option<AstTy>, CommandQueryParseError> {
    parse_user_query_type_loose(input).map_err(|message| {
        CommandQueryParseError::new(message, ctx.span_for_local_bytes(range.start, range.end))
    })
}

fn trimmed_byte_bounds(input: &str) -> Option<(usize, usize)> {
    let start = input
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, _)| idx)?;
    let end = input
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(start);
    Some((start, end))
}

fn trim_byte_range(input: &str, range: Range<usize>) -> Range<usize> {
    let slice = &input[range.clone()];
    let start = slice
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, _)| range.start + idx)
        .unwrap_or(range.end);
    let end = slice
        .char_indices()
        .rev()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(idx, ch)| range.start + idx + ch.len_utf8())
        .unwrap_or(start);
    start..end
}

fn empty_argument_span(
    ctx: &ParseContext<'_>,
    arg_range: &Range<usize>,
    close_paren: usize,
) -> Span {
    let point = if arg_range.start == arg_range.end
        || trim_byte_range(ctx.source, arg_range.clone()).is_empty()
    {
        close_paren
    } else {
        arg_range.start
    };
    ctx.point_span_for_local_byte(point)
}

fn parse_query_type(input: &str) -> Option<AstTy> {
    let source = format!("__query__: {input} = ()");
    let ast = spire::parse_with_context(
        &source,
        spire::ParserContext::repl(0).with_rules(spire::ParseRules::repl_chunk()),
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
        AstTy::Named(_, name) if name.starts_with('$') => Err(
            "Command queries do not accept generic type variables; use a concrete type."
                .to_string(),
        ),
        AstTy::ImplTrait(_, _) => Err(
            "Command queries require a concrete type; `impl Trait` is not supported."
                .to_string(),
        ),
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
        QueryArgKind::TypeExpr(ty) => Some(ty.clone()),
        QueryArgKind::Binding(_) => None,
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
        || input.contains('<')
        || input.contains("->")
        || input
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn is_callable_ref(input: &str) -> bool {
    !input.is_empty()
        && !input.chars().any(char::is_whitespace)
        && input
            .split("::")
            .all(|segment| !segment.is_empty() && is_callable_segment(segment))
}

fn is_callable_segment(segment: &str) -> bool {
    if let Some(head) = segment.strip_suffix('!') {
        return is_simple_name(head);
    }
    is_simple_name(segment)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_typed_call_query() {
        let query = parse_command_query("compare(Int, Int)").expect("query should parse");
        assert!(matches!(
            query,
            CommandQuery::TypedCall(ParsedTypedCallQuery {
                query: TypedCallQuery { callee, args },
                ..
            })
            if callee == "compare"
                && matches!(args[0].kind, QueryArgKind::TypeExpr(_))
                && matches!(args[1].kind, QueryArgKind::TypeExpr(_))
        ));
    }

    #[test]
    fn parse_typed_call_query_with_binding_identity_args() {
        let query = parse_command_query("compare(left, right)").expect("query should parse");
        assert!(matches!(
            query,
            CommandQuery::TypedCall(ParsedTypedCallQuery {
                query: TypedCallQuery { callee, args },
                ..
            })
            if callee == "compare"
                && matches!(args[0].kind, QueryArgKind::Binding(ref name) if name == "left")
                && matches!(args[1].kind, QueryArgKind::Binding(ref name) if name == "right")
        ));
    }

    #[test]
    fn parse_typed_call_query_with_deconstruct_callee() {
        let query = parse_command_query("User!()").expect("query should parse");
        assert!(matches!(
            query,
            CommandQuery::TypedCall(ParsedTypedCallQuery {
                query: TypedCallQuery { callee, args },
                ..
            })
            if callee == "User!" && args.is_empty()
        ));
    }

    #[test]
    fn parse_operator_target_query_with_type_target() {
        let query = parse_command_query("|*> Option").expect("query should parse");
        assert!(matches!(
            query,
            CommandQuery::TypedOperator(ParsedTypedOperatorQuery {
                query: TypedOperatorQuery {
                    operator: "|*>",
                    target: QueryArg { kind: QueryArgKind::TypeExpr(_), .. },
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn reject_empty_argument() {
        let err = parse_command_query("compare(Int, )").expect_err("query should fail");
        assert_eq!(err.message(), "Invalid typed call query: empty argument.");
    }

    #[test]
    fn reject_missing_closing_paren() {
        let err = parse_command_query("compare(Int, Int").expect_err("query should fail");
        assert_eq!(
            err.message(),
            "Invalid typed call query: missing closing `)`."
        );
    }

    #[test]
    fn reject_literal_typed_query_args() {
        let err = parse_command_query("compare(1, 2)").expect_err("query should fail");
        assert!(
            err.message()
                .contains("Unsupported command query argument `1`"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn reject_expression_typed_query_args() {
        let err = parse_command_query("compare(left + 1, right)").expect_err("query should fail");
        assert!(
            err.message()
                .contains("Unsupported command query argument `left + 1`"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn reject_generic_type_variables() {
        let err = parse_command_query("compare(List<$T>, value)").expect_err("query should fail");
        assert!(
            err.message().contains("generic type variables"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn reject_impl_trait_query_types() {
        let err = parse_command_query("show(impl Show)").expect_err("query should fail");
        assert!(err.message().contains("concrete type"), "{}", err.message());
    }

    #[test]
    fn reject_forced_binding_query_args() {
        let err = parse_command_query("compare($left, Int)").expect_err("query should fail");
        assert!(
            err.message().contains("`$left`"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn reject_legacy_operator_query_shapes() {
        let err = parse_command_query("ret |>= up").expect_err("query should fail");
        assert!(
            err.message().contains("Unsupported command query form"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn reject_capture_query_shapes() {
        let err = parse_command_query("map(&add(Int, &1))").expect_err("query should fail");
        assert!(
            err.message().contains("`&add(Int, &1)`"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn parse_symbol_query_tracks_char_span() {
        let query = parse_command_query("  value  ").expect("query should parse");
        assert!(matches!(
            query,
            CommandQuery::Symbol(SymbolQuery { ref source, span })
            if source == "value" && span == Span { start: 2, end: 7 }
        ));
    }

    #[test]
    fn parse_typed_call_query_tracks_argument_spans_in_char_offsets() {
        let query = parse_command_query("compare(x, Int)").expect("query should parse");
        assert!(matches!(
            query,
            CommandQuery::TypedCall(ParsedTypedCallQuery {
                query: TypedCallQuery {
                    ref callee,
                    ref args,
                },
                callee_span,
                span,
            })
            if callee == "compare"
                && callee_span == Span { start: 0, end: 7 }
                && span == Span { start: 0, end: 15 }
                && args.len() == 2
                && args[0].span == Span { start: 8, end: 9 }
                && args[1].span == Span { start: 11, end: 14 }
        ));
    }

    #[test]
    fn parse_typed_operator_query_tracks_operator_span_in_char_offsets() {
        let query = parse_command_query("|*> Option").expect("query should parse");
        assert!(matches!(
            query,
            CommandQuery::TypedOperator(ParsedTypedOperatorQuery {
                query: TypedOperatorQuery {
                    operator: "|*>",
                    ref target,
                    ..
                },
                operator_span,
                span,
            })
            if operator_span == Span { start: 0, end: 3 }
                && span == Span { start: 0, end: 10 }
                && target.span == Span { start: 4, end: 10 }
        ));
    }

    #[test]
    fn missing_closing_paren_reports_precise_span() {
        let err = parse_command_query("compare(x, Int").expect_err("query should fail");
        assert_eq!(
            err.message(),
            "Invalid typed call query: missing closing `)`."
        );
        assert_eq!(err.span(), Span { start: 7, end: 14 });
    }

    #[test]
    fn empty_argument_reports_precise_span() {
        let err = parse_command_query("compare(Int, )").expect_err("query should fail");
        assert_eq!(err.message(), "Invalid typed call query: empty argument.");
        assert_eq!(err.span(), Span { start: 13, end: 13 });
    }
}
