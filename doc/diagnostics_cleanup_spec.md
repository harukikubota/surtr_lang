# SafeBind 是正・診断統合残作業・do 開始条件

## 1. 状態と根拠

- 分類: `/doc` に置く、実装済みN06の入力仕様・履歴。
- 基準 commit: `f1986a27d84728e1e7a88de457a1d6b55cfe5ab8`。
- 入力: Git履歴に残る旧計画のTask 10、[`../docs/dev/diagnostics.md`](../docs/dev/diagnostics.md)に残る未実装契約、既存[`do_intrinsic_spec.md`](do_intrinsic_spec.md)。
- 旧 Task 9 は完了済み。reason/origin/typed data の基盤を作り直さない。
- N01–N05完了後のN06実装入力。§§3–5のSafeBind RHS射影・pattern・failure target契約、§§6–9のdiagnostic family、heuristic撤去、builtin metadata統合まで実装済みである。
- level4（型・評価規則とフェーズ間契約の変更）。本書整理時は文書検証のみ行い、実装時に全体検証と独立レビューを行う。
- Extractorは現行の`Option<T>`返却を維持する。

SafeBind RHS訂正は本書、恒久文書、標準`@doc`、実装とテストへ反映済みである。旧記述の「すべてのnon-Result RHSを
pass-throughする」「total patternでOption全体を束縛できる」は実装入力として使用しない。訂正元の一時提案書は、
この移管完了により削除した。

基準commitの旧計画は、Task 9での修正・検証記録と、SafeBind是正・残存familyのheuristic撤去をTask 10へ残すことを記録している。本書はその残作業だけを引き継ぐ。記録されたテスト結果は過去の実行記録であり、本書作成時の再実行結果ではない。

最新の文書整理方針を優先し、旧Task 10にある旧ドラフト削除手順は引き継がない。ドラフトは `/doc` に本文変更なしで残す。

## 2. 設計意図

SafeBindの入力射影・pattern失敗・failure targetを明示し、doが同じ契約を再利用できるようにする。

診断はproducerが作ったstructured reason・origin・typed dataから生成する。表示文章やsource文字列から型関係を再構成するfallbackを撤去する。

この作業はMonadT専用ではない。通常関数、標準型、ユーザ定義型、既存のpolicy診断を対象にする。

## 3. SafeBindのRHS

`pattern =? rhs` はRHSを一度だけ評価する。RHSがcanonical Resultなら外側一段のpayload、
それ以外ならRHS全体を通常MatchBlock checkerへ渡し、static pattern検査を先に完了する。

| LHS | RHS | patternへの入力 | static pattern検査後の判定 |
|---|---|---|---|
| total / partial | canonical `Result<A,E>` のOk(a) | a:A | 通常pattern検査へ進む |
| total / partial | canonical `Result<A,E>` のErr(e) | patternを実行しない | eをfailure targetへ渡す |
| partial | Result以外のT | RHS全体:T | 通常pattern検査とno-match failureを使う |
| total | Result以外の非Monad T | RHS全体:T | pattern型関係が成立した場合、SafeBind固有の非Monad compile error |
| total | Result以外のMonad M | RHS全体:M | pattern型関係が成立した場合、SafeBind固有の非Result Monad compile error |

canonical Resultの外側一段だけを分解する。Result<Option<A>>からはOption<A>、Result<Result<A>>からはResult<A>を渡す。再帰unwrapを行わない。

Option、List、ユーザ定義enum/structに対するpartial patternは通常のMatchBlock checkerへ渡す。
Someを自動的に分解したり、constructor patternをOkだけに限定したりしない。一方、total patternへ
non-Result値全体を束縛することは許可しない。

```surtr
# partial patternはOption全体を明示的に検査する。
def take_some(value: Option<Int>) -> Result<Int> {
  Option::Some(saved) =? value
  Ok(saved)
}

# Option<Int>とIntの通常pattern型不一致。SafeBind固有reasonにはしない。
def mismatch(value: Option<Int>) -> Result<Int> {
  saved: Int =? value
  Ok(saved)
}
```

