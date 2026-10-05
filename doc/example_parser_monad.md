# ParserMonad サンプルの仕様と実装方針

既存の Surtr 機能だけを使う level1 のサンプル。コンパイラと標準ライブラリは変更しない。
`Parser<A>` は入力状態から成功値と残りの入力、または位置付き失敗を返す関数を保持する。
文字位置は Unicode スカラー値の個数で数え、0から始める。

Functor、Applicative、Monadを実装し、順次処理では残りの入力を次へ渡す。
選択は左を優先し、通常の失敗時には元の入力から右を試す。両方失敗したら
遠い位置の失敗を返す。同位置なら左の診断を使う。
反復は未消費の通常失敗で終了し、消費後の失敗を伝播する。
未消費の成功は反復の不正な構成として致命的失敗にし、選択でも隠さない。

基本APIは pure、fail、item、satisfy、char、literal、eof、parse、parse_all。
コンビネータは fmap、bind、ap、choice、many、some、optional、pair、before、after、
between、sep_by、sep_by1、lazy。入力状態は不変で、左再帰は扱わない。

受入条件はUnicodeの消費、残余、失敗位置、バックトラック、反復の進捗検査、
区切り後の拒否、型の異なるbind、Monad則、do構文をSurtrテストで確認すること。
BNF付き四則演算例で優先順位、左結合、括弧、空白、全入力消費を確認する。

最初に item の契約テストを失敗させ、基本型とMonad、コンビネータ、四則演算例の順に実装する。
検証は対象テストと使用例、続いて `cargo run -- test --quiet --all`。
Rust変更はないためnextestは実行しない。

## 実装結果

`examples/parser_monad/` に実装。通常の `map` / `and_then` に処理を置き、
標準Traitはそれらへ委譲する。ListやOptionをpayloadにする合成にも対応する。
`Functor::fmap` のcontextual mapper制約は現行仕様どおりなので、該当する変換は
通常メソッドを使う。全入力消費は `parse_all` が末尾の結果を明示的に確認する。

使用例は `run.srt` と `entry.srt --entry main` の両方で実行できる。
明示entryの戻り値は現行契約の `Result<()>` とする。
READMEと `src/arithmetic.srt` に同じBNFを掲載した。

検証結果はParser 16件、四則演算6件が全件成功（skipped=0）。
使用例と明示entryも成功。標準テストの `cargo run --quiet -p rune -- test --quiet --all`
は終了コード0。Rust・標準定義の変更はなく、nextestは対象外とした。
