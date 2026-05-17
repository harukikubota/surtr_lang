use std::collections::BTreeMap;

use sigil::{DeclarationIndex, DeclarationKind};
use sindr::ir::{DocEntry, DocKind, SignatureEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionKind {
    Variable,
    TypeConstructor,
    TypePath,
    FunctionCall,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionSymbol {
    pub label: String,
    pub replacement: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticIndex {
    symbols: Vec<CompletionSymbol>,
}

impl SemanticIndex {
    pub fn from_symbols(symbols: Vec<CompletionSymbol>) -> Self {
        let mut deduped = Vec::new();
        for symbol in symbols {
            if let Some(existing) = deduped.iter_mut().find(|existing: &&mut CompletionSymbol| {
                existing.label == symbol.label && existing.kind == symbol.kind
            }) {
                if existing.detail.is_none() {
                    existing.detail = symbol.detail;
                }
                continue;
            }
            deduped.push(symbol);
        }
        deduped.sort_by(|left, right| {
            left.label
                .cmp(&right.label)
                .then_with(|| {
                    completion_kind_rank(&left.kind).cmp(&completion_kind_rank(&right.kind))
                })
                .then_with(|| left.replacement.cmp(&right.replacement))
        });
        Self { symbols: deduped }
    }

    pub fn from_metadata(docs: &[DocEntry], signatures: &[SignatureEntry]) -> Self {
        let signature_by_name = signatures
            .iter()
            .map(|entry| (entry.qualified_name.as_str(), entry.signature.as_str()))
            .collect::<BTreeMap<_, _>>();

        let mut symbols = Vec::new();
        for entry in signatures {
            symbols.push(CompletionSymbol {
                label: surface_name(&entry.qualified_name),
                replacement: surface_name(&entry.qualified_name),
                kind: completion_kind_for_doc_kind(&entry.kind),
                detail: Some(entry.signature.clone()),
            });
        }
        for entry in docs {
            let detail = signature_by_name
                .get(entry.qualified_name.as_str())
                .map(|signature| (*signature).to_string())
                .or_else(|| entry.signature.clone());
            symbols.push(CompletionSymbol {
                label: surface_name(&entry.qualified_name),
                replacement: surface_name(&entry.qualified_name),
                kind: completion_kind_for_doc_kind(&entry.kind),
                detail,
            });
        }

        Self::from_symbols(symbols)
    }

    pub fn from_declaration_index(declarations: &DeclarationIndex) -> Self {
        let mut symbols = Vec::new();
        for entry in declarations
            .values()
            .filter(|entry| !entry.hidden && (entry.user_importable || entry.user_callable))
        {
            if !entry.module_path.is_empty() {
                symbols.push(CompletionSymbol {
                    label: surface_name(&entry.module_path),
                    replacement: surface_name(&entry.module_path),
                    kind: CompletionKind::TypePath,
                    detail: None,
                });
            }

            if let Some(kind) = completion_kind_for_declaration_kind(&entry.kind) {
                symbols.push(CompletionSymbol {
                    label: surface_name(&entry.fq_name),
                    replacement: surface_name(&entry.fq_name),
                    kind,
                    detail: None,
                });
            }
        }
        Self::from_symbols(symbols)
    }

    pub fn symbols(&self) -> &[CompletionSymbol] {
        &self.symbols
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CompletionRequest<'a> {
    pub index: &'a SemanticIndex,
    pub source: &'a str,
    pub cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionCandidate {
    pub label: String,
    pub replacement: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    pub replace_start: usize,
    pub replace_end: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompletionResponse {
    pub candidates: Vec<CompletionCandidate>,
    pub replace_start: usize,
    pub replace_end: usize,
}

pub fn complete_prefix(request: CompletionRequest<'_>) -> CompletionResponse {
    let cursor = clamp_to_char_boundary(request.source, request.cursor);
    let (replace_start, replace_end, prefix) = completion_token(request.source, cursor);
    if prefix.is_empty() {
        return CompletionResponse {
            candidates: Vec::new(),
            replace_start,
            replace_end,
        };
    }

    let candidates = request
        .index
        .symbols()
        .iter()
        .filter(|symbol| completion_symbol_matches_prefix(symbol, &prefix))
        .map(|symbol| CompletionCandidate {
            label: symbol.label.clone(),
            replacement: symbol.replacement.clone(),
            kind: symbol.kind.clone(),
            detail: symbol.detail.clone(),
            replace_start,
            replace_end,
        })
        .collect();

    CompletionResponse {
        candidates,
        replace_start,
        replace_end,
    }
}

fn clamp_to_char_boundary(input: &str, mut cursor: usize) -> usize {
    cursor = cursor.min(input.len());
    while cursor > 0 && !input.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn completion_token(input: &str, cursor: usize) -> (usize, usize, String) {
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

fn completion_symbol_matches_prefix(symbol: &CompletionSymbol, prefix: &str) -> bool {
    symbol.label.starts_with(prefix)
        || symbol
            .label
            .rsplit_once("::")
            .is_some_and(|(_, tail)| tail.starts_with(prefix))
}

fn completion_kind_for_doc_kind(kind: &DocKind) -> CompletionKind {
    match kind {
        DocKind::Module => CompletionKind::TypePath,
        DocKind::Type => CompletionKind::TypeConstructor,
        DocKind::Function => CompletionKind::FunctionCall,
    }
}

fn completion_kind_for_declaration_kind(kind: &DeclarationKind) -> Option<CompletionKind> {
    match kind {
        DeclarationKind::Def
        | DeclarationKind::Extractor
        | DeclarationKind::TraitMethod
        | DeclarationKind::ResultCtor
        | DeclarationKind::ImplMethod
        | DeclarationKind::ImplCtorNew => Some(CompletionKind::FunctionCall),
        DeclarationKind::Struct
        | DeclarationKind::Record
        | DeclarationKind::Deferror
        | DeclarationKind::Enum
        | DeclarationKind::EnumVariant
        | DeclarationKind::BuiltinType => Some(CompletionKind::TypeConstructor),
        DeclarationKind::Const => Some(CompletionKind::Variable),
        DeclarationKind::Trait => Some(CompletionKind::TypePath),
    }
}

fn completion_kind_rank(kind: &CompletionKind) -> u8 {
    match kind {
        CompletionKind::Variable => 0,
        CompletionKind::FunctionCall => 1,
        CompletionKind::TypeConstructor => 2,
        CompletionKind::TypePath => 3,
    }
}

fn surface_name(name: &str) -> String {
    sindr::names::surface_rendered_name(name)
}
