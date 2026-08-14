use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sigil::resolved::*;
use sindr::builtin::{
    builtin_function_metas, builtin_type_head_metas, builtin_type_meta_by_name, builtin_uid,
    BuiltinMeta,
};
use sindr::names::builtin_type_usage_policy;
use sindr::policy::{ExitCodePolicy, RuntimeSourcePolicy};
use sindr::warning::{
    CompilerWarning, PhaseOutput, WarningBuffer, WarningKind, WarningPhase, WarningSpan,
};
use spire::ast::{AstTy, BinOp, Lit, Span};

use crate::env::{TypeEnv, TypeKind};
use crate::error::TypeError;
use crate::typed::*;
use crate::types::Ty;

mod definitions;
mod expr;
mod matching;
mod patterns;
mod predeclare;
mod specialize;
mod types;

#[derive(Debug, Clone, Copy)]
enum ProfileEvent {
    TypesCompatible,
    BindTyVar,
    InstantiateTyWithFresh,
    InstantiateEnumVariant,
    MatchExhaustive,
    EnumVariantCtorLookup,
    EnumVariantsLookup,
    EnumVariantSelectorLookup,
    TraitDispatchLookup,
    GenericTraitCandidateScan,
    OperatorTraitCandidateScan,
    ChildCheckerSpawn,
    ChildCheckerAbsorb,
    ResolveTypedNode,
    CheckBodyIsolated,
    MatchArm,
    ClosureBody,
    NormalizeEnvBindings,
}

#[cfg(test)]
mod process_boundary_policy_tests {
    use super::*;
    use sindr::policy::{CompileUnitKind, ExitCodePolicy, SourceKind};

    #[test]
    fn process_boundary_only_type_query_uses_builtin_usage_policy() {
        assert!(Checker::builtin_type_is_process_boundary_only(
            "StandbyInit"
        ));
        assert!(Checker::builtin_type_is_process_boundary_only(
            "Global::StandbyInit"
        ));
        assert!(!Checker::builtin_type_is_process_boundary_only("PID"));
        assert!(!Checker::builtin_type_is_process_boundary_only("String"));
    }

    #[test]
    fn typecheck_context_can_be_derived_from_source_policy() {
        let source_policy = SourceKind::DefinitionSource.policy(CompileUnitKind::Project, None);
        let context = TypecheckContext::from_source_policy(source_policy);

        assert_eq!(
            context.runtime_policy.exit_code_policy,
            ExitCodePolicy::EntryOnly
        );
        assert!(!context.enforce_builtin_type_contracts);
        assert!(!context.allow_error_function_params);
    }
}

#[derive(Default)]
struct ProfileCounter {
    calls: AtomicU64,
    nanos: AtomicU64,
}

impl ProfileCounter {
    fn reset(&self) {
        self.calls.store(0, Ordering::Relaxed);
        self.nanos.store(0, Ordering::Relaxed);
    }

    fn add(&self, elapsed: Duration) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.nanos
            .fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
    }

    fn snapshot(&self) -> (u64, u64) {
        (
            self.calls.load(Ordering::Relaxed),
            self.nanos.load(Ordering::Relaxed),
        )
    }
}

#[derive(Default)]
struct ProfileData {
    types_compatible: ProfileCounter,
    bind_tyvar: ProfileCounter,
    instantiate_ty_with_fresh: ProfileCounter,
    instantiate_enum_variant: ProfileCounter,
    match_exhaustive: ProfileCounter,
    enum_variant_ctor_lookup: ProfileCounter,
    enum_variants_lookup: ProfileCounter,
    enum_variant_selector_lookup: ProfileCounter,
    trait_dispatch_lookup: ProfileCounter,
    generic_trait_candidate_scan: ProfileCounter,
    operator_trait_candidate_scan: ProfileCounter,
    child_checker_spawn: ProfileCounter,
    child_checker_absorb: ProfileCounter,
    resolve_typed_node: ProfileCounter,
    check_body_isolated: ProfileCounter,
    match_arm: ProfileCounter,
    closure_body: ProfileCounter,
    normalize_env_bindings: ProfileCounter,
}

#[derive(Debug, Clone, Copy)]
struct ProfileSnapshot {
    types_compatible_calls: u64,
    types_compatible_nanos: u64,
    bind_tyvar_calls: u64,
    bind_tyvar_nanos: u64,
    instantiate_ty_with_fresh_calls: u64,
    instantiate_ty_with_fresh_nanos: u64,
    instantiate_enum_variant_calls: u64,
    instantiate_enum_variant_nanos: u64,
    match_exhaustive_calls: u64,
    match_exhaustive_nanos: u64,
    enum_variant_ctor_lookup_calls: u64,
    enum_variant_ctor_lookup_nanos: u64,
    enum_variants_lookup_calls: u64,
    enum_variants_lookup_nanos: u64,
    enum_variant_selector_lookup_calls: u64,
    enum_variant_selector_lookup_nanos: u64,
    trait_dispatch_lookup_calls: u64,
    trait_dispatch_lookup_nanos: u64,
    generic_trait_candidate_scan_calls: u64,
    generic_trait_candidate_scan_nanos: u64,
    operator_trait_candidate_scan_calls: u64,
    operator_trait_candidate_scan_nanos: u64,
    child_checker_spawn_calls: u64,
    child_checker_spawn_nanos: u64,
    child_checker_absorb_calls: u64,
    child_checker_absorb_nanos: u64,
    resolve_typed_node_calls: u64,
    resolve_typed_node_nanos: u64,
    check_body_isolated_calls: u64,
    check_body_isolated_nanos: u64,
    match_arm_calls: u64,
    match_arm_nanos: u64,
    closure_body_calls: u64,
    closure_body_nanos: u64,
    normalize_env_bindings_calls: u64,
    normalize_env_bindings_nanos: u64,
}

#[derive(Clone)]
struct TypecheckProfiler {
    enabled: bool,
    data: Arc<ProfileData>,
}

