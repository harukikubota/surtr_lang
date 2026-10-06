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
- `pattern =? rhs`: SafeBind。自動分解するRHSはResultの外側一段だけです。
- `expression?`: 文末の `?`。canonical Result の成功型の終端が Unit の場合に、成功値を捨てて外側一段の Err を do-local failure target へ接続します。
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
内側にネストした Result の Err を再帰的に探す規則もありません。
詳しい型制約と式位置の拒否は[エラーハンドリング](./error-handling.md)を参照してください。

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
mismatch # Ok(Option::None)
```

SafeBindでも、RHSの`Err(error)`と、matcherの不一致から生成したErrorを保持します。
list/string等の構造pattern固有Errorを一般的なErrorへ書き換えません。
Extractor は `MatchResult` を返し、`Err` の元 Error を MonadFail の経路 で保持します。Alternative route では破棄します。

```surtr
checked: Result<Int> = do::<Result> {
  Option::Some(value) =? Option<Int>::None
  Ok(value)
}
checked # Ok(Option::None)
```

SafeBindは`<-`の別表記ではありません。non-Result RHSではpartial patternが値全体を検査します。
`value =? Option::Some(1)`のようなtotal patternはcompile errorで、Option payloadを暗黙に取り出しません。
SafeBind RHSだけからdo carrierをResultへ決定する規則もありません。

## Alternative実装型

通常sequenceには`Monad`が必要です。failure matcherとSafeBindのfailure targetは、
`MonadFail > Alternative > Monad`の順で決まります。Monadだけではfailure targetを提供できません。

MonadFailを実装しないcarrierでは`Alternative`が必要で、failureのError/messageを破棄して
そのcarrierの`Alternative::empty`へ接続します。Optionなら`None`、Listなら空Listです。

```surtr
absent: Option<Int> = do::<Option> {
  2 <- Option::Some(1)
  Option::Some(3)
}
absent # Option::None

checked: Option<Int> = do::<Option> {
  Option::Some(value) =? Option<Int>::None
  Option::Some(value)
}
checked # Option::None
```

MonadFailもAlternativeも持たないcarrierではcompile errorになります。
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
  Option::Some(value) =? Option<Int>::None
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

`ResultT<M, A>`と`EitherT<Error, M, A>`は内側にErrorを保持します。
`ReaderT<E, M, A>`と`StateT<S, M, A>`は、baseがMonadFailを実装するとき、
run時にbaseの`fail`を返します。

## その他の境界

- `guard`、`pure`、`return`は通常callです。引数式の評価をdoが特別に遅延化しません。
- nested doはそれぞれ自身のcarrierでfailure targetを決め、外側のMonadFailを継承しません。
- do外のSafeBindは最も近いcallable自身のfailure targetを使います。通常の関数・ClosureではMonadFailを実装する返り型、Extractor・ExtractorClosure本文ではMatchResult return targetへ元Errorを保持して返します。
- `DoBlock`はcompiler専用signature markerで、利用者が値、field、annotation、implに使う型ではありません。

構文の一覧は[言語リファレンス](./language-reference.md)、Errorの扱いは[エラーハンドリング](./error-handling.md)、
Transformer固有APIは[Monad transformers](./monad-transformers.md)を参照してください。
