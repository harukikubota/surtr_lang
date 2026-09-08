use std::fs;
use std::time::Instant;

use crate::compile::{
    collect_default_script_compile_sources, compile_source_with_measurement,
    prepare_script_compile_plan, script_plan_error_as_rune_error,
};
use crate::error::{ExecutionEnv, RuneError, RuneResult};
use crate::measurement::{elapsed, CompileMeasurement, MeasurementOutput};

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    if args.is_empty() {
        return Err(RuneError::usage(String::new()));
    }

    let file_path = &args[0];
    if file_path.starts_with('-') {
        return Err(RuneError::message(
            1,
            format!("check: unknown option '{}'", file_path),
        ));
    }
    let mut format = "json";
    let mut format_seen = false;
    let mut phase_output = None;
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                i += 1;
                if i >= args.len() {
                    return Err(RuneError::message(1, "check: missing value for --format"));
                }
                if format_seen {
                    return Err(RuneError::message(
                        1,
                        "check: --format may only be specified once",
                    ));
                }
                format_seen = true;
                format = args[i].as_str();
            }
            "--phase-times" => {
                if phase_output.is_some() {
                    return Err(RuneError::message(
                        1,
                        "check: phase timing output may only be specified once",
                    ));
                }
                phase_output = Some(MeasurementOutput::Text);
            }
            "--phase-times-json" => {
                if phase_output.is_some() {
                    return Err(RuneError::message(
                        1,
                        "check: phase timing output may only be specified once",
                    ));
                }
                phase_output = Some(MeasurementOutput::Json);
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

    check_command(file_path, phase_output)
}

fn check_command(file_path: &str, phase_output: Option<MeasurementOutput>) -> RuneResult<()> {
    let mut measurement = CompileMeasurement::new(file_path);
    let source_read_start = Instant::now();
    let source = fs::read_to_string(file_path)
        .map_err(|e| RuneError::message(1, format!("Error reading {}: {}", file_path, e)))?;
    measurement.source_read = elapsed(source_read_start);
    let compile_start = Instant::now();
    let plan_start = Instant::now();
    let compile_plan = prepare_script_compile_plan(file_path, &source, None)
        .map_err(|e| script_plan_error_as_rune_error(file_path, &source, e))?;
    measurement.compile_plan = elapsed(plan_start);
    let compile_sources = collect_default_script_compile_sources(
        ExecutionEnv::Check,
        file_path,
        &compile_plan.source_for_parse,
        &compile_plan.include_modules,
        xldr::StdlibVariant::Default,
    )?;

    match compile_source_with_measurement(
        ExecutionEnv::Check,
        &compile_sources,
        &compile_plan,
        phase_output.map(|_| &mut measurement),
    ) {
        Ok(_) => {
            measurement.compile_total = elapsed(compile_start);
            measurement.total = elapsed(source_read_start);
            if let Some(output) = phase_output {
                measurement.emit(output);
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_rejects_duplicate_format_option() {
        let err = dispatch(&[
            "missing.srt".to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .expect_err("duplicate check format must fail before reading input");

        assert_eq!(err.summary(), "check: --format may only be specified once");
    }

    #[test]
    fn check_rejects_option_like_input() {
        let err = dispatch(&["--bad".to_string()])
            .expect_err("option-looking check input must fail before reading input");

        assert_eq!(err.summary(), "check: unknown option '--bad'");
    }
}
