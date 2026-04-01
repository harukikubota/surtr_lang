#[cfg(test)]
mod e2e {
    const BUILTIN_PRELUDE_SOURCE: &str = include_str!("../../lib/builtin.srt");

    fn parse_with_builtin_prelude(source: &str) -> Result<Vec<spire::ast::Ast>, String> {
        let mut ast = spire::parse(BUILTIN_PRELUDE_SOURCE)
            .map_err(|e| format!("Parse (builtin.srt): {}", e))?;
        let mut user_ast = spire::parse(source).map_err(|e| format!("Parse: {}", e))?;
        ast.append(&mut user_ast);
        Ok(ast)
    }

    fn run_surtr(source: &str) -> Result<Vec<String>, String> {
        let ast = parse_with_builtin_prelude(source)?;
        let resolved = sigil::resolve(ast).map_err(|e| format!("Resolve: {}", e))?;
        let typed = scar::typecheck(resolved).map_err(|e| format!("Typecheck: {}", e))?;
        let bytecode = forge::codegen(typed).map_err(|e| format!("Codegen: {}", e))?;
        let mut vm = eldr::VM::new(bytecode).with_output_capture();
        vm.run().map_err(|e| format!("Runtime: {}", e))?;
        Ok(vm.output.unwrap_or_default())
    }

    fn run_surtr_with_stderr(source: &str) -> Result<(Vec<String>, Vec<String>), String> {
        let ast = parse_with_builtin_prelude(source)?;
        let resolved = sigil::resolve(ast).map_err(|e| format!("Resolve: {}", e))?;
        let typed = scar::typecheck(resolved).map_err(|e| format!("Typecheck: {}", e))?;
        let bytecode = forge::codegen(typed).map_err(|e| format!("Codegen: {}", e))?;
        let mut vm = eldr::VM::new(bytecode)
            .with_output_capture()
            .with_error_capture();
        vm.run().map_err(|e| format!("Runtime: {}", e))?;
        Ok((
            vm.output.unwrap_or_default(),
            vm.error_output.unwrap_or_default(),
        ))
    }

    fn assert_output(source: &str, expected: &[&str]) {
        let output = run_surtr(source).expect("Pipeline failed");
        assert_eq!(output, expected, "\nSource:\n{}\n", source);
    }

    fn assert_compile_error(source: &str, expected_substr: &str) {
        let result = run_surtr(source);
        match result {
            Err(msg) => assert!(
                msg.contains(expected_substr),
                "Expected error containing '{}', got: {}",
                expected_substr,
                msg
            ),
            Ok(output) => panic!("Expected compile error, got output: {:?}", output),
        }
    }

    // Bindings

    #[test]
    fn bindings_basic_print() {
        assert_output("num = 10\nnum2 = 5\nprint(to_string(num))", &["10"]);
    }

    #[test]
    fn bindings_shadowing_last_wins() {
        assert_output("x = 10\nx = 20\nprint(to_string(x))", &["20"]);
    }

    // Type annotations

    #[test]
    fn annotations_accept_matching_types() {
        assert_output(
            "num: Int = 10\nname: String = \"hello\"\nprint(to_string(num))\nprint(name)",
            &["10", "hello"],
        );
    }

    #[test]
    fn annotations_reject_type_mismatch() {
        assert_compile_error("bad: Int = \"not an int\"", "expected Int, got String");
    }

    // Primitive values

    #[test]
    fn primitives_render_to_string() {
        assert_output(
            r#"int_val = 42
float_val = 3.14
str_val = "hello"
str_sq = 'single'
flag = True
unit_val = ()
print(to_string(int_val))
print(to_string(float_val))
print(str_val)
print(str_sq)
print(to_string(flag))
print(to_string(unit_val))"#,
            &["42", "3.14", "hello", "single", "True", "()"],
        );
    }

    #[test]
    fn inspect_builtin_renders_like_current_display() {
        assert_output(
            r#"nums = [1, 2, 3]
print(inspect(nums))
print(inspect(Ok(10)))

deferror MyError {
  "error message."
}
print(inspect(Err(MyError)))"#,
            &["[1, 2, 3]", "Ok(10)", "Err(MyError(\"error message.\"))"],
        );
    }

