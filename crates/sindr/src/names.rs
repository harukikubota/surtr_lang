use serde::{Deserialize, Serialize};

/// Internal canonical namespace used for implicit top-level definitions.
///
/// The compiler keeps this namespace in canonical identities, but user-facing
/// surfaces should hide it to keep signatures and diagnostics stable.
pub const IMPLICIT_ROOT_NAMESPACE_PREFIX: &str = "Global::";

/// Return a path name with the implicit root namespace hidden when it appears at
/// the beginning of a canonical name.
pub fn surface_path_name(name: &str) -> &str {
    name.strip_prefix(IMPLICIT_ROOT_NAMESPACE_PREFIX)
        .unwrap_or(name)
}

/// Render a canonical name for user-facing display.
///
/// This hides a leading `Global::` and nested `::Global::` segments that can
/// appear in generated trait/helper paths, without changing runtime identity.
pub fn surface_rendered_name(name: &str) -> String {
    surface_path_name(name).replace("::Global::", "::")
}

/// Compare two names after hiding a leading implicit root namespace.
pub fn surface_path_eq(left: &str, right: &str) -> bool {
    surface_path_name(left) == surface_path_name(right)
}

/// Compare two names after applying the full user-facing surface rendering.
pub fn surface_rendered_eq(left: &str, right: &str) -> bool {
    surface_rendered_name(left) == surface_rendered_name(right)
}

/// Canonical compile-space symbol identity. This form may include implicit
/// compiler namespaces such as `Global::`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanonicalSymbolName(String);

impl CanonicalSymbolName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn to_surface_symbol_name(&self) -> SurfaceSymbolName {
        SurfaceSymbolName(surface_rendered_name(&self.0))
    }
}

/// User-facing rendered symbol name for diagnostics, docs, and completion UI.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SurfaceSymbolName(String);

impl SurfaceSymbolName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

/// A symbol reference in the names visible from a specific source context.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VisibleSymbolRef(String);

impl VisibleSymbolRef {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn from_surface(surface: SurfaceSymbolName) -> Self {
        Self(surface.into_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn matches_qualified_name(&self, qualified_name: &CanonicalSymbolName) -> bool {
        let qualified = surface_path_name(qualified_name.as_str());
        let visible = surface_path_name(&self.0);
        qualified == visible
            || qualified
                .rsplit("::")
                .next()
                .is_some_and(|tail| tail == visible)
    }
}

/// Marker for the compiler's implicit root namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImplicitRootNamespace;

impl ImplicitRootNamespace {
    pub const PREFIX: &'static str = IMPLICIT_ROOT_NAMESPACE_PREFIX;

    pub fn hide(name: &str) -> &str {
        surface_path_name(name)
    }
}

/// Bump when compile-space symbol capability semantics change in a way that
/// invalidates staged semantic snapshots.
pub const SYMBOL_CAPABILITY_SCHEMA_VERSION: u32 = 1;

/// Surface-level type identity defined by the language spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeIdentity {
    Type,
    Struct,
    Record,
    Enum,
    ConcreteError,
    Mod,
    Const,
}

/// Compile-space root kind used when a symbol can serve as a Facet path root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FacetRootKind {
    TypeRoot,
    Tuple,
    List,
    HashMap,
}

/// Compile-space capability flags attached to a resolved symbol identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolCapabilities {
    pub type_annotation: bool,
    pub module_owner: bool,
    pub impl_target: bool,
    pub facet_root_path: Option<FacetRootKind>,
}

impl SymbolCapabilities {
    pub const fn new(
        type_annotation: bool,
        module_owner: bool,
        impl_target: bool,
        facet_root_path: Option<FacetRootKind>,
    ) -> Self {
        Self {
            type_annotation,
            module_owner,
            impl_target,
            facet_root_path,
        }
    }
}

/// Compile-space identity plus capabilities for a resolved symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolIdentityInfo {
    pub identity: TypeIdentity,
    pub capabilities: SymbolCapabilities,
}

impl SymbolIdentityInfo {
    pub const fn new(identity: TypeIdentity, capabilities: SymbolCapabilities) -> Self {
        Self {
            identity,
            capabilities,
        }
    }
}

/// Builtin symbol surface metadata for compile-space name/capability queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuiltinSymbolSurfaceMeta {
    pub name: &'static str,
    pub identity: TypeIdentity,
    pub capabilities: SymbolCapabilities,
}

/// Compile-space usage policy for builtin type heads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuiltinTypeUsagePolicy {
    pub type_annotation_allowed: bool,
    pub signature_allowed: bool,
    pub runtime_value_allowed: bool,
    pub type_ref_witness_allowed: bool,
    pub process_boundary_allowed: bool,
    pub facet_value_forbidden_in_stage1: bool,
    pub clause_block_surface_only: bool,
    pub lazy_signature_surface_only: bool,
}

