use super::scope_init::initialize_scope;
use super::*;
use super::{assign_declaration_uids, declaration_uid_kind_map};

#[derive(Debug, Clone)]
pub struct SigilCheckpoint {
    scope: Scope,
    declaration_entries: HashMap<String, DeclarationEntry>,
    declaration_uids: HashMap<String, u32>,
    declaration_uid_kinds: HashMap<u32, DeclarationKind>,
    declaration_hidden_by_uid: HashMap<u32, bool>,
}

#[derive(Debug, Clone)]
pub struct SigilSession {
    scope: Scope,
    declaration_entries: HashMap<String, DeclarationEntry>,
    declaration_uids: HashMap<String, u32>,
    declaration_uid_kinds: HashMap<u32, DeclarationKind>,
    declaration_hidden_by_uid: HashMap<u32, bool>,
    current_module_path: Option<String>,
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
                Ast::Def(span, name, _, _, _, _, _)
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
                            related_labels: Vec::new(),
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn new() -> Self {
        Self::with_module_path(None)
    }

    pub fn with_module_path(current_module_path: Option<String>) -> Self {
        Self {
            scope: initialize_scope(),
            declaration_entries: HashMap::new(),
            declaration_uids: HashMap::new(),
            declaration_uid_kinds: HashMap::from([
                (0, DeclarationKind::ResultCtor),
                (1, DeclarationKind::ResultCtor),
            ]),
            declaration_hidden_by_uid: HashMap::new(),
            current_module_path,
        }
    }

    pub fn resolve(&mut self, ast: Vec<Ast>) -> Result<Vec<Resolved>, ResolveError> {
        self.reject_duplicate_current_module_defs(&ast)?;
        let mut resolver = Resolver::with_scope(self.scope.clone());
        resolver.declaration_entries = self.declaration_entries.clone();
        resolver.declaration_uids = self.declaration_uids.clone();
        resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
        resolver.declaration_hidden_by_uid = self.declaration_hidden_by_uid.clone();
        resolver.current_module_path = self.current_module_path.clone();
        resolver.allow_top_level_shadowing = true;
        let resolved = resolver.resolve_program(ast)?;
        self.declaration_uids = resolver.declaration_uids.clone();
        self.declaration_entries = resolver.declaration_entries.clone();
        self.declaration_uid_kinds = resolver.declaration_uid_kinds.clone();
        self.declaration_hidden_by_uid = resolver.declaration_hidden_by_uid.clone();
        self.scope = resolver.into_scope();
        Ok(resolved)
    }

    pub fn checkpoint(&self) -> SigilCheckpoint {
        SigilCheckpoint {
            scope: self.scope.clone(),
            declaration_entries: self.declaration_entries.clone(),
            declaration_uids: self.declaration_uids.clone(),
            declaration_uid_kinds: self.declaration_uid_kinds.clone(),
            declaration_hidden_by_uid: self.declaration_hidden_by_uid.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: SigilCheckpoint) {
        self.scope = checkpoint.scope;
        self.declaration_entries = checkpoint.declaration_entries;
        self.declaration_uids = checkpoint.declaration_uids;
        self.declaration_uid_kinds = checkpoint.declaration_uid_kinds;
        self.declaration_hidden_by_uid = checkpoint.declaration_hidden_by_uid;
    }

    pub fn replace_scope(&mut self, scope: Scope) {
        self.scope = scope;
    }

    pub fn replace_scope_with_declarations(
        &mut self,
        mut scope: Scope,
        declaration_index: &DeclarationIndex,
    ) {
        let declaration_uids = assign_declaration_uids(declaration_index);
        let next_local_id = declaration_uids
            .values()
            .copied()
            .max()
            .map(|uid| uid.saturating_add(1))
            .unwrap_or_else(|| scope.next_id());
        scope.advance_next_id_to(next_local_id);
        self.declaration_entries = declaration_index.clone().into_iter().collect();
        let declaration_uid_kinds = declaration_uid_kind_map(declaration_index, &declaration_uids);
        let declaration_hidden_by_uid = declaration_index
            .iter()
            .filter_map(|(fq_name, entry)| {
                declaration_uids
                    .get(fq_name)
                    .copied()
                    .map(|uid| (uid, entry.hidden))
            })
            .collect::<HashMap<_, _>>();
        self.scope = scope;
        self.declaration_uids = declaration_uids;
        self.declaration_uid_kinds = declaration_uid_kinds;
        self.declaration_hidden_by_uid = declaration_hidden_by_uid;
    }

    pub fn lookup_uid(&self, name: &str) -> Option<u32> {
        self.scope.lookup(name)
    }

    pub fn define_with_id(&mut self, name: &str, id: u32) {
        self.scope.define_with_id(name, id);
    }
}

impl Default for SigilSession {
    fn default() -> Self {
        Self::new()
    }
}
