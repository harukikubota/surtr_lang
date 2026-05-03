# Regex

`Regex` は Rust `regex` crate をラップした標準モジュールです。
compile した正規表現値を `Regex` として保持し、マッチ判定、キャプチャ取得、置換、分割を行います。

## 最初の 3 点

- `re"pattern"` / `re'pattern'` は `Regex::compile("pattern")` へ lower される sugar です
- `Regex::is_match` は部分一致です。全体一致したいときは `^...$` を使います
- `Regex::captures` や `Regex::find` は対象がないと `Err(NoneError)` を返します

## 生成

```surtr
rx =? re"(?<name>[A-Za-z]+)-(?<id>[0-9]+)"
```

これは次と同じです。

```surtr
rx =? Regex::compile("(?<name>[A-Za-z]+)-(?<id>[0-9]+)")
```

pattern が不正なら `Err(RegexCompileError(detail))` になります。

## 主な API

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

## キャプチャとマッチ

```surtr
rx =? re"(?<name>[A-Za-z]+)-(?<id>[0-9]+)"
caps =? Regex::captures(rx, "alice-42")
name =? RegexCaptures::get_name(caps, "name")
id =? RegexCaptures::get(caps, 2)

print(RegexCaptures::whole(caps))
print(to_string(RegexCaptures::capture_count(caps)))
print(name)
print(id)
```

使う accessor は次です。

- `RegexCaptures::whole(caps: RegexCaptures) -> String`
- `RegexCaptures::capture_count(caps: RegexCaptures) -> Int`
- `RegexCaptures::get(caps: RegexCaptures, idx: Int) -> Result<String, NoneError>`
- `RegexCaptures::get_name(caps: RegexCaptures, name: String) -> Result<String, NoneError>`
- `RegexMatch::text(m: RegexMatch) -> String`
- `RegexMatch::start(m: RegexMatch) -> Int`
- `RegexMatch::end(m: RegexMatch) -> Int`

`capture_count` は `group 0` を含みます。  
`start` / `end` は byte offset で、区間は `[start, end)` です。

## 例

```surtr
rx =? re"(?<name>[A-Za-z]+)-(?<id>[0-9]+)"

print(to_string(Regex::is_match(rx, "alice-42")))

caps =? Regex::captures(rx, "alice-42")
name =? RegexCaptures::get_name(caps, "name")
id =? RegexCaptures::get(caps, 2)
print(name)
print(id)

first =? Regex::find(rx, "alice-42")
print(RegexMatch::text(first))
print(to_string(RegexMatch::start(first)))
print(to_string(RegexMatch::end(first)))

print(Regex::replace_all(rx, "alice-42 bob-7", "X"))
print(inspect(Regex::split(re",", "a,b,c")))
print(inspect(Regex::group_names(rx)))
```

## エラーの読み方

- `RegexCompileError`
  - pattern 自体が不正
- `NoneError`
  - マッチが見つからない
  - 指定した capture index / name が存在しない

`Regex` API は例外ではなく `Result` で失敗を返します。  
一直線の処理にしたいときは `=?` や `|>=` を併用すると読みやすくなります。

## どこを見るか

- source 上の一次情報: `../../lib/regex.srt`
- source 上の一次情報: `../../lib/types/regex.srt`
- `Result` の扱い: `./error-handling.md`
