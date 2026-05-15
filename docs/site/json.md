# JSON

Surtr では JSON を `JsonValue` と `Json` module、そして `Encode` / `Decode`
trait helper で扱います。

## まず覚えるもの

- text を JSON にする: `Json::decode(text)`
- JSON を text にする: `Json::encode(value)`
- JSON から typed value にする: `JsonValue::decode(json, TargetType)`
- typed value を JSON にする: `JsonValue::encode(value)`

`Json` は auto import されません。`Encode` / `Decode` も auto import されないため、
decode は `JsonValue::decode` helper、encode は `JsonValue::encode(value)` source alias を使います。

## `JsonValue`

`JsonValue` は JSON AST です。

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

- 整数は `JsonValue::Int`
- 小数と指数表記は `JsonValue::Float`
- object の key は常に `String`

## decode / encode

```surtr
json =? Json::decode("{\"name\":\"surtr\",\"ok\":true}")
name =? Json::get(json, "name") |>= JsonValue::decode(String)
ok =? Json::get(json, "ok") |>= JsonValue::decode(Boolean)

assert_eq("surtr", name)
assert_eq(True, ok)
```

`Json::decode` は malformed JSON を `RuntimeError` にせず、
`Err(JsonParseError(...))` として返します。

```surtr
value = JsonValue::Object(HashMap::from_entries([
  ("name", JsonValue::String("surtr")),
  ("ok", JsonValue::Bool(True)),
]))

json =? JsonValue::encode(value)
text =? Json::encode(json)
print(text)
```

```text
{"name":"surtr","ok":true}
```

## typed decode / encode

組み込みの decode は `String`, `Int`, `Float`, `Boolean`, `JsonValue` を扱えます。

```surtr
json =? Json::decode("{\"port\":8080}")
port =? Json::get(json, "port") |>= JsonValue::decode(Int)
assert_eq(8080, port)
```

pipeline でも同じ helper を使えます。

```surtr
json =? Json::decode("{\"name\":\"surtr\"}")
name =? Json::get(json, "name") |>= JsonValue::decode(String)
```

## custom schema

独自型への decode は `impl Decode<T> for JsonValue` を書きます。
独自型から JSON への encode は `impl Encode<JsonValue> for T` を書きます。

```surtr
defrecord JsonConfig(name: String, entrypoint: String)

impl Decode<JsonConfig> for JsonValue {
  def decode(self: Self, to: TypeRef<JsonConfig>) -> Result<JsonConfig, Error> {
    name =? Json::get(self, "name") |>= JsonValue::decode(String)
    entrypoint =? Json::get(self, "entrypoint") |>= JsonValue::decode(String)
    Ok(JsonConfig(name, entrypoint))
  }
}

impl Encode<JsonValue> for JsonConfig {
  def encode(self: Self, to: TypeRef<JsonValue>) -> Result<JsonValue, Error> {
    Ok(JsonValue::Object(HashMap::from_entries([
      ("name", JsonValue::String(self.name)),
      ("entrypoint", JsonValue::String(self.entrypoint)),
    ])))
  }
}

json =? Json::decode("{\"name\":\"surtr\",\"entrypoint\":\"boot\"}")
cfg =? JsonValue::decode(json, JsonConfig)
roundtrip_json =? JsonValue::encode(cfg)
roundtrip_text =? Json::encode(roundtrip_json)
```

## helper functions

`Json` module には field / index access と typed accessor があります。

- `Json::get(value, key)`
- `Json::at(value, index)`
- `Json::kind(value)`
- `Json::as_string(value)`
- `Json::as_int(value)`
- `Json::as_float(value)`
- `Json::as_bool(value)`
- `Json::as_array(value)`
- `Json::as_object(value)`

たとえば accessor を直接使うと次のようになります。

```surtr
json =? Json::decode("\"surtr\"")
text =? Json::as_string(json)
assert_eq("surtr", text)
```

## エラーの読み方

- parse failure: `JsonParseError`
- schema mismatch: `JsonDecodeError`
- stringify failure: `JsonEncodeError`

`JsonValue::decode(...)` が失敗すると `Err(JsonDecodeError(...))` として観測できます。

```surtr
json =? Json::decode("42")
result = JsonValue::decode(json, String)
assert_eq("JsonDecodeError", Error::kind(Result::err(result)))
```
