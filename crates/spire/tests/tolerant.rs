use spire::ast::Ast;
use spire::{
    parse_tolerant_with_context, CursorSyntaxContext, ParserContext, SyntaxOutlineKind,
    SyntaxTokenKind,
};

fn text_for_span(source: &str, span: &spire::ast::Span) -> String {
    source
        .chars()
        .skip(span.start)
        .take(span.end.saturating_sub(span.start))
        .collect()
}

fn bind_name(node: &Ast) -> Option<&str> {
    match node {
        Ast::Bind(_, spire::ast::AstPattern::Var(_, name), _) => Some(name.as_str()),
        _ => None,
    }
}

#[test]
fn tolerant_tokens_include_comments_newlines_and_path_separators() {
    let source = "# module docs\nimport Kernel::print\nvalue = 1 >> to_string";

    let result = parse_tolerant_with_context(source, ParserContext::script(0), None);

    assert!(result
        .tokens
        .iter()
        .any(|token| token.kind == SyntaxTokenKind::Comment
            && text_for_span(source, &token.span) == "# module docs"));
    assert!(result
        .tokens
        .iter()
        .any(|token| token.kind == SyntaxTokenKind::Newline));
    assert!(result
        .tokens
        .iter()
        .any(|token| token.kind == SyntaxTokenKind::PathSep
            && text_for_span(source, &token.span) == "::"));
    assert!(result
        .tokens
        .iter()
        .any(|token| token.kind == SyntaxTokenKind::Compose
            && text_for_span(source, &token.span) == ">>"));
    assert!(!result
        .tokens
        .iter()
        .any(|token| text_for_span(source, &token.span) == " "));
}

#[test]
fn tolerant_token_spans_use_character_offsets() {
    let source = "名前 = 1 # コメント\nnext = 2";

    let result = parse_tolerant_with_context(source, ParserContext::script(0), None);

    let comment = result
        .tokens
        .iter()
        .find(|token| token.kind == SyntaxTokenKind::Comment)
        .expect("comment token should be retained");
    assert_eq!(comment.span.start, "名前 = 1 ".chars().count());
    assert_eq!(text_for_span(source, &comment.span), "# コメント");
}

#[test]
fn tolerant_parse_recovers_after_broken_top_level_statement() {
    let source = "ok = 1\nbad = )\nnext = 2";

    let result = parse_tolerant_with_context(source, ParserContext::script(0), None);

    assert!(!result.diagnostics.is_empty());
    let names = result.ast.iter().filter_map(bind_name).collect::<Vec<_>>();
    assert_eq!(names, vec!["ok", "next"]);
}

#[test]
fn tolerant_parse_recovers_inside_defmod_body() {
    let source = r#"defmod M {
  def ok() -> Int { 1 }
  bad = )
  def next() -> Int { 2 }
}"#;

    let result = parse_tolerant_with_context(source, ParserContext::module(0, None), None);

    assert!(!result.diagnostics.is_empty());
    let module = result
        .ast
        .iter()
        .find_map(|node| match node {
            Ast::Defmod(_, name, body, _) if name == "M" || name == "Global::M" => Some(body),
            _ => None,
        })
        .expect("defmod outline should recover valid body declarations");
    let methods = module
        .iter()
        .filter_map(|node| match node {
            Ast::Def(_, name, ..) => Some(name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(methods, vec!["ok", "next"]);
}

#[test]
fn tolerant_parse_reports_unterminated_string_and_keeps_following_outline() {
    let source = "bad = \"unterminated\n\ndef next() -> Int { 2 }";

    let result = parse_tolerant_with_context(source, ParserContext::script(0), None);

    assert!(result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.error.message().contains("expected \"")));
    assert!(result.outline.iter().any(|item| {
        item.kind == SyntaxOutlineKind::Function && item.name.as_deref() == Some("next")
    }));
}

#[test]
fn tolerant_parse_reports_cursor_context_inside_import_path() {
    let source = "import Kernel::";
    let cursor = source.chars().count();

    let result = parse_tolerant_with_context(source, ParserContext::script(0), Some(cursor));

    assert_eq!(result.cursor_context, CursorSyntaxContext::ImportPath);
    assert!(!result.diagnostics.is_empty());
}
