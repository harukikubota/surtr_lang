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
