use std::collections::BTreeSet;
use std::io::{self, IsTerminal, Write};

use diagnostics::{SourceId, SourceRegistry};
use eldr::builtin::inspect_value;
use eldr::value::Value;
use forge::bytecode::populate_error_template_lines;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::history::DefaultHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Context, Editor, Helper};
use sindr::builtin::BUILTIN_METAS;

mod loader;

pub use loader::{
    collect_compile_sources, collect_compile_sources_with_module_file_stages,
    collect_compile_sources_with_module_stages, collect_compile_sources_with_modules,
    CompileSources, LoadError, ModuleInput, StagedModule,
};

const XLDR_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerMode {
    Light,
    Detailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplOptions {
    pub quiet: bool,
    pub banner: BannerMode,
    pub version: bool,
}

impl Default for ReplOptions {
    fn default() -> Self {
        Self {
            quiet: false,
            banner: BannerMode::Light,
            version: false,
        }
    }
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
    sources: SourceRegistry,
    builtin_source_id: SourceId,
    builtin_module_path: Option<String>,
    repl_source_id: SourceId,
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
    fn new() -> Result<Self, LoadError> {
        let repl_sources = loader::collect_repl_sources()?;
        let forge_session = forge::ForgeSession::new();
        let vm = eldr::VM::new_interactive(forge_session.type_registry());
        let mut engine = Self {
            sources: repl_sources.sources,
            builtin_source_id: repl_sources.builtin_source_id,
            builtin_module_path: repl_sources.builtin_module_path,
            repl_source_id: repl_sources.repl_source_id,
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
        Ok(engine)
    }

    fn bootstrap_builtins(&mut self) {
        let builtin_source = self
            .sources
            .source(self.builtin_source_id)
            .unwrap_or("")
            .to_string();
        let ast = match spire::parse_with_context(
            &builtin_source,
            spire::ParserContext::module(
                self.builtin_source_id.0,
                self.builtin_module_path.clone(),
            ),
        ) {
            Ok(ast) => ast,
            Err(e) => {
                diagnostics::report_error_by_id(
                    &self.sources,
                    self.builtin_source_id,
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
                diagnostics::report_error_by_id(
                    &self.sources,
                    self.builtin_source_id,
                    diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None),
                );
                return;
            }
        };

        let typed = match self.scar_session.typecheck(resolved) {
            Ok(t) => t,
            Err(e) => {
                diagnostics::report_error_by_id(
                    &self.sources,
                    self.builtin_source_id,
                    diagnostics::type_error_spec_by_id(&self.sources, self.builtin_source_id, &e),
                );
                return;
            }
        };

        let (mut chunk, meta) = match self.forge_session.codegen_chunk(typed) {
            Ok(c) => c,
            Err(e) => {
                diagnostics::report_error_by_id(
                    &self.sources,
                    self.builtin_source_id,
                    diagnostics::simple_error("CodegenError", &e.message, e.span.clone(), None),
                );
                return;
            }
        };

        populate_error_template_lines(&mut chunk.error_templates, &builtin_source);
        if let Some((source, file_name)) = self.sources.owned_context(self.builtin_source_id) {
            self.vm.set_source(source, file_name);
        }

        if let Err(e) = self.vm.push_atomic(chunk) {
            eldr::report_runtime_error(
                &e,
                self.vm.source(),
                self.vm.source_file(),
                self.vm.runtime_error_location(),
            );
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
        self.sources
            .update_source(self.repl_source_id, self.pending.clone());

        let ast = match spire::parse_with_context(
            &self.pending,
            spire::ParserContext::repl(self.repl_source_id.0),
        ) {
            Ok(ast) => ast,
            Err(e) if e.is_incomplete() => {
                return ReplOutcome::Continue;
            }
            Err(e) => {
                let message = e.message();
                diagnostics::report_error_by_id(
                    &self.sources,
                    self.repl_source_id,
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
                diagnostics::report_error_by_id(
                    &self.sources,
                    self.repl_source_id,
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
                diagnostics::report_error_by_id(
                    &self.sources,
                    self.repl_source_id,
                    diagnostics::type_error_spec_by_id(&self.sources, self.repl_source_id, &e),
                );
                self.pending.clear();
                self.bump_line(None);
                return ReplOutcome::Continue;
            }
        };

        let (mut chunk, meta) = match self.forge_session.codegen_chunk_repl_result(typed) {
            Ok(c) => c,
            Err(e) => {
                self.sigil_session.rollback(sigil_cp);
                self.scar_session.rollback(scar_cp);
                self.forge_session.rollback(forge_cp);
                diagnostics::report_error_by_id(
                    &self.sources,
                    self.repl_source_id,
                    diagnostics::simple_error("CodegenError", &e.message, e.span.clone(), None),
                );
                self.pending.clear();
                self.bump_line(None);
                return ReplOutcome::Continue;
            }
        };

        if let Some(repl_source) = self.sources.source(self.repl_source_id) {
            populate_error_template_lines(&mut chunk.error_templates, repl_source);
        }
        if let Some((source, file_name)) = self.sources.owned_context(self.repl_source_id) {
            self.vm.set_source(source, file_name);
        }

        match self.vm.push_atomic(chunk) {
            Ok(value) => {
                if self.report_main_result_error_if_any(&value) {
                    self.bump_line(None);
                } else {
                    display_repl_result(&self.vm, value.clone(), &meta);
                    for b in &meta.bindings {
                        self.symbols.insert(b.name.clone());
                    }
                    for name in &meta.function_defs {
                        self.symbols.insert(name.clone());
                    }
                    self.bump_line(Some(value));
                }
            }
            Err(e) => {
                eldr::report_runtime_error(
                    &e,
                    self.vm.source(),
                    self.vm.source_file(),
                    self.vm.runtime_error_location(),
                );
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
                println!("> {}", inspect_value(&self.vm, &value));
                self.bump_line(Some(value));
            }
            None => {
                eprintln!("Line {} has no value", line_num);
                self.bump_line(None);
            }
        }
    }

    fn report_main_result_error_if_any(&self, value: &Value) -> bool {
        match value {
            Value::Tagged { tag: 1, fields } => {
                if let Some(err_value) = fields.first() {
                    self.report_error_value(err_value);
                } else {
                    eprintln!("Error: InvalidResult: missing Err payload");
                }
                true
            }
            _ => false,
        }
    }

    fn report_error_value(&self, value: &Value) {
        match value {
            Value::Error(rich) => {
                let start = rich.location.span_start as usize;
                let mut end = rich.location.span_end as usize;
                if end <= start {
                    end = start.saturating_add(1);
                }
                diagnostics::report_error_by_id(
                    &self.sources,
                    self.repl_source_id,
                    diagnostics::simple_error(
                        rich.kind.clone(),
                        rich.message.clone(),
                        spire::ast::Span { start, end },
                        None,
                    ),
                );
            }
            other => {
                eprintln!("Error: {}", inspect_value(&self.vm, other));
            }
        }
    }

    fn bump_line(&mut self, value: Option<Value>) {
        self.results.push(value);
        self.next_line += 1;
    }
}

pub fn repl_command(options: ReplOptions) -> Result<(), i32> {
    if options.version {
        println!("xldr {}", XLDR_VERSION);
        return Ok(());
    }

    if !options.quiet {
        print_banner(options.banner);
    }

    let mut engine = ReplEngine::new().map_err(|e| {
        eprintln!("Error initializing source loader: {}", e);
        1
    })?;

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

fn print_banner(mode: BannerMode) {
    match mode {
        BannerMode::Light => {
            println!("Surtr xldr {}", XLDR_VERSION);
        }
        BannerMode::Detailed => {
            println!(
                r"
    ██\   ██\ ██\      ██████\  ██████\  
    ╚██\ ██  |██ |     ██  __██\ ██  __██\ 
     ╚████  / ██ |     ██ /  ██ |██ |  ██ |
     ██  ██<  ██ |     ██ |  ██ |██████  |
    ██  /\██\ ██ |     ██ |  ██ |██  __██< 
    ██ /  ██ |███████\ ██████  |██ |  ██ |
    \__|  \__|\_______|\______/ \__|  \__|
    
    "
            );
        }
    }
}

fn display_repl_result(vm: &eldr::VM, value: Value, meta: &forge::ChunkMeta) {
    if !matches!(value, Value::Unit) {
        println!("> {}", inspect_value(vm, &value));
        return;
    }

    if !meta.bindings.is_empty() {
        for binding in &meta.bindings {
            if let Some(val) = vm.get_local(binding.slot_id) {
                println!(
                    "> {}: {} = {}",
                    binding.name,
                    binding.ty,
                    inspect_value(vm, &val)
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
