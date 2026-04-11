//! CLI REPL — text-mode entry point.

use std::io::{self, IsTerminal, Write};

use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::history::DefaultHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Context, Editor, Helper};

use crate::repl::logic::core::{xldr_version, ReplEngine};
use crate::repl::logic::output::ReplOutput;
use crate::repl::logic::ReplResult;

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

struct ReplHelper {
    hinter: HistoryHinter,
    symbols: Vec<String>,
}

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

impl Helper for ReplHelper {}
impl Highlighter for ReplHelper {}

impl Validator for ReplHelper {
    fn validate(
        &self,
        _ctx: &mut ValidationContext<'_>,
    ) -> Result<ValidationResult, ReadlineError> {
        Ok(ValidationResult::Valid(None))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<Self::Hint> {
        self.hinter.hint(line, pos, ctx)
    }
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        const COMMANDS: &[&str] = &[":quit", ":doc", ":save", ":v"];
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
                    print_result(&result);
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
    } else {
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
            print_result(&result);
            if result.should_exit {
                break;
            }
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

fn print_result(result: &ReplResult) {
    match &result.output {
        ReplOutput::EvalSuccess { rendered, .. } => {
            for line in rendered {
                println!("> {}", line);
            }
        }
        ReplOutput::CommandOutput { rendered } => {
            for line in rendered {
                println!("> {}", line);
            }
        }
        ReplOutput::DocResolved {
            symbol,
            signature,
            summary,
            source_snippet,
        } => {
            println!("> {}", symbol);
            if let Some(sig) = signature {
                println!("> sig: {}", sig);
            }
            if let Some(text) = source_snippet.as_ref().or(summary.as_ref()) {
                for line in text.lines() {
                    if !line.trim().is_empty() {
                        println!("> {}", line);
                    }
                }
            }
        }
        ReplOutput::EvalError { .. } | ReplOutput::StatusMessage(_) => {
            // Errors already printed to stderr by diagnostics / runtime reporter.
        }
        _ => {}
    }
}
