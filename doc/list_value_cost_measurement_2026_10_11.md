# RT-3: Value と実入口の測定

測定日: 2026-10-11。製品 baseline: `adefb5fc`（main のRT-2保持方式比較を含む）。
状態: RT-3のpayload拡張と実入口の基準値測定を完了。このレポートは最適化前の結果を記録する。
以降の方式選定・製品最適化・前後比較は [最適化レポート](list_runtime_rt3_optimization_2026_10_11.md)を参照する。

入力は [2026-10-11 の測定レポート](/Users/haruca/.codex/worktrees/4c18/surtr/doc/list_runtime_measurement_report_2026_10_11.md) のRT-3。
今回の成果はlevel2相当の手動測定harnessと調査結果である。
製品コード、言語仕様、VM / schema version、通常のcorrectness testは変更していない。

## 確認したこと

- 同じB / E / Mでも、Tuple・HashMapの再帰的cloneとListのbuffer共有では割当量が異なる。幅2・深さ2の葉Stringを持つ8段singletonで、累積requestedはTuple 565.384 MiB、HashMap 842.920 MiB、List 126.694 MiBだった。累積割当量と同時保持量を分ける必要がある。
- 標準`Monad::bind`とgeneric doの実入力をcompiler全段からVMまで通し、準備・実行・破棄を分けて測れた。通常入口のrunには入力List生成と標準関数も含み、callback / flattenだけの費用は未分離である。
- Astraへの相談で、各opcode前にPendingFutureの有無によらずoperand stackを全走査する既存費用を確認した。要素をまとめてstackへ積む入力では走査要素数がΘ(n²)になる。Listの仮想計算量やValueの物理コピー量だけではrun時間を説明できない。

今回の条件内の観測結果であり、改善方式や速度・RSSの合否閾値は決めていない。
この測定では最適化を実装していない。

## 条件と原本

- Apple arm64、macOS 26.6.2、logical CPU 8、physical memory 8 GiB。
- `rustc 1.94.1 (e408947bf 2026-03-25)`、Cargo release profile、System allocator。
- 依存版とchecksumはroot lockと一致。ビルドは`--locked`で実行した。
- 手組みVMは6 workload × 2サイズ × 5 payloadの60条件、compiler入口は6 workload × 2サイズ × 2 payloadの24条件。各条件5回を独立processで実行した。
- カウンタ有効・無効の各420 process、合計840 process / 4,800区間サンプル / 960区間集計。別processの内容検証126件は性能原本へ入れていない。
- generic doの両系列は事前に標準snapshot cacheを作成し、全性能sampleで`Some(DiskHit)`を確認した。cold buildとprocess内cache hitは比較しない。
- 時間はカウンタ無効系列の中央値、割当指標は有効系列。RSSは無効系列のprocess全期間peakの中央値。全5回の最小・最大は集計に保存した。

| 系列 | 割当付き原本 / 集計 | 時間原本 / 集計 |
|---|---|---|
| 手組みVM | [raw](list_value_cost_vm_2026_10_11_count.jsonl) / [summary](list_value_cost_vm_2026_10_11_count.summary.json) | [raw](list_value_cost_vm_2026_10_11_time.jsonl) / [summary](list_value_cost_vm_2026_10_11_time.summary.json) |
| compiler入口 | [raw](list_value_cost_do_2026_10_11_count.jsonl) / [summary](list_value_cost_do_2026_10_11_count.summary.json) | [raw](list_value_cost_do_2026_10_11_time.jsonl) / [summary](list_value_cost_do_2026_10_11_time.summary.json) |

再現コマンド、各区間、counterとRSSの定義は [harness README](../tests/profile/list_value_cost/README.md)。
各summaryにbinary、harness source、lock、runnerのSHA-256とtoolchain / OSを保存した。
source commitは製品baselineを示し、今回追加した未コミットharnessはsourceのSHA-256で識別する。
requested layoutはallocatorの実消費量・RSS・Value clone回数・物理コピーbyte数を直接表さない。
reallocは新サイズ全体をrequested、旧サイズをfreedへ加える。
`peak_extra_bytes`は区間開始時の論理liveを超えた最大増分で、allocator内部の一時二重保持は未観測。
カウンタ無効系列の割当欄は`null`である。

