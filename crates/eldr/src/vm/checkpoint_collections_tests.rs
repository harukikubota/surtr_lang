use super::process_continuation::test_singleton_flight;
use super::tests::{base_bytecode, function_entry, test_runtime_process_spec};
use super::*;
use std::hint::black_box;
use std::ptr;
use std::time::Instant;

fn policy() -> RuntimeSupervisorPolicy {
    RuntimeSupervisorPolicy {
        strategy: "OneForOne".into(),
        max_restarts: 5,
        max_seconds: 10,
        child_restart_default: "Transient".into(),
        allow_adopt: true,
        shutdown_timeout_ms: None,
    }
}

fn spec(id: u32, name: &str) -> RuntimeProcessSpec {
    let mut spec = test_runtime_process_spec(
        id,
        name,
        RuntimeProcessKind::Agent,
        RuntimeProcessInstance::Worker,
        false,
        0,
        0,
        None,
    );
    spec.supervision.policy = Some(policy());
    spec.dependencies
        .handlers
        .push(sindr::ir::RuntimeHandlerDependency {
            slot: "output".into(),
            capability: "Write".into(),
            default_target: RuntimeHandlerTarget {
                name: format!("{name}Output"),
                named_args: vec![sindr::ir::RuntimeHandlerArg {
                    name: "path".into(),
                    value: "x".repeat(256),
                }],
            },
        });
    spec
}

fn metadata_bytecode(n: usize) -> Bytecode {
    let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::Return]);
    bytecode.functions = vec![function_entry(0, 1, 1, 0, Some("Collections::init"))];
    bytecode.runtime_process_specs.entries = (0..n)
        .map(|id| spec(id as u32, &format!("Process{id:06}")))
        .collect();
    bytecode
}

pub(super) fn metadata_vm(n: usize) -> VM {
    VM::new(metadata_bytecode(n))
}

// Synthetic container state isolates checkpoint ownership from scheduler work.
// These fake IDs are never executed by a process or used as actual waiters.
fn rich_vm(n: usize) -> VM {
    let mut vm = metadata_vm(n);
    let init = vm.callable_for_function(0);
    for id in 0..n {
        let id = id as u64;
        let name = format!("Process{id:06}");
        vm.process_runtime
            .singleton_by_name
            .insert(name.clone(), id);
        vm.process_runtime
            .singleton_inits
            .insert(name.clone(), test_singleton_flight(id));
        vm.process_runtime.refilling_worker_sets.insert(id);
        vm.process_runtime.reply_table.insert(id, id);
        vm.process_runtime
            .waiting_table
            .insert(id, ProcessWaitReason::Future(id));
        vm.process_runtime.deadline_queue.push_back(DeadlineEntry {
            deadline_tick: 1000 + id,
            future_id: id,
        });
        vm.process_runtime.run_queue.push_back(id);
        vm.process_runtime
            .output_events
            .push_back(RuntimeOutputEvent::StdOut("x".repeat(256)));
        vm.process_runtime
            .root_supervisor
            .boot_failures
            .insert(name.clone(), "x".repeat(256));
        vm.process_runtime
            .root_supervisor
            .child_table
            .insert(name.clone(), vec![0, 1, 2]);
        vm.process_runtime.worker_sets.insert(
            id,
            WorkerSetState {
                supervisor_name: name.clone(),
                worker_process: name,
                init_callable: init.clone(),
                strategy: WorkerStrategyState {
                    init: 3,
                    min: 0,
                    max: 3,
                    scale: WorkerScaleState::Fix(3),
                },
                target: 3,
                members: vec![0, 1, 2],
                next_index: 0,
            },
        );
    }
    vm
}

pub(super) fn chunk(vm: &VM, fail: bool) -> BytecodeChunk {
    BytecodeChunk {
        opcodes: if fail {
            vec![
                Opcode::RuntimeContractViolation {
                    message: "checkpoint test runtime failure".into(),
                },
                Opcode::Halt,
            ]
        } else {
            vec![Opcode::Halt]
        },
        source_map: None,
        const_base: vm.bytecode.constants.len() as u32,
        constants: Vec::new(),
        new_locals: 0,
        type_registry_base: vm.bytecode.type_registry.entries().len() as u32,
        type_entries: Vec::new(),
        error_template_base: vm.bytecode.error_templates.len() as u32,
        error_templates: Vec::new(),
        dbg_template_base: vm.bytecode.dbg_templates.len() as u32,
        dbg_templates: Vec::new(),
        callable_templates: Vec::new(),
        functions: Vec::new(),
        docs: Vec::new(),
        signatures: Vec::new(),
        runtime_process_specs: Vec::new(),
        runtime_boot_plan: Default::default(),
    }
}

