use serde::{Deserialize, Serialize};
use spire::ast::Span;

pub(crate) fn line_column_for_offset(source: &str, offset: usize) -> (u32, u32) {
    let mut line = 1u32;
    let mut column = 1u32;
    let limit = offset.min(source.chars().count());
    for ch in source.chars().take(limit) {
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

pub(crate) fn line_index_for_span(lines: &[(usize, usize)], pos: usize) -> Option<usize> {
    lines
        .iter()
        .position(|(start, end)| pos >= *start && pos <= *end)
}

pub(crate) fn line_spans(source: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = source.chars().collect();
    let mut spans = Vec::new();
    let mut start = 0usize;
    for (idx, ch) in chars.iter().enumerate() {
        if *ch == '\n' {
            spans.push((start, idx));
            start = idx + 1;
        }
    }
    spans.push((start, chars.len()));
    spans
}

pub(crate) fn slice_chars(source: &str, start: usize, end: usize) -> String {
    source
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

pub(crate) fn line_span_containing(source: &str, pos: usize) -> Option<(usize, usize)> {
    let lines = line_spans(source);
    let idx = line_index_for_span(&lines, pos)?;
    Some(lines[idx])
}

pub(crate) fn trimmed_line_span(source: &str, line: (usize, usize)) -> Option<Span> {
    let text = slice_chars(source, line.0, line.1);
    let chars = text.chars().collect::<Vec<_>>();
    let mut start = 0usize;
    let mut end = chars.len();
    while start < end && chars[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && chars[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    (start < end).then_some(Span {
        start: line.0 + start,
        end: line.0 + end,
    })
}

pub(crate) fn trimmed_line_span_containing(source: &str, pos: usize) -> Option<Span> {
    trimmed_line_span(source, line_span_containing(source, pos)?)
}

pub(crate) fn rewrite_line_at_span(source: &str, span: &Span, replacement: &str) -> Option<String> {
    let (line_start, line_end) = line_span_containing(source, span.start)?;
    if span.start < line_start || span.end > line_end || span.end < span.start {
        return None;
    }
    let before = slice_chars(source, line_start, span.start);
    let after = slice_chars(source, span.end, line_end);
    Some(
        format!("{}{}{}", before, replacement, after)
            .trim()
            .to_string(),
    )
}

pub(crate) fn normalized_char_span(source: &str, span: &Span) -> Span {
    let source_len = source.chars().count();
    if source_len == 0 {
        return Span { start: 0, end: 0 };
    }
    let mut start = span.start.min(source_len.saturating_sub(1));
    let mut end = span.end.min(source_len);
    if end <= start {
        end = (start + 1).min(source_len);
    }
    if end <= start {
        start = 0;
        end = 1.min(source_len);
    }
    Span { start, end }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    pub id: SourceId,
    pub file_name: String,
    pub source: String,
}

#[derive(Debug, Default, Clone)]
pub struct SourceRegistry {
    entries: Vec<SourceEntry>,
    next_id: u32,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        file_name: impl Into<String>,
        source: impl Into<String>,
    ) -> SourceId {
        let id = SourceId(self.next_id);
        self.next_id = self.next_id.checked_add(1).expect("source ID overflow");
        self.entries.push(SourceEntry {
            id,
            file_name: file_name.into(),
            source: source.into(),
        });
        id
    }

    pub fn get(&self, source_id: SourceId) -> Option<&SourceEntry> {
        self.entries
            .binary_search_by_key(&source_id.0, |entry| entry.id.0)
            .ok()
            .map(|index| &self.entries[index])
    }

    pub fn file_name(&self, source_id: SourceId) -> Option<&str> {
        self.get(source_id).map(|entry| entry.file_name.as_str())
    }

    pub fn source(&self, source_id: SourceId) -> Option<&str> {
        self.get(source_id).map(|entry| entry.source.as_str())
    }

    pub fn update_source(&mut self, source_id: SourceId, source: impl Into<String>) -> bool {
        if let Ok(index) = self
            .entries
            .binary_search_by_key(&source_id.0, |entry| entry.id.0)
        {
            let entry = &mut self.entries[index];
            entry.source = source.into();
            true
        } else {
            false
        }
    }

    pub fn owned_context(&self, source_id: SourceId) -> Option<(String, String)> {
        self.get(source_id)
            .map(|entry| (entry.source.clone(), entry.file_name.clone()))
    }

    pub fn entries(&self) -> &[SourceEntry] {
        &self.entries
    }

    /// Keep future compiler IDs distinct from sources in an already loaded VM image.
    pub fn reserve_ids_before(&mut self, next_id: u32) {
        self.next_id = self.next_id.max(next_id);
    }

    /// Restore a source under its original ID. Occupied IDs are never overwritten.
    pub fn restore(&mut self, entry: SourceEntry) -> bool {
        let Some(next_id) = entry.id.0.checked_add(1) else {
            return false;
        };
        match self
            .entries
            .binary_search_by_key(&entry.id.0, |source| source.id.0)
        {
            Ok(index) => {
                let existing = &self.entries[index];
                existing.file_name == entry.file_name && existing.source == entry.source
            }
            Err(index) => {
                self.next_id = self.next_id.max(next_id);
                self.entries.insert(index, entry);
                true
            }
        }
    }
}
