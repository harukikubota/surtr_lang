# Extractors

Extractor は `match` や `=?` で使う「分解の入口」です。

## builtin extractor

標準では `Kernel::uncons(term)` があります。

- `List<$A>` を `(head, tail)` へ分解
- `String` を `(head, tail)` へ分解

pattern の `[head, ..tail]` はこの alias です。

## user-defined extractor

定義は file-oriented です。

```surtr
defmod Matchers {
  defextractor never(self: Int) -> MatchResult<Int, Error> {
    MatchResult::NoMatch
  }
}
```

REPL では top-level に `defextractor` を直接置けないため、宣言は file で管理し、利用側を REPL で確認する形になります。

## match 側の見え方

```surtr
import Matchers::never

print(match 1 {
  never(value) => "bad",
  _ => "fallback",
})
```

この例では `never(...)` が常に `NoMatch` を返すため、fallback 側に流れます。

## ルール

- extractor 名は constructor-style の大文字始まりにしない
- extractor の入力型と pattern 期待型が合わないと type error
- 戻り値の arity と pattern 側の束縛数が合う必要がある

関連する compile error 例は `../../tests/compile_errors/modules/resolve_extractor_*` と `../../tests/compile_errors/modules/type_mismatch_extractor_*` にあります。

## 関連ページ

- pattern 側の使い方は `./pattern-matching.md`
- `Kernel::uncons` は `./kernel.md`

## 確認したソース

- ソース
  - `../../lib/kernel.srt`

## 躓きやすいポイント

- extractor は普通の `def` ではなく、`MatchResult` を返す pattern-side contract として読む必要があります。
- extractor の入力型と scrutinee 型、成功 payload の arity がずれると分かりにくい type error になりやすいです。
