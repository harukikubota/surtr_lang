//! TUI REPL — `surtr tui [file.eldr]`
//!
//! Layout (top → bottom):
//!   Results/History  (scrollable)
//!   Docs Queue       (fixed height)
//!   Completion       (collapsible)
//!   Input / Command  (growable)
//!   Status bar       (1 line)

pub mod app;
pub mod update;
pub mod widgets;

use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event as CrosstermEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    Terminal,
};

use crate::repl::logic::core::ReplEngine;
use crate::repl::logic::PresentedResultKind;
use crate::repl::ui::completion::BackgroundReplCompletionProvider;
use crate::{CommandError, CommandResult};

use app::App;

// ── Options ───────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct TuiOptions {
    /// Path to a `.eldr` file to preload into the VM before the session starts.
    pub eldr_path: Option<String>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Launch the TUI REPL.
pub fn run_command(options: TuiOptions) -> CommandResult<()> {
    let mut engine = match &options.eldr_path {
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|e| {
                CommandError::message(1, format!("tui: cannot read {}: {}", path, e))
            })?;
            ReplEngine::from_eldr(&bytes).map_err(CommandError::from)?
        }
        None => ReplEngine::new().map_err(|e| {
            CommandError::message(1, format!("tui: failed to initialise engine: {}", e))
        })?,
    };

    let mut app = App::new();
    if let Some(path) = &options.eldr_path {
        app.push_result(
            path.clone(),
            Vec::new(),
            vec![format!("loaded {path}")],
            Vec::new(),
            PresentedResultKind::Info,
        );
    }

    enable_raw_mode()
        .map_err(|e| CommandError::message(1, format!("tui: terminal init failed: {}", e)))?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| {
        let _ = disable_raw_mode();
        CommandError::message(1, format!("tui: {}", e))
    })?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).map_err(|e| {
        let _ = disable_raw_mode();
        CommandError::message(1, format!("tui: {}", e))
    })?;

    let result = run_loop(
        &mut terminal,
        &mut app,
        &mut engine,
        event::poll,
        event::read,
    );

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
}

fn run_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    engine: &mut ReplEngine,
    mut poll: impl FnMut(Duration) -> io::Result<bool>,
    mut read: impl FnMut() -> io::Result<CrosstermEvent>,
) -> CommandResult<()> {
    let mut completion_provider =
        BackgroundReplCompletionProvider::new(engine.completion_context());
    while !app.should_quit {
        update::poll_completion(app, &mut completion_provider);
        terminal
            .draw(|f| widgets::draw(f, app))
            .map_err(|e| CommandError::message(1, format!("tui: draw error: {}", e)))?;

        let timeout = Duration::from_millis(100);
        if poll(timeout)
            .map_err(|e| CommandError::message(1, format!("tui: event error: {}", e)))?
        {
            match read() {
                Ok(CrosstermEvent::Key(key)) => {
                    let event_received_at = Instant::now();
                    update::handle_key(
                        app,
                        engine,
                        &mut completion_provider,
                        key,
                        Some(event_received_at),
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    return Err(CommandError::message(1, format!("tui: event error: {}", e)));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn event_io_errors_stop_the_loop() {
        for fail_poll in [true, false] {
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            let mut app = App::new();
            let mut engine = ReplEngine::new().expect("REPL engine should bootstrap");
            let mut polls = 0;
            let mut reads = 0;
            let error = run_loop(
                &mut terminal,
                &mut app,
                &mut engine,
                |_| {
                    polls += 1;
                    assert_eq!(polls, 1, "event failure must stop before polling again");
                    if fail_poll {
                        Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "poll disconnected",
                        ))
                    } else {
                        Ok(true)
                    }
                },
                || {
                    reads += 1;
                    assert!(!fail_poll, "poll failure must not read an event");
                    Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "read disconnected",
                    ))
                },
            )
            .expect_err("event I/O failure must propagate");
            assert_eq!(polls, 1);
            assert_eq!(reads, usize::from(!fail_poll));
            assert_eq!(error.exit_code(), 1);
            let CommandError::Message { message, .. } = error else {
                panic!("event I/O failure must produce a command message");
            };
            let operation = if fail_poll { "poll" } else { "read" };
            assert_eq!(
                message,
                format!("tui: event error: {operation} disconnected")
            );
        }
    }
}
