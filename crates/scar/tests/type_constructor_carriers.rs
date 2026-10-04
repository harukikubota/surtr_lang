#[allow(dead_code)]
mod support;

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).expect("parse");
    support::typecheck(support::resolve_ast_with_builtin_prelude(ast).expect("resolve"))
}

#[test]
fn inheritance_connects_multiple_roots_into_one_family() {
    check(
        r#"
deftrait Left where Self: Type<$A> {}
deftrait Right where Self: Type<$B> {}
deftrait Joined where Self: Left + Right {}
"#,
    )
    .expect("connected roots form one family");
}

const FAMILY: &str = r#"
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor {}
defenum FixtureEither<$L, $R> { Pair($L, $R), }
impl FixtureFunctor for FixtureEither<$L, $R> where $R: FixtureFunctor.$A {}
impl FixtureMonad for FixtureEither<$L, $R> where $R: FixtureMonad.$A {}
def accept(a: FixtureFunctor<Int>, b: FixtureMonad<Boolean>) -> Unit { () }
"#;

#[test]
fn payload_changes_preserve_captured_argument() {
    check(&format!(
        "{FAMILY}\naccept(FixtureEither::Pair(\"left\", 1), FixtureEither::Pair(\"left\", True))"
    ))
    .expect("mapped payloads are independent");
}

#[test]
fn different_direct_captured_arguments_are_independent() {
    check(&format!(
        "{FAMILY}\naccept(FixtureEither::Pair(\"left\", 1), FixtureEither::Pair(2, True))"
    ))
    .expect("separate direct parameters have independent captured carrier arguments");
}

#[test]
fn unrelated_families_allow_distinct_carriers() {
    check(
        r#"
deftrait FixtureMonad where Self: Type<$A> {}
deftrait Monad2 where Self: Type<$A> {}
defenum Box<$T> { Box($T), }
impl FixtureMonad for List<$T> where $T: FixtureMonad.$A {}
impl Monad2 for Box<$T> where $T: Monad2.$A {}
def accept(a: FixtureMonad<Int>, b: Monad2<Boolean>) -> Unit { () }
accept([1], Box::Box(True))
"#,
    )
    .expect("unrelated families carry independent constructors");
}

#[test]
fn explicit_head_leaves_captured_arguments_to_expected_return() {
    for reverse in [false, true] {
        let implementations = [
            "impl Factory for Pair<String, $T> where $T: Factory.$A { def make::<Self>() -> Pair<String, Int> { Pair::Pair(\"left\", 1) } }",
            "impl Factory for Pair<Boolean, $T> where $T: Factory.$A { def make::<Self>() -> Pair<Boolean, Int> { Pair::Pair(True, 2) } }",
        ];
        let order = if reverse { [1, 0] } else { [0, 1] };
        check(&format!(
            r#"
deftrait Factory where Self: Type<$A> {{ def make::<Self>() -> Self<Int> }}
defenum Pair<$L, $R> {{ Pair($L, $R), }}
{}
{}
def make::<Factory>() -> Factory<Int> {{ Factory::make() }}
value: Pair<String, Int> = make::<Pair>()
"#,
            implementations[order[0]], implementations[order[1]]
        ))
        .expect("expected return selects captured arguments independently of impl order");
    }
}

#[test]
fn separate_direct_parameters_do_not_share_mapped_slot_identity() {
    check(
        r#"
deftrait Left where Self: Type<$A, $B> {}
deftrait Right where Self: Type<$A, $B> {}
deftrait Joined where Self: Left + Right {}
defenum Pair<$T, $U> { Pair($T, $U), }
impl Left for Pair<$T, $U> where
  $T: Left.$A
  $U: Left.$B
{}
impl Right for Pair<$T, $U> where
  $U: Right.$A
  $T: Right.$B
{}
def accept(a: Left<Int, String>, b: Right<String, Int>) -> Unit { () }
accept(Pair::Pair(1, "a"), Pair::Pair(2, "b"))
"#,
    )
    .expect("separate direct parameters do not compare their mapped slot positions");
}

#[test]
fn repeated_named_constructor_variable_requires_one_carrier() {
    let source = r#"
deftrait Context where Self: Type<$A> {
  def keep(self: Self<$A>) -> Self<$A>
}
defenum Left<$T> { Left($T), }
defenum Right<$T> { Right($T), }
impl Context for Left<$T> where $T: Context.$A {
  def keep(self: Left<$A>) -> Left<$A> { self }
}
impl Context for Right<$T> where $T: Context.$A {
  def keep(self: Right<$A>) -> Right<$A> { self }
}
def accept(left: $F<Int>, right: $F<String>) -> Unit
where
  $F: Context
{ Context::keep(left); () }
accept(Left::Left(1), Right::Right("right"))
"#;
    let error = check(source)
        .expect_err("the same named constructor variable must preserve carrier identity");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::TypeConstructorFamilyMismatch),
        "{error:?}"
    );
    let diagnostic = error
        .structured
        .as_ref()
        .expect("structured carrier mismatch");
    let diagnostics::DiagnosticData::TypeConstructorCarrier(data) = &diagnostic.data else {
        panic!("{error:?}")
    };
    assert_eq!(data.left_type.as_deref(), Some("Left<Int>"));
    assert_eq!(data.right_type.as_deref(), Some("Right<String>"));
    let left = data.left_origin.as_ref().expect("first argument origin");
    let right = data.right_origin.as_ref().expect("second argument origin");
    let left_start = source.rfind("Left::Left(1)").unwrap();
    let right_start = source.rfind("Right::Right(\"right\")").unwrap();
    assert_eq!(left.span.start, left_start);
    assert_eq!(left.span.end, left_start + "Left::Left(1)".len());
    assert_eq!(right.span.start, right_start);
    assert_eq!(
        right.span.end,
        right_start + "Right::Right(\"right\")".len()
    );
    assert_eq!(diagnostic.primary.span, right.span);
    assert!(diagnostic.related.iter().any(|fact| fact.span == left.span));
}

