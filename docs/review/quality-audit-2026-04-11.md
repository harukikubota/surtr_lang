# Quality Audit (2026-04-11)

## Scope

- In scope: `spire`, `sigil`, `scar`, `forge`, `eldr`, `rune`, `sindr`, `diagnostics`, `xldr` CLI/REPL
- Out of scope for this pass: `xldr` TUI and `surtr tui`
  - The TUI path will be handled in one coordinated change later.

## Review Direction

This pass prioritized repository simplification over compatibility preservation.

Highest-priority cleanup targets:

1. Remove backward-compatibility shims that no longer pay their way
2. Remove deprecated opcode paths and related dead compatibility surface
3. Remove duplicated loader / compile orchestration

The project decision for follow-up work is:

- Loader defaults should be established during loader initialization
- Files loaded by the default loader are centralized under `/lib/`
- TUI cleanup is intentionally deferred to the later TUI-wide rewrite

## Commands Run

```bash
cargo check --workspace
cargo test --workspace
cargo nextest run --workspace
cargo clippy --workspace --all-targets -- -D warnings

cargo test -p sindr
cargo test -p diagnostics
cargo test -p spire
cargo test -p sigil
cargo test -p scar
cargo test -p forge
cargo test -p rune --tests
```

## Result Snapshot

- `cargo check --workspace`: passed
- `cargo test --workspace`: passed
- `cargo nextest run --workspace`: passed
- `cargo clippy --workspace --all-targets -- -D warnings`: passed
- Per-crate tests passing: workspace-wide green

## Implementation Follow-Up

Implemented in this pass:

1. Removed backward-compatibility opcode handling for `MakeFrame` / `PopFrame`
2. Removed `scar::TypeEnv::register_type_def()` and migrated tests to predeclare/resolve flow
3. Removed dead `eldr::VM::invoke_callable()` and unified `FunctionEntry` test fixtures behind a local helper
4. Centralized `/lib` scanning and additional std-module discovery in `xldr::loader`
5. Switched REPL bootstrap to fail-fast `Result` propagation with phase-tagged `LoadError::BootstrapFailed`
6. Removed `forge` silent label fallback and routed final `match` failure through explicit `PatternMismatch`
7. Updated stale README / crate docs references and replaced nonexistent `lib/hello.srt` examples with a self-contained quick start
8. Narrowed the `.eldr` restore contract in `doc/Xldr_spec.md` to document the current partial-restore behavior

Deferred by project decision:

- `xldr` TUI cleanup
- Full `.eldr` semantic restoration for user-defined functions

## Historical Findings

The sections below preserve the original review snapshot taken before the fixes listed in
`Implementation Follow-Up`. Treat them as historical findings unless a section is explicitly
called out as deferred.

## High-Priority Findings

### [High] Workspace test gate is broken by stale `FunctionEntry` fixture construction

- Files:
  - `crates/eldr/src/vm.rs`
  - `crates/sindr/src/ir.rs`
- `FunctionEntry` now requires `end_pc`, `span_start`, `span_end`, and `flags`, but several `eldr` tests still construct the old shape.
- This is the clearest remaining old-API residue in the workspace.
- Impact:
  - `cargo test --workspace` fails
  - `cargo nextest run --workspace` fails
  - `cargo clippy --workspace --all-targets -- -D warnings` fails before lint cleanup can even complete
- Recommendation:
  - Introduce a single test helper / builder for `FunctionEntry`
  - Stop open-coding `FunctionEntry { ... }` in VM tests
  - Treat direct fixture construction of evolving IR structs as unsupported

Relevant locations:

- `crates/sindr/src/ir.rs:363`
- `crates/eldr/src/vm.rs:1653`
- `crates/eldr/src/vm.rs:1684`
- `crates/eldr/src/vm.rs:1923`
- `crates/eldr/src/vm.rs:1941`
- `crates/eldr/src/vm.rs:2087`

### [High] REPL bootstrap errors are reported but not propagated

- File: `crates/xldr/src/repl/logic/core.rs`
- `ReplEngine::new()` and `ReplEngine::from_eldr()` always return `Ok(engine)` after invoking bootstrap helpers.
- `bootstrap_std_modules()` and `bootstrap_std_modules_scope_only()` emit diagnostics and return early on failure instead of surfacing an initialization error.
- Result:
  - CLI/TUI can start with a partially initialized engine
  - Broken stdlib/bootstrap state is not fail-fast
  - The resulting runtime state is harder to reason about than a hard failure
- Recommendation:
  - Make both bootstrap helpers return `Result<(), ReplBootstrapError>`
  - Fail construction if bootstrap parse/resolve/typecheck/codegen/runtime setup fails
  - Keep diagnostics rendering at the outer entrypoint rather than inside bootstrap internals

Relevant locations:

- `crates/xldr/src/repl/logic/core.rs:70`
- `crates/xldr/src/repl/logic/core.rs:103`
- `crates/xldr/src/repl/logic/core.rs:119`
- `crates/xldr/src/repl/logic/core.rs:174`
- `crates/xldr/src/repl/logic/core.rs:342`

## Medium-Priority Findings

### [Medium] Loader / stdlib discovery logic is duplicated in three places

