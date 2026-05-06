# Surtr Open Issues

> 目的: V9 正本でまだ固定していない未解決事項だけを追跡する。
> 本ファイルは「未解決事項の台帳」であり、確定事項は `doc/要件定義v9.md`、開発者向け spec は `docs/dev/` 配下を正本とする。`doc/` は draft / input / tmp 置き場として扱う。

最終更新日: 2026-05-06

---

## Open Issues

### OI-000 `const` の将来拡張境界

- 背景:
  - V1 の `const` は top-level / literal / lens path alias に限定して導入した。
- 未確定点:
  - compile-time evaluable な純粋式まで広げるか
  - associated consts を trait / impl に導入するか
  - local / `defmod` / `impl` scope const を許可するか
- 受け入れ条件:
  - 追加する場合も runtime transport 制約と名前解決規則が崩れない。
  - 現行 V1 の global const namespace と互換性を保てる。

### OI-001 宣言インデックス収集の責務境界

- 背景:
  - `sigil::precollect_declaration_index` と staged resolve は既に存在し、前方参照の基盤として動作している。
  - 一方で、将来の複数ファイル並列処理に必要な「宣言だけを安価に収集する段階」の責務境界は、まだ正本で固定していない。
- 未確定点:
  - 宣言収集で保持すべき最小情報をどこまで広げるか
  - `unique_id` / `tag` の決定順を declaration index 単体でどこまで固定するか
- 受け入れ条件:
  - 本体解析前でも依存解決と決定性維持に必要な情報が欠けない。
  - declaration index の責務と、通常 resolve / typecheck の責務が文書上で分離される。
- テスト方針:
  - `unit/sigil` で declaration index が本体解決なしに安定して取れることを固定する。
  - `integration` で入力順を入れ替えても index と後続の決定結果が変わらないことを確認する。

### OI-002 依存グラフと再試行キューの明文化

- 背景:
  - 現在は staged resolve / predeclare により段階的に解決できているが、依存ノードと再試行条件の public contract はまだない。
  - 将来の macro、specialization、複数 compile unit 導入時に、再評価粒度が曖昧なままだと性能と決定性の説明が崩れる。
- 未確定点:
  - 再試行単位を定義単位のまま維持するか、より細かく分割するか
  - macro や追加 type feature 導入後の queue 優先順位をどう扱うか
- 受け入れ条件:
  - 依存解決イベント発生時に、無関係ノードを再評価しない方針が説明できる。
  - staged compile の進行順が docs と実装で矛盾しない。
- テスト方針:
  - `unit/sigil` / `unit/scar` で依存解決後に必要ノードだけ再試行されるケースを固定する。
  - `integration` で依存の深さが異なる入力を混在させても不要再試行が増えないことを確認する。

### OI-003 fixpoint 終了条件と診断集約

- 背景:
  - fixpoint failure の考え方自体はあるが、進捗ゼロ判定時にどの失敗集合を返すかはまだ仕様として固まっていない。
  - staged resolve / typecheck が増えるほど、停止理由の説明責務が重くなる。
- 未確定点:
  - 進捗定義を Pending 集合の減少だけで十分とみなすか
  - 進捗ゼロ時に複数の失敗候補をどう集約して表示するか
- 受け入れ条件:
  - 同一入力に対して fixpoint failure の件数と主診断が決定的である。
  - 成功 / fixpoint failure の停止理由を利用者に説明できる。
- テスト方針:
  - `compile_errors` に fixpoint failure 専用ケースを置き、`phase` と主文言を固定する。
  - `integration` で同一入力の複数回実行結果がぶれないことを確認する。

### OI-004 循環依存ポリシーの許可境界

- 背景:
  - `defenum` は実装済みで、型循環検出も `scar` 側に存在する。
  - ただし、どの循環を禁止し、どの循環を将来許可しうるかの境界はまだ正本で狭くしか定義していない。
- 未確定点:
  - 関数循環、型循環、混合循環のうち将来許可する余地がある範囲
  - 禁止時にどこまで最小 cycle 表示を責務に含めるか
- 受け入れ条件:
  - 構文カテゴリごとの許可 / 禁止境界が明文化される。
  - 禁止ケースで決定的な cycle 診断が返る。
