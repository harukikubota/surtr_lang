# RT-5: process 定義 metadata の checkpoint コピー削減

入力: `/Users/haruca/.codex/worktrees/4c18/surtr/doc/list_runtime_measurement_report_2026_10_11.md` の RT-5 と [残課題](list_runtime_followups.md)。ユーザー依頼に基づき、このうち一項目を改善する。level2: Eldr 内の所有表現を変更し、言語の意味論とフェーズ間契約は維持する。

後続の [RT-5残件改善](list_runtime_rt5_remaining_optimization_2026_10_12.md)で、管理表とqueueの共有、定義なしchunkの再構築削減を実施した。本書の値と結果は前回完了時点の記録である。失敗テストのtop-level `Return` は事前拒否だったため、後続作業では事前検証が通る実行時エラーに直して登録後rollbackを再検証した。

## 対象と受入条件

`ProcessRuntime` の `specs_by_id`、`spec_id_by_name`、`specs_by_name` は定義登録時に置換され、その後は読み取り専用である。現在の checkpoint は3表とその文字列・定義をすべて複製する。3表を一つの不変 registry にまとめて `Rc` で所有し、checkpoint と VM clone では registry を共有する。登録時は新しい registry を構築して置換し、保存側へ更新を漏らさない。

- ID、canonical 名、名前からの ID、定義内容の対応を維持する。既存の検証・診断・名前解決規則は変えない。
- 未変更の checkpoint は3表の backing storage を共有する。複数の checkpoint と live VM の所有関係を確認する。
- 定義追加・置換後も保存側は以前の定義を保持する。rollback は保存位置の registry に戻る。
- 成功 chunk の定義は後続から利用でき、失敗 chunk の定義は残らない。既存定義を失敗で失わない。
- 保存側の破棄後も live 側の lookup が有効である。

`handler_contexts` は handler override で更新されるため対象外。root supervisor policy、singleton/worker/reply/waiting 表、queue、Builder、bytecode 側 metadata の複製、停止済み PID の回収も変更しない。旧3表を並行して保持する経路や lookup のフォールバックは追加しない。VM/schema version は変更しない。

## 実装と検証

1. crate-local テストで storage 共有の Red を確認し、登録置換と rollback の境界を固定する。
2. 現行コードの保存・破棄・登録・rollback・実 chunk の時間を測る。入力構築と出力整形は測定外に置く。
3. 不変 registry に移行し、同じテストと測定を実行する。測定には時間の合否閾値を使わない。
4. Eldr と REPL の関連テスト、workspace CI と標準 Surtr テストを実行し、独立レビューを行う。デグレや新規問題は Astra に相談する。

`execute_chunk` は既存定義を含む metadata を再登録するため、そのコピーは残る。保存単独の削減率を chunk 全体へ読み替えず、RT-5 全体の完了や runtime 全体の定数時間保存を主張しない。時間、requested allocation、論理 live、RSS は別指標として扱い、測っていない指標を推定しない。

## 結果

実装基準は main の `1dc1aa3a`。3表を `Rc<ProcessSpecRegistry>` に移行した。registry 自体は `Clone` を実装せず、登録時は新規構築して置換する。旧3表を runtime に並行保持する経路は削除した。ID の構築、canonical 名の検索、snapshot 表示、エラー経路は既存の処理を維持した。

### 測定

[harness と区間の定義](../tests/profile/checkpoint_metadata/README.md)。Apple arm64 / macOS、root lock を使った release unit-test binary。process / future / Builder は0件とし、16 / 128 / 1,024 定義を比較した。各定義は2 handler、1 dependency handler と256-byte引数を持つ。

区間ごとに32反復の合計を取り、変更前後を AB / BA の順序で交互に各10 process 測った。下表は process 合計の中央値を32で割った1操作当たりの時間。ビルド・テストを測定と並行実行していない。準備・assert・表示は区間外。requested allocation、live bytes、RSS、物理コピー量は未測定。

