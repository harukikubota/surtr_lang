use crate::ast::*;
use crate::error::ParseError;
use crate::lexer::tokenize;
use crate::token::{Spanned, Token};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclLevel {
    Top,
    Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileUnitKind {
    Script,
    Module,
    Project,
    Repl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPoint {
    pub qualified_symbol: String,
}

impl EntryPoint {
    pub fn qualified(qualified_symbol: impl Into<String>) -> Self {
        Self {
            qualified_symbol: qualified_symbol.into(),
        }
    }

    pub fn script_short_name(
        short_name: impl AsRef<str>,
        pseudo_module_path: impl AsRef<str>,
    ) -> Self {
        Self::qualified(format!(
            "{}::{}",
            pseudo_module_path.as_ref(),
            short_name.as_ref()
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetExitCodePolicy {
    Forbidden,
    Anywhere,
    EntryOnly,
}

impl SetExitCodePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Forbidden => "Forbidden",
            Self::Anywhere => "Anywhere",
            Self::EntryOnly => "EntryOnly",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopLevelDeclKind {
    Def,
    ExtractorDef,
    Defmod,
    ImplDef,
    TraitDef,
    TraitImplDef,
    Import,
    StructDef,
    RecordDef,
    DeferrorDef,
    EnumDef,
    BuiltinDecl,
    BuiltinExtractorDecl,
    BuiltinTypeDecl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopLevelDeclPolicy {
    Any,
    Only(Vec<TopLevelDeclKind>),
}

impl TopLevelDeclPolicy {
    fn allows(&self, kind: TopLevelDeclKind) -> bool {
        match self {
            Self::Any => true,
            Self::Only(allowed) => allowed.contains(&kind),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRules {
    pub allow_top_level_expr: bool,
    pub allowed_top_level_decl_kinds: TopLevelDeclPolicy,
    pub set_exit_code_policy: SetExitCodePolicy,
    pub normalized_entrypoint: Option<String>,
}

impl SourceRules {
    pub fn script() -> Self {
        Self {
            allow_top_level_expr: true,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::Only(vec![
                TopLevelDeclKind::Def,
                TopLevelDeclKind::Import,
            ]),
            set_exit_code_policy: SetExitCodePolicy::Anywhere,
            normalized_entrypoint: Some("main".to_string()),
        }
    }

    pub fn module() -> Self {
        Self::module_source()
    }

    pub fn module_source() -> Self {
        Self::module_source_without_builtin()
    }

    pub fn module_source_without_builtin() -> Self {
        Self {
            allow_top_level_expr: false,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::Only(vec![
                TopLevelDeclKind::Defmod,
                TopLevelDeclKind::ImplDef,
                TopLevelDeclKind::TraitDef,
                TopLevelDeclKind::TraitImplDef,
                TopLevelDeclKind::Import,
                TopLevelDeclKind::StructDef,
                TopLevelDeclKind::RecordDef,
                TopLevelDeclKind::DeferrorDef,
                TopLevelDeclKind::EnumDef,
            ]),
            set_exit_code_policy: SetExitCodePolicy::Forbidden,
            normalized_entrypoint: None,
        }
    }

    pub fn std_module() -> Self {
        Self {
            allow_top_level_expr: false,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::Only(vec![
                TopLevelDeclKind::Defmod,
                TopLevelDeclKind::ImplDef,
                TopLevelDeclKind::TraitDef,
                TopLevelDeclKind::TraitImplDef,
                TopLevelDeclKind::Import,
                TopLevelDeclKind::StructDef,
                TopLevelDeclKind::RecordDef,
                TopLevelDeclKind::DeferrorDef,
                TopLevelDeclKind::EnumDef,
                TopLevelDeclKind::BuiltinDecl,
                TopLevelDeclKind::BuiltinTypeDecl,
            ]),
            set_exit_code_policy: SetExitCodePolicy::Forbidden,
            normalized_entrypoint: None,
        }
    }

    pub fn module_member() -> Self {
        Self {
            allow_top_level_expr: false,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::Only(vec![
                TopLevelDeclKind::Def,
                TopLevelDeclKind::ExtractorDef,
                TopLevelDeclKind::TraitDef,
                TopLevelDeclKind::TraitImplDef,
            ]),
            set_exit_code_policy: SetExitCodePolicy::Forbidden,
            normalized_entrypoint: None,
        }
    }

    pub fn std_module_member() -> Self {
        Self {
            allow_top_level_expr: false,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::Only(vec![
                TopLevelDeclKind::Def,
                TopLevelDeclKind::ExtractorDef,
                TopLevelDeclKind::TraitDef,
                TopLevelDeclKind::TraitImplDef,
                TopLevelDeclKind::BuiltinDecl,
                TopLevelDeclKind::BuiltinExtractorDecl,
                TopLevelDeclKind::BuiltinTypeDecl,
            ]),
            set_exit_code_policy: SetExitCodePolicy::Forbidden,
            normalized_entrypoint: None,
        }
    }

    pub fn repl_chunk() -> Self {
        Self {
            allow_top_level_expr: true,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::Only(vec![
                TopLevelDeclKind::Def,
                TopLevelDeclKind::Import,
            ]),
            set_exit_code_policy: SetExitCodePolicy::Forbidden,
            normalized_entrypoint: None,
        }
    }

    pub fn project() -> Self {
        Self {
            allow_top_level_expr: true,
            allowed_top_level_decl_kinds: TopLevelDeclPolicy::Only(vec![
                TopLevelDeclKind::Def,
                TopLevelDeclKind::Defmod,
                TopLevelDeclKind::ImplDef,
                TopLevelDeclKind::TraitDef,
                TopLevelDeclKind::TraitImplDef,
                TopLevelDeclKind::Import,
                TopLevelDeclKind::StructDef,
                TopLevelDeclKind::RecordDef,
                TopLevelDeclKind::DeferrorDef,
                TopLevelDeclKind::EnumDef,
            ]),
            set_exit_code_policy: SetExitCodePolicy::Forbidden,
            normalized_entrypoint: None,
        }
    }

    pub fn with_set_exit_code_policy(
        mut self,
        policy: SetExitCodePolicy,
        entrypoint: Option<&EntryPoint>,
    ) -> Self {
        self.set_exit_code_policy = policy;
        self.normalized_entrypoint = entrypoint.map(|entry| entry.qualified_symbol.clone());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserContext {
    pub level: DeclLevel,
    pub unit_kind: CompileUnitKind,
    pub source_id: u32,
    pub module_path: Option<String>,
    pub source_rules: SourceRules,
}

impl Default for ParserContext {
    fn default() -> Self {
        Self::script(0)
    }
}

impl ParserContext {
    pub fn script(source_id: u32) -> Self {
        Self {
            level: DeclLevel::Top,
            unit_kind: CompileUnitKind::Script,
            source_id,
            module_path: None,
            source_rules: SourceRules::script(),
        }
    }

    pub fn module(source_id: u32, module_path: Option<String>) -> Self {
        Self {
            level: DeclLevel::Top,
            unit_kind: CompileUnitKind::Module,
            source_id,
            module_path,
            source_rules: SourceRules::module_source(),
        }
    }

    pub fn repl(source_id: u32) -> Self {
        Self {
            level: DeclLevel::Top,
            unit_kind: CompileUnitKind::Repl,
            source_id,
            module_path: None,
            source_rules: SourceRules::repl_chunk(),
        }
    }

    pub fn project(source_id: u32) -> Self {
        Self {
            level: DeclLevel::Top,
            unit_kind: CompileUnitKind::Project,
            source_id,
            module_path: None,
            source_rules: SourceRules::project(),
        }
    }

    pub fn with_rules(mut self, source_rules: SourceRules) -> Self {
        self.source_rules = source_rules;
        self
    }
}

/// Parse Surtr source text into an abstract syntax tree.
pub fn parse(source: &str) -> Result<Vec<Ast>, ParseError> {
    parse_with_context(source, ParserContext::default())
}

/// Parse Surtr source text with explicit compile-unit context.
pub fn parse_with_context(source: &str, context: ParserContext) -> Result<Vec<Ast>, ParseError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser::new(tokens, context);
    parser.parse_program()
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

    // ── Program ──

    fn parse_program(&mut self) -> Result<Vec<Ast>, ParseError> {
        self.context.level = DeclLevel::Top;
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

        if self.context.level == DeclLevel::Expr
            && matches!(
                self.peek(),
                Token::Annotator(_)
                    | Token::Def
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
            Token::Def => self.parse_def()?,
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
        if self.context.level == DeclLevel::Top {
            if let Some(kind) = Self::top_level_decl_kind(stmt) {
                if !self
                    .context
                    .source_rules
                    .allowed_top_level_decl_kinds
                    .allows(kind)
                {
                    return Err(ParseError::syntax(
                        "This top-level declaration is not allowed in the current source policy",
                        stmt.span().clone(),
                    ));
                }
            } else if !self.context.source_rules.allow_top_level_expr {
                let message = if self.context.unit_kind == CompileUnitKind::Module {
                    "Top-level expressions are not allowed in module compile units"
                } else {
                    "Top-level expressions are not allowed in this source context"
                };
                return Err(ParseError::syntax(message, stmt.span().clone()));
            }
        }
        Ok(())
    }

    fn top_level_decl_kind(ast: &Ast) -> Option<TopLevelDeclKind> {
        match ast {
            Ast::Def(_, _, _, _, _, _, _) => Some(TopLevelDeclKind::Def),
            Ast::ExtractorDef(_, _, _, _, _, _, _) => Some(TopLevelDeclKind::ExtractorDef),
            Ast::Defmod(_, _, _, _) => Some(TopLevelDeclKind::Defmod),
            Ast::ImplDef(_, _, _) => Some(TopLevelDeclKind::ImplDef),
            Ast::TraitDef(_, _, _, _, _) => Some(TopLevelDeclKind::TraitDef),
            Ast::TraitImplDef(_, _, _, _, _) => Some(TopLevelDeclKind::TraitImplDef),
            Ast::Import(_, _, _) => Some(TopLevelDeclKind::Import),
            Ast::StructDef(_, _, _) => Some(TopLevelDeclKind::StructDef),
            Ast::RecordDef(_, _, _) => Some(TopLevelDeclKind::RecordDef),
            Ast::DeferrorDef(_, _, _, _, _) => Some(TopLevelDeclKind::DeferrorDef),
            Ast::EnumDef(_, _, _, _, _) => Some(TopLevelDeclKind::EnumDef),
            Ast::BuiltinDecl(_, _, _, _, _) => Some(TopLevelDeclKind::BuiltinDecl),
            Ast::BuiltinExtractorDecl(_, _, _, _, _) => {
                Some(TopLevelDeclKind::BuiltinExtractorDecl)
            }
            Ast::BuiltinTypeDecl(_, _, _) => Some(TopLevelDeclKind::BuiltinTypeDecl),
            Ast::ResultCtorDecl(_, _, _, _, _) => Some(TopLevelDeclKind::BuiltinDecl),
            _ => None,
        }
    }

    fn parse_module_body_stmts(
        &mut self,
        module_path: Option<String>,
    ) -> Result<Vec<Ast>, ParseError> {
        let prev_context = self.context.clone();
        self.context.level = DeclLevel::Top;
        self.context.unit_kind = CompileUnitKind::Module;
        self.context.module_path = module_path;
        self.context.source_rules = if prev_context
            .source_rules
            .allowed_top_level_decl_kinds
            .allows(TopLevelDeclKind::BuiltinDecl)
        {
            SourceRules::std_module_member()
        } else {
            SourceRules::module_member()
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
            if !matches!(self.peek(), Token::Def) {
                return Err(ParseError::syntax(
                    "impl body may only contain `def` declarations",
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
        self.expect(&Token::Def)?;
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
            DeclAttrs::default(),
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

    fn parse_pattern_bind_stmt(&mut self) -> Result<Ast, ParseError> {
        let pat = self.parse_bind_pattern()?;
        let assign_tok = self.peek().clone();
        if !matches!(assign_tok, Token::Bind | Token::SafeBind) {
            return Err(ParseError::syntax(
                "Pattern destructuring requires assignment operator (`=` or `=?`)",
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
        Ok(match assign_tok {
            Token::Bind => Ast::Bind(span, pat, Box::new(rhs)),
            Token::SafeBind => Ast::SafeBind(span, pat, Box::new(rhs)),
            _ => unreachable!("validated assignment token"),
        })
    }

    fn is_pattern_bind_stmt_start(&self) -> bool {
        matches!(
            self.peek(),
            Token::LBrack
                | Token::LParen
                | Token::Ident(_)
                | Token::Int(_)
                | Token::Str(_)
                | Token::True
                | Token::False
                | Token::Minus
        )
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
                Token::Int(index) => {
                    let span = self.advance().span.clone();
                    (index.to_string(), span)
                }
                _ => {
                    return Err(ParseError::syntax(
                        "Expected field name or tuple index after '.'",
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
                let (args, call_end) =
                    self.attach_trailing_block_arg(&path_expr, args, end_span.end)?;
                let span = Span {
                    start: name_span.start,
                    end: call_end,
                };
                if path_last_is_uppercase {
                    return Ok(Ast::ConstructorCall(span, path_name, args));
                }
                return Ok(Ast::App(span, Box::new(path_expr), args));
            }

            if matches!(self.peek(), Token::Unit) {
                let end_span = self.advance().span.clone();
                let (args, call_end) =
                    self.attach_trailing_block_arg(&path_expr, Vec::new(), end_span.end)?;
                let span = Span {
                    start: name_span.start,
                    end: call_end,
                };
                if path_last_is_uppercase {
                    return Ok(Ast::ConstructorCall(span, path_name, args));
                }
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
            let (args, call_end) = self.attach_trailing_block_arg(&func, args, end_span.end)?;
            let span = Span {
                start: name_span.start,
                end: call_end,
            };

            if is_uppercase {
                // Constructor call: Name(val, ...) or Name(field: val, ...)
                return Ok(Ast::ConstructorCall(span, name, args));
            } else {
                // Normal function call
                return Ok(Ast::App(span, Box::new(func), args));
            }
        }

        // Zero-arg call: name() / Name()
        // Lexer tokenizes `()` as Token::Unit.
        if matches!(self.peek(), Token::Unit) {
            let end_span = self.advance().span.clone();
            let func = Ast::Var(name_span.clone(), name.clone());
            let (args, call_end) =
                self.attach_trailing_block_arg(&func, Vec::new(), end_span.end)?;
            let span = Span {
                start: name_span.start,
                end: call_end,
            };
            if is_uppercase {
                return Ok(Ast::ConstructorCall(span, name, args));
            }
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

        if args.iter().any(|arg| matches!(arg, RecordLitArg::Named(_, _))) {
            return Err(ParseError::syntax(
                "Trailing block sugar cannot follow named arguments",
                self.peek_span(),
            ));
        }

        let trailing = self.parse_primary()?;
        call_end = trailing.span().end;
        let trailing = match trailing {
            Ast::Block(span, stmts) if Self::trailing_block_uses_closure_sugar(callee) => {
                Ast::Closure(
                    span.clone(),
                    Vec::new(),
                    Box::new(Ast::Block(span, stmts)),
                )
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

        let first = self.parse_bind_pattern()?;
        self.skip_newlines();
        let end = if matches!(self.peek(), Token::Comma) {
            self.advance();
            self.skip_newlines();
            if matches!(self.peek(), Token::DotDot) {
                self.advance();
                self.skip_newlines();
                let tail = self.parse_bind_pattern()?;
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
            items.push(self.parse_bind_pattern()?);
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::RBrack) {
                    break;
                }
                items.push(self.parse_bind_pattern()?);
            }
            self.skip_newlines();
            let end = self.expect(&Token::RBrack)?;
            return Ok(fixed_bind_list_pattern(sp.start, end.end, items));
        } else {
            self.expect(&Token::RBrack)?
        };

        Ok(fixed_bind_list_pattern(sp.start, end.end, vec![first]))
    }

    fn parse_bind_pattern(&mut self) -> Result<AstPattern, ParseError> {
        let mut pat = self.parse_bind_pattern_atom()?;
        loop {
            self.skip_newlines();
            if !matches!(self.peek(), Token::At) {
                break;
            }
            self.advance(); // '@'
            self.skip_newlines();
            let (alias, alias_span) = self.expect_ident()?;
            if alias == "self" && self.impl_target_stack.is_empty() {
                return Err(ParseError::syntax(
                    "`self` can only be used inside impl methods",
                    alias_span,
                ));
            }
            self.skip_newlines();
            let alias_ty = if matches!(self.peek(), Token::Colon) {
                self.advance();
                self.skip_newlines();
                Some(self.parse_type()?)
            } else {
                None
            };
            let end = alias_ty
                .as_ref()
                .map(|ty| ast_ty_span(ty).end)
                .unwrap_or(alias_span.end);
            let span = Span {
                start: pattern_span(&pat).start,
                end,
            };
            pat = AstPattern::As(span, Box::new(pat), alias, alias_ty);
        }
        Ok(pat)
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
                if name == "self" && self.impl_target_stack.is_empty() {
                    return Err(ParseError::syntax(
                        "`self` can only be used inside impl methods",
                        sp,
                    ));
                }
                self.advance();
                let mut segments = vec![name.clone()];
                let mut path_end = sp.end;
                while self.has_path_separator() && matches!(self.peek_n(2), Some(Token::Ident(_))) {
                    self.consume_path_separator()?;
                    let (seg, seg_span) = self.expect_ident()?;
                    path_end = seg_span.end;
                    segments.push(seg);
                }

                let callee_name = segments.join("::");
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    self.skip_newlines();
                    let mut inners = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        inners.push(self.parse_bind_pattern()?);
                        self.skip_newlines();
                        while matches!(self.peek(), Token::Comma) {
                            self.advance();
                            self.skip_newlines();
                            if matches!(self.peek(), Token::RParen) {
                                break;
                            }
                            inners.push(self.parse_bind_pattern()?);
                            self.skip_newlines();
                        }
                    }
                    let end = self.expect(&Token::RParen)?;
                    return Ok(AstPattern::Call(
                        Span {
                            start: sp.start,
                            end: end.end,
                        },
                        callee_name,
                        inners,
                    ));
                }

                let is_ctor = segments
                    .last()
                    .and_then(|segment| segment.chars().next())
                    .map(|ch| ch.is_uppercase())
                    .unwrap_or(false);
                if is_ctor {
                    let ctor_name = callee_name;
                    if matches!(self.peek(), Token::Unit) {
                        let end = self.advance().span.clone();
                        return Ok(AstPattern::Constructor(
                            Span {
                                start: sp.start,
                                end: end.end,
                            },
                            ctor_name,
                            Vec::new(),
                        ));
                    }
                    return Ok(AstPattern::Constructor(
                        Span {
                            start: sp.start,
                            end: path_end,
                        },
                        ctor_name,
                        Vec::new(),
                    ));
                }

                if segments.len() > 1 {
                    return Err(ParseError::syntax(
                        "Qualified patterns support constructor forms only",
                        Span {
                            start: sp.start,
                            end: path_end,
                        },
                    ));
                }

                Ok(AstPattern::Var(sp, name))
            }
            Token::LBrack => self.parse_list_bind_pattern(),
            Token::LParen => {
                self.advance();
                self.skip_newlines();
                let first = self.parse_bind_pattern()?;
                self.skip_newlines();
                if matches!(self.peek(), Token::Comma) {
                    self.advance();
                    self.skip_newlines();
                    if matches!(self.peek(), Token::RParen) {
                        return Err(ParseError::syntax(
                            "1-tuple patterns are not supported",
                            Span {
                                start: sp.start,
                                end: self.peek_span().end,
                            },
                        ));
                    }
                    let mut items = vec![first, self.parse_bind_pattern()?];
                    self.skip_newlines();
                    while matches!(self.peek(), Token::Comma) {
                        self.advance();
                        self.skip_newlines();
                        if matches!(self.peek(), Token::RParen) {
                            break;
                        }
                        items.push(self.parse_bind_pattern()?);
                        self.skip_newlines();
                    }
                    let end = self.expect(&Token::RParen)?;
                    Ok(AstPattern::Tuple(
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
            Token::Eof => Err(ParseError::incomplete("list pattern", sp)),
            _ => Err(ParseError::syntax(
                "Pattern supports identifiers, literals, `_`, list patterns, nested `Ok(...)` patterns, and `pattern @ alias`",
                sp,
            )),
        }
    }

    // ── Type annotation parsing ──

    fn parse_type(&mut self) -> Result<AstTy, ParseError> {
        self.parse_type_in_impl_context(self.impl_target_stack.last().cloned())
    }

    fn parse_type_in_impl_context(
        &mut self,
        impl_target: Option<String>,
    ) -> Result<AstTy, ParseError> {
        self.skip_newlines();
        let sp = self.peek_span();

        if matches!(self.peek(), Token::LParen) {
            self.advance();
            self.skip_newlines();
            if matches!(self.peek(), Token::Arrow) {
                self.advance();
                let ret = self.parse_type_in_impl_context(impl_target.clone())?;
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
            params.push(self.parse_type_in_impl_context(impl_target.clone())?);
            self.skip_newlines();
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), Token::RParen) {
                    return Err(ParseError::syntax(
                        "1-tuple types are not supported",
                        Span {
                            start: sp.start,
                            end: self.peek_span().end,
                        },
                    ));
                }
                params.push(self.parse_type_in_impl_context(impl_target.clone())?);
                self.skip_newlines();
            }
            if matches!(self.peek(), Token::Arrow) {
                self.advance();
                let ret = self.parse_type_in_impl_context(impl_target.clone())?;
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

            let end = self.expect(&Token::RParen)?;
            return if params.len() == 1 {
                Ok(params.into_iter().next().expect("single grouped type"))
            } else {
                Ok(AstTy::Tuple(
                    Span {
                        start: sp.start,
                        end: end.end,
                    },
                    params,
                ))
            };
        }

        if matches!(self.peek(), Token::Dollar) {
            self.advance();
            let (name, end) = self.expect_ident()?;
            let name = format!("${}", name);
            if name == "$Self" {
                return Err(ParseError::syntax("Invalid type variable name: $Self", sp));
            }
            return Ok(AstTy::Named(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                name,
            ));
        }

        if matches!(self.peek(), Token::Unit) {
            let end = self.advance().span.clone();
            return Ok(AstTy::Named(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                "Unit".to_string(),
            ));
        }

        if matches!(self.peek(), Token::Impl) {
            self.advance();
            self.skip_newlines();
            let (trait_name, trait_span) = self.expect_ident()?;
            return Ok(AstTy::ImplTrait(
                Span {
                    start: sp.start,
                    end: trait_span.end,
                },
                trait_name,
            ));
        }

        // Named type, possibly with type args: Result<Int>, List<Int>, Option<Int>, ...
        let (name, _) = self.expect_ident()?;
        if name == "Self" {
            if impl_target.is_some() {
                return Ok(AstTy::Named(
                    Span {
                        start: sp.start,
                        end: sp.end,
                    },
                    "Self".to_string(),
                ));
            }
            return Err(ParseError::syntax(
                "`Self` can only be used inside impl methods",
                sp,
            ));
        }
        if name == "self" {
            return Err(ParseError::syntax("`self` is not a type name", sp));
        }

        // Check for type parameters: Name<T> or Name<T, E>
        if matches!(self.peek(), Token::Lt) {
            self.advance();
            self.skip_newlines();
            let mut args = vec![self.parse_type_in_impl_context(impl_target.clone())?];
            self.skip_newlines();
            while matches!(self.peek(), Token::Comma) {
                self.advance();
                self.skip_newlines();
                args.push(self.parse_type_in_impl_context(impl_target.clone())?);
                self.skip_newlines();
            }
            let end = self.expect_type_gt()?;
            return Ok(AstTy::Generic(
                Span {
                    start: sp.start,
                    end: end.end,
                },
                name,
                args,
            ));
        }

        Ok(AstTy::Named(
            Span {
                start: sp.start,
                end: sp.end,
            },
            name,
        ))
    }

    fn is_self_type(ty: &AstTy) -> bool {
        matches!(ty, AstTy::Named(_, name) if name == "Self")
    }

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
    ) -> Result<(Span, Symbol, Vec<TypeParam>, Vec<FunParam>, Option<AstTy>), ParseError> {
        self.parse_def_signature_with_name_mode(false)
    }

    fn parse_def_signature_with_name_mode(
        &mut self,
        allow_builtin_keyword_name: bool,
    ) -> Result<(Span, Symbol, Vec<TypeParam>, Vec<FunParam>, Option<AstTy>), ParseError> {
        let sp = self.peek_span();
        self.expect(&Token::Def)?;
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

        Ok((sp, name, type_params, params, ret_ty))
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
        let (_def_span, name, _type_params, params, ret_ty) =
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

        let (sp, name, type_params, params, ret_ty) = self.parse_def_signature()?;

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
            .source_rules
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
    fn parse_match_pattern(&mut self) -> Result<AstPattern, ParseError> {
        self.parse_bind_pattern()
    }

    fn is_true_literal(expr: &Ast) -> bool {
        matches!(expr, Ast::Lit(_, Lit::Bool(true)))
    }

    fn parse_string_or_interpolated(&mut self, span: Span, raw: String) -> Result<Ast, ParseError> {
        let parts = self.parse_interpolated_parts(&raw, &span)?;
        if parts.is_empty() {
            Ok(Ast::Lit(span, Lit::Str(raw)))
        } else if matches!(parts.as_slice(), [InterpolatedPart::Text(_)]) {
            match parts.into_iter().next() {
                Some(InterpolatedPart::Text(text)) => Ok(Ast::Lit(span, Lit::Str(text))),
                _ => unreachable!("checked single text part"),
            }
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
        let mut has_escaped_interpolation = false;

        while i < chars.len() {
            let ch = chars[i];
            let is_interp_start = ch == '#'
                && i + 1 < chars.len()
                && chars[i + 1] == '{'
                && (i == 0 || chars[i - 1] != '\\');
            if !is_interp_start {
                if ch == '\\' && i + 2 < chars.len() && chars[i + 1] == '#' && chars[i + 2] == '{' {
                    text.push('#');
                    has_escaped_interpolation = true;
                    i += 2;
                    continue;
                }
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

        if has_interpolation || has_escaped_interpolation {
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
        Ast::DeferrorDef(span, name, fields, show_expr, attrs) => Ast::DeferrorDef(
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
mod tests {
    use super::*;
    use sindr::primitives::int;

    #[test]
    fn test_bind_and_var() {
        let ast = parse("x = 42").unwrap();
        assert_eq!(ast.len(), 1);
        match &ast[0] {
            Ast::Bind(_, AstPattern::Var(_, name), rhs) => {
                assert_eq!(name, "x");
                assert!(matches!(rhs.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(42)));
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
    fn test_function_call_accepts_trailing_block_arg() {
        let ast = parse("if_then(True) { num = 10; num }").expect("trailing block call should parse");
        match &ast[0] {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "if_then"));
                assert_eq!(args.len(), 2);
                assert!(matches!(
                    &args[0],
                    RecordLitArg::Positional(Ast::Lit(_, Lit::Bool(true)))
                ));
                assert!(matches!(
                    &args[1],
                    RecordLitArg::Positional(Ast::Block(_, stmts))
                        if matches!(stmts.as_slice(), [Ast::Semi(_, _), Ast::Var(_, name)] if name == "num")
                ));
            }
            _ => panic!("Expected App"),
        }
    }

    #[test]
    fn test_zero_arg_call_accepts_trailing_block_arg() {
        let ast = parse("run() { print(\"x\") }").expect("zero-arg trailing block call should parse");
        match &ast[0] {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "run"));
                assert_eq!(args.len(), 1);
                assert!(matches!(
                    &args[0],
                    RecordLitArg::Positional(Ast::Block(_, stmts))
                        if matches!(stmts.as_slice(), [Ast::App(_, _, inner_args)] if inner_args.len() == 1)
                ));
            }
            _ => panic!("Expected App"),
        }
    }

    #[test]
    fn test_test_dsl_trailing_block_uses_zero_arg_closure() {
        let ast = parse(r#"test("suite") { it("case") { assert_true(True) } }"#)
            .expect("test DSL trailing block should parse");
        match &ast[0] {
            Ast::App(_, func, args) => {
                assert!(matches!(func.as_ref(), Ast::Var(_, ref n) if n == "test"));
                assert_eq!(args.len(), 2);
                assert!(matches!(
                    &args[0],
                    RecordLitArg::Positional(Ast::Lit(_, Lit::Str(value))) if value == "suite"
                ));
                assert!(matches!(
                    &args[1],
                    RecordLitArg::Positional(Ast::Closure(_, params, _)) if params.is_empty()
                ));
            }
            _ => panic!("Expected App"),
        }
    }

    #[test]
    fn test_trailing_block_rejects_named_args() {
        let err = parse("add(x: 1) { 2 }").expect_err("named args with trailing block should fail");
        assert!(err
            .message()
            .contains("Trailing block sugar cannot follow named arguments"));
    }

    #[test]
    fn test_match_scrutinee_does_not_consume_arm_block_as_trailing_call_arg() {
        let ast = parse("match noop() { _ => 1 }").expect("match scrutinee should parse");
        match &ast[0] {
            Ast::Match(_, scrutinee, arms) => {
                assert!(matches!(scrutinee.as_ref(), Ast::App(_, _, args) if args.is_empty()));
                assert_eq!(arms.len(), 1);
            }
            _ => panic!("Expected Match"),
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
            Ast::Def(_, name, _, params, ret_ty, body, attrs) => {
                assert_eq!(name, "add");
                assert_eq!(params.len(), 2);
                assert_eq!(attrs, &DeclAttrs::default());
                assert!(matches!(ret_ty, Some(AstTy::Named(_, ty)) if ty == "Int"));
                assert!(
                    matches!(body.as_ref(), Ast::Block(_, stmts) if matches!(stmts.as_slice(), [Ast::BinOp(_, BinOp::Add, _, _)]))
                );
            }
            _ => panic!("Expected Def"),
        }
        match &ast[1] {
            Ast::Def(_, name, _, params, ret_ty, body, attrs) => {
                assert_eq!(name, "noop");
                assert_eq!(params.len(), 0);
                assert_eq!(attrs, &DeclAttrs::default());
                assert!(ret_ty.is_none());
                assert!(
                    matches!(body.as_ref(), Ast::Block(_, stmts) if matches!(stmts.as_slice(), [Ast::Lit(_, Lit::Unit)]))
                );
            }
            _ => panic!("Expected Def"),
        }
    }

    #[test]
    fn test_impl_parses_and_keeps_methods() {
        let ast = parse_with_context(
            r#"defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }

  def normalize(self) -> Self {
    self
  }
}"#,
            ParserContext::module(1, None),
        )
        .expect("impl should parse");

        let impl_node = ast
            .iter()
            .find(|node| matches!(node, Ast::ImplDef(_, _, _)))
            .expect("expected impl node");
        match impl_node {
            Ast::ImplDef(_, target, methods) => {
                assert_eq!(target, "User");
                assert_eq!(methods.len(), 2);
                assert!(matches!(
                    &methods[0],
                    Ast::Def(_, name, _, _, Some(AstTy::Named(_, ret)), _, _)
                        if name == "new" && ret == "Self"
                ));
                assert!(matches!(
                    &methods[1],
                    Ast::Def(_, name, _, _, Some(AstTy::Named(_, ret)), _, _)
                        if name == "normalize" && ret == "Self"
                ));
            }
            _ => panic!("Expected ImplDef"),
        }
    }

    #[test]
    fn test_trait_def_parses_method_signatures() {
        let ast = parse_with_context(
            r#"deftrait Numeric {
  def add(self: Self, rhs: Self) -> Self
  def abs(self: Self) -> Self
}"#,
            ParserContext::module(1, None),
        )
        .expect("trait should parse");

        match ast.as_slice() {
            [Ast::TraitDef(_, name, type_params, methods, attrs)] => {
                assert_eq!(name, "Numeric");
                assert_eq!(attrs, &DeclAttrs::default());
                assert!(type_params.is_empty());
                assert_eq!(methods.len(), 2);
                assert_eq!(methods[0].name, "add");
                assert_eq!(methods[0].params.len(), 2);
                assert!(methods[0].type_params.is_empty());
                assert!(matches!(methods[0].params[0].ty, AstTy::Named(_, ref ty) if ty == "Self"));
                assert!(matches!(methods[0].params[1].ty, AstTy::Named(_, ref ty) if ty == "Self"));
                assert!(matches!(methods[0].ret_ty, AstTy::Named(_, ref ty) if ty == "Self"));
                assert_eq!(methods[1].name, "abs");
                assert_eq!(methods[1].params.len(), 1);
            }
            _ => panic!("Expected TraitDef"),
        }
    }

    #[test]
    fn test_trait_impl_parses_and_keeps_methods() {
        let ast = parse_with_context(
            r#"impl Numeric for Int {
  def add(self: Self, rhs: Self) -> Self {
    self + rhs
  }

  def abs(self: Self) -> Self {
    self
  }
}"#,
            ParserContext::module(1, None),
        )
        .expect("trait impl should parse");

        match ast.as_slice() {
            [Ast::TraitImplDef(_, trait_name, trait_args, AstTy::Named(_, target), methods)] => {
                assert_eq!(trait_name, "Numeric");
                assert!(trait_args.is_empty());
                assert_eq!(target, "Int");
                assert_eq!(methods.len(), 2);
                assert!(matches!(
                    &methods[0],
                    Ast::Def(_, name, _, _, Some(AstTy::Named(_, ret)), _, _)
                        if name == "add" && ret == "Self"
                ));
                assert!(matches!(
                    &methods[1],
                    Ast::Def(_, name, _, _, Some(AstTy::Named(_, ret)), _, _)
                        if name == "abs" && ret == "Self"
                ));
            }
            _ => panic!("Expected TraitImplDef"),
        }
    }

    #[test]
    fn test_function_def_parses_bounded_type_params() {
        let ast = parse("def add<$N: Numeric>(x: $N, y: $N) -> $N { x }").unwrap();

        match ast.as_slice() {
            [Ast::Def(_, name, type_params, params, Some(AstTy::Named(_, ret_ty)), _, _)] => {
                assert_eq!(name, "add");
                assert_eq!(type_params.len(), 1);
                assert_eq!(type_params[0].name, "$N");
                assert_eq!(type_params[0].bound.as_deref(), Some("Numeric"));
                assert_eq!(params.len(), 2);
                assert!(matches!(params[0].ty, AstTy::Named(_, ref ty) if ty == "$N"));
                assert!(matches!(params[1].ty, AstTy::Named(_, ref ty) if ty == "$N"));
                assert_eq!(ret_ty, "$N");
            }
            _ => panic!("Expected Def with bounded type parameters"),
        }
    }

    #[test]
    fn test_trait_def_parses_head_type_params() {
        let ast = parse_with_context(
            r#"deftrait From<$To> {
  def from(self: Self, to: TypeRef<$To>) -> $To
}"#,
            ParserContext::module(1, None),
        )
        .expect("generic trait should parse");

        match ast.as_slice() {
            [Ast::TraitDef(_, name, type_params, methods, _)] => {
                assert_eq!(name, "From");
                assert_eq!(type_params.len(), 1);
                assert_eq!(type_params[0].name, "$To");
                assert_eq!(methods.len(), 1);
                assert!(matches!(
                    methods[0].params[1].ty,
                    AstTy::Generic(_, ref name, ref args)
                        if name == "TypeRef"
                            && matches!(args.as_slice(), [AstTy::Named(_, arg)] if arg == "$To")
                ));
            }
            _ => panic!("Expected TraitDef"),
        }
    }

    #[test]
    fn test_trait_impl_parses_trait_type_args() {
        let ast = parse_with_context(
            r#"impl From<String> for Int {
  def from(self: Self, to: TypeRef<String>) -> String {
    inspect(self)
  }
}"#,
            ParserContext::module(1, None),
        )
        .expect("generic trait impl should parse");

        match ast.as_slice() {
            [Ast::TraitImplDef(_, trait_name, trait_args, AstTy::Named(_, target), methods)] => {
                assert_eq!(trait_name, "From");
                assert!(matches!(trait_args.as_slice(), [AstTy::Named(_, name)] if name == "String"));
                assert_eq!(target, "Int");
                assert_eq!(methods.len(), 1);
            }
            _ => panic!("Expected TraitImplDef"),
        }
    }

    #[test]
    fn test_function_def_parses_parameter_position_impl_trait() {
        let ast = parse("def abs(x: impl Numeric) -> Int { 0 }").unwrap();

        match ast.as_slice() {
            [Ast::Def(_, name, type_params, params, Some(AstTy::Named(_, ret_ty)), _, _)] => {
                assert_eq!(name, "abs");
                assert!(type_params.is_empty());
                assert_eq!(params.len(), 1);
                assert!(
                    matches!(params[0].ty, AstTy::ImplTrait(_, ref trait_name) if trait_name == "Numeric")
                );
                assert_eq!(ret_ty, "Int");
            }
            _ => panic!("Expected Def with impl Trait parameter"),
        }
    }

    #[test]
    fn test_return_position_impl_trait_is_rejected() {
        let err = parse("def bad(x: Int) -> impl Numeric { x }")
            .expect_err("return-position impl Trait should be rejected");
        assert!(err
            .message()
            .contains("return-position `impl Trait` is not supported"));
    }

    #[test]
    fn test_where_clause_is_rejected() {
        let err = parse("def add(x: Int) -> Int where Int: Numeric { x }")
            .expect_err("where clauses should be rejected");
        assert!(err
            .message()
            .contains("`where` clauses are staged and not implemented yet"));
    }

    #[test]
    fn test_impl_rejects_self_not_first_param() {
        let err = parse_with_context(
            r#"defstruct User {
  name: String,
}

impl User {
  def bad(x: Int, self: Self) -> Self {
    self
  }
}"#,
            ParserContext::module(1, None),
        )
        .expect_err("self after first parameter must fail");
        assert!(err
            .message()
            .contains("`self` is only allowed as the first parameter of impl methods"));
    }

    #[test]
    fn test_impl_allows_self_rebinding_syntax() {
        let ast = parse_with_context(
            r#"defstruct User {
  name: String,
}

impl User {
  def bad(self) -> Self {
    self = self
    self
  }
}"#,
            ParserContext::module(1, None),
        )
        .expect("self rebinding should be parsed");
        assert!(ast.iter().any(|node| matches!(node, Ast::ImplDef(_, _, _))));
    }

    #[test]
    fn test_defmod_rejects_self_and_self_type() {
        let err = parse(
            r#"defmod UserTools {
  def bad(self: Int) -> Int { self }
}"#,
        )
        .expect_err("defmod must reject `self`");
        assert!(err
            .message()
            .contains("`self` is only allowed as the first parameter of impl methods"));

        let err = parse(
            r#"defmod UserTools {
  def bad(x: Self) -> Int { 1 }
}"#,
        )
        .expect_err("defmod must reject `Self`");
        assert!(err
            .message()
            .contains("`Self` can only be used inside impl methods"));
    }

    #[test]
    fn test_builtin_decl() {
        let ast = parse_with_context(
            "@@builtin def to_string(a: $A) -> String",
            ParserContext::module(1, Some("Bootstrap".into()))
                .with_rules(SourceRules::std_module()),
        )
        .expect("std module should accept builtin declarations");
        match &ast[0] {
            Ast::BuiltinDecl(_, name, params, ret_ty, attrs) => {
                assert_eq!(name, "to_string");
                assert_eq!(params.len(), 1);
                assert_eq!(attrs, &DeclAttrs::default());
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
    fn test_builtin_type_decl() {
        let ast = parse_with_context(
            "@@builtin\ntype Int",
            ParserContext::module(1, Some("Bootstrap".into()))
                .with_rules(SourceRules::std_module()),
        )
        .expect("std module should accept builtin type declarations");
        assert!(matches!(
            ast.as_slice(),
            [Ast::BuiltinTypeDecl(_, BuiltinTypeHead { name, params, .. }, attrs)]
                if name == "Int" && params.is_empty() && attrs == &DeclAttrs::default()
        ));
    }

    #[test]
    fn test_doc_annotates_builtin_type_decl() {
        let ast = parse_with_context(
            "@@doc \"\"\"\nBuiltin Int.\n\"\"\"\n@@builtin type Int",
            ParserContext::module(1, Some("Bootstrap".into()))
                .with_rules(SourceRules::std_module()),
        )
        .expect("doc + builtin type should parse");

        assert!(matches!(
            ast.as_slice(),
            [Ast::BuiltinTypeDecl(_, BuiltinTypeHead { name, .. }, DeclAttrs { doc: Some(doc), .. })]
                if name == "Int" && doc == "\nBuiltin Int.\n"
        ));
    }

    #[test]
    fn test_doc_annotates_defmod() {
        let ast = parse_with_context(
            "@@doc \"\"\"Kernel docs\"\"\"\ndefmod Kernel {\n  def add(x: Int, y: Int) -> Int { x + y }\n}",
            ParserContext::module(1, None),
        )
        .expect("doc + defmod should parse");

        assert!(matches!(
            ast.as_slice(),
            [Ast::Defmod(_, name, _, DeclAttrs { doc: Some(doc), .. })]
                if name == "Kernel" && doc == "Kernel docs"
        ));
    }

    #[test]
    fn test_autoimport_annotates_defmod() {
        let ast = parse_with_context(
            "@@autoimport\ndefmod Kernel { def add(x: Int, y: Int) -> Int { x + y } }",
            ParserContext::module(1, None),
        )
        .expect("autoimport + defmod should parse");

        assert!(matches!(
            ast.as_slice(),
            [Ast::Defmod(_, name, _, DeclAttrs { auto_import: true, .. })]
                if name == "Kernel"
        ));
    }

    #[test]
    fn test_doc_annotates_deferror() {
        let ast = parse_with_context(
            "@@doc \"\"\"Missing value error\"\"\"\ndeferror NoneError { \"None Value.\" }",
            ParserContext::module(1, None),
        )
        .expect("doc + deferror should parse");

        assert!(matches!(
            ast.as_slice(),
            [Ast::DeferrorDef(_, name, _, _, DeclAttrs { doc: Some(doc), .. })]
                if name == "NoneError" && doc == "Missing value error"
        ));
    }

    #[test]
    fn test_doc_requires_following_declaration() {
        let err = parse("@@doc \"\"\"dangling\"\"\"").expect_err("expected parse error");
        assert!(err.message().contains("declaration"));
    }

    #[test]
    fn test_builtin_type_decl_preserves_generic_head() {
        let ast = parse_with_context(
            "@@builtin type Result<$T>",
            ParserContext::module(1, Some("Bootstrap".into()))
                .with_rules(SourceRules::std_module()),
        )
        .expect("generic builtin type should parse");
        assert!(matches!(
            ast.as_slice(),
            [Ast::BuiltinTypeDecl(_, BuiltinTypeHead { name, params, .. }, _)]
                if name == "Result" && params.as_slice() == ["$T"]
        ));
    }

    #[test]
    fn test_std_module_result_ctor_decls_are_accepted() {
        let ast = parse_with_context(
            r#"@@doc """
Construct the success branch.
"""
def Ok($T) -> Result<$T>

@@doc """
Construct the error branch.
"""
def Err(Error) -> Result<$T>"#,
            ParserContext::module(1, None).with_rules(SourceRules::std_module()),
        )
        .expect("result constructor declarations should parse in std modules");

        assert_eq!(ast.len(), 2);
        assert!(matches!(
            &ast[0],
            Ast::ResultCtorDecl(_, name, AstTy::Named(_, param), AstTy::Generic(_, ret_name, args), DeclAttrs { doc: Some(doc), .. })
                if name == "Ok" && param == "$T" && ret_name == "Result" && args.len() == 1 && doc.contains("success")
        ));
        assert!(matches!(
            &ast[1],
            Ast::ResultCtorDecl(_, name, AstTy::Named(_, param), AstTy::Generic(_, ret_name, args), DeclAttrs { doc: Some(doc), .. })
                if name == "Err" && param == "Error" && ret_name == "Result" && args.len() == 1 && doc.contains("error")
        ));
    }

    #[test]
    fn test_std_module_result_ctor_builtin_type_contracts_are_accepted() {
        let ast = parse_with_context(
            r#"@@doc """
Construct the success branch.
"""
@@builtin type Ok($T) -> Result<$T>

@@doc """
Construct the error branch.
"""
@@builtin type Err(Error) -> Result<$T>"#,
            ParserContext::module(1, None).with_rules(SourceRules::std_module()),
        )
        .expect("result constructor builtin contracts should parse in std modules");

        assert_eq!(ast.len(), 2);
        assert!(matches!(
            &ast[0],
            Ast::ResultCtorDecl(_, name, AstTy::Named(_, param), AstTy::Generic(_, ret_name, args), DeclAttrs { doc: Some(doc), .. })
                if name == "Ok" && param == "$T" && ret_name == "Result" && args.len() == 1 && doc.contains("success")
        ));
        assert!(matches!(
            &ast[1],
            Ast::ResultCtorDecl(_, name, AstTy::Named(_, param), AstTy::Generic(_, ret_name, args), DeclAttrs { doc: Some(doc), .. })
                if name == "Err" && param == "Error" && ret_name == "Result" && args.len() == 1 && doc.contains("error")
        ));
    }

    #[test]
    fn test_type_keyword_cannot_be_used_as_function_name() {
        let err = parse("def type() -> Int { 0 }").expect_err("type should stay reserved");
        assert!(err.message().contains("Expected identifier"));
    }

    #[test]
    fn test_builtin_decl_with_body_is_error() {
        let err = parse("@@builtin def print(a: String) -> Unit { print(a) }").expect_err("error");
        assert!(err.message().contains("must not have a function body"));
    }

    #[test]
    fn test_builtin_if_decl_accepts_keyword_name_in_std_module_member() {
        let ast = parse_with_context(
            r#"defmod Kernel {
  @@builtin def if(flag: Boolean, then_branch: (-> $A), else_branch: (-> $A)) -> $A
}"#,
            ParserContext::module(1, None).with_rules(SourceRules::std_module()),
        )
        .expect("builtin if declaration should parse");

        match &ast[0] {
            Ast::Defmod(_, name, body, _) => {
                assert_eq!(name, "Kernel");
                assert!(matches!(
                    &body[0],
                    Ast::BuiltinDecl(_, builtin_name, params, Some(AstTy::Named(_, ret)), _)
                        if builtin_name == "if" && params.len() == 3 && ret == "$A"
                ));
            }
            other => panic!("expected defmod, got {:?}", other),
        }
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
        // Expr-class operators are same-precedence and left-associative.
        let ast = parse("x = 1 + 2 * 3").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::BinOp(_, BinOp::Mul, left, right) => {
                    assert!(matches!(right.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(3)));
                    assert!(matches!(
                        left.as_ref(),
                        Ast::BinOp(_, BinOp::Add, ll, lr)
                            if matches!(ll.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(1))
                                && matches!(lr.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(2))
                    ));
                }
                _ => panic!("Expected left-associative Expr-class parse at top"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_logical_precedence_is_lower_than_expr_class() {
        let ast = parse("x = a + b == c").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::BinOp(_, BinOp::Eq, left, right) => {
                    assert!(matches!(right.as_ref(), Ast::Var(_, name) if name == "c"));
                    assert!(matches!(
                        left.as_ref(),
                        Ast::BinOp(_, BinOp::Add, ll, lr)
                            if matches!(ll.as_ref(), Ast::Var(_, name) if name == "a")
                                && matches!(lr.as_ref(), Ast::Var(_, name) if name == "b")
                    ));
                }
                other => panic!("Expected logical top-level parse, got {:?}", other),
            },
            other => panic!("Expected bind, got {:?}", other),
        }
    }

    #[test]
    fn test_func_literal_name_lowers_to_binary_call() {
        let ast = parse("x = left `eq` right").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::App(_, func, args) => {
                    assert!(matches!(func.as_ref(), Ast::Var(_, name) if name == "eq"));
                    assert!(matches!(
                        args.as_slice(),
                        [RecordLitArg::Positional(left), RecordLitArg::Positional(right)]
                            if matches!(left, Ast::Var(_, name) if name == "left")
                                && matches!(right, Ast::Var(_, name) if name == "right")
                    ));
                }
                other => panic!("Expected lowered App, got {:?}", other),
            },
            other => panic!("Expected bind, got {:?}", other),
        }
    }

    #[test]
    fn test_func_literal_operator_lowers_to_binop() {
        let ast = parse("x = left `+` right").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => {
                assert!(matches!(
                    rhs.as_ref(),
                    Ast::BinOp(_, BinOp::Add, left, right)
                        if matches!(left.as_ref(), Ast::Var(_, name) if name == "left")
                            && matches!(right.as_ref(), Ast::Var(_, name) if name == "right")
                ));
            }
            other => panic!("Expected bind, got {:?}", other),
        }
    }

    #[test]
    fn test_func_literal_operator_comparison_uses_logical_tier() {
        let ast = parse("x = a `==` b + c").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::BinOp(_, BinOp::Eq, left, right) => {
                    assert!(matches!(left.as_ref(), Ast::Var(_, name) if name == "a"));
                    assert!(matches!(
                        right.as_ref(),
                        Ast::BinOp(_, BinOp::Add, rl, rr)
                            if matches!(rl.as_ref(), Ast::Var(_, name) if name == "b")
                                && matches!(rr.as_ref(), Ast::Var(_, name) if name == "c")
                    ));
                }
                other => panic!("Expected logical-tier func literal parse, got {:?}", other),
            },
            other => panic!("Expected bind, got {:?}", other),
        }
    }

    #[test]
    fn test_standalone_func_literal_is_error() {
        let err = parse("`eq`").expect_err("expected parse error");
        assert!(err
            .message()
            .contains("FuncLiteral must appear in infix position"));
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
            Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Generic(_, name, args)), rhs) => {
                assert_eq!(name, "List");
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0], AstTy::Named(_, ref n) if n == "Int"));
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
                    assert!(matches!(head.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(1)));
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
    fn test_as_pattern_safebind_with_annotation() {
        let ast = parse("[head, ..tail] @ list_dup: List<Int> =? value").unwrap();
        match &ast[0] {
            Ast::SafeBind(_, pattern, rhs) => {
                assert!(matches!(
                    pattern,
                    AstPattern::As(_, inner, alias, Some(AstTy::Generic(_, name, args)))
                        if alias == "list_dup"
                        && name == "List"
                        && matches!(args.as_slice(), [AstTy::Named(_, elem)] if elem == "Int")
                        && matches!(inner.as_ref(), AstPattern::ListCons(_, _, _))
                ));
                assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
            }
            _ => panic!("Expected SafeBind"),
        }
    }

    #[test]
    fn test_nested_as_pattern_safebind() {
        let ast = parse("[head, .. [e2, ..tail] @ tail_dup] @ list_dup =? value").unwrap();
        match &ast[0] {
            Ast::SafeBind(_, pattern, rhs) => {
                assert!(matches!(
                    pattern,
                    AstPattern::As(_, outer_inner, outer_alias, None)
                        if outer_alias == "list_dup"
                        && matches!(
                            outer_inner.as_ref(),
                            AstPattern::ListCons(_, _, tail_pattern)
                                if matches!(
                                    tail_pattern.as_ref(),
                                    AstPattern::As(_, inner_list, inner_alias, None)
                                        if inner_alias == "tail_dup"
                                        && matches!(inner_list.as_ref(), AstPattern::ListCons(_, _, _))
                                )
                        )
                ));
                assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
            }
            _ => panic!("Expected SafeBind"),
        }
    }

    #[test]
    fn test_as_pattern_bind() {
        let ast = parse("[head, ..tail] @ list_dup = list").unwrap();
        match &ast[0] {
            Ast::Bind(_, pattern, rhs) => {
                assert!(matches!(
                    pattern,
                    AstPattern::As(_, inner, alias, None)
                        if alias == "list_dup"
                        && matches!(inner.as_ref(), AstPattern::ListCons(_, _, _))
                ));
                assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "list"));
            }
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_constructor_pattern_safebind() {
        let ast = parse("Ok(num) =? value").unwrap();
        match &ast[0] {
            Ast::SafeBind(_, pattern, rhs) => {
                assert!(matches!(
                    pattern,
                    AstPattern::Call(_, ctor, inner)
                        if ctor == "Ok"
                        && matches!(inner.as_slice(), [AstPattern::Var(_, name)] if name == "num")
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
                assert!(matches!(pattern, AstPattern::IntLit(_, n) if n == &int(1)));
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
                            AstPattern::Call(_, ctor, inner)
                            if ctor == "Ok" && matches!(inner.as_slice(), [AstPattern::IntLit(_, n)] if n == &int(1))
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
            Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Generic(_, name, args)), _) => {
                assert_eq!(name, "Result");
                assert!(matches!(args.as_slice(), [AstTy::Named(_, n)] if n == "Int"));
            }
            _ => panic!("Expected annotated Bind with Result type"),
        }
    }

    #[test]
    fn test_result_unit_type_annotation_uses_unit_token() {
        let ast = parse("def main() -> Result<()> { Ok(()) }").unwrap();
        match &ast[0] {
            Ast::Def(_, _, _, _, Some(AstTy::Generic(_, name, args)), _, _) => {
                assert_eq!(name, "Result");
                assert!(matches!(args.as_slice(), [AstTy::Named(_, n)] if n == "Unit"));
            }
            _ => panic!("Expected def with Result<()> return type"),
        }
    }

    #[test]
    fn test_generic_type_args_are_preserved_for_user_defined_type() {
        let ast = parse("v: Option<Result<Int, ParseError>> = value").unwrap();
        match &ast[0] {
            Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Generic(_, name, args)), _) => {
                assert_eq!(name, "Option");
                assert!(matches!(
                    args.as_slice(),
                    [AstTy::Generic(_, inner_name, inner_args)]
                        if inner_name == "Result"
                        && matches!(
                            inner_args.as_slice(),
                            [AstTy::Named(_, a), AstTy::Named(_, b)] if a == "Int" && b == "ParseError"
                        )
                ));
            }
            _ => panic!("Expected annotated bind with nested generic type"),
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
    fn test_qualified_capture_and_flow_parse() {
        let ast = parse("reader = &User::get_name\nout = value |> trim() |*> normalize()").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Ast::Capture(_, target, args)
                    if args.is_empty() && matches!(target.as_ref(), Ast::Path(_, path) if path.segments == vec!["User".to_string(), "get_name".to_string()])));
            }
            _ => panic!("Expected qualified capture"),
        }
        match &ast[1] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::ContextMap(_, left, _) => {
                    assert!(matches!(left.as_ref(), Ast::Pipe(_, _, _)));
                }
                other => panic!("Expected left-associative flow parse, got {:?}", other),
            },
            _ => panic!("Expected bind"),
        }
    }

    #[test]
    fn test_pipe_rhs_call_stays_as_app() {
        let ast =
            parse("out = user |> User::get_name()").expect("pipe with method call should parse");
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::Pipe(_, _, right) => {
                    assert!(matches!(right.as_ref(), Ast::App(_, _, args) if args.is_empty()));
                }
                other => panic!("Expected pipe node, got {:?}", other),
            },
            other => panic!("Expected bind, got {:?}", other),
        }
    }

    #[test]
    fn test_nested_generic_type_closes_without_confusing_compose() {
        let ast =
            parse("value: Result<List<Int>> = Ok([])").expect("nested generic type should parse");
        match &ast[0] {
            Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Generic(_, name, outer_args)), rhs) => {
                assert_eq!(name, "Result");
                assert_eq!(outer_args.len(), 1);
                assert!(matches!(
                    &outer_args[0],
                    AstTy::Generic(_, inner_name, inner_args)
                        if inner_name == "List"
                            && inner_args.len() == 1
                            && matches!(&inner_args[0], AstTy::Named(_, ty) if ty == "Int")
                ));
                assert!(matches!(
                    rhs.as_ref(),
                    Ast::ConstructorCall(_, ctor, args) if ctor == "Ok" && args.len() == 1
                ));
            }
            other => panic!("Expected annotated Result<List<Int>> bind, got {:?}", other),
        }
    }

    #[test]
    fn test_qualified_partial_capture_parses() {
        let ast = parse(r#"rename = &User::with_name("bob")"#)
            .expect("qualified partial capture should parse");
        match &ast[0] {
            Ast::Bind(_, _, rhs) => {
                assert!(matches!(
                    rhs.as_ref(),
                    Ast::Capture(_, target, args)
                        if args.len() == 1
                            && matches!(target.as_ref(), Ast::Path(_, path)
                                if path.segments == vec!["User".to_string(), "with_name".to_string()])
                ));
            }
            other => panic!("Expected qualified partial capture bind, got {:?}", other),
        }
    }

    #[test]
    fn test_compose_chain_is_left_associative_at_same_precedence() {
        let ast = parse("pipeline = parse() |=> validate() >> render()")
            .expect("compose chain should parse");
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::Compose(_, left, right) => {
                    assert!(matches!(right.as_ref(), Ast::App(_, _, args) if args.is_empty()));
                    assert!(matches!(left.as_ref(), Ast::KleisliCompose(_, _, _)));
                }
                other => panic!("Expected outer compose node, got {:?}", other),
            },
            other => panic!("Expected bind, got {:?}", other),
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
    fn test_closure_param_annotation_is_optional() {
        let ast = parse("fun = {|x: Int, y| y}").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::Closure(_, params, _) => {
                    assert_eq!(params.len(), 2);
                    assert!(matches!(params[0].ty, Some(AstTy::Named(_, ref n)) if n == "Int"));
                    assert!(params[1].ty.is_none());
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
            Ast::Def(_, _, _, _, _, body, _) => {
                assert!(matches!(
                    body.as_ref(),
                    Ast::Block(_, stmts) if matches!(stmts.as_slice(), [Ast::Semi(_, inner)] if matches!(inner.as_ref(), Ast::App(_, _, _)))
                ));
            }
            _ => panic!("Expected Def"),
        }
    }

    #[test]
    fn test_empty_def_body_is_error() {
        let err = parse("def noop() -> Unit {}").expect_err("Expected parse error");
        assert!(err.message().contains("Function body must not be empty"));
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
    fn test_interpolation_escape_drops_backslash() {
        let ast = parse(r#"msg = "\#{name}""#).unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Ast::Lit(_, Lit::Str(s)) if s == "#{name}"));
            }
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_negative_int() {
        let ast = parse("x = -5").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(-5)));
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
    fn test_tuple_literal() {
        let ast = parse("pair = (1, 2)").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::TupleLiteral(_, elems) => {
                    assert_eq!(elems.len(), 2);
                    assert!(matches!(elems[0], Ast::Lit(_, Lit::Int(ref n)) if n == &int(1)));
                    assert!(matches!(elems[1], Ast::Lit(_, Lit::Int(ref n)) if n == &int(2)));
                }
                other => panic!("Expected TupleLiteral, got {:?}", other),
            },
            other => panic!("Expected Bind, got {:?}", other),
        }
    }

    #[test]
    fn test_parenthesized_expression_is_not_tuple_literal() {
        let ast = parse("value = (1)").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Ast::Lit(_, Lit::Int(n)) if n == &int(1)));
            }
            other => panic!("Expected Bind, got {:?}", other),
        }
    }

    #[test]
    fn test_tuple_pattern_bind() {
        let ast = parse("(left, right) = pair").unwrap();
        match &ast[0] {
            Ast::Bind(_, pattern, rhs) => {
                assert!(matches!(
                    pattern,
                    AstPattern::Tuple(_, items)
                        if matches!(items.as_slice(),
                            [AstPattern::Var(_, left), AstPattern::Var(_, right)]
                                if left == "left" && right == "right")
                ));
                assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "pair"));
            }
            other => panic!("Expected Bind, got {:?}", other),
        }
    }

    #[test]
    fn test_tuple_type_annotation() {
        let ast = parse("pair: (Int, String) = value").unwrap();
        match &ast[0] {
            Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Tuple(_, items)), rhs) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(&items[0], AstTy::Named(_, name) if name == "Int"));
                assert!(matches!(&items[1], AstTy::Named(_, name) if name == "String"));
                assert!(matches!(rhs.as_ref(), Ast::Var(_, name) if name == "value"));
            }
            other => panic!("Expected annotated tuple bind, got {:?}", other),
        }
    }

    #[test]
    fn test_function_type_annotation_is_not_tuple_type() {
        let ast = parse("fun: (Int, String -> Unit) = {|x, y| ()}").unwrap();
        match &ast[0] {
            Ast::Bind(_, AstPattern::Annotated(_, _, AstTy::Func(_, params, ret)), rhs) => {
                assert_eq!(params.len(), 2);
                assert!(matches!(&params[0], AstTy::Named(_, name) if name == "Int"));
                assert!(matches!(&params[1], AstTy::Named(_, name) if name == "String"));
                assert!(matches!(ret.as_ref(), AstTy::Named(_, name) if name == "Unit"));
                assert!(matches!(rhs.as_ref(), Ast::Closure(_, params, _) if params.len() == 2));
            }
            other => panic!("Expected function type bind, got {:?}", other),
        }
    }

    #[test]
    fn test_field_access() {
        let ast = parse("user.name").unwrap();
        assert!(matches!(&ast[0], Ast::FieldAccess(_, _, ref f) if f == "name"));
    }

    #[test]
    fn test_tuple_index_field_access() {
        let ast = parse("first = pair.0").unwrap();
        match &ast[0] {
            Ast::Bind(_, _, rhs) => {
                assert!(matches!(rhs.as_ref(), Ast::FieldAccess(_, _, field) if field == "0"));
            }
            other => panic!("Expected Bind, got {:?}", other),
        }
    }

    #[test]
    fn test_one_tuple_literal_is_rejected() {
        let err = parse("pair = (1,)").expect_err("Expected parse error");
        assert!(err.message().contains("1-tuple literals are not supported"));
    }

    #[test]
    fn test_one_tuple_pattern_is_rejected() {
        let err = parse("(x,) = pair").expect_err("Expected parse error");
        assert!(err.message().contains("1-tuple patterns are not supported"));
    }

    #[test]
    fn test_one_tuple_type_is_rejected() {
        let err = parse("pair: (Int,) = value").expect_err("Expected parse error");
        assert!(err.message().contains("1-tuple types are not supported"));
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
                    assert!(matches!(&arms[0].0, AstPattern::IntLit(_, n) if n == &int(1)));
                    assert!(matches!(&arms[1].0, AstPattern::Wildcard(_)));
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
                    assert!(matches!(&arms[0].0, AstPattern::StrLit(_, s) if s == "a"));
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
                        AstPattern::ListCons(_, head, tail)
                            if matches!(head.as_ref(), AstPattern::IntLit(_, n) if n == &int(-1))
                                && matches!(tail.as_ref(), AstPattern::ListNil(_))
                    ));
                }
                _ => panic!("Expected Match"),
            },
            _ => panic!("Expected Bind with Match"),
        }
    }

    #[test]
    fn test_empty_match_is_error() {
        let err = parse("x = match value {}").expect_err("Expected parse error");
        assert!(err
            .message()
            .contains("Match expression must contain at least one arm"));
    }

    #[test]
    fn test_cond_desugars_to_nested_if_apps() {
        let ast = parse(
            r#"x = cond {
  a => 1,
  b => 2,
  True => 3,
}"#,
        )
        .expect("cond should parse");

        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::App(_, func, args) => {
                    assert!(matches!(func.as_ref(), Ast::Var(_, name) if name == "if"));
                    assert!(
                        matches!(&args[0], RecordLitArg::Positional(Ast::Var(_, name)) if name == "a")
                    );
                    assert!(
                        matches!(&args[1], RecordLitArg::Positional(Ast::Lit(_, Lit::Int(n))) if n == &int(1))
                    );
                    assert!(matches!(
                        &args[2],
                        RecordLitArg::Positional(Ast::App(_, inner_func, inner_args))
                            if matches!(inner_func.as_ref(), Ast::Var(_, name) if name == "if")
                                && matches!(&inner_args[0], RecordLitArg::Positional(Ast::Var(_, name)) if name == "b")
                                && matches!(&inner_args[1], RecordLitArg::Positional(Ast::Lit(_, Lit::Int(n))) if n == &int(2))
                                && matches!(&inner_args[2], RecordLitArg::Positional(Ast::Lit(_, Lit::Int(n))) if n == &int(3))
                    ));
                }
                _ => panic!("Expected App"),
            },
            _ => panic!("Expected Bind with cond RHS"),
        }
    }

    #[test]
    fn test_cond_accepts_block_body() {
        let ast = parse(
            r#"x = cond {
  True => { print("ok"); 1 },
}"#,
        )
        .expect("cond with block body should parse");

        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::Block(_, stmts) => {
                    assert!(
                        matches!(stmts.as_slice(), [Ast::Semi(_, _), Ast::Lit(_, Lit::Int(n))] if n == &int(1))
                    );
                }
                _ => panic!("Expected final True clause body to remain as block"),
            },
            _ => panic!("Expected Bind"),
        }
    }

    #[test]
    fn test_empty_cond_is_error() {
        let err = parse("x = cond {}").expect_err("Expected parse error");
        assert!(err
            .message()
            .contains("Cond expression must contain at least one clause"));
    }

    #[test]
    fn test_cond_requires_final_true_clause() {
        let err = parse(
            r#"x = cond {
  flag => 1,
}"#,
        )
        .expect_err("Expected parse error");
        assert!(err
            .message()
            .contains("Final cond clause must use `True` as its condition"));
    }

    #[test]
    fn test_cond_rejects_non_final_true_clause() {
        let err = parse(
            r#"x = cond {
  True => 1,
  other => 2,
}"#,
        )
        .expect_err("Expected parse error");
        assert!(err
            .message()
            .contains("`True` clause must be the final cond clause"));
    }

    #[test]
    fn test_match_constructor_pattern_is_accepted() {
        let ast = parse(
            r#"x = match value {
  Some(y) => y,
  _ => 0,
}"#,
        )
        .expect("constructor pattern should parse");
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::Match(_, _, arms) => {
                    assert!(matches!(
                        &arms[0].0,
                        AstPattern::Call(_, name, inner)
                            if name == "Some"
                                && matches!(inner.as_slice(), [AstPattern::Var(_, bound)] if bound == "y")
                    ));
                }
                _ => panic!("Expected Match"),
            },
            _ => panic!("Expected Bind with Match"),
        }
    }

    #[test]
    fn test_match_bare_uppercase_identifier_is_constructor_pattern() {
        let ast = parse(
            r#"x = match value {
  ParseError => 0,
  _ => 1,
}"#,
        )
        .expect("bare uppercase identifier should parse as a constructor pattern");
        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::Match(_, _, arms) => {
                    assert!(matches!(
                        &arms[0].0,
                        AstPattern::Constructor(_, name, args) if name == "ParseError" && args.is_empty()
                    ));
                }
                _ => panic!("Expected Match"),
            },
            _ => panic!("Expected Bind with Match"),
        }
    }

    #[test]
    fn test_match_as_and_annotated_pattern_is_accepted() {
        let ast = parse(
            r#"x = match value {
  [head, ..tail] @ whole: List<Int> => head,
  _ => 0,
}"#,
        )
        .expect("as-pattern and annotation in match should parse");

        match &ast[0] {
            Ast::Bind(_, _, rhs) => match rhs.as_ref() {
                Ast::Match(_, _, arms) => {
                    assert!(matches!(
                        &arms[0].0,
                        AstPattern::As(_, inner, alias, Some(AstTy::Generic(_, ty_name, ty_args)))
                            if alias == "whole"
                                && ty_name == "List"
                                && ty_args.len() == 1
                                && matches!(inner.as_ref(), AstPattern::ListCons(_, _, _))
                    ));
                }
                _ => panic!("Expected Match"),
            },
            _ => panic!("Expected Bind with Match"),
        }
    }

    #[test]
    fn test_defmod_parses_module_body() {
        let ast = parse_with_context(
            r#"defmod Kernel {
  def add(x: Int, y: Int) -> Int { x + y }
}"#,
            ParserContext::module(1, None),
        )
        .expect("defmod should parse");

        match ast.as_slice() {
            [Ast::Defmod(_, name, body, _)] => {
                assert_eq!(name, "Kernel");
                assert!(matches!(body.as_slice(), [Ast::Def(..)]));
            }
            _ => panic!("Expected single defmod declaration"),
        }
    }

    #[test]
    fn test_import_three_forms_parse() {
        let ast = parse(
            r#"import Kernel;
import Kernel::add;
import Kernel::{add, sub};"#,
        )
        .expect("imports should parse");

        assert!(matches!(
            ast[0],
            Ast::Import(_, AstPath { ref segments, .. }, ImportSpec::All)
                if segments.as_slice() == ["Kernel"]
        ));
        assert!(matches!(
            ast[1],
            Ast::Import(_, AstPath { ref segments, .. }, ImportSpec::Single(ref name))
                if segments.as_slice() == ["Kernel"] && name == "add"
        ));
        assert!(matches!(
            ast[2],
            Ast::Import(_, AstPath { ref segments, .. }, ImportSpec::List(ref names))
                if segments.as_slice() == ["Kernel"] && names.as_slice() == ["add", "sub"]
        ));
    }

    #[test]
    fn test_defenum_parses_variants_with_payload_and_discriminant() {
        let ast = parse_with_context(
            r#"defenum Direction {
  Up = 1,
  Down,
  Arrow(Int, Int),
}"#,
            ParserContext::module(1, None),
        )
        .expect("defenum should parse");

        match ast.as_slice() {
            [Ast::EnumDef(_, name, type_params, variants, _)] => {
                assert_eq!(name, "Direction");
                assert!(type_params.is_empty());
                assert_eq!(variants.len(), 3);
                assert_eq!(variants[0].name, "Up");
                assert_eq!(variants[0].discriminant, Some(int(1)));
                assert_eq!(variants[1].name, "Down");
                assert_eq!(variants[1].payload.len(), 0);
                assert_eq!(variants[2].name, "Arrow");
                assert_eq!(variants[2].payload.len(), 2);
            }
            other => panic!("Expected enum definition, got {:?}", other),
        }
    }

    #[test]
    fn test_defenum_parses_generic_header() {
        let ast = parse_with_context(
            r#"defenum ReduceStep<$A> {
  Resume($A),
  Stop($A),
}"#,
            ParserContext::module(1, None),
        )
        .expect("generic defenum should parse");

        match ast.as_slice() {
            [Ast::EnumDef(_, name, type_params, variants, _)] => {
                assert_eq!(name, "ReduceStep");
                assert_eq!(type_params.len(), 1);
                assert_eq!(type_params[0].name, "$A");
                assert_eq!(variants.len(), 2);
            }
            other => panic!("Expected generic enum definition, got {:?}", other),
        }
    }

    #[test]
    fn test_qualified_constructor_call_and_unit_constructor_parse() {
        let ast = parse(
            r#"x = Direction::Up
y = KeyInput::Arrow(Direction::Down)"#,
        )
        .expect("qualified constructors should parse");

        assert!(matches!(
            ast[0],
            Ast::Bind(_, _, ref rhs)
                if matches!(rhs.as_ref(), Ast::ConstructorCall(_, name, args) if name == "Direction::Up" && args.is_empty())
        ));
        assert!(matches!(
            ast[1],
            Ast::Bind(_, _, ref rhs)
                if matches!(rhs.as_ref(), Ast::ConstructorCall(_, name, args) if name == "KeyInput::Arrow" && args.len() == 1)
        ));
    }

    #[test]
    fn test_nested_defmod_is_rejected() {
        let err = parse(
            r#"defmod Outer {
  defmod Inner {
    def run() -> Unit { () }
  }
}"#,
        )
        .expect_err("nested defmod must be rejected");
        assert!(err
            .message()
            .contains("Nested module declarations are not allowed"));
    }

    #[test]
    fn test_defmod_body_rejects_top_level_expression() {
        let err = parse(
            r#"defmod Kernel {
  x = 42
}"#,
        )
        .expect_err("module body should reject top-level expressions");
        assert!(err
            .message()
            .contains("Top-level expressions are not allowed in module compile units"));
    }

    #[test]
    fn test_module_compile_unit_rejects_top_level_bind() {
        let err = parse_with_context("x = 42", ParserContext::module(1, None))
            .expect_err("module compile unit should reject top-level binding");
        assert!(err
            .message()
            .contains("Top-level expressions are not allowed in module compile units"));
    }

    #[test]
    fn test_module_compile_unit_rejects_top_level_def() {
        let err = parse_with_context(
            "def add(x: Int, y: Int) -> Int { x + y }",
            ParserContext::module(1, None),
        )
        .expect_err("module compile unit should require defmod wrappers for functions");
        assert!(err
            .message()
            .contains("This top-level declaration is not allowed in the current source policy"));
    }

    #[test]
    fn test_module_compile_unit_rejects_top_level_defextractor() {
        let err = parse_with_context(
            "defextractor never(self: Int) -> MatchResult<Int, Error> { MatchResult::NoMatch }",
            ParserContext::module(1, None),
        )
        .expect_err("module compile unit should require defmod wrappers for extractors");
        assert!(err
            .message()
            .contains("This top-level declaration is not allowed in the current source policy"));
    }

    #[test]
    fn test_module_compile_unit_accepts_top_level_defmod() {
        let ast = parse_with_context(
            "defmod Kernel { def add(x: Int, y: Int) -> Int { x + y } }",
            ParserContext::module(1, None),
        )
        .expect("module compile unit should accept defmod declarations");
        assert!(matches!(ast.as_slice(), [Ast::Defmod(_, _, _, _)]));
    }

    #[test]
    fn test_defmod_body_accepts_defextractor() {
        let ast = parse_with_context(
            r#"defmod Matchers {
  defextractor never(self: Int) -> MatchResult<Int, Error> {
    MatchResult::NoMatch
  }
}"#,
            ParserContext::module(1, None),
        )
        .expect("defmod should accept extractor declarations");
        assert!(matches!(
            ast.as_slice(),
            [Ast::Defmod(_, name, body, _)]
                if name == "Matchers"
                    && matches!(body.as_slice(), [Ast::ExtractorDef(_, extractor_name, _, _, _, _, _)] if extractor_name == "never")
        ));
    }

    #[test]
    fn test_module_compile_unit_accepts_import() {
        let ast = parse_with_context("import Kernel::add;", ParserContext::module(1, None))
            .expect("module compile unit should accept import declarations");
        assert!(matches!(
            ast.as_slice(),
            [Ast::Import(_, AstPath { segments, .. }, ImportSpec::Single(name))]
                if segments.as_slice() == ["Kernel"] && name == "add"
        ));
    }

    #[test]
    fn test_defmod_body_rejects_non_function_declarations() {
        let err = parse(
            r#"defmod Kernel {
  defrecord Pair(left: Int, right: Int)
}"#,
        )
        .expect_err("defmod should only contain function declarations");
        assert!(err
            .message()
            .contains("This top-level declaration is not allowed in the current source policy"));
    }

    #[test]
    fn test_module_compile_unit_rejects_builtin_decl() {
        let err = parse_with_context(
            "@@builtin def print(a: String) -> Unit",
            ParserContext::module(1, None),
        )
        .expect_err("user module compile unit should reject builtin declarations");
        assert!(err
            .message()
            .contains("This top-level declaration is not allowed in the current source policy"));
    }

    #[test]
    fn test_module_compile_unit_rejects_builtin_type_decl() {
        let err = parse_with_context("@@builtin type Int", ParserContext::module(1, None))
            .expect_err("user module compile unit should reject builtin type declarations");
        assert!(err
            .message()
            .contains("This top-level declaration is not allowed in the current source policy"));
    }

    #[test]
    fn test_std_module_compile_unit_accepts_builtin_decl() {
        let ast = parse_with_context(
            "defmod Bootstrap { @@builtin def print(a: String) -> Unit }",
            ParserContext::module(1, None).with_rules(SourceRules::std_module()),
        )
        .expect("std module compile unit should accept builtin declarations");
        assert!(
            matches!(ast.as_slice(), [Ast::Defmod(_, name, body, _)] if name == "Bootstrap"
            && matches!(body.as_slice(), [Ast::BuiltinDecl(_, _, _, _, _)]))
        );
    }

    #[test]
    fn test_std_module_compile_unit_accepts_builtin_type_decl() {
        let ast = parse_with_context(
            "defmod Bootstrap { @@builtin type Int }",
            ParserContext::module(1, None).with_rules(SourceRules::std_module()),
        )
        .expect("std module compile unit should accept builtin type declarations");
        assert!(
            matches!(ast.as_slice(), [Ast::Defmod(_, name, body, _)] if name == "Bootstrap"
            && matches!(body.as_slice(), [Ast::BuiltinTypeDecl(_, BuiltinTypeHead { name: builtin_name, .. }, _)] if builtin_name == "Int"))
        );
    }

    #[test]
    fn test_script_compile_unit_accepts_top_level_def_and_import() {
        let ast = parse_with_context(
            "def add(x: Int, y: Int) -> Int { x + y }\nimport Kernel::add;",
            ParserContext::script(1),
        )
        .expect("script compile unit should accept top-level def and import");
        assert!(matches!(
            ast.as_slice(),
            [Ast::Def(_, name, _, _, _, _, _), Ast::Import(_, AstPath { segments, .. }, ImportSpec::Single(import_name))]
                if name == "add" && segments.as_slice() == ["Kernel"] && import_name == "add"
        ));
    }

    #[test]
    fn test_script_compile_unit_rejects_top_level_struct_def() {
        let err = parse_with_context("defstruct User { name: String }", ParserContext::script(1))
            .expect_err("script compile unit should reject top-level type declarations");
        assert!(err
            .message()
            .contains("This top-level declaration is not allowed in the current source policy"));
    }

    #[test]
    fn test_repl_compile_unit_rejects_top_level_impl_block() {
        let err = parse_with_context(
            "impl User { def new(name: String) -> Self { User { name: name } } }",
            ParserContext::repl(1),
        )
        .expect_err("repl chunk should reject top-level impl declarations");
        assert!(err
            .message()
            .contains("This top-level declaration is not allowed in the current source policy"));
    }

    #[test]
    fn test_project_parser_context_sets_unit_kind() {
        let context = ParserContext::project(7);
        assert_eq!(context.unit_kind, CompileUnitKind::Project);
        assert_eq!(context.source_id, 7);
        assert_eq!(context.module_path, None);
    }

    #[test]
    fn test_project_compile_unit_accepts_top_level_expression() {
        let ast = parse_with_context("x = 42", ParserContext::project(1))
            .expect("project compile unit should accept top-level expressions");
        assert!(matches!(ast.as_slice(), [Ast::Bind(_, _, _)]));
    }

    #[test]
    fn test_project_compile_unit_rejects_top_level_defextractor() {
        let err = parse_with_context(
            "defextractor never(self: Int) -> MatchResult<Int, Error> { MatchResult::NoMatch }",
            ParserContext::project(1),
        )
        .expect_err("project compile unit should reject top-level extractor declarations");
        assert!(err
            .message()
            .contains("This top-level declaration is not allowed in the current source policy"));
    }

    #[test]
    fn test_declaration_inside_function_body_is_rejected() {
        let err = parse(
            r#"def outer() -> Unit {
  def inner() -> Unit { () }
}"#,
        )
        .expect_err("declaration inside expression level should be rejected");
        assert!(err
            .message()
            .contains("Declarations are only allowed at the top level"));
    }

    #[test]
    fn test_constructor_like_capture_parses_and_is_left_for_later_validation() {
        let ast = parse("f = &Some").expect("constructor-like capture should parse");
        assert!(matches!(
            ast.as_slice(),
            [Ast::Bind(_, _, rhs)]
                if matches!(rhs.as_ref(), Ast::Capture(_, target, args)
                    if args.is_empty() && matches!(target.as_ref(), Ast::Var(_, name) if name == "Some"))
        ));
    }

    #[test]
    fn test_assignment_is_rejected_in_argument_position() {
        let err = parse("f(x: y = 1)").expect_err("Expected parse error");
        assert!(err.message().contains("cannot appear in argument position"));
    }
}
