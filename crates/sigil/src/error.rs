use spire::ast::Span;

#[derive(Debug, Clone)]
pub struct ResolveErrorLabel {
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ResolveError {
    pub message: String,
    pub span: Span,
    pub related_labels: Vec<ResolveErrorLabel>,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ResolveError at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}

impl std::error::Error for ResolveError {}
