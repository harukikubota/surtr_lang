#[allow(dead_code)]
mod support;
use scar::typed::{TypedInner, TypedNode, TypedPattern};
use std::collections::HashSet;

fn visit(node: &TypedNode, visitor: &mut impl FnMut(&TypedNode)) {
    visitor(node);
    match &node.node {
        TypedInner::Def(_, _, _, _, _, _, body, _)
        | TypedInner::Closure(_, _, body)
        | TypedInner::CaptureClosure(_, _, body)
        | TypedInner::ExtractorClosure(_, _, body)
        | TypedInner::Bind(_, body) => visit(body, visitor),
        TypedInner::Block(items)
        | TypedInner::ConstructorCall(_, items)
        | TypedInner::TupleLiteral(items)
        | TypedInner::ListLiteral(items)
        | TypedInner::TraitCall { args: items, .. } => {
            for item in items {
                visit(item, visitor);
            }
        }
        TypedInner::App(function, args) => {
            visit(function, visitor);
            for arg in args {
                visit(arg, visitor);
            }
        }
        _ => {}
    }
}

#[test]
fn process_context_is_lexically_captured_by_escaping_and_nested_closures() {
    let typed = support::typecheck_module_source_result(
        r#"
defgenserver Logger {
  meta {
    instance: Worker
    init_policy: Eager
    state: Int
    handlers { out: OutHandler = StdOut }
  }
  @init def init() -> Result<Int> { Ok(0) }
  @call
  def pick(state: Int) -> Result<CallResult<(Unit -> (Unit -> Unit)), Int>> {
    closure = {|_outer: Unit| {|_inner: Unit| _handler = ctx.out; () }}
    Ok(CallResult::Reply(closure, state))
  }
}
"#,
    )
    .expect("escaping handler PID closure should typecheck");
    let mut acquired = HashSet::new();
    for node in &typed {
        visit(node, &mut |node| {
            if let TypedInner::Bind(TypedPattern::Var(_, id), rhs) = &node.node {
                if matches!(rhs.node, TypedInner::ProcessContextHandler { .. }) {
                    acquired.insert(id.unique_id);
                }
            }
        });
    }
    assert_eq!(
        acquired.len(),
        1,
        "acquire the slot once outside the closure"
    );
    let mut captured = 0;
    let mut getter_in_closure = false;
    for node in &typed {
        visit(node, &mut |node| {
            if let TypedInner::Closure(_, captures, body) = &node.node {
                if captures.iter().any(|id| acquired.contains(&id.unique_id)) {
                    captured += 1;
                }
                visit(body, &mut |child| {
                    getter_in_closure |=
                        matches!(child.node, TypedInner::ProcessContextHandler { .. });
                });
            }
        });
    }
    assert_eq!(
        captured, 2,
        "outer and inner must capture the acquired handler PID"
    );
    assert!(
        !getter_in_closure,
        "external calls must not resolve ctx again"
    );
}

#[test]
fn captured_handler_slots_deduplicate_and_coexist_with_ordinary_captures() {
    let typed = support::typecheck_module_source_result(
        r#"
defgenserver Logger {
  meta {
    instance: Worker
    init_policy: Eager
    state: Int
    handlers {
      out: OutHandler = StdOut
      err: OutHandler = StdErr
    }
  }
  @init def init() -> Result<Int> { Ok(0) }
  @call
  def pick(state: Int) -> Result<CallResult<(Unit -> (Unit -> Unit)), Int>> {
    prefix = "prefix"
    closure = {|_outer: Unit| {|_inner: Unit|
      _prefix = prefix
      _first = ctx.out
      _second = ctx.out
      _error = ctx.err
      ()
    }}
    Ok(CallResult::Reply(closure, state))
  }
}
"#,
    )
    .expect("multiple slots and ordinary captures should typecheck");
    let mut acquired = HashSet::new();
    for node in &typed {
        visit(node, &mut |node| {
            if let TypedInner::Bind(TypedPattern::Var(_, id), rhs) = &node.node {
                if matches!(rhs.node, TypedInner::ProcessContextHandler { .. }) {
                    acquired.insert(id.unique_id);
                }
            }
        });
    }
    assert_eq!(acquired.len(), 2, "each distinct slot is acquired once");
    let mut closures = 0;
    for node in &typed {
        visit(node, &mut |node| {
            if let TypedInner::Closure(_, captures, _) = &node.node {
                if captures.iter().any(|id| acquired.contains(&id.unique_id)) {
                    closures += 1;
                    assert_eq!(
                        captures
                            .iter()
                            .filter(|id| acquired.contains(&id.unique_id))
                            .count(),
                        2
                    );
                    assert!(captures.iter().any(|id| id.name == "prefix"));
                }
            }
        });
    }
    assert_eq!(closures, 2, "both closures preserve every required capture");
}
