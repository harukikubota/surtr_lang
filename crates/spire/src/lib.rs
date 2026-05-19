pub mod ast;
pub mod error;
mod func_literal;
mod lexer;
mod parser;
mod token;

// Re-export the main entry point
pub use parser::{
    parse, parse_incomplete_expr, parse_incomplete_stmt, parse_operator_completion_context,
    parse_tolerant_with_context, parse_with_context, parse_with_context_diagnostic,
    rebase_ast_spans, CompletionContext, CursorSyntaxContext, IncompleteParseResult, LspDiagnostic,
    LspDiagnosticSeverity, LspPosition, LspRange, LspRelatedInformation, OperatorCompletionContext,
    OperatorCompletionStage, ParseDiagnostic, ParseRules, ParserContext, SyntaxOutlineItem,
    SyntaxOutlineKind, SyntaxToken, SyntaxTokenKind, TolerantParseResult,
};

pub fn parse_rules_for_source_policy(policy: &sindr::policy::SourcePolicy) -> ParseRules {
    match policy.parse_profile {
        sindr::policy::ParseProfile::Script => ParseRules::script(),
        sindr::policy::ParseProfile::Module => ParseRules::module(),
        sindr::policy::ParseProfile::StdModule => ParseRules::std_module(),
        sindr::policy::ParseProfile::Project => ParseRules::project(),
        sindr::policy::ParseProfile::ReplChunk => ParseRules::repl_chunk(),
    }
}

pub fn parse_rules_for_source_kind(source_kind: sindr::policy::SourceKind) -> ParseRules {
    let policy = source_kind.policy(sindr::policy::CompileUnitKind::DefinitionCheck, None);
    parse_rules_for_source_policy(&policy)
}

pub fn parser_context_for_source_policy(
    source_id: u32,
    policy: sindr::policy::SourcePolicy,
    module_path: Option<String>,
) -> ParserContext {
    match policy.parser_context {
        sindr::policy::ParserContextKind::Project => ParserContext::project(source_id),
        sindr::policy::ParserContextKind::Script => ParserContext::script(source_id),
        sindr::policy::ParserContextKind::Repl => ParserContext::repl(source_id),
        sindr::policy::ParserContextKind::Module => ParserContext::module(source_id, module_path)
            .with_rules(parse_rules_for_source_policy(&policy)),
    }
}

pub fn parser_context_for_source_kind(
    source_id: u32,
    source_kind: sindr::policy::SourceKind,
    compile_unit_kind: sindr::policy::CompileUnitKind,
    module_path: Option<String>,
) -> ParserContext {
    parser_context_for_source_policy(
        source_id,
        source_kind.policy(compile_unit_kind, None),
        module_path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sindr::policy::{CompileUnitKind, SourceKind};

    #[test]
    fn parser_context_for_source_kind_uses_std_builtin_policy() {
        let std_context = parser_context_for_source_kind(
            0,
            SourceKind::StdDefinitionSource,
            CompileUnitKind::DefinitionCheck,
            None,
        );
        let module_context = parser_context_for_source_kind(
            0,
            SourceKind::DefinitionSource,
            CompileUnitKind::DefinitionCheck,
            None,
        );

        let source = "@builtin def print(value: String) -> Unit";
        assert!(parse_with_context(source, std_context).is_ok());
        assert!(parse_with_context(source, module_context).is_err());
    }

    #[test]
    fn parser_context_for_source_policy_uses_policy_profile() {
        let project_policy =
            SourceKind::ProjectConfigSource.policy(CompileUnitKind::DefinitionCheck, None);
        let module_policy =
            SourceKind::DefinitionSource.policy(CompileUnitKind::DefinitionCheck, None);

        let project_context = parser_context_for_source_policy(0, project_policy, None);
        let module_context = parser_context_for_source_policy(0, module_policy, None);

        assert!(parse_with_context("1", project_context).is_ok());
        assert!(parse_with_context("1", module_context).is_err());
    }
}
