# RT-5 Builder共有化のmainへの反映

作業日: 2026-10-13。入力は [runtime一覧](../README.md)、[Builder改善の仕様](list_runtime_rt5_builder_optimization_2026_10_12.md)、[構築経路の選択](list_builder_route_decision_2026_10_12.md)、[空Builder予約容量の修正](list_builder_owned_tail_2026_10_12.md)。変更前はRegex共有化を含むmain `c577b32d`。mainへ反映し、Builder共有化を独立したコミットにまとめた。

## 変更と受入条件

Eldr内に閉じるlevel2の改善として、`9ea8`の採用済みRc segment方式を取り込んだ。中断中Taskのentryを初回変更するとき、Builder cloneは保存済みValueを複製せずsegmentを共有し、追加を新しい末尾へ積む。複数の保存枝は独立して再開できる。空の保存では元の予約bufferを共有化しない。

List.flat_mapとGeneratorのmaterializeは共通の完成継続を使う。単一の独占bufferはPackedへ移し、共有prefixは一要素またはsegment境界ごとに1 reductionを消費してConsへ接続する。完成途中の保存はcursorと不変の部分出力を保持し、Complete / Errの配送は共通stepで行う。旧Vec全cloneの製品経路、サイズによる方式切替、無課金の一括finishは残さない。公開API・標準 `.srt`・builtin ID・VM/schema versionは変更しない。

[正本EldrVM](../../../../docs/dev/EldrVM_spec.md)・[Process Runtime](../../../../docs/dev/ProcessRuntime_spec.md)と[テスト方針](../../../../docs/dev/テスト方針.md)を整合させた。元worktreeの製品sourceは変更していない。別調査のallocator feature・memory-profile moduleは取り込んでいない。long probeと短い出力のsmoke testはcrate-localに置く。

ユーザーの削除依頼に従い、このターンで作成した `rt5-builder-adoption` worktreeは削除した。元の `9ea8` worktreeは保持する。作業と測定・検証はmainで行った。

## 測定と来歴

[checkpoint_builder](../../../../tests/profile/checkpoint_builder/README.md)の実VM・detached Task probeを使う。Task数1 / 8 / 32、保存時Builder長32 / 1,024 / 16,384、位置番号Int / 256-byte Stringの18条件。保存時長の後に2,048件を追加するので、短い完成出力の測定とは扱わない。

前後とも同じ測定関数・counter・runner・root lockで、独立targetへrelease unit-test binaryをbuildした。前後のsource snapshot、Cargo compiler-artifactのmanifest / executable、保存binaryのSHA-256を照合した。[変更前build](rt5_builder_adoption_2026_10_13_before_build.json)・[変更後build](rt5_builder_adoption_2026_10_13_after_build.json)へ記録した。旧Vec側に追加したcounterは`cfg(test)`のみで、両版の時間からcounter overheadを差し引いていない。[baseline instrumentation patch](rt5_builder_adoption_2026_10_13_baseline_instrumentation.patch)を保存した。`c577b32d`の別checkoutにこのpatchとharnessを置けば変更前を再buildできる。

前後各1 process・4反復の初期系列はbuild metadataとcounter照合に使う。採用する時間比較は全18条件を前後各6 fresh process、各4反復でAB/BA同数にした交互系列である。216 process、8,640区間、360 phase集計。保存・再開・再保存・rollback・共有完成・保存側破棄の9区間をprocessごとに加算し、4反復で割ってから6 processの中央値を取った。phase中央値を足していない。ビルド・テストと測定を重ねていない。

| Task数 | payload | 保存付き9区間 前 ms | 後 ms | 変化 |
|---:|---|---:|---:|---:|
| 1 | int | 1.746 | 1.576 | -9.8% |
| 1 | string | 2.221 | 1.891 | -14.9% |
| 8 | int | 17.091 | 13.306 | -22.1% |
| 8 | string | 26.356 | 26.839 | +1.8% |
| 32 | int | 59.448 | 47.703 | -19.8% |
| 32 | string | 113.390 | 64.600 | -43.0% |

