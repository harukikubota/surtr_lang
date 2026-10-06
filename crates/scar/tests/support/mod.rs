mod cache;

use std::sync::OnceLock;

use scar::typed::TypedNode;
use scar::{
    typecheck_with_context as scar_typecheck_with_context, ScarCheckpoint, ScarSession,
    TypecheckContext,
};
use sindr::policy::RuntimeSourcePolicy;
use spire::ast::Ast;

pub(crate) fn typecheck(
    resolved: Vec<sigil::resolved::Resolved>,
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    typecheck_with_context(resolved, TypecheckContext::default())
}

pub(crate) fn typecheck_with_context(
    resolved: Vec<sigil::resolved::Resolved>,
    context: TypecheckContext,
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    let mut session = session_from_cached_std_prelude();
    session.typecheck_in_place_with_context(resolved, context)
}

fn parse_std_module_stage(source: &str, module_path: &str) -> Vec<sigil::StagedModuleAst> {
    let ast = spire::parse_with_context(
        source,
        spire::ParserContext::module(0, (module_path == "Facet").then(|| module_path.into()))
            .with_rules(spire::ParseRules::std_module()),
    )
    .unwrap_or_else(|error| panic!("standard module {module_path} should parse: {error:?}"));
    let fallback = sigil::const_only_fallback_module_path(&ast, Some(module_path));
    sigil::staged_modules_from_source_ast(ast, fallback)
}

pub(crate) fn std_module_stages() -> Vec<Vec<sigil::StagedModuleAst>> {
    cached_std_prelude().module_stages.clone()
}

struct CachedStdPrelude {
    module_stages: Vec<Vec<sigil::StagedModuleAst>>,
    declaration_index: sigil::DeclarationIndex,
    process_specs: Vec<sigil::resolved::ResolvedProcessSpec>,
    boot_plan: spire::ast::SupervisorInitSpec,
    resolve_resume_state: sigil::ResolveResumeState,
    checkpoint: ScarCheckpoint,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CachedStdSemantics {
    process_specs: Vec<sigil::resolved::ResolvedProcessSpec>,
    boot_plan: spire::ast::SupervisorInitSpec,
    resolve_resume_state: sigil::ResolveResumeState,
    checkpoint: ScarCheckpoint,
}

fn std_prefix_cache_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/scar-test-cache")
        .join(format!("{}.prefix", scar_test_cache_key::KEY))
}

// Only the setup example calls this entry point.
#[allow(dead_code)]
pub(crate) fn prewarm_std_prefix() -> std::path::PathBuf {
    let _ = cached_std_prelude();
    std_prefix_cache_path()
}

fn cached_std_prelude() -> &'static CachedStdPrelude {
    static CACHE: OnceLock<CachedStdPrelude> = OnceLock::new();

    CACHE.get_or_init(|| {
        let module_stages = build_std_module_stages(&[]);
        let declaration_index = sigil::precollect_declaration_index(&module_stages)
            .expect("std modules should precollect");
        let build = || {
            let std_resolved = sigil::resolve_staged_program_with_state(
                &module_stages,
                Vec::new(),
                &declaration_index,
                None,
            )
            .expect("std modules should resolve");
            let mut session = ScarSession::new();
            session
                .typecheck_with_context(
                    std_resolved.resolved,
                    TypecheckContext {
                        runtime_policy: RuntimeSourcePolicy::std_module(),
                        enforce_builtin_type_contracts: true,
                        allow_private_facet_inspection: false,
                    },
                )
                .expect("std modules should typecheck");
            CachedStdSemantics {
                process_specs: std_resolved.process_specs,
                boot_plan: std_resolved.boot_plan,
                resolve_resume_state: std_resolved.resume_state,
                checkpoint: session.checkpoint(),
            }
        };
        let semantics: CachedStdSemantics = match std::env::var_os("SURTR_SCAR_TEST_PREFIX") {
            Some(path) => {
                cache::load_prepared(std::path::Path::new(&path), scar_test_cache_key::KEY)
            }
            None => cache::load_or_build(&std_prefix_cache_path(), scar_test_cache_key::KEY, build),
        };
        CachedStdPrelude {
            module_stages,
            declaration_index,
            process_specs: semantics.process_specs,
            boot_plan: semantics.boot_plan,
            resolve_resume_state: semantics.resolve_resume_state,
            checkpoint: semantics.checkpoint,
        }
    })
}

