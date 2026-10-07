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
                vm.stack = vec![input, Value::Callable(mapper)];
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
                status: if pid % 2 == 0 {
                    ProcessStatus::Stopped
                } else {
                    ProcessStatus::Waiting(ProcessWaitReason::Boot)
                },
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
