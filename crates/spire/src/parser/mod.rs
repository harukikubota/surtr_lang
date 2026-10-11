use crate::ast::*;
use crate::error::ParseError;
use crate::lexer::tokenize;
use crate::token::{Spanned, Token};
use std::collections::{HashMap, HashSet, VecDeque};

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
mod tolerant;
mod ty;
mod validate;

pub use completion::{
    parse_incomplete_expr, parse_incomplete_stmt, parse_operator_completion_context,
    CompletionContext, IncompleteParseResult, OperatorCompletionContext, OperatorCompletionStage,
};
pub use context::{ParseRules, ParserContext};
pub use diagnostic::{
    LspDiagnostic, LspDiagnosticSeverity, LspPosition, LspRange, LspRelatedInformation,
    ParseDiagnostic,
};
pub use tolerant::{
    parse_tolerant_with_context, CursorSyntaxContext, SyntaxOutlineItem, SyntaxOutlineKind,
    SyntaxToken, SyntaxTokenKind, TolerantParseResult,
};

/// Fail-closed parser guard for nested syntax, type, and pattern structures.
pub const MAX_PARSE_NESTING: usize = 32;
pub const MAX_PARSE_NESTING_MESSAGE: &str = "maximum parse nesting depth exceeded";
const IMPLICIT_ROOT_NAMESPACE: &str = "Global";

/// Parse Surtr source text into an abstract syntax tree.
pub fn parse(source: &str) -> Result<Vec<Ast>, ParseError> {
    parse_with_context(source, ParserContext::default())
}

/// Parse Surtr source text with explicit compile-unit context.
pub fn parse_with_context(source: &str, context: ParserContext) -> Result<Vec<Ast>, ParseError> {
    let tokens = tokenize(source)?;
    reject_excessive_delimiter_nesting(&tokens)?;
    reject_marker_owner_paths(&tokens)?;
    let ast = chumsky_program::parse_program_with_chumsky(source, &tokens, context.clone())?;
    validate::validate_program_by_context(&context, &ast)?;
    let ast = lower_namespaces(ast)?;
    canonicalize_root_owner_heads(ast)
}

/// Parse Surtr source with parser diagnostic metadata for editor tooling.
pub fn parse_with_context_diagnostic(
    source: &str,
    context: ParserContext,
) -> Result<Vec<Ast>, ParseDiagnostic> {
    let tokens = tokenize(source).map_err(ParseDiagnostic::from)?;
    reject_excessive_delimiter_nesting(&tokens).map_err(ParseDiagnostic::from)?;
    reject_marker_owner_paths(&tokens).map_err(ParseDiagnostic::from)?;
    let ast =
        chumsky_program::parse_program_with_chumsky_diagnostic(source, &tokens, context.clone())
            .map_err(ParseDiagnostic::from)?;
    validate::validate_program_by_context(&context, &ast).map_err(ParseDiagnostic::from)?;
    let ast = lower_namespaces(ast).map_err(ParseDiagnostic::from)?;
    canonicalize_root_owner_heads(ast).map_err(ParseDiagnostic::from)
}

