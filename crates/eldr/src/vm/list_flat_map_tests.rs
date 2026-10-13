use super::process_continuation::{DetachedCompletion, DetachedTaskState};
use super::tests::{base_bytecode, function_entry};
use super::*;
use crate::builtin::{flat_map_metrics, reset_flat_map_metrics};
use sindr::builtin::builtin_id_by_name;

fn flat_map_callable() -> Callable {
    Callable {
        target: CallableTarget::Builtin(
            builtin_id_by_name("list_flat_map").expect("list_flat_map must be registered"),
        ),
        lexical_captures: Vec::new(),
        metadata: Default::default(),
    }
}

fn finish(vm: &mut VM, context: &mut ExecutionContext, quantum: u64) -> Value {
    for _ in 0..100_000 {
        match vm.run_quantum(context, &mut Budget::new(quantum)) {
            ProcessRunOutcome::QuantumExpired => {}
            ProcessRunOutcome::Halted(value) => return value,
            other => panic!("unexpected flat_map outcome: {other:?}"),
        }
    }
    panic!("flat_map did not finish within its bounded test workload");
}

#[test]
fn flat_map_mixed_lists_keep_logical_order_across_quantum_boundaries() {
    let middle = ListHandle::cons(
        Value::Int(int(1)),
        &ListHandle::from_items(vec![Value::Int(int(2)), Value::Int(int(3))]),
    );
    let input = ListHandle::cons(
        Value::List(ListHandle::empty()),
        &ListHandle::from_items(vec![
            Value::List(middle),
            Value::List(ListHandle::from_items(vec![
                Value::Int(int(4)),
                Value::Int(int(5)),
            ])),
        ]),
    );
    let expected = Value::List(ListHandle::from_items(
        (1..=5).map(|n| Value::Int(int(n))).collect(),
    ));
    for quantum in [1, 3, 37] {
        reset_flat_map_metrics();
        let mut bytecode = base_bytecode(vec![Opcode::LoadLocal(0), Opcode::Return]);
        bytecode.functions = vec![function_entry(0, 0, 1, 1, Some("Main::identity"))];
        let mut vm = VM::new(bytecode);
        let mapper = vm.callable_for_function(0);
        let mut context = vm
            .prepare_callable_context(
                flat_map_callable(),
                vec![Value::List(input.clone()), Value::Callable(mapper)],
            )
            .unwrap();
        let before = format!("{context:?}");
        assert!(matches!(
            vm.run_quantum(&mut context, &mut Budget::new(0)),
            ProcessRunOutcome::QuantumExpired
        ));
        assert_eq!(format!("{context:?}"), before);
        assert_eq!(finish(&mut vm, &mut context, quantum), expected);
        assert!(context.continuations.is_empty());
        assert!(context.pending_invocation.is_none());
        let metrics = flat_map_metrics();
        assert_eq!(metrics.input_visits, 3);
        assert_eq!(metrics.callback_calls, 3);
        assert_eq!(metrics.builder_pushes, 5);
        assert_eq!(metrics.builder_finishes, 1);
        assert_eq!(metrics.builder_clones, 0);
    }
}

fn flat_map_opcode() -> Opcode {
    Opcode::CallBuiltin {
        builtin_id: builtin_id_by_name("list_flat_map").unwrap(),
        arity: 2,
        span_start: 0,
        span_end: 0,
    }
}

fn list(values: impl IntoIterator<Item = Value>) -> Value {
    Value::List(ListHandle::from_items(values.into_iter().collect()))
}

fn constant_mapper_vm(output: Value) -> (VM, Callable) {
    let mut bytecode = base_bytecode(vec![Opcode::LoadLocal(0), Opcode::Return]);
    bytecode.functions = vec![function_entry(0, 0, 2, 2, Some("Main::constant"))];
    let vm = VM::new(bytecode);
    let mut mapper = vm.callable_for_function(0);
    mapper.lexical_captures.push(output);
    (vm, mapper)
}

#[test]
fn flat_map_empty_inputs_and_empty_results_are_budgeted_without_builder_copies() {
    for input_len in [0, 100] {
        for quantum in [1, 7, 64] {
            reset_flat_map_metrics();
            let (mut vm, mapper) = constant_mapper_vm(list([]));
            let input = list((0..input_len).map(|n| Value::Int(int(n))));
            let mut context = vm
                .prepare_callable_context(flat_map_callable(), vec![input, Value::Callable(mapper)])
                .unwrap();
            if input_len > 0 {
                let mut first = Budget::new(16);
                assert!(matches!(
                    vm.run_quantum(&mut context, &mut first),
                    ProcessRunOutcome::QuantumExpired
                ));
                assert_eq!(first.reductions(), 16);
                assert!(flat_map_metrics().input_visits > 0);
                assert!(flat_map_metrics().input_visits < input_len);
            }
            assert_eq!(finish(&mut vm, &mut context, quantum), list([]));
            let metrics = flat_map_metrics();
            assert_eq!(metrics.input_visits, input_len);
            assert_eq!(metrics.callback_calls, input_len);
            assert_eq!(metrics.builder_pushes, 0);
            assert_eq!(metrics.builder_finishes, 1);
            assert_eq!(metrics.builder_clones, 0);
            assert!(
                vm.process_runtime.futures.is_empty(),
                "CPU yield must not allocate a fake Future"
            );
        }
    }
}

fn add_peer(vm: &mut VM) {
    let pc = vm.bytecode.opcodes.len() as u32;
    let constant = vm.bytecode.constants.len() as u32;
    let index = vm.bytecode.functions.len() as u32;
    vm.bytecode.constants.push(Constant::Str("peer".into()));
    vm.bytecode.opcodes.extend([
        Opcode::LoadConst(constant),
        Opcode::CallBuiltin {
            builtin_id: builtin_id_by_name("print").unwrap(),
            arity: 1,
            span_start: 0,
            span_end: 0,
        },
        Opcode::Return,
    ]);
    vm.bytecode
        .functions
        .push(function_entry(index, pc, 0, 0, Some("Main::peer")));
    let peer = vm.callable_for_function(index);
    vm.start_task(peer, TaskMode::Launch, None).unwrap();
}

#[test]
fn flat_map_single_large_result_yields_during_output_and_allows_peer_progress() {
    reset_flat_map_metrics();
    let output = list((0..4096).map(|n| Value::Int(int(n))));
    let (mut vm, mapper) = constant_mapper_vm(output.clone());
    vm = vm.with_output_capture();
    add_peer(&mut vm);
    let mut context = vm
        .prepare_callable_context(
            flat_map_callable(),
            vec![list([Value::Unit]), Value::Callable(mapper)],
        )
        .unwrap();
    assert!(matches!(
        vm.run_quantum(&mut context, &mut Budget::new(32)),
        ProcessRunOutcome::QuantumExpired
    ));
    let before = flat_map_metrics();
    assert_eq!(before.callback_calls, 1);
    assert!(before.builder_pushes > 0 && before.builder_pushes < 4096);
    assert_eq!(before.builder_finishes, 0);
    assert_eq!(before.builder_clones, 0);
    assert!(vm.process_runtime.futures.is_empty());
    vm.drive_ready_runtime_quantum().unwrap();
    assert_eq!(vm.output.as_ref().unwrap(), &["peer".to_string()]);
    assert_eq!(
        flat_map_metrics(),
        before,
        "running a peer must not copy or advance the suspended builder"
    );
    assert_eq!(finish(&mut vm, &mut context, 11), output);
    let metrics = flat_map_metrics();
    assert_eq!(metrics.builder_pushes, 4096);
    assert_eq!(metrics.callback_calls, 1);
    assert_eq!(metrics.builder_finishes, 1);
    assert_eq!(metrics.builder_clones, 0);
}

