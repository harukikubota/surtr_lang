//! TUI application state types.
use std::collections::VecDeque;

// ── Enums ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FocusPane {
    Input,
    Results,
    Docs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputMode {
    Insert,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResultEntryKind {
    EvalSuccess,
    EvalError,
    CommandOutput,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplMode {
    Repl,
}

impl ReplMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ReplMode::Repl => "repl",
        }
    }
}

// ── Data types ────────────────────────────────────────────────────────────────

/// One evaluation unit in the results pane.
/// `source` is preserved so `:v idx` can restore it to the input buffer.
#[derive(Debug, Clone)]
pub(super) struct ResultEntry {
    pub(super) idx: usize,
    pub(super) source: String,
    pub(super) rendered_lines: Vec<String>,
    pub(super) kind: ResultEntryKind,
}

#[derive(Debug, Clone)]
pub(super) struct DocEntry {
    pub(super) idx: usize,
    pub(super) symbol: String,
    pub(super) signature: Option<String>,
    pub(super) summary: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct CompletionItem {
    pub(super) label: String,
    pub(super) detail: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub(super) struct Completion {
    pub(super) visible: bool,
    pub(super) selected: usize,
    pub(super) items: Vec<CompletionItem>,
}

impl Completion {
    pub(super) fn clear(&mut self) {
        self.visible = false;
        self.selected = 0;
        self.items.clear();
    }

    pub(super) fn select_prev(&mut self) {
        if !self.items.is_empty() {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    pub(super) fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1).min(self.items.len() - 1);
        }
    }

    pub(super) fn apply(&self, buf: &mut InputBuffer) {
        if let Some(item) = self.items.get(self.selected) {
            let prefix_len = super::update::current_token_prefix(buf).len();
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
pub(super) struct InputBuffer {
    pub(super) text: String,
    pub(super) cursor_byte: usize,
}

impl InputBuffer {
    pub(super) fn insert_char(&mut self, ch: char) {
        self.text.insert(self.cursor_byte, ch);
        self.cursor_byte += ch.len_utf8();
    }

    pub(super) fn backspace(&mut self) {
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

    pub(super) fn move_left(&mut self) {
        if self.cursor_byte == 0 {
            return;
        }
        self.cursor_byte = self.text[..self.cursor_byte]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
    }

    pub(super) fn move_right(&mut self) {
        if self.cursor_byte < self.text.len() {
            if let Some(ch) = self.text[self.cursor_byte..].chars().next() {
                self.cursor_byte += ch.len_utf8();
            }
        }
    }

    pub(super) fn clear(&mut self) {
        self.text.clear();
        self.cursor_byte = 0;
    }

    pub(super) fn set(&mut self, text: String) {
        self.cursor_byte = text.len();
        self.text = text;
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

pub(super) struct App {
    pub(super) should_quit: bool,
    pub(super) focus: FocusPane,
    pub(super) input_mode: InputMode,
    pub(super) input: InputBuffer,
    pub(super) command: InputBuffer,
    pub(super) results: VecDeque<ResultEntry>,
    pub(super) results_scroll: usize,
    pub(super) selected_result: Option<usize>,
    pub(super) docs: VecDeque<DocEntry>,
    pub(super) selected_doc: Option<usize>,
    pub(super) completion: Completion,
    pub(super) status: String,
    pub(super) mode: ReplMode,
}

impl App {
    pub(super) fn new() -> Self {
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
            mode: ReplMode::Repl,
        }
    }

    pub(super) fn push_result(
        &mut self,
        source: impl Into<String>,
        rendered_lines: Vec<String>,
        kind: ResultEntryKind,
    ) {
        let idx = self.results.len() + 1;
        self.results.push_back(ResultEntry {
            idx,
            source: source.into(),
            rendered_lines,
            kind,
        });
        // Keep scroll at bottom when new output arrives (line-based).
        // Set to total so the widget clamps to (total - viewport_height) correctly.
        let total: usize = self.results.iter().map(|e| 3 + e.rendered_lines.len()).sum();
        self.results_scroll = total;
    }

    pub(super) fn active_buf(&self) -> &InputBuffer {
        match self.input_mode {
            InputMode::Insert => &self.input,
            InputMode::Command => &self.command,
        }
    }

    pub(super) fn active_buf_mut(&mut self) -> &mut InputBuffer {
        match self.input_mode {
            InputMode::Insert => &mut self.input,
            InputMode::Command => &mut self.command,
        }
    }

    pub(super) fn next_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Input => FocusPane::Results,
            FocusPane::Results => FocusPane::Docs,
            FocusPane::Docs => FocusPane::Input,
        };
        self.update_status();
    }

    pub(super) fn prev_focus(&mut self) {
        self.focus = match self.focus {
            FocusPane::Input => FocusPane::Docs,
            FocusPane::Results => FocusPane::Input,
            FocusPane::Docs => FocusPane::Results,
        };
        self.update_status();
    }

    pub(super) fn update_status(&mut self) {
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
