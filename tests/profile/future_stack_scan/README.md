# Nonempty Future table / operand stack

Manual release probes for OI-036. The unit probe constructs a List from repeated
LoadLocal opcodes with an empty, unrelated Running/Ready, or stack-referenced
Running/Ready Future. Preparation is outside VM::run timing. Values are Int(7).
The same test function and counters run in baseline and candidate binaries.

Build and save each source state with independent target directories. Read the
Cargo JSON compiler-artifact executable and retain the source hashes. The unit
binary must include `vm::future_stack_tests::nonempty_future_stack_profile`.
The build records consumed by run.py contain `source_sha256` plus `records`
(command, artifact, saved binary path, binary_sha256). Both unit and Rune CLI
artifacts are recorded in the OI-036 report.

```sh
cargo test --release --locked -p eldr --lib --no-run --message-format=json --target-dir /absolute/before-target
cargo build --release --locked -p rune --bin surtr --message-format=json --target-dir /absolute/before-target
python3 tests/profile/future_stack_scan/run.py --before /absolute/before_builds.json --after /absolute/after_builds.json --before-layout count --after-layout indexed --output doc/optimize/runtime/value/future_stack_new_run --samples 6
python3 tests/profile/future_stack_scan/batch.py --before /absolute/before-surtr --after /absolute/after-surtr --output doc/optimize/runtime/value/future_stack_batch_new_run --samples 6
```

run.py uses 1,000 / 10,000 elements, five Future conditions, six AB/BA balanced
pairs per condition, and fresh processes. It validates exact read/write visit
counts. Full List content verification uses separate PROBE_VERIFY processes;
those results omit elapsed time and RSS. Select `--before-layout full|count|indexed` and `--after-layout full|count|indexed`
to check the actual source variants (defaults: full/count). A live PendingFuture
still triggers readiness checks; the indexed variant visits only that slot on each
opcode. This is not a fairness or allocation benchmark.

batch.py generates ordinary Task::async / List literal / List::len / Task::await
programs, plus controls without a Task. Compiler artifacts are warmed for both
executables before measurement; every measured process must report cache hit.
CLI execute includes runtime setup, run, background drain, and output. CLI total
also includes cache loading; parent process wall includes startup/final cleanup.
These are distinct from the unit probe's VM::run interval. Outputs are validated.
Source text and binary/source/runner hashes are retained in the summaries.

Darwin process peak RSS includes preparation, validation, JSON and test harness;
it is not a phase's retained memory. Allocation requests/live bytes and physical
copy bytes are unmeasured. Logical scan counters are enabled in unit time series.
No time or RSS thresholds are correctness tests. Runners refuse existing raw
output files. Stop builds/tests before performance samples.

See [implementation and measurements](../../../doc/optimize/runtime/value/future_stack_nonempty_optimization_2026_10_13.md).

The indexed follow-up compares count/indexed layouts, and further series compare
indexed/indexed binaries with inline hints, optional Box metadata, and the final
compact push/pop path. Preserve
both build records and treat binary code layout as part of the measured variant.
See [final costs](../../../doc/optimize/runtime/value/future_stack_metadata_optimization_2026_10_13.md)
and [follow-up plan](../../../doc/optimize/runtime/value/future_stack_index_plan_2026_10_13.md).

For the table guard follow-up, build both variants with table-empty-check counters
and use `--before-layout indexed --after-layout indexed --check-table-guard`.
The flag requires table-first baseline and stack-first candidate source states.
It checks exact table-empty-check counts independently of stack slot visits.
The CLI binary has no cfg(test) counters; compare it separately with batch.py.
See [table guard scope and results](../../../doc/optimize/runtime/value/future_stack_guard_optimization_2026_10_13.md).

## 連続する同一IDのready確認

`lookup_build.py`はCargoが返すdebug Eldr test executableを保存し、Eldr・Sindr・diagnosticsのRust sourceとmanifest / lock、binary hashを記録する。保存先は新しいディレクトリを指定する。前後のsource状態でそれぞれbuildし、保存したbinaryを比較する。同じtargetの逐次rebuildも使用できるが、前のbinaryを保存し、検証中のbuildは避ける。

```sh
python3 tests/profile/future_stack_scan/lookup_build.py --target-dir /absolute/target --output-dir /absolute/before
# 対象の製品変更後
python3 tests/profile/future_stack_scan/lookup_build.py --target-dir /absolute/target --output-dir /absolute/after
python3 tests/profile/future_stack_scan/lookup_runs.py --before /absolute/before/builds.json --after /absolute/after/builds.json --output /absolute/new_run
```

