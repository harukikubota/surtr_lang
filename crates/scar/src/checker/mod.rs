use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use sigil::resolved::*;
use sindr::builtin::{
    builtin_meta_by_name, builtin_type_meta_by_name, builtin_uid, BuiltinMeta, BUILTIN_METAS,
    BUILTIN_TYPE_METAS,
};
use sindr::policy::{ExitCodePolicy, RuntimeSourcePolicy};
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
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeSyntaxContext {
    General,
    FunctionReturn,
    ExtractorReturn,
    ExtractorBody,
    ErrorMarker,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct TraitMethodInfo {
    id: ResolvedId,
    type_params: Vec<ResolvedTypeParam>,
    params: Vec<ResolvedFunParam>,
    ret_ty: AstTy,
    span: Span,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct TraitInfo {
    id: ResolvedId,
    type_params: Vec<ResolvedTypeParam>,
    methods: HashMap<String, TraitMethodInfo>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct TraitImplMethodInfo {
    method_name: String,
    function_id: ResolvedId,
    type_params: Vec<ResolvedTypeParam>,
    params: Vec<ResolvedFunParam>,
    ret_ty: Option<AstTy>,
    body: Box<Resolved>,
    attrs: ResolvedDeclAttrs,
    span: Span,
    dispatch_override: Option<TraitDispatchTarget>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct TraitImplInfo {
    trait_id: ResolvedId,
    trait_args: Vec<AstTy>,
    target_name: String,
    target_ty: Ty,
    methods: HashMap<String, TraitImplMethodInfo>,
}

/// Type-check the resolved AST, producing a fully typed tree.
pub fn typecheck(resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
    typecheck_with_context(resolved, TypecheckContext::default())
}

pub fn typecheck_with_context(
    resolved: Vec<Resolved>,
    context: TypecheckContext,
) -> Result<Vec<TypedNode>, TypeError> {
    let mut checker = Checker::new(context);
    checker.check_program(resolved)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypecheckContext {
    pub runtime_policy: RuntimeSourcePolicy,
    pub enforce_builtin_type_contracts: bool,
}

impl Default for TypecheckContext {
    fn default() -> Self {
        Self {
            runtime_policy: RuntimeSourcePolicy::script(),
            enforce_builtin_type_contracts: false,
        }
    }
}

fn initialize_env() -> TypeEnv {
    let mut env = TypeEnv::new();
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

    for (idx, meta) in BUILTIN_METAS.iter().enumerate() {
        let uid = builtin_uid(idx as u16);
        let ty = builtin_ty_from_meta(meta, &mut env);
        env.bind_var(uid, ty);
    }

    env
}

fn builtin_ty_from_meta(meta: &BuiltinMeta, env: &mut TypeEnv) -> Ty {
    match meta.name {
        "print" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Str],
            ret: Box::new(Ty::Unit),
        },
        "to_string" => {
            let a = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![a],
                ret: Box::new(Ty::Str),
            }
        }
        "inspect" => {
            let a = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![a],
                ret: Box::new(Ty::Str),
            }
        }
        "safe_div" => {
            let a = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![a.clone(), a.clone()],
                ret: Box::new(Ty::Result(Box::new(a), Box::new(Ty::Error))),
            }
        }
        "safe_mod" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int, Ty::Int],
            ret: Box::new(Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error))),
        },
        "eprint" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Error],
            ret: Box::new(Ty::Unit),
        },
        "set_exit_code" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int],
            ret: Box::new(Ty::Unit),
        },
        "shl" | "shr" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int, Ty::Int],
            ret: Box::new(Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error))),
        },
        "bit_and" | "bit_or" | "bit_xor" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int, Ty::Int],
            ret: Box::new(Ty::Int),
        },
        "bit_not" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int],
            ret: Box::new(Ty::Int),
        },
        "test_bit" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int, Ty::Int],
            ret: Box::new(Ty::Result(Box::new(Ty::Bool), Box::new(Ty::Error))),
        },
        "set_bit" | "clear_bit" | "toggle_bit" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Int, Ty::Int],
            ret: Box::new(Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error))),
        },
        "codepoints" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Str, Ty::Enum("StringEncoding".into(), Vec::new())],
            ret: Box::new(Ty::Result(
                Box::new(Ty::List(Box::new(Ty::Int))),
                Box::new(Ty::Error),
            )),
        },
        "from_codepoints" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![
                Ty::List(Box::new(Ty::Int)),
                Ty::Enum("StringEncoding".into(), Vec::new()),
            ],
            ret: Box::new(Ty::Result(Box::new(Ty::Str), Box::new(Ty::Error))),
        },
        "len" => {
            let a = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::List(Box::new(a))],
                ret: Box::new(Ty::Int),
            }
        }
        "gen_make" => {
            let state = env.fresh_tyvar();
            let item = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::Int, Ty::List(Box::new(item.clone()))],
                ret: Box::new(Ty::Enum("Generator".into(), vec![state, item])),
            }
        }
        "gen_idx" => {
            let state = env.fresh_tyvar();
            let item = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::Enum("Generator".into(), vec![state, item])],
                ret: Box::new(Ty::Int),
            }
        }
        "gen_items" => {
            let state = env.fresh_tyvar();
            let item = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::Enum("Generator".into(), vec![state, item.clone()])],
                ret: Box::new(Ty::List(Box::new(item))),
            }
        }
        "group_count" => {
            let a = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::List(Box::new(a.clone()))],
                ret: Box::new(Ty::List(Box::new(Ty::Tuple(vec![a, Ty::Int])))),
            }
        }
        "zip" => {
            let a = env.fresh_tyvar();
            let b = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::List(Box::new(a.clone())), Ty::List(Box::new(b.clone()))],
                ret: Box::new(Ty::List(Box::new(Ty::Tuple(vec![a, b])))),
            }
        }
        "empty_map" => {
            let value = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: Vec::new(),
                ret: Box::new(Ty::Enum("HashMap".into(), vec![value])),
            }
        }
        "map_from_entries" => {
            let value = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::List(Box::new(Ty::Tuple(vec![Ty::Str, value.clone()])))],
                ret: Box::new(Ty::Enum("HashMap".into(), vec![value])),
            }
        }
        "map_len" => {
            let value = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::Enum("HashMap".into(), vec![value])],
                ret: Box::new(Ty::Int),
            }
        }
        "map_contains_key" => {
            let value = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::Enum("HashMap".into(), vec![value]), Ty::Str],
                ret: Box::new(Ty::Bool),
            }
        }
        "map_get" => {
            let value = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::Enum("HashMap".into(), vec![value.clone()]), Ty::Str],
                ret: Box::new(Ty::Result(Box::new(value), Box::new(Ty::Error))),
            }
        }
        "map_insert" => {
            let value = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![
                    Ty::Enum("HashMap".into(), vec![value.clone()]),
                    Ty::Str,
                    value.clone(),
                ],
                ret: Box::new(Ty::Enum("HashMap".into(), vec![value])),
            }
        }
        "map_remove" => {
            let value = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::Enum("HashMap".into(), vec![value.clone()]), Ty::Str],
                ret: Box::new(Ty::Enum("HashMap".into(), vec![value])),
            }
        }
        "map_keys" => {
            let value = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::Enum("HashMap".into(), vec![value])],
                ret: Box::new(Ty::List(Box::new(Ty::Str))),
            }
        }
        "map_values_list" => {
            let value = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![Ty::Enum("HashMap".into(), vec![value.clone()])],
                ret: Box::new(Ty::List(Box::new(value))),
            }
        }
        "view" => {
            let source = env.fresh_tyvar();
            let focus = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![
                    Ty::Lens(Box::new(source.clone()), Box::new(focus.clone())),
                    source,
                ],
                ret: Box::new(Ty::Result(Box::new(focus), Box::new(Ty::Error))),
            }
        }
        "compose" => {
            let source = env.fresh_tyvar();
            let middle = env.fresh_tyvar();
            let focus = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![
                    Ty::Lens(Box::new(source.clone()), Box::new(middle.clone())),
                    Ty::Lens(Box::new(middle), Box::new(focus.clone())),
                ],
                ret: Box::new(Ty::Lens(Box::new(source), Box::new(focus))),
            }
        }
        "set" => {
            let source = env.fresh_tyvar();
            let focus = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![
                    Ty::Lens(Box::new(source.clone()), Box::new(focus.clone())),
                    source.clone(),
                    focus,
                ],
                ret: Box::new(Ty::Result(Box::new(source), Box::new(Ty::Error))),
            }
        }
        "over" => {
            let source = env.fresh_tyvar();
            let focus = env.fresh_tyvar();
            Ty::BuiltinFunc {
                name: meta.name.into(),
                params: vec![
                    Ty::Lens(Box::new(source.clone()), Box::new(focus.clone())),
                    source.clone(),
                    Ty::Func(
                        vec![focus.clone()],
                        Box::new(Ty::Result(Box::new(focus), Box::new(Ty::Error))),
                    ),
                ],
                ret: Box::new(Ty::Result(Box::new(source), Box::new(Ty::Error))),
            }
        }
        "compile" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Str],
            ret: Box::new(Ty::Result(
                Box::new(Ty::Enum("Regex".into(), Vec::new())),
                Box::new(Ty::Error),
            )),
        },
        "is_match" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("Regex".into(), Vec::new()), Ty::Str],
            ret: Box::new(Ty::Bool),
        },
        "captures" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("Regex".into(), Vec::new()), Ty::Str],
            ret: Box::new(Ty::Result(
                Box::new(Ty::Enum("RegexCaptures".into(), Vec::new())),
                Box::new(Ty::Error),
            )),
        },
        "whole" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("RegexCaptures".into(), Vec::new())],
            ret: Box::new(Ty::Str),
        },
        "capture_count" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("RegexCaptures".into(), Vec::new())],
            ret: Box::new(Ty::Int),
        },
        "get" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("RegexCaptures".into(), Vec::new()), Ty::Int],
            ret: Box::new(Ty::Result(Box::new(Ty::Str), Box::new(Ty::Error))),
        },
        "get_name" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("RegexCaptures".into(), Vec::new()), Ty::Str],
            ret: Box::new(Ty::Result(Box::new(Ty::Str), Box::new(Ty::Error))),
        },
        "find" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("Regex".into(), Vec::new()), Ty::Str],
            ret: Box::new(Ty::Result(
                Box::new(Ty::Enum("RegexMatch".into(), Vec::new())),
                Box::new(Ty::Error),
            )),
        },
        "find_all" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("Regex".into(), Vec::new()), Ty::Str],
            ret: Box::new(Ty::List(Box::new(Ty::Enum(
                "RegexMatch".into(),
                Vec::new(),
            )))),
        },
        "split" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("Regex".into(), Vec::new()), Ty::Str],
            ret: Box::new(Ty::List(Box::new(Ty::Str))),
        },
        "replace" | "replace_all" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("Regex".into(), Vec::new()), Ty::Str, Ty::Str],
            ret: Box::new(Ty::Str),
        },
        "escape" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Str],
            ret: Box::new(Ty::Str),
        },
        "group_names" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("Regex".into(), Vec::new())],
            ret: Box::new(Ty::List(Box::new(Ty::Str))),
        },
        "text" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("RegexMatch".into(), Vec::new())],
            ret: Box::new(Ty::Str),
        },
        "start" | "end" => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Enum("RegexMatch".into(), Vec::new())],
            ret: Box::new(Ty::Int),
        },
        _ => Ty::BuiltinFunc {
            name: meta.name.into(),
            params: vec![Ty::Unit; meta.arity as usize],
            ret: Box::new(Ty::Unit),
        },
    }
}

