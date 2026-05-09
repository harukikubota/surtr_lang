use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use sindr::names::TypeIdentity;
use sindr::primitives::SurtrInt;
use spire::ast::Symbol;

use crate::types::Ty;

fn canonical_type_key(name: &str) -> String {
    if name.contains("::") {
        name.to_string()
    } else {
        format!("Global::{name}")
    }
}

fn type_lookup_candidates(name: &str) -> Vec<String> {
    let mut out = vec![name.to_string(), canonical_type_key(name)];
    let segments = name.split("::").collect::<Vec<_>>();
    if segments.len() > 1 {
        for start in 1..segments.len() {
            let suffix = segments[start..].join("::");
            if !out.iter().any(|candidate| candidate == &suffix) {
                out.push(suffix.clone());
            }
            let canonical_suffix = canonical_type_key(&suffix);
            if !out.iter().any(|candidate| candidate == &canonical_suffix) {
                out.push(canonical_suffix);
            }
        }
    }
    out
}

/// Kind of user-defined type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeKind {
    Struct,
    Record,
    ConcreteError,
    Enum,
}

impl TypeKind {
    pub const fn identity(self) -> TypeIdentity {
        match self {
            Self::Struct => TypeIdentity::Struct,
            Self::Record => TypeIdentity::Record,
            Self::ConcreteError => TypeIdentity::ConcreteError,
            Self::Enum => TypeIdentity::Enum,
        }
    }
}

/// Metadata for a user-defined type (struct, record, error).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDefInfo {
    pub tag: u32,
    pub kind: TypeKind,
    pub name: Symbol,
    pub type_params: Vec<Symbol>,
    pub fields: Vec<(Symbol, Ty)>,
    pub private_fields: HashSet<Symbol>,
    pub readonly_fields: HashSet<Symbol>,
    pub readonly_root: bool,
    pub state: TypeDefState,
}

