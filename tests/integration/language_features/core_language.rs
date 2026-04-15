use super::harness::{assert_compile_error, assert_output};

#[test]
fn bindings_basic_print() {
    assert_output("num = 10\nnum2 = 5\nprint(to_string(num))", &["10"]);
}

#[test]
fn bindings_shadowing_last_wins() {
    assert_output("x = 10\nx = 20\nprint(to_string(x))", &["20"]);
}

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
fn inspect_builtin_quotes_strings_and_preserves_error_rendering() {
    assert_output(
        r#"text = "hello"
pair = (1, "two", 3)
print(inspect(text))
print(inspect(pair))
print(inspect(Ok("value")))

deferror MyError {
  "error message."
}
print(inspect(Err(MyError)))"#,
        &[
            "\"hello\"",
            "(1, \"two\", 3)",
            "Ok(\"value\")",
            "Err(MyError(\"error message.\"))",
        ],
    );
}

#[test]
fn regex_generated_literal_and_builtin_wrappers_work_end_to_end() {
    assert_output(
        r#"rx =? re"(?<name>[A-Za-z]+)-(?<id>[0-9]+)"
caps =? Regex::captures(rx, "alice-42")
name =? RegexCaptures::get_name(caps, "name")
id =? RegexCaptures::get(caps, 2)
full = RegexCaptures::whole(caps)
count = RegexCaptures::capture_count(caps)
first =? Regex::find(rx, "alice-42")

print(name)
print(id)
print(full)
print(to_string(count))
print(RegexMatch::text(first))
print(to_string(Regex::is_match(rx, "bob-7")))
print(Regex::replace_all(rx, "alice-42 bob-7", "X"))"#,
        &["alice", "42", "alice-42", "3", "alice-42", "True", "X X"],
    );
}

#[test]
fn int_negative_literal() {
    assert_output("x = -5\nprint(to_string(x))", &["-5"]);
}

#[test]
fn arithmetic_int_ops() {
    assert_output(
        "print(to_string(10 + 5))\nprint(to_string(10 - 3))\nprint(to_string(4 * 3))\nprint(inspect(safe_div(10, 3)))\nprint(inspect(safe_mod(10, 3)))",
        &["15", "7", "12", "Ok(3)", "Ok(1)"],
    );
}

#[test]
fn arithmetic_float_ops() {
    assert_output(
        "print(to_string(1.5 + 2.5))\nprint(inspect(safe_div(10.0, 3.0)))",
        &["4.0", "Ok(3.3333333333333335)"],
    );
}

#[test]
fn safe_xxx_zero_returns_zero_division_error_display() {
    assert_output(
        "print(inspect(safe_div(1, 0)))\nprint(inspect(safe_mod(1, 0)))",
        &[
            "Err(ZeroDivisionError(\"division by zero\"))",
            "Err(ZeroDivisionError(\"division by zero\"))",
        ],
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
fn kernel_and_or_short_circuit() {
    assert_output(
        r#"def log_true(label: String) -> Boolean {
  print(label)
  True
}

def log_false(label: String) -> Boolean {
  print(label)
  False
}

print(to_string(and(True, log_false("and-rhs"))))
print(to_string(and(False, log_true("and-skip"))))
print(to_string(or(False, log_true("or-rhs"))))
print(to_string(or(True, log_false("or-skip"))))"#,
        &["and-rhs", "False", "False", "or-rhs", "True", "True"],
    );
}

#[test]
fn kernel_eq_neq_helpers_match_operator_behavior() {
    assert_output(
        r#"defenum Flag {
  On,
  Off,
}

print(to_string(eq(1, 1)))
print(to_string(neq("a", "b")))
print(to_string(eq(True, True)))
print(to_string(eq(Flag::On, Flag::On)))
print(to_string(neq(Flag::On, Flag::Off)))"#,
        &["True", "True", "True", "True", "True"],
    );
}

