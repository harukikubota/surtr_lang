use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::Instant;

use eldr::value::Value;
use serde_json::{json, Value as JsonValue};
use sindr::runtime::{RichError, RuntimeStackFrame};

use crate::compile::{
    collect_default_script_compile_sources, compile_source, prepare_script_compile_plan,
    script_plan_error_as_rune_error,
};
use crate::error::{ExecutionEnv, RuneError, RuneResult};
use crate::run_cache;
use crate::util::surface_strip_global_prefixes;

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RunObservationOptions {
    vm_stats: bool,
    vm_stats_json: bool,
    trace_opcodes: bool,
    trace_calls: bool,
    trace_limit: Option<usize>,
    trace_filter: BTreeSet<String>,
}

impl RunObservationOptions {
    fn enabled(&self) -> bool {
        self.vm_stats || self.vm_stats_json || self.trace_opcodes || self.trace_calls
    }

    fn to_vm_options(&self) -> eldr::vm::VmObservationOptions {
        eldr::vm::VmObservationOptions {
            trace_opcodes: self.trace_opcodes,
            trace_calls: self.trace_calls,
            trace_limit: self.trace_limit,
            trace_filter: self.trace_filter.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ErrorContextMode {
    #[default]
    Normal,
    Verbose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunOptions {
    pub(crate) file_path: String,
    pub(crate) entry: Option<String>,
    pub(crate) cli_args: Vec<String>,
    vm_dump: Option<VmDumpOptions>,
    observation: RunObservationOptions,
    phase_times: bool,
    error_context: ErrorContextMode,
}

#[derive(Debug, Clone, Default)]
struct PhaseTimes {
    parse_ms: Option<u128>,
    resolve_ms: Option<u128>,
    typecheck_ms: Option<u128>,
    codegen_ms: Option<u128>,
    compile_ms: Option<u128>,
    decode_ms: Option<u128>,
    execute_ms: Option<u128>,
    total_ms: u128,
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
    if file_path.starts_with('-') {
        return Err(RuneError::message(
            1,
            format!("run: unknown option '{}'", file_path),
        ));
    }
    let mut entry = None;
    let mut cli_args = Vec::new();
    let mut vm_dump_path = None;
    let mut vm_dump_mode = VmDumpMode::Error;
    let mut vm_dump_mode_seen = false;
    let mut observation = RunObservationOptions::default();
    let mut trace_limit_seen = false;
    let mut trace_filter_seen = false;
    let mut phase_times = false;
    let mut error_context = ErrorContextMode::Normal;
    let mut error_context_seen = false;
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--" => {
                cli_args.extend_from_slice(&args[i + 1..]);
                break;
            }
            "--entry" => {
                i += 1;
                if i >= args.len() || args[i].starts_with('-') {
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
                if i >= args.len() || args[i].starts_with('-') {
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
                if i >= args.len() || args[i].starts_with('-') {
                    return Err(RuneError::usage("run: missing value for --vm-dump-on"));
                }
                if vm_dump_mode_seen {
                    return Err(RuneError::message(
                        1,
                        "run: --vm-dump-on may only be specified once",
                    ));
                }
                vm_dump_mode_seen = true;
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
            "--vm-stats" => {
                if observation.vm_stats {
                    return Err(RuneError::message(
                        1,
                        "run: --vm-stats may only be specified once",
                    ));
                }
                observation.vm_stats = true;
            }
            "--vm-stats-json" => {
                if observation.vm_stats_json {
                    return Err(RuneError::message(
                        1,
                        "run: --vm-stats-json may only be specified once",
                    ));
                }
                observation.vm_stats_json = true;
            }
            "--trace-opcode" => {
                if observation.trace_opcodes {
                    return Err(RuneError::message(
                        1,
                        "run: --trace-opcode may only be specified once",
                    ));
                }
                observation.trace_opcodes = true;
            }
            "--trace-call" => {
                if observation.trace_calls {
                    return Err(RuneError::message(
                        1,
                        "run: --trace-call may only be specified once",
                    ));
                }
                observation.trace_calls = true;
            }
            "--trace-limit" => {
                i += 1;
                if i >= args.len() {
                    return Err(RuneError::usage("run: missing value for --trace-limit"));
                }
                if trace_limit_seen {
                    return Err(RuneError::message(
                        1,
                        "run: --trace-limit may only be specified once",
                    ));
                }
                trace_limit_seen = true;
                let limit = args[i].parse::<usize>().map_err(|_| {
                    RuneError::message(1, format!("run: invalid --trace-limit value '{}'", args[i]))
                })?;
                if limit == 0 {
                    return Err(RuneError::message(
                        1,
                        "run: --trace-limit must be greater than 0",
                    ));
                }
                observation.trace_limit = Some(limit);
            }
            "--trace-filter" => {
                i += 1;
                if i >= args.len() {
                    return Err(RuneError::usage("run: missing value for --trace-filter"));
                }
                if trace_filter_seen {
                    return Err(RuneError::message(
                        1,
                        "run: --trace-filter may only be specified once",
                    ));
                }
                trace_filter_seen = true;
                observation.trace_filter = args[i]
                    .split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(|item| item.to_ascii_lowercase())
                    .collect();
                if observation.trace_filter.is_empty() {
                    return Err(RuneError::message(
                        1,
                        "run: --trace-filter must include a filter",
                    ));
                }
            }
            "--phase-times" => {
                if phase_times {
                    return Err(RuneError::message(
                        1,
                        "run: --phase-times may only be specified once",
                    ));
                }
                phase_times = true;
            }
            "--error-context" => {
                i += 1;
                if i >= args.len() {
                    return Err(RuneError::usage("run: missing value for --error-context"));
                }
                if error_context_seen {
                    return Err(RuneError::message(
                        1,
                        "run: --error-context may only be specified once",
                    ));
                }
                error_context_seen = true;
                error_context = match args[i].as_str() {
                    "verbose" => ErrorContextMode::Verbose,
                    other => {
                        return Err(RuneError::message(
                            1,
                            format!(
                                "run: unsupported --error-context value '{}'. supported: verbose",
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
        observation,
        phase_times,
        error_context,
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
            &options.observation,
            options.phase_times,
            options.error_context,
        )
    } else {
        run_source_file(
            &options.file_path,
            options.entry.as_deref(),
            &options.cli_args,
            env,
            options.vm_dump.as_ref(),
            &options.observation,
            options.phase_times,
            options.error_context,
        )
    }
}

fn run_source_file(
    file_path: &str,
    cli_entry: Option<&str>,
    cli_args: &[String],
    env: ExecutionEnv,
    vm_dump: Option<&VmDumpOptions>,
    observation: &RunObservationOptions,
    report_phase_times: bool,
    error_context: ErrorContextMode,
) -> RuneResult<()> {
    let total_start = Instant::now();
    let source = fs::read_to_string(file_path)
        .map_err(|e| RuneError::message(1, format!("Error reading {}: {}", file_path, e)))?;

    let compile_start = Instant::now();
    let compile_plan = prepare_script_compile_plan(file_path, &source, cli_entry)
        .map_err(|e| script_plan_error_as_rune_error(file_path, &source, e))?;

    let compile_sources = collect_default_script_compile_sources(
        env,
        file_path,
        &compile_plan.source_for_parse,
        &compile_plan.include_modules,
        xldr::StdlibVariant::Default,
    )?;
    let bytecode = match run_cache::load(env, &compile_sources, &compile_plan) {
        Some(bytecode) => bytecode,
        None => {
            let bytecode = compile_source(env, &compile_sources, &compile_plan)?;
            run_cache::store(env, &compile_sources, &compile_plan, &bytecode);
            bytecode
        }
    };
    let mut phase_times = PhaseTimes {
        compile_ms: Some(compile_start.elapsed().as_millis()),
        ..PhaseTimes::default()
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
    let execute_start = Instant::now();
    let phase_times = report_phase_times.then_some(&mut phase_times);
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
        observation,
        error_context,
        phase_times,
        &total_start,
        &execute_start,
    )
}

fn run_eldr_file(
    file_path: &str,
    env: ExecutionEnv,
    cli_args: &[String],
    vm_dump: Option<&VmDumpOptions>,
    observation: &RunObservationOptions,
    report_phase_times: bool,
    error_context: ErrorContextMode,
) -> RuneResult<()> {
    let total_start = Instant::now();
    let bytes = fs::read(file_path)
        .map_err(|e| RuneError::message(1, format!("Error reading {}: {}", file_path, e)))?;

    let decode_start = Instant::now();
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
    let mut phase_times = PhaseTimes {
        decode_ms: Some(decode_start.elapsed().as_millis()),
        ..PhaseTimes::default()
    };

    let runtime_sources = source_registry_from_bytecode(&bytecode);
    let source_context = runtime_sources
        .as_ref()
        .and_then(|(sources, source_id)| sources.owned_context(*source_id));
    let execute_start = Instant::now();
    let phase_times = report_phase_times.then_some(&mut phase_times);
    execute_bytecode(
        env,
        bytecode,
        cli_args,
        source_context,
        runtime_sources,
        vm_dump,
        observation,
        error_context,
        phase_times,
        &total_start,
        &execute_start,
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
    observation_options: &RunObservationOptions,
    error_context: ErrorContextMode,
    mut phase_times: Option<&mut PhaseTimes>,
    total_start: &Instant,
    execute_start: &Instant,
) -> RuneResult<()> {
    let mut vm = match source_context {
        Some((source, file_path)) => eldr::VM::new(bytecode).with_source(source, file_path),
        None => eldr::VM::new(bytecode),
    }
    .with_cli_args(cli_args.to_vec());
    if observation_options.enabled() {
        vm.enable_observation(observation_options.to_vm_options());
    } else if vm_dump.is_some() {
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
        if matches!(error_context, ErrorContextMode::Verbose) {
            emit_verbose_runtime_context(&vm, &e);
        }
        emit_phase_times_if_requested(&mut phase_times, total_start, execute_start);
        emit_observation_if_requested(&vm, observation_options);
        write_vm_dump_if_needed(vm_dump, &vm, RuntimeOutcome::RuntimeError { error: &e })?;
        return Err(RuneError::silent(1));
    }

    if let Err(e) = vm.drain_background_tasks() {
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
        if matches!(error_context, ErrorContextMode::Verbose) {
            emit_verbose_runtime_context(&vm, &e);
        }
        emit_phase_times_if_requested(&mut phase_times, total_start, execute_start);
        emit_observation_if_requested(&vm, observation_options);
        write_vm_dump_if_needed(vm_dump, &vm, RuntimeOutcome::RuntimeError { error: &e })?;
        return Err(RuneError::silent(1));
    }

    if matches!(env, ExecutionEnv::Run) && report_final_result_error_if_any(&vm) {
        if matches!(error_context, ErrorContextMode::Verbose) {
            emit_verbose_vm_context(&vm);
        }
        emit_phase_times_if_requested(&mut phase_times, total_start, execute_start);
        emit_observation_if_requested(&vm, observation_options);
        write_vm_dump_if_needed(vm_dump, &vm, RuntimeOutcome::ResultErr)?;
        return Err(RuneError::silent(1));
    }

    match vm.exit_code() {
        0 => {
            emit_phase_times_if_requested(&mut phase_times, total_start, execute_start);
            emit_observation_if_requested(&vm, observation_options);
            write_vm_dump_if_needed(vm_dump, &vm, RuntimeOutcome::Success)?;
            Ok(())
        }
        code => {
            emit_phase_times_if_requested(&mut phase_times, total_start, execute_start);
            emit_observation_if_requested(&vm, observation_options);
            write_vm_dump_if_needed(vm_dump, &vm, RuntimeOutcome::ExitCode)?;
            Err(RuneError::silent(code))
        }
    }
}

fn emit_observation_if_requested(vm: &eldr::VM, options: &RunObservationOptions) {
    if !options.enabled() {
        return;
    }
    let observation = vm.observation().unwrap_or_default();
    if options.vm_stats {
        eprintln!("VM stats:");
        eprintln!("  executed_opcodes: {}", observation.stats.executed_opcodes);
        eprintln!("  builtin_calls: {}", observation.stats.builtin_calls);
        eprintln!("  function_calls: {}", observation.stats.function_calls);
        eprintln!("  closure_calls: {}", observation.stats.closure_calls);
        eprintln!("  return_count: {}", observation.stats.return_count);
        eprintln!(
            "  tail_calls_optimized: {}",
            observation.stats.tail_calls_optimized
        );
        eprintln!("  max_stack_depth: {}", observation.stats.max_stack_depth);
        eprintln!("  max_frame_depth: {}", observation.stats.max_frame_depth);
        eprintln!("  branch:");
        eprintln!(
            "    jump_if_true_taken: {}",
            observation.stats.branch.jump_if_true_taken
        );
        eprintln!(
            "    jump_if_true_not_taken: {}",
            observation.stats.branch.jump_if_true_not_taken
        );
        eprintln!(
            "    jump_if_false_taken: {}",
            observation.stats.branch.jump_if_false_taken
        );
        eprintln!(
            "    jump_if_false_not_taken: {}",
            observation.stats.branch.jump_if_false_not_taken
        );
        eprintln!("  per_opcode:");
        for (kind, count) in &observation.stats.per_opcode {
            eprintln!("    {kind}: {count}");
        }
    }
    if options.trace_opcodes || options.trace_calls {
        for line in &observation.trace_lines {
            eprintln!("{line}");
        }
        if observation.dropped_trace_events > 0 {
            eprintln!("dropped_trace_events: {}", observation.dropped_trace_events);
        }
    }
    if options.vm_stats_json {
        match serde_json::to_string(&build_observation_json(vm)) {
            Ok(text) => eprintln!("{text}"),
            Err(err) => eprintln!(
                "{{\"schema_version\":1,\"error\":\"failed to serialize observation: {err}\"}}"
            ),
        }
    }
}

fn build_observation_json(vm: &eldr::VM) -> JsonValue {
    let observation = vm.observation().unwrap_or_default();
    json!({
        "schema_version": 1,
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
            "branch": {
                "jump_if_true_taken": observation.stats.branch.jump_if_true_taken,
                "jump_if_true_not_taken": observation.stats.branch.jump_if_true_not_taken,
                "jump_if_false_taken": observation.stats.branch.jump_if_false_taken,
                "jump_if_false_not_taken": observation.stats.branch.jump_if_false_not_taken,
            }
        },
        "trace": {
            "dropped_events": observation.dropped_trace_events,
            "lines": observation.trace_lines,
        }
    })
}

fn emit_verbose_runtime_context(vm: &eldr::VM, error: &eldr::RuntimeError) {
    eprintln!("Runtime context:");
    eprintln!("  pc: {}", error.context.pc.unwrap_or(vm.pc()));
    eprintln!(
        "  opcode: {}",
        error
            .context
            .opcode
            .clone()
            .unwrap_or_else(|| "<unknown>".to_string())
    );
    eprintln!(
        "  function: {}",
        error
            .context
            .function
            .clone()
            .unwrap_or_else(|| "<unknown>".to_string())
    );
    eprintln!("  stack_depth: {}", vm.stack_depth());
    eprintln!("  frame_depth: {}", vm.frame_depth());
    if !error.context.details.is_empty() {
        eprintln!("  details:");
        for detail in &error.context.details {
            eprintln!("    {detail}");
        }
    }
    emit_stack_trace(&error.context.stack_trace);
}

fn emit_verbose_vm_context(vm: &eldr::VM) {
    let pc = vm.pc();
    let last_opcode = pc
        .checked_sub(1)
        .and_then(|idx| vm.bytecode().opcodes.get(idx));
    eprintln!("Runtime context:");
    eprintln!("  pc: {}", pc);
    eprintln!(
        "  opcode: {}",
        last_opcode
            .map(|opcode| format!("{opcode:?}"))
            .unwrap_or_else(|| "<unknown>".to_string())
    );
    eprintln!(
        "  function: {}",
        last_opcode
            .and_then(|_| function_name_for_pc(vm.bytecode(), pc.saturating_sub(1)))
            .unwrap_or_else(|| "<unknown>".to_string())
    );
    eprintln!("  stack_depth: {}", vm.stack_depth());
    eprintln!("  frame_depth: {}", vm.frame_depth());
    if let Some(error) = final_rich_error(vm) {
        emit_stack_trace(&error.stack_trace);
    }
}

fn emit_stack_trace(stack_trace: &[RuntimeStackFrame]) {
    if stack_trace.is_empty() {
        return;
    }
    eprintln!("Stack trace:");
    for (idx, frame) in stack_trace.iter().take(32).enumerate() {
        eprintln!("  {}: {}", idx, eldr::format_stack_frame(frame));
    }
    if stack_trace.len() > 32 {
        eprintln!("  ... {} frame(s) omitted", stack_trace.len() - 32);
    }
}

fn final_rich_error(vm: &eldr::VM) -> Option<&RichError> {
    match vm.last_value()? {
        Value::Error(rich) => Some(rich),
        Value::Tagged { tag: 1, fields } => match fields.as_slice() {
            [Value::Error(rich)] => Some(rich),
            _ => None,
        },
        _ => None,
    }
}

fn function_name_for_pc(bytecode: &forge::bytecode::Bytecode, pc: usize) -> Option<String> {
    bytecode
        .functions
        .iter()
        .find(|entry| {
            let start = entry.entry_pc as usize;
            let end = if entry.end_pc > entry.entry_pc {
                entry.end_pc as usize
            } else {
                bytecode.opcodes.len()
            };
            pc >= start && pc < end
        })
        .map(|entry| {
            entry
                .qualified_name
                .clone()
                .unwrap_or_else(|| format!("fun#{}", entry.fun_idx))
        })
}

fn emit_phase_times_if_requested(
    phase_times: &mut Option<&mut PhaseTimes>,
    total_start: &Instant,
    execute_start: &Instant,
) {
    let Some(times) = phase_times.as_mut() else {
        return;
    };
    times.execute_ms = Some(execute_start.elapsed().as_millis());
    times.total_ms = total_start.elapsed().as_millis();
    emit_phase_times(times);
}

fn emit_phase_times(times: &PhaseTimes) {
    eprintln!("Phase times:");
    eprintln!("  parse: {}", format_optional_ms(times.parse_ms));
    eprintln!("  resolve: {}", format_optional_ms(times.resolve_ms));
    eprintln!("  typecheck: {}", format_optional_ms(times.typecheck_ms));
    eprintln!("  codegen: {}", format_optional_ms(times.codegen_ms));
    if times.compile_ms.is_some() {
        eprintln!("  compile: {}", format_optional_ms(times.compile_ms));
    }
    if times.decode_ms.is_some() {
        eprintln!("  decode: {}", format_optional_ms(times.decode_ms));
    }
    eprintln!("  execute: {}", format_optional_ms(times.execute_ms));
    eprintln!("  total: {}ms", times.total_ms);
}

fn format_optional_ms(value: Option<u128>) -> String {
    value
        .map(|ms| format!("{ms}ms"))
        .unwrap_or_else(|| "n/a".to_string())
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
            "stack_trace": stack_trace_json(&error.context.stack_trace),
        })),
        _ => None,
    };
    let result_error = match outcome {
        RuntimeOutcome::ResultErr => final_rich_error(vm).map(|error| {
            json!({
                "kind": error.kind,
                "message": error.visible_message(),
                "location": location_json(Some(error.primary_location())),
                "stack_trace": stack_trace_json(&error.stack_trace),
            })
        }),
        _ => None,
    };
    let observation = vm.observation().unwrap_or_default();
    let process_runtime = vm.process_runtime_snapshot();

    let mut dump = json!({
        "schema_version": 1,
        "result": {
            "status": outcome.status(),
            "exit_code": vm.exit_code(),
            "last_value": vm.last_value().map(|value| eldr::builtin::inspect_value(vm, value)),
            "runtime_error": runtime_error,
            "error": result_error,
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
            "branch": {
                "jump_if_true_taken": observation.stats.branch.jump_if_true_taken,
                "jump_if_true_not_taken": observation.stats.branch.jump_if_true_not_taken,
                "jump_if_false_taken": observation.stats.branch.jump_if_false_taken,
                "jump_if_false_not_taken": observation.stats.branch.jump_if_false_not_taken,
            },
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
                "standby_state_pending": process.standby_state_pending,
                "state_value": process.state_value,
                "execution_context": process.execution_context.as_ref().map(|context| json!({
                    "pc": context.pc,
                    "stack_depth": context.stack_depth,
                    "frame_depth": context.frame_depth,
                    "target": context.target,
                })),
            })).collect::<Vec<_>>(),
            "worker_sets": process_runtime.worker_sets.iter().map(|worker_set| json!({
                "id": worker_set.id,
                "worker_process": worker_set.worker_process,
                "supervisor": worker_set.supervisor,
                "target": worker_set.target,
                "min": worker_set.min,
                "max": worker_set.max,
                "member_pids": worker_set.member_pids,
                "live_count": worker_set.live_count,
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
    });
    surface_strip_global_prefixes(&mut dump);
    dump
}

fn stack_trace_json(stack_trace: &[RuntimeStackFrame]) -> JsonValue {
    JsonValue::Array(
        stack_trace
            .iter()
            .map(|frame| {
                json!({
                    "phase": runtime_phase_label(&frame.phase),
                    "function": frame.function,
                    "fun_idx": frame.fun_idx,
                    "call_kind": runtime_call_kind_label(&frame.call_kind),
                    "location": location_json(frame.location.as_ref()),
                    "process": frame.process.as_ref().map(|process| json!({
                        "pid": process.pid,
                        "process_name": process.process_name,
                        "trigger": process.trigger,
                    })),
                    "tco": frame.tco,
                })
            })
            .collect(),
    )
}

fn location_json(location: Option<&sindr::runtime::Location>) -> JsonValue {
    location
        .map(|location| {
            json!({
                "file": location.file,
                "func": location.func,
                "line": location.line,
                "column": location.column,
                "span": [location.span_start, location.span_end],
            })
        })
        .unwrap_or(JsonValue::Null)
}

fn runtime_phase_label(phase: &sindr::runtime::RuntimeExecutionPhase) -> &'static str {
    match phase {
        sindr::runtime::RuntimeExecutionPhase::VmInit => "vm_init",
        sindr::runtime::RuntimeExecutionPhase::Runtime => "runtime",
    }
}

fn runtime_call_kind_label(kind: &sindr::runtime::RuntimeCallKind) -> &'static str {
    match kind {
        sindr::runtime::RuntimeCallKind::DirectFunction => "direct_function",
        sindr::runtime::RuntimeCallKind::ClosureFunction => "closure_function",
        sindr::runtime::RuntimeCallKind::Builtin => "builtin",
        sindr::runtime::RuntimeCallKind::CallableTemplate => "callable_template",
        sindr::runtime::RuntimeCallKind::ProcessMessage => "process_message",
        sindr::runtime::RuntimeCallKind::Task => "task",
        sindr::runtime::RuntimeCallKind::StandbyInit => "standby_init",
        sindr::runtime::RuntimeCallKind::EagerInit => "eager_init",
    }
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
    fn run_options_reject_option_like_entry_value() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--entry".to_string(),
            "--vm-stats".to_string(),
        ])
        .expect_err("option-looking entry value must fail");

        assert_eq!(err.summary(), "run: missing value for --entry");
    }

    #[test]
    fn run_options_reject_option_like_input() {
        let err = parse_run_options(&["--bad".to_string()])
            .expect_err("option-looking run input must fail before reading input");

        assert_eq!(err.summary(), "run: unknown option '--bad'");
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
    fn run_options_reject_option_like_vm_dump_value() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--vm-dump".to_string(),
            "--vm-stats".to_string(),
        ])
        .expect_err("option-looking vm dump value must fail");

        assert_eq!(err.summary(), "run: missing value for --vm-dump");
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
    fn run_options_reject_option_like_vm_dump_on_value() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--vm-dump".to_string(),
            "vm.json".to_string(),
            "--vm-dump-on".to_string(),
            "--vm-stats".to_string(),
        ])
        .expect_err("option-looking vm dump mode must fail");

        assert_eq!(err.summary(), "run: missing value for --vm-dump-on");
    }

    #[test]
    fn run_options_reject_duplicate_trace_limit() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--trace-limit".to_string(),
            "10".to_string(),
            "--trace-limit".to_string(),
            "20".to_string(),
        ])
        .expect_err("duplicate trace limit must fail");

        assert_eq!(
            err.summary(),
            "run: --trace-limit may only be specified once"
        );
    }

    #[test]
    fn run_options_reject_zero_trace_limit() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--trace-limit".to_string(),
            "0".to_string(),
        ])
        .expect_err("zero trace limit must fail");

        assert_eq!(err.summary(), "run: --trace-limit must be greater than 0");
    }

    #[test]
    fn run_options_reject_duplicate_vm_stats() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--vm-stats".to_string(),
            "--vm-stats".to_string(),
        ])
        .expect_err("duplicate vm stats must fail");

        assert_eq!(err.summary(), "run: --vm-stats may only be specified once");
    }

    #[test]
    fn run_options_reject_duplicate_vm_stats_json() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--vm-stats-json".to_string(),
            "--vm-stats-json".to_string(),
        ])
        .expect_err("duplicate vm stats json must fail");

        assert_eq!(
            err.summary(),
            "run: --vm-stats-json may only be specified once"
        );
    }

    #[test]
    fn run_options_reject_duplicate_trace_opcode() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--trace-opcode".to_string(),
            "--trace-opcode".to_string(),
        ])
        .expect_err("duplicate trace opcode must fail");

        assert_eq!(
            err.summary(),
            "run: --trace-opcode may only be specified once"
        );
    }

    #[test]
    fn run_options_reject_duplicate_trace_call() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--trace-call".to_string(),
            "--trace-call".to_string(),
        ])
        .expect_err("duplicate trace call must fail");

        assert_eq!(
            err.summary(),
            "run: --trace-call may only be specified once"
        );
    }

    #[test]
    fn run_options_reject_duplicate_trace_filter() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--trace-filter".to_string(),
            "call".to_string(),
            "--trace-filter".to_string(),
            "opcode".to_string(),
        ])
        .expect_err("duplicate trace filter must fail");

        assert_eq!(
            err.summary(),
            "run: --trace-filter may only be specified once"
        );
    }

    #[test]
    fn run_options_reject_empty_trace_filter() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--trace-filter".to_string(),
            " , ".to_string(),
        ])
        .expect_err("empty trace filter must fail");

        assert_eq!(err.summary(), "run: --trace-filter must include a filter");
    }

    #[test]
    fn run_options_reject_duplicate_phase_times() {
        let err = parse_run_options(&[
            "main.srt".to_string(),
            "--phase-times".to_string(),
            "--phase-times".to_string(),
        ])
        .expect_err("duplicate phase times must fail");

        assert_eq!(
            err.summary(),
            "run: --phase-times may only be specified once"
        );
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
