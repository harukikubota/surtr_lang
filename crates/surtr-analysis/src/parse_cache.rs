use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sindr::policy::{CompileUnitKind, SourceKind};
use spire::{ast::Ast, error::ParseError, TolerantParseResult};

use crate::{parse_document, parse_document_tolerant};

const MAX_CACHED_DOCUMENTS: usize = 64;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ParseInput<'a> {
    pub source: &'a str,
    pub source_id: u32,
    pub source_kind: SourceKind,
    pub compile_unit_kind: CompileUnitKind,
    pub module_path: Option<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DocumentParseCache {
    entries: Arc<Mutex<ParseEntries>>,
    #[cfg(test)]
    stats: Arc<Mutex<ParseStats>>,
}

#[derive(Debug, Default)]
struct ParseEntries {
    documents: VecDeque<CachedDocument>,
}

#[derive(Debug)]
struct CachedDocument {
    path: PathBuf,
    source: String,
    source_id: u32,
    source_kind: SourceKind,
    compile_unit_kind: CompileUnitKind,
    module_path: Option<String>,
    strict: Option<Arc<Result<Vec<Ast>, ParseError>>>,
    tolerant: Option<(Option<usize>, Arc<TolerantParseResult>)>,
}

impl CachedDocument {
    fn matches(&self, input: ParseInput<'_>) -> bool {
        self.source == input.source
            && self.source_id == input.source_id
            && self.source_kind == input.source_kind
            && self.compile_unit_kind == input.compile_unit_kind
            && self.module_path.as_deref() == input.module_path
    }
}

impl ParseEntries {
    fn current(&mut self, path: &Path, input: ParseInput<'_>) -> &mut CachedDocument {
        let previous = self
            .documents
            .iter()
            .position(|entry| entry.path == path)
            .and_then(|index| self.documents.remove(index));
        let current = previous
            .filter(|entry| entry.matches(input))
            .unwrap_or_else(|| CachedDocument {
                path: path.to_path_buf(),
                source: input.source.to_owned(),
                source_id: input.source_id,
                source_kind: input.source_kind,
                compile_unit_kind: input.compile_unit_kind,
                module_path: input.module_path.map(str::to_owned),
                strict: None,
                tolerant: None,
            });
        self.documents.push_front(current);
        self.documents.truncate(MAX_CACHED_DOCUMENTS);
        self.documents
            .front_mut()
            .expect("inserted current document")
    }
}

impl DocumentParseCache {
    // Shared service clones may have different hosts or document contents. Only
    // these complete, immutable parser inputs authorize reuse; no host state or
    // semantic result is cached. Parsing and result cloning happen outside locks.
    pub fn strict(&self, path: &Path, input: ParseInput<'_>) -> Result<Vec<Ast>, ParseError> {
        let cached = {
            let mut entries = self.entries.lock().expect("parse cache lock poisoned");
            entries.current(path, input).strict.clone()
        };
        if let Some(cached) = cached {
            return (*cached).clone();
        }
        #[cfg(test)]
        let started = std::time::Instant::now();
        let result = parse_document(
            input.source,
            input.source_id,
            input.source_kind,
            input.compile_unit_kind,
            input.module_path.map(str::to_owned),
        );
        #[cfg(test)]
        {
            let mut stats = self.stats.lock().expect("parse statistics lock poisoned");
            stats.strict_calls += 1;
            stats.strict_elapsed += started.elapsed();
        }
        let cached = Arc::new(result.clone());
        self.entries
            .lock()
            .expect("parse cache lock poisoned")
            .current(path, input)
            .strict = Some(cached);
        result
    }

