//! Session and standard-prefix contracts used by the Scar refactoring plan.
use super::*;
use diagnostics::TypeDiagnosticReason as Reason;
use sigil::{DeclarationIndex, ResolvedStagedProgram, StagedModuleAst};

struct StandardPrefix {
    stages: Vec<Vec<StagedModuleAst>>,
    declaration_index: DeclarationIndex,
    resolved: ResolvedStagedProgram,
    checkpoint: ScarCheckpoint,
}

fn standard_context() -> TypecheckContext {
    TypecheckContext {
        runtime_policy: RuntimeSourcePolicy::std_module(),
        enforce_builtin_type_contracts: true,
        allow_private_facet_inspection: false,
        test_script: false,
    }
}

// Deliberately compile the real standard inventory here. In particular, T-06
// must not accidentally compare two sessions restored from the disk cache.
fn standard_prefix() -> &'static StandardPrefix {
    static PREFIX: OnceLock<StandardPrefix> = OnceLock::new();
    PREFIX.get_or_init(|| {
        use sindr::stdlib::{stdlib_module_specs, StdlibStage, StdlibVariant};
        let mut stages = vec![Vec::new(), Vec::new()];
        for spec in stdlib_module_specs(StdlibVariant::Default) {
            let ast = spire::parse_with_context(
                spec.source,
                spire::ParserContext::module(
                    0,
                    spec.module_path
                        .filter(|path| *path == "Facet")
                        .map(str::to_owned),
                )
                .with_rules(spire::ParseRules::std_module()),
            )
            .expect("standard module should parse");
            let fallback = sigil::const_only_fallback_module_path(&ast, spec.module_path);
            let stage = match spec.stage {
                StdlibStage::Bootstrap => 0,
                StdlibStage::Main | StdlibStage::TestExtension => 1,
            };
            stages[stage].extend(sigil::staged_modules_from_source_ast(ast, fallback));
        }
        let declaration_index = sigil::precollect_declaration_index(&stages).unwrap();
        let resolved =
            sigil::resolve_staged_program_with_state(&stages, Vec::new(), &declaration_index, None)
                .unwrap();
        let mut session = ScarSession::new();
        session
            .typecheck_staged_program_with_context(resolved.clone(), standard_context())
            .unwrap();
        StandardPrefix {
            stages,
            declaration_index,
            resolved,
            checkpoint: session.checkpoint(),
        }
    })
}

fn prefix_session() -> ScarSession {
    let mut session = ScarSession::new();
    session.rollback(standard_prefix().checkpoint.clone());
    session
}

fn resolved_suffix(source: &str, process_source: Option<&str>) -> ResolvedStagedProgram {
    resolve_program(source, process_source, true)
}

fn resolve_program(
    source: &str,
    process_source: Option<&str>,
    resume: bool,
) -> ResolvedStagedProgram {
    let prefix = standard_prefix();
    let mut stages = prefix.stages.clone();
    if let Some(source) = process_source {
        let ast = spire::parse_with_context(source, spire::ParserContext::module(0, None)).unwrap();
        stages.push(sigil::staged_modules_from_source_ast(ast, None));
    }
    let declarations = if process_source.is_some() {
        sigil::precollect_declaration_index(&stages).unwrap()
    } else {
        prefix.declaration_index.clone()
    };
    let ast = spire::parse_with_context(source, spire::ParserContext::project(0)).unwrap();
    if !resume {
        return sigil::resolve_staged_program_with_state(&stages, ast, &declarations, None)
            .unwrap();
    }
    let mut suffix = sigil::resolve_staged_program_from_state(
        &stages,
        ast,
        &declarations,
        None,
        prefix.stages.len(),
        prefix.resolved.resume_state,
    )
    .unwrap();
    let mut boot = prefix.resolved.boot_plan.clone();
    boot.entries.append(&mut suffix.boot_plan.entries);
    boot.singletons.append(&mut suffix.boot_plan.singletons);
    boot.supervisors.append(&mut suffix.boot_plan.supervisors);
    suffix.boot_plan = boot;
    suffix
}

