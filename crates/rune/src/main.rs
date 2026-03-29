use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process;

use eldr::value::Value;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::history::DefaultHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Context, Editor, Helper};

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
                repl_command()
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

struct ReplHelper {
    hinter: HistoryHinter,
    symbols: Vec<String>,
}

impl ReplHelper {
    fn new() -> Self {
        Self {
            hinter: HistoryHinter {},
            symbols: Vec::new(),
        }
    }

    fn set_symbols(&mut self, symbols: Vec<String>) {
        self.symbols = symbols;
    }
}

impl Helper for ReplHelper {}
impl Highlighter for ReplHelper {}

impl Validator for ReplHelper {
    fn validate(
        &self,
        _ctx: &mut ValidationContext<'_>,
    ) -> Result<ValidationResult, ReadlineError> {
        Ok(ValidationResult::Valid(None))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<Self::Hint> {
        self.hinter.hint(line, pos, ctx)
    }
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        const COMMANDS: &[&str] = &[":quit", ":v"];
        let start = line[..pos]
            .rfind(char::is_whitespace)
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let word = &line[start..pos];

        let mut matches = Vec::new();
        for cmd in COMMANDS {
            if cmd.starts_with(word) {
                matches.push(Pair {
                    display: cmd.to_string(),
                    replacement: cmd.to_string(),
                });
            }
        }
        for symbol in &self.symbols {
            if symbol.starts_with(word) {
                matches.push(Pair {
                    display: symbol.clone(),
                    replacement: symbol.clone(),
                });
            }
        }
        Ok((start, matches))
    }
}

enum ReplOutcome {
    Continue,
    Exit,
}

struct ReplEngine {
    sigil_session: sigil::SigilSession,
    scar_session: scar::ScarSession,
    forge_session: forge::ForgeSession,
    vm: eldr::VM,
    pending: String,
    next_line: usize,
    results: Vec<Option<Value>>,
    symbols: BTreeSet<String>,
}

impl ReplEngine {
    fn new() -> Self {
        let forge_session = forge::ForgeSession::new();
        let vm = eldr::VM::new_interactive(forge_session.type_registry());
        let mut engine = Self {
            sigil_session: sigil::SigilSession::new(),
            scar_session: scar::ScarSession::new(),
            forge_session,
            vm,
            pending: String::new(),
            next_line: 1,
            results: Vec::new(),
            symbols: ["Ok", "Err", "print", "to_string", "eprint"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        };
        engine.bootstrap_builtins();
        engine
    }

    fn bootstrap_builtins(&mut self) {
        let ast = match spire::parse(BUILTIN_PRELUDE_SOURCE) {
            Ok(ast) => ast,
            Err(e) => {
                diagnostics::report_error(
                    BUILTIN_PRELUDE_FILE,
                    BUILTIN_PRELUDE_SOURCE,
                    diagnostics::simple_error("ParseError", e.message(), e.span().clone(), None),
                );
                return;
            }
        };

        if ast.is_empty() {
            return;
        }

        let resolved = match self.sigil_session.resolve(ast) {
            Ok(r) => r,
            Err(e) => {
                diagnostics::report_error(
                    BUILTIN_PRELUDE_FILE,
                    BUILTIN_PRELUDE_SOURCE,
                    diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None),
                );
                return;
            }
        };

        let typed = match self.scar_session.typecheck(resolved) {
            Ok(t) => t,
            Err(e) => {
                diagnostics::report_error(
                    BUILTIN_PRELUDE_FILE,
                    BUILTIN_PRELUDE_SOURCE,
                    diagnostics::type_error_spec(BUILTIN_PRELUDE_SOURCE, &e),
                );
                return;
            }
        };

        let (chunk, meta) = match self.forge_session.codegen_chunk(typed) {
            Ok(c) => c,
            Err(e) => {
                diagnostics::report_error(
                    BUILTIN_PRELUDE_FILE,
                    BUILTIN_PRELUDE_SOURCE,
                    diagnostics::simple_error("CodegenError", &e.message, e.span.clone(), None),
                );
                return;
            }
        };

        if let Err(e) = self.vm.push(chunk) {
            eprintln!("RuntimeError (builtin prelude): {}", e.message);
            return;
        }

        for name in &meta.function_defs {
            self.symbols.insert(name.clone());
        }
    }

    fn completion_symbols(&self) -> Vec<String> {
        self.symbols.iter().cloned().collect()
    }

    fn prompt(&self) -> String {
        if self.pending.is_empty() {
            format!("surtr({})> ", self.next_line)
        } else {
            format!("...({})> ", self.next_line)
        }
    }

