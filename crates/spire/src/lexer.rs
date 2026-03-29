use crate::ast::Span;
use crate::error::ParseError;
use crate::token::{Spanned, Token};

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
                        '\\' => s.push('\\'),
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
                return Err(ParseError::incomplete("'", Span { start, end: i }));
            }
            i += 1;
            tokens.push(Spanned {
                token: Token::Str(s),
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
                let text: String = chars[start..i].iter().collect();
                let val: i64 = text.parse().map_err(|_| {
                    ParseError::syntax(
                        format!("Invalid integer: {}", text),
                        Span { start, end: i },
                    )
                })?;
                tokens.push(Spanned {
                    token: Token::Int(val),
                    span: Span { start, end: i },
                });
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
                "defstruct" => Token::Defstruct,
                "defrecord" => Token::Defrecord,
                "deferror" => Token::Deferror,
                "match" => Token::Match,
                _ => Token::Ident(text),
            };
            tokens.push(Spanned {
                token,
                span: Span { start, end: i },
            });
            continue;
        }

        // Two-character operators
        if i + 1 < len {
            let two: String = chars[i..i + 2].iter().collect();
            let tok = match two.as_str() {
                "++" => Some(Token::Concat),
                "=?" => Some(Token::SafeBind),
                "==" => Some(Token::EqEq),
                "!=" => Some(Token::BangEq),
                "<=" => Some(Token::LtEq),
                ">=" => Some(Token::GtEq),
                "=>" => Some(Token::FatArrow),
                "->" => Some(Token::Arrow),
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
            '/' => Token::Slash,
            '%' => Token::Percent,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let tokens = tokenize("num = 10").unwrap();
        assert!(matches!(tokens[0].token, Token::Ident(ref s) if s == "num"));
        assert!(matches!(tokens[1].token, Token::Bind));
        assert!(matches!(tokens[2].token, Token::Int(10)));
    }

    #[test]
    fn test_float() {
        let tokens = tokenize("3.14").unwrap();
        assert!(matches!(tokens[0].token, Token::Float(f) if (f - 3.14).abs() < 1e-10));
    }

    #[test]
    fn test_string_escape() {
        let tokens = tokenize(r#""hello\nworld""#).unwrap();
        assert!(matches!(tokens[0].token, Token::Str(ref s) if s == "hello\nworld"));
    }

    #[test]
    fn test_unit() {
        let tokens = tokenize("()").unwrap();
        assert!(matches!(tokens[0].token, Token::Unit));
    }

    #[test]
    fn test_two_char_ops() {
        let tokens = tokenize("++ =? == != <= >= => ->").unwrap();
        assert!(matches!(tokens[0].token, Token::Concat));
        assert!(matches!(tokens[1].token, Token::SafeBind));
        assert!(matches!(tokens[2].token, Token::EqEq));
        assert!(matches!(tokens[3].token, Token::BangEq));
        assert!(matches!(tokens[4].token, Token::LtEq));
        assert!(matches!(tokens[5].token, Token::GtEq));
        assert!(matches!(tokens[6].token, Token::FatArrow));
        assert!(matches!(tokens[7].token, Token::Arrow));
    }

    #[test]
    fn test_def_keyword() {
        let tokens = tokenize("def noop() {()}").unwrap();
        assert!(matches!(tokens[0].token, Token::Def));
    }
}
