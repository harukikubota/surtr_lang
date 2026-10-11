use super::process_continuation::DetachedTaskState;
use super::tests::{
    base_bytecode, function_entry, install_process_error_definitions, test_runtime_process_spec,
};
use super::*;
use sindr::ir::{RuntimeBootPlan, RuntimeProcessKind};
use sindr::runtime::{GeneratorHandle, GeneratorProducer, TypeEntry, TypeKind};
use std::sync::Arc;

fn empty_chunk() -> BytecodeChunk {
    BytecodeChunk {
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
        runtime_boot_plan: RuntimeBootPlan::default(),
    }
}

fn worker_vm() -> VM {
    let mut bytecode = base_bytecode(vec![Opcode::Halt]);
    bytecode.runtime_process_specs.entries = vec![test_runtime_process_spec(
        0,
        "Worker",
        RuntimeProcessKind::GenServer,
        RuntimeProcessInstance::Worker,
        false,
        0,
        0,
        None,
    )];
    let mut vm = VM::new(bytecode);
    install_process_error_definitions(&mut vm);
    vm
}

fn spawn(vm: &mut VM, state: Value) -> PidHandle {
    let id = vm
        .allocate_supervised_worker("Worker".into(), Some(state), "DynamicSupervisor".into())
        .unwrap();
    vm.process_handle(id).unwrap()
}

fn builtin(name: &str, captures: Vec<Value>) -> Callable {
    Callable {
        target: CallableTarget::Builtin(sindr::builtin::builtin_id_by_name(name).unwrap()),
        lexical_captures: captures,
        metadata: CallableMetadata::default(),
    }
}

fn builtin_opcode(name: &str, arity: u8) -> Opcode {
    Opcode::CallBuiltin {
        builtin_id: sindr::builtin::builtin_id_by_name(name).unwrap(),
        arity,
        span_start: 0,
        span_end: 0,
    }
}

fn postprocessing_body(vm: &mut VM, pid: &PidHandle, stop: bool) -> Callable {
    let constant = vm.bytecode.constants.len() as u32;
    vm.bytecode.constants.extend([
        Constant::Unit,
        Constant::Int(int(if stop { 7 } else { 42 })),
    ]);
    let pc = vm.bytecode.opcodes.len() as u32;
    vm.bytecode.opcodes.extend([
        Opcode::LoadLocal(0),
        builtin_opcode("__process_state", 1),
        Opcode::Pop,
        Opcode::LoadLocal(0),
        builtin_opcode("__process_postprocess", 1),
        Opcode::Pop,
        Opcode::LoadLocal(0),
    ]);
    if !stop {
        vm.bytecode.opcodes.push(Opcode::LoadConst(constant));
    }
    vm.bytecode.opcodes.extend([
        Opcode::LoadConst(constant + 1),
        builtin_opcode(
            if stop {
                "__genserver_call_stop_normal"
            } else {
                "__genserver_call_reply"
            },
            if stop { 2 } else { 3 },
        ),
        Opcode::Return,
    ]);
    let fun_idx = vm.bytecode.functions.len() as u32;
    vm.bytecode.functions.push(function_entry(
        fun_idx,
        pc,
        1,
        1,
        Some("Worker::postprocessing_wrapper"),
    ));
    Callable {
        target: CallableTarget::Function(fun_idx),
        lexical_captures: vec![Value::Pid(pid.clone())],
        metadata: CallableMetadata::default(),
    }
}

fn stop(vm: &mut VM, pid: &PidHandle) {
    let body = postprocessing_body(vm, pid, true);
    let outcome = vm.start_process_execution(pid.clone(), body).unwrap();
    assert_eq!(
        vm.drive_builtin_outcome(outcome).unwrap(),
        ok_vm_result(Value::Int(int(7)))
    );
}

fn rejected_kind(vm: &mut VM, pid: PidHandle) -> String {
    let body = builtin("print", vec![Value::Str("must not run".into())]);
    let outcome = vm.start_process_execution(pid, body).unwrap();
    decode_vm_result(
        vm.drive_builtin_outcome(outcome).unwrap(),
        "test",
        "identity rejection",
    )
    .unwrap()
    .unwrap_err()
    .kind
}

