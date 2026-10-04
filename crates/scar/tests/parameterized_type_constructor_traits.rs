#[allow(dead_code)]
mod support;

use diagnostics::TypeDiagnosticReason;
use scar::typed::{TypedInner, TypedPattern};
use sigil::resolved::Resolved;

fn resolve_with_standard_environment(source: &str) -> Vec<Resolved> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse with the standard environment");
    support::resolve_ast_with_builtin_prelude(ast)
        .expect("source should resolve with the standard environment")
}

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    support::typecheck(resolve_with_standard_environment(source))
}

const USER_MONAD_T: &str = r#"
deftrait FixtureMonad
where
  Self: Type<$A>
{}

deftrait FixtureMonadT<$M>
where
  $M: FixtureMonad
  Self: FixtureMonad
{
  def lift::<Self>(value: $M<$A>) -> Self<$A>
}

defenum Base<$A> { Base($A), }
impl FixtureMonad for Base<$T> where $T: FixtureMonad.$A {}

defstruct Wrap<$M, $A>
where
  $M: FixtureMonad
{
  inner: $M<$A>
}
impl Wrap {
  def new(inner: $M<$A>) -> Wrap<$M, $A>
  where
    $M: FixtureMonad
  {
    Wrap { inner: inner }
  }
}

impl FixtureMonad for Wrap<$M, $T>
where
  $M: FixtureMonad
  $T: FixtureMonad.$A
{}

impl FixtureMonadT<$M> for Wrap<$M, $T>
where
  $M: FixtureMonad
  $T: FixtureMonadT.$A
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
value: Wrap<Base, Int> = FixtureMonadT::lift(source)
"#
    ))
    .expect("base and transformer carriers should be inferred from independent inputs");
}

