#![deny(dead_code)]

#[path = "support/case_registry.rs"]
mod case_registry;
#[allow(dead_code)]
mod support;

case_registry::register_case_suite!(
    "generic_trait_forwarding.rs",
    [
        ordinary_relay,
        unused_relay,
        multistage_relay,
        reversed_declarations,
        recursive_relay,
        mutually_recursive_relay,
        composite_slots,
        ordinary_erasure
    ]
);

fn check(source: &str) -> Vec<scar::typed::TypedNode> {
    support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect("forwarded specialization")
}

const INNER: &str = "def inner(source: Result<$A>, next: ($A -> Result<$B>)) -> Result<$B> { Monad::bind(source, next) }\n";
const OUTER: &str = "def outer(source: Result<$A>, next: ($A -> Result<$B>)) -> Result<$B> { inner(source, next) }\n";

fn ordinary_relay() {
    check(&format!(
        "{INNER}{OUTER}outer(Ok(7), {{|x: Int| Ok(inspect(x))}})"
    ));
}

fn unused_relay() {
    check(&format!("{INNER}{OUTER}"));
}

fn multistage_relay() {
    check(&format!("{INNER}{OUTER}def relay(s: Result<$A>, f: ($A -> Result<$B>)) -> Result<$B> {{ outer(s,f) }}\nrelay(Ok(7), {{|x: Int| Ok(inspect(x))}})"));
}

fn reversed_declarations() {
    check(&format!("{OUTER}{INNER}outer(Ok(7), {{|x: Int| Ok(x+1)}})"));
}

fn recursive_relay() {
    check(&format!("{INNER}def relay(n: Int, s: Result<$A>, f: ($A -> Result<$B>)) -> Result<$B> {{ if(n == 0, inner(s,f), relay(n-1,s,f)) }}\nrelay(2,Ok(7),{{|x: Int| Ok(x+1)}})"));
}

fn mutually_recursive_relay() {
    check(&format!("{INNER}def left(n: Int, s: Result<$A>, f: ($A -> Result<$B>)) -> Result<$B> {{ if(n == 0, inner(s,f), right(n-1,s,f)) }}\ndef right(n: Int, s: Result<$A>, f: ($A -> Result<$B>)) -> Result<$B> {{ left(n,s,f) }}\nright(2,Ok(7),{{|x: Int| Ok(x+1)}})"));
}

fn composite_slots() {
    check(&format!("{INNER}{OUTER}outer(Ok([1,2]), {{|x: List<Int>| Ok((x,True))}});\nouter(Ok([1,2]), {{|x: List<Int>| Ok(inspect(x))}})"));
}

fn ordinary_erasure() {
    check("def identity(x: $A) -> $A { x }\ndef relay(x: $A) -> $A { identity(x) }\nrelay(7);\nrelay(\"x\")");
}
