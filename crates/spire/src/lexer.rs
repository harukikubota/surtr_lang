use crate::ast::Span;
use crate::error::ParseError;
use crate::token::{Spanned, Token};
use sindr::primitives::SurtrInt;

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
                let val: SurtrInt = text.parse().map_err(|_| {
                    ParseError::syntax(format!("Invalid integer: {}", text), Span { start, end: i })
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
                "defmod" => Token::Defmod,
                "import" => Token::Import,
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
                ".." => Some(Token::DotDot),
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
        let tokens = tokenize("3.14").unwrap();
        assert!(matches!(tokens[0].token, Token::Float(f) if (f - 3.14).abs() < 1e-10));
    }

    #[test]
    fn test_string_escape() {
        let tokens = tokenize(r#""hello\nworld""#).unwrap();
        assert!(matches!(tokens[0].token, Token::Str(ref s) if s == "hello\nworld"));
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

    #[test]
    fn test_defmod_keyword() {
        let tokens = tokenize("defmod Kernel { def add() -> Unit { () } }").unwrap();
        assert!(matches!(tokens[0].token, Token::Defmod));
    }

    #[test]
    fn test_import_keyword() {
        let tokens = tokenize("import Kernel::add").unwrap();
        assert!(matches!(tokens[0].token, Token::Import));
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
}
