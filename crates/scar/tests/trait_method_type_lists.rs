use diagnostics::TypeDiagnosticReason;

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).expect("parse");
    scar::typecheck(sigil::resolve(ast).expect("resolve"))
}

#[test]
fn return_only_nested_mismatch_is_structured() {
    let error = check(
        r#"
defstruct Box<$T> { val: $T }
impl Box { def new(val: $T) -> Box<$T> { Box { val: val } } }
deftrait Make { def make(self: Self) -> Box<Int> }
impl Make for Int { def make(self: Self) -> Box<String> { Box::new("x") } }
"#,
    )
    .expect_err("return mismatch");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::TraitMethodTypeListMismatch),
        "{error}"
    );
    let structured = error.structured.expect("structured facts");
    let diagnostics::DiagnosticData::TraitMethodTypeList(data) = structured.data else {
        panic!("type-list payload")
    };
    assert_eq!(data.role, diagnostics::TypeListRole::ReturnType);
    assert_eq!(data.ordinal, 0);
    assert_eq!(data.nested_path, vec![0]);
    assert_eq!(data.expected_type, "Box<Int>");
    assert_eq!(data.actual_type, "Box<String>");
    assert_eq!(structured.primary.role, diagnostics::SourceRole::Impl);
    assert_eq!(
        structured.related[0].role,
        diagnostics::SourceRole::Contract
    );
    assert_ne!(structured.primary.span, structured.related[0].span);
}

#[test]
fn value_parameter_arity_is_structured() {
    let error = check(
        r#"
deftrait Copy { def copy(self: Self, value: Int) -> Int }
impl Copy for Int { def copy(self: Self) -> Int { 0 } }
"#,
    )
    .expect_err("arity mismatch");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::TraitMethodTypeListArityMismatch),
        "{error}"
    );
    let structured = error.structured.expect("arity diagnostic");
    assert!(structured.primary.ty.is_none());
    assert!(structured.related[0].ty.is_none());
}

#[test]
fn preserves_repeated_variables_across_entries() {
    let error = check(
        r#"
deftrait Same { def same(self: Self, left: $A, right: $A) -> $A }
impl Same for Int { def same(self: Self, left: $X, right: $Y) -> $X { left } }
"#,
    )
    .expect_err("relationships must match");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::TraitMethodTypeListMismatch),
        "{error}"
    );
}

#[test]
fn alpha_renamed_nested_signature_is_accepted() {
    check(r#"
defstruct Box<$T> { val: $T }
impl Box { def new(val: $T) -> Box<$T> { Box { val: val } } }
deftrait Identity { def identity(self: Self, value: (Box<$A>, ($A -> $A))) -> (Box<$A>, ($A -> $A)) }
impl Identity for Int { def identity(self: Self, value: (Box<$Z>, ($Z -> $Z))) -> (Box<$Z>, ($Z -> $Z)) { value } }
"#).expect("alpha equivalent recursive signature");
}

#[test]
fn method_constraint_mismatch_is_structured() {
    let error = check(
        r#"
deftrait Marker {}
deftrait Identity { def identity(self: Self, value: $A) -> $A where $A: Marker }
impl Identity for Int { def identity(self: Self, value: $Z) -> $Z { value } }
"#,
    )
    .expect_err("constraints must match");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::TraitMethodConstraintMismatch),
        "{error}"
    );
}

#[test]
fn return_type_argument_arity_is_structured() {
    let error = check(
        r#"
deftrait Factory { def make::<Self>() -> Self }
impl Factory for Int { def make() -> Int { 0 } }
"#,
    )
    .expect_err("return input must not disappear");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::TraitMethodTypeListArityMismatch),
        "{error}"
    );
}

#[test]
fn constraints_are_an_alpha_renamed_order_independent_set() {
    check(r#"
deftrait First { def first(self: Self) -> Self }
deftrait Second { def second(self: Self) -> Self }
deftrait Identity { def identity(self: Self, value: $A) -> $A where $A: First + Second }
impl Identity for Int { def identity(self: Self, value: $Z) -> $Z where $Z: Second + First { First::first(Second::second(value)) } }
"#).expect("constraint order and generic spelling are irrelevant");
}

#[test]
fn constraints_are_substituted_through_trait_arguments() {
    check(r#"
deftrait Marker { def mark(self: Self) -> Self }
impl Marker for Int { def mark(self: Self) -> Self { self } }
deftrait Identity<$A> { def identity(self: Self, value: $A) -> $A where $A: Marker }
impl Identity<Int> for Int { def identity(self: Self, value: Int) -> Int where Self: Marker { Marker::mark(value) } }
"#).expect("contract head arguments also substitute constraint subjects");
}

#[test]
fn constructor_self_application_expands_with_slot_mapping() {
    check(
        r#"
defstruct Box<$T> { val: $T }
impl Box { def new(val: $T) -> Box<$T> { Box { val: val } } }
deftrait Keep where Self: Type<$A> {
  def keep(self: Self<$A>) -> Self<$A>
}
impl Keep for Box<$T> where $T: Keep.$A {
  def keep(self: Box<$Z>) -> Box<$Z> { self }
}
"#,
    )
    .expect("Self application is expanded before structural matching");
}

#[test]
fn impl_head_variable_remains_anchored_even_without_receiver() {
    let error = check(
        r#"
defstruct Box<$T> { val: $T }
impl Box { def new(val: $T) -> Box<$T> { Box { val: val } } }
deftrait Identity<$A> { def identity(value: $A) -> $A }
impl Identity<$T> for Box<$T> { def identity(value: $Z) -> $Z { value } }
"#,
    )
    .expect_err("impl head relationships cannot be freshened away");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::TraitMethodTypeListMismatch),
        "{error}"
    );
}

