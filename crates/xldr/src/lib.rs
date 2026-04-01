use std::collections::BTreeSet;
use std::io::{self, IsTerminal, Write};

use eldr::value::Value;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::history::DefaultHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Context, Editor, Helper};
use sindr::builtin::BUILTIN_METAS;
use sindr::ir::BytecodeChunk;

mod diagnostics;

const BUILTIN_PRELUDE_FILE: &str = "builtin.srt";
const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/builtin.srt");

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
        let vm = eldr::VM::new_interactive(forge_session.type_registry())
            .with_source(BUILTIN_PRELUDE_SOURCE.to_string(), BUILTIN_PRELUDE_FILE.to_string())
            .with_output_capture()
            .with_error_capture();
        let mut engine = Self {
            sigil_session: sigil::SigilSession::new(),
            scar_session: scar::ScarSession::new(),
            forge_session,
            vm,
            pending: String::new(),
            next_line: 1,
            results: Vec::new(),
            symbols: ["Ok", "Err"]
                .into_iter()
                .map(str::to_string)
                .chain(BUILTIN_METAS.iter().map(|meta| meta.name.to_string()))
                .collect(),
        };
        engine.bootstrap_builtins();
        engine
    }

    fn bootstrap_builtins(&mut self) {
        let ast = match spire::parse_with_source(BUILTIN_PRELUDE_SOURCE, BUILTIN_PRELUDE_FILE) {
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

        let (mut chunk, meta) = match self.forge_session.codegen_chunk(typed) {
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

        populate_chunk_source_map_lines(&mut chunk, BUILTIN_PRELUDE_FILE, BUILTIN_PRELUDE_SOURCE);

        if let Err(e) = self.vm.push_atomic(chunk) {
            report_runtime_error(&self.vm, &e, BUILTIN_PRELUDE_SOURCE, BUILTIN_PRELUDE_FILE);
            return;
        }

        flush_captured_output(&mut self.vm);

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

        let ast = match spire::parse_with_source(&self.pending, "repl") {
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

        let (mut chunk, meta) = match self.forge_session.codegen_chunk(typed) {
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

        self.vm.add_source(self.pending.clone(), "repl".to_string());
        populate_chunk_source_map_lines(&mut chunk, "repl", &self.pending);

        match self.vm.push_atomic(chunk) {
            Ok(value) => {
                flush_captured_output(&mut self.vm);
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
                report_runtime_error(&self.vm, &e, &self.pending, "repl");
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
                println!("> {}", value.to_display_string(&registry));
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

pub fn repl_command() -> Result<(), i32> {
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
        println!("> {}", value.to_display_string(&registry));
        return;
    }

    if !meta.bindings.is_empty() {
        for binding in &meta.bindings {
            if let Some(val) = vm.get_local(binding.slot_id) {
                println!(
                    "> {}: {} = {}",
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
            println!("> {}", type_def.name);
        }
    }
}

fn flush_captured_output(vm: &mut eldr::VM) {
    if let Some(buf) = vm.output.as_mut() {
        for line in buf.drain(..) {
            println!("{}", line);
        }
    }

    if let Some(buf) = vm.error_output.as_mut() {
        for line in buf.drain(..) {
            eprintln!("{}", line);
        }
    }
}

fn populate_chunk_source_map_lines(chunk: &mut BytecodeChunk, source_name: &str, source: &str) {
    let Some(source_map) = chunk.source_map.as_mut() else {
        return;
    };

    let line_spans = source_line_spans(source);
    for entry in &mut source_map.entries {
        let pos = entry.span_start as usize;
        entry.source_name = Some(source_name.to_string());
        if let Some((line_idx, line_start)) = source_line_and_start(&line_spans, pos) {
            entry.line = (line_idx + 1) as u32;
            entry.column = (pos.saturating_sub(line_start) + 1) as u32;
        }
    }
}

fn report_runtime_error(vm: &eldr::VM, err: &eldr::error::RuntimeError, fallback_source: &str, fallback_file: &str) {
    if let Some(location) = err.location.as_ref() {
        if let Some(source) = vm.source_for_file(&location.file) {
            diagnostics::report_error(
                &location.file,
                source,
                diagnostics::simple_error(
                    "RuntimeError",
                    &err.message,
                    spire::ast::Span::with_source(
                        location.span_start as usize,
                        location.span_end as usize,
                        Some(location.file.clone()),
                    ),
                    None,
                ),
            );
            return;
        }

        if location.file == fallback_file {
            diagnostics::report_error(
                fallback_file,
                fallback_source,
                diagnostics::simple_error(
                    "RuntimeError",
                    &err.message,
                    spire::ast::Span::with_source(
                        location.span_start as usize,
                        location.span_end as usize,
                        Some(location.file.clone()),
                    ),
                    None,
                ),
            );
            return;
        }

        eprintln!(
            "RuntimeError: {} ({}:{}:{})",
            err.message, location.file, location.line, location.column
        );
        return;
    }

    eprintln!("RuntimeError: {}", err.message);
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
