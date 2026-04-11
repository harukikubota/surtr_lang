//! TUI key handling and event processing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::repl::logic::core::ReplEngine;
use crate::repl::logic::output::ReplOutput;

use super::app::{
    App, Completion, CompletionItem, FocusPane, InputBuffer, InputMode, ResultEntryKind,
};

// ── Completion helpers ────────────────────────────────────────────────────────

pub(super) fn current_token_prefix(buf: &InputBuffer) -> String {
    let before = &buf.text[..buf.cursor_byte];
    before
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '{' | '}' | ',' | ':'))
        .next_back()
        .unwrap_or("")
        .to_string()
}

pub(super) fn refresh_completion(app: &mut App, engine: &ReplEngine) {
    match app.input_mode {
        InputMode::Command => {
            let prefix = app.command.text.trim().to_string();
            let items = command_completions(&prefix, app.focus);
            app.completion = Completion {
                visible: !items.is_empty(),
                selected: 0,
                items,
            };
        }
        InputMode::Insert => {
            let prefix = current_token_prefix(&app.input);
            if prefix.is_empty() {
                app.completion.clear();
            } else {
                let symbols = engine.completion_symbols();
                let items: Vec<CompletionItem> = symbols
                    .iter()
                    .filter(|s| s.starts_with(&prefix))
                    .map(|s| CompletionItem {
                        label: s.clone(),
                        detail: None,
                    })
                    .collect();
                app.completion = Completion {
                    visible: !items.is_empty(),
                    selected: 0,
                    items,
                };
            }
        }
    }
}

static GLOBAL_COMMANDS: &[(&str, &str)] = &[
    ("q", ":q  — quit"),
    ("help", ":help  — show help"),
    ("doc", ":doc <symbol>  — show docs"),
    ("sig", ":sig <symbol>  — show signature"),
    ("type", ":type <expr>  — infer type"),
    ("save", ":save <path>  — save session to .eldr"),
];

static INPUT_COMMANDS: &[(&str, &str)] = &[("v", ":v <idx>  — recall result")];
static RESULTS_COMMANDS: &[(&str, &str)] = &[("j", ":j <idx>  — jump to result")];
static DOCS_COMMANDS: &[(&str, &str)] = &[
    ("doc-focus", ":doc-focus <idx>"),
    ("doc-drop", ":doc-drop <idx>"),
    ("doc-clear", ":doc-clear"),
];

fn command_completions(prefix: &str, focus: FocusPane) -> Vec<CompletionItem> {
    let pane_cmds = match focus {
        FocusPane::Input => INPUT_COMMANDS,
        FocusPane::Results => RESULTS_COMMANDS,
        FocusPane::Docs => DOCS_COMMANDS,
    };
    GLOBAL_COMMANDS
        .iter()
        .chain(pane_cmds.iter())
        .filter(|(name, _)| name.starts_with(prefix))
        .map(|(name, usage)| CompletionItem {
            label: name.to_string(),
            detail: Some(usage.to_string()),
        })
        .collect()
}

// ── Key handling ──────────────────────────────────────────────────────────────

pub(super) fn handle_key(app: &mut App, engine: &mut ReplEngine, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    match app.focus {
        FocusPane::Input => handle_input_pane(app, engine, key),
        FocusPane::Results => handle_results_pane(app, key),
        FocusPane::Docs => handle_docs_pane(app, key),
    }
}