## workloadと所有条件

| workload | n（表の条件） | B | E | 最終M | 手組みVMのcallback完了数 |
|---|---:|---:|---:|---:|---:|
| singleton | 10,000 | 10,000 | 10,000 | 10,000 | 10,000 |
| empty | 10,000 | 10,000 | 0 | 0 | 10,000 |
| huge | 10,000 | 1 | 10,000 | 10,000 | 1 |
| chain（8段） | 10,000 | 80,000 | 80,000 | 10,000 | 80,000 |
| nested（8段） | 10,000 | 8 | 80,000 | 10,000 | 8 |
| product（三段） | 20 | 8,420 | 24,000 | 8,000 | 8,420 |

小さい条件はn=1,000、productだけn=10（B=1,110 / E=3,000 / M=1,000）。
B / E / Mはworkloadから求める期待値である。
手組みVMの各mapperは一度だけReturnし、他のuser functionもTCOもないため、Return opcode数をcallback完了数の代理として照合した。
compiler入口は標準関数や生成callableも実行するのでcallbackを観測していない。
B / Eの期待式からcallbackの実測値やVM全体の操作数を補わない。

手組みVMのintはBigIntの7、stringは256-byteの`x`。
Tuple / List / HashMapは幅2・深さ2、葉は独立に構築する256-byte Stringが4個で合計1 KiB。
HashMapの各段は`key0` / `key1`を持つ。
run内でpayload templateを一度構築し、root localから各inputを取得する。
各mapperは受け取った引数localから値を返し、payloadを別のmapper captureへ保存しない。
List payloadのcloneは内部bufferを共有する。TupleとHashMapは内側も再帰的にcloneする。
したがって出力ごとの独立した1 KiBコピー量が、全payloadで同じになる条件ではない。
手組みproductは同じpayloadを増幅するモデルで、直積Tupleの構築は含まない。

compiler入口はtail recursionで1〜nのCons inputを作る。
Stringは位置番号、`:`、256個の`x`を持つ。input構築とStringの位置番号生成もrunに含む。
`singleton` / `nested` / `product`はgeneric do、`empty` / `huge` / `chain`は通常の`Monad::bind`である。
productは実際に`(x, y, z)`を構築する。
sourceの大きさはn個のliteral列挙には比例せず、nの数字だけを変える。
この系列と手組みVMは入力構築・payload・observerの条件が異なるため、両者の速度差をdo lowering単独の費用や改善率へ換算しない。

## 手組みVMの実行区間

n=10,000、productはn=20。runはboot / bytecode検証、payload templateとinput Listの生成、mapper、flatten、observerを含む。
VMとbytecodeの準備は別区間である。

