#![allow(dead_code)]
#[path = "../tests/support/mod.rs"]
mod support;

fn main() {
    let path = support::prewarm_std_prefix();
    // Manual prewarm has no nextest environment file to publish to.
    let Some(env_file) = std::env::var_os("NEXTEST_ENV") else {
        return;
    };
    use std::io::Write;
    let mut output = std::fs::OpenOptions::new()
        .append(true)
        .open(env_file)
        .expect("nextest environment file should be writable");
    writeln!(
        output,
        "SURTR_SCAR_TEST_PREFIX={}",
        path.canonicalize().unwrap().display()
    )
    .expect("Scar prefix path should publish to nextest");
}