表は保存時長16,384の条件である。32 Taskでは保存由来のコピーが、旧Vecの初回524,288件＋再保存後540,672件から、再開時0・共有完成時524,288件になった。完成時の共有Value cloneとCons構築は残る。初回再開だけの時間を、完成までの改善率として扱わない。

保存なしの32 Task / 長さ16,384の `unique_complete` は、Int 11.295→12.735ms、String 12.891→13.181msだった。既知の通常経路の追加費用は今回も残る。初期構築は区間外なので、この値を初回segment確保の費用とは扱わない。8 Task / 長いStringの保存付き合計も26.356→26.839msであり、全条件の高速化は主張しない。

同じ32 Task条件のprocess peak RSS中央値はInt 255.977→279.141 MiB、String 628.039→539.445 MiB。全4反復・10区間・準備・assert・JSON・test harnessを含む高水位であり、区間の保持量やrequested allocationではない。logical live / requested allocation / 物理copy byteは今回測っていない。IntのRSS差と通常経路の費用は[OI-039](../../../open-issues.md#oi-039-builderコピー削減後の通常経路とintのrss差)へ残す。

[採用した交互原本](rt5_builder_adoption_2026_10_13_balanced.jsonl)・[phase集計](rt5_builder_adoption_2026_10_13_balanced.summary.json)・[processごとの9区間合計集計](rt5_builder_adoption_2026_10_13_totals.json)。初期系列は[変更前](rt5_builder_adoption_2026_10_13_before.summary.json)・[変更後](rt5_builder_adoption_2026_10_13_after.summary.json)。別processで前後各18条件の全文・順序・長さ・Future結果・Task回収・callback再実行なしを確認した。[変更前検証](rt5_builder_adoption_2026_10_13_before_verify.summary.json)・[変更後検証](rt5_builder_adoption_2026_10_13_after_verify.summary.json)。時間・RSSを検証系列へ混ぜていない。

## 検証と点検

- 初回の計測テスト移植でimport漏れによるcompile errorがあり修正した。この失敗はRedと扱わない。
- Red: `checkpoint_shares_long_prefix_without_copying_values` が旧Vecの1,024コピーに対し期待0で失敗。終了コード100。
- `CARGO_TARGET_DIR=/tmp/surtr-rt5-adoption-debug-target rtk cargo nextest run -p eldr -E 'test(/list_builder::tests/) | test(/generator_tests/) | test(/list_flat_map_tests/)'`: 38 passed、330 skipped、終了コード0。
- `CARGO_TARGET_DIR=/tmp/surtr-rt5-adoption-debug-target rtk cargo nextest run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --profile ci --workspace`: 2,548 passed、69 binaries、失敗・除外0、終了コード0、nextest部分134.541秒。
- `CARGO_TARGET_DIR=/tmp/surtr-rt5-adoption-debug-target rtk proxy cargo run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --message-format=json -- test --quiet --all --format=json`: 標準Surtr 1,124件すべて成功。失敗・skip・pending・filter・policy error・scope failure・script errorは0、終了コード0。Cargo artifactのSindr・Eldr・Rune manifestがmainを指し、実行exeが専用targetであることを確認した。
- `cargo fmt --all -- --check`、`git diff --check`、Python構文、追加文書のローカルリンク・code fence、最終source hashと測定binaryの対応: 成功。

[検証記録](rt5_builder_adoption_2026_10_13_validation.json)に最終source hash、終了コード、標準テストのsummaryとCargo artifactを保存した。ログは`/tmp/surtr-rt5-adoption-{red,red-rechecked,focused,ci,stdlib}.log`。

受入条件と最終差分を自己点検し、保存枝の独立性、完成途中のcursor、予算0/1/完了境界、取消、Generatorのrest、mapper / step / predicateの一度だけの実行、深いsegment鎖のDrop / Debugを確認した。先行レポートのAstraレビューは元の製品sourceと原本に対するもので、今回の組合せを別エージェントへレビュー依頼したものではない。

## 残件

保存由来の全Valueコピーを除いたが、表indexと変更entryのコピー、共有prefixの完成時clone、解放、通常追加の所有確認は残る。OI-039、runtime全体のfairness、compiler / REPL / 通常batch全体の費用は未解決である。今回、以前のメモリ計数系列や短い出力の前後性能比較は再実行していない。
