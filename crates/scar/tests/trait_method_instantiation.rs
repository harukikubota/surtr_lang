use scar::typed::{TraitDispatch, TraitDispatchTarget, TypedInner};
use scar::types::Ty;

fn check(source: &str) -> Vec<scar::typed::TypedNode> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).expect("parse");
    scar::typecheck(sigil::resolve(ast).expect("resolve")).expect("typecheck")
}

#[test]
fn box_field_return_and_body_are_instantiated_together() {
    let nodes = check(
        r#"
defstruct Box<$T> { val: $T }
impl Box { def new(val: $T) -> Box<$T> { Box { val: val } } }
deftrait Read<$V> { def read::<$V>(self: Self) -> $V }
impl Read<$T> for Box<$T> { def read::<$T>(self: Self) -> $T { self.val } }
Read::read::<Int>(Box::new(1))
"#,
    );
    let call = nodes
        .iter()
        .find(|node| matches!(&node.node, TypedInner::TraitCall { .. }))
        .expect("call");
    assert_eq!(call.ty, Ty::Int);
    let TypedInner::TraitCall {
        dispatch: TraitDispatch::Static(TraitDispatchTarget::UserFunction { fun_idx, .. }),
        ..
    } = &call.node
    else {
        panic!("concrete dispatch required: {call:?}")
    };
    let def = nodes
        .iter()
        .find(|node| matches!(&node.node, TypedInner::Def(idx, ..) if idx == fun_idx))
        .expect("specialized method");
    let TypedInner::Def(_, _, _, params, ret, _, body, _) = &def.node else {
        unreachable!()
    };
    assert_eq!(*ret, Ty::Int);
    assert_eq!(body.ty, Ty::Int);
    assert!(matches!(&params[0].ty, Ty::Struct(_, fields) if fields[0].1 == Ty::Int));
}

#[test]
fn default_self_comes_from_explicit_or_expected_return() {
    for call in [
        "Default::default::<Int>()",
        "value: Int = Default::default()",
    ] {
        check(&format!(
            r#"
deftrait Default {{ def default::<Self>() -> Self }}
impl Default for Int {{ def default::<Int>() -> Int {{ 0 }} }}
{call}
"#
        ));
    }
}

#[test]
fn user_trait_method_is_not_replaced_by_a_matching_builtin_name() {
    let nodes = check(
        r#"
deftrait Show { def to_string(self: Self) -> String }
impl Show for Int { def to_string(self: Self) -> String { "custom" } }
Show::to_string(1)
"#,
    );
    let call = nodes
        .iter()
        .find(|node| matches!(&node.node, TypedInner::TraitCall { .. }))
        .expect("call");
    assert!(
        matches!(
            &call.node,
            TypedInner::TraitCall {
                dispatch: TraitDispatch::Static(TraitDispatchTarget::UserFunction { .. }),
                ..
            }
        ),
        "explicit implementation body must win: {call:?}"
    );
}

#[test]
fn builtin_trait_method_keeps_the_canonical_metadata_id() {
    let nodes = check(
        r#"
deftrait Add { def add(self: Self, rhs: Self) -> Self }
impl Add for Int { @builtin def add(self: Self, rhs: Self) -> Self }
Add::add(1, 2)
"#,
    );
    let expected = sindr::builtin::builtin_function_metas()
        .iter()
        .filter_map(sindr::builtin::BuiltinMeta::trait_method)
        .find(|metadata| {
            metadata.trait_name == "Add"
                && metadata.method_name == "add"
                && metadata.target == sindr::names::TypeName::Int
        })
        .expect("canonical Add<Int> metadata")
        .builtin_id;
    let call = nodes
        .iter()
        .find(|node| matches!(&node.node, TypedInner::TraitCall { .. }))
        .expect("call");
    assert!(matches!(
        &call.node,
        TypedInner::TraitCall {
            dispatch: TraitDispatch::Static(TraitDispatchTarget::Builtin(actual)),
            ..
        } if *actual == expected
    ));
}

