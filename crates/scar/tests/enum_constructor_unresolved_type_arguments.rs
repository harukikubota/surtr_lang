#[allow(dead_code)]
mod support;

use scar::typed::TypedNode;

fn typecheck(source: &str) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse");
    let resolved = support::resolve_ast_with_builtin_prelude(ast).expect("source should resolve");
    support::typecheck(resolved)
}

const DECLARATIONS: &str = r#"
def is_some(value: Option<$T>) -> Int { 1 }
def is_left(value: Either<$L, $R>) -> Int { 1 }
def check_none(make: (Int -> Option<$T>)) -> Int { 1 }
"#;

#[test]
fn unresolved_bare_enum_constructor_type_arguments_are_rejected() {
    for (expression, source_text, enum_name, constructor, ordinal) in [
        (
            "is_some(Option::None)",
            "Option::None",
            "Option",
            "Option::None",
            0,
        ),
        (
            "is_left(Either::Left(\"term\"))",
            "Either::Left(\"term\")",
            "Either",
            "Either::Left",
            1,
        ),
        (
            "value = Option::None",
            "Option::None",
            "Option",
            "Option::None",
            0,
        ),
        (
            "value = Either<String, _>::Left(\"term\")",
            "Either<String, _>::Left(\"term\")",
            "Either",
            "Either::Left",
            1,
        ),
        (
            "def unresolved_specializable::<$T>() -> Option<$T> { ignored = Option::None; Option::None }",
            "Option::None",
            "Option",
            "Option::None",
            0,
        ),
    ] {
        let source = format!("{DECLARATIONS}\n{expression}");
        let error = typecheck(&source).expect_err(expression);
        let diagnostic = error.structured.expect("structured enum diagnostic");
        assert_eq!(
            diagnostic.reason,
            diagnostics::TypeDiagnosticReason::UnresolvedEnumConstructorTypeArgument
        );
        assert_eq!(
            diagnostic.origin,
            diagnostics::DiagnosticOrigin::EnumConstructor { ordinal }
        );
        assert_eq!(&source[error.span.start..error.span.end], source_text);
        let data = diagnostic.data_json();
        assert_eq!(data["kind"], "EnumConstructorTypeArgument");
        assert_eq!(data["enum_name"], enum_name);
        assert_eq!(data["constructor"], constructor);
        assert_eq!(data["ordinal"], ordinal);
        assert_eq!(data["constraint_status"], "Insufficient");
    }
}

#[test]
fn expected_types_resolve_all_bare_enum_constructor_type_arguments() {
    for expression in [
        "none: Option<Int> = Option::None",
        "left: Either<String, Int> = Either::Left(\"term\")",
        "def generic_none::<$T>() -> Option<$T> { Option::None }",
    ] {
        let source = format!("{DECLARATIONS}\n{expression}");
        typecheck(&source).expect(expression);
    }
}

#[test]
fn enum_finalization_does_not_relax_callable_return_type_argument_rules() {
    let source = format!(
        r#"{DECLARATIONS}
def make::<$T>() -> Option<$T> {{ Option::None }}
factory = {{|value| [Option::Some(value), make()]}}"#
    );
    let error = typecheck(&source)
        .expect_err("an unresolved closure and callable RTA must not cross a binding boundary");
    assert!(
        error
            .message
            .contains("Callable binding requires a concrete signature"),
        "{error:?}"
    );
}

fn assert_nested_none_has_int_slot(node: &TypedNode) -> usize {
    use scar::{typed::TypedInner, types::Ty};
    match &node.node {
        TypedInner::ConstructorCall(_, _) => {
            assert_eq!(node.ty, Ty::Enum("Global::Option".into(), vec![Ty::Int]));
            1
        }
        TypedInner::Bind(_, rhs) | TypedInner::Closure(_, _, rhs) => {
            assert_nested_none_has_int_slot(rhs)
        }
        TypedInner::TupleLiteral(nodes)
        | TypedInner::ListLiteral(nodes)
        | TypedInner::Block(nodes) => nodes.iter().map(assert_nested_none_has_int_slot).sum(),
        TypedInner::Match(_, arms) => arms
            .iter()
            .map(|arm| assert_nested_none_has_int_slot(&arm.body))
            .sum(),
        TypedInner::Lit(_) | TypedInner::Var(_) => 0,
        other => panic!("nested enum regression must inspect this expression: {other:?}"),
    }
}

#[test]
fn nested_enum_finalization_uses_context_and_preserves_constructor_span() {
    for (with_context, without_context) in [
        (
            "value: (Option<Int>, Int) = (Option::None, 1)",
            "value = (Option::None, 1)",
        ),
        (
            "value: List<Option<Int>> = [Option::None]",
            "value = [Option::None]",
        ),
        (
            "value: (Int -> Option<Int>) = {|x: Int| Option::None}",
            "check_none({|x: Int| Option::None})",
        ),
        (
            "value: Option<Int> = match True { True => Option::None, False => Option::None }",
            "is_some(match True { True => Option::None, False => Option::None })",
        ),
    ] {
        let typed = typecheck(&format!("{DECLARATIONS}\n{with_context}"))
            .unwrap_or_else(|error| panic!("{with_context}: {error:?}"));
        assert_eq!(
            assert_nested_none_has_int_slot(typed.last().unwrap()),
            if with_context.contains("match True") {
                2
            } else {
                1
            },
            "every nested constructor must carry the resolved Int slot"
        );
        let source = format!("{DECLARATIONS}\n{without_context}");
        let error = typecheck(&source).expect_err(without_context);
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::UnresolvedEnumConstructorTypeArgument),
            "{without_context}: {error:?}"
        );
        assert_eq!(&source[error.span.start..error.span.end], "Option::None");
        let diagnostic = error.structured.expect("structured finalization error");
        assert_eq!(
            diagnostic.origin,
            diagnostics::DiagnosticOrigin::EnumConstructor { ordinal: 0 }
        );
        assert_eq!(diagnostic.data_json()["constraint_status"], "Insufficient");
    }
    // Definition-level polymorphism and the unused success slot of Err remain valid.
    for expression in [
        "def generic_nested::<$T>() -> List<Option<$T>> { [Option::None] }",
        "value = Err(NoneError())",
    ] {
        typecheck(&format!("{DECLARATIONS}\n{expression}"))
            .unwrap_or_else(|error| panic!("{expression}: {error:?}"));
    }
}
