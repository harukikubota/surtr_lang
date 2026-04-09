# Surtr Open Issues

> 目的: V9 正本でまだ固定していない将来課題を追跡する。
> 本ファイルは「未解決事項の台帳」であり、確定事項は `doc/要件定義v9.md` を正本とする。

最終更新日: 2026-04-09

2026-04-09 整理メモ:

- `BigInt` 採用、runtime 内部 ID 分離、`Float` 切り出しは open issue ではなく、`doc/要件定義v9.md` と `/Users/haruca/work/rust/surtr/作業フロー.md` 側で追跡する
- `@@doc`, `.eldr` の `Docs` chunk, 標準モジュール分割, `@@builtin type` 契約は今回の baseline で確定したため、本ファイルでは追跡しない
- 本ファイルは引き続き「宣言収集 / fixpoint / 循環依存 / マクロ段階 / 将来 UX」の未解決論点に限定する

---

## 1. 今回クローズ済み

以下は 2026-04-09 時点で open issue ではなくなった事項。

- `type` の予約語化
- `@@builtin type` の surface syntax 受理
- builtin type canonical head の固定
  - `Int`
  - `Float`
  - `String`
  - `Boolean`
  - `Unit`
  - `Error`
  - `List<$A>`
  - `Result<$T>`
- `@@doc """..."""` の導入
- `.eldr` への `Docs` chunk 追加
- 標準モジュールの type 単位分割
  - `Bootstrap -> [Kernel, Int, String, Boolean, Error, List, Result, Float] -> user`
  - cross-cutting builtin は `kernel.srt` へ置く
  - builtin type 宣言は各対応 `lib/*.srt` のトップレベルへ置く

これらの正本は `doc/要件定義v9.md`, `doc/EldrVM_spec.md`, `doc/Xldr_spec.md`, `doc/テスト方針.md` を参照する。

---

## 2. 策定トレーサビリティ（コミット基準）

前方参照ポリシー固定に関連する基盤コミット:

- `dd528e0`: sigil で `defstruct` / `defrecord` / `deferror` を事前登録対象へ拡張
- `303c5d5`: sigil 側の前方参照回帰テストを追加
- `9b2f28c`: scar/env に型定義の predeclare API と状態管理を追加
- `9262da8`: scar/checker に型/コンストラクタのシグネチャ先行登録を導入
- `4ee735e`: spec/integration に前方参照成功ケースを追加
- `aa7cd71`: compile_errors に最終未解決ケースを追加
- `efcd4ee`: unique_id / tag 決定性の回帰テストを追加

上記を「前方参照ポリシーの基準点」とし、以下の Open Issue はこの基準点以降の設計課題として扱う。

---

## 3. Open Issues

補足:

- `Bootstrap` / `Kernel` 分離
- `Bootstrap -> [Kernel + 他標準モジュール] -> ユーザ拡張` のロード順
- `Bootstrap` / `Kernel` の auto import と明示 import 禁止
- `@@builtin` は `SourceKind::StdModule` のみ許可

上記は 2026-04-05 時点で確定事項となったため、本ファイルでは追跡しない。正本は `doc/要件定義v9.md`、テスト観点は `doc/テスト方針.md` を参照する。

### OI-001 宣言インデックス先行収集（段階的ファイルリード）

- 策定コミット: `dd528e0`, `9262da8`
- 背景:
  - 現在は単一入力を順次処理しつつ、トップレベル事前登録で前方参照を解決している。
  - 並列コンパイルでは、ファイル本体解析前に「宣言のみ」を安価に収集する段階が必要。
- 2026-04-09 時点の固定事項:
  - 初期収集情報は `name / kind / signature / span / module path / dependency-type list` とする
  - 対象は top-level `def` header と型系定義（`defstruct` / `defrecord` / `deferror` / `@@builtin type` を含む）とする
- 受け入れ条件:
  - 宣言収集フェーズ単体で、依存解決に必要な情報を欠落なく抽出できる。
  - 本体解析前でも `unique_id` / `tag` の割り当て順序を決定できる。
- テスト方針:
  - `unit/spire` または `unit/sigil` に宣言抽出テストを追加（本体を読まなくても index が取れること）。
  - `integration` で複数ファイル入力順を入れ替えても index が一致することを検証。

### OI-002 依存グラフ + 再試行キュー

- 策定コミット: `9262da8`, `aa7cd71`
- 背景:
  - 現在の「Pending -> 後段で解決」方針はあるが、複数単位の依存関係を明示的には保持していない。