#[derive(Clone, Copy, Debug)]
enum SessionApi {
    Transactional,
    InPlace,
}

impl SessionApi {
    fn check(
        self,
        session: &mut ScarSession,
        resolved: Vec<Resolved>,
    ) -> Result<Vec<TypedNode>, TypeError> {
        match self {
            Self::Transactional => session.typecheck(resolved),
            Self::InPlace => {
                session.typecheck_in_place_with_context(resolved, TypecheckContext::default())
            }
        }
    }

    fn check_staged(
        self,
        session: &mut ScarSession,
        program: ResolvedStagedProgram,
    ) -> Result<TypedProgram, TypeError> {
        match self {
            Self::Transactional => session.typecheck_staged_program(program),
            Self::InPlace => session.typecheck_staged_program_in_place_with_context(
                program,
                TypecheckContext::default(),
            ),
        }
    }
}

#[test]
fn failed_suffix_preserves_transactional_state_and_explicit_rollback_allows_retry() {
    let good = "defenum SessionBox<$T> { Box($T), }\ndeferror SessionError(detail: Int) { |value: Int| Self(message: \"session\", detail: value) }\ndef identity(value: $T) -> $T { value }\nSessionError(7);\nidentity(1);\nidentity(\"ok\")";
    let good_resolved = resolved_suffix(good, None).into_resolved();
    let bad = format!("{good};\nbad: Int = \"wrong\"");
    let bad_resolved = resolved_suffix(&bad, None).into_resolved();
    for api in [SessionApi::Transactional, SessionApi::InPlace] {
        let mut failed = prefix_session();
        let checkpoint = failed.checkpoint();
        let before = bincode::serialize(&checkpoint).unwrap();
        let error = api.check(&mut failed, bad_resolved.clone()).unwrap_err();
        assert_eq!(
            error.reason(),
            Some(Reason::AnnotationTypeMismatch),
            "{api:?}: {error:?}"
        );
        if matches!(api, SessionApi::Transactional) {
            assert_eq!(bincode::serialize(&failed.checkpoint()).unwrap(), before);
            assert!(failed.lookup_type_def("SessionBox").is_none());
        }
        // A sibling shares the same retained prefix, including callable signatures.
        let mut sibling = prefix_session();
        let clean = api.check(&mut sibling, good_resolved.clone()).unwrap();
        let result_types = clean
            .iter()
            .rev()
            .take(2)
            .map(|node| match &node.node {
                TypedInner::Semi(value) => value.ty.clone(),
                _ => node.ty.clone(),
            })
            .collect::<Vec<_>>();
        assert_eq!(result_types, vec![Ty::Str, Ty::Int]);
        failed.rollback(checkpoint);
        let retry = api.check(&mut failed, good_resolved.clone()).unwrap();
        assert_eq!(
            retry, clean,
            "{api:?}: retry must match the independent sibling"
        );
    }
}

const COUNTER: &str = r#"defagent SessionCounter {
  meta { instance: Singleton init_policy: Eager state: Int }
  @init
  def init() -> Result<Int> { Ok(0) }
  @get
  def get(state: Int, _field: String) -> Result<Int> { Ok(state) }
  @set
  def set(_state: Int, next: Int) -> Result<Int> { Ok(next) }
}"#;

const SUCCESS: &str = r#"supervisor_init { SessionCounter {} }
defenum SessionOption<$T> { Some($T), None, }
defstruct SessionRecord { value: Int }
impl SessionRecord { def new(value: Int) -> SessionRecord { SessionRecord { value: value } } }
def session_identity(value: $T) -> $T { value }
session_identity(1);
session_identity("ok");
SessionOption::Some(1);
SessionRecord::new(1)
"#;

