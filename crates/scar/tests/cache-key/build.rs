use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    // Watching directories also catches source additions and deletions.
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_dir() {
        let mut children: Vec<_> = fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        children.sort();
        for child in children {
            collect(&child, files);
        }
    } else {
        files.push(path.to_owned());
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .unwrap();
    let mut files = vec![root.join("Cargo.toml"), root.join("Cargo.lock")];
    for name in ["diagnostics", "sindr", "spire", "sigil", "scar"] {
        files.push(root.join(format!("crates/{name}/Cargo.toml")));
        collect(&root.join(format!("crates/{name}/src")), &mut files);
    }
    collect(&root.join("lib"), &mut files);
    collect(&root.join("crates/scar/tests/support"), &mut files);
    collect(Path::new(env!("CARGO_MANIFEST_DIR")), &mut files);
    files.sort();
    files.dedup();
    let mut hash = Sha256::new();
    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        let relative = file
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .unwrap()
            .as_bytes();
        let contents = fs::read(&file).unwrap();
        hash.update((relative.len() as u64).to_le_bytes());
        hash.update(relative);
        hash.update((contents.len() as u64).to_le_bytes());
        hash.update(contents);
    }
    for name in [
        "TARGET",
        "HOST",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
        hash.update(name.as_bytes());
        hash.update(
            env::var_os(name)
                .unwrap_or_default()
                .to_string_lossy()
                .as_bytes(),
        );
        hash.update([0]);
    }
    let rustc = Command::new(env::var_os("RUSTC").unwrap())
        .args(["--version", "--verbose"])
        .output()
        .unwrap();
    assert!(rustc.status.success(), "rustc identity should be readable");
    hash.update(rustc.stdout);
    println!("cargo:rustc-env=SCAR_TEST_PREFIX_KEY={:x}", hash.finalize());
}
