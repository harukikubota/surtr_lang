use std::sync::Arc;

use forge::bytecode::Bytecode;

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
    pub(super) declaration_index: sigil::DeclarationIndex,
    pub(super) resolve_state: sigil::ResolveResumeState,
    pub(super) scar_checkpoint: scar::ScarCheckpoint,
    pub(super) bytecode: Bytecode,
}

pub(super) struct CachedPhaseSessions {
    pub(super) sigil_session: sigil::SigilSession,
    pub(super) scar_session: scar::ScarSession,
}

pub(super) type SharedCompilePrefix = Arc<CachedCompilePrefix>;