| workload | payload | run ms | 累積requested MiB | runのlive増分 MiB | alloc回数 | realloc回数 | process peak RSS MiB |
|---|---|---:|---:|---:|---:|---:|---:|
| singleton | int | 97.038 | 21.073 | 4.330 | 200,026 | 25 | 8.906 |
| singleton | string | 97.753 | 30.534 | 6.695 | 200,026 | 25 | 13.484 |
| singleton | tuple | 110.265 | 90.960 | 21.803 | 440,038 | 25 | 46.672 |
| singleton | list | 96.883 | 20.770 | 4.255 | 160,041 | 25 | 8.594 |
| singleton | hashmap | 123.820 | 135.381 | 32.905 | 680,151 | 25 | 74.875 |
| empty | int | 93.082 | 14.850 | 2.129 | 150,024 | 13 | 6.312 |
| empty | string | 92.903 | 19.580 | 2.129 | 150,024 | 13 | 8.594 |
| empty | tuple | 100.085 | 49.794 | 2.130 | 270,036 | 13 | 25.203 |
| empty | list | 92.437 | 14.699 | 2.130 | 130,039 | 13 | 6.141 |
| empty | hashmap | 106.553 | 72.013 | 2.131 | 390,149 | 13 | 39.375 |
| huge | int | 82.906 | 10.040 | 4.330 | 30,043 | 25 | 8.812 |
| huge | string | 83.821 | 14.771 | 6.695 | 30,043 | 25 | 13.375 |
| huge | tuple | 92.133 | 44.988 | 21.803 | 150,067 | 25 | 46.547 |
| huge | list | 82.954 | 9.890 | 4.255 | 10,056 | 25 | 8.469 |
| huge | hashmap | 97.331 | 67.210 | 32.905 | 270,192 | 25 | 74.750 |
| chain | int | 207.023 | 128.599 | 4.330 | 1,460,131 | 109 | 10.266 |
| chain | string | 211.392 | 187.727 | 6.695 | 1,460,131 | 109 | 14.844 |
| chain | tuple | 283.062 | 565.384 | 21.803 | 2,960,143 | 109 | 54.078 |
| chain | list | 204.098 | 126.694 | 4.255 | 1,210,146 | 109 | 9.938 |
| chain | hashmap | 356.257 | 842.920 | 32.905 | 4,460,256 | 109 | 76.266 |
| nested | int | 94.372 | 40.356 | 4.340 | 100,288 | 113 | 10.219 |
| nested | string | 96.494 | 61.646 | 6.705 | 100,288 | 113 | 14.781 |
| nested | tuple | 120.752 | 197.628 | 21.813 | 640,396 | 113 | 57.109 |
| nested | list | 93.228 | 39.671 | 4.266 | 10,287 | 113 | 9.891 |
| nested | hashmap | 148.023 | 297.570 | 32.915 | 1,180,605 | 113 | 76.219 |
| product | int | 17.991 | 18.981 | 1.134 | 190,726 | 1,356 | 4.406 |
| product | string | 18.365 | 30.533 | 3.026 | 190,726 | 1,356 | 6.344 |
| product | tuple | 28.916 | 104.313 | 15.113 | 483,778 | 1,356 | 20.312 |
| product | list | 17.108 | 18.610 | 1.075 | 141,901 | 1,356 | 4.266 |
| product | hashmap | 39.191 | 158.547 | 23.994 | 776,931 | 1,356 | 32.188 |

chainとnestedはE=80,000で同じだが、B / callback数が異なる。
HashMapで累積requestedは842.920 / 297.570 MiB、runは356.257 / 148.023 msだった。
List payloadでrequestedが小さいことは内部buffer共有の所有条件と整合する。
物理clone / drop回数、callbackとflattenの時間、Vec realloc内の移動byte数を直接数えたものではない。

### 出力を保持したままVMを破棄

出力ListHandleだけを共有cloneする`extract_output`は、全条件でallocation / reallocationが0だった。
元VMを破棄し、その後に最後の出力handleを破棄した。
singleton / n=10,000の結果は以下のとおり。最後のhandle破棄のfreed量は元VM破棄のfreed量に含めない。

| payload | VM破棄のfreed MiB | 最後の出力破棄のfreed MiB | 最後の出力破棄 ms |
|---|---:|---:|---:|
| int | 3.045 | 2.201 | 0.096 |
| string | 3.046 | 4.566 | 0.122 |
| tuple | 3.048 | 19.673 | 1.264 |
| list | 3.046 | 2.127 | 0.036 |
| hashmap | 3.050 | 30.773 | 5.785 |

内容検証は別processで元VM破棄後に行い、全出力の葉・構造・HashMapのキーを照合した。
手組み入力は各要素が同一payloadなので、位置ごとの順序違反を区別する検証ではない。
位置番号を持つcompiler入口で論理順と直積のx / y / zの順を全件照合した。

## compiler入口の準備・実行・破棄

n=10,000、productはn=20。observerは無効。
setup_stdlibは毎processで既存semantic snapshotを読む費用。
compileはScar / Forge session復元、ユーザソースの全compilerフェーズ、prefix bytecode cloneとchunk合成、session破棄を含む。
VM区間の前にlocalのcompiler input / handleを落とすが、global stdlib snapshotはprocess内で保持される。
RSSはcompilerを含むprocess全期間のピークであり、VM単体の保持量ではない。

