# CI Test Runtime Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore CI runtime margin and eliminate the two Xldr timeouts without reducing semantic coverage or increasing timeout limits.

**Architecture:** Remove full Scar state snapshots from non-speculative call checking while retaining rollback at explicit candidate probes. Aggregate Xldr core cases into deterministic process-sharing buckets, prewarm the shared stdlib snapshot once for the CI profile, and change Rune buckets only when post-fix timing demonstrates a need.

**Tech Stack:** Rust, cargo-nextest 0.9.132, TOML, POSIX shell

**Spec:** `docs/superpowers/specs/2026-09-06-ci-test-runtime-design.md`

## Global Constraints

- Preserve current language semantics and diagnostics.
- Preserve structured candidate failures and rollback at genuine probe sites.
- Keep `slow-timeout.period = "15s"` and `terminate-after = 1` unchanged.
- Keep every existing Xldr core assertion and register every case exactly once.
- Do not change fixture correctness based on cache availability.
- Do not commit unless the user explicitly requests it.

---

### Task 1: Remove non-speculative Scar call checkpoints

**Files:**
- Modify: `crates/scar/src/checker/mod.rs`
- Modify: `crates/scar/src/checker/expr.rs`

**Interfaces:**
- Consumes: `Checker::candidate_probe_checkpoint` and `Checker::rollback_candidate_probe` for genuine probes.
- Produces: ordinary `check_app_with_expected` success/error checking with no implicit checkpoint ownership.

- [x] Add a test-only checkpoint counter to `Checker`, increment it in `candidate_probe_checkpoint`, and add a crate-local test that successfully applies plain `Ty::Func`, `Ty::BuiltinFunc`, and `Ty::UserFunc` values and expects zero checkpoints.
- [x] Run the new test before production changes and confirm it fails because ordinary application creates five checkpoints (one outer checkpoint per call and one additional branch checkpoint for builtin/user calls).
- [x] Remove the outer and builtin/user branch checkpoint/rollback wrappers from `check_app_with_expected`; do not touch explicit caller-side probe checkpoints.
- [x] Run the new test, the existing `constructor_context_bind_preserves_candidate_failures` case, the return-type-argument test binaries, and `rtk cargo nextest run -p scar`.
- [x] Rebuild Rune and measure warm `tests/profile/heavy_compile.srt` with an isolated `SURTR_STDLIB_CACHE_DIR`.

### Task 2: Bucket Xldr REPL core cases and split timed-out contracts

**Files:**
- Modify: `crates/xldr/tests/repl_core.rs`

**Interfaces:**
- Consumes: all existing zero-argument REPL core case functions.
- Produces: `REPL_CORE_CASES: &[(&str, fn())]`, a deterministic `run_repl_core_bucket`, and a measured 64 `repl_core_bucket_N` nextest tests.

- [x] Split `core_completion_hides_lowercase_bool_alias_when_shadowed_by_import_or_top_level_def` into import-shadowing and live-definition-shadowing functions without changing their assertions.
- [x] Split `core_reload_and_clear_commands_preserve_only_requested_state` into clear, reload, and reload-defs functions. Reconstruct `seed`, `keep`, and the `:clear` step inside the reload case so the original history-dependent contract remains covered.
- [x] Remove `#[test]` only from the individual zero-argument case functions, register each by name and function pointer exactly once in `REPL_CORE_CASES`, add `#![deny(dead_code)]`, and add measured bucket wrappers.
- [x] Add an inventory test over the table that rejects duplicate names and duplicate function pointers. As a one-time migration audit, compare the 134 original test names plus the three net-new split names with all 137 table entries.
- [x] Measure bucket counts with `rtk cargo nextest run --profile ci -p xldr --test repl_core`; retain 64 after rejecting 16/32 for timeouts and 48 for inadequate margin, then record the slowest bucket.

### Task 3: Prime the shared stdlib snapshot once for clean CI

**Files:**
- Create: `tests/profile/stdlib_prewarm.srt`
- Create: `scripts/ci-stdlib-prewarm.sh`
- Modify: `.config/nextest.toml`
- Modify: `crates/sigil/src/resolved.rs`
- Modify: `crates/xldr/src/lib.rs`
- Modify: `docs/dev/テスト方針.md`
- Modify: `tests/README.md`

**Interfaces:**
- Consumes: `cargo run -p rune --bin surtr -- check tests/profile/stdlib_prewarm.srt` and Xldr's content-addressed stdlib semantic cache.
- Produces: a `ci-stdlib-prewarm` setup script selected only by `profile.ci`.

- [x] Add a minimal valid manual-profile Surtr source and a `set -eu` script that runs its definition check from the repository root while suppressing successful JSON output.
- [x] Enable `experimental = ["setup-scripts"]`, define `[scripts.setup.ci-stdlib-prewarm]` with a 60 second setup-only timeout and captured output, and select it through `[[profile.ci.scripts]] filter = "all()"`; leave default/cold profiles unchanged.
- [x] Add a bincode roundtrip regression for `ResolvedId { symbol_info: None }`, serialize the `None` marker instead of omitting the positional field, and bump the stdlib semantic cache schema so invalid schema-11 entries rebuild.
- [x] From an isolated empty `SURTR_STDLIB_CACHE_DIR`, run the script and confirm it creates `std.semantic`; run it again and prove a true cache hit by unchanged inode, mtime, and content hash.
- [x] Run a package-filtered CI-profile smoke test to ensure setup does not assume the plain CLI binary already exists.
- [x] Document the preparation order, cache correctness boundary, and exact CI command.

### Task 4: Measure and minimally rebalance remaining suites

**Files:**
- Modify only if measurement requires it: `tests/integration/module_import_fixtures.rs`
- Modify only if measurement requires it: `tests/integration/language_features.rs`
- Modify: `docs/dev/テスト方針.md`
- Modify: `tests/README.md`

**Interfaces:**
- Consumes: `SURTR_TEST_TIMING=1` reports and the fixed 15 second timeout.
- Produces: corrected bucket documentation and only evidence-required bucket splits.

- [x] Measure module fail bucket 3 and language-feature buckets 3 and 7 after Tasks 1-3.
- [x] If every measured bucket is below 7.5 seconds, record the ruling and do not add more processes; otherwise split only the measured offender so each half is below 7.5 seconds.
- [x] Correct script fixture documentation to pass buckets `0..7` and fail buckets `0..15`, and document Xldr's 64 core buckets.
- [x] Run the touched Rune integration filters and Xldr core suite.

### Task 5: Full verification

**Files:**
- Verify all changed files.

**Interfaces:**
- Consumes: Tasks 1-4.
- Produces: an evidence-backed before/after runtime report.

- [x] Run formatting and `git diff --check`.
- [x] Run `rtk cargo nextest run -p scar` and `rtk cargo nextest run --profile ci -p xldr --test repl_core`.
- [x] Clear only the task's test caches, then run `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace` once from the worktree.
- [x] Report test totals, timeouts, wall-clock, targeted performance measurements, changed files, and any intentionally unmodified buckets.
