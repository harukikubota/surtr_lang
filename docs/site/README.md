# Surtr Documentation

このディレクトリは、将来的に公開することを前提に整理した Markdown ドキュメント置き場です。

現時点では、実装済み・要件定義で確定済みの範囲だけを対象にしています。未確定事項や将来拡張は、必要に応じて本編から切り離して記述します。

## 読者別ガイド

### Surtr を使いたい人

- [言語ガイド](./language-guide.md)
  - Surtr の考え方
  - 基本文法
  - 関数、構造体、レコード、Enum、Result、List
  - apply / compose / SafeBind の使い分け
- [言語リファレンス](./language-reference.md)
  - 構文の一覧
  - 型と組込み関数の一覧
  - pipeline / bind 演算子の外部契約
  - 現時点の制約
- [標準ライブラリガイド](./standard-library.md)
  - 標準モジュールのロード順
  - `Kernel` と各 type file の役割
  - `@@doc` を source に載せる理由
  - `cons`, `first`, `len`, `List::map`, `List::find_map` と pipeline の対応

### プログラミング言語を作りたい人

- [コンパイラ設計ガイド](./compiler-design.md)
  - パイプライン設計
  - 標準モジュールのロード契約
  - Source / AST / Typed / Bytecode の境界
- [クレート設計リファレンス](./crate-reference.md)
  - 各 crate の責務
  - 入出力の型境界
  - crate 間の依存関係
- [実装レシピ](./implementation-recipes.md)
  - 命令追加の手順
  - builtin 追加の手順
  - コピペ用テンプレート

## 現在の前提

- 正本は [要件定義v9](../要件定義v9.md)
- VM の詳細仕様は [EldrVM_spec](../EldrVM_spec.md)
- REPL の詳細仕様は [Xldr_spec](../Xldr_spec.md)
- 未確定事項は [open-issues](../open-issues.md)

## ドキュメント方針

- 学習向けドキュメントは「まず書ける」ことを優先する
- 設計向けドキュメントは「どう実装を分けているか」を優先する
- Rust 実装の細部より、公開時に説明すべき外部契約を優先する
- 標準モジュール API の第一説明は `lib/*.srt` の `@@doc` に置き、`site/` はその読み方と背景を補う
