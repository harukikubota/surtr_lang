//! CLI REPL — text-mode entry point.

use std::io::{self, IsTerminal, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[cfg(feature = "line-editor")]
#[cfg(feature = "line-editor")]
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
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

use crate::repl::logic::core::{xldr_version, ReplEngine};
use crate::repl::logic::{present_for_cli, styled, ReplResult};

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
            ":help", ":h", ":quit", ":exit", ":doc", ":sig", ":info", ":type", ":lens", ":error",
            ":save", ":v",
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
pub fn cli_command(options: ReplOptions) -> Result<(), i32> {
    if options.version {
        println!("xldr {}", xldr_version());
        return Ok(());
    }

    if !options.quiet {
        print_banner(options.banner);
    }

    let mut engine = match (&options.module_path, &options.script_path) {
        (Some(module_path), Some(script_path)) => {
            ReplEngine::from_preload_files(Some(module_path), Some(script_path)).map_err(|e| {
                e.emit();
                1
            })?
        }
        (Some(module_path), None) => ReplEngine::from_preload_files(Some(module_path), None)
            .map_err(|e| {
                e.emit();
                1
            })?,
        (None, Some(script_path)) => ReplEngine::from_preload_files(None, Some(script_path))
            .map_err(|e| {
                e.emit();
                1
            })?,
        (None, None) => ReplEngine::new().map_err(|e| {
            eprintln!("Error initializing source loader: {}", e);
            1
        })?,
    };

    if io::stdin().is_terminal() {
        run_terminal_repl(&mut engine)?;
    } else {
        run_plain_repl(&mut engine)?;
    }

    Ok(())
}

#[cfg(feature = "line-editor")]
fn run_terminal_repl(engine: &mut ReplEngine) -> Result<(), i32> {
    struct RawModeGuard;

    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
        }
    }

    enable_raw_mode().map_err(|err| {
        eprintln!("Error enabling raw mode: {}", err);
        1
    })?;
    let _raw_mode_guard = RawModeGuard;

    let color = styled::color_enabled_from_env();
    let mut stdout = io::stdout();
    let mut buffer = String::new();
    let mut cursor_chars = 0usize;
    let mut last_background_progress = Instant::now();

    redraw_terminal_prompt(&mut stdout, &engine.prompt(), &buffer, cursor_chars).map_err(|_| 1)?;

    loop {
        let elapsed = last_background_progress.elapsed();
        last_background_progress = Instant::now();
        let background = engine.advance_background_time(elapsed);
        print_terminal_result(
            &mut stdout,
            engine,
            &background,
            color,
            &buffer,
            cursor_chars,
        )
        .map_err(|_| 1)?;

        let wait = engine
            .next_background_deadline_delay()
            .map(|delay| delay.min(TERMINAL_POLL_QUANTUM))
            .unwrap_or(TERMINAL_POLL_QUANTUM);
        let ready = event::poll(wait).map_err(|err| {
            eprintln!("Error polling input: {}", err);
            1
        })?;

        if !ready {
            continue;
        }

        let Event::Key(key) = event::read().map_err(|err| {
            eprintln!("Error reading input: {}", err);
            1
        })?
        else {
            continue;
        };

        match handle_terminal_key(&mut buffer, &mut cursor_chars, key) {
            TerminalAction::Continue => {
                redraw_terminal_prompt(&mut stdout, &engine.prompt(), &buffer, cursor_chars)
                    .map_err(|_| 1)?;
            }
            TerminalAction::Submit(line) => {
                let submitted = submitted_terminal_line(&engine.prompt(), &line);
                write_crlf(&mut stdout, &submitted).map_err(|_| 1)?;
                let result =
                    with_suspended_raw_mode(|| engine.handle_line(&line)).map_err(|_| 1)?;
                print_terminal_result(&mut stdout, engine, &result, color, &buffer, cursor_chars)
                    .map_err(|_| 1)?;
                last_background_progress = Instant::now();
                if result.should_exit {
                    return Ok(());
                }
            }
            TerminalAction::Exit => {
                write_crlf(&mut stdout, "^C").map_err(|_| 1)?;
                return Ok(());
            }
            TerminalAction::Noop => {}
        }
    }
}

#[cfg(not(feature = "line-editor"))]
fn run_terminal_repl(engine: &mut ReplEngine) -> Result<(), i32> {
    run_plain_repl(engine)
}

fn run_plain_repl(engine: &mut ReplEngine) -> Result<(), i32> {
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
            return Err(1);
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
                    eprintln!("Error reading input: {}", err);
                    return Err(1);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let result = engine.pump_background_to_next_deadline();
                    print_result(&result, styled::color_enabled_from_env());
                    print!("{}", engine.prompt());
                    if io::stdout().flush().is_err() {
                        return Err(1);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
    }
}

#[cfg(feature = "line-editor")]
enum TerminalAction {
    Continue,
    Submit(String),
    Exit,
    Noop,
}

#[cfg(feature = "line-editor")]
fn handle_terminal_key(
    buffer: &mut String,
    cursor_chars: &mut usize,
    key: KeyEvent,
) -> TerminalAction {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => TerminalAction::Exit,
            KeyCode::Char('d') if buffer.is_empty() => TerminalAction::Exit,
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
            if *cursor_chars > 0 {
                let end = byte_index_for_char_position(buffer, *cursor_chars);
                let start = byte_index_for_char_position(buffer, *cursor_chars - 1);
                buffer.replace_range(start..end, "");
                *cursor_chars -= 1;
            }
            TerminalAction::Continue
        }
        KeyCode::Delete => {
            if *cursor_chars < buffer.chars().count() {
                let start = byte_index_for_char_position(buffer, *cursor_chars);
                let end = byte_index_for_char_position(buffer, *cursor_chars + 1);
                buffer.replace_range(start..end, "");
            }
            TerminalAction::Continue
        }
        KeyCode::Left => {
            *cursor_chars = cursor_chars.saturating_sub(1);
            TerminalAction::Continue
        }
        KeyCode::Right => {
            *cursor_chars = (*cursor_chars + 1).min(buffer.chars().count());
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
    let mut lines = present_for_cli(result, color);
    lines.extend(result.stderr.iter().cloned());
    if lines.is_empty() {
        redraw_terminal_prompt(stdout, &engine.prompt(), buffer, cursor_chars)?;
        return Ok(());
    }

    for line in lines {
        write!(stdout, "\r\x1b[L\r\x1b[2K{line}\r\n")?;
    }
    stdout.flush()?;
    redraw_terminal_prompt(stdout, &engine.prompt(), buffer, cursor_chars)
}

#[cfg(feature = "line-editor")]
fn redraw_terminal_prompt(
    stdout: &mut io::Stdout,
    prompt: &str,
    buffer: &str,
    cursor_chars: usize,
) -> io::Result<()> {
    let column = prompt.chars().count().saturating_add(cursor_chars) as u16;
    write!(stdout, "\r\x1b[2K{prompt}{buffer}\r")?;
    if column > 0 {
        write!(stdout, "\x1b[{}C", column)?;
    }
    stdout.flush()
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

#[cfg(all(test, feature = "line-editor"))]
mod tests {
    #[test]
    fn submitted_terminal_line_keeps_prompt_and_input_together() {
        assert_eq!(
            super::submitted_terminal_line("xldr(1)> ", "1"),
            "xldr(1)> 1"
        );
    }
}
