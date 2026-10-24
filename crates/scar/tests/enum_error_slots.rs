#[allow(dead_code)]
mod support;

fn resolve_generic(source: &str) -> Vec<sigil::resolved::Resolved> {
    let standard = spire::parse_with_context(
        "@builtin defenum Result<$T> { Ok($T), Err(Error) }",
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
    )
    .unwrap();
    let stages = vec![sigil::staged_modules_from_source_ast(standard, None)];
    let precollected = sigil::precollect_declarations(&stages).unwrap();
    let source = format!(
        "deferror NoneError {{ \"none\" }}\ndeferror ZeroDivisionError {{ \"zero\" }}\n{source}"
    );
    let ast = spire::parse_with_context(&source, spire::ParserContext::project(0)).unwrap();
    let resolved =
        sigil::resolve_staged_program(&stages, ast, &precollected.declaration_index, None).unwrap();
    resolved
}

fn check_generic(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    scar::typecheck(resolve_generic(source))
}

#[test]
fn generic_error_positions_accept_error_sets_and_reject_non_errors() {
    for source in [
        "def relay(error: $E) -> Result<Int, $E> { Err(error) }\nvalue = relay(NoneError())",
        "defenumerror Closed { NoneError }\ndef relay(error: $E) -> Result<Int, $E> { Err(error) }\nerror = Closed::NoneError()\nvalue = relay(error)",
        "def success::<$E>(value: Int) -> Result<Int, $E> { Ok(value) }\ndefenumerror Closed { NoneError }\nvalue = success::<Closed>(1)",
    ] {
        check_generic(source).expect(source);
    }
    for source in [
        "def relay(error: $E) -> Result<Int, $E> { Err(error) }\nvalue = relay(1)",
        "def success::<$E>(value: Int) -> Result<Int, $E> { Ok(value) }\nvalue = success::<Int>(1)",
        "def success::<$E>(value: Int) -> Result<Int, $E> { Ok(value) }\nvalue = success::<NoneError>(1)",
    ] {
        check_generic(source).expect_err(source);
    }
}

#[test]
fn monad_fail_checks_the_result_error_position() {
    for source in [
        "value: Result<Int> = MonadFail::fail(NoneError())",
        "defenumerror Open { NoneError, Other(Error) }\nvalue: Result<Int, Open> = MonadFail::fail(NoneError())",
        "defenumerror Open { NoneError, Other(Error) }\nvalue: ResultT<Identity, Int, Open> = MonadFail::fail(NoneError())",
        "defenumerror Open { Other(Error) }\ndef checked() -> Result<Int, Open> { value =? Ok(1); Ok(value) }\nvalue = checked()",
        "defenumerror Open { Other(Error) }\nvalue: Result<Int, Open> = MonadFail::fail_if(True, NoneError(), Ok(1))",
        "defenumerror Open { Other(Error) }\nvalue = MonadFail::fail::<Result<Int, Open>>(NoneError())",
    ] {
        support::typecheck(support::resolve_with_builtin_prelude(source)).expect(source);
    }
    for source in [
        "defenumerror Closed { NoneError }\nvalue: Result<Int, Closed> = MonadFail::fail(NoneError())",
        "defenumerror Closed { NoneError }\nvalue: ResultT<Identity, Int, Closed> = MonadFail::fail(NoneError())",
        "defenumerror Closed { NoneError }\nfactory: (Error -> Result<Int, Closed>) = &MonadFail::fail",
        "defenumerror Closed { NoneError }\nvalue = MonadFail::fail::<Result<Int, Closed>>(NoneError())",
    ] {
        support::typecheck(support::resolve_with_builtin_prelude(source)).expect_err(source);
    }
}

#[test]
fn generic_constructor_membership_is_checked_after_specialization() {
    for source in [
        "def selected::<$E>() -> Result<Int, $E> { Err(NoneError()) }\ndefenumerror Closed { NoneError }\nvalue = selected::<Closed>()",
        "def selected::<$E>() -> Result<Int, $E> { Err(NoneError()) }\ndefenumerror Closed { NoneError }\nfactory: (-> Result<Int, Closed>) = &selected\nvalue = factory()",
        "def selected::<$E>() -> Result<Int, $E> { Err(NoneError()) }\ndefenumerror Open { Other(Error) }\nvalue = selected::<Open>()",
    ] {
        check_generic(source).expect(source);
    }
    for source in [
        "def selected::<$E>() -> Result<Int, $E> { Err(NoneError()) }\ndefenumerror Closed { ZeroDivisionError }\nvalue = selected::<Closed>()",
        "def selected::<$E>() -> Result<Int, $E> { Err(NoneError()) }\ndefenumerror Closed { ZeroDivisionError }\nfactory: (-> Result<Int, Closed>) = &selected\nvalue = factory()",
        "def erased::<$E>(error: Error) -> Result<Int, $E> { Err(error) }\ndefenumerror Closed { NoneError }\nvalue = erased::<Closed>(NoneError())",
    ] {
        check_generic(source).expect_err(source);
    }
}

