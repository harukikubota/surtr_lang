use std::path::PathBuf;

use sindr::policy::{CompileUnitKind, SourceKind};

use crate::project_runner::{
    extract_project_runner_input, resolve_project_runner, ProjectRunnerInput,
    ProjectRunnerSourceInput,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisMode {
    Script,
    DefinitionCheck,
    Project,
    ReplPreview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisSourceKind(pub SourceKind);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisContext {
    pub workspace_root: PathBuf,
    pub mode: AnalysisMode,
    pub entry_file: Option<PathBuf>,
    pub active_file: PathBuf,
    pub source_kind: SourceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisSpan {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectedContext {
    ScriptEntry(PathBuf),
    ProjectProfile {
        project_file: PathBuf,
        profile: String,
    },
    DefinitionStandalone,
    DefinitionUnderEntry {
        entry_file: PathBuf,
    },
    StdlibDevelopment,
    ReplPreview {
        session_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerSelection {
    pub project_file: PathBuf,
    pub selected_profile: String,
    pub normalized_args: Vec<(String, String)>,
    pub source: Option<ProjectRunnerSourceInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProjectPath {
    pub declared_by: PathBuf,
    pub literal_or_glob: String,
    pub declaration_span: Option<AnalysisSpan>,
    pub expanded_files: Vec<PathBuf>,
    pub source_kind: SourceKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectBootSummary {
    pub content_hash: Option<String>,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalInputStatus {
    Available,
    Missing,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalInputState {
    pub name: String,
    pub content_hash: Option<String>,
    pub status: ExternalInputStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleStage {
    pub files: Vec<ModuleFileFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerDiagnosticKind {
    MissingRunnerSelection,
    ProjectFileMismatch,
    ProjectProfileMismatch,
    ProjectProfileUnknown,
    ProjectSourceParseError,
    GlobNoMatch,
    UnreadablePath,
    LoadProjectUnsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerDiagnostic {
    pub kind: RunnerDiagnosticKind,
    pub path: Option<PathBuf>,
    pub span: Option<AnalysisSpan>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptProjectContext {
    pub directive_span: Option<AnalysisSpan>,
    pub project_file: Option<PathBuf>,
    pub profile: Option<String>,
    pub diagnostics: Vec<RunnerDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplAnalysisContext {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerContext {
    pub project_file: PathBuf,
    pub selected_profile: String,
    pub normalized_args: Vec<(String, String)>,
    pub resolved_paths: Vec<ResolvedProjectPath>,
    pub active_file_profiles: Vec<String>,
    pub module_stages: Vec<ModuleStage>,
    pub boot_summary: ProjectBootSummary,
    pub external_inputs: Vec<ExternalInputState>,
    pub diagnostics: Vec<RunnerDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalysisContextStatus {
    Ready,
    NeedsSelection,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextDiagnosticKind {
    NeedsContextSelection,
    MissingRunnerSelection,
    ProjectFileMismatch,
    ProjectProfileMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextDiagnostic {
    pub kind: ContextDiagnosticKind,
    pub path: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAnalysisContext {
    pub context: AnalysisContext,
    pub status: AnalysisContextStatus,
    pub runner: Option<RunnerContext>,
    pub script_project: Option<ScriptProjectContext>,
    pub repl: Option<ReplAnalysisContext>,
    pub diagnostics: Vec<ContextDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentVersion {
    pub path: PathBuf,
    pub version: Option<i64>,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisContextRequest {
    pub workspace_root: PathBuf,
    pub active_file: PathBuf,
    pub selected_context: Option<SelectedContext>,
    pub runner_selection: Option<RunnerSelection>,
    pub open_documents: Vec<DocumentVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleFileFingerprint {
    pub path: PathBuf,
    pub source_kind: SourceKind,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisCacheInput {
    pub context: AnalysisContext,
    pub active_document_hash: String,
    pub stdlib_hash: String,
    pub include_graph_hash: Option<String>,
    pub module_stages: Vec<Vec<ModuleFileFingerprint>>,
    pub runner_selection: Option<RunnerSelection>,
    pub project_runner_hash: Option<String>,
    pub project_path_hashes: Vec<(String, String)>,
    pub boot_summary_hash: Option<String>,
    pub external_inputs: Vec<(String, String)>,
    pub load_project_hash: Option<String>,
    pub active_file_profiles: Vec<String>,
}

pub fn resolve_context(request: AnalysisContextRequest) -> ResolvedAnalysisContext {
    let selected_context = request
        .selected_context
        .clone()
        .unwrap_or_else(|| auto_selected_context(&request));

    match selected_context {
        SelectedContext::ScriptEntry(entry_file) => {
            let source_kind = if same_path(&request.active_file, &entry_file) {
                SourceKind::Script
            } else {
                SourceKind::DefinitionSource
            };
            let mut resolved = ready_context(
                request.workspace_root,
                AnalysisMode::Script,
                Some(entry_file.clone()),
                request.active_file,
                source_kind,
            );
            if let Some(selection) = request.runner_selection {
                let project_file = selection.project_file.clone();
                let profile = selection.selected_profile.clone();
                let runner = if let Some(source_input) = selection.source.clone() {
                    match extract_project_runner_input(source_input) {
                        Ok(input) => resolve_project_runner(input),
                        Err(source_diagnostics) => {
                            let mut runner = empty_runner_context(project_file.clone(), selection);
                            runner.diagnostics = source_diagnostics;
                            runner
                        }
                    }
                } else {
                    empty_runner_context(project_file.clone(), selection)
                };
                let diagnostics = runner.diagnostics.clone();
                resolved.runner = Some(runner);
                resolved.script_project = Some(ScriptProjectContext {
                    directive_span: None,
                    project_file: Some(project_file),
                    profile: Some(profile),
                    diagnostics,
                });
            }
            resolved
        }
        SelectedContext::ProjectProfile {
            project_file,
            profile,
        } => resolve_project_context(request, project_file, profile),
        SelectedContext::DefinitionStandalone => ready_context(
            request.workspace_root,
            AnalysisMode::DefinitionCheck,
            None,
            request.active_file,
            SourceKind::DefinitionSource,
        ),
        SelectedContext::DefinitionUnderEntry { entry_file } => {
            let source_kind = if same_path(&request.active_file, &entry_file) {
                SourceKind::Script
            } else {
                SourceKind::DefinitionSource
            };
            ready_context(
                request.workspace_root,
                AnalysisMode::Script,
                Some(entry_file),
                request.active_file,
                source_kind,
            )
        }
        SelectedContext::StdlibDevelopment => ready_context(
            request.workspace_root,
            AnalysisMode::DefinitionCheck,
            None,
            request.active_file,
            SourceKind::StdDefinitionSource,
        ),
        SelectedContext::ReplPreview { session_id } => {
            let mut resolved = ready_context(
                request.workspace_root,
                AnalysisMode::ReplPreview,
                None,
                request.active_file,
                SourceKind::ReplChunk,
            );
            resolved.repl = Some(ReplAnalysisContext { session_id });
            resolved
        }
    }
}

fn resolve_project_context(
    request: AnalysisContextRequest,
    project_file: PathBuf,
    profile: String,
) -> ResolvedAnalysisContext {
    let active_file = request.active_file.clone();
    let source_kind = if same_path(&active_file, &project_file) {
        SourceKind::ProjectConfigSource
    } else {
        SourceKind::DefinitionSource
    };
    let base_context = AnalysisContext {
        workspace_root: request.workspace_root,
        mode: AnalysisMode::Project,
        entry_file: Some(project_file.clone()),
        active_file: request.active_file,
        source_kind,
    };

    let Some(selection) = request.runner_selection else {
        return ResolvedAnalysisContext {
            context: base_context,
            status: AnalysisContextStatus::NeedsSelection,
            runner: None,
            script_project: None,
            repl: None,
            diagnostics: vec![ContextDiagnostic {
                kind: ContextDiagnosticKind::MissingRunnerSelection,
                path: Some(project_file),
                message: "project context requires normalized runner selection".to_string(),
            }],
        };
    };

    let mut diagnostics = Vec::new();
    let mut runner_diagnostics = Vec::new();

    if !same_path(&selection.project_file, &project_file) {
        diagnostics.push(ContextDiagnostic {
            kind: ContextDiagnosticKind::ProjectFileMismatch,
            path: Some(selection.project_file.clone()),
            message: format!(
                "selected project file {} does not match runner project file {}",
                path_value(&project_file),
                path_value(&selection.project_file)
            ),
        });
        runner_diagnostics.push(RunnerDiagnostic {
            kind: RunnerDiagnosticKind::ProjectFileMismatch,
            path: Some(selection.project_file.clone()),
            span: None,
            message: "runner selection belongs to a different project file".to_string(),
        });
    }

    if selection.selected_profile != profile {
        diagnostics.push(ContextDiagnostic {
            kind: ContextDiagnosticKind::ProjectProfileMismatch,
            path: Some(project_file.clone()),
            message: format!(
                "selected profile {profile} does not match runner profile {}",
                selection.selected_profile
            ),
        });
        runner_diagnostics.push(RunnerDiagnostic {
            kind: RunnerDiagnosticKind::ProjectProfileMismatch,
            path: Some(project_file.clone()),
            span: None,
            message: "runner selection belongs to a different profile".to_string(),
        });
    }

    let mut runner = if let Some(mut source_input) = selection.source.clone() {
        if source_input.active_file.is_none() {
            source_input.active_file = Some(active_file.clone());
        }
        match extract_project_runner_input(source_input) {
            Ok(input) => resolve_project_runner(input),
            Err(source_diagnostics) => {
                let mut runner = empty_runner_context(project_file.clone(), selection.clone());
                runner.diagnostics = source_diagnostics;
                runner
            }
        }
    } else {
        empty_runner_context(project_file.clone(), selection.clone())
    };
    runner.diagnostics.append(&mut runner_diagnostics);

    let status = if diagnostics.is_empty() {
        AnalysisContextStatus::Ready
    } else {
        AnalysisContextStatus::NeedsSelection
    };

    ResolvedAnalysisContext {
        context: base_context,
        status,
        runner: Some(runner),
        script_project: None,
        repl: None,
        diagnostics,
    }
}

fn empty_runner_context(project_file: PathBuf, selection: RunnerSelection) -> RunnerContext {
    resolve_project_runner(ProjectRunnerInput {
        project_file,
        selected_profile: selection.selected_profile,
        normalized_args: selection.normalized_args,
        declared_paths: Vec::new(),
        active_file_profiles: Vec::new(),
        boot_summary: ProjectBootSummary::default(),
        external_inputs: Vec::new(),
    })
}

fn ready_context(
    workspace_root: PathBuf,
    mode: AnalysisMode,
    entry_file: Option<PathBuf>,
    active_file: PathBuf,
    source_kind: SourceKind,
) -> ResolvedAnalysisContext {
    ResolvedAnalysisContext {
        context: AnalysisContext {
            workspace_root,
            mode,
            entry_file,
            active_file,
            source_kind,
        },
        status: AnalysisContextStatus::Ready,
        runner: None,
        script_project: None,
        repl: None,
        diagnostics: Vec::new(),
    }
}

fn auto_selected_context(request: &AnalysisContextRequest) -> SelectedContext {
    if is_stdlib_source(&request.workspace_root, &request.active_file) {
        return SelectedContext::StdlibDevelopment;
    }
    if is_script_fixture(&request.workspace_root, &request.active_file)
        || is_module_fixture_entry(&request.workspace_root, &request.active_file)
    {
        return SelectedContext::ScriptEntry(request.active_file.clone());
    }
    SelectedContext::DefinitionStandalone
}

fn is_stdlib_source(workspace_root: &std::path::Path, active_file: &std::path::Path) -> bool {
    let Ok(relative) = active_file.strip_prefix(workspace_root) else {
        return false;
    };
    let path = path_value(relative);
    path.starts_with("lib/") && path.ends_with(".srt") && !path.starts_with("lib/tests/")
}

fn is_script_fixture(workspace_root: &std::path::Path, active_file: &std::path::Path) -> bool {
    let Ok(relative) = active_file.strip_prefix(workspace_root) else {
        return false;
    };
    let path = path_value(relative);
    (path.starts_with("tests/fixtures/script/pass/")
        || path.starts_with("tests/fixtures/script/fail/"))
        && path.ends_with(".srt")
}

fn is_module_fixture_entry(
    workspace_root: &std::path::Path,
    active_file: &std::path::Path,
) -> bool {
    let Ok(relative) = active_file.strip_prefix(workspace_root) else {
        return false;
    };
    let path = path_value(relative);
    (path.starts_with("tests/fixtures/modules/pass/")
        || path.starts_with("tests/fixtures/modules/fail/"))
        && path.ends_with("/entry.srt")
}

fn same_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    path_value(left) == path_value(right)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheKeyField {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisCacheKey {
    fields: Vec<CacheKeyField>,
}

impl AnalysisCacheKey {
    pub fn new(input: AnalysisCacheInput) -> Self {
        let mut fields = Vec::new();
        push_field(
            &mut fields,
            "workspace_root",
            path_value(&input.context.workspace_root),
        );
        push_field(&mut fields, "mode", format!("{:?}", input.context.mode));
        push_field(
            &mut fields,
            "entry_file",
            input
                .context
                .entry_file
                .as_ref()
                .map(|path| path_value(path))
                .unwrap_or_default(),
        );
        push_field(
            &mut fields,
            "active_file",
            path_value(&input.context.active_file),
        );
        push_field(
            &mut fields,
            "source_kind",
            format!("{:?}", input.context.source_kind),
        );
        push_field(
            &mut fields,
            "active_document_hash",
            input.active_document_hash,
        );
        push_field(&mut fields, "stdlib_hash", input.stdlib_hash);
        if let Some(hash) = input.include_graph_hash {
            push_field(&mut fields, "include_graph_hash", hash);
        }

        for (stage_idx, stage) in input.module_stages.iter().enumerate() {
            for (module_idx, module) in stage.iter().enumerate() {
                push_field(
                    &mut fields,
                    format!("module_stage.{stage_idx}.{module_idx}.path"),
                    path_value(&module.path),
                );
                push_field(
                    &mut fields,
                    format!("module_stage.{stage_idx}.{module_idx}.source_kind"),
                    format!("{:?}", module.source_kind),
                );
                push_field(
                    &mut fields,
                    format!("module_stage.{stage_idx}.{module_idx}.content_hash"),
                    module.content_hash.clone(),
                );
            }
        }

        if let Some(selection) = input.runner_selection {
            push_field(
                &mut fields,
                "project_file",
                path_value(&selection.project_file),
            );
            push_field(&mut fields, "selected_profile", selection.selected_profile);
            push_sorted_pairs(&mut fields, "runner_arg", selection.normalized_args);
        }
        if let Some(hash) = input.project_runner_hash {
            push_field(&mut fields, "project_runner_hash", hash);
        }
        push_sorted_pairs(&mut fields, "project_path", input.project_path_hashes);
        if let Some(hash) = input.boot_summary_hash {
            push_field(&mut fields, "boot_summary_hash", hash);
        }
        push_sorted_pairs(&mut fields, "external_input", input.external_inputs);
        if let Some(hash) = input.load_project_hash {
            push_field(&mut fields, "load_project_hash", hash);
        }
        let mut profiles = input.active_file_profiles;
        profiles.sort();
        for profile in profiles {
            push_field(&mut fields, "active_file_profile", profile);
        }

        Self { fields }
    }

    pub fn fields(&self) -> &[CacheKeyField] {
        &self.fields
    }
}

fn push_field(fields: &mut Vec<CacheKeyField>, name: impl Into<String>, value: impl Into<String>) {
    fields.push(CacheKeyField {
        name: name.into(),
        value: value.into(),
    });
}

fn push_sorted_pairs(
    fields: &mut Vec<CacheKeyField>,
    prefix: &str,
    mut pairs: Vec<(String, String)>,
) {
    pairs.sort();
    for (key, value) in pairs {
        push_field(fields, format!("{prefix}.{key}"), value);
    }
}

fn path_value(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn parse_document(
    source: &str,
    source_id: u32,
    source_kind: SourceKind,
    compile_unit_kind: CompileUnitKind,
    module_path: Option<String>,
) -> Result<Vec<spire::ast::Ast>, spire::error::ParseError> {
    let context = match (compile_unit_kind, source_kind, module_path) {
        (CompileUnitKind::Project, SourceKind::ProjectConfigSource, None) => {
            spire::ParserContext::project(source_id)
        }
        (_, SourceKind::Script, _) => spire::ParserContext::script(source_id),
        (_, SourceKind::ReplChunk, _) => spire::ParserContext::repl(source_id),
        (
            _,
            SourceKind::DefinitionSource
            | SourceKind::StdDefinitionSource
            | SourceKind::ProjectConfigSource,
            module_path,
        ) => spire::ParserContext::module(source_id, module_path)
            .with_rules(spire::parse_rules_for_source_kind(source_kind)),
    };

    spire::parse_with_context(source, context)
}
