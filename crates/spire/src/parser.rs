use crate::ast::*;
use crate::error::ParseError;
use crate::lexer::tokenize;
use crate::token::{Spanned, Token};

/// Parse Surtr source text into an abstract syntax tree.
pub fn parse(source: &str) -> Result<Vec<Ast>, ParseError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

struct Parser {
    tokens: Vec<Spanned<Token>>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Spanned<Token>>) -> Self {
        Self { tokens, pos: 0 }
    }

    // ── Helpers ──

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].token
    }

    fn peek_n(&self, n: usize) -> Option<&Token> {
        self.tokens.get(self.pos + n).map(|sp| &sp.token)
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos].span.clone()
    }

    fn advance(&mut self) -> &Spanned<Token> {
        let t = &self.tokens[self.pos];
        self.pos += 1;
        t
    }

    fn expected_token_name(expected: &Token) -> &'static str {
        match expected {
            Token::RParen => ")",
            Token::RBrace => "}",
            Token::RBrack => "]",
            _ => "token",
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<Span, ParseError> {
        let sp = self.peek_span();
        if self.peek() == expected {
            self.advance();
            Ok(sp)
        } else if matches!(self.peek(), Token::Eof)
            && matches!(expected, Token::RParen | Token::RBrace | Token::RBrack)
        {
            Err(ParseError::incomplete(
                Self::expected_token_name(expected),
                sp,
            ))
        } else {
            Err(ParseError::syntax(
                format!("Expected {:?}, got {:?}", expected, self.peek()),
                sp,
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<(Symbol, Span), ParseError> {
        let sp = self.peek_span();
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                Ok((name, sp))
            }
            Token::Eof => Err(ParseError::incomplete("identifier", sp)),
            _ => Err(ParseError::syntax(
                format!("Expected identifier, got {:?}", self.peek()),
                sp,
            )),
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline) {
            self.advance();
        }
    }

    fn stmt_has_explicit_separator(stmt: &Ast) -> bool {
        matches!(stmt, Ast::Semi(_, _))
    }

    fn ensure_stmt_boundary(&self, stmt: &Ast, allow_rbrace: bool) -> Result<(), ParseError> {
        if Self::stmt_has_explicit_separator(stmt) {
            return Ok(());
        }
        let ok = matches!(self.peek(), Token::Newline | Token::Eof)
            || (allow_rbrace && matches!(self.peek(), Token::RBrace));
        if ok {
            Ok(())
        } else {
            Err(ParseError::syntax(
                "Expected newline or `;` between statements",
                self.peek_span(),
            ))
        }
    }

    #[allow(dead_code)]
    fn at_stmt_end(&self) -> bool {
        matches!(
            self.peek(),
            Token::Newline
                | Token::Semicolon
                | Token::Eof
                | Token::RBrace
                | Token::RParen
                | Token::RBrack
                | Token::Comma
        )
    }

    // ── Program ──

    fn parse_program(&mut self) -> Result<Vec<Ast>, ParseError> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while !matches!(self.peek(), Token::Eof) {
            let stmt = self.parse_stmt()?;
            self.ensure_stmt_boundary(&stmt, false)?;
            stmts.push(stmt);
            while matches!(self.peek(), Token::Newline) {
                self.advance();
            }
        }
        Ok(stmts)
    }

    // ── Statement ──

    fn parse_stmt(&mut self) -> Result<Ast, ParseError> {
        self.skip_newlines();

        // Data definitions
        match self.peek() {
            Token::Annotator(_) => return self.parse_annotated_decl(),
            Token::Def => return self.parse_def(),
            Token::Defstruct => return self.parse_struct_def(),
            Token::Defrecord => return self.parse_record_def(),
            Token::Deferror => return self.parse_deferror_def(),
            _ => {}
        }

        if self.is_pattern_safebind_stmt_start() {
            let save = self.pos;
            if let Ok(stmt) = self.parse_pattern_safebind_stmt() {
                if matches!(self.peek(), Token::Semicolon) {
                    let semi = self.advance().span.clone();
                    let span = Span {
                        start: stmt.span().start,
                        end: semi.end,
                    };
                    return Ok(Ast::Semi(span, Box::new(stmt)));
                }
                return Ok(stmt);
            }
            self.pos = save;
        }

        let expr = self.parse_expr()?;
        if matches!(self.peek(), Token::Semicolon) {
            let semi = self.advance().span.clone();
            let span = Span {
                start: expr.span().start,
                end: semi.end,
            };
            Ok(Ast::Semi(span, Box::new(expr)))
        } else {
            Ok(expr)
        }
    }

    fn parse_block_stmts(&mut self) -> Result<Vec<Ast>, ParseError> {
        let mut stmts = Vec::new();
        self.skip_newlines();

        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            let stmt = self.parse_stmt()?;
            self.ensure_stmt_boundary(&stmt, true)?;
            stmts.push(stmt);
            while matches!(self.peek(), Token::Newline) {
                self.advance();
            }
        }

        Ok(stmts)
    }

    // ── Expression (entry point — handles binding at top level) ──

    fn parse_expr(&mut self) -> Result<Ast, ParseError> {
        self.parse_binop_expr(0)
    }

    fn parse_pattern_safebind_stmt(&mut self) -> Result<Ast, ParseError> {
        let pat = self.parse_bind_pattern_atom()?;
        if !matches!(self.peek(), Token::SafeBind) {
            return Err(ParseError::syntax(
                "Pattern destructuring is currently supported in `=?` pattern position",
                self.peek_span(),
            ));
        }
        self.advance();
        let rhs = self.parse_expr()?;
        self.ensure_non_associative_assignment(&rhs)?;
        let span = Span {
            start: pattern_span(&pat).start,
            end: rhs.span().end,
        };
        Ok(Ast::SafeBind(span, pat, Box::new(rhs)))
    }

    fn is_pattern_safebind_stmt_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::LBrack
                | Token::Ident(_)
                | Token::Int(_)
                | Token::Str(_)
                | Token::True
                | Token::False
                | Token::Minus
        )
    }

    // ── Binary operators with precedence climbing ──

    fn binop_precedence(tok: &Token) -> Option<(u8, BinOp)> {
        match tok {
            // Comparison (lowest)
            Token::EqEq => Some((1, BinOp::Eq)),
            Token::BangEq => Some((1, BinOp::Neq)),
            Token::Lt => Some((2, BinOp::Lt)),
            Token::Gt => Some((2, BinOp::Gt)),
            Token::LtEq => Some((2, BinOp::Lte)),
            Token::GtEq => Some((2, BinOp::Gte)),
            // Concat
            Token::Concat => Some((3, BinOp::Concat)),
            // Additive
            Token::Plus => Some((4, BinOp::Add)),
            Token::Minus => Some((4, BinOp::Sub)),
            // Multiplicative
            Token::Star => Some((5, BinOp::Mul)),
            _ => None,
        }
    }

    fn parse_binop_expr(&mut self, min_prec: u8) -> Result<Ast, ParseError> {
        let mut left = self.parse_postfix()?;

        loop {
            let (prec, op) = match Self::binop_precedence(self.peek()) {
                Some(p) => p,
                None => break,
            };
            if prec < min_prec {
                break;
            }
            let op_span = self.peek_span();
            self.advance(); // consume operator
            let right = self.parse_binop_expr(prec + 1)?;
            let span = Span {
                start: left.span().start,
                end: right.span().end,
            };
            left = Ast::BinOp(span, op, Box::new(left), Box::new(right));
            let _ = op_span;
        }

        Ok(left)
    }

    // ── Postfix (field access: expr.field) ──

    fn parse_postfix(&mut self) -> Result<Ast, ParseError> {
        let mut expr = self.parse_primary()?;

        while matches!(self.peek(), Token::Dot) {
            self.advance(); // consume .
            let (field, fspan) = self.expect_ident()?;
            let span = Span {
                start: expr.span().start,
                end: fspan.end,
            };
            expr = Ast::FieldAccess(span, Box::new(expr), field);
        }

        Ok(expr)
    }

    // ── Primary ──

    fn parse_primary(&mut self) -> Result<Ast, ParseError> {
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
                let inner = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(inner)
            }

            // Block expression: { stmt; stmt; expr }
            Token::LBrace => {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::Pipe) {
                    self.parse_closure_literal(sp)
                } else {
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
            }

            // Capture / partial application: &foo, &foo(1)
            Token::Amp => self.parse_capture_expr(sp),

            // Match expression
            Token::Match => self.parse_match_expr(),

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
    fn parse_ident_continuation(
        &mut self,
        name: Symbol,
        name_span: Span,
    ) -> Result<Ast, ParseError> {
        let is_uppercase = name
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false);

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
            self.skip_newlines();

            if is_uppercase {
                // Constructor call: Name(val, ...) or Name(field: val, ...)
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
                self.skip_newlines();
                let end_span = self.expect(&Token::RParen)?;
                let span = Span {
                    start: name_span.start,
                    end: end_span.end,
                };
                return Ok(Ast::ConstructorCall(span, name, args));
            } else {
                // Normal function call
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
                self.skip_newlines();
                let end_span = self.expect(&Token::RParen)?;
                let span = Span {
                    start: name_span.start,
                    end: end_span.end,
                };
                let func = Ast::Var(name_span, name);
                return Ok(Ast::App(span, Box::new(func), args));
            }
        }

        // Zero-arg call: name() / Name()
        // Lexer tokenizes `()` as Token::Unit.
        if matches!(self.peek(), Token::Unit) {
            let end_span = self.advance().span.clone();
            let span = Span {
                start: name_span.start,
                end: end_span.end,
            };
            if is_uppercase {
                return Ok(Ast::ConstructorCall(span, name, Vec::new()));
            }
            let func = Ast::Var(name_span, name);
            return Ok(Ast::App(span, Box::new(func), Vec::new()));
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
    fn ensure_non_associative_assignment(&self, rhs: &Ast) -> Result<(), ParseError> {
        if matches!(rhs, Ast::Bind(_, _, _) | Ast::SafeBind(_, _, _)) {
            return Err(ParseError::syntax(
                "`=` and `=?` are non-associative; a statement can contain only one assignment operator",
                rhs.span().clone(),
            ));
        }
        Ok(())
    }

    /// Parse a record literal argument: either positional or named.
    fn parse_record_lit_arg(&mut self) -> Result<RecordLitArg, ParseError> {
        // Peek ahead: if IDENT followed by `:`, it's named
        if let Token::Ident(name) = self.peek().clone() {
            let save = self.pos;
            let _name_span = self.peek_span();
            self.advance();
            if matches!(self.peek(), Token::Colon) {
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

    fn parse_non_assignment_expr(&mut self) -> Result<Ast, ParseError> {
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

    fn parse_list_expr(&mut self, sp: Span) -> Result<Ast, ParseError> {
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

    fn parse_list_bind_pattern(&mut self) -> Result<AstPattern, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::LBrack)?;
        self.skip_newlines();
        if matches!(self.peek(), Token::RBrack) {
            let end = self.expect(&Token::RBrack)?;
            return Ok(AstPattern::ListNil(Span {
                start: sp.start,
                end: end.end,
            }));
        }

        let first = self.parse_bind_pattern_atom()?;
        self.skip_newlines();
        let end = if matches!(self.peek(), Token::Comma) {
            self.advance();
            self.skip_newlines();
            if matches!(self.peek(), Token::DotDot) {
                self.advance();
                self.skip_newlines();
                let tail = self.parse_bind_pattern_atom()?;
                self.skip_newlines();
                let end = self.expect(&Token::RBrack)?;
                return Ok(AstPattern::ListCons(
                    Span {
                        start: sp.start,
                        end: end.end,
                    },
                    Box::new(first),
                    Box::new(tail),
                ));
            }

            let mut items = vec![first];
            items.push(self.parse_bind_pattern_atom()?);
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::RBrack) {
                    break;
                }
                items.push(self.parse_bind_pattern_atom()?);
            }
            self.skip_newlines();
            let end = self.expect(&Token::RBrack)?;
            return Ok(fixed_bind_list_pattern(sp.start, end.end, items));
        } else {
            self.expect(&Token::RBrack)?
        };

        Ok(fixed_bind_list_pattern(sp.start, end.end, vec![first]))
    }

    fn parse_bind_pattern_atom(&mut self) -> Result<AstPattern, ParseError> {
        let sp = self.peek_span();
        match self.peek().clone() {
            Token::Ident(name) if name == "_" => {
                self.advance();
                Ok(AstPattern::Wildcard(sp))
            }
            Token::Int(n) => {
                self.advance();
                Ok(AstPattern::IntLit(sp, n))
            }
            Token::Minus => {
                self.advance();
                let neg_span = self.peek_span();
                match self.peek().clone() {
                    Token::Int(n) => {
                        self.advance();
                        Ok(AstPattern::IntLit(
                            Span {
                                start: sp.start,
                                end: neg_span.end,
                            },
                            -n,
                        ))
                    }
                    Token::Eof => Err(ParseError::incomplete("integer literal", neg_span)),
                    _ => Err(ParseError::syntax(
                        "Expected integer literal after '-' in pattern",
                        neg_span,
                    )),
                }
            }
            Token::Str(s) => {
                self.advance();
                Ok(AstPattern::StrLit(sp, s))
            }
            Token::True => {
                self.advance();
                Ok(AstPattern::BoolLit(sp, true))
            }
            Token::False => {
                self.advance();
                Ok(AstPattern::BoolLit(sp, false))
            }
            Token::Ident(name) => {
                if name
                    .chars()
                    .next()
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false)
                    && matches!(self.peek_n(1), Some(Token::LParen))
                {
                    self.advance();
                    self.expect(&Token::LParen)?;
                    self.skip_newlines();
                    if matches!(self.peek(), Token::RParen) {
                        return Err(ParseError::syntax(
                            "Constructor pattern requires exactly one inner pattern",
                            self.peek_span(),
                        ));
                    }
                    let inner = self.parse_bind_pattern_atom()?;
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        return Err(ParseError::syntax(
                            "Constructor pattern supports exactly one inner pattern",
                            self.peek_span(),
                        ));
                    }
                    let end = self.expect(&Token::RParen)?;
                    return Ok(AstPattern::Constructor(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        name,
                        Box::new(inner),
                    ));
                }
                self.advance();
                Ok(AstPattern::Var(sp, name))
            }
            Token::LBrack => self.parse_list_bind_pattern(),
            Token::Eof => Err(ParseError::incomplete("list pattern", sp)),
            _ => Err(ParseError::syntax(
                "Pattern supports identifiers, literals, `_`, list patterns, and nested `Ok(...)` patterns",
                sp,
            )),
        }
    }

    // ── Type annotation parsing ──

    fn parse_type(&mut self) -> Result<AstTy, ParseError> {
        self.skip_newlines();
        let sp = self.peek_span();

        if matches!(self.peek(), Token::LParen) {
            self.advance();
            self.skip_newlines();
            if matches!(self.peek(), Token::Arrow) {
                self.advance();
                let ret = self.parse_type()?;
                self.skip_newlines();
                let end = self.expect(&Token::RParen)?;
                return Ok(AstTy::Func(
                    Span {
                        start: sp.start,
                        end: end.end,
                    },
                    Vec::new(),
                    Box::new(ret),
                ));
            }

            let mut params = Vec::new();
            params.push(self.parse_type()?);
            self.skip_newlines();
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                params.push(self.parse_type()?);
                self.skip_newlines();
            }
            self.expect(&Token::Arrow)?;
            let ret = self.parse_type()?;
            self.skip_newlines();
            let end = self.expect(&Token::RParen)?;
            return Ok(AstTy::Func(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                params,
                Box::new(ret),
            ));
        }

        if matches!(self.peek(), Token::Dollar) {
            self.advance();
            let (name, end) = self.expect_ident()?;
            return Ok(AstTy::Named(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                format!("${}", name),
            ));
        }

        // Named type, possibly with type args: Result<Int> or Result<Int, Error>
        let (name, _) = self.expect_ident()?;

        // Check for type parameters: Name<T> or Name<T, E>
        if matches!(self.peek(), Token::Lt) {
            self.advance();
            self.skip_newlines();
            let first = self.parse_type()?;
            self.skip_newlines();
            let second = if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                Some(Box::new(self.parse_type()?))
            } else {
                None
            };
            self.skip_newlines();
            let end = self.expect(&Token::Gt)?;
            let span = Span {
                start: sp.start,
                end: end.end,
            };

            if name == "Result" {
                return Ok(AstTy::ResultOf(span, Box::new(first), second));
            }
            if name == "List" && second.is_none() {
                return Ok(AstTy::ListOf(span, Box::new(first)));
            }
            // Keep parsing the generic syntax so we can reserve it for future user-defined
            // generic types, but for now only `Result<...>` and `List<...>` are preserved.
            return Ok(AstTy::Named(span, name));
        }

        Ok(AstTy::Named(sp, name))
    }

    fn parse_closure_literal(&mut self, sp: Span) -> Result<Ast, ParseError> {
        self.expect(&Token::Pipe)?;
        self.skip_newlines();

        let mut params = Vec::new();
        if !matches!(self.peek(), Token::Pipe) {
            loop {
                let (name, pspan) = self.expect_ident()?;
                params.push(ClosureParam { name, span: pspan });
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

    fn parse_capture_expr(&mut self, sp: Span) -> Result<Ast, ParseError> {
        self.expect(&Token::Amp)?;
        let (name, name_span) = self.expect_ident()?;
        let is_uppercase = name
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false);
        if is_uppercase {
            return Err(ParseError::syntax(
                "Constructor capture is not supported; use a lambda instead",
                Span {
                    start: sp.start,
                    end: name_span.end,
                },
            ));
        }

        let mut parsed_args = Vec::new();
        let mut end = name_span.end;
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
            Box::new(Ast::Var(name_span, name)),
            args,
        ))
    }

    // ── Data definitions (step 7, 9) ──

    /// `defstruct Name { field: Type, ... }`
    fn parse_struct_def(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Defstruct)?;
        let (name, _) = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut fields = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            self.skip_newlines();
            let (fname, fspan) = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let fty = self.parse_type()?;
            fields.push(StructField {
                name: fname,
                ty: fty,
                span: fspan,
            });
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::StructDef(
            Span {
                start: sp.start,
                end: end.end,
            },
            name,
            fields,
        ))
    }

    /// `defrecord Name(field: Type, ...)`
    fn parse_record_def(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Defrecord)?;
        let (name, _) = self.expect_ident()?;
        self.expect(&Token::LParen)?;
        self.skip_newlines();

        let mut fields = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete(")", self.peek_span()));
                }
                self.skip_newlines();
                let (fname, fspan) = self.expect_ident()?;
                self.expect(&Token::Colon)?;
                let fty = self.parse_type()?;
                fields.push(RecordField {
                    name: fname,
                    ty: fty,
                    span: fspan,
                });
                self.skip_newlines();
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                    if matches!(self.peek(), Token::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        let end = self.expect(&Token::RParen)?;
        Ok(Ast::RecordDef(
            Span {
                start: sp.start,
                end: end.end,
            },
            name,
            fields,
        ))
    }

    /// `deferror Name { expr }` or `deferror Name(fields) { expr }`
    fn parse_deferror_def(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Deferror)?;
        let (name, _) = self.expect_ident()?;

        // Optional fields: (field: Type, ...)
        let mut fields = Vec::new();
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            self.skip_newlines();
            if !matches!(self.peek(), Token::RParen) {
                loop {
                    if matches!(self.peek(), Token::Eof) {
                        return Err(ParseError::incomplete(")", self.peek_span()));
                    }
                    self.skip_newlines();
                    let (fname, fspan) = self.expect_ident()?;
                    self.expect(&Token::Colon)?;
                    let fty = self.parse_type()?;
                    fields.push(RecordField {
                        name: fname,
                        ty: fty,
                        span: fspan,
                    });
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            self.expect(&Token::RParen)?;
        }

        // Show block: { expr }
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();
        let show_expr = self.parse_expr()?;
        self.skip_newlines();
        let end = self.expect(&Token::RBrace)?;

        Ok(Ast::DeferrorDef(
            Span {
                start: sp.start,
                end: end.end,
            },
            name,
            fields,
            Box::new(show_expr),
        ))
    }

    fn parse_def_signature(
        &mut self,
    ) -> Result<(Span, Symbol, Vec<FunParam>, Option<AstTy>), ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Def)?;
        let (name, _) = self.expect_ident()?;
        let mut params = Vec::new();
        if matches!(self.peek(), Token::Unit) {
            self.advance();
        } else {
            self.expect(&Token::LParen)?;
            self.skip_newlines();

            if !matches!(self.peek(), Token::RParen) {
                loop {
                    if matches!(self.peek(), Token::Eof) {
                        return Err(ParseError::incomplete(")", self.peek_span()));
                    }
                    self.skip_newlines();
                    params.push(self.parse_fun_param()?);
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            self.expect(&Token::RParen)?;
        }

        let ret_ty = if matches!(self.peek(), Token::Arrow) {
            self.advance();
            self.skip_newlines();
            Some(self.parse_type()?)
        } else {
            None
        };

        Ok((sp, name, params, ret_ty))
    }

    fn parse_annotated_decl(&mut self) -> Result<Ast, ParseError> {
        let (annotator, annotator_span) = match self.peek().clone() {
            Token::Annotator(name) => {
                let span = self.peek_span();
                self.advance();
                (name, span)
            }
            _ => {
                return Err(ParseError::syntax(
                    "Expected annotator declaration",
                    self.peek_span(),
                ));
            }
        };

        self.skip_newlines();
        match annotator.as_str() {
            "builtin" => self.parse_builtin_decl(annotator_span),
            _ => Err(ParseError::syntax(
                format!("Unknown annotator: @@{}", annotator),
                annotator_span,
            )),
        }
    }

    fn parse_builtin_decl(&mut self, sp: Span) -> Result<Ast, ParseError> {
        let (_def_span, name, params, ret_ty) = self.parse_def_signature()?;

        let mut lookahead = self.pos;
        while matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::Newline)
        ) {
            lookahead += 1;
        }

        if matches!(
            self.tokens.get(lookahead).map(|sp| &sp.token),
            Some(Token::LBrace)
        ) {
            return Err(ParseError::syntax(
                "@@builtin declaration must not have a function body",
                self.tokens[lookahead].span.clone(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            sp.end
        };

        Ok(Ast::BuiltinDecl(
            Span {
                start: sp.start,
                end,
            },
            name,
            params,
            ret_ty,
        ))
    }

    /// `def name(arg: Type, ...) -> Type { expr }`
    fn parse_def(&mut self) -> Result<Ast, ParseError> {
        let (sp, name, params, ret_ty) = self.parse_def_signature()?;

        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body_stmts = self.parse_block_stmts()?;
        let end = self.expect(&Token::RBrace)?;
        let body = Ast::Block(
            Span {
                start: sp.start,
                end: end.end,
            },
            body_stmts,
        );

        Ok(Ast::Def(
            Span {
                start: sp.start,
                end: end.end,
            },
            name,
            params,
            ret_ty,
            Box::new(body),
        ))
    }

    fn parse_fun_param(&mut self) -> Result<FunParam, ParseError> {
        let (name, span) = self.expect_ident()?;
        self.expect(&Token::Colon)?;
        let ty = self.parse_type()?;
        Ok(FunParam { name, ty, span })
    }

    // ── Match expression (step 8) ──

    /// `match expr { pat => body, ... }`
    fn parse_match_expr(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Match)?;
        let scrutinee = self.parse_expr()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

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

    /// Match pattern: `_`, literals, `Ok(var)`, `Err(var)`
    fn parse_match_pattern(&mut self) -> Result<AstMatchPattern, ParseError> {
        let sp = self.peek_span();
        match self.peek().clone() {
            Token::LBrack => self.parse_match_list_pattern(),
            Token::Ident(name) if name == "_" => {
                self.advance();
                Ok(AstMatchPattern::Wildcard(sp))
            }
            Token::True => {
                self.advance();
                Ok(AstMatchPattern::BoolLit(sp, true))
            }
            Token::False => {
                self.advance();
                Ok(AstMatchPattern::BoolLit(sp, false))
            }
            Token::Int(n) => {
                self.advance();
                Ok(AstMatchPattern::IntLit(sp, n))
            }
            Token::Minus => {
                self.advance();
                match self.peek().clone() {
                    Token::Int(n) => {
                        let int_span = self.peek_span();
                        self.advance();
                        Ok(AstMatchPattern::IntLit(
                            Span {
                                start: sp.start,
                                end: int_span.end,
                            },
                            -n,
                        ))
                    }
                    _ => Err(ParseError::syntax(
                        "Expected integer after '-' in match pattern",
                        sp,
                    )),
                }
            }
            Token::Str(s) => {
                self.advance();
                Ok(AstMatchPattern::StrLit(sp, s))
            }
            Token::Ident(name) => {
                self.advance();
                if name == "Ok" || name == "Err" {
                    self.expect(&Token::LParen)?;
                    let (inner_name, _) = self.expect_ident()?;
                    let end = self.expect(&Token::RParen)?;
                    Ok(AstMatchPattern::Constructor(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        name,
                        Some(inner_name),
                    ))
                } else {
                    Err(ParseError::syntax(
                        "Phase 1 match patterns only support `_`, booleans, integer/string literals, and `Ok(name)` / `Err(name)`; remove this test when CamelCase patterns are implemented",
                        sp,
                    ))
                }
            }
            Token::Eof => Err(ParseError::incomplete("match pattern", sp)),
            _ => Err(ParseError::syntax(
                format!("Expected match pattern, got {:?}", self.peek()),
                sp,
            )),
        }
    }

    fn parse_match_list_pattern(&mut self) -> Result<AstMatchPattern, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::LBrack)?;
        self.skip_newlines();
        if matches!(self.peek(), Token::RBrack) {
            let end = self.expect(&Token::RBrack)?;
            return Ok(AstMatchPattern::ListNil(Span {
                start: sp.start,
                end: end.end,
            }));
        }

        let first = self.parse_match_list_item_pattern()?;
        self.skip_newlines();
        let end = if matches!(self.peek(), Token::Comma) {
            self.advance();
            self.skip_newlines();
            if matches!(self.peek(), Token::DotDot) {
                self.advance();
                self.skip_newlines();
                let tail = self.parse_match_list_item_pattern()?;
                self.skip_newlines();
                let end = self.expect(&Token::RBrack)?;
                return Ok(AstMatchPattern::ListCons(
                    Span {
                        start: sp.start,
                        end: end.end,
                    },
                    Box::new(first),
                    Box::new(tail),
                ));
            }

            let mut items = vec![first];
            items.push(self.parse_match_list_item_pattern()?);
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::RBrack) {
                    break;
                }
                items.push(self.parse_match_list_item_pattern()?);
            }
            self.skip_newlines();
            let end = self.expect(&Token::RBrack)?;
            return Ok(fixed_match_list_pattern(sp.start, end.end, items));
        } else {
            self.expect(&Token::RBrack)?
        };

        Ok(fixed_match_list_pattern(sp.start, end.end, vec![first]))
    }

    fn parse_match_list_item_pattern(&mut self) -> Result<AstMatchPattern, ParseError> {
        let sp = self.peek_span();
        match self.peek().clone() {
            Token::Ident(name) if name == "_" => {
                self.advance();
                Ok(AstMatchPattern::Wildcard(sp))
            }
            Token::Ident(name) => {
                self.advance();
                if name == "Ok" || name == "Err" {
                    self.expect(&Token::LParen)?;
                    let (inner_name, _) = self.expect_ident()?;
                    let end = self.expect(&Token::RParen)?;
                    Ok(AstMatchPattern::Constructor(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        name,
                        Some(inner_name),
                    ))
                } else {
                    Ok(AstMatchPattern::Binding(sp, name))
                }
            }
            Token::LBrack => self.parse_match_list_pattern(),
            Token::True => {
                self.advance();
                Ok(AstMatchPattern::BoolLit(sp, true))
            }
            Token::False => {
                self.advance();
                Ok(AstMatchPattern::BoolLit(sp, false))
            }
            Token::Int(n) => {
                self.advance();
                Ok(AstMatchPattern::IntLit(sp, n))
            }
            Token::Minus => {
                self.advance();
                match self.peek().clone() {
                    Token::Int(n) => {
                        let int_span = self.peek_span();
                        self.advance();
                        Ok(AstMatchPattern::IntLit(
                            Span {
                                start: sp.start,
                                end: int_span.end,
                            },
                            -n,
                        ))
                    }
                    _ => Err(ParseError::syntax(
                        "Expected integer after '-' in list pattern item",
                        sp,
                    )),
                }
            }
            Token::Str(s) => {
                self.advance();
                Ok(AstMatchPattern::StrLit(sp, s))
            }
            _ => Err(ParseError::syntax("Invalid list pattern item", sp)),
        }
    }

    fn parse_string_or_interpolated(&mut self, span: Span, raw: String) -> Result<Ast, ParseError> {
        let parts = self.parse_interpolated_parts(&raw, &span)?;
        if parts.is_empty() {
            Ok(Ast::Lit(span, Lit::Str(raw)))
        } else {
            Ok(Ast::InterpolatedStr(span, parts))
        }
    }

    fn parse_interpolated_parts(
        &mut self,
        raw: &str,
        base_span: &Span,
    ) -> Result<Vec<InterpolatedPart>, ParseError> {
        let chars: Vec<char> = raw.chars().collect();
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut i = 0;
        let mut has_interpolation = false;

        while i < chars.len() {
            let ch = chars[i];
            let is_interp_start = ch == '#'
                && i + 1 < chars.len()
                && chars[i + 1] == '{'
                && (i == 0 || chars[i - 1] != '\\');
            if !is_interp_start {
                text.push(ch);
                i += 1;
                continue;
            }

            has_interpolation = true;
            if !text.is_empty() {
                parts.push(InterpolatedPart::Text(std::mem::take(&mut text)));
            }

            i += 2; // skip #{
            let expr_start = i;
            let mut depth = 1usize;
            let mut expr_src = String::new();
            let mut quoted_by: Option<char> = None;
            let mut escaped = false;
            let mut in_comment = false;
            while i < chars.len() {
                let c = chars[i];
                if let Some(quote) = quoted_by {
                    expr_src.push(c);
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == quote {
                        quoted_by = None;
                    }
                    i += 1;
                    continue;
                }

                if in_comment {
                    expr_src.push(c);
                    if c == '\n' {
                        in_comment = false;
                    }
                    i += 1;
                    continue;
                }

                if c == '"' || c == '\'' {
                    quoted_by = Some(c);
                    expr_src.push(c);
                    i += 1;
                    continue;
                }

                if c == '#' {
                    in_comment = true;
                    expr_src.push(c);
                    i += 1;
                    continue;
                }

                if c == '{' {
                    depth += 1;
                    expr_src.push(c);
                    i += 1;
                    continue;
                }
                if c == '}' {
                    depth -= 1;
                    if depth == 0 {
                        i += 1; // consume closing }
                        break;
                    }
                    expr_src.push(c);
                    i += 1;
                    continue;
                }
                expr_src.push(c);
                i += 1;
            }

            if depth != 0 {
                return Err(ParseError::incomplete("}", base_span.clone()));
            }

            let parsed = parse(&expr_src).map_err(|e| {
                let expr_offset = base_span.start + 1 + expr_start;
                let mapped = Span {
                    start: expr_offset + e.span().start,
                    end: expr_offset + e.span().end,
                };
                ParseError::syntax(
                    format!("Invalid interpolation expression: {}", e.message()),
                    mapped,
                )
            })?;
            if parsed.len() != 1 {
                return Err(ParseError::syntax(
                    "Interpolation expression must contain exactly one expression",
                    base_span.clone(),
                ));
            }
            let expr_offset = base_span.start + 1 + expr_start;
            let expr = shift_ast_span(parsed.into_iter().next().unwrap(), expr_offset);
            parts.push(InterpolatedPart::Expr(Box::new(expr)));
        }

        if !text.is_empty() {
            parts.push(InterpolatedPart::Text(text));
        }

        if has_interpolation {
            Ok(parts)
        } else {
            Ok(Vec::new())
        }
    }
}

