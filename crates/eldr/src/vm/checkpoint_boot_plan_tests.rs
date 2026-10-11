use super::checkpoint_collections_tests::{chunk, metadata_vm};
use super::*;
use sindr::ir::{RuntimeBootPlan, RuntimeHandlerOverride, StandardOverrideEntry};

fn handler_override() -> RuntimeHandlerOverride {
    RuntimeHandlerOverride {
        target_process: "Process000000".into(),
        slot: "output".into(),
        handler_target: RuntimeHandlerTarget {
            name: "FailedOutput".into(),
            named_args: Vec::new(),
        },
    }
}

#[test]
fn failed_preload_does_not_keep_handler_override_for_next_chunk() {
    use crate::interactive::{InteractiveChunkPolicy, InteractiveVm};
    let vm = metadata_vm(1);
    let mut interactive = InteractiveVm::from_bytecode(vm.snapshot_bytecode());
    let original_plan = interactive.bytecode().runtime_boot_plan.clone();
    let mut failure = chunk(interactive.as_vm(), true);
    failure
        .runtime_boot_plan
        .handler_overrides
        .push(handler_override());
    interactive.as_vm().verify_chunk(&failure).unwrap();
    let error = interactive
        .push_chunk(failure, InteractiveChunkPolicy::Preload)
        .expect_err("runtime contract violation must fail after registration");
    assert!(
        error.message.contains("checkpoint test runtime failure"),
        "{}",
        error.message
    );
    assert_eq!(interactive.bytecode().runtime_boot_plan, original_plan);
    let success = chunk(interactive.as_vm(), false);
    assert_eq!(
        interactive
            .push_chunk(success, InteractiveChunkPolicy::Preload)
            .unwrap()
            .value,
        Value::Unit
    );
    assert_eq!(
        interactive.as_vm().process_runtime.handler_contexts["Process000000"]["output"].name,
        "Process000000Output"
    );
}

#[test]
fn rollback_truncates_appended_boot_plan_entries_and_preserves_prefix() {
    let mut vm = metadata_vm(1);
    vm.bytecode
        .runtime_boot_plan
        .handler_overrides
        .push(handler_override());
    let original_plan = vm.bytecode.runtime_boot_plan.clone();
    let saved = vm.checkpoint_for_chunk(&chunk(&vm, false));
    vm.bytecode
        .runtime_boot_plan
        .singletons
        .extend(RuntimeBootPlan::explicit_singleton("Appended").singletons);
    vm.bytecode
        .runtime_boot_plan
        .standard_overrides
        .push(StandardOverrideEntry {
            standard_name: "StdOut".into(),
            handler_target: handler_override().handler_target,
        });
    vm.bytecode
        .runtime_boot_plan
        .handler_overrides
        .push(handler_override());
    vm.rollback_to_checkpoint(saved);
    assert_eq!(vm.bytecode.runtime_boot_plan, original_plan);
}
