//! CLI REPL — text-mode entry point.

use std::io::{self, IsTerminal, Write};

#[cfg(feature = "line-editor")]
use rustyline::completion::{Completer, Pair};
#[cfg(feature = "line-editor")]
use rustyline::error::ReadlineError;
#[cfg(feature = "line-editor")]
use rustyline::highlight::Highlighter;
#[cfg(feature = "line-editor")]
use rustyline::hint::{Hinter, HistoryHinter};
#[cfg(feature = "line-editor")]
use rustyline::history::DefaultHistory;
#[cfg(feature = "line-editor")]
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
#[cfg(feature = "line-editor")]
use rustyline::{Context, Editor, Helper};

use crate::repl::logic::core::{xldr_version, ReplEngine};
use crate::repl::logic::{present_for_cli, styled, ReplResult};

// ── Options ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerMode {
    Light,
    Detailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplOptions {
    pub quiet: bool,
    pub banner: BannerMode,
    pub version: bool,
}

impl Default for ReplOptions {
    fn default() -> Self {
        Self {
            quiet: false,
            banner: BannerMode::Light,
            version: false,
        }
    }
}

// ── Line editor helper ────────────────────────────────────────────────────────

#[cfg(feature = "line-editor")]
struct ReplHelper {
    hinter: HistoryHinter,
    symbols: Vec<String>,
}

#[cfg(feature = "line-editor")]
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
            ":help", ":h", ":quit", ":exit", ":doc", ":sig", ":info", ":type", ":error", ":save",
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
pub fn cli_command(options: ReplOptions) -> Result<(), i32> {
    if options.version {
        println!("xldr {}", xldr_version());
        return Ok(());
    }

    if !options.quiet {
        print_banner(options.banner);
    }

    let mut engine = ReplEngine::new().map_err(|e| {
        eprintln!("Error initializing source loader: {}", e);
        1
    })?;

    if io::stdin().is_terminal() {
        run_terminal_repl(&mut engine)?;
    } else {
        run_plain_repl(&mut engine)?;
    }

    Ok(())
}

#[cfg(feature = "line-editor")]
fn run_terminal_repl(engine: &mut ReplEngine) -> Result<(), i32> {
    let mut editor: Editor<ReplHelper, DefaultHistory> = Editor::new().map_err(|e| {
        eprintln!("Error initializing line editor: {}", e);
        1
    })?;
    editor.set_helper(Some(ReplHelper::new()));

    loop {
        let prompt = engine.prompt();
        let symbols = engine.completion_symbols();
        if let Some(helper) = editor.helper_mut() {
            helper.set_symbols(symbols);
        }
        match editor.readline(&prompt) {
            Ok(line) => {
                if !line.trim().is_empty() {
                    let _ = editor.add_history_entry(line.as_str());
                }
                let result = engine.handle_line(&line);
                print_result(&result, styled::color_enabled_from_env());
                if result.should_exit {
                    break;
                }
            }
            Err(ReadlineError::Interrupted) => {
                return Err(130);
            }
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                return Err(1);
            }
        }
    }

    Ok(())
}

#[cfg(not(feature = "line-editor"))]
fn run_terminal_repl(engine: &mut ReplEngine) -> Result<(), i32> {
    run_plain_repl(engine)
}

fn run_plain_repl(engine: &mut ReplEngine) -> Result<(), i32> {
    let stdin = io::stdin();
    loop {
        print!("{}", engine.prompt());
        if io::stdout().flush().is_err() {
            return Err(1);
        }

        let mut line = String::new();
        let read = match stdin.read_line(&mut line) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                return Err(1);
            }
        };
        if read == 0 {
            break;
        }
        let line = line.trim_end_matches(&['\r', '\n'][..]);
        let result = engine.handle_line(line);
        print_result(&result, styled::color_enabled_from_env());
        if result.should_exit {
            break;
        }
    }

    Ok(())
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
}