`lookup_runs.py`は1 / 256 / 4,096追跡位置に普通値を挟み、反復IDと交互ID、Running / Ready / Cancelled / unknown / Futureを返すFuture / PendingFutureなしを前後2 sample、AB / BAの144 fresh processで比較する。実際の`resolve_ready_pending_stack_values`の呼出しcounterを読み、全slotの値と一段ずつのFuture解決も検証する。cleanup待ち、context切替え、checkpoint / rollbackはcrate-local受入テストで確認する。

操作数だけの系列であり、実時間・allocator要求・live・RSS・通常CLIを測らない。反復IDのready確認は位置数→1、交互IDは位置数のままである。read / write visitsは前後同じとし、counterとbinary hashが想定どおりでなければ停止する。既存の時間系列とは集計しない。

[仕様と結果](../../../doc/optimize/runtime/value/future_stack_lookup_runs_2026_10_14.md)の基準patchを`0288964c`へ適用すると、counterとmanual testを持つ変更前sourceを再現できる。受入テスト3件は変更前に期待どおり失敗するため、操作数の検証ではmanual testだけを指定する。

## cleanup前の結果clone

`ready_value_clones.py`はFuture表から配送候補を取得するValue cloneを数える。`lookup_build.py`で前後のCargo test executableを保存して比較する。

```sh
python3 tests/profile/future_stack_scan/ready_value_clones.py --before /absolute/before/builds.json --after /absolute/after/builds.json --output /absolute/new_clone_run
```

1 / 256 / 4,096回の確認、0 / 64要素Tuple、raw検査・Task終了判定・cleanup待ち・配送可能のReady / CancelledとRunning / unknownの10条件を、前後2 sample・AB / BAの240 fresh processで検証する。全値の一致、元の表の値の維持、cleanup前の配送不可と完了後の配送も確認する。counterはFuture表内の元のValueを複製する呼出しだけを数え、入れ子のclone、各slotへの配送clone、検証用の値準備は含まない。

これは操作数だけの系列であり、実時間・allocator要求・live・RSS・通常CLIは測らない。[仕様と結果](../../../doc/optimize/runtime/value/future_ready_clone_deferral_2026_10_14.md)の基準patchを`922c457d`へ適用すると、counterとmanual testを持つ変更前sourceを再現できる。受入テスト2件は変更前に失敗するため、操作数の比較ではmanual testだけを指定する。

## 待機Taskの判定と配送

`waiting_readiness.py`は製品の`start_await_task`が登録した待機Taskで、`has_runnable_detached_tasks`による判定だけのcloneと、driverが実配送するときのcloneを分ける。`lookup_build.py`で前後のCargo test executableを保存して比較する。

```sh
python3 tests/profile/future_stack_scan/waiting_readiness.py --before /absolute/before/builds.json --after /absolute/after/builds.json --output /absolute/new_waiting_run
```

1 / 256 / 4,096回の確認、0 / 64要素Tuple、Ready / Cancelled / Running / unknownを前後2 sample、AB / BAの96 fresh processで検証する。Ready / Cancelledでは判定のsource cloneが確認数→0、driver内のcloneが2→1になる。元の値と完了Futureの結果、Task recordの除去、完了が繰り返されないことも確認する。Running / unknownは待機のままとする。

操作数だけの系列であり、実時間・allocator要求・live・RSS・通常CLIは測らない。producer cleanup待ちの選択・通知・次のdriverでの配送はcrate-local受入テストで確認する。[仕様と結果](../../../doc/optimize/runtime/value/future_readiness_borrow_2026_10_14.md)の基準patchを`53354bdf`へ適用すると変更前sourceを再現できる。受入テスト2件は変更前に失敗するため、操作数の比較ではmanual testだけを指定する。

## 収集済みready IDの再利用

`ready_snapshot.py`は既存のstack解決manual testを使い、反復 / 2 ID交互 / 全ID別の配置でready確認と収集用source cloneを数える。`lookup_build.py`で前後のCargo test executableを保存して比較する。

```sh
python3 tests/profile/future_stack_scan/ready_snapshot.py --before /absolute/before/builds.json --after /absolute/after/builds.json --output /absolute/new_snapshot_run
```

1 / 256 / 4,096追跡位置、6条件、3配置、前後2 sampleをAB / BAの216 fresh processで検証する。2 ID交互のReady / Cancelled / Future結果では確認とsource cloneを位置数→distinct ID数に減らす。反復・全ID別・Running / unknown / PendingFutureなしの対照と、全slotの値、一段ずつのFuture結果、read / write visitsを固定する。

