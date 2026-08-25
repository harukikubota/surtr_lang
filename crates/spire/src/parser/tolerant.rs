use crate::ast::*;
use crate::error::ParseError;
use crate::token::{Spanned, Token};
use sindr::primitives::SurtrInt;

use super::context::{DeclLevel, ParseUnitKind, TopLevelDeclKind};
use super::{
    canonicalize_root_owner_heads, lower_namespaces, reject_excessive_delimiter_nesting,
    reject_marker_owner_paths, validate, ParseDiagnostic, ParseRules, Parser, ParserContext,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxTokenKind {
    Int,
    Float,
    String,
    DocString,
    Bool,
    Unit,
    Identifier,
    FuncLiteral,
    Annotator,
    Keyword,
    Operator,
    Delimiter,
    Punctuation,
    PathSep,
    Compose,
    Newline,
    Comment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxToken {
    pub kind: SyntaxTokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxOutlineKind {
    Function,
    Extractor,
    Const,
    Struct,
    Record,
    Error,
    Enum,
    Module,
    Impl,
    Trait,
    TraitImpl,
    Import,
    Include,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxOutlineItem {
    pub kind: SyntaxOutlineKind,
    pub name: Option<String>,
    pub span: Span,
    pub selection_span: Span,
    pub children: Vec<SyntaxOutlineItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorSyntaxContext {
    Expr,
    Type,
    DeclHead,
    ImportPath,
    QualifiedPath,
    CallArgName,
    CallArgValue,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct TolerantParseResult {
    pub ast: Vec<Ast>,
    pub diagnostics: Vec<ParseDiagnostic>,
    pub tokens: Vec<SyntaxToken>,
    pub outline: Vec<SyntaxOutlineItem>,
    pub cursor_context: CursorSyntaxContext,
}

struct TolerantScan {
    parser_tokens: Vec<Spanned<Token>>,
    syntax_tokens: Vec<SyntaxToken>,
    diagnostics: Vec<ParseDiagnostic>,
}

pub fn parse_tolerant_with_context(
    source: &str,
    context: ParserContext,
    cursor_char_offset: Option<usize>,
) -> TolerantParseResult {
    let scan = scan_tolerant(source);
    let mut diagnostics = scan.diagnostics;

    if let Err(error) = reject_excessive_delimiter_nesting(&scan.parser_tokens) {
        diagnostics.push(ParseDiagnostic::from(error));
    }
    if let Err(error) = reject_marker_owner_paths(&scan.parser_tokens) {
        diagnostics.push(ParseDiagnostic::from(error));
    }

    let mut ast = parse_tolerant_program(
        source,
        &scan.parser_tokens,
        context.clone(),
        &mut diagnostics,
    );
    if let Err(error) = validate::validate_program_by_context(&context, &ast) {
        diagnostics.push(ParseDiagnostic::from(error));
    }
    match lower_namespaces(ast.clone()).and_then(canonicalize_root_owner_heads) {
        Ok(lowered) => ast = lowered,
        Err(error) => diagnostics.push(ParseDiagnostic::from(error)),
    }

    let outline = merge_outline_items(outline_from_ast(&ast), outline_from_source(source));
    let cursor_context = cursor_char_offset
        .map(|cursor| infer_cursor_context(source, cursor))
        .unwrap_or(CursorSyntaxContext::Unknown);

    TolerantParseResult {
        ast,
        diagnostics,
        tokens: scan.syntax_tokens,
        outline,
        cursor_context,
    }
}

fn scan_tolerant(source: &str) -> TolerantScan {
    let chars = source.chars().collect::<Vec<_>>();
    let len = chars.len();
    let mut parser_tokens = Vec::new();
    let mut syntax_tokens = Vec::new();
    let mut diagnostics = Vec::new();
    let mut i = 0usize;

    while i < len {
        let c = chars[i];
        if matches!(c, ' ' | '\t' | '\r') {
            i += 1;
            continue;
        }

        if c == '#' {
            let start = i;
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            push_syntax(&mut syntax_tokens, SyntaxTokenKind::Comment, start, i);
            continue;
        }

        if c == '\n' {
            push_both(
                &mut parser_tokens,
                &mut syntax_tokens,
                Token::Newline,
                SyntaxTokenKind::Newline,
                i,
                i + 1,
            );
            i += 1;
            continue;
        }

        if c == '@' {
            let start = i;
            if i + 1 < len && (chars[i + 1].is_ascii_alphanumeric() || chars[i + 1] == '_') {
                i += 1;
                let name_start = i;
                while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let name = chars[name_start..i].iter().collect::<String>();
                push_both(
                    &mut parser_tokens,
                    &mut syntax_tokens,
                    Token::Annotator(name),
                    SyntaxTokenKind::Annotator,
                    start,
                    i,
                );
                continue;
            }
            push_both(
                &mut parser_tokens,
                &mut syntax_tokens,
                Token::At,
                SyntaxTokenKind::Punctuation,
                start,
                start + 1,
            );
            i += 1;
            continue;
        }

        if c == '"' && i + 2 < len && chars[i + 1] == '"' && chars[i + 2] == '"' {
            let start = i;
            i += 3;
            while i + 2 < len && !(chars[i] == '"' && chars[i + 1] == '"' && chars[i + 2] == '"') {
                i += 1;
            }
            if i + 2 >= len {
                diagnostics.push(parse_diag(ParseError::incomplete(
                    "\"\"\"",
                    Span { start, end: len },
                )));
                break;
            }
            let content = chars[start + 3..i].iter().collect::<String>();
            i += 3;
            push_both(
                &mut parser_tokens,
                &mut syntax_tokens,
                Token::DocString(content),
                SyntaxTokenKind::DocString,
                start,
                i,
            );
            continue;
        }

        if c == '"' || c == '\'' {
            let quote = c;
            let start = i;
            i += 1;
            let body_start = i;
            let mut escaped = false;
            while i < len {
                if chars[i] == '\n' && !escaped {
                    break;
                }
                if escaped {
                    escaped = false;
                } else if chars[i] == '\\' {
                    escaped = true;
                } else if chars[i] == quote {
                    break;
                }
                i += 1;
            }
            if i >= len || chars[i] == '\n' {
                diagnostics.push(parse_diag(ParseError::incomplete(
                    quote.to_string(),
                    Span { start, end: i },
                )));
                continue;
            }
            let text = chars[body_start..i].iter().collect::<String>();
            i += 1;
            push_both(
                &mut parser_tokens,
                &mut syntax_tokens,
                Token::Str(text),
                SyntaxTokenKind::String,
                start,
                i,
            );
            continue;
        }

        if c == '`' {
            let start = i;
            i += 1;
            let body_start = i;
            while i < len && chars[i] != '`' && chars[i] != '\n' {
                i += 1;
            }
            if i >= len || chars[i] == '\n' {
                diagnostics.push(parse_diag(ParseError::incomplete(
                    "`",
                    Span { start, end: i },
                )));
                continue;
            }
            let body = chars[body_start..i].iter().collect::<String>();
            i += 1;
            push_both(
                &mut parser_tokens,
                &mut syntax_tokens,
                Token::FuncLiteral(body),
                SyntaxTokenKind::FuncLiteral,
                start,
                i,
            );
            continue;
        }

        if c.is_ascii_digit() {
            let start = i;
            while i < len && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < len && chars[i] == '.' && i + 1 < len && chars[i + 1].is_ascii_digit() {
                i += 1;
                while i < len && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let text = chars[start..i].iter().collect::<String>();
                match text.parse::<f64>() {
                    Ok(value) if value.is_finite() => push_both(
                        &mut parser_tokens,
                        &mut syntax_tokens,
                        Token::Float(value),
                        SyntaxTokenKind::Float,
                        start,
                        i,
                    ),
                    _ => diagnostics.push(parse_diag(ParseError::syntax(
                        format!("Invalid float: {text}"),
                        Span { start, end: i },
                    ))),
                }
            } else {
                let text = chars[start..i].iter().collect::<String>();
                match text.parse::<SurtrInt>() {
                    Ok(value) => push_both(
                        &mut parser_tokens,
                        &mut syntax_tokens,
                        Token::Int(value),
                        SyntaxTokenKind::Int,
                        start,
                        i,
                    ),
                    Err(_) => diagnostics.push(parse_diag(ParseError::syntax(
                        format!("Invalid integer: {text}"),
                        Span { start, end: i },
                    ))),
                }
            }
            continue;
        }

        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let text = chars[start..i].iter().collect::<String>();
            let (token, kind) = keyword_token(&text);
            push_both(
                &mut parser_tokens,
                &mut syntax_tokens,
                token,
                kind,
                start,
                i,
            );
            continue;
        }

        if i + 2 < len {
            let three = chars[i..i + 3].iter().collect::<String>();
            let token = match three.as_str() {
                "|*>" => Some(Token::PipeMap),
                "|>=" => Some(Token::PipeBind),
                "|*|" => Some(Token::PipeApplyContext),
                ">=>" => Some(Token::KleisliCompose),
                _ => None,
            };
            if let Some(token) = token {
                push_both(
                    &mut parser_tokens,
                    &mut syntax_tokens,
                    token,
                    SyntaxTokenKind::Operator,
                    i,
                    i + 3,
                );
                i += 3;
                continue;
            }
        }

        if i + 1 < len {
            let two = chars[i..i + 2].iter().collect::<String>();
            if two == "::" {
                parser_tokens.push(Spanned {
                    token: Token::Colon,
                    span: Span {
                        start: i,
                        end: i + 1,
                    },
                });
                parser_tokens.push(Spanned {
                    token: Token::Colon,
                    span: Span {
                        start: i + 1,
                        end: i + 2,
                    },
                });
                push_syntax(&mut syntax_tokens, SyntaxTokenKind::PathSep, i, i + 2);
                i += 2;
                continue;
            }

            if two == "()" {
                push_both(
                    &mut parser_tokens,
                    &mut syntax_tokens,
                    Token::Unit,
                    SyntaxTokenKind::Unit,
                    i,
                    i + 2,
                );
                i += 2;
                continue;
            }

            if two == "||"
                && matches!(
                    parser_tokens.last().map(|sp| &sp.token),
                    Some(Token::LBrace)
                )
            {
                push_both(
                    &mut parser_tokens,
                    &mut syntax_tokens,
                    Token::Pipe,
                    SyntaxTokenKind::Punctuation,
                    i,
                    i + 1,
                );
                push_both(
                    &mut parser_tokens,
                    &mut syntax_tokens,
                    Token::Pipe,
                    SyntaxTokenKind::Punctuation,
                    i + 1,
                    i + 2,
                );
                i += 2;
                continue;
            }

            let token = match two.as_str() {
                "++" => Some((Token::Concat, SyntaxTokenKind::Operator)),
                "=?" => Some((Token::SafeBind, SyntaxTokenKind::Operator)),
                "==" => Some((Token::EqEq, SyntaxTokenKind::Operator)),
                "!=" => Some((Token::BangEq, SyntaxTokenKind::Operator)),
                "<=" => Some((Token::LtEq, SyntaxTokenKind::Operator)),
                ">=" => Some((Token::GtEq, SyntaxTokenKind::Operator)),
                "<-" => Some((Token::LeftArrow, SyntaxTokenKind::Operator)),
                "&&" => Some((Token::AndAnd, SyntaxTokenKind::Operator)),
                "||" => Some((Token::OrOr, SyntaxTokenKind::Operator)),
                ".." => Some((Token::DotDot, SyntaxTokenKind::Punctuation)),
                "=>" => Some((Token::FatArrow, SyntaxTokenKind::Operator)),
                "->" => Some((Token::Arrow, SyntaxTokenKind::Operator)),
                "|>" => Some((Token::PipeApply, SyntaxTokenKind::Operator)),
                ">>" => Some((Token::Compose, SyntaxTokenKind::Compose)),
                ">*" => Some((Token::LiftCompose, SyntaxTokenKind::Operator)),
                _ => None,
            };
            if let Some((token, kind)) = token {
                push_both(
                    &mut parser_tokens,
                    &mut syntax_tokens,
                    token,
                    kind,
                    i,
                    i + 2,
                );
                i += 2;
                continue;
            }
        }

        let token = match c {
            '+' => Some((Token::Plus, SyntaxTokenKind::Operator)),
            '-' => Some((Token::Minus, SyntaxTokenKind::Operator)),
            '*' => Some((Token::Star, SyntaxTokenKind::Operator)),
            '/' => Some((Token::Slash, SyntaxTokenKind::Operator)),
            '!' => Some((Token::Bang, SyntaxTokenKind::Operator)),
            '=' => Some((Token::Bind, SyntaxTokenKind::Operator)),
            '<' => Some((Token::Lt, SyntaxTokenKind::Operator)),
            '>' => Some((Token::Gt, SyntaxTokenKind::Operator)),
            '(' => Some((Token::LParen, SyntaxTokenKind::Delimiter)),
            ')' => Some((Token::RParen, SyntaxTokenKind::Delimiter)),
            '[' => Some((Token::LBrack, SyntaxTokenKind::Delimiter)),
            ']' => Some((Token::RBrack, SyntaxTokenKind::Delimiter)),
            '{' => Some((Token::LBrace, SyntaxTokenKind::Delimiter)),
            '}' => Some((Token::RBrace, SyntaxTokenKind::Delimiter)),
            ',' => Some((Token::Comma, SyntaxTokenKind::Punctuation)),
            ':' => Some((Token::Colon, SyntaxTokenKind::Punctuation)),
            '.' => Some((Token::Dot, SyntaxTokenKind::Punctuation)),
            '?' => Some((Token::Question, SyntaxTokenKind::Punctuation)),
            ';' => Some((Token::Semicolon, SyntaxTokenKind::Punctuation)),
            '|' => Some((Token::Pipe, SyntaxTokenKind::Punctuation)),
            '&' => Some((Token::Amp, SyntaxTokenKind::Punctuation)),
            '~' => Some((Token::Tilde, SyntaxTokenKind::Punctuation)),
            '$' => Some((Token::Dollar, SyntaxTokenKind::Punctuation)),
            '^' => Some((Token::Caret, SyntaxTokenKind::Punctuation)),
            _ => None,
        };
        if let Some((token, kind)) = token {
            push_both(
                &mut parser_tokens,
                &mut syntax_tokens,
                token,
                kind,
                i,
                i + 1,
            );
        } else {
            diagnostics.push(parse_diag(ParseError::syntax(
                format!("Unexpected character: '{c}'"),
                Span {
                    start: i,
                    end: i + 1,
                },
            )));
        }
        i += 1;
    }

    parser_tokens.push(Spanned {
        token: Token::Eof,
        span: Span {
            start: len,
            end: len,
        },
    });

    TolerantScan {
        parser_tokens,
        syntax_tokens,
        diagnostics,
    }
}

fn parse_tolerant_program(
    source: &str,
    tokens: &[Spanned<Token>],
    context: ParserContext,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Vec<Ast> {
    let mut parser = Parser::new(source, tokens, context);
    let mut stmts = Vec::new();
    parser.skip_newlines();

    while !matches!(parser.peek(), Token::Eof) {
        let start_pos = parser.pos;
        parser.synthetic_tokens.clear();
        let parsed =
            if matches!(parser.peek(), Token::Defmod) && parser.context.module_path.is_none() {
                parse_tolerant_defmod(&mut parser, diagnostics)
            } else if matches!(parser.peek(), Token::Impl) {
                let impl_pos = parser.pos;
                let impl_synthetic = parser.synthetic_tokens.clone();
                match parse_tolerant_plain_impl(&mut parser, diagnostics) {
                    Ok(stmt) => Ok(stmt),
                    Err(_) => {
                        parser.pos = impl_pos;
                        parser.synthetic_tokens = impl_synthetic;
                        parser.parse_stmt()
                    }
                }
            } else {
                parser.parse_stmt()
            };

        match parsed {
            Ok(stmt) => match parser.ensure_stmt_boundary(&stmt, false) {
                Ok(()) => stmts.push(stmt),
                Err(error) => {
                    diagnostics.push(ParseDiagnostic::from(error));
                    parser.pos = start_pos;
                    parser.synthetic_tokens.clear();
                    recover_to_boundary(&mut parser, false);
                }
            },
            Err(error) => {
                diagnostics.push(ParseDiagnostic::from(error));
                parser.pos = start_pos;
                parser.synthetic_tokens.clear();
                recover_to_boundary(&mut parser, false);
            }
        }

        parser.skip_newlines();
    }

    stmts
}

fn parse_tolerant_defmod(
    parser: &mut Parser<'_>,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<Ast, ParseError> {
    let sp = parser.peek_span();
    if parser.context.module_path.is_some() {
        return Err(ParseError::syntax(
            "Nested module declarations are not allowed",
            sp,
        ));
    }
    parser.expect(&Token::Defmod)?;
    let (name, _) = parser.expect_qualified_ident(2, "module")?;
    parser.skip_newlines();
    parser.expect(&Token::LBrace)?;

    let body = parse_tolerant_module_like_body(parser, Some(name.clone()), diagnostics)?;
    let end = parser.expect(&Token::RBrace)?;
    Ok(Ast::Defmod(
        Span {
            start: sp.start,
            end: end.end,
        },
        name,
        body,
        DeclAttrs::default(),
    ))
}

fn parse_tolerant_plain_impl(
    parser: &mut Parser<'_>,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<Ast, ParseError> {
    let sp = parser.peek_span();
    parser.expect(&Token::Impl)?;
    let (head, trait_args) = parser.parse_trait_impl_head()?;
    if !trait_args.is_empty() || matches!(parser.peek(), Token::For) {
        return Err(ParseError::syntax(
            "tolerant impl recovery only supports plain `impl Type { ... }` bodies",
            parser.peek_span(),
        ));
    }
    parser.skip_newlines();
    parser.expect(&Token::LBrace)?;
    let body = parse_tolerant_module_like_body(parser, Some(head.clone()), diagnostics)?;
    let end = parser.expect(&Token::RBrace)?;
    Ok(Ast::ImplDef(
        Span {
            start: sp.start,
            end: end.end,
        },
        head,
        body,
        DeclAttrs::default(),
    ))
}

fn parse_tolerant_module_like_body(
    parser: &mut Parser<'_>,
    module_path: Option<String>,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Result<Vec<Ast>, ParseError> {
    let prev_context = parser.context.clone();
    parser.context.level = DeclLevel::Top;
    parser.context.unit_kind = ParseUnitKind::Module;
    parser.context.module_path = module_path;
    parser.context.parse_rules = if prev_context
        .parse_rules
        .allowed_top_level_decl_kinds
        .allows(TopLevelDeclKind::BuiltinDecl)
    {
        ParseRules::std_module_member()
    } else {
        ParseRules::module_member()
    };

    let mut body = Vec::new();
    parser.skip_newlines();
    while !matches!(parser.peek(), Token::RBrace | Token::Eof) {
        let start_pos = parser.pos;
        parser.synthetic_tokens.clear();
        match parser.parse_stmt() {
            Ok(stmt) => match parser.ensure_stmt_boundary(&stmt, true) {
                Ok(()) => body.push(stmt),
                Err(error) => {
                    diagnostics.push(ParseDiagnostic::from(error));
                    parser.pos = start_pos;
                    parser.synthetic_tokens.clear();
                    recover_to_boundary(parser, true);
                }
            },
            Err(error) => {
                diagnostics.push(ParseDiagnostic::from(error));
                parser.pos = start_pos;
                parser.synthetic_tokens.clear();
                recover_to_boundary(parser, true);
            }
        }
        parser.skip_newlines();
    }

    if matches!(parser.peek(), Token::Eof) {
        parser.context = prev_context;
        return Err(ParseError::incomplete("}", parser.peek_span()));
    }

    parser.context = prev_context;
    Ok(body)
}

fn recover_to_boundary(parser: &mut Parser<'_>, allow_rbrace: bool) {
    let mut depth = 0usize;
    let mut consumed = false;
    loop {
        match parser.peek() {
            Token::Eof => break,
            Token::RBrace if allow_rbrace && depth == 0 => break,
            Token::Newline | Token::Semicolon if depth == 0 && consumed => {
                parser.advance();
                break;
            }
            Token::LParen | Token::LBrack | Token::LBrace => {
                depth += 1;
                parser.advance();
                consumed = true;
            }
            Token::RParen | Token::RBrack | Token::RBrace => {
                depth = depth.saturating_sub(1);
                parser.advance();
                consumed = true;
            }
            _ => {
                parser.advance();
                consumed = true;
            }
        }
    }
}

fn keyword_token(text: &str) -> (Token, SyntaxTokenKind) {
    let keyword = match text {
        "True" => Some(Token::True),
        "False" => Some(Token::False),
        "def" => Some(Token::Def),
        "defp" => Some(Token::Defp),
        "defagent" => Some(Token::Defagent),
        "defgenserver" => Some(Token::Defgenserver),
        "defsupervisor" => Some(Token::Defsupervisor),
        "defdynamic_supervisor" => Some(Token::DefdynamicSupervisor),
        "supervisor_init" => Some(Token::SupervisorInit),
        "defmod" => Some(Token::Defmod),
        "namespace" => Some(Token::Namespace),
        "deftrait" => Some(Token::Deftrait),
        "import" => Some(Token::Import),
        "include" => Some(Token::Include),
        "defstruct" => Some(Token::Defstruct),
        "defrecord" => Some(Token::Defrecord),
        "deferror" => Some(Token::Deferror),
        "defenum" => Some(Token::Defenum),
        "defextractor" => Some(Token::Defextractor),
        "impl" => Some(Token::Impl),
        "for" => Some(Token::For),
        "match" => Some(Token::Match),
        "when" => Some(Token::When),
        "cond" => Some(Token::Cond),
        "private" => Some(Token::Private),
        "public" => Some(Token::Public),
        "readonly" => Some(Token::Readonly),
        "const" => Some(Token::Const),
        "type" => Some(Token::Type),
        "where" => Some(Token::Where),
        _ => None,
    };
    match keyword {
        Some(Token::True) => (Token::True, SyntaxTokenKind::Bool),
        Some(Token::False) => (Token::False, SyntaxTokenKind::Bool),
        Some(token) => (token, SyntaxTokenKind::Keyword),
        None => (Token::Ident(text.to_string()), SyntaxTokenKind::Identifier),
    }
}

fn push_both(
    parser_tokens: &mut Vec<Spanned<Token>>,
    syntax_tokens: &mut Vec<SyntaxToken>,
    token: Token,
    kind: SyntaxTokenKind,
    start: usize,
    end: usize,
) {
    let span = Span { start, end };
    parser_tokens.push(Spanned {
        token,
        span: span.clone(),
    });
    syntax_tokens.push(SyntaxToken { kind, span });
}

fn push_syntax(tokens: &mut Vec<SyntaxToken>, kind: SyntaxTokenKind, start: usize, end: usize) {
    tokens.push(SyntaxToken {
        kind,
        span: Span { start, end },
    });
}

fn parse_diag(error: ParseError) -> ParseDiagnostic {
    let cursor_span = error.span().clone();
    ParseDiagnostic {
        error,
        expected_tokens: Vec::new(),
        cursor_span,
    }
}

fn infer_cursor_context(source: &str, cursor: usize) -> CursorSyntaxContext {
    let clamped = cursor.min(source.chars().count());
    let prefix = source.chars().take(clamped).collect::<String>();
    let trimmed = prefix.trim_end();
    let line = trimmed.rsplit('\n').next().unwrap_or("").trim_start();

    if trimmed.ends_with("::") {
        if line.starts_with("import ") {
            return CursorSyntaxContext::ImportPath;
        }
        return CursorSyntaxContext::QualifiedPath;
    }
    if line.starts_with("import ") {
        return CursorSyntaxContext::ImportPath;
    }
    if line.starts_with("def ")
        || line.starts_with("defp ")
        || line.starts_with("impl ")
        || line.starts_with("deftrait ")
        || line.starts_with("defmod ")
        || line.starts_with("@")
    {
        return CursorSyntaxContext::DeclHead;
    }
    if line.contains(':') && !line.contains('=') {
        return CursorSyntaxContext::Type;
    }
    if let Some(open) = trimmed.rfind('(') {
        if trimmed[open + 1..].contains(':') {
            return CursorSyntaxContext::CallArgValue;
        }
        return CursorSyntaxContext::CallArgName;
    }
    if trimmed.is_empty() {
        CursorSyntaxContext::Unknown
    } else {
        CursorSyntaxContext::Expr
    }
}

fn outline_from_ast(ast: &[Ast]) -> Vec<SyntaxOutlineItem> {
    ast.iter().filter_map(outline_item_from_ast).collect()
}

fn outline_item_from_ast(node: &Ast) -> Option<SyntaxOutlineItem> {
    let (kind, name, span, children) = match node {
        Ast::Def(span, name, ..) => (
            SyntaxOutlineKind::Function,
            Some(name.clone()),
            span,
            Vec::new(),
        ),
        Ast::ExtractorDef(span, name, ..) => (
            SyntaxOutlineKind::Extractor,
            Some(name.clone()),
            span,
            Vec::new(),
        ),
        Ast::ConstDef(span, name, ..) => (
            SyntaxOutlineKind::Const,
            Some(name.clone()),
            span,
            Vec::new(),
        ),
        Ast::StructDef(span, name, ..) => (
            SyntaxOutlineKind::Struct,
            Some(name.clone()),
            span,
            Vec::new(),
        ),
        Ast::RecordDef(span, name, ..) => (
            SyntaxOutlineKind::Record,
            Some(name.clone()),
            span,
            Vec::new(),
        ),
        Ast::DeferrorDef(span, name, ..) => (
            SyntaxOutlineKind::Error,
            Some(name.clone()),
            span,
            Vec::new(),
        ),
        Ast::EnumDef(span, name, ..) => (
            SyntaxOutlineKind::Enum,
            Some(name.clone()),
            span,
            Vec::new(),
        ),
        Ast::Defmod(span, name, body, _)
        | Ast::Defagent(span, name, body, ..)
        | Ast::Defgenserver(span, name, body, ..)
        | Ast::Defsupervisor(span, name, body, ..)
        | Ast::DefdynamicSupervisor(span, name, body, ..) => (
            SyntaxOutlineKind::Module,
            Some(name.clone()),
            span,
            outline_from_ast(body),
        ),
        Ast::ImplDef(span, name, body, _) => (
            SyntaxOutlineKind::Impl,
            Some(name.clone()),
            span,
            outline_from_ast(body),
        ),
        Ast::TraitDef(span, name, ..) => (
            SyntaxOutlineKind::Trait,
            Some(name.clone()),
            span,
            Vec::new(),
        ),
        Ast::TraitImplDef(span, trait_name, _, target, ..) => (
            SyntaxOutlineKind::TraitImpl,
            Some(format!("impl {trait_name} for {target:?}")),
            span,
            Vec::new(),
        ),
        Ast::Import(span, path, _) => (
            SyntaxOutlineKind::Import,
            Some(path.segments.join("::")),
            span,
            Vec::new(),
        ),
        Ast::Include(span, path) => (
            SyntaxOutlineKind::Include,
            Some(path.clone()),
            span,
            Vec::new(),
        ),
        _ => return None,
    };

    Some(SyntaxOutlineItem {
        kind,
        name,
        span: span.clone(),
        selection_span: span.clone(),
        children,
    })
}

fn outline_from_source(source: &str) -> Vec<SyntaxOutlineItem> {
    let mut items = Vec::new();
    let mut offset = 0usize;
    for line in source.lines() {
        let trimmed_start = line.len() - line.trim_start().len();
        let trimmed = line.trim_start();
        let start = offset + line[..trimmed_start].chars().count();
        if let Some(item) = outline_from_line(trimmed, start) {
            items.push(item);
        }
        offset += line.chars().count() + 1;
    }
    items
}

fn outline_from_line(line: &str, start: usize) -> Option<SyntaxOutlineItem> {
    let (kind, prefix) = if line.starts_with("def ") || line.starts_with("defp ") {
        (
            SyntaxOutlineKind::Function,
            if line.starts_with("defp ") {
                "defp "
            } else {
                "def "
            },
        )
    } else if line.starts_with("defmod ") {
        (SyntaxOutlineKind::Module, "defmod ")
    } else if line.starts_with("impl ") {
        (SyntaxOutlineKind::Impl, "impl ")
    } else if line.starts_with("deftrait ") {
        (SyntaxOutlineKind::Trait, "deftrait ")
    } else if line.starts_with("const ")
        || line.starts_with("public const ")
        || line.starts_with("private const ")
    {
        let prefix = if line.starts_with("public const ") {
            "public const "
        } else if line.starts_with("private const ") {
            "private const "
        } else {
            "const "
        };
        (SyntaxOutlineKind::Const, prefix)
    } else {
        return None;
    };
    let rest = &line[prefix.len()..];
    let name = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':'))
        .collect::<String>();
    if name.is_empty() {
        return None;
    }
    let name_start = start + prefix.chars().count();
    let name_end = name_start + name.chars().count();
    let span = Span {
        start,
        end: start + line.chars().count(),
    };
    Some(SyntaxOutlineItem {
        kind,
        name: Some(name),
        span,
        selection_span: Span {
            start: name_start,
            end: name_end,
        },
        children: Vec::new(),
    })
}

fn merge_outline_items(
    mut ast_items: Vec<SyntaxOutlineItem>,
    source_items: Vec<SyntaxOutlineItem>,
) -> Vec<SyntaxOutlineItem> {
    for item in source_items {
        if !outline_contains(&ast_items, &item) {
            ast_items.push(item);
        }
    }
    ast_items.sort_by_key(|item| item.span.start);
    ast_items
}

fn outline_contains(items: &[SyntaxOutlineItem], needle: &SyntaxOutlineItem) -> bool {
    items.iter().any(|item| {
        item.kind == needle.kind
            && item.name == needle.name
            && (ranges_overlap(&item.span, &needle.span)
                || outline_contains(&item.children, needle))
    })
}

fn ranges_overlap(left: &Span, right: &Span) -> bool {
    let left_end = left.end.max(left.start + 1);
    let right_end = right.end.max(right.start + 1);
    left.start < right_end && right.start < left_end
}