fn shift_span(span: Span, delta: usize) -> Span {
    Span {
        start: span.start + delta,
        end: span.end + delta,
    }
}

fn pattern_span(pat: &AstPattern) -> &Span {
    match pat {
        AstPattern::Var(span, _)
        | AstPattern::Annotated(span, _, _)
        | AstPattern::Wildcard(span)
        | AstPattern::ListNil(span)
        | AstPattern::ListCons(span, _, _)
        | AstPattern::IntLit(span, _)
        | AstPattern::StrLit(span, _)
        | AstPattern::BoolLit(span, _)
        | AstPattern::Constructor(span, _, _) => span,
    }
}

fn fixed_bind_list_pattern(start: usize, end: usize, items: Vec<AstPattern>) -> AstPattern {
    let span = Span { start, end };
    items
        .into_iter()
        .rev()
        .fold(AstPattern::ListNil(span.clone()), |tail, head| {
            AstPattern::ListCons(span.clone(), Box::new(head), Box::new(tail))
        })
}

fn fixed_match_list_pattern(
    start: usize,
    end: usize,
    items: Vec<AstMatchPattern>,
) -> AstMatchPattern {
    let span = Span { start, end };
    items
        .into_iter()
        .rev()
        .fold(AstMatchPattern::ListNil(span.clone()), |tail, head| {
            AstMatchPattern::ListCons(span.clone(), Box::new(head), Box::new(tail))
        })
}

