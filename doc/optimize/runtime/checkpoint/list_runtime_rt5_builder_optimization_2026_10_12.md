> 取り込み元: `/Users/haruca/.codex/worktrees/9ea8/surtr`（codex/rt5-builder-checkpoint-measurement、HEAD `72154707`）。資料の集約。集約時点では、記載された製品変更・harnessはmain未統合。作成時点の実装・検証記録を保存し、現在の整理一覧は [runtime README](../README.md) を参照。実行コマンドは取り込み元のソースを基準にする。

# RT-5: 保存した Builder のコピー削減

集約後に[main作業treeへの反映・再測定](list_runtime_rt5_builder_adoption_2026_10_13.md)を実施した（未コミット）。以下は先行worktreeの実装・測定記録として保持する。

入力は [中断中 Task の測定レポート](list_runtime_rt5_builder_measurement_2026_10_12.md)。ユーザーの「調査範囲の改善を進めてください」に基づき、同レポートの Builder コピーを削減する。level2とし、Eldr内の所有表現と共通の完成継続を変更する。言語の評価結果、公開API、VM/schema versionは変えない。process表index、override、chunk登録処理は対象外とする。

本書はBuilder共有化を採用した時点の記録である。その後の通常経路の局所改善と割当調査は [OI-039の追加結果](list_runtime_oi039_cost_2026_10_12.md) に分けて記録する。

## 方式と受入条件

`ListBuilder` は append専用のsegmentを `Rc` で共有する。保存状態が同じ末尾segmentを保持している場合、再開側は新しい末尾を作って追加する。Builder cloneはsegmentの参照だけを共有し、保存済みValueを複製しない。segment内のVecは独占所有のときだけ変更し、保存側と別の再開枝に追加を漏らさない。予約容量を持たない空Builderはsegmentを持たない。

通常の単一・独占segmentは、Vecを完成したPacked Listへ移す。共有・分割済みの場合は、独占末尾をPackedへ移し、以前のsegmentの要素を逆順に一件ずつ既存の `ListHandle::cons` で接続する。共有要素のValue cloneとCons allocationはこの完成段階に残るため、再開時のコピー削減を全処理のコピー消滅とは扱わない。

完成途中はsegment cursorと不変のListHandleを継続が保持する。完成遷移は一要素または一segment境界ごとに1 reductionを消費する。完成途中の保存でも部分出力を全cloneしない。新たな無課金の一括Rust loopや集約用の可変Vecへ費用を移さない。深いsegment鎖のDropは反復処理し、Debugは鎖を再帰表示しない。

完成継続を実行するquantumの最初の一stepは共通のcontext処理に通す。続く純粋なListFinishの再開はcontext上で直接進め、一回ごとに1 reductionを課金する。予算0では変更せず、Complete / Errはpending invocationへ戻し、配送・診断補完は次の共通stepで行う。取消中も共通経路を使う。これにより、各要素ごとの実行状態の入れ替えを減らす。

- Listの長さ・論理順、mapperの順序と呼出し回数、RuntimeError・待機・取消・結果の一度だけの配送を維持する。
- Builder clone、初回追加、再保存後追加で保存済みValueのcloneを行わない。保存なしでは従来のVec移譲を保つ。
- 追加なしの共有Builderの完成、複数保存位置からの分岐、完成途中の保存・再開・rollback・取消を検証する。
- `Generator` materializeも同じBuilderを使うため、Count終了、While false、有限GeneratorのNoneを共通完成継続へ接続する。restとcallbackを維持する。
- 保存の反復でsegmentが多数できても、Drop/Debugでstack overflowを起こさない。
- copied_itemsはBuilder clone時のValue件数、finish_copied_itemsは完成段階のValue clone件数として分ける。完成までの時間、通常経路、解放、RSSも比較する。

共有Builderを完成したListはConsとPackedが混在し得る。これは既存のListHandle表現と操作の範囲であり、利用者の意味論は変えない。内部表現の説明を `docs/dev/EldrVM_spec.md` と `docs/dev/ProcessRuntime_spec.md` に整合させる。

## 手順と検証

1. 既存の測定用counterを使い、Builder cloneのValueコピー0と分岐独立性をテストで固定し、旧Vec cloneでRedを確認する。
2. 変更前の製品コードで測定probeに完成・通常経路を追加し、release binaryと原本を保存する。前回の8区間原本は変更しない。
3. segment所有、予算付き完成継続、flat_mapとGeneratorの接続を実装し、局所テストとVMの保存・取消境界を確認する。
4. 同じ入力を変更後に測り、保存した前後binaryをAB/BA順で交互に比較する。測定中はビルド・テストを重ねない。
5. Eldr/REPL、workspace CI、標準Surtr全件を検証し、Astraが最終差分・結果を確認する。デグレ・新規問題はAstraへ相談する。コミット・main統合は今回の依頼に含めない。

## 結果

