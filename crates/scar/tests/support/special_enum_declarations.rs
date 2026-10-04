// Isolated phase tests supply the real standard enum declarations they use.
// Parse the user source in its own context so its spans and restrictions remain intact.
pub fn parse_with_canonical_special_enums(
    source: &str,
) -> Result<Vec<spire::ast::Ast>, spire::error::ParseError> {
    let user_ast = spire::parse_with_context(source, spire::ParserContext::project(0))?;
    let mut ast = spire::parse_with_context(
        "@builtin defenum Boolean { True, False }\n@builtin defenum Result<$T> { Ok($T), Err(Error) }",
        spire::ParserContext::module(1, None).with_rules(spire::ParseRules::std_module()),
    )?;
    ast.extend(user_ast);
    Ok(ast)
}
