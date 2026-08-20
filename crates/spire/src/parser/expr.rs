use crate::ast::*;
use crate::error::ParseError;
use crate::func_literal::{
    func_literal_operator, func_literal_operator_token, parse_func_literal_path,
    FuncLiteralOperator, FuncLiteralOperatorKind, FuncLiteralOperatorTier,
};
use crate::token::Token;
use sindr::primitives::ToPrimitive;

use super::Parser;

#[derive(Debug, Clone, PartialEq)]
enum FuncLiteralBodyKind {
    Name(Symbol),
    Path(AstPath),
    Operator(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlowOpKind {
    PipeApply,
    PipeMap,
    PipeApplyContext,
    PipeBind,
    Compose,
    LiftCompose,
    KleisliCompose,
    Choice,
}

impl Parser<'_> {
    pub(super) fn assignment_ast(
        assign_tok: Token,
        span: Span,
        pat: AstPattern,
        rhs: Ast,
    ) -> Result<Ast, ParseError> {
        match assign_tok {
            Token::Bind => Ok(Ast::Bind(span, pat, Box::new(rhs))),
            Token::SafeBind => Ok(Ast::SafeBind(span, pat, Box::new(rhs))),
            other => Err(ParseError::syntax(
                format!("Expected assignment operator (= or =?), got {:?}", other),
                span,
            )),
        }
    }

    fn function_on_path(span: Span) -> Ast {
        Ast::Path(
            span.clone(),
            AstPath {
                span,
                segments: vec!["Function".into(), "on".into()],
            },
        )
    }

    fn is_function_on_path(path: &AstPath) -> bool {
        matches!(path.segments.as_slice(), [module, name] if module == "Function" && name == "on")
    }

    fn low_precedence_on_target_callee(kind: &FuncLiteralBodyKind, span: &Span) -> Option<Ast> {
        match kind {
            FuncLiteralBodyKind::Name(name) if name == "on" => {
                Some(Self::function_on_path(span.clone()))
            }
            FuncLiteralBodyKind::Path(path) if Self::is_function_on_path(path) => {
                Some(Ast::Path(path.span.clone(), path.clone()))
            }
            _ => None,
        }
    }

    fn parse_func_literal_body(body: &str, span: Span) -> Result<FuncLiteralBodyKind, ParseError> {
        if func_literal_operator(body).is_some() {
            return Ok(FuncLiteralBodyKind::Operator(body.to_string()));
        }

        if let Some(segments) = parse_func_literal_path(body) {
            return Ok(FuncLiteralBodyKind::Path(AstPath { span, segments }));
        }

        if !crate::func_literal::is_func_literal_ident(body) {
            return Err(ParseError::syntax(
                format!("Unsupported FuncLiteral body: `{}`", body),
                span,
            ));
        }

        Ok(FuncLiteralBodyKind::Name(body.to_string()))
    }

    fn flow_op_kind(tok: &Token) -> Option<FlowOpKind> {
        match tok {
            Token::PipeApply => Some(FlowOpKind::PipeApply),
            Token::PipeMap => Some(FlowOpKind::PipeMap),
            Token::PipeApplyContext => Some(FlowOpKind::PipeApplyContext),
            Token::PipeBind => Some(FlowOpKind::PipeBind),
            Token::Compose => Some(FlowOpKind::Compose),
            Token::LiftCompose => Some(FlowOpKind::LiftCompose),
            Token::KleisliCompose => Some(FlowOpKind::KleisliCompose),
            Token::Choice => Some(FlowOpKind::Choice),
            _ => None,
        }
    }

    fn flow_op_injects_first_argument(kind: FlowOpKind) -> bool {
        matches!(
            kind,
            FlowOpKind::PipeApply | FlowOpKind::PipeMap | FlowOpKind::PipeBind
        )
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
        self.parse_on_expr()
    }

    pub(super) fn parse_on_expr(&mut self) -> Result<Ast, ParseError> {
        let mut left = self.parse_flow_expr()?;

        loop {
            let Some(Token::FuncLiteral(body)) = self.peek_n(0).cloned() else {
                break;
            };
            let func_span = self.peek_span();
            let func_kind = Self::parse_func_literal_body(&body, func_span.clone())?;
            let Some(func) = Self::low_precedence_on_target_callee(&func_kind, &func_span) else {
                break;
            };

            self.advance();
            let right = self.parse_flow_expr()?;
            left = Self::lower_func_literal_call(left, func, right);
        }

        Ok(left)
    }

    pub(super) fn parse_flow_expr(&mut self) -> Result<Ast, ParseError> {
        let mut left = self.parse_and_or_expr()?;
        loop {
            // Choice deliberately does not inherit flow's newline-continuation
            // rule: `<|>` must remain on a single source line.
            if !matches!(self.peek(), Token::Newline) {
                // Already at an operator or ordinary expression token.
            } else {
                let save = self.pos;
                self.skip_newlines_before_flow_op();
                if matches!(self.peek(), Token::Choice) {
                    self.pos = save;
                    break;
                }
            }

            let next = match Self::flow_op_kind(self.peek()) {
                Some(kind) => kind,
                None => break,
            };
            self.advance();
            if matches!(next, FlowOpKind::Choice) && matches!(self.peek(), Token::Newline) {
                return Err(ParseError::syntax(
                    "`<|>` must have its right operand on the same line",
                    self.peek_span(),
                ));
            }
            if !matches!(next, FlowOpKind::Choice) {
                self.skip_newlines();
            }
            let direct_partial_pair_call = if Self::flow_op_injects_first_argument(next) {
                match self.peek().clone() {
                    Token::FuncLiteral(body) if Self::is_pair_constructor_func_literal(&body) => {
                        Some((self.peek_span(), body))
                    }
                    _ => None,
                }
            } else {
                None
            };
            let right = if let Some((func_span, body)) = direct_partial_pair_call {
                let pair_call = self.parse_quoted_callee_call(func_span, body, true)?;
                if !matches!(
                    self.peek(),
                    Token::Newline
                        | Token::Semicolon
                        | Token::Comma
                        | Token::RParen
                        | Token::RBrack
                        | Token::RBrace
                        | Token::Eof
                ) && Self::flow_op_kind(self.peek()).is_none()
                {
                    return Err(ParseError::syntax(
                        "quoted pair constructor partial call must be the complete pipeline RHS",
                        pair_call.span().clone(),
                    ));
                }
                pair_call
            } else {
                self.parse_and_or_expr()?
            };
            let span = Span {
                start: left.span().start,
                end: right.span().end,
            };
            left = match next {
                FlowOpKind::PipeApply => Ast::Pipe(span, Box::new(left), Box::new(right)),
                FlowOpKind::PipeMap => Ast::ContextMap(span, Box::new(left), Box::new(right)),
                FlowOpKind::PipeApplyContext => {
                    Ast::ContextApply(span, Box::new(left), Box::new(right))
                }
                FlowOpKind::PipeBind => Ast::ContextBind(span, Box::new(left), Box::new(right)),
                FlowOpKind::Compose => Ast::Compose(span, Box::new(left), Box::new(right)),
                FlowOpKind::LiftCompose => {
                    Ast::LiftedCompose(span, Box::new(left), Box::new(right))
                }
                FlowOpKind::KleisliCompose => {
                    Ast::KleisliCompose(span, Box::new(left), Box::new(right))
                }
                FlowOpKind::Choice => {
                    Ast::BinOp(span, BinOp::Choice, Box::new(left), Box::new(right))
                }
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
            Token::Slash => Some(BinOp::Slash),
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
            // Keep symbolic logical operators distinguishable until resolution
            // so local `and` / `or` bindings cannot retarget `&&` / `||`.
            Token::AndAnd => Some("&&"),
            Token::OrOr => Some("||"),
            _ => None,
        }
    }

    pub(super) fn expr_binop_from_func_literal(body: &str) -> Option<BinOp> {
        let operator = func_literal_operator(body)?;
        match (operator.tier, operator.kind) {
            (FuncLiteralOperatorTier::Expr, FuncLiteralOperatorKind::BinOp(op)) => Some(op),
            _ => None,
        }
    }

    pub(super) fn logical_binop_from_func_literal(body: &str) -> Option<BinOp> {
        let operator = func_literal_operator(body)?;
        match (operator.tier, operator.kind) {
            (FuncLiteralOperatorTier::Logical, FuncLiteralOperatorKind::BinOp(op)) => Some(op),
            _ => None,
        }
    }

    fn is_pair_constructor_func_literal(body: &str) -> bool {
        matches!(
            func_literal_operator(body),
            Some(FuncLiteralOperator {
                kind: FuncLiteralOperatorKind::PairConstructor,
                ..
            })
        )
    }

    fn pair_constructor_at(&self) -> bool {
        matches!(
            (self.peek(), self.peek_n(1), self.peek_n(2)),
            (Token::LParen, Some(Token::Comma), Some(Token::RParen))
        )
    }

    fn pair_constructor_span(&self) -> Span {
        Span {
            start: self.peek_span().start,
            end: self.tokens[self.pos + 2].span.end,
        }
    }

    fn consume_pair_constructor(&mut self) -> Span {
        let start = self.advance().span.start;
        self.advance();
        let end = self.advance().span.end;
        Span { start, end }
    }

    pub(super) fn and_or_func_literal_name(body: &str) -> bool {
        matches!(body, "and" | "or")
    }

    pub(super) fn comparison_func_literal_name(body: &str) -> bool {
        matches!(body, "eq" | "neq")
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

    pub(super) fn lower_pair_constructor(left: Ast, right: Ast) -> Ast {
        let span = Span {
            start: left.span().start,
            end: right.span().end,
        };
        Ast::TupleLiteral(span, vec![left, right])
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
                if Self::logical_binop_from_func_literal(op_body).is_some()
                    || Self::is_pair_constructor_func_literal(op_body))
                || matches!(func_kind, FuncLiteralBodyKind::Name(ref name)
                    if Self::logical_func_literal_name(name))
                || Self::low_precedence_on_target_callee(&func_kind, &func_span).is_some()
            {
                break;
            }

            self.advance();
            let right = self.parse_postfix()?;
            match func_kind {
                FuncLiteralBodyKind::Operator(op_body) => {
                    let Some(op) = Self::expr_binop_from_func_literal(&op_body) else {
                        return Err(ParseError::syntax(
                            format!("Unsupported FuncLiteral body: `{}`", op_body),
                            func_span,
                        ));
                    };
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

    /// Parse the pair-constructor tier: `Expr > (,) > Compare`.
    /// The recursive RHS preserves right associativity and nested tuple shape.
    pub(super) fn parse_pair_expr(&mut self) -> Result<Ast, ParseError> {
        let left = self.parse_expr_class_expr()?;
        let is_pair = self.pair_constructor_at()
            || matches!(self.peek(), Token::FuncLiteral(body) if Self::is_pair_constructor_func_literal(body));
        if !is_pair {
            return Ok(left);
        }

        if self.pair_constructor_at() {
            self.consume_pair_constructor();
        } else {
            self.advance();
        }
        let right = self.parse_pair_expr()?;
        Ok(Self::lower_pair_constructor(left, right))
    }

    pub(super) fn parse_logical_expr(&mut self) -> Result<Ast, ParseError> {
        let mut left = self.parse_pair_expr()?;

        loop {
            if let Some(op) = Self::logical_binop(self.peek()) {
                self.advance();
                let right = self.parse_pair_expr()?;
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
                    let right = self.parse_pair_expr()?;
                    left = Self::lower_binop(left, op, right);
                    continue;
                }
            }

            if matches!(func_kind, FuncLiteralBodyKind::Name(ref name)
                if Self::comparison_func_literal_name(name))
            {
                self.advance();
                let right = self.parse_pair_expr()?;
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
            self.advance();
            let (segment, segment_span) = self.parse_facet_path_segment_after_dot()?;
            let span = Span {
                start: expr.span().start,
                end: segment_span.end,
            };
            expr = match segment {
                FacetPathSegment::Field {
                    name,
                    optional: false,
                } => Ast::FieldAccess(span, Box::new(expr), name),
                other => Ast::FacetSegmentAccess(span, Box::new(expr), other),
            };
        }

        if self.is_timeout_modifier_start() {
            expr = self.parse_timeout_modifier(expr)?;
        }

        Ok(expr)
    }

    fn explicit_type_args_start(&self) -> bool {
        self.has_path_separator() && matches!(self.peek_n(2), Some(Token::Lt))
    }

    fn parse_explicit_type_apply(&mut self, target: Ast) -> Result<Ast, ParseError> {
        let start = target.span().start;
        self.consume_path_separator()?;
        self.expect(&Token::Lt)?;
        self.skip_newlines();
        if matches!(self.peek(), Token::Gt) {
            return Err(ParseError::syntax(
                "Explicit type arguments cannot be empty",
                self.peek_span(),
            ));
        }
        let mut args = vec![self.parse_type_in_impl_context(None)?];
        self.skip_newlines();
        while matches!(self.peek(), Token::Comma) {
            self.advance();
            self.skip_newlines();
            if matches!(self.peek(), Token::Gt) {
                return Err(ParseError::syntax(
                    "Explicit type arguments cannot end with a comma",
                    self.peek_span(),
                ));
            }
            args.push(self.parse_type_in_impl_context(None)?);
            self.skip_newlines();
        }
        let end = self.expect_type_gt()?;
        Ok(Ast::TypeApply(
            Span {
                start,
                end: end.end,
            },
            Box::new(target),
            args,
        ))
    }

    fn parse_facet_path_segment_after_dot(
        &mut self,
    ) -> Result<(FacetPathSegment, Span), ParseError> {
        match self.peek() {
            Token::Ident(_) => {
                let (name, name_span) = self.expect_ident()?;
                if matches!(self.peek(), Token::Question) {
                    let question_span = self.advance().span;
                    Ok((FacetPathSegment::optional_field(name), question_span))
                } else {
                    Ok((FacetPathSegment::field(name), name_span))
                }
            }
            Token::True | Token::False => {
                let token = self.advance();
                let name = match token.token {
                    Token::True => "True",
                    Token::False => "False",
                    _ => unreachable!("matched Boolean variant token"),
                };
                if matches!(self.peek(), Token::Question) {
                    let question_span = self.advance().span;
                    Ok((FacetPathSegment::optional_field(name.to_string()), question_span))
                } else {
                    Ok((FacetPathSegment::field(name.to_string()), token.span))
                }
            }
            Token::LBrack => {
                self.advance();
                let start_expr = self.parse_expr()?;
                let expr = if matches!(self.peek(), Token::DotDot) {
                    self.advance();
                    let end_expr = self.parse_expr()?;
                    let span = Span {
                        start: start_expr.span().start,
                        end: end_expr.span().end,
                    };
                    Ast::RangeLiteral(span, Box::new(start_expr), Box::new(end_expr))
                } else {
                    start_expr
                };
                let rbrack = self.expect(&Token::RBrack)?;
                let display = self.source[expr.span().start..expr.span().end].to_string();
                Ok((
                    FacetPathSegment::Bracket(FacetBracketExpr {
                        expr: Box::new(expr),
                        display,
                    }),
                    rbrack,
                ))
            }
            Token::Int(_) => Err(ParseError::syntax(
                "Expected field name after '.'. Tuple access uses ._0, ._1, ...",
                self.peek_span(),
            )),
            other => Err(ParseError::syntax(
                format!(
                    "Facet path segment after `.` expects identifier or bracket segment, got {other:?}"
                ),
                self.peek_span(),
            )),
        }
    }

    // ── Primary ──

    pub(super) fn parse_primary(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();

        match self.peek().clone() {
            Token::Bang => {
                self.advance();
                let inner = self.parse_primary()?;
                let span = Span {
                    start: sp.start,
                    end: inner.span().end,
                };
                Ok(Ast::App(
                    span.clone(),
                    Box::new(Ast::Path(
                        span.clone(),
                        AstPath {
                            span: span.clone(),
                            segments: vec!["Boolean".into(), "not".into()],
                        },
                    )),
                    vec![RecordLitArg::Positional(inner)],
                ))
            }

            // Literals
            Token::Int(n) => {
                self.advance();
                if self.is_duration_suffix_here() {
                    let suffix_span = self.advance().span.clone();
                    let int_expr = Ast::Lit(sp.clone(), Lit::Int(n));
                    return Ok(self.duration_literal(
                        int_expr,
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
            Token::LParen if self.pair_constructor_at() => Err(ParseError::syntax(
                "bare `(,)` is only valid in infix position",
                self.pair_constructor_span(),
            )),
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
            Token::Tilde => self.parse_facet_capture_expr(sp),
            Token::Caret => Err(ParseError::syntax(
                "Pin operator ^ is only allowed in MatchBlock patterns and bulk_update paths.",
                sp,
            )),

            Token::FuncLiteral(body) => self.parse_quoted_callee_call(sp, body, false),

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

    /// Parse a backtick-quoted callee such as `` `Add::add`(1, 2) ``.
    /// Quoting is syntactic only: it never creates a function value.
    fn parse_quoted_callee_call(
        &mut self,
        span: Span,
        body: String,
        allow_partial_pair_constructor_call: bool,
    ) -> Result<Ast, ParseError> {
        let func_span = self.advance().span.clone();
        if !matches!(self.peek(), Token::LParen | Token::Unit) {
            return Err(ParseError::syntax(
                "FuncLiteral must appear in infix position or be followed by a call",
                func_span,
            ));
        }

        let args = if matches!(self.peek(), Token::Unit) {
            self.advance();
            Vec::new()
        } else {
            self.advance();
            let args = self.parse_call_args()?;
            self.skip_newlines();
            self.expect(&Token::RParen)?;
            args
        };
        let end = self.tokens[self.pos - 1].span.end;
        match Self::parse_func_literal_body(&body, func_span.clone())? {
            FuncLiteralBodyKind::Operator(op_body) => {
                if Self::is_pair_constructor_func_literal(&op_body) {
                    let [RecordLitArg::Positional(left), RecordLitArg::Positional(right)] =
                        args.as_slice()
                    else {
                        if allow_partial_pair_constructor_call
                            && matches!(args.as_slice(), [RecordLitArg::Positional(_)])
                        {
                            return Ok(Ast::App(
                                Span {
                                    start: span.start,
                                    end,
                                },
                                Box::new(Ast::FuncLiteralRef(
                                    func_span.clone(),
                                    FuncLiteralRef {
                                        span: func_span,
                                        body: op_body,
                                    },
                                )),
                                args,
                            ));
                        }

                        return Err(ParseError::syntax(
                            "quoted pair constructor call `(,)` expects exactly 2 positional arguments",
                            Span {
                                start: span.start,
                                end,
                            },
                        ));
                    };
                    return Ok(Self::lower_pair_constructor(left.clone(), right.clone()));
                }
                let [RecordLitArg::Positional(left), RecordLitArg::Positional(right)] =
                    args.as_slice()
                else {
                    return Err(ParseError::syntax(
                        format!(
                            "quoted operator call `{}` expects exactly 2 positional arguments",
                            op_body
                        ),
                        Span {
                            start: span.start,
                            end,
                        },
                    ));
                };
                let Some(op) = Self::expr_binop_from_func_literal(&op_body) else {
                    return Err(ParseError::syntax(
                        format!("quoted operator call `{}` is not supported", op_body),
                        func_span,
                    ));
                };
                Ok(Self::lower_binop(left.clone(), op, right.clone()))
            }
            FuncLiteralBodyKind::Name(name) => Ok(Ast::App(
                Span {
                    start: span.start,
                    end,
                },
                Box::new(Ast::Var(func_span, name)),
                args,
            )),
            FuncLiteralBodyKind::Path(path) => Ok(Ast::App(
                Span {
                    start: span.start,
                    end,
                },
                Box::new(Ast::Path(path.span.clone(), path)),
                args,
            )),
        }
    }

    pub(super) fn is_duration_suffix_here(&self) -> bool {
        matches!(self.peek(), Token::Ident(name) if name == "ms")
    }

    fn is_timeout_modifier_start(&self) -> bool {
        matches!(self.peek(), Token::Annotator(name) if name == "timeout")
    }

    fn hidden_call(&self, name: &str, args: Vec<RecordLitArg>, span: Span) -> Ast {
        Ast::App(
            span.clone(),
            Box::new(Ast::InternalVar(
                Span {
                    start: span.start,
                    end: span.start,
                },
                name.to_string(),
            )),
            args,
        )
    }

    fn duration_literal(&self, value: Ast, span: Span) -> Ast {
        Ast::InternalStructLit(
            span,
            "Duration".into(),
            vec![crate::ast::StructLitField::Explicit("millis".into(), value)],
        )
    }

    fn std_hidden_ref(&self, span: Span, name: Symbol) -> Ast {
        if name.starts_with("__")
            && self
                .context
                .parse_rules
                .allowed_top_level_decl_kinds
                .allows(super::context::TopLevelDeclKind::BuiltinDecl)
        {
            Ast::InternalVar(span, name)
        } else {
            Ast::Var(span, name)
        }
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
        Ok(self.duration_literal(
            int_expr,
            Span {
                start: sp.start,
                end: suffix_span.end,
            },
        ))
    }

    fn parse_timeout_modifier(&mut self, expr: Ast) -> Result<Ast, ParseError> {
        let start = expr.span().start;
        let modifier_span = self.peek_span();
        let Token::Annotator(modifier) = self.peek().clone() else {
            return Err(ParseError::syntax(
                "Expected @timeout(...)",
                self.peek_span(),
            ));
        };
        self.advance();
        if modifier != "timeout" {
            return Err(ParseError::syntax(
                format!("Unsupported call modifier: @{modifier}"),
                modifier_span,
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
                    "@timeout(...) can only be applied to runtime-managed calls",
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
            Ast::Path(_, path) if path.segments.as_slice() == ["Task", "await"] => {
                "__task_await_timeout"
            }
            Ast::Path(_, path) if path.segments.as_slice() == ["Workers", "submit"] => {
                "__workers_submit_timeout"
            }
            Ast::Path(_, path) if path.segments.as_slice() == ["Workers", "broadcast"] => {
                "__workers_broadcast_timeout"
            }
            _ => {
                return Err(ParseError::syntax(
                    "@timeout(...) is only supported on Task::call/await and Workers::submit/broadcast",
                    Span {
                        start,
                        end: end_span.end,
                    },
                ));
            }
        };

        let expected_arity = match hidden_name {
            "__task_call_timeout" | "__task_await_timeout" => 1,
            "__workers_submit_timeout" | "__workers_broadcast_timeout" => 2,
            _ => 1,
        };

        if args.len() != expected_arity
            || args
                .iter()
                .any(|arg| matches!(arg, RecordLitArg::Named(_, _)))
        {
            return Err(ParseError::syntax(
                "@timeout(...) expects positional arguments for the runtime-managed call",
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
        if name == "hash" && matches!(self.peek(), Token::Bang) {
            return self.parse_hash_map_literal(name_span);
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

        if let Some(mut path_expr) = path_ast {
            let path_name = path_segments.join("::");
            let path_last_is_uppercase = path_segments
                .last()
                .and_then(|segment| segment.chars().next())
                .map(|ch| ch.is_uppercase())
                .unwrap_or(false);
            if self.explicit_type_args_start() {
                if path_last_is_uppercase {
                    return Err(ParseError::syntax(
                        "Explicit type arguments apply to callables, not constructors",
                        self.peek_span(),
                    ));
                }
                path_expr = self.parse_explicit_type_apply(path_expr)?;
            }
            if matches!(self.peek(), Token::LParen) {
                self.advance();
                let args = if path_name == "Kernel::is_match" {
                    self.parse_is_match_args()?
                } else {
                    self.parse_call_args()?
                };
                self.skip_newlines();
                let end_span = self.expect(&Token::RParen)?;
                if path_name == "Kernel::is_match" {
                    return self.finish_is_match_special_form(
                        name_span.start,
                        end_span.end,
                        args,
                        "Kernel::is_match",
                    );
                }
                if path_name == "Facet::bulk_update" {
                    if args.len() != 1
                        || args
                            .iter()
                            .any(|arg| matches!(arg, RecordLitArg::Named(_, _)))
                    {
                        return Err(ParseError::syntax(
                            "Facet::bulk_update expects exactly 1 positional argument before its update block",
                            Span {
                                start: name_span.start,
                                end: end_span.end,
                            },
                        ));
                    }
                    if !matches!(self.peek(), Token::LBrace) {
                        return Err(ParseError::syntax(
                            "Facet::bulk_update requires a special update block",
                            self.peek_span(),
                        ));
                    }
                    let source = match <[RecordLitArg; 1]>::try_from(args) {
                        Ok([RecordLitArg::Positional(expr)]) => expr,
                        _ => {
                            return Err(ParseError::syntax(
                                "Facet::bulk_update expects exactly 1 positional argument before its update block",
                                Span {
                                    start: name_span.start,
                                    end: end_span.end,
                                },
                            ));
                        }
                    };
                    return self.parse_bulk_update_expr(name_span.start, source);
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

        if self.explicit_type_args_start() {
            if is_uppercase {
                return Err(ParseError::syntax(
                    "Explicit type arguments apply to callables, not constructors",
                    self.peek_span(),
                ));
            }
            let func = self
                .parse_explicit_type_apply(self.std_hidden_ref(name_span.clone(), name.clone()))?;
            if matches!(self.peek(), Token::LParen) {
                self.advance();
                let args = self.parse_call_args()?;
                self.skip_newlines();
                let end_span = self.expect(&Token::RParen)?;
                let (args, call_end) = self.attach_trailing_block_arg(&func, args, end_span.end)?;
                return Ok(Ast::App(
                    Span {
                        start: name_span.start,
                        end: call_end,
                    },
                    Box::new(func),
                    args,
                ));
            }
            if matches!(self.peek(), Token::Unit) {
                let end_span = self.advance().span.clone();
                let (args, call_end) =
                    self.attach_trailing_block_arg(&func, Vec::new(), end_span.end)?;
                return Ok(Ast::App(
                    Span {
                        start: name_span.start,
                        end: call_end,
                    },
                    Box::new(func),
                    args,
                ));
            }
            return Ok(func);
        }

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

        // Struct literal: Name { field: val, shorthand, ... }
        if is_uppercase && matches!(self.peek(), Token::LBrace) {
            use crate::ast::StructLitField;
            self.advance();
            self.skip_newlines();
            let mut fields = Vec::new();
            if !matches!(self.peek(), Token::RBrace) {
                loop {
                    self.skip_newlines();
                    let (field_name, _) = self.expect_ident()?;
                    if matches!(self.peek(), Token::Colon) {
                        self.advance();
                        let val = self.parse_non_assignment_expr()?;
                        fields.push(StructLitField::Explicit(field_name, val));
                    } else {
                        fields.push(StructLitField::Shorthand(field_name));
                    }
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
        if matches!(self.peek(), Token::LParen) && !self.pair_constructor_at() {
            self.advance();
            let args = if name == "is_match" {
                self.parse_is_match_args()?
            } else {
                self.parse_call_args()?
            };
            self.skip_newlines();
            let end_span = self.expect(&Token::RParen)?;
            if name == "is_match" {
                return self.finish_is_match_special_form(
                    name_span.start,
                    end_span.end,
                    args,
                    "is_match",
                );
            }
            let func = self.std_hidden_ref(name_span.clone(), name.clone());

            if is_uppercase {
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
            let func = self.std_hidden_ref(name_span.clone(), name.clone());
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
            return Self::assignment_ast(assign_tok, span, pat, rhs);
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
            return Self::assignment_ast(assign_tok, span, pat, rhs);
        }

        // Just a variable
        Ok(self.std_hidden_ref(name_span, name))
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

    fn parse_is_match_args(&mut self) -> Result<Vec<RecordLitArg>, ParseError> {
        self.skip_newlines();
        if matches!(self.peek(), Token::RParen) {
            return Ok(Vec::new());
        }
        let term = self.parse_record_lit_arg()?;
        self.skip_newlines();
        if !matches!(self.peek(), Token::Comma) {
            return Ok(vec![term]);
        }
        self.advance();
        self.skip_newlines();
        let pattern = self.parse_match_pattern()?;
        self.skip_newlines();
        if matches!(self.peek(), Token::Comma) {
            return Err(ParseError::syntax(
                "is_match expects exactly 2 positional arguments",
                self.peek_span(),
            ));
        }
        let pattern_expr = Ast::Match(
            super::pattern_span(&pattern).clone(),
            Box::new(Ast::Lit(super::pattern_span(&pattern).clone(), Lit::Unit)),
            vec![AstMatchArm {
                pattern,
                guard: None,
                body: Ast::Lit(self.peek_span(), Lit::Unit),
            }],
        );
        Ok(vec![term, RecordLitArg::Positional(pattern_expr)])
    }

    fn finish_is_match_special_form(
        &self,
        start: usize,
        end: usize,
        args: Vec<RecordLitArg>,
        name: &str,
    ) -> Result<Ast, ParseError> {
        if args.len() != 2
            || args
                .iter()
                .any(|arg| matches!(arg, RecordLitArg::Named(_, _)))
        {
            return Err(ParseError::syntax(
                format!("{name} expects exactly 2 positional arguments"),
                Span { start, end },
            ));
        }
        let [term_arg, pattern_arg] = <[RecordLitArg; 2]>::try_from(args).map_err(|args| {
            ParseError::syntax(
                format!(
                    "{name} expects exactly 2 positional arguments, got {}",
                    args.len()
                ),
                Span { start, end },
            )
        })?;
        let term = match term_arg {
            RecordLitArg::Positional(expr) => expr,
            RecordLitArg::Named(_, _) => {
                return Err(ParseError::syntax(
                    format!("{name} expects positional arguments"),
                    Span { start, end },
                ));
            }
        };
        let pattern_expr = match pattern_arg {
            RecordLitArg::Positional(expr) => expr,
            RecordLitArg::Named(_, _) => {
                return Err(ParseError::syntax(
                    format!("{name} expects positional arguments"),
                    Span { start, end },
                ));
            }
        };
        let Ast::Match(_, _, mut arms) = pattern_expr else {
            return Ok(Ast::App(
                Span { start, end },
                Box::new(Ast::Var(Span { start, end: start }, name.into())),
                vec![
                    RecordLitArg::Positional(term),
                    RecordLitArg::Positional(pattern_expr),
                ],
            ));
        };
        let pattern = arms.remove(0).pattern;
        if super::pattern::pattern_contains_binding_var(&pattern) {
            return Err(ParseError::syntax(
                "`is_match` pattern does not allow binding variables. Use `_` to ignore a value, or use `if_let` / `match` when you need bindings.",
                super::pattern_span(&pattern).clone(),
            ));
        }
        let span = Span { start, end };
        Ok(Ast::Match(
            span.clone(),
            Box::new(term),
            vec![
                AstMatchArm {
                    pattern,
                    guard: None,
                    body: Ast::Lit(span.clone(), Lit::Bool(true)),
                },
                AstMatchArm {
                    pattern: AstPattern::Wildcard(span.clone()),
                    guard: None,
                    body: Ast::Lit(span, Lit::Bool(false)),
                },
            ],
        ))
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
            let body = block_stmts_to_expr(body_stmts);
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
            if matches!(parser.peek(), Token::DotDot) {
                parser.advance();
                parser.skip_newlines();
                let stop = parser.parse_expr()?;
                parser.skip_newlines();
                let end = parser.expect(&Token::RBrack)?;
                return Ok(Ast::RangeLiteral(
                    Span {
                        start: sp.start,
                        end: end.end,
                    },
                    Box::new(first),
                    Box::new(stop),
                ));
            }
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
                if matches!(parser.peek(), Token::RBrack) {
                    let end = parser.expect(&Token::RBrack)?;
                    return Ok(Ast::ListLiteral(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        elems,
                    ));
                }
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

    fn parse_hash_map_literal(&mut self, name_span: Span) -> Result<Ast, ParseError> {
        self.advance();
        let bracket_start = self.expect(&Token::LBrack)?;
        self.with_parse_nesting(bracket_start.clone(), |parser| {
            parser.skip_newlines();
            if matches!(parser.peek(), Token::RBrack) {
                let end = parser.expect(&Token::RBrack)?;
                return Ok(Ast::HashMapLiteral(
                    Span {
                        start: name_span.start,
                        end: end.end,
                    },
                    Vec::new(),
                ));
            }

            let mut entries = Vec::new();
            loop {
                parser.skip_newlines();
                let key = parser.parse_non_assignment_expr()?;
                parser.skip_newlines();
                if !matches!(parser.peek(), Token::FatArrow) {
                    return Err(ParseError::syntax(
                        "expected `=>` in hash! literal",
                        parser.peek_span(),
                    ));
                }
                parser.advance();
                parser.skip_newlines();
                let value = parser.parse_non_assignment_expr()?;
                entries.push(HashMapLiteralEntry { key, value });
                parser.skip_newlines();

                if matches!(parser.peek(), Token::Comma) {
                    parser.advance();
                    parser.skip_newlines();
                    if matches!(parser.peek(), Token::RBrack) {
                        break;
                    }
                    continue;
                }
                break;
            }

            parser.skip_newlines();
            let end = parser.expect(&Token::RBrack)?;
            Ok(Ast::HashMapLiteral(
                Span {
                    start: name_span.start,
                    end: end.end,
                },
                entries,
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
        let body = block_stmts_to_expr(body_stmts);
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
        if self.pair_constructor_at() {
            return Err(ParseError::syntax(
                "bare `(,)` is only valid in infix position",
                Span {
                    start: sp.start,
                    end: self.pair_constructor_span().end,
                },
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
        if let Some(operator) = func_literal_operator_token(self.peek()) {
            let operator_span = self.peek_span();
            return Err(ParseError::syntax(
                format!("Unquoted operator capture: {operator}"),
                Span {
                    start: sp.start,
                    end: operator_span.end,
                },
            ));
        }
        let (mut target, mut end) = match self.peek().clone() {
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

        while matches!(self.peek(), Token::Dot) {
            self.advance();
            let (segment, segment_span) = self.parse_facet_path_segment_after_dot()?;
            end = segment_span.end;
            let span = Span {
                start: target.span().start,
                end,
            };
            target = match segment {
                FacetPathSegment::Field {
                    name,
                    optional: false,
                } => Ast::FieldAccess(span, Box::new(target), name),
                other => Ast::FacetSegmentAccess(span, Box::new(target), other),
            };
        }

        if self.explicit_type_args_start() {
            target = self.parse_explicit_type_apply(target)?;
            end = target.span().end;
        }

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

    pub(super) fn parse_facet_capture_expr(&mut self, sp: Span) -> Result<Ast, ParseError> {
        self.expect(&Token::Tilde)?;
        let inner = self.parse_postfix()?;
        Ok(Ast::FacetCapture(
            Span {
                start: sp.start,
                end: inner.span().end,
            },
            Box::new(inner),
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

    fn parse_bulk_update_expr(&mut self, start: usize, source: Ast) -> Result<Ast, ParseError> {
        self.skip_newlines();
        let lbrace = self.expect(&Token::LBrace)?;
        self.skip_newlines();

        if matches!(self.peek(), Token::RBrace) {
            return Err(ParseError::syntax(
                "Facet::bulk_update must contain at least one entry",
                lbrace,
            ));
        }

        let mut entries = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            self.skip_newlines();
            if matches!(self.peek(), Token::RBrace) {
                break;
            }
            entries.push(self.parse_bulk_update_entry()?);
            self.skip_newlines();
        }

        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::BulkUpdate(
            Span {
                start,
                end: end.end,
            },
            Box::new(source),
            entries,
        ))
    }

    fn parse_bulk_update_entry(&mut self) -> Result<BulkUpdateEntry, ParseError> {
        let start = self.peek_span().start;
        let path = self.parse_bulk_update_path()?;
        self.skip_newlines();

        if matches!(self.peek(), Token::LBrace) {
            let lbrace = self.advance().span.clone();
            self.skip_newlines();
            if matches!(self.peek(), Token::RBrace) {
                return Err(ParseError::syntax(
                    "Bulk update nested path must contain at least one entry",
                    lbrace,
                ));
            }

            let mut entries = Vec::new();
            while !matches!(self.peek(), Token::RBrace) {
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete("}", self.peek_span()));
                }
                self.skip_newlines();
                if matches!(self.peek(), Token::RBrace) {
                    break;
                }
                entries.push(self.parse_bulk_update_entry()?);
                self.skip_newlines();
            }
            let end = self.expect(&Token::RBrace)?;
            return Ok(BulkUpdateEntry {
                span: Span {
                    start,
                    end: end.end,
                },
                path,
                kind: BulkUpdateEntryKind::Nested(entries),
            });
        }

        self.expect(&Token::LeftArrow)?;
        let (kind, end) = self.parse_bulk_update_leaf()?;
        Ok(BulkUpdateEntry {
            span: Span { start, end },
            path,
            kind,
        })
    }

    fn parse_bulk_update_path(&mut self) -> Result<BulkUpdatePath, ParseError> {
        self.parse_bulk_update_path_chain()
    }

    fn parse_bulk_update_path_chain(&mut self) -> Result<BulkUpdatePath, ParseError> {
        let mut path = self.parse_bulk_update_path_primary()?;
        while matches!(self.peek(), Token::Slash) {
            self.advance();
            self.skip_newlines();
            let right = self.parse_bulk_update_path_primary()?;
            let span = Span {
                start: path.span().start,
                end: right.span().end,
            };
            path = BulkUpdatePath::Chain(span, Box::new(path), Box::new(right));
            self.skip_newlines();
        }
        Ok(path)
    }

    fn parse_bulk_update_path_primary(&mut self) -> Result<BulkUpdatePath, ParseError> {
        let start_span = self.peek_span();
        match self.peek().clone() {
            Token::Caret => {
                self.advance();
                let (name, name_span) = self.expect_ident()?;
                Ok(BulkUpdatePath::Pin(
                    Span {
                        start: start_span.start,
                        end: name_span.end,
                    },
                    name,
                ))
            }
            Token::Ident(name) if name == "Facet" => {
                let save = self.pos;
                self.advance();
                if self.has_path_separator() {
                    self.consume_path_separator()?;
                    if let Token::Ident(method) = self.peek().clone() {
                        if matches!(method.as_str(), "strip_left" | "strip_right") {
                            return self.parse_bulk_update_path_operation(start_span, method);
                        }
                    }
                }
                self.pos = save;
                self.parse_bulk_update_relative_path()
            }
            Token::Ident(_) => self.parse_bulk_update_relative_path(),
            _ => Err(ParseError::syntax(
                "Bulk update path must start with a path segment, pinned FacetPath (^name), or whitelisted Facet path operation",
                start_span,
            )),
        }
    }

    fn parse_bulk_update_relative_path(&mut self) -> Result<BulkUpdatePath, ParseError> {
        let start = self.peek_span().start;
        let (first, first_span) = self.expect_ident()?;
        let mut path = BulkUpdatePath::Segments(
            Span {
                start,
                end: first_span.end,
            },
            vec![FacetPathSegment::field(first)],
        );
        while matches!(self.peek(), Token::Dot) {
            self.advance();
            if matches!(self.peek(), Token::Caret) {
                let pin_start = self.peek_span();
                self.advance();
                let (name, name_span) = self.expect_ident()?;
                let pin = BulkUpdatePath::Pin(
                    Span {
                        start: pin_start.start,
                        end: name_span.end,
                    },
                    name,
                );
                let span = Span {
                    start: path.span().start,
                    end: pin.span().end,
                };
                path = BulkUpdatePath::Chain(span, Box::new(path), Box::new(pin));
                continue;
            }
            let (segment, segment_span) = self.parse_facet_path_segment_after_dot()?;
            match &mut path {
                BulkUpdatePath::Segments(span, segments) => {
                    span.end = segment_span.end;
                    segments.push(segment);
                }
                _ => {
                    let right = BulkUpdatePath::Segments(segment_span.clone(), vec![segment]);
                    let span = Span {
                        start: path.span().start,
                        end: segment_span.end,
                    };
                    path = BulkUpdatePath::Chain(span, Box::new(path), Box::new(right));
                }
            }
        }
        Ok(path)
    }

    fn parse_bulk_update_path_operation(
        &mut self,
        start_span: Span,
        method: Symbol,
    ) -> Result<BulkUpdatePath, ParseError> {
        let method_span = self.advance().span.clone();
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        self.skip_newlines();
        let inner = self.parse_bulk_update_path_chain()?;
        self.skip_newlines();
        self.expect(&Token::Comma)?;
        self.skip_newlines();
        let count_span = self.peek_span();
        let Token::Int(count) = self.peek().clone() else {
            return Err(ParseError::syntax(
                "Facet path operation count must be an integer literal",
                count_span,
            ));
        };
        self.advance();
        let Some(count) = count.to_usize() else {
            return Err(ParseError::syntax(
                "Facet path operation count must fit in usize",
                count_span,
            ));
        };
        self.skip_newlines();
        let end = self.expect(&Token::RParen)?;
        let span = Span {
            start: start_span.start,
            end: end.end,
        };
        match method.as_str() {
            "strip_left" => Ok(BulkUpdatePath::StripLeft(span, Box::new(inner), count)),
            "strip_right" => Ok(BulkUpdatePath::StripRight(span, Box::new(inner), count)),
            _ => Err(ParseError::syntax(
                "Unsupported bulk_update path operation",
                method_span,
            )),
        }
    }

    fn parse_bulk_update_leaf(&mut self) -> Result<(BulkUpdateEntryKind, usize), ParseError> {
        self.skip_newlines();
        let (name, name_span) = self.expect_ident()?;
        let entry_kind = match name.as_str() {
            "set" => BulkUpdateEntryKind::Set,
            "over" => BulkUpdateEntryKind::Over,
            "over_result" => BulkUpdateEntryKind::OverResult,
            "case_set" => BulkUpdateEntryKind::CaseSet,
            "case_over" => BulkUpdateEntryKind::CaseOver,
            _ => {
                return Err(ParseError::syntax(
                    "Bulk update entries must use set(value), over(update_fun), over_result(update_fun), case_set(payload), or case_over(update_fun)",
                    name_span,
                ));
            }
        };
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        let args = self.with_trailing_call_block_disabled(|parser| parser.parse_call_args())?;
        let call_end = self.expect(&Token::RParen)?;

        if args.len() != 1
            || args
                .iter()
                .any(|arg| matches!(arg, RecordLitArg::Named(_, _)))
        {
            return Err(ParseError::syntax(
                "Bulk update entries must use set(value), over(update_fun), over_result(update_fun), case_set(payload), or case_over(update_fun)",
                Span {
                    start: name_span.start,
                    end: call_end.end,
                },
            ));
        }

        let inner = match <[RecordLitArg; 1]>::try_from(args) {
            Ok([RecordLitArg::Positional(expr)]) => expr,
            _ => {
                return Err(ParseError::syntax(
                    "Bulk update entries must use set(value), over(update_fun), over_result(update_fun), case_set(payload), or case_over(update_fun)",
                    Span {
                        start: name_span.start,
                        end: call_end.end,
                    },
                ));
            }
        };
        if bulk_update_proc_contains_operation_call(&inner) {
            return Err(ParseError::syntax(
                "bulk_update operation calls cannot be nested. Use a single whitelisted operation at the leaf.",
                inner.span().clone(),
            ));
        }

        let end = inner.span().end;
        Ok((entry_kind(inner), end))
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
            return Err(ParseError::syntax(
                "Cond expression must contain at least one clause",
                lbrace,
            ));
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

fn bulk_update_proc_contains_operation_call(expr: &Ast) -> bool {
    match expr {
        Ast::App(_, func, args) => {
            let is_bulk_proc = match func.as_ref() {
                Ast::Var(_, name) => {
                    matches!(
                        name.as_str(),
                        "set" | "over" | "over_result" | "case_set" | "case_over"
                    )
                }
                Ast::Path(_, path) => {
                    matches!(
                        path.segments.as_slice(),
                        [module, name]
                            if module == "Facet"
                                && matches!(
                                    name.as_str(),
                                    "set" | "over" | "over_result" | "case_set" | "case_over"
                                )
                    )
                }
                _ => false,
            };
            is_bulk_proc
                || bulk_update_proc_contains_operation_call(func)
                || args.iter().any(|arg| match arg {
                    RecordLitArg::Positional(inner) | RecordLitArg::Named(_, inner) => {
                        bulk_update_proc_contains_operation_call(inner)
                    }
                })
        }
        Ast::TypeApply(_, target, _) => bulk_update_proc_contains_operation_call(target),
        Ast::Block(_, stmts) | Ast::ListLiteral(_, stmts) | Ast::TupleLiteral(_, stmts) => {
            stmts.iter().any(bulk_update_proc_contains_operation_call)
        }
        Ast::HashMapLiteral(_, entries) => entries.iter().any(|entry| {
            bulk_update_proc_contains_operation_call(&entry.key)
                || bulk_update_proc_contains_operation_call(&entry.value)
        }),
        Ast::Bind(_, _, rhs)
        | Ast::SafeBind(_, _, rhs)
        | Ast::Grouped(_, rhs)
        | Ast::FacetCapture(_, rhs)
        | Ast::Semi(_, rhs) => bulk_update_proc_contains_operation_call(rhs),
        Ast::BinOp(_, _, lhs, rhs)
        | Ast::Pipe(_, lhs, rhs)
        | Ast::ContextMap(_, lhs, rhs)
        | Ast::ContextApply(_, lhs, rhs)
        | Ast::ContextBind(_, lhs, rhs)
        | Ast::Compose(_, lhs, rhs)
        | Ast::LiftedCompose(_, lhs, rhs)
        | Ast::KleisliCompose(_, lhs, rhs)
        | Ast::ListCons(_, lhs, rhs)
        | Ast::RangeLiteral(_, lhs, rhs) => {
            bulk_update_proc_contains_operation_call(lhs)
                || bulk_update_proc_contains_operation_call(rhs)
        }
        Ast::FieldAccess(_, target, _) => bulk_update_proc_contains_operation_call(target),
        Ast::Match(_, scrutinee, arms) => {
            bulk_update_proc_contains_operation_call(scrutinee)
                || arms.iter().any(|arm| {
                    arm.guard
                        .as_ref()
                        .is_some_and(bulk_update_proc_contains_operation_call)
                        || bulk_update_proc_contains_operation_call(&arm.body)
                })
        }
        Ast::BulkUpdate(_, source, entries) => {
            bulk_update_proc_contains_operation_call(source)
                || entries
                    .iter()
                    .any(|entry| bulk_update_entry_contains_operation_call(entry))
        }
        Ast::Capture(_, target, args) => {
            bulk_update_proc_contains_operation_call(target)
                || args.iter().any(bulk_update_proc_contains_operation_call)
        }
        Ast::Closure(_, _, body) => bulk_update_proc_contains_operation_call(body),
        Ast::Dbg(_, args) => args
            .iter()
            .any(|arg| bulk_update_proc_contains_operation_call(&arg.expr)),
        Ast::StructLit(_, _, fields) => fields.iter().any(|field| match field {
            StructLitField::Explicit(_, expr) => bulk_update_proc_contains_operation_call(expr),
            StructLitField::Shorthand(_) => false,
        }),
        Ast::InternalStructLit(_, _, fields) => fields.iter().any(|field| match field {
            StructLitField::Explicit(_, expr) => bulk_update_proc_contains_operation_call(expr),
            StructLitField::Shorthand(_) => false,
        }),
        Ast::ConstructorCall(_, _, args) => args.iter().any(|arg| match arg {
            RecordLitArg::Positional(inner) | RecordLitArg::Named(_, inner) => {
                bulk_update_proc_contains_operation_call(inner)
            }
        }),
        Ast::FacetSegmentAccess(_, target, segment) => {
            bulk_update_proc_contains_operation_call(target)
                || match segment {
                    FacetPathSegment::Bracket(bracket) => {
                        bulk_update_proc_contains_operation_call(&bracket.expr)
                    }
                    FacetPathSegment::Field { .. } => false,
                }
        }
        Ast::InterpolatedStr(_, parts) => parts.iter().any(|part| match part {
            InterpolatedPart::Text(_) => false,
            InterpolatedPart::Expr(expr) => bulk_update_proc_contains_operation_call(expr),
        }),
        Ast::Lit(_, _)
        | Ast::Var(_, _)
        | Ast::InternalVar(_, _)
        | Ast::Path(_, _)
        | Ast::FuncLiteralRef(_, _)
        | Ast::ListNil(_)
        | Ast::CapturePlaceholder(_, _)
        | Ast::StructDef(..)
        | Ast::RecordDef(..)
        | Ast::EnumDef(..)
        | Ast::ConstDef(..)
        | Ast::SupervisorInit(..)
        | Ast::BuiltinDecl(..)
        | Ast::IntrinsicDecl(..)
        | Ast::BuiltinExtractorDecl(..)
        | Ast::BuiltinTypeDecl(..)
        | Ast::TypeAlias(..)
        | Ast::ResultCtorDecl(..)
        | Ast::Def(..)
        | Ast::DeferrorDef(..)
        | Ast::ExtractorDef(..)
        | Ast::TraitDef(..)
        | Ast::ImplDef(..)
        | Ast::TraitImplDef(..)
        | Ast::Import(..)
        | Ast::Include(..)
        | Ast::Defmod(..)
        | Ast::Defagent(..)
        | Ast::Defgenserver(..)
        | Ast::Defsupervisor(..)
        | Ast::DefdynamicSupervisor(..)
        | Ast::Namespace(..) => false,
    }
}

fn block_stmts_to_expr(mut body_stmts: Vec<Ast>) -> Ast {
    if body_stmts.len() == 1 {
        return body_stmts.remove(0);
    }
    Ast::Block(
        Span {
            start: body_stmts[0].span().start,
            end: body_stmts[body_stmts.len() - 1].span().end,
        },
        body_stmts,
    )
}

fn bulk_update_entry_contains_operation_call(entry: &BulkUpdateEntry) -> bool {
    match &entry.kind {
        BulkUpdateEntryKind::Set(expr)
        | BulkUpdateEntryKind::Over(expr)
        | BulkUpdateEntryKind::OverResult(expr)
        | BulkUpdateEntryKind::CaseSet(expr)
        | BulkUpdateEntryKind::CaseOver(expr) => bulk_update_proc_contains_operation_call(expr),
        BulkUpdateEntryKind::Nested(entries) => entries
            .iter()
            .any(bulk_update_entry_contains_operation_call),
    }
}
