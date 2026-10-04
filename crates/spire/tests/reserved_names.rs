use spire::{
    parse, parse_tolerant_with_context, parse_with_context, ParserContext, SyntaxTokenKind,
};

const PRECEDENCE_NAMES: [&str; 9] = ["on", "and", "or", "eq", "neq", "lt", "lte", "gt", "gte"];
const CONSUMER_NAMES: [&str; 4] = ["is_match", "apply_pattern", "if_let", "if_let_then"];

#[test]
fn precedence_names_reject_variable_and_field_positions() {
    for name in PRECEDENCE_NAMES {
        for source in [
            format!("{name} = 1"),
            format!("def f({name}: Int) -> Int {{ 1 }}"),
            format!("f = {{|{name}| 1}}"),
            format!("const {name}: Int = 1"),
            format!("match 1 {{ {name} => 1 }}"),
            format!("match 1 {{ _ @ {name} => 1 }}"),
            format!("defrecord User({name}: Int)"),
            format!("defstruct User {{ {name}: Int }}"),
            format!("deferror User({name}: Int) {{ \"problem\" }}"),
            format!("User({name}: 1)"),
            format!("value.{name}"),
        ] {
            assert!(
                parse_with_context(&source, ParserContext::project(0)).is_err(),
                "reserved position accepted: {source}"
            );
        }
    }
}

#[test]
fn precedence_names_allow_callable_members_and_import_syntax() {
    for name in PRECEDENCE_NAMES {
        for source in [
            format!("defmod User {{ def {name}(value: Int) -> Int {{ value }} }}"),
            format!("deftrait User {{ def {name}(self: Self) -> Self }}"),
            format!("defrecord User(value: Int)\nimpl User {{ def {name}(self) -> Int {{ 1 }} }}"),
            format!("import User::{name}"),
            format!("import User::{{{name}}}"),
            format!("User::{name}(1)"),
            format!("{name}(1, 2)"),
            format!("f = &{name}"),
            format!("f = &{name}(&1, &2)"),
            format!("f = &User::{name}"),
        ] {
            parse_with_context(&source, ParserContext::project(0))
                .unwrap_or_else(|error| panic!("callable member rejected: {source}: {error:?}"));
        }
    }
}

#[test]
fn consumer_names_reject_fields_and_user_members() {
    for name in CONSUMER_NAMES {
        for source in [
            format!("defrecord User({name}: Int)"),
            format!("defstruct User {{ {name}: Int }}"),
            format!("value.{name}"),
            format!("User({name}: 1)"),
            format!("defmod User {{ def {name}(value: Int) -> Int {{ value }} }}"),
        ] {
            assert!(
                parse_with_context(&source, ParserContext::project(0)).is_err(),
                "reserved position accepted: {source}"
            );
        }
    }
}

#[test]
fn tolerant_names_have_keyword_classification_and_matching_rejections() {
    for name in PRECEDENCE_NAMES.into_iter().chain(CONSUMER_NAMES) {
        let source = format!("defrecord User({name}: Int)");
        let result = parse_tolerant_with_context(&source, ParserContext::project(0), None);
        assert!(
            !result.diagnostics.is_empty(),
            "accepted reserved field: {source}"
        );
        let start = source.find('(').unwrap() + 1;
        assert!(
            result.tokens.iter().any(|token| {
                token.span.start == start && token.kind == SyntaxTokenKind::Keyword
            }),
            "reserved name not classified as keyword: {source}: {:?}",
            result.tokens
        );
    }
    parse("compare = 1\npipe = 2\nfmap = 3\nbind = 4").unwrap();
}

#[test]
fn tolerant_consumer_paths_reject_non_kernel_owners() {
    for name in CONSUMER_NAMES {
        let source = format!("Regex::{name}(value, input)");
        let result = parse_tolerant_with_context(&source, ParserContext::project(0), None);
        assert!(
            !result.diagnostics.is_empty(),
            "accepted reserved member: {source}"
        );
        parse(&format!("import Regex::{name}"))
            .expect("import syntax retains ordinary member validation");
    }
}