保存済みValueの再開時全cloneを除き、長いBuilderを持つTaskの保存付き処理を改善した。保存由来のValue cloneの累計は、今回の保存・再保存・rollback条件で従来のおよそ半分になった。通常経路の遅化とIntのprocess peak RSS増は残り、[OI-039](../../../open-issues.md#oi-039-builderコピー削減後の通常経路とintのrss差)へ記録した。RT-5全体の完了や一律の性能改善とはしない。

### 実装と検証した境界

- Builder cloneはRcの共有だけになり、初回追加は保存済みsegmentをprefixとして、新しい末尾へ行う。Defaultは空で、予約した空bufferをcloneした場合も空prefixを積まない。
- 独占した単一bufferの完成は一遷移でPackedへ移す。共有prefixはConsへ一件ずつ接続し、完成途中もcursorと不変の部分出力を共有する。保存を解放して元segmentが独占に戻った場合、処理済みsuffixの破棄も一件ずつ進め、残りを重複出力しない。
- 純粋な完成処理は残予算の範囲で型付き状態を保持する。各試行を1 reductionとして数え、エラーになった遷移も課金する。量子の先頭は共通のcontext処理を使い、Complete / Errはpendingに戻して次の共通stepで配送する。予算を使い切った場合は未配送で止まる。
- ListFinish継続をflat_mapとGeneratorで共有した。Count、While false、finite Noneの各終了経路でrestとcallbackの一度だけの実行を維持する。取消・不正predicateで部分Listを成功として公開しない。
- 複数保存位置の分岐、追加なしの共有buffer完成、完成途中の保存・rollback・取消、予算0/1/2/7/256、10,000 segmentのDebugと反復Dropを確認した。旧Vec全cloneの製品経路は残していない。

### 測定条件と原本

[再現方法と10区間の定義](/Users/haruca/.codex/worktrees/9ea8/surtr/tests/profile/checkpoint_builder/README.md)。Apple arm64 / macOS 26.6.2、rustc 1.94.1、root lockによるrelease unit-test binary。Task数1 / 8 / 32、保存時Builder長32 / 1,024 / 16,384、位置番号のInt / 256-byte Stringの18条件。各processは8反復し、VM初期化・準備・assert・内容走査・JSONを区間外とした。counterは時間系列でも有効で、overheadを差し引かない。

前回の8区間に、元保存位置を保持したまま完成させる `complete_shared` と、別VMで保存なしに完成させる `unique_complete` を追加した。前者はrollbackで戻した2回再開後の位置から進める。後者は指定のBuilder長まで区間外で準備済みで、残り2,048件/Taskの追加と完成を測る。初期構築・最初のsegment確保や、compiler / REPLのchunk入力全体の時間は含まない。

- [変更前原本](rt5_builder_2026_10_12_before_completion.jsonl)・[集計](rt5_builder_2026_10_12_before_completion.summary.json): 90 process、7,200区間、180集計。
- [採用版原本](rt5_builder_2026_10_12_accepted_completion.jsonl)・[集計](rt5_builder_2026_10_12_accepted_completion.summary.json): 同じ18条件を同数で測定。
- [採用した交互原本](rt5_builder_2026_10_12_accepted_recheck.jsonl)・[集計](rt5_builder_2026_10_12_accepted_recheck.summary.json): 前後各6 process、AB/BA同数、17,280区間、360集計。
- [採用版の内容検証](rt5_builder_2026_10_12_accepted_verify.jsonl)・[記録](rt5_builder_2026_10_12_accepted_verify.summary.json): 全18条件を別processで1反復し、保存あり・保存なし両方の全要素、長さ、順序、Future結果、Task回収を確認した。時間・RSSは保存しない。

前後binaryは `/var/folders/g5/r1ds6cgn3rn3_t7qzfr8zy140000gn/T/surtr-9ea8-builder-improvement-bli0isrw/` の `before-binary` / `accepted-binary` に保持する。`before_build.jsonl` / `accepted_build.jsonl` と各source snapshotも同所へ保存した。ビルド前後のsnapshot、compiler-artifactと保存binaryのSHA-256を照合した。採用版metadataには42のEldr/Sindrソース、manifest/lock、binary、runnerのhashとdirty statusを保存した。前後probeの測定条件・区間は同じで、後続の変更は境界テストとコメントである。測定とビルド・テストは重ねていない。

### コピー量

全条件で保存・Builder clone・初回追加・再保存後追加の保存済みValueコピーは0になった。新しい保存なしの2回目再開もコピー0である。共有prefixの完成には、元保存位置が保持する長さLの要素cloneが残る。通常の追加時に入力から取り出すValue cloneは、この保存由来のコピー指標に含めない。

32 Task / L=16,384では、従来の初回524,288件＋再保存後540,672件＝1,064,960件に対し、採用版は再開時0、完成時524,288件となった。保存由来のValue cloneは約50.8%減った。これは論理的な要素件数で、物理コピーbyte数やrequested allocationではない。

### 保存付き処理の時間

次の表は採用した交互測定。`unique_complete`以外の9区間を各process内で足し、8反復で割ってから6 processの中央値を取った。中央値同士は加算していない。保存、再開、再保存、rollback、完成、元保存状態の破棄を含み、入力準備と完成結果の全走査は含まない。L=16,384、単位はms。

| Task数 | payload | 変更前 | 採用版 | 時間の減少率 |
|---:|---|---:|---:|---:|
| 1 | Int | 1.703383 | 1.454370 | 14.6% |
| 1 | String | 2.075812 | 1.698937 | 18.2% |
| 8 | Int | 12.546300 | 10.492336 | 16.4% |
| 8 | String | 18.287696 | 13.147568 | 28.1% |
| 32 | Int | 51.823323 | 42.673609 | 17.7% |
| 32 | String | 81.566654 | 55.504249 | 31.9% |

32 Task / Stringの9区間合計の範囲は79.492771〜99.114989 → 53.407312〜62.219374ms、Intは50.787115〜55.334433 → 42.278266〜42.881942msだった。初回再開中央値はString 27.445198→1.482560ms、Int 15.114943→1.382206msとなった。

完成区間単独はString 8.446250→42.085318ms、Int 8.170333→32.957016msと増えた。共有要素のclone、Cons構築、予算内の進行をこの段階で行うためである。初回再開だけの差を改善率へ使わず、完成・破棄を含む合計を比較した。L=32 / 1,024の小さい条件では合計差が小さく、1 Task / Intは約0.3〜0.5%増えた。すべての条件が速くなるとはしない。

### 通常経路とRSSの残る差

同じ交互測定の `unique_complete`、32 Task / L=16,384。単位はms。

| payload | 変更前中央値（範囲） | 採用版中央値（範囲） | 時間の増加率 |
|---|---:|---:|---:|
| Int | 11.144932（11.085484〜11.299214） | 11.995281（11.879260〜12.091328） | 7.6% |
| String | 12.571930（12.421286〜12.869875） | 13.004888（12.777630〜13.332917） | 3.4% |

通常の独占Vec移譲を維持しても、追加時のRc所有確認やVMの完成継続判定等の費用は加わる。寄与は未分離で、最初のRc確保を含まないこの区間の差を、その確保費用とは説明しない。Astraに相談し、内部表現をさらに増やすlazy promotionは採用せず、原因調査をOI-039へ残した。

process全期間peak RSSの中央値は、同条件でInt 256.359375→279.093750 MiB、String 628.515625→541.820313 MiBだった。Intは約22.7 MiB増えた。8反復・全10区間・準備・assert・JSON・test harnessを含む高水位であり、区間別peakや現在の保持量ではない。Cons allocation、allocator、解放・再利用の寄与は未特定。requested allocationと論理liveは未測定で、一律のメモリ改善とはしない。

### デグレ確認と方式の調整

初期のsegment実装は、32 Task / Stringの完成が逐次測定で約123msとなり、再開から完成までの合計が遅くなった。Astraへ相談し、純粋なfinishで毎要素行っていたcontextの入替えと、大きなinvocation enumの包み直しを削減した。一要素/境界ごとの課金、PC、frame、trace、取消、次stepの配送は維持した。

中間値は [最初のsegment版](rt5_builder_2026_10_12_after_completion.summary.json)、[一遷移ずつ直接再開する版](rt5_builder_2026_10_12_after_final_completion.summary.json)・[その交互追試](rt5_builder_2026_10_12_final_recheck.summary.json) に原本との対応を残す。採用した値は `accepted_*` のみであり、中間版のbinaryも退避ディレクトリへ保持した。製品には方式を選ぶ互換切替を残していない。

### 最終検証

- Red: `checkpoint_shares_long_prefix_without_copying_values` は旧Vecの1,024件コピーに対し期待0で失敗した。構文・環境エラーをRedには使っていない。
- `rtk cargo nextest run -p eldr -E 'test(/list_builder::tests/) | test(/generator_tests/) | test(/list_flat_map_tests/)'`: 採用版で35 passed、325 skipped。
- 終端予算テストを追加補強し、予算2ではComplete未配送で停止、予算7では実費だけ課金して配送することを確認。対象の再実行1 passed、359 skipped。
- `rtk cargo nextest run --profile ci --workspace`: 採用版で2,540 passed、失敗0（69 binaries）。
- `rtk proxy cargo run -- test --quiet --all`: 採用版で標準Surtrテスト全件成功、終了コード0。
- release buildはすべて `--locked` で成功。採用版の測定90 process、AB/BA 216 process、内容検証18 processは終了コード0。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。
- サブエージェントによる方式調査・Builder実装・Generator/probeテストと、Astraによる最終core・原本の独立レビューを実施。Astraは前後各7,200行と交互17,280行の全時間・counter・位置・RSS・実行順、全18条件の内容検証、binary/source/runner hashを照合し、不一致や修正必須の正しさの指摘なし。Astraはビルド・テストを再実行していない。

本作業ではコミット・main統合を行っていない。稼働process・override・chunk数別の実利用、表indexとentryの寄与、通常経路とInt RSS差の原因は未解決である。
