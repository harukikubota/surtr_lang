mod core;
pub(crate) use core::*;

use scar::typed::TypedNode;
use scar::{typecheck_with_context as scar_typecheck_with_context, TypecheckContext};
use sindr::policy::RuntimeSourcePolicy;
use spire::ast::Ast;

pub(crate) fn std_module_stages() -> Vec<Vec<sigil::StagedModuleAst>> {
    cached_std_prelude().module_stages.clone()
}

// Only the setup example calls this entry point.
#[allow(dead_code)]
pub(crate) fn prewarm_std_prefix() -> std::path::PathBuf {
    let _ = cached_std_prelude();
    std_prefix_cache_path()
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
        prelude.semantics.resolve_resume_state.clone(),
    )
    .map(|resolved| resolved.resolved)
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
        prelude.semantics.resolve_resume_state,
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
        prelude.semantics.resolve_resume_state,
    )?;

    let mut process_specs = prelude.semantics.process_specs.clone();
    process_specs.append(&mut suffix.process_specs);
    suffix.process_specs = process_specs;

    let mut boot_plan = prelude.semantics.boot_plan.clone();
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
