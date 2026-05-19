//! CLI REPL — text-mode entry point.

use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
#[cfg(feature = "line-editor")]
use std::time::{Duration, Instant};

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

#[cfg(feature = "line-editor")]
use crate::repl::logic::core::{
    completion_allowed_at_cursor, completion_token, ReplCompletion, ReplCompletionCandidate,
    ReplCompletionContext, ReplCompletionKind,
};
use crate::repl::logic::core::{xldr_version, CompletionTelemetry, ReplEngine};
use crate::repl::logic::{present_for_cli, styled, ReplOutput, ReplResult};
#[cfg(feature = "line-editor")]
use crate::repl::ui::completion::{
    BackgroundReplCompletionProvider, ReplCompletionController, ReplCompletionProvider,
};
use crate::{CommandError, CommandResult};

#[cfg(feature = "line-editor")]
const TERMINAL_POLL_QUANTUM: Duration = Duration::from_millis(25);
const DEFAULT_COMPLETION_CANDIDATE_COUNT: usize = 5;

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
    pub project_path: Option<String>,
    pub project_profile: Option<String>,
    pub no_local_config: bool,
    pub config_path: Option<String>,
}

impl Default for ReplOptions {
    fn default() -> Self {
        Self {
            quiet: false,
            banner: BannerMode::Light,
            version: false,
            script_path: None,
            module_path: None,
            project_path: None,
            project_profile: None,
            no_local_config: false,
            config_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoadedCliUserConfig {
    config: CliUserConfig,
    loaded_from: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Deserialize)]
struct CliUserConfig {
    #[serde(default)]
    repl: ReplUserConfig,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Deserialize)]
struct ReplUserConfig {
    #[serde(default)]
    cli: ReplCliUserConfig,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Deserialize)]
struct ReplCliUserConfig {
    completion_candidates: Option<usize>,
}

impl CliUserConfig {
    fn completion_candidate_count(&self) -> usize {
        self.repl
            .cli
            .completion_candidates
            .unwrap_or(DEFAULT_COMPLETION_CANDIDATE_COUNT)
    }
}

fn parse_cli_user_config(path: &Path, contents: &str) -> Result<CliUserConfig, CommandError> {
    let config = serde_yaml::from_str::<CliUserConfig>(contents).map_err(|error| {
        CommandError::message(
            1,
            format!("repl: failed to parse {}: {}", path.display(), error),
        )
    })?;
    if config.repl.cli.completion_candidates == Some(0) {
        return Err(CommandError::message(
            1,
            format!(
                "repl: {} must set repl.cli.completion_candidates to 1 or more",
                path.display()
            ),
        ));
    }
    Ok(config)
}

fn load_cli_user_config(cwd: &Path) -> Result<LoadedCliUserConfig, CommandError> {
    let path = cwd.join(".xldr.yaml");
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(LoadedCliUserConfig {
                config: CliUserConfig::default(),
                loaded_from: None,
            });
        }
        Err(error) => {
            return Err(CommandError::message(
                1,
                format!("repl: failed to read {}: {}", path.display(), error),
            ));
        }
    };
    Ok(LoadedCliUserConfig {
        config: parse_cli_user_config(&path, &contents)?,
        loaded_from: Some(path),
    })
}

