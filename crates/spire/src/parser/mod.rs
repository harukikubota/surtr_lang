use crate::ast::*;
use crate::error::ParseError;
use crate::lexer::tokenize;
use crate::token::{Spanned, Token};
use std::collections::VecDeque;

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

pub const MAX_PARSE_NESTING: usize = 32;
pub const MAX_PARSE_NESTING_MESSAGE: &str = "maximum parse nesting depth exceeded";

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
    reject_excessive_delimiter_nesting(&tokens)?;
    let ast = chumsky_program::parse_program_with_chumsky(&tokens, context)?;
    lower_namespaces(ast)
}

/// Parse Surtr source with parser diagnostic metadata for editor tooling.
pub fn parse_with_context_diagnostic(
    source: &str,
    context: ParserContext,
) -> Result<Vec<Ast>, ParseDiagnostic> {
    let tokens = tokenize(source).map_err(ParseDiagnostic::from)?;
    reject_excessive_delimiter_nesting(&tokens).map_err(ParseDiagnostic::from)?;
    let ast = chumsky_program::parse_program_with_chumsky_diagnostic(&tokens, context)
        .map_err(ParseDiagnostic::from)?;
    lower_namespaces(ast).map_err(ParseDiagnostic::from)
}

fn reject_excessive_delimiter_nesting(tokens: &[Spanned<Token>]) -> Result<(), ParseError> {
    let mut depth = 0usize;
    for token in tokens {
        match token.token {
            Token::LParen | Token::LBrack | Token::LBrace => {
                depth += 1;
                if depth > MAX_PARSE_NESTING {
                    return Err(ParseError::syntax(
                        MAX_PARSE_NESTING_MESSAGE,
                        token.span.clone(),
                    ));
                }
            }
            Token::RParen | Token::RBrack | Token::RBrace => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    Ok(())
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

struct Parser<'a> {
    tokens: &'a [Spanned<Token>],
    synthetic_tokens: VecDeque<Spanned<Token>>,
    pos: usize,
    context: ParserContext,
    impl_target_stack: Vec<Symbol>,
    allow_trailing_call_block: bool,
    parse_nesting_depth: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Spanned<Token>], context: ParserContext) -> Self {
        Self {
            tokens,
            synthetic_tokens: VecDeque::new(),
            pos: 0,
            context,
            impl_target_stack: Vec::new(),
            allow_trailing_call_block: true,
            parse_nesting_depth: 0,
        }
    }

    // ── Helpers ──

    fn peek(&self) -> &Token {
        if let Some(token) = self.synthetic_tokens.front() {
            &token.token
        } else {
            &self.tokens[self.pos].token
        }
    }

    fn peek_n(&self, n: usize) -> Option<&Token> {
        if n < self.synthetic_tokens.len() {
            self.synthetic_tokens.get(n).map(|sp| &sp.token)
        } else {
            self.tokens
                .get(self.pos + n - self.synthetic_tokens.len())
                .map(|sp| &sp.token)
        }
    }

    fn peek_span(&self) -> Span {
        if let Some(token) = self.synthetic_tokens.front() {
            token.span.clone()
        } else {
            self.tokens[self.pos].span.clone()
        }
    }

    fn advance(&mut self) -> Spanned<Token> {
        if let Some(token) = self.synthetic_tokens.pop_front() {
            token
        } else {
            let token = self.tokens[self.pos].clone();
            self.pos += 1;
            token
        }
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
                let composed = self.advance().span;
                let first = Span {
                    start: composed.start,
                    end: composed.start + 1,
                };
                let second = Span {
                    start: composed.start + 1,
                    end: composed.end,
                };
                self.synthetic_tokens.push_front(Spanned {
                    token: Token::Gt,
                    span: second,
                });
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

    fn with_parse_nesting<T>(
        &mut self,
        span: Span,
        f: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        self.parse_nesting_depth += 1;
        if self.parse_nesting_depth > MAX_PARSE_NESTING {
            self.parse_nesting_depth -= 1;
            return Err(ParseError::syntax(MAX_PARSE_NESTING_MESSAGE, span));
        }
        let result = f(self);
        self.parse_nesting_depth -= 1;
        result
    }

    fn stmt_has_explicit_separator(stmt: &Ast) -> bool {
        matches!(stmt, Ast::Semi(_, _))
    }

    fn anonymous_callable_call_target(stmt: &Ast) -> Option<&Ast> {
        match stmt {
            Ast::Bind(_, _, rhs) | Ast::SafeBind(_, _, rhs) | Ast::Semi(_, rhs) => {
                Self::anonymous_callable_call_target(rhs)
            }
            Ast::Capture(_, _, _)
            | Ast::Closure(_, _, _)
            | Ast::Grouped(_, _)
            | Ast::App(_, _, _) => Some(stmt),
            _ => None,
        }
    }

    fn starts_immediate_anonymous_callable_call(&self, stmt: &Ast) -> bool {
        let next_starts_call = matches!(self.peek(), Token::LParen | Token::Unit);
        if !next_starts_call {
            return false;
        }

        Self::anonymous_callable_call_target(stmt).is_some()
    }

    fn ensure_stmt_boundary(&self, stmt: &Ast, allow_rbrace: bool) -> Result<(), ParseError> {
        if Self::stmt_has_explicit_separator(stmt) {
            return Ok(());
        }
        if self.starts_immediate_anonymous_callable_call(stmt) {
            return Err(ParseError::syntax(
                "Immediate calls on anonymous callable expressions are not supported; bind the callable to a name and call it as `fn(args)`",
                self.peek_span(),
            ));
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

    fn expect_qualified_ident(
        &mut self,
        max_segments: usize,
        label: &str,
    ) -> Result<(Symbol, Span), ParseError> {
        let (first, first_span) = self.expect_ident()?;
        let start = first_span.start;
        let mut end = first_span.end;
        let mut segments = vec![first];
        while self.has_path_separator() && matches!(self.peek_n(2), Some(Token::Ident(_))) {
            self.consume_path_separator()?;
            let (segment, span) = self.expect_ident()?;
            end = span.end;
            segments.push(segment);
            if segments.len() > max_segments {
                return Err(ParseError::syntax(
                    format!("{label} path must not exceed {max_segments} segments"),
                    Span { start, end },
                ));
            }
        }
        Ok((segments.join("::"), Span { start, end }))
    }
}

fn shift_span(span: Span, delta: usize) -> Span {
    Span {
        start: span.start + delta,
        end: span.end + delta,
    }
}

fn lower_namespaces(ast: Vec<Ast>) -> Result<Vec<Ast>, ParseError> {
    let mut out = Vec::new();
    for node in ast {
        lower_namespace_node(node, None, &mut out)?;
    }
    Ok(out)
}

fn lower_namespace_node(
    node: Ast,
    namespace: Option<&str>,
    out: &mut Vec<Ast>,
) -> Result<(), ParseError> {
    match node {
        Ast::Namespace(span, name, body) => {
            if namespace.is_some() {
                return Err(ParseError::syntax(
                    "Nested namespace declarations are not allowed",
                    span,
                ));
            }
            for inner in body {
                lower_namespace_node(inner, Some(name.as_str()), out)?;
            }
            Ok(())
        }
        other => {
            out.push(apply_namespace_to_decl(other, namespace)?);
            Ok(())
        }
    }
}

fn apply_namespace_to_decl(node: Ast, namespace: Option<&str>) -> Result<Ast, ParseError> {
    let Some(namespace) = namespace else {
        return Ok(node);
    };
    match node {
        Ast::Defmod(span, name, body, attrs) => Ok(Ast::Defmod(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "module")?,
            body,
            attrs,
        )),
        Ast::ImplDef(span, target, methods, attrs) => Ok(Ast::ImplDef(
            span.clone(),
            qualify_namespace_head(namespace, &target, 2, &span, "impl target")?,
            methods,
            attrs,
        )),
        Ast::TraitDef(span, name, type_params, methods, attrs) => Ok(Ast::TraitDef(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "trait")?,
            type_params,
            methods,
            attrs,
        )),
        Ast::TraitImplDef(span, trait_name, trait_args, target_ty, methods, attrs) => {
            Ok(Ast::TraitImplDef(
                span.clone(),
                qualify_namespace_head(namespace, &trait_name, 2, &span, "trait")?,
                trait_args,
                qualify_namespace_type(target_ty, namespace)?,
                methods,
                attrs,
            ))
        }
        Ast::StructDef(span, name, fields) => Ok(Ast::StructDef(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "type")?,
            fields,
        )),
        Ast::RecordDef(span, name, fields) => Ok(Ast::RecordDef(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "type")?,
            fields,
        )),
        Ast::DeferrorDef(span, name, fields, show_expr, attrs) => Ok(Ast::DeferrorDef(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "type")?,
            fields,
            show_expr,
            attrs,
        )),
        Ast::EnumDef(span, name, type_params, variants, attrs) => Ok(Ast::EnumDef(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "type")?,
            type_params,
            variants,
            attrs,
        )),
        Ast::BuiltinTypeDecl(span, mut head, attrs) => {
            head.name = qualify_namespace_head(namespace, &head.name, 2, &span, "type")?;
            Ok(Ast::BuiltinTypeDecl(span, head, attrs))
        }
        Ast::Namespace(span, _, _) => Err(ParseError::syntax(
            "Nested namespace declarations are not allowed",
            span,
        )),
        other => Ok(other),
    }
}

