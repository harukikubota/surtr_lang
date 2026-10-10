use surtr_analysis::query::{parse_command_query, CommandQuery, CommandQueryParseErrorReason};

#[test]
fn command_query_parser_is_public_for_editor_commands() {
    for source in [
        "Compare::compare",
        "predicate?",
        "dbg!",
        "User!",
        "==",
        "/",
        "|*>",
        "|*|",
        "<|>",
        "(,)",
        "Kernel::(,)",
        "=",
        "=?",
        "Kernel::=?",
    ] {
        let CommandQuery::Symbol(query) = parse_command_query(source).expect("symbol query") else {
            panic!("expected symbol query: {source}");
        };
        assert_eq!(query.source, source);
    }
}

#[test]
fn command_query_parser_rejects_argument_and_expression_forms() {
    for source in [
        "compare(Int, Int)",
        "compare(left, right)",
        "User()",
        "User!()",
        "predicate?(Int)",
        "|*> Option",
        "List<Int>",
        "1",
        "True()",
        "\"value\"",
        "$value",
        "_1",
        "&add",
        "map(&add(Int, &1))",
        "a+b",
        "User..field",
        "User::",
        "::name",
        "predicate??",
        "Predicates?::predicate",
        "Foo!::bar",
    ] {
        let err = parse_command_query(source).expect_err(source);
        assert!(
            matches!(
                err.reason(),
                CommandQueryParseErrorReason::UnsupportedSymbol
                    | CommandQueryParseErrorReason::UnsupportedForm
            ),
            "{source}: {err:?}"
        );
    }
}

#[test]
fn command_query_parser_preserves_character_offsets_for_rejected_forms() {
    let err = parse_command_query("  compare(型, Int)  ").expect_err("arguments are rejected");
    assert_eq!(err.span().start, 2);
    assert_eq!(err.span().end, 17);
    let err = parse_command_query("  ").expect_err("empty query");
    assert_eq!(err.reason(), CommandQueryParseErrorReason::Empty);
    assert_eq!(err.span().start, 2);
    assert_eq!(err.span().end, 2);
}

#[test]
fn command_query_parser_preserves_facet_and_field_paths() {
    assert!(matches!(
        parse_command_query("Facet.User").unwrap(),
        CommandQuery::FacetRootDoc(_)
    ));
    for source in ["User.password", "pair._1", "User::Nested.field"] {
        assert!(matches!(
            parse_command_query(source).unwrap(),
            CommandQuery::FieldPath(_)
        ));
    }
}

#[test]
fn command_query_parser_rejects_numbered_placeholders_as_names() {
    for source in [
        "_",
        "_0",
        "_1",
        "_2",
        "_12",
        "_12345678901234567890",
        "_01",
        "_2?",
        "_2!",
        "Module::_2",
        "_2::member",
        "_2.field",
        "名前",
        "Module::名前",
    ] {
        assert!(parse_command_query(source).is_err(), "{source}");
    }
    for source in ["_helper", "helper_12", "_12helper", "Module::_helper"] {
        assert!(
            matches!(
                parse_command_query(source).unwrap(),
                CommandQuery::Symbol(_)
            ),
            "{source}"
        );
    }
    for source in ["pair._0", "pair._1", "pair._12"] {
        assert!(
            matches!(
                parse_command_query(source).unwrap(),
                CommandQuery::FieldPath(_)
            ),
            "{source}"
        );
    }
}
