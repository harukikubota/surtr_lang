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
            if deduped.iter().any(|existing: &CompletionSymbol| {
                existing.label == symbol.label && existing.kind == symbol.kind
            }) {
                continue;
            }
            deduped.push(symbol);
        }
        Self { symbols: deduped }
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
        .filter(|symbol| symbol.label.starts_with(&prefix))
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
