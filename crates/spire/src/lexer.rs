use crate::ast::Span;
use crate::error::ParseError;
use crate::token::{Spanned, Token};
use sindr::primitives::SurtrInt;

#[derive(Clone, Copy)]
enum IntLiteralBase {
    Bin,
    Oct,
    Dec,
    Hex,
}

impl IntLiteralBase {
    fn radix(self) -> u32 {
        match self {
            Self::Bin => 2,
            Self::Oct => 8,
            Self::Dec => 10,
            Self::Hex => 16,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Bin => "binary",
            Self::Oct => "octal",
            Self::Dec => "decimal",
            Self::Hex => "hexadecimal",
        }
    }
}

pub fn tokenize(source: &str) -> Result<Vec<Spanned<Token>>, ParseError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        // Skip spaces and tabs
        if c == ' ' || c == '\t' || c == '\r' {
            i += 1;
            continue;
        }

        // Comment
        if c == '#' {
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Newline
        if c == '\n' {
            tokens.push(Spanned {
                token: Token::Newline,
                span: Span {
                    start: i,
                    end: i + 1,
                },
            });
            i += 1;
            continue;
        }

        // Annotator: @@builtin, @@foo, ...
        if c == '@' {
            let start = i;
            if i + 1 < len && chars[i + 1] == '@' {
                i += 2; // skip '@@'
                let name_start = i;
                while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let name: String = chars[name_start..i].iter().collect();
                if name.is_empty() {
                    return Err(ParseError::syntax(
                        "Expected annotator name after '@@'",
                        Span { start, end: i },
                    ));
                }
                tokens.push(Spanned {
                    token: Token::Annotator(name),
                    span: Span { start, end: i },
                });
                continue;
            }
            tokens.push(Spanned {
                token: Token::At,
                span: Span {
                    start,
                    end: start + 1,
                },
            });
            i += 1;
            continue;
        }

        // Raw triple-quoted string. Body indentation is checked against the
        // indentation of the line that starts the string, matching @@doc.
        if c == '"' && i + 2 < len && chars[i + 1] == '"' && chars[i + 2] == '"' {
            let (token, next) = lex_raw_triple_quoted_string(&chars, i, len)?;
            tokens.push(token);
            i = next;
            continue;
        }

        // String — double quote
        if c == '"' {
            let start = i;
            i += 1;
            let mut s = String::new();
            while i < len && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < len {
                    i += 1;
                    match chars[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        '\'' => s.push('\''),
                        other => {
                            s.push('\\');
                            s.push(other);
                        }
                    }
                } else {
                    s.push(chars[i]);
                }
                i += 1;
            }
            if i >= len {
                return Err(ParseError::incomplete("\"", Span { start, end: i }));
            }
            i += 1;
            tokens.push(Spanned {
                token: Token::Str(s),
                span: Span { start, end: i },
            });
            continue;
        }

        // String — single quote
        if c == '\'' {
            let start = i;
            i += 1;
            let mut s = String::new();
            while i < len && chars[i] != '\'' {
                if chars[i] == '\\' && i + 1 < len {
                    i += 1;
                    match chars[i] {
                        'n' => s.push('\n'),
                        't' => s.push('\t'),
                        '\\' => s.push('\\'),
                        '\'' => s.push('\''),
                        '"' => s.push('"'),
                        other => {
                            s.push('\\');
                            s.push(other);
                        }
                    }
                } else {
                    s.push(chars[i]);
                }
                i += 1;
            }
            if i >= len {
                return Err(ParseError::incomplete("'", Span { start, end: i }));
            }
            i += 1;
            tokens.push(Spanned {
                token: Token::Str(s),
                span: Span { start, end: i },
            });
            continue;
        }

        // FuncLiteral — backtick-quoted function name or operator
        if c == '`' {
            let start = i;
            i += 1;
            let body_start = i;
            while i < len && chars[i] != '`' {
                i += 1;
            }
            if i >= len {
                return Err(ParseError::incomplete("`", Span { start, end: i }));
            }
            let body: String = chars[body_start..i].iter().collect();
            if body.is_empty() {
                return Err(ParseError::syntax(
                    "FuncLiteral body must not be empty",
                    Span { start, end: i + 1 },
                ));
            }

            let is_ident = {
                let mut chars = body.chars();
                matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
                    && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            };
            let is_qualified_ident = body
                .split("::")
                .map(str::trim)
                .collect::<Vec<_>>()
                .as_slice()
                .split_first()
                .is_some_and(|(_, rest)| {
                    !rest.is_empty() && body.split("::").all(|segment| {
                        let mut chars = segment.chars();
                        matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
                            && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                    })
                });
            let is_supported_operator = matches!(
                body.as_str(),
                "+" | "-" | "*" | "++" | "==" | "!=" | "<" | ">" | "<=" | ">="
            );
            if !is_ident && !is_qualified_ident && !is_supported_operator {
                return Err(ParseError::syntax(
                    format!("Unsupported FuncLiteral body: `{}`", body),
                    Span { start, end: i + 1 },
                ));
            }

            i += 1;
            tokens.push(Spanned {
                token: Token::FuncLiteral(body),
                span: Span { start, end: i },
            });
            continue;
        }

        // Numbers
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
                let text: String = chars[start..i].iter().collect();
                let val: f64 = text.parse().map_err(|_| {
                    ParseError::syntax(format!("Invalid float: {}", text), Span { start, end: i })
                })?;
                tokens.push(Spanned {
                    token: Token::Float(val),
                    span: Span { start, end: i },
                });
            } else {
                let (val, next) = lex_integer_literal(&chars, start, i, len)?;
                tokens.push(Spanned {
                    token: Token::Int(val),
                    span: Span { start, end: next },
                });
                i = next;
            }
            continue;
        }

        // Identifiers and keywords
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let token = match text.as_str() {
                "True" => Token::True,
                "False" => Token::False,
                "def" => Token::Def,
                "defp" => Token::Defp,
                "defagent" => Token::Defagent,
                "defmod" => Token::Defmod,
                "namespace" => Token::Namespace,
                "deftrait" => Token::Deftrait,
                "import" => Token::Import,
                "include" => Token::Include,
                "defstruct" => Token::Defstruct,
                "defrecord" => Token::Defrecord,
                "deferror" => Token::Deferror,
                "defenum" => Token::Defenum,
                "defextractor" => Token::Defextractor,
                "impl" => Token::Impl,
                "for" => Token::For,
                "match" => Token::Match,
                "when" => Token::When,
                "cond" => Token::Cond,
                "private" => Token::Private,
                "public" => Token::Public,
                "const" => Token::Const,
                "type" => Token::Type,
                "where" => Token::Where,
                _ => Token::Ident(text),
            };
            tokens.push(Spanned {
                token,
                span: Span { start, end: i },
            });
            continue;
        }

        // Three-character operators
        if i + 2 < len {
            let three: String = chars[i..i + 3].iter().collect();
            let tok = match three.as_str() {
                "|*>" => Some(Token::PipeMap),
                "|>=" => Some(Token::PipeBind),
                ">=>" => Some(Token::KleisliCompose),
                _ => None,
            };
            if let Some(t) = tok {
                tokens.push(Spanned {
                    token: t,
                    span: Span {
                        start: i,
                        end: i + 3,
                    },
                });
                i += 3;
                continue;
            }
        }

        // Two-character operators
        if i + 1 < len {
            let two: String = chars[i..i + 2].iter().collect();
            if two == "||" && matches!(tokens.last().map(|sp| &sp.token), Some(Token::LBrace)) {
                tokens.push(Spanned {
                    token: Token::Pipe,
                    span: Span {
                        start: i,
                        end: i + 1,
                    },
                });
                tokens.push(Spanned {
                    token: Token::Pipe,
                    span: Span {
                        start: i + 1,
                        end: i + 2,
                    },
                });
                i += 2;
                continue;
            }
            let tok = match two.as_str() {
                "++" => Some(Token::Concat),
                "=?" => Some(Token::SafeBind),
                "==" => Some(Token::EqEq),
                "!=" => Some(Token::BangEq),
                "<=" => Some(Token::LtEq),
                ">=" => Some(Token::GtEq),
                "&&" => Some(Token::AndAnd),
                "||" => Some(Token::OrOr),
                ".." => Some(Token::DotDot),
                "=>" => Some(Token::FatArrow),
                "->" => Some(Token::Arrow),
                "|>" => Some(Token::PipeApply),
                ">>" => Some(Token::Compose),
                ">*" => Some(Token::LiftCompose),
                _ => None,
            };
            if let Some(t) = tok {
                tokens.push(Spanned {
                    token: t,
                    span: Span {
                        start: i,
                        end: i + 2,
                    },
                });
                i += 2;
                continue;
            }
        }

        // Single-character tokens
        let start = i;
        let token = match c {
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '!' => Token::Bang,
            '=' => Token::Bind,
            '<' => Token::Lt,
            '>' => Token::Gt,
            '(' => {
                if i + 1 < len && chars[i + 1] == ')' {
                    i += 2;
                    tokens.push(Spanned {
                        token: Token::Unit,
                        span: Span { start, end: i },
                    });
                    continue;
                }
                Token::LParen
            }
            ')' => Token::RParen,
            '[' => Token::LBrack,
            ']' => Token::RBrack,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            ',' => Token::Comma,
            ':' => Token::Colon,
            '.' => Token::Dot,
            ';' => Token::Semicolon,
            '|' => Token::Pipe,
            '&' => Token::Amp,
            '$' => Token::Dollar,
            _ => {
                return Err(ParseError::syntax(
                    format!("Unexpected character: '{}'", c),
                    Span {
                        start: i,
                        end: i + 1,
                    },
                ));
            }
        };
        tokens.push(Spanned {
            token,
            span: Span { start, end: i + 1 },
        });
        i += 1;
    }

    tokens.push(Spanned {
        token: Token::Eof,
        span: Span { start: i, end: i },
    });
    Ok(tokens)
}

