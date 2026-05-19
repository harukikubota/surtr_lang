use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use sigil::{declaration_symbol_identity_info, DeclarationIndex, DeclarationKind};
use sindr::ir::{DocEntry, DocKind, SignatureEntry};
use sindr::names::{
    builtin_symbol_identity_info, FacetRootKind, SymbolCapabilities, TypeIdentity,
};
use spire::ast::{AstTy, Visibility};

use crate::query::{format_query_ty, parse_signature_type};

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
    pub capabilities: Option<SymbolCapabilities>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolSemanticInfo {
    pub canonical_name: String,
    pub surface_name: String,
    pub replacement: String,
    pub kind: CompletionKind,
    pub identity: Option<TypeIdentity>,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub sort_text: Option<String>,
    pub origin: Option<CompletionOrigin>,
    pub definition: Option<SourceLocation>,
    pub capabilities: Option<SymbolCapabilities>,
    pub display_metadata: Option<SymbolDisplayMetadata>,
}

impl SymbolSemanticInfo {
    pub fn from_completion_symbol(symbol: &CompletionSymbol) -> Self {
        Self {
            canonical_name: symbol
                .origin
                .as_ref()
                .map(|origin| match origin {
                    CompletionOrigin::Metadata { qualified_name, .. }
                    | CompletionOrigin::Declaration { qualified_name, .. } => {
                        qualified_name.clone()
                    }
                })
                .unwrap_or_else(|| symbol.label.clone()),
            surface_name: symbol.label.clone(),
            replacement: symbol.replacement.clone(),
            kind: symbol.kind.clone(),
            identity: None,
            detail: symbol.detail.clone(),
            documentation: symbol.documentation.clone(),
            sort_text: symbol.sort_text.clone(),
            origin: symbol.origin.clone(),
            definition: symbol.definition.clone(),
            capabilities: symbol.capabilities.clone(),
            display_metadata: None,
        }
    }

    fn completion_key(&self) -> (String, u8) {
        (self.surface_name.clone(), completion_kind_rank(&self.kind))
    }