    fn handle_line(&mut self, line: &str) -> ReplOutcome {
        if self.pending.is_empty() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return ReplOutcome::Continue;
            }
            if trimmed == ":quit" {
                return ReplOutcome::Exit;
            }
            if let Some(rest) = trimmed.strip_prefix(":v") {
                self.handle_value_recall(rest.trim());
                return ReplOutcome::Continue;
            }
            if trimmed.starts_with(':') {
                eprintln!("Unknown REPL command: {}", trimmed);
                self.bump_line(None);
                return ReplOutcome::Continue;
            }
        }

        self.pending.push_str(line);
        self.pending.push('\n');

        let ast = match spire::parse(&self.pending) {
            Ok(ast) => ast,
            Err(e) if e.is_incomplete() => {
                return ReplOutcome::Continue;
            }
            Err(e) => {
                let message = e.message();
                diagnostics::report_error(
                    "repl",
                    &self.pending,
                    diagnostics::simple_error("ParseError", message, e.span().clone(), None),
                );
                self.pending.clear();
                self.bump_line(None);
                return ReplOutcome::Continue;
            }
        };

        if ast.is_empty() {
            self.pending.clear();
            return ReplOutcome::Continue;
        }

        let sigil_cp = self.sigil_session.checkpoint();
        let scar_cp = self.scar_session.checkpoint();
        let forge_cp = self.forge_session.checkpoint();

        let resolved = match self.sigil_session.resolve(ast) {
            Ok(r) => r,
            Err(e) => {
                self.sigil_session.rollback(sigil_cp);
                self.scar_session.rollback(scar_cp);
                self.forge_session.rollback(forge_cp);
                diagnostics::report_error(
                    "repl",
                    &self.pending,
                    diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None),
                );
                self.pending.clear();
                self.bump_line(None);
                return ReplOutcome::Continue;
            }
        };

        let typed = match self.scar_session.typecheck(resolved) {
            Ok(t) => t,
            Err(e) => {
                self.sigil_session.rollback(sigil_cp);
                self.scar_session.rollback(scar_cp);
                self.forge_session.rollback(forge_cp);
                diagnostics::report_error(
                    "repl",
                    &self.pending,
                    diagnostics::type_error_spec(&self.pending, &e),
                );
                self.pending.clear();
                self.bump_line(None);
                return ReplOutcome::Continue;
            }
        };

        let (chunk, meta) = match self.forge_session.codegen_chunk(typed) {
            Ok(c) => c,
            Err(e) => {
                self.sigil_session.rollback(sigil_cp);
                self.scar_session.rollback(scar_cp);
                self.forge_session.rollback(forge_cp);
                diagnostics::report_error(
                    "repl",
                    &self.pending,
                    diagnostics::simple_error("CodegenError", &e.message, e.span.clone(), None),
                );
                self.pending.clear();
                self.bump_line(None);
                return ReplOutcome::Continue;
            }
        };

        match self.vm.push(chunk) {
            Ok(value) => {
                display_repl_result(&self.vm, value.clone(), &meta);
                for b in &meta.bindings {
                    self.symbols.insert(b.name.clone());
                }
                for name in &meta.function_defs {
                    self.symbols.insert(name.clone());
                }
                self.bump_line(Some(value));
            }
            Err(e) => {
                eprintln!("RuntimeError: {}", e.message);
                self.bump_line(None);
            }
        }

        self.pending.clear();
        ReplOutcome::Continue
    }

    fn handle_value_recall(&mut self, arg: &str) {
        if arg.is_empty() {
            eprintln!("Usage: :v <line>");
            self.bump_line(None);
            return;
        }

        let line_num = match arg.parse::<usize>() {
            Ok(n) if n > 0 => n,
            _ => {
                eprintln!("Invalid line number for :v: {}", arg);
                self.bump_line(None);
                return;
            }
        };

        if line_num > self.results.len() {
            eprintln!("No such line: {}", line_num);
            self.bump_line(None);
            return;
        }

        match self.results[line_num - 1].clone() {
            Some(value) => {
                let registry = self.vm.type_registry();
                println!("{}", value.to_display_string(&registry));
                self.bump_line(Some(value));
            }
            None => {
                eprintln!("Line {} has no value", line_num);
                self.bump_line(None);
            }
        }
    }

    fn bump_line(&mut self, value: Option<Value>) {
        self.results.push(value);
        self.next_line += 1;
    }
}

fn repl_command() -> Result<(), i32> {
    let mut engine = ReplEngine::new();

    if io::stdin().is_terminal() {
        let mut editor: Editor<ReplHelper, DefaultHistory> = Editor::new().map_err(|e| {
            eprintln!("Error initializing line editor: {}", e);
            1
        })?;
        editor.set_helper(Some(ReplHelper::new()));

        loop {
            let prompt = engine.prompt();
            let symbols = engine.completion_symbols();
            if let Some(helper) = editor.helper_mut() {
                helper.set_symbols(symbols);
            }
            match editor.readline(&prompt) {
                Ok(line) => {
                    if !line.trim().is_empty() {
                        let _ = editor.add_history_entry(line.as_str());
                    }
                    if matches!(engine.handle_line(&line), ReplOutcome::Exit) {
                        break;
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    return Err(130);
                }
                Err(ReadlineError::Eof) => {
                    break;
                }
                Err(e) => {
                    eprintln!("Error reading input: {}", e);
                    return Err(1);
                }
            }
        }
    } else {
        let stdin = io::stdin();
        loop {
            print!("{}", engine.prompt());
            if io::stdout().flush().is_err() {
                return Err(1);
            }

            let mut line = String::new();
            let read = match stdin.read_line(&mut line) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("Error reading input: {}", e);
                    return Err(1);
                }
            };
            if read == 0 {
                break;
            }
            let line = line.trim_end_matches(&['\r', '\n'][..]);
            if matches!(engine.handle_line(line), ReplOutcome::Exit) {
                break;
            }
        }
    }

    Ok(())
}

fn display_repl_result(vm: &eldr::VM, value: Value, meta: &forge::ChunkMeta) {
    let registry = vm.type_registry();

    if !matches!(value, Value::Unit) {
        println!("{}", value.to_display_string(&registry));
        return;
    }

    if !meta.bindings.is_empty() {
        for binding in &meta.bindings {
            if let Some(val) = vm.get_local(binding.slot_id) {
                println!(
                    "{}: {} = {}",
                    binding.name,
                    binding.ty,
                    val.to_display_string(&registry)
                );
            }
        }
        return;
    }

    if !meta.type_defs.is_empty() {
        for type_def in &meta.type_defs {
            println!("{}", type_def.name);
        }
    }
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
    let bytecode = match forge::codegen(typed) {
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
