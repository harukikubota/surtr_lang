#[allow(dead_code)]
mod support;

use scar::typed::{TraitDispatch, TraitDispatchTarget, TypedInner, TypedNode};
use scar::types::{NominalType, Ty};

fn check(source: &str) -> Vec<scar::typed::TypedNode> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).expect("parse");
    support::typecheck(support::resolve_ast_with_builtin_prelude(ast).expect("resolve"))
        .expect("typecheck")
}

#[test]
fn finite_trait_specialization_chain_fits_the_default_test_stack() {
    const SPECIALIZATION_DEPTH: usize = 16;

    let mut source = String::from(
        "deftrait Echo { def echo(self: Self, value: $T) -> $T }\n\
         impl Echo for Int { def echo(self: Self, value: $T) -> $T { value } }\n",
    );
    for depth in 0..SPECIALIZATION_DEPTH {
        let body = if depth + 1 == SPECIALIZATION_DEPTH {
            "Echo::echo(1, value)".to_owned()
        } else {
            format!("Echo::echo(1, chain_{}(value))", depth + 1)
        };
        source.push_str(&format!(
            "def chain_{depth}(value: $T) -> $T {{ {body} }}\n"
        ));
    }
    source.push_str("chain_0(\"left\")\n");

    std::thread::Builder::new()
        .name("scar-specialization-stack-regression".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || check(&source))
        .expect("specialization stack regression thread should spawn")
        .join()
        .expect("finite specialization should fit the normal debug stack");
}

#[test]
fn generic_forwarding_chain_64_fits_the_cli_stack_in_both_declaration_orders() {
    for source in [
        include_str!("../../../tests/profile/scar_compile/generated/generic_chain_064.srt"),
        include_str!("../../../tests/profile/scar_compile/generated/generic_chain_reverse_064.srt"),
    ] {
        std::thread::Builder::new()
            .name("scar-generic-forwarding-stack-regression".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || check(source))
            .expect("generic forwarding regression thread should spawn")
            .join()
            .expect("64 generic forwarding stages must fit the CLI stack budget");
    }
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
    assert!(matches!(&params[0].ty, Ty::Struct(_, fields) if fields.arguments[0] == Ty::Int));
}

#[test]
fn default_self_comes_from_explicit_or_expected_return() {
    for call in [
        "Default::default::<Int>()",
        "value: Int = Default::default()",
    ] {
        check(&format!(
            r#"
{call}
"#
        ));
    }
}

