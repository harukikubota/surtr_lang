# Test Overhaul Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reorganize Surtr tests by execution temperature, improve fixture cache/timing visibility for cold and hot runs, and update partial-test guidance to match the actual Cargo targets.

**Architecture:** Keep crate-local Rust unit tests where they are, move disk fixtures under explicit `tests/fixtures/{script,modules}/{pass,fail}` roots, and keep CLI/process integration tests under `tests/integration`. Add opt-in timing/cache reporting behind `SURTR_TEST_TIMING=1` so normal test output remains quiet.

**Tech Stack:** Rust workspace, `cargo nextest`, Surtr fixture harnesses under `tests/integration`, docs under `docs/dev` and `tests`.

---

### Task 1: Fixture Root Contract

**Files:**
- Modify: `tests/integration/common.rs`
- Test: `tests/integration/common.rs`

- [x] **Step 1: Add tests that require new fixture roots**

Add unit tests in `tests/integration/common.rs` asserting that script pass fixtures come from `tests/fixtures/script/pass`, script fail fixtures come from `tests/fixtures/script/fail`, module pass fixtures come from `tests/fixtures/modules/pass`, and module fail fixtures come from `tests/fixtures/modules/fail`.

- [x] **Step 2: Run tests and confirm red**

Run: `cargo nextest run -p rune --test integration script_pass_fixtures_use_warm_fixture_root script_fail_fixtures_use_warm_fixture_root module_pass_fixtures_use_warm_fixture_root module_fail_fixtures_use_warm_fixture_root`

Expected: fails because discovery still reads `tests/spec` and `tests/compile_errors`.

- [x] **Step 3: Update discovery constants/helpers**

Introduce explicit root helpers in `tests/integration/common.rs` and point fixture collectors at the new roots.

- [x] **Step 4: Run common tests and confirm green**

Run: `cargo nextest run -p rune --test integration common::tests`

Expected: pass after fixture relocation in Task 2.

### Task 2: Fixture Relocation

**Files:**
- Move: `tests/spec/functions/**` to `tests/fixtures/script/pass/functions/**`
- Move: `tests/spec/json/**` to `tests/fixtures/script/pass/json/**`
- Move: `tests/spec/strings/**` to `tests/fixtures/script/pass/strings/**`
- Move: `tests/spec/usecases/**` to `tests/fixtures/script/pass/usecases/**`
- Move: root `tests/spec/process_*.{srt,expected}` to `tests/fixtures/script/pass/process_runtime/**`
- Move: `tests/spec/stdmod/**` to `tests/fixtures/script/pass/stdmod/**`
- Move: `tests/compile_errors/{parse,resolve,strings,undefined_variable,exhaustiveness}/**` to `tests/fixtures/script/fail/<same>/**`
- Move: `tests/compile_errors/type_mismatch/**` to `tests/fixtures/script/fail/typecheck/**`
- Move: root `tests/compile_errors/process_*.{srt,error}` to `tests/fixtures/script/fail/process_runtime/**`
- Move: `tests/spec/modules/**` to `tests/fixtures/modules/pass/**`
- Move: `tests/compile_errors/modules/**` to `tests/fixtures/modules/fail/**`
- Modify hard-coded fixture references in integration tests.

- [x] **Step 1: Move files with `git mv`**

Preserve all `.srt`, `.expected`, and `.error` pairs.

- [x] **Step 2: Fix direct path references**

Update references in `tests/integration/build_roundtrip.rs`, `tests/integration/run_eldr.rs`, `tests/integration/namespaces.rs`, and `tests/integration/private_visibility.rs`.

- [x] **Step 3: Run fixture harnesses**

Run: `cargo nextest run -p rune --test integration run_srt module_import_fixtures namespaces private_visibility`

Expected: pass.

### Task 3: Timing and Cache Report

**Files:**
- Create: `tests/integration/support/timing.rs`
- Modify: `tests/integration/support/mod.rs`
- Modify: `tests/integration/support/cache.rs`
- Modify: `tests/integration/run_srt.rs`

- [x] **Step 1: Add report formatting tests**

Add tests for a compact report that includes group name, fixture count, total seconds, slowest fixtures, semantic prefix cache hit/miss/write counts, and final `.eldr` hit/miss/write counts.

- [x] **Step 2: Run tests and confirm red**

Run: `cargo nextest run -p rune --test integration support::timing`

Expected: fails because `support::timing` does not exist.

- [x] **Step 3: Implement counters and report**

Keep reporting opt-in via `SURTR_TEST_TIMING=1`. Report to stderr only. Do not change normal stdout/stderr contracts.

- [x] **Step 4: Run timing tests and fixture buckets with env**

Run: `SURTR_TEST_TIMING=1 SURTR_TEST_CACHE=1 cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_0 run_srt::compile_error_fixtures_bucket_0 --no-capture`

Expected: pass and print timing/cache report lines.

### Task 4: Cache Strategy Cleanup

**Files:**
- Modify: `tests/integration/support/cache.rs`
- Test: `tests/integration/run_srt.rs`

- [x] **Step 1: Keep semantic prefix cache always-on and final fixture cache opt-in**

Preserve existing semantics while recording hit/miss/write/corrupt events.

- [x] **Step 2: Add/adjust tests for final cache reuse**

Use the existing `compile_error_phase_primes_semantic_prefix_cache_without_final_bytecode_cache` test and add a pass-fixture cache reuse check if needed.

- [x] **Step 3: Run cache-focused tests**

Run: `SURTR_TEST_CACHE=1 cargo nextest run -p rune --test integration run_srt::compile_error_phase_primes_semantic_prefix_cache_without_final_bytecode_cache run_eldr::run_source_uses_run_cache_on_repeated_invocation`

Expected: pass.

### Task 5: Documentation

**Files:**
- Modify: `docs/dev/テスト方針.md`
- Modify: `tests/README.md`
- Add: `tests/profile/README.md`

- [x] **Step 1: Update source-of-truth layout**

Document hot/warm/cold temperature:
`crates/**` unit tests are hot, `tests/fixtures/**` fixture suites are warm, `tests/integration/**` CLI/process tests are cold, `tests/profile/**` is manual profiling.

- [x] **Step 2: Fix partial commands**

Use the actual target shape, for example `cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket_0`.

- [x] **Step 3: Add cold/hot profile commands**

Document cold run as removing `target/test-fixture-cache`, then running with `SURTR_TEST_TIMING=1 SURTR_TEST_CACHE=1`; hot run reruns the same command without removal.

### Task 6: Verification and Main Integration

**Files:**
- Whole repo

- [x] **Step 1: Format**

Run: `cargo fmt`

- [x] **Step 2: Targeted verification**

Run: `cargo nextest run -p rune --test integration run_srt module_import_fixtures`

- [x] **Step 3: Full verification**

Run: `cargo nextest run --workspace`

- [x] **Step 4: Completion audit**

Map every user requirement to concrete artifacts and command output.

- [x] **Step 5: Merge to main and remove worktree**

Use `git checkout main`, `git merge codex/test-overhaul`, then remove `.worktrees/test-overhaul` after verification.
