# `do` 構文ユーザ向け公開文書 follow-up 仕様

## 1. 状態と目的

- 状態: 文書化完了。実装済み契約の移管記録。
- 種別: N11およびResult effect実装後のユーザ向け文書 follow-up。
- 対象: 現行実装済みの `do` 構文、SafeBind、failure matcher、`Alternative`、`MonadT` の公開説明。
- level: 文書のみ。既存の level 4 言語契約を変更せず、現行実装とテストで確定した意味を公開する。

N01–N11と `b2ef1a33` の Result effect 契約は実装済みとして扱う。Generatorの再設計は本書の対象・依存・完了条件に含めない。
本書は新しい構文、型規則、暗黙変換、compiler fallbackを提案しない。

ユーザが `do` を最初に理解できる入口を `Result` / `Option` の1-kind carrierから作り、
その後に一般の `Monad` / `Alternative`、captured argumentを持つcarrier、標準 `MonadT` 実装へ段階的に広げる。

## 2. 用語と公開上の境界

### 2.1 1-kind carrier

導入では `Result<A>`、`Option<A>` のようにpayload slotを一つ持つ型constructorを扱う。
`do::<Result>` / `do::<Option>` の型入力は通常のReturnTypeArgumentであり、`do`専用の型適用構文ではない。

`Either<L, A>`、`OptionT<M, A>`、`EitherT<L, M, A>`、`ReaderT<E, M, A>`、
`StateT<S, M, A>`のように追加のcaptured argumentを持つcarrierも、mapped payload slot `A`を一つ持つ
同じTypeCtorTrait規則で扱う。固定引数はdo block全体で一致し、payload型だけが文ごとに変化できる。

### 2.2 failure matcher

ユーザ向け文書では、実行時に一致しない可能性があるliteral、constructor、list/string分解、Extractor等の
partial patternを「failure matcher」と説明してよい。ただし、これは新しい構文要素、Trait、runtime値、
compiler内部型の公開名ではない。正規の構文名はpatternである。

`pattern <- rhs` と `pattern =? rhs` は別のbinding formであり、failure matcherの失敗先も異なる。

- `<-` はdo carrierの値からpayloadを取り出す。failure matcherを使う場合は、do carrier自身のResult effectを優先し、なければ`Alternative`が必要。
- `=?` はcanonical `Result` RHSだけを外側一段分解し、またはpartial patternでnon-Result RHS全体を検査する。
  do内では、失敗先をdo carrierに応じてResult保持または`Alternative::empty`へ接続する。

## 3. ユーザ向けに公開する共通契約

`do` blockは一つのMonad carrierを左から右へsequenceする。

- `do::<Carrier> { ... }`でcarrierを明示できる。
- `do::<_> { ... }`または`do { ... }`では、RHS、最終式、期待型などの通常の型制約から推論する。
- `<-`のRHSはpattern bindingをscopeへ追加する前に評価・解決する。bindingは後続文だけで見える。
- 通常の`name = expression`はMonad payloadを取り出さない。
- bareなMonad式は順に実行し、そのpayloadを捨てる。
- 最終式は同じcarrierのMonad値でなければならない。空blockや末尾のbindingだけでは完了しない。
- `Monad`は常に必要。partial `<-`または合法な`=?`ではResult effectを優先し、なければ同じcarrierの`Alternative`が必要。どちらもなければcapability errorになる。
- SafeBind RHSはdo carrierの推論元ではない。RHS型、impl数、登録順、標準型名からcarrierを逆決定しない。
- RHSは一度だけ評価する。失敗時は後続文を評価しない。

## 4. Result / Optionから始める説明

### 4.1 Result carrier

最初の例では、total patternの`<-`でResult pipelineと同じ逐次処理を示す。

```surtr
result: Result<Int> = do::<Result> {
  first <- Ok(20)
  second <- Ok(first + 1)
  Ok(second * 2)
}
```

Resultは`Monad`かつResult effect carrierとして利用できる。標準Resultは`Alternative`を
実装しないが、partial `<-`のfailure matcherは共通pattern ErrorをResultへ保持できる。

Result-do内の`=?`では既存SafeBindのfailureを保持する。

- RHSが`Err(error)`なら、そのerrorを変更せずdo結果の`Err(error)`として返す。
- failure matcherの不一致やExtractor failureから既存SafeBindが作るErrorは、kind、detail、messageを
  別の一般errorや`Alternative::empty`へ変換せず、そのままdo結果へ返す。
- 失敗後の文は評価しない。
- canonical Resultの外側一段だけを自動分解し、nested Resultを再帰的にunwrapしない。

