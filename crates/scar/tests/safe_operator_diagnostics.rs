#[allow(dead_code)]
mod support;
use diagnostics::{DiagnosticData, SourceRole, TypeDiagnosticReason};
use scar::typed::{TraitDispatch, TraitDispatchTarget, TypedInner, TypedNode};
use scar::types::Ty;

#[test]
fn result_operator_diagnostics_preserve_outer_contract_and_token_origin() {
    for (source, symbol, reason, signature) in [
        (
            "value = 1 + (8 / 2)",
            "/",
            TypeDiagnosticReason::ArgumentTypeMismatch,
            "(Int, Int) -> Result<Int, ZeroDivisionError>",
        ),
        (
            "value: Int = 7 % 3",
            "%",
            TypeDiagnosticReason::AnnotationTypeMismatch,
            "(Int, Int) -> Result<Int, ZeroModuloError>",
        ),
        (
            "def divide() -> Float { 8.0 / 2.0 }",
            "/",
            TypeDiagnosticReason::ReturnTypeMismatch,
            "(Float, Float) -> Result<Float, ZeroDivisionError>",
        ),
        (
            "def accept(value: Int) -> Int { value }\naccept(8 / 2)",
            "/",
            TypeDiagnosticReason::ArgumentTypeMismatch,
            "(Int, Int) -> Result<Int, ZeroDivisionError>",
        ),
    ] {
        let error =
            support::typecheck(support::resolve_with_builtin_prelude(source)).expect_err(source);
        let structured = error.structured.as_ref().expect("structured mismatch");
        assert_eq!(structured.reason.type_reason(), Some(reason), "{error:?}");
        let expected_primary = if source.starts_with("value =") {
            "(8 / 2)"
        } else if source.starts_with("value:") {
            "7 % 3"
        } else if source.starts_with("def divide") {
            "8.0 / 2.0"
        } else {
            "8 / 2"
        };
        assert_eq!(
            &source[structured.primary.span.start..structured.primary.span.end],
            expected_primary,
            "{error:?}"
        );
        let labels = structured
            .related
            .iter()
            .filter(|fact| fact.role.json_name() == "operator_signature")
            .collect::<Vec<_>>();
        assert_eq!(labels.len(), 1, "{error:?}");
        let label = labels[0];
        assert_eq!(&source[label.span.start..label.span.end], symbol);
        assert_eq!(label.ty.as_deref(), Some(signature));
        if let DiagnosticData::ArgumentRelation(data) = &structured.data {
            let numeric = if source.contains("Float") {
                "Float"
            } else {
                "Int"
            };
            assert_eq!(data.expected_type.as_deref(), Some(numeric));
            assert_eq!(
                data.actual_type.as_deref(),
                Some(format!("Result<{numeric}>").as_str())
            );
            assert!(!data
                .actual_type
                .as_deref()
                .unwrap()
                .contains("ZeroDivision"));
        }
        let rendered = diagnostics::structured_type_error_spec(structured);
        assert!(rendered
            .labels
            .iter()
            .any(|label| label.message == format!("`{symbol}`: {signature}")));
        let human = diagnostics::render_error("example.srt", source, &rendered);
        let expected_error = if symbol == "%" {
            "ZeroModuloError"
        } else {
            "ZeroDivisionError"
        };
        assert!(human.contains(expected_error), "{human}");
        let mut sources = diagnostics::SourceRegistry::new();
        let id = sources.register("example.srt", source);
        let json = diagnostics::serializable_diagnostic_by_id(&sources, id, "Scar", &rendered);
        let json_signatures = json
            .related
            .iter()
            .filter(|fact| fact.role == "operator_signature")
            .collect::<Vec<_>>();
        assert_eq!(json_signatures.len(), 1);
        assert_eq!(json_signatures[0].ty.as_deref(), Some(signature));
        assert_eq!(
            json_signatures[0].span,
            [label.span.start as u32, label.span.end as u32]
        );
        assert_eq!(json.reason.as_deref(), Some(structured.reason.as_str()));
        if let Some(help) = structured.remediation_text() {
            for forbidden in ["fmap", "unwrap", "do syntax"] {
                assert!(!help.contains(forbidden), "{help}");
            }
        }
    }
}