fn reject_excessive_delimiter_nesting(tokens: &[Spanned<Token>]) -> Result<(), ParseError> {
    let mut depth = 0usize;
    for token in tokens {
        match token.token {
            Token::LParen | Token::LBrack | Token::LBrace => {
                depth += 1;
                if depth > MAX_PARSE_NESTING {
                    return Err(ParseError::syntax(
                        crate::error::ParseErrorReason::PositionRule,
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

pub(super) fn reject_marker_owner_paths(tokens: &[Spanned<Token>]) -> Result<(), ParseError> {
    for token in tokens {
        if matches!(
            &token.token,
            Token::FuncLiteral(body)
                if matches!(body.split_once("::"), Some(("Self" | "Type", _)))
        ) {
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PositionRule,
                "`Self` and `Type` are type markers, not value-level call owners",
                token.span.clone(),
            ));
        }
    }

    for window in tokens.windows(4) {
        let is_marker = matches!(&window[0].token, Token::Ident(name) if name == "Self")
            || matches!(window[0].token, Token::Type);
        if is_marker
            && matches!(window[1].token, Token::Colon)
            && matches!(window[2].token, Token::Colon)
            && matches!(
                window[3].token,
                Token::Ident(_) | Token::ReservedCallName(_) | Token::PatternConsumer(_)
            )
        {
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PositionRule,
                "`Self` and `Type` are type markers, not value-level call owners",
                Span {
                    start: window[0].span.start,
                    end: window[3].span.end,
                },
            ));
        }
    }
    Ok(())
}

#[derive(Clone)]
struct Parser<'a> {
    source: &'a str,
    tokens: &'a [Spanned<Token>],
    synthetic_tokens: VecDeque<Spanned<Token>>,
    pos: usize,
    context: ParserContext,
    impl_target_stack: Vec<Symbol>,
    allow_trailing_call_block: bool,
    parse_nesting_depth: usize,
    pending_pipe_outer_call: bool,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: &'a [Spanned<Token>], context: ParserContext) -> Self {
        Self {
            source,
            tokens,
            synthetic_tokens: VecDeque::new(),
            pos: 0,
            context,
            impl_target_stack: Vec::new(),
            allow_trailing_call_block: true,
            parse_nesting_depth: 0,
            pending_pipe_outer_call: false,
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
                crate::error::ParseErrorReason::PositionRule,
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
            Token::GtEq => {
                let combined = self.advance().span;
                let gt = Span {
                    start: combined.start,
                    end: combined.start + 1,
                };
                let bind = Span {
                    start: combined.start + 1,
                    end: combined.end,
                };
                self.synthetic_tokens.push_front(Spanned {
                    token: Token::Bind,
                    span: bind,
                });
                Ok(gt)
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
                crate::error::ParseErrorReason::PositionRule,
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
            Token::Reflection(value) => Err(ParseError::syntax(
                crate::error::ParseErrorReason::DeclarationSyntax,
                format!("{} is reserved for source reflection", value.name()),
                sp,
            )),
            Token::ReservedEnv => Err(ParseError::syntax(
                crate::error::ParseErrorReason::DeclarationSyntax,
                "`__ENV__` is reserved and cannot be defined",
                sp,
            )),
            Token::Eof => Err(ParseError::incomplete("identifier", sp)),
            _ => Err(ParseError::syntax(
                crate::error::ParseErrorReason::PositionRule,
                format!("Expected identifier, got {:?}", self.peek()),
                sp,
            )),
        }
    }

    fn expect_callable_ident(&mut self) -> Result<(Symbol, Span), ParseError> {
        if let Token::ReservedCallName(kind) = self.peek().clone() {
            let span = self.advance().span;
            return Ok((kind.name().to_string(), span));
        }
        self.expect_ident()
    }

    /// Only callable positions opt into suffix parsing; ordinary identifiers stay unchanged.
    fn parse_callable_suffix(&mut self, mut name: Symbol, mut span: Span) -> (Symbol, Span) {
        if matches!(self.peek(), Token::Question) && span.end == self.peek_span().start {
            name.push('?');
            span.end = self.advance().span.end;
        }
        (name, span)
    }

    fn expect_member_ident(&mut self) -> Result<(Symbol, Span), ParseError> {
        if matches!(self.peek(), Token::True | Token::False) {
            let token = self.advance();
            return Ok((
                if matches!(token.token, Token::True) {
                    "True"
                } else {
                    "False"
                }
                .into(),
                token.span,
            ));
        }
        if let Token::PatternConsumer(kind) = self.peek().clone() {
            let span = self.advance().span;
            return Ok((kind.name().to_string(), span));
        }
        self.expect_callable_ident()
    }

    fn numbered_placeholder_index(digits: &str, span: Span) -> Result<u8, ParseError> {
        digits
            .parse::<u8>()
            .ok()
            .filter(|index| (1..=sindr::pattern::MAX_PROJECTION_INDEX).contains(index))
            .ok_or_else(|| {
                ParseError::syntax(
                    crate::error::ParseErrorReason::PatternSyntax,
                    "numbered placeholder index must be between _1 and _16",
                    span,
                )
            })
    }

    fn expect_builtin_decl_name(&mut self) -> Result<(Symbol, Span), ParseError> {
        let sp = self.peek_span();
        match self.peek().clone() {
            Token::ReservedCallName(kind) => {
                self.advance();
                Ok((kind.name().to_string(), sp))
            }
            Token::PipeApply => {
                self.advance();
                Ok(("|>".to_string(), sp))
            }
            Token::Compose => {
                self.advance();
                Ok((">>".to_string(), sp))
            }
            Token::LiftCompose => {
                self.advance();
                Ok((">*".to_string(), sp))
            }
            Token::KleisliCompose => {
                self.advance();
                Ok((">=>".to_string(), sp))
            }
            Token::PatternConsumer(kind) => {
                self.advance();
                Ok((kind.name().to_string(), sp))
            }
            Token::LParen
                if matches!(
                    (self.peek_n(1), self.peek_n(2)),
                    (Some(Token::Comma), Some(Token::RParen))
                ) =>
            {
                self.advance();
                self.advance();
                let end = self.advance().span.end;
                Ok((
                    "(,)".to_string(),
                    Span {
                        start: sp.start,
                        end,
                    },
                ))
            }
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
                crate::error::ParseErrorReason::PositionRule,
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

    fn source_text_for_span(&self, span: &Span) -> String {
        self.source
            .chars()
            .skip(span.start)
            .take(span.end.saturating_sub(span.start))
            .collect()
    }

    fn with_parse_nesting<T>(
        &mut self,
        span: Span,
        f: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        self.parse_nesting_depth += 1;
        if self.parse_nesting_depth > MAX_PARSE_NESTING {
            self.parse_nesting_depth -= 1;
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PositionRule,
                MAX_PARSE_NESTING_MESSAGE,
                span,
            ));
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
            Ast::Bind(_, _, rhs)
            | Ast::SafeBind(_, _, rhs, _)
            | Ast::StatementQuestion(_, rhs)
            | Ast::Semi(_, rhs) => Self::anonymous_callable_call_target(rhs),
            Ast::Capture(_, _, _)
            | Ast::FacetCapture(_, _)
            | Ast::Closure(_, _, _)
            | Ast::ExtractorClosure(_, _, _)
            | Ast::Grouped(_, _)
            | Ast::FacetSegmentAccess(_, _, _)
            | Ast::App(_, _, _)
            | Ast::PatternConsumerCall(..) => Some(stmt),
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
            return Err(ParseError::syntax(crate::error::ParseErrorReason::PositionRule,
                "Immediate calls on anonymous callable expressions are not supported; bind the callable to a name and call it as `fn(args)`",
                self.peek_span(),
            ).with_guidance(crate::error::ParseErrorGuidance::ImmediateAnonymousCall));
        }
        if matches!(self.peek(), Token::DotDot) {
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PositionRule,
                "Range literals must use bracket syntax",
                self.peek_span(),
            )
            .with_guidance(crate::error::ParseErrorGuidance::RangeLiteral));
        }
        let ok = matches!(self.peek(), Token::Newline | Token::Eof)
            || (allow_rbrace && matches!(self.peek(), Token::RBrace));
        if ok {
            Ok(())
        } else {
            Err(ParseError::syntax(
                crate::error::ParseErrorReason::PositionRule,
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
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PositionRule,
                "Expected `::`",
                self.peek_span(),
            ));
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
                    crate::error::ParseErrorReason::PositionRule,
                    format!("{label} path must not exceed {max_segments} segments"),
                    Span { start, end },
                ));
            }
        }
        if segments.len() > 1 && segments[0] == IMPLICIT_ROOT_NAMESPACE {
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PositionRule,
                format!(
                    "{label} path must not explicitly use reserved root namespace `{IMPLICIT_ROOT_NAMESPACE}`"
                ),
                Span { start, end },
            ));
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
    validate_top_level_namespace_owner_collisions(&ast)?;
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
                    crate::error::ParseErrorReason::PositionRule,
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
            qualify_namespace_head(namespace, &name, 2, &span, "module", true)?,
            body,
            attrs,
        )),
        Ast::Defagent(span, name, body, mut process_spec, attrs) => {
            let qualified = qualify_namespace_head(namespace, &name, 2, &span, "process", true)?;
            process_spec.process_name = qualified.clone();
            process_spec.state = qualify_namespace_type(process_spec.state, namespace)?;
            Ok(Ast::Defagent(
                span,
                qualified.clone(),
                rewrite_process_owner_refs_in_body(body, &name, &qualified),
                process_spec,
                attrs,
            ))
        }
        Ast::Defgenserver(span, name, body, mut process_spec, attrs) => {
            let qualified = qualify_namespace_head(namespace, &name, 2, &span, "process", true)?;
            process_spec.process_name = qualified.clone();
            process_spec.state = qualify_namespace_type(process_spec.state, namespace)?;
            Ok(Ast::Defgenserver(
                span,
                qualified.clone(),
                rewrite_process_owner_refs_in_body(body, &name, &qualified),
                process_spec,
                attrs,
            ))
        }
        Ast::Defsupervisor(span, name, body, mut process_spec, attrs) => {
            let qualified = qualify_namespace_head(namespace, &name, 2, &span, "process", true)?;
            process_spec.process_name = qualified.clone();
            process_spec.state = qualify_namespace_type(process_spec.state, namespace)?;
            Ok(Ast::Defsupervisor(
                span,
                qualified.clone(),
                rewrite_process_owner_refs_in_body(body, &name, &qualified),
                process_spec,
                attrs,
            ))
        }
        Ast::DefdynamicSupervisor(span, name, body, mut process_spec, attrs) => {
            let qualified = qualify_namespace_head(namespace, &name, 2, &span, "process", true)?;
            process_spec.process_name = qualified.clone();
            process_spec.state = qualify_namespace_type(process_spec.state, namespace)?;
            Ok(Ast::DefdynamicSupervisor(
                span,
                qualified.clone(),
                rewrite_process_owner_refs_in_body(body, &name, &qualified),
                process_spec,
                attrs,
            ))
        }
        Ast::ImplDef(span, target, target_span, methods, attrs) => Ok(Ast::ImplDef(
            span.clone(),
            qualify_namespace_head(namespace, &target, 2, &span, "impl target", false)?,
            target_span,
            methods,
            attrs,
        )),
        Ast::TraitDef(span, name, type_params, where_clause, methods, attrs) => Ok(Ast::TraitDef(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "trait", false)?,
            type_params,
            where_clause
                .map(|clause| qualify_namespace_where_clause(clause, namespace))
                .transpose()?,
            methods,
            attrs,
        )),
        Ast::TraitImplDef(
            span,
            trait_name,
            trait_args,
            target_ty,
            where_clause,
            methods,
            attrs,
        ) => Ok(Ast::TraitImplDef(
            span.clone(),
            qualify_namespace_head(namespace, &trait_name, 2, &span, "trait", false)?,
            trait_args,
            qualify_namespace_type(target_ty, namespace)?,
            where_clause
                .map(|clause| qualify_namespace_where_clause(clause, namespace))
                .transpose()?,
            methods,
            attrs,
        )),
        Ast::StructDef(span, name, type_params, fields, attrs) => Ok(Ast::StructDef(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "type", true)?,
            type_params,
            fields,
            attrs,
        )),
        Ast::RecordDef(span, name, fields, attrs) => Ok(Ast::RecordDef(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "type", true)?,
            fields,
            attrs,
        )),
        Ast::DeferrorDef(span, name, fields, show_expr, attrs) => Ok(Ast::DeferrorDef(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "type", true)?,
            fields,
            show_expr,
            attrs,
        )),
        Ast::EnumDef(span, name, type_params, variants, attrs) => Ok(Ast::EnumDef(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "type", true)?,
            type_params,
            variants,
            attrs,
        )),
        Ast::BuiltinTypeDecl(span, mut head, attrs) => {
            head.name = qualify_namespace_head(namespace, &head.name, 2, &span, "type", true)?;
            Ok(Ast::BuiltinTypeDecl(span, head, attrs))
        }
        Ast::TypeAlias(span, name, type_params, rhs) => Ok(Ast::TypeAlias(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "type alias", true)?,
            type_params,
            qualify_namespace_type(rhs, namespace)?,
        )),
        Ast::Namespace(span, _, _) => Err(ParseError::syntax(
            crate::error::ParseErrorReason::PositionRule,
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
    reject_same_tail_as_namespace: bool,
) -> Result<String, ParseError> {
    let segments = name.split("::").collect::<Vec<_>>();
    if segments.len() > max_segments {
        return Err(ParseError::syntax(
            crate::error::ParseErrorReason::PositionRule,
            format!("{label} path must not exceed {max_segments} segments"),
            span.clone(),
        ));
    }
    if reject_same_tail_as_namespace && segments.len() == 1 && segments[0] == namespace {
        return Err(ParseError::syntax(
            crate::error::ParseErrorReason::PositionRule,
            format!("{label} name `{name}` conflicts with active namespace `{namespace}`"),
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
                    qualify_namespace_head(namespace, &name, 2, &span, "type", false)?,
                ))
            }
        }
        AstTy::ImplTrait(span, name) => Ok(AstTy::ImplTrait(
            span.clone(),
            qualify_namespace_head(namespace, &name, 2, &span, "trait", false)?,
        )),
        AstTy::Generic(span, name, args) => {
            let name = if name == "Self" || name.starts_with('$') {
                name
            } else {
                qualify_namespace_head(namespace, &name, 2, &span, "type", false)?
            };
            Ok(AstTy::Generic(
                span,
                name,
                args.into_iter()
                    .map(|arg| qualify_namespace_type(arg, namespace))
                    .collect::<Result<Vec<_>, ParseError>>()?,
            ))
        }
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

fn qualify_namespace_where_clause(
    clause: WhereClause,
    namespace: &str,
) -> Result<WhereClause, ParseError> {
    Ok(WhereClause {
        span: clause.span,
        constraints: clause
            .constraints
            .into_iter()
            .map(|constraint| {
                Ok(WhereConstraint {
                    subject: qualify_namespace_type(constraint.subject, namespace)?,
                    bounds: constraint
                        .bounds
                        .into_iter()
                        .map(|bound| match bound {
                            WhereConstraintRhs::Trait(span, name) => {
                                Ok(WhereConstraintRhs::Trait(span, name))
                            }
                            WhereConstraintRhs::TypeConstructor(span, slots) => {
                                Ok(WhereConstraintRhs::TypeConstructor(
                                    span,
                                    slots
                                        .into_iter()
                                        .map(|slot| qualify_namespace_type(slot, namespace))
                                        .collect::<Result<Vec<_>, ParseError>>()?,
                                ))
                            }
                            WhereConstraintRhs::TraitSlot(span, name, slot) => {
                                Ok(WhereConstraintRhs::TraitSlot(span, name, slot))
                            }
                        })
                        .collect::<Result<Vec<_>, ParseError>>()?,
                    span: constraint.span,
                })
            })
            .collect::<Result<Vec<_>, ParseError>>()?,
    })
}

fn owner_head_name(node: &Ast) -> Option<&str> {
    match node {
        Ast::Defmod(_, name, _, _)
        | Ast::Defagent(_, name, _, _, _)
        | Ast::Defgenserver(_, name, _, _, _)
        | Ast::Defsupervisor(_, name, _, _, _)
        | Ast::DefdynamicSupervisor(_, name, _, _, _)
        | Ast::StructDef(_, name, ..)
        | Ast::RecordDef(_, name, _, _)
        | Ast::DeferrorDef(_, name, _, _, _)
        | Ast::EnumDef(_, name, _, _, _) => Some(name.as_str()),
        _ => None,
    }
}

fn canonicalize_root_owner_name(name: &str) -> String {
    if name.contains("::") {
        name.to_string()
    } else {
        format!("{IMPLICIT_ROOT_NAMESPACE}::{name}")
    }
}

fn canonicalize_root_owner_heads(ast: Vec<Ast>) -> Result<Vec<Ast>, ParseError> {
    ast.into_iter()
        .map(|node| match node {
            Ast::Defmod(span, name, body, attrs) => Ok(Ast::Defmod(
                span,
                canonicalize_root_owner_name(&name),
                body,
                attrs,
            )),
            Ast::Defagent(span, name, body, mut process_spec, attrs) => {
                let canonical_name = canonicalize_root_owner_name(&name);
                process_spec.process_name = canonical_name.clone();
                process_spec.state =
                    rewrite_process_owner_ty(process_spec.state, &name, &canonical_name);
                Ok(Ast::Defagent(
                    span,
                    canonical_name.clone(),
                    rewrite_process_owner_refs_in_body(body, &name, &canonical_name),
                    process_spec,
                    attrs,
                ))
            }
            Ast::Defgenserver(span, name, body, mut process_spec, attrs) => {
                let canonical_name = canonicalize_root_owner_name(&name);
                process_spec.process_name = canonical_name.clone();
                process_spec.state =
                    rewrite_process_owner_ty(process_spec.state, &name, &canonical_name);
                Ok(Ast::Defgenserver(
                    span,
                    canonical_name.clone(),
                    rewrite_process_owner_refs_in_body(body, &name, &canonical_name),
                    process_spec,
                    attrs,
                ))
            }
            Ast::Defsupervisor(span, name, body, mut process_spec, attrs) => {
                let canonical_name = canonicalize_root_owner_name(&name);
                process_spec.process_name = canonical_name.clone();
                process_spec.state =
                    rewrite_process_owner_ty(process_spec.state, &name, &canonical_name);
                Ok(Ast::Defsupervisor(
                    span,
                    canonical_name.clone(),
                    rewrite_process_owner_refs_in_body(body, &name, &canonical_name),
                    process_spec,
                    attrs,
                ))
            }
            Ast::DefdynamicSupervisor(span, name, body, mut process_spec, attrs) => {
                let canonical_name = canonicalize_root_owner_name(&name);
                process_spec.process_name = canonical_name.clone();
                process_spec.state =
                    rewrite_process_owner_ty(process_spec.state, &name, &canonical_name);
                Ok(Ast::DefdynamicSupervisor(
                    span,
                    canonical_name.clone(),
                    rewrite_process_owner_refs_in_body(body, &name, &canonical_name),
                    process_spec,
                    attrs,
                ))
            }
            Ast::StructDef(span, name, type_params, fields, attrs) => Ok(Ast::StructDef(
                span,
                canonicalize_root_owner_name(&name),
                type_params,
                fields,
                attrs,
            )),
            Ast::RecordDef(span, name, fields, attrs) => Ok(Ast::RecordDef(
                span,
                canonicalize_root_owner_name(&name),
                fields,
                attrs,
            )),
            Ast::DeferrorDef(span, name, fields, show_expr, attrs) => Ok(Ast::DeferrorDef(
                span,
                canonicalize_root_owner_name(&name),
                fields,
                show_expr,
                attrs,
            )),
            Ast::EnumDef(span, name, type_params, variants, attrs) => Ok(Ast::EnumDef(
                span,
                canonicalize_root_owner_name(&name),
                type_params,
                variants,
                attrs,
            )),
            other => Ok(other),
        })
        .collect()
}

fn rewrite_process_owner_refs_in_body(body: Vec<Ast>, old_name: &str, new_name: &str) -> Vec<Ast> {
    body.into_iter()
        .map(|node| rewrite_process_owner_refs(node, old_name, new_name))
        .collect()
}

fn rewrite_facet_path_segment_refs(
    segment: FacetPathSegment,
    old_name: &str,
    new_name: &str,
) -> FacetPathSegment {
    match segment {
        FacetPathSegment::Field { .. } => segment,
        FacetPathSegment::Bracket(expr) => FacetPathSegment::Bracket(FacetBracketExpr {
            expr: Box::new(rewrite_process_owner_refs(*expr.expr, old_name, new_name)),
            display: expr.display,
        }),
    }
}

fn rewrite_bulk_update_path_refs(
    path: BulkUpdatePath,
    old_name: &str,
    new_name: &str,
) -> BulkUpdatePath {
    match path {
        BulkUpdatePath::Segments(span, segments) => BulkUpdatePath::Segments(
            span,
            segments
                .into_iter()
                .map(|segment| rewrite_facet_path_segment_refs(segment, old_name, new_name))
                .collect(),
        ),
        BulkUpdatePath::Pin(span, name) => BulkUpdatePath::Pin(span, name),
        BulkUpdatePath::Chain(span, left, right) => BulkUpdatePath::Chain(
            span,
            Box::new(rewrite_bulk_update_path_refs(*left, old_name, new_name)),
            Box::new(rewrite_bulk_update_path_refs(*right, old_name, new_name)),
        ),
        BulkUpdatePath::StripLeft(span, inner, count) => BulkUpdatePath::StripLeft(
            span,
            Box::new(rewrite_bulk_update_path_refs(*inner, old_name, new_name)),
            count,
        ),
        BulkUpdatePath::StripRight(span, inner, count) => BulkUpdatePath::StripRight(
            span,
            Box::new(rewrite_bulk_update_path_refs(*inner, old_name, new_name)),
            count,
        ),
    }
}

fn rewrite_process_owner_bulk_entries(
    entries: Vec<BulkUpdateEntry>,
    old_name: &str,
    new_name: &str,
) -> Vec<BulkUpdateEntry> {
    entries
        .into_iter()
        .map(|entry| BulkUpdateEntry {
            span: entry.span,
            path: rewrite_bulk_update_path_refs(entry.path, old_name, new_name),
            kind: match entry.kind {
                BulkUpdateEntryKind::Set(expr) => {
                    BulkUpdateEntryKind::Set(rewrite_process_owner_refs(expr, old_name, new_name))
                }
                BulkUpdateEntryKind::Over(expr) => {
                    BulkUpdateEntryKind::Over(rewrite_process_owner_refs(expr, old_name, new_name))
                }
                BulkUpdateEntryKind::OverResult(expr) => BulkUpdateEntryKind::OverResult(
                    rewrite_process_owner_refs(expr, old_name, new_name),
                ),
                BulkUpdateEntryKind::CaseSet(expr) => BulkUpdateEntryKind::CaseSet(
                    rewrite_process_owner_refs(expr, old_name, new_name),
                ),
                BulkUpdateEntryKind::CaseOver(expr) => BulkUpdateEntryKind::CaseOver(
                    rewrite_process_owner_refs(expr, old_name, new_name),
                ),
                BulkUpdateEntryKind::Nested(entries) => BulkUpdateEntryKind::Nested(
                    rewrite_process_owner_bulk_entries(entries, old_name, new_name),
                ),
            },
        })
        .collect()
}

fn rewrite_process_owner_refs(node: Ast, old_name: &str, new_name: &str) -> Ast {
    match node {
        Ast::PatternConsumerCall(span, callee, args) => Ast::PatternConsumerCall(
            span,
            Box::new(rewrite_process_owner_refs(*callee, old_name, new_name)),
            args.into_iter()
                .map(|mut arg| {
                    arg.expression = arg.expression.map(|expr| {
                        Box::new(rewrite_process_owner_refs(*expr, old_name, new_name))
                    });
                    arg.pattern = arg.pattern.map(|pattern| {
                        Box::new(rewrite_process_owner_pattern(*pattern, old_name, new_name))
                    });
                    arg
                })
                .collect(),
        ),

        Ast::App(span, func, args) => {
            let func = Box::new(rewrite_process_owner_refs(*func, old_name, new_name));
            let args = rewrite_process_owner_call_args(
                func.as_ref(),
                args.into_iter()
                    .map(|arg| rewrite_process_owner_record_lit_arg(arg, old_name, new_name))
                    .collect(),
                old_name,
                new_name,
            );
            Ast::App(span, func, args)
        }
        Ast::Block(span, body) => Ast::Block(
            span,
            rewrite_process_owner_refs_in_body(body, old_name, new_name),
        ),
        Ast::Do(span, return_type_arguments, statements, keyword_span) => Ast::Do(
            span,
            return_type_arguments
                .into_iter()
                .map(|argument| ReturnTypeArgument {
                    ordinal: argument.ordinal,
                    ty: rewrite_process_owner_ty(argument.ty, old_name, new_name),
                    span: argument.span,
                })
                .collect(),
            statements
                .into_iter()
                .map(|statement| match statement {
                    AstDoStatement::Extract {
                        span,
                        operator_span,
                        pattern,
                        rhs,
                    } => AstDoStatement::Extract {
                        span,
                        operator_span,
                        pattern: rewrite_process_owner_pattern(pattern, old_name, new_name),
                        rhs: rewrite_process_owner_refs(rhs, old_name, new_name),
                    },
                    AstDoStatement::SafeBind {
                        span,
                        operator_span,
                        pattern,
                        rhs,
                    } => AstDoStatement::SafeBind {
                        span,
                        operator_span,
                        pattern: rewrite_process_owner_pattern(pattern, old_name, new_name),
                        rhs: rewrite_process_owner_refs(rhs, old_name, new_name),
                    },
                    AstDoStatement::Statement(statement) => AstDoStatement::Statement(
                        rewrite_process_owner_refs(statement, old_name, new_name),
                    ),
                })
                .collect(),
            keyword_span,
        ),
        Ast::Bind(span, pattern, expr) => Ast::Bind(
            span,
            rewrite_process_owner_pattern(pattern, old_name, new_name),
            Box::new(rewrite_process_owner_refs(*expr, old_name, new_name)),
        ),
        Ast::SafeBind(span, pattern, expr, operator_span) => Ast::SafeBind(
            span,
            rewrite_process_owner_pattern(pattern, old_name, new_name),
            Box::new(rewrite_process_owner_refs(*expr, old_name, new_name)),
            operator_span,
        ),
        Ast::BinOp(span, op, lhs, rhs) => Ast::BinOp(
            span,
            op,
            Box::new(rewrite_process_owner_refs(*lhs, old_name, new_name)),
            Box::new(rewrite_process_owner_refs(*rhs, old_name, new_name)),
        ),
        Ast::Pipe(span, lhs, rhs) => Ast::Pipe(
            span,
            Box::new(rewrite_process_owner_refs(*lhs, old_name, new_name)),
            Box::new(rewrite_process_owner_refs(*rhs, old_name, new_name)),
        ),
        Ast::ContextMap(span, lhs, rhs) => Ast::ContextMap(
            span,
            Box::new(rewrite_process_owner_refs(*lhs, old_name, new_name)),
            Box::new(rewrite_process_owner_refs(*rhs, old_name, new_name)),
        ),
        Ast::ContextBind(span, lhs, rhs) => Ast::ContextBind(
            span,
            Box::new(rewrite_process_owner_refs(*lhs, old_name, new_name)),
            Box::new(rewrite_process_owner_refs(*rhs, old_name, new_name)),
        ),
        Ast::Compose(span, lhs, rhs) => Ast::Compose(
            span,
            Box::new(rewrite_process_owner_refs(*lhs, old_name, new_name)),
            Box::new(rewrite_process_owner_refs(*rhs, old_name, new_name)),
        ),
        Ast::LiftedCompose(span, lhs, rhs) => Ast::LiftedCompose(
            span,
            Box::new(rewrite_process_owner_refs(*lhs, old_name, new_name)),
            Box::new(rewrite_process_owner_refs(*rhs, old_name, new_name)),
        ),
        Ast::KleisliCompose(span, lhs, rhs) => Ast::KleisliCompose(
            span,
            Box::new(rewrite_process_owner_refs(*lhs, old_name, new_name)),
            Box::new(rewrite_process_owner_refs(*rhs, old_name, new_name)),
        ),
        Ast::ListCons(span, head, tail) => Ast::ListCons(
            span,
            Box::new(rewrite_process_owner_refs(*head, old_name, new_name)),
            Box::new(rewrite_process_owner_refs(*tail, old_name, new_name)),
        ),
        Ast::ListLiteral(span, items) => Ast::ListLiteral(
            span,
            items
                .into_iter()
                .map(|item| rewrite_process_owner_refs(item, old_name, new_name))
                .collect(),
        ),
        Ast::RangeLiteral(span, start, end) => Ast::RangeLiteral(
            span,
            Box::new(rewrite_process_owner_refs(*start, old_name, new_name)),
            Box::new(rewrite_process_owner_refs(*end, old_name, new_name)),
        ),
        Ast::TupleLiteral(span, items) => Ast::TupleLiteral(
            span,
            items
                .into_iter()
                .map(|item| rewrite_process_owner_refs(item, old_name, new_name))
                .collect(),
        ),
        Ast::Cond(span, clauses) => Ast::Cond(
            span,
            clauses
                .into_iter()
                .map(|(condition, body)| {
                    (
                        rewrite_process_owner_refs(condition, old_name, new_name),
                        rewrite_process_owner_refs(body, old_name, new_name),
                    )
                })
                .collect(),
        ),
        Ast::Grouped(span, expr) => Ast::Grouped(
            span,
            Box::new(rewrite_process_owner_refs(*expr, old_name, new_name)),
        ),
        Ast::InterpolatedStr(span, parts) => Ast::InterpolatedStr(
            span,
            parts
                .into_iter()
                .map(|part| match part {
                    InterpolatedPart::Expr(expr) => InterpolatedPart::Expr(Box::new(
                        rewrite_process_owner_refs(*expr, old_name, new_name),
                    )),
                    other => other,
                })
                .collect(),
        ),
        Ast::Dbg(span, args) => Ast::Dbg(
            span,
            args.into_iter()
                .map(|arg| DbgArg {
                    span: arg.span,
                    expr: rewrite_process_owner_refs(arg.expr, old_name, new_name),
                })
                .collect(),
        ),
        Ast::Match(span, expr, arms) => Ast::Match(
            span,
            Box::new(rewrite_process_owner_refs(*expr, old_name, new_name)),
            arms.into_iter()
                .map(|arm| AstMatchArm {
                    pattern: rewrite_process_owner_pattern(arm.pattern, old_name, new_name),
                    guard: arm
                        .guard
                        .map(|guard| rewrite_process_owner_refs(guard, old_name, new_name)),
                    body: rewrite_process_owner_refs(arm.body, old_name, new_name),
                })
                .collect(),
        ),
        Ast::BulkUpdate(span, source, entries) => Ast::BulkUpdate(
            span,
            Box::new(rewrite_process_owner_refs(*source, old_name, new_name)),
            rewrite_process_owner_bulk_entries(entries, old_name, new_name),
        ),
        Ast::FieldAccess(span, expr, field) => Ast::FieldAccess(
            span,
            Box::new(rewrite_process_owner_refs(*expr, old_name, new_name)),
            field,
        ),
        Ast::FacetSegmentAccess(span, expr, segment) => Ast::FacetSegmentAccess(
            span,
            Box::new(rewrite_process_owner_refs(*expr, old_name, new_name)),
            rewrite_facet_path_segment_refs(segment, old_name, new_name),
        ),
        Ast::StructLit(span, name, fields) => Ast::StructLit(
            span,
            rewrite_process_owner_symbol(name, old_name, new_name),
            fields
                .into_iter()
                .map(|field| match field {
                    StructLitField::Explicit(name, value) => StructLitField::Explicit(
                        name,
                        rewrite_process_owner_refs(value, old_name, new_name),
                    ),
                    StructLitField::Shorthand(name) => StructLitField::Shorthand(name),
                })
                .collect(),
        ),
        Ast::InternalStructLit(span, name, fields) => Ast::InternalStructLit(
            span,
            rewrite_process_owner_symbol(name, old_name, new_name),
            fields
                .into_iter()
                .map(|field| match field {
                    StructLitField::Explicit(name, value) => StructLitField::Explicit(
                        name,
                        rewrite_process_owner_refs(value, old_name, new_name),
                    ),
                    StructLitField::Shorthand(name) => StructLitField::Shorthand(name),
                })
                .collect(),
        ),
        Ast::ConstructorCall(span, name, args) => Ast::ConstructorCall(
            span,
            rewrite_process_owner_symbol(name, old_name, new_name),
            args.into_iter()
                .map(|arg| rewrite_process_owner_record_lit_arg(arg, old_name, new_name))
                .collect(),
        ),
        Ast::EnumConstructorCall(span, owner, type_args, variant, args) => {
            Ast::EnumConstructorCall(
                span,
                rewrite_process_owner_symbol(owner, old_name, new_name),
                type_args
                    .into_iter()
                    .map(|ty| rewrite_process_owner_ty(ty, old_name, new_name))
                    .collect(),
                variant,
                args.into_iter()
                    .map(|arg| rewrite_process_owner_record_lit_arg(arg, old_name, new_name))
                    .collect(),
            )
        }
        Ast::DeferrorDef(span, name, fields, show_expr, attrs) => Ast::DeferrorDef(
            span,
            name,
            fields,
            Box::new(rewrite_process_owner_refs(*show_expr, old_name, new_name)),
            attrs,
        ),
        Ast::Def(span, name, type_params, params, ret_ty, where_clause, body, attrs) => Ast::Def(
            span,
            name,
            type_params,
            params
                .into_iter()
                .map(|param| ValueParameter {
                    name: param.name,
                    mode: param.mode,
                    ty: rewrite_process_owner_ty(param.ty, old_name, new_name),
                    span: param.span,
                })
                .collect(),
            ret_ty.map(|ty| rewrite_process_owner_ty(ty, old_name, new_name)),
            where_clause
                .map(|clause| rewrite_process_owner_where_clause(clause, old_name, new_name)),
            Box::new(rewrite_process_owner_refs(*body, old_name, new_name)),
            attrs,
        ),
        Ast::ConstDef(span, name, ty, expr, attrs) => Ast::ConstDef(
            span,
            name,
            ty.map(|ty| rewrite_process_owner_ty(ty, old_name, new_name)),
            Box::new(rewrite_process_owner_refs(*expr, old_name, new_name)),
            attrs,
        ),
        Ast::SupervisorInit(span, mut spec) => {
            for entry in &mut spec.entries {
                if entry.process_name == old_name {
                    entry.process_name = new_name.to_string();
                }
            }
            for singleton in &mut spec.singletons {
                if singleton.process_name == old_name {
                    singleton.process_name = new_name.to_string();
                }
            }
            for supervisor in &mut spec.supervisors {
                if supervisor.process_name == old_name {
                    supervisor.process_name = new_name.to_string();
                }
            }
            Ast::SupervisorInit(span, spec)
        }
        Ast::ExtractorDef(span, name, type_params, param, ret_ty, body, attrs) => {
            Ast::ExtractorDef(
                span,
                name,
                type_params,
                param
                    .into_iter()
                    .map(|param| ExtractorParam {
                        name: param.name,
                        ty: param
                            .ty
                            .map(|ty| rewrite_process_owner_ty(ty, old_name, new_name)),
                        span: param.span,
                    })
                    .collect(),
                rewrite_process_owner_ty(ret_ty, old_name, new_name),
                Box::new(rewrite_process_owner_refs(*body, old_name, new_name)),
                attrs,
            )
        }
        Ast::BuiltinDecl(
            span,
            name,
            return_type_arguments,
            params,
            ret_ty,
            where_clause,
            attrs,
        ) => Ast::BuiltinDecl(
            span,
            name,
            return_type_arguments,
            params
                .into_iter()
                .map(|param| ValueParameter {
                    name: param.name,
                    mode: param.mode,
                    ty: rewrite_process_owner_ty(param.ty, old_name, new_name),
                    span: param.span,
                })
                .collect(),
            ret_ty.map(|ty| rewrite_process_owner_ty(ty, old_name, new_name)),
            where_clause,
            attrs,
        ),
        Ast::BuiltinExtractorDecl(span, name, param, ret_ty, attrs) => Ast::BuiltinExtractorDecl(
            span,
            name,
            param
                .into_iter()
                .map(|param| ExtractorParam {
                    name: param.name,
                    ty: param
                        .ty
                        .map(|ty| rewrite_process_owner_ty(ty, old_name, new_name)),
                    span: param.span,
                })
                .collect(),
            rewrite_process_owner_ty(ret_ty, old_name, new_name),
            attrs,
        ),
        Ast::ImplDef(span, target, target_span, methods, attrs) => Ast::ImplDef(
            span,
            rewrite_process_owner_symbol(target, old_name, new_name),
            target_span,
            rewrite_process_owner_refs_in_body(methods, old_name, new_name),
            attrs,
        ),
        Ast::TraitImplDef(
            span,
            trait_name,
            trait_args,
            target_ty,
            where_clause,
            methods,
            attrs,
        ) => Ast::TraitImplDef(
            span,
            trait_name,
            trait_args
                .into_iter()
                .map(|ty| rewrite_process_owner_ty(ty, old_name, new_name))
                .collect(),
            rewrite_process_owner_ty(target_ty, old_name, new_name),
            where_clause
                .map(|clause| rewrite_process_owner_where_clause(clause, old_name, new_name)),
            rewrite_process_owner_refs_in_body(methods, old_name, new_name),
            attrs,
        ),
        Ast::Closure(span, params, body) => Ast::Closure(
            span,
            params
                .into_iter()
                .map(|param| ClosureParam {
                    name: param.name,
                    ty: param
                        .ty
                        .map(|ty| rewrite_process_owner_ty(ty, old_name, new_name)),
                    span: param.span,
                })
                .collect(),
            Box::new(rewrite_process_owner_refs(*body, old_name, new_name)),
        ),
        Ast::ExtractorClosure(span, params, body) => Ast::ExtractorClosure(
            span,
            params
                .into_iter()
                .map(|param| ClosureParam {
                    name: param.name,
                    ty: param
                        .ty
                        .map(|ty| rewrite_process_owner_ty(ty, old_name, new_name)),
                    span: param.span,
                })
                .collect(),
            Box::new(rewrite_process_owner_refs(*body, old_name, new_name)),
        ),
        Ast::StatementQuestion(span, expr) => Ast::StatementQuestion(
            span,
            Box::new(rewrite_process_owner_refs(*expr, old_name, new_name)),
        ),
        Ast::Semi(span, expr) => Ast::Semi(
            span,
            Box::new(rewrite_process_owner_refs(*expr, old_name, new_name)),
        ),
        other => other,
    }
}

fn rewrite_process_owner_record_lit_arg(
    arg: RecordLitArg,
    old_name: &str,
    new_name: &str,
) -> RecordLitArg {
    match arg {
        RecordLitArg::Positional(expr) => {
            RecordLitArg::Positional(rewrite_process_owner_refs(expr, old_name, new_name))
        }
        RecordLitArg::Named(name, expr) => {
            RecordLitArg::Named(name, rewrite_process_owner_refs(expr, old_name, new_name))
        }
    }
}

fn rewrite_process_owner_pattern(
    pattern: AstPattern,
    old_name: &str,
    new_name: &str,
) -> AstPattern {
    match pattern {
        AstPattern::Projection {
            span,
            index,
            inner,
            annotation,
        } => AstPattern::Projection {
            span,
            index,
            inner: Box::new(rewrite_process_owner_pattern(*inner, old_name, new_name)),
            annotation: annotation.map(|ty| rewrite_process_owner_ty(ty, old_name, new_name)),
        },
        AstPattern::Constructor(span, name, args) => AstPattern::Constructor(
            span,
            rewrite_process_owner_symbol(name, old_name, new_name),
            args.into_iter()
                .map(|arg| rewrite_process_owner_pattern(arg, old_name, new_name))
                .collect(),
        ),
        AstPattern::HashMap(span, entries) => AstPattern::HashMap(
            span,
            entries
                .into_iter()
                .map(|(key, child)| {
                    (
                        rewrite_process_owner_refs(key, old_name, new_name),
                        rewrite_process_owner_pattern(child, old_name, new_name),
                    )
                })
                .collect(),
        ),
        AstPattern::Tuple(span, items) => AstPattern::Tuple(
            span,
            items
                .into_iter()
                .map(|item| rewrite_process_owner_pattern(item, old_name, new_name))
                .collect(),
        ),
        AstPattern::ListCons(span, head, tail) => AstPattern::ListCons(
            span,
            Box::new(rewrite_process_owner_pattern(*head, old_name, new_name)),
            Box::new(rewrite_process_owner_pattern(*tail, old_name, new_name)),
        ),
        other => other,
    }
}

fn rewrite_process_owner_ty(ty: AstTy, old_name: &str, new_name: &str) -> AstTy {
    match ty {
        AstTy::Named(span, name) => {
            AstTy::Named(span, rewrite_process_owner_symbol(name, old_name, new_name))
        }
        AstTy::ImplTrait(span, name) => {
            AstTy::ImplTrait(span, rewrite_process_owner_symbol(name, old_name, new_name))
        }
        AstTy::Generic(span, name, args) => AstTy::Generic(
            span,
            rewrite_process_owner_symbol(name, old_name, new_name),
            args.into_iter()
                .map(|arg| rewrite_process_owner_ty(arg, old_name, new_name))
                .collect(),
        ),
        AstTy::Tuple(span, items) => AstTy::Tuple(
            span,
            items
                .into_iter()
                .map(|item| rewrite_process_owner_ty(item, old_name, new_name))
                .collect(),
        ),
        AstTy::Func(span, params, ret_ty) => AstTy::Func(
            span,
            params
                .into_iter()
                .map(|param| rewrite_process_owner_ty(param, old_name, new_name))
                .collect(),
            Box::new(rewrite_process_owner_ty(*ret_ty, old_name, new_name)),
        ),
    }
}

fn rewrite_process_owner_where_clause(
    clause: WhereClause,
    old_name: &str,
    new_name: &str,
) -> WhereClause {
    WhereClause {
        span: clause.span,
        constraints: clause
            .constraints
            .into_iter()
            .map(|constraint| WhereConstraint {
                subject: rewrite_process_owner_ty(constraint.subject, old_name, new_name),
                bounds: constraint
                    .bounds
                    .into_iter()
                    .map(|bound| match bound {
                        WhereConstraintRhs::Trait(span, name) => WhereConstraintRhs::Trait(
                            span,
                            rewrite_process_owner_symbol(name, old_name, new_name),
                        ),
                        WhereConstraintRhs::TypeConstructor(span, slots) => {
                            WhereConstraintRhs::TypeConstructor(
                                span,
                                slots
                                    .into_iter()
                                    .map(|slot| rewrite_process_owner_ty(slot, old_name, new_name))
                                    .collect(),
                            )
                        }
                        WhereConstraintRhs::TraitSlot(span, name, slot) => {
                            WhereConstraintRhs::TraitSlot(
                                span,
                                rewrite_process_owner_symbol(name, old_name, new_name),
                                slot,
                            )
                        }
                    })
                    .collect(),
                span: constraint.span,
            })
            .collect(),
    }
}

fn rewrite_process_owner_symbol(name: Symbol, old_name: &str, new_name: &str) -> Symbol {
    if name == old_name {
        new_name.to_string()
    } else {
        name
    }
}

fn rewrite_process_owner_call_args(
    func: &Ast,
    mut args: Vec<RecordLitArg>,
    old_name: &str,
    new_name: &str,
) -> Vec<RecordLitArg> {
    let Some(target) = process_owner_call_target(func) else {
        return args;
    };
    if !matches!(
        target.as_str(),
        "Agent::pid"
            | "GenServer::pid"
            | "Supervisor::spawn"
            | "Supervisor::adopt"
            | "Supervisor::status"
            | "Supervisor::workers"
            | "DynamicSupervisor::spawn"
    ) {
        return args;
    }
    if let Some(RecordLitArg::Positional(Ast::Lit(_, Lit::Str(value)))) = args.first_mut() {
        if value == old_name {
            *value = new_name.to_string();
        }
    }
    args
}

fn process_owner_call_target(func: &Ast) -> Option<String> {
    match func {
        Ast::InternalVar(_, name) | Ast::Var(_, name) => Some(name.clone()),
        Ast::Path(_, path) if path.segments.len() == 2 => Some(path.segments.join("::")),
        _ => None,
    }
}

fn validate_top_level_namespace_owner_collisions(ast: &[Ast]) -> Result<(), ParseError> {
    let mut namespaces = HashMap::<String, Span>::new();
    let mut owner_roots = HashSet::<String>::new();

    for node in ast {
        if let Ast::Namespace(span, name, _) = node {
            if name == IMPLICIT_ROOT_NAMESPACE {
                return Err(ParseError::syntax(
                    crate::error::ParseErrorReason::PositionRule,
                    format!(
                        "`{IMPLICIT_ROOT_NAMESPACE}` is reserved for the implicit root namespace"
                    ),
                    span.clone(),
                ));
            }
            if let Some(existing) = namespaces.get(name) {
                return Err(ParseError::syntax(
                    crate::error::ParseErrorReason::PositionRule,
                    format!("namespace `{name}` is already defined in this scope"),
                    Span {
                        start: existing.start,
                        end: span.end,
                    },
                ));
            }
            namespaces.insert(name.clone(), span.clone());
        }
    }

    for node in ast {
        let Some(owner_name) = owner_head_name(node) else {
            continue;
        };
        let owner_root = owner_name.split("::").next().unwrap_or(owner_name);
        if owner_root == IMPLICIT_ROOT_NAMESPACE {
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PositionRule,
                format!("`{IMPLICIT_ROOT_NAMESPACE}` is reserved for the implicit root namespace"),
                node.span().clone(),
            ));
        }
        if !owner_name.contains("::") {
            owner_roots.insert(owner_root.to_string());
        }
        if namespaces.contains_key(owner_root) && !owner_name.contains("::") {
            return Err(ParseError::syntax(
                crate::error::ParseErrorReason::PositionRule,
                format!("owner name `{owner_name}` conflicts with namespace `{owner_root}`"),
                node.span().clone(),
            ));
        }
    }

    Ok(())
}

