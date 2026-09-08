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
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    let data = error.structured.as_ref().unwrap().data_json();
    assert!(data["family_id"].as_str().unwrap().starts_with("family:"));
    assert_eq!(data["required_capability"], "Monad");
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
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    let data = error.structured.as_ref().unwrap().data_json();
    assert!(data["family_id"].as_str().unwrap().starts_with("family:"));
    assert_eq!(data["required_capability"], "Monad");
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
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    let data = error.structured.as_ref().unwrap().data_json();
    assert!(data["family_id"].as_str().unwrap().starts_with("family:"));
    assert_eq!(data["required_capability"], "Monad");
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
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    let data = error.structured.as_ref().unwrap().data_json();
    assert!(data["family_id"].as_str().unwrap().starts_with("family:"));
    assert_eq!(data["required_capability"], "Monad");
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

#[test]
fn family_diagnostic_identity_and_full_types_do_not_depend_on_registration_order() {
    let mut identities = Vec::new();
    for definitions in [
        "deftrait Left where Self: Type<$A> {}\ndeftrait Right where Self: Type<$A> {}",
        "deftrait Right where Self: Type<$A> {}\ndeftrait Left where Self: Type<$A> {}",
    ] {
        let source = format!(
            r#"
{definitions}
deftrait Joined where Self: Left + Right {{}}
defenum Carrier<$L, $R> {{ Pair($L, $R), }}
impl Left for Carrier<$L, $R> where $R: Left.$A {{}}
impl Right for Carrier<$L, $R> where $R: Right.$A {{}}
def accept(first: Left<Int>, second: Right<Boolean>) -> Unit {{ () }}
accept(Carrier::Pair("left", 1), Carrier::Pair(2, True))
"#
        );
        let error = check(&source).expect_err("captured argument changed");
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::TypeConstructorFamilyMismatch)
        );
        let diagnostic = error.structured.unwrap();
        let json = diagnostic.data_json();
        assert_eq!(json["left_type"], "Carrier<String, Int>");
        assert_eq!(json["right_type"], "Carrier<Int, Boolean>");
        identities.push(json["family_id"].as_str().unwrap().to_owned());
    }
    assert_eq!(identities[0], identities[1]);
    assert_eq!(identities[0], "family:Joined+Left+Right");
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
deftrait Functor where Self: Type<$A> {{}}
deftrait Monad where Self: Functor {{}}
defenum Box<$T> {{ Box($T), }}
impl Functor for Box<$T> {{}}
impl Monad for Box<$T> {{}}
def stronger(value: Monad<Int>) -> Int {{ 1 }}
def use(a: Functor<Int>, b: Monad<Int>) -> Int {{ choice = {expression}; stronger(choice) }}
use(Box::Box(1), Box::Box(2))
"#
        );
        check(&source.replace("a: Functor<Int>", "a: Monad<Int>"))
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
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor {}
defenum Box<$T> { Box($T), }
impl Functor for Box<$T> {}
impl Functor for List<$T> {}
impl Monad for Box<$T> {}
def stronger(value: Monad<Int>) -> Int { 1 }
"#;
    for body in [
        "stronger(value)",
        "thunk = { value }; stronger(thunk())",
        "id = {|x| x}; stronger(id(value))",
        "pair = (value, 1); match pair { (item, _) => stronger(item), }",
        "values = [value]; match values { [item] => stronger(item), _ => 0, }",
    ] {
        let source = format!(
            r#"{declarations}
def use(a: Functor<Int>) -> Int {{
    value: Box<Int> = a
    {body}
}}
"#
        );
        check(&format!(
            "{}\nuse(Box::Box(1))",
            source.replace("a: Functor<Int>", "a: Box<Int>")
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
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor {}
defenum Box<$T> { Box($T), }
impl Functor for Box<$T> {}
impl Monad for Box<$T> {}
def stronger(value: Monad<Int>) -> Int { 1 }
"#;
    let source = format!(
        r#"{declarations}
def use(a: Functor<Int>, b: Monad<Int>) -> Int {{
    pair = (a, 1)
    choice = match pair {{ (item, _) => if (True, item, b), }}
    stronger(choice)
}}
use(Box::Box(1), Box::Box(2))
"#
    );
    check(&source.replace("a: Functor<Int>", "a: Monad<Int>"))
        .expect("extracted sources retain their declared sufficient capability");
    let error = check(&source).expect_err("an unknown source cannot be omitted from intersection");
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
def retain(value: Monad<Int>) -> Monad<Int> {{ value }}
stronger(if (True, Box::Box(1), retain(Box::Box(2))))
"#
    ))
    .expect("a fresh concrete branch and a constrained return retain their common capability");
}

