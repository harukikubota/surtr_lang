# Working specifications and plans

`doc/` は、未実装仕様、実装計画、draft、調査記録を置く。実装済みの恒久契約は
`../docs/dev/` または `../docs/site/`、標準 API の一次情報は `../lib/*.srt` の
`@doc` に置く。

## TypeCtorTrait / Monad / do / Generator

| 分類 | 文書 | 状態 |
|---|---|---|
| 仕様決定・実装待ち | [TypeCtorTrait carrier](./type_constructor_trait_extension_spec.md) | N01 の入力 |
| 仕様決定・実装待ち | [Identity / Reader / State](./monad_instances_spec.md) | N02 の入力 |
| 仕様決定・実装待ち | [MonadT 言語拡張](./monadt_language_extension_spec.md) | N03–N04 の入力 |
| 仕様決定・実装待ち | [標準 MonadT 型](./monadt_standard_types_spec.md) | N05 の入力。未確定 API は本文の確認ゲートに従う |
| 仕様決定・実装待ち | [SafeBind / diagnostics cleanup](./diagnostics_cleanup_spec.md) | N06 の入力 |
| 仕様決定・実装待ち | [`do` intrinsic](./do_intrinsic_spec.md) | N07–N11 の入力 |
| 仕様決定・実装待ち | [Generator](./generator_spec.md) | N12–N13 の入力。未確定 API は本文の確認ゲートに従う |
| implementation plan | [N01–N14 implementation plan](./type_constructor_monad_do_implementation_plan.md) | 作業順・受け入れ条件・進捗だけを管理 |
| draft | [signature-level type constructor inference](./signature_level_type_constructor_inference_draft.md) | 本文を現行化せず、そのまま保持 |

旧 Type Constructor Signature Unification の実装済み契約は
[`../docs/dev/Trait_system_spec.md`](../docs/dev/Trait_system_spec.md)、
[`../docs/dev/diagnostics.md`](../docs/dev/diagnostics.md)、および利用者向け Trait 文書へ移管済みである。
旧入力と実装根拠の対応は新しい implementation plan の移管マトリクスに残す。

## 扱い

- 未実装の仕様例を、現在利用できる構文や実行済み結果として利用者向け文書へ載せない。
- 実装完了時は担当する正本と `@doc` へ同期し、同じ契約を `doc/` と二重管理しない。
- draft の古い用語・リンクは、現行正本の参照監査から除外する。draft 本文を整理のために書き換えない。
- `optimize/`、LSP、project runner、SYP など、上表と独立した作業文書は今回の整理対象外である。
