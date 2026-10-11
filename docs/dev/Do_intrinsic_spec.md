# `do` intrinsic 実装契約

現行の構文・型検査・loweringの正本。利用者向けの例は[do](../site/do.md)、
型入力・Trait解決は[Trait system](./Trait_system_spec.md)、診断は[diagnostics](./diagnostics.md)を参照する。
実装計画や過去の検証ログはGit履歴に残し、未採用仕様を現行契約へ混ぜない。

## Surfaceとscope

`do { ... }`、`do::<_> { ... }`、`do::<Carrier> { ... }`を受理する。
RTAは既存のcall-site parserを使い、一項のbare constructor head、完全・部分型application、`_`を保持する。
空RTA list、過剰項、`do<...>`、外側constructor variableを直接指定するRTAは拒否する。
generic contextではRHSや期待型からrigid constructor variableへ統一できるが、`do::<$F>`を別surfaceとして追加しない。

文はExtract (`pattern <- rhs`)、SafeBind (`pattern =? rhs`)、通常statementへ分類する。
文末の `?` は独立した通常statementとして保持し、式のpostfix演算子にはしない。
式全体の末尾だけに付けられ、複数行callも受理する。binding RHS、引数、演算途中、`??` は拒否する。
式の区切りには改行を使う。`;` は通常 block と同じ Stmt として式を Unit 化し、carrier を含む値を破棄する。
その式の Err は伝播しない。空block、末尾binding、最終Monad式不足はparse errorとする。
blockはchild scopeを持ち、RHSをLHS bindingより先に解決し、pattern名を後続文だけへ公開する。
capture、warning、ID rebase、Facet bulk_update等のvisitorはdo内部にも再帰する。
bulk_updateの`<-`とは構文所有者で区別し、通常callへ曖昧にfallbackしない。

