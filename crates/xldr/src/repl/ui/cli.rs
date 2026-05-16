//! CLI REPL — text-mode entry point.

use std::io::{self, IsTerminal, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[cfg(feature = "line-editor")]
#[cfg(feature = "line-editor")]
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
#[cfg(feature = "line-editor")]
use crossterm::terminal::size;
#[cfg(feature = "line-editor")]
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
#[cfg(feature = "line-editor")]
use rustyline::completion::{Completer, Pair};
#[cfg(feature = "line-editor")]
use rustyline::error::ReadlineError;
#[cfg(feature = "line-editor")]
use rustyline::highlight::Highlighter;
#[cfg(feature = "line-editor")]
use rustyline::hint::{Hinter, HistoryHinter};
#[cfg(feature = "line-editor")]
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
#[cfg(feature = "line-editor")]
use rustyline::{Context, Helper};

use crate::repl::logic::core::{
    xldr_version, ReplCompletion, ReplCompletionCandidate, ReplCompletionKind, ReplEngine,
};
use crate::repl::logic::{present_for_cli, styled, ReplResult};
use crate::{CommandError, CommandResult};

#[cfg(feature = "line-editor")]
const TERMINAL_POLL_QUANTUM: Duration = Duration::from_millis(25);

// ── Options ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerMode {
    Light,
    Detailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplOptions {
    pub quiet: bool,
    pub banner: BannerMode,
    pub version: bool,
    pub script_path: Option<String>,
    pub module_path: Option<String>,
}

impl Default for ReplOptions {
    fn default() -> Self {
        Self {
            quiet: false,
            banner: BannerMode::Light,
            version: false,
            script_path: None,
            module_path: None,
        }
    }
}

// ── Line editor helper ────────────────────────────────────────────────────────

#[cfg(feature = "line-editor")]
#[allow(dead_code)]
struct ReplHelper {
    hinter: HistoryHinter,
    symbols: Vec<String>,
}

#[cfg(feature = "line-editor")]
#[allow(dead_code)]
impl ReplHelper {
    fn new() -> Self {
        Self {
            hinter: HistoryHinter {},
            symbols: Vec::new(),
        }
    }

    fn set_symbols(&mut self, symbols: Vec<String>) {
        self.symbols = symbols;
    }
}

#[cfg(feature = "line-editor")]
impl Helper for ReplHelper {}
#[cfg(feature = "line-editor")]
impl Highlighter for ReplHelper {}

#[cfg(feature = "line-editor")]
impl Validator for ReplHelper {
    fn validate(
        &self,
        _ctx: &mut ValidationContext<'_>,
    ) -> Result<ValidationResult, ReadlineError> {
        Ok(ValidationResult::Valid(None))
    }
}

#[cfg(feature = "line-editor")]
impl Hinter for ReplHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<Self::Hint> {
        self.hinter.hint(line, pos, ctx)
    }
}

#[cfg(feature = "line-editor")]
impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        const COMMANDS: &[&str] = &[
            ":help",
            ":h",
            ":quit",
            ":exit",
            ":doc",
            ":sig",
            ":info",
            ":type",
            ":facet",
            ":error",
            ":save",
            ":vars",
            ":imported",
            ":defs",
            ":history",
            ":reload",
            ":clear",
            ":v",
        ];
        let start = line[..pos]
            .rfind(char::is_whitespace)
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let word = &line[start..pos];

        let mut matches = Vec::new();
        for cmd in COMMANDS {
            if cmd.starts_with(word) {
                matches.push(Pair {
                    display: cmd.to_string(),
                    replacement: cmd.to_string(),
                });
            }
        }
        for symbol in &self.symbols {
            if symbol.starts_with(word) {
                matches.push(Pair {
                    display: symbol.clone(),
                    replacement: symbol.clone(),
                });
            }
        }
        Ok((start, matches))
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Text-mode REPL entry point.
pub fn cli_command(options: ReplOptions) -> CommandResult<()> {
    if options.version {
        println!("xldr {}", xldr_version());
        return Ok(());
    }

    if !options.quiet {
        print_banner(options.banner);
    }

    let mut engine =
        match (&options.module_path, &options.script_path) {
            (Some(module_path), Some(script_path)) => {
                ReplEngine::from_preload_files(Some(module_path), Some(script_path))
                    .map_err(CommandError::from)?
            }
            (Some(module_path), None) => ReplEngine::from_preload_files(Some(module_path), None)
                .map_err(CommandError::from)?,
            (None, Some(script_path)) => ReplEngine::from_preload_files(None, Some(script_path))
                .map_err(CommandError::from)?,
            (None, None) => ReplEngine::new().map_err(|e| {
                CommandError::message(1, format!("Error initializing source loader: {}", e))
            })?,
        };

    let color = styled::color_enabled_from_env();
    for result in engine.take_startup_results() {
        print_result(&result, color);
    }

    if io::stdin().is_terminal() {
        run_terminal_repl(&mut engine)?;
    } else {
        run_plain_repl(&mut engine)?;
    }

    Ok(())
}

