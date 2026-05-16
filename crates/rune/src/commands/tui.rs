use crate::error::{ExecutionEnv, RuneError, RuneResult};

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    if let Some(path) = args.first() {
        if path.starts_with('-') {
            return Err(RuneError::message(
                1,
                format!("tui: unknown option '{}'", path),
            ));
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tui_rejects_option_like_input() {
        let err = dispatch(&["--bad".to_string()])
            .expect_err("option-looking tui input must fail before loading a file");

        assert_eq!(err.summary(), "tui: unknown option '--bad'");
    }

    #[test]
    fn tui_rejects_option_like_input_before_extra_args() {
        let err = dispatch(&["--bad".to_string(), "extra.eldr".to_string()])
            .expect_err("option-looking tui input must fail before arity validation");

        assert_eq!(err.summary(), "tui: unknown option '--bad'");
    }
}