- 2026-04-09 時点の固定事項:
  - 粒度は定義単位（`defmod`, `impl T`, `struct`, `Error`, `type`, `def`）を基本とする
  - 解決順は「宣言・依存型収集 -> macro slot(no-op) -> 関数/本体チェック」を基本線とする
- 未確定点:
  - macro 導入後に queue 優先度をどう調整するか
  - `impl Trait` や enum 導入後の依存ノード分割粒度
- 受け入れ条件:
  - 依存が解決したノードのみを再評価できる。
  - 無関係ノードの再評価を抑制し、総コンパイルコストが悪化しない。
- テスト方針:
  - `unit/sigil` / `unit/scar` で「解決イベント発生時に依存ノードだけ再試行」するケースを検証。
  - `integration` で依存が深い入力と浅い入力を混在させ、不要再試行が起きないことを確認。

### OI-003 fixpoint 終了条件の厳密化（新規解決ゼロ）

- 策定コミット: `aa7cd71`
- 背景:
  - 仕様上は「fixpoint 到達時に Pending 残存ならエラー」だが、実装レベルの終了判定が未標準化。
- 2026-04-09 時点の固定事項:
  - 進捗定義は Pending 集合の減少とする
- 未確定点:
  - 進捗ゼロ時の診断集約ルール
- 受け入れ条件:
  - 有限ステップで必ず停止し、停止理由（成功/fixpoint失敗）が説明可能。
  - 同一入力に対して常に同じ失敗集合を返す。
- テスト方針:
  - `compile_errors` に fixpoint 失敗専用ケースを追加し、`phase + contains` で固定。
  - 同一入力の複数回実行でエラー件数と主メッセージが一致することを `integration` で検証。

### OI-004 循環依存ポリシー

- 策定コミット: `aa7cd71`, `efcd4ee`
- 背景:
  - 前方参照は許可したが、循環依存（関数循環、型循環、混合循環）の許容範囲は未確定。
- 2026-04-09 時点の固定事項:
  - 現 phase の struct / record / error / type 相当の型循環は一律禁止
  - enum による条件付き循環や `impl Trait` が入る段階で再度 reopen する
- 未確定点:
  - 関数循環や将来 enum 導入後の許可境界
  - エラー時の責務点（cycle の最小閉路表示）
- 受け入れ条件:
  - 許可/禁止が構文カテゴリごとに明文化される。
  - 禁止ケースで決定的な診断（最小 cycle 表示）が得られる。
- テスト方針:
  - `spec` に許可循環ケース（成立するもの）を追加。
  - `compile_errors` に禁止循環ケース（関数/型/混合）を追加し、文言断片を固定。

### OI-005 マクロ展開段階と通常解決段階の分離

- 策定コミット: `dd528e0`（前方参照基盤導入時の拡張課題として明示）
- 背景:
  - 将来のマクロ導入時、展開前後で宣言集合が変化すると解決順と決定性に影響する。
- 未確定点:
  - マクロ展開をいつ行うか（宣言収集前/後/段階的）
  - 展開生成物への `unique_id` / `tag` 割り当て規則
- 受け入れ条件:
  - 展開の有無で同値プログラムの解決結果が一貫する。
  - 展開段階と通常解決段階の責務境界が文書化される。
- テスト方針:
  - マクロ導入フェーズで `unit/spire|sigil` に段階境界テストを追加。
  - 展開後 IR の決定性比較テスト（同一入力で同一 ID/tag）を必須化。

---

## 4. Deferred Topics

以下は今回の実装対象から外したが、正本からも落としたくない将来課題。

- Project runner
  - source 操作 API、runner DSL、init command を別途 reopen する
  - compile unit / source rules との責務分離を保ったまま仕様化する
- REPL command 拡張
  - `:type`, `:browse`, file ingest UX、補完改善を Xldr 将来課題として扱う
  - 今回入れた `:doc` の先に、どこまで interactive browsing を広げるかを整理する
- closure の `expected=None` 推論強化
  - 期待型や注釈なしでも強く推論する方向は別 issue で扱う
  - let-generalization を入れない current baseline を前提に reopen する
- OOM / host failure policy
  - 上限値、停止文言、回復可否は host 依存方針のまま、詳細契約は将来確定する
- Enum
  - `doc/Enum.md` のメモを起点に reopen する
  - 条件付き循環、variant payload、tag 戦略と合わせて設計する

---

## 5. 更新ルール

- Open Issue をクローズしたら、本ファイルから削除せず `Status: Closed` に変更し、解決コミットを追記する。
- 新規 Issue 追加時は、最低限 `策定コミット` と `テスト方針` を埋める。
- 実装先行で仕様が変わる場合は、先に本ファイルと `要件定義v9.md` の整合を取ってからコード変更する。
