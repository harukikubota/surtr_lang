//! Source-aware normal string scanning. Escapes are decoded once, before AST construction.
use crate::ast::Span;
use crate::error::{ParseError, ParseErrorReason};

#[derive(Debug, Clone, PartialEq)]
pub enum StringLiteral {
    Plain(String),
    Interpolated(Vec<StringPart>),
}

impl StringLiteral {
    pub(crate) fn into_static(self, token_span: Span) -> Result<String, ParseError> {
        match self {
            Self::Plain(value) => Ok(value),
            Self::Interpolated(parts) => {
                let marker_span = parts
                    .into_iter()
                    .find_map(|part| match part {
                        StringPart::Expr { span, .. } => Some(Span {
                            start: span.start - 2,
                            end: span.end + 1,
                        }),
                        StringPart::Text(_) => None,
                    })
                    .ok_or_else(|| {
                        ParseError::syntax(
                            ParseErrorReason::CompilerInvariant,
                            "Interpolated string token contains no source interpolation",
                            token_span,
                        )
                    })?;
                Err(ParseError::syntax(
                    ParseErrorReason::LiteralSyntax,
                    "String interpolation is not allowed in static string literals; use \\#{ for literal text",
                    marker_span,
                ))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Text(String),
    Expr { source: String, span: Span },
}

fn invalid(message: impl Into<String>, start: usize, end: usize) -> ParseError {
    ParseError::syntax(
        ParseErrorReason::LiteralSyntax,
        message,
        Span { start, end },
    )
}

/// Validated scalar and the next unconsumed source position.
struct Escape {
    scalar: u32,
    end: usize,
}

/// Validate an escape without creating its decoded character. Nested string
/// boundary scanning can share the rules without constructing a string value.
fn escape(chars: &[char], start: usize) -> Result<Escape, ParseError> {
    let Some(&kind) = chars.get(start + 1) else {
        return Err(ParseError::incomplete(
            "string escape",
            Span {
                start,
                end: chars.len(),
            },
        ));
    };
    if kind == '#' && chars.get(start + 2).is_none() {
        return Err(ParseError::incomplete(
            "'{' after \\#",
            Span {
                start,
                end: chars.len(),
            },
        ));
    }
    let simple = match kind {
        'n' => Some('\n'),
        't' => Some('\t'),
        '\\' => Some('\\'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '#' if chars.get(start + 2) == Some(&'{') => Some('#'),
        _ => None,
    };
    if let Some(value) = simple {
        return Ok(Escape {
            scalar: value as u32,
            end: start + 2,
        });
    }
    if kind != 'u' {
        return Err(invalid(
            format!("Unsupported string escape: \\{kind}"),
            start,
            start + 2,
        ));
    }
    let Some(&opening) = chars.get(start + 2) else {
        return Err(ParseError::incomplete(
            "'{' after \\u",
            Span {
                start,
                end: chars.len(),
            },
        ));
    };
    if opening != '{' {
        return Err(invalid("Expected '{' after \\u", start, start + 3));
    }
    let mut i = start + 3;
    let mut digits = 0;
    let mut value = 0u32;
    loop {
        let Some(&ch) = chars.get(i) else {
            // A valid prefix can always be closed. Surrogates can also become valid
            // by appending another digit, but six-digit invalid values cannot.
            return Err(ParseError::incomplete(
                "'}' in Unicode escape",
                Span { start, end: i },
            ));
        };
        if ch == '}' {
            if digits == 0 {
                return Err(invalid(
                    "Unicode escape requires at least one hexadecimal digit",
                    start,
                    i + 1,
                ));
            }
            if (0xd800..=0xdfff).contains(&value) {
                return Err(invalid(
                    "Unicode escape must not contain a surrogate value",
                    start,
                    i + 1,
                ));
            }
            if value > 0x10ffff {
                return Err(invalid(
                    "Unicode escape value is outside the Unicode scalar range",
                    start,
                    i + 1,
                ));
            }
            return Ok(Escape {
                scalar: value,
                end: i + 1,
            });
        }
        if ch == '"' || ch == '\'' {
            return Err(invalid(
                "Expected '}' in Unicode escape before closing quote",
                start,
                i,
            ));
        }
        let Some(digit) = ch.to_digit(16).filter(|_| ch.is_ascii_hexdigit()) else {
            return Err(invalid(
                "Unicode escape requires hexadecimal digits",
                start,
                i + 1,
            ));
        };
        digits += 1;
        if digits > 6 {
            return Err(invalid(
                "Unicode escape allows at most six hexadecimal digits",
                start,
                i + 1,
            ));
        }
        value = value * 16 + digit;
        i += 1;
        // Once six digits have been read, appending cannot repair an invalid
        // scalar. Reject it even at EOF rather than asking for more input.
        if digits == 6 {
            let end = i + usize::from(chars.get(i) == Some(&'}'));
            if value > 0x10ffff {
                return Err(invalid(
                    "Unicode escape value is outside the Unicode scalar range",
                    start,
                    end,
                ));
            }
            if (0xd800..=0xdfff).contains(&value) {
                return Err(invalid(
                    "Unicode escape must not contain a surrogate value",
                    start,
                    end,
                ));
            }
        }
    }
}

fn check_nesting(depth: usize, start: usize) -> Result<(), ParseError> {
    if depth > crate::parser::MAX_PARSE_NESTING {
        return Err(ParseError::syntax(
            ParseErrorReason::PositionRule,
            crate::parser::MAX_PARSE_NESTING_MESSAGE,
            Span {
                start,
                end: start + 1,
            },
        ));
    }
    Ok(())
}

/// Locate a nested string boundary without decoding its contents. Its own
/// lexer invocation validates and decodes the text when the expression parses.
fn string_boundary(chars: &[char], start: usize, nesting: usize) -> Result<usize, ParseError> {
    check_nesting(nesting, start)?;
    let quote = chars[start];
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            ch if ch == quote => return Ok(i + 1),
            '\\' => i = escape(chars, i)?.end,
            '#' if chars.get(i + 1) == Some(&'{') => {
                i = interpolation_end_nested(chars, i + 2, nesting + 1)? + 1;
            }
            _ => i += 1,
        }
    }
    Err(ParseError::incomplete(
        quote.to_string(),
        Span { start, end: i },
    ))
}

/// Find a source interpolation's matching brace, respecting nested literals
/// and comments without decoding the expression as outer string text.
pub(crate) fn interpolation_end(chars: &[char], start: usize) -> Result<usize, ParseError> {
    interpolation_end_nested(chars, start, 1)
}

fn interpolation_end_nested(
    chars: &[char],
    start: usize,
    nesting: usize,
) -> Result<usize, ParseError> {
    check_nesting(nesting, start)?;
    let mut depth = 1usize;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '"' if chars.get(i + 1) == Some(&'"') && chars.get(i + 2) == Some(&'"') => {
                let (_, next) = crate::lexer::lex_raw_triple_quoted_string(chars, i, chars.len())?;
                i = next;
            }
            '"' | '\'' => {
                i = string_boundary(chars, i, nesting + depth - 1)?;
            }
            '`' => {
                i += 1;
                while i < chars.len() && chars[i] != '`' {
                    i += 1;
                }
                if i == chars.len() {
                    return Err(ParseError::incomplete("`", Span { start, end: i }));
                }
                i += 1;
            }
            '#' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '{' => {
                depth += 1;
                check_nesting(nesting + depth - 1, i)?;
                i += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    Err(ParseError::incomplete("}", Span { start, end: i }))
}

pub(crate) fn scan_string(
    chars: &[char],
    start: usize,
) -> Result<(StringLiteral, usize), ParseError> {
    let quote = chars[start];
    let mut i = start + 1;
    let mut text = String::new();
    let mut parts = Vec::new();
    while i < chars.len() {
        match chars[i] {
            ch if ch == quote => {
                if parts.is_empty() {
                    return Ok((StringLiteral::Plain(text), i + 1));
                }
                if !text.is_empty() {
                    parts.push(StringPart::Text(text));
                }
                return Ok((StringLiteral::Interpolated(parts), i + 1));
            }
            '\\' => {
                let encoded = escape(chars, i)?;
                let decoded = char::from_u32(encoded.scalar).ok_or_else(|| {
                    ParseError::syntax(
                        ParseErrorReason::CompilerInvariant,
                        "Validated string escape is not a Unicode scalar",
                        Span {
                            start: i,
                            end: encoded.end,
                        },
                    )
                })?;
                text.push(decoded);
                i = encoded.end;
            }
            '#' if chars.get(i + 1) == Some(&'{') => {
                if !text.is_empty() {
                    parts.push(StringPart::Text(std::mem::take(&mut text)));
                }
                let expr_start = i + 2;
                let end = interpolation_end(chars, expr_start)?;
                parts.push(StringPart::Expr {
                    source: chars[expr_start..end].iter().collect(),
                    span: Span {
                        start: expr_start,
                        end,
                    },
                });
                i = end + 1;
            }
            ch => {
                text.push(ch);
                i += 1;
            }
        }
    }
    Err(ParseError::incomplete(
        quote.to_string(),
        Span { start, end: i },
    ))
}
