use crate::ast::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorReason {
    IncompleteInput,
    UnexpectedToken,
    DeclarationSyntax,
    ExpressionSyntax,
    StatementSyntax,
    PatternSyntax,
    TypeSyntax,
    LiteralSyntax,
    PositionRule,
    SourcePolicy,
    InterpolationSyntax,
    ReturnTypeArgumentArityMismatch,
    InvalidDoCarrierReturnTypeArgument,
    CompilerInvariant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorGuidance {
    UnexpectedToken,
    TopLevelDeclaration,
    TopLevelExpression,
    UnitPattern,
    AsPatternAlias,
    RangeLiteral,
    OperatorCapture(String),
    PairConstructorCapture,
    ReturnPositionImplTrait,
    WhereClause,
    MissingMetaState,
    MissingMetaInstance,
    AnonymousCaptureIdentity,
    AnonymousCaptureRequiresHelper,
    ImmediateAnonymousCall,
    DoCarrierReturnTypeArgument,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Incomplete {
        expected: String,
        span: Span,
        expected_tokens: Vec<String>,
        cursor_span: Span,
        guidance: Option<ParseErrorGuidance>,
        token_kind: Option<String>,
    },
    SyntaxError {
        message: String,
        span: Span,
        reason: ParseErrorReason,
        expected_tokens: Vec<String>,
        cursor_span: Span,
        guidance: Option<ParseErrorGuidance>,
        token_kind: Option<String>,
    },
}

impl ParseError {
    pub fn incomplete(expected: impl Into<String>, span: Span) -> Self {
        let expected = expected.into();
        Self::Incomplete {
            expected_tokens: vec![expected.clone()],
            cursor_span: span.clone(),
            expected,
            span,
            guidance: None,
            token_kind: None,
        }
    }

    pub fn syntax(reason: ParseErrorReason, message: impl Into<String>, span: Span) -> Self {
        Self::SyntaxError {
            message: message.into(),
            cursor_span: span.clone(),
            span,
            reason,
            expected_tokens: Vec::new(),
            guidance: None,
            token_kind: None,
        }
    }

    pub fn with_guidance(mut self, guidance: ParseErrorGuidance) -> Self {
        match &mut self {
            Self::Incomplete { guidance: slot, .. } | Self::SyntaxError { guidance: slot, .. } => {
                *slot = Some(guidance);
            }
        }
        self
    }

    pub fn with_token_kind(mut self, token_kind: impl Into<String>) -> Self {
        match &mut self {
            Self::Incomplete {
                token_kind: slot, ..
            }
            | Self::SyntaxError {
                token_kind: slot, ..
            } => *slot = Some(token_kind.into()),
        }
        self
    }

    pub fn with_parser_context(mut self, expected_tokens: Vec<String>, cursor_span: Span) -> Self {
        match &mut self {
            Self::Incomplete {
                expected_tokens: stored,
                cursor_span: cursor,
                ..
            }
            | Self::SyntaxError {
                expected_tokens: stored,
                cursor_span: cursor,
                ..
            } => {
                if !expected_tokens.is_empty() {
                    *stored = expected_tokens;
                }
                *cursor = cursor_span;
            }
        }
        self
    }

    pub fn with_span(mut self, span: Span) -> Self {
        match &mut self {
            Self::Incomplete {
                span: error_span,
                cursor_span,
                ..
            }
            | Self::SyntaxError {
                span: error_span,
                cursor_span,
                ..
            } => {
                *error_span = span.clone();
                *cursor_span = span;
            }
        }
        self
    }

    pub fn reason(&self) -> ParseErrorReason {
        match self {
            Self::Incomplete { .. } => ParseErrorReason::IncompleteInput,
            Self::SyntaxError { reason, .. } => *reason,
        }
    }

    pub fn expected_tokens(&self) -> &[String] {
        match self {
            Self::Incomplete {
                expected_tokens, ..
            }
            | Self::SyntaxError {
                expected_tokens, ..
            } => expected_tokens,
        }
    }

    pub fn cursor_span(&self) -> &Span {
        match self {
            Self::Incomplete { cursor_span, .. } | Self::SyntaxError { cursor_span, .. } => {
                cursor_span
            }
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Incomplete { expected, .. } => expected,
            Self::SyntaxError { message, .. } => message,
        }
    }

    pub fn guidance(&self) -> Option<&ParseErrorGuidance> {
        match self {
            Self::Incomplete { guidance, .. } | Self::SyntaxError { guidance, .. } => {
                guidance.as_ref()
            }
        }
    }

    pub fn token_kind(&self) -> Option<&str> {
        match self {
            Self::Incomplete { token_kind, .. } | Self::SyntaxError { token_kind, .. } => {
                token_kind.as_deref()
            }
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
