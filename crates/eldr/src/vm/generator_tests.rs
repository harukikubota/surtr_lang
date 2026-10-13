use super::tests::{base_bytecode, function_entry};
use super::*;
use crate::builtin::{list_builder_metrics, reset_list_builder_metrics};
use sindr::builtin::{builtin_id_by_name, OPTION_NONE_VARIANT, OPTION_SOME_VARIANT};
use sindr::runtime::{
    GeneratorHandle, GeneratorProducer, InfiniteGeneratorHandle, InfiniteGeneratorProducer,
    TypeEntry, TypeKind,
};
use std::sync::Arc;

fn generator_vm(opcodes: Vec<Opcode>) -> VM {
    let mut bytecode = base_bytecode(opcodes);
    for (tag, name, fields) in [
        (20, "Option::None", vec![]),
        (21, "Option::Some", vec!["value".into()]),
    ] {
        bytecode.type_registry.register(TypeEntry {
            tag,
            name: name.into(),
            kind: TypeKind::EnumVariant,
            private_flags: vec![false; fields.len()],
            field_names: fields,
        });
    }
    VM::new(bytecode)
}

fn finite_generator(state: Value, step: Callable) -> Value {
    Value::Generator(GeneratorHandle(Arc::new(GeneratorProducer::Unfold {
        state,
        step,
    })))
}

fn invalid_step_error(result: Value, infinite: bool) -> RuntimeError {
    let mut vm = generator_vm(vec![Opcode::LoadLocal(0), Opcode::Return]);
    vm.bytecode.functions = vec![function_entry(0, 0, 2, 2, Some("Main::invalid_step"))];
    let mut step = vm.callable_for_function(0);
    step.lexical_captures.push(result);
    let gen = if infinite {
        Value::InfiniteGenerator(InfiniteGeneratorHandle(Arc::new(
            InfiniteGeneratorProducer::Unfold {
                state: Value::Unit,
                step,
            },
        )))
    } else {
        finite_generator(Value::Unit, step)
    };
    let id = builtin_id_by_name(if infinite { "inf_gen_next" } else { "gen_step" }).unwrap();
    let outcome =
        call_builtin(&mut vm, id, vec![gen]).expect("producer should dispatch the callback");
    vm.drive_builtin_outcome(outcome)
        .expect_err("malformed producer output must fail")
}

#[test]
fn finite_generator_step_rejects_malformed_option_carriers() {
    let payload = Value::Tuple(vec![Value::Unit, Value::Unit]);
    for value in [
        Value::Tagged {
            tag: 99,
            fields: vec![Value::Int(int(0)), payload.clone()],
        },
        Value::Tagged {
            tag: 21,
            fields: vec![Value::Int(int(1)), payload.clone()],
        },
        Value::Tagged {
            tag: 21,
            fields: vec![Value::Int(int(0))],
        },
        Value::Tagged {
            tag: 21,
            fields: vec![Value::Int(int(0)), payload, Value::Unit],
        },
        Value::Tagged {
            tag: 20,
            fields: vec![Value::Int(int(0))],
        },
        Value::Tagged {
            tag: 20,
            fields: vec![],
        },
        Value::Tagged {
            tag: 20,
            fields: vec![Value::Int(int(1)), Value::Unit],
        },
    ] {
        let error = invalid_step_error(value, false);
        assert!(error.message.contains("invalid Option"), "{error:?}");
    }
}

#[test]
fn generator_producer_rejects_malformed_step_pairs() {
    for infinite in [false, true] {
        for fields in [vec![Value::Unit], vec![Value::Unit; 3]] {
            let pair = Value::Tuple(fields);
            let result = if infinite {
                pair
            } else {
                Value::Tagged {
                    tag: 21,
                    fields: vec![Value::Int(int(0)), pair],
                }
            };
            let error = invalid_step_error(result, infinite);
            assert!(
                error
                    .message
                    .contains("pair must contain exactly two fields"),
                "{error:?}"
            );
        }
    }
}

