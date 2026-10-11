# RT-5: 残る管理表と queue の checkpoint コピー削減

入力は [前回の改善と残件](list_runtime_rt5_metadata_optimization_2026_10_12.md)、[RT-5 の受入条件](list_runtime_followups.md#rt-5-repl-checkpoint-の残るコピー量)。ユーザーは RT-5 の残りの改善を依頼した。level2 とし、Eldr 内の所有表現・登録処理を改善する。意味論、VM/schema version、外部 I/O の扱いは変えない。

## 変更と受入条件

前回の不変 process 定義3表の共有に続き、handler 設定、singleton/flight/refilling/reply/waiting の管理表、deadline/run/output queue、supervisor の failure/policy/child 表も checkpoint と VM clone で共有する。可変アクセスは保存側から分離する。大きい状態を持つ worker set は既存の `RuntimeTable` に移し、index と entry の共有、変更対象 entry の複製を使う。

- 未変更の checkpoint は上記の容器を共有し、保存時に entry や文字列を複製しない。
- 保存中の live 側変更は保存側へ漏れない。handler の内側 slot 表、worker member Vec、supervisor child Vec、queue の順序を含めて確認する。
- 第1保存位置から変更して第2保存位置を作り、その後の変更を破棄して、各保存位置へ正しく復元できる。
- 不在キーへの `get_mut` / `remove`、空 queue の `pop_front` は読み取りで確認し、容器を不要に分離しない。副作用を持つ callback を二重評価する方法で変更有無を検査しない。
- worker 除去は影響を受ける entry だけを変更する。member 除去と next_index リセットの既存条件を維持する。
- deadline 到達のない queue は共有を維持する。到達時は既存の順序・future / correlation の更新・結果配送を維持する。
- 新 process 定義がない chunk は不変 registry を再構築しない。新定義がある chunk は更新済み registry を構築し、成功で確定、失敗で復元する。
- handler / supervisor の既定値は不変 registry に保持し、毎 chunk の overlay リセットと既存 override 適用を維持する。既定値を可変 overlay で汚染しない。
- snapshot 公開 API の型、内容、並び順、既存のエラー、待機、取消、rollback は維持する。

容器の共有は、最初の可変アクセス時にその容器を複製する方式とする。表 index や変更対象の entry、可変 Builder のコピーは必要な独立性のため残る。任意の書込みの定数時間化や、runtime 全体の実時間上限は保証しない。stack / frame / open-file 管理表等、process runtime 外のコピーはこの作業の対象外とする。

### 調査で見つけた原子性の修復

共有化前から、失敗 chunk が boot plan に追加した singleton / standard override / handler override を rollback で取り除かない問題があった。Astra に相談し、作業開始時と共有化後の両コードで同じ2件の Red を確認した。通常の `ReplAppendOnly` は boot plan 追加を拒否するため、影響は内部 `push_atomic` と追加を許可する `Preload` の経路である。

コピー削減とは別の契約修復として、checkpoint に3配列の元の長さを保存し、rollback で追加分を truncate する。既存 prefix とその他の boot plan 項目は維持する。公開 `Preload` の実行時失敗後に次の chunk へ override が残らないこと、3配列の復元と prefix 保持をテストで固定する。version は変更しない。

以前の失敗テストに使っていた top-level `Return` は実行前の検証で拒否され、登録後 rollback の検証になっていなかった。失敗入力を `RuntimeContractViolation` に変更し、事前検証の成功と指定した実行時エラーメッセージを確認する。以前のレポートの失敗境界は、この修正後の全体検証で補う。

## 手順と測定

1. storage 共有、入れ子変更、多段 rollback、無変更アクセス、定義なし chunk の registry 維持をテストで固定し、現行のコピー経路で Red を確認する。
2. 前回の実装を含む作業開始時のソースを退避し、同じ crate-local probe を release で測る。準備・assert・出力は区間外とする。
3. 管理表と queue の共有、worker entry COW、既定 overlay 共有、定義なし chunk の再構築削減を順に実装する。
4. VM 初期化・保存・破棄・初回/2回目変更・rollback・実 chunk を分け、変更前後を交互測定する。初期化の追加 allocation や、初回可変アクセスに移る費用を改善率へ隠さない。
5. Eldr / REPL、workspace CI、標準 Surtr テスト全件と独立レビューを実施する。デグレや新規問題は Astra に相談する。

時間の合否閾値は設けない。時間、requested allocation、論理 live、RSS は別指標とし、未測定の指標を推定しない。前回の原本とソースを保持し、本作業の原本を別名で保存する。

## 結果

作業開始状態は main `1dc1aa3a` に前回の不変3表共有の未コミット差分を加えた状態。基準ソースを `/tmp/surtr-a427-rt5-rest/before-sources/` に保存し、前回の実装を取り消さずに進めた。製品コードの変更は Eldr 内に閉じる。

### 実装

- `CheckpointValue<T>` により残る管理表・queue を共有し、保存中の可変アクセスで容器を分離する。不在 key と空 queue は先に検査する。単独所有の map / set では不要な二重 lookup を省く。
- worker set を既存 `RuntimeTable` へ移し、member 除去と非ゼロの範囲外 index 補正が必要な entry だけを変更する。
- deadline は期限到達 entry がなければ共有を維持する。到達時は元順序で未到達 entry を新 queue に保存し、Ready / Cancelled / 削除済み future の到達 entry も除去する。重複の既存処理も維持した。
- 不変 registry に handler / supervisor の既定値を保持し、毎 chunk の overlay リセットを共有参照の置換で行う。新定義なしの registry 再構築を除いた。override 適用は従来の処理を使う。
- 別の原子性修復として boot plan の追加3配列を rollback で元の長さへ戻した。

### 測定

[harness と区間定義](../tests/profile/checkpoint_collections/README.md)。Apple arm64 / macOS、root lock による release unit-test binary。各区間32反復、変更前後を AB/BA 順で交互に各条件10 process 測った。ビルド・テストと重ねていない。時間の合否閾値は設けていない。

保存・書込み・rollback は16 / 128 / 1,024件の管理表と各 queue を持つ synthetic state。実 process / Builder は0件で、fake ID を scheduler に投入しない。各 output / failure 文字列は256 bytes、worker member は3個。`init` と実 `chunk` は同件数の process 定義・handler/policy を持つ metadata-only VM で測る。`chunk_shared` は別 VM clone と checkpoint が保存側を保持し、`chunk_unique` は区間外で準備した独立 VM を使う。どちらも新定義・override追加なしの `Halt` chunk であり、override 再適用費用や compiler / REPL 入力全体の測定ではない。

下表は各 process の区間合計の中央値を32で割った1操作当たりの時間。

| 件数 | 区間 | 変更前 ms | 変更後 ms | 時間の減少率 |
|---:|---|---:|---:|---:|
| 16 | `init` | 0.026290 | 0.027145 | -3.3% |
| 16 | `save` | 0.007857 | 0.000389 | 95.0% |
| 16 | `drop` | 0.004642 | 0.000143 | 96.9% |
| 16 | `first_write` | 0.000844 | 0.008219 | -874.1% |
| 16 | `second_write` | 0.000178 | 0.000234 | -31.5% |
| 16 | `rollback` | 0.005020 | 0.004801 | 4.3% |
| 16 | `chunk_shared` | 0.033641 | 0.001563 | 95.4% |
| 16 | `chunk_unique` | 0.038609 | 0.001165 | 97.0% |
| 128 | `init` | 0.126673 | 0.123868 | 2.2% |
| 128 | `save` | 0.055632 | 0.000465 | 99.2% |
| 128 | `drop` | 0.033927 | 0.000143 | 99.6% |
| 128 | `first_write` | 0.001419 | 0.044963 | -3069.4% |
| 128 | `second_write` | 0.000244 | 0.000304 | -24.5% |
| 128 | `rollback` | 0.034126 | 0.028447 | 16.6% |
| 128 | `chunk_shared` | 0.240708 | 0.001952 | 99.2% |
| 128 | `chunk_unique` | 0.282417 | 0.001637 | 99.4% |
| 1,024 | `init` | 0.924553 | 0.904691 | 2.1% |
| 1,024 | `save` | 0.485344 | 0.000832 | 99.8% |
| 1,024 | `drop` | 0.274981 | 0.000210 | 99.9% |
| 1,024 | `first_write` | 0.004321 | 0.401678 | -9196.0% |
| 1,024 | `second_write` | 0.000441 | 0.000624 | -41.6% |
| 1,024 | `rollback` | 0.274529 | 0.225865 | 17.7% |
| 1,024 | `chunk_shared` | 2.001920 | 0.006211 | 99.7% |
| 1,024 | `chunk_unique` | 2.289874 | 0.005572 | 99.8% |

| 件数 | 保存＋初回更新 変更前 ms | 変更後 ms | 時間の減少率 |
|---:|---:|---:|---:|
| 16 | 0.008690 | 0.008587 | 1.2% |
| 128 | 0.057019 | 0.045401 | 20.4% |
| 1,024 | 0.489474 | 0.402423 | 17.8% |

「保存＋初回更新」は同じ process sample の区間値を足してから集計した。中央値同士は加算していない。初回更新への費用移動を含めても、1,024件では約17.8%短くなった。保存だけの99.8%を更新時の改善率として扱わない。

`second_write` は1,024件で0.000441 → 0.000624 msとなり、約41.6%（約0.18 µs）の遅化が残った。共有のない状態での二重 lookup を省く変更を加えた後の値である。Astra に相談し、共有化後の追加費用と、要因別の寄与が未測定であることを記録した。更新だけが支配的な workload 全体の改善は保証しない。

16件の初期化時間は約3.3%増えたが、観測範囲は重なる。空の容器にも `Rc` allocation が加わるため、初期化の費用は保存の改善と分ける。新定義の追加時は全 registry の構築が必要である。

| 件数 | process peak RSS 変更前 MiB | 変更後 MiB |
|---:|---:|---:|
| 16 | 5.000 | 5.055 |
| 128 | 8.156 | 7.477 |
| 1,024 | 32.461 | 25.914 |

RSS は `/usr/bin/time -l` による process 全期間の高水位であり、入力準備、全8区間、区間外の assert、JSON、test harness を含む。8区間に同じ値を記録するが、加算しない。区間別 peak、論理 live、保持量、requested allocation の減少とは読み替えない。16件の RSS は0.055 MiB増え、観測範囲は重なる。override 適用後は既定値と可変 overlay の容器が共存する。override を含む保持量はこの測定で分離していない。

原本を分けて保持した。

- [変更前](rt5_collections_2026_10_12_before.jsonl)・[集計](rt5_collections_2026_10_12_before.summary.json): 前回完了時点、120行。
- [共有化直後](rt5_collections_2026_10_12_after.jsonl)・[集計](rt5_collections_2026_10_12_after.summary.json)、[初回交互測定](rt5_collections_2026_10_12_recheck.jsonl)・[集計](rt5_collections_2026_10_12_recheck.summary.json): 二重 lookup の検討前。
- [最終コード](rt5_collections_2026_10_12_final_after.jsonl)・[集計](rt5_collections_2026_10_12_final_after.summary.json)、[採用した交互原本](rt5_collections_2026_10_12_final_recheck.jsonl)・[集計](rt5_collections_2026_10_12_final_recheck.summary.json): 480行、48集計、各10 process。

build / binary / probe / production source / dirty diff / lock / runner の hash を保存した。原本各32値の合計、各10 process の min/median/max、AB/BA の偏りがないことを root と Astra が照合した。変更前後の probe の fixture と8測定区間は同じで、後続差分は境界テスト追補・test helper可視性・性能では使わない失敗 branch の変更である。最終 runtime / probe hash と実測 build を照合した。

### 検証とレビュー

- コピー経路の Red: release で profile 1 passed、共有・未変更 entry・no-op・registry 維持の5件が意図した storage 比較で失敗。原子性修復の2件は作業開始時ソースと共有化後の修復前ソースで、どちらも実行時に失敗した。
- 失敗テストの `Return` 事前拒否を訂正。`verify_chunk` 成功を明示し、`RuntimeContractViolation` の指定メッセージが返ることを確認して、登録後の実行時エラーを検証する。公開 `Preload` 経路1件と3配列/prefix直接復元1件を固定した。
- `rtk cargo nextest run -p eldr checkpoint_`: 最終コードで24 passed、319 skipped。
- `rtk cargo nextest run -p eldr -p xldr`: 447 passed、92 skipped。単独所有 lookup 最適化の直前に実行した。
- `rtk cargo nextest run --profile ci --workspace`: 最終コードで2,523 passed、失敗0（69 binaries）。
- `rtk proxy cargo run -- test --quiet --all`: 最終コードで終了コード0、標準 Surtr テスト全件に成功。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。結果文書も Astra が確認し、修正必須の指摘なし。
- サブエージェントによるテスト・runner と、Astra による最終製品差分・原子性修復・性能結果のレビューで修正必須の指摘なし。Astra はビルド・テストを再実行していない。

RT-5 の残る管理表・queue の保存時コピーと、定義なし chunk の不変 registry 再構築を削減した。変更時の容器/index/entry/Builder コピー、反復更新の追加費用、稼働 process / 大きい Builder / override を含む実利用の詳細測定は残る。runtime 全体の定数時間や一律の性能改善は保証しない。実装・検証完了時点ではコミット・main 統合は未実施だった。


## 分割コミット

2026-10-12の統合依頼により、検証済みの最終コードを次の成果に分割した。

- `d4fb002e`: 不変 process registry の共有。
- `875145d3`: 管理表・queue の共有と定義なし chunk の再構築削減。
- `c6332ad9`: boot plan の追加分を rollback で破棄。

測定原本・harness・結果文書は後続の独立コミットにまとめる。最終Rustファイルが検証時の退避ソースとbyte一致することを確認した。main側の増分は対象外の`doc/open-issues.md`だけであり、その変更は保持して統合する。製品コードを変更しないGit操作のため、全体テストは前回の2,523件成功と標準Surtr全件成功の証拠を使用し、再実行しない。
