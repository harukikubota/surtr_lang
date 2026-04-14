use crate::ast::Span;
use crate::token::{Spanned, Token};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum SyntaxToken {
    PathSep,
    Gt,
    Token(Token),
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn adapt_tokens(tokens: &[Spanned<Token>]) -> Vec<Spanned<SyntaxToken>> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0usize;

    while i < tokens.len() {
        let current = &tokens[i];
        if matches!(current.token, Token::Colon)
            && matches!(tokens.get(i + 1).map(|sp| &sp.token), Some(Token::Colon))
        {
            let next = &tokens[i + 1];
            out.push(Spanned {
                token: SyntaxToken::PathSep,
                span: Span {
                    start: current.span.start,
                    end: next.span.end,
                },
            });
            i += 2;
            continue;
        }

        if matches!(current.token, Token::Compose) {
            let left = Span {
                start: current.span.start,
                end: current.span.start.saturating_add(1),
            };
            let right = Span {
                start: left.end,
                end: current.span.end,
            };
            out.push(Spanned {
                token: SyntaxToken::Gt,
                span: left,
            });
            out.push(Spanned {
                token: SyntaxToken::Gt,
                span: right,
            });
            i += 1;
            continue;
        }

        out.push(Spanned {
            token: SyntaxToken::Token(current.token.clone()),
            span: current.span.clone(),
        });
        i += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapt_merges_double_colon_into_path_separator() {
        let tokens = vec![
            Spanned {
                token: Token::Colon,
                span: Span { start: 3, end: 4 },
            },
            Spanned {
                token: Token::Colon,
                span: Span { start: 4, end: 5 },
            },
        ];

        let adapted = adapt_tokens(&tokens);
        assert_eq!(adapted.len(), 1);
        assert_eq!(adapted[0].token, SyntaxToken::PathSep);
        assert_eq!(adapted[0].span, Span { start: 3, end: 5 });
    }

    #[test]
    fn adapt_splits_compose_into_two_gt_tokens() {
        let tokens = vec![Spanned {
            token: Token::Compose,
            span: Span { start: 10, end: 12 },
        }];

        let adapted = adapt_tokens(&tokens);
        assert_eq!(adapted.len(), 2);
        assert_eq!(adapted[0].token, SyntaxToken::Gt);
        assert_eq!(adapted[1].token, SyntaxToken::Gt);
        assert_eq!(adapted[0].span, Span { start: 10, end: 11 });
        assert_eq!(adapted[1].span, Span { start: 11, end: 12 });
    }
}