#[test]
fn transactional_and_in_place_staged_outputs_and_structured_failures_match() {
    let program = resolved_suffix(SUCCESS, Some(COUNTER));
    let expected = SessionApi::Transactional
        .check_staged(&mut prefix_session(), program.clone())
        .unwrap();
    let actual = SessionApi::InPlace
        .check_staged(&mut prefix_session(), program)
        .unwrap();
    // Compare complete user nodes and templates. Generated process wrappers
    // contain globally fresh failure-binding IDs, so compare their signatures
    // separately while retaining exact checks on their authored handlers.
    assert_eq!(actual.process_specs, expected.process_specs);
    assert_eq!(actual.boot_plan.entries.len(), 1);
    assert_eq!(actual.boot_plan.entries[0].process_name, "SessionCounter");
    assert_eq!(actual.boot_plan, expected.boot_plan);
    assert_eq!(actual.nominal_definitions, expected.nominal_definitions);
    assert_eq!(
        actual.enum_definitions.len(),
        expected.enum_definitions.len()
    );
    for (name, variants) in &actual.enum_definitions {
        assert_eq!(variants, &expected.enum_definitions[name], "enum {name}");
    }
    assert_eq!(actual.nodes.len(), expected.nodes.len());
    for (index, (actual, expected)) in actual.nodes.iter().zip(&expected.nodes).enumerate() {
        match (&actual.node, &expected.node) {
            (
                TypedInner::Def(a_idx, a_id, a_types, a_params, a_ret, a_where, a_body, a_vis),
                TypedInner::Def(e_idx, e_id, e_types, e_params, e_ret, e_where, e_body, e_vis),
            ) if a_id
                .qualified_name
                .as_deref()
                .is_some_and(|name| name.starts_with("Global::SessionCounter::"))
                && !a_id.name.starts_with("__agent_") =>
            {
                assert_eq!(
                    (a_idx, a_id, a_types, a_params, a_ret, a_where, a_vis),
                    (e_idx, e_id, e_types, e_params, e_ret, e_where, e_vis)
                );
                assert_eq!(a_body.ty, e_body.ty);
                let (mut actual_calls, mut expected_calls) = (Vec::new(), Vec::new());
                wrapper_calls(a_body, &mut actual_calls);
                wrapper_calls(e_body, &mut expected_calls);
                assert!(!actual_calls.is_empty());
                assert_eq!(
                    actual_calls, expected_calls,
                    "process wrapper {}",
                    a_id.name
                );
            }
            _ => assert_eq!(actual, expected, "typed node {index}"),
        }
    }
    assert!(actual
        .enum_definitions
        .contains_key("Global::SessionOption"));
    assert!(actual
        .nominal_definitions
        .contains_key("Global::SessionRecord"));
    assert_eq!(
        actual
            .process_specs
            .iter()
            .filter(|spec| spec.process_name.ends_with("SessionCounter"))
            .count(),
        1
    );

    for (source, reason, offending) in [
        (
            "value: Int = \"x\"",
            Reason::AnnotationTypeMismatch,
            "\"x\"",
        ),
        (
            "def take(value: Int) -> Int { value }\ntake(\"x\")",
            Reason::ArgumentTypeMismatch,
            "\"x\"",
        ),
        ("if(True, 1, \"x\")", Reason::IfBranchTypeMismatch, "\"x\""),
    ] {
        let program = resolved_suffix(source, Some(COUNTER));
        let expected = SessionApi::Transactional
            .check_staged(&mut prefix_session(), program.clone())
            .unwrap_err();
        let actual = SessionApi::InPlace
            .check_staged(&mut prefix_session(), program)
            .unwrap_err();
        assert_eq!(actual.reason(), Some(reason));
        assert_eq!(actual.structured, expected.structured, "{source}");
        assert!(actual.structured.is_some(), "{source}");
        assert_eq!(actual.span, expected.span);
        assert_eq!(&source[actual.span.start..actual.span.end], offending);
    }
}

// Generated wrappers use fresh local failure IDs. Their callable types and
// dispatch identities still have to agree, including the SafeBind failure arm.
type WrapperCalls = Vec<(Ty, Option<Ty>, Option<TraitDispatch>, Vec<Ty>)>;