`num: Int =? Option::Some(10)`も同じ通常TypeErrorであり、Result-only分解のmessage / note / helpを付けない。
static pattern検査を通過した`value =? Option::Some(10)`は「Result以外のMonad」、`value =? 10`は
「非Monad」のSafeBind固有診断にする。non-Resultのpartial patternがruntimeに一致しない場合は、
通常のpattern / Extractor failure契約に従う。container名別の新しいfailure生成規則を加えない。

### 3.1 現行との差分と維持する境界

現行ScarにはOption RHSの先行拒否とSafeBind constructor patternの`Ok`限定がある。N06で両方を撤去し、
射影後の入力を通常MatchBlock checkerへ渡す。`num: Int =? Option::Some(1)`は変更後も通常の静的型不一致で拒否し、
SafeBind固有診断を重ねない。`saved =? Option::None`はpattern型関係が成立しても、total patternとResult以外のMonadの
組としてSafeBind固有診断で拒否する。

SafeBind固有reasonは次の二つだけを追加する。前提はLHSがstructurally total、RHSがnon-Result、
かつ通常pattern型関係が成立していることである。

1. non-Result RHSにcanonical Monad capabilityがない。
2. non-Result RHSにcanonical Monad capabilityがある。

Monad proofが`Deferred`なら早期分類せず、既存solverのboundaryまで保持する。Optionという表示名、impl数・登録順、
自然言語messageから分類しない。両診断は「`=?`が分解できるRHSはcanonical Resultの外側一段だけ」を
error message本体に含める。現行Option拒否headlineは2のOption向け表示で維持し、Result-only文を追記してよいが、
`Option::to_result(value, err)`、`from::<Result>(value)`などの変換helpは出さない。

RHS射影と明示pattern分解を区別する。nested Resultの内側も通常のscrutineeであり、
constructorの名前やpayloadがResultであることを理由に追加の自動射影を行わない。
現行の`TypedPattern::ResultOk`による内側Err伝播も撤去対象である。`Ok(x) =? Ok(Err(error))`は、
外側Okを射影した後、内側ErrがOk patternに一致しないため`PatternMismatch`となり、内側errorを伝播しない。
`Err(e) =? Ok(Err(error))`は通常constructor照合として成功し、eへerrorを束縛する。
二段ともErrを伝播したい場合は`inner =? rhs`、`value =? inner`の二文に分ける。
静的な型不一致・constructor arity・Extractor返却型の違反をruntime no-matchやfailure targetへ落とさない。
型検査後の未知tag等の内部契約違反もno-matchとして扱わない。

現行Extractorは`Option<T>`を返し、`Some(payload)`を子patternへ渡し、`None`をno-matchとする。
Extractorが独自の`Err(error)`を返す契約を導入しない。SafeBindではno-matchを既存のpattern failureへ変換するのであり、
Extractorの返却値からErrorを取り出すのではない。通常matchは引き続き次のarmへ進む。

各RHSは一回、各Extractor occurrenceは到達したとき一回だけ評価し、成功検査と束縛で再実行しない。
既存MatchBlockの段階的評価順・payload arity・pin/as-pattern・scopeを維持し、失敗後の子patternと後続文を評価しない。
Facetのcompile-time値を`=?`で束縛する禁止、nearest callableへの早期return、closureのcanonical ResultまたはResult effect戻り値要求、
REPL top-levelで診断後にセッションを継続する境界も維持する。partial non-Result pass-throughはこれらの独立したpolicyを解除しない。

## 4. failure target

do外のSafeBindは、既存のenclosing functionのcanonical `Result` または検証済み `@result_effect` carrier return targetに失敗を接続する。RHSのError、patternが生成する失敗、期待Error型の関係を共通の型関係検査で確認する。

typed IRには少なくとも次の意味を保持する。Rustの最終型名を新規の別体系として強制するものではない。

```text
SafeBindRhsProjection:
  UnwrapCanonicalResultOnce(payload_type, error_type, result_identity)
  PassThroughNonResultPartial(pattern_input_type)

SafeBindControl:
  rhs
  pattern
  rhs_projection
  success_continuation
  failure_target
  source_origins
```

Forgeが `in_function`、戻り値の表示名、元のsource構文からfailure targetを推測してはならない。