// The weak producer probes destruction of a real shared runtime value. A PID
// retained by the caller cannot keep this state or a closure capture alive.
fn retained_value() -> (Value, std::sync::Weak<GeneratorProducer>) {
    let producer = Arc::new(GeneratorProducer::Terminal);
    let weak = Arc::downgrade(&producer);
    (Value::Generator(GeneratorHandle(producer)), weak)
}

#[test]
fn checkpoint_before_stop_preserves_body_and_acceptance_without_live_pollution() {
    let mut vm = worker_vm();
    let (state, state_weak) = retained_value();
    let pid = spawn(&mut vm, state);
    let checkpoint = vm.checkpoint_for_chunk(&empty_chunk());
    stop(&mut vm, &pid);
    assert!(vm.process_runtime.processes.is_empty());
    assert!(vm.stopped_identity_matches(&pid));
    assert!(
        state_weak.upgrade().is_some(),
        "checkpoint must retain the saved state"
    );
    let saved = checkpoint.process_runtime.processes.get(&pid.id).unwrap();
    assert_eq!(saved.acceptance, ProcessAcceptance::Accepting);
    assert!(saved.stop_reason.is_none());
    assert!(saved.identity.same_identity(&pid));
    assert!(checkpoint.process_runtime.stopped_identities.is_empty());

    vm.rollback_to_checkpoint(checkpoint);
    assert!(!vm.stopped_identity_matches(&pid));
    let restored = vm.process_handle(pid.id).unwrap();
    assert!(restored.same_identity(&pid));
    assert_eq!(
        vm.process_runtime
            .processes
            .get(&pid.id)
            .unwrap()
            .acceptance,
        ProcessAcceptance::Accepting
    );
    let body = postprocessing_body(&mut vm, &pid, false);
    let outcome = vm.start_process_execution(pid.clone(), body).unwrap();
    assert_eq!(
        vm.drive_builtin_outcome(outcome).unwrap(),
        ok_vm_result(Value::Int(int(42)))
    );
    assert!(
        state_weak.upgrade().is_none(),
        "replaced checkpoint state must be released"
    );
    stop(&mut vm, &pid);
}

fn sleeping_reply_body(vm: &mut VM, pid: &PidHandle, capture: Value) -> Callable {
    let duration_tag = vm
        .bytecode
        .type_registry
        .entries()
        .iter()
        .map(|entry| entry.tag)
        .max()
        .map_or(2, |tag| (tag + 1).max(2));
    vm.bytecode.type_registry.register(TypeEntry {
        tag: duration_tag,
        name: "Duration".into(),
        kind: TypeKind::Struct,
        field_names: vec!["millis".into()],
        private_flags: vec![true],
    });
    let constants = vm.bytecode.constants.len() as u32;
    vm.bytecode.constants.extend([
        Constant::Tag(duration_tag),
        Constant::Int(int(1_000_000)),
        Constant::Int(int(99)),
        Constant::Int(int(42)),
    ]);
    let pc = vm.bytecode.opcodes.len() as u32;
    vm.bytecode.opcodes.extend([
        Opcode::LoadLocal(0),
        builtin_opcode("__process_state", 1),
        Opcode::Pop,
        Opcode::LoadConst(constants),
        Opcode::LoadConst(constants + 1),
        Opcode::StructNew { field_count: 1 },
        Opcode::CallBuiltin {
            builtin_id: sindr::builtin::builtin_id_by_name("__process_sleep").unwrap(),
            arity: 1,
            span_start: 0,
            span_end: 0,
        },
        Opcode::Pop,
        Opcode::LoadLocal(0),
        builtin_opcode("__process_postprocess", 1),
        Opcode::Pop,
        Opcode::LoadLocal(0),
        Opcode::LoadConst(constants + 2),
        Opcode::LoadConst(constants + 3),
        Opcode::CallBuiltin {
            builtin_id: sindr::builtin::builtin_id_by_name("__genserver_call_reply").unwrap(),
            arity: 3,
            span_start: 0,
            span_end: 0,
        },
        Opcode::Return,
    ]);
    let fun_idx = vm.bytecode.functions.len() as u32;
    vm.bytecode.functions.push(function_entry(
        fun_idx,
        pc,
        2,
        2,
        Some("Worker::waiting_wrapper"),
    ));
    Callable {
        target: CallableTarget::Function(fun_idx),
        lexical_captures: vec![Value::Pid(pid.clone()), capture],
        metadata: CallableMetadata::default(),
    }
}

