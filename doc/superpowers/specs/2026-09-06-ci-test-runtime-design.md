# CI Test Runtime Recovery Design

## Purpose

Restore the full CI suite's runtime margin after the call-site return type
argument work, without weakening semantic coverage or increasing the 15 second
timeout.

## Confirmed causes

- Test growth from roughly 1900 to 2034 explains only part of the wall-clock
  increase.
- Since `65ba6172`, every ordinary call snapshots the complete Scar inference
  state, and builtin/user calls snapshot it a second time. Successful calls
  discard those deep clones without using them.
- `crates/xldr/tests/repl_core.rs` runs 134 cases as separate nextest
  subprocesses. Each process pays REPL/bootstrap preparation again, and two
  multi-contract cases reach the 15 second timeout under CI contention.
- The stdlib semantic snapshot is safe to share across processes, but a clean
  CI run currently lets many test processes race to create it instead of
  preparing it once.
- The existing Rune fixture buckets are below the timeout in isolation. More
  buckets would add process/bootstrap overhead unless post-fix measurements
  demonstrate a remaining need.

## Design

### Scar checkpoint ownership

`CandidateProbeCheckpoint` belongs only to callers that intentionally try a
candidate and can continue along another path after failure. An ordinary
`check_app_with_expected` call is not itself speculative and must not snapshot
or roll back the full checker state. Existing candidate loops and fallback
sites retain their explicit checkpoint/rollback pairs, structured candidate
failures, and fail-closed behavior.

A crate-local regression test counts checkpoint creation in test builds and
asserts that successful plain, builtin, and user function applications create
no candidate checkpoints. Existing candidate-failure and return-type-argument
tests continue to guard rollback semantics.

### Xldr core test process layout

Keep every existing case body and assertion, but execute them through 64
deterministic bucket tests. Cases are assigned round-robin by a stable table in
source order. The bucket runner prints the case name before execution so a
failure remains attributable. Together with one inventory test, this reduces
the nextest process surface from 134 to 65. Fifty-five buckets contain two
semantic cases and nine contain three.

The original 16-bucket design was rejected by measurement: all 16 buckets
timed out at default eight-way concurrency. After fixing the previously
unreadable semantic cache, a current-schema recheck still timed out all 16
buckets; 32 left 11 timeouts. Forty-eight passed but peaked at 13.164 seconds.
The 64-bucket layout passed twice, with a complete raw-run maximum of 11.725
seconds, and is the smallest measured layout with the selected margin. The
final workspace run remains the contention gate.

Split the two timed-out multi-contract cases before bucketing:

- imported lowercase Boolean alias shadowing and live top-level definition
  shadowing become separate cases;
- `:clear`, `:reload`, and `:reload defs` become separate cases. The reload
  case reconstructs the original `:clear` history before reloading, so it still
  proves that definitions survive while value bindings do not.

No assertion is removed and the overall timeout stays at 15 seconds.

### Snapshot-backed REPL construction follow-up

After the 64-bucket recovery landed, measurement showed that
`ReplEngine::new()` still parsed, resolved, type-checked, generated, and
executed the default stdlib for every semantic case. Build one immutable
default REPL bootstrap state per process through the existing semantic
snapshot/preload path instead. The shared state contains sources, Sigil scope,
Scar checkpoint, bytecode, and metadata; each engine creates a fresh VM,
executes its runtime boot plan, and clones its mutable session state.

The replaced source-bootstrap path is removed. `.eldr` scope restoration uses
the same bootstrap state and rejects a stdlib stage-layout or compiled
function/type/callable/process prefix mismatch instead of falling back to
reparsing sources. With that construction path, eight round-robin buckets plus
the inventory test run all 137 cases in 7.180 seconds
under `profile ci`, so the timeout remains 15 seconds.

### Clean CI preparation

The `ci` nextest profile enables nextest's setup-script feature and runs one
setup script before test processes. The
script invokes the built Surtr CLI on a minimal manual profile source so the
default stdlib semantic snapshot is written once. It does not precompute
fixture results, change correctness conditions, or hide cache misses. The
existing `SURTR_TEST_CACHE=1` opt-in remains the final-bytecode cache policy.

The preparation command must work from a clean target directory and on a
package-filtered CI invocation; it uses `cargo run -p rune --bin surtr` so it
does not assume a pre-existing `target/debug/surtr` path.

Prewarm validation must prove a real cross-process cache hit, not merely two
successful compiles. The initial check exposed an incompatible Serde field
attribute on `ResolvedId`: omitting `None` fields shifts bincode's positional
struct encoding, so every cache load failed before schema/key validation. Keep
the field default for compatibility, serialize the `None` marker explicitly,
and bump the stdlib semantic cache schema so older invalid entries fail closed
and rebuild normally.

### Measurement-led fixture organization

After the Scar and Xldr changes, measure the previously large Rune buckets.
Only split a bucket if it remains close enough to 15 seconds to be unstable
under contention. Correct the documented script bucket counts and document the
CI prewarm. Do not merge or delete source-level fixtures merely because their
names resemble another layer's tests.

## Validation

- Scar crate-local regression and full `-p scar` suite.
- Return-type-argument and candidate-failure focused tests.
- Xldr bucket inventory check proving every case is registered exactly once,
  then `-p xldr --test repl_core` under the CI profile.
- Clean-cache CI setup smoke test.
- Rune timing for the previously largest module/language buckets.
- Final `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace` and
  comparison with the reported 359.479 second baseline.

## Constraints

- Preserve current language semantics and diagnostics.
- Preserve fail-closed candidate selection and rollback at genuine probe sites.
- Do not add a legacy path or fallback.
- Do not increase timeout values.
- Do not commit unless explicitly requested.
