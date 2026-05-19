pub mod error;
pub mod resolved;
pub mod resolver;
pub mod scope;

pub use resolver::{
    build_scope_for_module, precollect_declaration_index, resolve, resolve_staged_program,
    resolve_staged_program_from_state, resolve_staged_program_from_state_with_warnings,
    resolve_staged_program_with_state, resolve_staged_program_with_state_with_warnings,
    resolve_staged_program_with_warnings, resolve_with_warnings,
    declaration_symbol_identity_info, user_type_symbol_identity_info, DeclarationEntry,
    DeclarationIndex, DeclarationKind, ResolveResumeState, ResolvedStagedProgram,
    SigilCheckpoint, SigilSession, StagedModuleAst,
};
