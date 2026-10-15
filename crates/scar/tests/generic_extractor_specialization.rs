#![deny(dead_code)]

#[path = "support/case_registry.rs"]
mod case_registry;
#[allow(dead_code)]
mod support;

use scar::typed::{TraitDispatch, TypedInner, TypedMatchPattern, TypedNode, TypedPattern};
use scar::types::Ty;
use std::collections::HashSet;

case_registry::register_case_suite!(
    "generic_extractor_specialization.rs",
    [
        named_generic_extractor_apply_pattern,
        named_generic_extractor_match,
        named_generic_extractor_safe_bind,
        named_generic_extractor_do_bind,
        named_generic_extractor_consumer_capture,
        generic_extractor_consumer_capture_requires_its_body_capability,
        named_fixed_payload_extractor_generic_outer,
        named_generic_extractor_separates_instantiations,
        unused_named_generic_extractor_is_not_executable,
        named_generic_extractor_nested_record_head,
        named_generic_extractor_forward_declaration,
        generic_forwarding_inside_nested_record_pre_argument,
    ]
);

const RELAY: &str = r#"
impl Int {
  defextractor relay(source: Result<$A>) -> MatchResult<$A> {
    value =? Monad::bind(source, {|x| Ok(x)})
    MatchResult::Ok(value)
  }
}
"#;

fn pattern_heads<'a>(pattern: &'a TypedPattern, heads: &mut Vec<&'a Ty>) {
    match pattern {
        TypedPattern::Located(_, inner) | TypedPattern::As(_, inner, _) => {
            pattern_heads(inner, heads)
        }
        TypedPattern::Extractor {
            extractor_ty,
            pre_args,
            items,
            ..
        } => {
            heads.push(extractor_ty);
            for arg in pre_args {
                node_heads(arg, heads);
            }
            for item in items {
                pattern_heads(item, heads);
            }
        }
        TypedPattern::Tuple(_, items) | TypedPattern::Constructor { fields: items, .. } => {
            for item in items {
                pattern_heads(item, heads);
            }
        }
        TypedPattern::ListCons(_, head, tail) => {
            pattern_heads(head, heads);
            pattern_heads(tail, heads);
        }
        _ => {}
    }
}

fn match_heads<'a>(pattern: &'a TypedMatchPattern, heads: &mut Vec<&'a Ty>) {
    match pattern {
        TypedMatchPattern::As(inner, _) => match_heads(inner, heads),
        TypedMatchPattern::Extractor {
            extractor_ty,
            pre_args,
            items,
            ..
        } => {
            heads.push(extractor_ty);
            for arg in pre_args {
                node_heads(arg, heads);
            }
            for item in items {
                match_heads(item, heads);
            }
        }
        TypedMatchPattern::Tuple(items)
        | TypedMatchPattern::Record(items)
        | TypedMatchPattern::Or(items)
        | TypedMatchPattern::Constructor { fields: items, .. }
        | TypedMatchPattern::ErrorPayload { fields: items, .. } => {
            for item in items {
                match_heads(item, heads);
            }
        }
        TypedMatchPattern::ListCons(head, tail) => {
            match_heads(head, heads);
            match_heads(tail, heads);
        }
        _ => {}
    }
}

fn node_heads<'a>(node: &'a TypedNode, heads: &mut Vec<&'a Ty>) {
    match &node.node {
        TypedInner::ApplyPattern { value, pattern, .. } => {
            pattern_heads(pattern, heads);
            node_heads(value, heads);
        }
        TypedInner::SafeBind(pattern, rhs, ..) | TypedInner::Bind(pattern, rhs) => {
            pattern_heads(pattern, heads);
            node_heads(rhs, heads);
        }
        TypedInner::DoSafeBind(control) => {
            pattern_heads(&control.pattern, heads);
            node_heads(&control.rhs, heads);
            node_heads(&control.continuation, heads);
        }
        TypedInner::Match(source, arms) => {
            node_heads(source, heads);
            for arm in arms {
                match_heads(&arm.pattern, heads);
                node_heads(&arm.body, heads);
            }
        }
        TypedInner::Def(_, _, _, _, _, _, body, _)
        | TypedInner::ExtractorDef(_, _, _, _, _, body, _)
        | TypedInner::Closure(_, _, body)
        | TypedInner::ExtractorClosure(_, _, body)
        | TypedInner::CaptureClosure(_, _, body) => node_heads(body, heads),
        TypedInner::Semi(inner) => node_heads(inner, heads),
        TypedInner::Block(nodes)
        | TypedInner::TupleLiteral(nodes)
        | TypedInner::ListLiteral(nodes) => {
            for node in nodes {
                node_heads(node, heads);
            }
        }
        TypedInner::App(fun, args)
        | TypedInner::Capture(fun, args)
        | TypedInner::InjectCall(fun, args) => {
            node_heads(fun, heads);
            for arg in args {
                node_heads(arg, heads);
            }
        }
        TypedInner::TraitCall { dispatch, args, .. } => {
            assert!(
                matches!(dispatch, TraitDispatch::Static(_)),
                "executable Extractor traversal must finalize dispatch: {node:?}"
            );
            for arg in args {
                node_heads(arg, heads);
            }
        }
        _ => {}
    }
}