#[test]
fn generic_error_obligations_survive_serialized_checkpoints() {
    let mut resolved = resolve_generic("def erased::<$E>(error: Error) -> Result<Int, $E> { Err(error) }\ndefenumerror Open { Other(Error) }\ndefenumerror Closed { NoneError }\naccepted = erased::<Open>(NoneError())\nrejected = erased::<Closed>(NoneError())");
    let rejected = resolved.pop().unwrap();
    let accepted = resolved.pop().unwrap();
    let mut session = scar::ScarSession::new();
    session.typecheck(resolved).unwrap();
    let checkpoint: scar::ScarCheckpoint =
        bincode::deserialize(&bincode::serialize(&session.checkpoint()).unwrap()).unwrap();
    session = scar::ScarSession::new();
    session.rollback(checkpoint.clone());
    session
        .typecheck(vec![accepted])
        .expect("Open conversion retains its static obligation after restore");
    session.rollback(checkpoint);
    session
        .typecheck(vec![rejected])
        .expect_err("Closed conversion remains rejected after restore");
}

#[test]
fn other_patterns_select_only_the_inspected_enum_error_owner() {
    for source in [
        "defenumerror First { NoneError, Other(Error) }\ndefenumerror Second { ZeroDivisionError, Other(Error) }\nvalue = First::ZeroDivisionError()\nanswer = match value { NoneError => 0, Other(error: Error) => 1 }",
        "defenumerror First { NoneError, Other(Error) }\ndefenumerror Second { ZeroDivisionError, Other(Error) }\nvalue = Second::NoneError()\nanswer = match value { ZeroDivisionError => 0, Other(error: Error) => 1 }",
    ] {
        check_generic(source).expect(source);
    }
    for source in [
        "defenumerror Open { Other(Error) }\nvalue: Error = NoneError()\nanswer = match value { Other(error: Error) => 0, _ => 1 }",
        "defenumerror Open { Other(Error) }\ndefenumerror Closed { NoneError }\nvalue = Closed::NoneError()\nanswer = match value { Other(error: Error) => 0, _ => 1 }",
        "deferror Other { \"lexical\" }\ndefenumerror Open { Other(Error) }\nvalue = Open::NoneError()\nanswer = match value { Other(_) => 0, _ => 1 }",
    ] {
        check_generic(source).expect_err(source);
    }
}

#[test]
fn empty_enum_error_allows_success_only_result_values() {
    check_generic("defenumerror Empty {}\nvalue: Result<Int, Empty> = Ok(1)").unwrap();
    for source in [
        "defenumerror Empty {}\nvalue = Empty::NoneError()",
        "defenumerror Empty {}\nvalue: Result<Int, Empty> = Err(NoneError())",
    ] {
        check_generic(source).expect_err(source);
    }
}

#[test]
fn common_error_operator_helpers_accept_existing_payload_polymorphism() {
    let source = "def parse_id(text: String) -> Result<Int> { Ok(7) }\ndef load_user(id: Int) -> Result<Int> { Ok(id) }\nlookup = &parse_id >=> &load_user\nvalue: Result<String> = lookup(\"7\") |*> to_string()";
    support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect("ordinary Result operator pipelines preserve payload polymorphism");
}

#[test]
fn aggregate_error_transports_enforce_deferred_membership() {
    let accepted = "defenumerror Open { Other(Error) }\ndef convert::<$E>(values: List<Result<Int>>) -> List<Result<Int, $E>> { values }\nvalue = convert::<Open>([Err(NoneError())])";
    check_generic(accepted).expect(accepted);
    let rejected = "defenumerror Closed { NoneError }\ndef convert::<$E>(values: List<Result<Int>>) -> List<Result<Int, $E>> { values }\nvalue = convert::<Closed>([Err(NoneError())])";
    check_generic(rejected).expect_err(rejected);
}

