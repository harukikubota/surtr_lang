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
    PlainText {
        lines: Vec<String>,
    },
    StyledDoc {
        lines: Vec<String>,
    },
    Diagnostic {
        rendered: Vec<String>,
        summary_tail: Vec<String>,
    },
    DocResolved {
        symbol: String,
        signature: Option<String>,
        summary: Option<String>,
        source_snippet: Option<String>,
        details: Vec<String>,
    },
    StatusMessage(String),
}

/// Wraps `ReplOutput` with a session-exit signal.
///
/// `should_exit` is set to `true` only on runtime trap (E-1 contract).
pub struct ReplResult {
    pub output: ReplOutput,
    pub should_exit: bool,
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
}

impl ReplResult {
    pub fn ok(output: ReplOutput) -> Self {
        Self {
            output,
            should_exit: false,
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    pub fn exit(output: ReplOutput) -> Self {
        Self {
            output,
            should_exit: true,
            stdout: Vec::new(),
            stderr: Vec::new(),
        }
    }

    pub fn with_stdout(mut self, mut stdout: Vec<String>) -> Self {
        self.stdout.append(&mut stdout);
        self
    }

    pub fn with_stderr(mut self, mut stderr: Vec<String>) -> Self {
        self.stderr.append(&mut stderr);
        self
    }

    pub fn plain(lines: Vec<String>) -> Self {
        Self::ok(ReplOutput::PlainText { lines })
    }

    pub fn styled(lines: Vec<String>) -> Self {
        Self::ok(ReplOutput::StyledDoc { lines })
    }

    pub fn diagnostic(rendered: Vec<String>, summary_tail: Vec<String>) -> Self {
        Self::ok(ReplOutput::Diagnostic {
            rendered,
            summary_tail,
        })
    }
}
