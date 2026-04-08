/// UI-agnostic output event returned by the logic layer for one REPL input.
pub enum ReplOutput {
    EvalStarted {
        idx: usize,
        source: String,
    },
    EvalSuccess {
        idx: usize,
        source: String,
        rendered: Vec<String>,
    },
    EvalError {
        idx: usize,
        source: String,
        rendered: Vec<String>,
    },
    CommandOutput {
        rendered: Vec<String>,
    },
    DocResolved {
        symbol: String,
        signature: Option<String>,
        summary: Option<String>,
        source_snippet: Option<String>,
    },
    StatusMessage(String),
}

/// Wraps `ReplOutput` with a session-exit signal.
///
/// `should_exit` is set to `true` only on runtime trap (E-1 contract).
pub struct ReplResult {
    pub output: ReplOutput,
    pub should_exit: bool,
}

impl ReplResult {
    pub fn ok(output: ReplOutput) -> Self {
        Self {
            output,
            should_exit: false,
        }
    }

    pub fn exit(output: ReplOutput) -> Self {
        Self {
            output,
            should_exit: true,
        }
    }
}