#[test]
fn repeated_direct_trait_name_shares_carrier() {
    let error = check(
        r#"
def yen(value: Int) -> Int { value }
deftrait Context where Self: Type<$A> {}
defenum Carrier<$L, $R> { Pair($L, $R), }
impl Context for Carrier<$L, $R> where $R: Context.$A {}
def accept(left: Context<$A>, right: Context<$A>) -> Int { yen(1) }
accept(Carrier::Pair("left", 1), Carrier::Pair(True, 2))
"#,
    )
    .expect_err("the same direct Trait name must preserve carrier identity");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::TypeConstructorFamilyMismatch),
        "{error:?}"
    );
}

#[test]
fn shared_payload_variable_still_rejects_different_payload_types() {
    let error = check(
        r#"
deftrait Context where Self: Type<$A> {}
defenum Left<$T> { Left($T), }
defenum Right<$T> { Right($T), }
impl Context for Left<$T> where $T: Context.$A {}
impl Context for Right<$T> where $T: Context.$A {}
def accept(left: Context<$A>, right: Context<$A>) -> Unit { () }
accept(Left::Left(1), Left::Left("different"))
"#,
    )
    .expect_err("a shared payload variable still requires one payload type");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::TypePayloadMismatch),
        "{error:?}"
    );
}

#[test]
fn impl_body_self_application_uses_declared_slot_positions() {
    check(
        r#"
deftrait Factory where Self: Type<$A> { def make::<Self>() -> Self<Int> }
defenum Pair<$L, $R> { Pair($L, $R), }
impl Factory for Pair<String, $T> where $T: Factory.$A {
  def make::<Self>() -> Self<Int> { Pair::Pair("left", 1) }
}
value: Pair<String, Int> = Factory::make::<Pair>()
"#,
    )
    .expect("impl body uses the same constructor substitution as its contract");
}

#[test]
fn nested_direct_constructor_trait_application_is_rejected() {
    let error = check(
        r#"
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor {}
def invalid(value: FixtureFunctor<FixtureMonad<Int>>) -> Unit { () }
"#,
    )
    .expect_err("a TypeCtorTrait application is only valid at a direct signature position");
    assert!(
        error
            .message
            .contains("ConstructorTraitApplicationPosition"),
        "{error:?}"
    );
}

#[test]
fn self_application_preserves_phantom_captured_arguments() {
    let declarations = r#"
deftrait Replace where Self: Type<$A> {
  def replace(self: Self<$A>) -> Self<String>
}
defstruct PhantomCarrier<$Tag, $Value> { value: $Value }
impl PhantomCarrier {
  def new::<$Tag>(value: $Value) -> PhantomCarrier<$Tag, $Value> {
    PhantomCarrier { value: value }
  }
}
impl Replace for PhantomCarrier<$Tag, $Value> where $Value: Replace.$A {
  def replace(self: Self<$A>) -> Self<String> { PhantomCarrier::new::<$Tag>("replaced") }
}
input: PhantomCarrier<Int, Int> = PhantomCarrier::new::<Int>(1)
"#;

    check(&format!(
        "{declarations}\noutput: PhantomCarrier<Int, String> = Replace::replace(input)"
    ))
    .expect("Self payload replacement keeps the captured phantom argument");

    let error = check(&format!(
        "{declarations}\noutput: PhantomCarrier<Boolean, String> = Replace::replace(input)"
    ))
    .expect_err("Self payload replacement cannot change a captured phantom argument");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::ReturnTypeMismatch),
        "{error:?}"
    );
    let data = error.structured.as_ref().unwrap().data_json();
    assert_eq!(data["expected_type"], "PhantomCarrier<Boolean, String>");
    assert_eq!(data["actual_type"], "PhantomCarrier<Int, String>");
}

#[test]
fn independent_direct_positions_keep_their_declared_capabilities() {
    let declarations = r#"
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> {}
impl FixtureMonad for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
"#;
    check(&format!("{declarations}\ndef use(a: FixtureFunctor<Int>, b: FixtureMonad<Int>) -> Int {{ FixtureMonad::run(b) }}\nuse(Box::Box(1), Box::Box(2))"))
        .expect("the FixtureMonad position keeps its own capability");
    check(&format!("{declarations}\ndef use(a: FixtureFunctor<Int>, b: FixtureMonad<Int>) -> Int {{ FixtureMonad::run(a) }}\nuse(Box::Box(1), Box::Box(2))"))
        .expect_err("a concrete FixtureMonad value must not grant capability beyond the direct position's FixtureFunctor contract");
}