#[test]
fn stopping_checkpoint_restores_waiting_wrapper_frames_and_delivers_reply_once() {
    let mut vm = worker_vm();
    let (state, state_weak) = retained_value();
    let (capture, capture_weak) = retained_value();
    let pid = spawn(&mut vm, state);
    let body = sleeping_reply_body(&mut vm, &pid, capture);
    let outcome = vm.start_process_execution(pid.clone(), body).unwrap();
    let result_future = match &outcome {
        BuiltinOutcome::Wait { future_id, .. } => *future_id,
        other => panic!("expected independent process result future: {other:?}"),
    };
    vm.drive_ready_runtime_quantum().unwrap();
    let (sleep_future, execution_id) = vm
        .process_runtime
        .detached_tasks
        .values()
        .find_map(|task| match &task.state {
            DetachedTaskState::Waiting {
                future_id,
                context: Some(context),
            } => {
                assert!(
                    !context.frames.is_empty(),
                    "actual bytecode wrapper frame must survive the wait"
                );
                assert!(
                    !context.continuations.is_empty(),
                    "sleep resume continuation must be preserved"
                );
                Some((*future_id, context.current_process_execution.unwrap()))
            }
            _ => None,
        })
        .expect("wrapper waiting on sleep");
    stop(&mut vm, &pid);
    assert_eq!(
        vm.process_runtime
            .processes
            .get(&pid.id)
            .unwrap()
            .acceptance,
        ProcessAcceptance::Stopping
    );
    assert_eq!(vm.process_runtime.executions.len(), 1);
    let checkpoint = vm.checkpoint_for_chunk(&empty_chunk());
    assert_eq!(
        rejected_kind(&mut vm, pid.clone()),
        "Global::ProcessStopped"
    );

    vm.resolve_sleep_future(sleep_future).unwrap();
    vm.drive_ready_runtime_quantum().unwrap();
    assert_eq!(
        vm.ready_future_value(result_future),
        Some(ok_vm_result(Value::Int(int(42))))
    );
    assert!(vm.process_runtime.processes.is_empty());
    assert!(vm.process_runtime.executions.is_empty());
    assert!(state_weak.upgrade().is_some());
    assert!(
        capture_weak.upgrade().is_some(),
        "checkpoint retains suspended closure frame"
    );

    vm.rollback_to_checkpoint(checkpoint);
    assert!(vm.process_handle(pid.id).unwrap().same_identity(&pid));
    assert_eq!(
        vm.process_runtime
            .processes
            .get(&pid.id)
            .unwrap()
            .acceptance,
        ProcessAcceptance::Stopping
    );
    assert!(vm.process_runtime.executions.contains_key(&execution_id));
    assert!(vm.ready_future_value(result_future).is_none());
    assert_eq!(
        rejected_kind(&mut vm, pid.clone()),
        "Global::ProcessStopped"
    );
    vm.resolve_sleep_future(sleep_future).unwrap();
    assert_eq!(
        vm.drive_builtin_outcome(outcome).unwrap(),
        ok_vm_result(Value::Int(int(42)))
    );
    assert!(vm.process_runtime.processes.is_empty());
    assert!(vm.process_runtime.executions.is_empty());
    assert!(vm.process_runtime.detached_tasks.is_empty());
    assert!(vm.process_runtime.futures.is_empty());
    assert!(vm.process_runtime.deadline_queue.is_empty());
    assert!(vm.process_runtime.reply_table.is_empty());
    assert!(vm.process_runtime.waiting_table.is_empty());
    assert!(state_weak.upgrade().is_none());
    assert!(
        capture_weak.upgrade().is_none(),
        "completed restored frame must release its capture"
    );
}

#[test]
fn rollback_does_not_reuse_pid_or_reconnect_failed_chunk_handle() {
    let mut vm = worker_vm();
    let original = spawn(&mut vm, Value::Unit);
    let checkpoint = vm.checkpoint_for_chunk(&empty_chunk());
    let failed_chunk_pid = spawn(&mut vm, Value::Unit);
    let discarded_id = failed_chunk_pid.id;
    vm.rollback_to_checkpoint(checkpoint);
    assert!(vm
        .process_handle(original.id)
        .unwrap()
        .same_identity(&original));
    let replacement = spawn(&mut vm, Value::Unit);
    assert!(
        replacement.id > discarded_id,
        "allocator high-water survives rollback"
    );
    assert_eq!(
        rejected_kind(&mut vm, failed_chunk_pid),
        "Global::ProcessStateUnknownPid"
    );
    assert_eq!(vm.process_runtime.executions.len(), 0);
    assert_eq!(vm.process_runtime.processes.len(), 2);
    stop(&mut vm, &replacement);
    stop(&mut vm, &original);
}

