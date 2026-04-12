use super::scope_init::initialize_scope;
use super::*;
use super::{assign_declaration_uids, declaration_uid_kind_map};

#[derive(Debug, Clone)]
pub struct SigilCheckpoint {
    scope: Scope,
    declaration_uids: HashMap<String, u32>,
    declaration_uid_kinds: HashMap<u32, DeclarationKind>,
}

#[derive(Debug, Clone)]
pub struct SigilSession {
    scope: Scope,
    declaration_uids: HashMap<String, u32>,
    declaration_uid_kinds: HashMap<u32, DeclarationKind>,
    current_module_path: Option<String>,
}

impl SigilSession {
    pub fn new() -> Self {
        Self {
            scope: initialize_scope(),
            declaration_uids: HashMap::new(),
            declaration_uid_kinds: HashMap::from([
                (0, DeclarationKind::ResultCtor),
                (1, DeclarationKind::ResultCtor),
            ]),
            current_module_path: None,
        }
    }

    pub fn with_module_path(current_module_path: Option<String>) -> Self {
        Self {
            scope: initialize_scope(),
            declaration_uids: HashMap::new(),
            declaration_uid_kinds: HashMap::from([
                (0, DeclarationKind::ResultCtor),
                (1, DeclarationKind::ResultCtor),
            ]),
            current_module_path,
        }
    }

    pub fn resolve(&mut self, ast: Vec<Ast>) -> Result<Vec<Resolved>, ResolveError> {
        let mut resolver = Resolver::with_scope(self.scope.clone());
        resolver.declaration_uids = self.declaration_uids.clone();
        resolver.declaration_uid_kinds = self.declaration_uid_kinds.clone();
        resolver.current_module_path = self.current_module_path.clone();
        let resolved = resolver.resolve_program(ast)?;
        self.declaration_uids = resolver.declaration_uids.clone();
        self.declaration_uid_kinds = resolver.declaration_uid_kinds.clone();
        self.scope = resolver.into_scope();
        Ok(resolved)
    }

    pub fn checkpoint(&self) -> SigilCheckpoint {
        SigilCheckpoint {
            scope: self.scope.clone(),
            declaration_uids: self.declaration_uids.clone(),
            declaration_uid_kinds: self.declaration_uid_kinds.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: SigilCheckpoint) {
        self.scope = checkpoint.scope;
        self.declaration_uids = checkpoint.declaration_uids;
        self.declaration_uid_kinds = checkpoint.declaration_uid_kinds;
    }

    pub fn replace_scope(&mut self, scope: Scope) {
        self.scope = scope;
    }

    pub fn replace_scope_with_declarations(
        &mut self,
        scope: Scope,
        declaration_index: &DeclarationIndex,
    ) {
        let declaration_uids = assign_declaration_uids(declaration_index);
        let declaration_uid_kinds = declaration_uid_kind_map(declaration_index, &declaration_uids);
        self.scope = scope;
        self.declaration_uids = declaration_uids;
        self.declaration_uid_kinds = declaration_uid_kinds;
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
