use sindr::policy::CompileUnitKind;

use crate::SourceKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreloadCompileMode {
    pub(crate) compile_unit_kind: CompileUnitKind,
    pub(crate) runtime_source_kind: SourceKind,
}

impl PreloadCompileMode {
    pub(crate) const SCRIPT: Self = Self {
        compile_unit_kind: CompileUnitKind::Script,
        runtime_source_kind: SourceKind::Script,
    };

    pub(crate) const PROJECT: Self = Self {
        compile_unit_kind: CompileUnitKind::Project,
        runtime_source_kind: SourceKind::DefinitionSource,
    };
}
