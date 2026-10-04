#[allow(dead_code)]
mod support;

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).expect("parse");
    support::typecheck(support::resolve_ast_with_builtin_prelude(ast).expect("resolve"))
}

#[test]
fn compiler_managed_targets_reject_user_trait_impls() {
    for target in [
        "Error",
        "Facet<InfallibleStructural, Int, Int, _, _>",
        "Regex",
        "FileHandle",
    ] {
        let source = format!(
            "deftrait Marker {{ def mark(self: Self) -> Int }}\nimpl Marker for {target} {{ def mark(self: Self) -> Int {{ 1 }} }}"
        );
        let error = check(&source).expect_err(target);
        assert_eq!(
            error.reason(),
            Some(diagnostics::TypeDiagnosticReason::TraitImplementationForbidden),
            "{target}: {error}"
        );
        assert!(
            error.message.contains("trait implementation") || error.message.contains("trait impl"),
            "{target}: {error}"
        );
    }
}

#[test]
fn function_target_rejects_user_trait_impl() {
    let error = check(
        "deftrait Marker { def mark(self: Self) -> Int }\nimpl Marker for (Int -> Int) { def mark(self: Self) -> Int { 1 } }",
    )
    .expect_err("function target must be closed");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::TraitImplementationForbidden)
    );
    assert!(error.message.contains("trait impl"), "{error}");
}

#[test]
fn no_implicit_show_for_user_struct() {
    let error = check(
        "defstruct Box { value: Int }\nimpl Box { def new(value: Int) -> Box { Box { value: value } } }\nShow::to_string(Box::new(1))",
    )
    .expect_err("Show requires explicit impl");
    assert!(error.message.contains("Show"), "{error}");
}

#[test]
fn derive_show_requires_field_show_capability() {
    let source = "@derive Show\ndefstruct Holder { callback: (Int -> Int) }\nimpl Holder { def new(callback: (Int -> Int)) -> Holder { Holder { callback: callback } } }";
    let error = check(source).expect_err("function field has no Show capability");
    assert!(error.message.contains("Show"), "{error}");
    let field_type_start = source.find("callback: (Int -> Int)").unwrap() + "callback: ".len();
    assert_eq!(error.span.start, field_type_start, "{error}");
    let structured = error.structured.expect("derive failure must be structured");
    match structured.data {
        diagnostics::DiagnosticData::TraitDispatch(dispatch) => {
            let dependency = dispatch.dependency.expect("derive dependency path");
            assert_eq!(dependency.root_type, "Holder");
            assert_eq!(dependency.steps[0].name.as_deref(), Some("callback"));
            assert_eq!(dependency.leaf_type, "(Int -> Int)");
            assert_eq!(
                dependency.leaf_policy,
                diagnostics::TraitDependencyLeafPolicy::TraitImplementationForbidden
            );
        }
        other => panic!("expected trait dispatch data, got {other:?}"),
    }
}

#[test]
fn generic_derive_show_rejects_a_function_type_argument() {
    let error = check(
        "@derive Show\ndefstruct Box<$T> { value: $T }\nimpl Box { def new(value: $T) -> Box<$T> { Box { value: value } } }\ndef render(f: (Int -> Int)) -> String { Show::to_string(Box::new(f)) }",
    )
    .expect_err("generic Show bound must be checked at the call");
    assert!(error.message.contains("Show"), "{error}");
}

#[test]
fn generic_derive_show_requires_the_whole_field_type() {
    check(
        "defstruct Wrapper<$T> { value: $T }\nimpl Wrapper { def new(value: $T) -> Wrapper<$T> { Wrapper { value: value } } }\nimpl Show for Wrapper<$T> { def to_string(self: Self) -> String { \"wrapped\" } }\n@derive Show\ndefstruct Outer<$T> { field: Wrapper<$T> }\nimpl Outer { def new(field: Wrapper<$T>) -> Outer<$T> { Outer { field: field } } }\ndef render(f: (Int -> Int)) -> String { Show::to_string(Outer::new(Wrapper::new(f))) }",
    )
    .expect("the field's Show impl must satisfy derive without a Show bound on its argument");
}

