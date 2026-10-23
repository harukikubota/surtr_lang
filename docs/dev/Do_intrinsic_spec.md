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

SafeBind の RHS 分解能力と、context の失敗返却能力は分けて検査する。RHS の型が canonical `MonadExtractor` の能力を持つ場合、次の順に評価する。

1. RHS を一度評価する。
2. 解決済みの通常の `MonadExtractor::extract` を一度呼ぶ。
3. 返った canonical Result の外側一段を判定する。Err は元 Error を失敗先へ渡し、Pattern と後続を実行しない。
4. Ok の成功値全体を通常の Pattern checker に渡す。照合成功なら束縛して続行し、不一致ならその Pattern の Error を失敗先へ渡す。

分解の有無は RHS の型と宣言済み能力で決め、LHS の total / partial や分解後の型検査の成否で切り替えない。元 RHS が Result でも標準 impl を迂回しない。成功値や子 Pattern に自動分解を再適用しない。通常関数内と do 内は同じ入力規則を使う。

MonadExtractor 未実装が確定した型は値全体を照合し、全体に適合する partial Pattern のみ受理する。全体型を受け取る total Pattern は `=` を案内して拒否する。未注釈 total 束縛で RHS が Monad の場合、または LHS が全体とは不整合で直下の mapped payload の入力を要求する場合は、分解入口の不足を案内する。子 Pattern の不一致、無関係な型不一致、Extractor の契約不一致は通常診断を維持する。総称型の bound 不足・推論未確定・trait 候補の不成立や曖昧さは具象型の未実装へ置き換えない。詳細は [diagnostics](diagnostics.md) に従う。

文末の `expression?` は、RHS を一度評価して MonadExtractor で分解し、失敗を現在の失敗先へ渡す。式の型は Unit とする。通常の SafeBind の一段分解とは区別し、元の RHS が canonical Result のときは Result の連鎖を再帰的に判定する。元の RHS 型に次の条件を課す。

- canonical Result の成功型は `UnitSuccess := Unit | Result<UnitSuccess>` とする。
- それ以外では、Monad の mapped payload 型自体が直接 canonical Unit であることを要求する。

この条件に加えて MonadExtractor の能力を要求する。非 Result の型条件を extract の返す Result から判定しない。内部の保存 field や container を探索せず、成功 slot は constructor slot mapping から求める。alias は通常の正規化に従う。

| 元の RHS 型 | 文末 `?` |
|---|---|
| `Result<Unit>`、`Result<Result<Unit>>` など Result のみが連続し終端が Unit | 受理 |
| `Option<Unit>`、`Either<Error, Unit>`、MonadExtractor 実装済み `M<Unit>` | 受理 |
| `Option<Option<Unit>>`、`Either<Error, Either<Error, Unit>>` | 拒否 |
| `Option<Result<Unit>>`、`Result<Option<Unit>>`、`M<Int>` | Unit 条件で拒否 |
| MonadExtractor 未実装の具象 `M<Unit>` | 分解入口の不足を案内 |
| Unit であることを確定できない型 | 既存の推論・型関係の診断で拒否 |

元の RHS が canonical Result の場合は、外側の通常の extract 呼出しを一度だけ行い、その返却 Result から各層の Ok の成功値を順にたどり、Err または Unit を終端として判定する。内側の確認は文末 `?` 固有の判定であり、extract を再呼出ししない。どの層でも Err に到達したら元 Error を同じ現在の失敗先へ渡し、後続を実行しない。全層が Ok で終端の Unit に到達した場合だけ後続へ進む。`Ok(Err(error))?` は内側の元 Error を伝播する。
非 Result の場合は直接の Unit payload を処理し、`Option<Result<Unit>>` などへこの再帰判定を広げない。通常の SafeBind の成功値や子 Pattern にも再適用しない。
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
partial `<-` は `Monad::bind` が渡した payload 全体を照合し、その payload が MonadExtractor 実装型でも
extract を呼ばない。`Ok(x) <- [Ok(1), Err(NoneError())]` は成功要素だけを残す。
do 本文内の `apply_pattern` は自身の `Result` を返す式であり、外側 carrier の failure target を使わない。
各binding/continuationの実行ごとにRHSを一度評価し、failureとなった経路の後続continuationを実行しない。
List等の分岐carrierでは、後続continuationを各payloadについて実行する。

### HashMap Pattern

`hash![key => child, ...]` の非空 Pattern は partial なので、do の `<-` と HashMap 全体を入力にする SafeBind で使える。`<-` は carrier の payload 全体を照合する。SafeBind は MonadExtractor 実装型の extract 後に返る Result の外側一段を処理し、その Err ではキー式を実行せず元 Error を失敗先へ渡す。map 内の値には自動分解を追加しない。

空の `hash![]` は total とし、通常 Bind と total `<-` で使える。SafeBind では MonadExtractor により `HashMap<V>` を取り出す RHS は受理し、未実装の HashMap 全体への total 束縛は `SafeBindTotalPatternNonExtractingRhs` で拒否する。キー欠落は `HashMapKeyMissing`、子の不一致は子自身の Error を使い、MonadFail route は情報を保持、Alternative route は破棄する。照合はエントリの記述順に進み、最初の失敗以降のキー式・子 Pattern・continuation は未評価とする。型・scope・位置の正本は [HashMap Pattern](Pattern_spec.md#hashmap-の構造的-pattern) を参照する。

### Error Pattern と Payload の保持

SafeBind と do partial `<-` では Error 定義 Pattern による直接ダウンキャストを拒否する。`Err(error)` など Error 全体の運搬と通常 Extractor は既存規則に従う。局所具象束縛・readonly・Payload の保持は [Error spec](Error_spec.md) を参照する。RHS は本書の MonadExtractor による一段分解に従う。MonadFail > Alternative > Monad と ResultT の内側 Result 失敗層は維持する。

## Phase ownershipとlowering

- Spire: token、RTA、statement分類、pattern/operator/source span。parserではlowerしない。
- Sigil: canonical identity、surface検証、RHS-first scope、captureとsource origin。
- Scar: carrier / obligation 推論、pattern 検査、failure target、具体化済み bind / extract / fail / empty dispatch。文末 `?` の元 RHS 型に対する Unit 条件も検査する。
- Forge: concrete closure、block、match、typed SafeBind control を既存命令へ lower する。SafeBind は解決済みの通常の extract call の返す Result を処理し、元 RHS の型名やレイアウトから extract を推測しない。
- Eldr:既存命令の実行。carrier推論、candidate探索を行わない。

typed SafeBind は解決済み extract call と一段の Result 判定、success / failure、effective target、result span 等の元 source origins を保持する。文末 `?` の canonical Result 連鎖は型検査で確定した深さを保持し、Forge はその深さに従って Err / Ok / Unit を判定する。元の RHS の型名・runtime 値から深さを推測しない。具体化・substitution・function index・trait dispatch の走査でも extract call を処理する。
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
