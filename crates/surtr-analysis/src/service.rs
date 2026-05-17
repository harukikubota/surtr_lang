use std::fs;
use std::path::{Path, PathBuf};

use sindr::policy::{CompileUnitKind, SourceKind};
use spire::ast::Ast;

use crate::{
    complete_prefix, parse_document, AnalysisContextStatus, AnalysisMode, CompletionRequest,
    CompletionResponse, DocumentSnapshot, DocumentStore, LineIndex, ResolvedAnalysisContext,
    SemanticIndex, TextPosition, Utf16Position,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisDiagnosticKind {
    ContextSelection,
    ProjectRunner,
    Parse,
    Resolve,
    Typecheck,
    DocumentMissing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisSeverity {
    Error,
    Warning,
    Information,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisRange {
    pub start: Utf16Position,
    pub end: Utf16Position,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisDiagnostic {
    pub kind: AnalysisDiagnosticKind,
    pub severity: AnalysisSeverity,
    pub path: PathBuf,
    pub range: Option<AnalysisRange>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct AnalysisSnapshot {
    pub context: ResolvedAnalysisContext,
    pub active_document: Option<DocumentSnapshot>,
    pub ast: Option<Vec<Ast>>,
    pub resolved: Option<Vec<sigil::resolved::Resolved>>,
    pub typed: Option<Vec<scar::typed::TypedNode>>,
    pub semantic_index: SemanticIndex,
    pub diagnostics: Vec<AnalysisDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverResult {
    pub range: Option<AnalysisRange>,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHelpResult {
    pub signatures: Vec<String>,
    pub active_signature: Option<usize>,
    pub active_parameter: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub range: AnalysisRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSymbol {
    pub name: String,
    pub detail: Option<String>,
    pub range: AnalysisRange,
    pub selection_range: AnalysisRange,
}

#[derive(Debug, Clone, Default)]
pub struct AnalysisService {
    documents: DocumentStore,
    semantic_index: SemanticIndex,
}

impl AnalysisService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_document(
        &mut self,
        path: PathBuf,
        version: Option<i64>,
        text: String,
    ) -> DocumentSnapshot {
        self.documents.update_document(path, version, text)
    }

    pub fn remove_document(&mut self, path: &Path) -> Option<DocumentSnapshot> {
        self.documents.remove(path)
    }

    pub fn set_semantic_index(&mut self, semantic_index: SemanticIndex) {
        self.semantic_index = semantic_index;
    }

    pub fn document_store(&self) -> &DocumentStore {
        &self.documents
    }

    pub fn analyze(&self, context: ResolvedAnalysisContext) -> AnalysisSnapshot {
        let mut diagnostics = diagnostics_from_context(&context);
        let active_document = self.documents.get(&context.context.active_file).cloned();
        let mut resolved = None;
        let mut typed = None;

        let ast = if let Some(document) = active_document.as_ref() {
            match parse_document(
                &document.text,
                0,
                context.context.source_kind,
                compile_unit_kind_for_active_context(&context),
                module_path_for_document(
                    context.context.mode.clone(),
                    &context.context.active_file,
                ),
            ) {
                Ok(ast) => {
                    if should_analyze_project_stages(&context) {
                        analyze_project_stages(
                            self,
                            &context,
                            document,
                            &mut diagnostics,
                            &mut resolved,
                            &mut typed,
                        );
                    } else {
                        analyze_single_document(
                            &context,
                            document,
                            ast.clone(),
                            &mut diagnostics,
                            &mut resolved,
                            &mut typed,
                        );
                    }
                    Some(ast)
                }
                Err(error) => {
                    let span = error.span();
                    diagnostics.push(diagnostic_from_span(
                        AnalysisDiagnosticKind::Parse,
                        AnalysisSeverity::Error,
                        document,
                        span.start,
                        span.end,
                        error.message(),
                    ));
                    None
                }
            }
        } else {
            diagnostics.push(AnalysisDiagnostic {
                kind: AnalysisDiagnosticKind::DocumentMissing,
                severity: AnalysisSeverity::Warning,
                path: context.context.active_file.clone(),
                range: None,
                message: "active document is not open in the analysis service".to_string(),
            });
            None
        };

        AnalysisSnapshot {
            context,
            active_document,
            ast,
            resolved,
            typed,
            semantic_index: self.semantic_index.clone(),
            diagnostics,
        }
    }

    pub fn diagnostics(&self, snapshot: &AnalysisSnapshot) -> Vec<AnalysisDiagnostic> {
        snapshot.diagnostics.clone()
    }

    pub fn completions(
        &self,
        snapshot: &AnalysisSnapshot,
        position: Utf16Position,
    ) -> CompletionResponse {
        let Some(document) = snapshot.active_document.as_ref() else {
            return CompletionResponse::default();
        };
        let Some(cursor) = document.line_index.utf16_position_to_byte(position) else {
            return CompletionResponse::default();
        };

        complete_prefix(CompletionRequest {
            index: &snapshot.semantic_index,
            source: &document.text,
            cursor,
        })
    }

    pub fn hover(
        &self,
        _snapshot: &AnalysisSnapshot,
        _position: Utf16Position,
    ) -> Option<HoverResult> {
        None
    }

    pub fn signature_help(
        &self,
        _snapshot: &AnalysisSnapshot,
        _position: Utf16Position,
    ) -> Option<SignatureHelpResult> {
        None
    }

    pub fn definition(
        &self,
        _snapshot: &AnalysisSnapshot,
        _position: Utf16Position,
    ) -> Vec<Location> {
        Vec::new()
    }

    pub fn document_symbols(
        &self,
        _snapshot: &AnalysisSnapshot,
        _active_file: &Path,
    ) -> Vec<DocumentSymbol> {
        Vec::new()
    }
}

fn analyze_single_document(
    context: &ResolvedAnalysisContext,
    document: &DocumentSnapshot,
    ast: Vec<Ast>,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
    resolved: &mut Option<Vec<sigil::resolved::Resolved>>,
    typed: &mut Option<Vec<scar::typed::TypedNode>>,
) {
    match sigil::resolve(ast) {
        Ok(resolved_nodes) => {
            match scar::typecheck_with_context(
                resolved_nodes.clone(),
                typecheck_context_for_mode(&context.context.mode),
            ) {
                Ok(typed_nodes) => *typed = Some(typed_nodes),
                Err(error) => diagnostics.push(diagnostic_from_span(
                    AnalysisDiagnosticKind::Typecheck,
                    AnalysisSeverity::Error,
                    document,
                    error.span.start,
                    error.span.end,
                    error.message,
                )),
            }
            *resolved = Some(resolved_nodes);
        }
        Err(error) => diagnostics.push(diagnostic_from_span(
            AnalysisDiagnosticKind::Resolve,
            AnalysisSeverity::Error,
            document,
            error.span.start,
            error.span.end,
            error.message,
        )),
    }
}

fn analyze_project_stages(
    service: &AnalysisService,
    context: &ResolvedAnalysisContext,
    active_document: &DocumentSnapshot,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
    resolved: &mut Option<Vec<sigil::resolved::Resolved>>,
    typed: &mut Option<Vec<scar::typed::TypedNode>>,
) {
    let Some(runner) = context.runner.as_ref() else {
        return;
    };
    let Some(module_stages) = build_staged_modules(service, runner, active_document, diagnostics)
    else {
        return;
    };

    match sigil::precollect_declaration_index(&module_stages) {
        Ok(declaration_index) => match sigil::resolve_staged_program_with_state(
            &module_stages,
            Vec::new(),
            &declaration_index,
            None,
        ) {
            Ok(resolved_program) => {
                let resolved_nodes = resolved_program.resolved.clone();
                match scar::typecheck_staged_program_with_context(
                    resolved_program,
                    typecheck_context_for_mode(&context.context.mode),
                ) {
                    Ok(typed_program) => *typed = Some(typed_program.nodes),
                    Err(error) => diagnostics.push(diagnostic_from_span(
                        AnalysisDiagnosticKind::Typecheck,
                        AnalysisSeverity::Error,
                        active_document,
                        error.span.start,
                        error.span.end,
                        error.message,
                    )),
                }
                *resolved = Some(resolved_nodes);
            }
            Err(error) => diagnostics.push(diagnostic_from_span(
                AnalysisDiagnosticKind::Resolve,
                AnalysisSeverity::Error,
                active_document,
                error.span.start,
                error.span.end,
                error.message,
            )),
        },
        Err(error) => diagnostics.push(diagnostic_from_span(
            AnalysisDiagnosticKind::Resolve,
            AnalysisSeverity::Error,
            active_document,
            error.span.start,
            error.span.end,
            error.message,
        )),
    }
}

fn build_staged_modules(
    service: &AnalysisService,
    runner: &crate::RunnerContext,
    active_document: &DocumentSnapshot,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
) -> Option<Vec<Vec<sigil::StagedModuleAst>>> {
    let mut module_stages = Vec::new();
    for stage in &runner.module_stages {
        let mut staged_modules = Vec::new();
        for file in &stage.files {
            let Some(source) = source_for_module_file(service, &file.path) else {
                continue;
            };
            let ast = if file.path == active_document.path {
                parse_document(
                    &active_document.text,
                    0,
                    SourceKind::DefinitionSource,
                    CompileUnitKind::DefinitionCheck,
                    None,
                )
            } else {
                parse_document(
                    &source,
                    0,
                    file.source_kind,
                    CompileUnitKind::DefinitionCheck,
                    None,
                )
            };
            match ast {
                Ok(ast) => staged_modules.extend(lower_module_ast(ast, None)),
                Err(error) => {
                    let span = error.span();
                    let line_index = LineIndex::new(&source);
                    diagnostics.push(diagnostic_from_line_index(
                        AnalysisDiagnosticKind::Parse,
                        AnalysisSeverity::Error,
                        file.path.clone(),
                        &line_index,
                        span.start,
                        span.end,
                        error.message(),
                    ));
                    return None;
                }
            }
        }
        if !staged_modules.is_empty() {
            module_stages.push(staged_modules);
        }
    }
    Some(module_stages)
}

fn source_for_module_file(service: &AnalysisService, path: &Path) -> Option<String> {
    service
        .documents
        .get(path)
        .map(|document| document.text.clone())
        .or_else(|| fs::read_to_string(path).ok())
}

fn lower_module_ast(
    ast: Vec<Ast>,
    fallback_module_path: Option<&str>,
) -> Vec<sigil::StagedModuleAst> {
    let shared_imports = ast
        .iter()
        .filter_map(|stmt| match stmt {
            Ast::Import(_, _, _) => Some(stmt.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut lowered = Vec::new();
    let mut shared_global_defs = Vec::new();

    for stmt in ast {
        match stmt {
            Ast::Defmod(_, module_path, body, attrs) => {
                let mut module_ast = shared_imports.clone();
                module_ast.extend(body);
                lowered.push(sigil::StagedModuleAst {
                    module_path,
                    doc_module_path: None,
                    ast: module_ast,
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                    process_spec: None,
                });
            }
            Ast::Defagent(_, module_path, body, process_spec, attrs)
            | Ast::Defgenserver(_, module_path, body, process_spec, attrs)
            | Ast::Defsupervisor(_, module_path, body, process_spec, attrs)
            | Ast::DefdynamicSupervisor(_, module_path, body, process_spec, attrs) => {
                let mut module_ast = shared_imports.clone();
                module_ast.extend(body);
                lowered.push(sigil::StagedModuleAst {
                    module_path,
                    doc_module_path: None,
                    ast: module_ast,
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                    process_spec: Some(process_spec),
                });
            }
            Ast::ImplDef(span, target, methods, attrs) => {
                let mut module_ast = shared_imports.clone();
                module_ast.push(Ast::ImplDef(span, target.clone(), methods, attrs.clone()));
                lowered.push(sigil::StagedModuleAst {
                    module_path: target,
                    doc_module_path: None,
                    ast: module_ast,
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                    process_spec: None,
                });
            }
            Ast::Import(_, _, _) => {}
            other => shared_global_defs.push(other),
        }
    }

    if !shared_global_defs.is_empty() {
        let mut module_ast = shared_imports;
        module_ast.extend(shared_global_defs);
        lowered.push(sigil::StagedModuleAst {
            module_path: fallback_module_path.unwrap_or_default().to_string(),
            doc_module_path: None,
            ast: module_ast,
            module_doc: None,
            auto_import: false,
            process_spec: None,
        });
    }

    lowered
}

fn diagnostic_from_span(
    kind: AnalysisDiagnosticKind,
    severity: AnalysisSeverity,
    document: &DocumentSnapshot,
    start: usize,
    end: usize,
    message: String,
) -> AnalysisDiagnostic {
    diagnostic_from_line_index(
        kind,
        severity,
        document.path.clone(),
        &document.line_index,
        start,
        end,
        message,
    )
}

fn diagnostic_from_line_index(
    kind: AnalysisDiagnosticKind,
    severity: AnalysisSeverity,
    path: PathBuf,
    line_index: &LineIndex,
    start: usize,
    end: usize,
    message: String,
) -> AnalysisDiagnostic {
    AnalysisDiagnostic {
        kind,
        severity,
        path,
        range: Some(AnalysisRange {
            start: line_index.byte_to_utf16_position(start),
            end: line_index.byte_to_utf16_position(end),
        }),
        message,
    }
}

fn diagnostics_from_context(context: &ResolvedAnalysisContext) -> Vec<AnalysisDiagnostic> {
    let mut diagnostics = Vec::new();

    for diagnostic in &context.diagnostics {
        diagnostics.push(AnalysisDiagnostic {
            kind: AnalysisDiagnosticKind::ContextSelection,
            severity: match context.status {
                AnalysisContextStatus::Ready => AnalysisSeverity::Information,
                AnalysisContextStatus::NeedsSelection | AnalysisContextStatus::Invalid => {
                    AnalysisSeverity::Warning
                }
            },
            path: diagnostic
                .path
                .clone()
                .unwrap_or_else(|| context.context.active_file.clone()),
            range: None,
            message: diagnostic.message.clone(),
        });
    }

    if let Some(runner) = context.runner.as_ref() {
        for diagnostic in &runner.diagnostics {
            diagnostics.push(AnalysisDiagnostic {
                kind: AnalysisDiagnosticKind::ProjectRunner,
                severity: AnalysisSeverity::Warning,
                path: diagnostic
                    .path
                    .clone()
                    .unwrap_or_else(|| runner.project_file.clone()),
                range: diagnostic.span.map(|span| AnalysisRange {
                    start: Utf16Position {
                        line: 0,
                        character: span.start,
                    },
                    end: Utf16Position {
                        line: 0,
                        character: span.end,
                    },
                }),
                message: diagnostic.message.clone(),
            });
        }
    }

    diagnostics
}

fn compile_unit_kind_for_mode(mode: &AnalysisMode) -> CompileUnitKind {
    match mode {
        AnalysisMode::Script => CompileUnitKind::Script,
        AnalysisMode::DefinitionCheck => CompileUnitKind::DefinitionCheck,
        AnalysisMode::Project => CompileUnitKind::Project,
        AnalysisMode::ReplPreview => CompileUnitKind::Repl,
    }
}

fn compile_unit_kind_for_active_context(context: &ResolvedAnalysisContext) -> CompileUnitKind {
    if matches!(context.context.mode, AnalysisMode::Project)
        && context
            .context
            .entry_file
            .as_ref()
            .is_some_and(|entry| entry == &context.context.active_file)
    {
        return CompileUnitKind::Project;
    }
    if matches!(context.context.mode, AnalysisMode::Project) {
        return CompileUnitKind::DefinitionCheck;
    }
    compile_unit_kind_for_mode(&context.context.mode)
}

fn should_analyze_project_stages(context: &ResolvedAnalysisContext) -> bool {
    matches!(context.context.mode, AnalysisMode::Project)
        && context.runner.is_some()
        && !context
            .context
            .entry_file
            .as_ref()
            .is_some_and(|entry| entry == &context.context.active_file)
}

fn typecheck_context_for_mode(mode: &AnalysisMode) -> scar::TypecheckContext {
    let mut context = scar::TypecheckContext::default();
    if matches!(mode, AnalysisMode::ReplPreview) {
        context.runtime_policy = sindr::policy::RuntimeSourcePolicy::repl_chunk();
    }
    context
}

fn module_path_for_document(mode: AnalysisMode, path: &Path) -> Option<String> {
    match mode {
        AnalysisMode::Project => None,
        AnalysisMode::Script | AnalysisMode::DefinitionCheck | AnalysisMode::ReplPreview => path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem.to_string()),
    }
}

#[allow(dead_code)]
fn _text_position_for_byte(line_index: &LineIndex, byte_offset: usize) -> TextPosition {
    line_index.byte_to_text_position(byte_offset)
}
