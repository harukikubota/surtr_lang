use super::tests::base_bytecode;
use super::*;
use std::cell::Cell;

thread_local! {
    static TABLE_EMPTY_CHECKS: Cell<usize> = const { Cell::new(0) };
    static SCANNED_VALUES: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    static READY_LOOKUPS: Cell<usize> = const { Cell::new(0) };
    static READY_VALUE_CLONES: Cell<usize> = const { Cell::new(0) };
    static SLOT_VALUE_CLONES: Cell<usize> = const { Cell::new(0) };
    static SNAPSHOT_PAIR_BYTES: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_slot_value_clone() {
    SLOT_VALUE_CLONES.with(|count| count.set(count.get() + 1));
}

pub(super) fn record_snapshot_layout<T>(_: &HashMap<u64, T>) {
    SNAPSHOT_PAIR_BYTES.with(|size| size.set(std::mem::size_of::<(u64, T)>()));
}

pub(super) fn record_ready_value_clone() {
    READY_VALUE_CLONES.with(|count| count.set(count.get() + 1));
}

fn ready_value_clones() -> usize {
    READY_VALUE_CLONES.with(Cell::get)
}

pub(super) fn record_ready_lookup() {
    READY_LOOKUPS.with(|count| count.set(count.get() + 1));
}

fn ready_lookups() -> usize {
    READY_LOOKUPS.with(Cell::get)
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
    READY_LOOKUPS.with(|count| count.set(0));
    READY_VALUE_CLONES.with(|count| count.set(0));
    SLOT_VALUE_CLONES.with(|count| count.set(0));
    SNAPSHOT_PAIR_BYTES.with(|size| size.set(0));
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

fn future_with_cleanup_record(vm: &mut VM) -> (u64, FutureId) {
    use super::process_continuation::DetachedCompletion;
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
    (task_id, id)
}

#[test]
fn pending_slot_waits_until_its_producer_cleanup_record_is_removed() {
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let (task_id, id) = future_with_cleanup_record(&mut vm);
    vm.process_runtime.resolve_future(id, Value::Int(int(13)));
    vm.stack = vec![Value::PendingFuture(id); 256].into();
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack.last(), Some(&Value::PendingFuture(id)));
    assert_eq!(scans(), (256, 0));
    assert_eq!(ready_lookups(), 1);
    assert_eq!(
        ready_value_clones(),
        0,
        "cleanup blocks delivery without cloning its result"
    );
    vm.process_runtime.detached_tasks.take(&task_id).unwrap();
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack.last(), Some(&Value::Int(int(13))));
    assert_eq!(vm.stack, vec![Value::Int(int(13)); 256]);
    assert_eq!(scans(), (256, 256));
    assert_eq!(ready_lookups(), 1);
    assert_eq!(ready_value_clones(), 0, "borrow the source after cleanup");
    assert_eq!(SLOT_VALUE_CLONES.with(Cell::get), 256);
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(scans(), (0, 0));
}

#[test]
fn raw_future_inspection_and_task_completion_do_not_clone_results() {
    for cancelled in [false, true] {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let (task, id) = future_with_cleanup_record(&mut vm);
        let value = Value::Tuple(vec![Value::Unit; 64]);
        vm.process_runtime.futures.get_mut(&id).unwrap().state = if cancelled {
            FutureState::Cancelled(value)
        } else {
            FutureState::Ready(value)
        };
        reset_scans();
        assert!(vm.raw_ready_future_value(id).is_some());
        assert!(
            vm.has_runnable_detached_tasks(),
            "completion schedules cleanup"
        );
        assert_eq!(
            ready_value_clones(),
            0,
            "inspect completion without copying its result"
        );
        assert!(
            vm.ready_future_value(id).is_none(),
            "raw readiness does not bypass cleanup"
        );
        assert_eq!(ready_value_clones(), 0);
        vm.process_runtime.detached_tasks.take(&task).unwrap();
        let Value::Tuple(mut delivered) = vm.ready_future_value(id).unwrap() else {
            panic!("expected Tuple result");
        };
        delivered.push(Value::Unit);
        let Some(Value::Tuple(stored)) = vm.raw_ready_future_value(id) else {
            panic!("stored result remains available");
        };
        assert_eq!(
            stored.len(),
            64,
            "the delivered Tuple owns an independent buffer"
        );
        assert_eq!(
            ready_value_clones(),
            1,
            "clone only the delivered source result"
        );
    }
}

