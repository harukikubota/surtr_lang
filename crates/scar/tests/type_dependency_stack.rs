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
