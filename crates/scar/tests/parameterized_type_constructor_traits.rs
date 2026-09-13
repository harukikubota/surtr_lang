use diagnostics::TypeDiagnosticReason;
use scar::typed::{TypedInner, TypedPattern};
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
fn full_constructor_rta_preserves_its_mapped_payload_constraint() {
    let error = check(&format!(
        r#"{USER_MONAD_T}
source: Base<Int> = Base::Base(1)
value = MonadT::lift::<Wrap<Base, String>>(source)
"#
    ))
    .expect_err("a full constructor RTA must not erase its concrete mapped payload");
    assert!(
        matches!(
            error.reason(),
            Some(
                TypeDiagnosticReason::ReturnTypeArgumentMismatch
                    | TypeDiagnosticReason::ArgumentTypeMismatch
            )
        ),
        "{error:?}"
    );
}

#[test]
fn full_constructor_rta_rejects_a_captured_base_mismatch() {
    let error = check(&format!(
        r#"{USER_MONAD_T}
defenum Other<$A> {{ Other($A), }}
impl Monad for Other<$T> where $T: Monad.$A {{}}
source: Base<Int> = Base::Base(1)
value = MonadT::lift::<Wrap<Other, _>>(source)
"#
    ))
    .expect_err("a full carrier RTA must not rewrite its captured base from the value argument");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::ArgumentTypeMismatch),
        "{error:?}"
    );
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

#[test]
fn receiverless_lift_matches_trait_argument_target_and_value_together() {
    let source = r#"
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor {}
deftrait MonadT<$M>
where
  $M: Monad
  Self: Monad
{
  def lift::<Self>(value: $M<$A>) -> Self<$A>
}

defenum Base<$A> { Base($A), }
defenum Other<$A> { Other($A), }
defenum Wrap<$A> { Wrap($A), }
impl Functor for Base<$T> where $T: Functor.$A {}
impl Monad for Base<$T> where $T: Monad.$A {}
impl Functor for Other<$T> where $T: Functor.$A {}
impl Monad for Other<$T> where $T: Monad.$A {}
impl Functor for Wrap<$T> where $T: Functor.$A {}
impl Monad for Wrap<$T> where $T: Monad.$A {}

impl MonadT<Base> for Wrap<$T> {
  def lift::<Wrap<$T>>(value: Base<$A>) -> Wrap<$A> {
    match value { Base::Base(item) => Wrap::Wrap(item), }
  }
}
impl MonadT<Other> for Wrap<$T> {
  def lift::<Wrap<$T>>(value: Other<$A>) -> Wrap<$A> {
    match value { Other::Other(item) => Wrap::Wrap(item), }
  }
}

source: Base<Int> = Base::Base(1)
value: Wrap<Int> = MonadT::lift(source)
"#;
    check(source).expect("the concrete input selects the matching MonadT Trait argument");
}

#[test]
fn unique_monad_impl_does_not_infer_an_underconstrained_lift_base() {
    let error = check(
        r#"
deftrait Monad where Self: Type<$A> {
  def return::<Self>(value: $A) -> Self<$A>
}
deftrait MonadT<$M>
where
  $M: Monad
  Self: Monad
{
  def lift::<Self>(value: $M<$A>) -> Self<$A>
}
defenum Base<$A> { Base($A), }
impl Monad for Base<$T> {
  def return::<Base<$T>>(value: $A) -> Base<$A> { Base::Base(value) }
}
defenum Wrap<$A> { Wrap($A), }
impl Monad for Wrap<$T> {
  def return::<Wrap<$T>>(value: $A) -> Wrap<$A> { Wrap::Wrap(value) }
}
impl MonadT<Base> for Wrap<$T> {
  def lift::<Wrap<$T>>(value: Base<$A>) -> Wrap<$A> {
    match value { Base::Base(item) => Wrap::Wrap(item), }
  }
}
value = MonadT::lift(Monad::return(1))
"#,
    )
    .expect_err("a unique Monad impl is not evidence for the base of return");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::AmbiguousReturnTypeArgument),
        "{error:?}"
    );
}

#[test]
fn receiverless_lift_requires_the_value_static_monad_capability() {
    let error = check(
        r#"
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor {}
deftrait MonadT<$M>
where
  $M: Monad
  Self: Monad
{
  def lift::<Self>(value: $M<$A>) -> Self<$A>
}
defenum Base<$A> { Base($A), }
impl Functor for Base<$T> where $T: Functor.$A {}
impl Monad for Base<$T> where $T: Monad.$A {}
defenum Wrap<$M, $A> where $M: Monad { Wrap($M<$A>), }
impl Functor for Wrap<$M, $T>
where
  $M: Monad
  $T: Functor.$A
{}
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
  def lift::<Wrap<$M, $T>>(value: $M<$A>) -> Wrap<$M, $A> {
    Wrap::Wrap(value)
  }
}
def retain(value: $F<Int>) -> $F<Int> where $F: Functor { value }
source: Base<Int> = Base::Base(1)
view = retain(source)
lifted: Wrap<Base, Int> = MonadT::lift(view)
"#,
    )
    .expect_err("a Functor-only static view cannot provide Monad for lift's base");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    assert_eq!(
        error.structured.as_ref().unwrap().data_json()["required_capability"],
        "Monad"
    );
}

#[test]
fn declared_return_expectation_reaches_pipe_list_and_tuple_contents() {
    let declarations = r#"
deftrait PipeApply<$A, $B> {
  def pipe_apply::<$B>(self: Self, value: $A) -> $B
}
impl PipeApply<$A, $B> for ($A -> $B) {
  def pipe_apply::<$B>(self: Self, value: $A) -> $B { self(value) }
}
deftrait Monad where Self: Type<$A> {
  def return::<Self>(value: $A) -> Self<$A>
}
impl Monad for List<$T> where $T: Monad.$A {
  def return::<List<$T>>(value: $A) -> List<$A> { [value] }
}
"#;
    for (return_ty, body) in [
        ("List<List<Int>>", "[Monad::return(1)]"),
        ("List<Int>", "1 |> Monad::return()"),
        ("(List<Int>, Int)", "(Monad::return(1), 0)"),
        ("List<Int>", "if (True, Monad::return(1), Monad::return(2))"),
    ] {
        check(&format!(
            "{declarations}\ndef make() -> {return_ty} {{ {body} }}"
        ))
        .unwrap_or_else(|error| panic!("declared return {return_ty} for {body}: {error:?}"));
    }
}

#[test]
fn full_constructor_rta_binding_pattern_has_concrete_specialized_type() {
    let typed = check(&format!(
        r#"{USER_MONAD_T}
source: Base<Int> = Base::Base(1)
full = MonadT::lift::<Wrap<Base, Int>>(source)
"#
    ))
    .expect("the explicit constructor application typechecks");
    let full_pattern = typed.iter().find_map(|node| match &node.node {
        TypedInner::Bind(TypedPattern::Var(ty, id), rhs) if id.name == "full" => {
            Some((ty, &rhs.ty))
        }
        _ => None,
    });
    let (pattern_ty, rhs_ty) = full_pattern.expect("top-level binding retains its typed pattern");
    assert!(
        !scar::type_contains_unresolved_vars(pattern_ty),
        "specialized binding pattern kept unresolved constructor variables: {pattern_ty:?}; RHS {rhs_ty:?}"
    );
}
