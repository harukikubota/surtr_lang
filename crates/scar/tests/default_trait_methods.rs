use scar::typed::{TypedInner, TypedNode};
use sigil::resolved::Resolved;

fn resolve_without_std_prelude(source: &str) -> Vec<Resolved> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse without the std prelude");
    sigil::resolve(ast).expect("source should resolve without the std prelude")
}

fn typecheck_without_std_prelude(source: &str) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    scar::typecheck(resolve_without_std_prelude(source))
}

fn typecheck_std_surface_without_prelude(
    std_source: &str,
    project_source: &str,
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    let mut ast = spire::parse_with_context(
        std_source,
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
    )
    .expect("standard surface should parse without the std prelude");
    ast.extend(
        spire::parse_with_context(project_source, spire::ParserContext::project(1))
            .expect("project source should parse without the std prelude"),
    );
    let resolved =
        sigil::resolve(ast).expect("standard surface should resolve without the prelude");
    scar::typecheck(resolved)
}

#[test]
fn synthesized_trait_default_does_not_require_an_explicit_resolved_impl_method() {
    let typed = typecheck_without_std_prelude(
        r#"deftrait Choice {
  def choose(self: Self) -> Self

  def fallback(self: Self) -> Self { Choice::choose(self) }
}

impl Choice for Int {
  def choose(self: Int) -> Int { self }
}

value: Int = Choice::fallback(1)"#,
    )
    .expect("the omitted fallback method must be synthesized from its trait default");

    assert!(typed.iter().any(|node| {
        matches!(
            &node.node,
            TypedInner::Def(_, id, _, _, _, _, _, _)
                if id.compiler_generated && id.name == "fallback"
        )
    }));
}

#[test]
fn impl_block_capability_consumed_by_one_explicit_method_survives_default_synthesis() {
    typecheck_without_std_prelude(
        r#"defenum Verdict {
  Same,
}

deftrait Equal {
  def equal(self: Self, rhs: Self) -> Verdict

  def compare(self: Self, rhs: Self) -> Verdict { Equal::equal(self, rhs) }
}

impl Equal for Int {
  def equal(self: Int, rhs: Int) -> Verdict { Verdict::Same }
}

impl Equal for ($A, Int)
where
  $A: Equal
{
  def equal(self: Self, rhs: Self) -> Verdict {
    Equal::equal(self._0, rhs._0)
  }
}"#,
    )
    .expect("the impl-block capability is consumed by the explicit equal method");
}

#[test]
fn bare_impl_capability_defers_candidate_proof_to_the_full_body_obligation() {
    typecheck_without_std_prelude(
        r#"deftrait Marker<$Tag> {
  def mark::<$Tag>(self: Self) -> $Tag
}

deftrait Use {
  def use(self: Self) -> Int
}

defenum Box<$A> {
  Box($A),
}

impl Marker<Int> for Int {
  def mark::<Int>(self: Int) -> Int { self }
}

impl Use for Box<$A>
where
  $A: Marker
{
  def use(self: Box<$A>) -> Int {
    match self { Box::Box(value) => Marker::mark::<Int>(value) }
  }
}

value: Box<Int> = Box::Box(1)
result = Use::use(value)"#,
    )
    .expect("the body-emitted Marker<Int> obligation must prove the generic Use candidate");
}

#[test]
fn bare_impl_capability_does_not_replace_the_full_body_obligation() {
    let err = typecheck_without_std_prelude(
        r#"deftrait Marker<$Tag> {
  def mark::<$Tag>(self: Self) -> $Tag
}

deftrait Use {
  def use(self: Self) -> Int
}

defenum Box<$A> {
  Box($A),
}

impl Marker<String> for Int {
  def mark::<String>(self: Int) -> String { "wrong" }
}

impl Use for Box<$A>
where
  $A: Marker
{
  def use(self: Box<$A>) -> Int {
    match self { Box::Box(value) => Marker::mark::<Int>(value) }
  }
}

value: Box<Int> = Box::Box(1)
result = Use::use(value)"#,
    )
    .expect_err("a bare capability must not prove a missing Marker<Int> body obligation");

    assert!(err.message.contains("Marker<Int>"), "{err:?}");
}

