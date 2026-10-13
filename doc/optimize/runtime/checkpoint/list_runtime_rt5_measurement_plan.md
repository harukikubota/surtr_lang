> 取り込み元: `/Users/haruca/.codex/worktrees/4c18/surtr`（detached、HEAD `ee13e448`）。資料の集約。製品変更を伴わない基準測定。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# RT-5: REPL checkpoint のコピー量を分けて測る計画

作成日: 2026-10-11。状態: 読み取り調査と測定計画。実測・最適化は未実施。

入力は [List / VM の残課題](../list_runtime_followups.md) の RT-5。
本書は残るコピー箇所を現行コードへ対応させ、測定方法を具体化する。
性能値、共有方式の追加、停止済み PID の回収契約は本書で決めない。
言語の意味論は変更しない。計測用コードを追加する場合は、Eldr 内部に閉じる
level2 の作業を基本とし、公開観測 API の変更が必要になれば影響範囲を見直す。

## 現行コードで確認した範囲

[VM](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/vm.rs) の `push_atomic` は chunk の検証後に
`checkpoint_for_chunk` を呼ぶ。実行失敗、または実行後の bytecode 検証失敗では
`rollback_to_checkpoint` で復元する。成功時は checkpoint を破棄する。
[InteractiveVm](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/interactive.rs) の `push_chunk` は policy 検証を加えた通常入口である。

`ProcessRuntime` は `Clone` を derive している。保存時の複製は次のように分かれる。

| 範囲 | 現行の保存・変更時の処理 | 測定で分ける量 |
|---|---|---|
| `processes`、`executions`、`futures`、`detached_tasks`、`stopped_identities` | `RuntimeTable` の index と entry を `Rc` で共有する。共有中の変更で index を複製し、必要な対象 entry を独立させる | 各表の件数、変更した表数・entry 数、index の複製回数 |
| `specs_by_id`、`spec_id_by_name`、`specs_by_name`、`handler_contexts` | Vec / BTreeMap と、その文字列・nested metadata を複製する | process spec 数、handler 数、名前・引数文字列の総量 |
| `singleton_by_name`、`singleton_inits`、`refilling_worker_sets`、`worker_sets`、`root_supervisor` | 通常の clone。worker の member vector、supervisor の child table なども対象 | slot / flight / set / child 数、member 数、callable capture の形 |
| `reply_table`、`waiting_table` | BTreeMap を複製する | 件数、待機理由の種類 |
| `deadline_queue`、`run_queue`、`output_events` | VecDeque を複製する。output は文字列内容も対象 | 各 queue の長さ、output の総 byte 数 |
| VM 本体の stack / frame / continuation、`last_result`、test runner、open file 表、cwd | `VmCheckpoint` の生成で複製する。buffer は長さを保存する箇所もある | local / frame / continuation 数、値の形、file 数 |
| 置換対象 function | 入力 chunk が既存 function を上書きする場合、その既存 entry を保存する | 上書き数。通常 REPL append-only の測定では 0 に固定 |

[RuntimeTable](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/vm/runtime_table.rs) の `insert` / `remove` /
`get_mut` / `take` は `Rc::make_mut` を使う。`get_mut` は index と対象 entry を、
`take` は index と取り出す entry を必要に応じて独立させる。
このため、保存を軽くした効果と、保存中の初回変更へ移ったコストを合わせて評価する。
表の件数だけで Builder のコピー量を推定しない。

[ListBuilder](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/builtin/list_builder.rs) の `Clone` は
`items: Vec<Value>` を複製する。通常の実行切替えは所有権を移し、checkpoint と共有した
entry を変更する場合に、その entry が持つ Builder も独立させる。
同じ entry に複数の継続があれば、それらの Builder を含めて測る。
`Value::clone` の物理コストは値の種類に依存するため、要素数と byte 数は別の指標にする。

正本は [Eldr VM 仕様](../../../../docs/dev/EldrVM_spec.md) の 3.5 / 4.2 と
[Process Runtime 仕様](../../../../docs/dev/ProcessRuntime_spec.md) の継続・checkpoint 契約。
保存した位置から再開し、完了済み callback を呼び直さず、結果を復帰先へ一度だけ配送する。
停止識別表と実行 record も保存対象である。active VM の本体回収と、checkpoint が保持する
state / 継続の物理解放を混同しない。回収の設計はこの測定から切り離す。