    pub fn tolerant(
        &self,
        path: &Path,
        input: ParseInput<'_>,
        cursor_char_offset: Option<usize>,
    ) -> TolerantParseResult {
        let cached = {
            let mut entries = self.entries.lock().expect("parse cache lock poisoned");
            entries
                .current(path, input)
                .tolerant
                .as_ref()
                .filter(|(cursor, _)| *cursor == cursor_char_offset)
                .map(|(_, parsed)| Arc::clone(parsed))
        };
        if let Some(cached) = cached {
            return (*cached).clone();
        }
        #[cfg(test)]
        let started = std::time::Instant::now();
        let result = parse_document_tolerant(
            input.source,
            input.source_id,
            input.source_kind,
            input.compile_unit_kind,
            input.module_path.map(str::to_owned),
            cursor_char_offset,
        );
        #[cfg(test)]
        {
            let mut stats = self.stats.lock().expect("parse statistics lock poisoned");
            stats.tolerant_calls += 1;
            stats.tolerant_elapsed += started.elapsed();
        }
        let cached = Arc::new(result.clone());
        self.entries
            .lock()
            .expect("parse cache lock poisoned")
            .current(path, input)
            .tolerant = Some((cursor_char_offset, cached));
        result
    }

    pub fn remove(&self, path: &Path) {
        self.entries
            .lock()
            .expect("parse cache lock poisoned")
            .documents
            .retain(|entry| entry.path != path);
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub(crate) struct ParseStats {
    pub strict_calls: usize,
    pub tolerant_calls: usize,
    strict_elapsed: std::time::Duration,
    tolerant_elapsed: std::time::Duration,
}

#[cfg(test)]
impl DocumentParseCache {
    pub(crate) fn stats(&self) -> ParseStats {
        self.stats
            .lock()
            .expect("parse statistics lock poisoned")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(source: &str) -> ParseInput<'_> {
        ParseInput {
            source,
            source_id: 0,
            source_kind: SourceKind::DefinitionSource,
            compile_unit_kind: CompileUnitKind::DefinitionCheck,
            module_path: None,
        }
    }

    fn assert_tolerant_eq(left: &TolerantParseResult, right: &TolerantParseResult) {
        assert_eq!(left.ast, right.ast);
        assert_eq!(left.diagnostics.len(), right.diagnostics.len());
        for (left, right) in left.diagnostics.iter().zip(&right.diagnostics) {
            assert_eq!(left.error, right.error);
            assert_eq!(left.expected_tokens, right.expected_tokens);
            assert_eq!(left.cursor_span, right.cursor_span);
        }
        assert_eq!(left.tokens, right.tokens);
        assert_eq!(left.outline, right.outline);
        assert_eq!(left.cursor_context, right.cursor_context);
    }

    #[test]
    fn parse_cache_reuses_success_and_malformed_source_without_losing_diagnostics() {
        let cache = DocumentParseCache::default();
        let path = Path::new("/repo/sample.srt");
        for (index, source) in [
            "defmod Sample { def value() -> Int { 1 } }",
            "defmod Sample { def value( ",
        ]
        .into_iter()
        .enumerate()
        {
            let input = input(source);
            let strict =
                parse_document(source, 0, input.source_kind, input.compile_unit_kind, None);
            let tolerant = parse_document_tolerant(
                source,
                0,
                input.source_kind,
                input.compile_unit_kind,
                None,
                None,
            );
            assert_eq!(strict.is_err(), index == 1);
            assert_eq!(tolerant.diagnostics.is_empty(), index == 0);
            for _ in 0..3 {
                assert_eq!(cache.strict(path, input), strict);
                assert_tolerant_eq(&cache.tolerant(path, input, None), &tolerant);
            }
            assert_eq!(cache.stats().strict_calls, index + 1);
            assert_eq!(cache.stats().tolerant_calls, index + 1);
        }
    }

    #[test]
    fn parse_cache_separates_every_parser_input_and_tolerant_cursor() {
        let cache = DocumentParseCache::default();
        let path = Path::new("/repo/sample.srt");
        let original = input("defmod Sample { def value() -> Int { 1 } }");
        let variants = [
            original,
            ParseInput {
                source: "defmod Sample { def value() -> Int { 2 } }",
                ..original
            },
            ParseInput {
                source_id: 2,
                ..original
            },
            ParseInput {
                source_kind: SourceKind::StdDefinitionSource,
                ..original
            },
            ParseInput {
                compile_unit_kind: CompileUnitKind::Project,
                ..original
            },
            ParseInput {
                module_path: Some("Other"),
                ..original
            },
            original,
        ];
        for (index, input) in variants.into_iter().enumerate() {
            let strict = parse_document(
                input.source,
                input.source_id,
                input.source_kind,
                input.compile_unit_kind,
                input.module_path.map(str::to_owned),
            );
            let tolerant = parse_document_tolerant(
                input.source,
                input.source_id,
                input.source_kind,
                input.compile_unit_kind,
                input.module_path.map(str::to_owned),
                None,
            );
            for _ in 0..2 {
                assert_eq!(cache.strict(path, input), strict);
                assert_tolerant_eq(&cache.tolerant(path, input, None), &tolerant);
            }
            assert_eq!(cache.stats().strict_calls, index + 1);
            assert_eq!(cache.stats().tolerant_calls, index + 1);
        }
        for cursor in [Some(4), Some(12)] {
            let expected = parse_document_tolerant(
                original.source,
                0,
                original.source_kind,
                original.compile_unit_kind,
                None,
                cursor,
            );
            for _ in 0..2 {
                assert_tolerant_eq(&cache.tolerant(path, original, cursor), &expected);
            }
        }
        assert_eq!(cache.stats().tolerant_calls, variants.len() + 2);
        assert_eq!(cache.stats().strict_calls, variants.len());
    }

    #[test]
    fn parse_cache_bounds_documents_and_discards_old_inputs() {
        let cache = DocumentParseCache::default();
        let original = input("defmod Sample { def value() -> Int { 1 } }");
        let changed = input("defmod Sample { def value() -> Int { 2 } }");
        let path = Path::new("/repo/current.srt");
        for input in [original, changed, original] {
            cache.strict(path, input).unwrap();
        }
        assert_eq!(cache.stats().strict_calls, 3);
        assert_eq!(cache.entries.lock().unwrap().documents.len(), 1);

        for index in 0..MAX_CACHED_DOCUMENTS {
            cache
                .strict(&PathBuf::from(format!("/repo/{index}.srt")), original)
                .unwrap();
        }
        assert_eq!(
            cache.entries.lock().unwrap().documents.len(),
            MAX_CACHED_DOCUMENTS
        );
        cache.strict(path, original).unwrap();
        assert_eq!(cache.stats().strict_calls, MAX_CACHED_DOCUMENTS + 4);
        cache.strict(path, original).unwrap();
        assert_eq!(cache.stats().strict_calls, MAX_CACHED_DOCUMENTS + 4);
        cache.remove(path);
        cache.strict(path, original).unwrap();
        assert_eq!(cache.stats().strict_calls, MAX_CACHED_DOCUMENTS + 5);
    }

    #[test]
    fn parse_cache_repeated_document_measurement() {
        let cache = DocumentParseCache::default();
        let path = Path::new("/repo/large.srt");
        let source = (0..256)
            .map(|index| format!("defmod Value{index} {{ def get() -> Int {{ {index} }} }}\n"))
            .collect::<String>();
        let input = input(&source);
        let started = std::time::Instant::now();
        for _ in 0..32 {
            assert_eq!(cache.strict(path, input).unwrap().len(), 256);
            assert!(cache.tolerant(path, input, None).diagnostics.is_empty());
        }
        let elapsed = started.elapsed();
        let stats = cache.stats();
        eprintln!("parse measurement: strict_calls={} tolerant_calls={} strict_us={} tolerant_us={} total_us={}", stats.strict_calls, stats.tolerant_calls, stats.strict_elapsed.as_micros(), stats.tolerant_elapsed.as_micros(), elapsed.as_micros());
        assert_eq!(stats.strict_calls, 1);
        assert_eq!(stats.tolerant_calls, 1);
    }
}
