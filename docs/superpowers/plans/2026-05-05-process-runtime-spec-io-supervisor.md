# Process Runtime Spec, IO Handlers, And Supervisor Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `docs/dev/ProcessRuntime_spec.md` the formal process runtime specification and implement the required Rust/PureSurtr I/O handler replacement plus user-visible supervisor/process surfaces.

**Architecture:** Treat input2 as the new formal contract, then implement the smallest stable runtime schema that can carry Agent, GenServer, Supervisor, RuntimeSupervisor, DynamicSupervisor, Task, handler dependencies, and boot entries. Keep the current Agent lowering working while adding the new surface in staged slices, with runtime I/O handler replacement backed by Eldr-owned standard handler slots.

**Tech Stack:** Rust workspace crates (`spire`, `sigil`, `scar`, `forge`, `sindr`, `eldr`, `rune`, `xldr`), standard definition sources in `lib/*.srt`, `cargo nextest`.

---

### Task 1: Formal Spec Promotion

**Files:**
- Create: `docs/dev/ProcessRuntime_spec.md`
- Modify: `docs/dev/README.md`
- Modify: `docs/dev/EldrVM_spec.md`
- Modify: `docs/dev/テスト方針.md`
- Modify: `docs/dev/Xldr_spec.md`

- [ ] Copy input2 into `docs/dev/ProcessRuntime_spec.md`, preserving it as the formal spec.
- [ ] Add a short status header explaining that the process runtime spec is formal and that PubSub/distributed/generic receive/yield remain out of scope.
- [ ] Update `EldrVM_spec.md` to point VM process semantics to the new spec and summarize standard I/O handler replacement.
- [ ] Update `テスト方針.md` so `surtr test --all` and Test DSL I/O capture are defined in terms of handler-backed per-test buffers.
- [ ] Update `Xldr_spec.md` only for load-order names if new standard modules are added.
- [ ] Run `cargo nextest run -p rune --test test_command` if only docs and test stdlib contracts changed.
- [ ] Commit: `docs: promote process runtime spec`

### Task 2: Runtime IO Handler Backend

**Files:**
- Modify: `crates/eldr/src/vm.rs`
- Modify: `crates/eldr/src/builtin.rs`
- Modify: `crates/sindr/src/builtin.rs`
- Modify: `lib/test.srt`
- Test: `crates/eldr/src/builtin.rs` unit tests

- [ ] Add VM-owned standard I/O handler state for stdout, stderr, and stdin buffers. Preserve current `VmIoPolicy` public behavior.
- [ ] Add Rust APIs for tests: replace stdout/stderr with buffers, push stdin text, drain stdout/stderr, and inspect remaining stdin.
- [ ] Add hidden Test builtins for PureSurtr: `__test_push_stdin`, `__test_capture_stdin_remaining` or equivalent minimal observation helper.
- [ ] Implement PureSurtr `Test::push_stdin(text)`, `Test::capture_stdout()`, `Test::capture_stderr()`, `Test::assert_stdout_eq(lines)`, `Test::assert_stderr_eq(lines)`.
- [ ] Make `IO::get` and `IO::get_line` read through the injected stdin handler before host stdin.
- [ ] Verify red/green with focused `eldr` builtin tests.
- [ ] Commit: `feat: route test io through replaceable handlers`

### Task 3: Per-`it` Test IO Isolation

**Files:**
- Modify: `crates/eldr/src/vm.rs`
- Modify: `lib/test.srt`
- Modify: `tests/integration/test_command.rs`
- Modify: `lib/tests/*.srt` as needed

- [ ] Reset test I/O cursors and stdin buffer at the start of each `Test::it`.
- [ ] Preserve the test event `io` snapshots used by `surtr test`.
- [ ] Add integration tests proving one `it` cannot observe stdout/stderr/stdin left by a previous `it`.
- [ ] Add PureSurtr `lib/tests` coverage for stdout, stderr, and stdin replacement.
- [ ] Run `cargo nextest run -p rune --test test_command`.
- [ ] Run `cargo nextest run -p rune --test run_srt` if spec fixtures are touched.
- [ ] Commit: `feat: isolate test io per case`

### Task 4: Process Runtime Schema Expansion

**Files:**
- Modify: `crates/spire/src/ast.rs`
- Modify: `crates/sindr/src/ir.rs`
- Modify: `crates/sindr/src/viewer.rs`
- Modify: `crates/forge/src/codegen.rs`
- Modify: `crates/eldr/src/vm.rs`
- Modify: `tests/integration/build_roundtrip.rs`
- Modify: `tests/integration/run_eldr.rs`

