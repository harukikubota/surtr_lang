> 取り込み元: `/Users/haruca/.codex/worktrees/9ea8/surtr`（codex/rt5-builder-checkpoint-measurement、HEAD `72154707`）。資料の集約。記載された製品変更・harnessはmain未統合。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# OI-039: 末尾の直接所有の比較と空Builderの予約保持

直接RefCellに保持する案と専有Boxに保持する案は、改善・悪化が混在し、採用の根拠が足りなかった。製品はサイズ閾値なしの共通Rc方式を維持する。空のBuilderを複製すると元の予約bufferが共有化され、次の追加で予約を使えなくなる問題だけを最小修正で解消した。通常経路全体の遅化と実VMのRSS差は未解決である。

入力は [共通構築経路の方針](list_builder_route_decision_2026_10_12.md) と続行依頼。サイズ閾値なし・保存時Valueコピー0を維持し、通常追加時のRc所有確認を減らす候補を比較する。言語仕様・公開API・List表現・完成途中の予算は変えない。Eldr内のlevel2の所有表現変更として扱う。

## 候補と受入条件

以下は比較した候補の仕様であり、製品には残さない。Builderごとに専有するRefCellへ、変更中のOwned末尾（prefixとVec）またはFrozen segmentのRcを保持する。通常pushはmut参照からget_mutでOwned Vecへ追加し、動的borrowを取らない。Clone時だけ元のVecを不変segmentへ移し、各枝を別々のFrozen状態にする。共有可変セル、Value全clone、unsafe、サイズによる旧経路切替は追加しない。

予約した空Builderのcloneは元の予約を維持し、新側を空にする。Frozenが一意になれば予約を成功させてからVecをOwnedへ回収し、不要なprefixを作らない。共有中は新末尾の予約を成功させてから枝を独立させる。長さoverflowと予約失敗で既存要素・長さを失わない。freeze中に利用者callbackやValue cloneを呼ばず、内部borrow競合をfallbackで隠さない。

finishは既存のListBuilderFinishへ所有物を渡し、独占bufferの一回移譲、共有prefixの一要素/境界ごとの課金、完成途中の保存・取消・rollbackを維持する。深いsegment鎖のDrop/Debugの既存境界も維持する。

## 比較手順

1. 現行版のsource/binaryを保持する。既存の所有・予算テストに、予約済み空bufferを保存しても元の予約を使える境界を追加し、旧版で失敗する理由を確認する。
2. 真の短い完成出力0/1/8/32/256を同じVM入口で測るprobeを追加し、現行版と候補版を同じinstrumentationでビルドする。初期構築から完成と、途中保存から完成を別workloadにする。
3. 通常の16,384要素条件と保存付き処理も前後binaryで交互比較する。完成時のRc確保位置と、Builder型幅がcontinuation移動へ与える影響も確認する。測定中はbuild/testを重ねない。
4. Astraへ所有境界・原本・最終差分を渡す。契約を満たし改善が確認できた候補だけを採用し、採用後は既存の全体gateを一回確認する。実VMのRSS増原因をこの時間比較から断定しない。

## 結果

### 比較対象と原本

変更前は、前回のinline化・容量不足時だけの予約を含む、作業ツリーのRc segment版。mainの旧Vec全clone版とは異なる。同じ追加テスト・測定instrumentationを含めたbefore / direct / box / finalのrelease test binaryを個別に保存した。42 Rustソースとmanifest / lockを固定し、ビルド前後の一致、compiler-artifactと退避binaryの対応を照合した。各版のソース差は `list_builder.rs` のみである。

