#[allow(dead_code)]
mod support;

use scar::{
    typed::{TypedInner, TypedPattern},
    types::Ty,
};
use sindr::policy::RuntimeSourcePolicy;

fn ty(source: &str, name: &str) -> Ty {
    let nodes = support::typecheck_with_rules(source, RuntimeSourcePolicy::script())
        .unwrap_or_else(|error| panic!("{source}\n{error:?}"));
    nodes
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(pattern, value) if matches!(pattern.unlocated(), TypedPattern::Var(_, id) if id.name == name) => {
                Some(value.ty.clone())
            }
            _ => None,
        })
        .expect("binding")
}
fn thunk(ty: Ty) -> Ty {
    Ty::Func(vec![], Box::new(ty))
}

#[test]
fn known_lazy_branch_determines_capture_signature() {
    for branch in ["1", "{|| 1}"] {
        assert_eq!(
            ty(&format!("choose = &if(True, &1, {branch})"), "choose"),
            Ty::Func(vec![thunk(Ty::Int)], Box::new(Ty::Int))
        );
        assert_eq!(
            ty(&format!("choose = &if(True, (&1), {branch})"), "choose"),
            Ty::Func(vec![thunk(Ty::Int)], Box::new(Ty::Int))
        );
    }
    assert_eq!(
        ty("choose = &if(True, &1, {|| {|| 1}})", "choose"),
        Ty::Func(vec![thunk(thunk(Ty::Int))], Box::new(thunk(Ty::Int)))
    );
}

#[test]
fn lazy_depth_adjusts_at_most_one_shell_in_both_directions() {
    for branches in ["{|| {|| 1}}, {|| 2}", "{|| 1}, {|| {|| 2}}"] {
        assert_eq!(
            ty(&format!("value = if(True, {branches})"), "value"),
            thunk(Ty::Int)
        );
    }
    for branches in ["{|| {|| 1}}, 2", "1, {|| {|| 2}}"] {
        support::typecheck_with_rules(
            &format!("value = if(True, {branches})"),
            RuntimeSourcePolicy::script(),
        )
        .expect_err("two shell difference");
    }
}

#[test]
fn bare_call_retains_returned_thunk_and_grouped_call_consumes_once() {
    assert_eq!(
        ty(
            "def make() -> (-> Int) { {|| 1} }\nvalue = if(True, make(), {|| {|| 2}})",
            "value"
        ),
        thunk(Ty::Int)
    );
    assert_eq!(
        ty(
            "def make() -> (-> Int) { {|| 1} }\nvalue = if(True, (make()), {|| 2})",
            "value"
        ),
        Ty::Int
    );
}

#[test]
fn unknown_lazy_branches_require_context_and_same_placeholder_unifies() {
    support::typecheck_with_rules("choose = &if(True, &1, &2)", RuntimeSourcePolicy::script())
        .expect_err("concrete signature");
    assert_eq!(
        ty(
            "choose: ((-> Int), (-> Int) -> Int) = &if(True, &1, &2)",
            "choose"
        ),
        Ty::Func(vec![thunk(Ty::Int), thunk(Ty::Int)], Box::new(Ty::Int))
    );
    assert_eq!(
        ty("choose = &and(&1, &2)", "choose"),
        Ty::Func(vec![Ty::Bool, thunk(Ty::Bool)], Box::new(Ty::Bool))
    );
    support::typecheck_with_rules("choose = &and(&1, &1)", RuntimeSourcePolicy::script())
        .expect_err("placeholder roles conflict");
}

#[test]
fn single_lazy_branch_uses_standard_required_type() {
    assert_eq!(
        ty("act = &if_then(&1, &2)", "act"),
        Ty::Func(vec![Ty::Bool, thunk(Ty::Unit)], Box::new(Ty::Unit))
    );
    assert_eq!(
        ty("guard = &require(&1, &2)", "guard"),
        Ty::Func(
            vec![Ty::Bool, thunk(Ty::Error)],
            Box::new(Ty::Result(Box::new(Ty::Unit), Box::new(Ty::Error)))
        )
    );
}

#[test]
fn pattern_binding_success_uses_data_while_unbound_success_uses_lazy() {
    for success in ["&2", "(&2)", "x + &2"] {
        let result = ty(&format!("defenum Choice {{ Item(Int), Empty }}\nchoose = &if_let(&1, Choice::Item(x), {success}, 0)"), "choose");
        let Ty::Func(params, output) = result else {
            panic!("capture signature")
        };
        assert_eq!(params[1], Ty::Int);
        assert_eq!(*output, Ty::Int);
    }
    let result = ty(
        "defenum Choice { Item(Int), Empty }\nchoose = &if_let(&1, Choice::Item(_), &2, 0)",
        "choose",
    );
    let Ty::Func(params, output) = result else {
        panic!("capture signature")
    };
    assert_eq!(params[1], thunk(Ty::Int));
    assert_eq!(*output, Ty::Int);
    let result = ty("defenum Choice { Item(Int), Empty }\nvalue = if_let(Choice::Item(3), Choice::Item(x), {|| x}, {|| {|| 0}})", "value");
    assert_eq!(result, thunk(Ty::Int));
}

