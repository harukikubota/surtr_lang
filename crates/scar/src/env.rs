use std::collections::{HashMap, HashSet};

use spire::ast::Symbol;

use crate::types::Ty;

/// Kind of user-defined type.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Struct,
    Record,
    Error,
}

/// Metadata for a user-defined type (struct, record, error).
#[derive(Debug, Clone)]
pub struct TypeDefInfo {
    pub tag: u32,
    pub kind: TypeKind,
    pub name: Symbol,
    pub fields: Vec<(Symbol, Ty)>,
}

/// Type environment — tracks variable types and type definitions.
#[derive(Debug, Clone)]
pub struct TypeEnv {
    /// unique_id → type
    pub vars: HashMap<u32, Ty>,
    /// type name → definition
    pub type_defs: HashMap<Symbol, TypeDefInfo>,
    /// Next tag to assign (0 = Ok, 1 = Err are reserved)
    pub next_tag: u32,
    /// Next function index for `def`.
    pub next_fun_idx: u32,
    /// Next fresh type variable id
    pub next_tyvar: u32,
    /// Declared `deferror` type names, available before full type registration
    pub error_type_names: HashSet<Symbol>,
    /// `deferror` constructor bindings by unique_id
    pub error_constructor_ids: HashSet<u32>,
}

impl TypeEnv {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
            type_defs: HashMap::new(),
            next_tag: 2, // 0 = Ok, 1 = Err
            next_fun_idx: 0,
            next_tyvar: 0,
            error_type_names: HashSet::new(),
            error_constructor_ids: HashSet::new(),
        }
    }

    /// Bind a variable (by unique_id) to a type.
    pub fn bind_var(&mut self, unique_id: u32, ty: Ty) {
        self.vars.insert(unique_id, ty);
    }

    /// Look up the type of a variable.
    pub fn lookup_var(&self, unique_id: u32) -> Option<&Ty> {
        self.vars.get(&unique_id)
    }

    /// Register a type definition, assigning the next tag.
    pub fn register_type_def(
        &mut self,
        name: Symbol,
        kind: TypeKind,
        fields: Vec<(Symbol, Ty)>,
    ) -> u32 {
        let tag = self.next_tag;
        self.next_tag += 1;
        self.type_defs.insert(
            name.clone(),
            TypeDefInfo {
                tag,
                kind,
                name,
                fields,
            },
        );
        tag
    }

    /// Look up a type definition by name.
    pub fn lookup_type_def(&self, name: &str) -> Option<&TypeDefInfo> {
        self.type_defs.get(name)
    }

    /// Generate a fresh type variable.
    pub fn fresh_tyvar(&mut self) -> Ty {
        let id = self.next_tyvar;
        self.next_tyvar += 1;
        Ty::Var(id)
    }

    pub fn register_error_constructor(&mut self, unique_id: u32) {
        self.error_constructor_ids.insert(unique_id);
    }

    pub fn is_error_constructor(&self, unique_id: u32) -> bool {
        self.error_constructor_ids.contains(&unique_id)
    }

    pub fn declare_error_type_name(&mut self, name: Symbol) {
        self.error_type_names.insert(name);
    }

    pub fn is_declared_error_type_name(&self, name: &str) -> bool {
        self.error_type_names.contains(name)
    }
}
