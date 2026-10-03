use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn collect_sources(directory: &Path, files: &mut Vec<PathBuf>) {
    // Directory watches detect added and removed files, including nested modules.
    println!("cargo:rerun-if-changed={}", directory.display());
    let entries = fs::read_dir(directory).unwrap_or_else(|error| {
        panic!(
            "cannot read compiler source directory {}: {error}",
            directory.display()
        )
    });
    let mut children = entries
        .map(|entry| {
            entry.unwrap_or_else(|error| {
                panic!(
                    "cannot read compiler source entry in {}: {error}",
                    directory.display()
                )
            })
        })
        .collect::<Vec<_>>();
    children.sort_by_key(|entry| entry.path());
    for entry in children {
        let path = entry.path();
        let kind = entry.file_type().unwrap_or_else(|error| {
            panic!("cannot inspect compiler source {}: {error}", path.display())
        });
        if kind.is_dir() {
            collect_sources(&path, files);
        } else if kind.is_file() {
            files.push(path);
        } else {
            panic!(
                "compiler source must be a regular file or directory: {}",
                path.display()
            );
        }
    }
}

fn hash_field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn hash_environment(hash: &mut Sha256, name: &str, required: bool) {
    println!("cargo:rerun-if-env-changed={name}");
    hash_field(hash, name.as_bytes());
    match env::var_os(name) {
        Some(value) => {
            hash.update([1]);
            let value = value.into_string().unwrap_or_else(|_| {
                panic!("compiler build environment {name} must be valid Unicode")
            });
            hash_field(hash, value.as_bytes());
        }
        None if !required => hash.update([0]),
        None => panic!("compiler build environment {name} is required"),
    }
}

fn hash_tool_identity(hash: &mut Sha256, variable: &str) {
    let program = env::var_os(variable)
        .unwrap_or_else(|| panic!("compiler build environment {variable} is required"));
    let output = Command::new(program)
        .args(["--version", "--verbose"])
        .output()
        .unwrap_or_else(|error| panic!("cannot read {variable} toolchain identity: {error}"));
    assert!(
        output.status.success(),
        "{variable} toolchain identity command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    hash_field(hash, variable.as_bytes());
    hash_field(hash, &output.stdout);
}

fn main() {
    let manifest = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR is required for compiler build key"),
    );
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .expect("xldr must be located under the workspace crates directory");
    let mut files = vec![
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        manifest.join("build.rs"),
    ];
    for name in [
        "diagnostics",
        "sindr",
        "spire",
        "sigil",
        "scar",
        "forge",
        "eldr",
        "surtr-analysis",
        "xldr",
    ] {
        let directory = root.join("crates").join(name);
        files.push(directory.join("Cargo.toml"));
        collect_sources(&directory.join("src"), &mut files);
    }
    files.sort();
    files.dedup();
    let mut hash = Sha256::new();
    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        let relative = file
            .strip_prefix(root)
            .expect("compiler fingerprint source must be inside the workspace")
            .to_str()
            .expect("compiler fingerprint source path must be valid Unicode");
        let contents = fs::read(&file).unwrap_or_else(|error| {
            panic!("cannot read compiler fingerprint source {relative}: {error}")
        });
        hash_field(&mut hash, relative.as_bytes());
        hash_field(&mut hash, &contents);
    }
    for name in [
        "TARGET",
        "HOST",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "RUSTC",
        "CARGO",
    ] {
        hash_environment(&mut hash, name, true);
    }
    for name in [
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
    ] {
        hash_environment(&mut hash, name, false);
    }
    // Cargo supplies these values for the selected target and feature set.
    let mut configuration = env::vars_os()
        .filter_map(|(name, _)| {
            let name = name
                .into_string()
                .expect("compiler build environment name must be valid Unicode");
            (name.starts_with("CARGO_FEATURE_") || name.starts_with("CARGO_CFG_")).then_some(name)
        })
        .collect::<Vec<_>>();
    configuration.sort();
    for name in configuration {
        hash_environment(&mut hash, &name, true);
    }
    hash_tool_identity(&mut hash, "RUSTC");
    hash_tool_identity(&mut hash, "CARGO");
    println!(
        "cargo:rustc-env=XLDR_COMPILER_BUILD_KEY={:x}",
        hash.finalize()
    );
}
