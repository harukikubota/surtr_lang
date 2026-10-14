use super::tests::{base_bytecode, function_entry, test_runtime_process_spec};
use super::*;
use std::hint::black_box;
use std::ptr;
use std::time::Instant;

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

fn metadata_vm(n: usize) -> VM {
    let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::Return]);
    bytecode.functions = vec![function_entry(0, 1, 1, 0, Some("Metadata::init"))];
    bytecode.runtime_process_specs.entries = (0..n)
        .map(|id| spec(id as u32, &format!("Process{id:06}")))
        .collect();
    VM::new(bytecode)
}

fn chunk(vm: &VM, fail: bool) -> BytecodeChunk {
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

fn assert_shared_specs(live: &ProcessRuntime, saved: &ProcessRuntime) {
    assert!(!live.spec_registry.specs_by_id.is_empty());
    assert!(ptr::eq(
        live.spec_registry.specs_by_id.as_ptr(),
        saved.spec_registry.specs_by_id.as_ptr()
    ));
    let (live_name, live_id) = live
        .spec_registry
        .spec_id_by_name
        .first_key_value()
        .unwrap();
    let (saved_name, saved_id) = saved
        .spec_registry
        .spec_id_by_name
        .first_key_value()
        .unwrap();
    assert!(
        ptr::eq(live_name, saved_name),
        "name index storage must be shared"
    );
    assert!(ptr::eq(live_id, saved_id));
    let (live_name, live_spec) = live.spec_registry.specs_by_name.first_key_value().unwrap();
    let (saved_name, saved_spec) = saved.spec_registry.specs_by_name.first_key_value().unwrap();
    assert!(
        ptr::eq(live_name, saved_name),
        "named spec storage must be shared"
    );
    assert!(ptr::eq(live_spec, saved_spec));
}

#[test]
fn unchanged_checkpoints_share_process_spec_storage() {
    let vm = metadata_vm(32);
    let cloned = vm.clone();
    assert_shared_specs(&vm.process_runtime, &cloned.process_runtime);
    let first = checkpoint(&vm);
    let second = checkpoint(&vm);
    assert_shared_specs(&vm.process_runtime, &first.process_runtime);
    assert_shared_specs(&vm.process_runtime, &second.process_runtime);
    assert_shared_specs(&first.process_runtime, &second.process_runtime);
    drop(cloned);
    drop(first);
    assert_eq!(
        vm.process_runtime.spec_for_id(31).unwrap().type_name,
        "Process000031"
    );
    drop(second);
    assert_eq!(
        vm.process_runtime
            .spec_by_process_name("Process000000")
            .unwrap()
            .process_id,
        0
    );
}

#[test]
fn replacement_preserves_saved_lookup_and_handler_targets_until_rollback() {
    let mut vm = metadata_vm(2);
    let saved = checkpoint(&vm);
    let mut replacement = vm.bytecode.runtime_process_specs.clone();
    replacement.entries[0].state.state_type.name = "String".into();
    replacement.entries[0].dependencies.handlers[0]
        .default_target
        .name = "ChangedOutput".into();
    replacement.entries.push(spec(2, "Added"));
    vm.process_runtime.register_spec_table(&replacement);

    assert_eq!(
        vm.process_runtime
            .spec_for_id(0)
            .unwrap()
            .state
            .state_type
            .name,
        "String"
    );
    assert_eq!(
        saved
            .process_runtime
            .spec_for_id(0)
            .unwrap()
            .state
            .state_type
            .name,
        "Int"
    );
    assert!(saved
        .process_runtime
        .spec_by_process_name("Added")
        .is_none());
    assert_eq!(
        saved
            .process_runtime
            .spec_registry
            .spec_id_by_name
            .get("Process000000"),
        Some(&0)
    );
    assert_eq!(
        saved
            .process_runtime
            .handler_targets_for_process("Process000000")
            .unwrap()["output"]
            .name,
        "Process000000Output"
    );
    assert_eq!(
        vm.process_runtime
            .handler_targets_for_process("Process000000")
            .unwrap()["output"]
            .name,
        "ChangedOutput"
    );

    vm.rollback_to_checkpoint(saved).unwrap();
    assert_eq!(
        vm.process_runtime
            .spec_for_id(0)
            .unwrap()
            .state
            .state_type
            .name,
        "Int"
    );
    assert!(vm.process_runtime.spec_by_process_name("Added").is_none());
    assert_eq!(
        vm.process_runtime
            .handler_targets_for_process("Process000000")
            .unwrap()["output"]
            .name,
        "Process000000Output"
    );
}

#[test]
fn atomic_chunks_commit_successful_specs_and_discard_failed_specs() {
    let mut vm = metadata_vm(1);
    let mut success = chunk(&vm, false);
    success.runtime_process_specs.push(spec(1, "Committed"));
    assert_eq!(vm.push_atomic(success).unwrap(), Value::Unit);
    assert_eq!(
        vm.process_runtime.spec_for_id(1).unwrap().type_name,
        "Committed"
    );
    assert_eq!(vm.bytecode.runtime_process_specs.entries.len(), 2);

    let mut failure = chunk(&vm, true);
    failure.runtime_process_specs.push(spec(2, "Discarded"));
    vm.verify_chunk(&failure).unwrap();
    let opcode_len = vm.bytecode.opcodes.len();
    let err = vm
        .push_atomic(failure)
        .expect_err("runtime contract violation must fail after registration");
    assert!(
        err.message.contains("checkpoint test runtime failure"),
        "{}",
        err.message
    );
    assert!(vm
        .process_runtime
        .spec_by_process_name("Discarded")
        .is_none());
    assert!(vm
        .process_runtime
        .handler_targets_for_process("Discarded")
        .is_none());
    assert_eq!(
        vm.process_runtime.spec_for_id(1).unwrap().type_name,
        "Committed"
    );
    assert_eq!(
        vm.process_runtime
            .spec_registry
            .spec_id_by_name
            .get("Committed"),
        Some(&1)
    );
    assert_eq!(vm.bytecode.runtime_process_specs.entries.len(), 2);
    assert_eq!(vm.bytecode.opcodes.len(), opcode_len);
}

// Standalone mechanism timings: setup, assertions, inspection and JSON output
// stay outside each interval. Runtime metadata has no process/Builder entries.
// SURTR_METADATA_PROFILE=1 enables SURTR_METADATA_N / SURTR_METADATA_REPEATS.
#[test]
fn process_metadata_profile() {
    let (n, repeats) = if std::env::var("SURTR_METADATA_PROFILE").as_deref() == Ok("1") {
        (
            std::env::var("SURTR_METADATA_N")
                .expect("SURTR_METADATA_N")
                .parse::<usize>()
                .unwrap(),
            std::env::var("SURTR_METADATA_REPEATS")
                .expect("SURTR_METADATA_REPEATS")
                .parse::<usize>()
                .unwrap(),
        )
    } else {
        (4, 2)
    };
    assert!(repeats > 0);
    assert!(n < u32::MAX as usize);
    let vm = metadata_vm(n);
    let checkpoint_chunk = chunk(&vm, false);
    let mut replacement = vm.bytecode.runtime_process_specs.clone();
    replacement.entries.push(spec(n as u32, "ProfileAdded"));
    let mut save_ns = Vec::with_capacity(repeats);
    let mut drop_ns = Vec::with_capacity(repeats);
    let mut register_ns = Vec::with_capacity(repeats);
    let mut rollback_ns = Vec::with_capacity(repeats);
    let mut push_success_ns = Vec::with_capacity(repeats);
    let mut unique_chunk_ns = Vec::with_capacity(repeats);

    for _ in 0..repeats {
        let started = Instant::now();
        let saved = black_box(vm.checkpoint_for_chunk(black_box(&checkpoint_chunk)));
        save_ns.push(started.elapsed().as_nanos() as u64);
        let started = Instant::now();
        drop(black_box(saved));
        drop_ns.push(started.elapsed().as_nanos() as u64);

        let mut live = vm.clone();
        let saved = checkpoint(&live);
        let started = Instant::now();
        live.process_runtime
            .register_spec_table(black_box(&replacement));
        register_ns.push(started.elapsed().as_nanos() as u64);
        let started = Instant::now();
        live.rollback_to_checkpoint(black_box(saved)).unwrap();
        rollback_ns.push(started.elapsed().as_nanos() as u64);
        assert_eq!(live.process_runtime.spec_registry.specs_by_id.len(), n);

        let mut live = vm.clone();
        let success = chunk(&live, false);
        let started = Instant::now();
        let result = black_box(live.push_atomic(black_box(success)));
        push_success_ns.push(started.elapsed().as_nanos() as u64);
        assert_eq!(result.unwrap(), Value::Unit);
        assert_eq!(live.process_runtime.spec_registry.specs_by_id.len(), n);

        // No VM clone owns this registry: committing the chunk includes the
        // previous registry's final destruction when its checkpoint is dropped.
        let mut live = metadata_vm(n);
        let success = chunk(&live, false);
        let started = Instant::now();
        let result = black_box(live.push_atomic(black_box(success)));
        unique_chunk_ns.push(started.elapsed().as_nanos() as u64);
        assert_eq!(result.unwrap(), Value::Unit);
        assert_eq!(live.process_runtime.spec_registry.specs_by_id.len(), n);
    }
    for (phase, samples) in [
        ("save", save_ns),
        ("drop", drop_ns),
        ("register", register_ns),
        ("rollback", rollback_ns),
        ("chunk_shared", push_success_ns),
        ("chunk_unique", unique_chunk_ns),
    ] {
        eprintln!(
            "{}",
            serde_json::json!({
                "profile": "checkpoint_metadata",
                "phase": phase,
                "n": n,
                "repeats": repeats,
                "ns": samples.iter().sum::<u64>(),
                "samples_ns": samples,
                "handlers_per_spec": 2,
                "dependency_handlers_per_spec": 1,
                "argument_value_bytes_per_spec": 256,
                "runtime_process_count": 0,
                "builder_count": 0,
                "allocation_bytes": "not_measured",
                "rss": "not_measured",
            })
        );
    }
}
