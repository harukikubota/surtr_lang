use std::collections::{HashMap, HashSet};

use sindr::primitives::SurtrInt;
use spire::ast::Symbol;

use crate::types::Ty;

/// Kind of user-defined type.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Struct,
    Record,
    Error,
    Enum,
}

/// Metadata for a user-defined type (struct, record, error).
#[derive(Debug, Clone)]
pub struct TypeDefInfo {
    pub tag: u32,
    pub kind: TypeKind,
    pub name: Symbol,
    pub type_params: Vec<Symbol>,
    pub fields: Vec<(Symbol, Ty)>,
    pub state: TypeDefState,
}

/// Resolution state for user-defined type signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeDefState {
    /// Name/kind/tag are known, but field signature is not finalized yet.
    Declared,
    /// Full field signature is available.
    SignatureResolved,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariantInfo {
    pub constructor_name: Symbol,
    pub short_name: Symbol,
    pub enum_name: Symbol,
    pub enum_ty: Ty,
    pub tag: u32,
    pub payload: Vec<Ty>,
    pub discriminant: SurtrInt,
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
    /// enum constructor unique_id -> variant metadata
    pub enum_constructor_ids: HashMap<u32, EnumVariantInfo>,
    /// enum tag -> variant metadata
    pub enum_variant_tags: HashMap<u32, EnumVariantInfo>,
    /// enum type name -> variants
    pub enum_variants_by_enum: HashMap<Symbol, Vec<EnumVariantInfo>>,
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
            enum_constructor_ids: HashMap::new(),
            enum_variant_tags: HashMap::new(),
            enum_variants_by_enum: HashMap::new(),
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

    /// Predeclare a type definition and reserve a deterministic tag.
    ///
    /// Tags are assigned in declaration traversal order from the caller.
    /// Re-predeclaring the same type name reuses the already reserved tag.
    pub fn predeclare_type_def(
        &mut self,
        name: Symbol,
        kind: TypeKind,
        type_params: Vec<Symbol>,
    ) -> u32 {
        if let Some(existing) = self.type_defs.get(&name) {
            debug_assert!(
                existing.kind == kind,
                "Type predeclared with different kind: {}",
                name
            );
            debug_assert!(
                existing.type_params == type_params,
                "Type predeclared with different type params: {}",
                name
            );
            return existing.tag;
        }

        let tag = self.next_tag;
        self.next_tag += 1;
        self.type_defs.insert(
            name.clone(),
            TypeDefInfo {
                tag,
                kind,
                name,
                type_params,
                fields: Vec::new(),
                state: TypeDefState::Declared,
            },
        );
        tag
    }

    /// Finalize a predeclared type definition with its field signature.
    ///
    /// Returns `None` when the type name has not been predeclared.
    pub fn resolve_type_def_signature(
        &mut self,
        name: &str,
        fields: Vec<(Symbol, Ty)>,
    ) -> Option<u32> {
        let def = self.type_defs.get_mut(name)?;
        def.fields = fields;
        def.state = TypeDefState::SignatureResolved;
        Some(def.tag)
    }

    /// Reserve a fresh runtime tag.
    pub fn reserve_tag(&mut self) -> u32 {
        let tag = self.next_tag;
        self.next_tag += 1;
        tag
    }

    /// Register a type definition using the legacy single-step API.
    ///
    /// This keeps existing call-sites working while internally using the
    /// predeclare/resolve flow.
    pub fn register_type_def(
        &mut self,
        name: Symbol,
        kind: TypeKind,
        fields: Vec<(Symbol, Ty)>,
    ) -> u32 {
        let tag = self.predeclare_type_def(name.clone(), kind, Vec::new());
        let _ = self.resolve_type_def_signature(&name, fields);
        tag
    }

    /// Look up a type definition by name.
    pub fn lookup_type_def(&self, name: &str) -> Option<&TypeDefInfo> {
        self.type_defs.get(name)
    }

    pub fn is_type_signature_resolved(&self, name: &str) -> bool {
        self.type_defs
            .get(name)
            .is_some_and(|def| def.state == TypeDefState::SignatureResolved)
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

    pub fn register_enum_variant(
        &mut self,
        constructor_id: u32,
        variant: EnumVariantInfo,
    ) -> Result<(), String> {
        if self.enum_constructor_ids.contains_key(&constructor_id) {
            return Err(format!(
                "enum constructor id {} already registered",
                constructor_id
            ));
        }
        if self.enum_variant_tags.contains_key(&variant.tag) {
            return Err(format!("enum tag {} already registered", variant.tag));
        }

        self.enum_constructor_ids
            .insert(constructor_id, variant.clone());
        self.enum_variant_tags.insert(variant.tag, variant.clone());
        self.enum_variants_by_enum
            .entry(variant.enum_name.clone())
            .or_default()
            .push(variant);
        Ok(())
    }

    pub fn enum_variant_by_constructor_id(&self, unique_id: u32) -> Option<&EnumVariantInfo> {
        self.enum_constructor_ids.get(&unique_id)
    }

    pub fn enum_variant_by_tag(&self, tag: u32) -> Option<&EnumVariantInfo> {
        self.enum_variant_tags.get(&tag)
    }

    pub fn enum_variants_of(&self, enum_name: &str) -> Option<&Vec<EnumVariantInfo>> {
        self.enum_variants_by_enum.get(enum_name)
    }
}

#[cfg(test)]
mod tests {
    use super::{TypeDefState, TypeEnv, TypeKind};
    use crate::types::Ty;

    #[test]
    fn predeclare_type_def_assigns_deterministic_tags() {
        let mut env = TypeEnv::new();

        let user_tag = env.predeclare_type_def("User".into(), TypeKind::Struct, Vec::new());
        let point_tag = env.predeclare_type_def("Point".into(), TypeKind::Record, Vec::new());
        let user_tag_again = env.predeclare_type_def("User".into(), TypeKind::Struct, Vec::new());

        assert_eq!(user_tag, 2);
        assert_eq!(point_tag, 3);
        assert_eq!(user_tag_again, user_tag);
        assert_eq!(env.next_tag, 4);
    }

    #[test]
    fn resolve_type_def_signature_finalizes_predeclared_entry() {
        let mut env = TypeEnv::new();
        let tag = env.predeclare_type_def("ApiError".into(), TypeKind::Error, Vec::new());

        let before = env.lookup_type_def("ApiError").expect("must exist");
        assert_eq!(before.state, TypeDefState::Declared);
        assert!(before.fields.is_empty());

        let resolved = env.resolve_type_def_signature(
            "ApiError",
            vec![("code".into(), Ty::Int), ("msg".into(), Ty::Str)],
        );
        assert_eq!(resolved, Some(tag));
        assert!(env.is_type_signature_resolved("ApiError"));

        let after = env.lookup_type_def("ApiError").expect("must exist");
        assert_eq!(after.state, TypeDefState::SignatureResolved);
        assert_eq!(
            after.fields,
            vec![("code".into(), Ty::Int), ("msg".into(), Ty::Str)]
        );
    }

    #[test]
    fn register_type_def_still_behaves_as_single_step_api() {
        let mut env = TypeEnv::new();
        let tag = env.register_type_def(
            "Pair".into(),
            TypeKind::Record,
            vec![("first".into(), Ty::Int), ("second".into(), Ty::Str)],
        );

        assert_eq!(tag, 2);
        let def = env.lookup_type_def("Pair").expect("must exist");
        assert_eq!(def.state, TypeDefState::SignatureResolved);
        assert_eq!(def.tag, 2);
        assert_eq!(
            def.fields,
            vec![("first".into(), Ty::Int), ("second".into(), Ty::Str)]
        );
    }
}
