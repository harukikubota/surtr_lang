pub mod ast;
pub mod error;
mod lexer;
mod parser;
mod token;

// Re-export the main entry point
pub use parser::{
    collect_entrypoint_annotations, parse, parse_with_context, strip_test_annotations,
    EntryAnnotation, ParseRules, ParserContext,
};
