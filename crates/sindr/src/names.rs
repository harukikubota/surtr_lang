use serde::{Deserialize, Serialize};

use crate::intrinsic::IntrinsicId;

/// Reserved callable spellings whose bare infix syntax has a fixed precedence.
/// These names are restricted to their canonical declarations and standard
/// trait implementations, and cannot be used for value bindings or fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservedCallName {
    On,
    And,
    Or,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
}

impl ReservedCallName {
    pub const ALL: [Self; 9] = [
        Self::On,
        Self::And,
        Self::Or,
        Self::Eq,
        Self::Neq,
        Self::Lt,
        Self::Lte,
        Self::Gt,
        Self::Gte,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::And => "and",
            Self::Or => "or",
            Self::Eq => "eq",
            Self::Neq => "neq",
            Self::Lt => "lt",
            Self::Lte => "lte",
            Self::Gt => "gt",
            Self::Gte => "gte",
        }
    }

    pub const fn canonical_owner(self) -> &'static str {
        match self {
            Self::On => "Function",
            Self::And | Self::Or => "Kernel",
            Self::Eq | Self::Neq => "Eq",
            Self::Lt | Self::Lte | Self::Gt | Self::Gte => "Compare",
        }
    }

    pub fn canonical_name(self) -> String {
        format!("{}::{}", self.canonical_owner(), self.name())
    }

    pub const fn allows_trait_implementation(self) -> bool {
        !matches!(self, Self::On | Self::And | Self::Or)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

pub fn is_reserved_value_name(name: &str) -> bool {
    ReservedCallName::from_name(name).is_some()
        || crate::pattern::PatternConsumer::from_name(name).is_some()
        || special_enum_variant_alias_meta(name).is_some()
}

/// Internal canonical namespace used for implicit top-level definitions.
///
/// The compiler keeps this namespace in canonical identities, but user-facing
/// surfaces should hide it to keep signatures and diagnostics stable.
pub const IMPLICIT_ROOT_NAMESPACE_PREFIX: &str = "Global::";

/// Canonical identity for a compiler-generated error declared at the implicit
/// root. Callers supply known compiler/std error heads, never source expressions
/// or a runtime error kind received from user code.
pub fn compiler_global_error_kind(kind: &str) -> String {
    assert!(
        !kind.contains("::"),
        "compiler error kind must be an unqualified root head"
    );
    format!("{IMPLICIT_ROOT_NAMESPACE_PREFIX}{kind}")
}

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
pub const SYMBOL_CAPABILITY_SCHEMA_VERSION: u32 = 3;

/// Compile-space identity of a canonical declaration owner.
///
/// This is not a runtime value category or a runtime type tag. Identity-bearing
/// roots are registered once in Sigil's shared `OwnerRegistry`; members and
/// `impl` blocks refer back to their owner instead of creating another
/// identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeIdentity {
    Type,
    TypeConstructor,
    Struct,
    Record,
    Enum,
    Error,
    Mod,
    Supervisor,
    Sig,
    Const,
    Trait,
    /// Parser classification of a canonical compiler-managed Enum variant.
    /// The owner retains its ordinary Enum / type-constructor identity.
    SpecialEnumVariant,
}

/// Runtime lowering associated with an ordinary canonical Enum variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecialEnumVariantLowering {
    ResultOk,
    ResultErr,
    Boolean(bool),
}

/// Canonical special variant contract. Bare aliases refer to this declaration;
/// they never declare an additional constructor, symbol, or runtime tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecialEnumVariantMeta {
    pub owner: &'static str,
    pub qualified_name: &'static str,
    pub bare_alias: &'static str,
    pub payload_arity: usize,
    pub lowering: SpecialEnumVariantLowering,
}

impl SpecialEnumVariantMeta {
    pub const fn identity(self) -> TypeIdentity {
        TypeIdentity::SpecialEnumVariant
    }
}

