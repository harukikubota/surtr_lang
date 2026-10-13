> 取り込み元: `/Users/haruca/.codex/worktrees/4c18/surtr`（detached、HEAD `ee13e448`）。資料の集約。製品変更を伴わない基準測定。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# List runtime の測定と次の作業

測定日: 2026-10-11。対象 commit: `ee13e44820993fcfc56ecfa216ce4165be67f4ca`。
状態: 現行基準値の測定と調査を完了。最適化の実装は未着手。

入力は [List / VM の残課題](../list_runtime_followups.md)。採用方式が未確定のため、
この作業では既存契約の測定と、次の方式選定に必要な条件を具体化した。
製品コード、言語仕様、VM/schema の version、通常の correctness test は変更していない。
追加した Rust harness は公開 API で動く手動 profile 用で、局所の計測作業（level2 相当）に閉じる。
runtime の内部表現や継続を変える実装の level は、方式確定時に改めて判断する。

## 結果から分かったこと

- **RT-2**: 長さ1の suffix でも元の全 Packed buffer を保持する。10万個の256-byte Stringでは、元 handle を落とした後も37.384 MiBの requested allocation が残った。最後の tail の破棄で解放された。
- **RT-3**: Vec を一回構築するモデルは二段の Cons 構築より割当回数と時間が小さかった。ただしこれは構築 kernel の比較であり、旧 VM 全体に対する改善率ではない。実 VM では、同じ E でも callback 数等の異なる workload で費用が違う。その寄与はまだ分離していない。
- **RT-4**: `Regex::find_all` の n×m 入力コピーを実 VM で測れた。HashMap 構築の全 clone、`map_values` の前処理/再開時コピー、FileOutHandler の正本との差異はコードで確認し、Astra へ相談した。
- **RT-5**: 現行の5共有表と残るコピー箇所を確認し、保存・初回変更・破棄/rollback・実 chunk 全体を分ける測定計画を保存した。実測はまだ行っていない。

これらの性能値は各条件5回の観測範囲である。速度・RSS の合否基準や、改善方式の採用には用いていない。

## 測定条件と再現方法

- Apple arm64、macOS、8 logical CPU、physical memory 8 GiB。
- `rustc 1.94.1 (e408947bf 2026-03-25)`、Cargo release profile、System allocator。
- profile package の lock は root lock を基に、同版依存の不要項目を除いたもの。ビルドは `--locked` で実行した。
- 42条件を独立した process で各5回実行し、割当カウンタ有効・無効の2系列を採った。各系列は210 process、460区間サンプル、92区間集計。両系列を合わせて420 process。
- workloadの事前準備（bytecode/VM等）、JSONの整形・出力、`ps`、compile/build は区間の外に置く。実際の処理は区間内で測る。各 workload がどこまで入力を作るかは [harness README](/Users/haruca/.codex/worktrees/4c18/surtr/tests/profile/list_runtime/README.md)に記載した。
- RT-3の実VM系列では observer は有効、opcode/call trace は無効。同じ条件で比較するが、opcode 集計の overhead は run に残る。regex系列では observer を有効にしない。
- 以下の時間は allocator counter 無効系列の中央値。括弧や範囲を付ける箇所は5回の最小〜最大。割当指標は counter 有効系列。表のRSSはcounter無効系列の各process全期間のpeakの中央値。

再現コマンドは [README](/Users/haruca/.codex/worktrees/4c18/surtr/tests/profile/list_runtime/README.md)。
実測の原本は [割当付き生データ](list_runtime_measurements_2026_10_11.jsonl)・[集計](list_runtime_measurements_2026_10_11.summary.json)、
[時間系列の生データ](list_runtime_measurements_2026_10_11_time.jsonl)・[集計](list_runtime_measurements_2026_10_11_time.summary.json)。
集計の metadata に binary、harness source、lock の SHA-256 と toolchain / OS を保存した。

requested bytes は allocation layout の要求量であり、allocator の実消費量や RSS ではない。
realloc は新サイズ全体を累積 requested に加え、旧サイズを freed に加える。
実際の移動/コピー byte 数、allocator 内部で旧・新 buffer を同時保持するピークは取得していない。
`peak_extra_bytes` は区間開始時の論理 live を超えた最大増分で、process 全体の peak RSS とも異なる。
カウンタ無効系列の割当欄の0は未測定を表す。

## RT-2: 短い Packed tail の保持

n=1万 / 10万、Int(7) と256-byte Stringを独立に作り、`tail_handle` を繰り返して長さ1を残した。
元の List と最後の tail の破棄を分けた。advance 区間に要素の読み取り・clone は入れない。

