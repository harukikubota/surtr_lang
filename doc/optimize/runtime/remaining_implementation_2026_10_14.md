# Runtime残り6分野の実施計画

入力は[一覧](README.md)と、2026-10-14の「全て進めてください」。基準はmain `eec8c9b6`。項目ごとに仕様、TDD、測定、検証を完了し、独立コミットにする。旧原本を上書きせず、実測で改善を確認できない項目を解決済みにしない。

## Packed tail: 採用方式の確認

候補は256要素までの前向きchunkを共有する方式。短いtailはそれより前のchunkを保持せず、元Listや保存枝が生きている間はその所有分が残る。Consとの混在、head / tail / uncons / lenの論理的なO(1)、永続性と論理順を維持する。tailごとのcompactionやサイズ閾値による旧表現へのfallbackを置かない。

完成済みVecはchunkへValueを移すため、現在のbufferそのものの一括移譲から線形の移動・追加確保へ変わる。payloadをcloneしないことと、物理移動が不要であることは区別する。Builder完成は共通予算内で少しずつ移譲し、保存・取消・復帰を検証する。構築費用・peakと短いsuffixの保持量の交換条件はユーザーが256要素chunk方式を承認した。Builderの予約と各segmentは256要素までとし、独占bufferは最大256要素の所有権移譲一回を1 reduction、共有要素は一個のcloneを1 reductionとする。新しいsegmentへの移行とComplete / Err配送も既存予算へ接続する。Listの内部表現・完成予算にまたがるためlevel4とし、仕様と最終実装の独立レビューを行う。

## Future: cleanup検索と通常経路

現行のcleanup完了後だけ配送する規則を維持し、Future表とproducerの対応を照合する。普通値だけのstackは引き続き表を読まず、追加の永続metadataと変更時のCOW費用を採用前に比較する。未解決IDの再確認削減は、同一収集中の安定性とFuture結果の一段置換を守る。既存のcleanup・取消・待機・checkpointテストを拡張し、通常CLIと内部操作を別に測る。局所改善はlevel2。

## HashMap: 開始時のコピーとsort

所有する入力をsorted_entriesで全cloneしてからsortする経路を除く。キー昇順のcallback順、duplicate後勝ち、部分outputの独立性、待機・取消・checkpointを維持する。開始処理のclone / sort / iterator準備を区別し、分割できた単位を明記する。未分割の物理Value操作まで実時間上限を保証しない。continuation変更はlevel3。

## Regex: 同期探索と収集

複数一致・split・replace_allを既存のBuiltinOutcome::Resumeへ接続し、一回のresumeで一件の探索結果を進める。UTF-8、空一致の進行、anchor、capture展開、全入力共有と一致順を現在のregex crateと照合する。regex内部の一回の探索・compile・String clone / appendの物理費用は同期のまま。callback・taskの公平性は通常入口で確認し、cleanup・取消・checkpointの結果を検証する。level3。

## FileOutHandler: 未決規則の具体案

[修復案](file-out/file_out_handler_lifecycle_repair_spec.md)の未決4群について、次を確認対象の具体案とする。level4で正本と独立レビューを先に整合させる。

1. VM cwdを基に絶対化し、既存fileはcanonicalize、新規fileは存在する親をcanonicalizeしてleafを接続する。hardlinkは統合しない。同じcanonical pathを同じsinkへ接続し、path差替え後も取得済みdescriptorを使う。
2. user init前に全dependencyを取得し、Standby retryでは保持する。初期化失敗は既存ProcessInitFailed / WorkerのResultへ原因を保持して配送する。部分失敗は取得したleaseだけを解放し、他ownerを閉じない。processから起動するTaskにも取得済みbindingを保持する。
3. 最後のactive owner終了で新受付を止めてflushし、live leaseを解放する。保存leaseは物理descriptorを保持し、rollbackは保存位置の同じdescriptorへ戻る。VM cloneの片側終了で他方を閉じない。
4. sinkへの登録順をFIFOとし、通常再開では一度だけwrite・結果配送する。開始済みwriteをcall timeoutで取り消さない。外部bytesはrollbackしない。rollbackを跨ぐexactly-onceやreceipt registryは追加しない。flush失敗を成功として隠さず、通常の失敗を既存write失敗へ、終了時失敗をProcessLifecycleFailedへ接続し、元の失敗を優先して原因を保持する。

ユーザーが上記4群の具体案を承認した。正本と資源所有の実装を整合させてから検証する。表示文字列の逆parseとwriteごとのopenを最終経路から削除し、未登録sinkや不正bindingをRuntimeErrorにする。schema / VM versionを上げない。

## Builderと3件のRSS調査

Builderの独占追加・完成と共有prefix完成を分け、Rc確認・Value移動・VM継続判定の寄与を測る。保存なしの費用を下げるために、保存時の全Vec cloneや無課金finishを復活させない。局所変更はlevel2、List表現と接続する変更はPacked項目と同じlevel4として扱う。

OI-037 / OI-038 / OI-039は、保存された原本と同じ入力・所有条件の前後binaryを再現する。allocator構成を分け、同一binary内の分布とbinary間の差、要求layout・論理保持量・現在RSS・全期間peak RSSを区別する。既存binaryが消えていれば元revisionとpatchから再構築し、昔のsource hashと一致するか記録する。原因が特定できない場合は再現条件と除外できた仮説を残し、コピーを復活させたりallocatorを強制変更したりして解決扱いにしない。

## 検証と進捗

crate-localの直接テストから始め、通常入口の成功・拒否とfairnessを必要な層で固定する。各製品変更の最終sourceでworkspace CIと標準Surtr全件を一回確認する。level4は独立レビュー後、修正があれば最終sourceで再検証する。source / Cargo artifact / binary / runner hashを保存し、build・他のtestと性能系列を重ねない。

着手時点は6分野とも未完了。PackedとFileOutHandlerの方式はユーザーの回答で確定した。最終検証と採用測定が終わるまで未完了として追跡する。

## 進捗: 2026-10-15

- Packed: [製品採用と検証](list/packed_chunk_adoption_2026_10_15.md)を完了。独立レビューの指摘を修正し、CI 2,576件・標準1,124件成功。保持量・構築・RSSの交換条件を採用記録へ残した。
- Builder: 有界segmentと完成処理をPackedに接続し、実VMの前後測定を完了。通常CLI・旧Vec基準への時間差・allocator / OS原因は継続。
- HashMap: [前処理と構築の分割](hashmap/hashmap_preparation_split_2026_10_15.md)を実装し、所有entryの全cloneを削減。全内容と時間・確保系列を確認し、CI 2,581件成功。OI-038のRSS原因は継続。
- Regex: [探索・収集の分割](regex/regex_collection_split_2026_10_15.md)と全検証を完了。通常CLIの速度改善は未確認、RSS小幅増を記録。
- Future: [cleanup表の一括検索と小さい収集内cache](value/future_cleanup_snapshot_2026_10_15.md)を実装・検証。CI 2,586件・標準1,124件成功。条件ごとの追加費用・通常CLIの未確認の速度改善を記録し、OI-037は継続。
- FileOut: [第一段階のctx capture](file-out/file_out_handler_implementation_2026_10_15.md)を実装し、CI 2,588件・標準1,124件と独立レビュー成功。descriptor・owner・FIFO・復帰は未実装。FileOutの仕様段階レビューで、escaping closureのctx captureとTask bindingの所有接続を確認した。普通のPIDやclosureの保持だけでactive owner数を増やさず、process / init / Task / 未完了writeの所有と保存leaseを分ける。
