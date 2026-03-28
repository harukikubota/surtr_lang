#[cfg(test)]
mod e2e {
    fn run_surtr(source: &str) -> Result<Vec<String>, String> {
        let ast = spire::parse(source).map_err(|e| format!("Parse: {}", e))?;
        let resolved = sigil::resolve(ast).map_err(|e| format!("Resolve: {}", e))?;
        let typed = scar::typecheck(resolved).map_err(|e| format!("Typecheck: {}", e))?;
        let bytecode = forge::codegen(typed).map_err(|e| format!("Codegen: {}", e))?;
        let mut vm = eldr::VM::new(bytecode).with_output_capture();
        vm.run().map_err(|e| format!("Runtime: {}", e))?;
        Ok(vm.output.unwrap_or_default())
    }

    fn assert_output(source: &str, expected: &[&str]) {
        let output = run_surtr(source).expect("Pipeline failed");
        assert_eq!(output, expected, "\nSource:\n{}\n", source);
    }

    fn assert_compile_error(source: &str, expected_substr: &str) {
        let result = run_surtr(source);
        match result {
            Err(msg) => assert!(msg.contains(expected_substr),
                "Expected error containing '{}', got: {}", expected_substr, msg),
            Ok(output) => panic!("Expected compile error, got output: {:?}", output),
        }
    }

    // ── Step 2: Script level ──

    #[test]
    fn step2_basic_bind_and_print() {
        assert_output(
            "num = 10\nnum2 = 5\nprint(to_string(num))",
            &["10"],
        );
    }

    #[test]
    fn step2_shadowing() {
        assert_output(
            "x = 10\nx = 20\nprint(to_string(x))",
            &["20"],
        );
    }

    // ── Step 3: Type checking ──

    #[test]
    fn step3_annotated_bind() {
        assert_output(
            "num: Int = 10\nname: String = \"hello\"\nprint(to_string(num))\nprint(name)",
            &["10", "hello"],
        );
    }

    #[test]
    fn step3_type_mismatch() {
        assert_compile_error(
            "bad: Int = \"not an int\"",
            "expected Int, got String",
        );
    }

    // ── Step 4: Primitive types ──

    #[test]
    fn step4_all_primitives() {
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
    fn step4_negative_int() {
        assert_output(
            "x = -5\nprint(to_string(x))",
            &["-5"],
        );
    }

    // ── Step 5: Arithmetic & operators ──

    #[test]
    fn step5_int_arithmetic() {
        assert_output(
            "print(to_string(10 + 5))\nprint(to_string(10 - 3))\nprint(to_string(4 * 3))\nprint(to_string(10 / 3))\nprint(to_string(10 % 3))",
            &["15", "7", "12", "3", "1"],
        );
    }

    #[test]
    fn step5_float_arithmetic() {
        assert_output(
            "print(to_string(1.5 + 2.5))\nprint(to_string(10.0 / 3.0))",
            &["4.0", "3.3333333333333335"],
        );
    }

    #[test]
    fn step5_comparison() {
        assert_output(
            "print(to_string(10 > 5))\nprint(to_string(10 < 5))\nprint(to_string(10 == 10))",
            &["True", "False", "True"],
        );
    }

    #[test]
    fn step5_string_eq() {
        assert_output(
            r#"print(to_string("abc" == "abc"))"#,
            &["True"],
        );
    }

    #[test]
    fn step5_bool_neq() {
        assert_output(
            "print(to_string(True != False))",
            &["True"],
        );
    }

    #[test]
    fn step5_string_concat() {
        assert_output(
            r#"print("hello" ++ " world")"#,
            &["hello world"],
        );
    }

    #[test]
    fn step5_precedence() {
        // 2 + 3 * 4 = 2 + 12 = 14
        assert_output(
            "print(to_string(2 + 3 * 4))",
            &["14"],
        );
    }

    #[test]
    fn step5_type_mismatch_eq() {
        assert_compile_error(
            "x = 1 == \"one\"",
            "Cannot compare",
        );
    }

    // ── Step 6: Lists ──

    #[test]
    fn step6_list_literal() {
        assert_output(
            "nums = [1, 2, 3]\nprint(to_string(nums))",
            &["[1, 2, 3]"],
        );
    }

    #[test]
    fn step6_string_list() {
        assert_output(
            r#"strs = ["a", "b", "c"]
print(to_string(strs))"#,
            &["[a, b, c]"],
        );
    }

    #[test]
    fn step6_empty_list() {
        assert_output(
            "empty: [Int] = []\nprint(to_string(empty))",
            &["[]"],
        );
    }

    #[test]
    fn step6_mixed_list_error() {
        assert_compile_error(
            r#"mixed = [1, "two"]"#,
            "expected Int, got String",
        );
    }

    // ── Step 7: defstruct / defrecord ──

    #[test]
    fn step7_struct_def_and_access() {
        assert_output(
            r#"defstruct User {
  name: String,
  age: Int,
}

user = User { name: "alice", age: 30 }
print(to_string(user))
print(to_string(user.name))
print(to_string(user.age))"#,
            &[
                "User { name: alice, age: 30 }",
                "alice",
                "30",
            ],
        );
    }

    #[test]
    fn step7_record_positional() {
        assert_output(
            r#"defrecord Point(x: Float, y: Float)
point = Point(1.0, 2.0)
print(to_string(point))
print(to_string(point.x))"#,
            &[
                "Point(x: 1.0, y: 2.0)",
                "1.0",
            ],
        );
    }

    #[test]
    fn step7_record_named_args() {
        assert_output(
            r#"defrecord Point(x: Float, y: Float)
point2 = Point(y: 5.0, x: 3.0)
print(to_string(point2.x))"#,
            &["3.0"],
        );
    }

    // ── Step 8: if / match ──

    #[test]
    fn step8_if_three_args() {
        assert_output(
            r#"flag = True
greeting = if(flag, "hello", "goodbye")
print(greeting)"#,
            &["hello"],
        );
    }

    #[test]
    fn step8_if_two_args() {
        assert_output(
            r#"flag = True
if(flag, print("flag is true"))"#,
            &["flag is true"],
        );
    }

    #[test]
    fn step8_match_bool() {
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
    fn step8_match_result() {
        assert_output(
            r#"result: Result<Int> = Ok(42)
match result {
  Ok(val)  => print(to_string(val)),
  Err(e)   => print("error"),
}"#,
            &["42"],
        );
    }

    // ── Step 9: deferror / Error ──

    #[test]
    fn step9_deferror_no_args() {
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
    fn step9_ok_case() {
        assert_output(
            r#"ok_val: Result<Int> = Ok(100)
match ok_val {
  Ok(val)  => print(to_string(val)),
  Err(e)   => print("error"),
}"#,
            &["100"],
        );
    }

    // ── Full phase 1 goal ──

    #[test]
    fn phase1_goal_combined() {
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
