use spire::ast::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveErrorReason {
    NameResolution,
    Namespace,
    Visibility,
    Import,
    Capture,
    Pattern,
    Declaration,
    SpecialForm,
    SourcePolicy,
    InvalidIntrinsicSurfaceContract,
    ReservedIntrinsicMarkerDeclaration,
    ReservedIntrinsicMarkerImpl,
    CompilerInvariant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveErrorDiagnostic {
    pub reason: ResolveErrorReason,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolveSourceProvenance {
    pub stage_index: usize,
    pub source_index: usize,
}

#[derive(Debug, Clone)]
pub struct ResolveErrorLabel {
    pub span: Span,
    pub message: String,
    pub source: Option<ResolveSourceProvenance>,
}

#[derive(Debug, Clone)]
pub struct ResolveError {
    pub message: String,
    pub span: Span,
    pub related_labels: Vec<ResolveErrorLabel>,
    pub diagnostic: ResolveErrorDiagnostic,
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
