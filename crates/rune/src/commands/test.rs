use std::env;
use std::fs;
use std::io::IsTerminal;
use std::path::{Component, Path, PathBuf};

use eldr::vm::{VmTestDiagnostic, VmTestEvent, VmTestEventKind};
use forge::bytecode::{stable_hash_hex, Bytecode};
use spire::ast::Span;

use crate::compile::{
    collect_default_script_compile_sources, compile_source, prepare_script_compile_plan,
    script_plan_error_as_rune_error,
};
use crate::error::{ExecutionEnv, RuneError, RuneResult};

const TEST_CACHE_VERSION: &str = "surtr-test-dsl-v2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestOptions {
    pub(crate) mode: TestMode,
    pub(crate) quiet: bool,
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
    let mut quiet = false;
    let mut selector = None;

    for arg in args {
        match arg.as_str() {
            "--quiet" | "-q" => {
                if quiet {
                    return Err(RuneError::usage("test: --quiet may only be specified once"));
                }
                quiet = true;
            }
            value if value.starts_with('-') && value != "--all" => {
                return Err(RuneError::usage(format!("test: unknown option `{value}`")));
            }
            value => {
                validate_test_selector(value.trim())?;
                if value == "--all" && selector.as_deref() == Some("--all") {
                    return Err(RuneError::usage("test: --all may only be specified once"));
                }
                if selector.replace(value.trim().to_string()).is_some() {
                    return Err(RuneError::usage(
                        "test: expected exactly one lib-relative test name",
                    ));
                }
            }
        }
    }

    let Some(selector) = selector else {
        return Err(RuneError::usage(
            "test: expected exactly one lib-relative test name",
        ));
    };
    if selector.is_empty() {
        return Err(RuneError::usage("test: selector must not be empty"));
    }

    let mode = if selector == "--all" {
        TestMode::All
    } else {
        TestMode::One(selector)
    };

    Ok(TestOptions { mode, quiet })
}

fn validate_test_selector(selector: &str) -> RuneResult<()> {
    if selector == "--all" {
        return Ok(());
    }
    let normalized = selector.replace('\\', "/");
    let path = Path::new(&normalized);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(RuneError::usage(
            "test: selector must stay within lib/tests",
        ));
    }
    Ok(())
}

fn test_command(options: TestOptions, env: ExecutionEnv) -> RuneResult<()> {
    match options.mode {
        TestMode::One(selector) => run_one_test(&selector, env, options.quiet).map(|_| ()),
        TestMode::All => run_all_tests(env, options.quiet),
    }
}

fn run_one_test(selector: &str, env: ExecutionEnv, quiet: bool) -> RuneResult<TestRunSummary> {
    let summary = execute_test_script(selector, env, quiet)?;
    if summary.failed == 0 {
        Ok(summary)
    } else {
        Err(RuneError::silent(1))
    }
}

fn execute_test_script(
    selector: &str,
    env: ExecutionEnv,
    quiet: bool,
) -> RuneResult<TestRunSummary> {
    let script = load_test_script(selector)?;
    let bytecode = compile_test_script(&script, env)?;
    let color = test_color_enabled();

    let mut vm = eldr::VM::new(bytecode)
        .with_source(script.source.clone(), script.file_path.clone())
        .with_output_capture()
        .with_error_capture();

    if let Err(err) = vm.run() {
        print_test_event_line(
            "[FAIL]",
            &format!("{} ({})", script.selector, script.file_path),
            TestOutputColor::Red,
            color,
        );
        print_note_line(
            &format!("runtime error while running test script: {}", err),
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
                if !quiet {
                    print_test_event_line(
                        "[PASS]",
                        &format_event_path(event),
                        TestOutputColor::Green,
                        color,
                    );
                }
            }
            VmTestEventKind::Failed => {
                summary.failed += 1;
                let rendered_diagnostic =
                    render_test_event_diagnostic(event, &script, vm.bytecode());
                print_test_event_line(
                    "[FAIL]",
                    &format!("{} ({})", format_event_path(event), script.file_path),
                    TestOutputColor::Red,
                    color,
                );
                if rendered_diagnostic.is_none() {
                    if let Some(detail) = &event.detail {
                        print_note_line(detail, color);
                    }
                }
                if let Some(diagnostic) = rendered_diagnostic {
                    print!("{diagnostic}");
                    if !diagnostic.ends_with('\n') {
                        println!();
                    }
                }
            }
        }
    }

    summary.total = summary.passed + summary.failed;
    if summary.total == 0 {
        if script.file_path.contains("lib/tests/spec/") {
            return Ok(summary);
        }
        if !quiet {
            print_color_line(
                &format!("No tests found in {}.", script.file_path),
                TestOutputColor::Yellow,
                color,
            );
        }
        return Ok(summary);
    }

    if !quiet || summary.failed > 0 {
        print_summary(summary);
    }

    Ok(summary)
}

