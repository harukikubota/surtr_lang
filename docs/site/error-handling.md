# Error Handling

Surtr では例外機構を持ちません。  
失敗は `raise` するものではなく、`Result` に乗った値として返し、必要ならその場で `match` して回復します。

成功値を作る基本構文は `Result::Ok(value)` です。糖衣構文として `Ok(value)` とも書けます。
失敗値も同様に、`Result::Err(error)` を `Err(error)` と書けます。どちらも同じ variant を参照します。
以下の説明とコード例では、糖衣構文の `Ok` / `Err` を使います。

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
deferror InvalidPort(port: Int) {
  |port: Int|
  Self(message: "invalid port", port)
}

ret: Result<Int> = Err(InvalidPort(0))
```

このとき `Err(...)` の中に入っている実値は `InvalidPort(0)` であり、`Error(...)` のような別の concrete value が存在するわけではありません。

`Error` は内部表現を公開しない通常値です。引数、戻り値、型注釈、field、container、closure に保存して渡せます。`Err(err)` で取り出した Error もスコープの外へ返せます。

```surtr
deferror InvalidPort(port: Int) {
  |port: Int|
  Self(message: "invalid port", port)
}
def relay(error: Error) -> Error { error }
error: Error = relay(InvalidPort(0))
errors: List<Error> = [error]
saved: Result<Error> = Ok(error)
```

`Ok(error)` は成功です。`value =? Ok(error)` は Error を束縛して続行します。
抽象 `Error` の直接構築と Error 自体への Trait impl はできません。保存した値は具象 Error Pattern で取り出せます。

`error.kind` と `error.message` は、それぞれ `Error::kind(error)` と `Error::message(error)` と同じ文字列を返します。`Error.kind` と `Error.message` は読み取り専用の Facet path です。他の型の Error field を経由する `Failure.error.message` も読み取り専用で、`set`・`over`・bulk update は使えません。cause や location などの内部 field は公開しません。

## Error の定義・生成・分解

### 定義する

`deferror` のヘッダには保存するフィールド、本体先頭の `|...|` にはコンストラクタの入力を書きます。末尾の `Self(...)` で message と保存値を指定します。入力と保存値は同じ名前・型でなくても構いません。

```surtr
deferror InvalidPort(port: Int) {
  |input: Int|
  Self(message: "invalid port: #{input}", port: input)
}
```

保存フィールドがない Error は `deferror NoValue { "no value" }` と書けます。`deferror NoValue() { Self(message: "no value") }` も同じ空フィールドの定義です。String を返す形では、分岐の結果から message を作れます。

```surtr
deferror MissingInput {
  |blank: Boolean|
  if(blank, "blank input", "missing input")
}
```

`Self(...)` による内部構築は、定義本体のトップレベルかつ末尾に直接置きます。分岐の内側や束縛の右辺には置けません。次はコンパイルエラーです。

```surtr
# NG: 分岐の内側で Self(...) を構築している
deferror BranchConstruction {
  |blank: Boolean|
  if(blank, Self(message: "blank input"), Self(message: "missing input"))
}
```

分岐と保存フィールドを組み合わせる場合は、String や保存値を先に計算します。

```surtr
deferror InvalidPort(port: Int) {
  |input: Int|
  message = if(input < 0, "negative port", "invalid port")
  Self(message: message, port: input)
}
```

### 生成する

定義名を呼び出すと、入力を受け取って共通 `Error` を返します。生成しただけでは、保存フィールドにアクセスできる具象型にはなりません。

```surtr
err: Error = InvalidPort(0)
err.message                 # OK: 共通情報はそのまま読める
failed: Result<Int> = Err(err)
```

引数はキーワード指定がなければ宣言順で対応します。キーワードを一つでも書くと、裸変数は同名の指定になります。

```surtr
deferror PairError(left: Int, right: Int) {
  |a: Int, b: Int|
  left = a
  right = b
  Self(message: "pair", left, right)
}
a = 1
b = 2
PairError(b, a)    # 位置指定: a は 2、b は 1
PairError(a: a, b) # 名前指定: a は 1、b は 2
```

内部の `Self(...)` と保存値の Pattern も同じ指定規則です。名前指定の列に任意の式を混ぜて、残りの位置を推測させることはできません。

### 分解する

`InvalidPort` だけなら種類を照合し、`InvalidPort(port)` なら保存した値も取り出します。コンストラクタの入力 `input` ではなく、保存フィールド `port` を照合します。

```surtr
def describe_port(error: Error) -> String {
  match error {
    InvalidPort(port) @ e => "#{port}: #{e.message}",
    other => other.message,
  }
}
```

単一の種類への照合が成功した `@ e` では、`e.port` も読めます。この読み取りは `match`、`if_let`、`if_let_then` の成功側のレキシカルスコープに限ります。同じ成功側の別名と、そこで作る通常のクロージャは読み取りを引き継ぎます。

通常の関数や List・Tuple・field を通して運ぶと共通 `Error` として扱います。保存値は保持されるので、取り出した Error を再び照合してから読んでください。異なる種類の OR 全体につけた alias も共通 `Error` です。範囲とクロージャの例は [Error Pattern](./pattern-matching.md#error-の照合とダウンキャスト) を参照してください。

`is_match(InvalidPort(0), InvalidPort(0))` は保存値の一致を Boolean で返します。`is_match` は変数束縛と alias を作りません。

## コンパイルエラーになる例

次の NG 例はそれぞれ独立した拒否例です。`InvalidPort` と `PairError` は上の定義を使います。

### 定義のフィールドや最終結果が合わない

`kind` と `message` は保存フィールド名に使えません。保存フィールドを宣言した場合は、String だけを返しても構築できません。`Self(...)` では宣言したフィールドを不足・重複なく、正しい型で指定します。

```surtr
# NG: message は共通情報なので保存フィールド名にできない
deferror ReservedField(message: Int) { |value: Int| "invalid" }

