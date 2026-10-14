use super::tests::{base_bytecode, function_entry};
use super::*;
use sindr::builtin::builtin_id_by_name;
use sindr::runtime::HashMapHandle;

fn map(values: [i32; 3]) -> Value {
    Value::HashMap(HashMapHandle::from_entries(vec![
        ("c".into(), Value::Int(int(values[2]))),
        ("a".into(), Value::Int(int(values[0]))),
        ("b".into(), Value::Int(int(values[1]))),
    ]))
}

fn map_values_callable() -> Callable {
    Callable {
        target: CallableTarget::Builtin(builtin_id_by_name("map_values").unwrap()),
        lexical_captures: Vec::new(),
        metadata: Default::default(),
    }
}

fn print_opcode() -> Opcode {
    Opcode::CallBuiltin {
        builtin_id: builtin_id_by_name("print").unwrap(),
        arity: 1,
        span_start: 0,
        span_end: 0,
    }
}

fn prepare(wait_on_second: bool, input: Value) -> (VM, ExecutionContext, Option<FutureId>) {
    let local = u32::from(wait_on_second);
    let mut code = vec![Opcode::LoadLocal(local), print_opcode(), Opcode::Pop];
    if wait_on_second {
        code.extend([
            Opcode::LoadLocal(1),
            Opcode::LoadConst(0),
            Opcode::EqInt,
            Opcode::JumpIfFalse(9),
            Opcode::LoadLocal(0),
            Opcode::Return,
        ]);
    }
    code.extend([Opcode::LoadLocal(local), Opcode::Return]);
    let mut bytecode = base_bytecode(code);
    bytecode.constants = vec![Constant::Int(int(2))];
    bytecode.functions = vec![function_entry(
        0,
        0,
        local + 1,
        (local + 1) as u8,
        Some("Main::map_value"),
    )];
    let mut vm = VM::new(bytecode).with_output_capture();
    let mut mapper = vm.callable_for_function(0);
    let future = wait_on_second.then(|| {
        let future = vm.process_runtime.allocate_future(None, None, false);
        mapper.lexical_captures.push(Value::PendingFuture(future));
        future
    });
    let context = vm
        .prepare_callable_context(map_values_callable(), vec![input, Value::Callable(mapper)])
        .unwrap();
    (vm, context, future)
}

fn finish(vm: &mut VM, context: &mut ExecutionContext, quantum: u64) -> Value {
    for _ in 0..1000 {
        match vm.run_quantum(context, &mut Budget::new(quantum)) {
            ProcessRunOutcome::QuantumExpired => {}
            ProcessRunOutcome::Halted(value) => return value,
            other => panic!("unexpected map_values outcome: {other:?}"),
        }
    }
    panic!("map_values did not complete its bounded test workload");
}

#[test]
fn map_values_preserves_input_and_visits_sorted_keys_once_across_quantums() {
    for quantum in [1, 3, 64] {
        for input in [Value::HashMap(HashMapHandle::empty()), map([1, 2, 3])] {
            let (mut vm, mut context, _) = prepare(false, input.clone());
            let before = format!("{context:?}");
            assert!(matches!(
                vm.run_quantum(&mut context, &mut Budget::new(0)),
                ProcessRunOutcome::QuantumExpired
            ));
            assert_eq!(format!("{context:?}"), before);
            assert!(vm.output.as_ref().unwrap().is_empty());
            assert_eq!(finish(&mut vm, &mut context, quantum), input);
            let Value::HashMap(source) = &input else {
                unreachable!()
            };
            let expected: Vec<String> = if source.len() == 0 {
                Vec::new()
            } else {
                vec!["1".into(), "2".into(), "3".into()]
            };
            assert_eq!(vm.output.as_ref().unwrap(), &expected);
            assert!(context.continuations.is_empty());
            assert!(context.pending_invocation.is_none());
            assert!(vm.process_runtime.futures.is_empty());
        }
    }
}

