# JSON

Surtr では JSON を `JsonValue` と `Json` module、そして `Encode` / `Decode`
trait helper で扱います。

## まず覚えるもの

- text を JSON にする: `Json::decode(text)`
- JSON を text にする: `Json::encode(value)`
- JSON から typed value にする: `Decode::decode::<TargetType>(json)`
- typed value を JSON にする: `JsonValue::encode(value)`

`Json`、`Encode`、`Decode` は自動 import されません。このページでは
`Json::decode(...)` や `Decode::decode::<T>(...)` のように所属名を付けて呼びます。
`JsonValue::encode(value)` は、出力先を `JsonValue` に固定する関数です。

## `JsonValue`

`JsonValue` は JSON の構造を表す標準の列挙型です。次は定義の抜粋で、利用時に再定義する必要はありません。

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
- `JsonValue::Float` の中身は finite-only の `Float`
- object の key は常に `String`

## decode / encode

```surtr
json =? Json::decode("{\"name\":\"surtr\",\"ok\":true}")
name =? Json::get(json, "name") |>= Decode::decode::<String>
ok =? Json::get(json, "ok") |>= Decode::decode::<Boolean>

print(name) # surtr
print(to_string(ok)) # True
```

`Json::decode` は malformed JSON を `RuntimeError` にせず、
`Err(JsonParseError(...))` として返します。

```surtr
value = JsonValue::Object(HashMap::map_from_entries([
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

標準の `Decode` 実装で、`JsonValue` から `String`、`Int`、`Float`、`Boolean` へ変換できます。
`String` から `JsonValue` への変換は、JSON テキストを解析します。

```surtr
json =? Json::decode("{\"port\":8080}")
port =? Json::get(json, "port") |>= Decode::decode::<Int>
print(to_string(port)) # 8080
```

値を取り出した後は、`|>` でも同じ関数を使えます。

```surtr
json =? Json::decode("{\"name\":\"surtr\"}")
name_json =? Json::get(json, "name")
name =? name_json |> Decode::decode::<String>
print(name) # surtr
```

## custom schema

独自型への decode は `impl Decode<T> for JsonValue` を書きます。
独自型から JSON への encode は `impl Encode<JsonValue> for T` を書きます。
各メソッドにも `::<変換先の型>` を付けます。

```surtr
defrecord JsonConfig(name: String, entrypoint: String)

impl Decode<JsonConfig> for JsonValue {
  def decode::<JsonConfig>(self: Self) -> Result<JsonConfig, Error> {
    name =? Json::get(self, "name") |>= Decode::decode::<String>
    entrypoint =? Json::get(self, "entrypoint") |>= Decode::decode::<String>
    Ok(JsonConfig(name, entrypoint))
  }
}

impl Encode<JsonValue> for JsonConfig {
  def encode::<JsonValue>(self: Self) -> Result<JsonValue, Error> {
    Ok(JsonValue::Object(HashMap::map_from_entries([
      ("name", JsonValue::String(self.name)),
      ("entrypoint", JsonValue::String(self.entrypoint)),
    ])))
  }
}

json =? Json::decode("{\"name\":\"surtr\",\"entrypoint\":\"boot\"}")
cfg =? Decode::decode::<JsonConfig>(json)
roundtrip_json =? JsonValue::encode(cfg)
roundtrip_text =? Json::encode(roundtrip_json)
print(roundtrip_text)
```

```text
{"entrypoint":"boot","name":"surtr"}
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
print(text) # surtr
```

## エラーの読み方

- parse failure: `JsonParseError`
- schema mismatch: `JsonDecodeError`
- stringify failure: `JsonEncodeError`

標準の `JsonValue` からの decode は、型が合わないと `Err(JsonDecodeError(...))` を返します。

```surtr
json =? Json::decode("42")
result = Decode::decode::<String>(json)
match result {
  Ok(_) => print("unexpected-ok"),
  Err(err) => print(Error::kind(err)), # JsonDecodeError
}
```
