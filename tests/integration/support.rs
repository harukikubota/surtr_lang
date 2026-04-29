use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use forge::bytecode::{populate_error_template_lines, Bytecode};
use sindr::policy::{CompileUnitKind, RuntimeSourcePolicy};
use xldr::{CompileSources, ModuleInput, ModuleSources, SourceKind};

#[derive(Clone, Copy)]
enum TestCompileMode {
    Script,
    Project,
}

#[derive(Clone, Copy)]
enum CompileFailurePhase {
    Parse,
    Resolve,
    Typecheck,
    Codegen,
}

impl CompileFailurePhase {
    fn from_str(phase: &str) -> Result<Self, String> {
        match phase {
            "parse" => Ok(Self::Parse),
            "resolve" => Ok(Self::Resolve),
            "typecheck" => Ok(Self::Typecheck),
            "codegen" => Ok(Self::Codegen),
            other => Err(format!(
                "phase=test; message=unsupported compile-error phase `{}`",
                other
            )),
        }
    }
}

#[derive(Clone)]
struct CachedModulePipeline {
    module_asts: Vec<Vec<sigil::StagedModuleAst>>,
    declaration_index: sigil::DeclarationIndex,
}

struct CachedPhaseSessions {
    sigil_session: sigil::SigilSession,
    scar_session: scar::ScarSession,
}

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

fn default_module_sources() -> Result<ModuleSources, String> {
    static DEFAULT_MODULE_SOURCES: OnceLock<Result<ModuleSources, String>> = OnceLock::new();

    DEFAULT_MODULE_SOURCES
        .get_or_init(|| {
            let module_inputs = xldr::collect_additional_default_std_module_inputs()
                .map_err(|e| format!("phase=load; message={}", e))?;
            xldr::collect_module_sources_with_module_stages(&[module_inputs])
                .map_err(|e| format!("phase=load; message={}", e))
        })
        .clone()
}

fn compile_unit_kind_for_mode(mode: TestCompileMode) -> CompileUnitKind {
    match mode {
        TestCompileMode::Script => CompileUnitKind::Script,
        TestCompileMode::Project => CompileUnitKind::Project,
    }
}

fn typecheck_context_for_mode(mode: TestCompileMode) -> scar::TypecheckContext {
    scar::TypecheckContext {
        runtime_policy: match mode {
            TestCompileMode::Script => {
                xldr::derive_runtime_policy(CompileUnitKind::Script, SourceKind::Script, None)
            }
            TestCompileMode::Project => RuntimeSourcePolicy::project(),
        },
        enforce_builtin_type_contracts: true,
    }
}

fn std_typecheck_context_for_mode(mode: TestCompileMode) -> scar::TypecheckContext {
    scar::TypecheckContext {
        runtime_policy: xldr::derive_runtime_policy(
            compile_unit_kind_for_mode(mode),
            SourceKind::StdModule,
            None,
        ),
        enforce_builtin_type_contracts: true,
    }
}

