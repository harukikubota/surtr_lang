# `do` intrinsic 実装契約

現行の構文・型検査・loweringの正本。利用者向けの例は[do](../site/do.md)、
型入力・Trait解決は[Trait system](./Trait_system_spec.md)、診断は[diagnostics](./diagnostics.md)を参照する。
実装計画や過去の検証ログはGit履歴に残し、未採用仕様を現行契約へ混ぜない。

既知の実装不整合: REPLのpartial `<-`で合成したErrorのkind/messageが崩れる。
契約を変更せず、[フォローアップ仕様](../../doc/do_result_effect_repl_error_followup_spec.md)で追跡する。

## Surfaceとscope

`do { ... }`、`do::<_> { ... }`、`do::<Carrier> { ... }`を受理する。
RTAは既存のcall-site parserを使い、一項のbare constructor head、完全・部分型application、`_`を保持する。
空RTA list、過剰項、`do<...>`、外側constructor variableを直接指定するRTAは拒否する。
generic contextではRHSや期待型からrigid constructor variableへ統一できるが、`do::<$F>`を別surfaceとして追加しない。

文はExtract (`pattern <- rhs`)、SafeBind (`pattern =? rhs`)、通常statementへ分類する。
separatorは改行または`;`。空block、末尾binding、最終Monad式不足はparse errorとする。
blockはchild scopeを持ち、RHSをLHS bindingより先に解決し、pattern名を後続文だけへ公開する。
capture、warning、ID rebase、Facet bulk_update等のvisitorはdo内部にも再帰する。
bulk_updateの`<-`とは構文所有者で区別し、通常callへ曖昧にfallbackしない。

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
impl数・順序・表示名・field探索による逆推論、暗黙lift、runtime Trait dictionaryを認めない。
未確定obligationは`Deferred`を保持し、実行境界では構造化ambiguity等として拒否する。

## Patternとfailure target

Monadは常に要求する。total `<-`はMonadだけでsequenceする。
partial `<-`と合法なSafeBindのfailure targetは共通FailureEffectで、
`ResultEffect > Alternative > Monad`の順に選択する。

- canonical Resultまたは有効な`@result_effect` carrier: Errorを保持して最終carrierを構築する。
- Result effectなし、Alternativeあり: resolved `Alternative::empty`へ接続する。
- どちらもなし: capability error。Monad単独ではfailure targetを構築しない。

`@result_effect`の宣言検証と直接baseの条件はTrait system正本に従う。
標準適用対象はOptionTのみ。内部にResultがあるという理由ではeffectを付与しない。
`guard`はこの選択に参加せず、常に通常のAlternative callである。
nested doやdo内の別callableはそれぞれ自身のfailure contextを持ち、外側からeffectを借りない。

SafeBindはcanonical Resultの外側一段だけを射影する。
non-Result RHSは値と型全体を通常MatchBlock検査へ渡し、partial patternだけを受理する。
static pattern errorを先に報告し、型関係が成立するtotal non-Resultは既存の二SafeBind reasonで拒否する。
Result-effect return targetでもTransformer RHSを自動unwrapしない。

Result-preserving routeはRHS Err、既存pattern Errorのkind/message/location/causeを保存する。
Extractorは現行のOption返却で、Noneは共通PatternMismatchとなる。独自Err返却契約は追加しない。
partial `<-`のno-matchは共通pattern ErrorをResult effectで保持し、Alternative routeではemptyにする。
各binding/continuationの実行ごとにRHSを一度評価し、failureとなった経路の後続continuationを実行しない。
List等の分岐carrierでは、後続continuationを各payloadについて実行する。

## Phase ownershipとlowering

- Spire: token、RTA、statement分類、pattern/operator/source span。parserではlowerしない。
- Sigil: canonical identity、surface検証、RHS-first scope、captureとsource origin。
- Scar: carrier/obligation推論、pattern検査、failure target、具体化済みbind/empty dispatch。
- Forge: concrete closure、block、match、専用typed SafeBind controlを既存命令へlowerする。
- Eldr:既存命令の実行。carrier推論、candidate探索を行わない。

typed SafeBindはsuccess/failure、effective target、result span等の元source originsを明示して保持する。
synthetic closureの暗黙return先へ依存せず、現在のdo continuationのcarrierへfailureを接続する。
Forge前にpending carrier、dispatch、未具体化callableを拒否する。
do専用opcode、runtime候補探索、旧failure経路、compatibility fallbackを残さない。

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
- 言語の観測結果: `lib/tests/do.srt`、`lib/tests/monad_transformers.srt`。
- script拒否境界: `tests/fixtures/script/fail/typecheck/do_*`。
- human/JSON source facts: Rune binaryの`do_return_diagnostic_keeps_the_same_source_facts_in_human_and_json`。

全体の選択規則は[テスト方針](./テスト方針.md)に従う。