fn wrapper_calls(node: &TypedNode, calls: &mut WrapperCalls) {
    match &node.node {
        TypedInner::Block(nodes) => {
            for node in nodes {
                wrapper_calls(node, calls);
            }
        }
        TypedInner::Bind(_, rhs) => wrapper_calls(rhs, calls),
        TypedInner::SafeBind(_, rhs, _, failure) => {
            wrapper_calls(rhs, calls);
            match failure {
                SafeBindFailureTarget::EnclosingMonadFail(target) => {
                    wrapper_calls(&target.call, calls)
                }
                other => panic!("unexpected process wrapper failure target: {other:?}"),
            }
        }
        TypedInner::App(function, args) => {
            calls.push((
                node.ty.clone(),
                Some(function.ty.clone()),
                None,
                args.iter().map(|arg| arg.ty.clone()).collect(),
            ));
            wrapper_calls(function, calls);
            for arg in args {
                wrapper_calls(arg, calls);
            }
        }
        TypedInner::TraitCall {
            receiver_ty,
            dispatch,
            args,
            ..
        } => {
            assert!(matches!(dispatch, TraitDispatch::Static(_)));
            calls.push((
                node.ty.clone(),
                Some(receiver_ty.clone()),
                Some(dispatch.clone()),
                args.iter().map(|arg| arg.ty.clone()).collect(),
            ));
            for arg in args {
                wrapper_calls(arg, calls);
            }
        }
        TypedInner::Capture(function, args) => {
            wrapper_calls(function, calls);
            for arg in args {
                wrapper_calls(arg, calls);
            }
        }
        TypedInner::Closure(_, _, body) => wrapper_calls(body, calls),
        TypedInner::ConstructorCall(_, args) => {
            for arg in args {
                wrapper_calls(arg, calls);
            }
        }
        TypedInner::Lit(_) | TypedInner::Var(_) => {}
        other => panic!("unexpected process wrapper expression: {other:?}"),
    }
}

const DEFAULT_AND_CAPABILITY: &str = r#"supervisor_init { SessionCounter {} }
deftrait SessionChoice {
  def choose(self: Self) -> Self
  def fallback(self: Self) -> Self { SessionChoice::choose(self) }
}
impl SessionChoice for Int { def choose(self: Int) -> Int { self } }
deftrait SessionFunctor where Self: Type<$A> {
  def fmap(self: Self<$A>, mapper: ($A -> $B)) -> Self<$B>
}
defenum SessionBox<$T> { Box($T), }
impl SessionFunctor for SessionBox<$T> {
  def fmap(self: SessionBox<$A>, mapper: ($A -> $B)) -> SessionBox<$B> {
    match self { SessionBox::Box(value) => SessionBox::Box(mapper(value)), }
  }
}
def retain(value: $F<Int>) -> $F<Int> where $F: SessionFunctor {
  SessionFunctor::fmap(value, {|x| x})
}
chosen = SessionChoice::fallback(1)
retained = retain(SessionBox::Box(1))
"#;

fn binding_name(pattern: &TypedPattern) -> &str {
    match pattern {
        TypedPattern::Located(_, inner) => binding_name(inner),
        TypedPattern::Var(_, id) => &id.name,
        other => panic!("expected a simple user binding: {other:?}"),
    }
}

#[derive(Debug, PartialEq)]
struct BodyDispatch {
    trait_name: String,
    method_name: String,
    receiver: Ty,
    target: String,
    arguments: Vec<Ty>,
    result: Ty,
}

