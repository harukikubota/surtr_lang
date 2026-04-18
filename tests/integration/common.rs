#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use xldr::ModuleInput;

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

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

fn leak_text(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn collect_files_with_extension(dir: &Path, ext: &str) -> Vec<PathBuf> {
    fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
        let entries = fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("failed to read fixture dir {}: {}", dir.display(), e));

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, ext, out);
            } else if path.extension().and_then(|value| value.to_str()) == Some(ext) {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    walk(dir, ext, &mut files);
    files.sort();
    files
}

fn sorted_immediate_subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read fixture dir {}: {}", dir.display(), e))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn is_module_fixture(path: &Path) -> bool {
    normalize_path(path).contains("/modules/")
}

fn module_path_from_fixture_file(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_else(|| panic!("module file stem must be valid utf-8: {}", path.display()))
        .replace("__", "::")
}

fn collect_module_fixture_stages(case_dir: &Path) -> Vec<Vec<ModuleInput>> {
    let explicit_stage_dirs = sorted_immediate_subdirs(case_dir)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("stage"))
        })
        .collect::<Vec<_>>();

    if explicit_stage_dirs.is_empty() {
        let stage = collect_files_with_extension(case_dir, "srt")
            .into_iter()
            .filter(|path| path.parent() == Some(case_dir))
            .filter(|path| path.file_name().and_then(|name| name.to_str()) != Some("entry.srt"))
            .map(|path| ModuleInput {
                file_name: path.to_string_lossy().into_owned(),
                source: read_text(&path),
                module_path: module_path_from_fixture_file(&path),
            })
            .collect::<Vec<_>>();
        if stage.is_empty() {
            Vec::new()
        } else {
            vec![stage]
        }
    } else {
        explicit_stage_dirs
            .into_iter()
            .map(|stage_dir| {
                collect_files_with_extension(&stage_dir, "srt")
                    .into_iter()
                    .map(|path| ModuleInput {
                        file_name: path.to_string_lossy().into_owned(),
                        source: read_text(&path),
                        module_path: module_path_from_fixture_file(&path),
                    })
                    .collect()
            })
            .collect()
    }
}

pub fn spec_fixtures() -> Vec<SpecFixture> {
    static FIXTURES: OnceLock<Vec<SpecFixture>> = OnceLock::new();

    FIXTURES
        .get_or_init(|| {
            let spec_root = repo_root().join("tests/spec");
            let mut fixtures = collect_files_with_extension(&spec_root, "srt")
                .into_iter()
                .filter(|path| !is_module_fixture(path))
                .filter_map(|path| {
                    let expected_path = path.with_extension("expected");
                    expected_path.exists().then(|| SpecFixture {
                        source_path: path.clone(),
                        source: leak_text(read_text(&path)),
                        expected: leak_text(read_text(&expected_path)),
                    })
                })
                .collect::<Vec<_>>();
            fixtures.sort_by(|a, b| a.source_path.cmp(&b.source_path));
            fixtures
        })
        .clone()
}

pub fn compile_error_fixtures() -> Vec<CompileErrorFixture> {
    static FIXTURES: OnceLock<Vec<CompileErrorFixture>> = OnceLock::new();

    FIXTURES
        .get_or_init(|| {
            let compile_errors_root = repo_root().join("tests/compile_errors");
            let mut fixtures = collect_files_with_extension(&compile_errors_root, "srt")
                .into_iter()
                .filter(|path| !is_module_fixture(path))
                .filter_map(|path| {
                    let error_path = path.with_extension("error");
                    error_path.exists().then(|| CompileErrorFixture {
                        source_path: path.clone(),
                        source: leak_text(read_text(&path)),
                        error_path,
                    })
                })
                .collect::<Vec<_>>();
            fixtures.sort_by(|a, b| a.source_path.cmp(&b.source_path));
            fixtures
        })
        .clone()
}

pub fn module_spec_fixtures() -> Vec<ModuleSpecFixtureCase> {
    static FIXTURES: OnceLock<Vec<ModuleSpecFixtureCase>> = OnceLock::new();

    FIXTURES
        .get_or_init(|| {
            let modules_root = repo_root().join("tests/spec/modules");
            let mut fixtures = sorted_immediate_subdirs(&modules_root)
                .into_iter()
                .filter_map(|case_dir| {
                    let entry_path = case_dir.join("entry.srt");
                    let expected_path = case_dir.join("entry.expected");
                    expected_path.exists().then(|| ModuleSpecFixtureCase {
                        case: ModuleFixtureCase {
                            case_dir: case_dir.clone(),
                            entry_path: entry_path.clone(),
                            entry_source: leak_text(read_text(&entry_path)),
                            module_stages: collect_module_fixture_stages(&case_dir),
                        },
                        expected_path: expected_path.clone(),
                        expected: leak_text(read_text(&expected_path)),
                    })
                })
                .collect::<Vec<_>>();
            fixtures.sort_by(|a, b| a.case.case_dir.cmp(&b.case.case_dir));
            fixtures
        })
        .clone()
}

pub fn module_compile_error_fixtures() -> Vec<ModuleCompileErrorFixtureCase> {
    static FIXTURES: OnceLock<Vec<ModuleCompileErrorFixtureCase>> = OnceLock::new();

    FIXTURES
        .get_or_init(|| {
            let modules_root = repo_root().join("tests/compile_errors/modules");
            let mut fixtures = sorted_immediate_subdirs(&modules_root)
                .into_iter()
                .filter_map(|case_dir| {
                    let entry_path = case_dir.join("entry.srt");
                    let error_path = case_dir.join("entry.error");
                    error_path.exists().then(|| ModuleCompileErrorFixtureCase {
                        case: ModuleFixtureCase {
                            case_dir: case_dir.clone(),
                            entry_path: entry_path.clone(),
                            entry_source: leak_text(read_text(&entry_path)),
                            module_stages: collect_module_fixture_stages(&case_dir),
                        },
                        error_path,
                    })
                })
                .collect::<Vec<_>>();
            fixtures.sort_by(|a, b| a.case.case_dir.cmp(&b.case.case_dir));
            fixtures
        })
        .clone()
}

pub fn parse_compile_error_expectation(path: &Path) -> CompileErrorExpectation {
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
