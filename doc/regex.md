# Regex 仕様書（draft）

最終更新日: 2026-04-13

---

## 1. 目的

Surtr に正規表現を導入するための surface 仕様と runtime 契約を定義する。

- 生成リテラル: `re"pattern"` / `re'pattern'`
- 組込み型: `Regex`, `RegexCaptures`, `RegexMatch`
- 組込み API: compile / match / capture / find / replace 系

本書は Rust 実装（`regex` crate ラッパー）を前提とする。

---

## 2. 生成リテラル（sugar）

### 2.1 構文

- `re"pattern"`
- `re'pattern'`

### 2.2 Lowering

`re` 生成リテラルは次へ展開する。

```surtr
re"abc"   => Regex::compile("abc")
re'abc'   => Regex::compile("abc")
```

つまり型は `Result<Regex, RegexCompileError>` である。

### 2.3 利用上の注意

- 失敗しうる値なので、`=?` / `match` / `Result` pipeline で扱う。
- 動的パターンは通常どおり `Regex::compile(pattern)` を使う。

---

## 3. 型とエラー

```surtr
@@builtin type Regex
@@builtin type RegexCaptures
@@builtin type RegexMatch
```

```surtr
deferror RegexCompileError(detail: String) {
  detail
}
```

- `RegexCompileError` はパターン不正時に返す。
- マッチ不成立や group 不在は `NoneError` で表す（Surtr の `Result` 統一方針に合わせる）。

---

## 4. API 仕様

### 4.1 `Regex` モジュール

```surtr
defmod Regex {
  def compile(pattern: String) -> Result<Regex, RegexCompileError>
  def is_match(re: Regex, input: String) -> Boolean
  def captures(re: Regex, input: String) -> Result<RegexCaptures, NoneError>

  def find(re: Regex, input: String) -> Result<RegexMatch, NoneError>
  def find_all(re: Regex, input: String) -> List<RegexMatch>
  def split(re: Regex, input: String) -> List<String>

  def replace(re: Regex, input: String, replacement: String) -> String
  def replace_all(re: Regex, input: String, replacement: String) -> String

  def escape(text: String) -> String
  def group_names(re: Regex) -> List<String>
}
```

### 契約

- `is_match`: 部分一致ベース（`regex::Regex::is_match` と同様）
- `find`: 最初の一致 1 件。なければ `Err(NoneError)`
- `find_all`: 左から順に全一致。なければ空リスト
- `replace`: 最初の一致のみ置換
- `replace_all`: 全一致を置換
- `group_names`: 名前付きキャプチャ名のみ返す（定義順、重複なし）

### 4.2 `RegexCaptures` モジュール

```surtr
defmod RegexCaptures {
  def whole(caps: RegexCaptures) -> String
  def group_count(caps: RegexCaptures) -> Int
  def get(caps: RegexCaptures, idx: Int) -> Result<String, NoneError>
  def get_name(caps: RegexCaptures, name: String) -> Result<String, NoneError>
}
```

### 契約

- `whole`: group `0`（全体一致）を返す
- `group_count`: group `0` を含む総数
- `get`:
  - index 範囲外 or 対象 group 未一致なら `Err(NoneError)`
  - 一致していれば `Ok(text)`
- `get_name`:
  - name 未定義 or 対象 group 未一致なら `Err(NoneError)`
  - 一致していれば `Ok(text)`

### 4.3 `RegexMatch` モジュール

```surtr
defmod RegexMatch {
  def text(m: RegexMatch) -> String
  def start(m: RegexMatch) -> Int
  def end(m: RegexMatch) -> Int
}
```

### 契約

- `start` / `end` は `input` の半開区間 `[start, end)` を返す。

---

## 5. キャプチャ内部モデル（実装指針）

`RegexCaptures` は `HashMap<String, Option<String>>` 単体ではなく、次の保持を推奨する。

- `groups: Vec<Option<(start, end)>>`
- `name_to_index: HashMap<String, usize>`
- `input: String`（または実行時に安全に保持できる等価参照）

理由:

- index / name の両アクセスを同一経路に統一できる
- 文字列コピーを遅延できる
- `get_name(name)` を `name -> idx -> get(idx)` で実装できる

---

## 6. Rust ラッパー方針

- エンジンは Rust `regex` crate を利用する。
- サポート範囲は同 crate の仕様に従う（未サポート構文は compile error）。
- Surtr surface には `Result` / `NoneError` で公開し、Rust 側の `Option` は露出しない。

---

## 7. サンプルコード

### 7.1 単純マッチ（Boolean）

```surtr
rx =? re"^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\\.[A-Za-z]{2,}$"
ok: Boolean = Regex::is_match(rx, "alice@example.com")
print(to_string(ok))
```

### 7.2 名前付きキャプチャ

```surtr
def parse_user(input: String) -> Result<(String, String), Error> {
  rx =? re"^(?<name>[A-Za-z]+):(?<role>[a-z_]+)$"
  caps =? Regex::captures(rx, input)
  name =? RegexCaptures::get_name(caps, "name")
  role =? RegexCaptures::get_name(caps, "role")
  Ok((name, role))
}
```

### 7.3 optional group

```surtr
def parse_phone(input: String) -> Result<(String, String), Error> {
  rx =? re"^(?:(?<area>[0-9]{2,4})-)?(?<local>[0-9]{4})$"
  caps =? Regex::captures(rx, input)

  area: String = match RegexCaptures::get_name(caps, "area") {
    Ok(v) => v,
    Err(NoneError) => "000",
  }

  local =? RegexCaptures::get_name(caps, "local")
  Ok((area, local))
}
```

### 7.4 find / find_all

```surtr
rx =? re"[0-9]+"
first =? Regex::find(rx, "id=12, code=345")
print(RegexMatch::text(first))   // "12"
print(to_string(RegexMatch::start(first))) // 3
print(to_string(RegexMatch::end(first)))   // 5

all = Regex::find_all(rx, "id=12, code=345")
print(to_string(len(all))) // 2
```

### 7.5 replace / split / escape

```surtr
ws =? re"\\s+"
collapsed = Regex::replace_all(ws, "a   b   c", " ")
print(collapsed) // "a b c"

comma =? re","
parts = Regex::split(comma, "a,b,c")
print(inspect(parts)) // ["a", "b", "c"]

raw = Regex::escape("a+b*c")
print(raw) // "a\\+b\\*c"
```

---

## 8. 非目標（現時点）

- callback ベース置換（`replace_with`）
- フラグ付き compile API（`compile_with_flags`）
- streaming match API

上記は必要性が確認できた時点で追加検討する。
