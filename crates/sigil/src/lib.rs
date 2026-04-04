pub mod error;
pub mod resolved;
pub mod resolver;
pub mod scope;

pub use resolver::{
    precollect_declaration_index, resolve, DeclarationEntry, DeclarationIndex, DeclarationKind,
    SigilCheckpoint, SigilSession, StagedModuleAst,
};
