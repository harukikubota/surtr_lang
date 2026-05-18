# Error Handling

Surtr では例外機構を持ちません。  
失敗は `raise` するものではなく、`Result` に乗った値として返し、必要ならその場で `match` して回復します。

process surface の `init` / `get` / `set` / `call` でも同じ流儀を使います。`PID<T>` や singleton / worker の全体像は `./process.md` を見てください。

## 基本方針

- 失敗しうる処理は `Result<T>` を返す
- 成功値は `Ok(value)`
- 失敗値は `Err(error)`
- `Err(...)` を見つけたら、そのまま呼び出し元へ早期リターンできる

## `Error` は抽象、実体は常に具象 error

Surtr でコード中に `Error` と書かれていても、それは「失敗値の共通な見え方」を指す抽象名です。  
runtime にある実体は常に `deferror` で定義した具象 error です。

```surtr
deferror InvalidPort(port: Int) { "invalid port" }

ret: Result<Int> = Err(InvalidPort(0))
```

このとき `Err(...)` の中に入っている実値は `InvalidPort(0)` であり、`Error(...)` のような別の concrete value が存在するわけではありません。

あわせて、`Error` は user-owned な一般データ型としては使えません。

- ユーザー定義関数の引数型に `Error` は書けない
- ユーザー定義関数の戻り値型に `Error` は書けない
- 変数や field の型注釈に `Error` は書けない
- `Error` が生きられるのは `Err(...)` の内側、`match` の `Err(err)` で取り出したスコープ、標準定義ソース内の `Error` を受ける helper の中だけ

つまり、ユーザーコードが `Error` を保存したり運び回ったりするのではなく、具象 error を `Result` の失敗枝として流し、その観測だけを抽象 `Error` 越しに行うのが Surtr の流儀です。

```surtr
def parse_port(text: String) -> Result<Int> {
  value: Int =? try_from(text, Int)
  if(value > 0, Ok(value), Err(InvalidPort(value)))
}
```

上は「失敗したらその地点で抜ける」コードですが、例外を投げているわけではありません。  
概念的には次の `match` に近い動きです。

```surtr
def parse_port(text: String) -> Result<Int> {
  parsed = try_from(text, Int)
  match parsed {
    Ok(value) => if(value > 0, Ok(value), Err(InvalidPort(value))),
    _ => parsed,
  }
}
```

## `Result` が標準、`Option` は別コンテナ

Surtr では optional value も、まず `Result` で扱うのが基本です。  
特に「値がない」を recoverable failure として扱うときは `Err(NoneError)` を使います。

```surtr
def first_or_error(xs: List<Int>) -> Result<Int> {
  List::first(xs)
}
```

この種の API は、利用者視点では `Option<T>` ではなく
`Result<T, NoneError>` を返す失敗 API として読むのが自然です。

```surtr
match List::first([10, 20, 30]) {
  Ok(value) => to_string(value),
  Err(NoneError) => "empty",
  Err(err) => inspect(err),
}
```

`Option` 自体を値表現として使う場面はありますが、早期リターンや関数演算子の主軸は `Result` です。

## `match` で処理する

もっとも直接的な書き方は `match` です。

```surtr
def parse_bool(text: String) -> Result<Boolean> {
  match text {
    "true" => Ok(True),
    "false" => Ok(False),
    _ => Err(NoneError),
  }
}

def render_bool(text: String) -> String {
  match parse_bool(text) {
    Ok(flag) => if(flag, "yes", "no"),
    Err(NoneError) => "missing or invalid",
    Err(err) => inspect(err),
  }
}
```

役割は次のとおりです。

- `Ok(...)` branch で成功値を使う
- `Err(...)` branch で回復、変換、再送出を選ぶ
- recover しないなら `Err(err)` をそのまま返す

`Err(err)` arm で束縛した `err` は抽象 `Error` として見えますが、中身は依然として具象 error です。  
そのため `Error::kind(err)` や `Error::format(err)` のような共通 helper で観測でき、`Result::map_err(..., err)` や `assert(..., err)` のような標準 helper へそのまま渡せます。

## `=?` SafeBind と早期リターン

