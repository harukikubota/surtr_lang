use std::collections::HashMap;
use std::fs;
use std::path::Path;

use diagnostics::{SourceId, SourceRegistry};

const BUILTIN_PRELUDE_FILE: &str = "bootstrap.srt";
const BUILTIN_PRELUDE_MODULE_PATH: &str = "Bootstrap";
const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/bootstrap.srt");
const SPECIAL_TYPES_FILE: &str = "special_types.srt";
const SPECIAL_TYPES_MODULE_PATH: &str = "SpecialTypes";
const SPECIAL_TYPES_SOURCE: &str = include_str!("../../../lib/special_types.srt");
const KERNEL_PRELUDE_FILE: &str = "kernel.srt";
const KERNEL_PRELUDE_MODULE_PATH: &str = "Kernel";
const KERNEL_PRELUDE_SOURCE: &str = include_str!("../../../lib/kernel.srt");
const DEFAULT_STD_MODULES: &[(&str, &str, &str)] = &[
    (
        "trait/add.srt",
        include_str!("../../../lib/trait/add.srt"),
        "Add",
    ),
    (
        "trait/sub.srt",
        include_str!("../../../lib/trait/sub.srt"),
        "Sub",
    ),
    (
        "trait/mul.srt",
        include_str!("../../../lib/trait/mul.srt"),
        "Mul",
    ),
    (
        "trait/eq.srt",
        include_str!("../../../lib/trait/eq.srt"),
        "Eq",
    ),
    (
        "trait/neq.srt",
        include_str!("../../../lib/trait/neq.srt"),
        "Neq",
    ),
    (
        "trait/compare.srt",
        include_str!("../../../lib/trait/compare.srt"),
        "Compare",
    ),
    (
        "trait/lt.srt",
        include_str!("../../../lib/trait/lt.srt"),
        "Lt",
    ),
    (
        "trait/lte.srt",
        include_str!("../../../lib/trait/lte.srt"),
        "Lte",
    ),
    (
        "trait/gt.srt",
        include_str!("../../../lib/trait/gt.srt"),
        "Gt",
    ),
    (
        "trait/gte.srt",
        include_str!("../../../lib/trait/gte.srt"),
        "Gte",
    ),
    (
        "trait/concat.srt",
        include_str!("../../../lib/trait/concat.srt"),
        "Concat",
    ),
    (
        "trait/numeric.srt",
        include_str!("../../../lib/trait/numeric.srt"),
        "Numeric",
    ),
    (
        "trait/show.srt",
        include_str!("../../../lib/trait/show.srt"),
        "Show",
    ),
    (
        "ordering.srt",
        include_str!("../../../lib/ordering.srt"),
        "Ordering",
    ),
    (
        "trait/ord.srt",
        include_str!("../../../lib/trait/ord.srt"),
        "Ord",
    ),
    (
        "trait/from.srt",
        include_str!("../../../lib/trait/from.srt"),
        "From",
    ),
    (
        "trait/try_from.srt",
        include_str!("../../../lib/trait/try_from.srt"),
        "TryFrom",
    ),
    (
        "trait/functor.srt",
        include_str!("../../../lib/trait/functor.srt"),
        "Functor",
    ),
    (
        "trait/chainable.srt",
        include_str!("../../../lib/trait/chainable.srt"),
        "Chainable",
    ),
    ("int.srt", include_str!("../../../lib/int.srt"), "Int"),
    (
        "string.srt",
        include_str!("../../../lib/string.srt"),
        "String",
    ),
    ("regex.srt", include_str!("../../../lib/regex.srt"), "Regex"),
    (
        "boolean.srt",
        include_str!("../../../lib/boolean.srt"),
        "Boolean",
    ),
    ("error.srt", include_str!("../../../lib/error.srt"), "Error"),
    ("list.srt", include_str!("../../../lib/list.srt"), "List"),
    (
        "generator.srt",
        include_str!("../../../lib/generator.srt"),
        "Generator",
    ),
    (
        "hash_map.srt",
        include_str!("../../../lib/hash_map.srt"),
        "HashMap",
    ),
    (
        "result.srt",
        include_str!("../../../lib/result.srt"),
        "Result",
    ),
    (
        "option.srt",
        include_str!("../../../lib/option.srt"),
        "Option",
    ),
    ("lens.srt", include_str!("../../../lib/lens.srt"), "Lens"),
    ("float.srt", include_str!("../../../lib/float.srt"), "Float"),
    (
        "Config.srt",
        include_str!("../../../lib/Config.srt"),
        "Config",
    ),
    (
        "Project.srt",
        include_str!("../../../lib/Project.srt"),
        "Project",
    ),
];
const REPL_MODULE_NAME: &str = "REPL";
const SCRIPT_PSEUDO_MODULE_PREFIX: &str = "__Script";
const REPL_PSEUDO_MODULE_PATH: &str = "__Repl::Session";

