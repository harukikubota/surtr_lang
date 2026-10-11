#[allow(dead_code)]
mod support;

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    support::typecheck_with_rules(source, sindr::policy::RuntimeSourcePolicy::script())
}

#[test]
fn special_enum_constructor_capture_uses_expected_owner_arguments() {
    for capture in [
        "&Ok",
        "&Result::Ok",
        "&Ok(&1)",
        "&Result::Ok(&1)",
        "&Result<Int>::Ok",
        "&Result<_>::Ok(&1)",
    ] {
        let source =
            format!("wrap: (Int -> Result<Int>) = {capture}\nvalue: Result<Int> = wrap(1)");
        check(&source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
    }
    for capture in ["&True", "&Boolean::True", "&False", "&Boolean::False"] {
        let source = format!("make: (-> Boolean) = {capture}\nflag: Boolean = make()");
        check(&source).unwrap_or_else(|error| panic!("{source}: {error:?}"));
    }
}

#[test]
fn special_enum_result_owner_arguments_constrain_payload_and_failure() {
    for constructor in ["Ok", "Result::Ok", "Result<Int>::Ok", "Result<_>::Ok"] {
        check(&format!("value: Result<Int> = {constructor}(1)")).expect("Int payload");
        check(&format!("value: Result<Int> = {constructor}(\"text\")"))
            .expect_err("payload mismatch");
    }
    for constructor in ["Err", "Result::Err", "Result<_>::Err"] {
        check(&format!(
            "err = {constructor}(NoneError())\nvalue: Result<Result<Int>> = {constructor}(NoneError())"
        ))
        .expect("failure success slot is polymorphic");
        check(&format!("value = {constructor}(1)")).expect_err("concrete Error required");
    }
}

#[test]
fn special_enum_result_capture_accepts_existing_errors() {
    for capture in ["&Err", "&Result<Int>::Err", "&Result<Int>::Err(&1)"] {
        check(&format!(
            "wrap: (Error -> Result<Int>) = {capture}\nvalue = wrap(NoneError())"
        ))
        .expect("ordinary callable transports Error");
    }
    check("deferror NumberError(value: Int) { |value: Int| Self(message: \"number\", value: value) }\nwrap: (Int -> Result<Int>) = &Result<Int>::Err(NumberError(&1))\nvalue = wrap(2)").expect("fixed concrete Error expression is allowed");
}

#[test]
fn special_enum_explicit_owner_conflicts_are_checked_without_annotation() {
    check("value = Result<Int>::Ok(\"text\")").expect_err("explicit owner constrains payload");
    check("value: Result<String> = Result<Int>::Err(NoneError())")
        .expect_err("explicit owner conflicts with expected result");
    check("def wrap(value: $T) -> Result<$T> { make: ($T -> Result<$T>) = &Result<$T>::Ok\nmake(value) }").expect("introduced type variable remains rigid");
}

#[test]
fn special_enum_pattern_requires_resolved_canonical_identity() {
    check("defenum Other { Ok(Int), Err(Int) }\na: Other = Other::Ok(1)\nvalue: Int = match a { Other::Ok(x) => x, Other::Err(x) => x }").expect("other owner stays a normal enum");
    for pattern in ["Other::Ok(x)", "Other::Err(x)"] {
        check(&format!("defenum Other {{ Ok(Int), Err(Int) }}\nvalue = match Ok(1) {{ {pattern} => 1, _ => 0 }}")).expect_err("same short name cannot match Result");
    }
    for (ok, err) in [("Ok", "Err"), ("Result::Ok", "Result::Err")] {
        check(&format!("value: Result<Result<Int>> = Ok(Err(NoneError()))\nanswer: Int = match value {{ {ok}({err}(_)) => 1, _ => 0 }}")).expect("nested failure pattern");
    }
}

#[test]
fn canonical_result_shape_rejects_missing_or_extra_success_slots() {
    for declaration in [
        "@builtin defenum Result { Ok(Int), Err(Error) }",
        "@builtin defenum Result<$T, $U> { Ok($T), Err(Error) }",
    ] {
        let result_source = include_str!("../../../lib/types/result.srt").replace(
            "@builtin\ndefenum Result<$T> {\n  Ok($T),\n  Err(Error),\n}",
            declaration,
        );
        let error = support::typecheck_std_modules_with_overrides(&[("Result", &result_source)])
            .expect_err("the canonical Result shape must be validated before slot registration");
        assert!(
            error.message.contains("Builtin Result enum must match"),
            "{error:?}"
        );
    }
}