impl TypecheckProfiler {
    fn new_from_env() -> Self {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        let enabled = *ENABLED.get_or_init(|| {
            matches!(
                std::env::var("SURTR_SCAR_PROFILE").as_deref(),
                Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
            )
        });
        Self {
            enabled,
            data: Arc::new(ProfileData::default()),
        }
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn start(&self) -> Option<Instant> {
        self.enabled.then(Instant::now)
    }

    fn finish(&self, event: ProfileEvent, start: Option<Instant>) {
        let Some(start) = start else {
            return;
        };
        let elapsed = start.elapsed();
        match event {
            ProfileEvent::TypesCompatible => self.data.types_compatible.add(elapsed),
            ProfileEvent::BindTyVar => self.data.bind_tyvar.add(elapsed),
            ProfileEvent::InstantiateTyWithFresh => {
                self.data.instantiate_ty_with_fresh.add(elapsed)
            }
            ProfileEvent::InstantiateEnumVariant => self.data.instantiate_enum_variant.add(elapsed),
            ProfileEvent::MatchExhaustive => self.data.match_exhaustive.add(elapsed),
            ProfileEvent::EnumVariantCtorLookup => self.data.enum_variant_ctor_lookup.add(elapsed),
            ProfileEvent::EnumVariantsLookup => self.data.enum_variants_lookup.add(elapsed),
            ProfileEvent::EnumVariantSelectorLookup => {
                self.data.enum_variant_selector_lookup.add(elapsed)
            }
            ProfileEvent::TraitDispatchLookup => self.data.trait_dispatch_lookup.add(elapsed),
            ProfileEvent::GenericTraitCandidateScan => {
                self.data.generic_trait_candidate_scan.add(elapsed)
            }
            ProfileEvent::OperatorTraitCandidateScan => {
                self.data.operator_trait_candidate_scan.add(elapsed)
            }
            ProfileEvent::ChildCheckerSpawn => self.data.child_checker_spawn.add(elapsed),
            ProfileEvent::ChildCheckerAbsorb => self.data.child_checker_absorb.add(elapsed),
            ProfileEvent::ResolveTypedNode => self.data.resolve_typed_node.add(elapsed),
            ProfileEvent::CheckBodyIsolated => self.data.check_body_isolated.add(elapsed),
            ProfileEvent::MatchArm => self.data.match_arm.add(elapsed),
            ProfileEvent::ClosureBody => self.data.closure_body.add(elapsed),
            ProfileEvent::NormalizeEnvBindings => self.data.normalize_env_bindings.add(elapsed),
        }
    }

    fn reset(&self) {
        if !self.enabled {
            return;
        }
        self.data.types_compatible.reset();
        self.data.bind_tyvar.reset();
        self.data.instantiate_ty_with_fresh.reset();
        self.data.instantiate_enum_variant.reset();
        self.data.match_exhaustive.reset();
        self.data.enum_variant_ctor_lookup.reset();
        self.data.enum_variants_lookup.reset();
        self.data.enum_variant_selector_lookup.reset();
        self.data.trait_dispatch_lookup.reset();
        self.data.generic_trait_candidate_scan.reset();
        self.data.operator_trait_candidate_scan.reset();
        self.data.child_checker_spawn.reset();
        self.data.child_checker_absorb.reset();
        self.data.resolve_typed_node.reset();
        self.data.check_body_isolated.reset();
        self.data.match_arm.reset();
        self.data.closure_body.reset();
        self.data.normalize_env_bindings.reset();
    }

    fn snapshot(&self) -> ProfileSnapshot {
        let (types_compatible_calls, types_compatible_nanos) =
            self.data.types_compatible.snapshot();
        let (bind_tyvar_calls, bind_tyvar_nanos) = self.data.bind_tyvar.snapshot();
        let (instantiate_ty_with_fresh_calls, instantiate_ty_with_fresh_nanos) =
            self.data.instantiate_ty_with_fresh.snapshot();
        let (instantiate_enum_variant_calls, instantiate_enum_variant_nanos) =
            self.data.instantiate_enum_variant.snapshot();
        let (match_exhaustive_calls, match_exhaustive_nanos) =
            self.data.match_exhaustive.snapshot();
        let (enum_variant_ctor_lookup_calls, enum_variant_ctor_lookup_nanos) =
            self.data.enum_variant_ctor_lookup.snapshot();
        let (enum_variants_lookup_calls, enum_variants_lookup_nanos) =
            self.data.enum_variants_lookup.snapshot();
        let (enum_variant_selector_lookup_calls, enum_variant_selector_lookup_nanos) =
            self.data.enum_variant_selector_lookup.snapshot();
        let (trait_dispatch_lookup_calls, trait_dispatch_lookup_nanos) =
            self.data.trait_dispatch_lookup.snapshot();
        let (generic_trait_candidate_scan_calls, generic_trait_candidate_scan_nanos) =
            self.data.generic_trait_candidate_scan.snapshot();
        let (operator_trait_candidate_scan_calls, operator_trait_candidate_scan_nanos) =
            self.data.operator_trait_candidate_scan.snapshot();
        let (child_checker_spawn_calls, child_checker_spawn_nanos) =
            self.data.child_checker_spawn.snapshot();
        let (child_checker_absorb_calls, child_checker_absorb_nanos) =
            self.data.child_checker_absorb.snapshot();
        let (resolve_typed_node_calls, resolve_typed_node_nanos) =
            self.data.resolve_typed_node.snapshot();
        let (check_body_isolated_calls, check_body_isolated_nanos) =
            self.data.check_body_isolated.snapshot();
        let (match_arm_calls, match_arm_nanos) = self.data.match_arm.snapshot();
        let (closure_body_calls, closure_body_nanos) = self.data.closure_body.snapshot();
        let (normalize_env_bindings_calls, normalize_env_bindings_nanos) =
            self.data.normalize_env_bindings.snapshot();
        ProfileSnapshot {
            types_compatible_calls,
            types_compatible_nanos,
            bind_tyvar_calls,
            bind_tyvar_nanos,
            instantiate_ty_with_fresh_calls,
            instantiate_ty_with_fresh_nanos,
            instantiate_enum_variant_calls,
            instantiate_enum_variant_nanos,
            match_exhaustive_calls,
            match_exhaustive_nanos,
            enum_variant_ctor_lookup_calls,
            enum_variant_ctor_lookup_nanos,
            enum_variants_lookup_calls,
            enum_variants_lookup_nanos,
            enum_variant_selector_lookup_calls,
            enum_variant_selector_lookup_nanos,
            trait_dispatch_lookup_calls,
            trait_dispatch_lookup_nanos,
            generic_trait_candidate_scan_calls,
            generic_trait_candidate_scan_nanos,
            operator_trait_candidate_scan_calls,
            operator_trait_candidate_scan_nanos,
            child_checker_spawn_calls,
            child_checker_spawn_nanos,
            child_checker_absorb_calls,
            child_checker_absorb_nanos,
            resolve_typed_node_calls,
            resolve_typed_node_nanos,
            check_body_isolated_calls,
            check_body_isolated_nanos,
            match_arm_calls,
            match_arm_nanos,
            closure_body_calls,
            closure_body_nanos,
            normalize_env_bindings_calls,
            normalize_env_bindings_nanos,
        }
    }

    fn print_summary(&self, total: Duration) {
        if !self.enabled {
            return;
        }
        if total < Duration::from_millis(5) {
            return;
        }
        let snap = self.snapshot();
        eprintln!(
            "scar-profile total={:.3}ms | types_compatible={} ({:.3}ms) | bind_tyvar={} ({:.3}ms) | instantiate_ty_with_fresh={} ({:.3}ms) | instantiate_enum_variant={} ({:.3}ms) | match_exhaustive={} ({:.3}ms)",
            total.as_secs_f64() * 1000.0,
            snap.types_compatible_calls,
            snap.types_compatible_nanos as f64 / 1_000_000.0,
            snap.bind_tyvar_calls,
            snap.bind_tyvar_nanos as f64 / 1_000_000.0,
            snap.instantiate_ty_with_fresh_calls,
            snap.instantiate_ty_with_fresh_nanos as f64 / 1_000_000.0,
            snap.instantiate_enum_variant_calls,
            snap.instantiate_enum_variant_nanos as f64 / 1_000_000.0,
            snap.match_exhaustive_calls,
            snap.match_exhaustive_nanos as f64 / 1_000_000.0,
        );
        eprintln!(
            "scar-profile enum_lookup ctor={} ({:.3}ms) | variants={} ({:.3}ms) | selector={} ({:.3}ms)",
            snap.enum_variant_ctor_lookup_calls,
            snap.enum_variant_ctor_lookup_nanos as f64 / 1_000_000.0,
            snap.enum_variants_lookup_calls,
            snap.enum_variants_lookup_nanos as f64 / 1_000_000.0,
            snap.enum_variant_selector_lookup_calls,
            snap.enum_variant_selector_lookup_nanos as f64 / 1_000_000.0,
        );
        eprintln!(
            "scar-profile dispatch lookup={} ({:.3}ms) | generic_scan={} ({:.3}ms) | operator_scan={} ({:.3}ms) | child_spawn={} ({:.3}ms) | child_absorb={} ({:.3}ms)",
            snap.trait_dispatch_lookup_calls,
            snap.trait_dispatch_lookup_nanos as f64 / 1_000_000.0,
            snap.generic_trait_candidate_scan_calls,
            snap.generic_trait_candidate_scan_nanos as f64 / 1_000_000.0,
            snap.operator_trait_candidate_scan_calls,
            snap.operator_trait_candidate_scan_nanos as f64 / 1_000_000.0,
            snap.child_checker_spawn_calls,
            snap.child_checker_spawn_nanos as f64 / 1_000_000.0,
            snap.child_checker_absorb_calls,
            snap.child_checker_absorb_nanos as f64 / 1_000_000.0,
        );
        eprintln!(
            "scar-profile normalize resolve_typed_node={} ({:.3}ms) | env_bindings={} ({:.3}ms) | isolated_body={} ({:.3}ms) | match_arm={} ({:.3}ms) | closure_body={} ({:.3}ms)",
            snap.resolve_typed_node_calls,
            snap.resolve_typed_node_nanos as f64 / 1_000_000.0,
            snap.normalize_env_bindings_calls,
            snap.normalize_env_bindings_nanos as f64 / 1_000_000.0,
            snap.check_body_isolated_calls,
            snap.check_body_isolated_nanos as f64 / 1_000_000.0,
            snap.match_arm_calls,
            snap.match_arm_nanos as f64 / 1_000_000.0,
            snap.closure_body_calls,
            snap.closure_body_nanos as f64 / 1_000_000.0,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeSyntaxContext {
    General,
    BindingAnnotation,
    FunctionReturn,
    HoleClosureParam,
    FacetDeferredSlot,
    ExtractorReturn,
    ExtractorBody,
    ErrorMarker,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraitMethodInfo {
    id: ResolvedId,
    fun_params: Vec<AstTy>,
    type_params: Vec<ResolvedTypeParam>,
    params: Vec<ResolvedFunParam>,
    ret_ty: AstTy,
    where_clause: Option<TypedWhereClause>,
    attrs: ResolvedDeclAttrs,
    body: Option<Box<Resolved>>,
    span: Span,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraitInfo {
    id: ResolvedId,
    type_params: Vec<ResolvedTypeParam>,
    where_clause: Option<TypedWhereClause>,
    constructor_slots: Vec<String>,
    parents: Vec<ResolvedId>,
    methods: HashMap<String, TraitMethodInfo>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraitImplMethodInfo {
    method_name: String,
    function_id: ResolvedId,
    fun_params: Vec<AstTy>,
    type_params: Vec<ResolvedTypeParam>,
    params: Vec<ResolvedFunParam>,
    ret_ty: Option<AstTy>,
    where_clause: Option<TypedWhereClause>,
    body: Box<Resolved>,
    attrs: ResolvedDeclAttrs,
    span: Span,
    display_name_override: Option<String>,
    dispatch_override: Option<TraitDispatchTarget>,
    is_builtin: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraitImplInfo {
    trait_id: ResolvedId,
    trait_args: Vec<AstTy>,
    trait_arg_tys: Vec<Ty>,
    target_name: String,
    target_ast_ty: AstTy,
    target_ty: Ty,
    where_clause: Option<TypedWhereClause>,
    type_param_vars: Vec<u32>,
    constructor_slot_vars: Vec<u32>,
    constructor_slot_positions: Vec<usize>,
    methods: HashMap<String, TraitImplMethodInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SignatureAliasInfo {
    params: Vec<ResolvedTypeParam>,
    rhs: AstTy,
    span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ConstKind {
    PrimitiveLiteral,
    FacetPath,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum StoredConstValue {
    Literal(Lit),
    FacetPath(TypedFacetPath),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum StoredFacetPath {
    Concrete(TypedFacetPath),
    Pending(PendingFacetPath),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConstMeta {
    name: String,
    visibility: spire::ast::Visibility,
    ty: Ty,
    kind: ConstKind,
    value: StoredConstValue,
    span: Span,
}

/// Type-check the resolved AST, producing a fully typed tree.
pub fn typecheck(resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
    typecheck_with_context(resolved, TypecheckContext::default())
}

pub fn typecheck_with_warnings(
    resolved: Vec<Resolved>,
) -> Result<PhaseOutput<Vec<TypedNode>>, TypeError> {
    typecheck_with_context_with_warnings(resolved, TypecheckContext::default())
}

pub fn typecheck_staged_program(
    program: sigil::ResolvedStagedProgram,
) -> Result<TypedProgram, TypeError> {
    typecheck_staged_program_with_context(program, TypecheckContext::default())
}

pub fn typecheck_staged_program_with_warnings(
    program: sigil::ResolvedStagedProgram,
) -> Result<PhaseOutput<TypedProgram>, TypeError> {
    typecheck_staged_program_with_context_with_warnings(program, TypecheckContext::default())
}

pub fn typecheck_staged_program_with_context(
    program: sigil::ResolvedStagedProgram,
    context: TypecheckContext,
) -> Result<TypedProgram, TypeError> {
    typecheck_staged_program_with_context_with_warnings(program, context).map(|output| output.value)
}

pub fn typecheck_staged_program_with_context_with_warnings(
    program: sigil::ResolvedStagedProgram,
    context: TypecheckContext,
) -> Result<PhaseOutput<TypedProgram>, TypeError> {
    let process_specs = program
        .process_specs
        .into_iter()
        .map(Into::into)
        .collect::<Vec<TypedProcessSpec>>();
    let mut checker = Checker::new(context);
    checker.set_process_handler_dependencies(&process_specs);
    checker.boot_plan = program.boot_plan.clone();
    let nodes = checker.check_program(program.resolved)?;
    let warnings = checker.warnings.take();
    Ok(PhaseOutput::new(
        TypedProgram {
            nodes,
            process_specs,
            boot_plan: program.boot_plan,
        },
        warnings,
    ))
}

pub fn typecheck_with_context(
    resolved: Vec<Resolved>,
    context: TypecheckContext,
) -> Result<Vec<TypedNode>, TypeError> {
    typecheck_with_context_with_warnings(resolved, context).map(|output| output.value)
}

pub fn typecheck_with_context_with_warnings(
    resolved: Vec<Resolved>,
    context: TypecheckContext,
) -> Result<PhaseOutput<Vec<TypedNode>>, TypeError> {
    let mut checker = Checker::new(context);
    let value = checker.check_program(resolved)?;
    Ok(PhaseOutput::new(value, checker.warnings.take()))
}

pub fn type_contains_unresolved_vars(ty: &Ty) -> bool {
    match ty {
        Ty::Var(_) => true,
        Ty::List(inner) | Ty::Lazy(inner) => type_contains_unresolved_vars(inner),
        Ty::Tuple(items) | Ty::SelfApp(items) | Ty::Enum(_, items) => {
            items.iter().any(type_contains_unresolved_vars)
        }
        Ty::Func(params, ret) => {
            params.iter().any(type_contains_unresolved_vars) || type_contains_unresolved_vars(ret)
        }
        Ty::Result(source, focus) => {
            type_contains_unresolved_vars(source) || type_contains_unresolved_vars(focus)
        }
        Ty::Facet(_, source, focus, update_source, update_focus) => {
            type_contains_unresolved_vars(source)
                || type_contains_unresolved_vars(focus)
                || type_contains_unresolved_vars(update_source)
                || type_contains_unresolved_vars(update_focus)
        }
        Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
            params.iter().any(type_contains_unresolved_vars) || type_contains_unresolved_vars(ret)
        }
        Ty::Struct(_, fields) | Ty::Record(_, fields) => fields
            .iter()
            .any(|(_, field_ty)| type_contains_unresolved_vars(field_ty)),
        Ty::Int | Ty::Float | Ty::Str | Ty::Bool | Ty::Unit | Ty::Pid(_) | Ty::Hole | Ty::Error => {
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypecheckContext {
    pub runtime_policy: RuntimeSourcePolicy,
    pub enforce_builtin_type_contracts: bool,
    pub allow_error_function_params: bool,
    /// REPL `:facet` inspects structurally valid paths even when a private
    /// segment is not consumable from the current source scope.
    pub allow_private_facet_inspection: bool,
}

impl Default for TypecheckContext {
    fn default() -> Self {
        Self {
            runtime_policy: RuntimeSourcePolicy::script(),
            enforce_builtin_type_contracts: false,
            allow_error_function_params: false,
            allow_private_facet_inspection: false,
        }
    }
}

impl TypecheckContext {
    pub fn from_source_policy(policy: sindr::policy::SourcePolicy) -> Self {
        Self {
            runtime_policy: policy.runtime_policy,
            enforce_builtin_type_contracts: false,
            allow_error_function_params: false,
            allow_private_facet_inspection: false,
        }
    }
}

fn initialize_env() -> TypeEnv {
    let mut env = TypeEnv::new();
    // `Duration` is a stdlib-defined struct, but builtin signatures mention it
    // before stdlib declarations are typechecked. Reserve its type head here so
    // builtin signature parsing can treat it as the same struct identity.
    env.predeclare_type_def(
        "Global::Duration".into(),
        crate::env::TypeKind::Struct,
        Vec::new(),
    );
    env.predeclare_type_def(
        "Global::SupervisorStatus".into(),
        crate::env::TypeKind::Struct,
        Vec::new(),
    );
    for name in [
        "Global::FilePath",
        "Global::FileSystemEntry",
        "Global::FileSystemSnapshot",
        "Global::CommandResult",
    ] {
        env.predeclare_type_def(name.into(), crate::env::TypeKind::Struct, Vec::new());
    }

    // Ok constructor: ($A) -> Result<$A, $E>
    let ok_a = env.fresh_tyvar();
    let ok_e = env.fresh_tyvar();
    env.bind_var(
        0,
        Ty::BuiltinFunc {
            name: "Ok".into(),
            params: vec![ok_a.clone()],
            ret: Box::new(Ty::Result(Box::new(ok_a), Box::new(ok_e))),
        },
    );

    // Err constructor: ($E) -> Result<$A, $E>
    let err_a = env.fresh_tyvar();
    let err_e = env.fresh_tyvar();
    env.bind_var(
        1,
        Ty::BuiltinFunc {
            name: "Err".into(),
            params: vec![err_e.clone()],
            ret: Box::new(Ty::Result(Box::new(err_a), Box::new(err_e))),
        },
    );

    for (idx, meta) in builtin_function_metas().iter().enumerate() {
        let uid = builtin_uid(idx as u16);
        let ty = builtin_ty_from_meta(meta, &mut env);
        env.bind_var(uid, ty);
    }

    env
}

fn builtin_ty_from_meta(meta: &BuiltinMeta, env: &mut TypeEnv) -> Ty {
    let mut parser = BuiltinSignatureParser::new(meta.sig_str, env);
    let (params, ret) = parser
        .parse_signature()
        .unwrap_or_else(|err| panic!("invalid builtin signature for `{}`: {}", meta.name, err));
    Ty::BuiltinFunc {
        name: meta.name.into(),
        params,
        ret: Box::new(ret),
    }
}

struct BuiltinSignatureParser<'a, 'env> {
    input: &'a str,
    pos: usize,
    env: &'env mut TypeEnv,
    tyvars: HashMap<String, Ty>,
}

impl<'a, 'env> BuiltinSignatureParser<'a, 'env> {
    fn new(input: &'a str, env: &'env mut TypeEnv) -> Self {
        Self {
            input,
            pos: 0,
            env,
            tyvars: HashMap::new(),
        }
    }

    fn parse_signature(&mut self) -> Result<(Vec<Ty>, Ty), String> {
        self.skip_ws();
        let params = self.parse_param_list()?;
        self.skip_ws();
        self.expect("->")?;
        let ret = self.parse_type()?;
        self.skip_ws();
        if !self.is_eof() {
            return Err(format!(
                "unexpected trailing input `{}`",
                &self.input[self.pos..]
            ));
        }
        Ok((params, ret))
    }

    fn parse_param_list(&mut self) -> Result<Vec<Ty>, String> {
        self.expect("(")?;
        self.skip_ws();
        if self.consume(")") {
            return Ok(Vec::new());
        }
        let mut params = vec![self.parse_type()?];
        loop {
            self.skip_ws();
            if self.consume(")") {
                break;
            }
            self.expect(",")?;
            params.push(self.parse_type()?);
        }
        Ok(params)
    }

    fn parse_type(&mut self) -> Result<Ty, String> {
        self.skip_ws();
        let mut ty = if self.consume("(") {
            self.skip_ws();
            if self.consume(")") {
                Ty::Unit
            } else if self.consume("->") {
                let ret = self.parse_type()?;
                self.skip_ws();
                self.expect(")")?;
                Ty::Func(Vec::new(), Box::new(ret))
            } else {
                let first = self.parse_type()?;
                self.skip_ws();
                let mut items = vec![first];
                while self.consume(",") {
                    items.push(self.parse_type()?);
                    self.skip_ws();
                }
                if self.consume("->") {
                    let ret = self.parse_type()?;
                    self.skip_ws();
                    self.expect(")")?;
                    Ty::Func(items, Box::new(ret))
                } else {
                    self.expect(")")?;
                    match items.as_slice() {
                        [single] => single.clone(),
                        _ => Ty::Tuple(items),
                    }
                }
            }
        } else {
            let ident = self.parse_ident()?;
            self.skip_ws();
            if self.consume("<") {
                let mut args = vec![self.parse_type()?];
                loop {
                    self.skip_ws();
                    if self.consume(">") {
                        break;
                    }
                    self.expect(",")?;
                    args.push(self.parse_type()?);
                }
                self.build_generic_type(&ident, args)?
            } else {
                self.build_named_type(&ident)?
            }
        };

        self.skip_ws();
        while self.consume("?") {
            ty = Ty::Result(Box::new(ty), Box::new(Ty::Error));
            self.skip_ws();
        }
        Ok(ty)
    }

    fn builtin_special_enum_ty_for_query(ident: &str, args: &[Ty]) -> Option<Ty> {
        let surface = ident.strip_prefix("Global::").unwrap_or(ident);
        match surface {
            "Boolean" if args.is_empty() => Some(Ty::Bool),
            "Result" => args
                .first()
                .cloned()
                .map(|ok| Ty::Result(Box::new(ok), Box::new(Ty::Error))),
            _ => None,
        }
    }

    fn build_named_type(&mut self, ident: &str) -> Result<Ty, String> {
        if let Some(def) = self.env.lookup_type_def(ident) {
            return Ok(match &def.kind {
                crate::env::TypeKind::Struct => Ty::Struct(def.name.clone(), def.fields.clone()),
                crate::env::TypeKind::Record => Ty::Record(def.name.clone(), def.fields.clone()),
                crate::env::TypeKind::ConcreteError => Ty::Error,
                crate::env::TypeKind::Enum => Self::builtin_special_enum_ty_for_query(
                    def.name.strip_prefix("Global::").unwrap_or(&def.name),
                    &[],
                )
                .unwrap_or_else(|| Ty::Enum(def.name.clone(), Vec::new())),
            });
        }

        Ok(match ident {
            "Int" => Ty::Int,
            "Float" => Ty::Float,
            "String" => Ty::Str,
            "Boolean" => Ty::Bool,
            "Unit" => Ty::Unit,
            "Error" => Ty::Error,
            name if name.starts_with('$') => self
                .tyvars
                .entry(name.to_string())
                .or_insert_with(|| self.env.fresh_tyvar())
                .clone(),
            other => Ty::Enum(other.to_string(), Vec::new()),
        })
    }

    fn build_generic_type(&mut self, ident: &str, args: Vec<Ty>) -> Result<Ty, String> {
        Ok(match ident {
            "List" => {
                let [inner] = args.as_slice() else {
                    return Err("List requires exactly 1 type argument".into());
                };
                Ty::List(Box::new(inner.clone()))
            }
            "Result" => match args.as_slice() {
                [ok] => Ty::Result(Box::new(ok.clone()), Box::new(Ty::Error)),
                [ok, _err] => Ty::Result(Box::new(ok.clone()), Box::new(Ty::Error)),
                _ => return Err("Result requires 1 or 2 type arguments".into()),
            },
            "Lazy" => {
                let [inner] = args.as_slice() else {
                    return Err("Lazy requires exactly 1 type argument".into());
                };
                Ty::Lazy(Box::new(inner.clone()))
            }
            "Facet" => {
                let [kind, source, focus, update_source, update_focus] = args.as_slice() else {
                    return Err("Facet requires exactly 5 type arguments".into());
                };
                // Builtin polymorphic signatures use `$K` / `$L` for the
                // path-kind position.  `Ty` deliberately keeps Facet's kind
                // concrete, so use the broad readable capability while the
                // intrinsic checker preserves the path's actual atomic kind.
                let kind = match kind {
                    Ty::Enum(name, _) => crate::types::FacetKind::from_surface_name(&name)
                        .ok_or_else(|| format!("unknown Facet kind `{name}`"))?,
                    Ty::Var(_) => crate::types::FacetKind::ReadablePath,
                    _ => return Err("Facet kind must be a path kind name".into()),
                };
                Ty::Facet(
                    kind,
                    Box::new(source.clone()),
                    Box::new(focus.clone()),
                    Box::new(update_source.clone()),
                    Box::new(update_focus.clone()),
                )
            }
            "PID" => {
                let [inner] = args.as_slice() else {
                    return Err("PID requires exactly 1 type argument".into());
                };
                Ty::Pid(pid_marker_name_from_ty(inner))
            }
            other => Self::builtin_special_enum_ty_for_query(other, &args)
                .unwrap_or_else(|| Ty::Enum(other.to_string(), args)),
        })
    }

    fn parse_ident(&mut self) -> Result<String, String> {
        self.skip_ws();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        if self.pos == start {
            Err(format!("expected type identifier at byte {}", self.pos))
        } else {
            Ok(self.input[start..self.pos].to_string())
        }
    }

    fn expect(&mut self, needle: &str) -> Result<(), String> {
        self.skip_ws();
        if self.consume(needle) {
            Ok(())
        } else {
            Err(format!("expected `{}` at byte {}", needle, self.pos))
        }
    }

    fn consume(&mut self, needle: &str) -> bool {
        if self.input[self.pos..].starts_with(needle) {
            self.pos += needle.len();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.input.len()
    }
}

fn pid_marker_name_from_ty(ty: &Ty) -> String {
    match ty {
        Ty::Var(_) => "$Pid".to_string(),
        Ty::Int => "Int".to_string(),
        Ty::Float => "Float".to_string(),
        Ty::Str => "String".to_string(),
        Ty::Bool => "Boolean".to_string(),
        Ty::Unit => "Unit".to_string(),
        Ty::Error => "Error".to_string(),
        Ty::Hole => "_".to_string(),
        Ty::Pid(name) => name.clone(),
        Ty::Enum(name, _) | Ty::Struct(name, _) | Ty::Record(name, _) => name.clone(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod builtin_signature_tests {
    use super::{builtin_ty_from_meta, TypeEnv};
    use crate::types::Ty;
    use sindr::builtin::builtin_function_metas;

    #[test]
    fn builtin_meta_signatures_bootstrap_into_type_env() {
        let mut env = TypeEnv::new();
        for meta in builtin_function_metas() {
            let ty = builtin_ty_from_meta(meta, &mut env);
            match ty {
                Ty::BuiltinFunc { name, params, .. } => {
                    assert_eq!(name, meta.name);
                    assert_eq!(params.len(), meta.arity as usize);
                }
                other => panic!("expected builtin function type, got {:?}", other),
            }
        }
    }
}

fn format_builtin_type_param_suffix(params: &[&str]) -> String {
    if params.is_empty() {
        String::new()
    } else {
        format!("<{}>", params.join(", "))
    }
}

type TraitImplKey = (String, String);
type TraitImplIndex = HashMap<String, Vec<TraitImplKey>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct SpecializationKey {
    function_name: String,
    type_args: Vec<CanonicalTyKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum CanonicalTyKey {
    Int,
    Float,
    String,
    Boolean,
    Unit,
    Error,
    Hole,
    Var(u32),
    SelfApp(Vec<CanonicalTyKey>),
    List(Box<CanonicalTyKey>),
    Tuple(Vec<CanonicalTyKey>),
    Func {
        params: Vec<CanonicalTyKey>,
        ret: Box<CanonicalTyKey>,
    },
    Lazy(Box<CanonicalTyKey>),
    Facet {
        kind: crate::types::FacetKind,
        source: Box<CanonicalTyKey>,
        focus: Box<CanonicalTyKey>,
        update_source: Box<CanonicalTyKey>,
        update_focus: Box<CanonicalTyKey>,
    },
    Pid(String),
    BuiltinFunc {
        name: String,
        params: Vec<CanonicalTyKey>,
        ret: Box<CanonicalTyKey>,
    },
    UserFunc {
        type_params: Vec<u32>,
        params: Vec<CanonicalTyKey>,
        ret: Box<CanonicalTyKey>,
    },
    Struct {
        name: String,
        fields: Vec<(String, CanonicalTyKey)>,
    },
    Record {
        name: String,
        fields: Vec<(String, CanonicalTyKey)>,
    },
    Enum {
        name: String,
        args: Vec<CanonicalTyKey>,
    },
    Result {
        ok: Box<CanonicalTyKey>,
        err: Box<CanonicalTyKey>,
    },
}

#[derive(Debug, Clone)]
struct PersistentCheckerState {
    env: TypeEnv,
    consts: HashMap<u32, ConstMeta>,
    facet_bindings: HashMap<u32, StoredFacetPath>,
    error_observer_bindings: HashSet<u32>,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    specializable_defs: HashMap<u32, TypedNode>,
    specialization_fun_idxs: HashMap<SpecializationKey, u32>,
    traits: HashMap<String, TraitInfo>,
    trait_impls: HashMap<(String, String), TraitImplInfo>,
    trait_impl_index_by_base_trait: TraitImplIndex,
    trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
    signature_aliases: HashMap<String, SignatureAliasInfo>,
}

impl PersistentCheckerState {
    fn new() -> Self {
        Self {
            env: initialize_env(),
            consts: HashMap::new(),
            facet_bindings: HashMap::new(),
            error_observer_bindings: HashSet::new(),
            user_func_params: HashMap::new(),
            impl_method_uids: HashMap::new(),
            function_ids_by_name: HashMap::new(),
            specializable_defs: HashMap::new(),
            specialization_fun_idxs: HashMap::new(),
            traits: HashMap::new(),
            trait_impls: HashMap::new(),
            trait_impl_index_by_base_trait: HashMap::new(),
            trait_methods_by_qualified_name: HashMap::new(),
            tyvar_bounds: HashMap::new(),
            signature_aliases: HashMap::new(),
        }
    }

    fn checkpoint(&self, process_specs: Vec<TypedProcessSpec>) -> ScarCheckpoint {
        ScarCheckpoint {
            env: self.env.clone(),
            consts: self.consts.clone(),
            facet_bindings: self.facet_bindings.clone(),
            error_observer_bindings: self.error_observer_bindings.clone(),
            user_func_params: self.user_func_params.clone(),
            impl_method_uids: self.impl_method_uids.clone(),
            function_ids_by_name: self.function_ids_by_name.clone(),
            specializable_defs: self.specializable_defs.clone(),
            specialization_fun_idxs: self.specialization_fun_idxs.clone(),
            traits: self.traits.clone(),
            trait_impls: self.trait_impls.clone(),
            trait_impl_index_by_base_trait: self.trait_impl_index_by_base_trait.clone(),
            trait_methods_by_qualified_name: self.trait_methods_by_qualified_name.clone(),
            tyvar_bounds: self.tyvar_bounds.clone(),
            signature_aliases: self.signature_aliases.clone(),
            process_specs,
        }
    }
}

impl From<ScarCheckpoint> for PersistentCheckerState {
    fn from(checkpoint: ScarCheckpoint) -> Self {
        Self {
            env: checkpoint.env,
            consts: checkpoint.consts,
            facet_bindings: checkpoint.facet_bindings,
            error_observer_bindings: checkpoint.error_observer_bindings,
            user_func_params: checkpoint.user_func_params,
            impl_method_uids: checkpoint.impl_method_uids,
            function_ids_by_name: checkpoint.function_ids_by_name,
            specializable_defs: checkpoint.specializable_defs,
            specialization_fun_idxs: checkpoint.specialization_fun_idxs,
            traits: checkpoint.traits,
            trait_impls: checkpoint.trait_impls,
            trait_impl_index_by_base_trait: checkpoint.trait_impl_index_by_base_trait,
            trait_methods_by_qualified_name: checkpoint.trait_methods_by_qualified_name,
            tyvar_bounds: checkpoint.tyvar_bounds,
            signature_aliases: checkpoint.signature_aliases,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScarCheckpoint {
    env: TypeEnv,
    consts: HashMap<u32, ConstMeta>,
    facet_bindings: HashMap<u32, StoredFacetPath>,
    #[serde(default)]
    error_observer_bindings: HashSet<u32>,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    specializable_defs: HashMap<u32, TypedNode>,
    #[serde(default)]
    specialization_fun_idxs: HashMap<SpecializationKey, u32>,
    traits: HashMap<String, TraitInfo>,
    trait_impls: HashMap<(String, String), TraitImplInfo>,
    trait_impl_index_by_base_trait: TraitImplIndex,
    trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
    signature_aliases: HashMap<String, SignatureAliasInfo>,
    process_specs: Vec<TypedProcessSpec>,
}

#[derive(Debug, Clone)]
pub struct ScarSession {
    state: PersistentCheckerState,
    process_specs: Vec<TypedProcessSpec>,
}

impl ScarSession {
    pub fn new() -> Self {
        Self {
            state: PersistentCheckerState::new(),
            process_specs: Vec::new(),
        }
    }

    pub fn typecheck(&mut self, resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
        self.typecheck_with_context(resolved, TypecheckContext::default())
    }

    pub fn typecheck_with_warnings(
        &mut self,
        resolved: Vec<Resolved>,
    ) -> Result<PhaseOutput<Vec<TypedNode>>, TypeError> {
        self.typecheck_with_context_with_warnings(resolved, TypecheckContext::default())
    }

    pub fn typecheck_staged_program(
        &mut self,
        program: sigil::ResolvedStagedProgram,
    ) -> Result<TypedProgram, TypeError> {
        self.typecheck_staged_program_with_context(program, TypecheckContext::default())
    }

    pub fn typecheck_staged_program_with_warnings(
        &mut self,
        program: sigil::ResolvedStagedProgram,
    ) -> Result<PhaseOutput<TypedProgram>, TypeError> {
        self.typecheck_staged_program_with_context_with_warnings(
            program,
            TypecheckContext::default(),
        )
    }

    pub fn typecheck_staged_program_with_context(
        &mut self,
        program: sigil::ResolvedStagedProgram,
        context: TypecheckContext,
    ) -> Result<TypedProgram, TypeError> {
        self.typecheck_staged_program_with_context_with_warnings(program, context)
            .map(|output| output.value)
    }

    pub fn typecheck_staged_program_with_context_with_warnings(
        &mut self,
        program: sigil::ResolvedStagedProgram,
        context: TypecheckContext,
    ) -> Result<PhaseOutput<TypedProgram>, TypeError> {
        let process_specs = program
            .process_specs
            .into_iter()
            .map(Into::into)
            .collect::<Vec<TypedProcessSpec>>();
        let mut checker = Checker::with_persistent_state(self.state.clone(), context);
        checker.set_process_handler_dependencies(&process_specs);
        checker.boot_plan = program.boot_plan.clone();
        let nodes = checker.check_program(program.resolved)?;
        let persisted_process_specs = checker.process_specs.clone();
        let warnings = checker.warnings.take();
        self.state = checker.into_persistent_state();
        self.process_specs = persisted_process_specs;
        Ok(PhaseOutput::new(
            TypedProgram {
                nodes,
                process_specs,
                boot_plan: program.boot_plan,
            },
            warnings,
        ))
    }

    pub fn typecheck_with_context(
        &mut self,
        resolved: Vec<Resolved>,
        context: TypecheckContext,
    ) -> Result<Vec<TypedNode>, TypeError> {
        self.typecheck_with_context_with_warnings(resolved, context)
            .map(|output| output.value)
    }

    pub fn typecheck_with_context_with_warnings(
        &mut self,
        resolved: Vec<Resolved>,
        context: TypecheckContext,
    ) -> Result<PhaseOutput<Vec<TypedNode>>, TypeError> {
        let mut checker = Checker::with_persistent_state(self.state.clone(), context);
        checker.set_process_handler_dependencies(self.process_specs.as_slice());
        let typed = checker.check_program(resolved)?;
        let persisted_process_specs = checker.process_specs.clone();
        let warnings = checker.warnings.take();
        self.state = checker.into_persistent_state();
        self.process_specs = persisted_process_specs;
        Ok(PhaseOutput::new(typed, warnings))
    }

    pub fn checkpoint(&self) -> ScarCheckpoint {
        self.state.checkpoint(self.process_specs.clone())
    }

    pub fn lookup_type_def(&self, name: &str) -> Option<&crate::env::TypeDefInfo> {
        self.state.env.lookup_type_def(name)
    }

    pub fn enum_variants_of(&self, enum_name: &str) -> Option<&Vec<crate::env::EnumVariantInfo>> {
        self.state.env.enum_variants_of(enum_name)
    }

    pub fn rollback(&mut self, checkpoint: ScarCheckpoint) {
        let process_specs = checkpoint.process_specs.clone();
        self.state = checkpoint.into();
        self.process_specs = process_specs;
    }

    pub fn ensure_next_fun_idx_at_least(&mut self, next_fun_idx: u32) {
        // REPL runtime is the source of truth for currently materialized
        // function indices. Never move Scar backwards because stdlib
        // checkpoints may also reserve indices for delayed specializations.
        self.state.env.next_fun_idx = self.state.env.next_fun_idx.max(next_fun_idx);
    }

    pub fn reconcile_function_indices<'a, I>(&mut self, functions: I)
    where
        I: IntoIterator<Item = (&'a str, u32)>,
    {
        let function_indices = functions.into_iter().collect::<HashMap<_, _>>();
        let mut function_id_entries = self.state.function_ids_by_name.iter().collect::<Vec<_>>();
        function_id_entries.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));
        let specializable_by_name = self.specializable_fun_idxs_by_name();
        let mut next_fun_idx = function_indices
            .values()
            .copied()
            .max()
            .map(|idx| idx + 1)
            .unwrap_or(self.state.env.next_fun_idx);
        let mut specializable_rekeys = Vec::new();
        let mut fun_idx_rewrites = HashMap::new();
        for (qualified_name, id) in function_id_entries {
            let old_fun_idx = match self.state.env.vars.get(&id.unique_id) {
                Some(Ty::UserFunc { fun_idx, .. }) => Some(*fun_idx),
                _ => None,
            };
            let fun_idx = if let Some(fun_idx) = function_indices.get(qualified_name.as_str()) {
                *fun_idx
            } else if let Some(old_fun_idx) = specializable_by_name.get(qualified_name.as_str()) {
                let new_fun_idx = next_fun_idx;
                next_fun_idx += 1;
                specializable_rekeys.push((*old_fun_idx, new_fun_idx));
                fun_idx_rewrites.insert(*old_fun_idx, new_fun_idx);
                new_fun_idx
            } else {
                continue;
            };
            if let Some(Ty::UserFunc {
                fun_idx: stored_fun_idx,
                ..
            }) = self.state.env.vars.get_mut(&id.unique_id)
            {
                if let Some(old_fun_idx) = old_fun_idx {
                    fun_idx_rewrites.insert(old_fun_idx, fun_idx);
                }
                *stored_fun_idx = fun_idx;
            }
        }
        self.rekey_specializable_defs(specializable_rekeys);
        for def in self.state.specializable_defs.values_mut() {
            Self::rewrite_fun_indices_in_node(def, &fun_idx_rewrites);
        }
        Self::rewrite_specialization_fun_indices(
            &mut self.state.specialization_fun_idxs,
            &fun_idx_rewrites,
        );
        self.state.env.next_fun_idx = self.state.env.next_fun_idx.max(next_fun_idx);
    }

    pub fn reconcile_visible_function_indices<I>(&mut self, functions: I)
    where
        I: IntoIterator<Item = (u32, u32)>,
    {
        let mut next_fun_idx = self.state.env.next_fun_idx;
        let mut specializable_rekeys = Vec::new();
        let mut fun_idx_rewrites = HashMap::new();

        for (uid, fun_idx) in functions {
            let old_fun_idx = match self.state.env.vars.get(&uid) {
                Some(Ty::UserFunc { fun_idx, .. }) => Some(*fun_idx),
                _ => None,
            };
            if let Some(Ty::UserFunc {
                fun_idx: stored_fun_idx,
                ..
            }) = self.state.env.vars.get_mut(&uid)
            {
                if let Some(old_fun_idx) = old_fun_idx {
                    fun_idx_rewrites.insert(old_fun_idx, fun_idx);
                    specializable_rekeys.push((old_fun_idx, fun_idx));
                }
                *stored_fun_idx = fun_idx;
                next_fun_idx = next_fun_idx.max(fun_idx + 1);
            }
        }

        self.rekey_specializable_defs(specializable_rekeys);
        for def in self.state.specializable_defs.values_mut() {
            Self::rewrite_fun_indices_in_node(def, &fun_idx_rewrites);
        }
        Self::rewrite_specialization_fun_indices(
            &mut self.state.specialization_fun_idxs,
            &fun_idx_rewrites,
        );
        self.state.env.next_fun_idx = self.state.env.next_fun_idx.max(next_fun_idx);
    }

    fn rekey_specializable_defs(&mut self, rekeys: Vec<(u32, u32)>) {
        // Remove every old key before inserting any new key. A new index may
        // be another definition's old index (for example 451 -> 452 and
        // 452 -> 453); moving entries one at a time would overwrite the
        // first entry before it gets moved.
        let moved = rekeys
            .into_iter()
            .filter_map(|(old_fun_idx, new_fun_idx)| {
                self.state
                    .specializable_defs
                    .remove(&old_fun_idx)
                    .map(|mut def| {
                        Self::set_def_fun_idx(&mut def, new_fun_idx);
                        (new_fun_idx, def)
                    })
            })
            .collect::<Vec<_>>();
        for (new_fun_idx, def) in moved {
            self.state.specializable_defs.insert(new_fun_idx, def);
        }
    }

    fn rewrite_specialization_fun_indices(
        specialization_fun_idxs: &mut HashMap<SpecializationKey, u32>,
        rewrites: &HashMap<u32, u32>,
    ) {
        for fun_idx in specialization_fun_idxs.values_mut() {
            if let Some(new_fun_idx) = rewrites.get(fun_idx) {
                *fun_idx = *new_fun_idx;
            }
        }
    }

    fn specializable_fun_idxs_by_name(&self) -> HashMap<String, u32> {
        let mut entries: Vec<(String, u32)> = self
            .state
            .specializable_defs
            .iter()
            .filter_map(|(fun_idx, def)| Self::def_qualified_name(def).map(|name| (name, *fun_idx)))
            .collect::<Vec<_>>();
        entries.sort_by(|(left_name, left_idx), (right_name, right_idx)| {
            left_name
                .cmp(right_name)
                .then_with(|| left_idx.cmp(right_idx))
        });

        let mut by_name = HashMap::new();
        for (name, fun_idx) in entries {
            by_name.entry(name).or_insert(fun_idx);
        }
        by_name
    }

    fn def_qualified_name(def: &TypedNode) -> Option<String> {
        match &def.node {
            TypedInner::Def(_, id, ..) | TypedInner::ExtractorDef(_, id, ..) => {
                Some(id.qualified_name.clone().unwrap_or_else(|| id.name.clone()))
            }
            _ => None,
        }
    }

    fn set_def_fun_idx(def: &mut TypedNode, new_fun_idx: u32) {
        match &mut def.node {
            TypedInner::Def(fun_idx, ..) | TypedInner::ExtractorDef(fun_idx, ..) => {
                *fun_idx = new_fun_idx;
            }
            _ => {}
        }
    }

    fn rewrite_fun_indices_in_ty(ty: &mut Ty, rewrites: &HashMap<u32, u32>) {
        match ty {
            Ty::List(inner) | Ty::Lazy(inner) => Self::rewrite_fun_indices_in_ty(inner, rewrites),
            Ty::Tuple(items) | Ty::SelfApp(items) => {
                for item in items {
                    Self::rewrite_fun_indices_in_ty(item, rewrites);
                }
            }
            Ty::Func(params, ret) => {
                for param in params {
                    Self::rewrite_fun_indices_in_ty(param, rewrites);
                }
                Self::rewrite_fun_indices_in_ty(ret, rewrites);
            }
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                Self::rewrite_fun_indices_in_ty(source, rewrites);
                Self::rewrite_fun_indices_in_ty(focus, rewrites);
                Self::rewrite_fun_indices_in_ty(update_source, rewrites);
                Self::rewrite_fun_indices_in_ty(update_focus, rewrites);
            }
            Ty::BuiltinFunc { params, ret, .. } => {
                for param in params {
                    Self::rewrite_fun_indices_in_ty(param, rewrites);
                }
                Self::rewrite_fun_indices_in_ty(ret, rewrites);
            }
            Ty::UserFunc {
                fun_idx,
                params,
                ret,
                ..
            } => {
                if let Some(new_fun_idx) = rewrites.get(fun_idx) {
                    *fun_idx = *new_fun_idx;
                }
                for param in params {
                    Self::rewrite_fun_indices_in_ty(param, rewrites);
                }
                Self::rewrite_fun_indices_in_ty(ret, rewrites);
            }
            Ty::Struct(_, fields) | Ty::Record(_, fields) => {
                for (_, field_ty) in fields {
                    Self::rewrite_fun_indices_in_ty(field_ty, rewrites);
                }
            }
            Ty::Enum(_, args) => {
                for arg in args {
                    Self::rewrite_fun_indices_in_ty(arg, rewrites);
                }
            }
            Ty::Result(ok, err) => {
                Self::rewrite_fun_indices_in_ty(ok, rewrites);
                Self::rewrite_fun_indices_in_ty(err, rewrites);
            }
            Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Pid(_)
            | Ty::Hole
            | Ty::Var(_)
            | Ty::Error => {}
        }
    }

    fn rewrite_fun_indices_in_dispatch(dispatch: &mut TraitDispatch, rewrites: &HashMap<u32, u32>) {
        if let TraitDispatch::Static(TraitDispatchTarget::UserFunction { fun_idx, .. }) = dispatch {
            if let Some(new_fun_idx) = rewrites.get(fun_idx) {
                *fun_idx = *new_fun_idx;
            }
        }
    }

    fn rewrite_fun_indices_in_facet_path(path: &mut TypedFacetPath, rewrites: &HashMap<u32, u32>) {
        Self::rewrite_fun_indices_in_ty(&mut path.source_ty, rewrites);
        Self::rewrite_fun_indices_in_ty(&mut path.focus_ty, rewrites);
    }

    fn rewrite_fun_indices_in_pending_facet_path(
        path: &mut PendingFacetPath,
        rewrites: &HashMap<u32, u32>,
    ) {
        if let Some(source_ty_hint) = &mut path.source_ty_hint {
            Self::rewrite_fun_indices_in_ty(source_ty_hint, rewrites);
        }
    }

    fn rewrite_fun_indices_in_node(node: &mut TypedNode, rewrites: &HashMap<u32, u32>) {
        Self::rewrite_fun_indices_in_ty(&mut node.ty, rewrites);
        match &mut node.node {
            TypedInner::Lit(_) | TypedInner::Var(_) | TypedInner::ListNil => {}
            TypedInner::SupervisorSpawn { init, .. } => {
                Self::rewrite_fun_indices_in_node(init, rewrites);
            }
            TypedInner::SupervisorAdopt { pid, .. } => {
                Self::rewrite_fun_indices_in_node(pid, rewrites);
            }
            TypedInner::SupervisorStatus { .. } => {}
            TypedInner::SupervisorWorkers { init, strategy, .. } => {
                Self::rewrite_fun_indices_in_node(init, rewrites);
                Self::rewrite_fun_indices_in_node(strategy, rewrites);
            }
            TypedInner::App(func, args) => {
                Self::rewrite_fun_indices_in_node(func, rewrites);
                for arg in args {
                    Self::rewrite_fun_indices_in_node(arg, rewrites);
                }
            }
            TypedInner::TraitCall {
                receiver_ty,
                dispatch,
                origin,
                args,
                ..
            } => {
                Self::rewrite_fun_indices_in_ty(receiver_ty, rewrites);
                Self::rewrite_fun_indices_in_dispatch(dispatch, rewrites);
                if let TraitCallOrigin::Operator { lhs_ty, rhs_ty, .. } = origin {
                    Self::rewrite_fun_indices_in_ty(lhs_ty, rewrites);
                    Self::rewrite_fun_indices_in_ty(rhs_ty, rewrites);
                }
                for arg in args {
                    Self::rewrite_fun_indices_in_node(arg, rewrites);
                }
            }
            TypedInner::InjectCall(func, args) => {
                Self::rewrite_fun_indices_in_node(func, rewrites);
                for arg in args {
                    Self::rewrite_fun_indices_in_node(arg, rewrites);
                }
            }
            TypedInner::Block(stmts)
            | TypedInner::ListLiteral(stmts)
            | TypedInner::TupleLiteral(stmts) => {
                for stmt in stmts {
                    Self::rewrite_fun_indices_in_node(stmt, rewrites);
                }
            }
            TypedInner::HashMapLiteral(entries) => {
                for (key, value) in entries {
                    Self::rewrite_fun_indices_in_node(key, rewrites);
                    Self::rewrite_fun_indices_in_node(value, rewrites);
                }
            }
            TypedInner::Bind(pattern, rhs) | TypedInner::SafeBind(pattern, rhs) => {
                Self::rewrite_fun_indices_in_pattern(pattern, rewrites);
                Self::rewrite_fun_indices_in_node(rhs, rewrites);
            }
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right) => {
                Self::rewrite_fun_indices_in_node(left, rewrites);
                Self::rewrite_fun_indices_in_node(right, rewrites);
            }
            TypedInner::InterpolatedStr(parts) => {
                for part in parts {
                    if let TypedInterpolatedPart::Expr(expr) = part {
                        Self::rewrite_fun_indices_in_node(expr, rewrites);
                    }
                }
            }
            TypedInner::Dbg(args) => {
                for arg in args {
                    Self::rewrite_fun_indices_in_node(&mut arg.expr, rewrites);
                }
            }
            TypedInner::EagerBoundary(inner) => Self::rewrite_fun_indices_in_node(inner, rewrites),
            TypedInner::If(cond, then_node, else_node) => {
                Self::rewrite_fun_indices_in_node(cond, rewrites);
                Self::rewrite_fun_indices_in_node(then_node, rewrites);
                if let Some(else_node) = else_node {
                    Self::rewrite_fun_indices_in_node(else_node, rewrites);
                }
            }
            TypedInner::Assert(left, right) => {
                Self::rewrite_fun_indices_in_node(left, rewrites);
                Self::rewrite_fun_indices_in_node(right, rewrites);
            }
            TypedInner::Ensure(left, middle, right) => {
                Self::rewrite_fun_indices_in_node(left, rewrites);
                Self::rewrite_fun_indices_in_node(middle, rewrites);
                Self::rewrite_fun_indices_in_node(right, rewrites);
            }
            TypedInner::MapErr(value, err) | TypedInner::Cause(value, err) => {
                Self::rewrite_fun_indices_in_node(value, rewrites);
                Self::rewrite_fun_indices_in_node(err, rewrites);
            }
            TypedInner::RecoverKind(value, marker, handler) => {
                Self::rewrite_fun_indices_in_node(value, rewrites);
                Self::rewrite_fun_indices_in_node(marker, rewrites);
                Self::rewrite_fun_indices_in_node(handler, rewrites);
            }
            TypedInner::Match(scrutinee, arms) => {
                Self::rewrite_fun_indices_in_node(scrutinee, rewrites);
                for arm in arms {
                    Self::rewrite_fun_indices_in_match_arm(arm, rewrites);
                }
            }
            TypedInner::FieldAccess(value, _)
            | TypedInner::Semi(value)
            | TypedInner::Capture(value, _) => {
                Self::rewrite_fun_indices_in_node(value, rewrites);
            }
            TypedInner::ProcessContextHandler { .. } => {}
            TypedInner::FacetPath(path) => Self::rewrite_fun_indices_in_facet_path(path, rewrites),
            TypedInner::PendingFacetPath(path) => {
                Self::rewrite_fun_indices_in_pending_facet_path(path, rewrites);
            }
            TypedInner::FacetView {
                source,
                path,
                source_is_result: _,
            } => {
                Self::rewrite_fun_indices_in_node(source, rewrites);
                Self::rewrite_fun_indices_in_facet_path(path, rewrites);
            }
            TypedInner::FacetSet {
                source,
                path,
                value,
                source_is_result: _,
                mode: _,
            } => {
                Self::rewrite_fun_indices_in_node(source, rewrites);
                Self::rewrite_fun_indices_in_facet_path(path, rewrites);
                Self::rewrite_fun_indices_in_node(value, rewrites);
            }
            TypedInner::FacetOver {
                source,
                path,
                update_fun,
                source_is_result: _,
                mode: _,
            } => {
                Self::rewrite_fun_indices_in_node(source, rewrites);
                Self::rewrite_fun_indices_in_facet_path(path, rewrites);
                Self::rewrite_fun_indices_in_node(update_fun, rewrites);
            }
            TypedInner::StructLit(_, fields) | TypedInner::ConstructorCall(_, fields) => {
                for field in fields {
                    Self::rewrite_fun_indices_in_node(field, rewrites);
                }
            }
            TypedInner::DeferrorDef(_, _, _, params, body) => {
                for param in params {
                    Self::rewrite_fun_indices_in_fun_param(param, rewrites);
                }
                Self::rewrite_fun_indices_in_node(body, rewrites);
            }
            TypedInner::Def(_, _, type_params, params, ret_ty, _, body, _) => {
                for type_param in type_params {
                    let _ = type_param;
                }
                for param in params {
                    Self::rewrite_fun_indices_in_fun_param(param, rewrites);
                }
                Self::rewrite_fun_indices_in_ty(ret_ty, rewrites);
                Self::rewrite_fun_indices_in_node(body, rewrites);
            }
            TypedInner::ExtractorDef(_, _, type_params, param, ret_ty, body, _) => {
                for type_param in type_params {
                    let _ = type_param;
                }
                Self::rewrite_fun_indices_in_fun_param(param, rewrites);
                Self::rewrite_fun_indices_in_ty(ret_ty, rewrites);
                Self::rewrite_fun_indices_in_node(body, rewrites);
            }
            TypedInner::Closure(params, _, body) => {
                for param in params {
                    Self::rewrite_fun_indices_in_closure_param(param, rewrites);
                }
                Self::rewrite_fun_indices_in_node(body, rewrites);
            }
            TypedInner::EnumDef(_, _)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::BuiltinExtractorDecl(_, _, _)
            | TypedInner::StructDef(_, _, _, _, _)
            | TypedInner::RecordDef(_, _, _, _, _) => {}
        }
    }

    fn rewrite_fun_indices_in_pattern(pattern: &mut TypedPattern, rewrites: &HashMap<u32, u32>) {
        match pattern {
            TypedPattern::Var(ty, _)
            | TypedPattern::Pin(ty, _, _)
            | TypedPattern::As(ty, _, _)
            | TypedPattern::Wildcard(ty)
            | TypedPattern::ListNil(ty)
            | TypedPattern::ListCons(ty, _, _)
            | TypedPattern::IntLit(ty, _)
            | TypedPattern::StrLit(ty, _)
            | TypedPattern::BoolLit(ty, _)
            | TypedPattern::DurationLit(ty, _)
            | TypedPattern::Tuple(ty, _)
            | TypedPattern::ResultOk(ty, _) => Self::rewrite_fun_indices_in_ty(ty, rewrites),
            TypedPattern::Extractor {
                input_ty,
                extractor_ty,
                seq_tys,
                ..
            } => {
                Self::rewrite_fun_indices_in_ty(input_ty, rewrites);
                Self::rewrite_fun_indices_in_ty(extractor_ty, rewrites);
                for ty in seq_tys {
                    Self::rewrite_fun_indices_in_ty(ty, rewrites);
                }
            }
        }
        match pattern {
            TypedPattern::As(_, inner, _) | TypedPattern::ResultOk(_, inner) => {
                Self::rewrite_fun_indices_in_pattern(inner, rewrites);
            }
            TypedPattern::ListCons(_, head, tail) => {
                Self::rewrite_fun_indices_in_pattern(head, rewrites);
                Self::rewrite_fun_indices_in_pattern(tail, rewrites);
            }
            TypedPattern::Tuple(_, items) => {
                for item in items {
                    Self::rewrite_fun_indices_in_pattern(item, rewrites);
                }
            }
            TypedPattern::Extractor { items, .. } => {
                for item in items {
                    Self::rewrite_fun_indices_in_pattern(item, rewrites);
                }
            }
            TypedPattern::Var(_, _)
            | TypedPattern::Pin(_, _, _)
            | TypedPattern::Wildcard(_)
            | TypedPattern::ListNil(_)
            | TypedPattern::IntLit(_, _)
            | TypedPattern::StrLit(_, _)
            | TypedPattern::BoolLit(_, _)
            | TypedPattern::DurationLit(_, _) => {}
        }
    }

    fn rewrite_fun_indices_in_match_pattern(
        pattern: &mut TypedMatchPattern,
        rewrites: &HashMap<u32, u32>,
    ) {
        match pattern {
            TypedMatchPattern::As(inner, _) => {
                Self::rewrite_fun_indices_in_match_pattern(inner, rewrites)
            }
            TypedMatchPattern::Or(items) | TypedMatchPattern::Tuple(items) => {
                for item in items {
                    Self::rewrite_fun_indices_in_match_pattern(item, rewrites);
                }
            }
            TypedMatchPattern::Constructor { fields, .. }
            | TypedMatchPattern::Extractor { items: fields, .. } => {
                for field in fields {
                    Self::rewrite_fun_indices_in_match_pattern(field, rewrites);
                }
            }
            TypedMatchPattern::ListCons(head, tail) => {
                Self::rewrite_fun_indices_in_match_pattern(head, rewrites);
                Self::rewrite_fun_indices_in_match_pattern(tail, rewrites);
            }
            TypedMatchPattern::Binding(_)
            | TypedMatchPattern::Pin { .. }
            | TypedMatchPattern::Wildcard
            | TypedMatchPattern::BoolLit(_)
            | TypedMatchPattern::IntLit(_)
            | TypedMatchPattern::StrLit(_)
            | TypedMatchPattern::DurationLit(_)
            | TypedMatchPattern::ErrorKind(_)
            | TypedMatchPattern::ListNil => {}
        }
    }

    fn rewrite_fun_indices_in_match_arm(arm: &mut TypedMatchArm, rewrites: &HashMap<u32, u32>) {
        Self::rewrite_fun_indices_in_match_pattern(&mut arm.pattern, rewrites);
        if let Some(guard) = &mut arm.guard {
            Self::rewrite_fun_indices_in_node(guard, rewrites);
        }
        Self::rewrite_fun_indices_in_node(&mut arm.body, rewrites);
    }

    fn rewrite_fun_indices_in_fun_param(param: &mut TypedFunParam, rewrites: &HashMap<u32, u32>) {
        Self::rewrite_fun_indices_in_ty(&mut param.ty, rewrites);
    }

    fn rewrite_fun_indices_in_closure_param(
        param: &mut TypedClosureParam,
        rewrites: &HashMap<u32, u32>,
    ) {
        Self::rewrite_fun_indices_in_ty(&mut param.ty, rewrites);
    }
}

impl Default for ScarSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod specialization_state_tests {
    use super::*;

    fn test_span() -> Span {
        Span { start: 0, end: 0 }
    }

    fn resolved_id(name: &str, qualified_name: &str, unique_id: u32) -> ResolvedId {
        ResolvedId {
            name: name.to_string(),
            qualified_name: Some(qualified_name.to_string()),
            symbol_info: None,
            unique_id,
            compiler_generated: false,
            span: test_span(),
        }
    }

    fn user_func_ty(fun_idx: u32) -> Ty {
        Ty::UserFunc {
            fun_idx,
            type_params: Vec::new(),
            params: vec![Ty::Int],
            ret: Box::new(Ty::Int),
        }
    }

    fn specializable_def(fun_idx: u32, name: &str, uid: u32) -> TypedNode {
        let id = resolved_id(name, &format!("Global::{name}"), uid);
        TypedNode {
            ty: user_func_ty(fun_idx),
            span: test_span(),
            node: TypedInner::Def(
                fun_idx,
                id.clone(),
                Vec::new(),
                vec![TypedFunParam {
                    id: resolved_id("value", "", uid + 1000),
                    ty: Ty::Int,
                }],
                Ty::Int,
                None,
                Box::new(TypedNode {
                    ty: Ty::Int,
                    span: test_span(),
                    node: TypedInner::Lit(Lit::Int(1.into())),
                }),
                spire::ast::Visibility::Public,
            ),
        }
    }

    fn specialization_key() -> SpecializationKey {
        SpecializationKey {
            function_name: "Global::helper".to_string(),
            type_args: vec![CanonicalTyKey::Int],
        }
    }

    fn session_with_cached_specialization(uid: u32, old_fun_idx: u32) -> ScarSession {
        let mut session = ScarSession::new();
        let id = resolved_id("helper", "Global::helper", uid);
        session
            .state
            .function_ids_by_name
            .insert("Global::helper".to_string(), id);
        session
            .state
            .env
            .vars
            .insert(uid, user_func_ty(old_fun_idx));
        session
            .state
            .specialization_fun_idxs
            .insert(specialization_key(), old_fun_idx);
        session
    }

    #[test]
    fn reconcile_function_indices_rewrites_specialization_cache_values() {
        let mut session = session_with_cached_specialization(10, 40);

        session.reconcile_function_indices([("Global::helper", 77)]);

        assert_eq!(
            session
                .state
                .specialization_fun_idxs
                .get(&specialization_key())
                .copied(),
            Some(77)
        );
    }

    #[test]
    fn reconcile_visible_function_indices_rewrites_specialization_cache_values() {
        let mut session = session_with_cached_specialization(10, 40);

        session.reconcile_visible_function_indices([(10, 77)]);

        assert_eq!(
            session
                .state
                .specialization_fun_idxs
                .get(&specialization_key())
                .copied(),
            Some(77)
        );
    }

    #[test]
    fn reconcile_function_indices_moves_colliding_specializable_defs_without_overwrite() {
        let mut session = ScarSession::new();
        let eq_id = resolved_id("a_eq", "Global::a_eq", 10);
        let compare_id = resolved_id("b_compare", "Global::b_compare", 11);
        session
            .state
            .function_ids_by_name
            .insert("Global::a_eq".to_string(), eq_id.clone());
        session
            .state
            .function_ids_by_name
            .insert("Global::b_compare".to_string(), compare_id.clone());
        session
            .state
            .env
            .vars
            .insert(eq_id.unique_id, user_func_ty(451));
        session
            .state
            .env
            .vars
            .insert(compare_id.unique_id, user_func_ty(452));
        session
            .state
            .specializable_defs
            .insert(451, specializable_def(451, "a_eq", 10));
        session
            .state
            .specializable_defs
            .insert(452, specializable_def(452, "b_compare", 11));

        session.reconcile_function_indices([("Global::other", 451)]);

        let names_by_index = session
            .state
            .specializable_defs
            .iter()
            .map(|(fun_idx, def)| {
                let TypedInner::Def(_, id, ..) = &def.node else {
                    panic!("expected function definition");
                };
                (*fun_idx, id.qualified_name.clone().unwrap())
            })
            .collect::<HashMap<_, _>>();
        assert_eq!(names_by_index.get(&452), Some(&"Global::a_eq".to_string()));
        assert_eq!(
            names_by_index.get(&453),
            Some(&"Global::b_compare".to_string())
        );
    }
}

struct Checker {
    env: TypeEnv,
    function_return_ty: Option<Ty>,
    local_annotation_tyvars: HashMap<String, Ty>,
    /// Declaration-owned generic variables are rigid while their body is
    /// checked. Inference variables may bind to them, but they never bind to a
    /// concrete type (or to a different signature generic) at the definition
    /// site.
    rigid_tyvars: HashSet<u32>,
    current_function_symbol: Option<String>,
    current_impl_struct_target: Option<String>,
    in_extractor_body: bool,
    closure_depth: usize,
    facet_bindings: HashMap<u32, StoredFacetPath>,
    error_observer_bindings: HashSet<u32>,
    consts: HashMap<u32, ConstMeta>,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    specializable_defs: HashMap<u32, TypedNode>,
    specialization_fun_idxs: HashMap<SpecializationKey, u32>,
    substitutions: HashMap<u32, Ty>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
    signature_aliases: HashMap<String, SignatureAliasInfo>,
    alias_expansion_stack: Vec<String>,
    runtime_policy: RuntimeSourcePolicy,
    enforce_builtin_type_contracts: bool,
    allow_error_function_params: bool,
    allow_private_facet_inspection: bool,
    allow_error_observer_value_use: usize,
    seen_builtin_type_decls: HashMap<String, (Vec<String>, Span)>,
    facet_path_kind_decls: HashMap<String, Vec<String>>,
    traits: HashMap<String, TraitInfo>,
    trait_impls: HashMap<(String, String), TraitImplInfo>,
    trait_impl_index_by_base_trait: TraitImplIndex,
    trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    profiler: TypecheckProfiler,
    process_handler_dependencies: HashMap<String, HashMap<String, String>>,
    process_specs: Vec<TypedProcessSpec>,
    boot_plan: spire::ast::SupervisorInitSpec,
    warnings: WarningBuffer,
}

impl Checker {
    pub(super) fn surface_name<'a>(name: &'a str) -> &'a str {
        name.strip_prefix("Global::").unwrap_or(name)
    }

    fn surface_ast_ty(ast_ty: &AstTy) -> String {
        match ast_ty {
            AstTy::Named(_, name) | AstTy::ImplTrait(_, name) => Self::surface_name(name).into(),
            AstTy::Generic(_, name, args) => format!(
                "{}<{}>",
                Self::surface_name(name),
                args.iter()
                    .map(Self::surface_ast_ty)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Tuple(_, items) => format!(
                "({})",
                items
                    .iter()
                    .map(Self::surface_ast_ty)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AstTy::Func(_, params, ret) => format!(
                "({} -> {})",
                params
                    .iter()
                    .map(Self::surface_ast_ty)
                    .collect::<Vec<_>>()
                    .join(", "),
                Self::surface_ast_ty(ret)
            ),
        }
    }

    pub(super) fn surface_qualified_name<'a>(name: Option<&'a str>) -> Option<&'a str> {
        name.map(Self::surface_name)
    }

    fn new(context: TypecheckContext) -> Self {
        Self::with_persistent_state(PersistentCheckerState::new(), context)
    }

    fn with_persistent_state(state: PersistentCheckerState, context: TypecheckContext) -> Self {
        Self {
            env: state.env,
            function_return_ty: None,
            local_annotation_tyvars: HashMap::new(),
            rigid_tyvars: HashSet::new(),
            current_function_symbol: None,
            current_impl_struct_target: None,
            in_extractor_body: false,
            closure_depth: 0,
            facet_bindings: state.facet_bindings,
            error_observer_bindings: state.error_observer_bindings,
            consts: state.consts,
            user_func_params: state.user_func_params,
            impl_method_uids: state.impl_method_uids,
            function_ids_by_name: state.function_ids_by_name,
            specializable_defs: state.specializable_defs,
            specialization_fun_idxs: state.specialization_fun_idxs,
            substitutions: HashMap::new(),
            tyvar_bounds: state.tyvar_bounds,
            signature_aliases: state.signature_aliases,
            alias_expansion_stack: Vec::new(),
            runtime_policy: context.runtime_policy,
            enforce_builtin_type_contracts: context.enforce_builtin_type_contracts,
            allow_error_function_params: context.allow_error_function_params,
            allow_private_facet_inspection: context.allow_private_facet_inspection,
            allow_error_observer_value_use: 0,
            seen_builtin_type_decls: HashMap::new(),
            facet_path_kind_decls: HashMap::new(),
            traits: state.traits,
            trait_impls: state.trait_impls,
            trait_impl_index_by_base_trait: state.trait_impl_index_by_base_trait,
            trait_methods_by_qualified_name: state.trait_methods_by_qualified_name,
            profiler: TypecheckProfiler::new_from_env(),
            process_handler_dependencies: HashMap::new(),
            process_specs: Vec::new(),
            boot_plan: spire::ast::SupervisorInitSpec::default(),
            warnings: WarningBuffer::default(),
        }
    }

    fn spawn_child_checker(&self, env: TypeEnv) -> Self {
        let profile = self.profiler.start();
        let mut state = self.persistent_state();
        state.env = env;
        let mut checker = Checker::with_persistent_state(
            state,
            TypecheckContext {
                runtime_policy: self.runtime_policy.clone(),
                enforce_builtin_type_contracts: self.enforce_builtin_type_contracts,
                allow_error_function_params: self.allow_error_function_params,
                allow_private_facet_inspection: self.allow_private_facet_inspection,
            },
        );
        checker.function_return_ty = self.function_return_ty.clone();
        checker.local_annotation_tyvars = self.local_annotation_tyvars.clone();
        checker.rigid_tyvars = self.rigid_tyvars.clone();
        checker.current_function_symbol = self.current_function_symbol.clone();
        checker.current_impl_struct_target = self.current_impl_struct_target.clone();
        checker.in_extractor_body = self.in_extractor_body;
        checker.closure_depth = self.closure_depth;
        checker.facet_bindings = self.facet_bindings.clone();
        checker.error_observer_bindings = self.error_observer_bindings.clone();
        checker.substitutions = self.substitutions.clone();
        checker.seen_builtin_type_decls = self.seen_builtin_type_decls.clone();
        checker.facet_path_kind_decls = self.facet_path_kind_decls.clone();
        checker.process_handler_dependencies = self.process_handler_dependencies.clone();
        checker.process_specs = self.process_specs.clone();
        checker.profiler = self.profiler.clone();
        self.profiler
            .finish(ProfileEvent::ChildCheckerSpawn, profile);
        checker
    }

    fn set_process_handler_dependencies(&mut self, process_specs: &[TypedProcessSpec]) {
        self.process_specs = process_specs.to_vec();
        self.process_handler_dependencies = process_specs
            .iter()
            .map(|spec| {
                let slots = spec
                    .spec
                    .handlers
                    .iter()
                    .map(|handler| (handler.slot.clone(), handler.capability.clone()))
                    .collect::<HashMap<_, _>>();
                (spec.process_name.clone(), slots)
            })
            .collect();
    }

    fn warning_span(span: &Span) -> WarningSpan {
        WarningSpan {
            start: span.start,
            end: span.end,
        }
    }

    fn push_warning(
        &mut self,
        kind: WarningKind,
        phase: WarningPhase,
        span: &Span,
        message: impl Into<String>,
        hint: Option<String>,
    ) {
        self.warnings.push(CompilerWarning::new(
            kind,
            phase,
            Self::warning_span(span),
            message,
            hint,
        ));
    }

    fn collect_unused_type_parameter_warnings(
        &mut self,
        stmts: &[Resolved],
    ) -> Result<(), TypeError> {
        for stmt in stmts {
            match stmt {
                Resolved::StructDef(_, id, type_params, fields, _) => {
                    let mut used = HashSet::new();
                    for field in fields {
                        Self::collect_ast_ty_type_params(&field.ty, &mut used);
                    }
                    self.warn_unused_type_params(type_params, &used, &id.name);
                }
                Resolved::EnumDef(_, id, type_params, variants, _) => {
                    let mut used = HashSet::new();
                    for variant in variants {
                        for payload in &variant.payload {
                            Self::collect_ast_ty_type_params(payload, &mut used);
                        }
                    }
                    self.warn_unused_type_params(type_params, &used, &id.name);
                }
                Resolved::Def(_, id, type_params, params, ret_ty, _, _, _) => {
                    let used = Self::signature_type_param_uses(params, ret_ty.as_ref());
                    self.warn_unused_type_params(type_params, &used, &id.name);
                }
                Resolved::ExtractorDef(_, id, type_params, param, ret_ty, _, _) => {
                    let mut used = HashSet::new();
                    if let Some(param_ty) = &param.ty {
                        Self::collect_ast_ty_type_params(param_ty, &mut used);
                    }
                    Self::collect_ast_ty_type_params(ret_ty, &mut used);
                    self.warn_unused_type_params(type_params, &used, &id.name);
                }
                Resolved::TraitDef(_, id, type_params, _, methods, _) => {
                    let mut trait_used = HashSet::new();
                    for method in methods {
                        let mut fun_param_slots = HashSet::new();
                        for fun_param in &method.fun_params {
                            Self::collect_ast_ty_type_params(fun_param, &mut fun_param_slots);
                        }
                        let mut value_param_slots = HashSet::new();
                        for param in &method.params {
                            Self::collect_ast_ty_type_params(&param.ty, &mut trait_used);
                            Self::collect_ast_ty_type_params(&param.ty, &mut value_param_slots);
                        }
                        let mut method_input_used = fun_param_slots;
                        method_input_used.extend(value_param_slots);
                        let mut method_output_used = HashSet::new();
                        Self::collect_ast_ty_type_params(&method.ret_ty, &mut trait_used);
                        Self::collect_ast_ty_type_params(&method.ret_ty, &mut method_output_used);

                        let mut method_used = method_input_used.clone();
                        method_used.extend(method_output_used.iter().cloned());
                        if method
                            .where_clause
                            .as_ref()
                            .map(|clause| {
                                clause.constraints.iter().any(|constraint| {
                                    let mut vars = HashSet::new();
                                    Self::collect_ast_ty_type_params(
                                        &constraint.subject,
                                        &mut vars,
                                    );
                                    vars.into_iter().any(|var| !method_used.contains(&var))
                                })
                            })
                            .unwrap_or(false)
                        {
                            return Err(TypeError {
                                message: format!(
                                    "Trait method {} has a type variable used only by where constraints",
                                    method.id.name
                                ),
                                span: method.span.clone(),
                                hint: Some("Add the type variable to FunParams or a value argument.".into()),
                            });
                        }
                        if method_output_used.iter().any(|var| !method_input_used.contains(var)) {
                            return Err(TypeError {
                                message: format!(
                                    "Trait method {} has a type variable that is only present in its return type",
                                    method.id.name
                                ),
                                span: method.span.clone(),
                                hint: Some("Add the type variable to FunParams or a value argument.".into()),
                            });
                        }
                        if id.name == "Default"
                            && method.id.name == "default"
                            && !method.fun_params.iter().any(|param| {
                                matches!(param, AstTy::Named(_, name) if name == "Self")
                            })
                        {
                            return Err(TypeError {
                                message: "Default::default must declare Self in FunParams".into(),
                                span: method.span.clone(),
                                hint: Some("Use `def default::<Self>() -> Self`.".into()),
                            });
                        }

                        self.warn_unused_type_params(
                            &method.type_params,
                            &method_used,
                            &method.id.name,
                        );
                    }
                    self.warn_unused_type_params(type_params, &trait_used, &id.name);
                }
                Resolved::TraitImplDef(_, _, _, _, _, methods) => {
                    for method in methods {
                        let used =
                            Self::signature_type_param_uses(&method.params, method.ret_ty.as_ref());
                        self.warn_unused_type_params(
                            &method.type_params,
                            &used,
                            &method.function_id.name,
                        );
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn signature_type_param_uses(
        params: &[ResolvedFunParam],
        ret_ty: Option<&AstTy>,
    ) -> HashSet<String> {
        let mut used = HashSet::new();
        for param in params {
            Self::collect_ast_ty_type_params(&param.ty, &mut used);
        }
        if let Some(ret_ty) = ret_ty {
            Self::collect_ast_ty_type_params(ret_ty, &mut used);
        }
        used
    }

    fn reject_return_only_signature_slots(
        params: &[ResolvedFunParam],
        ret_ty: Option<&AstTy>,
        span: &Span,
    ) -> Result<(), TypeError> {
        let mut parameter_slots = HashSet::new();
        for param in params {
            Self::collect_ast_ty_type_params(&param.ty, &mut parameter_slots);
        }
        let mut return_slots = HashSet::new();
        if let Some(ret_ty) = ret_ty {
            Self::collect_ast_ty_type_params(ret_ty, &mut return_slots);
        }
        if let Some(name) = return_slots.difference(&parameter_slots).next().cloned() {
            return Err(TypeError {
                message: format!(
                    "Signature type slot {name} appears only in the return type and has no introduction site"
                ),
                span: span.clone(),
                hint: Some(
                    "Introduce the slot in an argument or receiver type, or declare it as a trait/constructor slot."
                        .into(),
                ),
            });
        }
        Ok(())
    }

    fn collect_ast_ty_type_params(ty: &AstTy, used: &mut HashSet<String>) {
        match ty {
            AstTy::Named(_, name) => {
                if name.starts_with('$') {
                    used.insert(name.clone());
                }
            }
            AstTy::ImplTrait(_, _) => {}
            AstTy::Generic(_, name, args) => {
                if name.starts_with('$') {
                    used.insert(name.clone());
                }
                for arg in args {
                    Self::collect_ast_ty_type_params(arg, used);
                }
            }
            AstTy::Tuple(_, items) => {
                for item in items {
                    Self::collect_ast_ty_type_params(item, used);
                }
            }
            AstTy::Func(_, params, ret) => {
                for param in params {
                    Self::collect_ast_ty_type_params(param, used);
                }
                Self::collect_ast_ty_type_params(ret, used);
            }
        }
    }

    /// Collect the input/output slots relevant to constructor-trait methods.
    /// `Self` is a hidden constructor witness in these signatures, while `$A`
    /// names an element slot.  Ordinary named types are intentionally ignored.
    fn collect_constructor_signature_slots(ty: &AstTy, used: &mut HashSet<String>) {
        match ty {
            AstTy::Named(_, name) => {
                if name == "Self" || name.starts_with('$') {
                    used.insert(name.clone());
                }
            }
            AstTy::ImplTrait(_, _) => {}
            AstTy::Generic(_, name, args) => {
                if name == "Self" || name.starts_with('$') {
                    used.insert(name.clone());
                }
                for arg in args {
                    Self::collect_constructor_signature_slots(arg, used);
                }
            }
            AstTy::Tuple(_, items) => {
                for item in items {
                    Self::collect_constructor_signature_slots(item, used);
                }
            }
            AstTy::Func(_, params, ret) => {
                for param in params {
                    Self::collect_constructor_signature_slots(param, used);
                }
                Self::collect_constructor_signature_slots(ret, used);
            }
        }
    }

    fn warn_unused_type_params(
        &mut self,
        type_params: &[ResolvedTypeParam],
        used: &HashSet<String>,
        owner_name: &str,
    ) {
        for param in type_params {
            if used.contains(&param.name) {
                continue;
            }
            self.push_warning(
                WarningKind::UnusedTypeParameter,
                WarningPhase::Typecheck,
                &param.span,
                format!("unused type parameter `{}` in `{}`", param.name, owner_name),
                Some("Remove the type parameter if it is not part of the signature.".into()),
            );
        }
    }

    fn collect_unused_value_warnings_in_sequence(&mut self, nodes: &[TypedNode]) {
        for (idx, node) in nodes.iter().enumerate() {
            if idx + 1 < nodes.len()
                && !matches!(node.ty, Ty::Unit)
                && !Self::is_explicit_discard(node)
            {
                self.push_warning(
                    WarningKind::UnusedValue,
                    WarningPhase::Typecheck,
                    &node.span,
                    "unused value",
                    Some("Use `;` if this value is intentionally discarded.".into()),
                );
            }
            self.collect_unused_value_warnings_in_node(node);
        }
    }

    fn collect_unused_value_warnings_in_node(&mut self, node: &TypedNode) {
        match &node.node {
            TypedInner::Block(stmts) => self.collect_unused_value_warnings_in_sequence(stmts),
            TypedInner::App(func, args)
            | TypedInner::InjectCall(func, args)
            | TypedInner::Capture(func, args) => {
                self.collect_unused_value_warnings_in_node(func);
                for arg in args {
                    self.collect_unused_value_warnings_in_node(arg);
                }
            }
            TypedInner::TraitCall { args, .. }
            | TypedInner::ListLiteral(args)
            | TypedInner::TupleLiteral(args)
            | TypedInner::StructLit(_, args)
            | TypedInner::ConstructorCall(_, args) => {
                for arg in args {
                    self.collect_unused_value_warnings_in_node(arg);
                }
            }
            TypedInner::Bind(_, rhs)
            | TypedInner::SafeBind(_, rhs)
            | TypedInner::FieldAccess(rhs, _)
            | TypedInner::Semi(rhs) => self.collect_unused_value_warnings_in_node(rhs),
            TypedInner::BinOp(_, left, right)
            | TypedInner::Pipe(left, right)
            | TypedInner::Compose(_, left, right)
            | TypedInner::ListCons(left, right) => {
                self.collect_unused_value_warnings_in_node(left);
                self.collect_unused_value_warnings_in_node(right);
            }
            TypedInner::InterpolatedStr(parts) => {
                for part in parts {
                    if let TypedInterpolatedPart::Expr(expr) = part {
                        self.collect_unused_value_warnings_in_node(expr);
                    }
                }
            }
            TypedInner::Dbg(args) => {
                for arg in args {
                    self.collect_unused_value_warnings_in_node(&arg.expr);
                }
            }
            TypedInner::EagerBoundary(inner) => self.collect_unused_value_warnings_in_node(inner),
            TypedInner::If(cond, then_branch, else_branch) => {
                self.collect_unused_value_warnings_in_node(cond);
                self.collect_unused_value_warnings_in_node(then_branch);
                if let Some(else_branch) = else_branch {
                    self.collect_unused_value_warnings_in_node(else_branch);
                }
            }
            TypedInner::Assert(cond, err)
            | TypedInner::MapErr(cond, err)
            | TypedInner::Cause(cond, err) => {
                self.collect_unused_value_warnings_in_node(cond);
                self.collect_unused_value_warnings_in_node(err);
            }
            TypedInner::Ensure(value, pred, err) | TypedInner::RecoverKind(value, pred, err) => {
                self.collect_unused_value_warnings_in_node(value);
                self.collect_unused_value_warnings_in_node(pred);
                self.collect_unused_value_warnings_in_node(err);
            }
            TypedInner::Match(scrutinee, arms) => {
                self.collect_unused_value_warnings_in_node(scrutinee);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.collect_unused_value_warnings_in_node(guard);
                    }
                    self.collect_unused_value_warnings_in_node(&arm.body);
                }
            }
            TypedInner::SupervisorSpawn { init, .. }
            | TypedInner::SupervisorWorkers { init, .. } => {
                self.collect_unused_value_warnings_in_node(init);
            }
            TypedInner::SupervisorAdopt { pid, .. } => {
                self.collect_unused_value_warnings_in_node(pid);
            }
            TypedInner::FacetView { source, .. } => {
                self.collect_unused_value_warnings_in_node(source);
            }
            TypedInner::FacetSet { source, value, .. } => {
                self.collect_unused_value_warnings_in_node(source);
                self.collect_unused_value_warnings_in_node(value);
            }
            TypedInner::FacetOver {
                source, update_fun, ..
            } => {
                self.collect_unused_value_warnings_in_node(source);
                self.collect_unused_value_warnings_in_node(update_fun);
            }
            TypedInner::DeferrorDef(_, _, _, _, show)
            | TypedInner::Def(_, _, _, _, _, _, show, _)
            | TypedInner::ExtractorDef(_, _, _, _, _, show, _)
            | TypedInner::Closure(_, _, show) => {
                self.collect_unused_value_warnings_in_node(show);
            }
            TypedInner::HashMapLiteral(entries) => {
                for (key, value) in entries {
                    self.collect_unused_value_warnings_in_node(key);
                    self.collect_unused_value_warnings_in_node(value);
                }
            }
            TypedInner::Lit(_)
            | TypedInner::Var(_)
            | TypedInner::ListNil
            | TypedInner::ProcessContextHandler { .. }
            | TypedInner::SupervisorStatus { .. }
            | TypedInner::FacetPath(_)
            | TypedInner::PendingFacetPath(_)
            | TypedInner::EnumDef(_, _)
            | TypedInner::TraitDef(..)
            | TypedInner::TraitImplDef(..)
            | TypedInner::BuiltinExtractorDecl(_, _, _)
            | TypedInner::StructDef(_, _, _, _, _)
            | TypedInner::RecordDef(_, _, _, _, _) => {}
        }
    }

    fn is_explicit_discard(node: &TypedNode) -> bool {
        matches!(node.node, TypedInner::Semi(_))
    }

    fn is_lazy_init_function_symbol(&self, symbol: &str) -> bool {
        self.process_specs.iter().any(|spec| {
            spec.spec.standby
                && self
                    .function_ids_by_name
                    .get(symbol)
                    .is_some_and(|id| id.unique_id == spec.init_uid)
        })
    }

    pub(super) fn ty_contains_process_init(&self, ty: &Ty) -> bool {
        match self.resolve_ty(ty) {
            Ty::Enum(name, args) => {
                Self::builtin_type_is_process_boundary_only(&name)
                    || args.iter().any(|arg| self.ty_contains_process_init(arg))
            }
            Ty::Result(ok, err) => {
                self.ty_contains_process_init(&ok) || self.ty_contains_process_init(&err)
            }
            Ty::List(inner) | Ty::Lazy(inner) => self.ty_contains_process_init(&inner),
            Ty::Tuple(items) | Ty::SelfApp(items) => {
                items.iter().any(|item| self.ty_contains_process_init(item))
            }
            Ty::Func(params, ret) => {
                params
                    .iter()
                    .any(|param| self.ty_contains_process_init(param))
                    || self.ty_contains_process_init(&ret)
            }
            Ty::Struct(_, fields) | Ty::Record(_, fields) => fields
                .iter()
                .any(|(_, field_ty)| self.ty_contains_process_init(field_ty)),
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| self.ty_contains_process_init(param))
                    || self.ty_contains_process_init(&ret)
            }
            Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Pid(_)
            | Ty::Hole
            | Ty::Var(_)
            | Ty::Error
            | Ty::Facet(..) => false,
        }
    }

    fn process_init_state_ty(&self, ty: &Ty) -> Option<Ty> {
        match self.resolve_ty(ty) {
            Ty::Enum(name, args) if Self::builtin_type_is_process_boundary_only(&name) => {
                args.into_iter().next()
            }
            _ => None,
        }
    }

    fn builtin_type_is_process_boundary_only(name: &str) -> bool {
        builtin_type_usage_policy(Self::surface_name(name)).is_some_and(|policy| {
            policy.process_boundary_allowed && !policy.type_annotation_allowed
        })
    }

    fn process_handler_function_ty(&self, uid: u32) -> Option<(Vec<Ty>, Ty)> {
        match self.env.lookup_var(uid)? {
            Ty::UserFunc { params, ret, .. } | Ty::BuiltinFunc { params, ret, .. } => {
                Some((params.clone(), ret.as_ref().clone()))
            }
            _ => None,
        }
    }

    fn process_result_ok_ty(&self, ty: &Ty) -> Option<Ty> {
        match self.resolve_ty(ty) {
            Ty::Result(ok, _) => Some(ok.as_ref().clone()),
            _ => None,
        }
    }

    fn process_handler_public_name(process: &TypedProcessSpec, handler_name: &str) -> String {
        format!(
            "{}::{}",
            Self::surface_name(&process.process_name),
            handler_name
        )
    }

    fn validate_handler_first_param_state(
        &self,
        process: &TypedProcessSpec,
        handler_kind: &str,
        handler_name: &str,
        params: &[Ty],
        state_ty: &Ty,
        state_name: &str,
        span: Span,
    ) -> Result<(), TypeError> {
        let state_param = match params.first() {
            Some(first)
                if matches!(
                    self.resolve_ty(first),
                    Ty::Pid(name)
                        if Self::surface_name(&name)
                            == Self::surface_name(&process.process_name)
                ) =>
            {
                params.get(1)
            }
            _ => params.first(),
        };
        if state_param.is_some_and(|param| self.resolve_ty(param) == *state_ty) {
            return Ok(());
        }

        Err(TypeError {
            message: format!(
                "@{} handler `{}` first parameter must match process state type `{}`",
                handler_kind,
                Self::process_handler_public_name(process, handler_name),
                Self::surface_name(state_name)
            ),
            span,
            hint: None,
        })
    }

    fn validate_handler_result_ok_state(
        &self,
        process: &TypedProcessSpec,
        handler_kind: &str,
        handler_name: &str,
        ret: &Ty,
        state_ty: &Ty,
        state_name: &str,
        span: Span,
    ) -> Result<(), TypeError> {
        if self
            .process_result_ok_ty(ret)
            .is_some_and(|ok| self.resolve_ty(&ok) == *state_ty)
        {
            return Ok(());
        }

        Err(TypeError {
            message: format!(
                "@{} handler `{}` Result ok type must match process state type `{}`",
                handler_kind,
                Self::process_handler_public_name(process, handler_name),
                Self::surface_name(state_name)
            ),
            span,
            hint: None,
        })
    }

    fn validate_call_handler_result_state(
        &self,
        process: &TypedProcessSpec,
        handler_name: &str,
        ret: &Ty,
        state_ty: &Ty,
        state_name: &str,
        span: Span,
    ) -> Result<(), TypeError> {
        let has_state = self.process_result_ok_ty(ret).is_some_and(|ok| {
            matches!(self.resolve_ty(&ok), Ty::Enum(name, items)
                if Self::surface_name(&name) == "CallResult"
                    && items.len() == 2
                    && self.resolve_ty(&items[1]) == *state_ty)
        });
        if has_state {
            return Ok(());
        }

        Err(TypeError {
            message: format!(
                "@call handler `{}` Result ok type must be CallResult<Reply, {}>",
                Self::process_handler_public_name(process, handler_name),
                Self::surface_name(state_name)
            ),
            span,
            hint: None,
        })
    }

    fn validate_cast_handler_result_state(
        &self,
        process: &TypedProcessSpec,
        handler_name: &str,
        ret: &Ty,
        state_ty: &Ty,
        state_name: &str,
        span: Span,
    ) -> Result<(), TypeError> {
        let has_state = self.process_result_ok_ty(ret).is_some_and(|ok| {
            matches!(self.resolve_ty(&ok), Ty::Enum(name, items)
                if Self::surface_name(&name) == "CastResult"
                    && items.len() == 1
                    && self.resolve_ty(&items[0]) == *state_ty)
        });
        if has_state {
            return Ok(());
        }

        Err(TypeError {
            message: format!(
                "@cast handler `{}` Result ok type must be CastResult<{}>",
                Self::process_handler_public_name(process, handler_name),
                Self::surface_name(state_name)
            ),
            span,
            hint: None,
        })
    }

    fn validate_process_state_contracts(&self) -> Result<(), TypeError> {
        for process in &self.process_specs {
            if !matches!(
                process.spec.kind,
                spire::ast::ProcessKind::Agent | spire::ast::ProcessKind::GenServer
            ) {
                continue;
            }
            let state_ty =
                self.resolve_ast_ty_in_context(&process.spec.state, TypeSyntaxContext::General)?;
            let Some(Ty::UserFunc { ret, .. } | Ty::BuiltinFunc { ret, .. }) =
                self.env.lookup_var(process.init_uid)
            else {
                continue;
            };
            let init_ok_ty = match self.resolve_ty(ret) {
                Ty::Result(ok, _) => ok.as_ref().clone(),
                other => other,
            };
            let init_state_ty = if process.spec.standby {
                self.process_init_state_ty(&init_ok_ty)
                    .ok_or_else(|| TypeError {
                        message: format!(
                            "Standby @init for process `{}` must return Result<StandbyInit<State>>",
                            process.process_name
                        ),
                        span: Span { start: 0, end: 0 },
                        hint: None,
                    })?
            } else {
                if self.ty_contains_process_init(&init_ok_ty) {
                    return Err(TypeError {
                        message: "StandbyInit<T> is only allowed as Standby @init return type"
                            .into(),
                        span: Span { start: 0, end: 0 },
                        hint: None,
                    });
                }
                init_ok_ty
            };

            let init_state_ty = self.resolve_ty(&init_state_ty);
            let state_ty = self.resolve_ty(&state_ty);
            let state_name = Self::surface_ast_ty(&process.spec.state);
            if init_state_ty != state_ty {
                return Err(TypeError {
                    message: format!(
                        "@init handler `{}::init` Result ok type must match process state type `{}`",
                        Self::surface_name(&process.process_name),
                        state_name,
                    ),
                    span: Span { start: 0, end: 0 },
                    hint: None,
                });
            }
            let handler_uids = process
                .handler_uids
                .iter()
                .map(|handler| (handler.internal_name.as_str(), handler.uid))
                .collect::<HashMap<_, _>>();
            for handler in &process.spec.handler_specs {
                let Some(uid) = handler_uids.get(handler.internal_name.as_str()).copied() else {
                    continue;
                };
                let Some((params, ret)) = self.process_handler_function_ty(uid) else {
                    continue;
                };
                match handler.kind {
                    spire::ast::ProcessRuntimeHandlerKind::Init => {}
                    spire::ast::ProcessRuntimeHandlerKind::Get => {
                        self.validate_handler_first_param_state(
                            process,
                            "get",
                            &handler.name,
                            &params,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                    }
                    spire::ast::ProcessRuntimeHandlerKind::Set => {
                        self.validate_handler_first_param_state(
                            process,
                            "set",
                            &handler.name,
                            &params,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                        self.validate_handler_result_ok_state(
                            process,
                            "set",
                            &handler.name,
                            &ret,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                    }
                    spire::ast::ProcessRuntimeHandlerKind::Call => {
                        self.validate_handler_first_param_state(
                            process,
                            "call",
                            &handler.name,
                            &params,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                        self.validate_call_handler_result_state(
                            process,
                            &handler.name,
                            &ret,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                    }
                    spire::ast::ProcessRuntimeHandlerKind::Cast => {
                        self.validate_handler_first_param_state(
                            process,
                            "cast",
                            &handler.name,
                            &params,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                        self.validate_cast_handler_result_state(
                            process,
                            &handler.name,
                            &ret,
                            &state_ty,
                            &state_name,
                            handler.span.clone(),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    fn process_handler_return_exposes_context_pid(&self, symbol: &str, ty: &Ty) -> bool {
        let Some((process_name, handler)) = symbol.rsplit_once("::") else {
            return false;
        };
        if !Self::is_process_handler_name(handler) {
            return false;
        }
        let Some(slots) = self.process_handler_dependencies.get(process_name) else {
            return false;
        };
        self.ty_contains_handler_capability_pid(ty, slots)
    }

    pub(super) fn is_process_handler_name(handler: &str) -> bool {
        matches!(handler, "__agent_init" | "__agent_get" | "__agent_set")
            || handler.starts_with("__agent_call_")
            || handler.starts_with("__agent_cast_")
    }

    pub(super) fn current_process_spec(&self) -> Option<&TypedProcessSpec> {
        let symbol = self.current_function_symbol.as_deref()?;
        let (module, _) = symbol.rsplit_once("::")?;
        self.process_specs
            .iter()
            .find(|spec| spec.module_path == module || spec.process_name == module)
    }

    pub(super) fn stop_constructor_allowed(&self) -> bool {
        let Some(spec) = self.current_process_spec() else {
            return false;
        };
        if spec.spec.kind != spire::ast::ProcessKind::GenServer
            || spec.spec.instance != spire::ast::ProcessInstance::Worker
        {
            return false;
        }
        let Some(symbol) = self.current_function_symbol.as_deref() else {
            return false;
        };
        let Some((_, function_name)) = symbol.rsplit_once("::") else {
            return false;
        };
        function_name != "__agent_init"
    }

    pub(super) fn stop_constructor_error(&self, span: &Span, enum_name: &str) -> TypeError {
        TypeError {
            message: format!(
                "{} can only be used inside Worker GenServer @call/@cast handlers or local helper functions",
                enum_name
            ),
            span: span.clone(),
            hint: Some(
                "Use Stop(...) only from Worker defgenserver handlers or helper defs in the same process block."
                    .into(),
            ),
        }
    }

    fn ty_contains_handler_capability_pid(&self, ty: &Ty, slots: &HashMap<String, String>) -> bool {
        match self.resolve_ty(ty) {
            Ty::Pid(name) => slots.values().any(|capability| capability == &name),
            Ty::Result(ok, err) => {
                self.ty_contains_handler_capability_pid(&ok, slots)
                    || self.ty_contains_handler_capability_pid(&err, slots)
            }
            Ty::List(inner) | Ty::Lazy(inner) => {
                self.ty_contains_handler_capability_pid(&inner, slots)
            }
            Ty::Tuple(items) | Ty::SelfApp(items) => items
                .iter()
                .any(|item| self.ty_contains_handler_capability_pid(item, slots)),
            Ty::Func(params, ret) => {
                params
                    .iter()
                    .any(|param| self.ty_contains_handler_capability_pid(param, slots))
                    || self.ty_contains_handler_capability_pid(&ret, slots)
            }
            Ty::Struct(_, fields) | Ty::Record(_, fields) => fields
                .iter()
                .any(|(_, field_ty)| self.ty_contains_handler_capability_pid(field_ty, slots)),
            Ty::Enum(_, args) => args
                .iter()
                .any(|arg| self.ty_contains_handler_capability_pid(arg, slots)),
            Ty::BuiltinFunc { params, ret, .. } | Ty::UserFunc { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| self.ty_contains_handler_capability_pid(param, slots))
                    || self.ty_contains_handler_capability_pid(&ret, slots)
            }
            Ty::Var(_)
            | Ty::Int
            | Ty::Float
            | Ty::Str
            | Ty::Bool
            | Ty::Unit
            | Ty::Error
            | Ty::Hole => false,
            Ty::Facet(_, source, focus, update_source, update_focus) => {
                self.ty_contains_handler_capability_pid(&source, slots)
                    || self.ty_contains_handler_capability_pid(&focus, slots)
                    || self.ty_contains_handler_capability_pid(&update_source, slots)
                    || self.ty_contains_handler_capability_pid(&update_focus, slots)
            }
        }
    }

    fn absorb_child_progress(&mut self, child: &Checker) {
        let profile = self.profiler.start();
        self.substitutions = child.substitutions.clone();
        self.tyvar_bounds = child.tyvar_bounds.clone();
        self.env.next_tyvar = self.env.next_tyvar.max(child.env.next_tyvar);
        self.env.next_tag = self.env.next_tag.max(child.env.next_tag);
        if self.seen_builtin_type_decls.len() != child.seen_builtin_type_decls.len() {
            self.seen_builtin_type_decls = child.seen_builtin_type_decls.clone();
        }
        if self.impl_method_uids.len() != child.impl_method_uids.len() {
            self.impl_method_uids = child.impl_method_uids.clone();
        }
        if self.traits.len() != child.traits.len() {
            self.traits = child.traits.clone();
        }
        if self.trait_impls.len() != child.trait_impls.len() {
            self.trait_impls = child.trait_impls.clone();
            self.trait_impl_index_by_base_trait = child.trait_impl_index_by_base_trait.clone();
        }
        if self.trait_methods_by_qualified_name.len() != child.trait_methods_by_qualified_name.len()
        {
            self.trait_methods_by_qualified_name = child.trait_methods_by_qualified_name.clone();
        }
        self.warnings
            .extend(child.warnings.as_slice().iter().cloned());
        self.profiler
            .finish(ProfileEvent::ChildCheckerAbsorb, profile);
    }

    pub(super) fn lookup_enum_variant_by_constructor_id(
        &self,
        unique_id: u32,
    ) -> Option<crate::env::EnumVariantInfo> {
        let profile = self.profiler.start();
        let variant = self.env.enum_variant_by_constructor_id(unique_id).cloned();
        self.profiler
            .finish(ProfileEvent::EnumVariantCtorLookup, profile);
        variant
    }

    pub(super) fn lookup_enum_variants_of<'a>(
        &'a self,
        enum_name: &str,
    ) -> Option<&'a Vec<crate::env::EnumVariantInfo>> {
        let profile = self.profiler.start();
        let variants = self.env.enum_variants_of(enum_name);
        self.profiler
            .finish(ProfileEvent::EnumVariantsLookup, profile);
        variants
    }

    pub(super) fn lookup_enum_variant_by_short_name(
        &self,
        enum_name: &str,
        short_name: &str,
    ) -> Option<crate::env::EnumVariantInfo> {
        let profile = self.profiler.start();
        let variant = self
            .env
            .enum_variants_of(enum_name)
            .and_then(|variants| {
                variants
                    .iter()
                    .find(|candidate| candidate.short_name == short_name)
            })
            .cloned();
        self.profiler
            .finish(ProfileEvent::EnumVariantSelectorLookup, profile);
        variant
    }

    fn persistent_state(&self) -> PersistentCheckerState {
        PersistentCheckerState {
            env: self.env.clone(),
            consts: self.consts.clone(),
            facet_bindings: self.facet_bindings.clone(),
            error_observer_bindings: self.error_observer_bindings.clone(),
            user_func_params: self.user_func_params.clone(),
            impl_method_uids: self.impl_method_uids.clone(),
            function_ids_by_name: self.function_ids_by_name.clone(),
            specializable_defs: self.specializable_defs.clone(),
            specialization_fun_idxs: self.specialization_fun_idxs.clone(),
            traits: self.traits.clone(),
            trait_impls: self.trait_impls.clone(),
            trait_impl_index_by_base_trait: self.trait_impl_index_by_base_trait.clone(),
            trait_methods_by_qualified_name: self.trait_methods_by_qualified_name.clone(),
            tyvar_bounds: self.tyvar_bounds.clone(),
            signature_aliases: self.signature_aliases.clone(),
        }
    }

    fn into_persistent_state(self) -> PersistentCheckerState {
        PersistentCheckerState {
            env: self.env,
            consts: self.consts,
            facet_bindings: self.facet_bindings,
            error_observer_bindings: self.error_observer_bindings,
            user_func_params: self.user_func_params,
            impl_method_uids: self.impl_method_uids,
            function_ids_by_name: self.function_ids_by_name,
            specializable_defs: self.specializable_defs,
            specialization_fun_idxs: self.specialization_fun_idxs,
            traits: self.traits,
            trait_impls: self.trait_impls,
            trait_impl_index_by_base_trait: self.trait_impl_index_by_base_trait,
            trait_methods_by_qualified_name: self.trait_methods_by_qualified_name,
            tyvar_bounds: self.tyvar_bounds,
            signature_aliases: self.signature_aliases,
        }
    }

    pub(super) fn base_trait_key(trait_name: &str) -> &str {
        trait_name
            .split_once('<')
            .map_or(trait_name, |(base, _)| base)
    }

    pub(super) fn index_trait_impl(&mut self, trait_impl_key: TraitImplKey) {
        let base_trait_name = Self::base_trait_key(&trait_impl_key.0).to_string();
        let entries = self
            .trait_impl_index_by_base_trait
            .entry(base_trait_name)
            .or_default();
        if !entries.iter().any(|existing| existing == &trait_impl_key) {
            entries.push(trait_impl_key);
        }
    }

    pub(super) fn trait_impl_candidate_keys(&self, trait_name: &str) -> Vec<TraitImplKey> {
        self.trait_impl_index_by_base_trait
            .get(Self::base_trait_key(trait_name))
            .cloned()
            .unwrap_or_default()
    }

    fn check_program(&mut self, stmts: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
        let profile_enabled = self.profiler.enabled();
        if profile_enabled {
            self.profiler.reset();
        }
        let profile_start = profile_enabled.then(Instant::now);
        let mut predeclare_error_types_dur = Duration::ZERO;
        let mut predeclare_type_signatures_dur = Duration::ZERO;
        let mut predeclare_traits_dur = Duration::ZERO;
        let mut predeclare_functions_dur = Duration::ZERO;
        let mut ensure_struct_impl_new_contract_dur = Duration::ZERO;
        let mut check_stmt_loop_dur = Duration::ZERO;
        let mut ensure_builtin_type_contracts_dur = Duration::ZERO;
        let mut specialize_program_dur = Duration::ZERO;
        let mut stmt_count = 0usize;
        let mut slow_stmts = Vec::<(Duration, String)>::new();
        let mut stmt_kind_totals = HashMap::<String, (u64, Duration)>::new();

        let result = (|| -> Result<Vec<TypedNode>, TypeError> {
            self.collect_unused_type_parameter_warnings(&stmts)?;

            let t = profile_enabled.then(Instant::now);
            self.predeclare_error_types(&stmts);
            if let Some(start) = t {
                predeclare_error_types_dur = start.elapsed();
            }

            self.predeclare_signature_aliases(&stmts)?;

            let t = profile_enabled.then(Instant::now);
            self.predeclare_type_signatures(&stmts)?;
            if let Some(start) = t {
                predeclare_type_signatures_dur = start.elapsed();
            }

            self.predeclare_consts(&stmts)?;

            let t = profile_enabled.then(Instant::now);
            self.predeclare_traits(&stmts)?;
            if let Some(start) = t {
                predeclare_traits_dur = start.elapsed();
            }

            let t = profile_enabled.then(Instant::now);
            self.predeclare_functions(&stmts)?;
            if let Some(start) = t {
                predeclare_functions_dur = start.elapsed();
            }

            self.validate_process_state_contracts()?;

            let t = profile_enabled.then(Instant::now);
            self.ensure_struct_impl_new_contract(&stmts)?;
            if let Some(start) = t {
                ensure_struct_impl_new_contract_dur = start.elapsed();
            }

            let mut typed = Vec::new();
            let t = profile_enabled.then(Instant::now);
            for stmt in stmts {
                stmt_count += 1;
                // Inference substitutions are expression-local. Letting them leak
                // across sibling top-level statements can accidentally monomorphize
                // later generic calls based on earlier ones.
                self.substitutions.clear();
                let stmt_label = profile_enabled.then(|| Self::profile_stmt_label(&stmt));
                let stmt_start = profile_enabled.then(Instant::now);
                if let Resolved::ConstDef(..) = &stmt {
                    continue;
                }
                if let Resolved::TraitImplDef(
                    span,
                    trait_id,
                    trait_args,
                    target_ty,
                    where_clause,
                    methods,
                ) = &stmt
                {
                    let nodes = self.check_trait_impl_items(
                        span,
                        trait_id,
                        trait_args,
                        target_ty,
                        where_clause.as_ref(),
                        methods,
                    )?;
                    typed.extend(nodes.into_iter().map(|node| {
                        let profile = self.profiler.start();
                        let node = self.resolve_typed_node(node);
                        self.profiler
                            .finish(ProfileEvent::ResolveTypedNode, profile);
                        node
                    }));
                    if let (Some(start), Some(label)) = (stmt_start, stmt_label.as_ref()) {
                        let elapsed = start.elapsed();
                        slow_stmts.push((elapsed, label.clone()));
                        let kind = Self::profile_stmt_kind(&stmt).to_string();
                        let entry = stmt_kind_totals.entry(kind).or_insert((0, Duration::ZERO));
                        entry.0 += 1;
                        entry.1 += elapsed;
                    }
                    continue;
                }
                let node = self.check_node(&stmt)?;
                let profile = self.profiler.start();
                let node = self.resolve_typed_node(node);
                self.profiler
                    .finish(ProfileEvent::ResolveTypedNode, profile);
                typed.push(node);
                if let (Some(start), Some(label)) = (stmt_start, stmt_label.as_ref()) {
                    let elapsed = start.elapsed();
                    slow_stmts.push((elapsed, label.clone()));
                    let kind = Self::profile_stmt_kind(&stmt).to_string();
                    let entry = stmt_kind_totals.entry(kind).or_insert((0, Duration::ZERO));
                    entry.0 += 1;
                    entry.1 += elapsed;
                }
            }
            if let Some(start) = t {
                check_stmt_loop_dur = start.elapsed();
            }

            let t = profile_enabled.then(Instant::now);
            self.ensure_builtin_type_contracts()?;
            if let Some(start) = t {
                ensure_builtin_type_contracts_dur = start.elapsed();
            }

            let t = profile_enabled.then(Instant::now);
            let specialized = self.specialize_program(typed)?;
            if let Some(start) = t {
                specialize_program_dur = start.elapsed();
            }
            self.collect_unused_value_warnings_in_sequence(&specialized);
            Ok(specialized)
        })();

        if let Some(start) = profile_start {
            let total = start.elapsed();
            self.profiler.print_summary(total);
            if total >= Duration::from_millis(5) {
                eprintln!(
                    "scar-phase predeclare_error_types={:.3}ms predeclare_type_signatures={:.3}ms predeclare_traits={:.3}ms predeclare_functions={:.3}ms ensure_struct_impl_new_contract={:.3}ms check_stmt_loop={:.3}ms ensure_builtin_type_contracts={:.3}ms specialize_program={:.3}ms",
                    predeclare_error_types_dur.as_secs_f64() * 1000.0,
                    predeclare_type_signatures_dur.as_secs_f64() * 1000.0,
                    predeclare_traits_dur.as_secs_f64() * 1000.0,
                    predeclare_functions_dur.as_secs_f64() * 1000.0,
                    ensure_struct_impl_new_contract_dur.as_secs_f64() * 1000.0,
                    check_stmt_loop_dur.as_secs_f64() * 1000.0,
                    ensure_builtin_type_contracts_dur.as_secs_f64() * 1000.0,
                    specialize_program_dur.as_secs_f64() * 1000.0,
                );
                if !slow_stmts.is_empty() {
                    slow_stmts.sort_by(|a, b| b.0.cmp(&a.0));
                    let top = slow_stmts
                        .iter()
                        .take(8)
                        .map(|(dur, label)| {
                            format!("{}:{:.3}ms", label, dur.as_secs_f64() * 1000.0)
                        })
                        .collect::<Vec<_>>()
                        .join(" | ");
                    eprintln!("scar-phase stmt_count={} slow_top {}", stmt_count, top);
                }
                if !stmt_kind_totals.is_empty() {
                    let mut kinds = stmt_kind_totals
                        .iter()
                        .map(|(kind, (count, dur))| (kind.clone(), *count, *dur))
                        .collect::<Vec<_>>();
                    kinds.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
                    let summary = kinds
                        .iter()
                        .take(8)
                        .map(|(kind, count, dur)| {
                            format!("{}:{} ({:.3}ms)", kind, count, dur.as_secs_f64() * 1000.0)
                        })
                        .collect::<Vec<_>>()
                        .join(" | ");
                    eprintln!("scar-phase kind_top {}", summary);
                }
            }
        }

        result
    }

    fn profile_stmt_label(stmt: &Resolved) -> String {
        match stmt {
            Resolved::Def(_, id, ..) => format!("Def {}", id.name),
            Resolved::ExtractorDef(_, id, ..) => format!("ExtractorDef {}", id.name),
            Resolved::ConstDef(_, id, ..) => format!("ConstDef {}", id.name),
            Resolved::TraitDef(_, id, ..) => format!("TraitDef {}", id.name),
            Resolved::TraitImplDef(_, id, ..) => format!("TraitImplDef {}", id.name),
            Resolved::BuiltinDecl(_, id, ..) => format!("BuiltinDecl {}", id.name),
            Resolved::BuiltinExtractorDecl(_, id, ..) => {
                format!("BuiltinExtractorDecl {}", id.name)
            }
            Resolved::BuiltinTypeDecl(_, id, ..) => format!("BuiltinTypeDecl {}", id.name),
            Resolved::ResultCtorDecl(_, id, ..) => format!("ResultCtorDecl {}", id.name),
            Resolved::StructDef(_, id, ..) => format!("StructDef {}", id.name),
            Resolved::RecordDef(_, id, ..) => format!("RecordDef {}", id.name),
            Resolved::DeferrorDef(_, id, ..) => format!("DeferrorDef {}", id.name),
            Resolved::EnumDef(_, id, ..) => format!("EnumDef {}", id.name),
            Resolved::Bind(..) => "Bind".to_string(),
            Resolved::SafeBind(..) => "SafeBind".to_string(),
            Resolved::Match(..) => "Match".to_string(),
            Resolved::Block(..) => "Block".to_string(),
            Resolved::App(..) => "App".to_string(),
            Resolved::Dbg(..) => "Dbg".to_string(),
            Resolved::If(..) => "If".to_string(),
            Resolved::Ensure(..) => "Ensure".to_string(),
            Resolved::Assert(..) => "Assert".to_string(),
            Resolved::MapErr(..) => "MapErr".to_string(),
            Resolved::Cause(..) => "Cause".to_string(),
            Resolved::RecoverKind(..) => "RecoverKind".to_string(),
            Resolved::Semi(..) => "Semi".to_string(),
            _ => "Expr".to_string(),
        }
    }

    fn profile_stmt_kind(stmt: &Resolved) -> &'static str {
        match stmt {
            Resolved::Def(..) => "Def",
            Resolved::ExtractorDef(..) => "ExtractorDef",
            Resolved::ConstDef(..) => "ConstDef",
            Resolved::TraitDef(..) => "TraitDef",
            Resolved::TraitImplDef(..) => "TraitImplDef",
            Resolved::BuiltinDecl(..) => "BuiltinDecl",
            Resolved::BuiltinExtractorDecl(..) => "BuiltinExtractorDecl",
            Resolved::BuiltinTypeDecl(..) => "BuiltinTypeDecl",
            Resolved::ResultCtorDecl(..) => "ResultCtorDecl",
            Resolved::StructDef(..) => "StructDef",
            Resolved::RecordDef(..) => "RecordDef",
            Resolved::DeferrorDef(..) => "DeferrorDef",
            Resolved::EnumDef(..) => "EnumDef",
            Resolved::Bind(..) => "Bind",
            Resolved::SafeBind(..) => "SafeBind",
            Resolved::Match(..) => "Match",
            Resolved::Block(..) => "Block",
            Resolved::App(..) => "App",
            Resolved::Dbg(..) => "Dbg",
            Resolved::If(..) => "If",
            Resolved::Ensure(..) => "Ensure",
            Resolved::Assert(..) => "Assert",
            Resolved::MapErr(..) => "MapErr",
            Resolved::Cause(..) => "Cause",
            Resolved::RecoverKind(..) => "RecoverKind",
            Resolved::Semi(..) => "Semi",
            _ => "Expr",
        }
    }
}
