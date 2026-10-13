# 実出力が短いBuilderのVM測定

`vm::list_flat_map_tests::true_short_builder_profile` を独立processで実行する。
出力全長は0 / 1 / 8 / 32 / 256、Task数1 / 32、位置番号Int / 256-byte String。
従来probeの「保存時長L＋残り2,048件」を短い出力の結果と読み替えないための40条件。
製品の方式切替を加えず、同じtest instrumentationを含む共有Builderのbefore/candidate binaryを比較する。旧Vecとの比較はcounter期待値が異なるため、このrunnerの対象に含めない。

| workload | 測定範囲 |
|---|---|
| `construct_complete` | immutable入力List、bytecode/VM、Task/Future登録の初期構築から全Task完成まで。checkpointなし |
| `snapshot_resume` | mapper完了後・floor(出力全長/2)件を追加した未完成Taskを区間外で準備し、実checkpoint生成→全Task再開・完成→保存状態破棄まで |

mapperは各Taskで入力1件から出力List全体を一度返す。
0 / 1要素の保存位置はBuilder長0だが、mapperの結果を受け取った後の
`Resume(FlatMap)` で止める。mapper完了前の保存や完了済みTaskを途中保存した値と混ぜない。
準備はTaskの実ExecutionContextを予算1で進め、source callerを保存・復元する。
通常のstartupで量子を使い切ったTaskと同じ`started`状態にする。
途中保存は正確なBuilder長・callback1回・finish未完了であることをassertする。

時間はVMの通常driverを含むworkload全体で、純粋なpush/clone/finishの時間ではない。
VM破棄、counter照合、全結果走査とJSON整形は時間区間外。
`cfg(test)` counterは前後とも有効で、overheadを差し引かない。
通常のtestは0 / 1 / 8要素の小さいsmoke。
`PROBE_VERIFY=1` は別processで全要素の位置・順・長さ、Future結果、Task回収を確認する。
保存後のcallback再実行0、Builder clone時のValueコピー0、完成1回/Taskを固定する。

## ビルド・保存・実行

beforeを作る前に新test instrumentationを固定する。Builder候補だけを変えてcandidateを作り、
両release test binaryを別名へ退避する。ビルドと測定・テストを重ねない。
各build直後、ソースを変える前に `record` を実行する。

```sh
cargo test --release --locked -p eldr --lib --no-run --message-format=json > /absolute/path/to/build.jsonl
# compiler-artifactのexecutableを別名に退避した後、そのbuildのソースのまま記録する。
python3 tests/profile/checkpoint_builder_short/run.py record --binary /absolute/path/to/short-before-binary --output /absolute/path/to/short-before.json
# Builder候補のbuild/退避後、candidateソースのまま同様に記録する。
python3 tests/profile/checkpoint_builder_short/run.py record --binary /absolute/path/to/short-after-binary --output /absolute/path/to/short-after.json
python3 tests/profile/checkpoint_builder_short/run.py compare --before /absolute/path/to/short-before.json --after /absolute/path/to/short-after.json --output doc/builder_short_compare --samples 6 --repeats 8
python3 tests/profile/checkpoint_builder_short/run.py verify --build /absolute/path/to/short-after.json --output doc/builder_short_verify --samples 1 --repeats 1
```

比較は各workloadを別processでAB/BA同数、40条件×前後各6 process。
各processの反復時間を合算し反復数で割り、そのprocess平均の中央値・最小・最大を採る。
6 samples / 8 repeatsで480 process・3,840区間、80集計。
`/usr/bin/time -l` のRSSは初期準備・全反復・VM破棄・assert・JSON・test harnessを含む
process全期間の高水位。各区間の保持量やpeakではなく、同じprocessの行を加算しない。
requested/live/物理コピーbyte数は測らない。

原本は新規作成のみで上書きしない。metadataにbuildごとのsource/binary/manifest/lockと
runnerのhash、toolchain/OSを保存する。比較時にbeforeのsourceを現在のcandidateから採り直さない。
`record`のsource hashは実行時の状態であり、単独ではcompile時の一致を証明しないため、
build前後のsource固定とcompiler-artifact/退避binaryの照合を別途行う。
ビルド後のsource変更やbinary差替えを隠して比較を続けない。