fn bound_call_contract(
    program: &TypedProgram,
    session: &ScarSession,
    name: &str,
) -> (Ty, String, Vec<Ty>, Ty, BodyDispatch) {
    let rhs = program
        .nodes
        .iter()
        .find_map(|node| match &node.node {
            TypedInner::Bind(pattern, rhs) if binding_name(pattern) == name => Some(rhs.as_ref()),
            _ => None,
        })
        .expect("user binding must exist");
    let (target_name, index) = match &rhs.node {
        TypedInner::TraitCall {
            dispatch: TraitDispatch::Static(TraitDispatchTarget::UserFunction { name, fun_idx }),
            ..
        } => (name.clone(), *fun_idx),
        TypedInner::App(function, _) => match &function.ty {
            Ty::UserFunc { fun_idx, .. } => {
                let id = match &function.node {
                    TypedInner::Var(id) => id,
                    other => panic!("expected direct generic call: {other:?}"),
                };
                (
                    id.qualified_name.clone().expect("canonical callable name"),
                    *fun_idx,
                )
            }
            other => panic!("expected specialized function type: {other:?}"),
        },
        other => panic!("expected a resolved user dispatch: {other:?}"),
    };
    let definition = session
        .state
        .specializable_defs
        .get(&index)
        .map(Arc::as_ref)
        .or_else(|| {
            program.nodes.iter().find(
                |node| matches!(&node.node, TypedInner::Def(fun_idx, ..) if *fun_idx == index),
            )
        })
        .expect("dispatch must refer to a retained or emitted definition");
    match &definition.node {
        TypedInner::Def(_, _, _, params, ret, _, body, _) => {
            assert_eq!(&body.ty, ret);
            let TypedInner::Block(statements) = &body.node else {
                panic!("expected authored function block")
            };
            assert_eq!(statements.len(), 1);
            let call = &statements[0];
            let TypedInner::TraitCall {
                trait_name,
                method_name,
                receiver_ty,
                args,
                dispatch: TraitDispatch::Static(TraitDispatchTarget::UserFunction { name, fun_idx }),
                ..
            } = &call.node
            else {
                panic!("default and capability bodies need concrete method dispatch: {call:?}")
            };
            assert!(
                session.state.specializable_defs.contains_key(fun_idx)
                    || program.nodes.iter().any(
                        |node| matches!(&node.node, TypedInner::Def(index, ..) if index == fun_idx)
                    ),
                "body dispatch must name an emitted or retained method"
            );
            (
                rhs.ty.clone(),
                target_name,
                params.iter().map(|param| param.ty.clone()).collect(),
                ret.clone(),
                BodyDispatch {
                    trait_name: trait_name.clone(),
                    method_name: method_name.clone(),
                    receiver: receiver_ty.clone(),
                    target: name.clone(),
                    arguments: args.iter().map(|arg| arg.ty.clone()).collect(),
                    result: call.ty.clone(),
                },
            )
        }
        other => panic!("dispatch target must be a function definition: {other:?}"),
    }
}

#[test]
fn full_standard_compile_and_checkpoint_resume_preserve_user_contracts() {
    let suffix = resolved_suffix(DEFAULT_AND_CAPABILITY, Some(COUNTER));
    let full = resolve_program(DEFAULT_AND_CAPABILITY, Some(COUNTER), false);
    let mut full_session = ScarSession::new();
    let full = full_session
        .typecheck_staged_program_with_context(full, standard_context())
        .unwrap();
    let mut resumed_session = prefix_session();
    let resumed = resumed_session.typecheck_staged_program(suffix).unwrap();
    assert_eq!(full_session.process_specs, resumed_session.process_specs);
    assert_eq!(full.boot_plan, resumed.boot_plan);
    // Predeclaring the suffix together with stdlib can legitimately allocate
    // different raw tags, type variables, and function indices. Compare the
    // concrete user call signatures and canonical target names instead.
    for binding in ["chosen", "retained"] {
        let full_contract = bound_call_contract(&full, &full_session, binding);
        let resumed_contract = bound_call_contract(&resumed, &resumed_session, binding);
        assert_eq!(full_contract, resumed_contract, "{binding}");
    }
    assert_eq!(
        bound_call_contract(&resumed, &resumed_session, "chosen").0,
        Ty::Int
    );
    assert_eq!(
        bound_call_contract(&resumed, &resumed_session, "retained").0,
        Ty::Enum("Global::SessionBox".to_owned(), vec![Ty::Int])
    );
    assert_eq!(
        resumed
            .process_specs
            .iter()
            .filter(|spec| spec.process_name.ends_with("SessionCounter"))
            .count(),
        1
    );

    let bad = format!("{DEFAULT_AND_CAPABILITY}\ninvalid: Int = \"bad\"");
    let full_error = ScarSession::new()
        .typecheck_staged_program_with_context(
            resolve_program(&bad, Some(COUNTER), false),
            standard_context(),
        )
        .unwrap_err();
    let resumed_error = prefix_session()
        .typecheck_staged_program(resolved_suffix(&bad, Some(COUNTER)))
        .unwrap_err();
    assert_eq!(resumed_error.reason(), Some(Reason::AnnotationTypeMismatch));
    assert_eq!(resumed_error.structured, full_error.structured);
    assert!(resumed_error.structured.is_some());
    assert_eq!(resumed_error.span, full_error.span);
    assert_eq!(
        &bad[resumed_error.span.start..resumed_error.span.end],
        "\"bad\""
    );
}