#[test]
fn aggregate_result_transport_preserves_success_types() {
    let source = "defenumerror Closed { NoneError }\nvalues: List<Result<Closed>> = [Ok(Closed::NoneError())]\nconverted: List<Result<Error>> = values";
    check_generic(source).expect_err(source);
}

#[test]
fn aggregate_error_casts_preserve_all_other_positions() {
    for source in [
        "defenumerror Open { Other(Error) }\noriginal: (Int, Error) = (1, NoneError())\nconverted: (String, Open) = original",
        "defenumerror Closed { NoneError }\ndefenumerror Open { Other(Error) }\ndef takes(error: Error) -> Int { 1 }\noriginal: ((Error -> Int), Error) = (&takes, NoneError())\nconverted: ((Closed -> Int), Open) = original",
    ] { check_generic(source).expect_err(source); }
}

#[test]
fn named_callable_widens_only_its_error_result() {
    for source in [
        "defenumerror Closed { NoneError }\ndef selected() -> Closed { Closed::NoneError() }\nfunction: (-> Error) = &selected\nvalue = function()",
        "defenumerror Closed { NoneError }\ndef selected(value: Int) -> Result<Int, Closed> { Err(Closed::NoneError()) }\nfunction: (Int -> Result<Int>) = &selected\nvalue = function(1)",
    ] { check_generic(source).expect(source); }
}

#[test]
fn standard_constructor_traits_widen_enum_error_inputs() {
    for source in [
        "defenumerror Closed { NoneError }\nselected: Result<Int, Closed> = Err(NoneError())\nvalue: Result<Int> = Functor::fmap(selected, {|x| x + 1})",
        "defenumerror Closed { NoneError }\ndefenumerror Open { Other(Error) }\nselected: Result<Int, Closed> = Err(NoneError())\nvalue: Result<Int, Open> = Functor::fmap(selected, {|x| x + 1})",
        "defenumerror Open { Other(Error) }\nvalue = Applicative::pure::<Result<Int, Open>>(1)",
    ] {
        support::typecheck(support::resolve_with_builtin_prelude(source)).expect(source);
    }
    let source = "defenumerror Closed { NoneError }\nselected: Result<Int, Closed> = Err(NoneError())\nvalue: Result<Int, Closed> = Functor::fmap(selected, {|x| x + 1})";
    support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect_err("standard output is common Error even when input set is known");
    let source =
        "defenumerror Closed { NoneError }\nvalue = Applicative::pure::<Result<Int, Closed>>(1)";
    support::typecheck(support::resolve_with_builtin_prelude(source))
        .expect_err("an explicit standard result receiver still requires Other");
}

#[test]
fn nominal_enum_transports_validate_stored_payload_positions() {
    for source in [
        "defenumerror Closed { NoneError }\ndefenum Box<$T> { Value(Result<$T>) }\noriginal: Box<Closed> = Box<Closed>::Value(Ok(Closed::NoneError()))\nconverted: Box<Error> = original",
        "defenumerror Closed { NoneError }\ndefenum Sink<$T> { Use(($T -> Int)) }\ndef consumes(error: Closed) -> Int { 1 }\noriginal: Sink<Closed> = Sink<Closed>::Use(&consumes)\nconverted: Sink<Error> = original",
    ] { check_generic(source).expect_err(source); }
    let source = "defenumerror Closed { NoneError }\ndefenum Box<$T> { Value($T), Next(Box<$T>) }\noriginal: Box<Closed> = Box<Closed>::Next(Box<Closed>::Value(Closed::NoneError()))\nconverted: Box<Error> = original";
    check_generic(source).expect(source);
}

#[test]
fn nonregular_enum_error_transport_terminates() {
    let source = "defenumerror Closed { NoneError }\ndefenum Nest<$T> { Value($T), Next(Nest<List<$T>>) }\noriginal: Nest<Closed> = Nest<Closed>::Value(Closed::NoneError())\nconverted: Nest<Error> = original";
    check_generic(source).expect(source);
}

#[test]
fn inferred_result_branches_do_not_retype_success_payloads() {
    let source = "value: Result<Int> = Err(NoneError())\nconverted = match value { Ok(_) => Ok(\"text\"), _ => value }";
    check_generic(source).expect_err(source);
}

