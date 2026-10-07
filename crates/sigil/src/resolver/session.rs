use super::imports::{apply_session_imports, ImportState};
use super::*;
use sindr::warning::PhaseOutput;

/// Names exposed by explicit imports in the last successfully resolved chunk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedImports {
    pub imported_symbols: Vec<String>,
    pub success_labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SigilCheckpoint {
    scope: Scope,
    declaration_entries: HashMap<String, DeclarationEntry>,
    declaration_uids: HashMap<String, u32>,
    declaration_uid_kinds: HashMap<u32, DeclarationKind>,
    declaration_hidden_by_uid: HashMap<u32, bool>,
    trait_constructor_slots: HashMap<u32, Vec<String>>,
    owner_registry: OwnerRegistry,
    import_state: ImportState,
    explicit_function_imports: Vec<ExplicitFunctionImport>,
    effective_auto_import_fq_names: Vec<String>,
    shadowed_auto_import_bindings: Vec<(String, u32)>,
    last_imports: ResolvedImports,
}

#[derive(Debug, Clone)]
pub struct SigilSession {
    scope: Scope,
    declaration_entries: HashMap<String, DeclarationEntry>,
    declaration_uids: HashMap<String, u32>,
    declaration_uid_kinds: HashMap<u32, DeclarationKind>,
    declaration_hidden_by_uid: HashMap<u32, bool>,
    trait_constructor_slots: HashMap<u32, Vec<String>>,
    owner_registry: OwnerRegistry,
    current_module_path: Option<String>,
    auto_import_modules: Vec<AutoImportModule>,
    current_stage_index: usize,
    import_state: ImportState,
    explicit_function_imports: Vec<ExplicitFunctionImport>,
    effective_auto_import_fq_names: Vec<String>,
    shadowed_auto_import_bindings: Vec<(String, u32)>,
    last_imports: ResolvedImports,
}

impl SigilSession {
    fn qualify_current_name(&self, name: &str) -> String {
        match &self.current_module_path {
            Some(module_path) => format!("{}::{}", module_path, name),
            None => name.to_string(),
        }
    }