#[test]
fn test_script_checks_unit_result_and_rejects_top_level_propagation() {
    let context = TypecheckContext {
        test_script: true,
        ..TypecheckContext::default()
    };
    for source in [
        "()",
        "value = 3",
        "1;",
        "do {\n Ok(())?\n Ok(())\n};\n()",
        "closure: (-> Result<Unit>) = {|| Ok(())?\n Ok(())}\n()",
    ] {
        prefix_session()
            .typecheck_staged_program_with_context(resolved_suffix(source, None), context.clone())
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
    }
    for (source, expected, span) in [
        (
            "42",
            "Test scripts must return Unit, got Int.",
            Span { start: 0, end: 2 },
        ),
        (
            "Ok(())?",
            "Test scripts return Unit and cannot receive propagated errors.",
            Span { start: 6, end: 7 },
        ),
        (
            "value =? Ok(3)",
            "Test scripts return Unit and cannot receive propagated errors.",
            Span { start: 6, end: 8 },
        ),
        (
            "Ok(())  ?;",
            "Test scripts return Unit and cannot receive propagated errors.",
            Span { start: 8, end: 9 },
        ),
    ] {
        let error = prefix_session()
            .typecheck_staged_program_with_context(resolved_suffix(source, None), context.clone())
            .err()
            .unwrap_or_else(|| panic!("expected rejection: {source}"));
        assert_eq!(error.message, expected, "{source}");
        assert_eq!(error.span, span, "{source}");
        assert!(error.structured.is_some());
    }
}

#[test]
fn staged_nominal_annotations_do_not_see_future_definitions() {
    const EARLY: &str = "defmod Early { def identity(value: Later) -> Later { value } }";
    const LATER: &str = "defrecord Later(value: Int)";
    for stages in [
        vec![format!("{EARLY}\n{LATER}")],
        vec![LATER.into(), EARLY.into()],
    ] {
        let sources = stages.iter().map(String::as_str).collect::<Vec<_>>();
        prefix_session()
            .typecheck_staged_program(custom_staged_program(&sources, ""))
            .expect("visible nominal");
    }
    let error = prefix_session()
        .typecheck_staged_program(custom_staged_program(&[EARLY, LATER], ""))
        .expect_err("a future nominal must not satisfy an earlier annotation");
    assert!(error.message.contains("Later"), "{error:?}");
}

fn custom_staged_program(sources: &[&str], entry: &str) -> ResolvedStagedProgram {
    let mut stages = standard_prefix().stages.clone();
    stages.extend(sources.iter().map(|source| {
        let ast = spire::parse_with_context(source, spire::ParserContext::module(0, None)).unwrap();
        sigil::staged_modules_from_source_ast(ast, None)
    }));
    let declarations = sigil::precollect_declaration_index(&stages).unwrap();
    let ast = spire::parse_with_context(entry, spire::ParserContext::project(0)).unwrap();
    sigil::resolve_staged_program_from_state(
        &stages,
        ast,
        &declarations,
        None,
        standard_prefix().stages.len(),
        standard_prefix().resolved.resume_state,
    )
    .unwrap()
}