#[test]
fn unannotated_alias_preserves_constructor_capability() {
    let error = check(
        r#"
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> {}
impl FixtureMonad for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def use(a: FixtureFunctor<Int>, b: FixtureMonad<Int>) -> Int {
  alias = a
  FixtureMonad::run(alias)
}
use(Box::Box(1), Box::Box(2))
"#,
    )
    .expect_err("aliasing must not grant a stronger constructor capability");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    let data = error.structured.as_ref().unwrap().data_json();
    assert!(data["family_id"].as_str().unwrap().starts_with("family:"));
    assert_eq!(data["required_capability"], "FixtureMonad");
}

#[test]
fn generic_identity_result_preserves_constructor_capability() {
    let error = check(
        r#"
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> {}
impl FixtureMonad for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def identity(value: $T) -> $T { value }
def use(a: FixtureFunctor<Int>) -> Int {
  alias = identity(a)
  FixtureMonad::run(alias)
}
use(Box::Box(1))
"#,
    )
    .expect_err("generic value forwarding must not grant a stronger capability");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    let data = error.structured.as_ref().unwrap().data_json();
    assert!(data["family_id"].as_str().unwrap().starts_with("family:"));
    assert_eq!(data["required_capability"], "FixtureMonad");
}

#[test]
fn explicitly_shared_constructor_rejects_unsupported_stronger_capability() {
    let error = check(
        r#"
deftrait FirstRoot where Self: Type<$A> {}
deftrait SecondRoot where Self: Type<$B> {}
deftrait Left where Self: FirstRoot + SecondRoot {}
deftrait Right where Self: FirstRoot + SecondRoot {}
deftrait Stronger where Self: Left { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl FirstRoot for Box<$T> {}
impl SecondRoot for Box<$T> {}
impl Left for Box<$T> {}
impl Right for Box<$T> {}
impl Stronger for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def use(a: $F<Int>, b: $F<Int>) -> Int where $F: Left + Right {
  alias = if (True, a, b)
  Stronger::run(alias)
}
use(Box::Box(1), Box::Box(2))
"#,
    )
    .expect_err("forwarding must retain every incomparable common capability");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    let data = error.structured.as_ref().unwrap().data_json();
    assert!(data["family_id"].as_str().unwrap().starts_with("family:"));
    assert_eq!(data["required_capability"], "Stronger");
}

#[test]
fn explicitly_shared_constructor_keeps_all_declared_parent_capabilities() {
    check(
        r#"
deftrait FirstRoot where Self: Type<$A> { def first(self: Self<Int>) -> Int }
deftrait SecondRoot where Self: Type<$B> { def second(self: Self<Int>) -> Int }
deftrait Left where Self: FirstRoot + SecondRoot {}
deftrait Right where Self: FirstRoot + SecondRoot {}
defenum Box<$T> { Box($T), }
impl FirstRoot for Box<$T> { def first(self: Self<Int>) -> Int { 1 } }
impl SecondRoot for Box<$T> { def second(self: Self<Int>) -> Int { 2 } }
impl Left for Box<$T> {}
impl Right for Box<$T> {}
def use(a: $F<Int>, b: $F<Int>) -> Int where $F: Left + Right {
  alias = if (True, a, b)
  first = FirstRoot::first(alias)
  SecondRoot::second(alias)
}
use(Box::Box(1), Box::Box(2))
"#,
    )
    .expect("forwarding retains methods from every guaranteed common parent");
}

#[test]
fn explicitly_shared_constructor_keeps_its_stronger_capability() {
    check(
        r#"
deftrait Root where Self: Type<$A> {}
deftrait Stronger where Self: Root { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl Root for Box<$T> {}
impl Stronger for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def use(a: $F<Int>, b: $F<Int>) -> Int where $F: Stronger {
  alias = if (True, a, b)
  Stronger::run(alias)
}
use(Box::Box(1), Box::Box(2))
"#,
    )
    .expect("identical constrained sources retain their stronger capability");
}

#[test]
fn declared_capability_set_remains_distinct_from_an_unrestricted_value() {
    check(
        r#"
deftrait Root where Self: Type<$A> {}
deftrait Stronger where Self: Root { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl Root for Box<$T> {}
impl Stronger for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def stronger(value: Stronger<Int>) -> Int { Stronger::run(value) }
stronger(Box::Box(1))
"#,
    )
    .expect("an ordinary concrete value has no provenance restriction");

    let declarations = r#"
deftrait FirstRoot where Self: Type<$A> {}
deftrait SecondRoot where Self: Type<$B> {}
deftrait Left where Self: FirstRoot {}
deftrait Right where Self: SecondRoot {}
deftrait Bridge where Self: Left + Right {}
deftrait Stronger where Self: Left { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl FirstRoot for Box<$T> {}
impl SecondRoot for Box<$T> {}
impl Left for Box<$T> {}
impl Right for Box<$T> {}
impl Stronger for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def stronger(value: Stronger<Int>) -> Int { Stronger::run(value) }
"#;
    let error = check(&format!(
        r#"{declarations}
def use(a: $F<Int>, b: $F<Int>) -> Int where $F: Left + Right {{
  alias = if (True, a, b)
  stronger(alias)
}}
use(Box::Box(1), Box::Box(2))"#
    ))
    .expect_err("an explicit constructor variable retains only its declared capabilities");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    let data = error.structured.as_ref().unwrap().data_json();
    assert!(data["family_id"].as_str().unwrap().starts_with("family:"));
    assert_eq!(data["required_capability"], "Stronger");
}

#[test]
fn ordinary_callable_argument_checks_constructor_capability() {
    let error = check(
        r#"
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> {}
impl FixtureMonad for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def stronger(value: FixtureMonad<Int>) -> Int { FixtureMonad::run(value) }
def use(a: FixtureFunctor<Int>) -> Int { stronger(a) }
use(Box::Box(1))
"#,
    )
    .expect_err("ordinary calls must enforce the parameter's constructor capability");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    let data = error.structured.as_ref().unwrap().data_json();
    assert!(data["family_id"].as_str().unwrap().starts_with("family:"));
    assert_eq!(data["required_capability"], "FixtureMonad");
}

#[test]
fn every_trait_method_constructor_argument_checks_capability() {
    let error = check(
        r#"
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor {
  def combine(left: Self<Int>, right: Self<Int>) -> Int
}
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> {}
impl FixtureMonad for Box<$T> {
  def combine(left: Self<Int>, right: Self<Int>) -> Int { 1 }
}
def use(a: FixtureFunctor<Int>, b: FixtureMonad<Int>) -> Int { FixtureMonad::combine(b, a) }
use(Box::Box(1), Box::Box(2))
"#,
    )
    .expect_err("each Self argument must satisfy the method trait capability");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    let data = error.structured.as_ref().unwrap().data_json();
    assert!(data["family_id"].as_str().unwrap().starts_with("family:"));
    assert_eq!(data["required_capability"], "FixtureMonad");
}

#[test]
fn bare_occurrences_keep_mapped_payloads_independent() {
    check(
        r#"
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor {}
impl FixtureFunctor for List<$T> {}
impl FixtureMonad for List<$T> {}
def accept(a: FixtureFunctor, b: FixtureMonad) -> Unit { () }
accept([1], [True])
"#,
    )
    .expect("bare occurrences compare the carrier without equating mapped payloads");
}

#[test]
fn repeated_bare_trait_occurrences_keep_mapped_payloads_independent() {
    check(
        r#"
deftrait FixtureFunctor where Self: Type<$A> {}
impl FixtureFunctor for List<$T> {}
def accept(a: FixtureFunctor, b: FixtureFunctor) -> Unit { () }
accept([1], [True])
"#,
    )
    .expect("repeated bare direct occurrences have independent witnesses and payloads");
}

#[test]
fn bare_occurrences_infer_captured_arguments_independently() {
    check(
        r#"
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor {}
defenum Carrier<$L, $R> { Pair($L, $R), }
impl FixtureFunctor for Carrier<$L, $R> where $R: FixtureFunctor.$A {}
impl FixtureMonad for Carrier<$L, $R> where $R: FixtureMonad.$A {}
def accept(a: FixtureFunctor, b: FixtureMonad) -> Unit { () }
accept(Carrier::Pair([True], 1), Carrier::Pair(["left"], 2))
"#,
    )
    .expect("each bare occurrence determines its own captured argument");
}

#[test]
fn bare_direct_occurrences_do_not_compare_mapped_slots() {
    check(
        r#"
deftrait Left where Self: Type<$A, $B> {}
deftrait Right where Self: Type<$A, $B> {}
deftrait Joined where Self: Left + Right {}
defenum Pair<$T, $U> { Pair($T, $U), }
impl Left for Pair<$T, $U> where
  $T: Left.$A
  $U: Left.$B
{}
impl Right for Pair<$T, $U> where
  $U: Right.$A
  $T: Right.$B
{}
def accept(a: Left, b: Right) -> Unit { () }
accept(Pair::Pair(1, "a"), Pair::Pair(2, "b"))
"#,
    )
    .expect("bare direct occurrences are independent even within one family");
}

#[test]
fn different_direct_trait_names_keep_result_independent() {
    check(
        r#"
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor {}
defenum Box<$T> { Box($T), }
impl FixtureFunctor for List<$T> {}
impl FixtureMonad for List<$T> {}
impl FixtureFunctor for Box<$T> {}
impl FixtureMonad for Box<$T> {}
def replace(value: FixtureFunctor<Int>) -> FixtureMonad<String> { Box::Box("wrong") }
result: Box<String> = replace([1])
"#,
    )
    .expect("a different direct Trait name chooses its carrier independently");
}

#[test]
fn different_direct_trait_parameters_do_not_depend_on_registration_order() {
    for trait_definitions in [
        "deftrait Left where Self: Type<$A> {}\ndeftrait Right where Self: Type<$A> {}",
        "deftrait Right where Self: Type<$A> {}\ndeftrait Left where Self: Type<$A> {}",
    ] {
        for implementations in [
            "impl Left for Carrier<$L, $R> where $R: Left.$A {}\nimpl Right for Carrier<$L, $R> where $R: Right.$A {}",
            "impl Right for Carrier<$L, $R> where $R: Right.$A {}\nimpl Left for Carrier<$L, $R> where $R: Left.$A {}",
        ] {
            for (parameters, arguments) in [
                (
                    "first: Left<Int>, second: Right<Boolean>",
                    "Carrier::Pair(\"left\", 1), Carrier::Pair(2, True)",
                ),
                (
                    "second: Right<Boolean>, first: Left<Int>",
                    "Carrier::Pair(2, True), Carrier::Pair(\"left\", 1)",
                ),
            ] {
                let source = format!(
                    r#"
{trait_definitions}
deftrait Joined where Self: Left + Right {{}}
defenum Carrier<$L, $R> {{ Pair($L, $R), }}
{implementations}
def accept({parameters}) -> Unit {{ () }}
accept({arguments})
"#
                );
                check(&source).expect(
                    "different direct Trait names are independent of definition and argument order",
                );
            }
        }
    }
}

#[test]
fn zero_argument_helper_checks_explicit_payload_against_expected_return() {
    let error = check(
        r#"
deftrait Factory where Self: Type<$A> { def make::<Self, $T>() -> Self<$T> }
impl Factory for List<$A> { def make::<Self, $T>() -> List<$T> { [] } }
value: List<String> = Factory::make::<List, Int>()
"#,
    )
    .expect_err("explicit Int payload cannot be replaced by expected String");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::ReturnTypeArgumentMismatch),
        "{error:?}"
    );
}

#[test]
fn helper_operator_and_capture_preserve_the_same_constructor_relation() {
    let declarations = r#"
defenum Box<$Value> { Box($Value), }
impl Functor for Box<$Value> {
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B> {
    match self { Box::Box(value) => Box::Box(mapper(value)), }
  }
}
"#;

    for expression in [
        "Functor::fmap(Box::Box(1), {|value: Int| \"mapped\"})",
        "Box::Box(1) |*> {|value: Int| \"mapped\"}",
    ] {
        check(&format!(
            "{declarations}\nvalue: Box<String> = {expression}"
        ))
        .unwrap_or_else(|error| panic!("{expression} must keep Box: {error:?}"));
        check(&format!(
            "{declarations}\nvalue: List<String> = {expression}"
        ))
        .expect_err("helper/operator result must not change Box into List");
    }

    check(&format!(
        r#"{declarations}
mapper: (Box<Int>, (Int -> String) -> Box<String>) = &Functor::fmap
value: Box<String> = mapper(Box::Box(1), {{|value: Int| "mapped"}})"#
    ))
    .expect("a qualified Trait method capture keeps its Self carrier");

    check(&format!(
        "{declarations}\nmapper: (Box<Int>, (Int -> String) -> List<String>) = &Functor::fmap"
    ))
    .expect_err("a qualified Trait method capture cannot change its Self carrier");
}

#[test]
fn concrete_constructor_capability_requires_impl_constraints() {
    let declarations = r#"
deftrait Marker { def mark(self: Self) -> Int }
deftrait Family where Self: Type<$A> { def mark(self: Self<Int>) -> Int }
defenum Plain { Plain(Int), }
defenum Marked { Marked(Int), }
impl Marker for Marked { def mark(self: Self) -> Int { 1 } }
defenum Pair<$L, $R> { Pair($L, $R), }
impl Family for Pair<$L, $R> where
  $L: Marker
  $R: Family.$A
{ def mark(self: Self<Int>) -> Int { match self { Pair::Pair(left, _) => Marker::mark(left), } } }
def accept(value: Family<Int>) -> Unit { () }
"#;
    check(&format!(
        "{declarations}\naccept(Pair::Pair(Marked::Marked(1), 2))"
    ))
    .expect("satisfied captured argument constraints permit projection");
    check(&format!(
        "{declarations}\naccept(Pair::Pair(Plain::Plain(1), 2))"
    ))
    .expect_err("constructor projection must prove the captured argument constraint");
}

#[test]
fn branch_and_block_results_preserve_constructor_capability() {
    for expression in [
        "if (True, a, a)",
        "if (True, a, b)",
        "if (True, { a }, { a })",
        "if (True, { alias = a; alias }, { a })",
        "match 1 { 1 => a, _ => a, }",
        "match 1 { 1 => a, _ => b, }",
    ] {
        let source = format!(
            r#"
deftrait FixtureFunctor where Self: Type<$A> {{}}
deftrait FixtureMonad where Self: FixtureFunctor {{ def run(self: Self<Int>) -> Int }}
defenum Box<$T> {{ Box($T), }}
impl FixtureFunctor for Box<$T> {{}}
impl FixtureMonad for Box<$T> {{ def run(self: Self<Int>) -> Int {{ 1 }} }}
def stronger(value: FixtureMonad<Int>) -> Int {{ 1 }}
def use(a: $F<Int>, b: $F<Int>) -> Int where $F: FixtureFunctor {{ evidence = FixtureMonad::run(a); choice = {expression}; stronger(choice) }}
use(Box::Box(1), Box::Box(2))
"#
        );
        check(&source.replace("$F: FixtureFunctor", "$F: FixtureMonad"))
            .unwrap_or_else(|error| panic!("{expression}: sufficient capability: {error:?}"));
        let error = check(&source)
            .expect_err("derived expressions must preserve the input capability restriction");
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
            "{expression}: {error:?}"
        );
    }
}

#[test]
fn nominal_annotation_cannot_choose_an_abstract_constructor() {
    let declarations = r#"
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> {}
impl FixtureFunctor for List<$T> {}
impl FixtureMonad for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def stronger(value: FixtureMonad<Int>) -> Int { 1 }
"#;
    for body in [
        "stronger(value)",
        "thunk = { value }; stronger(thunk())",
        "id: (Box<Int> -> Box<Int>) = {|x| x}; stronger(id(value))",
        "pair = (value, 1); match pair { (item, _) => stronger(item), }",
        "values = [value]; match values { [item] => stronger(item), _ => 0, }",
    ] {
        let source = format!(
            r#"{declarations}
def use(a: FixtureFunctor<Int>) -> Int {{
    value: Box<Int> = a
    {body}
}}
"#
        );
        check(&format!(
            "{}\nuse(Box::Box(1))",
            source.replace("a: FixtureFunctor<Int>", "a: Box<Int>")
        ))
        .expect("an already concrete carrier can retain its nominal annotation");
        for input in ["Box::Box(1)", "[1]"] {
            let error = check(&format!("{source}\nuse({input})"))
                .expect_err("a generic constructor cannot be asserted to have a nominal head");
            assert_eq!(
                error.reason(),
                Some(diagnostics::TypeDiagnosticReason::AnnotationTypeMismatch),
                "{body}, {input}: {error:?}"
            );
        }
    }
}

#[test]
fn provenance_intersection_proves_every_extracted_source() {
    let declarations = r#"
deftrait FixtureFunctor where Self: Type<$A> {}
deftrait FixtureMonad where Self: FixtureFunctor { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> {}
impl FixtureMonad for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def stronger(value: FixtureMonad<Int>) -> Int { 1 }
"#;
    let source = |capability: &str| {
        format!(
            r#"{declarations}
def use(a: $F<Int>, b: $F<Int>) -> Int where $F: {capability} {{
    pair = (a, 1)
    choice = match pair {{ (item, _) => if (True, item, b), }}
    evidence = FixtureMonad::run(a)
    stronger(choice)
}}
use(Box::Box(1), Box::Box(2))
"#
        )
    };
    check(&source("FixtureMonad"))
        .expect("extracted sources retain their declared sufficient capability");
    let error = check(&source("FixtureFunctor"))
        .expect_err("an unknown source cannot be omitted from intersection");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    check(&format!(
        "{declarations}\nstronger(if (True, Box::Box(1), Box::Box(2)))"
    ))
    .expect("fresh concrete branches prove their capability independently");
    check(&format!(
        r#"{declarations}
def retain(value: $F<Int>) -> $F<Int> where $F: FixtureMonad {{ evidence = FixtureMonad::run(value); value }}
stronger(if (True, Box::Box(1), retain(Box::Box(2))))
"#
    ))
    .expect("a fresh concrete branch and a constrained return retain their common capability");
}

#[test]
fn specialized_return_views_survive_value_projections() {
    let declarations = r#"
deftrait FixtureFunctor where Self: Type<$A> { def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B> }
deftrait FixtureMonad where Self: FixtureFunctor {}
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> { def fmap(self: Box<$A>, mapper: ($A -> $B)) -> Box<$B> { match self { Box::Box(x) => Box::Box(mapper(x)), } } }
impl FixtureMonad for Box<$T> {}
def retain(value: $F<Int>) -> $F<Int> where $F: FixtureFunctor { FixtureFunctor::fmap(value, {|x| x}) }
def stronger(value: FixtureMonad<Int>) -> Int { 1 }
deferror Marker { "marker" }
def wrap(value: $T) -> Result<List<$T>, Marker> { Ok([value]) }
defenum Maybe<$T> { Some($T), None, }
def head(values: List<$T>) -> Maybe<$T> {
    match values { [item, .._] => Maybe::Some(item), _ => Maybe::None, }
}
defstruct Holder<$T> { value: $T, }
impl Holder { def new(value: $T) -> Holder<$T> { Holder { value: value } } }
"#;
    for expression in [
        "nums =? wrap(a); [item, ..tail] =? nums; stronger(item)",
        "thunk: (-> Result<Box<Int>>) = { nums =? wrap(a); [item, ..tail] =? nums; Ok(item) }; match thunk() { Ok(item) => stronger(item), Err(_) => 0, }",
        "pair = (a, 1); match pair { (item, _) => stronger(item), }",
        "values = [a]; match values { [item] => stronger(item), _ => 0, }",
        "identity: (Box<Int> -> Box<Int>) = {|value| value}; stronger(identity(a))",
        "thunk = { a }; stronger(thunk())",
        "holder = Holder(a); stronger(holder.value)",
        "match head([a]) { Maybe::Some(item) => stronger(item), Maybe::None => 0, }",
    ] {
        check(&format!("{declarations}\na = Box::Box(1)\n{expression}"))
            .unwrap_or_else(|error| panic!("fresh concrete source: {expression}: {error:?}"));
        let source = format!("{declarations}\na = retain(Box::Box(1))\n{expression}");
        let error = check(&source)
            .expect_err("transparent projection must preserve a declared return view");
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
            "{expression}: {error:?}"
        );
    }
}

#[test]
fn specialized_return_views_follow_generic_callable_dependencies() {
    let declarations = r#"
deftrait FixtureFunctor where Self: Type<$A> { def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B> }
deftrait FixtureMonad where Self: FixtureFunctor {}
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> { def fmap(self: Box<$A>, mapper: ($A -> $B)) -> Box<$B> { match self { Box::Box(x) => Box::Box(mapper(x)), } } }
impl FixtureMonad for Box<$T> {}
def retain(value: $F<Int>) -> $F<Int> where $F: FixtureFunctor { FixtureFunctor::fmap(value, {|x| x}) }
def stronger(value: FixtureMonad<Int>) -> Int { 1 }
def id(value: $T) -> $T { value }
def preserve(values: $F<$T>) -> $F<$T> where $F: FixtureFunctor { FixtureFunctor::fmap(values, {|x| x}) }
"#;
    for expression in [
        "stronger(a |> id())",
        r#"identity: (Box<Int> -> Box<Int>) = &id
stronger(identity(a))"#,
        r#"mapped: Box<Box<Int>> = FixtureFunctor::fmap(Box::Box(0), {|x: Int| a})
match mapped { Box::Box(item) => stronger(item), }"#,
        r#"wrapped = preserve(Box::Box(a))
match wrapped { Box::Box(item) => stronger(item), }"#,
    ] {
        check(&format!("{declarations}\na = Box::Box(1)\n{expression}"))
            .unwrap_or_else(|error| panic!("fresh concrete source: {expression}: {error:?}"));
        let source = format!("{declarations}\na = retain(Box::Box(1))\n{expression}");
        let error =
            check(&source).expect_err("generic callable results must preserve payload views");
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
            "{expression}: {error:?}"
        );
    }
}

