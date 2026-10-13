> 取り込み元: `/Users/haruca/.codex/worktrees/9ea8/surtr`（codex/rt5-builder-checkpoint-measurement、HEAD `72154707`）。資料の集約。記載された製品変更・harnessはmain未統合。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# OI-039: Builderの通常経路とIntのRSS差

入力は [OI-039](../../../open-issues.md#oi-039-builderコピー削減後の通常経路とintのrss差) と [前回の採用結果](list_runtime_rt5_builder_optimization_2026_10_12.md)。通常経路の追加費用とメモリ差の寄与を分離し、原因を確認した局所的な改善を行う。level2とし、保存位置、再開枝の独立性、完成途中のcheckpoint、予算、結果配送、取消を維持する。言語仕様・公開API・List表現は変更しない。

## 調査と受入条件

- 通常経路は `unique_complete` の前後binary比較を使う。通常のBuilder追加にも実行するVMのListFinish判定と、Builderの所有確認を区別する。既存原本を上書きしない。
- 完成時のCons構築について、製品Builderを直接取り込む独立probeでrequested allocation、解放量、論理liveの増減を測る。count有効の時間を性能値に混ぜず、count無効のprocess RSSと区別する。準備・完成・保存状態の破棄・結果の破棄を分ける。
- 単なる構造上のallocationからRSS差の原因を断定しない。保存なし、保存あり、反復と単発を分け、測定していないcompiler/REPL全体へ外挿しない。
- 全Vec clone、無課金finish、unsafeな所有確認回避、製品の方式切替を追加しない。より広い表現変更が必要な場合は残件とする。

## 手順

1. サブエージェントと現行実装・原本・コンパイル結果を確認し、原因仮説を固定する。
2. 一つの寄与だけを変えたbinaryを保存し、前後を同じ入力で交互測定する。測定中はbuild/testを重ねない。
3. 局所改善を採用する場合、既存の所有・予算・取消・rollbackテストを確認する。速度閾値や実装を写したcounterテストは追加しない。
4. requested/live/RSSの測定と性能差をAstraへ渡して独立レビューを受ける。採用版を全体検証し、解決した範囲と残件を記録する。

## 結果

通常経路には軽微な改善傾向があり、Builder追加のinline化と容量条件の明示を採用した。対象の大きいInt条件の遅化解消は確認できず、OI-039は未解決のまま残す。共有prefix完成時の割当、完成後のrequested-live、解放後の現在RSSを分けて測り、process peak RSSだけから保持量やリークを推定しない。

### 通常経路の寄与分離

前回の採用binaryを基準として、変更を分けた3版を保存した。全版で所有表現、Value clone件数、予算、配送境界は同じである。

1. **VMの判定順だけを変更**: ListFinish判定を先に置いた。生成コードでは通常のFlatMap Resumeにもcancellationの読取が残った。交互測定の対象Intは12.269503→12.393995 ms、Stringは13.501307→13.688297 msで、観測範囲が重なった。改善を確認できず、この変更は撤回した。
2. **Builder pushのinline化だけ**: FlatMapからpushへの呼出しは消えたが、容量に余裕があっても予約関数を呼ぶ生成コードになった。Valueのstackコピーも残った。単独版は採用しない。
3. **inline化と容量条件の明示**: 独占bufferの `len == capacity` の場合だけ `try_reserve(1)` を呼ぶ。容量不足時のRuntimeError変換、総長のchecked_add、Rcの所有確認、共有枝の分離を維持した。生成コードでも容量に余裕がある場合のreserve/grow呼出しを省く。所有状態や方式切替は追加していない。

[採用版との全条件交互比較](oi039_2026_10_12_capacity_compare.jsonl)・[集計](oi039_2026_10_12_capacity_compare.summary.json)。Task数1/8/32、長さ32/1,024/16,384、Int/Stringの18条件、各版6 process、AB/BA同数、各process 8反復。17,280区間、360集計。測定中はbuild/testを重ねていない。

通常の `unique_complete` は18条件中17条件で中央値が下がり、対応する108組中86組で短かった。大きい条件の中央値と観測範囲は次のとおり。単位はms、32 Task / L=16,384。

| payload | 前回採用版中央値（範囲） | 今回採用版中央値（範囲） | 差 |
|---|---:|---:|---:|
| Int | 12.421253（12.219240〜12.611823） | 12.454167（12.058281〜12.517339） | +0.265% |
| String | 13.169023（13.031588〜13.802766） | 12.939667（12.813656〜13.119646） | -1.742% |

Stringは6組すべてで短かったが、観測範囲は重なる。Intは4組で短くても中央値は増えており、対象の遅化が解消したとはしない。これらは前回のRc segment版に対する差であり、旧Vec版への改善率ではない。ASM上の呼出し削減と時間差を区別する。

前回採用版・各候補binary、build JSON、ビルド前後のsource snapshot、disassemblyは `/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-9ea8-oi039-h7zpd3bk/` に保存した。`asm-evidence.md` に再現コマンドとaddressを記載した。42のEldr/Sindrソースとmanifest/lock、runner、binaryのhashを保持し、採用候補のsource差は `list_builder.rs` だけと照合した。

候補の[通常測定](oi039_2026_10_12_capacity.summary.json)は全18条件・各5 process、7,200区間、180集計。[判定順版の交互原本](oi039_2026_10_12_guard_compare.jsonl)・[集計](oi039_2026_10_12_guard_compare.summary.json) と [inline単独版の交互原本](oi039_2026_10_12_inline_compare.jsonl)・[集計](oi039_2026_10_12_inline_compare.summary.json) は各2条件・各版6 process、1,920区間、40集計であり、全条件比較ではない。中間版の原本は採用値に混ぜない。

### 割当と解放の分離

[独立probeの再現方法](/Users/haruca/.codex/worktrees/9ea8/surtr/tests/profile/checkpoint_builder_cost/README.md)。現行の製品Builderを直接取り込み、同じbinaryに旧main `72154707` のBuilderを履歴fixtureとして保持した。製品には旧経路を追加しない。両者が同じimmutable Packed入力を使い、保存保持、完成前の保存解放、保存なしの3条件を別processで測る。準備、追加、完成、保存・出力・入力の破棄を別区間にした。

requested/freedとrequested-liveはSystem allocatorの要求layoutで数える。counter有効系列の時間は性能比較に使わない。現在RSSとprocess peakはcounter無効の別processで測る。現在RSSは各区間終了時に所有物を保持したまま停止して `ps` から取得し、区間内peakとはしない。全期間peakにはbarrier、準備、全反復、assert、JSON、解放も含む。registryの全31 packageの版・checksumは本体lockと一致させた。

このprobeにはVMの表index、entry COW、scheduler、Future配送を含めない。最初のdirect flowでは旧Builder cloneがsave/resaveで起き、実VMの初回変更時とは時刻が異なる。追加区間には入力開始位置までのtail走査とValue取得も含む。kernelの結果を実VMのInt peak RSS増22.7 MiBの原因・量と同一視しない。metadata.scopeの「direct Builder kernels」はBuilderを直接操作する意味であり、`checkpoint_flow=direct` だけを指していない。

最初の [direct系列原本](oi039_2026_10_12_cost_final.jsonl)・[集計](oi039_2026_10_12_cost_final.summary.json) は9,792区間、544集計、192 process。32 Builder / L=16,384、Int/String、3所有条件、1/8反復、各4 process、counter有/無を分けた。割当を数えた96 process・432反復すべてで全破棄後のrequested-liveが開始値へ厳密に戻った。[内容検証](oi039_2026_10_12_cost_verify_final.jsonl)・[記録](oi039_2026_10_12_cost_verify_final.summary.json) は12条件で全要素の位置・順序を確認した。

direct系列はcloneしたBuilderを保存し、元のBuilderを進める条件である。実VMは元entryを保存してcloneしたlive枝を進めるため、旧Vecのcapacityの配置が異なる。directの旧版はrollback後の追加でもVecを再確保し、Intの追加区間で約140.625 MiBをrequestedへ加える。これには通常の入力Value cloneも含む。この差を実VMの費用と扱わない。元のsource/runnerは退避先の `kernel_direct_source/` に保持する。

directの割当系列で、保存を保持した条件の値は次のとおり。単位はMiB。最大liveは各反復のinput準備前からのrequested-live増分であり、process RSSではない。1/8反復と全sampleで値は同一だった。

| payload | 完成時requested（旧→今回） | 全区間の最大live（旧→今回） |
|---|---:|---:|
| Int | 0.002197→96.001953 | 289.392098→179.957771 |
| String | 0.002197→220.002197 | 675.017860→447.395790 |

保存を完成前に破棄した今回版でも、完成時に両payloadで92.002197 MiBをrequestedへ加える。Cons構築は残り、独占prefixを解放するため同区間のfreedは68.003662 MiBとなった。保存保持時との差はInt 3.999756 MiB、String 128 MiBで、共有Valueのcloneに対応する。通常の保存なし完成は旧/今回ともrequested 2,304 bytes、33 allocation、reallocation 0だった。

count無効のdirect系列で、保存を保持したIntのRSS中央値は次のとおり。各4 process、単位はMiB。「全破棄後」は最終反復のdrop_input終了位置。全期間peakと同じ位置の値ではない。

| 反復数 | 全破棄後の現在RSS（旧→今回） | process全期間peak（旧→今回） |
|---:|---:|---:|
| 1 | 155.875→160.266 | 348.828→228.203 |
| 8 | 218.688→232.172 | 359.312→232.273 |

割当系列ではworkload所有物の全解放を確認し、対応するRSS系列では解放後もresident memoryが高く残った。counter有/無は別processであり、同じ一回の実行で両者を同時測定した値ではない。保存なしIntでも解放区間でRSSが増えるため、完成時Cons要求量だけからRSSの原因を決めない。directでは実VMのInt peak増を再現していない。

最初の `oi039_2026_10_12_cost*` 系列はrawにprocessのrepeats設定がなく、1/8反復の自己識別が不足していた。原本を保持したままrecorderを修正して `cost_final` を再測定した。製品・compiled sourceは変えていない。初期のkernel原本と最終原本は混ぜない。

### 保存枝と進行枝の容量配置

追加した `checkpoint_flow=vm` はBuilder集合を `Rc<Vec<Builder>>` で共有し、最初の変更でlive集合を複製する。rollbackは元の保存Rcへ戻す。完成時は一意所有を要求してVecを移し、非一意をcloneで隠さない。

[追加control原本](oi039_2026_10_12_cost_vm.jsonl)・[集計](oi039_2026_10_12_cost_vm.summary.json) は各2 processのAB/BA、1/8反復、4,896区間、544集計、96 process。[内容検証](oi039_2026_10_12_cost_vm_verify.jsonl)・[記録](oi039_2026_10_12_cost_vm_verify.summary.json) は12条件で成功した。4 processのdirect系列と標本数が異なるので、両者のRSS差を性能改善率には使わない。

保存保持の旧Vec版で、rollback後の追加に使う要求量と全区間の最大requested-liveは次のとおり。単位はMiB。1/8反復と全sampleで値は同一。

| payload | 追加区間requested（direct→vm） | 全区間の最大live（direct→vm） |
|---|---:|---:|
| Int | 140.625→0.375 | 289.392098→359.517212 |
| String | 152.250→12.000 | 675.017860→745.142975 |

vm flowではrollback先の元Vecに容量が残り、追加区間のreallocationは0となった。減った140.250 MiBは、この区間のVec再確保の要求量である。一方、再保存後の最初の変更ではcloneしたlive枝を拡張するため、第三追加区間の最大liveはdirectより増えた。両flowとも全体の最大liveはこの区間にあり、費用が消滅したとはしない。今回のsegment版の最大liveはInt 179.957809 MiB、String 447.395828 MiBで、容量配置を変えてもdirect系列とほぼ同じだった。

count無効の保存保持Int / 8反復の全期間peak中央値は、旧Vec 367.023438→今回232.296875 MiB、最終全破棄後の現在RSSは220.789063→232.171875 MiBだった。解放後のresident残存差はあっても、実VMのpeak増22.7 MiBはこの対照でも再現しなかった。

このcontrolは保存枝の容量の向きを合わせたkernelである。集合を一度に複製するため、実VMのentry単位複製とscheduler順序は再現していない。実VMのrequested/liveとRSSを保存・entry変更・解放の境界で直接照合する作業をOI-039へ残す。既存List表現では共有prefixのCons構築が必要であり、新表現や所有方式の変更は今回採用しない。

### 検証と残件

サブエージェントがASMとprobeを担当し、Astraが通常経路の原本を独立再計算した。全時間・counterが一致し、修正必須の正しさの指摘はなかった。Astraとの相談で、小さな整理を採用し、対象Intの遅化とRSSを未解決として残す方針にした。

Astraはkernelのdirect 9,792行とvm 4,896行の全区間・counter・live収支・RSS集計、各12条件の内容検証、所有の向き、build/source/binary/runnerの対応も独立照合した。count有効のdirect 432反復、vm 216反復で全破棄後の開始値復帰を確認し、集計不一致や修正必須の指摘はなかった。Astraはbuild/testを再実行していない。

- `rtk cargo nextest run -p eldr -E 'test(/list_builder::tests/) | test(/generator_tests/) | test(/list_flat_map_tests/)'`: 最終製品コードで35 passed、325 skipped。
- `rtk cargo nextest run --profile ci --workspace`: 最終製品コードで2,540 passed、失敗0（69 binaries）。
- `rtk proxy cargo run -- test --quiet --all`: 標準Surtr全件成功、終了コード0。
- VM probeの内容検証は全18条件で成功。コピー量・仕事量の受入条件も全測定processで一致した。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。

言語の契約変更はなく、既存の所有・予算・取消・rollbackテストを利用した。性能だけの変更に速度閾値や実装を写したテストを追加していない。追加kernelは標準のworkspace gate外の手動測定であり、全件検証を繰り返す理由にはしない。製品sourceと採用binaryの42ソースhashは最終時点でも一致した。コミット・main統合は行っていない。
