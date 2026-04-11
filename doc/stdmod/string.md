# String module

`String` は文字列の基本 helper と、小さな builder 系関数をまとめる標準モジュールです。

## Exported functions

- `String::is_empty(value) -> Boolean`
- `String::non_empty(value) -> Boolean`
- `String::surround(value, prefix, suffix) -> String`
- `String::join(parts, separator) -> String`
- `String::repeat(value, count) -> Result<String, NegativeRepeatCount>`

## Error contract

- `NegativeRepeatCount(count: Int)`
  - `String::repeat` で `count < 0` のときに返します。
  - 表示メッセージは `repeat count must be non-negative: #{count}` です。

## Examples

```surtr
print(String::join(["a", "b", "c"], ","))
print(String::surround("surtr", "[", "]"))
print(to_string(String::repeat("na", 2)))
print(to_string(String::repeat("na", -1)))
```

## Notes

- `join` と `repeat` は pure Surtr の再帰で実装します。
- `repeat` は trap ではなく `Result` で失敗を返し、関数型らしく合成しやすい surface を優先します。