#[test]
fn generator_materialize_failure_does_not_publish_partial_builder() {
    let mut vm = generator_vm(vec![
        Opcode::LoadLocal(0),
        Opcode::LoadConst(0),
        Opcode::EqInt,
        Opcode::JumpIfFalse(9),
        Opcode::LoadConst(3),
        Opcode::LoadConst(1),
        Opcode::LoadConst(1),
        Opcode::StructNew { field_count: 2 },
        Opcode::Return,
        Opcode::LoadConst(2),
        Opcode::LoadConst(4),
        Opcode::LoadLocal(0),
        Opcode::LoadLocal(0),
        Opcode::LoadConst(1),
        Opcode::AddInt,
        Opcode::TupleNew { len: 2 },
        Opcode::StructNew { field_count: 2 },
        Opcode::Return,
    ]);
    vm.bytecode.constants = vec![
        Constant::Int(int(2)),
        Constant::Int(int(1)),
        Constant::Tag(21),
        Constant::Tag(20),
        Constant::Int(int(0)),
    ];
    vm.bytecode.functions = vec![function_entry(0, 0, 1, 1, Some("Main::corrupt_step"))];
    let gen = finite_generator(Value::Int(int(1)), vm.callable_for_function(0));
    reset_list_builder_metrics();
    let outcome = call_builtin(
        &mut vm,
        builtin_id_by_name("gen_to_list").unwrap(),
        vec![gen],
    )
    .expect("to_list should dispatch the producer");
    let error = vm
        .drive_builtin_outcome(outcome)
        .expect_err("corrupt producer output must discard the unfinished List");
    assert!(error.message.contains("invalid Option"), "{error:?}");
    assert_eq!(list_builder_metrics().pushes, 1);
    assert_eq!(list_builder_metrics().finishes, 0);
}

fn materialize_checkpoint_vm(invalid_predicate: bool) -> (VM, Value, Callable) {
    let print = Opcode::CallBuiltin {
        builtin_id: builtin_id_by_name("print").unwrap(),
        arity: 1,
        span_start: 0,
        span_end: 0,
    };
    // A finite producer emits 0,1,2 and evaluates state 3 once to return None.
    // Printing the state lets the finish/restore test detect producer reevaluation.
    let mut code = vec![
        Opcode::LoadLocal(0),
        print.clone(),
        Opcode::Pop,
        Opcode::LoadLocal(0),
        Opcode::LoadConst(0),
        Opcode::EqInt,
        Opcode::JumpIfFalse(11),
        Opcode::LoadConst(3),
        Opcode::LoadConst(8),
        Opcode::StructNew { field_count: 1 },
        Opcode::Return,
        Opcode::LoadConst(2),
        Opcode::LoadConst(4),
        Opcode::LoadLocal(0),
        Opcode::LoadLocal(0),
        Opcode::LoadConst(1),
        Opcode::AddInt,
        Opcode::TupleNew { len: 2 },
        Opcode::StructNew { field_count: 2 },
        Opcode::Return,
    ];
    let predicate_pc = code.len() as u32;
    code.extend([
        Opcode::LoadConst(5),
        print,
        Opcode::Pop,
        Opcode::LoadLocal(0),
        Opcode::LoadConst(if invalid_predicate { 1 } else { 6 }),
        Opcode::LtInt,
    ]);
    if invalid_predicate {
        code.extend([
            Opcode::JumpIfFalse(predicate_pc + 9),
            Opcode::LoadConst(7),
            Opcode::Return,
            Opcode::LoadConst(4),
            Opcode::Return,
        ]);
    } else {
        code.push(Opcode::Return);
    }
    let mut vm = generator_vm(code).with_output_capture();
    vm.bytecode.constants = vec![
        Constant::Int(int(3)),
        Constant::Int(int(1)),
        Constant::Tag(21),
        Constant::Tag(20),
        Constant::Int(int(OPTION_SOME_VARIANT.discriminant)),
        Constant::Str("predicate".into()),
        Constant::Int(int(2)),
        Constant::Bool(true),
        Constant::Int(int(OPTION_NONE_VARIANT.discriminant)),
    ];
    vm.bytecode.functions = vec![
        function_entry(0, 0, 1, 1, Some("Main::step")),
        function_entry(1, predicate_pc, 1, 1, Some("Main::predicate")),
    ];
    let source = finite_generator(Value::Int(int(0)), vm.callable_for_function(0));
    let predicate = vm.callable_for_function(1);
    (vm, source, predicate)
}

fn materialize_until_first_append(vm: &mut VM, context: &mut ExecutionContext) {
    for _ in 0..100 {
        assert!(matches!(
            vm.run_quantum(context, &mut Budget::new(1)),
            ProcessRunOutcome::QuantumExpired
        ));
        if list_builder_metrics().pushes == 1 {
            return;
        }
    }
    panic!("small producer did not append its first item");
}

fn finish_materialize(vm: &mut VM, context: &mut ExecutionContext) -> Value {
    for _ in 0..100 {
        match vm.run_quantum(context, &mut Budget::new(1)) {
            ProcessRunOutcome::QuantumExpired => {}
            ProcessRunOutcome::Halted(value) => return value,
            other => panic!("unexpected materialize finish outcome: {other:?}"),
        }
    }
    panic!("small materialize finish did not complete");
}

