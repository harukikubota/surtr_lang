use super::scope_init::initialize_scope;
use super::*;

#[derive(Debug, Clone)]
pub struct SigilCheckpoint {
    scope: Scope,
}

#[derive(Debug, Clone)]
pub struct SigilSession {
    scope: Scope,
    current_module_path: Option<String>,
}

impl SigilSession {
    pub fn new() -> Self {
        Self {
            scope: initialize_scope(),
            current_module_path: None,
        }
    }

    pub fn with_module_path(current_module_path: Option<String>) -> Self {
        Self {
            scope: initialize_scope(),
            current_module_path,
        }
    }

    pub fn resolve(&mut self, ast: Vec<Ast>) -> Result<Vec<Resolved>, ResolveError> {
        let mut resolver = Resolver::with_scope(self.scope.clone());
        resolver.current_module_path = self.current_module_path.clone();
        let resolved = resolver.resolve_program(ast)?;
        self.scope = resolver.into_scope();
        Ok(resolved)
    }

    pub fn checkpoint(&self) -> SigilCheckpoint {
        SigilCheckpoint {
            scope: self.scope.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: SigilCheckpoint) {
        self.scope = checkpoint.scope;
    }

    pub fn replace_scope(&mut self, scope: Scope) {
        self.scope = scope;
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
