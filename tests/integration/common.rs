#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use xldr::ModuleInput;

mod generated_fixture_registry {
    include!(concat!(env!("OUT_DIR"), "/generated_fixture_registry.rs"));
}

pub fn repo_root() -> PathBuf {
    static REPO_ROOT: OnceLock<PathBuf> = OnceLock::new();
    REPO_ROOT
        .get_or_init(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .expect("failed to resolve repository root")
        })
        .clone()
}

pub fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_string()
}

pub fn surtr_bin() -> String {
    if let Ok(path) = env::var("CARGO_BIN_EXE_surtr") {
        return path;
    }

    let mut path = env::current_exe().expect("failed to locate current test executable");
    path.pop();
    path.pop();
    path.push("surtr");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    assert!(
        path.exists(),
        "surtr binary not found at {}",
        path.display()
    );
    path.to_string_lossy().into_owned()
}

pub fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();
    let dir = env::temp_dir().join(format!("surtr-{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

pub fn write_source(path: &Path, source: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("failed to create parent directory");
    }
    fs::write(path, source).expect("failed to write source file");
}

#[derive(Debug)]
pub struct CompileErrorExpectation {
    pub phase: Option<String>,
    pub contains: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SpecFixture {
    pub source_path: PathBuf,
    pub source: &'static str,
    pub expected: &'static str,
}

#[derive(Debug, Clone)]
pub struct CompileErrorFixture {
    pub source_path: PathBuf,
    pub source: &'static str,
    pub error_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ModuleFixtureCase {
    pub case_dir: PathBuf,
    pub entry_path: PathBuf,
    pub entry_source: &'static str,
    pub module_stages: Vec<Vec<ModuleInput>>,
}

#[derive(Debug, Clone)]
pub struct ModuleSpecFixtureCase {
    pub case: ModuleFixtureCase,
    pub expected_path: PathBuf,
    pub expected: &'static str,
}

#[derive(Debug, Clone)]
pub struct ModuleCompileErrorFixtureCase {
    pub case: ModuleFixtureCase,
    pub error_path: PathBuf,
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn generated_compile_error_expectation(path: &Path) -> Option<CompileErrorExpectation> {
    let relative = path.strip_prefix(repo_root()).ok()?;
    let relative = normalize_path(relative);
    let entry = generated_fixture_registry::GENERATED_COMPILE_ERROR_EXPECTATIONS
        .iter()
        .find(|entry| entry.path == relative)?;
    Some(CompileErrorExpectation {
        phase: entry.phase.map(ToString::to_string),
        contains: entry
            .contains
            .iter()
            .map(|item| (*item).to_string())
            .collect(),
    })
}

pub fn spec_fixtures() -> Vec<SpecFixture> {
    generated_fixture_registry::GENERATED_SPEC_FIXTURES
        .iter()
        .map(|fixture| SpecFixture {
            source_path: repo_root().join(fixture.path),
            source: fixture.source,
            expected: fixture.expected,
        })
        .collect()
}

pub fn compile_error_fixtures() -> Vec<CompileErrorFixture> {
    generated_fixture_registry::GENERATED_COMPILE_ERROR_FIXTURES
        .iter()
        .map(|fixture| CompileErrorFixture {
            source_path: repo_root().join(fixture.path),
            source: fixture.source,
            error_path: repo_root().join(fixture.error_path),
        })
        .collect()
}

fn generated_module_stages(
    stages: &[generated_fixture_registry::GeneratedModuleStage],
) -> Vec<Vec<ModuleInput>> {
    stages
        .iter()
        .map(|stage| {
            stage
                .files
                .iter()
                .map(|file| ModuleInput {
                    file_name: repo_root()
                        .join(file.file_name)
                        .to_string_lossy()
                        .into_owned(),
                    source: file.source.to_string(),
                    module_path: file.module_path.to_string(),
                })
                .collect()
        })
        .collect()
}

fn base_module_fixture_case(
    case_dir: &str,
    entry_path: &str,
    entry_source: &'static str,
) -> ModuleFixtureCase {
    ModuleFixtureCase {
        case_dir: repo_root().join(case_dir),
        entry_path: repo_root().join(entry_path),
        entry_source,
        module_stages: Vec::new(),
    }
}

pub fn module_spec_fixtures() -> Vec<ModuleSpecFixtureCase> {
    generated_fixture_registry::GENERATED_MODULE_SPEC_CASES
        .iter()
        .map(|fixture| {
            let mut case = base_module_fixture_case(
                fixture.case_dir,
                fixture.entry_path,
                fixture.entry_source,
            );
            case.module_stages = generated_module_stages(fixture.stages);
            ModuleSpecFixtureCase {
                case,
                expected_path: repo_root().join(fixture.expected_path),
                expected: fixture.expected,
            }
        })
        .collect()
}

pub fn module_compile_error_fixtures() -> Vec<ModuleCompileErrorFixtureCase> {
    generated_fixture_registry::GENERATED_MODULE_COMPILE_ERROR_CASES
        .iter()
        .map(|fixture| {
            let mut case = base_module_fixture_case(
                fixture.case_dir,
                fixture.entry_path,
                fixture.entry_source,
            );
            case.module_stages = generated_module_stages(fixture.stages);
            ModuleCompileErrorFixtureCase {
                case,
                error_path: repo_root().join(fixture.error_path),
            }
        })
        .collect()
}

pub fn parse_compile_error_expectation(path: &Path) -> CompileErrorExpectation {
    if let Some(expectation) = generated_compile_error_expectation(path) {
        return expectation;
    }

    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    let mut phase = None;
    let mut contains = Vec::new();

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("phase:") {
            phase = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("contains:") {
            contains.push(rest.trim().to_string());
            continue;
        }
        panic!(
            "invalid compile error expectation line in {}: {}",
            path.display(),
            line
        );
    }

    CompileErrorExpectation { phase, contains }
}

pub fn extract_phase_tag(message: &str) -> Option<&str> {
    message
        .strip_prefix("phase=")
        .and_then(|rest| rest.split_once(';').map(|(phase, _)| phase))
}
