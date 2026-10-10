# Surtr Language Reference

このページは、現時点で確定している Surtr の言語仕様をコンパクトに引けるようにまとめたものです。

## 1. 基本構文

### 束縛

```surtr
name = expr
name: Ty = expr
name =? expr
operation()?
```

- `expr;` はその式を `Unit` として扱う
- `;` は改行区切りと同様に扱われ、同じ行で次の式を書ける
- `=` と `=?` 自体の結果型も `Unit`
- 文末の `?` は独立した文の式全体に付ける。canonical `Result` の成功型をたどった終端が Unit のときだけ使え、成功値を捨てて外側一段の Err を SafeBind と同じ返却先へ伝播する。文自体の型は Unit なので、Result が必要な戻り値位置に置くと通常の型不一致になる
- 文末の `?` は式の一部に組み込めず、成功時の型は Unit。詳しくは[エラーハンドリング](./error-handling.md)を参照
- `Unit` を返す closure が期待される場所では、最後の式に `;` を付ければよい

Tuple の番号 selector（`tuple._0?` など）の末尾の `?` は、従来どおり文末アンラップとして扱います。番号 selector は廃止済み OptionalSelector の対象には含めません。

### `const`

```surtr
public const DEFAULT_PORT: Int = 8080
private const PROFILE_NAME = User.profile -> Profile.name
```

- file top-level にだけ宣言でき、visibility の既定は `public`
- 名前は `[A-Z][A-Z0-9_]*`。先頭・末尾 `_` と `__` は使えない
- public const は compile unit 全体、private const は宣言 file だけから参照できる
- 値は primitive literal、Facet path、別の Facet const、またはそれらの `->` 合成に限定する
- const Facet の bracket segment は literal `Int` / `String`、または両端が literal `Int` の range だけを受け付ける

### 関数

```surtr
def name(args...) -> Ty { expr }
```

Boolean を返す関数は `def positive?(value: Int) -> Boolean { value > 0 }` のように名前の末尾へ隣接する `?` を1個付けられます。`positive?(1)` で呼び出し、`&positive?` で capture します。未確定の返り型や `Result<Boolean>`、`Option<Boolean>` は対象外です。変数・引数・Pattern 束縛・フィールド・型・モジュール・Extractor の名前には付けられません。

suffix call は LHS の Extractor にはなりません。`positive?(1)?` の最後の `?` は既存の文末アンラップとして読み、Boolean に対する型エラーになります。型位置の `Ty?` は `Option<Ty>` のままです。`EXPR . Identity ?` は廃止済み OptionalSelector として拒否します。

Surtr では、関数はすべて明示または暗黙の namespace に属します。

- 通常の module 関数は `defmod Name { ... }` に置く
- 型付属関数は `impl Type { ... }` に置く
- trait 契約は `deftrait Name { ... }` に置く
- trait 実装は `impl Trait for Type { ... }` に置く
- script / REPL の top-level `def` は暗黙の擬似 module に属する

### データ定義

```surtr
defstruct Name {
  field: Ty,
}

defstruct Wrapped<$M, $A>
where
  $M: Monad
{
  value: $M<$A>,
}

defrecord Name(field: Ty, ...) # 1個以上の public field。可視性指定は不可

deferror Name(field: Ty) { |input: Ty| Self(message: "message", field: input) }

defenum Name { Variant, Variant(Ty), Variant = Int, ... }

deftrait Show {
  def to_string(self: Self) -> String
}

impl Type {
  def method(...) -> ... { ... }
}

impl Show for Int {
  def to_string(self: Self) -> String { inspect(self) }
}
```

失敗を Error として保持する型は `MonadFail` を実装します。`Result`、`Either<Error, A>`、
`ResultT` などが標準で対応します。構造体の field の形から失敗処理を推測する規則はありません。

### 制御構造

```surtr
if(cond, then_expr, else_expr)
if_then(cond, expr)

match expr {
  pattern => expr,
  ...
}

do::<Carrier> {
  pattern <- monadic_expr
  final_monadic_expr
}
```

`match value { ... }`、`match(value) { ... }`、`match(value, { ... })` は同じ意味です。`cond { ... }` と `cond({ ... })` も同じ意味です。括弧内に置いても、`match` の arm と `cond` の条件節は専用のブロック構文であり、通常のブロック値にはなりません。

`do` の carrier は `::<Carrier>` で指定するか、RHS・最終式・期待型から推論します。
`<-` の束縛は後続文だけで見え、最終式は同じ carrier の Monad 値を返します。

## 2. 型

### 基本型

- `Int`
- `Float`
- `String`
- `Boolean`
- `Unit`

### 合成型

- `List<T>`
- `Result<T>`
- `Enum`
- 関数型 `(T1, T2, ...) -> R`
- ユーザ定義型

### `Enum`

- `defenum` で定義する
- 値生成は `Enum::Variant(...)` または `Enum<TypeArgument, ...>::Variant(...)`
- 後者の型引数 arity は enum 宣言と一致させる。各 `_` はその位置だけを payload と expected type から推論し、明示した通常型・scope 内型変数は固定する
- 通常の型引数位置ではTypeConstructor traitを使えない。nominal declaration parameterがTypeCtorTrait constraintを持つ位置だけは、対応する具象constructorのbare headを指定できる。call-site ReturnTypeArgumentでは完全・部分型applicationと`_`もcarrier入力として指定できる
- `Ok(...)` / `Err(...)` は通常 Enum の variant として解決し、Result の型制約と runtime 表現を適用する。成功型を固定したい場合は `failed: Result<Int> = Err(NoneError)` のように型注釈を付ける
- `Enum<...>::method`、struct constructor、型注釈・signature・pattern・impl target の `_` にはこの規則を適用しない
- `match` は網羅必須
- enum 値への field access（例: `.idx`）は不可

