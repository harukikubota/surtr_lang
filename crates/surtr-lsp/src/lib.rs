use std::path::{Path, PathBuf};

use surtr_analysis::{
    resolve_context, AnalysisContextRequest, AnalysisDiagnosticKind, AnalysisRange,
    AnalysisService, AnalysisSeverity, CompletionKind, DocumentVersion, RunnerSelection,
    SelectedContext, SemanticIndex, Utf16Position,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspDiagnostic {
    pub range: LspRange,
    pub severity: DiagnosticSeverity,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionItemKind {
    Variable,
    Function,
    Constructor,
    Module,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspTextEdit {
    pub range: LspRange,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspCompletionItem {
    pub label: String,
    pub kind: CompletionItemKind,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub sort_text: Option<String>,
    pub text_edit: LspTextEdit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspDocumentSymbol {
    pub name: String,
    pub detail: Option<String>,
    pub range: LspRange,
    pub selection_range: LspRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspHover {
    pub range: Option<LspRange>,
    pub contents: String,
}

#[derive(Debug, Clone)]
pub struct LspAnalysisHost {
    workspace_root: PathBuf,
    selected_context: Option<SelectedContext>,
    runner_selection: Option<RunnerSelection>,
    service: AnalysisService,
}

impl LspAnalysisHost {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            selected_context: None,
            runner_selection: None,
            service: AnalysisService::new(),
        }
    }

    pub fn did_open(&mut self, uri: String, version: Option<i64>, text: String) -> Option<()> {
        let path = file_uri_to_path(&uri)?;
        self.service.update_document(path, version, text);
        Some(())
    }

    pub fn did_change(&mut self, uri: &str, version: Option<i64>, text: String) -> Option<()> {
        let path = file_uri_to_path(uri)?;
        self.service.update_document(path, version, text);
        Some(())
    }

    pub fn did_close(&mut self, uri: &str) -> Option<()> {
        let path = file_uri_to_path(uri)?;
        self.service.remove_document(&path);
        Some(())
    }

    pub fn set_selected_context(&mut self, selected_context: Option<SelectedContext>) {
        self.selected_context = selected_context;
    }

    pub fn set_runner_selection(&mut self, runner_selection: Option<RunnerSelection>) {
        self.runner_selection = runner_selection;
    }

    pub fn set_semantic_index(&mut self, semantic_index: SemanticIndex) {
        self.service.set_semantic_index(semantic_index);
    }

    fn snapshot_for_uri(&self, uri: &str) -> Option<surtr_analysis::AnalysisSnapshot> {
        let active_file = file_uri_to_path(uri)?;
        let context = resolve_context(AnalysisContextRequest {
            workspace_root: self.workspace_root.clone(),
            active_file,
            selected_context: self.selected_context.clone(),
            runner_selection: self.runner_selection.clone(),
            open_documents: self.open_document_versions(),
        });
        Some(self.service.analyze(context))
    }

    fn open_document_versions(&self) -> Vec<DocumentVersion> {
        self.service.document_store().open_document_versions()
    }
}

pub fn diagnostics(host: &LspAnalysisHost, uri: &str) -> Vec<LspDiagnostic> {
    let Some(snapshot) = host.snapshot_for_uri(uri) else {
        return Vec::new();
    };

    host.service
        .diagnostics(&snapshot)
        .into_iter()
        .map(|diagnostic| LspDiagnostic {
            range: diagnostic.range.map(lsp_range).unwrap_or_else(zero_range),
            severity: diagnostic_severity(diagnostic.severity),
            source: diagnostic_source(diagnostic.kind).to_string(),
            message: diagnostic.message,
        })
        .collect()
}

pub fn completion_items(
    host: &LspAnalysisHost,
    uri: &str,
    position: LspPosition,
) -> Vec<LspCompletionItem> {
    let Some(snapshot) = host.snapshot_for_uri(uri) else {
        return Vec::new();
    };
    let Some(document) = snapshot.active_document.as_ref() else {
        return Vec::new();
    };

    host.service
        .completions(&snapshot, utf16_position(position))
        .candidates
        .into_iter()
        .map(|candidate| {
            let range = LspRange {
                start: lsp_position(
                    document
                        .line_index
                        .byte_to_utf16_position(candidate.replace_start),
                ),
                end: lsp_position(
                    document
                        .line_index
                        .byte_to_utf16_position(candidate.replace_end),
                ),
            };
            LspCompletionItem {
                label: candidate.label,
                kind: completion_item_kind(candidate.kind),
                detail: candidate.detail,
                documentation: candidate.documentation,
                sort_text: candidate.sort_text,
                text_edit: LspTextEdit {
                    range,
                    new_text: candidate.replacement,
                },
            }
        })
        .collect()
}

pub fn document_symbols(host: &LspAnalysisHost, uri: &str) -> Vec<LspDocumentSymbol> {
    let Some(snapshot) = host.snapshot_for_uri(uri) else {
        return Vec::new();
    };
    let Some(path) = file_uri_to_path(uri) else {
        return Vec::new();
    };

    host.service
        .document_symbols(&snapshot, &path)
        .into_iter()
        .map(|symbol| LspDocumentSymbol {
            name: symbol.name,
            detail: symbol.detail,
            range: lsp_range(symbol.range),
            selection_range: lsp_range(symbol.selection_range),
        })
        .collect()
}

pub fn hover(host: &LspAnalysisHost, uri: &str, position: LspPosition) -> Option<LspHover> {
    let snapshot = host.snapshot_for_uri(uri)?;
    host.service
        .hover(&snapshot, utf16_position(position))
        .map(|hover| LspHover {
            range: hover.range.map(lsp_range),
            contents: hover.contents,
        })
}

pub fn path_to_file_uri(path: &Path) -> String {
    format!("file://{}", percent_encode(&path.to_string_lossy()))
}

pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    percent_decode(path).map(PathBuf::from)
}

fn lsp_range(range: AnalysisRange) -> LspRange {
    LspRange {
        start: lsp_position(range.start),
        end: lsp_position(range.end),
    }
}

fn lsp_position(position: Utf16Position) -> LspPosition {
    LspPosition {
        line: position.line,
        character: position.character,
    }
}

fn utf16_position(position: LspPosition) -> Utf16Position {
    Utf16Position {
        line: position.line,
        character: position.character,
    }
}

fn zero_range() -> LspRange {
    LspRange {
        start: LspPosition {
            line: 0,
            character: 0,
        },
        end: LspPosition {
            line: 0,
            character: 0,
        },
    }
}

fn diagnostic_severity(severity: AnalysisSeverity) -> DiagnosticSeverity {
    match severity {
        AnalysisSeverity::Error => DiagnosticSeverity::Error,
        AnalysisSeverity::Warning => DiagnosticSeverity::Warning,
        AnalysisSeverity::Information => DiagnosticSeverity::Information,
    }
}

fn diagnostic_source(kind: AnalysisDiagnosticKind) -> &'static str {
    match kind {
        AnalysisDiagnosticKind::ContextSelection => "surtr:context",
        AnalysisDiagnosticKind::ProjectRunner => "surtr:project-runner",
        AnalysisDiagnosticKind::Parse => "surtr:parse",
        AnalysisDiagnosticKind::Resolve => "surtr:resolve",
        AnalysisDiagnosticKind::Typecheck => "surtr:typecheck",
        AnalysisDiagnosticKind::DocumentMissing => "surtr:document",
    }
}

fn completion_item_kind(kind: CompletionKind) -> CompletionItemKind {
    match kind {
        CompletionKind::Variable => CompletionItemKind::Variable,
        CompletionKind::FunctionCall => CompletionItemKind::Function,
        CompletionKind::TypeConstructor => CompletionItemKind::Constructor,
        CompletionKind::TypePath => CompletionItemKind::Module,
    }
}

fn percent_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        if is_uri_path_byte(byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn percent_decode(input: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut iter = input.as_bytes().iter().copied();
    while let Some(byte) = iter.next() {
        if byte != b'%' {
            bytes.push(byte);
            continue;
        }
        let high = hex_value(iter.next()?)?;
        let low = hex_value(iter.next()?)?;
        bytes.push((high << 4) | low);
    }
    String::from_utf8(bytes).ok()
}

fn is_uri_path_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~')
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
