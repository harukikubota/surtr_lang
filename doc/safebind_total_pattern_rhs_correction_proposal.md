# SafeBind total pattern / RHS 分類 訂正提案

## 1. 状態

- 分類: `/doc` に置く、N06着手前の仕様訂正。
- 対象: `diagnostics_cleanup_spec.md` のSafeBind RHS規則と、それを参照するN06・do計画。
- 実装状態: 未実装。本書は現在利用できる動作の説明ではない。
- 変更レベル: level4。型検査、structured diagnostic、typed IR、Forge failure target、REPL/CLI表示を横断する。
- 対象外: pattern自体の不正、Extractor更改、do symbol追加、SafeBindから一般Monad bindへの変更。

N06は本訂正を入力文書へ反映してから開始する。旧記述の「すべてのnon-Result RHSを値全体として
pass-throughする」「`saved =? Option::None`をtotal bindingとして受理する」は置き換える。

## 2. 訂正後の規則

`pattern =? rhs` はRHSを一度だけ評価する。LHS patternの全域性とRHSのcanonical type identityを
次の表で組み合わせる。

| LHS pattern | RHS | 判定 |
|---|---|---|
| total | canonical `Result<A, E>` | 外側一段だけ分解する。`Ok(value)`は`value: A`をpatternへ渡し、`Err(error)`はfailure targetへ進む |
| partial | canonical `Result<A, E>` | 外側一段だけ分解した後、通常MatchBlock pattern検査を行う |
| total | Result以外で、pattern型関係が成立し、canonical `Monad` capabilityがない型 | SafeBind固有compile error: total pattern `=?` 非Monad |
| total | Result以外で、pattern型関係が成立し、canonical `Monad` capabilityがある型 | SafeBind固有compile error: total pattern `=?` Result以外のMonad |
| partial | Result以外の型 | RHSの型と値を変更せず、通常MatchBlock pattern検査へ渡す |

この規則により、`total =? value` は`value`がcanonical Resultでない限り無効になる。
Option、List、ユーザ定義Monadをtotal patternへpass-throughする経路は作らない。

partial patternのnon-Result pass-throughは、Result以外へ分解能力を与える規則ではない。
constructor、literal、list/string分解、pin、Extractorなど、no-matchし得るLHSがRHS全体を明示的に
検査する通常MatchBlock規則である。Optionを暗黙に`Some`へ分解せず、Monadのmapped payloadも取り出さない。

Result判定は表示名やconstructor名ではなくcanonical builtin type identityで行う。自動分解は外側一段だけで、
`Result<Result<A>>`や`Result<Option<A>>`の内側を再帰的に分解しない。

## 3. 診断契約

SafeBind固有の新しいcompile errorは、structurally totalなLHSとnon-Result RHSの組のうち、
通常のpattern型関係が成立したものだけを対象にする。pattern arity、annotation mismatch、
Extractor返却型、通常のno-match可能性などは既存pattern診断の所有であり、この二分類へ畳み込まない。

たとえば`num: Int =? Option::Some(10)`は、non-Result RHS全体の`Option<Int>`とLHS注釈`Int`が
一致しない通常のTypeErrorである。この経路ではSafeBindの分解能力を説明するmessage / note / helpを追加しない。

structured producerは少なくとも次の二reasonを区別する。Rust enumの最終名は既存の命名規則へ合わせてよいが、
一つの自由形式messageへ統合してはならない。

1. `SafeBindTotalPatternNonMonadRhs`
   - LHSはtotal。
   - RHSはcanonical Resultではない。
   - RHSに対するcanonical Monad capability proofが成立しない。
2. `SafeBindTotalPatternNonResultMonadRhs`
   - LHSはtotal。
   - RHSはcanonical Resultではない。
   - RHSに対するcanonical Monad capability proofが成立する。

producer dataは、LHS totality、RHSの完全な型、canonical Result identityとの比較結果、Monad proof結果、
pattern / operator / RHSのsource originを保持する。rendererが型表示文字列、`Option`という名前、impl数・登録順、
既存messageを解析してreasonを決めてはならない。

未確定型に対するMonad proofが`Deferred`なら、どちらかへ早期分類しない。既存solverのboundaryまで保持し、
型またはcapabilityが確定してから上記二reasonを選ぶ。boundaryでも別の既存ambiguity/capability failureが
主原因として残る場合は、そのstructured failureを優先し、第三のSafeBind catch-all reasonを作らない。

### 3.1 表示要件

二つのheadlineは区別してよいが、どちらも「`=?` がRHSから取り出せるのはcanonical Resultの外側一段だけ」
という意味をerror message本体に明記する。

現行Option拒否の次のheadlineは、Result以外のMonad向けtemplateとして流用してよい。

```text
Option is not a SafeBind target; `=?` propagates Result-style failures, not optional values. Only a Result RHS can be decomposed by `=?`.
```

