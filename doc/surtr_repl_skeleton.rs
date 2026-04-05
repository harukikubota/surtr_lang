// Surtr REPL skeleton
// Modules are kept in a single file for easy `cat`-based review.

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
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Terminal,
};

// =========================
// app
// =========================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Input,
    Results,
    Docs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Insert,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    Input,
    Results,
    Docs,
}

impl From<FocusPane> for PaneKind {
    fn from(value: FocusPane) -> Self {
        match value {
            FocusPane::Input => PaneKind::Input,
            FocusPane::Results => PaneKind::Results,
            FocusPane::Docs => PaneKind::Docs,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplEntryKind {
    Input,
    Result,
    Error,
    Info,
    CommandOutput,
}

#[derive(Debug, Clone)]
pub struct ReplEntry {
    pub idx: usize,
    pub kind: ReplEntryKind,
    pub source: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    Function,
    Type,
    Module,
    Keyword,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct DocEntry {
    pub idx: usize,
    pub symbol: String,
    pub kind: DocKind,
    pub signature: Option<String>,
    pub summary: Option<String>,
    pub module_path: Option<String>,
    pub source_snippet: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StatusLine {
    pub message: String,
}

impl Default for StatusLine {
    fn default() -> Self {
        Self {
            message: "Ready".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct InputBuffer {
    pub text: String,
    pub cursor_byte: usize,
}

impl Default for InputBuffer {
    fn default() -> Self {
        Self {
            text: String::new(),
            cursor_byte: 0,
        }
    }
}

impl InputBuffer {
    pub fn insert_char(&mut self, ch: char) {
        self.text.insert(self.cursor_byte, ch);
        self.cursor_byte += ch.len_utf8();
    }

    pub fn backspace(&mut self) {
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

    pub fn move_left(&mut self) {
        if self.cursor_byte == 0 {
            return;
        }

        self.cursor_byte = self.text[..self.cursor_byte]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
    }

    pub fn move_right(&mut self) {
        if self.cursor_byte >= self.text.len() {
            return;
        }

        if let Some(ch) = self.text[self.cursor_byte..].chars().next() {
            self.cursor_byte += ch.len_utf8();
        }
    }

    pub fn set_text(&mut self, text: String) {
        self.cursor_byte = text.len();
        self.text = text;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Local,
    Function,
    Module,
    Type,
    Keyword,
    Command,
}

#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub signature: Option<String>,
    pub type_summary: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CompletionState {
    pub visible: bool,
    pub selected: usize,
    pub items: Vec<CompletionItem>,
}

impl CompletionState {
    pub fn clear(&mut self) {
        self.visible = false;
        self.selected = 0;
        self.items.clear();
    }

    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.items.len() - 1);
    }

    pub fn select_prev(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
    }
}

#[derive(Debug)]
pub struct ReplApp {
    pub should_quit: bool,
    pub focus: FocusPane,
    pub input_mode: InputMode,
    pub input: InputBuffer,
    pub command: InputBuffer,
    pub results: VecDeque<ReplEntry>,
    pub docs_queue: VecDeque<DocEntry>,
    pub selected_result: Option<usize>,
    pub selected_doc: Option<usize>,
    pub results_scroll: u16,
    pub completion: CompletionState,
    pub status: StatusLine,
    pub pending_command_specs: Vec<CommandSpec>,
    pub last_parsed_command: Option<ParsedCommand>,
}

impl Default for ReplApp {
    fn default() -> Self {
        Self {
            should_quit: false,
            focus: FocusPane::Input,
            input_mode: InputMode::Insert,
            input: InputBuffer::default(),
            command: InputBuffer::default(),
            results: VecDeque::new(),
            docs_queue: VecDeque::new(),
            selected_result: None,
            selected_doc: None,
            results_scroll: 0,
            completion: CompletionState::default(),
            status: StatusLine::default(),
            pending_command_specs: Vec::new(),
            last_parsed_command: None,
        }
    }
}

impl ReplApp {
    pub fn focused_pane_kind(&self) -> PaneKind {
        self.focus.into()
    }

    pub fn active_buffer(&self) -> &InputBuffer {
        match self.input_mode {
            InputMode::Insert => &self.input,
            InputMode::Command => &self.command,
        }
    }

    pub fn active_buffer_mut(&mut self) -> &mut InputBuffer {
        match self.input_mode {
            InputMode::Insert => &mut self.input,
            InputMode::Command => &mut self.command,
        }
    }

    pub fn next_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Input => FocusPane::Results,
            FocusPane::Results => FocusPane::Docs,
            FocusPane::Docs => FocusPane::Input,
        };
        self.status.message = format!("Focus: {:?}", self.focus);
    }

    pub fn prev_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Input => FocusPane::Docs,
            FocusPane::Results => FocusPane::Input,
            FocusPane::Docs => FocusPane::Results,
        };
        self.status.message = format!("Focus: {:?}", self.focus);
    }

    pub fn push_result(
        &mut self,
        kind: ReplEntryKind,
        source: Option<String>,
        text: impl Into<String>,
    ) {
        let idx = self.results.len();
        self.results.push_back(ReplEntry {
            idx,
            kind,
            source,
            text: text.into(),
        });
    }

    pub fn push_doc(&mut self, mut doc: DocEntry) {
        doc.idx = self.docs_queue.len();
        self.docs_queue.push_back(doc);
        self.selected_doc = Some(self.docs_queue.len().saturating_sub(1));
    }
}

// =========================
// command
// =========================

#[derive(Debug, Clone)]
pub enum GlobalCommand {
    Quit,
    Help,
    Doc { symbol: String },
    Sig { symbol: String },
    TypeOf { expr: String },
}

#[derive(Debug, Clone)]
pub enum InputPaneCommand {
    ViewResult { idx: usize },
}

#[derive(Debug, Clone)]
pub enum ResultPaneCommand {
    Jump { idx: usize },
}

#[derive(Debug, Clone)]
pub enum DocsPaneCommand {
    Focus { idx: usize },
    Drop { idx: usize },
    Clear,
}

#[derive(Debug, Clone)]
pub enum PaneCommand {
    Input(InputPaneCommand),
    Results(ResultPaneCommand),
    Docs(DocsPaneCommand),
}

#[derive(Debug, Clone)]
pub enum ParsedCommand {
    Global(GlobalCommand),
    Pane(PaneCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandScope {
    Global,
    Pane(PaneKind),
}

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub summary: &'static str,
    pub scope: CommandScope,
}

pub fn global_command_specs() -> &'static [CommandSpec] {
    &[
        CommandSpec {
            name: "q",
            aliases: &["quit"],
            usage: ":q",
            summary: "Quit REPL",
            scope: CommandScope::Global,
        },
        CommandSpec {
            name: "help",
            aliases: &["h"],
            usage: ":help",
            summary: "Show help",
            scope: CommandScope::Global,
        },
        CommandSpec {
            name: "doc",
            aliases: &[],
            usage: ":doc <symbol>",
            summary: "Show docs for symbol",
            scope: CommandScope::Global,
        },
        CommandSpec {
            name: "sig",
            aliases: &[],
            usage: ":sig <symbol>",
            summary: "Show signature for symbol",
            scope: CommandScope::Global,
        },
        CommandSpec {
            name: "type",
            aliases: &["t"],
            usage: ":type <expr>",
            summary: "Infer type of expression",
            scope: CommandScope::Global,
        },
    ]
}

pub fn pane_command_specs(pane: PaneKind) -> &'static [CommandSpec] {
    match pane {
        PaneKind::Input => &[
            CommandSpec {
                name: "v",
                aliases: &[],
                usage: ":v <idx>",
                summary: "Copy result input into input area",
                scope: CommandScope::Pane(PaneKind::Input),
            },
        ],
        PaneKind::Results => &[
            CommandSpec {
                name: "j",
                aliases: &[],
                usage: ":j <idx>",
                summary: "Jump to result index",
                scope: CommandScope::Pane(PaneKind::Results),
            },
        ],
        PaneKind::Docs => &[
            CommandSpec {
                name: "doc-focus",
                aliases: &[],
                usage: ":doc-focus <idx>",
                summary: "Focus doc entry",
                scope: CommandScope::Pane(PaneKind::Docs),
            },
            CommandSpec {
                name: "doc-drop",
                aliases: &[],
                usage: ":doc-drop <idx>",
                summary: "Drop doc entry",
                scope: CommandScope::Pane(PaneKind::Docs),
            },
            CommandSpec {
                name: "doc-clear",
                aliases: &[],
                usage: ":doc-clear",
                summary: "Clear all doc entries",
                scope: CommandScope::Pane(PaneKind::Docs),
            },
        ],
    }
}

pub fn merged_command_specs(pane: PaneKind) -> Vec<CommandSpec> {
    let mut specs = global_command_specs().to_vec();
    specs.extend_from_slice(pane_command_specs(pane));
    specs
}

#[derive(Debug)]
pub enum CommandParseError {
    Empty,
    UnknownCommand(String),
    MissingArgument(&'static str),
    InvalidNumber(String),
}

pub fn parse_command(input: &str, pane: PaneKind) -> Result<ParsedCommand, CommandParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(CommandParseError::Empty);
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap();
    let rest = parts.next().unwrap_or("").trim();

    match name {
        "q" | "quit" => Ok(ParsedCommand::Global(GlobalCommand::Quit)),
        "help" | "h" => Ok(ParsedCommand::Global(GlobalCommand::Help)),
        "doc" => {
            if rest.is_empty() {
                return Err(CommandParseError::MissingArgument("<symbol>"));
            }
            Ok(ParsedCommand::Global(GlobalCommand::Doc {
                symbol: rest.to_string(),
            }))
        }
        "sig" => {
            if rest.is_empty() {
                return Err(CommandParseError::MissingArgument("<symbol>"));
            }
            Ok(ParsedCommand::Global(GlobalCommand::Sig {
                symbol: rest.to_string(),
            }))
        }
        "type" | "t" => {
            if rest.is_empty() {
                return Err(CommandParseError::MissingArgument("<expr>"));
            }
            Ok(ParsedCommand::Global(GlobalCommand::TypeOf {
                expr: rest.to_string(),
            }))
        }
        "v" if pane == PaneKind::Input => {
            let idx = parse_usize(rest)?;
            Ok(ParsedCommand::Pane(PaneCommand::Input(
                InputPaneCommand::ViewResult { idx },
            )))
        }
        "j" if pane == PaneKind::Results => {
            let idx = parse_usize(rest)?;
            Ok(ParsedCommand::Pane(PaneCommand::Results(
                ResultPaneCommand::Jump { idx },
            )))
        }
        "doc-focus" if pane == PaneKind::Docs => {
            let idx = parse_usize(rest)?;
            Ok(ParsedCommand::Pane(PaneCommand::Docs(
                DocsPaneCommand::Focus { idx },
            )))
        }
        "doc-drop" if pane == PaneKind::Docs => {
            let idx = parse_usize(rest)?;
            Ok(ParsedCommand::Pane(PaneCommand::Docs(
                DocsPaneCommand::Drop { idx },
            )))
        }
        "doc-clear" if pane == PaneKind::Docs => {
            Ok(ParsedCommand::Pane(PaneCommand::Docs(DocsPaneCommand::Clear)))
        }
        other => Err(CommandParseError::UnknownCommand(other.to_string())),
    }
}

fn parse_usize(input: &str) -> Result<usize, CommandParseError> {
    if input.is_empty() {
        return Err(CommandParseError::MissingArgument("<idx>"));
    }

    input
        .parse::<usize>()
        .map_err(|_| CommandParseError::InvalidNumber(input.to_string()))
}

// =========================
// backend
// =========================

pub trait ReplBackend {
    fn eval(&mut self, source: &str) -> Result<String, String>;
    fn resolve_doc(&mut self, symbol: &str) -> Result<DocEntry, String>;
    fn resolve_sig(&mut self, symbol: &str) -> Result<String, String>;
    fn infer_type(&mut self, expr: &str) -> Result<String, String>;
    fn complete_symbol(&mut self, prefix: &str) -> Vec<CompletionItem>;
}

pub struct DummyBackend;

impl ReplBackend for DummyBackend {
    fn eval(&mut self, source: &str) -> Result<String, String> {
        Ok(format!("=> {source}"))
    }

    fn resolve_doc(&mut self, symbol: &str) -> Result<DocEntry, String> {
        Ok(DocEntry {
            idx: 0,
            symbol: symbol.to_string(),
            kind: DocKind::Function,
            signature: Some(format!("{symbol} : (TODO)")),
            summary: Some("TODO: resolve from symbol table".to_string()),
            module_path: Some("TODO".to_string()),
            source_snippet: None,
        })
    }

    fn resolve_sig(&mut self, symbol: &str) -> Result<String, String> {
        Ok(format!("{symbol} : (TODO signature)"))
    }

    fn infer_type(&mut self, expr: &str) -> Result<String, String> {
        Ok(format!("type({expr}) = TODO"))
    }

    fn complete_symbol(&mut self, prefix: &str) -> Vec<CompletionItem> {
        let samples = [
            CompletionItem {
                label: "List::map".to_string(),
                kind: CompletionKind::Function,
                signature: Some("(List<A>, A -> B) -> List<B>".to_string()),
                type_summary: None,
                detail: Some("std/list".to_string()),
            },
            CompletionItem {
                label: "Result".to_string(),
                kind: CompletionKind::Type,
                signature: None,
                type_summary: Some("Result<T, E>".to_string()),
                detail: None,
            },
        ];

        samples
            .into_iter()
            .filter(|item| item.label.starts_with(prefix))
            .collect()
    }
}

// =========================
// completion
// =========================

pub fn build_command_completion(prefix: &str, pane: PaneKind) -> CompletionState {
    let mut items = Vec::new();

    for spec in merged_command_specs(pane) {
        let matches_name = spec.name.starts_with(prefix);
        let matches_alias = spec.aliases.iter().any(|a| a.starts_with(prefix));

        if matches_name || matches_alias {
            let scope = match spec.scope {
                CommandScope::Global => "global",
                CommandScope::Pane(_) => "pane",
            };

            items.push(CompletionItem {
                label: spec.name.to_string(),
                kind: CompletionKind::Command,
                signature: Some(spec.usage.to_string()),
                type_summary: None,
                detail: Some(format!("{scope}: {}", spec.summary)),
            });
        }
    }

    CompletionState {
        visible: !items.is_empty(),
        selected: 0,
        items,
    }
}

pub fn build_symbol_completion(
    prefix: &str,
    backend: &mut dyn ReplBackend,
) -> CompletionState {
    let items = backend.complete_symbol(prefix);
    CompletionState {
        visible: !items.is_empty(),
        selected: 0,
        items,
    }
}

fn current_insert_prefix(input: &InputBuffer) -> String {
    let before_cursor = &input.text[..input.cursor_byte];
    let token = before_cursor
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '{' | '}' | '[' | ']' | ','))
        .last()
        .unwrap_or("");
    token.to_string()
}

// =========================
// event
// =========================

#[derive(Debug, Clone)]
pub enum AppEvent {
    Tick,
    Key(KeyEvent),
}

pub fn next_event(timeout: Duration) -> io::Result<AppEvent> {
    if event::poll(timeout)? {
        match event::read()? {
            CrosstermEvent::Key(key) => Ok(AppEvent::Key(key)),
            _ => Ok(AppEvent::Tick),
        }
    } else {
        Ok(AppEvent::Tick)
    }
}

// =========================
// update
// =========================

pub fn handle_key(app: &mut ReplApp, backend: &mut dyn ReplBackend, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    match app.focus {
        FocusPane::Input => handle_input_focus(app, backend, key),
        FocusPane::Results => handle_results_focus(app, key),
        FocusPane::Docs => handle_docs_focus(app, key),
    }
}

fn handle_input_focus(app: &mut ReplApp, backend: &mut dyn ReplBackend, key: KeyEvent) {
    match key.code {
        KeyCode::Tab => app.next_focus(),
        KeyCode::BackTab => app.prev_focus(),
        KeyCode::Left => {
            app.active_buffer_mut().move_left();
            refresh_completion(app, backend);
        }
        KeyCode::Right => {
            app.active_buffer_mut().move_right();
            refresh_completion(app, backend);
        }
        KeyCode::Up => app.completion.select_prev(),
        KeyCode::Down => app.completion.select_next(),
        KeyCode::Backspace => {
            app.active_buffer_mut().backspace();
            refresh_completion(app, backend);
        }
        KeyCode::Enter => match app.input_mode {
            InputMode::Insert => submit_input(app, backend),
            InputMode::Command => submit_command(app, backend),
        },
        KeyCode::Char(':') => match app.input_mode {
            InputMode::Insert => {
                if app.input.cursor_byte == 0 {
                    app.input_mode = InputMode::Command;
                    app.command.set_text(String::new());
                    refresh_completion(app, backend);
                } else {
                    app.input.insert_char(':');
                    refresh_completion(app, backend);
                }
            }
            InputMode::Command => {
                app.input_mode = InputMode::Insert;
                app.command.set_text(String::new());
                app.completion.clear();
            }
        },
        KeyCode::Esc => {
            if app.input_mode == InputMode::Command {
                app.input_mode = InputMode::Insert;
                app.command.set_text(String::new());
                app.completion.clear();
            }
        }
        KeyCode::Char(ch) => {
            app.active_buffer_mut().insert_char(ch);
            refresh_completion(app, backend);
        }
        _ => {}
    }
}

fn handle_results_focus(app: &mut ReplApp, key: KeyEvent) {
    match key.code {
        KeyCode::Tab => app.next_focus(),
        KeyCode::BackTab => app.prev_focus(),
        KeyCode::Up => app.results_scroll = app.results_scroll.saturating_sub(1),
        KeyCode::Down => app.results_scroll = app.results_scroll.saturating_add(1),
        _ => {}
    }
}

fn handle_docs_focus(app: &mut ReplApp, key: KeyEvent) {
    match key.code {
        KeyCode::Tab => app.next_focus(),
        KeyCode::BackTab => app.prev_focus(),
        KeyCode::Up => {
            let next = app.selected_doc.unwrap_or(0).saturating_sub(1);
            app.selected_doc = Some(next);
        }
        KeyCode::Down => {
            let max = app.docs_queue.len().saturating_sub(1);
            let next = app.selected_doc.unwrap_or(0).saturating_add(1).min(max);
            app.selected_doc = Some(next);
        }
        _ => {}
    }
}

fn refresh_completion(app: &mut ReplApp, backend: &mut dyn ReplBackend) {
    match app.input_mode {
        InputMode::Command => {
            let prefix = app.command.text.trim();
            app.completion = build_command_completion(prefix, app.focused_pane_kind());
        }
        InputMode::Insert => {
            let prefix = current_insert_prefix(&app.input);
            if prefix.is_empty() {
                app.completion.clear();
            } else {
                app.completion = build_symbol_completion(&prefix, backend);
            }
        }
    }
}

fn submit_input(app: &mut ReplApp, backend: &mut dyn ReplBackend) {
    let source = app.input.text.trim().to_string();
    if source.is_empty() {
        app.status.message = "Empty input".to_string();
        return;
    }

    app.push_result(ReplEntryKind::Input, Some(source.clone()), format!("> {source}"));

    match backend.eval(&source) {
        Ok(result_text) => {
            app.push_result(ReplEntryKind::Result, Some(source), result_text);
            app.status.message = "Evaluated".to_string();
        }
        Err(error_text) => {
            app.push_result(ReplEntryKind::Error, Some(source), error_text);
            app.status.message = "Evaluation error".to_string();
        }
    }

    app.input.set_text(String::new());
    app.completion.clear();
}

fn submit_command(app: &mut ReplApp, backend: &mut dyn ReplBackend) {
    let raw = app.command.text.trim().to_string();
    match parse_command(&raw, app.focused_pane_kind()) {
        Ok(parsed) => {
            app.last_parsed_command = Some(parsed.clone());
            execute_command(app, backend, parsed);
            app.command.set_text(String::new());
            app.input_mode = InputMode::Insert;
            app.completion.clear();
        }
        Err(err) => {
            app.push_result(ReplEntryKind::Error, None, format_command_error(err));
        }
    }
}

fn execute_command(app: &mut ReplApp, backend: &mut dyn ReplBackend, command: ParsedCommand) {
    match command {
        ParsedCommand::Global(cmd) => execute_global_command(app, backend, cmd),
        ParsedCommand::Pane(cmd) => execute_pane_command(app, cmd),
    }
}

fn execute_global_command(app: &mut ReplApp, backend: &mut dyn ReplBackend, cmd: GlobalCommand) {
    match cmd {
        GlobalCommand::Quit => app.should_quit = true,
        GlobalCommand::Help => {
            app.push_result(
                ReplEntryKind::CommandOutput,
                None,
                "Commands: :q, :help, :doc <symbol>, :sig <symbol>, :type <expr>",
            );
        }
        GlobalCommand::Doc { symbol } => match backend.resolve_doc(&symbol) {
            Ok(doc) => {
                app.push_doc(doc);
                app.status.message = format!("Doc added: {symbol}");
            }
            Err(err) => app.push_result(ReplEntryKind::Error, None, err),
        },
        GlobalCommand::Sig { symbol } => match backend.resolve_sig(&symbol) {
            Ok(sig) => app.push_result(ReplEntryKind::CommandOutput, None, sig),
            Err(err) => app.push_result(ReplEntryKind::Error, None, err),
        },
        GlobalCommand::TypeOf { expr } => match backend.infer_type(&expr) {
            Ok(ty) => app.push_result(ReplEntryKind::CommandOutput, None, ty),
            Err(err) => app.push_result(ReplEntryKind::Error, None, err),
        },
    }
}

fn execute_pane_command(app: &mut ReplApp, cmd: PaneCommand) {
    match cmd {
        PaneCommand::Input(InputPaneCommand::ViewResult { idx }) => {
            if let Some(entry) = app.results.get(idx) {
                if let Some(source) = &entry.source {
                    app.input.set_text(source.clone());
                    app.status.message = format!("Copied input from result {idx}");
                } else {
                    app.push_result(ReplEntryKind::Error, None, format!("No source for idx: {idx}"));
                }
            } else {
                app.push_result(ReplEntryKind::Error, None, format!("Result idx out of range: {idx}"));
            }
        }
        PaneCommand::Results(ResultPaneCommand::Jump { idx }) => {
            app.selected_result = Some(idx);
            app.status.message = format!("Jumped to result {idx}");
        }
        PaneCommand::Docs(DocsPaneCommand::Focus { idx }) => {
            if idx < app.docs_queue.len() {
                app.selected_doc = Some(idx);
                app.status.message = format!("Focused doc {idx}");
            } else {
                app.push_result(ReplEntryKind::Error, None, format!("Doc idx out of range: {idx}"));
            }
        }
        PaneCommand::Docs(DocsPaneCommand::Drop { idx }) => {
            if idx < app.docs_queue.len() {
                app.docs_queue.remove(idx);
                reindex_docs(app);
                app.status.message = format!("Dropped doc {idx}");
            } else {
                app.push_result(ReplEntryKind::Error, None, format!("Doc idx out of range: {idx}"));
            }
        }
        PaneCommand::Docs(DocsPaneCommand::Clear) => {
            app.docs_queue.clear();
            app.selected_doc = None;
            app.status.message = "Cleared docs".to_string();
        }
    }
}

fn reindex_docs(app: &mut ReplApp) {
    for (idx, doc) in app.docs_queue.iter_mut().enumerate() {
        doc.idx = idx;
    }
}

fn format_command_error(err: CommandParseError) -> String {
    match err {
        CommandParseError::Empty => "Empty command".to_string(),
        CommandParseError::UnknownCommand(cmd) => format!("Unknown command: {cmd}"),
        CommandParseError::MissingArgument(arg) => format!("Missing argument: {arg}"),
        CommandParseError::InvalidNumber(raw) => format!("Invalid number: {raw}"),
    }
}

// =========================
// ui
// =========================

pub fn draw(frame: &mut Frame, app: &ReplApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(7),
            Constraint::Length(completion_height(app)),
            Constraint::Length(input_height(app)),
            Constraint::Length(1),
        ])
        .split(frame.area());

    draw_results(frame, app, chunks[0]);
    draw_docs(frame, app, chunks[1]);
    draw_completion(frame, app, chunks[2]);
    draw_input(frame, app, chunks[3]);
    draw_status(frame, app, chunks[4]);
}

