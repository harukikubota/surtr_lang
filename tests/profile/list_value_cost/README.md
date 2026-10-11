# RT-3: Value と実入口の費用

入力は [2026-10-11 の測定レポート](/Users/haruca/.codex/worktrees/4c18/surtr/doc/list_runtime_measurement_report_2026_10_11.md) の RT-3。
製品コードや通常の correctness test から独立した、level2 相当の手動測定である。
harness自体は速度やRSSの合否閾値を持たない。方式選定と製品変更は [最適化レポート](../../../doc/list_runtime_rt3_optimization_2026_10_11.md) に記録する。

## 対象

`list-value-cost-probe` は公開 API と手で組み立てた bytecode で、
singleton / empty / huge / chain / nested / product を実行する。
小さい Int、256-byte String、固定深さの Tuple / List / HashMap を比較する。
scalar の値も共通の template local から取得する。
List は backing storage を共有し、Tuple と HashMap は内部の Value を clone する。
payload の詳細と所有条件は [測定結果](../../../doc/list_value_cost_measurement_2026_10_11.md) に記載する。

`generic_do` は現行の compiler 全段と標準 `Monad::bind` / generic do を通す。
Int / String の6 workloadを測る。三段直積は `(x, y, z)` の Tuple を作る。
手組み VM の product は同じ payload の増幅モデルなので、両者の速度比を最適化効果と扱わない。

singleton / empty / huge / chain / nested は n=1,000 / 10,000、product は n=10 / 20。
chain / nested は深さ8。各条件を新しい process で5回実行する。
各 bin の B（入力訪問数）/ E（全 Builder 追加数）と最終出力 M を区別する。
手組み VM の Return opcode 数は、この mapper だけが Return する成功 workload での
callback 完了数の代理指標である。compiled do の callback 数は期待値のみを報告する。

## 測定の境界

入力生成、callback、flatten は VM 実行区間に含む。それぞれの時間を直接分離した値ではない。
compiler の標準準備・ユーザソースの処理、VM 準備、実行、出力の取得、破棄を別区間で記録する。
出力の取得に clone が必要な場合は、その費用も独立区間に含める。
区間データは scalar で保存し、JSON 生成と出力は測定区間の後に置く。
検証の全走査や比較は `PROBE_VERIFY=1` の別 process で行い、性能原本へ採用しない。

| bin | 区間 | 含む処理 |
|---|---|---|
| 手組み VM | `prepare_bytecode` / `prepare_vm` | opcode / constant / function の構築、VM 作成と observer 有効化 |
| 手組み VM | `run` | boot / bytecode 検証、payload template と入力 List の構築、mapper、flatten、observer |
| 手組み VM | `extract_output` / `drop_vm` / `drop_output` | 出力 ListHandle の共有 clone、出力を保持したまま VM を破棄、最後の出力を破棄 |
| generic do | `setup_stdlib` | module source と script source の登録、既存 semantic snapshot の読込み |
| generic do | `compile` | Scar / Forge session 復元、ユーザソースの全 compiler フェーズ、prefix bytecode clone と chunk 合成、session 破棄 |
| generic do | `prepare_vm` / `run_vm` / `drop_vm` | VM 作成、boot / bytecode 検証・Cons 入力構築・mapper・flatten、出力を含む VM 全体の破棄 |

generic do の `singleton` / `nested` / `product` は do、`empty` / `huge` / `chain` は通常の
`Monad::bind` を使う。入力は tail recursion で1〜nを作り、String は位置番号と256個の `x` を持つ。
source は n 個の literal を列挙せず、n の数値だけを変える。
compiled source の hash、bytecode の opcode / function 数も各 raw row に保存する。
コンパイル用の local handle を破棄しても、process 内の global stdlib snapshot は保持される。

allocator は System allocator の requested layout を数える。
`requested_bytes` は成功した alloc と realloc の新サイズ全体の累積、
`freed_bytes` は dealloc と realloc の旧サイズの累積である。
`live_delta_bytes` は区間前後の論理 live の差、`peak_extra_bytes` は区間開始時を超えた最大増分。
allocator の管理領域、断片化、realloc 内部の一時二重保持は取得しない。
これらは Value clone 回数や物理コピー byte 数ではない。
カウンタ無効系列の割当欄の `null` は未計測を表す。

