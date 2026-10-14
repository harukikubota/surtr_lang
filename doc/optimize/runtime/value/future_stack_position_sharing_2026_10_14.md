# OI-036: 保存中のFuture位置表を共有する

入力は[runtime一覧](../README.md)と[未解決位置のコピーの残件](future_stack_empty_clone_2026_10_14.md#残件)。基準はmain `9c4cb4e4`。Eldr内のlevel2変更として、値Vecの独立性とFuture配送を保ち、位置表のClone時の全コピーを省く。

## 方式と受入条件

optional Boxの位置Vecをoptional Rcへ置き換える。stack内のmetadataは1 pointer幅のままとし、値Vecは従来どおり複製する。非空位置表はCloneで共有し、空位置表は複製先へ持ち込まない。実際に位置を追加・一部削除する場合だけ表を分離する。

- 非空位置表のCloneは元と同じVecを参照する。値と位置の論理等値、複製枝の独立性を維持する。
- 通常値のpush / pop、位置を削除しないtruncate、空または不一致のready map、PendingFutureを返すready値では位置表を分離しない。
- 共有位置表の全削除は参照を外し、残す位置のない表をコピーしない。独占表はclear後も予約容量を再利用する。
- 元のcheckpointからの再開で、ready / Cancelled / unknown ID、重複slot、Futureを返すFuture、cleanup完了、context切替え・rollbackを維持する。
- 追加の永続状態・公開API・Opcode・VM/schema versionを増やさず、通常値の全走査と不明状態へのfallbackを設けない。

単一ownerではRc control blockが増え、共有後の部分変更には位置表の初回コピーが残る。Cloneだけの改善率を保存・再開全体の改善率に読み替えない。値Vecのコピーは対象外とする。

## 手順と検証

共有pointerと枝の独立性を先に固定し、現行Boxの全コピーを理由とするRedを確認する。変更後は通常操作と全削除の無コピー、部分変更時の分離を直接検証する。

同じprivateソースを取り込むstandalone probeで、実allocator要求と時間を別process・AB/BA同数で比較する。非空位置のClone、初回追加、全解決、dropを分ける。旧empty cloneの対照も維持し、単一ownerの構築費用を報告する。全内容検証は別processにする。

正本EldrVMとテスト方針を更新し、対象テストからworkspace CIと標準Surtrの全件一回まで確認する。最終差分を自己点検し、実測結果と残件をこの文書へ追記する。

## 実装とRed / Green

Box版では、Clone直後の位置Vec pointerが異なるため共有のテストが失敗した。全削除のテストも、pop後に予約bufferが残るため失敗した（2件、終了コード100）。Rc版は関連19件が成功した（363 skipped、終了コード0）。テストは位置表共有、普通値の操作・不一致のready・Futureを返すreadyでの無分離、部分変更の分離、全削除で参照を外すこと、保存枝の値・位置維持を固定する。

位置を変える操作ではRc::make_mutで表を独立させる。解決値は従来と同じ一回のready収集結果を使い、まず値Vecを更新して残る位置数を数える。位置が全て残れば表は共有したまま、全て消えれば共有参照を外し、一部だけ残る場合に表を分離して不要な位置を除く。独占した表の全削除は予約bufferを再利用する。値の全走査を加えず、追跡位置だけを読む。

## Cloneと解放の対照

既存の[独立owner probe](../../../../tests/profile/future_stack_clone/README.md)を、前はowned位置表、後はshared位置表として実行した。普通値0 / 16 / 256×6条件×前後×allocation / time×6 sampleで432 fresh process、864行を記録した。全内容検証は別processの36件、前後はAB/BA同数である。

普通値16個、未解決位置2件、1,000 cloneの確保要求は2,488,000→2,448,000 bytes、alloc回数は3,000→1,000だった。位置VecとそのmetadataのClone確保を除き、値Vecは複製する。never / cleared / popped / truncated / resolvedの確保要求は前後一致し、前回の空位置表Clone削減を保った。

同じ条件のcounterを無効にした時間中央値は、Clone 0.297→0.265ms、drop 0.074→0.048msだった。これは独立ownerの区間で、VM・checkpoint全体の時間ではない。[原本](future_stack_position_sharing_2026_10_14.jsonl)・[集計](future_stack_position_sharing_2026_10_14.summary.json)・[全内容検証](future_stack_position_sharing_2026_10_14_verify.json)・[build来歴](future_stack_position_sharing_2026_10_14_builds.json)。

## 構築・再開費用の分離

追加のresume probeは、普通値0 / 16、未解決位置1 / 256 / 4,096、32 clone、保存のみ / 各枝へ初回追加 / 全解決の三条件を、前後・allocation / time・各6 sampleで測った。432 fresh process、construct / clone / mutate / dropの1,728行、別processの内容検証36件。sourceの値Vecと保持用root Vecは先に確保し、construct区間では位置表構築だけを数える。元のstackを保持したまま各clone枝へ一度ずつ操作する。

単一ownerの構築では、Rc control blockにより確保要求と構築区間のliveが1表あたり16 bytes増えた。値16個・位置1件ではconstructのrequestedは56→72 bytes、alloc回数は2→2。stackの固定幅は前後32 bytesである。

普通値16個・未解決位置4,096件・32 cloneの確保要求は次のとおり。bytesを示す。

| 再開条件 | clone 前→後 | 初回変更 前→後 | clone＋初回変更 前→後 |
|---|---:|---:|---:|
| none | 18,944,768 → 17,895,424 | 0 → 0 | 18,944,768 → 17,895,424 |
| append | 18,944,768 → 17,895,424 | 37,888,000 → 38,937,856 | 56,832,768 → 56,833,280 |
| resolve | 18,944,768 → 17,895,424 | 0 → 0 | 18,944,768 → 17,895,424 |

保存のみと全解決は32 cloneで1,049,344 bytesのClone要求を省いた。全解決の表分離は0確保で、残す位置のない表をコピーしていない。初回追加ではコピーが変更時へ移り、clone＋初回変更の合計要求は512 bytes増えた。位置1 / 256件の対照でも同じ32枝分のRc header増加が残る。構築区間を合わせると、今回の保存＋初回追加の総要求差は528 bytesである。

同じ条件のclone / mutate / drop時間を合計した中央値の和を示す。msであり、別phaseの中央値を合計した指標で、全行程を一括計測した時間ではない。

| 再開条件 | clone ms 前→後 | mutate ms 前→後 | 3区間の中央値の和 ms 前→後 |
|---|---:|---:|---:|
| none | 2.101 → 2.024 | 0.000 → 0.000 | 2.939 → 2.840 |
| append | 2.090 → 1.984 | 1.257 → 1.313 | 4.152 → 4.094 |
| resolve | 2.088 → 1.965 | 2.245 → 1.489 | 5.088 → 4.202 |

Clone時の位置コピーを除いたが、共有後の部分変更の費用は残る。保持中の単一ownerの増加、部分解決後の位置表整理、RC操作とbinary配置の寄与を混ぜて、全入力で速くなったとは主張しない。[再開原本](future_stack_position_resume_2026_10_14.jsonl)・[集計](future_stack_position_resume_2026_10_14.summary.json)・[別process検証](future_stack_position_resume_2026_10_14_verify.json)・[build](future_stack_position_resume_2026_10_14_builds.json)。

各確保系列はSystemへ渡すRust GlobalAlloc要求である。clone / mutationで数えた全storageがdrop後にlive 0となることを全sampleで確認した。元のmetadata、準備した値Vec、root Vecはその区間のlive / peakに含めず、allocator内の丸め・metadata・page保持やprocess RSSを測っていない。constructは別の確保区間で、他区間のpeakへ単純に足さない。時間系列はcounterを無効にし、無効判定のwrapperは前後に残る。

二種類のprobeはそれぞれ同じ前後sourceを取り込み、Cargo artifactのSindr libraryへリンクした。source / library / binary hashとrustc flagsを保存した。Clone probeのbuildは`9c4cb4e4`時点のbuilder、resumeは`--probe`でsourceを選べる更新後のbuilderを使う。[基準patch](future_stack_position_sharing_2026_10_14_baseline.patch)は`9c4cb4e4`へ受入テストだけを加え、記録したBox sourceとのbyte一致を独立した一時checkoutで確認した。

## 最終検証

- `CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target rtk cargo nextest run -p eldr -E 'test(/future_stack_tests/) | test(/operand_stack::tests/)'`: 19 passed、363 skipped、終了コード0。
- 同じtargetの`rtk cargo nextest run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --profile ci --workspace`: 2,562 passed、69 binaries、失敗・除外0、終了コード0、nextest部分113.282秒。
- 同じtargetの`rtk proxy cargo run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --message-format=json -- test --quiet --all --format=json`: 標準Surtr 1,124件すべて成功。failed / skipped / pending / filtered / policy_errors / scope_failures / script_errorsは0、終了コード0。
- Rust workspace fmt、製品・文書・harnessの差分空白検査、Python構文、文書のローカルリンク、sourceと保存binaryのhash一致を確認した。baseline patchは原本として保持する。

[検証記録](future_stack_position_sharing_2026_10_14_validation.json)に終了コード、標準summary、Cargo artifact、最終source hashを保存した。測定・CI・標準テスト中は製品ソースを変更していない。差分を自己点検し、共有表を任意に変更する経路と旧Box経路がないこと、実変更だけの分離、全削除の無コピー、独占buffer再利用、等値・cleanup・context切替え・rollbackを確認した。level2のため別エージェントは使っていない。

## 残件

値Vecのコピー、保存中の部分変更の位置表コピー、単一ownerの16 bytes増加、独占した使用済み位置bufferの保持、Future表・cleanup検索は残る。checkpoint全体、通常CLI、RSSの前後比較は今回行っていない。今回のClone削減をそれらの改善へ一般化しない。
