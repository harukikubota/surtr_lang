//! Manual whole-input ownership cost; result validation runs separately.
mod metrics;
use eldr::VM;
use metrics::{emit, measured};
use sindr::builtin::builtin_id_by_name;
use sindr::ir::{Bytecode, Constant, Opcode};
use sindr::runtime::Value;
use std::hint::black_box;
fn call(name: &str, arity: u8) -> Opcode {
    Opcode::CallBuiltin {
        builtin_id: builtin_id_by_name(name).unwrap(),
        arity,
        span_start: 0,
        span_end: 0,
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args[1].parse().unwrap();
    let case = &args[2];
    let (input, pattern, expected) = match case.as_str() {
        "dense" => ("a".repeat(n), "a", n),
        "single" => ("a".repeat(n), "a+", usize::from(n > 0)),
        "none" => ("a".repeat(n), "z", 0),
        "utf8_empty" => ("éあ".repeat(n), "", n * 2 + 1),
        _ => panic!("unknown case"),
    };
    let bytes = input.len();
    let mut vm = VM::new(Bytecode {
        opcodes: vec![
            Opcode::LoadConst(0),
            call("compile", 1),
            Opcode::GetField { field_index: 0 },
            Opcode::LoadConst(1),
            call("find_all", 2),
            Opcode::Halt,
        ],
        constants: vec![Constant::Str(pattern.into()), Constant::Str(input)],
        ..Default::default()
    });
    let (_, run) = measured(|| vm.run().unwrap());
    let Value::List(list) = vm.last_value().unwrap() else {
        panic!("expected List")
    };
    assert_eq!(list.len(), expected);
    let (handles, collect) = measured(|| list.iter().collect::<Vec<_>>());
    let (clones, clone_cost) = measured(|| black_box(handles.clone()));
    let (_, drop_vm) = measured(|| drop(vm));
    let verify = std::env::var_os("PROBE_VERIFY").is_some();
    let mut unique_inputs = std::collections::HashSet::new();
    if verify {
        let expected_input = if case == "utf8_empty" {
            "éあ".repeat(n)
        } else {
            "a".repeat(n)
        };
        for (idx, value) in clones.iter().enumerate() {
            let Value::RegexMatch(m) = value else {
                panic!("expected match")
            };
            assert_eq!(m.input.len(), bytes);
            assert_eq!(m.input.as_str(), expected_input);
            unique_inputs.insert(m.input.as_ptr());
            let (start, end) = match case.as_str() {
                "dense" => (idx, idx + 1),
                "single" => (0, bytes),
                "utf8_empty" => {
                    let position = if idx % 2 == 0 {
                        idx / 2 * 5
                    } else {
                        idx / 2 * 5 + 2
                    };
                    (position, position)
                }
                _ => unreachable!(),
            };
            assert_eq!((m.start, m.end), (start, end));
            assert_eq!(
                m.input.get(start..end).unwrap(),
                &expected_input[start..end]
            );
        }
    }
    let (_, drop_handles) = measured(|| drop(handles));
    let (_, drop_clones) = measured(|| drop(clones));
    for (phase, measurement) in [
        ("run", run),
        ("collect_handles", collect),
        ("clone_handles", clone_cost),
        ("drop_vm", drop_vm),
        ("drop_handles", drop_handles),
        ("drop_clones", drop_clones),
    ] {
        emit(
            case,
            phase,
            n,
            "string",
            measurement,
            serde_json::json!({"input_bytes":bytes,"matches":expected,"logical_referenced_input_bytes":bytes*expected,"verified_unique_input_buffers":if verify {Some(unique_inputs.len())} else {None}}),
        );
    }
}