/// Logical source categories that drive parser/typechecker policy selection.
///
/// The loader always materializes standard sources in the fixed order
/// `Bootstrap -> [SpecialTypes + Kernel + other standard modules] -> user source`.
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
    BootstrapFailed {
        phase: String,
        file_name: String,
        message: String,
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
            Self::BootstrapFailed {
                phase,
                file_name,
                message,
            } => write!(
                f,
                "bootstrap failed during {} for `{}`: {}",
                phase, file_name, message
            ),
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

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn lib_module_path_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(ToString::to_string)
        .unwrap_or_default()
}

pub fn derive_primary_module_path(source: &str) -> Option<String> {
    let stripped = crate::strip_test_annotations(source);
    let ast = spire::parse_with_context(
        &stripped,
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::module()),
    )
    .or_else(|_| {
        spire::parse_with_context(
            &stripped,
            spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
        )
    })
    .ok()?;
    crate::lower_module_source_ast(ast, None)
        .into_iter()
        .find(|module| module.declared_span.is_some() && !module.module_path.is_empty())
        .map(|module| module.module_path)
}

pub fn collect_lib_module_inputs() -> Result<Vec<ModuleInput>, LoadError> {
    let lib_dir = Path::new("lib");
    if !lib_dir.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(lib_dir).map_err(|e| LoadError::SourceReadFailed {
        file_name: display_path(lib_dir),
        message: e.to_string(),
    })?;

    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "srt")
        {
            files.push(path);
        }
    }
    files.sort();

    let mut module_inputs = Vec::with_capacity(files.len());
    for path in files {
        let file_name = display_path(&path);
        let source = fs::read_to_string(&path).map_err(|e| LoadError::SourceReadFailed {
            file_name: file_name.clone(),
            message: e.to_string(),
        })?;
        let module_path = derive_primary_module_path(&source)
            .filter(|module_path| !module_path.is_empty())
            .unwrap_or_else(|| lib_module_path_from_path(&path));
        module_inputs.push(ModuleInput {
            file_name,
            source,
            module_path,
        });
    }

    Ok(module_inputs)
}

