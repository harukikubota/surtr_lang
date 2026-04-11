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

## Next candidates

優先度が高い候補:

- `String::len(value) -> Int`
- `String::starts_with(value, prefix) -> Boolean`
- `String::ends_with(value, suffix) -> Boolean`
- `String::strip_prefix(value, prefix) -> Result<String, NoneError>`
- `String::strip_suffix(value, suffix) -> Result<String, NoneError>`
- `String::uncons(value) -> Result<StringUncons, NoneError>` または MatchBlock 用 builtin Extractor
- `String::split_once(value, separator) -> Result<StringSplit, NoneError>`
- `String::trim(value) -> String`
- `String::trim_start(value) -> String`
- `String::trim_end(value) -> String`

あると便利な候補:

- `String::contains(value, needle) -> Boolean`
- `String::replace(value, from, to) -> String`
- `String::lines(value) -> List<String>`
- `String::chars(value) -> List<String>`
- `String::from_chars(chars: List<String>) -> Result<String, InvalidCharList>`

設計メモ:

- `String` は UTF-8 を前提にするため、`len` は byte 長ではなく surface 上の文字数として扱うのが自然です。
- parser / combinator 用途では `uncons`, `starts_with`, `strip_prefix`, `split_once` があると組み立てやすくなります。
- `Char` を独立型にしないなら、1 文字は長さ 1 の `String` として扱う方針でも十分実用的です。
