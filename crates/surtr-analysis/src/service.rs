use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use sindr::names::{surface_path_name, FacetRootKind};
use sindr::policy::{CompileUnitKind, SourceKind};
use spire::ast::{Ast, Lit, RecordLitArg, Span};
use spire::{SyntaxOutlineItem, SyntaxOutlineKind};

use crate::{
    complete_prefix, extract_project_runner_input, lookup_symbol_at_cursor, parse_document,
    parse_document_tolerant, repl_assist_at_cursor, resolve_context, resolve_project_runner_with,
    signature_help_at_cursor, AnalysisContextRequest, AnalysisContextStatus, AnalysisMode,
    AnalysisSpan, CompletionKind, CompletionRequest, CompletionResponse, CompletionScope,
    CompletionSymbol, DocumentSnapshot, DocumentStore, LineIndex, ProjectRunnerInput,
    ProjectRunnerSourceInput, ReplAssist, ResolvedAnalysisContext, RunnerContext, RunnerDiagnostic,
    RunnerDiagnosticKind, ScriptProjectContext, SelectedContext, SemanticIndex, SourceLocation,
    TextPosition, Utf16Position,
};

pub trait AnalysisHost: std::fmt::Debug + Send + Sync {
    fn read_to_string(&self, path: &Path) -> Option<String>;

    fn resolve_project_runner(&self, input: ProjectRunnerInput) -> RunnerContext {
        resolve_project_runner_with(input, |path| self.read_to_string(path))
    }
}

#[derive(Debug, Default)]
struct FsAnalysisHost;

impl AnalysisHost for FsAnalysisHost {
    fn read_to_string(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }
}

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
    pub editor_ast: Option<Vec<Ast>>,
    pub syntax_outline: Vec<SyntaxOutlineItem>,
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

#[derive(Debug, Clone)]
pub struct AnalysisService {
    documents: DocumentStore,
    semantic_index: SemanticIndex,
    host: Arc<dyn AnalysisHost>,
}

impl AnalysisService {
    pub fn new() -> Self {
        Self::with_host(Arc::new(FsAnalysisHost))
    }