#[test]
fn specialized_return_views_survive_value_projections() {
    let declarations = r#"
deftrait Functor where Self: Type<$A> {}
deftrait Monad where Self: Functor {}
defenum Box<$T> { Box($T), }
impl Functor for Box<$T> {}
impl Monad for Box<$T> {}
def retain(value: Functor<Int>) -> Functor<Int> { value }
def stronger(value: Monad<Int>) -> Int { 1 }
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
        "thunk = { nums =? wrap(a); [item, ..tail] =? nums; Ok(item) }; match thunk() { Ok(item) => stronger(item), Err(_) => 0, }",
        "pair = (a, 1); match pair { (item, _) => stronger(item), }",
        "values = [a]; match values { [item] => stronger(item), _ => 0, }",
        "identity = {|value| value}; stronger(identity(a))",
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
deftrait Functor where Self: Type<$A> { def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B> }
deftrait Monad where Self: Functor {}
defenum Box<$T> { Box($T), }
impl Functor for Box<$T> { def fmap(self: Box<$A>, mapper: ($A -> $B)) -> Box<$B> { match self { Box::Box(x) => Box::Box(mapper(x)), } } }
impl Monad for Box<$T> {}
def retain(value: Functor<Int>) -> Functor<Int> { value }
def stronger(value: Monad<Int>) -> Int { 1 }
def id(value: $T) -> $T { value }
deftrait PipeApply<$A, $B> { def pipe_apply::<$B>(self: Self, value: $A) -> $B }
impl PipeApply<$A, $B> for ($A -> $B) { def pipe_apply::<$B>(self: Self, value: $A) -> $B { self(value) } }
def preserve(values: Functor<$T>) -> Functor<$T> { values }
"#;
    for expression in [
        "stronger(a |> id())",
        r#"identity = &id
stronger(identity(a))"#,
        r#"mapped: Box<Box<Int>> = Functor::fmap(Box::Box(0), {|x: Int| a})
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
deftrait Functor where Self: Type<$A> { def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B> }
deftrait Monad where Self: Functor { def return::<Self>(value: $A) -> Self<$A> }
defenum Box<$T> { Box($T), }
impl Functor for Box<$T> { def fmap(self: Box<$A>, mapper: ($A -> $B)) -> Box<$B> { match self { Box::Box(x) => Box::Box(mapper(x)), } } }
impl Monad for Box<$T> { def return::<Box<$T>>(value: $A) -> Box<$A> { Box::Box(value) } }
def retain(value: Functor<Int>) -> Functor<Int> { value }
def stronger(value: Monad<Int>) -> Int { 1 }
def id(value: $T) -> $T { value }
def factory(sample: $T) -> ($T -> $T) { {|value| value} }
def callback_factory::<$T>() -> ((Unit -> $T) -> $T) { {|f: (Unit -> $T)| f(())} }
def preserve(values: Functor<$T>) -> Functor<$T> { values }
defenum Wrap<$T> { Wrap($T), }
impl Functor for Wrap<$T> { def fmap(self: Wrap<$A>, mapper: ($A -> $B)) -> Wrap<$B> { match self { Wrap::Wrap(x) => Wrap::Wrap(mapper(x)), } } }
impl Monad for Wrap<$T> { def return::<Wrap<$T>>(value: $A) -> Wrap<$A> { Wrap::Wrap(value) } }
"#;
    for expression in [
        r#"identity = factory(Box::Box(0)); stronger(identity(a))"#,
        r#"apply: ((Unit -> Box<Int>) -> Box<Int>) = callback_factory(); stronger(apply({|u: Unit| a}))"#,
        r#"wrapped: Wrap<Box<Int>> = Monad::return(a)
match wrapped { Wrap::Wrap(item) => stronger(item), }"#,
        r#"identity = if (True, &id, &id)
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
