//! Re-export from the new module layout.
//! This file is kept for backward compatibility and will be removed.

#[cfg(feature = "tui")]
pub use crate::repl::ui::tui::{run_command, TuiOptions};

#[cfg(not(feature = "tui"))]
#[derive(Debug, Default)]
pub struct TuiOptions {
    /// Path to a `.eldr` file to preload into the VM before the session starts.
    pub eldr_path: Option<String>,
}

#[cfg(not(feature = "tui"))]
pub fn run_command(_options: TuiOptions) -> Result<(), i32> {
    eprintln!("tui: disabled in this build (rebuild with xldr 'tui' feature)");
    Err(2)
}