#[cfg(feature = "line-editor")]
fn run_terminal_repl(engine: &mut ReplEngine) -> CommandResult<()> {
    struct RawModeGuard;

    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
        }
    }

    enable_raw_mode()
        .map_err(|err| CommandError::message(1, format!("Error enabling raw mode: {}", err)))?;
    let _raw_mode_guard = RawModeGuard;

    let color = styled::color_enabled_from_env();
    let mut stdout = io::stdout();
    let mut buffer = String::new();
    let mut cursor_chars = 0usize;
    let mut history = TerminalHistory::default();
    let mut last_background_progress = Instant::now();

    redraw_terminal_prompt(&mut stdout, engine, &buffer, cursor_chars)
        .map_err(|_| CommandError::message(1, "repl: failed to redraw prompt"))?;

    loop {
        let elapsed = last_background_progress.elapsed();
        last_background_progress = Instant::now();
        let background = engine.advance_background_time(elapsed);
        if repl_result_has_visible_output(&background, color) {
            print_terminal_result(
                &mut stdout,
                engine,
                &background,
                color,
                &buffer,
                cursor_chars,
            )
            .map_err(|_| CommandError::message(1, "repl: failed to print terminal result"))?;
        }

        let wait = engine
            .next_background_deadline_delay()
            .map(|delay| delay.min(TERMINAL_POLL_QUANTUM))
            .unwrap_or(TERMINAL_POLL_QUANTUM);
        let ready = event::poll(wait)
            .map_err(|err| CommandError::message(1, format!("Error polling input: {}", err)))?;

        if !ready {
            continue;
        }

        let Event::Key(key) = event::read()
            .map_err(|err| CommandError::message(1, format!("Error reading input: {}", err)))?
        else {
            continue;
        };

        let before_buffer = buffer.clone();
        let before_cursor = cursor_chars;
        match handle_terminal_key(&mut history, &mut buffer, &mut cursor_chars, key) {
            TerminalAction::Continue => {
                if buffer != before_buffer || cursor_chars != before_cursor {
                    redraw_terminal_prompt(&mut stdout, engine, &buffer, cursor_chars)
                        .map_err(|_| CommandError::message(1, "repl: failed to redraw prompt"))?;
                }
            }
            TerminalAction::Submit(line) => {
                history.record(&line);
                let submitted = submitted_terminal_line(&engine.prompt(), &line);
                write_crlf(&mut stdout, &submitted)
                    .map_err(|_| CommandError::message(1, "repl: failed to write input line"))?;
                let result = with_suspended_raw_mode(|| engine.handle_line(&line))
                    .map_err(|_| CommandError::message(1, "repl: failed to suspend raw mode"))?;
                if repl_result_has_visible_output(&result, color) {
                    print_terminal_result(&mut stdout, engine, &result, color, &buffer, cursor_chars)
                        .map_err(|_| CommandError::message(1, "repl: failed to print terminal result"))?;
                } else {
                    redraw_terminal_prompt(&mut stdout, engine, &buffer, cursor_chars)
                        .map_err(|_| CommandError::message(1, "repl: failed to redraw prompt"))?;
                }
                last_background_progress = Instant::now();
                if result.should_exit {
                    return Ok(());
                }
            }
            TerminalAction::Exit => {
                write_crlf(&mut stdout, "^C").map_err(|_| {
                    CommandError::message(1, "repl: failed to write interrupt line")
                })?;
                return Ok(());
            }
            TerminalAction::Noop => {}
        }
    }
}