#[test]
fn staged_concrete_dispatch_cannot_borrow_a_future_impl() {
    const TRAIT: &str = "deftrait StageMetric { def metric(value: Self) -> Int }";
    const CALL: &str = "defmod Early { def call() -> Int { StageMetric::metric(7) } }";
    const IMPL: &str = "impl StageMetric for Int { def metric(value: Int) -> Int { value } }";
    let combined = format!("{TRAIT}\n{CALL}\n{IMPL}");
    let mut session = prefix_session();
    session
        .typecheck_staged_program(custom_staged_program(&[&combined], "Early::call()"))
        .expect("same-stage impl is visible before bodies");
    let program = custom_staged_program(&[&format!("{TRAIT}\n{CALL}"), IMPL], "");
    let mut session = prefix_session();
    let before = bincode::serialize(&session.checkpoint()).unwrap();
    let error = session
        .typecheck_staged_program(program)
        .expect_err("concrete call cannot use a future-stage impl");
    assert!(error.message.contains("StageMetric"), "{error:?}");
    assert_eq!(bincode::serialize(&session.checkpoint()).unwrap(), before);
}

#[test]
fn staged_generic_contract_can_instantiate_with_a_later_visible_impl() {
    const GENERIC: &str = "deftrait StageMetric { def metric(value: Self) -> Int }\ndefmod Early { def call(value: $T) -> Int where $T: StageMetric { StageMetric::metric(value) } }";
    const IMPL: &str = "impl StageMetric for Int { def metric(value: Int) -> Int { value } }";
    let program = custom_staged_program(&[GENERIC, IMPL], "Early::call(7)");
    let typed = prefix_session()
        .typecheck_staged_program(program)
        .expect("a checked generic is instantiated in its caller's visible environment");
    let call = typed
        .nodes
        .iter()
        .find(|node| matches!(node.node, TypedInner::App(..)))
        .expect("the entry call is retained alongside generated specializations");
    assert_eq!(call.ty, Ty::Int);
}

#[test]
fn const_dependency_failure_preserves_saved_prefix_and_allows_retry() {
    let prefix_source =
        "defrecord ConstSessionItem(value: Int)\nconst STABLE = ConstSessionItem.value";
    let mut prefix = prefix_session();
    prefix
        .typecheck(resolved_suffix(prefix_source, None).into_resolved())
        .unwrap();
    let checkpoint = prefix.checkpoint();
    let saved = bincode::serialize(&checkpoint).unwrap();
    let suffix = |source: &str| {
        // Keep resolution identities for the retained declarations while only
        // passing the new stage to the persistent typechecking environment.
        resolved_suffix(&format!("{prefix_source}\n{source}"), None)
            .into_resolved()
            .into_iter()
            .skip(2)
            .collect::<Vec<_>>()
    };
    let bad = suffix("const VALID = STABLE\nconst FIRST = SECOND\nconst SECOND = FIRST");
    let good = suffix("const VALID = STABLE\nconst FIRST = SECOND\nconst SECOND = STABLE\nFacet::view(FIRST, ConstSessionItem(7))");
    let stable_uid = prefix
        .state
        .consts
        .iter()
        .find_map(|(uid, value)| (value.name == "STABLE").then_some(*uid))
        .unwrap();
    let expected_path = prefix.state.consts[&stable_uid].value.clone();
    for api in [SessionApi::Transactional, SessionApi::InPlace] {
        let mut rejected = prefix.clone();
        let error = api.check(&mut rejected, bad.clone()).unwrap_err();
        assert_eq!(
            error.reason(),
            Some(Reason::CyclicConst),
            "{api:?}: {error:?}"
        );
        assert_eq!(
            bincode::serialize(&checkpoint).unwrap(),
            saved,
            "a failed suffix must not change its saved prefix"
        );
        assert_eq!(
            bincode::serialize(&prefix.checkpoint()).unwrap(),
            saved,
            "a failed sibling must not change the retained session"
        );
        if matches!(api, SessionApi::Transactional) {
            assert_eq!(bincode::serialize(&rejected.checkpoint()).unwrap(), saved);
            assert!(!rejected
                .state
                .consts
                .values()
                .any(|value| value.name == "VALID"));
            // The same IDs can be used immediately after a failed transaction.
            let immediate = api.check(&mut rejected, good.clone()).unwrap();
            assert_eq!(immediate.last().unwrap().ty, Ty::Int);
        } else {
            assert!(
                rejected
                    .state
                    .consts
                    .values()
                    .any(|value| value.name == "VALID"),
                "the one-shot failure occurred after completing its independent const"
            );
            assert!(
                !rejected
                    .state
                    .consts
                    .values()
                    .any(|value| matches!(value.name.as_str(), "FIRST" | "SECOND")),
                "pending const declarations must stay out of persistent state"
            );
        }
        rejected.rollback(checkpoint.clone());
        let retry = api.check(&mut rejected, good.clone()).unwrap();
        let mut sibling = prefix.clone();
        let expected = api.check(&mut sibling, good.clone()).unwrap();
        assert_eq!(retry, expected, "{api:?}: retry must match a fresh sibling");
        assert_eq!(rejected.state.consts[&stable_uid].value, expected_path);
        assert_eq!(bincode::serialize(&checkpoint).unwrap(), saved);
        // A serialized checkpoint must contain only completed const values.
        let restored: ScarCheckpoint = bincode::deserialize(&saved).unwrap();
        let mut restored_session = prefix_session();
        restored_session.rollback(restored);
        assert_eq!(
            api.check(&mut restored_session, good.clone()).unwrap(),
            expected
        );
    }
}

