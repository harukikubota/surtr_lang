use super::harness::{assert_compile_error, assert_output, run_surtr_with_stderr};

fn safebind_top_level_ok() {
    assert_output(
        r#"value: Result<Int> = Ok(5)
num =? value
print(to_string(num + 1))"#,
        &["6"],
    );
}

fn safebind_list_pattern_ok() {
    assert_output(
        r#"value: Result<List<Int>> = Ok([1, 2, 3])
[head, ..tail] =? value
print(to_string(head))
print(to_string(tail))"#,
        &["1", "[2, 3]"],
    );
}

fn safebind_list_pattern_plain_list_ok() {
    assert_output(
        r#"value = [1, 2, 3]
[head, ..tail] =? value
print(to_string(head))
print(to_string(tail))"#,
        &["1", "[2, 3]"],
    );
}

fn safebind_uncons_string_ok() {
    assert_output(
        r#"value = "source"
uncons(first, tail) =? value
print(first)
print(tail)"#,
        &["s", "ource"],
    );
}

fn safebind_string_pattern_plain_string_ok() {
    assert_output(
        r#"value = "source"
[first, ..tail] =? value
print(first)
print(tail)"#,
        &["s", "ource"],
    );
}

fn safebind_string_pattern_handles_multibyte_chars() {
    assert_output(
        r#"value = "あい"
[first, ..tail] =? value
print(first)
print(tail)"#,
        &["あ", "い"],
    );
}

fn safebind_list_pattern_plain_list_empty_propagates_empty_list() {
    let (_stdout, stderr) = run_surtr_with_stderr(
        r#"value: List<Int> = []
[head, ..tail] =? value
print("after")"#,
    )
    .expect("Pipeline failed");
    assert_eq!(stderr, vec!["Error: EmptyList: Empty List."]);
}

fn safebind_string_pattern_empty_propagates_pattern_mismatch() {
    let (_stdout, stderr) = run_surtr_with_stderr(
        r#"value: String = ""
[first, ..tail] =? value
print("after")"#,
    )
    .expect("Pipeline failed");
    assert_eq!(
        stderr,
        vec!["Error: PatternMismatch: Pattern did not match."]
    );
}

fn safebind_fixed_list_pattern_reports_index_out_of_bounds_for_longer_rhs() {
    let (_stdout, stderr) = run_surtr_with_stderr(
        r#"li = [1, 2]
[f] =? li"#,
    )
    .expect("Pipeline failed");
    assert_eq!(
        stderr,
        vec!["Error: IndexOutOfBounds: LHS.len(1) < RHS.len(2)"]
    );
}

fn safebind_fixed_list_pattern_reports_index_out_of_bounds_for_shorter_rhs() {
    let (_stdout, stderr) = run_surtr_with_stderr(
        r#"li = [1]
[e1, e2] =? li"#,
    )
    .expect("Pipeline failed");
    assert_eq!(
        stderr,
        vec!["Error: IndexOutOfBounds: LHS.len(2) > RHS.len(1)"]
    );
}

fn match_string_empty_and_uncons_is_exhaustive() {
    assert_output(
        r#"value = "source"
print(match value {
  [] => "empty",
  [first, ..tail] => tail,
})"#,
        &["ource"],
    );
}

fn pinned_match_and_safebind_compare_existing_value() {
    assert_output(
        r#"expected = 2
value = 2
print(match value {
  ^expected => "hit",
  _ => "miss",
})
^expected =? value
print(to_string(is_match(value, ^expected)))"#,
        &["hit", "True"],
    );
}

fn pinned_pattern_is_not_allowed_with_plain_bind() {
    assert_compile_error(
        r#"expected = 2
^expected = 2"#,
        "Pinned patterns are not allowed with =",
    );
}

fn pin_operator_is_not_allowed_in_expression_position() {
    assert_compile_error(
        r#"expected = 2
value = ^expected"#,
        "Pin operator ^ is only allowed in MatchBlock patterns and bulk_update paths.",
    );
}

fn expr_list_cons_does_not_become_string_cons() {
    assert_compile_error(
        r#"source = ["x"]
str: String = ["t", ..source]"#,
        "expected String, got List<String>",
    );
}

