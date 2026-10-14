# OI-036: ready収集mapへ値を複製しない

入力は[runtime一覧](../README.md)と[収集重複削減の残件](future_ready_snapshot_dedup_2026_10_14.md#残件)。基準はmain `c76a0980`。Eldr内のlevel2変更として、ready map用のownedコピーを省く。

## 方式と受入条件

ready mapを`HashMap<u64, &Value>`にし、Future表の値を借用して収集する。raw結果とcleanup完了を調べる共通判定は、表のownerであるprivate ProcessRuntimeへ置く。VMの既存の借用取得・owned配送はこの判定へ接続し、stack収集でも同じ判定を使う。

- 収集mapへのsource Value cloneは0回とする。各解決slotへは一回ずつValueをcloneし、独立したowned値を配送する。
- Future表とcleanup表の不変借用を保持したまま、別fieldのoperand stackだけを更新する。借用参照をstack・context・checkpoint・永続mapへ保存しない。
- Ready / Cancelledでもcleanup待ちなら収集しない。Running / unknownは未解決のままとする。重複IDの再利用・一回の収集結果・Future結果の一段置換を維持する。
- sourceと各配送slot、保存枝の独立性、通常のTask配送、取消、context切替え、checkpoint / rollbackを維持する。
- cleanup判定は重複実装せず、old owned収集経路・fallbackを残さない。公開API・Opcode・VM/schema version・永続metadataを追加しない。

Future表・cleanup表の検索、ID検査、map確保、各slotへのcloneは対象外とする。map要素型のサイズと実allocator要求・RSSを区別し、実時間の改善を測定なしに主張しない。

## 手順と検証

既存のsource cloneテストを0回へ更新し、配送slotのclone回数を直接数える。基準のowned収集でRedを確認し、共通判定のowner移動・借用収集・private stack APIを整合させてGreenを確認する。crate-local fixtureとstandalone owner probeは参照mapへ合わせ、旧版の原本は書き換えない。

前後のCargo test executableで、同じ216 processの配置・Future条件を使い、ready確認、収集用source clone、slot clone、要素型サイズと全slotの値を比較する。source・binary hashと基準patchを保存する。standalone probeも現行のprivate sourceで構築・内容確認する。

最終sourceでworkspace CIと標準Surtr全件を一回確認し、EldrVM・テスト方針・一覧・残件を更新する。自己点検後、この改善だけをコミットする。

## 結果

source clone期待値を0回にした基準では、2回・1回という理由で2件が失敗した（終了コード100）。変更後は関連56件が成功した（335 skipped、終了コード0）。cleanup完了後の既存テストも収集用clone 0回・256 slotへのclone 256回へ合わせた。

raw取得とcleanup判定はProcessRuntimeに一箇所だけ置き、VMの取得関数はそこへ委譲する。stack収集はRuntimeを不変借用し、全IDを収集してから別fieldのstackへ配送する。private stack APIは参照mapだけを受け取り、各slotで一回Valueをcloneする。位置追跡・shared buffer・owned配送の契約は維持する。参照を使う間のRuntime変更はRustの借用規則で防ぐ。

反復 / 2 ID交互 / 全ID別、6 Future条件、1 / 256 / 4,096位置を前後2 sample、AB / BAの216 fresh processで比較した。全slotの値、一段ずつのFuture結果、ready確認とread / write visitsは前後一致した。

| 4,096位置の配送可能なReady / Cancelled / Future結果 | 収集用source clone 前→後 | slot clone 前→後 |
|---|---:|---:|
| 同じID反復 | 1 → 0 | 4,096 → 4,096 |
| 2 ID交互 | 2 → 0 | 4,096 → 4,096 |
| 全ID別 | 4,096 → 0 | 4,096 → 4,096 |

Running / unknown / PendingFutureなしは両cloneとも前後0回だった。cleanup待ちはcrate-localで両clone 0回を確認した。counterは元のValueを複製する呼出しを数え、入れ子のcloneを数えない。

実際のmap型から測った`(u64, T)`はこのhostで144→16 bytesだった。要素型のサイズであり、bucket control・容量・allocator丸めを含む実確保量ではない。空mapの型サイズを測っても、その量を確保したという意味ではない。ExecutionContext 912 bytes、VM 3,120 bytesは前後一致。実時間・allocator要求・live・RSS・通常CLIは未測定である。

[原本](future_ready_snapshot_borrow_2026_10_14.jsonl)・[集計](future_ready_snapshot_borrow_2026_10_14.summary.json)・[変更前build](future_ready_snapshot_borrow_2026_10_14_before_builds.json)・[変更後build](future_ready_snapshot_borrow_2026_10_14_after_builds.json)。同じtargetを逐次buildし、Cargoが返すexecutableを別名で保存した。比較中にbuild・他のtestを行っていない。manual testとcounter条件は前後同じである。初回runnerが重複削減前の基準回数を期待した不完全系列は除外し、今回の216 processを取り直した。

[基準patch](future_ready_snapshot_borrow_2026_10_14_baseline.patch)は`c76a0980`へcfg(test)観測と受入期待値を加える。独立した一時checkoutの69 source / manifest / lockのhashと基準buildの一致を確認した。[再現手順](../../../../tests/profile/future_stack_scan/README.md#ready-mapの借用)を参照する。

standalone owner probeのfixtureも参照mapへ合わせ、現行sourceだけのclone / resumeの48 processで内容を確認した。前後性能の系列ではない。[検証](future_ready_snapshot_borrow_2026_10_14_owner_verify.json)・[clone build](future_ready_snapshot_borrow_2026_10_14_owner_clone_builds.json)・[resume build](future_ready_snapshot_borrow_2026_10_14_owner_resume_builds.json)。

## 最終検証

最終検証は同じ`CARGO_TARGET_DIR=/tmp/surtr-oi036-debug-target`で、関連nextest 56 passed / 335 skipped、`rtk cargo nextest run --locked --profile ci --workspace` 2,571 passed / 69 binaries / 失敗・除外0（114.928秒）、`rtk proxy cargo run --locked --message-format=json -- test --quiet --all --format=json` 標準1,124件すべて成功だった。終了コードはすべて0。標準のfailed / skipped / pending / filtered / policy_errors / scope_failures / script_errorsは0。

[検証記録](future_ready_snapshot_borrow_2026_10_14_validation.json)に終了コードと最終source hashを保存した。Rust fmt、差分空白、Python構文、ローカルリンク、source / binary / probe hashを確認した。測定・CI・標準テスト中に製品sourceを変更していない。最終差分を自己点検し、共通cleanup判定、借用の寿命、独立した配送値、Future結果とcleanup・context・checkpoint契約、旧owned収集経路の除去を確認した。level2のため別エージェントは使っていない。

## 残件

表の検索、IDとmap検査、map確保、slot clone、値Vecと位置表の保持費用は残る。共通判定の委譲と借用mapの実時間寄与、通常CLI・allocator・RSSとOI-037は未測定・未解決のままとする。

[後続の費用測定](future_ready_snapshot_cost_2026_10_14.md)で、Ready / Running / Futureなし、反復 / 全ID別、0 / 64要素Tupleの内部一回の実時間と要求layout bytesを確認した。通常CLI・RSS・cleanup Taskを含む費用は未測定である。
