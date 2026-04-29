use crate::{Color, SourceId};
use serde::{Deserialize, Serialize};
use spire::ast::Span;

#[derive(Debug, Clone)]
pub struct DiagnosticSpec {
    pub kind: String,
    pub message: String,
    pub primary_span: Span,
    pub labels: Vec<DiagnosticLabel>,
    pub notes: Vec<String>,
    pub help: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DiagnosticLabel {
    pub source_id: Option<SourceId>,
    pub span: Span,
    pub message: String,
    pub color: Option<Color>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeDiagnosticContext {
    pub opcode: Option<String>,
    pub function: Option<String>,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializableDiagnostic {
    pub kind: String,
    pub phase: String,
    pub line: u32,
    pub column: u32,
    pub span: [u32; 2],
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub got: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SerializableDiagnosticReport {
    pub errors: Vec<SerializableDiagnostic>,
}

pub fn simple_error(
    kind: impl Into<String>,
    message: impl Into<String>,
    span: Span,
    help: Option<String>,
) -> DiagnosticSpec {
    DiagnosticSpec {
        kind: kind.into(),
        message: message.into(),
        primary_span: span,
        labels: Vec::new(),
        notes: Vec::new(),
        help,
    }
}
