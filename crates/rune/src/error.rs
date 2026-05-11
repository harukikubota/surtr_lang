use diagnostics::{DiagnosticSpec, SourceId, SourceRegistry};
#[cfg(test)]
use forge::bytecode::line_column_for_offset;
use sindr::policy::CompileUnitKind;

pub(crate) type RuneResult<T> = Result<T, RuneError>;

pub(crate) const USAGE_TEXT: &str = "\
Usage:\n\
  surtr --version\n\
  surtr check <file.srt> [--format json]\n\
  surtr run <file.srt|file.eldr> [--entry <name>] [--vm-dump <path>] [--vm-dump-on error|always] [--vm-stats] [--vm-stats-json] [--trace-opcode] [--trace-call] [--trace-limit <n>] [--trace-filter <csv>] [--phase-times] [--error-context verbose] [-- <arg>...]\n\
  surtr test [--quiet|-q] <lib-relative-name|--all>\n\
  surtr repl [--quiet] [--banner] [--version] [--module <file.srt>] [--script <file.srt>]\n\
  surtr build <file.srt> [output.eldr]\n\
  surtr dump <file.eldr|entry.srt> [--format json] [--entry <name>] [--opcode-histogram] [--peephole-candidates]\n\
  surtr tui [file.eldr]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionEnv {
    Check,
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
            Self::Check => "check",
            Self::Run => "run",
            Self::Build => "build",
            Self::Test => "test",
            Self::DumpSource | Self::DumpBytecode => "dump",
            Self::Repl => "repl",
            Self::Tui => "tui",
        }
    }

    pub(crate) fn compile_unit_kind(self) -> CompileUnitKind {
        match self {
            Self::Repl => CompileUnitKind::Repl,
            _ => CompileUnitKind::Script,
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
pub(crate) struct RuneDiagnostic {
    pub(crate) sources: SourceRegistry,
    pub(crate) source_id: SourceId,
    pub(crate) phase: String,
    pub(crate) spec: DiagnosticSpec,
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
        diagnostic: Box<RuneDiagnostic>,
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

    pub(crate) fn from_xldr_command_error(error: xldr::CommandError) -> Self {
        match error {
            xldr::CommandError::Usage { message } => Self::usage(message),
            xldr::CommandError::Message { exit_code, message } => Self::message(exit_code, message),
            xldr::CommandError::Diagnostic {
                exit_code,
                diagnostic,
            } => Self::diagnostic(
                exit_code,
                &diagnostic.sources,
                diagnostic.source_id,
                diagnostic.phase,
                diagnostic.spec,
            ),
        }
    }

    pub(crate) fn diagnostic(
        exit_code: i32,
        sources: &SourceRegistry,
        source_id: SourceId,
        phase: impl Into<String>,
        spec: DiagnosticSpec,
    ) -> Self {
        Self::Diagnostic {
            exit_code,
            diagnostic: Box::new(RuneDiagnostic {
                sources: sources.clone(),
                source_id,
                phase: phase.into(),
                spec,
            }),
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
            Self::Diagnostic { diagnostic, .. } => diagnostics::report_error_by_id(
                &diagnostic.sources,
                diagnostic.source_id,
                diagnostic.spec.clone(),
            ),
        }
    }

    pub(crate) fn to_serializable_report(&self) -> diagnostics::SerializableDiagnosticReport {
        match self {
            Self::Diagnostic { diagnostic, .. } => diagnostics::serializable_report_by_id(
                &diagnostic.sources,
                diagnostic.source_id,
                diagnostic.phase.clone(),
                &diagnostic.spec,
            ),
            Self::Usage { message } => diagnostics::SerializableDiagnosticReport {
                errors: vec![diagnostics::SerializableDiagnostic {
                    kind: "UsageError".to_string(),
                    phase: "cli".to_string(),
                    line: 1,
                    column: 1,
                    span: [0, 0],
                    message: if message.is_empty() {
                        "usage error".to_string()
                    } else {
                        message.clone()
                    },
                    expected: None,
                    got: None,
                    hint: Some(USAGE_TEXT.to_string()),
                }],
            },
            Self::Message { message, .. } => diagnostics::SerializableDiagnosticReport {
                errors: vec![diagnostics::SerializableDiagnostic {
                    kind: "CommandError".to_string(),
                    phase: "cli".to_string(),
                    line: 1,
                    column: 1,
                    span: [0, 0],
                    message: if message.is_empty() {
                        "command failed".to_string()
                    } else {
                        message.clone()
                    },
                    expected: None,
                    got: None,
                    hint: None,
                }],
            },
        }
    }

    #[cfg(test)]
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
            Self::Diagnostic { diagnostic, .. } => {
                let file_name = diagnostic
                    .sources
                    .file_name(diagnostic.source_id)
                    .unwrap_or("<unknown>");
                let source = diagnostic
                    .sources
                    .source(diagnostic.source_id)
                    .unwrap_or("");
                let (line, column) =
                    line_column_for_offset(source, diagnostic.spec.primary_span.start);
                format!(
                    "{} at {}:{}:{}: {}",
                    diagnostic.spec.kind, file_name, line, column, diagnostic.spec.message
                )
            }
        }
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::RuneError;

    #[test]
    fn xldr_message_error_maps_to_rune_message() {
        let err = RuneError::from_xldr_command_error(xldr::CommandError::message(
            7,
            "tui: terminal init failed",
        ));

        assert_eq!(err.exit_code(), 7);
        assert_eq!(err.summary(), "tui: terminal init failed");
    }

    #[test]
    fn xldr_diagnostic_error_maps_to_rune_diagnostic() {
        let mut sources = diagnostics::SourceRegistry::new();
        let source_id = sources.register("bad.srt", "defmod Broken { }\n");
        let err = RuneError::from_xldr_command_error(xldr::CommandError::diagnostic(
            1,
            &sources,
            source_id,
            "parse",
            diagnostics::simple_error(
                "ParseError",
                "This top-level declaration is not allowed in script source",
                spire::ast::Span { start: 0, end: 6 },
                None,
            ),
        ));

        assert_eq!(err.exit_code(), 1);
        assert!(err.summary().contains(
            "ParseError at bad.srt:1:1: This top-level declaration is not allowed in script source"
        ));
    }
}
