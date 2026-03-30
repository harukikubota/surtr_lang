# Unit Test Map

`tests/unit/` mirrors the crate-level unit test ownership in the test policy.

Current execution model:
- Rust unit tests run from each crate's source files via `#[cfg(test)]`.
- This directory stores crate-by-crate viewpoints and TODOs for expansion.

Run all current unit tests with:

```bash
cargo test
```