# NG: 保存フィールドがあるのに String だけを返している
deferror MissingPayload(port: Int) { |input: Int| "invalid port" }

# NG: port が指定されていない
deferror MissingField(port: Int) { |input: Int| Self(message: "invalid port") }

# NG: port は Int であり、String を保存できない
deferror WrongFieldType(port: Int) {
  |input: Int|
  Self(message: "invalid port", port: "zero")
}
```

`Self(...)` を一度変数へ代入し、その変数を末尾で返す形も拒否されます。

```surtr
# NG: 内部構築が末尾の直接式ではない
deferror StoredConstruction(port: Int) {
  |input: Int|
  saved = Self(message: "invalid port", port: input)
  saved
}
```

### コンストラクタの入力指定が合わない

生成時は、保存フィールド名ではなく本体の入力名・型に合わせます。引数の不足・未知名・重複・型不一致を拒否します。

```surtr
InvalidPort()                  # NG: input が不足
InvalidPort(input: "zero")     # NG: input は Int
InvalidPort(port: 0)           # NG: 入力名は port ではなく input
InvalidPort(input: 0, input: 1) # NG: input が重複
PairError(a: 1, 2)             # NG: 名前指定の残りを位置で推測しない
```

### 照合せずに保存フィールドを読む・具象型を付ける

`err.message` と `err.kind` は共通 `Error` でも読めます。保存フィールドを読むには、先に種類を照合する必要があります。具象型の注釈を付けてもダウンキャストにはなりません。

```surtr
err = InvalidPort(0)
err.message                   # OK
err.port                      # NG: まだ共通 Error
typed: InvalidPort = err       # NG: 型注釈によるダウンキャスト
```

### Error のパスを更新する・具象パスをキャプチャする

Error の共通情報と保存フィールドは読み取り専用です。共通 root の読み取りキャプチャは使えますが、具象 Error を root にしたパスはキャプチャできません。

```surtr
read: (Error -> String) = &Error.message # OK
read_message = &InvalidPort.message     # NG: 具象 Error root
read_port = &InvalidPort.port           # NG: 具象 Error root
```

```surtr
err = InvalidPort(0)
match err {
  InvalidPort @ e => Facet::set(InvalidPort.port, e, 1), # NG: readonly
  _ => err,
}
```

保存フィールドを読んだ値自体は通常値です。元の Error を変更せず、その値を別の処理へ渡すことはできます。

### 許可されていない位置で Error Pattern を使う

Error 定義 Pattern を通常の `=`、SafeBind `=?`、do の `<-`、`apply_pattern` に使うことはできません。種類の分岐には `match`、`if_let`、`if_let_then` を使います。

```surtr
InvalidPort(port) = InvalidPort(0)       # NG: 通常の束縛
InvalidPort(port) =? Ok(InvalidPort(0))  # NG: SafeBind での直接照合
is_match(InvalidPort(0), InvalidPort(port)) # NG: is_match は束縛を作らない
is_match(InvalidPort(0), InvalidPort @ e)  # NG: is_match は alias を作らない
```

## Result の variant と型推論

`Ok` と `Err` は、標準 Enum `Result` の variant を参照する予約 alias です。
通常の Enum と同じ呼び出し・capture・Pattern を使います。

```surtr
value: Result<Int> = Ok(1)
failed: Result<Int> = Err(NoneError)
wrap: (Int -> Result<Int>) = &Ok
```

成功型は payload と期待型から推論できます。`value: Result<Int> = Ok("text")` は型不一致で拒否されます。
`err = Err(NoneError)` のような失敗値は成功型の多相性を保持します。
具象 error constructor と `Err` は通常の capture を使えます。詳細は [constructor capture](./capture-operator.md#result-と-boolean) を参照してください。

## `Result` が標準、`Option` は別コンテナ

Surtr では optional value も、まず `Result` で扱うのが基本です。  
特に「値がない」を recoverable failure として扱うときは `Err(NoneError)` を使います。

```surtr
def first_or_error(xs: List<Int>) -> Result<Int> {
  List::first(xs)
}
```

この種の API は、利用者視点では `Option<T>` ではなく
`Result<T, ListFirstEmpty>` を返す失敗 API として読むのが自然です。

```surtr
match List::first([10, 20, 30]) {
  Ok(value) => to_string(value),
  Err(ListFirstEmpty) => "empty",
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
そのため `Error::kind(err)` や `Error::format(err)` のような共通 helper で観測でき、`Result::map_err(..., err)` や `require(..., err)` のような標準 helper へそのまま渡せます。

### `Result` の等価性

`Result<T>` の `Eq` は `T: Eq` を要求します。`Ok` 同士は成功値の `Eq` を使い、`Ok` と `Err` は異なります。`Err` 同士は先頭の具象 error kind だけを比較し、message、保存した値、cause、発生位置や診断情報は比較しません。`Error::same_kind` はこの kind 判定用の helper であり、`Error` 自体の `Eq` や `Show` を提供するものではありません。

`inspect(result)` は表示の観測です。等価性を検査するときは `Eq::eq`、表示そのものを検査するときは `inspect` の戻り値を比較します。

## 文末の `?` で検証を続ける

途中の処理に `?` を付けると、`Err` の場合はそこで早期リターンし、成功した場合は
値を捨てて次の文へ進みます。最後の処理には `?` を付けず、その Result を返します。

```surtr
deferror OutOfRange { "value must be between 0 and 9" }

def check_value(value: Int) -> Result<()> {
  require(value >= 0, OutOfRange)?
  require(value < 10, OutOfRange)
}
```

`?` は成功型をたどった終端が Unit の Result に使えます。
`Result<()>` と `Result<Result<()>>` は使えますが、`Result<Int>`、
`Result<Result<Int>>`、Unit、Option、List、成功型が未確定の Result には使えません。
型 alias にも同じ規則を適用します。

`EXPR?` 自体の型は Unit です。Result を返す関数や `it` の最後に `?` を付けると、
必要な Result と Unit が一致しないため型エラーになります。最後の処理は Result のまま返してください。
複数行の呼び出しでは、閉じ括弧の後に `?` を付けられます。

`?` は独立した文に付けます。`value = operation()?`、`consume(operation()?)`、
`operation()? + 1`、`operation()??` は使えません。
成功値を取り出す場合は次節の `value =? operation()` を使います。
型位置の `Ty?` は `Option<Ty>` を表します。Boolean 関数名の `predicate?` も文末アンラップとは別の構文です。FacetPath の optional segment は廃止されているため使えません。

ネストした Result では外側一段だけを検査します。`Result<Result<()>>` の
`Ok(Err(error))` は外側が Ok なので、内側の値を捨てて次へ進みます。
内側の Err も検査する場合は、`=?` で内側の Result を取り出して検査してください。

`do` の外で `?` を使う関数やクロージャは、`MonadFail` を実装した型を返す必要があります。
クロージャの場合も、そのクロージャ自身の返り型が型注釈や呼び出し先の引数型から決まっている必要があります。
`do` の中で失敗した場合は、その `do` の処理を止めます。詳しくは [Monad の逐次処理](./do.md)を参照してください。
[テストを書く](./test.md)では、複数のアサーションを並べる例を紹介しています。

## `=?` SafeBind と早期リターン

`=?` は右辺の `Result` から成功値を取り出し、失敗時はその場で返す束縛です。
関数やクロージャの返り型には `MonadFail` が必要です。`Result`、`Either<Error, A>`、
`ResultT` などが対応しており、失敗時は元の Error を `MonadFail::fail` に渡します。

```surtr
def parse_and_increment(text: String) -> Result<Int> {
  value: Int =? try_to::<Int>(text)
  Ok(value + 1)
}
```

これは次の `match` 展開として読めます。

```surtr
def parse_and_increment(text: String) -> Result<Int> {
  parsed = try_to::<Int>(text)
  match parsed {
    Ok(value) => Ok(value + 1),
    _ => parsed,
  }
}
```

SafeBind は複数段にも使えます。

```surtr
def load_pair(a: String, b: String) -> Result<Int> {
  left: Int =? try_to::<Int>(a)
  right: Int =? try_to::<Int>(b)
  Div::safe_div(left + right, 2)
}
```

SafeBind が自動分解する RHS は canonical `Result` の外側一段だけです。Result 以外の値は、
partial pattern が値全体を明示的に検査するときだけ利用できます。

右辺からの取り出しと、失敗を返す型は別の規則です。たとえば `ResultT` を返す関数でも、
右辺の `ResultT` を暗黙に取り出すことはありません。`run` で明示的に取り出します。

SafeBind は右辺の `Err(error)`、パターンの不一致、Extractor の失敗を返り先へ渡します。
`ResultT<Result, A>` では内側に Error を保持するため、`run` の結果は `Ok(Err(error))` です。
`Error` 自体は通常値なので、`value =? Ok(error)` は Error を value に束縛して続行します。

エラー表示の主キャプションは、Error を生成した位置を指します。明示的に Error を作った場合は
その構築式、パターンの不一致は失敗した子パターンを表示します。list の長さなど構造自体が
一致しなかった場合は、その構造パターン全体を表示します。
関数や Extractor から返した Error も、SafeBind で伝播しても生成位置を保持します。
新しい Error で wrap すると、その新しい Error の構築位置を表示し、元 Error は cause に残ります。
呼び出し経路はスタックトレースで確認できます。

`do` 内では `MonadFail` があれば `MonadFail::fail(error)` で元の Error を保持し、なければ `Alternative::empty()` を使います。
partial `<-` のパターン不一致も同じ規則です。どちらの能力もなければコンパイルエラーになります。
常に一致するパターンの `<-` は `Monad` だけで利用できます。
`do` の外で使う SafeBind は、その関数やクロージャ自身の返り型に `MonadFail` を要求します。

`OptionT<Result, A>` のパターン不一致や SafeBind の失敗は `Ok(None)` になります。
一方、`<-` の右辺にある base の `Err(error)` は bind がそのまま保持します。
`guard` は常に `Alternative` の操作です。`Alternative::guard(False)` も `OptionT` では `Ok(None)` になります。

```surtr
Option::Some(value) =? Option::Some(1) # value は Int
```

`value =? Option::Some(1)` や `value =? 1` のような total pattern + non-Result RHS は
compile errorです。Optionや他のMonadからpayloadを暗黙に取り出す規則はありません。
また、`Ok(inner) =? Ok(Err(error))` は内側のErrを再伝播せず、通常のconstructor不一致になります。

概念的には左から順に `match` が入れ子になります。

```surtr
def load_pair(a: String, b: String) -> Result<Int> {
  left_result = try_to::<Int>(a)
  match left_result {
    Ok(left) => {
      right_result = try_to::<Int>(b)
      match right_result {
      Ok(right) => Div::safe_div(left + right, 2),
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

### `|*|` Applicative ap

`Result` では、成功値を保持した function と成功値を保持した引数を
組み合わせます。`Err` が含まれる場合は `Result` の失敗伝播規則に従います。

```surtr
Ok(&inc) |*| Ok(1)
Ok(curry(&Add::add)) |*| Ok(1) |*| Ok(2)
```

### `|>=` bind

成功値を次の `Result` 返却関数へ渡します。

```surtr
try_to::<Int>("42") |>= require_at_least(10)
```

`match` へ読み下すと次です。

```surtr
parsed = try_to::<Int>("42")
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
  match try_to::<Int>(text) {
    Ok(value) => value,
    Err(NoneError) => 0,
    Err(_) => 0,
  }
}
```

### 別の `Result` へ回復する

```surtr
def parse_or_zero(text: String) -> Result<Int> {
  parsed = try_to::<Int>(text)
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
  match try_to::<Int>(text) {
    Ok(value) => if(value > 0, Ok(value), Err(InvalidPort(value))),
    Err(_) => Err(InvalidPort(-1)),
  }
}
```

## 使い分けの目安

複数のMonad値を順に扱う場合は[do](./do.md)を使います。MonadFail を実装した型ではErrorを保持し、
MonadFailのないAlternative carrierではfailure matcher / SafeBindのErrorを破棄してそのcarrierのemptyになります。

- 分岐を明示したいときは `match`
- 失敗をそのまま流したいときは `=?`
- 直線的な pipeline は `|*>`, `|*|`, `|>=`, `>*`, `>=>`
- recover したいときは `Err(...)` branch を明示的に書く

## 関連ページ

- `match` の基本は `./pattern-matching.md`
- 演算子全体の制約は `./language-reference.md`
- 標準 `Result` / `Option` の位置づけは `./standard-library.md`
