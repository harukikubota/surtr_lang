use super::output::{ReplOutput, ReplResult};
use super::styled;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentedResultKind {
    EvalSuccess,
    EvalError,
    PlainText,
    Diagnostic,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedResult {
    pub stdout_lines: Vec<String>,
    pub lines: Vec<String>,
    pub stderr_lines: Vec<String>,
    pub kind: PresentedResultKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedDoc {
    pub symbol: String,
    pub signature: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentedEvent {
    None,
    Result(PresentedResult),
    Doc(PresentedDoc),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedInteraction {
    pub event: PresentedEvent,
    pub should_exit: bool,
}

pub fn present_for_cli(result: &ReplResult, color: bool) -> Vec<String> {
    let mut output = result.stdout.clone();
    let rendered = match &result.output {
        ReplOutput::EvalSuccess { rendered, .. } => rendered
            .iter()
            .map(|line| {
                if color {
                    styled::repl_result_line(line)
                } else {
                    line.clone()
                }
            })
            .collect(),
        ReplOutput::PlainText { lines } => lines.clone(),
        ReplOutput::StyledDoc { lines } => {
            if color {
                lines.iter().map(|line| styled::info_line(line)).collect()
            } else {
                lines.clone()
            }
        }
        ReplOutput::Diagnostic { .. } => Vec::new(),
        ReplOutput::DocResolved {
            symbol,
            signature,
            summary,
            source_snippet,
            details,
        } => {
            let mut lines = Vec::new();
            if let Some(sig) = signature {
                let banner = if color {
                    styled::doc_signature_banner(symbol, sig)
                } else {
                    styled::plain_doc_signature_banner(symbol, sig)
                };
                lines.push(banner);
            } else if color {
                lines.push(styled::doc_symbol(symbol));
            } else {
                lines.push(symbol.clone());
            }

            if let Some(text) = source_snippet.as_ref().or(summary.as_ref()) {
                let body_lines = if color {
                    styled::doc_body_lines(text)
                } else {
                    styled::plain_doc_body_lines(text)
                };
                lines.extend(body_lines);
            }
            if color {
                lines.extend(details.iter().map(|line| styled::doc_detail_line(line)));
            } else {
                lines.extend(details.iter().cloned());
            }
            lines
        }
        ReplOutput::EvalError { .. }
        | ReplOutput::StatusMessage(_)
        | ReplOutput::EvalStarted { .. } => Vec::new(),
    };
    output.extend(rendered);
    output
}

pub fn present_for_interaction(result: ReplResult) -> PresentedInteraction {
    let should_exit = result.should_exit;
    let stdout = result.stdout;
    let stderr = result.stderr;
    let event = match result.output {
        ReplOutput::EvalSuccess { rendered, .. } => PresentedEvent::Result(PresentedResult {
            stdout_lines: stdout,
            lines: rendered,
            stderr_lines: stderr,
            kind: PresentedResultKind::EvalSuccess,
        }),
        ReplOutput::EvalError { rendered, .. } => PresentedEvent::Result(PresentedResult {
            stdout_lines: stdout,
            lines: rendered,
            stderr_lines: stderr,
            kind: PresentedResultKind::EvalError,
        }),
        ReplOutput::PlainText { lines } => PresentedEvent::Result(PresentedResult {
            stdout_lines: stdout,
            lines,
            stderr_lines: stderr,
            kind: PresentedResultKind::PlainText,
        }),
        ReplOutput::StyledDoc { lines } => PresentedEvent::Result(PresentedResult {
            stdout_lines: stdout,
            lines,
            stderr_lines: stderr,
            kind: PresentedResultKind::Info,
        }),
        ReplOutput::Diagnostic {
            mut rendered,
            summary_tail,
        } => {
            rendered.extend(summary_tail);
            PresentedEvent::Result(PresentedResult {
                stdout_lines: stdout,
                lines: rendered,
                stderr_lines: stderr,
                kind: PresentedResultKind::Diagnostic,
            })
        }
        ReplOutput::DocResolved {
            symbol,
            signature,
            summary,
            source_snippet,
            details,
        } => PresentedEvent::Doc(PresentedDoc {
            symbol,
            signature,
            body: Some(
                [
                    source_snippet.or(summary),
                    (!details.is_empty()).then(|| details.join("\n")),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n"),
            )
            .filter(|body| !body.is_empty()),
        }),
        ReplOutput::StatusMessage(message) => {
            if should_exit {
                PresentedEvent::None
            } else {
                PresentedEvent::Result(PresentedResult {
                    stdout_lines: stdout,
                    lines: vec![message],
                    stderr_lines: stderr,
                    kind: PresentedResultKind::Info,
                })
            }
        }
        ReplOutput::EvalStarted { .. } => PresentedEvent::None,
    };

    PresentedInteraction { event, should_exit }
}
