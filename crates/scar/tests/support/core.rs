//! Basic standard-prelude helpers shared by Scar tests.
#[path = "cache.rs"]
mod cache;

use std::sync::OnceLock;

use scar::typed::TypedNode;
use scar::{ScarCheckpoint, ScarSession, TypecheckContext};
use sindr::policy::RuntimeSourcePolicy;
use spire::ast::Ast;

pub(crate) struct CachedStdPrelude {
    pub(crate) module_stages: Vec<Vec<sigil::StagedModuleAst>>,
    pub(crate) declaration_index: sigil::DeclarationIndex,
    pub(crate) semantics: CachedStdSemantics,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct CachedStdSemantics {
    pub(crate) process_specs: Vec<sigil::resolved::ResolvedProcessSpec>,
    pub(crate) boot_plan: spire::ast::SupervisorInitSpec,
    pub(crate) resolve_resume_state: sigil::ResolveResumeState,
    pub(crate) checkpoint: ScarCheckpoint,
}

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

fn parse_std_module_stage(source: &str, module_path: Option<&str>) -> Vec<sigil::StagedModuleAst> {
    let ast = spire::parse_with_context(
        source,
        spire::ParserContext::module(
            0,
            module_path
                .filter(|path| *path == "Facet")
                .map(str::to_owned),
        )
        .with_rules(spire::ParseRules::std_module()),
    )
    .unwrap_or_else(|error| panic!("standard source {module_path:?} should parse: {error:?}"));
    let fallback = sigil::const_only_fallback_module_path(&ast, module_path);
    sigil::staged_modules_from_source_ast(ast, fallback)
}

pub(crate) fn std_prefix_cache_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/scar-test-cache")
        .join(format!("{}.prefix", scar_test_cache_key::KEY))
}

pub(crate) fn cached_std_prelude() -> &'static CachedStdPrelude {
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
            semantics,
        }
    })
}

pub(crate) fn session_from_cached_std_prelude() -> ScarSession {
    let prelude = cached_std_prelude();
    let mut session = ScarSession::new();
    session.rollback(prelude.semantics.checkpoint.clone());
    session
}

pub(crate) fn build_std_module_stages(
    overrides: &[(&str, &str)],
) -> Vec<Vec<sigil::StagedModuleAst>> {
    use sindr::stdlib::{stdlib_module_specs, StdlibStage, StdlibVariant};
    let mut stages = vec![Vec::new(), Vec::new()];
    for (name, _) in overrides {
        assert!(
            stdlib_module_specs(StdlibVariant::Default).any(|spec| spec.module_path == Some(*name)),
            "unknown standard module override: {name}"
        );
    }
    for spec in stdlib_module_specs(StdlibVariant::Default) {
        let stage_index = match spec.stage {
            StdlibStage::Bootstrap => 0,
            StdlibStage::Main | StdlibStage::TestExtension => 1,
        };
        let source = spec
            .module_path
            .map(|path| pick_override(path, spec.source, overrides))
            .unwrap_or(spec.source);
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
        prelude.semantics.resolve_resume_state.clone(),
    )
    .map(|resolved| resolved.resolved)
}

pub(crate) fn resolve_with_builtin_prelude(source: &str) -> Vec<sigil::resolved::Resolved> {
    resolve_with_builtin_prelude_result(source).expect("source should resolve")
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
