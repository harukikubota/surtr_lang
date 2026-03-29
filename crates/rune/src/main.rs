use std::env;
use std::fs;
use std::process;

mod diagnostics;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 || args[1] != "run" {
        eprintln!("Usage: surtr run <file.srt>");
        process::exit(1);
    }

    let file_path = &args[2];
    let source = match fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", file_path, e);
            process::exit(1);
        }
    };

    if let Err(code) = run_pipeline(&source, file_path) {
        process::exit(code);
    }
}

fn run_pipeline(source: &str, file_path: &str) -> Result<(), i32> {
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
            diagnostics::report_error(
                file_path,
                source,
                "ResolveError",
                &e.message,
                &e.span,
                None,
            );
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

    // Phase 5: Eldr — execute
    let mut vm = eldr::VM::new(bytecode).with_source(source.to_string(), file_path.to_string());
    if let Err(e) = vm.run() {
        eprintln!("RuntimeError: {}", e.message);
        return Err(1);
    }

    Ok(())
}
