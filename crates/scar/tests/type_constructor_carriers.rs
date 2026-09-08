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
