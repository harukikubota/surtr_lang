use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use diagnostics::{SourceId, SourceRegistry};
use sindr::policy::SourceKind;
use spire::ast::{Ast, Span};

pub(crate) use sindr::stdlib::stdlib_module_specs;
pub use sindr::stdlib::StdlibVariant;
use sindr::stdlib::{StdlibStage, BUILTIN_PRELUDE_FILE, STDLIB_MODULE_SPECS};
#[cfg(test)]
use sindr::stdlib::{BUILTIN_PRELUDE_MODULE_PATH, TEST_STD_MODULE_PATH};

const REPL_MODULE_NAME: &str = "REPL";
const SCRIPT_PSEUDO_MODULE_PREFIX: &str = "__Script";
const REPL_PSEUDO_MODULE_PATH: &str = "__Repl::Session";

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
            kind: SourceKind::DefinitionSource,
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
            kind: SourceKind::StdDefinitionSource,
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
                module_path.strip_prefix("Global::").unwrap_or(module_path),
                first_file_name,
                second_file_name
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

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptIncludeDirective {
    pub file_path: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScriptSourcePrepareError {
    Parse { error: spire::error::ParseError },
    IncludeRead { message: String, span: Span },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedScriptSources {
    pub source_for_parse: String,
    pub include_directives: Vec<ScriptIncludeDirective>,
    pub include_modules: Vec<ModuleInput>,
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn module_path_from_source_or_file_name(file_name: &str, source: &str) -> String {
    derive_primary_module_path(source)
        .or_else(|| const_only_module_path_from_file_stem(file_name, source))
        .filter(|module_path| !module_path.is_empty())
        .unwrap_or_else(|| module_path_from_file_name_lossy(file_name))
}

fn const_only_module_path_from_file_stem(file_name: &str, source: &str) -> Option<String> {
    let ast = spire::parse_with_context(
        source,
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::module()),
    )
    .or_else(|_| {
        spire::parse_with_context(
            source,
            spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
        )
    })
    .ok()?;
    let fallback = Path::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty());
    sigil::const_only_fallback_module_path(&ast, fallback).map(str::to_string)
}

fn module_path_from_file_name_lossy(file_name: &str) -> String {
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
        "Main".to_string()
    } else {
        segments.join("::")
    }
}

pub fn collect_script_include_directives(
    source: &str,
    source_kind: SourceKind,
) -> Result<(String, Vec<ScriptIncludeDirective>), ScriptSourcePrepareError> {
    let ast = spire::parse_with_context(
        source,
        crate::derive_parser_context(0, source_kind, sindr::policy::CompileUnitKind::Script, None),
    )
    .map_err(|error| ScriptSourcePrepareError::Parse { error })?;

    let mut chars = source.chars().collect::<Vec<_>>();
    let mut directives = Vec::new();
    for stmt in &ast {
        if let Ast::Include(span, file_path) = stmt {
            directives.push(ScriptIncludeDirective {
                file_path: file_path.clone(),
                span: span.clone(),
            });
            for ch in chars.iter_mut().take(span.end).skip(span.start) {
                if *ch != '\n' {
                    *ch = ' ';
                }
            }
        }
    }

    Ok((chars.into_iter().collect::<String>(), directives))
}

pub fn prepare_script_sources(
    file_name: &str,
    source: &str,
    source_kind: SourceKind,
) -> Result<PreparedScriptSources, ScriptSourcePrepareError> {
    let (source_for_parse, include_directives) =
        collect_script_include_directives(source, source_kind)?;
    let include_modules = include_directives
        .iter()
        .map(|directive| resolve_script_include_module_input(file_name, source, directive))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(PreparedScriptSources {
        source_for_parse,
        include_directives,
        include_modules,
    })
}

fn resolve_script_include_module_input(
    script_file_path: &str,
    _script_source: &str,
    directive: &ScriptIncludeDirective,
) -> Result<ModuleInput, ScriptSourcePrepareError> {
    let resolved_path = resolve_script_include_file_path(script_file_path, &directive.file_path);
    let display_path = display_path(&resolved_path);
    let module_source =
        fs::read_to_string(&resolved_path).map_err(|e| ScriptSourcePrepareError::IncludeRead {
            span: directive.span.clone(),
            message: format!(
                "include failed to read `{}`: {}",
                resolved_path.display(),
                e
            ),
        })?;
    let module_path = module_path_from_source_or_file_name(&display_path, &module_source);

    Ok(ModuleInput {
        file_name: display_path,
        source: module_source,
        module_path,
    })
}

