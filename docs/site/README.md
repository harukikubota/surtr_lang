# Surtr Site Docs

`docs/site/` は利用者向けドキュメントです。

REPL でそのまま試しやすい題材を優先しており、実行例は `surtr repl` で確認した形に寄せています。  
一方で、`defstruct` / `defenum` / `impl` / `defextractor` のような file-oriented 宣言は REPL top-level へそのまま置けないため、宣言例は通常の `surtr` コードブロックで示します。
REPL は起動時に標準定義ソースと preload を読み切る OnceRead universe で動くので、その後の `include` や trait universe 増分更新を前提にした説明は避けています。`surtr repl --script file.srt` の preload は、script を一度実行した結果を引き継いだ状態から始まります。

## 入口

- [標準定義ソース](./standard-modules.md)
- [各種定義と使い方](./definitions-and-usage.md)
- [型注釈](./type-annotations.md)
- [Special Types](./special-types.md)
- [Trait システム](./trait-system.md)
- [トレイト実装](./trait-impls.md)
- [Convert / TryConvert](./traits/convert.md)
- [`@derive`](./derive.md)
- [構造体](./structs.md)
- [Record](./record.md)
- [HashMap](./hash_map.md)
- [Identity](./identity.md)
- [Reader](./reader.md)
- [State](./state.md)
- [Monad transformers](./monad-transformers.md)
- [doによるMonadの逐次処理](./do.md)
- [Range](./range.md)
- [Generator](./generator.md)
- [Float](./float.md)
- [Facet](./facet.md)
- [文字列の入力と表示](./strings.md)
- [Kernel](./kernel.md)
- [Lazy evaluation と括弧](./lazy-evaluation.md)
- [JSON](./json.md)
- [File I/O](./file-io.md)
- [FS and Shell](./shell.md)
- [Regex](./regex.md)
- [パターンマッチ](./pattern-matching.md)
- [関数名と呼出し構文](./callable-names.md): 関数の名前の指定方法と呼出し方。
- [関数コールと関数値](./callables.md)
- [キャプチャ演算子 `&`](./capture-operator.md)
- [パイプ演算子](./pipe-operators.md)
- [関数演算子](./function-operators.md)
- [エラーハンドリング](./error-handling.md)
- [テストを書く](./test.md)
- [Extractor](./extractors.md)
- [Process](./process.md)
- [Compiler Warnings](./warnings.md)
- [言語機能 (`import`, `include`, `@autoimport`)](./language-features.md)

## 補助ページ

- [言語ガイド](./language-guide.md)
- [言語リファレンス](./language-reference.md)
- [標準ライブラリ全体ガイド](./standard-library.md)

## 正本との関係

- 利用者向けの説明は `docs/site/`
- 標準定義ソース API の一次情報は `../../lib/**/*.srt` の `@doc`
- 現行挙動は Rust / Surtr のソースコードと実行可能テストを優先する
- compact な言語 surface は `./language-reference.md`
- 開発者向け仕様の導線は `../dev/README.md`