impl BuiltinTypeUsagePolicy {
    pub const fn new(
        type_annotation_allowed: bool,
        signature_allowed: bool,
        runtime_value_allowed: bool,
        type_ref_witness_allowed: bool,
        process_boundary_allowed: bool,
        facet_value_forbidden_in_stage1: bool,
        clause_block_surface_only: bool,
        lazy_signature_surface_only: bool,
    ) -> Self {
        Self {
            type_annotation_allowed,
            signature_allowed,
            runtime_value_allowed,
            type_ref_witness_allowed,
            process_boundary_allowed,
            facet_value_forbidden_in_stage1,
            clause_block_surface_only,
            lazy_signature_surface_only,
        }
    }

    pub const fn ordinary_runtime_type() -> Self {
        Self::new(true, true, true, false, true, false, false, false)
    }

    pub const fn compiler_surface_only() -> Self {
        Self::new(false, false, false, false, false, true, false, false)
    }

    pub const fn clause_block_surface_only() -> Self {
        Self::new(false, false, false, false, false, true, true, false)
    }

    pub const fn lazy_signature_surface_only() -> Self {
        Self::new(false, false, false, false, false, true, false, true)
    }
}

/// Canonical builtin type heads reserved by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeName {
    Int,
    Float,
    String,
    Boolean,
    Unit,
    Closure,
    MatchArms,
    CondClauses,
    BulkUpdateEntries,
    Error,
    Regex,
    RegexCaptures,
    RegexMatch,
    RandomGenerator,
    List,
    HashMap,
    Generator,
    Result,
    Duration,
    ProcessInit,
    Lazy,
    TypeRef,
    Hole,
    Facet,
    Pid,
    FileHandle,
    Workers,
    WorkerLease,
    TaskHandle,
}

impl TypeName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Float => "Float",
            Self::String => "String",
            Self::Boolean => "Boolean",
            Self::Unit => "Unit",
            Self::Closure => "Closure",
            Self::MatchArms => "MatchArms",
            Self::CondClauses => "CondClauses",
            Self::BulkUpdateEntries => "BulkUpdateEntries",
            Self::Error => "Error",
            Self::Regex => "Regex",
            Self::RegexCaptures => "RegexCaptures",
            Self::RegexMatch => "RegexMatch",
            Self::RandomGenerator => "RandomGenerator",
            Self::List => "List",
            Self::HashMap => "HashMap",
            Self::Generator => "Generator",
            Self::Result => "Result",
            Self::Duration => "Duration",
            Self::ProcessInit => "ProcessInit",
            Self::Lazy => "Lazy",
            Self::TypeRef => "TypeRef",
            Self::Hole => "Hole",
            Self::Facet => "Facet",
            Self::Pid => "PID",
            Self::FileHandle => "FileHandle",
            Self::Workers => "Workers",
            Self::WorkerLease => "WorkerLease",
            Self::TaskHandle => "TaskHandle",
        }
    }

    pub const fn identity(self) -> TypeIdentity {
        TypeIdentity::Type
    }

    pub const fn supports_inherent_impl(self) -> bool {
        !matches!(
            self,
            Self::TypeRef
                | Self::Hole
                | Self::Closure
                | Self::MatchArms
                | Self::CondClauses
                | Self::BulkUpdateEntries
                | Self::ProcessInit
                | Self::Lazy
                | Self::Pid
                | Self::FileHandle
        )
    }

    pub const fn usage_policy(self) -> BuiltinTypeUsagePolicy {
        match self {
            Self::TypeRef => {
                BuiltinTypeUsagePolicy::new(false, false, false, true, false, false, false, false)
            }
            Self::ProcessInit => {
                BuiltinTypeUsagePolicy::new(false, false, false, false, true, false, false, false)
            }
            Self::Lazy => BuiltinTypeUsagePolicy::lazy_signature_surface_only(),
            Self::Hole | Self::Closure => BuiltinTypeUsagePolicy::compiler_surface_only(),
            Self::MatchArms | Self::CondClauses | Self::BulkUpdateEntries => {
                BuiltinTypeUsagePolicy::clause_block_surface_only()
            }
            Self::Facet => {
                BuiltinTypeUsagePolicy::new(true, true, true, false, true, true, false, false)
            }
            Self::Pid | Self::Workers | Self::WorkerLease | Self::TaskHandle => {
                BuiltinTypeUsagePolicy::new(true, true, true, false, true, false, false, false)
            }
            _ => BuiltinTypeUsagePolicy::ordinary_runtime_type(),
        }
    }
}