    #[test]
    fn int_negative_literal() {
        assert_output("x = -5\nprint(to_string(x))", &["-5"]);
    }

    // Operators

    #[test]
    fn arithmetic_int_ops() {
        assert_output(
            "print(to_string(10 + 5))\nprint(to_string(10 - 3))\nprint(to_string(4 * 3))\nprint(to_string(10 / 3))\nprint(to_string(10 % 3))",
            &["15", "7", "12", "3", "1"],
        );
    }

    #[test]
    fn arithmetic_float_ops() {
        assert_output(
            "print(to_string(1.5 + 2.5))\nprint(to_string(10.0 / 3.0))",
            &["4.0", "3.3333333333333335"],
        );
    }

    #[test]
    fn comparison_int_ops() {
        assert_output(
            "print(to_string(10 > 5))\nprint(to_string(10 < 5))\nprint(to_string(10 == 10))",
            &["True", "False", "True"],
        );
    }

    #[test]
    fn equality_string() {
        assert_output(r#"print(to_string("abc" == "abc"))"#, &["True"]);
    }

    #[test]
    fn inequality_boolean() {
        assert_output("print(to_string(True != False))", &["True"]);
    }

    #[test]
    fn concat_strings() {
        assert_output(r#"print("hello" ++ " world")"#, &["hello world"]);
    }

    #[test]
    fn arithmetic_precedence() {
        // 2 + 3 * 4 = 2 + 12 = 14
        assert_output("print(to_string(2 + 3 * 4))", &["14"]);
    }

    #[test]
    fn equality_reject_mixed_types() {
        assert_compile_error("x = 1 == \"one\"", "Cannot compare");
    }

    // Lists

    #[test]
    fn list_literal_int() {
        assert_output("nums = [1, 2, 3]\nprint(to_string(nums))", &["[1, 2, 3]"]);
    }

    #[test]
    fn list_literal_string() {
        assert_output(
            r#"strs = ["a", "b", "c"]
print(to_string(strs))"#,
            &["[a, b, c]"],
        );
    }

    #[test]
    fn list_empty_with_annotation() {
        assert_output("empty: [Int] = []\nprint(to_string(empty))", &["[]"]);
    }

    #[test]
    fn list_reject_mixed_types() {
        assert_compile_error(r#"mixed = [1, "two"]"#, "expected Int, got String");
    }

    // Closures and partial application

    #[test]
    fn closure_literal_invocation() {
        assert_output(
            r#"add1: (Int -> Int) = {|x| x + 1}
print(to_string(add1(2)))"#,
            &["3"],
        );
    }

    #[test]
    fn closure_builtin_capture() {
        assert_output(
            r#"printer = &print
printer("hello")"#,
            &["hello"],
        );
    }

    #[test]
    fn function_partial_application_composition() {
        assert_output(
            r#"def inc(x: Int) -> Int { x + 1 }
def times2(x: Int) -> Int { x * 2 }
def compose(f: (Int -> Int), g: (Int -> Int), x: Int) -> Int {
  g(f(x))
}

apply_inc = &compose(&inc)
print(to_string(apply_inc(&times2, 10)))"#,
            &["22"],
        );
    }

    #[test]
    fn function_partial_application_type_error() {
        assert_compile_error(
            r#"def inc(x: Int) -> Int { x + 1 }
def compose(f: (Int -> Int), g: (Int -> Int), x: Int) -> Int {
  g(f(x))
}

bad = &compose(inc(1))"#,
            "expected (Int -> Int), got Int",
        );
    }

    // Structs and records

    #[test]
    fn struct_definition_and_field_access() {
        assert_output(
            r#"defstruct User {
  name: String,
  age: Int,
}

user = User { name: "alice", age: 30 }
print(to_string(user))
print(to_string(user.name))
print(to_string(user.age))"#,
            &["User { name: alice, age: 30 }", "alice", "30"],
        );
    }

    #[test]
    fn record_constructor_positional() {
        assert_output(
            r#"defrecord Point(x: Float, y: Float)
point = Point(1.0, 2.0)
print(to_string(point))
print(to_string(point.x))"#,
            &["Point(x: 1.0, y: 2.0)", "1.0"],
        );
    }

    #[test]
    fn record_constructor_named_args() {
        assert_output(
            r#"defrecord Point(x: Float, y: Float)
point2 = Point(y: 5.0, x: 3.0)
print(to_string(point2.x))"#,
            &["3.0"],
        );
    }

    // Function call named arguments

    #[test]
    fn function_named_args_reordered() {
        assert_output(
            r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(add(y: 2, x: 1)))"#,
            &["3"],
        );
    }

    #[test]
    fn function_named_args_mixed_with_positional_first() {
        assert_output(
            r#"def add3(x: Int, y: Int, z: Int) -> Int { x + y + z }
print(to_string(add3(1, z: 3, y: 2)))"#,
            &["6"],
        );
    }

    #[test]
    fn function_named_args_unknown_name_error() {
        assert_compile_error(
            r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(add(z: 1, y: 2)))"#,
            "Unknown argument name 'z'",
        );
    }

