use std::fs;
use std::path::{Path, PathBuf};

use sindr::policy::SourceKind;

use crate::{
    AnalysisSpan, ExternalInputState, ModuleFileFingerprint, ModuleStage, ProjectBootSummary,
    ResolvedProjectPath, RunnerContext, RunnerDiagnostic, RunnerDiagnosticKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredProjectPath {
    pub declared_by: PathBuf,
    pub literal_or_glob: String,
    pub declaration_span: Option<AnalysisSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRunnerInput {
    pub project_file: PathBuf,
    pub selected_profile: String,
    pub normalized_args: Vec<(String, String)>,
    pub declared_paths: Vec<DeclaredProjectPath>,
    pub active_file_profiles: Vec<String>,
    pub boot_summary: ProjectBootSummary,
    pub external_inputs: Vec<ExternalInputState>,
}

pub fn resolve_project_runner(input: ProjectRunnerInput) -> RunnerContext {
    let mut resolved_paths = Vec::new();
    let mut diagnostics = Vec::new();
    let mut fingerprints = Vec::new();

    for declared_path in input.declared_paths {
        let expanded_files = expand_declared_path(&declared_path, &mut diagnostics);
        for path in &expanded_files {
            match fs::read_to_string(path) {
                Ok(source) => fingerprints.push(ModuleFileFingerprint {
                    path: path.clone(),
                    source_kind: SourceKind::DefinitionSource,
                    content_hash: stable_hash_text(&source),
                }),
                Err(_) => diagnostics.push(RunnerDiagnostic {
                    kind: RunnerDiagnosticKind::UnreadablePath,
                    path: Some(path.clone()),
                    span: declared_path.declaration_span,
                    message: format!("project path {} is not readable", path_value(path)),
                }),
            }
        }

        resolved_paths.push(ResolvedProjectPath {
            declared_by: declared_path.declared_by,
            literal_or_glob: declared_path.literal_or_glob,
            declaration_span: declared_path.declaration_span,
            expanded_files,
            source_kind: SourceKind::DefinitionSource,
        });
    }

    fingerprints.sort_by(|left, right| path_value(&left.path).cmp(&path_value(&right.path)));

    RunnerContext {
        project_file: input.project_file,
        selected_profile: input.selected_profile,
        normalized_args: input.normalized_args,
        resolved_paths,
        active_file_profiles: input.active_file_profiles,
        module_stages: vec![ModuleStage {
            files: fingerprints,
        }],
        boot_summary: input.boot_summary,
        external_inputs: input.external_inputs,
        diagnostics,
    }
}

fn expand_declared_path(
    declared_path: &DeclaredProjectPath,
    diagnostics: &mut Vec<RunnerDiagnostic>,
) -> Vec<PathBuf> {
    let base_dir = declared_path
        .declared_by
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let path = base_dir.join(&declared_path.literal_or_glob);

    if !has_wildcard(&declared_path.literal_or_glob) {
        if path.is_file() {
            return vec![path];
        }
        diagnostics.push(RunnerDiagnostic {
            kind: RunnerDiagnosticKind::UnreadablePath,
            path: Some(path.clone()),
            span: declared_path.declaration_span,
            message: format!("project path {} is not readable", path_value(&path)),
        });
        return Vec::new();
    }

    let Some(pattern_name) = path.file_name().and_then(|name| name.to_str()) else {
        diagnostics.push(RunnerDiagnostic {
            kind: RunnerDiagnosticKind::GlobNoMatch,
            path: Some(path.clone()),
            span: declared_path.declaration_span,
            message: format!(
                "project glob {} did not match any files",
                declared_path.literal_or_glob
            ),
        });
        return Vec::new();
    };
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let Ok(entries) = fs::read_dir(parent) else {
        diagnostics.push(RunnerDiagnostic {
            kind: RunnerDiagnosticKind::UnreadablePath,
            path: Some(parent.to_path_buf()),
            span: declared_path.declaration_span,
            message: format!(
                "project glob directory {} is not readable",
                path_value(parent)
            ),
        });
        return Vec::new();
    };

    let mut matches = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| wildcard_match(pattern_name, name))
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|path| path_value(path));

    if matches.is_empty() {
        diagnostics.push(RunnerDiagnostic {
            kind: RunnerDiagnosticKind::GlobNoMatch,
            path: Some(path),
            span: declared_path.declaration_span,
            message: format!(
                "project glob {} did not match any files",
                declared_path.literal_or_glob
            ),
        });
    }

    matches
}

fn has_wildcard(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?')
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    wildcard_match_inner(pattern.as_bytes(), text.as_bytes())
}

fn wildcard_match_inner(pattern: &[u8], text: &[u8]) -> bool {
    match (pattern.split_first(), text.split_first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some((&b'*', rest)), _) => {
            wildcard_match_inner(rest, text)
                || text
                    .split_first()
                    .is_some_and(|(_, tail)| wildcard_match_inner(pattern, tail))
        }
        (Some((&b'?', rest)), Some((_, tail))) => wildcard_match_inner(rest, tail),
        (Some((&expected, rest)), Some((&actual, tail))) if expected == actual => {
            wildcard_match_inner(rest, tail)
        }
        _ => false,
    }
}

fn stable_hash_text(text: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

fn path_value(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
