use diagnostics::{DiagnosticSpec, SourceId, SourceRegistry};

use crate::{EldrLoadError, ReplLoadError};

pub type CommandResult<T> = Result<T, CommandError>;

#[derive(Debug, Clone)]
pub struct CommandDiagnostic {
    pub phase: String,
    pub sources: SourceRegistry,
    pub source_id: SourceId,
    pub spec: DiagnosticSpec,
}

#[derive(Debug, Clone)]
pub enum CommandError {
    Usage {
        message: String,
    },
    Message {
        exit_code: i32,
        message: String,
    },
    Diagnostic {
        exit_code: i32,
        diagnostic: Box<CommandDiagnostic>,
    },
}

impl CommandError {
    pub fn usage(message: impl Into<String>) -> Self {
        Self::Usage {
            message: message.into(),
        }
    }

    pub fn message(exit_code: i32, message: impl Into<String>) -> Self {
        Self::Message {
            exit_code,
            message: message.into(),
        }
    }

    pub fn diagnostic(
        exit_code: i32,
        sources: &SourceRegistry,
        source_id: SourceId,
        phase: impl Into<String>,
        spec: DiagnosticSpec,
    ) -> Self {
        Self::Diagnostic {
            exit_code,
            diagnostic: Box::new(CommandDiagnostic {
                phase: phase.into(),
                sources: sources.clone(),
                source_id,
                spec,
            }),
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage { .. } => 1,
            Self::Message { exit_code, .. } | Self::Diagnostic { exit_code, .. } => *exit_code,
        }
    }
}

impl From<ReplLoadError> for CommandError {
    fn from(value: ReplLoadError) -> Self {
        match value {
            ReplLoadError::SourceReadFailed { file_name, message } => {
                Self::message(1, format!("repl: cannot read {}: {}", file_name, message))
            }
            ReplLoadError::Diagnostic {
                phase,
                sources,
                source_id,
                spec,
            } => Self::diagnostic(1, &sources, source_id, phase, spec),
            ReplLoadError::Load(error) => Self::message(1, format!("repl: {}", error)),
            ReplLoadError::Runtime { file_name, message } => Self::message(
                1,
                format!(
                    "repl: runtime error while preloading {}: {}",
                    file_name, message
                ),
            ),
        }
    }
}

impl From<EldrLoadError> for CommandError {
    fn from(value: EldrLoadError) -> Self {
        Self::message(1, format!("tui: {}", value))
    }
}

#[cfg(test)]
mod tests {
    use super::CommandError;
    use crate::ReplLoadError;

    #[test]
    fn repl_load_diagnostic_maps_to_command_diagnostic() {
        let mut sources = diagnostics::SourceRegistry::new();
        let source_id = sources.register("bad.srt", "defmod Broken { }\n");
        let error = CommandError::from(ReplLoadError::Diagnostic {
            phase: "parse".to_string(),
            sources: sources.clone(),
            source_id,
            spec: diagnostics::simple_error(
                "ParseError",
                "This top-level declaration is not allowed in script source",
                spire::ast::Span { start: 0, end: 6 },
                None,
            ),
        });

        assert_eq!(error.exit_code(), 1);
        match error {
            CommandError::Diagnostic { diagnostic, .. } => {
                assert_eq!(diagnostic.phase, "parse");
                assert_eq!(diagnostic.source_id, source_id);
                assert_eq!(diagnostic.spec.kind, "ParseError");
            }
            other => panic!("expected diagnostic error, got {other:?}"),
        }
    }
}