#[test]
fn higher_kind_recursive_transport_uses_each_constructor_profile() {
    let source = "defenumerror Closed { NoneError }\ndefenum Shift<$M, $T> where $M: Functor { Value($M<$T>), Next(Shift<Result, $T>) }\noriginal: Shift<List, Closed> = Shift::Value([Closed::NoneError()])\nconverted: Shift<List, Error> = original";
    support::typecheck(support::resolve_with_builtin_prelude(source)).expect_err(
        "recursive Result success slots stay invariant even when the outer constructor is List",
    );
    let source = "defenumerror Closed { NoneError }\ndefenum Nest<$M, $T> where $M: Functor { Value($M<$T>), Next(Nest<$M, List<$T>>) }\noriginal: Nest<List, Closed> = Nest::Value([Closed::NoneError()])\nconverted: Nest<List, Error> = original";
    support::typecheck(support::resolve_with_builtin_prelude(source)).expect(source);
}

#[test]
fn generic_nominal_transport_checks_the_specialized_constructor_profile() {
    let declaration = "defenumerror Closed { NoneError }\ndefenum Layer<$M, $T> where $M: Functor { Value($M<$T>) }\ndef widen(value: Layer<$M, Closed>) -> Layer<$M, Error> where $M: Functor { value }";
    let accepted = format!("{declaration}\noriginal: Layer<List, Closed> = Layer::Value([Closed::NoneError()])\nconverted = widen(original)");
    support::typecheck(support::resolve_with_builtin_prelude(&accepted))
        .expect("List's mapped argument permits Error transport");
    let rejected = format!("{declaration}\noriginal: Layer<Result, Closed> = Layer::Value(Ok(Closed::NoneError()))\nconverted = widen(original)");
    support::typecheck(support::resolve_with_builtin_prelude(&rejected))
        .expect_err("Result's mapped success argument forbids the same generic transport");
}

#[test]
fn user_constructor_trait_implementation_keeps_its_error_contract() {
    let definitions = "defenumerror Closed { NoneError }\nimpl Functor for Result<$T, Closed> where $T: Functor.$A { def fmap(self: Result<$A, Closed>, mapper: ($A -> $B)) -> Result<$B, Closed> { match self { Ok(value) => Ok(mapper(value)), Err(error) => Err(error) } } }";
    support::typecheck(support::resolve_with_builtin_prelude(definitions))
        .expect("user declaration is a legal distinct carrier implementation");
    let source = format!("{definitions}\noriginal: Result<Int, Closed> = Err(NoneError())\nselected: Result<String, Closed> = Functor::fmap(original, {{|_| \"text\"}})");
    support::typecheck(support::resolve_with_builtin_prelude(&source))
        .expect("user implementation keeps its declared error position");
}

#[test]
fn generic_monad_fail_checks_its_specialized_error_receiver() {
    let declarations = "defenumerror Open { Other(Error) }\ndefenumerror Closed { NoneError }\ndef build::<$E>() -> Result<Int, $E> { MonadFail::fail(NoneError()) }";
    let accepted = format!("{declarations}\nvalue = build::<Open>()");
    support::typecheck(support::resolve_with_builtin_prelude(&accepted))
        .expect("generic fail can classify common Error into Open after specialization");
    let rejected = format!("{declarations}\nvalue = build::<Closed>()");
    support::typecheck(support::resolve_with_builtin_prelude(&rejected))
        .expect_err("generic fail cannot classify common Error into a closed set");
}

#[test]
fn generic_pattern_failure_checks_its_specialized_error_receiver() {
    for body in ["value =? Ok(1); Ok(value)", "True =? False; Ok(1)"] {
        let declarations = format!("defenumerror Open {{ Other(Error) }}\ndefenumerror Closed {{ NoneError }}\ndef checked::<$E>() -> Result<Int, $E> {{ {body} }}");
        let accepted = format!("{declarations}\nvalue = checked::<Open>()");
        support::typecheck(support::resolve_with_builtin_prelude(&accepted))
            .expect("a generic failure route can classify common Error into Open");
        let rejected = format!("{declarations}\nvalue = checked::<Closed>()");
        support::typecheck(support::resolve_with_builtin_prelude(&rejected))
            .expect_err("a generic failure route cannot classify common Error into a closed set");
    }
}