#[test]
fn binding_success_rejects_external_thunks_even_when_binding_unused() {
    for branch in ["outside", "&outside", "(&outside)"] {
        let error = support::typecheck_with_rules(&format!("defenum Choice {{ Item(Int), Empty }}\ndef outside() -> Int {{ 1 }}\nvalue = if_let(Choice::Item(3), Choice::Item(x), {branch}, 0)"), RuntimeSourcePolicy::script()).expect_err("external thunk");
        assert!(error.message.contains("DirectExpression"), "{error:?}");
    }
    let error = support::typecheck_with_rules("defenum Choice { Item(Int), Empty }\nchoose: (Choice, (-> Int) -> Int) = &if_let(&1, Choice::Item(x), &2, 0)", RuntimeSourcePolicy::script()).expect_err("external thunk parameter");
    assert!(error.message.contains("DirectExpression"), "{error:?}");
}

#[test]
fn binding_success_rejects_thunk_inferred_from_failure_branch() {
    let error = support::typecheck_with_rules("defenum Choice { Item(Int), Empty }\nchoose = &if_let(&1, Choice::Item(x), &2, {|| {|| 0}})", RuntimeSourcePolicy::script()).expect_err("external thunk inferred later");
    assert!(error.message.contains("DirectExpression"), "{error:?}");
}

#[test]
fn bare_special_form_call_retains_its_returned_thunk() {
    assert_eq!(
        ty(
            "value = if(True, if(True, {|| {|| 1}}, {|| {|| 2}}), {|| {|| 3}})",
            "value"
        ),
        thunk(Ty::Int)
    );
}

#[test]
fn bare_call_shell_survives_one_additional_depth_adjustment() {
    for branches in ["make(), {|| {|| 2}}", "{|| {|| 2}}, make()"] {
        assert_eq!(
            ty(
                &format!("def make() -> Int {{ 1 }}\nvalue = if(True, {branches})"),
                "value"
            ),
            thunk(Ty::Int)
        );
    }
}

#[test]
fn bare_pattern_consumer_call_keeps_its_source_call_shell() {
    assert_eq!(
        ty("value = if(True, is_match(1, 1), {|| {|| True}})", "value"),
        thunk(Ty::Bool)
    );
    support::typecheck_with_rules(
        "value = if(True, match 1 { _ => True }, {|| {|| True}})",
        RuntimeSourcePolicy::script(),
    )
    .expect_err("match expression has no source call shell");
}

#[test]
fn context_cannot_change_the_known_branch_normalization() {
    for source in [
        "choose: (Int -> Int) = &if(True, &1, 0)",
        "choose: ((-> (-> Int)) -> (-> Int)) = &if(True, &1, {|| 0})",
        "choose: ((-> (-> Int)) -> (-> Int)) = &if(True, &1, 1)",
    ] {
        support::typecheck_with_rules(source, RuntimeSourcePolicy::script())
            .expect_err("known normalized shell");
    }
}

#[test]
fn pattern_bound_thunk_is_an_ordinary_success_value() {
    assert_eq!(ty("defenum Choice { Item((-> Int)), Empty }\nvalue = if_let(Choice::Item({|| 7}), Choice::Item(f), f, {|| {|| 0}})", "value"), thunk(Ty::Int));
}

#[test]
fn later_call_argument_inference_keeps_direct_expression_constraint() {
    let error = support::typecheck_with_rules("def accept(f: (Int, $A, (-> $A) -> $A), value: $A) -> $A { f(1, value, {|| value}) }\nresult = accept(&if_let(&1, x, &2, &3), {|| 1})", RuntimeSourcePolicy::script()).expect_err("external thunk determined by later argument");
    assert!(error.message.contains("DirectExpression"), "{error:?}");
}

#[test]
fn ensure_capture_infers_unknown_predicate_from_value_contract() {
    let result = ty(
        "guard: (Int, (Int -> Boolean) -> Result<Int>) = &ensure(&1, &2, NoneError())",
        "guard",
    );
    let Ty::Func(params, _) = result else {
        panic!("capture signature")
    };
    assert_eq!(params[1], Ty::Func(vec![Ty::Int], Box::new(Ty::Bool)));
    let result = ty("guard = &ensure(1, &1, &2)", "guard");
    let Ty::Func(params, _) = result else {
        panic!("capture signature")
    };
    assert_eq!(params[0], Ty::Func(vec![Ty::Int], Box::new(Ty::Bool)));
}

