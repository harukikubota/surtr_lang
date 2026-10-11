# ParserMonad サンプルの仕様と実装方針

既存の Surtr 機能だけを使う level1 のサンプル。
再設計の入力仕様は [StateT と Result の比較・採用方針](parser_monad_state_t_comparison.md)。
コンパイラと標準ライブラリは変更しない。

## 構成と API

`Parser<A>` は `StateT<ParseInput, Result, A>` を保持する独立した型とする。
成功は `Ok((value, next_input))`、失敗は元 Error を保持した `Err(error)`。
`parse` は残余入力を含む Result、`parse_all` は全入力消費を要求した `Result<A>` を返す。
文字位置は0始まりの Unicode スカラー位置で、バイト位置や書記素位置ではない。

入力検査は private `_checked_transition` に集約する。
負の入口 offset は実行前に拒否する。成功時は rest が入口入力の suffix であり、
offset が消費したスカラー数だけ進んでいることを要求する。
run / bind / ap / recover は検査付き遷移を取り出し、中間の不正状態を次へ渡さない。
StateT の保存 helper は検査 wrapper を追加せず、保存だけを行う。

Functor、Applicative、Monad、MonadFail、MonadRecover を実装する。
pure / bind / ap / fail は標準 StateT に委譲する。
通常の map は and_then と pure の合成で List / Option / Tuple を成功値にできる。
Functor の contextual mapper 制約は現行仕様のままとする。
recover は Result の回復に接続し、元 Error を受けたハンドラの Parser を境界入口から一度実行する。
外側の recover は全体の入口、bind 内の recover は後続 Parser の入口へ戻る。
recover_kind の kind 判定は標準 Trait のデフォルト実装を継承する。

構文不一致は `ParseMismatch(offset, expected)`。
構成・反復進捗・状態契約の違反はそれぞれ `ParserConfigurationError`、
`ParserProgressError`、`ParserStateError` とする。
独自の ParseReply / ParseIssue / fatal と ExampleSyntaxError の変換は廃止する。

## 構文コンビネータ

choice は範囲内の ParseMismatch だけを入口入力へ戻して回復する。
両方の不一致は遠い位置の元 Error を返し、同位置なら左を返す。
optional も消費後の構文不一致を入口へ戻すが、many は試行入口位置の不一致だけで正常終了する。
消費後の不一致、範囲外の不一致、他の Error は元値を伝播する。
未消費の反復成功は ParserProgressError とし、空列にしない。
sep_by は入口不一致なら空列を返し、区切りの後の要素欠落は拒否する。

基本 API は new、run、parse、parse_all、pure、reject、fail、recover、item、satisfy、char、literal、eof。
合成は map、and_then、ap、choice、many、some、optional、pair、before、after、between、sep_by、sep_by1、lazy。
構文選択に標準 Alternative を使わず、Parser に Alternative を追加しない。
入力は不変だが、外部副作用は回復で巻き戻らない。左再帰は対象外。

## 四則演算と受入条件

BNF、優先順位、左結合、括弧、ASCII 空白、BigInt 評価を維持する。
README と `src/arithmetic.srt` の BNF を同じ記述に揃える。
parse_all の成功値を SafeBind で受けて evaluate へ接続する。
数字列の変換失敗と評価失敗を、別の構文 Error に作り直さない。
使用例は `run.srt` と `entry.srt --entry main` を残す。明示 entry の戻り値は `Result<()>`。

対象テストは成功値と残余入力、失敗の具象 Pattern による Payload 観測、
Error の message / format / cause 保持、回復境界、選択の評価順、反復進捗、
範囲外診断の伝播、owner / Facet で保存した不正遷移、Monad 則を検証する。
Result の Err の Eq だけで Payload の等価性を判断しない。

## 2026-10-11 の実装結果

main の `ee13e448` を取り込み、StateT の generic 呼出しの型解決修正後に実装を完成させた。
Parser 本体、対象テスト、四則演算例、README は新契約へ移行し、旧 API の実行経路は削除した。
停止時の最小再現と取り込みの詳細は、入力仕様の第13節に記録している。

- Parser テスト38件と四則演算テスト7件はすべて成功。
- `run.srt` と `entry.srt --entry main` は終了コード0。
- CLI の `--vm-dump` で、fail / recover / kind 不一致の recover_kind / choice を通った元 Error の生成位置と cause の保持を6経路で確認。

保存経路のテストは `Parser::new → _store_transition` の owner struct literal と Facet 差替えを通る。
テストから別ステージの owner impl を追加する方法は使わない。
`cargo run --quiet -p rune -- test --quiet --all` は終了コード0。
サブエージェントによるコード・テストの独立レビューで指摘はなかった。
このサンプルの差分に Rust / 標準定義の修正はなく、level1 のため nextest は実行していない。
