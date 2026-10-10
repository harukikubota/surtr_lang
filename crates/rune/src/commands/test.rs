use std::env;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use eldr::vm::{VmTestDiagnostic, VmTestEvent, VmTestEventKind, VmTestPolicy};
use forge::bytecode::{stable_hash_hex, Bytecode};
use serde_json::{json, Value as JsonValue};
use spire::ast::Span;
use std::time::Instant;

use super::test_progress::TestProgress;
use crate::compile::{
    collect_default_script_compile_sources, compile_source, load_default_stdlib_snapshot,
    prepare_script_compile_plan, script_plan_error_as_rune_error,
};
use crate::error::{ExecutionEnv, RuneError, RuneResult};

const TEST_CACHE_VERSION: &str = "surtr-test-dsl-v2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestOptions {
    pub(crate) mode: TestMode,
    pub(crate) quiet: bool,
    pub(crate) list: bool,
    pub(crate) include_xit: bool,
    pub(crate) deny_pending: bool,
    pub(crate) timings: bool,
    pub(crate) format: TestFormat,
    pub(crate) test_filter: Option<String>,
    pub(crate) describe_filter: Option<String>,
    pub(crate) it_filter: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestFormat {
    Human,
    Json,
}

#[derive(Debug)]
enum TestArg {
    Positional(String),
    Flag(String),
    Value(String, String),
    Invalid(String),
}

// One token stream is used both for validation and usage-error format selection.
fn tokenize_test_args(args: &[String]) -> Vec<TestArg> {
    let mut tokens = Vec::new();
    let mut positional = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        index += 1;
        if positional {
            tokens.push(TestArg::Positional(arg.clone()));
            continue;
        }
        if arg == "--" {
            positional = true;
            continue;
        }
        let (key, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        if matches!(key, "--test" | "--describe" | "--it" | "--format") {
            let value = if let Some(value) = inline {
                Some(value.to_string())
            } else if args.get(index).is_some_and(|v| !v.starts_with('-')) {
                index += 1;
                Some(args[index - 1].clone())
            } else {
                None
            };
            match value {
                Some(value) => tokens.push(TestArg::Value(key.into(), value)),
                None => tokens.push(TestArg::Invalid(format!("test: {key} requires a value"))),
            }
        } else if matches!(
            key,
            "--all"
                | "--quiet"
                | "-q"
                | "--list"
                | "--include-xit"
                | "--deny-pending"
                | "--timings"
        ) && inline.is_none()
        {
            tokens.push(TestArg::Flag(
                if key == "-q" { "--quiet" } else { key }.into(),
            ));
        } else if arg.starts_with('-') {
            tokens.push(TestArg::Invalid(format!("test: unknown option `{arg}`")));
        } else {
            tokens.push(TestArg::Positional(arg.clone()));
        }
    }
    tokens
}