fn shift_ast_ty(ty: AstTy, delta: usize) -> AstTy {
    match ty {
        AstTy::Named(span, name) => AstTy::Named(shift_span(span, delta), name),
        AstTy::ListOf(span, inner) => AstTy::ListOf(
            shift_span(span, delta),
            Box::new(shift_ast_ty(*inner, delta)),
        ),
        AstTy::ResultOf(span, ok, err) => AstTy::ResultOf(
            shift_span(span, delta),
            Box::new(shift_ast_ty(*ok, delta)),
            err.map(|e| Box::new(shift_ast_ty(*e, delta))),
        ),
        AstTy::Func(span, params, ret) => AstTy::Func(
            shift_span(span, delta),
            params.into_iter().map(|p| shift_ast_ty(p, delta)).collect(),
            Box::new(shift_ast_ty(*ret, delta)),
        ),
    }
}

fn shift_pattern(pat: AstPattern, delta: usize) -> AstPattern {
    match pat {
        AstPattern::Var(span, name) => AstPattern::Var(shift_span(span, delta), name),
        AstPattern::Annotated(span, name, ty) => {
            AstPattern::Annotated(shift_span(span, delta), name, shift_ast_ty(ty, delta))
        }
        AstPattern::Wildcard(span) => AstPattern::Wildcard(shift_span(span, delta)),
        AstPattern::ListNil(span) => AstPattern::ListNil(shift_span(span, delta)),
        AstPattern::ListCons(span, head, tail) => AstPattern::ListCons(
            shift_span(span, delta),
            Box::new(shift_pattern(*head, delta)),
            Box::new(shift_pattern(*tail, delta)),
        ),
        AstPattern::IntLit(span, n) => AstPattern::IntLit(shift_span(span, delta), n),
        AstPattern::StrLit(span, s) => AstPattern::StrLit(shift_span(span, delta), s),
        AstPattern::BoolLit(span, b) => AstPattern::BoolLit(shift_span(span, delta), b),
        AstPattern::Constructor(span, name, inner) => AstPattern::Constructor(
            shift_span(span, delta),
            name,
            Box::new(shift_pattern(*inner, delta)),
        ),
    }
}