/// Materialize source atoms before module span rebasing and name resolution.
/// `file_path` must identify a real file; `None` explicitly denotes virtual input.
pub fn materialize_reflections(
    ast: Vec<Ast>,
    source: &str,
    file_path: Option<&std::path::Path>,
) -> Result<Vec<Ast>, ParseError> {
    struct Materializer<'a> {
        source: &'a str,
        file_path: Option<&'a std::path::Path>,
        error: std::cell::RefCell<Option<ParseError>>,
    }
    impl AstMapper for Materializer<'_> {
        fn span(&self, span: Span) -> Span {
            span
        }
        fn reflection(&self, span: Span, value: sindr::reflection::Reflection) -> Ast {
            use sindr::reflection::Reflection;
            let literal = match value {
                Reflection::Line => Ok(Lit::Int(sindr::primitives::int(
                    1 + self
                        .source
                        .chars()
                        .take(span.start)
                        .filter(|ch| *ch == '\n')
                        .count(),
                ))),
                Reflection::File | Reflection::Dir => (|| {
                    let path = self
                        .file_path
                        .ok_or("source reflection requires a file source")?;
                    // Remove `.` components without resolving symlinks or collapsing `..`.
                    let path = path.components().collect::<std::path::PathBuf>();
                    let path = if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        std::env::current_dir()
                            .map_err(|_| "cannot determine absolute source path")?
                            .join(path)
                    };
                    let part = if value == Reflection::File {
                        path.file_name().and_then(|name| name.to_str())
                    } else {
                        path.parent().and_then(|parent| parent.to_str())
                    };
                    part.map(|part| Lit::Str(part.to_string()))
                        .ok_or("source path is not a valid UTF-8 file path")
                })(),
            };
            match literal {
                Ok(literal) => Ast::Lit(span, literal),
                Err(message) => {
                    self.error.borrow_mut().get_or_insert_with(|| {
                        ParseError::syntax(
                            crate::error::ParseErrorReason::PositionRule,
                            message,
                            span.clone(),
                        )
                    });
                    Ast::Reflection(span, value)
                }
            }
        }
    }
    let mapper = Materializer {
        source,
        file_path,
        error: std::cell::RefCell::new(None),
    };
    let ast = ast
        .into_iter()
        .map(|node| map_ast_span(node, &mapper))
        .collect();
    match mapper.error.into_inner() {
        Some(error) => Err(error),
        None => Ok(ast),
    }
}

