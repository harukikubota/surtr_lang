# Working specifications and plans

`doc/` は、未実装仕様、実装計画、draft、調査記録を置く。実装済み部分と後続Taskの入力を
同じ文書が持つ場合は、実装済み契約の移管先と残る入力境界を本文に明記して後続完了まで保持する。
実装済みの恒久契約は `../docs/dev/` または `../docs/site/`、標準 API の一次情報は
`../lib/*.srt` の `@doc` に置く。

## TypeCtorTrait / Monad / do / Generator

| 分類 | 文書 | 状態 |
|---|---|---|
| 実装済み API | [Identity](../docs/site/identity.md) / [Reader](../docs/site/reader.md) / [State](../docs/site/state.md) | N02 の利用者向け文書 |
| 実装済み仕様・後続入力 | [MonadT 言語拡張](./monadt_language_extension_spec.md) | N03–N04の恒久契約は担当文書へ移管済み。未実装のMT-L22だけをN11へ残し、generic `defrecord`は`open-issues.md` OI-035へ分離 |
| 実装済み標準 API・後続入力 | [標準 MonadT 型](./monadt_standard_types_spec.md) | N05の正本は`../lib/traits/monad_t.srt`と`../lib/types/monad_transformer/`、利用者向け説明は[`../docs/site/monad-transformers.md`](../docs/site/monad-transformers.md)。未実装のMT-S14だけをN11へ残す |
| 訂正提案・着手前必須 | [SafeBind total pattern / RHS分類](./safebind_total_pattern_rhs_correction_proposal.md) | N06開始前にSafeBind・doの各入力へ反映する。total non-Result二診断とpartial pass-throughの境界を定める |
| 仕様決定・実装待ち | [SafeBind / diagnostics cleanup](./diagnostics_cleanup_spec.md) | N06 の入力 |
| 仕様決定・実装待ち | [`do` intrinsic](./do_intrinsic_spec.md) | N07–N11 の入力 |
| 仕様決定・実装待ち | [Generator](./generator_spec.md) | N12–N13 の入力。未確定 API は本文の確認ゲートに従う |
| implementation plan | [N01–N14 implementation plan](./type_constructor_monad_do_implementation_plan.md) | 作業順・受け入れ条件・進捗だけを管理 |
| 独立draft・採用未決 | [Extractor更改](./extractor_revision_draft.md) | 現行はOption返却。N06–N14と相互依存なしの別タスク |
| draft | [signature-level type constructor inference](./signature_level_type_constructor_inference_draft.md) | 本文を現行化せず、そのまま保持 |

旧 Type Constructor Signature Unification の実装済み契約は
[`../docs/dev/Trait_system_spec.md`](../docs/dev/Trait_system_spec.md)、
[`../docs/dev/diagnostics.md`](../docs/dev/diagnostics.md)、および利用者向け Trait 文書へ移管済みである。
N01 の direct carrier 同一性契約と N02 の Identity / Reader / State 契約も担当する正本へ
移管済みであり、実装入力は削除した。
旧入力と実装根拠の対応は新しい implementation plan の移管マトリクスに残す。

## 扱い

- 未実装の仕様例を、現在利用できる構文や実行済み結果として利用者向け文書へ載せない。
- 実装完了時は担当する正本と `@doc` へ同期し、同じ契約を `doc/` と二重管理しない。
- draft の古い用語・リンクは、現行正本の参照監査から除外する。draft 本文を整理のために書き換えない。
- `optimize/`、LSP、project runner、SYP など、上表と独立した作業文書は今回の整理対象外である。
