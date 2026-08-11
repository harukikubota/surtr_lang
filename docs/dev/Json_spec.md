# Json / Encode / Decode spec

`Json` 標準 surface と `Encode` / `Decode` helper family の開発者向け正本。

この文書は次を扱う。

- 標準ライブラリ surface
- resolver / typechecker の lowering 契約
- runtime builtin 契約
- テストで固定すべき観点

language-level の正本は `doc/要件定義v9.md`、runtime 実装の詳細は
`EldrVM_spec.md`、テスト配置方針は `テスト方針.md` を併読する。

---

## 1. Surface

### 1.1 Top-level types

- `JsonValue` は JSON AST を表す source enum とする
- `Json` は type ではなく qualified operation namespace とする
- `Json` 自体は auto import しない

`JsonValue` の shape は次で固定する。

```surtr
defenum JsonValue {
  Null,
  Bool(Boolean),
  Int(Int),
  Float(Float),
  String(String),
  Array(List<JsonValue>),
  Object(HashMap<JsonValue>),
}
```

- `Object` の key は常に `String`
- object 表示と stringify 順序は `HashMap` の deterministic key order に従う
- JSON integer literal は `JsonValue::Int(Int)` として読む
- JSON decimal / exponent literal は `JsonValue::Float(Float)` として読む

### 1.2 Error surface

- malformed text JSON は `Err(JsonParseError(...))` を返す
- decode mismatch は `Err(JsonDecodeError(...))` を返す
- stringify 不能値は `Err(JsonEncodeError(...))` を返す
- VM 内部不整合だけは `RuntimeError` でよい

### 1.3 Trait surface

`Encode` / `Decode` は target-oriented trait とし、prelude へ auto import しない。

```surtr
deftrait Encode<$To> {
  def encode(self: Self, to: TypeRef<$To>) -> Result<$To, Error>
}

deftrait Decode<$To> {
  def decode(self: Self, to: TypeRef<$To>) -> Result<$To, Error>
}
```

- `JsonValue::decode(value, Target)` の `Target` は `TypeRef` witness slot として解釈する
- `JsonValue::encode(value)` は標準ライブラリ上の source alias として `Encode::encode(value, JsonValue)` を呼び、`Encode<JsonValue> for typeof(value)` へ dispatch する
- `value |> JsonValue::decode(Target)` と `value |> JsonValue::encode()` も同じ surface として扱う
- dispatch は receiver 引数の型と `To` witness で決定し、同じ pattern の再帰的 call も許可する
- `TypeRef` は trait method / trait impl method signature の witness parameter にだけ現れてよい
- 通常 `def` の parameter / return / field / local annotation では使えない

### 1.4 Standard operations

`defmod Json` は少なくとも次を持つ。

- builtin:
  - `parse(text: String) -> Result<JsonValue, JsonParseError>`
  - `stringify(value: JsonValue) -> Result<String, JsonEncodeError>`
- source helper:
  - `decode(text: String) -> Result<JsonValue, JsonParseError>`
  - `encode(value: JsonValue) -> Result<String, JsonEncodeError>`
  - `get(value, key)`
  - `at(value, index)`
  - `kind(value)`
  - `as_string(value)`
  - `as_int(value)`
  - `as_float(value)`
  - `as_bool(value)`
  - `as_array(value)`
  - `as_object(value)`

schema-level decode は builtin ではなく、利用者が
`impl Decode<T> for JsonValue` を明示実装して書く。
schema-level encode は `impl Encode<JsonValue> for T` を明示実装して書く。

---

## 2. Compile-time contract

### 2.1 Resolver

- `JsonValue::decode(...)` helper は trait helper として解決し、`JsonValue::encode(...)` は `impl JsonValue` の source alias として解決する
- helper call の target 引数は `Resolved::TypeRefWitness` に lower する
- pipeline partial call でも同じ lowering を使う
- `Encode::encode` / `Decode::decode` の impl dispatch は通常の trait call と同じ registry を通す

### 2.2 Typechecker

- helper call は generic trait call として型検査する
- `|>` と `|>=` の RHS に trait helper partial call が来た場合、LHS の型から受け口を concretize する
- `Decode::decode` / `Encode::encode` の impl dispatch は receiver 引数の型と `To` の `TypeRef` witness で決定する
- 同じ receiver / `To` pattern の recursive call が現れても compile error にはしない
- `Decode` は `Facet` を引数に受け取らない
- `Facet` は decode 前の `JsonValue` inspection、または decode 後の typed value update にだけ使う

---

## 3. Runtime contract

### 3.1 Builtin names

- `Json::parse` は runtime builtin `json_parse` に解決する
- `Json::stringify` は runtime builtin `json_stringify` に解決する
- `Json::decode` / `Json::encode` は source wrapper としてそれぞれ `parse` / `stringify` を呼ぶ
- Json 専用 opcode は追加しない

### 3.2 Runtime representation

- runtime 実装は `serde_json` を使って text JSON と `serde_json::Value` を相互変換する
- Surtr runtime value への変換では `TypeRegistry` から `JsonValue` variant tag を名前で引く
- tag 番号をハードコードしない
- `Object` は `HashMapHandle` に変換する
- duplicate key は parser 側の後勝ち値を採用する
- stringify 時の object key order は `HashMapHandle` の deterministic order に従う

---

## 4. Stdlib load order

compile 側の標準定義ソースロード順は次に固定する。

`Bootstrap -> [SpecialTypes, Function, Kernel, Add, Sub, Mul, Eq, Neq, Compare, Concat, Show, Ordering, Tuple, From, TryFrom, Encode, Decode, Functor, Applicative, Monad, PipeApply, Compose, Composable, LiftComposable, KleisliComposable, Int, String, Regex, Boolean, Error, List, Generator, HashMap, Result, Duration, Range, Option, Task, Facet, Float, Json, Config, Project, Random, File, FS, IO, Shell, StyledDoc] -> [Test] -> ユーザ拡張`

- `Encode` / `Decode` は `From` / `TryFrom` の後、`Json` の前にロードする
- `JsonValue` は `Json` module 側で定義し、helper trait 側から参照される

---

## 5. Testing contract

最低限、次を回帰基準にする。

- `unit/sigil`
  - `JsonValue::decode` helper が trait helper に解決し、`JsonValue::encode` source alias が typecheck できる
  - bare `encode` / `decode` が prelude だけでは解決されない
  - direct call と pipeline partial call の witness lowering が一致する
- `unit/scar`
  - helper call が generic trait call として型検査できる
  - `|>` / `|>=` で helper RHS を concretize できる
- `unit/eldr`
  - `Json::parse` が `null`, boolean, string, integer, decimal, exponent, array, object を正しく分類する
  - `Json::stringify` の object key order が deterministic である
- `spec/json`
  - malformed JSON が `Err(JsonParseError(...))` として観測できる
  - type mismatch が `Err(JsonDecodeError(...))` として観測できる
  - custom `impl Decode<Config> for JsonValue` が `Json::get(...) |>= JsonValue::decode(T)` と `=?` で書ける
  - custom `impl Encode<JsonValue> for Config` が `Config -> JsonValue -> String` の file RW 例で使える
  - 同じ pattern の recursive decode / encode call が compile error にならない
  - decode 後の typed value に `Facet::over` / `Facet::set` を適用できる