fn waiting_task_completion(vm: &mut VM, waited: FutureId) -> FutureId {
    vm.frames[0].call_site = Some((0, 1));
    let BuiltinOutcome::Wait { future_id, .. } = vm
        .start_await_task(&Value::TaskHandle(waited), None)
        .unwrap()
    else {
        panic!("await registers a waiting task");
    };
    future_id
}

#[test]
fn waiting_task_readiness_does_not_clone_and_delivery_clones_once() {
    for cancelled in [false, true] {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let waited = vm.process_runtime.allocate_future(None, None, false);
        let value = Value::Tuple(vec![Value::Unit; 64]);
        vm.process_runtime.futures.get_mut(&waited).unwrap().state = if cancelled {
            FutureState::Cancelled(value.clone())
        } else {
            FutureState::Ready(value.clone())
        };
        let completion = waiting_task_completion(&mut vm, waited);
        reset_scans();
        assert!(vm.has_runnable_detached_tasks());
        assert_eq!(
            ready_value_clones(),
            0,
            "readiness does not consume a result"
        );
        reset_scans();
        vm.drive_ready_detached_tasks().unwrap();
        assert_eq!(
            ready_value_clones(),
            1,
            "clone once to complete the waiting task"
        );
        assert!(vm.process_runtime.detached_tasks.is_empty());
        assert_eq!(vm.raw_ready_future_value(completion), Some(&value));
        assert_eq!(vm.raw_ready_future_value(waited), Some(&value));
        reset_scans();
        vm.drive_ready_detached_tasks().unwrap();
        assert_eq!(ready_value_clones(), 0, "completion is not repeated");
    }
}

#[test]
fn waiting_task_remains_blocked_until_producer_cleanup_finishes() {
    for cancelled in [false, true] {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let (producer, waited) = future_with_cleanup_record(&mut vm);
        let blocker = vm.process_runtime.allocate_future(None, None, false);
        vm.process_runtime
            .detached_tasks
            .get_mut(&producer)
            .unwrap()
            .state = super::process_continuation::DetachedTaskState::Waiting {
            future_id: blocker,
            context: None,
        };
        let value = Value::Tuple(vec![Value::Unit; 64]);
        vm.process_runtime.futures.get_mut(&waited).unwrap().state = if cancelled {
            FutureState::Cancelled(value.clone())
        } else {
            FutureState::Ready(value.clone())
        };
        let completion = waiting_task_completion(&mut vm, waited);
        reset_scans();
        vm.drive_ready_detached_tasks().unwrap();
        assert!(!vm.process_runtime.detached_tasks.contains_key(&producer));
        assert!(
            matches!(
                vm.process_runtime.futures[&completion].state,
                FutureState::Running
            ),
            "the waiting task was not selected before producer cleanup"
        );
        assert_eq!(
            ready_value_clones(),
            0,
            "publishing cleanup only checks readiness"
        );
        assert!(vm.has_runnable_detached_tasks());
        assert_eq!(ready_value_clones(), 0);
        reset_scans();
        vm.drive_ready_detached_tasks().unwrap();
        assert_eq!(ready_value_clones(), 1);
        assert_eq!(vm.raw_ready_future_value(completion), Some(&value));
        assert!(vm.process_runtime.detached_tasks.is_empty());
    }
}

