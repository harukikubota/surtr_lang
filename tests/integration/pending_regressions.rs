mod support;

use spire::{parse, parse_with_context, ParserContext, SourceRules};

#[test]
#[ignore = "pending: parser support for @@builtin + type declarations"]
fn pending_std_module_builtin_type_declaration_parses() {
    let source = r#"defmod Bootstrap {
  @@builtin
  type Int
}"#;
    let ast = parse_with_context(
        source,
        ParserContext::module(0, Some("Bootstrap".into())).with_rules(SourceRules::std_module()),
    )
    .expect("std module should accept builtin type declarations once implemented");
    assert_eq!(ast.len(), 1);
}

#[test]
#[ignore = "pending: SourceRules boundary for builtin type declarations"]
fn pending_user_module_builtin_type_declaration_is_rejected() {
    let source = r#"defmod UserExt {
  @@builtin
  type Int
}"#;
    let err = parse_with_context(
        source,
        ParserContext::module(0, Some("UserExt".into())).with_rules(SourceRules::module()),
    )
    .expect_err("user modules must not accept builtin type declarations");
    assert!(
        err.message().contains("not allowed") || err.message().contains("@@builtin"),
        "unexpected error: {}",
        err.message()
    );
}

#[test]
#[ignore = "pending: AstTy::Generic should preserve generic arguments"]
fn pending_generic_type_arguments_are_preserved() {
    let ast = parse("value: Option<Int> = make()").expect("generic surface syntax should parse");
    let debug = format!("{ast:?}");
    assert!(
        debug.contains("Generic"),
        "generic arguments should survive parse instead of collapsing: {debug}"
    );
}

#[test]
#[ignore = "pending: Float NaN/Infinity contract is not fixed yet"]
fn pending_float_non_finite_contract() {
    let source = r#"value = safe_div(0.0, 0.0)
print(to_string(value))"#;
    let (stdout, _stderr) = support::run_script_with_stderr("pending_float_contract.srt", source)
        .expect("float contract should be decidable once specified");
    assert!(
        stdout
            .iter()
            .any(|line| line.contains("NaN") || line.contains("ZeroDivisionError")),
        "future Float contract test should assert one precise behavior"
    );
}
