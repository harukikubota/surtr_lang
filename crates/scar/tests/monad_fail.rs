#[allow(dead_code)]
mod support;

use support::{resolve_with_builtin_prelude, typecheck, typecheck_with_builtin_prelude};

#[test]
fn safe_bind_uses_either_error_failure_capability() {
    typecheck_with_builtin_prelude(
        "def relay(value: Result<Int>) -> Either<Error, Int> { x =? value\n Either::Right(x) }",
    );
}

#[test]
fn failure_capability_is_provided_by_declared_generic_bound() {
    typecheck_with_builtin_prelude(
        r#"def relay(value: $M<Int>, input: Result<Int>) -> $M<Int> where $M: MonadFail {
  x =? input
  value
}
result: Either<Error, Int> = relay(Either<Error, Int>::Right(1), Ok(2))
"#,
    );
    let error = typecheck(resolve_with_builtin_prelude(
        r#"def relay(value: $M<Int>, input: Result<Int>) -> $M<Int> where $M: Monad {
  x =? input
  value
}
result: Result<Int> = relay(Ok(1), Ok(2))
"#,
    ))
    .expect_err("concretization must not strengthen a Monad bound");
    assert!(error.message.contains("MonadFail"), "{error:?}");
}

#[test]
fn do_uses_declared_monad_fail_bound_for_partial_pattern() {
    typecheck_with_builtin_prelude(
        r#"def relay(value: $M<Option<Int>>) -> $M<Int> where $M: MonadFail {
  do { Option::Some(x) <- value; Monad::return(x) }
}
result: Result<Int> = relay(Ok(Option::Some(1)))
"#,
    );
}

#[test]
fn ordinary_callable_cannot_borrow_alternative_or_outer_target() {
    for source in [
        "def bad(value: Result<Int>) -> Option<Int> { x =? value\n Option::Some(x) }",
        "def bad(value: Result<Int>) -> List<Int> { x =? value\n [x] }",
        "def outer() -> Result<Int> { inner = {|x: Int| value =? Ok(x)\n value}\n Ok(1) }",
    ] {
        let error = typecheck(resolve_with_builtin_prelude(source))
            .expect_err("the nearest callable needs its own MonadFail return type");
        assert!(error.message.contains("MonadFail"), "{source}: {error:?}");
    }
}

#[test]
fn safe_bind_does_not_unwrap_either_or_transformers() {
    for source in [
        "value =? Either<Error, Int>::Right(1)",
        "value =? ResultT::ok::<Identity>(1)",
        "value =? OptionT::some::<Identity>(1)",
    ] {
        let error = typecheck(resolve_with_builtin_prelude(source))
            .expect_err("total non-Result SafeBind remains invalid");
        assert!(
            error.message.contains("Only a Result RHS"),
            "{source}: {error:?}"
        );
    }
    typecheck_with_builtin_prelude(
        "def relay(value: Result<Result<Int>>) -> Result<Result<Int>> { inner: Result<Int> =? value\n Ok(inner) }",
    );
    typecheck_with_builtin_prelude(
        "value: Result<Result<Int>> = do::<Result> { inner: Result<Int> <- Ok(Ok(1)); Ok(inner) }",
    );
}

#[test]
fn unresolved_failure_dispatch_is_rejected_before_codegen() {
    let source = r#"def relay(value: Result<$A>) -> Result<$A> {
  item =? value
  Ok(item)
}
inspect(relay(Err(NoneError)))"#;
    let error = typecheck(resolve_with_builtin_prelude(source))
        .expect_err("an unresolved failure dispatch must not leak a generic function index");
    assert!(
        error.message.contains("concrete method instantiation"),
        "{error:?}"
    );
    typecheck_with_builtin_prelude(&source.replace(
        "inspect(relay(Err(NoneError)))",
        "value: Result<Int> = relay(Err(NoneError))",
    ));
}
