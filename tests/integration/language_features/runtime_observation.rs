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
