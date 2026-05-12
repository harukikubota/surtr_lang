use super::harness::observe_surtr;

fn tail_recursive_function_reuses_single_non_top_level_frame() {
    let observation = observe_surtr(
        r#"def fib_tail(n: Int, a: Int, b: Int) -> Int {
  if(n == 0, a, fib_tail(n - 1, b, a + b))
}

fib_tail(50, 0, 1)"#,
    );

    assert_eq!(observation.stats.max_frame_depth, 2);
    assert_eq!(observation.stats.function_calls, 51);
    assert_eq!(observation.stats.return_count, 1);
    assert_eq!(observation.stats.tail_calls_optimized, 50);
}

fn match_arm_tail_calls_are_optimized() {
    let observation = observe_surtr(
        r#"def sum_list(values: List<Int>, acc: Int) -> Int {
  match values {
    [] => acc,
    [head, ..tail] => sum_list(tail, acc + head),
  }
}

sum_list([1, 2, 3, 4, 5], 0)"#,
    );

    assert_eq!(observation.stats.max_frame_depth, 2);
    assert_eq!(observation.stats.tail_calls_optimized, 5);
}

fn mutual_tail_recursion_is_optimized() {
    let observation = observe_surtr(
        r#"def even(n: Int) -> Boolean {
  if(n == 0, True, odd(n - 1))
}

def odd(n: Int) -> Boolean {
  if(n == 0, False, even(n - 1))
}

even(100)"#,
    );

    assert_eq!(observation.stats.max_frame_depth, 2);
    assert_eq!(observation.stats.function_calls, 101);
    assert_eq!(observation.stats.return_count, 1);
    assert_eq!(observation.stats.tail_calls_optimized, 100);
}

fn non_tail_recursion_keeps_growing_frames() {
    let observation = observe_surtr(
        r#"def sum_non_tail(n: Int) -> Int {
  if(n == 0, 0, 1 + sum_non_tail(n - 1))
}

sum_non_tail(200)"#,
    );

    assert!(observation.stats.max_frame_depth > 100);
    assert_eq!(observation.stats.tail_calls_optimized, 0);
    assert_eq!(observation.stats.function_calls, 201);
    assert_eq!(observation.stats.return_count, 201);
}

fn tail_call_closure_to_user_function_counts_as_tco() {
    let observation = observe_surtr(
        r#"def inc(value: Int) -> Int {
  value + 1
}

def apply_tail(f: (Int -> Int), value: Int) -> Int {
  f(value)
}

apply_tail(&inc, 41)"#,
    );

    assert_eq!(observation.stats.max_frame_depth, 2);
    assert_eq!(observation.stats.tail_calls_optimized, 1);
}

fn tail_call_closure_to_builtin_is_not_user_function_tco() {
    let observation = observe_surtr(
        r#"def apply_tail(f: (Int -> String), value: Int) -> String {
  f(value)
}

apply_tail(&to_string, 41)"#,
    );

    assert_eq!(observation.stats.max_frame_depth, 2);
    assert_eq!(observation.stats.tail_calls_optimized, 0);
}

fn stdlib_list_reduce_large_input_uses_tail_calls() {
    let observation = observe_surtr(
        r#"List::reduce(
  [1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
   11, 12, 13, 14, 15, 16, 17, 18, 19, 20],
  0,
  {|acc, n| acc + n},
)"#,
    );

    assert!(
        observation.stats.max_frame_depth <= 4,
        "expected List::reduce frame depth to stay bounded, stats={:?}",
        observation.stats
    );
    assert!(
        observation.stats.tail_calls_optimized >= 20,
        "expected List::reduce to use tail-call optimization, stats={:?}",
        observation.stats
    );
}

fn stdlib_list_reduce_while_resume_path_uses_tail_calls() {
    let observation = observe_surtr(
        r#"List::reduce_while(
  [1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
   11, 12, 13, 14, 15, 16, 17, 18, 19, 20],
  0,
  {|acc, n| if(n < 15, ReduceStep::Resume(acc + n), ReduceStep::Stop(acc))},
)"#,
    );

    assert!(
        observation.stats.max_frame_depth <= 4,
        "expected List::reduce_while frame depth to stay bounded, stats={:?}",
        observation.stats
    );
    assert!(
        observation.stats.tail_calls_optimized >= 14,
        "expected List::reduce_while resume path to use tail-call optimization, stats={:?}",
        observation.stats
    );
}

fn result_branch_tail_call_is_optimized() {
    let observation = observe_surtr(
        r#"def next(value: Int) -> Result<Int> {
  Ok(value + 1)
}

def go(remaining: Int, acc: Int) -> Result<Int> {
  if(
    remaining == 0,
    Ok(acc),
    match next(acc) {
      Ok(next_acc) => go(remaining - 1, next_acc),
      Err(err) => Err(err),
    },
  )
}

go(40, 0)"#,
    );

    assert!(
        observation.stats.max_frame_depth <= 3,
        "expected Result branch frame depth to stay bounded, stats={:?}",
        observation.stats
    );
    assert_eq!(observation.stats.tail_calls_optimized, 40);
}

