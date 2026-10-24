use scar::typed::TypedInner;

#[path = "support/core.rs"]
mod support;
use support::{resolve_with_builtin_prelude, typecheck};

#[test]
fn long_forward_type_dependency_chain_fits_cli_stack() {
    const CHILD: &str = "SURTR_TYPE_DEPENDENCY_STACK_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "long_forward_type_dependency_chain_fits_cli_stack",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .expect("type dependency subprocess should start");
        assert!(
            output.status.success(),
            "type dependency subprocess failed: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    std::thread::Builder::new()
        .name("scar-long-type-dependencies".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            const COUNT: usize = 40_000;
            let mut source = String::new();
            for index in 0..COUNT - 1 {
                source.push_str(&format!("defrecord T{index:x}(n:T{:x})\n", index + 1));
            }
            source.push_str(&format!("defrecord T{:x}(n:Int)\n", COUNT - 1));
            source.push_str("def read(value: T0) -> T2 { value.n.n }\n");
            assert!(
                source.len() < 1_000_000,
                "probe must fit the CLI source span limit"
            );
            let resolved = resolve_with_builtin_prelude(&source);
            let typed =
                typecheck(resolved).expect("forward nominal dependency chain should typecheck");
            assert_eq!(
                typed
                    .iter()
                    .filter(|node| matches!(node.node, TypedInner::RecordDef(..)))
                    .count(),
                COUNT
            );
        })
        .expect("compiler regression thread should start")
        .join()
        .expect("type dependency checking should fit the CLI stack");
}

#[test]
fn shared_diamond_dependencies_and_a_tail_cycle_are_distinguished() {
    const COUNT: usize = 64;
    let mut source = String::new();
    for index in 0..COUNT {
        source.push_str(&format!(
            "defrecord N{index}(left: L{index}, right: R{index})\n"
        ));
        source.push_str(&format!("defrecord L{index}(next: N{})\n", index + 1));
        source.push_str(&format!("defrecord R{index}(next: N{})\n", index + 1));
    }
    let acyclic = format!("{source}defrecord N{COUNT}(value: Int)\n");
    let typed = typecheck(resolve_with_builtin_prelude(&acyclic))
        .expect("diamond branches sharing a terminal are not a cycle");
    assert_eq!(
        typed
            .iter()
            .filter(|node| matches!(node.node, TypedInner::RecordDef(..)))
            .count(),
        COUNT * 3 + 1
    );

    // Only the final diamond cycles; the long acyclic prefix is not part of it.
    let cyclic = format!("{source}defrecord N{COUNT}(next: N{})\n", COUNT - 1);
    let resolved = resolve_with_builtin_prelude(&cyclic);
    let spans: std::collections::HashMap<_, _> = resolved
        .iter()
        .filter_map(|node| {
            if let sigil::resolved::Resolved::RecordDef(_, id, ..) = node {
                Some((id.name.clone(), id.span.clone()))
            } else {
                None
            }
        })
        .collect();
    let error = typecheck(resolved).expect_err("a back edge at the tail must reject the graph");
    let cycle: Vec<_> = error
        .message
        .strip_prefix("Cyclic type definition detected: ")
        .expect("the nominal cycle diagnostic must be preserved")
        .split(" -> ")
        .collect();
    assert_eq!(cycle.first(), cycle.last());
    assert_eq!(
        cycle.len(),
        4,
        "only one terminal diamond branch forms the reported cycle"
    );
    for name in &cycle {
        assert!(
            [
                format!("Global::N{}", COUNT - 1),
                format!("Global::L{}", COUNT - 1),
                format!("Global::R{}", COUNT - 1),
                format!("Global::N{COUNT}")
            ]
            .iter()
            .any(|allowed| allowed == name),
            "prefix node incorrectly reported in cycle: {cycle:?}"
        );
    }
    assert_eq!(error.span, spans[cycle[0]]);
}

#[test]
fn forward_nominal_fields_use_the_completed_schema_in_both_orders() {
    for declarations in [
        "defrecord A(b: B)\ndefrecord B(value: Int)",
        "defrecord B(value: Int)\ndefrecord A(b: B)",
    ] {
        let source = format!("{declarations}\ndef read(a: A) -> Int {{ a.b.value }}\n");
        typecheck(resolve_with_builtin_prelude(&source))
            .expect("forward field signatures must share the canonical nominal schema");
    }
}

#[test]
fn three_nominal_schemas_allow_finite_field_and_facet_projections_in_all_orders() {
    let definitions = [
        "defrecord First(next: Second)",
        "defrecord Second(next: Third)",
        "defrecord Third(value: Int)",
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let source = format!(
            "{}\n{}\n{}\ndef read(value: First) -> Int {{ value.next.next.value }}\nvalue = First(Second(Third(42)))\nFacet::view(First.next.next.value, value)",
            definitions[order[0]], definitions[order[1]], definitions[order[2]],
        );
        let typed = typecheck(resolve_with_builtin_prelude(&source))
            .expect("finite projections must resolve the same completed schema in all six orders");
        assert_eq!(typed.last().unwrap().ty, scar::types::Ty::Int);
    }
}