#[test]
fn kernel_ordering_and_concat_helpers_match_operator_behavior() {
    assert_output(
        r#"print(to_string(compare(1, 2)))
print(to_string(Compare::compare(1, 1)))
print(to_string(lt(1, 2)))
print(to_string(lte(2, 2)))
print(to_string(gt(3, 2)))
print(to_string(gte(3, 3)))
print(concat("hello", " world"))
print(to_string(lt(1.5, 2.0)))"#,
        &[
            "Ordering::Less",
            "Ordering::Equal",
            "True",
            "True",
            "True",
            "True",
            "hello world",
            "True",
        ],
    );
}

#[test]
fn concat_strings() {
    assert_output(r#"print("hello" ++ " world")"#, &["hello world"]);
}

#[test]
fn arithmetic_precedence() {
    assert_output("print(to_string(2 + 3 * 4))", &["20"]);
}

#[test]
fn equality_reject_mixed_types() {
    assert_compile_error("x = 1 == \"one\"", "Cannot compare");
}

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
    assert_output("empty: List<Int> = []\nprint(to_string(empty))", &["[]"]);
}

#[test]
fn list_cons_expr() {
    assert_output(
        "tail: List<Int> = [2, 3]\nnums = [1, ..tail]\nprint(to_string(nums))",
        &["[1, 2, 3]"],
    );
}

#[test]
fn list_reject_mixed_types() {
    assert_compile_error(r#"mixed = [1, "two"]"#, "expected Int, got String");
}

#[test]
fn list_cons_rejects_non_list_tail() {
    assert_compile_error("nums = [1, ..2]", "list tail must be List<...>");
}

#[test]
fn closure_literal_invocation() {
    assert_output(
        r#"add1: (Int -> Int) = {|x| x + 1}
print(to_string(add1(2)))"#,
        &["3"],
    );
}

#[test]
fn closure_argument_type_infers_from_add_constraint() {
    assert_output(
        r#"x = 10
fun = {|num| x = x + 5;x+num}
print(to_string(fun(3)))"#,
        &["18"],
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
fn func_literal_infix_invocation_works() {
    assert_output(
        r#"def eq(left: Int, right: Int) -> Boolean {
  left == right
}

print(to_string(10 `+` 5))
print(to_string(7 `eq` 7))"#,
        &["15", "True"],
    );
}

#[test]
fn expr_class_operators_are_same_precedence() {
    assert_output(
        r#"print(to_string(2 + 3 * 4))
print(to_string(2 `*` 3 + 4))"#,
        &["20", "10"],
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

#[test]
fn function_forward_reference_succeeds() {
    assert_output(
        r#"print(to_string(double(21)))

def double(x: Int) -> Int { x * 2 }"#,
        &["42"],
    );
}

#[test]
fn struct_definition_and_field_access() {
    assert_output(
        r#"defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
}

user = User("alice", 30)
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

#[test]
fn struct_record_forward_references_and_type_annotation_succeed() {
    assert_output(
        r#"user: User = make_user("alice")
print(to_string(user.age))

point = Point(y: 9.5, x: 3.0)
print(to_string(point.x))

def make_user(name: String) -> User {
  User(name, 30)
}

defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
}

defrecord Point(x: Float, y: Float)"#,
        &["30", "3.0"],
    );
}

#[test]
fn struct_property_update_via_associated_functions() {
    assert_output(
        r#"defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }

  def with_age(self: Self, age: Int) -> Self {
    User { name: self.name, age: age }
  }

  def with_name(self, name: String) -> Self {
    User { name: name, age: self.age }
  }
}

original = User("alice", 30)
aged = User::with_age(original, 31)
renamed = User::with_name(aged, "bob")

print(to_string(original.age))
print(to_string(aged.age))
print(to_string(renamed.name))
print(to_string(renamed.age))"#,
        &["30", "31", "bob", "31"],
    );
}

#[test]
fn struct_constructor_sugar_mixed_named_positional_error() {
    assert_compile_error(
        r#"defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
}

user = User("alice", age: 30)"#,
        "Cannot mix positional and named arguments",
    );
}

#[test]
fn impl_method_call_mixed_named_positional_error() {
    assert_compile_error(
        r#"defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }

  def with_name_and_age(self, name: String, age: Int) -> Self {
    User { name: name, age: age }
  }
}

user = User("alice", 30)
updated = User::with_name_and_age(user, "bob", age: 31)"#,
        "Cannot mix positional and named arguments",
    );
}

#[test]
fn enum_state_transition_via_associated_functions() {
    assert_output(
        r#"defenum Light {
  Red,
  Yellow,
  Green,
}

impl Light {
  def next(self) -> Self {
    match self {
      Light::Red => Light::Green,
      Light::Green => Light::Yellow,
      Light::Yellow => Light::Red,
    }
  }

  def advance(self: Self, steps: Int) -> Self {
    if(steps == 0, self, Light::advance(Light::next(self), steps - 1))
  }

  def rebound_once(self) -> Self {
    self = Light::next(self)
    self
  }

  def is_stop(self) -> Boolean {
    match self {
      Light::Red => True,
      _ => False,
    }
  }
}

initial = Light::Red
once = Light::next(initial)
twice = Light::advance(initial, 2)
rebound = Light::rebound_once(Light::Yellow)

print(to_string(Light::is_stop(initial)))
print(to_string(Light::is_stop(once)))
print(to_string(Light::is_stop(twice)))
print(to_string(Light::is_stop(rebound)))"#,
        &["True", "False", "False", "True"],
    );
}

