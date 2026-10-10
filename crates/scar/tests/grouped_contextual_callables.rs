// This test target uses only part of the helpers shared with the surface tests.
#[allow(dead_code)]
mod support;
use support::{resolve_with_builtin_prelude, typecheck};

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    typecheck(resolve_with_builtin_prelude(source))
}

#[test]
fn grouped_hole_input_callable_result_rejects_context_operators() {
    for source in [
        "[1, 2] |*> (always(10))",
        "[1, 2] |*> ((always(10)))",
        "1 |> (always(10))",
        "Ok(1) |>= (always(Ok(10)))",
        "mapper = always(10)\n[1] |*> mapper",
        "bound = always(Ok(10))\nOk(1) |>= bound",
    ] {
        check(source).expect_err(source);
    }
}

#[test]
fn grouped_hole_input_callable_rejects_concrete_arguments_annotations_and_returns() {
    for expression in ["always(10)", "(always(10))", "((always(10)))"] {
        for source in [
            format!("def use(mapper: (Int -> Int)) -> Int {{ mapper(1) }}\nuse({expression})"),
            format!("mapper: (Int -> Int) = {expression}\nmapper(1)"),
            format!("def make() -> (Int -> Int) {{ {expression} }}\nmake()"),
            format!("mapper: (Int -> Int) = match True {{ True => {expression}, False => {expression} }}"),
        ] {
            check(&source).expect_err(&source);
        }
    }
}

#[test]
fn grouped_generic_callable_result_and_capture_receive_expected_types() {
    for expression in [
        "make()",
        "(make())",
        "((make()))",
        "&id",
        "(&id)",
        "((&id))",
    ] {
        let source = format!("def make::<$A>() -> ($A -> $A) {{ {{|value| value}} }}\ndef id(value: $A) -> $A {{ value }}\nmapper: (Int -> Int) = {expression}\nmapper(1)");
        check(&source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
    }
}

#[test]
fn grouped_context_does_not_accept_real_callable_or_return_mismatches() {
    for source in [
        "def text(value: String) -> Int { 1 }\nmapper: (Int -> Int) = (&text)",
        "mapper: (Int -> String) = (always(10))",
        "def consume(value: Int) -> Int { value }\nconsume((\"text\"))",
        "value: Int = (True)",
    ] {
        check(source).expect_err(source);
    }
}

#[test]
fn contextual_callable_branches_reject_hole_input_in_both_orders() {
    for (first, second) in [
        ("(always(10))", "(&identity)"),
        ("(&identity)", "((always(10)))"),
    ] {
        for expression in [
            format!("if(True, {first}, {second})"),
            format!("cond {{ False => {first}, True => {second} }}"),
            format!("match True {{ True => {first}, False => {second} }}"),
            format!("if_let(Ok(1), Ok(_), {first}, {second})"),
            format!("if_let(Ok(1), Ok(x), {first}, {second})"),
        ] {
            let source = format!("def identity(value: Int) -> Int {{ value }}\nmapper: (Int -> Int) = {expression}\nmapper(1)");
            check(&source).expect_err(&source);
        }
    }
}

#[test]
fn hole_input_relation_does_not_relax_identity_or_container_shapes() {
    for source in [
        "def identity(value: Int) -> Int { value }\nmapper: (_ -> Int) = (&identity)",
        "mapper: (Int, Int -> Int) = (always(10))",
        "mappers: List<(Int -> Int)> = [always(10)]",
        "def identity(value: Int) -> Int { value }\nif(True, (always(10)), (&identity))",
        "def identity(value: Int) -> Int { value }\nif(True, (&identity), (always(10)))",
    ] {
        check(source).expect_err(source);
    }
}

#[test]
fn contextual_branch_join_preserves_concrete_input_and_grouping_boundary() {
    use scar::typed::TypedInner;
    use scar::types::Ty;

    let program = check("def identity(value: Int) -> Int { value }\nmapper: (Int -> Int) = if(True, (({|value: Int| 10})), (&identity))").unwrap();
    let rhs = program
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) if matches!(rhs.node, TypedInner::If(..)) => {
                Some(rhs.as_ref())
            }
            _ => None,
        })
        .expect("contextual if binding");
    assert_eq!(rhs.ty, Ty::Func(vec![Ty::Int], Box::new(Ty::Int)));
    let TypedInner::If(_, first, Some(second)) = &rhs.node else {
        panic!("if branches")
    };
    assert_eq!(first.ty, Ty::Func(vec![Ty::Int], Box::new(Ty::Int)));
    assert!(matches!(first.node, TypedInner::EagerBoundary(_)));
    assert_eq!(second.ty, Ty::Func(vec![Ty::Int], Box::new(Ty::Int)));
}

#[test]
fn grouped_concrete_input_closures_and_captures_keep_context() {
    for expression in [
        "{|value: Int| 10}",
        "({|value: Int| 10})",
        "(({|value: Int| 10}))",
        "&identity",
        "(&identity)",
        "((&identity))",
    ] {
        for source in [
            format!("def identity(value: Int) -> Int {{ value }}\ndef use(mapper: (Int -> Int)) -> Int {{ mapper(1) }}\nuse({expression})"),
            format!("def identity(value: Int) -> Int {{ value }}\nmapper: (Int -> Int) = {expression}\nmapper(1)"),
            format!("def identity(value: Int) -> Int {{ value }}\ndef make() -> (Int -> Int) {{ {expression} }}\nmake()"),
            format!("def identity(value: Int) -> Int {{ value }}\nmapper: (Int -> Int) = match True {{ True => {expression}, False => {expression} }}"),
        ] {
            check(&source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
        }
    }
}