#[test]
fn nominal_field_constraints_use_later_same_stage_impls_and_reject_missing_impls() {
    let constrained = r#"
defstruct Checked<$T> where $T: Marker { value: $T }
impl Checked { def new(value: $T) -> Checked<$T> where $T: Marker { Checked { value: value } } }
defrecord Outer(value: Checked<Int>)
"#;
    for (before, after) in [
        ("deftrait Marker {}\nimpl Marker for Int {}", ""),
        ("", "deftrait Marker {}\nimpl Marker for Int {}"),
        ("deftrait Marker {}", ""),
    ] {
        let source = format!("{before}\n{constrained}\n{after}");
        let checked = typecheck(resolve_with_builtin_prelude(&source));
        if before.contains("impl") || after.contains("impl") {
            checked.expect("a later same-stage impl must satisfy a concrete field type argument");
        } else {
            let error = checked.expect_err("the deferred field proof must not be discarded");
            assert_eq!(
                error.reason(),
                Some(diagnostics::TypeDiagnosticReason::NominalDeclarationConstraintViolation),
                "{error:?}"
            );
        }
    }
}

#[test]
fn nominal_constructor_trait_header_is_available_before_its_declaration() {
    let declaration = r#"
defstruct Layer<$M, $A> where $M: Context { value: $M<$A> }
impl Layer {
    def new(value: $M<$A>) -> Layer<$M, $A> where $M: Context { Layer { value: value } }
}
defenum Local<$A> { Local($A) }
impl Context for Local<$T> where $T: Context.$A {}
"#;
    for source in [
        format!("deftrait Context where Self: Type<$A> {{}}\n{declaration}"),
        format!("{declaration}\ndeftrait Context where Self: Type<$A> {{}}"),
    ] {
        typecheck(resolve_with_builtin_prelude(&source)).expect(
            "constructor header and inherited slots must be independent of declaration order",
        );
    }
}

#[test]
fn nominal_pid_marker_field_positions_are_independent_of_schema_order() {
    let carrier = r#"defstruct Carrier<$P> { value: PID<$P> }
impl Carrier { def new(value: PID<$P>) -> Carrier<$P> { Carrier { value: value } } }"#;
    for source in [
        format!("{carrier}\ndefrecord Outer(value: Carrier<OutHandler>)"),
        format!("defrecord Outer(value: Carrier<OutHandler>)\n{carrier}"),
    ] {
        typecheck(resolve_with_builtin_prelude(&source))
            .expect("forward nominal arguments must use the same PID marker positions");
    }
}

#[test]
fn nominal_pid_marker_roles_propagate_through_aliases_enums_and_reordered_parameters() {
    let declarations = [
        "type Callback<$P> = (PID<$P> -> Unit)\ndefstruct Carrier<$P> { watch: Callback<$P> }\nimpl Carrier { def new(watch: Callback<$P>) -> Carrier<$P> { Carrier { watch: watch } } }",
        "defenum Carrier<$P> { Empty, Present(Workers<$P>) }",
        "defenum Carrier<$P> { Empty, Present(WorkerLease<$P>) }",
    ];
    for carrier in declarations {
        let shell = "defenum RoleLayer<$A, $P> { Empty, Present(Carrier<$P>, $A) }";
        for source in [
            format!("{carrier}\n{shell}\ndefrecord Outer(value: RoleLayer<Int, OutHandler>)"),
            format!("defrecord Outer(value: RoleLayer<Int, OutHandler>)\n{shell}\n{carrier}"),
        ] {
            typecheck(resolve_with_builtin_prelude(&source)).expect(
                "marker roles must propagate through finite declaration parameter dependencies",
            );
        }
    }
}

#[test]
fn nominal_task_handle_parameter_retains_its_ordinary_value_role() {
    for source in [
        "defenum Carrier<$T> { Empty, Present(TaskHandle<$T>) }\ndefrecord Outer(value: Carrier<Int>)",
        "defrecord Outer(value: Carrier<Int>)\ndefenum Carrier<$T> { Empty, Present(TaskHandle<$T>) }",
    ] {
        typecheck(resolve_with_builtin_prelude(source))
            .expect("TaskHandle<T> must keep its ordinary value parameter");
    }
}

#[test]
fn nominal_schema_constructor_bounds_use_all_same_stage_impl_heads() {
    for (head, implementation, argument, needs_captured_input) in [
        ("defenum ConstructorLocal<$A> { Item($A) }", "impl ConstructorContext for ConstructorLocal<$T> where $T: ConstructorContext.$A {}", "ConstructorLocal", false),
        ("defenum ConstructorPair<$Tag, $A> { Item($Tag, $A) }", "impl ConstructorContext for ConstructorPair<String, $T> where $T: ConstructorContext.$A {}", "ConstructorPair", true),
    ] {
        let context = "deftrait ConstructorContext where Self: Type<$A> {}";
        let layer = "defenum ConstructorLayer<$M, $A> where $M: ConstructorContext { Item($M<$A>) }";
        let outer = format!("defrecord ConstructorOuter(value: ConstructorLayer<{argument}, Int>)");
        for source in [
            format!("{context}\n{head}\n{implementation}\n{layer}\n{outer}"),
            format!("{outer}\n{layer}\n{head}\n{implementation}\n{context}"),
        ] {
            let checked = typecheck(resolve_with_builtin_prelude(&source));
            if needs_captured_input {
                let error = checked.expect_err("implementation patterns must not supply a missing captured type argument");
                assert_eq!(error.reason(), Some(diagnostics::TypeDiagnosticReason::NominalDeclarationConstraintViolation), "{error:?}");
            } else {
                checked.expect("schema constructor bounds must use all impl heads in the same stage");
            }
        }
        for source in [
            format!("{context}\n{head}\n{layer}\n{outer}"),
            format!("{outer}\n{layer}\n{head}\n{context}"),
        ] {
            let error = typecheck(resolve_with_builtin_prelude(&source)).expect_err("a missing constructor proof must reject the schema in both orders");
            assert_eq!(error.reason(), Some(diagnostics::TypeDiagnosticReason::NominalDeclarationConstraintViolation), "{error:?}");
        }
    }
}
