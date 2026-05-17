pub mod ast;
pub mod error;
mod func_literal;
mod lexer;
mod parser;
mod token;

// Re-export the main entry point
pub use parser::{
    parse, parse_incomplete_expr, parse_incomplete_stmt, parse_with_context,
    parse_with_context_diagnostic, rebase_ast_spans, CompletionContext, IncompleteParseResult,
    LspDiagnostic, LspDiagnosticSeverity, LspPosition, LspRange, LspRelatedInformation,
    ParseDiagnostic, ParseRules, ParserContext,
};

pub fn parse_rules_for_source_kind(source_kind: sindr::policy::SourceKind) -> ParseRules {
    match source_kind {
        sindr::policy::SourceKind::Script => ParseRules::script(),
        sindr::policy::SourceKind::DefinitionSource => ParseRules::module(),
        sindr::policy::SourceKind::StdDefinitionSource => ParseRules::std_module(),
        sindr::policy::SourceKind::ProjectConfigSource => ParseRules::project(),
        sindr::policy::SourceKind::ReplChunk => ParseRules::repl_chunk(),
    }
}
