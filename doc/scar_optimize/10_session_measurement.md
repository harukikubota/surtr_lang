# process warm / incremental の専用計測

2026-10-16。入力仕様は [03_compile_time.md](03_compile_time.md) の測定シナリオ P / I。
P1〜P4の製品変更を追加せず、公開APIを使う手動harnessで現在の基準値を取得する。level2。

## 対象と受入条件

- P: 同一processで標準snapshotを一度準備し、同じcheckpointから独立した同一suffixを反復する。transactional / in-place APIを分ける。
- I: 同じSigil / Scar sessionへ成功chunkを追加する。過去chunkのgeneric関数参照とtrait / impl追加を含める。途中まで宣言した後の型エラーを拒否し、両phaseのcheckpointへ復元して成功chunkを追加する。transactional APIを維持する。
- parse / resolveを含む外側とScar呼出しの内側を分ける。Resolved clone、session復元、checkpoint、rollbackの時間と、TypedProgram metadata生成を内側に含むかを記録する。
- 深さと宣言数の系列を混同せず、少なくとも三規模を測る。各条件10反復、全走行値・中央値・最小最大・入力hash・source差分・binary hash・toolchain・CPU・電源条件を保存する。
- 不正な計測出力、期待外の拒否、復元失敗は測定失敗とし、完成した集計を出さない。

## 実施順序

1. 独立suffix反復、過去bindingの保持、拒否後の両phase復元をharnessのテストで固定する。
2. `crates/rune/examples/scar-session-profile.rs` に計測入口、`tests/profile/scar_compile/measure_session.py` に収集・検証・集計を実装する。公開compiler API・checkpoint形式・schema / VM versionは変更しない。
3. 局所テスト後、release exampleを構築し、build時のsourceとbinaryのhashを保存して計測する。ビルド・テストを計測と同時実行しない。
4. 全体CI profileと標準Surtrテストを実行し、独立レビュー後に計画と記録を更新する。

## 計測の境界

本harnessはfrontendだけを扱う。Forge、VM、encode、artifact writeを実行しないため、外側の時間をCLIの `compile_total` と呼ばない。標準snapshotの初期化はP / Iの試行時間から除外する。

transactional / in-placeの差は呼出し契約も異なる二つの経路の観測であり、状態clone単独の費用とは断定しない。増分sessionのtransactional cloneを削除する実装、診断用child probeの最適化、永続summaryは今回の対象外。03で保留した設計候補を完了条件に追加しない。

## 反復比較で見つかった計測側の問題

最初のrelease計測は `patterns` 入力の3process目で停止した。型検査は成功したが、TypedProgram全体のJSON hashが反復間で異なった。raw JSONを比較すると、差はSafeBind / doの生成bindingにある6箇所の `unique_id` だけだった。これらは既存処理系のprocess共通counterからfreshに割り当てられ、同じcheckpointからの再開でも値が進む。

signatureは生成binding UIDだけを一対一に置き換えるalpha正規化を行う。同じbindingへの参照と別bindingの区別を保ち、関数index・dispatch・型・名前・literal・span・metadataは保持する。製品のID割当は変更しない。Pattern実入力の反復と、参照関係・literal・dispatchの変化を区別するテストで計測側を検証する。

この不完全な計測は基準値へ含めない。[停止した測定の条件と走行値](evidence/session/initial-incomplete-measurement.json.gz)、[回帰テストの失敗](evidence/session/pattern-signature-red.log)を保存する。先行したharness実装時の入力生成では、宣言と式の配置・REPLで許されない宣言にも失敗した。入力生成とparse contextを修正し、compilerの規則は変えていない。

## 現在の基準値

P4適用後の `d2717bd8` に本harnessを加え、同じrelease binaryで収集した。Apple M2、AC電源。対象のビルド・テストは測定中に停止した。OS page cache・CPU周波数・他アプリ負荷は固定していない。

27processが終了コード0で完了した。setup 27行とは別に、Pは12入力・2 API・各10反復の240sample、Iは16 / 32 / 64のgeneric追加chunkにtrait/impl・拒否・復帰を加えた各19 / 35 / 67chunk・10反復の1,210sampleを収集した。合計1,450sampleで、成功1,420件、意図した拒否30件。全30回で復元後に同じ失敗binding名を異なる型で定義し直す成功を確認した。

入力hash・bytes・signature・診断は各条件の全反復で一致した。Pの12入力すべてで、transactional / in-place間のsignatureも一致した。測定開始・終了時の全source・標準定義・入力・binaryのhashは一致した。

Pの代表入力の中央値。単位はms。復元はScar sessionの生成・checkpoint clone・rollbackを含み、Scar API呼出しとは別計時。

