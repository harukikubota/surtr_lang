#![deny(dead_code)]

#[path = "support/case_registry.rs"]
mod case_registry;

case_registry::register_case_suite!(
    "default_trait_methods.rs",
    [
        synthesized_trait_default_does_not_require_an_explicit_resolved_impl_method,
        impl_block_capability_consumed_by_one_explicit_method_survives_default_synthesis,
        bare_impl_capability_defers_candidate_proof_to_the_full_body_obligation,
        bare_impl_capability_does_not_replace_the_full_body_obligation,
        canonical_builtin_signature_forwards_the_callers_bare_capability,
        canonical_builtin_signature_still_rejects_a_missing_capability,
        canonical_builtin_signature_preserves_parameter_names_for_named_calls,
        builtin_declaration_rejects_an_unregistered_owner_alias,
        internal_runtime_name_does_not_authorize_an_unknown_owner,
        checked_generic_constructor_signature_replaces_predeclared_type_variables,
        receiverless_trait_call_consumes_the_contextual_return_capability,
        receiverless_value_trait_call_receives_the_declared_tail_result,
        inherited_rigid_bound_forwards_and_consumes_the_declared_capability,
        generic_trait_candidate_does_not_hide_an_unproven_rigid_bound,
        concrete_trait_candidate_checks_transitive_body_obligations,
        mutually_recursive_concrete_impl_obligations_report_a_cycle,
        receiverless_constructor_dispatch_checks_concrete_impl_cycles,
        constructor_projection_cycle_preserves_reason_and_argument_span,
        constructor_projection_keeps_one_way_success_and_plain_missing_bound,
        unrelated_cyclic_constructor_impl_does_not_reject_matching_head,
        unused_cyclic_constructor_capability_does_not_reject_plain_value,
        explicit_constructor_helper_preserves_required_cycle,
    ],
    standalone = [
        canonical_builtin_signature_rejects_a_noncanonical_constraint,
        canonical_builtin_signature_rejects_parameter_name_drift,
        canonical_builtin_signature_rejects_return_type_argument_drift,
        internal_runtime_name_does_not_bypass_builtin_surface_signature_validation,
    ]
);

#[allow(dead_code)]
mod support;

use scar::typed::{TypedInner, TypedNode};
use sigil::resolved::Resolved;

fn resolve_with_standard_environment(source: &str) -> Vec<Resolved> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse with the standard environment");
    support::resolve_ast_with_builtin_prelude(ast)
        .expect("source should resolve with the standard environment")
}

fn typecheck_with_standard_environment(
    source: &str,
) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    support::typecheck(resolve_with_standard_environment(source))
}

