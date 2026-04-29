use std::env;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use eldr::vm::{VmTestEvent, VmTestEventKind};
use forge::bytecode::{stable_hash_hex, Bytecode};

use crate::compile::{compile_source, ScriptCompilePlan};
use crate::error::{ExecutionEnv, RuneError, RuneResult};

const TEST_PRELUDE_FILE: &str = "lib/tests/prelude.srt";
const TEST_PRELUDE_MODULE_PATH: &str = "Test";
const TEST_PRELUDE_SOURCE: &str = include_str!("../../../../lib/tests/prelude.srt");
const TEST_CACHE_VERSION: &str = "surtr-test-dsl-v2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestOptions {
    pub(crate) mode: TestMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TestMode {
    One(String),
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct TestRunSummary {
    passed: usize,
    failed: usize,
    total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestScript {
    selector: String,
    file_path: String,
    source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestOutputColor {
    Green,
    Red,
    Yellow,
    Cyan,
}

pub(crate) fn dispatch(args: &[String]) -> RuneResult<()> {
    let options = parse_test_options(args)?;
    test_command(options, ExecutionEnv::Test)
}

pub(crate) fn parse_test_options(args: &[String]) -> RuneResult<TestOptions> {
    if args.len() != 1 {
        return Err(RuneError::usage(
            "test: expected exactly one lib-relative test name",
        ));
    }

    let selector = args[0].trim().to_string();
    if selector.is_empty() {
        return Err(RuneError::usage("test: selector must not be empty"));
    }

    let mode = if selector == "--all" {
        TestMode::All
    } else {
        TestMode::One(selector)
    };

    Ok(TestOptions { mode })
}

fn test_command(options: TestOptions, env: ExecutionEnv) -> RuneResult<()> {
    match options.mode {
        TestMode::One(selector) => run_one_test(&selector, env).map(|_| ()),
        TestMode::All => run_all_tests(env),
    }
}

fn run_one_test(selector: &str, env: ExecutionEnv) -> RuneResult<TestRunSummary> {
    let summary = execute_test_script(selector, env)?;
    if summary.failed == 0 {
        Ok(summary)
    } else {
        Err(RuneError::silent(1))
    }
}

fn execute_test_script(selector: &str, env: ExecutionEnv) -> RuneResult<TestRunSummary> {
    let script = load_test_script(selector)?;
    let bytecode = compile_test_script(&script, env)?;
    let color = test_color_enabled();

    let mut vm = eldr::VM::new(bytecode)
        .with_source(script.source.clone(), script.file_path.clone())
        .with_output_capture()
        .with_error_capture();

    if let Err(err) = vm.run() {
        print_color_line(
            &format!("[FAIL] {} ({})", script.selector, script.file_path),
            TestOutputColor::Red,
            color,
        );
        print_color_line(
            &format!("  note: runtime error while running test script: {}", err),
            TestOutputColor::Yellow,
            color,
        );
        print_summary(TestRunSummary {
            passed: 0,
            failed: 1,
            total: 1,
        });
        return Err(RuneError::silent(1));
    }

    let mut summary = TestRunSummary::default();
    for event in vm.test_events() {
        match event.kind {
            VmTestEventKind::Passed => {
                summary.passed += 1;
                print_color_line(
                    &format!("[PASS] {}", format_event_path(event)),
                    TestOutputColor::Green,
                    color,
                );
            }
            VmTestEventKind::Failed => {
                summary.failed += 1;
                print_color_line(
                    &format!("[FAIL] {} ({})", format_event_path(event), script.file_path),
                    TestOutputColor::Red,
                    color,
                );
                if let Some(detail) = &event.detail {
                    print_color_line(
                        &format!("  note: {}", detail),
                        TestOutputColor::Yellow,
                        color,
                    );
                }
            }
        }
    }

    summary.total = summary.passed + summary.failed;
    if summary.total == 0 {
        print_color_line(
            &format!("No tests found in {}.", script.file_path),
            TestOutputColor::Yellow,
            color,
        );
        return Ok(summary);
    }

    print_summary(summary);

    Ok(summary)
}

fn run_all_tests(env: ExecutionEnv) -> RuneResult<()> {
    let selectors = collect_all_test_selectors()?;
    if selectors.is_empty() {
        println!("No test scripts found in lib/tests.");
        return Ok(());
    }

    let mut aggregate = TestRunSummary::default();
    for selector in selectors {
        match execute_test_script(&selector, env) {
            Ok(summary) => {
                aggregate.passed += summary.passed;
                aggregate.failed += summary.failed;
                aggregate.total += summary.total;
            }
            Err(err) => {
                err.emit();
                aggregate.failed += 1;
                aggregate.total += 1;
            }
        }
    }

    print_summary(aggregate);

    if aggregate.failed == 0 {
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

fn collect_all_test_selectors() -> RuneResult<Vec<String>> {
    let root = Path::new("lib").join("tests");
    let mut paths = Vec::new();
    collect_test_script_paths(&root, &mut paths)?;
    paths.sort();

    let selectors = paths
        .into_iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) != Some("prelude.srt"))
        .filter_map(|path| selector_for_test_path(&root, &path))
        .collect();
    Ok(selectors)
}

fn collect_test_script_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> RuneResult<()> {
    let entries = fs::read_dir(dir).map_err(|e| {
        RuneError::message(
            1,
            format!(
                "test: failed to read test directory {}: {}",
                display_path(dir),
                e
            ),
        )
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| {
            RuneError::message(
                1,
                format!("test: failed to read test directory entry: {}", e),
            )
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_test_script_paths(&path, paths)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("srt") {
            paths.push(path);
        }
    }

    Ok(())
}

fn selector_for_test_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let without_extension = relative.with_extension("");
    Some(display_path(&without_extension))
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
    let module_sources =
        xldr::collect_module_sources_with_extra_std_sources(&extra_std_sources, &[module_inputs])
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
    let modules = xldr::collect_lib_module_inputs().map_err(|e| {
        RuneError::message(1, format!("test: failed to collect lib sources: {}", e))
    })?;
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

fn test_color_enabled() -> bool {
    match env::var("SURTR_TEST_COLOR") {
        Ok(value) if value.eq_ignore_ascii_case("always") => true,
        Ok(value) if value.eq_ignore_ascii_case("never") => false,
        _ if env::var_os("NO_COLOR").is_some() => false,
        _ => std::io::stdout().is_terminal(),
    }
}

fn color_code(color: TestOutputColor) -> u8 {
    match color {
        TestOutputColor::Green => 32,
        TestOutputColor::Red => 31,
        TestOutputColor::Yellow => 33,
        TestOutputColor::Cyan => 36,
    }
}

fn colorize_line(line: &str, color: TestOutputColor, enabled: bool) -> String {
    if enabled {
        format!("\x1b[{}m{}\x1b[0m", color_code(color), line)
    } else {
        line.to_string()
    }
}

fn print_color_line(line: &str, color: TestOutputColor, enabled: bool) {
    println!("{}", colorize_line(line, color, enabled));
}

fn summary_line(summary: TestRunSummary) -> String {
    format!(
        "test result: passed={}, failed={}, total={}",
        summary.passed, summary.failed, summary.total
    )
}

fn summary_color(summary: TestRunSummary) -> TestOutputColor {
    if summary.failed == 0 {
        TestOutputColor::Cyan
    } else {
        TestOutputColor::Red
    }
}

fn print_summary(summary: TestRunSummary) {
    print_color_line(
        &summary_line(summary),
        summary_color(summary),
        test_color_enabled(),
    );
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{
        colorize_line, parse_test_options, resolve_test_script_path, summary_color, summary_line,
        TestMode, TestOutputColor, TestRunSummary,
    };
    use std::path::Path;

    #[test]
    fn test_options_require_single_selector() {
        let opts = parse_test_options(&["string".to_string()]).expect("selector should parse");
        assert_eq!(opts.mode, TestMode::One("string".to_string()));
        assert!(parse_test_options(&[]).is_err());
        assert!(parse_test_options(&["a".to_string(), "b".to_string()]).is_err());
    }

    #[test]
    fn test_options_accept_all_flag() {
        let opts = parse_test_options(&["--all".to_string()]).expect("--all should parse");
        assert_eq!(opts.mode, TestMode::All);
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

    #[test]
    fn colorize_line_preserves_plain_substring() {
        let rendered = colorize_line("[PASS] Suite > case", TestOutputColor::Green, true);
        assert!(rendered.contains("[PASS] Suite > case"));
        assert_eq!(rendered, "\x1b[32m[PASS] Suite > case\x1b[0m".to_string());
        assert_eq!(
            colorize_line("[PASS] Suite > case", TestOutputColor::Green, false),
            "[PASS] Suite > case".to_string()
        );
    }

    #[test]
    fn summary_uses_success_or_failure_color() {
        let passed = TestRunSummary {
            passed: 2,
            failed: 0,
            total: 2,
        };
        let failed = TestRunSummary {
            passed: 1,
            failed: 1,
            total: 2,
        };
        assert_eq!(
            summary_line(passed),
            "test result: passed=2, failed=0, total=2"
        );
        assert_eq!(summary_color(passed), TestOutputColor::Cyan);
        assert_eq!(summary_color(failed), TestOutputColor::Red);
    }
}