fn match_string_uncons_without_empty_arm_is_non_exhaustive() {
    assert_compile_error(
        r#"value = "x"
print(match value {
  [head, ..tail] => head,
})"#,
        "Non-exhaustive match. Missing: []",
    );
}

fn safebind_list_pattern_with_nested_constructor_literals_ok() {
    assert_output(
        r#"lr = [Ok(1), Ok(2), Ok(3)]
[Ok(1), Ok(2), _] =? lr
print("ok")"#,
        &["ok"],
    );
}

fn safebind_list_pattern_with_nested_constructor_and_tail_ok() {
    assert_output(
        r#"lr = [Ok(1), Ok(2), Ok(3)]
[Ok(1), ..tail] =? lr
print(to_string(tail))"#,
        &["[Ok(2), Ok(3)]"],
    );
}

fn safebind_top_ok_pattern_requires_nested_result() {
    assert_compile_error(
        r#"value: Result<Int> = Ok(5)
Ok(num) =? value"#,
        "Constructor pattern requires an enum or Result RHS",
    );
}

fn safebind_top_ok_pattern_allows_nested_result() {
    assert_output(
        r#"value: Result<Result<Int>> = Ok(Ok(5))
Ok(num) =? value
print(to_string(num + 1))"#,
        &["6"],
    );
}

fn safebind_nested_result_err_is_a_normal_pattern_mismatch() {
    let (stdout, stderr) = run_surtr_with_stderr(
        r#"deferror Oops {
  "oops"
}

value: Result<Result<Int>> = Ok(Err(Oops))
Ok(num) =? value
print("after")"#,
    )
    .expect("Pipeline failed");
    assert_eq!(stdout, Vec::<String>::new());
    assert_eq!(
        stderr,
        vec!["Error: PatternMismatch: Pattern did not match."]
    );
}

fn safebind_list_pattern_empty_propagates_empty_list() {
    let (_stdout, stderr) = run_surtr_with_stderr(
        r#"def fun() -> Result<Int> {
  value: Result<List<Int>> = Ok([])
  [head, ..tail] =? value
  Ok(head)
}

ret: Result<Int> = fun()
match ret {
  Ok(v) => print(to_string(v)),
  Err(e) => eprint(e),
}"#,
    )
    .expect("program should run");
    assert_eq!(stderr, vec!["Error: EmptyList: Empty List."]);
}

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

fn safebind_closure_returns_ok_and_propagates_err() {
    assert_output(
        r#"deferror Oops {
  "oops"
}

def gen(flag: Boolean) -> Result<Int, Oops> {
  if(flag, Ok(10), Err(Oops))
}

handler: (Boolean -> Result<Int>) = {|flag|
  value =? gen(flag)
  Ok(value + 1)
}

print(inspect(handler(True)))
print(inspect(handler(False)))"#,
        &["Ok(11)", "Err(Oops(\"oops\"))"],
    );
}

fn safebind_closure_rejects_non_result_return() {
    assert_compile_error(
        r#"bad: (Int -> Int) = {|x|
  value =? Ok(x)
  value
}"#,
        "can only be used in functions returning Result",
    );
}

fn safebind_nested_closure_stops_at_nearest_callable() {
    assert_output(
        r#"deferror Inner {
  "inner"
}

def outer() -> Result<String, Inner> {
  handler: (Int -> Result<Int>) = {|x|
    value =? Err(Inner)
    Ok(value + x)
  }

  match handler(1) {
    Ok(_) => Ok("bad"),
    Err(_) => Ok("inner stopped here"),
  }
}

print(inspect(outer()))"#,
        &["Ok(\"inner stopped here\")"],
    );
}