fn checkpoint(vm: &VM) -> VmCheckpoint {
    vm.checkpoint_for_chunk(&chunk(vm, false))
}

fn assert_shared(live: &ProcessRuntime, saved: &ProcessRuntime) {
    macro_rules! shared_map {
        ($($field:ident).+) => {
            assert!(ptr::eq(
                live.$($field).+.iter().next().unwrap().1,
                saved.$($field).+.iter().next().unwrap().1,
            ), stringify!($($field).+));
        };
    }
    shared_map!(handler_contexts);
    shared_map!(singleton_by_name);
    shared_map!(singleton_inits);
    shared_map!(reply_table);
    shared_map!(waiting_table);
    shared_map!(root_supervisor.boot_failures);
    shared_map!(root_supervisor.effective_supervisors);
    shared_map!(root_supervisor.child_table);
    assert!(ptr::eq(
        live.refilling_worker_sets.iter().next().unwrap(),
        saved.refilling_worker_sets.iter().next().unwrap()
    ));
    assert!(ptr::eq(
        live.deadline_queue.as_slices().0.as_ptr(),
        saved.deadline_queue.as_slices().0.as_ptr()
    ));
    assert!(ptr::eq(
        live.run_queue.as_slices().0.as_ptr(),
        saved.run_queue.as_slices().0.as_ptr()
    ));
    assert!(ptr::eq(
        live.output_events.as_slices().0.as_ptr(),
        saved.output_events.as_slices().0.as_ptr()
    ));
    assert!(ptr::eq(
        live.worker_sets.get(&0).unwrap(),
        saved.worker_sets.get(&0).unwrap()
    ));
}

fn mutate(runtime: &mut ProcessRuntime, generation: u64) {
    let name = "Process000000";
    runtime
        .handler_contexts
        .get_mut(name)
        .unwrap()
        .get_mut("output")
        .unwrap()
        .name
        .push('!');
    *runtime.singleton_by_name.get_mut(name).unwrap() += 1;
    runtime.singleton_inits.get_mut(name).unwrap().future_id += 1;
    runtime.refilling_worker_sets.insert(u64::MAX - generation);
    *runtime.reply_table.get_mut(&0).unwrap() += 1;
    runtime
        .waiting_table
        .insert(0, ProcessWaitReason::Reply(generation));
    runtime.deadline_queue.front_mut().unwrap().future_id += 1;
    runtime.deadline_queue.push_back(DeadlineEntry {
        deadline_tick: 2000 + generation,
        future_id: 1000 + generation,
    });
    *runtime.run_queue.front_mut().unwrap() += 1;
    runtime.run_queue.push_back(1000 + generation);
    let RuntimeOutputEvent::StdOut(text) = runtime.output_events.front_mut().unwrap() else {
        panic!("stdout expected")
    };
    text.push('!');
    runtime
        .output_events
        .push_back(RuntimeOutputEvent::StdErr(format!("event{generation}")));
    runtime
        .root_supervisor
        .boot_failures
        .get_mut(name)
        .unwrap()
        .push('!');
    runtime
        .root_supervisor
        .effective_supervisors
        .get_mut(name)
        .unwrap()
        .max_restarts += 1;
    runtime
        .root_supervisor
        .child_table
        .get_mut(name)
        .unwrap()
        .push(100 + generation);
    runtime
        .worker_sets
        .get_mut(&0)
        .unwrap()
        .members
        .push(100 + generation);
}

