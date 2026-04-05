use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

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
use sigil::error::ResolveError;
use sindr::builtin::BUILTIN_METAS;
use spire::ast::{Ast, ImportSpec, Span};
use spire::token::Token;

mod loader;

pub use loader::{
    collect_module_sources_with_module_file_stages, collect_module_sources_with_module_stages,
    collect_module_sources_with_modules, collect_module_sources_with_std_module_stages,
    compose_script_compile_sources, script_pseudo_module_path, CompileSources, LoadError,
    ModuleInput, ModuleSources, SourceDescriptor, SourceKind, StagedModule,
};

#[derive(Debug, Clone, PartialEq)]
pub struct LoweredModuleAst {
    pub module_path: String,
    pub ast: Vec<spire::ast::Ast>,
    pub declared_span: Option<spire::ast::Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModuleStageParseErrorKind {
    Parse {
        message: String,
        span: spire::ast::Span,
    },
    DuplicateModulePath {
        module_path: String,
        first_file_name: String,
        second_file_name: String,
        span: spire::ast::Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleStageParseError {
    pub source_id: SourceId,
    pub kind: ModuleStageParseErrorKind,
}

impl ModuleStageParseError {
    pub fn message(&self) -> String {
        match &self.kind {
            ModuleStageParseErrorKind::Parse { message, .. } => message.clone(),
            ModuleStageParseErrorKind::DuplicateModulePath {
                module_path,
                first_file_name,
                second_file_name,
                ..
            } => format!(
                "duplicate module path `{}` in `{}` and `{}`",
                module_path, first_file_name, second_file_name
            ),
        }
    }

    pub fn span(&self) -> spire::ast::Span {
        match &self.kind {
            ModuleStageParseErrorKind::Parse { span, .. } => span.clone(),
            ModuleStageParseErrorKind::DuplicateModulePath { span, .. } => span.clone(),
        }
    }
}

pub fn derive_source_rules(
    compile_unit_kind: spire::CompileUnitKind,
    source_kind: SourceKind,
    entrypoint: Option<&spire::EntryPoint>,
) -> spire::SourceRules {
    // SourceKind controls the syntactic boundary (`@@builtin`, top-level expr,
    // etc.), while CompileUnitKind refines runtime-only rules such as where
    // `set_exit_code` is legal in project builds.
    let base = match source_kind {
        SourceKind::Script => spire::SourceRules::script(),
        SourceKind::Module => spire::SourceRules::module(),
        SourceKind::StdModule => spire::SourceRules::std_module(),
        SourceKind::ReplChunk => spire::SourceRules::repl_chunk(),
    };

    let policy = match source_kind {
        SourceKind::Script => spire::SetExitCodePolicy::Anywhere,
        SourceKind::ReplChunk => spire::SetExitCodePolicy::Forbidden,
        SourceKind::Module | SourceKind::StdModule
            if compile_unit_kind == spire::CompileUnitKind::Project =>
        {
            spire::SetExitCodePolicy::EntryOnly
        }
        SourceKind::Module | SourceKind::StdModule => spire::SetExitCodePolicy::Forbidden,
    };

    base.with_set_exit_code_policy(policy, entrypoint)
}

fn erase_non_newline_span(chars: &mut [char], start: usize, end: usize) {
    let len = chars.len();
    let capped_end = end.min(len);
    let capped_start = start.min(len);
    for ch in chars.iter_mut().take(capped_end).skip(capped_start) {
        if *ch != '\n' {
            *ch = ' ';
        }
    }
}

/// Strip `@@test <expr>` annotations while preserving source span offsets.
///
/// The parser does not need to process `@@test` in normal compilation flows.
/// Replacing characters with spaces keeps diagnostics line/column stable.
pub fn strip_test_annotations(source: &str) -> String {
    let tokens = match spire::lexer::tokenize(source) {
        Ok(tokens) => tokens,
        Err(_) => return source.to_string(),
    };

    let mut chars = source.chars().collect::<Vec<_>>();
    let mut i = 0usize;
    while i < tokens.len() {
        if let Token::Annotator(name) = &tokens[i].token {
            if name == "test" {
                let mut j = i + 1;
                while j < tokens.len() && !matches!(tokens[j].token, Token::Newline | Token::Eof) {
                    j += 1;
                }
                let end = if j > i + 1 {
                    tokens[j - 1].span.end
                } else {
                    tokens[i].span.end
                };
                erase_non_newline_span(&mut chars, tokens[i].span.start, end);
                i = j;
                continue;
            }
        }
        i += 1;
    }

    chars.into_iter().collect::<String>()
}

pub fn lower_module_source_ast(
    ast: Vec<spire::ast::Ast>,
    fallback_module_path: Option<&str>,
) -> Vec<LoweredModuleAst> {
    let shared_imports = ast
        .iter()
        .filter_map(|stmt| match stmt {
            spire::ast::Ast::Import(_, _, _) => Some(stmt.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut lowered = Vec::new();
    let mut shared_non_module_defs = Vec::new();

    for stmt in ast {
        match stmt {
            spire::ast::Ast::Defmod(span, module_path, body) => {
                let mut module_ast = shared_imports.clone();
                module_ast.extend(body);
                lowered.push(LoweredModuleAst {
                    module_path,
                    ast: module_ast,
                    declared_span: Some(span),
                });
            }
            spire::ast::Ast::Import(_, _, _) => {}
            spire::ast::Ast::StructDef(_, _, _)
            | spire::ast::Ast::RecordDef(_, _, _)
            | spire::ast::Ast::DeferrorDef(_, _, _, _)
            | spire::ast::Ast::BuiltinDecl(_, _, _, _) => {
                shared_non_module_defs.push(stmt);
            }
            _ => {
                // Defensive fallback. Parser policy should keep this unreachable for module sources.
                shared_non_module_defs.push(stmt);
            }
        }
    }

    if !shared_non_module_defs.is_empty() {
        let mut shared_ast = shared_imports;
        shared_ast.extend(shared_non_module_defs);
        lowered.push(LoweredModuleAst {
            module_path: fallback_module_path.unwrap_or_default().to_string(),
            ast: shared_ast,
            declared_span: None,
        });
    }

    lowered
}

fn parse_module_stages_from_sources(
    sources: &SourceRegistry,
    module_stages: &[Vec<loader::StagedModule>],
    compile_unit_kind: spire::CompileUnitKind,
) -> Result<Vec<Vec<sigil::StagedModuleAst>>, ModuleStageParseError> {
    let mut staged_module_asts = Vec::with_capacity(module_stages.len());
    let mut seen_module_paths: HashMap<String, String> = HashMap::new();

    for stage in module_stages {
        let mut stage_ast = Vec::new();
        for module in stage {
            let raw_module_source = sources.source(module.source_id).unwrap_or("");
            let module_source = strip_test_annotations(raw_module_source);
            let parsed = spire::parse_with_context(
                &module_source,
                spire::ParserContext::module(module.source_id.0, None).with_rules(
                    derive_source_rules(compile_unit_kind, module.source_kind, None),
                ),
            )
            .map_err(|e| ModuleStageParseError {
                source_id: module.source_id,
                kind: ModuleStageParseErrorKind::Parse {
                    message: e.message().to_string(),
                    span: e.span().clone(),
                },
            })?;

            for lowered in lower_module_source_ast(parsed, None) {
                if !lowered.module_path.is_empty() {
                    let second_file_name = sources
                        .file_name(module.source_id)
                        .unwrap_or("<unknown>")
                        .to_string();
                    if let Some(first_file_name) = seen_module_paths.get(&lowered.module_path) {
                        return Err(ModuleStageParseError {
                            source_id: module.source_id,
                            kind: ModuleStageParseErrorKind::DuplicateModulePath {
                                module_path: lowered.module_path.clone(),
                                first_file_name: first_file_name.clone(),
                                second_file_name,
                                span: lowered
                                    .declared_span
                                    .unwrap_or(spire::ast::Span { start: 0, end: 0 }),
                            },
                        });
                    }
                    seen_module_paths.insert(lowered.module_path.clone(), second_file_name);
                }

                stage_ast.push(sigil::StagedModuleAst {
                    module_path: lowered.module_path,
                    ast: lowered.ast,
                });
            }
        }
        staged_module_asts.push(stage_ast);
    }

    Ok(staged_module_asts)
}

pub fn parse_module_stages_from_compile_sources(
    compile_sources: &CompileSources,
    compile_unit_kind: spire::CompileUnitKind,
) -> Result<Vec<Vec<sigil::StagedModuleAst>>, ModuleStageParseError> {
    parse_module_stages_from_sources(
        &compile_sources.sources,
        &compile_sources.module_stages,
        compile_unit_kind,
    )
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn module_path_from_file_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(ToString::to_string)
        .unwrap_or_default()
}

fn derive_primary_module_path(source: &str) -> Option<String> {
    let tokens = spire::lexer::tokenize(source).ok()?;
    for (idx, spanned) in tokens.iter().enumerate() {
        if !matches!(spanned.token, Token::Defmod) {
            continue;
        }

        let mut j = idx + 1;
        while matches!(tokens.get(j).map(|t| &t.token), Some(Token::Newline)) {
            j += 1;
        }

        let mut segments = Vec::new();
        match tokens.get(j).map(|sp| &sp.token) {
            Some(Token::Ident(name)) => {
                segments.push(name.clone());
                j += 1;
            }
            _ => return None,
        }

        while matches!(tokens.get(j).map(|t| &t.token), Some(Token::Colon))
            && matches!(tokens.get(j + 1).map(|t| &t.token), Some(Token::Colon))
        {
            j += 2;
            match tokens.get(j).map(|sp| &sp.token) {
                Some(Token::Ident(name)) => {
                    segments.push(name.clone());
                    j += 1;
                }
                _ => return None,
            }
        }

        if !segments.is_empty() {
            return Some(segments.join("::"));
        }
    }

    None
}

fn collect_repl_std_module_inputs() -> Result<Vec<ModuleInput>, LoadError> {
    let lib_dir = Path::new("lib");
    if !lib_dir.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(lib_dir).map_err(|e| LoadError::SourceReadFailed {
        file_name: display_path(lib_dir),
        message: e.to_string(),
    })?;

    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "srt")
        {
            files.push(path);
        }
    }
    files.sort();

    let mut module_inputs = Vec::new();
    for path in files {
        let file_name = display_path(&path);
        let source = fs::read_to_string(&path).map_err(|e| LoadError::SourceReadFailed {
            file_name: file_name.clone(),
            message: e.to_string(),
        })?;
        let module_path = derive_primary_module_path(&source)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| module_path_from_file_name(&path));
        if REPL_AUTO_IMPORT_MODULES.contains(&module_path.as_str()) {
            continue;
        }
        module_inputs.push(ModuleInput {
            file_name,
            source,
            module_path,
        });
    }
    Ok(module_inputs)
}

const XLDR_VERSION: &str = env!("CARGO_PKG_VERSION");
const REPL_AUTO_IMPORT_MODULES: &[&str] = &["Bootstrap", "Kernel"];

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

#[derive(Debug, Default)]
struct ReplImportResult {
    imported_symbols: Vec<String>,
    success_labels: Vec<String>,
}

struct ReplEngine {
    sources: SourceRegistry,
    builtin_source_id: SourceId,
    module_stages: Vec<Vec<StagedModule>>,
    declaration_index: sigil::DeclarationIndex,
    repl_source_id: SourceId,
    repl_module_path: String,
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
        let std_module_inputs = collect_repl_std_module_inputs()?;
        let repl_sources = if std_module_inputs.is_empty() {
            loader::collect_repl_sources()?
        } else {
            loader::collect_repl_sources_with_std_module_stages(&[std_module_inputs])?
        };
        let forge_session = forge::ForgeSession::new();
        let vm = eldr::VM::new_interactive(forge_session.type_registry());
        let mut engine = Self {
            sources: repl_sources.sources,
            builtin_source_id: repl_sources.builtin_source_id,
            module_stages: repl_sources.module_stages,
            declaration_index: Default::default(),
            repl_source_id: repl_sources.repl_source_id,
            repl_module_path: repl_sources.repl_module_path.clone(),
            sigil_session: sigil::SigilSession::with_module_path(Some(
                repl_sources.repl_module_path,
            )),
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
        engine.bootstrap_std_modules();
        Ok(engine)
    }

    fn bootstrap_std_modules(&mut self) {
        let module_stages = match parse_module_stages_from_sources(
            &self.sources,
            &self.module_stages,
            spire::CompileUnitKind::Repl,
        ) {
            Ok(stages) => stages,
            Err(e) => {
                diagnostics::report_error_by_id(
                    &self.sources,
                    e.source_id,
                    diagnostics::simple_error("ParseError", e.message(), e.span(), None),
                );
                return;
            }
        };

        if module_stages.iter().all(|stage| stage.is_empty()) {
            return;
        }

        let declaration_index = match sigil::precollect_declaration_index(&module_stages) {
            Ok(index) => index,
            Err(e) => {
                diagnostics::report_error_by_id(
                    &self.sources,
                    self.builtin_source_id,
                    diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None),
                );
                return;
            }
        };
        self.declaration_index = declaration_index.clone();

        let resolved = match sigil::resolve_staged_program(
            &module_stages,
            Vec::new(),
            &declaration_index,
            None,
        ) {
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

        let typed = match self.scar_session.typecheck_with_context(
            resolved,
            scar::TypecheckContext {
                source_rules: derive_source_rules(
                    spire::CompileUnitKind::Repl,
                    SourceKind::StdModule,
                    None,
                ),
            },
        ) {
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

        for stage in &self.module_stages {
            for module in stage {
                if let Some(source) = self.sources.source(module.source_id) {
                    populate_error_template_lines(&mut chunk.error_templates, source);
                }
            }
        }
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

        let scope = match sigil::build_scope_for_module(
            &module_stages,
            Some(&self.repl_module_path),
            module_stages.len(),
        ) {
            Ok(scope) => scope,
            Err(e) => {
                diagnostics::report_error_by_id(
                    &self.sources,
                    self.builtin_source_id,
                    diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None),
                );
                return;
            }
        };
        self.sigil_session.replace_scope(scope);

        for name in &meta.function_defs {
            self.symbols.insert(name.clone());
        }
    }

    fn bind_import_name(
        &mut self,
        short_name: &str,
        uid: u32,
        module_name: &str,
        span: &Span,
        imported_symbols: &mut Vec<String>,
    ) -> Result<(), ResolveError> {
        if let Some(existing_uid) = self.sigil_session.lookup_uid(short_name) {
            if existing_uid == uid {
                return Ok(());
            }
            return Err(ResolveError {
                message: format!(
                    "Import conflict for `{}` from module `{}`",
                    short_name, module_name
                ),
                span: span.clone(),
            });
        }

        self.sigil_session.define_with_id(short_name, uid);
        if !imported_symbols.iter().any(|name| name == short_name) {
            imported_symbols.push(short_name.to_string());
        }
        Ok(())
    }

    fn import_module_all(
        &mut self,
        module_name: &str,
        span: &Span,
        imported_symbols: &mut Vec<String>,
    ) -> Result<(), ResolveError> {
        let mut imported_any = false;
        let mut blocked_by_stage = false;
        let current_stage_index = self.module_stages.len();
        let entries = self
            .declaration_index
            .values()
            .filter(|entry| entry.module_path == module_name)
            .cloned()
            .collect::<Vec<_>>();

        for entry in entries {
            if entry.stage_index >= current_stage_index {
                blocked_by_stage = true;
                continue;
            }
            let Some(uid) = self.sigil_session.lookup_uid(&entry.fq_name) else {
                blocked_by_stage = true;
                continue;
            };
            self.bind_import_name(&entry.name, uid, module_name, span, imported_symbols)?;
            imported_any = true;
        }

        if imported_any {
            Ok(())
        } else if blocked_by_stage {
            Err(ResolveError {
                message: format!(
                    "Import target `{}` is not available in the current stage",
                    module_name
                ),
                span: span.clone(),
            })
        } else {
            Err(ResolveError {
                message: format!("Unknown module import: {}", module_name),
                span: span.clone(),
            })
        }
    }

    fn import_module_member(
        &mut self,
        module_name: &str,
        name: &str,
        span: &Span,
        imported_symbols: &mut Vec<String>,
    ) -> Result<(), ResolveError> {
        let fq_name = format!("{}::{}", module_name, name);
        let Some(entry) = self.declaration_index.get(&fq_name) else {
            let module_exists = self
                .declaration_index
                .values()
                .any(|entry| entry.module_path == module_name);
            return Err(ResolveError {
                message: if module_exists {
                    format!("Unknown import member: {}", fq_name)
                } else {
                    format!("Unknown module import: {}", module_name)
                },
                span: span.clone(),
            });
        };

        if entry.stage_index >= self.module_stages.len() {
            return Err(ResolveError {
                message: format!(
                    "Import target `{}` is not available in the current stage",
                    fq_name
                ),
                span: span.clone(),
            });
        }

        let uid = self
            .sigil_session
            .lookup_uid(&entry.fq_name)
            .ok_or_else(|| ResolveError {
                message: format!(
                    "Import target `{}` is not available in the current stage",
                    fq_name
                ),
                span: span.clone(),
            })?;
        self.bind_import_name(name, uid, module_name, span, imported_symbols)
    }

    fn apply_repl_imports(&mut self, ast: &[Ast]) -> Result<ReplImportResult, ResolveError> {
        let mut result = ReplImportResult::default();
        for stmt in ast {
            let Ast::Import(span, path, spec) = stmt else {
                continue;
            };
            let module_name = path.segments.join("::");
            if REPL_AUTO_IMPORT_MODULES
                .iter()
                .any(|auto| auto == &module_name.as_str())
            {
                return Err(ResolveError {
                    message: format!(
                        "Duplicate import: `{}` is auto-imported and cannot be explicitly imported",
                        module_name
                    ),
                    span: span.clone(),
                });
            }

            match spec {
                ImportSpec::All => {
                    self.import_module_all(&module_name, span, &mut result.imported_symbols)?;
                    result.success_labels.push(module_name);
                }
                ImportSpec::Single(name) => {
                    self.import_module_member(
                        &module_name,
                        name,
                        span,
                        &mut result.imported_symbols,
                    )?;
                    result
                        .success_labels
                        .push(format!("{}::{}", module_name, name));
                }
                ImportSpec::List(names) => {
                    for name in names {
                        self.import_module_member(
                            &module_name,
                            name,
                            span,
                            &mut result.imported_symbols,
                        )?;
                        result
                            .success_labels
                            .push(format!("{}::{}", module_name, name));
                    }
                }
            }
        }
        Ok(result)
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
            spire::ParserContext::repl(self.repl_source_id.0).with_rules(derive_source_rules(
                spire::CompileUnitKind::Repl,
                SourceKind::ReplChunk,
                None,
            )),
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

        let import_only = ast.iter().all(|stmt| matches!(stmt, Ast::Import(_, _, _)));
        let sigil_cp = self.sigil_session.checkpoint();
        let scar_cp = self.scar_session.checkpoint();
        let forge_cp = self.forge_session.checkpoint();
        let import_result = match self.apply_repl_imports(&ast) {
            Ok(result) => result,
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

        let typed = match self.scar_session.typecheck_with_context(
            resolved,
            scar::TypecheckContext {
                source_rules: derive_source_rules(
                    spire::CompileUnitKind::Repl,
                    SourceKind::ReplChunk,
                    None,
                ),
            },
        ) {
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
                    if import_only {
                        for label in &import_result.success_labels {
                            println!("> imported {}", label);
                        }
                    }
                    for imported in &import_result.imported_symbols {
                        self.symbols.insert(imported.clone());
                    }
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
                // E-1 contract (REPL):
                // runtime traps are treated as terminal for the current REPL session.
                eldr::report_runtime_error(
                    &e,
                    self.vm.source(),
                    self.vm.source_file(),
                    self.vm.runtime_error_location(),
                );
                self.bump_line(None);
                self.pending.clear();
                return ReplOutcome::Exit;
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
        // E-3 note:
        // Unlike CLI `run`, REPL keeps the session alive after `Result::Err`.
        // This stays local to REPL entry handling by design.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_module_source_extracts_defmods_and_shared_defs() {
        let ast = spire::parse_with_context(
            r#"import Other::f;

defmod A {
  def fa() -> Int { 1 }
}

defrecord Pair(left: Int, right: Int)

defmod B {
  def fb() -> Int { f() }
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("module source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 3);
        assert_eq!(lowered[0].module_path, "A");
        assert_eq!(lowered[1].module_path, "B");
        assert_eq!(lowered[2].module_path, "");
        assert!(matches!(
            lowered[0].ast[0],
            spire::ast::Ast::Import(_, _, spire::ast::ImportSpec::Single(_))
        ));
        assert!(lowered[2]
            .ast
            .iter()
            .any(|stmt| matches!(stmt, spire::ast::Ast::RecordDef(_, _, _))));
    }

    #[test]
    fn parse_module_stages_detects_duplicate_defmod_paths() {
        let module_sources = collect_module_sources_with_module_stages(&[vec![
            ModuleInput {
                file_name: "a.srt".into(),
                source: "defmod Shared { def a() -> Int { 1 } }".into(),
                module_path: "A".into(),
            },
            ModuleInput {
                file_name: "b.srt".into(),
                source: "defmod Shared { def b() -> Int { 2 } }".into(),
                module_path: "B".into(),
            },
        ]])
        .expect("module collection should succeed");
        let compile_sources =
            compose_script_compile_sources("entry.srt", "print(\"hi\")", module_sources);

        let err = parse_module_stages_from_compile_sources(
            &compile_sources,
            spire::CompileUnitKind::Script,
        )
        .expect_err("duplicate defmod path must fail");
        assert!(matches!(
            err.kind,
            ModuleStageParseErrorKind::DuplicateModulePath { ref module_path, .. } if module_path == "Shared"
        ));
    }

    #[test]
    fn strip_test_annotations_replaces_annotated_line_with_spaces() {
        let source =
            "defmod M {\n  @@test add(1, 2) == 3\n  def add(x: Int, y: Int) -> Int { x + y }\n}\n";
        let stripped = strip_test_annotations(source);

        assert!(!stripped.contains("@@test"));
        assert!(stripped.contains("def add(x: Int, y: Int) -> Int { x + y }"));
        assert_eq!(source.lines().count(), stripped.lines().count());
    }

    #[test]
    fn strip_test_annotations_is_noop_without_annotations() {
        let source = "defmod Kernel {\n  def add(x: Int, y: Int) -> Int { x + y }\n}\n";
        assert_eq!(strip_test_annotations(source), source);
    }
}
