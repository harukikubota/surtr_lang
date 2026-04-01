use std::env;
use std::fs;
use std::path::Path;
use std::process;

use spire::ast::Span;

mod diagnostics;
mod dump;

const BUILTIN_PRELUDE_FILE: &str = "builtin.srt";
const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/builtin.srt");

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
        Some("repl") => {
            if args.len() != 2 {
                print_usage();
                Err(1)
            } else {
                xldr::repl_command()
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
                dump::dump_command(&args[2], &args[3..])
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
    eprintln!("  surtr repl");
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

fn default_output_path(input_srt: &str) -> String {
    let path = Path::new(input_srt);
    path.with_extension("eldr").to_string_lossy().into_owned()
}

fn parse_program_with_builtin_prelude(
    source: &str,
    file_path: &str,
) -> Result<Vec<spire::ast::Ast>, i32> {
    let mut ast = match spire::parse_with_source(BUILTIN_PRELUDE_SOURCE, BUILTIN_PRELUDE_FILE) {
        Ok(a) => a,
        Err(e) => {
            let message = e.message();
            diagnostics::report_error(
                BUILTIN_PRELUDE_FILE,
                BUILTIN_PRELUDE_SOURCE,
                diagnostics::simple_error("ParseError", message, e.span().clone(), None),
            );
            return Err(1);
        }
    };

    let mut user_ast = match spire::parse_with_source(source, file_path) {
        Ok(a) => a,
        Err(e) => {
            let message = e.message();
            diagnostics::report_error(
                file_path,
                source,
                diagnostics::simple_error("ParseError", message, e.span().clone(), None),
            );
            return Err(1);
        }
    };

    ast.append(&mut user_ast);
    Ok(ast)
}

fn compile_source(source: &str, file_path: &str) -> Result<forge::bytecode::Bytecode, i32> {
    // Phase 1: Spire — parse
    let ast = parse_program_with_builtin_prelude(source, file_path)?;

    // Phase 2: Sigil — resolve names
    let resolved = match sigil::resolve(ast) {
        Ok(r) => r,
        Err(e) => {
            diagnostics::report_error(
                file_path,
                source,
                diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None),
            );
            return Err(1);
        }
    };

    // Phase 3: Scar — type check
    let typed = match scar::typecheck(resolved) {
        Ok(t) => t,
        Err(e) => {
            diagnostics::report_error(file_path, source, diagnostics::type_error_spec(source, &e));
            return Err(1);
        }
    };

    // Phase 4: Forge — generate bytecode
    let mut bytecode = match forge::codegen(typed) {
        Ok(b) => b,
        Err(e) => {
            diagnostics::report_error(
                file_path,
                source,
                diagnostics::simple_error("CodegenError", &e.message, e.span.clone(), None),
            );
            return Err(1);
        }
    };

    populate_bytecode_source_map_lines(&mut bytecode, source, file_path);

    Ok(bytecode)
}

fn execute_bytecode(
    bytecode: forge::bytecode::Bytecode,
    source_context: Option<(String, String)>,
) -> Result<(), i32> {
    // Phase 5: Eldr — execute
    let mut vm = match source_context.as_ref() {
        Some((source, file_path)) => {
            eldr::VM::new(bytecode).with_source(source.clone(), file_path.clone())
        }
        None => eldr::VM::new(bytecode),
    };
    if let Err(e) = vm.run() {
        if let (Some(location), Some((source, file_path))) = (e.location.as_ref(), source_context.as_ref()) {
            diagnostics::report_error(
                file_path,
                source,
                diagnostics::simple_error(
                    "RuntimeError",
                    &e.message,
                    Span::with_source(
                        location.span_start as usize,
                        location.span_end as usize,
                        Some(location.file.clone()),
                    ),
                    None,
                ),
            );
        } else if let Some(location) = e.location.as_ref() {
            eprintln!(
                "RuntimeError: {} ({}:{}:{})",
                e.message, location.file, location.line, location.column
            );
        } else {
            eprintln!("RuntimeError: {}", e.message);
        }
        return Err(1);
    }

    Ok(())
}

fn populate_bytecode_source_map_lines(
    bytecode: &mut forge::bytecode::Bytecode,
    source: &str,
    file_path: &str,
) {
    let Some(source_map) = bytecode.source_map.as_mut() else {
        return;
    };

    let line_spans = source_line_spans(source);
    for entry in &mut source_map.entries {
        let pos = entry.span_start as usize;
        entry.source_name = Some(file_path.to_string());
        if let Some((line_idx, line_start)) = source_line_and_start(&line_spans, pos) {
            entry.line = (line_idx + 1) as u32;
            entry.column = (pos.saturating_sub(line_start) + 1) as u32;
        }
    }
}

fn source_line_spans(source: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = source.chars().collect();
    let mut spans = Vec::new();
    let mut start = 0usize;

    for (idx, ch) in chars.iter().enumerate() {
        if *ch == '\n' {
            spans.push((start, idx));
            start = idx + 1;
        }
    }

    spans.push((start, chars.len()));
    spans
}

fn source_line_and_start(line_spans: &[(usize, usize)], pos: usize) -> Option<(usize, usize)> {
    line_spans
        .iter()
        .enumerate()
        .find(|(_, (start, end))| pos >= *start && pos <= *end)
        .map(|(idx, (start, _))| (idx, *start))
}
