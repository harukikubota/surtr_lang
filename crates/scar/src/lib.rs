pub mod checker;
pub mod env;
pub mod error;
pub mod typed;
pub mod types;

pub use checker::{
    type_contains_unresolved_vars, typecheck, typecheck_staged_program,
    typecheck_staged_program_with_context, typecheck_with_context, ScarCheckpoint, ScarSession,
    TypecheckContext,
};
