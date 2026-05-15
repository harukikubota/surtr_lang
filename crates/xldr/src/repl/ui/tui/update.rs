//! TUI key handling and event processing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::repl::logic::core::ReplEngine;
use crate::repl::logic::{present_for_interaction, PresentedEvent, PresentedResultKind};

use super::app::{App, Completion, CompletionItem, FocusPane, InputBuffer, InputMode};

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
    ("error", ":error [full|summary]  — set error display mode"),
    ("sig", ":sig <function|query>  — show signature"),
    ("info", ":info <query>  — show derived info"),
    (
        "type",
        ":type <binding>  — lookup binding type (annotate unresolved generics before persistence)",
    ),
    ("facet", ":facet <binding|expr>  — inspect facet path"),
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
            let max: usize = app
                .results
                .iter()
                .map(|e| 3 + e.stdout_lines.len() + e.rendered_lines.len() + e.stderr_lines.len())
                .sum();
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

    let presented = present_for_interaction(engine.handle_line(&source));
    match presented.event {
        PresentedEvent::None => {}
        PresentedEvent::Result(result) => app.push_result(
            &source,
            result.stdout_lines,
            result.lines,
            result.stderr_lines,
            result.kind,
        ),
        PresentedEvent::Doc(doc) => app.push_doc(doc),
    }
    if presented.should_exit {
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
        "help" | "save" | "doc" | "error" | "sig" | "info" | "type" | "facet" => {
            let line = if arg.is_empty() {
                format!(":{cmd}")
            } else {
                format!(":{cmd} {arg}")
            };
            let presented = present_for_interaction(engine.handle_line(&line));
            match presented.event {
                PresentedEvent::Result(result) => {
                    app.push_result(
                        line,
                        result.stdout_lines,
                        result.lines,
                        result.stderr_lines,
                        result.kind,
                    );
                }
                PresentedEvent::Doc(doc) => app.push_doc(doc),
                PresentedEvent::None => {}
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
                            Vec::new(),
                            vec![format!("no result with idx {arg}")],
                            Vec::new(),
                            PresentedResultKind::EvalError,
                        );
                    }
                }
                _ => {
                    app.push_result(
                        format!(":v {arg}"),
                        Vec::new(),
                        vec![format!("invalid index: {arg}")],
                        Vec::new(),
                        PresentedResultKind::EvalError,
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
                        .map(|e| {
                            3 + e.stdout_lines.len() + e.rendered_lines.len() + e.stderr_lines.len()
                        })
                        .sum();
                    app.results_scroll = line_offset;
                } else {
                    app.push_result(
                        format!(":j {arg}"),
                        Vec::new(),
                        vec![format!("no result with idx {arg}")],
                        Vec::new(),
                        PresentedResultKind::EvalError,
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
                Vec::new(),
                vec![format!("unknown command: {other}")],
                Vec::new(),
                PresentedResultKind::EvalError,
            );
        }
    }
}
