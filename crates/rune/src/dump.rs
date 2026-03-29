use std::fs;

use serde_json::{json, Value};

pub fn dump_command(file_path: &str, args: &[String]) -> Result<(), i32> {
    let mut format = "json";
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

    let dump_json = build_dump_json(file_path, &inspected)?;
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
        "bytecode": bytecode
    }))
}
