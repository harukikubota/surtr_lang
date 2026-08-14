use std::sync::{Arc, OnceLock};

use sindr::policy::{CompileUnitKind, RuntimeSourcePolicy};
use xldr::{CompileSources, ModuleInput, ModuleSources, SourceKind};

use super::types::TestCompileMode;

#[allow(dead_code)]
pub fn collect_default_module_sources() -> Result<ModuleSources, String> {
    default_module_sources()
}

#[allow(dead_code)]
pub fn collect_module_sources(module_stages: &[Vec<ModuleInput>]) -> Result<ModuleSources, String> {
    xldr::collect_module_sources_with_module_stages(module_stages)
        .map_err(|e| format!("phase=load; message={}", e))
}

#[allow(dead_code)]
pub fn compose_script_sources(
    file_name: &str,
    source: &str,
    module_sources: ModuleSources,
) -> CompileSources {
    xldr::compose_script_compile_sources(file_name, source, module_sources)
}

#[allow(dead_code)]
pub fn collect_script_compile_sources(
    file_name: &str,
    source: &str,
) -> Result<CompileSources, String> {
    let module_sources = collect_default_module_sources()?;
    Ok(compose_script_sources(file_name, source, module_sources))
}

#[allow(dead_code)]
pub fn parse_module_stages(
    compile_sources: &CompileSources,
    compile_unit_kind: CompileUnitKind,
) -> Result<Vec<Vec<sigil::StagedModuleAst>>, String> {
    let sources = &compile_sources.sources;
    xldr::parse_module_stages_from_compile_sources(compile_sources, compile_unit_kind).map_err(
        |e| {
            let file_name = sources.file_name(e.source_id).unwrap_or("<unknown>");
            format!("phase=parse; file={}; message={}", file_name, e.message())
        },
    )
}

pub(super) fn parse_module_stage_suffix(
    compile_sources: &CompileSources,
    compile_unit_kind: CompileUnitKind,
    start_stage_index: usize,
) -> Result<Vec<Vec<sigil::StagedModuleAst>>, String> {
    let sources = &compile_sources.sources;
    xldr::parse_module_stages_from_compile_sources_suffix(
        compile_sources,
        compile_unit_kind,
        start_stage_index,
    )
    .map_err(|e| {
        let file_name = sources.file_name(e.source_id).unwrap_or("<unknown>");
        format!("phase=parse; file={}; message={}", file_name, e.message())
    })
}

pub(super) fn default_stdlib_snapshot() -> Result<Arc<xldr::DefaultStdlibSnapshot>, String> {
    xldr::default_stdlib_semantic_snapshot()
        .map_err(|e| format!("phase=load; message=failed to load stdlib snapshot: {}", e))
}

fn default_module_sources() -> Result<ModuleSources, String> {
    static DEFAULT_MODULE_SOURCES: OnceLock<Result<ModuleSources, String>> = OnceLock::new();

    DEFAULT_MODULE_SOURCES
        .get_or_init(|| {
            let module_inputs = xldr::cached_additional_default_std_module_inputs()
                .map_err(|e| format!("phase=load; message={}", e))?;
            xldr::collect_module_sources_with_module_stages(&[module_inputs])
                .map_err(|e| format!("phase=load; message={}", e))
        })
        .clone()
}

pub(super) fn compile_unit_kind_for_mode(mode: TestCompileMode) -> CompileUnitKind {
    match mode {
        TestCompileMode::Script => CompileUnitKind::Script,
        TestCompileMode::Project => CompileUnitKind::Project,
    }
}

pub(super) fn typecheck_context_for_mode(mode: TestCompileMode) -> scar::TypecheckContext {
    scar::TypecheckContext {
        runtime_policy: match mode {
            TestCompileMode::Script => {
                xldr::derive_runtime_policy(CompileUnitKind::Script, SourceKind::Script, None)
            }
            TestCompileMode::Project => RuntimeSourcePolicy::project(),
        },
        enforce_builtin_type_contracts: true,
        allow_error_function_params: false,
        allow_private_facet_inspection: false,
    }
}

pub(super) fn compile_chunk_typecheck_context_for_mode(
    mode: TestCompileMode,
) -> scar::TypecheckContext {
    scar::TypecheckContext {
        enforce_builtin_type_contracts: false,
        ..typecheck_context_for_mode(mode)
    }
}

pub(super) fn std_typecheck_context_for_mode(mode: TestCompileMode) -> scar::TypecheckContext {
    scar::TypecheckContext {
        runtime_policy: xldr::derive_runtime_policy(
            compile_unit_kind_for_mode(mode),
            SourceKind::StdDefinitionSource,
            None,
        ),
        enforce_builtin_type_contracts: true,
        allow_error_function_params: true,
        allow_private_facet_inspection: false,
    }
}

pub(super) fn parse_user_source(
    source_name: &str,
    source: &str,
    mode: TestCompileMode,
) -> Result<Vec<spire::ast::Ast>, String> {
    let user_ast = match mode {
        TestCompileMode::Script => spire::parse_with_context(
            source,
            spire::ParserContext::script(0)
                .with_rules(xldr::derive_parse_rules(SourceKind::Script)),
        ),
        TestCompileMode::Project => {
            spire::parse_with_context(source, spire::ParserContext::project(0))
        }
    }
    .map_err(|e| format!("phase=parse; file={}; message={}", source_name, e.message()))?;

    Ok(user_ast)
}

pub(super) fn parse_user_program(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<Vec<spire::ast::Ast>, String> {
    let sources = &compile_sources.sources;
    let user_source_id = compile_sources.user_source_id;
    let source_name = sources.file_name(user_source_id).unwrap_or("<unknown>");
    let user_source = sources.source(user_source_id).unwrap_or("");
    parse_user_source(source_name, user_source, mode)
}
