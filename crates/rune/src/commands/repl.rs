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
            "--quiet" => {
                if options.quiet {
                    return Err(RuneError::usage("repl: --quiet may only be specified once"));
                }
                options.quiet = true;
            }
            "--banner" => {
                if options.banner == xldr::BannerMode::Detailed {
                    return Err(RuneError::usage(
                        "repl: --banner may only be specified once",
                    ));
                }
                options.banner = xldr::BannerMode::Detailed;
            }
            "--version" => {
                if options.version {
                    return Err(RuneError::usage(
                        "repl: --version may only be specified once",
                    ));
                }
                options.version = true;
            }
            "--no-local-config" => {
                if options.no_local_config {
                    return Err(RuneError::usage(
                        "repl: --no-local-config may only be specified once",
                    ));
                }
                options.no_local_config = true;
            }
            "--config" => {
                i += 1;
                if i >= args.len() || args[i].starts_with('-') {
                    return Err(RuneError::usage("repl: missing value for --config"));
                }
                if options.config_path.is_some() {
                    return Err(RuneError::usage(
                        "repl: --config may only be specified once",
                    ));
                }
                options.config_path = Some(args[i].clone());
            }
            "--script" => {
                i += 1;
                if i >= args.len() || args[i].starts_with('-') {
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
                if i >= args.len() || args[i].starts_with('-') {
                    return Err(RuneError::usage("repl: missing value for --module"));
                }
                if options.module_path.is_some() {
                    return Err(RuneError::usage(
                        "repl: --module may only be specified once",
                    ));
                }
                options.module_path = Some(args[i].clone());
            }
            "--project" => {
                i += 1;
                if i >= args.len() || args[i].starts_with('-') {
                    return Err(RuneError::usage("repl: missing value for --project"));
                }
                if options.project_path.is_some() {
                    return Err(RuneError::usage(
                        "repl: --project may only be specified once",
                    ));
                }
                options.project_path = Some(args[i].clone());
            }
            "--profile" => {
                i += 1;
                if i >= args.len() || args[i].starts_with('-') {
                    return Err(RuneError::usage("repl: missing value for --profile"));
                }
                if options.project_profile.is_some() {
                    return Err(RuneError::usage(
                        "repl: --profile may only be specified once",
                    ));
                }
                options.project_profile = Some(args[i].clone());
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

    if options.project_path.is_some()
        && (options.module_path.is_some() || options.script_path.is_some())
    {
        return Err(RuneError::usage(
            "repl: --project cannot be combined with --module or --script",
        ));
    }
    if options.project_profile.is_some() && options.project_path.is_none() {
        return Err(RuneError::usage("repl: --profile requires --project"));
    }
    if options.config_path.is_some() && options.no_local_config {
        return Err(RuneError::usage(
            "repl: --config cannot be combined with --no-local-config",
        ));
    }

    Ok(options)
}

fn repl_command(options: xldr::ReplOptions, _env: ExecutionEnv) -> RuneResult<()> {
    xldr::cli_command(options).map_err(RuneError::from_xldr_command_error)
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
    fn parse_repl_options_accepts_project_and_profile() {
        let options = parse_repl_options(&[
            "--project".to_string(),
            "project.srt".to_string(),
            "--profile".to_string(),
            "test".to_string(),
        ])
        .expect("options should parse");

        assert_eq!(options.project_path.as_deref(), Some("project.srt"));
        assert_eq!(options.project_profile.as_deref(), Some("test"));
    }

    #[test]
    fn parse_repl_options_accepts_no_local_config() {
        let options = parse_repl_options(&["--no-local-config".to_string()])
            .expect("options should parse");

        assert!(options.no_local_config);
    }

    #[test]
    fn parse_repl_options_accepts_explicit_config_path() {
        let options = parse_repl_options(&[
            "--config".to_string(),
            "custom-xldr.yaml".to_string(),
        ])
        .expect("options should parse");

        assert_eq!(options.config_path.as_deref(), Some("custom-xldr.yaml"));
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
        assert!(
            rendered.contains("--script may only be specified once"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_duplicate_project_flag() {
        let err = parse_repl_options(&[
            "--project".to_string(),
            "a.srt".to_string(),
            "--project".to_string(),
            "b.srt".to_string(),
        ])
        .expect_err("duplicate project flag must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("--project may only be specified once"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_duplicate_profile_flag() {
        let err = parse_repl_options(&[
            "--project".to_string(),
            "project.srt".to_string(),
            "--profile".to_string(),
            "dev".to_string(),
            "--profile".to_string(),
            "test".to_string(),
        ])
        .expect_err("duplicate profile flag must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("--profile may only be specified once"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_duplicate_quiet() {
        let err = parse_repl_options(&["--quiet".to_string(), "--quiet".to_string()])
            .expect_err("duplicate quiet flag must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("--quiet may only be specified once"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_duplicate_banner() {
        let err = parse_repl_options(&["--banner".to_string(), "--banner".to_string()])
            .expect_err("duplicate banner flag must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("--banner may only be specified once"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_duplicate_version() {
        let err = parse_repl_options(&["--version".to_string(), "--version".to_string()])
            .expect_err("duplicate version flag must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("--version may only be specified once"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_duplicate_no_local_config() {
        let err = parse_repl_options(&[
            "--no-local-config".to_string(),
            "--no-local-config".to_string(),
        ])
        .expect_err("duplicate no-local-config flag must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("--no-local-config may only be specified once"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_duplicate_config_flag() {
        let err = parse_repl_options(&[
            "--config".to_string(),
            "first.yaml".to_string(),
            "--config".to_string(),
            "second.yaml".to_string(),
        ])
        .expect_err("duplicate config flag must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("--config may only be specified once"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_missing_module_value() {
        let err = parse_repl_options(&["--module".to_string()])
            .expect_err("missing module value must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("missing value for --module"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_missing_project_value() {
        let err = parse_repl_options(&["--project".to_string()])
            .expect_err("missing project value must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("missing value for --project"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_missing_profile_value() {
        let err = parse_repl_options(&["--profile".to_string()])
            .expect_err("missing profile value must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("missing value for --profile"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_missing_config_value() {
        let err = parse_repl_options(&["--config".to_string()])
            .expect_err("missing config value must fail");

        let rendered = format!("{err:?}");
        assert!(rendered.contains("missing value for --config"), "{rendered}");
    }

    #[test]
    fn parse_repl_options_rejects_option_like_script_value() {
        let err = parse_repl_options(&["--script".to_string(), "--module".to_string()])
            .expect_err("option-looking script value must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("missing value for --script"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_option_like_module_value() {
        let err = parse_repl_options(&["--module".to_string(), "--script".to_string()])
            .expect_err("option-looking module value must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("missing value for --module"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_profile_without_project() {
        let err = parse_repl_options(&["--profile".to_string(), "dev".to_string()])
            .expect_err("profile without project must fail");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("--profile requires --project"),
            "{rendered}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_project_with_script_or_module() {
        let script_err = parse_repl_options(&[
            "--project".to_string(),
            "project.srt".to_string(),
            "--script".to_string(),
            "main.srt".to_string(),
        ])
        .expect_err("project and script must not be combined");
        assert!(
            format!("{script_err:?}").contains("--project cannot be combined"),
            "{script_err:?}"
        );

        let module_err = parse_repl_options(&[
            "--project".to_string(),
            "project.srt".to_string(),
            "--module".to_string(),
            "mod.srt".to_string(),
        ])
        .expect_err("project and module must not be combined");
        assert!(
            format!("{module_err:?}").contains("--project cannot be combined"),
            "{module_err:?}"
        );
    }

    #[test]
    fn parse_repl_options_rejects_config_with_no_local_config() {
        let err = parse_repl_options(&[
            "--config".to_string(),
            "custom.yaml".to_string(),
            "--no-local-config".to_string(),
        ])
        .expect_err("config and no-local-config must not be combined");

        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("--config cannot be combined with --no-local-config"),
            "{rendered}"
        );
    }
}
