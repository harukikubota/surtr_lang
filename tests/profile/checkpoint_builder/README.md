# RT-5 中断中 Task / Builder の checkpoint profile

main `c577b32d` を基準とするBuilder共有化前後の初回変更費用を、Eldr の private API を使って測る。
`vm::list_flat_map_tests::checkpoint_builder_profile` を release test binary から単独実行する。
製品 API・feature は追加しない。通常のテストでは小さい条件で clone と継続の境界を確認する。

Builderコピー削減の比較では完成までと保存なしの経路を加えた10区間を使う。
前回の8区間原本 `rt5_builder_2026_10_12*` は変更せず、その時点のsource hashとbinaryを記録として保持する。

```sh
cargo test --release --locked -p eldr --lib --no-run --message-format=json > /tmp/eldr-builder-build.jsonl
```

build JSON の `compiler-artifact` のうち、`target.name == "eldr"` かつ
`executable` がある行から test binary のパスを取り、別名に保存する。
ビルド直後のソースのまま、次を順に実行する。測定とビルド・テストを重ねない。

```sh
python3 tests/profile/checkpoint_builder/run.py --binary /absolute/path/to/before-binary --output doc/optimize/runtime/checkpoint/rt5_builder_new_run_before_completion --copy-mode baseline --samples 5 --repeats 8
python3 tests/profile/checkpoint_builder/run.py --binary /absolute/path/to/after-binary --output doc/optimize/runtime/checkpoint/rt5_builder_new_run_after_completion --copy-mode shared --samples 5 --repeats 8
python3 tests/profile/checkpoint_builder/run.py --binary /absolute/path/to/after-binary --output doc/optimize/runtime/checkpoint/rt5_builder_new_run_after_verify --copy-mode shared --verify --samples 1 --repeats 1
python3 tests/profile/checkpoint_builder/recheck.py --before doc/optimize/runtime/checkpoint/rt5_builder_new_run_before_completion.summary.json --after doc/optimize/runtime/checkpoint/rt5_builder_new_run_after_completion.summary.json --output doc/optimize/runtime/checkpoint/rt5_builder_new_run_recheck --samples 6
```

runner は既存の原本を上書きせず、同名のファイルがある場合はエラーにする。
途中終了した `.jsonl` は完成系列に混ぜない。

## 入力と区間

Task 数は1 / 8 / 32、保存時の Builder 長は32 / 1,024 / 16,384、payload は位置番号を持つ
Int と256-byte String。全18条件を独立 process で各5回起動する。
各 process は8反復し、各反復の準備は区間外で独立に行う。

実際の detached Task と future を作り、同じ内容を返す mapper をTaskごとに一度実行する。
各 Task は入力1件の mapper を実行済みで、output cursor の途中である。
各 Builder を指定の長さへ揃えてから checkpoint を作る。
output 全長は保存時長さ＋2,048で、測定する3回の再開では完了しない。
共有する入力 List と、Task ごとに所有する可変 Builder を区別する。

| phase | 測定範囲 |
|---|---|
| `save` | 空 chunk の構築と、元の未完了位置の checkpoint 生成 |
| `drop_unchanged` | 区間外で追加保存した未変更 checkpoint を破棄する。元 VM と最初の保存側は生存 |
| `first_resume` | 最初の checkpoint が生存する状態で全 runnable Task を再開する。各 Task を256 reduction進める |
| `second_resume` | 同じ live 状態を再開する。間に新しい checkpoint は作らない |
| `resave` | 空 chunk の構築と、2回再開した位置での次の checkpoint 生成 |
| `resume_after_resave` | 新しい保存側が生存する状態で、全 Task を再び再開する |
| `rollback` | 再保存後の進行を破棄し、2回再開後の位置へ戻す。再保存した checkpoint は消費する |
| `complete_shared` | 最初の保存位置を保持したまま、rollback後の全Taskを完成させる。結果の走査は区間外 |
| `drop_changed` | 最初の保存位置の checkpoint を破棄する。完成したlive VMは生存 |
| `unique_complete` | 前のVMを区間外で破棄し、同じ指定長へ準備した別のVMで、checkpointなしに全Taskを完成させる |

時間には scheduler の処理、通常の Builder 追加とその Value clone、表 index の COW、
可変 entry の clone、Vec 成長などが含まれる。純粋な Builder clone 時間ではない。
初回・2回目・再保存後の時間差から、コピー単独の費用を差し引いて求めない。
VM 初期化、事前実行、metrics 操作、assert、JSON 出力は区間外。
全反復の測定を終えてから JSON を出力する。

## 指標と検証

