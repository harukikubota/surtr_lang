use diagnostics::TypeDiagnosticReason;
use scar::typed::TypedInner;
use sigil::resolved::Resolved;

fn resolve_without_std_prelude(source: &str) -> Vec<Resolved> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse without the standard prelude");
    sigil::resolve(ast).expect("source should resolve without the standard prelude")
}

fn typecheck_without_std_prelude(
    source: &str,
) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    scar::typecheck(resolve_without_std_prelude(source))
}

fn assert_reason(source: &str, expected: TypeDiagnosticReason) -> scar::error::TypeError {
    let error = typecheck_without_std_prelude(source).expect_err("source must be rejected");
    assert_eq!(error.reason(), Some(expected), "unexpected error: {error}");
    error
}

#[test]
fn accepts_declared_return_only_input() {
    typecheck_without_std_prelude(
        r#"deftrait Factory {
  def make::<$A>() -> $A
}"#,
    )
    .expect("a declared return-only input should be accepted");
}

#[test]
fn recursively_finds_missing_return_only_input() {
    let error = assert_reason(
        r#"def missing(mapper: ($A -> Int)) -> Option<$B> { 0 }"#,
        TypeDiagnosticReason::MissingReturnTypeArgument,
    );
    assert_eq!(error.message, "return-only type input `$B` is not declared");
}

#[test]
fn rejects_input_introduced_by_value_and_return_type_argument() {
    let error = assert_reason(
        r#"deftrait Functor
where
  Self: Type<$A>
{}

def duplicate::<$F>(value: $F<$A>) -> $F<$A>
where
  $F: Functor
{ value }"#,
        TypeDiagnosticReason::DuplicateReturnTypeArgumentInput,
    );
    assert_eq!(
        error.message,
        "type input `$F` is introduced more than once"
    );
    let structured = error
        .structured
        .expect("diagnostic should retain both origins");
    assert_eq!(structured.related.len(), 1);
}

#[test]
fn rejects_unused_return_type_argument() {
    let error = assert_reason(
        r#"def unused::<$A>() -> Int { 0 }"#,
        TypeDiagnosticReason::UnusedReturnTypeArgument,
    );
    assert_eq!(
        error.message,
        "return type argument `$A` does not appear in the return type"
    );
}

#[test]
fn rejects_constructor_variable_without_constructor_trait_constraint() {
    let error = assert_reason(
        r#"def invalid(value: $F<$A>) -> $F<$A> { value }"#,
        TypeDiagnosticReason::MissingTypeConstructorConstraint,
    );
    assert_eq!(
        error.message,
        "type constructor variable `$F` requires a TypeCtorTrait constraint"
    );
}

#[test]
fn accepts_constructor_variable_with_constructor_trait_constraint() {
    typecheck_without_std_prelude(
        r#"deftrait Functor
where
  Self: Type<$A>
{}

deftrait Keeper {
  def keep(value: $F<$A>) -> $F<$A>
  where
    $F: Functor
}"#,
    )
    .expect("a constrained constructor variable application should be accepted");
}

#[test]
fn rejects_trait_name_as_where_constraint_subject() {
    let error = assert_reason(
        r#"deftrait Add {}

deftrait Applicative
where
  Self: Type<$A>
{}

def invalid_subject::<$F>() -> $F<Unit>
where
  Applicative: Add
{ 0 }"#,
        TypeDiagnosticReason::InvalidTraitConstraintSubject,
    );
    assert_eq!(
        error.message,
        "trait `Applicative` cannot be used as a constraint subject"
    );
}

#[test]
fn recursive_value_occurrences_do_not_require_return_type_arguments() {
    typecheck_without_std_prelude(
        r#"deftrait Mapper {
  def map(mapper: ($A -> $B)) -> ($A -> $B)
}"#,
    )
    .expect("nested function-type inputs should be classified as value inputs");
}