非Monad向けには、同じResult-only境界に加えてMonad capabilityがない事実を表示する。

```text
Int is not a SafeBind target; it is not a Monad, and only a Result RHS can be decomposed by `=?`.
```

型名は例であり、実際のclosed typed dataからrenderする。`Option::to_result(value, err)`、
`from::<Result>(value)`、その他の変換APIを勧めるhelpは出さない。型変換はSafeBind診断の責務外であり、
変換候補の有無をこの二reasonの選択にも使わない。

## 4. 検査順序

1. RHSを一度型検査し、canonical Result identityを判定する。
2. Resultなら外側一段のpayload型、non-ResultならRHS全体の型を通常MatchBlock checkerへ渡す。
3. pattern arity、annotation、Extractor返却型などのstatic pattern検査が失敗したら、通常のTypeErrorをそのまま返す。
4. static pattern検査の成功後、既存pattern classifierでLHSのtotal / partialを構造的に判定する。
5. Resultまたはpartial patternなら、通常のSafeBind成功 / failure経路へ進む。
6. non-Resultかつtotalなら、canonical Monad capabilityを共通trait-selection経路で証明し、二reasonへ分類する。
7. static pattern errorはfailure targetやruntime no-matchへ落とさない。成功経路だけをtyped IR / Forgeへ渡す。

Option固有の先行拒否やSafeBind constructor patternの`Ok`限定は残さない。ただし、Optionをtotal patternで
束縛する旧拒否ケースは、型名特例ではなく「Result以外のMonad」の拒否テストとして保持する。

## 5. doとの境界

SafeBind RHSの分解規則とdo carrierのfailure policyを分離する。

- partial patternのnon-Result RHSは通常MatchBlockへpass-throughできる。
- total patternのnon-Result RHSはdo carrierがResultか、Alternativeを持つかに関係なく、N06の二分類で先に拒否する。
- do carrierがcanonical Resultなら、合法なSafeBindの既存failureを保持する。
- do carrierがResult以外なら、合法なSafeBindのfailure targetを同じcarrierの`Alternative::empty`へ差し替える。
- Alternative capabilityはRHS分解能力を追加しない。
- SafeBind RHSはdo carrierの推論源にしない。

したがって「non-Result SafeBind」という語を使う箇所では、RHSがnon-Resultなのか、do carrierがnon-Resultなのかを
明記する。N10はN06の入力判定を再実装せず、N06で合法と確定したSafeBindのfailure targetだけを差し替える。

## 6. N06着手前の必須作業

N06の実装用worktreeを作る前に、同じrevisionで次を完了する。

1. `diagnostics_cleanup_spec.md`のRHS表、typed projection、作業順、DC-01–DC-03 / DC-17を本書へ合わせる。
2. `type_constructor_monad_do_implementation_plan.md`のN06説明とN10引継ぎ境界を本書へ合わせる。
3. `do_intrinsic_spec.md`の`safe_bind_input`、normalized IR、診断、テストマトリクス、受け入れ基準を本書へ合わせる。
4. `要件定義v9.md`に、現行N05契約とN06変更後契約を混同しない形で本訂正を記録する。
5. `/doc`内の生きた参照を検索し、「全non-Result RHS pass-through」「total Option binding成功」などの旧契約を残さない。
6. Markdown link、code fence、`git diff --check`を検証し、文書差分だけを独立レビューする。

N06実装時は、Scarのreason producerと共通Monad capability proof、diagnostics projection / renderer、
Rune / Xldr adapter、focused success/rejection testを同じ変更境界で扱う。実装完了後に限り、
`docs/dev/diagnostics.md`、利用者向けSafeBind文書、`lib/bootstrap.srt`の`@doc`を現行実装へ同期する。

## 7. 受け入れ境界

- total + Resultの成功 / Err伝播 / 一段projection。
- partial + Resultの通常MatchBlock成功 / no-match。
- pattern型関係が成立するtotal + scalar等の非Monadが第一reasonになる。
- pattern型関係が成立するtotal + Option / List / user-defined Monadが第二reasonになる。
- partial + Option / List / user-defined型が値全体を通常MatchBlockで検査する。
- `Option::Some(value) =? Option::Some(1)`はpartial patternとして受理し、`value: Int`になる。
- `value =? Option::Some(1)`はResult以外のMonadとして拒否する。
- `value =? 1`は非Monadとして拒否する。
- `num: Int =? Option::Some(10)`は通常のpattern型不一致TypeErrorとなり、SafeBind固有reasonやResult-only案内を付けない。
- Option固有の型名分岐、変換help、message解析fallbackがない。
- Deferredをimpl数・登録順・表示名から二reasonのどちらかへ具体化しない。
- RHS / Extractorの単一評価、nearest callable、Facet禁止、REPL継続など独立policyを維持する。
