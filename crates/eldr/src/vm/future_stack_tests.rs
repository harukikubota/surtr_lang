use super::tests::base_bytecode;
use super::*;
use std::cell::Cell;

thread_local! {
    static TABLE_EMPTY_CHECKS: Cell<usize> = const { Cell::new(0) };
    static SCANNED_VALUES: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

pub(super) fn record_table_empty_check() {
    TABLE_EMPTY_CHECKS.with(|count| count.set(count.get() + 1));
}

fn table_empty_checks() -> usize {
    TABLE_EMPTY_CHECKS.with(Cell::get)
}

pub(super) fn record_read_scan(n: usize) {
    SCANNED_VALUES.with(|counts| {
        let (read, write) = counts.get();
        counts.set((read + n, write));
    });
}

pub(super) fn record_write_scan(n: usize) {
    SCANNED_VALUES.with(|counts| {
        let (read, write) = counts.get();
        counts.set((read, write + n));
    });
}

fn reset_scans() {
    TABLE_EMPTY_CHECKS.with(|count| count.set(0));
    SCANNED_VALUES.with(|counts| counts.set((0, 0)));
}

fn scans() -> (usize, usize) {
    SCANNED_VALUES.with(Cell::get)
}

fn checkpoint(vm: &VM) -> VmCheckpoint {
    vm.checkpoint_for_chunk(&BytecodeChunk {
        opcodes: Vec::new(),
        source_map: None,
        const_base: 0,
        constants: Vec::new(),
        new_locals: 0,
        type_registry_base: 0,
        type_entries: Vec::new(),
        error_template_base: 0,
        error_templates: Vec::new(),
        dbg_template_base: 0,
        dbg_templates: Vec::new(),
        callable_templates: Vec::new(),
        functions: Vec::new(),
        docs: Vec::new(),
        signatures: Vec::new(),
        runtime_process_specs: Vec::new(),
        runtime_boot_plan: Default::default(),
    })
}

#[test]
fn empty_future_table_skips_both_scans_of_deep_stack() {
    for n in [256, 1024] {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        vm.stack = (0..n).map(|i| Value::Int(int(i))).collect();
        let expected = vm.stack.clone();
        assert!(vm.process_runtime.futures.is_empty());

        reset_scans();
        vm.resolve_ready_pending_stack_values();

        assert_eq!(vm.stack, expected);
        assert_eq!(scans(), (0, 0), "empty table cannot resolve any slot");
        assert_eq!(
            table_empty_checks(),
            0,
            "ordinary stack does not query the Future table"
        );

        // A missing future ID also stays unresolved, as in the full scan.
        vm.stack.push(Value::PendingFuture(99));
        reset_scans();
        vm.resolve_ready_pending_stack_values();
        assert_eq!(vm.stack.last(), Some(&Value::PendingFuture(99)));
        assert_eq!(scans(), (0, 0));
        assert_eq!(
            table_empty_checks(),
            1,
            "a pending slot still checks the empty table"
        );
    }
}

#[test]
fn nonempty_future_table_without_resolved_slots_skips_write_scan() {
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let future = vm.process_runtime.allocate_future(None, None, false);
    vm.stack = (0..256).map(|i| Value::Int(int(i))).collect();
    let expected = vm.stack.clone();

    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack, expected);
    assert_eq!(scans(), (0, 0), "no PendingFuture slots to replace");
    assert_eq!(table_empty_checks(), 0);

    vm.stack = vec![Value::PendingFuture(future); 256].into();
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack, vec![Value::PendingFuture(future); 256]);
    assert_eq!(scans(), (256, 0), "Running future has no ready value");
    assert_eq!(table_empty_checks(), 1);

    vm.process_runtime
        .resolve_future(future, Value::Int(int(17)));
    vm.stack = expected.clone();
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack, expected);
    assert_eq!(scans(), (0, 0), "ready table entry has no matching slot");
    assert_eq!(table_empty_checks(), 0);
}

#[test]
fn ready_and_cancelled_futures_replace_all_matching_stack_slots() {
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let ready = vm.process_runtime.allocate_future(None, None, false);
    let cancelled = vm.process_runtime.allocate_future(None, None, false);
    let running = vm.process_runtime.allocate_future(None, None, false);
    vm.process_runtime
        .resolve_future(ready, Value::Int(int(41)));
    vm.process_runtime
        .futures
        .get_mut(&cancelled)
        .unwrap()
        .state = FutureState::Cancelled(Value::Int(int(-7)));
    vm.stack = vec![
        Value::PendingFuture(ready),
        Value::Unit,
        Value::PendingFuture(cancelled),
        Value::PendingFuture(ready),
        Value::PendingFuture(running),
        Value::PendingFuture(u64::MAX),
    ]
    .into();

    vm.resolve_ready_pending_stack_values();

    assert_eq!(
        vm.stack,
        vec![
            Value::Int(int(41)),
            Value::Unit,
            Value::Int(int(-7)),
            Value::Int(int(41)),
            Value::PendingFuture(running),
            Value::PendingFuture(u64::MAX),
        ]
    );
    // Stack delivery must not release an entry needed by other slots/contexts.
    assert_eq!(vm.ready_future_value(ready), Some(Value::Int(int(41))));
    assert_eq!(vm.ready_future_value(cancelled), Some(Value::Int(int(-7))));
}