| 入力 | API | Scar復元 | Scar呼出し | frontend total |
|---|---|---:|---:|---:|
| stdlib prewarm | transactional | 0.548 | 1.334 | 3.595 |
| stdlib prewarm | in-place | 0.554 | 0.501 | 2.832 |
| patterns | transactional | 0.560 | 2.592 | 6.313 |
| patterns | in-place | 0.556 | 1.822 | 5.552 |
| generic chain 64 | transactional | 0.576 | 6.243 | 14.335 |
| generic chain 64 | in-place | 0.589 | 5.524 | 13.694 |
| Error 256宣言 | transactional | 0.604 | 25.738 | 31.277 |
| Error 256宣言 | in-place | 0.573 | 25.023 | 30.451 |

Iの各条件10反復の中央値。単位はms。checkpoint / rollbackはScarだけの値で、Sigilは別列に示す。

| generic追加数 | 最後のgeneric chunkのScar | 同checkpoint | 拒否chunkのScar | 拒否後Scar rollback | 拒否後Sigil rollback | 復帰chunkのScar |
|---:|---:|---:|---:|---:|---:|---:|
| 16 | 1.323 | 0.515 | 0.867 | 0.311 | 0.100 | 1.334 |
| 32 | 1.334 | 0.518 | 0.869 | 0.313 | 0.101 | 1.347 |
| 64 | 1.374 | 0.528 | 0.890 | 0.316 | 0.102 | 1.397 |

Iの保持宣言数は、generic追加後に16 / 32 / 64、record・trait・impl追加後に19 / 35 / 67、拒否後も同数、復帰後に20 / 36 / 68となった。内部specializationの数とは区別する。

これらは現在の基準値であり、変更前後の比較ではない。二つのAPIの差を状態clone単独の費用とみなさず、transactional APIをin-placeへ置き換える根拠にしない。Iは小さなgeneric forwardingの系列で、任意の増分プログラムの費用を代表するものではない。

全145条件の中央値・最小最大・件数は [summary.json](evidence/session/summary.json)、各sampleは [runs.jsonl.gz](evidence/session/runs.jsonl.gz)、全27processの生stdout / stderrと引数・終了コードは [process-outputs.jsonl.gz](evidence/session/process-outputs.jsonl.gz) に保存した。個別stdout / stderr名は同archive内の各recordに対応する。
測定順・環境・入力hashは [manifest.json](evidence/session/manifest.json)、ビルド元の全source hash・toolchain・binary hashは [build-manifest.json](evidence/session/build-manifest.json)、実装差分は [build-source.diff](evidence/session/build-source.diff) を参照。cache payloadは一時測定先に置き、repositoryへ含めない。

## 最終検証

| コマンド・対象 | 結果 |
|---|---|
| `cargo test -p rune --example scar-session-profile` | 5件成功、終了コード0 |
| `python3 -m unittest discover -s tests/profile/scar_compile -p test_measure_session.py -v` | 20件成功、終了コード0 |
| `rtk cargo nextest run --profile ci --workspace` | 2,643件成功、70 binaries、終了コード0 |
| `rtk proxy cargo run -p rune --bin surtr -- test --quiet --all` | 終了コード0。quiet出力に件数はない |
| 通常debug example: 増分16 / 32 / 64 chunk、warm generic chain 64 | 4process成功。122sampleは型検査成功119件・意図した拒否3件で、復帰3件を含む。8176 KiB stack、`RUST_MIN_STACK`未指定 |
| 変更Rustの `rustfmt --check`、`git diff --check`、文書の相対リンク | 成功 |

コマンド・終了コードと未実施範囲は [validation.json](evidence/session/validation.json)、状態分離とsignatureのテストは [harness-tests.log](evidence/session/harness-tests.log)、収集側は [collector-tests.log](evidence/session/collector-tests.log)、全体検証は [workspace-ci.log](evidence/session/workspace-ci.log) と [stdlib-tests.log](evidence/session/stdlib-tests.log)、通常stackの記録は [debug-validation.json](evidence/session/debug-validation.json) に保存した。

独立レビューはP / Iの状態分離・prefix metadata・計時境界・生成bindingのalpha正規化を確認した。収集側のmetadata検証不足を修正後、全27processの生出力を再検証し、1,450sample・全145条件の全timingの中央値/最小最大・source483件・入力12件・binary hashを再計算して一致を確認した。最初の未完了測定は正式な集計から除外した。[review.json](evidence/session/review.json) に結果を記録する。

P1〜P4とC / D / P / Iの計測は実施済み。公開API・言語仕様・checkpoint形式・schema / VM versionは変更していない。P / Iの変更前後比較、VM実行、REPL入力構文・CLI全体の計測は今回行っていない。transactional cloneの削減・診断用child probe・永続summaryは保留する設計候補で、今回の完了に含めない。
