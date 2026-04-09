use crate::error::{ExecutionEnv, RuneError, RuneResult};

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    let options = parse_repl_options(args)?;
    repl_command(options, ExecutionEnv::Repl)
}

fn parse_repl_options(args: &[String]) -> RuneResult<xldr::ReplOptions> {
    let mut options = xldr::ReplOptions::default();

    for arg in args {
        match arg.as_str() {
            "--quiet" => options.quiet = true,
            "--banner" => options.banner = xldr::BannerMode::Detailed,
            "--version" => options.version = true,
            other => {
                return Err(RuneError::usage(format!(
                    "repl: unknown option '{}'",
                    other
                )));
            }
        }
    }

    Ok(options)
}

fn repl_command(options: xldr::ReplOptions, _env: ExecutionEnv) -> RuneResult<()> {
    xldr::cli_command(options).map_err(RuneError::silent)
}
