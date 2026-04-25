use super::harness::{assert_compile_error, assert_output};

fn pipe_accepts_capture_and_injected_call() {
    assert_output(
        r#"def add(x: Int, y: Int) -> Int {
  x + y
}

print(to_string(4 |> add(1)))
print(to_string(4 |> &add(1)))"#,
        &["5", "5"],
    );
}

fn pipe_accepts_qualified_capture_and_injected_call() {
    assert_output(
        r#"defstruct User {
  name: String,
  age: Int,
}

impl User {
  def new(name: String, age: Int) -> Self {
    User { name: name, age: age }
  }

  def get_name(self) -> String {
    self.name
  }
}

user = User("alice", 30)
print(user |> &User::get_name)
print(user |> User::get_name())"#,
        &["alice", "alice"],
    );
}

fn flow_operators_allow_multiline_elixir_style_layout() {
    assert_output(
        r#"def parse(text: String) -> Result<Int> {
  Ok(2)
}

def render(x: Int) -> String {
  to_string(x + 3)
}

def inc(x: Int) -> Int {
  x + 1
}

def double(x: Int) -> Int {
  x * 2
}

value = 4
  |> inc()
  |> double()

pipeline = &parse
  >* &render

plain = &inc
  >> &double

print(to_string(value))
match pipeline("x") {
  Ok(v) => print(v),
  Err(e) => print("err"),
}
print(to_string(plain(4)))"#,
        &["10", "5", "10"],
    );
}

fn result_pipeline_map_and_bind_work() {
    assert_output(
        r#"def inc(x: Int) -> Int {
  x + 1
}

def check(x: Int) -> Result<Int> {
  Ok(x + 10)
}

mapped: Result<Int> = Ok(1) |*> inc()
bound: Result<Int> = Ok(1) |>= check()

match mapped {
  Ok(v) => print(to_string(v)),
  Err(e) => print("mapped err"),
}

match bound {
  Ok(v) => print(to_string(v)),
  Err(e) => print("bound err"),
}"#,
        &["2", "11"],
    );
}

fn flow_operators_accept_function_value_variables_and_grouped_calls() {
    assert_output(
        r#"def inc(x: Int) -> Int {
  x + 1
}

def double(x: Int) -> Int {
  x * 2
}

def show(x: Int) -> String {
  to_string(x)
}

def check(x: Int) -> Result<Int> {
  Ok(x + 10)
}

def mk_add(n: Int) -> (Int -> Int) {
  {|x| x + n}
}

def mk_check(n: Int) -> (Int -> Result<Int>) {
  {|x| Ok(x + n)}
}

f: (Int -> Int) = &inc
g: (Int -> Int) = &double
show_fn: (Int -> String) = &show
check_fn: (Int -> Result<Int>) = &check

print(to_string(1 |> f))
print(to_string(1 |> (mk_add(2))))

plain = f >> g
print(to_string(plain(3)))

render = g >> show_fn
print(render(4))

mapped: Result<Int> = Ok(1) |*> f
mapped_grouped: Result<Int> = Ok(1) |*> (mk_add(4))
bound: Result<Int> = Ok(1) |>= check_fn
bound_grouped: Result<Int> = Ok(1) |>= (mk_check(5))

match mapped {
  Ok(v) => print(to_string(v)),
  Err(e) => print("mapped err"),
}
match mapped_grouped {
  Ok(v) => print(to_string(v)),
  Err(e) => print("mapped grouped err"),
}
match bound {
  Ok(v) => print(to_string(v)),
  Err(e) => print("bound err"),
}
match bound_grouped {
  Ok(v) => print(to_string(v)),
  Err(e) => print("bound grouped err"),
}"#,
        &["2", "3", "8", "8", "2", "5", "11", "6"],
    );
}

