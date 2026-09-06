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
fn accepts_return_only_input_deferred_into_a_callable_result() {
    typecheck_without_std_prelude(
        r#"def identity::<$A>() -> ($A -> $A) { {|value| value} }
callable = identity()
result: Int = callable(42)"#,
    )
    .expect("a returned callable should receive its witness from a later application");
}