    fn reject_duplicate_current_module_defs(&self, ast: &[Ast]) -> Result<(), ResolveError> {
        for stmt in ast {
            match stmt {
                Ast::Def(span, name, _, _, _, _, _, _)
                | Ast::ExtractorDef(span, name, _, _, _, _, _)
                | Ast::ConstDef(span, name, _, _, _) => {
                    let qualified_name = self.qualify_current_name(name);
                    if matches!(
                        (self.scope.lookup(name), self.declaration_uids.get(&qualified_name)),
                        (Some(existing_uid), Some(current_uid)) if existing_uid == *current_uid
                    ) {
                        return Err(ResolveError {
                            message: format!("Duplicate top-level definition: {}", name),
                            span: span.clone(),
                            diagnostic: crate::error::ResolveErrorDiagnostic {
                                reason: crate::error::ResolveErrorReason::Namespace,
                                subject: None,
                            },
                            related_labels: Vec::new(),
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn from_environment(
        environment: &ResolveEnvironment,
        current_module_path: Option<String>,
        resume_state: ResolveResumeState,
    ) -> Result<Self, ResolveError> {
        let build = build_module_scope_with_imports(
            &environment.global_scope,
            &environment.auto_import_modules,
            &environment.declaration_index,
            &environment.declaration_uids,
            &[],
            current_module_path.as_deref(),
            environment.stage_count,
        )?;
        let mut scope = build.scope;
        let declaration_end = environment
            .declaration_uids
            .values()
            .copied()
            .max()
            .map(|uid| uid + 1)
            .unwrap_or(scope.next_id());
        scope.advance_next_id_to(declaration_end.max(resume_state.next_local_id));
        Ok(Self {
            scope,
            declaration_entries: environment.declaration_index.clone().into_iter().collect(),
            declaration_uids: environment.declaration_uids.clone(),
            declaration_uid_kinds: environment.declaration_uid_kinds.clone(),
            declaration_hidden_by_uid: environment.declaration_hidden_by_uid.clone(),
            trait_constructor_slots: environment.trait_constructor_slots.clone(),
            owner_registry: environment.owner_registry.clone(),
            current_module_path,
            auto_import_modules: environment.auto_import_modules.clone(),
            current_stage_index: environment.stage_count,
            import_state: build.import_state,
            explicit_function_imports: Vec::new(),
            effective_auto_import_fq_names: build.effective_auto_import_fq_names,
            shadowed_auto_import_bindings: build.shadowed_auto_import_bindings,
            last_imports: ResolvedImports::default(),
        })
    }

    pub fn last_imports(&self) -> &ResolvedImports {
        &self.last_imports
    }

    pub fn resolve(&mut self, ast: Vec<Ast>) -> Result<Vec<Resolved>, ResolveError> {
        self.resolve_with_warnings(ast).map(|output| output.value)
    }

    pub fn resolve_with_warnings(
        &mut self,
        ast: Vec<Ast>,
    ) -> Result<PhaseOutput<Vec<Resolved>>, ResolveError> {
        self.reject_duplicate_current_module_defs(&ast)?;
        let owner_modules = ast
            .iter()
            .cloned()
            .flat_map(|stmt| {
                staged_modules_from_source_ast(vec![stmt], self.current_module_path.as_deref())
            })
            .collect::<Vec<_>>();
        let chunk_registry = precollect_owner_registry(&[owner_modules])?;
        let mut owner_registry = self.owner_registry.clone();
        owner_registry.merge(&chunk_registry)?;
        let declaration_index = self.declaration_entries.clone().into_iter().collect();
        let import_build = apply_session_imports(
            self.scope.clone(),
            &self.auto_import_modules,
            &declaration_index,
            &self.declaration_uids,
            &ast,
            self.current_stage_index,
            self.import_state.clone(),
            self.effective_auto_import_fq_names.clone(),
            self.shadowed_auto_import_bindings.clone(),
        )?;
        let mut resolver = Resolver::with_scope(import_build.scope);
        resolver.declaration_entries = Arc::new(self.declaration_entries.clone());
        resolver.declaration_uids = Arc::new(self.declaration_uids.clone());
        resolver.declaration_uid_kinds = Arc::new(self.declaration_uid_kinds.clone());
        resolver.declaration_hidden_by_uid = Arc::new(self.declaration_hidden_by_uid.clone());
        resolver.trait_constructor_slots = Arc::new(self.trait_constructor_slots.clone());
        resolver.owner_registry = Arc::new(owner_registry);
        resolver.current_module_path = self.current_module_path.clone();
        resolver.allow_top_level_shadowing = true;
        let resolved = resolver.resolve_program(import_build.program)?;
        let warnings = warnings::collect_resolution_warnings(
            &resolved,
            &import_build.explicit_function_imports,
        );
        self.declaration_uids = resolver.declaration_uids.as_ref().clone();
        self.declaration_entries = resolver.declaration_entries.as_ref().clone();
        self.declaration_uid_kinds = resolver.declaration_uid_kinds.as_ref().clone();
        self.declaration_hidden_by_uid = resolver.declaration_hidden_by_uid.as_ref().clone();
        self.trait_constructor_slots = resolver.trait_constructor_slots.as_ref().clone();
        self.owner_registry = resolver.owner_registry.as_ref().clone();
        self.scope = resolver.into_scope();
        self.import_state = import_build.import_state;
        self.explicit_function_imports
            .extend(import_build.explicit_function_imports);
        self.effective_auto_import_fq_names = import_build.effective_auto_import_fq_names;
        self.shadowed_auto_import_bindings = import_build.shadowed_auto_import_bindings;
        self.last_imports = import_build.import_result;
        Ok(PhaseOutput::new(resolved, warnings))
    }

    pub fn checkpoint(&self) -> SigilCheckpoint {
        SigilCheckpoint {
            scope: self.scope.clone(),
            declaration_entries: self.declaration_entries.clone(),
            declaration_uids: self.declaration_uids.clone(),
            declaration_uid_kinds: self.declaration_uid_kinds.clone(),
            declaration_hidden_by_uid: self.declaration_hidden_by_uid.clone(),
            trait_constructor_slots: self.trait_constructor_slots.clone(),
            owner_registry: self.owner_registry.clone(),
            import_state: self.import_state.clone(),
            explicit_function_imports: self.explicit_function_imports.clone(),
            effective_auto_import_fq_names: self.effective_auto_import_fq_names.clone(),
            shadowed_auto_import_bindings: self.shadowed_auto_import_bindings.clone(),
            last_imports: self.last_imports.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: SigilCheckpoint) {
        self.scope = checkpoint.scope;
        self.declaration_entries = checkpoint.declaration_entries;
        self.declaration_uids = checkpoint.declaration_uids;
        self.declaration_uid_kinds = checkpoint.declaration_uid_kinds;
        self.declaration_hidden_by_uid = checkpoint.declaration_hidden_by_uid;
        self.trait_constructor_slots = checkpoint.trait_constructor_slots;
        self.owner_registry = checkpoint.owner_registry;
        self.import_state = checkpoint.import_state;
        self.explicit_function_imports = checkpoint.explicit_function_imports;
        self.effective_auto_import_fq_names = checkpoint.effective_auto_import_fq_names;
        self.shadowed_auto_import_bindings = checkpoint.shadowed_auto_import_bindings;
        self.last_imports = checkpoint.last_imports;
    }

    pub fn owner_registry(&self) -> &OwnerRegistry {
        &self.owner_registry
    }

    pub fn lookup_uid(&self, name: &str) -> Option<u32> {
        self.scope.lookup(name)
    }

    pub fn define_with_id(&mut self, name: &str, id: u32) {
        self.scope.define_with_id(name, id);
    }

    pub fn visible_declaration_entries(&self) -> Vec<EffectiveVisibleEntry> {
        let entries_by_uid = self
            .declaration_uids
            .iter()
            .filter_map(|(fq_name, uid)| {
                self.declaration_entries
                    .get(fq_name)
                    .cloned()
                    .map(|entry| (*uid, entry))
            })
            .collect::<HashMap<_, _>>();
        collect_effective_visible_entries(
            &self.scope,
            &entries_by_uid,
            &self.explicit_function_imports,
            &self.effective_auto_import_fq_names,
            &self.shadowed_auto_import_bindings,
        )
    }
}
