//! RT-4 public Rust API and whole-VM measurements, outside correctness CI.
mod metrics;
use eldr::{vm::VmObservationOptions, VM};
use metrics::{emit, measured};
use sindr::builtin::builtin_id_by_name;
use sindr::ir::{Bytecode, Constant, FunctionEntry, Opcode};
use sindr::primitives::int;
use sindr::runtime::{HashMapHandle, Value};

fn key(i: usize, same: bool) -> String {
    assert!(i < 10_000_000);
    format!("k{:07}", if same { 0 } else { i })
}

fn payload(i: usize, kind: &str) -> Value {
    match kind {
        "int" => Value::Int(int(i)),
        "string" => Value::Str(format!("{i:016}{}", "x".repeat(240))),
        _ => panic!("payload must be int or string"),
    }
}

fn entries(n: usize, same: bool, kind: &str) -> Vec<(String, Value)> {
    (0..n).map(|i| (key(i, same), payload(i, kind))).collect()
}

fn verify(map: &HashMapHandle, n: usize, same: bool, kind: &str) {
    assert_eq!(map.len(), if same { usize::from(n > 0) } else { n });
    for i in 0..n {
        let index = if same { n - 1 } else { i };
        assert_eq!(
            map.entries.get(&key(i, same)).unwrap(),
            &payload(index, kind)
        );
    }
}

fn call(name: &str, arity: u8) -> Opcode {
    Opcode::CallBuiltin {
        builtin_id: builtin_id_by_name(name).expect("canonical builtin"),
        arity,
        span_start: 0,
        span_end: 0,
    }
}

fn bytecode(n: usize, same: bool, kind: &str, map_values: bool) -> Bytecode {
    let mut code = Vec::new();
    let mut constants = Vec::new();
    for (key, value) in entries(n, same, kind) {
        code.push(Opcode::LoadConst(constants.len().try_into().unwrap()));
        constants.push(Constant::Str(key));
        code.push(Opcode::LoadConst(constants.len().try_into().unwrap()));
        constants.push(match value {
            Value::Int(v) => Constant::Int(v),
            Value::Str(v) => Constant::Str(v),
            _ => unreachable!(),
        });
        code.push(Opcode::TupleNew { len: 2 });
    }
    code.push(Opcode::ListFromItems {
        len: n.try_into().unwrap(),
    });
    code.push(call("map_from_entries", 1));
    let mut functions = Vec::new();
    if map_values {
        code.push(Opcode::LoadFunctionRef(0));
        code.push(call("map_values", 2));
    }
    code.push(Opcode::Halt);
    if map_values {
        let pc = code.len();
        code.extend([Opcode::LoadLocal(0), Opcode::Return]);
        functions.push(FunctionEntry {
            fun_idx: 0,
            entry_pc: pc.try_into().unwrap(),
            end_pc: code.len().try_into().unwrap(),
            num_locals: 1,
            arity: 1,
            qualified_name: Some("Probe::identity".into()),
            signature: None,
            span_start: 0,
            span_end: 0,
            flags: Default::default(),
        });
    }
    Bytecode {
        opcodes: code,
        constants,
        functions,
        ..Default::default()
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 4, "usage: hashmap-cost-probe case n payload");
    let case = args[1].as_str();
    let n: usize = args[2].parse().unwrap();
    assert!(n < 10_000_000);
    let kind = args[3].as_str();
    assert!(matches!(kind, "int" | "string"));
    assert!(matches!(
        case,
        "rust_distinct" | "rust_same" | "sort" | "vm_distinct" | "vm_same" | "vm_map_values"
    ));
    let same = case.ends_with("_same");
    let mut rows = Vec::with_capacity(3);
    let mut callbacks = 0;
    if case.starts_with("vm_") {
        let (mut vm, prep) = measured(|| {
            let mut vm = VM::new(bytecode(n, same, kind, case == "vm_map_values"));
            vm.enable_observation(VmObservationOptions::default());
            vm
        });
        let (_, run) = measured(|| vm.run().expect("VM must complete"));
        let Value::HashMap(output) = vm.last_value().expect("output") else {
            panic!("expected HashMap")
        };
        assert_eq!(output.len(), if same { usize::from(n > 0) } else { n });
        let observation = vm.observation().unwrap();
        callbacks = observation
            .stats
            .per_opcode
            .get("Return")
            .copied()
            .unwrap_or(0);
        assert_eq!(
            callbacks as usize,
            if case == "vm_map_values" { n } else { 0 }
        );
        if std::env::var_os("PROBE_VERIFY").is_some() {
            verify(output, n, same, kind);
        }
        let (_, drop) = measured(|| drop(vm));
        rows.extend([("prepare", prep), ("run", run), ("drop", drop)]);
    } else if case == "sort" {
        // Build input directly, so setup does not itself use quadratic insert.
        let (map, prep) = measured(|| HashMapHandle {
            entries: entries(n, false, kind).into_iter().collect(),
        });
        let (sorted, run) = measured(|| map.sorted_entries());
        assert_eq!(sorted.len(), n);
        if std::env::var_os("PROBE_VERIFY").is_some() {
            verify(&map, n, false, kind);
            for (i, (k, v)) in sorted.iter().enumerate() {
                assert_eq!(k, &key(i, false));
                assert_eq!(v, &payload(i, kind));
            }
        }
        let (_, drop) = measured(|| drop((sorted, map)));
        rows.extend([("prepare", prep), ("run", run), ("drop", drop)]);
    } else {
        let (input, prep) = measured(|| entries(n, same, kind));
        let (map, run) = measured(|| HashMapHandle::from_entries(input));
        assert_eq!(map.len(), if same { usize::from(n > 0) } else { n });
        if std::env::var_os("PROBE_VERIFY").is_some() {
            verify(&map, n, same, kind);
        }
        let (_, drop) = measured(|| drop(map));
        rows.extend([("prepare", prep), ("run", run), ("drop", drop)]);
    }
    for (phase, metrics) in rows {
        emit(
            case,
            phase,
            n,
            kind,
            metrics,
            serde_json::json!({
                "entry": if case.starts_with("vm_") { "manual_bytecode" } else { "public_rust_api" },
                "existing_entry_clones_expected": 0,
                "callback_returns_observed": callbacks,
                "final_output": if same { usize::from(n > 0) } else { n },
                "key_bytes": 8, "payload_string_bytes": if kind == "string" { 256 } else { 0 },
                "observer_enabled": case.starts_with("vm_"),
            }),
        );
    }
}