/// Resolution state for user-defined type signatures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeDefState {
    /// Name/kind/tag are known, but field signature is not finalized yet.
    Declared,
    /// Full field signature is available.
    SignatureResolved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumVariantInfo {
    pub constructor_name: Symbol,
    pub short_name: Symbol,
    pub enum_name: Symbol,
    pub enum_ty: Ty,
    pub tag: u32,
    pub payload: Vec<Ty>,
    pub discriminant: SurtrInt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VarScopeFrame {
    touched: HashSet<u32>,
    undo: Vec<(u32, Option<Ty>)>,
}

/// Type environment — tracks variable types and type definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// type declaration bindings usable as type-root lens path heads.
    pub type_constructor_ids: HashSet<u32>,
    var_scope_frames: Vec<VarScopeFrame>,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
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
            type_constructor_ids: HashSet::new(),
            var_scope_frames: Vec::new(),
        }
    }

    /// Bind a variable (by unique_id) to a type.
    pub fn bind_var(&mut self, unique_id: u32, ty: Ty) {
        if let Some(frame) = self.var_scope_frames.last_mut() {
            if frame.touched.insert(unique_id) {
                frame
                    .undo
                    .push((unique_id, self.vars.get(&unique_id).cloned()));
            }
        }
        self.vars.insert(unique_id, ty);
    }

    /// Open a scoped mutation frame for `vars`.
    ///
    /// During an active frame, first writes to each `unique_id` record its
    /// previous value so `pop_var_scope` can restore the exact prior state.
    pub fn push_var_scope(&mut self) {
        self.var_scope_frames.push(VarScopeFrame {
            touched: HashSet::new(),
            undo: Vec::new(),
        });
    }

    /// Roll back all `bind_var` changes made since the last `push_var_scope`.
    pub fn pop_var_scope(&mut self) {
        let Some(frame) = self.var_scope_frames.pop() else {
            return;
        };
        for (unique_id, old) in frame.undo.into_iter().rev() {
            if let Some(old_ty) = old {
                self.vars.insert(unique_id, old_ty);
            } else {
                self.vars.remove(&unique_id);
            }
        }
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
        let key = canonical_type_key(&name);
        if let Some(existing) = self.type_defs.get(&key) {
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
            key,
            TypeDefInfo {
                tag,
                kind,
                name,
                type_params,
                fields: Vec::new(),
                private_fields: HashSet::new(),
                readonly_fields: HashSet::new(),
                readonly_root: false,
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
        private_fields: HashSet<Symbol>,
        readonly_fields: HashSet<Symbol>,
        readonly_root: bool,
    ) -> Option<u32> {
        let key = canonical_type_key(name);
        let def = self.type_defs.get_mut(&key)?;
        def.fields = fields;
        def.private_fields = private_fields;
        def.readonly_fields = readonly_fields;
        def.readonly_root = readonly_root;
        def.state = TypeDefState::SignatureResolved;
        Some(def.tag)
    }

    /// Reserve a fresh runtime tag.
    pub fn reserve_tag(&mut self) -> u32 {
        let tag = self.next_tag;
        self.next_tag += 1;
        tag
    }

    /// Look up a type definition by name.
    pub fn lookup_type_def(&self, name: &str) -> Option<&TypeDefInfo> {
        type_lookup_candidates(name)
            .into_iter()
            .find_map(|candidate| self.type_defs.get(&candidate))
    }

    pub fn is_private_field(&self, type_name: &str, field_name: &str) -> bool {
        self.lookup_type_def(type_name)
            .is_some_and(|def| def.private_fields.contains(field_name))
    }

    pub fn is_readonly_field(&self, type_name: &str, field_name: &str) -> bool {
        self.lookup_type_def(type_name)
            .is_some_and(|def| def.readonly_fields.contains(field_name))
    }

    pub fn is_readonly_root(&self, type_name: &str) -> bool {
        self.lookup_type_def(type_name)
            .is_some_and(|def| def.readonly_root)
    }

    pub fn is_type_signature_resolved(&self, name: &str) -> bool {
        self.lookup_type_def(name)
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
        self.error_type_names.insert(canonical_type_key(&name));
    }

    pub fn is_declared_error_type_name(&self, name: &str) -> bool {
        type_lookup_candidates(name)
            .into_iter()
            .any(|candidate| self.error_type_names.contains(&candidate))
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
        type_lookup_candidates(enum_name)
            .into_iter()
            .find_map(|candidate| self.enum_variants_by_enum.get(&candidate))
    }

    pub fn register_type_constructor_id(&mut self, unique_id: u32) {
        self.type_constructor_ids.insert(unique_id);
    }

    pub fn is_type_constructor_id(&self, unique_id: u32) -> bool {
        self.type_constructor_ids.contains(&unique_id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

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
        let tag = env.predeclare_type_def("ApiError".into(), TypeKind::ConcreteError, Vec::new());

        let before = env.lookup_type_def("ApiError").expect("must exist");
        assert_eq!(before.state, TypeDefState::Declared);
        assert!(before.fields.is_empty());

        let resolved = env.resolve_type_def_signature(
            "ApiError",
            vec![("code".into(), Ty::Int), ("msg".into(), Ty::Str)],
            HashSet::new(),
            HashSet::new(),
            false,
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
    fn predeclare_and_resolve_replace_legacy_single_step_registration() {
        let mut env = TypeEnv::new();
        let tag = env.predeclare_type_def("Pair".into(), TypeKind::Record, Vec::new());
        let resolved = env.resolve_type_def_signature(
            "Pair",
            vec![("first".into(), Ty::Int), ("second".into(), Ty::Str)],
            HashSet::new(),
            HashSet::new(),
            false,
        );

        assert_eq!(tag, 2);
        assert_eq!(resolved, Some(2));
        let def = env.lookup_type_def("Pair").expect("must exist");
        assert_eq!(def.state, TypeDefState::SignatureResolved);
        assert_eq!(def.tag, 2);
        assert_eq!(
            def.fields,
            vec![("first".into(), Ty::Int), ("second".into(), Ty::Str)]
        );
    }

    #[test]
    fn private_field_lookup_accepts_global_and_module_prefixed_names() {
        let mut env = TypeEnv::new();
        env.predeclare_type_def("User".into(), TypeKind::Struct, Vec::new());
        env.resolve_type_def_signature(
            "User",
            vec![("name".into(), Ty::Str), ("password".into(), Ty::Str)],
            HashSet::from(["password".into()]),
            HashSet::new(),
            false,
        );

        assert!(env.is_private_field("User", "password"));
        assert!(env.is_private_field("Global::User", "password"));
        assert!(env.is_private_field("Types::User", "password"));
        assert!(env.is_type_signature_resolved("Global::User"));
        assert!(env.is_type_signature_resolved("Types::User"));
    }

    #[test]
    fn readonly_metadata_lookup_accepts_global_and_module_prefixed_names() {
        let mut env = TypeEnv::new();
        env.predeclare_type_def("Profile".into(), TypeKind::Struct, Vec::new());
        env.resolve_type_def_signature(
            "Profile",
            vec![("name".into(), Ty::Str), ("score".into(), Ty::Int)],
            HashSet::new(),
            HashSet::from(["name".into()]),
            true,
        );

        assert!(env.is_readonly_field("Profile", "name"));
        assert!(env.is_readonly_field("Global::Profile", "name"));
        assert!(env.is_readonly_field("Types::Profile", "name"));
        assert!(env.is_readonly_root("Profile"));
        assert!(env.is_readonly_root("Global::Profile"));
        assert!(env.is_readonly_root("Types::Profile"));
    }
}
