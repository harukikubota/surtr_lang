use std::collections::BTreeMap;

use sigil::{DeclarationIndex, DeclarationKind};
use sindr::ir::{DocEntry, DocKind, SignatureEntry};
use spire::ast::Visibility;

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
    pub documentation: Option<String>,
    pub sort_text: Option<String>,
    pub origin: Option<CompletionOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionOrigin {
    Metadata {
        qualified_name: String,
        module_path: String,
    },
    Declaration {
        qualified_name: String,
        module_path: String,
        name: String,
        stage_index: usize,
        auto_import: bool,
        visibility: Visibility,
        user_importable: bool,
        user_callable: bool,
    },
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
                if existing.documentation.is_none() {
                    existing.documentation = symbol.documentation;
                }
                if existing.sort_text.is_none() {
                    existing.sort_text = symbol.sort_text;
                }
                if existing.origin.is_none() {
                    existing.origin = symbol.origin;
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
                documentation: None,
                sort_text: None,
                origin: Some(CompletionOrigin::Metadata {
                    qualified_name: entry.qualified_name.clone(),
                    module_path: entry.module_path.clone(),
                }),
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
                documentation: Some(entry.doc.clone()),
                sort_text: None,
                origin: Some(CompletionOrigin::Metadata {
                    qualified_name: entry.qualified_name.clone(),
                    module_path: entry.module_path.clone(),
                }),
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
                    documentation: None,
                    sort_text: None,
                    origin: None,
                });
            }

            if let Some(kind) = completion_kind_for_declaration_kind(&entry.kind) {
                symbols.push(CompletionSymbol {
                    label: surface_name(&entry.fq_name),
                    replacement: surface_name(&entry.fq_name),
                    kind,
                    detail: None,
                    documentation: None,
                    sort_text: None,
                    origin: Some(CompletionOrigin::Declaration {
                        qualified_name: entry.fq_name.clone(),
                        module_path: entry.module_path.clone(),
                        name: entry.name.clone(),
                        stage_index: entry.stage_index,
                        auto_import: entry.auto_import,
                        visibility: entry.visibility,
                        user_importable: entry.user_importable,
                        user_callable: entry.user_callable,
                    }),
                });
            }
        }
        Self::from_symbols(symbols)
    }

    pub fn symbols(&self) -> &[CompletionSymbol] {
        &self.symbols
    }

    pub fn find_symbol(&self, name: &str) -> Option<&CompletionSymbol> {
        if let Some(symbol) = self.symbols.iter().find(|symbol| symbol.label == name) {
            return Some(symbol);
        }

        let mut tail_matches = self.symbols.iter().filter(|symbol| {
            symbol
                .label
                .rsplit_once("::")
                .is_some_and(|(_, tail)| tail == name)
        });
        let first = tail_matches.next()?;
        tail_matches.next().is_none().then_some(first)
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
    pub documentation: Option<String>,
    pub sort_text: Option<String>,
    pub origin: Option<CompletionOrigin>,
    pub replace_start: usize,
    pub replace_end: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompletionResponse {
    pub candidates: Vec<CompletionCandidate>,
    pub replace_start: usize,
    pub replace_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolLookup {
    pub symbol: CompletionSymbol,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureLookup {
    pub signature: String,
    pub active_parameter: usize,
    pub callee_start: usize,
    pub callee_end: usize,
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
            documentation: symbol.documentation.clone(),
            sort_text: symbol
                .sort_text
                .clone()
                .or_else(|| Some(default_sort_text(symbol))),
            origin: symbol.origin.clone(),
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

pub fn lookup_symbol_at_cursor(
    index: &SemanticIndex,
    source: &str,
    cursor: usize,
) -> Option<SymbolLookup> {
    let cursor = clamp_to_char_boundary(source, cursor);
    let (start, end, token) = symbol_token(source, cursor)?;
    let symbol = index.find_symbol(&token)?.clone();
    Some(SymbolLookup { symbol, start, end })
}

pub fn signature_help_at_cursor(
    index: &SemanticIndex,
    source: &str,
    cursor: usize,
) -> Option<SignatureLookup> {
    let cursor = clamp_to_char_boundary(source, cursor);
    let before = &source[..cursor];
    let open = innermost_unclosed_lparen(before)?;
    let callee_end = before[..open].trim_end().len();
    let callee_start = before[..callee_end]
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| (!completion_token_char(ch)).then_some(idx + ch.len_utf8()))
        .unwrap_or(0);
    let callee = before[callee_start..callee_end].trim();
    if callee.is_empty() {
        return None;
    }

    let symbol = index.find_symbol(callee)?;
    let signature = symbol.detail.clone()?;
    Some(SignatureLookup {
        signature,
        active_parameter: active_call_parameter(&before[open + 1..]),
        callee_start,
        callee_end,
    })
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

fn symbol_token(input: &str, cursor: usize) -> Option<(usize, usize, String)> {
    let cursor = cursor.min(input.len());
    let start = input[..cursor]
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| (!completion_token_char(ch)).then_some(idx + ch.len_utf8()))
        .unwrap_or(0);
    let end = input[cursor..]
        .char_indices()
        .find_map(|(idx, ch)| (!completion_token_char(ch)).then_some(cursor + idx))
        .unwrap_or(input.len());

    (start < end).then(|| (start, end, input[start..end].to_string()))
}

fn completion_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':')
}

fn innermost_unclosed_lparen(input: &str) -> Option<usize> {
    let mut stack = Vec::new();
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
            '(' => stack.push(idx),
            ')' => {
                stack.pop();
            }
            _ => {}
        }
    }

    stack.pop()
}

fn active_call_parameter(args: &str) -> usize {
    let mut active = 0usize;
    let mut paren_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for ch in args.chars() {
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
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            ',' if paren_depth == 0 && angle_depth == 0 && bracket_depth == 0 => active += 1,
            _ => {}
        }
    }

    active
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

fn default_sort_text(symbol: &CompletionSymbol) -> String {
    format!("{}:{}", completion_kind_rank(&symbol.kind), symbol.label)
}

fn surface_name(name: &str) -> String {
    sindr::names::surface_rendered_name(name)
}