- テスト方針:
  - `spec` に許可する循環ケースを追加する場合はその成立条件を固定する。
  - `compile_errors` に禁止循環ケースを置き、cycle 表示の主文言を固定する。

### OI-005 マクロ展開段階と通常解決段階の分離

- 背景:
  - 現行 baseline では macro 段階は実質 no-op 前提だが、宣言収集と通常解決の間に macro slot を置ける構成になっている。
  - 将来 macro を入れると、宣言集合・ID 割り当て・依存解決順に直接影響する。
- 未確定点:
  - macro 展開を declaration index 前後のどこに置くか
  - 展開生成物の `unique_id` / `tag` の決定規則をどうするか
- 受け入れ条件:
  - macro あり / なしで同値プログラムの解決結果が一貫する。
  - macro 段階と通常 resolve 段階の責務境界が文書化される。
- テスト方針:
  - macro 導入時に `unit/spire` / `unit/sigil` で段階境界テストを追加する。
  - 展開後 IR の決定性比較を回帰基準にする。

### OI-006 `defmod` の module path 導出正本

- 背景:
  - `xldr::loader::derive_primary_module_path` は AST / lowering 優先になっているが、qualified head（`defmod A::B`, `namespace A { defmod B { ... } }`）を扱う token 走査 fallback がまだ残っている。
  - loader が token 仕様に依存し続けると、字句変更やコメント配置の影響を受けやすい。
- 未確定点:
  - module path の正本を `spire` AST と `xldr` lowering のどちらに置くか
  - qualified module path を AST 上でどう保持するか
- 受け入れ条件:
  - module path 抽出が token 走査 fallback なしで成立する。
  - コメントや空行に影響されず同じ module path を導出できる。
- テスト方針:
  - `unit/spire` または `unit/xldr` で `defmod Kernel` / `defmod A::B` の抽出結果を固定する。
  - `rune` / `xldr` 経路で fallback なしでも同じ結果になることを確認する。

### OI-007 Rune / Xldr の CLI エラー契約統一

- 背景:
  - `rune` は `RuneError` ベースだが、`xldr::cli_command` と `xldr::tui::run_command` は依然として `Result<(), i32>` を返している。
  - 現状でも動作はしているが、REPL / TUI 系の失敗経路だけ typed diagnostic から外れている。
- 未確定点:
  - `xldr` 側に共通エラー型を持たせるか、`rune` 側 adapter を正式境界とするか
  - usage / diagnostic / exit code をどこまで同一契約で扱うか
- 受け入れ条件:
  - CLI / REPL / TUI の失敗経路が同じ契約で説明できる。
  - stderr 出力責務と exit code 責務の境界が crate 間で明文化される。
- テスト方針:
  - `rune` integration で失敗時の exit code とメッセージ形状を固定する。
  - `unit/xldr` または `unit/rune` で adapter / 変換経路を確認する。

### OI-008 `TypeRegistry::lookup` の参照コスト最適化

- 背景:
  - `TypeRegistry::lookup` は現状でも正しく動くが、`entries.iter().find(...)` の線形探索である。
  - 表示系や nested value の増加時に、lookup コストの説明が実装依存のまま残っている。
- 未確定点:
  - 追加 index を持つか、tag と配列位置を一致させる設計に寄せるか
  - serialize 形と決定性を崩さずに O(1) 相当 lookup を導入する方法
- 受け入れ条件:
  - lookup の高速化方針が決定性と表示契約を壊さない。
  - 表示結果と runtime tag の外部観測が変わらない。
- テスト方針:
  - 既存 display 系テストを維持し、内部 index 導入後も同じ表示になることを確認する。
  - `unit/sindr` に lookup 整合テストを追加する。

### OI-009 List runtime 表現の簡素化

- 背景:
  - `cons cell + handle` と `len` O(1) の runtime 契約は固定済みで、現行実装もそれに沿っている。
  - ただし `ListNode` が単一バリアント enum のまま残っており、`tail_handle()` の不変条件も読み取りにくい。