| workload | payload | setup_stdlib ms | compile ms | prepare_vm ms | run_vm ms | drop_vm ms | runの累積requested MiB | process peak RSS MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| singleton | int | 17.717 | 6.845 | 0.068 | 94.398 | 0.266 | 83.207 | 52.250 |
| singleton | string | 17.741 | 6.858 | 0.065 | 107.723 | 0.354 | 113.861 | 58.172 |
| empty | int | 17.439 | 6.623 | 0.059 | 78.354 | 0.154 | 68.897 | 51.156 |
| empty | string | 17.703 | 6.692 | 0.068 | 90.034 | 0.163 | 87.434 | 54.406 |
| huge | int | 17.643 | 6.621 | 0.059 | 69.508 | 0.257 | 64.022 | 52.281 |
| huge | string | 17.702 | 6.723 | 0.063 | 81.495 | 0.342 | 82.445 | 58.375 |
| chain | int | 17.948 | 8.639 | 0.074 | 213.367 | 0.322 | 242.326 | 62.906 |
| chain | string | 17.717 | 8.783 | 0.068 | 233.494 | 0.464 | 337.344 | 89.469 |
| nested | int | 17.906 | 8.886 | 0.066 | 81.310 | 0.261 | 94.357 | 54.938 |
| nested | string | 17.862 | 8.944 | 0.074 | 94.231 | 0.326 | 129.663 | 63.641 |
| product | int | 17.829 | 7.413 | 0.059 | 34.037 | 0.569 | 46.527 | 54.734 |
| product | string | 17.754 | 7.515 | 0.068 | 36.888 | 0.828 | 86.134 | 62.234 |

count系列のsetup_stdlibは各条件で約69.13 MiBの累積requested、約21.454 MiBのlive増分だった。
compile / VM準備 / run / VM破棄を別々に確認できたが、run内のCons input構築、標準関数、callback、flattenの寄与は未分離。
標準prefixを含む全compilerフェーズを毎回cold buildする時間としてsetupを報告しない。

## Astraへ相談した新規の費用要因

手組みhuge / intの時間系列は、n=1,000: 0.760 ms（0.750〜0.774）、n=10,000: 82.906 ms（82.439〜84.863）だった。
どちらもcallback完了数は1。nだけからflat_mapやValueの実時間計算量を推定しない。

現行 [VM](../crates/eldr/src/vm.rs) の`step_active_context`は各opcode前に`resolve_ready_pending_stack_values`を呼ぶ。
同関数はPendingFutureがない場合もstack全体を読み取り、その後の書込み側loopでも全体を走査する。
手組み入力はn個の`LoadLocal`を積んでから`ListFromItems`を実行するため、この構築部分の走査要素数はΘ(n²)になる。
これは現行VMの既存費用としてコードで確認した。測定時間の何割がこの走査かは切り分けていない。
boot / 検証 / input構築もrunに含むため、全ての時間増加を一つの原因へ帰属しない。

[OI-036](open-issues.md#oi-036-future表が非空のoperand-stack走査)に、future配送とcontext切替え等を維持する受入境界と次の検証条件を記録した。
今回この走査の最適化や言語仕様の変更は行っていない。

## 検証と次の作業

- 両release buildは`--locked`で成功。84条件 × 5回 × 2系列の測定は終了コード0で完了。
- 別processの内容検証は手組みVM 90件、compiler入口36件が成功。測定条件全部とn=1 / depth=1の境界を含む。
- `rtk cargo nextest run -p sindr -p eldr -E 'test(/list_flat_map_tests/) | (package(sindr) & test(/list|cons/))'`: 41 passed、402 skipped、失敗0。
- Astraによる独立レビューで指標、所有条件、原本の取り違え拒否、cache条件を確認した。
- 全原本のmedian / min / maxと集計、source / binary / runnerのSHA-256を照合した。手組みVMのcount系列は各processで6区間のlive delta合計が0となり、測定対象の確保と解放が釣り合った。Astraは表47行と原本の最終照合を完了し、未解決の指摘はない。
- production変更を含まないためworkspace CI gateと標準Surtrテスト全件は実行していない。Task / process、REPL checkpoint、待機・取消・scheduler公平性の性能は未測定。

基準値測定から、opcodeごとの不要なstack走査を追加保持メモリ0で減らす方式を次の改善対象とした。
その実装・検証・残る条件は [最適化レポート](list_runtime_rt3_optimization_2026_10_11.md)に記録する。
任意の深さのValue木、cross-VMコピー、実時間上限、物理コピーbyte数は今回の検証範囲に含めない。