// Actual waiting-task queries and one delivery; no timing/allocator claims.
#[test]
fn waiting_future_readiness_profile() {
    let profile = std::env::var("OI036_WAITING_PROFILE").as_deref() == Ok("1");
    let queries: usize = if profile {
        std::env::var("OI036_QUERIES").unwrap().parse().unwrap()
    } else {
        16
    };
    let width: usize = if profile {
        std::env::var("OI036_WIDTH").unwrap().parse().unwrap()
    } else {
        64
    };
    let case = std::env::var("OI036_CASE").unwrap_or_else(|_| "ready".into());
    assert!(queries > 0);
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let allocated = vm.process_runtime.allocate_future(None, None, false);
    let waited = if case == "unknown" {
        u64::MAX
    } else {
        allocated
    };
    let value = Value::Tuple(vec![Value::Unit; width]);
    let ready = matches!(case.as_str(), "ready" | "cancelled");
    match case.as_str() {
        "ready" => {
            vm.process_runtime.resolve_future(waited, value.clone());
        }
        "cancelled" => {
            vm.process_runtime.futures.get_mut(&waited).unwrap().state =
                FutureState::Cancelled(value.clone());
        }
        "running" | "unknown" => {}
        _ => panic!("unknown OI036_CASE"),
    }
    let completion = waiting_task_completion(&mut vm, waited);
    reset_scans();
    for _ in 0..queries {
        assert_eq!(vm.has_runnable_detached_tasks(), ready);
    }
    let query_clones = ready_value_clones();
    reset_scans();
    vm.drive_ready_detached_tasks().unwrap();
    let delivery_clones = ready_value_clones();
    if ready {
        assert_eq!(vm.raw_ready_future_value(completion), Some(&value));
        assert_eq!(vm.raw_ready_future_value(waited), Some(&value));
        assert!(vm.process_runtime.detached_tasks.is_empty());
        reset_scans();
        vm.drive_ready_detached_tasks().unwrap();
        assert_eq!(ready_value_clones(), 0);
    } else {
        assert!(matches!(
            vm.process_runtime.futures[&completion].state,
            FutureState::Running
        ));
        assert_eq!(vm.process_runtime.detached_tasks.len(), 1);
    }
    if profile {
        eprintln!(
            "{}",
            serde_json::json!({
                "profile":"oi036_waiting_readiness", "case":case, "queries":queries,
                "width":width, "query_source_clones":query_clones,
                "delivery_source_clones":delivery_clones, "verified":true,
                "context_bytes":std::mem::size_of::<ExecutionContext>(),
                "vm_bytes":std::mem::size_of::<VM>(),
            })
        );
    }
}

