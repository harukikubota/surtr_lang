use std::sync::Arc;

use xldr::CompilationPrefixSnapshot;

#[derive(Clone, Copy)]
pub(super) enum TestCompileMode {
    Script,
    Project,
}

#[derive(Clone, Copy)]
pub(super) enum CompileFailurePhase {
    Parse,
    Resolve,
    Typecheck,
    Codegen,
}

impl CompileFailurePhase {
    pub(super) fn from_str(phase: &str) -> Result<Self, String> {
        match phase {
            "parse" => Ok(Self::Parse),
            "resolve" => Ok(Self::Resolve),
            "typecheck" => Ok(Self::Typecheck),
            "codegen" => Ok(Self::Codegen),
            other => Err(format!(
                "phase=test; message=unsupported compile-error phase `{}`",
                other
            )),
        }
    }
}

#[derive(Clone)]
pub(super) struct CachedModulePipeline {
    pub(super) module_asts: Vec<Vec<sigil::StagedModuleAst>>,
    pub(super) declaration_index: sigil::DeclarationIndex,
}

#[derive(Clone)]
pub(super) struct CachedCompilePrefix {
    pub(super) module_asts: Vec<Vec<sigil::StagedModuleAst>>,
    pub(super) compile_prefix: CompilationPrefixSnapshot,
}

impl CachedCompilePrefix {
    pub(super) fn declaration_index(&self) -> &sigil::DeclarationIndex {
        &self.compile_prefix.declaration_index
    }

    pub(super) fn resolve_state(&self) -> sigil::ResolveResumeState {
        self.compile_prefix.resolve_state
    }

    pub(super) fn scar_checkpoint(&self) -> &scar::ScarCheckpoint {
        &self.compile_prefix.scar_checkpoint
    }

    pub(super) fn bytecode(&self) -> &forge::bytecode::Bytecode {
        &self.compile_prefix.bytecode
    }
}

pub(super) type SharedCompilePrefix = Arc<CachedCompilePrefix>;