/// Map all source spans, including nested interpolation expressions.
pub fn map_ast_spans(ast: Vec<Ast>, map: &dyn Fn(Span) -> Span) -> Vec<Ast> {
    ast.into_iter()
        .map(|node| map_ast_span(node, &|span| map(span)))
        .collect()
}

pub fn rebase_ast_spans(ast: Vec<Ast>, delta: usize) -> Vec<Ast> {
    ast.into_iter()
        .map(|node| shift_ast_span(node, delta))
        .collect()
}

pub(super) fn ast_ty_span(ty: &AstTy) -> &Span {
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
        AstPattern::Projection { span, .. } => span,
        AstPattern::Var(span, _)
        | AstPattern::Annotated(span, _, _)
        | AstPattern::Pin(span, _)
        | AstPattern::Wildcard(span)
        | AstPattern::AnnotatedWildcard(span, _)
        | AstPattern::HashMap(span, _)
        | AstPattern::ListNil(span)
        | AstPattern::ListCons(span, _, _)
        | AstPattern::IntLit(span, _)
        | AstPattern::StrLit(span, _)
        | AstPattern::BoolLit(span, _)
        | AstPattern::DurationLit(span, _)
        | AstPattern::Constructor(span, _, _)
        | AstPattern::Call(span, _, _)
        | AstPattern::Tuple(span, _)
        | AstPattern::Or(span, _)
        | AstPattern::As(span, _, _, _, _) => span,
    }
}

