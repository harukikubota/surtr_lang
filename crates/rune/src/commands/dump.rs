use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{json, Value};
use sindr::ir::{Bytecode, FunctionEntry, Opcode, OpcodeSource};
use sindr::viewer::viewer_file_from_inspect;

use crate::compile::{
    collect_default_script_compile_sources, compile_source, prepare_script_compile_plan,
    script_plan_error_as_rune_error,
};
use crate::error::{ExecutionEnv, RuneError, RuneResult};

pub(crate) fn dispatch(file_path: &str, args: &[String]) -> RuneResult<()> {
    let mut format = "json";
    let mut entry: Option<String> = None;
    let mut include_opcode_histogram = false;
    let mut include_peephole_candidates = false;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                i += 1;
                if i >= args.len() {
                    return Err(RuneError::message(1, "dump: missing value for --format"));
                }
                format = args[i].as_str();
            }
            "--entry" => {
                i += 1;
                if i >= args.len() {
                    return Err(RuneError::message(1, "dump: missing value for --entry"));
                }
                if entry.is_some() {
                    return Err(RuneError::message(
                        1,
                        "dump: --entry may only be specified once",
                    ));
                }
                entry = Some(args[i].clone());
            }
            "--opcode-histogram" => {
                include_opcode_histogram = true;
            }
            "--peephole-candidates" => {
                include_peephole_candidates = true;
            }
            other => {
                return Err(RuneError::message(
                    1,
                    format!("dump: unknown option '{}'", other),
                ));
            }
        }
        i += 1;
    }

    if format != "json" && format != "viewer-json" {
        return Err(RuneError::message(
            1,
            format!(
                "dump: unsupported format '{}'. supported: json, viewer-json",
                format
            ),
        ));
    }

    let options = DumpOptions {
        include_opcode_histogram,
        include_peephole_candidates,
    };

    if Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("srt"))
    {
        return dump_entry_source(
            file_path,
            entry.as_deref(),
            format,
            &options,
            ExecutionEnv::DumpSource,
        );
    }

    if entry.is_some() {
        return Err(RuneError::message(
            1,
            "dump: --entry is only supported for .srt input",
        ));
    }

    let env = ExecutionEnv::DumpBytecode;
    let bytes = fs::read(file_path)
        .map_err(|e| RuneError::message(1, format!("Error reading {}: {}", file_path, e)))?;
    let inspected = forge::bytecode::Bytecode::inspect(&bytes).map_err(|e| {
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

    let text = serialize_dump_output(file_path, format, &inspected, None, &options)?;
    println!("{}", text);
    Ok(())
}

#[derive(Debug, Clone, Copy, Default)]
struct DumpOptions {
    include_opcode_histogram: bool,
    include_peephole_candidates: bool,
}

fn dump_entry_source(
    file_path: &str,
    cli_entry: Option<&str>,
    format: &str,
    options: &DumpOptions,
    env: ExecutionEnv,
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
    let bytes = bytecode
        .encode()
        .map_err(|e| RuneError::message(1, format!("dump: failed to encode bytecode: {}", e)))?;
    let inspected = forge::bytecode::Bytecode::inspect(&bytes).map_err(|e| {
        RuneError::message(
            1,
            format!("dump: failed to inspect compiled bytecode: {}", e),
        )
    })?;

    let entrypoint_trace = json!({
        "source": "entry_file",
        "selected_entry_name": compile_plan.selected_entry_name,
        "normalized_entrypoint": compile_plan
            .normalized_entrypoint
            .as_ref()
            .map(|entry| entry.qualified_symbol.clone())
    });

    let text = serialize_dump_output(
        file_path,
        format,
        &inspected,
        Some(entrypoint_trace),
        options,
    )?;
    println!("{}", text);
    Ok(())
}

fn serialize_dump_output(
    file_path: &str,
    format: &str,
    inspected: &forge::bytecode::EldrInspect,
    entrypoint_trace: Option<Value>,
    options: &DumpOptions,
) -> RuneResult<String> {
    match format {
        "json" => {
            let dump_json = build_dump_json(file_path, inspected, entrypoint_trace, options)?;
            serde_json::to_string(&dump_json).map_err(|e| {
                RuneError::message(1, format!("dump: failed to serialize json: {}", e))
            })
        }
        "viewer-json" => {
            let viewer = viewer_file_from_inspect(inspected);
            serde_json::to_string(&viewer).map_err(|e| {
                RuneError::message(1, format!("dump: failed to serialize viewer json: {}", e))
            })
        }
        other => Err(RuneError::message(
            1,
            format!("dump: unsupported format '{}'", other),
        )),
    }
}

fn build_dump_json(
    file_path: &str,
    inspected: &forge::bytecode::EldrInspect,
    entrypoint_trace: Option<Value>,
    options: &DumpOptions,
) -> RuneResult<Value> {
    let header = serde_json::to_value(&inspected.header)
        .map_err(|e| RuneError::message(1, format!("dump: failed to serialize header: {}", e)))?;
    let chunks = serde_json::to_value(&inspected.chunks)
        .map_err(|e| RuneError::message(1, format!("dump: failed to serialize chunks: {}", e)))?;
    let mut bytecode = serde_json::to_value(&inspected.bytecode)
        .map_err(|e| RuneError::message(1, format!("dump: failed to serialize bytecode: {}", e)))?;
    surface_strip_global_prefixes(&mut bytecode);

    let mut dump = json!({
        "file": file_path,
        "header": header,
        "chunks": chunks,
        "summary": {
            "opcode_count": inspected.bytecode.opcodes.len(),
            "constant_count": inspected.bytecode.constants.len(),
            "function_count": inspected.bytecode.functions.len(),
            "type_entry_count": inspected.bytecode.type_registry.entries().len(),
            "error_template_count": inspected.bytecode.error_templates.len(),
            "num_locals": inspected.bytecode.num_locals,
            "doc_count": inspected.bytecode.docs.len(),
            "label_count": inspected.bytecode.labels.len(),
            "import_count": inspected.bytecode.imports.len(),
            "export_count": inspected.bytecode.exports.len(),
            "literal_count": inspected.bytecode.literals.len(),
            "span_count": inspected.bytecode.spans.len(),
            "source_count": inspected.bytecode.sources.len(),
            "pc_span_count": inspected.bytecode.pc_spans.len(),
            "process_spec_count": inspected.bytecode.runtime_process_specs.entries.len(),
            "bytecode_version": inspected.bytecode.compile_info.bytecode_version
        },
        "entrypoint_trace": entrypoint_trace,
        "bytecode": bytecode
    });

    if options.include_opcode_histogram {
        dump["opcode_histogram"] = json!(opcode_histogram(&inspected.bytecode));
        dump["optimization_summary"] = optimization_summary(&inspected.bytecode);
    }
    if options.include_peephole_candidates {
        dump["peephole_candidates"] = peephole_candidates(&inspected.bytecode);
    }

    Ok(dump)
}

fn opcode_histogram(bytecode: &Bytecode) -> BTreeMap<&'static str, usize> {
    let mut histogram = BTreeMap::new();
    for opcode in &bytecode.opcodes {
        *histogram.entry(opcode.kind_name()).or_default() += 1;
    }
    histogram
}

