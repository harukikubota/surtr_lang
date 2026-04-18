use std::fs;
use std::path::Path;

use eldr::value::Value;
use serde_json::{json, Value as JsonValue};

use crate::compile::{
    collect_default_script_compile_sources, compile_source, prepare_script_compile_plan,
    script_plan_error_as_rune_error,
};
use crate::error::{ExecutionEnv, RuneError, RuneResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VmDumpMode {
    Error,
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VmDumpOptions {
    path: String,
    mode: VmDumpMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunOptions {
    pub(crate) file_path: String,
    pub(crate) entry: Option<String>,
    vm_dump: Option<VmDumpOptions>,
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
    let mut vm_dump_path = None;
    let mut vm_dump_mode = VmDumpMode::Error;
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
            "--vm-dump" => {
                i += 1;
                if i >= args.len() {
                    return Err(RuneError::usage("run: missing value for --vm-dump"));
                }
                if vm_dump_path.is_some() {
                    return Err(RuneError::message(
                        1,
                        "run: --vm-dump may only be specified once",
                    ));
                }
                vm_dump_path = Some(args[i].clone());
            }
            "--vm-dump-on" => {
                i += 1;
                if i >= args.len() {
                    return Err(RuneError::usage("run: missing value for --vm-dump-on"));
                }
                vm_dump_mode = match args[i].as_str() {
                    "error" => VmDumpMode::Error,
                    "always" => VmDumpMode::Always,
                    other => {
                        return Err(RuneError::message(
                            1,
                            format!(
                            "run: unsupported --vm-dump-on value '{}'. supported: error, always",
                            other
                        ),
                        ))
                    }
                };
            }
            other => {
                return Err(RuneError::usage(format!("run: unknown option '{}'", other)));
            }
        }
        i += 1;
    }

    if vm_dump_path.is_none() && vm_dump_mode != VmDumpMode::Error {
        return Err(RuneError::message(
            1,
            "run: --vm-dump-on requires --vm-dump",
        ));
    }

    Ok(RunOptions {
        file_path,
        entry,
        vm_dump: vm_dump_path.map(|path| VmDumpOptions {
            path,
            mode: vm_dump_mode,
        }),
    })
}

fn run_command(options: RunOptions, env: ExecutionEnv) -> RuneResult<()> {
    if options.file_path.ends_with(".eldr") {
        if options.entry.is_some() {
            return Err(RuneError::message(
                1,
                "run: --entry is only supported for .srt input",
            ));
        }
        run_eldr_file(&options.file_path, env, options.vm_dump.as_ref())
    } else {
        run_source_file(
            &options.file_path,
            options.entry.as_deref(),
            env,
            options.vm_dump.as_ref(),
        )
    }
}

fn run_source_file(
    file_path: &str,
    cli_entry: Option<&str>,
    env: ExecutionEnv,
    vm_dump: Option<&VmDumpOptions>,
) -> RuneResult<()> {
    let source = fs::read_to_string(file_path)
        .map_err(|e| RuneError::message(1, format!("Error reading {}: {}", file_path, e)))?;

    let compile_plan = prepare_script_compile_plan(file_path, &source, cli_entry)
        .map_err(|e| script_plan_error_as_rune_error(file_path, &source, e))?;

    let compile_sources = collect_default_script_compile_sources(
        env,
        file_path,
        &compile_plan.source_for_parse,
        &compile_plan.include_directives,
    )?;
    let bytecode = compile_source(env, &compile_sources, &compile_plan)?;
    execute_bytecode(
        env,
        bytecode,
        compile_sources
            .sources
            .owned_context(compile_sources.user_source_id),
        vm_dump,
    )
}

fn run_eldr_file(
    file_path: &str,
    env: ExecutionEnv,
    vm_dump: Option<&VmDumpOptions>,
) -> RuneResult<()> {
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

    execute_bytecode(env, bytecode, None, vm_dump)
}

fn execute_bytecode(
    env: ExecutionEnv,
    bytecode: forge::bytecode::Bytecode,
    source_context: Option<(String, String)>,
    vm_dump: Option<&VmDumpOptions>,
) -> RuneResult<()> {
    let mut vm = match source_context {
        Some((source, file_path)) => eldr::VM::new(bytecode).with_source(source, file_path),
        None => eldr::VM::new(bytecode),
    };
    if vm_dump.is_some() {
        vm.enable_observation(eldr::vm::VmObservationOptions::default());
    }
    if let Err(e) = vm.run() {
        xldr::error_display::emit_runtime_error(
            &e,
            vm.source(),
            vm.source_file(),
            vm.runtime_error_location(),
            xldr::ErrorDisplayMode::Full,
        );
        write_vm_dump_if_needed(vm_dump, &vm, RuntimeOutcome::RuntimeError { error: &e })?;
        return Err(RuneError::silent(1));
    }

    if matches!(env, ExecutionEnv::Run) && report_final_result_error_if_any(&vm) {
        write_vm_dump_if_needed(vm_dump, &vm, RuntimeOutcome::ResultErr)?;
        return Err(RuneError::silent(1));
    }

    match vm.exit_code() {
        0 => {
            write_vm_dump_if_needed(vm_dump, &vm, RuntimeOutcome::Success)?;
            Ok(())
        }
        code => {
            write_vm_dump_if_needed(vm_dump, &vm, RuntimeOutcome::ExitCode)?;
            Err(RuneError::silent(code))
        }
    }
}

enum RuntimeOutcome<'a> {
    Success,
    RuntimeError { error: &'a eldr::RuntimeError },
    ResultErr,
    ExitCode,
}

impl RuntimeOutcome<'_> {
    fn status(&self) -> &'static str {
        match self {
            Self::Success => "ok",
            Self::RuntimeError { .. } => "runtime_error",
            Self::ResultErr => "result_err",
            Self::ExitCode => "exit_code",
        }
    }

    fn is_error(&self) -> bool {
        !matches!(self, Self::Success)
    }
}