「メッセージをそのまま返す」は、render済み文字列を再解析・再生成するという意味ではない。
同じstructured Error / diagnosticのkind・detail・messageを保持し、通常の表示経路で出力することを意味する。

### 4.2 Option carrier

Optionでもtotal patternの`<-`は通常のMonad sequenceとして利用できる。

```surtr
result: Option<Int> = do::<Option> {
  first <- Option::Some(20)
  Option::Some(first + 1)
}
```

Optionは`Alternative`を実装するため、partial `<-`のfailure matcher、またはdo内の合法な`=?`が
失敗した場合は`Option::None`になる。Result RHSの`Err`やfailure matcherが持つErrorのmessageを
Optionへ暗黙変換せず、failure payloadを破棄してOptionの`empty`を返す。

## 5. Result effectを持たないAlternative実装型でのfailure matcher

Result effectを持たないdo carrierでは、合法なSafeBindのすべてのfailure出口を同じcarrierの
`Alternative::empty()`へ接続する。canonical Result以外でも、有効な`@result_effect` carrierは
このrouteより先にResult effectを選ぶ。

- Result RHSの`Err`
- Result payloadへ適用したfailure matcherの不一致
- non-Result RHS全体へ適用したfailure matcherの不一致
- Extractor failure

これらのfailure kind、message、payloadを別carrierへ変換する暗黙APIは作らない。
`Option`なら`None`、`List`なら空List、ユーザ定義carrierならその型のresolved `Alternative::empty`となる。
carrierが`Alternative`を持たなければ、fallbackを合成せずcompile errorにする。

## 6. MonadT実装型

標準 `OptionT`、`EitherT`、`ReaderT`、`StateT`は、それぞれの具体化された外側carrierが実装する
`Monad` / `Alternative`と、明示`@result_effect`から検証済みのResult effectを使ってdoを実行する。
annotationのないTransformerのfield構造からeffectを探索しない。

- base Monadの値をTransformer-doへ直接混ぜない。
- base値は`MonadT::lift`で明示的に持ち上げる。
- `do::<OptionT<Result, _>>`等は通常のapplied carrier ReturnTypeArgumentである。
- captured base、environment、state、Eitherのleft slotはblock全体で保持する。
- `EitherT`は標準`Alternative`を持たない。
- `ReaderT` / `StateT`が`Alternative`を得るにはbase carrierにも`Alternative`が必要。
- `OptionT`の`Alternative::empty`はbaseの`pure(None)`であり、baseの`empty`ではない。

## 7. `MonadT.OuterM`がResultの場合

ユーザ向け文書では、Transformerが内部で使う `M` を「base Monad」と呼び、必要な箇所だけ
`MonadT.OuterM`との対応を示す。ここでは `OptionT<Result, A>` 等の `Result` 部分を指す。

base Resultのfailureと、Transformer-do内SafeBindのfailure matcherを混同しない。

### 7.1 `MonadT::lift`と`<-`

`MonadT::lift(Err(error))`はbase Resultのfailureを保持する。持ち上げたTransformer値を`<-`でsequenceした場合も、
各Transformerの通常のMonad実装を通じてbaseの`Err(error)`が保持される。

```text
ResultのErr
  -> MonadT::lift
  -> OptionT<Result, A>等のbase failure
  -> doの通常`<-`
  -> run後も同じErr
```

### 7.2 `OptionT<Result, A>`のSafeBind / failure matcher

`OptionT<Result, A>`のdo carrierはResultではなくOptionTだが、標準`OptionT`には
`@result_effect`がある。したがってdo内の`=?`がRHS `Err(error)`またはfailure matcher
不一致で失敗した場合、OptionTの`Alternative`よりResult effectを優先し、Errorをbase
Resultへ保持する。

```text
OptionT<Result, _> do内のSafeBind / failure matcher
  -> OptionT { inner: Err(error) }
  -> OptionT::run
  -> Err(error)
```

`guard(False)`だけは通常の`Alternative` callなので、`OptionT::empty`を使い
`OptionT::run`後に`Ok(Option::None)`となる。

### 7.3 その他の標準Transformer + Result

- `EitherT<L, Result, A>`は`@result_effect`も`Alternative`も持たないため、partial `<-`やdo内SafeBindのfailure matcherは拒否する。
- `ReaderT<E, Result, A>`と`StateT<S, Result, A>`はbase Resultが`Alternative`を持たないため、
  Transformer側も`Alternative`を得ず、同じ用途を拒否する。