fn synthesized_trait_default_does_not_require_an_explicit_resolved_impl_method() {
    let typed = typecheck_with_standard_environment(
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

fn impl_block_capability_consumed_by_one_explicit_method_survives_default_synthesis() {
    typecheck_with_standard_environment(
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

fn bare_impl_capability_defers_candidate_proof_to_the_full_body_obligation() {
    typecheck_with_standard_environment(
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

fn bare_impl_capability_does_not_replace_the_full_body_obligation() {
    let err = typecheck_with_standard_environment(
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

    assert_eq!(
        err.reason(),
        Some(diagnostics::TypeDiagnosticReason::NoApplicableTraitImplementation),
        "{err:?}"
    );
    assert!(err.message.contains("Box<Int>"), "{err:?}");
}

fn canonical_builtin_signature_forwards_the_callers_bare_capability() {
    typecheck_with_standard_environment(
        "def count(values: List<$A>) -> List<($A, Int)> where $A: Eq { List::group_count(values) }",
    )
    .expect("the builtin's Eq proof forwarding must consume the caller capability");
}

#[test]
fn canonical_builtin_signature_rejects_a_noncanonical_constraint() {
    let list_source = include_str!("../../../lib/types/list.srt").replace(
        "@builtin def group_count(values: List<$A>) -> List<($A, Int)>\n  where\n    $A: Eq",
        "@builtin def group_count(values: List<$A>) -> List<($A, Int)>\n  where\n    $A: Show",
    );
    let err = support::typecheck_std_modules_with_overrides(&[("List", &list_source)])
        .expect_err("a different bound must not replace group_count's canonical Eq bound");
    assert!(
        err.message
            .contains("does not match its canonical surface signature"),
        "{err:?}"
    );
}

fn canonical_builtin_signature_still_rejects_a_missing_capability() {
    let err = typecheck_with_standard_environment(
        "def count(values: List<$A>) -> List<($A, Int)> { List::group_count(values) }",
    )
    .expect_err("builtin proof forwarding without Eq must be rejected");
    assert_eq!(
        err.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingGenericBound),
        "{err:?}"
    );
}

fn canonical_builtin_signature_preserves_parameter_names_for_named_calls() {
    typecheck_with_standard_environment(r#"def emit() -> Unit { Kernel::print(a: "ok") }"#)
        .expect("builtin calls use canonical named arguments from standard declarations");
}

#[test]
fn canonical_builtin_signature_rejects_parameter_name_drift() {
    let kernel = include_str!("../../../lib/kernel.srt").replace(
        "@builtin def print(a: String) -> Unit",
        "@builtin def print(value: String) -> Unit",
    );
    let err = support::typecheck_std_modules_with_overrides(&[("Kernel", &kernel)])
        .expect_err("builtin declaration parameter names are part of the canonical signature");
    assert!(
        err.message
            .contains("does not match its canonical surface signature"),
        "{err:?}"
    );
}

#[test]
fn canonical_builtin_signature_rejects_return_type_argument_drift() {
    let function = include_str!("../../../lib/function.srt").replace(
        "@builtin def curry::<$Curried>(fun: $Fun) -> $Curried",
        "@builtin def curry::<$Curried, $Other>(fun: $Fun) -> ($Curried, $Other)",
    );
    let err = support::typecheck_std_modules_with_overrides(&[("Function", &function)])
        .expect_err("builtin return type arguments are part of the canonical surface signature");
    assert!(
        err.message
            .contains("does not match its canonical surface signature"),
        "{err:?}"
    );
}

fn builtin_declaration_rejects_an_unregistered_owner_alias() {
    let source = r#"impl String {
  @builtin def print(a: String) -> Unit
}"#;
    let ast = spire::parse_with_context(
        source,
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
    )
    .expect("standard surface should parse");
    let err = support::resolve_ast_with_builtin_prelude(ast)
        .expect_err("a runtime builtin name must not authorize an arbitrary owner");
    assert!(
        err.message.contains("Unknown builtin declaration"),
        "{err:?}"
    );
}

#[test]
fn internal_runtime_name_does_not_bypass_builtin_surface_signature_validation() {
    let process = include_str!("../../../lib/process.srt").replace(
        "@builtin def call(body: (-> Result<$A>)) -> Result<$A>",
        "@builtin def call(body: (-> Result<$A>)) -> Int",
    );
    let err = support::typecheck_std_modules_with_overrides(&[("Task", &process)])
        .expect_err("a public builtin backed by an internal runtime name must match metadata");
    assert!(
        err.message
            .contains("does not match its canonical surface signature"),
        "{err:?}"
    );
}

fn internal_runtime_name_does_not_authorize_an_unknown_owner() {
    let source = r#"impl String {
  @builtin def __test_pop() -> Unit
}"#;
    let ast = spire::parse_with_context(
        source,
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
    )
    .expect("standard surface should parse");
    let err = support::resolve_ast_with_builtin_prelude(ast)
        .expect_err("an internal runtime name must not authorize an arbitrary owner");
    assert!(
        err.message.contains("Unknown builtin declaration"),
        "{err:?}"
    );
}

fn checked_generic_constructor_signature_replaces_predeclared_type_variables() {
    let typed = typecheck_with_standard_environment(
        r#"defstruct Box<$A> { value: $A }
impl Box {
  def new(value: $A) -> Box<$A> { Box { value: value } }
}

value = Box(1)"#,
    )
    .expect("generic struct construction should typecheck");
    let value_ty = typed.iter().find_map(|node| match &node.node {
        TypedInner::Bind(pattern, _) => match pattern.unlocated() {
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

fn receiverless_trait_call_consumes_the_contextual_return_capability() {
    typecheck_with_standard_environment(
        r#"def make(seed: $A) -> $A
where
  $A: Default
{
  Default::default()
}

value: Int = make(1)"#,
    )
    .expect("the contextual Default call must consume $A: Default");
}

fn receiverless_value_trait_call_receives_the_declared_tail_result() {
    typecheck_with_standard_environment(
        r#"deftrait FixtureApplicative
where
  Self: Type<$A>
{
  def pure::<Self>(value: $A) -> Self<$A>
}

defenum Boxed<$A> { Boxed($A) }

impl FixtureApplicative for Boxed<$A> {
  def pure::<Boxed<$A>>(value: $B) -> Boxed<$B> { Boxed::Boxed(value) }
}

def lift(value: $A) -> Boxed<$A> {
  FixtureApplicative::pure(value)
}

result: Boxed<Int> = lift(1)"#,
    )
    .expect(
        "the receiverless FixtureApplicative tail must receive Boxed<$A> as its expected result",
    );
}

fn inherited_rigid_bound_forwards_and_consumes_the_declared_capability() {
    typecheck_with_standard_environment(
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

fn generic_trait_candidate_does_not_hide_an_unproven_rigid_bound() {
    let err = typecheck_with_standard_environment(
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

    assert_eq!(
        err.reason(),
        Some(diagnostics::TypeDiagnosticReason::NoApplicableTraitImplementation),
        "{err:?}"
    );
}

fn concrete_trait_candidate_checks_transitive_body_obligations() {
    let err = typecheck_with_standard_environment(
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

    assert_eq!(
        err.reason(),
        Some(diagnostics::TypeDiagnosticReason::NoApplicableTraitImplementation),
        "{err:?}"
    );
    assert!(err.message.contains("Box<List<(Int -> Int)>>"), "{err:?}");
}

fn mutually_recursive_concrete_impl_obligations_report_a_cycle() {
    let err = typecheck_with_standard_environment(
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
    assert_eq!(
        err.reason(),
        Some(diagnostics::TypeDiagnosticReason::CyclicTraitObligation)
    );
}

fn receiverless_constructor_dispatch_checks_concrete_impl_cycles() {
    let err = typecheck_with_standard_environment(
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
    assert_eq!(
        err.reason(),
        Some(diagnostics::TypeDiagnosticReason::CyclicTraitObligation)
    );
}

fn projection_cycle_declarations() -> &'static str {
    r#"defenum Boxed<$A> { Boxed($A), }
deftrait FirstFamily where Self: Type<$A> { def use(self: Self<Int>) -> Int }
deftrait SecondFamily where Self: Type<$A> { def use(self: Self<Int>) -> Int }
impl FirstFamily for Boxed<$A> where Self: SecondFamily {
  def use(self: Boxed<Int>) -> Int { SecondFamily::use(self) }
}
impl SecondFamily for Boxed<$A> where Self: FirstFamily {
  def use(self: Boxed<Int>) -> Int { FirstFamily::use(self) }
}
def accept(value: FirstFamily<Int>) -> Unit { () }
"#
}

fn constructor_projection_cycle_preserves_reason_and_argument_span() {
    for use_site in [
        "accept(Boxed::Boxed(1))",
        "def make() -> FirstFamily<Int> { Boxed::Boxed(1) }",
    ] {
        let source = format!("{}{use_site}", projection_cycle_declarations());
        let error =
            typecheck_with_standard_environment(&source).expect_err("cyclic constructor proof");
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::CyclicTraitObligation),
            "{error:?}"
        );
        let start = source.rfind("Boxed::Boxed(1)").unwrap();
        assert_eq!(
            error.span,
            spire::ast::Span {
                start,
                end: start + "Boxed::Boxed(1)".len()
            }
        );
        assert!(error.message.contains("CyclicTraitObligation"), "{error:?}");
        let structured = error.structured.as_ref().expect("structured cycle cause");
        assert_eq!(structured.primary.span, error.span);
        assert!(
            matches!(&structured.data, diagnostics::DiagnosticData::TraitDispatch(data)
    if data.trait_name.ends_with("Family") && data.subject_type.as_deref().is_some_and(|name| name.contains("Boxed")))
        );
    }
}

fn constructor_projection_keeps_one_way_success_and_plain_missing_bound() {
    let declarations = projection_cycle_declarations();
    let success = declarations.replace(
        "impl SecondFamily for Boxed<$A> where Self: FirstFamily {\n  def use(self: Boxed<Int>) -> Int { FirstFamily::use(self) }\n}",
        "impl SecondFamily for Boxed<$A> { def use(self: Boxed<Int>) -> Int { 1 } }",
    );
    typecheck_with_standard_environment(&format!("{success}accept(Boxed::Boxed(1))"))
        .expect("a one-way satisfied bound remains applicable");
    let missing = r#"defenum Boxed<$A> { Boxed($A), }
deftrait FirstFamily where Self: Type<$A> { def use(self: Self<Int>) -> Int }
def accept(value: FirstFamily<Int>) -> Unit { () }
"#;
    let error = typecheck_with_standard_environment(&format!("{missing}accept(Boxed::Boxed(1))"))
        .expect_err("missing FirstFamily implementation is not a cycle");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
}

fn unrelated_cyclic_constructor_impl_does_not_reject_matching_head() {
    let source = format!(
        r#"{}
defenum Other<$A> {{ Other($A), }}
impl FirstFamily for Other<$A> {{ def use(self: Other<Int>) -> Int {{ 1 }} }}
accept(Other::Other(1))"#,
        projection_cycle_declarations()
    );
    typecheck_with_standard_environment(&source)
        .expect("the unrelated Boxed cycle is not requested");
}

fn unused_cyclic_constructor_capability_does_not_reject_plain_value() {
    let source = format!(
        "{}def plain(value: Boxed<Int>) -> Boxed<Int> {{ value }}\nplain(Boxed::Boxed(1))",
        projection_cycle_declarations()
    );
    typecheck_with_standard_environment(&source)
        .expect("optional capability enumeration must not require the cyclic FirstFamily proof");
}

fn explicit_constructor_helper_preserves_required_cycle() {
    let source = format!(
        r#"{}
deftrait Wrap where Self: Type<$A> {{ def wrap::<Self>(value: $A) -> Self<$A> }}
impl Wrap for Boxed<$T> {{
  def wrap::<Boxed<$T>>(value: $A) -> Boxed<$A> {{ Boxed::Boxed(value) }}
}}
FirstFamily::use(Wrap::wrap::<Boxed<Int>>(1))"#,
        projection_cycle_declarations()
    );
    let error = typecheck_with_standard_environment(&source)
        .expect_err("known carrier requires FirstFamily");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::CyclicTraitObligation),
        "{error:?}"
    );
    assert!(matches!(
        &error.structured.as_ref().unwrap().data,
        diagnostics::DiagnosticData::TraitDispatch(_)
    ));
}
