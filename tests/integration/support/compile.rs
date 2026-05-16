use forge::bytecode::{populate_error_template_lines, Bytecode};
use xldr::CompileSources;

use crate::common::ModuleFixtureCase;

use super::cache::{cached_compile_prefix, load_cached_bytecode, store_cached_bytecode};
use super::sources::{
    collect_module_sources, collect_script_compile_sources,
    compile_chunk_typecheck_context_for_mode, parse_user_program,
};
use super::types::TestCompileMode;

#[allow(dead_code)]
pub fn compile_script(source_name: &str, source: &str) -> Result<Bytecode, String> {
    let compile_sources = collect_script_compile_sources(source_name, source)?;
    compile_script_sources(&compile_sources)
}

#[allow(dead_code)]
pub fn compile_script_sources(compile_sources: &CompileSources) -> Result<Bytecode, String> {
    compile_sources_with_mode(compile_sources, TestCompileMode::Script)
}

#[allow(dead_code)]
pub fn compile_sources_for_module_fixture(
    case: &ModuleFixtureCase,
) -> Result<CompileSources, String> {
    let module_sources = collect_module_sources(&case.module_stages)?;
    Ok(super::sources::compose_script_sources(
        &case.entry_path.to_string_lossy(),
        case.entry_source,
        module_sources,
    ))
}

#[allow(dead_code)]
pub fn compile_module_fixture_case(case: &ModuleFixtureCase) -> Result<Bytecode, String> {
    let compile_sources = compile_sources_for_module_fixture(case)?;
    compile_script_sources(&compile_sources)
}

#[allow(dead_code)]
pub fn compile_project_script(source_name: &str, source: &str) -> Result<Bytecode, String> {
    let compile_sources = collect_script_compile_sources(source_name, source)?;
    compile_project_sources(&compile_sources)
}

#[allow(dead_code)]
pub fn compile_project_sources(compile_sources: &CompileSources) -> Result<Bytecode, String> {
    compile_sources_with_mode(compile_sources, TestCompileMode::Project)
}

pub(super) fn compile_sources_with_mode(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<Bytecode, String> {
    if let Some(bytecode) = load_cached_bytecode(compile_sources, mode)? {
        return Ok(bytecode);
    }

    let compile_prefix = cached_compile_prefix(compile_sources, mode)?;
    let user_ast = parse_user_program(compile_sources, mode)?;
    let (process_stage, user_ast) = xldr::extract_process_modules_from_user_ast(user_ast);
    let mut module_asts = compile_prefix.module_asts.clone();
    if !process_stage.is_empty() {
        module_asts.push(process_stage);
    }
    let declaration_index = if module_asts.len() == compile_prefix.module_asts.len() {
        compile_prefix.declaration_index.clone()
    } else {
        sigil::precollect_declaration_index(&module_asts)
            .map_err(|e| format!("phase=resolve; message={}", e))?
    };
    let docs = xldr::collect_doc_entries(
        &module_asts,
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );
    let signatures = xldr::collect_signature_entries(
        &module_asts,
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );
    let resolved = sigil::resolve_staged_program_from_state(
        &module_asts,
        user_ast,
        &declaration_index,
        Some(compile_sources.user_module_path.clone()),
        compile_prefix.module_asts.len(),
        compile_prefix.resolve_state,
    )
    .map_err(|e| format!("phase=resolve; message={}", e))?;
    let mut scar_session = scar::ScarSession::new();
    scar_session.rollback(compile_prefix.scar_checkpoint.clone());
    let next_fun_idx = compile_prefix
        .bytecode
        .functions
        .iter()
        .map(|entry| entry.fun_idx.saturating_add(1))
        .max()
        .unwrap_or(0);
    scar_session.ensure_next_fun_idx_at_least(next_fun_idx);
    let typed = scar_session
        .typecheck_staged_program_with_context(
            resolved,
            compile_chunk_typecheck_context_for_mode(mode),
        )
        .map_err(|e| format!("phase=typecheck; message={}", e))?;
    let mut forge_session = forge::ForgeSession::from_bytecode(&compile_prefix.bytecode);
    let (chunk, _) = forge_session
        .codegen_chunk_typed_program(typed)
        .map_err(|e| format!("phase=codegen; message={}", e))?;
    let mut bytecode = forge::compose_bytecode_with_chunk(compile_prefix.bytecode.clone(), chunk)
        .map_err(|e| format!("phase=codegen; message={}", e))?;
    let user_source = compile_sources
        .sources
        .source(compile_sources.user_source_id)
        .unwrap_or("");
    populate_error_template_lines(&mut bytecode.error_templates, user_source);
    bytecode.docs = docs;
    bytecode.signatures = signatures;
    store_cached_bytecode(compile_sources, mode, &bytecode)?;
    Ok(bytecode)
}