pub fn builtin_type_name(name: &str) -> Option<TypeName> {
    match name {
        "Int" => Some(TypeName::Int),
        "Float" => Some(TypeName::Float),
        "String" => Some(TypeName::String),
        "Boolean" => Some(TypeName::Boolean),
        "Unit" => Some(TypeName::Unit),
        "Closure" => Some(TypeName::Closure),
        "MatchArms" => Some(TypeName::MatchArms),
        "CondClauses" => Some(TypeName::CondClauses),
        "BulkUpdateEntries" => Some(TypeName::BulkUpdateEntries),
        "Error" => Some(TypeName::Error),
        "Regex" => Some(TypeName::Regex),
        "RegexCaptures" => Some(TypeName::RegexCaptures),
        "RegexMatch" => Some(TypeName::RegexMatch),
        "RandomGenerator" => Some(TypeName::RandomGenerator),
        "List" => Some(TypeName::List),
        "HashMap" => Some(TypeName::HashMap),
        "Generator" => Some(TypeName::Generator),
        "Result" => Some(TypeName::Result),
        "Duration" => Some(TypeName::Duration),
        "ProcessInit" => Some(TypeName::ProcessInit),
        "Lazy" => Some(TypeName::Lazy),
        "TypeRef" => Some(TypeName::TypeRef),
        "Hole" => Some(TypeName::Hole),
        "Facet" => Some(TypeName::Facet),
        "PID" => Some(TypeName::Pid),
        "FileHandle" => Some(TypeName::FileHandle),
        "Workers" => Some(TypeName::Workers),
        "WorkerLease" => Some(TypeName::WorkerLease),
        "TaskHandle" => Some(TypeName::TaskHandle),
        _ => None,
    }
}

pub fn builtin_type_usage_policy(name: &str) -> Option<BuiltinTypeUsagePolicy> {
    builtin_type_name(surface_path_name(name)).map(TypeName::usage_policy)
}

/// Return builtin surface metadata for compile-space name/capability queries.
pub fn builtin_symbol_surface_meta(name: &str) -> Option<BuiltinSymbolSurfaceMeta> {
    let name = surface_path_name(name);
    match name {
        "Tuple" => {
            return Some(BuiltinSymbolSurfaceMeta {
                name: "Tuple",
                identity: TypeIdentity::Type,
                capabilities: SymbolCapabilities::new(
                    false,
                    true,
                    false,
                    Some(FacetRootKind::Tuple),
                ),
            });
        }
        "Function" => {
            return Some(BuiltinSymbolSurfaceMeta {
                name: "Function",
                identity: TypeIdentity::Type,
                capabilities: SymbolCapabilities::new(false, true, false, None),
            });
        }
        _ => {}
    }

    let type_name = builtin_type_name(name)?;
    let facet_root_path = match type_name {
        TypeName::Boolean => Some(FacetRootKind::TypeRoot),
        TypeName::List => Some(FacetRootKind::List),
        TypeName::HashMap => Some(FacetRootKind::HashMap),
        _ => None,
    };
    let impl_target = type_name.supports_inherent_impl();
    Some(BuiltinSymbolSurfaceMeta {
        name: type_name.as_str(),
        identity: type_name.identity(),
        capabilities: SymbolCapabilities::new(true, impl_target, impl_target, facet_root_path),
    })
}