fn safebind_closure_local_ok_and_err_propagation() {
    let (stdout, stderr) = run_surtr_with_stderr(
        r#"deferror BadInput {
  "bad input"
}

ok_handler: (Int -> Result<Int>) = {|x|
  value =? Ok(x + 1)
  Ok(value)
}

checked: (Int -> Result<Int>) = {|x|
  value =? if(x > 0, Ok(x), Err(BadInput))
  Ok(value + 10)
}

print(inspect(ok_handler(1)))
match checked(0) {
  Ok(v) => print("bad:" ++ to_string(v)),
  Err(e) => eprint(e),
}"#,
    )
    .expect("program should run");
    assert_eq!(stdout, vec!["Ok(2)"]);
    assert_eq!(stderr, vec!["Error: BadInput: bad input"]);
}

fn safebind_nested_closure_propagates_to_nearest_callable() {
    let (stdout, stderr) = run_surtr_with_stderr(
        r#"deferror InnerStop {
  "inner stop"
}

def outer() -> Result<String> {
  inner: (Int -> Result<Int>) = {|x|
    value =? if(x > 0, Ok(x), Err(InnerStop))
    Ok(value + 1)
  }

  result: Result<Int> = inner(0)
  print("after inner")
  Ok(inspect(result))
}

match outer() {
  Ok(v) => print(v),
  Err(e) => eprint(e),
}"#,
    )
    .expect("program should run");
    assert_eq!(
        stdout,
        vec!["after inner", "Err(InnerStop(\"inner stop\"))"]
    );
    assert_eq!(stderr, Vec::<String>::new());
}

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

fn do_safebind_result_preserves_err_and_skips_continuation() {
    assert_output(
        r#"deferror Oops {
  "oops"
}

def source() -> Result<Int, Oops> {
  print("rhs")
  Err(Oops)
}

result: Result<Int> = do::<Result> {
  value =? source()
  print("after")
  Ok(value + 1)
}

print(inspect(result))"#,
        &["rhs", "Err(Oops(\"oops\"))"],
    );
}

fn do_safebind_option_overrides_result_err_with_none() {
    assert_output(
        r#"deferror Oops {
  "oops"
}

def source() -> Result<Int, Oops> {
  print("rhs")
  Err(Oops)
}

result: Option<Int> = do::<Option> {
  value =? source()
  print("after")
  Option::Some(value + 1)
}

print(inspect(result))"#,
        &["rhs", "Option::None"],
    );
}

fn do_safebind_option_pattern_failure_uses_empty() {
    assert_output(
        r#"result: Option<Int> = do::<Option> {
  1 =? 1
  2 =? 1
  print("after")
  Option::Some(3)
}

print(inspect(result))"#,
        &["Option::None"],
    );
}

fn do_safebind_success_evaluates_rhs_once() {
    assert_output(
        r#"def source() -> Result<Int> {
  print("rhs")
  Ok(1)
}

result: Option<Int> = do::<Option> {
  value =? source()
  Option::Some(value + 1)
}

print(inspect(result))"#,
        &["rhs", "Option::Some(2)"],
    );
}

fn safebind_rejects_total_plain_rhs() {
    assert_compile_error(
        "num =? 10",
        "Int is not a SafeBind target; it is not a Monad, and only a Result RHS can be decomposed by `=?`.",
    );
}

fn safebind_partial_option_constructor_checks_the_whole_rhs() {
    assert_output(
        r#"value: Option<Int> = Option::Some(1)
Option::Some(num) =? value
print(to_string(num))"#,
        &["1"],
    );
}

fn safebind_partial_option_none_constructor_checks_the_whole_rhs() {
    assert_output(
        r#"value: Option<Int> = Option::None
Option::None =? value
print("matched")"#,
        &["matched"],
    );
}

fn safebind_partial_user_enum_constructor_binds_all_payloads() {
    assert_output(
        r#"defenum Pair<$A, $B> {
  Both($A, $B),
}

pair: Pair<Int, String> = Pair::Both(7, "ok")
Pair::Both(num, label) =? pair
print(to_string(num) ++ ":" ++ label)"#,
        &["7:ok"],
    );
}

fn safebind_partial_option_mismatch_uses_normal_pattern_failure() {
    let (stdout, stderr) = run_surtr_with_stderr(
        r#"value: Option<Int> = Option::None
Option::Some(num) =? value
print("after")"#,
    )
    .expect("Pipeline failed");
    assert_eq!(stdout, Vec::<String>::new());
    assert_eq!(
        stderr,
        vec!["Error: PatternMismatch: Pattern did not match."]
    );
}