fn shift_fun_param(param: FunParam, delta: usize) -> FunParam {
    FunParam {
        name: param.name,
        ty: shift_ast_ty(param.ty, delta),
        span: shift_span(param.span, delta),
    }
}

fn shift_match_pattern(pat: AstMatchPattern, delta: usize) -> AstMatchPattern {
    match pat {
        AstMatchPattern::Binding(span, name) => {
            AstMatchPattern::Binding(shift_span(span, delta), name)
        }
        AstMatchPattern::Wildcard(span) => AstMatchPattern::Wildcard(shift_span(span, delta)),
        AstMatchPattern::BoolLit(span, b) => AstMatchPattern::BoolLit(shift_span(span, delta), b),
        AstMatchPattern::IntLit(span, n) => AstMatchPattern::IntLit(shift_span(span, delta), n),
        AstMatchPattern::StrLit(span, s) => AstMatchPattern::StrLit(shift_span(span, delta), s),
        AstMatchPattern::Constructor(span, ctor, inner) => {
            AstMatchPattern::Constructor(shift_span(span, delta), ctor, inner)
        }
        AstMatchPattern::ListNil(span) => AstMatchPattern::ListNil(shift_span(span, delta)),
        AstMatchPattern::ListCons(span, head, tail) => AstMatchPattern::ListCons(
            shift_span(span, delta),
            Box::new(shift_match_pattern(*head, delta)),
            Box::new(shift_match_pattern(*tail, delta)),
        ),
    }
}

