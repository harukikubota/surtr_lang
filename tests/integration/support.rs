#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use forge::bytecode::{populate_error_template_lines, Bytecode};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("failed to resolve repository root")
}

fn derive_primary_module_path(source: &str) -> Option<String> {
    let tokens = spire::lexer::tokenize(source).ok()?;
    for (idx, spanned) in tokens.iter().enumerate() {
        if !matches!(spanned.token, spire::token::Token::Defmod) {
            continue;
        }

        let mut j = idx + 1;
        while matches!(
            tokens.get(j).map(|t| &t.token),
            Some(spire::token::Token::Newline)
        ) {
            j += 1;
        }

        let mut segments = Vec::new();
        match tokens.get(j).map(|sp| &sp.token) {
            Some(spire::token::Token::Ident(name)) => {
                segments.push(name.clone());
                j += 1;
            }
            _ => return None,
        }

        while matches!(
            tokens.get(j).map(|t| &t.token),
            Some(spire::token::Token::Colon)
        ) && matches!(
            tokens.get(j + 1).map(|t| &t.token),
            Some(spire::token::Token::Colon)
        ) {
            j += 2;
            match tokens.get(j).map(|sp| &sp.token) {
                Some(spire::token::Token::Ident(name)) => {
                    segments.push(name.clone());
                    j += 1;
                }
                _ => return None,
            }
        }

        return Some(segments.join("::"));
    }

    None
}

fn collect_repo_std_modules() -> Result<Vec<xldr::ModuleInput>, String> {
    let lib_dir = repo_root().join("lib");
    if !lib_dir.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(&lib_dir).map_err(|e| {
        format!(
            "phase=load; message=failed to read `{}`: {}",
            lib_dir.display(),
            e
        )
    })?;
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "srt")
        {
            files.push(path);
        }
    }
    files.sort();

    let mut modules = Vec::new();
    for path in files {
        let source = fs::read_to_string(&path).map_err(|e| {
            format!(
                "phase=load; message=failed to read `{}`: {}",
                path.display(),
                e
            )
        })?;
        let module_path = derive_primary_module_path(&source)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default()
                    .to_string()
            });
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !xldr::is_default_std_module_file_name(file_name)
            && !xldr::is_default_std_module_path(&module_path)
        {
            modules.push(xldr::ModuleInput {
                file_name: path.to_string_lossy().replace('\\', "/"),
                source,
                module_path,
            });
        }
    }

    Ok(modules)
}

pub fn collect_script_compile_sources(
    file_name: &str,
    source: &str,
) -> Result<xldr::CompileSources, String> {
    let module_inputs = collect_repo_std_modules()?;
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