fn write_vm_dump_if_needed(
    vm_dump: Option<&VmDumpOptions>,
    vm: &eldr::VM,
    outcome: RuntimeOutcome<'_>,
) -> RuneResult<()> {
    let Some(vm_dump) = vm_dump else {
        return Ok(());
    };

    if matches!(vm_dump.mode, VmDumpMode::Error) && !outcome.is_error() {
        return Ok(());
    }

    let dump_json = build_vm_dump_json(vm, &outcome);
    write_vm_dump_file(&vm_dump.path, &dump_json)
}

fn write_vm_dump_file(path: &str, dump_json: &JsonValue) -> RuneResult<()> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                RuneError::message(
                    1,
                    format!(
                        "run: failed to create VM dump directory '{}': {}",
                        parent.display(),
                        e
                    ),
                )
            })?;
        }
    }

    let text = serde_json::to_string_pretty(dump_json).map_err(|e| {
        RuneError::message(1, format!("run: failed to serialize VM dump json: {}", e))
    })?;
    fs::write(path, text).map_err(|e| {
        RuneError::message(1, format!("run: failed to write VM dump '{}': {}", path, e))
    })
}

fn build_vm_dump_json(vm: &eldr::VM, outcome: &RuntimeOutcome<'_>) -> JsonValue {
    let pc = vm.pc();
    let success_opcode = pc
        .checked_sub(1)
        .and_then(|idx| vm.bytecode().opcodes.get(idx))
        .map(|opcode| opcode.kind_name().to_string());
    let last_opcode = match outcome {
        RuntimeOutcome::RuntimeError { error } => error.context.opcode.clone().or(success_opcode),
        _ => success_opcode,
    };
    let runtime_error = match outcome {
        RuntimeOutcome::RuntimeError { error } => Some(json!({
            "message": error.message,
            "pc": error.context.pc,
            "opcode": error.context.opcode,
            "function": error.context.function,
            "call_site": error.context.call_site.as_ref().map(|location| json!({
                "file": location.file,
                "line": location.line,
                "column": location.column,
                "span": [location.span_start, location.span_end]
            })),
            "details": error.context.details,
        })),
        _ => None,
    };
    let observation = vm.observation().unwrap_or_default();

    json!({
        "schema_version": 1,
        "result": {
            "status": outcome.status(),
            "exit_code": vm.exit_code(),
            "last_value": vm.last_value().map(|value| eldr::builtin::inspect_value(vm, value)),
            "runtime_error": runtime_error,
        },
        "vm": {
            "pc": pc,
            "last_opcode": last_opcode,
            "stack_depth": vm.stack_depth(),
            "frame_depth": vm.frame_depth(),
        },
        "stats": {
            "executed_opcodes": observation.stats.executed_opcodes,
            "builtin_calls": observation.stats.builtin_calls,
            "function_calls": observation.stats.function_calls,
            "closure_calls": observation.stats.closure_calls,
            "return_count": observation.stats.return_count,
            "tail_calls_optimized": observation.stats.tail_calls_optimized,
            "max_stack_depth": observation.stats.max_stack_depth,
            "max_frame_depth": observation.stats.max_frame_depth,
            "per_opcode": observation.stats.per_opcode,
        },
        "trace": {
            "dropped_events": observation.dropped_trace_events,
            "lines": observation.trace_lines,
        }
    })
}

fn report_final_result_error_if_any(vm: &eldr::VM) -> bool {
    match vm.last_value() {
        Some(Value::Tagged { tag: 1, fields }) => {
            if let Some(err_value) = fields.first() {
                xldr::error_display::emit_runtime_value_error_from_vm(
                    vm,
                    err_value,
                    xldr::ErrorDisplayMode::Full,
                );
            } else {
                xldr::error_display::emit_text(
                    "Error: InvalidResult: missing Err payload",
                    xldr::ErrorDisplayMode::Full,
                );
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_run_options, VmDumpMode};

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
        assert!(opts.vm_dump.is_none());
    }

    #[test]
    fn run_options_parse_vm_dump_options() {
        let opts = parse_run_options(&[
            "main.srt".to_string(),
            "--vm-dump".to_string(),
            "artifacts/vm.json".to_string(),
            "--vm-dump-on".to_string(),
            "always".to_string(),
        ])
        .expect("run options must parse vm dump");
        let vm_dump = opts.vm_dump.expect("vm dump options must exist");
        assert_eq!(vm_dump.path, "artifacts/vm.json");
        assert_eq!(vm_dump.mode, VmDumpMode::Always);
    }

    #[test]
    fn run_options_reject_vm_dump_on_without_vm_dump() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--vm-dump-on".to_string(),
            "always".to_string(),
        ])
        .expect_err("vm dump on without vm dump must fail");
        assert_eq!(err.summary(), "run: --vm-dump-on requires --vm-dump");
    }
}