#[test]
fn flat_map_cpu_callback_uses_shared_budget_and_ordinary_driver_runs_peer() {
    reset_flat_map_metrics();
    let mut code = Vec::new();
    for _ in 0..256 {
        code.extend([Opcode::LoadConst(0), Opcode::Pop]);
    }
    code.extend([
        Opcode::LoadConst(1),
        Opcode::CallBuiltin {
            builtin_id: builtin_id_by_name("print").unwrap(),
            arity: 1,
            span_start: 0,
            span_end: 0,
        },
        Opcode::Pop,
        Opcode::LoadLocal(0),
        Opcode::Return,
    ]);
    let mut bytecode = base_bytecode(code);
    bytecode.constants = vec![Constant::Unit, Constant::Str("mapper done".into())];
    bytecode.functions = vec![function_entry(0, 0, 2, 2, Some("Main::cpu_mapper"))];
    let mut vm = VM::new(bytecode).with_output_capture();
    let mut mapper = vm.callable_for_function(0);
    mapper.lexical_captures.push(list([Value::Int(int(42))]));
    add_peer(&mut vm);
    let mut context = vm
        .prepare_callable_context(
            flat_map_callable(),
            vec![list([Value::Unit]), Value::Callable(mapper)],
        )
        .unwrap();
    let mut first = Budget::new(32);
    assert!(matches!(
        vm.run_quantum(&mut context, &mut first),
        ProcessRunOutcome::QuantumExpired
    ));
    assert_eq!(first.reductions(), 32);
    assert_eq!(flat_map_metrics().callback_calls, 1);
    assert_eq!(flat_map_metrics().builder_pushes, 0);
    assert!(vm.output.as_ref().unwrap().is_empty());
    assert!(
        matches!(vm.resume_execution(context),StepOutcome::Halt(value) if value==list([Value::Int(int(42))]))
    );
    assert_eq!(
        vm.output.as_ref().unwrap(),
        &["peer".to_string(), "mapper done".to_string()]
    );
    assert_eq!(flat_map_metrics().builder_clones, 0);
}

#[test]
fn flat_map_nested_singletons_preserve_k_times_m_output_work() {
    for k in [1, 3, 5] {
        for m in [0, 1, 17] {
            reset_flat_map_metrics();
            let mut bytecode = base_bytecode(vec![
                Opcode::LoadLocal(0),
                Opcode::LoadLocal(1),
                flat_map_opcode(),
                Opcode::Return,
                Opcode::LoadLocal(0),
                Opcode::Return,
            ]);
            bytecode.functions = vec![
                function_entry(0, 0, 3, 3, Some("Main::nested")),
                function_entry(1, 4, 2, 2, Some("Main::leaf")),
            ];
            let mut vm = VM::new(bytecode);
            let output = list((0..m).map(|n| Value::Int(int(n))));
            let singleton = list([Value::Unit]);
            let mut mapper = vm.callable_for_function(1);
            mapper.lexical_captures.push(output.clone());
            for _ in 1..k {
                let mut outer = vm.callable_for_function(0);
                outer.lexical_captures = vec![singleton.clone(), Value::Callable(mapper)];
                mapper = outer;
            }
            let mut context = vm
                .prepare_callable_context(
                    flat_map_callable(),
                    vec![singleton, Value::Callable(mapper)],
                )
                .unwrap();
            assert_eq!(finish(&mut vm, &mut context, 1), output);
            let metrics = flat_map_metrics();
            assert_eq!(metrics.input_visits, k);
            assert_eq!(metrics.callback_calls, k);
            assert_eq!(metrics.builder_pushes, k * m);
            assert_eq!(metrics.builder_finishes, k);
            assert_eq!(metrics.builder_clones, 0);
        }
    }
}

#[test]
fn flat_map_three_stage_cartesian_product_counts_b_e_and_builder_work() {
    reset_flat_map_metrics();
    let mut bytecode = base_bytecode(vec![
        Opcode::LoadLocal(0),
        Opcode::LoadFunctionRef(1),
        Opcode::LoadLocal(0),
        Opcode::LoadLocal(1),
        Opcode::CaptureClosure(2),
        flat_map_opcode(),
        Opcode::Return,
        Opcode::LoadLocal(0),
        Opcode::LoadFunctionRef(2),
        Opcode::LoadLocal(1),
        Opcode::LoadLocal(2),
        Opcode::CaptureClosure(2),
        flat_map_opcode(),
        Opcode::Return,
        Opcode::LoadLocal(0),
        Opcode::LoadLocal(1),
        Opcode::LoadLocal(2),
        Opcode::TupleNew { len: 3 },
        Opcode::ListFromItems { len: 1 },
        Opcode::Return,
    ]);
    bytecode.functions = vec![
        function_entry(0, 0, 2, 2, Some("Main::x")),
        function_entry(1, 7, 3, 3, Some("Main::y")),
        function_entry(2, 14, 3, 3, Some("Main::z")),
    ];
    let mut vm = VM::new(bytecode);
    let values = list((0..10).map(|n| Value::Int(int(n))));
    let mut mapper = vm.callable_for_function(0);
    mapper.lexical_captures.push(values.clone());
    let mut context = vm
        .prepare_callable_context(flat_map_callable(), vec![values, Value::Callable(mapper)])
        .unwrap();
    let expected = list((0..10).flat_map(|x| {
        (0..10).flat_map(move |y| {
            (0..10).map(move |z| {
                Value::Tuple(vec![
                    Value::Int(int(x)),
                    Value::Int(int(y)),
                    Value::Int(int(z)),
                ])
            })
        })
    }));
    assert_eq!(finish(&mut vm, &mut context, 7), expected);
    let metrics = flat_map_metrics();
    assert_eq!(metrics.input_visits, 1110);
    assert_eq!(metrics.callback_calls, 1110);
    assert_eq!(metrics.builder_pushes, 3000);
    assert_eq!(metrics.builder_finishes, 111);
    assert_eq!(metrics.builder_clones, 0);
}

fn fail(vm: &mut VM, context: &mut ExecutionContext, quantum: u64) -> RuntimeError {
    for _ in 0..1000 {
        match vm.run_quantum(context, &mut Budget::new(quantum)) {
            ProcessRunOutcome::QuantumExpired => {}
            ProcessRunOutcome::Failed(error) => return error,
            other => panic!("expected flat_map failure, got {other:?}"),
        }
    }
    panic!("flat_map did not fail within its bounded test workload");
}

#[test]
fn flat_map_mapper_failure_discards_partial_builder_and_stops_later_callbacks() {
    for runtime_failure in [false, true] {
        for quantum in [1, 64] {
            reset_flat_map_metrics();
            let mut code = vec![
                Opcode::LoadLocal(0),
                Opcode::CallBuiltin {
                    builtin_id: builtin_id_by_name("print").unwrap(),
                    arity: 1,
                    span_start: 0,
                    span_end: 1,
                },
                Opcode::Pop,
                Opcode::LoadLocal(0),
                Opcode::LoadConst(0),
                Opcode::EqInt,
                Opcode::JumpIfFalse(0),
                Opcode::LoadConst(1),
            ];
            if runtime_failure {
                code.push(Opcode::ListHead);
            }
            code.push(Opcode::Return);
            code[6] = Opcode::JumpIfFalse(code.len() as u32);
            code.extend([
                Opcode::LoadLocal(0),
                Opcode::ListFromItems { len: 1 },
                Opcode::Return,
            ]);
            let mut bytecode = base_bytecode(code);
            bytecode.constants = vec![Constant::Int(int(2)), Constant::Int(int(99))];
            bytecode.functions = vec![function_entry(0, 0, 1, 1, Some("Main::mapper"))];
            let mut vm = VM::new(bytecode).with_output_capture().with_source(
                "List::flat_map(values, mapper)".into(),
                "flat-map.srt".into(),
            );
            vm.frames[0].call_site = Some((0, 14));
            vm.frames[0].trace_frame = Some(vm.trace_frame_for_builtin("list_flat_map", 0, 14));
            let mapper = vm.callable_for_function(0);
            let mut context = vm
                .prepare_callable_context(
                    flat_map_callable(),
                    vec![
                        list((1..=3).map(|n| Value::Int(int(n)))),
                        Value::Callable(mapper),
                    ],
                )
                .unwrap();
            let error = fail(&mut vm, &mut context, quantum);
            assert!(error.message.contains("List"), "{error:?}");
            assert!(error.context.call_site.is_some());
            assert!(error
                .context
                .stack_trace
                .iter()
                .any(|frame| frame.function.as_deref() == Some("list_flat_map")));
            assert_eq!(
                vm.output.as_ref().unwrap(),
                &["1".to_string(), "2".to_string()]
            );
            let metrics = flat_map_metrics();
            assert_eq!(metrics.input_visits, 2);
            assert_eq!(metrics.callback_calls, 2);
            assert_eq!(metrics.builder_pushes, 1);
            assert_eq!(metrics.builder_finishes, 0);
            assert_eq!(metrics.builder_clones, 0);
            assert!(context.continuations.is_empty());
            assert!(context.pending_invocation.is_none());
            assert!(
                context.stack.is_empty(),
                "an unfinished builder must never become a result"
            );
        }
    }
}