#[cfg(not(feature = "line-editor"))]
fn run_terminal_repl(engine: &mut ReplEngine) -> CommandResult<()> {
    run_plain_repl(engine)
}

fn run_plain_repl(engine: &mut ReplEngine) -> CommandResult<()> {
    let (tx, rx) = mpsc::channel::<Result<String, io::Error>>();
    std::thread::spawn(move || {
        let stdin = io::stdin();
        loop {
            let mut line = String::new();
            match stdin.read_line(&mut line) {
                Ok(0) => {
                    let _ = tx.send(Ok(String::new()));
                    break;
                }
                Ok(_) => {
                    let line = line.trim_end_matches(&['\r', '\n'][..]).to_string();
                    if tx.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    let _ = tx.send(Err(err));
                    break;
                }
            }
        }
    });

    loop {
        let background = engine.pump_background_ready();
        print_result(&background, styled::color_enabled_from_env());
        print!("{}", engine.prompt());
        if io::stdout().flush().is_err() {
            return Err(CommandError::message(1, "repl: failed to flush prompt"));
        }

        loop {
            let received = match engine.next_background_deadline_delay() {
                Some(delay) => rx.recv_timeout(delay),
                None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(Ok(line)) => {
                    if line.is_empty() {
                        return Ok(());
                    }
                    let result = engine.handle_line(&line);
                    print_result(&result, styled::color_enabled_from_env());
                    if result.should_exit {
                        return Ok(());
                    }
                    break;
                }
                Ok(Err(err)) => {
                    return Err(CommandError::message(
                        1,
                        format!("Error reading input: {}", err),
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let result = engine.pump_background_to_next_deadline();
                    print_result(&result, styled::color_enabled_from_env());
                    print!("{}", engine.prompt());
                    if io::stdout().flush().is_err() {
                        return Err(CommandError::message(1, "repl: failed to flush prompt"));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
    }
}

#[cfg(feature = "line-editor")]
#[derive(Debug, Clone, PartialEq, Eq)]
enum TerminalAction {
    Continue,
    Submit(String),
    Exit,
    Noop,
}

#[cfg(feature = "line-editor")]
#[derive(Debug, Default)]
struct TerminalHistory {
    entries: Vec<String>,
    position: Option<usize>,
    draft: Option<String>,
}

#[cfg(feature = "line-editor")]
impl TerminalHistory {
    fn record(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        if self.entries.last().is_some_and(|last| last == line) {
            self.position = None;
            self.draft = None;
            return;
        }
        self.entries.push(line.to_string());
        self.position = None;
        self.draft = None;
    }

    fn move_prev(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }

        let next_position = match self.position {
            Some(0) => 0,
            Some(position) => position.saturating_sub(1),
            None => {
                self.draft = Some(current.to_string());
                self.entries.len().saturating_sub(1)
            }
        };
        self.position = Some(next_position);
        self.entries.get(next_position).cloned()
    }

    fn move_next(&mut self) -> Option<String> {
        let position = self.position?;
        if position + 1 < self.entries.len() {
            let next_position = position + 1;
            self.position = Some(next_position);
            return self.entries.get(next_position).cloned();
        }

        self.position = None;
        Some(self.draft.take().unwrap_or_default())
    }
}

#[cfg(feature = "line-editor")]
fn move_cursor_left(cursor_chars: &mut usize) {
    *cursor_chars = cursor_chars.saturating_sub(1);
}

#[cfg(feature = "line-editor")]
fn move_cursor_right(buffer: &str, cursor_chars: &mut usize) {
    *cursor_chars = (*cursor_chars + 1).min(buffer.chars().count());
}

#[cfg(feature = "line-editor")]
fn delete_left(buffer: &mut String, cursor_chars: &mut usize) {
    if *cursor_chars > 0 {
        let end = byte_index_for_char_position(buffer, *cursor_chars);
        let start = byte_index_for_char_position(buffer, *cursor_chars - 1);
        buffer.replace_range(start..end, "");
        *cursor_chars -= 1;
    }
}

#[cfg(feature = "line-editor")]
fn delete_right(buffer: &mut String, cursor_chars: usize) {
    if cursor_chars < buffer.chars().count() {
        let start = byte_index_for_char_position(buffer, cursor_chars);
        let end = byte_index_for_char_position(buffer, cursor_chars + 1);
        buffer.replace_range(start..end, "");
    }
}

#[cfg(feature = "line-editor")]
fn handle_terminal_key(
    history: &mut TerminalHistory,
    buffer: &mut String,
    cursor_chars: &mut usize,
    key: KeyEvent,
) -> TerminalAction {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => TerminalAction::Exit,
            KeyCode::Char('d') if buffer.is_empty() => TerminalAction::Exit,
            KeyCode::Char('a') => {
                *cursor_chars = 0;
                TerminalAction::Continue
            }
            KeyCode::Char('b') => {
                move_cursor_left(cursor_chars);
                TerminalAction::Continue
            }
            KeyCode::Char('d') => {
                delete_right(buffer, *cursor_chars);
                TerminalAction::Continue
            }
            KeyCode::Char('e') => {
                *cursor_chars = buffer.chars().count();
                TerminalAction::Continue
            }
            KeyCode::Char('f') => {
                move_cursor_right(buffer, cursor_chars);
                TerminalAction::Continue
            }
            KeyCode::Char('h') => {
                delete_left(buffer, cursor_chars);
                TerminalAction::Continue
            }
            KeyCode::Char('n') => {
                if let Some(next) = history.move_next() {
                    *buffer = next;
                    *cursor_chars = buffer.chars().count();
                    TerminalAction::Continue
                } else {
                    TerminalAction::Noop
                }
            }
            KeyCode::Char('p') => {
                if let Some(previous) = history.move_prev(buffer) {
                    *buffer = previous;
                    *cursor_chars = buffer.chars().count();
                    TerminalAction::Continue
                } else {
                    TerminalAction::Noop
                }
            }
            _ => TerminalAction::Noop,
        };
    }

    match key.code {
        KeyCode::Enter => {
            let line = std::mem::take(buffer);
            *cursor_chars = 0;
            TerminalAction::Submit(line)
        }
        KeyCode::Char(ch) => {
            let idx = byte_index_for_char_position(buffer, *cursor_chars);
            buffer.insert(idx, ch);
            *cursor_chars += 1;
            TerminalAction::Continue
        }
        KeyCode::Backspace => {
            delete_left(buffer, cursor_chars);
            TerminalAction::Continue
        }
        KeyCode::Delete => {
            delete_right(buffer, *cursor_chars);
            TerminalAction::Continue
        }
        KeyCode::Left => {
            move_cursor_left(cursor_chars);
            TerminalAction::Continue
        }
        KeyCode::Right => {
            move_cursor_right(buffer, cursor_chars);
            TerminalAction::Continue
        }
        KeyCode::Home => {
            *cursor_chars = 0;
            TerminalAction::Continue
        }
        KeyCode::End => {
            *cursor_chars = buffer.chars().count();
            TerminalAction::Continue
        }
        KeyCode::Up => {
            if let Some(previous) = history.move_prev(buffer) {
                *buffer = previous;
                *cursor_chars = buffer.chars().count();
                TerminalAction::Continue
            } else {
                TerminalAction::Noop
            }
        }
        KeyCode::Down => {
            if let Some(next) = history.move_next() {
                *buffer = next;
                *cursor_chars = buffer.chars().count();
                TerminalAction::Continue
            } else {
                TerminalAction::Noop
            }
        }
        _ => TerminalAction::Noop,
    }
}

#[cfg(feature = "line-editor")]
fn print_terminal_result(
    stdout: &mut io::Stdout,
    engine: &ReplEngine,
    result: &ReplResult,
    color: bool,
    buffer: &str,
    cursor_chars: usize,
) -> io::Result<()> {
    let lines = repl_result_lines(result, color);
    for line in lines {
        write!(stdout, "\r\x1b[L\r\x1b[2K{line}\r\n")?;
    }
    stdout.flush()?;
    redraw_terminal_prompt(stdout, engine, buffer, cursor_chars)
}

#[cfg(feature = "line-editor")]
fn repl_result_has_visible_output(result: &ReplResult, color: bool) -> bool {
    !repl_result_lines(result, color).is_empty()
}

#[cfg(feature = "line-editor")]
fn repl_result_lines(result: &ReplResult, color: bool) -> Vec<String> {
    let mut lines = present_for_cli(result, color);
    lines.extend(result.stderr.iter().cloned());
    lines
}

#[cfg(feature = "line-editor")]
fn redraw_terminal_prompt(
    stdout: &mut io::Stdout,
    engine: &ReplEngine,
    buffer: &str,
    cursor_chars: usize,
) -> io::Result<()> {
    let color = styled::color_enabled_from_env();
    let prompt = engine.prompt();
    let column = prompt.chars().count().saturating_add(cursor_chars) as u16;
    let cursor_byte = byte_index_for_char_position(buffer, cursor_chars);
    let completion = engine.completions(buffer, cursor_byte);
    let completion_lines = render_completion_lines(&completion, color);
    let completion_rows = completion_lines
        .iter()
        .map(|line| terminal_rows_for_line(line))
        .sum::<usize>();
    write!(stdout, "\r\x1b[J{prompt}{buffer}")?;
    for line in &completion_lines {
        write!(stdout, "\r\n\x1b[2K{line}")?;
    }
    if completion_rows > 0 {
        write!(stdout, "\x1b[{}A", completion_rows)?;
    }
    write!(stdout, "\r")?;
    if column > 0 {
        write!(stdout, "\x1b[{}C", column)?;
    }
    stdout.flush()
}

#[cfg(feature = "line-editor")]
fn terminal_rows_for_line(line: &str) -> usize {
    let width = terminal_width();
    let columns = visible_line_width(line).max(1);
    columns.div_ceil(width)
}

#[cfg(feature = "line-editor")]
fn terminal_width() -> usize {
    size()
        .map(|(width, _)| width as usize)
        .ok()
        .filter(|width| *width > 0)
        .unwrap_or(80)
}

#[cfg(feature = "line-editor")]
fn render_completion_lines(completion: &ReplCompletion, color: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(signature) = &completion.signature {
        lines.extend(signature.lines.iter().map(|line| {
            if color {
                styled::completion_signature_line(line)
            } else {
                format!("sig  {line}")
            }
        }));
    }
    lines.extend(
        completion
            .candidates
            .iter()
            .map(|candidate| render_completion_candidate(candidate, color)),
    );
    lines
}

#[cfg(feature = "line-editor")]
fn render_completion_candidate(candidate: &ReplCompletionCandidate, color: bool) -> String {
    let kind = match candidate.kind {
        ReplCompletionKind::Variable => "var ",
        ReplCompletionKind::TypeConstructor => "type",
        ReplCompletionKind::TypePath => "path",
        ReplCompletionKind::FunctionCall => "call",
    };
    if color {
        styled::completion_candidate_line(kind, &candidate.label, candidate.detail.as_deref())
    } else {
        match &candidate.detail {
            Some(detail) => format!("{kind} {:<18} {detail}", candidate.label),
            None => format!("{kind} {}", candidate.label),
        }
    }
}

#[cfg(feature = "line-editor")]
fn visible_line_width(line: &str) -> usize {
    strip_ansi(line).chars().count()
}

#[cfg(feature = "line-editor")]
fn strip_ansi(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && matches!(chars.peek(), Some('[')) {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }

    out
}

#[cfg(feature = "line-editor")]
fn byte_index_for_char_position(text: &str, char_pos: usize) -> usize {
    if char_pos == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_pos)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

#[cfg(feature = "line-editor")]
fn submitted_terminal_line(prompt: &str, line: &str) -> String {
    format!("{prompt}{line}")
}

#[cfg(feature = "line-editor")]
fn write_crlf(stdout: &mut io::Stdout, line: &str) -> io::Result<()> {
    write!(stdout, "\r\x1b[2K{line}\r\n")?;
    stdout.flush()
}

#[cfg(feature = "line-editor")]
fn with_suspended_raw_mode<T>(f: impl FnOnce() -> T) -> io::Result<T> {
    disable_raw_mode()?;
    let result = f();
    enable_raw_mode()?;
    Ok(result)
}

fn print_banner(mode: BannerMode) {
    match mode {
        BannerMode::Light => {
            println!("Surtr xldr {}", xldr_version());
        }
        BannerMode::Detailed => {
            println!(
                r"
    ██\   ██\ ██\      ██████\  ██████\
    ╚██\ ██  |██ |     ██  __██\ ██  __██\
     ╚████  / ██ |     ██ /  ██ |██ |  ██ |
     ██  ██<  ██ |     ██ |  ██ |██████  |
    ██  /\██\ ██ |     ██ |  ██ |██  __██<
    ██ /  ██ |███████\ ██████  |██ |  ██ |
    \__|  \__|\_______|\______/ \__|  \__|

    "
            );
        }
    }
}

fn print_result(result: &ReplResult, color: bool) {
    for line in present_for_cli(result, color) {
        println!("{line}");
    }
    for line in &result.stderr {
        eprintln!("{line}");
    }
}

#[cfg(test)]
mod command_error_tests {
    use super::ReplOptions;

    #[test]
    fn preload_diagnostic_builds_typed_command_error() {
        let error = crate::cli_command(ReplOptions {
            quiet: true,
            banner: super::BannerMode::Light,
            version: false,
            script_path: Some("/definitely/missing-script.srt".to_string()),
            module_path: None,
        })
        .expect_err("missing preload script must fail");

        assert_eq!(error.exit_code(), 1);
        assert!(matches!(error, crate::CommandError::Message { .. }));
    }
}

#[cfg(all(test, feature = "line-editor"))]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::repl::logic::core::{
        ReplCompletion, ReplCompletionCandidate, ReplCompletionKind, ReplSignatureHelp,
    };

    #[test]
    fn submitted_terminal_line_keeps_prompt_and_input_together() {
        assert_eq!(
            super::submitted_terminal_line("xldr(1)> ", "1"),
            "xldr(1)> 1"
        );
    }

    #[test]
    fn terminal_history_walks_back_and_forward_and_restores_draft() {
        let mut history = super::TerminalHistory::default();
        history.record("first");
        history.record("second");

        let mut buffer = "draft".to_string();
        let mut cursor_chars = buffer.chars().count();

        assert_eq!(
            super::handle_terminal_key(
                &mut history,
                &mut buffer,
                &mut cursor_chars,
                KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            ),
            super::TerminalAction::Continue
        );
        assert_eq!(buffer, "second");
        assert_eq!(cursor_chars, "second".chars().count());

        assert_eq!(
            super::handle_terminal_key(
                &mut history,
                &mut buffer,
                &mut cursor_chars,
                KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            ),
            super::TerminalAction::Continue
        );
        assert_eq!(buffer, "first");

        assert_eq!(
            super::handle_terminal_key(
                &mut history,
                &mut buffer,
                &mut cursor_chars,
                KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            ),
            super::TerminalAction::Continue
        );
        assert_eq!(buffer, "second");

        assert_eq!(
            super::handle_terminal_key(
                &mut history,
                &mut buffer,
                &mut cursor_chars,
                KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            ),
            super::TerminalAction::Continue
        );
        assert_eq!(buffer, "draft");
        assert_eq!(cursor_chars, "draft".chars().count());
    }

    #[test]
    fn terminal_history_ignores_empty_and_adjacent_duplicates() {
        let mut history = super::TerminalHistory::default();
        history.record("");
        history.record("repeat");
        history.record("repeat");
        history.record("next");

        assert_eq!(
            history.entries,
            vec!["repeat".to_string(), "next".to_string()]
        );
    }

    #[test]
    fn terminal_history_recall_places_cursor_at_end_of_line() {
        let mut history = super::TerminalHistory::default();
        history.record("value");

        let mut buffer = String::new();
        let mut cursor_chars = 0usize;

        let action = super::handle_terminal_key(
            &mut history,
            &mut buffer,
            &mut cursor_chars,
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        );

        assert_eq!(action, super::TerminalAction::Continue);
        assert_eq!(buffer, "value");
        assert_eq!(cursor_chars, "value".chars().count());
    }

    #[test]
    fn terminal_control_navigation_matches_arrow_navigation() {
        let mut history = super::TerminalHistory::default();
        history.record("first");
        history.record("second");

        let mut buffer = "abcd".to_string();
        let mut cursor_chars = 2usize;

        let action = super::handle_terminal_key(
            &mut history,
            &mut buffer,
            &mut cursor_chars,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
        );
        assert_eq!(action, super::TerminalAction::Continue);
        assert_eq!(cursor_chars, 1);

        let action = super::handle_terminal_key(
            &mut history,
            &mut buffer,
            &mut cursor_chars,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        );
        assert_eq!(action, super::TerminalAction::Continue);
        assert_eq!(cursor_chars, 2);

        let action = super::handle_terminal_key(
            &mut history,
            &mut buffer,
            &mut cursor_chars,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        );
        assert_eq!(action, super::TerminalAction::Continue);
        assert_eq!(cursor_chars, 0);

        let action = super::handle_terminal_key(
            &mut history,
            &mut buffer,
            &mut cursor_chars,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
        );
        assert_eq!(action, super::TerminalAction::Continue);
        assert_eq!(cursor_chars, buffer.chars().count());

        let action = super::handle_terminal_key(
            &mut history,
            &mut buffer,
            &mut cursor_chars,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
        );
        assert_eq!(action, super::TerminalAction::Continue);
        assert_eq!(buffer, "second");

        let action = super::handle_terminal_key(
            &mut history,
            &mut buffer,
            &mut cursor_chars,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
        );
        assert_eq!(action, super::TerminalAction::Continue);
        assert_eq!(buffer, "abcd");
    }

    #[test]
    fn terminal_control_delete_matches_backspace_and_delete() {
        let mut history = super::TerminalHistory::default();
        let mut buffer = "abcd".to_string();
        let mut cursor_chars = 2usize;

        let action = super::handle_terminal_key(
            &mut history,
            &mut buffer,
            &mut cursor_chars,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL),
        );
        assert_eq!(action, super::TerminalAction::Continue);
        assert_eq!(buffer, "acd");
        assert_eq!(cursor_chars, 1);

        let action = super::handle_terminal_key(
            &mut history,
            &mut buffer,
            &mut cursor_chars,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        );
        assert_eq!(action, super::TerminalAction::Continue);
        assert_eq!(buffer, "ad");
        assert_eq!(cursor_chars, 1);
    }

    #[test]
    fn render_completion_lines_stays_plain_when_color_is_off() {
        let completion = ReplCompletion {
            signature: Some(ReplSignatureHelp {
                lines: vec!["Kernel::print(a: [String]) -> Unit".to_string()],
                active_parameter: Some(0),
            }),
            candidates: vec![ReplCompletionCandidate {
                label: "print".to_string(),
                replacement: "print".to_string(),
                kind: ReplCompletionKind::FunctionCall,
                detail: Some("Kernel::print(a: [String]) -> Unit".to_string()),
                replace_start: 0,
                replace_end: 0,
            }],
        };

        let rendered = super::render_completion_lines(&completion, false);
        assert_eq!(
            rendered,
            vec![
                "sig  Kernel::print(a: [String]) -> Unit".to_string(),
                "call print              Kernel::print(a: [String]) -> Unit".to_string(),
            ]
        );
    }

    #[test]
    fn render_completion_lines_adds_ansi_when_color_is_on() {
        let completion = ReplCompletion {
            signature: Some(ReplSignatureHelp {
                lines: vec!["Kernel::print(a: [String]) -> Unit".to_string()],
                active_parameter: Some(0),
            }),
            candidates: vec![
                ReplCompletionCandidate {
                    label: "name".to_string(),
                    replacement: "name".to_string(),
                    kind: ReplCompletionKind::Variable,
                    detail: Some("String".to_string()),
                    replace_start: 0,
                    replace_end: 0,
                },
                ReplCompletionCandidate {
                    label: "print".to_string(),
                    replacement: "print".to_string(),
                    kind: ReplCompletionKind::FunctionCall,
                    detail: Some("Kernel::print(a: [String]) -> Unit".to_string()),
                    replace_start: 0,
                    replace_end: 0,
                },
            ],
        };

        let rendered = super::render_completion_lines(&completion, true);
        assert!(rendered[0].contains("\x1b["), "{rendered:?}");
        assert!(rendered[1].contains("\x1b[1;90mvar \x1b[0m"), "{rendered:?}");
        assert!(rendered[1].contains("\x1b[36mname"), "{rendered:?}");
        assert!(rendered[2].contains("\x1b[1;90mcall\x1b[0m"), "{rendered:?}");
        assert!(rendered[2].contains("\x1b[1;35mprint"), "{rendered:?}");
    }
}
