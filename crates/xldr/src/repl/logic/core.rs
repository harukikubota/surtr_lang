use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::panic;

use diagnostics::{SourceId, SourceRegistry};
use eldr::builtin::inspect_value;
use eldr::value::Value;
use forge::bytecode::populate_error_template_lines;
use sigil::error::ResolveError;
use sindr::builtin::BUILTIN_METAS;
use sindr::ir::{DocEntry, DocKind};
use sindr::policy::CompileUnitKind;
use spire::ast::{Ast, ImportSpec, Span};

use super::command::{parse_repl_command, ReplCommand};
use super::output::{ReplOutput, ReplResult};
use super::render;
use crate::loader::{self, StagedModule};
use crate::ErrorDisplayMode;
use crate::{
    collect_additional_default_std_module_inputs, derive_parse_rules, derive_runtime_policy,
    error_display, LoadError, ModuleStageParseError, ModuleStageParseErrorKind, SourceKind,
};

const XLDR_VERSION: &str = env!("CARGO_PKG_VERSION");
const OPERATOR_DOC_ALIASES: &[(&str, &str)] = &[
    ("+", "Add"),
    ("-", "Sub"),
    ("*", "Mul"),
    ("&&", "and"),
    ("||", "or"),
    ("==", "Eq"),
    ("!=", "Neq"),
    ("<", "Lt"),
    ("<=", "Lte"),
    (">", "Gt"),
    (">=", "Gte"),
    ("++", "Concat"),
    ("|>", "PipeApply"),
    ("|*>", "Functor"),
    ("|>=", "Chainable"),
    (">>", "Composable"),
    (">*", "LiftComposable"),
    (">=>", "KleisliComposable"),
];
const METHOD_DOC_TRAIT_ALIASES: &[(&str, &str)] = &[
    ("add", "Add"),
    ("sub", "Sub"),
    ("mul", "Mul"),
    ("eq", "Eq"),
    ("neq", "Neq"),
    ("lt", "Lt"),
    ("lte", "Lte"),
    ("gt", "Gt"),
    ("gte", "Gte"),
    ("concat", "Concat"),
];
const STAGE_PARSE_WORKER_STACK_SIZE: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypedCallQuery {
    callee: String,
    arg_types: Vec<String>,
}

/// Error returned when loading a `.eldr` file into a REPL engine.
#[derive(Debug)]
pub enum EldrLoadError {
    /// Binary format error (bad magic, truncation, decode failure, etc.).
    Format(sindr::ir::BytecodeFormatError),
    /// Source / module loader error.
    Load(LoadError),
}

impl std::fmt::Display for EldrLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EldrLoadError::Format(e) => write!(f, "eldr format error: {}", e),
            EldrLoadError::Load(e) => write!(f, "loader error: {}", e),
        }
    }
}

#[derive(Debug, Default)]
struct ReplImportResult {
    imported_symbols: Vec<String>,
    success_labels: Vec<String>,
}

pub struct ReplEngine {
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
    result_metas: Vec<Option<forge::ChunkMeta>>,
    symbols: BTreeSet<String>,
    docs: Vec<DocEntry>,
    auto_import_modules: BTreeSet<String>,
    error_display_mode: ErrorDisplayMode,
}

