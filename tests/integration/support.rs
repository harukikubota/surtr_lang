use forge::bytecode::{populate_error_template_lines, Bytecode};
use spire::CompileUnitKind;
use xldr::{CompileSources, ModuleInput, ModuleSources, SourceKind};

#[derive(Clone, Copy)]
enum TestCompileMode {
    Script,
    Project,
}

#[allow(dead_code)]
pub fn collect_default_module_sources() -> Result<ModuleSources, String> {
    let module_inputs = xldr::collect_additional_default_std_module_inputs()
        .map_err(|e| format!("phase=load; message={}", e))?;
    if module_inputs.is_empty() {
        xldr::collect_module_sources_with_module_stages(&[])
    } else {
        xldr::collect_module_sources_with_std_module_stages(&[module_inputs])
    }
    .map_err(|e| format!("phase=load; message={}", e))
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

#[allow(dead_code)]
pub fn parse_script_program(
    compile_sources: &CompileSources,
) -> Result<(Vec<Vec<sigil::StagedModuleAst>>, Vec<spire::ast::Ast>), String> {
    parse_program(compile_sources, TestCompileMode::Script)
}

#[allow(dead_code)]
pub fn parse_project_program(
    compile_sources: &CompileSources,
) -> Result<(Vec<Vec<sigil::StagedModuleAst>>, Vec<spire::ast::Ast>), String> {
    parse_program(compile_sources, TestCompileMode::Project)
}

fn parse_program(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<(Vec<Vec<sigil::StagedModuleAst>>, Vec<spire::ast::Ast>), String> {
    let sources = &compile_sources.sources;
    let user_source_id = compile_sources.user_source_id;
    let compile_unit_kind = match mode {
        TestCompileMode::Script => CompileUnitKind::Script,
        TestCompileMode::Project => CompileUnitKind::Project,
    };
    let module_stages = parse_module_stages(compile_sources, compile_unit_kind)?;

    let user_source = sources.source(user_source_id).unwrap_or("");
    let user_ast = match mode {
        TestCompileMode::Script => {
            spire::parse_with_context(
                user_source,
                spire::ParserContext::script(user_source_id.0).with_rules(
                    xldr::derive_source_rules(CompileUnitKind::Script, SourceKind::Script, None),
                ),
            )
        }
        TestCompileMode::Project => {
            spire::parse_with_context(user_source, spire::ParserContext::project(user_source_id.0))
        }
    }
    .map_err(|e| {
        let file_name = sources.file_name(user_source_id).unwrap_or("<unknown>");
        format!("phase=parse; file={}; message={}", file_name, e.message())
    })?;

    Ok((module_stages, user_ast))
}

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
pub fn compile_project_script(source_name: &str, source: &str) -> Result<Bytecode, String> {
    let compile_sources = collect_script_compile_sources(source_name, source)?;
    compile_project_sources(&compile_sources)
}

#[allow(dead_code)]
pub fn compile_project_sources(compile_sources: &CompileSources) -> Result<Bytecode, String> {
    compile_sources_with_mode(compile_sources, TestCompileMode::Project)
}

fn compile_sources_with_mode(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<Bytecode, String> {
    let (module_asts, user_ast) = match mode {
        TestCompileMode::Script => parse_script_program(compile_sources)?,
        TestCompileMode::Project => parse_project_program(compile_sources)?,
    };
    let docs = xldr::collect_doc_entries(
        &module_asts,
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );
    let declaration_index = sigil::precollect_declaration_index(&module_asts)
        .map_err(|e| format!("phase=resolve; message={}", e))?;
    let resolved = sigil::resolve_staged_program(
        &module_asts,
        user_ast,
        &declaration_index,
        Some(compile_sources.user_module_path.clone()),
    )
    .map_err(|e| format!("phase=resolve; message={}", e))?;
    let typed = scar::typecheck_with_context(
        resolved,
        scar::TypecheckContext {
            source_rules: match mode {
                TestCompileMode::Script => {
                    xldr::derive_source_rules(CompileUnitKind::Script, SourceKind::Script, None)
                }
                TestCompileMode::Project => spire::SourceRules::project(),
            },
            enforce_builtin_type_contracts: true,
        },
    )
    .map_err(|e| format!("phase=typecheck; message={}", e))?;
    let mut bytecode =
        forge::codegen(typed).map_err(|e| format!("phase=codegen; message={}", e))?;
    let user_source = compile_sources
        .sources
        .source(compile_sources.user_source_id)
        .unwrap_or("");
    populate_error_template_lines(&mut bytecode.error_templates, user_source);
    bytecode.docs = docs;
    Ok(bytecode)
}

#[allow(dead_code)]
pub fn run_script(source_name: &str, source: &str) -> Result<Vec<String>, String> {
    let (stdout, _stderr) = run_script_with_stderr(source_name, source)?;
    Ok(stdout)
}

#[allow(dead_code)]
pub fn run_project_script(source_name: &str, source: &str) -> Result<Vec<String>, String> {
    let (stdout, _stderr) = run_project_script_with_stderr(source_name, source)?;
    Ok(stdout)
}

#[allow(dead_code)]
pub fn run_script_with_stderr(
    source_name: &str,
    source: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let bytecode = compile_script(source_name, source)?;
    run_bytecode_with_stderr(bytecode)
}

#[allow(dead_code)]
pub fn run_project_script_with_stderr(
    source_name: &str,
    source: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let bytecode = compile_project_script(source_name, source)?;
    run_bytecode_with_stderr(bytecode)
}

fn run_bytecode_with_stderr(bytecode: Bytecode) -> Result<(Vec<String>, Vec<String>), String> {
    let mut vm = eldr::VM::new(bytecode)
        .with_output_capture()
        .with_error_capture();
    vm.run()
        .map_err(|e| format!("phase=runtime; message={}", e))?;
    Ok((
        vm.output.unwrap_or_default(),
        vm.error_output.unwrap_or_default(),
    ))
}
