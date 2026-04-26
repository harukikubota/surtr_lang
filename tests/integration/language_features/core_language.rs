use super::harness::{assert_compile_error, assert_output};

fn bindings_basic_print() {
    assert_output("num = 10\nnum2 = 5\nprint(to_string(num))", &["10"]);
}

fn bindings_shadowing_last_wins() {
    assert_output("x = 10\nx = 20\nprint(to_string(x))", &["20"]);
}

fn annotations_accept_matching_types() {
    assert_output(
        "num: Int = 10\nname: String = \"hello\"\nprint(to_string(num))\nprint(name)",
        &["10", "hello"],
    );
}

fn annotations_reject_type_mismatch() {
    assert_compile_error("bad: Int = \"not an int\"", "expected Int, got String");
}

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

fn int_negative_literal() {
    assert_output("x = -5\nprint(to_string(x))", &["-5"]);
}

fn arithmetic_int_ops() {
    assert_output(
        "print(to_string(10 + 5))\nprint(to_string(10 - 3))\nprint(to_string(4 * 3))\nprint(inspect(safe_div(10, 3)))\nprint(inspect(safe_mod(10, 3)))",
        &["15", "7", "12", "Ok(3)", "Ok(1)"],
    );
}

fn arithmetic_float_ops() {
    assert_output(
        "print(to_string(1.5 + 2.5))\nprint(inspect(safe_div(10.0, 3.0)))",
        &["4.0", "Ok(3.3333333333333335)"],
    );
}

fn safe_xxx_zero_returns_zero_division_error_display() {
    assert_output(
        "print(inspect(safe_div(1, 0)))\nprint(inspect(safe_mod(1, 0)))",
        &[
            "Err(ZeroDivisionError(\"division by zero\"))",
            "Err(ZeroDivisionError(\"division by zero\"))",
        ],
    );
}

fn comparison_int_ops() {
    assert_output(
        "print(to_string(10 > 5))\nprint(to_string(10 < 5))\nprint(to_string(10 == 10))",
        &["True", "False", "True"],
    );
}

fn equality_string() {
    assert_output(r#"print(to_string("abc" == "abc"))"#, &["True"]);
}

fn inequality_boolean() {
    assert_output("print(to_string(True != False))", &["True"]);
}

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

fn concat_strings() {
    assert_output(r#"print("hello" ++ " world")"#, &["hello world"]);
}

fn arithmetic_precedence() {
    assert_output("print(to_string(2 + 3 * 4))", &["20"]);
}

fn equality_reject_mixed_types() {
    assert_compile_error("x = 1 == \"one\"", "Cannot compare");
}

fn list_literal_int() {
    assert_output("nums = [1, 2, 3]\nprint(to_string(nums))", &["[1, 2, 3]"]);
}

fn list_literal_string() {
    assert_output(
        r#"strs = ["a", "b", "c"]
print(to_string(strs))"#,
        &["[a, b, c]"],
    );
}

fn list_empty_with_annotation() {
    assert_output("empty: List<Int> = []\nprint(to_string(empty))", &["[]"]);
}

fn list_cons_expr() {
    assert_output(
        "tail: List<Int> = [2, 3]\nnums = [1, ..tail]\nprint(to_string(nums))",
        &["[1, 2, 3]"],
    );
}

fn list_reject_mixed_types() {
    assert_compile_error(r#"mixed = [1, "two"]"#, "expected Int, got String");
}

fn list_cons_rejects_non_list_tail() {
    assert_compile_error("nums = [1, ..2]", "list tail must be List<...>");
}

fn closure_literal_invocation() {
    assert_output(
        r#"add1: (Int -> Int) = {|x| x + 1}
print(to_string(add1(2)))"#,
        &["3"],
    );
}

fn closure_argument_type_infers_from_add_constraint() {
    assert_output(
        r#"x = 10
fun = {|num| x = x + 5;x+num}
print(to_string(fun(3)))"#,
        &["18"],
    );
}

fn closure_builtin_capture() {
    assert_output(
        r#"printer = &print
printer("hello")"#,
        &["hello"],
    );
}

fn const_helper_and_hole_return_surface_work() {
    assert_output(
        r#"always: (_ -> Int) = const(1)
print(to_string(always("ignored")))

print(to_string(id("ok")))

def make() -> (_ -> Int) {
  const(2)
}

next = make()
print(to_string(next(False)))

ten = {|_| 10}
print(to_string(ten([1, 2, 3])))

idle: (-> Unit) = noop()
print(inspect(idle()))
idle2 = noop()
print(inspect(idle2()))"#,
        &["1", "ok", "2", "10", "()", "()"],
    );
}

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

fn expr_class_operators_are_same_precedence() {
    assert_output(
        r#"print(to_string(2 + 3 * 4))
print(to_string(2 `*` 3 + 4))"#,
        &["20", "10"],
    );
}

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

fn function_forward_reference_succeeds() {
    assert_output(
        r#"print(to_string(double(21)))

def double(x: Int) -> Int { x * 2 }"#,
        &["42"],
    );
}

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

fn record_constructor_positional() {
    assert_output(
        r#"defrecord Point(x: Float, y: Float)
point = Point(1.0, 2.0)
print(to_string(point))
print(to_string(point.x))"#,
        &["Point(x: 1.0, y: 2.0)", "1.0"],
    );
}

fn record_constructor_named_args() {
    assert_output(
        r#"defrecord Point(x: Float, y: Float)
point2 = Point(y: 5.0, x: 3.0)
print(to_string(point2.x))"#,
        &["3.0"],
    );
}

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

fn function_named_args_reordered() {
    assert_output(
        r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(add(y: 2, x: 1)))"#,
        &["3"],
    );
}

fn function_named_args_mixed_with_positional_first() {
    assert_compile_error(
        r#"def add3(x: Int, y: Int, z: Int) -> Int { x + y + z }
print(to_string(add3(1, z: 3, y: 2)))"#,
        "Cannot mix positional and named arguments",
    );
}

fn function_named_args_unknown_name_error() {
    assert_compile_error(
        r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(add(z: 1, y: 2)))"#,
        "Unknown argument name 'z'",
    );
}

fn function_named_args_duplicate_error() {
    assert_compile_error(
        r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(add(1, x: 2)))"#,
        "Cannot mix positional and named arguments",
    );
}

