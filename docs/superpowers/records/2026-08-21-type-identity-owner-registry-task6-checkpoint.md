# TypeIdentity OwnerRegistry — Task 6 checkpoint

作成日: 2026-08-21

`docs/superpowers/plans/2026-08-20-type-identity-owner-registry.md` の実装は、ユーザー指示により Task 6 完了時点で停止した。

## 完了済み

- Task 1: TypeIdentity の11分類、OwnerRegistry の一意性、compile-space 境界、診断・テスト方針を規範化した。
- Task 2: Sindr に identity/capability metadata を集約し、標準 `Option` を `TypeConstructor` として明示分類した。
- Task 3: Sigil に `OwnerRegistry` を追加し、source-order collision、process/module owner、session/checkpoint、trait promotion、診断 note/help を実装した。
- Task 4: resolver の symbol metadata を registry-backed にし、member identity を owner から導出した。Scope/import/UID の既存挙動は維持した。
- Task 5: Scar の compile-space `TypeKind::ConcreteError -> TypeIdentity::Error` mapping を揃えた。runtime/internal `TypeKind::ConcreteError` は維持した。
- Task 6: semantic analysis と Xldr の compile metadata 経路へ OwnerRegistry を伝播した。module/supervisor/sig/const/trait/type-constructor identity と owner-derived member identity を completion/info metadata に公開した。Xldr の runtime `:type` 表示は変更していない。

Task 1–6 の section commits:

- `c9a5377e`, `dcd6aac7` — documentation contract
- `8c60fd7f`, `b10c04d3` — Sindr metadata and Option
- `cb61c128`, `d98e3160` — OwnerRegistry and lifecycle completion
- `d7898cce`, `7c991b30` — resolver metadata propagation
- `d31e5a66` — Scar mapping
- `be915433`, `702096fe` — semantic analysis propagation

## 検証済み

- `cargo nextest run -p sindr` — 78 passed
- `cargo nextest run -p sigil --lib` — 217 passed
- `cargo nextest run -p scar` — 108 passed
- `cargo nextest run -p surtr-analysis` — 132 passed
- `cargo nextest run -p xldr -E 'binary(repl_core)'` — 133 passed
- `cargo nextest run -p surtr-lsp` — 17 passed
- `cargo check --workspace` — passed

## 現時点の未解決事項

1. Task 7（module fail fixtures と cross-kind collision regression）は未実装。
2. Task 8（format/static/full-workspace gate と最終仕様監査）は未実施。
3. `cargo nextest run --workspace` は 3 件失敗する。いずれも OwnerRegistry 導入後の新しい resolver diagnostic に対して既存 fixture が旧メッセージを期待しているためで、Task 7 の fixture 更新で解消する予定。
   - `duplicate_public_const`: `Duplicate public const: APP_NAME` を期待
   - `resolve_duplicate_top_level_type_name`: `Duplicate fully-qualified declaration: User` を期待
   - `type_name_conflict_across_modules`: `Duplicate fully-qualified declaration: User` を期待

## 再開時の次の手

Task 7 を実施し、上記3件を含む resolver/module fixture の期待値を `Duplicate top-level owner: <name>` に合わせる。その後 Task 8 の `cargo fmt --check`、`cargo check --workspace`、affected suites、`cargo nextest run --workspace`、最終 grep/audit を実行する。
