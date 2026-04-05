use std::collections::HashMap;
use std::fs;

use diagnostics::{SourceId, SourceRegistry};

const BUILTIN_PRELUDE_FILE: &str = "bootstrap.srt";
const BUILTIN_PRELUDE_MODULE_PATH: &str = "Bootstrap";
const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/bootstrap.srt");
const KERNEL_PRELUDE_FILE: &str = "kernel.srt";
const KERNEL_PRELUDE_MODULE_PATH: &str = "Kernel";
const KERNEL_PRELUDE_SOURCE: &str = include_str!("../../../lib/kernel.srt");
const REPL_MODULE_NAME: &str = "REPL";
const SCRIPT_PSEUDO_MODULE_PREFIX: &str = "__Script";
const REPL_PSEUDO_MODULE_PATH: &str = "__Repl::Session";

/// Logical source categories that drive parser/typechecker policy selection.
///
/// The loader always materializes standard sources in the fixed order
/// `Bootstrap -> Kernel -> [other standard modules] -> user source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Script,
    Module,
    StdModule,
    ReplChunk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDescriptor {
    pub file_name: String,
    pub source: String,
    pub kind: SourceKind,
    pub module_path: Option<String>,
}

impl SourceDescriptor {
    pub fn script(file_name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            source: source.into(),
            kind: SourceKind::Script,
            module_path: None,
        }
    }

    pub fn module(
        file_name: impl Into<String>,
        source: impl Into<String>,
        module_path: impl Into<String>,
    ) -> Self {
        Self {
            file_name: file_name.into(),
            source: source.into(),
            kind: SourceKind::Module,
            module_path: Some(module_path.into()),
        }
    }

    pub fn std_module(
        file_name: impl Into<String>,
        source: impl Into<String>,
        module_path: impl Into<String>,
    ) -> Self {
        Self {
            file_name: file_name.into(),
            source: source.into(),
            kind: SourceKind::StdModule,
            module_path: Some(module_path.into()),
        }
    }

    pub fn repl_chunk(file_name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            source: source.into(),
            kind: SourceKind::ReplChunk,
            module_path: None,
        }
    }
}

fn sanitize_module_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for ch in segment.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let collapsed = out.trim_matches('_');
    if collapsed.is_empty() {
        "_".to_string()
    } else {
        collapsed.to_string()
    }
}