#[test]
fn canonical_builtin_signature_forwards_the_callers_bare_capability() {
    typecheck_std_surface_without_prelude(
        r#"deftrait Equal {
  def equal(self: Self, rhs: Self) -> Int
}

@builtin def group_count(values: List<$A>) -> List<($A, Int)>
where
  $A: Equal"#,
        r#"def count(values: List<$A>) -> List<($A, Int)>
where
  $A: Equal
{
  group_count(values)
}
"#,
    )
    .expect("the builtin's Equal proof forwarding must consume the caller capability");
}

#[test]
fn canonical_builtin_signature_still_rejects_a_missing_capability() {
    let err = typecheck_std_surface_without_prelude(
        r#"deftrait Equal {
  def equal(self: Self, rhs: Self) -> Int
}

@builtin def group_count(values: List<$A>) -> List<($A, Int)>
where
  $A: Equal"#,
        r#"def count(values: List<$A>) -> List<($A, Int)> {
  group_count(values)
}
"#,
    )
    .expect_err("builtin proof forwarding without Equal must be rejected");

    assert!(
        err.message.contains("Builtin group_count requires Equal"),
        "{err:?}"
    );
}

#[test]
fn canonical_builtin_signature_preserves_parameter_names_for_named_calls() {
    typecheck_std_surface_without_prelude(
        r#"@builtin def print(a: String) -> Unit"#,
        r#"def emit() -> Unit {
  print(a: "ok")
}"#,
    )
    .expect("builtin calls must use the same canonical named-argument route as user functions");
}

#[test]
fn canonical_builtin_signature_rejects_parameter_name_drift() {
    let err =
        typecheck_std_surface_without_prelude(r#"@builtin def print(value: String) -> Unit"#, "")
            .expect_err("builtin declaration parameter names are part of the canonical signature");
    assert!(
        err.message
            .contains("does not match its canonical surface signature"),
        "{err:?}"
    );
}

#[test]
fn builtin_declaration_rejects_an_unregistered_owner_alias() {
    let source = r#"@builtin type String
impl String {
  @builtin def print(a: String) -> Unit
}"#;
    let ast = spire::parse_with_context(
        source,
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
    )
    .expect("standard surface should parse");
    let err = sigil::resolve(ast)
        .expect_err("a runtime builtin name must not authorize an arbitrary owner");
    assert!(
        err.message.contains("Unknown builtin declaration"),
        "{err:?}"
    );
}

#[test]
fn checked_generic_constructor_signature_replaces_predeclared_type_variables() {
    let typed = typecheck_without_std_prelude(
        r#"defstruct Box<$A> { value: $A }
impl Box {
  def new(value: $A) -> Box<$A> { Box { value: value } }
}

value = Box(1)"#,
    )
    .expect("generic struct construction should typecheck");
    let value_ty = typed.iter().find_map(|node| match &node.node {
        TypedInner::Bind(pattern, _) => match pattern {
            scar::typed::TypedPattern::Var(ty, id) if id.name == "value" => Some(ty),
            _ => None,
        },
        _ => None,
    });
    assert!(
        matches!(
            value_ty,
            Some(scar::types::Ty::Struct(name, fields))
                if name == "Global::Box"
                    && matches!(fields.as_slice(), [(field, scar::types::Ty::Int)] if field == "value")
        ),
        "generic constructor result must retain its concrete argument, got {value_ty:?}"
    );
}

#[test]
fn receiverless_trait_call_consumes_the_contextual_return_capability() {
    typecheck_without_std_prelude(
        r#"deftrait Default {
  def default::<Self>() -> Self
}

impl Default for Int {
  def default::<Int>() -> Int { 0 }
}

def make(seed: $A) -> $A
where
  $A: Default
{
  Default::default()
}

value: Int = make(1)"#,
    )
    .expect("the contextual Default call must consume $A: Default");
}

#[test]
fn receiverless_value_trait_call_receives_the_declared_tail_result() {
    typecheck_without_std_prelude(
        r#"deftrait Applicative
where
  Self: Type<$A>
{
  def pure::<Self>(value: $A) -> Self<$A>
}

defenum Boxed<$A> { Boxed($A) }

impl Applicative for Boxed<$A> {
  def pure::<Boxed<$A>>(value: $B) -> Boxed<$B> { Boxed::Boxed(value) }
}

def lift(value: $A) -> Boxed<$A> {
  Applicative::pure(value)
}

result: Boxed<Int> = lift(1)"#,
    )
    .expect("the receiverless Applicative tail must receive Boxed<$A> as its expected result");
}

