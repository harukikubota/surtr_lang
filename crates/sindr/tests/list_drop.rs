//! Isolate aborts from long Cons destruction from the rest of the test suite.
use sindr::runtime::{Callable, CallableMetadata, CallableTarget, ListHandle, Value};
use std::process::Command;

const LENGTH: usize = 100_000;
const CHILD_CASE: &str = "SURTR_LIST_DROP_CHILD_CASE";

fn cons_chain(mut tail: ListHandle) -> ListHandle {
    for _ in 0..LENGTH {
        tail = ListHandle::cons(Value::Unit, &tail);
    }
    tail
}

fn subprocess(case: &str, body: fn()) {
    if std::env::var(CHILD_CASE).as_deref() == Ok(case) {
        // The same bounded stack covers construction and destruction on every platform.
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(body)
            .unwrap()
            .join()
            .unwrap();
        println!("completed {case}");
        return;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", case, "--nocapture"])
        .env(CHILD_CASE, case)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{case}: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("completed {case}")));
}

#[test]
fn unique_cons_chain_drop() {
    subprocess("unique_cons_chain_drop", || {
        let list = cons_chain(ListHandle::empty());
        assert_eq!(list.len(), LENGTH);
        drop(list);
    });
}

#[test]
fn shared_cons_tail_drop() {
    subprocess("shared_cons_tail_drop", || {
        let tail = cons_chain(ListHandle::empty());
        let list = cons_chain(tail.clone());
        drop(list);
        assert_eq!(tail.len(), LENGTH);
        assert!(tail.iter().all(|value| value == Value::Unit));
        // Iteration must leave the shared tail intact for its final owner's drop.
        assert_eq!(tail.head_value(), Some(Value::Unit));
        drop(tail);
    });
}

#[test]
fn cons_over_packed_drop() {
    subprocess("cons_over_packed_drop", || {
        let packed = ListHandle::from_items(vec![Value::Bool(true), Value::Bool(false)]);
        let list = cons_chain(packed.clone());
        drop(list);
        assert_eq!(
            packed.iter().collect::<Vec<_>>(),
            vec![Value::Bool(true), Value::Bool(false)]
        );
        // A unique Packed terminator is released by the Cons chain as well.
        drop(cons_chain(packed));
    });
}

#[test]
fn fixed_depth_values_with_long_cons_drop() {
    subprocess("fixed_depth_values_with_long_cons_drop", || {
        let long_list = || Value::List(cons_chain(ListHandle::empty()));
        let values = vec![
            Value::List(ListHandle::cons(long_list(), &ListHandle::empty())),
            Value::List(ListHandle::from_items(vec![long_list()])),
            Value::Tuple(vec![long_list()]),
            Value::Tagged {
                tag: 99,
                fields: vec![long_list()],
            },
            Value::Callable(Callable {
                target: CallableTarget::Function(0),
                lexical_captures: vec![long_list()],
                metadata: CallableMetadata::default(),
            }),
        ];
        // This checks bounded container depth, not arbitrary Value-tree depth.
        drop(Value::Tuple(values));
    });
}