fn shift_record_lit_arg(arg: RecordLitArg, delta: usize) -> RecordLitArg {
    match arg {
        RecordLitArg::Positional(expr) => RecordLitArg::Positional(shift_ast_span(expr, delta)),
        RecordLitArg::Named(name, expr) => RecordLitArg::Named(name, shift_ast_span(expr, delta)),
    }
}

fn shift_ast_span(ast: Ast, delta: usize) -> Ast {
    match ast {
        Ast::Lit(span, lit) => Ast::Lit(shift_span(span, delta), lit),
        Ast::Var(span, name) => Ast::Var(shift_span(span, delta), name),
        Ast::App(span, func, args) => Ast::App(
            shift_span(span, delta),
            Box::new(shift_ast_span(*func, delta)),
            args.into_iter()
                .map(|a| shift_record_lit_arg(a, delta))
                .collect(),
        ),
        Ast::Block(span, stmts) => Ast::Block(
            shift_span(span, delta),
            stmts
                .into_iter()
                .map(|s| shift_ast_span(s, delta))
                .collect(),
        ),
        Ast::Bind(span, pat, rhs) => Ast::Bind(
            shift_span(span, delta),
            shift_pattern(pat, delta),
            Box::new(shift_ast_span(*rhs, delta)),
        ),
        Ast::SafeBind(span, pat, rhs) => Ast::SafeBind(
            shift_span(span, delta),
            shift_pattern(pat, delta),
            Box::new(shift_ast_span(*rhs, delta)),
        ),
        Ast::BinOp(span, op, left, right) => Ast::BinOp(
            shift_span(span, delta),
            op,
            Box::new(shift_ast_span(*left, delta)),
            Box::new(shift_ast_span(*right, delta)),
        ),
        Ast::ListNil(span) => Ast::ListNil(shift_span(span, delta)),
        Ast::ListCons(span, head, tail) => Ast::ListCons(
            shift_span(span, delta),
            Box::new(shift_ast_span(*head, delta)),
            Box::new(shift_ast_span(*tail, delta)),
        ),
        Ast::ListLiteral(span, elems) => Ast::ListLiteral(
            shift_span(span, delta),
            elems
                .into_iter()
                .map(|e| shift_ast_span(e, delta))
                .collect(),
        ),
        Ast::InterpolatedStr(span, parts) => Ast::InterpolatedStr(
            shift_span(span, delta),
            parts
                .into_iter()
                .map(|p| match p {
                    InterpolatedPart::Text(s) => InterpolatedPart::Text(s),
                    InterpolatedPart::Expr(expr) => {
                        InterpolatedPart::Expr(Box::new(shift_ast_span(*expr, delta)))
                    }
                })
                .collect(),
        ),
        Ast::Match(span, expr, arms) => Ast::Match(
            shift_span(span, delta),
            Box::new(shift_ast_span(*expr, delta)),
            arms.into_iter()
                .map(|(pat, body)| (shift_match_pattern(pat, delta), shift_ast_span(body, delta)))
                .collect(),
        ),
        Ast::FieldAccess(span, expr, field) => Ast::FieldAccess(
            shift_span(span, delta),
            Box::new(shift_ast_span(*expr, delta)),
            field,
        ),
        Ast::StructDef(span, name, fields) => Ast::StructDef(
            shift_span(span, delta),
            name,
            fields
                .into_iter()
                .map(|f| StructField {
                    name: f.name,
                    ty: shift_ast_ty(f.ty, delta),
                    span: shift_span(f.span, delta),
                })
                .collect(),
        ),
        Ast::RecordDef(span, name, fields) => Ast::RecordDef(
            shift_span(span, delta),
            name,
            fields
                .into_iter()
                .map(|f| RecordField {
                    name: f.name,
                    ty: shift_ast_ty(f.ty, delta),
                    span: shift_span(f.span, delta),
                })
                .collect(),
        ),
        Ast::StructLit(span, name, fields) => Ast::StructLit(
            shift_span(span, delta),
            name,
            fields
                .into_iter()
                .map(|(name, expr)| (name, shift_ast_span(expr, delta)))
                .collect(),
        ),
        Ast::ConstructorCall(span, name, args) => Ast::ConstructorCall(
            shift_span(span, delta),
            name,
            args.into_iter()
                .map(|a| shift_record_lit_arg(a, delta))
                .collect(),
        ),
        Ast::DeferrorDef(span, name, fields, show_expr) => Ast::DeferrorDef(
            shift_span(span, delta),
            name,
            fields
                .into_iter()
                .map(|f| RecordField {
                    name: f.name,
                    ty: shift_ast_ty(f.ty, delta),
                    span: shift_span(f.span, delta),
                })
                .collect(),
            Box::new(shift_ast_span(*show_expr, delta)),
        ),
        Ast::Def(span, name, params, ret_ty, body) => Ast::Def(
            shift_span(span, delta),
            name,
            params
                .into_iter()
                .map(|p| shift_fun_param(p, delta))
                .collect(),
            ret_ty.map(|ty| shift_ast_ty(ty, delta)),
            Box::new(shift_ast_span(*body, delta)),
        ),
        Ast::BuiltinDecl(span, name, params, ret_ty) => Ast::BuiltinDecl(
            shift_span(span, delta),
            name,
            params
                .into_iter()
                .map(|p| shift_fun_param(p, delta))
                .collect(),
            ret_ty.map(|ty| shift_ast_ty(ty, delta)),
        ),
        Ast::Closure(span, params, body) => Ast::Closure(
            shift_span(span, delta),
            params
                .into_iter()
                .map(|p| ClosureParam {
                    name: p.name,
                    span: shift_span(p.span, delta),
                })
                .collect(),
            Box::new(shift_ast_span(*body, delta)),
        ),
        Ast::Capture(span, target, args) => Ast::Capture(
            shift_span(span, delta),
            Box::new(shift_ast_span(*target, delta)),
            args.into_iter().map(|a| shift_ast_span(a, delta)).collect(),
        ),
        Ast::Semi(span, inner) => Ast::Semi(
            shift_span(span, delta),
            Box::new(shift_ast_span(*inner, delta)),
        ),
    }
}