#[test]
fn inherited_rigid_bound_forwards_and_consumes_the_declared_capability() {
    typecheck_without_std_prelude(
        r#"deftrait Marker {
  def mark(self: Self) -> Int
}

deftrait StrongMarker
where
  Self: Marker
{}

deftrait Use {
  def use(self: Self) -> Int
}

impl Use for List<$A>
where
  $A: Marker
{
  def use(self: List<$A>) -> Int {
    match self {
      [] => 0,
      [head, ..tail] => Marker::mark(head),
    }
  }
}

def forward(values: List<$T>) -> Int
where
  $T: StrongMarker
{
  Use::use(values)
}"#,
    )
    .expect("StrongMarker must entail Marker and be consumed by generic proof forwarding");
}

#[test]
fn generic_trait_candidate_does_not_hide_an_unproven_rigid_bound() {
    let err = typecheck_without_std_prelude(
        r#"deftrait Marker {
  def mark(self: Self) -> Int
}

deftrait Use {
  def use(self: Self) -> Int
}

impl Use for List<$A>
where
  $A: Marker
{
  def use(self: List<$A>) -> Int {
    match self {
      [] => 0,
      [head, ..tail] => Marker::mark(head),
    }
  }
}

def hidden(values: List<$A>) -> Int { Use::use(values) }"#,
    )
    .expect_err("an unbounded rigid caller must not select the generic Use implementation");

    assert!(err.message.contains("implementing Use"), "{err:?}");
}

#[test]
fn concrete_trait_candidate_checks_transitive_body_obligations() {
    let err = typecheck_without_std_prelude(
        r#"defenum Box<$A> { Box($A) }

deftrait Equal {
  def equal(self: Self, rhs: Self) -> Int
}

deftrait Marker {
  def mark(self: Self) -> Int
}

deftrait Use {
  def use(self: Self) -> Int
}

impl Marker for List<$A>
where
  $A: Equal
{
  def mark(self: List<$A>) -> Int {
    match self {
      [] => 0,
      [head, ..tail] => Equal::equal(head, head),
    }
  }
}

impl Use for Box<$A>
where
  $A: Marker
{
  def use(self: Box<$A>) -> Int {
    match self { Box::Box(value) => Marker::mark(value) }
  }
}

f = {|n: Int| n}
value = Box::Box([f])
result = Use::use(value)"#,
    )
    .expect_err("the concrete Use candidate must prove the nested Equal obligation");

    assert!(
        err.message
            .contains("Equal::equal could not be specialized to a concrete dispatch target"),
        "{err:?}"
    );
}

#[test]
fn mutually_recursive_concrete_impl_obligations_report_a_cycle() {
    let err = typecheck_without_std_prelude(
        r#"deftrait First {
  def first(self: Self) -> Int
}

deftrait Second {
  def second(self: Self) -> Int
}

impl First for Int
where
  Self: Second
{
  def first(self: Int) -> Int { Second::second(self) }
}

impl Second for Int
where
  Self: First
{
  def second(self: Int) -> Int { First::first(self) }
}

value = First::first(1)"#,
    )
    .expect_err("mutually recursive concrete obligations must not prove one another");

    assert!(err.message.contains("CyclicTraitObligation"), "{err:?}");
}

#[test]
fn receiverless_constructor_dispatch_checks_concrete_impl_cycles() {
    let err = typecheck_without_std_prelude(
        r#"defenum Boxed<$A> { Boxed($A) }

deftrait FirstFactory
where
  Self: Type<$A>
{
  def make::<Self>() -> Self<Int>
}

deftrait SecondFactory
where
  Self: Type<$A>
{
  def make::<Self>() -> Self<Int>
}

impl FirstFactory for Boxed<$A>
where
  Self: SecondFactory
{
  def make::<Boxed<$A>>() -> Boxed<Int> { SecondFactory::make() }
}

impl SecondFactory for Boxed<$A>
where
  Self: FirstFactory
{
  def make::<Boxed<$A>>() -> Boxed<Int> { FirstFactory::make() }
}

value: Boxed<Int> = FirstFactory::make()"#,
    )
    .expect_err("receiverless constructor dispatch must reject mutually recursive impl proofs");

    assert!(err.message.contains("CyclicTraitObligation"), "{err:?}");
}
