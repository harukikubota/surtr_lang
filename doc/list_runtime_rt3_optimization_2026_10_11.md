# RT-3: 方式選定と製品最適化

実施日: 2026-10-11。状態: Future表が空の実行で不要なstack走査を省く方式を採用し、製品実装・全体検証・前後測定を完了。
対象は指定された [測定レポート](/Users/haruca/.codex/worktrees/4c18/surtr/doc/list_runtime_measurement_report_2026_10_11.md) のRT-3と「RT-3の方式選定・製品最適化まで」という依頼。
Eldr内の局所修正（level2）で、言語の意味論・VM / schema versionは維持する。

## 結果と実装

手組みVMのhuge / int / n=10,000は、run中央値が82.906から3.443 msになった（前 / 後: 24.08倍）。
全84条件でrun中央値が改善し、手組みVMは1.024〜25.383倍、compiler入口は1.039〜1.086倍だった。
5回の時間範囲が前後で重ならない改善は76条件で、中央値の悪化はなかった。
この倍率は今回の入力・環境・区間に限る。通常入口のrunには入力生成と標準関数も含む。

`VM::resolve_ready_pending_stack_values` に次の条件を追加した。

1. Future表が空なら、operand stackの読取り・書込み走査を省く。
2. 表が非空なら、従来のcleanup完了条件を含むready判定で解決値を収集する。
3. 解決値が空なら、書込み側の全走査を省く。
4. 解決値があれば、同じFutureを持つ全slotを従来どおり置換する。

Future表が値の唯一の供給元であることを根拠とし、追加の追跡状態を持たない。
新しい表・件数・予約容量による保持量は0。VM区間の割当量・論理live・peakも前後で一致した。
ただしprocess peak RSSはHashMapのchain / nestedの4条件で、n=1,000は約2.2 MiB、n=10,000は約22.4 MiB増えた。
**追加状態がないことと、OSが観測するRSSが増えないことは別の条件である。RSS非増加は確認できていない。**

