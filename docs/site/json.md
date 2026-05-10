# JSON

Surtr では JSON を `JsonValue` と `Json` module、そして `Encode` / `Decode`
trait helper で扱います。

## まず覚えるもの

- text を JSON にする: `Json::parse(text)`
- JSON を text にする: `Json::stringify(value)`
- helper で decode する: `decode(value, JsonFormat, TargetType)`
- helper で encode する: `encode(value, JsonFormat)`

`Json` は auto import されません。`Encode` / `Decode` helper は auto import されます。

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

## parse

```surtr
json =? Json::parse("{\"name\":\"surtr\",\"ok\":true}")
name =? Json::get(json, "name") |>= decode(JsonFormat, String)
ok =? Json::get(json, "ok") |>= decode(JsonFormat, Boolean)

assert_eq("surtr", name)
assert_eq(True, ok)
```

`Json::parse` は malformed JSON を `RuntimeError` にせず、
`Err(JsonParseError(...))` として返します。

## stringify

```surtr
value = JsonValue::Object(HashMap::from_entries([
  ("name", JsonValue::String("surtr")),
  ("ok", JsonValue::Bool(True)),
]))

text =? Json::stringify(value)
print(text)
```

```text
{"name":"surtr","ok":true}
```

`encode(value, JsonFormat)` は `Json::stringify(...)` の helper 版です。

```surtr
text =? encode(JsonValue::Int(42), JsonFormat)
assert_eq("42", text)
```

## typed decode

組み込みの decode は `String`, `Int`, `Float`, `Boolean`, `JsonValue` を扱えます。

```surtr
json =? Json::parse("{\"port\":8080}")
port =? Json::get(json, "port") |>= decode(JsonFormat, Int)
assert_eq(8080, port)
```

pipeline でも同じ helper を使えます。

```surtr
json =? "{\"name\":\"surtr\"}" |> decode(JsonFormat, JsonValue)
name =? Json::get(json, "name") |>= decode(JsonFormat, String)
```

## custom schema decode

独自型へは `impl Decode<JsonFormat, T> for JsonValue` を書きます。

```surtr
defstruct JsonConfig {
  name: String,
  entrypoint: String,
}

impl Decode<JsonFormat, JsonConfig> for JsonValue {
  def decode(self: Self, format: TypeRef<JsonFormat>, to: TypeRef<JsonConfig>) -> Result<JsonConfig, Error> {
    name =? Json::get(self, "name") |>= decode(JsonFormat, String)
    entrypoint =? Json::get(self, "entrypoint") |>= decode(JsonFormat, String)
    Ok(JsonConfig { name, entrypoint })
  }
}

json =? Json::parse("{\"name\":\"surtr\",\"entrypoint\":\"boot\"}")
cfg =? decode(json, JsonFormat, JsonConfig)
assert_eq(("surtr", "boot"), (cfg.name, cfg.entrypoint))
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
json =? Json::parse("\"surtr\"")
text =? Json::as_string(json)
assert_eq("surtr", text)
```

## エラーの読み方

- parse failure: `JsonParseError`
- schema mismatch: `JsonDecodeError`
- stringify failure: `JsonEncodeError`

`decode(...)` が失敗すると `Err(JsonDecodeError(...))` として観測できます。

```surtr
json =? Json::parse("42")
result = decode(json, JsonFormat, String)
assert_eq("JsonDecodeError", Error::kind(Result::err(result)))
```