fn completion_height(app: &ReplApp) -> u16 {
    if app.completion.visible { 5 } else { 1 }
}

fn input_height(app: &ReplApp) -> u16 {
    let line_count = app.active_buffer().text.lines().count().max(1) as u16;
    line_count.clamp(3, 8)
}

fn draw_results(frame: &mut Frame, app: &ReplApp, area: Rect) {
    let title = pane_title("Results / History", app.focus == FocusPane::Results);
    let items: Vec<ListItem> = app
        .results
        .iter()
        .map(|entry| ListItem::new(entry.text.clone()))
        .collect();

    let list = List::new(items).block(Block::default().title(title).borders(Borders::ALL));
    frame.render_widget(list, area);
}

fn draw_docs(frame: &mut Frame, app: &ReplApp, area: Rect) {
    let title = pane_title("Docs Queue", app.focus == FocusPane::Docs);
    let lines: Vec<Line> = app
        .docs_queue
        .iter()
        .map(|doc| {
            let sig = doc.signature.clone().unwrap_or_default();
            Line::from(format!("[{}] {} {}", doc.idx, doc.symbol, sig))
        })
        .collect();

    let para = Paragraph::new(lines).block(Block::default().title(title).borders(Borders::ALL));
    frame.render_widget(para, area);
}