### `impl` / `Self` / `self`

- `impl` 対象は `defstruct` / `defenum` のみ
- `impl Type` は型専用の module-like namespace であり、`self` / `Self` が使える `defmod` 相当として読む
- `Self` は `impl` 内の型位置でのみ使用可能
- `self` は `impl` メソッド第一引数専用（再束縛不可）
- メソッド呼び出しの正規形は `Type::method(...)`

### trait (V1)

- trait 宣言は `deftrait Name { ... }`
- trait 宣言は `deftrait Name<$T, ...> { ... }` のように型引数を取ってよい
- trait 実装は `impl Trait for Type { ... }`
- trait 実装は `impl Trait<Concrete, ...> for Type { ... }` の形も取れる
- 同じ Trait の impl は Trait 引数と target 型を再帰 unification して overlap を判定し、交差する場合は宣言順にかかわらず compile error とする。generic は任意の型 pattern と一致し、V1 は specialization 優先順位を持たない
- 同じ nominal target でも full pattern が構造的に disjoint なら併存できる。`where` 制約の違いだけでは disjoint とみなさない
- `defmod` / inherent `impl` / trait `impl` block 内の callable 名は一意であり、signature や `def` / `defp` の違いによる overload はできない
- 通常 callable に一般的な型parameter listはなく、value parameter由来の型スロットを`id::<Int>(1)`のように任意指定できない
- non-intrinsic callableがreturn-only入力を定義側ReturnTypeArgumentsとして宣言した場合は、通常関数・method・Trait helper・captureで対応する`::<Int>`を指定できる。省略時はexpected returnなどから推論し、最後まで決まらなければambiguityになる
- ReturnTypeArguments は、型変数が value parameter の型から導入できない場合にだけ使い、その型変数は戻り値にも現れなければならない。`Eq` の `Self` のように引数位置で導入済みの型変数を同じ型で ReturnTypeArguments に重ねることはエラーであり、`TryConvert<$To>` の `$To` は変換先指定として ReturnTypeArguments に置く。TypeCtorTraitのcall-site RTAでは、constructor head、完全・部分型application、`_`を一項の型入力として扱う。Trait methodの通常RTAも、`to::<Result>(option)`のようにimplのsource/targetが共有する型変数から全引数を解けるgeneric targetだけはbare headで指定できる。通常関数では完全型を使う
- trait は method のみを持つ
- 通常の bound は `$A: Trait` と書く。`Trait<Arg, ...>` は where RHS ではなく trait / impl head または expression dispatch target にだけ書ける
- generic receiver の Trait 呼び出しには、signature 上で宣言した `where` bound が必要である。呼び出しから implicit bound は追加されない
- body を持つ Trait method は default method として override でき、`where Self: Parent` は parent Trait を要求する
- Trait の詳細な利用規則は [`trait-system.md`](./trait-system.md)、実装例は [`trait-impls.md`](./trait-impls.md) を参照する
- 匿名 `impl Trait` 型は使えず、名前付き型変数と `where` clause で制約する
- `where` clause は宣言・trait・impl に制約を追加し、Trait-head / nominal binderとは分離する
- `Self: Type<...>` は trait definition where の `Self` だけ、`Trait.$Slot` は TypeConstructor trait impl の slot map だけで受理する
- constructor application は通常関数／trait method signature の direct parameter・return、またはTypeCtorTrait constraintを持つnominal field/payloadに限る。通常型注釈での部分適用は拒否するが、call-site RTAのcarrier型applicationは許可する。`Self::...` / `Type::...` は value owner path として不正
- `+`, `-`, `*` はそれぞれ `Add::add`, `Sub::sub`, `Mul::mul` へ resolve される
- `Int::abs` / `Float::abs` などの数値 helper は concrete type owner surface として提供する。除算と剰余は `Div::safe_div` / `Mod::safe_mod` のトレイトメソッドを使う
- `Compare` が三値比較の正本で、`< <= > >=` も `Compare` を前提に動く
- `Hole` は compiler-reserved な ignored-input callable marker
- 通常型注釈の `_` は `Hole` の surface 表記だが、call-site ReturnTypeArgumentの`_`は別の推論穴である
- `Hole` / 通常型注釈の`_`は data type wildcard ではなく、限定された callable surface にだけ現れる

### `to` / `try_to`

- source 上の呼び出しは `to::<TargetTy>(value)` / `try_to::<TargetTy>(value)`
- `TargetTy` は明示型引数であり runtime の値引数ではない
- `Convert<$To>` / `TryConvert<$To>` trait が impl coherence を担う
- `Convert` / `TryConvert` の排他は generic 名を alpha-normalize し、target と変換元を再帰照合する
- 利用例とパイプラインは [`Convert / TryConvert`](./traits/convert.md) を参照する

### `Result<T>`

- 成功値: `Ok(value)`
- 失敗値: `Err(error)`

現時点では、`match` を中心に `Ok(...)` / `Err(...)` を扱います。  
variant 判定だけなら `Result::is_ok(...)` / `Result::is_err(...)` も使えます。  
考え方としては `Either<Err, Ok>` に近く、失敗も値として明示的に運びます。
内部表現は enum-like ですが、language surface では `defenum` と区別された専用 abstraction です。

### 戻り値位置の `Result<T, E>`

`Result<T>` が正規表記です。`Result<T, E>` は関数定義の直接の戻り値位置だけで使える補助表記です。

- builtin type declaration の canonical head は `Result<T>`
- `E` は `Err` 側の error contract を説明する補助表記
- 値として保持される型の中心は引き続き `Result<T>`
- 値や引数など、戻り値以外の型注釈では `Result<T>` を使う

### `Error`