- 未確定点:
  - `ListNode` を struct に寄せるか、将来拡張余地を優先して enum のままにするか
  - `ListHandle` の非空条件を型 / コメント / helper のどこで明示するか
- 受け入れ条件:
  - 非空リストの不変条件がコードから読み取りやすくなる。
  - `head_value()` / `tail_handle()` の挙動と API は変えない。
- テスト方針:
  - 空 / 1 要素 / 複数要素の `head_value()` / `tail_handle()` を `unit/sindr` で固定する。
  - 必要なら `len` 更新と handle 連結の不変条件テストを追加する。

### OI-010 `RichError` 表示形式の仕様化

- 背景:
  - `RichError::to_display_string` は現在 message を quoted form で表示している。
  - 実装上は安定しているが、user-facing display としてその形式を正本化するかは未確定のままである。
- 未確定点:
  - quoted form を正式仕様にするか、より自然な表示へ変えるか
  - 仕様化する場合に `docs/dev/EldrVM_spec.md` と rustdoc のどちらを正本にするか
- 受け入れ条件:
  - 採用した表示形式が docs とテストで一致する。
  - `Value::Error` の表示契約が将来ぶれない。
- テスト方針:
  - `unit/sindr` の `display_for_rich_error_*` 系テストで採用形式を固定する。

### OI-011 apply / compose lowering 最適化方針

- 背景:
  - `|>`, `|*>`, `|>=`, `>>`, `>*`, `>=>` の外部契約は固まっている。
  - 一方で `forge` は compose 系で pending / synthetic callable を使う経路をまだ残しており、最適化方針は未固定である。
- 未確定点:
  - 即時適用 compose をどこまで直接 lower するか
  - synthetic callable と直接 lowering の選択基準
  - 追加最適化が debug 性能や span 診断に与える影響
- 受け入れ条件:
  - 最適化前後で外部挙動と診断位置が変わらない。
  - codegen / VM の責務境界を壊さない。
- テスト方針:
  - 既存 spec / compile_errors / integration を回帰基準にする。
  - IR / opcode 選択が変わる場合は unit test で固定する。

### OI-012 `.eldr` viewer follow-up

- 背景:
  - `.eldr` の viewer 向け chunk 基盤は入っているが、より深い debug 表示に必要な metadata はまだ最小限に留まっている。
  - `viewer.rs` 側にも source lookup や table 化の基盤はあるが、source compare や import 粒度の深掘りは未実装である。
- 未確定点:
  - `Dbgi` / `LocT` のような追加 table を入れるか
  - span に source id を持たせるか
  - import / literal metadata をどこまで viewer 向けに増やすか
- 受け入れ条件:
  - viewer が `.eldr` 単体で必要な debug 文脈を読める。
  - 追加 chunk 導入後も現行 decode 契約との後方整合が保たれる。
- テスト方針:
  - `unit/sindr` で chunk 整合性と参照先妥当性を固定する。
  - `integration` で dump 出力が必要テーブルを欠かさないことを維持する。

### OI-013 Tail Call Optimization の観測導線

- 背景:
  - TCO v1 は実装済みで、`tail_calls_optimized` も観測値として追加されている。
  - ただし、その観測値を CLI / dump / viewer にどう露出するかはまだ統一されていない。
- 未確定点:
  - `surtr run` / `surtr dump` / viewer で何を見せるか
  - tail-position call を bytecode 上で明示するか
  - span 診断や call trace を保ったまま追加最適化する方針
- 受け入れ条件:
  - 利用者が「TCO が効いたかどうか」を観測できる。
  - docs 間で TCO の適用範囲説明がぶれない。
- テスト方針:
  - tail recursion / mutual recursion / non-tail recursion の観測ケースを回帰基準にする。
  - CLI / JSON 露出を増やす場合は integration で形状を固定する。

### OI-014 private value 持ち出し warning 方針

- 背景:
  - 現行仕様では private value access の禁止境界は整理済みだが、`return user.password` のような値の持ち出しに warning を出すかは未確定である。
  - これは安全性というより lint / UX の契約として残っている論点である。
- 未確定点:
  - warning を導入するか
  - 導入する場合の severity、文言、lint 体系との接続
