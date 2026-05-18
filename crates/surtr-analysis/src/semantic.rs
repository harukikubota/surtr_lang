use std::collections::BTreeMap;
use std::path::PathBuf;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionScope {
    All,
    VariablesOnly,
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
    pub definition: Option<SourceLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub path: PathBuf,
    pub start: usize,
    pub end: usize,
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
                if existing.definition.is_none() {
                    existing.definition = symbol.definition;
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
            let qualified_name = surface_name(&entry.qualified_name);
            symbols.push(CompletionSymbol {
                label: qualified_name.clone(),
                replacement: qualified_name,
                kind: completion_kind_for_doc_kind(&entry.kind),
                detail: Some(entry.signature.clone()),
                documentation: None,
                sort_text: None,
                origin: Some(CompletionOrigin::Metadata {
                    qualified_name: entry.qualified_name.clone(),
                    module_path: entry.module_path.clone(),
                }),
                definition: None,
            });
        }
        for entry in docs {
            let detail = signature_by_name
                .get(entry.qualified_name.as_str())
                .map(|signature| (*signature).to_string())
                .or_else(|| entry.signature.clone());
            let qualified_name = surface_name(&entry.qualified_name);
            symbols.push(CompletionSymbol {
                label: qualified_name.clone(),
                replacement: qualified_name,
                kind: completion_kind_for_doc_kind(&entry.kind),
                detail,
                documentation: Some(entry.doc.clone()),
                sort_text: None,
                origin: Some(CompletionOrigin::Metadata {
                    qualified_name: entry.qualified_name.clone(),
                    module_path: entry.module_path.clone(),
                }),
                definition: None,
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
                    definition: None,
                });
            }

            if let Some(kind) = completion_kind_for_declaration_kind(&entry.kind) {
                let qualified_name = surface_name(&entry.fq_name);
                symbols.push(CompletionSymbol {
                    label: qualified_name.clone(),
                    replacement: qualified_name,
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
                    definition: None,
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

    pub fn upsert_symbol(&mut self, symbol: CompletionSymbol) {
        if let Some(existing) = self
            .symbols
            .iter_mut()
            .find(|existing| existing.label == symbol.label && existing.kind == symbol.kind)
        {
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
            if existing.definition.is_none() {
                existing.definition = symbol.definition;
            }
        } else {
            self.symbols.push(symbol);
        }
        self.symbols.sort_by(|left, right| {
            left.label
                .cmp(&right.label)
                .then_with(|| {
                    completion_kind_rank(&left.kind).cmp(&completion_kind_rank(&right.kind))
                })
                .then_with(|| left.replacement.cmp(&right.replacement))
        });
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplAssist {
    pub candidates: Vec<CompletionCandidate>,
    pub replace_start: usize,
    pub replace_end: usize,
    pub signature: Option<SignatureLookup>,
    pub active_parameter: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacetPathRootKind {
    TypeRoot,
    ValueRoot,
    ViewClosureRoot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetPathCompletionContext {
    pub root_kind: FacetPathRootKind,
    pub root: String,
    pub completed_segments: Vec<String>,
    pub prefix: String,
    pub current_path: String,
    pub replace_start: usize,
    pub replace_end: usize,
    pub token_start: usize,
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
    complete_prefix_with_options(request, CompletionScope::All, CompletionPresentation::Full)
}

pub fn complete_repl_prefix(
    request: CompletionRequest<'_>,
    scope: CompletionScope,
) -> CompletionResponse {
    complete_prefix_with_options(request, scope, CompletionPresentation::Repl)
}

pub fn repl_assist_at_cursor(request: CompletionRequest<'_>, scope: CompletionScope) -> ReplAssist {
    let completion = complete_repl_prefix(request, scope);
    let signature = signature_help_at_cursor(request.index, request.source, request.cursor);
    let active_parameter = signature
        .as_ref()
        .map(|signature| signature.active_parameter);

    ReplAssist {
        candidates: completion.candidates,
        replace_start: completion.replace_start,
        replace_end: completion.replace_end,
        signature,
        active_parameter,
    }
}

pub fn facet_path_context_at_cursor(
    source: &str,
    cursor: usize,
) -> Option<FacetPathCompletionContext> {
    let cursor = clamp_to_char_boundary(source, cursor);
    let before = &source[..cursor];
    let start = before
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| (!facet_path_token_char(ch)).then_some(idx + ch.len_utf8()))
        .unwrap_or(0);
    let token = &source[start..cursor];
    let (root_kind, body) = if let Some(rest) = token.strip_prefix('&') {
        (FacetPathRootKind::ViewClosureRoot, rest)
    } else {
        (FacetPathRootKind::ValueRoot, token)
    };
    let mut parts = split_facet_path_segments(body)?;
    if parts.len() < 2 {
        return None;
    }
    let prefix = parts.pop()?;
    let root = parts.first()?.to_string();
    if root.is_empty() || parts.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    let root_kind = match root_kind {
        FacetPathRootKind::ViewClosureRoot => {
            if !facet_type_root_name(&root) {
                return None;
            }
            FacetPathRootKind::ViewClosureRoot
        }
        FacetPathRootKind::ValueRoot if facet_type_root_name(&root) => FacetPathRootKind::TypeRoot,
        FacetPathRootKind::ValueRoot if facet_value_root_name(&root) => {
            FacetPathRootKind::ValueRoot
        }
        _ => return None,
    };
    Some(FacetPathCompletionContext {
        root_kind,
        root,
        completed_segments: parts
            .iter()
            .skip(1)
            .map(|segment| segment.to_string())
            .collect(),
        current_path: parts.join("."),
        replace_start: cursor.saturating_sub(prefix.len()),
        replace_end: cursor,
        prefix,
        token_start: start,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionPresentation {
    Full,
    Repl,
}

fn complete_prefix_with_options(
    request: CompletionRequest<'_>,
    scope: CompletionScope,
    presentation: CompletionPresentation,
) -> CompletionResponse {
    let cursor = clamp_to_char_boundary(request.source, request.cursor);
    let (replace_start, replace_end, prefix) = completion_token(request.source, cursor);
    let allow_empty_prefix =
        presentation == CompletionPresentation::Repl && scope == CompletionScope::VariablesOnly;
    if prefix.is_empty() && !allow_empty_prefix {
        return CompletionResponse {
            candidates: Vec::new(),
            replace_start,
            replace_end,
        };
    }

    let mut candidates = Vec::new();
    for symbol in request
        .index
        .symbols()
        .iter()
        .filter(|symbol| completion_scope_accepts(scope, symbol))
        .filter(|symbol| {
            prefix.is_empty()
                || completion_symbol_matches_prefix(
                    symbol,
                    &prefix,
                    presentation == CompletionPresentation::Full || prefix.contains("::"),
                )
        })
    {
        let mut candidate = CompletionCandidate {
            label: symbol.label.clone(),
            replacement: symbol.replacement.clone(),
            kind: symbol.kind.clone(),
            detail: symbol.detail.clone(),
            documentation: symbol.documentation.clone(),
            sort_text: symbol.sort_text.clone(),
            origin: symbol.origin.clone(),
            replace_start,
            replace_end,
        };
        apply_completion_presentation(&mut candidate, presentation, &prefix);
        if candidate.sort_text.is_none() {
            candidate.sort_text = Some(default_sort_text_for_candidate(&candidate));
        }
        push_completion_candidate(&mut candidates, candidate);
    }
    sort_completion_candidates(&mut candidates, presentation);

    CompletionResponse {
        candidates,
        replace_start,
        replace_end,
    }
}

fn completion_scope_accepts(scope: CompletionScope, symbol: &CompletionSymbol) -> bool {
    match scope {
        CompletionScope::All => true,
        CompletionScope::VariablesOnly => symbol.kind == CompletionKind::Variable,
    }
}

fn apply_completion_presentation(
    candidate: &mut CompletionCandidate,
    presentation: CompletionPresentation,
    prefix: &str,
) {
    if !prefix.contains("::") && is_builtin_special_variant_symbol(&candidate.label) {
        if let Some(tail) = candidate
            .label
            .rsplit_once("::")
            .map(|(_, tail)| tail.to_string())
        {
            candidate.label = tail.clone();
            candidate.replacement = tail;
        }
    }

    if presentation != CompletionPresentation::Repl {
        return;
    }

    if !prefix.contains("::") {
        if let Some(tail) = candidate
            .label
            .rsplit_once("::")
            .map(|(_, tail)| tail.to_string())
        {
            if tail.starts_with(prefix) {
                candidate.label = tail.clone();
                candidate.replacement = tail;
            }
        }
    } else if candidate.kind == CompletionKind::FunctionCall {
        candidate.kind = CompletionKind::TypePath;
    }
}

fn push_completion_candidate(
    candidates: &mut Vec<CompletionCandidate>,
    candidate: CompletionCandidate,
) {
    if let Some(existing) = candidates
        .iter_mut()
        .find(|existing| existing.label == candidate.label && existing.kind == candidate.kind)
    {
        if existing.detail.is_none() {
            existing.detail = candidate.detail;
        }
        if existing.documentation.is_none() {
            existing.documentation = candidate.documentation;
        }
        if existing.sort_text.is_none() {
            existing.sort_text = candidate.sort_text;
        }
        if existing.origin.is_none() {
            existing.origin = candidate.origin;
        }
        return;
    }
    candidates.push(candidate);
}

fn sort_completion_candidates(
    candidates: &mut [CompletionCandidate],
    presentation: CompletionPresentation,
) {
    match presentation {
        CompletionPresentation::Full => candidates.sort_by(|left, right| {
            left.label
                .cmp(&right.label)
                .then_with(|| {
                    completion_kind_rank(&left.kind).cmp(&completion_kind_rank(&right.kind))
                })
                .then_with(|| left.replacement.cmp(&right.replacement))
        }),
        CompletionPresentation::Repl => sort_repl_completion_candidates(candidates),
    }
}

fn sort_repl_completion_candidates(candidates: &mut [CompletionCandidate]) {
    candidates.sort_by(|left, right| {
        repl_completion_kind_rank(&left.kind)
            .cmp(&repl_completion_kind_rank(&right.kind))
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.replacement.cmp(&right.replacement))
    });
}

pub fn rank_completion_candidates_by_expected_type<F>(
    mut candidates: Vec<CompletionCandidate>,
    expected_type: Option<&str>,
    mut accepts: F,
) -> Vec<CompletionCandidate>
where
    F: FnMut(&str, &str) -> bool,
{
    let Some(expected_type) = expected_type else {
        return candidates;
    };

    candidates.sort_by_key(|candidate| {
        candidate
            .detail
            .as_deref()
            .is_none_or(|actual_type| !accepts(expected_type, actual_type))
    });
    candidates
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

fn facet_path_token_char(ch: char) -> bool {
    completion_token_char(ch) || matches!(ch, '.' | '&' | '[' | ']' | '"' | '?' | '-' | '+')
}

fn facet_type_root_name(name: &str) -> bool {
    name.rsplit("::")
        .next()
        .and_then(|segment| segment.chars().next())
        .is_some_and(char::is_uppercase)
}

fn facet_value_root_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase() || ch == '_')
}

fn split_facet_path_segments(input: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for ch in input.chars() {
        if in_string {
            current.push(ch);
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
            '"' => {
                in_string = true;
                current.push(ch);
            }
            '[' => {
                bracket_depth += 1;
                current.push(ch);
            }
            ']' => {
                bracket_depth = bracket_depth.checked_sub(1)?;
                current.push(ch);
            }
            '.' if bracket_depth == 0 => {
                out.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if bracket_depth != 0 || in_string {
        return None;
    }
    out.push(current.trim().to_string());
    Some(out)
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

fn completion_symbol_matches_prefix(
    symbol: &CompletionSymbol,
    prefix: &str,
    allow_tail_match: bool,
) -> bool {
    if !prefix.contains("::") && completion_symbol_hides_qualified_owner_match(symbol) {
        return allow_tail_match
            && !completion_symbol_hides_tail_match(symbol, prefix)
            && symbol
                .label
                .rsplit_once("::")
                .is_some_and(|(_, tail)| tail.starts_with(prefix));
    }
    symbol.label.starts_with(prefix)
        || (allow_tail_match
            && !completion_symbol_hides_tail_match(symbol, prefix)
            && symbol
                .label
                .rsplit_once("::")
                .is_some_and(|(_, tail)| tail.starts_with(prefix)))
}

fn completion_symbol_hides_qualified_owner_match(symbol: &CompletionSymbol) -> bool {
    symbol.kind == CompletionKind::FunctionCall
        && symbol.label.contains("::")
        && symbol
            .label
            .rsplit_once("::")
            .and_then(|(_, tail)| tail.chars().next())
            .is_some_and(char::is_uppercase)
}

fn completion_symbol_hides_tail_match(symbol: &CompletionSymbol, prefix: &str) -> bool {
    if prefix.contains("::") || symbol.kind != CompletionKind::FunctionCall {
        return false;
    }
    let Some((_, tail)) = symbol.label.rsplit_once("::") else {
        return false;
    };
    tail.chars().next().is_some_and(char::is_uppercase)
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

fn repl_completion_kind_rank(kind: &CompletionKind) -> u8 {
    match kind {
        CompletionKind::Variable => 0,
        CompletionKind::TypeConstructor => 1,
        CompletionKind::TypePath => 2,
        CompletionKind::FunctionCall => 3,
    }
}

fn default_sort_text_for_candidate(candidate: &CompletionCandidate) -> String {
    format!(
        "{}:{}",
        completion_kind_rank(&candidate.kind),
        candidate.label
    )
}

fn surface_name(name: &str) -> String {
    sindr::names::surface_rendered_name(name)
}

fn is_builtin_special_variant_symbol(name: &str) -> bool {
    matches!(
        surface_name(name).as_str(),
        "Result::Ok" | "Result::Err" | "Boolean::True" | "Boolean::False"
    )
}
