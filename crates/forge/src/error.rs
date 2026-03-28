use spire::ast::Span;

#[derive(Debug, Clone)]
pub struct CodegenError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CodegenError at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}

impl std::error::Error for CodegenError {}
