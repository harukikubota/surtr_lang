pub mod ast;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod token;

// Re-export the main entry point
pub use parser::parse;
pub use parser::parse_with_source;