total non-Resultはtyped controlへ到達する前にcompile errorとなる。do導入時は、N06で合法と確定したSafeBindについてだけ
現在のdo continuationの結果型へtargetを明示的に差し替える。通常SafeBindの成功/失敗判定を再実装しない。

## 5. do側で維持する規則

| 操作 | ResultContext-preserving do | Result effectのないdo |
|---|---|---|
| total patternの `<-` | Monad | Monad |
| partial patternの `<-` | Errorを保持 | Alternativeがあれば`empty`、なければcapability error |
| 合法な`=?` の失敗 | 既存のErrorを保持 | 同じcarrierのAlternative::emptyへ置換 |
| 通常のguard | 通常のsignatureに従う | 通常のsignatureに従う |

failureMatcher/partial `<-` と SafeBind は共通の ResultContext を使う。Result effect があれば Error を保持し、なければ Alternative::empty、どちらもなければ capability error とする。ResultT/EitherT/StateT等の名前やrepresentationを見て特例を増やさない。

failure target の ResultContext は `ResultEffect > Alternative > Monad` の順で解決する。Monad 単独は sequencing capability
であり、SafeBind/failureMatcher の failure target にはならない。`guard` は Result effect を参照しない通常の Alternative call である。

SafeBind RHSをdo carrierの推論源にしない。carrier確定前に失敗方針が必要ならDeferredにし、候補数からResult/Option等を選択しない。
total non-Result RHSはdo carrierの種類やAlternative capabilityにかかわらず、§3.1の二分類で先に拒否する。

## 6. 診断の残作業

Task 9で移行済みのsignature、Trait、operator/helper、branchの経路は再利用する。

未移行の以下のfamilyを、phase固有のerror型を保ったままstructured producerへ移す。

- parserの構文・位置規則。
- resolverの名前・namespace・visibility・import・capture。
- pattern/Extractor/exhaustiveness/assignment。
- Error、Facet、Process/Task、source/compileのpolicy。
- runtime value errorとcompiler invariant。

source/function/type関係で表せる部分は共通のtyped dataを使い、policy固有の意味は専用reasonとして保持する。全errorを無理に一つのenumへまとめない。

SafeBindについては、通常pattern type relation failureと、§3.1のtotal non-Result二reasonを別producer dataとして保持する。
rendererはpattern型不一致にSafeBindのResult-only説明を足さず、二reasonにだけRHS分解能力の境界を表示する。

## 7. renderer・JSON・adapter

rendererはreasonとclosed typed dataからmessage/labels/notes/helpを構築する。source registryは表示用のsource解決に使うが、source文字列の探索で意味を判定しない。

structured inputのoptional fieldが空でも、message解析へ戻らない。情報欠落はproducer側の契約またはinvariantとして扱う。

AriadneとJSONは同じfailureを入力とする。既存のphase/kind/span/expected/got/hintの意味を保ち、追加reason/origin/data/relatedをRuneとXldrがそのまま渡す。

内部の型IDやsynthetic closureの位置を利用者へ漏らさない。元のexpression、operator、pattern、宣言へのsource originを保持する。

## 8. heuristicの撤去

最後のproducer移行後に、意味判定を文章から行うheuristicと `UnmigratedMessage` 等の暫定経路を撤去する。

禁止するのは、型の表示文字列・自然言語message・label marker・source textを根拠に、reason/type/candidate/dispatchを再決定すること。

文字列APIの利用自体を全面禁止するわけではない。純粋な表示整形・無関係な文字列操作・parserの字句処理と、診断の意味推論を区別して監査する。

### 8.1 builtin surface metadataの残作業

qualified builtin surfaceは`crates/sindr/src/builtin.rs`の`BUILTIN_METAS.surfaces`を正本とし、
owner/name/runtime targetとcanonical signatureを構造データから解決する。compiler-generated declarationは
通常source surfaceへ混入させず、同じmetadata entryの`compiler_generated_surfaces`で正確なowner/nameだけを登録する。
未知ownerを拒否するfail-closed境界を維持し、旧`builtin_runtime_name_for_qualified`の明示allowlistと
`surface_variant_named`によるvariant合成は撤去した。