fn handle_input_pane(app: &mut App, engine: &mut ReplEngine, key: KeyEvent) {
    match key.code {
        KeyCode::Tab => {
            if app.completion.visible {
                let mut buf = app.active_buf().clone();
                app.completion.apply(&mut buf);
                *app.active_buf_mut() = buf;
                app.completion.clear();
            } else {
                app.next_focus();
            }
        }
        KeyCode::BackTab => app.prev_focus(),
        KeyCode::Left => {
            app.active_buf_mut().move_left();
            refresh_completion(app, engine);
        }
        KeyCode::Right => {
            app.active_buf_mut().move_right();
            refresh_completion(app, engine);
        }
        KeyCode::Up => app.completion.select_prev(),
        KeyCode::Down => app.completion.select_next(),
        KeyCode::Backspace => {
            app.active_buf_mut().backspace();
            refresh_completion(app, engine);
        }
        KeyCode::Esc => {
            if app.input_mode == InputMode::Command {
                app.input_mode = InputMode::Insert;
                app.command.clear();
                app.completion.clear();
                app.update_status();
            }
        }
        KeyCode::Enter => match app.input_mode {
            InputMode::Insert => submit_input(app, engine),
            InputMode::Command => submit_command(app, engine),
        },
        KeyCode::Char(':') => {
            if app.input_mode == InputMode::Insert && app.input.cursor_byte == 0 {
                app.input_mode = InputMode::Command;
                app.command.clear();
                app.update_status();
                refresh_completion(app, engine);
            } else if app.input_mode == InputMode::Command {
                app.input_mode = InputMode::Insert;
                app.command.clear();
                app.completion.clear();
                app.update_status();
            } else {
                app.input.insert_char(':');
                refresh_completion(app, engine);
            }
        }
        KeyCode::Char(ch) => {
            app.active_buf_mut().insert_char(ch);
            refresh_completion(app, engine);
        }
        _ => {}
    }
}

fn handle_results_pane(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Tab => app.next_focus(),
        KeyCode::BackTab => app.prev_focus(),
        KeyCode::Up => app.results_scroll = app.results_scroll.saturating_sub(1),
        KeyCode::Down => {
            let max: usize = app.results.iter().map(|e| 3 + e.rendered_lines.len()).sum();
            app.results_scroll = (app.results_scroll + 1).min(max);
        }
        _ => {}
    }
}

fn handle_docs_pane(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Tab => app.next_focus(),
        KeyCode::BackTab => app.prev_focus(),
        KeyCode::Up => {
            app.selected_doc = Some(app.selected_doc.unwrap_or(0).saturating_sub(1));
        }
        KeyCode::Down => {
            let max = app.docs.len().saturating_sub(1);
            let next = app.selected_doc.unwrap_or(0).saturating_add(1).min(max);
            app.selected_doc = Some(next);
        }
        _ => {}
    }
}

// ── Submission ────────────────────────────────────────────────────────────────

pub(super) fn submit_input(app: &mut App, engine: &mut ReplEngine) {
    let source = app.input.text.trim().to_string();
    if source.is_empty() {
        return;
    }

    app.input.clear();
    app.completion.clear();

    let result = engine.handle_line(&source);
    match result.output {
        ReplOutput::EvalSuccess { rendered, .. } => {
            app.push_result(&source, rendered, ResultEntryKind::EvalSuccess);
        }
        ReplOutput::EvalError { rendered, .. } => {
            app.push_result(&source, rendered, ResultEntryKind::EvalError);
        }
        ReplOutput::CommandOutput { rendered } => {
            app.push_result(&source, rendered, ResultEntryKind::CommandOutput);
        }
        ReplOutput::DocResolved {
            symbol,
            signature,
            summary,
            ..
        } => {
            app.docs.push_back(super::app::DocEntry {
                idx: app.docs.len(),
                symbol,
                signature,
                summary,
            });
            app.selected_doc = Some(app.docs.len() - 1);
        }
        ReplOutput::StatusMessage(_) => {
            if result.should_exit {
                app.should_quit = true;
                return;
            }
            app.push_result(&source, vec![], ResultEntryKind::Info);
        }
        _ => {
            app.push_result(&source, vec![], ResultEntryKind::EvalSuccess);
        }
    }
    if result.should_exit {
        app.should_quit = true;
    }
    app.update_status();
}

