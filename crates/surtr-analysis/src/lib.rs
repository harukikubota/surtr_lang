pub mod context;
pub mod document;
pub mod project_runner;
pub mod query;
pub mod semantic;
pub mod service;

pub use sindr::names::{FacetRootKind, SymbolCapabilities};

pub use context::{
    parse_document, parse_document_tolerant, resolve_context, AnalysisCacheInput, AnalysisCacheKey,
    AnalysisContext, AnalysisContextRequest, AnalysisContextStatus, AnalysisMode,
    AnalysisSourceKind, AnalysisSpan, CacheKeyField, ContextDiagnostic, ContextDiagnosticKind,
    DocumentVersion, ExternalInputState, ExternalInputStatus, ModuleFileFingerprint, ModuleStage,
    ProjectBootSummary, ReplAnalysisContext, ResolvedAnalysisContext, ResolvedProjectPath,
    RunnerContext, RunnerDiagnostic, RunnerDiagnosticKind, RunnerSelection, ScriptProjectContext,
    SelectedContext,
};
pub use document::{DocumentSnapshot, DocumentStore, LineIndex, TextPosition, Utf16Position};
pub use project_runner::{
    decode_project_runner_value, extract_project_runner_input, extract_project_runner_result,
    project_runner_input_from_result, resolve_project_runner, resolve_project_runner_with,
    DeclaredProjectPath, ProjectRunnerDecodeError, ProjectRunnerInput, ProjectRunnerPath,
    ProjectRunnerProfile, ProjectRunnerResult, ProjectRunnerSourceInput,
};
pub use semantic::{
    complete_call_argument, complete_facet_path_arg, complete_prefix, complete_repl_prefix,
    facet_path_context_at_cursor, facet_type_root_capabilities, lookup_symbol_at_cursor,
    rank_completion_candidates_by_expected_type, repl_assist_at_cursor, signature_help_at_cursor,
    symbol_capabilities_for_builtin_surface, symbol_capabilities_for_declaration_entry,
    symbol_semantic_infos_from_compile_metadata, symbol_semantic_infos_from_declaration_index,
    symbol_semantic_infos_from_metadata, CallableSignature, CompletionCandidate, CompletionKind,
    CompletionOrigin, CompletionRequest, CompletionResponse, CompletionScope, CompletionSymbol,
    FacetPathCompletionContext, FacetPathRootKind, InputSignatureHelp, ReplAssist,
    ReplInputSupport, ReplInputSupportContext, ReplInputSupportUpdate, SemanticIndex,
    SignatureLookup, SourceLocation, SymbolLookup, SymbolSemanticInfo,
};
pub use service::{
    AnalysisDiagnostic, AnalysisDiagnosticKind, AnalysisHost, AnalysisRange, AnalysisService,
    AnalysisSeverity, AnalysisSnapshot, DocumentSymbol, HoverResult, Location, SignatureHelpResult,
};