- 受け入れ条件:
  - warning 導入有無が `doc/要件定義v9.md` と `docs/dev/テスト方針.md` で一貫する。
  - warning を入れても成功ケースを compile error にしない。
- テスト方針:
  - warning 導入時は diagnostics の human / JSON 出力を integration で固定する。
  - warning を導入しない場合は既存成功 fixture を維持する。

### OI-015 test DSL I/O capture の `it` 単位分離

- 背景:
  - `Test::capture_stdout` / `Test::capture_stderr` は導入済みだが、現行は per-VM バッファ + drain cursor 方式である。
  - そのため同一 script VM 内の隣接 `it` に未読出力が流入する可能性が残っている。
- 未確定点:
  - `it` 境界で cursor reset を自動化するか
  - `test` / `describe` / `it` のどの粒度で capture scope を切るか
  - drain API のまま維持するか、peek 系 API を追加するか
- 受け入れ条件:
  - `it` 単位で deterministic に capture できる。
  - 後方互換方針が明確である。
- テスト方針:
  - `tests/integration/test_command.rs` に `it` 間混入防止ケースを追加する。
  - `lib/tests/*.srt` に capture API の推奨パターン fixture を用意する。

### OI-016 HashMap v1 follow-up

- 背景:
  - `HashMap<$V>`、`defmod HashMap`、表示契約は既に baseline として定着している。
  - 一方で literal sugar、追加 surface、runtime 内部表現の最適化余地はまだ未確定である。
- 未確定点:
  - `hash![...]` literal sugar を導入するか
  - `entries(map)` などの surface を v1.x で増やすか
  - runtime 内部表現を `Vec<(String, Value)>` のまま維持するか、補助 index を併設するか
- 受け入れ条件:
  - 採用方針が `doc/要件定義v9.md` / `docs/dev/EldrVM_spec.md` / `docs/dev/テスト方針.md` の3点で矛盾しない。
  - surface を増やす場合は std module と builtin metadata の整合が取れる。
- テスト方針:
  - literal 導入時は `unit/spire` / `unit/forge` / `spec` を同時に固定する。
  - runtime 表現変更時は insertion-order と display 契約を回帰基準にする。

### OI-017 Worker API 最終形

- 背景:
  - process runtime の大枠は `docs/dev/ProcessRuntime_spec.md` に固定したが、worker の user-facing API はまだ足場段階である。
  - 検討メモでは `spawn`, `DynamicSupervisor::spawn`, `adopt`, `handoff`, `join`, `await`, `on_down`, `Process::exit`, `Process::sleep` の整列が未決として残っている。
- 未確定点:
  - `Worker::spawn` と `DynamicSupervisor::spawn` の最終 surface をどう並べるか
  - `adopt` / `handoff` を user-facing API として公開するか、runtime intrinsic に留めるか
  - `join` / `await` / `on_down` を generic `receive` の代替となる目的別 API としてどう分けるか
  - `Process::exit` と `Process::sleep` を worker lifecycle API とどう整合させるか
- 受け入れ条件:
  - worker 生成、所有権移譲、終了観測の surface が REPL / script / project で一貫する。
  - current process ownership を default とする方針と `DynamicSupervisor` 配下運用の両方を矛盾なく説明できる。
- テスト方針:
  - `spec/process_runtime/**` に worker spawn / await / on_down / supervisor 経由 spawn の成立ケースを追加する。
  - `compile_errors/process_runtime/**` に不正な ownership / lifecycle API 組み合わせを追加する。

### OI-018 Process Runtime の最終同期項目

- 背景:
  - process runtime の基本契約は固まったが、ツーリング表示、boundary layer、VM 可視化、標準ライブラリ再編は最終段階で同期する想定のままである。
  - 現行実装でも `:doc`, `:sig`, `:type`, `:info`, `:lens`、runtime stats、標準 `Process` / `Task` API は存在するが、process-aware な見せ方は未完成である。