- recoverable failure を受ける抽象型
- `deferror` で定義した具体 error がここへ流れ込む
- `Error` 自体をユーザーが直接具体化する前提ではない
- source で `Error` が見えても、runtime 実体は常に具体 `deferror`
- 引数、戻り値、型注釈、field、container、closure で通常値として運べる
- 共通 `kind` / `message` は照合なしで readonly field と Facet として読める。共通情報の path capture は `&Error.kind` / `&Error.message` を使う。具象 Error を root にした path capture は共通情報・保存 Payload のどちらも拒否する
- 保存 Payload は、単一種類への照合が成功した局所束縛と、その束縛を捕捉した通常クロージャから読める。異なる種類の OR 全体の alias、関数・match の戻り値、container から取り出した値は共通 Error なので、Payload を読むには再照合が必要
- `Error(...)` / `&Error` と Error 自体への Trait impl は拒否する。Payload 分解は `match` / `if_let` / `if_let_then` / 束縛なしの `is_match` に限る

## 3. リテラル

### 数値

```surtr
1
10
1.5
```

### 真偽値

```surtr
True
False
```

### 文字列

```surtr
"hello"
"hello #{name}"
'hello #{name}'
"""
multi-line #{name}
"""
```

`"..."` と `'...'` は同じエスケープと補間の規則を使います。

| 入力 | 文字列に入る内容 |
|---|---|
| `\\` | バックスラッシュ |
| `\"` | 二重引用符 |
| `\'` | 単一引用符 |
| `\n` | LF（U+000A） |
| `\t` | TAB（U+0009） |
| `\u{HEX}` | Unicodeスカラー値1文字 |
| `\#{` | 文字としての `#{`。補間を開始しない |

`\u{HEX}` の数字は16進数1〜6桁です。大文字数字と先頭ゼロを受理します。
U+0000〜U+10FFFFのうちサロゲートU+D800〜U+DFFFを除いた値を使えます。
Unicode非文字は受理し、Unicode正規化は行いません。空白、符号、`_`、7桁以上の数字は拒否します。
Unicodeスカラー値を指定する構文なので、UTF-8バイト列を扱う `String::codepoints(..., Utf8)` とは区別します。

```surtr
"\u{1b}a"       # ESCに続くa
'\u{001B}a'     # 同じ値
"\u{3042}"      # あ
"\\u{1b}"       # 文字としての \u{1b}
```

未知のエスケープは解析エラーです。`\q`、`\e`、`\r`、`\0`、`\x1b` は使えません。
CR、NUL、ESCは `\u{d}`、`\u{0}`、`\u{1b}` と書き、文字としてのバックスラッシュは `\\` と書きます。
`"\u{}"`、`"\u{xyz}"`、`"\u{110000}"`、`"\u{d800}"` なども拒否します。

補間は元ソースのエスケープされていない `#{...}` だけで開始します。
`"\#{name}"`、`"\u{23}{name}"`、`"#\u{7b}name}"` は文字としての `#{name}` です。
エスケープで生成した引用符やバックスラッシュも構文として読み直しません。
`"\u{5c}#{name}"` はバックスラッシュ1文字の後で `name` を補間します。
補間式内の文字列リテラルは、それ自身の引用符とエスケープ規則で解析します。
補間値は通常の `Show::to_string` で文字列へ変換します。Show のない型は型検査で拒否し、Result は先に match などで中身を取り出します。式は左から一度ずつ評価します。

Stringパターン、includeパス、文字列の宣言引数も同じ復号と補間開始の規則を使います。
これらの静的文字列では元ソースの `#{...}` による補間を拒否します。字面としての `#{` は `\#{` と書きます。
`"\u{23}{name}"` や `"#\u{7b}name}"` のようにエスケープで生成した `#{` は、通常式と同じく補間にならず受理します。
`re"..."` / `re'...'` は通常文字列を `Regex::compile` に渡します。正規表現の `\d` は `re"\\d"` と書きます。

`"""..."""` はrawな複数行文字列です。開始行のインデントを基準に本文をdedentし、
上記のエスケープ復号と未知エスケープの拒否を適用しません。既存の補間と `\#{` による抑止は使えます。
`@doc` は同じtriple-quoted形式だけを受け付け、本文での補間は解析エラーです。

REPLと `inspect` の引用表示は制御文字と文字としての `#{` をエスケープし、
String単体では再入力すると元の値に戻ります。利用例と表示規則は[文字列の入力と表示](./strings.md)を参照してください。

### リスト

```surtr
[1, 2, 3]
["a", "b", "c"]
[]
```

## 4. 演算子

### 算術

- `+`
- `-`
- `*`
- `/`: `Div::safe_div`
- `%`: `Mod::safe_mod`

`/` と `%` は同じ型の値同士を受け、`Result` を返します。標準では `Int` と `Float` が `Div`、`Int` が `Mod` を実装します。ユーザー型への実装と bounded generic での利用もできます。暗黙の数値変換や Result の unwrap は行いません。

### 比較

- `<`
- `<=`
- `>`
- `>=`
- `==`
- `!=`

### 文字列結合

- `++`

### パイプ / bind / compose

- `|>`
- `|*>`
- `|*|`
- `|>=`
- `>>`
- `>*`
- `>=>`
- `=?`

#### `|>` 値 apply

`|>` は左辺の値を右辺へ流します。

- 右辺が capture / closure の場合は unary callable として適用する
- 右辺が関数型の変数または括弧付きの関数値式の場合も unary callable として適用する
- 右辺が call 式の場合は、左辺値を第一引数へ注入する

```surtr
value |> &normalize
value |> normalizer
value |> (make_normalizer(10))
value |> normalize(10)
user |> User::get_name()
```

意味:

```surtr
value |> normalize(10)      # => normalize(value, 10)
user |> User::get_name()    # => User::get_name(user)
value |> (make_normalizer(10)) # => make_normalizer(10)(value)
```

#### `|*>` 文脈 map

`|*>` は `Result`、`List`、または `Option` の中の値だけを変換します。

- `Result<A> |*> (A -> B)` は `Result<B>`
- `List<A> |*> (A -> B)` は `List<B>`
- `Option<A> |*> (A -> B)` は `Option<B>`
- 右辺が call 式なら、文脈内部の値が第一引数へ注入される

```surtr
Ok(1) |*> add(2)            # => Ok(add(1, 2))
["a", "b"] |*> wrap("[", "]")
```

`|*>` の右辺は通常、文脈の中身 `A` から値 `B` を返す関数として推論されます。

文脈付きの値も内側の値として保持できます。`|*>` は入れ子を平らにしません。

```surtr
nested: Result<Result<Int>> = Ok(1) |*> {|x: Int| Ok(x)}
flat = Ok(1) |>= {|x: Int| Ok(x)}
print(inspect(nested)) # Ok(Ok(1))
print(inspect(flat))   # Ok(1)
```

次の例は結果の型を指定していないため、型エラーになります。

```surtr
value = Ok(1) |*> {|x: Int| Ok(x)}
```

#### `|>=` 文脈 bind

`|>=` は `Result` / `List` / `Option` の文脈を維持したまま次の段階へ接続します。

- `Result<A> |>= (A -> Result<B>)`
- `List<A> |>= (A -> List<B>)`
- `Option<A> |>= (A -> Option<B>)`

```surtr
Ok(11) |>= require_at_least(10)
[1, 2, 3] |>= expand()
```

`|>=` の右辺が call 式なら、文脈内部の値を第一引数へ注入します。

```surtr
Ok(11) |>= require_at_least(10)   # => require_at_least(11, 10)
```

#### `|*|` Applicative ap

`|*|` は `Applicative::ap` の surface syntax です。

- `Result<(A -> B)> |*| Result<A> -> Result<B>`
- `List<(A -> B)> |*| List<A> -> List<B>`
- `Option<(A -> B)> |*| Option<A> -> Option<B>`

```surtr
Ok(&inc) |*| Ok(1)
Ok(curry(&Add::add)) |*| Ok(1) |*| Ok(2)
```

複数引数の function は `curry()` で明示的にカリー化します。`|*|` は
左結合で、各段階が callable の次の引数を消費します。

#### `>>` 通常関数合成

`>>` は plain function / closure の合成です。

```surtr
pipeline = &trim >> &render
```

左右の式を評価した値は、合成の型契約を満たす関数値でなければなりません。
関数値を返す関数呼出しは `make_inc() >> make_double()` のようにそのまま使えます。
合成は引数を注入しないため、引数不足の呼出しは拒否されます。この規則は `>*` / `>=>` でも同じです。

#### `>*` Lifted 合成

`>*` は `Result` / `List` / `Option` を返す関数の後ろへ pure function を接続します。

```surtr
pipeline = &parse >* &render
```

- `Result` なら `(A -> Result<B>) >* (B -> C)`
- `List` なら `(A -> List<B>) >* (B -> C)`
- `Option` なら `(A -> Option<B>) >* (B -> C)`

これも compose なので、左右とも関数値に限ります。
`parse() >* render()` は不許可です。

#### `>=>` Kleisli 合成

`>=>` は `Result` / `List` / `Option` を返す関数同士を合成します。

```surtr
pipeline = &parse >=> &validate
```

- `Result` なら `(A -> Result<B>) >=> (B -> Result<C>)`
- `List` なら `(A -> List<B>) >=> (B -> List<C>)`
- `Option` なら `(A -> Option<B>) >=> (B -> Option<C>)`

これも compose なので、左右とも関数値に限ります。
`parse() >=> validate()` は不許可です。

#### `=?` SafeBind

`=?` は「失敗したらそのまま伝播する束縛」です。

```surtr
value: Int =? parse_int("1")
[head, ..tail] =? [1, 2, 3]
[head, ..tail] =? Ok([1, 2, 3])
[first, ..tail] =? "source"
Option::Some(saved) =? Option::Some(1)
```

- `pattern =? Result<T, E>` は `Ok` を束縛し、`Err` を早期伝播する
- canonical `Result` RHS は外側一段だけを自動分解し、nested Result は通常 pattern として扱う
- Result 以外の RHS は値と型を変えず、constructor / literal / list / string / Extractor などの partial patternが値全体を明示検査するときだけ受理する
- total pattern + non-Result RHS は、非MonadとResult以外のMonadを区別したSafeBind compile errorにする
- 通常patternのannotation / constructor arity / Extractor契約エラーはSafeBind固有分類より先に報告する
- `do` 外の通常関数・Closureでは、enclosing callableが`MonadFail` を実装した型を返す必要がある。Extractor・ExtractorClosure本文では、その本文自身の `MatchResult::Err` へ元Errorを保持して返す
- `do` 内では、do-local carrierのMonadFailを優先し、なければ`Alternative::empty`、どちらもなければcapability errorにする
- Extractor の `MatchResult::Err` は元 Error を保持する。literal・pin・variant の不一致はそれぞれの Error、List/String 等の構造 Pattern 固有 Error は維持する
- `[head, ..tail]` は MatchBlock では `List` / `String` の分解に使えるが、Expr 位置では list 構築のまま

#### `do` と failure matcher