## 既存の観測でできることと限界

[VM テスト](/Users/haruca/.codex/worktrees/4c18/surtr/crates/eldr/src/vm/list_flat_map_tests.rs) の
`unchanged_process_checkpoints_do_not_clone_suspended_builders` は process 数 1 / 8 / 32 と
checkpoint 回数 1 / 4 / 16 を変え、Builder clone が 0 であることを検証する。
時間も表示するが、Builder 長は固定に近く、metadata / queue / future を独立に増やしていない。
空 chunk 用 checkpoint の生成と破棄を測るため、実 chunk 全体の性能測定ではない。

同ファイルの次のテストは、今後の測定でも維持する correctness の基準になる。

- `process_checkpoint_copies_only_mutated_entry_and_restores_future`: 未変更 process の Builder を複製せず、future / queue / state を復元する。
- `process_checkpoint_stop_copies_only_owned_detached_task`: 対象 owner の task だけを複製し、復元後に完了済み mapper を呼び直さない。
- `flat_map_checkpoint_restores_independent_partial_builder_and_cursor`: 部分 Builder を独立させ、保存した output cursor から再開する。

[REPL テスト](/Users/haruca/.codex/worktrees/4c18/surtr/crates/xldr/tests/repl_core.rs) の
`core_flat_map_builder_survives_runtime_error_rollback` は通常入力の失敗後に
pending Task を復元し、保持済み List と結果配送を確認する。
この関数は REPL case inventory の bucket で実行されるため、関数名をそのまま
nextest の単体 test 名として扱わない。

`ListBuilderMetrics` は test build の clone / push / finish 回数を持つ。
clone した Builder の長さ、capacity、総 byte 数、割当量は持たない。
`RuntimeTable` には index / entry clone 回数の既存カウンタがない。

`VM::observation()` と `process_runtime_snapshot()` は process / future / waiting /
reply / deadline 等の件数と状態を確認できる。snapshot は値を inspect し、文字列・一覧を
新たに構築するため、コピー時間やピークメモリの計測区間に含めない。
runnable process 件数を `run_queue.len()` の代用にせず、queue 自体を別に記録する。
Builder 長と runtime metadata の全コピー量は、現在の公開 snapshot だけでは取得できない。

[Rune 観測仕様](../../../../docs/dev/Rune_observability.md) の `--phase-times` /
`--vm-stats-json` / `--vm-dump` は batch run の補助になるが、REPL checkpoint の区間時間、
index の複製、Builder の複製量、各 chunk のピークメモリを独立に出力する入口ではない。
batch `run` の時間を RT-5 の保存コストとして報告しない。

## 測定方法

### 1. 条件と計測区間を固定する

同じ commit、Rust toolchain、release profile、allocator、OS / architecture で比較する。
ビルドと stdlib の初期化、workload の組立て、snapshot / 表示 / JSON 整形は計測外に置く。
計測時は trace を無効にし、別 run で状態と correctness を確認する。
初期状態の組立てコストと測定対象のコストを別々に記録する。

計測する区間は次の 4 つとする。

1. `checkpoint_for_chunk` の生成だけ。
2. 保存中の初回変更と、同じ entry の二回目以降の変更。新しい chunk が checkpoint を取り直す場合も分ける。
3. 成功後の checkpoint 破棄、または失敗後の rollback と失敗進捗の破棄。
4. policy 検証から返却までの `InteractiveVm::push_chunk` 全体。別測定で `ReplEngine::handle_line` 全体も確認する。

4 には chunk 検証、実行、実行後の検証、保存状態の破棄または rollback が含まれる。
REPL 全体には compiler 各フェーズの checkpoint と compile / 表示も加わる。
差し引きだけで各処理時間を断定せず、区間時間と全体時間を併記する。
成功 chunk の bytecode / local は追加されるため、連続 chunk 数を増やしたときは
prefix opcode / constant / function / local 数も記録する。

### 2. 一因子ずつ変える

各系列では対象以外の件数、値の形、計算量を固定する。
最初の探索点は件数 0 / 1 / 32 / 1,024、chunk 数 1 / 4 / 16 / 64 を候補とする。
大きい点で資源上限に達したら、上限と未測定点を残す。値を勝手に切り詰めない。