pub(crate) fn session_from_cached_std_prelude() -> ScarSession {
    let prelude = cached_std_prelude();
    let mut session = ScarSession::new();
    session.rollback(prelude.checkpoint.clone());
    session
}

fn parse_user_module_stage(source: &str) -> Vec<sigil::StagedModuleAst> {
    let ast = spire::parse_with_context(source, spire::ParserContext::module(0, None))
        .expect("definition source should parse");
    sigil::staged_modules_from_source_ast(ast, None)
}

pub(crate) fn std_module_stages_with_overrides(
    overrides: &[(&str, &str)],
) -> Vec<Vec<sigil::StagedModuleAst>> {
    build_std_module_stages(overrides)
}

fn build_std_module_stages(overrides: &[(&str, &str)]) -> Vec<Vec<sigil::StagedModuleAst>> {
    use sindr::stdlib::{stdlib_module_specs, StdlibStage, StdlibVariant};
    let mut stages = vec![Vec::new(), Vec::new()];
    for (name, _) in overrides {
        assert!(
            stdlib_module_specs(StdlibVariant::Default).any(|spec| spec.module_path == *name),
            "unknown standard module override: {name}"
        );
    }
    for spec in stdlib_module_specs(StdlibVariant::Default) {
        let stage_index = match spec.stage {
            StdlibStage::Bootstrap => 0,
            StdlibStage::Main | StdlibStage::TestExtension => 1,
        };
        let source = pick_override(spec.module_path, spec.source, overrides);
        stages[stage_index].extend(parse_std_module_stage(source, spec.module_path));
    }
    stages
}

pub(crate) fn resolve_with_builtin_prelude_result(
    source: &str,
) -> Result<Vec<sigil::resolved::Resolved>, sigil::error::ResolveError> {
    let user_ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse");
    resolve_ast_with_builtin_prelude(user_ast)
}

pub(crate) fn resolve_ast_with_builtin_prelude(
    user_ast: Vec<Ast>,
) -> Result<Vec<sigil::resolved::Resolved>, sigil::error::ResolveError> {
    let prelude = cached_std_prelude();
    sigil::resolve_staged_program_from_state(
        &prelude.module_stages,
        user_ast,
        &prelude.declaration_index,
        None,
        prelude.module_stages.len(),
        prelude.resolve_resume_state.clone(),
    )
    .map(|resolved| resolved.resolved)
}

pub(crate) fn resolve_program_with_builtin_prelude(source: &str) -> Vec<sigil::resolved::Resolved> {
    let prelude = cached_std_prelude();
    let user_ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse");
    sigil::resolve_staged_program(
        &prelude.module_stages,
        user_ast,
        &prelude.declaration_index,
        None,
    )
    .expect("source should resolve")
}

pub(crate) fn resolve_with_builtin_prelude_in_script_module(
    source: &str,
) -> Result<Vec<sigil::resolved::Resolved>, sigil::error::ResolveError> {
    resolve_with_builtin_prelude_in_module(source, "__Script::fixture")
}

pub(crate) fn resolve_with_builtin_prelude_in_module(
    source: &str,
    module_path: &str,
) -> Result<Vec<sigil::resolved::Resolved>, sigil::error::ResolveError> {
    let prelude = cached_std_prelude();
    let user_ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse");
    sigil::resolve_staged_program_from_state(
        &prelude.module_stages,
        user_ast,
        &prelude.declaration_index,
        Some(module_path.to_owned()),
        prelude.module_stages.len(),
        prelude.resolve_resume_state.clone(),
    )
    .map(|resolved| resolved.resolved)
}

pub(crate) fn resolve_with_builtin_prelude(source: &str) -> Vec<sigil::resolved::Resolved> {
    resolve_with_builtin_prelude_result(source).expect("source should resolve")
}

pub(crate) fn typecheck_with_builtin_prelude(source: &str) -> Vec<TypedNode> {
    let resolved = resolve_with_builtin_prelude(source);
    typecheck(resolved).expect("source should typecheck")
}