pub(super) fn submit_command(app: &mut App, engine: &mut ReplEngine) {
    let raw = app.command.text.trim().to_string();
    app.command.clear();
    app.input_mode = InputMode::Insert;
    app.completion.clear();
    app.update_status();

    if raw.is_empty() {
        return;
    }

    let mut parts = raw.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();

    match cmd {
        "q" | "quit" => app.should_quit = true,
        "help" => {
            app.push_result(
                ":help",
                vec![
                    ":q :help :doc <sym> :sig <sym> :type <expr> :save <path> :v <idx> :j <idx>"
                        .to_string(),
                ],
                ResultEntryKind::Info,
            );
        }
        "save" => {
            if arg.is_empty() {
                app.push_result(
                    ":save",
                    vec!["Usage: :save <path>".to_string()],
                    ResultEntryKind::EvalError,
                );
            } else {
                let result = engine.handle_line(&format!(":save {arg}"));
                let lines = match result.output {
                    ReplOutput::CommandOutput { rendered } => rendered,
                    _ => vec![format!("saved to {arg}")],
                };
                app.push_result(format!(":save {arg}"), lines, ResultEntryKind::Info);
            }
        }
        "doc" => {
            if arg.is_empty() {
                app.push_result(
                    ":doc",
                    vec!["Usage: :doc <symbol>".to_string()],
                    ResultEntryKind::EvalError,
                );
            } else {
                let result = engine.handle_line(&format!(":doc {arg}"));
                match result.output {
                    ReplOutput::DocResolved {
                        symbol,
                        signature,
                        summary,
                        ..
                    } => {
                        app.docs.push_back(super::app::DocEntry {
                            idx: app.docs.len(),
                            symbol,
                            signature,
                            summary,
                        });
                        app.selected_doc = Some(app.docs.len() - 1);
                    }
                    ReplOutput::EvalError { rendered, .. } => {
                        app.push_result(
                            format!(":doc {arg}"),
                            rendered,
                            ResultEntryKind::EvalError,
                        );
                    }
                    ReplOutput::CommandOutput { rendered } => {
                        app.push_result(
                            format!(":doc {arg}"),
                            rendered,
                            ResultEntryKind::CommandOutput,
                        );
                    }
                    _ => {}
                }
            }
        }
        "sig" => {
            app.push_result(
                format!(":sig {arg}"),
                vec![format!("sig({arg}): (not yet implemented)")],
                ResultEntryKind::Info,
            );
        }
        "type" => {
            if arg.is_empty() {
                app.push_result(
                    ":type",
                    vec!["Usage: :type <expr>".to_string()],
                    ResultEntryKind::EvalError,
                );
            } else {
                app.push_result(
                    format!(":type {arg}"),
                    vec![format!("type({arg}): (not yet implemented)")],
                    ResultEntryKind::Info,
                );
            }
        }
        "v" => {
            // Recall source of a previous result into the input buffer.
            match arg.parse::<usize>() {
                Ok(idx) => {
                    if let Some(entry) = app.results.iter().find(|e| e.idx == idx) {
                        let src = entry.source.clone();
                        app.input.set(src);
                    } else {
                        app.push_result(
                            format!(":v {arg}"),
                            vec![format!("no result with idx {arg}")],
                            ResultEntryKind::EvalError,
                        );
                    }
                }
                _ => {
                    app.push_result(
                        format!(":v {arg}"),
                        vec![format!("invalid index: {arg}")],
                        ResultEntryKind::EvalError,
                    );
                }
            }
        }
        "j" => {
            if let Ok(idx) = arg.parse::<usize>() {
                if let Some(pos) = app.results.iter().position(|e| e.idx == idx) {
                    app.selected_result = Some(idx);
                    let line_offset: usize = app
                        .results
                        .iter()
                        .take(pos)
                        .map(|e| 3 + e.rendered_lines.len())
                        .sum();
                    app.results_scroll = line_offset;
                } else {
                    app.push_result(
                        format!(":j {arg}"),
                        vec![format!("no result with idx {arg}")],
                        ResultEntryKind::EvalError,
                    );
                }
            }
        }
        "doc-focus" => {
            if let Ok(idx) = arg.parse::<usize>() {
                if idx < app.docs.len() {
                    app.selected_doc = Some(idx);
                }
            }
        }
        "doc-drop" => {
            if let Ok(idx) = arg.parse::<usize>() {
                if idx < app.docs.len() {
                    app.docs.remove(idx);
                    for (i, d) in app.docs.iter_mut().enumerate() {
                        d.idx = i;
                    }
                    app.selected_doc = app.selected_doc.filter(|&s| s < app.docs.len());
                }
            }
        }
        "doc-clear" => {
            app.docs.clear();
            app.selected_doc = None;
        }
        other => {
            app.push_result(
                format!(":{other}"),
                vec![format!("unknown command: {other}")],
                ResultEntryKind::EvalError,
            );
        }
    }
}
