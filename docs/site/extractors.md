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
deferror Rejected { "input rejected" }

defmod Matchers {
  defextractor never(self: Int) -> MatchResult<Int, Error> {
    MatchResult::Err(Rejected)
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

この例では `never(...)` が常に `MatchResult::Err(Rejected)` を返すため、fallback 側に流れます。

Extractor は `MatchResult<$A, Error>` を返します（`MatchResult<$A>` も可）。
`MatchResult::OK(payload)` は子 pattern の照合へ進み、`MatchResult::Err(error)` は
不一致になります。各 occurrence は到達時に一度だけ評価され、成功結果を再利用します。
`match` / `if_let` / `is_match` は Error を破棄します。SafeBind `=?` は元 Error の
kind / message / location / cause を保持して現在の failure target へ渡し、
do の Alternative route は破棄して `empty` へ進みます。

成功 payload が Unit の場合は `check()` と子 Pattern を省略でき、
`check(value: Unit)` や `check(_)` と明示することもできます。
単値は子 Pattern 1個、tuple は要素数と同じ個数が必要です。
Extractor 本文でも SafeBind を使えます。失敗は本文自身の MatchResult::Err となり、
成功終端には明示的な MatchResult::OK が必要です。
MatchResult は通常の変数・引数・field に保持できず、通常 Closure へ利用権限は継承されません。

## ルール

- extractor 名は constructor-style の大文字始まりにしない
- extractor の入力型と pattern 期待型が合わないと type error
- 戻り値の arity と pattern 側の束縛数が合う必要がある

関連する compile error 例は `../../tests/fixtures/modules/fail/resolve_extractor_*` と `../../tests/fixtures/modules/fail/type_mismatch_extractor_*` にあります。

## 関連ページ

- pattern 側の使い方は `./pattern-matching.md`
- `Kernel::uncons` は `./kernel.md`

## 確認したソース

- ソース
  - `../../lib/kernel.srt`

## 躓きやすいポイント

- extractor は普通の `def` ではなく、`MatchResult` を返す pattern-side contract として読む必要があります。
- extractor の入力型と scrutinee 型、成功 payload の arity がずれると分かりにくい type error になりやすいです。