#[test]
fn staged_suffix_exports_only_its_own_process_metadata() {
    for api in [SessionApi::Transactional, SessionApi::InPlace] {
        let mut session = prefix_session();
        let saved = session.process_specs.clone();
        assert!(
            !saved.is_empty(),
            "standard prefix must exercise process metadata"
        );
        let empty = api
            .check_staged(&mut session, resolved_suffix("", None))
            .unwrap();
        assert!(empty.process_specs.is_empty());
        assert_eq!(session.process_specs, saved);
        let added = api
            .check_staged(&mut session, resolved_suffix("", Some(COUNTER)))
            .unwrap();
        assert_eq!(added.process_specs.len(), 1);
        assert!(added.process_specs[0]
            .process_name
            .ends_with("SessionCounter"));
        assert_eq!(session.process_specs.len(), saved.len() + 1);
        assert!(session.process_specs.starts_with(&saved));
    }
}

#[test]
fn future_qualified_nominal_does_not_resolve_to_an_unrelated_visible_suffix() {
    let program = custom_staged_program(
        &[
            "defrecord Shared(value: Int)",
            "defmod Early { def identity(value: Future::Shared) -> Future::Shared { value } }",
            "namespace Future { defrecord Shared(value: String) }",
        ],
        "",
    );
    let error = prefix_session()
        .typecheck_staged_program(program)
        .expect_err("a future qualified type cannot be replaced with a visible short-name match");
    assert!(error.message.contains("Future::Shared"), "{error:?}");
}

#[test]
fn bootstrap_source_does_not_see_standard_nominals_from_native_inventory() {
    for metadata in sindr::builtin::NATIVE_SIGNATURE_NOMINAL_METAS {
        let mut stages = standard_prefix().stages.clone();
        let source = format!(
            "defmod Early {{ def identity(value: {0}) -> {0} {{ value }} }}",
            metadata.name
        );
        let early =
            spire::parse_with_context(&source, spire::ParserContext::module(0, None)).unwrap();
        stages[0].extend(sigil::staged_modules_from_source_ast(early, None));
        let declarations = sigil::precollect_declaration_index(&stages).unwrap();
        let resolved =
            sigil::resolve_staged_program_with_state(&stages, Vec::new(), &declarations, None)
                .unwrap();
        let error = ScarSession::new()
            .typecheck_staged_program_with_context(resolved, standard_context())
            .err()
            .expect("native signature inventory must not expose a future source nominal");
        assert!(
            error
                .message
                .contains(&format!("Unknown type: {}", metadata.name)),
            "{error:?}"
        );
    }
}