fn result_pipeline_injects_left_value_into_call_rhs() {
    assert_output(
        r#"deferror TooSmall {
  "too small"
}

def add(x: Int, y: Int) -> Int {
  x + y
}

def require_at_least(x: Int, floor: Int) -> Result<Int, TooSmall> {
  if(x >= floor, Ok(x), Err(TooSmall))
}

mapped: Result<Int> = Ok(1) |*> add(2)
bound: Result<Int> = Ok(11) |>= require_at_least(10)

match mapped {
  Ok(v) => print(to_string(v)),
  Err(e) => print("mapped err"),
}

match bound {
  Ok(v) => print(to_string(v)),
  Err(e) => print("bound err"),
}"#,
        &["3", "11"],
    );
}

fn pipeline_rhs_supports_partial_special_forms_without_lambda_wrapping() {
    assert_output(
        r#"deferror ParseHandError(detail: String) {
  detail
}

def is_digit_rank(n: Int) -> Boolean {
  and(n >= 1, n <= 9)
}

def mk_detail(ch: String) -> String {
  print("mk:" ++ ch)
  "digit must be 1..9: " ++ ch
}

def parse_hand(ch: String) -> Result<Int> {
  try_from(ch, Int)
    |> map_err(ParseHandError("invalid digit: " ++ ch))
    |>= ensure(&is_digit_rank, ParseHandError(mk_detail(ch)))
}

print(inspect(parse_hand("7")))
print(inspect(parse_hand("0")))
print(inspect(parse_hand("x")))"#,
        &[
            "Ok(7)",
            "mk:0",
            "Err(ParseHandError(\"digit must be 1..9: 0\"))",
            "Err(ParseHandError(\"invalid digit: x\"))",
        ],
    );
}

fn list_pipeline_helpers_and_compose_work() {
    assert_output(
        r#"def inc(x: Int) -> Int {
  x + 1
}

def dup(x: Int) -> List<Int> {
  [x, x + 10]
}

def singleton(x: Int) -> List<Int> {
  [x]
}

nums: List<Int> = [1, 2, 3]
expand = &singleton >=> &dup

print(to_string(singleton(5)))
print(to_string(nums |*> inc()))
print(to_string(nums |>= dup()))
print(to_string(expand(2)))"#,
        &["[5]", "[2, 3, 4]", "[1, 11, 2, 12, 3, 13]", "[2, 12]"],
    );
}

fn compose_builds_callable_from_capture_only() {
    assert_output(
        r#"def parse(text: String) -> Result<Int> {
  Ok(1)
}

def render(x: Int) -> Result<String> {
  Ok(to_string(x + 2))
}

def show(x: Int) -> String {
  "value:" ++ to_string(x + 2)
}

pipeline = &parse >=> &render
lifted = &parse >* &show

match pipeline("x") {
  Ok(v) => print(v),
  Err(e) => print("err"),
}

match lifted("x") {
  Ok(v) => print(v),
  Err(e) => print("err"),
}"#,
        &["3", "value:3"],
    );
}

fn flow_operators_reject_naked_function_refs() {
    assert_compile_error(
        r#"def inc(x: Int) -> Int {
  x + 1
}

value = 1 |> inc"#,
        "requires a function value",
    );

    assert_compile_error(
        r#"def parse(text: String) -> Result<Int> {
  Ok(1)
}

def render(x: Int) -> Result<String> {
  Ok(to_string(x))
}

pipeline = parse >=> render"#,
        "requires a function value",
    );
}

fn compose_rejects_call_expressions() {
    assert_compile_error(
        r#"def parse(text: String) -> Result<Int> {
  Ok(1)
}

def render(x: Int) -> Result<String> {
  Ok(to_string(x))
}

pipeline = parse() >=> render()"#,
        "requires a function value",
    );

    assert_compile_error(
        r#"def parse(text: String) -> Result<Int> {
  Ok(1)
}

def render(x: Int) -> String {
  to_string(x)
}

pipeline = parse() >* render()"#,
        "requires a function value",
    );

    assert_compile_error(
        r#"def inc(x: Int) -> Int {
  x + 1
}

