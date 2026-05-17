use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use sindr::policy::{CompileUnitKind, SourceKind};
use sindr::runtime::{TypeEntry, TypeRegistry, Value};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRunnerResult {
    pub profiles: Vec<ProjectRunnerProfile>,
    pub boot_summary: ProjectBootSummary,
    pub external_inputs: Vec<ExternalInputState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRunnerProfile {
    pub name: String,
    pub entrypoint: String,
    pub paths: Vec<ProjectRunnerPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRunnerPath {
    pub declared_by: PathBuf,
    pub literal_or_glob: String,
    pub declaration_span: Option<AnalysisSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRunnerDecodeError {
    message: String,
}

impl ProjectRunnerDecodeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProjectRunnerDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProjectRunnerDecodeError {}

const DEFAULT_PROJECT_ENTRYPOINT: &str = "Main::main";

pub fn decode_project_runner_value(
    project_file: &Path,
    value: &Value,
    registry: &TypeRegistry,
) -> Result<ProjectRunnerResult, ProjectRunnerDecodeError> {
    let project_fields = tagged_fields(value, registry, "Project")?;
    let entries = list_field(&project_fields, "entries", "Project.entries")?;
    let profiles = entries
        .iter()
        .enumerate()
        .map(|(idx, value)| decode_config_value(project_file, &value, registry, idx))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ProjectRunnerResult {
        boot_summary: ProjectBootSummary {
            content_hash: None,
            fields: project_boot_summary_fields(&profiles),
        },
        profiles,
        external_inputs: Vec::new(),
    })
}

pub fn extract_project_runner_result(
    input: ProjectRunnerSourceInput,
) -> Result<ProjectRunnerResult, Vec<RunnerDiagnostic>> {
    let ast = parse_document(
        &input.source,
        0,
        SourceKind::ProjectConfigSource,
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
        profiles: Vec::new(),
    };
    extractor.visit_many(&ast);

    Ok(ProjectRunnerResult {
        boot_summary: ProjectBootSummary {
            content_hash: Some(stable_hash_text(&input.source)),
            fields: project_boot_summary_fields(&extractor.profiles),
        },
        profiles: extractor.profiles,
        external_inputs: Vec::new(),
    })
}

pub fn extract_project_runner_input(
    input: ProjectRunnerSourceInput,
) -> Result<ProjectRunnerInput, Vec<RunnerDiagnostic>> {
    let result = extract_project_runner_result(input.clone())?;

    if !result
        .profiles
        .iter()
        .any(|profile| profile.name == input.selected_profile)
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
        .map(|active_file| active_file_profiles(&result.profiles, active_file))
        .unwrap_or_else(|| {
            result
                .profiles
                .iter()
                .map(|profile| profile.name.clone())
                .collect()
        });

    let declared_paths = result
        .profiles
        .iter()
        .filter(|profile| profile.name == input.selected_profile)
        .flat_map(|profile| profile.paths.iter())
        .map(declared_project_path)
        .collect();

    Ok(ProjectRunnerInput {
        project_file: input.project_file,
        selected_profile: input.selected_profile,
        normalized_args: input.normalized_args,
        declared_paths,
        active_file_profiles,
        boot_summary: result.boot_summary,
        external_inputs: result.external_inputs,
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
    profiles: Vec<ProjectRunnerProfile>,
}

impl ProjectRunnerExtractor {
    fn visit_many(&mut self, ast: &[Ast]) {
        for node in ast {
            self.visit(node);
        }
    }

    fn visit(&mut self, node: &Ast) {
        if let Some((profile, builder)) = entrypoint_builder(node) {
            let mut facts = ConfigBuilderFacts::default();
            collect_config_builder_facts(builder, &self.project_file, &mut facts);

            let entrypoint = facts
                .entrypoint_updates
                .last()
                .cloned()
                .unwrap_or_else(|| DEFAULT_PROJECT_ENTRYPOINT.to_string());

            if let Some(existing_profile) = self
                .profiles
                .iter_mut()
                .find(|known| known.name.as_str() == profile)
            {
                existing_profile.entrypoint = entrypoint;
                existing_profile.paths.extend(facts.paths);
            } else {
                self.profiles.push(ProjectRunnerProfile {
                    name: profile.to_string(),
                    entrypoint,
                    paths: facts.paths,
                });
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

fn active_file_profiles(profiles: &[ProjectRunnerProfile], active_file: &Path) -> Vec<String> {
    let active_file = normalized_path_value(active_file);
    let mut matching_profiles = Vec::new();
    for profile in profiles {
        let mut ignored_diagnostics = Vec::new();
        let contains_active_file = profile.paths.iter().any(|path| {
            let declared_path = declared_project_path(path);
            if declared_path_matches_active_file(&declared_path, &active_file) {
                return true;
            }
            expand_declared_path(&declared_path, &mut ignored_diagnostics)
                .iter()
                .any(|path| normalized_path_value(path) == active_file)
        });
        if contains_active_file {
            matching_profiles.push(profile.name.clone());
        }
    }
    matching_profiles
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

fn declared_project_path(path: &ProjectRunnerPath) -> DeclaredProjectPath {
    DeclaredProjectPath {
        declared_by: path.declared_by.clone(),
        literal_or_glob: path.literal_or_glob.clone(),
        declaration_span: path.declaration_span,
    }
}

fn project_boot_summary_fields(profiles: &[ProjectRunnerProfile]) -> Vec<(String, String)> {
    profiles
        .iter()
        .map(|profile| {
            (
                format!("profile.{}.entrypoint", profile.name),
                profile.entrypoint.clone(),
            )
        })
        .collect()
}

struct TaggedFields<'a> {
    entry: &'a TypeEntry,
    fields: &'a [Value],
}

fn decode_config_value(
    project_file: &Path,
    value: &Value,
    registry: &TypeRegistry,
    index: usize,
) -> Result<ProjectRunnerProfile, ProjectRunnerDecodeError> {
    let config_fields = tagged_fields(value, registry, "Config").map_err(|error| {
        ProjectRunnerDecodeError::new(format!("Project.entries[{index}]: {}", error.message()))
    })?;
    let name = string_field(&config_fields, "name", "Config.name")?.to_string();
    let entrypoint = string_field(&config_fields, "entrypoint", "Config.entrypoint")?.to_string();
    let path_values = list_field(&config_fields, "paths", "Config.paths")?;
    let paths = path_values
        .iter()
        .enumerate()
        .map(|(path_idx, value)| {
            let Value::Str(path) = value else {
                return Err(ProjectRunnerDecodeError::new(format!(
                    "Config.paths[{path_idx}] must be String"
                )));
            };
            Ok(ProjectRunnerPath {
                declared_by: project_file.to_path_buf(),
                literal_or_glob: path.clone(),
                declaration_span: None,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ProjectRunnerProfile {
        name,
        entrypoint,
        paths,
    })
}

fn tagged_fields<'a>(
    value: &'a Value,
    registry: &'a TypeRegistry,
    expected_name: &str,
) -> Result<TaggedFields<'a>, ProjectRunnerDecodeError> {
    let Value::Tagged { tag, fields } = value else {
        return Err(ProjectRunnerDecodeError::new(format!(
            "expected {expected_name} tagged value"
        )));
    };
    let entry = registry.lookup(*tag).ok_or_else(|| {
        ProjectRunnerDecodeError::new(format!("unknown runtime tag {tag} for {expected_name}"))
    })?;
    let expected = registry.lookup_by_name(expected_name).ok_or_else(|| {
        ProjectRunnerDecodeError::new(format!("runtime type {expected_name} is not registered"))
    })?;
    if entry.tag != expected.tag {
        return Err(ProjectRunnerDecodeError::new(format!(
            "expected {expected_name}, got {}",
            entry.name
        )));
    }
    if fields.len() != entry.field_names.len() {
        return Err(ProjectRunnerDecodeError::new(format!(
            "{} field count mismatch: expected {}, got {}",
            expected_name,
            entry.field_names.len(),
            fields.len()
        )));
    }
    Ok(TaggedFields { entry, fields })
}

fn field<'a>(
    fields: &'a TaggedFields<'a>,
    name: &str,
    display_name: &str,
) -> Result<&'a Value, ProjectRunnerDecodeError> {
    let idx = fields
        .entry
        .field_names
        .iter()
        .position(|field_name| field_name == name)
        .ok_or_else(|| ProjectRunnerDecodeError::new(format!("{display_name} field is missing")))?;
    fields
        .fields
        .get(idx)
        .ok_or_else(|| ProjectRunnerDecodeError::new(format!("{display_name} field is missing")))
}

fn string_field<'a>(
    fields: &'a TaggedFields<'a>,
    name: &str,
    display_name: &str,
) -> Result<&'a str, ProjectRunnerDecodeError> {
    match field(fields, name, display_name)? {
        Value::Str(value) => Ok(value),
        _ => Err(ProjectRunnerDecodeError::new(format!(
            "{display_name} must be String"
        ))),
    }
}

fn list_field(
    fields: &TaggedFields<'_>,
    name: &str,
    display_name: &str,
) -> Result<Vec<Value>, ProjectRunnerDecodeError> {
    match field(fields, name, display_name)? {
        Value::List(list) => Ok(list.iter().collect()),
        _ => Err(ProjectRunnerDecodeError::new(format!(
            "{display_name} must be List"
        ))),
    }
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

#[derive(Default)]
struct ConfigBuilderFacts {
    entrypoint_updates: Vec<String>,
    paths: Vec<ProjectRunnerPath>,
}

fn collect_config_builder_facts(node: &Ast, project_file: &Path, facts: &mut ConfigBuilderFacts) {
    if let Some((entrypoint, _span)) = entry_fun_literal(node) {
        facts.entrypoint_updates.push(entrypoint.to_string());
    }
    if let Some((literal_or_glob, span)) = add_path_literal(node) {
        facts.paths.push(ProjectRunnerPath {
            declared_by: project_file.to_path_buf(),
            literal_or_glob: literal_or_glob.to_string(),
            declaration_span: Some(analysis_span(span)),
        });
    }

    match node {
        Ast::App(_, callee, args) => {
            collect_config_builder_facts(callee, project_file, facts);
            for arg in positional_args(args) {
                collect_config_builder_facts(arg, project_file, facts);
            }
        }
        Ast::Block(_, nodes) => {
            for node in nodes {
                collect_config_builder_facts(node, project_file, facts);
            }
        }
        Ast::Pipe(_, left, right)
        | Ast::ContextMap(_, left, right)
        | Ast::ContextBind(_, left, right)
        | Ast::Compose(_, left, right)
        | Ast::LiftedCompose(_, left, right)
        | Ast::KleisliCompose(_, left, right)
        | Ast::BinOp(_, _, left, right) => {
            collect_config_builder_facts(left, project_file, facts);
            collect_config_builder_facts(right, project_file, facts);
        }
        Ast::Grouped(_, inner)
        | Ast::Closure(_, _, inner)
        | Ast::Capture(_, inner, _)
        | Ast::Semi(_, inner)
        | Ast::FieldAccess(_, inner, _)
        | Ast::FacetSegmentAccess(_, inner, _)
        | Ast::FacetCapture(_, inner) => collect_config_builder_facts(inner, project_file, facts),
        Ast::Bind(_, _, expr) | Ast::SafeBind(_, _, expr) => {
            collect_config_builder_facts(expr, project_file, facts);
        }
        _ => {}
    }
}

fn entry_fun_literal(node: &Ast) -> Option<(&str, &Span)> {
    let Ast::App(_, callee, args) = node else {
        return None;
    };
    if !is_path(callee, &["Config", "entry_fun"]) {
        return None;
    }
    positional_args(args)
        .into_iter()
        .rev()
        .find_map(string_lit_with_span)
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

    let matches = match expand_wildcard_path(base_dir, &declared_path.literal_or_glob) {
        Ok(matches) => matches,
        Err(unreadable_path) => {
            diagnostics.push(RunnerDiagnostic {
                kind: RunnerDiagnosticKind::UnreadablePath,
                path: Some(unreadable_path.clone()),
                span: declared_path.declaration_span,
                message: format!(
                    "project glob directory {} is not readable",
                    path_value(&unreadable_path)
                ),
            });
            return Vec::new();
        }
    };

    if matches.is_empty() {
        diagnostics.push(RunnerDiagnostic {
            kind: RunnerDiagnosticKind::GlobNoMatch,
            path: Some(path.clone()),
            span: declared_path.declaration_span,
            message: format!(
                "project glob {} did not match any files",
                declared_path.literal_or_glob
            ),
        });
    }

    matches
}

fn expand_wildcard_path(base_dir: &Path, pattern: &str) -> Result<Vec<PathBuf>, PathBuf> {
    let normalized_pattern = pattern.replace('\\', "/");
    let segments = normalized_pattern
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>();

    let mut matches = Vec::new();
    expand_wildcard_segments(base_dir.to_path_buf(), &segments, &mut matches)?;
    matches.sort_by_key(|path| path_value(path));
    matches.dedup();
    Ok(matches)
}

fn expand_wildcard_segments(
    current: PathBuf,
    segments: &[&str],
    matches: &mut Vec<PathBuf>,
) -> Result<(), PathBuf> {
    let Some((segment, remaining)) = segments.split_first() else {
        if current.is_file() {
            matches.push(current);
        }
        return Ok(());
    };

    if *segment == "**" {
        expand_wildcard_segments(current.clone(), remaining, matches)?;
        for child in sorted_directory_entries(&current)? {
            if child.is_dir() {
                expand_wildcard_segments(child, segments, matches)?;
            }
        }
        return Ok(());
    }

    if has_wildcard(segment) {
        for child in sorted_directory_entries(&current)? {
            let child_name_matches = child
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| wildcard_match(segment, name));
            if !child_name_matches {
                continue;
            }
            if remaining.is_empty() {
                if child.is_file() {
                    matches.push(child);
                }
            } else if child.is_dir() {
                expand_wildcard_segments(child, remaining, matches)?;
            }
        }
        return Ok(());
    }

    expand_wildcard_segments(current.join(segment), remaining, matches)
}

fn sorted_directory_entries(path: &Path) -> Result<Vec<PathBuf>, PathBuf> {
    let entries = fs::read_dir(path).map_err(|_| path.to_path_buf())?;
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| path_value(path));
    Ok(paths)
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
