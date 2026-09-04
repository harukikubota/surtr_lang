use diagnostics::StructuredDiagnostic;
use spire::ast::Span;

#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
    pub hint: Option<String>,
    /// Structured facts are populated incrementally by migrated checker
    /// families. `None` is the explicit legacy-message boundary.
    pub structured: Option<StructuredDiagnostic>,
}

impl TypeError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            hint: None,
            structured: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_structured(mut self, diagnostic: StructuredDiagnostic) -> Self {
        self.structured = Some(diagnostic);
        self
    }
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TypeError at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )?;
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {}", hint)?;
        }
        Ok(())
    }
}

impl std::error::Error for TypeError {}