fn assert_generation(runtime: &ProcessRuntime, n: usize, generation: u64) {
    let name = "Process000000";
    assert_eq!(
        runtime.handler_contexts[name]["output"].name,
        format!("{name}Output{}", "!".repeat(generation as usize))
    );
    assert_eq!(runtime.singleton_by_name[name], generation);
    assert_eq!(runtime.singleton_inits[name].future_id, generation);
    assert_eq!(runtime.refilling_worker_sets.len(), n + generation as usize);
    assert_eq!(runtime.reply_table[&0], generation);
    assert_eq!(
        runtime.waiting_table.get(&0),
        Some(&if generation == 0 {
            ProcessWaitReason::Future(0)
        } else {
            ProcessWaitReason::Reply(generation)
        })
    );
    let expected_ids = std::iter::once(generation)
        .chain(1..n as u64)
        .chain((1..=generation).map(|id| 1000 + id))
        .collect::<Vec<_>>();
    assert_eq!(
        runtime.run_queue.iter().copied().collect::<Vec<_>>(),
        expected_ids
    );
    assert_eq!(
        runtime
            .deadline_queue
            .iter()
            .map(|entry| entry.future_id)
            .collect::<Vec<_>>(),
        expected_ids
    );
    assert_eq!(runtime.output_events.len(), n + generation as usize);
    assert_eq!(
        runtime.output_events.front(),
        Some(&RuntimeOutputEvent::StdOut(format!(
            "{}{}",
            "x".repeat(256),
            "!".repeat(generation as usize)
        )))
    );
    for id in 1..=generation {
        assert_eq!(
            runtime.output_events.get(n + id as usize - 1),
            Some(&RuntimeOutputEvent::StdErr(format!("event{id}")))
        );
    }
    assert_eq!(
        runtime.root_supervisor.boot_failures[name],
        format!("{}{}", "x".repeat(256), "!".repeat(generation as usize))
    );
    assert_eq!(
        runtime.root_supervisor.effective_supervisors[name].max_restarts,
        5 + generation
    );
    let expected_members = [0, 1, 2]
        .into_iter()
        .chain((1..=generation).map(|id| 100 + id))
        .collect::<Vec<_>>();
    assert_eq!(runtime.root_supervisor.child_table[name], expected_members);
    assert_eq!(
        runtime.worker_sets.get(&0).unwrap().members,
        expected_members
    );
}

#[test]
fn unchanged_checkpoints_share_all_runtime_collections() {
    let vm = rich_vm(4);
    let first = checkpoint(&vm);
    let second = checkpoint(&vm);
    assert_shared(&vm.process_runtime, &first.process_runtime);
    assert_shared(&vm.process_runtime, &second.process_runtime);
    assert_shared(&first.process_runtime, &second.process_runtime);
    let cloned = vm.clone();
    assert_shared(&vm.process_runtime, &cloned.process_runtime);
    drop(first);
    drop(second);
    assert_generation(&vm.process_runtime, 4, 0);
}

#[test]
fn collection_mutations_detach_and_two_checkpoints_restore_deep_values_and_order() {
    let mut vm = rich_vm(4);
    let first = checkpoint(&vm);
    mutate(&mut vm.process_runtime, 1);
    assert_generation(&first.process_runtime, 4, 0);
    assert_generation(&vm.process_runtime, 4, 1);
    assert!(!ptr::eq(
        vm.process_runtime.worker_sets.get(&0).unwrap(),
        first.process_runtime.worker_sets.get(&0).unwrap()
    ));
    assert!(
        ptr::eq(
            vm.process_runtime.worker_sets.get(&1).unwrap(),
            first.process_runtime.worker_sets.get(&1).unwrap()
        ),
        "unmodified worker entry must stay shared"
    );
    let second = checkpoint(&vm);
    mutate(&mut vm.process_runtime, 2);
    assert_generation(&second.process_runtime, 4, 1);
    assert_generation(&first.process_runtime, 4, 0);
    assert_generation(&vm.process_runtime, 4, 2);
    vm.rollback_to_checkpoint(second).unwrap();
    assert_generation(&vm.process_runtime, 4, 1);
    vm.rollback_to_checkpoint(first).unwrap();
    assert_generation(&vm.process_runtime, 4, 0);
}

