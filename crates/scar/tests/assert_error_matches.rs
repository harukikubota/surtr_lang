#[allow(dead_code)]
mod support;

use scar::{ScarCheckpoint, ScarSession, TypecheckContext};
use sindr::policy::RuntimeSourcePolicy;
use std::sync::OnceLock;

struct TestPrelude {
    stages: Vec<Vec<sigil::StagedModuleAst>>,
    index: sigil::DeclarationIndex,
    resolve: sigil::ResolveResumeState,
    checkpoint: ScarCheckpoint,
}

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    static PRELUDE: OnceLock<TestPrelude> = OnceLock::new();
    let prelude = PRELUDE.get_or_init(|| {
        let mut stages = support::std_module_stages();
        let source = include_str!("../../../lib/test.srt");
        let ast = spire::parse_with_context(
            source,
            spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
        )
        .expect("Test declarations parse");
        stages[1].extend(sigil::staged_modules_from_source_ast(ast, None));
        let index =
            sigil::precollect_declaration_index(&stages).expect("Test declarations collect");
        let resolved = sigil::resolve_staged_program_with_state(&stages, vec![], &index, None)
            .expect("Test declarations resolve");
        let mut session = ScarSession::new();
        session
            .typecheck_staged_program_in_place_with_context(
                resolved.clone(),
                TypecheckContext {
                    runtime_policy: RuntimeSourcePolicy::std_module(),
                    enforce_builtin_type_contracts: true,
                    allow_private_facet_inspection: false,
                    test_script: false,
                },
            )
            .expect("Test declarations typecheck");
        TestPrelude {
            stages,
            index,
            resolve: resolved.resume_state,
            checkpoint: session.checkpoint(),
        }
    });
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).expect(source);
    let resolved = sigil::resolve_staged_program_from_state(
        &prelude.stages,
        ast,
        &prelude.index,
        None,
        prelude.stages.len(),
        prelude.resolve.clone(),
    )
    .expect(source);
    let mut session = ScarSession::new();
    session.rollback(prelude.checkpoint.clone());
    session
        .typecheck_staged_program_in_place_with_context(
            resolved,
            TypecheckContext {
                runtime_policy: RuntimeSourcePolicy::script(),
                enforce_builtin_type_contracts: false,
                allow_private_facet_inspection: false,
                test_script: true,
            },
        )
        .map(|program| program.nodes)
}

const ERROR: &str = r#"
import Test;
deferror Held(value: Int) { |value: Int| Self(message: "held", value) }
"#;

#[test]
fn assertion_requires_error_input_and_result_unit_check() {
    for (body, reason) in [
        (
            "value = assert_error_matches(Ok(Held(1)), Held, Ok(()))",
            "ArgumentTypeMismatch",
        ),
        (
            "value = assert_error_matches(Held(1), Held, ())",
            "ArgumentTypeMismatch",
        ),
        (
            "value = assert_error_matches(Held(1), Held, {|| 1})",
            "ArgumentTypeMismatch",
        ),
        (
            "value = assert_error_matches(Held(1), Held, {|x: Int| Ok(())})",
            "ArgumentTypeMismatch",
        ),
    ] {
        let source = format!("{ERROR}\n{body}");
        let error = check(&source).expect_err(&source);
        assert_eq!(
            error.reason().map(|reason| reason.as_str()),
            Some(reason),
            "{source}\n{error:?}"
        );
    }
}

#[test]
fn assertion_binding_scope_accepts_direct_checks_and_rejects_external_thunks() {
    for body in [
        "value = assert_error_matches(Held(1), Held(n), assert_eq(1, n))",
        "value = assert_error_matches(Held(1), Held(n), {|| assert_eq(1, n)})",
        "value = assert_error_matches(Held(1), Held(n), { assert_eq(1, n)?; assert_eq(n, 1) })",
        "value = assert_error_matches(Held(1), Held(n)) { assert_eq(1, n)?; assert_eq(n, 1) }",
        "thunk = {|| Ok(())}\nvalue = assert_error_matches(Held(1), Held, thunk)",
        "value = assert_error_matches(Held(1), Held(n) @ e) { read = {|| e.value}; assert_eq(n, read()) }",
    ] {
        let source = format!("{ERROR}\n{body}");
        check(&source).unwrap_or_else(|error| panic!("{source}\n{error:?}"));
    }
    for body in [
        "thunk = {|| Ok(())}\nvalue = assert_error_matches(Held(1), Held(n), thunk)",
        "thunk = {|| Ok(())}\nvalue = assert_error_matches(Held(1), Held(n), (thunk))",
        "def done() -> Result<()> { Ok(()) }\nvalue = assert_error_matches(Held(1), Held(n), &done)",
        "choose = &assert_error_matches(&1, Held(n), &2)\nvalue = choose(Held(1), {|| Ok(())})",
    ] {
        let source = format!("{ERROR}\n{body}");
        let error = check(&source).expect_err(&source);
        assert!(error.message.contains("DirectExpression"), "{source}\n{error:?}");
    }
}

#[test]
fn assertion_preserves_concrete_error_information_boundaries() {
    for body in [
        "value = assert_error_matches(Held(1), e, assert_eq(1, e.value))",
        "def identity(e: Held) -> Error { e }\nvalue = assert_error_matches(Held(1), Held @ e, assert_err(Err(identity(e))))",
        "value = assert_error_matches(Held(1), Held @ e, Ok(e))",
        "error =? Ok(Held(1))\nvalue = error.value",
        "def generic(thunk: $T) -> Result<()> { assert_error_matches(Held(1), Held(n), thunk) }\nvalue = generic({|| Ok(())})",
    ] {
        let source = format!("{ERROR}\n{body}");
        check(&source).expect_err(&source);
    }
}

#[test]
fn assertion_rejects_projection_outside_apply_pattern() {
    let source = format!("{ERROR}\nvalue = assert_error_matches(Held(1), Held(_1), Ok(()))");
    let error = check(&source).expect_err(&source);
    assert!(
        error.message.contains("apply_pattern"),
        "{source}\n{error:?}"
    );
}