#[test]
fn nominal_path_uses_declared_arguments_instead_of_field_order() {
    let error = check(r#"
defstruct Pair<$A, $B> { second: $B, first: $A }
impl Pair { def new(first: $A, second: $B) -> Pair<$A, $B> { Pair { first: first, second: second } } }
deftrait Make { def make(self: Self) -> Pair<Int, String> }
impl Make for Int { def make(self: Self) -> Pair<Int, Int> { Pair::new(1, 2) } }
"#).expect_err("nested argument mismatch");
    let diagnostics::DiagnosticData::TraitMethodTypeList(data) =
        error.structured.expect("structured").data
    else {
        panic!("type list")
    };
    assert_eq!(data.role, diagnostics::TypeListRole::ReturnType);
    assert_eq!(data.nested_path, vec![1]);
    assert_eq!(data.expected_type, "Pair<Int, String>");
    assert_eq!(data.actual_type, "Pair<Int, Int>");
}

#[test]
fn concrete_impl_return_type_arguments_keep_nested_structure() {
    let error = check(
        r#"
defstruct Box<$T> { val: $T }
impl Box { def new(val: $T) -> Box<$T> { Box { val: val } } }
deftrait Factory { def make::<Self>() -> Self }
impl Factory for Box<Int> { def make::<Box<String>>() -> Box<String> { Box::new("x") } }
"#,
    )
    .expect_err("nested return input mismatch");
    let diagnostics::DiagnosticData::TraitMethodTypeList(data) =
        error.structured.expect("structured").data
    else {
        panic!("type list")
    };
    assert_eq!(data.role, diagnostics::TypeListRole::ReturnTypeArgument);
    assert_eq!(data.ordinal, 0);
    assert_eq!(data.nested_path, vec![0]);
}

#[test]
fn phantom_nominal_arguments_are_part_of_contract() {
    let error = check(
        r#"
defstruct Phantom<$T> {}
impl Phantom { def new::<$T>() -> Phantom<$T> { Phantom {} } }
deftrait Make { def make(self: Self) -> Phantom<Int> }
impl Make for Int { def make(self: Self) -> Phantom<String> { Phantom::new::<String>() } }
"#,
    )
    .expect_err("phantom type argument mismatch");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::TraitMethodTypeListMismatch)
    );
}

#[test]
fn trait_parameter_names_do_not_capture_impl_variables() {
    check(
        r#"
deftrait Pair<$A, $B> { def keep(value: $A) -> $A }
impl Pair<$B, $A> for Int { def keep(value: $B) -> $B { value } }
"#,
    )
    .expect("impl names are in a separate namespace");
}

#[test]
fn colliding_trait_parameter_names_do_not_accept_wrong_impl_variable() {
    let error = check(
        r#"
deftrait Pair<$A, $B> { def keep(value: $A) -> $A }
impl Pair<$B, $A> for Int { def keep(value: $A) -> $A { value } }
"#,
    )
    .expect_err("trait substitutions cannot overwrite implementation names");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::TraitMethodTypeListMismatch)
    );
}

#[test]
fn matching_phantom_nominal_arguments_are_accepted() {
    check(
        r#"
defstruct Phantom<$T> {}
impl Phantom { def new::<$T>() -> Phantom<$T> { Phantom {} } }
deftrait Make { def make(self: Self) -> Phantom<Int> }
impl Make for Int { def make(self: Self) -> Phantom<Int> { Phantom::new::<Int>() } }
"#,
    )
    .expect("phantom applications remain legal");
}

#[test]
fn variable_cannot_be_equated_with_a_recursive_type_tree() {
    let error = check(
        r#"
deftrait Same { def same(self: Self, value: $A) -> $A }
impl Same for Int { def same(self: Self, value: $X) -> List<$X> { [value] } }
"#,
    )
    .expect_err("alpha equivalence never binds a variable to a recursive tree");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::TraitMethodTypeListMismatch)
    );
}

#[test]
fn duplicate_method_constraints_do_not_change_the_canonical_set() {
    check(r#"
deftrait Marker { def mark(self: Self) -> Self }
deftrait Identity { def identity(self: Self, value: $A) -> $A where $A: Marker }
impl Identity for Int { def identity(self: Self, value: $Z) -> $Z where $Z: Marker + Marker { Marker::mark(value) } }
"#).expect("where constraints are a set");
}

#[test]
fn default_method_keeps_impl_head_namespace_separate() {
    check(
        r#"
deftrait Pair<$A, $B> { def keep(value: $A) -> $A { value } }
impl Pair<$B, $A> for Int {}
"#,
    )
    .expect("synthesized defaults substitute contract names without changing impl head names");
}

#[test]
fn aliases_preserve_nested_phantom_arguments() {
    let error = check(
        r#"
defstruct Phantom<$T> {}
impl Phantom { def new::<$T>() -> Phantom<$T> { Phantom {} } }
type Alias<$A> = (Int -> List<Phantom<$A>>)
deftrait Make { def make(self: Self) -> Alias<Int> }
impl Make for Int { def make(self: Self) -> Alias<String> { {|value| []} } }
"#,
    )
    .expect_err("alias expansion keeps nested phantom arguments");
    let diagnostics::DiagnosticData::TraitMethodTypeList(data) =
        error.structured.expect("structured").data
    else {
        panic!("type list")
    };
    assert_eq!(data.nested_path, vec![1, 0, 0]);
    assert_eq!(data.expected_type, "(Int -> List<Phantom<Int>>)");
    assert_eq!(data.actual_type, "(Int -> List<Phantom<String>>)");
}