fn lex_integer_literal(
    chars: &[char],
    start: usize,
    decimal_end: usize,
    len: usize,
) -> Result<(SurtrInt, usize), ParseError> {
    if chars[start] == '0' && decimal_end == start + 1 && decimal_end < len {
        let prefix = chars[decimal_end];
        let base = match prefix {
            'b' => Some(IntLiteralBase::Bin),
            'o' => Some(IntLiteralBase::Oct),
            'd' => Some(IntLiteralBase::Dec),
            'x' => Some(IntLiteralBase::Hex),
            'B' | 'O' | 'D' | 'X' => {
                let mut end = decimal_end + 1;
                while end < len && chars[end].is_ascii_alphanumeric() {
                    end += 1;
                }
                let text: String = chars[start..end].iter().collect();
                return Err(ParseError::syntax(
                    format!("Invalid integer: {}", text),
                    Span { start, end },
                ));
            }
            _ => None,
        };

        if let Some(base) = base {
            let body_start = decimal_end + 1;
            if body_start >= len || !chars[body_start].is_ascii_alphanumeric() {
                let prefix_text = format!("0{}", prefix);
                return Err(ParseError::syntax(
                    format!("missing digits after integer base prefix: {}", prefix_text),
                    Span {
                        start,
                        end: body_start,
                    },
                ));
            }

            let mut end = body_start;
            while end < len && chars[end].is_ascii_alphanumeric() {
                end += 1;
            }

            for ch in &chars[body_start..end] {
                if !is_valid_int_digit(*ch, base) {
                    return Err(ParseError::syntax(
                        format!("invalid digit for {} integer literal: {}", base.label(), ch),
                        Span {
                            start,
                            end: end.min(body_start + 1),
                        },
                    ));
                }
            }

            let body: String = chars[body_start..end].iter().collect();
            let val = SurtrInt::parse_bytes(body.as_bytes(), base.radix()).ok_or_else(|| {
                ParseError::syntax(
                    format!("Invalid integer: 0{}{}", prefix, body),
                    Span { start, end },
                )
            })?;
            return Ok((val, end));
        }
    }

    let text: String = chars[start..decimal_end].iter().collect();
    let val: SurtrInt = text.parse().map_err(|_| {
        ParseError::syntax(
            format!("Invalid integer: {}", text),
            Span {
                start,
                end: decimal_end,
            },
        )
    })?;
    Ok((val, decimal_end))
}

