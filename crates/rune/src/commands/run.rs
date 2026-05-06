use std::fs;
use std::path::Path;

use eldr::value::Value;
use serde_json::{json, Value as JsonValue};

use crate::compile::{
    collect_default_script_compile_sources, compile_source, prepare_script_compile_plan,
    script_plan_error_as_rune_error,
};
use crate::error::{ExecutionEnv, RuneError, RuneResult};
use crate::run_cache;

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
    pub(crate) cli_args: Vec<String>,
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
    let mut cli_args = Vec::new();
    let mut vm_dump_path = None;
    let mut vm_dump_mode = VmDumpMode::Error;
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--" => {
                cli_args.extend_from_slice(&args[i + 1..]);
                break;
            }
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
        cli_args,
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
        run_eldr_file(
            &options.file_path,
            env,
            &options.cli_args,
            options.vm_dump.as_ref(),
        )
    } else {
        run_source_file(
            &options.file_path,
            options.entry.as_deref(),
            &options.cli_args,
            env,
            options.vm_dump.as_ref(),
        )
    }
}

fn run_source_file(
    file_path: &str,
    cli_entry: Option<&str>,
    cli_args: &[String],
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
    let bytecode = match run_cache::load(env, &compile_sources, &compile_plan) {
        Some(bytecode) => bytecode,
        None => {
            let bytecode = compile_source(env, &compile_sources, &compile_plan)?;
            run_cache::store(env, &compile_sources, &compile_plan, &bytecode);
            bytecode
        }
    };
    let runtime_sources = source_registry_from_bytecode(&bytecode);
    let source_context = runtime_sources
        .as_ref()
        .and_then(|(sources, source_id)| sources.owned_context(*source_id))
        .or_else(|| {
            compile_sources
                .sources
                .owned_context(compile_sources.user_source_id)
        });
    execute_bytecode(
        env,
        bytecode,
        cli_args,
        source_context,
        runtime_sources.or_else(|| {
            Some((
                compile_sources.sources.clone(),
                compile_sources.user_source_id,
            ))
        }),
        vm_dump,
    )
}

fn run_eldr_file(
    file_path: &str,
    env: ExecutionEnv,
    cli_args: &[String],
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

    let runtime_sources = source_registry_from_bytecode(&bytecode);
    let source_context = runtime_sources
        .as_ref()
        .and_then(|(sources, source_id)| sources.owned_context(*source_id));
    execute_bytecode(
        env,
        bytecode,
        cli_args,
        source_context,
        runtime_sources,
        vm_dump,
    )
}

fn source_registry_from_bytecode(
    bytecode: &forge::bytecode::Bytecode,
) -> Option<(diagnostics::SourceRegistry, diagnostics::SourceId)> {
    let mut embedded_sources = bytecode
        .sources
        .iter()
        .filter_map(|entry| {
            entry.text.as_ref().map(|text| {
                let file_name = entry
                    .normalized_path
                    .clone()
                    .unwrap_or_else(|| entry.path.clone());
                let file_name = if file_name.is_empty() {
                    "<embedded>".to_string()
                } else {
                    file_name
                };
                (entry.source_id, text.clone(), file_name)
            })
        })
        .collect::<Vec<_>>();
    embedded_sources.sort_by_key(|(source_id, _, _)| *source_id);
    let primary_embedded_source_id = embedded_sources
        .iter()
        .map(|(source_id, _, _)| *source_id)
        .max()?;

    let mut sources = diagnostics::SourceRegistry::new();
    let mut primary_source_id = None;
    for (embedded_source_id, source, file_name) in embedded_sources {
        let source_id = sources.register(file_name, source);
        if embedded_source_id == primary_embedded_source_id {
            primary_source_id = Some(source_id);
        }
    }

    primary_source_id.map(|source_id| (sources, source_id))
}