    #[test]
    fn function_named_args_duplicate_error() {
        assert_compile_error(
            r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(add(1, x: 2)))"#,
            "Duplicate argument 'x'",
        );
    }

    #[test]
    fn function_named_args_positional_after_named_error() {
        assert_compile_error(
            r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(add(y: 2, 1)))"#,
            "Positional arguments must come before named arguments",
        );
    }

    #[test]
    fn function_duplicate_name_is_compile_error() {
        assert_compile_error(
            r#"def f() -> Int { 1 }
def f() -> Int { 2 }"#,
            "Duplicate top-level definition: f",
        );
    }

    #[test]
    fn top_level_name_collision_between_struct_and_def_is_compile_error() {
        assert_compile_error(
            r#"defstruct User {
  name: String,
}
def User() -> Int { 1 }"#,
            "Duplicate top-level definition: User",
        );
    }

    // If and match

    #[test]
    fn if_expression_with_else() {
        assert_output(
            r#"flag = True
greeting = if(flag, "hello", "goodbye")
print(greeting)"#,
            &["hello"],
        );
    }

    #[test]
    fn if_expression_without_else_returns_unit() {
        assert_output(
            r#"flag = True
if_then(flag, print("flag is true"))"#,
            &["flag is true"],
        );
    }

    #[test]
    fn match_boolean_exhaustive() {
        assert_output(
            r#"flag = True
print(to_string(match flag {
  True  => "yes",
  False => "no",
}))"#,
            &["yes"],
        );
    }

    #[test]
    fn match_result_exhaustive() {
        assert_output(
            r#"result: Result<Int> = Ok(42)
match result {
  Ok(val)  => print(to_string(val)),
  Err(e)   => print("error"),
}"#,
            &["42"],
        );
    }

    // Match extensions

    #[test]
    fn match_boolean_wildcard_arm() {
        assert_output(
            r#"flag = True
print(match flag {
  True => "hit",
  _ => "miss",
})"#,
            &["hit"],
        );
    }

    #[test]
    fn match_int_literal_patterns() {
        assert_output(
            r#"n = 2
print(match n {
  1 => "one",
  2 => "two",
  _ => "other",
})"#,
            &["two"],
        );
    }

    #[test]
    fn match_string_literal_patterns() {
        assert_output(
            r#"s = "b"
print(match s {
  "a" => "A",
  "b" => "B",
  _ => "?",
})"#,
            &["B"],
        );
    }

    #[test]
    fn match_boolean_non_exhaustive_error() {
        assert_compile_error(
            r#"flag = True
print(match flag {
  True => "yes",
})"#,
            "Non-exhaustive match. Missing: False",
        );
    }

    #[test]
    fn match_result_non_exhaustive_error() {
        assert_compile_error(
            r#"r: Result<Int> = Ok(1)
print(match r {
  Ok(v) => to_string(v),
})"#,
            "Non-exhaustive match. Missing: Err",
        );
    }

    #[test]
    fn match_int_non_exhaustive_error() {
        assert_compile_error(
            r#"n = 1
print(match n {
  1 => "one",
})"#,
            "Non-exhaustive match. Missing: _",
        );
    }

    // String interpolation

    #[test]
    fn string_interpolation_basic() {
        assert_output(
            r#"name = "alice"
score = 10
print("hello #{name}")
print("score=#{score + 2}")"#,
            &["hello alice", "score=12"],
        );
    }

    #[test]
    fn string_interpolation_result_type_error() {
        assert_compile_error(
            r#"r: Result<Int> = Ok(1)
print("r=#{r}")"#,
            "Interpolation does not allow Result type",
        );
    }

    // Function definitions

    #[test]
    fn function_definition_minimal() {
        assert_output(
            r#"def noop() {()}
def const() -> Int { 1 }
def do_something(num: Int) -> Unit { () }
def add_two(num: Int) -> Int { num + 2 }
def add(x: Int, y: Int) -> Int { x + y }"#,
            &[],
        );
    }

    #[test]
    fn function_call_locals_are_isolated() {
        assert_output(
            r#"def outer(x: Int, y: Int) -> Int {
    x = x + 10
    y = y + 100
    ret = inner(x, y)
    print(to_string(x))
    ret
}

def inner(x: Int, y: Int) -> Int {
    x + y
}

print(to_string(outer(1, 2)))"#,
            &["11", "113"],
        );
    }

    #[test]
    fn function_call_missing_return_reports_unit_hint() {
        assert_compile_error(
            r#"def outer(x: Int, y: Int) -> Int {
    x = x + 10
    y = y + 100
    ret = inner(x, y)
    print(to_string(x))
}

def inner(x: Int, y: Int) -> Int {
    x + y
}

ret = outer(2, 4)
print(to_string(ret))"#,
            "print: (String) -> Unit",
        );
    }

    #[test]
    fn function_zero_arg_call() {
        assert_output(
            r#"def sf() -> Result<String> {
  str = "hoge"
  str2 =? Ok(str)
  Ok(str2)
}

ret: Result<String> = sf()
match ret {
  Ok(str) => print("ok"),
  Err(e) => print("ng"),
}"#,
            &["ok"],
        );
    }

    // Safe bind (`=?`)

    #[test]
    fn safebind_top_level_ok() {
        assert_output(
            r#"value: Result<Int> = Ok(5)
num =? value
print(to_string(num + 1))"#,
            &["6"],
        );
    }

    #[test]
    fn safebind_function_early_return_on_err() {
        let (stdout, stderr) = run_surtr_with_stderr(
            r#"deferror Oops {
  "oops"
}

def gen(flag: Boolean) -> Result<Int> {
  if(flag, Ok(10), Err(Oops))
}

def fun(flag: Boolean) -> Result<Int> {
  num =? gen(flag)
  Ok(num + 10)
}

ok: Result<Int> = fun(True)
match ok {
  Ok(v) => print(to_string(v)),
  Err(e) => print("bad"),
}

err: Result<Int> = fun(False)
match err {
  Ok(v) => print("bad"),
  Err(e) => eprint(e),
}"#,
        )
        .expect("Pipeline failed");
        assert_eq!(stdout, vec!["20"]);
        assert_eq!(stderr, vec!["Error: Oops: oops"]);
    }

    #[test]
    fn safebind_script_error_eprints() {
        let (stdout, stderr) = run_surtr_with_stderr(
            r#"deferror Oops {
  "oops"
}

value: Result<Int> = Err(Oops)
num =? value
print("after")"#,
        )
        .expect("Pipeline failed");
        assert_eq!(stdout, Vec::<String>::new());
        assert_eq!(stderr, vec!["Error: Oops: oops"]);
    }

    #[test]
    fn safebind_requires_result_rhs() {
        assert_compile_error("num =? 10", "`=?` requires Result");
    }

    #[test]
    fn safebind_requires_result_return_function() {
        assert_compile_error(
            r#"def bad() -> Int {
  num =? Ok(1)
  num
}"#,
            "can only be used in functions returning Result",
        );
    }

    #[test]
    fn assignment_operators_non_associative() {
        assert_compile_error("x = y =? z", "non-associative");
    }

    #[test]
    fn plain_bind_rejects_result_test_pattern() {
        assert_compile_error("Ok(num) = Ok(1)", "Unexpected token: Bind");
    }

    // Errors

    #[test]
    fn deferror_no_args_basic() {
        // deferror defines an error type, Err wraps it as Tagged{tag:1}
        // eprint outputs to stderr which we capture separately
        let source = r#"deferror ValidationError {
  "Validation failed"
}

