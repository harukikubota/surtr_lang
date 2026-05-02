use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sigil::resolved::*;
use sindr::builtin::{
    builtin_type_meta_by_name, builtin_uid, BuiltinMeta, BUILTIN_METAS, BUILTIN_TYPE_METAS,
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
    ExtractorReturn,
    ExtractorBody,
    ErrorMarker,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraitMethodInfo {
    id: ResolvedId,
    type_params: Vec<ResolvedTypeParam>,
    params: Vec<ResolvedFunParam>,
    ret_ty: AstTy,
    span: Span,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraitInfo {
    id: ResolvedId,
    type_params: Vec<ResolvedTypeParam>,
    methods: HashMap<String, TraitMethodInfo>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    type_param_vars: Vec<u32>,
    methods: HashMap<String, TraitImplMethodInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ConstKind {
    PrimitiveLiteral,
    LensPath,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum StoredConstValue {
    Literal(Lit),
    LensPath(TypedLensPath),
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

pub fn typecheck_staged_program(
    program: sigil::ResolvedStagedProgram,
) -> Result<TypedProgram, TypeError> {
    let nodes = typecheck_with_context(program.resolved, TypecheckContext::default())?;
    Ok(TypedProgram {
        nodes,
        process_specs: program.process_specs.into_iter().map(Into::into).collect(),
    })
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
    pub allow_error_function_params: bool,
}

impl Default for TypecheckContext {
    fn default() -> Self {
        Self {
            runtime_policy: RuntimeSourcePolicy::script(),
            enforce_builtin_type_contracts: false,
            allow_error_function_params: false,
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
        if self.consume("(") {
            self.skip_ws();
            if self.consume(")") {
                return Ok(Ty::Unit);
            }
            if self.consume("->") {
                let ret = self.parse_type()?;
                self.skip_ws();
                self.expect(")")?;
                return Ok(Ty::Func(Vec::new(), Box::new(ret)));
            }

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
                return Ok(Ty::Func(items, Box::new(ret)));
            }
            self.expect(")")?;
            return if items.len() == 1 {
                Ok(items.pop().expect("single grouped type"))
            } else {
                Ok(Ty::Tuple(items))
            };
        }

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
            return self.build_generic_type(&ident, args);
        }

        self.build_named_type(&ident)
    }

    fn build_named_type(&mut self, ident: &str) -> Result<Ty, String> {
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
            "Lens" => {
                let [source, focus] = args.as_slice() else {
                    return Err("Lens requires exactly 2 type arguments".into());
                };
                Ty::Lens(Box::new(source.clone()), Box::new(focus.clone()))
            }
            "TypeRef" => {
                let [inner] = args.as_slice() else {
                    return Err("TypeRef requires exactly 1 type argument".into());
                };
                Ty::TypeRef(Box::new(inner.clone()))
            }
            other => Ty::Enum(other.to_string(), args),
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

#[cfg(test)]
mod builtin_signature_tests {
    use super::{builtin_ty_from_meta, TypeEnv};
    use crate::types::Ty;
    use sindr::builtin::BUILTIN_METAS;

    #[test]
    fn builtin_meta_signatures_bootstrap_into_type_env() {
        let mut env = TypeEnv::new();
        for meta in BUILTIN_METAS {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScarCheckpoint {
    env: TypeEnv,
    consts: HashMap<u32, ConstMeta>,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    traits: HashMap<String, TraitInfo>,
    trait_impls: HashMap<(String, String), TraitImplInfo>,
    trait_impl_index_by_base_trait: TraitImplIndex,
    trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ScarSession {
    env: TypeEnv,
    consts: HashMap<u32, ConstMeta>,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    traits: HashMap<String, TraitInfo>,
    trait_impls: HashMap<(String, String), TraitImplInfo>,
    trait_impl_index_by_base_trait: TraitImplIndex,
    trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
}

struct CheckerParts {
    env: TypeEnv,
    consts: HashMap<u32, ConstMeta>,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    traits: HashMap<String, TraitInfo>,
    trait_impls: HashMap<(String, String), TraitImplInfo>,
    trait_impl_index_by_base_trait: TraitImplIndex,
    trait_methods_by_qualified_name: HashMap<String, (String, String)>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
}

impl ScarSession {
    pub fn new() -> Self {
        Self {
            env: initialize_env(),
            consts: HashMap::new(),
            user_func_params: HashMap::new(),
            impl_method_uids: HashMap::new(),
            function_ids_by_name: HashMap::new(),
            traits: HashMap::new(),
            trait_impls: HashMap::new(),
            trait_impl_index_by_base_trait: HashMap::new(),
            trait_methods_by_qualified_name: HashMap::new(),
            tyvar_bounds: HashMap::new(),
        }
    }

    pub fn typecheck(&mut self, resolved: Vec<Resolved>) -> Result<Vec<TypedNode>, TypeError> {
        self.typecheck_with_context(resolved, TypecheckContext::default())
    }

    pub fn typecheck_staged_program(
        &mut self,
        program: sigil::ResolvedStagedProgram,
    ) -> Result<TypedProgram, TypeError> {
        let nodes = self.typecheck_with_context(program.resolved, TypecheckContext::default())?;
        Ok(TypedProgram {
            nodes,
            process_specs: program.process_specs.into_iter().map(Into::into).collect(),
        })
    }

    pub fn typecheck_with_context(
        &mut self,
        resolved: Vec<Resolved>,
        context: TypecheckContext,
    ) -> Result<Vec<TypedNode>, TypeError> {
        let mut checker = Checker::with_env_and_params(
            self.env.clone(),
            self.consts.clone(),
            self.user_func_params.clone(),
            self.impl_method_uids.clone(),
            self.function_ids_by_name.clone(),
            self.traits.clone(),
            self.trait_impls.clone(),
            self.trait_impl_index_by_base_trait.clone(),
            self.trait_methods_by_qualified_name.clone(),
            self.tyvar_bounds.clone(),
            context,
        );
        let typed = checker.check_program(resolved)?;
        let CheckerParts {
            env,
            consts,
            user_func_params,
            impl_method_uids,
            function_ids_by_name,
            traits,
            trait_impls,
            trait_impl_index_by_base_trait,
            trait_methods_by_qualified_name,
            tyvar_bounds,
        } = checker.into_parts();
        self.env = env;
        self.consts = consts;
        self.user_func_params = user_func_params;
        self.impl_method_uids = impl_method_uids;
        self.function_ids_by_name = function_ids_by_name;
        self.traits = traits;
        self.trait_impls = trait_impls;
        self.trait_impl_index_by_base_trait = trait_impl_index_by_base_trait;
        self.trait_methods_by_qualified_name = trait_methods_by_qualified_name;
        self.tyvar_bounds = tyvar_bounds;
        Ok(typed)
    }

    pub fn checkpoint(&self) -> ScarCheckpoint {
        ScarCheckpoint {
            env: self.env.clone(),
            consts: self.consts.clone(),
            user_func_params: self.user_func_params.clone(),
            impl_method_uids: self.impl_method_uids.clone(),
            function_ids_by_name: self.function_ids_by_name.clone(),
            traits: self.traits.clone(),
            trait_impls: self.trait_impls.clone(),
            trait_impl_index_by_base_trait: self.trait_impl_index_by_base_trait.clone(),
            trait_methods_by_qualified_name: self.trait_methods_by_qualified_name.clone(),
            tyvar_bounds: self.tyvar_bounds.clone(),
        }
    }

    pub fn rollback(&mut self, checkpoint: ScarCheckpoint) {
        self.env = checkpoint.env;
        self.consts = checkpoint.consts;
        self.user_func_params = checkpoint.user_func_params;
        self.impl_method_uids = checkpoint.impl_method_uids;
        self.function_ids_by_name = checkpoint.function_ids_by_name;
        self.traits = checkpoint.traits;
        self.trait_impls = checkpoint.trait_impls;
        self.trait_impl_index_by_base_trait = checkpoint.trait_impl_index_by_base_trait;
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
    consts: HashMap<u32, ConstMeta>,
    user_func_params: HashMap<u32, Vec<String>>,
    impl_method_uids: HashMap<String, u32>,
    function_ids_by_name: HashMap<String, ResolvedId>,
    substitutions: HashMap<u32, Ty>,
    tyvar_bounds: HashMap<u32, Vec<String>>,
    runtime_policy: RuntimeSourcePolicy,
    enforce_builtin_type_contracts: bool,
    allow_error_function_params: bool,
    seen_builtin_type_decls: HashMap<String, (Vec<String>, Span)>,
    traits: HashMap<String, TraitInfo>,
    trait_impls: HashMap<(String, String), TraitImplInfo>,
    trait_impl_index_by_base_trait: TraitImplIndex,
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
            consts: HashMap::new(),
            user_func_params: HashMap::new(),
            impl_method_uids: HashMap::new(),
            function_ids_by_name: HashMap::new(),
            substitutions: HashMap::new(),
            tyvar_bounds: HashMap::new(),
            runtime_policy: context.runtime_policy,
            enforce_builtin_type_contracts: context.enforce_builtin_type_contracts,
            allow_error_function_params: context.allow_error_function_params,
            seen_builtin_type_decls: HashMap::new(),
            traits: HashMap::new(),
            trait_impls: HashMap::new(),
            trait_impl_index_by_base_trait: HashMap::new(),
            trait_methods_by_qualified_name: HashMap::new(),
            profiler: TypecheckProfiler::new_from_env(),
        }
    }

    fn with_env_and_params(
        env: TypeEnv,
        consts: HashMap<u32, ConstMeta>,
        user_func_params: HashMap<u32, Vec<String>>,
        impl_method_uids: HashMap<String, u32>,
        function_ids_by_name: HashMap<String, ResolvedId>,
        traits: HashMap<String, TraitInfo>,
        trait_impls: HashMap<(String, String), TraitImplInfo>,
        trait_impl_index_by_base_trait: TraitImplIndex,
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
            consts,
            user_func_params,
            impl_method_uids,
            function_ids_by_name,
            substitutions: HashMap::new(),
            tyvar_bounds,
            runtime_policy: context.runtime_policy,
            enforce_builtin_type_contracts: context.enforce_builtin_type_contracts,
            allow_error_function_params: context.allow_error_function_params,
            seen_builtin_type_decls: HashMap::new(),
            traits,
            trait_impls,
            trait_impl_index_by_base_trait,
            trait_methods_by_qualified_name,
            profiler: TypecheckProfiler::new_from_env(),
        }
    }

    fn spawn_child_checker(&self, env: TypeEnv) -> Self {
        let profile = self.profiler.start();
        let mut checker = Checker::with_env_and_params(
            env,
            self.consts.clone(),
            self.user_func_params.clone(),
            self.impl_method_uids.clone(),
            self.function_ids_by_name.clone(),
            self.traits.clone(),
            self.trait_impls.clone(),
            self.trait_impl_index_by_base_trait.clone(),
            self.trait_methods_by_qualified_name.clone(),
            self.tyvar_bounds.clone(),
            TypecheckContext {
                runtime_policy: self.runtime_policy.clone(),
                enforce_builtin_type_contracts: self.enforce_builtin_type_contracts,
                allow_error_function_params: self.allow_error_function_params,
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
        self.profiler
            .finish(ProfileEvent::ChildCheckerSpawn, profile);
        checker
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

    fn into_parts(self) -> CheckerParts {
        CheckerParts {
            env: self.env,
            consts: self.consts,
            user_func_params: self.user_func_params,
            impl_method_uids: self.impl_method_uids,
            function_ids_by_name: self.function_ids_by_name,
            traits: self.traits,
            trait_impls: self.trait_impls,
            trait_impl_index_by_base_trait: self.trait_impl_index_by_base_trait,
            trait_methods_by_qualified_name: self.trait_methods_by_qualified_name,
            tyvar_bounds: self.tyvar_bounds,
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
                if let Resolved::ConstDef(..) = &stmt {
                    continue;
                }
                if let Resolved::TraitImplDef(span, trait_id, trait_args, target_ty, methods) =
                    &stmt
                {
                    let nodes = self
                        .check_trait_impl_items(span, trait_id, trait_args, target_ty, methods)?;
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
            Resolved::RecoverKind(..) => "RecoverKind",
            Resolved::Semi(..) => "Semi",
            _ => "Expr",
        }
    }
}