- total `<-`だけのdoと、明示的にliftしたbase Resultの通常sequenceは利用できる。

拒否時にResult用のfailure保持へfallbackしたり、Transformer名から例外を作ったりしない。

## 8. 公開文書の変更先

### 8.1 `docs/site/language-guide.md`

Result / Optionの短い成功例を入口にする。`<-`、最終Monad式、carrier指定、通常assignmentとの違いを説明し、
詳細はlanguage referenceとerror handlingへリンクする。

### 8.2 `docs/site/language-reference.md`

構文、statement分類、scope、carrier推論、captured argument、Monad / Alternative条件、
SafeBindとの違い、成功・拒否境界を正本として記載する。

### 8.3 `docs/site/error-handling.md`

ResultContext-preserving doでfailure matcherのError messageを保持する場合と、Result effectのないAlternative-doで
failure payloadを破棄して`empty`へ変換する場合を対にして説明する。

### 8.4 `docs/site/monad-transformers.md`

標準4 Transformer、明示lift、OptionT Result effectの`Err(error)`保持、guardの`Ok(None)`、
EitherT / ReaderT / StateTのResult effect・Alternative不足を一つの節で比較する。

### 8.5 標準sourceの`@doc`

`lib/bootstrap.srt`の`do`文書には、REPLで実行できるResultまたはOptionの最小例を置く。
Transformer固有の説明は各型の既存`@doc`を正本とし、利用者向けページから参照する。

## 9. 例と説明の要件

- 公開するSurtr例は実際のparser、型検査、runtimeで実行して確認する。
- 成功例は観測結果まで示す。
- Result保持とAlternative emptyは、同じfailure条件を使った対の例にする。
- `OptionT<Result, _>`のSafeBind / partial `<-`による`Err(error)`保持と、guardによる`Ok(None)`を区別する。
- compile error例は、Result effectもAlternativeも持たないTransformer、base値の暗黙混在を含める。
- internal Rust型名やsynthetic closureをユーザが操作する概念として説明しない。
- `failure matcher`を独立した構文名として索引へ追加しない。pattern / SafeBind節から説明する。

## 10. 対象外

- `do`、SafeBind、MonadT、Alternativeの意味論変更。
- Resultへの`Alternative`追加。
- EitherTへの`Alternative`追加。
- base ResultのErrorをOption/Either等へ暗黙変換するAPI。
- Transformer内部構造に依存するcompiler特例。
- Generator再設計。

## 11. 受け入れ条件

- Result / Optionの1-kind carrierから`do`を理解できる。
- `do::<Carrier>`、推論、`<-`、`=?`、通常`=`、bare Monad式、最終式、scopeの違いが分かる。
- ResultContext-preserving doではSafeBind RHSのErrとfailure matcher由来Errorのkind・detail・messageが保持されると分かる。
- Result effectのないAlternative-doでは同じfailureがcarrierの`empty`となり、Error payloadを観測しないと分かる。
- canonical ResultはAlternativeなしでもpartial `<-`の共通pattern Errorを保持できると分かる。
- MonadT実装型でbase値の明示liftが必要だと分かる。
- `MonadT.OuterM`がResultの場合、OptionT SafeBind / partial `<-`のErr保持とguardの`Ok(None)`を区別できる。
- EitherT / ReaderT / StateT + ResultでResult effect・Alternativeが不足する拒否境界が分かる。
- user-defined Monad / Alternative carrierにも標準型名fallbackなしで同じ規則が適用されると分かる。
- 既存のユーザ向け文書間で説明が矛盾せず、削除予定の`doc/`入力仕様を正本として参照しない。

## 12. 検証

文書変更後は次を実施する。

1. 掲載する全コード例を対象のSurtr testまたは実REPLで実行する。
2. `lib/tests/do.srt`のResult、Option、MonadT、user-defined carrierの既存比較と説明を照合する。
3. ScarのAlternative不足、SafeBind、carrier mismatchの既存拒否テストと文言を照合する。
4. ResultContextのfailure message保持とOptionT ResultのSafeBind `Err(error)` / guard `Ok(None)`を、既存テストで直接確認できなければ
   文書作業とは別のテスト補強候補として報告する。意味論は変更しない。
5. Markdown link、削除予定入力への参照、`git diff --check`を確認する。

文書だけを変更する場合、compiler全体テストは実行しない。例または契約と現行挙動の不一致が見つかった場合は、
文書に合わせて実装を変更せず、現行仕様・実装・テストのどこが不一致かを報告して別タスクへ切り出す。
