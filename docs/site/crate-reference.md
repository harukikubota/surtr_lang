# Surtr Crate Reference

このページは、Surtr の各 crate が何を担当しているかを一覧できるようにまとめたものです。

## 1. 全体像

```text
spire -> sigil -> scar -> forge -> eldr
                  ^                    ^
                sindr              diagnostics

rune: CLI entrypoint
xldr: REPL / interactive runtime
```

## 2. crate 一覧

### `spire`

役割:

- lexer
- parser
- parser context / source rules の適用

主な責務:

- source text を AST に変換する
- `Script` / `Module` / `StdModule` / `ReplChunk` ごとの構文許可を反映する

入出力:

- 入力: `&str`
- 出力: `Vec<Ast>` または `ParseError`

設計メモ:

- ここでは名前解決しない
- ここでは型検査しない
- `@@builtin` をどこで許可するかは `SourceRules` で管理する

### `sigil`

役割:

- 名前解決
- import 処理
- auto import
- 宣言インデックス構築

主な責務:

- 識別子を unique id に束縛する
- file 単位の import 重複を検出する
- `Bootstrap` / `Kernel` の auto import を適用する

入出力:

- 入力: `Vec<Ast>`
- 出力: `Vec<Resolved>` または `ResolveError`

設計メモ:

- 「見える名前かどうか」は Sigil の責務
- `if` の専用ノード化のような、後段を単純化する変換も担う

### `scar`

役割:

- 型検査
- `match` 網羅性検査
- field 解決

主な責務:

- `Resolved` を `TypedNode` に変換する
- field 名を field index に落とす
- `Result` の使い方や戻り値型整合を保証する

入出力:

- 入力: `Vec<Resolved>`
- 出力: `Vec<TypedNode>` または `TypeError`

設計メモ:

- `Forge` が field 名を知らなくて済むように、ここで index 化する
- 型推論や型互換の中心は Scar に集める

### `forge`

役割:

- codegen
- bytecode 構築
- function table / type registry / error template 構築

主な責務:

- `TypedNode` から opcode 列を生成する
- runtime が実行しやすい形に落とし込む

入出力:

- 入力: `Vec<TypedNode>`
- 出力: `Bytecode` または `CodegenError`

設計メモ:

- 型名解決や import 解決はここへ持ち込まない
- Eldr が実行専用で済むように整形済み bytecode を作る

### `eldr`

役割:

- 仮想マシン
- bytecode 実行
- builtin dispatch

主な責務:

- stack-based VM を実装する
- `CallBuiltin` を `builtin_id` で実行する
- runtime error を fail-fast で返す

入出力:

- 入力: `Bytecode`
- 出力: 実行結果または `RuntimeError`

設計メモ:

- import 解決はしない
- 名前解決はしない
- 型推論はしない
- 「正しい bytecode をそのまま実行する」ことに集中する

### `sindr`

役割:

- 共有基盤型
- IR
- runtime value
- builtin metadata

主な責務:

- compiler / runtime の両方が使う型を置く
- builtin の canonical な定義を保持する

設計メモ:

- crate 間の接着剤として重要
- builtin の定義順や id の正本はここに置く

### `xldr`

役割:

- REPL
- 対話実行補助
- module source 収集

主な責務:

- 標準モジュールと user source を compile 用に束ねる
- `Bootstrap -> [Kernel + 他標準モジュール] -> ユーザ拡張` のロード順を守る
- REPL セッションの増分 compile / 実行を管理する

設計メモ:

- CLI 本体ではなく、対話実行層
- parser / resolver / checker / VM を再実装せず、既存パイプラインを束ねる

### `rune`

役割:

- CLI entrypoint

主な責務:

- `surtr run`
- `surtr build`
- `surtr dump`
- `surtr repl`

設計メモ:

- ビジネスロジックの本体より、ユーザー入口としての責務を持つ
- compile / execute の orchestration を行う

### `diagnostics`

役割:

- 診断表示の共通化

主な責務:

- 人間向けの診断表示
- source registry と span の橋渡し

設計メモ:

- エラー型そのものより「どう見せるか」を担当する

## 3. crate 間の依存関係

Surtr では、後段 crate が前段の出力型だけに依存する構成を目指しています。

基本方針:

- parser の内部型を他 crate が直接触らない
- resolver の内部状態を typechecker が直接触らない
- runtime が source text を再解釈しない

これにより、各段階の責務が崩れにくくなります。

## 4. 標準モジュールはどこが扱うか

標準モジュールの扱いは複数 crate にまたがりますが、責務は分かれています。

- `xldr`
  - source を集める
  - ロード順を保証する
- `spire`
  - std module 用の構文ルールを適用する
- `sigil`
  - auto import と import 制約を適用する
- `scar`
  - その定義を型検査する
- `forge`
  - bytecode 化する
- `eldr`
  - 実行する

## 5. Surtr で言語実装を学ぶときの見方

もし「自分も言語を作りたい」という目線で読むなら、次の順で追うのがおすすめです。

1. `spire`
2. `sigil`
3. `scar`
4. `forge`
5. `eldr`
6. `xldr` / `rune`

この順で読むと、「構文を読んでから、意味を与え、型を付けて、最終的に実行する」という言語処理系の流れが見えやすくなります。
