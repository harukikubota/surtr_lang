use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use forge::bytecode::Bytecode;
use xldr::{CachedTestSemanticPrefixPayload, CompileSources, SourceKind};

use super::sources::{
    compile_chunk_typecheck_context_for_mode, compile_unit_kind_for_mode, default_stdlib_snapshot,
    parse_module_stage_suffix, parse_module_stages, std_typecheck_context_for_mode,
};
use super::timing::CacheStatsSnapshot;
use super::types::{
    CachedCompilePrefix, CachedModulePipeline, SharedCompilePrefix, TestCompileMode,
};

fn test_binary_fingerprint() -> Result<String, String> {
    xldr::current_exe_fingerprint().map_err(|e| {
        format!(
            "phase=cache; message=failed to fingerprint current exe: {}",
            e
        )
    })
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

fn semantic_prefix_cache_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-fixture-cache/prefix")
}

pub(crate) fn cache_stats_snapshot() -> CacheStatsSnapshot {
    cache_stats()
        .lock()
        .expect("test fixture cache stats poisoned")
        .clone()
}

fn cache_stats() -> &'static Mutex<CacheStatsSnapshot> {
    static CACHE_STATS: OnceLock<Mutex<CacheStatsSnapshot>> = OnceLock::new();
    CACHE_STATS.get_or_init(|| Mutex::new(CacheStatsSnapshot::default()))
}

