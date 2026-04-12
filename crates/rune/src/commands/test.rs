use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use eldr::vm::{VmTestEvent, VmTestEventKind};
use forge::bytecode::{stable_hash_hex, Bytecode};

use crate::compile::{compile_source, ScriptCompilePlan};
use crate::error::{ExecutionEnv, RuneError, RuneResult};

const TEST_PRELUDE_FILE: &str = "lib/tests/prelude.srt";
const TEST_PRELUDE_MODULE_PATH: &str = "Test";
const TEST_PRELUDE_SOURCE: &str = include_str!("../../../../lib/tests/prelude.srt");
const TEST_CACHE_VERSION: &str = "surtr-test-dsl-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestOptions {
    pub(crate) selector: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestScript {
    selector: String,
    file_path: String,
    source: String,
}

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    let options = parse_test_options(args)?;
    test_command(options, ExecutionEnv::Test)
}

pub(crate) fn parse_test_options(args: &[String]) -> RuneResult<TestOptions> {
    if args.len() != 1 {
        return Err(RuneError::usage("test: expected exactly one lib-relative test name"));
    }

    let selector = args[0].trim().to_string();
    if selector.is_empty() {
        return Err(RuneError::usage("test: selector must not be empty"));
    }

    Ok(TestOptions { selector })
}

fn test_command(options: TestOptions, env: ExecutionEnv) -> RuneResult<()> {
    let script = load_test_script(&options.selector)?;
    let bytecode = compile_test_script(&script, env)?;

    let mut vm = eldr::VM::new(bytecode)
        .with_source(script.source.clone(), script.file_path.clone())
        .with_output_capture()
        .with_error_capture();

    if let Err(err) = vm.run() {
        println!("[FAIL] {} ({})", script.selector, script.file_path);
        println!("  note: runtime error while running test script: {}", err);
        println!("test result: passed=0, failed=1, total=1");
        return Err(RuneError::silent(1));
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    for event in vm.test_events() {
        match event.kind {
            VmTestEventKind::Passed => {
                passed += 1;
                println!("[PASS] {}", format_event_path(event));
            }
            VmTestEventKind::Failed => {
                failed += 1;
                println!("[FAIL] {} ({})", format_event_path(event), script.file_path);
                if let Some(detail) = &event.detail {
                    println!("  note: {}", detail);
                }
            }
        }
    }

    let total = passed + failed;
    if total == 0 {
        println!("No tests found in {}.", script.file_path);
        return Ok(());
    }

    println!(
        "test result: passed={}, failed={}, total={}",
        passed, failed, total
    );

    if failed == 0 {
        Ok(())
    } else {
        Err(RuneError::silent(1))
    }
}

fn load_test_script(selector: &str) -> RuneResult<TestScript> {
    let path = resolve_test_script_path(selector);
    let source = fs::read_to_string(&path).map_err(|e| {
        RuneError::message(
            1,
            format!(
                "test: failed to read {} for selector `{}`: {}",
                display_path(&path),
                selector,
                e
            ),
        )
    })?;

    Ok(TestScript {
        selector: selector.trim_end_matches(".srt").to_string(),
        file_path: display_path(&path),
        source,
    })
}

fn resolve_test_script_path(selector: &str) -> PathBuf {
    let trimmed = selector.trim().replace('\\', "/");
    let without_prefix = trimmed.trim_start_matches("./");
    let normalized = without_prefix.trim_start_matches("lib/tests/");
    let has_extension = normalized.ends_with(".srt");
    let relative = if has_extension {
        normalized.to_string()
    } else {
        format!("{normalized}.srt")
    };
    Path::new("lib").join("tests").join(relative)
}

fn collect_test_compile_sources(
    script: &TestScript,
    env: ExecutionEnv,
) -> RuneResult<xldr::CompileSources> {
    let module_inputs = xldr::collect_additional_default_std_module_inputs().map_err(|e| {
        RuneError::message(
            1,
            format!(
                "{}: failed to collect module sources: {}",
                env.command_name(),
                e
            ),
        )
    })?;
    let extra_std_sources = vec![xldr::SourceDescriptor::std_module(
        TEST_PRELUDE_FILE,
        TEST_PRELUDE_SOURCE,
        TEST_PRELUDE_MODULE_PATH,
    )];
    let module_sources = xldr::collect_module_sources_with_extra_std_sources(
        &extra_std_sources,
        &[module_inputs],
    )
    .map_err(|e| {
        RuneError::message(
            1,
            format!(
                "{}: failed to collect test module sources: {}",
                env.command_name(),
                e
            ),
        )
    })?;
    Ok(xldr::compose_script_compile_sources(
        &script.file_path,
        &script.source,
        module_sources,
    ))
}

fn compile_test_script(script: &TestScript, env: ExecutionEnv) -> RuneResult<Bytecode> {
    let cache_path = cached_eldr_path(script)?;
    if let Some(bytecode) = load_cached_bytecode(&cache_path)? {
        return Ok(bytecode);
    }

    let compile_plan = ScriptCompilePlan::plain(script.source.clone());
    let compile_sources = collect_test_compile_sources(script, env)?;
    let bytecode = compile_source(env, &compile_sources, &compile_plan)?;
    store_cached_bytecode(&cache_path, &bytecode)?;
    Ok(bytecode)
}

fn fixture_cache_root() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("target")
        .join("surtr-test-cache")
        .join("eldr")
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

fn binary_fingerprint() -> Result<String, RuneError> {
    let exe = env::current_exe()
        .map_err(|e| RuneError::message(1, format!("test: failed to locate current exe: {}", e)))?;
    let bytes = fs::read(&exe).map_err(|e| {
        RuneError::message(
            1,
            format!("test: failed to read current exe {}: {}", exe.display(), e),
        )
    })?;
    Ok(stable_hash_bytes(&bytes))
}

fn library_sources_fingerprint() -> Result<String, RuneError> {
    let modules = xldr::collect_lib_module_inputs()
        .map_err(|e| RuneError::message(1, format!("test: failed to collect lib sources: {}", e)))?;
    let mut payload = String::new();
    for module in modules {
        payload.push_str(&module.file_name);
        payload.push('\x1f');
        payload.push_str(&module.module_path);
        payload.push('\x1f');
        payload.push_str(&stable_hash_hex(&module.source));
        payload.push('\x1e');
    }
    Ok(stable_hash_hex(&payload))
}

fn cached_eldr_path(script: &TestScript) -> Result<PathBuf, RuneError> {
    let mut key = String::new();
    key.push_str(TEST_CACHE_VERSION);
    key.push('\x1f');
    key.push_str(&binary_fingerprint()?);
    key.push('\x1f');
    key.push_str(&library_sources_fingerprint()?);
    key.push('\x1f');
    key.push_str(&script.file_path);
    key.push('\x1f');
    key.push_str(&stable_hash_hex(&script.source));
    Ok(fixture_cache_root().join(format!("{}.eldr", stable_hash_hex(&key))))
}

fn load_cached_bytecode(cache_path: &Path) -> RuneResult<Option<Bytecode>> {
    if !cache_path.exists() {
        return Ok(None);
    }

    let bytes = match fs::read(cache_path) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };

    match Bytecode::decode(&bytes) {
        Ok(bytecode) => Ok(Some(bytecode)),
        Err(_) => {
            let _ = fs::remove_file(cache_path);
            Ok(None)
        }
    }
}

