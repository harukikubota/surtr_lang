use super::tests::base_bytecode;
use super::*;
use crate::builtin::{call_builtin, BuiltinOutcome};
use sindr::builtin::builtin_id_by_name;
use sindr::runtime::RegexHandle;

fn start(vm: &mut VM, name: &str, pattern: &str, input: &str) -> ExecutionContext {
    let mut args = vec![
        Value::Regex(RegexHandle {
            pattern: pattern.into(),
        }),
        Value::Str(input.into()),
    ];
    if name == "replace_all" {
        args.push(Value::Str("<$0>".into()));
    }
    let outcome = call_builtin(vm, builtin_id_by_name(name).unwrap(), args).unwrap();
    assert!(
        matches!(outcome, BuiltinOutcome::Resume(_)),
        "collection must yield before searching"
    );
    VM::standalone_builtin_context(outcome)
}

fn finish(vm: &mut VM, mut context: ExecutionContext, quantum: u64) -> Value {
    for _ in 0..20000 {
        match vm.run_quantum(&mut context, &mut Budget::new(quantum)) {
            ProcessRunOutcome::QuantumExpired => {}
            ProcessRunOutcome::Halted(value) => return value,
            other => panic!("unexpected regex result: {other:?}"),
        }
    }
    panic!("regex did not finish");
}

#[test]
fn regex_collection_yields_and_restores_mid_search_with_small_budgets() {
    for name in ["find_all", "split", "replace_all"] {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let mut context = start(&mut vm, name, "a?", &"あa".repeat(300));
        assert!(matches!(
            vm.run_quantum(&mut context, &mut Budget::new(0)),
            ProcessRunOutcome::QuantumExpired
        ));
        assert!(matches!(
            vm.run_quantum(&mut context, &mut Budget::new(17)),
            ProcessRunOutcome::QuantumExpired
        ));
        let saved = context.clone();
        let result = finish(&mut vm, context, 1);
        assert_eq!(finish(&mut vm, saved, 256), result);
    }
}

#[test]
fn regex_collection_cancellation_never_returns_partial_output() {
    for name in ["find_all", "split", "replace_all"] {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let mut context = start(&mut vm, name, ".", &"あa".repeat(300));
        assert!(matches!(
            vm.run_quantum(&mut context, &mut Budget::new(8)),
            ProcessRunOutcome::QuantumExpired
        ));
        VM::cancel_execution_context(&mut context, RuntimeError::new("cancelled regex"));
        let ProcessRunOutcome::Failed(error) = vm.run_quantum(&mut context, &mut Budget::new(256))
        else {
            panic!("partial regex must fail")
        };
        assert_eq!(error.message, "cancelled regex");
    }
}

#[test]
fn regex_collection_matches_crate_iteration_split_and_capture_expansion() {
    let mut inputs = vec![String::new()];
    let mut previous = vec![String::new()];
    for _ in 0..3 {
        previous = previous
            .iter()
            .flat_map(|prefix| {
                ["a", "b", "あ", "\n"]
                    .into_iter()
                    .map(move |suffix| format!("{prefix}{suffix}"))
            })
            .collect();
        inputs.extend(previous.iter().cloned());
    }
    let patterns = [
        "",
        "a",
        "a?",
        "a*",
        "a+",
        "a|",
        "|a",
        ".",
        "^",
        "$",
        "(?m)^|$",
        "\\b",
        "[^a]*",
        "(?P<letter>a)?(あ|b)?",
        "(?s).*?",
        "あ?",
    ];
    for pattern in patterns {
        let re = regex::Regex::new(pattern).unwrap();
        for input in &inputs {
            let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
            let context = start(&mut vm, "find_all", pattern, input);
            let Value::List(matches) = finish(&mut vm, context, 7) else {
                panic!("find_all List")
            };
            let spans: Vec<_> = matches
                .iter()
                .map(|value| {
                    let Value::RegexMatch(matched) = value else {
                        panic!("match handle")
                    };
                    assert_eq!(matched.input.as_str(), input);
                    assert!(
                        input.is_char_boundary(matched.start)
                            && input.is_char_boundary(matched.end)
                    );
                    (matched.start, matched.end)
                })
                .collect();
            assert_eq!(
                spans,
                re.find_iter(input)
                    .map(|m| (m.start(), m.end()))
                    .collect::<Vec<_>>(),
                "{pattern:?} / {input:?}"
            );
            let context = start(&mut vm, "split", pattern, input);
            assert_eq!(
                finish(&mut vm, context, 7),
                Value::List(ListHandle::from_items(
                    re.split(input)
                        .map(|part| Value::Str(part.into()))
                        .collect()
                )),
                "{pattern:?} / {input:?}"
            );
            for replacement in [
                "literal",
                "<$0>",
                "$1:${letter}:$2",
                "$$${missing}$99",
                "${1}a$letter",
            ] {
                let outcome = call_builtin(
                    &mut vm,
                    builtin_id_by_name("replace_all").unwrap(),
                    vec![
                        Value::Regex(RegexHandle {
                            pattern: pattern.into(),
                        }),
                        Value::Str(input.clone()),
                        Value::Str(replacement.into()),
                    ],
                )
                .unwrap();
                let context = VM::standalone_builtin_context(outcome);
                assert_eq!(
                    finish(&mut vm, context, 7),
                    Value::Str(re.replace_all(input, replacement).into_owned()),
                    "{pattern:?} / {input:?} / {replacement:?}"
                );
            }
        }
    }
}
