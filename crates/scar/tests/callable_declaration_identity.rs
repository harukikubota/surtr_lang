#[allow(dead_code)]
mod support;

fn check(source: &str) -> Result<Vec<scar::typed::TypedNode>, scar::error::TypeError> {
    support::typecheck(support::resolve_with_builtin_prelude(source))
}

#[test]
fn ordinary_curry_uses_its_declared_signature() {
    for source in [
        "def curry(fun: (Int, Int -> Int)) -> Int { 999 }\ndef add(a: Int, b: Int) -> Int { a + b }\nvalue: Int = curry(&add)",
        "def curry(value: Int) -> Int { value }\nvalue = curry(999)",
        "curry = {|value: Int| value}\nvalue: Int = curry(999)",
        "def curry(value: Int) -> Int { value }\nf = &curry\nvalue: Int = f(999)",
        "def curry(value: Int) -> Int { value }\nvalue: Int = 999 |> curry()",
        "def curry::<$A>(value: Int) -> ($A -> Int) { {|_| value} }\nf = curry::<String>(999)\nvalue: Int = f(\"text\")",
    ] {
        check(source).unwrap_or_else(|err| panic!("{source}: {err:?}"));
    }
}

#[test]
fn standard_curry_checks_its_contract() {
    for source in [
        "def add(a: Int, b: Int) -> Int { a + b }\nf = curry(&add)\ng = f(1)\nvalue: Int = g(2)",
        "def add(a: Int, b: Int) -> Int { a + b }\nf: (Int -> (Int -> Int)) = Function::curry(&add)",
        "def add(a: Int, b: Int) -> Int { a + b }\nf = Function::curry::<(Int -> (Int -> Int))>(&add)",
        "def add(a: Int, b: Int) -> Int { a + b }\nf = &add |> curry()",
        "f: ((Int, Int -> Int) -> (Int -> (Int -> Int))) = &curry(&1)",
    ] {
        check(source).unwrap_or_else(|err| panic!("{source}: {err:?}"));
    }
    for (source, message) in [
        (
            "f: ((Int, Int -> Int) -> (Int -> (Int -> Int))) = &Function::curry",
            "cannot be captured",
        ),
        (
            "f: (String -> (String -> String)) = Function::curry::<(Int -> (Int -> Int))>({|a: Int, b: Int| a + b})",
            "type mismatch",
        ),
        (
            "Function::curry::<Int>({|a: Int, b: Int| a + b})",
            "type mismatch",
        ),
        (
            "Function::curry::<Int, String>({|a: Int, b: Int| a + b})",
            "argument",
        ),
        (
            "curry({|a: Int, b: Int| a + b}, 1)",
            "exactly one positional callable",
        ),
        ("curry()", "exactly one positional callable"),
        ("curry(1)", "requires a function value"),
        ("curry({|x: Int| x})", "at least two arguments"),
    ] {
        let err = check(source).expect_err(source);
        assert!(err.message.contains(message), "{source}: {err:?}");
    }
}

#[test]
fn standard_curry_identity_survives_reference_aliases() {
    let mut resolved = support::resolve_with_builtin_prelude("curry({|a: Int, b: Int| a + b})");
    let sigil::resolved::Resolved::App(_, target, _) = resolved.last_mut().unwrap() else {
        panic!("expected call");
    };
    let sigil::resolved::Resolved::Var(_, id) = target.as_mut() else {
        panic!("expected declaration reference");
    };
    id.name = "renamed_curry".into();
    id.qualified_name = Some("Alias::renamed_curry".into());
    let typed = support::typecheck(resolved).expect("identity remains standard curry");
    assert!(matches!(
        typed.last().unwrap().node,
        scar::typed::TypedInner::Closure(..)
    ));
}

#[test]
fn ordinary_curry_return_values_remain_non_callable() {
    for source in [
        "def curry(fun: (Int, Int -> Int)) -> Int { 999 }\ndef add(a: Int, b: Int) -> Int { a + b }\nvalue = curry(&add)\nvalue(2)",
        "curry = 999\ncurry(2)",
        "def curry(value: Int) -> Int { value }\ncurry::<Int>(999)",
    ] {
        let err = check(source).expect_err(source);
        assert!(!err.message.contains("Function::curry"), "{source}: {err:?}");
    }
}
