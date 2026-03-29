use crate::ast::Span;

#[derive(Debug, Clone)]
pub enum ParseError {
    Incomplete { expected: String, span: Span },
    SyntaxError { message: String, span: Span },
}

impl ParseError {
    pub fn incomplete(expected: impl Into<String>, span: Span) -> Self {
        Self::Incomplete {
            expected: expected.into(),
            span,
        }
    }

    pub fn syntax(message: impl Into<String>, span: Span) -> Self {
        Self::SyntaxError {
            message: message.into(),
            span,
        }
    }

    pub fn span(&self) -> &Span {
        match self {
            ParseError::Incomplete { span, .. } | ParseError::SyntaxError { span, .. } => span,
        }
    }

    pub fn message(&self) -> String {
        match self {
            ParseError::Incomplete { expected, .. } => {
                format!("Incomplete input: expected {}", expected)
            }
            ParseError::SyntaxError { message, .. } => message.clone(),
        }
    }

    pub fn is_incomplete(&self) -> bool {
        matches!(self, ParseError::Incomplete { .. })
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let span = self.span();
        write!(
            f,
            "ParseError at {}..{}: {}",
            span.start,
            span.end,
            self.message()
        )
    }
}

impl std::error::Error for ParseError {}