#[test]
fn context_swap_delivers_ready_value_only_to_its_matching_slots() {
    let mut vm = VM::new(base_bytecode(vec![Opcode::Jump(0), Opcode::Halt]));
    vm.stack = vec![Value::Int(int(3))].into();
    let mut ordinary = vm.take_execution_context(0, ExecutionTarget::TopLevel);

    let future = vm.process_runtime.allocate_future(None, None, false);
    vm.stack = vec![Value::PendingFuture(future), Value::PendingFuture(future)].into();
    let mut suspended = vm.take_execution_context(0, ExecutionTarget::TopLevel);
    vm.process_runtime
        .resolve_future(future, Value::Int(int(23)));

    assert!(matches!(
        vm.step_context(&mut ordinary),
        StepOutcome::Continue
    ));
    assert_eq!(ordinary.stack, vec![Value::Int(int(3))]);
    assert_eq!(suspended.stack, vec![Value::PendingFuture(future); 2]);

    assert!(matches!(
        vm.step_context(&mut suspended),
        StepOutcome::Continue
    ));
    assert_eq!(suspended.stack, vec![Value::Int(int(23)); 2]);
    assert_eq!(ordinary.stack, vec![Value::Int(int(3))]);
    assert!(vm.stack.is_empty());
}

#[test]
fn checkpoint_restores_pending_stack_and_future_table_before_redelivery() {
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let future = vm.process_runtime.allocate_future(None, None, false);
    vm.stack = vec![Value::PendingFuture(future); 2].into();
    let saved = checkpoint(&vm);

    vm.process_runtime
        .resolve_future(future, Value::Int(int(11)));
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack, vec![Value::Int(int(11)); 2]);
    vm.forget_internal_future(future);
    let added = vm.process_runtime.allocate_future(None, None, false);

    vm.rollback_to_checkpoint(saved);
    assert_eq!(vm.stack, vec![Value::PendingFuture(future); 2]);
    assert!(vm.process_runtime.futures.contains_key(&future));
    assert!(!vm.process_runtime.futures.contains_key(&added));
    assert!(vm.ready_future_value(future).is_none());

    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack, vec![Value::PendingFuture(future); 2]);

    vm.process_runtime
        .resolve_future(future, Value::Int(int(19)));
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack, vec![Value::Int(int(19)); 2]);
}

#[test]
fn unrelated_future_does_not_scan_each_growing_literal_stack() {
    let n = 128;
    let mut bytecode = base_bytecode(
        (0..n)
            .map(|_| Opcode::LoadLocal(0))
            .chain([Opcode::ListFromItems { len: n }, Opcode::Halt])
            .collect(),
    );
    bytecode.num_locals = 1;
    let mut vm = VM::new(bytecode);
    vm.frames[0].locals[0] = Value::Int(int(7));
    vm.process_runtime.allocate_future(None, None, false);
    reset_scans();
    vm.run().unwrap();
    assert!(matches!(vm.last_value(), Some(Value::List(list)) if list.len() == n as usize));
    assert_eq!(scans(), (0, 0));
}

#[test]
fn resolution_preserves_pending_future_returned_by_another_future() {
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let first = vm.process_runtime.allocate_future(None, None, false);
    let second = vm.process_runtime.allocate_future(None, None, false);
    vm.stack = vec![Value::PendingFuture(first)].into();
    vm.process_runtime
        .resolve_future(first, Value::PendingFuture(second));
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack.last(), Some(&Value::PendingFuture(second)));
    vm.process_runtime
        .resolve_future(second, Value::Int(int(29)));
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack.last(), Some(&Value::Int(int(29))));
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(scans(), (0, 0));
}