追加・変更の起点を`BUILTIN_METAS`に一本化し、任意ownerのvariantを合成するfallbackや、表示名・登録順から
runtime targetを選ぶ経路は設けない。source `@builtin def`はcanonical surface signatureとの完全一致検証、
`@doc`、provenanceだけを担当する。この残作業は旧fallback是正資料から引き継ぐもので、完了済みTask 9の
diagnostic reason基盤を作り直す理由にはしない。

## 9. remediation overlay

変換や修正案はcanonicalなsource/target identity、visibility、関連する通常のFrom/TryFrom情報から生成する。

helpのための候補照会は、型推論の成功条件やdispatch結果を変えてはならない。候補が複数、不可視、根拠不足の場合は一般的な案内に留める。未確定型を「唯一の変換があるから」具体化しない。

§3.1のSafeBind二reasonには変換helpを生成しない。通常のremediation候補から`Option::to_result`や`From<Result>`を
探索して追記することも禁止する。

## 10. do開始条件

本書の完了と、進捗管理ファイルが指定する前提タスクの完了を記録する。

最低限、次を満たす。

1. 共通の型入力・static dispatch経路が完成し、Forgeへ未解決carrier/dispatchを渡さない。
2. SafeBindの一段Result射影、partial non-Result pass-through、total non-Result二診断、通常pattern TypeError優先がfocused testで固定される。
3. 残存診断familyがstructured producerへ移行し、意味推論heuristicが残らない。
4. Rune/Xldrでstructured情報が欠落せず、エラー後のREPL継続を確認する。
5. 新しいcarrier同一性仕様を反映し、do-local同一性の要件が通常関数と衝突しない。
6. 既存計画の「do開始前に全体検証を2回連続成功」の条件を維持する。

このゲートより前にdoのlexer/parser/AST/resolver/checker/lowering/標準宣言を追加しない。ゲート通過時には、do未導入と検証結果を同じrevisionで記録する。

## 11. 文書移管・監査

旧診断統一入力の実装済み部分は[`../docs/dev/diagnostics.md`](../docs/dev/diagnostics.md)等へ移管済みであり、残作業は本書が引き継ぐ。

旧資料が全て完了していたと仮定して削除しない。各節を実装済み・本書へ移管・do仕様へ移管に対応付け、移管先を確認してから削除する。

ドラフトは内容を変えず残す。旧用語の検索では、draftや保存された入力メモを正本のゼロ件監査対象に含めない。draft内の歴史的リンクを理由にdraft本文を書き換えない。

### 11.1 N06の変更先と作業順

1. Scarの`checker/expr.rs`、`checker/patterns.rs`と通常MatchBlock経路を照合し、RHS射影、static pattern検査優先、totality、Monad proof、failure型関係を固定する。
2. typed IRとForgeの各failure emitterを明示targetへ接続する。旧SafeBind専用pattern経路を互換fallbackとして残さない。
3. §§6–9のphase producer、renderer、Rune/Xldr adapter、builtin surface metadataを移行し、最後にheuristicを削除する。
4. `option_safebind_rejected`等の旧fixtureは、total Option RHSをResult以外のMonadとして拒否する期待へ更新する。
   partial Option patternの成功、total scalarの非Monad拒否、`num: Int =? Option::Some(10)`の通常TypeErrorを追加する。
5. `要件定義v9.md` §§2.3・3.1、`../docs/dev/{diagnostics,テスト方針}.md`、
   `../docs/site/{language-reference,error-handling,function-operators}.md`のSafeBind・pattern関連説明と
   `../lib/bootstrap.srt`の`@doc`を実装に合わせて更新する。ExtractorのOption返却規定は維持する。
6. DC条件を実テストへ対応付け、focused / REPL検証後、同一revisionでCI workspaceを2回連続成功させる。
   `cargo run -- test --quiet --all`、独立レビューと文書移管を含めてN06完了を記録し、N07へ渡す。

N06ではdo宣言・DoBlock・DoIntrinsicContractを追加しない。N10は完成したSafeBindのfailure targetを
do continuationへ差し替える作業であり、N06のRHS/pattern是正を再実装しない。

