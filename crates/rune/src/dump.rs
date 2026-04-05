use std::fs;
use std::path::Path;

use serde_json::{json, Value};

pub fn dump_command(file_path: &str, args: &[String]) -> Result<(), i32> {
    let mut format = "json";
    let mut entry: Option<String> = None;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("dump: missing value for --format");
                    return Err(1);
                }
                format = args[i].as_str();
            }
            "--entry" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("dump: missing value for --entry");
                    return Err(1);
                }
                if entry.is_some() {
                    eprintln!("dump: --entry may only be specified once");
                    return Err(1);
                }
                entry = Some(args[i].clone());
            }
            other => {
                eprintln!("dump: unknown option '{}'", other);
                return Err(1);
            }
        }
        i += 1;
    }

    if format != "json" {
        eprintln!("dump: unsupported format '{}'. supported: json", format);
        return Err(1);
    }

    if Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("srt"))
    {
        return dump_entry_source_as_json(file_path, entry.as_deref());
    }

    if entry.is_some() {
        eprintln!("dump: --entry is only supported for .srt input");
        return Err(1);
    }

    let bytes = match fs::read(file_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error reading {}: {}", file_path, e);
            return Err(1);
        }
    };

    let inspected = match forge::bytecode::Bytecode::inspect(&bytes) {
        Ok(info) => info,
        Err(e) => {
            eprintln!("Error decoding {}: {}", file_path, e);
            return Err(1);
        }
    };

    let dump_json = build_dump_json(file_path, &inspected, None)?;
    let text = match serde_json::to_string(&dump_json) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("dump: failed to serialize json: {}", e);
            return Err(1);
        }
    };
    println!("{}", text);
    Ok(())
}

fn dump_entry_source_as_json(file_path: &str, cli_entry: Option<&str>) -> Result<(), i32> {
    let source = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", file_path, e);
            return Err(1);
        }
    };

    let compile_plan = match super::prepare_script_compile_plan(file_path, &source, cli_entry) {
        Ok(plan) => plan,
        Err(e) => {
            let mut sources = diagnostics::SourceRegistry::new();
            let source_id = sources.register(file_path, source.clone());
            diagnostics::report_error_by_id(
                &sources,
                source_id,
                diagnostics::simple_error("ParseError", &e.message, e.span, None),
            );
            return Err(1);
        }
    };

    let module_sources = xldr::collect_module_sources_with_module_stages(&[]).map_err(|e| {
        eprintln!("Error collecting module sources: {}", e);
        1
    })?;
    let compile_sources = xldr::compose_script_compile_sources(
        file_path,
        &compile_plan.source_for_parse,
        module_sources,
    );
    let bytecode = super::compile_source(&compile_sources, &compile_plan)?;
    let bytes = match bytecode.encode() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("dump: failed to encode bytecode: {}", e);
            return Err(1);
        }
    };
    let inspected = match forge::bytecode::Bytecode::inspect(&bytes) {
        Ok(info) => info,
        Err(e) => {
            eprintln!("dump: failed to inspect compiled bytecode: {}", e);
            return Err(1);
        }
    };

    let entrypoint_trace = json!({
        "source": "entry_file",
        "selected_entry_name": compile_plan.selected_entry_name,
        "normalized_entrypoint": compile_plan
            .normalized_entrypoint
            .as_ref()
            .map(|entry| entry.qualified_symbol.clone())
    });

    let dump_json = build_dump_json(file_path, &inspected, Some(entrypoint_trace))?;
    let text = match serde_json::to_string(&dump_json) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("dump: failed to serialize json: {}", e);
            return Err(1);
        }
    };
    println!("{}", text);
    Ok(())
}

fn build_dump_json(
    file_path: &str,
    inspected: &forge::bytecode::EldrInspect,
    entrypoint_trace: Option<Value>,
) -> Result<Value, i32> {
    let header = match serde_json::to_value(&inspected.header) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("dump: failed to serialize header: {}", e);
            return Err(1);
        }
    };
    let chunks = match serde_json::to_value(&inspected.chunks) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("dump: failed to serialize chunks: {}", e);
            return Err(1);
        }
    };
    let bytecode = match serde_json::to_value(&inspected.bytecode) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("dump: failed to serialize bytecode: {}", e);
            return Err(1);
        }
    };

    Ok(json!({
        "file": file_path,
        "header": header,
        "chunks": chunks,
        "summary": {
            "opcode_count": inspected.bytecode.opcodes.len(),
            "constant_count": inspected.bytecode.constants.len(),
            "type_entry_count": inspected.bytecode.type_registry.entries.len(),
            "error_template_count": inspected.bytecode.error_templates.len(),
            "num_locals": inspected.bytecode.num_locals
        },
        "entrypoint_trace": entrypoint_trace,
        "bytecode": bytecode
    }))
}
