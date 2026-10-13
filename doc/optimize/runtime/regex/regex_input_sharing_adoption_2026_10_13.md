# RT-4 Regex入力共有のmainへの反映

作業日: 2026-10-13。入力は [runtime一覧](../README.md) と [入力共有の仕様・先行実装](regex_input_sharing_2026_10_13.md)。変更前はmain `b5229abd`。作業時はmainの作業treeへ反映した未コミット状態だった。その後、ユーザーが `c577b32d` としてコミットした。

## 変更と受入条件

Sindr・Eldrにまたがる内部表現の改善としてlevel3で実施した。`RegexMatchHandle`・`RegexCapturesHandle`の入力全文を不変の`Rc<String>`にし、`find`・`captures`・`find_all`へ渡された所有Stringを移す。`find_all`の一致間とValue cloneで一つのbufferを共有する。従来の全文コピー経路は置き換えた。captureのgroup・name tableは従来どおりcloneする。

`list-runtime-remaining`の未コミット差分から、製品コード3ファイル、Regexテスト、EldrVMのRegex契約だけを照合して取り込んだ。元worktreeは変更していない。既存テストから重複したpointer検査を除き、無一致エラーのpattern・input payload保持も検証した。[測定harness](../../../../tests/profile/regex_input_cost/README.md)をmainへ追加し、[テスト方針](../../../../docs/dev/テスト方針.md)も整合させた。

全文＋spanの内容による等価比較、UTF-8のbyte offset、空一致、無一致、元のList・Valueを破棄した後の寿命を維持する。不正spanは従来どおりRuntimeErrorにする。公開API・builtin ID・VM/schema versionは変更しない。Regexの同期探索・収集の分割はこの改善に含めない。

## 現在のソースでの再測定

変更前は`git archive b5229abd`のソースに同じharnessを置き、変更後はmainの作業treeからrelease buildした。前後は独立したtarget directoryを使う。Cargo JSONのSindr・Eldr・probe manifestと生成executableを保存binaryへ対応させた。[build記録](regex_input_adoption_cost_2026_10_13_builds.json)と[集計metadata](regex_input_adoption_cost_2026_10_13.summary.json)にbinary・source hashとtoolchainを残した。

同一harness・lock・toolchainで、10入力条件を各5回、割当counterあり／なし・変更前／後の4系列で測った。200 fresh process、1,200区間行、240群。順序はsampleごとにAB/BA、ビルド・テストとは重ねていない。時間はcounter無効系列、requested allocationはcounter有効系列の中央値である。

| 入力bytes / 一致数 | VM run requested MiB 前→後 | VM run ms 前→後 | process peak RSS MiB 前→後 |
|---:|---:|---:|---:|
| 1024 / 1024 | 1.281 → 0.281 | 0.291 → 0.183 | 6.562 → 3.562 |
| 2048 / 2048 | 4.547 → 0.547 | 0.694 → 0.286 | 15.984 → 3.938 |
| 4096 / 4096 | 17.081 → 1.081 | 1.963 → 0.481 | 52.953 → 4.750 |

4,096bytes・4,096一致のVM runの論理live増分は16.534 → 0.538 MiB、handle Vec cloneのrequested allocationは16.531 → 0.531 MiBだった。VecやVM等の容器を含む値であり、入力だけの保持量や物理コピーbyte数ではない。process peak RSSは初期化・6区間・区間外処理・JSON出力を含む全期間の高水位で、区間ごとに加算しない。

1一致対照の4,096bytesではrun requestedが25,664 → 21,608bytes、時間は0.210 → 0.213msだった。無一致ではRc容器の確保に40 requested bytesが加わるが、空Listは入力を残さない。一律の高速化は主張しない。通常CLI batch入口は今回再測定していない。先行レポートのbatch値は過去の検証記録として残す。

[生データ](regex_input_adoption_cost_2026_10_13.jsonl)・[集計](regex_input_adoption_cost_2026_10_13.summary.json)を保存した。[別process検証](regex_input_adoption_cost_2026_10_13_verify.json)は24件で、全文・span・UTF-8空一致・VM破棄後の利用を確認した。4,096件の一致は変更前に4,096個の入力buffer、変更後に1個を保持する。無一致は0個。検証系列は性能の集計に混ぜていない。

## 検証

- Red: 共有契約2テストが旧実装の入力buffer pointer不一致で失敗。環境エラーではない。
- `rtk cargo nextest run --target-dir /tmp/surtr-regex-adoption-debug-target -p sindr -p eldr -E 'test(/regex_/) | test(opaque_error_schema_checks_all_builtin_value_kinds)'`: 8 passed、468 skipped、終了コード0。
- `CARGO_TARGET_DIR=/tmp/surtr-regex-adoption-debug-target rtk cargo nextest run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --target-dir /tmp/surtr-regex-adoption-debug-target --profile ci --workspace`: 2,528 passed、69 binaries、失敗・除外0、終了コード0、nextest部分107.980秒。
- `CARGO_TARGET_DIR=/tmp/surtr-regex-adoption-debug-target rtk proxy cargo run --locked --manifest-path /Users/haruca/work/rust/surtr/Cargo.toml --target-dir /tmp/surtr-regex-adoption-debug-target --message-format=json -- test --quiet --all`: 標準Surtr全件成功、終了コード0。Cargo artifactのSindr・Eldr・Rune manifestがすべてmainを指し、実行exeが専用targetであることを確認した。
- workspaceとharnessの`cargo fmt -- --check`、`git diff --check`、Python構文、追加文書のローカルリンク・code fence: 成功。

初回CIは2,229 passed・299 failedで、299件すべてScarのprepared prefix key不一致だった。`--target-dir`だけではsetup script内のCargoにtargetが伝わらず、別target由来の準備cacheとテストbinaryが混在した。`CARGO_TARGET_DIR`を揃えて再実行し、上記の全件成功を確認した。製品コードの回避処理やcache無効化は追加していない。

ログは`/tmp/surtr-regex-adoption-{red,focused,ci,ci-rechecked,stdlib}.log`。初回失敗と再確認後の成功は区別して保存する。[検証記録](regex_input_sharing_adoption_2026_10_13_validation.json)に終了コードと最終製品ソースのhashを残した。

最終差分を受入条件と照合し、所有権移譲・内容比較・span拒否・clone後の寿命・builtin arity検査・schema不変を自己点検した。今回のlevel3作業では別エージェントを使っていない。先行レポートのAstraレビューを今回の最終差分へのレビューとは扱わない。

## 残件

Regex compile・探索・結果収集は同期のまま。短い一致を一つ保持すると元の入力全文を保持する。fairnessと一致部分だけの保持は別の仕様・改善として残る。他のruntime項目の製品変更は取り込んでいない。