    pub fn with_host(host: Arc<dyn AnalysisHost>) -> Self {
        Self {
            documents: DocumentStore::default(),
            semantic_index: SemanticIndex::default(),
            host,
        }
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

    pub fn resolve_context(&self, request: AnalysisContextRequest) -> ResolvedAnalysisContext {
        let mut context = resolve_context(request.clone());
        if let Some((script_project, runner)) =
            self.resolve_load_project_script_context(&request, &context)
        {
            context.script_project = Some(script_project);
            context.runner = runner;
        }
        context
    }

    pub fn analyze(&self, context: ResolvedAnalysisContext) -> AnalysisSnapshot {
        let mut diagnostics = diagnostics_from_context(&context);
        let active_document = self.documents.get(&context.context.active_file).cloned();
        let mut resolved = None;
        let mut typed = None;
        let mut semantic_index = self.semantic_index.clone();
        let mut editor_ast = None;
        let mut syntax_outline = Vec::new();

        let ast = if let Some(document) = active_document.as_ref() {
            let module_path = module_path_for_document(
                context.context.mode.clone(),
                &context.context.active_file,
            );
            let compile_unit_kind = compile_unit_kind_for_active_context(&context);
            let tolerant = parse_document_tolerant(
                &document.text,
                0,
                context.context.source_kind,
                compile_unit_kind,
                module_path.clone(),
                None,
            );
            editor_ast = Some(tolerant.ast.clone());
            syntax_outline = tolerant.outline.clone();

            match parse_document(
                &document.text,
                0,
                context.context.source_kind,
                compile_unit_kind,
                module_path,
            ) {
                Ok(ast) => {
                    semantic_index =
                        semantic_index_with_source_locations(&semantic_index, &document.path, &ast);
                    if should_analyze_project_stages(&context) {
                        analyze_project_stages(
                            self,
                            &context,
                            document,
                            Some(&ast),
                            &mut diagnostics,
                            &mut resolved,
                            &mut typed,
                            &mut semantic_index,
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
                    if tolerant.diagnostics.is_empty() {
                        let span = error.span();
                        diagnostics.push(diagnostic_from_span(
                            AnalysisDiagnosticKind::Parse,
                            AnalysisSeverity::Error,
                            document,
                            span.start,
                            span.end,
                            error.message(),
                        ));
                    } else {
                        diagnostics.extend(tolerant.diagnostics.into_iter().map(|diagnostic| {
                            let span = diagnostic.error.span();
                            diagnostic_from_span(
                                AnalysisDiagnosticKind::Parse,
                                AnalysisSeverity::Error,
                                document,
                                span.start,
                                span.end,
                                diagnostic.error.message(),
                            )
                        }));
                    }
                    if should_analyze_project_stages(&context) {
                        analyze_project_stages(
                            self,
                            &context,
                            document,
                            None,
                            &mut diagnostics,
                            &mut resolved,
                            &mut typed,
                            &mut semantic_index,
                        );
                    }
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
            editor_ast,
            syntax_outline,
            resolved,
            typed,
            semantic_index,
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

        let request = CompletionRequest {
            index: &snapshot.semantic_index,
            source: &document.text,
            cursor,
        };
        crate::complete_call_argument(request).unwrap_or_else(|| complete_prefix(request))
    }

    pub fn repl_assist(
        &self,
        snapshot: &AnalysisSnapshot,
        position: Utf16Position,
        scope: CompletionScope,
    ) -> ReplAssist {
        let Some(document) = snapshot.active_document.as_ref() else {
            return ReplAssist::default();
        };
        let Some(cursor) = document.line_index.utf16_position_to_byte(position) else {
            return ReplAssist::default();
        };

        repl_assist_at_cursor(
            CompletionRequest {
                index: &snapshot.semantic_index,
                source: &document.text,
                cursor,
            },
            scope,
        )
    }

    pub fn hover(
        &self,
        snapshot: &AnalysisSnapshot,
        position: Utf16Position,
    ) -> Option<HoverResult> {
        let document = snapshot.active_document.as_ref()?;
        let cursor = document.line_index.utf16_position_to_byte(position)?;
        let lookup = lookup_symbol_at_cursor(&snapshot.semantic_index, &document.text, cursor)?;
        let contents = hover_contents(
            lookup.symbol.detail.as_deref(),
            lookup.symbol.documentation.as_deref(),
        )?;
        Some(HoverResult {
            range: Some(AnalysisRange {
                start: document.line_index.byte_to_utf16_position(lookup.start),
                end: document.line_index.byte_to_utf16_position(lookup.end),
            }),
            contents,
        })
    }

    pub fn signature_help(
        &self,
        snapshot: &AnalysisSnapshot,
        position: Utf16Position,
    ) -> Option<SignatureHelpResult> {
        let document = snapshot.active_document.as_ref()?;
        let cursor = document.line_index.utf16_position_to_byte(position)?;
        let lookup = signature_help_at_cursor(&snapshot.semantic_index, &document.text, cursor)?;
        Some(SignatureHelpResult {
            signatures: vec![lookup.signature],
            active_signature: Some(0),
            active_parameter: Some(lookup.active_parameter),
        })
    }

    pub fn definition(
        &self,
        snapshot: &AnalysisSnapshot,
        position: Utf16Position,
    ) -> Vec<Location> {
        let Some(document) = snapshot.active_document.as_ref() else {
            return Vec::new();
        };
        let Some(cursor) = document.line_index.utf16_position_to_byte(position) else {
            return Vec::new();
        };
        let Some(lookup) =
            lookup_symbol_at_cursor(&snapshot.semantic_index, &document.text, cursor)
        else {
            return Vec::new();
        };
        let Some(location) = lookup.symbol.definition else {
            return Vec::new();
        };

        location_from_source_location(self, snapshot, location)
            .into_iter()
            .collect()
    }

    pub fn document_symbols(
        &self,
        snapshot: &AnalysisSnapshot,
        active_file: &Path,
    ) -> Vec<DocumentSymbol> {
        let Some(document) = snapshot.active_document.as_ref() else {
            return Vec::new();
        };
        if document.path != active_file {
            return Vec::new();
        }
        let mut symbols = Vec::new();
        if let Some(ast) = snapshot.ast.as_ref().or(snapshot.editor_ast.as_ref()) {
            collect_document_symbols(ast, None, &document.line_index, &mut symbols);
        }
        collect_outline_document_symbols(
            &snapshot.syntax_outline,
            None,
            &document.line_index,
            &mut symbols,
        );
        symbols.sort_by_key(|symbol| {
            (
                symbol.range.start.line,
                symbol.range.start.character,
                symbol.name.clone(),
            )
        });
        symbols.dedup_by(|left, right| {
            left.name == right.name
                && left.detail == right.detail
                && analysis_ranges_overlap(&left.range, &right.range)
        });
        symbols
    }
}

impl Default for AnalysisService {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisService {
    fn resolve_load_project_script_context(
        &self,
        request: &AnalysisContextRequest,
        context: &ResolvedAnalysisContext,
    ) -> Option<(ScriptProjectContext, Option<crate::RunnerContext>)> {
        if !matches!(context.context.mode, AnalysisMode::Script) {
            return None;
        }
        let script_file = match request.selected_context.as_ref() {
            Some(SelectedContext::ScriptEntry(path)) => path.clone(),
            Some(SelectedContext::DefinitionUnderEntry { entry_file }) => entry_file.clone(),
            _ => context.context.entry_file.clone()?,
        };

        let script_source = self.source_for_path(&script_file)?;
        let directive = match extract_load_project_directive(&script_source) {
            Ok(Some(directive)) => directive,
            Ok(None) => return None,
            Err(diagnostic) => {
                return Some((
                    ScriptProjectContext {
                        directive_span: diagnostic.span,
                        project_file: None,
                        profile: None,
                        project_context: None,
                        diagnostics: vec![diagnostic],
                    },
                    None,
                ));
            }
        };

        let project_file = resolve_relative_path(&script_file, &directive.project_literal);
        let mut diagnostics = Vec::new();
        let project_source = match self.source_for_path(&project_file) {
            Some(source) => source,
            None => {
                diagnostics.push(RunnerDiagnostic {
                    kind: RunnerDiagnosticKind::UnreadablePath,
                    path: Some(project_file.clone()),
                    span: directive.span,
                    message: format!(
                        "load_project could not read project file {}",
                        path_value(&project_file)
                    ),
                });
                return Some((
                    ScriptProjectContext {
                        directive_span: directive.span,
                        project_file: Some(project_file),
                        profile: Some(directive.profile),
                        project_context: None,
                        diagnostics,
                    },
                    None,
                ));
            }
        };

        let selected_profile = directive.profile.clone();
        let runner_input = ProjectRunnerSourceInput {
            project_file: project_file.clone(),
            selected_profile: selected_profile.clone(),
            normalized_args: vec![("profile".to_string(), selected_profile.clone())],
            active_file: Some(request.active_file.clone()),
            source: project_source,
        };

        let runner = match extract_project_runner_input(runner_input) {
            Ok(input) => Some(self.host.resolve_project_runner(input)),
            Err(mut runner_diagnostics) => {
                diagnostics.append(&mut runner_diagnostics);
                None
            }
        };

        Some((
            ScriptProjectContext {
                directive_span: directive.span,
                project_file: Some(project_file),
                profile: Some(selected_profile),
                project_context: runner.clone(),
                diagnostics,
            },
            runner,
        ))
    }

    fn source_for_path(&self, path: &Path) -> Option<String> {
        self.documents
            .get(path)
            .map(|document| document.text.clone())
            .or_else(|| self.host.read_to_string(path))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoadProjectDirective {
    project_literal: String,
    profile: String,
    span: Option<AnalysisSpan>,
}

fn extract_load_project_directive(
    source: &str,
) -> Result<Option<LoadProjectDirective>, RunnerDiagnostic> {
    let ast = parse_document(source, 0, SourceKind::Script, CompileUnitKind::Script, None)
        .map_err(|error| {
            let span = error.span();
            RunnerDiagnostic {
                kind: RunnerDiagnosticKind::LoadProjectUnsupported,
                path: None,
                span: Some(analysis_span(span)),
                message: error.message(),
            }
        })?;

    let Some(first_non_include) = ast.iter().find(|node| !matches!(node, Ast::Include(_, _)))
    else {
        return Ok(None);
    };
    let Ast::App(span, callee, args) = first_non_include else {
        return Ok(None);
    };
    if !is_path(callee, &["load_project"]) {
        return Ok(None);
    }

    let positional = positional_args(args);
    let Some((project_literal, _)) = positional.first().and_then(|arg| string_lit_with_span(arg))
    else {
        return Err(load_project_error(
            span,
            "load_project expects a literal project path as the first argument",
        ));
    };
    let profile = named_arg(args, "profile")
        .and_then(string_lit_with_span)
        .map(|(profile, _)| profile.to_string())
        .or_else(|| {
            positional
                .get(1)
                .and_then(|arg| string_lit_with_span(arg))
                .map(|(profile, _)| profile.to_string())
        })
        .unwrap_or_else(|| "main".to_string());

    Ok(Some(LoadProjectDirective {
        project_literal: project_literal.to_string(),
        profile,
        span: Some(analysis_span(span)),
    }))
}

fn load_project_error(span: &Span, message: impl Into<String>) -> RunnerDiagnostic {
    RunnerDiagnostic {
        kind: RunnerDiagnosticKind::LoadProjectUnsupported,
        path: None,
        span: Some(analysis_span(span)),
        message: message.into(),
    }
}

fn location_from_source_location(
    service: &AnalysisService,
    snapshot: &AnalysisSnapshot,
    location: SourceLocation,
) -> Option<Location> {
    let line_index = snapshot
        .active_document
        .as_ref()
        .filter(|document| document.path == location.path)
        .map(|document| document.line_index.clone())
        .or_else(|| {
            service
                .documents
                .get(&location.path)
                .map(|document| document.line_index.clone())
        })
        .or_else(|| {
            service
                .host
                .read_to_string(&location.path)
                .map(|source| LineIndex::new(&source))
        })?;

    Some(Location {
        path: location.path,
        range: AnalysisRange {
            start: line_index.char_to_utf16_position(location.start),
            end: line_index.char_to_utf16_position(location.end),
        },
    })
}

fn collect_document_symbols(
    ast: &[Ast],
    owner: Option<&str>,
    line_index: &LineIndex,
    out: &mut Vec<DocumentSymbol>,
) {
    for node in ast {
        if let Some(symbol) = document_symbol_for_ast(node, owner, line_index) {
            let nested_owner = nested_owner_for_ast(node, owner);
            out.push(symbol);
            if let Some((body, owner_name)) = module_body_for_ast(node).zip(nested_owner.as_deref())
            {
                collect_document_symbols(body, Some(owner_name), line_index, out);
            }
        }
    }
}

fn collect_outline_document_symbols(
    outline: &[SyntaxOutlineItem],
    owner: Option<&str>,
    line_index: &LineIndex,
    out: &mut Vec<DocumentSymbol>,
) {
    for item in outline {
        let Some(symbol) = document_symbol_for_outline(item, owner, line_index) else {
            continue;
        };
        let nested_owner = item
            .name
            .as_deref()
            .filter(|_| {
                matches!(
                    item.kind,
                    SyntaxOutlineKind::Module | SyntaxOutlineKind::Impl
                )
            })
            .map(|name| qualify_symbol(owner, name));
        out.push(symbol);
        collect_outline_document_symbols(
            &item.children,
            nested_owner.as_deref().or(owner),
            line_index,
            out,
        );
    }
}

fn document_symbol_for_outline(
    item: &SyntaxOutlineItem,
    owner: Option<&str>,
    line_index: &LineIndex,
) -> Option<DocumentSymbol> {
    let name = item
        .name
        .as_deref()
        .map(|name| qualify_symbol(owner, name))
        .unwrap_or_else(|| "<anonymous>".to_string());
    let detail = match item.kind {
        SyntaxOutlineKind::Function => Some("function".to_string()),
        SyntaxOutlineKind::Extractor => Some("extractor".to_string()),
        SyntaxOutlineKind::Const => Some("const".to_string()),
        SyntaxOutlineKind::Struct => Some("struct".to_string()),
        SyntaxOutlineKind::Record => Some("record".to_string()),
        SyntaxOutlineKind::Error => Some("error".to_string()),
        SyntaxOutlineKind::Enum => Some("enum".to_string()),
        SyntaxOutlineKind::Module => Some("module".to_string()),
        SyntaxOutlineKind::Impl => Some("impl".to_string()),
        SyntaxOutlineKind::Trait => Some("trait".to_string()),
        SyntaxOutlineKind::TraitImpl => Some("trait impl".to_string()),
        SyntaxOutlineKind::Import => Some("import".to_string()),
        SyntaxOutlineKind::Include => Some("include".to_string()),
    };
    Some(DocumentSymbol {
        name,
        detail,
        range: analysis_range_for_span(line_index, &item.span),
        selection_range: analysis_range_for_span(line_index, &item.selection_span),
    })
}

fn document_symbol_for_ast(
    node: &Ast,
    owner: Option<&str>,
    line_index: &LineIndex,
) -> Option<DocumentSymbol> {
    let (name, detail, span) = match node {
        Ast::Def(span, name, ..) => (
            qualify_symbol(owner, name),
            Some("function".to_string()),
            span,
        ),
        Ast::ExtractorDef(span, name, ..) => (
            qualify_symbol(owner, name),
            Some("extractor".to_string()),
            span,
        ),
        Ast::ConstDef(span, name, ..) => {
            (qualify_symbol(owner, name), Some("const".to_string()), span)
        }
        Ast::StructDef(span, name, ..) => (
            qualify_symbol(owner, name),
            Some("struct".to_string()),
            span,
        ),
        Ast::RecordDef(span, name, ..) => (
            qualify_symbol(owner, name),
            Some("record".to_string()),
            span,
        ),
        Ast::DeferrorDef(span, name, ..) => {
            (qualify_symbol(owner, name), Some("error".to_string()), span)
        }
        Ast::EnumDef(span, name, ..) => {
            (qualify_symbol(owner, name), Some("enum".to_string()), span)
        }
        Ast::Defmod(span, name, ..)
        | Ast::Defagent(span, name, ..)
        | Ast::Defgenserver(span, name, ..)
        | Ast::Defsupervisor(span, name, ..)
        | Ast::DefdynamicSupervisor(span, name, ..) => (
            qualify_symbol(owner, name),
            Some("module".to_string()),
            span,
        ),
        Ast::ImplDef(span, target, ..) => (
            qualify_symbol(owner, target),
            Some("impl".to_string()),
            span,
        ),
        Ast::TraitDef(span, name, ..) => {
            (qualify_symbol(owner, name), Some("trait".to_string()), span)
        }
        Ast::TraitImplDef(span, trait_name, _, target, ..) => (
            qualify_symbol(owner, &format!("impl {trait_name} for {target:?}")),
            Some("trait impl".to_string()),
            span,
        ),
        _ => return None,
    };
    let range = analysis_range_for_span(line_index, span);
    Some(DocumentSymbol {
        name,
        detail,
        range,
        selection_range: range,
    })
}

fn semantic_index_with_source_locations(
    existing: &SemanticIndex,
    path: &Path,
    ast: &[Ast],
) -> SemanticIndex {
    let mut symbols = existing.symbols().to_vec();
    collect_source_location_symbols(ast, None, path, &mut symbols);
    SemanticIndex::from_symbols(symbols)
}

fn collect_source_location_symbols(
    ast: &[Ast],
    owner: Option<&str>,
    path: &Path,
    out: &mut Vec<CompletionSymbol>,
) {
    for node in ast {
        if let Some(symbol) = source_location_symbol_for_ast(node, owner, path) {
            let nested_owner = nested_owner_for_ast(node, owner);
            out.push(symbol);
            if let Some((body, owner_name)) = module_body_for_ast(node).zip(nested_owner.as_deref())
            {
                collect_source_location_symbols(body, Some(owner_name), path, out);
            }
        }
    }
}

fn source_location_symbol_for_ast(
    node: &Ast,
    owner: Option<&str>,
    path: &Path,
) -> Option<CompletionSymbol> {
    let (name, kind, span, capabilities) = match node {
        Ast::Def(span, name, ..) | Ast::ExtractorDef(span, name, ..) => (
            qualify_symbol(owner, name),
            CompletionKind::FunctionCall,
            span,
            None,
        ),
        Ast::ConstDef(span, name, ..) => (
            qualify_symbol(owner, name),
            CompletionKind::Variable,
            span,
            None,
        ),
        Ast::StructDef(span, name, ..)
        | Ast::RecordDef(span, name, ..)
        | Ast::EnumDef(span, name, ..) => (
            qualify_symbol(owner, name),
            CompletionKind::TypeConstructor,
            span,
            Some(crate::semantic::facet_root_capabilities(
                FacetRootKind::TypeRoot,
            )),
        ),
        Ast::DeferrorDef(span, name, ..) => (
            qualify_symbol(owner, name),
            CompletionKind::TypeConstructor,
            span,
            None,
        ),
        Ast::Defmod(span, name, ..)
        | Ast::Defagent(span, name, ..)
        | Ast::Defgenserver(span, name, ..)
        | Ast::Defsupervisor(span, name, ..)
        | Ast::DefdynamicSupervisor(span, name, ..)
        | Ast::TraitDef(span, name, ..) => (
            qualify_symbol(owner, name),
            CompletionKind::TypePath,
            span,
            None,
        ),
        Ast::ImplDef(span, target, ..) => (
            qualify_symbol(owner, target),
            CompletionKind::TypePath,
            span,
            None,
        ),
        _ => return None,
    };

    Some(CompletionSymbol {
        replacement: name.clone(),
        label: name,
        kind,
        detail: None,
        documentation: None,
        sort_text: None,
        origin: None,
        definition: Some(SourceLocation {
            path: path.to_path_buf(),
            start: span.start,
            end: span.end,
        }),
        capabilities,
    })
}

fn hover_contents(detail: Option<&str>, documentation: Option<&str>) -> Option<String> {
    match (detail, documentation) {
        (Some(detail), Some(documentation)) if !documentation.is_empty() => {
            Some(format!("{detail}\n\n{documentation}"))
        }
        (Some(detail), _) => Some(detail.to_string()),
        (None, Some(documentation)) if !documentation.is_empty() => Some(documentation.to_string()),
        _ => None,
    }
}

fn nested_owner_for_ast(node: &Ast, owner: Option<&str>) -> Option<String> {
    match node {
        Ast::Defmod(_, name, ..)
        | Ast::Defagent(_, name, ..)
        | Ast::Defgenserver(_, name, ..)
        | Ast::Defsupervisor(_, name, ..)
        | Ast::DefdynamicSupervisor(_, name, ..)
        | Ast::ImplDef(_, name, ..) => Some(qualify_symbol(owner, name)),
        _ => None,
    }
}

fn module_body_for_ast(node: &Ast) -> Option<&[Ast]> {
    match node {
        Ast::Defmod(_, _, body, _)
        | Ast::Defagent(_, _, body, ..)
        | Ast::Defgenserver(_, _, body, ..)
        | Ast::Defsupervisor(_, _, body, ..)
        | Ast::DefdynamicSupervisor(_, _, body, ..)
        | Ast::ImplDef(_, _, body, _) => Some(body),
        _ => None,
    }
}

fn qualify_symbol(owner: Option<&str>, name: &str) -> String {
    let qualified = owner
        .filter(|owner| !owner.is_empty() && !name.contains("::"))
        .map(|owner| format!("{owner}::{name}"))
        .unwrap_or_else(|| name.to_string());
    sindr::names::surface_rendered_name(&qualified)
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
                typecheck_context_for_analysis(context),
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
    active_ast: Option<&[Ast]>,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
    resolved: &mut Option<Vec<sigil::resolved::Resolved>>,
    typed: &mut Option<Vec<scar::typed::TypedNode>>,
    semantic_index: &mut SemanticIndex,
) {
    let Some(runner) = context.runner.as_ref() else {
        return;
    };
    let Some(module_stages) = build_staged_modules(
        service,
        context,
        runner,
        active_document,
        diagnostics,
        semantic_index,
    ) else {
        return;
    };
    let visible_ast = if matches!(context.context.mode, AnalysisMode::Script)
        && context.script_project.is_some()
    {
        project_user_ast_for_active_document(context, active_ast)
    } else {
        active_ast.unwrap_or(&[]).to_vec()
    };

    match sigil::precollect_declaration_index(&module_stages) {
        Ok(declaration_index) => {
            *semantic_index = semantic_index_with_declarations(
                semantic_index,
                &declaration_index,
                &module_stages,
                &visible_ast,
                completion_module_path_for_ast(&visible_ast).as_deref(),
                active_stage_index_for_document(runner, active_document),
            );
            let user_ast = project_user_ast_for_active_document(context, active_ast);
            match sigil::resolve_staged_program_with_state(
                &module_stages,
                user_ast,
                &declaration_index,
                None,
            ) {
                Ok(resolved_program) => {
                    let resolved_nodes = resolved_program.resolved.clone();
                    match scar::typecheck_staged_program_with_context(
                        resolved_program,
                        typecheck_context_for_analysis(context),
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
            }
        }
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

fn project_user_ast_for_active_document(
    context: &ResolvedAnalysisContext,
    active_ast: Option<&[Ast]>,
) -> Vec<Ast> {
    if !matches!(context.context.mode, AnalysisMode::Script) || context.script_project.is_none() {
        return Vec::new();
    }

    let Some(active_ast) = active_ast else {
        return Vec::new();
    };

    let mut removed_load_project = false;
    active_ast
        .iter()
        .filter_map(|stmt| {
            if !removed_load_project && is_load_project_statement(stmt) {
                removed_load_project = true;
                None
            } else {
                Some(stmt.clone())
            }
        })
        .collect()
}

fn is_load_project_statement(stmt: &Ast) -> bool {
    matches!(stmt, Ast::App(_, callee, _) if is_path(callee, &["load_project"]))
}

fn semantic_index_with_declarations(
    existing: &SemanticIndex,
    declaration_index: &sigil::DeclarationIndex,
    module_stages: &[Vec<sigil::StagedModuleAst>],
    active_ast: &[Ast],
    current_module_path: Option<&str>,
    current_stage_index: usize,
) -> SemanticIndex {
    let mut symbols = existing.symbols().to_vec();
    symbols.extend(
        SemanticIndex::from_compile_metadata(declaration_index, &[], &[])
            .symbols()
            .iter()
            .cloned(),
    );
    if let Ok(visible_entries) = sigil::effective_visible_entries(
        module_stages,
        active_ast,
        current_module_path,
        current_stage_index,
    ) {
        let visible_symbols = visible_entries
            .into_iter()
            .filter_map(|visible| completion_symbol_for_effective_visible_entry(&symbols, visible))
            .collect::<Vec<_>>();
        symbols.extend(visible_symbols);
    }
    SemanticIndex::from_symbols(symbols)
}

fn completion_symbol_for_effective_visible_entry(
    existing_symbols: &[CompletionSymbol],
    visible: sigil::EffectiveVisibleEntry,
) -> Option<CompletionSymbol> {
    crate::semantic::completion_symbol_for_effective_visible_entry(existing_symbols, &visible)
}

fn active_stage_index_for_document(
    runner: &crate::RunnerContext,
    active_document: &DocumentSnapshot,
) -> usize {
    runner
        .module_stages
        .iter()
        .enumerate()
        .find_map(|(stage_index, stage)| {
            stage.files.iter().any(|file| file.path == active_document.path).then_some(stage_index)
        })
        .unwrap_or_else(|| runner.module_stages.len().saturating_sub(1))
}

fn completion_module_path_for_ast(ast: &[Ast]) -> Option<String> {
    let mut module_paths = BTreeSet::new();
    for stmt in ast {
        match stmt {
            Ast::Defmod(_, module_path, ..)
            | Ast::Defagent(_, module_path, ..)
            | Ast::Defgenserver(_, module_path, ..)
            | Ast::Defsupervisor(_, module_path, ..)
            | Ast::DefdynamicSupervisor(_, module_path, ..) => {
                module_paths.insert(module_path.clone());
            }
            Ast::ImplDef(_, target, ..) => {
                module_paths.insert(target.clone());
            }
            Ast::TraitImplDef(_, _, _, target_ty, ..) => match target_ty {
                spire::ast::AstTy::Named(_, name)
                | spire::ast::AstTy::ImplTrait(_, name)
                | spire::ast::AstTy::Generic(_, name, _) => {
                    module_paths.insert(name.clone());
                }
                _ => {}
            },
            _ => {}
        }
    }
    (module_paths.len() == 1).then(|| module_paths.into_iter().next().unwrap())
}

fn build_staged_modules(
    service: &AnalysisService,
    context: &ResolvedAnalysisContext,
    runner: &crate::RunnerContext,
    active_document: &DocumentSnapshot,
    diagnostics: &mut Vec<AnalysisDiagnostic>,
    semantic_index: &mut SemanticIndex,
) -> Option<Vec<Vec<sigil::StagedModuleAst>>> {
    let compile_unit_kind = compile_unit_kind_for_mode(&context.context.mode);
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
                    file.source_kind,
                    compile_unit_kind,
                    None,
                )
            } else {
                parse_document(
                    &source,
                    0,
                    file.source_kind,
                    compile_unit_kind,
                    None,
                )
            };
            match ast {
                Ok(ast) => {
                    *semantic_index =
                        semantic_index_with_source_locations(semantic_index, &file.path, &ast);
                    staged_modules.extend(lower_module_ast(ast, None));
                }
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
        .or_else(|| service.host.read_to_string(path))
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
    let mut shared_namespace_consts = Vec::new();
    let mut shared_result_ctor_contracts = Vec::new();

    for stmt in ast {
        match stmt {
            Ast::Defmod(_span, module_path, body, attrs) => {
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
            Ast::Defagent(span, module_path, body, process_spec, attrs)
            | Ast::Defgenserver(span, module_path, body, process_spec, attrs)
            | Ast::Defsupervisor(span, module_path, body, process_spec, attrs)
            | Ast::DefdynamicSupervisor(span, module_path, body, process_spec, attrs) => {
                let _ = span;
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
                let _ = span;
                let mut module_ast = shared_imports.clone();
                let (local_imports, methods) = partition_nested_imports(methods);
                module_ast.extend(local_imports);
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
            Ast::TraitImplDef(span, trait_name, trait_args, target_ty, methods, attrs) => {
                let module_path = match &target_ty {
                    spire::ast::AstTy::Named(_, name)
                    | spire::ast::AstTy::ImplTrait(_, name)
                    | spire::ast::AstTy::Generic(_, name, _) => name.clone(),
                    _ => fallback_module_path.unwrap_or_default().to_string(),
                };
                let mut module_ast = shared_imports.clone();
                let (local_imports, methods) = partition_nested_imports(methods);
                module_ast.extend(local_imports);
                module_ast.push(Ast::TraitImplDef(
                    span,
                    trait_name,
                    trait_args,
                    target_ty,
                    methods,
                    attrs.clone(),
                ));
                lowered.push(sigil::StagedModuleAst {
                    module_path,
                    doc_module_path: fallback_module_path.map(str::to_string),
                    ast: module_ast,
                    module_doc: attrs.doc,
                    auto_import: attrs.auto_import,
                    process_spec: None,
                });
            }
            Ast::Import(_, _, _) => {}
            Ast::ResultCtorDecl(_, _, _, _, _) => shared_result_ctor_contracts.push(stmt),
            Ast::ConstDef(_, _, _, _, _) => shared_namespace_consts.push(stmt),
            Ast::StructDef(..)
            | Ast::RecordDef(..)
            | Ast::DeferrorDef(_, _, _, _, _)
            | Ast::EnumDef(_, _, _, _, _)
            | Ast::BuiltinDecl(_, _, _, _, _)
            | Ast::IntrinsicDecl(_, _, _, _)
            | Ast::BuiltinTypeDecl(_, _, _) => shared_global_defs.push(stmt),
            _ => shared_global_defs.push(stmt),
        }
    }

    if !shared_namespace_consts.is_empty() {
        if let Some(idx) = find_fallback_namespace_module(&lowered, fallback_module_path)
            .or_else(|| (lowered.len() == 1).then_some(0))
        {
            let insert_at = first_non_import_index(&lowered[idx].ast);
            lowered[idx]
                .ast
                .splice(insert_at..insert_at, shared_namespace_consts);
        } else {
            let mut shared_ast = shared_imports.clone();
            shared_ast.extend(shared_namespace_consts);
            lowered.push(sigil::StagedModuleAst {
                module_path: fallback_module_path.unwrap_or_default().to_string(),
                doc_module_path: None,
                ast: shared_ast,
                module_doc: None,
                auto_import: false,
                process_spec: None,
            });
        }
    }

    if !shared_result_ctor_contracts.is_empty() {
        if let Some(idx) =
            find_result_owner_module(&lowered).or_else(|| (lowered.len() == 1).then_some(0))
        {
            let insert_at = first_non_import_index(&lowered[idx].ast);
            lowered[idx]
                .ast
                .splice(insert_at..insert_at, shared_result_ctor_contracts);
        } else {
            let mut shared_ast = shared_imports.clone();
            shared_ast.extend(shared_result_ctor_contracts);
            lowered.push(sigil::StagedModuleAst {
                module_path: fallback_module_path.unwrap_or_default().to_string(),
                doc_module_path: None,
                ast: shared_ast,
                module_doc: None,
                auto_import: false,
                process_spec: None,
            });
        }
    }

    if !shared_global_defs.is_empty() {
        let mut shared_ast = shared_imports;
        shared_ast.extend(shared_global_defs);
        lowered.push(sigil::StagedModuleAst {
            module_path: fallback_module_path.unwrap_or_default().to_string(),
            doc_module_path: None,
            ast: shared_ast,
            module_doc: None,
            auto_import: false,
            process_spec: None,
        });
    }

    lowered
}

fn partition_nested_imports(body: Vec<Ast>) -> (Vec<Ast>, Vec<Ast>) {
    let mut imports = Vec::new();
    let mut rest = Vec::new();
    for stmt in body {
        if matches!(stmt, Ast::Import(_, _, _)) {
            imports.push(stmt);
        } else {
            rest.push(stmt);
        }
    }
    (imports, rest)
}

fn first_non_import_index(ast: &[Ast]) -> usize {
    ast.iter()
        .take_while(|stmt| matches!(stmt, Ast::Import(_, _, _)))
        .count()
}

fn lowered_module_is_impl_owner(lowered: &sigil::StagedModuleAst) -> bool {
    matches!(
        lowered
            .ast
            .iter()
            .find(|stmt| !matches!(stmt, Ast::Import(_, _, _))),
        Some(Ast::ImplDef(_, _, _, _) | Ast::TraitImplDef(_, _, _, _, _, _))
    )
}

fn find_result_owner_module(lowered: &[sigil::StagedModuleAst]) -> Option<usize> {
    lowered.iter().position(|module| {
        surface_path_name(&module.module_path) == "Result"
            && matches!(
                module
                    .ast
                    .iter()
                    .find(|stmt| !matches!(stmt, Ast::Import(_, _, _))),
                Some(Ast::ImplDef(_, target, _, _)) if surface_path_name(target) == "Result"
            )
    })
}

fn find_fallback_namespace_module(
    lowered: &[sigil::StagedModuleAst],
    fallback_module_path: Option<&str>,
) -> Option<usize> {
    let fallback = fallback_module_path?;
    lowered
        .iter()
        .position(|module| module.module_path == fallback && !lowered_module_is_impl_owner(module))
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

fn analysis_range_for_span(line_index: &LineIndex, span: &Span) -> AnalysisRange {
    AnalysisRange {
        start: line_index.char_to_utf16_position(span.start),
        end: line_index.char_to_utf16_position(span.end),
    }
}

fn analysis_ranges_overlap(left: &AnalysisRange, right: &AnalysisRange) -> bool {
    !position_leq(left.end, right.start) && !position_leq(right.end, left.start)
}

fn position_leq(left: Utf16Position, right: Utf16Position) -> bool {
    left.line < right.line || (left.line == right.line && left.character <= right.character)
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
            start: line_index.char_to_utf16_position(start),
            end: line_index.char_to_utf16_position(end),
        }),
        message,
    }
}

fn positional_args(args: &[RecordLitArg]) -> Vec<&Ast> {
    args.iter()
        .filter_map(|arg| match arg {
            RecordLitArg::Positional(ast) => Some(ast),
            RecordLitArg::Named(_, _) => None,
        })
        .collect()
}

fn named_arg<'a>(args: &'a [RecordLitArg], expected: &str) -> Option<&'a Ast> {
    args.iter().find_map(|arg| match arg {
        RecordLitArg::Named(name, ast) if name == expected => Some(ast),
        _ => None,
    })
}

fn string_lit_with_span(node: &Ast) -> Option<(&str, &Span)> {
    match node {
        Ast::Lit(span, Lit::Str(value)) => Some((value.as_str(), span)),
        _ => None,
    }
}

fn is_path(node: &Ast, expected: &[&str]) -> bool {
    match node {
        Ast::Var(_, symbol) => expected == [symbol.as_str()],
        Ast::Path(_, path) => path
            .segments
            .iter()
            .map(String::as_str)
            .eq(expected.iter().copied()),
        _ => false,
    }
}

fn analysis_span(span: &Span) -> AnalysisSpan {
    AnalysisSpan {
        start: span.start.min(u32::MAX as usize) as u32,
        end: span.end.min(u32::MAX as usize) as u32,
    }
}

fn resolve_relative_path(base_file: &Path, raw_path: &str) -> PathBuf {
    let raw = PathBuf::from(raw_path);
    let path = if raw.is_absolute() {
        raw
    } else {
        base_file
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(raw)
    };
    path.components()
        .fold(PathBuf::new(), |mut normalized, component| {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    normalized.pop();
                }
                other => normalized.push(other.as_os_str()),
            }
            normalized
        })
}

fn path_value(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
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
    if context.runner.is_none() {
        return false;
    }
    if matches!(context.context.mode, AnalysisMode::Script) && context.script_project.is_some() {
        return true;
    }
    matches!(context.context.mode, AnalysisMode::Project)
        && !context
            .context
            .entry_file
            .as_ref()
            .is_some_and(|entry| entry == &context.context.active_file)
}

fn typecheck_context_for_analysis(context: &ResolvedAnalysisContext) -> scar::TypecheckContext {
    let compile_unit_kind = compile_unit_kind_for_mode(&context.context.mode);
    let entrypoint = context
        .runner
        .as_ref()
        .map(|runner| sindr::policy::EntryPoint::qualified(runner.entrypoint.clone()));

    scar::TypecheckContext {
        runtime_policy: context.context.source_kind.runtime_policy(
            compile_unit_kind,
            entrypoint.as_ref(),
        ),
        ..scar::TypecheckContext::default()
    }
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

#[cfg(test)]
mod tests {
    use super::lower_module_ast;

    #[test]
    fn lower_module_ast_hoists_impl_local_imports_like_xldr() {
        let ast = spire::parse_with_context(
            r#"
impl User {
  import Helper::help
  def use() -> Int { help() }
}
"#,
            spire::ParserContext::module(0, Some("User".to_string())),
        )
        .expect("module source should parse");

        let lowered = lower_module_ast(ast, Some("User"));
        let module = lowered.first().expect("impl owner module should exist");

        assert!(matches!(
            module.ast.first(),
            Some(spire::ast::Ast::Import(_, _, _))
        ));
        assert!(matches!(
            module.ast.get(1),
            Some(spire::ast::Ast::ImplDef(_, _, _, _))
        ));
    }
}
