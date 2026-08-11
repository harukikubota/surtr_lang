# Definitions And Usage

ここでは、Surtr でよく使う定義の置き場所と、利用側からどう見えるかをまとめます。

## `def`

REPL でそのまま試しやすいのは `def` です。

```text
xldr(1)> def add1(x: Int) -> Int { x + 1 }
xldr(2)> print(to_string(add1(41)))
42
xldr(3)>
```

型注釈の書き方自体は `./type-annotations.md` にまとめています。

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

deferror InvalidPort(port: Int) { "invalid port" }
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

## `Result`

Surtr では失敗も値として扱います。

```text
xldr(1)> def parse_bool(text: String) -> Result<Boolean> { match text { "true" => Ok(True), "false" => Ok(False), _ => Err(NoneError), } }
xldr(2)> print(match parse_bool("true") { Ok(flag) => if(flag, "yes", "no"), Err(err) => inspect(err), })
yes
xldr(3)>
```

## `from(...)`

target type を取る変換は、value ではなく型スロットとして読みます。

```text
xldr(1)> print(from::<String>(42))
42
xldr(2)>
```

この `String` は ordinary value ではなく、変換先型の指定です。  
型注釈と明示型引数のルール全体は `./type-annotations.md` を参照してください。

## 関連ページ

- `Result` や `match` は `./pattern-matching.md`
- 関数コール / capture / closure / FuncLiteral は `./callables.md`
- 型注釈は `./type-annotations.md`
- trait 経由の変換は `./trait-impls.md`
- import / include は `./language-features.md`

## 確認したソース

- ソース
  - `../../lib/kernel.srt`

## 躓きやすいポイント

- `defstruct` / `defenum` / `defextractor` のような宣言は REPL top-level にそのまま置けません。
- `from::<TargetTy>(value)` の第2引数は ordinary value ではなく型指定スロットです。
