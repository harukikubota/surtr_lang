fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).expect("parse");
    scar::typecheck(sigil::resolve(ast).expect("resolve"))
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
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor {}
defenum Either<$L, $R> { Pair($L, $R), }
impl Functor for Either<$L, $R> where $R: Functor.$A {}
impl Monad for Either<$L, $R> where $R: Monad.$A {}
def accept(a: Functor<Int>, b: Monad<Boolean>) -> Unit { () }
"#;

#[test]
fn payload_changes_preserve_captured_argument() {
    check(&format!(
        "{FAMILY}\naccept(Either::Pair(\"left\", 1), Either::Pair(\"left\", True))"
    ))
    .expect("mapped payloads are independent");
}

#[test]
fn different_captured_arguments_conflict() {
    let error = check(&format!(
        "{FAMILY}\naccept(Either::Pair(\"left\", 1), Either::Pair(2, True))"
    ))
    .expect_err("captured arguments belong to carrier identity");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::TypeConstructorFamilyMismatch),
        "{error}"
    );
}

#[test]
fn unrelated_families_allow_distinct_carriers() {
    check(
        r#"
deftrait Monad where Self: Type<$A> {}
deftrait Monad2 where Self: Type<$A> {}
defenum Box<$T> { Box($T), }
impl Monad for List<$T> where $T: Monad.$A {}
impl Monad2 for Box<$T> where $T: Monad2.$A {}
def accept(a: Monad<Int>, b: Monad2<Boolean>) -> Unit { () }
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
fn every_mapped_slot_identity_and_position_must_agree() {
    let error = check(
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
accept(Pair::Pair(1, "a"), Pair::Pair("b", 2))
"#,
    )
    .expect_err("same family must reject swapped mapped-slot positions");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::TypeConstructorFamilyMismatch),
        "{error}"
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
fn shared_family_keeps_each_positions_required_capability() {
    let declarations = r#"
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl Functor for Box<$T> {}
impl Monad for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
"#;
    check(&format!("{declarations}\ndef use(a: Functor<Int>, b: Monad<Int>) -> Int {{ Monad::run(b) }}\nuse(Box::Box(1), Box::Box(2))"))
        .expect("the Monad position keeps its own capability");
    check(&format!("{declarations}\ndef use(a: Functor<Int>, b: Monad<Int>) -> Int {{ Monad::run(a) }}\nuse(Box::Box(1), Box::Box(2))"))
        .expect_err("sharing a carrier must not grant Monad capability to a Functor position");
}

#[test]
fn unannotated_alias_preserves_constructor_capability() {
    let error = check(
        r#"
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl Functor for Box<$T> {}
impl Monad for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def use(a: Functor<Int>, b: Monad<Int>) -> Int {
  alias = a
  Monad::run(alias)
}
use(Box::Box(1), Box::Box(2))
"#,
    )
    .expect_err("aliasing must not grant a stronger constructor capability");
    assert!(
        error
            .to_string()
            .contains("Monad::run is not available for a value constrained by Functor"),
        "{error}"
    );
}

#[test]
fn generic_identity_result_preserves_constructor_capability() {
    let error = check(
        r#"
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl Functor for Box<$T> {}
impl Monad for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def identity(value: $T) -> $T { value }
def use(a: Functor<Int>) -> Int {
  alias = identity(a)
  Monad::run(alias)
}
use(Box::Box(1))
"#,
    )
    .expect_err("generic value forwarding must not grant a stronger capability");
    assert!(
        error
            .to_string()
            .contains("Monad::run is not available for a value constrained by Functor"),
        "{error}"
    );
}

#[test]
fn multi_root_forwarding_rejects_unsupported_stronger_capability() {
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
def choose(first: $T, second: $T) -> $T { first }
def use(a: Left<Int>, b: Right<Int>) -> Int {
  alias = choose(a, b)
  Stronger::run(alias)
}
use(Box::Box(1), Box::Box(2))
"#,
    )
    .expect_err("forwarding must retain every incomparable common capability");
    assert!(
        error.to_string().contains(
            "Stronger::run is not available for a value constrained by FirstRoot + SecondRoot"
        ),
        "{error}"
    );
}

#[test]
fn multi_root_forwarding_keeps_all_common_parent_capabilities() {
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
def choose(first: $T, second: $T) -> $T { first }
def use(a: Left<Int>, b: Right<Int>) -> Int {
  alias = choose(a, b)
  first = FirstRoot::first(alias)
  SecondRoot::second(alias)
}
use(Box::Box(1), Box::Box(2))
"#,
    )
    .expect("forwarding retains methods from every guaranteed common parent");
}

