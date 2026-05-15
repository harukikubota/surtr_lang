use crate::{CommandError, CommandResult};

#[derive(Debug, Default)]
pub struct TuiOptions {
    /// Path to a `.eldr` file to preload into the VM before the session starts.
    pub eldr_path: Option<String>,
}

pub fn run_command(_options: TuiOptions) -> CommandResult<()> {
    Err(CommandError::message(
        2,
        "tui: disabled in this build (rebuild with xldr 'tui' feature)",
    ))
}
