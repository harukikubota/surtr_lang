use crate::ast::*;
use crate::error::ParseError;

use super::Parser;

impl Parser<'_> {
    pub(super) fn parse_string_or_interpolated(
        &mut self,
        span: Span,
        raw: String,
    ) -> Result<Ast, ParseError> {
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

            let parsed = super::parse(&expr_src).map_err(|e| {
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
            let expr = super::shift_ast_span(parsed.into_iter().next().unwrap(), expr_offset);
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