#[test]
fn direct_type_constructor_trait_return_type_argument_is_accepted() {
    typecheck_without_std_prelude(
        r#"deftrait Alternative
where
  Self: Type<$A>
{}

deftrait GuardFactory {
  def guard::<Alternative>(condition: Boolean) -> Alternative<Unit>
}"#,
    )
    .expect("direct TypeCtorTrait syntax should normalize as one constructor input");
}

#[test]
fn direct_type_constructor_trait_uses_one_typed_witness() {
    let typed = typecheck_without_std_prelude(
        r#"deftrait Alternative
where
  Self: Type<$A>
{}

impl Alternative for List<$T>
where
  $T: Alternative.$A
{}

def guard::<Alternative>(condition: Boolean) -> Alternative<Unit> {
  []
}"#,
    )
    .expect("a concrete constructor body should satisfy the direct constructor signature");
    let (_, return_type_arguments, return_type) = typed
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Def(_, id, arguments, _, return_type, _, _, _) if id.name == "guard" => {
                Some((id, arguments, return_type))
            }
            _ => None,
        })
        .expect("guard should be present in the typed program");
    let rta_witness = match &return_type_arguments[0].ty {
        scar::types::Ty::SelfApp(items) => &items[1],
        other => panic!("expected constructor RTA, got {other:?}"),
    };
    assert_eq!(rta_witness, return_type);
}

#[test]
fn rejects_omitted_return_only_input_without_a_witness() {
    let error = assert_reason(
        r#"def make::<$A>() -> List<$A> { [] }
make()"#,
        TypeDiagnosticReason::AmbiguousReturnTypeArgument,
    );
    assert_eq!(
        error.message,
        "return type arguments for `make` cannot be inferred"
    );
}

#[test]
fn accepts_return_only_input_inferred_from_expected_result() {
    typecheck_without_std_prelude(
        r#"def make::<$A>() -> List<$A> { [] }
value: List<Int> = make()"#,
    )
    .expect("the expected result type should determine the return-only input");
}

#[test]
fn rejects_ambiguous_return_only_input_inside_an_unannotated_binding() {
    assert_reason(
        r#"def make::<$A>() -> List<$A> { [] }
value = make()"#,
        TypeDiagnosticReason::AmbiguousReturnTypeArgument,
    );
}

#[test]
fn accepts_return_only_input_forwarded_by_an_outer_generic_result() {
    typecheck_without_std_prelude(
        r#"def make::<$A>() -> List<$A> { [] }
def forward::<$A>() -> List<$A> { make() }"#,
    )
    .expect("an outer declared input should witness the nested return-only input");
}

#[test]
fn forwards_return_only_where_obligation_through_outer_generic_bound() {
    for tail in [
        "inner()",
        "if(True, inner(), inner())",
        "match 0 { 0 => inner(), _ => inner() }",
    ] {
        let source = r#"deftrait Default {
  def default::<Self>() -> Self
}

impl Default for Int {
  def default::<Int>() -> Int { 0 }
}

def inner::<$A>() -> $A
where
  $A: Default
{
  Default::default()
}

def outer::<$A>() -> $A
where
  $A: Default
{
  BODY
}

value: Int = outer()"#
            .replace("BODY", tail);
        typecheck_without_std_prelude(&source)
            .unwrap_or_else(|error| panic!("generic proof forwarding failed for {tail}: {error}"));
    }
}

#[test]
fn accepts_return_only_input_deferred_into_a_callable_result() {
    typecheck_without_std_prelude(
        r#"def identity::<$A>() -> ($A -> $A) { {|value| value} }
callable = identity()
result: Int = callable(42)"#,
    )
    .expect("a returned callable should receive its witness from a later application");
}

#[test]
fn accepts_explicit_return_type_argument_on_ordinary_callable() {
    typecheck_without_std_prelude(
        r#"def make::<$A>() -> List<$A> { [] }
value: List<Int> = make::<Int>()"#,
    )
    .expect("an ordinary callable should accept its declared ReturnTypeArgument");
}

