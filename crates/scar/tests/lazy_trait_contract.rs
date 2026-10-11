#[allow(dead_code)]
mod support;

use sindr::policy::RuntimeSourcePolicy;

#[test]
fn standard_trait_can_declare_lazy_default_method() {
    let source = format!(
        "{}\n deftrait LazyProbe {{ def choose(value: Lazy<Self>) -> Self {{ value() }} def relay(value: Self) -> Self {{ LazyProbe::choose(value) }} }}\n impl LazyProbe for Int {{}}",
        include_str!("../../../lib/traits/default.srt")
    );
    support::typecheck_std_modules_with_overrides(&[("Default", &source)])
        .expect("standard Trait declarations can own Lazy contracts");
}

#[test]
fn user_trait_cannot_declare_lazy_contract_even_with_standard_method_spelling() {
    let error = support::typecheck_with_rules(
        "deftrait Local { def fail_if(value: Lazy<Self>) -> Self { value() } }",
        RuntimeSourcePolicy::script(),
    )
    .expect_err("new user Lazy contracts must reject");
    assert!(error.message.contains("Lazy"), "{error:?}");
}

#[test]
fn user_override_inherits_standard_lazy_signature_and_call_contract() {
    support::typecheck_with_rules(
        r#"
impl MonadFail for Either<String, $T>
where
  $T: MonadFail.$A
{
  def fail::<Either<String, $T>>(error: Error) -> Either<String, $T> {
    Either::Left(Error::message(error))
  }
  def fail_if(condition: Boolean, err: Lazy<Error>, then: Lazy<Either<String, $T>>) -> Either<String, $T> {
    if(condition, Either::Left(Error::message(err())), then())
  }
}
value: Either<String, Int> = MonadFail::fail_if(False, NoneError(), Either::Right(3))
captured: (Boolean, (-> Error), (-> Either<String, Int>) -> Either<String, Int>) = &MonadFail::fail_if(&1, &2, &3)
"#,
        RuntimeSourcePolicy::script(),
    ).expect("override inherits Lazy from its resolved standard Trait declaration");
}

#[test]
fn lazy_trait_capture_preserves_eager_and_bare_capture_boundaries() {
    for source in [
        "captured: (Boolean, (-> Error), (-> Result<Int>) -> Result<Int>) = &MonadFail::fail_if",
        "captured: (Boolean, Error -> Result<Int>) = &MonadFail::fail_if(&1, (id(&2)), Ok(1))",
    ] {
        let error = support::typecheck_with_rules(source, RuntimeSourcePolicy::script())
            .expect_err("Lazy capture boundary must reject");
        assert!(error.message.contains("Lazy"), "{error:?}");
    }
}

#[test]
fn standard_ordinary_function_can_declare_lazy_contract() {
    let source = format!(
        "{}\n def lazy_probe(value: Lazy<$A>) -> $A {{ value() }}\n def lazy_probe_relay() -> Int {{ lazy_probe(3) }}\n}}",
        include_str!("../../../lib/kernel.srt").trim_end().strip_suffix('}').expect("Kernel owner")
    );
    support::typecheck_std_modules_with_overrides(&[("Kernel", &source)])
        .expect("standard ordinary functions can own Lazy contracts");
}

#[test]
fn user_ordinary_function_cannot_declare_lazy_contract() {
    let error = support::typecheck_with_rules(
        "def lazy_probe(value: Lazy<Int>) -> Int { value() }",
        RuntimeSourcePolicy::script(),
    )
    .expect_err("user ordinary functions cannot create Lazy contracts");
    assert!(error.message.contains("Lazy"), "{error:?}");
}