fn execute_bytecode(
    env: ExecutionEnv,
    bytecode: forge::bytecode::Bytecode,
    cli_args: &[String],
    source_context: Option<(String, String)>,
    runtime_sources: Option<(diagnostics::SourceRegistry, diagnostics::SourceId)>,
    vm_dump: Option<&VmDumpOptions>,
) -> RuneResult<()> {
    let mut vm = match source_context {
        Some((source, file_path)) => eldr::VM::new(bytecode).with_source(source, file_path),
        None => eldr::VM::new(bytecode),
    }
    .with_cli_args(cli_args.to_vec());
    if vm_dump.is_some() {
        vm.enable_observation(eldr::vm::VmObservationOptions::default());
    }
    if let Err(e) = vm.run() {
        let location = e
            .context
            .call_site
            .clone()
            .or_else(|| vm.runtime_error_location());
        match runtime_sources.as_ref() {
            Some((sources, source_id)) => xldr::error_display::emit_runtime_error_with_registry(
                &e,
                sources,
                *source_id,
                location.clone(),
                xldr::ErrorDisplayMode::Full,
            ),
            None => xldr::error_display::emit_runtime_error(
                &e,
                vm.source(),
                vm.source_file(),
                location,
                xldr::ErrorDisplayMode::Full,
            ),
        }
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
    let process_runtime = vm.process_runtime_snapshot();

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
            "process": {
                "process_spec_count": process_runtime.counters.process_spec_count,
                "singleton_slot_count": process_runtime.counters.singleton_slot_count,
                "process_count": process_runtime.counters.process_count,
                "runnable_process_count": process_runtime.counters.runnable_process_count,
                "waiting_process_count": process_runtime.counters.waiting_process_count,
                "completed_process_count": process_runtime.counters.completed_process_count,
                "failed_process_count": process_runtime.counters.failed_process_count,
                "mailbox_message_count": process_runtime.counters.mailbox_message_count,
                "future_count": process_runtime.counters.future_count,
                "running_future_count": process_runtime.counters.running_future_count,
                "ready_future_count": process_runtime.counters.ready_future_count,
                "cancelled_future_count": process_runtime.counters.cancelled_future_count,
                "waiting_table_count": process_runtime.counters.waiting_table_count,
                "reply_waiter_count": process_runtime.counters.reply_waiter_count,
                "deadline_queue_count": process_runtime.counters.deadline_queue_count,
            },
        },
        "process_runtime": {
            "counters": {
                "process_spec_count": process_runtime.counters.process_spec_count,
                "singleton_slot_count": process_runtime.counters.singleton_slot_count,
                "process_count": process_runtime.counters.process_count,
                "runnable_process_count": process_runtime.counters.runnable_process_count,
                "waiting_process_count": process_runtime.counters.waiting_process_count,
                "completed_process_count": process_runtime.counters.completed_process_count,
                "failed_process_count": process_runtime.counters.failed_process_count,
                "mailbox_message_count": process_runtime.counters.mailbox_message_count,
                "future_count": process_runtime.counters.future_count,
                "running_future_count": process_runtime.counters.running_future_count,
                "ready_future_count": process_runtime.counters.ready_future_count,
                "cancelled_future_count": process_runtime.counters.cancelled_future_count,
                "waiting_table_count": process_runtime.counters.waiting_table_count,
                "reply_waiter_count": process_runtime.counters.reply_waiter_count,
                "deadline_queue_count": process_runtime.counters.deadline_queue_count,
            },
            "specs": process_runtime.specs.iter().map(|spec| json!({
                "spec_id": spec.spec_id,
                "type_name": spec.type_name,
                "kind": spec.kind,
                "instance": spec.instance,
                "init_fun_idx": spec.init_fun_idx,
                "init_policy": spec.init_policy,
                "state_type": spec.state_type,
                "handler_count": spec.handler_count,
                "dependency_count": spec.dependency_count,
            })).collect::<Vec<_>>(),
            "singleton_slots": process_runtime.singleton_slots,
            "processes": process_runtime.processes.iter().map(|process| json!({
                "pid": process.pid,
                "process_name": process.process_name,
                "spec_id": process.spec_id,
                "status": process.status,
                "mailbox_len": process.mailbox_len,
                "owner": process.owner,
                "lazy_state_pending": process.lazy_state_pending,
                "state_value": process.state_value,
                "execution_context": process.execution_context.as_ref().map(|context| json!({
                    "pc": context.pc,
                    "stack_depth": context.stack_depth,
                    "frame_depth": context.frame_depth,
                    "target": context.target,
                })),
            })).collect::<Vec<_>>(),
            "waiting": process_runtime.waiting,
            "replies": process_runtime.replies,
            "deadlines": process_runtime.deadlines.iter().map(|deadline| json!({
                "future_id": deadline.future_id,
                "deadline_tick": deadline.deadline_tick,
            })).collect::<Vec<_>>(),
            "futures": process_runtime.futures.iter().map(|future| json!({
                "future_id": future.future_id,
                "owner": future.owner,
                "state": future.state,
                "value": future.value,
                "deadline_tick": future.deadline_tick,
                "waiter_count": future.waiter_count,
                "cancel_on_timeout": future.cancel_on_timeout,
                "correlation_id": future.correlation_id,
            })).collect::<Vec<_>>(),
        },
        "trace": {
            "dropped_events": observation.dropped_trace_events,
            "lines": observation.trace_lines,
        }
    })
}

