#[allow(dead_code)]
mod support;

use scar::typed::{SafeBindRhsProjection, TypedInner};
use support::{resolve_with_builtin_prelude, typecheck, typecheck_with_builtin_prelude};

#[test]
fn safe_bind_extracts_standard_carriers_once() {
    for rhs in ["Ok(1)", "Option::Some(1)", "Either<Error, Int>::Right(1)"] {
        let typed = typecheck_with_builtin_prelude(&format!("value: Int =? {rhs}"));
        let (rhs, projection) = typed
            .iter()
            .find_map(|node| match &node.node {
                TypedInner::SafeBind(_, rhs, projection, _) => Some((rhs, projection)),
                _ => None,
            })
            .expect("expected SafeBind");
        assert!(matches!(
            projection,
            SafeBindRhsProjection::CanonicalResultOnce { .. }
        ));
        assert!(
            matches!(&rhs.node, TypedInner::TraitCall { method_name, args, .. } if method_name == "extract" && args.len() == 1),
            "{rhs:?}"
        );
    }
}

#[test]
fn safe_bind_extracts_only_one_layer() {
    typecheck_with_builtin_prelude("value: Option<Int> =? Option::Some(Option::Some(1))");
    let error = typecheck(resolve_with_builtin_prelude(
        "Option::Some(value) =? Option::Some(1)",
    ))
    .expect_err("Pattern must receive the extracted Int");
    assert!(error.message.contains("Int"), "{error:?}");
}

#[test]
fn question_accepts_direct_unit_of_extracting_carriers() {
    for rhs in ["Option::Some(())", "Either<Error, Unit>::Right(())"] {
        typecheck_with_builtin_prelude(&format!("{rhs}?"));
    }
    for rhs in [
        "Option::Some(Ok(()))",
        "Ok(Option::Some(()))",
        "Option::Some(Option::Some(()))",
        "Ok(Option::Some(Either<Error, Unit>::Right(())))",
    ] {
        typecheck_with_builtin_prelude(&format!("{rhs}?"));
    }
}

#[test]
fn question_requires_extraction_at_each_declared_generic_layer() {
    typecheck_with_builtin_prelude(
        "def check(value: $F<$G<Unit>>) -> Result<Unit> where $F: MonadExtractor\n$G: MonadExtractor { value?; Ok(()) }\ncheck(Option::Some(Ok(())))",
    );
    let error = typecheck(resolve_with_builtin_prelude(
        "def check(value: $F<$G<Unit>>) -> Result<Unit> where $F: MonadExtractor\n$G: Monad { value?; Ok(()) }",
    ))
    .expect_err("the outer capability cannot supply the inner extraction bound");
    assert!(error.message.contains("MonadExtractor"), "{error:?}");
}

#[test]
fn safe_bind_requires_declared_extraction_bound() {
    typecheck_with_builtin_prelude("def relay(target: $F<$T>) -> Result<$T> where $F: MonadExtractor { value =? target; Ok(value) }\nresult = relay(Option::Some(1))");
    let error = typecheck(resolve_with_builtin_prelude("def relay(target: $F<$T>) -> Result<$T> where $F: Monad { value =? target; Ok(value) }\nresult = relay(Ok(1))"))
        .expect_err("concretization cannot strengthen a declared bound");
    assert!(error.message.contains("MonadExtractor"), "{error:?}");
}

#[test]
fn safe_bind_keeps_input_and_failure_capabilities_independent() {
    typecheck_with_builtin_prelude(
        "result: Option<Int> = do::<Option> { value =? Option::Some(1); Option::Some(value) }",
    );
    let error = typecheck(resolve_with_builtin_prelude(
        "def reject(target: Option<Int>) -> Option<Int> { value =? target; Option::Some(value) }",
    ))
    .expect_err("Option extraction cannot supply a callable failure target");
    assert!(error.message.contains("MonadFail"), "{error:?}");
}