fn is_valid_int_digit(ch: char, base: IntLiteralBase) -> bool {
    match base {
        IntLiteralBase::Bin => matches!(ch, '0' | '1'),
        IntLiteralBase::Oct => matches!(ch, '0'..='7'),
        IntLiteralBase::Dec => ch.is_ascii_digit(),
        IntLiteralBase::Hex => ch.is_ascii_digit() || matches!(ch, 'a'..='f' | 'A'..='F'),
    }
}

fn lex_raw_triple_quoted_string(
    chars: &[char],
    start: usize,
    len: usize,
) -> Result<(Spanned<Token>, usize), ParseError> {
    let content_start = start + 3;
    let mut content_end = content_start;
    while content_end + 2 < len
        && !(chars[content_end] == '"'
            && chars[content_end + 1] == '"'
            && chars[content_end + 2] == '"')
    {
        content_end += 1;
    }
    if content_end + 2 >= len {
        return Err(ParseError::incomplete("\"\"\"", Span { start, end: len }));
    }

    let content = normalize_triple_quoted_string(chars, start, content_start, content_end)?;
    let end = content_end + 3;
    Ok((
        Spanned {
            token: Token::DocString(content),
            span: Span { start, end },
        },
        end,
    ))
}

fn normalize_triple_quoted_string(
    chars: &[char],
    quote_start: usize,
    content_start: usize,
    content_end: usize,
) -> Result<String, ParseError> {
    let base_indent = line_indent_before(chars, quote_start);
    let mut out = String::new();
    let mut i = content_start;
    let mut at_line_start = content_start == 0 || chars[content_start - 1] == '\n';

    while i < content_end {
        if !at_line_start {
            while i < content_end && chars[i] != '\n' {
                out.push(chars[i]);
                i += 1;
            }
            if i < content_end {
                out.push(chars[i]);
                i += 1;
                at_line_start = true;
            }
            continue;
        }

        let mut columns = 0usize;
        let mut indent_chars = Vec::new();
        while i < content_end {
            match chars[i] {
                ' ' => {
                    columns += 1;
                    indent_chars.push(chars[i]);
                    i += 1;
                }
                '\t' => {
                    columns += 4 - (columns % 4);
                    indent_chars.push(chars[i]);
                    i += 1;
                }
                '\r' => {
                    i += 1;
                }
                '\n' => {
                    out.push(chars[i]);
                    i += 1;
                    break;
                }
                _ => {
                    if columns < base_indent {
                        return Err(ParseError::syntax(
                            "Triple-quoted string content must be indented at least as far as the starting line",
                            Span {
                                start: i,
                                end: i + 1,
                            },
                        ));
                    }
                    push_indent_after_base(&mut out, &indent_chars, base_indent);
                    while i < content_end && chars[i] != '\n' {
                        out.push(chars[i]);
                        i += 1;
                    }
                    if i < content_end {
                        out.push(chars[i]);
                        i += 1;
                    }
                    break;
                }
            }
        }
        at_line_start = true;
    }

    Ok(out)
}