pub(crate) fn typecheck_with_builtin_prelude_in_script_module(source: &str) -> Vec<TypedNode> {
    let resolved = resolve_with_builtin_prelude_in_script_module(source)
        .expect("source should resolve inside script module");
    typecheck(resolved).expect("source should typecheck inside script module")
}

pub(crate) fn typecheck_with_rules(
    source: &str,
    runtime_policy: RuntimeSourcePolicy,
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    let resolved = resolve_with_builtin_prelude(source);
    typecheck_with_context(
        resolved,
        TypecheckContext {
            runtime_policy,
            enforce_builtin_type_contracts: false,
            allow_private_facet_inspection: false,
        },
    )
}

pub(crate) fn typecheck_module_source_result(source: &str) -> Result<Vec<TypedNode>, String> {
    let prelude = cached_std_prelude();
    let mut module_stages = prelude.module_stages.clone();
    module_stages.push(parse_user_module_stage(source));
    let declaration_index = sigil::precollect_declaration_index(&module_stages)
        .map_err(|err| format!("resolve precollect failed: {}", err.message))?;
    let resolved = sigil::resolve_staged_program_from_state(
        &module_stages,
        Vec::new(),
        &declaration_index,
        None,
        prelude.module_stages.len(),
        prelude.resolve_resume_state,
    )
    .map_err(|err| format!("resolve failed: {}", err.message))?;
    let mut session = session_from_cached_std_prelude();
    session
        .typecheck_staged_program_in_place_with_context(resolved, TypecheckContext::default())
        .map(|program| program.nodes)
        .map_err(|err| err.message)
}

pub(crate) fn typecheck_resolved_program_suffix_with_builtin_prelude(
    resolved: sigil::ResolvedStagedProgram,
) -> Result<scar::typed::TypedProgram, scar::error::TypeError> {
    let mut session = session_from_cached_std_prelude();
    session.typecheck_staged_program_in_place_with_context(resolved, TypecheckContext::default())
}

pub(crate) fn resolve_staged_program_suffix_with_builtin_prelude(
    module_stages: &[Vec<sigil::StagedModuleAst>],
    user_ast: Vec<Ast>,
    declaration_index: &sigil::DeclarationIndex,
    user_module_path: Option<String>,
) -> Result<sigil::ResolvedStagedProgram, sigil::error::ResolveError> {
    let prelude = cached_std_prelude();
    assert!(
        module_stages.len() >= prelude.module_stages.len(),
        "staged program is shorter than the cached std prefix"
    );
    let mut suffix = sigil::resolve_staged_program_from_state(
        module_stages,
        user_ast,
        declaration_index,
        user_module_path,
        prelude.module_stages.len(),
        prelude.resolve_resume_state,
    )?;

    let mut process_specs = prelude.process_specs.clone();
    process_specs.append(&mut suffix.process_specs);
    suffix.process_specs = process_specs;

    let mut boot_plan = prelude.boot_plan.clone();
    boot_plan.entries.append(&mut suffix.boot_plan.entries);
    boot_plan
        .singletons
        .append(&mut suffix.boot_plan.singletons);
    boot_plan
        .supervisors
        .append(&mut suffix.boot_plan.supervisors);
    suffix.boot_plan = boot_plan;

    Ok(suffix)
}

pub(crate) fn typecheck_std_modules_with_overrides(
    overrides: &[(&str, &str)],
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    let module_stages = std_module_stages_with_overrides(overrides);
    let declaration_index =
        sigil::precollect_declaration_index(&module_stages).expect("std modules should precollect");
    let resolved =
        sigil::resolve_staged_program(&module_stages, Vec::new(), &declaration_index, None)
            .expect("std modules should resolve");
    scar_typecheck_with_context(
        resolved,
        TypecheckContext {
            runtime_policy: RuntimeSourcePolicy::std_module(),
            enforce_builtin_type_contracts: true,
            allow_private_facet_inspection: false,
        },
    )
}

fn pick_override<'a>(
    name: &str,
    default_source: &'a str,
    overrides: &[(&str, &'a str)],
) -> &'a str {
    overrides
        .iter()
        .find(|(override_name, _)| *override_name == name)
        .map(|(_, source)| *source)
        .unwrap_or(default_source)
}