| 比較 | 条件と反復 | 原本 / 集計 |
|---|---|---|
| directの長い条件 | 32 Task、保存時長16,384、Int/String。各条件前後6 process、AB/BA同数、各8反復。1,920行 / 40集計 | [原本](owned_tail_2026_10_12_direct_compare.jsonl) / [集計](owned_tail_2026_10_12_direct_compare.summary.json) |
| boxの従来全条件 | 1/8/32 Task、保存時長32/1,024/16,384、Int/Stringの18条件。前後6 process、AB/BA同数、各8反復。17,280行 / 360集計 | [原本](owned_tail_2026_10_12_box_compare.jsonl) / [集計](owned_tail_2026_10_12_box_compare.summary.json) |
| directの実出力が短い条件 | 実出力0/1/8/32/256、1/32 Task、Int/String、2 workloadの40条件。前後4 process、AB/BA同数、各8反復。2,560行 / 80集計 | [原本](owned_tail_2026_10_12_direct_short.jsonl) / [集計](owned_tail_2026_10_12_direct_short.summary.json) |
| boxの実出力が短い条件 | 同じ40条件・4 process・8反復。2,560行 / 80集計 | [原本](owned_tail_2026_10_12_box_short.jsonl) / [集計](owned_tail_2026_10_12_box_short.summary.json) |

従来probeは保存済みprefixの後に2,048件を追加するため、保存時長32でも実出力は2,080件となる。今回の [短い出力のprobe](/Users/haruca/.codex/worktrees/9ea8/surtr/tests/profile/checkpoint_builder_short/README.md) は完成出力そのものを0/1/8/32/256件にした。

- `construct_complete`: 入力List・bytecode/VM・Task/Future登録の初期構築から全Task完成まで。checkpointなし。
- `snapshot_resume`: mapper完了後、出力全長の半分を追加した未完成Taskを区間外で準備し、checkpoint→再開・完成→保存状態破棄まで。0/1件もmapper結果の受取後、最初の追加・完成前で保存する。

時間はVM driverを含み、純粋なpushの時間ではない。VM破棄・内容検証・counter照合・JSONは時間区間外。前後ともtest counterを有効にし、その費用は差し引かない。processごとの8反復平均を作り、その中央値・最小・最大を比較した。外れ値は除いていない。RSSは準備・反復・検証・破棄を含むprocess全期間の高水位で、区間の保持量やrequested allocationではない。短い条件だけを保存時長別の従来条件へ混ぜて比較しない。

### 直接所有する2案を不採用とした理由

| 所有方式 | Builder幅 | invocation移動幅 | ExecutionContext移動幅 |
|---|---:|---:|---:|
| 変更前のRc | 16 bytes | 728 bytes | 904 bytes |
| direct: RefCellを直接保持 | 48 bytes | 760 bytes | 936 bytes |
| box: RefCellを専有Boxへ保持 | 16 bytes | 728 bytes | 904 bytes |

これは今回のcompiler/binaryで確認したlayoutであり、安定ABIではない。directは通常のOwned追加からRc所有確認と動的borrow確認を除けたが、VMの状態移動とstack幅が増えた。boxは状態移動幅を元に戻せた一方、間接参照とcellの確保・解放が加わった。boxの空Defaultへの初回追加には40-byte cell確保、非空Cloneには新側cell確保、Owned非空のfreezeとfinishには48-byte Rc segment確保がある。Valueのstack経由の一時コピーは両案に残った。[機械語の証拠と再現手順](list_builder_owned_tail_evidence_2026_10_12/asm-evidence.md) に位置と原出力を保存した。

32 Task / 保存時長16,384の通常完了 `unique_complete` は次のとおり。単位はms、各6 processの中央値。

| 候補 | Value | before | after | 変化 |
|---|---|---:|---:|---:|
| direct | Int | 12.517766 | 12.578607 | +0.49% |
| direct | String | 13.656654 | 14.341255 | +5.01% |
| box | Int | 12.883482 | 12.482925 | −3.11% |
| box | String | 13.564685 | 13.263555 | −2.22% |

direct/boxは別時期の交互系列なので、after同士を直接比べて優劣を決めない。boxの長い32 Task条件には改善傾向があるが、前後の観測範囲は重なった。従来18条件の通常完了は中央値改善8・増加10、保存付き9区間合計は改善10・増加8だった。Astraが原本のprocessごとの9区間合計を再集計して確認した。各phase中央値の和を、合計時間の中央値と同一視しない。

短い40条件では、directは構築完了20条件中13改善・7増加、保存再開20条件中13改善・7増加。boxはそれぞれ12改善・8増加、11改善・9増加だった。boxの構築完了は全20条件で前後範囲が重なり、保存再開も19条件で重なった。通常追加の検査削減だけを全処理の改善と扱えず、2案とも安定した採用根拠がない。製品にはRc方式だけを残し、候補ソースは調査の証拠として保存した。