#[test]
fn ordinary_constructor_inference_preserves_common_error_carriers() {
    let source =
        "full = MonadT::lift::<OptionT<Result, Int>>(Ok(1))\nrestored = OptionT::run(full)";
    support::typecheck(support::resolve_with_builtin_prelude(source)).expect("ordinary constructor inference with an unchanged common Error slot needs no Error transport");
}

const EXPLICIT_ERROR_HOLE_MESSAGE: &str =
    "Error position cannot be `_`; use Error, EnumError, or a named type variable.";

#[test]
fn explicit_result_error_holes_are_rejected_in_every_source_type_position() {
    for source in [
        "value: Result<Int, _> = Ok(1)",
        "defstruct Holder { value: Result<Int, _> }",
        "def read(value: Result<Int, _>) -> Result<Int> { value }",
        "def make() -> Result<Int, _> { Ok(1) }",
        "factory: (-> Result<Int, _>) = {|| Ok(1)}",
        "values: List<Result<Int, _>> = [Ok(1)]",
        "def make::<$T>() -> List<$T> { [] }\nvalue = make::<Result<Int, _>>()",
    ] {
        let error = check_generic(source).expect_err(source);
        assert_eq!(
            error.message, EXPLICIT_ERROR_HOLE_MESSAGE,
            "{source}: {error:?}"
        );
        assert_eq!(error.span.end - error.span.start, 1, "{source}: {error:?}");
        let prefix = "deferror NoneError { \"none\" }\ndeferror ZeroDivisionError { \"zero\" }\n";
        assert_eq!(
            error.span.start,
            prefix.len() + source.find('_').unwrap(),
            "{source}: {error:?}"
        );
    }
}

#[test]
fn explicit_result_t_error_holes_are_rejected_in_signatures_and_return_type_arguments() {
    for source in [
        "value: ResultT<Identity, Int, _> = ResultT::ok::<Identity>(1)",
        "defstruct Holder { value: ResultT<Identity, Int, _> }",
        "def read(value: ResultT<Identity, Int, _>) -> ResultT<Identity, Int, Error> { value }",
        "def make() -> ResultT<Identity, Int, _> { ResultT::ok::<Identity>(1) }",
        "factory: (-> ResultT<Identity, Int, _>) = {|| ResultT::ok::<Identity>(1)}",
        "values: List<ResultT<Identity, Int, _>> = [ResultT::ok::<Identity>(1)]",
        "value = MonadFail::fail::<ResultT<Identity, Int, _>>(NoneError())",
        "def make::<$T>() -> List<$T> { [] }\nvalue = make::<ResultT<Identity, Int, _>>()",
        "value = Applicative::pure::<Result<Int, _>>(1)",
    ] {
        let error =
            support::typecheck(support::resolve_with_builtin_prelude(source)).expect_err(source);
        assert_eq!(
            error.message, EXPLICIT_ERROR_HOLE_MESSAGE,
            "{source}: {error:?}"
        );
        assert_eq!(error.span.end - error.span.start, 1, "{source}: {error:?}");
    }
}

#[test]
fn omitted_result_errors_and_success_type_holes_keep_their_existing_meaning() {
    for source in [
        "value: Result<Int> = Ok(1)",
        "value = Result<Int>::Ok(1)",
        "def make::<$T>() -> List<$T> { [] }\nvalue: List<Int> = make::<_>()",
    ] {
        check_generic(source).expect(source);
    }
    for source in [
        "value: Result<Int> = Applicative::pure::<Result<_>>(1)",
        "value: Result<Int, Error> = Applicative::pure::<Result<_, Error>>(1)",
        "value: ResultT<Identity, Int, Error> = MonadFail::fail::<ResultT<Identity, _, Error>>(NoneError())",
    ] {
        support::typecheck(support::resolve_with_builtin_prelude(source)).expect(source);
    }
}

#[test]
fn enum_error_headers_are_visible_before_any_same_stage_field_schema() {
    let holder = "defstruct Holder { value: Result<Int, Failures> }\nimpl Holder { def new(value: Result<Int, Failures>) -> Holder { Holder { value: value } } }";
    let errors = "defenumerror Failures { NoneError }";
    for declarations in [format!("{holder}\n{errors}"), format!("{errors}\n{holder}")] {
        let source = format!("{declarations}\nvalue = Holder::new(Err(NoneError()))");
        check_generic(&source)
            .expect("same-stage EnumError declarations must not depend on field-schema order");
    }
}