#[test]
fn safe_bind_extracts_explicit_non_error_left_implementation() {
    typecheck_with_builtin_prelude(
        r#"
impl MonadExtractor for Either<String, $T>
where $T: MonadExtractor.$A
{
  def extract(self: Either<String, $A>) -> Result<$A> {
    match self {
      Either::Left(message) => Err(NoneError()),
      Either::Right(value) => Ok(value),
    }
  }
}
value: Int =? Either<String, Int>::Right(1)
"#,
    );
}

const EQ_IDENTITY_EXTRACTOR: &str = r#"
impl MonadExtractor for Identity<$T>
where $T: MonadExtractor.$A + Eq
{
  def extract(self: Identity<$A>) -> Result<$A> { Ok(Identity::run(self)) }
}
"#;

#[test]
fn conditional_extraction_impl_inherits_its_bound_in_default_methods() {
    for implementation in [
        EQ_IDENTITY_EXTRACTOR.to_owned(),
        format!(
            "impl Eq for Identity<$T> where $T: Eq {{ def eq(self: Self, rhs: Self) -> Boolean {{ Eq::eq(self.value, rhs.value) }} }}\n{}",
            EQ_IDENTITY_EXTRACTOR.replace(
                "$T: MonadExtractor.$A + Eq",
                "$T: MonadExtractor.$A\nSelf: Eq",
            ),
        ),
    ] {
        typecheck_with_builtin_prelude(&format!(
            "{implementation}\nvalue: Int =? Identity::new(1)\nerror = MonadExtractor::extract_error(Identity::new(1))"
        ));
    }
}

#[test]
fn matching_extraction_impl_retains_its_unsatisfied_constraint() {
    let source = format!(
        r#"{EQ_IDENTITY_EXTRACTOR}
whole = *{{|value: Identity<(Int -> Int)>| MatchResult::Ok(())}}
whole() =? Identity::new({{|value: Int| value}})
"#,
    );
    let error = typecheck(resolve_with_builtin_prelude(&source))
        .expect_err("a matching implementation with a failed constraint cannot inspect the whole RHS");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::MissingTypeConstructorCapability),
        "{error:?}"
    );
    let input = "Identity::new({|value: Int| value})";
    let start = source.find(input).expect("rejected input");
    assert_eq!(error.span, spire::ast::Span { start, end: start + input.len() }, "{error:?}");
    let diagnostic = error.structured.as_ref().expect("constructor capability diagnostic");
    let diagnostics::DiagnosticData::TypeConstructorCarrier(data) = &diagnostic.data else {
        panic!("expected constructor capability details: {error:?}");
    };
    assert_eq!(data.actual_carrier, "Identity<(Int -> Int)>");
    assert_eq!(data.required_capability, "MonadExtractor");
}

fn check_standard_program(
    resolved: Vec<sigil::resolved::Resolved>,
) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    scar::typecheck_with_context(
        resolved,
        scar::TypecheckContext {
            runtime_policy: sindr::policy::RuntimeSourcePolicy::std_module(),
            enforce_builtin_type_contracts: true,
            allow_private_facet_inspection: false,
            test_script: false,
        },
    )
}

#[test]
fn malformed_canonical_extraction_contract_is_an_invariant_error() {
    use sigil::resolved::Resolved;
    let mut resolved = support::resolve_program_with_builtin_prelude("value =? Ok(1)");
    for node in &mut resolved {
        if let Resolved::TraitDef(_, id, _, _, methods, _) = node {
            if id.qualified_name.as_deref() == Some("MonadExtractor") {
                methods
                    .iter_mut()
                    .find(|method| method.id.name == "extract")
                    .unwrap()
                    .ret_ty
                    .syntax =
                    spire::ast::AstTy::Named(spire::ast::Span { start: 0, end: 1 }, "Int".into());
            }
        }
    }
    let error = check_standard_program(resolved)
        .expect_err("invalid trusted contracts must be rejected before impl checking");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::TypecheckInvariantViolation),
        "{error:?}"
    );
}

