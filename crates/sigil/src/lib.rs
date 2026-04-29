pub mod error;
pub mod resolved;
pub mod resolver;
pub mod scope;

pub use resolver::{
    build_scope_for_module, precollect_declaration_index, resolve, resolve_staged_program,
    resolve_staged_program_from_state, resolve_staged_program_with_state, DeclarationEntry,
    DeclarationIndex, DeclarationKind, ResolveResumeState, ResolvedStagedProgram, SigilCheckpoint,
    SigilSession, StagedModuleAst,
};
