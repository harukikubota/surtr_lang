#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("failed to resolve repository root")
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
