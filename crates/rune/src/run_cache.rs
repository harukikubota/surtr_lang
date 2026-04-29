use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use forge::bytecode::{stable_hash_hex, Bytecode};

use crate::compile::ScriptCompilePlan;
use crate::error::ExecutionEnv;

const CACHE_VERSION: &str = "surtr-run-cache-v1";

pub(crate) fn load(
    env: ExecutionEnv,
    compile_sources: &xldr::CompileSources,
    compile_plan: &ScriptCompilePlan,
) -> Option<Bytecode> {
    if !enabled() {
        return None;
    }

    let cache_path = cache_path(env, compile_sources, compile_plan)?;
    if !cache_path.exists() {
        return None;
    }

    let bytes = match fs::read(&cache_path) {
        Ok(bytes) => bytes,
        Err(_) => return None,
    };
    match Bytecode::decode(&bytes) {
        Ok(bytecode) => Some(bytecode),
        Err(_) => {
            let _ = fs::remove_file(cache_path);
            None
        }
    }
}

pub(crate) fn store(
    env: ExecutionEnv,
    compile_sources: &xldr::CompileSources,
    compile_plan: &ScriptCompilePlan,
    bytecode: &Bytecode,
) {
    if !enabled() {
        return;
    }

    let Some(cache_path) = cache_path(env, compile_sources, compile_plan) else {
        return;
    };
    let Some(parent) = cache_path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }

    let Ok(bytes) = bytecode.encode() else {
        return;
    };
    let temp_path = cache_path.with_extension(format!("{}.tmp", std::process::id()));
    if fs::write(&temp_path, bytes).is_err() {
        let _ = fs::remove_file(&temp_path);
        return;
    }
    if fs::rename(&temp_path, &cache_path).is_err() {
        if fs::copy(&temp_path, &cache_path).is_err() {
            let _ = fs::remove_file(&temp_path);
            return;
        }
        let _ = fs::remove_file(&temp_path);
    }
}

fn enabled() -> bool {
    !matches!(
        env::var("SURTR_RUN_CACHE").ok().as_deref(),
        Some("0") | Some("false") | Some("FALSE") | Some("no") | Some("NO")
    )
}

fn cache_path(
    env: ExecutionEnv,
    compile_sources: &xldr::CompileSources,
    compile_plan: &ScriptCompilePlan,
) -> Option<PathBuf> {
    Some(cache_root().join(format!(
        "{}.eldr",
        cache_key(env, compile_sources, compile_plan)?
    )))
}

fn cache_root() -> PathBuf {
    if let Some(path) = env::var_os("SURTR_RUN_CACHE_DIR") {
        return PathBuf::from(path);
    }

    target_root_from_current_exe()
        .map(|root| root.join("run-cache").join("eldr"))
        .unwrap_or_else(|| env::temp_dir().join("surtr-run-cache").join("eldr"))
}

fn target_root_from_current_exe() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let mut current = exe.parent()?;
    while let Some(name) = current.file_name().and_then(|name| name.to_str()) {
        if name == "debug" || name == "release" {
            return current.parent().map(Path::to_path_buf);
        }
        current = current.parent()?;
    }
    None
}

fn cache_key(
    env: ExecutionEnv,
    compile_sources: &xldr::CompileSources,
    compile_plan: &ScriptCompilePlan,
) -> Option<String> {
    let user_file_name = compile_sources
        .sources
        .file_name(compile_sources.user_source_id)
        .unwrap_or("<unknown>");
    let user_source = compile_sources
        .sources
        .source(compile_sources.user_source_id)
        .unwrap_or("");

    let mut key = String::new();
    key.push_str(CACHE_VERSION);
    key.push('\x1f');
    key.push_str(&current_exe_fingerprint()?);
    key.push('\x1f');
    key.push_str(env.command_name());
    key.push('\x1f');
    key.push_str(&format!("{:?}", env.compile_unit_kind()));
    key.push('\x1f');
    key.push_str(compile_plan.selected_entry_name.as_deref().unwrap_or(""));
    key.push('\x1f');
    key.push_str(
        compile_plan
            .normalized_entrypoint
            .as_ref()
            .map(|entry| entry.qualified_symbol.as_str())
            .unwrap_or(""),
    );
    key.push('\x1f');
    key.push_str(user_file_name);
    key.push('\x1f');
    key.push_str(&compile_sources.user_module_path);
    key.push('\x1f');
    key.push_str(&stable_hash_hex(user_source));
    key.push('\x1f');
    push_module_pipeline_key(&mut key, compile_sources);

    Some(stable_hash_hex(&key))
}

fn push_module_pipeline_key(key: &mut String, compile_sources: &xldr::CompileSources) {
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
            key.push_str(file_name);
            key.push('\x1f');
            key.push_str(&module.module_path);
            key.push('\x1f');
            key.push_str(source_kind_key(module.source_kind));
            key.push('\x1f');
            key.push_str(&stable_hash_hex(source));
            key.push('\x1e');
        }
    }
}

fn source_kind_key(kind: xldr::SourceKind) -> &'static str {
    match kind {
        xldr::SourceKind::Script => "script",
        xldr::SourceKind::Module => "module",
        xldr::SourceKind::StdModule => "std",
        xldr::SourceKind::ReplChunk => "repl",
    }
}

fn current_exe_fingerprint() -> Option<String> {
    static FINGERPRINT: OnceLock<Option<String>> = OnceLock::new();
    FINGERPRINT
        .get_or_init(|| {
            let exe = env::current_exe().ok()?;
            let bytes = fs::read(exe).ok()?;
            Some(stable_hash_bytes(&bytes))
        })
        .clone()
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