- [ ] Replace/extend `ReadOnlyAgent` and `StateAgent` runtime kind storage with `Agent`, `GenServer`, `Supervisor`, `RuntimeSupervisor`, `DynamicSupervisor`, and `Task`.
- [ ] Rename runtime instance `Multi` to `Worker`, keeping deserialization compatibility for old `.eldr` if practical.
- [ ] Add handler/dependency/boot-plan fields with serde defaults so older bytecode remains readable.
- [ ] Keep existing Agent tests passing by mapping old lowered Agent metadata into the new schema.
- [ ] Update dump/viewer JSON tests to assert new kind/instance labels.
- [ ] Run `cargo nextest run -p forge`, `cargo nextest run -p rune --test build_roundtrip`, and `cargo nextest run -p rune --test run_eldr`.
- [ ] Commit: `feat: generalize runtime process spec schema`

### Task 5: Surface Parser For New Process Forms

**Files:**
- Modify: `crates/spire/src/token.rs`
- Modify: `crates/spire/src/lexer.rs`
- Modify: `crates/spire/src/parser/decl.rs`
- Modify: `crates/spire/src/ast.rs`
- Test: `crates/spire/src/parser/tests.rs`

- [ ] Add tokens for `defgenserver`, `defsupervisor`, `defdynamic_supervisor` if a keyword form is needed, and `supervisor_init`.
- [ ] Parse `meta { instance, init_policy, handlers { slot: Capability = Target } }` inside process declarations.
- [ ] Derive Agent kind from `@set` presence for the new `defagent` form while preserving old `@agent(...) defagent` compatibility temporarily.
- [ ] Parse GenServer `@init`, `@call`, `@cast`, private helper defs, and generate module wrappers.
- [ ] Parse supervisor definitions and `supervisor_init` top-level blocks into AST metadata.
- [ ] Reject unsupported forms from input2: PubSub, generic receive, generic send, yield, Task.Supervisor, worker lazy init.
- [ ] Run `cargo nextest run -p spire`.
- [ ] Commit: `feat: parse process runtime surface`

### Task 6: Semantic Checks And BootPlan

**Files:**
- Modify: `crates/sigil/src/**`
- Modify: `crates/scar/src/**`
- Modify: `crates/forge/src/codegen.rs`
- Modify: `crates/eldr/src/vm.rs`
- Test: `tests/compile_errors/process_runtime/**`
- Test: `tests/spec/process_runtime/**`

- [ ] Resolve process-local `ctx.<slot>` names from `meta.handlers`.
- [ ] Enforce Lazy allowed only for Singleton Agent and Singleton GenServer.
- [ ] Enforce `ProcessInit<T>` only as Lazy `@init` return.
- [ ] Enforce `required_singletons <= available_singletons`, with standard I/O singletons always available.
- [ ] Validate handler override slots and capability names in `supervisor_init`.
- [ ] Emit BootPlan metadata and initialize standard singletons plus listed singleton processes at VM boot.
- [ ] Add compile-error tests for invalid Lazy, missing singleton availability, unknown handler slot, capability mismatch, and `ctx` misuse.
- [ ] Run `cargo nextest run -p sigil`, `cargo nextest run -p scar`, `cargo nextest run -p rune --test run_srt`.
- [ ] Commit: `feat: validate process boot plan`

### Task 7: Supervisor And DynamicSupervisor User Surface

**Files:**
- Modify: `lib/process.srt`
- Modify: `crates/spire/src/parser/decl.rs`
- Modify: `crates/eldr/src/vm.rs`
- Modify: `crates/eldr/src/builtin.rs`
- Modify: `crates/sindr/src/builtin.rs`
- Test: `lib/tests/process.srt`
- Test: `tests/spec/process_runtime/supervisor_*.srt`

- [ ] Expose `Supervisor`, `RuntimeSupervisor`, and `DynamicSupervisor` standard module/user surface expected by input2.
- [ ] Keep `DynamicSupervisor::spawn(MyWorker::init_route(args))` user-facing without requiring an explicit supervisor PID.
- [ ] Implement worker default owner as current process in VM process allocation.
- [ ] Add lifecycle sink placeholders and dump/status visibility without implementing PubSub or distributed behavior.
- [ ] Add tests proving user code can call supervisor surface and spawn worker processes through DynamicSupervisor.
- [ ] Run `cargo nextest run -p eldr`, `cargo nextest run -p rune --test run_eldr`, `cargo nextest run -p rune --test test_command`.
- [ ] Commit: `feat: expose supervisor process surface`

### Task 8: Final Verification

**Files:**
- All touched files

- [ ] Run `cargo fmt`.
- [ ] Run targeted suites from changed phases.
- [ ] Run `cargo nextest run --workspace`.
- [ ] Run `cargo nextest run -p rune --test run_srt`.
- [ ] Run `cargo nextest run -p rune --test test_command`.
- [ ] Review `git status --short` and `git diff --stat`.
- [ ] Commit remaining fixes if any.
