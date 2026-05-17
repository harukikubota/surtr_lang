use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningKind {
    UnusedVariable,
    UnusedValue,
    UnusedImportFunction,
    UnusedTypeParameter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningPhase {
    Resolve,
    Typecheck,
    Codegen,
    Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarningSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerWarning {
    pub kind: WarningKind,
    pub phase: WarningPhase,
    pub span: WarningSpan,
    pub message: String,
    pub hint: Option<String>,
}

impl CompilerWarning {
    pub fn new(
        kind: WarningKind,
        phase: WarningPhase,
        span: WarningSpan,
        message: impl Into<String>,
        hint: Option<String>,
    ) -> Self {
        Self {
            kind,
            phase,
            span,
            message: message.into(),
            hint,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarningBuffer {
    warnings: Vec<CompilerWarning>,
}

impl WarningBuffer {
    pub fn push(&mut self, warning: CompilerWarning) {
        self.warnings.push(warning);
    }

    pub fn extend<I>(&mut self, warnings: I)
    where
        I: IntoIterator<Item = CompilerWarning>,
    {
        self.warnings.extend(warnings);
    }

    pub fn take(&mut self) -> Vec<CompilerWarning> {
        std::mem::take(&mut self.warnings)
    }

    pub fn is_empty(&self) -> bool {
        self.warnings.is_empty()
    }

    pub fn as_slice(&self) -> &[CompilerWarning] {
        &self.warnings
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseOutput<T> {
    pub value: T,
    pub warnings: Vec<CompilerWarning>,
}

impl<T> PhaseOutput<T> {
    pub fn new(value: T, warnings: Vec<CompilerWarning>) -> Self {
        Self { value, warnings }
    }
}