fn format_builtin_type_param_suffix(params: &[&str]) -> String {
    if params.is_empty() {
        String::new()
    } else {
        format!("<{}>", params.join(", "))
    }
}

#[derive(Debug, Clone)]
pub struct ScarCheckpoint {
    env: TypeEnv,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    traits: HashMap<String, TraitInfo>,
    trait_impls: HashMap<(String, String), TraitImplInfo>,
    trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ScarSession {
    env: TypeEnv,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    traits: HashMap<String, TraitInfo>,
    trait_impls: HashMap<(String, String), TraitImplInfo>,
    trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
}

struct CheckerParts {
    env: TypeEnv,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    traits: HashMap<String, TraitInfo>,
    trait_impls: HashMap<(String, String), TraitImplInfo>,
    trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
}

impl ScarSession {
    pub fn new() -> Self {
        Self {
            env: initialize_env(),
            user_func_params: HashMap::new(),
            impl_method_uids: HashMap::new(),
            function_ids_by_name: HashMap::new(),
            traits: HashMap::new(),
            trait_impls: HashMap::new(),
            trait_methods_by_qualified_name: HashMap::new(),
            tyvar_bounds: HashMap::new(),
        }
    }

    pub fn typecheck(&mut self, resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
        self.typecheck_with_context(resolved, TypecheckContext::default())
    }