impl ReplEngine {
    pub fn new() -> Result<Self, LoadError> {
        let std_module_inputs = collect_additional_default_std_module_inputs()?;
        let repl_sources = loader::collect_repl_sources_with_module_stages(&[std_module_inputs])?;
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
            result_metas: Vec::new(),
            symbols: ["Ok", "Err"]
                .into_iter()
                .map(str::to_string)
                .chain(BUILTIN_METAS.iter().map(|meta| meta.name.to_string()))
                .collect(),
            docs: Vec::new(),
            auto_import_modules: BTreeSet::new(),
            error_display_mode: ErrorDisplayMode::Full,
        };
        engine.bootstrap_std_modules()?;
        Ok(engine)
    }

    /// Initialise a REPL engine from an existing `.eldr` bytecode payload.
    ///
    /// The VM is seeded with the compiled bytecode from the file, so all
    /// function definitions in the image are already resident.  Standard-
    /// library sigil / scar context is bootstrapped from source (no re-
    /// execution needed) so that new REPL chunks can reference stdlib symbols.
    ///
    /// Limitation: user-defined functions in the `.eldr` that are beyond the
    /// standard library are present in the VM but are not visible to sigil name
    /// resolution.  New REPL input that calls them will therefore fail to
    /// resolve.  Full restoration requires the compile-time type context, which
    /// will be addressed when `--debug=full` span / type metadata is added.
    pub fn from_eldr(bytes: &[u8]) -> Result<Self, EldrLoadError> {
        let bytecode = sindr::ir::Bytecode::decode(bytes).map_err(EldrLoadError::Format)?;

        let std_module_inputs =
            collect_additional_default_std_module_inputs().map_err(EldrLoadError::Load)?;
        let repl_sources = loader::collect_repl_sources_with_module_stages(&[std_module_inputs])
            .map_err(EldrLoadError::Load)?;

        let docs = bytecode.docs.clone();
        let forge_session = forge::ForgeSession::from_bytecode(&bytecode);
        let vm = eldr::VM::new(bytecode);

        // Populate completion symbols from the pre-loaded function table.
        let mut symbols: BTreeSet<String> = ["Ok", "Err"]
            .into_iter()
            .map(str::to_string)
            .chain(BUILTIN_METAS.iter().map(|meta| meta.name.to_string()))
            .collect();
        for entry in vm.bytecode().functions.iter() {
            if let Some(name) = &entry.qualified_name {
                symbols.insert(name.clone());
            }
        }
        for entry in vm.bytecode().type_registry.entries.iter() {
            symbols.insert(entry.name.clone());
        }

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
            result_metas: Vec::new(),
            symbols,
            docs,
            auto_import_modules: BTreeSet::new(),
            error_display_mode: ErrorDisplayMode::Full,
        };
        // Set up sigil / scar scope for stdlib without re-executing bytecode.
        engine
            .bootstrap_std_modules_scope_only()
            .map_err(EldrLoadError::Load)?;
        Ok(engine)
    }

    fn bootstrap_std_modules(&mut self) -> Result<(), LoadError> {
        let module_stages = match parse_module_stages_from_sources(
            &self.sources,
            &self.module_stages,
            CompileUnitKind::Repl,
        ) {
            Ok(stages) => stages,
            Err(e) => return Err(load_error_from_parse_failure(&self.sources, e)),
        };

        if module_stages.iter().all(|stage| stage.is_empty()) {
            return Ok(());
        }

        self.auto_import_modules = module_stages
            .iter()
            .flat_map(|stage| stage.iter())
            .filter(|module| module.auto_import)
            .map(|module| module.module_path.clone())
            .collect();

        let declaration_index = match sigil::precollect_declaration_index(&module_stages) {
            Ok(index) => index,
            Err(e) => {
                return Err(load_error_from_span_failure(
                    &self.sources,
                    &self.module_stages,
                    &e.span,
                    self.builtin_source_id,
                    "resolve",
                    e.message,
                ));
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
                return Err(load_error_from_span_failure(
                    &self.sources,
                    &self.module_stages,
                    &e.span,
                    self.builtin_source_id,
                    "resolve",
                    e.message,
                ));
            }
        };

        let typed = match self.scar_session.typecheck_with_context(
            resolved,
            scar::TypecheckContext {
                runtime_policy: derive_runtime_policy(
                    CompileUnitKind::Repl,
                    SourceKind::StdModule,
                    None,
                ),
                enforce_builtin_type_contracts: true,
                allow_error_function_params: true,
            },
        ) {
            Ok(t) => t,
            Err(e) => {
                return Err(load_error_from_span_failure(
                    &self.sources,
                    &self.module_stages,
                    &e.span,
                    self.builtin_source_id,
                    "typecheck",
                    e.message,
                ));
            }
        };

        let docs = crate::collect_doc_entries(&module_stages, &[], None);
        let (mut chunk, mut meta) = match self.forge_session.codegen_chunk(typed) {
            Ok(c) => c,
            Err(e) => {
                return Err(load_error_from_span_failure(
                    &self.sources,
                    &self.module_stages,
                    &e.span,
                    self.builtin_source_id,
                    "codegen",
                    e.message,
                ));
            }
        };
        meta.docs = docs.clone();
        chunk.docs = docs.clone();

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
            let file_name = self.vm.source_file().unwrap_or("<runtime>").to_string();
            return Err(LoadError::BootstrapFailed {
                phase: "runtime".into(),
                file_name,
                message: e.to_string(),
            });
        }
        self.sync_scar_fun_index_with_vm();

        let scope = match sigil::build_scope_for_module(
            &module_stages,
            Some(&self.repl_module_path),
            module_stages.len(),
        ) {
            Ok(scope) => scope,
            Err(e) => {
                return Err(load_error_from_span_failure(
                    &self.sources,
                    &self.module_stages,
                    &e.span,
                    self.builtin_source_id,
                    "resolve",
                    e.message,
                ));
            }
        };
        self.sigil_session
            .replace_scope_with_declarations(scope, &declaration_index);

        for name in &meta.function_defs {
            self.symbols.insert(name.clone());
        }
        self.append_docs(docs);
        Ok(())
    }

    /// Bootstrap stdlib sigil / scar context WITHOUT re-executing bytecode.
    ///
    /// Used when loading a `.eldr` image: the VM already contains compiled
    /// stdlib code, so we only need the name-resolution and type-checking
    /// context that sigil and scar provide.  Forge codegen and vm.push_atomic
    /// are intentionally skipped.
    fn bootstrap_std_modules_scope_only(&mut self) -> Result<(), LoadError> {
        if let Ok(snapshot) = crate::default_stdlib_semantic_snapshot() {
            if self.module_stages.len() == snapshot.default_stage_count {
                self.auto_import_modules = snapshot.auto_import_modules.clone();
                self.declaration_index = snapshot.declaration_index.clone();
                self.scar_session.rollback(snapshot.scar_checkpoint.clone());
                self.sync_scar_fun_index_with_vm();
                self.append_docs(snapshot.docs.clone());

                let scope = match sigil::build_scope_for_module(
                    &snapshot.module_stages,
                    Some(&self.repl_module_path),
                    snapshot.module_stages.len(),
                ) {
                    Ok(scope) => scope,
                    Err(e) => {
                        return Err(load_error_from_span_failure(
                            &self.sources,
                            &self.module_stages,
                            &e.span,
                            self.builtin_source_id,
                            "resolve",
                            e.message,
                        ));
                    }
                };
                self.sigil_session
                    .replace_scope_with_declarations(scope, &self.declaration_index);
                return Ok(());
            }
        }

        let module_stages = match parse_module_stages_from_sources(
            &self.sources,
            &self.module_stages,
            CompileUnitKind::Repl,
        ) {
            Ok(stages) => stages,
            Err(e) => return Err(load_error_from_parse_failure(&self.sources, e)),
        };

        if module_stages.iter().all(|stage| stage.is_empty()) {
            return Ok(());
        }

        self.auto_import_modules = module_stages
            .iter()
            .flat_map(|stage| stage.iter())
            .filter(|module| module.auto_import)
            .map(|module| module.module_path.clone())
            .collect();

        let declaration_index = match sigil::precollect_declaration_index(&module_stages) {
            Ok(index) => index,
            Err(e) => {
                return Err(load_error_from_span_failure(
                    &self.sources,
                    &self.module_stages,
                    &e.span,
                    self.builtin_source_id,
                    "resolve",
                    e.message,
                ));
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
                return Err(load_error_from_span_failure(
                    &self.sources,
                    &self.module_stages,
                    &e.span,
                    self.builtin_source_id,
                    "resolve",
                    e.message,
                ));
            }
        };

        // Type-check to populate scar session; discard typed nodes (no codegen).
        if let Err(e) = self.scar_session.typecheck_with_context(
            resolved,
            scar::TypecheckContext {
                runtime_policy: derive_runtime_policy(
                    CompileUnitKind::Repl,
                    SourceKind::StdModule,
                    None,
                ),
                enforce_builtin_type_contracts: true,
                allow_error_function_params: true,
            },
        ) {
            return Err(load_error_from_span_failure(
                &self.sources,
                &self.module_stages,
                &e.span,
                self.builtin_source_id,
                "typecheck",
                e.message,
            ));
        }
        self.sync_scar_fun_index_with_vm();
        self.append_docs(crate::collect_doc_entries(&module_stages, &[], None));

        let scope = match sigil::build_scope_for_module(
            &module_stages,
            Some(&self.repl_module_path),
            module_stages.len(),
        ) {
            Ok(scope) => scope,
            Err(e) => {
                return Err(load_error_from_span_failure(
                    &self.sources,
                    &self.module_stages,
                    &e.span,
                    self.builtin_source_id,
                    "resolve",
                    e.message,
                ));
            }
        };
        self.sigil_session
            .replace_scope_with_declarations(scope, &declaration_index);
        Ok(())
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
                related_labels: Vec::new(),
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
                related_labels: Vec::new(),
            })
        } else {
            Err(ResolveError {
                message: format!("Unknown module import: {}", module_name),
                span: span.clone(),
                related_labels: Vec::new(),
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
                related_labels: Vec::new(),
            });
        };

        if entry.stage_index >= self.module_stages.len() {
            return Err(ResolveError {
                message: format!(
                    "Import target `{}` is not available in the current stage",
                    fq_name
                ),
                span: span.clone(),
                related_labels: Vec::new(),
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
                related_labels: Vec::new(),
            })?;
        self.bind_import_name(name, uid, module_name, span, imported_symbols)
    }

    fn apply_repl_imports(&mut self, ast: &[Ast]) -> Result<ReplImportResult, ResolveError> {
        let mut result = ReplImportResult::default();
        let auto_import_traits = self
            .declaration_index
            .values()
            .filter(|entry| entry.kind == sigil::DeclarationKind::Trait && entry.auto_import)
            .map(|entry| entry.name.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for stmt in ast {
            let Ast::Import(span, path, spec) = stmt else {
                continue;
            };
            let module_name = path.segments.join("::");
            if self.auto_import_modules.contains(&module_name)
                || auto_import_traits.contains(&module_name)
            {
                return Err(ResolveError {
                    message: format!(
                        "Duplicate import: `{}` is auto-imported and cannot be explicitly imported",
                        module_name
                    ),
                    span: span.clone(),
                    related_labels: Vec::new(),
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

    pub fn completion_symbols(&self) -> Vec<String> {
        self.symbols.iter().cloned().collect()
    }

    pub fn prompt(&self) -> String {
        if self.pending.is_empty() {
            format!("xldr({})> ", self.next_line)
        } else {
            format!("...({})> ", self.next_line)
        }
    }

    fn report_main_result_error_if_any(&self, value: &Value) -> Option<Vec<String>> {
        // E-3 note:
        // Unlike CLI `run`, REPL keeps the session alive after `Result::Err`.
        // This stays local to REPL entry handling by design.
        match value {
            Value::Tagged { tag: 1, fields } => {
                if let Some(err_value) = fields.first() {
                    Some(self.report_error_value(err_value))
                } else {
                    let text = error_display::invalid_result_missing_payload_text(
                        self.vm.source(),
                        self.vm.source_file(),
                        self.vm.runtime_error_location(),
                    );
                    error_display::emit_text(&text, self.error_display_mode);
                    Some(error_display::lines_for_mode(
                        &text,
                        self.error_display_mode,
                    ))
                }
            }
            _ => None,
        }
    }

    fn report_error_value(&self, value: &Value) -> Vec<String> {
        error_display::emit_runtime_value_error_with_registry(
            &self.vm,
            value,
            &self.sources,
            self.repl_source_id,
            self.error_display_mode,
        );
        error_display::runtime_value_error_lines_with_registry(
            &self.vm,
            value,
            &self.sources,
            self.repl_source_id,
            self.error_display_mode,
        )
    }

    fn bump_line(&mut self, value: Option<Value>, meta: Option<forge::ChunkMeta>) {
        self.results.push(value);
        self.result_metas.push(meta);
        self.next_line += 1;
    }

    fn help_lines() -> Vec<String> {
        vec![
            "REPL commands:".to_string(),
            ":help, :h [command]  Show REPL help".to_string(),
            ":quit, :exit         Exit the REPL".to_string(),
            ":doc <symbol>        Show documentation for a visible symbol".to_string(),
            ":sig <symbol>        Show the signature for a visible function".to_string(),
            ":error [full|summary]  Show or change error display mode".to_string(),
            ":save <path.eldr>    Save the current session as .eldr".to_string(),
            ":v <line>            Recall a previous result".to_string(),
        ]
    }

    fn doc_help_lines() -> Vec<String> {
        vec![
            "Usage: :doc <symbol>".to_string(),
            "Also: :doc <typed-call>".to_string(),
            "Examples: :doc print, :doc Kernel::if, :doc Add, :doc +, :doc gt(3, 2)".to_string(),
        ]
    }

    fn sig_help_lines() -> Vec<String> {
        vec![
            "Usage: :sig <function>".to_string(),
            "Also: :sig <typed-call>".to_string(),
            "Examples: :sig print, :sig Kernel::if, :sig add, :sig gt(_ : Float, _ : Float)"
                .to_string(),
        ]
    }

    fn handle_help(&self, topic: Option<&str>) -> Vec<String> {
        let Some(topic) = topic.map(str::trim).filter(|topic| !topic.is_empty()) else {
            return Self::help_lines();
        };
        match topic.strip_prefix(':').unwrap_or(topic) {
            "doc" => Self::doc_help_lines(),
            "sig" => Self::sig_help_lines(),
            other => {
                let mut rendered = vec![format!("No help found for :{}", other)];
                rendered.push("Type :help for available REPL commands.".to_string());
                rendered
            }
        }
    }

    fn handle_doc(&self, symbol: &str) -> ReplResult {
        let trimmed = symbol.trim();
        if trimmed.is_empty() {
            return ReplResult::ok(ReplOutput::CommandOutput {
                rendered: Self::doc_help_lines(),
            });
        }
        if let Some(query) = self.parse_typed_call_query(trimmed) {
            return match query {
                Ok(query) => self.handle_doc_typed_call(trimmed, &query),
                Err(message) => ReplResult::ok(ReplOutput::CommandOutput {
                    rendered: vec![message],
                }),
            };
        }
        if trimmed.split_whitespace().count() != 1 {
            return ReplResult::ok(ReplOutput::CommandOutput {
                rendered: Self::doc_help_lines(),
            });
        }

        let canonical = OPERATOR_DOC_ALIASES
            .iter()
            .find_map(|(alias, trait_name)| (*alias == trimmed).then_some(*trait_name))
            .unwrap_or(trimmed);

        let matches = self.matching_doc_entries(canonical, None);

        match matches.as_slice() {
            [] => ReplResult::ok(ReplOutput::CommandOutput {
                rendered: vec![format!("No docs found for {}", trimmed)],
            }),
            [entry] => ReplResult::ok(Self::doc_resolved_output(entry)),
            entries => ReplResult::ok(ReplOutput::CommandOutput {
                rendered: Self::ambiguous_doc_lines(trimmed, entries),
            }),
        }
    }

    fn canonical_doc_symbol(symbol: &str) -> &str {
        OPERATOR_DOC_ALIASES
            .iter()
            .find_map(|(alias, trait_name)| (*alias == symbol).then_some(*trait_name))
            .unwrap_or(symbol)
    }

    fn symbol_matches(qualified_name: &str, symbol: &str) -> bool {
        qualified_name == symbol
            || qualified_name
                .rsplit("::")
                .next()
                .is_some_and(|tail| tail == symbol)
    }

    fn doc_resolved_output(entry: &DocEntry) -> ReplOutput {
        let summary = entry
            .doc
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(ToString::to_string);
        ReplOutput::DocResolved {
            symbol: entry.qualified_name.clone(),
            signature: entry.signature.clone(),
            summary,
            source_snippet: Some(entry.doc.clone()),
        }
    }

    fn doc_method_tail(qualified_name: &str) -> &str {
        qualified_name.rsplit("::").next().unwrap_or(qualified_name)
    }

    fn matching_doc_entries<'a>(
        &'a self,
        symbol: &str,
        kind: Option<DocKind>,
    ) -> Vec<&'a DocEntry> {
        let mut matches = self
            .docs
            .iter()
            .filter(|entry| kind.as_ref().is_none_or(|kind| &entry.kind == kind))
            .filter(|entry| Self::symbol_matches(&entry.qualified_name, symbol))
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        matches.dedup_by(|a, b| {
            a.qualified_name == b.qualified_name
                && a.kind == b.kind
                && a.signature == b.signature
                && a.doc == b.doc
        });
        matches
    }

    fn ambiguous_doc_lines(symbol: &str, entries: &[&DocEntry]) -> Vec<String> {
        let mut rendered = vec![format!("{symbol} has multiple docs:")];
        rendered.extend(
            entries
                .iter()
                .map(|entry| format!("  {}", entry.qualified_name)),
        );
        rendered.push(
            "Use a qualified name or add type annotations, for example `:doc gt(3, 2)`."
                .to_string(),
        );
        rendered
    }

    fn handle_doc_typed_call(&self, source_query: &str, query: &TypedCallQuery) -> ReplResult {
        let matches = self.match_typed_call_docs(query);
        match matches.as_slice() {
            [] => ReplResult::ok(ReplOutput::CommandOutput {
                rendered: vec![format!("No docs found for {}", source_query)],
            }),
            [entry] => ReplResult::ok(Self::doc_resolved_output(entry)),
            entries => ReplResult::ok(ReplOutput::CommandOutput {
                rendered: Self::ambiguous_doc_lines(source_query, entries),
            }),
        }
    }

    fn match_typed_call_docs<'a>(&'a self, query: &TypedCallQuery) -> Vec<&'a DocEntry> {
        if let Some(matches) = self.match_special_form_typed_call_docs(query) {
            return matches;
        }

        let Some(receiver_ty) = query.arg_types.first() else {
            return Vec::new();
        };
        let preferred_trait = METHOD_DOC_TRAIT_ALIASES
            .iter()
            .find_map(|(method, trait_name)| (*method == query.callee).then_some(*trait_name));
        let mut matches = self
            .docs
            .iter()
            .filter(|entry| entry.kind == DocKind::Function)
            .filter(|entry| Self::doc_method_tail(&entry.qualified_name) == query.callee)
            .filter(|entry| {
                entry.signature.as_deref().is_some_and(|sig| {
                    if sig.starts_with("impl ") {
                        return sig.contains(&format!(" for {receiver_ty}::{}", query.callee));
                    }
                    Self::signature_matches_callee(sig, &query.callee)
                })
            })
            .filter(|entry| {
                preferred_trait.is_none_or(|trait_name| {
                    entry
                        .signature
                        .as_deref()
                        .is_some_and(|sig| sig.starts_with(&format!("impl {trait_name} for ")))
                })
            })
            .filter(|entry| {
                entry
                    .signature
                    .as_deref()
                    .is_none_or(|sig| Self::signature_accepts_arg_types(sig, &query.arg_types))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        matches
    }

    fn match_special_form_typed_call_docs<'a>(
        &'a self,
        query: &TypedCallQuery,
    ) -> Option<Vec<&'a DocEntry>> {
        match query.callee.as_str() {
            "dbg!" => {
                let mut matches = self
                    .docs
                    .iter()
                    .filter(|entry| entry.kind == DocKind::Function)
                    .filter(|entry| Self::doc_method_tail(&entry.qualified_name) == "dbg!")
                    .collect::<Vec<_>>();
                matches.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
                Some(matches)
            }
            _ => None,
        }
    }

    fn signature_matches_callee(signature: &str, callee: &str) -> bool {
        signature.starts_with(&format!("{callee}("))
            || signature.contains(&format!("::{callee}("))
            || signature.starts_with(&format!("@@intrinsic def {callee}<"))
            || signature.starts_with(&format!("@@intrinsic def {callee}("))
    }

    fn parameter_type_accepts_arg_type(param: &str, arg: &str) -> bool {
        param == arg || param == "Self" || param.starts_with('$')
    }

    fn signature_accepts_arg_types(signature: &str, arg_types: &[String]) -> bool {
        let Some(params) = signature
            .split_once('(')
            .and_then(|(_, rest)| rest.rsplit_once(')').map(|(params, _)| params))
        else {
            return false;
        };
        let param_types = split_top_level_commas(params)
            .into_iter()
            .filter_map(|param| param.split_once(':').map(|(_, ty)| ty.trim().to_string()))
            .collect::<Vec<_>>();
        let variadic_index = param_types.iter().position(|param| param.starts_with('*'));

        match variadic_index {
            Some(index) => {
                if index != param_types.len().saturating_sub(1) || arg_types.len() < index + 1 {
                    return false;
                }

                let fixed_match = param_types[..index]
                    .iter()
                    .zip(&arg_types[..index])
                    .all(|(param, arg)| Self::parameter_type_accepts_arg_type(param, arg));
                if !fixed_match {
                    return false;
                }

                let variadic_param = param_types[index].trim_start_matches('*').trim();
                arg_types[index..]
                    .iter()
                    .all(|arg| Self::parameter_type_accepts_arg_type(variadic_param, arg))
            }
            None => {
                if param_types.len() != arg_types.len() {
                    return false;
                }
                param_types
                    .iter()
                    .zip(arg_types)
                    .all(|(param, arg)| Self::parameter_type_accepts_arg_type(param, arg))
            }
        }
    }

    fn find_signature(&self, symbol: &str) -> Option<(String, String)> {
        let canonical = Self::canonical_doc_symbol(symbol);

        if canonical == symbol {
            if let Some(found) = self
                .vm
                .function_entries()
                .iter()
                .rev()
                .filter(|entry| !entry.flags.generated)
                .find_map(|entry| {
                    let qualified_name = entry.qualified_name.as_ref()?;
                    if !Self::symbol_matches(qualified_name, canonical) {
                        return None;
                    }
                    let signature = entry.signature.clone()?;
                    Some((qualified_name.clone(), signature))
                })
            {
                return Some(found);
            }
        }

        if let Some(entry) = self
            .docs
            .iter()
            .rev()
            .find(|entry| entry.kind == DocKind::Function && entry.qualified_name == canonical)
        {
            if let Some(signature) = entry.signature.clone() {
                return Some((entry.qualified_name.clone(), signature));
            }
        }

        if let Some(entry) = self.docs.iter().rev().find(|entry| {
            entry.kind == DocKind::Function
                && Self::symbol_matches(&entry.qualified_name, canonical)
        }) {
            if let Some(signature) = entry.signature.clone() {
                return Some((entry.qualified_name.clone(), signature));
            }
        }

        None
    }

    fn handle_sig(&self, symbol: &str) -> ReplResult {
        let trimmed = symbol.trim();
        if trimmed.is_empty() {
            return ReplResult::ok(ReplOutput::CommandOutput {
                rendered: Self::sig_help_lines(),
            });
        }
        if let Some(query) = self.parse_typed_call_query(trimmed) {
            return match query {
                Ok(query) => self.handle_sig_typed_call(trimmed, &query),
                Err(message) => ReplResult::ok(ReplOutput::CommandOutput {
                    rendered: vec![message],
                }),
            };
        }
        if trimmed.split_whitespace().count() != 1 {
            return ReplResult::ok(ReplOutput::CommandOutput {
                rendered: Self::sig_help_lines(),
            });
        }

        match self.find_signature(trimmed) {
            Some((qualified_name, signature)) => {
                let rendered = if let Some((module, tail)) = qualified_name.rsplit_once("::") {
                    if signature == tail || signature.starts_with(&format!("{tail}(")) {
                        format!("{module}::{signature}")
                    } else {
                        signature
                    }
                } else {
                    signature
                };
                ReplResult::ok(ReplOutput::SigResolved {
                    signature: rendered,
                })
            }
            None => ReplResult::ok(ReplOutput::EvalError {
                idx: self.results.len(),
                source: format!(":sig {trimmed}"),
                rendered: vec![format!("No signature found for {}", trimmed)],
            }),
        }
    }

    fn handle_sig_typed_call(&self, source_query: &str, query: &TypedCallQuery) -> ReplResult {
        let matches = self.match_typed_call_docs(query);
        match matches.as_slice() {
            [entry] => {
                let defined = entry
                    .signature
                    .clone()
                    .unwrap_or_else(|| entry.qualified_name.clone());
                let rendered = if query.callee == "dbg!" {
                    defined
                } else {
                    format!(
                        "defined:\n  {defined}\n\nspecialized:\n  {}({}) -> {}",
                        query.callee,
                        query.arg_types.join(", "),
                        signature_return_type(&defined).unwrap_or("_")
                    )
                };
                ReplResult::ok(ReplOutput::SigResolved {
                    signature: rendered,
                })
            }
            [] => ReplResult::ok(ReplOutput::CommandOutput {
                rendered: vec![format!("No signature found for {}", source_query)],
            }),
            entries => ReplResult::ok(ReplOutput::CommandOutput {
                rendered: Self::ambiguous_doc_lines(source_query, entries),
            }),
        }
    }

    fn parse_typed_call_query(&self, input: &str) -> Option<Result<TypedCallQuery, String>> {
        let open = input.find('(')?;
        if !input.ends_with(')') {
            return Some(Err(
                "Invalid typed call query: missing closing `)`.".to_string()
            ));
        }
        let callee = input[..open].trim();
        if callee.is_empty() || callee.chars().any(char::is_whitespace) {
            return Some(Err("Invalid typed call query: missing callee.".to_string()));
        }
        let args_src = &input[open + 1..input.len() - 1];
        let args = split_top_level_commas(args_src);
        let mut arg_types = Vec::with_capacity(args.len());
        for arg in args {
            let arg = arg.trim();
            if arg.is_empty() {
                return Some(Err("Invalid typed call query: empty argument.".to_string()));
            }
            let ty = match self.query_arg_type(arg) {
                Ok(ty) => ty,
                Err(message) => return Some(Err(message)),
            };
            arg_types.push(ty);
        }
        Some(Ok(TypedCallQuery {
            callee: Self::canonical_doc_symbol(callee).to_string(),
            arg_types,
        }))
    }

    fn query_arg_type(&self, arg: &str) -> Result<String, String> {
        if let Some(ty) = split_type_annotation(arg) {
            return Ok(ty.to_string());
        }
        if arg == "()" {
            return Ok("Unit".to_string());
        }
        if matches!(arg, "True" | "False") {
            return Ok("Boolean".to_string());
        }
        if is_string_literal(arg) {
            return Ok("String".to_string());
        }
        if is_float_literal(arg) {
            return Ok("Float".to_string());
        }
        if is_int_literal(arg) {
            return Ok("Int".to_string());
        }
        if is_bare_type_query(arg) {
            return Ok(arg.to_string());
        }
        if is_simple_name(arg) {
            if let Some(ty) = self.binding_type(arg) {
                return Ok(ty);
            }
            return Err(format!("Unknown query binding `{arg}`."));
        }
        Err(format!(
            "Unsupported typed call query argument `{arg}`. Use literals, existing bindings, or `_ : Type`."
        ))
    }

    fn binding_type(&self, name: &str) -> Option<String> {
        self.result_metas
            .iter()
            .rev()
            .flatten()
            .flat_map(|meta| meta.bindings.iter().rev())
            .find(|binding| binding.name == name)
            .map(|binding| binding.ty.clone())
    }

    fn handle_error_mode(&mut self, mode: Option<&str>) -> Vec<String> {
        let Some(mode) = mode else {
            return vec![format!(
                "error display mode: {}",
                self.error_display_mode.as_str()
            )];
        };

        match ErrorDisplayMode::parse(mode.trim()) {
            Some(parsed) => {
                self.error_display_mode = parsed;
                vec![format!("error display mode: {}", parsed.as_str())]
            }
            None => vec!["Usage: :error [full|summary]".to_string()],
        }
    }

    fn append_docs(&mut self, docs: Vec<DocEntry>) {
        for doc in docs {
            let exists = self.docs.iter().any(|existing| {
                existing.qualified_name == doc.qualified_name
                    && existing.kind == doc.kind
                    && existing.signature == doc.signature
                    && existing.doc == doc.doc
            });
            if !exists {
                self.docs.push(doc);
            }
        }
    }

    /// Evaluate one line and return a structured `ReplResult`.
    ///
    /// The unified entry point used by both CLI and TUI.
    pub fn handle_line(&mut self, line: &str) -> ReplResult {
        if self.pending.is_empty() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return ReplResult::ok(ReplOutput::CommandOutput { rendered: vec![] });
            }
            if let Some(cmd) = parse_repl_command(trimmed) {
                match cmd {
                    ReplCommand::Quit => {
                        return ReplResult::exit(ReplOutput::StatusMessage("quit".to_string()));
                    }
                    ReplCommand::Help { topic } => {
                        let rendered = self.handle_help(topic.as_deref());
                        return ReplResult::ok(ReplOutput::CommandOutput { rendered });
                    }
                    ReplCommand::Doc { symbol } => {
                        return self.handle_doc(&symbol);
                    }
                    ReplCommand::Sig { symbol } => {
                        return self.handle_sig(&symbol);
                    }
                    ReplCommand::Error { mode } => {
                        let rendered = self.handle_error_mode(mode.as_deref());
                        return ReplResult::ok(ReplOutput::CommandOutput { rendered });
                    }
                    ReplCommand::ValueRecall { arg } => {
                        let rendered = self.handle_value_recall(&arg);
                        return ReplResult::ok(ReplOutput::CommandOutput { rendered });
                    }
                    ReplCommand::Save { path } => {
                        let rendered = self.handle_save(&path);
                        return ReplResult::ok(ReplOutput::CommandOutput { rendered });
                    }
                    ReplCommand::Unknown { raw } => {
                        return ReplResult::ok(ReplOutput::CommandOutput {
                            rendered: vec![
                                format!("Unknown REPL command: {}", raw),
                                "Type :help for available REPL commands.".to_string(),
                            ],
                        });
                    }
                }
            }
        }

        let idx = self.results.len();
        let source = line.to_string();

        self.pending.push_str(line);
        self.pending.push('\n');
        self.sources
            .update_source(self.repl_source_id, self.pending.clone());

        let ast = match spire::parse_with_context(
            &self.pending,
            spire::ParserContext::repl(self.repl_source_id.0)
                .with_rules(derive_parse_rules(SourceKind::ReplChunk)),
        ) {
            Ok(ast) => ast,
            Err(e) if e.is_incomplete() => {
                return ReplResult::ok(ReplOutput::CommandOutput { rendered: vec![] });
            }
            Err(e) => {
                let message = e.message();
                let spec = diagnostics::parse_error_spec(&self.pending, message, e.span().clone());
                let rendered = error_display::diagnostic_lines_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                error_display::emit_diagnostic_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                self.pending.clear();
                self.bump_line(None, None);
                return ReplResult::ok(ReplOutput::EvalError {
                    idx,
                    source,
                    rendered,
                });
            }
        };

        if ast.is_empty() {
            self.pending.clear();
            return ReplResult::ok(ReplOutput::CommandOutput { rendered: vec![] });
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
                let spec =
                    diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None);
                let rendered = error_display::diagnostic_lines_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                error_display::emit_diagnostic_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                self.pending.clear();
                self.bump_line(None, None);
                return ReplResult::ok(ReplOutput::EvalError {
                    idx,
                    source,
                    rendered,
                });
            }
        };

        let docs = crate::collect_doc_entries(&[], &ast, Some(self.repl_module_path.as_str()));
        let resolved = match self.sigil_session.resolve(ast) {
            Ok(r) => r,
            Err(e) => {
                self.sigil_session.rollback(sigil_cp);
                self.scar_session.rollback(scar_cp);
                self.forge_session.rollback(forge_cp);
                let spec =
                    diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None);
                let rendered = error_display::diagnostic_lines_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                error_display::emit_diagnostic_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                self.pending.clear();
                self.bump_line(None, None);
                return ReplResult::ok(ReplOutput::EvalError {
                    idx,
                    source,
                    rendered,
                });
            }
        };

        let typed = match self.scar_session.typecheck_with_context(
            resolved,
            scar::TypecheckContext {
                runtime_policy: derive_runtime_policy(
                    CompileUnitKind::Repl,
                    SourceKind::ReplChunk,
                    None,
                ),
                enforce_builtin_type_contracts: false,
                allow_error_function_params: false,
            },
        ) {
            Ok(t) => t,
            Err(e) => {
                self.sigil_session.rollback(sigil_cp);
                self.scar_session.rollback(scar_cp);
                self.forge_session.rollback(forge_cp);
                let error = diagnostics::TypeErrorDiagnostic::new(e.message, e.span, e.hint);
                let spec =
                    diagnostics::type_error_spec_by_id(&self.sources, self.repl_source_id, &error);
                let rendered = error_display::diagnostic_lines_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                error_display::emit_diagnostic_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                self.pending.clear();
                self.bump_line(None, None);
                return ReplResult::ok(ReplOutput::EvalError {
                    idx,
                    source,
                    rendered,
                });
            }
        };

        let (mut chunk, mut meta) = match self.forge_session.codegen_chunk_repl_result(typed) {
            Ok(c) => c,
            Err(e) => {
                self.sigil_session.rollback(sigil_cp);
                self.scar_session.rollback(scar_cp);
                self.forge_session.rollback(forge_cp);
                let spec =
                    diagnostics::simple_error("CodegenError", &e.message, e.span.clone(), None);
                let rendered = error_display::diagnostic_lines_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                error_display::emit_diagnostic_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                self.pending.clear();
                self.bump_line(None, None);
                return ReplResult::ok(ReplOutput::EvalError {
                    idx,
                    source,
                    rendered,
                });
            }
        };
        meta.docs = docs.clone();
        chunk.docs = docs.clone();

        if let Some(repl_source) = self.sources.source(self.repl_source_id) {
            populate_error_template_lines(&mut chunk.error_templates, repl_source);
        }
        if let Some((source_str, file_name)) = self.sources.owned_context(self.repl_source_id) {
            self.vm.set_source(source_str, file_name);
        }

        match self.vm.push_atomic(chunk) {
            Ok(value) => {
                self.sync_scar_fun_index_with_vm();
                if let Some(rendered) = self.report_main_result_error_if_any(&value) {
                    self.bump_line(None, None);
                    self.pending.clear();
                    return ReplResult::ok(ReplOutput::EvalError {
                        idx,
                        source,
                        rendered,
                    });
                }

                let rendered = render::format_result_lines(&self.vm, Some(&value), Some(&meta));

                let mut all_rendered = rendered;
                if import_only {
                    for label in &import_result.success_labels {
                        all_rendered.push(format!("imported {}", label));
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
                self.append_docs(docs);
                self.bump_line(Some(value), Some(meta.clone()));
                self.pending.clear();
                ReplResult::ok(ReplOutput::EvalSuccess {
                    idx,
                    source,
                    rendered: all_rendered,
                })
            }
            Err(e) => {
                let location = e
                    .context
                    .call_site
                    .clone()
                    .or_else(|| self.vm.runtime_error_location());
                let rendered = error_display::runtime_error_lines_with_registry(
                    &e,
                    &self.sources,
                    self.repl_source_id,
                    location.clone(),
                    self.error_display_mode,
                );
                error_display::emit_runtime_error_with_registry(
                    &e,
                    &self.sources,
                    self.repl_source_id,
                    location,
                    self.error_display_mode,
                );
                self.bump_line(None, None);
                self.pending.clear();
                ReplResult::exit(ReplOutput::EvalError {
                    idx,
                    source,
                    rendered,
                })
            }
        }
    }

    fn handle_save(&mut self, arg: &str) -> Vec<String> {
        if arg.is_empty() {
            return vec!["Usage: :save <path.eldr>".to_string()];
        }

        let path = if arg.ends_with(".eldr") {
            arg.to_string()
        } else {
            format!("{}.eldr", arg)
        };

        let mut bytecode = self.vm.snapshot_bytecode();
        bytecode.docs = self.docs.clone();
        match bytecode.encode() {
            Err(e) => vec![format!("Error encoding bytecode: {}", e)],
            Ok(bytes) => match fs::write(&path, bytes) {
                Ok(()) => vec![format!("saved to {}", path)],
                Err(e) => vec![format!("Error writing {}: {}", path, e)],
            },
        }
    }

    fn handle_value_recall(&mut self, arg: &str) -> Vec<String> {
        if arg.is_empty() {
            self.bump_line(None, None);
            return vec!["Usage: :v <line>".to_string()];
        }

        let line_num = match arg.parse::<usize>() {
            Ok(n) if n > 0 => n,
            _ => {
                self.bump_line(None, None);
                return vec![format!("Invalid line number for :v: {}", arg)];
            }
        };

        if line_num > self.results.len() {
            self.bump_line(None, None);
            return vec![format!("No such line: {}", line_num)];
        }

        match self.results[line_num - 1].clone() {
            Some(value) => {
                let displayed = inspect_value(&self.vm, &value);
                self.bump_line(Some(value), None);
                vec![displayed]
            }
            None => {
                self.bump_line(None, None);
                vec![format!("Line {} has no value", line_num)]
            }
        }
    }
}