#[test]
fn declared_generic_capture_keeps_constraint_through_specialization() {
    let source = "def build(value: $A) -> (Int, $A -> $A) { &if_let(&1, x, &2, value) }\nresult = build({|| 1})";
    let error = support::typecheck_with_rules(source, RuntimeSourcePolicy::script())
        .expect_err("external thunk after generic specialization");
    assert!(error.message.contains("DirectExpression"), "{error:?}");
    assert_eq!(ty("def build(value: $A) -> (Int, $A -> $A) { &if_let(&1, x, &2, value) }\nresult = build(0)", "result"),
        Ty::Func(vec![Ty::Int, Ty::Int], Box::new(Ty::Int)));
    assert_eq!(
        ty(
            "def outside() -> Int { 1 }\nvalue = match 1 { _ => &outside }",
            "value"
        ),
        thunk(Ty::Int)
    );
}

#[test]
fn result_error_lazy_capture_uses_fixed_error_requirement() {
    for consumer in ["map_err", "cause"] {
        let result = ty(
            &format!("change = &Result::{consumer}(Ok(1), &1)"),
            "change",
        );
        assert_eq!(
            result,
            Ty::Func(
                vec![thunk(Ty::Error)],
                Box::new(Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error)))
            )
        );
    }
}

#[test]
fn result_or_else_capture_requires_matching_result_thunk() {
    let result = Ty::Result(Box::new(Ty::Int), Box::new(Ty::Error));
    assert_eq!(
        ty("choose = &Result::or_else(Ok(1), &1)", "choose"),
        Ty::Func(vec![thunk(result.clone())], Box::new(result.clone()))
    );
    assert_eq!(
        ty("def choose(value: Result<$A>, next: (-> Result<$A>)) -> Result<$A> { Result::or_else(value, next) }\nselected = choose(Err(NoneError()), {|| Ok(1)})", "selected"),
        result
    );
    for source in [
        "Result::or_else(1, Ok(2))",
        "Result::or_else(Ok(1), 2)",
        "Result::or_else(Ok(1), Ok(\"wrong\"))",
        "Result::or_else(Ok(1), {|| {|| Ok(2)}})",
        "choose: (Result<Int>, Result<Int> -> Result<Int>) = &Result::or_else(&1, &2)",
    ] {
        support::typecheck_with_rules(source, RuntimeSourcePolicy::script()).expect_err(source);
    }
}

#[test]
fn result_or_else_preserves_lazy_capture_boundaries_and_declaration_identity() {
    for source in [
        "choose = &Result::or_else",
        "choose = &Result::or_else(Ok(1), (id(&1)))",
    ] {
        let error = support::resolve_with_builtin_prelude_result(source).expect_err(source);
        assert!(error.message.contains("Lazy"), "{error:?}");
    }
    assert_eq!(
        ty(
            "def or_else(left: Int, right: Int) -> Int { left + right }\nselected = or_else(1, 2)",
            "selected"
        ),
        Ty::Int
    );
}

#[test]
fn direct_expression_requirement_survives_generic_forwarding_and_capture() {
    let header = "def build(value: $A) -> (Int, $A -> $A) { &if_let(&1, x, &2, value) }";
    for consumer in [
        "def forward(value: $A) -> (Int, $A -> $A) { build(value) }\nresult = forward({|| 1})",
        "def forward(value: $A) -> (Int, $A -> $A) { build(value) }\ndef outer(value: $B) -> (Int, $B -> $B) { forward(value) }\nresult = outer({|| 1})",
        "build_fn: ((-> Int) -> (Int, (-> Int) -> (-> Int))) = &build",
    ] {
        let source = format!("{header}\n{consumer}");
        let error = support::typecheck_with_rules(&source, RuntimeSourcePolicy::script())
            .expect_err("external thunk through generic forwarding");
        assert!(error.message.contains("DirectExpression"), "{error:?}");
    }
    let forwarded = format!(
        "{header}\ndef forward(value: $A) -> (Int, $A -> $A) {{ build(value) }}\ndef outer(value: $B) -> (Int, $B -> $B) {{ forward(value) }}\nresult = outer(0)"
    );
    assert_eq!(
        ty(&forwarded, "result"),
        Ty::Func(vec![Ty::Int, Ty::Int], Box::new(Ty::Int))
    );
}

