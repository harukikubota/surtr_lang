use std::env;
use std::fs;
use std::path::Path;
use std::process;

use forge::bytecode::populate_error_template_lines;

mod diagnostics;
mod dump;

const BUILTIN_PRELUDE_FILE: &str = "builtin.srt";
const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/builtin.srt");
const RUNE_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = env::args().collect();

    let result = match args.get(1).map(String::as_str) {
        Some("--version") => {
            println!("surtr {}", RUNE_VERSION);
            Ok(())
        }
        Some("run") => {
            if args.len() != 3 {
                print_usage();
                Err(1)
            } else {
                run_command(&args[2])
            }
        }
        Some("repl") => parse_repl_options(&args[2..]).and_then(xldr::repl_command),
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
    eprintln!("  surtr --version");
    eprintln!("  surtr run <file.srt|file.eldr>");
    eprintln!("  surtr repl [--quiet] [--banner] [--version]");
    eprintln!("  surtr build <file.srt> [output.eldr]");
    eprintln!("  surtr dump <file.eldr> [--format json]");
}

fn parse_repl_options(args: &[String]) -> Result<xldr::ReplOptions, i32> {
    let mut options = xldr::ReplOptions::default();

    for arg in args {
        match arg.as_str() {
            "--quiet" => options.quiet = true,
            "--banner" => options.banner = xldr::BannerMode::Detailed,
            "--version" => options.version = true,
            other => {
                eprintln!("repl: unknown option '{}'", other);
                print_usage();
                return Err(1);
            }
        }
    }

    Ok(options)
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
    let mut ast = match spire::parse(BUILTIN_PRELUDE_SOURCE) {
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

    let mut user_ast = match spire::parse(source) {
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

    populate_error_template_lines(&mut bytecode.error_templates, source);

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
        eldr::report_runtime_error(
            &e,
            vm.source(),
            vm.source_file(),
            vm.runtime_error_location(),
        );
        return Err(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::populate_error_template_lines;
    use forge::bytecode::{line_column_for_offset, ErrTemplate};

    #[test]
    fn line_column_for_offset_tracks_multiline_source() {
        let source = "deferror Boom {\n  \"boom\"\n}\n";
        assert_eq!(line_column_for_offset(source, 0), (1, 1));
        assert_eq!(line_column_for_offset(source, 16), (2, 1));
    }

    #[test]
    fn populate_error_template_lines_uses_span_start() {
        let source = "deferror Boom {\n  \"boom\"\n}\n";
        let mut templates = vec![ErrTemplate {
            id: 0,
            kind: "Boom".into(),
            span_start: 16,
            span_end: 24,
            line: 0,
            column: 0,
            format: "{}".into(),
            num_params: 1,
        }];

        populate_error_template_lines(&mut templates, source);

        assert_eq!(templates[0].line, 2);
        assert_eq!(templates[0].column, 1);
    }
}
