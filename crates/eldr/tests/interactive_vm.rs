use eldr::interactive::{InteractiveChunkPolicy, InteractiveVm};
use eldr::value::Value;
use sindr::ir::{BytecodeChunk, Constant, Opcode};
use sindr::primitives::int;
use sindr::runtime::TypeRegistry;

fn const_chunk(value: i64) -> BytecodeChunk {
    BytecodeChunk {
        opcodes: vec![Opcode::LoadConst(0), Opcode::Halt],
        source_map: None,
        const_base: 0,
        constants: vec![Constant::Int(int(value))],
        new_locals: 0,
        type_entries: Vec::new(),
        error_template_base: 0,
        error_templates: Vec::new(),
        dbg_template_base: 0,
        dbg_templates: Vec::new(),
        functions: Vec::new(),
        docs: Vec::new(),
        runtime_process_specs: Vec::new(),
        runtime_boot_plan: Default::default(),
    }
}

fn invalid_const_base_chunk() -> BytecodeChunk {
    BytecodeChunk {
        opcodes: vec![Opcode::LoadConst(0), Opcode::Halt],
        source_map: None,
        const_base: 99,
        constants: vec![Constant::Int(int(7))],
        new_locals: 0,
        type_entries: Vec::new(),
        error_template_base: 0,
        error_templates: Vec::new(),
        dbg_template_base: 0,
        dbg_templates: Vec::new(),
        functions: Vec::new(),
        docs: Vec::new(),
        runtime_process_specs: Vec::new(),
        runtime_boot_plan: Default::default(),
    }
}

#[test]
fn interactive_push_records_last_result_and_clears_operand_stack() {
    let mut vm = InteractiveVm::new(TypeRegistry::new());

    let execution = vm
        .push_chunk(const_chunk(42), InteractiveChunkPolicy::ReplAppendOnly)
        .expect("chunk should run");

    assert_eq!(execution.value, Value::Int(int(42)));
    assert_eq!(vm.last_result(), Some(&Value::Int(int(42))));
    assert_eq!(vm.stack_depth(), 0);
}

#[test]
fn interactive_push_rolls_back_last_result_and_bytecode_on_failure() {
    let mut vm = InteractiveVm::new(TypeRegistry::new());
    vm.push_chunk(const_chunk(42), InteractiveChunkPolicy::ReplAppendOnly)
        .expect("first chunk should run");

    let before = vm.snapshot_bytecode();
    let err = vm
        .push_chunk(
            invalid_const_base_chunk(),
            InteractiveChunkPolicy::ReplAppendOnly,
        )
        .expect_err("bad chunk base should fail");

    assert!(
        err.message.contains("constant base mismatch"),
        "{}",
        err.message
    );
    assert_eq!(vm.last_result(), Some(&Value::Int(int(42))));
    assert_eq!(vm.stack_depth(), 0);
    assert_eq!(vm.snapshot_bytecode(), before);
}

#[test]
fn preload_policy_allows_non_append_only_metadata() {
    use sindr::ir::RuntimeBootPlan;
    use sindr::runtime::{TypeEntry, TypeKind};

    let mut vm = InteractiveVm::new(TypeRegistry::new());
    let mut chunk = const_chunk(7);
    chunk.type_entries.push(TypeEntry {
        tag: 99,
        name: "Scratch".into(),
        kind: TypeKind::Struct,
        field_names: vec![],
        private_flags: vec![],
    });
    chunk.runtime_boot_plan = RuntimeBootPlan::default();

    let execution = vm
        .push_chunk(chunk, InteractiveChunkPolicy::Preload)
        .expect("preload policy should permit metadata growth");

    assert_eq!(execution.value, Value::Int(int(7)));
}
