use std::path::PathBuf;

use sindr::policy::{CompileUnitKind, SourceKind};

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
        (CompileUnitKind::Project, SourceKind::DefinitionSource, None) => {
            spire::ParserContext::project(source_id)
        }
        (_, SourceKind::Script, _) => spire::ParserContext::script(source_id),
        (_, SourceKind::ReplChunk, _) => spire::ParserContext::repl(source_id),
        (_, SourceKind::DefinitionSource | SourceKind::StdDefinitionSource, module_path) => {
            spire::ParserContext::module(source_id, module_path)
                .with_rules(spire::parse_rules_for_source_kind(source_kind))
        }
    };

    spire::parse_with_context(source, context)
}