#[test]
fn missing_map_keys_and_set_members_do_not_detach_shared_storage() {
    let mut vm = rich_vm(4);
    let saved = checkpoint(&vm);
    assert!(vm
        .process_runtime
        .handler_contexts
        .get_mut("missing")
        .is_none());
    assert!(vm
        .process_runtime
        .handler_contexts
        .remove("missing")
        .is_none());
    assert!(vm
        .process_runtime
        .singleton_by_name
        .get_mut("missing")
        .is_none());
    assert!(vm
        .process_runtime
        .singleton_by_name
        .remove("missing")
        .is_none());
    assert!(vm
        .process_runtime
        .singleton_inits
        .get_mut("missing")
        .is_none());
    assert!(vm
        .process_runtime
        .singleton_inits
        .remove("missing")
        .is_none());
    assert!(!vm.process_runtime.refilling_worker_sets.remove(&u64::MAX));
    assert!(vm.process_runtime.reply_table.remove(&u64::MAX).is_none());
    assert!(vm.process_runtime.waiting_table.remove(&u64::MAX).is_none());
    assert!(vm
        .process_runtime
        .root_supervisor
        .boot_failures
        .get_mut("missing")
        .is_none());
    assert!(vm
        .process_runtime
        .root_supervisor
        .child_table
        .get_mut("missing")
        .is_none());
    assert!(vm
        .process_runtime
        .root_supervisor
        .effective_supervisors
        .get_mut("missing")
        .is_none());
    assert!(vm.process_runtime.worker_sets.get_mut(&u64::MAX).is_none());
    assert_shared(&vm.process_runtime, &saved.process_runtime);
}

#[test]
fn empty_queue_pops_do_not_detach_allocated_shared_buffers() {
    let mut vm = rich_vm(4);
    vm.process_runtime.deadline_queue.clear();
    vm.process_runtime.run_queue.clear();
    vm.process_runtime.output_events.clear();
    let saved = checkpoint(&vm);
    assert!(vm.process_runtime.deadline_queue.pop_front().is_none());
    assert!(vm.process_runtime.run_queue.pop_front().is_none());
    assert!(vm.process_runtime.output_events.pop_front().is_none());
    assert!(ptr::eq(
        vm.process_runtime.deadline_queue.as_slices().0.as_ptr(),
        saved.process_runtime.deadline_queue.as_slices().0.as_ptr()
    ));
    assert!(ptr::eq(
        vm.process_runtime.run_queue.as_slices().0.as_ptr(),
        saved.process_runtime.run_queue.as_slices().0.as_ptr()
    ));
    assert!(ptr::eq(
        vm.process_runtime.output_events.as_slices().0.as_ptr(),
        saved.process_runtime.output_events.as_slices().0.as_ptr()
    ));
}

#[test]
fn worker_removal_detaches_only_member_changes_and_index_corrections() {
    let mut vm = rich_vm(4);
    vm.process_runtime.refilling_worker_sets.clear();
    let removed = 99;
    {
        let state = vm.process_runtime.worker_sets.get_mut(&0).unwrap();
        state.members = vec![1, removed, 2, removed];
        state.next_index = 3;
        state.target = 0;
    }
    {
        let state = vm.process_runtime.worker_sets.get_mut(&1).unwrap();
        state.members = vec![1, 2];
        state.next_index = 8;
        state.target = 0;
    }
    vm.process_runtime
        .worker_sets
        .get_mut(&2)
        .unwrap()
        .next_index = 1;
    vm.process_runtime
        .worker_sets
        .get_mut(&3)
        .unwrap()
        .members
        .clear();
    let saved = checkpoint(&vm);
    vm.remove_worker_from_sets(removed);

    let member_change = vm.process_runtime.worker_sets.get(&0).unwrap();
    assert_eq!(member_change.members, vec![1, 2]);
    assert_eq!(member_change.next_index, 0);
    assert_eq!(member_change.target, 0);
    let correction = vm.process_runtime.worker_sets.get(&1).unwrap();
    assert_eq!(correction.members, vec![1, 2]);
    assert_eq!(correction.next_index, 0);
    assert_eq!(correction.target, 0);
    for id in [0, 1] {
        assert!(!ptr::eq(
            vm.process_runtime.worker_sets.get(&id).unwrap(),
            saved.process_runtime.worker_sets.get(&id).unwrap()
        ));
    }
    for id in [2, 3] {
        assert!(
            ptr::eq(
                vm.process_runtime.worker_sets.get(&id).unwrap(),
                saved.process_runtime.worker_sets.get(&id).unwrap()
            ),
            "unrelated set {id} must remain shared"
        );
    }
    assert_eq!(
        vm.process_runtime.worker_sets.get(&2).unwrap().next_index,
        1
    );
    assert!(vm
        .process_runtime
        .worker_sets
        .get(&3)
        .unwrap()
        .members
        .is_empty());
    assert_eq!(
        saved.process_runtime.worker_sets.get(&0).unwrap().members,
        vec![1, removed, 2, removed]
    );
    assert_eq!(
        saved
            .process_runtime
            .worker_sets
            .get(&0)
            .unwrap()
            .next_index,
        3
    );
    assert_eq!(
        saved
            .process_runtime
            .worker_sets
            .get(&1)
            .unwrap()
            .next_index,
        8
    );
    // A zero target needs no refill. Correcting an unrelated index must not
    // enqueue a refill for that set either.
    assert!(vm.process_runtime.detached_tasks.is_empty());
    assert!(vm.process_runtime.refilling_worker_sets.is_empty());
}