fn run_all_tests(env: ExecutionEnv, quiet: bool) -> RuneResult<()> {
    let selectors = collect_all_test_selectors()?;
    if selectors.is_empty() {
        if !quiet {
            println!("No test scripts found in lib/tests.");
        }
        return Ok(());
    }

    let mut aggregate = TestRunSummary::default();
    for selector in selectors {
        match execute_test_script(&selector, env, quiet) {
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

    if !quiet || aggregate.failed > 0 {
        print_summary(aggregate);
    }

    if aggregate.failed == 0 {
        Ok(())
    } else {
        Err(RuneError::silent(1))
    }
}

fn load_test_script(selector: &str) -> RuneResult<TestScript> {
    let path = resolve_test_script_path(selector)?;
    let read_error = |e| {
        RuneError::message(
            1,
            format!(
                "test: failed to read {} for selector `{}`: {}",
                display_path(&path),
                selector,
                e
            ),
        )
    };
    let canonical_path = fs::canonicalize(&path).map_err(read_error)?;
    let canonical_root = fs::canonicalize(Path::new("lib/tests")).map_err(read_error)?;
    if !canonical_path.starts_with(canonical_root) {
        return Err(RuneError::usage(
            "test: selector must stay within lib/tests",
        ));
    }
    let source = fs::read_to_string(&canonical_path).map_err(read_error)?;

    Ok(TestScript {
        selector: selector.trim_end_matches(".srt").to_string(),
        file_path: display_path(&path),
        source,
    })
}

fn resolve_test_script_path(selector: &str) -> RuneResult<PathBuf> {
    validate_test_selector(selector)?;
    let trimmed = selector.trim().replace('\\', "/");
    let without_prefix = trimmed.trim_start_matches("./");
    let normalized = without_prefix.trim_start_matches("lib/tests/");
    let has_extension = normalized.ends_with(".srt");
    let relative = if has_extension {
        normalized.to_string()
    } else {
        format!("{normalized}.srt")
    };
    Ok(Path::new("lib").join("tests").join(relative))
}

fn collect_all_test_selectors() -> RuneResult<Vec<String>> {
    let root = Path::new("lib").join("tests");
    let mut paths = Vec::new();
    collect_test_script_paths(&root, &mut paths)?;
    paths.sort();

    let selectors = paths
        .into_iter()
        .filter(|path| !is_schema_test_path(&root, path))
        .filter(|path| !is_spec_module_path(&root, path))
        .filter(|path| {
            !matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some("prelude.srt") | Some("spec_defs.srt")
            )
        })
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

fn is_schema_test_path(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|component| component.as_os_str() == "schema")
}

fn is_spec_module_path(root: &Path, path: &Path) -> bool {
    let mut components = match path.strip_prefix(root).ok() {
        Some(relative) => relative.components(),
        None => return false,
    };

    matches!(
        (components.next(), components.next()),
        (Some(first), Some(second))
            if first.as_os_str() == "spec" && second.as_os_str() == "modules"
    )
}

fn compile_test_script(script: &TestScript, env: ExecutionEnv) -> RuneResult<Bytecode> {
    let compile_plan = prepare_script_compile_plan(&script.file_path, &script.source, None)
        .map_err(|e| script_plan_error_as_rune_error(&script.file_path, &script.source, e))?;
    let cache_path = cached_eldr_path(script, &compile_plan.include_directives)?;
    if let Some(bytecode) = load_cached_bytecode(&cache_path)? {
        return Ok(bytecode);
    }

    let compile_sources = collect_default_script_compile_sources(
        env,
        &script.file_path,
        &compile_plan.source_for_parse,
        &compile_plan.include_modules,
        xldr::StdlibVariant::TestEnabled,
    )?;
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

fn binary_fingerprint() -> Result<String, RuneError> {
    xldr::current_exe_fingerprint().map_err(|e| {
        RuneError::message(1, format!("test: failed to fingerprint current exe: {}", e))
    })
}

fn library_sources_fingerprint() -> Result<String, RuneError> {
    let modules = xldr::cached_lib_module_inputs().map_err(|e| {
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

fn cached_eldr_path(
    script: &TestScript,
    include_directives: &[xldr::ScriptIncludeDirective],
) -> Result<PathBuf, RuneError> {
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
    key.push('\x1f');
    key.push_str(&include_sources_fingerprint(
        &script.file_path,
        include_directives,
    )?);
    Ok(fixture_cache_root().join(format!("{}.eldr", stable_hash_hex(&key))))
}

fn include_sources_fingerprint(
    script_file_path: &str,
    include_directives: &[xldr::ScriptIncludeDirective],
) -> Result<String, RuneError> {
    let mut payload = String::new();
    for directive in include_directives {
        let resolved_path = resolve_include_file_path(script_file_path, &directive.file_path);
        let source = fs::read_to_string(&resolved_path).map_err(|e| {
            RuneError::message(
                1,
                format!(
                    "test: failed to read include source {}: {}",
                    resolved_path.display(),
                    e
                ),
            )
        })?;
        payload.push_str(&display_path(&resolved_path));
        payload.push('\x1f');
        payload.push_str(&stable_hash_hex(&source));
        payload.push('\x1e');
    }
    Ok(stable_hash_hex(&payload))
}

fn resolve_include_file_path(script_file_path: &str, raw_path: &str) -> PathBuf {
    let candidate = Path::new(raw_path);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }

    let base_dir = Path::new(script_file_path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    base_dir.join(candidate)
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

fn render_test_event_diagnostic(
    event: &VmTestEvent,
    script: &TestScript,
    bytecode: &Bytecode,
) -> Option<String> {
    let diagnostic = event.diagnostic.as_ref()?;
    let source = if diagnostic.file == script.file_path {
        Some(script.source.as_str())
    } else {
        bytecode
            .sources
            .iter()
            .find(|source| {
                source.normalized_path.as_deref().unwrap_or(&source.path) == diagnostic.file
            })
            .and_then(|source| source.text.as_deref())
    };
    let span = Span {
        start: diagnostic.span_start as usize,
        end: diagnostic.span_end as usize,
    };
    let Some(source) = source.filter(|source| source_for_span(source, &span).is_some()) else {
        return Some(format!(
            "Error: {}: {}\n  at {}:{}:{}\n",
            diagnostic.kind,
            diagnostic.message,
            diagnostic.file,
            diagnostic.line,
            diagnostic.column,
        ));
    };
    if diagnostic.assertion.as_deref() == Some("assert_eq")
        && diagnostic.assertion_call_kind == Some(sindr::runtime::RuntimeCallKind::DirectFunction)
    {
        if let Some(spans) = test_assert_eq_spans(source, &span) {
            return Some(render_assert_eq_failure_diagnostic(
                &diagnostic.file,
                source,
                diagnostic,
                &spans,
            ));
        }
    }
    let spec = diagnostics::simple_error(
        diagnostic.kind.clone(),
        diagnostic.message.clone(),
        span,
        diagnostic
            .assertion
            .as_ref()
            .map(|name| format!("{name} failed: {}", diagnostic.message)),
    );
    Some(diagnostics::render_error(&diagnostic.file, source, &spec))
}

fn source_for_span<'a>(source: &'a str, span: &Span) -> Option<&'a str> {
    if span.start >= span.end || span.end > source.chars().count() {
        return None;
    }
    let start = source.char_indices().nth(span.start)?.0;
    let end = source
        .char_indices()
        .nth(span.end)
        .map(|(byte, _)| byte)
        .unwrap_or(source.len());
    source.get(start..end)
}

#[derive(Debug, Clone)]
struct AssertEqSpans {
    call: Span,
    lhs: Span,
    rhs: Span,
    lhs_term: String,
    rhs_term: String,
}

fn test_assert_eq_spans(source: &str, call: &Span) -> Option<AssertEqSpans> {
    // Parse only the executed call. The runtime identity determines which
    // assertion failed; source text is used solely for its argument labels.
    let call_source = source_for_span(source, call)?;
    let nodes = spire::parse(call_source).ok()?;
    let mut node = nodes.first()?;
    while let spire::ast::Ast::Grouped(_, inner) = node {
        node = inner;
    }
    let spire::ast::Ast::App(_, _, args) = node else {
        return None;
    };
    let (mut lhs, mut rhs) = (None, None);
    let mut positional = 0;
    for arg in args {
        match arg {
            spire::ast::RecordLitArg::Positional(value) => {
                match positional {
                    0 => lhs = Some(value),
                    1 => rhs = Some(value),
                    _ => return None,
                }
                positional += 1;
            }
            spire::ast::RecordLitArg::Named(name, value) => match name.as_str() {
                "expected" => lhs = Some(value),
                "actual" => rhs = Some(value),
                _ => return None,
            },
        }
    }
    let (lhs, rhs) = (lhs?.span(), rhs?.span());
    let absolute = |span: &Span| Span {
        start: call.start + span.start,
        end: call.start + span.end,
    };
    Some(AssertEqSpans {
        call: call.clone(),
        lhs: absolute(lhs),
        rhs: absolute(rhs),
        lhs_term: source_for_span(call_source, lhs)?.to_string(),
        rhs_term: source_for_span(call_source, rhs)?.to_string(),
    })
}

fn render_assert_eq_failure_diagnostic(
    file_name: &str,
    source: &str,
    diagnostic: &VmTestDiagnostic,
    spans: &AssertEqSpans,
) -> String {
    let spec = diagnostics::surtr_assert_eq_error_spec(
        diagnostic.kind.clone(),
        diagnostic.message.clone(),
        spans.call.clone(),
        spans.lhs.clone(),
        spans.rhs.clone(),
        spans.lhs_term.clone(),
        spans.rhs_term.clone(),
    );
    diagnostics::render_surtr_code_error(file_name, source, &spec)
}

fn test_color_enabled() -> bool {
    match env::var("SURTR_TEST_COLOR") {
        Ok(value) if value.trim().eq_ignore_ascii_case("always") => true,
        Ok(value) if value.trim().eq_ignore_ascii_case("never") => false,
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

fn colorize_text(text: &str, color: TestOutputColor, enabled: bool) -> String {
    if enabled {
        format!("\x1b[{}m{}\x1b[0m", color_code(color), text)
    } else {
        text.to_string()
    }
}

fn print_color_line(line: &str, color: TestOutputColor, enabled: bool) {
    println!("{}", colorize_text(line, color, enabled));
}

fn test_event_line(label: &str, detail: &str, color: TestOutputColor, enabled: bool) -> String {
    format!("{} {}", colorize_text(label, color, enabled), detail)
}

fn print_test_event_line(label: &str, detail: &str, color: TestOutputColor, enabled: bool) {
    println!("{}", test_event_line(label, detail, color, enabled));
}

fn note_line(detail: &str, enabled: bool) -> String {
    format!(
        "  {} {}",
        colorize_text("note:", TestOutputColor::Yellow, enabled),
        detail
    )
}

fn print_note_line(detail: &str, enabled: bool) {
    println!("{}", note_line(detail, enabled));
}

fn summary_line(summary: TestRunSummary, enabled: bool) -> String {
    let failed_color = if summary.failed == 0 {
        TestOutputColor::Green
    } else {
        TestOutputColor::Red
    };
    format!(
        "test result: {}, {}, {}",
        colorize_text(
            &format!("passed={}", summary.passed),
            TestOutputColor::Green,
            enabled
        ),
        colorize_text(&format!("failed={}", summary.failed), failed_color, enabled),
        colorize_text(
            &format!("total={}", summary.total),
            TestOutputColor::Cyan,
            enabled
        )
    )
}

#[cfg(test)]
fn summary_color(summary: TestRunSummary) -> TestOutputColor {
    if summary.failed == 0 {
        TestOutputColor::Cyan
    } else {
        TestOutputColor::Red
    }
}

fn print_summary(summary: TestRunSummary) {
    println!("{}", summary_line(summary, test_color_enabled()));
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{
        colorize_text, note_line, parse_test_options, resolve_test_script_path, summary_color,
        summary_line, test_color_enabled, test_event_line, TestMode, TestOutputColor,
        TestRunSummary,
    };
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_diagnostic_without_matching_source_keeps_only_known_location() {
        let script = super::TestScript {
            selector: "math".to_string(),
            file_path: "lib/tests/math.srt".to_string(),
            source: "assert_eq(1, 2)".to_string(),
        };
        let event = eldr::vm::VmTestEvent {
            path: vec!["suite".to_string(), "failure".to_string()],
            detail: None,
            kind: eldr::vm::VmTestEventKind::Failed,
            io: None,
            diagnostic: Some(eldr::vm::VmTestDiagnostic {
                kind: "Global::TestAssertionFailed".to_string(),
                message: "expected True, got False".to_string(),
                assertion: Some("assert_eq".to_string()),
                assertion_call_kind: Some(sindr::runtime::RuntimeCallKind::DirectFunction),
                file: "helper.srt".to_string(),
                line: 4,
                column: 3,
                span_start: 0,
                span_end: 15,
            }),
        };
        let rendered =
            super::render_test_event_diagnostic(&event, &script, &sindr::ir::Bytecode::default())
                .unwrap();
        assert_eq!(
            rendered,
            "Error: Global::TestAssertionFailed: expected True, got False\n  at helper.srt:4:3\n"
        );
    }

    #[test]
    fn test_options_require_single_selector() {
        let opts = parse_test_options(&["string".to_string()]).expect("selector should parse");
        assert_eq!(opts.mode, TestMode::One("string".to_string()));
        assert!(!opts.quiet);
        assert!(parse_test_options(&[]).is_err());
        assert!(parse_test_options(&["a".to_string(), "b".to_string()]).is_err());
    }

    #[test]
    fn test_options_accept_all_flag() {
        let opts = parse_test_options(&["--all".to_string()]).expect("--all should parse");
        assert_eq!(opts.mode, TestMode::All);
        assert!(!opts.quiet);
    }

    #[test]
    fn test_options_accept_quiet_flag() {
        let opts =
            parse_test_options(&["--quiet".to_string(), "string".to_string()]).expect("quiet");
        assert_eq!(opts.mode, TestMode::One("string".to_string()));
        assert!(opts.quiet);

        let opts = parse_test_options(&["--all".to_string(), "-q".to_string()]).expect("quiet all");
        assert_eq!(opts.mode, TestMode::All);
        assert!(opts.quiet);
    }

    #[test]
    fn parse_test_options_rejects_duplicate_quiet() {
        let err = parse_test_options(&[
            "--quiet".to_string(),
            "-q".to_string(),
            "string".to_string(),
        ])
        .expect_err("duplicate quiet flag must fail");

        assert_eq!(err.summary(), "test: --quiet may only be specified once");
    }

    #[test]
    fn parse_test_options_rejects_duplicate_all() {
        let err = parse_test_options(&["--all".to_string(), "--all".to_string()])
            .expect_err("duplicate all flag must fail");

        assert_eq!(err.summary(), "test: --all may only be specified once");
    }

    #[test]
    fn selector_rejects_parent_components() {
        let err =
            parse_test_options(&["../string".to_string()]).expect_err("parent selector must fail");

        assert_eq!(err.summary(), "test: selector must stay within lib/tests");

        for selector in [r"..\..\lib\kernel", r"lib\tests\..\kernel"] {
            let err = parse_test_options(&[selector.to_string()])
                .expect_err("backslash parent selector must fail");
            assert_eq!(err.summary(), "test: selector must stay within lib/tests");
        }
    }

    #[test]
    fn selector_rejects_absolute_paths() {
        let err = parse_test_options(&["/tmp/string".to_string()])
            .expect_err("absolute selector must fail");

        assert_eq!(err.summary(), "test: selector must stay within lib/tests");

        let err = parse_test_options(&[r"\tmp\string".to_string()])
            .expect_err("backslash absolute selector must fail");
        assert_eq!(err.summary(), "test: selector must stay within lib/tests");
    }

    #[test]
    fn selector_allows_all_flag_after_path_validation() {
        let opts = parse_test_options(&["--all".to_string()]).expect("--all should parse");

        assert_eq!(opts.mode, TestMode::All);
    }

    #[test]
    fn selector_resolves_into_lib_tests() {
        assert_eq!(
            resolve_test_script_path("string").unwrap(),
            Path::new("lib").join("tests").join("string.srt")
        );
        assert_eq!(
            resolve_test_script_path("string.srt").unwrap(),
            Path::new("lib").join("tests").join("string.srt")
        );
        assert_eq!(
            resolve_test_script_path(r"lib\tests\spec\example.srt").unwrap(),
            Path::new("lib").join("tests").join("spec/example.srt")
        );
    }

    #[test]
    fn test_event_line_colors_only_status_label() {
        let rendered = test_event_line("[PASS]", "Suite > case", TestOutputColor::Green, true);
        assert_eq!(rendered, "\x1b[32m[PASS]\x1b[0m Suite > case".to_string());
        assert_eq!(
            test_event_line("[PASS]", "Suite > case", TestOutputColor::Green, false),
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
            summary_line(passed, false),
            "test result: passed=2, failed=0, total=2"
        );
        assert_eq!(
            summary_line(passed, true),
            "test result: \x1b[32mpassed=2\x1b[0m, \x1b[32mfailed=0\x1b[0m, \x1b[36mtotal=2\x1b[0m"
        );
        assert_eq!(summary_color(passed), TestOutputColor::Cyan);
        assert_eq!(summary_color(failed), TestOutputColor::Red);
    }

    #[test]
    fn note_line_colors_only_note_label() {
        assert_eq!(
            note_line("expected 1, got 2", true),
            "  \x1b[33mnote:\x1b[0m expected 1, got 2"
        );
        assert_eq!(colorize_text("x", TestOutputColor::Red, false), "x");
    }

    #[test]
    fn test_color_env_trims_value() {
        let _guard = env_lock().lock().expect("env lock");
        let previous = std::env::var("SURTR_TEST_COLOR").ok();
        std::env::set_var("SURTR_TEST_COLOR", " always ");

        assert!(test_color_enabled());

        match previous {
            Some(value) => std::env::set_var("SURTR_TEST_COLOR", value),
            None => std::env::remove_var("SURTR_TEST_COLOR"),
        }
    }
}