実行例とResult / Option / MonadTのfailure比較は[do](./do.md)を参照してください。
`do::<Carrier>`、`do::<_>`、`do {}`を使えます。文は改行または`;`で区切り、最後には同じcarrierのMonad式が必要です。
空blockや末尾bindingだけのblockは拒否します。通常`=`はpayloadを取り出さず、途中のbare Monad式はpayloadを捨ててsequenceします。
pattern bindingはRHS解決後に後続文だけへ公開し、block外へ漏れません。captured/fixed引数はblock全体で一致し、payload型は変化できます。

`do` は一つのMonad carrierを左から右へsequenceします。

```surtr
result: Option<Int> = do::<Option> {
  first <- Option::Some(20)
  Option::Some(first + 1)
}
```

- total patternの`<-`は`Monad`だけを要求する
- literal、constructor、list/string、Extractor等のpartial patternを使う`<-`はfailure matcherになる
- failure targetは`MonadFail > Alternative > Monad`の順で選び、`Monad`単独ではfailure targetを提供しない
- SafeBind `=?` もdo-local failure targetを使うが、RHSだけからdo carrierを推論しない
- `guard`は通常の`Alternative`関数であり、MonadFailを参照しない
- base Monad値をTransformerへ暗黙liftせず、`MonadT::lift`を明示する
- nested `do` はそれぞれ自身のcarrierだけでfailure targetを決める

#### range literal

`[start..stop]` は inclusive range literal です。

```surtr
[1..3]         # => [1, 2, 3]
["a".."c"]     # => Ok([a, b, c])
```

- `[Int..Int]` は `List<Int>`
- `[String..String]` は `Result<List<String>, Error>`
- `String` endpoint は single ASCII char として扱う
- constant endpoint は compile-time に fold される
- `""` や `"ab"` のような不正な string endpoint は `Generator::range_char` と同じく start / stop の文字数または ASCII 制約に対応する Error になる
- `[head, ..tail]` とは別構文で、range form は comma を持たない

有限の range helper は `Generator<Item>` を返し、整数は `Generator::range` → `Generator::to_list`、文字は `Generator::range_char` の入力検証 → `Generator::to_list` で List 化します。構築時に全件生成せず、文字 endpoint の検証エラーは start / stop、文字数 / 非 ASCII の条件ごとに分かれます。

#### 共通制約

