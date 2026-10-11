#[allow(dead_code)]
mod support;
use sindr::policy::RuntimeSourcePolicy;

#[test]
fn errors_are_ordinary_values() {
    for source in [
        "deferror Trouble(code: Int) { |code: Int| Self(message: \"trouble\", code: code) }\ndef relay(error: Error) -> Error { error }\nerror: Error = relay(Trouble(1))\nerrors: List<Error> = [error]\nsaved: Result<Error> = Ok(error)",
        "deferror Trouble(code: Int) { |code: Int| Self(message: \"trouble\", code: code) }\nfactory: (Int -> Error) = &Trouble\nwrap: (Error -> Result<Int>) = &Err\nvalue = wrap(factory(1))",
        "factory: (-> Error) = &NoneError\nerror = factory()",
        "impl Int { defextractor relay(v: Int) -> MatchResult<Int> { error = NoneError()\nMatchResult::Err(error) } }",
        "deferror Trouble(code: Int) { |code: Int| Self(message: \"trouble\", code: code) }\nerror = Trouble(1)\nhandler: (Error -> String) = {|e: Error| Error::message(e)}\nname = handler(error)",
        "deferror Trouble(code: Int) { |code: Int| Self(message: \"trouble\", code: code) }\npath = Trouble.code",
        "deferror Trouble(code: Int) { |code: Int| Self(message: \"trouble\", code: code) }\nvalue = match Trouble(1) { Trouble(code) => code, _ => 0 }",
    ] {
        support::typecheck_with_rules(source, RuntimeSourcePolicy::script()).expect(source);
    }
}

#[test]
fn error_observation_paths_are_readonly() {
    for source in [
        "error = NoneError()\nkind: String = error.kind\nmessage: String = error.message\npath = Error.message\nread: String = Facet::view(path, error)",
        "defrecord Failure(error: Error)\nfailure = Failure(NoneError())\nmessage: String = failure.error.message\npath = Failure.error.message\nread: String = Facet::view(path, failure)",
    ] {
        support::typecheck_with_rules(source, RuntimeSourcePolicy::script()).expect(source);
    }
    for source in [
        "value = Facet::set(Error.message, NoneError(), \"changed\")",
        "value = Facet::over(Error.kind, NoneError(), {|value| value})",
        "update = &Facet::set(Error.message, &1, \"changed\")",
        "defrecord Failure(error: Error)\nvalue = Facet::set(Failure.error.message, Failure(NoneError()), \"changed\")",
        "value = Facet::bulk_update(NoneError()) { message <- set(\"changed\") }",
        "path = Facet::compose(ErrorEnvelope.error, Error.message)\ndefrecord ErrorEnvelope(error: Error)\nvalue = Facet::set(path, ErrorEnvelope(NoneError()), \"changed\")",
        "value = (NoneError()).location",
        "value = (NoneError()).stack_trace",
    ] {
        support::typecheck_with_rules(source, RuntimeSourcePolicy::script()).expect_err(source);
    }
}

#[test]
fn error_representation_and_capabilities_stay_private() {
    for source in [
        "value = Error()",
        "value = Error",
        "factory = &Error",
        "value = Error(kind: \"fake\", message: \"fake\")",
        "deftrait ReadError { def read(value: Self) -> Int }\nimpl ReadError for Error { def read(value: Error) -> Int { 1 } }",
        "value = NoneError() == NoneError()",
        "value = to_string(NoneError())",
    ] {
        let result = support::resolve_with_builtin_prelude_result(source).map(support::typecheck);
        assert!(result.is_err() || result.unwrap().is_err(), "must reject: {source}");
    }
}

#[test]
fn error_kinds_are_ordinary_values_and_calls_stay_errors() {
    for source in [
        "kind: ErrorKind = NoneError\npair: (ErrorKind, Int) = (NoneError, 1)\nkinds: List<ErrorKind> = [NoneError, ZeroDivisionError]",
        "deferror Trouble(code: Int) { |code: Int| Self(message: \"trouble\", code: code) }\nkind = Trouble\ndef relay(kind: ErrorKind) -> ErrorKind { kind }\nsaved = relay(kind)\nerror: Error = Trouble(1)",
        "kind = (NoneError)\nfactory: (-> Error) = &NoneError\nerror: Error = factory()",
        "deferror Tagged(selected: ErrorKind) { |selected: ErrorKind| Self(message: \"tagged\", selected: selected) }\nerror: Error = Tagged(NoneError)",
    ] {
        support::typecheck_with_rules(source, RuntimeSourcePolicy::script()).expect(source);
    }
}

#[test]
fn bare_error_kind_never_implicitly_constructs_error() {
    for source in [
        "error: Error = NoneError",
        "value = Err(NoneError)",
        "value = require(True, NoneError)",
        "kind = NoneError\nerror: Error = kind",
    ] {
        support::typecheck_with_rules(source, RuntimeSourcePolicy::script()).expect_err(source);
    }
}

#[test]
fn error_kind_diagnostics_guide_constructor_calls_without_calling_kind_variables() {
    for (source, constructor) in [
        ("value = Err(NoneError)", "`NoneError()`"),
        ("value = require(True, NoneError)", "`NoneError()`"),
        ("deferror Trouble(code: Int) { |code: Int| Self(message: \"trouble\", code: code) }\nvalue = Err(Trouble)", "`Trouble(...)`"),
    ] {
        let error = support::typecheck_with_rules(source, RuntimeSourcePolicy::script()).expect_err(source);
        let hint = error.hint.expect("ErrorKind mismatch has constructor guidance");
        assert!(hint.contains(constructor), "{source}: {hint}");
    }
    for source in [
        "kind = NoneError\nvalue = Err(kind)",
        "kind = NoneError\nerror: Error = kind",
    ] {
        let error =
            support::typecheck_with_rules(source, RuntimeSourcePolicy::script()).expect_err(source);
        let hint = error
            .hint
            .expect("ErrorKind variable mismatch has value guidance");
        assert!(hint.contains("ErrorKind"), "{source}: {hint}");
        assert!(
            !hint.contains("kind()"),
            "a kind value is not callable: {hint}"
        );
    }
}

#[test]
fn error_kind_operations_and_representation_stay_private() {
    for source in [
        "value = NoneError == NoneError",
        "value = to_string(NoneError)",
        "deftrait ObserveKind { def observe(self: Self) -> Int }\nimpl ObserveKind for ErrorKind { def observe(self: Self) -> Int { 1 } }",
        "impl ErrorKind { def same(self: Self) -> Self { self } }",
        "value = ErrorKind()",
        "factory = &ErrorKind",
        "kind = NoneError\nvalue = kind.code",
    ] {
        let result = support::resolve_with_builtin_prelude_result(source).map(support::typecheck);
        assert!(result.is_err() || result.unwrap().is_err(), "must reject: {source}");
    }
}