#[test]
fn generator_materialize_terminal_modes_restore_budgeted_finish_without_repeating_callbacks() {
    for mode in ["gen_take", "gen_take_while", "gen_to_list"] {
        let (mut vm, source, predicate) = materialize_checkpoint_vm(false);
        let args = match mode {
            "gen_take" => vec![source, Value::Int(int(3))],
            "gen_take_while" => vec![source, Value::Callable(predicate)],
            "gen_to_list" => vec![source],
            _ => unreachable!(),
        };
        reset_list_builder_metrics();
        let outcome = call_builtin(&mut vm, builtin_id_by_name(mode).unwrap(), args).unwrap();
        let mut context = VM::standalone_builtin_context(outcome);
        materialize_until_first_append(&mut vm, &mut context);
        let saved_prefix = context.clone();
        assert_eq!(list_builder_metrics().copied_items, 0);

        // Stop after the first finish transition transferred the owned suffix.
        // The nonempty prefix is still held by the construction checkpoint.
        for _ in 0..150 {
            let outcome = vm.run_quantum(&mut context, &mut Budget::new(1));
            assert!(
                matches!(outcome, ProcessRunOutcome::QuantumExpired),
                "{mode}: {outcome:?}; metrics={:?}; callbacks={:?}",
                list_builder_metrics(),
                vm.output
            );
            if list_builder_metrics().finish_steps > 0 {
                break;
            }
        }
        assert_eq!(list_builder_metrics().finish_steps, 1, "{mode}");
        assert_eq!(list_builder_metrics().finishes, 0, "{mode}");
        let expected_callbacks = match mode {
            "gen_take" => vec!["0", "1", "2"],
            "gen_take_while" => vec!["0", "predicate", "1", "predicate", "2", "predicate"],
            "gen_to_list" => vec!["0", "1", "2", "3"],
            _ => unreachable!(),
        };
        assert_eq!(vm.output.as_ref().unwrap(), &expected_callbacks, "{mode}");
        let saved_finish = context.clone();
        assert!(matches!(
            vm.run_quantum(&mut context, &mut Budget::new(1)),
            ProcessRunOutcome::QuantumExpired
        ));
        assert_eq!(list_builder_metrics().finish_copied_items, 1, "{mode}");
        assert_eq!(list_builder_metrics().finishes, 0, "{mode}");
        drop(context);
        let mut restored = saved_finish;
        let result = finish_materialize(&mut vm, &mut restored);
        assert_eq!(vm.output.as_ref().unwrap(), &expected_callbacks, "{mode}");
        assert_eq!(list_builder_metrics().finishes, 1, "{mode}");
        assert_eq!(list_builder_metrics().finish_copied_items, 2, "{mode}");
        let expected_len = if mode == "gen_take_while" { 2 } else { 3 };
        let expected = Value::List(ListHandle::from_items(
            (0..expected_len).map(|n| Value::Int(int(n))).collect(),
        ));
        if mode == "gen_to_list" {
            assert_eq!(result, expected);
        } else {
            let Value::Tuple(fields) = result else {
                panic!("materialize with rest must return its pair: {mode}")
            };
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0], expected);
            let Value::Generator(rest) = &fields[1] else {
                panic!("materialize rest must remain a finite generator: {mode}")
            };
            let GeneratorProducer::Unfold { state, .. } = rest.0.as_ref() else {
                panic!("unconsumed producer rest must preserve its state: {mode}")
            };
            assert_eq!(state, &Value::Int(int(expected_len)), "{mode}");
        }
        drop(saved_prefix);
    }
}

#[test]
fn generator_take_while_invalid_predicate_discards_shared_builder_without_finishing() {
    let (mut vm, source, predicate) = materialize_checkpoint_vm(true);
    reset_list_builder_metrics();
    let outcome = call_builtin(
        &mut vm,
        builtin_id_by_name("gen_take_while").unwrap(),
        vec![source, Value::Callable(predicate)],
    )
    .unwrap();
    let mut context = VM::standalone_builtin_context(outcome);
    materialize_until_first_append(&mut vm, &mut context);
    let saved = context.clone();
    let error = (0..100)
        .find_map(
            |_| match vm.run_quantum(&mut context, &mut Budget::new(1)) {
                ProcessRunOutcome::QuantumExpired => None,
                ProcessRunOutcome::Failed(error) => Some(error),
                other => panic!("invalid predicate must reject its partial result: {other:?}"),
            },
        )
        .expect("invalid predicate must fail within its small workload");
    assert!(
        error.message.contains("predicate must return Boolean"),
        "{error:?}"
    );
    assert_eq!(list_builder_metrics().pushes, 1);
    assert_eq!(list_builder_metrics().finishes, 0);
    assert_eq!(list_builder_metrics().finish_steps, 0);
    assert_eq!(list_builder_metrics().finish_copied_items, 0);
    assert_eq!(
        vm.output.as_ref().unwrap(),
        &["0", "predicate", "1", "predicate"]
    );
    drop(saved);
}