#[test]
fn namespaced_enum_error_headers_are_visible_across_same_stage_modules() {
    let standard = spire::parse_with_context(
        "@builtin defenum Result<$T> { Ok($T), Err(Error) }",
        spire::ParserContext::module(0, None).with_rules(spire::ParseRules::std_module()),
    )
    .unwrap();
    let holder = "defstruct Holder { value: Result<Int, FailuresNS::Failures> }\nimpl Holder { def new(value: Result<Int, FailuresNS::Failures>) -> Holder { Holder { value: value } } }";
    let errors = "deferror ErrorsNS::Failure { \"failure\" }\ndefenumerror FailuresNS::Failures { ErrorsNS::Failure }";
    for sources in [[holder, errors], [errors, holder]] {
        let modules = sources
            .into_iter()
            .flat_map(|source| {
                let ast = spire::parse_with_context(source, spire::ParserContext::module(0, None))
                    .unwrap();
                sigil::staged_modules_from_source_ast(ast, None)
            })
            .collect::<Vec<_>>();
        let stages = vec![
            sigil::staged_modules_from_source_ast(standard.clone(), None),
            modules,
        ];
        let declarations = sigil::precollect_declarations(&stages).unwrap();
        let user = spire::parse("value = Holder::new(Err(ErrorsNS::Failure()))").unwrap();
        let resolved = sigil::resolve_staged_program_from_state(
            &stages,
            user,
            &declarations.declaration_index,
            None,
            0,
            sigil::ResolveResumeState::default(),
        )
        .unwrap();
        scar::typecheck_staged_program(resolved).expect("namespaced members of a declaration stage share their reserved Error collection identities");
    }
}

#[test]
fn nominal_error_roles_follow_forward_and_recursive_schema_references() {
    for declarations in [
        "defenum Outer<$E> { Wrapped(Inner<$E>) }\ndefenum Inner<$E> { Held(Result<Int, $E>) }",
        "defenum Inner<$E> { Held(Result<Int, $E>) }\ndefenum Outer<$E> { Wrapped(Inner<$E>) }",
    ] {
        let accepted = format!("{declarations}\ndef inspect(value: Outer<$E>) -> Outer<$E> {{ value }}\nvalue = inspect(Outer<Error>::Wrapped(Inner<Error>::Held(Ok(1))))");
        check_generic(&accepted).expect(&accepted);
        let rejected = format!("{declarations}\ndef inspect(value: Outer<Int>) -> () {{ () }}");
        check_generic(&rejected).expect_err(&rejected);
    }
    let source = "defenum Nest<$T, $E> { Value(Result<$T, $E>), Next(Nest<List<$T>, $E>) }\nvalue: Nest<Int, Error> = Nest<Int, Error>::Value(Ok(1))";
    check_generic(source)
        .expect("recursive success arguments may grow while the Error role stays fixed");
}

#[test]
fn nominal_error_roles_require_the_whole_argument_to_be_an_error_type() {
    for source in [
        "defenum Holder<$E> { Held(Result<Int, $E>) }\ndefenum Invalid<$T> { Wrapped(Holder<List<$T>>) }",
        "defenum Invalid<$E> { Value(Result<Int, $E>), Next(Invalid<List<$E>>) }",
        "defenum Holder<$E> { Held(Result<Int, $E>) }\ndef inspect(value: Holder<Result<Int>>) -> () { () }",
    ] {
        check_generic(source).expect_err(source);
    }
    let source = "defenum Box<$T> { Value($T), Next(Box<List<$T>>) }\nvalue: Box<List<Result<Int>>> = Box<List<Result<Int>>>::Value([Ok(1)])";
    check_generic(source).expect("an ordinary payload containing Result has no Error role itself");
}

#[test]
fn enum_error_trait_policy_rejects_traits_and_allows_inherent_methods() {
    let error = check_generic(
        "defenumerror Failures { NoneError }\ndeftrait Marker { def mark(self: Self) -> Int }\nimpl Marker for Failures { def mark(self: Self) -> Int { 1 } }",
    ).expect_err("EnumError owners cannot acquire user trait implementations");
    assert_eq!(
        error.reason(),
        Some(diagnostics::TypeDiagnosticReason::TraitImplementationForbidden),
        "{error:?}"
    );
    check_generic(
        "defenumerror Failures { NoneError }\nimpl Failures { def code(value: Failures) -> Int { 1 } }\nvalue = Failures::code(Failures::NoneError())",
    ).expect("an EnumError owner still permits its single inherent implementation");
}
