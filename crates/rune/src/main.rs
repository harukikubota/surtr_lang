use std::env;
use std::fs;
use std::path::Path;
use std::process;

use serde_json::{json, Value};

mod diagnostics;

fn main() {
    let args: Vec<String> = env::args().collect();

    let result = match args.get(1).map(String::as_str) {
        Some("run") => {
            if args.len() != 3 {
                print_usage();
                Err(1)
            } else {
                run_command(&args[2])
            }
        }
        Some("build") => {
            if !(3..=4).contains(&args.len()) {
                print_usage();
                Err(1)
            } else {
                build_command(&args[2], args.get(3).map(String::as_str))
            }
        }
        Some("dump") => {
            if args.len() < 3 {
                print_usage();
                Err(1)
            } else {
                dump_command(&args[2], &args[3..])
            }
        }
        _ => {
            print_usage();
            Err(1)
        }
    };

    if let Err(code) = result {
        process::exit(code);
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  surtr run <file.srt|file.eldr>");
    eprintln!("  surtr build <file.srt> [output.eldr]");
    eprintln!("  surtr dump <file.eldr> [--format json]");
}

fn run_command(file_path: &str) -> Result<(), i32> {
    if file_path.ends_with(".eldr") {
        run_eldr_file(file_path)
    } else {
        run_source_file(file_path)
    }
}

fn run_source_file(file_path: &str) -> Result<(), i32> {
    let source = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", file_path, e);
            return Err(1);
        }
    };

    let bytecode = compile_source(&source, file_path)?;
    execute_bytecode(bytecode, Some((source, file_path.to_string())))
}

fn run_eldr_file(file_path: &str) -> Result<(), i32> {
    let bytes = match fs::read(file_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error reading {}: {}", file_path, e);
            return Err(1);
        }
    };

    let bytecode = match forge::bytecode::Bytecode::decode(&bytes) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error decoding {}: {}", file_path, e);
            return Err(1);
        }
    };

    execute_bytecode(bytecode, None)
}

fn build_command(input_srt: &str, output_eldr: Option<&str>) -> Result<(), i32> {
    let source = match fs::read_to_string(input_srt) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", input_srt, e);
            return Err(1);
        }
    };

    let bytecode = compile_source(&source, input_srt)?;
    let bytes = match bytecode.encode() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error encoding bytecode: {}", e);
            return Err(1);
        }
    };

    let output_path = output_eldr
        .map(ToString::to_string)
        .unwrap_or_else(|| default_output_path(input_srt));
    if let Err(e) = fs::write(&output_path, bytes) {
        eprintln!("Error writing {}: {}", output_path, e);
        return Err(1);
    }
    Ok(())
}

fn dump_command(file_path: &str, args: &[String]) -> Result<(), i32> {
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

fn default_output_path(input_srt: &str) -> String {
    let path = Path::new(input_srt);
    path.with_extension("eldr").to_string_lossy().into_owned()
}

fn build_dump_json(file_path: &str, inspected: &forge::bytecode::EldrInspect) -> Result<Value, i32> {
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

fn compile_source(source: &str, file_path: &str) -> Result<forge::bytecode::Bytecode, i32> {
    // Phase 1: Spire — parse
    let ast = match spire::parse(source) {
        Ok(a) => a,
        Err(e) => {
            diagnostics::report_error(file_path, source, "ParseError", &e.message, &e.span, None);
            return Err(1);
        }
    };

    // Phase 2: Sigil — resolve names
    let resolved = match sigil::resolve(ast) {
        Ok(r) => r,
        Err(e) => {
            diagnostics::report_error(file_path, source, "ResolveError", &e.message, &e.span, None);
            return Err(1);
        }
    };

    // Phase 3: Scar — type check
    let typed = match scar::typecheck(resolved) {
        Ok(t) => t,
        Err(e) => {
            diagnostics::report_error(
                file_path,
                source,
                "TypeError",
                &e.message,
                &e.span,
                e.hint.as_deref(),
            );
            return Err(1);
        }
    };

    // Phase 4: Forge — generate bytecode
    let bytecode = match forge::codegen(typed) {
        Ok(b) => b,
        Err(e) => {
            diagnostics::report_error(file_path, source, "CodegenError", &e.message, &e.span, None);
            return Err(1);
        }
    };

    Ok(bytecode)
}

fn execute_bytecode(
    bytecode: forge::bytecode::Bytecode,
    source_context: Option<(String, String)>,
) -> Result<(), i32> {
    // Phase 5: Eldr — execute
    let mut vm = match source_context {
        Some((source, file_path)) => eldr::VM::new(bytecode).with_source(source, file_path),
        None => eldr::VM::new(bytecode),
    };
    if let Err(e) = vm.run() {
        eprintln!("RuntimeError: {}", e.message);
        return Err(1);
    }

    Ok(())
}
