#![allow(dead_code)]

use forge::bytecode::{populate_error_template_lines, Bytecode};

pub fn collect_script_compile_sources(
    file_name: &str,
    source: &str,
) -> Result<xldr::CompileSources, String> {
    let module_inputs = xldr::collect_additional_default_std_module_inputs()
        .map_err(|e| format!("phase=load; message={}", e))?;
    let module_sources = if module_inputs.is_empty() {
        xldr::collect_module_sources_with_module_stages(&[])
    } else {
        xldr::collect_module_sources_with_std_module_stages(&[module_inputs])
    }
    .map_err(|e| format!("phase=load; message={}", e))?;
    Ok(xldr::compose_script_compile_sources(
        file_name,
        source,
        module_sources,
    ))
}

pub fn parse_script_program(
    compile_sources: &xldr::CompileSources,
) -> Result<(Vec<Vec<sigil::StagedModuleAst>>, Vec<spire::ast::Ast>), String> {
    let sources = &compile_sources.sources;
    let user_source_id = compile_sources.user_source_id;
    let module_stages = xldr::parse_module_stages_from_compile_sources(
        compile_sources,
        spire::CompileUnitKind::Script,
    )
    .map_err(|e| {
        let file_name = sources.file_name(e.source_id).unwrap_or("<unknown>");
        format!("phase=parse; file={}; message={}", file_name, e.message())
    })?;

    let user_source = sources.source(user_source_id).unwrap_or("");
    let user_ast = spire::parse_with_context(
        user_source,
        spire::ParserContext::script(user_source_id.0).with_rules(xldr::derive_source_rules(
            spire::CompileUnitKind::Script,
            xldr::SourceKind::Script,
            None,
        )),
    )
    .map_err(|e| {
        let file_name = sources.file_name(user_source_id).unwrap_or("<unknown>");
        format!("phase=parse; file={}; message={}", file_name, e.message())
    })?;

    Ok((module_stages, user_ast))
}

pub fn compile_script(source_name: &str, source: &str) -> Result<Bytecode, String> {
    let compile_sources = collect_script_compile_sources(source_name, source)?;
    compile_script_sources(&compile_sources)
}

pub fn compile_script_sources(compile_sources: &xldr::CompileSources) -> Result<Bytecode, String> {
    let (module_asts, user_ast) = parse_script_program(compile_sources)?;
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
            source_rules: xldr::derive_source_rules(
                spire::CompileUnitKind::Script,
                xldr::SourceKind::Script,
                None,
            ),
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

pub fn run_script(source_name: &str, source: &str) -> Result<Vec<String>, String> {
    let bytecode = compile_script(source_name, source)?;
    let mut vm = eldr::VM::new(bytecode).with_output_capture();
    vm.run()
        .map_err(|e| format!("phase=runtime; message={}", e))?;
    Ok(vm.output.unwrap_or_default())
}

pub fn run_script_with_stderr(
    source_name: &str,
    source: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let bytecode = compile_script(source_name, source)?;
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