// ── Ast span accessor ──

impl Ast {
    pub fn span(&self) -> &Span {
        match self {
            Ast::Lit(s, _)
            | Ast::Var(s, _)
            | Ast::App(s, _, _)
            | Ast::Block(s, _)
            | Ast::Bind(s, _, _)
            | Ast::SafeBind(s, _, _)
            | Ast::BinOp(s, _, _, _)
            | Ast::ListNil(s)
            | Ast::ListCons(s, _, _)
            | Ast::ListLiteral(s, _)
            | Ast::InterpolatedStr(s, _)
            | Ast::Match(s, _, _)
            | Ast::FieldAccess(s, _, _)
            | Ast::StructDef(s, _, _)
            | Ast::RecordDef(s, _, _)
            | Ast::StructLit(s, _, _)
            | Ast::ConstructorCall(s, _, _)
            | Ast::DeferrorDef(s, _, _, _)
            | Ast::Def(s, _, _, _, _)
            | Ast::BuiltinDecl(s, _, _, _)
            | Ast::Closure(s, _, _)
            | Ast::Capture(s, _, _)
            | Ast::Semi(s, _) => s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_and_var() {
        let ast = parse("x = 42").unwrap();
        assert_eq!(ast.len(), 1);
        match &ast[0] {
            Ast::Bind(_, AstPattern::Var(_, name), rhs) => {
                assert_eq!(name, "x");
                assert!(matches!(rhs.as_ref(), Ast::Lit(_, Lit::Int(42))));
            }
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_annotated_bind() {
        let ast = parse("num: Int = 10").unwrap();
        match &ast[0] {
            Ast::Bind(_, AstPattern::Annotated(_, name, AstTy::Named(_, ty)), _) => {
                assert_eq!(name, "num");
                assert_eq!(ty, "Int");
            }
            _ => panic!("Expected annotated Bind"),
        }
    }

    #[test]
    fn test_safebind() {
        let ast = parse("num =? gen()").unwrap();
        match &ast[0] {
            Ast::SafeBind(_, AstPattern::Var(_, name), rhs) => {
                assert_eq!(name, "num");
                assert!(matches!(rhs.as_ref(), Ast::App(_, _, _)));
            }
            _ => panic!("Expected SafeBind"),
        }
    }

    #[test]
    fn test_assignment_operator_is_non_associative() {
        let err = parse("x = y =? z").expect_err("Expected parse error");
        assert!(err.message().contains("non-associative"));
    }

    #[test]
    fn test_function_call() {
        let ast = parse("print(to_string(num))").unwrap();
        match &ast[0] {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "print"));
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0], RecordLitArg::Positional(_)));
            }
            _ => panic!("Expected App"),
        }
    }

    #[test]
    fn test_function_call_named_args() {
        let ast = parse("add(y: 2, x: 1)").unwrap();
        match &ast[0] {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "add"));
                assert_eq!(args.len(), 2);
                assert!(matches!(&args[0], RecordLitArg::Named(n, _) if n == "y"));
                assert!(matches!(&args[1], RecordLitArg::Named(n, _) if n == "x"));
            }
            _ => panic!("Expected App"),
        }
    }

    #[test]
    fn test_zero_arg_call() {
        let ast = parse("x = noop()").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::App(_, func, args) => {
                    assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "noop"));
                    assert!(args.is_empty());
                }
                _ => panic!("Expected zero-arg App"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_function_def() {
        let ast = parse(
            r#"def add(x: Int, y: Int) -> Int { x + y }
def noop() {()}"#,
        )
        .unwrap();
        match &ast[0] {
            Ast::Def(_, name, params, ret_ty, body) => {
                assert_eq!(name, "add");
                assert_eq!(params.len(), 2);
                assert!(matches!(ret_ty, Some(AstTy::Named(_, ty)) if ty == "Int"));
                assert!(
                    matches!(body.as_ref(), Ast::Block(_, stmts) if matches!(stmts.as_slice(), [Ast::BinOp(_, BinOp::Add, _, _)]))
                );
            }
            _ => panic!("Expected Def"),
        }
        match &ast[1] {
            Ast::Def(_, name, params, ret_ty, body) => {
                assert_eq!(name, "noop");
                assert_eq!(params.len(), 0);
                assert!(ret_ty.is_none());
                assert!(
                    matches!(body.as_ref(), Ast::Block(_, stmts) if matches!(stmts.as_slice(), [Ast::Lit(_, Lit::Unit)]))
                );
            }
            _ => panic!("Expected Def"),
        }
    }

    #[test]
    fn test_builtin_decl() {
        let ast = parse("@@builtin def to_string(a: $A) -> String").unwrap();
        match &ast[0] {
            Ast::BuiltinDecl(_, name, params, ret_ty) => {
                assert_eq!(name, "to_string");
                assert_eq!(params.len(), 1);
                assert!(matches!(
                    params[0].ty,
                    AstTy::Named(_, ref name) if name == "$A"
                ));
                assert!(matches!(ret_ty, Some(AstTy::Named(_, ty)) if ty == "String"));
            }
            _ => panic!("Expected BuiltinDecl"),
        }
    }

    #[test]
    fn test_builtin_decl_with_body_is_error() {
        let err =
            parse("@@builtin def print(a: String) -> Unit { print(a) }").expect_err("error");
        assert!(err.message().contains("must not have a function body"));
    }

    #[test]
    fn test_unknown_annotator_is_error() {
        let err = parse("@@memo def f()").expect_err("error");
        assert!(err.message().contains("Unknown annotator: @@memo"));
    }

    #[test]
    fn test_binop() {
        let ast = parse("x = 10 + 5").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Ast::BinOp(_, BinOp::Add, _, _)));
            }
            _ => panic!("Expected Bind with BinOp"),
        }
    }

    #[test]
    fn test_precedence() {
        // 1 + 2 * 3 should parse as 1 + (2 * 3)
        let ast = parse("x = 1 + 2 * 3").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::BinOp(_, BinOp::Add, left, right) => {
                    assert!(matches!(left.as_ref(), Ast::Lit(_, Lit::Int(1))));
                    assert!(matches!(right.as_ref(), Ast::BinOp(_, BinOp::Mul, _, _)));
                }
                _ => panic!("Expected Add at top"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_list_literal() {
        let ast = parse("nums = [1, 2, 3]").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::ListLiteral(_, elems) => assert_eq!(elems.len(), 3),
                _ => panic!("Expected ListLiteral"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_empty_list_with_annotation() {
        let ast = parse("empty: List<Int> = []").unwrap();
        match &ast[0] {
            Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::ListOf(_, inner)), rhs) => {
                assert!(matches!(inner.as_ref(), AstTy::Named(_, ref n) if n == "Int"));
                assert!(matches!(rhs.as_ref(), Ast::ListNil(_)));
            }
            _ => panic!("Expected annotated Bind with empty List"),
        }
    }

    #[test]
    fn test_list_cons_expr() {
        let ast = parse("nums = [1, ..tail]").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::ListCons(_, head, tail) => {
                    assert!(matches!(head.as_ref(), Ast::Lit(_, Lit::Int(1))));
                    assert!(matches!(tail.as_ref(), Ast::Var(_, name) if name == "tail"));
                }
                _ => panic!("Expected ListCons"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_list_pattern_safebind() {
        let ast = parse("[head, ..tail] =? value").unwrap();
        match &ast[0] {
            Ast::SafeBind(_, pattern, rhs) => {
                assert!(matches!(
                    pattern,
                    AstPattern::ListCons(_, head, tail)
                        if matches!(head.as_ref(), AstPattern::Var(_, name) if name == "head")
                        && matches!(tail.as_ref(), AstPattern::Var(_, name) if name == "tail")
                ));
                assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
            }
            _ => panic!("Expected SafeBind"),
        }
    }

    #[test]
    fn test_constructor_pattern_safebind() {
        let ast = parse("Ok(num) =? value").unwrap();
        match &ast[0] {
            Ast::SafeBind(_, pattern, rhs) => {
                assert!(matches!(
                    pattern,
                    AstPattern::Constructor(_, ctor, inner)
                        if ctor == "Ok"
                        && matches!(inner.as_ref(), AstPattern::Var(_, name) if name == "num")
                ));
                assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
            }
            _ => panic!("Expected SafeBind"),
        }
    }

    #[test]
    fn test_wildcard_pattern_safebind() {
        let ast = parse("_ =? value").unwrap();
        match &ast[0] {
            Ast::SafeBind(_, pattern, rhs) => {
                assert!(matches!(pattern, AstPattern::Wildcard(_)));
                assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
            }
            _ => panic!("Expected SafeBind"),
        }
    }

    #[test]
    fn test_integer_literal_pattern_safebind() {
        let ast = parse("1 =? value").unwrap();
        match &ast[0] {
            Ast::SafeBind(_, pattern, rhs) => {
                assert!(matches!(pattern, AstPattern::IntLit(_, 1)));
                assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
            }
            _ => panic!("Expected SafeBind"),
        }
    }

    #[test]
    fn test_list_pattern_with_nested_constructor_literals_safebind() {
        let ast = parse("[Ok(1), Ok(2), _] =? lr").unwrap();
        match &ast[0] {
            Ast::SafeBind(_, pattern, rhs) => {
                assert!(matches!(
                    pattern,
                    AstPattern::ListCons(_, first, rest)
                        if matches!(first.as_ref(),
                            AstPattern::Constructor(_, ctor, inner)
                            if ctor == "Ok" && matches!(inner.as_ref(), AstPattern::IntLit(_, 1))
                        )
                        && matches!(rhs.as_ref(), Ast::Var(_, name) if name == "lr")
                        && matches!(rest.as_ref(), AstPattern::ListCons(_, _, _))
                ));
            }
            _ => panic!("Expected SafeBind"),
        }
    }

    #[test]
    fn test_result_type_annotation() {
        let ast = parse("r: Result<Int> = Ok(42)").unwrap();
        match &ast[0] {
            Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::ResultOf(_, ok_ty, None)), _) => {
                assert!(matches!(ok_ty.as_ref(), AstTy::Named(_, ref n) if n == "Int"));
            }
            _ => panic!("Expected annotated Bind with Result type"),
        }
    }

    #[test]
    fn test_function_type_and_closure_literal() {
        let ast = parse("fun: (Int -> Unit) = {|val| do_something(val)}").unwrap();
        match &ast[0] {
            Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Func(_, params, ret)), rhs) => {
                assert_eq!(params.len(), 1);
                assert!(matches!(params[0], AstTy::Named(_, ref n) if n == "Int"));
                assert!(matches!(ret.as_ref(), AstTy::Named(_, ref n) if n == "Unit"));
                assert!(
                    matches!(rhs.as_ref(), Ast::Closure(_, params, body) if params.len() == 1 && matches!(body.as_ref(), Ast::App(_, _, _)))
                );
            }
            _ => panic!("Expected annotated Bind with function type and closure"),
        }
    }

    #[test]
    fn test_multiline_function_type_annotation() {
        let ast = parse(
            r#"handler: (
  Int,
  String
  -> Unit
) = {|x, y| print(y)}"#,
        )
        .unwrap();
        match &ast[0] {
            Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Func(_, params, ret)), rhs) => {
                assert_eq!(params.len(), 2);
                assert!(matches!(params[0], AstTy::Named(_, ref name) if name == "Int"));
                assert!(matches!(params[1], AstTy::Named(_, ref name) if name == "String"));
                assert!(matches!(ret.as_ref(), AstTy::Named(_, name) if name == "Unit"));
                assert!(matches!(rhs.as_ref(), Ast::Closure(_, params, _) if params.len() == 2));
            }
            _ => panic!("Expected multiline function type bind"),
        }
    }

    #[test]
    fn test_capture_and_zero_arg_closure() {
        let ast = parse("f = &print\nnoop: (-> Unit) = {|| print(\"x\")}").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => {
                assert!(
                    matches!(rhs.as_ref(), Ast::Capture(_, target, args) if args.is_empty() && matches!(target.as_ref(), Ast::Var(_, ref n) if n == "print"))
                );
            }
            _ => panic!("Expected Capture"),
        }
        match &ast[1] {
            Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Func(_, params, _)), rhs) => {
                assert!(params.is_empty());
                assert!(matches!(rhs.as_ref(), Ast::Closure(_, params, _) if params.is_empty()));
            }
            _ => panic!("Expected zero-arg closure"),
        }
    }

    #[test]
    fn test_closure_body_accepts_semicolon_separated_statements() {
        let ast = parse("fun = {|num| x = x + 5;x+num}").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::Closure(_, params, body) => {
                    assert_eq!(params.len(), 1);
                    assert!(matches!(params[0].name.as_str(), "num"));
                    assert!(matches!(
                        body.as_ref(),
                        Ast::Block(_, stmts)
                            if matches!(stmts.as_slice(), [Ast::Semi(_, _), Ast::BinOp(_, _, _, _)])
                    ));
                }
                _ => panic!("Expected Closure"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_semicolon_wraps_statement_in_semi() {
        let ast = parse("print(\"x\");").unwrap();
        assert!(matches!(
            &ast[0],
            Ast::Semi(_, inner) if matches!(inner.as_ref(), Ast::App(_, _, _))
        ));
    }

    #[test]
    fn test_function_body_trailing_semicolon_is_explicit_unit() {
        let ast = parse("def fun() -> Unit { print(\"x\"); }").unwrap();
        match &ast[0] {
            Ast::Def(_, _, _, _, body) => {
                assert!(matches!(
                    body.as_ref(),
                    Ast::Block(_, stmts) if matches!(stmts.as_slice(), [Ast::Semi(_, inner)] if matches!(inner.as_ref(), Ast::App(_, _, _)))
                ));
            }
            _ => panic!("Expected Def"),
        }
    }

    #[test]
    fn test_multiline() {
        let ast = parse("x = 1\ny = 2\nprint(to_string(x))").unwrap();
        assert_eq!(ast.len(), 3);
    }

    #[test]
    fn test_statements_on_same_line_require_separator() {
        let err = parse("[]1").expect_err("Expected parse error");
        assert!(err.message().contains("Expected newline or `;`"));
    }

    #[test]
    fn test_safebind_rhs_requires_statement_separator() {
        let err = parse("[] =? []1").expect_err("Expected parse error");
        assert!(err.message().contains("Expected newline or `;`"));
    }

    #[test]
    fn test_safebind_allows_trailing_semicolon() {
        let ast = parse("[] =? value;").unwrap();
        assert!(matches!(
            &ast[0],
            Ast::Semi(_, inner) if matches!(inner.as_ref(), Ast::SafeBind(_, _, _))
        ));
    }

    #[test]
    fn test_string_concat() {
        let ast = parse(r#"msg = "hello" ++ " world""#).unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Ast::BinOp(_, BinOp::Concat, _, _)));
            }
            _ => panic!("Expected Bind with Concat"),
        }
    }

    #[test]
    fn test_string_concat_is_left_associative() {
        let ast = parse(r#"msg = a ++ b ++ c"#).unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::BinOp(_, BinOp::Concat, left, right) => {
                    assert!(matches!(right.as_ref(), Ast::Var(_, name) if name == "c"));
                    assert!(matches!(
                        left.as_ref(),
                        Ast::BinOp(_, BinOp::Concat, ll, lr)
                            if matches!(ll.as_ref(), Ast::Var(_, name) if name == "a")
                                && matches!(lr.as_ref(), Ast::Var(_, name) if name == "b")
                    ));
                }
                _ => panic!("Expected nested left-associative concat"),
            },
            _ => panic!("Expected Bind with chained Concat"),
        }
    }

    #[test]
    fn test_interpolated_string_ast() {
        let ast = parse(r#"msg = "hi #{name}!""#).unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::InterpolatedStr(_, parts) => {
                    assert!(matches!(parts.first(), Some(InterpolatedPart::Text(s)) if s == "hi "));
                    assert!(matches!(parts.get(1), Some(InterpolatedPart::Expr(expr))
                            if matches!(expr.as_ref(), Ast::Var(_, name) if name == "name")));
                    assert!(matches!(parts.get(2), Some(InterpolatedPart::Text(s)) if s == "!"));
                }
                _ => panic!("Expected InterpolatedStr"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_interpolated_string_allows_brace_in_inner_string_literal() {
        let ast = parse(r#"msg = '#{to_string("}")}'"#).unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::InterpolatedStr(_, parts) => {
                    assert!(matches!(
                        parts.as_slice(),
                        [InterpolatedPart::Expr(expr)]
                            if matches!(expr.as_ref(), Ast::App(_, _, _))
                    ));
                }
                _ => panic!("Expected InterpolatedStr"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_negative_int() {
        let ast = parse("x = -5").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Ast::Lit(_, Lit::Int(-5))));
            }
            _ => panic!("Expected Bind with negative Int"),
        }
    }

    #[test]
    fn test_negative_variable_reports_phase1_guidance() {
        let err = parse("x = -value").expect_err("Expected parse error");
        assert!(err
            .message()
            .contains("write `0 - value` instead of `-value`"));
    }

    #[test]
    fn test_field_access() {
        let ast = parse("user.name").unwrap();
        assert!(matches!(&ast[0], Ast::FieldAccess(_, _, ref f) if f == "name"));
    }

    #[test]
    fn test_match_wildcard_and_int_pattern() {
        let ast = parse(
            r#"x = match n {
  1 => "one",
  _ => "other",
}"#,
        )
        .unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::Match(_, _, arms) => {
                    assert!(matches!(&arms[0].0, AstMatchPattern::IntLit(_, 1)));
                    assert!(matches!(&arms[1].0, AstMatchPattern::Wildcard(_)));
                }
                _ => panic!("Expected Match"),
            },
            _ => panic!("Expected Bind with Match"),
        }
    }

    #[test]
    fn test_match_string_pattern() {
        let ast = parse(
            r#"x = match s {
  "a" => 1,
  _ => 0,
}"#,
        )
        .unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::Match(_, _, arms) => {
                    assert!(matches!(&arms[0].0, AstMatchPattern::StrLit(_, s) if s == "a"));
                }
                _ => panic!("Expected Match"),
            },
            _ => panic!("Expected Bind with Match"),
        }
    }

    #[test]
    fn test_match_negative_int_in_list_pattern() {
        let ast = parse(
            r#"x = match nums {
  [-1] => "neg",
  _ => "other",
}"#,
        )
        .unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::Match(_, _, arms) => {
                    assert!(matches!(
                        &arms[0].0,
                        AstMatchPattern::ListCons(_, head, tail)
                            if matches!(head.as_ref(), AstMatchPattern::IntLit(_, -1))
                                && matches!(tail.as_ref(), AstMatchPattern::ListNil(_))
                    ));
                }
                _ => panic!("Expected Match"),
            },
            _ => panic!("Expected Bind with Match"),
        }
    }

    #[test]
    fn test_match_camel_case_pattern_is_rejected_for_now() {
        // Remove this test when CamelCase match patterns are intentionally implemented.
        let err = parse(
            r#"x = match value {
  Some(y) => y,
  _ => 0,
}"#,
        )
        .expect_err("Expected parse error");
        assert!(err
            .message()
            .contains("remove this test when CamelCase patterns are implemented"));
    }

    #[test]
    fn test_match_bare_constructor_pattern_is_rejected_for_now() {
        // Remove this test when CamelCase match patterns are intentionally implemented.
        let err = parse(
            r#"x = match value {
  ParseError => 0,
  _ => 1,
}"#,
        )
        .expect_err("Expected parse error");
        assert!(err
            .message()
            .contains("remove this test when CamelCase patterns are implemented"));
    }

    #[test]
    fn test_constructor_capture_is_rejected() {
        let err = parse("f = &Some").expect_err("Expected parse error");
        assert!(err.message().contains("use a lambda instead"));
    }

    #[test]
    fn test_assignment_is_rejected_in_argument_position() {
        let err = parse("f(x: y = 1)").expect_err("Expected parse error");
        assert!(err.message().contains("cannot appear in argument position"));
    }
}
