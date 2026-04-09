use diagnostics::{DiagnosticSpec, SourceId, SourceRegistry};
use forge::bytecode::line_column_for_offset;

pub(crate) type RuneResult<T> = Result<T, RuneError>;

pub(crate) const USAGE_TEXT: &str = "\
Usage:\n\
  surtr --version\n\
  surtr run <file.srt|file.eldr> [--entry <name>]\n\
  surtr test [selector]\n\
  surtr repl [--quiet] [--banner] [--version]\n\
  surtr build <file.srt> [output.eldr]\n\
  surtr dump <file.eldr|entry.srt> [--format json] [--entry <name>]\n\
  surtr tui [file.eldr]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionEnv {
    Run,
    Build,
    Test,
    DumpSource,
    DumpBytecode,
    Repl,
    Tui,
}

impl ExecutionEnv {
    pub(crate) fn command_name(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Build => "build",
            Self::Test => "test",
            Self::DumpSource | Self::DumpBytecode => "dump",
            Self::Repl => "repl",
            Self::Tui => "tui",
        }
    }

    pub(crate) fn compile_unit_kind(self) -> spire::CompileUnitKind {
        match self {
            Self::Repl => spire::CompileUnitKind::Repl,
            _ => spire::CompileUnitKind::Script,
        }
    }

    pub(crate) fn source_kind(self) -> xldr::SourceKind {
        match self {
            Self::Repl => xldr::SourceKind::ReplChunk,
            _ => xldr::SourceKind::Script,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum RuneError {
    Usage {
        message: String,
    },
    Message {
        exit_code: i32,
        message: String,
    },
    Diagnostic {
        exit_code: i32,
        sources: SourceRegistry,
        source_id: SourceId,
        spec: DiagnosticSpec,
    },
}

impl RuneError {
    pub(crate) fn usage(message: impl Into<String>) -> Self {
        Self::Usage {
            message: message.into(),
        }
    }

    pub(crate) fn message(exit_code: i32, message: impl Into<String>) -> Self {
        Self::Message {
            exit_code,
            message: message.into(),
        }
    }

    pub(crate) fn silent(exit_code: i32) -> Self {
        Self::Message {
            exit_code,
            message: String::new(),
        }
    }

    pub(crate) fn diagnostic(
        exit_code: i32,
        sources: &SourceRegistry,
        source_id: SourceId,
        spec: DiagnosticSpec,
    ) -> Self {
        Self::Diagnostic {
            exit_code,
            sources: sources.clone(),
            source_id,
            spec,
        }
    }

    pub(crate) fn exit_code(&self) -> i32 {
        match self {
            Self::Usage { .. } => 1,
            Self::Message { exit_code, .. } | Self::Diagnostic { exit_code, .. } => *exit_code,
        }
    }

    pub(crate) fn emit(&self) {
        match self {
            Self::Usage { message } => {
                if !message.is_empty() {
                    eprintln!("{message}");
                }
                eprintln!("{USAGE_TEXT}");
            }
            Self::Message { message, .. } => {
                if !message.is_empty() {
                    eprintln!("{message}");
                }
            }
            Self::Diagnostic {
                sources,
                source_id,
                spec,
                ..
            } => diagnostics::report_error_by_id(sources, *source_id, spec.clone()),
        }
    }

    pub(crate) fn summary(&self) -> String {
        match self {
            Self::Usage { message } => {
                if message.is_empty() {
                    "usage error".to_string()
                } else {
                    message.clone()
                }
            }
            Self::Message { message, .. } => {
                if message.is_empty() {
                    "command failed".to_string()
                } else {
                    message.clone()
                }
            }
            Self::Diagnostic {
                sources,
                source_id,
                spec,
                ..
            } => {
                let file_name = sources.file_name(*source_id).unwrap_or("<unknown>");
                let source = sources.source(*source_id).unwrap_or("");
                let (line, column) = line_column_for_offset(source, spec.primary_span.start);
                format!(
                    "{} at {}:{}:{}: {}",
                    spec.kind, file_name, line, column, spec.message
                )
            }
        }
    }
}
