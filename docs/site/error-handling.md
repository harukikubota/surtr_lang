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

## Error の構築と種類による回復

裸の `deferror` 名は、種類を表す `ErrorKind` 値です。Error を作るには明示的に呼び出します。入力がない Error も `NoneError()` と書きます。

```surtr
kind = NoneError
error = NoneError()
Result::recover_kind(Err(error), kind, {|original| Ok(original.message)})
```

ErrorKind は変数、引数、戻り値、List、Tuple、field に保持できます。`Error::is_kind(error, kind)` は種類の一致を Boolean で返します。文字列や Error instance から ErrorKind を作る API はありません。

`Result::recover(value, {|error| ...})` は失敗時に元の Error をハンドラへ渡します。Error が不要なら `{|_| ...}` を使います。成功値はそのまま返り、ハンドラの再失敗もそのまま返ります。`recover_kind` は種類が一致した失敗だけを回復し、不一致なら元の Error とその情報を保持します。両関数とも引数式は呼出し前に評価し、ハンドラ本体だけを条件付きで実行します。Result はこの能力を `MonadRecover` としても提供します。

SafeBind で種類の判定を失敗条件にすると、元の Error が失われることがあります。たとえば `True =? Error::is_kind(error, kind)` は False のとき `BooleanLiteralPatternMismatch` を返し、元の Error を再伝播しません。種類による回復には `recover_kind`、元の Error を保持する分岐には `match` を使います。Error の Pattern を SafeBind へ移しても、通常の型・Pattern 規則に従うため、種類による回復の代わりにはなりません。

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

具象エラー定義の保存フィールドとメッセージコンストラクタ引数には、具体型を指定します。`$T` や `List<$T>` など、型変数を含む型では宣言できません。`List<Int>` のように型引数が具体化済みの型は使えます。総称値を表示・加工したい場合は、呼び出し元で処理してから具体型の値を渡します。

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
failed: Result<Int> = Err(NoneError())
wrap: (Int -> Result<Int>) = &Ok
```

成功型は payload と期待型から推論できます。`value: Result<Int> = Ok("text")` は型不一致で拒否されます。
`err = Err(NoneError())` のような失敗値は成功型の多相性を保持します。
具象 error constructor と `Err` は通常の capture を使えます。詳細は [constructor capture](./capture-operator.md#result-と-boolean) を参照してください。

## `Result` が標準、`Option` は別コンテナ

Surtr では optional value も、まず `Result` で扱うのが基本です。  
特に「値がない」を recoverable failure として扱うときは `Err(NoneError())` を使います。

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
    _ => Err(NoneError()),
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

## 元 Error を伝播する

SafeBind `=?` では、RHS（右辺）と LHS（左辺）がそれぞれ独立して失敗を返します。

| 失敗する側 | Error の発生条件 |
|---|---|
| RHS | RHS の型の `impl MonadExtractor` が、成功値を取り出す際に失敗を返す |
| LHS | failurePattern（一致しない可能性のある Pattern）が不一致になる、または適用した Extractor が失敗を返す |

RHS が失敗した場合、LHS は照合しません。RHS が成功した場合でも、
その成功値を照合する LHS は独立して失敗し得ます。
どちらの Error も現在の失敗返却先へ渡されます。
通常の関数や Closure では、自身の返り型の `MonadFail` が受け取ります。

RHS で使える標準の `MonadExtractor` インスタンスは次の3型です。

| RHS の型 | 成功値 | 失敗時の Error |
|---|---|---|
| `Result<A>` | `Ok(value)` の value | `Err(error)` の元 Error |
| `Option<A>` | `Some(value)` の value | `None` に対する `NoneError` |
| `Either<Error, A>` | `Right(value)` の value | `Left(error)` の元 Error |

RHS の `MonadExtractor` と、失敗返却先の `MonadFail` は別の能力です。

```surtr
def require_one(text: String) -> Result<Int> {
  1 =? try_to::<Int>(text)
  Ok(1)
}
```

`require_one("invalid")` は RHS から数値変換の Error を返します。
`require_one("2")` は RHS が成功した後、LHS の `1` との不一致による
`IntLiteralPatternMismatch` を返します。`require_one("1")` は `Ok(1)` です。

LHS に Extractor を使う場合も同じです。

```surtr
def require_head(target: Result<List<Int>>) -> Result<Int> {
  uncons(head, _) =? target
  Ok(head)
}
```

`require_head(Err(NoneError()))` は RHS の元 Error を返します。
`require_head(Ok([]))` は RHS が成功した後、LHS の `uncons` による
`UnconsEmptyList` を返します。

`Error` 自体は通常値なので、`value =? Ok(error)` は Error を束縛して続行します。
SafeBind は成功値の中にある Error を失敗へ変えません。

エラー表示の主キャプションは Error を生成した位置を指します。
明示的な Error はその構築式、Pattern 不一致は失敗した子 Pattern、構造自体の
不一致はその構造 Pattern 全体を表示します。関数や Extractor から返した Error も、
SafeBind による伝播では生成位置を保持します。新しい Error で wrap すると、
その構築位置が表示され、元 Error は cause に残ります。

文末の `?` は、成功値が Unit に至るまで各層の `MonadExtractor::extract` を呼びます。
`Result<Option<Unit>>` や `Option<Result<Unit>>` のように異なる型が入れ子の場合も使えます。
各層の能力と、有限段で Unit に至ることは静的に確認します。
`Result<Int>`、未確定の成功型、Unit 自体には使えません。

RHS と到達した各層の extract は一度だけ評価します。どの層でも extract が Err を返したら、
その元 Error を同じ現在の失敗返却先へ渡し、内側の分解と後続を実行しません。
全層が成功して Unit に達した場合だけ次の文へ進みます。
例えば `Ok(Option<Unit>::None)` に `?` を付けると、内側の Option が生成した NoneError を伝播します。
通常の SafeBind `=?` と明示的な extract は一段の分解を維持します。最後の Result は `?` を付けずに返します。

```surtr
deferror OutOfRange { "value must be between 0 and 9" }

def check_value(value: Int) -> Result<()> {
  require(value >= 0, OutOfRange())?
  require(value < 10, OutOfRange())
}
```

共通の型条件、一段分解、未対応型の診断、関数演算子の使い方は
[Monad](./monad.md)を参照してください。do 内の失敗返却先は[do](./do.md)、
複数の検証を並べる例は[テストを書く](./test.md)で説明しています。

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
MonadFail のない Alternative carrier では、partial `<-`、SafeBind、文末 `?` の Error を破棄して、その carrier の empty になります。
この失敗処理先は carrier から一意に決まり、payload の型では切り替わりません。
たとえば `List<Result<Unit>>` でも、これらの失敗は要素の Err にはならず、その経路の空 List になります。
Result を値として List に集める用途では、通常の List 構築や map などを使えます。

Pattern の失敗を呼び出し先で処理し、同じ do-carrier 型の値として返すなら、
total `<-` や bare Monad 式でそのまま接続できます。do はその値を empty に上書きしません。
この接続自体に Alternative や MonadFail は不要で、Monad だけを要求します。

- 分岐を明示したいときは `match`
- 現在の失敗処理先に従って短絡したいときは `=?`。元 Error を保持したい場合は、Result などの MonadFail を持つ型を失敗処理先にする
- 直線的な pipeline は `|*>`, `|*|`, `|>=`, `>*`, `>=>`
- recover したいときは `Err(...)` branch を明示的に書く

## 関連ページ

- `match` の基本は `./pattern-matching.md`
- 演算子全体の制約は `./language-reference.md`
- 標準 `Result` / `Option` の位置づけは `./standard-library.md`
