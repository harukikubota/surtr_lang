#[allow(dead_code)]
mod support;

use support::{resolve_with_builtin_prelude, typecheck, typecheck_with_builtin_prelude};

#[test]
fn standard_carriers_without_error_retention_do_not_implement_monad_fail() {
    for carrier in [
        "Option<Int>",
        "List<Int>",
        "OptionT<Result, Int>",
        "Identity<Int>",
        "Reader<String, Int>",
        "State<Int, Int>",
        "Either<String, Int>",
        "EitherT<String, Identity, Int>",
    ] {
        let source = format!("value: {carrier} = MonadFail::fail(NoneError)");
        let error = typecheck(resolve_with_builtin_prelude(&source))
            .expect_err("absence carriers must not acquire a standard MonadFail implementation");
        assert!(error.message.contains("MonadFail"), "{carrier}: {error:?}");
    }
}

#[test]
fn blanket_either_failure_impl_overlaps_the_standard_error_impl() {
    for source in [
        r#"impl MonadFail for Either<$L, $T>
where $T: MonadFail.$A
{
  def fail::<Either<$L, $T>>(error: Error) -> Either<$L, $T> {
    MonadFail::fail(error)
  }
}"#,
        r#"impl MonadFail for EitherT<$L, $M, $T>
where
  $M: Monad
  $T: MonadFail.$A
{
  def fail::<EitherT<$L, $M, $T>>(error: Error) -> EitherT<$L, $M, $T> {
    MonadFail::fail(error)
  }
}"#,
    ] {
        let error = typecheck(resolve_with_builtin_prelude(source))
            .expect_err("a blanket Left parameter intersects the standard Error implementation");
        assert!(
            error
                .message
                .contains("Overlapping trait impls for MonadFail"),
            "{error:?}"
        );
    }
}

#[test]
fn concrete_non_error_left_failure_impl_does_not_overlap() {
    typecheck_with_builtin_prelude(
        r#"impl MonadFail for Either<String, $T>
where $T: MonadFail.$A
{
  def fail::<Either<String, $T>>(error: Error) -> Either<String, $T> {
    Either::Left(Error::message(error))
  }
}
impl MonadFail for EitherT<String, $M, $T>
where
  $M: Monad
  $T: MonadFail.$A
{
  def fail::<EitherT<String, $M, $T>>(error: Error) -> EitherT<String, $M, $T> {
    EitherT::left(Error::message(error))
  }
}
value: Either<String, Int> = MonadFail::fail(NoneError)
transformed: EitherT<String, Identity, Int> = MonadFail::fail(NoneError)
"#,
    );
}