fn report_final_result_error_if_any(vm: &eldr::VM) -> bool {
    match vm.last_value() {
        Some(value @ Value::Error(_)) => {
            xldr::error_display::emit_runtime_value_error_from_vm(
                vm,
                value,
                xldr::ErrorDisplayMode::Full,
            );
            true
        }
        Some(Value::Tagged { tag: 1, fields }) => {
            if let Some(err_value) = fields.first() {
                xldr::error_display::emit_runtime_value_error_from_vm(
                    vm,
                    err_value,
                    xldr::ErrorDisplayMode::Full,
                );
            } else {
                xldr::error_display::emit_invalid_result_missing_payload(
                    vm.source(),
                    vm.source_file(),
                    vm.runtime_error_location(),
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
    use super::{parse_run_options, source_registry_from_bytecode, VmDumpMode};
    use forge::bytecode::{Bytecode, SourceFileEntry};

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
        assert!(opts.cli_args.is_empty());
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
        assert!(opts.cli_args.is_empty());
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

    #[test]
    fn embedded_source_context_prefers_latest_source_id_with_text() {
        let bytecode = Bytecode {
            sources: vec![
                SourceFileEntry {
                    source_id: 10,
                    path: "mod_a.srt".to_string(),
                    normalized_path: None,
                    content_hash: None,
                    text: Some("a".to_string()),
                },
                SourceFileEntry {
                    source_id: 20,
                    path: "mod_b.srt".to_string(),
                    normalized_path: None,
                    content_hash: None,
                    text: None,
                },
                SourceFileEntry {
                    source_id: 30,
                    path: "main.srt".to_string(),
                    normalized_path: Some("/tmp/main.srt".to_string()),
                    content_hash: None,
                    text: Some("main()".to_string()),
                },
            ],
            ..Bytecode::default()
        };

        let (sources, source_id) =
            source_registry_from_bytecode(&bytecode).expect("embedded registry should be resolved");
        let context = sources
            .owned_context(source_id)
            .expect("embedded context should be resolved");
        assert!(context.0.contains("main"), "expected main source text");
        assert!(context.1.contains("main.srt"), "expected main file hint");
        assert_eq!(
            sources.entries().len(),
            2,
            "only embedded text sources register"
        );
        assert_eq!(
            sources.file_name(source_id),
            Some("/tmp/main.srt"),
            "highest embedded source id should remain the primary context"
        );
    }

    #[test]
    fn embedded_source_context_returns_none_when_no_embedded_text_exists() {
        let bytecode = Bytecode {
            sources: vec![SourceFileEntry {
                source_id: 1,
                path: "main.srt".to_string(),
                normalized_path: None,
                content_hash: None,
                text: None,
            }],
            ..Bytecode::default()
        };
        assert!(source_registry_from_bytecode(&bytecode).is_none());
    }

    #[test]
    fn run_options_collects_cli_args_after_separator() {
        let opts = parse_run_options(&[
            "main.srt".to_string(),
            "--entry".to_string(),
            "start".to_string(),
            "--".to_string(),
            "--dry-run".to_string(),
            "input.txt".to_string(),
        ])
        .expect("run options must parse cli args");
        assert_eq!(opts.entry.as_deref(), Some("start"));
        assert_eq!(
            opts.cli_args,
            vec!["--dry-run".to_string(), "input.txt".to_string()]
        );
    }
}
