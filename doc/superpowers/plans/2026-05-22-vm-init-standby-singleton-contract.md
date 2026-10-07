# VM Init Standby Singleton Contract

## Goal

Define the implementation contract for runtime error provenance and singleton initialization phases before changing code.

Surtr should keep process runtime simple: all configured singleton services must be ready before user runtime execution begins. The former `Lazy` singleton initialization model is renamed and redefined as a VM-init-time standby/retry protocol, not first-use lazy loading.

## Naming Changes

- Rename `init_policy: Lazy` to `init_policy: Standby`.
- Rename implementation and spec identifiers that say `lazy_init` / `LazyProcessInit` to `standby_init` / `StandbyProcessInit`.
- Keep `init_policy: Eager` unchanged.
- Keep the user-facing enum shape conceptually the same, but rename it from `ProcessInit<T>` to `StandbyInit<T>` unless compatibility concerns require a staged alias.
- `Standby` means: the VM may retry or wait during `VM::Init`, but runtime execution never starts until the singleton reaches `Ready`.

The important semantic change is that `Standby` is not lazy-on-first-message. It is boot/init phase readiness work.

## Phase Model

Runtime diagnostics and trace snapshots should distinguish three phases:

- `Compile`: parse, resolve, typecheck, and codegen errors.
- `VM::Init`: BootPlan processing, singleton allocation, eager init, standby init retry, handler override validation, and init timeout handling.
- `Runtime`: top-level execution, function calls, closure calls, TCO, process messaging, worker spawn, task execution, and ordinary `Result::Err` propagation.

`VM::Init` must finish before `Runtime` starts. If `VM::Init` fails, user top-level runtime code has not run and no runtime rollback of already-executed user work is needed.

## Singleton Readiness Contract

All singleton entries selected by `RuntimeBootPlan` must be fully initialized during `VM::Init`.

For `Eager`:

- `Ok(state)` means the singleton is ready.
- `Err(error)` becomes `RuntimeError::ProcessInitFailed`.

For `Standby`:

- `Ok(StandbyInit::Ready(state))` means the singleton is ready.
- `Ok(StandbyInit::Pending)` retries using the VM default standby retry policy.
- `Ok(StandbyInit::PendingAfter(duration))` retries after the requested duration, subject to init timeout.
- `Err(error)` becomes `RuntimeError::ProcessInitFailed`.
- Init timeout becomes `RuntimeError::ProcessInitTimeout`.

When `Runtime` begins, singleton direct messaging and singleton `pid()` lookup may assume the service exists and is ready. Callers should only handle messaging-domain errors, not readiness or init failures.

## Messaging During Init

Messaging from singleton `@init` / standby init is prohibited.

This includes:

- Singleton direct call/cast/get/set generated surfaces.
- PID-based process messaging.
- `Task::launch` or any fire-and-forget task surface.
- Reply-later callbacks that introduce a detached continuation.
- Any runtime-managed call that can outlive the current init execution.

The preferred enforcement is compile-time rejection for process `@init` bodies. If a dynamic path reaches the VM anyway, it must fail as a `VM::Init` phase violation, not as a recoverable `Result::Err`.

Allowed during init:

- Pure function calls.
- Builtins that do not enqueue messages, spawn tasks, or create detached continuations.
- Construction of values, including init route values for worker pools, as long as no worker process is spawned during singleton init unless the operation is explicitly part of the VM init contract.

## Worker And Runtime Process Contract

Worker spawn remains a runtime operation unless a later spec explicitly moves worker pool warm-up into `VM::Init`.

Therefore:

- Worker init `Err(error)` remains an ordinary `Result::Err` returned by the spawning API.
- Runtime messaging errors remain ordinary `Result::Err` values unless they indicate a VM invariant violation.
- `Standby` is only valid for singleton Agent / singleton GenServer.

## Error Construction Rules

Compile phase:

- Use phase-specific compiler errors (`ParseError`, `ResolveError`, `TypeError`, `CodegenError`).
- Disallowed init messaging should preferably become a semantic compile error.

`VM::Init` phase:

- Init failure is a runtime error, not a user recoverable result.
- `Err(error)` from eager or standby singleton init becomes `RuntimeError::ProcessInitFailed`.
- Timeout while waiting for standby readiness becomes `RuntimeError::ProcessInitTimeout`.
- Messaging during init that escaped compile-time checks becomes a `RuntimeError` with phase `VM::Init`.

Runtime phase:

- User-level `Result::Err` remains a value.
- Final `Err(...)` from `surtr run` is rendered as a runtime value error.
- Optional stack trace rendering uses VM stack trace metadata, but normal result semantics do not change.

## Stack Trace Foundation

The implementation should use VM frame metadata as the canonical stack trace source.

Minimum data per trace frame:

- phase: `VM::Init` or `Runtime`
- function identity: `fun_idx` and best-effort qualified name
- call kind: direct function, closure function, builtin, template, process message, task
- caller location: source file, line, column, span
- optional process context: pid, process name, init policy, boot trigger

Direct calls and closure-to-function calls already carry call-site spans in `CallFrame`. Extend the frame with function identity and call kind.

For TCO, keep bounded breadcrumbs separate from live frames so optimized calls can still be diagnosed without restoring full frame growth.

For `VM::Init`, attach `process_name`, `init_policy`, and `trigger` (`boot`, `standby_retry`) to the trace snapshot.

## Runtime Path Changes To Plan

The current implementation has a semantic mismatch to fix:

- Eager boot converts init `Err` to `RuntimeError::ProcessInitFailed`.
- Current lazy materialization returns `Ok(Some(err_vm_result(err)))`.

Under this contract, standby init may not return `Err` to the caller. It must fail the `VM::Init` phase with `RuntimeError::ProcessInitFailed`.

Implementation should remove first-use materialization for singleton standby services. The VM init driver must run standby retries to completion before top-level runtime execution or REPL live execution begins.

## REPL Contract

REPL bootstrap and preload phases run `VM::Init` before entering live input.

If standby singleton init fails during REPL startup or preload, the REPL reports a `VM::Init` runtime diagnostic and does not enter the live prompt for that session state.

Live REPL input should not trigger singleton standby initialization. At live time, singleton services are either ready or the session failed to start.

## Documentation Updates Required Later

Update the normative specs after implementation direction is accepted:

- `docs/dev/ProcessRuntime_spec.md`
- `docs/dev/EldrVM_spec.md`
- `docs/dev/Xldr_spec.md`
- `docs/dev/Rune_observability.md`
- `doc/要件定義v9.md`

Replace `Lazy` wording with `Standby` wording and state explicitly that standby singleton init completes during `VM::Init`.

## Test Plan

Add or update tests for:

- `init_policy: Standby` parses and lowers where `Lazy` used to be accepted.
- `init_policy: Lazy` is rejected or accepted only through a staged compatibility diagnostic, depending on migration choice.
- Standby singleton returning `Pending` retries during VM init.
- Standby singleton returning `PendingAfter` respects init timeout.
- Standby singleton returning `Err(error)` produces `RuntimeError::ProcessInitFailed`.
- Runtime top-level code does not execute if standby init fails.
- Singleton messaging from `@init` is rejected by semantic checks.
- VM fallback rejects init-phase messaging if compiler checks miss it.
- Runtime singleton messaging sees ready services and returns only messaging-domain `Result::Err` values.
- Stack trace metadata for init failure includes phase, process name, init policy, and init call site.

Suggested focused commands:

- `cargo nextest run -p scar --tests`
- `cargo nextest run -p forge --tests`
- `cargo nextest run -p eldr --tests`
- `cargo nextest run -p xldr --tests`
- `cargo nextest run -p rune --test integration run_srt`
- `cargo nextest run --workspace`