// Manual profile: setup and content verification stay outside the run interval.
#[test]
fn nonempty_future_stack_profile() {
    let profile = std::env::var("OI036_PROFILE").as_deref() == Ok("1");
    let n: u32 = if profile {
        std::env::var("OI036_N").unwrap().parse().unwrap()
    } else {
        16
    };
    let case = std::env::var("OI036_CASE").unwrap_or_else(|_| "unrelated_running".into());
    let mut bytecode = base_bytecode(
        (0..n)
            .map(|_| Opcode::LoadLocal(0))
            .chain([Opcode::ListFromItems { len: n }, Opcode::Halt])
            .collect(),
    );
    bytecode.num_locals = 1;
    let mut vm = VM::new(bytecode);
    vm.frames[0].locals[0] = Value::Int(int(7));
    match case.as_str() {
        "empty" => {}
        "unrelated_running" | "unrelated_ready" | "pending_ready" | "pending_running" => {
            let id = vm.process_runtime.allocate_future(None, None, false);
            if case == "unrelated_ready" || case == "pending_ready" {
                vm.process_runtime.resolve_future(id, Value::Int(int(3)));
            }
            if case.starts_with("pending_") {
                vm.stack.push(Value::PendingFuture(id));
            }
        }
        _ => panic!("unknown OI036_CASE"),
    }
    let future_count = vm.process_runtime.futures.len();
    reset_scans();
    let started = std::time::Instant::now();
    vm.run().unwrap();
    let elapsed_ns = started.elapsed().as_nanos();
    let (read, write) = scans();
    let Value::List(list) = vm.last_value().unwrap() else {
        panic!("expected List")
    };
    assert_eq!(list.len(), n as usize);
    if !profile || std::env::var("PROBE_VERIFY").as_deref() == Ok("1") {
        assert!(list.iter().all(|v| v == Value::Int(int(7))));
    }
    if profile {
        eprintln!(
            "{}",
            serde_json::json!({
                "profile":"oi036", "case":case, "n":n, "future_count":future_count,
                "elapsed_ns":elapsed_ns, "read_visits":read, "write_visits":write,
                "table_empty_checks":table_empty_checks(),
                "context_bytes":std::mem::size_of::<ExecutionContext>(),
                "vm_bytes":std::mem::size_of::<VM>(),
                "verify":std::env::var("PROBE_VERIFY").as_deref() == Ok("1"),
            })
        );
    }
}

#[test]
fn pending_slot_waits_until_its_producer_cleanup_record_is_removed() {
    use super::process_continuation::DetachedCompletion;
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    vm.frames[0].call_site = Some((0, 1));
    let callable = Callable {
        target: CallableTarget::Builtin(
            sindr::builtin::builtin_id_by_name("list_flat_map").unwrap(),
        ),
        lexical_captures: vec![
            Value::List(ListHandle::empty()),
            Value::Callable(Callable {
                target: CallableTarget::Function(0),
                lexical_captures: vec![],
                metadata: Default::default(),
            }),
        ],
        metadata: Default::default(),
    };
    let task_id = vm.process_runtime.next_detached_task_id;
    vm.start_task(callable, TaskMode::Async, None).unwrap();
    let DetachedCompletion::Future(Some(id)) =
        vm.process_runtime.detached_tasks[&task_id].completion
    else {
        panic!("expected async completion")
    };
    vm.process_runtime.resolve_future(id, Value::Int(int(13)));
    vm.stack.push(Value::PendingFuture(id));
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack.last(), Some(&Value::PendingFuture(id)));
    assert_eq!(scans(), (1, 0));
    vm.process_runtime.detached_tasks.take(&task_id).unwrap();
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack.last(), Some(&Value::Int(int(13))));
    assert_eq!(scans(), (1, 1));
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(scans(), (0, 0));
}

#[test]
fn unresolved_future_only_visits_its_slot_in_a_growing_literal_stack() {
    let n = 128;
    let mut bytecode = base_bytecode(
        (0..n)
            .map(|_| Opcode::LoadLocal(0))
            .chain([Opcode::ListFromItems { len: n }, Opcode::Halt])
            .collect(),
    );
    bytecode.num_locals = 1;
    let mut vm = VM::new(bytecode);
    vm.frames[0].locals[0] = Value::Int(int(7));
    let id = vm.process_runtime.allocate_future(None, None, false);
    vm.stack.push(Value::PendingFuture(id));
    reset_scans();
    vm.run().unwrap();
    assert_eq!(scans(), (n as usize + 2, 0));
    assert_eq!(vm.stack.first(), Some(&Value::PendingFuture(id)));
    assert!(matches!(vm.last_value(), Some(Value::List(list)) if list.len() == n as usize));
}

#[test]
fn ready_delivery_visits_only_pending_slots_among_ordinary_values() {
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let ready = vm.process_runtime.allocate_future(None, None, false);
    let running = vm.process_runtime.allocate_future(None, None, false);
    vm.stack = (0..4096).map(|_| Value::Unit).collect();
    for id in [ready, running, ready] {
        vm.stack.push(Value::PendingFuture(id));
    }
    vm.stack
        .push(Value::Tuple(vec![Value::PendingFuture(ready)]));
    vm.process_runtime.resolve_future(ready, Value::Int(int(5)));
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(scans(), (3, 3));
    assert_eq!(
        &vm.stack[4096..],
        &[
            Value::Int(int(5)),
            Value::PendingFuture(running),
            Value::Int(int(5)),
            Value::Tuple(vec![Value::PendingFuture(ready)]),
        ]
    );
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(scans(), (1, 0));
}