fn load_cli_user_config_from_path(path: &Path) -> Result<LoadedCliUserConfig, CommandError> {
    let contents = std::fs::read_to_string(path).map_err(|error| {
        CommandError::message(
            1,
            format!("repl: failed to read {}: {}", path.display(), error),
        )
    })?;
    Ok(LoadedCliUserConfig {
        config: parse_cli_user_config(path, &contents)?,
        loaded_from: Some(path.to_path_buf()),
    })
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
        if !completion_allowed_at_cursor(line, pos) {
            return Ok((pos, Vec::new()));
        }
        const COMMANDS: &[&str] = &[
            ":help",
            ":h",
            ":quit",
            ":exit",
            ":q",
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
        let (start, _end, word) = completion_token(line, pos);

        let mut matches = Vec::new();
        for cmd in COMMANDS {
            if cmd.starts_with(&word) {
                matches.push(Pair {
                    display: cmd.to_string(),
                    replacement: cmd.to_string(),
                });
            }
        }
        for symbol in &self.symbols {
            if symbol.starts_with(&word) {
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

    if options.project_path.is_some()
        && (options.module_path.is_some() || options.script_path.is_some())
    {
        return Err(CommandError::message(
            1,
            "repl: --project cannot be combined with --module or --script",
        ));
    }
    if options.project_profile.is_some() && options.project_path.is_none() {
        return Err(CommandError::message(
            1,
            "repl: --profile requires --project",
        ));
    }

    let current_dir = env::current_dir().map_err(|error| {
        CommandError::message(
            1,
            format!("repl: failed to resolve current directory: {}", error),
        )
    })?;
    let cli_config = if let Some(path) = &options.config_path {
        load_cli_user_config_from_path(Path::new(path))?
    } else if options.no_local_config {
        LoadedCliUserConfig {
            config: CliUserConfig::default(),
            loaded_from: None,
        }
    } else {
        load_cli_user_config(&current_dir)?
    };
    let completion_candidate_count = cli_config.config.completion_candidate_count();

    let mut engine = match (
        &options.project_path,
        &options.module_path,
        &options.script_path,
    ) {
        (Some(project_path), None, None) => {
            ReplEngine::from_project_runner_file(project_path, options.project_profile.as_deref())
                .map_err(CommandError::from)?
        }
        (None, Some(module_path), Some(script_path)) => {
            ReplEngine::from_preload_files(Some(module_path), Some(script_path))
                .map_err(CommandError::from)?
        }
        (None, Some(module_path), None) => {
            ReplEngine::from_preload_files(Some(module_path), None).map_err(CommandError::from)?
        }
        (None, None, Some(script_path)) => {
            ReplEngine::from_preload_files(None, Some(script_path)).map_err(CommandError::from)?
        }
        (None, None, None) => ReplEngine::new().map_err(|e| {
            CommandError::message(1, format!("Error initializing source loader: {}", e))
        })?,
        _ => unreachable!("project/script/module combinations are validated above"),
    };

    let color = styled::color_enabled_from_env();
    for result in engine.take_startup_results() {
        print_result(&result, color);
    }

    if io::stdin().is_terminal() {
        run_terminal_repl(&mut engine, completion_candidate_count)?;
    } else {
        run_plain_repl(&mut engine)?;
    }

    Ok(())
}

#[cfg(feature = "line-editor")]
fn run_terminal_repl(
    engine: &mut ReplEngine,
    completion_candidate_count: usize,
) -> CommandResult<()> {
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
    let mut completion_provider =
        BackgroundReplCompletionProvider::new(engine.completion_context());
    let mut completion_controller = ReplCompletionController::default();
    let mut rendered_completion = ReplCompletion::default();
    let mut rendered_completion_key: Option<(String, usize)> = None;
    let mut tab_completion_mode = false;

    redraw_terminal_prompt(
        &mut stdout,
        engine,
        &buffer,
        cursor_chars,
        Some(&rendered_completion),
        completion_candidate_count,
        None,
    )
    .map_err(|_| CommandError::message(1, "repl: failed to redraw prompt"))?;

    loop {
        if apply_terminal_completion_result(
            &mut stdout,
            engine,
            &buffer,
            cursor_chars,
            &mut completion_provider,
            &mut completion_controller,
            &mut rendered_completion,
            &mut rendered_completion_key,
            completion_candidate_count,
        )
        .map_err(|_| CommandError::message(1, "repl: failed to redraw prompt"))?
        {
            continue;
        }

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
                &rendered_completion,
                completion_candidate_count,
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
            let _ = apply_terminal_completion_result(
                &mut stdout,
                engine,
                &buffer,
                cursor_chars,
                &mut completion_provider,
                &mut completion_controller,
                &mut rendered_completion,
                &mut rendered_completion_key,
                completion_candidate_count,
            );
            continue;
        }

        let Event::Key(key) = event::read()
            .map_err(|err| CommandError::message(1, format!("Error reading input: {}", err)))?
        else {
            continue;
        };
        let event_received_at = Instant::now();

        let before_buffer = buffer.clone();
        let before_cursor = cursor_chars;
        match handle_terminal_key(&mut history, &mut buffer, &mut cursor_chars, key) {
            TerminalAction::Continue => {
                if buffer != before_buffer || cursor_chars != before_cursor {
                    let cursor_byte = byte_index_for_char_position(&buffer, cursor_chars);
                    if tab_completion_mode
                        && ReplCompletionContext::should_request(&buffer, cursor_byte)
                    {
                        completion_controller.submit_if_changed(
                            &mut completion_provider,
                            &buffer,
                            cursor_byte,
                            Some(event_received_at),
                        );
                    } else {
                        completion_controller.cancel_pending();
                        rendered_completion = ReplCompletion::default();
                        rendered_completion_key = None;
                    }
                    redraw_terminal_prompt(
                        &mut stdout,
                        engine,
                        &buffer,
                        cursor_chars,
                        Some(&rendered_completion),
                        completion_candidate_count,
                        None,
                    )
                    .map_err(|_| CommandError::message(1, "repl: failed to redraw prompt"))?;
                }
            }
            TerminalAction::EnterCompletionMode => {
                if tab_completion_mode
                    && rendered_completion_key
                        .as_ref()
                        .is_some_and(|(input, cursor)| {
                            input == &buffer
                                && *cursor == byte_index_for_char_position(&buffer, cursor_chars)
                        })
                    && apply_top_terminal_completion(
                        &mut buffer,
                        &mut cursor_chars,
                        &rendered_completion,
                    )
                {
                    completion_controller.cancel_pending();
                    rendered_completion = ReplCompletion::default();
                    rendered_completion_key = None;
                } else {
                    tab_completion_mode = true;
                    let cursor_byte = byte_index_for_char_position(&buffer, cursor_chars);
                    if ReplCompletionContext::should_request(&buffer, cursor_byte) {
                        completion_controller.submit_if_changed(
                            &mut completion_provider,
                            &buffer,
                            cursor_byte,
                            Some(event_received_at),
                        );
                    } else {
                        completion_controller.cancel_pending();
                        rendered_completion = ReplCompletion::default();
                        rendered_completion_key = None;
                    }
                }
                redraw_terminal_prompt(
                    &mut stdout,
                    engine,
                    &buffer,
                    cursor_chars,
                    Some(&rendered_completion),
                    completion_candidate_count,
                    None,
                )
                .map_err(|_| CommandError::message(1, "repl: failed to redraw prompt"))?;
            }
            TerminalAction::ExitCompletionMode => {
                tab_completion_mode = false;
                completion_controller.cancel_pending();
                rendered_completion = ReplCompletion::default();
                rendered_completion_key = None;
                redraw_terminal_prompt(
                    &mut stdout,
                    engine,
                    &buffer,
                    cursor_chars,
                    Some(&rendered_completion),
                    completion_candidate_count,
                    None,
                )
                .map_err(|_| CommandError::message(1, "repl: failed to redraw prompt"))?;
            }
            TerminalAction::Submit(line) => {
                history.record(&line);
                let submitted = submitted_terminal_line(&engine.prompt(), &line);
                write_crlf(&mut stdout, &submitted)
                    .map_err(|_| CommandError::message(1, "repl: failed to write input line"))?;
                tab_completion_mode = false;
                completion_controller.cancel_pending();
                rendered_completion = ReplCompletion::default();
                rendered_completion_key = None;
                let result = with_suspended_raw_mode(|| engine.handle_line(&line))
                    .map_err(|_| CommandError::message(1, "repl: failed to suspend raw mode"))?;
                if let Some(context) = engine.cached_completion_context() {
                    completion_provider.schedule_context_refresh(context);
                }
                if repl_result_has_visible_output(&result, color) {
                    print_terminal_result(
                        &mut stdout,
                        engine,
                        &result,
                        color,
                        &buffer,
                        cursor_chars,
                        &rendered_completion,
                        completion_candidate_count,
                    )
                    .map_err(|_| {
                        CommandError::message(1, "repl: failed to print terminal result")
                    })?;
                } else {
                    redraw_terminal_prompt(
                        &mut stdout,
                        engine,
                        &buffer,
                        cursor_chars,
                        Some(&rendered_completion),
                        completion_candidate_count,
                        None,
                    )
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
    EnterCompletionMode,
    ExitCompletionMode,
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
        KeyCode::Tab => TerminalAction::EnterCompletionMode,
        KeyCode::Esc => TerminalAction::ExitCompletionMode,
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
    completion: &ReplCompletion,
    completion_candidate_count: usize,
) -> io::Result<()> {
    let lines = repl_result_lines(result, color);
    for line in lines {
        write!(stdout, "\r\x1b[L\r\x1b[2K{line}\r\n")?;
    }
    stdout.flush()?;
    redraw_terminal_prompt(
        stdout,
        engine,
        buffer,
        cursor_chars,
        Some(completion),
        completion_candidate_count,
        None,
    )
    .map(|_| ())
}

#[cfg(feature = "line-editor")]
fn repl_result_has_visible_output(result: &ReplResult, color: bool) -> bool {
    !repl_result_lines(result, color).is_empty()
}

#[cfg(feature = "line-editor")]
fn repl_result_lines(result: &ReplResult, color: bool) -> Vec<String> {
    let mut lines = present_for_cli(result, color);
    lines.extend(repl_result_diagnostic_lines(result));
    lines.extend(result.stderr.iter().cloned());
    lines
}

#[cfg(feature = "line-editor")]
fn redraw_terminal_prompt(
    stdout: &mut io::Stdout,
    engine: &ReplEngine,
    buffer: &str,
    cursor_chars: usize,
    completion: Option<&ReplCompletion>,
    completion_candidate_count: usize,
    event_received_at: Option<Instant>,
) -> io::Result<CompletionTelemetry> {
    let color = styled::color_enabled_from_env();
    let prompt = engine.prompt();
    let column = prompt.chars().count().saturating_add(cursor_chars) as u16;
    let total_started = event_received_at.unwrap_or_else(Instant::now);
    let mut telemetry = completion
        .map(|completion| completion.telemetry.clone())
        .unwrap_or_default();
    let render_started = Instant::now();
    let completion_lines = completion
        .map(|completion| render_completion_lines(completion, color, completion_candidate_count))
        .unwrap_or_default();
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
    stdout.flush()?;
    telemetry.record_completion_render(render_started.elapsed());
    telemetry.record_total_key_to_visible_response(total_started.elapsed());
    Ok(telemetry)
}

#[cfg(feature = "line-editor")]
fn apply_terminal_completion_result(
    stdout: &mut io::Stdout,
    engine: &ReplEngine,
    buffer: &str,
    cursor_chars: usize,
    provider: &mut dyn ReplCompletionProvider,
    controller: &mut ReplCompletionController,
    rendered_completion: &mut ReplCompletion,
    rendered_completion_key: &mut Option<(String, usize)>,
    completion_candidate_count: usize,
) -> io::Result<bool> {
    let Some(result) = provider.poll_ready() else {
        return Ok(false);
    };
    let Some(result) = controller.accept_ready(result) else {
        return Ok(false);
    };

    let apply_started = Instant::now();
    let mut completion = result.completion;
    completion
        .telemetry
        .record_completion_apply(apply_started.elapsed());
    if let Some(event_received_at) = result.event_received_at {
        completion
            .telemetry
            .record_input_event_to_ui_handler(event_received_at.elapsed());
    }
    let telemetry = redraw_terminal_prompt(
        stdout,
        engine,
        buffer,
        cursor_chars,
        Some(&completion),
        completion_candidate_count,
        result.event_received_at,
    )?;
    completion.telemetry = telemetry;
    *rendered_completion = completion;
    *rendered_completion_key = Some((result.input, result.cursor));
    Ok(true)
}

#[cfg(feature = "line-editor")]
fn apply_top_terminal_completion(
    buffer: &mut String,
    cursor_chars: &mut usize,
    completion: &ReplCompletion,
) -> bool {
    let Some(candidate) = completion.candidates.first() else {
        return false;
    };

    let replace_start = previous_char_boundary(buffer, candidate.replace_start.min(buffer.len()));
    let replace_end =
        previous_char_boundary(buffer, candidate.replace_end.min(buffer.len())).max(replace_start);
    buffer.replace_range(replace_start..replace_end, &candidate.replacement);
    let cursor_byte = replace_start + candidate.replacement.len();
    *cursor_chars = buffer[..cursor_byte].chars().count();
    true
}

#[cfg(feature = "line-editor")]
fn previous_char_boundary(text: &str, mut idx: usize) -> usize {
    idx = idx.min(text.len());
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
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
fn render_completion_lines(
    completion: &ReplCompletion,
    color: bool,
    completion_candidate_count: usize,
) -> Vec<String> {
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
            .take(completion_candidate_count)
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
    for line in repl_result_diagnostic_lines(result) {
        eprintln!("{line}");
    }
    for line in &result.stderr {
        eprintln!("{line}");
    }
}

fn repl_result_diagnostic_lines(result: &ReplResult) -> Vec<String> {
    match &result.output {
        ReplOutput::EvalError { rendered, .. } => rendered.clone(),
        ReplOutput::Diagnostic {
            rendered,
            summary_tail,
        } => {
            let mut lines = rendered.clone();
            lines.extend(summary_tail.iter().cloned());
            lines
        }
        _ => Vec::new(),
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
            project_path: None,
            project_profile: None,
            no_local_config: true,
            config_path: None,
        })
        .expect_err("missing preload script must fail");

        assert_eq!(error.exit_code(), 1);
        assert!(matches!(error, crate::CommandError::Message { .. }));
    }
}

#[cfg(all(test, feature = "line-editor"))]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rustyline::completion::Completer;
    use rustyline::history::DefaultHistory;

    use crate::repl::logic::core::{
        CompletionTelemetry, ReplCompletion, ReplCompletionCandidate, ReplCompletionContext,
        ReplCompletionKind, ReplSignatureHelp,
    };
    use crate::repl::ui::completion::{
        ReplCompletionController, ReplCompletionProvider, ReplCompletionRequest,
        ReplCompletionResult,
    };
    use crate::ReplEngine;

    struct ReadyCompletionProvider {
        context: ReplCompletionContext,
        ready: VecDeque<ReplCompletionResult>,
    }

    impl ReadyCompletionProvider {
        fn new(engine: &ReplEngine) -> Self {
            Self {
                context: engine.completion_context(),
                ready: VecDeque::new(),
            }
        }
    }

    impl ReplCompletionProvider for ReadyCompletionProvider {
        fn submit(&mut self, request: ReplCompletionRequest) {
            let mut completion = self.context.completions(&request.input, request.cursor);
            completion
                .telemetry
                .record_completion_queue(Duration::from_nanos(1));
            self.ready.push_back(ReplCompletionResult {
                input: request.input,
                cursor: request.cursor,
                generation: request.generation,
                completion,
                enqueued_at: request.enqueued_at,
                event_received_at: request.event_received_at,
            });
        }

        fn poll_ready(&mut self) -> Option<ReplCompletionResult> {
            self.ready.pop_front()
        }

        fn schedule_context_refresh(&mut self, _context: ReplCompletionContext) {}
    }

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
    fn terminal_tab_enters_completion_mode_and_escape_exits_it() {
        let mut history = super::TerminalHistory::default();
        let mut buffer = "Str".to_string();
        let mut cursor_chars = buffer.chars().count();

        let action = super::handle_terminal_key(
            &mut history,
            &mut buffer,
            &mut cursor_chars,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        );
        assert_eq!(action, super::TerminalAction::EnterCompletionMode);

        let action = super::handle_terminal_key(
            &mut history,
            &mut buffer,
            &mut cursor_chars,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert_eq!(action, super::TerminalAction::ExitCompletionMode);
    }

    #[test]
    fn terminal_completion_applies_top_candidate_to_input_buffer() {
        let mut buffer = "a".to_string();
        let mut cursor_chars = buffer.chars().count();
        let completion = ReplCompletion {
            candidates: vec![ReplCompletionCandidate {
                label: "add".to_string(),
                replacement: "add".to_string(),
                kind: ReplCompletionKind::FunctionCall,
                detail: Some("Add::add(left: Int, right: Int) -> Int".to_string()),
                documentation: None,
                replace_start: 0,
                replace_end: 1,
            }],
            signature: None,
            telemetry: CompletionTelemetry::default(),
        };

        assert!(super::apply_top_terminal_completion(
            &mut buffer,
            &mut cursor_chars,
            &completion
        ));
        assert_eq!(buffer, "add");
        assert_eq!(cursor_chars, "add".chars().count());
    }

    #[test]
    fn rustyline_helper_completes_only_inside_string_interpolation() {
        let mut helper = super::ReplHelper::new();
        helper.set_symbols(vec!["String".to_string()]);
        let history = DefaultHistory::new();
        let ctx = rustyline::Context::new(&history);

        let plain = r#""plain Str"#;
        let (_start, matches) = helper
            .complete(plain, plain.len(), &ctx)
            .expect("rustyline completion should succeed");
        assert!(
            matches.is_empty(),
            "rustyline fallback completion should stay disabled in string text"
        );

        let interpolation = r#""plain #{Str"#;
        let (_start, matches) = helper
            .complete(interpolation, interpolation.len(), &ctx)
            .expect("rustyline completion should succeed");
        assert!(
            matches
                .iter()
                .any(|candidate| candidate.replacement == "String"),
            "rustyline fallback completion should run inside interpolation"
        );
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
                documentation: None,
                replace_start: 0,
                replace_end: 0,
            }],
            telemetry: CompletionTelemetry::default(),
        };

        let rendered = super::render_completion_lines(&completion, false, 5);
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
                    documentation: None,
                    replace_start: 0,
                    replace_end: 0,
                },
                ReplCompletionCandidate {
                    label: "print".to_string(),
                    replacement: "print".to_string(),
                    kind: ReplCompletionKind::FunctionCall,
                    detail: Some("Kernel::print(a: [String]) -> Unit".to_string()),
                    documentation: None,
                    replace_start: 0,
                    replace_end: 0,
                },
            ],
            telemetry: CompletionTelemetry::default(),
        };

        let rendered = super::render_completion_lines(&completion, true, 5);
        assert!(rendered[0].contains("\x1b["), "{rendered:?}");
        assert!(
            rendered[1].contains("\x1b[1;90mvar \x1b[0m"),
            "{rendered:?}"
        );
        assert!(rendered[1].contains("\x1b[36mname"), "{rendered:?}");
        assert!(
            rendered[2].contains("\x1b[1;90mcall\x1b[0m"),
            "{rendered:?}"
        );
        assert!(rendered[2].contains("\x1b[1;35mprint"), "{rendered:?}");
    }

    #[test]
    fn render_completion_lines_shows_only_configured_candidate_count() {
        let completion = ReplCompletion {
            signature: Some(ReplSignatureHelp {
                lines: vec!["Kernel::print(a: [String]) -> Unit".to_string()],
                active_parameter: Some(0),
            }),
            candidates: vec![
                ReplCompletionCandidate {
                    label: "one".to_string(),
                    replacement: "one".to_string(),
                    kind: ReplCompletionKind::Variable,
                    detail: Some("Int".to_string()),
                    documentation: None,
                    replace_start: 0,
                    replace_end: 0,
                },
                ReplCompletionCandidate {
                    label: "two".to_string(),
                    replacement: "two".to_string(),
                    kind: ReplCompletionKind::Variable,
                    detail: Some("Int".to_string()),
                    documentation: None,
                    replace_start: 0,
                    replace_end: 0,
                },
                ReplCompletionCandidate {
                    label: "three".to_string(),
                    replacement: "three".to_string(),
                    kind: ReplCompletionKind::Variable,
                    detail: Some("Int".to_string()),
                    documentation: None,
                    replace_start: 0,
                    replace_end: 0,
                },
            ],
            telemetry: CompletionTelemetry::default(),
        };

        let rendered = super::render_completion_lines(&completion, false, 2);
        assert_eq!(rendered.len(), 3, "{rendered:?}");
        assert!(rendered[1].contains("one"), "{rendered:?}");
        assert!(rendered[2].contains("two"), "{rendered:?}");
        assert!(
            !rendered.iter().any(|line| line.contains("three")),
            "{rendered:?}"
        );
    }

    #[test]
    fn load_cli_config_reads_completion_limit_from_xldr_yaml() {
        let temp_root = temp_root("xldr-cli-config");
        let config_path = temp_root.join(".xldr.yaml");
        fs::write(
            &config_path,
            "repl:\n  cli:\n    completion_candidates: 7\n",
        )
        .expect("config file should be written");

        let loaded =
            super::load_cli_user_config(&temp_root).expect("config should load successfully");
        assert_eq!(loaded.config.repl.cli.completion_candidates, Some(7));
        assert_eq!(loaded.loaded_from.as_deref(), Some(config_path.as_path()));
    }

    #[test]
    fn load_cli_config_returns_loaded_from_for_implicit_xldr_yaml() {
        let temp_root = temp_root("xldr-cli-config-loaded-from");
        let config_path = temp_root.join(".xldr.yaml");
        fs::write(&config_path, "repl:\n  cli: {}\n").expect("config file should be written");

        let loaded = super::load_cli_user_config(&temp_root).expect("config should load");
        assert_eq!(loaded.loaded_from.as_deref(), Some(config_path.as_path()));
    }

    #[test]
    fn load_cli_config_reads_explicit_config_path() {
        let temp_root = temp_root("xldr-cli-explicit-config");
        let config_path = temp_root.join("custom-xldr.yaml");
        fs::write(
            &config_path,
            "repl:\n  cli:\n    completion_candidates: 3\n",
        )
        .expect("config file should be written");

        let loaded = super::load_cli_user_config_from_path(&config_path)
            .expect("explicit config should load successfully");
        assert_eq!(loaded.config.repl.cli.completion_candidates, Some(3));
        assert_eq!(loaded.loaded_from.as_deref(), Some(config_path.as_path()));
    }

    #[test]
    fn redraw_terminal_prompt_returns_completion_telemetry() {
        let engine = ReplEngine::new().expect("REPL engine should bootstrap");
        let mut stdout = std::io::stdout();
        let mut completion = engine.completions("String::re", "String::re".len());
        completion
            .telemetry
            .record_input_event_to_ui_handler(Duration::from_nanos(1));

        let telemetry = super::redraw_terminal_prompt(
            &mut stdout,
            &engine,
            "String::re",
            "String::re".chars().count(),
            Some(&completion),
            5,
            Some(Instant::now()),
        )
        .expect("prompt redraw should succeed");

        assert!(
            telemetry
                .completion_compute_ns
                .is_some_and(|value| value > 0),
            "telemetry should include completion compute timing: {telemetry:?}"
        );
        assert!(
            telemetry
                .completion_render_ns
                .is_some_and(|value| value > 0),
            "telemetry should include render timing: {telemetry:?}"
        );
        assert!(
            telemetry
                .total_key_to_visible_response_ns
                .is_some_and(|value| value > 0),
            "telemetry should include end-to-end redraw timing: {telemetry:?}"
        );
    }

    #[test]
    fn apply_terminal_completion_result_records_positive_latency_samples() {
        let engine = ReplEngine::new().expect("REPL engine should bootstrap");
        let mut stdout = std::io::stdout();
        let mut provider = ReadyCompletionProvider::new(&engine);
        let mut controller = ReplCompletionController::default();
        let event_received_at = Instant::now()
            .checked_sub(Duration::from_millis(1))
            .expect("test event timestamp should fit");

        assert!(controller.submit_if_changed(
            &mut provider,
            "String::re",
            "String::re".len(),
            Some(event_received_at),
        ));

        let mut rendered_completion = ReplCompletion::default();
        let mut rendered_completion_key = None;
        let applied = super::apply_terminal_completion_result(
            &mut stdout,
            &engine,
            "String::re",
            "String::re".chars().count(),
            &mut provider,
            &mut controller,
            &mut rendered_completion,
            &mut rendered_completion_key,
            5,
        )
        .expect("completion result should apply");

        assert!(applied, "completion should be rendered");
        assert_eq!(
            rendered_completion_key,
            Some(("String::re".to_string(), "String::re".len()))
        );
        assert!(
            rendered_completion
                .telemetry
                .completion_queue_ns
                .is_some_and(|value| value > 0),
            "telemetry should include queue timing: {:?}",
            rendered_completion.telemetry
        );
        assert!(
            rendered_completion
                .telemetry
                .completion_compute_ns
                .is_some(),
            "telemetry should include compute timing: {:?}",
            rendered_completion.telemetry
        );
        assert!(
            rendered_completion
                .telemetry
                .completion_apply_ns
                .is_some(),
            "telemetry should include apply timing: {:?}",
            rendered_completion.telemetry
        );
        assert!(
            rendered_completion
                .telemetry
                .completion_render_ns
                .is_some(),
            "telemetry should include render timing: {:?}",
            rendered_completion.telemetry
        );
        assert!(
            rendered_completion
                .telemetry
                .total_key_to_visible_response_ns
                .is_some_and(|value| value > 0),
            "telemetry should include end-to-end timing: {:?}",
            rendered_completion.telemetry
        );
    }

    fn temp_root(prefix: &str) -> PathBuf {
        let unique = format!(
            "{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }
}
