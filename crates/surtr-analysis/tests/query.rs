use surtr_analysis::query::{
    parse_command_query, CaptureQueryArgKind, CommandQuery, OperatorRhs, QueryArgKind,
};

#[test]
fn command_query_parser_is_public_for_editor_commands() {
    let query = parse_command_query("text |> replace($from, _1, $to)")
        .expect("editor command query should parse");

    let CommandQuery::TypedOperator(operator) = query else {
        panic!("expected typed operator query");
    };
    assert_eq!(operator.operator, "|>");
    let OperatorRhs::TopLevelCall(call) = &operator.rhs else {
        panic!("expected top-level call rhs");
    };
    assert_eq!(call.callee, "replace");
    assert!(matches!(
        call.args[0].kind,
        QueryArgKind::ForcedBinding(ref name) if name == "from"
    ));
    assert!(matches!(call.args[1].kind, QueryArgKind::PipePlaceholder));
}

#[test]
fn command_query_parser_rejects_nested_capture_application() {
    let err = parse_command_query("map(&List::map(&1, &add(Int, &1)))")
        .expect_err("nested capture application should stay rejected");

    assert!(err.message().contains("nested capture applications"));
}

#[test]
fn command_query_parser_keeps_capture_slots_and_type_args() {
    let query = parse_command_query("map(&add(Int, &1))").expect("capture query should parse");

    let CommandQuery::TypedCall(call) = query else {
        panic!("expected typed call query");
    };
    assert_eq!(call.callee, "map");
    let QueryArgKind::Capture(capture) = &call.args[0].kind else {
        panic!("expected capture argument");
    };
    assert_eq!(capture.callable, "add");
    assert!(matches!(
        capture.args[0].kind,
        CaptureQueryArgKind::TypeExpr(_)
    ));
    assert!(matches!(capture.args[1].kind, CaptureQueryArgKind::Slot(1)));
}