#[test]
fn flat_map_mapper_future_wait_preserves_exactly_once_effects_and_output_order() {
    reset_flat_map_metrics();
    let mut bytecode = base_bytecode(vec![
        Opcode::LoadLocal(1),
        Opcode::CallBuiltin {
            builtin_id: builtin_id_by_name("print").unwrap(),
            arity: 1,
            span_start: 0,
            span_end: 1,
        },
        Opcode::Pop,
        Opcode::LoadLocal(0),
        Opcode::Return,
    ]);
    bytecode.functions = vec![function_entry(0, 0, 2, 2, Some("Main::waiting_mapper"))];
    let mut vm = VM::new(bytecode).with_output_capture();
    let future = vm.process_runtime.allocate_future(None, None, false);
    let mut mapper = vm.callable_for_function(0);
    mapper.lexical_captures.push(Value::PendingFuture(future));
    let mut context = vm
        .prepare_callable_context(
            flat_map_callable(),
            vec![
                list([Value::Int(int(1)), Value::Int(int(2))]),
                Value::Callable(mapper),
            ],
        )
        .unwrap();
    assert!(
        matches!(vm.run_quantum(&mut context,&mut Budget::new(64)),ProcessRunOutcome::Pending(id) if id==future)
    );
    assert_eq!(vm.output.as_ref().unwrap(), &["1".to_string()]);
    assert_eq!(flat_map_metrics().callback_calls, 1);
    assert_eq!(flat_map_metrics().builder_pushes, 0);
    assert_eq!(vm.process_runtime.futures.len(), 1);
    vm.process_runtime
        .resolve_future(future, list([Value::Int(int(7)), Value::Int(int(8))]));
    assert_eq!(
        finish(&mut vm, &mut context, 1),
        list([7, 8, 7, 8].map(|n| Value::Int(int(n))))
    );
    assert_eq!(
        vm.output.as_ref().unwrap(),
        &["1".to_string(), "2".to_string()]
    );
    let metrics = flat_map_metrics();
    assert_eq!(metrics.callback_calls, 2);
    assert_eq!(metrics.builder_pushes, 4);
    assert_eq!(metrics.builder_clones, 0);
}

#[test]
fn flat_map_cancellation_discards_builder_during_output_or_mapper_wait() {
    for waiting in [false, true] {
        reset_flat_map_metrics();
        let output = list((0..4096).map(|n| Value::Int(int(n))));
        let (mut vm, mut mapper) = constant_mapper_vm(output);
        let future = if waiting {
            let future = vm.process_runtime.allocate_future(None, None, false);
            mapper.lexical_captures[0] = Value::PendingFuture(future);
            Some(future)
        } else {
            None
        };
        let mut context = vm
            .prepare_callable_context(
                flat_map_callable(),
                vec![list([Value::Unit, Value::Unit]), Value::Callable(mapper)],
            )
            .unwrap();
        let outcome = vm.run_quantum(&mut context, &mut Budget::new(32));
        if let Some(future) = future {
            assert!(matches!(outcome, ProcessRunOutcome::Pending(id) if id == future));
            assert_eq!(flat_map_metrics().builder_pushes, 0);
        } else {
            assert!(matches!(outcome, ProcessRunOutcome::QuantumExpired));
            assert!((1..4096).contains(&flat_map_metrics().builder_pushes));
        }
        let before = flat_map_metrics();
        assert_eq!(before.callback_calls, 1);
        VM::cancel_execution_context(&mut context, RuntimeError::new("cancelled"));
        assert_eq!(fail(&mut vm, &mut context, 1).message, "cancelled");
        assert_eq!(flat_map_metrics(), before);
        assert_eq!(flat_map_metrics().builder_finishes, 0);
        assert!(context.continuations.is_empty());
        assert!(context.pending_invocation.is_none());
        assert!(context.stack.is_empty());
    }
}

#[test]
fn flat_map_direct_closure_tail_partial_inject_and_compose_use_same_continuation() {
    for route in ["direct", "closure", "tail", "partial", "inject", "compose"] {
        reset_flat_map_metrics();
        let mut bytecode = base_bytecode(vec![
            flat_map_opcode(),
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::Return,
            Opcode::LoadLocal(0),
            Opcode::LoadLocal(1),
            Opcode::LoadLocal(2),
            Opcode::TailCallClosure {
                arity: 2,
                span_start: 0,
                span_end: 0,
            },
        ]);
        bytecode.functions = vec![
            function_entry(0, 2, 1, 1, Some("Main::identity")),
            function_entry(1, 4, 3, 3, Some("Main::tail")),
        ];
        let mut vm = VM::new(bytecode);
        let input = list([
            list([Value::Int(int(1))]),
            list([Value::Int(int(2)), Value::Int(int(3))]),
        ]);
        let expected = list([1, 2, 3].map(|n| Value::Int(int(n))));
        let mapper = vm.callable_for_function(0);
        let (callable, args) = match route {
            "direct" => {
                vm.stack = vec![input, Value::Callable(mapper)].into();
                vm.run().unwrap();
                assert_eq!(vm.last_result, Some(expected));
                assert_eq!(flat_map_metrics().builder_clones, 0);
                continue;
            }
            "closure" => (flat_map_callable(), vec![input, Value::Callable(mapper)]),
            "tail" => {
                let mut tail = vm.callable_for_function(1);
                tail.lexical_captures
                    .push(Value::Callable(flat_map_callable()));
                (tail, vec![input, Value::Callable(mapper)])
            }
            "partial" => {
                vm.bytecode.callable_templates.push(CallableTemplate {
                    template_id: 0,
                    kind: CallableTemplateKind::PartialDirectCall {
                        target: CallableTemplateDirectTarget::Builtin(
                            builtin_id_by_name("list_flat_map").unwrap(),
                        ),
                        arg_sources: vec![
                            CallableTemplateArg::Bound(0),
                            CallableTemplateArg::Runtime(0),
                        ],
                    },
                    metadata: Default::default(),
                });
                (
                    Callable {
                        target: CallableTarget::Template(0),
                        lexical_captures: vec![input],
                        metadata: Default::default(),
                    },
                    vec![Value::Callable(mapper)],
                )
            }
            _ => {
                vm.bytecode.callable_templates.push(CallableTemplate {
                    template_id: 0,
                    kind: CallableTemplateKind::InjectDirectCall {
                        target: CallableTemplateDirectTarget::Builtin(
                            builtin_id_by_name("list_flat_map").unwrap(),
                        ),
                        bound_arg_count: 1,
                    },
                    metadata: Default::default(),
                });
                let inject = Callable {
                    target: CallableTarget::Template(0),
                    lexical_captures: vec![Value::Callable(mapper)],
                    metadata: Default::default(),
                };
                if route == "inject" {
                    (inject, vec![input])
                } else {
                    vm.bytecode.callable_templates.push(CallableTemplate {
                        template_id: 1,
                        kind: CallableTemplateKind::ComposeDirect {
                            flavor: CallableTemplateComposeFlavor::Plain,
                        },
                        metadata: Default::default(),
                    });
                    (
                        Callable {
                            target: CallableTarget::Template(1),
                            lexical_captures: vec![
                                Value::Callable(inject),
                                Value::Callable(vm.callable_for_function(0)),
                            ],
                            metadata: Default::default(),
                        },
                        vec![input],
                    )
                }
            }
        };
        let mut context = vm.prepare_callable_context(callable, args).unwrap();
        assert_eq!(finish(&mut vm, &mut context, 1), expected, "route {route}");
        let metrics = flat_map_metrics();
        assert_eq!(metrics.callback_calls, 2, "route {route}");
        assert_eq!(metrics.builder_pushes, 3, "route {route}");
        assert_eq!(metrics.builder_finishes, 1, "route {route}");
        assert_eq!(metrics.builder_clones, 0, "route {route}");
    }
}

