# Surtr Open Issues

> 目的: V9 正本でまだ固定していない将来課題を追跡する。
> 本ファイルは「未解決事項の台帳」であり、確定事項は `doc/要件定義v9.md` を正本とする。

最終更新日: 2026-04-14

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
- `.eldr` viewer 向け chunk 基盤
  - `Code`, `Cnst`, `Func`, `Type`, `ErrT`, `CInf`, `LblT`, `ImpT`, `ExpT`, `LitT`, `Line`, `SpnT`, `SrcP`, `PcSp`
- 標準モジュールの type 単位分割
  - `Bootstrap -> [Kernel, Numeric, Int, String, Boolean, Error, List, Result, Lens, Float] -> user`
  - cross-cutting builtin は `kernel.srt` へ置く
  - builtin type 宣言は各対応 `lib/*.srt` のトップレベルへ置く
- `List` 最小 surface の固定
  - 関数 surface は `defmod List` に置く
  - `ReduceStep` はトップレベル enum として維持する
  - `List::find*` / `any` / `all` の early-exit 基盤は `reduce_while` とする
- List runtime 前提の固定
  - cons cell + handle
  - `len` は handle に保持して O(1)
- FuncLiteral v1 の surface 固定
  - backtick infix (`expr \`name\` expr`, `expr \`operator\` expr`)
  - FuncLiteral は parser-only であり値にはならない
  - `Bind < Apply=Compose < Logical < Expr`
  - `Expr` クラスの `+`, `-`, `*`, `++` は同列・左結合

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
- `Bootstrap -> [Kernel, Numeric, Int, String, Boolean, Error, List, Result, Float] -> ユーザ拡張` のロード順
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
  - trait specialization や enum 導入後の依存ノード分割粒度
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
  - enum による条件付き循環や trait specialization が複雑化した段階で再度 reopen する
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

### OI-006 `defmod` の module path 導出を AST / lowering ベースに統一

- 策定コミット: `2026-04-09 review follow-up`
- 背景:
  - `rune` の `derive_primary_module_path` は今回 AST / lowering を優先する形へ寄せたが、`defmod A::B` を扱うために token 走査 fallback をまだ残している。
  - parser / lowering 側で qualified module path を正規化できれば、loader が token 仕様に依存しない単純な実装になる。
- 2026-04-09 時点の未解決点:
  - `spire` / `xldr` のどちらを module path 正本にするか
  - `defmod A::B` を AST 上でどう保持するか
- 受け入れ条件:
  - `derive_primary_module_path` が token 走査 fallback なしで動作する。
  - コメント、空行、字句仕様変更の影響を直接受けない。
- テスト方針:
  - `unit/spire` または `unit/xldr` に `defmod Kernel` / `defmod A::B` の module path 抽出テストを追加する。
  - `rune` の unit test で fallback を使わず同じ結果になることを確認する。

### OI-007 Rune / Xldr の CLI エラー契約統一

- 策定コミット: `2026-04-09 review follow-up`
- 背景:
  - `rune` 側は `RuneError` へ統一したが、`xldr::cli_command` / `xldr::tui::run_command` は引き続き `i32` ベースの返却をしている。
  - 現状は `rune` 境界で `RuneError::Message` に包んでいるため、REPL / TUI 系の診断契約が phase error と同じレイヤで表現されていない。
- 2026-04-09 時点の未解決点:
  - `xldr` 側に共通エラー型を持たせるか、`rune` 側 adapter を正式 API とみなすか
  - interactive command の usage / message / diagnostic をどこまで typed に扱うか
- 受け入れ条件:
  - CLI / REPL / TUI の失敗経路が同じエラー契約で扱える。
  - exit code と stderr 出力責務の境界が crate 間で明文化される。
- テスト方針:
  - `rune` integration で CLI / REPL / TUI 起点の失敗時に期待する exit code とメッセージが維持されることを確認する。
  - `unit/xldr` または `unit/rune` で error adapter の変換テストを追加する。

### OI-008 `TypeRegistry::lookup` の参照コスト最適化

- 策定コミット: `2026-04-09 review follow-up`
- 背景:
  - `TypeRegistry::lookup` はまだ `entries.iter().find(...)` の線形探索で、`Value::to_display_string` のような表示系で繰り返し呼ばれる。
  - 現行規模では許容できるが、型数やネストした値が増えると表示コストが読みやすくない形で増える。
- 2026-04-09 時点の未解決点:
  - `HashMap<u32, usize>` を併設するか、tag と配列位置を一致させる設計に寄せるか
  - 決定性と serialize 形に影響を出さずに index を持つ方法
- 受け入れ条件:
  - `lookup` が O(1) 相当で引ける。
  - 現行の tag 決定性と表示結果が変わらない。
- テスト方針:
  - 既存 display 系テストを維持したまま内部 index 導入後も同じ表示になることを確認する。
  - `unit/sindr` に tag lookup の整合テストを追加する。

### OI-009 List runtime 表現の簡素化

