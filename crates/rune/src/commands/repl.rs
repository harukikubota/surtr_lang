use crate::error::{ExecutionEnv, RuneError, RuneResult};

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    let options = parse_repl_options(args)?;
    repl_command(options, ExecutionEnv::Repl)
}

fn parse_repl_options(args: &[String]) -> RuneResult<xldr::ReplOptions> {
    let mut options = xldr::ReplOptions::default();
    let mut i = 0usize;

    while i < args.len() {
        match args[i].as_str() {
            "--quiet" => options.quiet = true,
            "--banner" => options.banner = xldr::BannerMode::Detailed,
            "--version" => options.version = true,
            "--script" => {
                i += 1;
                if i >= args.len() {
                    return Err(RuneError::usage("repl: missing value for --script"));
                }
                if options.script_path.is_some() {
                    return Err(RuneError::usage(
                        "repl: --script may only be specified once",
                    ));
                }
                options.script_path = Some(args[i].clone());
            }
            "--module" => {
                i += 1;
                if i >= args.len() {
                    return Err(RuneError::usage("repl: missing value for --module"));
                }
                if options.module_path.is_some() {
                    return Err(RuneError::usage(
                        "repl: --module may only be specified once",
                    ));
                }
                options.module_path = Some(args[i].clone());
            }
            other => {
                return Err(RuneError::usage(format!(
                    "repl: unknown option '{}'",
                    other
                )));
            }
        }
        i += 1;
    }

    Ok(options)
}

fn repl_command(options: xldr::ReplOptions, _env: ExecutionEnv) -> RuneResult<()> {
    xldr::cli_command(options).map_err(RuneError::silent)
}

#[cfg(test)]
mod tests {
    use super::parse_repl_options;

    #[test]
    fn parse_repl_options_accepts_script_and_module_once_each() {
        let options = parse_repl_options(&[
            "--module".to_string(),
            "mod.srt".to_string(),
            "--script".to_string(),
            "main.srt".to_string(),
        ])
        .expect("options should parse");

        assert_eq!(options.module_path.as_deref(), Some("mod.srt"));
        assert_eq!(options.script_path.as_deref(), Some("main.srt"));
    }

    #[test]
    fn parse_repl_options_rejects_duplicate_script_flag() {
        let err = parse_repl_options(&[
            "--script".to_string(),
            "a.srt".to_string(),
            "--script".to_string(),
            "b.srt".to_string(),
        ])
        .expect_err("duplicate script flag must fail");

        let rendered = format!("{err:?}");
        assert!(rendered.contains("--script may only be specified once"), "{rendered}");
    }

    #[test]
    fn parse_repl_options_rejects_missing_module_value() {
        let err = parse_repl_options(&["--module".to_string()])
            .expect_err("missing module value must fail");

        let rendered = format!("{err:?}");
        assert!(rendered.contains("missing value for --module"), "{rendered}");
    }
}