pub fn collect_additional_default_std_module_inputs() -> Result<Vec<ModuleInput>, LoadError> {
    Ok(collect_lib_module_inputs()?
        .into_iter()
        .filter(|module| {
            let file_name = Path::new(&module.file_name)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            !is_default_std_module_file_name(file_name)
                && !is_default_std_module_path(&module.module_path)
        })
        .collect())
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

fn build_module_sources_from_stage_specs(
    stage_specs: Vec<Vec<SourceDescriptor>>,
) -> Result<ModuleSources, LoadError> {
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

pub fn is_default_std_module_path(module_path: &str) -> bool {
    module_path == BUILTIN_PRELUDE_MODULE_PATH
        || module_path == SPECIAL_TYPES_MODULE_PATH
        || module_path == KERNEL_PRELUDE_MODULE_PATH
        || DEFAULT_STD_MODULES
            .iter()
            .any(|(_, _, builtin_module_path)| *builtin_module_path == module_path)
}

pub fn is_default_std_module_file_name(file_name: &str) -> bool {
    file_name == BUILTIN_PRELUDE_FILE
        || file_name == SPECIAL_TYPES_FILE
        || file_name == KERNEL_PRELUDE_FILE
        || DEFAULT_STD_MODULES
            .iter()
            .any(|(builtin_file_name, _, _)| *builtin_file_name == file_name)
}

pub fn collect_module_sources_with_extra_std_sources(
    extra_std_sources: &[SourceDescriptor],
    module_input_stages: &[Vec<ModuleInput>],
) -> Result<ModuleSources, LoadError> {
    // Stage 0/1 are reserved for the built-in standard layers. User-provided
    // modules are appended afterwards so they can depend on
    // `Bootstrap -> [SpecialTypes + Kernel + other std modules]` but never precede them.
    let mut stage_specs = vec![
        vec![SourceDescriptor::std_module(
            BUILTIN_PRELUDE_FILE,
            BUILTIN_PRELUDE_SOURCE,
            BUILTIN_PRELUDE_MODULE_PATH,
        )],
        std::iter::once(SourceDescriptor::std_module(
            SPECIAL_TYPES_FILE,
            SPECIAL_TYPES_SOURCE,
            SPECIAL_TYPES_MODULE_PATH,
        ))
        .chain(std::iter::once(SourceDescriptor::std_module(
            KERNEL_PRELUDE_FILE,
            KERNEL_PRELUDE_SOURCE,
            KERNEL_PRELUDE_MODULE_PATH,
        )))
        .chain(
            DEFAULT_STD_MODULES
                .iter()
                .map(|(file_name, source, module_path)| {
                    SourceDescriptor::std_module(*file_name, *source, *module_path)
                }),
        )
        .collect(),
    ];

    if !extra_std_sources.is_empty() {
        stage_specs.push(extra_std_sources.to_vec());
    }

    for stage in module_input_stages {
        if stage.is_empty() {
            continue;
        }
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
    build_module_sources_from_stage_specs(stage_specs)
}

pub fn collect_module_sources_with_module_stages(
    module_input_stages: &[Vec<ModuleInput>],
) -> Result<ModuleSources, LoadError> {
    collect_module_sources_with_extra_std_sources(&[], module_input_stages)
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
            let module_path = derive_primary_module_path(&source)
                .or_else(|| module_path_from_file_name(file_name))
                .ok_or_else(|| LoadError::EmptyModulePath {
                    file_name: file_name.clone(),
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

pub(crate) fn collect_repl_sources_with_module_stages(
    module_input_stages: &[Vec<ModuleInput>],
) -> Result<ReplSources, LoadError> {
    let mut module_sources = collect_module_sources_with_module_stages(module_input_stages)?;
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
        assert_eq!(
            loaded.module_source_ids.len(),
            3 + DEFAULT_STD_MODULES.len()
        );
        assert_eq!(loaded.module_source_ids[0], loaded.builtin_source_id);
        assert_eq!(loaded.module_stages.len(), 2);
        assert_eq!(loaded.module_stages[0][0].module_path, "Bootstrap");
        assert_eq!(loaded.module_stages[1].len(), 2 + DEFAULT_STD_MODULES.len());
        let std_paths = loaded.module_stages[1]
            .iter()
            .map(|module| module.module_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            std_paths,
            vec![
                "SpecialTypes",
                "Kernel",
                "Add",
                "Sub",
                "Mul",
                "Eq",
                "Neq",
                "Compare",
                "Lt",
                "Lte",
                "Gt",
                "Gte",
                "Concat",
                "Numeric",
                "Show",
                "Ordering",
                "Ord",
                "From",
                "TryFrom",
                "Functor",
                "Chainable",
                "Int",
                "String",
                "Regex",
                "Boolean",
                "Error",
                "List",
                "Generator",
                "HashMap",
                "Result",
                "Option",
                "Lens",
                "Float",
                "Config",
                "Project",
            ]
        );
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
        assert_eq!(loaded.module_stages[1].len(), 2 + DEFAULT_STD_MODULES.len()); // special types + kernel + other std modules
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
        assert_eq!(
            loaded.module_stages[1][1].source_kind,
            SourceKind::StdModule
        );
        assert_eq!(loaded.module_stages[2][0].source_kind, SourceKind::Module);
        assert_eq!(loaded.module_stages[3][0].source_kind, SourceKind::Module);
        assert_eq!(loaded.module_stages[3][1].source_kind, SourceKind::Module);
        let std_paths = loaded.module_stages[1]
            .iter()
            .map(|module| module.module_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            std_paths,
            vec![
                "SpecialTypes",
                "Kernel",
                "Add",
                "Sub",
                "Mul",
                "Eq",
                "Neq",
                "Compare",
                "Lt",
                "Lte",
                "Gt",
                "Gte",
                "Concat",
                "Numeric",
                "Show",
                "Ordering",
                "Ord",
                "From",
                "TryFrom",
                "Functor",
                "Chainable",
                "Int",
                "String",
                "Regex",
                "Boolean",
                "Error",
                "List",
                "Generator",
                "HashMap",
                "Result",
                "Option",
                "Lens",
                "Float",
                "Config",
                "Project",
            ]
        );
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

    #[test]
    fn derive_primary_module_path_ignores_test_annotations() {
        let source = r#"@@test 1 == 1
defmod Math {
  def add(x: Int, y: Int) -> Int { x + y }
}"#;
        assert_eq!(derive_primary_module_path(source).as_deref(), Some("Math"));
    }
}