fn record_cache_event(update: impl FnOnce(&mut CacheStatsSnapshot)) {
    let mut stats = cache_stats()
        .lock()
        .expect("test fixture cache stats poisoned");
    update(&mut stats);
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
                SourceKind::DefinitionSource => "module",
                SourceKind::StdDefinitionSource => "std",
                SourceKind::ProjectConfigSource => "project-config",
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

fn cached_semantic_prefix_path(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<PathBuf, String> {
    let key =
        xldr::test_semantic_prefix_cache_key(compile_unit_kind_for_mode(mode), compile_sources)
            .map_err(|e| {
                format!(
                    "phase=cache; message=failed to build semantic prefix key: {}",
                    e
                )
            })?;
    Ok(semantic_prefix_cache_root().join(format!("{key}.semantic")))
}

pub(super) fn load_cached_bytecode(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<Option<Bytecode>, String> {
    if !fixture_cache_enabled() {
        return Ok(None);
    }

    let cache_path = cached_eldr_path(compile_sources, mode)?;
    if !cache_path.exists() {
        record_cache_event(|stats| stats.final_bytecode_misses += 1);
        return Ok(None);
    }

    let bytes = match fs::read(&cache_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            record_cache_event(|stats| stats.final_bytecode_misses += 1);
            return Ok(None);
        }
    };

    match Bytecode::decode(&bytes) {
        Ok(bytecode) => {
            record_cache_event(|stats| stats.final_bytecode_hits += 1);
            Ok(Some(bytecode))
        }
        Err(_) => {
            record_cache_event(|stats| stats.final_bytecode_corrupt += 1);
            let _ = fs::remove_file(&cache_path);
            Ok(None)
        }
    }
}

pub(super) fn store_cached_bytecode(
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
    static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
    let temp_id = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_path = cache_path.with_extension(format!("{}.{}.tmp", std::process::id(), temp_id));
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

    record_cache_event(|stats| stats.final_bytecode_writes += 1);
    Ok(())
}

pub(super) fn cached_module_pipeline(
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

    let module_asts = if matches!(mode, TestCompileMode::Script) {
        let std_snapshot = default_stdlib_snapshot()?;
        let mut module_asts = std_snapshot.module_stages.clone();
        let mut suffix_asts = parse_module_stage_suffix(
            compile_sources,
            compile_unit_kind_for_mode(mode),
            std_snapshot.default_stage_count,
        )?;
        module_asts.append(&mut suffix_asts);
        module_asts
    } else {
        parse_module_stages(compile_sources, compile_unit_kind_for_mode(mode))?
    };
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

fn next_fun_idx(bytecode: &Bytecode) -> u32 {
    bytecode
        .functions
        .iter()
        .map(|entry| entry.fun_idx.saturating_add(1))
        .max()
        .unwrap_or(0)
}

fn cached_script_compile_prefix(
    compile_sources: &CompileSources,
) -> Result<SharedCompilePrefix, String> {
    let std_snapshot = default_stdlib_snapshot()?;
    let cached_modules = cached_module_pipeline(compile_sources, TestCompileMode::Script)?;

    if cached_modules.module_asts.len() == std_snapshot.default_stage_count {
        return Ok(Arc::new(CachedCompilePrefix {
            module_asts: cached_modules.module_asts,
            compile_prefix: xldr::CompilationPrefixSnapshot {
                declaration_index: cached_modules.declaration_index,
                resolve_state: std_snapshot.resolve_state(),
                scar_checkpoint: std_snapshot.scar_checkpoint().clone(),
                bytecode: std_snapshot.bytecode().clone(),
            },
        }));
    }

    let cache_key = xldr::test_semantic_prefix_cache_key(
        compile_unit_kind_for_mode(TestCompileMode::Script),
        compile_sources,
    )
    .map_err(|e| {
        format!(
            "phase=cache; message=failed to build semantic prefix key: {}",
            e
        )
    })?;
    let cache_path = cached_semantic_prefix_path(compile_sources, TestCompileMode::Script)?;

    if let Some(payload) = xldr::load_cached_test_semantic_prefix(&cache_path, &cache_key) {
        record_cache_event(|stats| stats.semantic_prefix_hits += 1);
        return Ok(Arc::new(CachedCompilePrefix {
            module_asts: cached_modules.module_asts,
            compile_prefix: xldr::CompilationPrefixSnapshot {
                declaration_index: cached_modules.declaration_index,
                resolve_state: payload.resolve_state,
                scar_checkpoint: payload.scar_checkpoint,
                bytecode: payload.bytecode,
            },
        }));
    }
    if cache_path.exists() {
        record_cache_event(|stats| stats.semantic_prefix_corrupt += 1);
    } else {
        record_cache_event(|stats| stats.semantic_prefix_misses += 1);
    }

    let resolved = sigil::resolve_staged_program_from_state(
        &cached_modules.module_asts,
        Vec::new(),
        &cached_modules.declaration_index,
        None,
        std_snapshot.default_stage_count,
        std_snapshot.resolve_state(),
    )
    .map_err(|e| format!("phase=resolve; message={}", e))?;
    let resume_state = resolved.resume_state;

    let mut scar_session = scar::ScarSession::new();
    scar_session.rollback(std_snapshot.scar_checkpoint().clone());
    scar_session.ensure_next_fun_idx_at_least(next_fun_idx(std_snapshot.bytecode()));
    let typed = scar_session
        .typecheck_staged_program_with_context(
            resolved,
            compile_chunk_typecheck_context_for_mode(TestCompileMode::Script),
        )
        .map_err(|e| format!("phase=typecheck; message={}", e))?;

    let mut forge_session = forge::ForgeSession::from_bytecode(std_snapshot.bytecode());
    let (chunk, _) = forge_session
        .codegen_chunk_typed_program(typed)
        .map_err(|e| format!("phase=codegen; message={}", e))?;
    let bytecode = forge::compose_bytecode_with_chunk(std_snapshot.bytecode().clone(), chunk)
        .map_err(|e| format!("phase=codegen; message={}", e))?;
    scar_session.reconcile_function_indices(bytecode.functions.iter().filter_map(|entry| {
        entry
            .qualified_name
            .as_deref()
            .map(|qualified_name| (qualified_name, entry.fun_idx))
    }));

    let prefix = Arc::new(CachedCompilePrefix {
        module_asts: cached_modules.module_asts,
        compile_prefix: xldr::CompilationPrefixSnapshot {
            declaration_index: cached_modules.declaration_index,
            resolve_state: sigil::ResolveResumeState {
                next_local_id: resume_state.next_local_id.max(next_fun_idx(&bytecode)),
            },
            scar_checkpoint: scar_session.checkpoint(),
            bytecode,
        },
    });
    xldr::store_cached_test_semantic_prefix(
        &cache_path,
        &cache_key,
        CachedTestSemanticPrefixPayload {
            declaration_index: prefix.declaration_index().clone(),
            resolve_state: prefix.resolve_state(),
            scar_checkpoint: prefix.scar_checkpoint().clone(),
            bytecode: prefix.bytecode().clone(),
        },
    );
    record_cache_event(|stats| stats.semantic_prefix_writes += 1);
    Ok(prefix)
}

pub(super) fn cached_compile_prefix(
    compile_sources: &CompileSources,
    mode: TestCompileMode,
) -> Result<SharedCompilePrefix, String> {
    static COMPILE_PREFIX_CACHE: OnceLock<
        Mutex<HashMap<String, Result<SharedCompilePrefix, String>>>,
    > = OnceLock::new();

    let cache = COMPILE_PREFIX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cache_key = module_pipeline_cache_key(compile_sources, mode);

    if let Some(cached) = cache
        .lock()
        .expect("compile prefix cache poisoned")
        .get(&cache_key)
    {
        return cached.clone();
    }

    let prefix = if matches!(mode, TestCompileMode::Script) {
        cached_script_compile_prefix(compile_sources)?
    } else {
        let cached_modules = cached_module_pipeline(compile_sources, mode)?;
        let cache_key =
            xldr::test_semantic_prefix_cache_key(compile_unit_kind_for_mode(mode), compile_sources)
                .map_err(|e| {
                    format!(
                        "phase=cache; message=failed to build semantic prefix key: {}",
                        e
                    )
                })?;
        let cache_path = cached_semantic_prefix_path(compile_sources, mode)?;

        if let Some(payload) = xldr::load_cached_test_semantic_prefix(&cache_path, &cache_key) {
            record_cache_event(|stats| stats.semantic_prefix_hits += 1);
            Arc::new(CachedCompilePrefix {
                module_asts: cached_modules.module_asts,
                compile_prefix: xldr::CompilationPrefixSnapshot {
                    declaration_index: cached_modules.declaration_index,
                    resolve_state: payload.resolve_state,
                    scar_checkpoint: payload.scar_checkpoint,
                    bytecode: payload.bytecode,
                },
            })
        } else {
            if cache_path.exists() {
                record_cache_event(|stats| stats.semantic_prefix_corrupt += 1);
            } else {
                record_cache_event(|stats| stats.semantic_prefix_misses += 1);
            }
            let resolved = sigil::resolve_staged_program_with_state(
                &cached_modules.module_asts,
                Vec::new(),
                &cached_modules.declaration_index,
                None,
            )
            .map_err(|e| format!("phase=resolve; message={}", e))?;
            let resume_state = resolved.resume_state;
            let mut scar_session = scar::ScarSession::new();
            let typed = scar_session
                .typecheck_staged_program_with_context(
                    resolved,
                    std_typecheck_context_for_mode(mode),
                )
                .map_err(|e| format!("phase=typecheck; message={}", e))?;
            let bytecode = forge::codegen_typed_program(typed)
                .map_err(|e| format!("phase=codegen; message={}", e))?;
            scar_session.reconcile_function_indices(bytecode.functions.iter().filter_map(
                |entry| {
                    entry
                        .qualified_name
                        .as_deref()
                        .map(|qualified_name| (qualified_name, entry.fun_idx))
                },
            ));
            let prefix = Arc::new(CachedCompilePrefix {
                module_asts: cached_modules.module_asts,
                compile_prefix: xldr::CompilationPrefixSnapshot {
                    declaration_index: cached_modules.declaration_index,
                    resolve_state: sigil::ResolveResumeState {
                        next_local_id: resume_state.next_local_id.max(next_fun_idx(&bytecode)),
                    },
                    scar_checkpoint: scar_session.checkpoint(),
                    bytecode,
                },
            });
            xldr::store_cached_test_semantic_prefix(
                &cache_path,
                &cache_key,
                CachedTestSemanticPrefixPayload {
                    declaration_index: prefix.declaration_index().clone(),
                    resolve_state: prefix.resolve_state(),
                    scar_checkpoint: prefix.scar_checkpoint().clone(),
                    bytecode: prefix.bytecode().clone(),
                },
            );
            record_cache_event(|stats| stats.semantic_prefix_writes += 1);
            prefix
        }
    };

    cache
        .lock()
        .expect("compile prefix cache poisoned")
        .insert(cache_key, Ok(Arc::clone(&prefix)));
    Ok(prefix)
}