plain = inc() >> inc()"#,
        "requires a function value",
    );
}

fn flow_operators_reject_context_mismatch_and_monadic_map_rhs() {
    assert_compile_error(
        r#"def lift(x: Int) -> Result<Int> {
  Ok(x + 1)
}

value: Result<Int> = Ok(1)
bad = value |*> lift()"#,
        "expects a plain function on the right-hand side",
    );

    assert_compile_error(
        r#"def expand(x: Int) -> List<Int> {
  [x]
}

value: Result<Int> = Ok(1)
bad = value |>= expand()"#,
        "cannot mix Result and List context",
    );

    assert_compile_error(
        r#"def parse(text: String) -> Result<Int> {
  Ok(1)
}

def render(x: Int) -> Result<String> {
  Ok(to_string(x))
}

pipeline = &parse >* &render"#,
        "expects a plain function on the right-hand side",
    );
}

fn result_pipeline_usecase_user_lookup_and_render() {
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

def parse_id(text: String) -> Result<Int> {
  Ok(7)
}

def load_user(id: Int) -> Result<User> {
  Ok(User("alice", 20))
}

def ensure_adult(user: User) -> Result<User> {
  Ok(user)
}

def render(user: User) -> String {
  user.name ++ ":" ++ to_string(user.age)
}

lookup = &parse_id >=> &load_user
summary: Result<String> = lookup("7") |>= ensure_adult() |*> render()

match summary {
  Ok(v) => print(v),
  Err(e) => print("err"),
}"#,
        &["alice:20"],
    );
}

fn kernel_helper_usecase_works_with_funcliteral_and_flow_ops() {
    assert_output(
        r#"defstruct User {
  name: String,
  age: Int,
  active: Boolean,
}

impl User {
  def new(name: String, age: Int, active: Boolean) -> Self {
    User { name: name, age: age, active: active }
  }
}

deferror HiddenUser {
  "hidden user"
}

def parse_key(key: String) -> Result<Int, HiddenUser> {
  if(eq(key, "alice"), Ok(1), if(eq(key, "boss"), Ok(2), Ok(3)))
}

def load_user(id: Int) -> Result<User, HiddenUser> {
  if(
    eq(id, 1),
    Ok(User("alice", 21, True)),
    if(eq(id, 2), Ok(User("boss", 70, True)), Ok(User("guest", 17, False))),
  )
}

def allow(user: User) -> Result<User, HiddenUser> {
  visible = and(
    user.active,
    and(
      user.name `neq` "banned",
      or(user.age `gte` 20, user.name `eq` "alice"),
    ),
  )

  if(visible, Ok(user), Err(HiddenUser))
}

def age_band(user: User) -> String {
  if(
    user.age `lt` 13,
    "child",
    if(user.age `lte` 19, "teen", if(user.age `gt` 64, "senior", "adult")),
  )
}

def render(user: User) -> String {
  visibility = if(and(user.active, user.name `neq` "banned"), "visible", "hidden")
  user.name `concat` ":" `concat` age_band(user) `concat` ":" `concat` visibility
}

lookup = &parse_key >=> &load_user

match lookup("alice") |>= allow() |*> render() {
  Ok(v) => print(v),
  Err(e) => print("hidden"),
}

match lookup("boss") |>= allow() |*> render() {
  Ok(v) => print(v),
  Err(e) => print("hidden"),
}

match lookup("guest") |>= allow() |*> render() {
  Ok(v) => print(v),
  Err(e) => print("hidden"),
}"#,
        &["alice:adult:visible", "boss:senior:visible", "hidden"],
    );
}

