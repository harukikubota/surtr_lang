# Scar Src Cleanup Plan

## 目的

- `crates/scar/src` の型検査実装を、Surtr surface の挙動を変えずに読みやすくする。
- 今後の型周りの追加が薄い前提で、重複した状態受け渡しと legacy な分岐を減らす。
- Surtr source (`*.srt`) と仕様 fixture (`*.expected` / `*.error`) は変更しない。

## 作成日

- 2026-05-10 (Asia/Tokyo)

## 前提

- `doc/optimize/009_scar_surface_test_harness_cleanup.md` の方針を引き継ぐ。
- user-visible behavior は `tests/fixtures/script/**` と `tests/fixtures/modules/**` を正本にする。
- Scar crate 側に残す Rust tests は、typed IR / metadata / private invariant を直接見るものに寄せる。

## 実施内容

- `ScarSession` の永続状態を private `PersistentCheckerState` にまとめた。
  - `ScarCheckpoint` の serialized shape は維持した。
  - `typecheck_with_context` / `typecheck_staged_program_with_context` の state clone / restore を `PersistentCheckerState` 経由へ集約した。
- `Checker` 生成を `with_persistent_state` に寄せた。
  - child checker 作成時も同じ状態 object を使い、必要な一時 state だけ上書きする。
- `check_app` の builtin / function value call で重複していた positional argument validation を `typecheck_positional_call_args` に集約した。
  - named argument 拒否、arity mismatch、argument mismatch の message / hint は維持した。
- process intrinsic routing を `try_check_process_intrinsic_app` にまとめ、`check_app` の先頭分岐を薄くした。
- builtin special-form declaration contract を `SpecialFormBuiltinContract` に集約した。
  - expected qualified name、canonical signature、shape validator を同じ contract から参照する。
- `resolve_ast_ty_in_context` の単純 arity check を `require_type_arg_count` に寄せた。
  - `Result` / `MatchResult` / `TypeRef` / `Hole` / `Facet` の文脈制約は維持した。
- `resolve_signature_ast_ty_in_context` / `resolve_trait_signature_ast_ty_in_context` / `resolve_builtin_ast_ty_in_context` の shared branch を private `SignatureTyMode` + `resolve_signature_like_ast_ty_in_context` に集約した。
  - `Self` は trait signature のみ、`Lazy<T>` は builtin declaration のみ、`impl Trait` / `ErrorMarker` の既存挙動は維持した。
  - `List` / `HashMap` / `Generator` / `ProcessInit` / `Facet` / `PID` / `Workers` / `WorkerLease` / `TaskHandle` / `MatchResult` / `Result` / `Tuple` / `Func` / user-defined generic enum fallback の重複を畳んだ。
- `expr.rs` の Facet intrinsic 群に read-style / mutating-style の argument parser helper と `PreparedFacetInput` を追加した。
  - `Facet::view` / `preview` / `set` / `replace` / `over` / `over_result` で共通だった source/path 前処理を `prepare_facet_input` に寄せた。
  - `TypedInner::FacetView` / `FacetSet` / `FacetOver` の shape は変更しない。

## 変更しない範囲

- `lib/*.srt`
- `tests/fixtures/**/*.srt`
- `*.expected` / `*.error`
- `types_compatible` / `bind_tyvar` / `resolve_ty`
- trait specialization 本体

## 検証方針

```bash
cargo check -p scar
cargo nextest run -p scar
cargo nextest run -p rune --test integration run_srt::compile_error_fixtures_bucket
cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket
cargo nextest run --workspace
```

## 検証結果

- 2026-05-10 実施:
  - `cargo check -p scar`
  - `cargo nextest run -p scar`
  - `cargo nextest run -p rune --test integration run_srt::compile_error_fixtures_bucket`
  - `cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket`
  - `cargo nextest run --workspace`
- 結果:
  - `cargo nextest run -p scar`: 10 passed
  - `cargo nextest run -p rune --test integration run_srt::compile_error_fixtures_bucket`: 4 passed
  - `cargo nextest run -p rune --test integration run_srt::spec_fixtures_bucket`: 4 passed
  - `cargo nextest run --workspace`: 1064 passed

## 今回見送った整理

- `definitions.rs` の special-form contract table は validator を一箇所に寄せるところまでで止め、`const` table 化は次の純粋整理に回した。
- Facet intrinsic は argument parser / source-path resolver の切り出しまでに留め、`view` / `preview` / `set` / `replace` / `over` / `over_result` のさらに上の family-level builder までは導入していない。
