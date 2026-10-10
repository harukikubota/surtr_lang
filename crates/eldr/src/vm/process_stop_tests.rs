use super::tests::{base_bytecode, test_runtime_process_spec};
use super::*;
use sindr::ir::RuntimeProcessKind;

#[test]
fn store_outside_process_execution_is_contract_error() {
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
    let id = vm
        .allocate_process_state("Worker".into(), Some(Value::Unit))
        .unwrap();
    let pid = vm.process_handle(id).unwrap();
    assert!(vm.process_store(&pid, Value::Unit).is_err());
}

fn worker_vm() -> (VM, PidHandle) {
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
    super::tests::install_process_error_definitions(&mut vm);
    vm.process_runtime
        .root_supervisor
        .effective_supervisors
        .insert(
            "DynamicSupervisor".into(),
            RuntimeSupervisorPolicy {
                strategy: "OneForOne".into(),
                max_restarts: 5,
                max_seconds: 10,
                child_restart_default: "Transient".into(),
                allow_adopt: true,
                shutdown_timeout_ms: None,
            },
        );
    let id = vm
        .allocate_supervised_worker(
            "Worker".into(),
            Some(Value::Unit),
            "DynamicSupervisor".into(),
        )
        .unwrap();
    let pid = vm.process_handle(id).unwrap();
    (vm, pid)
}

fn builtin(name: &str, captures: Vec<Value>) -> Callable {
    Callable {
        target: CallableTarget::Builtin(sindr::builtin::builtin_id_by_name(name).unwrap()),
        lexical_captures: captures,
        metadata: CallableMetadata::default(),
    }
}

