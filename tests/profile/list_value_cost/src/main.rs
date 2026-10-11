//! RT-3 current-VM workloads. No historical VM path or production optimization.
use eldr::{vm::VmObservationOptions, VM};
use list_value_cost_probe::metrics::{emit, measured};
use sindr::builtin::builtin_id_by_name;
use sindr::ir::{Bytecode, Constant, FunctionEntry, Opcode};
use sindr::primitives::int;
use sindr::runtime::Value;

const PAYLOAD_WIDTH: usize = 2;
const PAYLOAD_DEPTH: usize = 2;
const LEAF_BYTES: usize = 256;

fn call_builtin(name: &str, arity: u8) -> Opcode {
    Opcode::CallBuiltin {
        builtin_id: builtin_id_by_name(name).expect("canonical builtin"),
        arity,
        span_start: 0,
        span_end: 0,
    }
}

// Container construction occurs once at the root, inside run. Inputs are
// obtained from this template using the same LoadLocal path for every payload.
fn payload_code(code: &mut Vec<Opcode>, kind: &str, depth: usize) {
    if matches!(kind, "int" | "string") || depth == 0 {
        code.push(Opcode::LoadConst(0));
        return;
    }
    for index in 0..PAYLOAD_WIDTH {
        if kind == "hashmap" {
            code.push(Opcode::LoadConst((index + 1).try_into().unwrap()));
        }
        payload_code(code, kind, depth - 1);
        if kind == "hashmap" {
            code.push(Opcode::TupleNew { len: 2 });
        }
    }
    match kind {
        "tuple" => code.push(Opcode::TupleNew {
            len: PAYLOAD_WIDTH.try_into().unwrap(),
        }),
        "list" | "hashmap" => {
            code.push(Opcode::ListFromItems {
                len: PAYLOAD_WIDTH.try_into().unwrap(),
            });
            if kind == "hashmap" {
                code.push(call_builtin("map_from_entries", 1));
            }
        }
        _ => panic!("payload must be int, string, tuple, list or hashmap"),
    }
}

fn list_code(code: &mut Vec<Opcode>, n: usize) {
    code.extend(std::iter::repeat(Opcode::LoadLocal(0)).take(n));
    code.push(Opcode::ListFromItems {
        len: n.try_into().unwrap(),
    });
}

fn append_flat_map(code: &mut Vec<Opcode>, mapper: u32) {
    code.push(Opcode::LoadFunctionRef(mapper));
    code.push(call_builtin("list_flat_map", 2));
}

fn bytecode(case: &str, n: usize, kind: &str, depth: usize) -> Bytecode {
    assert!(n > 0 && depth > 0);
    assert!(matches!(
        kind,
        "int" | "string" | "tuple" | "list" | "hashmap"
    ));
    assert!(matches!(
        case,
        "singleton" | "empty" | "huge" | "chain" | "nested" | "product"
    ));
    let mut code = Vec::new();
    payload_code(&mut code, kind, PAYLOAD_DEPTH);
    code.push(Opcode::StoreLocal(0));
    list_code(
        &mut code,
        if matches!(case, "huge" | "nested") {
            1
        } else {
            n
        },
    );
    append_flat_map(&mut code, 0);
    if case == "chain" {
        for _ in 1..depth {
            append_flat_map(&mut code, 0);
        }
    }
    code.push(Opcode::Halt);
    let layers = if case == "product" {
        3
    } else if case == "nested" {
        depth
    } else {
        1
    };
    let mut functions = Vec::new();
    for layer in 0..layers {
        let pc = code.len();
        if matches!(case, "product" | "nested") && layer + 1 < layers {
            list_code(&mut code, if case == "nested" { 1 } else { n });
            append_flat_map(&mut code, (layer + 1).try_into().unwrap());
        } else if case == "empty" {
            code.push(Opcode::ListNil);
        } else {
            list_code(
                &mut code,
                if matches!(case, "huge" | "nested") {
                    n
                } else {
                    1
                },
            );
        }
        code.push(Opcode::Return);
        functions.push(FunctionEntry {
            fun_idx: layer.try_into().unwrap(),
            entry_pc: pc.try_into().unwrap(),
            num_locals: 1,
            arity: 1,
            qualified_name: Some(format!("Probe::mapper{layer}")),
            signature: None,
            end_pc: code.len().try_into().unwrap(),
            span_start: 0,
            span_end: 0,
            flags: Default::default(),
        });
    }
    let mut constants = vec![if kind == "int" {
        Constant::Int(int(7))
    } else {
        Constant::Str("x".repeat(LEAF_BYTES))
    }];
    if kind == "hashmap" {
        constants.extend((0..PAYLOAD_WIDTH).map(|i| Constant::Str(format!("key{i}"))));
    }
    Bytecode {
        opcodes: code,
        constants,
        functions,
        num_locals: 1,
        ..Default::default()
    }
}