fn safebind_nested_err_constructor_binds_instead_of_propagating() {
    assert_output(
        r#"deferror Oops {
  "oops"
}

value: Result<Result<Int>> = Ok(Err(Oops))
Err(error) =? value
print(Error::kind(error))"#,
        &["Oops"],
    );
}

fn safebind_requires_result_return_function() {
    assert_compile_error(
        r#"def bad() -> Int {
  num =? Ok(1)
  num
}"#,
        "can only be used in functions returning Result",
    );
}

fn safebind_rejects_compile_time_facet_values() {
    assert_compile_error(
        r#"defstruct User { name: String }
impl User {
  def new(name: String) -> Self { User { name: name } }
}
path =? User.name"#,
        "Facet values cannot be bound with `=?`",
    );
}

fn assignment_operators_non_associative() {
    assert_compile_error("x = y =? z", "non-associative");
}

fn plain_bind_rejects_result_test_pattern() {
    assert_compile_error(
        "Ok(num) = Ok(1)",
        "Only total MatchBlock patterns can be used with `=`",
    );
}

fn deferror_no_args_basic() {
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

fn deferror_forward_reference_in_result_signature_succeeds() {
    assert_output(
        r#"ret: Result<Int> = load()
match ret {
  Ok(val) => print("ok"),
  Err(e)  => print("err"),
}

def load() -> Result<Int, NotFound> {
  Err(NotFound("/api"))
}

deferror NotFound(path: String) {
  "Not Found: #{path}"
}"#,
        &["err"],
    );
}

fn builtin_prelude_provides_none_error() {
    let (stdout, stderr) = run_surtr_with_stderr(
        r#"ret: Result<Int> = Err(NoneError)
match ret {
  Ok(val) => print(to_string(val)),
  Err(e)  => eprint(e),
}"#,
    )
    .expect("Pipeline failed");
    assert_eq!(stdout, Vec::<String>::new());
    assert_eq!(stderr, vec!["Error: NoneError: None Value."]);
}

fn builtin_safe_xxx_zero_error_can_be_matched_and_eprinted() {
    let (stdout, stderr) = run_surtr_with_stderr(
        r#"match safe_div(1, 0) {
  Ok(val) => print(to_string(val)),
  Err(e)  => eprint(e),
}

match safe_mod(1, 0) {
  Ok(val) => print(to_string(val)),
  Err(e)  => eprint(e),
}"#,
    )
    .expect("Pipeline failed");
    assert_eq!(stdout, Vec::<String>::new());
    assert_eq!(
        stderr,
        vec![
            "Error: ZeroDivisionError: division by zero",
            "Error: ZeroDivisionError: division by zero",
        ]
    );
}

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

fn deferror_rejects_raw_error_binding() {
    assert_compile_error(
        r#"deferror PageNotFound(html: String) {
  "Page Not Found. #{html}"
}

bad = PageNotFound("404")"#,
        "Error values must be wrapped with Err(...)",
    );
}

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

fn result_helpers_render_multiline_cause_trees() {
    assert_output(
        r#"deferror Lower {
  "lower"
}

deferror Higher {
  "higher"
}

deferror Tail {
  "tail"
}

print(inspect(Result::cause(Err(Lower), Higher)))
print(inspect(Result::chain(Err(Lower), Result::cause(Err(Tail), Higher))))"#,
        &[
            "Err(Higher(\"higher\"))\n|_ Lower(\"lower\")",
            "Err(Higher(\"higher\"))\n|_ Tail(\"tail\")\n   |_ Lower(\"lower\")",
        ],
    );
}

fn eprint_renders_linear_cause_chain_lines() {
    let (stdout, stderr) = run_surtr_with_stderr(
        r#"deferror Lower {
  "lower"
}

deferror Higher {
  "higher"
}

deferror Tail {
  "tail"
}

match Result::cause(Err(Lower), Higher) {
  Ok(_) => (),
  Err(e) => eprint(e),
}

match Result::chain(Err(Lower), Result::cause(Err(Tail), Higher)) {
  Ok(_) => (),
  Err(e) => eprint(e),
}"#,
    )
    .expect("Pipeline failed");
    assert_eq!(stdout, Vec::<String>::new());
    assert_eq!(
        stderr,
        vec![
            "Error: Higher: higher",
            "Caused by: Lower: lower",
            "Error: Higher: higher",
            "Caused by: Tail: tail",
            "Caused by: Lower: lower",
        ]
    );
}

