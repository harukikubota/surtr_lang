# Monad の利用と実装

Monad は、成功値に次の計算をつなぐためのトレイトです。Result なら成功側は Ok、
失敗側は Err です。成功値を変換するとき、次の計算も失敗しうるとき、複数の値を
順に扱うときで、使う操作が変わります。

## Result から始める

```surtr
def increment(value: Int) -> Int { value + 1 }
def require_small(value: Int) -> Result<Int> {
  if(value < 100, Ok(value), Err(NoneError()))
}

mapped = Ok(20) |*> &increment       # Ok(21)
combined = Ok(&increment) |*| Ok(20) # Ok(21)
checked = Ok(20) |>= &require_small  # Ok(20)
```

| トレイト | 操作 | 役割 |
|---|---|---|
| Functor | `fmap`、`|*>` | 成功値に通常の関数を適用し、結果を同じ文脈に入れる |
| Applicative | `pure`、`ap`、`|*|` | 通常値を包み、包まれた関数を同じ文脈の引数へ適用する |
| Monad | `return`、`bind`、`|>=` | 成功値を、同じ文脈の値を返す次の関数へ渡す |

Result の各操作では Err のとき mapper を呼びません。`|*>` の mapper が Result を
返すと `Ok(Ok(value))` のように入れ子になります。`|>=` は mapper の Result をそのまま返します。
複数引数の関数を `|*|` で使うときは curry を明示します。

```surtr
sum = Ok(curry(&Add::add)) |*| Ok(20) |*| Ok(22) # Ok(42)
```

関数自体をつなぐときは `>>`、`>*`、`>=>` を使います。

| 構文 | 接続 |
|---|---|
| `value |> fun(...)` | 値を呼出しの第一引数へ渡す |
| `&f >> &g` | 通常の関数を合成する |
| `&f >* &g` | 文脈付きの結果を返す f の成功値を通常の g で変換する |
| `&f >=> &g` | 同じ文脈の値を返す関数どうしを bind でつなぐ |

```surtr
def parse_int(text: String) -> Result<Int> { try_to::<Int>(text) }
pipeline = &parse_int >=> &require_small >* &to_string
rendered = pipeline("42") # Ok("42")
```

各演算子の引数、優先順位、capture の制約は[関数演算子](./function-operators.md)を参照してください。
Result の Error の生成・保持・回復は[エラーハンドリング](./error-handling.md)で説明しています。

## 成功値の型位置

Functor は `Self: Type<$A>` によって、文脈の中で変化する成功値の型を一つ定めます。
型引数が複数ある型では、どれが成功値なのかを impl の slot mapping で明示します。
Applicative と Monad はその対応を親トレイトから継承します。

`MyEither<L, R>` を例にします。L は計算を通して固定する capture、R は成功値です。
以下の宣言と impl は一つの `.srt` ファイルへ順に置いて実行できます。

```surtr
defenum MyEither<$L, $R> {
  Left($L),
  Right($R),
}
```

### Functor

`$R: Functor.$A` が R を成功値の型位置へ対応させます。fmap は Right にだけ mapper を適用し、
Left の値と L の型を保持します。コンパイラが「最後の型引数」を成功値と推測する規則ではありません。

```surtr
impl Functor for MyEither<$L, $R>
where
  $R: Functor.$A
{
  def fmap(self: MyEither<$L, $A>, mapper: ($A -> $B)) -> MyEither<$L, $B> {
    match self {
      MyEither::Left(left) => MyEither::Left(left),
      MyEither::Right(value) => MyEither::Right(mapper(value)),
    }
  }
}
```

### Applicative と Monad

pure と return は Right を作ります。ap は包まれた関数を適用し、bind は Right の値を
次の計算へ渡します。これらは L が Error でなくても使えます。

```surtr
impl Applicative for MyEither<$L, $R> {
  def pure::<MyEither<$L, $R>>(value: $A) -> MyEither<$L, $A> {
    MyEither::Right(value)
  }

  def ap(mapper: MyEither<$L, ($A -> $B)>, value: MyEither<$L, $A>) -> MyEither<$L, $B> {
    match mapper {
      MyEither::Left(left) => MyEither::Left(left),
      MyEither::Right(fun) => Functor::fmap(value, fun),
    }
  }
}

impl Monad for MyEither<$L, $R> {
  def return::<MyEither<$L, $R>>(value: $A) -> MyEither<$L, $A> {
    MyEither::Right(value)
  }

  def bind(self: MyEither<$L, $A>, mapper: ($A -> MyEither<$L, $B>)) -> MyEither<$L, $B> {
    match self {
      MyEither::Left(left) => MyEither::Left(left),
      MyEither::Right(value) => mapper(value),
    }
  }
}
```

```surtr
mapped: MyEither<String, String> = MyEither<String, Int>::Right(12) |*> &to_string
# MyEither::Right("12")
```

## 成功値を取り出す MonadExtractor

MonadExtractor は Monad を親に持ち、成功値を `Result<A>` へ取り出します。
Monad の実装から自動導出される能力ではありません。