    pub fn typecheck_with_context(
        &mut self,
        resolved: Vec<Resolved>,
        context: TypecheckContext,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let mut checker = Checker::with_env_and_params(
            self.env.clone(),
            self.user_func_params.clone(),
            self.impl_method_uids.clone(),
            self.function_ids_by_name.clone(),
            self.traits.clone(),
            self.trait_impls.clone(),
            self.trait_methods_by_qualified_name.clone(),
            self.tyvar_bounds.clone(),
            context,
        );
        let typed = checker.check_program(resolved)?;
        let CheckerParts {
            env,
            user_func_params,
            impl_method_uids,
            function_ids_by_name,
            traits,
            trait_impls,
            trait_methods_by_qualified_name,
            tyvar_bounds,
        } = checker.into_parts();
        self.env = env;
        self.user_func_params = user_func_params;
        self.impl_method_uids = impl_method_uids;
        self.function_ids_by_name = function_ids_by_name;
        self.traits = traits;
        self.trait_impls = trait_impls;
        self.trait_methods_by_qualified_name = trait_methods_by_qualified_name;
        self.tyvar_bounds = tyvar_bounds;
        Ok(typed)
    }

    pub fn checkpoint(&self) -> ScarCheckpoint {
        ScarCheckpoint {
            env: self.env.clone(),
            user_func_params: self.user_func_params.clone(),
            impl_method_uids: self.impl_method_uids.clone(),
            function_ids_by_name: self.function_ids_by_name.clone(),
            traits: self.traits.clone(),
            trait_impls: self.trait_impls.clone(),
            trait_methods_by_qualified_name: self.trait_methods_by_qualified_name.clone(),
            tyvar_bounds: self.tyvar_bounds.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: ScarCheckpoint) {
        self.env = checkpoint.env;
        self.user_func_params = checkpoint.user_func_params;
        self.impl_method_uids = checkpoint.impl_method_uids;
        self.function_ids_by_name = checkpoint.function_ids_by_name;
        self.traits = checkpoint.traits;
        self.trait_impls = checkpoint.trait_impls;
        self.trait_methods_by_qualified_name = checkpoint.trait_methods_by_qualified_name;
        self.tyvar_bounds = checkpoint.tyvar_bounds;
    }

    pub fn ensure_next_fun_idx_at_least(&mut self, next_fun_idx: u32) {
        // REPL runtime is the source of truth for currently materialized
        // function indices. Keep Scar aligned exactly so newly inferred
        // callable indices continue to match VM function entries.
        self.env.next_fun_idx = next_fun_idx;
    }
}

impl Default for ScarSession {
    fn default() -> Self {
        Self::new()
    }
}

struct Checker {
    env: TypeEnv,
    function_return_ty: Option<Ty>,
    current_function_symbol: Option<String>,
    current_impl_struct_target: Option<String>,
    in_extractor_body: bool,
    closure_depth: usize,
    lens_bindings: HashMap<u32, TypedLensPath>,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    substitutions: HashMap<u32, Ty>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
    runtime_policy: RuntimeSourcePolicy,
    enforce_builtin_type_contracts: bool,
    seen_builtin_type_decls: HashMap<String, (Vec<String>, Span)>,
    traits: HashMap<String, TraitInfo>,
    trait_impls: HashMap<(String, String), TraitImplInfo>,
    trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    profiler: TypecheckProfiler,
}

impl Checker {
    fn new(context: TypecheckContext) -> Self {
        Self {
            env: initialize_env(),
            function_return_ty: None,
            current_function_symbol: None,
            current_impl_struct_target: None,
            in_extractor_body: false,
            closure_depth: 0,
            lens_bindings: HashMap::new(),
            user_func_params: HashMap::new(),
            impl_method_uids: HashMap::new(),
            function_ids_by_name: HashMap::new(),
            substitutions: HashMap::new(),
            tyvar_bounds: HashMap::new(),
            runtime_policy: context.runtime_policy,
            enforce_builtin_type_contracts: context.enforce_builtin_type_contracts,
            seen_builtin_type_decls: HashMap::new(),
            traits: HashMap::new(),
            trait_impls: HashMap::new(),
            trait_methods_by_qualified_name: HashMap::new(),
            profiler: TypecheckProfiler::new_from_env(),
        }
    }