pub(crate) fn run_bucket(bucket: usize, bucket_count: usize) -> usize {
    let cases: &[(&str, fn())] = &[
        ("safebind_top_level_ok", safebind_top_level_ok as fn()),
        ("safebind_list_pattern_ok", safebind_list_pattern_ok as fn()),
        (
            "safebind_list_pattern_plain_list_ok",
            safebind_list_pattern_plain_list_ok as fn(),
        ),
        (
            "safebind_uncons_string_ok",
            safebind_uncons_string_ok as fn(),
        ),
        (
            "safebind_string_pattern_plain_string_ok",
            safebind_string_pattern_plain_string_ok as fn(),
        ),
        (
            "safebind_string_pattern_handles_multibyte_chars",
            safebind_string_pattern_handles_multibyte_chars as fn(),
        ),
        (
            "safebind_list_pattern_plain_list_empty_propagates_empty_list",
            safebind_list_pattern_plain_list_empty_propagates_empty_list as fn(),
        ),
        (
            "safebind_string_pattern_empty_propagates_pattern_mismatch",
            safebind_string_pattern_empty_propagates_pattern_mismatch as fn(),
        ),
        (
            "safebind_fixed_list_pattern_reports_index_out_of_bounds_for_longer_rhs",
            safebind_fixed_list_pattern_reports_index_out_of_bounds_for_longer_rhs as fn(),
        ),
        (
            "safebind_fixed_list_pattern_reports_index_out_of_bounds_for_shorter_rhs",
            safebind_fixed_list_pattern_reports_index_out_of_bounds_for_shorter_rhs as fn(),
        ),
        (
            "match_string_empty_and_uncons_is_exhaustive",
            match_string_empty_and_uncons_is_exhaustive as fn(),
        ),
        (
            "pinned_match_and_safebind_compare_existing_value",
            pinned_match_and_safebind_compare_existing_value as fn(),
        ),
        (
            "pinned_pattern_is_not_allowed_with_plain_bind",
            pinned_pattern_is_not_allowed_with_plain_bind as fn(),
        ),
        (
            "pin_operator_is_not_allowed_in_expression_position",
            pin_operator_is_not_allowed_in_expression_position as fn(),
        ),
        (
            "expr_list_cons_does_not_become_string_cons",
            expr_list_cons_does_not_become_string_cons as fn(),
        ),
        (
            "match_string_uncons_without_empty_arm_is_non_exhaustive",
            match_string_uncons_without_empty_arm_is_non_exhaustive as fn(),
        ),
        (
            "safebind_list_pattern_with_nested_constructor_literals_ok",
            safebind_list_pattern_with_nested_constructor_literals_ok as fn(),
        ),
        (
            "safebind_list_pattern_with_nested_constructor_and_tail_ok",
            safebind_list_pattern_with_nested_constructor_and_tail_ok as fn(),
        ),
        (
            "safebind_top_ok_pattern_requires_nested_result",
            safebind_top_ok_pattern_requires_nested_result as fn(),
        ),
        (
            "safebind_top_ok_pattern_allows_nested_result",
            safebind_top_ok_pattern_allows_nested_result as fn(),
        ),
        (
            "safebind_nested_result_err_is_a_normal_pattern_mismatch",
            safebind_nested_result_err_is_a_normal_pattern_mismatch as fn(),
        ),
        (
            "safebind_list_pattern_empty_propagates_empty_list",
            safebind_list_pattern_empty_propagates_empty_list as fn(),
        ),
        (
            "safebind_function_early_return_on_err",
            safebind_function_early_return_on_err as fn(),
        ),
        (
            "safebind_closure_returns_ok_and_propagates_err",
            safebind_closure_returns_ok_and_propagates_err as fn(),
        ),
        (
            "safebind_closure_rejects_non_result_return",
            safebind_closure_rejects_non_result_return as fn(),
        ),
        (
            "safebind_nested_closure_stops_at_nearest_callable",
            safebind_nested_closure_stops_at_nearest_callable as fn(),
        ),
        (
            "safebind_closure_local_ok_and_err_propagation",
            safebind_closure_local_ok_and_err_propagation as fn(),
        ),
        (
            "safebind_nested_closure_propagates_to_nearest_callable",
            safebind_nested_closure_propagates_to_nearest_callable as fn(),
        ),
        (
            "safebind_script_error_eprints",
            safebind_script_error_eprints as fn(),
        ),
        (
            "do_safebind_result_preserves_err_and_skips_continuation",
            do_safebind_result_preserves_err_and_skips_continuation as fn(),
        ),
        (
            "do_safebind_option_overrides_result_err_with_none",
            do_safebind_option_overrides_result_err_with_none as fn(),
        ),
        (
            "do_safebind_option_pattern_failure_uses_empty",
            do_safebind_option_pattern_failure_uses_empty as fn(),
        ),
        (
            "do_safebind_success_evaluates_rhs_once",
            do_safebind_success_evaluates_rhs_once as fn(),
        ),
        (
            "safebind_rejects_total_plain_rhs",
            safebind_rejects_total_plain_rhs as fn(),
        ),
        (
            "safebind_partial_option_constructor_checks_the_whole_rhs",
            safebind_partial_option_constructor_checks_the_whole_rhs as fn(),
        ),
        (
            "safebind_partial_option_none_constructor_checks_the_whole_rhs",
            safebind_partial_option_none_constructor_checks_the_whole_rhs as fn(),
        ),
        (
            "safebind_partial_user_enum_constructor_binds_all_payloads",
            safebind_partial_user_enum_constructor_binds_all_payloads as fn(),
        ),
        (
            "safebind_partial_option_mismatch_uses_normal_pattern_failure",
            safebind_partial_option_mismatch_uses_normal_pattern_failure as fn(),
        ),
        (
            "safebind_nested_err_constructor_binds_instead_of_propagating",
            safebind_nested_err_constructor_binds_instead_of_propagating as fn(),
        ),
        (
            "safebind_requires_result_return_function",
            safebind_requires_result_return_function as fn(),
        ),
        (
            "safebind_rejects_compile_time_facet_values",
            safebind_rejects_compile_time_facet_values as fn(),
        ),
        (
            "assignment_operators_non_associative",
            assignment_operators_non_associative as fn(),
        ),
        (
            "plain_bind_rejects_result_test_pattern",
            plain_bind_rejects_result_test_pattern as fn(),
        ),
        ("deferror_no_args_basic", deferror_no_args_basic as fn()),
        (
            "deferror_forward_reference_in_result_signature_succeeds",
            deferror_forward_reference_in_result_signature_succeeds as fn(),
        ),
        (
            "builtin_prelude_provides_none_error",
            builtin_prelude_provides_none_error as fn(),
        ),
        (
            "builtin_safe_xxx_zero_error_can_be_matched_and_eprinted",
            builtin_safe_xxx_zero_error_can_be_matched_and_eprinted as fn(),
        ),
        (
            "deferror_interpolated_message_display",
            deferror_interpolated_message_display as fn(),
        ),
        (
            "match_err_eprint_with_wildcard_arm",
            match_err_eprint_with_wildcard_arm as fn(),
        ),
        (
            "deferror_rejects_raw_error_binding",
            deferror_rejects_raw_error_binding as fn(),
        ),
        (
            "result_ok_case_prints_value",
            result_ok_case_prints_value as fn(),
        ),
        (
            "result_helpers_render_multiline_cause_trees",
            result_helpers_render_multiline_cause_trees as fn(),
        ),
        (
            "eprint_renders_linear_cause_chain_lines",
            eprint_renders_linear_cause_chain_lines as fn(),
        ),
    ];
    super::run_bucket_cases("safebind_and_errors", cases, bucket, bucket_count)
}