#[test]
fn identical_forwarded_sources_keep_their_stronger_capability() {
    check(
        r#"
deftrait Root where Self: Type<$A> {}
deftrait Stronger where Self: Root { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl Root for Box<$T> {}
impl Stronger for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def choose(first: $T, second: $T) -> $T { first }
def use(a: Stronger<Int>, b: Stronger<Int>) -> Int {
  alias = choose(a, b)
  Stronger::run(alias)
}
use(Box::Box(1), Box::Box(2))
"#,
    )
    .expect("identical constrained sources retain their stronger capability");
}

#[test]
fn empty_guarantee_intersection_remains_distinct_from_an_unrestricted_value() {
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
def choose(first: $T, second: $T) -> $T { first }
def stronger(value: Stronger<Int>) -> Int { Stronger::run(value) }
"#;
    let error = check(&format!(
        r#"{declarations}
def use(a: Left<Int>, b: Right<Int>) -> Int {{
  alias = choose(a, b)
  stronger(alias)
}}
use(Box::Box(1), Box::Box(2))"#
    ))
    .expect_err("an empty guarantee intersection remains constrained");
    assert!(
        error
            .to_string()
            .contains("function requires a value constrained by Stronger, got a constrained value with no guaranteed constructor capability"),
        "{error}"
    );
}

#[test]
fn ordinary_callable_argument_checks_constructor_capability() {
    let error = check(
        r#"
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl Functor for Box<$T> {}
impl Monad for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def stronger(value: Monad<Int>) -> Int { Monad::run(value) }
def use(a: Functor<Int>) -> Int { stronger(a) }
use(Box::Box(1))
"#,
    )
    .expect_err("ordinary calls must enforce the parameter's constructor capability");
    assert!(
        error.to_string().contains(
            "function requires a value constrained by Monad, got a value constrained by Functor"
        ),
        "{error}"
    );
}

#[test]
fn every_trait_method_constructor_argument_checks_capability() {
    let error = check(
        r#"
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor {
  def combine(left: Self<Int>, right: Self<Int>) -> Int
}
defenum Box<$T> { Box($T), }
impl Functor for Box<$T> {}
impl Monad for Box<$T> {
  def combine(left: Self<Int>, right: Self<Int>) -> Int { 1 }
}
def use(a: Functor<Int>, b: Monad<Int>) -> Int { Monad::combine(b, a) }
use(Box::Box(1), Box::Box(2))
"#,
    )
    .expect_err("each Self argument must satisfy the method trait capability");
    assert!(
        error
            .to_string()
            .contains("Monad::combine is not available for a value constrained by Functor"),
        "{error}"
    );
}

#[test]
fn bare_occurrences_keep_mapped_payloads_independent() {
    check(
        r#"
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor {}
impl Functor for List<$T> {}
impl Monad for List<$T> {}
def accept(a: Functor, b: Monad) -> Unit { () }
accept([1], [True])
"#,
    )
    .expect("bare occurrences compare the carrier without equating mapped payloads");
}

#[test]
fn repeated_bare_trait_occurrences_keep_mapped_payloads_independent() {
    check(
        r#"
deftrait Functor where Self: Type<$A> {}
impl Functor for List<$T> {}
def accept(a: Functor, b: Functor) -> Unit { () }
accept([1], [True])
"#,
    )
    .expect("reusing one bare witness still ignores mapped payload values");
}

#[test]
fn bare_occurrences_unify_captured_inference_variables() {
    check(
        r#"
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor {}
defenum Carrier<$L, $R> { Pair($L, $R), }
impl Functor for Carrier<$L, $R> where $R: Functor.$A {}
impl Monad for Carrier<$L, $R> where $R: Monad.$A {}
def accept(a: Functor, b: Monad) -> Unit { () }
accept(Carrier::Pair([], 1), Carrier::Pair(["left"], 2))
"#,
    )
    .expect("the later occurrence may determine an earlier captured inference variable");
}

#[test]
fn bare_occurrences_still_compare_every_mapped_slot() {
    let error = check(
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
    .expect_err("bare occurrences must reject reversed mapped-slot positions");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::TypeConstructorFamilyMismatch),
        "{error}"
    );
}

#[test]
fn same_family_result_cannot_replace_the_input_carrier() {
    check(
        r#"
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor {}
defenum Box<$T> { Box($T), }
impl Functor for List<$T> {}
impl Monad for List<$T> {}
impl Functor for Box<$T> {}
impl Monad for Box<$T> {}
def replace(value: Functor<Int>) -> Monad<String> { Box::Box("wrong") }
result: Box<String> = replace([1])
"#,
    )
    .expect_err("return and input occurrences share the same family carrier");
}
