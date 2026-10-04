pub mod checker;
pub mod env;
pub mod error;
pub mod typed;
pub mod types;

pub use checker::{
    type_contains_unresolved_vars, typecheck, typecheck_staged_program,
    typecheck_staged_program_with_context, typecheck_staged_program_with_context_with_warnings,
    typecheck_staged_program_with_warnings, typecheck_with_context,
    typecheck_with_context_with_warnings, typecheck_with_warnings, ScarCheckpoint, ScarSession,
    TypecheckContext,
};

#[cfg(test)]
extern crate self as scar;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../tests/support/mod.rs"]
mod test_support;