#[test]
fn ordinary_return_type_argument_requires_a_complete_type() {
    let error = typecheck_without_std_prelude(
        r#"def make::<$A>() -> List<$A> { [] }
value = make::<List>()"#,
    )
    .expect_err("a bare constructor head is not a complete ordinary type argument");
    assert!(
        error.message.contains("Unknown type: List"),
        "unexpected error: {error}"
    );
}

#[test]
fn constructor_return_type_argument_accepts_only_a_bare_head() {
    typecheck_without_std_prelude(
        r#"deftrait Alternative
where
  Self: Type<$A>
{}

impl Alternative for List<$T>
where
  $T: Alternative.$A
{}

def guard::<Alternative>(condition: Boolean) -> Alternative<Unit> { [] }
value: List<Unit> = guard::<List>(True)"#,
    )
    .expect("a direct constructor input should accept its bare constructor head");

    let error = typecheck_without_std_prelude(
        r#"deftrait Alternative
where
  Self: Type<$A>
{}

impl Alternative for List<$T>
where
  $T: Alternative.$A
{}

def guard::<Alternative>(condition: Boolean) -> Alternative<Unit> { [] }
value: List<Unit> = guard::<List<Unit>>(True)"#,
    )
    .expect_err("a constructor input must not accept a fully applied type");
    assert!(
        error.message.contains("bare type constructor head"),
        "unexpected error: {error}"
    );
}

#[test]
fn rejects_constructor_head_with_an_unresolved_fixed_argument() {
    assert_reason(
        r#"deftrait Alternative
where
  Self: Type<$A>
{}

defenum Either<$L, $R> {
  Left($L),
  Right($R),
}

impl Alternative for Either<$L, $R>
where
  $R: Alternative.$A
{}

def choose::<Alternative>() -> Alternative<Unit> { Either::Right(()) }
value = choose::<Either>()"#,
        TypeDiagnosticReason::AmbiguousReturnTypeArgument,
    );
}

#[test]
fn rejects_captured_constructor_head_with_an_unresolved_fixed_argument() {
    assert_reason(
        r#"deftrait Alternative
where
  Self: Type<$A>
{}

defenum Either<$L, $R> {
  Left($L),
  Right($R),
}

impl Alternative for Either<$L, $R>
where
  $R: Alternative.$A
{}

def choose::<Alternative>() -> Alternative<Unit> { Either::Right(()) }
factory = &choose::<Either>"#,
        TypeDiagnosticReason::AmbiguousReturnTypeArgument,
    );
}

#[test]
fn expected_constructor_selection_is_independent_of_impl_order() {
    for impls in [
        r#"impl Factory for First<$T> {
  def make::<First<$T>>() -> First<Unit> { First::First(()) }
}

impl Factory for Second<$T> {
  def make::<Second<$T>>() -> Second<Unit> { Second::Second(()) }
}"#,
        r#"impl Factory for Second<$T> {
  def make::<Second<$T>>() -> Second<Unit> { Second::Second(()) }
}

impl Factory for First<$T> {
  def make::<First<$T>>() -> First<Unit> { First::First(()) }
}"#,
    ] {
        let source = format!(
            r#"defenum First<$A> {{ First($A) }}
defenum Second<$A> {{ Second($A) }}

deftrait Factory
where
  Self: Type<$A>
{{
  def make::<Self>() -> Self<Unit>
}}

{impls}

def choose::<Factory>() -> Factory<Unit> {{ Factory::make() }}
value: Second<Unit> = choose()"#
        );
        typecheck_without_std_prelude(&source)
            .expect("expected return type must select Second regardless of impl order");
    }
}

#[test]
fn omitted_and_underscore_return_type_arguments_share_inference() {
    typecheck_without_std_prelude(
        r#"def make::<$A>() -> List<$A> { [] }
omitted: List<Int> = make()
underscore: List<Int> = make::<_>()"#,
    )
    .expect("omitted and underscore ReturnTypeArguments should use the same inference route");
}

#[test]
fn rejects_return_type_argument_arity_underflow_without_partial_zip() {
    assert_reason(
        r#"def choose::<$A, $B>() -> ($A, $B) { choose() }
choose::<Int>()"#,
        TypeDiagnosticReason::ReturnTypeArgumentArityMismatch,
    );
}