fn usage_test_format(tokens: &[TestArg]) -> Option<TestFormat> {
    let mut formats = Vec::new();
    for token in tokens {
        match token {
            TestArg::Value(key, value) if key == "--format" => formats.push(match value.as_str() {
                "human" => Some(TestFormat::Human),
                "json" => Some(TestFormat::Json),
                _ => None,
            }),
            TestArg::Invalid(message) if message == "test: --format requires a value" => {
                formats.push(None)
            }
            _ => (),
        }
    }
    if formats.len() == 1 {
        formats[0]
    } else {
        None
    }
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
    discovered: usize,
    selected: usize,
    executed: usize,
    skipped: usize,
    pending: usize,
    filtered: usize,
    runnable: usize,
    scope_failures: usize,
    script_errors: usize,
    policy_errors: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestScript {
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
    let tokens = tokenize_test_args(args);
    match parse_test_tokens(&tokens) {
        Ok(options) => test_command(options, ExecutionEnv::Test),
        Err(error) if usage_test_format(&tokens) == Some(TestFormat::Json) => {
            let report = error.to_serializable_report();
            println!(
                "{}",
                json!({
                    "command": "test", "mode": null, "options": null,
                    "scripts": [], "cases": [],
                    "errors": [{"kind": "usage", "message": report.errors[0].message, "diagnostic": report}],
                    "summary": summary_json(TestRunSummary::default()), "exit_code": 1, "duration_ns": null,
                })
            );
            Err(RuneError::silent(1))
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
pub(crate) fn parse_test_options(args: &[String]) -> RuneResult<TestOptions> {
    parse_test_tokens(&tokenize_test_args(args))
}

fn parse_test_tokens(tokens: &[TestArg]) -> RuneResult<TestOptions> {
    let mut options = TestOptions {
        mode: TestMode::All,
        quiet: false,
        list: false,
        include_xit: false,
        deny_pending: false,
        timings: false,
        format: TestFormat::Human,
        test_filter: None,
        describe_filter: None,
        it_filter: None,
    };
    let mut target = None;
    let mut seen = std::collections::HashSet::new();
    for token in tokens {
        match token {
            TestArg::Invalid(message) => return Err(RuneError::usage(message)),
            TestArg::Positional(value) => {
                if value.is_empty() {
                    return Err(RuneError::usage("test: file path must not be empty"));
                }
                if target.replace(TestMode::One(value.to_string())).is_some() {
                    return Err(RuneError::usage(
                        "test: expected exactly one test file path",
                    ));
                }
            }
            TestArg::Flag(key) | TestArg::Value(key, _) => {
                if !seen.insert(key) {
                    return Err(RuneError::usage(format!(
                        "test: {key} may only be specified once"
                    )));
                }
                match token {
                    TestArg::Flag(key) => match key.as_str() {
                        "--all" => {
                            if target.replace(TestMode::All).is_some() {
                                return Err(RuneError::usage(
                                    "test: expected exactly one test file path",
                                ));
                            }
                        }
                        "--quiet" => options.quiet = true,
                        "--list" => options.list = true,
                        "--include-xit" => options.include_xit = true,
                        "--deny-pending" => options.deny_pending = true,
                        "--timings" => options.timings = true,
                        _ => unreachable!("tokenizer supplies closed flag set"),
                    },
                    TestArg::Value(key, value) if key == "--format" => {
                        options.format = match value.as_str() {
                            "human" => TestFormat::Human,
                            "json" => TestFormat::Json,
                            _ => {
                                return Err(RuneError::usage(
                                    "test: --format must be human or json",
                                ))
                            }
                        }
                    }
                    TestArg::Value(key, value) => {
                        if value.trim().is_empty() {
                            return Err(RuneError::usage(format!("test: {key} must not be empty")));
                        }
                        *match key.as_str() {
                            "--test" => &mut options.test_filter,
                            "--describe" => &mut options.describe_filter,
                            "--it" => &mut options.it_filter,
                            _ => unreachable!("tokenizer supplies closed value option set"),
                        } = Some(value.clone());
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
    options.mode =
        target.ok_or_else(|| RuneError::usage("test: expected exactly one test file path"))?;
    Ok(options)
}

struct TestCaseRecord {
    file: String,
    event: VmTestEvent,
    rendered_diagnostic: Option<String>,
}
struct TestReportError {
    kind: &'static str,
    message: String,
    source: Option<RuneError>,
    diagnostic: Option<JsonValue>,
}
#[derive(Default)]
struct TestReport {
    cases: Vec<TestCaseRecord>,
    scripts: Vec<JsonValue>,
    errors: Vec<TestReportError>,
    summary: TestRunSummary,
}

fn test_command(options: TestOptions, env: ExecutionEnv) -> RuneResult<()> {
    let started = options.timings.then(Instant::now);
    let mut report = TestReport::default();
    let mut compile_context = TestCompileContext::default();
    let stderr = std::io::stderr();
    let terminal = stderr.is_terminal();
    let mut progress = TestProgress::new(stderr, terminal);
    let paths = match &options.mode {
        TestMode::One(file_path) => Ok(vec![file_path.clone()]),
        TestMode::All => collect_all_test_paths(),
    };
    match paths {
        Ok(paths) => {
            let total = paths.len();
            let mut compiled = Vec::with_capacity(total);
            for (index, file_path) in paths.into_iter().enumerate() {
                let position = TestFilePosition {
                    index: index + 1,
                    total,
                };
                let result = prepare_test_script(
                    &file_path,
                    env,
                    position,
                    &mut progress,
                    &mut compile_context,
                );
                let result = match result {
                    Ok(script) => Ok(script),
                    Err(TestCompileError::Compile(error)) => Err(error),
                    Err(TestCompileError::Progress(error)) => return Err(progress_error(error)),
                };
                compiled.push((file_path, result));
            }
            let compilation_failed = compiled.iter().any(|(_, result)| result.is_err());
            for (index, (file_path, result)) in compiled.into_iter().enumerate() {
                match result {
                    Err(error) => {
                        report.scripts.push(json!({
                            "file": file_path, "status": "aborted",
                            "io": {"stdout": [], "stderr": []},
                        }));
                        report.script_error(error);
                    }
                    Ok(_) if compilation_failed => report.scripts.push(json!({
                        "file": file_path, "status": "not_run",
                        "io": {"stdout": [], "stderr": []},
                    })),
                    Ok(compiled) => execute_test_script(
                        compiled,
                        &options,
                        &mut report,
                        TestFilePosition {
                            index: index + 1,
                            total,
                        },
                        &mut progress,
                    )?,
                }
            }
        }
        Err(error) => report.script_error(error),
    }
    if options.has_filters()
        && report.summary.selected == 0
        && report.summary.script_errors == 0
        && report.summary.scope_failures == 0
    {
        report.policy_error("test: no cases matched the specified filters".into());
    }
    progress.clear().map_err(progress_error)?;
    let duration_ns = started.map(|time| time.elapsed().as_nanos());
    let failed = report.summary.failed
        + report.summary.scope_failures
        + report.summary.script_errors
        + report.summary.policy_errors
        > 0;
    match options.format {
        TestFormat::Human => render_human_report(&report, &options, duration_ns, failed),
        TestFormat::Json => println!(
            "{}",
            json!({
                "command": "test", "mode": if options.list { "list" } else { "run" },
                "options": options.to_json(), "scripts": report.scripts,
                "cases": report.cases.iter().filter(|record| visible_case(&record.event, &options)).map(case_json).collect::<Vec<_>>(),
                "errors": report.errors.iter().map(|error| json!({
                    "kind": error.kind, "message": error.message,
                    "diagnostic": error.diagnostic,
                })).collect::<Vec<_>>(),
                "summary": summary_json(report.summary), "exit_code": if failed { 1 } else { 0 }, "duration_ns": duration_ns,
            })
        ),
    }
    if failed {
        Err(RuneError::silent(1))
    } else {
        Ok(())
    }
}
impl TestOptions {
    fn has_filters(&self) -> bool {
        self.test_filter.is_some() || self.describe_filter.is_some() || self.it_filter.is_some()
    }
    fn policy(&self) -> VmTestPolicy {
        VmTestPolicy {
            test_filter: self.test_filter.clone(),
            describe_filter: self.describe_filter.clone(),
            it_filter: self.it_filter.clone(),
            list: self.list,
            include_xit: self.include_xit,
            timings: self.timings,
        }
    }
    fn to_json(&self) -> JsonValue {
        json!({
            "target": match &self.mode { TestMode::All => json!({"all": true, "file": null}), TestMode::One(name) => json!({"all": false, "file": name}) },
            "filters": {"test": self.test_filter, "describe": self.describe_filter, "it": self.it_filter},
            "include_xit": self.include_xit, "deny_pending": self.deny_pending, "quiet": self.quiet,
            "timings": self.timings, "format": match self.format {TestFormat::Human => "human", TestFormat::Json => "json"},
        })
    }
}
impl TestReport {
    fn script_error(&mut self, error: RuneError) {
        self.summary.script_errors += 1;
        let message = error
            .to_serializable_report()
            .errors
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        let diagnostic = Some(json!(error.to_serializable_report()));
        self.errors.push(TestReportError {
            kind: "script",
            message,
            source: Some(error),
            diagnostic,
        });
    }
    fn policy_error(&mut self, message: String) {
        self.summary.policy_errors += 1;
        self.errors.push(TestReportError {
            kind: "policy",
            message,
            source: None,
            diagnostic: None,
        });
    }
}
struct CompiledTestScript {
    script: TestScript,
    bytecode: Bytecode,
}

fn prepare_test_script(
    file_path: &str,
    env: ExecutionEnv,
    position: TestFilePosition,
    progress: &mut TestProgress<std::io::Stderr>,
    compile_context: &mut TestCompileContext,
) -> Result<CompiledTestScript, TestCompileError> {
    progress.compiling(position.index, position.total, file_path)?;
    let script = load_test_script(file_path)?;
    let bytecode = compile_test_script(&script, env, position, progress, compile_context)?;
    Ok(CompiledTestScript { script, bytecode })
}

fn execute_test_script(
    compiled: CompiledTestScript,
    options: &TestOptions,
    report: &mut TestReport,
    position: TestFilePosition,
    progress: &mut TestProgress<std::io::Stderr>,
) -> RuneResult<()> {
    let CompiledTestScript { script, bytecode } = compiled;
    progress
        .running(position.index, position.total, &script.file_path)
        .map_err(progress_error)?;
    let mut vm = eldr::VM::new(bytecode)
        .with_source(script.source.clone(), script.file_path.clone())
        .with_output_capture()
        .with_error_capture()
        .with_test_policy(options.policy());
    let result = vm.run();
    for event in vm.test_events() {
        if event.kind == VmTestEventKind::ScopeFailed {
            report.summary.scope_failures += 1;
            report.errors.push(TestReportError {
                kind: "scope",
                message: format!(
                    "{} ({}): {}",
                    format_event_path(event),
                    script.file_path,
                    event.detail.as_deref().unwrap_or("scope failed")
                ),
                source: None,
                diagnostic: None,
            });
            continue;
        }
        let case = event.case.as_ref().expect("case events have case metadata");
        report.summary.discovered += 1;
        report.summary.selected += usize::from(case.selected);
        match event.kind {
            VmTestEventKind::Passed => report.summary.passed += 1,
            VmTestEventKind::Failed => report.summary.failed += 1,
            VmTestEventKind::Skipped => report.summary.skipped += 1,
            VmTestEventKind::Pending => {
                report.summary.pending += 1;
                if options.deny_pending {
                    report.policy_error(format!(
                        "pending case denied: {} ({}): {}",
                        format_event_path(event),
                        script.file_path,
                        case.reason.as_deref().expect("pending reason")
                    ));
                }
            }
            VmTestEventKind::Filtered => report.summary.filtered += 1,
            VmTestEventKind::Runnable => report.summary.runnable += 1,
            VmTestEventKind::ScopeFailed => unreachable!(),
        }
        report.cases.push(TestCaseRecord {
            file: script.file_path.clone(),
            event: event.clone(),
            rendered_diagnostic: render_test_event_diagnostic(event, &script, vm.bytecode()),
        });
    }
    report.summary.executed = report.summary.passed + report.summary.failed;
    report.summary.total = report.summary.discovered;
    report.scripts.push(json!({
        "file": script.file_path, "status": if result.is_ok() {"completed"} else {"aborted"},
        "io": {"stdout": vm.take_stdout(), "stderr": vm.take_stderr()},
    }));
    if let Err(error) = result {
        let diagnostic = json!({
            "message": error.message, "pc": error.context.pc, "opcode": error.context.opcode,
            "function": error.context.function, "call_site": super::run::location_json(error.context.call_site.as_ref()),
            "details": error.context.details, "stack_trace": super::run::stack_trace_json(&error.context.stack_trace),
        });
        report.script_error(RuneError::message(
            1,
            format!(
                "runtime error while running test script {}: {error}",
                script.file_path
            ),
        ));
        report
            .errors
            .last_mut()
            .expect("recorded runtime error")
            .diagnostic = Some(diagnostic);
    }
    Ok(())
}
fn progress_error(error: std::io::Error) -> RuneError {
    RuneError::message(1, format!("test: failed to write progress: {error}"))
}
fn visible_case(event: &VmTestEvent, options: &TestOptions) -> bool {
    !options.quiet
        || (options.list && event.case.as_ref().is_some_and(|case| case.selected))
        || event.kind == VmTestEventKind::Failed
        || (event.kind == VmTestEventKind::Pending && options.deny_pending)
}
fn event_status(kind: &VmTestEventKind) -> &'static str {
    match kind {
        VmTestEventKind::Passed => "passed",
        VmTestEventKind::Failed => "failed",
        VmTestEventKind::Skipped => "skipped",
        VmTestEventKind::Pending => "pending",
        VmTestEventKind::Filtered => "filtered",
        VmTestEventKind::Runnable => "runnable",
        VmTestEventKind::ScopeFailed => "scope_failed",
    }
}
fn case_json(record: &TestCaseRecord) -> JsonValue {
    let event = &record.event;
    let case = event.case.as_ref().expect("case record");
    json!({
        "file": record.file, "case_index": case.case_index,
        "scopes": case.scopes.iter().map(|scope| json!({"kind": scope.kind.as_str(), "name": scope.name})).collect::<Vec<_>>(),
        "name": case.name, "declaration": case.declaration.as_str(), "selected": case.selected,
        "status": event_status(&event.kind), "reason": case.reason, "detail": event.detail, "duration_ns": case.duration_ns,
        "io": event.io.as_ref().map(|io| json!({"stdout": io.stdout, "stderr": io.stderr})),
        "diagnostic": event.diagnostic.as_ref().map(|d| json!({
            "kind": d.kind, "message": d.message, "assertion": d.assertion,
            "assertion_call_kind": d.assertion_call_kind.as_ref().map(|kind| format!("{kind:?}")),
            "file": d.file, "line": d.line, "column": d.column, "span_start": d.span_start, "span_end": d.span_end,
        })),
    })
}
fn summary_json(s: TestRunSummary) -> JsonValue {
    json!({"discovered": s.discovered, "selected": s.selected, "executed": s.executed,
        "passed": s.passed, "failed": s.failed, "skipped": s.skipped, "pending": s.pending,
        "filtered": s.filtered, "runnable": s.runnable, "scope_failures": s.scope_failures,
        "script_errors": s.script_errors, "policy_errors": s.policy_errors})
}
fn render_human_report(
    report: &TestReport,
    options: &TestOptions,
    duration_ns: Option<u128>,
    failed: bool,
) {
    let color = test_color_enabled();
    for record in report
        .cases
        .iter()
        .filter(|record| visible_case(&record.event, options))
    {
        let event = &record.event;
        let case = event.case.as_ref().expect("case record");
        let (label, tint) = if options.list {
            ("[LIST]", TestOutputColor::Cyan)
        } else {
            match event.kind {
                VmTestEventKind::Passed => ("[PASS]", TestOutputColor::Green),
                VmTestEventKind::Failed => ("[FAIL]", TestOutputColor::Red),
                VmTestEventKind::Skipped => ("[SKIP]", TestOutputColor::Yellow),
                VmTestEventKind::Pending => ("[PENDING]", TestOutputColor::Yellow),
                VmTestEventKind::Filtered => ("[FILTERED]", TestOutputColor::Cyan),
                _ => unreachable!("execution case state"),
            }
        };
        let mut detail = if options.list {
            case.scopes
                .iter()
                .map(|scope| format!("{}:{}", scope.kind.as_str(), scope.name))
                .chain(std::iter::once(case.name.clone()))
                .collect::<Vec<_>>()
                .join(" > ")
        } else {
            format_event_path(event)
        };
        if options.list {
            detail.push_str(&format!(
                " [{} {}] ({}, case {})",
                case.declaration.as_str(),
                event_status(&event.kind),
                record.file,
                case.case_index
            ));
        } else if event.kind == VmTestEventKind::Failed {
            detail.push_str(&format!(" ({})", record.file));
        }
        if let Some(reason) = &case.reason {
            detail.push_str(&format!(" — {reason}"));
        }
        if let Some(ns) = case.duration_ns {
            detail.push_str(&format!(" ({:.3} ms)", ns as f64 / 1_000_000.0));
        }
        print_test_event_line(label, &detail, tint, color);
        if let Some(diagnostic) = &record.rendered_diagnostic {
            print!("{diagnostic}");
            if !diagnostic.ends_with('\n') {
                println!();
            }
        } else if let Some(detail) = &event.detail {
            print_note_line(detail, color);
        }
    }
    for error in &report.errors {
        if let Some(source) = &error.source {
            source.emit();
        } else {
            print_test_event_line("[FAIL]", &error.message, TestOutputColor::Red, color);
        }
    }
    if !options.quiet || failed {
        let s = report.summary;
        let mut line = if options.list {
            format!("test list: runnable={}", s.runnable)
        } else {
            summary_line(s, color)
        };
        line.push_str(&format!(", discovered={}, selected={}, executed={}, skipped={}, pending={}, filtered={}, scope_failures={}, script_errors={}, policy_errors={}", s.discovered, s.selected, s.executed, s.skipped, s.pending, s.filtered, s.scope_failures, s.script_errors, s.policy_errors));
        if let Some(ns) = duration_ns {
            line.push_str(&format!(", duration={:.3} ms", ns as f64 / 1_000_000.0));
        }
        println!("{line}");
    }
}

fn load_test_script(file_path: &str) -> RuneResult<TestScript> {
    let path = Path::new(file_path);
    let source = fs::read_to_string(path).map_err(|e| {
        RuneError::message(
            1,
            format!("test: failed to read {}: {}", display_path(path), e),
        )
    })?;
    Ok(TestScript {
        file_path: display_path(path),
        source,
    })
}

fn collect_all_test_paths() -> RuneResult<Vec<String>> {
    let root = Path::new("lib/tests");
    let mut paths = Vec::new();
    for category in read_test_directory(root)? {
        if category.is_dir() {
            for path in read_test_directory(&category)? {
                if (path.is_file() || path.is_symlink())
                    && path.extension().and_then(|ext| ext.to_str()) == Some("srt")
                {
                    paths.push(display_path(&path));
                }
            }
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(RuneError::message(
            1,
            "test: no test files found in lib/tests/*/*.srt",
        ));
    }
    Ok(paths)
}

fn read_test_directory(dir: &Path) -> RuneResult<Vec<PathBuf>> {
    fs::read_dir(dir)
        .map_err(|e| {
            RuneError::message(
                1,
                format!(
                    "test: failed to read test directory {}: {}",
                    display_path(dir),
                    e
                ),
            )
        })?
        .map(|entry| {
            entry.map(|entry| entry.path()).map_err(|e| {
                RuneError::message(
                    1,
                    format!("test: failed to read test directory entry: {}", e),
                )
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
struct TestFilePosition {
    index: usize,
    total: usize,
}

#[derive(Default)]
struct TestCompileContext {
    fingerprints: Option<TestCacheFingerprints>,
    standard_prepared: bool,
}

struct TestCacheFingerprints {
    binary: String,
    library: String,
}

enum TestCompileError {
    Compile(RuneError),
    Progress(std::io::Error),
}

impl From<RuneError> for TestCompileError {
    fn from(error: RuneError) -> Self {
        Self::Compile(error)
    }
}

impl From<std::io::Error> for TestCompileError {
    fn from(error: std::io::Error) -> Self {
        Self::Progress(error)
    }
}

fn compile_test_script(
    script: &TestScript,
    env: ExecutionEnv,
    position: TestFilePosition,
    progress: &mut TestProgress<std::io::Stderr>,
    context: &mut TestCompileContext,
) -> Result<Bytecode, TestCompileError> {
    let compile_plan = prepare_script_compile_plan(&script.file_path, &script.source, None)
        .map_err(|e| script_plan_error_as_rune_error(&script.file_path, &script.source, e))?;
    if context.fingerprints.is_none() {
        context.fingerprints = Some(TestCacheFingerprints {
            binary: binary_fingerprint()?,
            library: library_sources_fingerprint()?,
        });
    }
    let cache_path = cached_eldr_path(
        script,
        &compile_plan.include_directives,
        context
            .fingerprints
            .as_ref()
            .expect("test cache fingerprints prepared"),
    )?;
    if let Some(bytecode) = load_cached_bytecode(&cache_path)? {
        return Ok(bytecode);
    }

    if !context.standard_prepared {
        progress.preparing()?;
    }
    let compile_sources = collect_default_script_compile_sources(
        env,
        &script.file_path,
        &compile_plan.source_for_parse,
        &compile_plan.include_modules,
        xldr::StdlibVariant::TestEnabled,
    )?;
    if !context.standard_prepared {
        load_default_stdlib_snapshot(env, &compile_sources)?;
        context.standard_prepared = true;
    }
    progress.compiling(position.index, position.total, &script.file_path)?;
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
        payload.push_str(&format!("{:?}", module.module_path));
        payload.push('\x1f');
        payload.push_str(&stable_hash_hex(&module.source));
        payload.push('\x1e');
    }
    Ok(stable_hash_hex(&payload))
}

fn cached_eldr_path(
    script: &TestScript,
    include_directives: &[xldr::ScriptIncludeDirective],
    fingerprints: &TestCacheFingerprints,
) -> Result<PathBuf, RuneError> {
    let mut key = String::new();
    key.push_str(TEST_CACHE_VERSION);
    key.push('\x1f');
    key.push_str(&fingerprints.binary);
    key.push('\x1f');
    key.push_str(&fingerprints.library);
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

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::{
        colorize_text, note_line, parse_test_options, summary_color, summary_line,
        test_color_enabled, test_event_line, TestMode, TestOutputColor, TestRunSummary,
    };
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_diagnostic_without_matching_source_keeps_only_known_location() {
        let script = super::TestScript {
            file_path: "lib/tests/math.srt".to_string(),
            source: "assert_eq(1, 2)".to_string(),
        };
        let event = eldr::vm::VmTestEvent {
            case: None,
            path: vec!["suite".to_string(), "failure".to_string()],
            detail: None,
            kind: eldr::vm::VmTestEventKind::Failed,
            io: None,
            diagnostic: Some(eldr::vm::VmTestDiagnostic {
                kind: "Global::TestExpectedTrue".to_string(),
                message: "expected True, got False".to_string(),
                assertion: Some("assert_true".to_string()),
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
            "Error: Global::TestExpectedTrue: expected True, got False\n  at helper.srt:4:3\n"
        );
    }

    #[test]
    fn test_options_accept_all_1024_combinations() {
        for bits in 0..1024 {
            let mut args = vec![if bits & 1 == 0 { "string" } else { "--all" }.to_string()];
            for (bit, flag) in [
                (1, "--list"),
                (2, "--quiet"),
                (3, "--include-xit"),
                (4, "--deny-pending"),
                (5, "--timings"),
            ] {
                if bits & (1 << bit) != 0 {
                    args.push(flag.into());
                }
            }
            args.extend([
                "--format".into(),
                if bits & 64 == 0 { "human" } else { "json" }.into(),
            ]);
            for (bit, flag) in [(7, "--test"), (8, "--describe"), (9, "--it")] {
                if bits & (1 << bit) != 0 {
                    args.extend([flag.into(), "name".into()]);
                }
            }
            let options = parse_test_options(&args)
                .unwrap_or_else(|_| panic!("combination {bits}: {args:?}"));
            assert_eq!(
                options.mode,
                if bits & 1 == 0 {
                    TestMode::One("string".into())
                } else {
                    TestMode::All
                }
            );
            assert_eq!(options.list, bits & 2 != 0);
            assert_eq!(options.quiet, bits & 4 != 0);
            assert_eq!(options.include_xit, bits & 8 != 0);
            assert_eq!(options.deny_pending, bits & 16 != 0);
            assert_eq!(options.timings, bits & 32 != 0);
            assert_eq!(
                options.format,
                if bits & 64 == 0 {
                    super::TestFormat::Human
                } else {
                    super::TestFormat::Json
                }
            );
            assert_eq!(
                options.test_filter.as_deref(),
                (bits & 128 != 0).then_some("name")
            );
            assert_eq!(
                options.describe_filter.as_deref(),
                (bits & 256 != 0).then_some("name")
            );
            assert_eq!(
                options.it_filter.as_deref(),
                (bits & 512 != 0).then_some("name")
            );
        }
    }

    #[test]
    fn test_options_value_forms_terminator_and_rejections() {
        let parse = |args: &[&str]| {
            parse_test_options(&args.iter().map(|v| v.to_string()).collect::<Vec<_>>())
        };
        let left = parse(&[
            "--test",
            " A ",
            "--describe",
            "B",
            "math",
            "--it=-name",
            "--format",
            "json",
        ])
        .unwrap();
        let right = parse(&[
            "--format=json",
            "--it=-name",
            "--describe=B",
            "--test= A ",
            "math",
        ])
        .unwrap();
        assert_eq!(left, right);
        assert_eq!(left.test_filter.as_deref(), Some(" A "));
        assert_eq!(
            parse(&["--", "--all"]).unwrap().mode,
            TestMode::One("--all".into())
        );
        for args in [
            vec!["math", "--it", "--list"],
            vec!["math", "--it"],
            vec!["math", "--it= "],
            vec!["math", "--it", "-name"],
            vec!["math", "--format=yaml"],
            vec!["math", "--unknown"],
            vec!["math", "--list=true"],
            vec!["--all", "math"],
            vec!["math", "--all"],
            vec!["--all", "--", "--all"],
            vec!["--", "a", "b"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        for flag in [
            "--list",
            "--include-xit",
            "--deny-pending",
            "--timings",
            "--quiet",
        ] {
            assert!(parse(&["math", flag, flag]).is_err());
        }
        for flag in ["--test", "--describe", "--it", "--format"] {
            let value = if flag == "--format" { "human" } else { "name" };
            assert!(parse(&["math", flag, value, flag, value]).is_err());
        }
    }

    #[test]
    fn usage_format_uses_the_same_tokenization_as_validation() {
        let format = |args: &[&str]| {
            super::usage_test_format(&super::tokenize_test_args(
                &args.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
            ))
        };
        assert_eq!(
            format(&["--bad", "--format=json"]),
            Some(super::TestFormat::Json)
        );
        assert_eq!(
            format(&["--it", "--format", "json"]),
            Some(super::TestFormat::Json)
        );
        for args in [
            vec!["--format=json", "--format=human"],
            vec!["--format=json", "--format"],
            vec!["--format=yaml"],
            vec!["--", "--format=json"],
        ] {
            assert_eq!(format(&args), None, "{args:?}");
        }
    }

    #[test]
    fn test_options_require_single_file_path() {
        let opts = parse_test_options(&["string".to_string()]).expect("file_path should parse");
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
    fn test_options_preserve_file_paths() {
        for path in [
            "./string.srt",
            "../string.srt",
            "/tmp/string.srt",
            " string ",
            r"lib\tests\string.srt",
            "result",
        ] {
            let opts = parse_test_options(&[path.to_string()]).expect("file path should parse");
            assert_eq!(opts.mode, TestMode::One(path.to_string()));
        }
        assert!(parse_test_options(&[String::new()]).is_err());
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
            ..TestRunSummary::default()
        };
        let failed = TestRunSummary {
            passed: 1,
            failed: 1,
            total: 2,
            ..TestRunSummary::default()
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
