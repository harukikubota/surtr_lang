use std::fs;

use crate::compile::{
    collect_default_script_compile_sources, compile_source, prepare_script_compile_plan,
    script_plan_error_as_rune_error,
};
use crate::error::{ExecutionEnv, RuneError, RuneResult};
use crate::util::default_output_path;

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    if !(1..=2).contains(&args.len()) {
        return Err(RuneError::usage(String::new()));
    }
    for arg in args {
        if arg.starts_with('-') {
            return Err(RuneError::message(
                1,
                format!("build: unknown option '{}'", arg),
            ));
        }
    }
    build_command(
        &args[0],
        args.get(1).map(String::as_str),
        ExecutionEnv::Build,
    )
}

fn build_command(input_srt: &str, output_eldr: Option<&str>, env: ExecutionEnv) -> RuneResult<()> {
    let source = fs::read_to_string(input_srt)
        .map_err(|e| RuneError::message(1, format!("Error reading {}: {}", input_srt, e)))?;

    let compile_plan = prepare_script_compile_plan(input_srt, &source, None)
        .map_err(|e| script_plan_error_as_rune_error(input_srt, &source, e))?;
    let compile_sources = collect_default_script_compile_sources(
        env,
        input_srt,
        &compile_plan.source_for_parse,
        &compile_plan.include_modules,
        xldr::StdlibVariant::Default,
    )?;
    let bytecode = compile_source(env, &compile_sources, &compile_plan)?;
    let bytes = bytecode
        .encode()
        .map_err(|e| RuneError::message(1, format!("Error encoding bytecode: {}", e)))?;

    let output_path = output_eldr
        .map(ToString::to_string)
        .unwrap_or_else(|| default_output_path(input_srt));
    fs::write(&output_path, bytes)
        .map_err(|e| RuneError::message(1, format!("Error writing {}: {}", output_path, e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_rejects_option_like_input() {
        let err = dispatch(&["--bad".to_string()])
            .expect_err("option-looking build input must fail before reading input");

        assert_eq!(err.summary(), "build: unknown option '--bad'");
    }
}
