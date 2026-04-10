use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use crate::compile::{
    collect_default_script_compile_sources, compile_source, prepare_script_compile_plan,
    script_plan_error_as_rune_error,
};
use crate::error::{ExecutionEnv, RuneError, RuneResult};

pub(crate) fn dispatch(file_path: &str, args: &[String]) -> RuneResult<()> {
    let mut format = "json";
    let mut entry: Option<String> = None;
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
            other => {
                return Err(RuneError::message(
                    1,
                    format!("dump: unknown option '{}'", other),
                ));
            }
        }
        i += 1;
    }

    if format != "json" {
        return Err(RuneError::message(
            1,
            format!("dump: unsupported format '{}'. supported: json", format),
        ));
    }

    if Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("srt"))
    {
        return dump_entry_source_as_json(file_path, entry.as_deref(), ExecutionEnv::DumpSource);
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

    let dump_json = build_dump_json(file_path, &inspected, None)?;
    let text = serde_json::to_string(&dump_json)
        .map_err(|e| RuneError::message(1, format!("dump: failed to serialize json: {}", e)))?;
    println!("{}", text);
    Ok(())
}

fn dump_entry_source_as_json(
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

    let dump_json = build_dump_json(file_path, &inspected, Some(entrypoint_trace))?;
    let text = serde_json::to_string(&dump_json)
        .map_err(|e| RuneError::message(1, format!("dump: failed to serialize json: {}", e)))?;
    println!("{}", text);
    Ok(())
}

fn build_dump_json(
    file_path: &str,
    inspected: &forge::bytecode::EldrInspect,
    entrypoint_trace: Option<Value>,
) -> RuneResult<Value> {
    let header = serde_json::to_value(&inspected.header)
        .map_err(|e| RuneError::message(1, format!("dump: failed to serialize header: {}", e)))?;
    let chunks = serde_json::to_value(&inspected.chunks)
        .map_err(|e| RuneError::message(1, format!("dump: failed to serialize chunks: {}", e)))?;
    let bytecode = serde_json::to_value(&inspected.bytecode)
        .map_err(|e| RuneError::message(1, format!("dump: failed to serialize bytecode: {}", e)))?;

    Ok(json!({
        "file": file_path,
        "header": header,
        "chunks": chunks,
        "summary": {
            "opcode_count": inspected.bytecode.opcodes.len(),
            "constant_count": inspected.bytecode.constants.len(),
            "function_count": inspected.bytecode.functions.len(),
            "type_entry_count": inspected.bytecode.type_registry.entries.len(),
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
            "bytecode_version": inspected.bytecode.compile_info.bytecode_version
        },
        "entrypoint_trace": entrypoint_trace,
        "bytecode": bytecode
    }))
}
