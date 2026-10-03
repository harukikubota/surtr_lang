# Regex

`Regex` は Rust `regex` crate をラップした標準定義ソースです。
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

文字列として成立したpatternが不正なら `Err(RegexCompileError(detail))` になります。

### バックスラッシュの書き方

regex sugarも[通常の文字列のエスケープ規則](./language-reference.md#文字列)を使います。
正規表現の `\d` や `\s` を渡すには、Surtr文字列のバックスラッシュを `\\` と書きます。

```surtr
rx =? re"\\d+"
print(to_string(Regex::is_match(rx, "123")))
```

`re"\d"` は未知のエスケープとして解析時に拒否します。従来の表記を使っていた場合は `re"\\d"` に書き換えてください。
`re"\u{1b}"` はESC1文字を渡し、`re"\\u{1b}"` は文字としての `\u{1b}` を正規表現エンジンへ渡します。
Surtrの文字列解析と正規表現エンジンの検証は別の段階です。
`re"..."` / `re'...'` 内の補間は通常文字列と同じ元ソース上の規則で扱います。

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

- 文字列の解析エラー
  - `re"\d"` などの未知エスケープや、不正なUnicodeエスケープ
  - 正規表現のコンパイル前に拒否する
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
