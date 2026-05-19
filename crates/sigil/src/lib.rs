pub mod error;
pub mod resolved;
pub mod resolver;
pub mod scope;
pub mod semantic_metadata;

pub use resolver::{
    build_scope_for_module, declaration_stage_ordering, declaration_symbol_identity_info,
    declaration_uid_order, effective_auto_import_entries, effective_visible_entries,
    lower_module_source_ast, lowered_module_is_impl_owner, precollect_declaration_index, resolve,
    resolve_staged_program, resolve_staged_program_from_state,
    resolve_staged_program_from_state_with_warnings, resolve_staged_program_with_state,
    resolve_staged_program_with_state_with_warnings, resolve_staged_program_with_warnings,
    resolve_with_warnings, user_type_symbol_identity_info, DeclarationEntry, DeclarationIndex,
    DeclarationKind, DeclarationOrdering, EffectiveVisibleEntry, LoweredModuleAst,
    ResolveResumeState, ResolvedStagedProgram, SigilCheckpoint, SigilSession,
    StageOrderedDeclaration, StagedModuleAst,
};
pub use semantic_metadata::{
    collect_doc_entries, collect_doc_entries_with_base, collect_signature_entries,
    collect_signature_entries_with_base,
};