fn check_heads(source: &str, minimum: usize) -> usize {
    let nodes = support::typecheck_with_builtin_prelude(source);
    let definitions = nodes
        .iter()
        .filter_map(|node| match node.node {
            TypedInner::Def(index, ..) | TypedInner::ExtractorDef(index, ..) => Some(index),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut heads = Vec::new();
    for node in &nodes {
        node_heads(node, &mut heads);
    }
    assert!(
        heads.len() >= minimum,
        "expected Extractor execution references"
    );
    let mut indices = HashSet::new();
    for head in heads {
        if let Ty::UserFunc {
            fun_idx,
            params,
            ret,
            ..
        } = head
        {
            assert!(
                definitions.contains(fun_idx),
                "Extractor head must name an executable definition: {head:?}"
            );
            indices.insert(*fun_idx);
            let definition = nodes
                .iter()
                .find(|node| matches!(&node.node, TypedInner::ExtractorDef(index, ..) if index == fun_idx))
                .expect("Extractor reference must name an Extractor definition");
            let TypedInner::ExtractorDef(_, _, _, definition_params, definition_ret, body, _) =
                &definition.node
            else {
                unreachable!()
            };
            assert_eq!(
                params,
                &definition_params
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect::<Vec<_>>()
            );
            assert_eq!(ret.as_ref(), definition_ret);
            assert_eq!(&body.ty, definition_ret);
        }
    }
    indices.len()
}

fn named_generic_extractor_apply_pattern() {
    check_heads(&format!("{RELAY}\napply_pattern(Ok(7), Int::relay(_1))"), 1);
}
fn named_generic_extractor_match() {
    check_heads(
        &format!("{RELAY}\nmatch Ok(7) {{ Int::relay(value) => value, _ => 0 }}"),
        1,
    );
}
fn named_generic_extractor_safe_bind() {
    check_heads(&format!("{RELAY}\ndef run() -> Result<Int> {{ Int::relay(value) =? Ok(Ok(7)); Ok(value) }}\nrun()"), 1);
}
fn named_generic_extractor_do_bind() {
    check_heads(
        &format!("{RELAY}\ndo {{ Int::relay(value) <- Ok(Ok(7)); Ok(value) }}"),
        1,
    );
}
fn named_generic_extractor_consumer_capture() {
    check_heads(&format!("{RELAY}\nconsume: (Result<Int> -> Result<Int>) = &apply_pattern(&1, Int::relay(_1))\nconsume(Ok(7))"), 1);
}

fn generic_extractor_consumer_capture_requires_its_body_capability() {
    let source = r#"
impl Int {
  defextractor add_values(source: Result<$A>) -> MatchResult<$A> {
    value =? Monad::bind(source, {|x| Ok(x + x)})
    MatchResult::Ok(value)
  }
}
consume: (Result<Int> -> Result<Int>) = &apply_pattern(&1, Int::add_values(_1))
consume(Ok(7))
"#;
    let error = support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect_err("concrete capture input must not infer an undeclared generic Add capability");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingGenericBound),
        "{error:?}"
    );
    assert_eq!(&source[error.span.start..error.span.end], "x");
    let data = error
        .structured
        .as_ref()
        .expect("structured capability failure")
        .data_json();
    assert_eq!(data["trait_name"], "Add");
}
fn named_fixed_payload_extractor_generic_outer() {
    check_heads(
        r#"
impl Int {
  defextractor fixed(source: Result<$A>) -> MatchResult<Int> {
    value =? Monad::bind(source, {|x| Ok(7)})
    MatchResult::Ok(value)
  }
}
def outer(source: Result<$A>) -> Result<Int> { apply_pattern(source, Int::fixed(_1)) }
outer(Ok(7))
outer(Ok("x"))
"#,
        2,
    );
}
fn named_generic_extractor_separates_instantiations() {
    assert_eq!(check_heads(&format!("{RELAY}\napply_pattern(Ok(7), Int::relay(_1))\napply_pattern(Ok(\"x\"), Int::relay(_1))\napply_pattern(Ok(7), Int::relay(_1))"), 3), 2);
}
fn unused_named_generic_extractor_is_not_executable() {
    let nodes = support::typecheck_with_builtin_prelude(RELAY);
    assert!(!nodes.iter().any(
        |node| matches!(&node.node, TypedInner::ExtractorDef(_, id, ..) if id.name == "relay")
    ));
}

fn named_generic_extractor_nested_record_head() {
    check_heads(
        &format!(
            "{RELAY}\ndefrecord Holder(value: Result<Int>)\n\
             match Holder(Ok(7)) {{ Holder(value: Int::relay(value)) => value, _ => 0 }}"
        ),
        1,
    );
}

fn named_generic_extractor_forward_declaration() {
    check_heads(
        r#"
def outer(source: Result<$A>) -> Result<Int> { apply_pattern(source, Int::fixed(_1)) }
impl Int {
  defextractor fixed(source: Result<$A>) -> MatchResult<Int> {
    value =? Monad::bind(source, {|_| Ok(7)})
    MatchResult::Ok(value)
  }
}
outer(Ok("x"))
outer(Ok(7))
"#,
        2,
    );
}

fn generic_forwarding_inside_nested_record_pre_argument() {
    check_heads(
        r#"
defrecord Holder(value: Int)
impl Int {
  defextractor offset(amount: Int, source: Int) -> MatchResult<Int> {
    MatchResult::Ok(amount + source)
  }
}
def inner(source: Result<$A>, next: ($A -> Result<$B>)) -> Result<$B> {
  Monad::bind(source, next)
}
def outer(source: Result<$A>) -> Int {
  match Holder(10) {
    Holder(value: Int::offset(match inner(source, {|_| Ok(1)}) { Ok(value) => value, _ => 0 }, value)) => value,
    _ => 0,
  }
}
outer(Ok(7))
outer(Ok("x"))
"#,
        2,
    );
}
