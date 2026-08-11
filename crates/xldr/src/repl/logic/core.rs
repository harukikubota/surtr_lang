#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::panic;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use diagnostics::{DiagnosticSpec, SourceId, SourceRegistry};
use eldr::builtin::inspect_value;
use eldr::interactive::InteractiveChunkPolicy;
use eldr::value::{TypeKind, Value};
use forge::bytecode::populate_error_template_lines;
use scar::typed::{TypedInner, TypedNode, TypedPattern};
use scar::types::Ty;
use serde::{Deserialize, Serialize};
use sigil::error::ResolveError;
use sindr::builtin::builtin_function_metas;
use sindr::ir::{DocEntry, DocKind, SignatureEntry};
use sindr::names::SymbolCapabilities;
use sindr::policy::CompileUnitKind;
use spire::ast::{Ast, AstTy, ImportSpec, Span};

use super::command::{
    parse_repl_command, repl_command_help_lines, repl_command_spec_for_alias, repl_command_specs,
    repl_command_topic_help_lines, ReplCommand, ReplCommandArgCompletion, ReplCommandKind,
    ReplCommandSpec,
};
use super::output::{ReplOutput, ReplResult};
use super::preload::PreloadCompileMode;
use super::query::{
    ast_ty_from_query_arg, format_query_ty, parse_binding_query_type, parse_repl_query,
    parse_signature_type, OperatorTargetQuery, QueryArg, QueryArgKind, ReplQuery, TypedCallQuery,
};
use super::{eval, render, session};
use crate::error_display::StackTraceDisplayMode;
use crate::loader::{self, StagedModule};
use crate::ErrorDisplayMode;
use crate::{
    collect_additional_default_std_module_inputs, error_display, LoadError, ModuleStageParseError,
    ModuleStageParseErrorKind, SourceKind,
};

const XLDR_VERSION: &str = env!("CARGO_PKG_VERSION");
const OPERATOR_DOC_TARGETS: &[(&str, &str)] = &[
    ("+", "Add::add"),
    ("-", "Sub::sub"),
    ("*", "Mul::mul"),
    ("&&", "and"),
    ("||", "or"),
    ("==", "Eq::eq"),
    ("!=", "Neq::neq"),
    ("<", "Compare::lt"),
    ("<=", "Compare::lte"),
    (">", "Compare::gt"),
    (">=", "Compare::gte"),
    ("/", "Compose::compose"),
    ("++", "Concat::concat"),
    ("|>", "PipeApply::pipe_apply"),
    ("|*>", "Functor::fmap"),
    ("|>=", "Monad::bind"),
    (">>", "Composable::compose"),
    (">*", "LiftComposable::lift_compose"),
    (">=>", "KleisliComposable::kleisli_compose"),
];
const OPERATOR_DOC_TRAIT_ALIASES: &[(&str, &str)] = &[
    ("+", "Add"),
    ("-", "Sub"),
    ("*", "Mul"),
    ("&&", "and"),
    ("||", "or"),
    ("==", "Eq"),
    ("!=", "Neq"),
    ("<", "Compare"),
    ("<=", "Compare"),
    (">", "Compare"),
    (">=", "Compare"),
    ("/", "Compose"),
    ("++", "Concat"),
    ("|>", "PipeApply"),
    ("|*>", "Functor"),
    ("|>=", "Monad"),
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
    ("compare", "Compare"),
    ("lt", "Compare"),
    ("lte", "Compare"),
    ("gt", "Compare"),
    ("gte", "Compare"),
    ("concat", "Concat"),
];
const COMPARE_METHOD_DOC_TARGETS: &[(&str, &str)] = &[
    ("compare", "Compare::compare"),
    ("lt", "Compare::lt"),
    ("lte", "Compare::lte"),
    ("gt", "Compare::gt"),
    ("gte", "Compare::gte"),
];
const REPL_UNRESOLVED_TYPE_MESSAGE: &str = "Cannot persist binding with unresolved type variable.";
const REPL_UNRESOLVED_TYPE_HINT: &str =
    "Add a type annotation or use the value in a context that determines the success type.";
const STAGE_PARSE_WORKER_STACK_SIZE: usize = 8 * 1024 * 1024;