fn pattern_depth(pat: &AstPattern) -> usize {
    match pat {
        AstPattern::HashMap(_, entries) => {
            1 + entries
                .iter()
                .map(|(_, child)| pattern_depth(child))
                .max()
                .unwrap_or(0)
        }
        AstPattern::ListCons(_, head, tail) => 1 + pattern_depth(head).max(pattern_depth(tail)),
        AstPattern::Constructor(_, _, inners)
        | AstPattern::Tuple(_, inners)
        | AstPattern::Or(_, inners) => 1 + inners.iter().map(pattern_depth).max().unwrap_or(0),
        AstPattern::Call(_, _, args) => {
            1 + args
                .iter()
                .filter_map(|arg| arg.pattern.as_deref())
                .map(pattern_depth)
                .max()
                .unwrap_or(0)
        }
        AstPattern::As(_, inner, _, _, _) | AstPattern::Projection { inner, .. } => {
            1 + pattern_depth(inner)
        }
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

fn shift_ast_span(ast: Ast, delta: usize) -> Ast {
    map_ast_span(ast, &|span| shift_span(span, delta))
}

trait AstMapper {
    fn span(&self, span: Span) -> Span;
    fn reflection(&self, span: Span, value: sindr::reflection::Reflection) -> Ast {
        Ast::Reflection(self.span(span), value)
    }
}

impl<F: Fn(Span) -> Span> AstMapper for F {
    fn span(&self, span: Span) -> Span {
        self(span)
    }
}

fn map_span(span: Span, map: &dyn AstMapper) -> Span {
    map.span(span)
}

fn map_ast_ty(ty: AstTy, map: &dyn AstMapper) -> AstTy {
    match ty {
        AstTy::Named(span, name) => AstTy::Named(map_span(span, map), name),
        AstTy::ImplTrait(span, name) => AstTy::ImplTrait(map_span(span, map), name),
        AstTy::Generic(span, name, args) => AstTy::Generic(
            map_span(span, map),
            name,
            args.into_iter().map(|arg| map_ast_ty(arg, map)).collect(),
        ),
        AstTy::Tuple(span, items) => AstTy::Tuple(
            map_span(span, map),
            items
                .into_iter()
                .map(|item| map_ast_ty(item, map))
                .collect(),
        ),
        AstTy::Func(span, params, ret) => AstTy::Func(
            map_span(span, map),
            params.into_iter().map(|p| map_ast_ty(p, map)).collect(),
            Box::new(map_ast_ty(*ret, map)),
        ),
    }
}

fn map_where_clause(clause: WhereClause, map: &dyn AstMapper) -> WhereClause {
    WhereClause {
        span: map_span(clause.span, map),
        constraints: clause
            .constraints
            .into_iter()
            .map(|constraint| WhereConstraint {
                subject: map_ast_ty(constraint.subject, map),
                bounds: constraint
                    .bounds
                    .into_iter()
                    .map(|bound| match bound {
                        WhereConstraintRhs::Trait(span, name) => {
                            WhereConstraintRhs::Trait(map_span(span, map), name)
                        }
                        WhereConstraintRhs::TypeConstructor(span, slots) => {
                            WhereConstraintRhs::TypeConstructor(
                                map_span(span, map),
                                slots
                                    .into_iter()
                                    .map(|slot| map_ast_ty(slot, map))
                                    .collect(),
                            )
                        }
                        WhereConstraintRhs::TraitSlot(span, name, slot) => {
                            WhereConstraintRhs::TraitSlot(map_span(span, map), name, slot)
                        }
                    })
                    .collect(),
                span: map_span(constraint.span, map),
            })
            .collect(),
    }
}

fn map_parse_error(error: ParseError, map: &dyn AstMapper) -> ParseError {
    error.map_spans(|span| map.span(span.clone()))
}

fn map_pattern(pat: AstPattern, map: &dyn AstMapper) -> AstPattern {
    match pat {
        AstPattern::Projection {
            span,
            index,
            inner,
            annotation,
        } => AstPattern::Projection {
            span: map_span(span, map),
            index,
            inner: Box::new(map_pattern(*inner, map)),
            annotation: annotation.map(|ty| map_ast_ty(ty, map)),
        },
        AstPattern::Var(span, name) => AstPattern::Var(map_span(span, map), name),
        AstPattern::Annotated(span, name, ty) => {
            AstPattern::Annotated(map_span(span, map), name, map_ast_ty(ty, map))
        }
        AstPattern::Pin(span, name) => AstPattern::Pin(map_span(span, map), name),
        AstPattern::Wildcard(span) => AstPattern::Wildcard(map_span(span, map)),
        AstPattern::AnnotatedWildcard(span, ty) => {
            AstPattern::AnnotatedWildcard(map_span(span, map), map_ast_ty(ty, map))
        }
        AstPattern::HashMap(span, entries) => AstPattern::HashMap(
            map_span(span, map),
            entries
                .into_iter()
                .map(|(key, child)| (map_ast_span(key, map), map_pattern(child, map)))
                .collect(),
        ),
        AstPattern::ListNil(span) => AstPattern::ListNil(map_span(span, map)),
        AstPattern::ListCons(span, head, tail) => AstPattern::ListCons(
            map_span(span, map),
            Box::new(map_pattern(*head, map)),
            Box::new(map_pattern(*tail, map)),
        ),
        AstPattern::IntLit(span, n) => AstPattern::IntLit(map_span(span, map), n),
        AstPattern::StrLit(span, s) => AstPattern::StrLit(map_span(span, map), s),
        AstPattern::BoolLit(span, b) => AstPattern::BoolLit(map_span(span, map), b),
        AstPattern::DurationLit(span, n) => AstPattern::DurationLit(map_span(span, map), n),
        AstPattern::Constructor(span, name, inners) => AstPattern::Constructor(
            map_span(span, map),
            name,
            inners
                .into_iter()
                .map(|inner| map_pattern(inner, map))
                .collect(),
        ),
        AstPattern::Call(span, name, args) => AstPattern::Call(
            map_span(span, map),
            name,
            args.into_iter()
                .map(|arg| AstPatternArgument {
                    span: map_span(arg.span, map),
                    expression_error: arg
                        .expression_error
                        .map(|error| map_parse_error(error, map)),
                    pattern_error: arg.pattern_error.map(|error| map_parse_error(error, map)),
                    expression: arg
                        .expression
                        .map(|expr| Box::new(map_ast_span(*expr, map))),
                    pattern: arg
                        .pattern
                        .map(|pattern| Box::new(map_pattern(*pattern, map))),
                    named_pattern: arg
                        .named_pattern
                        .map(|(name, pattern)| (name, Box::new(map_pattern(*pattern, map)))),
                })
                .collect(),
        ),
        AstPattern::Tuple(span, items) => AstPattern::Tuple(
            map_span(span, map),
            items
                .into_iter()
                .map(|item| map_pattern(item, map))
                .collect(),
        ),
        AstPattern::Or(span, items) => AstPattern::Or(
            map_span(span, map),
            items
                .into_iter()
                .map(|item| map_pattern(item, map))
                .collect(),
        ),
        AstPattern::As(span, inner, alias, alias_ty, alias_span) => AstPattern::As(
            map_span(span, map),
            Box::new(map_pattern(*inner, map)),
            alias,
            alias_ty.map(|ty| map_ast_ty(ty, map)),
            map_span(alias_span, map),
        ),
    }
}

fn map_value_parameter(param: ValueParameter, map: &dyn AstMapper) -> ValueParameter {
    ValueParameter {
        name: param.name,
        mode: param.mode,
        ty: map_ast_ty(param.ty, map),
        span: map_span(param.span, map),
    }
}

fn map_return_type_argument(
    argument: ReturnTypeArgument,
    map: &dyn AstMapper,
) -> ReturnTypeArgument {
    ReturnTypeArgument {
        ordinal: argument.ordinal,
        ty: map_ast_ty(argument.ty, map),
        span: map_span(argument.span, map),
    }
}

fn map_extractor_param(param: ExtractorParam, map: &dyn AstMapper) -> ExtractorParam {
    ExtractorParam {
        name: param.name,
        ty: param.ty.map(|ty| map_ast_ty(ty, map)),
        span: map_span(param.span, map),
    }
}

fn map_match_pattern(pat: AstPattern, map: &dyn AstMapper) -> AstPattern {
    map_pattern(pat, map)
}

fn map_decl_attrs(attrs: DeclAttrs, _map: &dyn AstMapper) -> DeclAttrs {
    attrs
}

fn map_process_spec(mut spec: ProcessSpec, map: &dyn AstMapper) -> ProcessSpec {
    spec.state = map_ast_ty(spec.state, map);
    spec.handlers = spec
        .handlers
        .into_iter()
        .map(|handler| ProcessHandlerDependency {
            slot: handler.slot,
            capability: handler.capability,
            default_target: ProcessHandlerTarget {
                name: handler.default_target.name,
                span: map_span(handler.default_target.span, map),
            },
            span: map_span(handler.span, map),
        })
        .collect();
    if let Some(policy) = &mut spec.supervisor_policy {
        let _ = policy;
    }
    spec.handler_specs = spec
        .handler_specs
        .into_iter()
        .map(|handler| ProcessRuntimeHandlerSpec {
            name: handler.name,
            internal_name: handler.internal_name,
            kind: handler.kind,
            span: map_span(handler.span, map),
        })
        .collect();
    spec
}

fn map_builtin_type_head(head: BuiltinTypeHead, map: &dyn AstMapper) -> BuiltinTypeHead {
    BuiltinTypeHead {
        span: map_span(head.span, map),
        name: head.name,
        params: head.params,
    }
}

fn map_record_lit_arg(arg: RecordLitArg, map: &dyn AstMapper) -> RecordLitArg {
    match arg {
        RecordLitArg::Positional(expr) => RecordLitArg::Positional(map_ast_span(expr, map)),
        RecordLitArg::Named(name, expr) => RecordLitArg::Named(name, map_ast_span(expr, map)),
    }
}

fn map_ast_path(path: AstPath, map: &dyn AstMapper) -> AstPath {
    AstPath {
        span: map_span(path.span, map),
        segments: path.segments,
    }
}

fn map_facet_path_segment(segment: FacetPathSegment, map: &dyn AstMapper) -> FacetPathSegment {
    match segment {
        FacetPathSegment::Field { .. } => segment,
        FacetPathSegment::Bracket(expr) => FacetPathSegment::Bracket(FacetBracketExpr {
            expr: Box::new(map_ast_span(*expr.expr, map)),
            display: expr.display,
        }),
    }
}

fn map_bulk_update_path(path: BulkUpdatePath, map: &dyn AstMapper) -> BulkUpdatePath {
    match path {
        BulkUpdatePath::Segments(span, segments) => BulkUpdatePath::Segments(
            map_span(span, map),
            segments
                .into_iter()
                .map(|segment| map_facet_path_segment(segment, map))
                .collect(),
        ),
        BulkUpdatePath::Pin(span, name) => BulkUpdatePath::Pin(map_span(span, map), name),
        BulkUpdatePath::Chain(span, left, right) => BulkUpdatePath::Chain(
            map_span(span, map),
            Box::new(map_bulk_update_path(*left, map)),
            Box::new(map_bulk_update_path(*right, map)),
        ),
        BulkUpdatePath::StripLeft(span, inner, count) => BulkUpdatePath::StripLeft(
            map_span(span, map),
            Box::new(map_bulk_update_path(*inner, map)),
            count,
        ),
        BulkUpdatePath::StripRight(span, inner, count) => BulkUpdatePath::StripRight(
            map_span(span, map),
            Box::new(map_bulk_update_path(*inner, map)),
            count,
        ),
    }
}

fn map_bulk_update_entries(
    entries: Vec<BulkUpdateEntry>,
    map: &dyn AstMapper,
) -> Vec<BulkUpdateEntry> {
    entries
        .into_iter()
        .map(|entry| BulkUpdateEntry {
            span: map_span(entry.span, map),
            path: map_bulk_update_path(entry.path, map),
            kind: match entry.kind {
                BulkUpdateEntryKind::Set(expr) => BulkUpdateEntryKind::Set(map_ast_span(expr, map)),
                BulkUpdateEntryKind::Over(expr) => {
                    BulkUpdateEntryKind::Over(map_ast_span(expr, map))
                }
                BulkUpdateEntryKind::OverResult(expr) => {
                    BulkUpdateEntryKind::OverResult(map_ast_span(expr, map))
                }
                BulkUpdateEntryKind::CaseSet(expr) => {
                    BulkUpdateEntryKind::CaseSet(map_ast_span(expr, map))
                }
                BulkUpdateEntryKind::CaseOver(expr) => {
                    BulkUpdateEntryKind::CaseOver(map_ast_span(expr, map))
                }
                BulkUpdateEntryKind::Nested(entries) => {
                    BulkUpdateEntryKind::Nested(map_bulk_update_entries(entries, map))
                }
            },
        })
        .collect()
}

fn map_do_statements(statements: Vec<AstDoStatement>, map: &dyn AstMapper) -> Vec<AstDoStatement> {
    statements
        .into_iter()
        .map(|statement| match statement {
            AstDoStatement::Extract {
                span,
                operator_span,
                pattern,
                rhs,
            } => AstDoStatement::Extract {
                span: map_span(span, map),
                operator_span: map_span(operator_span, map),
                pattern: map_pattern(pattern, map),
                rhs: map_ast_span(rhs, map),
            },
            AstDoStatement::SafeBind {
                span,
                operator_span,
                pattern,
                rhs,
            } => AstDoStatement::SafeBind {
                span: map_span(span, map),
                operator_span: map_span(operator_span, map),
                pattern: map_pattern(pattern, map),
                rhs: map_ast_span(rhs, map),
            },
            AstDoStatement::Statement(statement) => {
                AstDoStatement::Statement(map_ast_span(statement, map))
            }
        })
        .collect()
}

fn map_ast_span(ast: Ast, map: &dyn AstMapper) -> Ast {
    match ast {
        Ast::NumberedPlaceholder(span, index) => {
            Ast::NumberedPlaceholder(map_span(span, map), index)
        }
        Ast::PatternConsumerCall(span, callee, args) => Ast::PatternConsumerCall(
            map_span(span, map),
            Box::new(map_ast_span(*callee, map)),
            args.into_iter()
                .map(|arg| AstPatternArgument {
                    span: map_span(arg.span, map),
                    expression: arg
                        .expression
                        .map(|expr| Box::new(map_ast_span(*expr, map))),
                    pattern: arg
                        .pattern
                        .map(|pattern| Box::new(map_pattern(*pattern, map))),
                    named_pattern: arg
                        .named_pattern
                        .map(|(name, pattern)| (name, Box::new(map_pattern(*pattern, map)))),
                    expression_error: arg
                        .expression_error
                        .map(|error| map_parse_error(error, map)),
                    pattern_error: arg.pattern_error.map(|error| map_parse_error(error, map)),
                })
                .collect(),
        ),
        Ast::Reflection(span, value) => map.reflection(span, value),
        Ast::BuiltinReflectionDecl(span, value, attrs) => {
            Ast::BuiltinReflectionDecl(map_span(span, map), value, attrs)
        }
        Ast::Lit(span, lit) => Ast::Lit(map_span(span, map), lit),
        Ast::Var(span, name) => Ast::Var(map_span(span, map), name),
        Ast::InternalVar(span, name) => Ast::InternalVar(map_span(span, map), name),
        Ast::Path(span, path) => Ast::Path(map_span(span, map), map_ast_path(path, map)),
        Ast::App(span, func, args) => Ast::App(
            map_span(span, map),
            Box::new(map_ast_span(*func, map)),
            args.into_iter()
                .map(|a| map_record_lit_arg(a, map))
                .collect(),
        ),
        Ast::ReturnTypeArgumentApply(span, target, args) => Ast::ReturnTypeArgumentApply(
            map_span(span, map),
            Box::new(map_ast_span(*target, map)),
            args.into_iter()
                .map(|arg| map_return_type_argument(arg, map))
                .collect(),
        ),
        Ast::EnumConstructorCall(span, owner, type_args, variant, args) => {
            Ast::EnumConstructorCall(
                map_span(span, map),
                owner,
                type_args
                    .into_iter()
                    .map(|ty| map_ast_ty(ty, map))
                    .collect(),
                variant,
                args.into_iter()
                    .map(|arg| map_record_lit_arg(arg, map))
                    .collect(),
            )
        }
        Ast::Block(span, stmts) => Ast::Block(
            map_span(span, map),
            stmts.into_iter().map(|s| map_ast_span(s, map)).collect(),
        ),
        Ast::Bind(span, pat, rhs) => Ast::Bind(
            map_span(span, map),
            map_pattern(pat, map),
            Box::new(map_ast_span(*rhs, map)),
        ),
        Ast::SafeBind(span, pat, rhs, operator_span) => Ast::SafeBind(
            map_span(span, map),
            map_pattern(pat, map),
            Box::new(map_ast_span(*rhs, map)),
            map_span(operator_span, map),
        ),
        Ast::Do(span, return_type_arguments, statements, keyword_span) => Ast::Do(
            map_span(span, map),
            return_type_arguments
                .into_iter()
                .map(|argument| map_return_type_argument(argument, map))
                .collect(),
            map_do_statements(statements, map),
            map_span(keyword_span, map),
        ),
        Ast::BinOp(span, op, left, right) => Ast::BinOp(
            map_span(span, map),
            op,
            Box::new(map_ast_span(*left, map)),
            Box::new(map_ast_span(*right, map)),
        ),
        Ast::Pipe(span, left, right) => Ast::Pipe(
            map_span(span, map),
            Box::new(map_ast_span(*left, map)),
            Box::new(map_ast_span(*right, map)),
        ),
        Ast::ContextMap(span, left, right) => Ast::ContextMap(
            map_span(span, map),
            Box::new(map_ast_span(*left, map)),
            Box::new(map_ast_span(*right, map)),
        ),
        Ast::ContextApply(span, left, right) => Ast::ContextApply(
            map_span(span, map),
            Box::new(map_ast_span(*left, map)),
            Box::new(map_ast_span(*right, map)),
        ),
        Ast::ContextBind(span, left, right) => Ast::ContextBind(
            map_span(span, map),
            Box::new(map_ast_span(*left, map)),
            Box::new(map_ast_span(*right, map)),
        ),
        Ast::Compose(span, left, right) => Ast::Compose(
            map_span(span, map),
            Box::new(map_ast_span(*left, map)),
            Box::new(map_ast_span(*right, map)),
        ),
        Ast::LiftedCompose(span, left, right) => Ast::LiftedCompose(
            map_span(span, map),
            Box::new(map_ast_span(*left, map)),
            Box::new(map_ast_span(*right, map)),
        ),
        Ast::KleisliCompose(span, left, right) => Ast::KleisliCompose(
            map_span(span, map),
            Box::new(map_ast_span(*left, map)),
            Box::new(map_ast_span(*right, map)),
        ),
        Ast::ListNil(span) => Ast::ListNil(map_span(span, map)),
        Ast::ListCons(span, head, tail) => Ast::ListCons(
            map_span(span, map),
            Box::new(map_ast_span(*head, map)),
            Box::new(map_ast_span(*tail, map)),
        ),
        Ast::ListLiteral(span, elems) => Ast::ListLiteral(
            map_span(span, map),
            elems.into_iter().map(|e| map_ast_span(e, map)).collect(),
        ),
        Ast::HashMapLiteral(span, entries) => Ast::HashMapLiteral(
            map_span(span, map),
            entries
                .into_iter()
                .map(|entry| HashMapLiteralEntry {
                    key: map_ast_span(entry.key, map),
                    value: map_ast_span(entry.value, map),
                })
                .collect(),
        ),
        Ast::RangeLiteral(span, start, stop) => Ast::RangeLiteral(
            map_span(span, map),
            Box::new(map_ast_span(*start, map)),
            Box::new(map_ast_span(*stop, map)),
        ),
        Ast::TupleLiteral(span, elems) => Ast::TupleLiteral(
            map_span(span, map),
            elems.into_iter().map(|e| map_ast_span(e, map)).collect(),
        ),
        Ast::Cond(span, clauses) => Ast::Cond(
            map_span(span, map),
            clauses
                .into_iter()
                .map(|(condition, body)| (map_ast_span(condition, map), map_ast_span(body, map)))
                .collect(),
        ),
        Ast::Grouped(span, inner) => {
            Ast::Grouped(map_span(span, map), Box::new(map_ast_span(*inner, map)))
        }
        Ast::InterpolatedStr(span, parts) => Ast::InterpolatedStr(
            map_span(span, map),
            parts
                .into_iter()
                .map(|p| match p {
                    InterpolatedPart::Text(s) => InterpolatedPart::Text(s),
                    InterpolatedPart::Expr(expr) => {
                        InterpolatedPart::Expr(Box::new(map_ast_span(*expr, map)))
                    }
                })
                .collect(),
        ),
        Ast::Dbg(span, args) => Ast::Dbg(
            map_span(span, map),
            args.into_iter()
                .map(|arg| DbgArg {
                    span: map_span(arg.span, map),
                    expr: map_ast_span(arg.expr, map),
                })
                .collect(),
        ),
        Ast::Match(span, expr, arms) => Ast::Match(
            map_span(span, map),
            Box::new(map_ast_span(*expr, map)),
            arms.into_iter()
                .map(|arm| AstMatchArm {
                    pattern: map_match_pattern(arm.pattern, map),
                    guard: arm.guard.map(|guard| map_ast_span(guard, map)),
                    body: map_ast_span(arm.body, map),
                })
                .collect(),
        ),
        Ast::BulkUpdate(span, source, entries) => Ast::BulkUpdate(
            map_span(span, map),
            Box::new(map_ast_span(*source, map)),
            map_bulk_update_entries(entries, map),
        ),
        Ast::FieldAccess(span, expr, field) => Ast::FieldAccess(
            map_span(span, map),
            Box::new(map_ast_span(*expr, map)),
            field,
        ),
        Ast::FacetSegmentAccess(span, expr, segment) => Ast::FacetSegmentAccess(
            map_span(span, map),
            Box::new(map_ast_span(*expr, map)),
            map_facet_path_segment(segment, map),
        ),
        Ast::FacetCapture(span, expr) => {
            Ast::FacetCapture(map_span(span, map), Box::new(map_ast_span(*expr, map)))
        }
        Ast::StructDef(span, name, type_params, fields, attrs) => Ast::StructDef(
            map_span(span, map),
            name,
            type_params
                .into_iter()
                .map(|param| TypeParam {
                    name: param.name,
                    bound: param.bound,
                    span: map_span(param.span, map),
                })
                .collect(),
            fields
                .into_iter()
                .map(|f| StructField {
                    name: f.name,
                    ty: map_ast_ty(f.ty, map),
                    span: map_span(f.span, map),
                    visibility: f.visibility,
                    readonly: f.readonly,
                })
                .collect(),
            map_decl_attrs(attrs, map),
        ),
        Ast::RecordDef(span, name, fields, attrs) => Ast::RecordDef(
            map_span(span, map),
            name,
            fields
                .into_iter()
                .map(|f| RecordField {
                    name: f.name,
                    ty: map_ast_ty(f.ty, map),
                    span: map_span(f.span, map),
                    visibility: f.visibility,
                    readonly: f.readonly,
                })
                .collect(),
            map_decl_attrs(attrs, map),
        ),
        Ast::StructLit(span, name, fields) => Ast::StructLit(
            map_span(span, map),
            name,
            fields
                .into_iter()
                .map(|field| match field {
                    StructLitField::Explicit(name, expr) => {
                        StructLitField::Explicit(name, map_ast_span(expr, map))
                    }
                    StructLitField::Shorthand(name) => StructLitField::Shorthand(name),
                })
                .collect(),
        ),
        Ast::InternalStructLit(span, name, fields) => Ast::InternalStructLit(
            map_span(span, map),
            name,
            fields
                .into_iter()
                .map(|field| match field {
                    StructLitField::Explicit(name, expr) => {
                        StructLitField::Explicit(name, map_ast_span(expr, map))
                    }
                    StructLitField::Shorthand(name) => StructLitField::Shorthand(name),
                })
                .collect(),
        ),
        Ast::ConstructorCall(span, name, args) => Ast::ConstructorCall(
            map_span(span, map),
            name,
            args.into_iter()
                .map(|a| map_record_lit_arg(a, map))
                .collect(),
        ),
        Ast::DeferrorDef(span, name, fields, show_expr, attrs) => Ast::DeferrorDef(
            map_span(span, map),
            name,
            fields
                .into_iter()
                .map(|f| RecordField {
                    name: f.name,
                    ty: map_ast_ty(f.ty, map),
                    span: map_span(f.span, map),
                    visibility: f.visibility,
                    readonly: f.readonly,
                })
                .collect(),
            Box::new(map_ast_span(*show_expr, map)),
            map_decl_attrs(attrs, map),
        ),
        Ast::EnumDef(span, name, type_params, variants, attrs) => Ast::EnumDef(
            map_span(span, map),
            name,
            type_params
                .into_iter()
                .map(|param| TypeParam {
                    name: param.name,
                    bound: param.bound,
                    span: map_span(param.span, map),
                })
                .collect(),
            variants
                .into_iter()
                .map(|variant| EnumVariant {
                    name: variant.name,
                    payload: variant
                        .payload
                        .into_iter()
                        .map(|ty| map_ast_ty(ty, map))
                        .collect(),
                    discriminant: variant.discriminant,
                    span: map_span(variant.span, map),
                })
                .collect(),
            map_decl_attrs(attrs, map),
        ),
        Ast::Def(span, name, return_type_arguments, params, ret_ty, where_clause, body, attrs) => {
            Ast::Def(
                map_span(span, map),
                name,
                return_type_arguments
                    .into_iter()
                    .map(|argument| map_return_type_argument(argument, map))
                    .collect(),
                params
                    .into_iter()
                    .map(|p| map_value_parameter(p, map))
                    .collect(),
                ret_ty.map(|ty| map_ast_ty(ty, map)),
                where_clause.map(|clause| map_where_clause(clause, map)),
                Box::new(map_ast_span(*body, map)),
                map_decl_attrs(attrs, map),
            )
        }
        Ast::ConstDef(span, name, ty, value, attrs) => Ast::ConstDef(
            map_span(span, map),
            name,
            ty.map(|ty| map_ast_ty(ty, map)),
            Box::new(map_ast_span(*value, map)),
            map_decl_attrs(attrs, map),
        ),
        Ast::SupervisorInit(span, spec) => Ast::SupervisorInit(
            map_span(span, map),
            SupervisorInitSpec {
                entries: spec
                    .entries
                    .into_iter()
                    .map(|entry| SupervisorInitEntry {
                        process_name: entry.process_name,
                        timeout_ms: entry.timeout_ms,
                        handlers: entry
                            .handlers
                            .into_iter()
                            .map(|handler| SupervisorInitHandlerOverride {
                                slot: handler.slot,
                                target: SupervisorInitHandlerTarget {
                                    name: handler.target.name,
                                    named_args: handler
                                        .target
                                        .named_args
                                        .into_iter()
                                        .map(|arg| SupervisorInitHandlerArg {
                                            name: arg.name,
                                            value: arg.value,
                                            span: map_span(arg.span, map),
                                        })
                                        .collect(),
                                    span: map_span(handler.target.span, map),
                                },
                                span: map_span(handler.span, map),
                            })
                            .collect(),
                        overrides: entry.overrides,
                        span: map_span(entry.span, map),
                    })
                    .collect(),
                singletons: spec
                    .singletons
                    .into_iter()
                    .map(|singleton| SupervisorInitSingleton {
                        process_name: singleton.process_name,
                        timeout_ms: singleton.timeout_ms,
                        handlers: singleton
                            .handlers
                            .into_iter()
                            .map(|handler| SupervisorInitHandlerOverride {
                                slot: handler.slot,
                                target: SupervisorInitHandlerTarget {
                                    name: handler.target.name,
                                    named_args: handler
                                        .target
                                        .named_args
                                        .into_iter()
                                        .map(|arg| SupervisorInitHandlerArg {
                                            name: arg.name,
                                            value: arg.value,
                                            span: map_span(arg.span, map),
                                        })
                                        .collect(),
                                    span: map_span(handler.target.span, map),
                                },
                                span: map_span(handler.span, map),
                            })
                            .collect(),
                        span: map_span(singleton.span, map),
                    })
                    .collect(),
                supervisors: spec
                    .supervisors
                    .into_iter()
                    .map(|supervisor| SupervisorInitOverride {
                        process_name: supervisor.process_name,
                        overrides: supervisor.overrides,
                        span: map_span(supervisor.span, map),
                    })
                    .collect(),
            },
        ),
        Ast::ExtractorDef(span, name, type_params, param, ret_ty, body, attrs) => {
            Ast::ExtractorDef(
                map_span(span, map),
                name,
                type_params
                    .into_iter()
                    .map(|param| TypeParam {
                        name: param.name,
                        bound: param.bound,
                        span: map_span(param.span, map),
                    })
                    .collect(),
                param
                    .into_iter()
                    .map(|param| map_extractor_param(param, map))
                    .collect(),
                map_ast_ty(ret_ty, map),
                Box::new(map_ast_span(*body, map)),
                map_decl_attrs(attrs, map),
            )
        }
        Ast::BuiltinDecl(
            span,
            name,
            return_type_arguments,
            params,
            ret_ty,
            where_clause,
            attrs,
        ) => Ast::BuiltinDecl(
            map_span(span, map),
            name,
            return_type_arguments
                .into_iter()
                .map(|argument| map_return_type_argument(argument, map))
                .collect(),
            params
                .into_iter()
                .map(|p| map_value_parameter(p, map))
                .collect(),
            ret_ty.map(|ty| map_ast_ty(ty, map)),
            where_clause.map(|clause| map_where_clause(clause, map)),
            map_decl_attrs(attrs, map),
        ),
        Ast::IntrinsicDecl(span, name, signature, attrs) => Ast::IntrinsicDecl(
            map_span(span, map),
            name,
            IntrinsicSignature {
                raw: signature.raw,
                return_type_arguments: signature
                    .return_type_arguments
                    .into_iter()
                    .map(|argument| map_return_type_argument(argument, map))
                    .collect(),
                value_parameters: signature
                    .value_parameters
                    .into_iter()
                    .map(|parameter| map_value_parameter(parameter, map))
                    .collect(),
                return_type: signature.return_type.map(|ty| map_ast_ty(ty, map)),
                where_clause: signature
                    .where_clause
                    .map(|clause| map_where_clause(clause, map)),
            },
            map_decl_attrs(attrs, map),
        ),
        Ast::BuiltinExtractorDecl(span, name, param, ret_ty, attrs) => Ast::BuiltinExtractorDecl(
            map_span(span, map),
            name,
            param
                .into_iter()
                .map(|param| map_extractor_param(param, map))
                .collect(),
            map_ast_ty(ret_ty, map),
            map_decl_attrs(attrs, map),
        ),
        Ast::BuiltinTypeDecl(span, head, attrs) => Ast::BuiltinTypeDecl(
            map_span(span, map),
            map_builtin_type_head(head, map),
            map_decl_attrs(attrs, map),
        ),
        Ast::TypeAlias(span, name, type_params, rhs) => {
            Ast::TypeAlias(map_span(span, map), name, type_params, map_ast_ty(rhs, map))
        }
        Ast::Namespace(span, name, body) => Ast::Namespace(
            map_span(span, map),
            name,
            body.into_iter()
                .map(|stmt| map_ast_span(stmt, map))
                .collect(),
        ),
        Ast::Defmod(span, name, body, attrs) => Ast::Defmod(
            map_span(span, map),
            name,
            body.into_iter().map(|n| map_ast_span(n, map)).collect(),
            map_decl_attrs(attrs, map),
        ),
        Ast::Defagent(span, name, body, process_spec, attrs) => Ast::Defagent(
            map_span(span, map),
            name,
            body.into_iter().map(|n| map_ast_span(n, map)).collect(),
            map_process_spec(process_spec, map),
            map_decl_attrs(attrs, map),
        ),
        Ast::Defgenserver(span, name, body, process_spec, attrs) => Ast::Defgenserver(
            map_span(span, map),
            name,
            body.into_iter().map(|n| map_ast_span(n, map)).collect(),
            map_process_spec(process_spec, map),
            map_decl_attrs(attrs, map),
        ),
        Ast::Defsupervisor(span, name, body, process_spec, attrs) => Ast::Defsupervisor(
            map_span(span, map),
            name,
            body.into_iter().map(|n| map_ast_span(n, map)).collect(),
            map_process_spec(process_spec, map),
            map_decl_attrs(attrs, map),
        ),
        Ast::DefdynamicSupervisor(span, name, body, process_spec, attrs) => {
            Ast::DefdynamicSupervisor(
                map_span(span, map),
                name,
                body.into_iter().map(|n| map_ast_span(n, map)).collect(),
                map_process_spec(process_spec, map),
                map_decl_attrs(attrs, map),
            )
        }
        Ast::ImplDef(span, target, target_span, methods, attrs) => Ast::ImplDef(
            map_span(span, map),
            target,
            map_span(target_span, map),
            methods
                .into_iter()
                .map(|method| map_ast_span(method, map))
                .collect(),
            map_decl_attrs(attrs, map),
        ),
        Ast::TraitDef(span, name, type_params, where_clause, methods, attrs) => Ast::TraitDef(
            map_span(span, map),
            name,
            type_params
                .into_iter()
                .map(|param| TypeParam {
                    name: param.name,
                    bound: param.bound,
                    span: map_span(param.span, map),
                })
                .collect(),
            where_clause.map(|clause| map_where_clause(clause, map)),
            methods
                .into_iter()
                .map(|method| TraitMethodSig {
                    name: method.name,
                    return_type_arguments: method
                        .return_type_arguments
                        .into_iter()
                        .map(|argument| map_return_type_argument(argument, map))
                        .collect(),
                    type_params: method
                        .type_params
                        .into_iter()
                        .map(|param| TypeParam {
                            name: param.name,
                            bound: param.bound,
                            span: map_span(param.span, map),
                        })
                        .collect(),
                    value_parameters: method
                        .value_parameters
                        .into_iter()
                        .map(|param| map_value_parameter(param, map))
                        .collect(),
                    ret_ty: map_ast_ty(method.ret_ty, map),
                    where_clause: method
                        .where_clause
                        .map(|clause| map_where_clause(clause, map)),
                    body: method.body.map(|body| Box::new(map_ast_span(*body, map))),
                    attrs: map_decl_attrs(method.attrs, map),
                    span: map_span(method.span, map),
                })
                .collect(),
            map_decl_attrs(attrs, map),
        ),
        Ast::TraitImplDef(span, trait_name, trait_args, target, where_clause, methods, attrs) => {
            Ast::TraitImplDef(
                map_span(span, map),
                trait_name,
                trait_args
                    .into_iter()
                    .map(|arg| map_ast_ty(arg, map))
                    .collect(),
                map_ast_ty(target, map),
                where_clause.map(|clause| map_where_clause(clause, map)),
                methods
                    .into_iter()
                    .map(|method| map_ast_span(method, map))
                    .collect(),
                map_decl_attrs(attrs, map),
            )
        }
        Ast::Import(span, path, spec) => {
            Ast::Import(map_span(span, map), map_ast_path(path, map), spec)
        }
        Ast::Include(span, path) => Ast::Include(map_span(span, map), path),
        Ast::Closure(span, params, body) => Ast::Closure(
            map_span(span, map),
            params
                .into_iter()
                .map(|p| ClosureParam {
                    name: p.name,
                    ty: p.ty.map(|ty| map_ast_ty(ty, map)),
                    span: map_span(p.span, map),
                })
                .collect(),
            Box::new(map_ast_span(*body, map)),
        ),
        Ast::ExtractorClosure(span, params, body) => Ast::ExtractorClosure(
            map_span(span, map),
            params
                .into_iter()
                .map(|p| ClosureParam {
                    name: p.name,
                    ty: p.ty.map(|ty| map_ast_ty(ty, map)),
                    span: map_span(p.span, map),
                })
                .collect(),
            Box::new(map_ast_span(*body, map)),
        ),
        Ast::Capture(span, target, args) => Ast::Capture(
            map_span(span, map),
            Box::new(map_ast_span(*target, map)),
            args.into_iter().map(|a| map_ast_span(a, map)).collect(),
        ),
        Ast::NamedInfixRef(span, path) => Ast::NamedInfixRef(
            map_span(span, map),
            AstPath {
                span: map_span(path.span, map),
                segments: path.segments,
            },
        ),
        Ast::FuncLiteralRef(span, func) => Ast::FuncLiteralRef(
            map_span(span, map),
            FuncLiteralRef {
                span: map_span(func.span, map),
                body: func.body,
            },
        ),
        Ast::CapturePlaceholder(span, index) => Ast::CapturePlaceholder(map_span(span, map), index),
        Ast::StatementQuestion(span, inner) => {
            Ast::StatementQuestion(map_span(span, map), Box::new(map_ast_span(*inner, map)))
        }
        Ast::Semi(span, inner) => {
            Ast::Semi(map_span(span, map), Box::new(map_ast_span(*inner, map)))
        }
    }
}

// ── Ast span accessor ──

impl Ast {
    pub fn span(&self) -> &Span {
        match self {
            Ast::Reflection(s, _)
            | Ast::BuiltinReflectionDecl(s, _, _)
            | Ast::PatternConsumerCall(s, _, _)
            | Ast::NumberedPlaceholder(s, _)
            | Ast::Lit(s, _)
            | Ast::Var(s, _)
            | Ast::InternalVar(s, _)
            | Ast::Path(s, _)
            | Ast::FuncLiteralRef(s, _)
            | Ast::NamedInfixRef(s, _)
            | Ast::App(s, _, _)
            | Ast::ReturnTypeArgumentApply(s, _, _)
            | Ast::Block(s, _)
            | Ast::Bind(s, _, _)
            | Ast::SafeBind(s, _, _, _)
            | Ast::StatementQuestion(s, _)
            | Ast::Do(s, _, _, _)
            | Ast::BinOp(s, _, _, _)
            | Ast::Pipe(s, _, _)
            | Ast::ContextMap(s, _, _)
            | Ast::ContextApply(s, _, _)
            | Ast::ContextBind(s, _, _)
            | Ast::Compose(s, _, _)
            | Ast::LiftedCompose(s, _, _)
            | Ast::KleisliCompose(s, _, _)
            | Ast::ListNil(s)
            | Ast::ListCons(s, _, _)
            | Ast::ListLiteral(s, _)
            | Ast::HashMapLiteral(s, _)
            | Ast::RangeLiteral(s, _, _)
            | Ast::TupleLiteral(s, _)
            | Ast::Cond(s, _)
            | Ast::Grouped(s, _)
            | Ast::InterpolatedStr(s, _)
            | Ast::Dbg(s, _)
            | Ast::Match(s, _, _)
            | Ast::BulkUpdate(s, _, _)
            | Ast::FieldAccess(s, _, _)
            | Ast::FacetSegmentAccess(s, _, _)
            | Ast::FacetCapture(s, _)
            | Ast::StructDef(s, ..)
            | Ast::RecordDef(s, _, _, _)
            | Ast::StructLit(s, _, _)
            | Ast::InternalStructLit(s, _, _)
            | Ast::ConstructorCall(s, _, _)
            | Ast::EnumConstructorCall(s, _, _, _, _)
            | Ast::DeferrorDef(s, _, _, _, _)
            | Ast::EnumDef(s, _, _, _, _)
            | Ast::Def(s, _, _, _, _, _, _, _)
            | Ast::ConstDef(s, _, _, _, _)
            | Ast::SupervisorInit(s, _)
            | Ast::ExtractorDef(s, _, _, _, _, _, _)
            | Ast::BuiltinDecl(s, ..)
            | Ast::IntrinsicDecl(s, _, _, _)
            | Ast::BuiltinExtractorDecl(s, _, _, _, _)
            | Ast::BuiltinTypeDecl(s, _, _)
            | Ast::TypeAlias(s, _, _, _)
            | Ast::Namespace(s, _, _)
            | Ast::Defmod(s, _, _, _)
            | Ast::Defagent(s, _, _, _, _)
            | Ast::Defgenserver(s, _, _, _, _)
            | Ast::Defsupervisor(s, _, _, _, _)
            | Ast::DefdynamicSupervisor(s, _, _, _, _)
            | Ast::ImplDef(s, _, _, _, _)
            | Ast::TraitDef(s, _, _, _, _, _)
            | Ast::TraitImplDef(s, _, _, _, _, _, _)
            | Ast::Import(s, _, _)
            | Ast::Include(s, _)
            | Ast::Closure(s, _, _)
            | Ast::ExtractorClosure(s, _, _)
            | Ast::Capture(s, _, _)
            | Ast::CapturePlaceholder(s, _)
            | Ast::Semi(s, _) => s,
        }
    }
}

#[cfg(test)]
mod tests;
