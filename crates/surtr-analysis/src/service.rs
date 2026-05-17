use std::path::{Path, PathBuf};

use sindr::policy::CompileUnitKind;
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
                compile_unit_kind_for_mode(&context.context.mode),
                module_path_for_document(
                    context.context.mode.clone(),
                    &context.context.active_file,
                ),
            ) {
                Ok(ast) => {
                    match sigil::resolve(ast.clone()) {
                        Ok(resolved_nodes) => {
                            match scar::typecheck_with_context(
                                resolved_nodes.clone(),
                                typecheck_context_for_mode(&context.context.mode),
                            ) {
                                Ok(typed_nodes) => typed = Some(typed_nodes),
                                Err(error) => diagnostics.push(diagnostic_from_span(
                                    AnalysisDiagnosticKind::Typecheck,
                                    AnalysisSeverity::Error,
                                    document,
                                    error.span.start,
                                    error.span.end,
                                    error.message,
                                )),
                            }
                            resolved = Some(resolved_nodes);
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

fn diagnostic_from_span(
    kind: AnalysisDiagnosticKind,
    severity: AnalysisSeverity,
    document: &DocumentSnapshot,
    start: usize,
    end: usize,
    message: String,
) -> AnalysisDiagnostic {
    AnalysisDiagnostic {
        kind,
        severity,
        path: document.path.clone(),
        range: Some(AnalysisRange {
            start: document.line_index.byte_to_utf16_position(start),
            end: document.line_index.byte_to_utf16_position(end),
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