- 未確定点:
  - REPL / tooling 表示で init route、process API、singleton slot、supervisor tree をどこまで露出するか
  - domain error / runtime error / boot error を host outcome としてどう正規化するか
  - `RuntimeProcessSpec`, singleton slot, process table, deadline queue, waiting table, hidden message dispatch, supervisor tree をどの debug surface で見せるか
  - `Process`, `Task`, `File`, `Env`, `StdIn`, `StdOut`, `StdErr`, `Logger`, `DynamicSupervisor` の標準ライブラリ再編境界をどう切るか
- 受け入れ条件:
  - 開発者向け spec と REPL / CLI / dump / viewer の観測導線が矛盾しない。
  - process runtime の host 境界と標準ライブラリ境界が `docs/dev/` と実装の両方で説明できる。
- テスト方針:
  - `unit/xldr` / `integration/repl` / `rune` integration で process-aware な表示と失敗形状を固定する。
  - `integration/build_roundtrip` / `run_eldr` / viewer 系テストで runtime metadata の可視化形状を固定する。

### OI-019 `Workers<$Worker>` 拡張 API 境界

- 背景:
  - `Workers<$Worker>` と `WorkerLease<$Worker>`、および `submit` / `broadcast` / `reserve` / `size` は現行 process runtime v2 の確定 surface である。
  - 一方で設計メモには `submit_timeout`, `snapshot`, `idle_count`, `busy_count`, `drain`, `set_target` などの候補 API が残っている。
- 未確定点:
  - 追加 API を `Workers` の public surface に含めるか、pool wrapper 側 helper に留めるか
  - `snapshot` / `idle_count` / `busy_count` を観測 API として固定するか
  - `submit_timeout` / `drain` / `set_target` を runtime primitive として持つか
- 受け入れ条件:
  - `Workers<$Worker>` が opaque closed handle である前提を崩さない。
  - pool 用 helper を増やしても `List<PID<_>>` 的な抽象漏れを起こさない。
- テスト方針:
  - surface 追加時は `spec/modules/process_workers_pool_surface` と `lib/process.srt` を同時に固定する。
  - 不採用 API は `compile_errors` または parser/rewrite テストで誤用を防ぐ。

### OI-020 Worker pool membership / scale / reconcile 意味論

- 背景:
  - 現行仕様で確定しているのは、`Workers<$Worker>` が runtime-managed closed membership を持つことと、Singleton GenServer pool state に置く使い方までである。
  - ただし pool の増減、補充、再同期、worker 異常終了後の再構成方針はまだ固定していない。
- 未確定点:
  - target worker 数を runtime が維持するか、user code が reconcile loop を持つか
  - membership 変更を supervisor policy とどう連携させるか
  - busy / idle / dead worker を pool surface でどこまで観測できるようにするか
- 受け入れ条件:
  - pool size 変化と worker 再構成の責務境界が docs と runtime 実装で説明できる。
  - `Workers<$Worker>` の closed-set 契約と supervisor ownership が矛盾しない。
- テスト方針:
  - 方針確定後に worker exit / pool refill / scale up/down の spec fixture を追加する。
  - observability を増やす場合は `status` / dump / snapshot 出力の形状を固定する。

### OI-021 Supervisor hierarchy の柔軟化

- 背景:
  - 現行 `supervisor_init` は親構成を固定し、`parent` override を reject する。
  - custom supervisor / DynamicSupervisor / singleton の基本配置は正本化済みだが、より柔軟な親子指定は将来課題として残っている。
- 未確定点:
  - `supervisor_init` に親 override を導入するか
  - 導入する場合に singleton / worker / supervisor ごとの許可境界をどう切るか
  - tree 構成変更を compile-time 検査と runtime boot plan にどう反映するか
- 受け入れ条件:
  - 親子 DSL を広げても current fixed hierarchy と同じ安全性を保てる。
  - boot diagnostics と runtime observability が新 hierarchy を矛盾なく表現できる。
- テスト方針:
  - 現状の reject ケースは `compile_errors` で維持する。
  - 将来許可する場合は boot plan 生成、restart 伝播、status 表示の fixture を追加する。

### OI-022 Supervisor policy の公開深度

- 背景:
  - `defsupervisor` では `strategy`, `max_restarts`, `max_seconds`, `child_restart_default`, `allow_adopt`、必要なら `shutdown_timeout` を policy 値として扱う方針である。
  - ただし `shutdown_timeout` を含む policy の user-facing surface、status 表示、override 深度はまだ十分固定されていない。
