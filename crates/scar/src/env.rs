use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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
            Self::ConcreteError => TypeIdentity::Error,
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
    /// Declaration constraints aligned with `type_params`.
    pub type_param_bounds: Vec<Option<Symbol>>,
    pub type_param_vars: Vec<u32>,
    pub fields: Vec<(Symbol, Ty)>,
    #[serde(default)]
    pub field_type_spans: Vec<spire::ast::Span>,
    pub private_fields: HashSet<Symbol>,
    pub readonly_fields: HashSet<Symbol>,
    pub state: TypeDefState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeFieldPolicy {
    pub private: bool,
    pub readonly: bool,
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
    pub special_variant: Option<sindr::names::SpecialEnumVariantLowering>,
    pub constructor_name: Symbol,
    pub short_name: Symbol,
    pub enum_name: Symbol,
    pub enum_ty: Ty,
    pub tag: u32,
    pub payload: Vec<Ty>,
    #[serde(default)]
    pub payload_type_spans: Vec<spire::ast::Span>,
    pub discriminant: SurtrInt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VarScopeFrame {
    touched: HashSet<u32>,
    undo: Vec<(u32, Option<Ty>)>,
    concrete_errors_before: HashMap<u32, String>,
}

/// Type environment — tracks variable types and type definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeEnv {
    /// unique_id → type
    pub vars: HashMap<u32, Ty>,
    /// type name → definition; cloned environments share metadata until mutation.
    pub type_defs: HashMap<Symbol, Arc<TypeDefInfo>>,
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
    pub error_constructor_inputs: HashMap<u32, Vec<(String, Ty)>>,
    pub concrete_error_bindings: HashMap<u32, String>,
    /// enum constructor unique_id -> variant metadata
    pub enum_constructor_ids: HashMap<u32, Arc<EnumVariantInfo>>,
    /// enum tag -> variant metadata
    pub enum_variant_tags: HashMap<u32, Arc<EnumVariantInfo>>,
    /// enum type name -> variants
    pub enum_variants_by_enum: HashMap<Symbol, Arc<Vec<EnumVariantInfo>>>,
    /// type declaration bindings usable as type-root facet path heads.
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
            error_constructor_inputs: HashMap::new(),
            concrete_error_bindings: HashMap::new(),
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
            concrete_errors_before: self.concrete_error_bindings.clone(),
        });
    }

    /// Roll back all `bind_var` changes made since the last `push_var_scope`.
    pub fn pop_var_scope(&mut self) {
        let Some(frame) = self.var_scope_frames.pop() else {
            return;
        };
        self.concrete_error_bindings = frame.concrete_errors_before;
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
        type_param_bounds: Vec<Option<Symbol>>,
    ) -> u32 {
        debug_assert_eq!(type_params.len(), type_param_bounds.len());
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
            debug_assert!(
                existing.type_param_bounds == type_param_bounds,
                "Type predeclared with different type parameter bounds: {}",
                name
            );
            return existing.tag;
        }

        let tag = self.next_tag;
        self.next_tag += 1;
        self.type_defs.insert(
            key,
            Arc::new(TypeDefInfo {
                tag,
                kind,
                name,
                type_params,
                type_param_bounds,
                type_param_vars: Vec::new(),
                fields: Vec::new(),
                field_type_spans: Vec::new(),
                private_fields: HashSet::new(),
                readonly_fields: HashSet::new(),
                state: TypeDefState::Declared,
            }),
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
        type_param_vars: Vec<u32>,
        private_fields: HashSet<Symbol>,
        readonly_fields: HashSet<Symbol>,
    ) -> Option<u32> {
        let key = canonical_type_key(name);
        let def = Arc::make_mut(self.type_defs.get_mut(&key)?);
        def.fields = fields;
        def.type_param_vars = type_param_vars;
        def.private_fields = private_fields;
        def.readonly_fields = readonly_fields;
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
            .find_map(|candidate| self.type_defs.get(&candidate).map(Arc::as_ref))
    }

    pub fn lookup_type_def_mut(&mut self, name: &str) -> Option<&mut TypeDefInfo> {
        let key = type_lookup_candidates(name)
            .into_iter()
            .find(|candidate| self.type_defs.contains_key(candidate))?;
        self.type_defs.get_mut(&key).map(Arc::make_mut)
    }

    pub fn is_private_field(&self, type_name: &str, field_name: &str) -> bool {
        self.field_policy(type_name, field_name)
            .is_some_and(|policy| policy.private)
    }

    pub fn is_readonly_field(&self, type_name: &str, field_name: &str) -> bool {
        self.field_policy(type_name, field_name)
            .is_some_and(|policy| policy.readonly)
    }

    pub fn field_policy(&self, type_name: &str, field_name: &str) -> Option<TypeFieldPolicy> {
        self.lookup_type_def(type_name).map(|def| TypeFieldPolicy {
            private: def.private_fields.contains(field_name),
            readonly: def.readonly_fields.contains(field_name),
        })
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

        let indexed_variant = Arc::new(variant.clone());
        self.enum_constructor_ids
            .insert(constructor_id, Arc::clone(&indexed_variant));
        self.enum_variant_tags.insert(variant.tag, indexed_variant);
        Arc::make_mut(
            self.enum_variants_by_enum
                .entry(variant.enum_name.clone())
                .or_default(),
        )
        .push(variant);
        Ok(())
    }

    pub fn enum_variant_by_constructor_id(&self, unique_id: u32) -> Option<&EnumVariantInfo> {
        self.enum_constructor_ids.get(&unique_id).map(Arc::as_ref)
    }

    pub fn enum_variant_by_tag(&self, tag: u32) -> Option<&EnumVariantInfo> {
        self.enum_variant_tags.get(&tag).map(Arc::as_ref)
    }

    pub fn enum_variants_of(&self, enum_name: &str) -> Option<&Vec<EnumVariantInfo>> {
        type_lookup_candidates(enum_name)
            .into_iter()
            .find_map(|candidate| self.enum_variants_by_enum.get(&candidate).map(Arc::as_ref))
    }

    pub fn typed_enum_definitions(
        &self,
    ) -> HashMap<String, Vec<crate::typed::TypedEnumVariantDef>> {
        self.enum_variants_by_enum
            .iter()
            .map(|(name, variants)| (name.clone(), variants.iter().map(Into::into).collect()))
            .collect()
    }

    pub fn typed_nominal_definitions(
        &self,
    ) -> HashMap<String, crate::typed::TypedNominalDefinition> {
        self.type_defs
            .iter()
            .filter(|(_, definition)| {
                matches!(definition.kind, TypeKind::Struct | TypeKind::Record)
            })
            .map(|(name, definition)| {
                assert_eq!(
                    definition.state,
                    TypeDefState::SignatureResolved,
                    "checked nominal definition must have a resolved signature: {name}"
                );
                (
                    name.clone(),
                    crate::typed::TypedNominalDefinition {
                        type_param_vars: definition.type_param_vars.clone(),
                        fields: definition.fields.clone(),
                    },
                )
            })
            .collect()
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

    use super::{EnumVariantInfo, TypeDefState, TypeEnv, TypeKind};
    use crate::types::Ty;
    use spire::ast::Span;

    #[test]
    fn cloned_type_metadata_shares_until_signature_or_policy_updates() {
        let mut parent = TypeEnv::new();
        let tag = parent.predeclare_type_def(
            "Shared".into(),
            TypeKind::Struct,
            vec!["$A".into()],
            vec![Some("Eq".into())],
        );
        parent.resolve_type_def_signature(
            "Shared",
            vec![("value".into(), Ty::Var(7)), ("secret".into(), Ty::Str)],
            vec![7],
            HashSet::from(["secret".into()]),
            HashSet::from(["value".into()]),
        );
        parent
            .lookup_type_def_mut("Shared")
            .unwrap()
            .field_type_spans = vec![Span { start: 10, end: 14 }, Span { start: 20, end: 26 }];
        parent.predeclare_type_def("Unchanged".into(), TypeKind::Record, vec![], vec![]);
        parent.bind_var(44, Ty::Int);
        parent.push_var_scope();
        parent.bind_var(44, Ty::Str);
        parent.next_fun_idx = 23;
        parent.next_tyvar = 30;
        let saved = parent.clone();
        let mut sibling = parent.clone();
        let mut child = parent.clone();
        let mut policy_only = parent.clone();
        assert!(std::ptr::eq(
            parent.lookup_type_def("Shared").unwrap(),
            child.lookup_type_def("Shared").unwrap(),
        ));
        assert_eq!(
            bincode::serialize(&parent.type_defs["Global::Shared"]).unwrap(),
            bincode::serialize(parent.lookup_type_def("Shared").unwrap()).unwrap(),
        );

        assert_eq!(
            child.resolve_type_def_signature(
                "Shared",
                vec![("value".into(), Ty::Bool), ("secret".into(), Ty::Str)],
                vec![19],
                HashSet::from(["value".into()]),
                HashSet::from(["secret".into()]),
            ),
            Some(tag)
        );
        let changed = child.lookup_type_def_mut("Shared").unwrap();
        changed.type_params.push("$B".into());
        changed.type_param_bounds.push(None);
        changed.type_param_vars.push(20);
        changed.field_type_spans[0] = Span { start: 70, end: 74 };
        assert!(std::ptr::eq(
            parent.lookup_type_def("Shared").unwrap(),
            policy_only.lookup_type_def("Shared").unwrap(),
        ));
        let changed_policy = policy_only.lookup_type_def_mut("Shared").unwrap();
        changed_policy.field_type_spans[0] = Span { start: 80, end: 84 };
        changed_policy.private_fields.clear();
        changed_policy.readonly_fields.clear();
        assert!(!std::ptr::eq(
            parent.lookup_type_def("Shared").unwrap(),
            policy_only.lookup_type_def("Shared").unwrap(),
        ));
        assert!(std::ptr::eq(
            parent.lookup_type_def("Unchanged").unwrap(),
            policy_only.lookup_type_def("Unchanged").unwrap(),
        ));
        assert!(!policy_only.is_private_field("Shared", "secret"));
        assert!(!policy_only.is_readonly_field("Shared", "value"));
        assert_eq!(
            policy_only.lookup_type_def("Shared").unwrap().fields[0].1,
            Ty::Var(7)
        );
        for original in [&parent, &saved, &sibling] {
            let definition = original.lookup_type_def("Shared").unwrap();
            assert_eq!(definition.fields[0].1, Ty::Var(7));
            assert_eq!(definition.type_params, vec!["$A"]);
            assert_eq!(definition.type_param_vars, vec![7]);
            assert_eq!(definition.field_type_spans[0], Span { start: 10, end: 14 });
            assert!(original.is_private_field("Shared", "secret"));
            assert!(original.is_readonly_field("Shared", "value"));
        }
        assert!(!std::ptr::eq(
            parent.lookup_type_def("Shared").unwrap(),
            child.lookup_type_def("Shared").unwrap()
        ));
        assert!(std::ptr::eq(
            parent.lookup_type_def("Unchanged").unwrap(),
            child.lookup_type_def("Unchanged").unwrap()
        ));
        assert!(child.is_private_field("Shared", "value"));
        assert!(child.is_readonly_field("Shared", "secret"));

        child.bind_var(44, Ty::Bool);
        child.pop_var_scope();
        assert_eq!(child.lookup_var(44), Some(&Ty::Int));
        assert_eq!(parent.lookup_var(44), Some(&Ty::Str));
        sibling.pop_var_scope();
        assert_eq!(sibling.lookup_var(44), Some(&Ty::Int));
        assert_eq!(child.fresh_tyvar(), Ty::Var(30));
        assert_eq!(child.reserve_tag(), parent.next_tag);
        assert_eq!(parent.next_tyvar, 30);
        assert_eq!(parent.next_fun_idx, 23);
        assert_eq!(saved.next_tag, parent.next_tag);

        let restored: TypeEnv = bincode::deserialize(&bincode::serialize(&saved).unwrap()).unwrap();
        assert_eq!(
            restored.lookup_type_def("Shared").unwrap().field_type_spans,
            saved.lookup_type_def("Shared").unwrap().field_type_spans
        );
        assert_eq!(restored.lookup_var(44), Some(&Ty::Str));
        assert_eq!(
            (
                restored.next_tag,
                restored.next_fun_idx,
                restored.next_tyvar
            ),
            (saved.next_tag, 23, 30)
        );
        let mut restored = restored;
        restored.pop_var_scope();
        assert_eq!(restored.lookup_var(44), Some(&Ty::Int));
    }

    fn shared_variant(tag: u32, short_name: &str) -> EnumVariantInfo {
        EnumVariantInfo {
            special_variant: None,
            constructor_name: format!("Global::Choice::{short_name}"),
            short_name: short_name.into(),
            enum_name: "Global::Choice".into(),
            enum_ty: Ty::Enum("Global::Choice".into(), vec![Ty::Var(7)]),
            tag,
            payload: vec![Ty::Var(7)],
            payload_type_spans: vec![Span { start: 10, end: 14 }],
            discriminant: tag.into(),
        }
    }

    #[test]
    fn cloned_enum_metadata_shares_indexes_and_isolates_appended_variants() {
        let mut parent = TypeEnv::new();
        parent
            .register_enum_variant(10, shared_variant(2, "First"))
            .unwrap();
        parent
            .enum_variants_by_enum
            .insert("Global::Empty".into(), Default::default());
        let saved = parent.clone();
        let sibling = parent.clone();
        let mut child = parent.clone();
        let first = parent.enum_variant_by_constructor_id(10).unwrap();
        assert!(std::ptr::eq(first, parent.enum_variant_by_tag(2).unwrap()));
        assert!(std::ptr::eq(
            first,
            child.enum_variant_by_constructor_id(10).unwrap()
        ));
        assert!(std::ptr::eq(
            parent.enum_variants_of("Choice").unwrap(),
            child.enum_variants_of("Choice").unwrap()
        ));
        assert!(std::ptr::eq(
            parent.enum_variants_of("Empty").unwrap(),
            child.enum_variants_of("Empty").unwrap()
        ));
        assert_eq!(
            bincode::serialize(&parent.enum_constructor_ids[&10]).unwrap(),
            bincode::serialize(first).unwrap()
        );
        assert_eq!(
            bincode::serialize(&parent.enum_variant_tags[&2]).unwrap(),
            bincode::serialize(first).unwrap()
        );
        assert_eq!(
            bincode::serialize(&parent.enum_variants_by_enum["Global::Choice"]).unwrap(),
            bincode::serialize(parent.enum_variants_of("Choice").unwrap()).unwrap()
        );

        child
            .register_enum_variant(11, shared_variant(3, "Second"))
            .unwrap();
        assert_eq!(child.enum_variants_of("Choice").unwrap().len(), 2);
        assert!(!std::ptr::eq(
            parent.enum_variants_of("Choice").unwrap(),
            child.enum_variants_of("Choice").unwrap()
        ));
        assert!(std::ptr::eq(
            parent.enum_variants_of("Empty").unwrap(),
            child.enum_variants_of("Empty").unwrap()
        ));
        for original in [&parent, &saved, &sibling] {
            assert_eq!(original.enum_variants_of("Choice").unwrap().len(), 1);
            assert!(original.enum_variant_by_constructor_id(11).is_none());
            assert!(original.enum_variant_by_tag(3).is_none());
            assert!(original.enum_variants_of("Empty").unwrap().is_empty());
        }
        assert!(std::ptr::eq(first, child.enum_variant_by_tag(2).unwrap()));
        assert!(std::ptr::eq(
            child.enum_variant_by_constructor_id(11).unwrap(),
            child.enum_variant_by_tag(3).unwrap()
        ));
        let unchanged = bincode::serialize(&child).unwrap();
        // A duplicate constructor is diagnosed first even when its tag also collides.
        assert_eq!(
            child
                .register_enum_variant(10, shared_variant(3, "DuplicateId"))
                .unwrap_err(),
            "enum constructor id 10 already registered"
        );
        assert_eq!(bincode::serialize(&child).unwrap(), unchanged);
        assert_eq!(
            child
                .register_enum_variant(12, shared_variant(3, "DuplicateTag"))
                .unwrap_err(),
            "enum tag 3 already registered"
        );
        assert_eq!(bincode::serialize(&child).unwrap(), unchanged);

        let restored: TypeEnv = bincode::deserialize(&unchanged).unwrap();
        assert_eq!(
            restored.enum_variant_by_constructor_id(11).unwrap(),
            child.enum_variant_by_constructor_id(11).unwrap()
        );
        assert_eq!(restored.enum_variant_by_tag(2).unwrap(), first);
        assert_eq!(
            restored.enum_variants_of("Choice").unwrap(),
            child.enum_variants_of("Choice").unwrap()
        );
        assert!(restored.enum_variants_of("Empty").unwrap().is_empty());
        assert_eq!(
            (
                restored.next_tag,
                restored.next_fun_idx,
                restored.next_tyvar
            ),
            (parent.next_tag, parent.next_fun_idx, parent.next_tyvar)
        );
    }

    #[test]
    fn predeclare_type_def_assigns_deterministic_tags() {
        let mut env = TypeEnv::new();

        let user_tag =
            env.predeclare_type_def("User".into(), TypeKind::Struct, Vec::new(), Vec::new());
        let point_tag =
            env.predeclare_type_def("Point".into(), TypeKind::Record, Vec::new(), Vec::new());
        let user_tag_again =
            env.predeclare_type_def("User".into(), TypeKind::Struct, Vec::new(), Vec::new());

        assert_eq!(user_tag, 2);
        assert_eq!(point_tag, 3);
        assert_eq!(user_tag_again, user_tag);
        assert_eq!(env.next_tag, 4);
    }

    #[test]
    fn resolve_type_def_signature_finalizes_predeclared_entry() {
        let mut env = TypeEnv::new();
        let tag = env.predeclare_type_def(
            "ApiError".into(),
            TypeKind::ConcreteError,
            Vec::new(),
            Vec::new(),
        );

        let before = env.lookup_type_def("ApiError").expect("must exist");
        assert_eq!(before.state, TypeDefState::Declared);
        assert!(before.fields.is_empty());
        let declared = env.clone();

        let resolved = env.resolve_type_def_signature(
            "ApiError",
            vec![("code".into(), Ty::Int), ("msg".into(), Ty::Str)],
            Vec::new(),
            HashSet::new(),
            HashSet::new(),
        );
        assert_eq!(resolved, Some(tag));
        assert!(env.is_type_signature_resolved("ApiError"));

        let after = env.lookup_type_def("ApiError").expect("must exist");
        assert_eq!(after.state, TypeDefState::SignatureResolved);
        assert_eq!(
            after.fields,
            vec![("code".into(), Ty::Int), ("msg".into(), Ty::Str)]
        );
        let saved = declared.lookup_type_def("ApiError").expect("must exist");
        assert_eq!(saved.state, TypeDefState::Declared);
        assert!(saved.fields.is_empty());
        assert_eq!(saved.tag, tag);
    }

    #[test]
    fn predeclare_and_resolve_replace_legacy_single_step_registration() {
        let mut env = TypeEnv::new();
        let tag = env.predeclare_type_def("Pair".into(), TypeKind::Record, Vec::new(), Vec::new());
        let resolved = env.resolve_type_def_signature(
            "Pair",
            vec![("first".into(), Ty::Int), ("second".into(), Ty::Str)],
            Vec::new(),
            HashSet::new(),
            HashSet::new(),
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
        env.predeclare_type_def("User".into(), TypeKind::Struct, Vec::new(), Vec::new());
        env.resolve_type_def_signature(
            "User",
            vec![("name".into(), Ty::Str), ("password".into(), Ty::Str)],
            Vec::new(),
            HashSet::from(["password".into()]),
            HashSet::new(),
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
        env.predeclare_type_def("Profile".into(), TypeKind::Struct, Vec::new(), Vec::new());
        env.resolve_type_def_signature(
            "Profile",
            vec![("name".into(), Ty::Str), ("score".into(), Ty::Int)],
            Vec::new(),
            HashSet::new(),
            HashSet::from(["name".into()]),
        );

        assert!(env.is_readonly_field("Profile", "name"));
        assert!(env.is_readonly_field("Global::Profile", "name"));
        assert!(env.is_readonly_field("Types::Profile", "name"));
        let policy = env
            .field_policy("Types::Profile", "name")
            .expect("field policy should resolve through surface candidates");
        assert!(!policy.private);
        assert!(policy.readonly);
    }
}