- 策定コミット: `2026-04-09 review follow-up`
- 背景:
  - runtime 前提としての `cons cell + handle` と `len` の O(1) 契約は固定済み。
  - そのうえで Rust 実装上の `ListNode` は単一バリアント enum のままで、`tail_handle()` も非空前提の箇所で `saturating_sub` を使っている。
  - 挙動上の不具合ではないが、runtime の意図がコードから読み取りにくい。
- 2026-04-09 時点の未解決点:
  - `ListNode` を struct に寄せるか、将来空ノード等の variant 拡張余地を残すか
  - `ListHandle` の不変条件を型とコメントのどちらで表現するか
- 受け入れ条件:
  - 非空リストの不変条件がコード上で明確になる。
  - `head_value()` / `tail_handle()` の既存挙動と API は維持される。
- テスト方針:
  - 空リスト / 1 要素 / 複数要素で `head_value()` / `tail_handle()` の結果が変わらないことを `unit/sindr` で確認する。
  - 必要なら `len` 更新の不変条件テストを追加する。

### OI-010 `RichError` 表示形式の仕様化

- 策定コミット: `2026-04-09 review follow-up`
- 背景:
  - `RichError::to_display_string` は message を `{:?}` で表示しており、現在は quoted form が仕様として固定されている。
  - review では `{}` 表示、もしくは意図をコメントで明示する案が出たが、今回は挙動変更を見送った。
- 2026-04-09 時点の未解決点:
  - user-facing error と debug-like display のどちらを正本にするか
  - quoted form を保つ場合、`doc/EldrVM_spec.md` か rustdoc のどちらで説明するか
- 受け入れ条件:
  - `RichError` の表示が仕様として文書化される。
  - quoted / unquoted のどちらを採るかがテストとドキュメントで一致する。
- テスト方針:
  - `unit/sindr` に `display_for_rich_error_*` 系テストを固定し、採用形式が将来ぶれないようにする。

### OI-011 apply / compose lowering 最適化方針

- 策定コミット: `12b4406`
- 背景:
  - `|>`, `|*>`, `|>=`, `>>`, `|=>` の外部契約自体は確定した。
  - 一方で lowering は現時点では正しさ優先で、compose 系は synthetic callable を使う経路を残している。
  - 仕様として必要なのは「何が書けるか」と「どの値が返るか」であり、即時インライン化や branch chain 平坦化の閾値は未確定。
- 2026-04-10 時点の固定事項:
  - compose の外部契約は capture / closure 限定
  - apply 系は第一引数注入
  - `cons`, `first`, `len` と `List::reverse / reduce / reduce_while / map / filter / find / find_map / any / all` を公開 surface とする
- 未確定点:
  - 即時適用される compose 式をどこまで branch chain へ直下ろしするか
  - synthetic callable と直接 lower の選択基準
  - 将来の最適化が debug 性能や span 診断へ与える影響
- 受け入れ条件:
  - 最適化前後で外部挙動とエラースパンが変わらない。
  - codegen / VM の複雑化が段階責務を壊さない。
- テスト方針:
  - 既存の spec / compile_errors / integration を回帰基準にする。
  - 追加最適化を入れる場合は opcode 列または IR 選択が変わるケースを unit で固定する。

### OI-012 `.eldr` viewer follow-up

- 策定コミット: `2026-04-10 .eldr viewer chunk baseline`
- 背景:
  - `.eldr` には viewer 向け chunk 基盤を導入済みだが、source id を持たない span や最小限の import/literal metadata など、将来拡張を前提にした部分が残っている。
  - VSCode 拡張と TUI viewer はこの baseline を消費するが、より深い debug 表示は未実装である。
- 2026-04-10 時点の後続項目:
  - `Dbgi` を追加する
  - `LocT` を追加する
  - span に source id を持たせる設計を検討する
  - `ImpT` の import 粒度を module import / external import まで拡張する
  - `LitT` に use-site / reverse index を追加する
  - VSCode 拡張 / TUI 側で `Func` / `LblT` / `ImpT` / `ExpT` / `LitT` / `PcSp` を消費する viewer を実装する
- 受け入れ条件:
  - viewer が `.eldr` 単体で関数ビュー / ラベルビュー / import/export ビュー / literal ビュー / source compare を提供できる
  - `Dbgi` / `LocT` 導入後も現行 chunk 契約と後方整合する
- テスト方針:
  - `unit/sindr` で chunk 整合性と参照先の妥当性を固定する
  - `integration` で `surtr dump` 出力が viewer の期待するテーブルを欠かさないことを維持する

### OI-013 Tail Call Optimization の適用範囲と観測導線

- 策定コミット: `codex/tail-call-optimization`
- 背景:
  - 現在の TCO は user-function の tail-position call に対して、bytecode 上で「次 opcode が `Return`」と判定できる場合に current frame を再利用する v1 実装である。
  - `fib_tail(50)` と `reduce` ワークロードでは frame depth と return count の削減を確認済みだが、CLI 観測・viewer・bytecode 表現にはまだ最小限しか反映していない。