pub const SPECIAL_ENUM_VARIANT_METAS: &[SpecialEnumVariantMeta] = &[
    SpecialEnumVariantMeta {
        owner: "Result",
        qualified_name: "Result::Ok",
        bare_alias: "Ok",
        payload_arity: 1,
        lowering: SpecialEnumVariantLowering::ResultOk,
    },
    SpecialEnumVariantMeta {
        owner: "Result",
        qualified_name: "Result::Err",
        bare_alias: "Err",
        payload_arity: 1,
        lowering: SpecialEnumVariantLowering::ResultErr,
    },
    SpecialEnumVariantMeta {
        owner: "Boolean",
        qualified_name: "Boolean::True",
        bare_alias: "True",
        payload_arity: 0,
        lowering: SpecialEnumVariantLowering::Boolean(true),
    },
    SpecialEnumVariantMeta {
        owner: "Boolean",
        qualified_name: "Boolean::False",
        bare_alias: "False",
        payload_arity: 0,
        lowering: SpecialEnumVariantLowering::Boolean(false),
    },
];

/// Lookup only a canonical declaration; unrelated owners sharing a variant tail
/// must never acquire special lowering or constraints.
pub fn special_enum_variant_meta(name: &str) -> Option<&'static SpecialEnumVariantMeta> {
    let name = surface_path_name(name);
    SPECIAL_ENUM_VARIANT_METAS
        .iter()
        .find(|meta| meta.qualified_name == name)
}

pub fn special_enum_variant_alias_meta(name: &str) -> Option<&'static SpecialEnumVariantMeta> {
    SPECIAL_ENUM_VARIANT_METAS
        .iter()
        .find(|meta| meta.bare_alias == name)
}

/// Syntax classification before normalization to an ordinary Enum reference.
pub fn special_enum_variant_surface_meta(name: &str) -> Option<&'static SpecialEnumVariantMeta> {
    special_enum_variant_meta(name).or_else(|| special_enum_variant_alias_meta(name))
}

/// Compile-space root kind used when a symbol can serve as a Facet path root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FacetRootKind {
    TypeRoot,
    Tuple,
    List,
    HashMap,
}

/// Closed policy for using a nominal constructor as a capture target.
///
/// `None` on `SymbolCapabilities` means that the symbol is not a constructor
/// target.  The policy is deliberately carried by canonical identity rather
/// than inferred from a display name in later phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstructorCapturePolicy {
    Ordinary,
    CompilerManaged,
    Forbidden,
}

/// Permission to declare a trait implementation for a compiler-known type.
/// This is distinct from permission to declare inherent methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraitImplPolicy {
    Open,
    CompilerOwned,
    Forbidden,
}

/// Compile-space capability flags attached to a resolved symbol identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolCapabilities {
    pub type_annotation: bool,
    pub module_owner: bool,
    pub impl_target: bool,
    pub facet_root_path: Option<FacetRootKind>,
    pub constructor_capture: Option<ConstructorCapturePolicy>,
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
            constructor_capture: None,
        }
    }

    pub const fn with_constructor_capture(mut self, policy: ConstructorCapturePolicy) -> Self {
        self.constructor_capture = Some(policy);
        self
    }

    pub const fn with_constructor_capture_policy(
        mut self,
        policy: Option<ConstructorCapturePolicy>,
    ) -> Self {
        self.constructor_capture = policy;
        self
    }

    pub const fn type_owner() -> Self {
        Self::new(true, true, true, None)
    }

    pub const fn module_owner() -> Self {
        Self::new(false, true, false, None)
    }

    pub const fn supervisor_owner() -> Self {
        Self::new(false, true, false, None)
    }

    pub const fn signature_owner() -> Self {
        Self::new(true, false, false, None)
    }

    pub const fn const_owner() -> Self {
        Self::new(false, false, false, None)
    }

    pub const fn trait_owner() -> Self {
        Self::new(false, true, false, None)
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

/// Reason why a surface name cannot be used as a declaration owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReservedOwnerSurfaceNameKind {
    CanonicalBuiltinType,
    BuiltinSpecialEnumVariantAlias,
}

impl ReservedOwnerSurfaceNameKind {
    pub const fn diagnostic_suffix(self) -> &'static str {
        match self {
            Self::CanonicalBuiltinType => "reserved by a canonical builtin type declaration",
            Self::BuiltinSpecialEnumVariantAlias => "reserved for canonical enum variant aliases",
        }
    }
}

/// Shared uniqueness/reservation constraint for names that would become owners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReservedOwnerSurfaceNameConstraint {
    pub surface_name: &'static str,
    pub kind: ReservedOwnerSurfaceNameKind,
}

/// Closed usage class for a builtin type head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuiltinTypeUsage {
    General,
    CompilerSurfaceOnly,
    ClauseBlockSurfaceOnly,
    LazySignatureSurfaceOnly,
    IntrinsicSignatureOnly(IntrinsicId),
    /// MatchResult is confined to an Extractor's result and return paths.
    ExtractorResultOnly,
}

