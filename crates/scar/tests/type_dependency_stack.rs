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
