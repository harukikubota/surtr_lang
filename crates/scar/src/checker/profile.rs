use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub(super) enum ProfileEvent {
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
pub(super) struct ProfileSnapshot {
    pub(super) types_compatible_calls: u64,
    types_compatible_nanos: u64,
    pub(super) bind_tyvar_calls: u64,
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
pub(super) struct TypecheckProfiler {
    pub(super) enabled: bool,
    data: Arc<ProfileData>,
}

impl TypecheckProfiler {
    pub(super) fn new_from_env() -> Self {
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

    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn start(&self) -> Option<Instant> {
        self.enabled.then(Instant::now)
    }

    pub(super) fn finish(&self, event: ProfileEvent, start: Option<Instant>) {
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

    pub(super) fn reset(&self) {
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

    pub(super) fn snapshot(&self) -> ProfileSnapshot {
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

    pub(super) fn print_summary(&self, total: Duration) {
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

impl Checker {
    pub(super) fn profile_stmt_label(stmt: &Resolved) -> String {
        match stmt {
            Resolved::Def(_, id, ..) => format!("Def {}", id.name),
            Resolved::ExtractorDef(_, id, ..) => format!("ExtractorDef {}", id.name),
            Resolved::ConstDef(_, id, ..) => format!("ConstDef {}", id.name),
            Resolved::TraitDef(_, id, ..) => format!("TraitDef {}", id.name),
            Resolved::TraitImplDef(_, _, id, ..) => format!("TraitImplDef {}", id.name),
            Resolved::BuiltinDecl(_, id, ..) => format!("BuiltinDecl {}", id.name),
            Resolved::BuiltinExtractorDecl(_, id, ..) => {
                format!("BuiltinExtractorDecl {}", id.name)
            }
            Resolved::BuiltinTypeDecl(_, id, ..) => format!("BuiltinTypeDecl {}", id.name),
            Resolved::StructDef(_, id, ..) => format!("StructDef {}", id.name),
            Resolved::RecordDef(_, id, ..) => format!("RecordDef {}", id.name),
            Resolved::DeferrorDef(_, id, ..) => format!("DeferrorDef {}", id.name),
            Resolved::EnumDef(_, id, ..) => format!("EnumDef {}", id.name),
            Resolved::Bind(..) => "Bind".to_string(),
            Resolved::SafeBind(..) => "SafeBind".to_string(),
            Resolved::StatementQuestion(..) => "StatementQuestion".to_string(),
            Resolved::Match(..) | Resolved::IsMatch(..) | Resolved::IfLet(..) => {
                "Match".to_string()
            }
            Resolved::Block(..) => "Block".to_string(),
            Resolved::App(..) | Resolved::LazyApp(..) => "App".to_string(),
            Resolved::Dbg(..) => "Dbg".to_string(),
            Resolved::Cond(..) => "Cond".to_string(),
            Resolved::If(..) => "If".to_string(),
            Resolved::Cause(..) => "Cause".to_string(),
            Resolved::Semi(..) => "Semi".to_string(),
            _ => "Expr".to_string(),
        }
    }

    pub(super) fn profile_stmt_kind(stmt: &Resolved) -> &'static str {
        match stmt {
            Resolved::Def(..) => "Def",
            Resolved::ExtractorDef(..) => "ExtractorDef",
            Resolved::ConstDef(..) => "ConstDef",
            Resolved::TraitDef(..) => "TraitDef",
            Resolved::TraitImplDef(..) => "TraitImplDef",
            Resolved::BuiltinDecl(..) => "BuiltinDecl",
            Resolved::BuiltinExtractorDecl(..) => "BuiltinExtractorDecl",
            Resolved::BuiltinTypeDecl(..) => "BuiltinTypeDecl",
            Resolved::StructDef(..) => "StructDef",
            Resolved::RecordDef(..) => "RecordDef",
            Resolved::DeferrorDef(..) => "DeferrorDef",
            Resolved::EnumDef(..) => "EnumDef",
            Resolved::Bind(..) => "Bind",
            Resolved::SafeBind(..) => "SafeBind",
            Resolved::StatementQuestion(..) => "StatementQuestion",
            Resolved::Match(..) | Resolved::IsMatch(..) | Resolved::IfLet(..) => "Match",
            Resolved::Block(..) => "Block",
            Resolved::App(..) | Resolved::LazyApp(..) => "App",
            Resolved::Dbg(..) => "Dbg",
            Resolved::Cond(..) => "Cond",
            Resolved::If(..) => "If",
            Resolved::Cause(..) => "Cause",
            Resolved::Semi(..) => "Semi",
            _ => "Expr",
        }
    }
}
