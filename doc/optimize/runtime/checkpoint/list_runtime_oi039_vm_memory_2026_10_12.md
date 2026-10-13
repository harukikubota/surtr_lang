> 取り込み元: `/Users/haruca/.codex/worktrees/9ea8/surtr`（codex/rt5-builder-checkpoint-measurement、HEAD `72154707`）。資料の集約。記載された製品変更・harnessはmain未統合。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# OI-039: 実VMの割当・生存量・RSSの境界測定

[前回報告の残件](list_builder_owned_tail_2026_10_12.md#残件)と続行依頼を入力に、既存checkpoint workloadの費用を実VMで観測する。製品のBuilder・List表現・言語・公開API・予算・取消・rollbackは変更しない。専用featureでのみコンパイルするテスト観測の追加で、level2として扱う。サイズ分岐や旧経路の復活は行わない。

## 対象と受入条件

32 Task、保存時Builder長16,384、位置番号Int/256-byte String、1/8反復。共有保存を保持したまま再開・再保存・rollback・完成・保存破棄・VM全破棄を測り、続けて保存なしのVM準備・完成・全破棄を測る。準備を含む15区間を連続して記録する。入力・mapper・Task登録・scheduler・Future配送は既存VM入口を使い、完成結果は18,432件となる。

要求layoutをSystem allocatorで数える系列と、counterを無効にして現在RSS/全期間peakを測る系列を別processにする。realloc成功時は旧layout全体をfreed、新layout全体をrequestedとし、物理的な移動量や旧新buffer同時存在のpeakとは扱わない。全Rust GlobalAlloc要求を累積追跡し、区間はsnapshot差にする。test threadの整数ledgerも照合してharness等の混入を拒否する。allocator内で割当・format・lock・stdioを呼ばない。globalallocatorとunsafeラッパーは専用featureかつcfg(test)に限る。

record容量、argv・stdio・barrier、TLSと小さいwarmupをVM/入力生成前に準備する。VM・checkpoint・FutureId Vecの全破棄後に同じbaselineへ復帰すること、各区間のrequested-freed=live差と連続性を検証する。失敗を未知の常駐量として差し引かず、原因をAstraと確認する。

first_resumeは表index/entry COWと256件/Taskの追加、schedulerを含む。complete_sharedは残り1,536件/Taskの追加・finish・Future配送を含む。工程全体の費用であり、entry cloneやCons完成単独の割当と呼ばない。内容検証は別processで全位置・長さ・順序、FutureとTask回収を確認する。JSONは全区間終了後、RSS停止通知と同じstdoutへ出し、stderrは並行排出またはfileへ退避してpipe deadlockを避ける。

## 手順と比較の限界

1. cfg(test)専用probeと計数を固定する。通常の製品build・既存テストにはallocatorを登録しない。
2. 現行と旧mainのソースを別の退避checkoutへ固定し、同じprobeでcounter有/無binaryを作る。旧mainは履歴の比較fixtureとし、製品には入れない。source/binary/manifest/lock/build前後対応を保持する。
3. counter有の収支と内容を先に確認し、counter無効のRSSをAB/BAで測る。停止barrierなしcontrolも同じbinaryで残す。ビルド・テストを測定に重ねない。
4. Astraへ原本・集計・所有境界を渡す。requested-live、停止境界RSS、全期間peakを分け、既存のInt peak増22.7 MiBを再現しない場合はその事実を残す。未分離の費用を説明済みとはしない。

## 結果

実VMでもrequested-liveの最大増分は減少し、全所有物の破棄後に開始値へ戻った。一方、Intのprocess peak RSSは約22 MiB増える方向を今回も観測した。停止barrierなしでも差が残ったため、pauseだけで生じた差とはしない。allocator/OS内部の原因や工程別のpeak寄与は未特定で、製品の所有方式は変更しない。

### 原本とビルド対応

[専用probeの再現方法](/Users/haruca/.codex/worktrees/9ea8/surtr/tests/profile/checkpoint_builder_vm_memory/README.md)。`vm-memory-profile`はcounter無効、`vm-memory-count`はcfg(test)でのみSystem allocatorを包む。通常の製品build・標準のunit gateには観測moduleもallocatorも入らない。計数有の時間をcounter無効の性能値へ合成しない。

| 系列 | 原本 / 集計 | 範囲 |
|---|---|---|
| 要求割当・生存量 | [原本](oi039_2026_10_12_vm_memory_count.jsonl) / [集計](oi039_2026_10_12_vm_memory_count.summary.json) | 旧/新各2 process、AB/BA同数、Int/String、1/8反復 |
| RSS停止境界 | [原本](oi039_2026_10_12_vm_memory_rss.jsonl) / [集計](oi039_2026_10_12_vm_memory_rss.summary.json) | 同条件、counter無効、各区間終了で停止 |
| RSS pauseなしcontrol | [原本](oi039_2026_10_12_vm_memory_control.jsonl) / [集計](oi039_2026_10_12_vm_memory_control.summary.json) | 同じcounter無効binary・同条件、停止せず実行 |

各系列は16 process・72反復・1,080行・120集計。時間はprocessごとの反復平均から集計し、allocation/RSSは区間ごとの分布を残した。初回と後続反復はrawのrepeat位置で区別する。各processの全期間peakは全行で同じ値であり、区間ごとに加算しない。新しいprobeはwarmup・record・parent test配置も従来と異なるので、過去の22.7 MiBの差と完全に同一の実行を再現したとは扱わない。

旧版はmain `72154707`を根拠に、6製品ファイルと依存する2 testファイルを隔離コピー内だけで復元した。現在版と共通の新probe2ファイル・feature・Task開始済みを示すcfg(test) helperを使う。旧finishを現在の課金付き完成継続へadapter接続せず、当時のVec→Packed移譲とVMの実装を使う。製品へ旧経路を登録しない。両parentの必要なhelper本文は一致するが、その他の未実行テスト配置は異なる。

[証拠一覧](list_runtime_oi039_vm_memory_evidence_2026_10_12/README.md)、[source/build照合](list_runtime_oi039_vm_memory_evidence_2026_10_12/source-build-audit.json)、build前後snapshot・compiler-artifact・4版metadata・共通sourceを保持した。4binaryと全隔離sourceは `/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-9ea8-vm-memory-mlt131l7/` に保持する。harnessの独立した `ok` 行をrunnerが拒否する点は測定開始前にAstraと確認し修正した。初期metadataはpilot記録として退避し、全系列で修正後runnerの `*-final.json` を使った。

### 要求割当と生存量

ここでrequested-liveは、GlobalAllocへ要求して未解放のlayoutサイズの合計で、未使用Vec容量も含む。意味上のValueだけの保持量、mallocの実消費量、resident page量とは異なる。次は各反復のVM生成前からの最大requested-live増分。全sample・1/8反復で同値、単位はMiB。

| payload | 旧Vec | 現在Rc | 最大の工程 |
|---|---:|---:|---|
| Int | 359.824243 | 180.187633 | 旧: 再保存後の再開 / 現: 共有完成 |
| String | 745.450006 | 447.618083 | 同上 |

実VMの値は、以前のkernelの容量配置controlで測ったInt 359.517→179.958 MiB、String 745.143→447.396 MiBと近い。kernelで省いたVM管理表・entry・scheduler・Future等を含む実入口でも削減が残ることを確認した。この近さだけで個々の管理費用を差分として分離できたとはしない。

工程別の累積requestedは次のとおり。allocationの大きさを合算した量で、工程内の最大生存量ではない。

| 工程 | Int 旧→現 | String 旧→現 |
|---|---:|---:|
| 最初の再開 | 208.129677→2.239296 | 334.067421→4.176796 |
| 共有完成 | 0.600548→111.422325 | 12.225548→247.047569 |

初回entry変更で起きる全Vec clone等の要求が減り、共有prefixをListとして完成する段階へ要求が移る。表の工程はschedulerと追加・配送を含むため、Vec clone/Cons構築単独の費用とは呼ばない。

保存なしの最大requested-liveはInt 143.629615→143.629859 MiB、String 287.067389→287.067634 MiBでほぼ同じ。両payload、全72反復でshared/uniqueそれぞれの全破棄後に開始値へ厳密に戻った。global/threadのallocation・reallocation・requested・freed・live・phase peakが一致し、計測中の別thread混入はなかった。

### RSS差と解放境界

counter無効・8反復・各2 processの全期間peak中央値。単位はMiB。

| payload | 停止barrierあり 旧→現 | 停止なし 旧→現 |
|---|---:|---:|
| Int | 257.265625→279.359375 | 257.234375→279.289062 |
| String | 628.187500→539.757812 | 628.687500→540.992188 |

Intはそれぞれ+22.093750 / +22.054688 MiB。観測した2 processずつの範囲も前後で重ならない。Stringは今回も減少した。標本数が小さく、他環境・一般入力のRSSまで保証する結果ではない。

初回Intの保存なし完成からVM全破棄へのresident増分は、旧/新、1/8反復の全8 processで65.875 MiBだった。1反復sample0の具体値は次のとおり。

| 停止境界 | 旧Vec resident | 現在Rc resident |
|---|---:|---:|
| 共有完成後 | 251.437500 | 199.250000 |
| 共有VM全破棄後 | 122.578125 | 128.921875 |
| 保存なし完成後 | 111.984375 | 208.343750 |
| 保存なしVM全破棄後 | 177.859375 | 274.218750 |

対応する別の計数系列では、Intの `drop_unique_vm` は全条件でallocation/reallocation/requestedが0、freedが147,340,095 bytes、追加peakが0、生存量は開始値へ復帰した。**同じ一回の実行でRSSとcounterを同時測定した値ではない。** 新規のRust GlobalAlloc要求がない工程に対応して、RSS系列の解放境界でresident増を観測したことを示す。

解放工程がprocess全期間peakを作ったとは断定しない。現在版1反復sample0の最大観測endpointは274.218750 MiBだが、全期間peakは276.343750 MiBと高い。8反復の現在版では、sample0はrepeat6、sample1はrepeat7の `prepare_shared` に、それぞれ全期間peakと同じresident値が観測された。解放工程の前後でresidentが増え、その後の反復にも高いresidentが残るところまで確認した。allocatorのpage保持・書込み・再利用・OS処理の内訳と最終peakへの寄与量は未特定である。

開始値へ戻るという結果は、測定したRust allocationの未解放増分が残らないことを表す。Rust GlobalAlloc外の割当やRSS残存を否定する結果ではない。`MallocNanoZone=0`等のshell環境は [測定後の観測記録](list_runtime_oi039_vm_memory_evidence_2026_10_12/environment-after-measurement.json) に残したが、child起動時に直接記録した値ではない。この差からallocator設定の寄与量は決めない。

### 検証

別processの [計数有の内容検証](oi039_2026_10_12_vm_memory_verify.summary.json) と [計数無の内容検証](oi039_2026_10_12_vm_memory_rss_verify.summary.json) は各4条件、計8条件で成功した。全出力の位置・長さ・順序、Future結果、Task回収と計数有での全破棄復帰を確認した。4版のsmall smokeも全成功。

- `rtk cargo nextest run -p eldr --features vm-memory-count -E 'test(/memory_profile::vm_memory_profile/)'`: 新しい専用probeの1件成功、対象外363件。
- `cargo fmt --all -- --check`: 成功。
- `rtk cargo nextest run --profile ci --workspace`: 通常featureで2,543件成功、69 binaries、122.095秒、終了コード0。
- `git diff --check`: 成功。

製品のRust本体と標準 `.srt` は前回検証後から変えていないため、標準Surtr全1,124件は今回再実行していない。前回の成功値を今回の実行結果には数えない。

サブエージェントがprobeとsource/build証拠を担当し、Astraが安全性・各1,080行の3系列・全8counter・RSS・source/binary対応を独立照合した。データ不整合や追加の正しさの指摘はなかった。新たに観測した解放境界のresident増もAstraへ相談し、工程への因果帰属を限定した。

## 残件と次の一手

OI-039は未解決。requested-live削減と全解放を実VMで確認し、約22 MiBのRSS増がpauseなしでも残るところまで進めた。今回の測定からAllocatorやBuilder方式を切り替える根拠はないため、サイズ分岐なしの共通Rc方式を維持する。製品の意味論・標準 `.srt`・VM/schema versionは変えていない。未コミット・main未統合である。

次は共有処理だけ・保存なし処理だけのprocessと、両者を連続実行するprocessを分ける。反復・解放順序によるRSS残存を比較し、保存なしPacked bufferの解放境界でVM領域のresident状態を観測する。allocator設定はchild起動時に記録し、同じ実行でのイベント/領域照合を別の診断系列として扱う。時間の測定へ重い診断を混ぜない。Rust allocation要求が0でもresidentが増える動作を確認し、原因を固定するまでは製品コードを変更しない。