```surtr
deftrait MonadExtractor
where
  Self: Monad
{
  def extract(self: Self<$A>) -> Result<$A>
}
```

この宣言は標準定義に含まれるため、利用者が再宣言する必要はありません。
標準実装は次の3型です。

| 入力 | extract の結果 |
|---|---|
| Result の Ok／Err | 入力をそのまま返す |
| Option の Some／None | Ok(value)／Err(NoneError()) |
| Either<Error, A> の Right／Left | Ok(value)／Err(error) |

NoneError は Option の extract 本体で通常どおり生成します。
List、Identity、Reader、State、各 Transformer にこの標準実装はありません。
MonadFail を実装している型にも自動では付与されません。

MyEither の実装例では L を Error に固定します。

```surtr
impl MonadExtractor for MyEither<Error, $R> {
  def extract(self: MyEither<Error, $A>) -> Result<$A> {
    match self {
      MyEither::Left(error) => Err(error),
      MyEither::Right(value) => Ok(value),
    }
  }
}
```

### Error を取り出す

`MonadExtractor::extract_error(value)` は extract を一度呼び、失敗側の Error を
`Ok(error)` として取り出します。成功側なら `Err(ErrorExtractionFailed())` を返します。

```surtr
checked = MonadExtractor::extract_error(Option<Int>::None)
# checked は NoneError を保持する Ok(error)
```

取り出すのは一段だけです。`Ok(Err(error))` は外側が成功なので取り出しに失敗します。
内側の失敗を調べるときは、対象の層を明示的に畳み込みます。

```surtr
nested: Result<Result<Int>> = Ok(Err(NoneError()))
flattened: Result<Int> = Result::flatten(nested)
checked: Result<Error> = MonadExtractor::extract_error(flattened)
# checked は内側の NoneError を保持する Ok(error)
```

Test モジュールの `expect_error` も同じ一段の取り出しを行い、失敗時には元入力を表示します。
テストで Payload を検証する例は[テスト](./test.md)を参照してください。

### 失敗を受け取る MonadFail

MonadFail は Error を受け取って失敗値を作り、元 Error を保持する能力です。
RHS から取り出す MonadExtractor とは別に実装します。

```surtr
impl MonadFail for MyEither<Error, $R> {
  def fail::<MyEither<Error, $R>>(error: Error) -> MyEither<Error, $R> {
    MyEither::Left(error)
  }
}
```

この例で2つの能力を Error 固定にしたことは、言語上の Left 型の制限ではありません。
例えば `MyEither<String, R>` の extract で String を message とする具体 Error を作ったり、
`MyEither<Int, R>` で数値に応じた具体 Error を作ったりできます。
どの Error を生成するかは実装者が定め、Right はそのまま Ok へ渡します。
任意の L が自動的に取り出せるわけではなく、通常の coherence に従う明示 impl が必要です。

Left から Error を生成できても、Error を受け取って失敗値を構築する逆向きの能力は得られません。
MonadFail にも、その Error を保持する契約を満たす実装が必要です。

### Convert は別の契約

`Convert<Result<A>>` は MonadExtractor から自動実装されません。
標準3型も型別の明示 impl を持ちます。ユーザー型で処理を共有するなら、片方向に委譲できます。

```surtr
impl Convert<Result<$A>> for MyEither<Error, $A> {
  def to::<Result<$A>>(self: Self) -> Result<$A> {
    MonadExtractor::extract(self)
  }
}

```

```surtr
converted = to::<Result>(MyEither<Error, Int>::Right(3)) # Ok(3)
```

## SafeBind

`pattern =? rhs` は RHS の型から取り出し方を決めます。MonadExtractor があれば、次の順で処理します。

1. RHS を一度評価し、解決した extract を一度呼ぶ。
2. 返された Result が Err なら、その Error を現在の失敗返却先へ渡して処理を止める。
3. Ok なら、その成功値全体を左辺の Pattern へ渡す。
4. Pattern が一致すれば束縛して続行し、不一致ならその Pattern の Error を失敗返却先へ渡す。

```surtr
def option_value(target: Option<Int>) -> Result<Int> {
  value =? target
  Ok(value)
}

def my_value(target: MyEither<Error, Int>) -> Result<Int> {
  value =? target
  Ok(value)
}
```

構文の利用に `import MonadExtractor` は不要です。
取り出しは一段です。成功値が別の MonadExtractor 実装型でも、子 Pattern 内でも、再分解しません。
左辺の形や型検査の結果によって、元の RHS 全体を照合する経路へ切り替えることもありません。

```surtr
def nested_value(target: Option<Result<Int>>) -> Result<Result<Int>> {
  inner: Result<Int> =? target
  Ok(inner)
}
```

`Option::Some(value) =? Option::Some(1)` は取り出し後の Int に Option Pattern を適用するため型エラーです。
`Option::Some(value) =? Option::Some(Option::Some(1))` なら内側の Option を照合します。
値全体を照合するときは match、apply_pattern、または外側の Result を明示します。

