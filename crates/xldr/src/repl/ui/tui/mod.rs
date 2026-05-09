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
use std::time::Duration;

use crossterm::{
    event::{self, Event as CrosstermEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::repl::logic::core::ReplEngine;
use crate::repl::logic::PresentedResultKind;

use app::App;

// ── Options ───────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct TuiOptions {
    /// Path to a `.eldr` file to preload into the VM before the session starts.
    pub eldr_path: Option<String>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Launch the TUI REPL.
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
        app.push_result(
            path.clone(),
            Vec::new(),
            vec![format!("loaded {path}")],
            Vec::new(),
            PresentedResultKind::Info,
        );
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
        terminal.draw(|f| widgets::draw(f, app)).map_err(|e| {
            eprintln!("tui: draw error: {}", e);
            1i32
        })?;

        let timeout = Duration::from_millis(100);
        if event::poll(timeout).unwrap_or(false) {
            match event::read() {
                Ok(CrosstermEvent::Key(key)) => update::handle_key(app, engine, key),
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
