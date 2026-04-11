# List module

`List` は逐次処理を中心にした標準モジュールです。
今回の拡張では、既存の fold/map 系に加えて append 系と位置アクセス系を追加します。

## Exported functions

- `List::cons(head, tail) -> List<$A>`
- `List::first(values) -> Result<$A, NoneError>`
- `List::last(values) -> Result<$A, NoneError>`
- `List::at(values, index) -> Result<$A, IndexOutOfBounds>`
- `List::len(values) -> Int`
- `List::append(left, right) -> List<$A>`
- `List::concat(values: List<List<$A>>) -> List<$A>`
- `List::reverse(values) -> List<$A>`
- `List::reduce(values, init, f) -> $B`
- `List::reduce_while(values, init, f) -> $B`
- `List::map(values, f) -> List<$B>`
- `List::flat_map(values, f) -> List<$B>`
- `List::filter(values, pred) -> List<$A>`
- `List::find(values, pred) -> Result<$A, NoneError>`
- `List::find_map(values, f) -> Result<$B, NoneError>`
- `List::any(values, pred) -> Boolean`
- `List::all(values, pred) -> Boolean`

## Error contract

- `List::first([])` / `List::last([])` は `Err(NoneError)` を返します。
- `List::at(values, index)` は負 index と範囲外 access で `Err(IndexOutOfBounds(...))` を返します。
- `List::at` のメッセージは `index #{index} out of bounds for len #{List::len(values)}` に固定します。

## Examples

```surtr
print(to_string(List::append([1, 2], [3, 4])))
print(to_string(List::concat([[1], [2, 3], []])))
print(to_string(List::flat_map([1, 2], {|n| [n, n + 10]})))
print(to_string(List::last([1, 2, 3])))
print(to_string(List::at([10, 20, 30], 1)))
```

## Notes

- 推奨される構築スタイルは引き続き `List::cons + List::reverse` です。
- `append` / `concat` / `at` を追加しても、`List` の中心用途は先頭からの逐次処理のままです。

## Next candidates

`List` は逐次処理中心の方針を保ちつつ、探索・集約・比較で頻出する helper は追加候補に入れてよいです。

優先度が高い候補:

- `List::sum(values: List<Int>) -> Int`
- `List::max(values: List<Int>) -> Result<Int, NoneError>`
- `List::min(values: List<Int>) -> Result<Int, NoneError>`
- `List::max_by(values, cmp) -> Result<$A, NoneError>`
- `List::min_by(values, cmp) -> Result<$A, NoneError>`
- `List::take(values, count) -> List<$A>`
- `List::drop(values, count) -> List<$A>`
- `List::partition(values, pred) -> Pair<List<$A>, List<$A>>` 相当
- `List::count(values, pred) -> Int`
- `List::zip(left: List<$A>, right: List<$B>) -> List<Pair<$A, $B>>` 相当

牌計算や parser 以外でも便利な候補:

- `List::enumerate(values) -> List<Pair<Int, $A>>` 相当
- `List::take_while(values, pred) -> List<$A>`
- `List::drop_while(values, pred) -> List<$A>`
- `List::span(values, pred) -> Pair<List<$A>, List<$A>>` 相当
- `List::group_count(values) -> List<Pair<$A, Int>>` 相当
- `List::dedup(values) -> List<$A>`
- `List::sort(values) -> List<$A>`
- `List::sort_by(values, cmp) -> List<$A>`

設計メモ:

- `List::reduce` / `reduce_while` / `flat_map` がすでにあるため、多くの helper は pure Surtr で記述できます。
- 一方で `sort`, `group_count`, `zip`, `partition` は userland で毎回書くと冗長になりやすく、標準 surface に置く価値があります。
- `Pair` や tuple を一般機能として導入しない方針なら、`Seq` を流用せず、専用 record / enum か複数戻り値相当の別設計を用意した方が境界が明確です。
