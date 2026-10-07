use crate::ast::*;
use crate::error::ParseError;

use super::Parser;

impl Parser<'_> {
    pub(super) fn string_has_interpolation(raw: &str) -> bool {
        let chars: Vec<char> = raw.chars().collect();
        let mut i = 0;
        while i + 1 < chars.len() {
            if chars[i] == '#' && chars[i + 1] == '{' && (i == 0 || chars[i - 1] != '\\') {
                return true;
            }
            i += 1;
        }
        false
    }

    pub(super) fn parse_string_or_interpolated(
        &mut self,
        span: Span,
        literal: crate::string_literal::StringLiteral,
    ) -> Result<Ast, ParseError> {
        use crate::string_literal::{StringLiteral, StringPart};
        let source_parts = match literal {
            StringLiteral::Plain(value) => return Ok(Ast::Lit(span, Lit::Str(value))),
            StringLiteral::Interpolated(parts) => parts,
        };
        let mut parts = Vec::new();
        for part in source_parts {
            match part {
                StringPart::Text(text) => parts.push(InterpolatedPart::Text(text)),
                StringPart::Expr {
                    source,
                    span: expr_span,
                } => {
                    let parsed = super::parse(&source).map_err(|error| {
                        error.map_spans(|span| Span {
                            start: expr_span.start + span.start,
                            end: expr_span.start + span.end,
                        })
                    })?;
                    let [expr] = <[Ast; 1]>::try_from(parsed).map_err(|_| {
                        ParseError::syntax(
                            crate::error::ParseErrorReason::InterpolationSyntax,
                            "Interpolation expression must contain exactly one expression",
                            expr_span.clone(),
                        )
                    })?;
                    parts.push(InterpolatedPart::Expr(Box::new(super::shift_ast_span(
                        expr,
                        expr_span.start,
                    ))));
                }
            }
        }
        Ok(Ast::InterpolatedStr(span, parts))
    }

    pub(super) fn parse_triple_string_or_interpolated(
        &mut self,
        span: Span,
        raw: crate::token::RawStringLiteral,
    ) -> Result<Ast, ParseError> {
        // Raw text keeps its existing decoding and dedent contract.
        let parts = self.parse_raw_interpolated_parts(&raw, &span)?;
        if parts.is_empty() {
            Ok(Ast::Lit(span, Lit::Str(raw.text)))
        } else if let [InterpolatedPart::Text(_)] = parts.as_slice() {
            let [part] = <[InterpolatedPart; 1]>::try_from(parts).map_err(|parts| {
                ParseError::syntax(
                    crate::error::ParseErrorReason::InterpolationSyntax,
                    format!(
                        "Interpolated string text simplification expected one part, got {}",
                        parts.len()
                    ),
                    span.clone(),
                )
            })?;
            match part {
                InterpolatedPart::Text(text) => Ok(Ast::Lit(span, Lit::Str(text))),
                InterpolatedPart::Expr(_) => Ok(Ast::InterpolatedStr(span, vec![part])),
            }
        } else {
            Ok(Ast::InterpolatedStr(span, parts))
        }
    }

    fn parse_raw_interpolated_parts(
        &mut self,
        raw: &crate::token::RawStringLiteral,
        base_span: &Span,
    ) -> Result<Vec<InterpolatedPart>, ParseError> {
        let chars: Vec<char> = raw.text.chars().collect();
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

            let map = |span: Span| {
                raw.source_span(Span {
                    start: expr_start + span.start,
                    end: expr_start + span.end,
                })
            };
            let parsed = super::parse(&expr_src).map_err(|error| ParseError::SyntaxError {
                message: format!("Invalid interpolation expression: {}", error.message()),
                span: map(error.span().clone()),
                reason: crate::error::ParseErrorReason::InterpolationSyntax,
                expected_tokens: error.expected_tokens().to_vec(),
                cursor_span: map(error.cursor_span().clone()),
                guidance: error.guidance().cloned(),
                token_kind: error.token_kind().map(str::to_owned),
            })?;
            if parsed.len() != 1 {
                return Err(ParseError::syntax(
                    crate::error::ParseErrorReason::InterpolationSyntax,
                    "Interpolation expression must contain exactly one expression",
                    base_span.clone(),
                ));
            }
            let [expr] = <[Ast; 1]>::try_from(parsed).map_err(|parsed| {
                ParseError::syntax(
                    crate::error::ParseErrorReason::InterpolationSyntax,
                    format!(
                        "Interpolation expression must contain exactly one expression, got {}",
                        parsed.len()
                    ),
                    base_span.clone(),
                )
            })?;
            let expr = super::map_ast_span(expr, &map);
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