| 元の要素数 | payload | build の live増分 MiB | root破棄の freed bytes | 最後のtail破棄の freed MiB | 最後のtail破棄 ms | process peak RSS MiB |
|---:|---|---:|---:|---:|---:|---:|
| 10,000 | int | 1.602 | 0 | 1.602 | 0.166 | 3.859 |
| 100,000 | int | 16.022 | 0 | 16.022 | 1.596 | 18.297 |
| 10,000 | string | 3.738 | 0 | 3.738 | 0.127 | 6.000 |
| 100,000 | string | 37.384 | 0 | 37.384 | 2.700 | 39.766 |

全系列で advance の allocation/reallocation は0、root破棄での freed bytes も0だった。
最後の tail の破棄では build 時に増えた allocation 全体が解放された。
root破棄の時間は時計分解能付近なので、定数時間や速度差の根拠にはしない。
最後の解放時間も payload と allocator の条件に依存する。

次は chunk、明示的 compaction、共有継続を同じ入力で比較する。
複数の元 List / suffix / checkpoint が共有する場合と、固定深さの入れ子 payload を加える。
採用方式は仮想 O(1) の `cons` / `head` / `tail` / `uncons` / `len` を維持し、
tail ごとの全コピーを導入しない。改善しない保持ケースも記録する。

## RT-3: 構築 kernel と実 VM

### 構築 kernel の比較

入力 Packed は測定前に作る。packed モデルは Vec の `try_reserve(1)` と push を繰り返して
`from_items` に所有権を移す。cons モデルは逆順・正順の二段構築を行い、中間逆順 List の解放を build に含む。
private ListBuilder や削除済みの VM 経路を復活させたものではない。callback と VM はこの比較に入らない。

| n=100,000 | 方式 | build ms | alloc回数 | realloc回数 | 累積requested MiB | buildのlive増分 MiB | process peak RSS MiB |
|---|---|---:|---:|---:|---:|---:|---:|
| int | packed | 3.822 | 100,002 | 15 | 34.762 | 17.763 | 33.969 |
| int | cons | 14.527 | 400,000 | 0 | 36.621 | 18.311 | 58.297 |
| string | packed | 6.082 | 100,002 | 15 | 58.414 | 41.414 | 78.359 |
| string | cons | 21.879 | 400,000 | 0 | 83.923 | 41.962 | 125.703 |

packed の alloc 回数は n個のpayloadとVec/Rc、consは二回のpayload取得と2n個のConsが主体になる。
Int は BigInt であり、小さい値でも無割当ではない。これは allocator 経由の集計で、
全ての Value clone 回数や物理コピー byte 数を直接測ったものではない。

### 実 VM の workload

canonical builtin ID を metadata から取得し、手で構築した bytecode を `VM::run` で実行した。
run は boot / bytecode検証、入力 List生成、mapper、flat_map、observer を含み、VM構築は含まない。
drop_vm は出力だけでなく、bytecode・定数・入力・observer等のVM所有物全体を含む。
compiler全段、標準Monad::bindとgeneric do lowering、REPL、待機・取消・schedulerの公平性はこの系列では確認しない。

B は全入力訪問数、E は全Builder追加数。これらはworkloadから求める期待値である。
callbackの完了数は各mapperが一度実行するReturn opcode数で確認した。
この成功workloadには他のuser functionもTCOもないため成立する代理指標であり、一般のVMのcallbackカウンタではない。

| payload | workload | 最終出力M | B | E | callback完了数 | run ms（最小〜最大） | 累積requested MiB | process peak RSS MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| int | singleton | 10,000 | 10,000 | 10,000 | 10,000 | 101.236（97.681〜103.768） | 21.073 | 8.953 |
| int | empty | 0 | 10,000 | 0 | 10,000 | 94.452（92.577〜97.583） | 14.850 | 6.359 |
| int | huge | 10,000 | 1 | 10,000 | 1 | 85.612（82.095〜94.664） | 10.040 | 9.953 |
| int | chain | 10,000 | 80,000 | 80,000 | 80,000 | 230.955（209.963〜260.612） | 128.599 | 10.469 |
| int | nested | 10,000 | 8 | 80,000 | 8 | 93.784（93.639〜94.107） | 40.356 | 11.531 |
| int | product | 8,000 | 8,420 | 24,000 | 8,420 | 17.750（17.634〜17.820） | 18.981 | 4.469 |
| string | singleton | 10,000 | 10,000 | 10,000 | 10,000 | 97.481（96.677〜99.495） | 30.534 | 13.547 |
| string | empty | 0 | 10,000 | 0 | 10,000 | 96.112（92.838〜98.100） | 19.580 | 8.641 |
| string | huge | 10,000 | 1 | 10,000 | 1 | 85.550（82.767〜88.080） | 14.771 | 14.516 |
| string | chain | 10,000 | 80,000 | 80,000 | 80,000 | 212.289（209.779〜221.380） | 187.727 | 14.891 |
| string | nested | 10,000 | 8 | 80,000 | 8 | 94.924（94.634〜100.495） | 61.646 | 15.922 |
| string | product | 8,000 | 8,420 | 24,000 | 8,420 | 18.183（18.155〜18.265） | 30.532 | 6.391 |

