use crate::ast::*;
use crate::error::ParseError;
use crate::token::Token;
use sindr::primitives::ToPrimitive;

use super::Parser;

#[derive(Debug, Clone, PartialEq)]
enum FuncLiteralBodyKind {
    Name(Symbol),
    Path(AstPath),
    Operator(String),
}

impl Parser<'_> {
    fn parse_func_literal_body(body: &str, span: Span) -> Result<FuncLiteralBodyKind, ParseError> {
        if Self::expr_binop_from_func_literal(body).is_some()
            || Self::logical_binop_from_func_literal(body).is_some()
        {
            return Ok(FuncLiteralBodyKind::Operator(body.to_string()));
        }

        if body.contains("::") {
            let segments = body.split("::").map(str::to_string).collect::<Vec<_>>();
            let is_valid_path = segments.len() >= 2
                && segments.iter().all(|segment| {
                    let mut chars = segment.chars();
                    matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
                        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                });
            if !is_valid_path {
                return Err(ParseError::syntax(
                    format!("Unsupported FuncLiteral body: `{}`", body),
                    span,
                ));
            }
            return Ok(FuncLiteralBodyKind::Path(AstPath { span, segments }));
        }

        Ok(FuncLiteralBodyKind::Name(body.to_string()))
    }

    fn flow_op_kind(tok: &Token) -> Option<u8> {
        match tok {
            Token::PipeApply => Some(0),
            Token::PipeMap => Some(1),
            Token::PipeBind => Some(2),
            Token::Compose => Some(3),
            Token::LiftCompose => Some(4),
            Token::KleisliCompose => Some(5),
            _ => None,
        }
    }

    fn skip_newlines_before_flow_op(&mut self) {
        if !matches!(self.peek(), Token::Newline) {
            return;
        }

        let save = self.pos;
        self.skip_newlines();
        if Self::flow_op_kind(self.peek()).is_none() {
            self.pos = save;
        }
    }

    pub(super) fn parse_expr(&mut self) -> Result<Ast, ParseError> {
        self.parse_flow_expr()
    }

    pub(super) fn parse_flow_expr(&mut self) -> Result<Ast, ParseError> {
        let mut left = self.parse_and_or_expr()?;
        loop {
            self.skip_newlines_before_flow_op();

            let next = match Self::flow_op_kind(self.peek()) {
                Some(kind) => kind,
                None => break,
            };
            self.advance();
            self.skip_newlines();
            let right = self.parse_and_or_expr()?;
            let span = Span {
                start: left.span().start,
                end: right.span().end,
            };
            left = match next {
                0 => Ast::Pipe(span, Box::new(left), Box::new(right)),
                1 => Ast::ContextMap(span, Box::new(left), Box::new(right)),
                2 => Ast::ContextBind(span, Box::new(left), Box::new(right)),
                3 => Ast::Compose(span, Box::new(left), Box::new(right)),
                4 => Ast::LiftedCompose(span, Box::new(left), Box::new(right)),
                5 => Ast::KleisliCompose(span, Box::new(left), Box::new(right)),
                _ => unreachable!("validated flow token"),
            };
        }
        Ok(left)
    }

    pub(super) fn stmt_has_top_level_assignment_from(&self, start: usize) -> bool {
        self.stmt_has_top_level_token_from(start, |token| {
            matches!(token, Token::Bind | Token::SafeBind)
        })
    }

    pub(super) fn stmt_has_top_level_at_from(&self, start: usize) -> bool {
        self.stmt_has_top_level_token_from(start, |token| matches!(token, Token::At))
    }

    fn stmt_has_top_level_token_from(
        &self,
        start: usize,
        predicate: impl Fn(&Token) -> bool,
    ) -> bool {
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;

        for token in self.tokens.iter().skip(start).map(|sp| &sp.token) {
            match token {
                Token::LParen => paren_depth += 1,
                Token::RParen => paren_depth = paren_depth.saturating_sub(1),
                Token::LBrack => bracket_depth += 1,
                Token::RBrack => bracket_depth = bracket_depth.saturating_sub(1),
                Token::LBrace => brace_depth += 1,
                Token::RBrace => {
                    if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 {
                        break;
                    }
                    brace_depth = brace_depth.saturating_sub(1);
                }
                Token::Newline | Token::Semicolon | Token::Eof
                    if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 =>
                {
                    break;
                }
                _ if paren_depth == 0
                    && bracket_depth == 0
                    && brace_depth == 0
                    && predicate(token) =>
                {
                    return true;
                }
                _ => {}
            }
        }

        false
    }

    // ── Infix operators grouped by OpKind ──

    pub(super) fn expr_binop(tok: &Token) -> Option<BinOp> {
        match tok {
            Token::Plus => Some(BinOp::Add),
            Token::Minus => Some(BinOp::Sub),
            Token::Star => Some(BinOp::Mul),
            Token::Concat => Some(BinOp::Concat),
            _ => None,
        }
    }

    pub(super) fn logical_binop(tok: &Token) -> Option<BinOp> {
        match tok {
            Token::EqEq => Some(BinOp::Eq),
            Token::BangEq => Some(BinOp::Neq),
            Token::Lt => Some(BinOp::Lt),
            Token::Gt => Some(BinOp::Gt),
            Token::LtEq => Some(BinOp::Lte),
            Token::GtEq => Some(BinOp::Gte),
            _ => None,
        }
    }

    pub(super) fn and_or_name(tok: &Token) -> Option<&'static str> {
        match tok {
            Token::AndAnd => Some("and"),
            Token::OrOr => Some("or"),
            _ => None,
        }
    }

    pub(super) fn expr_binop_from_func_literal(body: &str) -> Option<BinOp> {
        match body {
            "+" => Some(BinOp::Add),
            "-" => Some(BinOp::Sub),
            "*" => Some(BinOp::Mul),
            "++" => Some(BinOp::Concat),
            _ => None,
        }
    }

    pub(super) fn logical_binop_from_func_literal(body: &str) -> Option<BinOp> {
        match body {
            "==" => Some(BinOp::Eq),
            "!=" => Some(BinOp::Neq),
            "<" => Some(BinOp::Lt),
            ">" => Some(BinOp::Gt),
            "<=" => Some(BinOp::Lte),
            ">=" => Some(BinOp::Gte),
            _ => None,
        }
    }

    pub(super) fn and_or_func_literal_name(body: &str) -> bool {
        matches!(body, "and" | "or")
    }

    pub(super) fn comparison_func_literal_name(body: &str) -> bool {
        // `le` / `ge` are accepted here as comparison-style helper aliases.
        // They stay normal function calls, but parse at comparison precedence.
        matches!(
            body,
            "eq" | "neq" | "lt" | "lte" | "gt" | "gte" | "le" | "ge"
        )
    }

    pub(super) fn logical_func_literal_name(body: &str) -> bool {
        Self::and_or_func_literal_name(body) || Self::comparison_func_literal_name(body)
    }

    pub(super) fn lower_binop(left: Ast, op: BinOp, right: Ast) -> Ast {
        let span = Span {
            start: left.span().start,
            end: right.span().end,
        };
        Ast::BinOp(span, op, Box::new(left), Box::new(right))
    }

    pub(super) fn lower_func_literal_call(left: Ast, func: Ast, right: Ast) -> Ast {
        let span = Span {
            start: left.span().start,
            end: right.span().end,
        };
        Ast::App(
            span,
            Box::new(func),
            vec![
                RecordLitArg::Positional(left),
                RecordLitArg::Positional(right),
            ],
        )
    }

    pub(super) fn parse_expr_class_expr(&mut self) -> Result<Ast, ParseError> {
        let mut left = self.parse_postfix()?;

        loop {
            if let Some(op) = Self::expr_binop(self.peek()) {
                self.advance();
                let right = self.parse_postfix()?;
                left = Self::lower_binop(left, op, right);
                continue;
            }

            let Some(Token::FuncLiteral(body)) = self.peek_n(0).cloned() else {
                break;
            };
            let func_span = self.peek_span();
            let func_kind = Self::parse_func_literal_body(&body, func_span.clone())?;

            if matches!(func_kind, FuncLiteralBodyKind::Operator(ref op_body)
                if Self::logical_binop_from_func_literal(op_body).is_some())
                || matches!(func_kind, FuncLiteralBodyKind::Name(ref name)
                    if Self::logical_func_literal_name(name))
            {
                break;
            }

            self.advance();
            let right = self.parse_postfix()?;
            match func_kind {
                FuncLiteralBodyKind::Operator(op_body) => {
                    let op = Self::expr_binop_from_func_literal(&op_body)
                        .expect("expr operator classification checked above");
                    left = Self::lower_binop(left, op, right);
                }
                FuncLiteralBodyKind::Name(name) => {
                    left = Self::lower_func_literal_call(left, Ast::Var(func_span, name), right);
                }
                FuncLiteralBodyKind::Path(path) => {
                    left = Self::lower_func_literal_call(
                        left,
                        Ast::Path(path.span.clone(), path),
                        right,
                    );
                }
            }
        }

        Ok(left)
    }

    pub(super) fn parse_logical_expr(&mut self) -> Result<Ast, ParseError> {
        let mut left = self.parse_expr_class_expr()?;

        loop {
            if let Some(op) = Self::logical_binop(self.peek()) {
                self.advance();
                let right = self.parse_expr_class_expr()?;
                left = Self::lower_binop(left, op, right);
                continue;
            }

            let Some(Token::FuncLiteral(body)) = self.peek_n(0).cloned() else {
                break;
            };
            let func_span = self.peek_span();
            let func_kind = Self::parse_func_literal_body(&body, func_span.clone())?;

            if let FuncLiteralBodyKind::Operator(ref op_body) = func_kind {
                if let Some(op) = Self::logical_binop_from_func_literal(op_body) {
                    self.advance();
                    let right = self.parse_expr_class_expr()?;
                    left = Self::lower_binop(left, op, right);
                    continue;
                }
            }

            if matches!(func_kind, FuncLiteralBodyKind::Name(ref name)
                if Self::comparison_func_literal_name(name))
            {
                self.advance();
                let right = self.parse_expr_class_expr()?;
                left = Self::lower_func_literal_call(left, Ast::Var(func_span, body), right);
                continue;
            }

            break;
        }

        Ok(left)
    }

    pub(super) fn parse_and_or_expr(&mut self) -> Result<Ast, ParseError> {
        let mut left = self.parse_logical_expr()?;

        loop {
            if let Some(name) = Self::and_or_name(self.peek()) {
                let func_span = self.peek_span();
                self.advance();
                let right = self.parse_logical_expr()?;
                left = Self::lower_func_literal_call(
                    left,
                    Ast::Var(func_span, name.to_string()),
                    right,
                );
                continue;
            }

            let Some(Token::FuncLiteral(body)) = self.peek_n(0).cloned() else {
                break;
            };
            let func_span = self.peek_span();
            let func_kind = Self::parse_func_literal_body(&body, func_span.clone())?;
            let FuncLiteralBodyKind::Name(name) = func_kind else {
                break;
            };
            if !Self::and_or_func_literal_name(&name) {
                break;
            }

            self.advance();
            let right = self.parse_logical_expr()?;
            left = Self::lower_func_literal_call(left, Ast::Var(func_span, name), right);
        }

        Ok(left)
    }

    // ── Postfix (field access: expr.field) ──

    pub(super) fn parse_postfix(&mut self) -> Result<Ast, ParseError> {
        let mut expr = self.parse_primary()?;

        while matches!(self.peek(), Token::Dot) {
            self.advance(); // consume .
            let (field, fspan) = match self.peek().clone() {
                Token::Ident(field) => {
                    let span = self.advance().span.clone();
                    (field, span)
                }
                _ => {
                    return Err(ParseError::syntax(
                        "Expected field name after '.'. Tuple access uses ._0, ._1, ...",
                        self.peek_span(),
                    ));
                }
            };
            let span = Span {
                start: expr.span().start,
                end: fspan.end,
            };
            expr = Ast::FieldAccess(span, Box::new(expr), field);
        }

        if self.is_timeout_modifier_start() {
            expr = self.parse_timeout_modifier(expr)?;
        }

        Ok(expr)
    }

    // ── Primary ──

    pub(super) fn parse_primary(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();

        match self.peek().clone() {
            // Literals
            Token::Int(n) => {
                self.advance();
                if self.is_duration_suffix_here() {
                    let suffix_span = self.advance().span.clone();
                    let int_expr = Ast::Lit(sp.clone(), Lit::Int(n));
                    return Ok(self.hidden_call(
                        "__duration_literal",
                        vec![RecordLitArg::Positional(int_expr)],
                        Span {
                            start: sp.start,
                            end: suffix_span.end,
                        },
                    ));
                }
                Ok(Ast::Lit(sp, Lit::Int(n)))
            }
            Token::Float(f) => {
                self.advance();
                Ok(Ast::Lit(sp, Lit::Float(f)))
            }
            Token::Str(s) => {
                self.advance();
                self.parse_string_or_interpolated(sp, s)
            }
            Token::DocString(s) => {
                self.advance();
                self.parse_triple_string_or_interpolated(sp, s)
            }
            Token::True => {
                self.advance();
                Ok(Ast::Lit(sp, Lit::Bool(true)))
            }
            Token::False => {
                self.advance();
                Ok(Ast::Lit(sp, Lit::Bool(false)))
            }
            Token::Unit => {
                self.advance();
                Ok(Ast::Lit(sp, Lit::Unit))
            }

            // Negative number: unary minus
            Token::Minus => {
                self.advance();
                if let Token::Ident(name) = self.peek().clone() {
                    let name_span = self.peek_span();
                    return Err(ParseError::syntax(
                        format!(
                            "Unary minus on variables is not supported in Phase 1; write `0 - {}` instead of `-{}`",
                            name, name
                        ),
                        Span {
                            start: sp.start,
                            end: name_span.end,
                        },
                    ));
                }
                let inner = self.parse_primary()?;
                let end = inner.span().end;
                // Fold negative literals directly
                match inner {
                    Ast::Lit(_, Lit::Int(n)) => Ok(Ast::Lit(
                        Span {
                            start: sp.start,
                            end,
                        },
                        Lit::Int(-n),
                    )),
                    Ast::Lit(_, Lit::Float(f)) => Ok(Ast::Lit(
                        Span {
                            start: sp.start,
                            end,
                        },
                        Lit::Float(-f),
                    )),
                    _ => {
                        // General unary minus: desugar to 0 - expr (for Int)
                        // For now, only support literal negation
                        Err(ParseError::syntax(
                            "Unary minus is only supported on numeric literals in Phase 1; write `0 - expr` for general subtraction",
                            Span {
                                start: sp.start,
                                end,
                            },
                        ))
                    }
                }
            }

            // List expression: [], [a, b, c], [head, ..tail]
            Token::LBrack => self.parse_list_expr(sp),

            // Parenthesized expression
            Token::LParen => self.with_parse_nesting(sp.clone(), |parser| {
                parser.advance();
                parser.skip_newlines();
                let first = parser.parse_expr()?;
                parser.skip_newlines();
                if matches!(parser.peek(), Token::Comma) {
                    parser.advance();
                    parser.skip_newlines();
                    if matches!(parser.peek(), Token::RParen) {
                        return Err(ParseError::syntax(
                            "1-tuple literals are not supported",
                            Span {
                                start: sp.start,
                                end: parser.peek_span().end,
                            },
                        ));
                    }
                    let mut items = vec![first, parser.parse_expr()?];
                    parser.skip_newlines();
                    while matches!(parser.peek(), Token::Comma) {
                        parser.advance();
                        parser.skip_newlines();
                        if matches!(parser.peek(), Token::RParen) {
                            break;
                        }
                        items.push(parser.parse_expr()?);
                        parser.skip_newlines();
                    }
                    let end = parser.expect(&Token::RParen)?;
                    Ok(Ast::TupleLiteral(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        items,
                    ))
                } else {
                    let end = parser.expect(&Token::RParen)?;
                    Ok(Ast::Grouped(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        Box::new(first),
                    ))
                }
            }),

            // Zero-argument closure expression: { stmt; stmt; expr }
            Token::LBrace => self.parse_trailing_block_expr_from_lbrace(sp),

            // Capture / placeholder capture: &foo, &foo(&1), &1
            Token::Amp => self.parse_capture_expr(sp),

            Token::FuncLiteral(_) => Err(ParseError::syntax(
                "FuncLiteral must appear in infix position",
                sp,
            )),

            // Match expression
            Token::Match => self.parse_match_expr(),

            // Cond expression
            Token::Cond => self.parse_cond_expr(),

            // Identifier — could be: variable, binding, function call
            Token::Ident(name) => {
                self.advance();
                self.parse_ident_continuation(name, sp)
            }

            Token::Eof => Err(ParseError::incomplete("expression", sp)),
            _ => Err(ParseError::syntax(
                format!("Unexpected token: {:?}", self.peek()),
                sp,
            )),
        }
    }

    fn is_duration_suffix_here(&self) -> bool {
        matches!(self.peek(), Token::Ident(name) if name == "ms")
    }

    fn is_timeout_modifier_start(&self) -> bool {
        matches!(self.peek(), Token::At)
            && matches!(self.peek_n(1), Some(Token::Ident(name)) if name == "timeout")
    }

    fn hidden_call(&self, name: &str, args: Vec<RecordLitArg>, span: Span) -> Ast {
        Ast::App(
            span.clone(),
            Box::new(Ast::Var(
                Span {
                    start: span.start,
                    end: span.start,
                },
                name.to_string(),
            )),
            args,
        )
    }

    fn parse_timeout_duration_literal(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        let Token::Int(n) = self.peek().clone() else {
            return Err(ParseError::syntax(
                "@timeout(...) requires a duration literal like `100ms`",
                sp,
            ));
        };
        self.advance();
        if !self.is_duration_suffix_here() {
            return Err(ParseError::syntax(
                "@timeout(...) requires a duration literal like `100ms`",
                sp,
            ));
        }
        let suffix_span = self.advance().span.clone();
        let int_expr = Ast::Lit(sp.clone(), Lit::Int(n));
        Ok(self.hidden_call(
            "__duration_literal",
            vec![RecordLitArg::Positional(int_expr)],
            Span {
                start: sp.start,
                end: suffix_span.end,
            },
        ))
    }

    fn parse_timeout_modifier(&mut self, expr: Ast) -> Result<Ast, ParseError> {
        let start = expr.span().start;
        self.expect(&Token::At)?;
        let (modifier, _) = self.expect_ident()?;
        if modifier != "timeout" {
            return Err(ParseError::syntax(
                format!("Unsupported call modifier: @{modifier}"),
                self.peek_span(),
            ));
        }
        self.expect(&Token::LParen)?;
        let timeout_arg = self.parse_timeout_duration_literal()?;
        self.skip_newlines();
        let end_span = self.expect(&Token::RParen)?;

        let (target, mut args) = match expr {
            Ast::App(_, func, args) => (*func, args),
            _ => {
                return Err(ParseError::syntax(
                    "@timeout(...) can only be applied to Task calls",
                    Span {
                        start,
                        end: end_span.end,
                    },
                ));
            }
        };

        let hidden_name = match &target {
            Ast::Path(_, path) if path.segments.as_slice() == ["Task", "call"] => {
                "__task_call_timeout"
            }
            Ast::Path(_, path) if path.segments.as_slice() == ["Task", "async"] => {
                "__task_async_timeout"
            }
            Ast::Path(_, path) if path.segments.as_slice() == ["Task", "launch"] => {
                "__task_launch_timeout"
            }
            Ast::Path(_, path) if path.segments.as_slice() == ["Task", "cast"] => {
                "__task_cast_timeout"
            }
            _ => {
                return Err(ParseError::syntax(
                    "@timeout(...) is only supported on Task::call/async/launch/cast",
                    Span {
                        start,
                        end: end_span.end,
                    },
                ));
            }
        };

        if args.len() != 1 || matches!(args[0], RecordLitArg::Named(_, _)) {
            return Err(ParseError::syntax(
                "@timeout(...) expects a Task call with exactly one positional body argument",
                Span {
                    start,
                    end: end_span.end,
                },
            ));
        }

        args.insert(0, RecordLitArg::Positional(timeout_arg));
        Ok(self.hidden_call(
            hidden_name,
            args,
            Span {
                start,
                end: end_span.end,
            },
        ))
    }

    /// After seeing an identifier, figure out what it is:
    /// - `Name { field: val }` → StructLit (uppercase start + `{`)
    /// - `Name(args)` → ConstructorCall if uppercase, App if lowercase
    /// - `name()` / `Name()` → zero-arg call
    /// - `name: Type = expr` → Bind (annotated)
    /// - `name = expr` → Bind
    /// - otherwise → Var
    pub(super) fn parse_ident_continuation(
        &mut self,
        name: Symbol,
        name_span: Span,
    ) -> Result<Ast, ParseError> {
        if name == "dbg" && matches!(self.peek(), Token::Bang) {
            return self.parse_dbg_special_form(name_span);
        }

        if name == "self" && self.impl_target_stack.is_empty() {
            return Err(ParseError::syntax(
                "`self` can only be used inside impl methods",
                name_span,
            ));
        }

        let mut path_segments = vec![name.clone()];
        let mut path_end = name_span.end;
        while self.has_path_separator() && matches!(self.peek_n(2), Some(Token::Ident(_))) {
            self.consume_path_separator()?;
            let (seg, seg_span) = self.expect_ident()?;
            path_end = seg_span.end;
            path_segments.push(seg);
        }

        let path_ast = if path_segments.len() > 1 {
            Some(Ast::Path(
                Span {
                    start: name_span.start,
                    end: path_end,
                },
                AstPath {
                    span: Span {
                        start: name_span.start,
                        end: path_end,
                    },
                    segments: path_segments.clone(),
                },
            ))
        } else {
            None
        };

        if let Some(path_expr) = path_ast {
            let path_name = path_segments.join("::");
            let path_last_is_uppercase = path_segments
                .last()
                .and_then(|segment| segment.chars().next())
                .map(|ch| ch.is_uppercase())
                .unwrap_or(false);
            if matches!(self.peek(), Token::LParen) {
                self.advance();
                let args = self.parse_call_args()?;
                self.skip_newlines();
                let end_span = self.expect(&Token::RParen)?;
                if path_name == "Duration" {
                    if args.len() != 1 || matches!(args[0], RecordLitArg::Named(_, _)) {
                        return Err(ParseError::syntax(
                            "Duration(...) expects exactly one positional Int argument",
                            Span {
                                start: name_span.start,
                                end: end_span.end,
                            },
                        ));
                    }
                    let span = Span {
                        start: name_span.start,
                        end: end_span.end,
                    };
                    return Ok(self.hidden_call("__duration_from_int", args, span));
                }
                if path_last_is_uppercase {
                    self.reject_constructor_trailing_block()?;
                    let span = Span {
                        start: name_span.start,
                        end: end_span.end,
                    };
                    return Ok(Ast::ConstructorCall(span, path_name, args));
                }
                let (args, call_end) =
                    self.attach_trailing_block_arg(&path_expr, args, end_span.end)?;
                let span = Span {
                    start: name_span.start,
                    end: call_end,
                };
                return Ok(Ast::App(span, Box::new(path_expr), args));
            }

            if matches!(self.peek(), Token::Unit) {
                let end_span = self.advance().span.clone();
                if path_last_is_uppercase {
                    self.reject_constructor_trailing_block()?;
                    let span = Span {
                        start: name_span.start,
                        end: end_span.end,
                    };
                    return Ok(Ast::ConstructorCall(span, path_name, Vec::new()));
                }
                let (args, call_end) =
                    self.attach_trailing_block_arg(&path_expr, Vec::new(), end_span.end)?;
                let span = Span {
                    start: name_span.start,
                    end: call_end,
                };
                return Ok(Ast::App(span, Box::new(path_expr), args));
            }

            if path_last_is_uppercase {
                return Ok(Ast::ConstructorCall(
                    Span {
                        start: name_span.start,
                        end: path_end,
                    },
                    path_name,
                    Vec::new(),
                ));
            }

            return Ok(path_expr);
        }

        let is_uppercase = name
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false);

        // Regex generated literal sugar:
        //   re"pattern" / re'pattern'  => Regex::compile("pattern")
        if name == "re" {
            if let Token::Str(raw) = self.peek().clone() {
                let str_span = self.advance().span.clone();
                let pattern_expr = self.parse_string_or_interpolated(str_span.clone(), raw)?;
                let call_span = Span {
                    start: name_span.start,
                    end: pattern_expr.span().end,
                };
                let path = Ast::Path(
                    call_span.clone(),
                    AstPath {
                        span: call_span.clone(),
                        segments: vec!["Regex".into(), "compile".into()],
                    },
                );
                return Ok(Ast::App(
                    call_span,
                    Box::new(path),
                    vec![RecordLitArg::Positional(pattern_expr)],
                ));
            }
        }

        // Struct literal: Name { field: val, ... }
        if is_uppercase && matches!(self.peek(), Token::LBrace) {
            self.advance();
            self.skip_newlines();
            let mut fields = Vec::new();
            if !matches!(self.peek(), Token::RBrace) {
                loop {
                    self.skip_newlines();
                    let (field_name, _) = self.expect_ident()?;
                    self.expect(&Token::Colon)?;
                    let val = self.parse_non_assignment_expr()?;
                    fields.push((field_name, val));
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RBrace) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            self.skip_newlines();
            let end_span = self.expect(&Token::RBrace)?;
            let span = Span {
                start: name_span.start,
                end: end_span.end,
            };
            return Ok(Ast::StructLit(span, name, fields));
        }

        // Function call or constructor call: name(args)
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            let args = self.parse_call_args()?;
            self.skip_newlines();
            let end_span = self.expect(&Token::RParen)?;
            let func = Ast::Var(name_span.clone(), name.clone());

            if is_uppercase {
                if name == "Duration" {
                    if args.len() != 1 || matches!(args[0], RecordLitArg::Named(_, _)) {
                        return Err(ParseError::syntax(
                            "Duration(...) expects exactly one positional Int argument",
                            Span {
                                start: name_span.start,
                                end: end_span.end,
                            },
                        ));
                    }
                    let span = Span {
                        start: name_span.start,
                        end: end_span.end,
                    };
                    return Ok(self.hidden_call("__duration_from_int", args, span));
                }
                self.reject_constructor_trailing_block()?;
                let span = Span {
                    start: name_span.start,
                    end: end_span.end,
                };
                // Constructor call: Name(val, ...) or Name(field: val, ...)
                return Ok(Ast::ConstructorCall(span, name, args));
            } else {
                let (args, call_end) = self.attach_trailing_block_arg(&func, args, end_span.end)?;
                let span = Span {
                    start: name_span.start,
                    end: call_end,
                };
                // Normal function call
                return Ok(Ast::App(span, Box::new(func), args));
            }
        }

        // Zero-arg call: name() / Name()
        // Lexer tokenizes `()` as Token::Unit.
        if matches!(self.peek(), Token::Unit) {
            let end_span = self.advance().span.clone();
            let func = Ast::Var(name_span.clone(), name.clone());
            if is_uppercase {
                self.reject_constructor_trailing_block()?;
                let span = Span {
                    start: name_span.start,
                    end: end_span.end,
                };
                return Ok(Ast::ConstructorCall(span, name, Vec::new()));
            }
            let (args, call_end) =
                self.attach_trailing_block_arg(&func, Vec::new(), end_span.end)?;
            let span = Span {
                start: name_span.start,
                end: call_end,
            };
            return Ok(Ast::App(span, Box::new(func), args));
        }

        // Annotated binding: name: Type = expr / name: Type =? expr
        if matches!(self.peek(), Token::Colon) {
            self.advance();
            let ty = self.parse_type()?;
            if matches!(self.peek(), Token::Arrow) {
                return Err(ParseError::syntax(
                    "Parenthesized type signatures must choose tuple or function syntax after the first element: use `,` and another type for a tuple, or put `->` before `)` for a function type (for example, `(Int -> String)`, not `(Int) -> String`).",
                    self.peek_span(),
                ));
            }
            let assign_tok = self.peek().clone();
            match assign_tok {
                Token::Bind | Token::SafeBind => {
                    self.advance();
                }
                Token::Eof => {
                    return Err(ParseError::incomplete(
                        "assignment operator (= or =?)",
                        self.peek_span(),
                    ));
                }
                _ => {
                    return Err(ParseError::syntax(
                        format!(
                            "Expected assignment operator (= or =?), got {:?}",
                            self.peek()
                        ),
                        self.peek_span(),
                    ));
                }
            }
            let rhs = self.parse_expr()?;
            self.ensure_non_associative_assignment(&rhs)?;
            let span = Span {
                start: name_span.start,
                end: rhs.span().end,
            };
            let pat = AstPattern::Annotated(name_span, name, ty);
            return Ok(match assign_tok {
                Token::Bind => Ast::Bind(span, pat, Box::new(rhs)),
                Token::SafeBind => Ast::SafeBind(span, pat, Box::new(rhs)),
                _ => unreachable!("validated assignment token"),
            });
        }

        // Simple binding: name = expr / name =? expr
        if matches!(self.peek(), Token::Bind | Token::SafeBind) {
            let assign_tok = self.peek().clone();
            self.advance();
            let rhs = self.parse_expr()?;
            self.ensure_non_associative_assignment(&rhs)?;
            let span = Span {
                start: name_span.start,
                end: rhs.span().end,
            };
            let pat = AstPattern::Var(name_span, name);
            return Ok(match assign_tok {
                Token::Bind => Ast::Bind(span, pat, Box::new(rhs)),
                Token::SafeBind => Ast::SafeBind(span, pat, Box::new(rhs)),
                _ => unreachable!("validated assignment token"),
            });
        }

        // Just a variable
        Ok(Ast::Var(name_span, name))
    }

    fn parse_dbg_special_form(&mut self, name_span: Span) -> Result<Ast, ParseError> {
        let bang_span = self.advance().span.clone();
        if matches!(self.peek(), Token::Unit) {
            let unit_span = self.advance().span.clone();
            return Err(ParseError::syntax(
                "`dbg!` expects at least one argument",
                Span {
                    start: name_span.start,
                    end: unit_span.end,
                },
            ));
        }
        if !matches!(self.peek(), Token::LParen) {
            return Err(ParseError::syntax(
                "Expected `(` after `dbg!`",
                Span {
                    start: name_span.start,
                    end: bang_span.end,
                },
            ));
        }

        self.advance();
        self.skip_newlines();
        if matches!(self.peek(), Token::RParen) {
            return Err(ParseError::syntax(
                "`dbg!` expects at least one argument",
                self.peek_span(),
            ));
        }

        let mut args = Vec::new();
        loop {
            let expr = self.parse_non_assignment_expr()?;
            args.push(DbgArg {
                span: expr.span().clone(),
                expr,
            });
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::RParen) {
                    break;
                }
                continue;
            }
            break;
        }

        self.skip_newlines();
        let end_span = self.expect(&Token::RParen)?;
        Ok(Ast::Dbg(
            Span {
                start: name_span.start,
                end: end_span.end,
            },
            args,
        ))
    }

    /// `=` / `=?` are non-associative in a single statement.
    pub(super) fn ensure_non_associative_assignment(&self, rhs: &Ast) -> Result<(), ParseError> {
        if matches!(rhs, Ast::Bind(_, _, _) | Ast::SafeBind(_, _, _)) {
            return Err(ParseError::syntax(
                "`=` and `=?` are non-associative; a statement can contain only one assignment operator",
                rhs.span().clone(),
            ));
        }
        Ok(())
    }

    pub(super) fn with_trailing_call_block_disabled<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        let prev = self.allow_trailing_call_block;
        self.allow_trailing_call_block = false;
        let result = f(self);
        self.allow_trailing_call_block = prev;
        result
    }

    pub(super) fn parse_call_args(&mut self) -> Result<Vec<RecordLitArg>, ParseError> {
        self.skip_newlines();

        let mut args = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            args.push(self.parse_record_lit_arg()?);
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::RParen) {
                    break;
                }
                args.push(self.parse_record_lit_arg()?);
            }
        }

        Ok(args)
    }

    pub(super) fn parse_trailing_block_expr_from_lbrace(
        &mut self,
        sp: Span,
    ) -> Result<Ast, ParseError> {
        self.with_parse_nesting(sp.clone(), |parser| {
            parser.expect(&Token::LBrace)?;
            parser.skip_newlines();

            if matches!(parser.peek(), Token::Pipe) {
                return parser.parse_closure_literal(sp);
            }

            let body_stmts = parser.parse_block_stmts()?;
            if body_stmts.is_empty() {
                return Err(ParseError::incomplete("expression", parser.peek_span()));
            }
            let end = parser.expect(&Token::RBrace)?;
            let body = if body_stmts.len() == 1 {
                body_stmts.into_iter().next().expect("checked non-empty")
            } else {
                Ast::Block(
                    Span {
                        start: body_stmts[0].span().start,
                        end: body_stmts[body_stmts.len() - 1].span().end,
                    },
                    body_stmts,
                )
            };
            Ok(Ast::Closure(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                Vec::new(),
                Box::new(body),
            ))
        })
    }

    pub(super) fn parse_match_arm_body(&mut self) -> Result<Ast, ParseError> {
        if matches!(self.peek(), Token::LBrace) {
            let sp = self.peek_span();
            return self.parse_match_arm_block_expr_from_lbrace(sp);
        }
        self.parse_expr()
    }

    pub(super) fn parse_match_arm_block_expr_from_lbrace(
        &mut self,
        sp: Span,
    ) -> Result<Ast, ParseError> {
        self.with_parse_nesting(sp.clone(), |parser| {
            parser.expect(&Token::LBrace)?;
            parser.skip_newlines();

            if matches!(parser.peek(), Token::Pipe) {
                return parser.parse_closure_literal(sp);
            }

            let stmts = parser.parse_block_stmts()?;
            if stmts.is_empty() {
                return Err(ParseError::incomplete("expression", parser.peek_span()));
            }
            let end = parser.expect(&Token::RBrace)?;
            Ok(Ast::Block(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                stmts,
            ))
        })
    }

    pub(super) fn reject_constructor_trailing_block(&self) -> Result<(), ParseError> {
        if self.allow_trailing_call_block && matches!(self.peek(), Token::LBrace) {
            return Err(ParseError::syntax(
                "Trailing block sugar is not supported for constructor calls",
                self.peek_span(),
            ));
        }
        Ok(())
    }

    pub(super) fn attach_trailing_block_arg(
        &mut self,
        _callee: &Ast,
        mut args: Vec<RecordLitArg>,
        mut call_end: usize,
    ) -> Result<(Vec<RecordLitArg>, usize), ParseError> {
        if !self.allow_trailing_call_block || !matches!(self.peek(), Token::LBrace) {
            return Ok((args, call_end));
        }

        if args
            .iter()
            .any(|arg| matches!(arg, RecordLitArg::Named(_, _)))
        {
            return Err(ParseError::syntax(
                "Trailing block sugar cannot follow named arguments",
                self.peek_span(),
            ));
        }

        let trailing = self.parse_trailing_block_expr_from_lbrace(self.peek_span())?;
        call_end = trailing.span().end;
        args.push(RecordLitArg::Positional(trailing));
        Ok((args, call_end))
    }

    /// Parse a record literal argument: either positional or named.
    pub(super) fn parse_record_lit_arg(&mut self) -> Result<RecordLitArg, ParseError> {
        // Peek ahead: if IDENT followed by `:`, it's named
        if let Token::Ident(name) = self.peek().clone() {
            let save = self.pos;
            let _name_span = self.peek_span();
            self.advance();
            if matches!(self.peek(), Token::Colon) && !self.has_path_separator() {
                self.advance();
                let val = self.parse_non_assignment_expr()?;
                return Ok(RecordLitArg::Named(name, val));
            }
            // Not named, restore and parse as expression
            self.pos = save;
        }
        let expr = self.parse_non_assignment_expr()?;
        Ok(RecordLitArg::Positional(expr))
    }

    pub(super) fn parse_non_assignment_expr(&mut self) -> Result<Ast, ParseError> {
        let expr = self.parse_expr()?;
        if matches!(expr, Ast::Bind(_, _, _) | Ast::SafeBind(_, _, _)) {
            Err(ParseError::syntax(
                "Assignments (`=` and `=?`) are statements and cannot appear in argument position",
                expr.span().clone(),
            ))
        } else {
            Ok(expr)
        }
    }

    pub(super) fn parse_list_expr(&mut self, sp: Span) -> Result<Ast, ParseError> {
        self.with_parse_nesting(sp.clone(), |parser| {
            parser.expect(&Token::LBrack)?;
            parser.skip_newlines();

            if matches!(parser.peek(), Token::RBrack) {
                let end = parser.expect(&Token::RBrack)?;
                return Ok(Ast::ListNil(Span {
                    start: sp.start,
                    end: end.end,
                }));
            }

            let first = parser.parse_expr()?;
            parser.skip_newlines();
            if matches!(parser.peek(), Token::Comma) {
                parser.advance();
                parser.skip_newlines();
                if matches!(parser.peek(), Token::DotDot) {
                    parser.advance();
                    parser.skip_newlines();
                    let tail = parser.parse_expr()?;
                    parser.skip_newlines();
                    let end = parser.expect(&Token::RBrack)?;
                    return Ok(Ast::ListCons(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        Box::new(first),
                        Box::new(tail),
                    ));
                }

                let mut elems = vec![first];
                elems.push(parser.parse_expr()?);
                while matches!(parser.peek(), Token::Comma) {
                    parser.advance();
                    parser.skip_newlines();
                    if matches!(parser.peek(), Token::RBrack) {
                        break;
                    }
                    elems.push(parser.parse_expr()?);
                }
                parser.skip_newlines();
                let end = parser.expect(&Token::RBrack)?;
                return Ok(Ast::ListLiteral(
                    Span {
                        start: sp.start,
                        end: end.end,
                    },
                    elems,
                ));
            }

            let end = parser.expect(&Token::RBrack)?;
            Ok(Ast::ListLiteral(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                vec![first],
            ))
        })
    }

    // ── Type annotation parsing ──

    pub(super) fn parse_closure_literal(&mut self, sp: Span) -> Result<Ast, ParseError> {
        self.expect(&Token::Pipe)?;
        self.skip_newlines();

        let mut params = Vec::new();
        if !matches!(self.peek(), Token::Pipe) {
            loop {
                let (name, pspan) = self.expect_ident()?;
                let ty = if matches!(self.peek(), Token::Colon) {
                    self.advance();
                    self.skip_newlines();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                params.push(ClosureParam {
                    name,
                    ty,
                    span: pspan,
                });
                self.skip_newlines();
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                    continue;
                }
                break;
            }
        }

        self.expect(&Token::Pipe)?;
        self.skip_newlines();
        let body_stmts = self.parse_block_stmts()?;
        if body_stmts.is_empty() {
            return Err(ParseError::incomplete("expression", self.peek_span()));
        }
        let body = if body_stmts.len() == 1 {
            body_stmts.into_iter().next().expect("checked non-empty")
        } else {
            Ast::Block(
                Span {
                    start: body_stmts[0].span().start,
                    end: body_stmts[body_stmts.len() - 1].span().end,
                },
                body_stmts,
            )
        };
        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::Closure(
            Span {
                start: sp.start,
                end: end.end,
            },
            params,
            Box::new(body),
        ))
    }

    pub(super) fn parse_capture_expr(&mut self, sp: Span) -> Result<Ast, ParseError> {
        self.expect(&Token::Amp)?;
        if let Token::Int(n) = self.peek().clone() {
            let span = self.advance().span.clone();
            let Some(index) = n.to_usize() else {
                return Err(ParseError::syntax(
                    "capture placeholder index must be a positive integer",
                    span,
                ));
            };
            if index == 0 {
                return Err(ParseError::syntax(
                    "capture placeholder index starts at &1",
                    span,
                ));
            }
            return Ok(Ast::CapturePlaceholder(
                Span {
                    start: sp.start,
                    end: span.end,
                },
                index,
            ));
        }
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            self.skip_newlines();
            let inner = self.parse_expr()?;
            self.skip_newlines();
            let end_span = self.expect(&Token::RParen)?;
            let message = match inner {
                Ast::CapturePlaceholder(_, 1) => {
                    "anonymous capture is not supported; use `&id` instead".to_string()
                }
                _ => "anonymous capture is not supported; extract a named function and capture it like `&fun_name(&1, &2)`".to_string(),
            };
            return Err(ParseError::syntax(
                message,
                Span {
                    start: sp.start,
                    end: end_span.end,
                },
            ));
        }
        let (target, mut end) = match self.peek().clone() {
            Token::Ident(_) => {
                let (name, name_span) = self.expect_ident()?;
                let mut path_segments = vec![name.clone()];
                let mut path_end = name_span.end;
                while self.has_path_separator() && matches!(self.peek_n(2), Some(Token::Ident(_))) {
                    self.consume_path_separator()?;
                    let (seg, seg_span) = self.expect_ident()?;
                    path_end = seg_span.end;
                    path_segments.push(seg);
                }

                let target = if path_segments.len() == 1 {
                    Ast::Var(name_span.clone(), name)
                } else {
                    Ast::Path(
                        Span {
                            start: name_span.start,
                            end: path_end,
                        },
                        AstPath {
                            span: Span {
                                start: name_span.start,
                                end: path_end,
                            },
                            segments: path_segments,
                        },
                    )
                };
                (target, path_end)
            }
            Token::FuncLiteral(body) => {
                let func_span = self.advance().span.clone();
                match Self::parse_func_literal_body(&body, func_span.clone())? {
                    FuncLiteralBodyKind::Name(name) => {
                        (Ast::Var(func_span.clone(), name), func_span.end)
                    }
                    FuncLiteralBodyKind::Path(path) => {
                        let end = path.span.end;
                        (Ast::Path(path.span.clone(), path), end)
                    }
                    FuncLiteralBodyKind::Operator(body) => (
                        Ast::FuncLiteralRef(
                            func_span.clone(),
                            FuncLiteralRef {
                                span: func_span.clone(),
                                body,
                            },
                        ),
                        func_span.end,
                    ),
                }
            }
            _ => {
                return Err(ParseError::syntax(
                    format!("Expected identifier, got {:?}", self.peek()),
                    self.peek_span(),
                ))
            }
        };

        let mut parsed_args = Vec::new();
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            self.skip_newlines();
            if !matches!(self.peek(), Token::RParen) {
                parsed_args.push(self.parse_record_lit_arg()?);
                while matches!(self.peek(), Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                    if matches!(self.peek(), Token::RParen) {
                        break;
                    }
                    parsed_args.push(self.parse_record_lit_arg()?);
                }
            }
            self.skip_newlines();
            let end_span = self.expect(&Token::RParen)?;
            end = end_span.end;
        }

        let mut args = Vec::new();
        for arg in parsed_args {
            match arg {
                RecordLitArg::Positional(expr) => args.push(expr),
                RecordLitArg::Named(arg_name, _) => {
                    return Err(ParseError::syntax(
                        format!("capture does not accept named argument '{}'", arg_name),
                        Span {
                            start: sp.start,
                            end,
                        },
                    ));
                }
            }
        }

        Ok(Ast::Capture(
            Span {
                start: sp.start,
                end,
            },
            Box::new(target),
            args,
        ))
    }

    pub(super) fn parse_match_expr(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Match)?;
        let scrutinee = self.with_trailing_call_block_disabled(|parser| parser.parse_expr())?;
        self.skip_newlines();
        let lbrace = self.expect(&Token::LBrace)?;
        self.skip_newlines();

        if matches!(self.peek(), Token::RBrace) {
            return Err(ParseError::syntax(
                "Match expression must contain at least one arm",
                lbrace,
            ));
        }

        let mut arms = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            self.skip_newlines();
            let mut patterns = expand_top_level_or_pattern(self.parse_match_pattern()?);
            while matches!(self.peek(), Token::Pipe) {
                self.advance();
                self.skip_newlines();
                patterns.extend(expand_top_level_or_pattern(self.parse_match_pattern()?));
            }
            let guard = if matches!(self.peek(), Token::When) {
                self.advance();
                self.skip_newlines();
                Some(self.parse_non_assignment_expr()?)
            } else {
                None
            };
            self.expect(&Token::FatArrow)?;
            let body = self.parse_match_arm_body()?;
            for pattern in patterns {
                arms.push(AstMatchArm {
                    pattern,
                    guard: guard.clone(),
                    body: body.clone(),
                });
            }
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::Match(
            Span {
                start: sp.start,
                end: end.end,
            },
            Box::new(scrutinee),
            arms,
        ))
    }

    /// `cond { cond1 => expr1, ..., True => exprN }`
    pub(super) fn parse_cond_expr(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Cond)?;
        self.skip_newlines();
        let lbrace = self.expect(&Token::LBrace)?;
        self.skip_newlines();

        if matches!(self.peek(), Token::RBrace) {
            return Err(ParseError::syntax(
                "Cond expression must contain at least one clause",
                lbrace,
            ));
        }

        let mut clauses = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            self.skip_newlines();
            let cond = self.parse_expr()?;
            self.expect(&Token::FatArrow)?;
            let body = self.parse_expr()?;
            clauses.push((cond, body));
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        let end = self.expect(&Token::RBrace)?;

        for (idx, (cond, _)) in clauses.iter().enumerate() {
            if Self::is_true_literal(cond) && idx + 1 != clauses.len() {
                return Err(ParseError::syntax(
                    "`True` clause must be the final cond clause",
                    cond.span().clone(),
                ));
            }
        }

        let Some((last_cond, last_body)) = clauses.pop() else {
            unreachable!("checked non-empty clauses");
        };
        if !Self::is_true_literal(&last_cond) {
            return Err(ParseError::syntax(
                "Final cond clause must use `True` as its condition",
                last_cond.span().clone(),
            ));
        }

        let mut expr = last_body;
        while let Some((cond, body)) = clauses.pop() {
            let span = Span {
                start: sp.start,
                end: end.end,
            };
            expr = Ast::App(
                span,
                Box::new(Ast::Var(sp.clone(), "if".to_string())),
                vec![
                    RecordLitArg::Positional(cond),
                    RecordLitArg::Positional(body),
                    RecordLitArg::Positional(expr),
                ],
            );
        }

        Ok(expr)
    }

    /// Match pattern now reuses the same grammar as bind/safe-bind patterns.
    pub(super) fn is_true_literal(expr: &Ast) -> bool {
        matches!(expr, Ast::Lit(_, Lit::Bool(true)))
    }
}

fn expand_top_level_or_pattern(pattern: AstPattern) -> Vec<AstPattern> {
    match pattern {
        AstPattern::Or(_, patterns) => patterns,
        pattern => vec![pattern],
    }
}
