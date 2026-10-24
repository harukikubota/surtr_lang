# `do`によるMonadの逐次処理

`do`は、一つのcontainer（carrier）に属する値を順に処理する構文です。
まずpayload型を一つ持つ`Result<A>`と`Option<A>`から始めます。
`<-`でpayloadを取り出し、最後に同じcarrierの値を返します。

```surtr
result: Result<Int> = do::<Result> {
  first <- Ok(20)
  second <- Ok(first + 1)
  Ok(second * 2)
}
result # Ok(42)

option: Option<Int> = do::<Option> {
  value <- Option::Some(20)
  Option::Some(value + 1)
}
option # Option::Some(21)
```

`do::<Result>`等の指定は[ReturnTypeArgument](./type-annotations.md)です。
`do { ... }`や`do::<_> { ... }`では期待型、RHS、最終式等からcarrierを推論します。
根拠が不足する場合はambiguityになり、候補数や標準型名では補完されません。

## 文とscope

- `pattern <- rhs`: carrierのpayloadをpatternへ渡します。
- `pattern =? rhs`: SafeBind。[MonadExtractor による共通の入力規則](./monad.md#safebind)で分解し、失敗を do-local な返却先へ渡します。
- `expression?`: 文末の `?`。共通の Unit 条件を満たす入力を確認し、失敗を do-local な返却先へ渡します。成功値が Unit に至るまで各層の extract を呼びます。
- `name = expression`:通常の束縛。Monad payloadを取り出しません。
- 途中のbare Monad式: payloadを捨てて次の文へ進みます。
- 最後の式:同じcarrierのMonad値を返します。空blockや末尾bindingだけのblockは拒否されます。

式は改行で区切ります。`;` を付けた式は通常のブロックと同じく Unit になり、値を破棄します。
`do` 直下でも Err を伝播しないため、アサーションの失敗を伝播するときは `;` を付けずに並べます。
bindingはそのRHSでは見えず、後続文だけで使えます。
block内で導入した名前はblock外へ漏れません。
各binding/continuationの実行ごとにRHSを一度評価し、failureとなったその経路の後続文を実行しません。
List等の分岐carrierでは、後続のcontinuationを各payloadについて実行します。

文末の `?` は Unit 文なので、carrier を推論する根拠にはなりません。
Unit は最終 Monad 値にはならないため、最後の処理は `?` を付けず、その Monad 値を返します。
文末の `?` は異なる MonadExtractor 実装型の入れ子も分解し、各層の extract が返す Err を同じ do-local な失敗返却先へ渡します。
RHS と到達した各層の extract は一度だけ評価し、失敗後の内側と後続は実行しません。全層が成功して Unit に達した場合だけ続行します。SafeBind `=?` は一段の分解を維持します。
詳しい型条件と式位置の制約は[Monad](./monad.md#文末の-)を参照してください。

複数のアサーションは、通常の bare Result 式を順に書けば短絡できます。
`do` はテストで必須ではなく、文末の `?` や SafeBind でも失敗を伝播できます。
具体例は[テストを書く](./test.md)を参照してください。

## Failure matcherとResult

literal、constructor、list/string分解、Extractor等の、実行時に不一致となり得るpatternを
ここではfailure matcherと呼びます。新しいTraitや構文名ではなく、partial patternの説明です。

Result carrierでは、不一致のErrorはmessage等を置き換えず、そのままResultへ返します。

```surtr
mismatch: Result<Int> = do::<Result> {
  2 <- Ok(1)
  Ok(3)
}
mismatch # Err(IntLiteralPatternMismatch(...))
```

SafeBindでも、RHSの`Err(error)`と、matcherの不一致から生成したErrorを保持します。
list/string等の構造pattern固有Errorを一般的なErrorへ書き換えません。
Extractor は `MatchResult` を返し、`Err` の元 Error を MonadFail の経路 で保持します。Alternative route では破棄します。

```surtr
checked: Result<Int> = do::<Result> {
  value =? Option<Int>::None
  Ok(value)
}
checked # Err(NoneError("None Value."))
```

SafeBind と `<-` は取り出しの入口が異なります。SafeBind は RHS の
`MonadExtractor::extract`、`<-` は carrier の `Monad::bind` を使います。
`<-` は bind が渡す payload 全体を Pattern へ渡し、payload の extract を呼びません。
SafeBind の RHS だけから do の carrier を決めることもありません。

```surtr
selected: List<Int> = do::<List> {
  Ok(value) <- [Ok(1), Err(NoneError())]
  [value]
}
selected # [1]
```

## Alternative実装型

通常の逐次処理には `Monad` が必要です。partial `<-`、SafeBind、文末 `?` の失敗処理先は、
carrier の `MonadFail`、次に `Alternative` の順で決まります。Monad だけでは失敗処理先を提供できません。

これらの失敗処理を使い、carrier が MonadFail を実装しない場合は `Alternative` が必要で、failure の Error/message を破棄して
そのcarrierの`Alternative::empty`へ接続します。Optionなら`None`、Listなら空Listです。

この処理は、その do が所有する失敗処理に適用されます。呼び出した関数が返した Err も、
SafeBind や文末 `?` で処理すれば同じ規則に従います。通常の値として保持した Result の中身を do が探索することはありません。

Pattern の失敗を呼び出し先の関数で処理し、同じ do-carrier 型の値として返すこともできます。
その値を total `<-` や途中の bare Monad 式で接続すれば、do は通常の `Monad::bind` を使います。
呼び出し先で処理済みの失敗を do が再処理して empty に上書きすることはありません。
この接続には Monad だけが必要で、carrier が Alternative や MonadFail を実装していなくても使えます。
do 側で改めて partial Pattern を照合する場合などは、その do の失敗処理先が必要です。

たとえば、結果型が `List<Result<Unit>>` でも carrier は List です。SafeBind、partial `<-`、文末 `?` の失敗は、
内側の Result の Err にはならず、その経路の空 List になります。他の経路の結果は残ります。
do は carrier に従って一意に解釈され、payload が Result であることを理由に失敗処理先を切り替えません。
Result を成功・失敗の値として List に集めたい場合は、通常の List 構築や map などを使えます。

```surtr
absent: Option<Int> = do::<Option> {
  2 <- Option::Some(1)
  Option::Some(3)
}
absent # Option::None

checked: Option<Int> = do::<Option> {
  value =? Option<Int>::None
  Option::Some(value)
}
checked # Option::None
```

MonadFail も Alternative も持たない carrier で、do 自身の失敗処理先が必要になる構文を使うと compile error になります。
Identity/Reader/Stateのtotal `<-`は使えますが、partial `<-`やSafeBindは使えません。
user-defined Monad/Alternativeでも同じ規則を使い、標準型だけの特例ではありません。

## MonadTとbase Result

[Monad transformers](./monad-transformers.md)も通常のcarrierとして使えます。
base Monad（`MonadT<$M>`の`M`。OuterMと呼ぶ場合もこの部分）からの値は`MonadT::lift`で明示的に接続します。
`Either<L, A>`等のfixed引数、Transformerのbase/environment/stateはblock全体で一致する必要があります。
payload型だけは文ごとに変化できます。

```surtr
lifted: OptionT<Result, Int> = MonadT::lift(Ok(20))
sequenced: OptionT<Result, Int> = do::<OptionT<Result, _>> {
  value <- lifted
  OptionT::some::<Result>(value + 1)
}
OptionT::run(sequenced) # Ok(Option::Some(21))
```

`OptionT`にはMonadFail実装がありません。do内のSafeBindとpartial `<-`の失敗は、
baseによらず`Alternative::empty`へ接続します。Result baseでは`Ok(None)`です。

```surtr
checked: OptionT<Result, Int> = do::<OptionT<Result, _>> {
  value =? Option<Int>::None
  OptionT::some::<Result>(value)
}
OptionT::run(checked) # Ok(Option::None)

mismatch: OptionT<Result, Int> = do::<OptionT<Result, _>> {
  2 <- OptionT::some::<Result>(1)
  OptionT::some::<Result>(3)
}
OptionT::run(mismatch) # Ok(Option::None)

blocked: OptionT<Result, Unit> = Alternative::guard(False)
OptionT::run(blocked) # Ok(Option::None)
```

`guard`は通常のAlternative関数で、MonadFailを参照しません。
`Alternative::guard::<List>(condition)`が返す`List<Unit>`も、そのまま途中の式としてsequenceできます。
`True`なら後続文へ進み、`False`ならその経路の後続文を評価せず空Listを返します。

```surtr
selected: List<Int> = do::<List> {
  value <- [1, 2, 3]
  Alternative::guard::<List>(value > 1)
  [value]
}
selected # [2, 3]
```

liftしたbaseのErrをtotal `<-`でsequenceする場合も、通常のTransformer Monad実装がそのErrを保持します。

`ResultT<M, A, E>`と`EitherT<Error, M, A>`は内側にErrorを保持します。
`ReaderT<E, M, A>`と`StateT<S, M, A>`は、baseがMonadFailを実装するとき、
run時にbaseの`fail`を返します。

## その他の境界

- `guard`、`pure`、`return`は通常callです。引数式の評価をdoが特別に遅延化しません。
- nested doはそれぞれ自身のcarrierでfailure targetを決め、外側のMonadFailを継承しません。
- do 内の関数・Closure・Extractor 本文は、それぞれ自身の失敗処理先を使います。そこから返された値を外側の SafeBind 等で処理した場合は、その構文が属する do の規則に従います。
- do外のSafeBindは最も近いcallable自身のfailure targetを使います。通常の関数・ClosureではMonadFailを実装する返り型、Extractor・ExtractorClosure本文ではMatchResult return targetへ元Errorを保持して返します。
- `DoBlock`はcompiler専用signature markerで、利用者が値、field、annotation、implに使う型ではありません。

構文の一覧は[言語リファレンス](./language-reference.md)、Errorの扱いは[エラーハンドリング](./error-handling.md)、
Transformer固有APIは[Monad transformers](./monad-transformers.md)を参照してください。