fn function_named_args_positional_after_named_error() {
    assert_compile_error(
        r#"def add(x: Int, y: Int) -> Int { x + y }
print(to_string(add(y: 2, 1)))"#,
        "Cannot mix positional and named arguments",
    );
}

fn function_duplicate_name_is_compile_error() {
    assert_compile_error(
        r#"def f() -> Int { 1 }
def f() -> Int { 2 }"#,
        "Duplicate top-level definition: f",
    );
}

fn top_level_name_collision_between_struct_and_def_is_compile_error() {
    assert_compile_error(
        r#"defstruct User {
  name: String,
}
def User() -> Int { 1 }"#,
        "Duplicate top-level definition: User",
    );
}

fn if_expression_with_else() {
    assert_output(
        r#"flag = True
greeting = if(flag, "hello", "goodbye")
print(greeting)"#,
        &["hello"],
    );
}

fn if_expression_without_else_returns_unit() {
    assert_output(
        r#"flag = True
if_then(flag, print("flag is true"))"#,
        &["flag is true"],
    );
}

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

fn match_boolean_non_exhaustive_error() {
    assert_compile_error(
        r#"flag = True
print(match flag {
  True => "yes",
})"#,
        "Non-exhaustive match. Missing: False",
    );
}

fn match_result_non_exhaustive_error() {
    assert_compile_error(
        r#"r: Result<Int> = Ok(1)
print(match r {
  Ok(v) => to_string(v),
})"#,
        "Non-exhaustive match. Missing: Err",
    );
}

fn match_int_non_exhaustive_error() {
    assert_compile_error(
        r#"n = 1
print(match n {
  1 => "one",
})"#,
        "Non-exhaustive match. Missing: _",
    );
}

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

fn cond_allows_block_bodies() {
    assert_output(
        r#"print(to_string(cond {
  False => 0,
  True => { print("branch"); 42 },
}))"#,
        &["branch", "42"],
    );
}

fn cond_condition_must_be_boolean() {
    assert_compile_error(
        r#"print(to_string(cond {
  1 => 10,
  True => 20,
}))"#,
        "if condition must be Boolean, got Int",
    );
}