    pub fn into_completion_symbol(self) -> CompletionSymbol {
        CompletionSymbol {
            label: self.surface_name,
            replacement: self.replacement,
            kind: self.kind,
            detail: self.detail,
            documentation: self.documentation,
            sort_text: self.sort_text,
            origin: self.origin,
            definition: self.definition,
            capabilities: self.capabilities,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolDisplayMetadata {
    pub qualified_name: String,
    pub module_path: String,
    pub has_doc: bool,
    pub has_signature: bool,
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
        via_import: bool,
        via_auto_import: bool,
        shadowed_auto_import: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticIndex {
    symbols: Vec<CompletionSymbol>,
    symbol_semantic_infos: Vec<SymbolSemanticInfo>,
}

impl SemanticIndex {
    pub fn from_symbol_semantic_infos(infos: Vec<SymbolSemanticInfo>) -> Self {
        let mut infos = infos;
        merge_duplicate_symbol_semantic_infos(&mut infos);
        infos.sort_by(|left, right| {
            left.surface_name
                .cmp(&right.surface_name)
                .then_with(|| {
                    completion_kind_rank(&left.kind).cmp(&completion_kind_rank(&right.kind))
                })
                .then_with(|| left.replacement.cmp(&right.replacement))
        });
        let symbols = infos
            .iter()
            .cloned()
            .map(SymbolSemanticInfo::into_completion_symbol)
            .collect();
        Self {
            symbols,
            symbol_semantic_infos: infos,
        }
    }

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
                merge_symbol_capabilities(&mut existing.capabilities, symbol.capabilities);
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
        let symbol_semantic_infos = deduped
            .iter()
            .map(SymbolSemanticInfo::from_completion_symbol)
            .collect();
        Self {
            symbols: deduped,
            symbol_semantic_infos,
        }
    }

    pub fn from_metadata(docs: &[DocEntry], signatures: &[SignatureEntry]) -> Self {
        Self::from_symbol_semantic_infos(symbol_semantic_infos_from_metadata(docs, signatures))
    }

    pub fn from_compile_metadata(
        declarations: &DeclarationIndex,
        docs: &[DocEntry],
        signatures: &[SignatureEntry],
    ) -> Self {
        Self::from_symbol_semantic_infos(symbol_semantic_infos_from_compile_metadata(
            declarations,
            docs,
            signatures,
        ))
    }

    pub fn enrich_symbols_with_compile_metadata(
        symbols: Vec<CompletionSymbol>,
        declarations: &DeclarationIndex,
        docs: &[DocEntry],
        signatures: &[SignatureEntry],
    ) -> Self {
        let metadata = Self::from_compile_metadata(declarations, docs, signatures);
        let mut metadata_by_key = BTreeMap::new();
        for info in metadata.symbol_semantic_infos {
            metadata_by_key.insert(
                (info.surface_name.clone(), completion_kind_rank(&info.kind)),
                info,
            );
        }

        let mut enriched = Vec::with_capacity(symbols.len());
        for symbol in symbols {
            let mut info = SymbolSemanticInfo::from_completion_symbol(&symbol);
            if let Some(metadata_info) =
                metadata_by_key.get(&(symbol.label.clone(), completion_kind_rank(&symbol.kind)))
            {
                if info.identity.is_none() {
                    info.identity = metadata_info.identity;
                }
                if info.detail.is_none() {
                    info.detail = metadata_info.detail.clone();
                }
                if info.documentation.is_none() {
                    info.documentation = metadata_info.documentation.clone();
                }
                if info.sort_text.is_none() {
                    info.sort_text = metadata_info.sort_text.clone();
                }
                if info.origin.is_none() {
                    info.origin = metadata_info.origin.clone();
                }
                if info.definition.is_none() {
                    info.definition = metadata_info.definition.clone();
                }
                if info.capabilities.is_none() {
                    info.capabilities = metadata_info.capabilities.clone();
                }
                merge_symbol_display_metadata(
                    &mut info.display_metadata,
                    metadata_info.display_metadata.clone(),
                );
            }
            enriched.push(info);
        }

        Self::from_symbol_semantic_infos(enriched)
    }

    pub fn from_declaration_index(declarations: &DeclarationIndex) -> Self {
        Self::from_symbol_semantic_infos(symbol_semantic_infos_from_declaration_index(
            declarations,
        ))
    }

    pub fn symbols(&self) -> &[CompletionSymbol] {
        &self.symbols
    }

    pub fn symbol_semantic_infos(&self) -> &[SymbolSemanticInfo] {
        &self.symbol_semantic_infos
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
        let incoming_info = SymbolSemanticInfo::from_completion_symbol(&symbol);
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
            merge_symbol_capabilities(&mut existing.capabilities, symbol.capabilities);
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
        self.symbol_semantic_infos.push(incoming_info);
        merge_duplicate_symbol_semantic_infos(&mut self.symbol_semantic_infos);
        self.symbol_semantic_infos.sort_by(|left, right| {
            left.surface_name
                .cmp(&right.surface_name)
                .then_with(|| {
                    completion_kind_rank(&left.kind).cmp(&completion_kind_rank(&right.kind))
                })
                .then_with(|| left.replacement.cmp(&right.replacement))
        });
    }
}

fn merge_duplicate_symbol_semantic_infos(infos: &mut Vec<SymbolSemanticInfo>) {
    let mut deduped = Vec::new();
    for info in std::mem::take(infos) {
        if let Some(existing) = deduped.iter_mut().find(|existing: &&mut SymbolSemanticInfo| {
            existing.surface_name == info.surface_name && existing.kind == info.kind
        }) {
            if existing.identity.is_none() {
                existing.identity = info.identity;
            }
            if existing.detail.is_none() {
                existing.detail = info.detail;
            }
            if existing.documentation.is_none() {
                existing.documentation = info.documentation;
            }
            if existing.sort_text.is_none() {
                existing.sort_text = info.sort_text;
            }
            if existing.origin.is_none() {
                existing.origin = info.origin;
            }
            if existing.definition.is_none() {
                existing.definition = info.definition;
            }
            merge_symbol_capabilities(&mut existing.capabilities, info.capabilities);
            merge_symbol_display_metadata(&mut existing.display_metadata, info.display_metadata);
            continue;
        }
        deduped.push(info);
    }
    *infos = deduped;
}

pub fn symbol_semantic_infos_from_metadata(
    docs: &[DocEntry],
    signatures: &[SignatureEntry],
) -> Vec<SymbolSemanticInfo> {
    let signature_by_name = signatures
        .iter()
        .map(|entry| (entry.qualified_name.as_str(), entry.signature.as_str()))
        .collect::<BTreeMap<_, _>>();

    let mut infos = Vec::new();
    for entry in signatures {
        let qualified_name = surface_name(&entry.qualified_name);
        infos.push(SymbolSemanticInfo {
            canonical_name: entry.qualified_name.clone(),
            surface_name: qualified_name.clone(),
            replacement: qualified_name,
            kind: completion_kind_for_doc_kind(&entry.kind),
            identity: symbol_identity_for_builtin_surface(&entry.qualified_name),
            detail: Some(entry.signature.clone()),
            documentation: None,
            sort_text: None,
            origin: Some(CompletionOrigin::Metadata {
                qualified_name: entry.qualified_name.clone(),
                module_path: entry.module_path.clone(),
            }),
            definition: None,
            capabilities: completion_capabilities_for_builtin(&entry.qualified_name),
            display_metadata: Some(SymbolDisplayMetadata {
                qualified_name: entry.qualified_name.clone(),
                module_path: entry.module_path.clone(),
                has_doc: false,
                has_signature: true,
            }),
        });
    }
    for entry in docs {
        let detail = signature_by_name
            .get(entry.qualified_name.as_str())
            .map(|signature| (*signature).to_string())
            .or_else(|| entry.signature.clone());
        let has_signature = detail.is_some();
        let qualified_name = surface_name(&entry.qualified_name);
        infos.push(SymbolSemanticInfo {
            canonical_name: entry.qualified_name.clone(),
            surface_name: qualified_name.clone(),
            replacement: qualified_name,
            kind: completion_kind_for_doc_kind(&entry.kind),
            identity: symbol_identity_for_builtin_surface(&entry.qualified_name),
            detail,
            documentation: Some(entry.doc.clone()),
            sort_text: None,
            origin: Some(CompletionOrigin::Metadata {
                qualified_name: entry.qualified_name.clone(),
                module_path: entry.module_path.clone(),
            }),
            definition: None,
            capabilities: completion_capabilities_for_builtin(&entry.qualified_name),
            display_metadata: Some(SymbolDisplayMetadata {
                qualified_name: entry.qualified_name.clone(),
                module_path: entry.module_path.clone(),
                has_doc: true,
                has_signature,
            }),
        });
    }
    infos
}

pub fn symbol_semantic_infos_from_declaration_index(
    declarations: &DeclarationIndex,
) -> Vec<SymbolSemanticInfo> {
    let mut infos = Vec::new();
    for entry in declarations
        .values()
        .filter(|entry| !entry.hidden && (entry.user_importable || entry.user_callable))
    {
        if !entry.module_path.is_empty() {
            let surface_module_name = surface_name(&entry.module_path);
            infos.push(SymbolSemanticInfo {
                canonical_name: entry.module_path.clone(),
                surface_name: surface_module_name.clone(),
                replacement: surface_module_name,
                kind: CompletionKind::TypePath,
                identity: Some(TypeIdentity::Mod),
                detail: None,
                documentation: None,
                sort_text: None,
                origin: None,
                definition: None,
                capabilities: completion_capabilities_for_builtin(&entry.module_path),
                display_metadata: None,
            });
        }

        if let Some(kind) = completion_kind_for_declaration_kind(&entry.kind) {
            let qualified_name = surface_name(&entry.fq_name);
            let capabilities = symbol_capabilities_for_declaration_entry(entry);
            let identity = symbol_identity_for_declaration_entry(entry);
            infos.push(SymbolSemanticInfo {
                canonical_name: entry.fq_name.clone(),
                surface_name: qualified_name.clone(),
                replacement: qualified_name,
                kind,
                identity,
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
                    via_import: false,
                    via_auto_import: false,
                    shadowed_auto_import: false,
                }),
                definition: None,
                capabilities,
                display_metadata: None,
            });
        }
    }
    infos
}

pub fn symbol_semantic_infos_from_compile_metadata(
    declarations: &DeclarationIndex,
    docs: &[DocEntry],
    signatures: &[SignatureEntry],
) -> Vec<SymbolSemanticInfo> {
    let mut infos = symbol_semantic_infos_from_declaration_index(declarations);
    merge_semantic_info(&mut infos, symbol_semantic_infos_from_metadata(docs, signatures));
    infos
}

fn merge_semantic_info(base: &mut Vec<SymbolSemanticInfo>, incoming: Vec<SymbolSemanticInfo>) {
    let mut base_by_key = base
        .iter()
        .enumerate()
        .map(|(idx, info)| (info.completion_key(), idx))
        .collect::<HashMap<_, _>>();
    for info in incoming {
        if let Some(existing_idx) = base_by_key.get(&info.completion_key()).copied() {
            let existing = &mut base[existing_idx];
            if existing.identity.is_none() {
                existing.identity = info.identity;
            }
            if existing.detail.is_none() {
                existing.detail = info.detail;
            }
            if existing.documentation.is_none() {
                existing.documentation = info.documentation;
            }
            if existing.sort_text.is_none() {
                existing.sort_text = info.sort_text;
            }
            if existing.origin.is_none() {
                existing.origin = info.origin;
            }
            if existing.definition.is_none() {
                existing.definition = info.definition;
            }
            merge_symbol_capabilities(&mut existing.capabilities, info.capabilities);
            merge_symbol_display_metadata(&mut existing.display_metadata, info.display_metadata);
        } else {
            let next_idx = base.len();
            base_by_key.insert(info.completion_key(), next_idx);
            base.push(info);
        }
    }
}

fn merge_symbol_capabilities(
    existing: &mut Option<SymbolCapabilities>,
    incoming: Option<SymbolCapabilities>,
) {
    let Some(incoming) = incoming else {
        return;
    };
    match existing.as_ref() {
        None => *existing = Some(incoming),
        Some(current)
            if current.facet_root_path.is_none() && incoming.facet_root_path.is_some() =>
        {
            *existing = Some(incoming);
        }
        Some(_) => {}
    }
}

fn merge_symbol_display_metadata(
    existing: &mut Option<SymbolDisplayMetadata>,
    incoming: Option<SymbolDisplayMetadata>,
) {
    match (existing.as_mut(), incoming) {
        (Some(existing), Some(incoming)) => {
            existing.has_doc |= incoming.has_doc;
            existing.has_signature |= incoming.has_signature;
        }
        (None, incoming) => *existing = incoming,
        _ => {}
    }
}

/// Return shared compile-space capabilities for builtin surface symbols.
pub fn symbol_capabilities_for_builtin_surface(name: &str) -> Option<SymbolCapabilities> {
    let surface_name = surface_name(name);
    builtin_symbol_identity_info(&surface_name).map(|info| info.capabilities)
}

pub fn symbol_identity_for_builtin_surface(name: &str) -> Option<TypeIdentity> {
    let surface_name = surface_name(name);
    builtin_symbol_identity_info(&surface_name).map(|info| info.identity)
}

pub(crate) fn completion_capabilities_for_builtin(name: &str) -> Option<SymbolCapabilities> {
    symbol_capabilities_for_builtin_surface(name)
}

pub fn symbol_identity_for_declaration_entry(
    entry: &sigil::DeclarationEntry,
) -> Option<TypeIdentity> {
    symbol_identity_for_builtin_surface(&entry.name)
        .or_else(|| symbol_identity_for_builtin_surface(&entry.fq_name))
        .or_else(|| {
            declaration_symbol_identity_info(&entry.name, &entry.kind).map(|info| info.identity)
        })
        .or(match entry.kind {
            DeclarationKind::Const => Some(TypeIdentity::Const),
            _ => None,
        })
}

pub fn facet_root_capabilities(kind: FacetRootKind) -> SymbolCapabilities {
    SymbolCapabilities::new(true, true, true, Some(kind))
}

pub fn facet_type_root_capabilities() -> SymbolCapabilities {
    facet_root_capabilities(FacetRootKind::TypeRoot)
}

pub fn symbol_capabilities_for_declaration_entry(
    entry: &sigil::DeclarationEntry,
) -> Option<SymbolCapabilities> {
    symbol_capabilities_for_builtin_surface(&entry.name)
        .or_else(|| symbol_capabilities_for_builtin_surface(&entry.fq_name))
        .or_else(|| {
            declaration_symbol_identity_info(&entry.name, &entry.kind)
                .map(|info| info.capabilities)
        })
}

pub fn symbol_semantic_info_for_effective_visible_entry(
    existing_infos: &[SymbolSemanticInfo],
    visible: &sigil::EffectiveVisibleEntry,
) -> Option<SymbolSemanticInfo> {
    let kind = match visible.entry.kind {
        DeclarationKind::BuiltinType => return None,
        _ => completion_kind_for_declaration_kind(&visible.entry.kind)?,
    };
    let qualified_label = surface_name(&visible.entry.fq_name);
    let mut detail = None;
    let mut documentation = None;
    let mut sort_text = None;
    let mut definition = None;
    let mut inherited_identity = None;
    let mut inherited_capabilities = None;
    let mut display_metadata = None;
    for info in existing_infos
        .iter()
        .filter(|info| info.surface_name == qualified_label && info.kind == kind)
    {
        if detail.is_none() {
            detail = info.detail.clone();
        }
        if documentation.is_none() {
            documentation = info.documentation.clone();
        }
        if sort_text.is_none() {
            sort_text = info.sort_text.clone();
        }
        if definition.is_none() {
            definition = info.definition.clone();
        }
        if inherited_identity.is_none() {
            inherited_identity = info.identity;
        }
        merge_symbol_capabilities(&mut inherited_capabilities, info.capabilities.clone());
        merge_symbol_display_metadata(&mut display_metadata, info.display_metadata.clone());
    }
    let identity = symbol_identity_for_declaration_entry(&visible.entry).or(inherited_identity);
    let capabilities = declaration_symbol_identity_info(&visible.entry.name, &visible.entry.kind)
        .map(|info| info.capabilities)
        .or(inherited_capabilities);
    Some(SymbolSemanticInfo {
        canonical_name: visible.entry.fq_name.clone(),
        surface_name: visible.visible_name.clone(),
        replacement: visible.visible_name.clone(),
        kind,
        identity,
        detail,
        documentation,
        sort_text,
        origin: Some(CompletionOrigin::Declaration {
            qualified_name: visible.entry.fq_name.clone(),
            module_path: visible.entry.module_path.clone(),
            name: visible.entry.name.clone(),
            stage_index: visible.entry.stage_index,
            auto_import: visible.entry.auto_import,
            visibility: visible.entry.visibility,
            user_importable: visible.entry.user_importable,
            user_callable: visible.entry.user_callable,
            via_import: visible.via_import,
            via_auto_import: visible.via_auto_import,
            shadowed_auto_import: visible.shadowed_auto_import,
        }),
        definition,
        capabilities,
        display_metadata,
    })
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
    pub capabilities: Option<SymbolCapabilities>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplInputSupport {
    pub candidates: Vec<CompletionCandidate>,
    pub replace_start: usize,
    pub replace_end: usize,
    pub signature: Option<InputSignatureHelp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSignatureHelp {
    pub lines: Vec<String>,
    pub active_parameter: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallableSignature {
    pub label: String,
    pub qualified_name: String,
    pub signature: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplInputSupportUpdate {
    pub symbols: Vec<CompletionSymbol>,
    pub callable_signatures: Vec<CallableSignature>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplInputSupportContext {
    index: SemanticIndex,
    callable_signatures: BTreeMap<String, (String, String)>,
}

impl ReplInputSupportContext {
    pub fn from_parts(
        index: SemanticIndex,
        callable_signatures: BTreeMap<String, (String, String)>,
    ) -> Self {
        Self {
            index,
            callable_signatures,
        }
    }

    pub fn from_update(update: ReplInputSupportUpdate) -> Self {
        let mut context = Self::default();
        context.apply_update(update);
        context
    }

    pub fn apply_update(&mut self, update: ReplInputSupportUpdate) {
        for symbol in update.symbols {
            self.index.upsert_symbol(symbol);
        }
        for signature in update.callable_signatures {
            self.insert_callable_signature(
                &signature.label,
                signature.qualified_name,
                signature.signature,
            );
        }
    }

    pub fn input_support(
        &self,
        input: &str,
        cursor: usize,
        scope: CompletionScope,
    ) -> ReplInputSupport {
        let cursor = clamp_to_char_boundary(input, cursor.min(input.len()));
        if !completion_allowed_at_cursor(input, cursor) {
            return ReplInputSupport::default();
        }

        let (replace_start, replace_end, prefix) = completion_token(input, cursor);
        let call_context = call_context_at_cursor(input, cursor);
        let operator_assist = if call_context.is_none() {
            spire::parse_operator_completion_context(input, cursor)
                .and_then(|context| self.operator_completion_assist(input, &context))
        } else {
            None
        };
        let signature = call_context
            .as_ref()
            .and_then(|context| self.signature_help_for_call(context))
            .or_else(|| {
                operator_assist
                    .as_ref()
                    .map(|assist| assist.signature.clone())
            });

        if call_context.is_none() && operator_assist.is_none() && prefix.is_empty() {
            return ReplInputSupport {
                candidates: Vec::new(),
                replace_start,
                replace_end,
                signature,
            };
        }

        let request = CompletionRequest {
            index: &self.index,
            source: input,
            cursor,
        };
        let completion = if let Some(context) = call_context.as_ref() {
            let expected_ty = self.expected_param_type_for_call(context);
            if let Some(completion) =
                complete_call_argument_with_presentation(request, CompletionPresentation::Repl)
            {
                completion
            } else {
                let mut completion = complete_repl_prefix(request, CompletionScope::VariablesOnly);
                completion.candidates = rank_completion_candidates_by_expected_type(
                    completion.candidates,
                    expected_ty.as_deref(),
                    parameter_type_accepts_arg_type,
                );
                if completion.candidates.is_empty() && expected_ty.is_none() && !prefix.is_empty() {
                    complete_repl_prefix(request, scope)
                } else {
                    completion
                }
            }
        } else if let Some(assist) = operator_assist.as_ref() {
            self.operator_completion_candidates(
                &prefix,
                replace_start,
                replace_end,
                assist.candidate_mode,
                assist.expected_type.as_deref(),
                assist.expected_callable_return_context.as_deref(),
            )
        } else {
            complete_repl_prefix(request, scope)
        };

        let mut candidates = completion.candidates;
        if call_context.is_none() && operator_assist.is_none() {
            self.inject_special_repl_candidates(
                &mut candidates,
                &prefix,
                replace_start,
                replace_end,
            );
        }

        ReplInputSupport {
            candidates,
            replace_start: completion.replace_start,
            replace_end: completion.replace_end,
            signature,
        }
    }

    pub fn should_request(input: &str, cursor: usize) -> bool {
        let cursor = clamp_to_char_boundary(input, cursor.min(input.len()));
        if !completion_allowed_at_cursor(input, cursor) {
            return false;
        }
        let (_, _, prefix) = completion_token(input, cursor);
        facet_path_context_at_cursor(input, cursor).is_some()
            || call_context_at_cursor(input, cursor).is_some()
            || spire::parse_operator_completion_context(input, cursor).is_some()
            || !prefix.is_empty()
    }

    fn insert_callable_signature(
        &mut self,
        label: &str,
        qualified_name: String,
        signature: String,
    ) {
        self.callable_signatures.insert(
            label.to_string(),
            (qualified_name.clone(), signature.clone()),
        );
        if let Some(tail) = label.rsplit("::").next() {
            self.callable_signatures
                .entry(tail.to_string())
                .or_insert((qualified_name, signature));
        }
    }

    fn signature_help_for_call(
        &self,
        context: &CompletionCallContext,
    ) -> Option<InputSignatureHelp> {
        let (qualified_name, signature) =
            self.display_signature_for_call_completion(&context.callee)?;
        let rendered = render_signature_with_qualified_name(&qualified_name, signature);
        Some(InputSignatureHelp {
            lines: vec![highlight_signature_parameter(
                &rendered,
                context.active_parameter,
            )],
            active_parameter: Some(context.active_parameter),
        })
    }

    fn operator_completion_assist(
        &self,
        input: &str,
        context: &spire::OperatorCompletionContext,
    ) -> Option<OperatorCompletionAssist> {
        let mut rendered = String::new();
        let mut current_ty = None;

        for (idx, stage) in context.stages.iter().enumerate() {
            let lhs_ty = if idx == 0 {
                self.infer_completion_operand_type(input, &stage.lhs)
            } else {
                current_ty.clone()
            };
            if idx == 0 {
                rendered.push_str(&Self::display_completion_ty(lhs_ty.as_ref()));
            }

            if let Some(rhs) = &stage.rhs {
                let (rhs_display, result_ty) = self.completed_operator_stage(
                    input,
                    stage.operator.as_str(),
                    lhs_ty.as_ref(),
                    rhs,
                );
                rendered.push_str(&format!(" {} {}", stage.operator, rhs_display));
                current_ty = result_ty;
                continue;
            }

            let expected = Self::active_operator_expected(stage.operator.as_str(), lhs_ty.as_ref());
            rendered.push_str(&format!(" {} [{}]", stage.operator, expected.display));
            return Some(OperatorCompletionAssist {
                signature: InputSignatureHelp {
                    lines: vec![rendered],
                    active_parameter: Some(0),
                },
                expected_type: expected.candidate_expected_type,
                expected_callable_return_context: expected.candidate_expected_return_context,
                candidate_mode: expected.candidate_mode,
            });
        }

        None
    }

    fn operator_completion_candidates(
        &self,
        prefix: &str,
        replace_start: usize,
        replace_end: usize,
        mode: OperatorCompletionCandidateMode,
        expected_type: Option<&str>,
        expected_callable_return_context: Option<&str>,
    ) -> CompletionResponse {
        let mut candidates = Vec::new();
        for symbol in self.index.symbols() {
            if !Self::operator_candidate_mode_accepts(mode, symbol) {
                continue;
            }
            let Some((label, replacement)) = Self::operator_completion_label(symbol, prefix) else {
                continue;
            };
            push_completion_candidate(
                &mut candidates,
                CompletionCandidate {
                    label,
                    replacement,
                    kind: symbol.kind.clone(),
                    detail: symbol.detail.clone(),
                    documentation: symbol.documentation.clone(),
                    sort_text: symbol.sort_text.clone(),
                    origin: symbol.origin.clone(),
                    capabilities: symbol.capabilities.clone(),
                    replace_start,
                    replace_end,
                },
            );
        }

        candidates.sort_by(|left, right| {
            let left_matches = self.operator_candidate_matches_expected(
                left,
                mode,
                expected_type,
                expected_callable_return_context,
            );
            let right_matches = self.operator_candidate_matches_expected(
                right,
                mode,
                expected_type,
                expected_callable_return_context,
            );
            right_matches
                .cmp(&left_matches)
                .then_with(|| {
                    completion_kind_rank(&left.kind).cmp(&completion_kind_rank(&right.kind))
                })
                .then_with(|| left.label.cmp(&right.label))
                .then_with(|| left.replacement.cmp(&right.replacement))
        });

        CompletionResponse {
            candidates,
            replace_start,
            replace_end,
        }
    }

    fn operator_candidate_mode_accepts(
        mode: OperatorCompletionCandidateMode,
        symbol: &CompletionSymbol,
    ) -> bool {
        match mode {
            OperatorCompletionCandidateMode::Variables => symbol.kind == CompletionKind::Variable,
            OperatorCompletionCandidateMode::Callables => {
                symbol.kind == CompletionKind::FunctionCall
                    || (symbol.kind == CompletionKind::Variable
                        && symbol
                            .detail
                            .as_deref()
                            .and_then(parse_signature_type)
                            .is_some_and(|ty| matches!(ty, AstTy::Func(_, _, _))))
            }
        }
    }

    fn operator_completion_label(
        symbol: &CompletionSymbol,
        prefix: &str,
    ) -> Option<(String, String)> {
        if prefix.is_empty() || symbol.label.starts_with(prefix) {
            return Some((symbol.label.clone(), symbol.replacement.clone()));
        }
        let tail = symbol.label.rsplit_once("::")?.1;
        if tail.starts_with(prefix) {
            return Some((tail.to_string(), tail.to_string()));
        }
        None
    }

    fn operator_candidate_matches_expected(
        &self,
        candidate: &CompletionCandidate,
        mode: OperatorCompletionCandidateMode,
        expected_type: Option<&str>,
        expected_callable_return_context: Option<&str>,
    ) -> bool {
        let Some(expected_type) = expected_type else {
            return false;
        };
        match mode {
            OperatorCompletionCandidateMode::Variables => candidate
                .detail
                .as_deref()
                .is_some_and(|actual| parameter_type_accepts_arg_type(expected_type, actual)),
            OperatorCompletionCandidateMode::Callables => self.callable_candidate_matches(
                candidate,
                expected_type,
                expected_callable_return_context,
            ),
        }
    }

    fn callable_candidate_matches(
        &self,
        candidate: &CompletionCandidate,
        expected_input: &str,
        expected_return_context: Option<&str>,
    ) -> bool {
        match candidate.kind {
            CompletionKind::FunctionCall => {
                let Some(signature) = candidate.detail.as_deref().or_else(|| {
                    self.callable_signature_for_completion(&candidate.label)
                        .map(|(_, signature)| signature.as_str())
                }) else {
                    return false;
                };
                let input_matches = signature_param_types(signature)
                    .and_then(|params| params.into_iter().next())
                    .is_some_and(|param| parameter_type_accepts_arg_type(&param, expected_input));
                if !input_matches {
                    return false;
                }
                expected_return_context.is_none_or(|context| {
                    signature_return_type(signature)
                        .and_then(parse_signature_type)
                        .is_some_and(|ret| Self::generic_context_name(&ret) == Some(context))
                })
            }
            CompletionKind::Variable => candidate
                .detail
                .as_deref()
                .and_then(parse_signature_type)
                .and_then(|ty| match ty {
                    AstTy::Func(_, params, ret) => Some((params, ret.as_ref().clone())),
                    _ => None,
                })
                .is_some_and(|(params, ret)| {
                    params.first().is_some_and(|param| {
                        Self::completion_param_accepts_expected_input(param, expected_input)
                    }) && expected_return_context
                        .is_none_or(|context| Self::generic_context_name(&ret) == Some(context))
                }),
            _ => false,
        }
    }

    fn completion_param_accepts_expected_input(param: &AstTy, expected_input: &str) -> bool {
        match param {
            AstTy::Named(_, name) if name == "Self" || name.starts_with('$') => true,
            _ => parameter_type_accepts_arg_type(&format_query_ty(param), expected_input),
        }
    }

    fn completed_operator_stage(
        &self,
        input: &str,
        operator: &str,
        lhs_ty: Option<&AstTy>,
        rhs: &spire::ast::Span,
    ) -> (String, Option<AstTy>) {
        if Self::is_function_operator(operator) {
            let rhs_source = source_slice_by_span(input, rhs).trim();
            let callable = self.completed_function_operator_stage(operator, lhs_ty, rhs_source);
            return match callable {
                Some((display, ret)) => (display, Some(ret)),
                None => ("(_ -> _)".to_string(), None),
            };
        }

        let rhs_ty = self.infer_completion_operand_type(input, rhs);
        let rhs_display = Self::display_completion_ty(rhs_ty.as_ref());
        let result_ty = Self::operator_result_type(operator, lhs_ty, rhs_ty.as_ref());
        (rhs_display, result_ty)
    }

    fn active_operator_expected(operator: &str, lhs_ty: Option<&AstTy>) -> ActiveOperatorExpected {
        if Self::is_function_operator(operator) {
            return Self::active_function_operator_expected(operator, lhs_ty);
        }

        let expected = if operator == "++" {
            "String".to_string()
        } else {
            Self::display_completion_ty(lhs_ty)
        };
        ActiveOperatorExpected {
            candidate_expected_type: (expected != "_").then_some(expected.clone()),
            candidate_expected_return_context: None,
            display: expected,
            candidate_mode: OperatorCompletionCandidateMode::Variables,
        }
    }

    fn active_function_operator_expected(
        operator: &str,
        lhs_ty: Option<&AstTy>,
    ) -> ActiveOperatorExpected {
        let (input_ty, return_context) = Self::function_operator_expected_parts(operator, lhs_ty);
        let input_display = Self::display_completion_ty(input_ty.as_ref());
        let return_display = return_context
            .as_ref()
            .map(|context| format!("{context}<_>"))
            .unwrap_or_else(|| "_".to_string());
        ActiveOperatorExpected {
            display: format!("({input_display} -> {return_display})"),
            candidate_expected_type: input_ty.as_ref().map(Self::display_ast_ty_for_completion),
            candidate_expected_return_context: return_context,
            candidate_mode: OperatorCompletionCandidateMode::Callables,
        }
    }

    fn function_operator_expected_parts(
        operator: &str,
        lhs_ty: Option<&AstTy>,
    ) -> (Option<AstTy>, Option<String>) {
        match operator {
            "|>" => (lhs_ty.cloned(), None),
            "|*>" => (
                lhs_ty
                    .and_then(Self::context_inner_type)
                    .map(|(_, inner)| inner.clone()),
                None,
            ),
            "|>=" => lhs_ty
                .and_then(Self::context_inner_type)
                .map(|(context, inner)| (Some(inner.clone()), Some(context.to_string())))
                .unwrap_or((None, None)),
            ">>" => lhs_ty
                .and_then(Self::unary_func_parts)
                .map(|(_, ret)| (Some(ret.clone()), None))
                .unwrap_or((None, None)),
            ">*" => lhs_ty
                .and_then(Self::unary_func_parts)
                .and_then(|(_, ret)| {
                    Self::context_inner_type(ret).map(|(_, inner)| (Some(inner.clone()), None))
                })
                .unwrap_or((None, None)),
            ">=>" => lhs_ty
                .and_then(Self::unary_func_parts)
                .and_then(|(_, ret)| {
                    Self::context_inner_type(ret)
                        .map(|(context, inner)| (Some(inner.clone()), Some(context.to_string())))
                })
                .unwrap_or((None, None)),
            _ => (None, None),
        }
    }

    fn operator_result_type(
        operator: &str,
        lhs_ty: Option<&AstTy>,
        rhs_ty: Option<&AstTy>,
    ) -> Option<AstTy> {
        match operator {
            "+" | "-" | "*" => lhs_ty.cloned().or_else(|| rhs_ty.cloned()),
            "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||" => Some(AstTy::Named(
                spire::ast::Span { start: 0, end: 0 },
                "Boolean".to_string(),
            )),
            "++" => Some(AstTy::Named(
                spire::ast::Span { start: 0, end: 0 },
                "String".to_string(),
            )),
            _ => None,
        }
    }

    fn is_function_operator(operator: &str) -> bool {
        matches!(operator, "|>" | "|*>" | "|>=" | ">>" | ">*" | ">=>")
    }

    fn infer_completion_operand_type(&self, input: &str, span: &spire::ast::Span) -> Option<AstTy> {
        let source = source_slice_by_span(input, span).trim();
        if source.is_empty() {
            return None;
        }
        if source.starts_with('"') && source.ends_with('"') && source.len() >= 2 {
            return Some(AstTy::Named(
                spire::ast::Span { start: 0, end: 0 },
                "String".to_string(),
            ));
        }
        if matches!(source, "True" | "False" | "true" | "false") {
            return Some(AstTy::Named(
                spire::ast::Span { start: 0, end: 0 },
                "Boolean".to_string(),
            ));
        }
        if source.parse::<i128>().is_ok() {
            return Some(AstTy::Named(
                spire::ast::Span { start: 0, end: 0 },
                "Int".to_string(),
            ));
        }
        if source.parse::<f64>().is_ok() && source.contains('.') {
            return Some(AstTy::Named(
                spire::ast::Span { start: 0, end: 0 },
                "Float".to_string(),
            ));
        }
        self.index
            .find_symbol(source)
            .and_then(|symbol| symbol.detail.as_deref())
            .and_then(parse_signature_type)
    }

    fn completed_function_operator_stage(
        &self,
        operator: &str,
        lhs_ty: Option<&AstTy>,
        symbol: &str,
    ) -> Option<(String, AstTy)> {
        let (_qualified_name, signature) = self.callable_signature_for_completion(symbol)?;
        let (params, ret) = signature_param_asts_and_return(signature)?;
        let first_param = params.first();
        let (expected_input, expected_return_context) =
            Self::function_operator_expected_parts(operator, lhs_ty);
        let display_input = expected_input
            .as_ref()
            .map(Self::display_ast_ty_for_completion)
            .or_else(|| first_param.map(Self::display_ast_ty_unknown_generics))
            .unwrap_or_else(|| "_".to_string());
        let ret = Self::specialize_callable_return(signature, expected_input.as_ref())
            .unwrap_or_else(|| Self::unknown_generics_to_hole(&ret));
        let display_ret = Self::display_ast_ty_for_completion(&ret);
        let display = format!("({display_input} -> {display_ret})");
        let result_ty =
            Self::function_operator_result_type(operator, lhs_ty, expected_return_context, ret)?;
        Some((display, result_ty))
    }

    fn function_operator_result_type(
        operator: &str,
        lhs_ty: Option<&AstTy>,
        expected_return_context: Option<String>,
        rhs_ret: AstTy,
    ) -> Option<AstTy> {
        match operator {
            "|>" => Some(rhs_ret),
            "|*>" => lhs_ty
                .and_then(Self::context_inner_type)
                .map(|(context, _)| Self::context_ty(context, rhs_ret)),
            "|>=" => expected_return_context.as_deref().and_then(|context| {
                Self::generic_context_name(&rhs_ret)
                    .is_some_and(|name| name == context)
                    .then_some(rhs_ret)
            }),
            ">>" => lhs_ty
                .and_then(Self::unary_func_parts)
                .map(|(params, _)| Self::func_ty(params[0].clone(), rhs_ret)),
            ">*" => lhs_ty
                .and_then(Self::unary_func_parts)
                .and_then(|(params, ret)| {
                    Self::context_inner_type(ret).map(|(context, _)| {
                        Self::func_ty(params[0].clone(), Self::context_ty(context, rhs_ret))
                    })
                }),
            ">=>" => lhs_ty
                .and_then(Self::unary_func_parts)
                .and_then(|(params, _)| {
                    expected_return_context.as_deref().and_then(|context| {
                        Self::generic_context_name(&rhs_ret)
                            .is_some_and(|name| name == context)
                            .then_some(Self::func_ty(params[0].clone(), rhs_ret))
                    })
                }),
            _ => None,
        }
    }

    fn context_inner_type(ty: &AstTy) -> Option<(&str, &AstTy)> {
        match ty {
            AstTy::Generic(_, name, args)
                if matches!(name.as_str(), "Result" | "List") && !args.is_empty() =>
            {
                Some((name.as_str(), &args[0]))
            }
            _ => None,
        }
    }

    fn generic_context_name(ty: &AstTy) -> Option<&str> {
        match ty {
            AstTy::Generic(_, name, args)
                if matches!(name.as_str(), "Result" | "List") && !args.is_empty() =>
            {
                Some(name.as_str())
            }
            _ => None,
        }
    }

    fn context_ty(context: &str, inner: AstTy) -> AstTy {
        AstTy::Generic(
            spire::ast::Span { start: 0, end: 0 },
            context.to_string(),
            vec![inner],
        )
    }

    fn unary_func_parts(ty: &AstTy) -> Option<(&[AstTy], &AstTy)> {
        match ty {
            AstTy::Func(_, params, ret) if params.len() == 1 => Some((params.as_slice(), ret)),
            _ => None,
        }
    }

    fn func_ty(input: AstTy, ret: AstTy) -> AstTy {
        AstTy::Func(
            spire::ast::Span { start: 0, end: 0 },
            vec![input],
            Box::new(ret),
        )
    }

    fn specialize_callable_return(signature: &str, input_ty: Option<&AstTy>) -> Option<AstTy> {
        let input_ty = input_ty?;
        let (params, ret) = signature_param_asts_and_return(signature)?;
        let first_param = params.first()?;
        let substitutions = build_type_substitutions(
            std::slice::from_ref(first_param),
            std::slice::from_ref(input_ty),
            None,
        )?;
        Some(substitute_query_ty(&ret, &substitutions, None))
    }

    fn display_completion_ty(ty: Option<&AstTy>) -> String {
        ty.map(Self::display_ast_ty_for_completion)
            .unwrap_or_else(|| "_".to_string())
    }

    fn display_ast_ty_for_completion(ty: &AstTy) -> String {
        format_query_ty(&Self::unknown_generics_to_hole(ty))
    }

    fn display_ast_ty_unknown_generics(ty: &AstTy) -> String {
        format_query_ty(&Self::unknown_generics_to_hole(ty))
    }

    fn unknown_generics_to_hole(ty: &AstTy) -> AstTy {
        match ty {
            AstTy::Named(_, name) if name == "Self" || name.starts_with('$') => {
                AstTy::Named(spire::ast::Span { start: 0, end: 0 }, "_".to_string())
            }
            AstTy::Named(_, _) | AstTy::ImplTrait(_, _) => ty.clone(),
            AstTy::Generic(span, name, args) if name == "Result" && args.len() > 1 => {
                AstTy::Generic(
                    span.clone(),
                    name.clone(),
                    vec![Self::unknown_generics_to_hole(&args[0])],
                )
            }
            AstTy::Generic(span, name, args) => AstTy::Generic(
                span.clone(),
                name.clone(),
                args.iter().map(Self::unknown_generics_to_hole).collect(),
            ),
            AstTy::Tuple(span, items) => AstTy::Tuple(
                span.clone(),
                items.iter().map(Self::unknown_generics_to_hole).collect(),
            ),
            AstTy::Func(span, params, ret) => AstTy::Func(
                span.clone(),
                params.iter().map(Self::unknown_generics_to_hole).collect(),
                Box::new(Self::unknown_generics_to_hole(ret)),
            ),
        }
    }

    fn inject_special_repl_candidates(
        &self,
        candidates: &mut Vec<CompletionCandidate>,
        prefix: &str,
        replace_start: usize,
        replace_end: usize,
    ) {
        for (label, replacement) in [("true", "True"), ("false", "False")] {
            if !label.starts_with(prefix) {
                continue;
            }
            if candidates
                .iter()
                .any(|candidate| candidate.label == label && candidate.replacement == label)
            {
                continue;
            }
            if candidates
                .iter()
                .any(|candidate| candidate.label == label && candidate.replacement == replacement)
            {
                continue;
            }
            candidates.push(CompletionCandidate {
                label: label.to_string(),
                replacement: replacement.to_string(),
                kind: CompletionKind::FunctionCall,
                detail: self.special_repl_candidate_detail(replacement),
                documentation: None,
                sort_text: None,
                origin: None,
                capabilities: None,
                replace_start,
                replace_end,
            });
        }
        candidates.sort_by(|left, right| {
            repl_completion_kind_rank(&left.kind)
                .cmp(&repl_completion_kind_rank(&right.kind))
                .then_with(|| left.label.cmp(&right.label))
                .then_with(|| left.replacement.cmp(&right.replacement))
        });
    }

    fn special_repl_candidate_detail(&self, replacement: &str) -> Option<String> {
        self.callable_signatures
            .get(replacement)
            .map(|(qualified_name, signature)| {
                render_signature_with_qualified_name(qualified_name, signature.clone())
            })
    }

    fn expected_param_type_for_call(&self, context: &CompletionCallContext) -> Option<String> {
        let (_qualified_name, signature) = self.signature_for_call_completion(&context.callee)?;
        let types = signature_param_types(&signature)?;
        types.get(context.active_parameter).cloned()
    }

    fn signature_for_call_completion(&self, symbol: &str) -> Option<(String, String)> {
        self.callable_signature_for_completion(symbol)
            .map(|(qualified_name, signature)| (qualified_name.clone(), signature.clone()))
            .or_else(|| {
                let found = if symbol.contains("::") {
                    self.index
                        .symbols()
                        .iter()
                        .find(|candidate| candidate.label == symbol)
                } else {
                    self.index.find_symbol(symbol)
                };
                found.and_then(|symbol| {
                    if symbol.kind == CompletionKind::Variable {
                        return None;
                    }
                    symbol
                        .detail
                        .as_ref()
                        .map(|signature| (symbol.label.clone(), signature.clone()))
                })
            })
    }

    fn display_signature_for_call_completion(&self, symbol: &str) -> Option<(String, String)> {
        let found = if symbol.contains("::") {
            self.index
                .symbols()
                .iter()
                .find(|candidate| candidate.label == symbol)
        } else {
            self.index.find_symbol(symbol)
        };
        found
            .and_then(|symbol| {
                if symbol.kind != CompletionKind::FunctionCall {
                    return None;
                }
                symbol
                    .detail
                    .as_ref()
                    .map(|signature| (symbol.label.clone(), signature.clone()))
            })
            .or_else(|| self.signature_for_call_completion(symbol))
    }

    fn callable_signature_for_completion(&self, symbol: &str) -> Option<&(String, String)> {
        let direct = self
            .callable_signatures
            .get(symbol)
            .or_else(|| self.callable_signatures.get(&canonical_symbol(symbol)));
        if direct.is_some() || symbol.contains("::") {
            return direct;
        }
        symbol
            .rsplit("::")
            .next()
            .and_then(|tail| self.callable_signatures.get(tail))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OperatorCompletionAssist {
    signature: InputSignatureHelp,
    expected_type: Option<String>,
    expected_callable_return_context: Option<String>,
    candidate_mode: OperatorCompletionCandidateMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperatorCompletionCandidateMode {
    Variables,
    Callables,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveOperatorExpected {
    display: String,
    candidate_expected_type: Option<String>,
    candidate_expected_return_context: Option<String>,
    candidate_mode: OperatorCompletionCandidateMode,
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

pub fn complete_call_argument(request: CompletionRequest<'_>) -> Option<CompletionResponse> {
    complete_call_argument_with_presentation(request, CompletionPresentation::Full)
}

fn complete_call_argument_with_presentation(
    request: CompletionRequest<'_>,
    presentation: CompletionPresentation,
) -> Option<CompletionResponse> {
    let signature = signature_help_at_cursor(request.index, request.source, request.cursor)?;
    let expected_ty_src =
        signature_expected_param_type(&signature.signature, signature.active_parameter)?;
    if let Some(expected_ty) = parse_signature_type(&expected_ty_src) {
        if let Some(completion) = complete_facet_path_arg(request, &expected_ty) {
            return Some(completion);
        }
    }

    if let Some(trait_name) =
        signature_trait_constraint_for_param(&signature.signature, signature.active_parameter)
    {
        let mut completion =
            complete_prefix_with_options(request, CompletionScope::VariablesOnly, presentation);
        if completion.candidates.is_empty() {
            let cursor = clamp_to_char_boundary(request.source, request.cursor);
            let (replace_start, replace_end, prefix) = completion_token(request.source, cursor);
            if prefix.is_empty() {
                completion = complete_variable_candidates_for_empty_prefix(
                    request.index,
                    replace_start,
                    replace_end,
                    presentation,
                );
            }
        }
        completion.candidates = rank_completion_candidates_by_trait_constraint(
            request.index,
            completion.candidates,
            &trait_name,
        );
        Some(completion)
    } else {
        let mut completion =
            complete_prefix_with_options(request, CompletionScope::VariablesOnly, presentation);
        completion.candidates = rank_completion_candidates_by_expected_type(
            completion.candidates,
            Some(&expected_ty_src),
            parameter_type_accepts_arg_type,
        );
        Some(completion)
    }
}

pub fn complete_facet_path_arg(
    request: CompletionRequest<'_>,
    expected_ty: &AstTy,
) -> Option<CompletionResponse> {
    if !facet_path_arg_type(expected_ty) {
        return None;
    }

    let cursor = clamp_to_char_boundary(request.source, request.cursor);
    let (replace_start, replace_end, prefix) = completion_token(request.source, cursor);
    let mut candidates = Vec::new();
    for symbol in request.index.symbols() {
        if !facet_path_arg_candidate_matches_prefix(symbol, &prefix) {
            continue;
        }
        let Some(candidate) = facet_api_path_arg_candidate(symbol, replace_start, replace_end)
        else {
            continue;
        };
        push_completion_candidate(&mut candidates, candidate);
    }
    sort_completion_candidates(&mut candidates, CompletionPresentation::Full);
    Some(CompletionResponse {
        candidates,
        replace_start,
        replace_end,
    })
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
            capabilities: symbol.capabilities.clone(),
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

fn complete_variable_candidates_for_empty_prefix(
    index: &SemanticIndex,
    replace_start: usize,
    replace_end: usize,
    presentation: CompletionPresentation,
) -> CompletionResponse {
    let mut candidates = Vec::new();
    for symbol in index
        .symbols()
        .iter()
        .filter(|symbol| completion_scope_accepts(CompletionScope::VariablesOnly, symbol))
    {
        let mut candidate = completion_candidate_from_symbol(symbol, replace_start, replace_end);
        apply_completion_presentation(&mut candidate, presentation, "");
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
        merge_symbol_capabilities(&mut existing.capabilities, candidate.capabilities);
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
    let call = call_context_at_cursor(source, cursor)?;

    let symbol = index.find_symbol(&call.callee)?;
    let signature = symbol.detail.clone()?;
    Some(SignatureLookup {
        signature,
        active_parameter: call.active_parameter,
        callee_start: call.callee_start,
        callee_end: call.callee_end,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletionCallContext {
    callee: String,
    active_parameter: usize,
    callee_start: usize,
    callee_end: usize,
}

fn call_context_at_cursor(source: &str, cursor: usize) -> Option<CompletionCallContext> {
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

    Some(CompletionCallContext {
        callee: callee.to_string(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionLexState {
    Code,
    String { escaped: bool },
    Interpolation { brace_depth: usize },
    InterpolationString { brace_depth: usize, escaped: bool },
}

fn completion_allowed_at_cursor(input: &str, cursor: usize) -> bool {
    let cursor = clamp_to_char_boundary(input, cursor.min(input.len()));
    let before = &input[..cursor];
    let mut state = CompletionLexState::Code;
    let mut chars = before.char_indices().peekable();

    while let Some((_idx, ch)) = chars.next() {
        state = match state {
            CompletionLexState::Code => match ch {
                '"' => CompletionLexState::String { escaped: false },
                _ => CompletionLexState::Code,
            },
            CompletionLexState::String { escaped } => {
                if escaped {
                    CompletionLexState::String { escaped: false }
                } else if ch == '\\' {
                    CompletionLexState::String { escaped: true }
                } else if ch == '"' {
                    CompletionLexState::Code
                } else if ch == '#' && chars.peek().is_some_and(|(_, next)| *next == '{') {
                    chars.next();
                    CompletionLexState::Interpolation { brace_depth: 1 }
                } else {
                    CompletionLexState::String { escaped: false }
                }
            }
            CompletionLexState::Interpolation { brace_depth } => match ch {
                '"' => CompletionLexState::InterpolationString {
                    brace_depth,
                    escaped: false,
                },
                '{' => CompletionLexState::Interpolation {
                    brace_depth: brace_depth + 1,
                },
                '}' if brace_depth <= 1 => CompletionLexState::String { escaped: false },
                '}' => CompletionLexState::Interpolation {
                    brace_depth: brace_depth - 1,
                },
                _ => CompletionLexState::Interpolation { brace_depth },
            },
            CompletionLexState::InterpolationString {
                brace_depth,
                escaped,
            } => {
                if escaped {
                    CompletionLexState::InterpolationString {
                        brace_depth,
                        escaped: false,
                    }
                } else if ch == '\\' {
                    CompletionLexState::InterpolationString {
                        brace_depth,
                        escaped: true,
                    }
                } else if ch == '"' {
                    CompletionLexState::Interpolation { brace_depth }
                } else {
                    CompletionLexState::InterpolationString {
                        brace_depth,
                        escaped: false,
                    }
                }
            }
        };
    }

    matches!(
        state,
        CompletionLexState::Code | CompletionLexState::Interpolation { .. }
    )
}

fn render_signature_with_qualified_name(qualified_name: &str, signature: String) -> String {
    let qualified_name = sindr::names::surface_path_name(qualified_name);
    let signature = surface_name(&signature);
    if let Some((module, tail)) = qualified_name.rsplit_once("::") {
        if signature == tail
            || signature.starts_with(&format!("{tail}("))
            || signature.starts_with(&format!("{tail}<"))
        {
            return format!("{module}::{signature}");
        }
    }
    signature
}

fn signature_param_types(signature: &str) -> Option<Vec<String>> {
    signature
        .split_once('(')
        .and_then(|(_, rest)| rest.rsplit_once(')').map(|(params, _)| params))
        .map(|params| {
            split_top_level_commas(params)
                .into_iter()
                .map(|param| {
                    param
                        .split_once(':')
                        .map(|(_, ty)| ty)
                        .unwrap_or(param)
                        .trim()
                        .to_string()
                })
                .collect()
        })
}

fn signature_param_asts_and_return(signature: &str) -> Option<(Vec<AstTy>, AstTy)> {
    let params = signature_param_types(signature)?
        .into_iter()
        .filter_map(|ty| parse_signature_type(&ty))
        .collect::<Vec<_>>();
    let return_ty = signature_return_type(signature).and_then(parse_signature_type)?;
    Some((params, return_ty))
}

fn signature_return_type(signature: &str) -> Option<&str> {
    signature.rsplit_once("->").map(|(_, ret)| ret.trim())
}

fn highlight_signature_parameter(signature: &str, active_parameter: usize) -> String {
    let Some((head, rest)) = signature.split_once('(') else {
        return signature.to_string();
    };
    let Some((params_src, tail)) = rest.rsplit_once(')') else {
        return signature.to_string();
    };
    let mut params = split_top_level_commas(params_src)
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if let Some(param) = params.get_mut(active_parameter) {
        if let Some((name, ty)) = param.split_once(':') {
            *param = format!("{}: [{}]", name.trim(), ty.trim());
        } else {
            *param = format!("[{}]", param.trim());
        }
    }
    format!("{head}({}){tail}", params.join(", "))
}

fn source_slice_by_span<'a>(source: &'a str, span: &spire::ast::Span) -> &'a str {
    let start = char_to_byte(source, span.start);
    let end = char_to_byte(source, span.end);
    &source[start..end]
}

fn char_to_byte(source: &str, char_offset: usize) -> usize {
    source
        .char_indices()
        .nth(char_offset)
        .map(|(idx, _)| idx)
        .unwrap_or(source.len())
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0usize;
    let mut angle_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
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
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if paren_depth == 0
                && angle_depth == 0
                && bracket_depth == 0
                && brace_depth == 0 =>
            {
                parts.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    let tail = input[start..].trim();
    if !tail.is_empty() || !input.trim().is_empty() {
        parts.push(tail);
    }
    parts
}

fn canonical_symbol(symbol: &str) -> String {
    let trimmed = symbol.trim();
    match trimmed.rsplit_once("::") {
        Some((module, tail)) if module == "Global" => tail.to_string(),
        _ => trimmed.to_string(),
    }
}

fn build_type_substitutions(
    params: &[AstTy],
    args: &[AstTy],
    self_ty: Option<&AstTy>,
) -> Option<HashMap<String, AstTy>> {
    if params.len() != args.len() {
        return None;
    }
    let mut substitutions = HashMap::new();
    for (param, arg) in params.iter().zip(args) {
        if !unify_query_ty(param, arg, &mut substitutions, self_ty) {
            return None;
        }
    }
    Some(substitutions)
}

fn unify_query_ty(
    param: &AstTy,
    arg: &AstTy,
    substitutions: &mut HashMap<String, AstTy>,
    self_ty: Option<&AstTy>,
) -> bool {
    match param {
        AstTy::Named(_, name) if name == "Self" => self_ty.is_none_or(|ty| ty == arg),
        AstTy::Named(_, name) if name.starts_with('$') => {
            if let Some(existing) = substitutions.get(name) {
                existing == arg
            } else {
                substitutions.insert(name.clone(), arg.clone());
                true
            }
        }
        AstTy::Named(_, name) => matches!(arg, AstTy::Named(_, other) if other == name),
        AstTy::ImplTrait(_, name) => matches!(arg, AstTy::ImplTrait(_, other) if other == name),
        AstTy::Generic(_, name, params) if name == "TypeRef" && params.len() == 1 => {
            unify_query_ty(&params[0], arg, substitutions, self_ty)
        }
        AstTy::Generic(_, name, params) => match arg {
            AstTy::Generic(_, other, args) if name == other && params.len() == args.len() => params
                .iter()
                .zip(args)
                .all(|(param, arg)| unify_query_ty(param, arg, substitutions, self_ty)),
            _ => false,
        },
        AstTy::Tuple(_, items) => match arg {
            AstTy::Tuple(_, other) if items.len() == other.len() => items
                .iter()
                .zip(other)
                .all(|(param, arg)| unify_query_ty(param, arg, substitutions, self_ty)),
            _ => false,
        },
        AstTy::Func(_, params, ret) => match arg {
            AstTy::Func(_, other_params, other_ret) if params.len() == other_params.len() => {
                params
                    .iter()
                    .zip(other_params)
                    .all(|(param, arg)| unify_query_ty(param, arg, substitutions, self_ty))
                    && unify_query_ty(ret, other_ret, substitutions, self_ty)
            }
            _ => false,
        },
    }
}

fn substitute_query_ty(
    ty: &AstTy,
    substitutions: &HashMap<String, AstTy>,
    self_ty: Option<&AstTy>,
) -> AstTy {
    match ty {
        AstTy::Named(_, name) if name == "Self" => self_ty.cloned().unwrap_or_else(|| ty.clone()),
        AstTy::Named(_, name) if name.starts_with('$') => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| ty.clone()),
        AstTy::Named(_, _) | AstTy::ImplTrait(_, _) => ty.clone(),
        AstTy::Generic(span, name, args) => AstTy::Generic(
            span.clone(),
            name.clone(),
            args.iter()
                .map(|arg| substitute_query_ty(arg, substitutions, self_ty))
                .collect(),
        ),
        AstTy::Tuple(span, items) => AstTy::Tuple(
            span.clone(),
            items
                .iter()
                .map(|item| substitute_query_ty(item, substitutions, self_ty))
                .collect(),
        ),
        AstTy::Func(span, params, ret) => AstTy::Func(
            span.clone(),
            params
                .iter()
                .map(|param| substitute_query_ty(param, substitutions, self_ty))
                .collect(),
            Box::new(substitute_query_ty(ret, substitutions, self_ty)),
        ),
    }
}

fn signature_expected_param_type(signature: &str, active_parameter: usize) -> Option<String> {
    signature_param_types(signature)?
        .get(active_parameter)
        .cloned()
}

fn signature_trait_constraint_for_param(
    signature: &str,
    active_parameter: usize,
) -> Option<String> {
    let expected = signature_expected_param_type(signature, active_parameter)?;
    let expected = expected.trim();
    if let Some(trait_name) = signature_type_param_bound(signature, expected) {
        return Some(trait_name);
    }
    if expected == "Self" {
        return signature
            .split_once("::")
            .map(|(trait_name, _)| trait_name.trim().to_string())
            .filter(|trait_name| !trait_name.is_empty());
    }
    None
}

fn signature_type_param_bound(signature: &str, param_name: &str) -> Option<String> {
    if !param_name.starts_with('$') {
        return None;
    }
    let params_start = signature.find('<')?;
    let params_end = signature[params_start + 1..].find('>')? + params_start + 1;
    split_top_level_commas(&signature[params_start + 1..params_end])
        .into_iter()
        .find_map(|param| {
            let (name, bound) = param.split_once(':')?;
            (name.trim() == param_name)
                .then(|| bound.trim().to_string())
                .filter(|bound| !bound.is_empty())
        })
}

fn facet_path_arg_type(ty: &AstTy) -> bool {
    matches!(ty, AstTy::Generic(_, name, args) if name == "Facet" && args.len() == 2)
}

fn parameter_type_accepts_arg_type(param: &str, arg: &str) -> bool {
    if param == arg || param == "Self" || param.starts_with('$') {
        return true;
    }
    if param.starts_with("TypeRef<") && param.ends_with('>') {
        let inner = &param["TypeRef<".len()..param.len() - 1];
        return inner == arg || inner.starts_with('$');
    }
    false
}

fn rank_completion_candidates_by_trait_constraint(
    index: &SemanticIndex,
    candidates: Vec<CompletionCandidate>,
    trait_name: &str,
) -> Vec<CompletionCandidate> {
    let mut ranked = candidates
        .into_iter()
        .enumerate()
        .map(|(idx, candidate)| {
            let matches_constraint = candidate
                .detail
                .as_deref()
                .is_some_and(|detail| type_satisfies_trait_constraint(index, detail, trait_name));
            (idx, matches_constraint, candidate)
        })
        .collect::<Vec<_>>();

    ranked.sort_by(
        |(left_idx, left_matches, _), (right_idx, right_matches, _)| {
            right_matches
                .cmp(left_matches)
                .then_with(|| left_idx.cmp(right_idx))
        },
    );
    ranked
        .into_iter()
        .map(|(_, _, candidate)| candidate)
        .collect()
}

fn type_satisfies_trait_constraint(index: &SemanticIndex, ty: &str, trait_name: &str) -> bool {
    index.symbols().iter().any(|symbol| {
        symbol
            .detail
            .as_deref()
            .or(Some(symbol.label.as_str()))
            .and_then(|text| trait_impl_target(text, trait_name))
            .is_some_and(|target| target == ty)
    })
}

fn trait_impl_target<'a>(text: &'a str, trait_name: &str) -> Option<&'a str> {
    let text = text.trim();
    let rest = text.strip_prefix("impl ")?;
    let rest = rest.strip_prefix(trait_name)?;
    let rest = rest.strip_prefix(" for ")?;
    let target = rest
        .split_once("::")
        .map(|(target, _)| target)
        .unwrap_or(rest)
        .trim();
    (!target.is_empty()).then_some(target)
}

fn facet_api_path_arg_candidate(
    symbol: &CompletionSymbol,
    replace_start: usize,
    replace_end: usize,
) -> Option<CompletionCandidate> {
    match symbol.kind {
        CompletionKind::Variable if symbol.detail.as_deref().is_some_and(facet_binding_type) => {
            Some(completion_candidate_from_symbol(
                symbol,
                replace_start,
                replace_end,
            ))
        }
        CompletionKind::TypeConstructor if path_constructable_type_root(symbol) => Some(
            completion_candidate_from_symbol(symbol, replace_start, replace_end),
        ),
        _ => None,
    }
}

fn completion_candidate_from_symbol(
    symbol: &CompletionSymbol,
    replace_start: usize,
    replace_end: usize,
) -> CompletionCandidate {
    CompletionCandidate {
        label: symbol.label.clone(),
        replacement: symbol.replacement.clone(),
        kind: symbol.kind.clone(),
        detail: symbol.detail.clone(),
        documentation: symbol.documentation.clone(),
        sort_text: symbol.sort_text.clone(),
        origin: symbol.origin.clone(),
        capabilities: symbol.capabilities.clone(),
        replace_start,
        replace_end,
    }
}

fn facet_binding_type(detail: &str) -> bool {
    matches!(
        parse_signature_type(detail),
        Some(AstTy::Generic(_, name, args)) if name == "Facet" && args.len() == 2
    )
}

fn path_constructable_type_root(symbol: &CompletionSymbol) -> bool {
    symbol
        .capabilities
        .as_ref()
        .is_some_and(|capabilities| capabilities.facet_root_path.is_some())
}

fn facet_path_arg_candidate_matches_prefix(symbol: &CompletionSymbol, prefix: &str) -> bool {
    prefix.is_empty()
        || symbol.label.starts_with(prefix)
        || symbol
            .label
            .rsplit_once("::")
            .is_some_and(|(_, tail)| tail.starts_with(prefix))
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