fn optimization_summary(bytecode: &Bytecode) -> Value {
    let histogram = opcode_histogram(bytecode);
    let store_const = histogram.get("StoreConstLocal").copied().unwrap_or(0);
    let copy_local = histogram.get("CopyLocal").copied().unwrap_or(0);
    let eq_local_tag = histogram.get("EqLocalTag").copied().unwrap_or(0);
    let make_ok = histogram.get("MakeOk").copied().unwrap_or(0);
    let make_err = histogram.get("MakeErr").copied().unwrap_or(0);
    let jump_if_local_tag_eq = histogram.get("JumpIfLocalTagEq").copied().unwrap_or(0);
    let jump_if_local_tag_ne = histogram.get("JumpIfLocalTagNe").copied().unwrap_or(0);

    let mut capture_closure_total = 0usize;
    let mut capture_closure_zero = 0usize;
    let mut call_closure = 0usize;
    let mut direct_builtin_calls = 0usize;
    let mut direct_user_calls = 0usize;
    for opcode in &bytecode.opcodes {
        match opcode {
            Opcode::CaptureClosure(count) => {
                capture_closure_total += 1;
                if *count == 0 {
                    capture_closure_zero += 1;
                }
            }
            Opcode::CallClosure { .. } => call_closure += 1,
            Opcode::CallBuiltin { .. } => direct_builtin_calls += 1,
            Opcode::Call { .. } => direct_user_calls += 1,
            _ => {}
        }
    }

    let generated_functions = bytecode
        .functions
        .iter()
        .filter(|entry| entry.flags.generated)
        .count();
    let partial_apply_wrappers = bytecode
        .functions
        .iter()
        .filter(|entry| entry.flags.partial_apply_wrapper)
        .count();

    json!({
        "compressed_opcodes": {
            "StoreConstLocal": {
                "count": store_const,
                "estimated_saved_opcodes": store_const
            },
            "CopyLocal": {
                "count": copy_local,
                "estimated_saved_opcodes": copy_local
            },
            "EqLocalTag": {
                "count": eq_local_tag,
                "estimated_saved_opcodes": eq_local_tag * 3
            },
            "MakeOk": {
                "count": make_ok,
                "estimated_saved_opcodes": make_ok
            },
            "MakeErr": {
                "count": make_err,
                "estimated_saved_opcodes": make_err
            },
            "JumpIfLocalTagEq": {
                "count": jump_if_local_tag_eq,
                "estimated_saved_opcodes": jump_if_local_tag_eq
            },
            "JumpIfLocalTagNe": {
                "count": jump_if_local_tag_ne,
                "estimated_saved_opcodes": jump_if_local_tag_ne
            },
            "estimated_saved_opcodes_total": store_const
                + copy_local
                + (eq_local_tag * 3)
                + make_ok
                + make_err
                + jump_if_local_tag_eq
                + jump_if_local_tag_ne
        },
        "apply_compose": {
            "call_closure": call_closure,
            "capture_closure_total": capture_closure_total,
            "capture_closure_zero": capture_closure_zero,
            "generated_functions": generated_functions,
            "partial_apply_wrappers": partial_apply_wrappers,
            "direct_calls": direct_builtin_calls + direct_user_calls,
            "direct_builtin_calls": direct_builtin_calls,
            "direct_user_calls": direct_user_calls
        }
    })
}

