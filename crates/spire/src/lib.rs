pub mod ast;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod token;

// Re-export the main entry point
pub use parser::{
    parse, parse_with_context, CompileUnitKind, DeclLevel, EntryPoint, ParserContext,
    SetExitCodePolicy, SourceRules, TopLevelDeclKind, TopLevelDeclPolicy,
};