fn checkpoint(vm: &VM) -> VmCheckpoint {
    vm.checkpoint_for_chunk(&BytecodeChunk {
        opcodes: Vec::new(),
        source_map: None,
        const_base: 0,
        constants: Vec::new(),
        new_locals: 0,
        type_registry_base: 0,
        type_entries: Vec::new(),
        error_template_base: 0,
        error_templates: Vec::new(),
        dbg_template_base: 0,
        dbg_templates: Vec::new(),
        callable_templates: Vec::new(),
        functions: Vec::new(),
        docs: Vec::new(),
        signatures: Vec::new(),
        runtime_process_specs: Vec::new(),
        runtime_boot_plan: Default::default(),
    })
}

fn suspended_process_vm(process_count: u64) -> (VM, usize) {
    reset_flat_map_metrics();
    let (mut vm, mapper) = constant_mapper_vm(list((0..512).map(|n| Value::Int(int(n)))));
    let mut context = vm
        .prepare_callable_context(
            flat_map_callable(),
            vec![list([Value::Unit]), Value::Callable(mapper)],
        )
        .unwrap();
    assert!(matches!(
        vm.run_quantum(&mut context, &mut Budget::new(32)),
        ProcessRunOutcome::QuantumExpired
    ));
    let builder_len = flat_map_metrics().builder_pushes;
    assert!(builder_len > 0);
    for pid in 0..process_count {
        vm.process_runtime.processes.insert(
            pid,
            ProcessInstance {
                pid,
                spec_id: 0,
                identity: PidHandle::new(pid, "Worker".into(), PidKind::Worker),
                acceptance: if pid % 2 == 0 {
                    ProcessAcceptance::Stopping
                } else {
                    ProcessAcceptance::Accepting
                },
                stop_reason: None,
                status: ProcessStatus::Waiting(ProcessWaitReason::Boot),
                mailbox: VecDeque::new(),
                execution_context: Some(context.clone()),
                state_value: None,
                owner: None,
                lifecycle_sink: None,
                standby_state_pending: false,
            },
        );
    }
    reset_flat_map_metrics();
    (vm, builder_len)
}

#[test]
fn unchanged_process_checkpoints_do_not_clone_suspended_builders() {
    let mut samples = Vec::new();
    for process_count in [1, 8, 32] {
        for chunk_count in [1, 4, 16] {
            let (vm, builder_len) = suspended_process_vm(process_count);
            let start = std::time::Instant::now();
            for _ in 0..chunk_count {
                drop(checkpoint(&vm));
            }
            let clones = flat_map_metrics().builder_clones;
            samples.push((
                process_count,
                chunk_count,
                builder_len,
                clones,
                clones * builder_len,
                start.elapsed().as_micros(),
            ));
        }
    }
    eprintln!("checkpoint samples (processes, checkpoints, builder_len, clones, copied_items, microseconds): {samples:?}");
    assert!(
        samples.iter().all(|(_, _, _, clones, _, _)| *clones == 0),
        "unchanged checkpoint builders must stay shared"
    );
}

#[test]
fn process_checkpoint_copies_only_mutated_entry_and_restores_future() {
    let (mut vm, _) = suspended_process_vm(3);
    let future = vm.process_runtime.allocate_future(Some(1), None, false);
    let saved = checkpoint(&vm);
    assert_eq!(flat_map_metrics().builder_clones, 0);
    vm.process_runtime
        .processes
        .get_mut(&1)
        .unwrap()
        .state_value = Some(Value::Int(int(9)));
    vm.process_runtime
        .mark_process_waiting(1, ProcessWaitReason::Future(future));
    vm.process_runtime
        .resolve_future(future, Value::Int(int(17)));
    assert_eq!(
        flat_map_metrics().builder_clones,
        1,
        "unmodified processes must not copy their builders"
    );
    assert_eq!(vm.ready_future_value(future), Some(Value::Int(int(17))));
    assert_eq!(vm.process_runtime.run_queue.front(), Some(&1));
    vm.rollback_to_checkpoint(saved);
    assert!(vm.ready_future_value(future).is_none());
    assert!(vm.process_runtime.run_queue.is_empty());
    assert_eq!(
        vm.process_runtime.processes[&1].status,
        ProcessStatus::Waiting(ProcessWaitReason::Boot)
    );
    assert_eq!(vm.process_runtime.processes[&1].state_value, None);

    let committed = checkpoint(&vm);
    vm.process_runtime
        .processes
        .get_mut(&1)
        .unwrap()
        .state_value = Some(Value::Int(int(42)));
    drop(committed);
    assert_eq!(
        vm.process_runtime.processes[&1].state_value,
        Some(Value::Int(int(42)))
    );
}

#[test]
fn process_checkpoint_stop_copies_only_owned_detached_task() {
    let expected = list((0..4096).map(|n| Value::Int(int(n))));
    let (mut vm, mapper) = constant_mapper_vm(expected.clone());
    vm.frames[0].call_site = Some((0, 1));
    let mut futures = Vec::new();
    for owner in [7, 8] {
        let mut computation = flat_map_callable();
        computation.lexical_captures = vec![list([Value::Unit]), Value::Callable(mapper.clone())];
        let Value::TaskHandle(future) = vm.invoke_task(computation, TaskMode::Async).unwrap()
        else {
            panic!("expected task handle")
        };
        futures.push(future);
        let id = *vm.process_runtime.detached_tasks.iter().last().unwrap().0;
        vm.process_runtime
            .detached_tasks
            .get_mut(&id)
            .unwrap()
            .owner_pid = Some(owner);
    }
    assert_eq!(vm.process_runtime.detached_tasks.len(), 2);
    reset_flat_map_metrics();
    let saved = checkpoint(&vm);
    vm.remove_process_detached_tasks(7);
    assert_eq!(
        flat_map_metrics().builder_clones,
        1,
        "stopping one owner must not copy another owner's suspended task"
    );
    vm.rollback_to_checkpoint(saved);
    vm.drain_background_tasks().unwrap();
    for future in futures {
        assert_eq!(vm.ready_future_value(future), Some(expected.clone()));
    }
    assert_eq!(
        flat_map_metrics().callback_calls,
        0,
        "restoring task cursors must not invoke completed mappers again"
    );
    assert_eq!(flat_map_metrics().builder_finishes, 2);
    assert_eq!(flat_map_metrics().builder_clones, 1);
}

