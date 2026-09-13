// The shared helper module is compiled as part of this test target, although
// most of its helpers belong to the larger surface-test target.
#[allow(dead_code)]
mod support;

use scar::error::TypeError;
use scar::typed::TypedNode;
use sindr::policy::RuntimeSourcePolicy;

fn check(source: &str) -> Result<Vec<TypedNode>, TypeError> {
    support::typecheck_with_rules(source, RuntimeSourcePolicy::script())
}

const OPTION_T: &str = r#"
defstruct OptionT<$M, $A>
where
  $M: Monad
{
  inner: $M<Option<$A>>,
}

impl OptionT {
  def new(inner: $M<Option<$A>>) -> OptionT<$M, $A>
  where
    $M: Monad
  {
    OptionT { inner: inner }
  }
}
"#;

#[test]
fn nominal_constructor_parameter_applies_known_unary_head_in_nested_field() {
    check(&format!(
        r#"{OPTION_T}
value: OptionT<Result, Int> = OptionT(Ok(Option::Some(1)))
inner: Result<Option<Int>> = Facet::view(OptionT.inner, value)
optional: OptionT<Option, Int> = OptionT(Option::Some(Option::Some(2)))
listed: OptionT<List, Int> = OptionT([Option::Some(3)])
"#
    ))
    .expect("a bound nominal constructor parameter should apply Result to Option<Int>");
}

#[test]
fn nominal_enum_constructor_parameter_uses_the_same_bound_metadata() {
    check(
        r#"
defenum Layer<$M, $A> where $M: Monad {
  Layer($M<Option<$A>>),
}

value: Layer<Result, Int> = Layer::Layer(Ok(Option::Some(1)))
"#,
    )
    .expect("enum payloads should use the same nominal constructor parameter rules");
}

#[test]
fn nominal_enum_constructor_parameter_supports_local_constructor_trait() {
    check(
        r#"
deftrait Context where Self: Type<$A> {}
defenum Local<$A> { Local($A), }
impl Context for Local<$T> where $T: Context.$A {}

defenum Layer<$M, $A> where $M: Context {
  Layer($M<$A>),
}

value: Layer<Local, Int> = Layer::Layer(Local::Local(1))
"#,
    )
    .expect("same-unit constructor Traits should be activated before enum constructors are used");
}

#[test]
fn nominal_struct_constructor_parameter_supports_local_constructor_trait() {
    check(
        r#"
deftrait Context where Self: Type<$A> {}
defstruct Local<$A> { value: $A }
impl Local {
  def new(value: $A) -> Local<$A> { Local { value: value } }
}
impl Context for Local<$T> where $T: Context.$A {}

defstruct Layer<$M, $A> where $M: Context { value: $M<$A> }
impl Layer {
  def new(value: $M<$A>) -> Layer<$M, $A>
  where
    $M: Context
  {
    Layer { value: value }
  }
}

value: Layer<Local, Int> = Layer(Local(1))
"#,
    )
    .expect("same-unit constructor Traits should support a user-defined struct head");
}

#[test]
fn nominal_enum_constructor_parameter_rejects_wrong_slot_arity() {
    let error = check(
        r#"
defenum InvalidLayer<$M, $A, $B> where $M: Monad {
  InvalidLayer($M<$A, $B>),
}
"#,
    )
    .expect_err("enum payload constructor applications must use every declared slot");

    assert!(
        error.message.contains("requires 1 slot argument"),
        "{error:?}"
    );
}

#[test]
fn nominal_enum_constructor_parameter_rejects_head_without_capability() {
    let error = check(
        r#"
defenum Layer<$M, $A> where $M: Monad { Layer($M<$A>), }
defenum Plain<$A> { Plain($A), }
def reject(value: Layer<Plain, Int>) -> Unit { () }
"#,
    )
    .expect_err("enum nominal arguments must satisfy the declaration constructor constraint");

    assert!(
        error
            .message
            .contains("does not satisfy declaration constraint Monad"),
        "{error:?}"
    );
}

