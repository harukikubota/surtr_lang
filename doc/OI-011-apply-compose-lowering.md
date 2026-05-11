# OI-011 apply / compose lowering optimization note

## Scope

This note records the current optimization direction for apply / compose-adjacent
bytecode lowering. The language surface stays unchanged. Optimizations should
prefer narrow Forge peepholes and VM opcodes that preserve the existing stack and
local-frame semantics.

## Current commits

- `StoreConstLocal { const_idx, local_idx }`
  - Replaces `LoadConst(const_idx); StoreLocal(local_idx)`.
  - Useful as a general lowering primitive, but did not appear in the sampled
    `result_helpers.srt` bytecode after standard-library lowering.
- `CopyLocal { src_local_idx, dst_local_idx }`
  - Replaces `LoadLocal(src_local_idx); StoreLocal(dst_local_idx)`.
  - In the sampled standard-heavy `tests/fixtures/script/pass/stdmod/result_helpers.srt`
    bytecode, this removed 689 adjacent load/store pairs.
- `EqLocalTag { local_idx, tag_const_idx }`
  - Replaces `LoadLocal(local_idx); GetTag; LoadConst(tag_const_idx); EqTag`.
  - In the same sample, this matched 10 sites. It is mostly a stepping stone for
    a later branch-fused tag test.

## Measurement

Sample command shape:

```bash
SURTR_RUN_CACHE=0 CARGO_TARGET_DIR=/private/tmp/surtr-target \
  cargo run --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml \
  -p rune -- dump /Users/haruca/work/rust/surtr/tests/fixtures/script/pass/stdmod/result_helpers.srt \
  --format json
```

Observed opcode counts for `result_helpers.srt`:

- Before additional local-copy compression: `14026`
- After `CopyLocal`: `13337`
- After `EqLocalTag`: `13307`

Fresh VM dump for the same source still shows runtime hotspots around:

- `LoadConst`
- `LoadLocal`
- `Pop`
- `Call` / `CallBuiltin` / `CallClosure`
- Result tag checks (`GetTag`, `EqTag`, `JumpIfFalse`)

## Apply / compose direction

Do not add a broad `Apply` or `Compose` VM opcode yet. Current lowering already
turns apply / pipe / compose surfaces into ordinary callable references, local
slots, closure calls, and generated wrapper functions. A broad opcode would mix
call semantics, capture semantics, and branch behavior too early.

Next steps, in order:

1. Zero-capture closure creation fusion, replacing
   `LoadFunctionRef(fun_idx); CaptureClosure(0)` when it appears.
2. Tail closure-call fusion only after checking interaction with existing VM
   tail-call frame reuse. TCO behavior may change, but if it becomes disruptive,
   disable the new fused path temporarily rather than changing surface behavior.

Avoid for now:

- New polymorphic apply / compose VM opcodes.
- Rewriting standard `.srt` definitions for optimization only.
- Optimizations that require changing user-visible callable behavior.

## Implemented in Result opcode batch

- `JumpIfLocalTagEq` / `JumpIfLocalTagNe` are now emitted from the centralized
  Forge jump layer when `EqLocalTag` is immediately consumed by `JumpIfTrue` /
  `JumpIfFalse`.
- `rune dump --format json --opcode-histogram` now exposes histogram and
  optimization-summary entries for both fused branch opcodes.
- `MakeOk` / `MakeErr` are available as Result constructor opcodes for the
  direct helper emission paths used by `assert`, `ensure`, and related Result
  builders.