// These internal tests omit handler evaluation and exercise its postprocessing
// endpoint. Use the same explicit state-read and compiler marker as a wrapper.
fn start_synthetic_execution(
    vm: &mut VM,
    pid: PidHandle,
    body: Callable,
) -> Result<BuiltinOutcome, RuntimeError> {
    let endpoints = [
        "__genserver_call_reply",
        "__genserver_call_reply_later",
        "__genserver_call_stop_normal",
        "__genserver_call_stop_error",
        "__genserver_cast_next",
        "__genserver_cast_stop_normal",
        "__genserver_cast_stop_error",
    ];
    let endpoint = match body.target {
        CallableTarget::Builtin(id) => endpoints
            .iter()
            .any(|name| sindr::builtin::builtin_id_by_name(name) == Some(id)),
        _ => false,
    };
    let body = if endpoint {
        let pc = vm.bytecode.opcodes.len() as u32;
        vm.bytecode.opcodes.extend([
            Opcode::LoadLocal(0),
            Opcode::CallBuiltin {
                builtin_id: sindr::builtin::builtin_id_by_name("__process_state").unwrap(),
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Pop,
            Opcode::LoadLocal(0),
            Opcode::CallBuiltin {
                builtin_id: sindr::builtin::builtin_id_by_name("__process_postprocess").unwrap(),
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Pop,
            Opcode::LoadLocal(1),
            Opcode::CallClosure {
                arity: 0,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Return,
        ]);
        let id = vm.bytecode.functions.len() as u32;
        vm.bytecode.functions.push(super::tests::function_entry(
            id,
            pc,
            2,
            2,
            Some("Worker::synthetic_postprocessing"),
        ));
        Callable {
            target: CallableTarget::Function(id),
            lexical_captures: vec![Value::Pid(pid.clone()), Value::Callable(body)],
            metadata: CallableMetadata::default(),
        }
    } else {
        body
    };
    vm.start_process_execution(pid, body)
}

#[test]
fn managed_stop_finishes_reply_reclaims_body_and_preserves_stopped_identity() {
    let (mut vm, pid) = worker_vm();
    let body = builtin(
        "__genserver_call_stop_normal",
        vec![Value::Pid(pid.clone()), Value::Int(int(7))],
    );
    let outcome = start_synthetic_execution(&mut vm, pid.clone(), body).unwrap();
    assert_eq!(
        vm.drive_builtin_outcome(outcome).unwrap(),
        ok_vm_result(Value::Int(int(7)))
    );
    assert!(vm.process_runtime.processes.is_empty());
    assert!(vm.process_runtime.executions.is_empty());
    assert!(vm.process_runtime.detached_tasks.is_empty());
    assert!(vm.process_runtime.futures.is_empty());
    assert!(vm.stopped_identity_matches(&pid));
    let denied = start_synthetic_execution(
        &mut vm,
        pid.clone(),
        builtin("print", vec![Value::Str("must not run".into())]),
    )
    .unwrap();
    let result = vm.drive_builtin_outcome(denied).unwrap();
    let error = decode_vm_result(result, "test", "stopped")
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind, "Global::ProcessStopped");
    drop(pid);
    vm.sweep_stopped_identities();
    assert!(vm.process_runtime.stopped_identities.is_empty());
}

#[test]
fn stop_error_returns_original_error_and_reclaims_process() {
    let (mut vm, pid) = worker_vm();
    let body = builtin(
        "__genserver_call_stop_error",
        vec![
            Value::Pid(pid.clone()),
            Value::Error(Box::new(vm.process_error("Boom", "boom"))),
        ],
    );
    let outcome = start_synthetic_execution(&mut vm, pid.clone(), body).unwrap();
    let error = decode_vm_result(
        vm.drive_builtin_outcome(outcome).unwrap(),
        "test",
        "StopError",
    )
    .unwrap()
    .unwrap_err();
    assert_eq!(error.kind, "Global::Boom");
    assert!(vm.process_runtime.executions.is_empty());
    assert!(vm.process_runtime.processes.is_empty());
}

#[test]
fn fake_same_number_handle_cannot_reach_live_or_stopped_instance() {
    let (mut vm, pid) = worker_vm();
    let fake = PidHandle::new(pid.id, pid.process_name.clone(), PidKind::Worker);
    let call = start_synthetic_execution(
        &mut vm,
        fake,
        builtin("print", vec![Value::Str("must not run".into())]),
    )
    .unwrap();
    let error = decode_vm_result(vm.drive_builtin_outcome(call).unwrap(), "test", "fake")
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind, "Global::ProcessStateUnknownPid");
    assert!(vm.process_runtime.executions.is_empty());
}

#[test]
fn stopping_worker_keeps_membership_but_cannot_be_selected_or_adopted() {
    use sindr::runtime::{TypeEntry, TypeKind};
    let (mut vm, pid) = worker_vm();
    vm.bytecode.type_registry.register(TypeEntry {
        tag: 200,
        name: "Duration".into(),
        kind: TypeKind::Struct,
        field_names: vec!["millis".into()],
        private_flags: vec![true],
    });
    let handle = WorkersHandle {
        id: 1,
        process_name: "Worker".into(),
    };
    vm.process_runtime.worker_sets.insert(
        handle.id,
        WorkerSetState {
            supervisor_name: "DynamicSupervisor".into(),
            worker_process: "Worker".into(),
            init_callable: builtin("print", vec![]),
            strategy: WorkerStrategyState {
                init: 0,
                min: 0,
                max: 1,
                scale: WorkerScaleState::Fix(0),
            },
            target: 0,
            members: vec![pid.id],
            next_index: 0,
        },
    );
    let sleeping = start_synthetic_execution(
        &mut vm,
        pid.clone(),
        builtin(
            "__process_sleep",
            vec![Value::Tagged {
                tag: 200,
                fields: vec![Value::Int(int(1_000_000))],
            }],
        ),
    )
    .unwrap();
    let result_future = match &sleeping {
        BuiltinOutcome::Wait { future_id, .. } => *future_id,
        _ => panic!("wait"),
    };
    vm.drive_ready_detached_tasks().unwrap();
    let stop = start_synthetic_execution(
        &mut vm,
        pid.clone(),
        builtin(
            "__genserver_call_stop_normal",
            vec![Value::Pid(pid.clone()), Value::Unit],
        ),
    )
    .unwrap();
    assert_eq!(
        vm.drive_builtin_outcome(stop).unwrap(),
        ok_vm_result(Value::Unit)
    );
    assert_eq!(vm.workers_size(&handle).unwrap(), Value::Int(int(1)));
    assert_eq!(
        vm.unique_live_supervisor_child_count("DynamicSupervisor"),
        1
    );
    let snapshot = vm.process_runtime_snapshot().unwrap();
    assert_eq!(snapshot.processes[0].acceptance, "stopping");
    assert_eq!(snapshot.processes[0].status, "waiting");
    let reserve = vm.workers_reserve(&handle).unwrap();
    assert_eq!(
        decode_vm_result(
            vm.drive_builtin_outcome(reserve).unwrap(),
            "test",
            "reserve"
        )
        .unwrap()
        .unwrap_err()
        .kind,
        "Global::WorkersUnavailable"
    );
    let broadcast = vm
        .start_workers_broadcast(&handle, builtin("print", vec![]), None)
        .unwrap();
    assert_eq!(
        vm.drive_builtin_outcome(broadcast).unwrap(),
        Value::List(ListHandle::from_items(vec![]))
    );
    let adopt = vm
        .supervisor_adopt("DynamicSupervisor".into(), pid.clone())
        .unwrap();
    assert_eq!(
        decode_vm_result(vm.drive_builtin_outcome(adopt).unwrap(), "test", "adopt")
            .unwrap()
            .unwrap_err()
            .kind,
        "Global::SupervisorAdoptWorkerNotLive"
    );
    let sleep = vm
        .process_runtime
        .executions
        .values()
        .find(|record| record.result_future == result_future)
        .unwrap()
        .internal_futures[0];
    vm.process_runtime
        .resolve_future(sleep, ok_vm_result(Value::Unit));
    assert_eq!(
        vm.drive_builtin_outcome(sleeping).unwrap(),
        ok_vm_result(Value::Unit)
    );
    assert!(vm.process_runtime.processes.is_empty());
    assert!(vm.process_runtime.futures.is_empty());
    assert_eq!(vm.workers_size(&handle).unwrap(), Value::Int(int(0)));
    assert_eq!(
        vm.unique_live_supervisor_child_count("DynamicSupervisor"),
        0
    );
}

#[test]
fn runtime_failure_cancels_same_instance_executions_and_preserves_other_instance() {
    use sindr::runtime::{TypeEntry, TypeKind};
    let (mut vm, pid) = worker_vm();
    vm.bytecode.type_registry.register(TypeEntry {
        tag: 200,
        name: "Duration".into(),
        kind: TypeKind::Struct,
        field_names: vec!["millis".into()],
        private_flags: vec![true],
    });
    let other_id = vm
        .allocate_process_state("Worker".into(), Some(Value::Unit))
        .unwrap();
    let other_pid = vm.process_handle(other_id).unwrap();
    let sleep_body = || {
        builtin(
            "__process_sleep",
            vec![Value::Tagged {
                tag: 200,
                fields: vec![Value::Int(int(1_000_000))],
            }],
        )
    };
    let first = start_synthetic_execution(&mut vm, pid.clone(), sleep_body()).unwrap();
    let other = start_synthetic_execution(&mut vm, other_pid.clone(), sleep_body()).unwrap();
    vm.drive_ready_detached_tasks().unwrap();
    let bad = start_synthetic_execution(
        &mut vm,
        pid.clone(),
        builtin(
            "__genserver_call_stop_error",
            vec![Value::Pid(pid.clone()), Value::Unit],
        ),
    )
    .unwrap();
    let error = vm.drive_builtin_outcome(bad).unwrap_err();
    assert!(error.message.contains("expects Error"));
    assert!(!vm.process_runtime.processes.contains_key(&pid.id));
    assert!(vm.process_runtime.processes.contains_key(&other_pid.id));
    assert!(!vm
        .process_runtime
        .executions
        .values()
        .any(|record| record.pid.id == pid.id));
    assert!(!vm
        .process_runtime
        .detached_tasks
        .values()
        .any(|task| task.owner_pid == Some(pid.id)));
    let stopped = decode_vm_result(
        vm.drive_builtin_outcome(first).unwrap(),
        "test",
        "aborted reply",
    )
    .unwrap()
    .unwrap_err();
    assert_eq!(stopped.kind, "Global::ProcessReplyTargetStopped");
    let sleep = vm
        .process_runtime
        .executions
        .values()
        .find(|record| record.pid.id == other_pid.id)
        .unwrap()
        .internal_futures[0];
    vm.process_runtime
        .resolve_future(sleep, ok_vm_result(Value::Unit));
    assert_eq!(
        vm.drive_builtin_outcome(other).unwrap(),
        ok_vm_result(Value::Unit)
    );
    assert!(vm.process_runtime.executions.is_empty());
    assert!(vm.process_runtime.detached_tasks.is_empty());
    assert!(vm.process_runtime.futures.is_empty());
}

#[test]
fn reclaimed_identity_validates_type_before_pointer_and_stopped_error() {
    let (mut vm, pid) = worker_vm();
    let stop = start_synthetic_execution(
        &mut vm,
        pid.clone(),
        builtin(
            "__genserver_call_stop_normal",
            vec![Value::Pid(pid.clone()), Value::Unit],
        ),
    )
    .unwrap();
    vm.drive_builtin_outcome(stop).unwrap();
    for (handle, expected) in [
        (
            PidHandle::new(pid.id, "Other".into(), PidKind::Worker),
            "Global::ProcessStatePidTypeMismatch",
        ),
        (
            PidHandle::new(pid.id, pid.process_name.clone(), PidKind::Singleton),
            "Global::ProcessStatePidTypeMismatch",
        ),
        (
            PidHandle::new(pid.id, pid.process_name.clone(), PidKind::Worker),
            "Global::ProcessStateUnknownPid",
        ),
        (pid.clone(), "Global::ProcessStopped"),
    ] {
        let outcome = start_synthetic_execution(
            &mut vm,
            handle,
            builtin("print", vec![Value::Str("must not run".into())]),
        )
        .unwrap();
        assert_eq!(
            decode_vm_result(
                vm.drive_builtin_outcome(outcome).unwrap(),
                "test",
                "reclaimed identity"
            )
            .unwrap()
            .unwrap_err()
            .kind,
            expected
        );
    }
}

fn constant_body(vm: &mut VM, value: Value) -> Callable {
    let pc = vm.bytecode.opcodes.len() as u32;
    vm.bytecode
        .opcodes
        .extend([Opcode::LoadLocal(0), Opcode::Return]);
    let id = vm.bytecode.functions.len() as u32;
    vm.bytecode.functions.push(super::tests::function_entry(
        id,
        pc,
        1,
        1,
        Some("Worker::constant_wrapper"),
    ));
    Callable {
        target: CallableTarget::Function(id),
        lexical_captures: vec![value],
        metadata: CallableMetadata::default(),
    }
}

#[test]
fn early_language_error_finishes_record_without_aborting_process() {
    let (mut vm, pid) = worker_vm();
    let error = vm.process_error("Boom", "handler rejected message");
    let body = constant_body(&mut vm, err_vm_result(error.clone()));
    let call = start_synthetic_execution(&mut vm, pid.clone(), body).unwrap();
    assert_eq!(
        vm.drive_builtin_outcome(call).unwrap(),
        err_vm_result(error)
    );
    assert_eq!(
        vm.process_runtime
            .processes
            .get(&pid.id)
            .unwrap()
            .acceptance,
        ProcessAcceptance::Accepting
    );
    assert!(vm.process_runtime.executions.is_empty());
    assert!(vm.process_runtime.detached_tasks.is_empty());
    assert!(vm.process_runtime.futures.is_empty());
}

#[test]
fn registered_snapshot_and_single_store_survive_stop_before_first_dispatch() {
    let (mut vm, pid) = worker_vm();
    vm.process_runtime
        .processes
        .get_mut(&pid.id)
        .unwrap()
        .state_value = Some(Value::Int(int(41)));
    let body = constant_body(&mut vm, ok_vm_result(Value::Int(int(77))));
    let pending = start_synthetic_execution(&mut vm, pid.clone(), body).unwrap();
    let execution_id = vm
        .process_runtime
        .executions
        .iter()
        .next()
        .unwrap()
        .0
        .to_owned();
    let task_id = vm
        .process_runtime
        .detached_tasks
        .iter()
        .next()
        .unwrap()
        .0
        .to_owned();
    let task = vm.process_runtime.detached_tasks.take(&task_id).unwrap();
    let stop = start_synthetic_execution(
        &mut vm,
        pid.clone(),
        builtin(
            "__genserver_call_stop_normal",
            vec![Value::Pid(pid.clone()), Value::Unit],
        ),
    )
    .unwrap();
    vm.drive_builtin_outcome(stop).unwrap();
    assert_eq!(
        vm.process_runtime
            .processes
            .get(&pid.id)
            .unwrap()
            .acceptance,
        ProcessAcceptance::Stopping
    );
    vm.current_process_execution = Some(execution_id);
    assert!(
        matches!(vm.process_state(&pid).unwrap(), BuiltinOutcome::Complete(value) if value == ok_vm_result(Value::Int(int(41))))
    );
    assert!(vm
        .process_state(&pid)
        .unwrap_err()
        .message
        .contains("already read"));
    vm.process_postprocess(&pid).unwrap();
    assert!(
        matches!(vm.process_store(&pid, Value::Int(int(99))).unwrap(), BuiltinOutcome::Complete(value) if value == ok_vm_result(Value::Unit))
    );
    assert!(vm
        .process_store(&pid, Value::Int(int(100)))
        .unwrap_err()
        .message
        .contains("unsaved postprocessing"));
    vm.current_process_execution = None;
    let denied = start_synthetic_execution(
        &mut vm,
        pid.clone(),
        builtin("print", vec![Value::Str("must not run".into())]),
    )
    .unwrap();
    assert_eq!(
        decode_vm_result(
            vm.drive_builtin_outcome(denied).unwrap(),
            "test",
            "new child"
        )
        .unwrap()
        .unwrap_err()
        .kind,
        "Global::ProcessStopped"
    );
    vm.process_runtime.detached_tasks.insert(task_id, task);
    assert_eq!(
        vm.drive_builtin_outcome(pending).unwrap(),
        ok_vm_result(Value::Int(int(77)))
    );
    assert!(vm.process_runtime.processes.is_empty());
    assert!(vm.process_runtime.executions.is_empty());
}

#[test]
fn independent_task_started_by_handler_outlives_process_stop() {
    use sindr::runtime::{TypeEntry, TypeKind};
    let (mut vm, pid) = worker_vm();
    vm.bytecode.type_registry.register(TypeEntry {
        tag: 200,
        name: "Duration".into(),
        kind: TypeKind::Struct,
        field_names: vec!["millis".into()],
        private_flags: vec![true],
    });
    let independent = builtin(
        "__process_sleep",
        vec![Value::Tagged {
            tag: 200,
            fields: vec![Value::Int(int(1_000_000))],
        }],
    );
    let call = start_synthetic_execution(
        &mut vm,
        pid.clone(),
        builtin("__task_launch", vec![Value::Callable(independent)]),
    )
    .unwrap();
    assert_eq!(
        vm.drive_builtin_outcome(call).unwrap(),
        ok_vm_result(Value::Unit)
    );
    assert!(vm.process_runtime.executions.is_empty());
    let task_sleep = vm
        .process_runtime
        .detached_tasks
        .values()
        .find_map(|task| match &task.state {
            process_continuation::DetachedTaskState::Waiting { future_id, context } => {
                if let Some(context) = context {
                    assert!(context.current_process_execution.is_none());
                }
                Some(*future_id)
            }
            _ => None,
        })
        .unwrap();
    let stop = start_synthetic_execution(
        &mut vm,
        pid.clone(),
        builtin(
            "__genserver_call_stop_normal",
            vec![Value::Pid(pid.clone()), Value::Unit],
        ),
    )
    .unwrap();
    vm.drive_builtin_outcome(stop).unwrap();
    assert!(vm.process_runtime.processes.is_empty());
    assert!(!vm.process_runtime.detached_tasks.is_empty());
    vm.process_runtime
        .resolve_future(task_sleep, ok_vm_result(Value::Unit));
    vm.drive_ready_detached_tasks().unwrap();
    assert!(vm.process_runtime.detached_tasks.is_empty());
}

#[test]
fn reply_later_callback_does_not_inherit_wrapper_store_authority() {
    let (mut vm, pid) = worker_vm();
    let callback = builtin(
        "__process_store",
        vec![Value::Pid(pid.clone()), Value::Int(int(99))],
    );
    let body = builtin(
        "__genserver_call_reply_later",
        vec![
            Value::Pid(pid.clone()),
            Value::Int(int(41)),
            Value::Callable(callback),
        ],
    );
    let call = start_synthetic_execution(&mut vm, pid.clone(), body).unwrap();
    let error = vm.drive_builtin_outcome(call).unwrap_err();
    assert!(error.message.contains("unsaved postprocessing execution"));
    assert!(vm.process_runtime.processes.is_empty());
    assert!(vm.process_runtime.executions.is_empty());
    assert!(vm.process_runtime.detached_tasks.is_empty());
    assert!(vm.process_runtime.futures.is_empty());
}

#[test]
fn process_store_rejects_handling_before_initial_state_read() {
    let (mut vm, pid) = worker_vm();
    let body = constant_body(&mut vm, ok_vm_result(Value::Unit));
    start_synthetic_execution(&mut vm, pid.clone(), body).unwrap();
    let id = *vm.process_runtime.executions.iter().next().unwrap().0;
    vm.current_process_execution = Some(id);
    assert!(vm.process_store(&pid, Value::Int(int(99))).is_err());
}

#[test]
fn process_store_rejects_handler_after_state_read_before_postprocessing_boundary() {
    let (mut vm, pid) = worker_vm();
    let body = constant_body(&mut vm, ok_vm_result(Value::Unit));
    start_synthetic_execution(&mut vm, pid.clone(), body).unwrap();
    let id = *vm.process_runtime.executions.iter().next().unwrap().0;
    vm.current_process_execution = Some(id);
    vm.process_state(&pid).unwrap();
    assert!(vm.process_store(&pid, Value::Int(int(99))).is_err());
}

#[test]
fn process_state_rejects_call_outside_managed_execution() {
    let (mut vm, pid) = worker_vm();
    assert!(vm.process_state(&pid).is_err());
}

#[test]
fn postprocessing_marker_rejects_unread_state_duplicate_and_callback() {
    let (mut vm, pid) = worker_vm();
    let body = constant_body(&mut vm, ok_vm_result(Value::Unit));
    vm.start_process_execution(pid.clone(), body).unwrap();
    let id = *vm.process_runtime.executions.iter().next().unwrap().0;
    vm.current_process_execution = Some(id);
    assert!(vm.process_postprocess(&pid).is_err());
    vm.process_state(&pid).unwrap();
    assert_eq!(vm.process_postprocess(&pid).unwrap(), Value::Unit);
    assert!(vm.process_postprocess(&pid).is_err());
    assert!(vm.process_state(&pid).is_err());
    vm.process_runtime.executions.get_mut(&id).unwrap().stage = ProcessExecutionStage::Callback;
    assert!(vm.process_postprocess(&pid).is_err());
    assert!(vm.process_store(&pid, Value::Unit).is_err());
}

#[test]
fn reply_later_callback_cannot_enter_postprocessing_again() {
    let (mut vm, pid) = worker_vm();
    let body = builtin(
        "__genserver_call_reply_later",
        vec![
            Value::Pid(pid.clone()),
            Value::Unit,
            Value::Callable(builtin(
                "__process_postprocess",
                vec![Value::Pid(pid.clone())],
            )),
        ],
    );
    let call = start_synthetic_execution(&mut vm, pid.clone(), body).unwrap();
    assert!(vm
        .drive_builtin_outcome(call)
        .unwrap_err()
        .message
        .contains("matching handler"));
    assert!(vm.process_runtime.processes.is_empty());
    assert!(vm.process_runtime.executions.is_empty());
    assert!(vm.process_runtime.detached_tasks.is_empty());
    assert!(vm.process_runtime.futures.is_empty());
}

#[test]
fn competing_started_stops_keep_first_reason_and_each_execution_reply() {
    use sindr::runtime::{TypeEntry, TypeKind};
    for first_error in [false, true] {
        let (mut vm, pid) = worker_vm();
        vm.bytecode.type_registry.register(TypeEntry {
            tag: 200,
            name: "Duration".into(),
            kind: TypeKind::Struct,
            field_names: vec!["millis".into()],
            private_flags: vec![true],
        });
        let sleeping = vm
            .start_process_execution(
                pid.clone(),
                builtin(
                    "__process_sleep",
                    vec![Value::Tagged {
                        tag: 200,
                        fields: vec![Value::Int(int(1_000_000))],
                    }],
                ),
            )
            .unwrap();
        let first_body = if first_error {
            builtin(
                "__genserver_call_stop_error",
                vec![
                    Value::Pid(pid.clone()),
                    Value::Error(Box::new(vm.process_error("FirstStop", "first"))),
                ],
            )
        } else {
            builtin(
                "__genserver_call_stop_normal",
                vec![Value::Pid(pid.clone()), Value::Int(int(111))],
            )
        };
        let second_body = if first_error {
            builtin(
                "__genserver_call_stop_normal",
                vec![Value::Pid(pid.clone()), Value::Int(int(222))],
            )
        } else {
            builtin(
                "__genserver_call_stop_error",
                vec![
                    Value::Pid(pid.clone()),
                    Value::Error(Box::new(vm.process_error("SecondStop", "second"))),
                ],
            )
        };
        let first = start_synthetic_execution(&mut vm, pid.clone(), first_body).unwrap();
        let second = start_synthetic_execution(&mut vm, pid.clone(), second_body).unwrap();
        let first_value =
            decode_vm_result(vm.drive_builtin_outcome(first).unwrap(), "test", "first").unwrap();
        let second_value =
            decode_vm_result(vm.drive_builtin_outcome(second).unwrap(), "test", "second").unwrap();
        if first_error {
            assert_eq!(first_value.unwrap_err().kind, "Global::FirstStop");
            assert_eq!(second_value.unwrap(), Value::Int(int(222)));
            assert!(
                matches!(vm.process_runtime.processes.get(&pid.id).unwrap().stop_reason.as_ref(), Some(ProcessStopReason::Error(error)) if error.kind == "Global::FirstStop")
            );
        } else {
            assert_eq!(first_value.unwrap(), Value::Int(int(111)));
            assert_eq!(second_value.unwrap_err().kind, "Global::SecondStop");
            assert!(matches!(
                vm.process_runtime
                    .processes
                    .get(&pid.id)
                    .unwrap()
                    .stop_reason
                    .as_ref(),
                Some(ProcessStopReason::Normal)
            ));
        }
        let sleep = vm
            .process_runtime
            .executions
            .values()
            .next()
            .unwrap()
            .internal_futures[0];
        vm.process_runtime
            .resolve_future(sleep, ok_vm_result(Value::Unit));
        vm.drive_builtin_outcome(sleeping).unwrap();
        assert!(vm.process_runtime.processes.is_empty());
        assert!(vm.process_runtime.executions.is_empty());
        assert!(vm.process_runtime.futures.is_empty());
    }
}

#[test]
fn nested_started_execution_updates_follow_wrapper_completion_order() {
    let (mut vm, pid) = worker_vm();
    let outer_body = constant_body(&mut vm, ok_vm_result(Value::Int(int(1))));
    let outer = vm.start_process_execution(pid.clone(), outer_body).unwrap();
    let outer_id = *vm.process_runtime.executions.iter().next().unwrap().0;
    vm.current_process_execution = Some(outer_id);
    vm.process_state(&pid).unwrap();
    let inner_body = constant_body(&mut vm, ok_vm_result(Value::Int(int(2))));
    let inner = vm.start_process_execution(pid.clone(), inner_body).unwrap();
    let inner_id = *vm
        .process_runtime
        .executions
        .iter()
        .find(|(id, _)| **id != outer_id)
        .unwrap()
        .0;
    assert_eq!(
        vm.process_runtime.executions.get(&inner_id).unwrap().parent,
        Some(outer_id)
    );
    vm.current_process_execution = Some(inner_id);
    vm.process_state(&pid).unwrap();
    vm.process_postprocess(&pid).unwrap();
    vm.process_store(&pid, Value::Int(int(99))).unwrap();
    vm.current_process_execution = Some(outer_id);
    vm.process_postprocess(&pid).unwrap();
    vm.process_store(&pid, Value::Int(int(100))).unwrap();
    assert_eq!(
        vm.process_runtime
            .processes
            .get(&pid.id)
            .unwrap()
            .state_value,
        Some(Value::Int(int(100)))
    );
    vm.current_process_execution = None;
    assert_eq!(
        vm.drive_builtin_outcome(inner).unwrap(),
        ok_vm_result(Value::Int(int(2)))
    );
    assert_eq!(
        vm.drive_builtin_outcome(outer).unwrap(),
        ok_vm_result(Value::Int(int(1)))
    );
    assert!(vm.process_runtime.executions.is_empty());
    assert!(vm.process_runtime.futures.is_empty());
}

// A normal generated wrapper: snapshot first, an unfinished handler wait, then
// the compiler postprocessing marker, one save, and a reply. No ReplyLater.
fn ordinary_waiting_wrapper(vm: &mut VM, pid: &PidHandle) -> Callable {
    use sindr::runtime::{TypeEntry, TypeKind};
    vm.bytecode.type_registry.register(TypeEntry {
        tag: 200,
        name: "Duration".into(),
        kind: TypeKind::Struct,
        field_names: vec!["millis".into()],
        private_flags: vec![true],
    });
    let pc = vm.bytecode.opcodes.len() as u32;
    let call = |name, arity| Opcode::CallBuiltin {
        builtin_id: sindr::builtin::builtin_id_by_name(name).unwrap(),
        arity,
        span_start: 0,
        span_end: 0,
    };
    vm.bytecode.opcodes.extend([
        Opcode::LoadLocal(0),
        call("__process_state", 1),
        Opcode::Pop,
        Opcode::LoadLocal(1),
        call("__process_sleep", 1),
        Opcode::Pop,
        Opcode::LoadLocal(2),
        call("print", 1),
        Opcode::Pop,
        Opcode::LoadLocal(0),
        call("__process_postprocess", 1),
        Opcode::Pop,
        Opcode::LoadLocal(0),
        Opcode::LoadLocal(3),
        Opcode::LoadLocal(4),
        call("__genserver_call_reply", 3),
        Opcode::Return,
    ]);
    let id = vm.bytecode.functions.len() as u32;
    vm.bytecode.functions.push(super::tests::function_entry(
        id,
        pc,
        5,
        5,
        Some("Worker::ordinary_timed_wrapper"),
    ));
    Callable {
        target: CallableTarget::Function(id),
        lexical_captures: vec![
            Value::Pid(pid.clone()),
            Value::Tagged {
                tag: 200,
                fields: vec![Value::Int(int(1_000_000))],
            },
            Value::Str("handler completed once".into()),
            Value::Int(int(99)),
            Value::Int(int(42)),
        ],
        metadata: CallableMetadata::default(),
    }
}

fn timed_process_caller(vm: &mut VM, pid: &PidHandle, body: Callable) -> BuiltinOutcome {
    vm.start_task(
        builtin(
            "__process_execute",
            vec![Value::Pid(pid.clone()), Value::Callable(body)],
        ),
        TaskMode::Call,
        Some(1_000_000),
    )
    .unwrap()
}

fn caller_completion(outcome: &BuiltinOutcome) -> FutureId {
    match outcome {
        BuiltinOutcome::Wait { future_id, .. } => *future_id,
        _ => panic!("timed caller must wait"),
    }
}

fn expire_caller_now(vm: &mut VM, future: FutureId) {
    let now = vm.process_runtime.current_tick_ms;
    vm.process_runtime
        .attach_future_deadline(future, now, 0, true);
    vm.expire_process_deadlines(now);
}

#[test]
fn timed_caller_deadline_before_dispatch_does_not_start_process_handler() {
    let (vm, pid) = worker_vm();
    let mut vm = vm.with_output_capture();
    vm.process_runtime
        .processes
        .get_mut(&pid.id)
        .unwrap()
        .state_value = Some(Value::Int(int(0)));
    let body = ordinary_waiting_wrapper(&mut vm, &pid);
    let caller = timed_process_caller(&mut vm, &pid, body);
    let completion = caller_completion(&caller);
    expire_caller_now(&mut vm, completion);
    let value = vm.drive_builtin_outcome(caller).unwrap();
    assert_eq!(
        decode_vm_result(value.clone(), "test", "before dispatch")
            .unwrap()
            .unwrap_err()
            .kind,
        "Global::FutureDeadlineExceeded"
    );
    assert_eq!(vm.raw_ready_future_value(completion), Some(value));
    assert_eq!(
        vm.process_runtime
            .processes
            .get(&pid.id)
            .unwrap()
            .state_value,
        Some(Value::Int(int(0)))
    );
    assert_eq!(vm.output.as_deref(), Some([].as_slice()));
    assert!(vm.process_runtime.executions.is_empty());
    assert!(vm.process_runtime.detached_tasks.is_empty());
    assert!(vm.process_runtime.deadline_queue.is_empty());
}

#[test]
fn started_normal_handler_saves_once_after_timed_caller_cancellation() {
    let (vm, pid) = worker_vm();
    let mut vm = vm.with_output_capture();
    vm.process_runtime
        .processes
        .get_mut(&pid.id)
        .unwrap()
        .state_value = Some(Value::Int(int(0)));
    let body = ordinary_waiting_wrapper(&mut vm, &pid);
    let caller = timed_process_caller(&mut vm, &pid, body);
    let completion = caller_completion(&caller);

    // Dispatch the caller, then its independent callee, without expiring time.
    // Arm the deadline only after a real wrapper frame reached its handler wait.
    vm.drive_ready_detached_tasks().unwrap();
    vm.drive_ready_detached_tasks().unwrap();
    let (execution_id, sleep_future) = vm
        .process_runtime
        .detached_tasks
        .values()
        .find_map(|task| match &task.state {
            process_continuation::DetachedTaskState::Waiting {
                future_id,
                context: Some(context),
            } if task.owner_pid == Some(pid.id) => {
                Some((context.current_process_execution.unwrap(), *future_id))
            }
            _ => None,
        })
        .expect("started normal wrapper must be waiting in its own context");
    let record = vm.process_runtime.executions.get(&execution_id).unwrap();
    assert_eq!(record.stage, ProcessExecutionStage::Handling);
    assert!(record.initial_state.is_none());
    assert!(!record.stored);
    let managed_result = record.result_future;
    assert_ne!(completion, managed_result);
    assert_ne!(sleep_future, managed_result);

    expire_caller_now(&mut vm, completion);
    let timeout = vm.drive_builtin_outcome(caller).unwrap();
    assert_eq!(
        decode_vm_result(timeout.clone(), "test", "started handler")
            .unwrap()
            .unwrap_err()
            .kind,
        "Global::FutureDeadlineExceeded"
    );
    assert_eq!(vm.raw_ready_future_value(completion), Some(timeout.clone()));
    assert!(vm.process_runtime.executions.get(&execution_id).is_some());
    assert!(
        vm.process_runtime.futures.get(&managed_result).is_none(),
        "cancelled caller releases its wait result without cancelling the callee"
    );
    assert_eq!(
        vm.process_runtime
            .processes
            .get(&pid.id)
            .unwrap()
            .state_value,
        Some(Value::Int(int(0)))
    );
    assert_eq!(vm.output.as_deref(), Some([].as_slice()));

    // The caller has already received its timeout and progressed. Complete the
    // handler wait explicitly; the late save/reply cannot change that result.
    vm.process_runtime
        .resolve_future(sleep_future, ok_vm_result(Value::Unit));
    vm.drive_ready_detached_tasks().unwrap();
    assert_eq!(
        vm.process_runtime
            .processes
            .get(&pid.id)
            .unwrap()
            .state_value,
        Some(Value::Int(int(99)))
    );
    assert_eq!(
        vm.output.as_deref(),
        Some(["handler completed once".to_owned()].as_slice())
    );
    assert_eq!(vm.raw_ready_future_value(completion), Some(timeout.clone()));
    assert!(vm.process_runtime.executions.is_empty());
    assert!(vm.process_runtime.detached_tasks.is_empty());
    assert!(vm.process_runtime.waiting_table.is_empty());
    assert!(vm.process_runtime.reply_table.is_empty());
    assert!(vm.process_runtime.deadline_queue.is_empty());
    assert_eq!(
        vm.process_runtime.futures.len(),
        1,
        "retain only the caller's confirmed result"
    );
    vm.drive_ready_detached_tasks().unwrap();
    assert_eq!(
        vm.output.as_deref(),
        Some(["handler completed once".to_owned()].as_slice())
    );
    assert_eq!(vm.raw_ready_future_value(completion), Some(timeout));
}
