mod command_error;
pub mod error_display;
mod loader;
mod project_runner;
pub mod repl;
pub mod tui;

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub use command_error::{CommandDiagnostic, CommandError, CommandResult};
pub use error_display::ErrorDisplayMode;
use loader::stdlib_module_spec_cache_key;
pub use loader::{
    collect_additional_default_std_module_inputs, collect_lib_module_inputs,
    collect_module_sources_with_extra_std_sources, collect_module_sources_with_module_file_stages,
    collect_module_sources_with_module_stages, collect_module_sources_with_modules,
    collect_module_sources_with_stdlib_variant, collect_script_include_directives,
    collect_test_module_sources_with_module_stages, compose_script_compile_sources,
    compose_script_compile_sources_with_stdlib_variant, derive_primary_module_path,
    is_default_std_module_file_name, is_default_std_module_path,
    module_path_from_source_or_file_name, prepare_script_sources, script_pseudo_module_path,
    CompileSources, LoadError, ModuleInput, ModuleSources, PreparedScriptSources,
    ScriptIncludeDirective, ScriptSourcePrepareError, SourceDescriptor, StagedModule,
    StdlibVariant,
};
pub use project_runner::{
    execute_project_runner_source, project_runner_module_input_stages, ProjectRunnerVmError,
};

use diagnostics::SourceId;
pub use repl::logic::core::{
    CompletionTelemetry, EldrLoadError, ReplCompletionContext, ReplEngine, ReplLoadError,
};
pub use repl::ui::cli::{cli_command, BannerMode, ReplOptions};
pub use repl::ui::completion::{
    ReplCompletionProvider, ReplCompletionRequest, ReplCompletionResult,
};
use serde::{Deserialize, Serialize};
use sindr::builtin::{builtin_function_metas, builtin_type_head_metas};
use sindr::ir::{stable_hash_hex, DocEntry, SignatureEntry};
pub use sindr::policy::SourceKind;
use sindr::policy::{
    CompileUnitKind, EntryPoint, RuntimeSourcePolicy, SOURCE_POLICY_SCHEMA_VERSION,
};

pub const MODULE_SPAN_STRIDE: usize = 1_000_000;

pub(crate) fn surface_path_name(name: &str) -> &str {
    sindr::names::surface_path_name(name)
}

pub(crate) fn surface_rendered_name(name: &str) -> String {
    sindr::names::surface_rendered_name(name)
}

fn stable_hash_bytes(bytes: &[u8]) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

fn stdlib_variant_cache_key(stdlib_variant: StdlibVariant) -> &'static str {
    match stdlib_variant {
        StdlibVariant::Default => "default",
        StdlibVariant::TestEnabled => "test-enabled",
    }
}

pub fn module_span_base_for_source(source_id: SourceId) -> usize {
    (source_id.0 as usize + 1) * MODULE_SPAN_STRIDE
}

pub fn rebase_module_ast_spans(
    ast: Vec<spire::ast::Ast>,
    source_id: SourceId,
) -> Vec<spire::ast::Ast> {
    spire::rebase_ast_spans(ast, module_span_base_for_source(source_id))
}

pub fn decode_rebased_module_span(span: &spire::ast::Span) -> Option<(SourceId, spire::ast::Span)> {
    if span.start < MODULE_SPAN_STRIDE {
        return None;
    }
    let bucket = span.start / MODULE_SPAN_STRIDE;
    if bucket == 0 {
        return None;
    }
    let base = bucket * MODULE_SPAN_STRIDE;
    Some((
        SourceId((bucket - 1) as u32),
        spire::ast::Span {
            start: span.start.saturating_sub(base),
            end: span.end.saturating_sub(base),
        },
    ))
}

// ── Public types used by other crates ────────────────────────────────────────

pub use sigil::LoweredModuleAst;

pub(crate) fn lowered_module_is_impl_owner(lowered: &LoweredModuleAst) -> bool {
    sigil::lowered_module_is_impl_owner(lowered)
}

fn format_struct_signature(name: &str) -> String {
    format!("defstruct {}", surface_path_name(name))
}

fn format_record_signature(name: &str) -> String {
    format!("defrecord {}", surface_path_name(name))
}

/// Collect doc metadata from lowered std/user modules so it can be attached to
/// REPL chunks and serialized `.eldr` artifacts.
pub fn collect_doc_entries(
    module_stages: &[Vec<sigil::StagedModuleAst>],
    user_ast: &[spire::ast::Ast],
    user_module_path: Option<&str>,
) -> Vec<DocEntry> {
    sigil::collect_doc_entries(module_stages, user_ast, user_module_path)
}

pub fn collect_signature_entries(
    module_stages: &[Vec<sigil::StagedModuleAst>],
    user_ast: &[spire::ast::Ast],
    user_module_path: Option<&str>,
) -> Vec<SignatureEntry> {
    sigil::collect_signature_entries(module_stages, user_ast, user_module_path)
}

/// Collect doc metadata while reusing already-collected prefix docs, such as
/// the default stdlib docs stored in the semantic snapshot.
pub fn collect_doc_entries_with_base(
    base_docs: &[DocEntry],
    module_stages: &[Vec<sigil::StagedModuleAst>],
    user_ast: &[spire::ast::Ast],
    user_module_path: Option<&str>,
) -> Vec<DocEntry> {
    sigil::collect_doc_entries_with_base(base_docs, module_stages, user_ast, user_module_path)
}

pub fn collect_signature_entries_with_base(
    base_signatures: &[SignatureEntry],
    module_stages: &[Vec<sigil::StagedModuleAst>],
    user_ast: &[spire::ast::Ast],
    user_module_path: Option<&str>,
) -> Vec<SignatureEntry> {
    sigil::collect_signature_entries_with_base(
        base_signatures,
        module_stages,
        user_ast,
        user_module_path,
    )
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
    pub source_id: diagnostics::SourceId,
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

pub fn derive_source_policy(
    compile_unit_kind: CompileUnitKind,
    source_kind: SourceKind,
    entrypoint: Option<&EntryPoint>,
) -> sindr::policy::SourcePolicy {
    source_kind.policy(compile_unit_kind, entrypoint)
}

pub fn derive_parse_rules(source_kind: SourceKind) -> spire::ParseRules {
    let policy = derive_source_policy(CompileUnitKind::DefinitionCheck, source_kind, None);
    spire::parse_rules_for_source_policy(&policy)
}

pub fn derive_parser_context(
    source_id: u32,
    source_kind: SourceKind,
    compile_unit_kind: CompileUnitKind,
    module_path: Option<String>,
) -> spire::ParserContext {
    spire::parser_context_for_source_policy(
        source_id,
        derive_source_policy(compile_unit_kind, source_kind, None),
        module_path,
    )
}

pub fn derive_runtime_policy(
    compile_unit_kind: CompileUnitKind,
    source_kind: SourceKind,
    entrypoint: Option<&EntryPoint>,
) -> RuntimeSourcePolicy {
    derive_source_policy(compile_unit_kind, source_kind, entrypoint).runtime_policy
}

pub fn lower_module_source_ast(
    ast: Vec<spire::ast::Ast>,
    fallback_module_path: Option<&str>,
) -> Vec<LoweredModuleAst> {
    sigil::lower_module_source_ast(ast, fallback_module_path)
}

pub fn extract_process_modules_from_user_ast(
    user_ast: Vec<spire::ast::Ast>,
) -> (Vec<sigil::StagedModuleAst>, Vec<spire::ast::Ast>) {
    sigil::extract_process_modules_from_user_ast(user_ast)
}

pub fn parse_module_stages_from_compile_sources(
    compile_sources: &CompileSources,
    compile_unit_kind: CompileUnitKind,
) -> Result<Vec<Vec<sigil::StagedModuleAst>>, ModuleStageParseError> {
    repl::logic::core::parse_module_stages_from_sources(
        &compile_sources.sources,
        &compile_sources.module_stages,
        compile_unit_kind,
    )
}

pub fn parse_module_stages_from_compile_sources_suffix(
    compile_sources: &CompileSources,
    compile_unit_kind: CompileUnitKind,
    start_stage_index: usize,
) -> Result<Vec<Vec<sigil::StagedModuleAst>>, ModuleStageParseError> {
    let suffix = compile_sources
        .module_stages
        .iter()
        .skip(start_stage_index)
        .cloned()
        .collect::<Vec<_>>();
    repl::logic::core::parse_module_stages_from_sources(
        &compile_sources.sources,
        &suffix,
        compile_unit_kind,
    )
}

pub struct ExpandedSnapshotModuleStages<'a> {
    pub module_stages: std::borrow::Cow<'a, [Vec<sigil::StagedModuleAst>]>,
    default_stage_count: usize,
}

