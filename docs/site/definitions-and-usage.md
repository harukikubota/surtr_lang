# Definitions And Usage

ここでは、Surtr でよく使う定義の置き場所と、利用側からどう見えるかをまとめます。

## `defmod`

module の関数は `defmod Name { ... }` の中に定義します。呼び出し側では `Name::function(...)` と書きます。

### `def` と `defp`

`def` は module の外からも呼べる関数、`defp` は定義した module 内だけで使える private helper です。

```surtr
defmod Math {
  def add1(value: Int) -> Int = add(value, 1)
  defp add(left: Int, right: Int) -> Int = left + right
}
```

利用側では `Math::add1(41)` と呼びます。`defp` は import 対象にならず、`Math::add(41, 1)` のように外部から呼ぶこともできません。impl 内の `defp` も、その impl 内だけで参照できます。

### 関数本体の定義パターン

複数の処理を並べるときは `{ ... }` を使います。短い関数は `=` の後に単一行の式を書けます。

```surtr
defmod Math {
  def add(left: Int, right: Int) -> Int {
    left + right
  }

  def add1(value: Int) -> Int = value + 1
  def positive?(value: Int) -> Boolean = value > 0
  def choose(flag: Boolean) -> Int = if(flag, 1, 0)
  def discard(value: Int) -> Unit = value;
}
```

`= expr` の末尾に `;` を1個付けると、結果を Unit にします。その他の式を同じ行に続けることはできません。括弧、引数、closure、文字列の内部も含め、本体を一行に収めます。

`do`、`match`、`cond` を本体に直接書く形式では、複数行を使えます。

```surtr
defmod Examples {
  def report(value: Int) -> Result<()> = do {
    num <- Ok(value)
    print(to_string(num))
    Ok(())
  }

  def option_tag(value: Option<Int>) -> Int = match value {
    Option::Some(_) => 1,
    Option::None => 0
  }

  def sign(value: Int) -> Int = cond {
    value > 0 => 1,
    value < 0 => -1,
    True => 0
  }
}
```

`do` は戻り値型を期待型として carrier を決定します。上の `report` では carrier は `Result` です。`match` はパターンで、`cond` は条件で分岐します。これらの本体末尾には `;` を付けません。

同じ定義パターンを `defp`、impl の関数、trait のデフォルト本体にも使えます。詳しい制約は[言語リファレンス](./language-reference.md#関数)、型注釈は[型注釈](./type-annotations.md)を参照してください。

### Script・REPL のトップレベル定義

Script と REPL では、`defmod` で囲まずに `def` を定義できます。これらの関数は暗黙の擬似 module に属し、同じ Script や REPL から名前で呼びます。関数本体には、上で紹介した定義パターンを使えます。

Script の例です。

```surtr
def add1(value: Int) -> Int = value + 1
print(to_string(add1(41)))
```

REPL でも同じように試せます。

```text
xldr(1)> def add1(value: Int) -> Int = value + 1
xldr(2)> print(to_string(add1(41)))
42
xldr(3)>
```

## `defstruct` / `defrecord` / `defenum` / `deferror`

これらは file-oriented な宣言です。  
REPL top-level には直接置かず、`.srt` file で定義します。

```surtr
defstruct User {
  name: String,
  age: Int,
}

defrecord Config(host: String, port: Int)

defenum Mode {
  Dev,
  Prod,
}

deferror InvalidPort(port: Int) {
  |port: Int|
  Self(message: "invalid port", port)
}
```

使うときの見え方は次の通りです。

- `User { ... }` は struct literal
- `Config(...)` は record constructor
- `Mode::Dev` は enum variant
- `InvalidPort(...)` は concrete error value

## `impl Type`

型に属する helper は `impl Type { ... }` に置きます。

```surtr
impl User {
  def new(name: String, age: Int) -> Self {
    User { name, age }
  }
}
```

呼び出し側は `Type::method(...)` で読みます。

`defstruct` の内部再構築では field shorthand も使えます。

```surtr
impl User {
  def with_age(self: Self, next_age: Int) -> Self {
    User { name: self.name, age: next_age }
  }
}
```

`new`、構造体リテラル、`deconstruct`、private field、property access のまとまった説明は
`./structs.md` にあります。
Record の定義・構築・分解・位置 path は [`record.md`](./record.md) を参照してください。

## `Result`

Surtr では失敗も値として扱います。

```surtr
defmod Parsing {
  def parse_bool(text: String) -> Result<Boolean> = match text {
    "true" => Ok(True),
    "false" => Ok(False),
    _ => Err(NoneError)
  }
}
```

`Parsing::parse_bool("true")` は `Ok(True)` を返します。結果は `match` で成功値とエラーに分けて扱えます。

## `to(...)`

target type を取る変換は、value ではなく型スロットとして読みます。

```text
xldr(1)> print(to::<String>(42))
42
xldr(2)>
```

この `String` は ordinary value ではなく、変換先型の指定です。  
型注釈と明示型引数のルール全体は `./type-annotations.md` を参照してください。

## 関連ページ

- `Result` や `match` は `./pattern-matching.md`
- 関数コール / capture / closure / FuncLiteral は `./callables.md`
- 型注釈は `./type-annotations.md`
- 変換 Trait のAPIは [`Convert / TryConvert`](./traits/convert.md)
- trait 実装全般は `./trait-impls.md`
- import / include は `./language-features.md`

## 確認したソース

- ソース
  - `../../lib/kernel.srt`

## 躓きやすいポイント

- `defstruct` / `defenum` / `defextractor` のような宣言は REPL top-level にそのまま置けません。
- `to::<TargetTy>(value)` の第2引数は ordinary value ではなく型指定スロットです。