操作数だけの系列であり、実時間・allocator要求・live・RSS・通常CLIは測らない。cleanup待ちの再確認、Tupleの配送と保存枝の独立性はcrate-local受入テストで確認する。[仕様と結果](../../../doc/optimize/runtime/value/future_ready_snapshot_dedup_2026_10_14.md)の基準patchを`cabbcf9e`へ適用すると変更前sourceを再現できる。受入テスト2件は変更前に失敗するため、操作数の比較ではmanual testだけを指定する。

ready ID重複削減済みのowned収集と参照収集の比較では、`ready_snapshot.py --borrowed-snapshot`を使う。前後ともdistinct ready ID数の確認を要求し、参照版のsource cloneは0回、slot cloneは前後同数とする。要素型サイズも測るが、実allocator要求・RSSは測らない。

## ready mapの借用

```sh
python3 tests/profile/future_stack_scan/ready_snapshot.py --before /absolute/before/builds.json --after /absolute/after/builds.json --borrowed-snapshot --output /absolute/new_borrow_run
```

[仕様と結果](../../../doc/optimize/runtime/value/future_ready_snapshot_borrow_2026_10_14.md)の基準patchを`c76a0980`へ適用すると同じ観測を持つowned版を再現できる。受入テスト2件はowned版で失敗するため、比較ではmanual testだけを指定する。過去の重複削減比較はこのflagなしで実行する。

## ready収集の確保要求と時間

`snapshot_cost.py`は実際のstack解決一回を測る。`lookup_build.py --release`で独立targetへ前後のtest executableを構築し、Cargo artifact・source / binary hashを保存する。Git archiveを使う場合は`--root`と元の完全な`--revision`を指定し、[測定記録](../../../doc/optimize/runtime/value/future_ready_snapshot_cost_2026_10_14.md)の基準patchを適用してsource hashを照合する。

```sh
python3 tests/profile/future_stack_scan/lookup_build.py --root /absolute/baseline --revision c76a0980e736d71d80663717aa8cc5031b4a197f --release --target-dir /absolute/before-target --output-dir /absolute/before
python3 tests/profile/future_stack_scan/lookup_build.py --release --target-dir /absolute/after-target --output-dir /absolute/after
python3 tests/profile/future_stack_scan/snapshot_cost.py --before /absolute/before/builds.json --after /absolute/after/builds.json --output /absolute/new_cost_run --samples 6
```

256 / 4,096位置、0 / 64要素Tuple、反復 / 全ID別、Ready / Running / Futureなしを使う。両版とも同じtest専用System wrapperを通し、確保系列では測定threadの明示scope内の要求layout bytes・解放bytes・回数を記録する。reallocは新しい要求全量を数える。時間系列は確保の記録を無効にし、Instant区間を測る。両系列とも準備を含まず、時間には既存の論理counterとwrapperの判定が残る。両版に同じslot clone / map型サイズのcounter hookを加える。

全内容を別の48 verify processで確認し、確保・時間を前後6 sampleずつAB / BA同数の576 processで測る。計624 process。要求と解放の差が配送先Tupleの要素bufferと一致することも照合する。この差はscope内の要求layoutの純増であり、VM全体のlive / peak保持量ではない。RSS・allocator usable size・通常CLI・cleanup Taskの費用を測らず、速度閾値をCIへ追加しない。測定中はbuild・他のtestを実行しない。runnerは既存の原本への上書きを拒否する。

## Cleanup表の一括検索と小さい未解決cache

`cleanup_cost.py`は`lookup_build.py --release`の前後metadataを使う。8a5f7505へ[基準の観測patch](../../../doc/optimize/runtime/value/future_cleanup_snapshot_2026_10_15_baseline.patch)を適用し、同じfixture/counterを持つ旧binaryを別targetへ保存する。76条件で全内容確認152 process、確保と時間各912 processをAB / BA同数で記録する。cleanup表訪問・ID確認・一段配送・全値とscope内live収支を検証し、Running / unknown / 普通値の確保0も確認する。時間にはcfg(test)counterとwrapper判定が残る。all-blockedの新しい一時map確保を記録し、全条件の時間改善を要求しない。RSSと通常CLIは別系列。

`../runtime_adoption_build.py`は通常Rune release artifactと全compiler/runtime/lib sourceを保存する。既存`batch.py`でFutureなしと未await Taskのcached普通入口を別に比較する。全build/test終了後に測り、原本は新しいprefixへ保存する。[結果と限界](../../../doc/optimize/runtime/value/future_cleanup_snapshot_2026_10_15.md)。