- Files:
  - `crates/rune/src/loader.rs`
  - `crates/xldr/src/repl/logic/core.rs`
  - `tests/integration/support.rs`
- The project currently maintains separate logic for:
  - scanning `/lib`
  - deriving module paths
  - skipping built-in std modules
  - constructing additional module inputs
- This is the main structural duplication left in the repo.
- It also conflicts with the new simplification direction: loader defaults should be fixed at loader initialization, not re-derived ad hoc in multiple callers.
- Recommendation:
  - Move `/lib` discovery and default stage construction behind a single loader entrypoint
  - Reuse that same path from `rune`, REPL, and integration tests
  - Delete the token-only `derive_primary_module_path()` copy in `tests/integration/support.rs`

Relevant locations:

- `crates/rune/src/loader.rs:9`
- `crates/rune/src/loader.rs:54`
- `crates/rune/src/loader.rs:93`
- `crates/xldr/src/repl/logic/core.rs:1166`
- `tests/integration/support.rs:15`
- `tests/integration/support.rs:62`

### [Medium] `.eldr` restore remains intentionally incomplete, but is exposed as a regular path

- Files:
  - `crates/xldr/src/repl/logic/core.rs`
  - `crates/xldr/src/repl/ui/tui/mod.rs`
  - `tests/integration/repl.rs`
- The code explicitly documents that user-defined functions loaded from `.eldr` are present in the VM but absent from sigil resolution for future REPL input.
- That means restore is only partial.
- Existing test coverage checks that `:save` writes a decodable `.eldr`, but does not verify semantic restoration.
- Recommendation:
  - Either complete restoration semantics, or clearly narrow the supported contract
  - Since TUI is deferred, keep this item documented and avoid expanding surface before the later TUI rewrite

Relevant locations:

- `crates/xldr/src/repl/logic/core.rs:114`
- `crates/xldr/src/repl/ui/tui/mod.rs:39`
- `tests/integration/repl.rs:325`

### [Medium] `forge` still has defensive gaps that can silently produce bad bytecode

- File: `crates/forge/src/codegen.rs`
- `emit_match()` falls through to `end_label` for the final non-match path instead of forcing a mismatch failure path.
- `finalize()` resolves missing labels with `unwrap_or(0)`, which silently rewrites unresolved jumps to `pc=0`.
- These are not compatibility features; they are silent-failure behavior.
- Recommendation:
  - Replace unresolved label fallback with a hard codegen failure
  - Add an explicit no-match failure path in `emit_match()`

Relevant locations:

- `crates/forge/src/codegen.rs:2312`
- `crates/forge/src/codegen.rs:2575`

## Low-Priority Findings

### [Low] Small compatibility leftovers remain and should be deleted during simplification

- `crates/scar/src/env.rs` still exposes a legacy single-step helper:
  - `register_type_def()`
- `crates/xldr/src/tui.rs` is a backward-compatibility re-export shim marked for later removal
- Recommendation:
  - Delete the compatibility re-export during the TUI rewrite
  - Delete or internalize the scar legacy helper once all call sites are gone

Relevant locations:

- `crates/scar/src/env.rs:160`
- `crates/xldr/src/tui.rs:1`

### [Low] Docs contain stale references

- `README.md` still points to `lib/hello.srt`, which does not exist
- `README.md` references `doc/Enum.md`, which is not present in the repo
- `tests/unit/README.md` says `cargo test` runs all current unit tests, but the current workspace gate is broken
- Recommendation:
  - Update these after the highest-priority cleanup so the docs match reality again

Relevant locations:

- `README.md:30`
- `README.md:34`
- `README.md:84`
- `tests/unit/README.md:9`

## Test Coverage Assessment

### Stronger Areas

- `spire`: broad parser and lexer coverage
- `sigil`: strong resolver/session coverage
- `scar`: solid type-system regression coverage
- `rune` integration tests: good coverage of CLI contracts and spec fixtures

### Weaker Areas

- `forge`: only 4 unit tests despite large codegen surface
- `xldr` restore behavior: decode/save covered, semantic restore not covered
- TUI: currently deferred, effectively uncovered in this pass
- `diagnostics`: helper-template inference remains only lightly covered

## Lint / Simplification Pressure

The current lint backlog reinforces the simplification direction:

- `eldr` has a large `RuntimeError` flowing through many `Result<_, RuntimeError>` APIs
- `eldr` still contains unused `invoke_callable()`
- `spire` still has minor style/test cleanup items (`while_let_loop`, `approx_constant`)

Relevant locations:

- `crates/eldr/src/error.rs:5`
- `crates/eldr/src/builtin.rs:9`
- `crates/eldr/src/vm.rs:206`
- `crates/spire/src/parser.rs:1035`
- `crates/spire/src/lexer.rs:351`

## Recommended Execution Order

1. Fix `FunctionEntry` test fixtures so workspace-wide test and lint commands become meaningful again
2. Make REPL bootstrap fail-fast
3. Centralize loader defaults and `/lib` scanning in one path
4. Remove deprecated opcode compatibility paths if no longer needed
5. Remove remaining compatibility shims and dead helpers
6. Expand `forge` and `.eldr` restore tests after the simplification lands

## Deferred by Decision

- TUI cleanup and compatibility-shim removal tied to the TUI rewrite
- Any broad TUI contract review or test expansion