fn store_cached_bytecode(cache_path: &Path, bytecode: &Bytecode) -> RuneResult<()> {
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            RuneError::message(
                1,
                format!(
                    "test: failed to create cache directory {}: {}",
                    parent.display(),
                    e
                ),
            )
        })?;
    }

    let bytes = bytecode
        .encode()
        .map_err(|e| RuneError::message(1, format!("test: failed to encode bytecode: {}", e)))?;
    let temp_path = cache_path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp_path, bytes).map_err(|e| {
        RuneError::message(
            1,
            format!(
                "test: failed to write cache file {}: {}",
                temp_path.display(),
                e
            ),
        )
    })?;
    fs::rename(&temp_path, cache_path)
        .or_else(|_| {
            fs::copy(&temp_path, cache_path)
                .map(|_| ())
                .and_then(|_| fs::remove_file(&temp_path))
        })
        .map_err(|e| {
            RuneError::message(
                1,
                format!(
                    "test: failed to finalize cache file {}: {}",
                    cache_path.display(),
                    e
                ),
            )
        })?;
    Ok(())
}

fn format_event_path(event: &VmTestEvent) -> String {
    event.path.join(" > ")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{parse_test_options, resolve_test_script_path};
    use std::path::Path;

    #[test]
    fn test_options_require_single_selector() {
        let opts = parse_test_options(&["string".to_string()]).expect("selector should parse");
        assert_eq!(opts.selector, "string");
        assert!(parse_test_options(&[]).is_err());
        assert!(parse_test_options(&["a".to_string(), "b".to_string()]).is_err());
    }

    #[test]
    fn selector_resolves_into_lib_tests() {
        assert_eq!(
            resolve_test_script_path("string"),
            Path::new("lib").join("tests").join("string.srt")
        );
        assert_eq!(
            resolve_test_script_path("string.srt"),
            Path::new("lib").join("tests").join("string.srt")
        );
    }
}