#[test]
fn map_values_wait_keeps_partial_output_and_does_not_repeat_callback_effects() {
    let source = map([1, 2, 3]);
    let (mut vm, mut context, future) = prepare(true, source.clone());
    let future = future.unwrap();
    assert!(matches!(
        vm.run_quantum(&mut context, &mut Budget::new(64)),
        ProcessRunOutcome::Pending(id) if id == future
    ));
    assert_eq!(vm.output.as_ref().unwrap(), &["1", "2"]);
    assert!(vm.ready_future_value(future).is_none());
    vm.process_runtime
        .resolve_future(future, Value::Int(int(20)));
    assert_eq!(finish(&mut vm, &mut context, 1), map([1, 20, 3]));
    assert_eq!(vm.output.as_ref().unwrap(), &["1", "2", "3"]);
    assert_eq!(source, map([1, 2, 3]));
    assert!(context.continuations.is_empty());
    assert!(context.pending_invocation.is_none());
}

#[test]
fn map_values_cancellation_discards_partial_output_and_stops_later_callbacks() {
    let (mut vm, mut context, future) = prepare(true, map([1, 2, 3]));
    let future = future.unwrap();
    assert!(matches!(
        vm.run_quantum(&mut context, &mut Budget::new(64)),
        ProcessRunOutcome::Pending(id) if id == future
    ));
    VM::cancel_execution_context(&mut context, RuntimeError::new("cancelled"));
    // A completed future must not revive a cancelled mapper or publish its partial map.
    vm.process_runtime
        .resolve_future(future, Value::Int(int(20)));
    let mut failed = false;
    for _ in 0..100 {
        match vm.run_quantum(&mut context, &mut Budget::new(1)) {
            ProcessRunOutcome::QuantumExpired => {}
            ProcessRunOutcome::Failed(error) => {
                assert_eq!(error.message, "cancelled");
                failed = true;
                break;
            }
            other => panic!("cancelled map_values returned {other:?}"),
        }
    }
    assert!(failed);
    assert_eq!(vm.output.as_ref().unwrap(), &["1", "2"]);
    assert!(context.continuations.is_empty());
    assert!(context.pending_invocation.is_none());
    assert!(context.stack.is_empty());
}

fn checkpoint_with_context(
    vm: &mut VM,
    context: ExecutionContext,
) -> (VmCheckpoint, ExecutionTarget) {
    let target = context.target;
    vm.stack = context.stack;
    vm.frames = context.frames;
    vm.tail_call_breadcrumbs = context.tail_call_breadcrumbs;
    vm.continuations = context.continuations;
    vm.pending_invocation = context.pending_invocation;
    vm.cancellation = context.cancellation;
    vm.current_process_execution = context.current_process_execution;
    vm.pc = context.pc;
    let saved = vm.checkpoint_for_chunk(&BytecodeChunk {
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
    });
    (saved, target)
}

#[test]
fn map_values_checkpoint_restores_independent_partial_output_and_callback_position() {
    let (mut vm, mut context, future) = prepare(true, map([1, 2, 3]));
    let future = future.unwrap();
    assert!(matches!(
        vm.run_quantum(&mut context, &mut Budget::new(64)),
        ProcessRunOutcome::Pending(id) if id == future
    ));
    assert_eq!(vm.output.as_ref().unwrap(), &["1", "2"]);
    let (saved, target) = checkpoint_with_context(&mut vm, context);
    let saved_continuations = format!("{:?}", saved.continuations);
    let mut live = vm.take_execution_context(vm.pc, target.clone());
    vm.process_runtime
        .resolve_future(future, Value::Int(int(20)));
    assert_eq!(finish(&mut vm, &mut live, 1), map([1, 20, 3]));
    assert_eq!(vm.output.as_ref().unwrap(), &["1", "2", "3"]);
    assert_eq!(
        format!("{:?}", saved.continuations),
        saved_continuations,
        "advancing the live output must not change the saved partial map"
    );
    vm.rollback_to_checkpoint(saved).unwrap();
    assert!(vm.ready_future_value(future).is_none());
    assert_eq!(vm.output.as_ref().unwrap(), &["1", "2"]);
    let mut restored = vm.take_execution_context(vm.pc, target);
    vm.process_runtime
        .resolve_future(future, Value::Int(int(99)));
    assert_eq!(finish(&mut vm, &mut restored, 1), map([1, 99, 3]));
    assert_eq!(vm.output.as_ref().unwrap(), &["1", "2", "3"]);
    assert!(restored.continuations.is_empty());
    assert!(restored.pending_invocation.is_none());
}