#[test]
fn specialized_return_views_survive_receiverless_and_joined_calls() {
    let declarations = r#"
deftrait FixtureFunctor where Self: Type<$A> { def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B> }
deftrait FixtureMonad where Self: FixtureFunctor { def return::<Self>(value: $A) -> Self<$A> }
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> { def fmap(self: Box<$A>, mapper: ($A -> $B)) -> Box<$B> { match self { Box::Box(x) => Box::Box(mapper(x)), } } }
impl FixtureMonad for Box<$T> { def return::<Box<$T>>(value: $A) -> Box<$A> { Box::Box(value) } }
def retain(value: $F<Int>) -> $F<Int> where $F: FixtureFunctor { FixtureFunctor::fmap(value, {|x| x}) }
def stronger(value: FixtureMonad<Int>) -> Int { 1 }
def id(value: $T) -> $T { value }
def factory(sample: $T) -> ($T -> $T) { {|value| value} }
def callback_factory::<$T>() -> ((Unit -> $T) -> $T) { {|f: (Unit -> $T)| f(())} }
def preserve(values: $F<$T>) -> $F<$T> where $F: FixtureFunctor { FixtureFunctor::fmap(values, {|x| x}) }
defenum Wrap<$T> { Wrap($T), }
impl FixtureFunctor for Wrap<$T> { def fmap(self: Wrap<$A>, mapper: ($A -> $B)) -> Wrap<$B> { match self { Wrap::Wrap(x) => Wrap::Wrap(mapper(x)), } } }
impl FixtureMonad for Wrap<$T> { def return::<Wrap<$T>>(value: $A) -> Wrap<$A> { Wrap::Wrap(value) } }
deftrait Nest where Self: Type<$A> { def wrap::<Self>(values: List<$A>) -> Self<List<$A>> }
impl Nest for Wrap<$T> { def wrap::<Wrap<$T>>(values: List<$A>) -> Wrap<List<$A>> { Wrap::Wrap(values) } }
"#;
    for expression in [
        r#"identity = factory(Box::Box(0)); stronger(identity(a))"#,
        r#"apply: ((Unit -> Box<Int>) -> Box<Int>) = callback_factory(); stronger(apply({|u: Unit| a}))"#,
        r#"wrapped: Wrap<Box<Int>> = FixtureMonad::return(a)
match wrapped { Wrap::Wrap(item) => stronger(item), }"#,
        r#"nested: Wrap<List<Box<Int>>> = Nest::wrap([a])
match nested {
  Wrap::Wrap(items) => match items {
    [item, .._] => stronger(item),
    [] => 0,
  },
}"#,
        r#"identity: (Box<Int> -> Box<Int>) = if (True, &id, &id)
stronger(identity(a))"#,
    ] {
        check(&format!("{declarations}\na = Box::Box(1)\n{expression}"))
            .unwrap_or_else(|error| panic!("fresh concrete source: {expression}: {error:?}"));
        let source = format!("{declarations}\na = retain(Box::Box(1))\n{expression}");
        let error =
            check(&source).expect_err("receiverless and joined calls must preserve payload views");
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
            "{expression}: {error:?}"
        );
    }
}