`=?` は「`Ok` を取り出し、`Err` ならその場で返す」ための束縛です。

```surtr
def parse_and_increment(text: String) -> Result<Int> {
  value: Int =? try_from(text, Int)
  Ok(value + 1)
}
```

これは次の `match` 展開として読めます。

```surtr
def parse_and_increment(text: String) -> Result<Int> {
  parsed = try_from(text, Int)
  match parsed {
    Ok(value) => Ok(value + 1),
    _ => parsed,
  }
}
```

SafeBind は複数段にも使えます。

```surtr
def load_pair(a: String, b: String) -> Result<Int> {
  left: Int =? try_from(a, Int)
  right: Int =? try_from(b, Int)
  Int::safe_div(left + right, 2)
}
```

概念的には左から順に `match` が入れ子になります。

```surtr
def load_pair(a: String, b: String) -> Result<Int> {
  left_result = try_from(a, Int)
  match left_result {
    Ok(left) => {
      right_result = try_from(b, Int)
      match right_result {
      Ok(right) => Int::safe_div(left + right, 2),
      _ => right_result,
    }
    },
    _ => left_result,
  }
}
```

## 関数演算子での `Result` 処理

`Result` は `match` で直接書けますが、処理の流れが一直線なら演算子でも書けます。

### `|*>` fmap

成功値だけを pure function で変換します。

```surtr
Ok(10) |*> add(1)
```

`match` へ読み下すと次です。

```surtr
mapped = Ok(10)
match mapped {
  Ok(value) => Ok(add(value, 1)),
  _ => mapped,
}
```

### `|>=` bind

成功値を次の `Result` 返却関数へ渡します。

```surtr
try_from("42", Int) |>= require_at_least(10)
```

`match` へ読み下すと次です。

```surtr
parsed = try_from("42", Int)
match parsed {
  Ok(value) => require_at_least(value, 10),
  _ => parsed,
}
```

### `>*` lifted compose

`A -> Result<B>` の後ろへ `B -> C` を繋ぎます。

```surtr
pipeline = &parse_int >* &to_string
```

入力 `x` に適用したときの読み方は次です。

```surtr
def pipeline(x: String) -> Result<String> {
  parsed = parse_int(x)
  match parsed {
    Ok(value) => Ok(to_string(value)),
    _ => parsed,
  }
}
```

### `>=>` Kleisli compose

`A -> Result<B>` と `B -> Result<C>` を直列接続します。

```surtr
pipeline = &parse_int >=> &require_small
```

入力 `x` に適用したときの読み方は次です。

```surtr
def pipeline(x: String) -> Result<Int> {
  parsed = parse_int(x)
  match parsed {
    Ok(value) => require_small(value),
    _ => parsed,
  }
}
```

## エラー回復

Surtr のエラー回復は「例外を捕まえる」のではなく、`Err` を `match` して別値へ変換することです。

### 値へ回復する

```surtr
def read_with_default(text: String) -> Int {
  match try_from(text, Int) {
    Ok(value) => value,
    Err(NoneError) => 0,
    Err(_) => 0,
  }
}
```

### 別の `Result` へ回復する

```surtr
def parse_or_zero(text: String) -> Result<Int> {
  parsed = try_from(text, Int)
  match parsed {
    Ok(value) => Ok(value),
    Err(NoneError) => Ok(0),
    _ => parsed,
  }
}
```

### 文脈を足して再送出する

```surtr
def require_port(text: String) -> Result<Int> {
  match try_from(text, Int) {
    Ok(value) => if(value > 0, Ok(value), Err(InvalidPort(value))),
    Err(_) => Err(InvalidPort(-1)),
  }
}
```

## 使い分けの目安

- 分岐を明示したいときは `match`
- 失敗をそのまま流したいときは `=?`
- 直線的な pipeline は `|*>`, `|>=`, `>*`, `>=>`
- recover したいときは `Err(...)` branch を明示的に書く

## 関連ページ

- `match` の基本は `./pattern-matching.md`
- 演算子全体の制約は `./language-reference.md`
- 標準 `Result` / `Option` の位置づけは `./standard-library.md`
