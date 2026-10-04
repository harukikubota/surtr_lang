use super::*;

/// Declaration and import information supplied by the source loader.
///
/// Sigil does not load the standard library itself. Every source entry point
/// receives the same stage-backed environment from its caller.
#[derive(Debug, Clone)]
pub struct ResolveEnvironment {
    pub(super) declaration_index: DeclarationIndex,
    pub(super) owner_registry: OwnerRegistry,
    pub(super) declaration_uids: HashMap<String, u32>,
    pub(super) declaration_uid_kinds: HashMap<u32, DeclarationKind>,
    pub(super) declaration_hidden_by_uid: HashMap<u32, bool>,
    pub(super) trait_constructor_slots: HashMap<u32, Vec<String>>,
    pub(super) global_scope: Scope,
    pub(super) auto_import_modules: Vec<AutoImportModule>,
    pub(super) stage_count: usize,
}

impl ResolveEnvironment {
    pub fn from_stages(module_stages: &[Vec<StagedModuleAst>]) -> Result<Self, ResolveError> {
        let precollected = precollect_declarations(module_stages)?;
        Ok(Self::from_precollected(module_stages, &precollected))
    }

    pub fn from_precollected(
        module_stages: &[Vec<StagedModuleAst>],
        precollected: &PrecollectedDeclarations,
    ) -> Self {
        let declaration_index = precollected.declaration_index.clone();
        let declaration_uids = assign_declaration_uids(&declaration_index);
        let declaration_uid_kinds = declaration_uid_kind_map(&declaration_index, &declaration_uids);
        let declaration_hidden_by_uid = declaration_index
            .iter()
            .filter_map(|(fq_name, entry)| {
                declaration_uids
                    .get(fq_name)
                    .map(|uid| (*uid, entry.hidden))
            })
            .collect();
        let trait_constructor_slots =
            collect_staged_trait_constructor_slots(module_stages, &declaration_uids);
        let global_scope = build_global_scope(&declaration_index, &declaration_uids);
        Self {
            declaration_index,
            owner_registry: precollected.owner_registry.clone(),
            declaration_uids,
            declaration_uid_kinds,
            declaration_hidden_by_uid,
            trait_constructor_slots,
            global_scope,
            auto_import_modules: auto_import_module_names(module_stages),
            stage_count: module_stages.len(),
        }
    }
}