関数本体の `def ... -> Ty = do { ... }` でも通常の do 式と同じ契約を使う。関数の戻り値型を期待型として carrier を決め、本文や明示 RTA との衝突を通常の型エラーとして拒否する。関数本体専用の推論・lowering 経路は設けない。関数本体の構文規則は[言語リファレンス](../site/language-reference.md#関数)を参照する。

## Compiler-owned contract

Sindrの`IntrinsicId::Do` / `DoIntrinsicContract`が標準surfaceとloweringのidentityを所有する。
`Bootstrap::do`の一項のMonad RTA、`DoBlock<$Result>` parameter、Monad return、反復payload関係、
canonical Trait / method / Result identityをSigilで構造比較する。
raw display signatureを再解析してcallable schemeやintrinsic identityを作らない。

`DoBlock`はintrinsic signature専用markerで、通常のfirst-class valueではない。
user declaration、inherent/Trait impl target、通常parameter/return/field/annotationでの利用を用途別に拒否する。
標準sourceから新しいmarker identityを生成しない。

## Carrierと型推論

ReturnTypeArgument position 0へ結び付く一つのdo-local carrierをinstantiateする。
明示RTA、expected result、`<-` RHS、bare Monad式、最終式、通常callのsignature制約を共通solverへ渡す。
全monadic originはconstructor head、arity、mapped slots、captured/fixed argumentsが一致する必要がある。
payload型は文ごとに変化できる。family所属だけからcarrier同一性を導出しない。

`=?` RHSはpattern inputであり、do carrierの推論元ではない。
普通の`=`はpayloadを取り出さず、bare Monad式はpayloadを捨ててsequenceする。
`pure` / `return` / `guard`は通常callとして検査し、名前でcheckerの分岐を作らない。

返り値にだけ現れるconstructor入力は、通常callの型引数の具体化結果を使ってcapabilityを検査する。
たとえば`Alternative::guard::<List>(...)`の返り値は`List<Unit>`としてMonadの実装を確認できる。
値引数から受け継いだcapabilityは、同じnominal型へ具体化されても強めない。
`<-` のpayloadも、通常のgeneric callback引数と同じく入力値の由来を保持する。
生成したcallbackの仮引数へ由来を渡してから本文を検査し、束縛パターンの型注釈や
tuple・list・Extractorによる取り出しで能力制約を失わない。
Facet の読み取りが返した `Result` も通常の `Result` として `<-` に渡せる。
Facetの読み取りでは外側のcarrierと成功payloadのcapabilityを区別し、
読み取り自体でpayloadの制約を強めない。
`List<Unit>`のbare式と`_ <- rhs`は通常の`Monad::bind`でsequenceし、空Listでは後続文を評価しない。
impl数・順序・表示名・field探索による逆推論、暗黙lift、runtime Trait dictionaryを認めない。
未確定obligationは`Deferred`を保持し、実行境界では構造化ambiguity等として拒否する。

## Patternとfailure target

Monadは常に要求する。total `<-`はMonadだけでsequenceする。
partial `<-`と合法なSafeBindのfailure targetは共通FailureEffectで、
`MonadFail > Alternative > Monad`の順に選択する。

- canonical `MonadFail` を実装する carrier: 通常の `MonadFail::fail(error)` 呼出しで元の Error を保持する。
- MonadFailなし、Alternativeあり: resolved `Alternative::empty`へ接続する。
- どちらもなし: capability error。Monad単独ではfailure targetを構築しない。

`MonadFail` の constructor shape と payload slot は親 `Monad` から引き継ぐ。失敗先は返り型・期待型・宣言した generic bound から決定し、SafeBind の RHS は推論元にしない。通常 callable では `MonadFail` のみを使い、do 内だけで `Alternative::empty` を次の候補とする。metadata の不正や曖昧な dispatch を Alternative への切替えで隠さない。
標準 MonadFail 宣言の欠落・同名の非標準宣言も契約エラーとし、carrier の能力不足とは区別する。
OptionT は標準 MonadFail を持たず、do の Pattern failure は Alternative で処理する。base の Err は bind 自身が保持する。
`guard`はこの選択に参加せず、常に通常のAlternative callである。
nested doやdo内の別callableはそれぞれ自身のfailure contextを持ち、外側からeffectを借りない。

SafeBind自体にはMonadFail / AlternativeのTrait requirementを持たせず、失敗を扱うcontextが能力を検査する。
SafeBindはcanonical Resultの外側一段だけを射影する。
non-Result RHSは値と型全体を通常MatchBlock検査へ渡し、partial patternだけを受理する。
static pattern errorを先に報告し、型関係が成立するtotal non-Resultは既存の二SafeBind reasonで拒否する。
MonadFail の返り先でもTransformer RHSを自動unwrapしない。

文末の `?` の対象型は canonical `Result<UnitSuccess, E>` とする。
`UnitSuccess := Unit | Result<UnitSuccess, E>` で、alias は通常の正規化を行い、
終端非 Unit、non-Result、終端成功型が未確定の場合は型エラーにする。
構文固有の型制約を検査してから、既存の `_ =? rhs` と同じ外側一段の射影へ接続する。
対象式を一度だけ評価し、成功値を捨てて Unit 文とする。
`Ok(Err(error))` の内側の Err を再伝播しない。
do 内では既存 do-local failure target、do 外では最も近い callable 自身の SafeBind target を使う。
match arm などの通常 Block は新しい target を作らず、成立しない位置は拒否する。
closure に入ると自身の期待返り型、または自身の未確定返り型へ切り替える。外側 callable の target を継承せず、未確定 target の SafeBind / 文末 `?` は拒否する。
carrier の推論元にせず、末尾の Unit を最終 Monad 値へ暗黙 wrap しない。
文末 `?` 自体の型は Unit として扱い、Result や最終 Monad 値が必要な位置では通常の型検査で拒否する。
ブロック末尾の `?` を一律に禁じる構文規則は追加しない。
optional 型や FacetPath optional segment の既存 `?` と構文所有者を区別し、fallback は設けない。

MonadFail route は RHS Err と Pattern failure の元 Error を保持し、Alternative route は破棄する。情報保持と生成位置は [Error spec](Error_spec.md) に従い、`<-` の位置へ置き換えない。Extractor / ExtractorClosure 本文内でも do の failure は do-local target に接続し、外側 MatchResult へ直接 return しない。
標準 MonadFail は Result、Either<Error, A>、ResultT、EitherT<Error, M, A>、および base が MonadFail の ReaderT / StateT に提供する。
MonadFail を持たない Option / List / OptionT 等は現行の Alternative route を維持し、Extractor Error を破棄する。
partial `<-` は `Monad::bind` が渡した payload 全体を照合し、その payload が `Result` でも
SafeBind の外側一段の自動分解を追加しない。`Ok(x) <- [Ok(1), Err(NoneError())]` は成功要素だけを残す。
do 本文内の `apply_pattern` は自身の `Result` を返す式であり、外側 carrier の failure target を使わない。
各binding/continuationの実行ごとにRHSを一度評価し、failureとなった経路の後続continuationを実行しない。
List等の分岐carrierでは、後続continuationを各payloadについて実行する。

### HashMap Pattern

`hash![key => child, ...]` の非空 Pattern は partial なので、do の `<-` と non-Result RHS の SafeBind で使える。`<-` は carrier の payload 全体を照合する。SafeBind は canonical Result の外側一段だけを射影し、RHS の Err ではキー式を実行せず元 Error を失敗先へ渡す。map 内の Result 値には自動射影を追加しない。

空の `hash![]` は total とし、通常 Bind と total `<-` で使える。SafeBind では `Result<HashMap<V>>` の RHS は受理し、non-Result の HashMap は total non-Result の既存分類で拒否する。キー欠落は `HashMapKeyMissing`、子の不一致は子自身の Error を使い、MonadFail route は情報を保持、Alternative route は破棄する。照合はエントリの記述順に進み、最初の失敗以降のキー式・子 Pattern・continuation は未評価とする。型・scope・位置の正本は [HashMap Pattern](Pattern_spec.md#hashmap-の構造的-pattern) を参照する。

### Error Pattern と Payload の保持

SafeBind と do partial `<-` では Error 定義 Pattern による直接ダウンキャストを拒否する。`Err(error)` など Error 全体の運搬と通常 Extractor は既存規則に従う。局所具象束縛・readonly・Payload の保持は [Error spec](Error_spec.md) を参照する。Result 専用の RHS 一段射影、MonadFail > Alternative > Monad、ResultT の内側 Result 失敗層は本書の規則を維持する。

## Phase ownershipとlowering

- Spire: token、RTA、statement分類、pattern/operator/source span。parserではlowerしない。
- Sigil: canonical identity、surface検証、RHS-first scope、captureとsource origin。
- Scar: carrier/obligation推論、pattern検査、failure target、具体化済みbind/fail/empty dispatch。
- 文末 `?` の canonical Result / 終端 Unit 制約も Scar で検査し、既存 typed SafeBind control へ接続する。
- Forge: concrete closure、block、match、専用typed SafeBind controlを既存命令へlowerする。
- Eldr:既存命令の実行。carrier推論、candidate探索を行わない。

typed SafeBindはsuccess/failure、effective target、result span等の元source originsを明示して保持する。
synthetic closureの暗黙return先へ依存せず、現在のdo continuationのcarrierへfailureを接続する。
Forge前にpending carrier、dispatch、未具体化callableを拒否する。
do専用opcode、runtime候補探索、旧failure経路、compatibility fallbackを残さない。

生成したcontinuationは通常のBlock / Closure / TraitCallとして検査する。
共通visitorの再帰フレームへ宣言処理や大きな値・診断の一時領域を積み重ねず、
外側の通常blockやclosureを含む正常な多段の式を処理する。
Test DSLやListだけの特例、compilerのstack増量による再試行は設けない。

## 診断と検証配置

通常のuser match/if内部のbranch mismatchは固有診断を優先する。
式全体の型が決まった後のdo carrierとの衝突だけを共通carrier診断にする。
synthetic matchはexhaustiveで、生成branchにuser exhaustiveness診断を出さない。

RTA/expected/RHSの衝突は元の二地点source factsを保ち、partial `<-`はpattern、
SafeBind能力不足は`=?`、末尾return不一致は保存済みresult spanを示す。
humanとJSONは同じreason/origin/typed data/source factsから投影する。
JSON schemaはdiagnostics正本の閉じたvariantを使い、未採用のdo専用fieldや自由形式mapを追加しない。

直接の検証先は次で、同じ契約を各層に重複させない。

- Sindr: `crates/sindr/tests/do_intrinsic_contract.rs`。
- Spire/Sigil:各crateのsyntax、RTA、reserved marker、scope/captureテスト。
- Scar: `crates/scar/tests/typecheck_surface.rs`のcarrier、capability、SafeBind、Facet、diagnostic境界。
- 言語の観測結果: `lib/tests/language_features/do.srt`。Transformerの観測は `lib/tests/monads/option_t.srt`、`either_t.srt`、`reader_t.srt`、`state_t.srt`（同じディレクトリ）で検証する。
- script拒否境界: `tests/fixtures/script/fail/typecheck/do_*`。
- human/JSON source facts: Rune binaryの`do_return_diagnostic_keeps_the_same_source_facts_in_human_and_json`。

全体の選択規則は[テスト方針](./テスト方針.md)に従う。