fn qualify_namespace_head(
    namespace: &str,
    name: &str,
    max_segments: usize,
    span: &Span,
    label: &str,
) -> Result<String, ParseError> {
    let segments = name.split("::").collect::<Vec<_>>();
    if segments.len() > max_segments {
        return Err(ParseError::syntax(
            format!("{label} path must not exceed {max_segments} segments"),
            span.clone(),
        ));
    }
    if segments.len() == max_segments {
        return Ok(name.to_string());
    }
    Ok(format!("{namespace}::{name}"))
}

fn qualify_namespace_type(ty: AstTy, namespace: &str) -> Result<AstTy, ParseError> {
    match ty {
        AstTy::Named(span, name) => {
            if name == "Self" || name.starts_with('$') || name == "_" || name == "Hole" {
                Ok(AstTy::Named(span, name))
            } else {
                Ok(AstTy::Named(
                    span.clone(),
                    qualify_namespace_head(namespace, &name, 2, &span, "type")?,
                ))
            }
        }
        AstTy::ImplTrait(span, name) => Ok(AstTy::ImplTrait(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "trait")?,
        )),
        AstTy::Generic(span, name, args) => Ok(AstTy::Generic(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "type")?,
            args.into_iter()
                .map(|arg| qualify_namespace_type(arg, namespace))
                .collect::<Result<Vec<_>, ParseError>>()?,
        )),
        AstTy::Tuple(span, items) => Ok(AstTy::Tuple(
            span,
            items
                .into_iter()
                .map(|item| qualify_namespace_type(item, namespace))
                .collect::<Result<Vec<_>, ParseError>>()?,
        )),
        AstTy::Func(span, params, ret) => Ok(AstTy::Func(
            span,
            params
                .into_iter()
                .map(|param| qualify_namespace_type(param, namespace))
                .collect::<Result<Vec<_>, ParseError>>()?,
            Box::new(qualify_namespace_type(*ret, namespace)?),
        )),
    }
}

