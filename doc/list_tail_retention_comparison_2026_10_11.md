# RT-2: 短い Packed tail の保持方式比較

測定日: 2026-10-11。対象 commit: `ee13e44820993fcfc56ecfa216ce4165be67f4ca`。
状態: RT-2 の方式比較を完了。製品への採用・実装は未決定。

入力は [前回の測定レポート](/Users/haruca/.codex/worktrees/4c18/surtr/doc/list_runtime_measurement_report_2026_10_11.md) の RT-2 と
[List / VM の残課題](list_runtime_followups.md)。今回は RT-2 だけを進めた。
製品コード、言語仕様、VM/schema version、通常の correctness test は変更していない。
独立した手動 harness による level2 相当の比較である。

## 結果と判断

長さ1の suffix だけを残す条件では、chunk モデルと明示的 compaction のどちらも
現行 Packed の全 buffer 保持を減らせた。ただし root checkpoint が生存していれば、
いずれも元 List 全体を解放できない。別の suffix が残る場合、chunk は最古の参照 chunk
より前を解放できるが、末尾だけの compaction はその別 suffix が保持する元 buffer を解放できない。

chunk は保持の単位を小さくし、compaction は対象 suffix の Value を一回 clone する。
解放する総 payload が消えるわけではなく、最後の suffix の破棄から、元 List や checkpoint の破棄へ
大きな解放処理が移る。最後の破棄だけの短縮を VM 全体の実時間上限の改善とは扱わない。
この測定値だけで採用方式を確定しない。

## 方法と再現性

[README](../tests/profile/list_tail_retention/README.md) に入力、受入条件、再現コマンドと指標を記載した。

- 現行 `ListHandle` の共有継続、capacity 256 の前方 Rc chunk 鎖、末尾1要素だけの明示的 compaction を比較した。
- n=10,000 / 100,000、位置番号を持つ Int・256-byte String・その String を3段の singleton List で包む payload を使った。入れ子は要素ごとに独立生成する。
- ownership は suffix のみ、中点 n/2 からの別 suffix、元 List の clone を持つ checkpoint 代理の3条件。checkpoint は実 VM の保存操作ではない。
- 54条件を独立 process で各5回。割当カウンタ有効と無効の2系列、合計540 process・3,780区間サンプルを採った。build、advance、compact、各所有者の破棄を分けた。
- Apple arm64 / macOS 26.6.2、rustc 1.94.1、release / System allocator。両 binary は `--locked` で build した。依存版は root lock と一致する。
- 区間の間は scalar の測定結果だけを保持し、全所有者の破棄後に JSON を出す。ビルド・内容検証・JSON 出力を区間外に置いた。最終性能系列の実行中は、この作業の別ビルド・テスト・検証を並行しなかった。

今回の Int は位置番号で、前回の固定値7とは入力が異なる。String も順序検証用の番号を含み、
生成方法が異なる。前回の build 時間と今回を直接比較しない。今回の3方式の間では同じ内容を使う。

requested bytes は allocator layout の要求量で、物理メモリ消費量や RSS ではない。
残存量は build 以降の区間 live delta の和である。以下の保持量・割当はカウンタ有効系列、
時間・process peak RSS は無効系列の中央値を示す。各値は5回の観測で、合否閾値ではない。
全区間の最小・最大は集計原本に保存した。

## 元 List 破棄後の保持量

n=100,000。単位は requested MiB。compaction は末尾だけを置換し、別 suffix / checkpoint は維持する。

| payload | 残る所有者 | 現行共有 | chunk モデル | 明示的 compaction |
|---|---|---:|---:|---:|
| int | 末尾1 | 16.021736 | 0.025681 | 0.000565 |
| int | 中点 suffix + 末尾1 | 16.021736 | 8.032654 | 16.022301 |
| int | root checkpoint + 末尾1 | 16.021736 | 16.039597 | 16.022301 |
| string | 末尾1 | 37.384071 | 0.059860 | 0.000801 |
| string | 中点 suffix + 末尾1 | 37.384071 | 18.730896 | 37.384872 |
| string | root checkpoint + 末尾1 | 37.384071 | 37.401932 | 37.384872 |
| nested | 末尾1 | 87.738075 | 0.140427 | 0.001305 |
| nested | 中点 suffix + 末尾1 | 87.738075 | 43.948181 | 87.738632 |
| nested | root checkpoint + 末尾1 | 87.738075 | 87.755936 | 87.738632 |

chunk の末尾1が保持するのは最後の chunk 全体である。n=10,000 では16要素、
n=100,000 では160要素が残る。中点 suffix もある場合はそれぞれ5,136 / 50,080要素が残る。
root checkpoint がある間は全 n 要素が残る。この要素数は chunk 境界と所有条件から求めた値で、
保持量の実測と整合する。

中点 suffix / checkpoint を破棄した後は、chunk は末尾 chunk のみ、compaction は末尾の
コピーのみになる。現行共有は末尾が消えるまで全 buffer を保持する。
全条件で最後の破棄後の workload 残存量は0だった。

## compaction のコピーと追加保持

末尾1要素だけを clone する compact 区間の allocation/reallocation は次のとおり。
この費用は n や ownership に依存せず今回の全条件で同じだった。

