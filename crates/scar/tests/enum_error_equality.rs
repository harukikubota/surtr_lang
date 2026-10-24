#[allow(dead_code)]
mod support;

use scar::typed::{TraitDispatch, TraitDispatchTarget, TypedInner, TypedNode};
use scar::types::Ty;

fn check(source: &str) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    support::typecheck(support::resolve_with_builtin_prelude(source))
}

const INPUTS: &str = "defenumerror Closed { NoneError }\nleft: Result<Int, Closed> = Err(NoneError())\nright: Result<Int, Closed> = Err(NoneError())\n";
const GENERIC: &str =
    "def same(left: $T, right: $T) -> Boolean where $T: Eq { Eq::eq(left, right) }\n";

#[test]
fn enum_error_result_equality_accepts_direct_operator_and_generic_calls() {
    for call in ["Eq::eq(left, right)", "left == right", "same(left, right)"] {
        check(&format!("{GENERIC}{INPUTS}value = {call}"))
            .unwrap_or_else(|error| panic!("{call}: {error:?}"));
    }
}

#[test]
fn enum_error_result_equality_preserves_a_rigid_payload_eq_bound() {
    let source = format!(
        "{INPUTS}def equal(left: Result<$A, Closed>, right: Result<$A, Closed>) -> Boolean where $A: Eq {{ Eq::eq(left, right) }}\nvalue = equal(left, right)"
    );
    check(&source)
        .expect("a declared success-type Eq bound authorizes the standard Result equality");
}

#[test]
fn enum_error_result_equality_accepts_different_error_sets_and_common_error() {
    for call in ["Eq::eq(left, right)", "left == right"] {
        for right_ty in ["Result<Int>", "Result<Int, Second>"] {
            let source = format!("defenumerror Closed {{ NoneError }}\ndefenumerror Second {{ NoneError }}\nleft: Result<Int, Closed> = Err(NoneError())\nright: {right_ty} = Err(NoneError())\nvalue = {call}");
            check(&source).unwrap_or_else(|error| panic!("{right_ty} {call}: {error:?}"));
        }
    }
}

#[test]
fn enum_error_result_equality_preserves_success_type_and_eq_requirement() {
    for call in ["Eq::eq(left, right)", "left == right", "same(left, right)"] {
        for (left_ty, left, right_ty, right) in [
            ("Int", "1", "String", "\"one\""),
            (
                "(Int -> Int)",
                "{|value: Int| value}",
                "(Int -> Int)",
                "{|value: Int| value}",
            ),
        ] {
            let source = format!("{GENERIC}defenumerror Closed {{ NoneError }}\nleft: Result<{left_ty}, Closed> = Ok({left})\nright: Result<{right_ty}, Closed> = Ok({right})\nvalue = {call}");
            check(&source).expect_err("Error transport does not erase success or Eq constraints");
        }
    }
}

#[test]
fn enum_error_result_equality_keeps_a_user_implementation_on_the_original_set() {
    let source = format!("{INPUTS}impl Eq for Result<Int, Closed> {{ def eq(self: Self, rhs: Self) -> Boolean {{ False }} }}\nvalue = Eq::eq(left, right)");
    let nodes = check(&source).expect("the raw user implementation is selected first");
    let result = nodes
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(_, rhs) if matches!(rhs.node, TypedInner::TraitCall { .. }) => {
                Some(rhs)
            }
            _ => None,
        })
        .expect("checked equality call");
    let TypedInner::TraitCall {
        receiver_ty,
        args,
        dispatch,
        ..
    } = &result.node
    else {
        unreachable!()
    };
    assert!(
        matches!(receiver_ty, Ty::Result(_, error) if matches!(error.as_ref(), Ty::Enum(name, _) if name.ends_with("Closed")))
    );
    let TraitDispatch::Static(TraitDispatchTarget::UserFunction { fun_idx, .. }) = dispatch else {
        panic!("expected finalized user implementation: {dispatch:?}");
    };
    let definition = nodes
        .iter()
        .find(|node| matches!(&node.node, TypedInner::Def(index, ..) if index == fun_idx))
        .expect("selected implementation body");
    let TypedInner::Def(_, _, _, _, _, _, body, _) = &definition.node else {
        unreachable!()
    };
    let body = match &body.node {
        TypedInner::Block(items) => items.last().expect("method result"),
        _ => body,
    };
    assert!(matches!(
        body.node,
        TypedInner::Lit(spire::ast::Lit::Bool(false))
    ));
    assert!(args
        .iter()
        .all(|argument| !matches!(argument.node, TypedInner::ErrorCoerce { .. })));
}
