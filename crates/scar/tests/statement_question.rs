use scar::typed::{
    ResultPreserveConstruction, SafeBindFailureTarget, SafeBindRhsProjection, TypedInner,
    TypedNode, TypedPattern,
};
use scar::types::Ty;
use sindr::warning::WarningKind;

#[allow(dead_code)]
mod support;

use support::{resolve_with_builtin_prelude, typecheck, typecheck_with_builtin_prelude};

fn result_binding_rhs(typed: &[TypedNode]) -> &TypedNode {
    typed.iter().find_map(|node| match &node.node {
        TypedInner::Bind(pattern, rhs)
            if matches!(pattern.unlocated(), TypedPattern::Var(_, id) if id.name == "result") => Some(rhs.as_ref()),
        _ => None,
    }).expect("expected result binding")
}

#[test]
fn question_accepts_only_canonical_result_with_unit_terminal_success() {
    for expression in ["Ok(())", "Ok(Ok(()))", "Ok(Ok(Ok(())))"] {
        let typed = typecheck_with_builtin_prelude(&format!("{expression}?"));
        let node = typed.last().expect("question statement");
        assert_eq!(node.ty, Ty::Unit);
        let TypedInner::SafeBind(pattern, rhs, projection, target) = &node.node else {
            panic!("question must use existing SafeBind control: {node:?}")
        };
        assert!(matches!(pattern.unlocated(), TypedPattern::Wildcard(_)));
        assert!(matches!(target, SafeBindFailureTarget::TopLevel));
        let SafeBindRhsProjection::CanonicalResultOnce { payload_ty, .. } = projection else {
            panic!("only the outer canonical Result may be projected")
        };
        let Ty::Result(outer_success, _) = &rhs.ty else {
            panic!("question RHS must be canonical Result")
        };
        assert_eq!(outer_success.as_ref(), payload_ty);
    }
}

#[test]
fn question_rejects_non_unit_terminal_success() {
    for expression in ["Ok(1)", "Ok(\"text\")", "Ok(Ok(1))"] {
        let error = typecheck(resolve_with_builtin_prelude(&format!("{expression}?")))
            .expect_err("question cannot discard a non-Unit terminal success");
        assert!(
            error.message.contains("terminal success type Unit"),
            "{error:?}"
        );
    }
}

#[test]
fn question_rejects_non_result_values() {
    for expression in ["()", "True", "Option::Some(())", "[()]"] {
        let error = typecheck(resolve_with_builtin_prelude(&format!("{expression}?")))
            .expect_err("question requires a canonical Result");
        assert!(error.message.contains("canonical Result"), "{error:?}");
    }
}

#[test]
fn question_does_not_infer_unknown_success_as_unit() {
    for source in [
        "Err(NoneError)?",
        "def reject(value: Result<$T>) -> Result<()> { value?\n Ok(()) }",
        "def reject(value: Result<Result<$T>>) -> Result<()> { value?\n Ok(()) }",
    ] {
        let error = typecheck(resolve_with_builtin_prelude(source))
            .expect_err("unresolved or rigid terminal success must be rejected");
        assert!(
            error.message.contains("terminal success type Unit"),
            "{error:?}"
        );
    }
}

#[test]
fn question_uses_nearest_callable_target() {
    typecheck_with_builtin_prelude(
        "def run() -> Result<()> { Ok(())?\n Ok(()) }\nhandler: (Int -> Result<()>) = {|value| Ok(())?\n Ok(()) }",
    );
    for source in [
        "def bad() -> Int { Ok(())?\n 1 }",
        "def outer() -> Result<()> { inner: (Int -> Int) = {|value| Ok(())?\n value}\n Ok(()) }",
    ] {
        let error = typecheck(resolve_with_builtin_prelude(source))
            .expect_err("a non-Result callable must not use an outer failure target");
        assert!(
            error
                .message
                .contains("requires an enclosing ResultContext return type"),
            "{error:?}"
        );
    }
}

#[test]
fn question_does_not_implicitly_wrap_a_unit_tail() {
    let error = typecheck(resolve_with_builtin_prelude(
        "def bad() -> Result<()> { Ok(())? }",
    ))
    .expect_err("a Unit statement is not a Result return value");
    assert!(error.message.contains("Unit"), "{error:?}");
}

#[test]
fn question_inside_do_uses_do_local_failure_target() {
    let typed = typecheck_with_builtin_prelude("result = do::<Result> { Ok(Ok(()))?; Ok(1) }");
    let rhs = result_binding_rhs(&typed);
    let TypedInner::DoSafeBind(control) = &rhs.node else {
        panic!("question inside do must preserve local control: {rhs:?}")
    };
    assert!(matches!(
        control.pattern.unlocated(),
        TypedPattern::Wildcard(_)
    ));
    assert!(matches!(
        control.failure_target,
        SafeBindFailureTarget::DoResultContext(_)
    ));
    assert!(matches!(
        control.projection,
        SafeBindRhsProjection::CanonicalResultOnce {
            payload_ty: Ty::Result(..),
            ..
        }
    ));
    typecheck_with_builtin_prelude(
        "def outer() -> Int { result = do::<Result> { Ok(())?\n Ok(1) }\n 1 }",
    );
}

#[test]
fn question_inside_do_enforces_terminal_success_constraint() {
    for source in [
        "result = do::<Result> { Ok(1)?\n Ok(()) }",
        "result = do::<Result> { Err(NoneError)?\n Ok(()) }",
    ] {
        let error = typecheck(resolve_with_builtin_prelude(source))
            .expect_err("do must preserve question-specific type constraints");
        assert!(
            error.message.contains("terminal success type Unit"),
            "{error:?}"
        );
    }
}

