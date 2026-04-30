use super::output::{ReplOutput, ReplResult};
use super::styled;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentedResultKind {
    EvalSuccess,
    EvalError,
    CommandOutput,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedResult {
    pub lines: Vec<String>,
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
    match &result.output {
        ReplOutput::EvalSuccess { rendered, .. } => rendered
            .iter()
            .map(|line| {
                let rendered = if color {
                    styled::repl_result_line(line)
                } else {
                    line.clone()
                };
                format!("> {rendered}")
            })
            .collect(),
        ReplOutput::CommandOutput { rendered } => {
            rendered.iter().map(|line| format!("> {line}")).collect()
        }
        ReplOutput::SigResolved { signature } => {
            let rendered = if color {
                styled::signature(signature)
            } else {
                signature.clone()
            };
            vec![rendered]
        }
        ReplOutput::DocResolved {
            symbol,
            signature,
            summary,
            source_snippet,
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
            lines
        }
        ReplOutput::EvalError { .. }
        | ReplOutput::StatusMessage(_)
        | ReplOutput::EvalStarted { .. } => Vec::new(),
    }
}

pub fn present_for_interaction(result: ReplResult) -> PresentedInteraction {
    let event = match result.output {
        ReplOutput::EvalSuccess { rendered, .. } => PresentedEvent::Result(PresentedResult {
            lines: rendered,
            kind: PresentedResultKind::EvalSuccess,
        }),
        ReplOutput::EvalError { rendered, .. } => PresentedEvent::Result(PresentedResult {
            lines: rendered,
            kind: PresentedResultKind::EvalError,
        }),
        ReplOutput::CommandOutput { rendered } => PresentedEvent::Result(PresentedResult {
            lines: rendered,
            kind: PresentedResultKind::CommandOutput,
        }),
        ReplOutput::SigResolved { signature } => PresentedEvent::Result(PresentedResult {
            lines: vec![signature],
            kind: PresentedResultKind::Info,
        }),
        ReplOutput::DocResolved {
            symbol,
            signature,
            summary,
            source_snippet,
        } => PresentedEvent::Doc(PresentedDoc {
            symbol,
            signature,
            body: source_snippet.or(summary),
        }),
        ReplOutput::StatusMessage(message) => {
            if result.should_exit {
                PresentedEvent::None
            } else {
                PresentedEvent::Result(PresentedResult {
                    lines: vec![message],
                    kind: PresentedResultKind::Info,
                })
            }
        }
        ReplOutput::EvalStarted { .. } => PresentedEvent::None,
    };

    PresentedInteraction {
        event,
        should_exit: result.should_exit,
    }
}