```surtr
def option_whole(target: Option<Int>) -> Result<Int> {
  Option::Some(value) =? Ok(target)
  Ok(value)
}
```

未実装が確定した型は、その値全体を受け取れる partial Pattern で照合できます。
例えば `[head, ..tail] =? values` は List 全体を照合します。
全体を受け取る total Pattern は `=` を使います。Monad の payload を求める入力なら
MonadExtractor の不足を案内します。これは分解の入口の案内であり、実装後に子 Pattern が
必ず型検査を通る、または実行時に一致するという保証ではありません。
分解後の Pattern や子だけの不一致は通常の型・Pattern エラーになります。

総称型では、宣言時に bound を要求します。呼出しの具体型に実装があっても、
`$F: Monad` だけでは分解できません。

```surtr
def generic_value(target: $F<$T>) -> Result<$T>
where
  $F: MonadExtractor
{
  value =? target
  Ok(value)
}
```

### 失敗返却先と do

通常の関数・Closure は、自身の返り型の MonadFail を使います。
Extractor／ExtractorClosure 本文では自身の MatchResult の失敗にし、元 Error を保持します。
do 内は do-local な MonadFail を優先し、なければ Alternative の empty で Error を捨てます。
どちらもない場合は型エラーです。内側の callable や do は外側の失敗返却先を借りません。
失敗返却先は現在の context から決まり、SafeBind の RHS や carrier の payload に含まれる Result からは選びません。
`List<Result<Unit>>` でも carrier は List なので、その do の失敗処理は Alternative を使います。
Error の保持と値としての収集の使い分けは [do](./do.md#alternative実装型) を参照してください。

```surtr
def my_failure_target(target: Option<Int>) -> MyEither<Error, Int> {
  value =? target
  MyEither::Right(value)
}

sequenced: MyEither<Error, Int> = do::<MyEither<Error, _>> {
  value <- MyEither::Right(3)
  MyEither::Right(value + 1)
}
```

`<-` は Monad::bind が渡した payload 全体を Pattern に渡します。
payload に MonadExtractor があっても extract を呼びません。

```surtr
selected: List<Int> = do::<List> {
  Ok(value) <- [Ok(1), Err(NoneError())]
  [value]
}
selected # [1]
```

SafeBind の RHS だけから do の carrier を推論することもありません。
sequence、scope、carrier 推論の詳しい規則は[do](./do.md)を参照してください。

## 文末の `?`

`expression?` は成功値が Unit に至るまで分解し、成功なら次の文へ進み、失敗なら Error を現在の失敗返却先へ渡します。異なる MonadExtractor 実装型が入れ子になっていても使えます。文の型は Unit です。

入力から成功値の型を順にたどり、各層に MonadExtractor があり、有限段で Unit に至ることを静的に確認します。総称型では各層に必要な bound を宣言します。未確定の成功型を Unit と推論して受理することはありません。

| 元の入力型 | 利用 |
|---|---|
| Result<Unit>、Result<Result<Unit>> | 可 |
| Option<Unit>、Either<Error, Unit>、対応するユーザー型 M<Unit> | 可 |
| Option<Option<Unit>>、Option<Result<Unit>>、Either<Error, Either<Error, Unit>> | 可 |
| Result<Option<Either<Error, Unit>>> | 可 |
| M<Int>、Result<Option<Int>> | 不可 |
| 成功型の列は Unit に至るが、内側を含め MonadExtractor がない Monad 層を持つ型 | 実装不足 |
| 成功型が未確定、または循環して Unit に至らない型 | 不可 |
| Unit | 不可 |

RHS は一度評価します。到達した各層の extract も一度だけ呼び、その返却 Result を一段判定します。Err なら元 Error を同じ失敗返却先へ渡し、内側の extract と後続を実行しません。Ok の payload が Unit なら続行し、それ以外ならその payload に次の extract を適用します。成功値の保存フィールドや container を探索する規則ではありません。

```surtr
def check_present(target: Option<Unit>) -> Result<Int> {
  target?
  Ok(7)
}

def keep_inner_error() -> Result<Int> {
  nested: Result<Option<Result<Unit>>> = Ok(Option::Some(Err(NoneError())))
  nested?
  Ok(7)
}
```

`keep_inner_error()` は `Err(NoneError("None Value."))` です。外側の Result、内側の Option、最内の Result の順に extract を呼び、最後の Err を伝播します。全層が成功して Unit に到達した場合だけ後続へ進みます。例えば `Ok(Option::Some(Ok(())))` は成功です。
通常の SafeBind `=?` と明示的な extract は一段だけを分解するため、この再帰的な確認は文末の `?` に限られます。

`?` は独立した文の式全体に付けます。`value = operation()?`、`consume(operation()?)`、
`operation()? + 1`、`operation()??` は使えません。必要な戻り値が Result なら、
最後の式は `?` を付けずに返します。Unit を最終 Monad 値へ暗黙に包む規則はありません。
型位置の `T?`、Boolean 関数名の `predicate?`、FacetPath はそれぞれ別の構文です。