| 系列 | 変える量 | 固定・分離する条件 |
|---|---|---|
| metadata | spec 数、handler 数、名前・引数の byte 数 | process / future / queue / Builder を固定。spec 件数と一件のサイズを別系列にする |
| queue / 管理表 | deadline / runnable / output queue、waiting / reply 表、singleton / worker / supervisor の件数 | 一つずつ増やす。output は件数と文字列 byte 数、worker は set 数と member 数を分ける |
| future | future 数 | unchanged 保存、1 entry 更新、1 entry 追加・削除を分ける。Running / Ready の値と waiter 数は別系列 |
| index | processes / executions / futures / detached_tasks / stopped_identities の表件数 | 保存のみ、1 entry 変更、複数 entry 変更。Builder なしの基準を先に測る |
| Builder | 保存時点の Builder 長、Builder を持つ entry 数 | 1 entry に 1 Builder の基準から始め、未変更 / 1 entry 進行 / 全 entry 進行を分ける |
| chunk | 同じ workload を進める成功・失敗 chunk 数 | chunk ごとの進行量と、追加 bytecode 量を記録。保存のみの反復と実入力の反復を分ける |

Builder 長は 0 / 32 / 1,024 / 65,536 を初期候補とし、small scalar と
String / nested value などの payload を別に測る。継続は同じ操作で指定位置まで進め、
その時点の実長を記録する。Builder capacity や mapper の出力 List を初期状態の
RSS に含め、Builder だけがそのメモリを保持していると誤認しない。

metadata と queue を独立に用意する低水準測定は Eldr の private 構造を扱うため、
将来の計測 harness は crate-local に置く。低水準の合成状態はコピー機構の切り分けに限定し、
通常入口の意味論・性能の証拠は実際の `push_chunk` と REPL 入力で別に得る。
private 状態の測定だけを目的に利用者向け API を増やさない。

### 3. 時間とメモリを別 run で測る

時間は短い区間を複数回まとめて測り、各サンプルの総時間、回数、一回当たりの時間、
中央値とばらつきを残す。変更を測る反復では、各サンプルを同じ保存状態から開始する。
初回変更を一度だけ含むループの平均を「変更一回のコスト」として報告しない。

メモリは各 workload を独立した process で実行し、OS が提供する peak RSS を記録する。
macOS では `/usr/bin/time -l <計測 harness>` を候補とし、実測時に出力単位も確認する。
計測用コードを追加する場合、割当回数・累積割当 byte・最大 live allocation は
allocator の別観測 run で取得する。計測器のオーバーヘッドを時間の基準 run に混ぜない。
process 全体の peak RSS は初期化も含むため、測定区間内のピークと同一視しない。
RSS の差分を clone byte 数の代用にせず、初期状態の RSS と peak を併記する。

保存状態を破棄する前後の解放時間も測る。rollback では失敗側を破棄する時間、
成功時には保存側の参照を落とす時間が含まれる。生存要素と共有 backing storage を区別する。
通常の REPL は一 chunk の checkpoint を保持するため、checkpoint を大量に同時保持する
実験は追加の機構測定として分け、通常 workload として報告しない。

### 4. 報告と受け入れ条件

一行の測定結果には系列名、全状態件数、Builder 実長と payload、chunk 数、
index / entry / Builder clone 回数、clone した Builder の総要素数、入力訪問数・出力数・
callback 数、区間時間・全体時間、割当量・peak RSS、ビルド条件を対応させる。
既存観測で取れない指標は「未取得」とする。総要素数から byte 数を推定して確定値にしない。

性能の受け入れ値と最適化の方式は実測後に決める。時間や RSS の閾値を correctness test の
合否に入れない。採用候補は失敗した chunk の進捗を破棄し、保存位置より前の callback を
呼び直さず、復帰先への一度だけの結果配送と Builder の独立性を維持する。
外部 I/O の rollback 保証を追加しない。

## 残作業

本書ではビルド・テスト・性能測定を実行していない。製品コードと実行可能テストも変更していない。
新しい不具合やデグレの再現証拠は得ていない。
次は既存の clone 回数の検証を維持したまま、必要な指標を取得する crate-local の計測 harness を
具体化し、保存のみと初回変更の基準を測る。そこから実 chunk 全体へ広げる。
RT-5 は未解決のままとする。