#[test]
fn nominal_constructor_parameter_rejects_wrong_slot_arity() {
    let error = check(
        r#"
defstruct Invalid<$M, $A, $B> where $M: Monad {
  inner: $M<$A, $B>,
}
"#,
    )
    .expect_err("the constructor application must use every declared slot");

    assert!(
        error.message.contains("requires 1 slot argument"),
        "{error:?}"
    );
}

#[test]
fn nominal_constructor_parameter_does_not_treat_underscore_as_constructor_inference() {
    let error = check(&format!(
        r#"{OPTION_T}
def reject(value: OptionT<_, Int>) -> Unit {{ () }}"#
    ))
    .expect_err("a normal type annotation must not infer a constructor parameter from `_`");
    assert!(
        error.message.contains("`_` is only allowed"),
        "unexpected error: {error}"
    );
}

#[test]
fn nominal_constructor_application_requires_constructor_trait_bound() {
    let error = check(
        r#"
deftrait Marker {}
defstruct Invalid<$M, $A> where $M: Marker {
  value: $M<$A>,
}
"#,
    )
    .expect_err("an ordinary declaration constraint cannot authorize constructor application");

    assert!(
        error.message.contains("Marker is not a constructor trait"),
        "{error:?}"
    );
}

#[test]
fn nominal_constructor_parameter_requires_explicit_rigid_bound() {
    let error = check(&format!(
        r#"{OPTION_T}
def keep(value: OptionT<$M, $A>) -> OptionT<$M, $A> {{ value }}
"#
    ))
    .expect_err("using the nominal type must not infer a Monad bound for $M");

    assert!(
        error
            .message
            .contains("does not satisfy declaration constraint Monad"),
        "{error:?}"
    );
}

#[test]
fn nested_nominal_declaration_requires_forwarded_constructor_bound() {
    let error = check(
        r#"
defstruct RequiresMonad<$M, $A> where $M: Monad {
  value: $A,
}

defstruct Invalid<$M> {
  value: RequiresMonad<$M, Int>,
}
"#,
    )
    .expect_err("a nested nominal type must not infer its declaration constraint from use");

    assert!(
        error
            .message
            .contains("does not satisfy declaration constraint Monad"),
        "{error:?}"
    );
}

#[test]
fn nested_nominal_declaration_accepts_forwarded_constructor_bound() {
    check(&format!(
        r#"{OPTION_T}
defstruct Wrapped<$M> where $M: Monad {{
  value: OptionT<$M, Int>,
}}
impl Wrapped {{
  def new(value: OptionT<$M, Int>) -> Wrapped<$M>
  where
    $M: Monad
  {{
    Wrapped {{ value: value }}
  }}
}}
"#
    ))
    .expect("a nested nominal type should accept the declaration's explicit bound");
}

#[test]
fn trait_method_nominal_constructor_parameter_requires_explicit_bound() {
    let error = check(&format!(
        r#"{OPTION_T}
deftrait Invalid {{
  def use(value: OptionT<$M, Int>) -> Unit
}}
"#
    ))
    .expect_err("a Trait method signature must not infer a nominal declaration constraint");

    assert!(
        error
            .message
            .contains("does not satisfy declaration constraint Monad"),
        "{error:?}"
    );
}

#[test]
fn trait_impl_target_nominal_constructor_parameter_requires_explicit_bound() {
    let error = check(&format!(
        r#"{OPTION_T}
deftrait Keep {{ def keep(self: Self) -> Self }}
impl Keep for OptionT<$M, Int> {{
  def keep(self: Self) -> Self {{ self }}
}}
"#
    ))
    .expect_err("a Trait impl target must not infer a nominal declaration constraint");

    assert!(
        error
            .message
            .contains("does not satisfy declaration constraint Monad"),
        "{error:?}"
    );
}