fn cond_branch_types_must_match() {
    assert_compile_error(
        r#"print(to_string(cond {
  False => 1,
  True => "x",
}))"#,
        "if branches have different types: Int and String",
    );
}

fn string_interpolation_basic() {
    assert_output(
        r#"name = "alice"
score = 10
print("hello #{name}")
print("score=#{score + 2}")"#,
        &["hello alice", "score=12"],
    );
}

fn string_interpolation_result_type_error() {
    assert_compile_error(
        r#"r: Result<Int> = Ok(1)
print("r=#{r}")"#,
        "Interpolation does not allow Result type",
    );
}

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

pub(crate) fn run_bucket(bucket: usize, bucket_count: usize) {
    let cases: &[(&str, fn())] = &[
        ("bindings_basic_print", bindings_basic_print as fn()),
        (
            "bindings_shadowing_last_wins",
            bindings_shadowing_last_wins as fn(),
        ),
        (
            "annotations_accept_matching_types",
            annotations_accept_matching_types as fn(),
        ),
        (
            "annotations_reject_type_mismatch",
            annotations_reject_type_mismatch as fn(),
        ),
        (
            "primitives_render_to_string",
            primitives_render_to_string as fn(),
        ),
        (
            "inspect_builtin_quotes_strings_and_preserves_error_rendering",
            inspect_builtin_quotes_strings_and_preserves_error_rendering as fn(),
        ),
        (
            "regex_generated_literal_and_builtin_wrappers_work_end_to_end",
            regex_generated_literal_and_builtin_wrappers_work_end_to_end as fn(),
        ),
        ("int_negative_literal", int_negative_literal as fn()),
        ("arithmetic_int_ops", arithmetic_int_ops as fn()),
        ("arithmetic_float_ops", arithmetic_float_ops as fn()),
        (
            "safe_xxx_zero_returns_zero_division_error_display",
            safe_xxx_zero_returns_zero_division_error_display as fn(),
        ),
        ("comparison_int_ops", comparison_int_ops as fn()),
        ("equality_string", equality_string as fn()),
        ("inequality_boolean", inequality_boolean as fn()),
        (
            "kernel_and_or_short_circuit",
            kernel_and_or_short_circuit as fn(),
        ),
        (
            "kernel_eq_neq_helpers_match_operator_behavior",
            kernel_eq_neq_helpers_match_operator_behavior as fn(),
        ),
        (
            "kernel_ordering_and_concat_helpers_match_operator_behavior",
            kernel_ordering_and_concat_helpers_match_operator_behavior as fn(),
        ),
        ("concat_strings", concat_strings as fn()),
        ("arithmetic_precedence", arithmetic_precedence as fn()),
        (
            "equality_reject_mixed_types",
            equality_reject_mixed_types as fn(),
        ),
        ("list_literal_int", list_literal_int as fn()),
        ("list_literal_string", list_literal_string as fn()),
        (
            "list_empty_with_annotation",
            list_empty_with_annotation as fn(),
        ),
        ("list_cons_expr", list_cons_expr as fn()),
        ("list_reject_mixed_types", list_reject_mixed_types as fn()),
        (
            "list_cons_rejects_non_list_tail",
            list_cons_rejects_non_list_tail as fn(),
        ),
        (
            "closure_literal_invocation",
            closure_literal_invocation as fn(),
        ),
        (
            "closure_argument_type_infers_from_add_constraint",
            closure_argument_type_infers_from_add_constraint as fn(),
        ),
        ("closure_builtin_capture", closure_builtin_capture as fn()),
        (
            "const_helper_and_hole_return_surface_work",
            const_helper_and_hole_return_surface_work as fn(),
        ),
        (
            "func_literal_infix_invocation_works",
            func_literal_infix_invocation_works as fn(),
        ),
        (
            "expr_class_operators_are_same_precedence",
            expr_class_operators_are_same_precedence as fn(),
        ),
        (
            "function_partial_application_composition",
            function_partial_application_composition as fn(),
        ),
        (
            "function_partial_application_type_error",
            function_partial_application_type_error as fn(),
        ),
        (
            "function_forward_reference_succeeds",
            function_forward_reference_succeeds as fn(),
        ),
        (
            "struct_definition_and_field_access",
            struct_definition_and_field_access as fn(),
        ),
        (
            "record_constructor_positional",
            record_constructor_positional as fn(),
        ),
        (
            "record_constructor_named_args",
            record_constructor_named_args as fn(),
        ),
        (
            "struct_record_forward_references_and_type_annotation_succeed",
            struct_record_forward_references_and_type_annotation_succeed as fn(),
        ),
        (
            "struct_property_update_via_associated_functions",
            struct_property_update_via_associated_functions as fn(),
        ),
        (
            "struct_constructor_sugar_mixed_named_positional_error",
            struct_constructor_sugar_mixed_named_positional_error as fn(),
        ),
        (
            "impl_method_call_mixed_named_positional_error",
            impl_method_call_mixed_named_positional_error as fn(),
        ),
        (
            "enum_state_transition_via_associated_functions",
            enum_state_transition_via_associated_functions as fn(),
        ),
        (
            "enum_impl_method_call_mixed_named_positional_error",
            enum_impl_method_call_mixed_named_positional_error as fn(),
        ),
        (
            "enum_self_rebinding_requires_self_type",
            enum_self_rebinding_requires_self_type as fn(),
        ),
        (
            "function_named_args_reordered",
            function_named_args_reordered as fn(),
        ),
        (
            "function_named_args_mixed_with_positional_first",
            function_named_args_mixed_with_positional_first as fn(),
        ),
        (
            "function_named_args_unknown_name_error",
            function_named_args_unknown_name_error as fn(),
        ),
        (
            "function_named_args_duplicate_error",
            function_named_args_duplicate_error as fn(),
        ),
        (
            "function_named_args_positional_after_named_error",
            function_named_args_positional_after_named_error as fn(),
        ),
        (
            "function_duplicate_name_is_compile_error",
            function_duplicate_name_is_compile_error as fn(),
        ),
        (
            "top_level_name_collision_between_struct_and_def_is_compile_error",
            top_level_name_collision_between_struct_and_def_is_compile_error as fn(),
        ),
        ("if_expression_with_else", if_expression_with_else as fn()),
        (
            "if_expression_without_else_returns_unit",
            if_expression_without_else_returns_unit as fn(),
        ),
        ("match_boolean_exhaustive", match_boolean_exhaustive as fn()),
        ("match_result_exhaustive", match_result_exhaustive as fn()),
        (
            "match_boolean_wildcard_arm",
            match_boolean_wildcard_arm as fn(),
        ),
        (
            "match_int_literal_patterns",
            match_int_literal_patterns as fn(),
        ),
        (
            "match_string_literal_patterns",
            match_string_literal_patterns as fn(),
        ),
        ("match_list_patterns", match_list_patterns as fn()),
        (
            "match_boolean_non_exhaustive_error",
            match_boolean_non_exhaustive_error as fn(),
        ),
        (
            "match_result_non_exhaustive_error",
            match_result_non_exhaustive_error as fn(),
        ),
        (
            "match_int_non_exhaustive_error",
            match_int_non_exhaustive_error as fn(),
        ),
        (
            "cond_selects_first_true_branch_and_skips_later_branches",
            cond_selects_first_true_branch_and_skips_later_branches as fn(),
        ),
        ("cond_allows_block_bodies", cond_allows_block_bodies as fn()),
        (
            "cond_condition_must_be_boolean",
            cond_condition_must_be_boolean as fn(),
        ),
        (
            "cond_branch_types_must_match",
            cond_branch_types_must_match as fn(),
        ),
        (
            "string_interpolation_basic",
            string_interpolation_basic as fn(),
        ),
        (
            "string_interpolation_result_type_error",
            string_interpolation_result_type_error as fn(),
        ),
        (
            "function_definition_minimal",
            function_definition_minimal as fn(),
        ),
        (
            "function_call_locals_are_isolated",
            function_call_locals_are_isolated as fn(),
        ),
        (
            "function_call_missing_return_reports_unit_hint",
            function_call_missing_return_reports_unit_hint as fn(),
        ),
        ("function_zero_arg_call", function_zero_arg_call as fn()),
    ];
    super::run_bucket_cases("core_language", cases, bucket, bucket_count);
}