#[test]
fn flat_map_checkpoint_restores_independent_partial_builder_and_cursor() {
    reset_flat_map_metrics();
    let expected = list((0..4096).map(|n| Value::Int(int(n))));
    let (mut vm, mapper) = constant_mapper_vm(expected.clone());
    vm.frames[0].call_site = Some((0, 1));
    let mut computation = flat_map_callable();
    computation.lexical_captures = vec![list([Value::Unit]), Value::Callable(mapper)];
    let task = vm.invoke_task(computation, TaskMode::Async).unwrap();
    let Value::TaskHandle(future) = task else {
        panic!("expected task handle")
    };
    let saved_pushes = flat_map_metrics().builder_pushes;
    assert!(saved_pushes > 0 && saved_pushes < 4096);
    assert_eq!(flat_map_metrics().callback_calls, 1);
    assert_eq!(flat_map_metrics().builder_clones, 0);
    assert!(
        vm.ready_future_value(future).is_none(),
        "incomplete builder cannot be a Task result"
    );
    let saved = checkpoint(&vm);
    assert_eq!(
        flat_map_metrics().builder_clones,
        0,
        "the checkpoint shares suspended construction state"
    );
    vm.drive_ready_runtime_quantum().unwrap();
    assert_eq!(
        flat_map_metrics().builder_clones,
        1,
        "only advancing the saved task copies its mutable construction state"
    );
    let advanced_pushes = flat_map_metrics().builder_pushes;
    assert!(advanced_pushes > saved_pushes && advanced_pushes < 4096);
    vm.rollback_to_checkpoint(saved);
    assert!(vm.ready_future_value(future).is_none());
    vm.drain_background_tasks().unwrap();
    assert_eq!(vm.ready_future_value(future), Some(expected));
    let metrics = flat_map_metrics();
    assert_eq!(
        metrics.callback_calls, 1,
        "saved output cursor must resume without invoking mapper again"
    );
    assert_eq!(
        metrics.builder_pushes,
        4096 + advanced_pushes - saved_pushes
    );
    assert_eq!(metrics.builder_finishes, 1);
    assert_eq!(metrics.builder_clones, 1);
}

#[test]
fn flat_map_rejects_invalid_runtime_arguments_before_starting_work() {
    reset_flat_map_metrics();
    let (mut vm, mapper) = constant_mapper_vm(list([]));
    let id = builtin_id_by_name("list_flat_map").unwrap();
    for (args, message) in [
        (vec![], "arity mismatch"),
        (vec![list([])], "arity mismatch"),
        (
            vec![list([]), Value::Callable(mapper.clone()), Value::Unit],
            "arity mismatch",
        ),
        (
            vec![Value::Unit, Value::Callable(mapper)],
            "expects List as values",
        ),
        (vec![list([]), Value::Unit], "expects callable value as f"),
    ] {
        let error = call_builtin(&mut vm, id, args).expect_err("invalid bytecode must fail");
        assert!(error.message.contains(message), "{error:?}");
    }
    assert_eq!(flat_map_metrics(), Default::default());
}

// This crate-local probe measures the real detached-task driver. Its timers include
// scheduler work and Value copies/growth during 256 output transitions per task.
// Completion is a separate phase; whole-output scans run only in smoke or
// PROBE_VERIFY mode, outside every measured interval.
fn builder_probe_payload(index: usize, payload: &str) -> Value {
    match payload {
        "int" => Value::Int(int(index)),
        "string" => Value::Str(format!("{index:020}{}", "x".repeat(236))),
        other => panic!("unknown SURTR_BUILDER_PAYLOAD: {other}"),
    }
}

fn builder_probe_vm(
    task_count: usize,
    builder_len: usize,
    payload: &str,
) -> (VM, Vec<FutureId>, usize) {
    // Three timed quanta must all stop inside one completed mapper's output.
    let output_len = builder_len.checked_add(2048).unwrap();
    let output = list((0..output_len).map(|n| builder_probe_payload(n, payload)));
    let (mut vm, mapper) = constant_mapper_vm(output);
    vm.frames[0].call_site = Some((0, 1));
    let mut futures = Vec::with_capacity(task_count);
    for _ in 0..task_count {
        let mut computation = flat_map_callable();
        computation.lexical_captures = vec![list([Value::Unit]), Value::Callable(mapper.clone())];
        let id = vm.process_runtime.next_detached_task_id;
        // start_task registers a genuine scheduler task without invoke_task's
        // automatic startup quantum, so task count and saved length are independent.
        vm.start_task(computation, TaskMode::Async, None).unwrap();
        let mut task = vm.process_runtime.detached_tasks.take(&id).unwrap();
        let DetachedCompletion::Future(Some(future)) = task.completion else {
            panic!("async task must have a completion Future")
        };
        futures.push(future);
        let DetachedTaskState::Runnable(context) = &mut task.state else {
            panic!("new task must be runnable")
        };
        let caller = vm.take_execution_context(vm.pc, ExecutionTarget::TopLevel);
        reset_flat_map_metrics();
        for _ in 0..builder_len + 32 {
            if flat_map_metrics().builder_pushes == builder_len {
                break;
            }
            assert!(matches!(
                vm.run_quantum(context, &mut Budget::new(1)),
                ProcessRunOutcome::QuantumExpired
            ));
        }
        let prepared = flat_map_metrics();
        assert_eq!(prepared.builder_pushes, builder_len);
        assert_eq!(prepared.callback_calls, 1);
        assert_eq!(prepared.builder_finishes, 0);
        assert_eq!(prepared.builder_clones, 0);
        vm.restore_execution_context(caller);
        task.test_mark_started();
        vm.process_runtime.detached_tasks.insert(id, task);
        assert!(vm.ready_future_value(future).is_none());
    }
    assert_eq!(vm.process_runtime.detached_tasks.len(), task_count);
    reset_flat_map_metrics();
    (vm, futures, output_len)
}

#[derive(Debug)]
struct BuilderProbeSample {
    phase: &'static str,
    repeat: usize,
    elapsed_ns: u64,
    position_before: usize,
    position_after: usize,
    metrics: crate::builtin::FlatMapMetrics,
}

fn builder_probe_phase<R>(
    timed: bool,
    phase: &'static str,
    repeat: usize,
    position: usize,
    action: impl FnOnce() -> R,
) -> (R, BuilderProbeSample) {
    reset_flat_map_metrics();
    let start = timed.then(std::time::Instant::now);
    let result = action();
    let elapsed_ns = start.map_or(0, |start| start.elapsed().as_nanos() as u64);
    let metrics = flat_map_metrics();
    (
        result,
        BuilderProbeSample {
            phase,
            repeat,
            elapsed_ns,
            position_before: position,
            position_after: position,
            metrics,
        },
    )
}

fn builder_probe_sample_assertions(
    sample: &BuilderProbeSample,
    copies: usize,
    copied_items: usize,
) {
    assert_eq!(sample.metrics.builder_clones, copies, "{sample:?}");
    assert_eq!(
        sample.metrics.builder_copied_items, copied_items,
        "{sample:?}"
    );
    assert_eq!(sample.metrics.callback_calls, 0, "{sample:?}");
    assert_eq!(sample.metrics.builder_finishes, 0, "{sample:?}");
    assert_eq!(sample.metrics.builder_finish_steps, 0, "{sample:?}");
    assert_eq!(sample.metrics.builder_finish_copied_items, 0, "{sample:?}");
}