fn duration_to_nanos(elapsed: Duration) -> u64 {
    elapsed.as_nanos().min(u128::from(u64::MAX)) as u64
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

/// Error returned when preloading source files into a REPL engine.
#[derive(Debug, Clone)]
pub enum ReplLoadError {
    SourceReadFailed {
        file_name: String,
        message: String,
    },
    Diagnostic {
        phase: String,
        sources: SourceRegistry,
        source_id: SourceId,
        spec: DiagnosticSpec,
    },
    Load(LoadError),
    Runtime {
        file_name: String,
        message: String,
    },
}

impl std::fmt::Display for ReplLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceReadFailed { file_name, message } => {
                write!(f, "cannot read {}: {}", file_name, message)
            }
            Self::Diagnostic { .. } => write!(f, "preload diagnostic"),
            Self::Load(error) => write!(f, "{}", error),
            Self::Runtime { file_name, message } => {
                write!(
                    f,
                    "runtime error while preloading {}: {}",
                    file_name, message
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
struct PreparedScriptPreload {
    file_name: String,
    source_for_parse: String,
    include_modules: Vec<crate::ModuleInput>,
}

#[derive(Clone)]
struct PreloadedChunkState {
    sources: SourceRegistry,
    builtin_source_id: SourceId,
    repl_source_id: SourceId,
    repl_module_path: String,
    module_stages: Vec<Vec<StagedModule>>,
    declaration_index: sigil::DeclarationIndex,
    sigil_session: sigil::SigilSession,
    scar_checkpoint: scar::ScarCheckpoint,
    vm: eldr::InteractiveVm,
    docs: Vec<DocEntry>,
    signatures: Vec<SignatureEntry>,
    process_metadata: BTreeMap<String, ReplProcessMetadata>,
    symbols: BTreeSet<String>,
    auto_import_modules: BTreeSet<String>,
    auto_import_records: Vec<ReplImportRecord>,
    script_runtime_inputs: Vec<String>,
    script_preload_docs: Vec<DocEntry>,
    script_preload_signatures: Vec<SignatureEntry>,
    import_records: Vec<ReplImportRecord>,
    def_records: Vec<ReplDefRecord>,
}

#[derive(Debug, Clone, PartialEq)]
struct ReplProcessMetadata {
    kind: spire::ast::ProcessKind,
    instance: spire::ast::ProcessInstance,
    handler_specs: Vec<spire::ast::ProcessRuntimeHandlerSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplTypeDisplayCategory {
    Type,
    Struct,
    Record,
    Enum,
    Closure,
    Capture,
    FacetPath,
}

impl ReplTypeDisplayCategory {
    fn identity_label(self) -> &'static str {
        match self {
            Self::Type => "TypeIdentity::Type",
            Self::Struct => "TypeIdentity::Struct",
            Self::Record => "TypeIdentity::Record",
            Self::Enum => "TypeIdentity::Enum",
            Self::Closure => "TypeIdentity::Closure",
            Self::Capture => "TypeIdentity::Capture",
            Self::FacetPath => "TypeIdentity::FacetPath",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplSessionPhase {
    Bootstrap,
    Preload,
    Live,
}

impl ReplSessionPhase {
    fn execution_policy(self) -> InteractiveChunkPolicy {
        match self {
            Self::Bootstrap | Self::Preload => InteractiveChunkPolicy::Preload,
            Self::Live => InteractiveChunkPolicy::ReplAppendOnly,
        }
    }
}

#[derive(Debug, Default)]
struct ReplImportResult {
    imported_symbols: Vec<String>,
    success_labels: Vec<String>,
}

#[derive(Debug, Clone, Default)]
enum ReplReloadSeed {
    #[default]
    Empty,
    ProjectModuleStages(Vec<Vec<crate::ModuleInput>>),
    Sources {
        module: Option<(String, String)>,
        script: Option<(String, String)>,
    },
}

#[derive(Debug, Clone)]
struct ReplHistoryEntry {
    line: usize,
    source: String,
}

#[derive(Debug, Clone)]
struct ReplBindingRecord {
    line: usize,
    name: String,
    ty: String,
}

#[derive(Debug, Clone)]
struct ReplImportRecord {
    line: usize,
    src: String,
    item: String,
    via: String,
}

#[derive(Debug, Clone)]
struct ReplDefRecord {
    line: usize,
    name: String,
    arity: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplCompletionKind {
    Variable,
    TypeConstructor,
    TypePath,
    FunctionCall,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplCompletionCandidate {
    pub label: String,
    pub replacement: String,
    pub kind: ReplCompletionKind,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub replace_start: usize,
    pub replace_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplSignatureHelp {
    pub lines: Vec<String>,
    pub active_parameter: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionTelemetry {
    pub input_event_to_ui_handler_ns: Option<u64>,
    pub completion_queue_ns: Option<u64>,
    pub completion_compute_ns: Option<u64>,
    pub completion_apply_ns: Option<u64>,
    pub completion_render_ns: Option<u64>,
    pub total_key_to_visible_response_ns: Option<u64>,
}

impl CompletionTelemetry {
    pub fn record_input_event_to_ui_handler(&mut self, elapsed: Duration) {
        self.input_event_to_ui_handler_ns = Some(duration_to_nanos(elapsed));
    }

    pub fn record_completion_compute(&mut self, elapsed: Duration) {
        self.completion_compute_ns = Some(duration_to_nanos(elapsed));
    }

    pub fn record_completion_queue(&mut self, elapsed: Duration) {
        self.completion_queue_ns = Some(duration_to_nanos(elapsed));
    }

    pub fn record_completion_apply(&mut self, elapsed: Duration) {
        self.completion_apply_ns = Some(duration_to_nanos(elapsed));
    }

    pub fn record_completion_render(&mut self, elapsed: Duration) {
        self.completion_render_ns = Some(duration_to_nanos(elapsed));
    }

    pub fn record_total_key_to_visible_response(&mut self, elapsed: Duration) {
        self.total_key_to_visible_response_ns = Some(duration_to_nanos(elapsed));
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplCompletion {
    pub candidates: Vec<ReplCompletionCandidate>,
    pub signature: Option<ReplSignatureHelp>,
    pub telemetry: CompletionTelemetry,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplCompletionContext {
    input_support: surtr_analysis::ReplInputSupportContext,
}

impl ReplCompletionContext {
    pub fn completions(&self, input: &str, cursor: usize) -> ReplCompletion {
        let started = Instant::now();
        let use_site = surtr_analysis::repl_completion_use_site(input, cursor);
        let (candidates, signature) =
            if matches!(use_site, surtr_analysis::ReplCompletionUseSite::CommandHead) {
                (
                    Self::command_head_completion_candidates(input, cursor),
                    None,
                )
            } else {
                let support = self.input_support.input_support(input, cursor, use_site);
                let candidates = support
                    .candidates
                    .into_iter()
                    .map(ReplEngine::repl_completion_candidate_from_analysis)
                    .collect::<Vec<_>>();
                let signature = support.signature.map(|signature| ReplSignatureHelp {
                    lines: signature.lines,
                    active_parameter: signature.active_parameter,
                });
                (candidates, signature)
            };
        let mut telemetry = CompletionTelemetry::default();
        telemetry.record_completion_compute(started.elapsed());
        ReplCompletion {
            candidates,
            signature,
            telemetry,
        }
    }

    pub fn should_request(input: &str, cursor: usize) -> bool {
        surtr_analysis::ReplInputSupportContext::should_request(input, cursor)
    }

    fn command_head_completion_candidates(
        input: &str,
        cursor: usize,
    ) -> Vec<ReplCompletionCandidate> {
        if !completion_allowed_at_cursor(input, cursor) {
            return Vec::new();
        }
        let (replace_start, replace_end, prefix) = completion_token(input, cursor);
        let mut candidates = Vec::new();
        for spec in repl_command_specs() {
            for alias in spec.aliases {
                let label = format!(":{alias}");
                if !label.starts_with(&prefix) {
                    continue;
                }
                candidates.push(ReplCompletionCandidate {
                    label: label.clone(),
                    replacement: label,
                    kind: ReplCompletionKind::FunctionCall,
                    detail: Some(spec.completion_detail()),
                    documentation: None,
                    replace_start,
                    replace_end,
                });
            }
        }
        candidates
    }

    #[cfg(test)]
    pub(crate) fn insert_callable_signature_for_test(
        &mut self,
        label: &str,
        qualified_name: &str,
        signature: &str,
    ) {
        self.input_support
            .apply_update(surtr_analysis::ReplInputSupportUpdate {
                symbols: Vec::new(),
                callable_signatures: vec![surtr_analysis::CallableSignature {
                    label: label.to_string(),
                    qualified_name: qualified_name.to_string(),
                    signature: signature.to_string(),
                }],
            });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FacetCompletionAssist {
    candidates: Vec<ReplCompletionCandidate>,
    signature: ReplSignatureHelp,
}

#[derive(Debug, Clone, PartialEq)]
struct FacetStep {
    ty: AstTy,
    fallible: bool,
    variant: bool,
}

#[derive(Debug, Clone)]
struct ReplCommandArgContext<'a> {
    spec: ReplCommandSpec,
    arg_start: usize,
    cursor: usize,
    args_before_cursor: &'a str,
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
    vm: eldr::InteractiveVm,
    pending: String,
    next_line: usize,
    results: Vec<Option<Value>>,
    result_metas: Vec<Option<forge::ChunkMeta>>,
    symbols: BTreeSet<String>,
    docs: Vec<DocEntry>,
    signatures: Vec<SignatureEntry>,
    process_metadata: BTreeMap<String, ReplProcessMetadata>,
    auto_import_modules: BTreeSet<String>,
    auto_import_records: Vec<ReplImportRecord>,
    reload_seed: ReplReloadSeed,
    replay_inputs: Vec<String>,
    history_entries: Vec<ReplHistoryEntry>,
    binding_records: Vec<ReplBindingRecord>,
    import_records: Vec<ReplImportRecord>,
    def_records: Vec<ReplDefRecord>,
    completion_context_cache: RefCell<Option<ReplCompletionContext>>,
    #[cfg(test)]
    completion_context_builds: Cell<usize>,
    startup_results: Vec<ReplResult>,
    error_display_mode: ErrorDisplayMode,
    stack_trace_display_mode: StackTraceDisplayMode,
}

impl ReplEngine {
    fn execute_vm_chunk(
        &mut self,
        chunk: sindr::ir::BytecodeChunk,
        phase: ReplSessionPhase,
    ) -> Result<eldr::interactive::ChunkExecution, eldr::RuntimeError> {
        self.vm.push_chunk(chunk, phase.execution_policy())
    }

    pub fn new() -> Result<Self, LoadError> {
        let std_module_inputs = collect_additional_default_std_module_inputs()?;
        let repl_sources = loader::collect_repl_sources_with_module_stages(&[std_module_inputs])?;
        let forge_session = forge::ForgeSession::new();
        let vm = session::empty_interactive_vm(forge_session.type_registry());
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
            startup_results: Vec::new(),
            results: Vec::new(),
            result_metas: Vec::new(),
            symbols: ["Ok", "Err"]
                .into_iter()
                .map(str::to_string)
                .chain(
                    builtin_function_metas()
                        .iter()
                        .map(|meta| meta.name.to_string()),
                )
                .collect(),
            docs: Vec::new(),
            signatures: Vec::new(),
            process_metadata: BTreeMap::new(),
            auto_import_modules: BTreeSet::new(),
            auto_import_records: Vec::new(),
            reload_seed: ReplReloadSeed::Empty,
            replay_inputs: Vec::new(),
            history_entries: Vec::new(),
            binding_records: Vec::new(),
            import_records: Vec::new(),
            def_records: Vec::new(),
            completion_context_cache: RefCell::new(None),
            #[cfg(test)]
            completion_context_builds: Cell::new(0),
            error_display_mode: ErrorDisplayMode::Full,
            stack_trace_display_mode: StackTraceDisplayMode::Off,
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
        let signatures = bytecode.signatures.clone();
        let forge_session = forge::ForgeSession::from_bytecode(&bytecode);
        let vm = session::bytecode_interactive_vm(bytecode);

        // Populate completion symbols from the pre-loaded function table.
        let mut symbols: BTreeSet<String> = ["Ok", "Err"]
            .into_iter()
            .map(str::to_string)
            .chain(
                builtin_function_metas()
                    .iter()
                    .map(|meta| meta.name.to_string()),
            )
            .collect();
        for entry in vm.bytecode().functions.iter() {
            if let Some(name) = &entry.qualified_name {
                let surface_name = crate::surface_rendered_name(name);
                symbols.insert(surface_name.clone());
                if let Some(short) = surface_name.rsplit("::").next() {
                    symbols.insert(short.to_string());
                }
            }
        }
        for entry in vm.bytecode().type_registry.entries().iter() {
            let surface_name = crate::surface_rendered_name(&entry.name);
            symbols.insert(surface_name.clone());
            if let Some(short) = surface_name.rsplit("::").next() {
                symbols.insert(short.to_string());
            }
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
            signatures,
            process_metadata: BTreeMap::new(),
            auto_import_modules: BTreeSet::new(),
            auto_import_records: Vec::new(),
            reload_seed: ReplReloadSeed::Empty,
            replay_inputs: Vec::new(),
            history_entries: Vec::new(),
            binding_records: Vec::new(),
            import_records: Vec::new(),
            def_records: Vec::new(),
            completion_context_cache: RefCell::new(None),
            #[cfg(test)]
            completion_context_builds: Cell::new(0),
            startup_results: vec![Self::eldr_partial_semantic_restore_notice()],
            error_display_mode: ErrorDisplayMode::Full,
            stack_trace_display_mode: StackTraceDisplayMode::Off,
        };
        // Set up sigil / scar scope for stdlib without re-executing bytecode.
        engine
            .bootstrap_std_modules_scope_only()
            .map_err(EldrLoadError::Load)?;
        Ok(engine)
    }

    pub fn from_script_file(path: &str) -> Result<Self, ReplLoadError> {
        Self::from_preload_files(None, Some(path))
    }

    pub fn from_module_file(path: &str) -> Result<Self, ReplLoadError> {
        Self::from_preload_files(Some(path), None)
    }

    pub fn from_script_source(file_name: &str, source: &str) -> Result<Self, ReplLoadError> {
        Self::from_preload_sources(None, Some((file_name, source)))
    }

    pub fn from_module_source(file_name: &str, source: &str) -> Result<Self, ReplLoadError> {
        Self::from_preload_sources(Some((file_name, source)), None)
    }

    pub fn from_project_module_stages(
        module_input_stages: &[Vec<crate::ModuleInput>],
    ) -> Result<Self, ReplLoadError> {
        let state = compile_project_repl_chunk(module_input_stages)?;
        let mut engine = Self::from_preloaded_state(state)?;
        engine.reload_seed = ReplReloadSeed::ProjectModuleStages(module_input_stages.to_vec());
        Ok(engine)
    }

    pub fn from_project_runner_source(
        input: surtr_analysis::ProjectRunnerSourceInput,
    ) -> Result<Self, ReplLoadError> {
        let project_file = input.project_file.to_string_lossy().into_owned();
        let module_input_stages =
            crate::project_runner_module_input_stages(input).map_err(|error| {
                ReplLoadError::Runtime {
                    file_name: project_file,
                    message: error.to_string(),
                }
            })?;
        Self::from_project_module_stages(&module_input_stages)
    }

    pub fn from_project_runner_file(
        path: &str,
        profile: Option<&str>,
    ) -> Result<Self, ReplLoadError> {
        let selected_profile = profile.unwrap_or("main").to_string();
        let source = fs::read_to_string(path).map_err(|e| ReplLoadError::SourceReadFailed {
            file_name: path.to_string(),
            message: e.to_string(),
        })?;
        Self::from_project_runner_source(surtr_analysis::ProjectRunnerSourceInput {
            project_file: path.into(),
            selected_profile: selected_profile.clone(),
            normalized_args: vec![("profile".to_string(), selected_profile)],
            active_file: None,
            source,
        })
    }

    pub fn from_preload_files(
        module_path: Option<&str>,
        script_path: Option<&str>,
    ) -> Result<Self, ReplLoadError> {
        let module_source = match module_path {
            Some(path) => Some((
                path,
                fs::read_to_string(path).map_err(|e| ReplLoadError::SourceReadFailed {
                    file_name: path.to_string(),
                    message: e.to_string(),
                })?,
            )),
            None => None,
        };
        let script_source = match script_path {
            Some(path) => Some((
                path,
                fs::read_to_string(path).map_err(|e| ReplLoadError::SourceReadFailed {
                    file_name: path.to_string(),
                    message: e.to_string(),
                })?,
            )),
            None => None,
        };

        Self::from_preload_sources(
            module_source
                .as_ref()
                .map(|(path, source)| (*path, source.as_str())),
            script_source
                .as_ref()
                .map(|(path, source)| (*path, source.as_str())),
        )
    }

    pub fn from_preload_sources(
        module: Option<(&str, &str)>,
        script: Option<(&str, &str)>,
    ) -> Result<Self, ReplLoadError> {
        let state = compile_preloaded_repl_chunk(module, script)?;
        let mut engine = Self::from_preloaded_state(state)?;
        engine.reload_seed = ReplReloadSeed::Sources {
            module: module.map(|(file_name, source)| (file_name.to_string(), source.to_string())),
            script: script.map(|(file_name, source)| (file_name.to_string(), source.to_string())),
        };
        Ok(engine)
    }

    fn from_preloaded_state(state: PreloadedChunkState) -> Result<Self, ReplLoadError> {
        let forge_session = forge::ForgeSession::from_bytecode(&state.vm.snapshot_bytecode());

        Ok(Self {
            sources: state.sources,
            builtin_source_id: state.builtin_source_id,
            module_stages: state.module_stages,
            declaration_index: state.declaration_index,
            repl_source_id: state.repl_source_id,
            repl_module_path: state.repl_module_path.clone(),
            sigil_session: state.sigil_session,
            scar_session: {
                let mut session = scar::ScarSession::new();
                session.rollback(state.scar_checkpoint);
                session
            },
            forge_session,
            vm: state.vm,
            pending: String::new(),
            next_line: 1,
            results: Vec::new(),
            result_metas: Vec::new(),
            symbols: state.symbols,
            docs: state.docs,
            signatures: state.signatures,
            process_metadata: state.process_metadata,
            auto_import_modules: state.auto_import_modules,
            auto_import_records: state.auto_import_records,
            reload_seed: ReplReloadSeed::Empty,
            replay_inputs: Vec::new(),
            history_entries: Vec::new(),
            binding_records: Vec::new(),
            import_records: state.import_records.clone(),
            def_records: state.def_records.clone(),
            completion_context_cache: RefCell::new(None),
            #[cfg(test)]
            completion_context_builds: Cell::new(0),
            startup_results: Vec::new(),
            error_display_mode: ErrorDisplayMode::Full,
            stack_trace_display_mode: StackTraceDisplayMode::Off,
        })
        .map(|mut engine| {
            engine.append_docs(state.script_preload_docs.clone());
            engine.append_signatures(state.script_preload_signatures.clone());
            engine.sync_scar_fun_index_with_vm();
            engine
        })
        .and_then(|mut engine| {
            let script_runtime_inputs = state.script_runtime_inputs;
            for input in script_runtime_inputs {
                let result = engine.handle_line(&input);
                if result.should_exit {
                    return Err(ReplLoadError::Runtime {
                        file_name: "<repl-preload>".to_string(),
                        message: "preloaded script requested REPL exit".to_string(),
                    });
                }
                match result.output {
                    ReplOutput::EvalSuccess { .. }
                    | ReplOutput::PlainText { .. }
                    | ReplOutput::StyledDoc { .. }
                    | ReplOutput::StatusMessage(_) => {
                        engine.startup_results.push(result);
                    }
                    ReplOutput::EvalError { rendered, .. } => {
                        return Err(ReplLoadError::Runtime {
                            file_name: "<repl-preload>".to_string(),
                            message: rendered.join("\n"),
                        });
                    }
                    ReplOutput::Diagnostic {
                        rendered,
                        summary_tail,
                    } => {
                        return Err(ReplLoadError::Runtime {
                            file_name: "<repl-preload>".to_string(),
                            message: rendered
                                .into_iter()
                                .chain(summary_tail.into_iter())
                                .collect::<Vec<_>>()
                                .join("\n"),
                        });
                    }
                    ReplOutput::DocResolved { .. } | ReplOutput::EvalStarted { .. } => {}
                }
            }
            engine.vm.enable_repl_host_io_buffering();
            Ok(engine)
        })
    }

    pub fn take_startup_results(&mut self) -> Vec<ReplResult> {
        std::mem::take(&mut self.startup_results)
    }

    fn eldr_partial_semantic_restore_notice() -> ReplResult {
        Self::plain(vec![
            ".eldr runtime image loaded; compile semantic metadata for user definitions is not restored yet.".to_string(),
        ])
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
        self.auto_import_records =
            Self::collect_auto_import_records(&module_stages, &declaration_index);

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

        let typed = match self
            .scar_session
            .typecheck_with_context(resolved, Self::std_definition_typecheck_context())
        {
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
        let signatures = crate::collect_signature_entries(&module_stages, &[], None);
        self.process_metadata = collect_process_metadata(&module_stages);
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

        if let Err(e) = self.execute_vm_chunk(chunk, ReplSessionPhase::Bootstrap) {
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
            self.insert_surface_symbol(name);
        }
        self.append_docs(docs);
        self.append_signatures(signatures);
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
                self.declaration_index = snapshot.declaration_index().clone();
                self.auto_import_records = Self::collect_auto_import_records(
                    &snapshot.module_stages,
                    &self.declaration_index,
                );
                self.scar_session
                    .rollback(snapshot.scar_checkpoint().clone());
                self.sync_scar_fun_index_with_vm();
                self.process_metadata = collect_process_metadata(&snapshot.module_stages);

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
        self.auto_import_records =
            Self::collect_auto_import_records(&module_stages, &declaration_index);

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
        if let Err(e) = self
            .scar_session
            .typecheck_with_context(resolved, Self::std_definition_typecheck_context())
        {
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
        self.process_metadata = collect_process_metadata(&module_stages);
        // `.eldr` sessions read docs/signatures from persisted chunks rather than
        // recollecting them from source during scope-only bootstrap.

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
            .filter(|entry| crate::surface_path_name(&entry.module_path) == module_name)
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
        let Some(entry) = self.declaration_index.get(&fq_name).or_else(|| {
            self.declaration_index.values().find(|entry| {
                crate::surface_path_name(&entry.fq_name) == fq_name
                    || (crate::surface_path_name(&entry.module_path) == module_name
                        && entry.name == name)
            })
        }) else {
            let module_exists = self
                .declaration_index
                .values()
                .any(|entry| crate::surface_path_name(&entry.module_path) == module_name);
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

    pub fn semantic_index(&self) -> surtr_analysis::SemanticIndex {
        surtr_analysis::SemanticIndex::from_symbol_semantic_infos(self.symbol_semantic_infos())
    }

    pub fn symbol_semantic_infos(&self) -> Vec<surtr_analysis::SymbolSemanticInfo> {
        let mut symbols = surtr_analysis::SemanticIndex::from_compile_metadata(
            &self.declaration_index,
            &self.docs,
            &self.signatures,
        )
        .symbols()
        .iter()
        .filter(|symbol| self.compile_symbol_is_repl_completion_surface(symbol))
        .cloned()
        .collect::<Vec<_>>();
        self.enrich_compile_symbol_details(&mut symbols);

        let compile_symbols = symbols.clone();
        let compile_semantic_infos = compile_symbols
            .iter()
            .map(|symbol| self.symbol_semantic_info_from_completion_symbol(symbol))
            .collect::<Vec<_>>();
        let visible_symbols = self
            .sigil_session
            .visible_declaration_entries()
            .into_iter()
            .filter_map(|visible| {
                surtr_analysis::symbol_semantic_info_for_effective_visible_entry(
                    &compile_semantic_infos,
                    &visible,
                )
                .map(surtr_analysis::SymbolSemanticInfo::into_completion_symbol)
            })
            .filter(|symbol| {
                symbol.kind == surtr_analysis::CompletionKind::FunctionCall
                    || self
                        .completion_symbol_declaration(symbol)
                        .is_some_and(|decl| {
                            matches!(
                                decl.kind,
                                sigil::DeclarationKind::EnumVariant
                                    | sigil::DeclarationKind::ResultCtor
                            )
                        })
            })
            .collect::<Vec<_>>();
        symbols.extend(visible_symbols);
        self.enrich_compile_symbol_details(&mut symbols);
        Self::remove_shadowed_type_path_symbols(&mut symbols);

        for label in self.completion_visible_module_labels() {
            if symbols.iter().any(|symbol| {
                symbol.label == label
                    && symbol.kind == surtr_analysis::CompletionKind::TypeConstructor
            }) {
                continue;
            }
            let capabilities = Self::completion_capabilities_for_builtin(&label);
            symbols.push(surtr_analysis::CompletionSymbol {
                label: label.clone(),
                replacement: label,
                kind: surtr_analysis::CompletionKind::TypeConstructor,
                detail: None,
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities,
            });
        }

        let mut seen_bindings = BTreeSet::new();
        for binding in self.binding_records.iter().rev() {
            if !seen_bindings.insert(binding.name.as_str()) {
                continue;
            }
            symbols.push(surtr_analysis::CompletionSymbol {
                label: binding.name.clone(),
                replacement: binding.name.clone(),
                kind: surtr_analysis::CompletionKind::Variable,
                detail: Some(binding.ty.clone()),
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities: None,
            });
        }

        for entry in self.vm.function_entries().iter().rev() {
            if entry.flags.generated {
                continue;
            }
            let Some(qualified_name) = entry.qualified_name.as_deref() else {
                continue;
            };
            if !self.function_entry_is_top_level_repl_surface(qualified_name) {
                continue;
            }
            let label = crate::surface_path_name(qualified_name).to_string();
            symbols.push(surtr_analysis::CompletionSymbol {
                label: label.clone(),
                replacement: label,
                kind: surtr_analysis::CompletionKind::FunctionCall,
                detail: entry.signature.clone().map(|signature| {
                    Self::render_signature_with_qualified_name(qualified_name, signature)
                }),
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities: None,
            });
            if let Some(tail) = crate::surface_path_name(qualified_name).rsplit("::").next() {
                symbols.push(surtr_analysis::CompletionSymbol {
                    label: tail.to_string(),
                    replacement: tail.to_string(),
                    kind: surtr_analysis::CompletionKind::FunctionCall,
                    detail: entry.signature.clone().map(|signature| {
                        Self::render_signature_with_qualified_name(qualified_name, signature)
                    }),
                    documentation: None,
                    sort_text: None,
                    origin: None,
                    definition: None,
                    capabilities: Self::completion_capabilities_for_builtin(qualified_name),
                });
            }
        }

        symbols
            .into_iter()
            .map(|symbol| self.symbol_semantic_info_from_completion_symbol(&symbol))
            .collect()
    }

    fn typecheck_context_for_source(source_kind: SourceKind) -> scar::TypecheckContext {
        scar::TypecheckContext::from_source_policy(source_kind.policy(CompileUnitKind::Repl, None))
    }

    fn std_definition_typecheck_context() -> scar::TypecheckContext {
        let mut context = Self::typecheck_context_for_source(SourceKind::StdDefinitionSource);
        context.enforce_builtin_type_contracts = true;
        context.allow_error_function_params = true;
        context
    }

    fn compile_symbol_is_repl_completion_surface(
        &self,
        symbol: &surtr_analysis::CompletionSymbol,
    ) -> bool {
        if symbol.label.starts_with("impl ") || symbol.label.contains("::impl ") {
            return false;
        }
        if let Some((owner, _)) = symbol.label.split_once("::") {
            if !Self::completion_visible_owner_name(owner) {
                return false;
            }
        } else if symbol.kind == surtr_analysis::CompletionKind::TypeConstructor
            && !Self::completion_visible_owner_name(&symbol.label)
        {
            return false;
        }
        match symbol.kind {
            surtr_analysis::CompletionKind::TypePath => {
                Self::completion_visible_module_name(&symbol.label)
            }
            surtr_analysis::CompletionKind::TypeConstructor => self
                .completion_symbol_declaration(symbol)
                .is_none_or(|decl| Self::declaration_is_repl_completion_surface(decl)),
            _ => self
                .completion_symbol_declaration(symbol)
                .is_none_or(|decl| Self::declaration_is_repl_completion_surface(decl)),
        }
    }

    fn enrich_compile_symbol_details(&self, symbols: &mut [surtr_analysis::CompletionSymbol]) {
        for symbol in symbols {
            if let Some(decl) = self.completion_symbol_declaration(symbol) {
                if matches!(
                    decl.kind,
                    sigil::DeclarationKind::EnumVariant | sigil::DeclarationKind::ResultCtor
                ) {
                    symbol.kind = surtr_analysis::CompletionKind::FunctionCall;
                    if let Some(signature) = Self::special_variant_completion_detail(decl) {
                        symbol.detail = Some(signature);
                    } else if let Some(signature) = symbol.detail.take() {
                        symbol.detail = Some(Self::render_signature_with_qualified_name(
                            &decl.fq_name,
                            signature,
                        ));
                    }
                }
                symbol.capabilities = Self::completion_capabilities_for_declaration(decl);
                if decl.kind == sigil::DeclarationKind::TraitMethod && !symbol.label.contains("::")
                {
                    if let Some((owner, _)) = decl.fq_name.rsplit_once("::") {
                        if let Some(owner_decl) = self.qualified_declaration(owner) {
                            symbol.detail = self
                                .declaration_signature(owner_decl)
                                .or(symbol.detail.take());
                            continue;
                        }
                    }
                }
                if matches!(decl.kind, sigil::DeclarationKind::Trait) {
                    symbol.detail = self.declaration_signature(decl).or(symbol.detail.take());
                    continue;
                }
                if symbol.detail.is_some() {
                    continue;
                }
                symbol.detail = self.declaration_signature(decl);
            }
        }
    }

    fn remove_shadowed_type_path_symbols(symbols: &mut Vec<surtr_analysis::CompletionSymbol>) {
        let type_constructor_labels = symbols
            .iter()
            .filter(|symbol| symbol.kind == surtr_analysis::CompletionKind::TypeConstructor)
            .map(|symbol| symbol.label.clone())
            .collect::<BTreeSet<_>>();
        symbols.retain(|symbol| {
            symbol.kind != surtr_analysis::CompletionKind::TypePath
                || !type_constructor_labels.contains(&symbol.label)
        });
    }

    fn special_variant_completion_detail(entry: &sigil::DeclarationEntry) -> Option<String> {
        match crate::surface_path_name(&entry.fq_name) {
            "Result::Ok" => Some("Result::Ok($T) -> Result<$T, Error>".to_string()),
            "Result::Err" => Some("Result::Err(Error) -> Result<$T, Error>".to_string()),
            "Boolean::True" => Some("Boolean::True() -> Boolean".to_string()),
            "Boolean::False" => Some("Boolean::False() -> Boolean".to_string()),
            _ => None,
        }
    }

    fn completion_symbol_declaration<'a>(
        &'a self,
        symbol: &surtr_analysis::CompletionSymbol,
    ) -> Option<&'a sigil::DeclarationEntry> {
        match symbol.origin.as_ref() {
            Some(surtr_analysis::CompletionOrigin::Declaration { qualified_name, .. }) => {
                self.qualified_declaration(qualified_name)
            }
            _ => self.qualified_declaration(&symbol.label),
        }
    }

    fn symbol_semantic_info_from_completion_symbol(
        &self,
        symbol: &surtr_analysis::CompletionSymbol,
    ) -> surtr_analysis::SymbolSemanticInfo {
        let mut info = surtr_analysis::SymbolSemanticInfo::from_completion_symbol(symbol);
        if info.identity.is_none() {
            info.identity = self
                .completion_symbol_declaration(symbol)
                .and_then(surtr_analysis::symbol_identity_for_declaration_entry)
                .or_else(|| surtr_analysis::symbol_identity_for_builtin_surface(&symbol.label));
        }
        if info.display_metadata.is_none() {
            info.display_metadata = self.symbol_display_metadata_for_completion_symbol(symbol);
        }
        info
    }

    fn symbol_display_metadata_for_completion_symbol(
        &self,
        symbol: &surtr_analysis::CompletionSymbol,
    ) -> Option<surtr_analysis::SymbolDisplayMetadata> {
        let (qualified_name, module_path) = match symbol.origin.as_ref() {
            Some(surtr_analysis::CompletionOrigin::Metadata {
                qualified_name,
                module_path,
            })
            | Some(surtr_analysis::CompletionOrigin::Declaration {
                qualified_name,
                module_path,
                ..
            }) => (qualified_name.as_str(), module_path.as_str()),
            None => self
                .completion_symbol_declaration(symbol)
                .map(|decl| (decl.fq_name.as_str(), decl.module_path.as_str()))?,
        };
        let surface_name = crate::surface_path_name(qualified_name);
        let has_doc = self.docs.iter().any(|entry| {
            entry.qualified_name == qualified_name || entry.qualified_name == surface_name
        });
        let has_signature = self.signatures.iter().any(|entry| {
            entry.qualified_name == qualified_name || entry.qualified_name == surface_name
        }) || self.docs.iter().any(|entry| {
            (entry.qualified_name == qualified_name || entry.qualified_name == surface_name)
                && entry.signature.is_some()
        });
        (has_doc || has_signature).then(|| surtr_analysis::SymbolDisplayMetadata {
            qualified_name: qualified_name.to_string(),
            module_path: module_path.to_string(),
            has_doc,
            has_signature,
        })
    }

    fn build_completion_context(&self) -> ReplCompletionContext {
        let semantic_index = self.semantic_index();
        let mut callable_signatures = BTreeMap::new();

        for symbol in semantic_index.symbols() {
            if !matches!(
                symbol.kind,
                surtr_analysis::CompletionKind::FunctionCall
                    | surtr_analysis::CompletionKind::TypeConstructor
            ) {
                continue;
            }
            let bare_trait_method_decl = (!symbol.label.contains("::"))
                .then(|| self.completion_symbol_declaration(symbol))
                .flatten()
                .filter(|decl| decl.kind == sigil::DeclarationKind::TraitMethod);
            let signature_entry = bare_trait_method_decl
                .and_then(|decl| self.declaration_signature_entry(decl))
                .or_else(|| self.completion_symbol_signature_entry(symbol));
            let Some((qualified_name, signature)) = signature_entry else {
                continue;
            };
            if bare_trait_method_decl.is_some() {
                callable_signatures.insert(symbol.label.clone(), (qualified_name, signature));
            } else {
                Self::insert_completion_context_signature(
                    &mut callable_signatures,
                    &symbol.label,
                    qualified_name,
                    signature,
                );
            }
        }

        for entry in self.vm.function_entries().iter().rev() {
            if entry.flags.generated {
                continue;
            }
            let Some(qualified_name) = entry.qualified_name.as_deref() else {
                continue;
            };
            if !self.function_entry_is_top_level_repl_surface(qualified_name) {
                continue;
            }
            let Some(signature) = entry.signature.as_ref() else {
                continue;
            };
            let label = crate::surface_path_name(qualified_name).to_string();
            Self::insert_completion_context_signature(
                &mut callable_signatures,
                &label,
                qualified_name.to_string(),
                signature.clone(),
            );
        }

        let mut seen_bindings = BTreeSet::new();
        for binding in self.binding_records.iter().rev() {
            if !seen_bindings.insert(binding.name.as_str()) {
                continue;
            }
            if let Some(signature) =
                Self::callable_binding_signature_from_type(&binding.name, &binding.ty)
            {
                Self::insert_completion_context_signature(
                    &mut callable_signatures,
                    &binding.name,
                    binding.name.clone(),
                    signature,
                );
            }
        }

        ReplCompletionContext {
            input_support: surtr_analysis::ReplInputSupportContext::from_parts(
                semantic_index,
                callable_signatures,
            ),
        }
    }

    fn insert_completion_context_signature(
        callable_signatures: &mut BTreeMap<String, (String, String)>,
        label: &str,
        qualified_name: String,
        signature: String,
    ) {
        callable_signatures
            .entry(label.to_string())
            .or_insert((qualified_name.clone(), signature.clone()));
        if let Some(tail) = label.rsplit("::").next() {
            callable_signatures
                .entry(tail.to_string())
                .or_insert((qualified_name, signature));
        }
    }

    fn completion_symbol_qualified_name(
        symbol: &surtr_analysis::CompletionSymbol,
    ) -> Option<String> {
        match symbol.origin.as_ref()? {
            surtr_analysis::CompletionOrigin::Metadata { qualified_name, .. }
            | surtr_analysis::CompletionOrigin::Declaration { qualified_name, .. } => {
                Some(qualified_name.clone())
            }
        }
    }

    fn completion_symbol_signature_entry(
        &self,
        symbol: &surtr_analysis::CompletionSymbol,
    ) -> Option<(String, String)> {
        let qualified_name = Self::completion_symbol_qualified_name(symbol)?;
        if symbol.kind == surtr_analysis::CompletionKind::TypeConstructor {
            if let Some(decl) = self.qualified_declaration(&qualified_name) {
                if let Some(signature) = self.constructor_signature_entry(decl) {
                    return Some(signature);
                }
            }
        }
        symbol
            .detail
            .clone()
            .map(|signature| (qualified_name.clone(), signature))
            .or_else(|| {
                self.qualified_declaration(&qualified_name)
                    .and_then(|decl| self.declaration_signature_entry(decl))
            })
    }

    pub fn completion_context(&self) -> ReplCompletionContext {
        if let Some(cached) = self.completion_context_cache.borrow().clone() {
            return cached;
        }

        let context = self.build_completion_context();
        #[cfg(test)]
        self.completion_context_builds
            .set(self.completion_context_builds.get() + 1);
        *self.completion_context_cache.borrow_mut() = Some(context.clone());
        context
    }

    fn insert_completion_symbol(&mut self, symbol: String) {
        self.symbols.insert(symbol);
    }

    fn insert_surface_symbol(&mut self, name: &str) {
        let surface_name = crate::surface_rendered_name(name);
        self.insert_completion_symbol(surface_name.clone());
        if let Some(short) = surface_name.rsplit("::").next() {
            self.insert_completion_symbol(short.to_string());
        }
    }

    fn empty_completion() -> ReplCompletion {
        ReplCompletion {
            candidates: Vec::new(),
            signature: None,
            telemetry: CompletionTelemetry::default(),
        }
    }

    fn completion_from_candidates(candidates: Vec<ReplCompletionCandidate>) -> ReplCompletion {
        ReplCompletion {
            candidates,
            signature: None,
            telemetry: CompletionTelemetry::default(),
        }
    }

    fn repl_command_arg_context<'a>(
        input: &'a str,
        cursor: usize,
    ) -> Option<ReplCommandArgContext<'a>> {
        let cursor = clamp_to_char_boundary(input, cursor.min(input.len()));
        let before_cursor = &input[..cursor];
        let trimmed_start = before_cursor.trim_start();
        let line_start = before_cursor.len() - trimmed_start.len();
        if !trimmed_start.starts_with(':') {
            return None;
        }

        let command_start = line_start + ':'.len_utf8();
        let command_tail = &input[command_start..];
        let command_len = command_tail
            .char_indices()
            .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx))
            .unwrap_or(command_tail.len());
        let command_end = command_start + command_len;
        if cursor <= command_end {
            return None;
        }

        let command = &input[command_start..command_end];
        let spec = repl_command_spec_for_alias(command)?;
        let mut arg_start = command_end;
        for (idx, ch) in input[command_end..cursor].char_indices() {
            if !ch.is_whitespace() {
                arg_start = command_end + idx;
                break;
            }
            arg_start = command_end + idx + ch.len_utf8();
        }
        Some(ReplCommandArgContext {
            spec,
            arg_start,
            cursor,
            args_before_cursor: &input[arg_start..cursor],
        })
    }

    fn command_arg_word_range(
        input: &str,
        cursor: usize,
        arg_start: usize,
    ) -> (usize, usize, String) {
        let (replace_start, replace_end, prefix) = completion_token(input, cursor);
        if replace_start < arg_start {
            (arg_start, cursor, input[arg_start..cursor].to_string())
        } else {
            (replace_start, replace_end, prefix)
        }
    }

    fn selector_arg_word_range(
        input: &str,
        cursor: usize,
        arg_start: usize,
    ) -> (usize, usize, String) {
        let before = &input[arg_start..cursor];
        let comma_start = before.rfind(',').map(|idx| arg_start + idx + 1);
        let range_start = before.rfind("..").map(|idx| arg_start + idx + 2);
        let mut replace_start = comma_start
            .into_iter()
            .chain(range_start)
            .max()
            .unwrap_or(arg_start);
        while replace_start < cursor {
            let Some(ch) = input[replace_start..cursor].chars().next() else {
                break;
            };
            if !ch.is_whitespace() {
                break;
            }
            replace_start += ch.len_utf8();
        }
        (
            replace_start,
            cursor,
            input[replace_start..cursor].to_string(),
        )
    }

    fn fixed_arg_candidates(
        options: &[&str],
        detail: Option<String>,
        replace_start: usize,
        replace_end: usize,
        prefix: &str,
    ) -> Vec<ReplCompletionCandidate> {
        options
            .iter()
            .filter(|option| option.starts_with(prefix))
            .map(|option| ReplCompletionCandidate {
                label: (*option).to_string(),
                replacement: (*option).to_string(),
                kind: ReplCompletionKind::Variable,
                detail: detail.clone(),
                documentation: None,
                replace_start,
                replace_end,
            })
            .collect()
    }

    fn help_topic_candidates(
        input: &str,
        ctx: &ReplCommandArgContext<'_>,
    ) -> Vec<ReplCompletionCandidate> {
        let (replace_start, replace_end, prefix) =
            Self::command_arg_word_range(input, ctx.cursor, ctx.arg_start);
        let typed_colon = prefix.starts_with(':');
        let topic_prefix = prefix.strip_prefix(':').unwrap_or(&prefix).to_string();
        repl_command_specs()
            .iter()
            .flat_map(|spec| {
                spec.aliases.iter().filter_map({
                    let prefix = prefix.clone();
                    let topic_prefix = topic_prefix.clone();
                    move |alias| {
                        let label = format!(":{alias}");
                        if !alias.starts_with(&topic_prefix) && !label.starts_with(&prefix) {
                            return None;
                        }
                        Some(ReplCompletionCandidate {
                            label,
                            replacement: if typed_colon {
                                format!(":{alias}")
                            } else {
                                (*alias).to_string()
                            },
                            kind: ReplCompletionKind::FunctionCall,
                            detail: Some(spec.completion_detail()),
                            documentation: None,
                            replace_start,
                            replace_end,
                        })
                    }
                })
            })
            .collect()
    }

    fn numeric_line_candidates<I>(
        lines: I,
        replace_start: usize,
        replace_end: usize,
        prefix: &str,
    ) -> Vec<ReplCompletionCandidate>
    where
        I: IntoIterator<Item = usize>,
    {
        lines
            .into_iter()
            .map(|line| line.to_string())
            .filter(|line| line.starts_with(prefix))
            .map(|line| ReplCompletionCandidate {
                label: line.clone(),
                replacement: line,
                kind: ReplCompletionKind::Variable,
                detail: Some("REPL line".to_string()),
                documentation: None,
                replace_start,
                replace_end,
            })
            .collect()
    }

    fn save_path_candidates(ctx: &ReplCommandArgContext<'_>) -> Vec<ReplCompletionCandidate> {
        let (replace_start, replace_end) = (ctx.arg_start, ctx.cursor);
        let typed_path = ctx.args_before_cursor;
        let path = Path::new(typed_path);
        let (dir, prefix) = if typed_path.is_empty() {
            (PathBuf::from("."), "")
        } else if typed_path.ends_with(std::path::MAIN_SEPARATOR) {
            (PathBuf::from(path), "")
        } else {
            (
                path.parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from(".")),
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(""),
            )
        };

        let Ok(entries) = fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut candidates = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let file_name = entry.file_name().to_str()?.to_string();
                if !file_name.starts_with(prefix) {
                    return None;
                }
                let is_dir = entry.file_type().ok().is_some_and(|ty| ty.is_dir());
                let mut replacement = if dir == Path::new(".") {
                    file_name.clone()
                } else {
                    dir.join(&file_name).display().to_string()
                };
                let label = if is_dir {
                    replacement.push(std::path::MAIN_SEPARATOR);
                    format!("{file_name}{}", std::path::MAIN_SEPARATOR)
                } else {
                    file_name
                };
                Some(ReplCompletionCandidate {
                    label,
                    replacement,
                    kind: ReplCompletionKind::Variable,
                    detail: Some(if is_dir {
                        "directory".to_string()
                    } else {
                        ".eldr session path".to_string()
                    }),
                    documentation: None,
                    replace_start,
                    replace_end,
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| left.label.cmp(&right.label));
        candidates
    }

    fn binding_target_candidates(
        &self,
        input: &str,
        ctx: &ReplCommandArgContext<'_>,
        facet_only: bool,
    ) -> Vec<ReplCompletionCandidate> {
        let (replace_start, replace_end, prefix) =
            Self::command_arg_word_range(input, ctx.cursor, ctx.arg_start);
        let mut seen = BTreeSet::new();
        let mut candidates = Vec::new();
        for binding in self
            .result_metas
            .iter()
            .rev()
            .flatten()
            .flat_map(|meta| meta.bindings.iter().rev())
        {
            if facet_only && binding.facet_info.is_none() {
                continue;
            }
            if !seen.insert(binding.name.as_str()) || !binding.name.starts_with(&prefix) {
                continue;
            }
            candidates.push(ReplCompletionCandidate {
                label: binding.name.clone(),
                replacement: binding.name.clone(),
                kind: if binding.facet_info.is_some() {
                    ReplCompletionKind::TypePath
                } else {
                    ReplCompletionKind::Variable
                },
                detail: Some(crate::surface_rendered_name(&binding.ty)),
                documentation: None,
                replace_start,
                replace_end,
            });
        }
        if !facet_only {
            for (name, metadata) in &self.process_metadata {
                if metadata.instance != spire::ast::ProcessInstance::Singleton {
                    continue;
                }
                let rendered = name.rsplit("::").next().unwrap_or(name);
                if !seen.insert(rendered) || !rendered.starts_with(&prefix) {
                    continue;
                }
                candidates.push(ReplCompletionCandidate {
                    label: rendered.to_string(),
                    replacement: rendered.to_string(),
                    kind: ReplCompletionKind::Variable,
                    detail: Some("singleton process owner".to_string()),
                    documentation: None,
                    replace_start,
                    replace_end,
                });
            }
        }
        candidates
    }

    fn command_argument_completion(
        &self,
        input: &str,
        cursor: usize,
        use_site: surtr_analysis::ReplCompletionUseSite,
    ) -> Option<ReplCompletion> {
        if !matches!(use_site, surtr_analysis::ReplCompletionUseSite::Command(_)) {
            return None;
        }
        if !completion_allowed_at_cursor(input, cursor) {
            return Some(Self::empty_completion());
        }
        let ctx = Self::repl_command_arg_context(input, cursor)?;
        match ctx.spec.arg_completion {
            ReplCommandArgCompletion::Semantic => None,
            ReplCommandArgCompletion::None => Some(Self::empty_completion()),
            ReplCommandArgCompletion::CommandTopic => Some(Self::completion_from_candidates(
                Self::help_topic_candidates(input, &ctx),
            )),
            ReplCommandArgCompletion::Fixed(options) => {
                let (replace_start, replace_end, prefix) =
                    Self::command_arg_word_range(input, cursor, ctx.arg_start);
                Some(Self::completion_from_candidates(
                    Self::fixed_arg_candidates(
                        options,
                        Some(ctx.spec.usage.to_string()),
                        replace_start,
                        replace_end,
                        &prefix,
                    ),
                ))
            }
            ReplCommandArgCompletion::ResultLine => {
                let (replace_start, replace_end, prefix) =
                    Self::selector_arg_word_range(input, cursor, ctx.arg_start);
                Some(Self::completion_from_candidates(
                    Self::numeric_line_candidates(
                        1..=self.results.len(),
                        replace_start,
                        replace_end,
                        &prefix,
                    ),
                ))
            }
            ReplCommandArgCompletion::HistorySelector => {
                let (replace_start, replace_end, prefix) =
                    Self::selector_arg_word_range(input, cursor, ctx.arg_start);
                Some(Self::completion_from_candidates(
                    Self::numeric_line_candidates(
                        self.history_entries.iter().map(|entry| entry.line),
                        replace_start,
                        replace_end,
                        &prefix,
                    ),
                ))
            }
            ReplCommandArgCompletion::SavePath => Some(Self::completion_from_candidates(
                Self::save_path_candidates(&ctx),
            )),
            ReplCommandArgCompletion::TypeTarget => Some(Self::completion_from_candidates(
                self.binding_target_candidates(input, &ctx, false),
            )),
            ReplCommandArgCompletion::FacetTarget => {
                if let Some(assist) = self.facet_completion_assist(input, cursor) {
                    return Some(ReplCompletion {
                        candidates: assist.candidates,
                        signature: Some(assist.signature),
                        telemetry: CompletionTelemetry::default(),
                    });
                }
                Some(Self::completion_from_candidates(
                    self.binding_target_candidates(input, &ctx, true),
                ))
            }
        }
    }

    pub fn completions(&self, input: &str, cursor: usize) -> ReplCompletion {
        let use_site = surtr_analysis::repl_completion_use_site(input, cursor);
        if let Some(completion) = self.command_argument_completion(input, cursor, use_site) {
            return completion;
        }
        if matches!(
            use_site,
            surtr_analysis::ReplCompletionUseSite::Input
                | surtr_analysis::ReplCompletionUseSite::Command(
                    surtr_analysis::ReplCommandUseSite::Facet
                )
        ) {
            if let Some(assist) = self.facet_completion_assist(input, cursor) {
                return ReplCompletion {
                    candidates: assist.candidates,
                    signature: Some(assist.signature),
                    telemetry: CompletionTelemetry::default(),
                };
            }
        }
        self.completion_context().completions(input, cursor)
    }

    pub fn cached_completion_context(&self) -> Option<ReplCompletionContext> {
        self.completion_context_cache.borrow().clone()
    }

    #[cfg(test)]
    fn completion_context_build_count(&self) -> usize {
        self.completion_context_builds.get()
    }

    fn repl_completion_candidate_from_analysis(
        candidate: surtr_analysis::CompletionCandidate,
    ) -> ReplCompletionCandidate {
        let label = Self::repl_completion_label(
            &candidate.label,
            &candidate.replacement,
            candidate.detail.as_deref(),
        );
        let kind = match candidate.kind {
            surtr_analysis::CompletionKind::Variable => ReplCompletionKind::Variable,
            surtr_analysis::CompletionKind::TypeConstructor => ReplCompletionKind::TypeConstructor,
            surtr_analysis::CompletionKind::TypePath => ReplCompletionKind::TypePath,
            surtr_analysis::CompletionKind::FunctionCall => ReplCompletionKind::FunctionCall,
        };

        ReplCompletionCandidate {
            label,
            replacement: candidate.replacement,
            kind,
            detail: candidate.detail,
            documentation: candidate.documentation,
            replace_start: candidate.replace_start,
            replace_end: candidate.replace_end,
        }
    }

    fn facet_completion_assist(&self, input: &str, cursor: usize) -> Option<FacetCompletionAssist> {
        let cursor = clamp_to_char_boundary(input, cursor.min(input.len()));
        if !completion_allowed_at_cursor(input, cursor) {
            return None;
        }
        // `:facet` is definition inspection. It can traverse private segments so
        // that the REPL can describe a path which is valid in its owner scope,
        // while ordinary source completion keeps those members hidden.
        let command_context = Self::repl_command_arg_context(input, cursor);
        let include_private = command_context
            .as_ref()
            .is_some_and(|context| context.spec.kind == ReplCommandKind::Facet);
        let (facet_input, facet_cursor, offset) = if let Some(context) = &command_context {
            if include_private {
                (
                    &input[context.arg_start..],
                    cursor.saturating_sub(context.arg_start),
                    context.arg_start,
                )
            } else {
                (input, cursor, 0)
            }
        } else {
            (input, cursor, 0)
        };
        let mut context = surtr_analysis::facet_path_context_at_cursor(facet_input, facet_cursor)?;
        context.replace_start += offset;
        context.replace_end += offset;
        context.token_start += offset;
        let (root_ty, mut focus_ty, source_is_result) = self.facet_root_ast_ty(&context)?;
        let mut path_is_variant = false;
        let mut path_is_fallible = false;
        for segment in &context.completed_segments {
            if let Some(inner) = Self::result_inner_ast_ty(&focus_ty) {
                focus_ty = inner.clone();
                path_is_fallible = true;
            }
            let step = self.facet_next_ast_ty(&focus_ty, segment, include_private)?;
            path_is_variant |= step.variant;
            path_is_fallible |= step.fallible;
            focus_ty = step.ty;
        }
        let candidate_focus_ty = Self::result_inner_ast_ty(&focus_ty).unwrap_or(&focus_ty);
        let placeholder = if matches!(candidate_focus_ty, AstTy::Tuple(_, _)) {
            "[segment]"
        } else {
            "[field]"
        };
        let candidates = self
            .facet_candidate_segments(
                candidate_focus_ty,
                context.replace_start,
                context.replace_end,
                include_private,
            )
            .unwrap_or_default()
            .into_iter()
            .filter(|candidate| candidate.label.starts_with(&context.prefix))
            .collect::<Vec<_>>();
        let path_display = if context.completed_segments.is_empty() {
            format!(
                "{}{}",
                &input[context.token_start..context.replace_start],
                placeholder
            )
        } else {
            context.current_path.clone()
        };
        let signature = ReplSignatureHelp {
            lines: self.facet_api_help_lines(
                &path_display,
                &root_ty,
                &focus_ty,
                source_is_result || path_is_fallible,
                path_is_variant,
                context.completed_segments.is_empty(),
                context.root_kind,
                context.root_kind == surtr_analysis::FacetPathRootKind::ViewClosureRoot,
            ),
            active_parameter: Some(0),
        };
        Some(FacetCompletionAssist {
            candidates,
            signature,
        })
    }

    fn facet_root_ast_ty(
        &self,
        context: &surtr_analysis::FacetPathCompletionContext,
    ) -> Option<(AstTy, AstTy, bool)> {
        match context.root_kind {
            surtr_analysis::FacetPathRootKind::TypeRoot
            | surtr_analysis::FacetPathRootKind::ViewClosureRoot => Some((
                AstTy::Named(Span { start: 0, end: 0 }, context.root.clone()),
                AstTy::Named(Span { start: 0, end: 0 }, context.root.clone()),
                false,
            )),
            surtr_analysis::FacetPathRootKind::ValueRoot => self
                .binding_type(&context.root)
                .as_deref()
                .and_then(parse_signature_type)
                .or_else(|| {
                    self.semantic_index()
                        .find_symbol(&context.root)
                        .and_then(|symbol| symbol.detail.as_deref())
                        .and_then(parse_signature_type)
                })
                .map(|root_ty| {
                    if let Some((source, focus)) = Self::facet_source_and_focus_ast_ty(&root_ty) {
                        return (source, focus, false);
                    }
                    if let Some(inner) = Self::result_inner_ast_ty(&root_ty) {
                        (root_ty.clone(), inner.clone(), true)
                    } else {
                        (root_ty.clone(), root_ty, false)
                    }
                }),
        }
    }

    fn facet_candidate_segments(
        &self,
        ty: &AstTy,
        replace_start: usize,
        replace_end: usize,
        include_private: bool,
    ) -> Option<Vec<ReplCompletionCandidate>> {
        match ty {
            AstTy::Named(_, name) => {
                if name.rsplit("::").next().unwrap_or(name) == "Tuple" {
                    return Some(vec![ReplCompletionCandidate {
                        label: "_".to_string(),
                        replacement: "_".to_string(),
                        kind: ReplCompletionKind::TypePath,
                        detail: Some("pass _N: N requires UnsignedInt".to_string()),
                        documentation: None,
                        replace_start,
                        replace_end,
                    }]);
                }
                if name.rsplit("::").next().unwrap_or(name) == "Result" {
                    return Some(Vec::new());
                }
                if let Some(def) = self.scar_session.lookup_type_def(name) {
                    let mut candidates = def
                        .fields
                        .iter()
                        .filter(|(field, _)| include_private || !def.private_fields.contains(field))
                        .map(|(field, field_ty)| ReplCompletionCandidate {
                            label: field.clone(),
                            replacement: field.clone(),
                            kind: ReplCompletionKind::TypePath,
                            detail: Some(Self::field_completion_detail(
                                field,
                                field_ty,
                                def.private_fields.contains(field),
                                def.readonly_fields.contains(field),
                            )),
                            documentation: None,
                            replace_start,
                            replace_end,
                        })
                        .collect::<Vec<_>>();
                    if matches!(def.kind, scar::env::TypeKind::Enum) {
                        candidates.extend(
                            self.scar_session
                                .enum_variants_of(name)
                                .into_iter()
                                .flatten()
                                .map(|variant| ReplCompletionCandidate {
                                    label: variant.short_name.clone(),
                                    replacement: variant.short_name.clone(),
                                    kind: ReplCompletionKind::TypePath,
                                    detail: Some(if variant.payload.is_empty() {
                                        format!(
                                            "{}::{}",
                                            crate::surface_path_name(name),
                                            variant.short_name
                                        )
                                    } else {
                                        format!(
                                            "{}::{}({})",
                                            crate::surface_path_name(name),
                                            variant.short_name,
                                            variant
                                                .payload
                                                .iter()
                                                .map(Self::ty_to_string)
                                                .collect::<Vec<_>>()
                                                .join(", ")
                                        )
                                    }),
                                    documentation: None,
                                    replace_start,
                                    replace_end,
                                }),
                        );
                    }
                    candidates.sort_by(|left, right| left.label.cmp(&right.label));
                    return Some(candidates);
                }
                None
            }
            AstTy::Tuple(_, items) => Some(
                items
                    .iter()
                    .enumerate()
                    .map(|(idx, item)| ReplCompletionCandidate {
                        label: format!("_{idx}"),
                        replacement: format!("_{idx}"),
                        kind: ReplCompletionKind::TypePath,
                        detail: Some(format!("_{idx}: {}", format_query_ty(item))),
                        documentation: None,
                        replace_start,
                        replace_end,
                    })
                    .collect(),
            ),
            AstTy::Generic(_, name, args) if name == "List" && args.len() == 1 => {
                let item = Self::facet_help_ty_display(&args[0]);
                Some(vec![
                    ReplCompletionCandidate {
                        label: "[idx]".to_string(),
                        replacement: "[idx]".to_string(),
                        kind: ReplCompletionKind::TypePath,
                        detail: Some(format!("[idx: Int] -> {item}")),
                        documentation: None,
                        replace_start,
                        replace_end,
                    },
                    ReplCompletionCandidate {
                        label: "[idx + 1]".to_string(),
                        replacement: "[idx + 1]".to_string(),
                        kind: ReplCompletionKind::TypePath,
                        detail: Some(format!("[idx + 1: Int] -> {item}")),
                        documentation: None,
                        replace_start,
                        replace_end,
                    },
                    ReplCompletionCandidate {
                        label: "[start..end]".to_string(),
                        replacement: "[start..end]".to_string(),
                        kind: ReplCompletionKind::TypePath,
                        detail: Some(format!("[start..end: Int] -> List<{item}>")),
                        documentation: None,
                        replace_start,
                        replace_end,
                    },
                ])
            }
            AstTy::Generic(_, name, args) if name == "HashMap" && args.len() == 1 => {
                let value = Self::facet_help_ty_display(&args[0]);
                Some(vec![
                    ReplCompletionCandidate {
                        label: "[key]".to_string(),
                        replacement: "[key]".to_string(),
                        kind: ReplCompletionKind::TypePath,
                        detail: Some(format!("[key: String] -> {value}")),
                        documentation: None,
                        replace_start,
                        replace_end,
                    },
                    ReplCompletionCandidate {
                        label: "[\"key\"]".to_string(),
                        replacement: "[\"key\"]".to_string(),
                        kind: ReplCompletionKind::TypePath,
                        detail: Some(format!("[\"key\": String] -> {value}")),
                        documentation: None,
                        replace_start,
                        replace_end,
                    },
                ])
            }
            _ => None,
        }
    }

    fn field_completion_detail(
        field: &str,
        field_ty: &Ty,
        private: bool,
        readonly: bool,
    ) -> String {
        let mut detail = format!("{}: {}", field, Self::ty_to_string(field_ty));
        if private {
            detail.push_str(" private");
        }
        if readonly {
            detail.push_str(" readonly");
        }
        detail
    }

    fn facet_next_ast_ty(
        &self,
        ty: &AstTy,
        segment: &str,
        include_private: bool,
    ) -> Option<FacetStep> {
        match ty {
            AstTy::Named(_, name) => {
                let def = self.scar_session.lookup_type_def(name)?;
                if let Some((_, field_ty)) = def.fields.iter().find(|(field, _)| {
                    field == segment && (include_private || !def.private_fields.contains(field))
                }) {
                    return parse_signature_type(&Self::ty_to_string(field_ty)).map(|ty| {
                        FacetStep {
                            ty,
                            fallible: false,
                            variant: false,
                        }
                    });
                }
                if matches!(def.kind, scar::env::TypeKind::Enum) {
                    let variant = self
                        .scar_session
                        .enum_variants_of(name)?
                        .iter()
                        .find(|variant| variant.short_name == segment.trim_end_matches('?'))?;
                    return match variant.payload.as_slice() {
                        [] => Some(FacetStep {
                            ty: AstTy::Named(
                                Span { start: 0, end: 0 },
                                crate::surface_path_name(name).to_string(),
                            ),
                            fallible: true,
                            variant: true,
                        }),
                        [single] => {
                            parse_signature_type(&Self::ty_to_string(single)).map(|ty| FacetStep {
                                ty,
                                fallible: true,
                                variant: true,
                            })
                        }
                        many => Some(FacetStep {
                            ty: AstTy::Tuple(
                                Span { start: 0, end: 0 },
                                many.iter()
                                    .map(|item| parse_signature_type(&Self::ty_to_string(item)))
                                    .collect::<Option<Vec<_>>>()?,
                            ),
                            fallible: true,
                            variant: true,
                        }),
                    };
                }
                None
            }
            AstTy::Tuple(_, items) => segment
                .strip_prefix('_')
                .and_then(|value| value.parse::<usize>().ok())
                .and_then(|idx| items.get(idx).cloned())
                .map(|ty| FacetStep {
                    ty,
                    fallible: false,
                    variant: false,
                }),
            AstTy::Generic(_, name, args) if segment.starts_with('[') && segment.ends_with(']') => {
                match name.as_str() {
                    "List" if args.len() == 1 => Some(FacetStep {
                        ty: if segment.contains("..") {
                            AstTy::Generic(
                                Span { start: 0, end: 0 },
                                "List".to_string(),
                                vec![args[0].clone()],
                            )
                        } else {
                            args[0].clone()
                        },
                        fallible: true,
                        variant: false,
                    }),
                    "HashMap" if args.len() == 1 => Some(FacetStep {
                        ty: args[0].clone(),
                        fallible: true,
                        variant: false,
                    }),
                    _ => None,
                }
            }
            AstTy::Generic(_, name, args) if name != "Result" => {
                match (name.as_str(), segment.trim_end_matches('?')) {
                    ("Option", "Some") if args.len() == 1 => Some(FacetStep {
                        ty: args[0].clone(),
                        fallible: true,
                        variant: true,
                    }),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn result_inner_ast_ty(ty: &AstTy) -> Option<&AstTy> {
        match ty {
            AstTy::Generic(_, name, args) if name == "Result" && !args.is_empty() => args.first(),
            _ => None,
        }
    }

    fn facet_source_and_focus_ast_ty(ty: &AstTy) -> Option<(AstTy, AstTy)> {
        match ty {
            AstTy::Generic(_, name, args) if name == "Facet" && args.len() == 5 => {
                Some((args[1].clone(), args[2].clone()))
            }
            _ => None,
        }
    }

    fn facet_api_help_lines(
        &self,
        path_display: &str,
        source_ty: &AstTy,
        focus_ty: &AstTy,
        path_is_fallible: bool,
        path_is_variant: bool,
        root_only: bool,
        root_kind: surtr_analysis::FacetPathRootKind,
        is_view_closure: bool,
    ) -> Vec<String> {
        let source = Self::facet_help_ty_display(source_ty);
        let focus = Self::facet_help_ty_display(focus_ty);
        let view_result = if path_is_fallible {
            format!("Result<{focus}>")
        } else {
            focus.clone()
        };
        let mut lines = if root_only && is_view_closure {
            vec![format!("{path_display} -> ({source} -> _)")]
        } else if root_only && root_kind == surtr_analysis::FacetPathRootKind::TypeRoot {
            vec![format!(
                "{path_display} -> Facet<InfallibleStructural, {source}, _, _, _>"
            )]
        } else if root_only && root_kind == surtr_analysis::FacetPathRootKind::ValueRoot {
            vec![format!("{path_display} -> _")]
        } else if is_view_closure {
            vec![format!("&{path_display} -> ({source} -> {view_result})")]
        } else {
            vec![format!(
                "Facet::view({path_display}, {source}) -> {view_result}"
            )]
        };
        lines.push(format!(
            "Facet::set({path_display}, {source}, {}) -> Result<{source}>",
            self.facet_set_value_display(focus_ty)
        ));
        let over_input = self.facet_over_input_display(focus_ty);
        lines.push(format!(
            "Facet::over({path_display}, {source}, ({over_input} -> Result<{over_input}>)) -> Result<{source}>"
        ));
        if Self::result_inner_ast_ty(focus_ty).is_some() {
            lines.push(format!(
                "Facet::over_result({path_display}, {source}, ({focus} -> Result<{focus}>)) -> Result<{source}>"
            ));
        }
        if path_is_variant {
            lines.push(format!(
                "Facet::preview({path_display}, {source}) -> {view_result}"
            ));
            lines.push(format!(
                "Facet::case_set({path_display}, {source}, {}) -> Result<{source}>",
                self.facet_set_value_display(focus_ty)
            ));
            lines.push(format!(
                "Facet::case_over({path_display}, {source}, ({over_input} -> Result<{over_input}>)) -> Result<{source}>"
            ));
        }
        lines
    }

    fn facet_set_value_display(&self, focus_ty: &AstTy) -> String {
        if let Some(inner) = Self::result_inner_ast_ty(focus_ty) {
            format!(
                "{} or {}",
                Self::facet_help_ty_display(inner),
                Self::facet_help_ty_display(focus_ty)
            )
        } else {
            Self::facet_help_ty_display(focus_ty)
        }
    }

    fn facet_over_input_display(&self, focus_ty: &AstTy) -> String {
        Self::result_inner_ast_ty(focus_ty)
            .map(Self::facet_help_ty_display)
            .unwrap_or_else(|| Self::facet_help_ty_display(focus_ty))
    }

    fn facet_help_ty_display(ty: &AstTy) -> String {
        match ty {
            AstTy::Generic(_, name, args)
                if name == "Result"
                    && args.len() == 2
                    && matches!(
                        args.get(1),
                        Some(AstTy::Named(_, error)) if error == "Error"
                    ) =>
            {
                format!("Result<{}>", Self::facet_help_ty_display(&args[0]))
            }
            AstTy::Generic(_, name, args) => format!(
                "{}<{}>",
                name,
                args.iter()
                    .map(Self::facet_help_ty_display)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Tuple(_, items) => format!(
                "({})",
                items
                    .iter()
                    .map(Self::facet_help_ty_display)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Func(_, params, ret) => format!(
                "({} -> {})",
                params
                    .iter()
                    .map(Self::facet_help_ty_display)
                    .collect::<Vec<_>>()
                    .join(", "),
                Self::facet_help_ty_display(ret)
            ),
            _ => format_query_ty(ty),
        }
    }

    fn repl_completion_label(label: &str, replacement: &str, detail: Option<&str>) -> String {
        if label != replacement {
            return label.to_string();
        }

        if !matches!(replacement, "True" | "False") {
            return label.to_string();
        }

        if detail.is_some_and(|detail| detail.contains(&format!("Result::{replacement}("))) {
            label.to_string()
        } else {
            label.to_string()
        }
    }

    fn callable_binding_signature_from_type(binding_name: &str, ty: &str) -> Option<String> {
        let AstTy::Func(_, params, ret) = parse_signature_type(ty)? else {
            return None;
        };
        let params = params
            .iter()
            .map(format_query_ty)
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "{binding_name}({params}) -> {}",
            format_query_ty(ret.as_ref())
        ))
    }

    fn sync_cached_completion_context_after_commit(
        &self,
        imported_symbols: &[String],
        bindings: &[forge::BindingInfo],
        function_defs: &[String],
    ) {
        if self.completion_context_cache.borrow().is_none() {
            return;
        }

        let _ = (imported_symbols, bindings, function_defs);
        *self.completion_context_cache.borrow_mut() = Some(self.build_completion_context());
    }

    fn completion_capabilities_for_builtin(name: &str) -> Option<SymbolCapabilities> {
        let surface_name = crate::surface_rendered_name(name);
        surtr_analysis::symbol_capabilities_for_builtin_surface(&surface_name)
    }

    fn completion_capabilities_for_declaration(
        entry: &sigil::DeclarationEntry,
    ) -> Option<SymbolCapabilities> {
        surtr_analysis::symbol_capabilities_for_declaration_entry(entry)
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
        error_display::runtime_value_error_lines_with_registry_and_stack_trace(
            self.vm.as_vm(),
            value,
            &self.sources,
            self.repl_source_id,
            self.error_display_mode,
            self.stack_trace_display_mode,
        )
    }

    fn bump_line(&mut self, value: Option<Value>, meta: Option<forge::ChunkMeta>) {
        self.results.push(value);
        self.result_metas.push(meta);
        self.next_line += 1;
    }

    fn help_lines() -> Vec<String> {
        repl_command_help_lines()
    }

    fn doc_help_lines() -> Vec<String> {
        repl_command_topic_help_lines("doc").expect(":doc should have topic help")
    }

    fn sig_help_lines() -> Vec<String> {
        repl_command_topic_help_lines("sig").expect(":sig should have topic help")
    }

    fn info_help_lines() -> Vec<String> {
        repl_command_topic_help_lines("info").expect(":info should have topic help")
    }

    fn type_help_lines() -> Vec<String> {
        repl_command_topic_help_lines("type").expect(":type should have topic help")
    }

    fn facet_help_lines() -> Vec<String> {
        repl_command_topic_help_lines("facet").expect(":facet should have topic help")
    }

    fn handle_help(&self, topic: Option<&str>) -> Vec<String> {
        let Some(topic) = topic.map(str::trim).filter(|topic| !topic.is_empty()) else {
            return Self::help_lines();
        };
        match repl_command_topic_help_lines(topic) {
            Some(lines) => lines,
            None => {
                let other = topic.strip_prefix(':').unwrap_or(topic);
                let mut rendered = vec![format!("No help found for :{}", other)];
                rendered.push("Type :help for available REPL commands.".to_string());
                rendered
            }
        }
    }

    fn plain(lines: Vec<String>) -> ReplResult {
        ReplResult::plain(lines)
    }

    fn styled(lines: Vec<String>) -> ReplResult {
        ReplResult::styled(lines)
    }

    fn take_repl_host_io_lines(&mut self) -> (Vec<String>, Vec<String>) {
        (
            self.vm.take_repl_host_stdout(),
            self.vm.take_repl_host_stderr(),
        )
    }

    fn repl_command_diagnostic(
        &self,
        source: &str,
        message: impl Into<String>,
        span: Span,
        help: Option<String>,
        summary_tail: Vec<String>,
    ) -> ReplResult {
        let mut spec = diagnostics::repl_command_parse_error_spec(source, message, span);
        if let Some(help) = help {
            spec.help = Some(help);
        }
        let rendered =
            error_display::diagnostic_lines("REPL", source, &spec, self.error_display_mode);
        ReplResult::diagnostic(rendered, summary_tail)
    }

    fn repl_query_diagnostic(
        &self,
        source: &str,
        message: impl Into<String>,
        span: Span,
        help: Option<String>,
    ) -> ReplResult {
        let mut spec = diagnostics::repl_query_parse_error_spec(source, message, span);
        if let Some(help) = help {
            spec.help = Some(help);
        }
        let rendered =
            error_display::diagnostic_lines("REPL", source, &spec, self.error_display_mode);
        ReplResult::diagnostic(rendered, Vec::new())
    }

    fn handle_doc(&self, symbol: &str) -> ReplResult {
        let trimmed = symbol.trim();
        if trimmed.is_empty() {
            return Self::plain(Self::doc_help_lines());
        }
        match parse_repl_query(trimmed) {
            Ok(ReplQuery::Symbol(symbol)) => self.handle_doc_symbol(trimmed, &symbol),
            Ok(ReplQuery::FacetRootDoc(symbol)) => self.handle_facet_root_doc(&symbol),
            Ok(ReplQuery::FieldPath(symbol)) => Self::plain(vec![format!(
                "`{}` is not a documentation target. Fields do not have standalone declaration documentation. Use `:info {}` or `:facet {}`.",
                symbol.source,
                symbol.source.split('.').next().unwrap_or(&symbol.source),
                symbol.source
            )]),
            Ok(ReplQuery::TypedCall(query)) => self.handle_doc_typed_call(trimmed, &query),
            Ok(ReplQuery::OperatorTarget(query)) => self.handle_doc_typed_operator(trimmed, &query),
            Err(err) => self.repl_query_diagnostic(
                &format!(":doc {trimmed}"),
                err.message().to_string(),
                err.span(),
                Some("Accepted forms: symbol, typed call, or operator target.".to_string()),
            ),
        }
    }

    fn handle_facet_root_doc(&self, ty: &str) -> ReplResult {
        let kind = self
            .visible_declaration(ty)
            .and_then(|decl| sigil::declaration_symbol_identity_info(&decl.fq_name, &decl.kind))
            .and_then(|info| info.capabilities.facet_root_path)
            .or_else(|| {
                sindr::names::builtin_symbol_identity_info(ty)
                    .and_then(|info| info.capabilities.facet_root_path)
            });
        let Some(kind) = kind else {
            return Self::plain(vec![format!("{ty} is not a Facet path root.")]);
        };
        let root_form = match kind {
            sindr::names::FacetRootKind::Tuple => "Tuple._N",
            sindr::names::FacetRootKind::List => "List.[index]",
            sindr::names::FacetRootKind::HashMap => "HashMap.[key]",
            sindr::names::FacetRootKind::TypeRoot => "Ty.segment",
        };
        ReplResult::ok(ReplOutput::DocResolved {
            symbol: format!("Facet.{ty}"),
            signature: None,
            summary: Some(format!("Facet root for {ty}.")),
            source_snippet: Some(format!(
                "`{ty}` is a Facet path root. Use `{root_form}` to build a path.\nUse `:info {ty}` for field policies and `:facet {ty}.segment` for path inspection."
            )),
            details: Vec::new(),
        })
    }

    fn handle_doc_symbol(&self, source_symbol: &str, symbol: &str) -> ReplResult {
        if symbol == "Tuple" {
            return ReplResult::ok(Self::tuple_doc_output());
        }
        if symbol == "Closure" {
            if let Some(entry) = self.closure_doc_entry() {
                return ReplResult::ok(Self::doc_resolved_output(entry));
            }
        }
        if let Some(entry) = self.special_form_doc_entry(symbol) {
            return ReplResult::ok(Self::doc_resolved_output(entry));
        }
        if !Self::is_qualified_symbol(source_symbol) {
            if let Some(binding) = self.binding_info(source_symbol) {
                if let Some(result) = self.handle_doc_binding(source_symbol) {
                    return result;
                }
                if let Some(type_symbol) = Self::binding_doc_type_symbol(binding) {
                    return self.handle_doc_symbol(&type_symbol, &type_symbol);
                }
            }
        }
        let canonical = self
            .visible_helper_doc_alias(symbol)
            .unwrap_or_else(|| Self::canonical_symbol(symbol).to_string());
        let preferred_kind = Self::definition_doc_kind(&canonical);
        let matches = if let Some(matches) = self.type_owner_doc_entries(&canonical) {
            matches
        } else {
            let visible = self.visible_doc_entries(&canonical, preferred_kind.clone());
            if visible.is_empty() {
                if canonical != symbol || preferred_kind.is_some() {
                    self.script_preload_doc_entries(&canonical, preferred_kind)
                } else if let Some(decl) = self.visible_declaration(symbol) {
                    let expected_signature = self.declaration_signature(decl);
                    let mut matches = self
                        .docs
                        .iter()
                        .filter(|entry| {
                            crate::surface_path_name(&entry.qualified_name)
                                == crate::surface_path_name(&decl.fq_name)
                        })
                        .filter(|entry| {
                            expected_signature.as_ref().is_none_or(|signature| {
                                entry
                                    .signature
                                    .as_ref()
                                    .is_some_and(|entry_sig| entry_sig == signature)
                            })
                        })
                        .collect::<Vec<_>>();
                    matches.dedup_by(|a, b| {
                        a.qualified_name == b.qualified_name
                            && a.kind == b.kind
                            && a.signature == b.signature
                            && a.doc == b.doc
                    });
                    matches
                } else {
                    self.script_preload_doc_entries(symbol, None)
                }
            } else {
                visible
            }
        };

        match matches.as_slice() {
            [] => self
                .private_declaration(&canonical)
                .map(Self::private_doc_output)
                .or_else(|| {
                    self.private_declaration(symbol)
                        .map(Self::private_doc_output)
                })
                .or_else(|| self.concrete_process_alias_doc_output(&canonical))
                .or_else(|| self.undocumented_doc_output(&canonical))
                .map(|output| ReplResult::ok(output))
                .unwrap_or_else(|| {
                    if let Some(binding) = self.binding_info(source_symbol) {
                        if let Some(result) = self.handle_doc_binding(source_symbol) {
                            result
                        } else if let Some(type_symbol) = Self::binding_doc_type_symbol(binding) {
                            self.handle_doc_symbol(&type_symbol, &type_symbol)
                        } else {
                            Self::plain(vec![format!("No docs found for {}", source_symbol)])
                        }
                    } else {
                        Self::plain(vec![format!("No docs found for {}", source_symbol)])
                    }
                }),
            [entry] => ReplResult::ok(Self::doc_resolved_output(entry)),
            entries => Self::plain(Self::ambiguous_doc_lines(source_symbol, entries)),
        }
    }

    fn type_owner_doc_entries<'a>(&'a self, symbol: &str) -> Option<Vec<&'a DocEntry>> {
        let decl = self.visible_declaration(symbol)?;
        if !matches!(
            decl.kind,
            sigil::DeclarationKind::Struct
                | sigil::DeclarationKind::Record
                | sigil::DeclarationKind::Deferror
                | sigil::DeclarationKind::Enum
                | sigil::DeclarationKind::BuiltinType
                | sigil::DeclarationKind::Trait
        ) {
            return None;
        }

        let mut matches = self
            .docs
            .iter()
            .filter(|entry| {
                entry.kind == DocKind::Type
                    && crate::surface_path_name(&entry.qualified_name)
                        == crate::surface_path_name(&decl.fq_name)
            })
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        matches.dedup_by(|a, b| {
            a.qualified_name == b.qualified_name
                && a.kind == b.kind
                && a.signature == b.signature
                && a.doc == b.doc
        });
        Some(matches)
    }

    fn handle_doc_typed_operator(
        &self,
        source_query: &str,
        query: &OperatorTargetQuery,
    ) -> ReplResult {
        if let Some(symbol) = Self::operator_target_symbol(query) {
            return self
                .operator_target_doc_entry(query)
                .map(|entry| {
                    let signature = self
                        .operator_target_signature_entry(query)
                        .map(|(_, signature)| signature);
                    ReplResult::ok(Self::doc_output_with_symbol_and_signature(
                        entry,
                        symbol,
                        signature,
                        Vec::new(),
                    ))
                })
                .unwrap_or_else(|| Self::plain(vec![format!("No docs found for {source_query}")]));
        }
        let synthetic = TypedCallQuery {
            callee: Self::canonical_symbol(query.operator).to_string(),
            args: vec![query.target.clone(), query.target.clone()],
        };
        self.handle_doc_typed_call(source_query, &synthetic)
    }

    fn canonical_symbol(symbol: &str) -> &str {
        OPERATOR_DOC_TARGETS
            .iter()
            .find_map(|(alias, target)| (*alias == symbol).then_some(*target))
            .unwrap_or(symbol)
    }

    fn operator_target_symbol(query: &OperatorTargetQuery) -> Option<String> {
        let owner = Self::operator_target_owner_name(&query.target)?;
        let member = Self::operator_target_member_name(query.operator)?;
        Some(format!("{owner}::{member}"))
    }

    fn operator_target_signature_entry(
        &self,
        query: &OperatorTargetQuery,
    ) -> Option<(String, String)> {
        let owner = Self::operator_target_owner_name(&query.target)?;
        let member = Self::operator_target_member_name(query.operator)?;
        let symbol = format!("{owner}::{member}");
        self.signatures
            .iter()
            .rev()
            .find(|entry| {
                entry.kind == DocKind::Function
                    && Self::operator_target_entry_matches(entry.signature.as_str(), &owner, member)
            })
            .map(|entry| {
                (
                    entry.qualified_name.clone(),
                    crate::surface_rendered_name(&entry.signature),
                )
            })
            .or_else(|| self.find_signature(&symbol))
    }

    fn operator_target_doc_entry<'a>(
        &'a self,
        query: &OperatorTargetQuery,
    ) -> Option<&'a DocEntry> {
        let owner = Self::operator_target_owner_name(&query.target)?;
        let member = Self::operator_target_member_name(query.operator)?;
        self.docs
            .iter()
            .rev()
            .find(|entry| {
                entry.kind == DocKind::Function
                    && Self::operator_target_entry_matches(
                        Self::display_signature_for_doc_entry(entry)
                            .as_deref()
                            .unwrap_or(""),
                        &owner,
                        member,
                    )
            })
            .or_else(|| {
                let trait_name = Self::operator_target_trait_name(query.operator)?;
                self.docs.iter().rev().find(|entry| {
                    entry
                        .doc
                        .contains(&format!("`{trait_name}` implementation for `{owner}`"))
                        || entry.doc.contains(&format!(
                            "`{trait_name}` implementation for `{owner}`-returning"
                        ))
                })
            })
    }

    fn operator_target_entry_matches(signature: &str, owner: &str, member: &str) -> bool {
        (signature.starts_with(&format!("{owner}::{member}("))
            || signature.starts_with(&format!("{owner}::{member}<"))
            || signature.contains(&format!("::{member}("))
            || signature.contains(&format!("::{member}<")))
            && (signature.contains(&format!(" for {owner}::"))
                || signature.contains(&format!(" for {owner}<"))
                || signature.starts_with(&format!("{owner}::{member}("))
                || signature.starts_with(&format!("{owner}::{member}<")))
    }

    fn operator_target_owner_name(target: &QueryArg) -> Option<String> {
        let ty = ast_ty_from_query_arg(target)?;
        Some(match ty {
            AstTy::Named(_, name) => name,
            AstTy::Generic(_, name, _) => name,
            _ => return None,
        })
    }

    fn operator_target_member_name(operator: &str) -> Option<&'static str> {
        Some(match operator {
            "+" => "add",
            "-" => "sub",
            "*" => "mul",
            "&&" => "and",
            "||" => "or",
            "==" => "eq",
            "!=" => "neq",
            "<" => "lt",
            "<=" => "lte",
            ">" => "gt",
            ">=" => "gte",
            "/" | ">>" => "compose",
            "++" => "concat",
            "|>" => "pipe_apply",
            "|*>" => "fmap",
            "|>=" => "bind",
            ">*" => "lift_compose",
            ">=>" => "kleisli_compose",
            _ => return None,
        })
    }

    fn operator_target_trait_name(operator: &str) -> Option<&'static str> {
        Some(match operator {
            "+" => "Add",
            "-" => "Sub",
            "*" => "Mul",
            "==" => "Eq",
            "!=" => "Neq",
            "<" | "<=" | ">" | ">=" => "Compare",
            "/" => "Compose",
            "++" => "Concat",
            "|>" => "PipeApply",
            "|*>" => "Functor",
            "|>=" => "Monad",
            ">>" => "Composable",
            ">*" => "LiftComposable",
            ">=>" => "KleisliComposable",
            _ => return None,
        })
    }

    fn definition_doc_kind(symbol: &str) -> Option<DocKind> {
        if symbol != "and"
            && symbol != "or"
            && OPERATOR_DOC_TRAIT_ALIASES
                .iter()
                .any(|(_, trait_name)| *trait_name == symbol)
        {
            Some(DocKind::Type)
        } else {
            None
        }
    }

    fn symbol_matches(qualified_name: &str, symbol: &str) -> bool {
        let qualified_name = sindr::names::CanonicalSymbolName::new(qualified_name);
        let symbol = sindr::names::VisibleSymbolRef::new(symbol);
        symbol.matches_qualified_name(&qualified_name)
    }

    fn is_qualified_symbol(symbol: &str) -> bool {
        symbol.contains("::")
    }

    fn function_entry_is_top_level_repl_surface(&self, qualified_name: &str) -> bool {
        let qualified_name = crate::surface_path_name(qualified_name);
        qualified_name.starts_with("__Script::")
            || qualified_name.starts_with("REPL::")
            || qualified_name.starts_with("__Repl::Session::")
            || qualified_name.starts_with(&format!(
                "{}::",
                crate::surface_path_name(&self.repl_module_path)
            ))
    }

    fn visible_doc_entries<'a>(&'a self, symbol: &str, kind: Option<DocKind>) -> Vec<&'a DocEntry> {
        let mut matches = self
            .docs
            .iter()
            .filter(|entry| kind.as_ref().is_none_or(|kind| &entry.kind == kind))
            .filter(|entry| self.doc_entry_matches_visible_symbol(entry, symbol))
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

    fn script_preload_doc_entries<'a>(
        &'a self,
        symbol: &str,
        kind: Option<DocKind>,
    ) -> Vec<&'a DocEntry> {
        let mut matches = self
            .docs
            .iter()
            .filter(|entry| kind.as_ref().is_none_or(|kind| &entry.kind == kind))
            .filter(|entry| entry.module_path.starts_with("__Script::"))
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

    fn doc_entry_matches_visible_symbol(&self, entry: &DocEntry, symbol: &str) -> bool {
        if !Self::symbol_matches(&entry.qualified_name, symbol) {
            return false;
        }
        if entry.kind == DocKind::Module {
            return sindr::names::surface_path_eq(&entry.qualified_name, symbol);
        }
        if Self::is_qualified_symbol(symbol) {
            let Some(decl) = self.qualified_declaration(symbol) else {
                return sindr::names::surface_path_eq(&entry.qualified_name, symbol);
            };
            return sindr::names::surface_path_eq(&entry.qualified_name, &decl.fq_name)
                && Self::declaration_is_public_surface(decl);
        }
        self.visible_uid_matches(symbol, &entry.qualified_name)
            || (entry.module_path.starts_with("__Script::")
                && self.sigil_session.lookup_uid(symbol).is_some())
    }

    fn qualified_declaration<'a>(&'a self, symbol: &str) -> Option<&'a sigil::DeclarationEntry> {
        self.declaration_index.get(symbol).or_else(|| {
            self.declaration_index
                .values()
                .find(|entry| sindr::names::surface_path_eq(&entry.fq_name, symbol))
        })
    }

    fn visible_uid_matches(&self, visible_name: &str, qualified_name: &str) -> bool {
        let Some(visible_uid) = self.sigil_session.lookup_uid(visible_name) else {
            return false;
        };
        self.qualified_declaration(qualified_name)
            .and_then(|entry| self.sigil_session.lookup_uid(&entry.fq_name))
            .or_else(|| self.sigil_session.lookup_uid(qualified_name))
            .or_else(|| {
                self.sigil_session
                    .lookup_uid(crate::surface_path_name(qualified_name))
            })
            .is_some_and(|qualified_uid| visible_uid == qualified_uid)
    }

    fn tuple_doc_output() -> ReplOutput {
        ReplOutput::DocResolved {
            symbol: "Tuple".to_string(),
            signature: None,
            summary: Some("Tuple doc surface for tuple values and Facet paths.".to_string()),
            source_snippet: Some(
                "Tuple is the doc surface for tuple values and `Tuple._N` facet roots.\nValues use `pair._0`, `pair._1`, ... and facet paths use `Tuple._0`, `Tuple._1`, ...\nExamples:\n- pair: (String, Int)\n- pair._0\n- pair._1\n- Facet::view(Tuple._0, pair)\n- Facet::view(Tuple._1, pair)\n- Facet::set(Tuple._1, pair, 3)"
                    .to_string(),
            ),
            details: Vec::new(),
        }
    }

    fn undocumented_doc_output(&self, symbol: &str) -> Option<ReplOutput> {
        let decl = self.visible_declaration(symbol)?;
        if self.function_entry_is_top_level_repl_surface(&decl.fq_name)
            && matches!(
                decl.kind,
                sigil::DeclarationKind::Def | sigil::DeclarationKind::Extractor
            )
        {
            return None;
        }
        let signature = self.declaration_signature(decl);
        let display_fq_name = crate::surface_path_name(&decl.fq_name);
        let display_name = crate::surface_path_name(&decl.name);
        let source_snippet = if let Some(signature) = &signature {
            format!(
                "`{}` resolves in the current scope, but it does not have an `@doc` entry yet.\nAdd `@doc` immediately before the declaration.\nExample:\n@doc \"\"\"\nDescribe `{}` here.\n\"\"\"\n{}",
                display_fq_name, display_name, signature
            )
        } else {
            format!(
                "`{}` resolves in the current scope, but it does not have an `@doc` entry yet.\nAdd `@doc` at the declaration site.\nExample:\n@doc \"\"\"\nDescribe `{}` here.\n\"\"\"",
                display_fq_name, display_name
            )
        };
        Some(ReplOutput::DocResolved {
            symbol: display_fq_name.to_string(),
            signature,
            summary: Some(format!("`{}` is currently undocumented.", display_name)),
            source_snippet: Some(source_snippet),
            details: vec!["status: undocumented".to_string()],
        })
    }

    fn declaration_is_public_surface(entry: &sigil::DeclarationEntry) -> bool {
        entry.visibility == spire::ast::Visibility::Public
    }

    fn declaration_is_repl_completion_surface(entry: &sigil::DeclarationEntry) -> bool {
        Self::declaration_is_public_surface(entry)
            && !entry.hidden
            && (entry.user_callable || entry.user_importable)
    }

    fn completion_visible_owner_name(label: &str) -> bool {
        !matches!(
            crate::surface_path_name(label),
            "MatchArms"
                | "CondClauses"
                | "BulkUpdateEntries"
                | "Hole"
                | "Lazy"
                | "StandbyInit"
                | "Closure"
        )
    }

    fn completion_visible_module_labels(&self) -> Vec<String> {
        let mut labels = BTreeSet::new();
        for entry in self.declaration_index.values() {
            if !Self::declaration_is_public_surface(entry) || entry.hidden {
                continue;
            }
            let module_label = crate::surface_path_name(&entry.module_path);
            if Self::completion_visible_module_name(module_label) {
                labels.insert(module_label.to_string());
            }
        }
        labels.into_iter().collect()
    }

    fn completion_visible_module_name(label: &str) -> bool {
        !label.is_empty()
            && !label.contains("::")
            && !label.starts_with("__")
            && Self::completion_visible_owner_name(label)
    }

    fn parse_pid_type_name(ty: &str) -> Option<&str> {
        ty.strip_prefix("PID<")?.strip_suffix('>')
    }

    fn process_metadata_for_pid_type<'a>(
        &self,
        ty: &'a str,
    ) -> Option<(&'a str, &ReplProcessMetadata)> {
        let process_name = Self::parse_pid_type_name(ty)?;
        Some((process_name, self.lookup_process_metadata(process_name)?))
    }

    fn process_metadata_for_singleton_owner<'a>(
        &self,
        symbol: &'a str,
    ) -> Option<(&'a str, &ReplProcessMetadata)> {
        if Self::is_qualified_symbol(symbol) {
            return None;
        }
        let metadata = self.lookup_process_metadata(symbol)?;
        (metadata.instance == spire::ast::ProcessInstance::Singleton).then_some((symbol, metadata))
    }

    fn process_metadata_for_owner<'a>(
        &self,
        symbol: &'a str,
    ) -> Option<(&'a str, &ReplProcessMetadata)> {
        if Self::is_qualified_symbol(symbol) {
            return None;
        }
        Some((symbol, self.lookup_process_metadata(symbol)?))
    }

    fn pid_type_from_value(value: &Value) -> Option<String> {
        match value {
            Value::Pid(pid) => Some(format!(
                "PID<{}>",
                crate::surface_rendered_name(&pid.process_name)
            )),
            _ => None,
        }
    }

    fn process_metadata_for_pid_value<'a>(
        &self,
        value: &'a Value,
    ) -> Option<(&'a str, &ReplProcessMetadata)> {
        let Value::Pid(pid) = value else {
            return None;
        };
        Some((
            pid.process_name.as_str(),
            self.lookup_process_metadata(&pid.process_name)?,
        ))
    }

    fn lookup_process_metadata(&self, name: &str) -> Option<&ReplProcessMetadata> {
        self.process_metadata.get(name).or_else(|| {
            self.process_metadata
                .iter()
                .find(|(key, _)| crate::surface_path_name(key) == crate::surface_path_name(name))
                .map(|(_, metadata)| metadata)
        })
    }

    fn process_hidden_doc_alias(&self, symbol: &str) -> Option<String> {
        let (owner, method) = symbol.rsplit_once("::")?;
        let metadata = self.lookup_process_metadata(owner)?;
        let hidden_owner = match (metadata.kind, method) {
            (spire::ast::ProcessKind::Agent, "pid") => "Agent",
            (spire::ast::ProcessKind::Agent, "spawn")
                if metadata.instance == spire::ast::ProcessInstance::Worker =>
            {
                "Agent"
            }
            (spire::ast::ProcessKind::GenServer, "pid") => "GenServer",
            (spire::ast::ProcessKind::GenServer, "spawn")
                if metadata.instance == spire::ast::ProcessInstance::Worker =>
            {
                "GenServer"
            }
            (
                spire::ast::ProcessKind::Supervisor
                | spire::ast::ProcessKind::RuntimeSupervisor
                | spire::ast::ProcessKind::DynamicSupervisor,
                "spawn" | "adopt" | "status" | "workers",
            ) => "Supervisor",
            _ => return None,
        };
        Some(format!("{hidden_owner}::{method}"))
    }

    fn doc_output_with_symbol_and_signature(
        entry: &DocEntry,
        symbol: String,
        signature: Option<String>,
        details: Vec<String>,
    ) -> ReplOutput {
        let summary = entry
            .doc
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(ToString::to_string);
        ReplOutput::DocResolved {
            symbol,
            signature,
            summary,
            source_snippet: Some(entry.doc.clone()),
            details,
        }
    }

    fn concrete_process_alias_doc_output(&self, symbol: &str) -> Option<ReplOutput> {
        let hidden_symbol = self.process_hidden_doc_alias(symbol)?;
        let entry = self.docs.iter().find(|entry| {
            entry.kind == DocKind::Function && entry.qualified_name == hidden_symbol
        })?;
        let signature = self
            .find_signature(symbol)
            .or_else(|| self.concrete_process_alias_signature(symbol))
            .map(|(_, signature)| signature);
        Some(Self::doc_output_with_symbol_and_signature(
            entry,
            symbol.to_string(),
            signature,
            Vec::new(),
        ))
    }

    fn concrete_process_alias_signature(&self, symbol: &str) -> Option<(String, String)> {
        self.process_hidden_doc_alias(symbol)?;
        if let Some((owner, metadata)) = symbol.rsplit_once("::").and_then(|(owner, method)| {
            (method == "pid").then_some((owner, self.lookup_process_metadata(owner)?))
        }) {
            if metadata.instance == spire::ast::ProcessInstance::Singleton {
                let owner = crate::surface_rendered_name(owner);
                return Some((symbol.to_string(), format!("{symbol}() -> PID<{owner}>")));
            }
        }
        self.vm
            .function_entries()
            .iter()
            .rev()
            .filter_map(|entry| {
                let qualified_name = entry.qualified_name.as_ref()?;
                let signature = entry.signature.as_ref()?;
                (crate::surface_path_name(qualified_name) == crate::surface_path_name(symbol))
                    .then(|| (qualified_name.clone(), signature.clone()))
            })
            .next()
    }

    fn process_owner_type_lines(
        &self,
        owner: &str,
        _metadata: &ReplProcessMetadata,
    ) -> Vec<String> {
        vec![
            owner.to_string(),
            format!("type: PID<{}>", crate::surface_rendered_name(owner)),
            "identity: TypeIdentity::Type".to_string(),
        ]
    }

    fn process_owner_info_lines(&self, owner: &str, metadata: &ReplProcessMetadata) -> Vec<String> {
        let owner = crate::surface_rendered_name(owner);
        vec![
            owner.clone(),
            "kind: process singleton".to_string(),
            format!("origin: {}", Self::origin_for_name(&owner)),
            format!("defined: PID<{owner}>"),
            format!("type: PID<{owner}>"),
            "identity: TypeIdentity::Type".to_string(),
            format!("instance: {:?}", metadata.instance),
            format!("runtime kind: {:?}", metadata.kind),
        ]
    }

    fn process_kind_heading(kind: spire::ast::ProcessKind) -> &'static str {
        match kind {
            spire::ast::ProcessKind::Agent => "Agent",
            spire::ast::ProcessKind::GenServer => "GenServer",
            spire::ast::ProcessKind::Supervisor => "Supervisor",
            spire::ast::ProcessKind::RuntimeSupervisor => "RuntimeSupervisor",
            spire::ast::ProcessKind::DynamicSupervisor => "DynamicSupervisor",
            spire::ast::ProcessKind::Task => "Task",
        }
    }

    fn process_handler_label(kind: spire::ast::ProcessRuntimeHandlerKind) -> &'static str {
        match kind {
            spire::ast::ProcessRuntimeHandlerKind::Init => "@init",
            spire::ast::ProcessRuntimeHandlerKind::Get => "@get",
            spire::ast::ProcessRuntimeHandlerKind::Set => "@set",
            spire::ast::ProcessRuntimeHandlerKind::Call => "@call",
            spire::ast::ProcessRuntimeHandlerKind::Cast => "@cast",
        }
    }

    fn render_process_surface_signature(&self, owner: &str, method: &str) -> Option<String> {
        let symbol = format!("{owner}::{method}");
        let (qualified_name, signature) = self
            .find_signature(&symbol)
            .or_else(|| self.concrete_process_alias_signature(&symbol))?;
        let rendered = Self::render_signature_with_qualified_name(&qualified_name, signature);
        let owner_prefix = format!("{}::", crate::surface_rendered_name(owner));
        Some(
            rendered
                .strip_prefix(&owner_prefix)
                .unwrap_or(&rendered)
                .to_string(),
        )
    }

    fn prepend_pid_param_to_signature(signature: &str, owner: &str) -> Option<String> {
        let (head, tail) = signature.split_once('(')?;
        let (params, rest) = tail.rsplit_once(')')?;
        let pid_param = format!("pid: PID<{}>", crate::surface_rendered_name(owner));
        let params = params.trim();
        let new_params = if params.is_empty() {
            pid_param
        } else {
            format!("{pid_param}, {params}")
        };
        Some(format!("{head}({new_params}){rest}"))
    }

    fn render_process_message_signature(
        &self,
        owner: &str,
        metadata: &ReplProcessMetadata,
        method: &str,
    ) -> Option<String> {
        let signature = self.render_process_surface_signature(owner, method)?;
        if metadata.instance == spire::ast::ProcessInstance::Singleton {
            return Self::prepend_pid_param_to_signature(&signature, owner);
        }
        Some(signature)
    }

    fn render_process_init_summary_signature(&self, owner: &str, method: &str) -> Option<String> {
        let signature = self.render_process_surface_signature(owner, method)?;
        let pid_return = format!("PID<{}>", crate::surface_rendered_name(owner));
        let result_return = format!("Result<{pid_return}>");
        Some(signature.replace(&format!("-> {pid_return}"), &format!("-> {result_return}")))
    }

    fn process_owner_sig_summary_lines(
        &self,
        owner: &str,
        metadata: &ReplProcessMetadata,
    ) -> Option<Vec<String>> {
        let owner = crate::surface_rendered_name(owner);
        let mut lines = vec![format!(
            "{} {}",
            Self::process_kind_heading(metadata.kind),
            owner
        )];

        let init_spec = metadata
            .handler_specs
            .iter()
            .find(|spec| spec.kind == spire::ast::ProcessRuntimeHandlerKind::Init)?;
        if let Some(init_sig) = self.render_process_init_summary_signature(&owner, &init_spec.name)
        {
            lines.push(format!(
                "{} {}",
                Self::process_handler_label(init_spec.kind),
                init_sig
            ));
        }
        if metadata.instance == spire::ast::ProcessInstance::Singleton {
            if let Some(pid_sig) = self.render_process_surface_signature(&owner, "pid") {
                lines.push(format!("@pid {pid_sig}"));
            }
        }
        for spec in &metadata.handler_specs {
            if spec.kind == spire::ast::ProcessRuntimeHandlerKind::Init {
                continue;
            }
            if let Some(sig) = self.render_process_message_signature(&owner, metadata, &spec.name) {
                lines.push(format!(
                    "{} {}",
                    Self::process_handler_label(spec.kind),
                    sig
                ));
            }
        }
        Some(lines)
    }

    fn process_pid_binding_sig_summary_lines(
        &self,
        owner: &str,
        metadata: &ReplProcessMetadata,
    ) -> Option<Vec<String>> {
        let owner = crate::surface_rendered_name(owner);
        let mut lines = vec![format!("PID<{owner}> messaging")];
        for spec in &metadata.handler_specs {
            if spec.kind == spire::ast::ProcessRuntimeHandlerKind::Init {
                continue;
            }
            if let Some(sig) = self.render_process_message_signature(&owner, metadata, &spec.name) {
                lines.push(format!(
                    "{} {}",
                    Self::process_handler_label(spec.kind),
                    sig
                ));
            }
        }
        (lines.len() > 1).then_some(lines)
    }

    fn method_trait_alias(symbol: &str) -> Option<&'static str> {
        METHOD_DOC_TRAIT_ALIASES
            .iter()
            .find_map(|(method, trait_name)| (*method == symbol).then_some(*trait_name))
    }

    fn compare_method_doc_target(symbol: &str) -> Option<&'static str> {
        COMPARE_METHOD_DOC_TARGETS
            .iter()
            .find_map(|(method, target)| (*method == symbol).then_some(*target))
    }

    fn visible_helper_doc_alias(&self, symbol: &str) -> Option<String> {
        if let Some(target) = Self::compare_method_doc_target(symbol) {
            return self.visible_compare_method_doc_alias(symbol).or_else(|| {
                self.visible_declaration(symbol)
                    .is_none()
                    .then(|| target.to_string())
            });
        }
        self.visible_helper_trait_alias(symbol)
    }

    fn visible_compare_method_doc_alias(&self, symbol: &str) -> Option<String> {
        let target = Self::compare_method_doc_target(symbol)?;
        let trait_name = Self::method_trait_alias(symbol)?;
        let decl = self.visible_declaration(symbol)?;
        if decl.kind != sigil::DeclarationKind::TraitMethod {
            return None;
        }
        let (owner_fq_name, _) = decl.fq_name.rsplit_once("::")?;
        let owner = self.declaration_index.get(owner_fq_name)?;
        (owner.kind == sigil::DeclarationKind::Trait
            && owner.auto_import
            && owner.name == trait_name)
            .then(|| target.to_string())
    }

    fn visible_helper_trait_alias(&self, symbol: &str) -> Option<String> {
        let trait_name = Self::method_trait_alias(symbol)?;
        let decl = self.visible_declaration(symbol)?;
        if decl.kind != sigil::DeclarationKind::TraitMethod {
            return None;
        }
        let (owner_fq_name, _) = decl.fq_name.rsplit_once("::")?;
        let owner = self.declaration_index.get(owner_fq_name)?;
        (owner.kind == sigil::DeclarationKind::Trait
            && owner.auto_import
            && owner.name == trait_name)
            .then(|| trait_name.to_string())
    }

    fn visible_declaration<'a>(&'a self, symbol: &str) -> Option<&'a sigil::DeclarationEntry> {
        if Self::is_qualified_symbol(symbol) {
            let entry = self.qualified_declaration(symbol)?;
            return Self::declaration_is_public_surface(entry).then_some(entry);
        }
        if let Some(visible_uid) = self.sigil_session.lookup_uid(symbol) {
            return self.declaration_index.values().find(|entry| {
                Self::declaration_is_public_surface(entry)
                    && (entry.name == symbol
                        || entry
                            .name
                            .rsplit("::")
                            .next()
                            .is_some_and(|tail| tail == symbol))
                    && self.sigil_session.lookup_uid(&entry.fq_name) == Some(visible_uid)
            });
        }

        // Builtin types intentionally do not enter the value scope, but they
        // remain public declarations and must still be addressable by :doc.
        // Resolve the bare type name directly from the declaration index only
        // after the ordinary scope lookup has found nothing, preserving local
        // binding shadowing for all scope-visible declarations.
        self.declaration_index.values().find(|entry| {
            entry.kind == sigil::DeclarationKind::BuiltinType
                && Self::declaration_is_public_surface(entry)
                && sindr::names::surface_path_eq(&entry.fq_name, symbol)
        })
    }

    fn private_declaration<'a>(&'a self, symbol: &str) -> Option<&'a sigil::DeclarationEntry> {
        if Self::is_qualified_symbol(symbol) {
            let entry = self.qualified_declaration(symbol)?;
            return (!Self::declaration_is_public_surface(entry)).then_some(entry);
        }
        let visible_uid = self.sigil_session.lookup_uid(symbol)?;
        self.declaration_index.values().find(|entry| {
            !Self::declaration_is_public_surface(entry)
                && (entry.name == symbol
                    || entry
                        .name
                        .rsplit("::")
                        .next()
                        .is_some_and(|tail| tail == symbol))
                && self.sigil_session.lookup_uid(&entry.fq_name) == Some(visible_uid)
        })
    }

    fn private_doc_output(entry: &sigil::DeclarationEntry) -> ReplOutput {
        Self::plain(vec![
            format!(
                "`{}` is private and cannot be queried with `:doc`.",
                crate::surface_path_name(&entry.fq_name)
            ),
            "Add `@doc` only to public declarations.".to_string(),
        ])
        .output
    }

    fn private_sig_output(entry: &sigil::DeclarationEntry) -> ReplOutput {
        Self::plain(vec![
            format!(
                "`{}` is private and cannot be queried with `:sig`.",
                crate::surface_path_name(&entry.fq_name)
            ),
            "Only public declarations are visible to REPL signature lookup.".to_string(),
        ])
        .output
    }

    fn declaration_signature(&self, decl: &sigil::DeclarationEntry) -> Option<String> {
        match decl.kind {
            sigil::DeclarationKind::Struct => Some(crate::format_struct_signature(
                crate::surface_path_name(&decl.name),
            )),
            sigil::DeclarationKind::Record => Some(crate::format_record_signature(
                crate::surface_path_name(&decl.name),
            )),
            sigil::DeclarationKind::Deferror => {
                Some(format!("deferror {}", crate::surface_path_name(&decl.name)))
            }
            sigil::DeclarationKind::Enum => {
                Some(format!("defenum {}", crate::surface_path_name(&decl.name)))
            }
            sigil::DeclarationKind::EnumVariant => {
                self.enum_variant_signature_entry(decl)
                    .map(|(qualified_name, signature)| {
                        Self::render_signature_with_qualified_name(&qualified_name, signature)
                    })
            }
            sigil::DeclarationKind::BuiltinType => {
                Some(format!("type {}", crate::surface_path_name(&decl.name)))
            }
            _ => self
                .find_signature(&decl.fq_name)
                .map(|(qualified_name, signature)| {
                    Self::render_signature_with_qualified_name(&qualified_name, signature)
                }),
        }
    }

    fn declaration_signature_entry(
        &self,
        decl: &sigil::DeclarationEntry,
    ) -> Option<(String, String)> {
        match decl.kind {
            sigil::DeclarationKind::EnumVariant => self.enum_variant_signature_entry(decl),
            _ => self.find_signature(&decl.fq_name),
        }
    }

    fn enum_variant_signature_entry(
        &self,
        decl: &sigil::DeclarationEntry,
    ) -> Option<(String, String)> {
        if decl.kind != sigil::DeclarationKind::EnumVariant {
            return None;
        }
        let (owner, _) = decl.fq_name.rsplit_once("::")?;
        let variant = self
            .scar_session
            .enum_variants_of(owner)?
            .iter()
            .find(|variant| {
                variant.short_name == decl.name
                    || variant.constructor_name == decl.name
                    || decl.name.ends_with(&format!("::{}", variant.short_name))
            })?;
        let params = variant
            .payload
            .iter()
            .map(Self::ty_to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let signature = format!(
            "{}({params}) -> {}",
            crate::surface_path_name(&decl.fq_name),
            Self::ty_to_string(&variant.enum_ty)
        );
        Some((decl.fq_name.clone(), signature))
    }

    fn doc_resolved_output(entry: &DocEntry) -> ReplOutput {
        Self::doc_resolved_output_with_details(entry, Vec::new())
    }

    fn doc_resolved_output_with_details(entry: &DocEntry, details: Vec<String>) -> ReplOutput {
        let summary = entry
            .doc
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(ToString::to_string);
        ReplOutput::DocResolved {
            symbol: crate::surface_path_name(&entry.qualified_name).to_string(),
            signature: Self::display_signature_for_doc_entry(entry),
            summary,
            source_snippet: Some(entry.doc.clone()),
            details,
        }
    }

    fn display_signature_for_doc_entry(entry: &DocEntry) -> Option<String> {
        entry
            .signature
            .clone()
            .filter(|signature| Self::trait_signature_methods(signature).is_empty())
            .map(|signature| crate::surface_rendered_name(&signature))
    }

    fn handle_doc_binding(&self, symbol: &str) -> Option<ReplResult> {
        let binding = self.binding_info(symbol)?;
        let kind = self.binding_callable_kind(binding)?;
        let value = self.vm.get_local(binding.slot_id)?;

        let entry = match kind {
            forge::ReplCallableKind::Capture => self.capture_doc_entry(&value)?,
            forge::ReplCallableKind::Closure => self.closure_doc_entry()?,
        };

        Some(ReplResult::ok(Self::doc_resolved_output_with_details(
            entry,
            self.binding_doc_details(symbol, binding, &value, kind),
        )))
    }

    fn binding_doc_type_symbol(binding: &forge::BindingInfo) -> Option<String> {
        if let Some(ty) = parse_binding_query_type(&binding.ty) {
            match ty {
                AstTy::Named(_, name) => return Some(name),
                AstTy::Generic(_, name, _) => return Some(name),
                _ => {}
            }
        }
        let surface = crate::surface_rendered_name(&binding.ty);
        let head = surface.split('<').next().unwrap_or(surface.as_str()).trim();
        Self::is_type_lookup_symbol(head).then(|| head.to_string())
    }

    fn closure_doc_entry(&self) -> Option<&DocEntry> {
        self.matching_doc_entries("Closure", Some(DocKind::Type))
            .into_iter()
            .next()
    }

    fn capture_doc_entry(&self, value: &Value) -> Option<&DocEntry> {
        let Value::Callable(callable) = value else {
            return None;
        };
        let module = callable.metadata.module.as_deref()?;
        let name = callable.metadata.name.as_deref()?;
        let qualified = format!("{module}::{name}");
        self.docs
            .iter()
            .find(|entry| entry.qualified_name == qualified && entry.kind == DocKind::Function)
            .or_else(|| {
                self.matching_doc_entries(&qualified, None)
                    .into_iter()
                    .find(|entry| entry.kind == DocKind::Function)
            })
            .or_else(|| {
                self.matching_doc_entries(name, None)
                    .into_iter()
                    .find(|entry| entry.kind == DocKind::Function)
            })
    }

    fn binding_doc_details(
        &self,
        symbol: &str,
        binding: &forge::BindingInfo,
        value: &Value,
        kind: forge::ReplCallableKind,
    ) -> Vec<String> {
        match kind {
            forge::ReplCallableKind::Capture => {
                let mut details = vec![format!("binding: {symbol}")];
                details.push(format!(
                    "type: {}",
                    crate::surface_rendered_name(&binding.ty)
                ));
                if let Value::Callable(callable) = value {
                    let capture_count = callable.lexical_captures.len();
                    if capture_count == 0 {
                        details.push("captures: none".to_string());
                    } else {
                        details.push(format!("captures: {capture_count} bound value(s)"));
                    }
                }
                if let Some(derived) = self.callable_origin_label(value) {
                    details.push(format!("derived from: {derived}"));
                }
                details
            }
            forge::ReplCallableKind::Closure => self.closure_binding_doc_details(symbol, binding),
        }
    }

    fn closure_binding_doc_details(
        &self,
        symbol: &str,
        binding: &forge::BindingInfo,
    ) -> Vec<String> {
        let rendered_ty = crate::surface_rendered_name(&binding.ty);
        let mut details = vec![format!("type: {rendered_ty}")];
        if let Some(example) = Self::closure_binding_example(symbol, &rendered_ty) {
            details.push(format!("example: {example}"));
        }
        details
    }

    fn closure_binding_example(symbol: &str, signature: &str) -> Option<String> {
        let ty = parse_binding_query_type(signature)?;
        let AstTy::Func(_, params, ret) = ty else {
            return None;
        };
        let args = params
            .iter()
            .map(format_query_ty)
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "ret: {} = {}({args})",
            format_query_ty(ret.as_ref()),
            symbol
        ))
    }

    fn callable_origin_label(&self, value: &Value) -> Option<String> {
        let Value::Callable(callable) = value else {
            return None;
        };
        match (
            callable.metadata.module.as_deref(),
            callable.metadata.name.as_deref(),
        ) {
            (Some("<local>"), Some(name)) => Some(name.to_string()),
            (Some(module), Some(name)) => Some(format!("{module}::{name}")),
            (None, Some(name)) => Some(name.to_string()),
            _ => None,
        }
    }

    fn doc_method_tail(qualified_name: &str) -> &str {
        qualified_name.rsplit("::").next().unwrap_or(qualified_name)
    }

    fn callee_tail(callee: &str) -> &str {
        callee.rsplit("::").next().unwrap_or(callee)
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
                .map(|entry| format!("  {}", crate::surface_path_name(&entry.qualified_name))),
        );
        rendered.push(
            "Use a qualified name or add type annotations, for example `:doc compare(Int, Int)`."
                .to_string(),
        );
        rendered
    }

    fn ambiguous_signature_lines(symbol: &str, entries: &[&SignatureEntry]) -> Vec<String> {
        let mut rendered = vec![format!("{symbol} has multiple signatures:")];
        rendered.extend(
            entries
                .iter()
                .map(|entry| format!("  {}", crate::surface_path_name(&entry.qualified_name))),
        );
        rendered.push(
            "Use a qualified name or add type annotations, for example `:sig compare(Int, Int)`."
                .to_string(),
        );
        rendered
    }

    fn handle_doc_typed_call(&self, source_query: &str, query: &TypedCallQuery) -> ReplResult {
        if let Some(message) = self.invalid_attached_extractor_query_message(query) {
            return Self::plain(vec![message]);
        }
        let arg_types = match self.query_arg_types(query.args.as_slice()) {
            Ok(arg_types) => arg_types,
            Err(message) => {
                return Self::plain(vec![message]);
            }
        };
        if let Some(result) = self.handle_doc_trait_target(&query.callee, &arg_types) {
            return result;
        }
        let matches = self.match_typed_call_docs(query);
        match matches.as_slice() {
            [] => self
                .private_declaration(query.callee.strip_suffix('!').unwrap_or(&query.callee))
                .map(|entry| ReplResult::ok(Self::private_doc_output(entry)))
                .unwrap_or_else(|| {
                    Self::plain(vec![format!("No docs found for {}", source_query)])
                }),
            [entry] => ReplResult::ok(Self::doc_resolved_output(entry)),
            entries => Self::plain(Self::ambiguous_doc_lines(source_query, entries)),
        }
    }

    fn handle_doc_trait_target(&self, callee: &str, arg_types: &[String]) -> Option<ReplResult> {
        let decl = self.visible_declaration(callee)?;
        if decl.kind != sigil::DeclarationKind::Trait {
            return None;
        }
        let entries = self.type_owner_doc_entries(callee)?;
        let entry = entries.into_iter().next()?;
        Some(ReplResult::ok(Self::doc_resolved_output_with_details(
            entry,
            vec![
                format!("target: {}", arg_types.join(", ")),
                "impl docs are not synthesized".to_string(),
            ],
        )))
    }

    fn match_typed_call_docs<'a>(&'a self, query: &TypedCallQuery) -> Vec<&'a DocEntry> {
        if let Some(matches) = self.match_special_form_typed_call_docs(query) {
            return matches;
        }
        if let Some(matches) = self.match_owner_typed_call_docs(query) {
            return matches;
        }

        let Ok(arg_types) = self.query_arg_types(query.args.as_slice()) else {
            return Vec::new();
        };
        let Some(receiver_ty) = arg_types.first() else {
            return Vec::new();
        };
        let preferred_trait = METHOD_DOC_TRAIT_ALIASES
            .iter()
            .find_map(|(method, trait_name)| (*method == query.callee).then_some(*trait_name))
            .or_else(|| {
                OPERATOR_DOC_TRAIT_ALIASES
                    .iter()
                    .find_map(|(alias, trait_name)| (*alias == query.callee).then_some(*trait_name))
            });
        let callee_tail = Self::callee_tail(&query.callee);
        let callee_is_qualified = Self::is_qualified_symbol(&query.callee);
        let mut matches = self
            .docs
            .iter()
            .filter(|entry| entry.kind == DocKind::Function)
            .filter(|entry| Self::doc_method_tail(&entry.qualified_name) == callee_tail)
            .filter(|entry| {
                if !callee_is_qualified {
                    return true;
                }
                entry.qualified_name == query.callee
                    || entry
                        .signature
                        .as_deref()
                        .is_some_and(|sig| sig.starts_with(&format!("{}(", query.callee)))
            })
            .filter(|entry| {
                entry.signature.as_deref().is_some_and(|sig| {
                    if sig.starts_with("impl ") {
                        return sig.contains(&format!(" for {receiver_ty}::{callee_tail}"));
                    }
                    Self::signature_matches_callee(sig, &query.callee)
                })
            })
            .filter(|entry| {
                entry
                    .signature
                    .as_deref()
                    .is_none_or(|sig| self.signature_accepts_arg_types(sig, &arg_types))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|entry| {
            self.typed_call_doc_rank(entry, preferred_trait, receiver_ty, callee_tail)
        });
        if let Some(best_rank) = matches
            .first()
            .map(|entry| self.typed_call_doc_rank(entry, preferred_trait, receiver_ty, callee_tail))
        {
            matches.retain(|entry| {
                self.typed_call_doc_rank(entry, preferred_trait, receiver_ty, callee_tail)
                    == best_rank
            });
        }
        matches.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        matches
    }

    fn match_typed_call_signatures<'a>(
        &'a self,
        query: &TypedCallQuery,
    ) -> Vec<&'a SignatureEntry> {
        if let Some(matches) = self.match_special_form_typed_call_signatures(query) {
            return matches;
        }
        if let Some(matches) = self.match_owner_typed_call_signatures(query) {
            return matches;
        }

        let Ok(arg_types) = self.query_arg_types(query.args.as_slice()) else {
            return Vec::new();
        };
        let Some(receiver_ty) = arg_types.first() else {
            return Vec::new();
        };
        let preferred_trait = METHOD_DOC_TRAIT_ALIASES
            .iter()
            .find_map(|(method, trait_name)| (*method == query.callee).then_some(*trait_name))
            .or_else(|| {
                OPERATOR_DOC_TRAIT_ALIASES
                    .iter()
                    .find_map(|(alias, trait_name)| (*alias == query.callee).then_some(*trait_name))
            });
        let callee_tail = Self::callee_tail(&query.callee);
        let callee_is_qualified = Self::is_qualified_symbol(&query.callee);
        let mut matches = self
            .signatures
            .iter()
            .filter(|entry| entry.kind == DocKind::Function)
            .filter(|entry| Self::doc_method_tail(&entry.qualified_name) == callee_tail)
            .filter(|entry| {
                if !callee_is_qualified {
                    return true;
                }
                entry.qualified_name == query.callee
                    || entry.signature.starts_with(&format!("{}(", query.callee))
            })
            .filter(|entry| {
                let sig = entry.signature.as_str();
                if sig.starts_with("impl ") {
                    return sig.contains(&format!(" for {receiver_ty}::{callee_tail}"));
                }
                Self::signature_matches_callee(sig, &query.callee)
            })
            .filter(|entry| self.signature_accepts_arg_types(&entry.signature, &arg_types))
            .collect::<Vec<_>>();
        matches.sort_by_key(|entry| {
            self.typed_call_signature_rank(entry, preferred_trait, receiver_ty, callee_tail)
        });
        if let Some(best_rank) = matches.first().map(|entry| {
            self.typed_call_signature_rank(entry, preferred_trait, receiver_ty, callee_tail)
        }) {
            matches.retain(|entry| {
                self.typed_call_signature_rank(entry, preferred_trait, receiver_ty, callee_tail)
                    == best_rank
            });
        }
        matches.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        matches
    }

    fn typed_call_doc_rank(
        &self,
        entry: &DocEntry,
        preferred_trait: Option<&str>,
        receiver_ty: &str,
        callee_tail: &str,
    ) -> u8 {
        let Some(signature) = entry.signature.as_deref() else {
            return u8::MAX;
        };
        let Some(trait_name) = preferred_trait else {
            return 0;
        };
        if signature.starts_with(&format!(
            "impl {trait_name} for {receiver_ty}::{callee_tail}"
        )) {
            return 0;
        }
        if crate::surface_path_name(&entry.qualified_name) == format!("{trait_name}::{callee_tail}")
            || signature.starts_with(&format!("{trait_name}::{callee_tail}("))
        {
            return 1;
        }
        if signature.starts_with(&format!("impl {trait_name} for ")) {
            return 2;
        }
        3
    }

    fn typed_call_signature_rank(
        &self,
        entry: &SignatureEntry,
        preferred_trait: Option<&str>,
        receiver_ty: &str,
        callee_tail: &str,
    ) -> u8 {
        let signature = entry.signature.as_str();
        let Some(trait_name) = preferred_trait else {
            return if signature.starts_with("impl ") { 0 } else { 1 };
        };
        if signature.starts_with(&format!(
            "impl {trait_name} for {receiver_ty}::{callee_tail}"
        )) {
            return 0;
        }
        if crate::surface_path_name(&entry.qualified_name) == format!("{trait_name}::{callee_tail}")
            || signature.starts_with(&format!("{trait_name}::{callee_tail}("))
        {
            return 1;
        }
        if signature.starts_with(&format!("impl {trait_name} for ")) {
            return 2;
        }
        3
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

    fn match_special_form_typed_call_signatures<'a>(
        &'a self,
        query: &TypedCallQuery,
    ) -> Option<Vec<&'a SignatureEntry>> {
        match query.callee.as_str() {
            "dbg!" => {
                let mut matches = self
                    .signatures
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

    fn match_owner_typed_call_docs<'a>(
        &'a self,
        query: &TypedCallQuery,
    ) -> Option<Vec<&'a DocEntry>> {
        if let Some(owner) = query.callee.strip_suffix('!') {
            let decl = self.visible_declaration(owner)?;
            if decl.kind != sigil::DeclarationKind::Struct {
                return Some(Vec::new());
            }
            let qualified_name = format!("{}::deconstruct", decl.fq_name);
            let Ok(arg_types) = self.query_arg_types(query.args.as_slice()) else {
                return Some(Vec::new());
            };
            let mut matches = self
                .docs
                .iter()
                .filter(|entry| entry.kind == DocKind::Function)
                .filter(|entry| {
                    crate::surface_path_name(&entry.qualified_name)
                        == crate::surface_path_name(&qualified_name)
                })
                .filter(|entry| {
                    entry.signature.as_deref().is_none_or(|sig| {
                        arg_types.is_empty() || self.signature_accepts_arg_types(sig, &arg_types)
                    })
                })
                .collect::<Vec<_>>();
            matches.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
            return Some(matches);
        }

        let decl = self.visible_declaration(&query.callee)?;
        if decl.kind != sigil::DeclarationKind::Struct {
            return None;
        }
        let Ok(arg_types) = self.query_arg_types(query.args.as_slice()) else {
            return Some(Vec::new());
        };
        let qualified_name = format!("{}::new", decl.fq_name);
        let mut matches = self
            .docs
            .iter()
            .filter(|entry| entry.kind == DocKind::Function)
            .filter(|entry| {
                crate::surface_path_name(&entry.qualified_name)
                    == crate::surface_path_name(&qualified_name)
            })
            .filter(|entry| {
                entry
                    .signature
                    .as_deref()
                    .is_none_or(|sig| self.signature_accepts_arg_types(sig, &arg_types))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        Some(matches)
    }

    fn match_owner_typed_call_signatures<'a>(
        &'a self,
        query: &TypedCallQuery,
    ) -> Option<Vec<&'a SignatureEntry>> {
        if let Some(owner) = query.callee.strip_suffix('!') {
            let decl = self.visible_declaration(owner)?;
            if decl.kind != sigil::DeclarationKind::Struct {
                return Some(Vec::new());
            }
            let qualified_name = format!("{}::deconstruct", decl.fq_name);
            let Ok(arg_types) = self.query_arg_types(query.args.as_slice()) else {
                return Some(Vec::new());
            };
            let mut matches = self
                .signatures
                .iter()
                .filter(|entry| entry.kind == DocKind::Function)
                .filter(|entry| {
                    crate::surface_path_name(&entry.qualified_name)
                        == crate::surface_path_name(&qualified_name)
                })
                .filter(|entry| {
                    arg_types.is_empty()
                        || self.signature_accepts_arg_types(&entry.signature, &arg_types)
                })
                .collect::<Vec<_>>();
            matches.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
            return Some(matches);
        }

        let decl = self.visible_declaration(&query.callee)?;
        if decl.kind != sigil::DeclarationKind::Struct {
            return None;
        }
        let Ok(arg_types) = self.query_arg_types(query.args.as_slice()) else {
            return Some(Vec::new());
        };
        let qualified_name = format!("{}::new", decl.fq_name);
        let mut matches = self
            .signatures
            .iter()
            .filter(|entry| entry.kind == DocKind::Function)
            .filter(|entry| {
                crate::surface_path_name(&entry.qualified_name)
                    == crate::surface_path_name(&qualified_name)
            })
            .filter(|entry| self.signature_accepts_arg_types(&entry.signature, &arg_types))
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        Some(matches)
    }

    fn invalid_attached_extractor_query_message(&self, query: &TypedCallQuery) -> Option<String> {
        query.callee.strip_suffix('!')?;
        None
    }

    fn is_attached_extractor_owner_query(&self, symbol: &str) -> bool {
        let Some(owner) = symbol.strip_suffix('!') else {
            return false;
        };
        self.visible_declaration(owner)
            .is_some_and(|decl| decl.kind == sigil::DeclarationKind::Struct)
    }

    fn special_form_doc_entry(&self, symbol: &str) -> Option<&DocEntry> {
        self.docs.iter().find(|entry| {
            entry.kind == DocKind::Function
                && entry.qualified_name == format!("Bootstrap::{symbol}")
        })
    }

    fn special_form_signature_entry(&self, symbol: &str) -> Option<&SignatureEntry> {
        self.signatures.iter().find(|entry| {
            entry.kind == DocKind::Function
                && entry.qualified_name == format!("Bootstrap::{symbol}")
        })
    }

    fn signature_matches_callee(signature: &str, callee: &str) -> bool {
        signature.starts_with(&format!("{callee}("))
            || signature.contains(&format!("::{callee}("))
            || signature.starts_with(&format!("@intrinsic def {callee}<"))
            || signature.starts_with(&format!("@intrinsic def {callee}("))
    }

    fn find_signature(&self, symbol: &str) -> Option<(String, String)> {
        if symbol == "Tuple" {
            return None;
        }
        if let Some(entry) = self.special_form_doc_entry(symbol) {
            if let Some(signature) = Self::display_signature_for_doc_entry(entry) {
                return Some((entry.qualified_name.clone(), signature));
            }
        }
        if let Some(entry) = self.special_form_signature_entry(symbol) {
            return Some((
                entry.qualified_name.clone(),
                crate::surface_rendered_name(&entry.signature),
            ));
        }
        let canonical = self
            .visible_helper_doc_alias(symbol)
            .unwrap_or_else(|| Self::canonical_symbol(symbol).to_string());
        let qualified_lookup = Self::is_qualified_symbol(&canonical);
        if qualified_lookup
            && self
                .qualified_declaration(&canonical)
                .is_some_and(|entry| !Self::declaration_is_public_surface(entry))
        {
            return None;
        }
        let visible_uid = (!qualified_lookup)
            .then(|| self.sigil_session.lookup_uid(&canonical))
            .flatten();

        if let Some(entry) = self.docs.iter().rev().find(|entry| {
            if entry.kind != DocKind::Function {
                return false;
            }
            if !Self::symbol_matches(&entry.qualified_name, &canonical) {
                return false;
            }
            if qualified_lookup {
                crate::surface_path_name(&entry.qualified_name)
                    == crate::surface_path_name(&canonical)
            } else if let Some(uid) = visible_uid {
                self.sigil_session.lookup_uid(&entry.qualified_name) == Some(uid)
            } else {
                false
            }
        }) {
            if let Some(signature) = Self::display_signature_for_doc_entry(entry) {
                return Some((entry.qualified_name.clone(), signature));
            }
        }

        if canonical == symbol {
            if let Some(found) = self
                .vm
                .function_entries()
                .iter()
                .rev()
                .filter(|entry| !entry.flags.generated)
                .find_map(|entry| {
                    let qualified_name = entry.qualified_name.as_ref()?;
                    if !Self::symbol_matches(qualified_name, &canonical) {
                        return None;
                    }
                    if qualified_lookup {
                        if crate::surface_path_name(qualified_name)
                            != crate::surface_path_name(&canonical)
                        {
                            return None;
                        }
                    } else if let Some(uid) = visible_uid {
                        if self.sigil_session.lookup_uid(qualified_name) != Some(uid) {
                            return None;
                        }
                    } else {
                        return None;
                    }
                    let signature = entry.signature.clone()?;
                    Some((qualified_name.clone(), signature))
                })
            {
                return Some(found);
            }
        }

        if let Some(found) = self
            .vm
            .function_entries()
            .iter()
            .rev()
            .filter(|entry| !entry.flags.generated)
            .find_map(|entry| {
                let qualified_name = entry.qualified_name.as_ref()?;
                if !self.function_entry_is_top_level_repl_surface(qualified_name) {
                    return None;
                }
                let surface_qualified = crate::surface_path_name(qualified_name);
                if qualified_lookup {
                    if surface_qualified != crate::surface_path_name(&canonical) {
                        return None;
                    }
                } else if surface_qualified
                    .rsplit("::")
                    .next()
                    .is_none_or(|tail| tail != crate::surface_path_name(&canonical))
                {
                    return None;
                }
                let signature = entry.signature.clone()?;
                Some((qualified_name.clone(), signature))
            })
        {
            return Some(found);
        }

        if let Some(entry) = self.signatures.iter().rev().find(|entry| {
            qualified_lookup
                && crate::surface_path_name(&entry.qualified_name)
                    == crate::surface_path_name(&canonical)
        }) {
            return Some((entry.qualified_name.clone(), entry.signature.clone()));
        }

        if let Some(entry) = self.signatures.iter().rev().find(|entry| {
            if !Self::symbol_matches(&entry.qualified_name, &canonical) {
                return false;
            }
            if qualified_lookup {
                crate::surface_path_name(&entry.qualified_name)
                    == crate::surface_path_name(&canonical)
            } else if let Some(uid) = visible_uid {
                self.sigil_session.lookup_uid(&entry.qualified_name) == Some(uid)
            } else {
                false
            }
        }) {
            return Some((entry.qualified_name.clone(), entry.signature.clone()));
        }

        None
    }

    fn render_signature_with_qualified_name(qualified_name: &str, signature: String) -> String {
        let qualified_name = crate::surface_path_name(qualified_name);
        let signature = crate::surface_rendered_name(&signature);
        if let Some((module, tail)) = qualified_name.rsplit_once("::") {
            if signature == tail
                || signature.starts_with(&format!("{tail}("))
                || signature.starts_with(&format!("{tail}<"))
            {
                return format!("{module}::{signature}");
            }
        }
        signature
    }

    fn handle_sig(&mut self, symbol: &str) -> ReplResult {
        let trimmed = symbol.trim();
        if trimmed.is_empty() {
            return Self::plain(Self::sig_help_lines());
        }
        if Self::facet_path_kind_info(trimmed).is_some() {
            return Self::plain(vec![
                format!("`{trimmed}` has no constructor signature."),
                "Facet path kinds are compiler-managed and cannot be used as values.".into(),
                format!("Use `:info {trimmed}`."),
            ]);
        }
        match parse_repl_query(trimmed) {
            Ok(ReplQuery::Symbol(symbol)) => {
                if let Some(result) = self.handle_sig_binding(symbol.source.as_str()) {
                    return result;
                }
                if self.is_attached_extractor_owner_query(&symbol) {
                    return self.handle_sig_typed_call(
                        trimmed,
                        &TypedCallQuery {
                            callee: symbol.source.clone(),
                            args: Vec::new(),
                        },
                    );
                }
                if let Some(message) = self.enum_sig_extra_input_message_for_symbol(&symbol) {
                    return Self::plain(vec![message]);
                }
                if let Some((owner, metadata)) =
                    self.process_metadata_for_owner(symbol.source.as_str())
                {
                    if let Some(lines) = self.process_owner_sig_summary_lines(owner, metadata) {
                        return Self::styled(lines);
                    }
                }
                if let Some(lines) = self.sig_type_owner_summary_lines(&symbol) {
                    return Self::styled(lines);
                }
                if let Some(lines) = self.builtin_literal_pattern_sig_lines(&symbol) {
                    return Self::styled(lines);
                }
                if let Some(lines) = self.trait_family_signature_lines(&symbol) {
                    return Self::styled(lines);
                }
                if let Some(lines) = self.operator_impl_signature_lines(&symbol) {
                    return Self::styled(lines);
                }
                if let Some(lines) = self.trait_method_family_signature_lines(&symbol) {
                    return Self::styled(lines);
                }
                match self
                    .find_signature(&symbol)
                    .or_else(|| self.concrete_process_alias_signature(&symbol))
                {
                    Some((qualified_name, signature)) => {
                        let rendered =
                            Self::render_signature_with_qualified_name(&qualified_name, signature);
                        Self::styled(vec![rendered])
                    }
                    None => {
                        if let Some(entry) = self.private_declaration(trimmed) {
                            return ReplResult::ok(Self::private_sig_output(entry));
                        }
                        Self::plain(vec![
                            format!("No signature found for {}", trimmed),
                            "Try `:doc <symbol>` for docs.".to_string(),
                        ])
                    }
                }
            }
            Ok(ReplQuery::FacetRootDoc(symbol)) => Self::plain(vec![format!(
                "`Facet.{}` has no constructor signature. Use `:doc Facet.{}`.",
                symbol.source, symbol.source
            )]),
            Ok(ReplQuery::FieldPath(symbol)) => Self::plain(vec![format!(
                "`{}` has no field signature. Use `:info {}` or `:facet {}`.",
                symbol.source,
                symbol.source.split('.').next().unwrap_or(&symbol.source),
                symbol.source
            )]),
            Ok(ReplQuery::TypedCall(query)) => self.handle_sig_typed_call(trimmed, &query),
            Ok(ReplQuery::OperatorTarget(query)) => self.handle_sig_typed_operator(trimmed, &query),
            Err(err) => self.repl_query_diagnostic(
                &format!(":sig {trimmed}"),
                err.message().to_string(),
                err.span(),
                Some("Accepted forms: symbol, typed call, or operator target.".to_string()),
            ),
        }
    }

    fn handle_type(&self, symbol: &str) -> ReplResult {
        let trimmed = symbol.trim();
        if trimmed.is_empty() {
            return Self::plain(Self::type_help_lines());
        }
        if !Self::is_type_lookup_symbol(trimmed) {
            return self.repl_command_diagnostic(
                &format!(":type {trimmed}"),
                format!("Invalid binding lookup target `{trimmed}`."),
                Span {
                    start: ":type ".chars().count(),
                    end: format!(":type {trimmed}").chars().count(),
                },
                Some("Usage: :type <binding|singleton-owner>".to_string()),
                Vec::new(),
            );
        }
        let binding_name = trimmed;

        let Some(binding) = self.binding_info(binding_name) else {
            if let Some((owner, metadata)) = self.process_metadata_for_singleton_owner(binding_name)
            {
                return Self::styled(self.process_owner_type_lines(owner, metadata));
            }
            return Self::plain(vec![format!("No binding found for {}", trimmed)]);
        };

        if binding.facet_info.is_some() {
            return Self::styled(vec![
                binding_name.to_string(),
                format!(
                    "type: {}",
                    crate::surface_rendered_name(binding.ty.as_str())
                ),
                format!("identity: {}", self.render_type_identity(binding, None)),
            ]);
        }

        let Some(value) = self.vm.get_local(binding.slot_id) else {
            return Self::plain(vec![format!("Binding `{}` has no current value.", trimmed)]);
        };

        let rendered_ty = Self::pid_type_from_value(&value)
            .unwrap_or_else(|| crate::surface_rendered_name(binding.ty.as_str()));

        Self::styled(vec![
            binding_name.to_string(),
            format!("type: {rendered_ty}"),
            format!(
                "identity: {}",
                self.render_type_identity(binding, Some(&value))
            ),
        ])
    }

    fn handle_info(&mut self, query: &str) -> ReplResult {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Self::plain(Self::info_help_lines());
        }
        match parse_repl_query(trimmed) {
            Ok(ReplQuery::Symbol(symbol)) => self.handle_info_symbol(trimmed, &symbol),
            Ok(ReplQuery::FacetRootDoc(symbol)) => self.handle_info_symbol(trimmed, &symbol),
            Ok(ReplQuery::FieldPath(symbol)) => Self::plain(vec![format!(
                "`{}` is a Facet path. Use `:facet {}`.",
                symbol.source, symbol.source
            )]),
            Ok(ReplQuery::TypedCall(query)) => self.handle_info_typed_call(trimmed, &query),
            Ok(ReplQuery::OperatorTarget(query)) => {
                self.handle_info_typed_operator(trimmed, &query)
            }
            Err(err) => self.repl_query_diagnostic(
                &format!(":info {trimmed}"),
                err.message().to_string(),
                err.span(),
                Some("Accepted forms: symbol, typed call, or operator target.".to_string()),
            ),
        }
    }

    fn render_facet_info(info: &forge::ReplFacetInfo) -> Vec<String> {
        let mut lines = vec![
            if info.operation.is_some() {
                "## FacetOperation".to_string()
            } else {
                "## FacetPath".to_string()
            },
            format!("type: {}", crate::surface_rendered_name(&info.ty)),
            format!(
                "stage: {}",
                match info.stage {
                    forge::ReplFacetStage::Template => "template",
                    forge::ReplFacetStage::Pending => "pending",
                }
            ),
            format!("kind: {}", info.path_kind),
            format!("source: {}", crate::surface_rendered_name(&info.source_ty)),
            format!("focus: {}", crate::surface_rendered_name(&info.focus_ty)),
            format!(
                "updated source: {}",
                crate::surface_rendered_name(&info.update_source_ty)
            ),
            format!(
                "replacement focus: {}",
                crate::surface_rendered_name(&info.update_focus_ty)
            ),
            format!(
                "full path: {}",
                crate::surface_rendered_name(&info.full_path)
            ),
            format!("facet root: {}", info.root_policy),
            format!(
                "availability in this REPL: {}",
                if info.available_in_current_scope {
                    "available"
                } else {
                    "unavailable (private path)"
                }
            ),
            "## API eligibility".to_string(),
            "## Flow".to_string(),
        ];
        if let Some(operation) = &info.operation {
            lines.insert(1, format!("operation: {}", operation.name));
            lines.insert(2, format!("kind constraint: {}", operation.kind_constraint));
            lines.insert(
                3,
                format!(
                    "result: {}",
                    crate::surface_rendered_name(&operation.result_ty)
                ),
            );
            if let Some(replacement) = &operation.replacement_ty {
                lines.insert(
                    4,
                    format!("replacement: {}", crate::surface_rendered_name(replacement)),
                );
            }
            if let Some(mapper) = &operation.mapper_ty {
                lines.insert(
                    5,
                    format!("mapper: {}", crate::surface_rendered_name(mapper)),
                );
            }
        }
        let flow_index = lines.len() - 1;
        lines.splice(flow_index..flow_index, info.api_eligibility.iter().cloned());

        let mut previous_terminal = None::<String>;
        for (index, segment) in info.segments.iter().enumerate() {
            let terminal = Self::facet_segment_terminal_name(&segment.label);
            let local_path =
                Self::render_facet_local_hop(segment, previous_terminal.as_deref(), &terminal);
            lines.push(format!("hop {}: {}", index + 1, local_path));
            lines.push(format!(
                "relation: {} -> {}",
                crate::surface_rendered_name(&segment.source_ty),
                crate::surface_rendered_name(&segment.focus_ty)
            ));
            lines.push(format!(
                "cumulative: {}",
                crate::surface_rendered_name(&segment.label)
            ));
            lines.push(format!(
                "fallible: {}",
                if segment.fallible { "yes" } else { "no" }
            ));
            lines.push(format!("reason: {}", segment.reason));
            lines.push(format!("policy: {}", segment.policy));
            previous_terminal = Some(terminal);
        }
        lines.push("## Stops".to_string());
        if info.stop_points.is_empty() {
            lines.push("none".to_string());
        } else {
            for (index, stop_point) in info.stop_points.iter().enumerate() {
                lines.push(format!("stop {}: {}", index + 1, stop_point));
            }
        }
        lines
    }

    fn facet_segment_terminal_name(label: &str) -> String {
        label.rsplit('.').next().unwrap_or(label).to_string()
    }

    fn render_facet_local_hop(
        segment: &forge::ReplFacetSegmentInfo,
        previous_terminal: Option<&str>,
        terminal: &str,
    ) -> String {
        if segment.label == format!("Tuple.{terminal}") {
            return segment.label.clone();
        }
        if segment.source_ty != "_" && !segment.source_ty.starts_with('(') {
            return format!("{}.{}", segment.source_ty, terminal);
        }
        if let Some(previous_terminal) = previous_terminal {
            return format!("{previous_terminal}.{terminal}");
        }
        segment.label.clone()
    }

    fn handle_facet(&mut self, query: &str) -> ReplResult {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Self::plain(Self::facet_help_lines());
        }
        if let Some(binding) = self.binding_info(trimmed) {
            if let Some(facet_info) = &binding.facet_info {
                return Self::styled(Self::render_facet_info(facet_info));
            }
        }
        self.handle_facet_expression(trimmed)
    }

    fn handle_facet_expression(&mut self, source_query: &str) -> ReplResult {
        let original_pending = self.pending.clone();
        let original_source = self
            .sources
            .source(self.repl_source_id)
            .unwrap_or("")
            .to_string();
        let query_source = format!("{source_query}\n");
        let sigil_cp = self.sigil_session.checkpoint();
        let scar_cp = self.scar_session.checkpoint();
        let forge_cp = self.forge_session.checkpoint();

        self.sources
            .update_source(self.repl_source_id, query_source.clone());

        let result = self.handle_facet_expression_inner(source_query, &query_source);

        self.sigil_session.rollback(sigil_cp);
        self.scar_session.rollback(scar_cp);
        self.forge_session.rollback(forge_cp);
        self.pending = original_pending;
        self.sources
            .update_source(self.repl_source_id, original_source);

        result
    }

    fn handle_facet_expression_inner(
        &mut self,
        source_query: &str,
        query_source: &str,
    ) -> ReplResult {
        let ast = match spire::parse_with_context(
            query_source,
            crate::derive_parser_context(
                self.repl_source_id.0,
                SourceKind::ReplChunk,
                CompileUnitKind::Repl,
                None,
            ),
        ) {
            Ok(ast) => ast,
            Err(e) => {
                let message = e.message();
                let spec = diagnostics::parse_error_spec(query_source, message, e.span().clone());
                let rendered = error_display::diagnostic_lines_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                return ReplResult::ok(ReplOutput::EvalError {
                    idx: self.results.len(),
                    source: format!(":facet {source_query}"),
                    rendered,
                });
            }
        };

        let resolved = match self.sigil_session.resolve(ast.clone()) {
            Ok(r) => r,
            Err(e) => {
                let spec =
                    diagnostics::simple_error("ResolveError", &e.message, e.span.clone(), None);
                let rendered = error_display::diagnostic_lines_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                return ReplResult::ok(ReplOutput::EvalError {
                    idx: self.results.len(),
                    source: format!(":facet {source_query}"),
                    rendered,
                });
            }
        };

        let mut context = Self::typecheck_context_for_source(SourceKind::ReplChunk);
        context.allow_private_facet_inspection = true;
        let typed = match self.scar_session.typecheck_with_context(resolved, context) {
            Ok(t) => t,
            Err(e) => {
                let error = diagnostics::TypeErrorDiagnostic::new(e.message, e.span, e.hint);
                let spec =
                    diagnostics::type_error_spec_by_id(&self.sources, self.repl_source_id, &error);
                let rendered = error_display::diagnostic_lines_by_id(
                    &self.sources,
                    self.repl_source_id,
                    &spec,
                    self.error_display_mode,
                );
                return ReplResult::ok(ReplOutput::EvalError {
                    idx: self.results.len(),
                    source: format!(":facet {source_query}"),
                    rendered,
                });
            }
        };

        let Some(root) = typed.first() else {
            return Self::plain(Self::facet_help_lines());
        };
        let Some(info) = forge::repl_facet_info_for_node(root) else {
            return Self::plain(vec![format!(
                "`{source_query}` is not a FacetPath binding or expression."
            )]);
        };

        Self::styled(Self::render_facet_info(&info))
    }

    fn handle_info_symbol(&self, source_query: &str, symbol: &str) -> ReplResult {
        if let Some(lines) = Self::facet_path_kind_info(symbol) {
            return Self::styled(lines);
        }
        if let Some(lines) = self.type_definition_info_lines(symbol) {
            return Self::styled(lines);
        }
        if let Some(binding) = self.binding_info(symbol) {
            return self.render_info_binding(symbol, binding);
        }

        if let Some((owner, metadata)) = self.process_metadata_for_singleton_owner(symbol) {
            return Self::styled(self.process_owner_info_lines(owner, metadata));
        }

        if let Some((qualified_name, signature)) = self.find_signature(symbol) {
            let kind = self
                .visible_declaration(symbol)
                .map(|decl| match decl.kind {
                    sigil::DeclarationKind::Enum => "enum",
                    sigil::DeclarationKind::Struct
                    | sigil::DeclarationKind::Record
                    | sigil::DeclarationKind::BuiltinType
                    | sigil::DeclarationKind::Trait => "type",
                    _ => "function",
                })
                .unwrap_or("function");
            return Self::styled(vec![
                qualified_name.clone(),
                format!("kind: {kind}"),
                format!("origin: {}", Self::origin_for_name(&qualified_name)),
                format!(
                    "defined: {}",
                    Self::render_signature_with_qualified_name(&qualified_name, signature)
                ),
            ]);
        }

        Self::plain(vec![format!("No signature found for {}", source_query)])
    }

    fn type_definition_info_lines(&self, symbol: &str) -> Option<Vec<String>> {
        let decl = self.visible_declaration(symbol)?;
        let kind = match decl.kind {
            sigil::DeclarationKind::Struct => "struct",
            sigil::DeclarationKind::Record => "record",
            _ => return None,
        };
        let def = self
            .scar_session
            .lookup_type_def(&decl.fq_name)
            .or_else(|| self.scar_session.lookup_type_def(symbol))?;
        let mut rows = def
            .fields
            .iter()
            .map(|(field, ty)| {
                let mut policy = if def.private_fields.contains(field) {
                    "private".to_string()
                } else {
                    "public".to_string()
                };
                if def.readonly_fields.contains(field) {
                    policy.push_str(" readonly");
                }
                (policy, field.clone(), Self::ty_to_string(ty))
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.1.cmp(&right.1));
        let policy_width = rows.iter().map(|row| row.0.len()).max().unwrap_or(0);
        let field_width = rows.iter().map(|row| row.1.len()).max().unwrap_or(0);
        let mut lines = vec![
            crate::surface_path_name(&decl.fq_name).to_string(),
            format!("kind: {kind}"),
            format!(
                "facet root: {}",
                if def.readonly_root {
                    "readonly"
                } else {
                    "public"
                }
            ),
            "fields:".to_string(),
        ];
        lines.extend(rows.into_iter().map(|(policy, field, ty)| {
            format!("- {policy:policy_width$} {field:field_width$}: {ty}")
        }));
        Some(lines)
    }

    fn facet_path_kind_info(symbol: &str) -> Option<Vec<String>> {
        let name = crate::surface_path_name(symbol).rsplit("::").next()?;
        let atomic = ["InfallibleStructural", "FallibleStructural", "VariantPath"];
        let aliases: &[(&str, &[&str])] = &[
            ("ReadablePath", &atomic),
            ("WritablePath", &atomic),
            ("PutPath", &["InfallibleStructural"]),
            ("PreviewPath", &["VariantPath"]),
            ("CasePath", &["VariantPath"]),
        ];
        if name == "FacetPathKind" {
            let mut lines = vec![
                "FacetPathKind".into(),
                "Compiler-managed types usable only as the first argument of Facet<K, S, A, T, B>."
                    .into(),
                "Atomic kinds".into(),
            ];
            lines.extend(atomic.iter().map(|kind| format!("- {kind}")));
            lines.push("Kind-set aliases".into());
            lines.extend(aliases.iter().map(|(alias, _)| format!("- {alias}")));
            return Some(lines);
        }
        if atomic.contains(&name) {
            return Some(vec![
                format!("@FacetPathKind Type {name}"),
                "Compiler-derived atomic Facet path kind.".into(),
                "Usage: first type argument of Facet<K, S, A, T, B> only.".into(),
            ]);
        }
        aliases
            .iter()
            .find(|(alias, _)| *alias == name)
            .map(|(alias, members)| {
                let rhs = members.join(" | ");
                let mut lines = vec![
                    format!("@FacetPathKind Type {alias} ="),
                    format!("  {rhs}"),
                    "Direct members".into(),
                ];
                lines.extend(members.iter().map(|member| format!("- {member}")));
                lines.push("Effective atomic kinds".into());
                lines.extend(members.iter().map(|member| format!("- {member}")));
                lines.push("Usage: first type argument of Facet<K, S, A, T, B> only.".into());
                lines.push("Compiler-managed; not a runtime value or constructor.".into());
                lines
            })
    }

    fn render_info_binding(&self, symbol: &str, binding: &forge::BindingInfo) -> ReplResult {
        if let Some(value) = self.vm.get_local(binding.slot_id) {
            if let Some((process_name, metadata)) = self.process_metadata_for_pid_value(&value) {
                let rendered_ty = Self::pid_type_from_value(&value)
                    .unwrap_or_else(|| crate::surface_rendered_name(&binding.ty));
                return Self::styled(vec![
                    symbol.to_string(),
                    "kind: process pid".to_string(),
                    "origin: repl".to_string(),
                    format!("type: {rendered_ty}"),
                    "identity: TypeIdentity::Type".to_string(),
                    format!(
                        "defined: PID<{}>",
                        crate::surface_rendered_name(process_name)
                    ),
                    format!("instance: {:?}", metadata.instance),
                    format!("runtime kind: {:?}", metadata.kind),
                ]);
            }
        }

        if let Some((_, metadata)) = self.process_metadata_for_pid_type(binding.ty.as_str()) {
            return Self::styled(vec![
                symbol.to_string(),
                "kind: process pid".to_string(),
                "origin: repl".to_string(),
                format!("type: {}", crate::surface_rendered_name(&binding.ty)),
                "identity: TypeIdentity::Type".to_string(),
                format!("defined: {}", crate::surface_rendered_name(&binding.ty)),
                format!("instance: {:?}", metadata.instance),
                format!("runtime kind: {:?}", metadata.kind),
            ]);
        }

        let mut lines = vec![symbol.to_string()];
        let kind = match self.binding_callable_kind(binding) {
            Some(forge::ReplCallableKind::Closure) => "closure",
            Some(forge::ReplCallableKind::Capture) => "capture",
            None => "binding",
        };
        lines.push(format!("kind: {kind}"));
        lines.push("origin: repl".to_string());
        if binding.facet_info.is_some() {
            lines.push(format!(
                "type: {}",
                crate::surface_rendered_name(&binding.ty)
            ));
            lines.push(format!(
                "identity: {}",
                self.render_type_identity(binding, None)
            ));
            if let Some(facet_info) = &binding.facet_info {
                lines.push(format!(
                    "full path: {}",
                    crate::surface_rendered_name(&facet_info.full_path)
                ));
            }
        } else if let Some(value) = self.vm.get_local(binding.slot_id) {
            lines.push(format!(
                "type: {}",
                crate::surface_rendered_name(&binding.ty)
            ));
            lines.push(format!(
                "identity: {}",
                self.render_type_identity(binding, Some(&value))
            ));
        }
        if let Some(sig) = self.binding_callable_sig_summary(symbol) {
            lines.push(format!("defined: {sig}"));
        }
        Self::styled(lines)
    }

    fn handle_info_typed_call(&mut self, source_query: &str, query: &TypedCallQuery) -> ReplResult {
        let sig = self.handle_sig_typed_call(source_query, query);
        let sig_text = Self::repl_result_text(&sig);
        match sig.output {
            ReplOutput::EvalError { rendered, .. } => ReplResult::ok(ReplOutput::EvalError {
                idx: self.results.len(),
                source: format!(":info {source_query}"),
                rendered,
            }),
            _ => {
                let mut lines = vec![source_query.to_string(), "kind: function".to_string()];
                lines.push(format!("origin: {}", Self::origin_for_name(&query.callee)));
                for block in sig_text.split("\n\n") {
                    if let Some(rest) = block.strip_prefix("defined:\n  ") {
                        lines.push(format!("defined: {rest}"));
                    } else if let Some(rest) = block.strip_prefix("specialized:\n  ") {
                        lines.push(format!("specialized: {rest}"));
                    } else {
                        lines.push(format!("defined: {block}"));
                    }
                }
                Self::styled(lines)
            }
        }
    }

    fn handle_info_typed_operator(
        &mut self,
        source_query: &str,
        query: &OperatorTargetQuery,
    ) -> ReplResult {
        if Self::operator_target_symbol(query).is_some() {
            return self
                .operator_target_signature_entry(query)
                .map(|(qualified_name, signature)| {
                    Self::styled(vec![
                        source_query.to_string(),
                        "kind: operator".to_string(),
                        format!("origin: {}", Self::origin_for_name(&qualified_name)),
                        format!(
                            "defined: {}",
                            Self::render_signature_with_qualified_name(&qualified_name, signature)
                        ),
                    ])
                })
                .unwrap_or_else(|| {
                    Self::plain(vec![format!("No signature found for {source_query}")])
                });
        }
        match self.typed_operator_signature(query) {
            Ok((defined, result_ty)) => Self::styled(vec![
                source_query.to_string(),
                "kind: operator".to_string(),
                format!("origin: {}", Self::origin_for_name(query.operator)),
                format!("defined: {defined}"),
                format!(
                    "specialized: {source_query}: {}",
                    format_query_ty(&result_ty)
                ),
                format!("type: {}", format_query_ty(&result_ty)),
            ]),
            Err(message) => self.repl_query_diagnostic(
                &format!(":info {source_query}"),
                message,
                Span {
                    start: ":info ".chars().count(),
                    end: format!(":info {source_query}").chars().count(),
                },
                None,
            ),
        }
    }

    fn origin_for_name(name: &str) -> &'static str {
        if name.contains("REPL::") {
            "repl"
        } else if builtin_function_metas()
            .iter()
            .any(|meta| name == meta.name || name.ends_with(&format!("::{}", meta.name)))
        {
            "builtin"
        } else {
            "stdlib"
        }
    }

    fn repl_result_text(result: &ReplResult) -> String {
        match &result.output {
            ReplOutput::StyledDoc { lines } | ReplOutput::PlainText { lines } => lines.join("\n"),
            ReplOutput::Diagnostic {
                rendered,
                summary_tail,
            } => rendered
                .iter()
                .chain(summary_tail.iter())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
            ReplOutput::EvalSuccess { rendered, .. } | ReplOutput::EvalError { rendered, .. } => {
                rendered.join("\n")
            }
            ReplOutput::DocResolved {
                symbol,
                signature,
                summary,
                source_snippet,
                details,
            } => [
                vec![symbol.clone()],
                signature.clone().into_iter().collect(),
                summary.clone().into_iter().collect(),
                source_snippet.clone().into_iter().collect(),
                details.clone(),
            ]
            .concat()
            .join("\n"),
            ReplOutput::StatusMessage(message) => message.clone(),
            ReplOutput::EvalStarted { source, .. } => source.clone(),
        }
    }

    fn ty_to_string(ty: &Ty) -> String {
        match ty {
            Ty::Int => "Int".into(),
            Ty::Float => "Float".into(),
            Ty::Str => "String".into(),
            Ty::Bool => "Boolean".into(),
            Ty::Unit => "Unit".into(),
            Ty::Hole => "_".into(),
            Ty::List(inner) => format!("List<{}>", Self::ty_to_string(inner)),
            Ty::Lazy(inner) => format!("Lazy<{}>", Self::ty_to_string(inner)),
            Ty::Pid(name) => format!("PID<{}>", crate::surface_rendered_name(name)),
            Ty::Facet(kind, source, focus, update_source, update_focus) => {
                format!(
                    "Facet<{}, {}, {}, {}, {}>",
                    kind.as_str(),
                    Self::ty_to_string(source),
                    Self::ty_to_string(focus),
                    Self::ty_to_string(update_source),
                    Self::ty_to_string(update_focus)
                )
            }
            Ty::Tuple(items) => format!(
                "({})",
                items
                    .iter()
                    .map(Self::ty_to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::SelfApp(args) => format!(
                "Self<{}>",
                args.iter()
                    .map(Self::ty_to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Result(ok, err) => {
                format!(
                    "Result<{}, {}>",
                    Self::ty_to_string(ok),
                    Self::ty_to_string(err)
                )
            }
            Ty::Struct(name, _) | Ty::Record(name, _) => crate::surface_path_name(name).to_string(),
            Ty::Enum(name, args) => {
                let name = crate::surface_path_name(name);
                if args.is_empty() {
                    name.to_string()
                } else {
                    let args = args
                        .iter()
                        .map(Self::ty_to_string)
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{name}<{args}>")
                }
            }
            Ty::Error => "Error".into(),
            Ty::Var(_) => "_".into(),
            Ty::Func(params, ret) => {
                let param_str = params
                    .iter()
                    .map(Self::ty_to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                if param_str.is_empty() {
                    format!("(-> {})", Self::ty_to_string(ret))
                } else {
                    format!("({} -> {})", param_str, Self::ty_to_string(ret))
                }
            }
            Ty::BuiltinFunc { name, .. } => format!("Builtin({})", name),
            Ty::UserFunc { .. } => "UserFunc".into(),
        }
    }

    fn handle_sig_typed_call(&mut self, source_query: &str, query: &TypedCallQuery) -> ReplResult {
        if let Some(message) = self.invalid_attached_extractor_query_message(query) {
            return Self::plain(vec![message]);
        }
        if let Some(message) = self.enum_sig_extra_input_message_for_typed_call(query) {
            return Self::plain(vec![message]);
        }
        if let Some(lines) = self.sig_zero_arg_type_owner_fallback(query) {
            return Self::styled(lines);
        }
        if let Some(binding) = self.binding_info(&query.callee) {
            if let Some(rendered) = self.handle_sig_binding_typed_call(source_query, query, binding)
            {
                return rendered;
            }
        }
        if let Some(lines) = self.trait_target_signature_lines(query) {
            return Self::styled(lines);
        }
        let matches = self.match_typed_call_signatures(query);
        match matches.as_slice() {
            [entry] => {
                let defined = Self::render_signature_with_qualified_name(
                    &entry.qualified_name,
                    entry.signature.clone(),
                );
                let arg_types = match self.query_arg_ast_types(query.args.as_slice()) {
                    Ok(arg_types) => arg_types,
                    Err(message) => {
                        return Self::plain(vec![message]);
                    }
                };
                let rendered = if query.callee == "dbg!" {
                    defined
                } else {
                    let specialized_return = self
                        .specialize_signature_return(&defined, &arg_types)
                        .map(|ty| format_query_ty(&ty))
                        .map(|ret| {
                            if ret == "Self" {
                                arg_types.first().map(format_query_ty).unwrap_or(ret)
                            } else {
                                ret
                            }
                        })
                        .or_else(|| {
                            signature_return_type(&defined)
                                .filter(|ret| *ret == "Self")
                                .and_then(|_| arg_types.first().map(format_query_ty))
                        })
                        .unwrap_or_else(|| {
                            signature_return_type(&defined).unwrap_or("_").to_string()
                        });
                    format!(
                        "defined:\n  {defined}\n\nspecialized:\n  {}({}) -> {}",
                        query.callee,
                        arg_types
                            .iter()
                            .map(format_query_ty)
                            .collect::<Vec<_>>()
                            .join(", "),
                        specialized_return
                    )
                };
                Self::styled(rendered.lines().map(|line| line.to_string()).collect())
            }
            [] => self
                .private_declaration(query.callee.strip_suffix('!').unwrap_or(&query.callee))
                .map(|entry| ReplResult::ok(Self::private_sig_output(entry)))
                .unwrap_or_else(|| {
                    Self::plain(vec![format!("No signature found for {}", source_query)])
                }),
            entries => Self::plain(Self::ambiguous_signature_lines(source_query, entries)),
        }
    }

    fn sig_type_owner_summary_lines(&self, symbol: &str) -> Option<Vec<String>> {
        let decl = self.visible_declaration(symbol)?;
        match decl.kind {
            sigil::DeclarationKind::Struct | sigil::DeclarationKind::Record => {
                self.constructor_signature_lines(decl)
            }
            sigil::DeclarationKind::Enum => self.enum_variant_signature_lines(decl),
            _ => None,
        }
    }

    fn builtin_literal_pattern_sig_lines(&self, symbol: &str) -> Option<Vec<String>> {
        Some(vec![match symbol {
            "List" => "[T1, ..] : List<T>".to_string(),
            "HashMap" => "hash![String => V1, ..] : HashMap<V>".to_string(),
            "Tuple" => "(T1, T2, ..) : Tuple<T1, T2, ..>".to_string(),
            _ => return None,
        }])
    }

    fn trait_family_signature_lines(&self, symbol: &str) -> Option<Vec<String>> {
        let decl = self.visible_declaration(symbol)?;
        if decl.kind != sigil::DeclarationKind::Trait {
            return None;
        }
        let (_, signature) = self.trait_signature_entry(symbol)?;
        let methods = Self::trait_signature_methods(&signature);
        if methods.is_empty() {
            return Some(vec![crate::surface_rendered_name(&signature)]);
        }

        let mut lines = Vec::with_capacity(methods.len() + 3);
        lines.push(format!(
            "trait {} {{",
            crate::surface_path_name(&decl.fq_name)
        ));
        lines.extend(methods.into_iter().map(|method| format!("  {method}")));
        lines.push("}".to_string());
        if let Some(targets) = self.impl_targets_summary_for_trait(symbol) {
            lines.push(format!("impl targets: {targets}"));
        }
        Some(lines)
    }

    fn trait_method_family_signature_lines(&self, symbol: &str) -> Option<Vec<String>> {
        let (trait_name, member) = match symbol {
            "compare" => ("Compare", "compare"),
            "|>" => ("PipeApply", "pipe_apply"),
            "|*>" => ("Functor", "fmap"),
            _ => return None,
        };
        let (_, signature) = self.trait_signature_entry(trait_name)?;
        let method = Self::trait_signature_methods(&signature)
            .into_iter()
            .find(|method| {
                Self::callee_tail(method.split_once('(').map_or(method, |(head, _)| head)) == member
            })?;
        let mut lines = vec![format!("{trait_name}::{method}")];
        if let Some(targets) = self.impl_targets_summary_for_trait(trait_name) {
            lines.push(format!("impl targets: {targets}"));
        }
        lines.push(format!("try: :sig {symbol}(Int, Int)"));
        Some(lines)
    }

    fn operator_impl_signature_lines(&self, symbol: &str) -> Option<Vec<String>> {
        if symbol != "|>=" {
            return None;
        }
        let trait_name = Self::operator_target_trait_name(symbol)?;
        let member = Self::operator_target_member_name(symbol)?;
        let mut lines = self
            .signatures
            .iter()
            .filter(|entry| entry.kind == DocKind::Function)
            .filter(|entry| {
                let signature = entry.signature.as_str();
                signature.starts_with(&format!("impl {trait_name}"))
                    && signature.contains(&format!("::{member}("))
            })
            .map(|entry| crate::surface_rendered_name(&entry.signature))
            .collect::<Vec<_>>();
        lines.sort();
        (!lines.is_empty()).then_some(lines)
    }

    fn trait_target_signature_lines(&self, query: &TypedCallQuery) -> Option<Vec<String>> {
        let decl = self.visible_declaration(&query.callee)?;
        if decl.kind != sigil::DeclarationKind::Trait {
            return None;
        }
        let (_, signature) = self.trait_signature_entry(&query.callee)?;
        let mut lines = Vec::new();
        for method in Self::trait_signature_methods(&signature) {
            let Some((method_name, _)) = method.split_once('(') else {
                continue;
            };
            let method_query = TypedCallQuery {
                callee: method_name.trim().to_string(),
                args: query.args.clone(),
            };
            if let Some(entry) = self
                .match_typed_call_signatures(&method_query)
                .into_iter()
                .next()
            {
                lines.push(Self::render_signature_with_qualified_name(
                    &entry.qualified_name,
                    entry.signature.clone(),
                ));
            }
        }
        (!lines.is_empty()).then_some(lines)
    }

    fn trait_signature_entry(&self, trait_name: &str) -> Option<(String, String)> {
        self.find_signature(trait_name)
            .filter(|(_, signature)| Self::trait_signature_methods(signature).len() > 0)
    }

    fn trait_signature_methods(signature: &str) -> Vec<String> {
        let Some(open) = signature.find('{') else {
            return Vec::new();
        };
        let Some(close) = signature.rfind('}') else {
            return Vec::new();
        };
        let head = signature[..open].trim();
        if !head.starts_with("trait ") && !head.starts_with("deftrait ") {
            return Vec::new();
        }
        Self::split_top_level_items(&signature[open + 1..close])
            .into_iter()
            .map(|method| crate::surface_rendered_name(&method))
            .collect()
    }

    fn impl_targets_summary_for_trait(&self, trait_name: &str) -> Option<String> {
        let mut targets = Vec::new();
        for signature in self.signatures.iter().map(|entry| entry.signature.as_str()) {
            if let Some(target) = Self::impl_signature_target_for_trait(signature, trait_name) {
                let target = Self::impl_target_summary_name(target);
                if !targets.iter().any(|seen| seen == &target) {
                    targets.push(target);
                }
            }
        }
        if targets
            .iter()
            .any(|target| Self::is_tuple_summary_name(target))
        {
            let mut collapsed = vec!["TupleN".to_string()];
            collapsed.extend(
                targets
                    .into_iter()
                    .filter(|target| !Self::is_tuple_summary_name(target)),
            );
            targets = collapsed;
        }
        (!targets.is_empty()).then(|| targets.join(", "))
    }

    fn impl_signature_target_for_trait<'a>(
        signature: &'a str,
        trait_name: &str,
    ) -> Option<&'a str> {
        let rest = signature.strip_prefix("impl ")?;
        let for_pos = rest.find(" for ")?;
        let trait_head = rest[..for_pos].trim();
        let trait_head_name = trait_head.split('<').next().unwrap_or(trait_head).trim();
        if trait_head_name != trait_name {
            return None;
        }
        let target_and_member = rest[for_pos + " for ".len()..].trim();
        target_and_member
            .split_once("::")
            .map(|(target, _)| target.trim())
    }

    fn impl_target_summary_name(target: &str) -> String {
        let target = crate::surface_rendered_name(target);
        if target.starts_with('(') && target.ends_with(')') {
            let inner = &target[1..target.len() - 1];
            let arity = Self::split_top_level_items(inner).len();
            if arity > 0 {
                return format!("Tuple{arity}");
            }
        }
        target
            .split('<')
            .next()
            .unwrap_or(target.as_str())
            .trim()
            .to_string()
    }

    fn is_tuple_summary_name(target: &str) -> bool {
        target
            .strip_prefix("Tuple")
            .is_some_and(|tail| !tail.is_empty() && tail.chars().all(|ch| ch.is_ascii_digit()))
    }

    fn enum_sig_extra_input_message_for_symbol(&self, symbol: &str) -> Option<String> {
        let (owner, _) = symbol.split_once("::")?;
        self.enum_sig_extra_input_message(owner.trim(), symbol.trim())
    }

    fn enum_sig_extra_input_message_for_typed_call(
        &self,
        query: &TypedCallQuery,
    ) -> Option<String> {
        let callee = query.callee.trim();
        if let Some((owner, _)) = callee.split_once("::") {
            return self.enum_sig_extra_input_message(owner.trim(), callee);
        }
        if query.args.is_empty() {
            return None;
        }
        self.enum_sig_extra_input_message(callee, callee)
    }

    fn enum_sig_extra_input_message(&self, owner: &str, source: &str) -> Option<String> {
        let decl = self.visible_declaration(owner)?;
        (decl.kind == sigil::DeclarationKind::Enum
            && !matches!(
                crate::surface_path_name(&decl.fq_name),
                "Result" | "Boolean"
            ))
        .then(|| {
            format!(
                "Enum signatures are only available for bare type owners: use `:sig {}` instead of `:sig {}`.",
                crate::surface_path_name(&decl.fq_name),
                source
            )
        })
    }

    fn sig_zero_arg_type_owner_fallback(&self, query: &TypedCallQuery) -> Option<Vec<String>> {
        if !query.args.is_empty() {
            return None;
        }
        let decl = self.visible_declaration(&query.callee)?;
        match decl.kind {
            sigil::DeclarationKind::Struct | sigil::DeclarationKind::Record => {
                self.constructor_signature_lines(decl)
            }
            _ => None,
        }
    }

    fn constructor_signature_lines(&self, decl: &sigil::DeclarationEntry) -> Option<Vec<String>> {
        if let Some((qualified_name, signature)) = self.constructor_signature_entry(decl) {
            return Some(vec![Self::render_signature_with_qualified_name(
                &qualified_name,
                signature,
            )]);
        }

        None
    }

    fn constructor_signature_entry(
        &self,
        decl: &sigil::DeclarationEntry,
    ) -> Option<(String, String)> {
        let constructor_qualified_name = format!("{}::new", decl.fq_name);
        if let Some(signature) = self.find_signature(&constructor_qualified_name) {
            return Some(signature);
        }

        let mut matches = self
            .signatures
            .iter()
            .filter(|entry| entry.kind == DocKind::Function)
            .filter(|entry| {
                crate::surface_path_name(&entry.qualified_name)
                    == crate::surface_path_name(&constructor_qualified_name)
            })
            .map(|entry| (entry.qualified_name.clone(), entry.signature.clone()))
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        if let Some(first) = matches.into_iter().next() {
            return Some(first);
        }

        self.scar_session
            .lookup_type_def(&decl.fq_name)
            .filter(|def| {
                matches!(
                    def.kind,
                    scar::env::TypeKind::Struct | scar::env::TypeKind::Record
                )
            })
            .map(|def| {
                let params = def
                    .fields
                    .iter()
                    .map(|(name, ty)| format!("{name}: {}", Self::ty_to_string(ty)))
                    .collect::<Vec<_>>()
                    .join(", ");
                match def.kind {
                    scar::env::TypeKind::Record => (
                        decl.fq_name.clone(),
                        format!(
                            "{}({params}) -> {}",
                            crate::surface_path_name(&decl.fq_name),
                            crate::surface_path_name(&decl.fq_name)
                        ),
                    ),
                    scar::env::TypeKind::Struct => (
                        constructor_qualified_name,
                        format!(
                            "{}::new({params}) -> {}",
                            crate::surface_path_name(&decl.fq_name),
                            crate::surface_path_name(&decl.fq_name)
                        ),
                    ),
                    _ => unreachable!("constructor signatures only support struct/record"),
                }
            })
    }

    fn enum_variant_signature_lines(&self, decl: &sigil::DeclarationEntry) -> Option<Vec<String>> {
        let entry = self.signatures.iter().rev().find(|entry| {
            entry.kind == DocKind::Type
                && crate::surface_path_name(&entry.qualified_name)
                    == crate::surface_path_name(&decl.fq_name)
        });
        if let Some(entry) = entry {
            if let Some((_owner_surface, variants_src)) =
                Self::parse_defenum_signature(&entry.signature)
            {
                let variants = Self::split_top_level_items(variants_src)
                    .into_iter()
                    .map(|variant| Self::render_enum_variant_listing(&decl.fq_name, &variant))
                    .collect::<Vec<_>>();
                if !variants.is_empty() {
                    return Some(variants);
                }
            }
        }

        self.scar_session
            .enum_variants_of(&decl.fq_name)
            .and_then(|variants| {
                let lines = variants
                    .iter()
                    .map(|variant| {
                        if variant.payload.is_empty() {
                            format!(
                                "* {}::{}",
                                crate::surface_path_name(&decl.fq_name),
                                variant.short_name
                            )
                        } else {
                            let payload = variant
                                .payload
                                .iter()
                                .map(Self::ty_to_string)
                                .collect::<Vec<_>>()
                                .join(", ");
                            format!(
                                "* {}::{}({payload})",
                                crate::surface_path_name(&decl.fq_name),
                                variant.short_name
                            )
                        }
                    })
                    .collect::<Vec<_>>();
                (!lines.is_empty()).then_some(lines)
            })
    }

    fn parse_defenum_signature(signature: &str) -> Option<(String, &str)> {
        let rest = signature.strip_prefix("defenum ")?;
        let open = rest.find('{')?;
        let close = rest.rfind('}')?;
        let owner = rest[..open].trim().to_string();
        let variants = rest[open + 1..close].trim();
        Some((owner, variants))
    }

    fn split_top_level_items(input: &str) -> Vec<String> {
        let mut items = Vec::new();
        let mut current = String::new();
        let mut depth = 0usize;
        for ch in input.chars() {
            match ch {
                '<' | '(' | '[' | '{' => {
                    depth += 1;
                    current.push(ch);
                }
                '>' | ')' | ']' | '}' => {
                    depth = depth.saturating_sub(1);
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    let item = current.trim();
                    if !item.is_empty() {
                        items.push(item.to_string());
                    }
                    current.clear();
                }
                _ => current.push(ch),
            }
        }
        let tail = current.trim();
        if !tail.is_empty() {
            items.push(tail.to_string());
        }
        items
    }

    fn render_enum_variant_listing(owner_fq: &str, variant: &str) -> String {
        let owner_fq = crate::surface_path_name(owner_fq);
        if let Some((name, payload)) = variant.split_once('(') {
            let payload = crate::surface_rendered_name(payload.trim_end_matches(')').trim());
            format!("* {}::{}({payload})", owner_fq, name.trim())
        } else {
            format!("* {}::{}", owner_fq, variant.trim())
        }
    }

    fn handle_sig_typed_operator(
        &mut self,
        source_query: &str,
        query: &OperatorTargetQuery,
    ) -> ReplResult {
        if Self::operator_target_symbol(query).is_some() {
            return match self.operator_target_signature_entry(query) {
                Some((qualified_name, signature)) => {
                    Self::styled(vec![Self::render_signature_with_qualified_name(
                        &qualified_name,
                        signature,
                    )])
                }
                None => Self::plain(vec![format!("No signature found for {source_query}")]),
            };
        }
        match self.typed_operator_signature(query) {
            Ok((defined, result_ty)) => Self::styled(
                format!(
                    "defined:\n  {defined}\n\nspecialized:\n  {source_query}: {}",
                    format_query_ty(&result_ty)
                )
                .lines()
                .map(|line| line.to_string())
                .collect(),
            ),
            Err(message) => Self::plain(vec![message]),
        }
    }

    fn query_arg_type(&self, arg: &QueryArg) -> Result<String, String> {
        self.query_arg_ast_ty(arg).map(|ty| format_query_ty(&ty))
    }

    fn handle_sig_binding(&self, symbol: &str) -> Option<ReplResult> {
        let binding = self.binding_info(symbol)?;
        if let Some(value) = self.vm.get_local(binding.slot_id) {
            if let Some((owner, metadata)) = self.process_metadata_for_pid_value(&value) {
                if let Some(lines) = self.process_pid_binding_sig_summary_lines(owner, metadata) {
                    return Some(Self::styled(lines));
                }
            }
        }
        if let Some((owner, metadata)) = self.process_metadata_for_pid_type(binding.ty.as_str()) {
            if let Some(lines) = self.process_pid_binding_sig_summary_lines(owner, metadata) {
                return Some(Self::styled(lines));
            }
        }
        self.binding_callable_sig_summary(symbol)
            .map(|rendered| Self::styled(vec![rendered]))
            .or_else(|| {
                Some(Self::plain(vec![format!(
                    "No signature found for {}",
                    symbol
                )]))
            })
    }

    fn binding_callable_sig_summary(&self, symbol: &str) -> Option<String> {
        let binding = self.binding_info(symbol)?;
        let kind = self.binding_callable_kind(binding)?;
        let ty = self.binding_callable_ty(binding)?;
        Some(Self::render_callable_sig_summary(
            symbol,
            &format_query_ty(&ty),
            kind,
        ))
    }

    fn handle_sig_binding_typed_call(
        &self,
        source_query: &str,
        query: &TypedCallQuery,
        binding: &forge::BindingInfo,
    ) -> Option<ReplResult> {
        let Some(func_ty) = self.binding_callable_ty(binding) else {
            return None;
        };
        let AstTy::Func(_, params, ret) = func_ty else {
            return None;
        };

        let arg_types = match self.query_arg_ast_types(query.args.as_slice()) {
            Ok(arg_types) => arg_types,
            Err(message) => {
                return Some(Self::plain(vec![message]));
            }
        };

        if arg_types.len() != params.len() {
            return Some(self.sig_callable_arity_error(
                source_query,
                params.len(),
                arg_types.len(),
                &AstTy::Func(Span { start: 0, end: 0 }, params, ret),
            ));
        }

        Some(Self::styled(vec![
            Self::render_callable_application_summary(&query.callee, &arg_types, ret.as_ref()),
        ]))
    }

    fn render_callable_sig_summary(
        name_or_source: &str,
        ty: &str,
        kind: forge::ReplCallableKind,
    ) -> String {
        let kind = match kind {
            forge::ReplCallableKind::Closure => "Closure",
            forge::ReplCallableKind::Capture => "Capture",
        };
        format!("{name_or_source}: {ty} :: {kind}")
    }

    fn render_callable_application_summary(
        callee: &str,
        arg_types: &[AstTy],
        return_ty: &AstTy,
    ) -> String {
        let args = arg_types
            .iter()
            .map(format_query_ty)
            .collect::<Vec<_>>()
            .join(", ");
        format!("{callee}({args}) -> {}", format_query_ty(return_ty))
    }

    fn binding_callable_kind(
        &self,
        binding: &forge::BindingInfo,
    ) -> Option<forge::ReplCallableKind> {
        if let Some(kind) = binding.callable_kind {
            return Some(kind);
        }
        let value = self.vm.get_local(binding.slot_id)?;
        match value {
            Value::Callable(callable) => match callable.metadata.origin {
                sindr::runtime::CallableOrigin::Closure => Some(forge::ReplCallableKind::Closure),
                sindr::runtime::CallableOrigin::Capture => Some(forge::ReplCallableKind::Capture),
                sindr::runtime::CallableOrigin::Unknown => None,
            },
            _ => None,
        }
    }

    fn binding_callable_ty(&self, binding: &forge::BindingInfo) -> Option<AstTy> {
        let ty = parse_binding_query_type(&binding.ty)?;
        matches!(ty, AstTy::Func(_, _, _)).then_some(ty)
    }

    fn sig_callable_arity_error(
        &self,
        source_query: &str,
        expected: usize,
        got: usize,
        callable_ty: &AstTy,
    ) -> ReplResult {
        let error = diagnostics::TypeErrorDiagnostic::new(
            format!("function expects {} argument(s), got {}", expected, got),
            Span {
                start: 0,
                end: source_query.len(),
            },
            Some(format!(
                "Callable type signature: {}",
                format_query_ty(callable_ty)
            )),
        );
        let spec = diagnostics::type_error_spec(source_query, &error);
        let rendered =
            error_display::diagnostic_lines("REPL", source_query, &spec, self.error_display_mode);
        ReplResult::ok(ReplOutput::EvalError {
            idx: self.results.len(),
            source: format!(":sig {source_query}"),
            rendered,
        })
    }

    fn query_arg_types(&self, args: &[QueryArg]) -> Result<Vec<String>, String> {
        args.iter().map(|arg| self.query_arg_type(arg)).collect()
    }

    fn repl_operator_query_unresolved_generic_error() -> String {
        format!(
            "Cannot use unresolved generic binding in REPL operator query. {}",
            REPL_UNRESOLVED_TYPE_HINT
        )
    }

    fn repl_typed_operator_operand_error(source: &str) -> String {
        format!(
            "Unsupported operator target query operand `{source}`. Use an existing binding or a concrete type such as `(Int -> String)`."
        )
    }

    fn query_arg_ast_types(&self, args: &[QueryArg]) -> Result<Vec<AstTy>, String> {
        args.iter().map(|arg| self.query_arg_ast_ty(arg)).collect()
    }

    fn query_arg_ast_ty(&self, arg: &QueryArg) -> Result<AstTy, String> {
        let ty = match &arg.kind {
            QueryArgKind::Binding(name) => {
                let Some(ty) = self.binding_type(name) else {
                    return Err(format!("Unknown query binding `{name}`."));
                };
                let parsed = parse_binding_query_type(&ty).ok_or_else(|| {
                    format!("Binding `{name}` has unsupported query type `{ty}`.")
                })?;
                if ast_ty_contains_query_placeholder(&parsed) {
                    return Err(Self::repl_operator_query_unresolved_generic_error());
                }
                Ok(parsed)
            }
            QueryArgKind::TypeExpr(_) => ast_ty_from_query_arg(arg)
                .ok_or_else(|| Self::repl_typed_operator_operand_error(&arg.source)),
        }?;
        if ast_ty_contains_query_placeholder(&ty) {
            return Err(Self::repl_operator_query_unresolved_generic_error());
        }
        Ok(ty)
    }

    fn typed_operator_signature(
        &self,
        query: &OperatorTargetQuery,
    ) -> Result<(String, AstTy), String> {
        let lhs_ty = self.query_arg_ast_ty(&query.target)?;
        let rhs_ty = self.query_arg_ast_ty(&query.target)?;
        match query.operator {
            "|>" => {
                let (params, ret) = Self::query_unary_func_parts(&rhs_ty, "|>")?;
                Self::ensure_query_type_matches(
                    &lhs_ty,
                    &params[0],
                    "`|>` requires the left operand to match the function input type",
                )?;
                Ok((
                    format!(
                        "PipeApply::pipe_apply(lhs: {}, rhs: {}) -> {}",
                        format_query_ty(&lhs_ty),
                        format_query_ty(&rhs_ty),
                        format_query_ty(&ret)
                    ),
                    ret,
                ))
            }
            "|*>" => {
                let (ctx_arg, _inner_ty, result_ty) = Self::query_map_result(&lhs_ty, &rhs_ty)?;
                Ok((
                    format!(
                        "Functor::fmap(lhs: {}, rhs: {}) -> {}",
                        format_query_ty(&ctx_arg),
                        format_query_ty(&rhs_ty),
                        format_query_ty(&result_ty)
                    ),
                    result_ty,
                ))
            }
            "|>=" => {
                let result_ty = Self::query_bind_result(&lhs_ty, &rhs_ty)?;
                Ok((
                    format!(
                        "Monad::bind(lhs: {}, rhs: {}) -> {}",
                        format_query_ty(&lhs_ty),
                        format_query_ty(&rhs_ty),
                        format_query_ty(&result_ty)
                    ),
                    result_ty,
                ))
            }
            "/" => match (&lhs_ty, &rhs_ty) {
                (
                    AstTy::Generic(_, left_name, left_args),
                    AstTy::Generic(_, right_name, right_args),
                ) if left_name == "Facet"
                    && right_name == "Facet"
                    && left_args.len() == 2
                    && right_args.len() == 2 =>
                {
                    Self::ensure_query_type_matches(
                        &left_args[1],
                        &right_args[0],
                        "`/` requires the left focus type to match the right source type",
                    )?;
                    let result_ty = AstTy::Generic(
                        Span { start: 0, end: 0 },
                        "Facet".to_string(),
                        vec![left_args[0].clone(), right_args[1].clone()],
                    );
                    Ok((
                        format!(
                            "Compose::compose(lhs: {}, rhs: {}) -> {}",
                            format_query_ty(&lhs_ty),
                            format_query_ty(&rhs_ty),
                            format_query_ty(&result_ty)
                        ),
                        result_ty,
                    ))
                }
                _ => Err(
                    "`/` currently models Facet composition. Use `Int::safe_div(...)` or `Float::safe_div(...)` for division."
                        .to_string(),
                ),
            },
            ">>" => {
                let (left_params, left_ret) = Self::query_unary_func_parts(&lhs_ty, ">>")?;
                let (right_params, right_ret) = Self::query_unary_func_parts(&rhs_ty, ">>")?;
                Self::ensure_query_type_matches(
                    &left_ret,
                    &right_params[0],
                    "`>>` requires the left output type to match the right input type",
                )?;
                let result_ty = AstTy::Func(
                    Span { start: 0, end: 0 },
                    vec![left_params[0].clone()],
                    Box::new(right_ret),
                );
                Ok((
                    format!(
                        "Composable::compose(lhs: {}, rhs: {}) -> {}",
                        format_query_ty(&lhs_ty),
                        format_query_ty(&rhs_ty),
                        format_query_ty(&result_ty)
                    ),
                    result_ty,
                ))
            }
            ">*" => {
                let (left_params, left_ret) = Self::query_unary_func_parts(&lhs_ty, ">*")?;
                let (right_params, right_ret) = Self::query_unary_func_parts(&rhs_ty, ">*")?;
                let result_inner = match &left_ret {
                    AstTy::Generic(_, name, args) if name == "Result" && args.len() == 1 => {
                        Self::ensure_query_type_matches(
                            &args[0],
                            &right_params[0],
                            "`>*` requires the contextual output to match the right input type",
                        )?;
                        AstTy::Generic(
                            Span { start: 0, end: 0 },
                            "Result".to_string(),
                            vec![right_ret],
                        )
                    }
                    AstTy::Generic(_, name, args) if name == "List" && args.len() == 1 => {
                        Self::ensure_query_type_matches(
                            &args[0],
                            &right_params[0],
                            "`>*` requires the contextual output to match the right input type",
                        )?;
                        AstTy::Generic(
                            Span { start: 0, end: 0 },
                            "List".to_string(),
                            vec![right_ret],
                        )
                    }
                    other => {
                        return Err(format!(
                            "`>*` requires a contextual left output, got {}.",
                            format_query_ty(other)
                        ));
                    }
                };
                let result_ty = AstTy::Func(
                    Span { start: 0, end: 0 },
                    vec![left_params[0].clone()],
                    Box::new(result_inner),
                );
                Ok((
                    format!(
                        "LiftComposable::lift_compose(lhs: {}, rhs: {}) -> {}",
                        format_query_ty(&lhs_ty),
                        format_query_ty(&rhs_ty),
                        format_query_ty(&result_ty)
                    ),
                    result_ty,
                ))
            }
            ">=>" => {
                let (left_params, left_ret) = Self::query_unary_func_parts(&lhs_ty, ">=>")?;
                let (right_params, right_ret) = Self::query_unary_func_parts(&rhs_ty, ">=>")?;
                let result_inner = match (&left_ret, &right_ret) {
                    (
                        AstTy::Generic(_, left_name, left_args),
                        AstTy::Generic(_, right_name, right_args),
                    ) if left_name == "Result"
                        && right_name == "Result"
                        && left_args.len() == 1
                        && right_args.len() == 1 =>
                    {
                        Self::ensure_query_type_matches(
                            &left_args[0],
                            &right_params[0],
                            "`>=>` requires the left contextual output to match the right input type",
                        )?;
                        AstTy::Generic(
                            Span { start: 0, end: 0 },
                            "Result".to_string(),
                            vec![right_args[0].clone()],
                        )
                    }
                    (
                        AstTy::Generic(_, left_name, left_args),
                        AstTy::Generic(_, right_name, right_args),
                    ) if left_name == "List"
                        && right_name == "List"
                        && left_args.len() == 1
                        && right_args.len() == 1 =>
                    {
                        Self::ensure_query_type_matches(
                            &left_args[0],
                            &right_params[0],
                            "`>=>` requires the left contextual output to match the right input type",
                        )?;
                        AstTy::Generic(
                            Span { start: 0, end: 0 },
                            "List".to_string(),
                            vec![right_args[0].clone()],
                        )
                    }
                    _ => {
                        return Err(
                            "`>=>` requires matching Result or List context on both sides."
                                .to_string(),
                        );
                    }
                };
                let result_ty = AstTy::Func(
                    Span { start: 0, end: 0 },
                    vec![left_params[0].clone()],
                    Box::new(result_inner),
                );
                Ok((
                    format!(
                        "KleisliComposable::kleisli_compose(lhs: {}, rhs: {}) -> {}",
                        format_query_ty(&lhs_ty),
                        format_query_ty(&rhs_ty),
                        format_query_ty(&result_ty)
                    ),
                    result_ty,
                ))
            }
            other => Err(format!("Unsupported operator query `{other}`.")),
        }
    }

    fn signature_accepts_arg_types(&self, signature: &str, arg_types: &[String]) -> bool {
        let Some(param_types) = Self::signature_param_types(signature) else {
            return false;
        };
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

    fn signature_param_types(signature: &str) -> Option<Vec<String>> {
        signature
            .split_once('(')
            .and_then(|(_, rest)| rest.rsplit_once(')').map(|(params, _)| params))
            .map(|params| {
                split_top_level_commas(params)
                    .into_iter()
                    .filter_map(|param| param.split_once(':').map(|(_, ty)| ty.trim().to_string()))
                    .collect()
            })
    }

    fn parameter_type_accepts_arg_type(param: &str, arg: &str) -> bool {
        if param == arg || param == "Self" || param.starts_with('$') {
            return true;
        }
        false
    }

    fn specialize_signature_return(&self, signature: &str, arg_types: &[AstTy]) -> Option<AstTy> {
        let (param_types, return_ty) = Self::signature_param_asts_and_return(signature)?;
        let self_ty = Self::self_type_from_signature(signature)
            .or_else(|| Self::implicit_self_type_from_args(&param_types, arg_types));
        let Some(substitutions) =
            Self::build_type_substitutions(&param_types, arg_types, self_ty.as_ref())
        else {
            return match &return_ty {
                AstTy::Named(_, name) if name == "Self" => self_ty,
                _ => None,
            };
        };
        Some(Self::substitute_query_ty(
            &return_ty,
            &substitutions,
            self_ty.as_ref(),
        ))
    }

    fn signature_param_asts_and_return(signature: &str) -> Option<(Vec<AstTy>, AstTy)> {
        let params = Self::signature_param_types(signature)?
            .into_iter()
            .filter_map(|ty| parse_signature_type(&ty))
            .collect::<Vec<_>>();
        let return_ty = signature_return_type(signature).and_then(parse_signature_type)?;
        Some((params, return_ty))
    }

    fn self_type_from_signature(signature: &str) -> Option<AstTy> {
        let for_pos = signature.find(" for ")?;
        let after_for = &signature[for_pos + " for ".len()..];
        let method_sep = after_for.find("::")?;
        parse_signature_type(after_for[..method_sep].trim())
    }

    fn implicit_self_type_from_args(params: &[AstTy], args: &[AstTy]) -> Option<AstTy> {
        (params.len() == args.len())
            .then_some((params.first()?, args.first()?))
            .and_then(|(param, arg)| match param {
                AstTy::Named(_, name) if name == "Self" => Some(arg.clone()),
                _ => None,
            })
    }

    fn build_type_substitutions(
        params: &[AstTy],
        args: &[AstTy],
        self_ty: Option<&AstTy>,
    ) -> Option<HashMap<String, AstTy>> {
        if params.len() != args.len() {
            return None;
        }
        let mut substitutions = HashMap::new();
        for (param, arg) in params.iter().zip(args) {
            if !Self::unify_query_ty(param, arg, &mut substitutions, self_ty) {
                return None;
            }
        }
        Some(substitutions)
    }

    fn unify_query_ty(
        param: &AstTy,
        arg: &AstTy,
        substitutions: &mut HashMap<String, AstTy>,
        self_ty: Option<&AstTy>,
    ) -> bool {
        match param {
            AstTy::Named(_, name) if name == "Self" => self_ty.is_none_or(|ty| ty == arg),
            AstTy::Named(_, name) if name.starts_with('$') => {
                if let Some(existing) = substitutions.get(name) {
                    existing == arg
                } else {
                    substitutions.insert(name.clone(), arg.clone());
                    true
                }
            }
            AstTy::Named(_, name) => matches!(arg, AstTy::Named(_, other) if other == name),
            AstTy::ImplTrait(_, name) => matches!(arg, AstTy::ImplTrait(_, other) if other == name),
            AstTy::Generic(_, name, params) => match arg {
                AstTy::Generic(_, other, args) if name == other && params.len() == args.len() => {
                    params.iter().zip(args).all(|(param, arg)| {
                        Self::unify_query_ty(param, arg, substitutions, self_ty)
                    })
                }
                _ => false,
            },
            AstTy::Tuple(_, items) => match arg {
                AstTy::Tuple(_, other) if items.len() == other.len() => items
                    .iter()
                    .zip(other)
                    .all(|(param, arg)| Self::unify_query_ty(param, arg, substitutions, self_ty)),
                _ => false,
            },
            AstTy::Func(_, params, ret) => match arg {
                AstTy::Func(_, other_params, other_ret) if params.len() == other_params.len() => {
                    params.iter().zip(other_params).all(|(param, arg)| {
                        Self::unify_query_ty(param, arg, substitutions, self_ty)
                    }) && Self::unify_query_ty(ret, other_ret, substitutions, self_ty)
                }
                _ => false,
            },
        }
    }

    fn substitute_query_ty(
        ty: &AstTy,
        substitutions: &HashMap<String, AstTy>,
        self_ty: Option<&AstTy>,
    ) -> AstTy {
        match ty {
            AstTy::Named(_, name) if name == "Self" => {
                self_ty.cloned().unwrap_or_else(|| ty.clone())
            }
            AstTy::Named(_, name) if name.starts_with('$') => substitutions
                .get(name)
                .cloned()
                .unwrap_or_else(|| ty.clone()),
            AstTy::Named(_, _) | AstTy::ImplTrait(_, _) => ty.clone(),
            AstTy::Generic(span, name, args) => AstTy::Generic(
                span.clone(),
                name.clone(),
                args.iter()
                    .map(|arg| Self::substitute_query_ty(arg, substitutions, self_ty))
                    .collect(),
            ),
            AstTy::Tuple(span, items) => AstTy::Tuple(
                span.clone(),
                items
                    .iter()
                    .map(|item| Self::substitute_query_ty(item, substitutions, self_ty))
                    .collect(),
            ),
            AstTy::Func(span, params, ret) => AstTy::Func(
                span.clone(),
                params
                    .iter()
                    .map(|param| Self::substitute_query_ty(param, substitutions, self_ty))
                    .collect(),
                Box::new(Self::substitute_query_ty(ret, substitutions, self_ty)),
            ),
        }
    }

    fn query_unary_func_parts(ty: &AstTy, operator: &str) -> Result<(Vec<AstTy>, AstTy), String> {
        match ty {
            AstTy::Func(_, params, ret) if params.len() == 1 => {
                Ok((params.clone(), ret.as_ref().clone()))
            }
            AstTy::Func(_, params, _) => Err(format!(
                "`{operator}` expects a unary function type on this side, got {} parameter(s).",
                params.len()
            )),
            _ => Err(format!(
                "`{operator}` expects a function type, got {}.",
                format_query_ty(ty)
            )),
        }
    }

    fn ensure_query_type_matches(lhs: &AstTy, rhs: &AstTy, message: &str) -> Result<(), String> {
        if format_query_ty(lhs) == format_query_ty(rhs) {
            Ok(())
        } else {
            Err(format!(
                "{message}: left is {}, right is {}.",
                format_query_ty(lhs),
                format_query_ty(rhs)
            ))
        }
    }

    fn query_map_result(lhs_ty: &AstTy, rhs_ty: &AstTy) -> Result<(AstTy, AstTy, AstTy), String> {
        let (rhs_params, rhs_ret) = Self::query_unary_func_parts(rhs_ty, "|*>")?;
        match lhs_ty {
            AstTy::Generic(_, name, args) if name == "Result" && args.len() == 1 => {
                Self::ensure_query_type_matches(
                    &args[0],
                    &rhs_params[0],
                    "`|*>` requires the container value type to match the function input type",
                )?;
                Ok((
                    lhs_ty.clone(),
                    args[0].clone(),
                    AstTy::Generic(
                        Span { start: 0, end: 0 },
                        "Result".to_string(),
                        vec![rhs_ret],
                    ),
                ))
            }
            AstTy::Generic(_, name, args) if name == "List" && args.len() == 1 => {
                Self::ensure_query_type_matches(
                    &args[0],
                    &rhs_params[0],
                    "`|*>` requires the container value type to match the function input type",
                )?;
                Ok((
                    lhs_ty.clone(),
                    args[0].clone(),
                    AstTy::Generic(Span { start: 0, end: 0 }, "List".to_string(), vec![rhs_ret]),
                ))
            }
            other => Err(format!(
                "`|*>` requires Result or List on the left, got {}.",
                format_query_ty(other)
            )),
        }
    }

    fn query_bind_result(lhs_ty: &AstTy, rhs_ty: &AstTy) -> Result<AstTy, String> {
        let (rhs_params, rhs_ret) = Self::query_unary_func_parts(rhs_ty, "|>=")?;
        match (lhs_ty, &rhs_ret) {
            (
                AstTy::Generic(_, left_name, left_args),
                AstTy::Generic(_, right_name, right_args),
            ) if left_name == "Result"
                && right_name == "Result"
                && left_args.len() == 1
                && right_args.len() == 1 =>
            {
                Self::ensure_query_type_matches(
                    &left_args[0],
                    &rhs_params[0],
                    "`|>=` requires the container value type to match the function input type",
                )?;
                Ok(rhs_ret)
            }
            (
                AstTy::Generic(_, left_name, left_args),
                AstTy::Generic(_, right_name, right_args),
            ) if left_name == "List"
                && right_name == "List"
                && left_args.len() == 1
                && right_args.len() == 1 =>
            {
                Self::ensure_query_type_matches(
                    &left_args[0],
                    &rhs_params[0],
                    "`|>=` requires the container value type to match the function input type",
                )?;
                Ok(rhs_ret)
            }
            (other, _) => Err(format!(
                "`|>=` requires matching contextual types on both sides; left is {}, right is {}.",
                format_query_ty(other),
                format_query_ty(&rhs_ret)
            )),
        }
    }

    fn binding_type(&self, name: &str) -> Option<String> {
        self.result_metas
            .iter()
            .rev()
            .flatten()
            .flat_map(|meta| meta.bindings.iter().rev())
            .find(|binding| binding.name == name)
            .map(|binding| crate::surface_rendered_name(&binding.ty))
    }

    fn binding_info(&self, name: &str) -> Option<&forge::BindingInfo> {
        self.result_metas
            .iter()
            .rev()
            .flatten()
            .flat_map(|meta| meta.bindings.iter().rev())
            .find(|binding| binding.name == name)
    }

    fn history_summary(source: &str) -> String {
        source
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn is_replayable_stmt(stmt: &Ast) -> bool {
        matches!(
            stmt,
            Ast::Import(_, _, _) | Ast::Def(..) | Ast::ExtractorDef(..)
        )
    }

    fn chunk_is_replayable(ast: &[Ast]) -> bool {
        !ast.is_empty() && ast.iter().all(Self::is_replayable_stmt)
    }

    fn collect_def_records(ast: &[Ast], line: usize) -> Vec<ReplDefRecord> {
        ast.iter()
            .filter_map(|stmt| match stmt {
                Ast::Def(_, name, _, params, ..) => Some(ReplDefRecord {
                    line,
                    name: name.clone(),
                    arity: params.len(),
                }),
                Ast::ExtractorDef(_, name, ..) => Some(ReplDefRecord {
                    line,
                    name: name.clone(),
                    arity: 1,
                }),
                _ => None,
            })
            .collect()
    }

    fn collect_import_records(ast: &[Ast], line: usize) -> Vec<ReplImportRecord> {
        let mut records = Vec::new();
        for stmt in ast {
            let Ast::Import(_, path, spec) = stmt else {
                continue;
            };
            let module_name = path.segments.join("::");
            match spec {
                ImportSpec::All => records.push(ReplImportRecord {
                    line,
                    src: "kwd".to_string(),
                    item: module_name,
                    via: "import".to_string(),
                }),
                ImportSpec::Single(name) => records.push(ReplImportRecord {
                    line,
                    src: "kwd".to_string(),
                    item: format!("{}::{}", module_name, name),
                    via: "import".to_string(),
                }),
                ImportSpec::List(names) => {
                    for name in names {
                        records.push(ReplImportRecord {
                            line,
                            src: "kwd".to_string(),
                            item: format!("{}::{}", module_name, name),
                            via: "import".to_string(),
                        });
                    }
                }
            }
        }
        records
    }

    fn collect_auto_import_records(
        module_stages: &[Vec<sigil::StagedModuleAst>],
        declaration_index: &sigil::DeclarationIndex,
    ) -> Vec<ReplImportRecord> {
        let mut seen = BTreeSet::new();
        let mut records = Vec::new();
        let mut member_auto_import_modules = BTreeSet::new();

        for stage in module_stages {
            for module in stage {
                if !module.auto_import {
                    continue;
                }
                let first_non_import = module
                    .ast
                    .iter()
                    .find(|stmt| !matches!(stmt, Ast::Import(_, _, _)));
                match first_non_import {
                    Some(Ast::ImplDef(_, _, _, _) | Ast::TraitImplDef(..)) => {
                        member_auto_import_modules.insert(module.module_path.clone());
                    }
                    _ => {
                        let item = crate::surface_rendered_name(&module.module_path);
                        if seen.insert((
                            0usize,
                            "auto".to_string(),
                            item.clone(),
                            "@autoimport".to_string(),
                        )) {
                            records.push(ReplImportRecord {
                                line: 0,
                                src: "auto".to_string(),
                                item,
                                via: "@autoimport".to_string(),
                            });
                        }
                    }
                }
            }
        }

        for entry in declaration_index.values() {
            if entry.kind != sigil::DeclarationKind::Trait
                || !entry.auto_import
                || entry.hidden
                || !entry.user_importable
                || entry.visibility != spire::ast::Visibility::Public
            {
                continue;
            }
            let method_prefix = format!("{}::", entry.fq_name);
            for method_entry in declaration_index.values() {
                if method_entry.kind != sigil::DeclarationKind::TraitMethod
                    || !method_entry.fq_name.starts_with(&method_prefix)
                    || method_entry.hidden
                    || !method_entry.user_importable
                    || method_entry.visibility != spire::ast::Visibility::Public
                {
                    continue;
                }
                let item = crate::surface_rendered_name(&method_entry.name);
                if seen.insert((
                    0usize,
                    "auto".to_string(),
                    item.clone(),
                    "@autoimport".to_string(),
                )) {
                    records.push(ReplImportRecord {
                        line: 0,
                        src: "auto".to_string(),
                        item,
                        via: "@autoimport".to_string(),
                    });
                }
            }
        }

        let current_stage_index = module_stages.len().saturating_sub(1);
        if let Ok(entries) =
            sigil::effective_auto_import_entries(module_stages, None, current_stage_index)
        {
            for entry in entries {
                if !member_auto_import_modules.contains(&entry.module_path) {
                    continue;
                }
                let item = match entry.kind {
                    sigil::DeclarationKind::TraitMethod => {
                        crate::surface_rendered_name(&entry.name)
                    }
                    _ => crate::surface_rendered_name(&entry.fq_name),
                };
                if seen.insert((
                    0usize,
                    "auto".to_string(),
                    item.clone(),
                    "@autoimport".to_string(),
                )) {
                    records.push(ReplImportRecord {
                        line: 0,
                        src: "auto".to_string(),
                        item,
                        via: "@autoimport".to_string(),
                    });
                }
            }
        }

        records.sort_by(|left, right| {
            left.line
                .cmp(&right.line)
                .then_with(|| left.src.cmp(&right.src))
                .then_with(|| left.item.cmp(&right.item))
                .then_with(|| left.via.cmp(&right.via))
        });
        records
    }

    fn handle_vars(&self) -> ReplResult {
        if self.binding_records.is_empty() {
            return Self::plain(vec!["No visible value bindings.".to_string()]);
        }
        let mut lines = vec!["line | name | type".to_string()];
        lines.extend(
            self.binding_records
                .iter()
                .map(|binding| format!("{}: {}: {}", binding.line, binding.name, binding.ty)),
        );
        Self::plain(lines)
    }

    fn handle_imported(&self) -> ReplResult {
        let mut lines = vec!["line | src | item | via".to_string()];
        lines.extend(self.auto_import_records.iter().map(|record| {
            format!(
                "{} | {} | {} | {}",
                record.line, record.src, record.item, record.via
            )
        }));
        lines.extend(self.import_records.iter().map(|record| {
            format!(
                "{} | {} | {} | {}",
                record.line, record.src, record.item, record.via
            )
        }));
        if lines.len() == 1 {
            lines.push("No imports are active.".to_string());
        }
        Self::plain(lines)
    }

    fn handle_defs(&self) -> ReplResult {
        if self.def_records.is_empty() {
            return Self::plain(vec!["No visible top-level defs.".to_string()]);
        }
        let mut lines = vec!["line | name | arity".to_string()];
        lines.extend(
            self.def_records
                .iter()
                .map(|record| format!("{}: {}/{}", record.line, record.name, record.arity)),
        );
        Self::plain(lines)
    }

    fn parse_history_selector(&self, selector: &str) -> Result<Vec<usize>, String> {
        let selector = selector.trim();
        if selector.is_empty() {
            return Ok(self
                .history_entries
                .iter()
                .map(|entry| entry.line)
                .collect());
        }
        if let Some((start, end)) = selector.split_once("..") {
            let start = start
                .trim()
                .parse::<usize>()
                .map_err(|_| format!("Invalid history selector `{selector}`."))?;
            let end = end
                .trim()
                .parse::<usize>()
                .map_err(|_| format!("Invalid history selector `{selector}`."))?;
            if start > end {
                return Err(format!("History range `{selector}` is reversed."));
            }
            return Ok((start..=end).collect());
        }

        selector
            .split(',')
            .map(|part| {
                part.trim()
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid history selector `{selector}`."))
            })
            .collect()
    }

    fn handle_history(&self, selector: Option<&str>) -> ReplResult {
        if self.history_entries.is_empty() {
            return Self::plain(vec!["No REPL history yet.".to_string()]);
        }
        let selected = match selector {
            Some(selector) => match self.parse_history_selector(selector) {
                Ok(lines) => lines,
                Err(message) => {
                    return self.repl_command_diagnostic(
                        &format!(":history {}", selector.trim()),
                        message,
                        Span {
                            start: ":history ".chars().count(),
                            end: format!(":history {}", selector.trim()).chars().count(),
                        },
                        Some("Usage: :history [selector]".to_string()),
                        Vec::new(),
                    );
                }
            },
            None => self
                .history_entries
                .iter()
                .map(|entry| entry.line)
                .collect(),
        };

        let mut lines = vec!["line | input".to_string()];
        for line in selected {
            let Some(entry) = self.history_entries.iter().find(|entry| entry.line == line) else {
                return self.repl_command_diagnostic(
                    &format!(":history {}", selector.unwrap_or_default().trim()),
                    format!("History line {} is out of range.", line),
                    Span {
                        start: ":history ".chars().count(),
                        end: format!(":history {}", selector.unwrap_or_default().trim())
                            .chars()
                            .count(),
                    },
                    Some("Usage: :history [selector]".to_string()),
                    Vec::new(),
                );
            };
            lines.push(format!(
                "{}: {}",
                entry.line,
                Self::history_summary(&entry.source)
            ));
        }
        Self::plain(lines)
    }

    fn rebuild_for_reload(&self, keep_session_defs: bool) -> Result<Self, ReplResult> {
        let mut engine = match &self.reload_seed {
            ReplReloadSeed::Empty => ReplEngine::new()
                .map_err(|error| Self::plain(vec![format!("reload failed: {}", error)]))?,
            ReplReloadSeed::ProjectModuleStages(module_input_stages) => {
                ReplEngine::from_project_module_stages(module_input_stages)
                    .map_err(|error| Self::plain(vec![format!("reload failed: {}", error)]))?
            }
            ReplReloadSeed::Sources { module, script } => ReplEngine::from_preload_sources(
                module
                    .as_ref()
                    .map(|(file_name, source)| (file_name.as_str(), source.as_str())),
                script
                    .as_ref()
                    .map(|(file_name, source)| (file_name.as_str(), source.as_str())),
            )
            .map_err(|error| Self::plain(vec![format!("reload failed: {}", error)]))?,
        };
        engine.reload_seed = self.reload_seed.clone();
        engine.error_display_mode = self.error_display_mode;
        engine.stack_trace_display_mode = self.stack_trace_display_mode;
        if keep_session_defs {
            for input in &self.replay_inputs {
                let result = engine.handle_line(input);
                if result.should_exit {
                    return Err(Self::plain(vec![
                        "reload failed: replay requested REPL exit".to_string(),
                    ]));
                }
                match result.output {
                    ReplOutput::EvalSuccess { .. }
                    | ReplOutput::PlainText { .. }
                    | ReplOutput::StyledDoc { .. }
                    | ReplOutput::StatusMessage(_) => {}
                    ReplOutput::EvalError { .. } | ReplOutput::Diagnostic { .. } => {
                        return Err(result);
                    }
                    ReplOutput::DocResolved { .. } | ReplOutput::EvalStarted { .. } => {}
                }
            }
        }
        Ok(engine)
    }

    fn handle_reload(&mut self, mode: Option<&str>) -> ReplResult {
        let keep_session_defs = match mode.map(str::trim).filter(|mode| !mode.is_empty()) {
            None | Some("all") => true,
            Some("defs") => false,
            Some(other) => {
                return self.repl_command_diagnostic(
                    &format!(":reload {}", other),
                    format!("Invalid reload mode `{}`.", other),
                    Span {
                        start: ":reload ".chars().count(),
                        end: format!(":reload {}", other).chars().count(),
                    },
                    Some("Usage: :reload [all|defs]".to_string()),
                    Vec::new(),
                );
            }
        };

        let mode_name = if keep_session_defs { "all" } else { "defs" };
        match self.rebuild_for_reload(keep_session_defs) {
            Ok(engine) => {
                *self = engine;
                Self::plain(vec![format!("reload complete: {}", mode_name)])
            }
            Err(result) => result,
        }
    }

    fn handle_clear(&self) -> ReplResult {
        Self::plain(vec!["clear is not available in this host".to_string()])
    }

    fn is_type_lookup_symbol(symbol: &str) -> bool {
        if symbol.is_empty() {
            return false;
        }
        if matches!(
            symbol,
            "True"
                | "False"
                | "def"
                | "defp"
                | "defmod"
                | "namespace"
                | "deftrait"
                | "import"
                | "include"
                | "if"
                | "if_then"
                | "defstruct"
                | "defrecord"
                | "deferror"
                | "defenum"
                | "defextractor"
                | "impl"
                | "for"
                | "match"
                | "when"
                | "cond"
                | "private"
                | "public"
                | "const"
                | "type"
                | "where"
        ) {
            return false;
        }

        let mut chars = symbol.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        if !(first.is_ascii_alphabetic() || first == '_') {
            return false;
        }
        chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    }

    fn render_type_identity(&self, binding: &forge::BindingInfo, value: Option<&Value>) -> String {
        if binding.facet_info.is_some() {
            return ReplTypeDisplayCategory::FacetPath
                .identity_label()
                .to_string();
        }
        if let Some(kind) = binding.callable_kind {
            return Self::callable_display_category(kind)
                .identity_label()
                .to_string();
        }
        let category = match value {
            None => ReplTypeDisplayCategory::Type,
            Some(Value::Callable(callable)) => match callable.metadata.origin {
                sindr::runtime::CallableOrigin::Closure => ReplTypeDisplayCategory::Closure,
                sindr::runtime::CallableOrigin::Capture => ReplTypeDisplayCategory::Capture,
                sindr::runtime::CallableOrigin::Unknown => ReplTypeDisplayCategory::Closure,
            },
            Some(Value::Tagged { tag, .. }) => self
                .vm
                .type_registry()
                .lookup(*tag)
                .map(|entry| match entry.kind {
                    TypeKind::Struct => ReplTypeDisplayCategory::Struct,
                    TypeKind::Record => ReplTypeDisplayCategory::Record,
                    TypeKind::EnumVariant => ReplTypeDisplayCategory::Enum,
                })
                .unwrap_or(ReplTypeDisplayCategory::Type),
            Some(_) => ReplTypeDisplayCategory::Type,
        };
        category.identity_label().to_string()
    }

    fn callable_display_category(kind: forge::ReplCallableKind) -> ReplTypeDisplayCategory {
        match kind {
            forge::ReplCallableKind::Closure => ReplTypeDisplayCategory::Closure,
            forge::ReplCallableKind::Capture => ReplTypeDisplayCategory::Capture,
        }
    }

    fn handle_error_mode(&mut self, mode: Option<&str>) -> ReplResult {
        let Some(mode) = mode else {
            return Self::plain(vec![format!(
                "error display mode: {}",
                self.error_display_mode.as_str()
            )]);
        };

        match ErrorDisplayMode::parse(mode.trim()) {
            Some(parsed) => {
                self.error_display_mode = parsed;
                Self::plain(vec![format!("error display mode: {}", parsed.as_str())])
            }
            None => self.repl_command_diagnostic(
                &format!(":error {}", mode.trim()),
                format!("Invalid error display mode `{}`.", mode.trim()),
                Span {
                    start: ":error ".chars().count(),
                    end: format!(":error {}", mode.trim()).chars().count(),
                },
                Some("Usage: :error [full|summary]".to_string()),
                vec!["Use `:error full` or `:error summary`.".to_string()],
            ),
        }
    }

    fn handle_stacktrace_mode(&mut self, mode: Option<&str>) -> ReplResult {
        let Some(mode) = mode else {
            return Self::plain(vec![format!(
                "stacktrace display mode: {}",
                self.stack_trace_display_mode.as_str()
            )]);
        };
        let mode = mode.trim();

        if mode == "full" {
            return Self::plain(vec![
                "stacktrace display mode `full` is not supported yet.".to_string(),
                "Full stacktrace rendering is reserved for future HTMLViewer support.".to_string(),
                format!(
                    "stacktrace display mode: {}",
                    self.stack_trace_display_mode.as_str()
                ),
            ]);
        }

        match StackTraceDisplayMode::parse(mode) {
            Some(parsed) => {
                self.stack_trace_display_mode = parsed;
                Self::plain(vec![format!(
                    "stacktrace display mode: {}",
                    parsed.as_str()
                )])
            }
            None => self.repl_command_diagnostic(
                &format!(":stacktrace {}", mode),
                format!("Invalid stacktrace display mode `{}`.", mode),
                Span {
                    start: ":stacktrace ".chars().count(),
                    end: format!(":stacktrace {}", mode).chars().count(),
                },
                Some("Usage: :stacktrace [off|verbose|full]".to_string()),
                vec![
                    "Use `:stacktrace off` or `:stacktrace verbose`.".to_string(),
                    "`:stacktrace full` is reserved for future HTMLViewer support.".to_string(),
                ],
            ),
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

    fn append_signatures(&mut self, signatures: Vec<SignatureEntry>) {
        for signature in signatures {
            let exists = self
                .signatures
                .iter()
                .any(|existing| existing == &signature);
            if !exists {
                self.signatures.push(signature);
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
                return Self::plain(vec![]);
            }
            if let Some(cmd) = parse_repl_command(trimmed) {
                match cmd {
                    ReplCommand::Quit => {
                        return ReplResult::exit(ReplOutput::StatusMessage("quit".to_string()));
                    }
                    ReplCommand::Help { topic } => {
                        let rendered = self.handle_help(topic.as_deref());
                        return Self::plain(rendered);
                    }
                    ReplCommand::Doc { symbol } => {
                        return self.handle_doc(&symbol);
                    }
                    ReplCommand::Sig { symbol } => {
                        return self.handle_sig(&symbol);
                    }
                    ReplCommand::Info { query } => {
                        return self.handle_info(&query);
                    }
                    ReplCommand::Type { symbol } => {
                        return self.handle_type(&symbol);
                    }
                    ReplCommand::Facet { query } => {
                        return self.handle_facet(&query);
                    }
                    ReplCommand::Error { mode } => {
                        return self.handle_error_mode(mode.as_deref());
                    }
                    ReplCommand::StackTrace { mode } => {
                        return self.handle_stacktrace_mode(mode.as_deref());
                    }
                    ReplCommand::ValueRecall { arg } => {
                        return self.handle_value_recall(&arg);
                    }
                    ReplCommand::Save { path } => {
                        return self.handle_save(&path);
                    }
                    ReplCommand::Vars => {
                        return self.handle_vars();
                    }
                    ReplCommand::Imported => {
                        return self.handle_imported();
                    }
                    ReplCommand::Defs => {
                        return self.handle_defs();
                    }
                    ReplCommand::History { selector } => {
                        return self.handle_history(selector.as_deref());
                    }
                    ReplCommand::Reload { mode } => {
                        return self.handle_reload(mode.as_deref());
                    }
                    ReplCommand::Clear => {
                        return self.handle_clear();
                    }
                    ReplCommand::Unknown { raw } => {
                        return self.repl_command_diagnostic(
                            &raw,
                            format!("Unknown REPL command: {}", raw),
                            Span {
                                start: 0,
                                end: raw.chars().count(),
                            },
                            Some("Type :help for available REPL commands.".to_string()),
                            Vec::new(),
                        );
                    }
                }
            }
        }

        let idx = self.results.len();
        let source = line.to_string();
        let committed_line = self.next_line;

        self.pending.push_str(line);
        self.pending.push('\n');
        self.sources
            .update_source(self.repl_source_id, self.pending.clone());

        let ast = match spire::parse_with_context(
            &self.pending,
            crate::derive_parser_context(
                self.repl_source_id.0,
                SourceKind::ReplChunk,
                CompileUnitKind::Repl,
                None,
            ),
        ) {
            Ok(ast) => ast,
            Err(e) if e.is_incomplete() => {
                return Self::plain(vec![]);
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
                self.history_entries.push(ReplHistoryEntry {
                    line: committed_line,
                    source: self.pending.clone(),
                });
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
            return Self::plain(vec![]);
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
                self.history_entries.push(ReplHistoryEntry {
                    line: committed_line,
                    source: self.pending.clone(),
                });
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
        let signatures =
            crate::collect_signature_entries(&[], &ast, Some(self.repl_module_path.as_str()));
        let resolved = match self.sigil_session.resolve(ast.clone()) {
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
                self.history_entries.push(ReplHistoryEntry {
                    line: committed_line,
                    source: self.pending.clone(),
                });
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
            Self::typecheck_context_for_source(SourceKind::ReplChunk),
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
                self.history_entries.push(ReplHistoryEntry {
                    line: committed_line,
                    source: self.pending.clone(),
                });
                self.pending.clear();
                self.bump_line(None, None);
                return ReplResult::ok(ReplOutput::EvalError {
                    idx,
                    source,
                    rendered,
                });
            }
        };

        if let Some((span, names)) = unresolved_repl_binding_issue(&typed) {
            self.sigil_session.rollback(sigil_cp);
            self.scar_session.rollback(scar_cp);
            self.forge_session.rollback(forge_cp);
            let hint = if names.is_empty() {
                REPL_UNRESOLVED_TYPE_HINT.to_string()
            } else {
                format!(
                    "{} Affected binding(s): {}.",
                    REPL_UNRESOLVED_TYPE_HINT,
                    names.join(", ")
                )
            };
            let error = diagnostics::TypeErrorDiagnostic::new(
                REPL_UNRESOLVED_TYPE_MESSAGE,
                span,
                Some(hint),
            );
            let spec =
                diagnostics::type_error_spec_by_id(&self.sources, self.repl_source_id, &error);
            let rendered = error_display::diagnostic_lines_by_id(
                &self.sources,
                self.repl_source_id,
                &spec,
                self.error_display_mode,
            );
            self.history_entries.push(ReplHistoryEntry {
                line: committed_line,
                source: self.pending.clone(),
            });
            self.pending.clear();
            self.bump_line(None, None);
            return ReplResult::ok(ReplOutput::EvalError {
                idx,
                source,
                rendered,
            });
        }

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
                self.history_entries.push(ReplHistoryEntry {
                    line: committed_line,
                    source: self.pending.clone(),
                });
                self.pending.clear();
                self.bump_line(None, None);
                return ReplResult::ok(ReplOutput::EvalError {
                    idx,
                    source,
                    rendered,
                });
            }
        };
        let chunk_functions = chunk.functions.clone();
        meta.docs = docs.clone();
        chunk.docs = docs.clone();

        if let Some(repl_source) = self.sources.source(self.repl_source_id) {
            populate_error_template_lines(&mut chunk.error_templates, repl_source);
        }
        if let Some((source_str, file_name)) = self.sources.owned_context(self.repl_source_id) {
            self.vm.set_source(source_str, file_name);
        }

        match self.execute_vm_chunk(chunk, ReplSessionPhase::Live) {
            Ok(execution) => {
                let committed_source = self.pending.clone();
                let value = eval::committed_chunk_value(execution);
                self.sync_scar_fun_index_with_vm();
                self.sync_repl_chunk_function_indices(&meta.function_defs, &chunk_functions);
                if let Some(rendered) = self.report_main_result_error_if_any(&value) {
                    let (stdout, stderr) = self.take_repl_host_io_lines();
                    self.history_entries.push(ReplHistoryEntry {
                        line: committed_line,
                        source: committed_source,
                    });
                    self.bump_line(None, None);
                    self.pending.clear();
                    return ReplResult::ok(ReplOutput::EvalError {
                        idx,
                        source,
                        rendered,
                    })
                    .with_stdout(stdout)
                    .with_stderr(stderr);
                }

                let rendered =
                    render::format_result_lines(self.vm.as_vm(), Some(&value), Some(&meta));

                let (stdout, stderr) = self.take_repl_host_io_lines();
                let mut all_rendered = rendered;
                if import_only {
                    for label in &import_result.success_labels {
                        all_rendered.push(format!("Imported {}", label));
                    }
                }
                for imported in &import_result.imported_symbols {
                    self.insert_completion_symbol(imported.clone());
                }
                for b in &meta.bindings {
                    self.insert_completion_symbol(b.name.clone());
                }
                for name in &meta.function_defs {
                    self.insert_surface_symbol(name);
                }
                self.append_docs(docs);
                self.append_signatures(signatures);
                self.history_entries.push(ReplHistoryEntry {
                    line: committed_line,
                    source: committed_source.clone(),
                });
                self.binding_records
                    .extend(meta.bindings.iter().map(|binding| ReplBindingRecord {
                        line: committed_line,
                        name: binding.name.clone(),
                        ty: crate::surface_rendered_name(&binding.ty),
                    }));
                self.import_records
                    .extend(Self::collect_import_records(&ast, committed_line));
                self.def_records
                    .extend(Self::collect_def_records(&ast, committed_line));
                self.sync_cached_completion_context_after_commit(
                    &import_result.imported_symbols,
                    &meta.bindings,
                    &meta.function_defs,
                );
                if Self::chunk_is_replayable(&ast) {
                    self.replay_inputs.push(committed_source);
                }
                let history_value = history_value_for_result(&self.vm, &value, &meta);
                self.bump_line(Some(history_value), Some(meta.clone()));
                self.pending.clear();
                ReplResult::ok(ReplOutput::EvalSuccess {
                    idx,
                    source,
                    rendered: all_rendered,
                })
                .with_stdout(stdout)
                .with_stderr(stderr)
            }
            Err(e) => {
                let committed_source = self.pending.clone();
                self.sigil_session.rollback(sigil_cp);
                self.scar_session.rollback(scar_cp);
                self.forge_session.rollback(forge_cp);
                let location = e
                    .context
                    .call_site
                    .clone()
                    .or_else(|| self.vm.runtime_error_location());
                let rendered = error_display::runtime_error_lines_with_registry_and_stack_trace(
                    &e,
                    &self.sources,
                    self.repl_source_id,
                    location.clone(),
                    self.error_display_mode,
                    self.stack_trace_display_mode,
                );
                let (stdout, stderr) = self.take_repl_host_io_lines();
                self.history_entries.push(ReplHistoryEntry {
                    line: committed_line,
                    source: committed_source,
                });
                self.bump_line(None, None);
                self.pending.clear();
                ReplResult::exit(ReplOutput::EvalError {
                    idx,
                    source,
                    rendered,
                })
                .with_stdout(stdout)
                .with_stderr(stderr)
            }
        }
    }

    pub fn has_pending_background_work(&self) -> bool {
        self.vm.has_pending_background_work()
    }

    pub fn next_background_deadline_delay(&self) -> Option<std::time::Duration> {
        self.vm.next_background_deadline_delay()
    }

    pub fn pump_background_ready(&mut self) -> ReplResult {
        match self.vm.pump_background_ready() {
            Ok(()) => {
                let (stdout, stderr) = self.take_repl_host_io_lines();
                Self::plain(stdout).with_stderr(stderr)
            }
            Err(err) => {
                let location = err
                    .context
                    .call_site
                    .clone()
                    .or_else(|| self.vm.runtime_error_location());
                let rendered = error_display::runtime_error_lines_with_stack_trace(
                    &err,
                    self.vm.source(),
                    self.vm.source_file(),
                    location,
                    self.error_display_mode,
                    self.stack_trace_display_mode,
                );
                let (stdout, stderr) = self.take_repl_host_io_lines();
                ReplResult::ok(ReplOutput::EvalError {
                    idx: self.results.len(),
                    source: "<background>".to_string(),
                    rendered,
                })
                .with_stdout(stdout)
                .with_stderr(stderr)
            }
        }
    }

    pub fn advance_background_time(&mut self, elapsed: std::time::Duration) -> ReplResult {
        match self.vm.advance_background_time(elapsed) {
            Ok(()) => {
                let (stdout, stderr) = self.take_repl_host_io_lines();
                Self::plain(stdout).with_stderr(stderr)
            }
            Err(err) => {
                let location = err
                    .context
                    .call_site
                    .clone()
                    .or_else(|| self.vm.runtime_error_location());
                let rendered = error_display::runtime_error_lines_with_stack_trace(
                    &err,
                    self.vm.source(),
                    self.vm.source_file(),
                    location,
                    self.error_display_mode,
                    self.stack_trace_display_mode,
                );
                let (stdout, stderr) = self.take_repl_host_io_lines();
                ReplResult::ok(ReplOutput::EvalError {
                    idx: self.results.len(),
                    source: "<background>".to_string(),
                    rendered,
                })
                .with_stdout(stdout)
                .with_stderr(stderr)
            }
        }
    }

    pub fn pump_background_to_next_deadline(&mut self) -> ReplResult {
        match self.vm.pump_background_to_next_deadline() {
            Ok(_) => self.pump_background_ready(),
            Err(err) => {
                let location = err
                    .context
                    .call_site
                    .clone()
                    .or_else(|| self.vm.runtime_error_location());
                let rendered = error_display::runtime_error_lines_with_stack_trace(
                    &err,
                    self.vm.source(),
                    self.vm.source_file(),
                    location,
                    self.error_display_mode,
                    self.stack_trace_display_mode,
                );
                let (stdout, stderr) = self.take_repl_host_io_lines();
                ReplResult::ok(ReplOutput::EvalError {
                    idx: self.results.len(),
                    source: "<background>".to_string(),
                    rendered,
                })
                .with_stdout(stdout)
                .with_stderr(stderr)
            }
        }
    }

    fn handle_save(&mut self, arg: &str) -> ReplResult {
        if arg.is_empty() {
            return Self::plain(vec!["Usage: :save <path.eldr>".to_string()]);
        }

        let path = if arg.ends_with(".eldr") {
            arg.to_string()
        } else {
            format!("{}.eldr", arg)
        };

        let mut bytecode = self.vm.snapshot_bytecode();
        bytecode.docs = self.docs.clone();
        bytecode.signatures = self.signatures.clone();
        match bytecode.encode() {
            Err(e) => Self::plain(vec![format!("Error encoding bytecode: {}", e)]),
            Ok(bytes) => match fs::write(&path, bytes) {
                Ok(()) => Self::plain(vec![format!("saved to {}", path)]),
                Err(e) => Self::plain(vec![format!("Error writing {}: {}", path, e)]),
            },
        }
    }

    fn handle_value_recall(&mut self, arg: &str) -> ReplResult {
        if arg.is_empty() {
            self.bump_line(None, None);
            return Self::plain(vec!["Usage: :v <line>".to_string()]);
        }

        let line_num = match arg.parse::<usize>() {
            Ok(n) if n > 0 => n,
            _ => {
                self.bump_line(None, None);
                return self.repl_command_diagnostic(
                    &format!(":v {arg}"),
                    format!("Invalid line number for :v: {}", arg),
                    Span {
                        start: ":v ".chars().count(),
                        end: format!(":v {arg}").chars().count(),
                    },
                    Some("Usage: :v <line>".to_string()),
                    Vec::new(),
                );
            }
        };

        if line_num > self.results.len() {
            self.bump_line(None, None);
            return Self::plain(vec![format!("No such line: {}", line_num)]);
        }

        match self.results[line_num - 1].clone() {
            Some(value) => {
                let displayed = inspect_value(self.vm.as_vm(), &value);
                self.bump_line(Some(value), None);
                Self::plain(vec![displayed])
            }
            None => {
                self.bump_line(None, None);
                Self::plain(vec![format!("Line {} has no value", line_num)])
            }
        }
    }
}

fn compile_preloaded_repl_chunk(
    module: Option<(&str, &str)>,
    script: Option<(&str, &str)>,
) -> Result<PreloadedChunkState, ReplLoadError> {
    let std_module_inputs =
        collect_additional_default_std_module_inputs().map_err(ReplLoadError::Load)?;
    let prepared_script = prepare_script_preload(script)?;
    let mut module_input_stages = vec![std_module_inputs];
    if let Some((file_name, source)) = module {
        module_input_stages.push(vec![crate::ModuleInput {
            file_name: file_name.to_string(),
            source: source.to_string(),
            module_path: crate::module_path_from_source_or_file_name(file_name, source),
        }]);
    }
    if let Some(script) = prepared_script.as_ref() {
        for module in &script.include_modules {
            module_input_stages.push(vec![module.clone()]);
        }
    }
    compile_repl_preload_from_module_stages(
        module_input_stages,
        prepared_script,
        PreloadCompileMode::SCRIPT,
    )
}

fn compile_project_repl_chunk(
    project_module_input_stages: &[Vec<crate::ModuleInput>],
) -> Result<PreloadedChunkState, ReplLoadError> {
    let std_module_inputs =
        collect_additional_default_std_module_inputs().map_err(ReplLoadError::Load)?;
    let mut module_input_stages = vec![std_module_inputs];
    module_input_stages.extend(project_module_input_stages.iter().cloned());
    compile_repl_preload_from_module_stages(module_input_stages, None, PreloadCompileMode::PROJECT)
}

fn compile_repl_preload_from_module_stages(
    module_input_stages: Vec<Vec<crate::ModuleInput>>,
    prepared_script: Option<PreparedScriptPreload>,
    mode: PreloadCompileMode,
) -> Result<PreloadedChunkState, ReplLoadError> {
    let mut repl_sources = loader::collect_repl_sources_with_module_stages(&module_input_stages)
        .map_err(ReplLoadError::Load)?;

    let user_file_name = prepared_script
        .as_ref()
        .map(|script| script.file_name.as_str())
        .unwrap_or("<repl-preload>");
    let user_source = prepared_script
        .as_ref()
        .map(|script| script.source_for_parse.as_str())
        .unwrap_or("");
    let user_source_id = repl_sources.sources.register(user_file_name, user_source);
    let compile_sources = crate::CompileSources {
        module_source_ids: repl_sources
            .module_stages
            .iter()
            .flat_map(|stage| stage.iter().map(|entry| entry.source_id))
            .collect(),
        sources: repl_sources.sources.clone(),
        user_source_id,
        user_module_path: crate::script_pseudo_module_path(user_file_name),
        builtin_source_id: repl_sources.builtin_source_id,
        builtin_module_path: Some("Bootstrap".to_string()),
        module_stages: repl_sources.module_stages.clone(),
        stdlib_variant: crate::StdlibVariant::Default,
    };
    let snapshot = crate::default_stdlib_semantic_snapshot().map_err(ReplLoadError::Load)?;
    let (module_stage_asts, raw_module_stages, user_ast, script_runtime_inputs) =
        parse_preload_sources(&compile_sources, &snapshot, mode.compile_unit_kind)?;

    let script_preload_docs = crate::collect_doc_entries(
        &[],
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );
    let script_preload_signatures = crate::collect_signature_entries(
        &[],
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );
    let process_metadata = collect_process_metadata(&module_stage_asts);
    let docs = crate::collect_doc_entries_with_base(
        &snapshot.docs,
        if module_stage_asts.len() > snapshot.default_stage_count {
            &module_stage_asts[snapshot.default_stage_count..]
        } else {
            &[]
        },
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );
    let signatures = crate::collect_signature_entries_with_base(
        &snapshot.signatures,
        if module_stage_asts.len() > snapshot.default_stage_count {
            &module_stage_asts[snapshot.default_stage_count..]
        } else {
            &[]
        },
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );

    let mut declaration_index = if module_stage_asts.len() == snapshot.default_stage_count {
        snapshot.declaration_index().clone()
    } else {
        sigil::precollect_declaration_index(&module_stage_asts).map_err(|e| {
            let spec = diagnostics::simple_error("ResolveError", &e.message, e.span, None);
            ReplLoadError::Diagnostic {
                phase: "resolve".to_string(),
                sources: compile_sources.sources.clone(),
                source_id: compile_sources.builtin_source_id,
                spec,
            }
        })?
    };
    merge_user_preload_declarations(
        &mut declaration_index,
        &user_ast,
        &compile_sources.user_module_path,
        module_stage_asts.len(),
        &compile_sources.sources,
        compile_sources.user_source_id,
    )?;

    let mut staged_program = sigil::resolve_staged_program_from_state(
        &module_stage_asts,
        Vec::new(),
        &declaration_index,
        Some(compile_sources.user_module_path.clone()),
        snapshot.default_stage_count,
        snapshot.resolve_state(),
    )
    .map_err(|e| preload_resolve_error(&compile_sources, &e))?;

    let mut sigil_session =
        sigil::SigilSession::with_module_path(Some(repl_sources.repl_module_path.clone()));
    let mut scope = sigil::build_scope_for_module(
        &module_stage_asts,
        Some(compile_sources.user_module_path.as_str()),
        module_stage_asts.len(),
    )
    .map_err(|e| preload_resolve_error(&compile_sources, &e))?;
    scope.advance_next_id_to(staged_program.resume_state.next_local_id);
    sigil_session.replace_scope_with_declarations(scope, &declaration_index);

    let mut preload_imported = Vec::new();
    if !user_ast.is_empty() {
        preload_imported = apply_preload_imports(
            &mut sigil_session,
            &declaration_index,
            &raw_module_stages,
            &module_stage_asts,
            &user_ast,
            &snapshot.auto_import_modules,
        )?;
        let user_resolved = sigil_session
            .resolve(user_ast.clone())
            .map_err(|e| preload_resolve_error(&compile_sources, &e))?;
        bind_preload_script_qualified_names(
            &mut sigil_session,
            &user_ast,
            &compile_sources.user_module_path,
        );
        staged_program.resolved.extend(user_resolved);
    }

    let mut scar_session = snapshot.compile_prefix().restored_scar_session();
    let typed = scar_session
        .typecheck_staged_program_with_context(
            staged_program,
            scar::TypecheckContext::from_source_policy(
                mode.runtime_source_kind
                    .policy(mode.compile_unit_kind, None),
            ),
        )
        .map_err(|e| ReplLoadError::Diagnostic {
            phase: "typecheck".to_string(),
            sources: compile_sources.sources.clone(),
            source_id: diagnostic_source_id(&compile_sources, &e.span),
            spec: diagnostics::type_error_spec_by_id(
                &compile_sources.sources,
                diagnostic_source_id(&compile_sources, &e.span),
                &diagnostics::TypeErrorDiagnostic::new(
                    e.message,
                    local_diagnostic_span(&compile_sources, &e.span),
                    e.hint,
                ),
            ),
        })?;

    let mut forge_session = snapshot.compile_prefix().forge_session();
    let (mut chunk, meta) = forge_session
        .codegen_chunk_typed_program(typed)
        .map_err(|e| ReplLoadError::Diagnostic {
            phase: "codegen".to_string(),
            sources: compile_sources.sources.clone(),
            source_id: diagnostic_source_id(&compile_sources, &e.span),
            spec: diagnostics::simple_error(
                "CodegenError",
                &e.message,
                local_diagnostic_span(&compile_sources, &e.span),
                None,
            ),
        })?;
    chunk.docs = docs.clone();
    chunk.signatures = signatures.clone();
    for stage in &raw_module_stages {
        for module in stage {
            if let Some(source) = compile_sources.sources.source(module.source_id) {
                populate_error_template_lines(&mut chunk.error_templates, source);
            }
        }
    }
    if let Some(source) = compile_sources.sources.source(user_source_id) {
        populate_error_template_lines(&mut chunk.error_templates, source);
    }

    let source_context = compile_sources
        .sources
        .owned_context(user_source_id)
        .or_else(|| {
            compile_sources
                .sources
                .owned_context(compile_sources.builtin_source_id)
        });
    let mut vm = match source_context {
        Some((source, file_name)) => session::bytecode_interactive_vm(snapshot.bytecode().clone())
            .with_source(source, file_name),
        None => session::bytecode_interactive_vm(snapshot.bytecode().clone()),
    };
    vm.push_chunk(chunk, ReplSessionPhase::Preload.execution_policy())
        .map_err(|e| ReplLoadError::Runtime {
            file_name: compile_sources
                .sources
                .file_name(user_source_id)
                .unwrap_or("<repl-preload>")
                .to_string(),
            message: e.to_string(),
        })?;

    let mut symbols: BTreeSet<String> = ["Ok", "Err"]
        .into_iter()
        .map(str::to_string)
        .chain(
            builtin_function_metas()
                .iter()
                .map(|meta| meta.name.to_string()),
        )
        .collect();
    for entry in vm.bytecode().functions.iter() {
        if let Some(name) = &entry.qualified_name {
            let surface_name = crate::surface_rendered_name(name);
            symbols.insert(surface_name.clone());
            if let Some(short) = surface_name.rsplit("::").next() {
                symbols.insert(short.to_string());
            }
        }
    }
    for entry in vm.bytecode().type_registry.entries().iter() {
        let surface_name = crate::surface_rendered_name(&entry.name);
        symbols.insert(surface_name.clone());
        if let Some(short) = surface_name.rsplit("::").next() {
            symbols.insert(short.to_string());
        }
    }
    for binding in &meta.bindings {
        symbols.insert(binding.name.clone());
    }
    for visible in apply_preload_visible_names(&user_ast, preload_imported) {
        symbols.insert(visible);
    }

    let auto_import_modules = module_stage_asts
        .iter()
        .flat_map(|stage| stage.iter())
        .filter(|module| module.auto_import)
        .map(|module| module.module_path.clone())
        .collect();
    let auto_import_records =
        ReplEngine::collect_auto_import_records(&module_stage_asts, &declaration_index);
    let import_records = ReplEngine::collect_import_records(&user_ast, 0);
    let def_records = ReplEngine::collect_def_records(&user_ast, 0);

    Ok(PreloadedChunkState {
        sources: repl_sources.sources,
        builtin_source_id: repl_sources.builtin_source_id,
        repl_source_id: repl_sources.repl_source_id,
        repl_module_path: repl_sources.repl_module_path,
        module_stages: raw_module_stages,
        declaration_index,
        sigil_session,
        scar_checkpoint: scar_session.checkpoint(),
        vm,
        docs,
        signatures,
        process_metadata,
        symbols,
        auto_import_modules,
        auto_import_records,
        script_runtime_inputs,
        script_preload_docs,
        script_preload_signatures,
        import_records,
        def_records,
    })
}

fn parse_preload_sources(
    compile_sources: &crate::CompileSources,
    snapshot: &crate::DefaultStdlibSnapshot,
    compile_unit_kind: CompileUnitKind,
) -> Result<
    (
        Vec<Vec<sigil::StagedModuleAst>>,
        Vec<Vec<StagedModule>>,
        Vec<Ast>,
        Vec<String>,
    ),
    ReplLoadError,
> {
    let expanded =
        crate::expand_snapshot_module_stages(compile_sources, snapshot, compile_unit_kind)
            .map_err(|e| ReplLoadError::Diagnostic {
                phase: "parse".to_string(),
                sources: compile_sources.sources.clone(),
                source_id: e.source_id,
                spec: diagnostics::parse_error_spec(
                    compile_sources.sources.source(e.source_id).unwrap_or(""),
                    e.message(),
                    e.span(),
                ),
            })?;
    let mut module_stage_asts = expanded.module_stages.into_owned();
    let raw_module_stages = compile_sources.module_stages.clone();

    let user_source = compile_sources
        .sources
        .source(compile_sources.user_source_id)
        .unwrap_or("");
    let user_ast = spire::parse_with_context(
        user_source,
        crate::derive_parser_context(
            compile_sources.user_source_id.0,
            SourceKind::Script,
            CompileUnitKind::Script,
            None,
        ),
    )
    .map_err(|e| ReplLoadError::Diagnostic {
        phase: "parse".to_string(),
        sources: compile_sources.sources.clone(),
        source_id: compile_sources.user_source_id,
        spec: diagnostics::parse_error_spec(user_source, e.message(), e.span().clone()),
    })?;
    let (preload_ast, script_runtime_inputs) = split_preload_script_ast(&user_ast, user_source);
    let (process_stage, preload_ast) = crate::extract_process_modules_from_user_ast(preload_ast);
    if !process_stage.is_empty() {
        module_stage_asts.push(process_stage);
    }

    Ok((
        module_stage_asts,
        raw_module_stages,
        preload_ast,
        script_runtime_inputs,
    ))
}

fn collect_process_metadata(
    module_stages: &[Vec<sigil::StagedModuleAst>],
) -> BTreeMap<String, ReplProcessMetadata> {
    let mut out = BTreeMap::new();
    for stage in module_stages {
        for module in stage {
            if let Some(spec) = &module.process_spec {
                out.insert(
                    spec.process_name.clone(),
                    ReplProcessMetadata {
                        kind: spec.kind,
                        instance: spec.instance,
                        handler_specs: spec.handler_specs.clone(),
                    },
                );
            }
        }
    }
    out
}

fn split_preload_script_ast(ast: &[Ast], source: &str) -> (Vec<Ast>, Vec<String>) {
    let first_runtime_index = ast
        .iter()
        .position(|stmt| !is_preload_declaration(stmt))
        .unwrap_or(ast.len());
    let preload_ast = ast[..first_runtime_index].to_vec();
    let runtime_inputs = ast[first_runtime_index..]
        .iter()
        .filter_map(|stmt| {
            let span = ast_span(stmt)?;
            let snippet = source
                .chars()
                .skip(span.start)
                .take(span.end.saturating_sub(span.start))
                .collect::<String>();
            let trimmed = snippet.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .collect();
    (preload_ast, runtime_inputs)
}

fn is_preload_declaration(stmt: &Ast) -> bool {
    matches!(
        stmt,
        Ast::Import(_, _, _)
            | Ast::Def(..)
            | Ast::ExtractorDef(..)
            | Ast::ConstDef(..)
            | Ast::SupervisorInit(..)
            | Ast::StructDef(..)
            | Ast::RecordDef(..)
            | Ast::DeferrorDef(..)
            | Ast::EnumDef(..)
            | Ast::TraitDef(..)
            | Ast::TraitImplDef(..)
            | Ast::ImplDef(..)
            | Ast::Namespace(..)
            | Ast::Defagent(..)
            | Ast::Defgenserver(..)
            | Ast::Defsupervisor(..)
            | Ast::DefdynamicSupervisor(..)
            | Ast::BuiltinDecl(..)
            | Ast::IntrinsicDecl(..)
            | Ast::BuiltinExtractorDecl(..)
            | Ast::BuiltinTypeDecl(..)
            | Ast::ResultCtorDecl(..)
    )
}

fn ast_span(stmt: &Ast) -> Option<&Span> {
    match stmt {
        Ast::Lit(span, _)
        | Ast::Var(span, _)
        | Ast::InternalVar(span, _)
        | Ast::Path(span, _)
        | Ast::FuncLiteralRef(span, _)
        | Ast::App(span, _, _)
        | Ast::TypeApply(span, _, _)
        | Ast::Block(span, _)
        | Ast::Bind(span, _, _)
        | Ast::SafeBind(span, _, _)
        | Ast::BinOp(span, _, _, _)
        | Ast::Pipe(span, _, _)
        | Ast::ContextMap(span, _, _)
        | Ast::ContextBind(span, _, _)
        | Ast::Compose(span, _, _)
        | Ast::LiftedCompose(span, _, _)
        | Ast::KleisliCompose(span, _, _)
        | Ast::ListNil(span)
        | Ast::ListCons(span, _, _)
        | Ast::ListLiteral(span, _)
        | Ast::HashMapLiteral(span, _)
        | Ast::RangeLiteral(span, _, _)
        | Ast::TupleLiteral(span, _)
        | Ast::Grouped(span, _)
        | Ast::InterpolatedStr(span, _)
        | Ast::Dbg(span, _)
        | Ast::Match(span, _, _)
        | Ast::BulkUpdate(span, _, _)
        | Ast::FieldAccess(span, _, _)
        | Ast::FacetSegmentAccess(span, _, _)
        | Ast::FacetCapture(span, _)
        | Ast::StructDef(span, ..)
        | Ast::RecordDef(span, _, _, _)
        | Ast::StructLit(span, _, _)
        | Ast::InternalStructLit(span, _, _)
        | Ast::ConstructorCall(span, _, _)
        | Ast::DeferrorDef(span, _, _, _, _)
        | Ast::EnumDef(span, _, _, _, _)
        | Ast::Def(span, ..)
        | Ast::ConstDef(span, _, _, _, _)
        | Ast::SupervisorInit(span, _)
        | Ast::ExtractorDef(span, _, _, _, _, _, _)
        | Ast::BuiltinDecl(span, _, _, _, _)
        | Ast::IntrinsicDecl(span, _, _, _)
        | Ast::BuiltinExtractorDecl(span, _, _, _, _)
        | Ast::BuiltinTypeDecl(span, _, _)
        | Ast::ResultCtorDecl(span, _, _, _, _)
        | Ast::Defmod(span, _, _, _)
        | Ast::Defagent(span, _, _, _, _)
        | Ast::Defgenserver(span, _, _, _, _)
        | Ast::Defsupervisor(span, _, _, _, _)
        | Ast::DefdynamicSupervisor(span, _, _, _, _)
        | Ast::Namespace(span, _, _)
        | Ast::ImplDef(span, _, _, _)
        | Ast::TraitDef(span, ..)
        | Ast::TraitImplDef(span, ..)
        | Ast::Import(span, _, _)
        | Ast::Include(span, _)
        | Ast::Closure(span, _, _)
        | Ast::Capture(span, _, _)
        | Ast::CapturePlaceholder(span, _)
        | Ast::Semi(span, _) => Some(span),
    }
}

fn prepare_script_preload(
    script: Option<(&str, &str)>,
) -> Result<Option<PreparedScriptPreload>, ReplLoadError> {
    let Some((file_name, source)) = script else {
        return Ok(None);
    };

    let prepared = crate::prepare_script_sources(file_name, source, SourceKind::Script)
        .map_err(|e| preload_script_prepare_error(file_name, source, e))?;

    Ok(Some(PreparedScriptPreload {
        file_name: file_name.to_string(),
        source_for_parse: prepared.source_for_parse,
        include_modules: prepared.include_modules,
    }))
}

fn merge_user_preload_declarations(
    declaration_index: &mut sigil::DeclarationIndex,
    user_ast: &[Ast],
    user_module_path: &str,
    stage_index: usize,
    sources: &SourceRegistry,
    source_id: SourceId,
) -> Result<(), ReplLoadError> {
    if user_ast.is_empty() {
        return Ok(());
    }

    let stage = vec![sigil::StagedModuleAst {
        module_path: user_module_path.to_string(),
        doc_module_path: Some(user_module_path.to_string()),
        ast: user_ast.to_vec(),
        module_doc: None,
        auto_import: false,
        process_spec: None,
    }];
    let user_index = sigil::precollect_declaration_index(&[stage]).map_err(|e| {
        let spec = diagnostics::simple_error("ResolveError", &e.message, e.span, None);
        ReplLoadError::Diagnostic {
            phase: "resolve".to_string(),
            sources: sources.clone(),
            source_id,
            spec,
        }
    })?;

    for (fq_name, mut entry) in user_index {
        entry.stage_index = stage_index;
        declaration_index.insert(fq_name, entry);
    }
    Ok(())
}

fn preload_script_prepare_error(
    file_name: &str,
    source: &str,
    error: crate::ScriptSourcePrepareError,
) -> ReplLoadError {
    match error {
        crate::ScriptSourcePrepareError::Parse { message, span } => preload_script_diagnostic(
            file_name,
            source,
            diagnostics::parse_error_spec(source, &message, span),
        ),
        crate::ScriptSourcePrepareError::IncludeRead { message, span } => {
            preload_script_load_error(file_name, source, span, message)
        }
    }
}

fn preload_script_load_error(
    file_name: &str,
    source: &str,
    span: Span,
    message: String,
) -> ReplLoadError {
    preload_script_diagnostic(
        file_name,
        source,
        diagnostics::simple_error("LoadError", message, span, None),
    )
}

fn preload_script_diagnostic(
    file_name: &str,
    source: &str,
    spec: diagnostics::DiagnosticSpec,
) -> ReplLoadError {
    let mut sources = SourceRegistry::new();
    let source_id = sources.register(file_name, source.to_string());
    ReplLoadError::Diagnostic {
        phase: "parse".to_string(),
        sources,
        source_id,
        spec,
    }
}

fn preload_resolve_error(
    compile_sources: &crate::CompileSources,
    error: &sigil::error::ResolveError,
) -> ReplLoadError {
    let source_id = diagnostic_source_id(compile_sources, &error.span);
    let source = compile_sources.sources.source(source_id).unwrap_or("");
    ReplLoadError::Diagnostic {
        phase: "resolve".to_string(),
        sources: compile_sources.sources.clone(),
        source_id,
        spec: diagnostics::resolve_error_spec(
            source,
            &error.message,
            local_diagnostic_span(compile_sources, &error.span),
        ),
    }
}

fn bind_preload_script_qualified_names(
    sigil_session: &mut sigil::SigilSession,
    user_ast: &[Ast],
    module_path: &str,
) {
    for stmt in user_ast {
        let Some(name) = preload_decl_name(stmt) else {
            continue;
        };
        let Some(uid) = sigil_session.lookup_uid(name) else {
            continue;
        };
        let qualified = format!("{module_path}::{name}");
        if sigil_session.lookup_uid(&qualified).is_none() {
            sigil_session.define_with_id(&qualified, uid);
        }
    }
}

fn preload_decl_name(stmt: &Ast) -> Option<&str> {
    match stmt {
        Ast::Def(_, name, ..)
        | Ast::ExtractorDef(_, name, ..)
        | Ast::ConstDef(_, name, ..)
        | Ast::StructDef(_, name, ..)
        | Ast::RecordDef(_, name, ..)
        | Ast::DeferrorDef(_, name, ..)
        | Ast::EnumDef(_, name, ..)
        | Ast::TraitDef(_, name, ..) => Some(name.as_str()),
        _ => None,
    }
}

fn diagnostic_source_id(compile_sources: &crate::CompileSources, span: &Span) -> SourceId {
    if let Some((source_id, _)) = crate::decode_rebased_module_span(span) {
        return source_id;
    }
    compile_sources.user_source_id
}

fn local_diagnostic_span(compile_sources: &crate::CompileSources, span: &Span) -> Span {
    if let Some((_, local_span)) = crate::decode_rebased_module_span(span) {
        return local_span;
    }
    let source = compile_sources
        .sources
        .source(compile_sources.user_source_id)
        .unwrap_or("");
    if source.chars().count() >= span.end {
        span.clone()
    } else {
        Span { start: 0, end: 0 }
    }
}

fn history_value_for_result(
    vm: &eldr::InteractiveVm,
    value: &Value,
    meta: &forge::ChunkMeta,
) -> Value {
    if matches!(value, Value::Unit) {
        if let Some(binding) = meta.bindings.last() {
            if let Some(bound) = vm.get_local(binding.slot_id) {
                return bound;
            }
        }
    }
    value.clone()
}

fn ast_ty_contains_query_placeholder(ty: &AstTy) -> bool {
    match ty {
        AstTy::Named(_, name) => matches!(name.as_str(), "_" | "Hole"),
        AstTy::Generic(_, _, args) | AstTy::Tuple(_, args) => {
            args.iter().any(ast_ty_contains_query_placeholder)
        }
        AstTy::Func(_, params, ret) => {
            params.iter().any(ast_ty_contains_query_placeholder)
                || ast_ty_contains_query_placeholder(ret)
        }
        _ => false,
    }
}

fn collect_unresolved_pattern_binding_names(pat: &TypedPattern, names: &mut Vec<String>) {
    match pat {
        TypedPattern::Var(ty, id) => {
            if scar::type_contains_unresolved_vars(ty) {
                names.push(id.name.clone());
            }
        }
        TypedPattern::As(ty, inner, id) => {
            if scar::type_contains_unresolved_vars(ty) {
                names.push(id.name.clone());
            }
            collect_unresolved_pattern_binding_names(inner, names);
        }
        TypedPattern::ListCons(_, head, tail) => {
            collect_unresolved_pattern_binding_names(head, names);
            collect_unresolved_pattern_binding_names(tail, names);
        }
        TypedPattern::Tuple(_, items) | TypedPattern::Extractor { items, .. } => {
            for item in items {
                collect_unresolved_pattern_binding_names(item, names);
            }
        }
        TypedPattern::ResultOk(_, inner) => collect_unresolved_pattern_binding_names(inner, names),
        TypedPattern::Wildcard(_)
        | TypedPattern::Pin(_, _, _)
        | TypedPattern::ListNil(_)
        | TypedPattern::IntLit(_, _)
        | TypedPattern::StrLit(_, _)
        | TypedPattern::BoolLit(_, _)
        | TypedPattern::DurationLit(_, _) => {}
    }
}

fn unresolved_repl_binding_issue(typed: &[TypedNode]) -> Option<(Span, Vec<String>)> {
    fn binding_rhs_allows_unresolved_persistence(rhs: &TypedNode) -> bool {
        matches!(
            rhs.node,
            TypedInner::FacetPath(_) | TypedInner::PendingFacetPath(_)
        )
    }

    fn visit_stmt(stmt: &TypedNode) -> Option<(Span, Vec<String>)> {
        match &stmt.node {
            TypedInner::Bind(pat, rhs) | TypedInner::SafeBind(pat, rhs) => {
                if binding_rhs_allows_unresolved_persistence(rhs) {
                    return None;
                }
                let mut names = Vec::new();
                collect_unresolved_pattern_binding_names(pat, &mut names);
                (!names.is_empty()).then(|| (stmt.span.clone(), names))
            }
            TypedInner::Semi(inner) => visit_stmt(inner),
            _ => None,
        }
    }

    typed.iter().find_map(visit_stmt)
}

fn apply_preload_imports(
    sigil_session: &mut sigil::SigilSession,
    declaration_index: &sigil::DeclarationIndex,
    raw_module_stages: &[Vec<StagedModule>],
    module_stage_asts: &[Vec<sigil::StagedModuleAst>],
    user_ast: &[Ast],
    auto_import_modules: &BTreeSet<String>,
) -> Result<Vec<String>, ReplLoadError> {
    let mut imported_symbols = Vec::new();
    let current_stage_index = raw_module_stages.len().max(module_stage_asts.len());
    let auto_import_traits = declaration_index
        .values()
        .filter(|entry| entry.kind == sigil::DeclarationKind::Trait && entry.auto_import)
        .map(|entry| entry.name.clone())
        .collect::<BTreeSet<_>>();

    for stmt in user_ast {
        let Ast::Import(_span, path, spec) = stmt else {
            continue;
        };
        let module_name = path.segments.join("::");
        let canonical_module_name =
            preload_import_module_name(declaration_index, auto_import_modules, &module_name);
        if auto_import_modules.contains(&canonical_module_name)
            || auto_import_traits.contains(&module_name)
        {
            return Err(ReplLoadError::Load(LoadError::BootstrapFailed {
                phase: "resolve".into(),
                file_name: "<repl-preload>".into(),
                message: format!(
                    "Duplicate import: `{}` is auto-imported and cannot be explicitly imported",
                    module_name
                ),
            }));
        }

        match spec {
            ImportSpec::All => {
                for entry in declaration_index.values().filter(|entry| {
                    entry.module_path == canonical_module_name
                        && entry.stage_index < current_stage_index
                }) {
                    let uid = sigil_session.lookup_uid(&entry.fq_name).ok_or_else(|| {
                        ReplLoadError::Load(LoadError::BootstrapFailed {
                            phase: "resolve".into(),
                            file_name: "<repl-preload>".into(),
                            message: format!(
                                "Import target `{}` is not available in the current stage",
                                entry.fq_name
                            ),
                        })
                    })?;
                    sigil_session.define_with_id(&entry.name, uid);
                    imported_symbols.push(entry.name.clone());
                }
            }
            ImportSpec::Single(name) => {
                let fq_name = format!("{}::{}", canonical_module_name, name);
                let entry = declaration_index.get(&fq_name).ok_or_else(|| {
                    ReplLoadError::Load(LoadError::BootstrapFailed {
                        phase: "resolve".into(),
                        file_name: "<repl-preload>".into(),
                        message: format!("Unknown import member: {}::{}", module_name, name),
                    })
                })?;
                let uid = sigil_session.lookup_uid(&entry.fq_name).ok_or_else(|| {
                    ReplLoadError::Load(LoadError::BootstrapFailed {
                        phase: "resolve".into(),
                        file_name: "<repl-preload>".into(),
                        message: format!(
                            "Import target `{}` is not available in the current stage",
                            fq_name
                        ),
                    })
                })?;
                sigil_session.define_with_id(name, uid);
                imported_symbols.push(name.clone());
            }
            ImportSpec::List(names) => {
                for name in names {
                    let fq_name = format!("{}::{}", canonical_module_name, name);
                    let entry = declaration_index.get(&fq_name).ok_or_else(|| {
                        ReplLoadError::Load(LoadError::BootstrapFailed {
                            phase: "resolve".into(),
                            file_name: "<repl-preload>".into(),
                            message: format!("Unknown import member: {}::{}", module_name, name),
                        })
                    })?;
                    let uid = sigil_session.lookup_uid(&entry.fq_name).ok_or_else(|| {
                        ReplLoadError::Load(LoadError::BootstrapFailed {
                            phase: "resolve".into(),
                            file_name: "<repl-preload>".into(),
                            message: format!(
                                "Import target `{}` is not available in the current stage",
                                fq_name
                            ),
                        })
                    })?;
                    sigil_session.define_with_id(name, uid);
                    imported_symbols.push(name.clone());
                }
            }
        }
    }

    Ok(imported_symbols)
}

fn preload_import_module_name(
    declaration_index: &sigil::DeclarationIndex,
    auto_import_modules: &BTreeSet<String>,
    module_name: &str,
) -> String {
    if auto_import_modules.contains(module_name)
        || declaration_index
            .values()
            .any(|entry| entry.module_path == module_name)
    {
        return module_name.to_string();
    }
    if module_name.contains("::") {
        return module_name.to_string();
    }
    let canonical_name = format!("Global::{module_name}");
    if auto_import_modules.contains(&canonical_name)
        || declaration_index
            .values()
            .any(|entry| entry.module_path == canonical_name)
    {
        canonical_name
    } else {
        module_name.to_string()
    }
}

fn apply_preload_visible_names(user_ast: &[Ast], mut visible: Vec<String>) -> Vec<String> {
    for stmt in user_ast {
        match stmt {
            Ast::Def(_, name, ..)
            | Ast::ExtractorDef(_, name, ..)
            | Ast::ConstDef(_, name, ..)
            | Ast::StructDef(_, name, ..)
            | Ast::RecordDef(_, name, ..)
            | Ast::DeferrorDef(_, name, ..)
            | Ast::EnumDef(_, name, ..)
            | Ast::TraitDef(_, name, ..) => visible.push(name.clone()),
            _ => {}
        }
    }

    visible
}

impl ReplEngine {
    fn sync_scar_fun_index_with_vm(&mut self) {
        let mut function_indices = HashMap::new();
        for entry in &self.vm.bytecode().functions {
            let Some(qualified_name) = entry.qualified_name.as_deref() else {
                continue;
            };
            function_indices.insert(qualified_name.to_string(), entry.fun_idx);
            if let Some(short_name) = qualified_name.strip_prefix("__Repl::Session::") {
                function_indices.insert(short_name.to_string(), entry.fun_idx);
            }
        }
        self.scar_session.reconcile_function_indices(
            function_indices
                .iter()
                .map(|(qualified_name, fun_idx)| (qualified_name.as_str(), *fun_idx)),
        );
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

    fn sync_repl_chunk_function_indices(
        &mut self,
        function_names: &[String],
        functions: &[sindr::ir::FunctionEntry],
    ) {
        let visible_function_indices = function_names
            .iter()
            .filter_map(|name| {
                let uid = self.sigil_session.lookup_uid(name)?;
                let fun_idx = functions
                    .iter()
                    .rev()
                    .find(|entry| {
                        entry
                            .qualified_name
                            .as_deref()
                            .is_some_and(|qualified_name| {
                                self.sigil_session.lookup_uid(qualified_name) == Some(uid)
                                    || (Self::symbol_matches(qualified_name, name)
                                        && entry.signature.as_deref().is_some_and(|signature| {
                                            signature.starts_with(&format!("{name}("))
                                        }))
                            })
                    })
                    .map(|entry| entry.fun_idx)?;
                Some((uid, fun_idx))
            })
            .collect::<Vec<_>>();
        self.scar_session
            .reconcile_visible_function_indices(visible_function_indices);
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
    compile_unit_kind: CompileUnitKind,
) -> Result<Vec<Vec<sigil::StagedModuleAst>>, ModuleStageParseError> {
    let mut staged_module_asts = Vec::with_capacity(module_stages.len());
    let mut seen_module_paths: HashMap<String, SeenModulePath> = HashMap::new();

    for stage in module_stages {
        let mut stage_ast = Vec::new();
        let parsed_stage = parse_stage_modules_parallel(sources, stage, compile_unit_kind);
        for (module, lowered_modules) in stage.iter().zip(parsed_stage) {
            for lowered in lowered_modules? {
                if !lowered.module_path.is_empty() {
                    let is_impl_owner = crate::lowered_module_is_impl_owner(&lowered);
                    let second_file_name = sources
                        .file_name(module.source_id)
                        .unwrap_or("<unknown>")
                        .to_string();
                    if let Some(seen) = seen_module_paths.get(&lowered.module_path) {
                        if !is_impl_owner && seen.has_normal_module() {
                            return Err(ModuleStageParseError {
                                source_id: module.source_id,
                                kind: ModuleStageParseErrorKind::DuplicateModulePath {
                                    module_path: lowered.module_path.clone(),
                                    first_file_name: seen
                                        .first_normal_file_name()
                                        .unwrap_or_else(|| seen.first_file_name())
                                        .to_string(),
                                    second_file_name,
                                    span: lowered
                                        .declared_span
                                        .unwrap_or(spire::ast::Span { start: 0, end: 0 }),
                                },
                            });
                        }
                    }
                    seen_module_paths
                        .entry(lowered.module_path.clone())
                        .and_modify(|seen| seen.record(second_file_name.clone(), is_impl_owner))
                        .or_insert_with(|| SeenModulePath::new(second_file_name, is_impl_owner));
                }

                stage_ast.push(sigil::StagedModuleAst {
                    module_path: lowered.module_path,
                    doc_module_path: lowered.doc_module_path,
                    ast: crate::rebase_module_ast_spans(lowered.ast, module.source_id),
                    module_doc: lowered.module_doc,
                    auto_import: lowered.auto_import,
                    process_spec: lowered.process_spec,
                });
            }
        }
        staged_module_asts.push(stage_ast);
    }

    Ok(staged_module_asts)
}

#[derive(Debug, Clone)]
struct SeenModulePath {
    first_file_name: String,
    first_normal_file_name: Option<String>,
}

impl SeenModulePath {
    fn new(file_name: String, is_impl_owner: bool) -> Self {
        Self {
            first_normal_file_name: (!is_impl_owner).then(|| file_name.clone()),
            first_file_name: file_name,
        }
    }

    fn record(&mut self, file_name: String, is_impl_owner: bool) {
        if !is_impl_owner && self.first_normal_file_name.is_none() {
            self.first_normal_file_name = Some(file_name);
        }
    }

    fn has_normal_module(&self) -> bool {
        self.first_normal_file_name.is_some()
    }

    fn first_file_name(&self) -> &str {
        &self.first_file_name
    }

    fn first_normal_file_name(&self) -> Option<&str> {
        self.first_normal_file_name.as_deref()
    }
}

fn parse_stage_modules_parallel(
    sources: &SourceRegistry,
    stage: &[StagedModule],
    compile_unit_kind: CompileUnitKind,
) -> Vec<Result<Vec<crate::LoweredModuleAst>, ModuleStageParseError>> {
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(stage.len());
        for module in stage {
            handles.push(
                std::thread::Builder::new()
                    .stack_size(STAGE_PARSE_WORKER_STACK_SIZE)
                    .spawn_scoped(scope, move || {
                        let module_source = sources.source(module.source_id).unwrap_or("");
                        let parsed = spire::parse_with_context(
                            module_source,
                            crate::derive_parser_context(
                                module.source_id.0,
                                module.source_kind,
                                compile_unit_kind,
                                (module.module_path == "Facet").then(|| "Facet".into()),
                            ),
                        )
                        .map_err(|e| ModuleStageParseError {
                            source_id: module.source_id,
                            kind: ModuleStageParseErrorKind::Parse {
                                message: e.message().to_string(),
                                span: e.span().clone(),
                            },
                        })?;
                        let fallback_module_path = sigil::const_only_fallback_module_path(
                            &parsed,
                            Some(module.module_path.as_str()),
                        );
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

fn clamp_to_char_boundary(input: &str, mut cursor: usize) -> usize {
    cursor = cursor.min(input.len());
    while cursor > 0 && !input.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

pub(crate) fn completion_token(input: &str, cursor: usize) -> (usize, usize, String) {
    let cursor = clamp_to_char_boundary(input, cursor);
    let before = &input[..cursor];
    let start = before
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| (!completion_token_char(ch)).then_some(idx + ch.len_utf8()))
        .unwrap_or(0);
    (start, cursor, input[start..cursor].to_string())
}

fn completion_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionLexState {
    Code,
    String { escaped: bool },
    Interpolation { brace_depth: usize },
    InterpolationString { brace_depth: usize, escaped: bool },
}

pub(crate) fn completion_allowed_at_cursor(input: &str, cursor: usize) -> bool {
    let cursor = clamp_to_char_boundary(input, cursor.min(input.len()));
    let before = &input[..cursor];
    let mut state = CompletionLexState::Code;
    let mut chars = before.char_indices().peekable();

    while let Some((_idx, ch)) = chars.next() {
        state = match state {
            CompletionLexState::Code => match ch {
                '"' => CompletionLexState::String { escaped: false },
                _ => CompletionLexState::Code,
            },
            CompletionLexState::String { escaped } => {
                if escaped {
                    CompletionLexState::String { escaped: false }
                } else if ch == '\\' {
                    CompletionLexState::String { escaped: true }
                } else if ch == '"' {
                    CompletionLexState::Code
                } else if ch == '#' && chars.peek().is_some_and(|(_, next)| *next == '{') {
                    chars.next();
                    CompletionLexState::Interpolation { brace_depth: 1 }
                } else {
                    CompletionLexState::String { escaped: false }
                }
            }
            CompletionLexState::Interpolation { brace_depth } => match ch {
                '"' => CompletionLexState::InterpolationString {
                    brace_depth,
                    escaped: false,
                },
                '{' => CompletionLexState::Interpolation {
                    brace_depth: brace_depth + 1,
                },
                '}' if brace_depth <= 1 => CompletionLexState::String { escaped: false },
                '}' => CompletionLexState::Interpolation {
                    brace_depth: brace_depth - 1,
                },
                _ => CompletionLexState::Interpolation { brace_depth },
            },
            CompletionLexState::InterpolationString {
                brace_depth,
                escaped,
            } => {
                if escaped {
                    CompletionLexState::InterpolationString {
                        brace_depth,
                        escaped: false,
                    }
                } else if ch == '\\' {
                    CompletionLexState::InterpolationString {
                        brace_depth,
                        escaped: true,
                    }
                } else if ch == '"' {
                    CompletionLexState::Interpolation { brace_depth }
                } else {
                    CompletionLexState::InterpolationString {
                        brace_depth,
                        escaped: false,
                    }
                }
            }
        };
    }

    matches!(
        state,
        CompletionLexState::Code | CompletionLexState::Interpolation { .. }
    )
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

fn signature_return_type(signature: &str) -> Option<&str> {
    signature.rsplit_once("->").map(|(_, ret)| ret.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eldr::interactive::InteractiveChunkPolicy;
    use eldr::value::CallableTarget;
    use sindr::ir::{
        BootEntrySource, BytecodeChunk, Constant, Opcode, RuntimeBootPlan, SingletonBootEntry,
    };
    use sindr::runtime::{TypeEntry, TypeKind};
    #[cfg(unix)]
    use std::io::{Read, Write};
    #[cfg(unix)]
    use std::os::fd::FromRawFd;
    #[cfg(unix)]
    use std::sync::Mutex;

    #[cfg(unix)]
    static STDERR_CAPTURE_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(unix)]
    extern "C" {
        fn close(fd: i32) -> i32;
        fn dup(fd: i32) -> i32;
        fn dup2(src: i32, dst: i32) -> i32;
        fn pipe(fds: *mut i32) -> i32;
    }

    #[cfg(unix)]
    fn capture_process_stderr<T>(f: impl FnOnce() -> T) -> (T, String) {
        let _guard = STDERR_CAPTURE_LOCK.lock().expect("stderr capture lock");
        let mut pipe_fds = [0_i32; 2];
        unsafe {
            assert_eq!(pipe(pipe_fds.as_mut_ptr()), 0, "pipe should succeed");
            let saved_stderr = dup(2);
            assert!(saved_stderr >= 0, "dup stderr should succeed");
            assert_eq!(dup2(pipe_fds[1], 2), 2, "redirect stderr should succeed");
            assert_eq!(close(pipe_fds[1]), 0, "close write pipe should succeed");

            let result = f();
            std::io::stderr().flush().expect("flush stderr");

            assert_eq!(dup2(saved_stderr, 2), 2, "restore stderr should succeed");
            assert_eq!(close(saved_stderr), 0, "close saved stderr should succeed");

            let mut stderr = String::new();
            let mut reader = std::fs::File::from_raw_fd(pipe_fds[0]);
            reader
                .read_to_string(&mut stderr)
                .expect("read captured stderr");
            (result, stderr)
        }
    }

    fn interactive_test_chunk() -> BytecodeChunk {
        BytecodeChunk {
            opcodes: vec![Opcode::LoadConst(0), Opcode::Halt],
            source_map: None,
            const_base: 0,
            constants: vec![Constant::Int(sindr::primitives::int(1))],
            new_locals: 0,
            type_registry_base: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
        }
    }

    fn bootstrap_engine_with_module(source: &str, module_path: &str) -> ReplEngine {
        let repl_sources =
            loader::collect_repl_sources_with_module_stages(&[vec![crate::ModuleInput {
                file_name: "lib/bad.srt".into(),
                source: source.into(),
                module_path: module_path.into(),
            }]])
            .expect("test module stage should load");
        let forge_session = forge::ForgeSession::new();
        let vm = session::empty_interactive_vm(forge_session.type_registry());

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
            startup_results: Vec::new(),
            results: Vec::new(),
            result_metas: Vec::new(),
            symbols: ["Ok", "Err"]
                .into_iter()
                .map(str::to_string)
                .chain(
                    builtin_function_metas()
                        .iter()
                        .map(|meta| meta.name.to_string()),
                )
                .collect(),
            docs: Vec::new(),
            signatures: Vec::new(),
            process_metadata: BTreeMap::new(),
            auto_import_modules: BTreeSet::new(),
            auto_import_records: Vec::new(),
            reload_seed: ReplReloadSeed::Empty,
            replay_inputs: Vec::new(),
            history_entries: Vec::new(),
            binding_records: Vec::new(),
            import_records: Vec::new(),
            def_records: Vec::new(),
            completion_context_cache: RefCell::new(None),
            #[cfg(test)]
            completion_context_builds: Cell::new(0),
            error_display_mode: ErrorDisplayMode::Full,
            stack_trace_display_mode: StackTraceDisplayMode::Off,
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
    fn repl_help_lists_q_quit_alias() {
        let mut engine = ReplEngine::new().expect("engine should initialize");

        let help = engine.handle_line(":help");
        let rendered = ReplEngine::repl_result_text(&help);

        assert!(rendered.contains(":quit, :exit, :q"), "{rendered}");
    }

    #[test]
    #[cfg(unix)]
    fn handle_line_type_error_returns_diagnostic_without_writing_process_stderr() {
        let mut engine = ReplEngine::new().expect("engine should initialize");

        let (result, stderr) = capture_process_stderr(|| engine.handle_line("bad: Int = \"oops\""));
        let rendered = ReplEngine::repl_result_text(&result);

        assert!(
            stderr.is_empty(),
            "REPL core wrote directly to process stderr:\n{stderr}"
        );
        assert!(matches!(result.output, ReplOutput::EvalError { .. }));
        assert!(rendered.contains("expected Int"), "{rendered}");
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
        engine.vm = session::empty_interactive_vm(engine.forge_session.type_registry());
        engine
            .vm
            .push_chunk(
                sindr::ir::BytecodeChunk {
                    opcodes: vec![sindr::ir::Opcode::Halt],
                    source_map: None,
                    const_base: 0,
                    constants: vec![sindr::ir::Constant::Int(sindr::primitives::int(1))],
                    new_locals: 0,
                    type_registry_base: 0,
                    type_entries: Vec::new(),
                    dbg_template_base: 0,
                    dbg_templates: Vec::new(),
                    error_template_base: 0,
                    error_templates: Vec::new(),
                    callable_templates: Vec::new(),
                    functions: Vec::new(),
                    docs: Vec::new(),
                    signatures: Vec::new(),
                    runtime_process_specs: Vec::new(),
                    runtime_boot_plan: Default::default(),
                },
                InteractiveChunkPolicy::ReplAppendOnly,
            )
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

    #[test]
    fn partial_capture_chain_keeps_outer_capture_arity_in_vm() {
        let mut engine = ReplEngine::new().expect("engine should initialize");

        let def = engine.handle_line("def f(a: Int, b: Int, c: Int) -> Int { a + b + c }");
        assert!(!def.should_exit);
        let f3 = engine.handle_line("f3 = &f");
        assert!(!f3.should_exit);
        let f2 = engine.handle_line("f2 = &f3(&1, &2, 3)");
        assert!(!f2.should_exit);
        let f1 = engine.handle_line("f1 = &f2(&1, 2)");
        assert!(!f1.should_exit);

        let bytecode = engine.vm.snapshot_bytecode();
        let f_entry = bytecode
            .functions
            .iter()
            .find(|entry| entry.signature.as_deref() == Some("f(a: Int, b: Int, c: Int) -> Int"))
            .expect("repl def f should be present in bytecode");
        assert_eq!(f_entry.arity, 3, "{f_entry:?}");

        let binding = engine.binding_info("f2").expect("f2 binding should exist");
        let value = engine
            .vm
            .get_local(binding.slot_id)
            .expect("f2 value should be stored");
        let Value::Callable(callable) = value else {
            panic!("expected callable binding, got {value:?}");
        };
        assert_eq!(callable.lexical_captures.len(), 1, "{callable:?}");

        let CallableTarget::Function(fun_idx) = callable.target else {
            panic!(
                "expected function callable target, got {:?}",
                callable.target
            );
        };
        let entry = bytecode
            .functions
            .get(fun_idx as usize)
            .expect("callable target function should exist");
        assert_eq!(entry.fun_idx, fun_idx, "{entry:?}");
        assert_eq!(entry.arity, 3, "{entry:?}");
        let Some(Value::Callable(inner)) = callable.lexical_captures.first() else {
            panic!(
                "expected f2 to capture f3 callable, got {:?}",
                callable.lexical_captures
            );
        };
        let CallableTarget::Function(fun_idx) = inner.target.clone() else {
            panic!(
                "expected captured f3 function target, got {:?}",
                inner.target
            );
        };
        let entry = bytecode
            .functions
            .get(fun_idx as usize)
            .expect("captured f3 function should exist");
        assert_eq!(
            fun_idx, f_entry.fun_idx,
            "captured f3 should target repl f: {f_entry:?}"
        );
        assert_eq!(entry.fun_idx, fun_idx, "{entry:?}");
        assert_eq!(entry.arity, 3, "{entry:?}");

        let binding = engine.binding_info("f1").expect("f1 binding should exist");
        let value = engine
            .vm
            .get_local(binding.slot_id)
            .expect("f1 value should be stored");
        let Value::Callable(callable) = value else {
            panic!("expected callable binding, got {value:?}");
        };
        assert_eq!(callable.lexical_captures.len(), 1, "{callable:?}");
        let Some(Value::Callable(inner)) = callable.lexical_captures.first() else {
            panic!(
                "expected f1 to capture f2 callable, got {:?}",
                callable.lexical_captures
            );
        };
        let CallableTarget::Function(fun_idx) = inner.target.clone() else {
            panic!(
                "expected captured f2 function target, got {:?}",
                inner.target
            );
        };
        let entry = bytecode
            .functions
            .get(fun_idx as usize)
            .expect("captured f2 function should exist");
        assert_eq!(entry.fun_idx, fun_idx, "{entry:?}");
        assert_eq!(entry.arity, 3, "{entry:?}");
        let Some(Value::Callable(inner_f3)) = inner.lexical_captures.first() else {
            panic!(
                "expected captured f2 to retain f3 callable, got {:?}",
                inner.lexical_captures
            );
        };
        let CallableTarget::Function(fun_idx) = inner_f3.target.clone() else {
            panic!(
                "expected retained f3 function target, got {:?}",
                inner_f3.target
            );
        };
        let entry = bytecode
            .functions
            .get(fun_idx as usize)
            .expect("retained f3 function should exist");
        assert_eq!(entry.fun_idx, fun_idx, "{entry:?}");
        assert_eq!(entry.arity, 3, "{entry:?}");

        let applied = engine.handle_line("f1(10)");
        let applied_text = ReplEngine::repl_result_text(&applied);
        assert!(applied_text.contains("15"), "{applied_text}");
    }

    #[test]
    fn completion_context_is_memoized_until_repl_state_changes() {
        let mut engine = ReplEngine::new().expect("engine should initialize");

        let baseline = engine.completion_context_build_count();
        let first = engine.completion_context();
        let second = engine.completion_context();

        assert_eq!(first, second);
        assert_eq!(engine.completion_context_build_count(), baseline + 1);

        let bind = engine.handle_line("value = 1");
        assert!(!bind.should_exit, "{}", ReplEngine::repl_result_text(&bind));

        let after_mutation = engine
            .cached_completion_context()
            .expect("completion cache should stay available after commit");
        assert!(
            after_mutation
                .completions("value(", "value(".len())
                .signature
                .is_none(),
            "plain value bindings should not become callable signatures"
        );
        assert_eq!(engine.completion_context_build_count(), baseline + 1);

        let after_cached_reuse = engine.completion_context();
        assert_eq!(after_mutation, after_cached_reuse);
        assert_eq!(engine.completion_context_build_count(), baseline + 1);
    }

    #[test]
    fn cached_completion_context_keeps_new_binding_after_commit() {
        let mut engine = ReplEngine::new().expect("engine should initialize");
        let _ = engine.completion_context();

        let bind = engine.handle_line("value = 1");
        assert!(!bind.should_exit, "{}", ReplEngine::repl_result_text(&bind));

        let cached = engine
            .cached_completion_context()
            .expect("completion cache should remain available after commit");
        let labels = cached
            .completions("val", 3)
            .candidates
            .into_iter()
            .map(|candidate| candidate.label)
            .collect::<Vec<_>>();
        assert!(
            labels.iter().any(|label| label == "value"),
            "cached completion context should include new binding: {labels:?}"
        );
    }

    #[test]
    fn core_completion_uses_live_imported_short_names_from_session_scope() {
        let mut engine = ReplEngine::from_module_source(
            "helper.srt",
            r#"defmod Helper {
  def helper() -> Int { 1 }
}"#,
        )
        .expect("module source should initialize");

        let imported = engine.handle_line("import Helper::helper");
        assert!(
            !imported.should_exit,
            "{}",
            ReplEngine::repl_result_text(&imported)
        );

        let semantic_labels = engine
            .semantic_index()
            .symbols()
            .iter()
            .map(|symbol| symbol.label.clone())
            .collect::<Vec<_>>();
        assert!(
            semantic_labels.iter().any(|label| label == "helper"),
            "semantic index should include imported short symbol: {semantic_labels:?}"
        );

        let completion_labels = engine
            .completions("hel", 3)
            .candidates
            .into_iter()
            .map(|candidate| candidate.label)
            .collect::<Vec<_>>();
        assert!(
            completion_labels.iter().any(|label| label == "helper"),
            "completion should expose imported short symbol: {completion_labels:?}"
        );

        let signature_lines = engine
            .completion_context()
            .completions("helper(", "helper(".len())
            .signature
            .map(|signature| signature.lines)
            .unwrap_or_default();
        assert!(
            signature_lines
                .iter()
                .any(|line| line.contains("helper() -> Int")),
            "signature help should resolve imported short callable: {signature_lines:?}"
        );
    }

    #[test]
    fn repl_session_rejects_top_level_def_capturing_existing_value_binding() {
        let mut engine = ReplEngine::new().expect("engine should initialize");

        let bind = engine.handle_line("x = 1");
        assert!(!bind.should_exit);
        assert!(engine.sigil_session.lookup_uid("x").is_some());

        let ast = spire::parse("def f() -> Int { x }").expect("parse failed");
        let err = engine
            .sigil_session
            .resolve(ast)
            .expect_err("top-level capture must fail");
        assert!(
            err.message
                .contains("Top-level definition `f` cannot reference value binding `x`"),
            "{}",
            err.message
        );
    }

    #[test]
    fn repl_session_phase_maps_to_interactive_vm_policy() {
        assert_eq!(
            ReplSessionPhase::Bootstrap.execution_policy(),
            InteractiveChunkPolicy::Preload
        );
        assert_eq!(
            ReplSessionPhase::Preload.execution_policy(),
            InteractiveChunkPolicy::Preload
        );
        assert_eq!(
            ReplSessionPhase::Live.execution_policy(),
            InteractiveChunkPolicy::ReplAppendOnly
        );
    }

    #[test]
    fn bootstrap_phase_allows_structural_vm_growth() {
        let mut engine = ReplEngine::new().expect("engine should initialize");
        let mut chunk = interactive_test_chunk();
        chunk.const_base = engine.vm.bytecode().constants.len() as u32;
        chunk.error_template_base = engine.vm.bytecode().error_templates.len() as u32;
        chunk.type_registry_base = engine.vm.bytecode().type_registry.entries().len() as u32;
        let next_tag = engine
            .vm
            .bytecode()
            .type_registry
            .entries()
            .iter()
            .map(|entry| entry.tag)
            .max()
            .unwrap_or(1)
            + 1;
        chunk.type_entries.push(TypeEntry {
            tag: next_tag,
            name: "Extra".into(),
            kind: TypeKind::Struct,
            field_names: Vec::new(),
            private_flags: Vec::new(),
        });

        let execution = engine
            .execute_vm_chunk(chunk, ReplSessionPhase::Bootstrap)
            .expect("bootstrap phase should allow preload-style chunk");

        assert_eq!(execution.value, Value::Int(sindr::primitives::int(1)));
    }

    #[test]
    fn live_phase_rejects_runtime_boot_plan_growth() {
        let mut engine = ReplEngine::new().expect("engine should initialize");
        let mut chunk = interactive_test_chunk();
        chunk.runtime_boot_plan = RuntimeBootPlan {
            singletons: vec![SingletonBootEntry {
                process_name: "Counter".into(),
                init_timeout_ms: 5000,
                source: BootEntrySource::ExplicitConfig,
            }],
            ..RuntimeBootPlan::default()
        };

        let err = engine
            .execute_vm_chunk(chunk, ReplSessionPhase::Live)
            .expect_err("live phase should reject runtime boot plan growth");

        assert!(err.message.contains("runtime_boot_plan"), "{}", err.message);
    }

    #[test]
    fn repl_singleton_cast_accepts_explicit_pid_argument() {
        let mut engine = ReplEngine::from_script_source(
            "ticker_preload.srt",
            r#"
defgenserver Ticker {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @call
  def value(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }

  @cast
  def tick(state: Int) -> Result<CastResult<Int>> {
    Ok(CastResult::Next(state + 1))
  }
}

supervisor_init {
  Ticker {}
}
"#,
        )
        .expect("preloaded singleton process should initialize");

        let bind = engine.handle_line("p = Ticker::pid()");
        assert!(!bind.should_exit, "{}", ReplEngine::repl_result_text(&bind));

        let tick = engine.handle_line("Ticker::tick(p)");
        assert!(!tick.should_exit, "{}", ReplEngine::repl_result_text(&tick));
        assert!(
            !matches!(
                tick.output,
                ReplOutput::EvalError { .. } | ReplOutput::Diagnostic { .. }
            ),
            "{}",
            ReplEngine::repl_result_text(&tick)
        );
    }

    #[test]
    fn core_type_identity_reports_repl_surface_contract() {
        assert_eq!(
            ReplTypeDisplayCategory::FacetPath.identity_label(),
            "TypeIdentity::FacetPath"
        );
        assert_eq!(
            ReplTypeDisplayCategory::Closure.identity_label(),
            "TypeIdentity::Closure"
        );
        assert_eq!(
            ReplTypeDisplayCategory::Struct.identity_label(),
            "TypeIdentity::Struct"
        );
    }
}