- 裸の関数参照は許可しない
- `value |> normalize` は不許可
- `pipeline = parse >=> validate` も不許可
- 関数値として保持できるのは capture、closure、または `Ty::Func` を持つ変数・式
- backtick FuncLiteral は中置・前置の呼出しと capture に使う補助構文で、単独では値にならない
- FuncLiteral body は `ident | qualified_path | operator`
- ``left `name` right`` は、通常の名前解決で最初に選んだ参照が関数宣言の場合だけ許可する。変数・関数引数は関数型でも拒否し、外側の宣言を探し直さない
- ``left `operator` right`` は対応する通常演算に lower される
- ``left `Type::method` right`` は `Type::method(left, right)` に lower される
- `&`name`` / `&`Type::method`` はそれぞれ通常の capture と同義
- `&`op`` は 2 引数 callable に lower される
- `&`op`(args...)`` は placeholder capture 規約で lower される
- `&Type`、`&Type::Variant` は user-defined Record / Struct / Enum / Error の constructor capture として扱う。具象 `deferror` constructor は外部入力を受けて共通 `Error` を返す callable として capture できる
- constructor capture の引数は位置指定だけで、引数ブロックには少なくとも1個の placeholderが必要
- constructor capture は `Capture` origin と canonical constructor identity を保持する。compiler-managed constructor と抽象 `Error` 自体の capture はできない
- capture placeholder は `&1` から `&16` までとし、`&0` と `&17` 以上は parse error とする
- bare capture を `inspect` / `to_string` すると、metadata があれば
  `FnCapture(module: M, name: f, sig: sig)` 形式で表示する
- callable の表示は `Result` などの tagged value、struct / record field、List、HashMap、tuple の内部にも再帰適用する
- 部分適用した capture は変数への束縛や再 capture を経ても capture として表示し、closure literal で包んだ場合は `Closure` として表示する
- user-facing callable 表示に内部 function / template ID を出さない
- `Result` と `List` を `|*>`, `|*|`, `|>=`, `>*`, `>=>` で混在させない
- `|>`, `|*>`, `|*|`, `|>=`, `>>`, `>*`, `>=>`, `=?` は同一優先度・左結合
- unqualified infix `` `on` `` と `` `Function::on` `` は flow より低優先度
- 結合優先度は `Bind < StdOn < Apply=Compose < AndOr < Compare < Pair < Expr < FacetChain < Postfix`
- `on`、`and`、`or`、`eq`、`neq`、`lt`、`lte`、`gt`、`gte` は予約名。変数・引数・Patternの束縛名・フィールド名には使えない。独立した関数・独自 member 名にも使えない。標準 Eq / Compare trait の同名 method 実装は許可する
- 裸の比較関数6名の中置Callは比較演算子と同じ `Compare` 層・左結合。修飾中置Callは既存の `Function::on`・`Kernel::and`・`Kernel::or` を除き通常の `Expr` 層
- `compare`、`pipe` / `fmap` / `bind`、関数合成の関数インターフェースは予約せず、名前付き中置Callの対象は関数宣言に限る
- pair constructor `(,)` は右結合で、`left (,) right` を nested pair に lower する
- `Expr` クラスの `+`, `-`, `*`, `/`, `%`, `++` は同列・左結合
- `FacetChain` の `->` は Facet path 合成に限定した固定構文で、左結合。各オペランドのドット・呼び出しは `Postfix` で先に結合する
- comparison 系 (`==`, `!=`, `<`, `>`, `<=`, `>=`) は `Logical` クラス
- ``left `on` right`` は scope に見えている `on` ではなく、常に `Function::on(left, right)` として解釈される
- 利用者の `Other::on` 宣言は予約名規則で拒否する

## 5. パターン

Pattern は照合位置に直接記述します。通常の式や第一級の値として保持するものではありません。基本形は次のとおりです。

- binding pattern
- `True`
- `False`
- `Ok(x)`
- `Err(e)`
- `_` または `_name` のような identifier（wildcard pattern。束縛を生成しない）。数字だけの `_N` は projection 等の専用位置に限る
- as-pattern は `pattern @ name` または `pattern@name` で書ける
- `Int` リテラル
- `String` リテラル
- Duration リテラル（`1ms` など。Float リテラルと Unit 値 `()` の検査 Pattern は使えない）
- pin `^name`（外側で束縛済みの値と `Eq` で比較）
- tuple Pattern `(left, right)`（1要素の tuple Pattern は使えない）
- list Pattern `[]`、`[first, second]`、`[head, ..tail]`。String の head / tail 分解にも `[head, ..tail]` を使う
- HashMap Pattern `hash![key => child, ...]`（String キーの存在と値を照合し、追加キーを許容する。空 Pattern は任意の HashMap に成功する）
- 入れ子になった constructor pattern
- Record の構造的 Pattern `User(name, age)` / `User(age: selected_age, name: selected_name)` / `User(name: selected_name, age)`（全 field を指定する。名前指定内の裸の束縛名は同名 field の省略記法）
- named Extractor または束縛済み ExtractorClosure の `head(pre_args..., payload_patterns...)`
- OR Pattern `p1 | p2`（`match` arm、`if_let`、`if_let_then`、binding-free な `is_match`。子 Pattern 内でも使用可能）

`match` / `if_let` / `if_let_then` の同一 OR 内では、全 alternative の束縛変数名・解決済み型・個数が一致する必要があります。Pattern 内での順序は異なっても構いません。`if_let` 系は全候補失敗時に fallback へ進み、網羅性を要求しません。`is_match` は全 alternative で変数束縛を禁止します。`=` / `=?`、do binding、`apply_pattern` の Pattern では、入れ子の OR も構文エラーです。これらの input / RHS にある通常 `match` の arm 内 OR は許可されます。

構造体の constructor Pattern は attached Extractor `Type::deconstruct(...)` を通ります。Record の構造的 Pattern とは別の契約です。通常の `=` は全体が必ず成功する Pattern だけに使えます。Extractor は常に partial として扱います。

`match` の網羅性は guard のない arm から検査します。現行解析は外側の variant や空 / 非空の被覆を中心とし、複数 arm の子 Pattern を合成した一般的な構造的網羅性は証明しません。実行時の取りこぼしを避ける書き方と詳細は [Pattern Matching](./pattern-matching.md)、型ごとの分解は [Record](./record.md) / [Structs](./structs.md) / [Extractors](./extractors.md) を参照してください。

## 6. フィールドアクセス

```surtr
value.field
```

`defstruct` と `defrecord` の両方で使えます。`defenum` では使えません。

### `defstruct` の構築規約

- `impl struct` では `new` を必須実装とする
- `Type(...)` は `Type::new(...)` の糖衣として解決される
- `Type { ... }` 構造体リテラルは `impl Type` の同型メソッド本体内でのみ使用可能
- struct literal の field は `field: expr` または shorthand の `field` を使える
- shorthand は `field: field` の sugar で、`Type { name, age: next_age }` のように混在可能
- Struct の `Type(...)` / `Type::new(...)` や Struct Pattern にはこの shorthand を追加しない
- `Type::new` は import 対象外
- `Type(...)` の pattern 側は `Type::deconstruct(...)` を要求する

private field と property access を含む構造体全体の契約は `./structs.md` を参照してください。

### 引数規約

- 通常の関数呼出しと Struct の `Type(...)` / `Type::new(...)` は名前付き引数を使えるが、位置引数との混在は禁止
- Record 構築は名前指定がなければ位置指定、名前指定が一つ以上あれば名前指定として扱う。名前指定内の裸の変数 `field` は `field: field` の省略記法で、任意式には field 名が必要
- Record は全 field の指定が必要。名前指定の記述順は自由で、構築値の評価・配置は宣言順。入れ子の名前指定は外側の分類に影響しない
- constructor capture の引数は位置指定だけで、Record の省略記法は使えない

## 7. 組込み関数

| 名前 | 型 |
|---|---|
| `if` | `(Boolean, (-> $A), (-> $A)) -> $A` |
| `if_then` | `(Boolean, (-> Unit)) -> Unit` |
| `require` | `(Boolean, Lazy<Error>) -> Result<Unit>` |
| `ensure` | `($A, ($A -> Boolean), Error) -> Result<$A>` |
| `and` | `(Boolean, Boolean) -> Boolean` |
| `or` | `(Boolean, Boolean) -> Boolean` |
| `on` | `(($B, $B -> $C), ($A -> $B) -> ($A, $A -> $C))` |
| `compare` | `($A, $A) -> Ordering` |
| `lt` | `($A, $A) -> Boolean` |
| `lte` | `($A, $A) -> Boolean` |
| `gt` | `($A, $A) -> Boolean` |
| `gte` | `($A, $A) -> Boolean` |
| `eq` | `($A, $A) -> Boolean` |
| `neq` | `($A, $A) -> Boolean` |
| `concat` | `(String, String) -> String` |
| `print` | `(String) -> Unit` |
| `to_string` | `($A) -> String` |
| `inspect` | `($A) -> String` |
| `Div::safe_div` | `(Self, Self) -> Result<Self>` |
| `Mod::safe_mod` | `(Self, Self) -> Result<Self>` |
| `eprint` | `(Error) -> Unit` |
| `set_exit_code` | `(Int) -> Unit` |

### 補足

- `if` / `if_then` の branch が関数型で書かれているのは、選ばれた側だけを評価する special form であることを型で表しているため
- 普段の source では closure を明示せず `if(flag, "ok", err_reason)` や `if_then(flag, print("ok"))` のように書ける
- `and` / `or` は宣言上は普通の 2 引数関数だが、コンパイラが short-circuit として解釈する
- `compare` は ordered comparison の公開 helper で、`Compare::compare` と同じ比較制約に従う
- `lt` / `lte` / `gt` / `gte` は ordered comparison の公開 helper で、`Compare` の default helper method と同じ比較制約に従う
- `eq` / `neq` は call-style helper で、`==` / `!=` と同じ比較制約に従う
- `<` / `<=` / `>` / `>=` は `Compare` を満たす型に対してのみ使え、それぞれ `Compare::lt` / `Compare::lte` / `Compare::gt` / `Compare::gte` に対応する
- `concat` は call-style helper で、`++` と同じく `String` 同士だけを受ける
- `Div` の標準数値実装はゼロ除算時に `Err(ZeroDivisionError)`、`Mod` は `Err(ZeroModuloError)` を返す。トレイトはエラー契約を固定せず、ユーザー実装は独自エラーを指定できる
- `set_exit_code` は処理系側で使用位置制約を持つ

## 8. 標準エラー

標準定義ソース層で最初から提供される汎用 error には、少なくとも次が含まれます。

```surtr
deferror NoneError { "None Value." }
deferror ZeroDivisionError { "division by zero" }
deferror ZeroModuloError { "modulo by zero" }
```

現在の実装には次も含まれます。

```surtr
deferror EmptyHeadTailListPattern { "head-tail list pattern requires a non-empty List" }
deferror ListIndexOutOfBounds(index: Int, length: Int) {
  |index: Int, length: Int|
  Self(message: "list index #{index} out of bounds for length #{length}", index, length)
}
```

これらは `Error` 抽象に乗る具体 error です。

## 9. モジュールと import

### 関数の所属

Surtr では「module の外に生の関数がぶら下がる」モデルを取りません。

- `defmod Name` は通常 module を作る
- `impl Type` は型専用の module-like namespace を作る
- `deftrait Name` は trait method の契約 namespace を作る
- `impl Trait for Type` は trait 実装 namespace を作る
- script / REPL の top-level `def` は暗黙の擬似 module に入る

一方で `defstruct` / `defrecord` / `deferror` / `defenum` / `@builtin type` は
関数 namespace の内側ではなく、top-level 宣言名として直接見えます。

### 標準定義ソース

標準定義は `Bootstrap` stage、test extension を必要に応じて含む shared standard stage、ユーザ拡張の順でロードされます。
完全なモジュール inventory と順序は compiler source の `STDLIB_MODULE_SPECS` が管理します。

### auto import

- `Bootstrap`、`Kernel`、`Function` などの `@autoimport` 付き標準モジュールと、`@autoimport` 付き標準 `impl Type` owner helper surface / 標準 trait は auto import 対象
- `Functor`, `Applicative`, `Monad` は auto import 対象であり、`fmap`, `pure`, `ap`, `return`, `bind` を bare 名で呼べる
- auto importは各ファイルの先頭で対象モジュールを全件importする規則である。既にauto importされたモジュールへの明示importは、全件・単一member・リストのいずれも重複importとしてcompile errorになる
- auto importされたtrait宣言とhelperも導入済みとして扱う。namespaceの親モジュールからそのtraitを選択するimportも重複になるが、親モジュールの他のmemberだけをimportすることはできる
- それ以外の標準定義ソースは auto import しない

### import の意味

- `import` は `Bootstrap` に属する builtin function として文書化される
- 実際の surface では compile-time declaration syntax として扱う
- `import Mod` は、その module の import 可能 member を現在 scope に unqualified で入れる
- `import Mod::fun` は単一 member だけを入れる
- `Struct` 名や `new` のように import 不可の宣言もある
- `import` は file declaration area と `defmod` / `impl Type` / `impl Trait for Type` body に書ける
- `def` / `defp` / `defextractor` / closure / top-level expr の中では使えない
- auto import 済みのモジュールを再 import することはできない
- 明示 import と auto-import の組み合わせを含め、取り込み同士が同じ unqualified 関数名を導入する場合は compile error
- 全件取り込み後の通常関数名は内側のスコープの定義・束縛で shadow できる。SpecialForm などの既存禁止対象は除く

### user namespace

- `namespace N { ... }` は型、trait、`impl` target、`defmod` head を1段の user namespace `N` に置く
- `namespace N { defmod M { ... } }` と `defmod N::M { ... }` は同じ canonical module path を作る
- 型は import せず、別 namespace の型を `N::Type` で直接参照する
- `Global::` は compiler の暗黙 root 名であり、user source には書かない

### script / REPL の top-level ルール

- script source では top-level expr を許可する
- script source の declaration area には `include` / `import` / `const` / top-level `def` / `namespace` / `defstruct` / `defrecord` / `defenum` / `deferror` / `deftrait` / `impl Type` / `impl Trait for Type` を書ける
- script source の explicit top-level `defmod` は禁止する。共有 helper は `namespace + defmod` か include module file に置く
- script source の `include` は file 先頭に連続して置く必要があり、宣言や top-level expr の後には書けない
- script source で最初の top-level expr が現れた後は、それ以降の宣言を置けない
- script source の bare top-level `def` は暗黙の script namespace に属し、同一 script の top-level expr と top-level `def` body からは bare call できる
- `namespace` / `impl` / include 先 module file / 他 module から script helper を参照する場合は canonical path を使う
- REPL chunk では top-level `def` / `import` だけを許可し、`const`、型定義、`impl`、`defmod` は許可しない
- `surtr repl --script <file.srt>` は REPL 開始前に script を 1 件 preload できる
- `surtr repl --module <file.srt>` は REPL 開始前に definition source を 1 件 preload できる
- preload は `module -> script` の順で同一 compile unit として読む。`--script` preload は `include` を先に解決して script を一度実行し、その結果を引き継いだうえで、その後の対話入力だけを `ReplChunk` 制約で扱う

### include の意味

- `include` も `Bootstrap` に属する builtin function として文書化される
- 実際の surface では compile-time declaration syntax として扱う
- script source でのみ使える loader directive
- `include "./path/to/module.srt"` の形だけを受け付ける
- source file 先頭に連続して置かなければならない
- `include` 先の file は definition source として読み込まれ、`surtr repl --script` preload でも同じ規則を使う
- block 内や式位置では使えない

### builtin type の置き場所

各 builtin type は、対応する標準定義ソース file のトップレベルで宣言します。

```surtr
// kernel.srt
@builtin type Unit