- singleton / empty は1万入力から、入力ごとにsingleton / 空Listを返す。
- huge は1入力から1万要素を返す。mapperによる巨大なList生成もrunに含む。
- chain は1万要素を8段のsingleton bindで順に変換する。B=80,000 / E=80,000。
- nested は8段のsingleton bindを入れ子にし、末尾で1万要素を作る。B=8 / E=80,000。
- product は各段20入力の三段bind増幅モデル。同じ定数payloadを返すため、直積のTupleを構築する費用は含まない。

n=10のproductはB=1,110 / E=3,000 / M=1,000となり、callback完了数も1,110だった。
B / E と物理費用を分けるための基準値を得たが、何が実利用で支配的かはまだ断定しない。
次はTuple・List・入れ子のHashMap等のpayloadと、実際のgeneric do入力を追加し、
入力構築・VM準備・callback・flatten・解放の時間を必要な入口で切り分ける。
allocatorの計測overheadを差し引いた値を真の処理時間として扱わない。

## RT-4: Regex の保持量と新規の相談結果

pattern="a"、input="a"をn個並べた場合、match数m=nとなる。対照の"a+"はm=1。
実VMでcompileとfind_allを実行し、結果を生存させた。`compile_cached_regex`は毎回Regex::newを呼ぶため、
両方の正規表現コンパイルをrunに含む。結果の全走査は`PROBE_VERIFY=1`の別processで検証し、
性能系列へ混ぜていない。

| n bytes | pattern | matches | input保存量の期待値 MiB | runのlive増分 MiB | run ms | process peak RSS MiB |
|---:|---|---:|---:|---:|---:|---:|
| 1,024 | a | 1,024 | 1.000 | 1.136 | 0.259 | 4.406 |
| 1,024 | a+ | 1 | 0.001 | 0.005 | 0.092 | 3.156 |
| 2,048 | a | 2,048 | 4.000 | 4.269 | 0.610 | 7.531 |
| 2,048 | a+ | 1 | 0.002 | 0.006 | 0.124 | 3.156 |
| 4,096 | a | 4,096 | 16.000 | 16.534 | 1.890 | 19.859 |
| 4,096 | a+ | 1 | 0.004 | 0.007 | 0.214 | 3.188 |

密一致では保存文字列だけで1 / 4 / 16 MiB、live増分はこれにVecやVM等の保持量が加わる。
最後のVM破棄のfreed量も生データへ保存した。1match対照のlive増分は小さい。
この増加は現行コードのn×m Stringコピーと整合する。直近変更のデグレという判定はしていない。
入力共有や一致部分だけの保持を採用するかは未確定であり、UTF-8、空match、元入力の破棄後の有効性、
start/endのbyte offset、不正spanの拒否を保つ受入仕様が必要になる。

[RT-4棚卸し](list_runtime_rt4_inventory.md)は入力サイズに比例する処理と外部呼出しを整理した。
[Astraレビュー](list_runtime_astra_review.md)ではHashMapの異なるn keyの構築が既存entryを
n(n−1)/2回cloneすること、map_valuesの開始時clone/sortと各resumeのoutput全cloneを確認した。
これらの時間・割当量はまだ測定していない。

FileOutHandlerは正本のinit時open・lifecycle保持・shutdown時flush/closeに対し、
現行コードはwriteごとの局所open/dropである。これは分割方式だけでは解決しない契約不一致で、
最小再現の実行は未実施。[未解決事項](../../../open-issues.md)へ記録した。正本を現行コードに合わせる変更は行っていない。

## 検証と残作業

- count有効/無効のrelease buildは`--locked`で成功。各42条件×5回の測定は終了コード0で完了。
- 独立Astraレビューで検証用cloneによるRSS汚染とcallback観測指標を修正した。修正後の測定値だけを原本へ採用した。
- `PROBE_VERIFY=1`の別process10件（10万要素kernelの両payload/両方式、regexの全6条件）で内容確認に成功。これらの出力は性能原本に含めていない。
- `rtk cargo nextest run -p sindr -p eldr -E 'test(/list_flat_map_tests/) | (package(sindr) & test(/list|cons/)) | test(/regex_capture_access_and_find_helpers_work/)'`: 42 passed、401 skipped、失敗0。Listの永続性/解放、flat_mapの予算・待機・取消・checkpoint、regex補助APIの既存契約を選択した。
- production変更を含まないためworkspace CI gateと標準Surtrテスト全件は実行しない。compiler/generic do/REPL/Task/processの入口全体の性能や実時間上限は未検証。

RT-2〜5はいずれも未解決である。次はRT-2の保持方式比較、RT-3のpayloadと実入口の切り分け、
RT-4の改善対象/一単位の処理量、RT-5のcrate-local計測を順に具体化する。
この測定harnessや構築モデルを製品の旧経路として使わない。
