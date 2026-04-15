use crate::ast::*;
use crate::error::ParseError;
use crate::token::Token;

use super::Parser;

impl Parser {
    fn flow_op_kind(tok: &Token) -> Option<u8> {
        match tok {
            Token::PipeApply => Some(0),
            Token::PipeMap => Some(1),
            Token::PipeBind => Some(2),
            Token::Compose => Some(3),
            Token::PipeCompose => Some(4),
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
        let mut left = self.parse_logical_expr()?;
        loop {
            self.skip_newlines_before_flow_op();

            let next = match Self::flow_op_kind(self.peek()) {
                Some(kind) => kind,
                None => break,
            };
            self.advance();
            self.skip_newlines();
            let right = self.parse_logical_expr()?;
            let span = Span {
                start: left.span().start,
                end: right.span().end,
            };
            left = match next {
                0 => Ast::Pipe(span, Box::new(left), Box::new(right)),
                1 => Ast::ContextMap(span, Box::new(left), Box::new(right)),
                2 => Ast::ContextBind(span, Box::new(left), Box::new(right)),
                3 => Ast::Compose(span, Box::new(left), Box::new(right)),
                4 => Ast::KleisliCompose(span, Box::new(left), Box::new(right)),
                _ => unreachable!("validated flow token"),
            };
        }
        Ok(left)
    }

    pub(super) fn stmt_has_top_level_assignment_from(&self, start: usize) -> bool {
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
                Token::Bind | Token::SafeBind
                    if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 =>
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

    pub(super) fn lower_binop(left: Ast, op: BinOp, right: Ast) -> Ast {
        let span = Span {
            start: left.span().start,
            end: right.span().end,
        };
        Ast::BinOp(span, op, Box::new(left), Box::new(right))
    }

    pub(super) fn lower_func_literal_call(
        left: Ast,
        func_span: Span,
        name: Symbol,
        right: Ast,
    ) -> Ast {
        let span = Span {
            start: left.span().start,
            end: right.span().end,
        };
        Ast::App(
            span,
            Box::new(Ast::Var(func_span, name)),
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

            if let Some(op) = Self::expr_binop_from_func_literal(&body) {
                self.advance();
                let right = self.parse_postfix()?;
                left = Self::lower_binop(left, op, right);
                continue;
            }

            if Self::logical_binop_from_func_literal(&body).is_some() {
                break;
            }

            let func_span = self.advance().span.clone();
            let right = self.parse_postfix()?;
            left = Self::lower_func_literal_call(left, func_span, body, right);
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

            let Some(op) = Self::logical_binop_from_func_literal(&body) else {
                break;
            };
            self.advance();
            let right = self.parse_expr_class_expr()?;
            left = Self::lower_binop(left, op, right);
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

        Ok(expr)
    }

    // ── Primary ──

    pub(super) fn parse_primary(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();

        match self.peek().clone() {
            // Literals
            Token::Int(n) => {
                self.advance();
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
            Token::DocString(_) => Err(ParseError::syntax(
                "Doc strings are only allowed after @@doc",
                sp,
            )),
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
            Token::LParen => {
                self.advance();
                self.skip_newlines();
                let first = self.parse_expr()?;
                self.skip_newlines();
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                    if matches!(self.peek(), Token::RParen) {
                        return Err(ParseError::syntax(
                            "1-tuple literals are not supported",
                            Span {
                                start: sp.start,
                                end: self.peek_span().end,
                            },
                        ));
                    }
                    let mut items = vec![first, self.parse_expr()?];
                    self.skip_newlines();
                    while matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                        items.push(self.parse_expr()?);
                        self.skip_newlines();
                    }
                    let end = self.expect(&Token::RParen)?;
                    Ok(Ast::TupleLiteral(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        items,
                    ))
                } else {
                    self.expect(&Token::RParen)?;
                    Ok(first)
                }
            }

            // Block expression: { stmt; stmt; expr }
            Token::LBrace => self.parse_trailing_block_expr_from_lbrace(sp),

            // Capture / partial application: &foo, &foo(1)
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
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        if matches!(self.peek(), Token::Pipe) {
            return self.parse_closure_literal(sp);
        }

        let stmts = self.parse_block_stmts()?;
        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::Block(
            Span {
                start: sp.start,
                end: end.end,
            },
            stmts,
        ))
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

    pub(super) fn trailing_block_uses_closure_sugar(callee: &Ast) -> bool {
        fn is_test_dsl_name(name: &str) -> bool {
            matches!(name, "test" | "describe" | "it")
        }

        match callee {
            Ast::Var(_, name) => is_test_dsl_name(name),
            Ast::Path(_, path) => path
                .segments
                .last()
                .is_some_and(|name| is_test_dsl_name(name)),
            _ => false,
        }
    }

    pub(super) fn attach_trailing_block_arg(
        &mut self,
        callee: &Ast,
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
        let trailing = match trailing {
            Ast::Block(span, stmts) if Self::trailing_block_uses_closure_sugar(callee) => {
                Ast::Closure(span.clone(), Vec::new(), Box::new(Ast::Block(span, stmts)))
            }
            other => other,
        };
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
        self.expect(&Token::LBrack)?;
        self.skip_newlines();

        if matches!(self.peek(), Token::RBrack) {
            let end = self.expect(&Token::RBrack)?;
            return Ok(Ast::ListNil(Span {
                start: sp.start,
                end: end.end,
            }));
        }

        let first = self.parse_expr()?;
        self.skip_newlines();
        if matches!(self.peek(), Token::Comma) {
            self.advance();
            self.skip_newlines();
            if matches!(self.peek(), Token::DotDot) {
                self.advance();
                self.skip_newlines();
                let tail = self.parse_expr()?;
                self.skip_newlines();
                let end = self.expect(&Token::RBrack)?;
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
            elems.push(self.parse_expr()?);
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::RBrack) {
                    break;
                }
                elems.push(self.parse_expr()?);
            }
            self.skip_newlines();
            let end = self.expect(&Token::RBrack)?;
            return Ok(Ast::ListLiteral(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                elems,
            ));
        }

        let end = self.expect(&Token::RBrack)?;
        Ok(Ast::ListLiteral(
            Span {
                start: sp.start,
                end: end.end,
            },
            vec![first],
        ))
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
        let (name, name_span) = self.expect_ident()?;
        let mut path_segments = vec![name.clone()];
        let mut path_end = name_span.end;
        while self.has_path_separator() && matches!(self.peek_n(2), Some(Token::Ident(_))) {
            self.consume_path_separator()?;
            let (seg, seg_span) = self.expect_ident()?;
            path_end = seg_span.end;
            path_segments.push(seg);
        }

        let mut parsed_args = Vec::new();
        let mut end = path_end;
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

        let target = if path_segments.len() == 1 {
            Ast::Var(name_span, name)
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
            let pat = self.parse_match_pattern()?;
            self.expect(&Token::FatArrow)?;
            let body = self.parse_expr()?;
            arms.push((pat, body));
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