#[test]
fn user_trait_method_is_not_replaced_by_a_matching_builtin_name() {
    let nodes = check(
        r#"
deftrait FixtureShow { def to_string(self: Self) -> String }
impl FixtureShow for Int { def to_string(self: Self) -> String { "custom" } }
FixtureShow::to_string(1)
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
Add::add(1, 2)
"#,
    );
    let expected = sindr::builtin::builtin_function_metas()
        .iter()
        .filter_map(sindr::builtin::BuiltinMeta::trait_method)
        .find(|metadata| {
            metadata.trait_name == "Add"
                && metadata.method_name == "add"
                && metadata.targets.contains(&sindr::names::TypeName::Int)
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
fn builtin_show_trait_method_keeps_the_to_string_metadata_id() {
    let nodes = check(
        r#"
Show::to_string(1)
"#,
    );
    let expected = sindr::builtin::builtin_meta_by_name("to_string")
        .expect("canonical to_string metadata")
        .builtin_id();
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
fn builtin_trait_method_rejects_runtime_signature_drift() {
    for declaration in [
        "@builtin def to_string(self: Self) -> Int",
        "@builtin def to_string(self: Self, extra: Self) -> String",
        "@builtin def to_string(value: String) -> String",
    ] {
        let int_source = include_str!("../../../lib/types/int.srt")
            .replace("@builtin def to_string(self: Self) -> String", declaration);
        let error = support::typecheck_std_modules_with_overrides(&[("Int", &int_source)])
            .expect_err("builtin signature drift must fail in the real standard environment");
        assert!(
            error.message.contains("canonical runtime signature"),
            "{error:?}"
        );
    }
}

#[test]
fn trait_argument_is_distinct_from_dispatch_subject() {
    let nodes = check(
        r#"
deftrait FixtureTryConvert<$Source> { def try_to::<Self>(value: $Source) -> Self }
impl FixtureTryConvert<Int> for String { def try_to::<String>(value: Int) -> String { "converted" } }
FixtureTryConvert::try_to::<String>(1)
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
        Ty::Struct("Global::Box".into(), NominalType::new(vec![Ty::Int]))
    );
    assert_eq!(*ret, params[0].ty);
    assert_eq!(body.ty, params[0].ty);
}

#[test]
fn generic_caller_specializes_selected_trait_method_signature() {
    let nodes = check(
        r#"
deftrait Echo { def echo(self: Self, value: $T) -> $T }
impl Echo for Boolean { def echo(self: Self, value: $T) -> $T { value } }
impl Echo for Int { def echo(self: Self, value: $T) -> $T { value } }
def wrap(value: $T) -> $T { Echo::echo(1, value) }
wrap(1);
wrap("left");
wrap(2);
keep_int: (Int -> Int) = &wrap(&1)
keep_string: (String -> String) = &wrap(&1)
keep_int_again: (Int -> Int) = &wrap(&1)
keep_int(3);
keep_string("right");
keep_int_again(4)
"#,
    );
    let wrappers = nodes
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Def(index, id, _, params, ret, _, body, _) if id.name == "wrap" => {
                Some((*index, params, ret, body))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(wrappers.len(), 2, "Int and String need exactly two bodies");
    let mut wrapper_indices = Vec::new();
    let mut method_indices = Vec::new();
    for expected in [Ty::Int, Ty::Str] {
        let (index, params, ret, body) = wrappers
            .iter()
            .find(|(_, _, ret, _)| **ret == expected)
            .expect("specialized wrapper result");
        assert_eq!(params[0].ty, expected);
        assert_eq!(**ret, expected);
        assert_eq!(body.ty, expected);
        let mut expressions = Vec::new();
        collect_callable_expressions(body, &mut expressions);
        let calls = expressions
            .iter()
            .filter(|node| matches!(&node.node, TypedInner::TraitCall { method_name, .. } if method_name == "echo"))
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 1);
        let call = calls[0];
        let TypedInner::TraitCall {
            receiver_ty,
            dispatch: TraitDispatch::Static(TraitDispatchTarget::UserFunction { fun_idx, .. }),
            args,
            ..
        } = &call.node
        else {
            panic!("concrete selected method required: {call:?}")
        };
        assert_eq!(*receiver_ty, Ty::Int, "Boolean candidate must be rejected");
        assert_eq!(call.ty, expected);
        assert_eq!(args[1].ty, expected);
        let definition = nodes
            .iter()
            .find(|node| matches!(&node.node, TypedInner::Def(idx, ..) if idx == fun_idx))
            .expect("dispatch must name an emitted method definition");
        let TypedInner::Def(_, _, _, params, ret, _, body, _) = &definition.node else {
            unreachable!()
        };
        assert_eq!(params[0].ty, Ty::Int);
        assert_eq!(params[1].ty, expected);
        assert_eq!(*ret, expected);
        assert_eq!(body.ty, expected);
        let mut expressions = Vec::new();
        collect_callable_expressions(body, &mut expressions);
        assert!(expressions.iter().any(|node| matches!(&node.node, TypedInner::Var(id) if id.unique_id == params[1].id.unique_id)),
            "selected body must return its value parameter: {definition:?}");
        wrapper_indices.push(*index);
        method_indices.push(*fun_idx);
    }
    assert_ne!(wrapper_indices[0], wrapper_indices[1]);
    assert_ne!(method_indices[0], method_indices[1]);
    assert!(!nodes.iter().any(|node| matches!(&node.node, TypedInner::Def(_, id, _, params, ..) if id.name.ends_with("::echo") && params.first().is_some_and(|param| param.ty == Ty::Bool))),
        "rejected candidate must not emit an executable specialization");

    let direct_calls = nodes
        .iter()
        .map(|node| match &node.node {
            TypedInner::Semi(value) => value.as_ref(),
            _ => node,
        })
        .filter_map(|node| match &node.node {
            TypedInner::App(fun, _) => match &fun.ty {
                Ty::UserFunc { fun_idx, .. } if wrapper_indices.contains(fun_idx) => {
                    Some((node.ty.clone(), *fun_idx))
                }
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        direct_calls,
        vec![
            (Ty::Int, wrapper_indices[0]),
            (Ty::Str, wrapper_indices[1]),
            (Ty::Int, wrapper_indices[0])
        ]
    );
    let mut capture_calls = Vec::new();
    for node in &nodes {
        if let TypedInner::Bind(_, value) = &node.node {
            if let TypedInner::CaptureClosure(_, _, body) = &value.node {
                let mut expressions = Vec::new();
                collect_callable_expressions(body, &mut expressions);
                for expression in expressions {
                    if let TypedInner::App(fun, _) = &expression.node {
                        if let Ty::UserFunc {
                            fun_idx,
                            params,
                            ret,
                            ..
                        } = &fun.ty
                        {
                            if wrapper_indices.contains(fun_idx) {
                                assert_eq!(params, &vec![expression.ty.clone()]);
                                assert_eq!(**ret, expression.ty);
                                capture_calls.push((expression.ty.clone(), *fun_idx));
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        capture_calls, direct_calls,
        "captures must reuse the exact concrete wrapper definitions"
    );
}

fn collect_callable_expressions<'a>(node: &'a TypedNode, nodes: &mut Vec<&'a TypedNode>) {
    nodes.push(node);
    match &node.node {
        TypedInner::Block(items) => {
            for item in items {
                collect_callable_expressions(item, nodes);
            }
        }
        TypedInner::App(fun, args) => {
            collect_callable_expressions(fun, nodes);
            for arg in args {
                collect_callable_expressions(arg, nodes);
            }
        }
        TypedInner::TraitCall { args, .. } => {
            for arg in args {
                collect_callable_expressions(arg, nodes);
            }
        }
        _ => {}
    }
}

#[test]
fn generated_boolean_eq_uses_canonical_builtin_dispatch() {
    let boolean_source = include_str!("../../../lib/types/boolean.srt")
        .replace("@derive Eq\n", "")
        .replace(
            "@builtin\ndefenum Boolean",
            "@derive Eq\n@builtin\ndefenum Boolean",
        );
    let stages = support::std_module_stages_with_overrides(&[("Boolean", &boolean_source)]);
    let index = sigil::precollect_declaration_index(&stages).expect("precollect");
    let ast = spire::parse_with_context(
        "equal = Eq::eq(True, False)\nEq::neq(True, False)\n",
        spire::ParserContext::project(0),
    )
    .expect("parse");
    let resolved =
        sigil::resolve_staged_program_with_state(&stages, ast, &index, None).expect("resolve");
    let nodes = scar::typecheck_staged_program_with_context(
        resolved,
        scar::TypecheckContext {
            runtime_policy: sindr::policy::RuntimeSourcePolicy::std_module(),
            enforce_builtin_type_contracts: true,
            allow_private_facet_inspection: false,
            test_script: false,
        },
    )
    .expect("typecheck")
    .nodes;
    for method in ["eq", "neq"] {
        let expected = sindr::builtin::builtin_function_metas()
            .iter()
            .filter_map(sindr::builtin::BuiltinMeta::trait_method)
            .find(|meta| {
                meta.trait_name == "Eq"
                    && meta.method_name == method
                    && meta.targets.contains(&sindr::names::TypeName::Boolean)
            })
            .expect("canonical Boolean equality metadata")
            .builtin_id;
        assert!(nodes.iter().any(|node| {
            let expression = match &node.node {
                TypedInner::Bind(_, value) => value.as_ref(),
                _ => node,
            };
            matches!(&expression.node,
                TypedInner::TraitCall {
                    method_name, dispatch: TraitDispatch::Static(TraitDispatchTarget::Builtin(actual)), ..
                } if method_name == method && *actual == expected
            )
        }), "generated Boolean {method} must use its canonical builtin");
    }
}

#[test]
fn generated_user_enum_equality_keeps_user_dispatch() {
    let nodes = check("@derive Eq\ndefenum Switch { On, Off }\nEq::eq(Switch::On, Switch::Off)");
    assert!(
        nodes.iter().any(|node| matches!(
            &node.node,
            TypedInner::TraitCall {
                dispatch: TraitDispatch::Static(TraitDispatchTarget::UserFunction { .. }),
                ..
            }
        )),
        "ordinary enum derive must retain its generated implementation"
    );
}

#[test]
fn range_derives_require_endpoint_traits() {
    for operation in ["Eq::eq", "Compare::compare"] {
        let source = format!(
            "defstruct Endpoint {{ value: Int }}\nimpl Endpoint {{ def new(value: Int) -> Self {{ Endpoint {{ value }} }} }}\n{operation}(Range(Endpoint::new(1), Endpoint::new(2)), Range(Endpoint::new(1), Endpoint::new(2)))"
        );
        let ast =
            spire::parse_with_context(&source, spire::ParserContext::project(0)).expect("parse");
        let resolved = support::resolve_ast_with_builtin_prelude(ast).expect("resolve");
        let error = support::typecheck(resolved).expect_err("endpoint Trait must be required");
        assert!(error.message.contains("Endpoint"), "{error:?}");
        assert!(!error.message.contains("must define `new`"), "{error:?}");
    }
}