fn draw_completion(frame: &mut Frame, app: &ReplApp, area: Rect) {
    let block = Block::default().title("Completion").borders(Borders::ALL);

    let lines: Vec<Line> = if app.completion.visible {
        app.completion
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let cursor = if idx == app.completion.selected { ">" } else { " " };
                let sig = item
                    .signature
                    .clone()
                    .or(item.type_summary.clone())
                    .unwrap_or_default();
                Line::from(format!("{cursor} {:<14} {}", item.label, sig))
            })
            .collect()
    } else {
        vec![Line::from("")]
    };

    let para = Paragraph::new(lines).block(block);
    frame.render_widget(para, area);
}

fn draw_input(frame: &mut Frame, app: &ReplApp, area: Rect) {
    let title = match app.input_mode {
        InputMode::Insert => pane_title("Input", app.focus == FocusPane::Input),
        InputMode::Command => pane_title("Command", app.focus == FocusPane::Input),
    };

    let text = app.active_buffer().text.clone();
    let para = Paragraph::new(text).block(Block::default().title(title).borders(Borders::ALL));
    frame.render_widget(para, area);
}

fn draw_status(frame: &mut Frame, app: &ReplApp, area: Rect) {
    let mode = match app.input_mode {
        InputMode::Insert => "INSERT",
        InputMode::Command => "COMMAND",
    };

    let focus = match app.focus {
        FocusPane::Input => "Input",
        FocusPane::Results => "Results",
        FocusPane::Docs => "Docs",
    };

    let text = format!(
        "{mode} | Focus: {focus} | Tab Focus | Ctrl-C Quit | {}",
        app.status.message
    );

    frame.render_widget(Paragraph::new(text), area);
}

fn pane_title<'a>(name: &'a str, focused: bool) -> Line<'a> {
    if focused {
        Line::from(format!("[*] {name}"))
    } else {
        Line::from(format!("[ ] {name}"))
    }
}

// =========================
// main
// =========================

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = ReplApp::default();
    let mut backend = DummyBackend;

    let result = run_app(&mut terminal, &mut app, &mut backend);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut ReplApp,
    backend: &mut dyn ReplBackend,
) -> io::Result<()> {
    while !app.should_quit {
        terminal.draw(|frame| draw(frame, app))?;

        match next_event(Duration::from_millis(100))? {
            AppEvent::Key(key) => handle_key(app, backend, key),
            AppEvent::Tick => {}
        }
    }

    Ok(())
}
