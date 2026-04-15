use crate::ast::*;
use crate::error::ParseError;
use crate::lexer::tokenize;
use crate::token::{Spanned, Token};

mod chumsky_program;
mod completion;
mod context;
mod decl;
mod diagnostic;
mod error_map;
mod expr;
mod interpolate;
mod pattern;
mod stmt;
mod syntax_token;
mod ty;
mod validate;

pub use completion::{
    parse_incomplete_expr, parse_incomplete_stmt, CompletionContext, IncompleteParseResult,
};
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
        let sp = self.peek_span();
        match self.peek().clone() {
            Token::Ident(name) => {
                self.advance();
                Ok((name, sp))
            }
            Token::Import => {
                self.advance();
                Ok(("import".to_string(), sp))
            }
            Token::Include => {
                self.advance();
                Ok(("include".to_string(), sp))
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
                .map(|arm| AstMatchArm {
                    pattern: shift_match_pattern(arm.pattern, delta),
                    guard: arm.guard.map(|guard| shift_ast_span(guard, delta)),
                    body: shift_ast_span(arm.body, delta),
                })
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
        Ast::Include(span, path) => Ast::Include(shift_span(span, delta), path),
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
            | Ast::Include(s, _)
            | Ast::Closure(s, _, _)
            | Ast::Capture(s, _, _)
            | Ast::Semi(s, _) => s,
        }
    }
}

#[cfg(test)]
mod tests;
