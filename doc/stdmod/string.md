# String module

`String` は文字列の基本 helper と、小さな builder 系関数をまとめる標準モジュールです。

## Exported functions

- `String::is_empty(value) -> Boolean`
- `String::non_empty(value) -> Boolean`
- `String::len(value) -> Int`
- `String::surround(value, prefix, suffix) -> String`
- `String::join(parts, separator) -> String`
- `String::repeat(value, count) -> Result<String, NegativeRepeatCount>`
- `String::starts_with(value, prefix) -> Boolean`
- `String::ends_with(value, suffix) -> Boolean`
- `String::strip_prefix(value, prefix) -> Result<String, NoneError>`
- `String::strip_suffix(value, suffix) -> Result<String, NoneError>`
- `String::split_once(value, separator) -> Result<StringSplit, NoneError>`
- `String::contains(value, needle) -> Boolean`
- `String::replace(value, from, to) -> String`
- `String::split(value, separator) -> List<String>`
- `String::lines(value) -> List<String>`
- `String::chars(value) -> List<String>`
- `String::from_chars(chars) -> Result<String, InvalidCharList>`
- `String::codepoints(value, encoding) -> Result<List<Int>, InvalidStringEncoding>`
- `String::from_codepoints(values, encoding) -> Result<String, InvalidStringEncoding>`
- `String::trim_start(value) -> String`
- `String::trim_end(value) -> String`
- `String::trim(value) -> String`

## Exported enums

- `StringSplit::Split(String, String)`
- `StringEncoding::Utf8`
- `StringEncoding::Ascii`

## Error contract

- `NegativeRepeatCount(count: Int)`
  - `String::repeat` で `count < 0` のときに返します。
  - 表示メッセージは `repeat count must be non-negative: #{count}` です。
- `InvalidCharList(detail: String)`
  - `String::from_chars` で 1 文字以外の `String` 要素を受け取ったときに返します。
  - 表示メッセージは `detail` をそのまま使います。
- `InvalidStringEncoding(detail: String)`
  - `String::codepoints` / `String::from_codepoints` で encoding に合わない文字や数値列を受け取ったときに返します。
  - 表示メッセージは `detail` をそのまま使います。
- `NoneError`
  - `String::strip_prefix`, `String::strip_suffix`, `String::split_once` が失敗したときに返します。

## Examples

```surtr
print(String::join(["a", "b", "c"], ","))
print(String::surround("surtr", "[", "]"))
print(to_string(String::repeat("na", 2)))
print(to_string(String::repeat("na", -1)))
print(to_string(String::strip_prefix("surtr", "sur")))
print(to_string(String::split_once("key=value", "=")))
print(String::replace("banana", "na", "NA"))
print(to_string(String::split("a,,b,", ",")))
print(to_string(String::split("surtr", "")))
print(to_string(String::lines("a\r\nb\r\n")))
print(to_string(String::from_chars(["あ", "b"])))
print(to_string(String::codepoints("Aあ", StringEncoding::Utf8)))
print(to_string(String::from_codepoints([65, 227, 129, 130], StringEncoding::Utf8)))
print(to_string(String::codepoints("AZ", StringEncoding::Ascii)))
print(String::trim(" \ncore\t "))
```

## Notes

- `join` と `repeat` は pure Surtr の再帰で実装します。
- `repeat` は trap ではなく `Result` で失敗を返し、関数型らしく合成しやすい surface を優先します。
- `split_once("", value)` ではなく `split_once(value, "")` を呼んだ場合、先頭で一致したものとして `Ok(StringSplit::Split("", value))` を返します。
- `replace(value, "", to)` は `value` をそのまま返します。
- `split(value, "")` は `chars(value)` と同じ結果を返します。
- `split` は空要素を保持します。たとえば `split("a,,b,", ",")` は `["a", "", "b", ""]` です。
- `lines` は `\n` で分割し、各行末の `\r` を取り除きます。末尾改行だけで生じる最後の空行は落とします。
- `codepoints` / `from_codepoints` の `Utf8` は Unicode scalar value ではなく UTF-8 byte 列を扱います。
- `Ascii` は 7-bit ASCII だけを受け付け、範囲外は `Err(InvalidStringEncoding(...))` で返します。
- `trim_start` / `trim_end` / `trim` は現状 space / newline / tab / carriage return の ASCII whitespace のみを対象にします。
- MatchBlock では `Kernel::uncons(term)` または `[head, ..tail]` を使って `String` を分解します。通常関数としての `String::uncons` は置きません。

## Next candidates

あると便利な候補:

- `String::pad_start(value, width, fill) -> Result<String, InvalidCharList>`
- `String::pad_end(value, width, fill) -> Result<String, InvalidCharList>`

設計メモ:

- `String` は UTF-8 を前提にするため、`len` は byte 長ではなく surface 上の文字数として扱うのが自然です。
- parser / combinator 用途では `uncons`, `starts_with`, `strip_prefix`, `split_once` があると組み立てやすくなります。
- `Char` を独立型にしないなら、1 文字は長さ 1 の `String` として扱う方針でも十分実用的です。