    fn with_env_and_params(
        env: TypeEnv,
        user_func_params: HashMap<u32, Vec<String>>,
        impl_method_uids: HashMap<String, u32>,
        function_ids_by_name: HashMap<String, ResolvedId>,
        traits: HashMap<String, TraitInfo>,
        trait_impls: HashMap<(String, String), TraitImplInfo>,
        trait_methods_by_qualified_name: HashMap<String, (String, String)>,
        tyvar_bounds: HashMap<u32, Vec<String>>,
        context: TypecheckContext,
    ) -> Self {
        Self {
            env,
            function_return_ty: None,
            current_function_symbol: None,
            current_impl_struct_target: None,
            in_extractor_body: false,
            closure_depth: 0,
            lens_bindings: HashMap::new(),
            user_func_params,
            impl_method_uids,
            function_ids_by_name,
            substitutions: HashMap::new(),
            tyvar_bounds,
            runtime_policy: context.runtime_policy,
            enforce_builtin_type_contracts: context.enforce_builtin_type_contracts,
            seen_builtin_type_decls: HashMap::new(),
            traits,
            trait_impls,
            trait_methods_by_qualified_name,
            profiler: TypecheckProfiler::new_from_env(),
        }
    }

    fn spawn_child_checker(&self, env: TypeEnv) -> Self {
        let mut checker = Checker::with_env_and_params(
            env,
            self.user_func_params.clone(),
            self.impl_method_uids.clone(),
            self.function_ids_by_name.clone(),
            self.traits.clone(),
            self.trait_impls.clone(),
            self.trait_methods_by_qualified_name.clone(),
            self.tyvar_bounds.clone(),
            TypecheckContext {
                runtime_policy: self.runtime_policy.clone(),
                enforce_builtin_type_contracts: self.enforce_builtin_type_contracts,
            },
        );
        checker.function_return_ty = self.function_return_ty.clone();
        checker.current_function_symbol = self.current_function_symbol.clone();
        checker.current_impl_struct_target = self.current_impl_struct_target.clone();
        checker.in_extractor_body = self.in_extractor_body;
        checker.closure_depth = self.closure_depth;
        checker.lens_bindings = self.lens_bindings.clone();
        checker.substitutions = self.substitutions.clone();
        checker.seen_builtin_type_decls = self.seen_builtin_type_decls.clone();
        checker.profiler = self.profiler.clone();
        checker
    }