pub fn script_pseudo_module_path(file_name: &str) -> String {
    let normalized = file_name.replace('\\', "/");
    let mut body = normalized.trim().trim_start_matches("./").to_string();
    if let Some(stripped) = body.strip_suffix(".srt") {
        body = stripped.to_string();
    }
    let mut segments = body
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(sanitize_module_segment)
        .collect::<Vec<_>>();
    if segments.is_empty() {
        segments.push("Main".to_string());
    }
    format!("{}::{}", SCRIPT_PSEUDO_MODULE_PREFIX, segments.join("::"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceBinding {
    source_id: SourceId,
    kind: SourceKind,
    module_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    ConflictingSource {
        file_name: String,
    },
    DuplicateModulePath {
        module_path: String,
        first_file_name: String,
        second_file_name: String,
    },
    SourceReadFailed {
        file_name: String,
        message: String,
    },
    EmptyModulePath {
        file_name: String,
    },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConflictingSource { file_name } => {
                write!(f, "conflicting source registration for `{}`", file_name)
            }
            Self::DuplicateModulePath {
                module_path,
                first_file_name,
                second_file_name,
            } => write!(
                f,
                "duplicate module path `{}` in `{}` and `{}`",
                module_path, first_file_name, second_file_name
            ),
            Self::SourceReadFailed { file_name, message } => {
                write!(f, "failed to read `{}`: {}", file_name, message)
            }
            Self::EmptyModulePath { file_name } => {
                write!(f, "empty module path derived from `{}`", file_name)
            }
        }
    }
}

impl std::error::Error for LoadError {}

#[derive(Debug, Clone)]
struct CollectedSources {
    sources: SourceRegistry,
    bindings: Vec<SourceBinding>,
}

fn collect_sources(specs: &[SourceDescriptor]) -> Result<CollectedSources, LoadError> {
    let mut sources = SourceRegistry::new();
    let mut bindings = Vec::with_capacity(specs.len());
    let mut by_file: HashMap<String, (SourceId, String, SourceKind, Option<String>)> =
        HashMap::new();

    for spec in specs {
        if let Some((source_id, existing_source, existing_kind, existing_module)) =
            by_file.get(&spec.file_name)
        {
            if existing_source == &spec.source
                && existing_kind == &spec.kind
                && existing_module == &spec.module_path
            {
                bindings.push(SourceBinding {
                    source_id: *source_id,
                    kind: spec.kind,
                    module_path: spec.module_path.clone(),
                });
                continue;
            }

            return Err(LoadError::ConflictingSource {
                file_name: spec.file_name.clone(),
            });
        }

        let source_id = sources.register(spec.file_name.clone(), spec.source.clone());
        by_file.insert(
            spec.file_name.clone(),
            (
                source_id,
                spec.source.clone(),
                spec.kind,
                spec.module_path.clone(),
            ),
        );

        bindings.push(SourceBinding {
            source_id,
            kind: spec.kind,
            module_path: spec.module_path.clone(),
        });
    }

    Ok(CollectedSources { sources, bindings })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInput {
    pub file_name: String,
    pub source: String,
    pub module_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedModule {
    pub source_id: SourceId,
    pub module_path: String,
    pub source_kind: SourceKind,
}

#[derive(Debug, Clone)]
pub struct ModuleSources {
    pub sources: SourceRegistry,
    pub builtin_source_id: SourceId,
    pub builtin_module_path: Option<String>,
    pub module_source_ids: Vec<SourceId>,
    pub module_stages: Vec<Vec<StagedModule>>,
}

#[derive(Debug, Clone)]
pub struct CompileSources {
    pub sources: SourceRegistry,
    pub user_source_id: SourceId,
    pub user_module_path: String,
    pub builtin_source_id: SourceId,
    pub builtin_module_path: Option<String>,
    pub module_source_ids: Vec<SourceId>,
    pub module_stages: Vec<Vec<StagedModule>>,
}

pub fn collect_module_sources_with_modules(
    module_inputs: &[ModuleInput],
) -> Result<ModuleSources, LoadError> {
    if module_inputs.is_empty() {
        return collect_module_sources_with_module_stages(&[]);
    }
    collect_module_sources_with_module_stages(&[module_inputs.to_vec()])
}

pub fn collect_module_sources_with_module_stages(
    module_input_stages: &[Vec<ModuleInput>],
) -> Result<ModuleSources, LoadError> {
    // Stage 0/1 are reserved for the built-in standard layers. User-provided
    // modules are appended afterwards so they can depend on Bootstrap/Kernel
    // but never precede them.
    let mut stage_specs = vec![
        vec![SourceDescriptor::std_module(
            BUILTIN_PRELUDE_FILE,
            BUILTIN_PRELUDE_SOURCE,
            BUILTIN_PRELUDE_MODULE_PATH,
        )],
        vec![SourceDescriptor::std_module(
            KERNEL_PRELUDE_FILE,
            KERNEL_PRELUDE_SOURCE,
            KERNEL_PRELUDE_MODULE_PATH,
        )],
    ];

    for stage in module_input_stages {
        let mut specs = Vec::with_capacity(stage.len());
        for module in stage {
            specs.push(SourceDescriptor::module(
                module.file_name.clone(),
                module.source.clone(),
                module.module_path.clone(),
            ));
        }
        stage_specs.push(specs);
    }

    let mut flattened_specs = Vec::new();
    for stage in &stage_specs {
        for spec in stage {
            flattened_specs.push(spec.clone());
        }
    }

    let collected = collect_sources(&flattened_specs)?;

    let mut idx = 0;
    let mut module_stages = Vec::with_capacity(stage_specs.len());
    for stage in &stage_specs {
        let mut stage_bindings = Vec::with_capacity(stage.len());
        for _ in stage {
            let binding = &collected.bindings[idx];
            idx += 1;
            stage_bindings.push(StagedModule {
                source_id: binding.source_id,
                module_path: binding.module_path.clone().unwrap_or_default(),
                source_kind: binding.kind,
            });
        }
        module_stages.push(stage_bindings);
    }

    let builtin = module_stages
        .first()
        .and_then(|stage| stage.first())
        .ok_or_else(|| LoadError::ConflictingSource {
            file_name: BUILTIN_PRELUDE_FILE.into(),
        })?;
    let module_source_ids = module_stages
        .iter()
        .flat_map(|stage| stage.iter().map(|entry| entry.source_id))
        .collect();

    Ok(ModuleSources {
        sources: collected.sources,
        builtin_source_id: builtin.source_id,
        builtin_module_path: Some(builtin.module_path.clone()),
        module_source_ids,
        module_stages,
    })
}

pub fn compose_script_compile_sources(
    user_file_name: &str,
    user_source: &str,
    mut module_sources: ModuleSources,
) -> CompileSources {
    let user_source_id = module_sources.sources.register(user_file_name, user_source);
    CompileSources {
        sources: module_sources.sources,
        user_source_id,
        user_module_path: script_pseudo_module_path(user_file_name),
        builtin_source_id: module_sources.builtin_source_id,
        builtin_module_path: module_sources.builtin_module_path,
        module_source_ids: module_sources.module_source_ids,
        module_stages: module_sources.module_stages,
    }
}

pub fn collect_module_sources_with_module_file_stages(
    module_file_stages: &[Vec<String>],
) -> Result<ModuleSources, LoadError> {
    let mut module_input_stages = Vec::with_capacity(module_file_stages.len());
    for stage in module_file_stages {
        let mut stage_inputs = Vec::with_capacity(stage.len());
        for file_name in stage {
            let source =
                fs::read_to_string(file_name).map_err(|e| LoadError::SourceReadFailed {
                    file_name: file_name.clone(),
                    message: e.to_string(),
                })?;
            let module_path = module_path_from_file_name(file_name).ok_or_else(|| {
                LoadError::EmptyModulePath {
                    file_name: file_name.clone(),
                }
            })?;
            stage_inputs.push(ModuleInput {
                file_name: file_name.clone(),
                source,
                module_path,
            });
        }
        module_input_stages.push(stage_inputs);
    }

    collect_module_sources_with_module_stages(&module_input_stages)
}

fn module_path_from_file_name(file_name: &str) -> Option<String> {
    let normalized = file_name.replace('\\', "/");
    let mut body = normalized.trim().trim_start_matches("./").to_string();
    if let Some(stripped) = body.strip_suffix(".srt") {
        body = stripped.to_string();
    }

    let segments = body
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return None;
    }

    Some(segments.join("::"))
}

#[derive(Debug, Clone)]
pub(crate) struct ReplSources {
    pub(crate) sources: SourceRegistry,
    pub(crate) builtin_source_id: SourceId,
    pub(crate) module_stages: Vec<Vec<StagedModule>>,
    pub(crate) repl_source_id: SourceId,
    pub(crate) repl_module_path: String,
}

pub(crate) fn collect_repl_sources() -> Result<ReplSources, LoadError> {
    let mut module_sources = collect_module_sources_with_module_stages(&[])?;
    let repl_source_id = module_sources.sources.register(REPL_MODULE_NAME, "");

    Ok(ReplSources {
        sources: module_sources.sources,
        builtin_source_id: module_sources.builtin_source_id,
        module_stages: module_sources.module_stages,
        repl_source_id,
        repl_module_path: REPL_PSEUDO_MODULE_PATH.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_source_is_registered_once() {
        let specs = vec![
            SourceDescriptor::module("a.srt", "defmod A {}", "A"),
            SourceDescriptor::module("a.srt", "defmod A {}", "A"),
        ];

        let collected = collect_sources(&specs).expect("loader should deduplicate same source");
        assert_eq!(
            collected.bindings[0].source_id,
            collected.bindings[1].source_id
        );
        assert_eq!(
            collected.sources.file_name(collected.bindings[0].source_id),
            Some("a.srt")
        );
    }

    #[test]
    fn duplicate_module_path_is_not_rejected_during_source_registration() {
        let specs = vec![
            SourceDescriptor::module("a.srt", "defmod A {}", "Std::Math"),
            SourceDescriptor::module("b.srt", "defmod B {}", "Std::Math"),
        ];

        let collected =
            collect_sources(&specs).expect("module-path validation is handled after defmod lower");
        assert_eq!(collected.bindings.len(), 2);
    }

    #[test]
    fn compile_sources_register_user_and_builtin() {
        let module_sources =
            collect_module_sources_with_module_stages(&[]).expect("module collection must succeed");
        let loaded = compose_script_compile_sources("main.srt", "print(\"hi\")", module_sources);

        assert_eq!(
            loaded.sources.file_name(loaded.user_source_id),
            Some("main.srt")
        );
        assert_eq!(
            loaded.sources.file_name(loaded.builtin_source_id),
            Some(BUILTIN_PRELUDE_FILE)
        );
        assert_eq!(
            loaded.builtin_module_path.as_deref(),
            Some(BUILTIN_PRELUDE_MODULE_PATH)
        );
        assert_eq!(loaded.module_source_ids.len(), 2);
        assert_eq!(loaded.module_source_ids[0], loaded.builtin_source_id);
        assert_eq!(loaded.module_stages.len(), 2);
        assert_eq!(loaded.module_stages[0][0].module_path, "Bootstrap");
        assert_eq!(loaded.module_stages[1][0].module_path, "Kernel");
    }

    #[test]
    fn same_file_with_different_source_kind_conflicts() {
        let specs = vec![
            SourceDescriptor::script("main.srt", "print(\"hi\")"),
            SourceDescriptor {
                file_name: "main.srt".into(),
                source: "print(\"hi\")".into(),
                kind: SourceKind::ReplChunk,
                module_path: None,
            },
        ];

        let err = collect_sources(&specs).expect_err("different source kinds must conflict");
        assert!(
            matches!(err, LoadError::ConflictingSource { file_name } if file_name == "main.srt")
        );
    }

    #[test]
    fn same_file_module_and_std_module_conflicts() {
        let specs = vec![
            SourceDescriptor::module("bootstrap.srt", "defmod Bootstrap {}", "Bootstrap"),
            SourceDescriptor::std_module("bootstrap.srt", "defmod Bootstrap {}", "Bootstrap"),
        ];

        let err = collect_sources(&specs).expect_err("module and std module must not alias");
        assert!(
            matches!(err, LoadError::ConflictingSource { file_name } if file_name == "bootstrap.srt")
        );
    }

    #[test]
    fn compile_sources_preserves_stage_order() {
        let module_sources = collect_module_sources_with_module_stages(&[
            vec![ModuleInput {
                file_name: "std/math.srt".into(),
                source: "defmod Std::Math {}".into(),
                module_path: "Std::Math".into(),
            }],
            vec![
                ModuleInput {
                    file_name: "std/string.srt".into(),
                    source: "defmod Std::String {}".into(),
                    module_path: "Std::String".into(),
                },
                ModuleInput {
                    file_name: "std/list.srt".into(),
                    source: "defmod Std::List {}".into(),
                    module_path: "Std::List".into(),
                },
            ],
        ])
        .expect("staged module collection should succeed");
        let loaded = compose_script_compile_sources("main.srt", "print(\"hi\")", module_sources);

        assert_eq!(loaded.module_stages.len(), 4);
        assert_eq!(loaded.module_stages[0].len(), 1); // bootstrap
        assert_eq!(loaded.module_stages[1].len(), 1); // kernel
        assert_eq!(loaded.module_stages[2].len(), 1);
        assert_eq!(loaded.module_stages[3].len(), 2);
        assert_eq!(
            loaded.module_stages[0][0].source_id,
            loaded.builtin_source_id
        );
        assert_eq!(
            loaded.module_stages[0][0].source_kind,
            SourceKind::StdModule
        );
        assert_eq!(
            loaded.module_stages[1][0].source_kind,
            SourceKind::StdModule
        );
        assert_eq!(loaded.module_stages[2][0].source_kind, SourceKind::Module);
        assert_eq!(loaded.module_stages[3][0].source_kind, SourceKind::Module);
        assert_eq!(loaded.module_stages[3][1].source_kind, SourceKind::Module);
        assert_eq!(loaded.module_stages[1][0].module_path, "Kernel");
        assert_eq!(loaded.module_stages[2][0].module_path, "Std::Math");
        assert_eq!(loaded.module_stages[3][0].module_path, "Std::String");
        assert_eq!(loaded.module_stages[3][1].module_path, "Std::List");
    }

    #[test]
    fn module_path_is_derived_from_file_name() {
        assert_eq!(
            module_path_from_file_name("lib/std/math.srt").as_deref(),
            Some("lib::std::math")
        );
        assert_eq!(
            module_path_from_file_name("./bootstrap.srt").as_deref(),
            Some("bootstrap")
        );
        assert_eq!(module_path_from_file_name(""), None);
    }
}
