use super::tests::base_bytecode;
use super::*;
use std::cell::Cell;

thread_local! {
    static SCANNED_VALUES: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
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

        // A missing future ID also stays unresolved, as in the full scan.
        vm.stack.push(Value::PendingFuture(99));
        reset_scans();
        vm.resolve_ready_pending_stack_values();
        assert_eq!(vm.stack.last(), Some(&Value::PendingFuture(99)));
        assert_eq!(scans(), (0, 0));
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
    assert_eq!(scans(), (256, 0), "no PendingFuture slots to replace");

    vm.stack = vec![Value::PendingFuture(future); 256];
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack, vec![Value::PendingFuture(future); 256]);
    assert_eq!(scans(), (256, 0), "Running future has no ready value");

    vm.process_runtime
        .resolve_future(future, Value::Int(int(17)));
    vm.stack = expected.clone();
    reset_scans();
    vm.resolve_ready_pending_stack_values();
    assert_eq!(vm.stack, expected);
    assert_eq!(scans(), (256, 0), "ready table entry has no matching slot");
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
    ];

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
    vm.stack = vec![Value::Int(int(3))];
    let mut ordinary = vm.take_execution_context(0, ExecutionTarget::TopLevel);

    let future = vm.process_runtime.allocate_future(None, None, false);
    vm.stack = vec![Value::PendingFuture(future), Value::PendingFuture(future)];
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
    vm.stack = vec![Value::PendingFuture(future); 2];
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
