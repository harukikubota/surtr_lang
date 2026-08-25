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
fn builtin_contract_forwarding_consumes_the_callers_bare_capability() {
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
fn builtin_contract_forwarding_still_rejects_a_missing_capability() {
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
        err.message.contains("Equal") || err.message.contains("Use"),
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