impl ReplEngine {
    fn sync_scar_fun_index_with_vm(&mut self) {
        let next_fun_idx = self
            .vm
            .bytecode()
            .functions
            .iter()
            .map(|entry| entry.fun_idx + 1)
            .max()
            .unwrap_or(0);
        self.scar_session.ensure_next_fun_idx_at_least(next_fun_idx);
    }
}

/// Find the source_id most likely to own `span`.
///
/// Strategy: among all staged modules whose source fully contains the span,
/// prefer those where the character at `span.start` is not ASCII whitespace
/// (i.e., the span points to actual code rather than incidental whitespace),
/// then pick the shortest qualifying source.  This correctly handles cases
/// where a short module (e.g. kernel.srt) has a coincidental span-in-range
/// while the real owner is a slightly longer module (e.g. list.srt) whose
/// code starts at that offset.
fn find_source_id_for_span(
    sources: &SourceRegistry,
    module_stages: &[Vec<StagedModule>],
    span: &Span,
    fallback: SourceId,
) -> SourceId {
    let mut best_code: Option<(SourceId, usize)> = None;
    let mut best_any: Option<(SourceId, usize)> = None;
    for stage in module_stages {
        for module in stage {
            if let Some(source) = sources.source(module.source_id) {
                let chars: Vec<char> = source.chars().collect();
                let len = chars.len();
                if len < span.end {
                    continue;
                }
                let is_code = chars
                    .get(span.start)
                    .is_some_and(|ch| !ch.is_ascii_whitespace());
                if is_code {
                    match best_code {
                        None => best_code = Some((module.source_id, len)),
                        Some((_, bl)) if len < bl => best_code = Some((module.source_id, len)),
                        _ => {}
                    }
                }
                match best_any {
                    None => best_any = Some((module.source_id, len)),
                    Some((_, bl)) if len < bl => best_any = Some((module.source_id, len)),
                    _ => {}
                }
            }
        }
    }
    best_code.or(best_any).map(|(id, _)| id).unwrap_or(fallback)
}