// int.srt
@builtin type Int

// list.srt
@builtin type List<$A>

// hash_map.srt
@builtin type HashMap<$V>

// result.srt
@builtin type Result<$T>
```

`Unit`, `Hole` は `special_types.srt` に集約します。
数値 helper は `int.srt` / `float.srt` の `impl Int` / `impl Float` に置きます。

### import の重複

同一 file では、同じモジュールまたは同じメンバーの再 import を禁止します。auto importもこの重複判定に含みます。Kernelは既に導入済みなので、次はいずれも最初の明示importでエラーになります。

禁止例:

```surtr
import Kernel;
```

```surtr
import Kernel::print;
```

## 10. `@builtin` と `@doc`

`@builtin def ...` は標準定義ソースでのみ使えます。

- user script では使えない
- user module では使えない
- REPL でも使えない

これは「builtin をユーザーが追加するための構文」ではなく、「処理系内の共有 builtin テーブルを Surtr source 側から宣言するための構文」です。

`@builtin type ...` も同じく標準定義ソース専用です。  
各標準定義ソース file の top-level に置いて、compiler が canonical head と照合します。

`@doc """..."""` は public な module、type、callable、process owner などの宣言に付けられます。
private declaration には付けられず、`impl Type` block 全体ではなく public member ごとに書きます。
標準ライブラリではこの仕組みを使って source に API 説明を埋め込みます。

`Result` には declaration-only の special constructor head もあります。

```surtr
@builtin type Ok($T) -> Result<$T>
@builtin type Err(Error) -> Result<$T>
```

これらは通常の関数本体付き `def` ではなく、標準定義ソース `result.srt` で compiler が特別扱いする surface contract です。

`Bootstrap` では次のような macro doc anchor を置けます。

```surtr
defmod Bootstrap {
  @doc """Language-provided import macro function."""
  @builtin def import() -> Unit

  @doc """Language-provided include macro function."""
  @builtin def include(path: String) -> Unit
}
```

これらは `Bootstrap::import` / `Bootstrap::include` の canonical source です。`import` は file declaration area と `defmod` / `impl Type` / `impl Trait for Type` body に書け、`include` は引き続き file top-level だけで使えます。

## 11. Pattern の結果を扱う

### apply_pattern

- canonical surface は `Kernel::apply_pattern(value, pattern) -> Result<$Return>`。input が Result でも自動 unwrap しない
- projection `_1`〜`_16` を番号順に返す。0個は Unit、1個は値、複数は tuple。番号は1から連続・重複なしとし、`_01` は1と同じ
- 通常位置・alias・list tail の型注釈を許可する。通常 binding は内部限定で、全照合成功後にだけ投影結果を公開する
- Extractor の元 Error と通常 Pattern の Error を `Err` に保持し、外側 callable から早期 return しない
- OR、Pattern への pipe 注入、Pattern 引数の部分適用補完は拒否する。`value |> apply_pattern(pattern)` は第1 Expr 引数へ注入する
- `if_let` / `if_let_then` / `is_match` / `apply_pattern` は予約 consumer 名。`Regex::matches` は通常 call / capture として扱う
- これらの consumer は Pattern を直接記述した完全 call を capture できる（`&is_match(&1, Ok(_))`、`&apply_pattern(&1, [_1, .._])` など）。bare capture、Pattern 引数の `&N` による直接置換は拒否する

### Result callable の Extractor 変換

`Extractor::from_result(f: ($A -> Result<$B>)) -> ExtractorClosure<($A -> MatchResult<$B>)>` は通常SRTの標準APIです。単項callableをcaptureし、各Pattern occurrenceで1回実行します。外側Resultだけをunwrapし、成功payloadと元Errorを保持します。Option/raw/入力0個/複数入力の暗黙変換はありません。

詳しい使い方は [Pattern Matching](./pattern-matching.md)、[HashMap](./hash_map.md)、[Extractors](./extractors.md)、実装契約は [Pattern / Extractor 実装契約](../dev/Pattern_spec.md) を参照してください。

## 12. 現在のスコープ外

このリファレンスでは扱わないもの:

- associated types / associated consts
- 匿名 `impl Trait` 型（parameter / return / generic argument / impl target component）
- 任意のデータ型エイリアス / NewType（関数シグネチャ alias の `type F = (...)` は対応済み）
- マクロシステム拡張
- 並列コンパイル
- 高度なモジュールシステム拡張

Trait system の利用規則は [Trait システム](./trait-system.md) と [Trait Impls](./trait-impls.md) を参照してください。開発者向けの正本一覧は [Developer Docs](../dev/README.md) にあります。

名前の予約範囲と標準宣言の特殊処理は [関数名と呼出し構文](./callable-names.md) を参照してください。