#[test]
fn constructor_method_result_provenance_uses_the_declared_self_receiver() {
    let declarations = r#"
deftrait FixtureFunctor where Self: Type<$A> { def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B> }
deftrait FixtureMonad where Self: FixtureFunctor {}
deftrait Extract where Self: Type<$A> { def extract(self: Self<$A>) -> $A }
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> { def fmap(self: Box<$A>, mapper: ($A -> $B)) -> Box<$B> { match self { Box::Box(x) => Box::Box(mapper(x)), } } }
impl FixtureMonad for Box<$T> {}
defenum Outer<$T> { Outer($T), }
impl Extract for Outer<$T> { def extract(self: Outer<$A>) -> $A { match self { Outer::Outer(value) => value, } } }
def retain(value: $F<Int>) -> $F<Int> where $F: FixtureFunctor { FixtureFunctor::fmap(value, {|x| x}) }
def stronger(value: FixtureMonad<Int>) -> Int { 1 }
"#;

    check(&format!(
        "{declarations}\nwrapped = Outer::Outer(Box::Box(1))\nstronger(Extract::extract(wrapped))"
    ))
    .expect("a fresh payload projected from Self uses the receiver provenance");

    let error = check(&format!(
        "{declarations}\npayload = retain(Box::Box(1))\nwrapped = Outer::Outer(payload)\nstronger(Extract::extract(wrapped))"
    ))
    .expect_err("a projected payload must retain the receiver's restricted capability");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
}

