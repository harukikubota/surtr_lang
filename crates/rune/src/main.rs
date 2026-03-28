use std::env;
use std::fs;
use std::process;

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

    if let Err(e) = run_pipeline(&source, file_path) {
        eprintln!("{}", e);
        process::exit(1);
    }
}

fn run_pipeline(source: &str, file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Phase 1: Spire — parse
    let ast = spire::parse(source)?;

    // Phase 2: Sigil — resolve names
    let resolved = sigil::resolve(ast)?;

    // Phase 3: Scar — type check
    let typed = scar::typecheck(resolved)?;

    // Phase 4: Forge — generate bytecode
    let bytecode = forge::codegen(typed)?;

    // Phase 5: Eldr — execute
    let mut vm = eldr::VM::new(bytecode)
        .with_source(source.to_string(), file_path.to_string());
    vm.run()?;

    Ok(())
}