fn builder_probe_repeat(
    task_count: usize,
    builder_len: usize,
    payload: &str,
    repeat: usize,
    verify: bool,
    copy_mode: &str,
) -> (usize, Vec<BuilderProbeSample>) {
    let (mut vm, futures, output_len) = builder_probe_vm(task_count, builder_len, payload);
    let timed = !verify;
    let mut samples = Vec::with_capacity(10);
    let (original, save) =
        builder_probe_phase(timed, "save", repeat, builder_len, || checkpoint(&vm));
    builder_probe_sample_assertions(&save, 0, 0);
    samples.push(save);

    let unchanged = checkpoint(&vm);
    let (_, drop_unchanged) =
        builder_probe_phase(timed, "drop_unchanged", repeat, builder_len, || {
            drop(unchanged)
        });
    builder_probe_sample_assertions(&drop_unchanged, 0, 0);
    samples.push(drop_unchanged);

    let mut position = builder_len;
    for (phase, copies) in [("first_resume", task_count), ("second_resume", 0)] {
        let (result, mut sample) = builder_probe_phase(timed, phase, repeat, position, || {
            vm.drive_ready_runtime_quantum()
        });
        result.unwrap();
        let copied_items = if copy_mode == "baseline" {
            copies * position
        } else {
            0
        };
        builder_probe_sample_assertions(&sample, copies, copied_items);
        assert_eq!(
            sample.metrics.builder_pushes,
            task_count * 256,
            "{sample:?}"
        );
        position += sample.metrics.builder_pushes / task_count;
        sample.position_after = position;
        samples.push(sample);
    }

    let restored_position = position;
    let (advanced, resave) =
        builder_probe_phase(timed, "resave", repeat, position, || checkpoint(&vm));
    builder_probe_sample_assertions(&resave, 0, 0);
    samples.push(resave);
    let (result, mut resume) =
        builder_probe_phase(timed, "resume_after_resave", repeat, position, || {
            vm.drive_ready_runtime_quantum()
        });
    result.unwrap();
    let copied_items = if copy_mode == "baseline" {
        task_count * position
    } else {
        0
    };
    builder_probe_sample_assertions(&resume, task_count, copied_items);
    assert_eq!(
        resume.metrics.builder_pushes,
        task_count * 256,
        "{resume:?}"
    );
    position += resume.metrics.builder_pushes / task_count;
    resume.position_after = position;
    samples.push(resume);
    assert_eq!(vm.process_runtime.detached_tasks.len(), task_count);
    for &future in &futures {
        assert!(vm.ready_future_value(future).is_none());
    }

    let (_, mut rollback) = builder_probe_phase(timed, "rollback", repeat, position, || {
        vm.rollback_to_checkpoint(advanced)
    });
    rollback.position_after = restored_position;
    builder_probe_sample_assertions(&rollback, 0, 0);
    samples.push(rollback);
    // Keep the original save alive through completion. The shared representation
    // only clones its saved prefix Values when materializing the final List.
    let (result, mut complete) =
        builder_probe_phase(timed, "complete_shared", repeat, restored_position, || {
            vm.drain_background_tasks()
        });
    result.unwrap();
    complete.position_after = output_len;
    let finish_copied_items = if copy_mode == "shared" {
        task_count * builder_len
    } else {
        0
    };
    builder_probe_completion_assertions(
        &complete,
        task_count,
        task_count * (output_len - restored_position),
        finish_copied_items,
    );
    if verify {
        builder_probe_verify_results(&vm, &futures, output_len, payload);
    }
    samples.push(complete);

    let (_, drop_changed) =
        builder_probe_phase(timed, "drop_changed", repeat, output_len, || drop(original));
    builder_probe_sample_assertions(&drop_changed, 0, 0);
    samples.push(drop_changed);

    drop(vm);
    drop(futures);
    let (mut unique, unique_futures, unique_output_len) =
        builder_probe_vm(task_count, builder_len, payload);
    assert_eq!(unique_output_len, output_len);
    let (result, mut complete) =
        builder_probe_phase(timed, "unique_complete", repeat, builder_len, || {
            unique.drain_background_tasks()
        });
    result.unwrap();
    complete.position_after = output_len;
    builder_probe_completion_assertions(
        &complete,
        task_count,
        task_count * (output_len - builder_len),
        0,
    );
    if verify {
        builder_probe_verify_results(&unique, &unique_futures, output_len, payload);
    }
    samples.push(complete);
    (output_len, samples)
}

fn builder_probe_completion_assertions(
    sample: &BuilderProbeSample,
    task_count: usize,
    pushes: usize,
    finish_copied_items: usize,
) {
    assert_eq!(sample.metrics.builder_clones, 0, "{sample:?}");
    assert_eq!(sample.metrics.builder_copied_items, 0, "{sample:?}");
    assert_eq!(
        sample.metrics.callback_calls, 0,
        "completion must not rerun mappers: {sample:?}"
    );
    assert_eq!(sample.metrics.builder_finishes, task_count, "{sample:?}");
    assert_eq!(sample.metrics.builder_pushes, pushes, "{sample:?}");
    assert_eq!(
        sample.metrics.builder_finish_copied_items, finish_copied_items,
        "{sample:?}"
    );
}

fn builder_probe_verify_results(vm: &VM, futures: &[FutureId], output_len: usize, payload: &str) {
    assert!(vm.process_runtime.detached_tasks.is_empty());
    for &future in futures {
        let Some(Value::List(mut output)) = vm.ready_future_value(future) else {
            panic!("completed task must deliver its List Future")
        };
        assert_eq!(output.len(), output_len);
        for index in 0..output_len {
            assert_eq!(
                output.head_value(),
                Some(builder_probe_payload(index, payload))
            );
            output = output.tail_handle().unwrap();
        }
        assert!(output.head_value().is_none());
    }
}

#[test]
fn checkpoint_builder_profile() {
    fn positive_env(name: &str, default: usize) -> usize {
        match std::env::var(name) {
            Ok(value) => {
                let parsed: usize = value.parse().unwrap_or_else(|_| panic!("invalid {name}"));
                assert!(parsed > 0, "{name} must be positive");
                parsed
            }
            Err(std::env::VarError::NotPresent) => default,
            Err(error) => panic!("invalid {name}: {error}"),
        }
    }
    let profile = std::env::var("SURTR_BUILDER_PROFILE").as_deref() == Ok("1");
    if !profile {
        builder_probe_repeat(2, 32, "int", 0, true, "shared");
        return;
    }
    let task_count = positive_env("SURTR_BUILDER_TASKS", 1);
    let builder_len = positive_env("SURTR_BUILDER_LEN", 32);
    let repeats = positive_env("SURTR_BUILDER_REPEATS", 5);
    let payload = std::env::var("SURTR_BUILDER_PAYLOAD").unwrap_or_else(|_| "int".into());
    assert!(matches!(payload.as_str(), "int" | "string"));
    let verify = std::env::var("PROBE_VERIFY").as_deref() == Ok("1");
    let copy_mode = std::env::var("SURTR_BUILDER_COPY_MODE").unwrap_or_else(|_| "shared".into());
    assert!(
        matches!(copy_mode.as_str(), "baseline" | "shared"),
        "unknown SURTR_BUILDER_COPY_MODE: {copy_mode}"
    );
    let mut samples = Vec::with_capacity(repeats * 10);
    let mut output_len = 0;
    for repeat in 0..repeats {
        let (len, current) = builder_probe_repeat(
            task_count,
            builder_len,
            &payload,
            repeat,
            verify,
            &copy_mode,
        );
        output_len = len;
        samples.extend(current);
    }
    // Formatting and emission happen after every timed interval and repeat.
    if verify {
        eprintln!(
            "{}",
            serde_json::json!({
                "profile": "checkpoint_builder_verify", "tasks": task_count,
                "builder_len": builder_len, "payload": payload, "repeats": repeats,
                "copy_mode": copy_mode,
                "verified": true,
            })
        );
    } else {
        for sample in samples {
            eprintln!(
                "{}",
                serde_json::json!({
                    "profile": "checkpoint_builder", "phase": sample.phase, "repeat": sample.repeat,
                    "tasks": task_count, "builder_len": builder_len, "payload": payload,
                    "copy_mode": copy_mode,
                    "output_len": output_len, "quantum": 256, "elapsed_ns": sample.elapsed_ns,
                    "copies": sample.metrics.builder_clones,
                    "copied_items": sample.metrics.builder_copied_items,
                    "pushes": sample.metrics.builder_pushes,
                    "callback_calls": sample.metrics.callback_calls,
                    "finishes": sample.metrics.builder_finishes,
                    "capacity_growths": sample.metrics.builder_capacity_growths,
                    "finish_steps": sample.metrics.builder_finish_steps,
                    "finish_copied_items": sample.metrics.builder_finish_copied_items,
                    "position_before": sample.position_before, "position_after": sample.position_after,
                })
            );
        }
    }
}