#[test]
fn migrated_lazy_calls_pass_normalized_callables_to_ordinary_functions() {
    fn ordinary_call(
        node: &scar::typed::TypedNode,
    ) -> (&scar::typed::TypedNode, &[scar::typed::TypedNode]) {
        match &node.node {
            TypedInner::Block(statements) => ordinary_call(statements.last().expect("call tail")),
            TypedInner::App(function, args) => (function, args),
            other => panic!("expected ordinary call, got {other:?}"),
        }
    }
    for (expression, lazy_positions) in [
        ("if(True, 1, 2)", vec![1, 2]),
        ("if_then(True, ())", vec![1]),
        ("require(True, NoneError())", vec![1]),
        ("ensure(1, {|x| x == 1}, NoneError())", vec![2]),
        ("Result::or_else(Ok(1), Ok(2))", vec![1]),
        ("Result::map_err(Ok(1), NoneError())", vec![1]),
    ] {
        let nodes = support::typecheck_with_rules(
            &format!("value = {expression}"),
            RuntimeSourcePolicy::script(),
        )
        .expect(expression);
        let value = nodes.iter().find_map(|node| match &node.node {
            TypedInner::Bind(pattern, value) if matches!(pattern.unlocated(), TypedPattern::Var(_, id) if id.name == "value") => Some(value.as_ref()),
            _ => None,
        }).expect("value binding");
        let (function, args) = ordinary_call(value);
        assert!(
            matches!(function.ty, Ty::UserFunc { .. }),
            "{expression}: {:?}",
            function.ty
        );
        for index in lazy_positions {
            assert!(
                matches!(args[index].node, TypedInner::Closure(..)),
                "{expression}: {:?}",
                args[index]
            );
            assert!(matches!(&args[index].ty, Ty::Func(params, _) if params.is_empty()));
        }
    }
}

#[test]
fn lazy_standard_call_rejects_facet_transport_but_accepts_delayed_facet_view() {
    let error = support::typecheck_with_rules(
        "defrecord User(name: String)\nvalue = if(True, (User.name), (User.name))",
        RuntimeSourcePolicy::script(),
    )
    .expect_err("ordinary function boundaries cannot transport compile-time Facet values");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::FacetCompileTimeOnly),
        "{error:?}"
    );
    assert_eq!(ty("defrecord User(name: String)\nuser = User(\"alice\")\nvalue = if(True, Facet::view(User.name, user), \"other\")", "value"), Ty::Str);
}

#[test]
fn lazy_standard_call_hoists_preserve_constructor_capabilities() {
    let declarations = r#"
deftrait FixtureFunctor where Self: Type<$A> { def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B> }
deftrait FixtureMonad where Self: FixtureFunctor { def run(self: Self<Int>) -> Int }
defenum Box<$T> { Box($T), }
impl FixtureFunctor for Box<$T> { def fmap(self: Box<$A>, mapper: ($A -> $B)) -> Box<$B> { match self { Box::Box(x) => Box::Box(mapper(x)), } } }
impl FixtureMonad for Box<$T> { def run(self: Self<Int>) -> Int { 1 } }
def retain_functor(value: $F<Int>) -> $F<Int> where $F: FixtureFunctor { FixtureFunctor::fmap(value, {|x| x}) }
def retain_monad(value: $F<Int>) -> $F<Int> where $F: FixtureMonad { evidence = FixtureMonad::run(value); value }
def stronger(value: FixtureMonad<Int>) -> Int { 1 }
"#;
    for expression in [
        "stronger(if(True, (source), (source)))",
        "match Result::map_err(Ok(source), NoneError()) { Ok(value) => stronger(value), Err(_) => 0 }",
        "match ensure(source, {|value: Box<Int>| True}, NoneError()) { Ok(value) => stronger(value), Err(_) => 0 }",
    ] {
        let valid = format!("{declarations}\nsource = retain_monad(Box::Box(1))\n{expression}");
        support::typecheck_with_rules(&valid, RuntimeSourcePolicy::script()).unwrap_or_else(|error| panic!("{valid}: {error:?}"));
        let invalid = format!("{declarations}\nsource = retain_functor(Box::Box(1))\n{expression}");
        let error = support::typecheck_with_rules(&invalid, RuntimeSourcePolicy::script()).expect_err("normalizing Lazy arguments must not strengthen retained capability views");
        assert_eq!(error.reason(), Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability), "{invalid}: {error:?}");
    }
    let source = format!("{declarations}\nsource = retain_functor(Box::Box(1))\nchecked = ensure(source, {{|value: Box<Int>| True}}, NoneError())");
    support::typecheck_with_rules(&source, RuntimeSourcePolicy::script())
        .expect("a matching explicit nominal predicate remains accepted");
}