const RESTRICTED_RETURN_PROJECTIONS: &str = r#"
deftrait ProjectionFunctor where Self: Type<$A> { def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B> }
deftrait ProjectionMonad where Self: ProjectionFunctor {}
defenum Box<$T> { Box($T), }
impl ProjectionFunctor for Box<$T> { def fmap(self: Box<$A>, mapper: ($A -> $B)) -> Box<$B> { match self { Box::Box(x) => Box::Box(mapper(x)), } } }
impl ProjectionMonad for Box<$T> {}
def retain(value: $F<Int>) -> $F<Int> where $F: ProjectionFunctor { ProjectionFunctor::fmap(value, {|x| x}) }
def stronger(value: ProjectionMonad<Int>) -> Int { 1 }
"#;

#[test]
fn captured_generic_callable_keeps_its_required_constructor_capability() {
    for capture in ["&stronger", "&stronger(&1)"] {
        check(&format!(
        "{RESTRICTED_RETURN_PROJECTIONS}\nmapper: (Box<Int> -> Int) = {capture}\nmapper(Box::Box(1))"
    ))
    .expect("fresh nominal sources meet the generic function's required capability");
        let error = check(&format!(
        "{RESTRICTED_RETURN_PROJECTIONS}\nmapper: (Box<Int> -> Int) = {capture}\nmapper(retain(Box::Box(1)))"
    ))
    .expect_err("capturing a generic function must not erase its original FixtureMonad requirement");
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
            "{error:?}"
        );
    }
}

