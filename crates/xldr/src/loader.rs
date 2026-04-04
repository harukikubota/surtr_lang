use std::collections::HashMap;
use std::fs;

use diagnostics::{SourceId, SourceRegistry};
use spire::CompileUnitKind;

const BUILTIN_PRELUDE_FILE: &str = "bootstrap.srt";
const BUILTIN_PRELUDE_MODULE_PATH: &str = "Bootstrap";
const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/bootstrap.srt");
const REPL_MODULE_NAME: &str = "REPL";

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceSpec {
    file_name: String,
    source: String,
    unit_kind: CompileUnitKind,
    module_path: Option<String>,
}

impl SourceSpec {
    fn script(file_name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            source: source.into(),
            unit_kind: CompileUnitKind::Script,
            module_path: None,
        }
    }

    fn module(
        file_name: impl Into<String>,
        source: impl Into<String>,
        module_path: impl Into<String>,
    ) -> Self {
        Self {
            file_name: file_name.into(),
            source: source.into(),
            unit_kind: CompileUnitKind::Module,
            module_path: Some(module_path.into()),
        }
    }

    fn repl(file_name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            source: source.into(),
            unit_kind: CompileUnitKind::Repl,
            module_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceBinding {
    source_id: SourceId,
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

fn collect_sources(specs: &[SourceSpec]) -> Result<CollectedSources, LoadError> {
    let mut sources = SourceRegistry::new();
    let mut bindings = Vec::with_capacity(specs.len());
    let mut by_file: HashMap<String, (SourceId, String, CompileUnitKind, Option<String>)> =
        HashMap::new();
    let mut by_module: HashMap<String, String> = HashMap::new();

    for spec in specs {
        if let Some((source_id, existing_source, existing_kind, existing_module)) =
            by_file.get(&spec.file_name)
        {
            if existing_source == &spec.source
                && existing_kind == &spec.unit_kind
                && existing_module == &spec.module_path
            {
                bindings.push(SourceBinding {
                    source_id: *source_id,
                    module_path: spec.module_path.clone(),
                });
                continue;
            }

            return Err(LoadError::ConflictingSource {
                file_name: spec.file_name.clone(),
            });
        }

        if let Some(module_path) = &spec.module_path {
            if let Some(first_file_name) = by_module.get(module_path) {
                return Err(LoadError::DuplicateModulePath {
                    module_path: module_path.clone(),
                    first_file_name: first_file_name.clone(),
                    second_file_name: spec.file_name.clone(),
                });
            }
        }

        let source_id = sources.register(spec.file_name.clone(), spec.source.clone());
        by_file.insert(
            spec.file_name.clone(),
            (
                source_id,
                spec.source.clone(),
                spec.unit_kind,
                spec.module_path.clone(),
            ),
        );

        if let Some(module_path) = &spec.module_path {
            by_module.insert(module_path.clone(), spec.file_name.clone());
        }

        bindings.push(SourceBinding {
            source_id,
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
}

#[derive(Debug, Clone)]
pub struct CompileSources {
    pub sources: SourceRegistry,
    pub user_source_id: SourceId,
    pub builtin_source_id: SourceId,
    pub builtin_module_path: Option<String>,
    pub module_source_ids: Vec<SourceId>,
    pub module_stages: Vec<Vec<StagedModule>>,
}

pub fn collect_compile_sources(
    user_file_name: &str,
    user_source: &str,
) -> Result<CompileSources, LoadError> {
    collect_compile_sources_with_module_stages(user_file_name, user_source, &[])
}

pub fn collect_compile_sources_with_modules(
    user_file_name: &str,
    user_source: &str,
    module_inputs: &[ModuleInput],
) -> Result<CompileSources, LoadError> {
    if module_inputs.is_empty() {
        return collect_compile_sources_with_module_stages(user_file_name, user_source, &[]);
    }
    collect_compile_sources_with_module_stages(
        user_file_name,
        user_source,
        &[module_inputs.to_vec()],
    )
}

pub fn collect_compile_sources_with_module_stages(
    user_file_name: &str,
    user_source: &str,
    module_input_stages: &[Vec<ModuleInput>],
) -> Result<CompileSources, LoadError> {
    let mut stage_specs = vec![vec![SourceSpec::module(
        BUILTIN_PRELUDE_FILE,
        BUILTIN_PRELUDE_SOURCE,
        BUILTIN_PRELUDE_MODULE_PATH,
    )]];

    for stage in module_input_stages {
        let mut specs = Vec::with_capacity(stage.len());
        for module in stage {
            specs.push(SourceSpec::module(
                module.file_name.clone(),
                module.source.clone(),
                module.module_path.clone(),
            ));
        }
        stage_specs.push(specs);
    }

    let mut flattened_specs = vec![SourceSpec::script(user_file_name, user_source)];
    for stage in &stage_specs {
        for spec in stage {
            flattened_specs.push(spec.clone());
        }
    }

    let collected = collect_sources(&flattened_specs)?;
    let user = &collected.bindings[0];

    let mut idx = 1;
    let mut module_stages = Vec::with_capacity(stage_specs.len());
    for stage in &stage_specs {
        let mut stage_bindings = Vec::with_capacity(stage.len());
        for _ in stage {
            let binding = &collected.bindings[idx];
            idx += 1;
            stage_bindings.push(StagedModule {
                source_id: binding.source_id,
                module_path: binding.module_path.clone().unwrap_or_default(),
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

    Ok(CompileSources {
        sources: collected.sources,
        user_source_id: user.source_id,
        builtin_source_id: builtin.source_id,
        builtin_module_path: Some(builtin.module_path.clone()),
        module_source_ids,
        module_stages,
    })
}

pub fn collect_compile_sources_with_module_file_stages(
    user_file_name: &str,
    user_source: &str,
    module_file_stages: &[Vec<String>],
) -> Result<CompileSources, LoadError> {
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

    collect_compile_sources_with_module_stages(user_file_name, user_source, &module_input_stages)
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
    pub(crate) builtin_module_path: Option<String>,
    pub(crate) repl_source_id: SourceId,
}

pub(crate) fn collect_repl_sources() -> Result<ReplSources, LoadError> {
    let specs = vec![
        SourceSpec::module(
            BUILTIN_PRELUDE_FILE,
            BUILTIN_PRELUDE_SOURCE,
            BUILTIN_PRELUDE_MODULE_PATH,
        ),
        SourceSpec::repl(REPL_MODULE_NAME, ""),
    ];
    let collected = collect_sources(&specs)?;
    let builtin = &collected.bindings[0];
    let repl = &collected.bindings[1];

    Ok(ReplSources {
        sources: collected.sources,
        builtin_source_id: builtin.source_id,
        builtin_module_path: builtin.module_path.clone(),
        repl_source_id: repl.source_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_source_is_registered_once() {
        let specs = vec![
            SourceSpec::module("a.srt", "defmod A {}", "A"),
            SourceSpec::module("a.srt", "defmod A {}", "A"),
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
    fn duplicate_module_path_returns_error() {
        let specs = vec![
            SourceSpec::module("a.srt", "defmod A {}", "Std::Math"),
            SourceSpec::module("b.srt", "defmod B {}", "Std::Math"),
        ];

        let err = collect_sources(&specs).expect_err("duplicate module path must fail");
        assert!(matches!(
            err,
            LoadError::DuplicateModulePath { module_path, .. } if module_path == "Std::Math"
        ));
    }

    #[test]
    fn compile_sources_register_user_and_builtin() {
        let loaded = collect_compile_sources("main.srt", "print(\"hi\")")
            .expect("compile source collection should succeed");

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
        assert_eq!(loaded.module_source_ids.len(), 1);
        assert_eq!(loaded.module_source_ids[0], loaded.builtin_source_id);
        assert_eq!(loaded.module_stages.len(), 1);
    }

    #[test]
    fn compile_sources_preserves_stage_order() {
        let loaded = collect_compile_sources_with_module_stages(
            "main.srt",
            "print(\"hi\")",
            &[
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
            ],
        )
        .expect("staged compile source collection should succeed");

        assert_eq!(loaded.module_stages.len(), 3);
        assert_eq!(loaded.module_stages[0].len(), 1); // bootstrap
        assert_eq!(loaded.module_stages[1].len(), 1);
        assert_eq!(loaded.module_stages[2].len(), 2);
        assert_eq!(
            loaded.module_stages[0][0].source_id,
            loaded.builtin_source_id
        );
        assert_eq!(loaded.module_stages[1][0].module_path, "Std::Math");
        assert_eq!(loaded.module_stages[2][0].module_path, "Std::String");
        assert_eq!(loaded.module_stages[2][1].module_path, "Std::List");
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
