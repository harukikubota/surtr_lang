use super::tests::{base_bytecode, function_entry};
use super::*;
use crate::builtin::{list_builder_metrics, reset_list_builder_metrics};
use sindr::builtin::builtin_id_by_name;
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