fn push_indent_after_base(out: &mut String, indent_chars: &[char], base_indent: usize) {
    let mut columns = 0usize;
    let mut keep_from = indent_chars.len();
    for (idx, ch) in indent_chars.iter().enumerate() {
        let next_columns = match ch {
            ' ' => columns + 1,
            '\t' => columns + (4 - (columns % 4)),
            _ => columns,
        };
        if next_columns > base_indent {
            keep_from = idx + 1;
            break;
        }
        columns = next_columns;
        if columns == base_indent {
            keep_from = idx + 1;
            break;
        }
    }
    for ch in &indent_chars[keep_from..] {
        out.push(*ch);
    }
}

fn line_indent_before(chars: &[char], idx: usize) -> usize {
    let mut line_start = idx;
    while line_start > 0 && chars[line_start - 1] != '\n' {
        line_start -= 1;
    }

    let mut columns = 0usize;
    let mut i = line_start;
    while i < idx {
        match chars[i] {
            ' ' => columns += 1,
            '\t' => columns += 4 - (columns % 4),
            '\r' => {}
            _ => break,
        }
        i += 1;
    }
    columns
}

#[cfg(test)]
mod tests {
    use super::*;
    use sindr::primitives::int;

    #[test]
    fn test_basic_tokens() {
        let tokens = tokenize("num = 10").unwrap();
        assert!(matches!(tokens[0].token, Token::Ident(ref s) if s == "num"));
        assert!(matches!(tokens[1].token, Token::Bind));
        assert!(matches!(tokens[2].token, Token::Int(ref n) if n == &int(10)));
    }