fn check_with_uncons(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    support::typecheck(support::resolve_with_builtin_prelude(source))
}

#[test]
fn specialized_return_views_survive_extractor_payload_projections() {
    let declarations = format!(
        "{RESTRICTED_RETURN_PROJECTIONS}\nimpl Int {{ defextractor project(chosen: $T, value: Int) -> MatchResult<$T> {{ MatchResult::Ok(chosen) }} }}"
    );
    for expression in [
        "match 0 { Int::project(a, item) => stronger(item), _ => 0, }",
        "match [a] { uncons(item, _) => stronger(item), _ => 0, }",
        "uncons(item, _) =? [a]; stronger(item)",
        "match apply_pattern([a], uncons(_1, _)) { Ok(item) => stronger(item), Err(_) => 0, }",
    ] {
        check_with_uncons(&format!("{declarations}\na = Box::Box(1)\n{expression}"))
            .unwrap_or_else(|error| panic!("fresh concrete source: {expression}: {error:?}"));
        let source = format!("{declarations}\na = retain(Box::Box(1))\n{expression}");
        let error = check_with_uncons(&source).expect_err(
            "extractor output projections must preserve the input payload's return view",
        );
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
            "{expression}: {error:?}"
        );
    }
}

#[test]
fn callback_output_dependencies_preserve_transitive_input_views() {
    let declarations = format!(
        r#"{RESTRICTED_RETURN_PROJECTIONS}
def consume_chain(value: $A, first: ($A -> $B), second: ($B -> $C)) -> $C {{ second(first(value)) }}
def consume_chain_callback_first(first: ($A -> $B), second: ($B -> $C), value: $A) -> $C {{ second(first(value)) }}
def compose_reversed(value: $A, second: ($B -> $C), first: ($A -> $B)) -> $C {{ second(first(value)) }}
def independent_callbacks(value: $A, weaker: ($A -> $A), consume: ($A -> Int)) -> Int {{ consume(value) }}
"#
    );
    for expression in [
        "consume_chain(a, {|item| item}, {|item| stronger(item)})",
        "consume_chain_callback_first({|item| item}, {|item| stronger(item)}, a)",
        "b = compose_reversed(a, {|item| item}, {|item| item}); stronger(b)",
        "independent_callbacks(a, {|item| retain(item)}, {|item| stronger(item)})",
    ] {
        check(&format!("{declarations}\na = Box::Box(1)\n{expression}"))
            .unwrap_or_else(|error| panic!("fresh source: {expression}: {error:?}"));
        let error = check(&format!(
            "{declarations}\na = retain(Box::Box(1))\n{expression}"
        ))
        .expect_err("a callback output must retain restrictions from its transitive source");
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
            "{expression}: {error:?}"
        );
    }
}