#[test]
fn user_operator_diagnostics_preserve_authored_or_omitted_error() {
    for (symbol, method, trait_name, error_contract) in [
        ("/", "safe_div", "Div", ", DomainFailure"),
        ("/", "safe_div", "Div", ", Error"),
        ("%", "safe_mod", "Mod", ""),
    ] {
        let source = format!(
            r#"deferror DomainFailure {{ "domain" }}
impl {trait_name} for String {{
  def {method}(self: String, rhs: String) -> Result<String{error_contract}> {{ Ok(self) }}
}}
value: String = "left" {symbol} "right""#
        );
        let error = support::typecheck(support::resolve_with_builtin_prelude(&source))
            .expect_err("Result value needs Result annotation");
        let diagnostic = error.structured.unwrap();
        let signature = diagnostic
            .related
            .iter()
            .find(|fact| fact.role == SourceRole::OperatorSignature)
            .expect("operator signature");
        assert_eq!(
            signature.ty.as_deref(),
            Some(format!("(String, String) -> Result<String{error_contract}>").as_str())
        );
    }
}

#[test]
fn unrelated_safe_operators_are_not_labels_of_another_value() {
    for source in [
        "unrelated = 8 / 2\nvalue: Int = \"bad\"",
        "def invalid() -> Int { unrelated = 8 / 2\n\"bad\" }",
    ] {
        let error = support::typecheck(support::resolve_with_builtin_prelude(source))
            .expect_err("String is not Int");
        let diagnostic = error.structured.unwrap();
        assert!(
            !diagnostic
                .related
                .iter()
                .any(|fact| fact.role == SourceRole::OperatorSignature),
            "{diagnostic:?}"
        );
    }
}

#[test]
fn only_the_result_operator_at_the_failing_value_is_labeled() {
    let source = "def invalid() -> Int { unrelated = 8 / 2\n7 % 3 }";
    let error = support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect_err("Result is not Int");
    let diagnostic = error.structured.unwrap();
    let signatures = diagnostic
        .related
        .iter()
        .filter(|fact| fact.role == SourceRole::OperatorSignature)
        .collect::<Vec<_>>();
    assert_eq!(signatures.len(), 1, "{diagnostic:?}");
    assert_eq!(
        &source[signatures[0].span.start..signatures[0].span.end],
        "%"
    );
}

#[test]
fn bounded_generic_does_not_claim_an_unselected_implementation_signature() {
    let source = "def invalid(value: $T) -> Int where $T: Div { value / value }";
    let error = support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect_err("generic Result is not Int");
    let diagnostic = error.structured.unwrap();
    assert_eq!(
        diagnostic.reason.type_reason(),
        Some(TypeDiagnosticReason::ReturnTypeMismatch)
    );
    assert!(!diagnostic
        .related
        .iter()
        .any(|fact| fact.role == SourceRole::OperatorSignature));
}

#[test]
fn result_on_left_operand_also_reports_its_operator_signature() {
    let source = "value = (8 / 2) * 3";
    let error = support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect_err("Result and Int cannot multiply");
    let diagnostic = error.structured.unwrap();
    assert!(
        diagnostic
            .related
            .iter()
            .any(|fact| fact.role == SourceRole::OperatorSignature),
        "{diagnostic:?}"
    );
}