fn peephole_candidates(bytecode: &Bytecode) -> Value {
    let mut items = Vec::new();
    let mut summary: BTreeMap<&'static str, usize> = BTreeMap::new();

    for pc in 0..bytecode.opcodes.len() {
        let remaining = &bytecode.opcodes[pc..];
        let candidate = if matches!(remaining, [Opcode::LoadConst(_), Opcode::StoreLocal(_), ..]) {
            Some(("load_const_store_local", 2usize))
        } else if matches!(remaining, [Opcode::LoadLocal(_), Opcode::StoreLocal(_), ..]) {
            Some(("load_local_store_local", 2usize))
        } else if matches!(
            remaining,
            [
                Opcode::LoadLocal(_),
                Opcode::GetTag,
                Opcode::LoadConst(_),
                Opcode::EqTag,
                ..
            ]
        ) {
            Some(("local_tag_compare", 4usize))
        } else if matches!(
            remaining,
            [
                Opcode::EqLocalTag { .. },
                Opcode::JumpIfFalse(_) | Opcode::JumpIfTrue(_),
                ..
            ]
        ) {
            Some(("branch_fusion", 2usize))
        } else {
            None
        };

        let Some((kind, len)) = candidate else {
            continue;
        };
        *summary.entry(kind).or_default() += 1;
        items.push(peephole_candidate_json(bytecode, pc, kind, len));
    }

    json!({
        "summary": summary,
        "items": items
    })
}

fn peephole_candidate_json(bytecode: &Bytecode, pc: usize, kind: &str, len: usize) -> Value {
    let opcode_window = bytecode.opcodes[pc..pc + len]
        .iter()
        .map(|opcode| opcode.kind_name())
        .collect::<Vec<_>>();
    let function = function_for_pc(bytecode, pc).map(|entry| {
        entry
            .qualified_name
            .clone()
            .unwrap_or_else(|| format!("fun#{}", entry.fun_idx))
    });
    let source = opcode_source_for_pc(bytecode, pc);

    json!({
        "kind": kind,
        "pc": pc,
        "function": function,
        "source": source.map(|entry| json!({
            "line": entry.line,
            "column": entry.column,
            "span": [entry.span_start, entry.span_end],
            "source_name": entry.source_name
        })),
        "opcode_window": opcode_window
    })
}

fn function_for_pc(bytecode: &Bytecode, pc: usize) -> Option<&FunctionEntry> {
    bytecode.functions.iter().find(|entry| {
        let start = entry.entry_pc as usize;
        let end = if entry.end_pc > entry.entry_pc {
            entry.end_pc as usize
        } else {
            bytecode.opcodes.len()
        };
        pc >= start && pc < end
    })
}

fn opcode_source_for_pc(bytecode: &Bytecode, pc: usize) -> Option<OpcodeSource> {
    bytecode
        .source_map
        .as_ref()
        .and_then(|source_map| {
            source_map
                .entries
                .iter()
                .find(|entry| entry.opcode_index as usize == pc)
        })
        .cloned()
}

fn surface_strip_global_prefixes(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some(stripped) = text.strip_prefix("Global::") {
                *text = stripped.to_string();
            }
        }
        Value::Array(items) => {
            for item in items {
                surface_strip_global_prefixes(item);
            }
        }
        Value::Object(map) => {
            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let mut item = map
                    .remove(&key)
                    .expect("json object key should still exist during surface rewrite");
                surface_strip_global_prefixes(&mut item);
                let surface_key = key
                    .strip_prefix("Global::")
                    .unwrap_or(key.as_str())
                    .to_string();
                map.insert(surface_key, item);
            }
            for item in map.values_mut() {
                surface_strip_global_prefixes(item);
            }
        }
        _ => {}
    }
}