### 採用した空Cloneの最小修正

空のBuilderは共有するValueがないため、Cloneで新側をDefaultの空状態にする。元側の予約bufferは独占状態を維持し、元側の次の追加で再利用できる。非空のClone・push・finish・反復Dropは変更前と同じで、変更前の退避ソースから5行追加だけである。[差分](list_builder_owned_tail_evidence_2026_10_12/final-versus-before.patch) と [build/source照合](list_builder_owned_tail_evidence_2026_10_12/build-source-audit.json) を保存した。これは空状態の正規化であり、入力・出力サイズによる構築方式の切替ではない。

新規の `empty_checkpoint_preserves_the_original_reservation` は、容量16の空BuilderをCloneしてから元側へ追加する。変更前binaryはcapacity growthが1となり、期待0に対して失敗した。最終binaryは0で成功した。[同じテストのRed/Green記録](owned_tail_2026_10_12_reservation_red_green.json) を残した。新規の `released_checkpoint_reuses_the_unique_buffer` は、保存側を解放すると一意になったbufferを再利用し、完成時のコピー0を保つ既存動作の保護テストである。こちらを変更前Redとは扱わない。

### 最終版の検証

最終版の短い条件も同じbefore binaryと交互比較した。[原本](owned_tail_2026_10_12_final_short.jsonl) / [集計](owned_tail_2026_10_12_final_short.summary.json) は40条件、前後4 process、各8反復、2,560行 / 80集計。中央値は構築完了10改善・10増加、保存再開12改善・8増加で、通常経路全体の高速化は主張しない。

従来18条件の [最終原本](owned_tail_2026_10_12_final.jsonl) / [集計](owned_tail_2026_10_12_final.summary.json) は各1 process・8反復、1,440行 / 180集計。所有・callback・完成step counterの照合に使い、1 sampleの時間から性能優位を決めない。別processの [従来18条件の内容検証](owned_tail_2026_10_12_final_verify.summary.json) と [短い40条件の内容検証](owned_tail_2026_10_12_final_short_verify.summary.json) は全条件成功した。

保存時Valueコピー0、保存後callback再実行0、Taskごとの完成1回、出力全要素の位置・順・長さ、Future結果、Task回収を確認した。共有prefixの完成時コピーとCons構築は残る。Astraとサブエージェントが原本・集計・source/binary対応を独立照合し、追加の正しさの指摘はなかった。

- 対象テスト: `rtk cargo nextest run -p eldr -E 'test(builtin::list_builder::tests) | test(vm::list_flat_map_tests) | test(vm::generator_tests)'` — 38件成功、対象外325件。
- 全体: `rtk cargo nextest run --profile ci --workspace` — 2,543件成功、69 binaries、終了コード0。
- 標準: `rtk proxy cargo run -- test --quiet --all --format=json` — 1,124件を全て実行・成功。失敗・skip・pending・filterは0、終了コード0。
- `cargo fmt --all -- --check` と `git diff --check` — 成功。

全体検証の [記録](list_builder_owned_tail_evidence_2026_10_12/final-validation.json)、[Rust log](list_builder_owned_tail_evidence_2026_10_12/final-workspace.log)、[標準JSON](list_builder_owned_tail_evidence_2026_10_12/final-stdlib.jsonl) を保存した。言語・公開API・標準 `.srt`・VM/schema versionの変更はない。List/Generatorの共通Builderへ作業ツリーで適用した状態で、未コミット・未統合。mainとHEADは確認時点でともに `72154707`。

## 残件

[OI-039](../../../open-issues.md#oi-039-builderコピー削減後の通常経路とintのrss差) は継続する。今回、通常追加のRc検査削減だけでは方式変更を正当化できず、型幅とallocation位置も費用に影響することを確認した。Rc検査自体の寄与が0と確定したわけではない。

次は実VMのentry変更・保存枝・完成・全破棄の境界で、requested allocation、論理live、count無効系列のRSSを照合する。kernelのCons構築とallocator再利用の測定を実VMへ接続し、既存のInt RSS増を説明できるか確認する。compiler / REPL / 通常batch全体の効果と、Valueのstackコピーの寄与も未分離である。