fn load_error_from_parse_failure(
    sources: &SourceRegistry,
    error: ModuleStageParseError,
) -> LoadError {
    let file_name = sources
        .file_name(error.source_id)
        .unwrap_or("<unknown>")
        .to_string();
    LoadError::BootstrapFailed {
        phase: "parse".into(),
        file_name,
        message: error.message().to_string(),
    }
}

fn load_error_from_span_failure(
    sources: &SourceRegistry,
    module_stages: &[Vec<StagedModule>],
    span: &Span,
    fallback: SourceId,
    phase: &str,
    message: impl Into<String>,
) -> LoadError {
    let source_id = find_source_id_for_span(sources, module_stages, span, fallback);
    let file_name = sources
        .file_name(source_id)
        .unwrap_or("<unknown>")
        .to_string();
    LoadError::BootstrapFailed {
        phase: phase.to_string(),
        file_name,
        message: message.into(),
    }
}

pub(crate) fn parse_module_stages_from_sources(
    sources: &SourceRegistry,
    module_stages: &[Vec<StagedModule>],
    _compile_unit_kind: CompileUnitKind,
) -> Result<Vec<Vec<sigil::StagedModuleAst>>, ModuleStageParseError> {
    let mut staged_module_asts = Vec::with_capacity(module_stages.len());
    let mut seen_module_paths: HashMap<String, (String, bool)> = HashMap::new();

    for stage in module_stages {
        let mut stage_ast = Vec::new();
        let parsed_stage = parse_stage_modules_parallel(sources, stage);
        for (module, lowered_modules) in stage.iter().zip(parsed_stage) {
            for lowered in lowered_modules? {
                if !lowered.module_path.is_empty() {
                    let is_impl_owner = crate::lowered_module_is_impl_owner(&lowered);
                    let second_file_name = sources
                        .file_name(module.source_id)
                        .unwrap_or("<unknown>")
                        .to_string();
                    if let Some((first_file_name, first_is_impl_owner)) =
                        seen_module_paths.get(&lowered.module_path)
                    {
                        if !(*first_is_impl_owner || is_impl_owner) {
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
                    }
                    seen_module_paths.insert(
                        lowered.module_path.clone(),
                        (second_file_name, is_impl_owner),
                    );
                }

                stage_ast.push(sigil::StagedModuleAst {
                    module_path: lowered.module_path,
                    ast: crate::rebase_module_ast_spans(lowered.ast, module.source_id),
                    module_doc: lowered.module_doc,
                    auto_import: lowered.auto_import,
                });
            }
        }
        staged_module_asts.push(stage_ast);
    }

    Ok(staged_module_asts)
}

fn parse_stage_modules_parallel(
    sources: &SourceRegistry,
    stage: &[StagedModule],
) -> Vec<Result<Vec<crate::LoweredModuleAst>, ModuleStageParseError>> {
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(stage.len());
        for module in stage {
            handles.push(
                std::thread::Builder::new()
                    .stack_size(STAGE_PARSE_WORKER_STACK_SIZE)
                    .spawn_scoped(scope, move || {
                        let raw_module_source = sources.source(module.source_id).unwrap_or("");
                        let module_source = crate::strip_test_annotations(raw_module_source);
                        let parsed = spire::parse_with_context(
                            &module_source,
                            spire::ParserContext::module(module.source_id.0, None)
                                .with_rules(derive_parse_rules(module.source_kind)),
                        )
                        .map_err(|e| ModuleStageParseError {
                            source_id: module.source_id,
                            kind: ModuleStageParseErrorKind::Parse {
                                message: e.message().to_string(),
                                span: e.span().clone(),
                            },
                        })?;
                        let fallback_module_path = if parsed
                            .iter()
                            .any(|stmt| matches!(stmt, spire::ast::Ast::ConstDef(_, _, _, _, _)))
                            && parsed.iter().all(|stmt| {
                                matches!(
                                    stmt,
                                    spire::ast::Ast::Import(_, _, _)
                                        | spire::ast::Ast::ConstDef(_, _, _, _, _)
                                )
                            }) {
                            Some(module.module_path.as_str())
                        } else {
                            None
                        };
                        Ok(crate::lower_module_source_ast(parsed, fallback_module_path))
                    })
                    .expect("stage parser worker thread should spawn"),
            );
        }

        handles
            .into_iter()
            .map(|handle| match handle.join() {
                Ok(result) => result,
                Err(payload) => panic::resume_unwind(payload),
            })
            .collect()
    })
}