#[test]
fn rejects_return_type_argument_arity_overflow_without_partial_zip() {
    assert_reason(
        r#"def make::<$A>() -> List<$A> { [] }
make::<Int, String>()"#,
        TypeDiagnosticReason::ReturnTypeArgumentArityMismatch,
    );
}

#[test]
fn rejects_explicit_return_type_argument_conflicting_with_expected_return() {
    let error = assert_reason(
        r#"def make::<$A>() -> List<$A> { [] }
value: List<String> = make::<Int>()"#,
        TypeDiagnosticReason::ReturnTypeArgumentMismatch,
    );
    let structured = error
        .structured
        .expect("mismatch should retain both origins");
    assert_eq!(
        structured.primary.role,
        diagnostics::SourceRole::ReturnTypeArgument
    );
    assert_eq!(
        structured.related[0].role,
        diagnostics::SourceRole::Expected
    );
}

#[test]
fn mismatch_reports_the_conflicting_return_type_argument_ordinal() {
    let error = assert_reason(
        r#"def choose::<$A, $B>() -> ($A, $B) { choose() }
value: (Int, String) = choose::<Int, Boolean>()"#,
        TypeDiagnosticReason::ReturnTypeArgumentMismatch,
    );
    let structured = error.structured.expect("mismatch should be structured");
    assert_eq!(
        structured.origin,
        diagnostics::DiagnosticOrigin::ReturnTypeArgument { ordinal: 1 }
    );
}

#[test]
fn ambiguity_reports_the_unresolved_return_type_argument_ordinal() {
    let error = assert_reason(
        r#"def choose::<$A, $B>() -> ($A, $B) { choose() }
value = choose::<Int, _>()"#,
        TypeDiagnosticReason::AmbiguousReturnTypeArgument,
    );
    let structured = error.structured.expect("ambiguity should be structured");
    assert_eq!(
        structured.origin,
        diagnostics::DiagnosticOrigin::ReturnTypeArgument { ordinal: 1 }
    );
}

#[test]
fn accepts_explicit_return_type_argument_capture_with_expected_shape() {
    typecheck_without_std_prelude(
        r#"def make::<$A>() -> List<$A> { [] }
factory: (-> List<Int>) = &make::<Int>"#,
    )
    .expect("a capture should preserve an explicitly solved ReturnTypeArgument");
}

#[test]
fn rejects_explicit_capture_conflicting_with_expected_return() {
    let error = assert_reason(
        r#"def make::<$A>() -> List<$A> { [] }
factory: (-> List<String>) = &make::<Int>"#,
        TypeDiagnosticReason::ReturnTypeArgumentMismatch,
    );
    let structured = error.structured.expect("mismatch should be structured");
    assert_eq!(
        structured.origin,
        diagnostics::DiagnosticOrigin::ReturnTypeArgument { ordinal: 0 }
    );
    assert_eq!(
        structured.related[0].role,
        diagnostics::SourceRole::Expected
    );
}

#[test]
fn rejects_ambiguous_return_type_argument_capture() {
    assert_reason(
        r#"def make::<$A>() -> List<$A> { [] }
factory = &make::<_>"#,
        TypeDiagnosticReason::AmbiguousReturnTypeArgument,
    );
}

#[test]
fn omitted_and_underscore_captures_share_ambiguity_check() {
    assert_reason(
        r#"def make::<$A>() -> List<$A> { [] }
factory = &make"#,
        TypeDiagnosticReason::AmbiguousReturnTypeArgument,
    );
}

#[test]
fn expected_generic_result_allows_err_only_self_match_arm() {
    typecheck_without_std_prelude(
        r#"def map_result(value: Result<$A>, mapper: ($A -> $B)) -> Result<$B> {
  match value {
    Ok(inner) => Ok(mapper(inner)),
    _ => value,
  }
}"#,
    )
    .expect("an error-only fallback preserves Result's error channel under an expected result");
}