/// Return compile-space identity/capability metadata for builtin surface roots.
pub fn builtin_symbol_identity_info(name: &str) -> Option<SymbolIdentityInfo> {
    builtin_symbol_surface_meta(name)
        .map(|meta| SymbolIdentityInfo::new(meta.identity, meta.capabilities))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_name_rendering_hides_implicit_global_namespace() {
        assert_eq!(surface_path_name("Global::User"), "User");
        assert_eq!(surface_rendered_name("Global::User::new"), "User::new");
        assert_eq!(
            surface_rendered_name("Trait::Global::User::method"),
            "Trait::User::method"
        );
    }

    #[test]
    fn surface_name_equality_normalizes_canonical_and_rendered_forms() {
        assert!(surface_path_eq("Global::User", "User"));
        assert!(surface_rendered_eq(
            "Trait::Global::User::method",
            "Trait::User::method"
        ));
        assert!(!surface_rendered_eq("Trait::User::method", "User::method"));
    }

    #[test]
    fn symbol_name_types_separate_canonical_surface_and_visible_forms() {
        let canonical = CanonicalSymbolName::new("Trait::Global::User::method");
        let surface = canonical.to_surface_symbol_name();
        let visible = VisibleSymbolRef::from_surface(surface.clone());

        assert_eq!(canonical.as_str(), "Trait::Global::User::method");
        assert_eq!(surface.as_str(), "Trait::User::method");
        assert_eq!(visible.as_str(), "Trait::User::method");
        let tail_visible = VisibleSymbolRef::new("method");
        assert!(tail_visible.matches_qualified_name(&canonical));
        assert_eq!(ImplicitRootNamespace::hide("Global::User"), "User");
    }

    #[test]
    fn builtin_symbol_identity_info_marks_core_type_capabilities() {
        let string = builtin_symbol_identity_info("String").expect("String should be known");
        assert_eq!(string.identity, TypeIdentity::Type);
        assert!(string.capabilities.type_annotation);
        assert!(string.capabilities.module_owner);
        assert!(string.capabilities.impl_target);
        assert_eq!(string.capabilities.facet_root_path, None);

        let result = builtin_symbol_identity_info("Result").expect("Result should be known");
        assert_eq!(result.identity, TypeIdentity::Type);
        assert!(result.capabilities.type_annotation);
        assert!(result.capabilities.module_owner);
        assert!(result.capabilities.impl_target);
        assert_eq!(result.capabilities.facet_root_path, None);

        let facet = builtin_symbol_identity_info("Facet").expect("Facet should be known");
        assert_eq!(facet.identity, TypeIdentity::Type);
        assert!(facet.capabilities.type_annotation);
        assert!(facet.capabilities.module_owner);
        assert!(facet.capabilities.impl_target);
        assert_eq!(facet.capabilities.facet_root_path, None);

        let boolean = builtin_symbol_identity_info("Boolean").expect("Boolean should be known");
        assert_eq!(boolean.identity, TypeIdentity::Type);
        assert!(boolean.capabilities.type_annotation);
        assert!(boolean.capabilities.module_owner);
        assert!(boolean.capabilities.impl_target);
        assert_eq!(
            boolean.capabilities.facet_root_path,
            Some(FacetRootKind::TypeRoot)
        );
    }

    #[test]
    fn builtin_symbol_identity_info_marks_container_facet_roots() {
        let tuple = builtin_symbol_identity_info("Tuple").expect("Tuple should be known");
        assert_eq!(tuple.identity, TypeIdentity::Type);
        assert!(!tuple.capabilities.type_annotation);
        assert!(tuple.capabilities.module_owner);
        assert!(!tuple.capabilities.impl_target);
        assert_eq!(
            tuple.capabilities.facet_root_path,
            Some(FacetRootKind::Tuple)
        );

        let list = builtin_symbol_identity_info("List").expect("List should be known");
        assert_eq!(list.identity, TypeIdentity::Type);
        assert!(list.capabilities.type_annotation);
        assert!(list.capabilities.module_owner);
        assert!(list.capabilities.impl_target);
        assert_eq!(list.capabilities.facet_root_path, Some(FacetRootKind::List));

        let hash_map = builtin_symbol_identity_info("HashMap").expect("HashMap should be known");
        assert_eq!(hash_map.identity, TypeIdentity::Type);
        assert!(hash_map.capabilities.type_annotation);
        assert!(hash_map.capabilities.module_owner);
        assert!(hash_map.capabilities.impl_target);
        assert_eq!(
            hash_map.capabilities.facet_root_path,
            Some(FacetRootKind::HashMap)
        );
    }

    #[test]
    fn builtin_symbol_surface_meta_is_separate_from_runtime_aliases() {
        let string = builtin_symbol_surface_meta("String").expect("String should be known");
        assert_eq!(string.name, "String");
        assert_eq!(string.identity, TypeIdentity::Type);
        assert!(string.capabilities.module_owner);

        assert!(
            builtin_symbol_surface_meta("String::len").is_none(),
            "runtime dispatch aliases must not become symbol surface metadata"
        );
    }

    #[test]
    fn builtin_type_usage_policy_separates_annotation_and_witness_capabilities() {
        let string = builtin_type_usage_policy("String").expect("String should be known");
        assert!(string.type_annotation_allowed);
        assert!(string.signature_allowed);
        assert!(string.runtime_value_allowed);
        assert!(!string.type_ref_witness_allowed);

        let type_ref = builtin_type_usage_policy("TypeRef").expect("TypeRef should be known");
        assert!(!type_ref.type_annotation_allowed);
        assert!(!type_ref.runtime_value_allowed);
        assert!(type_ref.type_ref_witness_allowed);

        let process_init =
            builtin_type_usage_policy("ProcessInit").expect("ProcessInit should be known");
        assert!(!process_init.type_annotation_allowed);
        assert!(process_init.process_boundary_allowed);

        let match_arms = builtin_type_usage_policy("MatchArms").expect("MatchArms should be known");
        assert!(match_arms.clause_block_surface_only);

        let lazy = builtin_type_usage_policy("Lazy").expect("Lazy should be known");
        assert!(!lazy.clause_block_surface_only);
        assert!(lazy.lazy_signature_surface_only);
        assert!(!match_arms.lazy_signature_surface_only);
    }
}
