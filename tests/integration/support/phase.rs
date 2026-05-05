use xldr::CompileSources;

use super::cache::{cached_compile_prefix, cached_module_pipeline};
use super::compile::compile_sources_with_mode;
use super::sources::{
    compile_chunk_typecheck_context_for_mode, compile_unit_kind_for_mode, default_stdlib_snapshot,
    parse_module_stage_suffix, parse_module_stages, parse_user_program, parse_user_source,
};
use super::types::{CompileFailurePhase, TestCompileMode};

fn resolve_sources_in_compile_order(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<(), String> {
    let cached_modules = cached_module_pipeline(compile_sources, mode)?;
    let user_ast = parse_user_program(compile_sources, mode)?;
    let (process_stage, user_ast) = xldr::extract_process_modules_from_user_ast(user_ast);
    let mut module_asts = cached_modules.module_asts.clone();
    if !process_stage.is_empty() {
        module_asts.push(process_stage);
    }
    let declaration_index = if module_asts.len() == cached_modules.module_asts.len() {
        cached_modules.declaration_index.clone()
    } else {
        sigil::precollect_declaration_index(&module_asts)
            .map_err(|e| format!("phase=resolve; message={}", e))?
    };
    let (start_stage_index, resume_state) = if matches!(mode, TestCompileMode::Script) {
        let std_snapshot = default_stdlib_snapshot()?;
        (std_snapshot.default_stage_count, std_snapshot.resolve_state)
    } else {
        let std_resolved = sigil::resolve_staged_program_with_state(
            &cached_modules.module_asts,
            Vec::new(),
            &declaration_index,
            None,
        )
        .map_err(|e| format!("phase=resolve; message={}", e))?;
        (cached_modules.module_asts.len(), std_resolved.resume_state)
    };
    sigil::resolve_staged_program_from_state(
        &module_asts,
        user_ast,
        &declaration_index,
        Some(compile_sources.user_module_path.clone()),
        start_stage_index,
        resume_state,
    )
    .map_err(|e| format!("phase=resolve; message={}", e))?;
    Ok(())
}

fn typecheck_sources_in_compile_order(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<(), String> {
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
    scar_session
        .typecheck_with_context(
            resolved.resolved,
            compile_chunk_typecheck_context_for_mode(mode),
        )
        .map_err(|e| format!("phase=typecheck; message={}", e))?;
    Ok(())
}

#[allow(dead_code)]
pub fn check_script_phase(source_name: &str, source: &str, phase: &str) -> Result<(), String> {
    check_source_phase(source_name, source, TestCompileMode::Script, phase)
}

#[allow(dead_code)]
pub fn check_script_sources_phase(
    compile_sources: &CompileSources,
    phase: &str,
) -> Result<(), String> {
    check_sources_phase(compile_sources, TestCompileMode::Script, phase)
}

#[allow(dead_code)]
pub fn check_project_phase(source_name: &str, source: &str, phase: &str) -> Result<(), String> {
    check_source_phase(source_name, source, TestCompileMode::Project, phase)
}

fn check_source_phase(
    source_name: &str,
    source: &str,
    mode: TestCompileMode,
    phase: &str,
) -> Result<(), String> {
    let phase = CompileFailurePhase::from_str(phase)?;
    match phase {
        CompileFailurePhase::Parse => {
            parse_user_source(source_name, source, mode)?;
            Ok(())
        }
        CompileFailurePhase::Resolve => {
            let compile_sources =
                super::sources::collect_script_compile_sources(source_name, source)?;
            resolve_sources_in_compile_order(&compile_sources, mode)
        }
        CompileFailurePhase::Typecheck => {
            let compile_sources =
                super::sources::collect_script_compile_sources(source_name, source)?;
            typecheck_sources_in_compile_order(&compile_sources, mode)
        }
        CompileFailurePhase::Codegen => {
            let compile_sources =
                super::sources::collect_script_compile_sources(source_name, source)?;
            compile_sources_with_mode(&compile_sources, mode).map(|_| ())
        }
    }
}

fn check_sources_phase(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
    phase: &str,
) -> Result<(), String> {
    let phase = CompileFailurePhase::from_str(phase)?;
    match phase {
        CompileFailurePhase::Parse => {
            if matches!(mode, TestCompileMode::Script) {
                let std_snapshot = default_stdlib_snapshot()?;
                parse_module_stage_suffix(
                    compile_sources,
                    compile_unit_kind_for_mode(mode),
                    std_snapshot.default_stage_count,
                )?;
            } else {
                parse_module_stages(compile_sources, compile_unit_kind_for_mode(mode))?;
            }
            parse_user_program(compile_sources, mode)?;
            Ok(())
        }
        CompileFailurePhase::Resolve => resolve_sources_in_compile_order(compile_sources, mode),
        CompileFailurePhase::Typecheck => typecheck_sources_in_compile_order(compile_sources, mode),
        CompileFailurePhase::Codegen => {
            compile_sources_with_mode(compile_sources, mode).map(|_| ())
        }
    }
}
