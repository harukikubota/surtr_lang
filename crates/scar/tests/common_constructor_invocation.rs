#[allow(dead_code)]
mod support;

#[test]
fn constructor_invocations_propagate_expected_return() {
    for expression in [
        "Functor::fmap([1], {|x: Int| []})",
        "[1] |*> {|x: Int| []}",
        "Monad::bind([1], {|x: Int| []})",
        "[1] |>= {|x: Int| []}",
    ] {
        let source = format!("value: List<List<String>> = {expression}");
        support::typecheck(support::resolve_with_builtin_prelude(&source))
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
    }
}

#[test]
fn callable_result_context_is_shared_by_helpers_and_operators() {
    for (expected, expression) in [
        (
            "List<List<String>>",
            "Applicative::ap([{|x: Int| []}], [1])",
        ),
        ("List<List<String>>", "[{|x: Int| []}] |*| [1]"),
        (
            "(Int -> List<String>)",
            "Composable::compose({|x: Int| x}, {|x: Int| []})",
        ),
        ("(Int -> List<String>)", "{|x: Int| x} >> {|x: Int| []}"),
        (
            "(Int -> List<List<String>>)",
            "LiftComposable::lift_compose({|x: Int| [x]}, {|x: Int| []})",
        ),
        (
            "(Int -> List<List<String>>)",
            "{|x: Int| [x]} >* {|x: Int| []}",
        ),
        (
            "(Int -> List<String>)",
            "KleisliComposable::kleisli_compose({|x: Int| [x]}, {|x: Int| []})",
        ),
        ("(Int -> List<String>)", "{|x: Int| [x]} >=> {|x: Int| []}"),
    ] {
        let source = format!("value: {expected} = {expression}");
        support::typecheck(support::resolve_with_builtin_prelude(&source))
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
    }
}

#[test]
fn slash_and_helper_use_the_expected_result_to_solve_trait_arguments() {
    for expression in ["Segment(1) / 2", "Compose::compose(Segment(1), 2)"] {
        let source = format!(
            r#"
defrecord Segment(value: Int)
impl Compose<Int, String> for Segment {{
    def compose::<String>(self: Segment, rhs: Int) -> String {{ "joined" }}
}}
value: String = {expression}
"#
        );
        support::typecheck(support::resolve_with_builtin_prelude(&source))
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
    }
}

#[test]
fn trait_result_conflict_preserves_expected_and_actual_direction() {
    let error = support::typecheck(support::resolve_with_builtin_prelude(
        "value: Int = Show::to_string(1)",
    ))
    .expect_err("the declared result is String, while the context requires Int");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::ReturnTypeMismatch)
    );
    let data = error.structured.unwrap().data.to_json_value();
    assert_eq!(data["expected_type"], "Int");
    assert_eq!(data["actual_type"], "String");
}

#[test]
fn custom_functor_returns_follow_the_shared_plain_inference_policy() {
    let definitions = r#"
defenum Boxed<$T> { Box($T) }
impl Functor for Boxed<$T> {
    def fmap(self: Boxed<$A>, mapper: ($A -> $B)) -> Boxed<$B> {
        match self { Boxed::Box(value) => Boxed::Box(mapper(value)) }
    }
}
"#;
    for (expression, expected_origin) in [
        (
            "Functor::fmap(Boxed::Box(1), {|x: Int| Boxed::Box(x)})",
            diagnostics::DiagnosticOrigin::TraitCall,
        ),
        (
            "Boxed::Box(1) |*> {|x: Int| Boxed::Box(x)}",
            diagnostics::DiagnosticOrigin::Operator {
                operator: "|*>".into(),
            },
        ),
    ] {
        let annotated = format!("{definitions}\nvalue: Boxed<Boxed<Int>> = {expression}");
        support::typecheck(support::resolve_with_builtin_prelude(&annotated))
            .expect("an explicit nested carrier context permits a contextual mapper result");
        let inferred = format!("{definitions}\nvalue = {expression}");
        let error = support::typecheck(support::resolve_with_builtin_prelude(&inferred))
            .expect_err("plain mapper policy is independent of the constructor's name");
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::CallableShapeMismatch)
        );
        assert_eq!(error.structured.unwrap().origin, expected_origin);
    }
}

#[test]
fn one_registered_carrier_is_not_constructor_inference_evidence() {
    let definitions = r#"
deftrait Functor where Self: Type<$A> {
    def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}
deftrait Monad where Self: Functor {
    def return::<Self>(value: $A) -> Self<$A>
    def bind(self: Self<$A>, mapper: ($A -> Self<$B>)) -> Self<$B>
}
defenum Boxed<$T> { Box($T) }
impl Functor for Boxed<$T> {
    def fmap(self: Boxed<$A>, mapper: ($A -> $B)) -> Boxed<$B> {
        match self { Boxed::Box(value) => Boxed::Box(mapper(value)) }
    }
}
impl Monad for Boxed<$T> {
    def return::<Boxed<$T>>(value: $A) -> Boxed<$A> { Boxed::Box(value) }
    def bind(self: Boxed<$A>, mapper: ($A -> Boxed<$B>)) -> Boxed<$B> {
        match self { Boxed::Box(value) => mapper(value) }
    }
}
"#;
    let check = |tail| {
        let source = format!("{definitions}\n{tail}");
        let ast = spire::parse_with_context(&source, spire::ParserContext::project(0)).unwrap();
        scar::typecheck(sigil::resolve(ast).unwrap())
    };
    let error = check("value = Monad::return(1) |>= {|x| Monad::return(x)}")
        .expect_err("registration count supplies no source-level carrier evidence");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::AmbiguousReturnTypeArgument),
        "{error:?}"
    );
    check("value = Monad::return(1) |>= {|x| Boxed::Box(x)}")
        .expect("a concrete callback result supplies the carrier");
    check("value: Boxed<Int> = Monad::return(1) |>= {|x| Monad::return(x)}")
        .expect("an expected concrete carrier supplies the missing evidence");
    let error = check("value = Functor::fmap(Monad::return(1), {|x: Int| x})")
        .expect_err("a plain mapped payload never provides the outer carrier");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::AmbiguousReturnTypeArgument)
    );
    check("value: Boxed<Int> = Functor::fmap(Monad::return(1), {|x: Int| x})")
        .expect("the expected carrier determines both helper calls");
    check("value = Functor::fmap(Monad::return::<Boxed>(1), {|x: Int| x})")
        .expect("an explicit inner constructor head supplies source evidence");
}

#[test]
fn generic_receiverless_family_helpers_use_expected_return() {
    let definitions = r#"
deftrait Family where Self: Type<$A> {
    def make::<Self>(value: $A) -> Self<$A>
    def map(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}
defenum Boxed<$T> { Box($T) }
impl Family for Boxed<$T> {
    def make::<Boxed<$T>>(value: $A) -> Boxed<$A> { Boxed::Box(value) }
    def map(self: Boxed<$A>, mapper: ($A -> $B)) -> Boxed<$B> {
        match self { Boxed::Box(value) => Boxed::Box(mapper(value)) }
    }
}
"#;
    for expression in [
        "Family::make(1)",
        "Family::map(Family::make(1), {|x: Int| x})",
    ] {
        let source = format!("{definitions}\nvalue: Boxed<Int> = {expression}");
        let ast = spire::parse_with_context(&source, spire::ParserContext::project(0)).unwrap();
        scar::typecheck(sigil::resolve(ast).unwrap())
            .expect("receiverless helpers are signature-driven");
    }
}