/// Compile-space usage policy for builtin type heads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuiltinTypeUsagePolicy {
    pub usage: BuiltinTypeUsage,
    pub type_annotation_allowed: bool,
    pub signature_allowed: bool,
    pub runtime_value_allowed: bool,
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
        process_boundary_allowed: bool,
        facet_value_forbidden_in_stage1: bool,
        clause_block_surface_only: bool,
        lazy_signature_surface_only: bool,
    ) -> Self {
        Self::new_with_usage(
            BuiltinTypeUsage::General,
            type_annotation_allowed,
            signature_allowed,
            runtime_value_allowed,
            process_boundary_allowed,
            facet_value_forbidden_in_stage1,
            clause_block_surface_only,
            lazy_signature_surface_only,
        )
    }

    const fn new_with_usage(
        usage: BuiltinTypeUsage,
        type_annotation_allowed: bool,
        signature_allowed: bool,
        runtime_value_allowed: bool,
        process_boundary_allowed: bool,
        facet_value_forbidden_in_stage1: bool,
        clause_block_surface_only: bool,
        lazy_signature_surface_only: bool,
    ) -> Self {
        Self {
            usage,
            type_annotation_allowed,
            signature_allowed,
            runtime_value_allowed,
            process_boundary_allowed,
            facet_value_forbidden_in_stage1,
            clause_block_surface_only,
            lazy_signature_surface_only,
        }
    }

    pub const fn ordinary_runtime_type() -> Self {
        Self::new(true, true, true, true, false, false, false)
    }

    pub const fn compiler_surface_only() -> Self {
        Self::new_with_usage(
            BuiltinTypeUsage::CompilerSurfaceOnly,
            false,
            false,
            false,
            false,
            true,
            false,
            false,
        )
    }

    pub const fn clause_block_surface_only() -> Self {
        Self::new_with_usage(
            BuiltinTypeUsage::ClauseBlockSurfaceOnly,
            false,
            false,
            false,
            false,
            true,
            true,
            false,
        )
    }

    pub const fn lazy_signature_surface_only() -> Self {
        Self::new_with_usage(
            BuiltinTypeUsage::LazySignatureSurfaceOnly,
            false,
            false,
            false,
            false,
            true,
            false,
            true,
        )
    }

    pub const fn intrinsic_signature_only(intrinsic: IntrinsicId) -> Self {
        Self::new_with_usage(
            BuiltinTypeUsage::IntrinsicSignatureOnly(intrinsic),
            false,
            false,
            false,
            false,
            true,
            false,
            false,
        )
    }

    pub const fn extractor_result_only() -> Self {
        Self::new_with_usage(
            BuiltinTypeUsage::ExtractorResultOnly,
            false,
            false,
            false,
            false,
            true,
            false,
            false,
        )
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
    StandbyInit,
    Lazy,
    Hole,
    Facet,
    Pid,
    FileHandle,
    Workers,
    WorkerLease,
    TaskHandle,
    // Keep new variants appended so bincode discriminants of existing canonical
    // type identities remain stable across semantic snapshot revisions.
    DoBlock,
    MatchResult,
    ExtractorClosure,
    ErrorKind,
    InfiniteGenerator,
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
            Self::DoBlock => "DoBlock",
            Self::MatchResult => "MatchResult",
            Self::ExtractorClosure => "ExtractorClosure",
            Self::BulkUpdateEntries => "BulkUpdateEntries",
            Self::Error => "Error",
            Self::ErrorKind => "ErrorKind",
            Self::Regex => "Regex",
            Self::RegexCaptures => "RegexCaptures",
            Self::RegexMatch => "RegexMatch",
            Self::RandomGenerator => "RandomGenerator",
            Self::List => "List",
            Self::HashMap => "HashMap",
            Self::Generator => "Generator",
            Self::InfiniteGenerator => "InfiniteGenerator",
            Self::Result => "Result",
            Self::Duration => "Duration",
            Self::StandbyInit => "StandbyInit",
            Self::Lazy => "Lazy",
            Self::Hole => "Hole",
            Self::Facet => "Facet",
            Self::Pid => "PID",
            Self::FileHandle => "FileHandle",
            Self::Workers => "Workers",
            Self::WorkerLease => "WorkerLease",
            Self::TaskHandle => "TaskHandle",
        }
    }

    pub const fn supports_inherent_impl(self) -> bool {
        !matches!(
            self,
            Self::Hole
                | Self::Closure
                | Self::MatchArms
                | Self::CondClauses
                | Self::DoBlock
                | Self::MatchResult
                | Self::ExtractorClosure
                | Self::BulkUpdateEntries
                | Self::StandbyInit
                | Self::Lazy
                | Self::ErrorKind
                | Self::Pid
                | Self::FileHandle
        )
    }

    pub const fn trait_impl_policy(self) -> TraitImplPolicy {
        match self {
            Self::Int
            | Self::Float
            | Self::String
            | Self::Boolean
            | Self::Unit
            | Self::List
            | Self::HashMap
            | Self::Result
            | Self::Duration => TraitImplPolicy::Open,
            Self::Pid => TraitImplPolicy::CompilerOwned,
            Self::Closure
            | Self::MatchArms
            | Self::CondClauses
            | Self::DoBlock
            | Self::MatchResult
            | Self::ExtractorClosure
            | Self::BulkUpdateEntries
            | Self::Error
            | Self::Regex
            | Self::RegexCaptures
            | Self::RegexMatch
            | Self::RandomGenerator
            | Self::Generator
            | Self::InfiniteGenerator
            | Self::StandbyInit
            | Self::Lazy
            | Self::ErrorKind
            | Self::Hole
            | Self::Facet
            | Self::FileHandle
            | Self::Workers
            | Self::WorkerLease
            | Self::TaskHandle => TraitImplPolicy::Forbidden,
        }
    }

    pub const fn usage_policy(self) -> BuiltinTypeUsagePolicy {
        match self {
            Self::StandbyInit => {
                BuiltinTypeUsagePolicy::new(false, false, false, true, false, false, false)
            }
            Self::Lazy => BuiltinTypeUsagePolicy::lazy_signature_surface_only(),
            Self::Hole | Self::Closure => BuiltinTypeUsagePolicy::compiler_surface_only(),
            Self::MatchArms | Self::CondClauses | Self::BulkUpdateEntries => {
                BuiltinTypeUsagePolicy::clause_block_surface_only()
            }
            Self::DoBlock => BuiltinTypeUsagePolicy::intrinsic_signature_only(IntrinsicId::Do),
            Self::MatchResult => BuiltinTypeUsagePolicy::extractor_result_only(),
            Self::Facet => BuiltinTypeUsagePolicy::new(true, true, true, true, true, false, false),
            Self::Pid | Self::Workers | Self::WorkerLease | Self::TaskHandle => {
                BuiltinTypeUsagePolicy::new(true, true, true, true, false, false, false)
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
        "DoBlock" => Some(TypeName::DoBlock),
        "MatchResult" => Some(TypeName::MatchResult),
        "ExtractorClosure" => Some(TypeName::ExtractorClosure),
        "BulkUpdateEntries" => Some(TypeName::BulkUpdateEntries),
        "Error" => Some(TypeName::Error),
        "ErrorKind" => Some(TypeName::ErrorKind),
        "Regex" => Some(TypeName::Regex),
        "RegexCaptures" => Some(TypeName::RegexCaptures),
        "RegexMatch" => Some(TypeName::RegexMatch),
        "RandomGenerator" => Some(TypeName::RandomGenerator),
        "List" => Some(TypeName::List),
        "HashMap" => Some(TypeName::HashMap),
        "Generator" => Some(TypeName::Generator),
        "InfiniteGenerator" => Some(TypeName::InfiniteGenerator),
        "Result" => Some(TypeName::Result),
        "Duration" => Some(TypeName::Duration),
        "StandbyInit" => Some(TypeName::StandbyInit),
        "Lazy" => Some(TypeName::Lazy),
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

pub const fn canonical_builtin_type_has_surface_declaration(type_name: TypeName) -> bool {
    matches!(
        type_name,
        TypeName::Int
            | TypeName::Float
            | TypeName::String
            | TypeName::Boolean
            | TypeName::Unit
            | TypeName::Closure
            | TypeName::MatchArms
            | TypeName::CondClauses
            | TypeName::DoBlock
            | TypeName::MatchResult
            | TypeName::ExtractorClosure
            | TypeName::BulkUpdateEntries
            | TypeName::Error
            | TypeName::ErrorKind
            | TypeName::Regex
            | TypeName::RegexCaptures
            | TypeName::RegexMatch
            | TypeName::RandomGenerator
            | TypeName::FileHandle
            | TypeName::List
            | TypeName::HashMap
            | TypeName::Generator
            | TypeName::InfiniteGenerator
            | TypeName::Result
            | TypeName::StandbyInit
            | TypeName::Lazy
            | TypeName::Hole
            | TypeName::Facet
            | TypeName::Workers
            | TypeName::WorkerLease
            | TypeName::TaskHandle
    )
}

fn owner_surface_tail(name: &str) -> &str {
    surface_path_name(name)
        .rsplit("::")
        .next()
        .unwrap_or_else(|| surface_path_name(name))
}

pub fn reserved_owner_surface_name_constraint(
    name: &str,
) -> Option<ReservedOwnerSurfaceNameConstraint> {
    let surface_name = owner_surface_tail(name);
    if let Some(meta) = special_enum_variant_alias_meta(surface_name) {
        return Some(ReservedOwnerSurfaceNameConstraint {
            surface_name: meta.bare_alias,
            kind: ReservedOwnerSurfaceNameKind::BuiltinSpecialEnumVariantAlias,
        });
    }

    let type_name = builtin_type_name(surface_name)?;
    if surface_name != TypeName::StandbyInit.as_str()
        && canonical_builtin_type_has_surface_declaration(type_name)
    {
        return Some(ReservedOwnerSurfaceNameConstraint {
            surface_name: type_name.as_str(),
            kind: ReservedOwnerSurfaceNameKind::CanonicalBuiltinType,
        });
    }

    None
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
    let identity = crate::builtin::builtin_type_meta_by_name(type_name.as_str())
        .map(|meta| meta.identity)
        .unwrap_or(TypeIdentity::Type);
    let facet_root_path = match type_name {
        TypeName::Boolean | TypeName::Error => Some(FacetRootKind::TypeRoot),
        TypeName::List => Some(FacetRootKind::List),
        TypeName::HashMap => Some(FacetRootKind::HashMap),
        _ => None,
    };
    let impl_target = type_name.supports_inherent_impl();
    let constructor_capture = match type_name {
        TypeName::Result | TypeName::Boolean => Some(ConstructorCapturePolicy::CompilerManaged),
        TypeName::MatchResult | TypeName::Error => Some(ConstructorCapturePolicy::Forbidden),
        _ => None,
    };
    Some(BuiltinSymbolSurfaceMeta {
        name: type_name.as_str(),
        identity,
        capabilities: SymbolCapabilities::new(
            type_name != TypeName::MatchResult,
            impl_target,
            impl_target,
            facet_root_path,
        )
        .with_constructor_capture_policy(constructor_capture),
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
    fn special_enum_registry_classifies_only_canonical_variants_and_bare_aliases() {
        for meta in SPECIAL_ENUM_VARIANT_METAS {
            assert_eq!(meta.identity(), TypeIdentity::SpecialEnumVariant);
            assert_eq!(special_enum_variant_meta(meta.qualified_name), Some(meta));
            assert_eq!(special_enum_variant_alias_meta(meta.bare_alias), Some(meta));
            assert_eq!(
                special_enum_variant_surface_meta(meta.bare_alias),
                Some(meta)
            );
            assert_eq!(
                special_enum_variant_meta(&format!("Global::{}", meta.qualified_name)),
                Some(meta)
            );
            assert!(is_reserved_value_name(meta.bare_alias));
            assert_eq!(special_enum_variant_meta(meta.bare_alias), None);
        }
        for name in [
            "Other::Ok",
            "Other::Err",
            "MatchResult::Ok",
            "MatchResult::Err",
            "Other::True",
            "Nested::Result::Ok",
        ] {
            assert_eq!(special_enum_variant_surface_meta(name), None);
            assert!(!is_reserved_value_name(name));
        }
        assert_eq!(SPECIAL_ENUM_VARIANT_METAS.len(), 4);
        assert_eq!(
            special_enum_variant_meta("Result::Ok")
                .unwrap()
                .payload_arity,
            1
        );
        assert_eq!(
            special_enum_variant_meta("Boolean::False")
                .unwrap()
                .payload_arity,
            0
        );
        assert_eq!(
            special_enum_variant_meta("Boolean::True").unwrap().lowering,
            SpecialEnumVariantLowering::Boolean(true)
        );
        assert_eq!(
            builtin_symbol_identity_info("Result").unwrap().identity,
            TypeIdentity::TypeConstructor
        );
        assert_eq!(
            builtin_symbol_identity_info("Boolean").unwrap().identity,
            TypeIdentity::Enum
        );
    }

    #[test]
    fn match_result_is_reserved_without_general_value_capabilities() {
        let ty = builtin_type_name("MatchResult").expect("MatchResult has canonical identity");
        assert!(canonical_builtin_type_has_surface_declaration(ty));
        assert!(!ty.supports_inherent_impl());
        assert!(!ty.usage_policy().type_annotation_allowed);
        assert!(!ty.usage_policy().runtime_value_allowed);
        assert!(!ty.usage_policy().process_boundary_allowed);
        assert!(reserved_owner_surface_name_constraint("MatchResult").is_some());
        let info = builtin_symbol_identity_info("MatchResult").unwrap();
        assert!(!info.capabilities.type_annotation);
        assert!(!info.capabilities.impl_target);
    }

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
        let cases = [
            ("Int", TypeIdentity::Type),
            ("Error", TypeIdentity::Type),
            ("List", TypeIdentity::TypeConstructor),
            ("HashMap", TypeIdentity::TypeConstructor),
            ("Result", TypeIdentity::TypeConstructor),
        ];

        for (name, identity) in cases {
            assert_eq!(
                builtin_symbol_identity_info(name).unwrap().identity,
                identity
            );
        }

        let list = builtin_symbol_identity_info("List").expect("List should be known");
        assert!(list.capabilities.type_annotation);
        assert!(list.capabilities.impl_target);
        assert_eq!(list.capabilities.facet_root_path, Some(FacetRootKind::List));

        let hash_map = builtin_symbol_identity_info("HashMap").expect("HashMap should be known");
        assert!(hash_map.capabilities.type_annotation);
        assert!(hash_map.capabilities.impl_target);
        assert_eq!(
            hash_map.capabilities.facet_root_path,
            Some(FacetRootKind::HashMap)
        );

        let result = builtin_symbol_identity_info("Result").expect("Result should be known");
        assert!(result.capabilities.type_annotation);
        assert!(result.capabilities.impl_target);
        assert_eq!(result.capabilities.facet_root_path, None);
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
        assert_eq!(list.identity, TypeIdentity::TypeConstructor);
        assert!(list.capabilities.type_annotation);
        assert!(list.capabilities.module_owner);
        assert!(list.capabilities.impl_target);
        assert_eq!(list.capabilities.facet_root_path, Some(FacetRootKind::List));

        let hash_map = builtin_symbol_identity_info("HashMap").expect("HashMap should be known");
        assert_eq!(hash_map.identity, TypeIdentity::TypeConstructor);
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
    fn reserved_owner_surface_name_constraint_marks_builtin_special_variant_aliases() {
        for name in ["Ok", "Err", "True", "False"] {
            let constraint = reserved_owner_surface_name_constraint(name)
                .expect("builtin-special variant alias should be reserved as owner");
            assert_eq!(constraint.surface_name, name);
            assert_eq!(
                constraint.kind,
                ReservedOwnerSurfaceNameKind::BuiltinSpecialEnumVariantAlias
            );
        }

        let nested = reserved_owner_surface_name_constraint("Auth::Ok")
            .expect("owner tail should be reserved inside qualified paths");
        assert_eq!(nested.surface_name, "Ok");
    }

    #[test]
    fn reserved_owner_surface_name_constraint_marks_canonical_builtin_types() {
        let constraint = reserved_owner_surface_name_constraint("MatchArms")
            .expect("canonical builtin type surface should be reserved as owner");
        assert_eq!(constraint.surface_name, "MatchArms");
        assert_eq!(
            constraint.kind,
            ReservedOwnerSurfaceNameKind::CanonicalBuiltinType
        );

        assert!(
            reserved_owner_surface_name_constraint("StandbyInit").is_none(),
            "StandbyInit owner remains allowed for runtime init lowering"
        );
    }

    #[test]
    fn builtin_type_usage_policy_separates_annotation_and_special_capabilities() {
        let string = builtin_type_usage_policy("String").expect("String should be known");
        assert!(string.type_annotation_allowed);
        assert!(string.signature_allowed);
        assert!(string.runtime_value_allowed);

        let process_init =
            builtin_type_usage_policy("StandbyInit").expect("StandbyInit should be known");
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