fn shared_builder_finishing_vm() -> (VM, FutureId, VmCheckpoint, usize) {
    let (mut vm, futures, output_len) = builder_probe_vm(1, 1024, "int");
    let original = checkpoint(&vm);
    for _ in 0..32 {
        vm.drive_ready_runtime_quantum().unwrap();
        if flat_map_metrics().builder_finish_steps > 0
            || vm.ready_future_value(futures[0]).is_some()
        {
            break;
        }
    }
    assert!(flat_map_metrics().builder_finish_steps > 0);
    assert!(vm.ready_future_value(futures[0]).is_none());
    (vm, futures[0], original, output_len)
}

#[test]
fn flat_map_checkpoint_during_shared_finish_restores_cursor_without_recopies() {
    let (mut vm, future, original, output_len) = shared_builder_finishing_vm();
    reset_flat_map_metrics();
    let saved = checkpoint(&vm);
    assert_eq!(flat_map_metrics().builder_copied_items, 0);
    vm.drive_ready_runtime_quantum().unwrap();
    assert_eq!(flat_map_metrics().builder_copied_items, 0);
    assert_eq!(flat_map_metrics().builder_finish_steps, 256);
    assert!(vm.ready_future_value(future).is_none());
    vm.rollback_to_checkpoint(saved);
    reset_flat_map_metrics();
    vm.drain_background_tasks().unwrap();
    assert_eq!(flat_map_metrics().callback_calls, 0);
    assert_eq!(flat_map_metrics().builder_copied_items, 0);
    assert_eq!(flat_map_metrics().builder_finishes, 1);
    assert!(vm.process_runtime.detached_tasks.is_empty());
    assert_eq!(
        vm.ready_future_value(future),
        Some(list((0..output_len).map(|index| Value::Int(int(index)))))
    );
    drop(original);
}

#[test]
fn flat_map_cancellation_during_shared_finish_discards_partial_result() {
    let (mut vm, future, original, _) = shared_builder_finishing_vm();
    let id = *vm.process_runtime.detached_tasks.iter().next().unwrap().0;
    let mut task = vm.process_runtime.detached_tasks.take(&id).unwrap();
    let DetachedTaskState::Runnable(context) = &mut task.state else {
        panic!("finish must be runnable");
    };
    reset_flat_map_metrics();
    VM::cancel_execution_context(context, RuntimeError::new("cancelled finish"));
    assert_eq!(fail(&mut vm, context, 1).message, "cancelled finish");
    assert_eq!(flat_map_metrics().builder_finishes, 0);
    assert_eq!(flat_map_metrics().builder_finish_steps, 0);
    assert!(vm.ready_future_value(future).is_none());
    drop(original);
}

#[test]
fn flat_map_shared_finish_keeps_zero_and_one_reduction_boundaries() {
    for quantum in [1, 7, 256] {
        let (mut vm, future, original, output_len) = shared_builder_finishing_vm();
        let id = *vm.process_runtime.detached_tasks.iter().next().unwrap().0;
        let mut task = vm.process_runtime.detached_tasks.take(&id).unwrap();
        let DetachedTaskState::Runnable(context) = &mut task.state else {
            panic!("finish must be runnable");
        };
        let before = format!("{context:?}");
        let mut zero = Budget::new(0);
        assert!(matches!(
            vm.run_quantum(context, &mut zero),
            ProcessRunOutcome::QuantumExpired
        ));
        assert_eq!(format!("{context:?}"), before);
        reset_flat_map_metrics();
        let mut one = Budget::new(1);
        assert!(matches!(
            vm.run_quantum(context, &mut one),
            ProcessRunOutcome::QuantumExpired
        ));
        assert_eq!(one.reductions(), 1);
        assert_eq!(flat_map_metrics().builder_finish_steps, 1);
        assert!(vm.ready_future_value(future).is_none());
        assert_eq!(vm.pc, context.pc);
        assert_eq!(
            finish(&mut vm, context, quantum),
            list((0..output_len).map(|index| Value::Int(int(index))))
        );
        assert_eq!(flat_map_metrics().callback_calls, 0);
        assert_eq!(flat_map_metrics().builder_finishes, 1);
        drop(original);
    }
}

#[test]
fn flat_map_finish_completion_at_budget_boundary_defers_delivery() {
    for quantum in [2, 7] {
        let (mut vm, future, original, output_len) = shared_builder_finishing_vm();
        let id = *vm.process_runtime.detached_tasks.iter().next().unwrap().0;
        let mut task = vm.process_runtime.detached_tasks.take(&id).unwrap();
        let DetachedTaskState::Runnable(context) = &mut task.state else {
            panic!("finish must be runnable");
        };
        // Leave one prefix item and the final boundary for the next quantum.
        while flat_map_metrics().builder_finish_copied_items < 1023 {
            assert!(matches!(
                vm.run_quantum(context, &mut Budget::new(1)),
                ProcessRunOutcome::QuantumExpired
            ));
        }
        let mut budget = Budget::new(quantum);
        let outcome = vm.run_quantum(context, &mut budget);
        assert_eq!(flat_map_metrics().builder_finishes, 1);
        assert!(vm.ready_future_value(future).is_none());
        let expected = list((0..output_len).map(|index| Value::Int(int(index))));
        if quantum == 2 {
            assert!(matches!(outcome, ProcessRunOutcome::QuantumExpired));
            assert_eq!(budget.reductions(), 2);
            assert!(matches!(
                context.pending_invocation,
                Some(Ok(continuation::Invocation::Builtin(
                    BuiltinOutcome::Complete(Value::List(_))
                )))
            ));
            assert_eq!(finish(&mut vm, context, 1), expected);
        } else {
            let ProcessRunOutcome::Halted(value) = outcome else {
                panic!("finish must use its remaining budget for delivery: {outcome:?}");
            };
            assert_eq!(value, expected);
            assert!(budget.reductions() > 2 && budget.reductions() < quantum);
        }
        drop(original);
    }
}

fn short_builder_start_vm(tasks: usize, output_len: usize, payload: &str) -> (VM, Vec<FutureId>) {
    let output = list((0..output_len).map(|index| builder_probe_payload(index, payload)));
    let (mut vm, mapper) = constant_mapper_vm(output);
    vm.frames[0].call_site = Some((0, 1));
    let mut futures = Vec::with_capacity(tasks);
    for _ in 0..tasks {
        let mut computation = flat_map_callable();
        computation.lexical_captures = vec![list([Value::Unit]), Value::Callable(mapper.clone())];
        let id = vm.process_runtime.next_detached_task_id;
        vm.start_task(computation, TaskMode::Async, None).unwrap();
        let DetachedCompletion::Future(Some(future)) =
            vm.process_runtime.detached_tasks[&id].completion
        else {
            panic!("short async task must have its completion Future")
        };
        futures.push(future);
    }
    (vm, futures)
}