#[test]
fn enum_impl_method_call_mixed_named_positional_error() {
    assert_compile_error(
        r#"defenum Light {
  Red,
  Yellow,
  Green,
}

impl Light {
  def with_steps(self: Self, steps: Int) -> Self {
    self
  }
}

light = Light::Green
bad = Light::with_steps(light, steps: 1)"#,
        "Cannot mix positional and named arguments",
    );
}

#[test]
fn enum_self_rebinding_requires_self_type() {
    assert_compile_error(
        r#"defenum Light {
  Red,
  Green,
}

impl Light {
  def bad(self) -> Self {
    self = 1
    self
  }
}"#,
        "`self` rebinding requires Self type",
    );
}

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
    assert_compile_error(
        r#"def add3(x: Int, y: Int, z: Int) -> Int { x + y + z }
print(to_string(add3(1, z: 3, y: 2)))"#,
        "Cannot mix positional and named arguments",
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
        "Cannot mix positional and named arguments",
    );
}

#[test]
fn function_named_args_positional_after_named_error() {
    assert_compile_error(
        r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(add(y: 2, 1)))"#,
        "Cannot mix positional and named arguments",
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
fn match_list_patterns() {
    assert_output(
        r#"nums: List<Int> = [1, 2, 3]
print(match nums {
  [] => "empty",
  [head, ..tail] => to_string(head),
})"#,
        &["1"],
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

#[test]
fn cond_selects_first_true_branch_and_skips_later_branches() {
    assert_output(
        r#"print(to_string(cond {
  False => 0,
  1 < 2 => 1,
  True => 2,
}))"#,
        &["1"],
    );
}

#[test]
fn cond_allows_block_bodies() {
    assert_output(
        r#"print(to_string(cond {
  False => 0,
  True => { print("branch"); 42 },
}))"#,
        &["branch", "42"],
    );
}

#[test]
fn cond_condition_must_be_boolean() {
    assert_compile_error(
        r#"print(to_string(cond {
  1 => 10,
  True => 20,
}))"#,
        "if condition must be Boolean, got Int",
    );
}

#[test]
fn cond_branch_types_must_match() {
    assert_compile_error(
        r#"print(to_string(cond {
  False => 1,
  True => "x",
}))"#,
        "if branches have different types: Int and String",
    );
}

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