| 定義数 | 区間 | 変更前 ms | 変更後 ms | 時間の減少率 |
|---:|---|---:|---:|---:|
| 16 | `save` | 0.011254 | 0.002585 | 77.0% |
| 16 | `drop` | 0.006599 | 0.001605 | 75.7% |
| 16 | `register` | 0.020181 | 0.015167 | 24.8% |
| 16 | `rollback` | 0.007076 | 0.007117 | -0.6% |
| 16 | `chunk_shared` | 0.072088 | 0.026482 | 63.3% |
| 16 | `chunk_unique` | 0.072376 | 0.031930 | 55.9% |
| 128 | `save` | 0.084921 | 0.017570 | 79.3% |
| 128 | `drop` | 0.050555 | 0.011316 | 77.6% |
| 128 | `register` | 0.150251 | 0.109437 | 27.2% |
| 128 | `rollback` | 0.050798 | 0.050592 | 0.4% |
| 128 | `chunk_shared` | 0.546159 | 0.190382 | 65.1% |
| 128 | `chunk_unique` | 0.546191 | 0.230749 | 57.8% |
| 1,024 | `save` | 0.744958 | 0.177050 | 76.2% |
| 1,024 | `drop` | 0.404771 | 0.093914 | 76.8% |
| 1,024 | `register` | 1.200519 | 0.896784 | 25.3% |
| 1,024 | `rollback` | 0.407381 | 0.408094 | -0.2% |
| 1,024 | `chunk_shared` | 4.537091 | 1.599960 | 64.7% |
| 1,024 | `chunk_unique` | 4.453169 | 1.891891 | 57.5% |

1,024 定義では checkpoint 保存が76.2%、単独所有の `Halt` chunk が57.5%短くなった。保存には共有対象外の handler 設定等の複製が残るため、checkpoint 全体が定数時間になったという結果ではない。`register` も定義全体の再構築を維持する。

`chunk_shared` は元 VM clone が旧 registry を保持する条件であり、最終解放を含む `chunk_unique` と分けた。通常の compiler・REPL 入力処理全体、稼働中 process、長い Builder、queue の費用はこの測定では扱っていない。全体 gate は性能値の保証ではない。

rollback は全条件で変更前後の観測範囲が重なり、中央値差も −0.4〜+0.6%だった。この測定では明確な短縮を確認できなかった。

初回の逐次測定では16定義の登録・rollback が遅くなった。Astra に相談し、実行順による時間の低下と、共有元保持条件の影響を確認した。単独所有 chunk を追加し、順序を釣り合わせた再測定では大幅な遅化は再現しなかった。初回のばらつきの原因自体は未特定であり、実装によるデグレや改善とは断定しない。

原本には build / source / lock hash も記録した。

- 初回5区間: [変更前](rt5_metadata_2026_10_12_before.jsonl)・[集計](rt5_metadata_2026_10_12_before.summary.json)、[変更後](rt5_metadata_2026_10_12_after.jsonl)・[集計](rt5_metadata_2026_10_12_after.summary.json)。各75行。`chunk` は共有元保持条件。
- 単独所有を追加した6区間: [変更前](rt5_metadata_2026_10_12_before_v2.jsonl)・[集計](rt5_metadata_2026_10_12_before_v2.summary.json)、[変更後](rt5_metadata_2026_10_12_after_v2.jsonl)・[集計](rt5_metadata_2026_10_12_after_v2.summary.json)。各90行。
- 採用した交互再測定: [原本](rt5_metadata_2026_10_12_recheck.jsonl)・[集計](rt5_metadata_2026_10_12_recheck.summary.json)。360行、36集計、各10 process。原本の個別値32件の合計と集計の min / median / max を Astra が照合した。

変更前後の binary は `/tmp/surtr-a427-rt5/` に別名で保存した。変更前の再ビルド時だけ製品ソースを基準 commit の状態へ戻し、同じ probe でビルド・測定後に最終ソースを復元した。最終ソースと退避した3ファイルの byte 一致を確認した。旧製品経路は最終ソースに残していない。

### 検証とレビュー

- Red: 変更前の release unit test で3 passed / 1 failed。共有テストが3表 storage の `ptr::eq` で失敗し、既存の登録・rollback・chunk 動作は成功した。
- `rtk cargo nextest run -p eldr checkpoint_metadata_tests`: 4 passed、329 skipped。共有、登録置換、成功・失敗 chunk の境界と小さい profile 条件を確認した。その後、VM clone の共有 assert と単独所有 chunk の profile を追加した。
- `rtk cargo nextest run -p eldr -p xldr`: 437 passed、92 skipped（default profile）。単独所有 profile の追加前に実行した。最終追加分は下記の全体検証で確認する。
- `rtk cargo nextest run --profile ci --workspace`: 最終コードで2,513 passed、失敗0（69 binaries）。単独所有 profile の追加後に実行した。
- `rtk proxy cargo run -- test --quiet --all`: 最終コードで終了コード0。標準 Surtr テスト全件に成功。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。変更後測定の runtime / probe / binary hash を最終ファイルと照合し、一致を確認した。
- サブエージェントによる受入条件・テスト照合、Astra による最終製品差分・テスト・測定のレビューで修正を要する指摘なし。Astra はコードと原本を確認し、ビルド・テストを再実行していない。

RT-5 のうち不変 process 定義3表の保存コピー削減を完了した。その他の metadata / queue / table と登録再構築は残るため、RT-5 全体は未解決。実装・検証完了時点ではコミット・main 統合は未実施だった。
