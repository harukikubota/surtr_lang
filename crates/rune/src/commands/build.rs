use std::fs;

use crate::compile::{collect_default_script_compile_sources, compile_source, ScriptCompilePlan};
use crate::error::{ExecutionEnv, RuneError, RuneResult};
use crate::util::default_output_path;

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    if !(1..=2).contains(&args.len()) {
        return Err(RuneError::usage(String::new()));
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

    let compile_plan = ScriptCompilePlan::plain(source);
    let compile_sources =
        collect_default_script_compile_sources(env, input_srt, &compile_plan.source_for_parse)?;
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
