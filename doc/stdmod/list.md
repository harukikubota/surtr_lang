# List module

`List` は逐次処理を中心にした標準モジュールです。
今回の拡張では、既存の fold/map 系に加えて集約 helper、prefix/suffix helper、比較ベースの sort helper を追加します。

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
- `List::count(values, pred) -> Int`
- `List::sum(values: List<Int>) -> Int`
- `List::max(values: List<$A>) -> Result<$A, NoneError>` (`$A: Compare`)
- `List::min(values: List<$A>) -> Result<$A, NoneError>` (`$A: Compare`)
- `List::max_by(values, cmp) -> Result<$A, NoneError>`
- `List::min_by(values, cmp) -> Result<$A, NoneError>`
- `List::sort(values: List<$A>) -> List<$A>` (`$A: Compare`)
- `List::sort_by(values, cmp) -> List<$A>`
- `List::take(values, count) -> List<$A>`
- `List::drop(values, count) -> List<$A>`
- `List::take_while(values, pred) -> List<$A>`
- `List::drop_while(values, pred) -> List<$A>`
- `List::span(values, pred) -> (List<$A>, List<$A>)`
- `List::partition(values, pred) -> (List<$A>, List<$A>)`
- `List::dedup(values) -> List<$A>`
- `List::group_count(values: List<$A>) -> List<($A, Int)>` (`$A: Eq`)
- `List::zip(left: List<$A>, right: List<$B>) -> List<($A, $B)>`

## Error contract

- `List::first([])` / `List::last([])` は `Err(NoneError)` を返します。
- `List::at(values, index)` は負 index と範囲外 access で `Err(IndexOutOfBounds(...))` を返します。
- `List::at` のメッセージは `index #{index} out of bounds for len #{List::len(values)}` に固定します。
- `List::sum([])` は `0` を返します。
- `List::max([])` / `List::min([])` は `Err(NoneError)` を返します。
- `List::max_by([])` / `List::min_by([])` は `Err(NoneError)` を返します。
- `List::take(values, count)` は `count <= 0` で `[]` を返します。
- `List::drop(values, count)` は `count <= 0` で入力をそのまま返します。
- `List::take_while(values, pred)` は最長 prefix を返します。
- `List::drop_while(values, pred)` は最長 prefix を落とした suffix を返します。
- `List::span(values, pred)` は `(take_while(values, pred), drop_while(values, pred))` を返します。
- `List::partition(values, pred)` は `(matched, rest)` を返し、両側とも元の順序を保ちます。
- `List::dedup(values)` は最初の出現順を保ちながら重複を除きます。
- `List::group_count(values)` は最初の出現順を保ったまま `(value, count)` を並べます。
- `List::zip(left, right)` は短い方の list 長で打ち切ります。

## Examples

```surtr
print(to_string(List::append([1, 2], [3, 4])))
print(to_string(List::concat([[1], [2, 3], []])))
print(to_string(List::flat_map([1, 2], {|n| [n, n + 10]})))
print(to_string(List::last([1, 2, 3])))
print(to_string(List::at([10, 20, 30], 1)))
print(to_string(List::max([1, 5, 3, 2])))
print(to_string(List::min([1, 5, 3, 2])))
print(to_string(List::max([1.5, 3.25, 2.0])))
print(to_string(List::min([1.5, 3.25, 2.0])))
print(to_string(List::max_by(["a", "abcd", "xy"], {|left, right| Compare::compare(String::len(left), String::len(right)) })))
print(to_string(List::min_by(["a", "abcd", "xy"], {|left, right| Compare::compare(String::len(left), String::len(right)) })))
print(to_string(List::sort([3, 1, 2, 1])))
print(to_string(List::sort([3.25, 1.5, 2.0, 1.5])))
print(to_string(List::sort_by(["a", "abcd", "xy"], {|left, right| Compare::compare(String::len(left), String::len(right)) })))
print(to_string(List::take_while([2, 4, 6, 7, 8], {|n| Int::is_even(n) })))
print(to_string(List::drop_while([2, 4, 6, 7, 8], {|n| Int::is_even(n) })))
print(to_string(List::span([2, 4, 6, 7, 8], {|n| Int::is_even(n) })))
print(to_string(List::partition([1, 2, 3, 4], {|n| Int::is_even(n) })))
print(to_string(List::dedup(["a", "b", "a", "c", "b", "a"])))
print(to_string(List::group_count(["a", "b", "a", "c", "b", "a"])))
print(to_string(List::zip([1, 2, 3], ["x", "y"])))
```

## Notes

- 推奨される構築スタイルは引き続き `List::cons + List::reverse` です。
- `sort_by` は引き続き任意 comparator を受け取り、`max` / `min` / `sort` は `Compare` 実装型に使えます。
- `sum` は今回も `Int` 専用のままで、`group_count` / `dedup` の明示 bound surface はまだ未整理です。
- `append` / `concat` / `at` / `partition` / `zip` を追加しても、`List` の中心用途は先頭からの逐次処理のままです。

## Next candidates

`List` は逐次処理中心の方針を保ちつつ、探索・集約・比較で頻出する helper は追加候補に入れてよいです。

優先度が高い候補:

- `List::enumerate(values) -> List<(Int, $A)>`

牌計算や parser 以外でも便利な候補:

- `List::enumerate_with(values, start) -> List<(Int, $A)>`

設計メモ:

- `List::reduce` / `reduce_while` / `flat_map` がすでにあるため、多くの helper は pure Surtr で記述できます。
- `group_count` は `Eq` 制約付きの pure Surtr helper として実装でき、重複集計の最小 surface を提供します。
- `zip` / `group_count` は VM builtin で tuple list を直接返し、userland では頻出 pair 集約だけを手短に使えるようにします。
