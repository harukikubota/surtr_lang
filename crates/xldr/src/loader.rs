use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use diagnostics::{SourceId, SourceRegistry};
use serde::{Deserialize, Serialize};
use sindr::policy::SourceKind;
use spire::ast::{Ast, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StdlibVariant {
    Default,
    TestEnabled,
}

const BUILTIN_PRELUDE_FILE: &str = "bootstrap.srt";
const BUILTIN_PRELUDE_MODULE_PATH: &str = "Bootstrap";
const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/bootstrap.srt");
const SPECIAL_TYPES_FILE: &str = "types/special_types.srt";
const SPECIAL_TYPES_MODULE_PATH: &str = "SpecialTypes";
const SPECIAL_TYPES_SOURCE: &str = include_str!("../../../lib/types/special_types.srt");
const FUNCTION_PRELUDE_FILE: &str = "function.srt";
const FUNCTION_PRELUDE_MODULE_PATH: &str = "Function";
const FUNCTION_PRELUDE_SOURCE: &str = include_str!("../../../lib/function.srt");
const KERNEL_PRELUDE_FILE: &str = "kernel.srt";
const KERNEL_PRELUDE_MODULE_PATH: &str = "Kernel";
const KERNEL_PRELUDE_SOURCE: &str = include_str!("../../../lib/kernel.srt");
const STYLED_DOC_FILE: &str = "styled_doc.srt";
const STYLED_DOC_MODULE_PATH: &str = "StyledDoc";
const STYLED_DOC_SOURCE: &str = include_str!("../../../lib/styled_doc.srt");
const TEST_STD_FILE: &str = "test.srt";
const TEST_STD_MODULE_PATH: &str = "Test";
const TEST_STD_SOURCE: &str = include_str!("../../../lib/test.srt");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StdlibStage {
    Bootstrap,
    Main,
    TestExtension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StdlibModuleSpec {
    pub file_name: &'static str,
    pub module_path: &'static str,
    pub source: &'static str,
    pub stage: StdlibStage,
    pub variant: StdlibVariant,
}

const STDLIB_MODULE_SPECS: &[StdlibModuleSpec] = &[
    StdlibModuleSpec {
        file_name: BUILTIN_PRELUDE_FILE,
        module_path: BUILTIN_PRELUDE_MODULE_PATH,
        source: BUILTIN_PRELUDE_SOURCE,
        stage: StdlibStage::Bootstrap,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: SPECIAL_TYPES_FILE,
        module_path: SPECIAL_TYPES_MODULE_PATH,
        source: SPECIAL_TYPES_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: FUNCTION_PRELUDE_FILE,
        module_path: FUNCTION_PRELUDE_MODULE_PATH,
        source: FUNCTION_PRELUDE_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: KERNEL_PRELUDE_FILE,
        module_path: KERNEL_PRELUDE_MODULE_PATH,
        source: KERNEL_PRELUDE_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/add.srt",
        module_path: "Add",
        source: include_str!("../../../lib/traits/operator/add.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/sub.srt",
        module_path: "Sub",
        source: include_str!("../../../lib/traits/operator/sub.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/mul.srt",
        module_path: "Mul",
        source: include_str!("../../../lib/traits/operator/mul.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/eq.srt",
        module_path: "Eq",
        source: include_str!("../../../lib/traits/operator/eq.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/compare.srt",
        module_path: "Compare",
        source: include_str!("../../../lib/traits/operator/compare.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/concat.srt",
        module_path: "Concat",
        source: include_str!("../../../lib/traits/operator/concat.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/show.srt",
        module_path: "Show",
        source: include_str!("../../../lib/traits/show.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/default.srt",
        module_path: "Default",
        source: include_str!("../../../lib/traits/default.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/ordering.srt",
        module_path: "Ordering",
        source: include_str!("../../../lib/types/ordering.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/tuple.srt",
        module_path: "Tuple",
        source: include_str!("../../../lib/types/tuple.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/from.srt",
        module_path: "From",
        source: include_str!("../../../lib/traits/from.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/try_from.srt",
        module_path: "TryFrom",
        source: include_str!("../../../lib/traits/try_from.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/encode.srt",
        module_path: "Encode",
        source: include_str!("../../../lib/traits/encode.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/decode.srt",
        module_path: "Decode",
        source: include_str!("../../../lib/traits/decode.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/functor.srt",
        module_path: "Functor",
        source: include_str!("../../../lib/traits/operator/functor.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/applicative.srt",
        module_path: "Applicative",
        source: include_str!("../../../lib/traits/operator/applicative.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/monad.srt",
        module_path: "Monad",
        source: include_str!("../../../lib/traits/operator/monad.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/alternative.srt",
        module_path: "Alternative",
        source: include_str!("../../../lib/traits/operator/alternative.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/monoid.srt",
        module_path: "Monoid",
        source: include_str!("../../../lib/types/monoid.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/pipe_apply.srt",
        module_path: "PipeApply",
        source: include_str!("../../../lib/traits/operator/pipe_apply.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/compose.srt",
        module_path: "Compose",
        source: include_str!("../../../lib/traits/operator/compose.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/composable.srt",
        module_path: "Composable",
        source: include_str!("../../../lib/traits/operator/composable.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/lift_composable.srt",
        module_path: "LiftComposable",
        source: include_str!("../../../lib/traits/operator/lift_composable.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/kleisli_composable.srt",
        module_path: "KleisliComposable",
        source: include_str!("../../../lib/traits/operator/kleisli_composable.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/int.srt",
        module_path: "Int",
        source: include_str!("../../../lib/types/int.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/string.srt",
        module_path: "String",
        source: include_str!("../../../lib/types/string.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/regex.srt",
        module_path: "Regex",
        source: include_str!("../../../lib/types/regex.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/boolean.srt",
        module_path: "Boolean",
        source: include_str!("../../../lib/types/boolean.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/error.srt",
        module_path: "Error",
        source: include_str!("../../../lib/types/error.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/list.srt",
        module_path: "List",
        source: include_str!("../../../lib/types/list.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/generator.srt",
        module_path: "Generator",
        source: include_str!("../../../lib/types/generator.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/hash_map.srt",
        module_path: "HashMap",
        source: include_str!("../../../lib/types/hash_map.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/result.srt",
        module_path: "Result",
        source: include_str!("../../../lib/types/result.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/duration.srt",
        module_path: "Duration",
        source: include_str!("../../../lib/types/duration.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/range.srt",
        module_path: "Range",
        source: include_str!("../../../lib/types/range.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/option.srt",
        module_path: "Option",
        source: include_str!("../../../lib/types/option.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "process.srt",
        module_path: "Task",
        source: include_str!("../../../lib/process.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "facet.srt",
        module_path: "Facet",
        source: include_str!("../../../lib/facet.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/float.srt",
        module_path: "Float",
        source: include_str!("../../../lib/types/float.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/json.srt",
        module_path: "Json",
        source: include_str!("../../../lib/types/json.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "Config.srt",
        module_path: "Config",
        source: include_str!("../../../lib/Config.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "Project.srt",
        module_path: "Project",
        source: include_str!("../../../lib/Project.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "Random.srt",
        module_path: "Random",
        source: include_str!("../../../lib/Random.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "file.srt",
        module_path: "File",
        source: include_str!("../../../lib/file.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "FileSystem.srt",
        module_path: "FS",
        source: include_str!("../../../lib/FileSystem.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "IO.srt",
        module_path: "IO",
        source: include_str!("../../../lib/IO.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "Shell.srt",
        module_path: "Shell",
        source: include_str!("../../../lib/Shell.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: STYLED_DOC_FILE,
        module_path: STYLED_DOC_MODULE_PATH,
        source: STYLED_DOC_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: TEST_STD_FILE,
        module_path: TEST_STD_MODULE_PATH,
        source: TEST_STD_SOURCE,
        stage: StdlibStage::TestExtension,
        variant: StdlibVariant::TestEnabled,
    },
];

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
    Parse { message: String, span: Span },
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
    .map_err(|e| ScriptSourcePrepareError::Parse {
        message: e.message().to_string(),
        span: e.span().clone(),
    })?;

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

fn collect_lib_module_files(
    dir: &Path,
    files: &mut Vec<std::path::PathBuf>,
) -> Result<(), LoadError> {
    if lib_relative_path(dir) == "tests" {
        return Ok(());
    }

    let entries = fs::read_dir(dir).map_err(|e| LoadError::SourceReadFailed {
        file_name: display_path(dir),
        message: e.to_string(),
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| LoadError::SourceReadFailed {
            file_name: display_path(dir),
            message: e.to_string(),
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_lib_module_files(&path, files)?;
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "srt")
        {
            files.push(path);
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

pub(crate) fn stdlib_module_specs(
    stdlib_variant: StdlibVariant,
) -> impl Iterator<Item = &'static StdlibModuleSpec> {
    STDLIB_MODULE_SPECS.iter().filter(move |spec| {
        spec.variant == StdlibVariant::Default || stdlib_variant == StdlibVariant::TestEnabled
    })
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
                "Eq",
                "Compare",
                "Concat",
                "Show",
                "Default",
                "Ordering",
                "Tuple",
                "From",
                "TryFrom",
                "Encode",
                "Decode",
                "Functor",
                "Applicative",
                "Monad",
                "PipeApply",
                "Compose",
                "Composable",
                "LiftComposable",
                "KleisliComposable",
                "Int",
                "String",
                "Regex",
                "Boolean",
                "Error",
                "List",
                "Generator",
                "HashMap",
                "Result",
                "Duration",
                "Range",
                "Option",
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
                "Eq",
                "Compare",
                "Concat",
                "Show",
                "Default",
                "Ordering",
                "Tuple",
                "From",
                "TryFrom",
                "Encode",
                "Decode",
                "Functor",
                "Applicative",
                "Monad",
                "PipeApply",
                "Compose",
                "Composable",
                "LiftComposable",
                "KleisliComposable",
                "Int",
                "String",
                "Regex",
                "Boolean",
                "Error",
                "List",
                "Generator",
                "HashMap",
                "Result",
                "Duration",
                "Range",
                "Option",
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
