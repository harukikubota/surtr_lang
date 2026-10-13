> 取り込み元: `/Users/haruca/.codex/worktrees/9ea8/surtr`（codex/rt5-builder-checkpoint-measurement、HEAD `72154707`）。資料の集約。記載された製品変更・harnessはmain未統合。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# RT-5: 中断中 Task の Builder コピー測定

本書は改善前の測定記録である。後続の [Builderコピー削減](list_runtime_rt5_builder_optimization_2026_10_12.md) で製品実装と前後比較を行った。以下の8区間原本と数値は変更しない。

入力は [2026-10-11 の測定レポート](../baseline/list_runtime_measurement_report_2026_10_11.md) と、main `72154707` の [RT-5 の残件](../list_runtime_followups.md#rt-5-repl-checkpoint-の残るコピー量)。今回は中断中の detached Task が持つ長い Builder の保存・再開・rollback に絞る。製品最適化の追加、process / override の費用分離、compiler / REPL の入力全体の測定は対象外とする。

main では未変更の runtime 表・entry・管理容器を checkpoint と共有する方式へ移行済みである。本作業は、その後も独立性のために必要となる初回変更時の Builder コピーを測る。Eldr の `cfg(test)` probe と手動 runner に閉じる level2 相当の測定で、製品の意味論と VM/schema version は変えない。

## 測定条件と受入条件

- 実際の Task と future を作り、mapper を一度呼び終え、output cursor と非空 Builder を持った未完了位置で保存する。Task 数、保存時 Builder 長、payload を独立に変える。
- 保存と未変更 checkpoint の破棄、保存中の初回再開、新しい保存なしの2回目再開、進行後の再保存、その後の初回再開、rollback を分ける。
- `ListBuilder::clone` 時の実際の要素数を `cfg(test)` で数える。再保存後は長さが増えるため、開始時の長さからコピー量を推定しない。これは論理的な Value clone 件数であり、物理コピー byte 数や requested allocation ではない。
- 保存・未変更 checkpoint 破棄では Builder clone 0、最初の再開では対象 Task の Builder だけを複製し、2回目再開では複製しない。再保存後の再開では新しい保存位置の Builder を複製する。
- rollback 後の内容検証は別 process で行い、全要素の順序・内容、保存前 mapper の再呼出しなし、結果配送の完了を確認する。性能原本に検証実行を混ぜない。
- 準備、assert、JSON 出力を測定区間外に置く。時間の合否閾値は設けず、計測 overhead を差し引かない。RSS は process 全期間の高水位とし、区間の保持量や peak と読み替えない。

既存の正本は [Eldr VM 仕様](../../../../docs/dev/EldrVM_spec.md) の List Builder・checkpoint と、[Process Runtime 仕様](../../../../docs/dev/ProcessRuntime_spec.md) の保存状態の独立性。今回は契約を変更しないため、正本文書の更新は不要である。

## 手順

1. 既存の Task / flat_map テストの helper を使い、測定位置と clone 要素数を直接確認する probe を追加する。通常のテストでは小さい条件で probe の受入境界を確認する。既存動作の測定なので、人工的に製品を壊した Red は作らない。
2. 対象テストを実行し、release test binary を保存する。同じ binary で条件ごとに独立 process を起動し、生データ・集計・source / lock / binary hash を保存する。
3. 別 process で rollback 後の全内容を検証する。サブエージェントと Astra が測定の境界、差分、結果を確認する。測定とビルド・テストは重ねない。
4. 結果と制限を本書へ記録し、RT-5 の残件を更新する。製品変更がないため workspace CI と標準 Surtr 全件は実行しない。

## 結果

中断中 Task の保存と初回再開を分けて測定し、現行の共有方式が Builder コピーを保存時から初回再開へ移していることを確認した。新しい保存なしの2回目再開では複製しない。進行後に再保存すると、次の再開で増えた長さ分も複製する。RT-5 のこの測定単位は完了したが、コピーを減らす製品変更は行っていない。

### 条件と原本

[harness と再現方法](/Users/haruca/.codex/worktrees/9ea8/surtr/tests/profile/checkpoint_builder/README.md)。Apple arm64 / macOS 26.6.2、rustc 1.94.1、root lock を使った release unit-test binary。Task 数1 / 8 / 32、保存時 Builder 長32 / 1,024 / 16,384、位置番号の Int / 256-byte String、計18条件をそれぞれ5 processで測った。各 process は8反復、各反復は独立に VM と Task を準備する。

実際の `start_task` で Task と completion future を作り、準備中だけ1 reductionずつ進めて各 Builder を指定の長さへ揃えた。mapper はTaskごとに1回実行済みで、結果の長さは保存時長さ＋2,048。準備後は通常の startup 境界と同じ started 状態とする。process 本体、override、deadline、外部 I/O は使わない。Task 数を増やすと future 数も同時に増えるため、両者の費用は分離していない。

全 Task が共有する元の出力 List と、Task ごとに所有する Builder を区別する。計測する3回の再開は、通常の scheduler で全 runnable Task を各256 reduction進め、各 Task の Builder に256件ずつ追加する。純粋な Builder clone の kernel や、compiler / REPL の chunk 実行全体の測定ではない。

- [性能原本](rt5_builder_2026_10_12.jsonl)・[集計](rt5_builder_2026_10_12.summary.json): 90 process、5,760区間、144集計。
- [内容検証原本](rt5_builder_2026_10_12_verify.jsonl)・[検証記録](rt5_builder_2026_10_12_verify.summary.json): 全18条件を別 processで1反復ずつ確認。時間とRSSは性能原本へ混ぜない。

ビルド前の source snapshot とビルド直後のソースが一致し、保存した binary と compiler-artifact の binary の SHA-256 が一致した。性能・内容検証は同じ binary を使った。集計に Eldr / Sindr のソース、manifest / lock / binary / runner の SHA-256、dirty status と差分 hash を保存した。測定とビルド・テストは重ねていない。

保存した binary は `/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-9ea8-rt5-builder-dqx_cjov/eldr-builder-profile`。同ディレクトリに `build.jsonl`、`build.stderr`、`build_source_snapshot.json` を保持した。binary SHA-256 は `233e138f4c5e371ab10d90135028af863499e4b0f8974d70556369136350167b`。

### コピー量

全条件・全反復で次の境界が一致した。NはTask数、Lは最初の保存時Builder長である。

| 区間 | Builder clone 回数 | cloneしたValue要素数 | 通常のBuilder追加数 |
|---|---:|---:|---:|
| 保存・未変更保存の破棄 | 0 | 0 | 0 |
| 初回再開 | N | N×L | N×256 |
| 新保存なしの2回目再開 | 0 | 0 | N×256 |
| 再保存 | 0 | 0 | 0 |
| 再保存後の再開 | N | N×(L＋512) | N×256 |
| rollback・最初の保存状態の破棄 | 0 | 0 | 0 |

32 Task・L=16,384では、初回再開が32 clone / 524,288要素、再保存後が32 clone / 540,672要素だった。この要素数は `ListBuilder::clone` 時の `items.len()` を直接累積して測り、保存位置と進行量から求めた期待値とも照合した。通常の追加時に行う Value clone はこの `copied_items` に含めない。

clone 後の Builder は最初の追加で Vec capacity が増えた。L=1,024 / 16,384では初回再開と再保存後の再開で各 Task に1回、新保存なしの2回目再開では0回だった。L=32では初回再開に各4回、2回目と再保存後の再開に各1回の成長があった。これは capacity の変化回数で、allocator 内部の物理移動量を表さない。

### 時間と解放

L=16,384。表はprocessごとの区間合計を8で割った1操作当たりの時間の中央値、単位はms。保存は空chunkの構築も含み、再開は全Taskを一巡する時間である。

| Task数 | payload | 保存 | 初回再開 | 2回目再開 | 再保存後再開 | rollback | 最初の保存状態の破棄 |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | Int | 0.001089 | 0.469438 | 0.046823 | 0.370297 | 0.171635 | 0.161896 |
| 1 | String | 0.001719 | 0.662240 | 0.049646 | 0.643156 | 0.216568 | 0.218860 |
| 8 | Int | 0.002000 | 3.731683 | 0.387943 | 3.405849 | 1.368656 | 1.297771 |
| 8 | String | 0.002531 | 5.294912 | 0.398849 | 4.378031 | 1.958838 | 2.234260 |
| 32 | Int | 0.002693 | 14.775088 | 1.543239 | 13.543521 | 5.745333 | 5.488213 |
| 32 | String | 0.002870 | 21.347646 | 1.651214 | 19.136875 | 10.413443 | 10.268026 |

32 Task / String の初回再開は20.521276〜26.025693ms、再保存後の再開は18.669037〜34.143588msだった。rollbackは7.694005〜14.756542ms、古い保存状態の破棄は9.071604〜10.925208msだった。全条件の最小・中央値・最大は集計に保持した。保存の短さは大きな Builder の初回変更や破棄の短さを保証しない。

rollbackは3回目の進行を持つlive側のBuilderを破棄して、2回再開後の保存位置へ戻す。再保存したcheckpointはこの操作で消費する。最初の保存状態の破棄では、最初の長さLのBuilderを解放する。未変更checkpointの破棄とは所有条件が違う。保存状態を共有しても、必要なコピーと最終解放の仕事は残る。

時間には表indexのCOW、entryのclone、通常の要素追加、Vec成長、scheduler、future表の確認が含まれる。初回と2回目の差を純粋なclone時間とは扱わない。`cfg(test)` のclone / push / capacity counterは時間系列でも有効で、overheadを差し引かない。準備や前区間のallocator状態も後区間へ影響し得る。以前の改善前binaryとは比較しておらず、改善率・直近変更のデグレ・VM全体の実時間上限を判定する測定ではない。

32 Task / L=16,384のprocess全期間peak RSS中央値はIntで245.016 MiB、Stringで620.3125 MiBだった。8反復分の準備・全区間・区間外assert・JSON・test harnessを含む高水位であり、各区間のpeakや現在の保持量ではない。requested allocation、論理live bytes、物理コピーbyte数は未測定である。

### 検証とレビュー

- `rtk cargo nextest run -p eldr list_flat_map_tests`: 修正後16 passed、328 skipped、失敗0。
- `rtk cargo nextest run -p eldr checkpoint_`: 25 passed、319 skipped、失敗0。新probeは両フィルタに含まれるため、通過件数を単純に加算しない。
- `cargo test --release --locked -p eldr --lib --no-run --message-format=json`: build成功。release binaryによる90 processの測定と18 processの内容検証はすべて終了コード0。
- 初回の小さいprobeは2番目のTask作成時にsource context不足で失敗した。直接 `run_quantum` する準備処理が呼出し側の実行状態を残さないことを確認し、通常driverと同じ退避・復元で修正した。Astraが修正を確認した。製品の不具合として扱わない。
- 内容検証ではrollback後の全要素の長さ・位置・payloadを確認した。完成したmapperを呼び直さず、復帰後のBuilder cloneは0、finishは各Task1回、残りの要素だけを追加し、全futureのList結果とTask回収を確認した。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。
- サブエージェントがmain上の残件と受入境界を確認し、別担当がprobeを実装した。Astraは最終harnessと原本を独立レビューし、全5,760行・144集計の時間・counter・位置・RSS、18条件の内容検証記録、binary/source hashを照合した。追加の問題は見つからなかった。Astraはビルド・テストを再実行していない。

製品経路の変更を含まないため、workspace CI、標準Surtr全件、compiler / REPLのchunk全体の測定は実行していない。稼働process、override、追加chunk数別の時間・保持量、表indexとentryの費用の分離、反復更新費用は残る。長いBuilderのコピー削減方式も未決定であり、RT-5全体を完了とはしない。
