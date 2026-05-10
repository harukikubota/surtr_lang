# Surtr Site Docs

`docs/site/` は利用者向けドキュメントです。

REPL でそのまま試しやすい題材を優先しており、実行例は `surtr repl` で確認した形に寄せています。  
一方で、`defstruct` / `defenum` / `impl` / `defextractor` のような file-oriented 宣言は REPL top-level へそのまま置けないため、宣言例は通常の `surtr` コードブロックで示します。
REPL は起動時に標準定義ソースと preload を読み切る OnceRead universe で動くので、その後の `include` や trait universe 増分更新を前提にした説明は避けています。`surtr repl --script file.srt` の preload は、script を一度実行した結果を引き継いだ状態から始まります。

## 入口

- [標準定義ソース](./standard-modules.md)
- [各種定義と使い方](./definitions-and-usage.md)
- [型注釈](./type-annotations.md)
- [トレイト実装](./trait-impls.md)
- [構造体](./structs.md)
- [Facet](./facet.md)
- [Kernel](./kernel.md)
- [File I/O](./file-io.md)
- [Regex](./regex.md)
- [パターンマッチ](./pattern-matching.md)
- [関数コールと関数値](./callables.md)
- [キャプチャ演算子 `&`](./capture-operator.md)
- [パイプ演算子](./pipe-operators.md)
- [関数演算子](./function-operators.md)
- [エラーハンドリング](./error-handling.md)
- [Extractor](./extractors.md)
- [Agents](./agents.md)
- [言語機能 (`import`, `include`, `@autoimport`)](./language-features.md)

## 補助ページ

- [言語ガイド](./language-guide.md)
- [言語リファレンス](./language-reference.md)
- [標準ライブラリ全体ガイド](./standard-library.md)

## 正本との関係

- 利用者向けの説明は `docs/site/`
- 標準定義ソース API の一次情報は `../../lib/*.srt` の `@doc`
- 正本仕様は `../../doc/要件定義v9.md`
- 開発者向け仕様の導線は `../dev/README.md`