#[test]
fn question_checks_normalized_callable_signature_alias_output() {
    typecheck_with_builtin_prelude(
        "type Check = (Int -> Result<Unit>)\ndef probe(check: Check) -> Result<()> { check(1)?\n Ok(()) }",
    );
    let error = typecheck(resolve_with_builtin_prelude(
        "type Action<$A> = (Int -> Result<$A>)\ndef reject(action: Action<Int>) -> Result<()> { action(1)?\n Ok(()) }",
    ))
    .expect_err("aliases cannot conceal a non-Unit result payload");
    assert!(
        error.message.contains("terminal success type Unit"),
        "{error:?}"
    );
}

#[test]
fn question_does_not_accept_nominal_result_lookalikes() {
    let error = typecheck(resolve_with_builtin_prelude(
        "defenum ResultLike<$T> { Ok($T), Err(String) }\nResultLike::Ok(())?",
    ))
    .expect_err("a user enum with Result-style variants is not canonical Result");
    assert!(error.message.contains("canonical Result"), "{error:?}");
}

#[test]
fn question_preserves_result_effect_targets_in_callable_and_do() {
    let source = format!(
        "{}\ndef run() -> OptionT<Result, Int> {{ Ok(())?\n OptionT::some::<Result>(1) }}\nresult = do::<OptionT<Result, _>> {{ Ok(())?; OptionT::some::<Result>(1) }}",
        include_str!("../../../lib/types/monad_transformer/option_t.srt"),
    );
    let typed = typecheck_with_builtin_prelude(&source);
    let rhs = result_binding_rhs(&typed);
    let TypedInner::DoSafeBind(control) = &rhs.node else {
        panic!("question in Result-effect do must preserve do-local control")
    };
    assert!(matches!(
        &control.failure_target,
        SafeBindFailureTarget::DoResultContext(target)
            if matches!(target.construction, ResultPreserveConstruction::AnnotatedStruct { .. })
    ));
}

#[test]
fn question_in_non_result_closure_inside_do_rejects_outer_failure_targets() {
    let error = typecheck(resolve_with_builtin_prelude(
        "def outer() -> Result<()> { result = do::<Result> { inner: (Int -> Int) = {|value| Ok(())?\n value}\n Ok(()) }\n Ok(()) }",
    ))
    .expect_err("a nested non-Result callable cannot borrow a do or outer callable target");
    assert!(
        error
            .message
            .contains("requires an enclosing ResultContext return type"),
        "{error:?}"
    );
}

#[test]
fn question_preserves_safebind_alternative_target_for_non_result_do() {
    for operator in ["Ok(())?", "_ =? Ok(())"] {
        let typed = typecheck_with_builtin_prelude(&format!(
            "result = do::<Option> {{ {operator}\n Option::Some(1) }}",
        ));
        let rhs = result_binding_rhs(&typed);
        let TypedInner::DoSafeBind(control) = &rhs.node else {
            panic!("question and SafeBind must use identical do control")
        };
        assert!(matches!(
            control.failure_target,
            SafeBindFailureTarget::DoAlternative { .. }
        ));
        assert!(matches!(
            control.projection,
            SafeBindRhsProjection::CanonicalResultOnce {
                payload_ty: Ty::Unit,
                ..
            }
        ));
    }
}

#[test]
fn unannotated_closures_cannot_borrow_outer_safebind_failure_targets() {
    for statement in ["Ok(())?", "_ =? Ok(())"] {
        for source in [
            format!("closure = {{|value: Int| {statement}\n ()}}"),
            format!("def outer() -> Result<()> {{ closure = {{|value: Int| {statement}\n ()}}\n Ok(()) }}"),
            format!("closure = {{|value: Int| {statement}\n Ok(())}}"),
        ] {
            let error = typecheck(resolve_with_builtin_prelude(&source))
                .expect_err("an unannotated closure has no concrete local Result failure target");
            assert!(error.message.contains("requires an enclosing ResultContext return type"), "{source}: {error:?}");
        }
        let source = format!("impl Int {{ defextractor outer(value: Int) -> MatchResult<Int> {{ closure = {{|item: Int| {statement}\n ()}}\n MatchResult::Ok(value) }} }}");
        let error = typecheck(resolve_with_builtin_prelude(&source))
            .expect_err("an ordinary closure cannot borrow an enclosing MatchResult target");
        assert!(
            error
                .message
                .contains("requires an enclosing ResultContext return type"),
            "{error:?}"
        );
    }
}

#[test]
fn explicit_result_closure_signatures_provide_local_safebind_failure_targets() {
    for statement in ["Ok(())?", "_ =? Ok(())"] {
        typecheck_with_builtin_prelude(&format!(
            "def outer() -> Int {{ closure: (Int -> Result<()>) = {{|value| {statement}\n Ok(()) }}\n 1 }}",
        ));
    }
}

#[test]
fn question_is_an_explicit_discard_for_unused_value_warnings() {
    let resolved = resolve_with_builtin_prelude("Ok(())?\n ()");
    let output = support::session_from_cached_std_prelude()
        .typecheck_with_warnings(resolved)
        .expect("question statement should typecheck");
    assert!(output
        .warnings
        .iter()
        .all(|warning| warning.kind != WarningKind::UnusedValue));
}