pub(crate) fn xldr_version() -> &'static str {
    XLDR_VERSION
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut paren_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in input.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            ',' if paren_depth == 0 && angle_depth == 0 => {
                parts.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    let tail = input[start..].trim();
    if !tail.is_empty() || !input.trim().is_empty() {
        parts.push(tail);
    }
    parts
}

fn split_type_annotation(input: &str) -> Option<&str> {
    let (name, ty) = input.split_once(':')?;
    let name = name.trim();
    let ty = ty.trim();
    if ty.is_empty() || !(name == "_" || is_simple_name(name)) {
        return None;
    }
    Some(ty)
}

fn is_simple_name(input: &str) -> bool {
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_bare_type_query(input: &str) -> bool {
    input
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn is_string_literal(input: &str) -> bool {
    input.len() >= 2 && input.starts_with('"') && input.ends_with('"')
}

fn is_int_literal(input: &str) -> bool {
    let digits = input.strip_prefix('-').unwrap_or(input);
    !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit())
}

fn is_float_literal(input: &str) -> bool {
    let digits = input.strip_prefix('-').unwrap_or(input);
    let Some((lhs, rhs)) = digits.split_once('.') else {
        return false;
    };
    !lhs.is_empty()
        && !rhs.is_empty()
        && lhs.chars().all(|ch| ch.is_ascii_digit())
        && rhs.chars().all(|ch| ch.is_ascii_digit())
}

fn signature_return_type(signature: &str) -> Option<&str> {
    signature.rsplit_once("->").map(|(_, ret)| ret.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bootstrap_engine_with_module(source: &str, module_path: &str) -> ReplEngine {
        let repl_sources =
            loader::collect_repl_sources_with_module_stages(&[vec![crate::ModuleInput {
                file_name: "lib/bad.srt".into(),
                source: source.into(),
                module_path: module_path.into(),
            }]])
            .expect("test module stage should load");
        let forge_session = forge::ForgeSession::new();
        let vm = eldr::VM::new_interactive(forge_session.type_registry());

        ReplEngine {
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
            result_metas: Vec::new(),
            symbols: ["Ok", "Err"]
                .into_iter()
                .map(str::to_string)
                .chain(BUILTIN_METAS.iter().map(|meta| meta.name.to_string()))
                .collect(),
            docs: Vec::new(),
            auto_import_modules: BTreeSet::new(),
            error_display_mode: ErrorDisplayMode::Full,
        }
    }

    fn expect_bootstrap_failure(source: &str, phase: &str, message_fragment: &str) -> LoadError {
        let mut engine = bootstrap_engine_with_module(source, "Broken");
        let err = engine
            .bootstrap_std_modules()
            .expect_err("bootstrap should fail");
        match &err {
            LoadError::BootstrapFailed {
                phase: actual_phase,
                file_name,
                message,
            } => {
                assert_eq!(actual_phase, phase);
                assert!(
                    file_name == "lib/bad.srt" || file_name == "bootstrap.srt",
                    "unexpected bootstrap failure file `{}`",
                    file_name
                );
                assert!(
                    message.contains(message_fragment),
                    "expected `{}` to contain `{}`",
                    message,
                    message_fragment
                );
            }
            other => panic!("expected bootstrap failure, got {:?}", other),
        }
        err
    }

    #[test]
    fn bootstrap_std_modules_returns_parse_failure() {
        expect_bootstrap_failure("defmod Broken { def nope( }", "parse", "Expected");
    }

    #[test]
    fn bootstrap_std_modules_returns_resolve_failure() {
        expect_bootstrap_failure(
            "defmod Broken { def nope() -> Int { missing } }",
            "resolve",
            "Undefined variable",
        );
    }

    #[test]
    fn bootstrap_std_modules_returns_typecheck_failure() {
        expect_bootstrap_failure(
            "defmod Broken { def nope() -> Int { \"bad\" } }",
            "typecheck",
            "expected Int",
        );
    }

    #[test]
    fn bootstrap_std_modules_returns_runtime_failure() {
        let mut engine =
            bootstrap_engine_with_module("defmod Broken { def nope() -> Int { 1 } }", "Broken");
        engine.vm = eldr::VM::new_interactive(engine.forge_session.type_registry());
        engine
            .vm
            .push_atomic(sindr::ir::BytecodeChunk {
                opcodes: vec![sindr::ir::Opcode::Halt],
                source_map: None,
                const_base: 0,
                constants: vec![sindr::ir::Constant::Int(sindr::primitives::int(1))],
                new_locals: 0,
                type_entries: Vec::new(),
                dbg_template_base: 0,
                dbg_templates: Vec::new(),
                error_template_base: 0,
                error_templates: Vec::new(),
                functions: Vec::new(),
                docs: Vec::new(),
            })
            .expect("vm bootstrap corruption setup should succeed");

        let err = engine
            .bootstrap_std_modules()
            .expect_err("bootstrap should fail at runtime");
        match err {
            LoadError::BootstrapFailed {
                phase,
                file_name,
                message,
            } => {
                assert_eq!(phase, "runtime");
                assert_eq!(file_name, "bootstrap.srt");
                assert!(message.contains("Chunk constant base mismatch"));
            }
            other => panic!("expected runtime bootstrap failure, got {:?}", other),
        }
    }
}
