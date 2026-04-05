//! TUI REPL — `surtr tui [file.eldr]`
//!
//! Layout (top → bottom):
//!   Results/History  (scrollable)
//!   Docs Queue       (fixed height)
//!   Completion       (collapsible)
//!   Input / Command  (growable)
//!   Status bar       (1 line)

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Padding, Paragraph},
    Terminal,
};

use crate::{ReplEngine, ReplOutcome};

// ── Options ──────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct TuiOptions {
    /// Path to a `.eldr` file to preload into the VM before the session starts.
    pub eldr_path: Option<String>,
}

// ── Data model ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusPane {
    Input,
    Results,
    Docs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Insert,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultEntryKind {
    EvalSuccess,
    EvalError,
    CommandOutput,
    Info,
}

/// One evaluation unit in the results pane.
/// `source` is preserved so `:v idx` can restore it to the input buffer.
#[derive(Debug, Clone)]
struct ResultEntry {
    idx: usize,
    source: String,
    rendered_lines: Vec<String>,
    kind: ResultEntryKind,
}

#[derive(Debug, Clone)]
struct DocEntry {
    idx: usize,
    symbol: String,
    signature: Option<String>,
    summary: Option<String>,
}

#[derive(Debug, Clone)]
struct CompletionItem {
    label: String,
    detail: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct Completion {
    visible: bool,
    selected: usize,
    items: Vec<CompletionItem>,
}

impl Completion {
    fn clear(&mut self) {
        self.visible = false;
        self.selected = 0;
        self.items.clear();
    }

    fn select_prev(&mut self) {
        if !self.items.is_empty() {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1).min(self.items.len() - 1);
        }
    }

    fn apply(&self, buf: &mut InputBuffer) {
        if let Some(item) = self.items.get(self.selected) {
            let prefix_len = current_token_prefix(buf).len();
            let cursor = buf.cursor_byte;
            buf.text.drain((cursor - prefix_len)..cursor);
            buf.cursor_byte -= prefix_len;
            for ch in item.label.chars() {
                buf.insert_char(ch);
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
struct InputBuffer {
    text: String,
    cursor_byte: usize,
}

impl InputBuffer {
    fn insert_char(&mut self, ch: char) {
        self.text.insert(self.cursor_byte, ch);
        self.cursor_byte += ch.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor_byte == 0 {
            return;
        }
        let prev = self.text[..self.cursor_byte]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.text.drain(prev..self.cursor_byte);
        self.cursor_byte = prev;
    }

    fn move_left(&mut self) {
        if self.cursor_byte == 0 {
            return;
        }
        self.cursor_byte = self.text[..self.cursor_byte]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
    }

    fn move_right(&mut self) {
        if self.cursor_byte < self.text.len() {
            if let Some(ch) = self.text[self.cursor_byte..].chars().next() {
                self.cursor_byte += ch.len_utf8();
            }
        }
    }

    fn clear(&mut self) {
        self.text.clear();
        self.cursor_byte = 0;
    }

    fn set(&mut self, text: String) {
        self.cursor_byte = text.len();
        self.text = text;
    }
}

struct App {
    should_quit: bool,
    focus: FocusPane,
    input_mode: InputMode,
    input: InputBuffer,
    command: InputBuffer,
    results: VecDeque<ResultEntry>,
    results_scroll: usize,
    selected_result: Option<usize>,
    docs: VecDeque<DocEntry>,
    selected_doc: Option<usize>,
    completion: Completion,
    status: String,
}

impl App {
    fn new() -> Self {
        Self {
            should_quit: false,
            focus: FocusPane::Input,
            input_mode: InputMode::Insert,
            input: InputBuffer::default(),
            command: InputBuffer::default(),
            results: VecDeque::new(),
            results_scroll: 0,
            selected_result: None,
            docs: VecDeque::new(),
            selected_doc: None,
            completion: Completion::default(),
            status: "INSERT | Tab Focus | Ctrl-C Quit".to_string(),
        }
    }

    fn push_result(&mut self, source: impl Into<String>, rendered_lines: Vec<String>, kind: ResultEntryKind) {
        let idx = self.results.len();
        self.results.push_back(ResultEntry {
            idx,
            source: source.into(),
            rendered_lines,
            kind,
        });
        // Keep scroll at bottom when new output arrives.
        self.results_scroll = self.results.len().saturating_sub(1);
    }

    fn active_buf(&self) -> &InputBuffer {
        match self.input_mode {
            InputMode::Insert => &self.input,
            InputMode::Command => &self.command,
        }
    }

    fn active_buf_mut(&mut self) -> &mut InputBuffer {
        match self.input_mode {
            InputMode::Insert => &mut self.input,
            InputMode::Command => &mut self.command,
        }
    }

    fn next_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Input => FocusPane::Results,
            FocusPane::Results => FocusPane::Docs,
            FocusPane::Docs => FocusPane::Input,
        };
        self.update_status();
    }

    fn prev_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Input => FocusPane::Docs,
            FocusPane::Results => FocusPane::Input,
            FocusPane::Docs => FocusPane::Results,
        };
        self.update_status();
    }

