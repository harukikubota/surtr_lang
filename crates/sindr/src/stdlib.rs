//! Canonical standard source inventory shared by compilation and analysis.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StdlibVariant {
    Default,
    TestEnabled,
}

pub const BUILTIN_PRELUDE_FILE: &str = "bootstrap.srt";
pub const BUILTIN_PRELUDE_MODULE_PATH: &str = "Bootstrap";
const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../../lib/bootstrap.srt");
const SPECIAL_TYPES_FILE: &str = "types/special_types.srt";
const SPECIAL_TYPES_SOURCE: &str = include_str!("../../../lib/types/special_types.srt");
const FUNCTION_PRELUDE_FILE: &str = "function.srt";
const FUNCTION_PRELUDE_MODULE_PATH: &str = "Function";
const FUNCTION_PRELUDE_SOURCE: &str = include_str!("../../../lib/function.srt");
const KERNEL_PRELUDE_FILE: &str = "kernel.srt";
const KERNEL_PRELUDE_MODULE_PATH: &str = "Kernel";
const KERNEL_PRELUDE_SOURCE: &str = include_str!("../../../lib/kernel.srt");
const IDENTITY_FILE: &str = "types/identity.srt";
const IDENTITY_MODULE_PATH: &str = "Identity";
const IDENTITY_SOURCE: &str = include_str!("../../../lib/types/identity.srt");
const READER_FILE: &str = "types/reader.srt";
const READER_MODULE_PATH: &str = "Reader";
const READER_SOURCE: &str = include_str!("../../../lib/types/reader.srt");
const STATE_FILE: &str = "types/state.srt";
const STATE_MODULE_PATH: &str = "State";
const STATE_SOURCE: &str = include_str!("../../../lib/types/state.srt");
const OPTION_T_FILE: &str = "types/monad_transformer/option_t.srt";
const OPTION_T_MODULE_PATH: &str = "OptionT";
const OPTION_T_SOURCE: &str = include_str!("../../../lib/types/monad_transformer/option_t.srt");
const EITHER_T_FILE: &str = "types/monad_transformer/either_t.srt";
const EITHER_T_MODULE_PATH: &str = "EitherT";
const EITHER_T_SOURCE: &str = include_str!("../../../lib/types/monad_transformer/either_t.srt");
const READER_T_FILE: &str = "types/monad_transformer/reader_t.srt";
const READER_T_MODULE_PATH: &str = "ReaderT";
const READER_T_SOURCE: &str = include_str!("../../../lib/types/monad_transformer/reader_t.srt");
const STATE_T_FILE: &str = "types/monad_transformer/state_t.srt";
const STATE_T_MODULE_PATH: &str = "StateT";
const STATE_T_SOURCE: &str = include_str!("../../../lib/types/monad_transformer/state_t.srt");
const STYLED_DOC_FILE: &str = "styled_doc.srt";
const STYLED_DOC_MODULE_PATH: &str = "StyledDoc";
const STYLED_DOC_SOURCE: &str = include_str!("../../../lib/styled_doc.srt");
const TEST_STD_FILE: &str = "test.srt";
pub const TEST_STD_MODULE_PATH: &str = "Test";
const TEST_STD_SOURCE: &str = include_str!("../../../lib/test.srt");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdlibStage {
    Bootstrap,
    Main,
    TestExtension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StdlibModuleSpec {
    pub file_name: &'static str,
    pub module_path: Option<&'static str>,
    pub source: &'static str,
    pub stage: StdlibStage,
    pub variant: StdlibVariant,
}

/// Canonical standard-library module inventory, stage assignment, and order.
///
/// `Bootstrap` forms compile stage 0. `Main` and `TestExtension` are inventory
/// classes collected into the shared standard compile stage 1; the variant
/// controls whether test extensions are present. Documentation must not
/// maintain a second exhaustive ordered module list.
pub const STDLIB_MODULE_SPECS: &[StdlibModuleSpec] = &[
    StdlibModuleSpec {
        file_name: BUILTIN_PRELUDE_FILE,
        module_path: Some(BUILTIN_PRELUDE_MODULE_PATH),
        source: BUILTIN_PRELUDE_SOURCE,
        stage: StdlibStage::Bootstrap,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: SPECIAL_TYPES_FILE,
        module_path: None,
        source: SPECIAL_TYPES_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: FUNCTION_PRELUDE_FILE,
        module_path: Some(FUNCTION_PRELUDE_MODULE_PATH),
        source: FUNCTION_PRELUDE_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: KERNEL_PRELUDE_FILE,
        module_path: Some(KERNEL_PRELUDE_MODULE_PATH),
        source: KERNEL_PRELUDE_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/add.srt",
        module_path: Some("Add"),
        source: include_str!("../../../lib/traits/operator/add.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/sub.srt",
        module_path: Some("Sub"),
        source: include_str!("../../../lib/traits/operator/sub.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/mul.srt",
        module_path: Some("Mul"),
        source: include_str!("../../../lib/traits/operator/mul.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/div.srt",
        module_path: Some("Div"),
        source: include_str!("../../../lib/traits/operator/div.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/mod.srt",
        module_path: Some("Mod"),
        source: include_str!("../../../lib/traits/operator/mod.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/eq.srt",
        module_path: Some("Eq"),
        source: include_str!("../../../lib/traits/operator/eq.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/compare.srt",
        module_path: Some("Compare"),
        source: include_str!("../../../lib/traits/operator/compare.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/concat.srt",
        module_path: Some("Concat"),
        source: include_str!("../../../lib/traits/operator/concat.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/show.srt",
        module_path: Some("Show"),
        source: include_str!("../../../lib/traits/show.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/default.srt",
        module_path: Some("Default"),
        source: include_str!("../../../lib/traits/default.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/ordering.srt",
        module_path: Some("Ordering"),
        source: include_str!("../../../lib/types/ordering.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/tuple.srt",
        module_path: Some("Tuple"),
        source: include_str!("../../../lib/types/tuple.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/convert.srt",
        module_path: Some("Convert"),
        source: include_str!("../../../lib/traits/convert.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/try_convert.srt",
        module_path: Some("TryConvert"),
        source: include_str!("../../../lib/traits/try_convert.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/encode.srt",
        module_path: Some("Encode"),
        source: include_str!("../../../lib/traits/encode.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/decode.srt",
        module_path: Some("Decode"),
        source: include_str!("../../../lib/traits/decode.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/functor.srt",
        module_path: Some("Functor"),
        source: include_str!("../../../lib/traits/operator/functor.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/bifunctor.srt",
        module_path: Some("Bifunctor"),
        source: include_str!("../../../lib/traits/bifunctor.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/applicative.srt",
        module_path: Some("Applicative"),
        source: include_str!("../../../lib/traits/operator/applicative.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/monad.srt",
        module_path: Some("Monad"),
        source: include_str!("../../../lib/traits/operator/monad.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/monad_fail.srt",
        module_path: Some("MonadFail"),
        source: include_str!("../../../lib/traits/monad_fail.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/monad_t.srt",
        module_path: Some("MonadT"),
        source: include_str!("../../../lib/traits/monad_t.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: IDENTITY_FILE,
        module_path: Some(IDENTITY_MODULE_PATH),
        source: IDENTITY_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: READER_FILE,
        module_path: Some(READER_MODULE_PATH),
        source: READER_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: STATE_FILE,
        module_path: Some(STATE_MODULE_PATH),
        source: STATE_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "traits/operator/alternative.srt",
        module_path: Some("Alternative"),
        source: include_str!("../../../lib/traits/operator/alternative.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/monoid.srt",
        module_path: Some("Monoid"),
        source: include_str!("../../../lib/types/monoid.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/int.srt",
        module_path: Some("Int"),
        source: include_str!("../../../lib/types/int.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/string.srt",
        module_path: Some("String"),
        source: include_str!("../../../lib/types/string.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/regex.srt",
        module_path: Some("Regex"),
        source: include_str!("../../../lib/types/regex.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/boolean.srt",
        module_path: Some("Boolean"),
        source: include_str!("../../../lib/types/boolean.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/error.srt",
        module_path: Some("Error"),
        source: include_str!("../../../lib/types/error.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/list.srt",
        module_path: Some("List"),
        source: include_str!("../../../lib/types/list.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/generator.srt",
        module_path: Some("Generator"),
        source: include_str!("../../../lib/types/generator.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/infinite_generator.srt",
        module_path: Some("InfiniteGenerator"),
        source: include_str!("../../../lib/types/infinite_generator.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/hash_map.srt",
        module_path: Some("HashMap"),
        source: include_str!("../../../lib/types/hash_map.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/result.srt",
        module_path: Some("Result"),
        source: include_str!("../../../lib/types/result.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "extractor.srt",
        module_path: Some("Extractor"),
        source: include_str!("../../../lib/extractor.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/either.srt",
        module_path: Some("Either"),
        source: include_str!("../../../lib/types/either.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/duration.srt",
        module_path: Some("Duration"),
        source: include_str!("../../../lib/types/duration.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/range.srt",
        module_path: Some("Range"),
        source: include_str!("../../../lib/types/range.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/option.srt",
        module_path: Some("Option"),
        source: include_str!("../../../lib/types/option.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: OPTION_T_FILE,
        module_path: Some(OPTION_T_MODULE_PATH),
        source: OPTION_T_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: EITHER_T_FILE,
        module_path: Some(EITHER_T_MODULE_PATH),
        source: EITHER_T_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/monad_transformer/result_t.srt",
        module_path: Some("ResultT"),
        source: include_str!("../../../lib/types/monad_transformer/result_t.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: READER_T_FILE,
        module_path: Some(READER_T_MODULE_PATH),
        source: READER_T_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: STATE_T_FILE,
        module_path: Some(STATE_T_MODULE_PATH),
        source: STATE_T_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "process.srt",
        module_path: Some("Task"),
        source: include_str!("../../../lib/process.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "facet.srt",
        module_path: Some("Facet"),
        source: include_str!("../../../lib/facet.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/float.srt",
        module_path: Some("Float"),
        source: include_str!("../../../lib/types/float.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "types/json.srt",
        module_path: Some("Json"),
        source: include_str!("../../../lib/types/json.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "Config.srt",
        module_path: Some("Config"),
        source: include_str!("../../../lib/Config.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "Project.srt",
        module_path: Some("Project"),
        source: include_str!("../../../lib/Project.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "Random.srt",
        module_path: Some("Random"),
        source: include_str!("../../../lib/Random.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "file.srt",
        module_path: Some("File"),
        source: include_str!("../../../lib/file.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "FileSystem.srt",
        module_path: Some("FS"),
        source: include_str!("../../../lib/FileSystem.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "IO.srt",
        module_path: Some("IO"),
        source: include_str!("../../../lib/IO.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: "Shell.srt",
        module_path: Some("Shell"),
        source: include_str!("../../../lib/Shell.srt"),
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: STYLED_DOC_FILE,
        module_path: Some(STYLED_DOC_MODULE_PATH),
        source: STYLED_DOC_SOURCE,
        stage: StdlibStage::Main,
        variant: StdlibVariant::Default,
    },
    StdlibModuleSpec {
        file_name: TEST_STD_FILE,
        module_path: Some(TEST_STD_MODULE_PATH),
        source: TEST_STD_SOURCE,
        stage: StdlibStage::TestExtension,
        variant: StdlibVariant::TestEnabled,
    },
];

pub fn stdlib_module_specs(
    stdlib_variant: StdlibVariant,
) -> impl Iterator<Item = &'static StdlibModuleSpec> {
    STDLIB_MODULE_SPECS.iter().filter(move |spec| {
        spec.variant == StdlibVariant::Default || stdlib_variant == StdlibVariant::TestEnabled
    })
}