#[test]
fn trait_argument_is_distinct_from_dispatch_subject() {
    let nodes = check(
        r#"
deftrait TryFrom<$Source> { def try_from::<Self>(value: $Source) -> Self }
impl TryFrom<Int> for String { def try_from::<String>(value: Int) -> String { "converted" } }
TryFrom::try_from::<String>(1)
"#,
    );
    let call = nodes
        .iter()
        .find(|node| matches!(&node.node, TypedInner::TraitCall { .. }))
        .unwrap();
    let TypedInner::TraitCall {
        obligation,
        receiver_ty,
        dispatch,
        ..
    } = &call.node
    else {
        unreachable!()
    };
    assert_eq!(call.ty, Ty::Str);
    assert_eq!(*receiver_ty, Ty::Str);
    assert_eq!(obligation.trait_args, vec![Ty::Int]);
    assert!(matches!(
        dispatch,
        TraitDispatch::Static(TraitDispatchTarget::UserFunction { .. })
    ));
}

#[test]
fn default_body_substitutes_self_trait_rta_and_closure_capture() {
    let nodes = check(
        r#"
deftrait Keep<$Value> {
  def seed::<$Value>(self: Self) -> $Value
  def keep::<$Value>(self: Self) -> $Value {
    get = {|| Keep::seed::<$Value>(self) }
    get()
  }
}
defstruct Box<$T> { value: $T }
impl Box { def new(value: $T) -> Box<$T> { Box { value: value } } }
impl Keep<$T> for Box<$T> { def seed::<$T>(self: Self) -> $T { self.value } }
Keep::keep::<Int>(Box::new(1))
"#,
    );
    let call = nodes
        .iter()
        .find(|node| matches!(&node.node, TypedInner::TraitCall { .. }))
        .unwrap();
    assert_eq!(call.ty, Ty::Int);
    assert!(matches!(
        &call.node,
        TypedInner::TraitCall {
            dispatch: TraitDispatch::Static(TraitDispatchTarget::UserFunction { .. }),
            ..
        }
    ));
}

#[test]
fn derived_generic_method_substitutes_field_obligation() {
    check(
        r#"
deftrait Default { def default::<Self>() -> Self }
impl Default for Int { def default::<Int>() -> Int { 0 } }
@derive Default
defstruct Wrapped<$T> { value: $T }
impl Wrapped { def new(value: $T) -> Wrapped<$T> { Wrapped { value: value } } }
Default::default::<Wrapped<Int>>()
"#,
    );
}

#[test]
fn unconstrained_generic_impl_method_uses_selected_substitution() {
    let nodes = check(
        r#"
defstruct Box<$T> { value: $T }
impl Box { def new(value: $T) -> Box<$T> { Box { value: value } } }
deftrait Echo { def echo(self: Self) -> Self }
impl Echo for Box<$T> { def echo(self: Self) -> Self { self } }
Echo::echo(Box::new(1))
"#,
    );
    let call = nodes
        .iter()
        .find(|node| matches!(&node.node, TypedInner::TraitCall { .. }))
        .expect("call");
    let TypedInner::TraitCall {
        dispatch: TraitDispatch::Static(TraitDispatchTarget::UserFunction { fun_idx, .. }),
        ..
    } = &call.node
    else {
        panic!("concrete dispatch required: {call:?}")
    };
    let def = nodes
        .iter()
        .find(|node| matches!(&node.node, TypedInner::Def(idx, ..) if idx == fun_idx))
        .expect("specialized method");
    let TypedInner::Def(_, _, _, params, ret, _, body, _) = &def.node else {
        unreachable!()
    };
    assert_eq!(
        params[0].ty,
        Ty::Struct("Global::Box".into(), vec![("value".into(), Ty::Int)])
    );
    assert_eq!(*ret, params[0].ty);
    assert_eq!(body.ty, params[0].ty);
}