fn short_builder_prepare_snapshot(vm: &mut VM, tasks: usize, output_len: usize) {
    let midpoint = output_len / 2;
    let ids: Vec<_> = vm
        .process_runtime
        .detached_tasks
        .iter()
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(ids.len(), tasks);
    for id in ids {
        let mut task = vm.process_runtime.detached_tasks.take(&id).unwrap();
        let DetachedTaskState::Runnable(context) = &mut task.state else {
            panic!("short Task must be runnable before its preparation")
        };
        let caller = vm.take_execution_context(vm.pc, ExecutionTarget::TopLevel);
        reset_flat_map_metrics();
        for _ in 0..midpoint + 32 {
            let metrics = flat_map_metrics();
            // In particular n=0/1 stop after the callback result handoff and
            // before the first append/finish, with an empty runnable Builder.
            let output_ready = matches!(
                &context.pending_invocation,
                Some(Ok(continuation::Invocation::Builtin(
                    BuiltinOutcome::Resume(BuiltinContinuation::FlatMap(_))
                )))
            );
            if metrics.builder_pushes == midpoint && metrics.callback_calls == 1 && output_ready {
                break;
            }
            let outcome = vm.run_quantum(context, &mut Budget::new(1));
            assert!(
                matches!(outcome, ProcessRunOutcome::QuantumExpired),
                "{outcome:?}"
            );
        }
        let metrics = flat_map_metrics();
        assert_eq!(metrics.builder_pushes, midpoint);
        assert_eq!(metrics.callback_calls, 1);
        assert_eq!(metrics.builder_finishes, 0);
        assert!(matches!(
            &context.pending_invocation,
            Some(Ok(continuation::Invocation::Builtin(
                BuiltinOutcome::Resume(BuiltinContinuation::FlatMap(_))
            )))
        ));
        vm.restore_execution_context(caller);
        task.test_mark_started();
        vm.process_runtime.detached_tasks.insert(id, task);
    }
}

#[derive(Debug)]
struct ShortBuilderSample {
    repeat: usize,
    elapsed_ns: u64,
    metrics: crate::builtin::FlatMapMetrics,
}

fn short_builder_sample(
    tasks: usize,
    output_len: usize,
    payload: &str,
    workload: &str,
    repeat: usize,
    verify: bool,
) -> ShortBuilderSample {
    let (vm, futures, elapsed_ns, metrics) = match workload {
        "construct_complete" => {
            reset_flat_map_metrics();
            let started = (!verify).then(std::time::Instant::now);
            let (mut vm, futures) = short_builder_start_vm(tasks, output_len, payload);
            let result = vm.drain_background_tasks();
            let elapsed_ns = started.map_or(0, |start| start.elapsed().as_nanos() as u64);
            let metrics = flat_map_metrics();
            result.unwrap();
            (vm, futures, elapsed_ns, metrics)
        }
        "snapshot_resume" => {
            let (mut vm, futures) = short_builder_start_vm(tasks, output_len, payload);
            short_builder_prepare_snapshot(&mut vm, tasks, output_len);
            reset_flat_map_metrics();
            let started = (!verify).then(std::time::Instant::now);
            let saved = checkpoint(&vm);
            let result = vm.drain_background_tasks();
            drop(saved);
            let elapsed_ns = started.map_or(0, |start| start.elapsed().as_nanos() as u64);
            let metrics = flat_map_metrics();
            result.unwrap();
            (vm, futures, elapsed_ns, metrics)
        }
        _ => panic!("invalid short builder workload: {workload}"),
    };
    assert!(vm.process_runtime.detached_tasks.is_empty());
    assert_eq!(metrics.builder_finishes, tasks);
    assert_eq!(metrics.builder_copied_items, 0);
    match workload {
        "construct_complete" => {
            assert_eq!(metrics.callback_calls, tasks);
            assert_eq!(metrics.input_visits, tasks);
            assert_eq!(metrics.builder_pushes, tasks * output_len);
            assert_eq!(metrics.builder_clones, 0);
        }
        "snapshot_resume" => {
            assert_eq!(
                metrics.callback_calls, 0,
                "saved mappers must not be reevaluated"
            );
            assert_eq!(metrics.input_visits, 0);
            assert_eq!(
                metrics.builder_pushes,
                tasks * (output_len - output_len / 2)
            );
            assert_eq!(metrics.builder_clones, tasks);
        }
        _ => unreachable!(),
    }
    if verify {
        builder_probe_verify_results(&vm, &futures, output_len, payload);
    } else {
        for future in futures {
            let Some(Value::List(output)) = vm.ready_future_value(future) else {
                panic!("completed short Task must deliver its List")
            };
            assert_eq!(output.len(), output_len);
        }
    }
    // VM destruction is outside the two requested intervals.
    ShortBuilderSample {
        repeat,
        elapsed_ns,
        metrics,
    }
}

#[test]
fn true_short_builder_profile() {
    fn positive_env(name: &str, default: usize) -> usize {
        let value = match std::env::var(name) {
            Ok(value) => value
                .parse::<usize>()
                .unwrap_or_else(|_| panic!("invalid {name}")),
            Err(std::env::VarError::NotPresent) => default,
            Err(error) => panic!("invalid {name}: {error}"),
        };
        assert!(value > 0, "{name} must be positive");
        value
    }
    let profile = std::env::var("SURTR_SHORT_PROFILE").as_deref() == Ok("1");
    if !profile {
        for workload in ["construct_complete", "snapshot_resume"] {
            for len in [0, 1, 8] {
                short_builder_sample(1, len, "int", workload, 0, true);
            }
        }
        return;
    }
    let output_len = match std::env::var("SURTR_SHORT_LEN") {
        Ok(value) => value.parse::<usize>().expect("invalid SURTR_SHORT_LEN"),
        Err(std::env::VarError::NotPresent) => 8,
        Err(error) => panic!("invalid SURTR_SHORT_LEN: {error}"),
    };
    let tasks = positive_env("SURTR_SHORT_TASKS", 1);
    let repeats = positive_env("SURTR_SHORT_REPEATS", 8);
    let payload = std::env::var("SURTR_SHORT_PAYLOAD").unwrap_or_else(|_| "int".into());
    let workload =
        std::env::var("SURTR_SHORT_WORKLOAD").unwrap_or_else(|_| "construct_complete".into());
    assert!(matches!(payload.as_str(), "int" | "string"));
    assert!(matches!(
        workload.as_str(),
        "construct_complete" | "snapshot_resume"
    ));
    let verify = std::env::var("PROBE_VERIFY").as_deref() == Ok("1");
    let mut samples = Vec::with_capacity(repeats);
    for repeat in 0..repeats {
        samples.push(short_builder_sample(
            tasks, output_len, &payload, &workload, repeat, verify,
        ));
    }
    if verify {
        eprintln!(
            "{}",
            serde_json::json!({
                "profile": "checkpoint_builder_short_verify", "workload": workload,
                "tasks": tasks, "output_len": output_len, "payload": payload,
                "repeats": repeats, "verified": true,
            })
        );
    } else {
        for sample in samples {
            eprintln!(
                "{}",
                serde_json::json!({
                    "profile": "checkpoint_builder_short", "workload": workload,
                    "tasks": tasks, "output_len": output_len, "payload": payload,
                    "saved_len": if workload == "snapshot_resume" { Some(output_len / 2) } else { None },
                    "repeat": sample.repeat, "repeats": repeats, "elapsed_ns": sample.elapsed_ns,
                    "callback_calls": sample.metrics.callback_calls, "input_visits": sample.metrics.input_visits,
                    "pushes": sample.metrics.builder_pushes, "finishes": sample.metrics.builder_finishes,
                    "copies": sample.metrics.builder_clones, "copied_items": sample.metrics.builder_copied_items,
                    "finish_steps": sample.metrics.builder_finish_steps,
                    "finish_copied_items": sample.metrics.builder_finish_copied_items,
                })
            );
        }
    }
}