## 12. 受け入れ条件

| ID | 検証 |
|---|---|
| DC-01 | Result RHSの成功・失敗と一段だけのprojection |
| DC-02 | pattern型関係が成立するtotal non-Resultを、非Monad / Result以外のMonadの二reasonで拒否し、Result-only分解を表示する。変換helpは出さない |
| DC-03 | partial non-Result pass-throughと通常のpartial pattern・Extractor・annotation mismatch。通常TypeErrorへSafeBind固有説明を混ぜない |
| DC-04 | RHS/Extractorの評価回数とfailure時の後続非評価 |
| DC-05 | enclosing resultとfailureのError型関係 |
| DC-06 | operator/helper/branchのTask9診断契約を退行させない |
| DC-07 | parser/resolver/policy/runtimeの実sourceからstructured情報を得る |
| DC-08 | optional data欠落時にmessage解析へfallbackしない |
| DC-09 | JSON/Ariadne/Rune/Xldrが同じreason/origin/factsを保持する |
| DC-10 | remediation候補が型推論・primary reasonを変更しない |
| DC-11 | heuristic・暫定payloadの実行経路が撤去されている |
| DC-12 | do開始前のfocused/REPL/workspace gateを記録する |
| DC-13 | draftを削除・現行化せず、正本の参照だけ更新する |
| DC-14 | qualified builtin surfaceを`BUILTIN_METAS`の構造データから解決し、allowlist合成経路を撤去する |
| DC-15 | 現行Option-returning ExtractorのSome/None、子pattern不一致、単一評価、非Option返却拒否を維持し、更改を要求しない |
| DC-16 | nearest callable / closure / REPL継続 / Facet禁止と、静的patternエラーをruntime failureにしない境界を維持する |
| DC-17 | 通常constructor / nested Result / 明示pattern分解を共通MatchBlock経路で検証し、旧Ok限定・Option先行拒否・total non-Result pass-throughを残さない |

### 12.1 実装・検証対応

| 条件 | 主な実装テスト・監査 |
|---|---|
| DC-01–05、DC-16/17 | `crates/scar/tests/typecheck_surface.rs`のSafeBind projection / reason / target群、`tests/integration/language_features/safebind_and_errors.rs`、module fixture `control_safebind_extractor_some_once` |
| DC-06 | ScarのTrait/operator/branch既存回帰群と`crates/diagnostics/src/tests/structured_reasons.rs` |
| DC-07–10 | `crates/diagnostics/src/tests/`、Spire/Sigil producer tests、Rune integration、`crates/xldr/tests/repl_core.rs`の診断後継続 |
| DC-04、DC-15 | Forgeの`emit_extractor_bind_invokes_user_extractor_once`とtyped SafeBind failure tests、既存pattern / Extractor success・rejection tests |
| DC-11 | `crates/diagnostics/src/heuristics*`削除、旧暫定reason・message/source/marker分類参照のゼロ件監査 |
| DC-12 | `cargo run -- test --quiet --all`、CI profile workspaceの2回連続実行、独立レビュー |
| DC-13 | draft本文を除外した正本参照監査とtask-local diff確認 |
| DC-14 | Sindrのsurface signature / exact generated identity tests、Scarの完全signature検証、Sigilのunknown builtin拒否 |

実装後、本書の恒久的契約を `/docs` へ移管し、進捗やコマンドログを言語仕様へ混在させない。

## 13. 実装結果

DC-01–DC-17を実装し、恒久契約を[`../docs/dev/diagnostics.md`](../docs/dev/diagnostics.md)と
[`../docs/dev/Trait_system_spec.md`](../docs/dev/Trait_system_spec.md)へ同期した。SafeBind / Extractorの
現行Option契約、structured producer、typed runtime failure、builtin metadataのfail-closed境界を維持し、
draft本文とN07以降のdo実装には変更を加えていない。

`cargo run -- test --quiet --all`とCI profileのworkspace 1874 testsを同一worktree差分で2回連続成功させ、
独立レビューはfindings 0件だった。これにより§10のdo開始条件を満たし、N07を着手可能とする。
