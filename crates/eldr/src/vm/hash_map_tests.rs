use super::tests::base_bytecode;
use super::*;
use sindr::builtin::builtin_id_by_name;
use sindr::runtime::HashMapHandle;

fn keys_context(vm: &mut VM) -> ExecutionContext {
    let map = HashMapHandle::from_entries(
        (0..128)
            .rev()
            .map(|n| (format!("k{n:03}"), Value::Int(int(n))))
            .collect(),
    );
    let outcome = crate::builtin::call_builtin(
        vm,
        builtin_id_by_name("map_keys").unwrap(),
        vec![Value::HashMap(map)],
    )
    .unwrap();
    VM::standalone_builtin_context(outcome)
}

fn finish(vm: &mut VM, mut context: ExecutionContext, quantum: u64) -> Value {
    loop {
        match vm.run_quantum(&mut context, &mut Budget::new(quantum)) {
            ProcessRunOutcome::QuantumExpired => {}
            ProcessRunOutcome::Halted(value) => return value,
            other => panic!("unexpected map result: {other:?}"),
        }
    }
}

#[test]
fn map_preparation_zero_one_and_grouped_budgets_restore_the_same_result() {
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let mut context = keys_context(&mut vm);
    let saved = context.clone();
    assert!(matches!(
        vm.run_quantum(&mut context, &mut Budget::new(0)),
        ProcessRunOutcome::QuantumExpired
    ));
    let mut one = Budget::new(1);
    assert!(matches!(
        vm.run_quantum(&mut context, &mut one),
        ProcessRunOutcome::QuantumExpired
    ));
    assert!(one.expired());
    let expected = Value::List(ListHandle::from_items(
        (0..128).map(|n| Value::Str(format!("k{n:03}"))).collect(),
    ));
    assert_eq!(finish(&mut vm, context, 1), expected);
    assert_eq!(finish(&mut vm, saved, 256), expected);
}

#[test]
fn map_preparation_cancellation_rejects_the_partial_result() {
    let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
    let mut context = keys_context(&mut vm);
    assert!(matches!(
        vm.run_quantum(&mut context, &mut Budget::new(8)),
        ProcessRunOutcome::QuantumExpired
    ));
    VM::cancel_execution_context(&mut context, RuntimeError::new("cancelled map preparation"));
    let ProcessRunOutcome::Failed(error) = vm.run_quantum(&mut context, &mut Budget::new(256))
    else {
        panic!("cancelled sort must fail")
    };
    assert_eq!(error.message, "cancelled map preparation");
}