impl<'a> ExpandedSnapshotModuleStages<'a> {
    pub fn suffix_module_stages(&self) -> &[Vec<sigil::StagedModuleAst>] {
        if self.module_stages.len() > self.default_stage_count {
            &self.module_stages[self.default_stage_count..]
        } else {
            &[]
        }
    }
}

pub fn expand_snapshot_module_stages<'a>(
    compile_sources: &CompileSources,
    snapshot: &'a DefaultStdlibSnapshot,
    compile_unit_kind: CompileUnitKind,
) -> Result<ExpandedSnapshotModuleStages<'a>, ModuleStageParseError> {
    let mut module_stages = std::borrow::Cow::Borrowed(snapshot.module_stages.as_slice());
    let mut suffix_module_stages = parse_module_stages_from_compile_sources_suffix(
        compile_sources,
        compile_unit_kind,
        snapshot.default_stage_count,
    )?;
    if !suffix_module_stages.is_empty() {
        module_stages.to_mut().append(&mut suffix_module_stages);
    }
    Ok(ExpandedSnapshotModuleStages {
        module_stages,
        default_stage_count: snapshot.default_stage_count,
    })
}

#[derive(Debug, Clone)]
pub struct StagedCompilationSnapshot {
    pub module_stages: Vec<Vec<sigil::StagedModuleAst>>,
    pub compile_prefix: CompilationPrefixSnapshot,
    pub docs: Vec<DocEntry>,
    pub signatures: Vec<SignatureEntry>,
    pub auto_import_modules: BTreeSet<String>,
    pub default_stage_count: usize,
}

/// Backward-compatible name for the default standard-library staged snapshot.
pub type DefaultStdlibSnapshot = StagedCompilationSnapshot;

impl StagedCompilationSnapshot {
    pub fn compile_prefix(&self) -> &CompilationPrefixSnapshot {
        &self.compile_prefix
    }

    pub fn declaration_index(&self) -> &sigil::DeclarationIndex {
        &self.compile_prefix.declaration_index
    }

    pub fn resolve_state(&self) -> sigil::ResolveResumeState {
        self.compile_prefix.resolve_state
    }

    pub fn scar_checkpoint(&self) -> &scar::ScarCheckpoint {
        &self.compile_prefix.scar_checkpoint
    }

    pub fn bytecode(&self) -> &forge::bytecode::Bytecode {
        &self.compile_prefix.bytecode
    }

    pub fn symbol_semantic_infos(&self) -> Vec<surtr_analysis::SymbolSemanticInfo> {
        let owner_registry = sigil::precollect_owner_registry(&self.module_stages)
            .expect("validated staged snapshot must retain a valid owner registry");
        surtr_analysis::symbol_semantic_infos_from_compile_metadata(
            &owner_registry,
            self.declaration_index(),
            &self.docs,
            &self.signatures,
        )
    }