- 未確定点:
  - `shutdown_timeout` を初期フェーズから正式 surface に含めるか
  - supervisor `status()` や observability で policy 値をどこまで露出するか
  - boot-time override をどの policy まで許可するか
- 受け入れ条件:
  - compiler-managed supervisor surface と runtime status 表示が同じ policy 集合を前提にできる。
  - policy を追加・露出しても restart semantics の未確定部分を先に固定しなくて済む。
- テスト方針:
  - surface 化する policy は `spec/modules/process_supervisor_user_surface` と compile error fixture で固定する。
  - observability へ露出する場合は dump / REPL 表示の形状を integration で固定する。

### OI-023 Task.Supervisor / Task-DynamicSupervisor link / worker lazy init

- 背景:
  - process runtime v2 では `Task`、`DynamicSupervisor`、worker lifecycle の土台は入ったが、Task supervision と worker lazy init は非対象として残っている。
  - 既存 spec でも `Task.Supervisor`、`Task` と `DynamicSupervisor` の link、worker async/lazy init は後続課題扱いである。
- 未確定点:
  - `Task.Supervisor` を独立 surface として持つか、`DynamicSupervisor` に統合するか
  - `Task` を supervisor 配下に link したときの ownership / restart / cancellation をどう扱うか
  - worker の lazy init や async init を `ProcessInit<T>` と別契約で導入するか
- 受け入れ条件:
  - Task と worker の lifecycle 契約が既存の singleton / worker / supervisor モデルと衝突しない。
  - timeout、waiting、completion 観測が scheduler / runtime diagnostics と整合する。
- テスト方針:
  - 仕様確定後に Task spawn/link/cancel、worker async init、supervisor 配下 completion の spec fixture を追加する。
  - compile-time 制約を入れる場合は process runtime 系 compile error fixture を増やす。

---

## Deferred Topics

以下は現行 baseline に含めず、必要時に reopen する将来課題。

- Project runner
  - source 操作 API、runner DSL、init command を別途仕様化する。
  - compile unit / source rules との責務分離を崩さないことを前提にする。
- REPL command 拡張
  - `:doc` は実装済み前提とし、その先の `:browse`, file ingest UX、補完改善を整理対象にする。
- closure の `expected=None` 推論強化
  - 期待型なしクロージャをどこまで多相的に扱うかを、let-generalization なしの baseline と矛盾しない範囲で再検討する。
  - 退避ケース:
    - `id = {|value| value}` が最初の呼び出しで単相化され、後続の別型呼び出しで `Argument type mismatch` になるケース
- runtime fuel budget
  - 非停止プログラムに step / fuel budget を導入するか、CLI / REPL / library execution のどこで設定するかを決める。
  - 退避ケース:
    - 再帰関数 `loop()` を budget 超過として安定停止させる契約
  - 現状の正本テストは `lib/tests/*.srt` 側へ分離している。
- FuncLiteral surface の将来拡張
  - backtick capture / qualified path / operator capture は実装済み前提とし、それを超える surface 追加だけを reopen 対象にする。
- OOM / host failure policy
  - allocation failure を `RuntimeError`、process failure、host abort のどれとして扱うかと、利用者向け報告文言を将来固定する。
- Enum conversion helper
  - `defenum` 本体や `.idx` 廃止は確定済み前提とし、`Enum::from(Int)` / `Enum::try_from(Int)` の自動生成だけを未実装課題として扱う。

---

## 更新ルール

- 解決済み事項は本ファイルに残さず削除する。必要な履歴は正本仕様・関連 spec・コミット履歴で追跡する。
- 新規 Issue を追加するときは、少なくとも `背景`、`未確定点`、`受け入れ条件`、`テスト方針` を埋める。
- 実装先行で仕様が変わる場合は、先に本ファイルと正本仕様の整合を確認してからコード変更する。
- 将来課題のうち、まだ open issue として具体化していないものは `Deferred Topics` に置き、実装着手時に個別 OI へ昇格させる。
