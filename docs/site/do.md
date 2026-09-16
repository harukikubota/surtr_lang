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
- `name = expression`:通常の束縛。Monad payloadを取り出しません。
- 途中のbare Monad式: payloadを捨てて次の文へ進みます。
- 最後の式:同じcarrierのMonad値を返します。空blockや末尾bindingだけのblockは拒否されます。

separatorには改行か`;`を使います。bindingはそのRHSでは見えず、後続文だけで使えます。
block内で導入した名前はblock外へ漏れません。
各binding/continuationの実行ごとにRHSを一度評価し、failureとなったその経路の後続文を実行しません。
List等の分岐carrierでは、後続のcontinuationを各payloadについて実行します。

## Failure matcherとResult

literal、constructor、list/string分解、Extractor等の、実行時に不一致となり得るpatternを
ここではfailure matcherと呼びます。新しいTraitや構文名ではなく、partial patternの説明です。

Result carrierでは、不一致のErrorはmessage等を置き換えず、そのままResultへ返します。

現在のREPLでは、partial `<-`の不一致から生成するErrorのkind/messageが崩れる既知の問題があります。
以下のpartial `<-`の出力コメントは仕様上の期待値です。SafeBindの例はREPLで確認済みです。
再現条件と修正条件は[フォローアップ](../../doc/do_result_effect_repl_error_followup_spec.md)に記録しています。

```surtr
mismatch: Result<Int> = do::<Result> {
  2 <- Ok(1)
  Ok(3)
}
mismatch # Err(PatternMismatch("Pattern did not match."))
```

SafeBindでも、RHSの`Err(error)`と、matcherの不一致から生成したErrorを保持します。
list/string等の構造pattern固有Errorを一般的なErrorへ書き換えません。
Extractorは現行の`Option`返却で、`None`は共通`PatternMismatch`になります。

```surtr
checked: Result<Int> = do::<Result> {
  Option::Some(value) =? Option<Int>::None
  Ok(value)
}
checked # Err(PatternMismatch("Pattern did not match."))
```

SafeBindは`<-`の別表記ではありません。non-Result RHSではpartial patternが値全体を検査します。
`value =? Option::Some(1)`のようなtotal patternはcompile errorで、Option payloadを暗黙に取り出しません。
SafeBind RHSだけからdo carrierをResultへ決定する規則もありません。

## Alternative実装型

通常sequenceには`Monad`が必要です。failure matcherとSafeBindのfailure targetは、
`Result effect > Alternative > Monad`の順で決まります。Monadだけではfailure targetを提供できません。

Result effectを持たないcarrierでは`Alternative`が必要で、failureのError/messageを破棄して
そのcarrierの`empty`へ接続します。Optionなら`None`、Listなら空Listです。

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

ResultもAlternativeも持たないcarrierではcompile errorになります。
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

`OptionT<Result, A>`は明示`@result_effect`により、baseがcanonical Resultへ直接具体化したとき
Result effectを持ちます。内部のどこかにResultがあるだけでは対象になりません。
SafeBindとpartial `<-`ではOptionTのAlternativeよりResult effectを優先し、Errorをbase Resultへ保持します。

```surtr
checked: OptionT<Result, Int> = do::<OptionT<Result, _>> {
  Option::Some(value) =? Option<Int>::None
  OptionT::some::<Result>(value)
}
OptionT::run(checked) # Err(PatternMismatch("Pattern did not match."))

mismatch: OptionT<Result, Int> = do::<OptionT<Result, _>> {
  2 <- OptionT::some::<Result>(1)
  OptionT::some::<Result>(3)
}
OptionT::run(mismatch) # Err(PatternMismatch("Pattern did not match."))

blocked: OptionT<Result, Unit> = guard(False)
OptionT::run(blocked) # Ok(Option::None)
```

`guard`は通常のAlternative関数で、Result effectを参照しません。
liftしたbaseのErrをtotal `<-`でsequenceする場合も、通常のTransformer Monad実装がそのErrを保持します。

標準型でResult effectが付くのはOptionTだけです。
EitherTはAlternativeも持たず、ReaderT/StateTのAlternativeにはbaseのAlternativeも必要です。
したがって標準`EitherT<L, Result, A>`、`ReaderT<E, Result, A>`、`StateT<S, Result, A>`では
total `<-`は利用できても、partial `<-`やSafeBindは能力不足になります。

## その他の境界

- `guard`、`pure`、`return`は通常callです。引数式の評価をdoが特別に遅延化しません。
- nested doはそれぞれ自身のcarrierでfailure targetを決め、外側のResult effectを継承しません。
- do外のSafeBindは最も近いcallable自身のResult/Result-effect return targetを要求します。
- `DoBlock`はcompiler専用signature markerで、利用者が値、field、annotation、implに使う型ではありません。

構文の一覧は[言語リファレンス](./language-reference.md)、Errorの扱いは[エラーハンドリング](./error-handling.md)、
Transformer固有APIは[Monad transformers](./monad-transformers.md)を参照してください。