#[test]
fn extractor_nominal_constructor_parameter_requires_explicit_bound() {
    let error = check(&format!(
        r#"{OPTION_T}
defstruct Matchers {{}}
impl Matchers {{
  def new() -> Matchers {{ Matchers {{}} }}
  defextractor read(value: OptionT<$M, Int>) -> Option<Int> {{ Option::None }}
}}
"#
    ))
    .expect_err("an extractor signature must not defer an unbound nominal constructor parameter");

    assert!(
        error
            .message
            .contains("does not satisfy declaration constraint Monad"),
        "{error:?}"
    );

    check(&format!(
        r#"{OPTION_T}
defstruct ConcreteMatchers {{}}
impl ConcreteMatchers {{
  def new() -> ConcreteMatchers {{ ConcreteMatchers {{}} }}
  defextractor read(value: OptionT<Result, Int>) -> Option<Int> {{ Option::None }}
}}
"#
    ))
    .expect("an extractor may use a concrete constructor satisfying the declaration constraint");
}

#[test]
fn nominal_constructor_parameter_checks_return_destination_bound() {
    let error = check(&format!(
        r#"{OPTION_T}
def invalid::<$M, $A>() -> OptionT<$M, $A> {{ () }}
"#
    ))
    .expect_err("a return-only constructor variable still needs an explicit bound");

    assert!(
        error
            .message
            .contains("does not satisfy declaration constraint Monad"),
        "{error:?}"
    );
}

#[test]
fn nominal_constructor_parameter_consumes_explicit_rigid_bound_for_well_formedness() {
    check(&format!(
        r#"{OPTION_T}
def keep(value: OptionT<$M, $A>) -> OptionT<$M, $A>
where
  $M: Monad
{{
  value
}}
"#
    ))
    .expect("a where bound used to form a nominal type is not unused");
}

#[test]
fn nominal_constructor_parameter_rejects_head_without_declared_capability() {
    let error = check(&format!(
        r#"{OPTION_T}
defenum Plain<$A> {{ Plain($A), }}
def reject(value: OptionT<Plain, Int>) -> Unit {{ () }}
"#
    ))
    .expect_err("a concrete constructor head must satisfy the nominal declaration constraint");

    assert!(
        error
            .message
            .contains("does not satisfy declaration constraint Monad"),
        "{error:?}"
    );
}

#[test]
fn facet_rebuild_preserves_constructor_head_and_checks_destination_bound() {
    check(&format!(
        r#"{OPTION_T}
source: OptionT<Result, Int> = OptionT(Ok(Option::Some(1)))
updated: OptionT<Result, String> = Facet::put(OptionT.inner, source, Ok(Option::Some("one")))
"#
    ))
    .expect("Facet should rebuild the payload while preserving the Result constructor head");
}

#[test]
fn facet_rebuild_rejects_a_different_constructor_head() {
    let error = check(&format!(
        r#"{OPTION_T}
source: OptionT<Result, Int> = OptionT(Ok(Option::Some(1)))
Facet::put(OptionT.inner, source, Option::Some(Option::Some("one")))
"#
    ))
    .expect_err("Facet must not silently replace the nominal constructor parameter");

    assert!(
        error
            .message
            .contains("changes the constructor family of OptionT"),
        "{error:?}"
    );
}

#[test]
fn facet_rebuild_rejects_destination_that_violates_declaration_constraint() {
    let error = check(
        r#"
deftrait Marker {}
impl Marker for Int {}

defstruct Checked<$A> where $A: Marker {
  value: $A,
}
impl Checked {
  def new(value: $A) -> Checked<$A> { Checked { value: value } }
}

source: Checked<Int> = Checked(1)
Facet::put(Checked.value, source, "one")
"#,
    )
    .expect_err("the rebuilt nominal destination must satisfy its declaration constraint");

    assert!(
        error
            .message
            .contains("does not satisfy declaration constraint Marker"),
        "{error:?}"
    );
}
