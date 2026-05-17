use std::fs;
use std::path::{Path, PathBuf};

use sindr::policy::{CompileUnitKind, SourceKind};
use spire::ast::{Ast, Lit, RecordLitArg, Span};

use crate::{
    parse_document, AnalysisSpan, ExternalInputState, ModuleFileFingerprint, ModuleStage,
    ProjectBootSummary, ResolvedProjectPath, RunnerContext, RunnerDiagnostic, RunnerDiagnosticKind,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRunnerSourceInput {
    pub project_file: PathBuf,
    pub selected_profile: String,
    pub normalized_args: Vec<(String, String)>,
    pub active_file: Option<PathBuf>,
    pub source: String,
}

pub fn extract_project_runner_input(
    input: ProjectRunnerSourceInput,
) -> Result<ProjectRunnerInput, Vec<RunnerDiagnostic>> {
    let ast = parse_document(
        &input.source,
        0,
        SourceKind::DefinitionSource,
        CompileUnitKind::Project,
        None,
    )
    .map_err(|error| {
        let span = error.span();
        vec![RunnerDiagnostic {
            kind: RunnerDiagnosticKind::ProjectSourceParseError,
            path: Some(input.project_file.clone()),
            span: Some(analysis_span(span)),
            message: error.message(),
        }]
    })?;

    let mut extractor = ProjectRunnerExtractor {
        project_file: input.project_file.clone(),
        selected_profile: input.selected_profile.clone(),
        profiles: Vec::new(),
        profile_declared_paths: Vec::new(),
        selected_declared_paths: Vec::new(),
    };
    extractor.visit_many(&ast);

    if !extractor
        .profiles
        .iter()
        .any(|profile| profile == &input.selected_profile)
    {
        return Err(vec![RunnerDiagnostic {
            kind: RunnerDiagnosticKind::ProjectProfileUnknown,
            path: Some(input.project_file.clone()),
            span: None,
            message: format!(
                "project profile {} was not found in {}",
                input.selected_profile,
                path_value(&input.project_file)
            ),
        }]);
    }

    let active_file_profiles = input
        .active_file
        .as_ref()
        .map(|active_file| extractor.active_file_profiles(active_file))
        .unwrap_or_else(|| extractor.profiles.clone());

    Ok(ProjectRunnerInput {
        project_file: input.project_file,
        selected_profile: input.selected_profile,
        normalized_args: input.normalized_args,
        declared_paths: extractor.selected_declared_paths,
        active_file_profiles,
        boot_summary: ProjectBootSummary::default(),
        external_inputs: Vec::new(),
    })
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

struct ProjectRunnerExtractor {
    project_file: PathBuf,
    selected_profile: String,
    profiles: Vec<String>,
    profile_declared_paths: Vec<ProfileDeclaredPaths>,
    selected_declared_paths: Vec<DeclaredProjectPath>,
}

impl ProjectRunnerExtractor {
    fn active_file_profiles(&self, active_file: &Path) -> Vec<String> {
        let active_file = normalized_path_value(active_file);
        let mut profiles = Vec::new();
        for profile in &self.profile_declared_paths {
            let mut ignored_diagnostics = Vec::new();
            let contains_active_file = profile.declared_paths.iter().any(|declared_path| {
                if declared_path_matches_active_file(declared_path, &active_file) {
                    return true;
                }
                expand_declared_path(declared_path, &mut ignored_diagnostics)
                    .iter()
                    .any(|path| normalized_path_value(path) == active_file)
            });
            if contains_active_file {
                profiles.push(profile.profile.clone());
            }
        }
        profiles
    }

    fn visit_many(&mut self, ast: &[Ast]) {
        for node in ast {
            self.visit(node);
        }
    }

    fn visit(&mut self, node: &Ast) {
        if let Some((profile, builder)) = entrypoint_builder(node) {
            if !self.profiles.iter().any(|known| known == profile) {
                self.profiles.push(profile.to_string());
            }
            let mut declared_paths = Vec::new();
            collect_add_paths(builder, &self.project_file, &mut declared_paths);
            self.profile_declared_paths.push(ProfileDeclaredPaths {
                profile: profile.to_string(),
                declared_paths: declared_paths.clone(),
            });
            if profile == self.selected_profile {
                self.selected_declared_paths.extend(declared_paths);
            }
        }

        match node {
            Ast::App(_, callee, args) => {
                self.visit(callee);
                for arg in positional_args(args) {
                    self.visit(arg);
                }
            }
            Ast::Block(_, nodes) => self.visit_many(nodes),
            Ast::Pipe(_, left, right)
            | Ast::ContextMap(_, left, right)
            | Ast::ContextBind(_, left, right)
            | Ast::Compose(_, left, right)
            | Ast::LiftedCompose(_, left, right)
            | Ast::KleisliCompose(_, left, right)
            | Ast::BinOp(_, _, left, right) => {
                self.visit(left);
                self.visit(right);
            }
            Ast::Grouped(_, inner)
            | Ast::Closure(_, _, inner)
            | Ast::Capture(_, inner, _)
            | Ast::Semi(_, inner)
            | Ast::FieldAccess(_, inner, _)
            | Ast::FacetSegmentAccess(_, inner, _)
            | Ast::FacetCapture(_, inner) => self.visit(inner),
            Ast::Bind(_, _, expr) | Ast::SafeBind(_, _, expr) => self.visit(expr),
            _ => {}
        }
    }
}

fn declared_path_matches_active_file(
    declared_path: &DeclaredProjectPath,
    active_file: &str,
) -> bool {
    if has_wildcard(&declared_path.literal_or_glob) {
        return false;
    }
    let base_dir = declared_path
        .declared_by
        .parent()
        .unwrap_or_else(|| Path::new(""));
    normalized_path_value(&base_dir.join(&declared_path.literal_or_glob)) == active_file
}

struct ProfileDeclaredPaths {
    profile: String,
    declared_paths: Vec<DeclaredProjectPath>,
}

fn entrypoint_builder(node: &Ast) -> Option<(&str, &Ast)> {
    let Ast::App(_, callee, args) = node else {
        return None;
    };
    if !is_path(callee, &["Project", "entrypoint"]) {
        return None;
    }
    let args = positional_args(args);
    let profile = string_lit(args.get(1).copied()?)?;
    let builder = args.get(2).copied()?;
    Some((profile, builder))
}

fn collect_add_paths(
    node: &Ast,
    project_file: &Path,
    declared_paths: &mut Vec<DeclaredProjectPath>,
) {
    if let Some((literal_or_glob, span)) = add_path_literal(node) {
        declared_paths.push(DeclaredProjectPath {
            declared_by: project_file.to_path_buf(),
            literal_or_glob: literal_or_glob.to_string(),
            declaration_span: Some(analysis_span(span)),
        });
    }

    match node {
        Ast::App(_, callee, args) => {
            collect_add_paths(callee, project_file, declared_paths);
            for arg in positional_args(args) {
                collect_add_paths(arg, project_file, declared_paths);
            }
        }
        Ast::Block(_, nodes) => {
            for node in nodes {
                collect_add_paths(node, project_file, declared_paths);
            }
        }
        Ast::Pipe(_, left, right)
        | Ast::ContextMap(_, left, right)
        | Ast::ContextBind(_, left, right)
        | Ast::Compose(_, left, right)
        | Ast::LiftedCompose(_, left, right)
        | Ast::KleisliCompose(_, left, right)
        | Ast::BinOp(_, _, left, right) => {
            collect_add_paths(left, project_file, declared_paths);
            collect_add_paths(right, project_file, declared_paths);
        }
        Ast::Grouped(_, inner)
        | Ast::Closure(_, _, inner)
        | Ast::Capture(_, inner, _)
        | Ast::Semi(_, inner)
        | Ast::FieldAccess(_, inner, _)
        | Ast::FacetSegmentAccess(_, inner, _)
        | Ast::FacetCapture(_, inner) => collect_add_paths(inner, project_file, declared_paths),
        Ast::Bind(_, _, expr) | Ast::SafeBind(_, _, expr) => {
            collect_add_paths(expr, project_file, declared_paths);
        }
        _ => {}
    }
}

fn add_path_literal(node: &Ast) -> Option<(&str, &Span)> {
    let Ast::App(_, callee, args) = node else {
        return None;
    };
    if !is_path(callee, &["Config", "add_path"]) {
        return None;
    }
    positional_args(args)
        .into_iter()
        .rev()
        .find_map(string_lit_with_span)
}

fn positional_args(args: &[RecordLitArg]) -> Vec<&Ast> {
    args.iter()
        .filter_map(|arg| match arg {
            RecordLitArg::Positional(ast) => Some(ast),
            RecordLitArg::Named(_, _) => None,
        })
        .collect()
}

fn string_lit(node: &Ast) -> Option<&str> {
    string_lit_with_span(node).map(|(value, _)| value)
}

fn string_lit_with_span(node: &Ast) -> Option<(&str, &Span)> {
    match node {
        Ast::Lit(span, Lit::Str(value)) => Some((value.as_str(), span)),
        _ => None,
    }
}

fn is_path(node: &Ast, expected: &[&str]) -> bool {
    match node {
        Ast::Path(_, path) => path
            .segments
            .iter()
            .map(String::as_str)
            .eq(expected.iter().copied()),
        _ => false,
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

fn normalized_path_value(path: &Path) -> String {
    let mut value = path_value(path);
    while value.contains("/./") {
        value = value.replace("/./", "/");
    }
    value
}

fn analysis_span(span: &Span) -> AnalysisSpan {
    AnalysisSpan {
        start: span.start.min(u32::MAX as usize) as u32,
        end: span.end.min(u32::MAX as usize) as u32,
    }
}
