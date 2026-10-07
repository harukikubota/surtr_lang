# Empty Struct Default Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 明示的な `new` 必須契約を維持したまま、0 フィールド `defstruct` に `@derive Default` を適用できることを仕様・実装・テストで固定する。

**Architecture:** 空構造体の型登録・tag・構造体リテラル生成は既存経路を利用する。変更対象は、derive expansion が生成する空構造体リテラルを実際の checker が受理することの回帰保証と、仕様文書の契約明文化に限定する。

**Tech Stack:** Rust, Cargo nextest, Surtr script fixtures, Markdown specification.

**Spec:** `doc/要件定義v9.md` and `docs/dev/Trait_system_spec.md`

## Global Constraints

- フィールド数に関係なく user-defined `Struct` の inherent `new` は必須。
- `@derive Default` は constructor surface を経由せず、struct literal で `Self` を生成する。
- 0 フィールド struct literal は `StructNew { field_count: 0 }` として runtime に保持する。
- 仕様変更時は `doc/要件定義v9.md` と該当する `docs/dev/` 正本を同期する。

### Task 1: Add regression coverage

**Files:**
- Modify: `crates/scar/tests/typecheck_surface.rs`
- Create: `tests/fixtures/script/pass/derive/empty_struct_default.srt`
- Create: `tests/fixtures/script/pass/derive/empty_struct_default.expected`

**Interfaces:**
- Consumes: existing `resolve_with_builtin_prelude`, `typecheck`, and script fixture runner.
- Produces: tests proving `@derive Default` accepts an empty struct while a missing inherent `new` still fails.

- [x] **Step 1: Write the regression test**

Add a typechecker test that defines `@derive Default defstruct Empty {}` and an explicit `impl Empty { def new() -> Self { Empty {} } }`, then checks `Default::default::<Empty>()` and `Empty()` typecheck. Add a separate assertion that `defstruct Empty {}` without `new` still reports the existing new-contract error.

- [x] **Step 2: Run the focused test to verify the current behavior**

Run: `cargo nextest run -p scar --test typecheck_surface empty_struct_default`

Observed: both cases passed immediately, confirming the implementation already supports the requested behavior and the test should remain a contract regression test.

- [x] **Step 3: Add the script fixture and expected output**

Use:

```srt
@derive Default
defstruct Empty {}

impl Empty {
  def new() -> Self { Empty {} }
}

print(inspect(Default::default::<Empty>() == Empty()))
```

Expected output is `True`.

- [x] **Step 4: Run the focused fixture test**

Run: `cargo nextest run -p rune --test integration run_srt`

Expected: the fixture passes once the implementation is correct.

### Task 2: Confirm no production change is required

**Files:**
- Modify: the smallest resolver/checker file identified by the failing test, only if the test exposes a real implementation gap.
- Test: `crates/scar/tests/typecheck_surface.rs`

**Interfaces:**
- Consumes: empty `StructDef`, generated `Default::default` struct literal, and existing inherent `new` contract.
- Produces: successful typechecking/code generation for `@derive Default` on a 0-field struct without weakening the `new` requirement.

- [x] **Step 1: Run the focused typechecker test and inspect the result**

Run: `cargo nextest run -p scar --test typecheck_surface empty_struct_default`

- [x] **Step 2: Confirm the production path needs no patch**

`ensure_struct_impl_new_contract`, derive expansion, empty struct literal checking, and `StructNew { field_count: 0 }` already satisfy the contract. No production code change is needed; do not add automatic `new` generation.

- [x] **Step 3: Run the focused typechecker test**

Run: `cargo nextest run -p scar --test typecheck_surface empty_struct_default`

Expected: PASS, including the negative no-`new` assertion.

- [x] **Step 4: Run the focused integration fixture**

Run: `cargo nextest run -p rune --test integration run_srt`

Expected: PASS with output `True` for `empty_struct_default`.

### Task 3: Update normative documentation

**Files:**
- Modify: `doc/要件定義v9.md`
- Modify: `docs/dev/Trait_system_spec.md`
- Modify: `docs/dev/テスト方針.md`

**Interfaces:**
- Consumes: the tested language behavior.
- Produces: explicit documentation that 0-field structs are valid, `new` remains mandatory for every user struct, and `@derive Default` can construct an empty struct literal.

- [x] **Step 1: Clarify the language contract**

State that `defstruct Name {}` is valid, but it does not waive the inherent `new` requirement. State that `@derive Default` is valid for the 0-field product and generates an empty struct literal; it remains independent of and does not synthesize `new`.

- [x] **Step 2: Document constructor decoupling**

State that `Name()` resolves through the existing `new` signature and therefore remains decoupled from field count and Default derivation.

- [x] **Step 3: Update testing guidance**

Add the empty-struct Default positive case and the no-`new` negative case to the relevant test matrix.

### Task 4: Full verification

- [x] **Step 1: Run workspace tests**

Run: `cargo nextest run --workspace`

- Note: the workspace run hit the repository's existing 15-second nextest timeout on `language_features_bucket_5`; the same test passed with `cargo test` in 16.92s (and again in 11.54s).

- [x] **Step 2: Run the required Rune integration suites**

Run: `cargo nextest run -p rune --test integration run_srt`

Run: `cargo nextest run -p rune --test integration module_import_fixtures`

- [x] **Step 3: Inspect the diff**

Run: `git diff --check` and `git diff --stat`.