- 2026-04-11 時点の固定事項:
  - top-level call は再利用しない
  - builtin target の `CallClosure` は再利用しない
  - tail-position 判定は `forge` の終端生成と VM の `next opcode == Return` 判定に依存する
  - 観測値として `tail_calls_optimized` は追加済み
- 未確定点:
  - `surtr run` / `surtr dump` に `tail_calls_optimized` をどの形で露出するか
  - bytecode / viewer に tail-position call を明示する marker を追加するか
  - top-level trampoline や loop lowering まで進めるか
  - 将来の追加最適化で span 診断と call trace をどう維持するか
- 受け入れ条件:
  - TCO の適用範囲が public docs と canonical docs の両方でぶれずに説明される。
  - 観測導線から「最適化されたか」「されていないか」が利用者に分かる。
  - 追加最適化を入れても既存の実行意味と診断位置が変わらない。
- テスト方針:
  - tail recursion / mutual recursion / non-tail recursion の観測テストを回帰基準にする。
  - CLI や dump へ露出を足す場合は integration で stderr / JSON 形状を固定する。
  - bytecode 表現を増やす場合は `unit/sindr` と viewer schema テストを更新する。

### OI-014 private value 持ち出し warning 方針

- 策定コミット: `42fd699`（Lens / private capability 境界固定）
- 背景:
  - 現行仕様では `User.password`（type-root capability）は禁止し、`user.password`（value access）は許可している。
  - また closure 内 private access（`{|| user.password}`）は scope 外 escape 防止のため禁止した。
  - 一方で `return user.password` は「値の持ち出し」として現行仕様上は許可しており、warning を出すかどうかは未確定である。
- 未確定点:
  - warning を導入するか（導入する場合の default severity）
  - warning message に「ユーザ責任の持ち出し」であることを明示するか
  - lint 体系（on/off / warning code / CI fail-on-warning）とどう接続するか
- 受け入れ条件:
  - warning 導入有無が `doc/要件定義v9.md` と `doc/テスト方針.md` で一貫する。
  - warning を導入する場合、`return user.password` は compile error にしない。
- テスト方針:
  - warning 導入時は `integration` で diagnostics（human/json）の warning 出力を固定する。
  - warning 非導入の場合は現行どおり `spec/modules/private_visibility_function_return_private_value` の成功を維持する。

### OI-015 test DSL I/O capture の `it` 単位分離

- 策定コミット: `2026-04-14 capture API 導入`
- 背景:
  - `Test::capture_stdout` / `Test::capture_stderr` を追加し、Surtr test script から `print` / `eprint` 出力を直接アサート可能にした。
  - 現行実装は per-VM バッファ + drain cursor 方式のため、同一 script VM 内で `it` 間の未読出力が次の `capture_*` に流入しうる。
- 2026-04-14 時点の固定事項:
  - Rust 側の並列実行（別 process / 別 VM）ではバッファは分離される。
  - 直近運用は `it` 内で `print` と `capture_*` を 1 対 1 で完結させる前提とする。
- 未確定点:
  - `it` 開始/終了に合わせた自動 cursor reset を入れるか
  - `test` / `describe` / `it` のどの粒度で capture scope を切るか
  - `capture_*` を drain API のまま維持するか、peek API を追加するか
- 受け入れ条件:
  - `it` 単位で deterministic に capture でき、隣接 `it` の未読出力が混入しない。
  - 現行の `capture_*` 利用コードに対して後方互換方針（移行手順）が明示される。
- テスト方針:
  - `tests/integration/test_command.rs` に「前の `it` が出した未読出力を次の `it` が拾わない」ケースを追加する。
  - `lib/tests/*.srt` に capture API の推奨パターン fixture を追加して契約を固定する。

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
- FuncLiteral の未実装部分
  - `&` 側の operator capture (`&`+``)
  - placeholder capture (`&1`, `&2`, ...)
  - qualified backtick path (`\`Type::method\``)
  - これらは v1 範囲外のため、仕様を詰めてから reopen する
- OOM / host failure policy
  - 上限値、停止文言、回復可否は host 依存方針のまま、詳細契約は将来確定する
- Enum conversion helper
  - `defenum` 本体（variant payload / discriminant / 条件付き循環）は正本 (`doc/要件定義v9.md`) と `docs/site/*` へ反映済み
  - `.idx` アクセッサは廃止済み（Enum への field access は禁止）
  - `doc/Enum.md` は廃止済み（内容は上記ドキュメントへ統合）
  - `Enum::from(Int)` / `Enum::try_from(Int)` の自動生成は未対応のため、仕様確定後に reopen する

---

## 5. 更新ルール

- Open Issue をクローズしたら、本ファイルから削除せず `Status: Closed` に変更し、解決コミットを追記する。
- 新規 Issue 追加時は、最低限 `策定コミット` と `テスト方針` を埋める。
- 実装先行で仕様が変わる場合は、先に本ファイルと `要件定義v9.md` の整合を取ってからコード変更する。
