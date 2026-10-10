#[allow(dead_code)]
mod support;

use support::{resolve_with_builtin_prelude, typecheck, typecheck_with_builtin_prelude};

#[test]
fn safe_bind_uses_either_error_failure_capability() {
    typecheck_with_builtin_prelude(
        "def relay(value: Result<Int>) -> Either<Error, Int> { x =? value\n Either::Right(x) }",
    );
}

#[test]
fn failure_capability_is_provided_by_declared_generic_bound() {
    typecheck_with_builtin_prelude(
        r#"def relay(value: $M<Int>, input: Result<Int>) -> $M<Int> where $M: MonadFail {
  x =? input
  value
}
result: Either<Error, Int> = relay(Either<Error, Int>::Right(1), Ok(2))
"#,
    );
    let error = typecheck(resolve_with_builtin_prelude(
        r#"def relay(value: $M<Int>, input: Result<Int>) -> $M<Int> where $M: Monad {
  x =? input
  value
}
result: Result<Int> = relay(Ok(1), Ok(2))
"#,
    ))
    .expect_err("concretization must not strengthen a Monad bound");
    assert!(error.message.contains("MonadFail"), "{error:?}");
}

#[test]
fn do_uses_declared_monad_fail_bound_for_partial_pattern() {
    typecheck_with_builtin_prelude(
        r#"def relay(value: $M<Option<Int>>) -> $M<Int> where $M: MonadFail {
  do { Option::Some(x) <- value; Monad::return(x) }
}
result: Result<Int> = relay(Ok(Option::Some(1)))
"#,
    );
}

#[test]
fn ordinary_callable_cannot_borrow_alternative_or_outer_target() {
    for source in [
        "def bad(value: Result<Int>) -> Option<Int> { x =? value\n Option::Some(x) }",
        "def bad(value: Result<Int>) -> List<Int> { x =? value\n [x] }",
        "def outer() -> Result<Int> { inner = {|x: Int| value =? Ok(x)\n value}\n Ok(1) }",
    ] {
        let error = typecheck(resolve_with_builtin_prelude(source))
            .expect_err("the nearest callable needs its own MonadFail return type");
        assert!(error.message.contains("MonadFail"), "{source}: {error:?}");
    }
}

#[test]
fn safe_bind_does_not_unwrap_either_or_transformers() {
    for source in [
        "value =? Either<Error, Int>::Right(1)",
        "value =? ResultT::ok::<Identity>(1)",
        "value =? OptionT::some::<Identity>(1)",
    ] {
        let error = typecheck(resolve_with_builtin_prelude(source))
            .expect_err("total non-Result SafeBind remains invalid");
        assert!(
            error.message.contains("Only a Result RHS"),
            "{source}: {error:?}"
        );
    }
    typecheck_with_builtin_prelude(
        "def relay(value: Result<Result<Int>>) -> Result<Result<Int>> { inner: Result<Int> =? value\n Ok(inner) }",
    );
    typecheck_with_builtin_prelude(
        "value: Result<Result<Int>> = do::<Result> { inner: Result<Int> <- Ok(Ok(1)); Ok(inner) }",
    );
}

#[test]
fn unresolved_failure_dispatch_is_rejected_before_codegen() {
    let source = r#"def relay(value: Result<$A>) -> Result<$A> {
  item =? value
  Ok(item)
}
inspect(relay(Err(NoneError)))"#;
    let error = typecheck(resolve_with_builtin_prelude(source))
        .expect_err("an unresolved failure dispatch must not leak a generic function index");
    assert!(
        error.message.contains("concrete method instantiation"),
        "{error:?}"
    );
    typecheck_with_builtin_prelude(&source.replace(
        "inspect(relay(Err(NoneError)))",
        "value: Result<Int> = relay(Err(NoneError))",
    ));
}

fn check_full_program(
    resolved: Vec<sigil::resolved::Resolved>,
) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    scar::typecheck_with_context(
        resolved,
        scar::TypecheckContext {
            runtime_policy: sindr::policy::RuntimeSourcePolicy::std_module(),
            enforce_builtin_type_contracts: true,
            allow_private_facet_inspection: false,
        },
    )
}

#[test]
fn missing_canonical_monad_fail_declaration_is_a_contract_error() {
    use sigil::resolved::Resolved;
    for source in [
        "def relay(value: Result<Int>) -> Result<Int> { x =? value\n Ok(x) }",
        "value: Option<Int> = do::<Option> { Option::Some(x) <- Option::Some(Option::Some(1)); Option::Some(x) }",
    ] {
        let mut resolved = support::resolve_program_with_builtin_prelude(source);
        resolved.retain(|node| !matches!(node,
            Resolved::TraitDef(_, id, ..) | Resolved::TraitImplDef(_, _, id, ..)
                if id.qualified_name.as_deref() == Some("MonadFail")
        ));
        let error = check_full_program(resolved).expect_err("missing declaration cannot switch to Alternative");
        assert_eq!(error.reason(), Some(diagnostics::TypeDiagnosticReason::TypecheckInvariantViolation), "{error:?}");
    }
}

#[test]
fn nonstandard_same_name_declaration_cannot_supply_canonical_failure_capability() {
    use sigil::resolved::Resolved;
    let mut resolved = support::resolve_program_with_builtin_prelude(
        "def relay(value: Result<Int>) -> Result<Int> { x =? value\n Ok(x) }",
    );
    let mut impostor = resolved
        .iter()
        .find(|node| {
            matches!(node,
                Resolved::TraitDef(_, id, ..) if id.qualified_name.as_deref() == Some("MonadFail")
            )
        })
        .expect("standard declaration")
        .clone();
    if let Resolved::TraitDef(_, id, _, _, methods, _) = &mut impostor {
        id.qualified_name = Some("Impostor::MonadFail".into());
        id.unique_id = u32::MAX - 10;
        methods.clear();
    }
    resolved.retain(|node| {
        !matches!(node,
            Resolved::TraitDef(_, id, ..) | Resolved::TraitImplDef(_, _, id, ..)
                if id.qualified_name.as_deref() == Some("MonadFail")
        )
    });
    resolved.insert(0, impostor);
    let error = check_full_program(resolved)
        .expect_err("a same-name Trait cannot replace canonical MonadFail");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::TypecheckInvariantViolation),
        "{error:?}"
    );
}

#[test]
fn result_failure_requires_its_monad_fail_impl() {
    use sigil::resolved::Resolved;
    use spire::ast::AstTy;
    let mut resolved = support::resolve_program_with_builtin_prelude(
        "def relay(value: Result<Int>) -> Result<Int> { x =? value\n Ok(x) }",
    );
    resolved.retain(|node| !matches!(node,
        Resolved::TraitImplDef(_, _, id, _, AstTy::Generic(_, target, _), ..)
            if id.qualified_name.as_deref() == Some("MonadFail") && target.rsplit("::").next() == Some("Result")
    ));
    let error = check_full_program(resolved)
        .err()
        .expect("Result spelling must not bypass its impl requirement");
    assert!(error.message.contains("MonadFail"), "{error:?}");
    assert_ne!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::TypecheckInvariantViolation)
    );
}
