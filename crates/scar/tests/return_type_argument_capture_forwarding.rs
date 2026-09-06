use diagnostics::TypeDiagnosticReason;

fn typecheck(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0))
        .expect("capture forwarding source should parse");
    let resolved = sigil::resolve(ast).expect("capture forwarding source should resolve");
    scar::typecheck(resolved)
}

fn assert_generic_capture_forwarding(capture: &str) {
    let source = format!(
        "def make::<$A>() -> List<$A> {{ [] }}\n\
         def outer::<$A>() -> (-> List<$A>) {{ {capture} }}\n\
         factory: (-> List<Int>) = outer::<Int>()"
    );
    typecheck(&source).unwrap_or_else(|error| panic!("{capture} should forward $A: {error}"));
}

#[test]
fn omitted_capture_forwards_outer_return_type_argument() {
    assert_generic_capture_forwarding("&make");
}

#[test]
fn explicit_capture_forwards_outer_return_type_argument() {
    assert_generic_capture_forwarding("&make::<$A>");
}

#[test]
fn underscore_capture_forwards_outer_return_type_argument() {
    assert_generic_capture_forwarding("&make::<_>");
}

#[test]
fn closure_return_shape_infers_return_type_argument_capture() {
    for capture in ["&make", "&make::<_>"] {
        let source = format!(
            "def make::<$A>() -> List<$A> {{ [] }}\n\
             factory: (-> (-> List<Int>)) = {{|| {capture}}}"
        );
        typecheck(&source)
            .unwrap_or_else(|error| panic!("closure must constrain {capture}: {error}"));
    }
}

#[test]
fn unrelated_outer_type_argument_does_not_resolve_capture() {
    for capture in ["&make", "&make::<_>"] {
        let source = format!(
            "def make::<$A>() -> List<$A> {{ [] }}\n\
             def outer(value: $B) -> $B {{\n\
               unresolved = {capture}\n\
               value\n\
             }}"
        );
        let error = typecheck(&source).expect_err("a fresh capture input must remain ambiguous");
        assert_eq!(
            error.reason(),
            Some(TypeDiagnosticReason::AmbiguousReturnTypeArgument)
        );
    }
}