#[test]
fn deadline_collection_preserves_no_due_storage_and_due_entry_order() {
    let mut vm = metadata_vm(1);
    let running_a = vm.process_runtime.allocate_future(None, None, false);
    let running_b = vm.process_runtime.allocate_future(None, None, false);
    let ready = vm.process_runtime.allocate_future(None, None, false);
    let cancelled = vm.process_runtime.allocate_future(None, None, false);
    vm.process_runtime.resolve_future(ready, Value::Unit);
    vm.process_runtime
        .futures
        .get_mut(&cancelled)
        .unwrap()
        .state = FutureState::Cancelled(Value::Unit);
    for (id, correlation) in [
        (running_a, 100),
        (running_b, 101),
        (ready, 102),
        (cancelled, 103),
    ] {
        vm.process_runtime
            .futures
            .get_mut(&id)
            .unwrap()
            .correlation_id = Some(correlation);
        vm.process_runtime.reply_table.insert(correlation, id);
    }
    let missing = u64::MAX;
    let entries = [
        DeadlineEntry {
            deadline_tick: 30,
            future_id: running_a,
        },
        DeadlineEntry {
            deadline_tick: 5,
            future_id: ready,
        },
        DeadlineEntry {
            deadline_tick: 10,
            future_id: running_b,
        },
        DeadlineEntry {
            deadline_tick: 20,
            future_id: cancelled,
        },
        DeadlineEntry {
            deadline_tick: 2,
            future_id: missing,
        },
        DeadlineEntry {
            deadline_tick: 10,
            future_id: running_a,
        },
        DeadlineEntry {
            deadline_tick: 1,
            future_id: cancelled,
        },
        DeadlineEntry {
            deadline_tick: 3,
            future_id: running_a,
        },
        DeadlineEntry {
            deadline_tick: 40,
            future_id: ready,
        },
        DeadlineEntry {
            deadline_tick: 50,
            future_id: missing,
        },
    ];
    vm.process_runtime.deadline_queue = entries.iter().copied().collect();
    let saved = checkpoint(&vm);
    assert!(vm.process_runtime.collect_expired_futures(0).is_empty());
    assert!(
        ptr::eq(
            vm.process_runtime.deadline_queue.as_slices().0.as_ptr(),
            saved.process_runtime.deadline_queue.as_slices().0.as_ptr()
        ),
        "no due entry must leave the shared queue untouched"
    );

    let expired = vm.process_runtime.collect_expired_futures(10);
    assert_eq!(expired, vec![running_b, running_a, running_a]);
    assert_eq!(
        vm.process_runtime
            .deadline_queue
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![entries[0], entries[3], entries[8], entries[9]]
    );
    assert_eq!(
        saved
            .process_runtime
            .deadline_queue
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        entries
    );
    for (id, correlation) in [(running_a, 100), (running_b, 101)] {
        assert_eq!(
            vm.process_runtime.futures.get(&id).unwrap().correlation_id,
            None
        );
        assert!(vm.process_runtime.reply_table.get(&correlation).is_none());
        assert_eq!(
            saved
                .process_runtime
                .futures
                .get(&id)
                .unwrap()
                .correlation_id,
            Some(correlation)
        );
        assert_eq!(
            saved.process_runtime.reply_table.get(&correlation),
            Some(&id)
        );
        assert_eq!(
            vm.process_runtime.futures.get(&id).unwrap().state,
            FutureState::Running
        );
    }
    for (id, correlation) in [(ready, 102), (cancelled, 103)] {
        assert_eq!(
            vm.process_runtime.futures.get(&id).unwrap().correlation_id,
            Some(correlation)
        );
        assert_eq!(vm.process_runtime.reply_table.get(&correlation), Some(&id));
    }
}