#[test]
fn independently_annotated_callbacks_do_not_wait_on_each_other() {
    check(
        r#"
def accept(left: ($T -> $T), right: ($T -> $T)) -> Int { 1 }
accept({|value: Int| value}, {|value: Int| value})
"#,
    )
    .expect("explicit callback contracts establish their input types independently");
}

#[test]
fn nested_declared_constructor_requirements_survive_direct_and_captured_calls() {
    let declarations = format!(
        r#"{RESTRICTED_RETURN_PROJECTIONS}
def stronger_list(values: List<$F<Int>>) -> Int where $F: ProjectionMonad {{ 1 }}
"#
    );
    for expression in [
        "stronger_list([a])",
        "mapper: (List<Box<Int>> -> Int) = &stronger_list; mapper([a])",
        "mapper: (List<Box<Int>> -> Int) = &stronger_list(&1); mapper([a])",
    ] {
        check(&format!("{declarations}\na = Box::Box(1)\n{expression}"))
            .unwrap_or_else(|error| panic!("fresh source: {expression}: {error:?}"));
        let error = check(&format!(
            "{declarations}\na = retain(Box::Box(1))\n{expression}"
        ))
        .expect_err("nested constructor constraints must be checked on the actual payload view");
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
            "{expression}: {error:?}"
        );
    }
}
