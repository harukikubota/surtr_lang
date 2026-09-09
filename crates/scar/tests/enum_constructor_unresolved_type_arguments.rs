use scar::typed::TypedNode;

fn typecheck(source: &str) -> Result<Vec<TypedNode>, scar::error::TypeError> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("source should parse");
    let resolved = sigil::resolve(ast).expect("source should resolve");
    scar::typecheck(resolved)
}

const DECLARATIONS: &str = r#"
defenum Option<$T> { Some($T), None }
defenum Either<$L, $R> { Left($L), Right($R) }
def is_some(value: Option<$T>) -> Int { 1 }
def is_left(value: Either<$L, $R>) -> Int { 1 }
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
    let error = typecheck(&source).expect_err("closure input must not resolve a callable RTA");
    assert_eq!(
        error.structured.expect("structured RTA diagnostic").reason,
        diagnostics::TypeDiagnosticReason::AmbiguousReturnTypeArgument
    );
}