    fn absorb_child_progress(&mut self, child: &Checker) {
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
        }
        if self.trait_methods_by_qualified_name.len() != child.trait_methods_by_qualified_name.len()
        {
            self.trait_methods_by_qualified_name = child.trait_methods_by_qualified_name.clone();
        }
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

    fn into_parts(self) -> CheckerParts {
        CheckerParts {
            env: self.env,
            user_func_params: self.user_func_params,
            impl_method_uids: self.impl_method_uids,
            function_ids_by_name: self.function_ids_by_name,
            traits: self.traits,
            trait_impls: self.trait_impls,
            trait_methods_by_qualified_name: self.trait_methods_by_qualified_name,
            tyvar_bounds: self.tyvar_bounds,
        }
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
            let t = profile_enabled.then(Instant::now);
            self.predeclare_error_types(&stmts);
            if let Some(start) = t {
                predeclare_error_types_dur = start.elapsed();
            }

            let t = profile_enabled.then(Instant::now);
            self.predeclare_type_signatures(&stmts)?;
            if let Some(start) = t {
                predeclare_type_signatures_dur = start.elapsed();
            }

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

            let t = profile_enabled.then(Instant::now);
            self.ensure_struct_impl_new_contract(&stmts)?;
            if let Some(start) = t {
                ensure_struct_impl_new_contract_dur = start.elapsed();
            }

            let mut typed = Vec::new();
            let t = profile_enabled.then(Instant::now);
            for stmt in stmts {
                stmt_count += 1;
                let stmt_label = profile_enabled.then(|| Self::profile_stmt_label(&stmt));
                let stmt_start = profile_enabled.then(Instant::now);
                if let Resolved::TraitImplDef(span, trait_id, trait_args, target_ty, methods) =
                    &stmt
                {
                    let nodes = self
                        .check_trait_impl_items(span, trait_id, trait_args, target_ty, methods)?;
                    typed.extend(nodes.into_iter().map(|node| self.resolve_typed_node(node)));
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
                typed.push(self.resolve_typed_node(node));
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
            Resolved::If(..) => "If".to_string(),
            Resolved::Ensure(..) => "Ensure".to_string(),
            Resolved::Assert(..) => "Assert".to_string(),
            Resolved::RecoverKind(..) => "RecoverKind".to_string(),
            Resolved::Semi(..) => "Semi".to_string(),
            _ => "Expr".to_string(),
        }
    }

    fn profile_stmt_kind(stmt: &Resolved) -> &'static str {
        match stmt {
            Resolved::Def(..) => "Def",
            Resolved::ExtractorDef(..) => "ExtractorDef",
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
            Resolved::If(..) => "If",
            Resolved::Ensure(..) => "Ensure",
            Resolved::Assert(..) => "Assert",
            Resolved::RecoverKind(..) => "RecoverKind",
            Resolved::Semi(..) => "Semi",
            _ => "Expr",
        }
    }
}
