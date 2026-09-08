use std::fs;
use std::time::Instant;

use crate::compile::{
    collect_default_script_compile_sources, compile_source_with_measurement,
    prepare_script_compile_plan, script_plan_error_as_rune_error,
};
use crate::error::{ExecutionEnv, RuneError, RuneResult};
use crate::measurement::{elapsed, CompileMeasurement, MeasurementOutput};
use crate::util::default_output_path;

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    if args.is_empty() {
        return Err(RuneError::usage(String::new()));
    }

    let mut positional = Vec::new();
    let mut phase_output = None;
    for arg in args {
        match arg.as_str() {
            "--phase-times" => {
                if phase_output.is_some() {
                    return Err(RuneError::message(
                        1,
                        "build: phase timing output may only be specified once",
                    ));
                }
                phase_output = Some(MeasurementOutput::Text);
            }
            "--phase-times-json" => {
                if phase_output.is_some() {
                    return Err(RuneError::message(
                        1,
                        "build: phase timing output may only be specified once",
                    ));
                }
                phase_output = Some(MeasurementOutput::Json);
            }
            value if value.starts_with('-') => {
                return Err(RuneError::message(
                    1,
                    format!("build: unknown option '{}'", value),
                ));
            }
            value => positional.push(value),
        }
    }
    if !(1..=2).contains(&positional.len()) {
        return Err(RuneError::usage(String::new()));
    }
    build_command(
        positional[0],
        positional.get(1).copied(),
        phase_output,
        ExecutionEnv::Build,
    )
}

fn build_command(
    input_srt: &str,
    output_eldr: Option<&str>,
    phase_output: Option<MeasurementOutput>,
    env: ExecutionEnv,
) -> RuneResult<()> {
    let total_start = Instant::now();
    let mut measurement = CompileMeasurement::new(input_srt);
    let source_read_start = Instant::now();
    let source = fs::read_to_string(input_srt)
        .map_err(|e| RuneError::message(1, format!("Error reading {}: {}", input_srt, e)))?;
    measurement.source_read = elapsed(source_read_start);

    let compile_start = Instant::now();
    let plan_start = Instant::now();
    let compile_plan = prepare_script_compile_plan(input_srt, &source, None)
        .map_err(|e| script_plan_error_as_rune_error(input_srt, &source, e))?;
    measurement.compile_plan = elapsed(plan_start);
    let compile_sources = collect_default_script_compile_sources(
        env,
        input_srt,
        &compile_plan.source_for_parse,
        &compile_plan.include_modules,
        xldr::StdlibVariant::Default,
    )?;
    let bytecode = compile_source_with_measurement(
        env,
        &compile_sources,
        &compile_plan,
        phase_output.map(|_| &mut measurement),
    )?;
    let encode_start = Instant::now();
    let bytes = bytecode
        .encode()
        .map_err(|e| RuneError::message(1, format!("Error encoding bytecode: {}", e)))?;
    measurement.bytecode_encode = elapsed(encode_start);

    let output_path = output_eldr
        .map(ToString::to_string)
        .unwrap_or_else(|| default_output_path(input_srt));
    let write_start = Instant::now();
    fs::write(&output_path, bytes)
        .map_err(|e| RuneError::message(1, format!("Error writing {}: {}", output_path, e)))?;
    measurement.output_write = elapsed(write_start);
    measurement.compile_total = elapsed(compile_start);
    measurement.total = elapsed(total_start);
    if let Some(output) = phase_output {
        measurement.emit(output);
    }
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
