use surtr_analysis::query::{parse_command_query, CommandQuery, QueryArgKind};

#[test]
fn command_query_parser_is_public_for_editor_commands() {
    let query = parse_command_query("|*> Option").expect("editor command query should parse");

    let CommandQuery::OperatorTarget(operator) = query else {
        panic!("expected operator target query");
    };
    assert_eq!(operator.operator, "|*>");
    assert!(matches!(operator.target.kind, QueryArgKind::TypeExpr(_)));
}

#[test]
fn command_query_parser_rejects_removed_forced_binding_symbol_surface() {
    let err = parse_command_query("$value").expect_err("forced binding symbol surface should fail");

    assert_eq!(err.message(), "Unsupported command query symbol `$value`.");
}

#[test]
fn command_query_parser_rejects_removed_forced_binding_surface() {
    let err = parse_command_query("compare($value, Int)")
        .expect_err("forced binding surface should stay rejected");

    assert!(err.message().contains("`$value`"));
}

#[test]
fn command_query_parser_rejects_removed_capture_surface() {
    let err = parse_command_query("map(&add(Int, &1))").expect_err("capture surface should fail");
    assert!(err.message().contains("`&add(Int, &1)`"));
}