| payload | allocation回数 | realloc回数 | requested bytes | 元 List 等をすべて落とした後の残存 bytes |
|---|---:|---:|---:|---:|
| int | 3 | 0 | 592 | 592 |
| string | 3 | 0 | 840 | 840 |
| nested | 2 | 0 | 584 | 1368 |

入れ子の ListHandle clone は内部 buffer を共有する。nested の584 bytesは外側の新しい
Vec / Rc の要求量であり、元 List 破棄後は共有した末尾 payload の buffer も残るため、
最終的に1,368 bytesを保持する。深いコピーが省かれたことと、保持される内部 buffer を区別する。
`collect()` の Vec は長さ1でも最小限の1要素容量とは限らない。

checkpoint や別 suffix が元 buffer を保持している間、compaction は元 buffer の保持を減らさず、
新しい外側 buffer 等の分だけ保持量が増える。今回の末尾1では増分は小さいが、
大きな suffix への compaction や複数所有者の一括置換の費用を推定しない。

## 解放時点と時間・RSS

n=100,000、suffix のみ。空操作の compact と root 破棄は `—` とする。

| payload | 方式 | build ms | advance ms | compact ms | root破棄 ms | 最後のsuffix破棄 ms | process peak RSS MiB |
|---|---|---:|---:|---:|---:|---:|---:|
| string | shared | 19.364 | 0.756 | — | — | 4.945 | 39.234 |
| string | chunk | 19.246 | 0.563 | — | 2.568 | 0.006 | 44.641 |
| string | compact | 18.741 | 0.745 | 0.002 | 2.663 | 0.000 | 39.234 |
| nested | shared | 30.804 | 0.743 | — | — | 14.246 | 99.250 |
| nested | chunk | 31.709 | 0.561 | — | 13.453 | 0.022 | 104.688 |
| nested | compact | 30.538 | 0.746 | 0.005 | 14.758 | 0.001 | 99.250 |

String の最後の suffix 破棄は、shared が2.424〜6.200 ms、chunk が0.006〜0.010 ms、
compact が0.000334〜0.000459 msだった。一方、chunk / compact は root の破棄で
大量の要素を解放する。最後の破棄が短くなることを、解放仕事全体の消滅と解釈しない。

chunk の process peak RSS は今回の String / nested 条件では共有方式より高かった。
全方式が最初に n 要素を作るため、suffix 保持量の改善が process 全期間の peak RSS の改善になるとは限らない。
allocator の管理領域・断片化・resident page の再利用や解放は requested bytes から求めない。
chunk の build / advance 時間の差も、VM / Builder 全体の改善率へ換算しない。

## 検証と独立レビュー

- 54条件×5回×2系列の最終測定は終了コード0。各系列1,890区間、378区間集計。全 advance の allocation/reallocation は0、全最後の破棄後の残存量は0だった。
- n=2 / 255 / 256 / 257 / 511 / 512 / 513 / 10,000 / 100,000 と全方式・payload・ownership を組み合わせた243件の別 process で、全要素の長さ・順序・内容、空の head/tail、所有者破棄後の有効性を確認した。性能原本には含めていない。
- `rtk cargo nextest run -p sindr -E 'test(/list|cons/)'`: 26 passed、100 skipped、失敗0。既存の List / Cons 契約と解放境界を確認した。
- サブエージェントが現行コード・正本との境界を独立確認した。Astra が最終 harness を静的レビューし、必須修正はなかった。製品のデグレや新規問題はこの作業では見つかっていない。
- 製品コードに触れていないため、workspace CI gate、標準 Surtr 全件、compiler/generic do/REPL/Task/process の性能検証は実行していない。

割当原本: [生データ](list_tail_retention_2026_10_11_count.jsonl)・[集計](list_tail_retention_2026_10_11_count.summary.json)。
時間原本: [生データ](list_tail_retention_2026_10_11_time.jsonl)・[集計](list_tail_retention_2026_10_11_time.summary.json)。
集計の metadata に source / lock / binary の SHA-256 と実行環境を保存した。

## 製品への移行前に決めること

RT-2 の測定段階は完了したが、最適化は未解決である。方式の採用には次の受入仕様が必要になる。

- chunk: 完成した Builder / `from_items(Vec<Value>)` からの所有権移動、構築時の追加コピー・peak、chunk サイズ、混在した Cons / chunk の論理順と反復解放を決める。今回の直接生成モデルは元 Vec の移譲を検証していない。
- compaction: 実行する入口と対象 suffix を決める。通常の tail は引き続き仮想 O(1) とし、tail ごとの全コピーを導入しない。大きな suffix の clone / allocation と予算・取消・rollback の関係を確認する。
- 両方式: 永続性、head/tail/uncons/len/cons の仮想 O(1)、mapper の回数・順序、source trace、待機・取消、checkpoint 保存状態の独立性を維持する。製品導入時に最小の回帰テストから検証する。

chunk は末尾以外の所有者を強制的にコピーせず、保持を最古の参照 chunk まで絞る候補として
検討できる。compaction は短い suffix を独立させる入口が明確な場合の候補になる。
今回の境界を仕様へ反映してから製品導入へ進む。RT-3〜5は今回進めていない。