#[test]
fn nested_generic_derive_show_preserves_inner_requirement() {
    let error = check(
        "@derive Show\ndefstruct Inner<$T> { value: $T }\nimpl Inner { def new(value: $T) -> Inner<$T> { Inner { value: value } } }\n@derive Show\ndefstruct Outer<$T> { field: Inner<$T> }\nimpl Outer { def new(field: Inner<$T>) -> Outer<$T> { Outer { field: field } } }\ndef render(f: (Int -> Int)) -> String { Show::to_string(Outer::new(Inner::new(f))) }",
    )
    .expect_err("the inner derived Show impl still requires Show for its field");
    assert!(error.message.contains("Show"), "{error}");
}

#[test]
fn user_impl_cannot_declare_a_composite_where_subject() {
    let error = check(
        "defstruct Wrapper<$T> { value: $T }\nimpl Show for Wrapper<$T> where Wrapper<$T>: Show { def to_string(self: Self) -> String { \"wrapped\" } }",
    )
    .expect_err("composite where subjects are reserved for generated derive impls");
    assert!(
        error.message.contains("where constraint subjects must be"),
        "{error}"
    );
}

#[test]
fn derive_show_does_not_bound_unused_type_parameters() {
    check(
        "@derive Show\ndefstruct Phantom<$Tag> {}\nimpl Phantom { def new::<$Tag>() -> Phantom<$Tag> { Phantom {} } }\nShow::to_string(Phantom::new::<(Int -> Int)>())",
    )
    .expect("unused type parameter must not need Show");
}

#[test]
fn standard_eq_cannot_be_redeclared_in_the_same_owner() {
    let error = support::resolve_with_builtin_prelude_result(
        "deftrait Eq { def eq(self: Self, rhs: Self) -> Boolean }\ndefenum Choice { One, Two }\nEq::eq(Choice::One, Choice::One)",
    ).expect_err("the standard Eq declaration cannot be replaced by a fixture declaration");
    assert!(
        error.message.contains("Duplicate top-level owner: Eq"),
        "{error:?}"
    );
}

#[test]
fn derive_eq_reports_the_rejected_enum_payload_type() {
    let source = "@derive Eq\ndefenum Callback { Fn((Int -> Int)), Empty }";
    let error = check(source).expect_err("function payload cannot satisfy Eq");
    assert!(error.message.contains("Eq"), "{error}");
    assert_eq!(
        error.span.start,
        source.find("(Int -> Int)").unwrap(),
        "{error}"
    );
    let structured = error.structured.expect("derive failure must be structured");
    match structured.data {
        diagnostics::DiagnosticData::TraitDispatch(dispatch) => {
            let dependency = dispatch.dependency.expect("enum payload dependency path");
            assert_eq!(dependency.root_type, "Callback");
            assert_eq!(dependency.steps[0].name.as_deref(), Some("Fn"));
            assert_eq!(dependency.steps[0].ordinal, Some(0));
            assert_eq!(dependency.leaf_type, "(Int -> Int)");
        }
        other => panic!("expected trait dispatch data, got {other:?}"),
    }
}

#[test]
fn derive_show_stops_at_container_without_show_impl() {
    let source = "@derive Show\ndefstruct Holder { callbacks: List<(Int -> Int)> }\nimpl Holder { def new(callbacks: List<(Int -> Int)>) -> Holder { Holder { callbacks: callbacks } } }";
    let error = check(source).expect_err("List has no Show implementation");
    assert!(error.structured.is_some(), "{error:?}");
    let structured = error.structured.expect("derive failure must be structured");
    match structured.data {
        diagnostics::DiagnosticData::TraitDispatch(dispatch) => {
            let dependency = dispatch.dependency.expect("derive dependency path");
            assert_eq!(dependency.steps.len(), 1);
            assert_eq!(dependency.leaf_type, "List<(Int -> Int)>");
            assert_eq!(
                dependency.leaf_policy,
                diagnostics::TraitDependencyLeafPolicy::TraitImplementationMissing
            );
        }
        other => panic!("expected trait dispatch data, got {other:?}"),
    }
}