#[test]
fn canonical_result_requires_its_explicit_extraction_implementation() {
    use sigil::resolved::Resolved;
    use spire::ast::AstTy;
    let mut resolved = support::resolve_program_with_builtin_prelude("value: Int =? Ok(1)");
    resolved.retain(|node| {
        !matches!(node,
            Resolved::TraitImplDef(_, _, id, _, AstTy::Generic(_, target, _), ..)
                if id.qualified_name.as_deref() == Some("MonadExtractor")
                    && target.rsplit("::").next() == Some("Result")
        )
    });
    let error = check_standard_program(resolved)
        .expect_err("Result cannot bypass its declared extraction implementation");
    assert!(error.message.contains("MonadExtractor"), "{error:?}");
}

#[test]
fn safe_bind_retains_annotation_inference_before_the_next_statement() {
    typecheck_with_builtin_prelude(
        r#"
def unknown::<$A>() -> Result<$A> { Err(NoneError()) }
def relay() -> Result<Int> {
  value: Int =? unknown()
  ()
  Ok(value)
}
result = relay()
"#,
    );
}

#[test]
fn extraction_uses_canonical_monad_when_another_trait_has_the_same_short_name() {
    fn with_shadow(source: &str) -> Vec<sigil::resolved::Resolved> {
        let mut resolved = resolve_with_builtin_prelude(&format!(
            "deftrait ShadowMonad {{ def mark(self: Self) -> Self }}\n{source}"
        ));
        // Sigil's flat owner namespace rejects this source spelling. Inject
        // distinct resolved identities to exercise Scar's identity contract.
        for node in &mut resolved {
            if let sigil::resolved::Resolved::TraitDef(_, id, _, _, methods, _) = node {
                if id.name == "ShadowMonad" {
                    id.name = "Monad".into();
                    id.qualified_name = Some("Shadow::Monad".into());
                    for method in methods {
                        method.id.qualified_name =
                            Some(format!("Shadow::Monad::{}", method.id.name));
                    }
                }
            }
        }
        resolved
    }
    typecheck(with_shadow("Option::Some(())?"))
        .expect("the standard Monad identity must remain available");
    let error = typecheck(with_shadow("value =? Identity::new(1)"))
        .expect_err("Identity must keep its ordinary missing extraction diagnostic");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::SafeBindMissingMonadExtractor),
        "{error:?}"
    );
}

#[test]
fn known_non_extracting_list_head_keeps_partial_patterns_with_unknown_elements() {
    for source in [
        "def relay() -> Result<Int> { [[x]] =? [[]]; Ok(0) }\nrelay()",
        "def relay() -> Result<Int> { [head, ..tail] =? []; Ok(0) }\nrelay()",
        "result: Result<Int> = do::<Result> { [[x]] =? [[]]; Ok(0) }",
    ] {
        typecheck_with_builtin_prelude(source);
    }
}

#[test]
fn unknown_rhs_head_cannot_gain_a_non_extracting_success_path() {
    typecheck(resolve_with_builtin_prelude(
        "def reject(target: $T) -> Result<Int> { 1 =? target; Ok(0) }",
    ))
    .expect_err("an unknown RHS head cannot be accepted by switching to whole-value matching");
}

#[test]
fn question_rejects_non_extracting_and_non_unit_inner_layers() {
    for rhs in ["Ok(Identity::new(()))", "Option::Some([()])"] {
        let error = typecheck(resolve_with_builtin_prelude(&format!("{rhs}?")))
            .expect_err("every inner constructor must implement extraction");
        assert!(error.message.contains("MonadExtractor"), "{error:?}");
    }
    for rhs in ["Option::Some(Ok(1))", "Ok(Option::Some(1))"] {
        let error = typecheck(resolve_with_builtin_prelude(&format!("{rhs}?")))
            .expect_err("the final payload must be Unit");
        assert!(
            error.message.contains("terminal success type Unit"),
            "{error:?}"
        );
    }
}