    fn update_status(&mut self) {
        let mode = match self.input_mode {
            InputMode::Insert => "INSERT",
            InputMode::Command => "COMMAND",
        };
        let focus = match self.focus {
            FocusPane::Input => "Input",
            FocusPane::Results => "Results",
            FocusPane::Docs => "Docs",
        };
        self.status = format!("{mode} | Focus: {focus} | Tab Focus | Ctrl-C Quit");
    }
}

// ── Completion helpers ────────────────────────────────────────────────────────

fn current_token_prefix(buf: &InputBuffer) -> String {
    let before = &buf.text[..buf.cursor_byte];
    before
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '{' | '}' | ',' | ':'))
        .last()
        .unwrap_or("")
        .to_string()
}

fn refresh_completion(app: &mut App, engine: &ReplEngine) {
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
                    .map(|s| CompletionItem { label: s.clone(), detail: None })
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

// ── Event handling ────────────────────────────────────────────────────────────

fn handle_key(app: &mut App, engine: &mut ReplEngine, key: KeyEvent) {
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
            let max = app.results.len().saturating_sub(1);
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

fn submit_input(app: &mut App, engine: &mut ReplEngine) {
    let source = app.input.text.trim().to_string();
    if source.is_empty() {
        return;
    }

    app.input.clear();
    app.completion.clear();

    let idx = app.results.len();
    let prompt = format!("xldr({})> {}", idx + 1, source);
    let sep = "-".repeat(40);

    match engine.handle_line_tui(&source) {
        TuiEvalResult::Ok(lines) => {
            let mut rendered = vec![sep.clone(), prompt];
            rendered.extend(lines);
            rendered.push(sep);
            app.push_result(&source, rendered, ResultEntryKind::EvalSuccess);
        }
        TuiEvalResult::Error(text) => {
            let rendered = vec![sep.clone(), prompt, text, sep];
            app.push_result(&source, rendered, ResultEntryKind::EvalError);
        }
        TuiEvalResult::Exit => {
            app.should_quit = true;
            return;
        }
        TuiEvalResult::Continue => {
            let rendered = vec![sep.clone(), prompt, sep];
            app.push_result(&source, rendered, ResultEntryKind::EvalSuccess);
        }
    }
    app.update_status();
}

fn submit_command(app: &mut App, engine: &mut ReplEngine) {
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
                vec![":q :help :doc <sym> :sig <sym> :type <expr> :save <path> :v <idx> :j <idx>".to_string()],
                ResultEntryKind::Info,
            );
        }
        "save" => {
            if arg.is_empty() {
                app.push_result(":save", vec!["Usage: :save <path>".to_string()], ResultEntryKind::EvalError);
            } else {
                engine.handle_line_tui(&format!(":save {arg}"));
                app.push_result(format!(":save {arg}"), vec![format!("saved to {arg}")], ResultEntryKind::Info);
            }
        }
        "doc" => {
            if arg.is_empty() {
                app.push_result(":doc", vec!["Usage: :doc <symbol>".to_string()], ResultEntryKind::EvalError);
            } else {
                // Placeholder: future work hooks into sigil doc resolution.
                app.docs.push_back(DocEntry {
                    idx: app.docs.len(),
                    symbol: arg.to_string(),
                    signature: None,
                    summary: Some("(doc resolution not yet implemented)".to_string()),
                });
                app.selected_doc = Some(app.docs.len() - 1);
            }
        }
        "sig" => {
            app.push_result(format!(":sig {arg}"), vec![format!("sig({arg}): (not yet implemented)")], ResultEntryKind::Info);
        }
        "type" => {
            if arg.is_empty() {
                app.push_result(":type", vec!["Usage: :type <expr>".to_string()], ResultEntryKind::EvalError);
            } else {
                app.push_result(format!(":type {arg}"), vec![format!("type({arg}): (not yet implemented)")], ResultEntryKind::Info);
            }
        }
        "v" => {
            // Recall source of a previous result into the input buffer.
            match arg.parse::<usize>() {
                Ok(idx) if idx >= 1 => {
                    if let Some(entry) = app.results.get(idx - 1) {
                        app.input.set(entry.source.clone());
                    } else {
                        app.push_result(
                            format!(":v {arg}"),
                            vec![format!("no result at index {arg}")],
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
                app.results_scroll = idx.min(app.results.len().saturating_sub(1));
                app.selected_result = Some(app.results_scroll);
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

// ── Engine bridge ─────────────────────────────────────────────────────────────

/// Result of evaluating one REPL line in TUI mode.
pub enum TuiEvalResult {
    /// Successful evaluation; payload is the display lines (may be empty for Unit).
    Ok(Vec<String>),
    /// Evaluation produced an error message.
    Error(String),
    /// No value to display (e.g. incomplete input, import-only).
    Continue,
    /// Session should end.
    Exit,
}

impl ReplEngine {
    /// Evaluate one line and return a structured result for the TUI.
    ///
    /// On success, returns display lines formatted like the CLI REPL
    /// (`name: Type = value` for bindings, raw value otherwise).
    pub fn handle_line_tui(&mut self, line: &str) -> TuiEvalResult {
        let prev_result_count = self.result_count();
        match self.handle_line(line) {
            ReplOutcome::Exit => TuiEvalResult::Exit,
            ReplOutcome::Continue => {
                if self.result_count() > prev_result_count {
                    let last_idx = self.result_count() - 1;
                    let lines = self.format_result_lines(last_idx);
                    TuiEvalResult::Ok(lines)
                } else {
                    TuiEvalResult::Continue
                }
            }
        }
    }
}

// ── Drawing ───────────────────────────────────────────────────────────────────

fn draw(frame: &mut Frame, app: &App) {
    let completion_h = if app.completion.visible { 5u16 } else { 1 };
    let input_lines = app.active_buf().text.lines().count().max(1) as u16;
    let input_h = input_lines.clamp(3, 8);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(5),
            Constraint::Length(completion_h),
            Constraint::Length(input_h),
            Constraint::Length(1),
        ])
        .split(frame.area());

    draw_results(frame, app, chunks[0]);
    draw_docs(frame, app, chunks[1]);
    draw_completion(frame, app, chunks[2]);
    draw_input(frame, app, chunks[3]);
    draw_status(frame, app, chunks[4]);
}

fn pane_title(name: &str, focused: bool) -> String {
    if focused {
        format!("[*] {name}")
    } else {
        format!("[ ] {name}")
    }
}

fn result_entry_to_list_item(entry: &ResultEntry) -> ListItem<'static> {
    let base_style = match entry.kind {
        ResultEntryKind::EvalError => Style::default().fg(Color::Red),
        ResultEntryKind::Info => Style::default().fg(Color::Yellow),
        ResultEntryKind::EvalSuccess | ResultEntryKind::CommandOutput => Style::default(),
    };
    // Result lines (bindings / values) in green.
    let result_style = match entry.kind {
        ResultEntryKind::EvalSuccess => Style::default().fg(Color::Green),
        _ => base_style,
    };

    let lines: Vec<Line<'static>> = entry
        .rendered_lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            // First and last line (separator) use base_style; middle lines that
            // are not the prompt use result_style.
            let is_sep = i == 0 || i == entry.rendered_lines.len() - 1;
            let is_prompt = i == 1 && !entry.rendered_lines.is_empty();
            let style = if is_sep || is_prompt { base_style } else { result_style };
            Line::styled(line.clone(), style)
        })
        .collect();

    ListItem::new(Text::from(lines))
}

fn draw_results(frame: &mut Frame, app: &App, area: Rect) {
    let title = pane_title("Results / History", app.focus == FocusPane::Results);
    let items: Vec<ListItem> = app
        .results
        .iter()
        .skip(app.results_scroll)
        .map(result_entry_to_list_item)
        .collect();

    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL).padding(Padding::horizontal(1)));
    frame.render_widget(list, area);
}