fn counts(case: &str, n: usize, depth: usize) -> (usize, usize, usize, usize) {
    match case {
        "product" => {
            let n2 = n.checked_mul(n).unwrap();
            let n3 = n2.checked_mul(n).unwrap();
            (
                n.checked_add(n2).unwrap().checked_add(n3).unwrap(),
                n3.checked_mul(3).unwrap(),
                n3,
                3,
            )
        }
        "chain" => (
            n.checked_mul(depth).unwrap(),
            n.checked_mul(depth).unwrap(),
            n,
            depth,
        ),
        "nested" => (depth, n.checked_mul(depth).unwrap(), n, depth),
        "huge" => (1, n, n, 1),
        "empty" => (n, 0, 0, 1),
        "singleton" => (n, n, n, 1),
        _ => panic!("invalid workload"),
    }
}

// Runs only in a separate PROBE_VERIFY process, after the original VM and all
// its root locals/constants have been destroyed. List iteration may clone
// values; it is deliberately absent from performance measurements.
fn verify_payload(value: &Value, kind: &str, depth: usize) {
    if kind == "int" {
        assert_eq!(value, &Value::Int(int(7)));
    } else if kind == "string" || depth == 0 {
        let Value::Str(text) = value else {
            panic!("expected string leaf")
        };
        assert_eq!(text.len(), LEAF_BYTES);
        assert!(text.as_bytes().iter().all(|&b| b == b'x'));
    } else {
        match (kind, value) {
            ("tuple", Value::Tuple(items)) => {
                assert_eq!(items.len(), PAYLOAD_WIDTH);
                for item in items {
                    verify_payload(item, kind, depth - 1);
                }
            }
            ("list", Value::List(items)) => {
                assert_eq!(items.len(), PAYLOAD_WIDTH);
                for item in items.iter() {
                    verify_payload(&item, kind, depth - 1);
                }
            }
            ("hashmap", Value::HashMap(map)) => {
                assert_eq!(map.len(), PAYLOAD_WIDTH);
                for index in 0..PAYLOAD_WIDTH {
                    verify_payload(
                        map.entries
                            .get(&format!("key{index}"))
                            .expect("expected key"),
                        kind,
                        depth - 1,
                    );
                }
            }
            _ => panic!("unexpected payload shape"),
        }
    }
}

fn vm_flat_map(case: &str, n: usize, kind: &str, depth: usize) {
    let (visits, pushes, expected, actual_depth) = counts(case, n, depth);
    let (bytecode, prepare) = measured(|| bytecode(case, n, kind, depth));
    let (mut vm, vm_prepare) = measured(|| {
        let mut vm = VM::new(bytecode);
        vm.enable_observation(VmObservationOptions::default());
        vm
    });
    let (_, run) = measured(|| vm.run().expect("VM workload must complete"));
    let Value::List(output) = vm.last_value().expect("output") else {
        panic!("expected List")
    };
    assert_eq!(output.len(), expected);
    // Each mapper completes exactly one Return; this is a workload-specific
    // proxy, not a general callback counter. There are no other user functions.
    let callbacks = vm.observation().unwrap().stats.per_opcode["Return"];
    assert_eq!(callbacks as usize, visits);
    let (output, extract) = measured(|| {
        let Value::List(output) = vm.last_value().unwrap() else {
            unreachable!()
        };
        output.clone()
    });
    let (_, drop_vm) = measured(|| drop(vm));
    if std::env::var_os("PROBE_VERIFY").is_some() {
        assert_eq!(output.len(), expected);
        let mut checked = 0;
        for value in output.iter() {
            verify_payload(&value, kind, PAYLOAD_DEPTH);
            checked += 1;
        }
        assert_eq!(checked, expected);
    }
    let (_, drop_output) = measured(|| drop(output));
    // JSON formatting/output is deferred until every measured interval ends.
    for (phase, metrics) in [
        ("prepare_bytecode", prepare),
        ("prepare_vm", vm_prepare),
        ("run", run),
        ("extract_output", extract),
        ("drop_vm", drop_vm),
        ("drop_output", drop_output),
    ] {
        emit(
            case,
            phase,
            n,
            kind,
            metrics,
            serde_json::json!({
                "entry": "manual_bytecode",
                "input_visits_expected": visits,
                "builder_pushes_expected": pushes,
                "callback_calls_observed": callbacks,
                "final_output": expected,
                "depth": actual_depth,
                "payload_container_depth": if matches!(kind, "int" | "string") { 0 } else { PAYLOAD_DEPTH },
                "payload_container_width": if matches!(kind, "int" | "string") { 0 } else { PAYLOAD_WIDTH },
                "payload_leaf_count": if matches!(kind, "int" | "string") { 1 } else { PAYLOAD_WIDTH.pow(PAYLOAD_DEPTH.try_into().unwrap()) },
                "payload_leaf_string_bytes": if kind == "int" { 0 } else { LEAF_BYTES },
                "observer_enabled": true,
            }),
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert!(
        (4..=5).contains(&args.len()),
        "usage: list-value-cost-probe case n payload [depth]"
    );
    vm_flat_map(
        &args[1],
        args[2].parse().unwrap(),
        &args[3],
        args.get(4).map(|s| s.parse().unwrap()).unwrap_or(8),
    );
}