fn process_handler_helper_tail_call_is_optimized() {
    let observation = observe_surtr(
        r#"defagent Counter {
  meta {
    instance: Singleton
    init_policy: Eager
    state: Int
  }

  @init
  def init() -> Result<Int> {
    Ok(30)
  }

  def count_down(remaining: Int, acc: Int) -> Result<Int> {
    if(
      remaining == 0,
      Ok(acc),
      count_down(remaining - 1, acc + 1),
    )
  }

  @get
  def value(state: Int) -> Result<Int> {
    count_down(state, 0)
  }
}

Counter::value()"#,
    );

    assert!(
        observation.stats.max_frame_depth <= 4,
        "expected process handler helper frame depth to stay bounded, stats={:?}",
        observation.stats
    );
    assert!(
        observation.stats.tail_calls_optimized >= 30,
        "expected process handler helper to use tail-call optimization, stats={:?}",
        observation.stats
    );
}

fn generator_fibonacci_resume_consumer_uses_tail_calls() {
    let observation = observe_surtr(
        r#"def fib_step(state: (Int, Int), idx: Int, count: Int) -> Result<(Int, (Int, Int))> {
  if(
    idx < count,
    fib_emit_next(state),
    Err(NoneError),
  )
}

def fib_emit_next(state: (Int, Int)) -> Result<(Int, (Int, Int))> {
  (a, b) = state
  Ok((a, (b, a + b)))
}

def fib_generator(count: Int) -> Generator<(Int, Int), Int> {
  Generator::unfold((0, 1), {|state, idx|
    fib_step(state, idx, count)
  })
}

def take_and_resume(
  gen: Generator<(Int, Int), Int>,
  count: Int,
  acc_rev: List<Int>,
) -> (List<Int>, Generator<(Int, Int), Int>) {
  if(
    count <= 0,
    (List::reverse(acc_rev), gen),
    match Generator::next(gen) {
      Ok(pair) => take_pair_and_resume(pair, count, acc_rev),
      Err(_) => (List::reverse(acc_rev), gen),
    },
  )
}

def take_pair_and_resume(
  pair: (Int, Generator<(Int, Int), Int>),
  count: Int,
  acc_rev: List<Int>,
) -> (List<Int>, Generator<(Int, Int), Int>) {
  (value, next_gen) = pair
  take_and_resume(next_gen, count - 1, [value, ..acc_rev])
}

fib0 = fib_generator(240)
pair = take_and_resume(fib0, 150, [])
(_first, fib150) = pair
Generator::idx(fib150)"#,
    );

    assert!(
        observation.stats.max_frame_depth <= 8,
        "expected frame depth to stay bounded in generator resume flow, stats={:?}",
        observation.stats
    );
    assert!(
        observation.stats.tail_calls_optimized >= 150,
        "expected generator resume flow to use tail-call optimization, stats={:?}",
        observation.stats
    );
}

pub(crate) fn run_bucket(bucket: usize, bucket_count: usize) -> usize {
    let cases: &[(&str, fn())] = &[
        (
            "tail_recursive_function_reuses_single_non_top_level_frame",
            tail_recursive_function_reuses_single_non_top_level_frame as fn(),
        ),
        (
            "match_arm_tail_calls_are_optimized",
            match_arm_tail_calls_are_optimized as fn(),
        ),
        (
            "mutual_tail_recursion_is_optimized",
            mutual_tail_recursion_is_optimized as fn(),
        ),
        (
            "non_tail_recursion_keeps_growing_frames",
            non_tail_recursion_keeps_growing_frames as fn(),
        ),
        (
            "tail_call_closure_to_user_function_counts_as_tco",
            tail_call_closure_to_user_function_counts_as_tco as fn(),
        ),
        (
            "tail_call_closure_to_builtin_is_not_user_function_tco",
            tail_call_closure_to_builtin_is_not_user_function_tco as fn(),
        ),
        (
            "stdlib_list_reduce_large_input_uses_tail_calls",
            stdlib_list_reduce_large_input_uses_tail_calls as fn(),
        ),
        (
            "stdlib_list_reduce_while_resume_path_uses_tail_calls",
            stdlib_list_reduce_while_resume_path_uses_tail_calls as fn(),
        ),
        (
            "result_branch_tail_call_is_optimized",
            result_branch_tail_call_is_optimized as fn(),
        ),
        (
            "process_handler_helper_tail_call_is_optimized",
            process_handler_helper_tail_call_is_optimized as fn(),
        ),
        (
            "generator_fibonacci_resume_consumer_uses_tail_calls",
            generator_fibonacci_resume_consumer_uses_tail_calls as fn(),
        ),
    ];
    super::run_bucket_cases("runtime_observation", cases, bucket, bucket_count)
}