fn draw_docs(frame: &mut Frame, app: &App, area: Rect) {
    let title = pane_title("Docs Queue", app.focus == FocusPane::Docs);
    let lines: Vec<Line> = if app.docs.is_empty() {
        vec![Line::from("(empty)")]
    } else {
        app.docs
            .iter()
            .map(|d| {
                let sig = d.signature.clone().unwrap_or_default();
                let focused = app.selected_doc == Some(d.idx);
                let marker = if focused { ">" } else { " " };
                Line::from(format!("{marker} [{}] {} {}", d.idx, d.symbol, sig))
            })
            .collect()
    };
    let para = Paragraph::new(lines)
        .block(Block::default().title(title).borders(Borders::ALL).padding(Padding::horizontal(1)));
    frame.render_widget(para, area);
}

fn draw_completion(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().title("Completion").borders(Borders::ALL).padding(Padding::horizontal(1));
    let lines: Vec<Line> = if app.completion.visible {
        app.completion
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let cursor = if i == app.completion.selected { ">" } else { " " };
                let detail = item.detail.clone().unwrap_or_default();
                let style = if i == app.completion.selected {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                };
                Line::styled(format!("{cursor} {:<16} {}", item.label, detail), style)
            })
            .collect()
    } else {
        vec![Line::from("")]
    };
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let title = match app.input_mode {
        InputMode::Insert => pane_title("Input", app.focus == FocusPane::Input),
        InputMode::Command => pane_title("Command", app.focus == FocusPane::Input),
    };
    let text = app.active_buf().text.clone();
    let para = Paragraph::new(text)
        .block(Block::default().title(title).borders(Borders::ALL).padding(Padding::horizontal(1)));
    frame.render_widget(para, area);
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Paragraph::new(app.status.clone()), area);
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Launch the TUI REPL.  Called from `rune` as `surtr tui [file.eldr]`.
pub fn run_command(options: TuiOptions) -> Result<(), i32> {
    let mut engine = match &options.eldr_path {
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|e| {
                eprintln!("tui: cannot read {}: {}", path, e);
                1i32
            })?;
            ReplEngine::from_eldr(&bytes).map_err(|e| {
                eprintln!("tui: {}", e);
                1i32
            })?
        }
        None => ReplEngine::new().map_err(|e| {
            eprintln!("tui: failed to initialise engine: {}", e);
            1i32
        })?,
    };

    let mut app = App::new();
    if let Some(path) = &options.eldr_path {
        app.push_result(path.clone(), vec![format!("loaded {path}")], ResultEntryKind::Info);
    }

    enable_raw_mode().map_err(|e| {
        eprintln!("tui: terminal init failed: {}", e);
        1i32
    })?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| {
        eprintln!("tui: {}", e);
        let _ = disable_raw_mode();
        1i32
    })?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).map_err(|e| {
        eprintln!("tui: {}", e);
        let _ = disable_raw_mode();
        1i32
    })?;

    let result = run_loop(&mut terminal, &mut app, &mut engine);

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    engine: &mut ReplEngine,
) -> Result<(), i32> {
    while !app.should_quit {
        terminal.draw(|f| draw(f, app)).map_err(|e| {
            eprintln!("tui: draw error: {}", e);
            1i32
        })?;

        let timeout = Duration::from_millis(100);
        if event::poll(timeout).unwrap_or(false) {
            match event::read() {
                Ok(CrosstermEvent::Key(key)) => handle_key(app, engine, key),
                Ok(_) => {}
                Err(e) => {
                    eprintln!("tui: event error: {}", e);
                    return Err(1);
                }
            }
        }
    }
    Ok(())
}
