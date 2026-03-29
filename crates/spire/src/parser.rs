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
            stmts.push(stmt);
            // consume statement separators
            while matches!(self.peek(), Token::Newline | Token::Semicolon) {
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
            Token::Defstruct => return self.parse_struct_def(),
            Token::Defrecord => return self.parse_record_def(),
            Token::Deferror => return self.parse_deferror_def(),
            _ => {}
        }

        let expr = self.parse_expr()?;
        Ok(expr)
    }

    // ── Expression (entry point — handles binding at top level) ──

    fn parse_expr(&mut self) -> Result<Ast, ParseError> {
        self.parse_binop_expr(0)
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
            Token::Slash => Some((5, BinOp::Div)),
            Token::Percent => Some((5, BinOp::Mod)),
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
                            "Unary minus is only supported on numeric literals",
                            Span {
                                start: sp.start,
                                end,
                            },
                        ))
                    }
                }
            }

            // List literal: [expr, ...]
            Token::LBrack => {
                self.advance();
                self.skip_newlines();
                let mut elems = Vec::new();
                if !matches!(self.peek(), Token::RBrack) {
                    elems.push(self.parse_expr()?);
                    while matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RBrack) {
                            break;
                        }
                        elems.push(self.parse_expr()?);
                    }
                }
                self.skip_newlines();
                let end_span = self.expect(&Token::RBrack)?;
                Ok(Ast::List(
                    Span {
                        start: sp.start,
                        end: end_span.end,
                    },
                    elems,
                ))
            }

            // Parenthesized expression
            Token::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(inner)
            }

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
                    let val = self.parse_expr()?;
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
                    args.push(self.parse_expr()?);
                    while matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                        args.push(self.parse_expr()?);
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

        // Annotated binding: name: Type = expr
        if matches!(self.peek(), Token::Colon) {
            self.advance();
            let ty = self.parse_type()?;
            self.expect(&Token::Bind)?;
            let rhs = self.parse_expr()?;
            let span = Span {
                start: name_span.start,
                end: rhs.span().end,
            };
            let pat = AstPattern::Annotated(name_span, name, ty);
            return Ok(Ast::Bind(span, pat, Box::new(rhs)));
        }

        // Simple binding: name = expr
        if matches!(self.peek(), Token::Bind) {
            self.advance();
            let rhs = self.parse_expr()?;
            let span = Span {
                start: name_span.start,
                end: rhs.span().end,
            };
            let pat = AstPattern::Var(name_span, name);
            return Ok(Ast::Bind(span, pat, Box::new(rhs)));
        }

        // Just a variable
        Ok(Ast::Var(name_span, name))
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
                let val = self.parse_expr()?;
                return Ok(RecordLitArg::Named(name, val));
            }
            // Not named, restore and parse as expression
            self.pos = save;
        }
        let expr = self.parse_expr()?;
        Ok(RecordLitArg::Positional(expr))
    }

    // ── Type annotation parsing ──

    fn parse_type(&mut self) -> Result<AstTy, ParseError> {
        let sp = self.peek_span();

        // [Type] — list type
        if matches!(self.peek(), Token::LBrack) {
            self.advance();
            let inner = self.parse_type()?;
            let end = self.expect(&Token::RBrack)?;
            return Ok(AstTy::ListOf(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                Box::new(inner),
            ));
        }

        // Named type, possibly with type args: Result<Int> or Result<Int, Error>
        let (name, _) = self.expect_ident()?;

        // Check for type parameters: Name<T> or Name<T, E>
        if matches!(self.peek(), Token::Lt) {
            self.advance();
            let first = self.parse_type()?;
            let second = if matches!(self.peek(), Token::Comma) {
                self.advance();
                Some(Box::new(self.parse_type()?))
            } else {
                None
            };
            let end = self.expect(&Token::Gt)?;
            let span = Span {
                start: sp.start,
                end: end.end,
            };

            if name == "Result" {
                return Ok(AstTy::ResultOf(span, Box::new(first), second));
            }
            // Generic named type — for now just treat as Named
            return Ok(AstTy::Named(span, name));
        }

        Ok(AstTy::Named(sp, name))
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
                // Constructor pattern: Ok(var) / Err(var)
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let inner = if matches!(self.peek(), Token::RParen) {
                        None
                    } else {
                        let (inner_name, _) = self.expect_ident()?;
                        Some(inner_name)
                    };
                    self.expect(&Token::RParen)?;
                    Ok(AstMatchPattern::Constructor(sp, name, inner))
                } else {
                    // Bare identifier as constructor without parens (e.g. error value)
                    Ok(AstMatchPattern::Constructor(sp, name, None))
                }
            }
            Token::Eof => Err(ParseError::incomplete("match pattern", sp)),
            _ => Err(ParseError::syntax(
                format!("Expected match pattern, got {:?}", self.peek()),
                sp,
            )),
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
            while i < chars.len() {
                let c = chars[i];
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

fn shift_ast_ty(ty: AstTy, delta: usize) -> AstTy {
    match ty {
        AstTy::Named(span, name) => AstTy::Named(shift_span(span, delta), name),
        AstTy::ListOf(span, inner) => {
            AstTy::ListOf(shift_span(span, delta), Box::new(shift_ast_ty(*inner, delta)))
        }
        AstTy::ResultOf(span, ok, err) => AstTy::ResultOf(
            shift_span(span, delta),
            Box::new(shift_ast_ty(*ok, delta)),
            err.map(|e| Box::new(shift_ast_ty(*e, delta))),
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
    }
}

fn shift_match_pattern(pat: AstMatchPattern, delta: usize) -> AstMatchPattern {
    match pat {
        AstMatchPattern::Wildcard(span) => AstMatchPattern::Wildcard(shift_span(span, delta)),
        AstMatchPattern::BoolLit(span, b) => AstMatchPattern::BoolLit(shift_span(span, delta), b),
        AstMatchPattern::IntLit(span, n) => AstMatchPattern::IntLit(shift_span(span, delta), n),
        AstMatchPattern::StrLit(span, s) => AstMatchPattern::StrLit(shift_span(span, delta), s),
        AstMatchPattern::Constructor(span, ctor, inner) => {
            AstMatchPattern::Constructor(shift_span(span, delta), ctor, inner)
        }
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
            args.into_iter().map(|a| shift_ast_span(a, delta)).collect(),
        ),
        Ast::Block(span, stmts) => Ast::Block(
            shift_span(span, delta),
            stmts.into_iter().map(|s| shift_ast_span(s, delta)).collect(),
        ),
        Ast::Bind(span, pat, rhs) => Ast::Bind(
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
        Ast::List(span, elems) => Ast::List(
            shift_span(span, delta),
            elems.into_iter().map(|e| shift_ast_span(e, delta)).collect(),
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
        Ast::Semi(span, inner) => Ast::Semi(shift_span(span, delta), Box::new(shift_ast_span(*inner, delta))),
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
            | Ast::BinOp(s, _, _, _)
            | Ast::List(s, _)
            | Ast::InterpolatedStr(s, _)
            | Ast::Match(s, _, _)
            | Ast::FieldAccess(s, _, _)
            | Ast::StructDef(s, _, _)
            | Ast::RecordDef(s, _, _)
            | Ast::StructLit(s, _, _)
            | Ast::ConstructorCall(s, _, _)
            | Ast::DeferrorDef(s, _, _, _)
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
    fn test_function_call() {
        let ast = parse("print(to_string(num))").unwrap();
        match &ast[0] {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "print"));
                assert_eq!(args.len(), 1);
            }
            _ => panic!("Expected App"),
        }
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
                Ast::List(_, elems) => assert_eq!(elems.len(), 3),
                _ => panic!("Expected List"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_empty_list_with_annotation() {
        let ast = parse("empty: [Int] = []").unwrap();
        match &ast[0] {
            Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::ListOf(_, inner)), rhs) => {
                assert!(matches!(inner.as_ref(), AstTy::Named(_, ref n) if n == "Int"));
                assert!(matches!(rhs.as_ref(), Ast::List(_, elems) if elems.is_empty()));
            }
            _ => panic!("Expected annotated Bind with empty List"),
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
    fn test_multiline() {
        let ast = parse("x = 1\ny = 2\nprint(to_string(x))").unwrap();
        assert_eq!(ast.len(), 3);
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
                    assert!(
                        matches!(parts.get(1), Some(InterpolatedPart::Expr(expr))
                            if matches!(expr.as_ref(), Ast::Var(_, name) if name == "name"))
                    );
                    assert!(matches!(parts.get(2), Some(InterpolatedPart::Text(s)) if s == "!"));
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
}