比較した方式と不採用の理由は [実装計画](list_runtime_rt3_optimization_plan.md) に記録した。
Tuple / HashMap共有化、PendingFuture slotの常時追跡、Builder reserveは今回導入していない。
Future表が非空の場合の読取り全走査は [OI-036](open-issues.md#oi-036-future表が非空のoperand-stack走査) に残る。

## 前後の識別と測定条件

基準commitはmainのRT-2対応を含む `adefb5fc1321e8fadbeda3a26ca9e025dd6d7278`。
測定時の最適化後は同じcommitに未コミットの製品変更を加えた状態であり、summaryの`source_commit`だけでは前後を識別できない。

| 対象 | SHA-256 |
|---|---|
| 基準 `crates/eldr/src/vm.rs` | `8094854c162c47fa538b75374fe0c4b1139f7a70c241d8626f4193795e11174e` |
| 最適化後 `crates/eldr/src/vm.rs` | `d62d2bedc2256d6558f66fcd65465f845e530c939da106f53dd80a99f6954050` |
| `git diff -- crates/eldr/src/vm.rs` | `21c02d2fbb07b2e04eb4de078d2dad4ad00f1e8e71b3237bcbfa14ae00d6df59` |

両系列のharness Rust source・runner・lockのSHAは一致する。binaryのSHAとビルド先は各summaryに保存した。
Apple arm64 / macOS 26.6.2 / rustc 1.94.1、release / System allocator / `--locked`。
手組みVM60条件、compiler入口24条件を各5回、割当カウンタ有効・無効に分けて測定した。
前後それぞれ840 process / 4,800区間サンプル / 960区間集計。内容検証は別processの126件ずつ。
generic doの性能sampleは全て`Some(DiskHit)`。baseline binary・原本は上書きしていない。

時間はカウンタ無効系列、割当指標は有効系列の中央値を使う。
RSSは無効系列のprocess全期間peakであり、compiler入口ではcompiler準備も含む。
allocatorのrequested layout、論理live、RSS、Valueの物理clone / drop回数を同じ指標として扱わない。
区間・payload・所有条件と基準原本は [基準値レポート](list_value_cost_measurement_2026_10_11.md)、再現手順は [harness README](../tests/profile/list_value_cost/README.md) を参照。

| 最適化後の系列 | 割当付き原本 / 集計 | 時間原本 / 集計 |
|---|---|---|
| 手組みVM | [raw](list_value_cost_vm_2026_10_11_optimized_count.jsonl) / [summary](list_value_cost_vm_2026_10_11_optimized_count.summary.json) | [raw](list_value_cost_vm_2026_10_11_optimized_time.jsonl) / [summary](list_value_cost_vm_2026_10_11_optimized_time.summary.json) |
| compiler入口 | [raw](list_value_cost_do_2026_10_11_optimized_count.jsonl) / [summary](list_value_cost_do_2026_10_11_optimized_count.summary.json) | [raw](list_value_cost_do_2026_10_11_optimized_time.jsonl) / [summary](list_value_cost_do_2026_10_11_optimized_time.summary.json) |

## 手組みVMの前後比較

n=10,000、productはn=20。runはbytecode検証・入力生成・mapper・flatten・observerを含む。

| workload | payload | 前 ms | 後 ms | 前 / 後 | 前 RSS MiB | 後 RSS MiB |
|---|---|---:|---:|---:|---:|---:|
| singleton | int | 97.038 | 17.180 | 5.65 | 8.906 | 8.875 |
| singleton | string | 97.753 | 17.783 | 5.50 | 13.484 | 13.438 |
| singleton | tuple | 110.265 | 29.433 | 3.75 | 46.672 | 46.609 |
| singleton | list | 96.883 | 16.464 | 5.88 | 8.594 | 8.531 |
| singleton | hashmap | 123.820 | 40.968 | 3.02 | 74.875 | 74.828 |
| empty | int | 93.082 | 13.238 | 7.03 | 6.312 | 6.297 |
| empty | string | 92.903 | 13.338 | 6.97 | 8.594 | 8.547 |
| empty | tuple | 100.085 | 19.646 | 5.09 | 25.203 | 25.156 |
| empty | list | 92.437 | 12.651 | 7.31 | 6.141 | 6.109 |
| empty | hashmap | 106.553 | 26.835 | 3.97 | 39.375 | 39.344 |
| huge | int | 82.906 | 3.443 | 24.08 | 8.812 | 8.797 |
| huge | string | 83.821 | 3.881 | 21.60 | 13.375 | 13.344 |
| huge | tuple | 92.133 | 10.903 | 8.45 | 46.547 | 46.531 |
| huge | list | 82.954 | 3.268 | 25.38 | 8.469 | 8.438 |
| huge | hashmap | 97.331 | 17.565 | 5.54 | 74.750 | 74.766 |
| chain | int | 207.023 | 124.986 | 1.66 | 10.266 | 10.359 |
| chain | string | 211.392 | 126.337 | 1.67 | 14.844 | 14.812 |
| chain | tuple | 283.062 | 199.162 | 1.42 | 54.078 | 54.016 |
| chain | list | 204.098 | 119.398 | 1.71 | 9.938 | 9.891 |
| chain | hashmap | 356.257 | 268.232 | 1.33 | 76.266 | 98.641 |
| nested | int | 94.372 | 14.857 | 6.35 | 10.219 | 10.328 |
| nested | string | 96.494 | 16.008 | 6.03 | 14.781 | 14.766 |
| nested | tuple | 120.752 | 39.730 | 3.04 | 57.109 | 54.016 |
| nested | list | 93.228 | 13.629 | 6.84 | 9.891 | 9.844 |
| nested | hashmap | 148.023 | 69.151 | 2.14 | 76.219 | 98.609 |
| product | int | 17.991 | 17.081 | 1.05 | 4.406 | 4.359 |
| product | string | 18.365 | 17.640 | 1.04 | 6.344 | 6.281 |
| product | tuple | 28.916 | 27.716 | 1.04 | 20.312 | 20.234 |
| product | list | 17.108 | 16.135 | 1.06 | 4.266 | 4.219 |
| product | hashmap | 39.191 | 38.141 | 1.03 | 32.188 | 32.281 |

huge / intのn=1,000は0.760（0.750〜0.774）から0.379（0.370〜0.408）ms、n=10,000は82.906から3.443 msになった。
両サイズのcallback完了数は1。最適化前の不要な走査要素数Θ(n²)を、空Future表では0にした。
実行全体の計算量や、時間の全てをstack走査へ帰属する主張ではない。

## compiler入口の前後比較

n=10,000、productはn=20。observerは無効。実際のgeneric do / 標準 `Monad::bind` をcompiler全段からVMへ通した。
手組みVMと入力・payload・callback観測条件が違うため、両系列の倍率をdo lowering単独の改善率へ換算しない。

| workload | payload | 前 ms | 後 ms | 前 / 後 | 前 RSS MiB | 後 RSS MiB |
|---|---|---:|---:|---:|---:|---:|
| singleton | int | 94.398 | 87.847 | 1.07 | 52.250 | 52.125 |
| singleton | string | 107.723 | 99.329 | 1.08 | 58.172 | 58.125 |
| empty | int | 78.354 | 72.696 | 1.08 | 51.156 | 51.047 |
| empty | string | 90.034 | 84.265 | 1.07 | 54.406 | 54.094 |
| huge | int | 69.508 | 64.002 | 1.09 | 52.281 | 52.359 |
| huge | string | 81.495 | 75.058 | 1.09 | 58.375 | 58.109 |
| chain | int | 213.367 | 203.072 | 1.05 | 62.906 | 63.078 |
| chain | string | 233.494 | 224.231 | 1.04 | 89.469 | 89.672 |
| nested | int | 81.310 | 75.040 | 1.08 | 54.938 | 55.172 |
| nested | string | 94.231 | 88.078 | 1.07 | 63.641 | 63.844 |
| product | int | 34.037 | 32.091 | 1.06 | 54.734 | 54.719 |
| product | string | 36.888 | 35.336 | 1.04 | 62.234 | 62.016 |

## 割当量と内容の照合

- 手組みVM: 全60条件の全6区間について、allocation / reallocation / requested / freed / live_delta / peak_extraのmin / median / maxが前後で一致した。
- compiler入口: 全24条件のprepare_vm / run_vm / drop_vmで同じ6指標の全統計が一致した。
- compiler準備: setup_stdlib / compileの一時的なalloc / requested / freedには小さい差とsample内の変動がある。live_deltaは全統計で一致し、compileのpeak_extraも一致した。setup_stdlibのpeak_extraは全条件で20 bytes増えた。ビルド先pathが10 bytes長いことと整合するが、原因を実測で確定していない。
- B / E / M、手組みcallback完了数、payloadの構造、compiler入口のsource hash / bytecode opcode・function数は全4系列で不変だった。
- 手組みVMは元VMの破棄後、compiler入口は位置番号と直積順を含む全出力を別processで照合した。最適化後もVM90件・compiler入口36件が成功した。

別サブエージェントが8原本と8集計の1,920区間について27,840個の数値集計を再計算し、全て一致した。
時間やRSSの閾値はcorrectness testへ加えていない。

## RSS差異の相談と追加診断

HashMapのchainとnestedで、counter無効のRSS中央値がn=1,000では約2.2 MiB増え、n=10,000では約76.2から98.6 MiBへ増えた。
元の5sample範囲は前後で分離している。一方counter有効系列では約76.1 MiBで前後ほぼ同じだった。
この差をAstraへ相談し、既存binaryを交互に5回ずつ実行する20 processの追加診断を行った。
原本は [RSS追加診断](list_value_cost_rss_diagnostic_2026_10_11.jsonl) に保存した。
chainでは同じ差が再現し、nestedの基準binaryでも1回98.625 MiBが観測された。
論理保持量が一致することだけでは、allocatorの配置・page保持・RSSの原因を確定できない。

さらに両binaryへ `MallocReportConfig=1 MallocDebugReport=stderr` を設定した2 processでallocator構成を確認した。
[構成診断原本](list_value_cost_allocator_config_2026_10_11.json) では、両方とも `Enable Xzone Malloc: 1`、`Enable Nano On Xzone: 0`、`Pointer Bucket Count: 4` だった。
これは性能sampleに含めていない。今回のRSS差の原因は未確定で、[OI-037](open-issues.md#oi-037-rt-3のhashmap測定におけるprocess-rss差) に追跡条件を残す。

Astraは、計画で具体化した追加保持状態0とVM区間の割当・live・peak不変を受入として、今回の方式を採用できると判断した。
RSS増加を測定誤差として除外せず、実測した不利益として残す。RSS非増加まで含む「メモリ増加なし」とは報告しない。

## 検証

製品ソースを固定した状態で順に実行した。

| 検証 | 結果 |
|---|---|
| 追加テストのRed: `rtk cargo nextest run -p eldr future_stack_tests` | 3 passed / 2 failed。空Future表の走査と解決値なしの書込み走査を旧実装が行うことによる、意図した失敗 |
| Green: `rtk cargo nextest run -p eldr` | 322 passed |
| 全体gate: `rtk cargo nextest run --profile ci --workspace` | 69 binaries / 2,500 passed、exit 0 |
| 標準テスト: `rtk proxy cargo run -- test --quiet --all` | exit 0（quiet出力に件数表示なし） |
| 最適化後の別process内容検証 | VM 90 / compiler入口36 passed |

追加5テストで深いstackの走査数、Running / Ready / Cancelled、複数slot、context切替え、checkpoint / rollbackを固定した。
cleanup完了前の配送抑制は既存テストで確認し、全体gateに含めた。
Astraは採用した条件の意味論・追加状態なし・既存配送処理の維持と、RSS差異の限定した受入を確認した。最終差分・原本・文書の独立レビューを完了し、指摘を解消した。

## 改善しない範囲

Future表が非空なら、PendingFutureを持たないstackでも読取り全走査が残る（OI-036）。
Valueの再帰clone / drop、List buffer共有、Vec再確保、allocatorのpage保持は今回の最適化で変えていない。
これらの一般的な費用、実時間の上限、RSS非増加を保証するものではない。
RT-2の保持方式、RT-4の未分割builtin、RT-5のcheckpointコピーも今回の対象外である。