fn assert_defaults_match_declared_spec(vm: &VM, id: usize) {
    let declared = &vm.bytecode.runtime_process_specs.entries[id];
    assert_eq!(
        &vm.process_runtime.spec_registry.handler_defaults[declared.type_name.as_str()]["output"],
        &declared.dependencies.handlers[0].default_target,
    );
    assert_eq!(
        &vm.process_runtime.spec_registry.supervisor_defaults[declared.type_name.as_str()],
        declared.supervision.policy.as_ref().unwrap(),
    );
}

#[test]
fn unchanged_chunks_share_registry_and_restore_configured_overlays() {
    let mut vm = metadata_vm(2);
    vm.bytecode
        .runtime_boot_plan
        .handler_overrides
        .push(sindr::ir::RuntimeHandlerOverride {
            target_process: "Process000000".into(),
            slot: "output".into(),
            handler_target: RuntimeHandlerTarget {
                name: "ConfiguredOutput".into(),
                named_args: Vec::new(),
            },
        });
    let mut configured_policy = policy();
    configured_policy.max_restarts = 9;
    vm.bytecode.runtime_boot_plan.supervisor_overrides.push(
        sindr::ir::RuntimeSupervisorOverrideEntry {
            process_name: "Process000000".into(),
            policy: configured_policy,
        },
    );
    vm.ensure_root_supervisor_booted().unwrap();
    assert_defaults_match_declared_spec(&vm, 0);
    vm.process_runtime
        .handler_contexts
        .get_mut("Process000000")
        .unwrap()
        .get_mut("output")
        .unwrap()
        .name = "TemporaryOutput".into();
    vm.process_runtime
        .root_supervisor
        .effective_supervisors
        .get_mut("Process000000")
        .unwrap()
        .max_restarts = 99;
    assert_defaults_match_declared_spec(&vm, 0);
    let registry = Rc::clone(&vm.process_runtime.spec_registry);
    assert_eq!(vm.push_atomic(chunk(&vm, false)).unwrap(), Value::Unit);
    assert!(Rc::ptr_eq(&registry, &vm.process_runtime.spec_registry));
    assert_defaults_match_declared_spec(&vm, 0);
    assert_eq!(
        vm.process_runtime.handler_contexts["Process000000"]["output"].name,
        "ConfiguredOutput"
    );
    assert_eq!(
        vm.process_runtime.root_supervisor.effective_supervisors["Process000000"].max_restarts,
        9
    );
    assert_eq!(
        vm.bytecode.runtime_process_specs.entries[0]
            .dependencies
            .handlers[0]
            .default_target
            .name,
        "Process000000Output"
    );
    assert_eq!(
        vm.bytecode.runtime_process_specs.entries[0]
            .supervision
            .policy
            .as_ref()
            .unwrap()
            .max_restarts,
        5
    );

    let mut added = chunk(&vm, false);
    added.runtime_process_specs.push(spec(2, "Added"));
    assert_eq!(vm.push_atomic(added).unwrap(), Value::Unit);
    assert!(!Rc::ptr_eq(&registry, &vm.process_runtime.spec_registry));
    assert_defaults_match_declared_spec(&vm, 0);
    assert_defaults_match_declared_spec(&vm, 2);
    assert_eq!(
        vm.process_runtime.handler_contexts["Added"]["output"].name,
        "AddedOutput"
    );
    assert_eq!(
        vm.process_runtime.root_supervisor.effective_supervisors["Added"].max_restarts,
        5
    );
    assert_eq!(
        vm.process_runtime.handler_contexts["Process000000"]["output"].name,
        "ConfiguredOutput"
    );
    assert_eq!(
        vm.process_runtime.root_supervisor.effective_supervisors["Process000000"].max_restarts,
        9
    );
}