pub fn rebase_ast_spans(ast: Vec<Ast>, delta: usize) -> Vec<Ast> {
    ast.into_iter()
        .map(|node| shift_ast_span(node, delta))
        .collect()
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
        | AstPattern::Or(span, _)
        | AstPattern::As(span, _, _, _) => span,
    }
}

fn pattern_depth(pat: &AstPattern) -> usize {
    match pat {
        AstPattern::ListCons(_, head, tail) => 1 + pattern_depth(head).max(pattern_depth(tail)),
        AstPattern::Constructor(_, _, inners)
        | AstPattern::Call(_, _, inners)
        | AstPattern::Tuple(_, inners)
        | AstPattern::Or(_, inners) => 1 + inners.iter().map(pattern_depth).max().unwrap_or(0),
        AstPattern::As(_, inner, _, _) => 1 + pattern_depth(inner),
        _ => 1,
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
        AstPattern::Or(span, items) => AstPattern::Or(
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
        Ast::LiftedCompose(span, left, right) => Ast::LiftedCompose(
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
        Ast::Grouped(span, inner) => Ast::Grouped(
            shift_span(span, delta),
            Box::new(shift_ast_span(*inner, delta)),
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
        Ast::Dbg(span, args) => Ast::Dbg(
            shift_span(span, delta),
            args.into_iter()
                .map(|arg| DbgArg {
                    span: shift_span(arg.span, delta),
                    expr: shift_ast_span(arg.expr, delta),
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
        Ast::ConstDef(span, name, ty, value, attrs) => Ast::ConstDef(
            shift_span(span, delta),
            name,
            ty.map(|ty| shift_ast_ty(ty, delta)),
            Box::new(shift_ast_span(*value, delta)),
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
        Ast::IntrinsicDecl(span, name, signature, attrs) => Ast::IntrinsicDecl(
            shift_span(span, delta),
            name,
            signature,
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
        Ast::Namespace(span, name, body) => Ast::Namespace(
            shift_span(span, delta),
            name,
            body.into_iter()
                .map(|stmt| shift_ast_span(stmt, delta))
                .collect(),
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
        Ast::ImplDef(span, target, methods, attrs) => Ast::ImplDef(
            shift_span(span, delta),
            target,
            methods
                .into_iter()
                .map(|method| shift_ast_span(method, delta))
                .collect(),
            shift_decl_attrs(attrs),
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
        Ast::TraitImplDef(span, trait_name, trait_args, target, methods, attrs) => {
            Ast::TraitImplDef(
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
                shift_decl_attrs(attrs),
            )
        }
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
        Ast::FuncLiteralRef(span, func) => Ast::FuncLiteralRef(
            shift_span(span, delta),
            FuncLiteralRef {
                span: shift_span(func.span, delta),
                body: func.body,
            },
        ),
        Ast::CapturePlaceholder(span, index) => {
            Ast::CapturePlaceholder(shift_span(span, delta), index)
        }
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
            | Ast::FuncLiteralRef(s, _)
            | Ast::App(s, _, _)
            | Ast::Block(s, _)
            | Ast::Bind(s, _, _)
            | Ast::SafeBind(s, _, _)
            | Ast::BinOp(s, _, _, _)
            | Ast::Pipe(s, _, _)
            | Ast::ContextMap(s, _, _)
            | Ast::ContextBind(s, _, _)
            | Ast::Compose(s, _, _)
            | Ast::LiftedCompose(s, _, _)
            | Ast::KleisliCompose(s, _, _)
            | Ast::ListNil(s)
            | Ast::ListCons(s, _, _)
            | Ast::ListLiteral(s, _)
            | Ast::TupleLiteral(s, _)
            | Ast::Grouped(s, _)
            | Ast::InterpolatedStr(s, _)
            | Ast::Dbg(s, _)
            | Ast::Match(s, _, _)
            | Ast::FieldAccess(s, _, _)
            | Ast::StructDef(s, _, _)
            | Ast::RecordDef(s, _, _)
            | Ast::StructLit(s, _, _)
            | Ast::ConstructorCall(s, _, _)
            | Ast::DeferrorDef(s, _, _, _, _)
            | Ast::EnumDef(s, _, _, _, _)
            | Ast::Def(s, _, _, _, _, _, _)
            | Ast::ConstDef(s, _, _, _, _)
            | Ast::ExtractorDef(s, _, _, _, _, _, _)
            | Ast::BuiltinDecl(s, _, _, _, _)
            | Ast::IntrinsicDecl(s, _, _, _)
            | Ast::BuiltinExtractorDecl(s, _, _, _, _)
            | Ast::BuiltinTypeDecl(s, _, _)
            | Ast::Namespace(s, _, _)
            | Ast::ResultCtorDecl(s, _, _, _, _)
            | Ast::Defmod(s, _, _, _)
            | Ast::ImplDef(s, _, _, _)
            | Ast::TraitDef(s, _, _, _, _)
            | Ast::TraitImplDef(s, _, _, _, _, _)
            | Ast::Import(s, _, _)
            | Ast::Include(s, _)
            | Ast::Closure(s, _, _)
            | Ast::Capture(s, _, _)
            | Ast::CapturePlaceholder(s, _)
            | Ast::Semi(s, _) => s,
        }
    }
}

#[cfg(test)]
mod tests;