err1: Result<Int> = Err(ValidationError)
match err1 {
  Ok(val)  => print("ok"),
  Err(e)   => print("got error"),
}"#;
        assert_output(source, &["got error"]);
    }

    #[test]
    fn deferror_interpolated_message_display() {
        let (stdout, stderr) = run_surtr_with_stderr(
            r#"deferror PageNotFound(html: String) {
  "Page Not Found. #{html}"
}

err_result: Result<Int> = Err(PageNotFound("404"))
match err_result {
  Ok(num) => print(to_string(num)),
  Err(e)  => eprint(e),
}"#,
        )
        .expect("Pipeline failed");
        assert_eq!(stdout, Vec::<String>::new());
        assert_eq!(stderr, vec!["Error: PageNotFound: Page Not Found. 404"]);
    }

    #[test]
    fn match_err_eprint_with_wildcard_arm() {
        let (stdout, stderr) = run_surtr_with_stderr(
            r#"deferror MyE {
  "hoge"
}

ret: Result<Int> = Err(MyE)
match ret {
  Err(e) => eprint(e),
  _ => print("")
}"#,
        )
        .expect("Pipeline failed");
        assert_eq!(stdout, Vec::<String>::new());
        assert_eq!(stderr, vec!["Error: MyE: hoge"]);
    }

    #[test]
    fn deferror_rejects_raw_error_binding() {
        assert_compile_error(
            r#"deferror PageNotFound(html: String) {
  "Page Not Found. #{html}"
}

bad = PageNotFound("404")"#,
            "Error values must be wrapped with Err(...)",
        );
    }

    #[test]
    fn result_ok_case_prints_value() {
        assert_output(
            r#"ok_val: Result<Int> = Ok(100)
match ok_val {
  Ok(val)  => print(to_string(val)),
  Err(e)   => print("error"),
}"#,
            &["100"],
        );
    }

    // Language goal

    #[test]
    fn language_goal_combined() {
        assert_output(
            r#"num = 10
num2 = 5
typed_num: Int = 42
float_val = 3.14
str_val = "hello"
flag = True
unit_val = ()
print(to_string(num + num2))
print(to_string(10 > 5))
print(to_string("abc" == "abc"))
print("hello" ++ " world")
nums: [Int] = [1, 2, 3]
print(to_string(nums))
empty: [Int] = []
print(to_string(empty))
defstruct User {
  name: String,
  age: Int,
}
user = User { name: "alice", age: 30 }
print(to_string(user))
print(to_string(user.name))
defrecord Pair(first: Int, second: String)
pair = Pair(1, "hello")
print(to_string(pair))
print(to_string(pair.first))
greeting = if(flag, "hello", "goodbye")
print(greeting)
match flag {
  True  => print("flag is true"),
  False => print("flag is false"),
}
result: Result<Int> = Ok(42)
match result {
  Ok(val)  => print(to_string(val)),
  Err(e)   => print("error"),
}
msg = "hello" ++ " world"
print(msg)"#,
            &[
                "15",
                "True",
                "True",
                "hello world",
                "[1, 2, 3]",
                "[]",
                "User { name: alice, age: 30 }",
                "alice",
                "Pair(first: 1, second: hello)",
                "1",
                "hello",
                "flag is true",
                "42",
                "hello world",
            ],
        );
    }
}