#[test]
fn result_context_preserves_generic_payload_inference_and_finalizes_safe_dispatch() {
    let source = r#"defstruct Number<$T> { value: $T }
impl Number { def new(value: $T) -> Number<$T> { Number { value: value } } }
impl Div for Number<$T> {
  def safe_div(self: Number<$T>, rhs: Number<$T>) -> Result<Number<$T>> { Ok(self) }
}
value: Result<Number<Option<Int>>> = Number(Option::None) / Number(Option::None)
def identity(value: $T) -> $T { value }
def divide(value: $T) -> Result<$T> where $T: Div {
  identity(value) / identity(value)
}
divide(Number::new(7))
divide(Number::new("seven"))"#;
    let nodes = support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect("compatible Result context is preserved");
    let wrappers = nodes
        .iter()
        .filter_map(|node| match &node.node {
            TypedInner::Def(_, id, _, params, ret, _, body, _) if id.name == "divide" => {
                Some((params, ret, body))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(wrappers.len(), 2);
    for expected in [Ty::Int, Ty::Str] {
        let (params, ret, body) = wrappers
            .iter()
            .find(|(params, _, _)| matches!(&params[0].ty, Ty::Struct(_, fields) if fields[0].1 == expected))
            .expect("concrete payload specialization");
        assert!(matches!(ret, Ty::Result(payload, _) if payload.as_ref() == &params[0].ty));
        assert_eq!(&body.ty, *ret);
        let mut expressions = Vec::new();
        safe_operator_expressions(body, &mut expressions);
        let operators = expressions
            .iter()
            .filter(|node| matches!(&node.node, TypedInner::TraitCall { method_name, .. } if method_name == "safe_div"))
            .collect::<Vec<_>>();
        assert_eq!(operators.len(), 1);
        let operator = operators[0];
        let TypedInner::TraitCall {
            dispatch: TraitDispatch::Static(TraitDispatchTarget::UserFunction { fun_idx, .. }),
            receiver_ty,
            args,
            ..
        } = &operator.node
        else {
            panic!("safe operator needs concrete user dispatch: {operator:?}")
        };
        assert_eq!(receiver_ty, &params[0].ty);
        assert_eq!(&operator.ty, *ret);
        let definition = nodes
            .iter()
            .find(|node| matches!(&node.node, TypedInner::Def(index, ..) if index == fun_idx))
            .expect("safe dispatch definition");
        let TypedInner::Def(_, _, _, method_params, method_ret, _, method_body, _) =
            &definition.node
        else {
            unreachable!()
        };
        assert!(method_params.iter().all(|param| param.ty == params[0].ty));
        assert_eq!(method_ret, *ret);
        assert_eq!(&method_body.ty, *ret);
        for argument in args {
            let TypedInner::App(function, _) = &argument.node else {
                panic!("generic identity call must remain in safe operator operand: {argument:?}")
            };
            let Ty::UserFunc {
                fun_idx,
                params: call_params,
                ret: call_ret,
                ..
            } = &function.ty
            else {
                panic!("identity call needs concrete function identity")
            };
            assert_eq!(call_params, &vec![params[0].ty.clone()]);
            assert_eq!(call_ret.as_ref(), &params[0].ty);
            assert_eq!(argument.ty, params[0].ty);
            let definition = nodes.iter().find(|node| matches!(&node.node, TypedInner::Def(index, id, ..) if index == fun_idx && id.name == "identity"))
                .expect("operand call must name the identity definition");
            let TypedInner::Def(_, _, _, params, ret, _, body, _) = &definition.node else {
                unreachable!()
            };
            assert_eq!(&params[0].ty, ret);
            assert_eq!(&body.ty, ret);
        }
    }

    let invalid = "def divide(value: $T) -> Result<$T> { value / value }\ndivide(1)";
    let error = support::typecheck(support::resolve_with_builtin_prelude(invalid))
        .expect_err("concrete application cannot supply an undeclared Div bound");
    assert_eq!(
        error.reason(),
        Some(TypeDiagnosticReason::MissingGenericBound),
        "{error:?}"
    );
    assert_eq!(&invalid[error.span.start..error.span.end], "value");
    let data = error
        .structured
        .as_ref()
        .expect("structured missing bound")
        .data_json();
    assert_eq!(data["trait_name"], "Div");
}

fn safe_operator_expressions<'a>(node: &'a TypedNode, expressions: &mut Vec<&'a TypedNode>) {
    expressions.push(node);
    match &node.node {
        TypedInner::Block(nodes) => {
            for node in nodes {
                safe_operator_expressions(node, expressions);
            }
        }
        TypedInner::TraitCall { dispatch, args, .. } => {
            assert!(
                matches!(dispatch, TraitDispatch::Static(_)),
                "pending safe dispatch: {node:?}"
            );
            for arg in args {
                safe_operator_expressions(arg, expressions);
            }
        }
        TypedInner::App(function, args) => {
            safe_operator_expressions(function, expressions);
            for arg in args {
                safe_operator_expressions(arg, expressions);
            }
        }
        _ => {}
    }
}

#[test]
fn incompatible_result_payload_keeps_annotation_diagnostic_and_signature() {
    let source = "value: Result<Float> = 8 / 2";
    let error = support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect_err("payload differs");
    let diagnostic = error.structured.unwrap();
    assert_eq!(
        diagnostic.reason.type_reason(),
        Some(TypeDiagnosticReason::AnnotationTypeMismatch)
    );
    assert!(diagnostic
        .related
        .iter()
        .any(|fact| fact.role == SourceRole::OperatorSignature));
    if let DiagnosticData::ArgumentRelation(data) = diagnostic.data {
        assert_eq!(data.expected_type.as_deref(), Some("Result<Float>"));
        assert_eq!(data.actual_type.as_deref(), Some("Result<Int>"));
    } else {
        panic!("annotation mismatch requires its relation data");
    }
}
