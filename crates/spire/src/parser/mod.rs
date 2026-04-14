use crate::ast::*;
use crate::error::ParseError;
use crate::lexer::tokenize;
use crate::token::{Spanned, Token};

mod chumsky_program;
mod completion;
mod context;
mod diagnostic;
mod error_map;
mod interpolate;
mod pattern;
mod syntax_token;
mod ty;
mod validate;

pub use completion::{
    parse_incomplete_expr, parse_incomplete_stmt, CompletionContext, IncompleteParseResult,
};
use context::{DeclLevel, ParseUnitKind, TopLevelDeclKind};
pub use context::{ParseRules, ParserContext};
pub use diagnostic::{
    LspDiagnostic, LspDiagnosticSeverity, LspPosition, LspRange, LspRelatedInformation,
    ParseDiagnostic,
};

#[derive(Debug, Clone, PartialEq)]
pub struct EntryAnnotation {
    pub name: String,
    pub span: Span,
}

/// Parse Surtr source text into an abstract syntax tree.
pub fn parse(source: &str) -> Result<Vec<Ast>, ParseError> {
    parse_with_context(source, ParserContext::default())
}

/// Parse Surtr source text with explicit compile-unit context.
pub fn parse_with_context(source: &str, context: ParserContext) -> Result<Vec<Ast>, ParseError> {
    let tokens = tokenize(source)?;
    chumsky_program::parse_program_with_chumsky(&tokens, context)
}

/// Parse Surtr source with parser diagnostic metadata for editor tooling.
pub fn parse_with_context_diagnostic(
    source: &str,
    context: ParserContext,
) -> Result<Vec<Ast>, ParseDiagnostic> {
    let tokens = tokenize(source).map_err(ParseDiagnostic::from)?;
    chumsky_program::parse_program_with_chumsky_diagnostic(&tokens, context)
        .map_err(ParseDiagnostic::from)
}