#[test]
fn foreign_vm_same_id_does_not_authorize_live_or_reclaimed_process() {
    let mut vm = worker_vm();
    let mut other = worker_vm();
    let pid = spawn(&mut vm, Value::Unit);
    let foreign = spawn(&mut other, Value::Unit);
    assert_eq!(pid.id, foreign.id);
    assert_eq!(pid.process_name, foreign.process_name);
    assert!(!pid.same_identity(&foreign));
    assert_eq!(
        rejected_kind(&mut vm, foreign.clone()),
        "Global::ProcessStateUnknownPid"
    );
    stop(&mut vm, &pid);
    assert!(vm.stopped_identity_matches(&pid));
    assert_eq!(
        rejected_kind(&mut vm, foreign),
        "Global::ProcessStateUnknownPid"
    );
    assert_eq!(rejected_kind(&mut vm, pid), "Global::ProcessStopped");
}

#[test]
fn handler_capability_cannot_enter_process_execution() {
    let mut vm = worker_vm();
    let pid = spawn(&mut vm, Value::Unit);
    let handler = PidHandle::new(pid.id, pid.process_name.clone(), PidKind::Handler);
    let error = vm
        .start_process_execution(
            handler,
            builtin("print", vec![Value::Str("must not run".into())]),
        )
        .unwrap_err();
    assert!(error
        .message
        .contains("handler capability is not a process destination"));
    assert!(vm.process_runtime.executions.is_empty());
    assert_eq!(
        vm.process_runtime
            .processes
            .get(&pid.id)
            .unwrap()
            .acceptance,
        ProcessAcceptance::Accepting
    );
}

#[test]
fn retained_pid_copy_and_lease_preserve_identity_without_retaining_state() {
    let mut vm = worker_vm();
    for _ in 0..32 {
        let (state, state_weak) = retained_value();
        let pid = spawn(&mut vm, state);
        let copy = Value::Pid(pid.clone());
        let lease = WorkerLeaseHandle {
            workers_id: 0,
            pid: pid.clone(),
        };
        stop(&mut vm, &pid);
        assert!(
            state_weak.upgrade().is_none(),
            "old handles must not own reclaimed state"
        );
        assert!(vm.process_runtime.processes.is_empty());
        assert!(vm.process_runtime.executions.is_empty());
        assert!(vm.process_runtime.detached_tasks.is_empty());
        assert!(vm.process_runtime.futures.is_empty());
        assert_eq!(vm.process_runtime.stopped_identities.len(), 1);
        drop(pid);
        vm.sweep_stopped_identities();
        assert_eq!(vm.process_runtime.stopped_identities.len(), 1);
        assert_eq!(
            rejected_kind(&mut vm, lease.pid.clone()),
            "Global::ProcessStopped"
        );
        drop(copy);
        vm.sweep_stopped_identities();
        assert_eq!(vm.process_runtime.stopped_identities.len(), 1);
        drop(lease);
        vm.sweep_stopped_identities();
        assert!(vm.process_runtime.stopped_identities.is_empty());
    }
}

#[test]
fn dropping_checkpoint_releases_reclaimed_process_state() {
    let mut vm = worker_vm();
    let (state, state_weak) = retained_value();
    let pid = spawn(&mut vm, state);
    let checkpoint = vm.checkpoint_for_chunk(&empty_chunk());
    stop(&mut vm, &pid);
    assert!(vm.process_runtime.processes.is_empty());
    assert!(state_weak.upgrade().is_some());
    drop(checkpoint);
    vm.sweep_stopped_identities();
    assert!(state_weak.upgrade().is_none());
    assert_eq!(
        rejected_kind(&mut vm, pid.clone()),
        "Global::ProcessStopped"
    );
    drop(pid);
    vm.sweep_stopped_identities();
    assert!(vm.process_runtime.stopped_identities.is_empty());
}
