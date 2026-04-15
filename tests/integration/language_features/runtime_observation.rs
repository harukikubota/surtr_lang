use super::harness::observe_surtr;

#[test]
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

#[test]
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

#[test]
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

#[test]
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

#[test]
fn generator_fibonacci_resume_consumer_uses_tail_calls() {
    let observation = observe_surtr(
        r#"def fib_generator(count: Int) -> Generator<(Int, Int), Int> {
  Generator::unfold((0, 1), {|state, idx|
    if(
      idx < count,
      {
        (a, b) = state
        Ok((a, (b, a + b)))
      },
      Err(NoneError),
    )
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
      Ok(pair) => {
        (value, next_gen) = pair
        take_and_resume(next_gen, count - 1, [value, ..acc_rev])
      },
      Err(_) => (List::reverse(acc_rev), gen),
    },
  )
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