fn parse_user_source(
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

fn parse_user_program(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<Vec<spire::ast::Ast>, String> {
    let sources = &compile_sources.sources;
    let user_source_id = compile_sources.user_source_id;
    let source_name = sources.file_name(user_source_id).unwrap_or("<unknown>");
    let user_source = sources.source(user_source_id).unwrap_or("");
    parse_user_source(source_name, user_source, mode)
}

fn stable_hash_bytes(bytes: &[u8]) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

fn test_binary_fingerprint() -> Result<String, String> {
    static TEST_BINARY_FINGERPRINT: OnceLock<Result<String, String>> = OnceLock::new();

    TEST_BINARY_FINGERPRINT
        .get_or_init(|| {
            let exe = env::current_exe()
                .map_err(|e| format!("phase=cache; message=failed to locate current exe: {}", e))?;
            let bytes = fs::read(&exe).map_err(|e| {
                format!(
                    "phase=cache; message=failed to read test binary {}: {}",
                    exe.display(),
                    e
                )
            })?;
            Ok(stable_hash_bytes(&bytes))
        })
        .clone()
}

fn fixture_cache_enabled() -> bool {
    matches!(
        env::var("SURTR_TEST_CACHE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

fn fixture_cache_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-fixture-cache/eldr")
}

fn module_pipeline_cache_key(compile_sources: &CompileSources, mode: TestCompileMode) -> String {
    let mut key = format!("{:?}", compile_unit_kind_for_mode(mode));

    for stage in &compile_sources.module_stages {
        key.push('|');
        for module in stage {
            let file_name = compile_sources
                .sources
                .file_name(module.source_id)
                .unwrap_or("<unknown>");
            let source = compile_sources
                .sources
                .source(module.source_id)
                .unwrap_or("");
            let source_kind = match module.source_kind {
                SourceKind::Script => "script",
                SourceKind::Module => "module",
                SourceKind::StdModule => "std",
                SourceKind::ReplChunk => "repl",
            };

            key.push_str(file_name);
            key.push('\x1f');
            key.push_str(&module.module_path);
            key.push('\x1f');
            key.push_str(source_kind);
            key.push('\x1f');
            key.push_str(&forge::bytecode::stable_hash_hex(source));
            key.push('\x1e');
        }
    }

    key
}

fn compile_sources_cache_key(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<String, String> {
    let test_binary = test_binary_fingerprint()?;
    let user_file_name = compile_sources
        .sources
        .file_name(compile_sources.user_source_id)
        .unwrap_or("<unknown>");
    let user_source = compile_sources
        .sources
        .source(compile_sources.user_source_id)
        .unwrap_or("");

    let mut key = String::new();
    key.push_str("v1");
    key.push('\x1f');
    key.push_str(&test_binary);
    key.push('\x1f');
    key.push_str(&format!("{:?}", compile_unit_kind_for_mode(mode)));
    key.push('\x1f');
    key.push_str(user_file_name);
    key.push('\x1f');
    key.push_str(&compile_sources.user_module_path);
    key.push('\x1f');
    key.push_str(&forge::bytecode::stable_hash_hex(user_source));
    key.push('\x1f');
    key.push_str(&module_pipeline_cache_key(compile_sources, mode));

    Ok(forge::bytecode::stable_hash_hex(&key))
}

fn cached_eldr_path(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<PathBuf, String> {
    let key = compile_sources_cache_key(compile_sources, mode)?;
    Ok(fixture_cache_root().join(format!("{key}.eldr")))
}

fn load_cached_bytecode(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<Option<Bytecode>, String> {
    if !fixture_cache_enabled() {
        return Ok(None);
    }

    let cache_path = cached_eldr_path(compile_sources, mode)?;
    if !cache_path.exists() {
        return Ok(None);
    }

    let bytes = match fs::read(&cache_path) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };

    match Bytecode::decode(&bytes) {
        Ok(bytecode) => Ok(Some(bytecode)),
        Err(_) => {
            let _ = fs::remove_file(&cache_path);
            Ok(None)
        }
    }
}

fn store_cached_bytecode(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
    bytecode: &Bytecode,
) -> Result<(), String> {
    if !fixture_cache_enabled() {
        return Ok(());
    }

    let cache_path = cached_eldr_path(compile_sources, mode)?;
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            format!(
                "phase=cache; message=failed to create cache dir {}: {}",
                parent.display(),
                e
            )
        })?;
    }

    let bytes = bytecode
        .encode()
        .map_err(|e| format!("phase=cache; message=failed to encode .eldr cache: {}", e))?;
    let temp_path = cache_path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp_path, bytes).map_err(|e| {
        format!(
            "phase=cache; message=failed to write cache file {}: {}",
            temp_path.display(),
            e
        )
    })?;
    fs::rename(&temp_path, &cache_path)
        .or_else(|_| {
            fs::copy(&temp_path, &cache_path)
                .map(|_| ())
                .and_then(|_| fs::remove_file(&temp_path))
        })
        .map_err(|e| {
            format!(
                "phase=cache; message=failed to finalize cache file {}: {}",
                cache_path.display(),
                e
            )
        })?;

    Ok(())
}

fn cached_module_pipeline(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<CachedModulePipeline, String> {
    static MODULE_PIPELINE_CACHE: OnceLock<
        Mutex<HashMap<String, Result<CachedModulePipeline, String>>>,
    > = OnceLock::new();

    let cache = MODULE_PIPELINE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cache_key = module_pipeline_cache_key(compile_sources, mode);

    if let Some(cached) = cache
        .lock()
        .expect("module pipeline cache poisoned")
        .get(&cache_key)
    {
        return cached.clone();
    }

    let module_asts = parse_module_stages(compile_sources, compile_unit_kind_for_mode(mode))?;
    let declaration_index = sigil::precollect_declaration_index(&module_asts)
        .map_err(|e| format!("phase=resolve; message={}", e))?;
    let pipeline = CachedModulePipeline {
        module_asts,
        declaration_index,
    };

    cache
        .lock()
        .expect("module pipeline cache poisoned")
        .insert(cache_key, Ok(pipeline.clone()));

    Ok(pipeline)
}

fn phase_session_cache_key(compile_sources: &CompileSources, mode: TestCompileMode) -> String {
    let mut key = module_pipeline_cache_key(compile_sources, mode);
    key.push('\x1f');
    key.push_str(&compile_sources.user_module_path);
    key
}

fn cached_phase_sessions(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<Arc<Mutex<CachedPhaseSessions>>, String> {
    static PHASE_SESSION_CACHE: OnceLock<
        Mutex<HashMap<String, Result<Arc<Mutex<CachedPhaseSessions>>, String>>>,
    > = OnceLock::new();

    let cache = PHASE_SESSION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cache_key = phase_session_cache_key(compile_sources, mode);

    if let Some(cached) = cache
        .lock()
        .expect("phase session cache poisoned")
        .get(&cache_key)
    {
        return cached.clone();
    }

    let cached_modules = cached_module_pipeline(compile_sources, mode)?;
    let std_resolved = sigil::resolve_staged_program(
        &cached_modules.module_asts,
        Vec::new(),
        &cached_modules.declaration_index,
        None,
    )
    .map_err(|e| format!("phase=resolve; message={}", e))?;

    let mut scar_session = scar::ScarSession::new();
    scar_session
        .typecheck_with_context(std_resolved, std_typecheck_context_for_mode(mode))
        .map_err(|e| format!("phase=typecheck; message={}", e))?;

    let scope = sigil::build_scope_for_module(
        &cached_modules.module_asts,
        Some(compile_sources.user_module_path.as_str()),
        cached_modules.module_asts.len(),
    )
    .map_err(|e| format!("phase=resolve; message={}", e))?;
    let mut sigil_session =
        sigil::SigilSession::with_module_path(Some(compile_sources.user_module_path.clone()));
    sigil_session.replace_scope_with_declarations(scope, &cached_modules.declaration_index);

    let sessions = Arc::new(Mutex::new(CachedPhaseSessions {
        sigil_session,
        scar_session,
    }));

    cache
        .lock()
        .expect("phase session cache poisoned")
        .insert(cache_key, Ok(Arc::clone(&sessions)));
    Ok(sessions)
}

fn resolve_sources_with_mode(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<(), String> {
    let user_ast = parse_user_program(compile_sources, mode)?;
    let sessions = cached_phase_sessions(compile_sources, mode)?;
    let mut sessions = sessions
        .lock()
        .map_err(|_| "phase=resolve; message=phase session cache poisoned".to_string())?;
    let sigil_checkpoint = sessions.sigil_session.checkpoint();
    let resolved_result = sessions
        .sigil_session
        .resolve(user_ast)
        .map_err(|e| format!("phase=resolve; message={}", e));
    sessions.sigil_session.rollback(sigil_checkpoint);
    resolved_result?;
    Ok(())
}

fn typecheck_sources_with_mode(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<(), String> {
    let user_ast = parse_user_program(compile_sources, mode)?;
    let sessions = cached_phase_sessions(compile_sources, mode)?;
    let mut sessions = sessions
        .lock()
        .map_err(|_| "phase=typecheck; message=phase session cache poisoned".to_string())?;
    let sigil_checkpoint = sessions.sigil_session.checkpoint();
    let scar_checkpoint = sessions.scar_session.checkpoint();
    let resolved_result = sessions
        .sigil_session
        .resolve(user_ast)
        .map_err(|e| format!("phase=resolve; message={}", e));
    let typecheck_result = match resolved_result {
        Ok(resolved) => sessions
            .scar_session
            .typecheck_with_context(resolved, typecheck_context_for_mode(mode))
            .map(|_| ())
            .map_err(|e| format!("phase=typecheck; message={}", e)),
        Err(e) => Err(e),
    };
    sessions.sigil_session.rollback(sigil_checkpoint);
    sessions.scar_session.rollback(scar_checkpoint);
    typecheck_result?;
    Ok(())
}

#[allow(dead_code)]
pub fn check_script_phase(source_name: &str, source: &str, phase: &str) -> Result<(), String> {
    check_source_phase(source_name, source, TestCompileMode::Script, phase)
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
            let compile_sources = collect_script_compile_sources(source_name, source)?;
            resolve_sources_with_mode(&compile_sources, mode)
        }
        CompileFailurePhase::Typecheck => {
            let compile_sources = collect_script_compile_sources(source_name, source)?;
            typecheck_sources_with_mode(&compile_sources, mode)
        }
        CompileFailurePhase::Codegen => {
            let compile_sources = collect_script_compile_sources(source_name, source)?;
            compile_sources_with_mode(&compile_sources, mode).map(|_| ())
        }
    }
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
    if let Some(bytecode) = load_cached_bytecode(compile_sources, mode)? {
        return Ok(bytecode);
    }

    let cached_modules = cached_module_pipeline(compile_sources, mode)?;
    let user_ast = parse_user_program(compile_sources, mode)?;
    let docs = xldr::collect_doc_entries(
        &cached_modules.module_asts,
        &user_ast,
        Some(compile_sources.user_module_path.as_str()),
    );
    let resolved = sigil::resolve_staged_program(
        &cached_modules.module_asts,
        user_ast,
        &cached_modules.declaration_index,
        Some(compile_sources.user_module_path.clone()),
    )
    .map_err(|e| format!("phase=resolve; message={}", e))?;
    let typed = scar::typecheck_with_context(resolved, typecheck_context_for_mode(mode))
        .map_err(|e| format!("phase=typecheck; message={}", e))?;
    let mut bytecode =
        forge::codegen(typed).map_err(|e| format!("phase=codegen; message={}", e))?;
    let user_source = compile_sources
        .sources
        .source(compile_sources.user_source_id)
        .unwrap_or("");
    populate_error_template_lines(&mut bytecode.error_templates, user_source);
    bytecode.docs = docs;
    store_cached_bytecode(compile_sources, mode, &bytecode)?;
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

#[allow(dead_code)]
pub fn run_project_script_with_input(
    source_name: &str,
    source: &str,
    input: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let bytecode = compile_project_script(source_name, source)?;
    run_bytecode_with_input(bytecode, input)
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

fn run_bytecode_with_input(
    bytecode: Bytecode,
    input: &str,
) -> Result<(Vec<String>, Vec<String>), String> {
    let mut vm = eldr::VM::new(bytecode)
        .with_output_capture()
        .with_error_capture()
        .with_stdin_input(input);
    vm.run()
        .map_err(|e| format!("phase=runtime; message={}", e))?;
    Ok((
        vm.output.unwrap_or_default(),
        vm.error_output.unwrap_or_default(),
    ))
}