    #[test]
    fn test_float() {
        let tokens = tokenize("2.5").unwrap();
        assert!(matches!(tokens[0].token, Token::Float(f) if (f - 2.5).abs() < 1e-10));
    }

    #[test]
    fn test_int_base_literals() {
        let tokens = tokenize("123 0d123 0xff 0o17 0b1101").unwrap();
        assert!(matches!(tokens[0].token, Token::Int(ref n) if n == &int(123)));
        assert!(matches!(tokens[1].token, Token::Int(ref n) if n == &int(123)));
        assert!(matches!(tokens[2].token, Token::Int(ref n) if n == &int(255)));
        assert!(matches!(tokens[3].token, Token::Int(ref n) if n == &int(15)));
        assert!(matches!(tokens[4].token, Token::Int(ref n) if n == &int(13)));
    }

    #[test]
    fn test_int_hex_rejects_uppercase_prefix() {
        let err = tokenize("0Xff").expect_err("expected invalid integer literal");
        assert!(err.message().contains("Invalid integer"));
    }

    #[test]
    fn test_int_base_prefix_requires_digits() {
        for literal in ["0x", "0o", "0b", "0d"] {
            let err = tokenize(literal).expect_err("expected missing digits error");
            assert!(err
                .message()
                .contains("missing digits after integer base prefix"));
        }
    }

    #[test]
    fn test_int_base_rejects_invalid_digits() {
        let cases = [
            ("0o18", "invalid digit for octal integer literal: 8"),
            ("0b102", "invalid digit for binary integer literal: 2"),
            ("0xfg", "invalid digit for hexadecimal integer literal: g"),
        ];

        for (literal, expected) in cases {
            let err = tokenize(literal).expect_err("expected invalid digit error");
            assert!(err.message().contains(expected), "got: {}", err.message());
        }
    }