時間はカウンタ無効系列を使う。カウンタ有効時の時間から overhead を差し引かない。
`process_max_rss_bytes` は macOS `/usr/bin/time -l` による process 全期間のピーク。
入力準備、generic do の compiler・stdlib 準備、区間外の検査・出力も含み、VM 実行区間のピークではない。
generic do は事前に cache を作り、runner は `Some(DiskHit)` だけを性能原本へ採用する。
これは毎 process の既存 snapshot 読込みの費用で、cold compile や process 内 cache hit の費用ではない。
REPL checkpoint、Task / process、待機・取消、scheduler の公平性、実時間上限は測らない。

## 再現

リポジトリルートで、ビルドを先に完了させてから実行する。
以下の基準値コマンドは、製品ソースが `adefb5fc` のcheckoutに同じharnessを置いて実行する。
最適化後のcheckoutでは次節の別target / prefixを使い、既存の基準binaryと原本を上書きしない。
lock は root と同じ依存版を使用する。runner は macOS 専用。
性能系列では `PROBE_VERIFY` を unset する。

```bash
cargo build --release --locked --manifest-path tests/profile/list_value_cost/Cargo.toml --target-dir target/list-value-cost-count
target/list-value-cost-count/release/generic_do singleton 1 int 1 > /dev/null
python3 tests/profile/list_value_cost/run.py --binary target/list-value-cost-count/release/list-value-cost-probe --entry vm --output doc/list_value_cost_vm_2026_10_11_count --samples 5
python3 tests/profile/list_value_cost/run.py --binary target/list-value-cost-count/release/generic_do --entry do --output doc/list_value_cost_do_2026_10_11_count --samples 5
cargo build --release --locked --no-default-features --manifest-path tests/profile/list_value_cost/Cargo.toml --target-dir target/list-value-cost-time
target/list-value-cost-time/release/generic_do singleton 1 int 1 > /dev/null
python3 tests/profile/list_value_cost/run.py --binary target/list-value-cost-time/release/list-value-cost-probe --entry vm --output doc/list_value_cost_vm_2026_10_11_time --samples 5
python3 tests/profile/list_value_cost/run.py --binary target/list-value-cost-time/release/generic_do --entry do --output doc/list_value_cost_do_2026_10_11_time --samples 5
python3 tests/profile/list_value_cost/verify.py --binary target/list-value-cost-count/release/list-value-cost-probe --entry vm
python3 tests/profile/list_value_cost/verify.py --binary target/list-value-cost-count/release/generic_do --entry do
```

出力は `{prefix}.jsonl` と `{prefix}.summary.json`。
全サンプルと中央値・最小・最大に加え、commit、source / lock / runner / binary の SHA-256、toolchain / OS を保存する。
同じ commit、ビルド条件、payload の形と所有条件で比較する。

## 製品最適化後の再測定

前後の結果は [最適化レポート](../../../doc/list_runtime_rt3_optimization_2026_10_11.md)に記録する。
基準値のbinaryと原本を上書きせず、別のtarget directoryと出力prefixを使う。
未コミットの製品差分がある場合は、`source_commit`に加えて変更ソースとdiffのSHA-256をレポートへ保存する。
同じharnessを使い、workload・依存版・sample数・cache条件を維持する。

```bash
cargo build --release --locked --manifest-path tests/profile/list_value_cost/Cargo.toml --target-dir target/list-value-cost-optimized-count
target/list-value-cost-optimized-count/release/generic_do singleton 1 int 1 > /dev/null
python3 tests/profile/list_value_cost/run.py --binary target/list-value-cost-optimized-count/release/list-value-cost-probe --entry vm --output doc/list_value_cost_vm_2026_10_11_optimized_count --samples 5
python3 tests/profile/list_value_cost/run.py --binary target/list-value-cost-optimized-count/release/generic_do --entry do --output doc/list_value_cost_do_2026_10_11_optimized_count --samples 5
cargo build --release --locked --no-default-features --manifest-path tests/profile/list_value_cost/Cargo.toml --target-dir target/list-value-cost-optimized-time
target/list-value-cost-optimized-time/release/generic_do singleton 1 int 1 > /dev/null
python3 tests/profile/list_value_cost/run.py --binary target/list-value-cost-optimized-time/release/list-value-cost-probe --entry vm --output doc/list_value_cost_vm_2026_10_11_optimized_time --samples 5
python3 tests/profile/list_value_cost/run.py --binary target/list-value-cost-optimized-time/release/generic_do --entry do --output doc/list_value_cost_do_2026_10_11_optimized_time --samples 5
python3 tests/profile/list_value_cost/verify.py --binary target/list-value-cost-optimized-count/release/list-value-cost-probe --entry vm
python3 tests/profile/list_value_cost/verify.py --binary target/list-value-cost-optimized-count/release/generic_do --entry do
```
