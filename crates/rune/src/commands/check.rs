use std::fs;

use crate::compile::{
    collect_default_script_compile_sources, compile_source, prepare_script_compile_plan,
    script_plan_error_as_rune_error,
};
use crate::error::{ExecutionEnv, RuneError, RuneResult};

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    if args.is_empty() {
        return Err(RuneError::usage(String::new()));
    }

    let file_path = &args[0];
    let mut format = "json";
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                i += 1;
                if i >= args.len() {
                    return Err(RuneError::message(1, "check: missing value for --format"));
                }
                format = args[i].as_str();
            }
            other => {
                return Err(RuneError::message(
                    1,
                    format!("check: unknown option '{}'", other),
                ));
            }
        }
        i += 1;
    }

    if format != "json" {
        return Err(RuneError::message(
            1,
            format!("check: unsupported format '{}'. supported: json", format),
        ));
    }

    check_command(file_path)
}

fn check_command(file_path: &str) -> RuneResult<()> {
    let source = fs::read_to_string(file_path)
        .map_err(|e| RuneError::message(1, format!("Error reading {}: {}", file_path, e)))?;
    let compile_plan = prepare_script_compile_plan(file_path, &source, None)
        .map_err(|e| script_plan_error_as_rune_error(file_path, &source, e))?;
    let compile_sources = collect_default_script_compile_sources(
        ExecutionEnv::Check,
        file_path,
        &compile_plan.source_for_parse,
        &compile_plan.include_modules,
    )?;

    match compile_source(ExecutionEnv::Check, &compile_sources, &compile_plan) {
        Ok(_) => {
            println!(r#"{{"errors":[]}}"#);
            Ok(())
        }
        Err(error) => {
            let text = serde_json::to_string(&error.to_serializable_report()).map_err(|e| {
                RuneError::message(1, format!("check: failed to serialize diagnostics: {}", e))
            })?;
            println!("{text}");
            Err(RuneError::silent(error.exit_code()))
        }
    }
}
