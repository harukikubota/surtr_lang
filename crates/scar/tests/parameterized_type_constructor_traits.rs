use diagnostics::TypeDiagnosticReason;
use sigil::resolved::Resolved;

fn resolve_without_std_prelude(source: &str) -> Vec<Resolved> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse without the standard prelude");
    sigil::resolve(ast).expect("source should resolve without the standard prelude")
}

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    scar::typecheck(resolve_without_std_prelude(source))
}

const USER_MONAD_T: &str = r#"
deftrait Monad
where
  Self: Type<$A>
{}

deftrait MonadT<$M>
where
  $M: Monad
  Self: Monad
{
  def lift::<Self>(value: $M<$A>) -> Self<$A>
}

defenum Base<$A> { Base($A), }
impl Monad for Base<$T> where $T: Monad.$A {}

defstruct Wrap<$M, $A>
where
  $M: Monad
{
  inner: $M<$A>
}
impl Wrap {
  def new(inner: $M<$A>) -> Wrap<$M, $A>
  where
    $M: Monad
  {
    Wrap { inner: inner }
  }
}

impl Monad for Wrap<$M, $T>
where
  $M: Monad
  $T: Monad.$A
{}

impl MonadT<$M> for Wrap<$M, $T>
where
  $M: Monad
  $T: MonadT.$A
{
  def lift::<Wrap<$M, $T>>(value: $M<$A>) -> Wrap<$M, $A>
  {
    Wrap(value)
  }
}
"#;

#[test]
fn user_defined_parameterized_constructor_trait_lifts_from_argument_into_expected_carrier() {
    check(&format!(
        r#"{USER_MONAD_T}
source: Base<Int> = Base::Base(1)
value: Wrap<Base, Int> = MonadT::lift(source)
"#
    ))
    .expect("base and transformer carriers should be inferred from independent inputs");
}

#[test]
fn parameterized_constructor_trait_does_not_choose_an_output_carrier_from_impl_count() {
    let error = check(&format!(
        r#"{USER_MONAD_T}
source: Base<Int> = Base::Base(1)
value = MonadT::lift(source)
"#
    ))
    .expect_err("an absent output constraint must remain ambiguous");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::AmbiguousReturnTypeArgument),
        "{error:?}"
    );
}

#[test]
fn parameterized_constructor_trait_direct_signature_surface_is_not_added() {
    let error = check(
        r#"deftrait Monad
where
  Self: Type<$A>
{}
deftrait MonadT<$M>
where
  $M: Monad
  Self: Monad
{}
def invalid(value: MonadT<Result>) -> Unit { () }"#,
    )
    .expect_err("parameterized TypeCtorTrait direct signatures are outside N04");
    assert!(
        error.message.contains("parameterized constructor trait"),
        "{error:?}"
    );
}