#[test]
fn full_constructor_rta_preserves_its_mapped_payload_constraint() {
    let error = check(&format!(
        r#"{USER_MONAD_T}
source: Base<Int> = Base::Base(1)
value = FixtureMonadT::lift::<Wrap<Base, String>>(source)
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
impl FixtureMonad for Other<$T> where $T: FixtureMonad.$A {{}}
source: Base<Int> = Base::Base(1)
value = FixtureMonadT::lift::<Wrap<Other, _>>(source)
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
value = FixtureMonadT::lift(source)
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
        r#"deftrait FixtureMonad
where
  Self: Type<$A>
{}
deftrait FixtureMonadT<$M>
where
  $M: FixtureMonad
  Self: FixtureMonad
{}
def invalid(value: FixtureMonadT<Result>) -> Unit { () }"#,
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
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor {}
deftrait FixtureMonadT<$M>
where
  $M: FixtureMonad
  Self: FixtureMonad
{
  def lift::<Self>(value: $M<$A>) -> Self<$A>
}

defenum Base<$A> { Base($A), }
defenum Other<$A> { Other($A), }
defenum Wrap<$A> { Wrap($A), }
impl FixtureFunctor for Base<$T> where $T: FixtureFunctor.$A {}
impl FixtureMonad for Base<$T> where $T: FixtureMonad.$A {}
impl FixtureFunctor for Other<$T> where $T: FixtureFunctor.$A {}
impl FixtureMonad for Other<$T> where $T: FixtureMonad.$A {}
impl FixtureFunctor for Wrap<$T> where $T: FixtureFunctor.$A {}
impl FixtureMonad for Wrap<$T> where $T: FixtureMonad.$A {}

impl FixtureMonadT<Base> for Wrap<$T> {
  def lift::<Wrap<$T>>(value: Base<$A>) -> Wrap<$A> {
    match value { Base::Base(item) => Wrap::Wrap(item), }
  }
}
impl FixtureMonadT<Other> for Wrap<$T> {
  def lift::<Wrap<$T>>(value: Other<$A>) -> Wrap<$A> {
    match value { Other::Other(item) => Wrap::Wrap(item), }
  }
}

source: Base<Int> = Base::Base(1)
value: Wrap<Int> = FixtureMonadT::lift(source)
"#;
    check(source).expect("the concrete input selects the matching FixtureMonadT Trait argument");
}

#[test]
fn unique_monad_impl_does_not_infer_an_underconstrained_lift_base() {
    let error = check(
        r#"
deftrait FixtureMonad where Self: Type<$A> {
  def return::<Self>(value: $A) -> Self<$A>
}
deftrait FixtureMonadT<$M>
where
  $M: FixtureMonad
  Self: FixtureMonad
{
  def lift::<Self>(value: $M<$A>) -> Self<$A>
}
defenum Base<$A> { Base($A), }
impl FixtureMonad for Base<$T> {
  def return::<Base<$T>>(value: $A) -> Base<$A> { Base::Base(value) }
}
defenum Wrap<$A> { Wrap($A), }
impl FixtureMonad for Wrap<$T> {
  def return::<Wrap<$T>>(value: $A) -> Wrap<$A> { Wrap::Wrap(value) }
}
impl FixtureMonadT<Base> for Wrap<$T> {
  def lift::<Wrap<$T>>(value: Base<$A>) -> Wrap<$A> {
    match value { Base::Base(item) => Wrap::Wrap(item), }
  }
}
value = FixtureMonadT::lift(FixtureMonad::return(1))
"#,
    )
    .expect_err("a unique FixtureMonad impl is not evidence for the base of return");
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
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor {}
deftrait FixtureMonadT<$M>
where
  $M: FixtureMonad
  Self: FixtureMonad
{
  def lift::<Self>(value: $M<$A>) -> Self<$A>
}
defenum Base<$A> { Base($A), }
impl FixtureFunctor for Base<$T> where $T: FixtureFunctor.$A {}
impl FixtureMonad for Base<$T> where $T: FixtureMonad.$A {}
defenum Wrap<$M, $A> where $M: FixtureMonad { Wrap($M<$A>), }
impl FixtureFunctor for Wrap<$M, $T>
where
  $M: FixtureMonad
  $T: FixtureFunctor.$A
{}
impl FixtureMonad for Wrap<$M, $T>
where
  $M: FixtureMonad
  $T: FixtureMonad.$A
{}
impl FixtureMonadT<$M> for Wrap<$M, $T>
where
  $M: FixtureMonad
  $T: FixtureMonadT.$A
{
  def lift::<Wrap<$M, $T>>(value: $M<$A>) -> Wrap<$M, $A> {
    Wrap::Wrap(value)
  }
}
def retain(value: $F<Int>) -> $F<Int> where $F: FixtureFunctor { value }
source: Base<Int> = Base::Base(1)
view = retain(source)
lifted: Wrap<Base, Int> = FixtureMonadT::lift(view)
"#,
    )
    .expect_err("a FixtureFunctor-only static view cannot provide FixtureMonad for lift's base");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    assert_eq!(
        error.structured.as_ref().unwrap().data_json()["required_capability"],
        "FixtureMonad"
    );
}

#[test]
fn declared_return_expectation_reaches_pipe_list_and_tuple_contents() {
    let declarations = r#"
deftrait FixtureMonad where Self: Type<$A> {
  def return::<Self>(value: $A) -> Self<$A>
}
impl FixtureMonad for List<$T> where $T: FixtureMonad.$A {
  def return::<List<$T>>(value: $A) -> List<$A> { [value] }
}
"#;
    for (return_ty, body) in [
        ("List<List<Int>>", "[FixtureMonad::return(1)]"),
        ("List<Int>", "1 |> FixtureMonad::return()"),
        ("(List<Int>, Int)", "(FixtureMonad::return(1), 0)"),
        (
            "List<Int>",
            "if (True, FixtureMonad::return(1), FixtureMonad::return(2))",
        ),
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
full = FixtureMonadT::lift::<Wrap<Base, Int>>(source)
"#
    ))
    .expect("the explicit constructor application typechecks");
    let full_pattern = typed.iter().find_map(|node| match &node.node {
        TypedInner::Bind(pattern, rhs) if matches!(pattern.unlocated(), TypedPattern::Var(_, id) if id.name == "full") => {
                let TypedPattern::Var(ty, _) = pattern.unlocated() else { unreachable!("binding guard checked"); };
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
