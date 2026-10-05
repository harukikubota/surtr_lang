#[allow(dead_code)]
mod support;

use sigil::resolved::{Resolved, ResolvedId};

// Exercise Scar's declaration contract independently of the parser's suffix grammar.
fn suffix_id(id: &mut ResolvedId) {
    if id.name == "predicate" || id.name.ends_with("::predicate") {
        id.name.push('?');
        if let Some(name) = &mut id.qualified_name {
            name.push('?');
        }
    }
}

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).unwrap();
    let mut resolved = support::resolve_ast_with_builtin_prelude(ast).unwrap();
    for node in &mut resolved {
        match node {
            Resolved::Def(_, id, ..) => suffix_id(id),
            Resolved::TraitDef(_, _, _, _, methods, _) => {
                for method in methods {
                    suffix_id(&mut method.id);
                }
            }
            Resolved::TraitImplDef(_, _, _, _, _, _, methods) => {
                for method in methods {
                    suffix_id(&mut method.function_id);
                    if method.method_name == "predicate" {
                        method.method_name.push('?');
                    }
                }
            }
            _ => {}
        }
    }
    support::typecheck(resolved)
}

#[test]
fn boolean_suffix_accepts_boolean_and_argument_generic_signatures() {
    for source in [
        "def predicate(value: Int) -> Boolean { value > 0 }",
        "def predicate(value: $A) -> Boolean { True }",
        "deftrait FixturePredicate { def predicate(value: Self) -> Boolean }",
        "deftrait FixturePredicate { def predicate(value: Self) -> Boolean }\nimpl FixturePredicate for Int { def predicate(value: Self) { value > 0 } }",
        "defstruct Probe {}\nimpl Probe { def new() -> Probe { Probe {} }\ndef predicate() -> Boolean { True } }",
    ] {
        check(source).unwrap_or_else(|error| panic!("{source}: {error}"));
    }
}

#[test]
fn boolean_suffix_rejects_non_boolean_and_unresolved_signatures() {
    for source in [
        "def predicate(value: Int) -> Int { value }",
        "def predicate(value: $A) -> $A { value }",
        "def predicate::<$A>() -> $A { 0 }",
        "def predicate() { () }",
        "def predicate() -> Option<Boolean> { Option::Some(True) }",
        "def predicate() -> Result<Boolean> { Ok(True) }",
        "def positive(value: Int) -> Boolean { value > 0 }\ndef predicate() -> (Int -> Boolean) { &positive }",
        "deftrait FixturePredicate { def predicate(value: Self) -> Int }",
        "defstruct Probe {}\nimpl Probe { def new() -> Probe { Probe {} }\ndef predicate() -> Int { 0 } }",
    ] {
        let error = check(source).expect_err(source);
        assert!(error.message.contains("must return Boolean"), "{source}: {error}");
        assert!(error.message.contains("predicate?"), "{source}: {error}");
    }
}

#[test]
fn boolean_suffix_preserves_unsuffixed_signatures() {
    check("def ordinary(value: Int) -> Int { value }").unwrap();
    check("def ordinary(value: Int) -> Boolean { value > 0 }").unwrap();
}

#[test]
fn boolean_suffix_preserves_trait_impl_signature_compatibility() {
    let error = check("deftrait FixturePredicate { def predicate(value: Self) -> Boolean }\nimpl FixturePredicate for Int { def predicate(value: Self) -> Int { value } }").unwrap_err();
    assert!(error
        .message
        .contains("predicate? has an incompatible signature"));
}
