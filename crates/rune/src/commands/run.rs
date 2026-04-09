use std::fs;

use eldr::value::Value;

use crate::compile::{
    collect_default_script_compile_sources, compile_source, prepare_script_compile_plan,
    script_plan_error_as_rune_error,
};
use crate::error::{ExecutionEnv, RuneError, RuneResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunOptions {
    pub(crate) file_path: String,
    pub(crate) entry: Option<String>,
}

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    let options = parse_run_options(args)?;
    run_command(options, ExecutionEnv::Run)
}

pub(crate) fn parse_run_options(args: &[String]) -> RuneResult<RunOptions> {
    if args.is_empty() {
        return Err(RuneError::usage(String::new()));
    }

    let file_path = args[0].clone();
    let mut entry = None;
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--entry" => {
                i += 1;
                if i >= args.len() {
                    return Err(RuneError::usage("run: missing value for --entry"));
                }
                if entry.is_some() {
                    return Err(RuneError::message(
                        1,
                        "run: --entry may only be specified once",
                    ));
                }
                entry = Some(args[i].clone());
            }
            other => {
                return Err(RuneError::usage(format!("run: unknown option '{}'", other)));
            }
        }
        i += 1;
    }

    Ok(RunOptions { file_path, entry })
}

fn run_command(options: RunOptions, env: ExecutionEnv) -> RuneResult<()> {
    if options.file_path.ends_with(".eldr") {
        if options.entry.is_some() {
            return Err(RuneError::message(
                1,
                "run: --entry is only supported for .srt input",
            ));
        }
        run_eldr_file(&options.file_path, env)
    } else {
        run_source_file(&options.file_path, options.entry.as_deref(), env)
    }
}

pub(crate) fn run_source_file(
    file_path: &str,
    cli_entry: Option<&str>,
    env: ExecutionEnv,
) -> RuneResult<()> {
    let source = fs::read_to_string(file_path)
        .map_err(|e| RuneError::message(1, format!("Error reading {}: {}", file_path, e)))?;

    let compile_plan = prepare_script_compile_plan(file_path, &source, cli_entry)
        .map_err(|e| script_plan_error_as_rune_error(file_path, &source, e))?;

    let compile_sources =
        collect_default_script_compile_sources(env, file_path, &compile_plan.source_for_parse)?;
    let bytecode = compile_source(env, &compile_sources, &compile_plan)?;
    execute_bytecode(
        env,
        bytecode,
        compile_sources
            .sources
            .owned_context(compile_sources.user_source_id),
    )
}

pub(crate) fn run_eldr_file(file_path: &str, env: ExecutionEnv) -> RuneResult<()> {
    let bytes = fs::read(file_path)
        .map_err(|e| RuneError::message(1, format!("Error reading {}: {}", file_path, e)))?;

    let bytecode = forge::bytecode::Bytecode::decode(&bytes).map_err(|e| {
        RuneError::message(
            1,
            format!(
                "{}: failed to decode {}: {}",
                env.command_name(),
                file_path,
                e
            ),
        )
    })?;

    execute_bytecode(env, bytecode, None)
}

pub(crate) fn execute_bytecode(
    env: ExecutionEnv,
    bytecode: forge::bytecode::Bytecode,
    source_context: Option<(String, String)>,
) -> RuneResult<()> {
    let mut vm = match source_context {
        Some((source, file_path)) => eldr::VM::new(bytecode).with_source(source, file_path),
        None => eldr::VM::new(bytecode),
    };
    if let Err(e) = vm.run() {
        eldr::report_runtime_error(
            &e,
            vm.source(),
            vm.source_file(),
            vm.runtime_error_location(),
        );
        return Err(RuneError::silent(1));
    }

    if matches!(env, ExecutionEnv::Run) && report_final_result_error_if_any(&vm) {
        return Err(RuneError::silent(1));
    }

    match vm.exit_code() {
        0 => Ok(()),
        code => Err(RuneError::silent(code)),
    }
}

fn report_final_result_error_if_any(vm: &eldr::VM) -> bool {
    match vm.last_value() {
        Some(Value::Tagged { tag: 1, fields }) => {
            if let Some(err_value) = fields.first() {
                report_error_value(vm, err_value);
            } else {
                eprintln!("Error: InvalidResult: missing Err payload");
            }
            true
        }
        _ => false,
    }
}

fn report_error_value(vm: &eldr::VM, value: &Value) {
    match value {
        Value::Error(rich) => {
            let start = rich.location.span_start as usize;
            let mut end = rich.location.span_end as usize;
            if end <= start {
                end = start.saturating_add(1);
            }
            match (vm.source(), vm.source_file()) {
                (Some(source), Some(file_name)) => diagnostics::report_error(
                    file_name,
                    source,
                    diagnostics::simple_error(
                        rich.kind.clone(),
                        rich.message.clone(),
                        spire::ast::Span { start, end },
                        None,
                    ),
                ),
                _ => eprintln!("Error: {}: {}", rich.kind, rich.message),
            }
        }
        other => {
            eprintln!("Error: {}", eldr::builtin::inspect_value(vm, other));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_run_options;

    #[test]
    fn run_options_parses_entry() {
        let opts = parse_run_options(&[
            "main.srt".to_string(),
            "--entry".to_string(),
            "start".to_string(),
        ])
        .expect("run options must parse");
        assert_eq!(opts.file_path, "main.srt");
        assert_eq!(opts.entry.as_deref(), Some("start"));
    }
}