fn resolve_script_include_file_path(script_file_path: &str, raw_path: &str) -> PathBuf {
    let candidate = Path::new(raw_path);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }

    let base_dir = Path::new(script_file_path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    base_dir.join(candidate)
}

fn lib_relative_path(path: &Path) -> String {
    path.strip_prefix("lib")
        .map(display_path)
        .unwrap_or_else(|_| display_path(path))
}

fn lib_module_path_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(ToString::to_string)
        .unwrap_or_default()
}

pub fn derive_primary_module_path(source: &str) -> Option<String> {
    let ast = spire::parse_with_context(
        source,
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::module()),
    )
    .or_else(|_| {
        spire::parse_with_context(
            source,
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

    let mut files = Vec::new();
    collect_lib_module_files(lib_dir, &mut files)?;
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

fn collect_lib_module_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), LoadError> {
    let read_error = |path: &Path, error: std::io::Error| LoadError::SourceReadFailed {
        file_name: display_path(path),
        message: error.to_string(),
    };
    let canonical_root = fs::canonicalize(root).map_err(|error| read_error(root, error))?;
    let tests_path = root.join("tests");
    let excluded = match fs::canonicalize(&tests_path) {
        Ok(path) => Some(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(read_error(&tests_path, error)),
    };
    let mut pending = vec![canonical_root.clone()];
    let mut visited = HashSet::new();
    while let Some(path) = pending.pop() {
        let actual = fs::canonicalize(&path).map_err(|error| read_error(&path, error))?;
        let Ok(relative) = actual.strip_prefix(&canonical_root) else {
            continue;
        };
        if excluded
            .as_ref()
            .is_some_and(|tests| actual.starts_with(tests))
            || !visited.insert(actual.clone())
        {
            continue;
        }
        let metadata = fs::metadata(&actual).map_err(|error| read_error(&path, error))?;
        if metadata.is_dir() {
            let entries = fs::read_dir(&actual).map_err(|error| read_error(&path, error))?;
            for entry in entries {
                pending.push(entry.map_err(|error| read_error(&path, error))?.path());
            }
        } else if actual.extension().and_then(|extension| extension.to_str()) == Some("srt") {
            // Retain the caller's root spelling, but use the real file's identity.
            files.push(root.join(relative));
        }
    }
    Ok(())
}

pub fn collect_additional_default_std_module_inputs() -> Result<Vec<ModuleInput>, LoadError> {
    let lib_dir = Path::new("lib");
    if !lib_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    collect_lib_module_files(lib_dir, &mut files)?;
    files.sort();

    let mut module_inputs = Vec::new();
    for path in files {
        let relative_file_name = lib_relative_path(&path);
        if is_default_std_module_file_name(&relative_file_name) {
            continue;
        }

        let file_name = display_path(&path);
        let source = fs::read_to_string(&path).map_err(|e| LoadError::SourceReadFailed {
            file_name: file_name.clone(),
            message: e.to_string(),
        })?;
        let module_path = derive_primary_module_path(&source)
            .filter(|module_path| !module_path.is_empty())
            .unwrap_or_else(|| lib_module_path_from_path(&path));
        if is_default_std_module_path(&module_path) {
            continue;
        }

        module_inputs.push(ModuleInput {
            file_name,
            source,
            module_path,
        });
    }

    Ok(module_inputs)
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
    pub stdlib_variant: StdlibVariant,
}

pub(crate) fn stdlib_module_spec_cache_key(stdlib_variant: StdlibVariant) -> String {
    let mut key = String::new();
    for spec in stdlib_module_specs(stdlib_variant) {
        key.push_str(spec.file_name);
        key.push('\x1e');
        key.push_str(spec.module_path);
        key.push('\x1e');
        key.push_str(match spec.stage {
            StdlibStage::Bootstrap => "bootstrap",
            StdlibStage::Main => "main",
            StdlibStage::TestExtension => "test-extension",
        });
        key.push('\x1e');
        key.push_str(match spec.variant {
            StdlibVariant::Default => "variant=default",
            StdlibVariant::TestEnabled => "variant=test-enabled",
        });
        key.push('\x1e');
        key.push_str(spec.source);
        key.push('\x1f');
    }
    key
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
    let module_path = module_path.strip_prefix("Global::").unwrap_or(module_path);
    STDLIB_MODULE_SPECS
        .iter()
        .any(|spec| spec.module_path == module_path)
}

pub fn is_default_std_module_file_name(file_name: &str) -> bool {
    STDLIB_MODULE_SPECS
        .iter()
        .any(|spec| spec.file_name == file_name)
}

pub fn collect_module_sources_with_extra_std_sources(
    extra_std_sources: &[SourceDescriptor],
    module_input_stages: &[Vec<ModuleInput>],
) -> Result<ModuleSources, LoadError> {
    collect_module_sources_with_stdlib_variant(
        StdlibVariant::Default,
        extra_std_sources,
        module_input_stages,
    )
}

pub fn collect_module_sources_with_stdlib_variant(
    stdlib_variant: StdlibVariant,
    extra_std_sources: &[SourceDescriptor],
    module_input_stages: &[Vec<ModuleInput>],
) -> Result<ModuleSources, LoadError> {
    // Stage 0/1 are reserved for the built-in standard layers. User-provided
    // modules are appended afterwards so they can depend on
    // `Bootstrap -> [SpecialTypes + Function + Kernel + other std modules]` but never precede them.
    let mut stage_specs = vec![Vec::new(), Vec::new()];
    for spec in stdlib_module_specs(stdlib_variant) {
        let stage_index = match spec.stage {
            StdlibStage::Bootstrap => 0,
            StdlibStage::Main | StdlibStage::TestExtension => 1,
        };
        stage_specs[stage_index].push(SourceDescriptor::std_module(
            spec.file_name,
            spec.source,
            spec.module_path,
        ));
    }

    if !extra_std_sources.is_empty() {
        stage_specs.push(extra_std_sources.to_vec());
    }

    let module_sources = build_module_sources_from_stage_specs(stage_specs)?;
    extend_module_sources_with_module_stages(module_sources, module_input_stages)
}

/// Append definition stages while preserving the prefix's source IDs and layout.
pub fn extend_module_sources_with_module_stages(
    mut module_sources: ModuleSources,
    module_input_stages: &[Vec<ModuleInput>],
) -> Result<ModuleSources, LoadError> {
    if module_input_stages.iter().all(Vec::is_empty) {
        return Ok(module_sources);
    }
    let mut by_file = HashMap::new();
    for module in module_sources.module_stages.iter().flatten() {
        let file_name = module_sources
            .sources
            .file_name(module.source_id)
            .expect("module source must be registered");
        by_file.insert(
            file_name.to_string(),
            (
                module.source_id,
                module.source_kind,
                module.module_path.clone(),
            ),
        );
    }
    for stage in module_input_stages {
        if stage.is_empty() {
            continue;
        }
        let mut modules = Vec::with_capacity(stage.len());
        for module in stage {
            let source_id = if let Some((source_id, source_kind, module_path)) =
                by_file.get(&module.file_name)
            {
                if *source_kind != SourceKind::DefinitionSource
                    || module_path != &module.module_path
                    || module_sources.sources.source(*source_id) != Some(module.source.as_str())
                {
                    return Err(LoadError::ConflictingSource {
                        file_name: module.file_name.clone(),
                    });
                }
                *source_id
            } else {
                let source_id = module_sources
                    .sources
                    .register(module.file_name.clone(), module.source.clone());
                by_file.insert(
                    module.file_name.clone(),
                    (
                        source_id,
                        SourceKind::DefinitionSource,
                        module.module_path.clone(),
                    ),
                );
                source_id
            };
            module_sources.module_source_ids.push(source_id);
            modules.push(StagedModule {
                source_id,
                module_path: module.module_path.clone(),
                source_kind: SourceKind::DefinitionSource,
            });
        }
        module_sources.module_stages.push(modules);
    }
    Ok(module_sources)
}

pub fn collect_module_sources_with_module_stages(
    module_input_stages: &[Vec<ModuleInput>],
) -> Result<ModuleSources, LoadError> {
    collect_module_sources_with_stdlib_variant(StdlibVariant::Default, &[], module_input_stages)
}

pub fn collect_test_module_sources_with_module_stages(
    module_input_stages: &[Vec<ModuleInput>],
) -> Result<ModuleSources, LoadError> {
    collect_module_sources_with_stdlib_variant(StdlibVariant::TestEnabled, &[], module_input_stages)
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
        stdlib_variant: StdlibVariant::Default,
    }
}

pub fn compose_script_compile_sources_with_stdlib_variant(
    user_file_name: &str,
    user_source: &str,
    mut module_sources: ModuleSources,
    stdlib_variant: StdlibVariant,
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
        stdlib_variant,
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

    #[cfg(unix)]
    #[test]
    fn standard_source_walk_uses_real_paths_for_scope_and_duplicates() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!(
            "surtr-stdlib-walk-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        fs::create_dir_all(root.join("lib/nested")).unwrap();
        let _cleanup = Cleanup(root.clone());
        fs::create_dir_all(root.join("lib/tests")).unwrap();
        fs::create_dir_all(root.join("external")).unwrap();
        for relative in [
            "lib/main.srt",
            "lib/nested/extra.srt",
            "lib/tests/hidden.srt",
            "external/outside.srt",
        ] {
            fs::write(root.join(relative), "defmod Example {}").unwrap();
        }
        symlink("tests", root.join("lib/test_alias")).unwrap();
        symlink("../external", root.join("lib/external_alias")).unwrap();
        symlink("tests/hidden.srt", root.join("lib/hidden_alias.srt")).unwrap();
        symlink(
            "../external/outside.srt",
            root.join("lib/outside_alias.srt"),
        )
        .unwrap();
        symlink("nested/extra.srt", root.join("lib/valid_alias.srt")).unwrap();
        let mut files = Vec::new();
        collect_lib_module_files(&root.join("lib"), &mut files).unwrap();
        files.sort();
        assert_eq!(
            files,
            vec![root.join("lib/main.srt"), root.join("lib/nested/extra.srt")]
        );
        symlink("..", root.join("lib/nested/ancestor")).unwrap();
        let mut cycle_files = Vec::new();
        collect_lib_module_files(&root.join("lib"), &mut cycle_files).unwrap();
        cycle_files.sort();
        assert_eq!(cycle_files, files);
    }

    fn stdlib_stage_len(stdlib_variant: StdlibVariant, stage: StdlibStage) -> usize {
        stdlib_module_specs(stdlib_variant)
            .filter(|spec| spec.stage == stage)
            .count()
    }

    #[test]
    fn stdlib_module_specs_expose_variant_metadata() {
        let default_specs = stdlib_module_specs(StdlibVariant::Default).collect::<Vec<_>>();
        let test_specs = stdlib_module_specs(StdlibVariant::TestEnabled).collect::<Vec<_>>();

        assert!(default_specs
            .iter()
            .all(|spec| spec.variant == StdlibVariant::Default));
        assert!(test_specs.iter().any(|spec| {
            spec.module_path == TEST_STD_MODULE_PATH && spec.variant == StdlibVariant::TestEnabled
        }));
        assert!(stdlib_module_spec_cache_key(StdlibVariant::TestEnabled)
            .contains("variant=test-enabled"));
    }

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
            stdlib_stage_len(StdlibVariant::Default, StdlibStage::Bootstrap)
                + stdlib_stage_len(StdlibVariant::Default, StdlibStage::Main)
        );
        assert_eq!(loaded.module_source_ids[0], loaded.builtin_source_id);
        assert_eq!(loaded.module_stages.len(), 2);
        assert_eq!(loaded.module_stages[0][0].module_path, "Bootstrap");
        assert_eq!(
            loaded.module_stages[1].len(),
            stdlib_stage_len(StdlibVariant::Default, StdlibStage::Main)
        );
        let std_paths = loaded.module_stages[1]
            .iter()
            .map(|module| module.module_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            std_paths,
            vec![
                "SpecialTypes",
                "Function",
                "Kernel",
                "Add",
                "Sub",
                "Mul",
                "Div",
                "Mod",
                "Eq",
                "Compare",
                "Concat",
                "Show",
                "Default",
                "Ordering",
                "Tuple",
                "Convert",
                "TryConvert",
                "Encode",
                "Decode",
                "Functor",
                "Bifunctor",
                "Applicative",
                "Monad",
                "MonadFail",
                "MonadT",
                "Identity",
                "Reader",
                "State",
                "Alternative",
                "Monoid",
                "Int",
                "String",
                "Regex",
                "Boolean",
                "Error",
                "List",
                "Generator",
                "InfiniteGenerator",
                "HashMap",
                "Result",
                "Extractor",
                "Either",
                "Duration",
                "Range",
                "Option",
                "OptionT",
                "EitherT",
                "ResultT",
                "ReaderT",
                "StateT",
                "Task",
                "Facet",
                "Float",
                "Json",
                "Config",
                "Project",
                "Random",
                "File",
                "FS",
                "IO",
                "Shell",
                "StyledDoc",
            ]
        );
    }

    #[test]
    fn extending_module_sources_preserves_prefix_and_duplicate_stages() {
        let prefix = collect_test_module_sources_with_module_stages(&[])
            .expect("standard sources should load");
        let dependency = ModuleInput {
            file_name: "support/helper.srt".into(),
            source: "defmod Helper { def value() -> Int { 1 } }".into(),
            module_path: "Helper".into(),
        };
        let extended = extend_module_sources_with_module_stages(
            prefix.clone(),
            &[vec![dependency.clone()], vec![dependency]],
        )
        .expect("identical source registration should succeed");
        assert_eq!(
            &extended.sources.entries()[..prefix.sources.entries().len()],
            prefix.sources.entries()
        );
        assert_eq!(
            &extended.module_stages[..prefix.module_stages.len()],
            prefix.module_stages
        );
        assert_eq!(
            extended.sources.entries().len(),
            prefix.sources.entries().len() + 1
        );
        assert_eq!(extended.module_stages.len(), prefix.module_stages.len() + 2);
        assert_eq!(
            extended.module_stages[prefix.module_stages.len()][0].source_id,
            extended.module_stages[prefix.module_stages.len() + 1][0].source_id
        );
    }

    #[test]
    fn extending_module_sources_rejects_conflicting_source_identity() {
        let prefix = collect_test_module_sources_with_module_stages(&[])
            .expect("standard sources should load");
        let dependency = ModuleInput {
            file_name: "support/helper.srt".into(),
            source: "defmod Helper { def value() -> Int { 1 } }".into(),
            module_path: "Helper".into(),
        };
        let extended =
            extend_module_sources_with_module_stages(prefix.clone(), &[vec![dependency.clone()]])
                .expect("dependency should load");
        let mut changed_body = dependency.clone();
        changed_body.source.push_str("\n");
        let mut changed_module = dependency;
        changed_module.module_path = "Other".into();
        for changed in [changed_body, changed_module] {
            let error =
                extend_module_sources_with_module_stages(extended.clone(), &[vec![changed]])
                    .expect_err("different source identities must conflict");
            assert!(matches!(error, LoadError::ConflictingSource { .. }));
        }
        let standard = &prefix.sources.entries()[0];
        let explicit_standard = ModuleInput {
            file_name: standard.file_name.clone(),
            source: standard.source.clone(),
            module_path: prefix.module_stages[0][0].module_path.clone(),
        };
        assert!(
            matches!(
                extend_module_sources_with_module_stages(prefix, &[vec![explicit_standard]]),
                Err(LoadError::ConflictingSource { .. })
            ),
            "source kind differences must conflict"
        );
    }

    #[test]
    fn test_enabled_compile_sources_include_test_module() {
        let module_sources = collect_test_module_sources_with_module_stages(&[])
            .expect("test-enabled module collection must succeed");
        let loaded = compose_script_compile_sources_with_stdlib_variant(
            "main.srt",
            "print(\"hi\")",
            module_sources,
            StdlibVariant::TestEnabled,
        );

        assert_eq!(
            loaded.module_source_ids.len(),
            stdlib_stage_len(StdlibVariant::TestEnabled, StdlibStage::Bootstrap)
                + stdlib_stage_len(StdlibVariant::TestEnabled, StdlibStage::Main)
                + stdlib_stage_len(StdlibVariant::TestEnabled, StdlibStage::TestExtension)
        );
        assert_eq!(
            loaded.module_stages[1].len(),
            stdlib_stage_len(StdlibVariant::TestEnabled, StdlibStage::Main)
                + stdlib_stage_len(StdlibVariant::TestEnabled, StdlibStage::TestExtension)
        );
        let std_paths = loaded.module_stages[1]
            .iter()
            .map(|module| module.module_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(std_paths.last().copied(), Some("Test"));
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
        assert_eq!(
            loaded.module_stages[0].len(),
            stdlib_stage_len(StdlibVariant::Default, StdlibStage::Bootstrap)
        );
        assert_eq!(
            loaded.module_stages[1].len(),
            stdlib_stage_len(StdlibVariant::Default, StdlibStage::Main)
        );
        assert_eq!(loaded.module_stages[2].len(), 1);
        assert_eq!(loaded.module_stages[3].len(), 2);
        assert_eq!(
            loaded.module_stages[0][0].source_id,
            loaded.builtin_source_id
        );
        assert_eq!(
            loaded.module_stages[0][0].source_kind,
            SourceKind::StdDefinitionSource
        );
        assert_eq!(
            loaded.module_stages[1][0].source_kind,
            SourceKind::StdDefinitionSource
        );
        assert_eq!(
            loaded.module_stages[1][1].source_kind,
            SourceKind::StdDefinitionSource
        );
        assert_eq!(
            loaded.module_stages[2][0].source_kind,
            SourceKind::DefinitionSource
        );
        assert_eq!(
            loaded.module_stages[3][0].source_kind,
            SourceKind::DefinitionSource
        );
        assert_eq!(
            loaded.module_stages[3][1].source_kind,
            SourceKind::DefinitionSource
        );
        let std_paths = loaded.module_stages[1]
            .iter()
            .map(|module| module.module_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            std_paths,
            vec![
                "SpecialTypes",
                "Function",
                "Kernel",
                "Add",
                "Sub",
                "Mul",
                "Div",
                "Mod",
                "Eq",
                "Compare",
                "Concat",
                "Show",
                "Default",
                "Ordering",
                "Tuple",
                "Convert",
                "TryConvert",
                "Encode",
                "Decode",
                "Functor",
                "Bifunctor",
                "Applicative",
                "Monad",
                "MonadFail",
                "MonadT",
                "Identity",
                "Reader",
                "State",
                "Alternative",
                "Monoid",
                "Int",
                "String",
                "Regex",
                "Boolean",
                "Error",
                "List",
                "Generator",
                "InfiniteGenerator",
                "HashMap",
                "Result",
                "Extractor",
                "Either",
                "Duration",
                "Range",
                "Option",
                "OptionT",
                "EitherT",
                "ResultT",
                "ReaderT",
                "StateT",
                "Task",
                "Facet",
                "Float",
                "Json",
                "Config",
                "Project",
                "Random",
                "File",
                "FS",
                "IO",
                "Shell",
                "StyledDoc",
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
    fn derive_primary_module_path_reads_module_definition() {
        let source = r#"defmod Math {
  def add(x: Int, y: Int) -> Int { x + y }
}"#;
        assert_eq!(
            derive_primary_module_path(source).as_deref(),
            Some("Global::Math")
        );
    }

    #[test]
    fn derive_primary_module_path_reads_qualified_module_definition() {
        let source = r#"defmod Auth::Math {
  def add(x: Int, y: Int) -> Int { x + y }
}"#;
        assert_eq!(
            derive_primary_module_path(source).as_deref(),
            Some("Auth::Math")
        );
    }

    #[test]
    fn derive_primary_module_path_reads_namespace_lowered_module_definition() {
        let source = r#"namespace Auth {
  defmod Math {
    def add(x: Int, y: Int) -> Int { x + y }
  }
}"#;
        assert_eq!(
            derive_primary_module_path(source).as_deref(),
            Some("Auth::Math")
        );
    }

    #[test]
    fn derive_primary_module_path_ignores_comments_and_blank_lines() {
        let source = r#"
# leading comment

defmod Math {
  # inside comment
  def add(x: Int, y: Int) -> Int { x + y }
}
"#;
        assert_eq!(
            derive_primary_module_path(source).as_deref(),
            Some("Global::Math")
        );
    }
}
