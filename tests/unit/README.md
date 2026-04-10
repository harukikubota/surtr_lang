# Unit Test Map

`tests/unit/` mirrors the crate-level unit test ownership in the test policy.

Current execution model:
- Rust unit tests run from each crate's source files via `#[cfg(test)]`.
- This directory stores crate-by-crate viewpoints and TODOs for expansion.

Run the current workspace test gate with:

```bash
cargo test --workspace
```

Use the default runner for the wider workspace suite with:

```bash
cargo nextest run --workspace
```