#[test]
fn checkpoint_collections_profile() {
    let (n, repeats) = if std::env::var("SURTR_COLLECTION_PROFILE").as_deref() == Ok("1") {
        (
            std::env::var("SURTR_COLLECTION_N")
                .unwrap()
                .parse::<usize>()
                .unwrap(),
            std::env::var("SURTR_COLLECTION_REPEATS")
                .unwrap()
                .parse::<usize>()
                .unwrap(),
        )
    } else {
        (4, 2)
    };
    assert!(n > 0 && n < u32::MAX as usize);
    assert!(repeats > 0);
    let rich = rich_vm(n);
    let metadata = metadata_vm(n);
    let initial_bytecode = metadata_bytecode(n);
    let checkpoint_chunk = chunk(&rich, false);
    let mut measurements: BTreeMap<&str, Vec<u64>> = [
        "init",
        "save",
        "drop",
        "first_write",
        "second_write",
        "rollback",
        "chunk_shared",
        "chunk_unique",
    ]
    .into_iter()
    .map(|phase| (phase, Vec::with_capacity(repeats)))
    .collect();
    for _ in 0..repeats {
        let bytecode = initial_bytecode.clone();
        let started = Instant::now();
        let initialized = black_box(VM::new(black_box(bytecode)));
        measurements
            .get_mut("init")
            .unwrap()
            .push(started.elapsed().as_nanos() as u64);
        drop(initialized);
        let started = Instant::now();
        let saved = black_box(rich.checkpoint_for_chunk(black_box(&checkpoint_chunk)));
        measurements
            .get_mut("save")
            .unwrap()
            .push(started.elapsed().as_nanos() as u64);
        let started = Instant::now();
        drop(black_box(saved));
        measurements
            .get_mut("drop")
            .unwrap()
            .push(started.elapsed().as_nanos() as u64);

        let mut live = rich.clone();
        let saved = checkpoint(&live);
        let started = Instant::now();
        mutate(black_box(&mut live.process_runtime), 1);
        measurements
            .get_mut("first_write")
            .unwrap()
            .push(started.elapsed().as_nanos() as u64);
        let started = Instant::now();
        mutate(black_box(&mut live.process_runtime), 2);
        measurements
            .get_mut("second_write")
            .unwrap()
            .push(started.elapsed().as_nanos() as u64);
        let started = Instant::now();
        live.rollback_to_checkpoint(black_box(saved)).unwrap();
        measurements
            .get_mut("rollback")
            .unwrap()
            .push(started.elapsed().as_nanos() as u64);
        assert_generation(&live.process_runtime, n, 0);
        drop(live);

        let mut live = metadata.clone();
        let hold_shared = checkpoint(&live);
        let input = chunk(&live, false);
        let started = Instant::now();
        let result = black_box(live.push_atomic(black_box(input)));
        measurements
            .get_mut("chunk_shared")
            .unwrap()
            .push(started.elapsed().as_nanos() as u64);
        assert_eq!(result.unwrap(), Value::Unit);
        drop(hold_shared);
        drop(live);

        // Build an independent VM before timing, so only push_atomic's own
        // transient checkpoint shares its containers in this interval.
        let mut live = metadata_vm(n);
        let input = chunk(&live, false);
        let started = Instant::now();
        let result = black_box(live.push_atomic(black_box(input)));
        measurements
            .get_mut("chunk_unique")
            .unwrap()
            .push(started.elapsed().as_nanos() as u64);
        assert_eq!(result.unwrap(), Value::Unit);
    }
    for (phase, samples) in measurements {
        eprintln!(
            "{}",
            serde_json::json!({
                "profile": "checkpoint_collections", "phase": phase, "n": n,
                "repeats": repeats, "ns": samples.iter().sum::<u64>(), "samples_ns": samples,
                "state": if phase.starts_with("chunk") || phase == "init" { "metadata_only" } else { "synthetic_rich_collections" },
                "process_count": 0, "builder_count": 0, "queue_items_per_rich_queue": n,
                "worker_members_per_entry": 3, "text_bytes_per_rich_output_or_failure": 256,
                "allocation_bytes": "not_measured", "rss": "not_measured",
            })
        );
    }
}