/// Strip `@@test <expr>` annotations while preserving source span offsets.
pub fn strip_test_annotations(source: &str) -> String {
    let tokens = match tokenize(source) {
        Ok(tokens) => tokens,
        Err(_) => return source.to_string(),
    };

    let mut chars = source.chars().collect::<Vec<_>>();
    let mut i = 0usize;
    while i < tokens.len() {
        if let Token::Annotator(name) = &tokens[i].token {
            if name == "test" {
                let mut j = i + 1;
                while j < tokens.len() && !matches!(tokens[j].token, Token::Newline | Token::Eof) {
                    j += 1;
                }
                let end = if j > i + 1 {
                    tokens[j - 1].span.end
                } else {
                    tokens[i].span.end
                };
                for ch in chars.iter_mut().take(end).skip(tokens[i].span.start) {
                    if *ch != '\n' {
                        *ch = ' ';
                    }
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }

    chars.into_iter().collect::<String>()
}

/// Collect `@@entrypoint` annotations and return source with annotation tokens stripped.
pub fn collect_entrypoint_annotations(
    source: &str,
) -> Result<(String, Vec<EntryAnnotation>), ParseError> {
    let tokens = tokenize(source)?;
    let mut chars = source.chars().collect::<Vec<_>>();
    let mut annotations = Vec::new();

    let mut i = 0usize;
    while i < tokens.len() {
        let token = &tokens[i];
        if let Token::Annotator(name) = &token.token {
            if name == "entrypoint" {
                for ch in chars.iter_mut().take(token.span.end).skip(token.span.start) {
                    if *ch != '\n' {
                        *ch = ' ';
                    }
                }
                let mut j = i + 1;
                while j < tokens.len() && matches!(tokens[j].token, Token::Newline) {
                    j += 1;
                }
                if j >= tokens.len() || !matches!(tokens[j].token, Token::Def) {
                    return Err(ParseError::syntax(
                        "@@entrypoint must annotate a function definition (`def`)",
                        token.span.clone(),
                    ));
                }
                let mut k = j + 1;
                while k < tokens.len() && matches!(tokens[k].token, Token::Newline) {
                    k += 1;
                }
                let def_name = match tokens.get(k).map(|sp| &sp.token) {
                    Some(Token::Ident(name)) => name.clone(),
                    _ => {
                        return Err(ParseError::syntax(
                            "@@entrypoint must target `def <name>(...)`",
                            tokens[j].span.clone(),
                        ));
                    }
                };
                annotations.push(EntryAnnotation {
                    name: def_name,
                    span: token.span.clone(),
                });
            }
        }
        i += 1;
    }

    Ok((chars.into_iter().collect::<String>(), annotations))
}

struct Parser {
    tokens: Vec<Spanned<Token>>,
    pos: usize,
    context: ParserContext,
    impl_target_stack: Vec<Symbol>,
    allow_trailing_call_block: bool,
}

impl Parser {
    fn new(tokens: Vec<Spanned<Token>>, context: ParserContext) -> Self {
        Self {
            tokens,
            pos: 0,
            context,
            impl_target_stack: Vec::new(),
            allow_trailing_call_block: true,
        }
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

    fn expect_type_gt(&mut self) -> Result<Span, ParseError> {
        let sp = self.peek_span();
        match self.peek() {
            Token::Gt => {
                self.advance();
                Ok(sp)
            }
            Token::Compose => {
                let composed = self.advance().span.clone();
                let first = Span {
                    start: composed.start,
                    end: composed.start + 1,
                };
                let second = Span {
                    start: composed.start + 1,
                    end: composed.end,
                };
                self.tokens.insert(
                    self.pos,
                    Spanned {
                        token: Token::Gt,
                        span: second,
                    },
                );
                Ok(first)
            }
            Token::Eof => Err(ParseError::incomplete(">", sp)),
            other => Err(ParseError::syntax(
                format!("Expected Gt, got {:?}", other),
                sp,
            )),
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

    fn expect_builtin_decl_name(&mut self) -> Result<(Symbol, Span), ParseError> {
        self.expect_ident()
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

    fn has_path_separator(&self) -> bool {
        matches!(self.peek(), Token::Colon) && matches!(self.peek_n(1), Some(Token::Colon))
    }

    fn consume_path_separator(&mut self) -> Result<Span, ParseError> {
        if !self.has_path_separator() {
            return Err(ParseError::syntax("Expected `::`", self.peek_span()));
        }
        let start = self.peek_span().start;
        self.advance();
        let end = self.peek_span().end;
        self.advance();
        Ok(Span { start, end })
    }

    // ── Statement ──

    fn parse_stmt(&mut self) -> Result<Ast, ParseError> {
        self.skip_newlines();

        if self.context.level == DeclLevel::Expr
            && matches!(
                self.peek(),
                Token::Annotator(_)
                    | Token::Def
                    | Token::Defp
                    | Token::Defmod
                    | Token::Deftrait
                    | Token::Impl
                    | Token::Import
                    | Token::Defstruct
                    | Token::Defrecord
                    | Token::Deferror
                    | Token::Defenum
                    | Token::Defextractor
            )
        {
            return Err(ParseError::syntax(
                "Declarations are only allowed at the top level",
                self.peek_span(),
            ));
        }

        // Data definitions
        let stmt = match self.peek() {
            Token::Annotator(_) => self.parse_annotated_decl()?,
            Token::Def | Token::Defp => self.parse_def()?,
            Token::Defmod => self.parse_defmod()?,
            Token::Deftrait => self.parse_trait_def()?,
            Token::Impl => self.parse_impl_def()?,
            Token::Import => self.parse_import()?,
            Token::Defstruct => self.parse_struct_def()?,
            Token::Defrecord => self.parse_record_def()?,
            Token::Deferror => self.parse_deferror_def()?,
            Token::Defenum => self.parse_enum_def()?,
            Token::Defextractor => self.parse_extractor_def()?,
            _ => {
                if self.is_pattern_bind_stmt_start() {
                    let save = self.pos;
                    match self.parse_pattern_bind_stmt() {
                        Ok(stmt) => {
                            if matches!(self.peek(), Token::Semicolon) {
                                let semi = self.advance().span.clone();
                                let span = Span {
                                    start: stmt.span().start,
                                    end: semi.end,
                                };
                                let wrapped = Ast::Semi(span, Box::new(stmt));
                                self.validate_stmt_by_context(&wrapped)?;
                                return Ok(wrapped);
                            }
                            self.validate_stmt_by_context(&stmt)?;
                            return Ok(stmt);
                        }
                        Err(err) => {
                            let looks_like_bind = matches!(
                                self.tokens.get(save).map(|sp| &sp.token),
                                Some(Token::LParen | Token::LBrack)
                            ) && self
                                .stmt_has_top_level_assignment_from(save);
                            self.pos = save;
                            if looks_like_bind {
                                return Err(err);
                            }
                        }
                    }
                }

                let expr = self.parse_expr()?;
                if matches!(self.peek(), Token::Semicolon) {
                    let semi = self.advance().span.clone();
                    let span = Span {
                        start: expr.span().start,
                        end: semi.end,
                    };
                    Ast::Semi(span, Box::new(expr))
                } else {
                    expr
                }
            }
        };

        self.validate_stmt_by_context(&stmt)?;

        Ok(stmt)
    }

    fn validate_stmt_by_context(&self, stmt: &Ast) -> Result<(), ParseError> {
        validate::validate_stmt_by_context(&self.context, stmt)
    }

    fn parse_module_body_stmts(
        &mut self,
        module_path: Option<String>,
    ) -> Result<Vec<Ast>, ParseError> {
        let prev_context = self.context.clone();
        self.context.level = DeclLevel::Top;
        self.context.unit_kind = ParseUnitKind::Module;
        self.context.module_path = module_path;
        self.context.parse_rules = if prev_context
            .parse_rules
            .allowed_top_level_decl_kinds
            .allows(TopLevelDeclKind::BuiltinDecl)
        {
            ParseRules::std_module_member()
        } else {
            ParseRules::module_member()
        };

        let result = (|| {
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
        })();

        self.context = prev_context;
        result
    }

    fn parse_field_visibility(&mut self) -> Visibility {
        if matches!(self.peek(), Token::Private) {
            self.advance();
            self.skip_newlines();
            Visibility::Private
        } else {
            Visibility::Public
        }
    }

    fn parse_import_selector_list(&mut self) -> Result<(Vec<Symbol>, Span), ParseError> {
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut names = Vec::new();
        loop {
            if matches!(self.peek(), Token::RBrace) {
                break;
            }
            let (name, _span) = self.expect_ident()?;
            names.push(name);
            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::RBrace) {
                    break;
                }
                continue;
            }
            break;
        }

        if names.is_empty() {
            return Err(ParseError::syntax(
                "Import list requires at least one symbol",
                self.peek_span(),
            ));
        }

        let end = self.expect(&Token::RBrace)?;
        Ok((names, end))
    }

    fn parse_import(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Import)?;
        let (first_seg, first_span) = self.expect_ident()?;
        let path_start = first_span.start;
        let mut qualified = vec![(first_seg, first_span)];
        let mut saw_separator = false;

        while self.has_path_separator() && matches!(self.peek_n(2), Some(Token::Ident(_))) {
            saw_separator = true;
            self.consume_path_separator()?;
            let (seg, seg_span) = self.expect_ident()?;
            qualified.push((seg, seg_span));
        }

        let (module_segments, module_end, spec, mut stmt_end) =
            if self.has_path_separator() && matches!(self.peek_n(2), Some(Token::LBrace)) {
                self.consume_path_separator()?;
                let (names, end) = self.parse_import_selector_list()?;
                (
                    qualified.iter().map(|(name, _)| name.clone()).collect(),
                    qualified.last().expect("non-empty path").1.end,
                    ImportSpec::List(names),
                    end.end,
                )
            } else if self.has_path_separator() {
                return Err(ParseError::syntax(
                    "Expected identifier or `{` after `::` in import",
                    self.peek_span(),
                ));
            } else if saw_separator {
                let (name, selected_span) = qualified
                    .pop()
                    .expect("qualified import with separator has at least 2 segments");
                (
                    qualified.iter().map(|(module, _)| module.clone()).collect(),
                    qualified.last().expect("module path is non-empty").1.end,
                    ImportSpec::Single(name),
                    selected_span.end,
                )
            } else {
                (
                    qualified.iter().map(|(name, _)| name.clone()).collect(),
                    qualified.last().expect("non-empty path").1.end,
                    ImportSpec::All,
                    qualified.last().expect("non-empty path").1.end,
                )
            };

        if matches!(self.peek(), Token::Semicolon) {
            stmt_end = self.advance().span.end;
        }

        let path = AstPath {
            span: Span {
                start: path_start,
                end: module_end,
            },
            segments: module_segments,
        };

        Ok(Ast::Import(
            Span {
                start: sp.start,
                end: stmt_end,
            },
            path,
            spec,
        ))
    }

    fn parse_defmod(&mut self) -> Result<Ast, ParseError> {
        self.parse_defmod_with_attrs(DeclAttrs::default(), None)
    }

    fn parse_trait_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_trait_def_with_attrs(DeclAttrs::default(), None)
    }

    fn parse_trait_impl_head(&mut self) -> Result<(Symbol, Vec<AstTy>), ParseError> {
        let (trait_name, _) = self.expect_ident()?;
        let trait_args = if matches!(self.peek(), Token::Lt) {
            self.advance();
            self.skip_newlines();
            let mut args = Vec::new();
            if !matches!(self.peek(), Token::Gt) {
                loop {
                    args.push(self.parse_type_in_impl_context(None)?);
                    self.skip_newlines();
                    if matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        continue;
                    }
                    break;
                }
            }
            self.expect_type_gt()?;
            args
        } else {
            Vec::new()
        };
        Ok((trait_name, trait_args))
    }

    fn parse_impl_def(&mut self) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Impl)?;
        let (head, trait_args) = self.parse_trait_impl_head()?;
        self.skip_newlines();

        if matches!(self.peek(), Token::For) {
            self.advance();
            self.skip_newlines();
            let target_ty = self.parse_type_in_impl_context(None)?;
            let self_target = self.trait_impl_self_target_name(&target_ty)?;
            self.skip_newlines();
            self.expect(&Token::LBrace)?;
            self.skip_newlines();

            let mut methods = Vec::new();
            while !matches!(self.peek(), Token::RBrace) {
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete("}", self.peek_span()));
                }
                if !matches!(self.peek(), Token::Def) {
                    return Err(ParseError::syntax(
                        "trait impl body may only contain `def` declarations",
                        self.peek_span(),
                    ));
                }
                let method = self.parse_impl_method(&self_target)?;
                self.ensure_stmt_boundary(&method, true)?;
                methods.push(method);
                while matches!(self.peek(), Token::Newline) {
                    self.advance();
                }
            }

            let end = self.expect(&Token::RBrace)?;
            return Ok(Ast::TraitImplDef(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                head,
                trait_args,
                target_ty,
                methods,
            ));
        }

        if !trait_args.is_empty() {
            return Err(ParseError::syntax(
                "Plain `impl Type { ... }` does not accept trait-style type arguments",
                self.peek_span(),
            ));
        }

        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut methods = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            if !matches!(self.peek(), Token::Def | Token::Defp) {
                return Err(ParseError::syntax(
                    "impl body may only contain `def` / `defp` declarations",
                    self.peek_span(),
                ));
            }
            let method = self.parse_impl_method(&head)?;
            self.ensure_stmt_boundary(&method, true)?;
            methods.push(method);
            while matches!(self.peek(), Token::Newline) {
                self.advance();
            }
        }

        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::ImplDef(
            Span {
                start: sp.start,
                end: end.end,
            },
            head,
            methods,
        ))
    }

    fn parse_impl_method(&mut self, target: &str) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        let visibility = match self.peek() {
            Token::Def => {
                self.advance();
                Visibility::Public
            }
            Token::Defp => {
                self.advance();
                Visibility::Private
            }
            _ => {
                return Err(ParseError::syntax(
                    "Expected `def` or `defp`",
                    self.peek_span(),
                ));
            }
        };
        let (name, _) = self.expect_ident()?;
        let mut params = Vec::new();

        if matches!(self.peek(), Token::Unit) {
            self.advance();
        } else {
            self.expect(&Token::LParen)?;
            self.skip_newlines();
            let mut first_param = true;
            if !matches!(self.peek(), Token::RParen) {
                loop {
                    if matches!(self.peek(), Token::Eof) {
                        return Err(ParseError::incomplete(")", self.peek_span()));
                    }
                    self.skip_newlines();
                    let (param_name, param_span) = self.expect_ident()?;

                    let param_ty = if param_name == "self" {
                        if !first_param {
                            return Err(ParseError::syntax(
                                "`self` is only allowed as the first parameter of impl methods",
                                param_span,
                            ));
                        }
                        if matches!(self.peek(), Token::Colon) {
                            self.advance();
                            self.skip_newlines();
                            let ty = self.parse_type_in_impl_context(Some(target.to_string()))?;
                            if !Self::is_self_type(&ty) {
                                return Err(ParseError::syntax(
                                    "`self` receiver type must be `Self`",
                                    ast_ty_span(&ty).clone(),
                                ));
                            }
                            ty
                        } else {
                            AstTy::Named(param_span.clone(), "Self".to_string())
                        }
                    } else {
                        self.expect(&Token::Colon)?;
                        self.skip_newlines();
                        self.parse_type_in_impl_context(Some(target.to_string()))?
                    };

                    params.push(FunParam {
                        name: param_name,
                        ty: param_ty,
                        span: param_span,
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
                    first_param = false;
                }
            }
            self.expect(&Token::RParen)?;
        }

        let ret_ty = if matches!(self.peek(), Token::Arrow) {
            self.advance();
            self.skip_newlines();
            Some(self.parse_type_in_impl_context(Some(target.to_string()))?)
        } else {
            None
        };

        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.impl_target_stack.push(target.to_string());
        let body_stmts = self.parse_block_stmts();
        self.impl_target_stack.pop();
        let body_stmts = body_stmts?;
        if body_stmts.is_empty() {
            return Err(ParseError::syntax(
                "Function body must not be empty",
                self.peek_span(),
            ));
        }
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
            Vec::new(),
            params,
            ret_ty,
            Box::new(body),
            DeclAttrs {
                visibility,
                ..DeclAttrs::default()
            },
        ))
    }

    fn trait_impl_self_target_name(&self, ty: &AstTy) -> Result<String, ParseError> {
        match ty {
            AstTy::Named(_, name) => Ok(name.clone()),
            AstTy::Generic(_, name, args) => {
                if args.is_empty() {
                    Ok(name.clone())
                } else {
                    Err(ParseError::syntax(
                        "trait impl target must be a concrete named type in V1",
                        ast_ty_span(ty).clone(),
                    ))
                }
            }
            _ => Err(ParseError::syntax(
                "trait impl target must be a concrete named type in V1",
                ast_ty_span(ty).clone(),
            )),
        }
    }

    fn parse_defmod_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        if self.context.module_path.is_some() {
            return Err(ParseError::syntax(
                "Nested module declarations are not allowed",
                sp,
            ));
        }
        self.expect(&Token::Defmod)?;
        let (name, _) = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body = self.parse_module_body_stmts(Some(name.clone()))?;
        let end = self.expect(&Token::RBrace)?;

        Ok(Ast::Defmod(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            body,
            attrs,
        ))
    }

    fn parse_trait_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Deftrait)?;
        let (name, _) = self.expect_ident()?;
        let type_params = self.parse_decl_type_params()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut methods = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            if !matches!(self.peek(), Token::Def) {
                return Err(ParseError::syntax(
                    "trait body may only contain `def` signatures",
                    self.peek_span(),
                ));
            }
            let method = self.parse_trait_method_sig()?;
            methods.push(method);
            while matches!(self.peek(), Token::Newline) {
                self.advance();
            }
        }

        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::TraitDef(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            type_params,
            methods,
            attrs,
        ))
    }

    fn parse_trait_method_sig(&mut self) -> Result<TraitMethodSig, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Def)?;
        let (name, _) = self.expect_ident()?;
        let type_params = self.parse_decl_type_params()?;
        let mut params = Vec::new();
        let self_context = Some("Self".to_string());

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
                    params.push(
                        self.parse_trait_method_param(params.is_empty(), self_context.clone())?,
                    );
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

        self.skip_newlines();
        self.expect(&Token::Arrow)?;
        self.skip_newlines();
        let ret_ty = self.parse_type_in_impl_context(self_context)?;
        if matches!(ret_ty, AstTy::ImplTrait(_, _)) {
            return Err(ParseError::syntax(
                "return-position `impl Trait` is not supported; name the type parameter explicitly",
                ast_ty_span(&ret_ty).clone(),
            ));
        }
        self.reject_where_clause()?;

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
                "trait method declarations must not have a body",
                self.tokens[lookahead].span.clone(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            sp.end
        };

        Ok(TraitMethodSig {
            name,
            type_params,
            params,
            ret_ty,
            span: Span {
                start: sp.start,
                end,
            },
        })
    }

    fn parse_trait_method_param(
        &mut self,
        is_first_param: bool,
        self_context: Option<String>,
    ) -> Result<FunParam, ParseError> {
        let (name, span) = self.expect_ident()?;
        if name == "self" {
            if !is_first_param {
                return Err(ParseError::syntax(
                    "`self` is only allowed as the first parameter of trait methods",
                    span,
                ));
            }

            let ty = if matches!(self.peek(), Token::Colon) {
                self.advance();
                self.skip_newlines();
                let ty = self.parse_type_in_impl_context(self_context)?;
                if !Self::is_self_type(&ty) {
                    return Err(ParseError::syntax(
                        "`self` receiver type must be `Self`",
                        ast_ty_span(&ty).clone(),
                    ));
                }
                ty
            } else {
                AstTy::Named(span.clone(), "Self".to_string())
            };
            return Ok(FunParam { name, ty, span });
        }

        self.expect(&Token::Colon)?;
        let ty = self.parse_type_in_impl_context(self_context)?;
        Ok(FunParam { name, ty, span })
    }

    fn parse_block_stmts(&mut self) -> Result<Vec<Ast>, ParseError> {
        let prev_level = self.context.level;
        self.context.level = DeclLevel::Expr;
        let result = (|| {
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
        })();
        self.context.level = prev_level;
        result
    }

    // ── Expression (entry point — handles binding at top level) ──

    fn parse_expr(&mut self) -> Result<Ast, ParseError> {
        self.parse_flow_expr()
    }

    fn parse_flow_expr(&mut self) -> Result<Ast, ParseError> {
        let mut left = self.parse_logical_expr()?;
        loop {
            let next = match self.peek() {
                Token::PipeApply => 0,
                Token::PipeMap => 1,
                Token::PipeBind => 2,
                Token::Compose => 3,
                Token::PipeCompose => 4,
                _ => break,
            };
            self.advance();
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

    fn stmt_has_top_level_assignment_from(&self, start: usize) -> bool {
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

    fn expr_binop(tok: &Token) -> Option<BinOp> {
        match tok {
            Token::Plus => Some(BinOp::Add),
            Token::Minus => Some(BinOp::Sub),
            Token::Star => Some(BinOp::Mul),
            Token::Concat => Some(BinOp::Concat),
            _ => None,
        }
    }

    fn logical_binop(tok: &Token) -> Option<BinOp> {
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

    fn expr_binop_from_func_literal(body: &str) -> Option<BinOp> {
        match body {
            "+" => Some(BinOp::Add),
            "-" => Some(BinOp::Sub),
            "*" => Some(BinOp::Mul),
            "++" => Some(BinOp::Concat),
            _ => None,
        }
    }

    fn logical_binop_from_func_literal(body: &str) -> Option<BinOp> {
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

    fn lower_binop(left: Ast, op: BinOp, right: Ast) -> Ast {
        let span = Span {
            start: left.span().start,
            end: right.span().end,
        };
        Ast::BinOp(span, op, Box::new(left), Box::new(right))
    }

    fn lower_func_literal_call(left: Ast, func_span: Span, name: Symbol, right: Ast) -> Ast {
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

    fn parse_expr_class_expr(&mut self) -> Result<Ast, ParseError> {
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

    fn parse_logical_expr(&mut self) -> Result<Ast, ParseError> {
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

    fn parse_postfix(&mut self) -> Result<Ast, ParseError> {
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
    fn parse_ident_continuation(
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
    fn ensure_non_associative_assignment(&self, rhs: &Ast) -> Result<(), ParseError> {
        if matches!(rhs, Ast::Bind(_, _, _) | Ast::SafeBind(_, _, _)) {
            return Err(ParseError::syntax(
                "`=` and `=?` are non-associative; a statement can contain only one assignment operator",
                rhs.span().clone(),
            ));
        }
        Ok(())
    }

    fn with_trailing_call_block_disabled<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        let prev = self.allow_trailing_call_block;
        self.allow_trailing_call_block = false;
        let result = f(self);
        self.allow_trailing_call_block = prev;
        result
    }

    fn parse_call_args(&mut self) -> Result<Vec<RecordLitArg>, ParseError> {
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

    fn parse_trailing_block_expr_from_lbrace(&mut self, sp: Span) -> Result<Ast, ParseError> {
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

    fn reject_constructor_trailing_block(&self) -> Result<(), ParseError> {
        if self.allow_trailing_call_block && matches!(self.peek(), Token::LBrace) {
            return Err(ParseError::syntax(
                "Trailing block sugar is not supported for constructor calls",
                self.peek_span(),
            ));
        }
        Ok(())
    }

    fn trailing_block_uses_closure_sugar(callee: &Ast) -> bool {
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

    fn attach_trailing_block_arg(
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
    fn parse_record_lit_arg(&mut self) -> Result<RecordLitArg, ParseError> {
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

    // ── Type annotation parsing ──

    fn parse_closure_literal(&mut self, sp: Span) -> Result<Ast, ParseError> {
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

    fn parse_capture_expr(&mut self, sp: Span) -> Result<Ast, ParseError> {
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
            let visibility = self.parse_field_visibility();
            let (fname, fspan) = self.expect_ident()?;
            self.expect(&Token::Colon)?;
            let fty = self.parse_type()?;
            fields.push(StructField {
                name: fname,
                ty: fty,
                span: fspan,
                visibility,
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
                let visibility = self.parse_field_visibility();
                let (fname, fspan) = self.expect_ident()?;
                self.expect(&Token::Colon)?;
                let fty = self.parse_type()?;
                fields.push(RecordField {
                    name: fname,
                    ty: fty,
                    span: fspan,
                    visibility,
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

    fn parse_enum_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_enum_def_with_attrs(DeclAttrs::default(), None)
    }

    fn parse_enum_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        start_override: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Defenum)?;
        let (name, _name_span) = self.expect_ident()?;
        let type_params = self.parse_decl_type_params()?;
        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        self.skip_newlines();

        let mut variants = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete("}", self.peek_span()));
            }
            self.skip_newlines();
            let variant_start = self.peek_span().start;
            let (variant_name, _) = self.expect_ident()?;
            let mut payload = Vec::new();

            if matches!(self.peek(), Token::LParen) {
                self.advance();
                self.skip_newlines();
                if !matches!(self.peek(), Token::RParen) {
                    payload.push(self.parse_type()?);
                    self.skip_newlines();
                    while matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                        payload.push(self.parse_type()?);
                        self.skip_newlines();
                    }
                }
                self.expect(&Token::RParen)?;
            }

            let discriminant = if matches!(self.peek(), Token::Bind) {
                self.advance();
                self.skip_newlines();
                Some(self.parse_enum_discriminant()?)
            } else {
                None
            };

            let variant_end = if self.pos > 0 {
                self.tokens[self.pos - 1].span.end
            } else {
                variant_start
            };
            variants.push(EnumVariant {
                name: variant_name,
                payload,
                discriminant,
                span: Span {
                    start: variant_start,
                    end: variant_end,
                },
            });

            self.skip_newlines();
            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                continue;
            }
        }

        if variants.is_empty() {
            return Err(ParseError::syntax(
                "Enum definition requires at least one variant",
                Span {
                    start: sp.start,
                    end: sp.end,
                },
            ));
        }

        let end = self.expect(&Token::RBrace)?;
        Ok(Ast::EnumDef(
            Span {
                start: start_override.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            type_params
                .into_iter()
                .map(|param| TypeParam {
                    name: param.name,
                    bound: param.bound,
                    span: param.span,
                })
                .collect(),
            variants,
            attrs,
        ))
    }

    fn parse_decl_type_params(&mut self) -> Result<Vec<TypeParam>, ParseError> {
        if !matches!(self.peek(), Token::Lt) {
            return Ok(Vec::new());
        }

        self.advance();
        self.skip_newlines();

        let mut params = Vec::new();
        loop {
            let param_span = self.peek_span();
            self.expect(&Token::Dollar)?;
            let (param_name, _) = self.expect_ident()?;
            let bound = if matches!(self.peek(), Token::Colon) {
                self.advance();
                self.skip_newlines();
                let (bound_name, _) = self.expect_ident()?;
                Some(bound_name)
            } else {
                None
            };
            params.push(TypeParam {
                name: format!("${}", param_name),
                bound,
                span: param_span,
            });
            self.skip_newlines();

            if matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                continue;
            }

            if matches!(self.peek(), Token::Gt) {
                self.expect(&Token::Gt)?;
                break;
            }

            if matches!(self.peek(), Token::Eof) {
                return Err(ParseError::incomplete(">", self.peek_span()));
            }

            return Err(ParseError::syntax(
                "Expected `,` or `>` in declaration type parameter list",
                self.peek_span(),
            ));
        }

        Ok(params)
    }

    fn parse_enum_discriminant(&mut self) -> Result<sindr::primitives::SurtrInt, ParseError> {
        let span = self.peek_span();
        if matches!(self.peek(), Token::Minus) {
            self.advance();
            let int_span = self.peek_span();
            let Token::Int(n) = self.peek().clone() else {
                return Err(ParseError::syntax(
                    "Expected integer literal after '-' in enum discriminant",
                    int_span,
                ));
            };
            self.advance();
            return Ok(-n);
        }
        match self.peek().clone() {
            Token::Int(n) => {
                self.advance();
                Ok(n)
            }
            Token::Eof => Err(ParseError::incomplete("integer literal", span)),
            _ => Err(ParseError::syntax(
                "Enum discriminant must be an integer literal",
                span,
            )),
        }
    }

    /// `deferror Name { expr }` or `deferror Name(fields) { expr }`
    fn parse_deferror_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_deferror_def_with_attrs(DeclAttrs::default(), None)
    }

    fn parse_deferror_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        start_override: Option<usize>,
    ) -> Result<Ast, ParseError> {
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
                    let visibility = self.parse_field_visibility();
                    let (fname, fspan) = self.expect_ident()?;
                    self.expect(&Token::Colon)?;
                    let fty = self.parse_type()?;
                    fields.push(RecordField {
                        name: fname,
                        ty: fty,
                        span: fspan,
                        visibility,
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
                start: start_override.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            fields,
            Box::new(show_expr),
            attrs,
        ))
    }

    fn parse_def_signature(
        &mut self,
    ) -> Result<
        (
            Span,
            Symbol,
            Vec<TypeParam>,
            Vec<FunParam>,
            Option<AstTy>,
            Visibility,
        ),
        ParseError,
    > {
        self.parse_def_signature_with_name_mode(false)
    }

    fn parse_def_signature_with_name_mode(
        &mut self,
        allow_builtin_keyword_name: bool,
    ) -> Result<
        (
            Span,
            Symbol,
            Vec<TypeParam>,
            Vec<FunParam>,
            Option<AstTy>,
            Visibility,
        ),
        ParseError,
    > {
        let sp = self.peek_span();
        let visibility = match self.peek() {
            Token::Def => {
                self.advance();
                Visibility::Public
            }
            Token::Defp => {
                self.advance();
                Visibility::Private
            }
            _ => {
                return Err(ParseError::syntax(
                    "Expected `def` or `defp`",
                    self.peek_span(),
                ));
            }
        };
        let (name, _) = if allow_builtin_keyword_name {
            self.expect_builtin_decl_name()?
        } else {
            self.expect_ident()?
        };
        let type_params = self.parse_decl_type_params()?;
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
            let ret_ty = self.parse_type()?;
            if matches!(ret_ty, AstTy::ImplTrait(_, _)) {
                return Err(ParseError::syntax(
                    "return-position `impl Trait` is not supported; name the type parameter explicitly",
                    ast_ty_span(&ret_ty).clone(),
                ));
            }
            Some(ret_ty)
        } else {
            None
        };

        self.reject_where_clause()?;

        Ok((sp, name, type_params, params, ret_ty, visibility))
    }

    fn parse_extractor_signature(
        &mut self,
    ) -> Result<(Span, Symbol, Vec<TypeParam>, ExtractorParam, AstTy), ParseError> {
        self.parse_extractor_signature_with_name_mode(false)
    }

    fn parse_extractor_signature_with_name_mode(
        &mut self,
        allow_builtin_keyword_name: bool,
    ) -> Result<(Span, Symbol, Vec<TypeParam>, ExtractorParam, AstTy), ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Defextractor)?;
        let (name, name_span) = if allow_builtin_keyword_name {
            self.expect_builtin_decl_name()?
        } else {
            self.expect_ident()?
        };
        let type_params = self.parse_decl_type_params()?;
        if Self::is_constructor_style_name(&name) {
            return Err(ParseError::syntax(
                format!(
                    "Extractor names must not use constructor-style names like `{}`; implement `{}`::deconstruct(...) instead",
                    name, name
                ),
                name_span,
            ));
        }
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        self.skip_newlines();
        let (param_name, param_span) = self.expect_ident()?;
        self.skip_newlines();
        let param_ty = if matches!(self.peek(), Token::Colon) {
            self.advance();
            self.skip_newlines();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.skip_newlines();
        self.expect(&Token::RParen)?;
        self.skip_newlines();
        self.expect(&Token::Arrow)?;
        self.skip_newlines();
        let ret_ty = self.parse_type()?;
        if matches!(ret_ty, AstTy::ImplTrait(_, _)) {
            return Err(ParseError::syntax(
                "return-position `impl Trait` is not supported; name the type parameter explicitly",
                ast_ty_span(&ret_ty).clone(),
            ));
        }
        self.reject_where_clause()?;
        Ok((
            sp,
            name,
            type_params,
            ExtractorParam {
                name: param_name,
                ty: param_ty,
                span: param_span,
            },
            ret_ty,
        ))
    }

    fn reject_where_clause(&self) -> Result<(), ParseError> {
        if matches!(self.peek(), Token::Where) {
            return Err(ParseError::syntax(
                "`where` clauses are staged and not implemented yet",
                self.peek_span(),
            ));
        }
        Ok(())
    }

    fn is_constructor_style_name(name: &str) -> bool {
        name.chars().next().is_some_and(|ch| ch.is_uppercase())
    }

    fn parse_annotated_decl(&mut self) -> Result<Ast, ParseError> {
        let mut attrs = DeclAttrs::default();
        let mut saw_builtin = false;
        let mut start_span: Option<Span> = None;

        while let Token::Annotator(name) = self.peek().clone() {
            let annotator_span = self.peek_span();
            if start_span.is_none() {
                start_span = Some(annotator_span.clone());
            }
            self.advance();
            self.skip_newlines();
            match name.as_str() {
                "builtin" => {
                    if saw_builtin {
                        return Err(ParseError::syntax(
                            "@@builtin may only appear once before a declaration",
                            annotator_span,
                        ));
                    }
                    saw_builtin = true;
                }
                "doc" => {
                    if attrs.doc.is_some() {
                        return Err(ParseError::syntax(
                            "@@doc may only appear once before a declaration",
                            annotator_span,
                        ));
                    }
                    let token = self.peek().clone();
                    match token {
                        Token::DocString(text) => {
                            self.advance();
                            attrs.doc = Some(text);
                        }
                        Token::Eof => {
                            return Err(ParseError::incomplete("doc string", self.peek_span()));
                        }
                        _ => {
                            return Err(ParseError::syntax(
                                "@@doc expects a triple-quoted doc string",
                                self.peek_span(),
                            ));
                        }
                    }
                }
                "autoimport" => {
                    if attrs.auto_import {
                        return Err(ParseError::syntax(
                            "@@autoimport may only appear once before a declaration",
                            annotator_span,
                        ));
                    }
                    attrs.auto_import = true;
                }
                _ => {
                    return Err(ParseError::syntax(
                        format!("Unknown annotator: @@{}", name),
                        annotator_span,
                    ));
                }
            }
            self.skip_newlines();
        }

        let start = start_span
            .map(|span| span.start)
            .unwrap_or_else(|| self.peek_span().start);

        if saw_builtin {
            match self.peek() {
                Token::Def => self.parse_builtin_decl(start, attrs),
                Token::Defextractor => self.parse_builtin_extractor_decl(start, attrs),
                Token::Type => self.parse_builtin_type_decl(start, attrs),
                _ => Err(ParseError::syntax(
                    "Expected `def`, `defextractor`, or `type` after @@builtin",
                    self.peek_span(),
                )),
            }
        } else {
            match self.peek() {
                Token::Def => self.parse_def_with_attrs(attrs, Some(start)),
                Token::Defmod => self.parse_defmod_with_attrs(attrs, Some(start)),
                Token::Deftrait => self.parse_trait_def_with_attrs(attrs, Some(start)),
                Token::Deferror => self.parse_deferror_def_with_attrs(attrs, Some(start)),
                Token::Defenum => self.parse_enum_def_with_attrs(attrs, Some(start)),
                Token::Defextractor => self.parse_extractor_def_with_attrs(attrs, Some(start)),
                Token::Eof => Err(ParseError::incomplete("declaration", self.peek_span())),
                _ => Err(ParseError::syntax(
                    "@@doc / @@autoimport must annotate `def`, `defmod`, `deftrait`, `deferror`, `defenum`, `defextractor`, or `@@builtin type/def/defextractor`",
                    self.peek_span(),
                )),
            }
        }
    }

    fn parse_builtin_decl(&mut self, start: usize, attrs: DeclAttrs) -> Result<Ast, ParseError> {
        let (_def_span, name, _type_params, params, ret_ty, _visibility) =
            self.parse_def_signature_with_name_mode(true)?;

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
            start
        };

        Ok(Ast::BuiltinDecl(
            Span { start, end },
            name,
            params,
            ret_ty,
            attrs,
        ))
    }

    fn parse_builtin_extractor_decl(
        &mut self,
        start: usize,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        let (_sp, name, _type_params, param, ret_ty) =
            self.parse_extractor_signature_with_name_mode(true)?;

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
                "@@builtin extractor declaration must not have a function body",
                self.tokens[lookahead].span.clone(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };

        Ok(Ast::BuiltinExtractorDecl(
            Span { start, end },
            name,
            param,
            ret_ty,
            attrs,
        ))
    }

    fn parse_builtin_type_decl(
        &mut self,
        start: usize,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        self.expect(&Token::Type)?;
        self.skip_newlines();
        let (name, name_span) = self.expect_ident()?;

        // `Result` keeps `Ok` / `Err` as declaration-only constructor
        // contracts. They intentionally live behind `@@builtin type ...` so
        // the std-module declaration layer stays visually uniform, even though
        // the payload that follows is function-shaped rather than type-shaped.
        if (name == "Ok" || name == "Err") && matches!(self.peek(), Token::LParen) {
            return self.parse_result_ctor_builtin_type_decl(start, name, attrs);
        }

        let mut params = Vec::new();
        if matches!(self.peek(), Token::Lt) {
            self.advance();
            self.skip_newlines();
            loop {
                self.expect(&Token::Dollar)?;
                let (param_name, _) = self.expect_ident()?;
                params.push(format!("${}", param_name));
                self.skip_newlines();
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                    continue;
                }
                if matches!(self.peek(), Token::Gt) {
                    let gt = self.expect(&Token::Gt)?;
                    let end = if self.pos > 0 {
                        self.tokens[self.pos - 1].span.end
                    } else {
                        gt.end
                    };
                    return Ok(Ast::BuiltinTypeDecl(
                        Span { start, end },
                        BuiltinTypeHead {
                            span: Span {
                                start: name_span.start,
                                end,
                            },
                            name,
                            params,
                        },
                        attrs,
                    ));
                }
                if matches!(self.peek(), Token::Eof) {
                    return Err(ParseError::incomplete(">", self.peek_span()));
                }
                return Err(ParseError::syntax(
                    "Expected `,` or `>` in builtin type parameter list",
                    self.peek_span(),
                ));
            }
        }
        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };

        Ok(Ast::BuiltinTypeDecl(
            Span { start, end },
            BuiltinTypeHead {
                span: Span { start, end },
                name,
                params,
            },
            attrs,
        ))
    }

    fn parse_result_ctor_builtin_type_decl(
        &mut self,
        start: usize,
        name: Symbol,
        attrs: DeclAttrs,
    ) -> Result<Ast, ParseError> {
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        self.skip_newlines();
        let param_ty = self.parse_type()?;
        self.skip_newlines();
        self.expect(&Token::RParen)?;
        self.skip_newlines();
        self.expect(&Token::Arrow)?;
        self.skip_newlines();
        let ret_ty = self.parse_type()?;

        if matches!(self.peek(), Token::LBrace) {
            return Err(ParseError::syntax(
                "Result constructor builtin contracts in std modules must not have a function body",
                self.peek_span(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            start
        };

        Ok(Ast::ResultCtorDecl(
            Span { start, end },
            name,
            param_ty,
            ret_ty,
            attrs,
        ))
    }

    /// `def name(arg: Type, ...) -> Type { expr }`
    fn parse_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_def_with_attrs(DeclAttrs::default(), None)
    }

    fn parse_extractor_def(&mut self) -> Result<Ast, ParseError> {
        self.parse_extractor_def_with_attrs(DeclAttrs::default(), None)
    }

    fn parse_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        if self.should_parse_result_ctor_decl() {
            return self.parse_result_ctor_decl_with_attrs(attrs, annotator_start);
        }

        let (sp, name, type_params, params, ret_ty, visibility) = self.parse_def_signature()?;
        let mut attrs = attrs;
        attrs.visibility = visibility;

        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body_stmts = self.parse_block_stmts()?;
        if body_stmts.is_empty() {
            return Err(ParseError::syntax(
                "Function body must not be empty",
                self.peek_span(),
            ));
        }
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
                start: annotator_start.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            type_params,
            params,
            ret_ty,
            Box::new(body),
            attrs,
        ))
    }

    fn parse_extractor_def_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let (sp, name, type_params, param, ret_ty) = self.parse_extractor_signature()?;

        self.skip_newlines();
        self.expect(&Token::LBrace)?;
        let body_stmts = self.parse_block_stmts()?;
        if body_stmts.is_empty() {
            return Err(ParseError::syntax(
                "Extractor body must not be empty",
                self.peek_span(),
            ));
        }
        let end = self.expect(&Token::RBrace)?;
        let body = Ast::Block(
            Span {
                start: sp.start,
                end: end.end,
            },
            body_stmts,
        );

        Ok(Ast::ExtractorDef(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end: end.end,
            },
            name,
            type_params,
            param,
            ret_ty,
            Box::new(body),
            attrs,
        ))
    }

    fn should_parse_result_ctor_decl(&self) -> bool {
        if self.context.level != DeclLevel::Top {
            return false;
        }
        if self.context.module_path.is_some() {
            return false;
        }
        if !self
            .context
            .parse_rules
            .allowed_top_level_decl_kinds
            .allows(TopLevelDeclKind::BuiltinDecl)
        {
            return false;
        }
        if !matches!(self.peek(), Token::Def) {
            return false;
        }
        matches!(
            self.tokens.get(self.pos + 1).map(|sp| &sp.token),
            Some(Token::Ident(name)) if name == "Ok" || name == "Err"
        )
    }

    fn parse_result_ctor_decl_with_attrs(
        &mut self,
        attrs: DeclAttrs,
        annotator_start: Option<usize>,
    ) -> Result<Ast, ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Def)?;
        let (name, _) = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&Token::LParen)?;
        self.skip_newlines();
        let param_ty = self.parse_type()?;
        self.skip_newlines();
        self.expect(&Token::RParen)?;
        self.skip_newlines();
        self.expect(&Token::Arrow)?;
        self.skip_newlines();
        let ret_ty = self.parse_type()?;

        if matches!(self.peek(), Token::LBrace) {
            return Err(ParseError::syntax(
                "Result constructor declarations in std modules must not have a function body",
                self.peek_span(),
            ));
        }

        let end = if self.pos > 0 {
            self.tokens[self.pos - 1].span.end
        } else {
            sp.start
        };

        Ok(Ast::ResultCtorDecl(
            Span {
                start: annotator_start.unwrap_or(sp.start),
                end,
            },
            name,
            param_ty,
            ret_ty,
            attrs,
        ))
    }

    fn parse_fun_param(&mut self) -> Result<FunParam, ParseError> {
        let (name, span) = self.expect_ident()?;
        if name == "self" {
            return Err(ParseError::syntax(
                "`self` is only allowed as the first parameter of impl methods",
                span,
            ));
        }
        self.expect(&Token::Colon)?;
        let ty = self.parse_type()?;
        Ok(FunParam { name, ty, span })
    }

    // ── Match expression (step 8) ──

    /// `match expr { pat => body, ... }`
    fn parse_match_expr(&mut self) -> Result<Ast, ParseError> {
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
    fn parse_cond_expr(&mut self) -> Result<Ast, ParseError> {
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
    fn is_true_literal(expr: &Ast) -> bool {
        matches!(expr, Ast::Lit(_, Lit::Bool(true)))
    }
}

fn shift_span(span: Span, delta: usize) -> Span {
    Span {
        start: span.start + delta,
        end: span.end + delta,
    }
}

fn ast_ty_span(ty: &AstTy) -> &Span {
    match ty {
        AstTy::Named(span, _)
        | AstTy::ImplTrait(span, _)
        | AstTy::Generic(span, _, _)
        | AstTy::Tuple(span, _)
        | AstTy::Func(span, _, _) => span,
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
        | AstPattern::Constructor(span, _, _)
        | AstPattern::Call(span, _, _)
        | AstPattern::Tuple(span, _)
        | AstPattern::As(span, _, _, _) => span,
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

fn shift_ast_ty(ty: AstTy, delta: usize) -> AstTy {
    match ty {
        AstTy::Named(span, name) => AstTy::Named(shift_span(span, delta), name),
        AstTy::ImplTrait(span, name) => AstTy::ImplTrait(shift_span(span, delta), name),
        AstTy::Generic(span, name, args) => AstTy::Generic(
            shift_span(span, delta),
            name,
            args.into_iter()
                .map(|arg| shift_ast_ty(arg, delta))
                .collect(),
        ),
        AstTy::Tuple(span, items) => AstTy::Tuple(
            shift_span(span, delta),
            items
                .into_iter()
                .map(|item| shift_ast_ty(item, delta))
                .collect(),
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
        AstPattern::Constructor(span, name, inners) => AstPattern::Constructor(
            shift_span(span, delta),
            name,
            inners
                .into_iter()
                .map(|inner| shift_pattern(inner, delta))
                .collect(),
        ),
        AstPattern::Call(span, name, inners) => AstPattern::Call(
            shift_span(span, delta),
            name,
            inners
                .into_iter()
                .map(|inner| shift_pattern(inner, delta))
                .collect(),
        ),
        AstPattern::Tuple(span, items) => AstPattern::Tuple(
            shift_span(span, delta),
            items
                .into_iter()
                .map(|item| shift_pattern(item, delta))
                .collect(),
        ),
        AstPattern::As(span, inner, alias, alias_ty) => AstPattern::As(
            shift_span(span, delta),
            Box::new(shift_pattern(*inner, delta)),
            alias,
            alias_ty.map(|ty| shift_ast_ty(ty, delta)),
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

fn shift_extractor_param(param: ExtractorParam, delta: usize) -> ExtractorParam {
    ExtractorParam {
        name: param.name,
        ty: param.ty.map(|ty| shift_ast_ty(ty, delta)),
        span: shift_span(param.span, delta),
    }
}

fn shift_match_pattern(pat: AstPattern, delta: usize) -> AstPattern {
    shift_pattern(pat, delta)
}

fn shift_decl_attrs(attrs: DeclAttrs) -> DeclAttrs {
    attrs
}

fn shift_builtin_type_head(head: BuiltinTypeHead, delta: usize) -> BuiltinTypeHead {
    BuiltinTypeHead {
        span: shift_span(head.span, delta),
        name: head.name,
        params: head.params,
    }
}

fn shift_record_lit_arg(arg: RecordLitArg, delta: usize) -> RecordLitArg {
    match arg {
        RecordLitArg::Positional(expr) => RecordLitArg::Positional(shift_ast_span(expr, delta)),
        RecordLitArg::Named(name, expr) => RecordLitArg::Named(name, shift_ast_span(expr, delta)),
    }
}

fn shift_ast_path(path: AstPath, delta: usize) -> AstPath {
    AstPath {
        span: shift_span(path.span, delta),
        segments: path.segments,
    }
}

fn shift_ast_span(ast: Ast, delta: usize) -> Ast {
    match ast {
        Ast::Lit(span, lit) => Ast::Lit(shift_span(span, delta), lit),
        Ast::Var(span, name) => Ast::Var(shift_span(span, delta), name),
        Ast::Path(span, path) => Ast::Path(shift_span(span, delta), shift_ast_path(path, delta)),
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
        Ast::Pipe(span, left, right) => Ast::Pipe(
            shift_span(span, delta),
            Box::new(shift_ast_span(*left, delta)),
            Box::new(shift_ast_span(*right, delta)),
        ),
        Ast::ContextMap(span, left, right) => Ast::ContextMap(
            shift_span(span, delta),
            Box::new(shift_ast_span(*left, delta)),
            Box::new(shift_ast_span(*right, delta)),
        ),
        Ast::ContextBind(span, left, right) => Ast::ContextBind(
            shift_span(span, delta),
            Box::new(shift_ast_span(*left, delta)),
            Box::new(shift_ast_span(*right, delta)),
        ),
        Ast::Compose(span, left, right) => Ast::Compose(
            shift_span(span, delta),
            Box::new(shift_ast_span(*left, delta)),
            Box::new(shift_ast_span(*right, delta)),
        ),
        Ast::KleisliCompose(span, left, right) => Ast::KleisliCompose(
            shift_span(span, delta),
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
        Ast::TupleLiteral(span, elems) => Ast::TupleLiteral(
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
                    visibility: f.visibility,
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
                    visibility: f.visibility,
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
        Ast::DeferrorDef(span, name, fields, show_expr, attrs) => Ast::DeferrorDef(
            shift_span(span, delta),
            name,
            fields
                .into_iter()
                .map(|f| RecordField {
                    name: f.name,
                    ty: shift_ast_ty(f.ty, delta),
                    span: shift_span(f.span, delta),
                    visibility: f.visibility,
                })
                .collect(),
            Box::new(shift_ast_span(*show_expr, delta)),
            shift_decl_attrs(attrs),
        ),
        Ast::EnumDef(span, name, type_params, variants, attrs) => Ast::EnumDef(
            shift_span(span, delta),
            name,
            type_params
                .into_iter()
                .map(|param| TypeParam {
                    name: param.name,
                    bound: param.bound,
                    span: shift_span(param.span, delta),
                })
                .collect(),
            variants
                .into_iter()
                .map(|variant| EnumVariant {
                    name: variant.name,
                    payload: variant
                        .payload
                        .into_iter()
                        .map(|ty| shift_ast_ty(ty, delta))
                        .collect(),
                    discriminant: variant.discriminant,
                    span: shift_span(variant.span, delta),
                })
                .collect(),
            shift_decl_attrs(attrs),
        ),
        Ast::Def(span, name, type_params, params, ret_ty, body, attrs) => Ast::Def(
            shift_span(span, delta),
            name,
            type_params
                .into_iter()
                .map(|param| TypeParam {
                    name: param.name,
                    bound: param.bound,
                    span: shift_span(param.span, delta),
                })
                .collect(),
            params
                .into_iter()
                .map(|p| shift_fun_param(p, delta))
                .collect(),
            ret_ty.map(|ty| shift_ast_ty(ty, delta)),
            Box::new(shift_ast_span(*body, delta)),
            shift_decl_attrs(attrs),
        ),
        Ast::ExtractorDef(span, name, type_params, param, ret_ty, body, attrs) => {
            Ast::ExtractorDef(
                shift_span(span, delta),
                name,
                type_params
                    .into_iter()
                    .map(|param| TypeParam {
                        name: param.name,
                        bound: param.bound,
                        span: shift_span(param.span, delta),
                    })
                    .collect(),
                shift_extractor_param(param, delta),
                shift_ast_ty(ret_ty, delta),
                Box::new(shift_ast_span(*body, delta)),
                shift_decl_attrs(attrs),
            )
        }
        Ast::BuiltinDecl(span, name, params, ret_ty, attrs) => Ast::BuiltinDecl(
            shift_span(span, delta),
            name,
            params
                .into_iter()
                .map(|p| shift_fun_param(p, delta))
                .collect(),
            ret_ty.map(|ty| shift_ast_ty(ty, delta)),
            shift_decl_attrs(attrs),
        ),
        Ast::BuiltinExtractorDecl(span, name, param, ret_ty, attrs) => Ast::BuiltinExtractorDecl(
            shift_span(span, delta),
            name,
            shift_extractor_param(param, delta),
            shift_ast_ty(ret_ty, delta),
            shift_decl_attrs(attrs),
        ),
        Ast::BuiltinTypeDecl(span, head, attrs) => Ast::BuiltinTypeDecl(
            shift_span(span, delta),
            shift_builtin_type_head(head, delta),
            shift_decl_attrs(attrs),
        ),
        Ast::ResultCtorDecl(span, name, param_ty, ret_ty, attrs) => Ast::ResultCtorDecl(
            shift_span(span, delta),
            name,
            shift_ast_ty(param_ty, delta),
            shift_ast_ty(ret_ty, delta),
            shift_decl_attrs(attrs),
        ),
        Ast::Defmod(span, name, body, attrs) => Ast::Defmod(
            shift_span(span, delta),
            name,
            body.into_iter().map(|n| shift_ast_span(n, delta)).collect(),
            shift_decl_attrs(attrs),
        ),
        Ast::ImplDef(span, target, methods) => Ast::ImplDef(
            shift_span(span, delta),
            target,
            methods
                .into_iter()
                .map(|method| shift_ast_span(method, delta))
                .collect(),
        ),
        Ast::TraitDef(span, name, type_params, methods, attrs) => Ast::TraitDef(
            shift_span(span, delta),
            name,
            type_params
                .into_iter()
                .map(|param| TypeParam {
                    name: param.name,
                    bound: param.bound,
                    span: shift_span(param.span, delta),
                })
                .collect(),
            methods
                .into_iter()
                .map(|method| TraitMethodSig {
                    name: method.name,
                    type_params: method
                        .type_params
                        .into_iter()
                        .map(|param| TypeParam {
                            name: param.name,
                            bound: param.bound,
                            span: shift_span(param.span, delta),
                        })
                        .collect(),
                    params: method
                        .params
                        .into_iter()
                        .map(|param| shift_fun_param(param, delta))
                        .collect(),
                    ret_ty: shift_ast_ty(method.ret_ty, delta),
                    span: shift_span(method.span, delta),
                })
                .collect(),
            shift_decl_attrs(attrs),
        ),
        Ast::TraitImplDef(span, trait_name, trait_args, target, methods) => Ast::TraitImplDef(
            shift_span(span, delta),
            trait_name,
            trait_args
                .into_iter()
                .map(|arg| shift_ast_ty(arg, delta))
                .collect(),
            shift_ast_ty(target, delta),
            methods
                .into_iter()
                .map(|method| shift_ast_span(method, delta))
                .collect(),
        ),
        Ast::Import(span, path, spec) => {
            Ast::Import(shift_span(span, delta), shift_ast_path(path, delta), spec)
        }
        Ast::Closure(span, params, body) => Ast::Closure(
            shift_span(span, delta),
            params
                .into_iter()
                .map(|p| ClosureParam {
                    name: p.name,
                    ty: p.ty.map(|ty| shift_ast_ty(ty, delta)),
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
            | Ast::Path(s, _)
            | Ast::App(s, _, _)
            | Ast::Block(s, _)
            | Ast::Bind(s, _, _)
            | Ast::SafeBind(s, _, _)
            | Ast::BinOp(s, _, _, _)
            | Ast::Pipe(s, _, _)
            | Ast::ContextMap(s, _, _)
            | Ast::ContextBind(s, _, _)
            | Ast::Compose(s, _, _)
            | Ast::KleisliCompose(s, _, _)
            | Ast::ListNil(s)
            | Ast::ListCons(s, _, _)
            | Ast::ListLiteral(s, _)
            | Ast::TupleLiteral(s, _)
            | Ast::InterpolatedStr(s, _)
            | Ast::Match(s, _, _)
            | Ast::FieldAccess(s, _, _)
            | Ast::StructDef(s, _, _)
            | Ast::RecordDef(s, _, _)
            | Ast::StructLit(s, _, _)
            | Ast::ConstructorCall(s, _, _)
            | Ast::DeferrorDef(s, _, _, _, _)
            | Ast::EnumDef(s, _, _, _, _)
            | Ast::Def(s, _, _, _, _, _, _)
            | Ast::ExtractorDef(s, _, _, _, _, _, _)
            | Ast::BuiltinDecl(s, _, _, _, _)
            | Ast::BuiltinExtractorDecl(s, _, _, _, _)
            | Ast::BuiltinTypeDecl(s, _, _)
            | Ast::ResultCtorDecl(s, _, _, _, _)
            | Ast::Defmod(s, _, _, _)
            | Ast::ImplDef(s, _, _)
            | Ast::TraitDef(s, _, _, _, _)
            | Ast::TraitImplDef(s, _, _, _, _)
            | Ast::Import(s, _, _)
            | Ast::Closure(s, _, _)
            | Ast::Capture(s, _, _)
            | Ast::Semi(s, _) => s,
        }
    }
}

#[cfg(test)]
mod tests;