各区間に `elapsed_ns` と `cfg(test)` counters を保存する。
`copies` は `ListBuilder::clone` 回数、`copied_items` はBuilder clone時に実際にcloneしたValue件数の累積（baselineではBuilder長の累積、sharedでは0）。
`pushes` は通常の Builder 追加件数、`capacity_growths` は push による Vec capacity 増加回数である。
capacity 増加は移動 byte 数や allocator 内部の再確保回数を表さない。
`finish_copied_items` は完成継続が共有要素をcloneした件数、`finish_steps` はBuilder完成遷移の件数。
通常追加時のValue cloneはpushesとして別に数える。
この instrumentation は時間系列でも有効で、overhead を差し引かない。
製品 binary の処理時間、requested allocation、論理 live bytes、物理コピー byte 数は測らない。

`--copy-mode baseline` は旧Vecの測定期待値、`shared` はsegment共有の期待値を指定する。
これはprobeの受入検証だけに使い、製品の旧経路を残す切替ではない。
各反復でsave / drop / resave / rollbackはBuilder clone 0、初回再開と再保存後はTask数分のBuilder clone。
baselineは初回にTask数×保存時長さ、再保存後に2回の進行を含むcopied_itemsを期待する。
sharedはcopied_items 0、完成時にTask数×最初の保存時長さ分のfinish_copied_itemsを期待する。
保存なしのunique_completeはどちらもfinish_copied_items 0。
全区間でmapper callbackは0、完成する2区間だけBuilder finishがTask数となる。

runnerは10 phase×8反復の過不足、条件、counter境界を検証し、
process ごとの区間時間合計を反復数で割った値の min / median / max を集計する。
原本は各7,200行、集計は180件。交互比較は前後各6 processで17,280行、360集計となる。
AB/BAの実行順を同数にし、保存した各buildのmetadataを継承する。現在のソースをbeforeのソースとして取り直さない。
計測値に合否閾値を置かない。

`/usr/bin/time -l` の macOS maximum resident set size を byte 単位で取得する。
これは準備、全8反復・10区間、区間外のassert、JSON、test harnessを含むprocess全期間の高水位。
同じ process の各行に同じ RSS を付けるが、加算せず、区間別 peak や保持量とは読まない。
この測定コマンドは macOS 専用とし、他 OS は拒否する。

`--verify`は全18条件を別processで起動し、保存あり・保存なしの完成結果について
全要素の順序・内容、mapper 再呼出しなし、future の結果と Task の回収を確認する。
時間と RSS は保存せず、成功証明だけを別原本へ出す。性能原本へ混ぜない。

集計 metadata に HEAD、dirty status、追跡差分の hash、Eldr / Sindr のソース、
manifest / lock / binary / runner の SHA-256、toolchain と platform を保存する。
実行前後で source・binary・runner が変わったら集計を保存しない。
source hash は runner 実行時の情報であり、単独ではビルド時のソースの証明にならない。
ビルド前後にソースを変更せず、保存した binary をそのソースで測定する。

## main反映時の変更前を再現する

main `c577b32d` の別checkoutへ、このharnessと
[baseline instrumentation patch](../../../doc/optimize/runtime/checkpoint/rt5_builder_adoption_2026_10_13_baseline_instrumentation.patch)
を置く。patchは`cfg(test)`のcounter・測定関数だけを追加し、Vec版の製品挙動を変えない。
変更前のbuild・測定はそのcheckoutで行い、変更後とはtarget directoryを分ける。
[main反映と再測定](../../../doc/optimize/runtime/checkpoint/list_runtime_rt5_builder_adoption_2026_10_13.md)
にCargo artifact、source・binary hash、前後交互系列を保存している。

## 256要素segment / Packed chunkの比較

`chunk_adoption.py`は`lookup_build.py --release`で保存した2つの実VM test executableを比較する。旧Vec基準ではなく、`eec8c9b6`のRc segment版と有界chunk版の比較である。

```sh
python3 tests/profile/checkpoint_builder/chunk_adoption.py --before /absolute/before/builds.json --after /absolute/after/builds.json --output /absolute/new_chunk_run --samples 6 --repeats 8
```

1 / 32 Task、長さ1,024 / 16,384、Int / Stringの8条件。別16 processの全内容確認、前後6 sample・8反復・AB / BA同数の96時間processで、10区間と既存のlogical counterを照合する。要求allocatorは計数しない。RSSは準備・全区間・全反復・assert・JSONを含むprocess全期間peakで、特定の完成区間へ帰属させない。途中保存のValue clone 0、shared完成のclone数を維持する。完成遷移の増加は256上限の所有移譲を共通予算へ接続した結果である。[採用記録](../../../doc/optimize/runtime/list/packed_chunk_adoption_2026_10_15.md#実vmのbuilderへの影響)を参照する。
