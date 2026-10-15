# 現行機能を含むコンパイル計測入力

2026-10-15。前の調査に対する「`heavy_compile.srt` は機能が少ない段階の入力なので、複数パターンを用意する」という追加依頼に対応した。作業は現行仕様内の計測用 `.srt` と文書の整備（level1）。コンパイラ本体、標準 API、通常の CI gate は変更していない。

## 追加した構成

[tests/profile/scar_compile](../../tests/profile/scar_compile/README.md) に、新規 22 入口と補助 module 2 本を用意した。既存の小入力と `heavy_compile.srt` を含めた [一覧](../../tests/profile/scar_compile/cases.json) は 24 入口。

- 機能別 6 入力: Error constructor / Payload、generic / RTA / capture、Extractor / pattern、Facet、Monad carrier / transformer、closure / lazy。
- 規模別 15 入力: Error 宣言 64 / 128 / 256、generic chain 16 / 32 / 64、同じ chain の逆順宣言、Trait impl 16 / 32 / 64、nominal dependency 16 / 32 / 64。
- 複数 module 1 入口: Trait と generic 関数、所有型と impl、利用側を 3 ファイルに分け、追加 module の stage を通す。

生成系列は [generate.py](../../tests/profile/scar_compile/generate.py) と生成済み `.srt` を保存した。`--check` で一致を確認できる。機能別入力は手書きとし、既存 fixture の現行構文を参照しながら、測る処理の組合せを独立したファイルへまとめた。

`heavy_compile.srt` は具体型中心の対照として残し、`Err(NoneError)` を `Err(NoneError())` に修正した。以前の失敗ログと時間集計はその時点の証拠として保持し、[04_baseline.md](04_baseline.md) の値を修正後入力の値へ置き換えていない。

## 入力設計で確認したこと

Error 系列は宣言数を増やしても constructor 呼出しを 2 箇所に固定した。generic chain は呼出しを Int / String / Int の 3 箇所に固定し、同型の再利用と異型の具体化を通す。逆順系列は body と呼出しを変えず宣言順だけを変え、固定点走査に都合のよい順序だけに偏らないようにした。

nominal 系列の最初の構築案は constructor を深く入れ子にし、32 / 64 の入力が parser の nesting limit で拒否された。これは型依存の測定に適さないため、構築を一段ずつの binding に分けた。制限値やコンパイラを変更せず、型依存の深さを維持した成功入力にした。

Trait impl 系列では型宣言と method も同時に増える。前宣言全体の比較には使えるが、候補探索の時間だけには帰属させない。機能別入力も単一関数の microbenchmark ではない。結果は [性能計画](03_compile_time.md) の phase / profile と合わせて解釈する。

独立レビューで一覧・依存の対応と規模条件を確認し、宣言順の逆転系列と、複数処理が同時に増える場合の説明を追加した。

## 検証

使用したコンパイラは commit `a0d430a609da73f7f35bbba7a01f41f9b28dfc09` の release binary。`.srt` の追加・修正を含む作業ツリーで実行した。入力・依存ソース・生成元・一覧・binary の hash と実行条件は [inputs_manifest.json](evidence/inputs_manifest.json) に保存した。

| 検証 | 結果 |
|---|---|
| 全 24 入口の `build --phase-times-json`、cold / disk hit 各 1 回 | 計 48 回成功。artifact cache は不使用、execute は skipped |
| `python3 tests/profile/scar_compile/generate.py --check` | 15 入力が生成元と一致 |
| `cargo run --release --target-dir /tmp/surtr-scar-investigation-a0d430a6 -p rune --bin surtr -- test --quiet --all` | 終了コード 0。標準テスト全件を確認 |
| 一覧の重複・入口と依存ファイルの存在・未登録ソース、文書リンク、差分 | 確認済み。対象文書の相対リンク 177 件、入力 hash、逆順系列の body / 呼出し一致も確認 |

[48 回の実行記録](evidence/inputs_validation.jsonl) は各引数、終了コード、stdout / stderr、cache 条件、phase JSON を含む。[標準テストのログ](evidence/inputs_stdlib_tests.log) は quiet 実行のため成功件数を表示していない。件数は推定しない。

この 48 回は成功確認であり、before/after 比較でも反復統計でもない。追加入力の VM 実行、出力値の照合、Rust の nextest 全件は実施していない。今回は実行可能な測定入力を揃えた段階で、Scar 自体の速度改善は未実装。

次は候補を一つ選び、対応する機能入力と規模系列について同条件で反復測定する。process-local warm / incremental session の専用 harness は引き続き別作業である。