fn safebind_usecase_result_and_list_pipeline() {
    assert_output(
        r##"def parse_csv(text: String) -> Result<List<Int>> {
  Ok([1, 2, 3])
}

def expand(n: Int) -> List<Int> {
  [n, n + 10]
}

def show(n: Int) -> String {
  "#" ++ to_string(n)
}

def singleton(n: Int) -> List<Int> {
  [n]
}

nums =? parse_csv("1,2,3")
[head, ..tail] =? nums

print(to_string(head))
print(to_string(tail |>= expand()))
print(to_string((head |> singleton()) |*> show()))"##,
        &["1", "[2, 12, 3, 13]", "[#1]"],
    );
}

fn list_pipeline_usecase_expand_and_present_keywords() {
    assert_output(
        r#"def aliases(word: String) -> List<String> {
  [word, word ++ "_alt"]
}

def wrap_bracket(word: String) -> String {
  "[" ++ word ++ "]"
}

def singleton(word: String) -> List<String> {
  [word]
}

lift_and_expand = &singleton >=> &aliases

words: List<String> = ["surtr", "vm"]
print(to_string(words |>= aliases() |*> wrap_bracket()))
print(to_string(lift_and_expand("bind") |*> wrap_bracket()))"#,
        &[
            "[[surtr], [surtr_alt], [vm], [vm_alt]]",
            "[[bind], [bind_alt]]",
        ],
    );
}

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
nums: List<Int> = [1, 2, 3]
print(to_string(nums))
empty: List<Int> = []
print(to_string(empty))
defstruct User {
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

pub(crate) fn run_bucket(bucket: usize, bucket_count: usize) {
    let cases: &[(&str, fn())] = &[
        (
            "pipe_accepts_capture_and_injected_call",
            pipe_accepts_capture_and_injected_call as fn(),
        ),
        (
            "pipe_accepts_qualified_capture_and_injected_call",
            pipe_accepts_qualified_capture_and_injected_call as fn(),
        ),
        (
            "flow_operators_allow_multiline_elixir_style_layout",
            flow_operators_allow_multiline_elixir_style_layout as fn(),
        ),
        (
            "result_pipeline_map_and_bind_work",
            result_pipeline_map_and_bind_work as fn(),
        ),
        (
            "flow_operators_accept_function_value_variables_and_grouped_calls",
            flow_operators_accept_function_value_variables_and_grouped_calls as fn(),
        ),
        (
            "result_pipeline_injects_left_value_into_call_rhs",
            result_pipeline_injects_left_value_into_call_rhs as fn(),
        ),
        (
            "pipeline_rhs_supports_partial_special_forms_without_lambda_wrapping",
            pipeline_rhs_supports_partial_special_forms_without_lambda_wrapping as fn(),
        ),
        (
            "list_pipeline_helpers_and_compose_work",
            list_pipeline_helpers_and_compose_work as fn(),
        ),
        (
            "compose_builds_callable_from_capture_only",
            compose_builds_callable_from_capture_only as fn(),
        ),
        (
            "flow_operators_reject_naked_function_refs",
            flow_operators_reject_naked_function_refs as fn(),
        ),
        (
            "compose_rejects_call_expressions",
            compose_rejects_call_expressions as fn(),
        ),
        (
            "flow_operators_reject_context_mismatch_and_monadic_map_rhs",
            flow_operators_reject_context_mismatch_and_monadic_map_rhs as fn(),
        ),
        (
            "result_pipeline_usecase_user_lookup_and_render",
            result_pipeline_usecase_user_lookup_and_render as fn(),
        ),
        (
            "kernel_helper_usecase_works_with_funcliteral_and_flow_ops",
            kernel_helper_usecase_works_with_funcliteral_and_flow_ops as fn(),
        ),
        (
            "safebind_usecase_result_and_list_pipeline",
            safebind_usecase_result_and_list_pipeline as fn(),
        ),
        (
            "list_pipeline_usecase_expand_and_present_keywords",
            list_pipeline_usecase_expand_and_present_keywords as fn(),
        ),
        ("language_goal_combined", language_goal_combined as fn()),
    ];
    super::run_bucket_cases("pipelines_and_usecases", cases, bucket, bucket_count);
}
