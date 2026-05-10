use crate::error::{ExecutionEnv, RuneError, RuneResult};

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    if args.len() > 1 {
        return Err(RuneError::usage("tui: too many arguments"));
    }
    let options = xldr::tui::TuiOptions {
        eldr_path: args.first().cloned(),
    };
    tui_command(options, ExecutionEnv::Tui)
}

fn tui_command(options: xldr::tui::TuiOptions, _env: ExecutionEnv) -> RuneResult<()> {
    xldr::tui::run_command(options).map_err(RuneError::from_xldr_command_error)
}