// Count source-value clones in the real VM; no elapsed time or allocator claims.
#[test]
fn future_ready_value_clone_profile() {
    let profile = std::env::var("OI036_VALUE_CLONE_PROFILE").as_deref() == Ok("1");
    let queries: usize = if profile {
        std::env::var("OI036_QUERIES").unwrap().parse().unwrap()
    } else {
        16
    };
    let width: usize = if profile {
        std::env::var("OI036_WIDTH").unwrap().parse().unwrap()
    } else {
        64
    };
    let case = std::env::var("OI036_CASE").unwrap_or_else(|_| "blocked_ready".into());
    assert!(queries > 0);
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let with_cleanup = case.starts_with("blocked_") || case.starts_with("task_");
    let cleanup = with_cleanup.then(|| future_with_cleanup_record(&mut vm));
    let allocated = cleanup.map_or_else(
        || vm.process_runtime.allocate_future(None, None, false),
        |(_, id)| id,
    );
    let id = if case == "unknown" {
        u64::MAX
    } else {
        allocated
    };
    let value = Value::Tuple(vec![Value::Unit; width]);
    if case.ends_with("cancelled") {
        vm.process_runtime.futures.get_mut(&id).unwrap().state =
            FutureState::Cancelled(value.clone());
    } else if case.ends_with("ready") {
        vm.process_runtime.resolve_future(id, value.clone());
    } else {
        assert!(
            matches!(case.as_str(), "running" | "unknown"),
            "unknown OI036_CASE"
        );
    }
    reset_scans();
    for _ in 0..queries {
        if case.starts_with("raw_") {
            assert!(vm.raw_ready_future_value(id).is_some());
        } else if case.starts_with("task_") {
            assert!(vm.has_runnable_detached_tasks());
        } else if case.starts_with("deliverable_") {
            let delivered = vm.ready_future_value(id);
            assert_eq!(delivered.as_ref(), Some(&value));
        } else {
            assert!(vm.ready_future_value(id).is_none());
        }
    }
    let clones = ready_value_clones();
    if matches!(case.as_str(), "running" | "unknown") {
        assert!(matches!(
            vm.process_runtime.futures[&allocated].state,
            FutureState::Running
        ));
    } else {
        let stored = match &vm.process_runtime.futures[&allocated].state {
            FutureState::Ready(stored) | FutureState::Cancelled(stored) => stored,
            FutureState::Running => panic!("result state changed"),
        };
        assert_eq!(
            stored, &value,
            "inspection and delivery leave the source value intact"
        );
    }
    if let Some((task, future)) = cleanup {
        assert!(vm.ready_future_value(future).is_none());
        vm.process_runtime.detached_tasks.take(&task).unwrap();
        assert_eq!(vm.ready_future_value(future), Some(value));
    }
    if profile {
        eprintln!(
            "{}",
            serde_json::json!({
                "profile":"oi036_ready_value_clones", "case":case, "queries":queries,
                "width":width, "source_value_clones":clones, "verified":true,
                "context_bytes":std::mem::size_of::<ExecutionContext>(),
                "vm_bytes":std::mem::size_of::<VM>(),
            })
        );
    }
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

#[test]
fn repeated_future_ids_share_one_ready_check_even_across_ordinary_values() {
    for case in ["running", "ready", "cancelled", "unknown"] {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let allocated = vm.process_runtime.allocate_future(None, None, false);
        let id = if case == "unknown" {
            u64::MAX
        } else {
            allocated
        };
        match case {
            "ready" => {
                vm.process_runtime.resolve_future(id, Value::Unit);
            }
            "cancelled" => {
                vm.process_runtime.futures.get_mut(&id).unwrap().state =
                    FutureState::Cancelled(Value::Unit);
            }
            _ => {}
        }
        vm.stack = (0..256)
            .flat_map(|_| [Value::PendingFuture(id), Value::Unit])
            .collect();
        reset_scans();
        vm.resolve_ready_pending_stack_values();
        assert_eq!(ready_lookups(), 1, "{case}: one check for the repeated ID");
        let resolved = matches!(case, "ready" | "cancelled");
        assert_eq!(scans(), (256, if resolved { 256 } else { 0 }));
        for pair in vm.stack.chunks_exact(2) {
            assert_eq!(
                pair[0],
                if resolved {
                    Value::Unit
                } else {
                    Value::PendingFuture(id)
                }
            );
            assert_eq!(pair[1], Value::Unit);
        }
    }
}

#[test]
fn ready_snapshot_reuses_ids_and_resets_each_collection_without_cascading() {
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let first = vm.process_runtime.allocate_future(None, None, false);
    let second = vm.process_runtime.allocate_future(None, None, false);
    vm.process_runtime
        .resolve_future(first, Value::PendingFuture(second));
    vm.process_runtime.resolve_future(second, Value::Unit);
    vm.stack = [first, first, second, first, first]
        .into_iter()
        .map(Value::PendingFuture)
        .collect();
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!((ready_lookups(), ready_value_clones()), (2, 0));
    assert_eq!(SLOT_VALUE_CLONES.with(Cell::get), 5);
    assert_eq!(
        vm.stack,
        vec![
            Value::PendingFuture(second),
            Value::PendingFuture(second),
            Value::Unit,
            Value::PendingFuture(second),
            Value::PendingFuture(second),
        ]
    );
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(
        ready_lookups(),
        1,
        "recheck the returned Future on the next pass"
    );
    assert_eq!(vm.stack, vec![Value::Unit; 5]);
    vm.stack.push(Value::PendingFuture(second));
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(
        ready_lookups(),
        1,
        "a previous pass does not cache its result"
    );
    assert_eq!(vm.stack.last(), Some(&Value::Unit));
}

#[test]
fn ready_snapshot_keeps_missing_results_uncached_and_slot_copies_independent() {
    for case in ["running", "unknown", "cleanup"] {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let first = vm.process_runtime.allocate_future(None, None, false);
        let second = match case {
            "running" => vm.process_runtime.allocate_future(None, None, false),
            "unknown" => u64::MAX,
            "cleanup" => {
                let (_, id) = future_with_cleanup_record(&mut vm);
                vm.process_runtime.resolve_future(id, Value::Unit);
                id
            }
            _ => unreachable!(),
        };
        let value = Value::Tuple(vec![Value::Unit; 64]);
        vm.process_runtime.resolve_future(first, value.clone());
        vm.stack = [first, second, first, second, first]
            .into_iter()
            .map(Value::PendingFuture)
            .collect();
        let saved = vm.stack.clone();
        reset_scans();
        vm.resolve_ready_pending_stack_values();
        assert_eq!(
            (ready_lookups(), ready_value_clones()),
            (3, 0),
            "{case}: ready once, missing result twice"
        );
        assert_eq!(scans(), (5, 5));
        assert_eq!(SLOT_VALUE_CLONES.with(Cell::get), 3);
        assert_eq!(
            vm.stack,
            vec![
                value.clone(),
                Value::PendingFuture(second),
                value.clone(),
                Value::PendingFuture(second),
                value.clone()
            ]
        );
        assert_eq!(
            saved,
            vec![
                Value::PendingFuture(first),
                Value::PendingFuture(second),
                Value::PendingFuture(first),
                Value::PendingFuture(second),
                Value::PendingFuture(first)
            ]
        );
        let Some(Value::Tuple(mut delivered)) = vm.stack.pop() else {
            panic!("expected independent Tuple result");
        };
        delivered.push(Value::Unit);
        assert_eq!(vm.stack[0], value);
        assert_eq!(vm.raw_ready_future_value(first), Some(&value));
        assert_eq!(vm.stack.pending_slot_count(), 2);
    }
}

// Operation counts only: deliberately omit elapsed time, allocator and RSS.
#[test]
fn repeated_future_lookup_profile() {
    let profile = std::env::var("OI036_LOOKUP_PROFILE").as_deref() == Ok("1");
    let n: usize = if profile {
        std::env::var("OI036_N").unwrap().parse().unwrap()
    } else {
        16
    };
    assert!(n > 0);
    let case = std::env::var("OI036_CASE").unwrap_or_else(|_| "ready".into());
    let alternating = std::env::var("OI036_ALTERNATING").as_deref() == Ok("1");
    let pattern = std::env::var("OI036_PATTERN").unwrap_or_else(|_| {
        if alternating {
            "alternating".into()
        } else {
            "repeated".into()
        }
    });
    assert!(matches!(
        pattern.as_str(),
        "repeated" | "alternating" | "distinct"
    ));
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let allocated: Vec<_> = (0..if pattern == "distinct" { n.max(2) } else { 2 })
        .map(|_| vm.process_runtime.allocate_future(None, None, false))
        .collect();
    let first = allocated[0];
    let second = allocated[1];
    let ids: Vec<_> = if case == "unknown" {
        (0..allocated.len()).map(|i| u64::MAX - i as u64).collect()
    } else {
        allocated
    };
    match case.as_str() {
        "running" | "unknown" | "empty_slots" => {}
        "ready" => {
            for &id in &ids {
                vm.process_runtime.resolve_future(id, Value::Unit);
            }
        }
        "cancelled" => {
            for &id in &ids {
                vm.process_runtime.futures.get_mut(&id).unwrap().state =
                    FutureState::Cancelled(Value::Unit);
            }
        }
        "future_result" => {
            vm.process_runtime
                .resolve_future(first, Value::PendingFuture(second));
            for &id in &ids[1..] {
                vm.process_runtime.resolve_future(id, Value::Unit);
            }
        }
        _ => panic!("unknown OI036_CASE"),
    }
    vm.stack = (0..n)
        .flat_map(|i| {
            [
                if case == "empty_slots" {
                    Value::Unit
                } else {
                    Value::PendingFuture(
                        ids[match pattern.as_str() {
                            "repeated" => 0,
                            "alternating" => i % 2,
                            "distinct" => i,
                            _ => unreachable!(),
                        }],
                    )
                },
                Value::Unit,
            ]
        })
        .collect();
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    let (read, write) = scans();
    let lookups = ready_lookups();
    let source_clones = ready_value_clones();
    let slot_clones = SLOT_VALUE_CLONES.with(Cell::get);
    let pair_bytes = SNAPSHOT_PAIR_BYTES.with(Cell::get);
    let empty_checks = table_empty_checks();
    for (i, pair) in vm.stack.chunks_exact(2).enumerate() {
        let id = ids[match pattern.as_str() {
            "repeated" => 0,
            "alternating" => i % 2,
            "distinct" => i,
            _ => unreachable!(),
        }];
        let expected = match case.as_str() {
            "running" | "unknown" => Value::PendingFuture(id),
            "future_result" if id == first => Value::PendingFuture(second),
            _ => Value::Unit,
        };
        assert_eq!(pair[0], expected);
        assert_eq!(pair[1], Value::Unit);
    }
    assert_eq!(vm.stack.len(), 2 * n);
    assert_eq!(read, if case == "empty_slots" { 0 } else { n });
    assert_eq!(
        write,
        if matches!(case.as_str(), "ready" | "cancelled" | "future_result") {
            n
        } else {
            0
        }
    );
    if case == "future_result" {
        vm.resolve_ready_pending_stack_values();
        assert!(vm.stack.iter().all(|v| *v == Value::Unit));
    }
    if profile {
        eprintln!(
            "{}",
            serde_json::json!({
                "profile":"oi036_lookup_runs", "case":case, "n":n,
                "alternating":alternating, "ready_lookups":lookups,
                "pattern":pattern, "source_value_clones":source_clones,
                "slot_value_clones":slot_clones, "snapshot_pair_bytes":pair_bytes,
                "owned_pair_bytes":std::mem::size_of::<(u64, Value)>(),
                "borrowed_pair_bytes":std::mem::size_of::<(u64, &Value)>(),
                "read_visits":read, "write_visits":write, "table_empty_checks":empty_checks,
                "context_bytes":std::mem::size_of::<ExecutionContext>(),
                "vm_bytes":std::mem::size_of::<VM>(), "verified":true,
            })
        );
    }
}

// Preparation and full verification are outside the measured resolve interval.
// Allocator traffic and time use separate fresh processes in the manual runner.
#[test]
fn ready_snapshot_cost_profile() {
    let profile = std::env::var("OI036_COST_PROFILE").as_deref() == Ok("1");
    let n: usize = std::env::var("OI036_N")
        .map(|value| value.parse().unwrap())
        .unwrap_or(16);
    let width: usize = std::env::var("OI036_WIDTH")
        .map(|value| value.parse().unwrap())
        .unwrap_or(64);
    let case = std::env::var("OI036_CASE").unwrap_or_else(|_| "ready".into());
    let pattern = std::env::var("OI036_PATTERN").unwrap_or_else(|_| "distinct".into());
    let mode = std::env::var("OI036_MODE").unwrap_or_else(|_| "verify".into());
    assert!(n > 0);
    assert!(matches!(case.as_str(), "ready" | "running" | "ordinary"));
    assert!(matches!(pattern.as_str(), "repeated" | "distinct"));
    assert!(matches!(mode.as_str(), "allocation" | "time" | "verify"));
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let value = Value::Tuple(vec![Value::Unit; width]);
    let ids: Vec<_> = (0..if pattern == "distinct" { n } else { 1 })
        .map(|_| {
            let id = vm.process_runtime.allocate_future(None, None, false);
            if case != "running" {
                vm.process_runtime.resolve_future(id, value.clone());
            }
            id
        })
        .collect();
    vm.stack = (0..n)
        .flat_map(|i| {
            [
                if case == "ordinary" {
                    Value::Unit
                } else {
                    Value::PendingFuture(ids[if pattern == "distinct" { i } else { 0 }])
                },
                Value::Unit,
            ]
        })
        .collect();
    reset_scans();
    let mut traffic = None;
    let mut elapsed_ns = None;
    match mode.as_str() {
        "allocation" => {
            let scope = super::profile_allocator::Scope::start();
            vm.resolve_ready_pending_stack_values();
            traffic = Some(scope.finish());
        }
        "time" => {
            let start = std::time::Instant::now();
            vm.resolve_ready_pending_stack_values();
            elapsed_ns = Some(start.elapsed().as_nanos());
        }
        "verify" => vm.resolve_ready_pending_stack_values(),
        _ => unreachable!(),
    }
    let (read, write) = scans();
    assert_eq!(read, if case == "ordinary" { 0 } else { n });
    assert_eq!(write, if case == "ready" { n } else { 0 });
    assert_eq!(vm.stack.len(), 2 * n);
    assert_eq!(
        vm.stack.pending_slot_count(),
        if case == "running" { n } else { 0 }
    );
    if mode == "verify" {
        for (i, pair) in vm.stack.chunks_exact(2).enumerate() {
            let id = ids[if pattern == "distinct" { i } else { 0 }];
            match case.as_str() {
                "ready" => {
                    assert_eq!(&pair[0], &value);
                    assert_eq!(vm.raw_ready_future_value(id), Some(&value));
                }
                "running" => assert_eq!(pair[0], Value::PendingFuture(id)),
                "ordinary" => assert_eq!(pair[0], Value::Unit),
                _ => unreachable!(),
            }
            assert_eq!(pair[1], Value::Unit);
        }
    }
    if profile {
        let mut row = serde_json::json!({
            "profile":"oi036_snapshot_cost", "n":n, "width":width,
            "case":case, "pattern":pattern, "mode":mode,
            "read_visits":read, "write_visits":write,
            "ready_lookups":ready_lookups(), "source_value_clones":ready_value_clones(),
            "value_bytes":std::mem::size_of::<Value>(),
            "contents_verified":mode == "verify",
        });
        if let Some(traffic) = traffic {
            row["requested_bytes"] = traffic.requested_bytes.into();
            row["freed_bytes"] = traffic.freed_bytes.into();
            row["alloc_calls"] = traffic.alloc_calls.into();
            row["realloc_calls"] = traffic.realloc_calls.into();
            row["dealloc_calls"] = traffic.dealloc_calls.into();
        }
        if let Some(elapsed_ns) = elapsed_ns {
            row["elapsed_ns"] = u64::try_from(elapsed_ns).unwrap().into();
        }
        eprintln!("{row}");
    }
}