    pub fn semantic_index(&self) -> surtr_analysis::SemanticIndex {
        let owner_registry = sigil::precollect_owner_registry(&self.module_stages)
            .expect("validated staged snapshot must retain a valid owner registry");
        surtr_analysis::SemanticIndex::from_compile_metadata(
            &owner_registry,
            self.declaration_index(),
            &self.docs,
            &self.signatures,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationPrefixSnapshot {
    pub declaration_index: sigil::DeclarationIndex,
    pub resolve_state: sigil::ResolveResumeState,
    pub scar_checkpoint: scar::ScarCheckpoint,
    pub bytecode: forge::bytecode::Bytecode,
}

impl CompilationPrefixSnapshot {
    pub fn from_parts(
        declaration_index: sigil::DeclarationIndex,
        resolve_state: sigil::ResolveResumeState,
        scar_checkpoint: scar::ScarCheckpoint,
        bytecode: forge::bytecode::Bytecode,
    ) -> Self {
        Self {
            declaration_index,
            resolve_state,
            scar_checkpoint,
            bytecode,
        }
    }

    pub fn bytecode(&self) -> &forge::bytecode::Bytecode {
        &self.bytecode
    }

    pub fn next_fun_idx(&self) -> u32 {
        self.bytecode
            .functions
            .iter()
            .map(|entry| entry.fun_idx.saturating_add(1))
            .max()
            .unwrap_or(0)
    }

    pub fn restored_scar_session(&self) -> scar::ScarSession {
        let mut scar_session = scar::ScarSession::new();
        scar_session.rollback(self.scar_checkpoint.clone());
        scar_session.ensure_next_fun_idx_at_least(self.next_fun_idx());
        scar_session
    }

    pub fn forge_session(&self) -> forge::ForgeSession {
        forge::ForgeSession::from_bytecode(&self.bytecode)
    }
}

const STDLIB_SEMANTIC_CACHE_SCHEMA: u32 = 10;
const TEST_SEMANTIC_PREFIX_CACHE_SCHEMA: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedStdlibSemanticEnvelope {
    schema: u32,
    key: String,
    payload: CachedStdlibSemanticPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedStdlibSemanticPayload {
    compile_prefix: CompilationPrefixSnapshot,
    docs: Vec<DocEntry>,
    signatures: Vec<SignatureEntry>,
    auto_import_modules: BTreeSet<String>,
    default_stage_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedTestSemanticPrefixEnvelope {
    schema: u32,
    key: String,
    payload: CachedTestSemanticPrefixPayload,
}

pub type CachedTestSemanticPrefixPayload = CompilationPrefixSnapshot;

pub fn cached_lib_module_inputs() -> Result<Vec<ModuleInput>, LoadError> {
    static CACHE: OnceLock<Result<Vec<ModuleInput>, LoadError>> = OnceLock::new();
    CACHE.get_or_init(collect_lib_module_inputs).clone()
}

pub fn cached_additional_default_std_module_inputs() -> Result<Vec<ModuleInput>, LoadError> {
    static CACHE: OnceLock<Result<Vec<ModuleInput>, LoadError>> = OnceLock::new();
    CACHE
        .get_or_init(collect_additional_default_std_module_inputs)
        .clone()
}

pub fn current_exe_fingerprint() -> Result<String, String> {
    static FINGERPRINT: OnceLock<Result<String, String>> = OnceLock::new();
    FINGERPRINT
        .get_or_init(|| {
            let exe =
                env::current_exe().map_err(|e| format!("failed to locate current exe: {}", e))?;
            let bytes =
                fs::read(&exe).map_err(|e| format!("failed to read {}: {}", exe.display(), e))?;
            Ok(stable_hash_bytes(&bytes))
        })
        .clone()
}

pub fn test_semantic_prefix_cache_key(
    compile_unit_kind: CompileUnitKind,
    compile_sources: &CompileSources,
) -> Result<String, String> {
    let fingerprint = current_exe_fingerprint()?;
    let stdlib_fingerprint = match compile_sources.stdlib_variant {
        StdlibVariant::Default => default_stdlib_source_fingerprint()?,
        StdlibVariant::TestEnabled => test_enabled_stdlib_source_fingerprint()?,
    };
    Ok(test_semantic_prefix_cache_key_with_fingerprint(
        &fingerprint,
        &stdlib_fingerprint,
        compile_unit_kind,
        compile_sources,
    ))
}

pub fn test_semantic_prefix_cache_key_with_fingerprint(
    current_exe_fingerprint: &str,
    stdlib_fingerprint: &str,
    compile_unit_kind: CompileUnitKind,
    compile_sources: &CompileSources,
) -> String {
    let user_file_name = compile_sources
        .sources
        .file_name(compile_sources.user_source_id)
        .unwrap_or("<unknown>");
    let user_source = compile_sources
        .sources
        .source(compile_sources.user_source_id)
        .unwrap_or("");

    let mut key = String::new();
    key.push_str("surtr-test-semantic-prefix-v");
    key.push_str(&TEST_SEMANTIC_PREFIX_CACHE_SCHEMA.to_string());
    key.push('\x1f');
    key.push_str(current_exe_fingerprint);
    key.push('\x1f');
    key.push_str(stdlib_fingerprint);
    key.push('\x1f');
    key.push_str(&STDLIB_SEMANTIC_CACHE_SCHEMA.to_string());
    key.push('\x1f');
    key.push_str(stdlib_variant_cache_key(compile_sources.stdlib_variant));
    key.push('\x1f');
    key.push_str(match compile_unit_kind {
        CompileUnitKind::Script => "script",
        CompileUnitKind::DefinitionCheck => "definition-check",
        CompileUnitKind::Project => "project",
        CompileUnitKind::Repl => "repl",
    });
    key.push('\x1f');
    key.push_str(user_file_name);
    key.push('\x1f');
    key.push_str(&compile_sources.user_module_path);
    key.push('\x1f');
    key.push_str(&stable_hash_hex(user_source));

    for stage in &compile_sources.module_stages {
        key.push('|');
        for module in stage {
            let file_name = compile_sources
                .sources
                .file_name(module.source_id)
                .unwrap_or("<unknown>");
            let source = compile_sources
                .sources
                .source(module.source_id)
                .unwrap_or("");
            key.push_str(file_name);
            key.push('\x1e');
            key.push_str(&module.module_path);
            key.push('\x1e');
            key.push_str(source_kind_cache_key(module.source_kind));
            key.push('\x1e');
            key.push_str(&stable_hash_hex(source));
            key.push('\x1f');
        }
    }

    stable_hash_hex(&key)
}

fn default_stdlib_source_fingerprint() -> Result<String, String> {
    let module_sources =
        collect_module_sources_with_module_stages(&[]).map_err(|err| err.to_string())?;
    Ok(stdlib_semantic_cache_key(&module_sources))
}

fn test_enabled_stdlib_source_fingerprint() -> Result<String, String> {
    let module_sources =
        collect_test_module_sources_with_module_stages(&[]).map_err(|err| err.to_string())?;
    Ok(stdlib_semantic_cache_key(&module_sources))
}

pub fn load_cached_test_semantic_prefix(
    cache_path: &Path,
    expected_key: &str,
) -> Option<CachedTestSemanticPrefixPayload> {
    let bytes = fs::read(cache_path).ok()?;
    let envelope: CachedTestSemanticPrefixEnvelope = bincode::deserialize(&bytes).ok()?;
    if envelope.schema != TEST_SEMANTIC_PREFIX_CACHE_SCHEMA || envelope.key != expected_key {
        return None;
    }
    Some(envelope.payload)
}

pub fn store_cached_test_semantic_prefix(
    cache_path: &Path,
    key: &str,
    payload: CachedTestSemanticPrefixPayload,
) {
    let Some(parent) = cache_path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let envelope = CachedTestSemanticPrefixEnvelope {
        schema: TEST_SEMANTIC_PREFIX_CACHE_SCHEMA,
        key: key.to_string(),
        payload,
    };
    let Ok(bytes) = bincode::serialize(&envelope) else {
        return;
    };
    let temp_path = cache_path.with_extension(format!("{}.tmp", std::process::id()));
    if fs::write(&temp_path, bytes).is_err() {
        let _ = fs::remove_file(&temp_path);
        return;
    }
    if fs::rename(&temp_path, cache_path).is_err() {
        if fs::copy(&temp_path, cache_path).is_err() {
            let _ = fs::remove_file(&temp_path);
            return;
        }
        let _ = fs::remove_file(&temp_path);
    }
}

pub fn store_cached_test_semantic_prefix_snapshot(
    cache_path: &Path,
    key: &str,
    snapshot: &CompilationPrefixSnapshot,
) {
    store_cached_test_semantic_prefix(cache_path, key, snapshot.clone());
}

pub fn default_stdlib_semantic_snapshot() -> Result<Arc<DefaultStdlibSnapshot>, LoadError> {
    static SNAPSHOT: OnceLock<Result<Arc<DefaultStdlibSnapshot>, LoadError>> = OnceLock::new();
    SNAPSHOT
        .get_or_init(|| build_default_stdlib_snapshot().map(Arc::new))
        .clone()
}

pub fn test_enabled_stdlib_semantic_snapshot() -> Result<Arc<DefaultStdlibSnapshot>, LoadError> {
    static SNAPSHOT: OnceLock<Result<Arc<DefaultStdlibSnapshot>, LoadError>> = OnceLock::new();
    SNAPSHOT
        .get_or_init(|| build_stdlib_snapshot(StdlibVariant::TestEnabled).map(Arc::new))
        .clone()
}

fn build_default_stdlib_snapshot() -> Result<DefaultStdlibSnapshot, LoadError> {
    build_stdlib_snapshot(StdlibVariant::Default)
}

fn build_stdlib_snapshot(
    stdlib_variant: StdlibVariant,
) -> Result<DefaultStdlibSnapshot, LoadError> {
    let module_sources = match stdlib_variant {
        StdlibVariant::Default => collect_module_sources_with_module_stages(&[])?,
        StdlibVariant::TestEnabled => collect_test_module_sources_with_module_stages(&[])?,
    };
    let cache_key = stdlib_semantic_cache_key(&module_sources);
    let module_stages = repl::logic::core::parse_module_stages_from_sources(
        &module_sources.sources,
        &module_sources.module_stages,
        CompileUnitKind::Script,
    )
    .map_err(|e| LoadError::BootstrapFailed {
        phase: "parse".into(),
        file_name: module_sources
            .sources
            .file_name(e.source_id)
            .unwrap_or("<stdlib>")
            .to_string(),
        message: e.message(),
    })?;
    let docs = collect_doc_entries(&module_stages, &[], None);
    let signatures = collect_signature_entries(&module_stages, &[], None);
    let auto_import_modules = module_stages
        .iter()
        .flat_map(|stage| stage.iter())
        .filter(|module| module.auto_import)
        .map(|module| surface_path_name(&module.module_path).to_string())
        .collect::<BTreeSet<_>>();
    let default_stage_count = module_stages.len();

    if let Some(payload) = load_cached_stdlib_semantic_snapshot(
        &stdlib_semantic_cache_path(stdlib_variant),
        &cache_key,
    ) {
        if payload.default_stage_count == default_stage_count {
            return Ok(DefaultStdlibSnapshot {
                module_stages,
                compile_prefix: payload.compile_prefix,
                docs: payload.docs,
                signatures: payload.signatures,
                auto_import_modules: payload.auto_import_modules,
                default_stage_count: payload.default_stage_count,
            });
        }
    }

    let declaration_index = sigil::precollect_declaration_index(&module_stages).map_err(|e| {
        LoadError::BootstrapFailed {
            phase: "resolve".into(),
            file_name: "<stdlib>".into(),
            message: e.message,
        }
    })?;
    let resolved = sigil::resolve_staged_program_with_state(
        &module_stages,
        Vec::new(),
        &declaration_index,
        None,
    )
    .map_err(|e| LoadError::BootstrapFailed {
        phase: "resolve".into(),
        file_name: "<stdlib>".into(),
        message: e.message,
    })?;
    let resume_state = resolved.resume_state;
    let mut scar_session = scar::ScarSession::new();
    let mut typecheck_context = scar::TypecheckContext::from_source_policy(
        SourceKind::StdDefinitionSource.policy(CompileUnitKind::DefinitionCheck, None),
    );
    typecheck_context.enforce_builtin_type_contracts = true;
    typecheck_context.allow_error_function_params = true;
    let typed = scar_session
        .typecheck_staged_program_with_context(resolved, typecheck_context)
        .map_err(|e| LoadError::BootstrapFailed {
            phase: "typecheck".into(),
            file_name: "<stdlib>".into(),
            message: e.message,
        })?;
    let mut bytecode =
        forge::codegen_typed_program(typed).map_err(|e| LoadError::BootstrapFailed {
            phase: "codegen".into(),
            file_name: "<stdlib>".into(),
            message: e.message,
        })?;
    scar_session.reconcile_function_indices(bytecode.functions.iter().filter_map(|entry| {
        entry
            .qualified_name
            .as_deref()
            .map(|qualified_name| (qualified_name, entry.fun_idx))
    }));
    bytecode.docs = docs.clone();
    bytecode.signatures = signatures.clone();
    let next_fun_idx = bytecode
        .functions
        .iter()
        .map(|entry| entry.fun_idx.saturating_add(1))
        .max()
        .unwrap_or(0);
    let resolve_state = sigil::ResolveResumeState {
        next_local_id: resume_state.next_local_id.max(next_fun_idx),
    };

    let snapshot = DefaultStdlibSnapshot {
        default_stage_count,
        compile_prefix: CompilationPrefixSnapshot::from_parts(
            declaration_index,
            resolve_state,
            scar_session.checkpoint(),
            bytecode,
        ),
        docs,
        signatures,
        auto_import_modules,
        module_stages,
    };
    store_cached_stdlib_semantic_snapshot(
        &stdlib_semantic_cache_path(stdlib_variant),
        &cache_key,
        CachedStdlibSemanticPayload {
            compile_prefix: snapshot.compile_prefix.clone(),
            docs: snapshot.docs.clone(),
            signatures: snapshot.signatures.clone(),
            auto_import_modules: snapshot.auto_import_modules.clone(),
            default_stage_count: snapshot.default_stage_count,
        },
    );
    Ok(snapshot)
}

fn load_cached_stdlib_semantic_snapshot(
    cache_path: &Path,
    expected_key: &str,
) -> Option<CachedStdlibSemanticPayload> {
    let bytes = fs::read(cache_path).ok()?;
    let envelope: CachedStdlibSemanticEnvelope = bincode::deserialize(&bytes).ok()?;
    if envelope.schema != STDLIB_SEMANTIC_CACHE_SCHEMA || envelope.key != expected_key {
        return None;
    }
    Some(envelope.payload)
}

fn store_cached_stdlib_semantic_snapshot(
    cache_path: &Path,
    key: &str,
    payload: CachedStdlibSemanticPayload,
) {
    let Some(parent) = cache_path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let envelope = CachedStdlibSemanticEnvelope {
        schema: STDLIB_SEMANTIC_CACHE_SCHEMA,
        key: key.to_string(),
        payload,
    };
    let Ok(bytes) = bincode::serialize(&envelope) else {
        return;
    };
    let temp_path = cache_path.with_extension(format!("{}.tmp", std::process::id()));
    if fs::write(&temp_path, bytes).is_err() {
        let _ = fs::remove_file(&temp_path);
        return;
    }
    if fs::rename(&temp_path, cache_path).is_err() {
        if fs::copy(&temp_path, cache_path).is_err() {
            let _ = fs::remove_file(&temp_path);
            return;
        }
        let _ = fs::remove_file(&temp_path);
    }
}

fn stdlib_semantic_cache_path(stdlib_variant: StdlibVariant) -> PathBuf {
    let file_name = match stdlib_variant {
        StdlibVariant::Default => "std.semantic",
        StdlibVariant::TestEnabled => "std.test.semantic",
    };
    if let Some(path) = env::var_os("SURTR_STDLIB_CACHE_DIR") {
        return PathBuf::from(path).join(file_name);
    }
    target_root_from_current_exe()
        .map(|root| root.join("surtr-stdlib-cache").join(file_name))
        .unwrap_or_else(|| env::temp_dir().join("surtr-stdlib-cache").join(file_name))
}

pub fn target_root_from_current_exe() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let mut current = exe.parent()?;
    while let Some(name) = current.file_name().and_then(|name| name.to_str()) {
        if name == "debug" || name == "release" {
            return current.parent().map(Path::to_path_buf);
        }
        current = current.parent()?;
    }
    None
}

fn stdlib_semantic_cache_key(module_sources: &ModuleSources) -> String {
    stable_hash_hex(&stdlib_semantic_cache_material(module_sources))
}

fn stdlib_semantic_cache_material(module_sources: &ModuleSources) -> String {
    let mut key = String::new();
    key.push_str("surtr-stdlib-semantic-cache-v");
    key.push_str(&STDLIB_SEMANTIC_CACHE_SCHEMA.to_string());
    key.push('\x1f');
    key.push_str(env!("CARGO_PKG_VERSION"));
    key.push('\x1f');
    key.push_str("source-policy-schema-v");
    key.push_str(&SOURCE_POLICY_SCHEMA_VERSION.to_string());
    key.push('\x1f');
    key.push_str("symbol-capability-schema-v");
    key.push_str(&sindr::names::SYMBOL_CAPABILITY_SCHEMA_VERSION.to_string());
    key.push('\x1f');
    for meta in builtin_function_metas() {
        key.push_str(meta.name);
        key.push('\x1e');
        key.push_str(meta.sig_str);
        key.push('\x1e');
        key.push_str(&meta.arity.to_string());
        key.push('\x1f');
    }
    key.push('\x1d');
    for meta in builtin_type_head_metas() {
        key.push_str(meta.name);
        key.push('\x1e');
        key.push_str(&meta.params.join(","));
        key.push('\x1f');
    }
    key.push('\x1d');
    let stdlib_variant = if module_sources
        .module_stages
        .iter()
        .flatten()
        .any(|module| module.module_path == "Test")
    {
        StdlibVariant::TestEnabled
    } else {
        StdlibVariant::Default
    };
    key.push_str(&stdlib_module_spec_cache_key(stdlib_variant));
    key.push('\x1d');
    for stage in &module_sources.module_stages {
        key.push('|');
        for module in stage {
            let file_name = module_sources
                .sources
                .file_name(module.source_id)
                .unwrap_or("<unknown>");
            let source = module_sources
                .sources
                .source(module.source_id)
                .unwrap_or("");
            key.push_str(file_name);
            key.push('\x1e');
            key.push_str(&module.module_path);
            key.push('\x1e');
            key.push_str(source_kind_cache_key(module.source_kind));
            key.push('\x1e');
            key.push_str(&stable_hash_hex(source));
            key.push('\x1f');
        }
    }
    key
}

pub fn source_kind_cache_key(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Script => "script",
        // Keep cache key strings stable for backward compatibility with existing cache entries.
        SourceKind::DefinitionSource => "module",
        SourceKind::StdDefinitionSource => "std",
        SourceKind::ProjectConfigSource => "project-config",
        SourceKind::ReplChunk => "repl",
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sindr::ir::DocKind;

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
        .expect("definition source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 3);
        assert_eq!(lowered[0].module_path, "Global::A");
        assert_eq!(lowered[1].module_path, "Global::B");
        assert_eq!(lowered[2].module_path, "");
        assert!(matches!(
            lowered[0].ast[0],
            spire::ast::Ast::Import(_, _, spire::ast::ImportSpec::Single(_))
        ));
        assert!(lowered[2]
            .ast
            .iter()
            .any(|stmt| matches!(stmt, spire::ast::Ast::RecordDef(..))));
    }

    #[test]
    fn default_stdlib_snapshot_contains_only_default_stages() {
        let snapshot =
            default_stdlib_semantic_snapshot().expect("default stdlib snapshot should build");

        assert_eq!(snapshot.default_stage_count, snapshot.module_stages.len());
        assert!(snapshot
            .declaration_index()
            .values()
            .any(|entry| entry.fq_name == "Global::Kernel::print"));
        assert!(!snapshot
            .declaration_index()
            .values()
            .any(|entry| entry.fq_name.starts_with("Global::Test::")));
        assert!(!snapshot
            .declaration_index()
            .values()
            .any(|entry| entry.module_path == "TestOnly"));
    }

    #[test]
    fn stdlib_snapshot_is_a_staged_compilation_snapshot() {
        let snapshot =
            default_stdlib_semantic_snapshot().expect("default stdlib snapshot should build");
        let staged: &StagedCompilationSnapshot = snapshot.as_ref();

        assert_eq!(staged.default_stage_count, staged.module_stages.len());
        assert!(!staged.compile_prefix.declaration_index.is_empty());
    }

    #[test]
    fn stdlib_snapshot_exposes_joined_symbol_semantic_info() {
        let snapshot =
            default_stdlib_semantic_snapshot().expect("default stdlib snapshot should build");

        let infos = snapshot.symbol_semantic_infos();
        let index = snapshot.semantic_index();

        let duration = infos
            .iter()
            .find(|info| info.surface_name == "Duration")
            .expect("Duration semantic info should exist");
        assert_eq!(duration.identity, Some(sindr::names::TypeIdentity::Struct));
        assert_eq!(
            duration.capabilities,
            Some(sindr::names::SymbolCapabilities::new(
                true,
                true,
                true,
                Some(sindr::names::FacetRootKind::TypeRoot),
            ))
        );

        assert!(infos.iter().any(|info| {
            info.canonical_name == "Global::Kernel::print"
                && info.surface_name == "Kernel::print"
                && info.detail.is_some()
                && info.documentation.is_some()
        }));
        assert!(index.symbols().iter().any(|symbol| {
            symbol.label == "Kernel::print"
                && symbol.detail.is_some()
                && symbol.documentation.is_some()
        }));
    }

    #[test]
    fn test_enabled_stdlib_snapshot_contains_test_module() {
        let snapshot = test_enabled_stdlib_semantic_snapshot()
            .expect("test-enabled stdlib snapshot should build");

        assert!(snapshot
            .declaration_index()
            .values()
            .any(|entry| entry.fq_name.starts_with("Global::Test::")));
    }

    #[test]
    fn stdlib_semantic_cache_rejects_corrupt_file() {
        let cache_path = std::env::temp_dir().join(format!(
            "surtr-corrupt-stdlib-cache-{}.semantic",
            std::process::id()
        ));
        std::fs::write(&cache_path, b"not a semantic cache").expect("write corrupt cache");

        let loaded = load_cached_stdlib_semantic_snapshot(&cache_path, "expected-key");

        assert!(loaded.is_none());
        let _ = std::fs::remove_file(cache_path);
    }

    #[test]
    fn stdlib_semantic_cache_key_tracks_stdlib_module_spec_variant() {
        let default_sources =
            collect_module_sources_with_module_stages(&[]).expect("default stdlib should load");
        let test_sources = collect_test_module_sources_with_module_stages(&[])
            .expect("test-enabled stdlib should load");

        assert_ne!(
            crate::loader::stdlib_module_spec_cache_key(StdlibVariant::Default),
            crate::loader::stdlib_module_spec_cache_key(StdlibVariant::TestEnabled)
        );
        assert_ne!(
            stdlib_semantic_cache_key(&default_sources),
            stdlib_semantic_cache_key(&test_sources)
        );
    }

    #[test]
    fn stdlib_semantic_cache_material_tracks_shared_semantic_schemas() {
        let module_sources =
            collect_module_sources_with_module_stages(&[]).expect("default stdlib should load");

        let material = stdlib_semantic_cache_material(&module_sources);

        assert!(material.contains(&format!(
            "source-policy-schema-v{}",
            sindr::policy::SOURCE_POLICY_SCHEMA_VERSION
        )));
        assert!(material.contains(&format!(
            "symbol-capability-schema-v{}",
            sindr::names::SYMBOL_CAPABILITY_SCHEMA_VERSION
        )));
    }

    #[test]
    fn derive_source_policy_carries_parse_and_runtime_policy() {
        let entrypoint = EntryPoint::qualified("App::main");

        let policy = derive_source_policy(
            CompileUnitKind::Project,
            SourceKind::ProjectConfigSource,
            Some(&entrypoint),
        );

        assert_eq!(policy.parse_profile, sindr::policy::ParseProfile::Project);
        assert_eq!(
            policy.runtime_policy.exit_code_policy,
            sindr::policy::ExitCodePolicy::EntryOnly
        );
        assert_eq!(
            policy.runtime_policy.normalized_entrypoint.as_deref(),
            Some("App::main")
        );
    }

    #[test]
    fn test_semantic_prefix_cache_roundtrips_snapshot() {
        let cache_path = std::env::temp_dir().join(format!(
            "surtr-test-prefix-cache-{}.semantic",
            std::process::id()
        ));
        let snapshot = CompilationPrefixSnapshot::from_parts(
            sigil::DeclarationIndex::new(),
            sigil::ResolveResumeState { next_local_id: 7 },
            scar::ScarSession::new().checkpoint(),
            forge::bytecode::Bytecode::default(),
        );

        store_cached_test_semantic_prefix_snapshot(&cache_path, "expected-key", &snapshot);

        let loaded = load_cached_test_semantic_prefix(&cache_path, "expected-key")
            .expect("payload should roundtrip");

        assert_eq!(loaded.resolve_state.next_local_id, 7);
        assert_eq!(loaded.declaration_index, snapshot.declaration_index);
        let _ = std::fs::remove_file(cache_path);
    }

    #[test]
    fn test_semantic_prefix_cache_rejects_corrupt_file() {
        let cache_path = std::env::temp_dir().join(format!(
            "surtr-corrupt-test-prefix-cache-{}.semantic",
            std::process::id()
        ));
        std::fs::write(&cache_path, b"not a semantic prefix cache").expect("write corrupt cache");

        let loaded = load_cached_test_semantic_prefix(&cache_path, "expected-key");

        assert!(loaded.is_none());
        let _ = std::fs::remove_file(cache_path);
    }

    #[test]
    fn compilation_prefix_snapshot_derives_next_fun_idx_from_bytecode() {
        let mut bytecode = forge::bytecode::Bytecode::default();
        bytecode.functions.push(sindr::ir::FunctionEntry {
            fun_idx: 4,
            entry_pc: 0,
            num_locals: 0,
            arity: 0,
            qualified_name: Some("Global::prefix".to_string()),
            signature: None,
            end_pc: 0,
            span_start: 0,
            span_end: 0,
            flags: Default::default(),
        });
        let snapshot = CompilationPrefixSnapshot::from_parts(
            sigil::DeclarationIndex::new(),
            sigil::ResolveResumeState { next_local_id: 0 },
            scar::ScarSession::new().checkpoint(),
            bytecode,
        );

        assert_eq!(snapshot.next_fun_idx(), 5);
    }

    #[test]
    fn lower_module_source_merges_result_ctors_into_single_impl_owner() {
        let ast = spire::parse_with_context(
            r#"@builtin type Ok($T) -> Result<$T>

impl Result {
  def dummy() { () }
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0].module_path, "Global::Result");
        assert!(lowered[0].ast.iter().any(
            |stmt| matches!(stmt, spire::ast::Ast::ResultCtorDecl(_, name, _, _, _) if name == "Ok")
        ));
        assert!(lowered[0].ast.iter().any(
            |stmt| matches!(stmt, spire::ast::Ast::ImplDef(_, target, methods, _) if target == "Global::Result"
                && methods.iter().any(|method| matches!(method, spire::ast::Ast::Def(_, name, _, _, _, _, _, _) if name == "dummy")))
        ));
    }

    #[test]
    fn lower_module_source_keeps_builtin_decls_global_even_with_single_impl_owner() {
        let ast = spire::parse_with_context(
            r#"@builtin type Int
@builtin def safe_mod(a: Int, b: Int) -> Result<Int, ZeroDivisionError>

impl Int {
  def dummy() { () }
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 2);
        assert_eq!(lowered[0].module_path, "Global::Int");
        assert_eq!(lowered[1].module_path, "");
        assert!(lowered[1]
            .ast
            .iter()
            .any(|stmt| matches!(stmt, spire::ast::Ast::BuiltinTypeDecl(_, _, _))));
        assert!(lowered[1]
            .ast
            .iter()
            .any(|stmt| matches!(stmt, spire::ast::Ast::BuiltinDecl(_, name, _, _, _, _) if name == "safe_mod")));
    }

    #[test]
    fn lower_module_source_attaches_top_level_consts_to_namespace_module() {
        let ast = spire::parse_with_context(
            r#"const APP_NAME = "surtr"

defmod AppConfig {
  def label() -> String { APP_NAME }
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::module()),
        )
        .expect("definition source should parse");

        let lowered = lower_module_source_ast(ast, Some("AppConfig"));
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0].module_path, "Global::AppConfig");
        assert!(lowered[0].ast.iter().any(
            |stmt| matches!(stmt, spire::ast::Ast::ConstDef(_, name, _, _, _) if name == "APP_NAME")
        ));
    }

    #[test]
    fn lower_module_source_keeps_defmod_local_imports_in_that_module() {
        let ast = spire::parse_with_context(
            r#"defmod Parser {
  import String;
  def parse(line: String) -> String { trim(line) }
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("definition source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0].module_path, "Global::Parser");
        assert!(matches!(
            lowered[0].ast.as_slice(),
            [
                spire::ast::Ast::Import(_, _, spire::ast::ImportSpec::All),
                spire::ast::Ast::Def(_, name, _, _, _, _, _, _)
            ] if name == "parse"
        ));
    }

    #[test]
    fn lower_module_source_hoists_impl_local_imports_into_impl_owner_module() {
        let ast = spire::parse_with_context(
            r#"impl User {
  def normalize(self: Self, name: String) -> String { trim(name) }
  import String;
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("definition source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0].module_path, "Global::User");
        assert!(matches!(
            lowered[0].ast.as_slice(),
            [
                spire::ast::Ast::Import(_, _, spire::ast::ImportSpec::All),
                spire::ast::Ast::ImplDef(_, target, methods, _)
            ] if target == "Global::User"
                && matches!(methods.as_slice(), [spire::ast::Ast::Def(_, name, _, _, _, _, _, _)] if name == "normalize")
        ));
    }

    #[test]
    fn lower_module_source_hoists_trait_impl_local_imports_into_trait_impl_module() {
        let ast = spire::parse_with_context(
            r#"impl Show for User {
  def to_string(self: Self) -> String { trim("x") }
  import String;
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("definition source should parse");

        let lowered = lower_module_source_ast(ast, None);
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0].module_path, "Global::User");
        assert!(matches!(
            lowered[0].ast.as_slice(),
            [
                spire::ast::Ast::Import(_, _, spire::ast::ImportSpec::All),
                spire::ast::Ast::TraitImplDef(_, trait_name, _, spire::ast::AstTy::Named(_, target), _, methods, _)
            ] if trait_name == "Show"
                && target == "Global::User"
                && matches!(methods.as_slice(), [spire::ast::Ast::Def(_, name, _, _, _, _, _, _)] if name == "to_string")
        ));
    }

    #[test]
    fn collect_doc_entries_includes_deferror_docs() {
        let ast = spire::parse_with_context(
            r#"defmod Bootstrap {
  def dummy() { () }
}

@doc """Missing value."""
deferror NoneError { "None Value." }"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, Some("Bootstrap"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Bootstrap::NoneError"
                && entry.signature.as_deref() == Some("deferror NoneError")
                && entry.doc == "Missing value."
        }));
    }

    #[test]
    fn collect_doc_entries_includes_special_closure_type_docs() {
        let ast = spire::parse_with_context(
            r#"@doc """Closure docs."""
@builtin type Closure"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, Some("SpecialTypes"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.kind == DocKind::Type
                && entry.signature.as_deref() == Some("type Closure")
                && entry.doc == "Closure docs."
        }));
    }

    #[test]
    fn bundled_bootstrap_source_parses_in_std_module_context() {
        let ast = spire::parse_with_context(
            include_str!("../../../lib/bootstrap.srt"),
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("bootstrap source should parse as a std module");
        assert!(ast.iter().any(|stmt| matches!(
            stmt,
            spire::ast::Ast::Defmod(_, name, body, _)
                if name == "Global::Bootstrap"
                && body.iter().any(|stmt| matches!(
                    stmt,
                    spire::ast::Ast::BuiltinDecl(_, builtin_name, _, _, _, _) if builtin_name == "import"
                ))
        )));
    }

    #[test]
    fn bundled_kernel_source_marks_kernel_module_autoimport() {
        let ast = spire::parse_with_context(
            include_str!("../../../lib/kernel.srt"),
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("kernel source should parse as a std module");

        let lowered = lower_module_source_ast(ast, None);
        assert!(lowered
            .iter()
            .any(|module| module.module_path == "Global::Kernel" && module.auto_import));
    }

    #[test]
    fn bundled_special_types_source_declares_lazy_builtin_type() {
        let ast = spire::parse_with_context(
            include_str!("../../../lib/types/special_types.srt"),
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("special types source should parse as a std module");

        let lowered = lower_module_source_ast(ast, Some("SpecialTypes"));
        assert!(lowered.iter().any(|module| {
            module.module_path == "SpecialTypes"
                && module.ast.iter().any(|stmt| {
                    matches!(
                        stmt,
                        spire::ast::Ast::BuiltinTypeDecl(_, head, _)
                            if head.name == "Lazy"
                    )
                })
        }));
    }

    #[test]
    fn collect_doc_entries_includes_bootstrap_import_docs() {
        let ast = spire::parse_with_context(
            r#"defmod Bootstrap {
  @doc """Language-provided import macro function."""
  @builtin def import() -> Unit
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, Some("Bootstrap"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Bootstrap::import"
                && entry.kind == DocKind::Function
                && entry.signature.as_deref() == Some("import() -> Unit")
                && entry.doc == "Language-provided import macro function."
        }));
    }

    #[test]
    fn collect_doc_entries_include_kernel_builtin_docs() {
        let ast = spire::parse_with_context(
            r#"@doc """Kernel module."""
@autoimport
defmod Kernel {
  @doc """Print a string to stdout."""
  @builtin def print(a: String) -> Unit
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("kernel source should parse");

        let lowered = lower_module_source_ast(ast, Some("Kernel"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(
            docs.iter().any(|entry| {
                surface_path_name(&entry.qualified_name) == "Kernel"
                    && entry.kind == DocKind::Module
                    && entry.doc == "Kernel module."
            }),
            "{docs:?}"
        );
        assert!(
            docs.iter().any(|entry| {
                entry.qualified_name == "Kernel::print"
                    && entry.kind == DocKind::Function
                    && entry.signature.as_deref() == Some("print(a: String) -> Unit")
                    && entry.doc == "Print a string to stdout."
            }),
            "{docs:?}"
        );
    }

    #[test]
    fn collect_doc_entries_keeps_single_bootstrap_dbg_intrinsic_doc() {
        let ast = spire::parse_with_context(
            r#"defmod Bootstrap {
  @doc """Debug special form."""
  @intrinsic def dbg!(values: *$A) -> Unit
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, Some("Bootstrap"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        let dbg_docs = docs
            .iter()
            .filter(|entry| entry.qualified_name == "Bootstrap::dbg!")
            .collect::<Vec<_>>();
        assert_eq!(dbg_docs.len(), 1, "{dbg_docs:?}");
        assert_eq!(
            dbg_docs[0].signature.as_deref(),
            Some("@intrinsic def dbg!(values: *$A) -> Unit")
        );
    }

    #[test]
    fn lower_module_source_ast_keeps_process_spec_on_lowered_module() {
        let ast = spire::parse_with_context(
            r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }

  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("defagent source should parse");

        let lowered = lower_module_source_ast(ast, Some("Counter"));
        assert_eq!(lowered.len(), 1);
        let process_spec = lowered[0]
            .process_spec
            .as_ref()
            .expect("lowered module should keep process spec");
        assert_eq!(process_spec.process_name, "Global::Counter");
        assert!(!process_spec.boot);
        assert!(matches!(process_spec.kind, spire::ast::ProcessKind::Agent));
    }

    #[test]
    fn collect_doc_entries_keeps_bootstrap_bind_intrinsic_docs() {
        let ast = spire::parse_with_context(
            r#"defmod Bootstrap {
  @doc """Bind special form."""
  @intrinsic def =(pattern: $Pattern, value: $A) -> Unit

  @doc """SafeBind special form."""
  @intrinsic def =?(pattern: $Pattern, value: $A) -> Unit
}"#,
            spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("standard definition source should parse");

        let lowered = lower_module_source_ast(ast, Some("Bootstrap"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        let bind_docs = docs
            .iter()
            .filter(|entry| entry.qualified_name == "Bootstrap::=")
            .collect::<Vec<_>>();
        assert_eq!(bind_docs.len(), 1, "{bind_docs:?}");
        assert_eq!(
            bind_docs[0].signature.as_deref(),
            Some("@intrinsic def =(pattern: $Pattern, value: $A) -> Unit")
        );

        let safe_bind_docs = docs
            .iter()
            .filter(|entry| entry.qualified_name == "Bootstrap::=?")
            .collect::<Vec<_>>();
        assert_eq!(safe_bind_docs.len(), 1, "{safe_bind_docs:?}");
        assert_eq!(
            safe_bind_docs[0].signature.as_deref(),
            Some("@intrinsic def =?(pattern: $Pattern, value: $A) -> Unit")
        );
    }

    #[test]
    fn collect_doc_entries_includes_impl_and_trait_docs() {
        let ast = spire::parse_with_context(
            r#"@doc """Trait docs."""
deftrait Metric {
  def add(self: Self, rhs: Self) -> Self
}

defstruct User {
  name: String,
}

@doc """Metric Int docs."""
impl Metric for Int {
  def add(self: Self, rhs: Self) -> Self {
    self + rhs
  }
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("annotated trait and impl docs should parse");

        let lowered = lower_module_source_ast(ast, Some("Sample"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::Metric"
                && entry.kind == DocKind::Type
                && entry.doc == "Trait docs."
        }));
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::Metric::add"
                && entry.kind == DocKind::Function
                && entry.signature.as_deref() == Some("Metric::add(self: Self, rhs: Self) -> Self")
                && entry.doc == "Trait docs."
        }));
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::impl Metric for Int"
                && entry.kind == DocKind::Type
                && entry.signature.as_deref() == Some("impl Metric for Int")
                && entry.doc == "Metric Int docs."
        }));
    }

    #[test]
    fn collect_doc_entries_excludes_impl_owner_docs() {
        let ast = spire::parse_with_context(
            r#"defstruct User {
  name: String,
}

@autoimport
impl User {
  def new(name: String) -> Self {
    User { name: name }
  }
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("impl owner annotations should parse");

        let lowered = lower_module_source_ast(ast, Some("Sample"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(!docs.iter().any(|entry| {
            entry.qualified_name == "User" && entry.signature.as_deref() == Some("impl User")
        }));
    }

    #[test]
    fn collect_doc_entries_includes_impl_method_docs() {
        let ast = spire::parse_with_context(
            r#"defstruct User {
  name: String,
}

impl User {
  @doc """Construct a new user value."""
  def new(name: String) -> Self {
    User { name: name }
  }

  @doc """Deconstruct a user value for pattern matching."""
  defextractor deconstruct(self: Self) -> Option<String> {
    Option::Some(self.name)
  }
}

@doc """String conversion for `Int`."""
impl Show for Int {
  @doc """Render `Int` through the standard display surface."""
  def to_string(self: Self) -> String {
    inspect(self)
  }
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("annotated impl method docs should parse");

        let lowered = lower_module_source_ast(ast, Some("Sample"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "User::new"
                && entry.kind == DocKind::Function
                && entry.signature.as_deref() == Some("User::new(name: String) -> User")
                && entry.doc == "Construct a new user value."
        }));
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "User::deconstruct"
                && entry.kind == DocKind::Function
                && entry.signature.as_deref()
                    == Some("User::deconstruct(self: User) -> Option<String>")
                && entry.doc == "Deconstruct a user value for pattern matching."
        }));
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::impl Show for Int::to_string"
                && entry.kind == DocKind::Function
                && entry.signature.as_deref()
                    == Some("impl Show for Int::to_string(self: Int) -> String")
                && entry.doc == "Render `Int` through the standard display surface."
        }));
    }

    #[test]
    fn collect_doc_entries_include_struct_and_record_docs_with_head_only_signatures() {
        let ast = spire::parse_with_context(
            r#"@doc """User docs."""
defstruct User {
  name: String,
}

@doc """Point docs."""
defrecord Point(x: Float, y: Float)"#,
            spire::ParserContext::module(1, None),
        )
        .expect("annotated struct and record docs should parse");

        let lowered = lower_module_source_ast(ast, Some("Sample"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::User"
                && entry.kind == DocKind::Type
                && entry.signature.as_deref() == Some("defstruct User")
                && entry.doc == "User docs."
        }));
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::Point"
                && entry.kind == DocKind::Type
                && entry.signature.as_deref() == Some("defrecord Point")
                && entry.doc == "Point docs."
        }));
    }

    #[test]
    fn collect_doc_entries_include_generic_struct_signatures() {
        let ast = spire::parse_with_context(
            r#"@doc """Box docs."""
defstruct Box<$A> {
  value: $A,
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("annotated generic struct docs should parse");

        let lowered = lower_module_source_ast(ast, Some("Sample"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::Box"
                && entry.kind == DocKind::Type
                && entry.signature.as_deref() == Some("defstruct Box<$A>")
                && entry.doc == "Box docs."
        }));
    }

    #[test]
    fn collect_doc_entries_include_multiple_generic_struct_signatures() {
        let ast = spire::parse_with_context(
            r#"@doc """Pair docs."""
defstruct Pair<$A, $B> {
  left: $A,
  right: $B,
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("annotated generic struct docs should parse");

        let lowered = lower_module_source_ast(ast, Some("Sample"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "Sample::Pair"
                && entry.kind == DocKind::Type
                && entry.signature.as_deref() == Some("defstruct Pair<$A, $B>")
                && entry.doc == "Pair docs."
        }));
    }

    #[test]
    fn collect_doc_entries_qualify_builtin_impl_method_signatures() {
        let ast = spire::parse_with_context(
            r#"defstruct User {
  name: String,
}

impl User {
  @doc """Builtin helper doc."""
  @builtin def inspect_name(user: User) -> String
}"#,
            spire::ParserContext::module(1, None),
        )
        .expect("annotated builtin impl method docs should parse");

        let lowered = lower_module_source_ast(ast, Some("Sample"));
        let stages = vec![lowered
            .into_iter()
            .map(|module| sigil::StagedModuleAst {
                source_index: 0,
                module_path: module.module_path,
                doc_module_path: module.doc_module_path,
                ast: module.ast,
                owner: module.owner,
                module_doc: module.module_doc,
                auto_import: module.auto_import,
                process_spec: module.process_spec,
            })
            .collect::<Vec<_>>()];

        let docs = collect_doc_entries(&stages, &[], None);
        assert!(docs.iter().any(|entry| {
            entry.qualified_name == "User::inspect_name"
                && entry.kind == DocKind::Function
                && entry.signature.as_deref() == Some("User::inspect_name(user: User) -> String")
                && entry.doc == "Builtin helper doc."
        }));
    }

    #[test]
    fn explicit_duplicate_defmod_paths_reach_owner_registry() {
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

        let stages =
            parse_module_stages_from_compile_sources(&compile_sources, CompileUnitKind::Script)
                .expect("explicit owners must pass the structural staging check");
        let err = sigil::precollect_declarations(&stages)
            .expect_err("duplicate defmod owners must fail in OwnerRegistry");
        assert_eq!(err.message, "Duplicate top-level owner: Shared");
        assert_eq!(err.related_labels[0].message, "first Mod declaration");
        assert_eq!(err.related_labels[1].message, "conflicting Mod declaration");
    }

    #[test]
    fn explicit_duplicate_process_paths_reach_owner_registry() {
        let agent_source = r#"defagent Shared {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> { Ok(0) }

  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }
}"#;
        let supervisor_source = r#"defsupervisor Shared {
  meta {
    strategy: OneForOne
    max_restarts: 5
    max_seconds: 10
    child_restart_default: Transient
    allow_adopt: True
  }
}"#;
        let module_sources = collect_module_sources_with_module_stages(&[vec![
            ModuleInput {
                file_name: "a.srt".into(),
                source: agent_source.into(),
                module_path: "A".into(),
            },
            ModuleInput {
                file_name: "b.srt".into(),
                source: supervisor_source.into(),
                module_path: "B".into(),
            },
        ]])
        .expect("module collection should succeed");
        let compile_sources =
            compose_script_compile_sources("entry.srt", "print(\"hi\")", module_sources);

        let stages =
            parse_module_stages_from_compile_sources(&compile_sources, CompileUnitKind::Script)
                .expect("explicit process owners must pass the structural staging check");
        let err = sigil::precollect_declarations(&stages)
            .expect_err("duplicate process owners must fail in OwnerRegistry");
        assert_eq!(err.message, "Duplicate top-level owner: Shared");
        assert_eq!(err.related_labels[0].message, "first Mod declaration");
        assert_eq!(
            err.related_labels[1].message,
            "conflicting Supervisor declaration"
        );
    }

    #[test]
    fn explicit_duplicate_after_impl_owner_extension_reaches_owner_registry() {
        let module_sources = collect_module_sources_with_module_stages(&[vec![
            ModuleInput {
                file_name: "a.srt".into(),
                source: "defmod Shared { def a() -> Int { 1 } }".into(),
                module_path: "A".into(),
            },
            ModuleInput {
                file_name: "impl.srt".into(),
                source: "impl Shared { def new() -> Self { Shared } }".into(),
                module_path: "Shared".into(),
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

        let stages =
            parse_module_stages_from_compile_sources(&compile_sources, CompileUnitKind::Script)
                .expect("explicit owners must pass the structural staging check");
        let err = sigil::precollect_declarations(&stages)
            .expect_err("duplicate explicit owners must fail in OwnerRegistry");
        assert_eq!(err.message, "Duplicate top-level owner: Shared");
    }

    #[test]
    fn parse_module_stages_rejects_duplicate_ownerless_structural_roots() {
        let module_sources = collect_module_sources_with_module_stages(&[vec![
            ModuleInput {
                file_name: "a.srt".into(),
                source: "const FIRST: Int = 1".into(),
                module_path: "Shared".into(),
            },
            ModuleInput {
                file_name: "b.srt".into(),
                source: "const SECOND: Int = 2".into(),
                module_path: "Shared".into(),
            },
        ]])
        .expect("module collection should succeed");
        let compile_sources =
            compose_script_compile_sources("entry.srt", "print(\"hi\")", module_sources);

        let err =
            parse_module_stages_from_compile_sources(&compile_sources, CompileUnitKind::Script)
                .expect_err("ownerless structural roots must stay unique");
        assert!(
            matches!(
                err.kind,
                ModuleStageParseErrorKind::DuplicateModulePath { ref module_path, .. }
                    if module_path == "Shared"
            ),
            "{err:?}"
        );
    }

    #[test]
    fn parse_module_stages_preserves_same_stage_file_order_after_parallel_parse() {
        let module_sources = collect_module_sources_with_module_stages(&[vec![
            ModuleInput {
                file_name: "a.srt".into(),
                source: "defmod First { def value() -> Int { 1 } }".into(),
                module_path: "First".into(),
            },
            ModuleInput {
                file_name: "b.srt".into(),
                source: "defmod Second { def value() -> Int { 2 } }".into(),
                module_path: "Second".into(),
            },
        ]])
        .expect("module collection should succeed");
        let compile_sources =
            compose_script_compile_sources("entry.srt", "print(\"hi\")", module_sources);
        let parsed =
            parse_module_stages_from_compile_sources(&compile_sources, CompileUnitKind::Script)
                .expect("module stages should parse");
        let user_stage = parsed.last().expect("user module stage should exist");

        assert_eq!(user_stage[0].module_path, "Global::First");
        assert_eq!(user_stage[1].module_path, "Global::Second");
    }

    #[test]
    fn expanded_snapshot_module_stages_borrow_prefix_when_no_suffix_exists() {
        let snapshot =
            default_stdlib_semantic_snapshot().expect("default stdlib snapshot should build");
        let module_sources =
            collect_module_sources_with_module_stages(&[]).expect("module collection should work");
        let compile_sources =
            compose_script_compile_sources("entry.srt", "print(\"hi\")", module_sources);

        let expanded =
            expand_snapshot_module_stages(&compile_sources, &snapshot, CompileUnitKind::Script)
                .expect("snapshot expansion should succeed");

        assert!(matches!(
            expanded.module_stages,
            std::borrow::Cow::Borrowed(_)
        ));
        assert!(expanded.suffix_module_stages().is_empty());
    }

    #[test]
    fn expanded_snapshot_module_stages_append_parsed_suffix_stages() {
        let snapshot =
            default_stdlib_semantic_snapshot().expect("default stdlib snapshot should build");
        let module_sources = collect_module_sources_with_module_stages(&[vec![ModuleInput {
            file_name: "extra.srt".into(),
            source: "defmod Extra { def value() -> Int { 1 } }".into(),
            module_path: "Extra".into(),
        }]])
        .expect("module collection should succeed");
        let compile_sources =
            compose_script_compile_sources("entry.srt", "print(\"hi\")", module_sources);

        let expanded =
            expand_snapshot_module_stages(&compile_sources, &snapshot, CompileUnitKind::Script)
                .expect("snapshot expansion should succeed");

        assert!(matches!(expanded.module_stages, std::borrow::Cow::Owned(_)));
        let suffix = expanded.suffix_module_stages();
        assert_eq!(suffix.len(), 1);
        assert_eq!(suffix[0][0].module_path, "Global::Extra");
    }
}
