pub mod checker;
pub mod env;
pub mod error;
pub mod typed;
pub mod types;

pub use checker::{
    typecheck, typecheck_staged_program, typecheck_with_context, ScarCheckpoint, ScarSession,
    TypecheckContext,
};

#[cfg(test)]
extern crate self as scar;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod typecheck_surface_tests;
