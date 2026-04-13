# Regex module

`Regex` は Rust `regex` crate をラップした標準モジュールです。
パターン compile、マッチ判定、キャプチャ取得、置換、分割を提供します。

## Generated literal

- `re"pattern"`
- `re'pattern'`

上記は `Regex::compile("pattern")` に lower されます。

## Exported functions

- `Regex::compile(pattern: String) -> Result<Regex, RegexCompileError>`
- `Regex::is_match(re: Regex, input: String) -> Boolean`
- `Regex::captures(re: Regex, input: String) -> Result<RegexCaptures, NoneError>`
- `Regex::find(re: Regex, input: String) -> Result<RegexMatch, NoneError>`
- `Regex::find_all(re: Regex, input: String) -> List<RegexMatch>`
- `Regex::split(re: Regex, input: String) -> List<String>`
- `Regex::replace(re: Regex, input: String, replacement: String) -> String`
- `Regex::replace_all(re: Regex, input: String, replacement: String) -> String`
- `Regex::escape(text: String) -> String`
- `Regex::group_names(re: Regex) -> List<String>`

## Captures / Match accessors

- `RegexCaptures::whole(caps: RegexCaptures) -> String`
- `RegexCaptures::capture_count(caps: RegexCaptures) -> Int`
- `RegexCaptures::get(caps: RegexCaptures, idx: Int) -> Result<String, NoneError>`
- `RegexCaptures::get_name(caps: RegexCaptures, name: String) -> Result<String, NoneError>`
- `RegexMatch::text(m: RegexMatch) -> String`
- `RegexMatch::start(m: RegexMatch) -> Int`
- `RegexMatch::end(m: RegexMatch) -> Int`

## Error contract

- `RegexCompileError(detail: String)`
  - `Regex::compile` で pattern が不正なときに返します。
- `NoneError`
  - `Regex::captures`, `Regex::find`, `RegexCaptures::get`, `RegexCaptures::get_name` が対象なしのときに返します。

## Examples

```surtr
rx =? re"(?<name>[A-Za-z]+)-(?<id>[0-9]+)"

print(to_string(Regex::is_match(rx, "alice-42")))

caps =? Regex::captures(rx, "alice-42")
name =? RegexCaptures::get_name(caps, "name")
id =? RegexCaptures::get(caps, 2)
print(name)
print(id)
print(RegexCaptures::whole(caps))
print(to_string(RegexCaptures::capture_count(caps)))

first =? Regex::find(rx, "alice-42")
print(RegexMatch::text(first))
print(to_string(RegexMatch::start(first)))
print(to_string(RegexMatch::end(first)))

print(Regex::replace_all(rx, "alice-42 bob-7", "X"))
print(inspect(Regex::split(re",", "a,b,c")))
```

## Notes

- `is_match` は部分一致です（全体一致したい場合は `^...$` を使う）。
- `start` / `end` は byte offset です（半開区間 `[start, end)`）。
- `group_names` は名前付きキャプチャだけを返します。