    #[test]
    fn test_string_escape() {
        let tokens = tokenize(r#""hello\nworld""#).unwrap();
        assert!(matches!(tokens[0].token, Token::Str(ref s) if s == "hello\nworld"));
    }

    #[test]
    fn test_doc_string_token() {
        let tokens = tokenize("@@doc \"\"\"\nHello\n\"\"\"").unwrap();
        assert!(matches!(tokens[0].token, Token::Annotator(ref name) if name == "doc"));
        assert!(matches!(tokens[1].token, Token::DocString(ref s) if s == "\nHello\n"));
    }

    #[test]
    fn test_doc_string_allows_content_at_doc_indent_with_tabs() {
        let tokens = tokenize("\t@@doc \"\"\"\n\tabcde\n\t    5\n\t\"\"\"").unwrap();
        assert!(matches!(tokens[0].token, Token::Annotator(ref name) if name == "doc"));
        assert!(matches!(tokens[1].token, Token::DocString(ref s) if s == "\nabcde\n    5\n"));
    }

    #[test]
    fn test_doc_string_rejects_content_shallower_than_doc_indent() {
        let err = tokenize("\t@@doc \"\"\"\nabcde\n    5\n\t\"\"\"")
            .expect_err("expected doc indentation error");
        assert!(
            err.message()
                .contains("Triple-quoted string content must be indented at least as far as the starting line"),
            "unexpected error: {}",
            err.message()
        );
    }

    #[test]
    fn test_double_quote_string_escape_symmetry() {
        let tokens = tokenize(r#""a\n\t\"\'\\z""#).unwrap();
        assert!(matches!(
            tokens[0].token,
            Token::Str(ref s) if s == "a\n\t\"'\\z"
        ));
    }

    #[test]
    fn test_single_quote_string_escape_symmetry() {
        let tokens = tokenize(r#"'a\n\t\"\'\\z'"#).unwrap();
        assert!(matches!(
            tokens[0].token,
            Token::Str(ref s) if s == "a\n\t\"'\\z"
        ));
    }

    #[test]
    fn test_unit() {
        let tokens = tokenize("()").unwrap();
        assert!(matches!(tokens[0].token, Token::Unit));
    }

    #[test]
    fn test_two_char_ops() {
        let tokens = tokenize("++ =? == != <= >= && || => -> |> >> >* |*> |>= >=>").unwrap();
        assert!(matches!(tokens[0].token, Token::Concat));
        assert!(matches!(tokens[1].token, Token::SafeBind));
        assert!(matches!(tokens[2].token, Token::EqEq));
        assert!(matches!(tokens[3].token, Token::BangEq));
        assert!(matches!(tokens[4].token, Token::LtEq));
        assert!(matches!(tokens[5].token, Token::GtEq));
        assert!(matches!(tokens[6].token, Token::AndAnd));
        assert!(matches!(tokens[7].token, Token::OrOr));
        assert!(matches!(tokens[8].token, Token::FatArrow));
        assert!(matches!(tokens[9].token, Token::Arrow));
        assert!(matches!(tokens[10].token, Token::PipeApply));
        assert!(matches!(tokens[11].token, Token::Compose));
        assert!(matches!(tokens[12].token, Token::LiftCompose));
        assert!(matches!(tokens[13].token, Token::PipeMap));
        assert!(matches!(tokens[14].token, Token::PipeBind));
        assert!(matches!(tokens[15].token, Token::KleisliCompose));
    }

    #[test]
    fn test_dbg_bang_tokens() {
        let tokens = tokenize("dbg!(x)").unwrap();
        assert!(matches!(tokens[0].token, Token::Ident(ref s) if s == "dbg"));
        assert!(matches!(tokens[1].token, Token::Bang));
        assert!(matches!(tokens[2].token, Token::LParen));
        assert!(matches!(tokens[3].token, Token::Ident(ref s) if s == "x"));
        assert!(matches!(tokens[4].token, Token::RParen));
    }

    #[test]
    fn test_zero_arg_closure_double_pipe_is_not_oror() {
        let tokens = tokenize("{|| 1}").unwrap();
        assert!(matches!(tokens[0].token, Token::LBrace));
        assert!(matches!(tokens[1].token, Token::Pipe));
        assert!(matches!(tokens[2].token, Token::Pipe));
        assert!(matches!(tokens[3].token, Token::Int(_)));
        assert!(matches!(tokens[4].token, Token::RBrace));
    }

    #[test]
    fn test_def_keyword() {
        let tokens = tokenize("def noop() {()}").unwrap();
        assert!(matches!(tokens[0].token, Token::Def));
    }

    #[test]
    fn test_defmod_keyword() {
        let tokens = tokenize("defmod Kernel { def add() -> Unit { () } }").unwrap();
        assert!(matches!(tokens[0].token, Token::Defmod));
    }

    #[test]
    fn test_defenum_keyword() {
        let tokens = tokenize("defenum Color { Red, Green }").unwrap();
        assert!(matches!(tokens[0].token, Token::Defenum));
    }

    #[test]
    fn test_impl_keyword() {
        let tokens = tokenize("impl User { def new(self) -> Self { self } }").unwrap();
        assert!(matches!(tokens[0].token, Token::Impl));
    }

    #[test]
    fn test_import_keyword() {
        let tokens = tokenize("import Kernel::add").unwrap();
        assert!(matches!(tokens[0].token, Token::Import));
    }

    #[test]
    fn test_include_keyword() {
        let tokens = tokenize("include './mylib.srt'").unwrap();
        assert!(matches!(tokens[0].token, Token::Include));
    }

    #[test]
    fn test_type_keyword() {
        let tokens = tokenize("type Int").unwrap();
        assert!(matches!(tokens[0].token, Token::Type));
    }

    #[test]
    fn test_const_keywords() {
        let tokens = tokenize("public const APP_NAME = \"surtr\"").unwrap();
        assert!(matches!(tokens[0].token, Token::Public));
        assert!(matches!(tokens[1].token, Token::Const));
    }

    #[test]
    fn test_cond_keyword() {
        let tokens = tokenize("cond { True => 1 }").unwrap();
        assert!(matches!(tokens[0].token, Token::Cond));
    }

    #[test]
    fn test_when_keyword() {
        let tokens = tokenize("when").unwrap();
        assert!(matches!(tokens[0].token, Token::When));
    }

    #[test]
    fn test_at_builtin_annotator_token() {
        let tokens = tokenize("@@builtin def print(a: String) -> Unit").unwrap();
        assert!(matches!(tokens[0].token, Token::Annotator(ref name) if name == "builtin"));
    }

    #[test]
    fn test_custom_annotator_token() {
        let tokens = tokenize("@@memo def f()").unwrap();
        assert!(matches!(tokens[0].token, Token::Annotator(ref name) if name == "memo"));
    }

    #[test]
    fn test_invalid_empty_annotator_name() {
        let err = tokenize("@@ def f()").expect_err("expected lexer error");
        assert!(err.message().contains("Expected annotator name after '@@'"));
    }

    #[test]
    fn test_single_at_token() {
        let tokens = tokenize("@x").unwrap();
        assert!(matches!(tokens[0].token, Token::At));
        assert!(matches!(tokens[1].token, Token::Ident(ref s) if s == "x"));
    }

    #[test]
    fn test_dollar_token() {
        let tokens = tokenize("$A").unwrap();
        assert!(matches!(tokens[0].token, Token::Dollar));
        assert!(matches!(tokens[1].token, Token::Ident(ref s) if s == "A"));
    }

    #[test]
    fn test_func_literal_tokens() {
        let tokens = tokenize("a `eq` b `+` c `<=` d `User::cmp` e").unwrap();
        assert!(matches!(tokens[1].token, Token::FuncLiteral(ref body) if body == "eq"));
        assert!(matches!(tokens[3].token, Token::FuncLiteral(ref body) if body == "+"));
        assert!(matches!(tokens[5].token, Token::FuncLiteral(ref body) if body == "<="));
        assert!(matches!(tokens[7].token, Token::FuncLiteral(ref body) if body == "User::cmp"));
    }

    #[test]
    fn test_empty_func_literal_is_error() {
        let err = tokenize("``").expect_err("expected lexer error");
        assert!(err.message().contains("FuncLiteral body must not be empty"));
    }

    #[test]
    fn test_unclosed_func_literal_is_error() {
        let err = tokenize("`eq").expect_err("expected lexer error");
        assert!(err.message().contains("Incomplete"));
    }

    #[test]
    fn test_unsupported_func_literal_body_is_error() {
        let err = tokenize("`User::1`").expect_err("expected lexer error");
        assert!(err.message().contains("Unsupported FuncLiteral body"));
    }
}
