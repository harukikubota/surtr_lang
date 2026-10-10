# 型検査調査の修正方針・継続調査

2026-10-10、現行ソース・正本文書・既存テストから再調査した。2026-10-10の利用者判断は維持し、古い観測・未確認範囲を現行の不具合と混同しない。

調査開始時のHEADは`717d0fd301f4d51b506ceb72347b74d4bf8134fc`、終了時は`c569e5bab2d229e7ce6d42a1d7428cbeec0a92cc`。間の変更は別作業による`doc/callable_name_and_syntax_classification.md`の削除だけで、対象ソースは同じである。指定された2つの調査書だけを更新した。製品コード・標準定義・正本文書・テストファイルは変更していない。

プロセスの停止・回収は実装・検証を完了し、契約を[Process Runtime 正本](../docs/dev/ProcessRuntime_spec.md#3102-stop受付拒否停止完了)へ反映した。TC-03・04とTC-06のPID側もプロセス関連として別作業へ渡すが、この停止仕様だけで型適合や標準の比較実装まで修正されるとは扱わない。

## 対応が必要な項目

| ID | 現行の判定 | 次の対応 |
|---|---|---|
| TC-03 | WorkerLease→PIDの暗黙適合を再現 | プロセス側で通常型比較から除去。生成helperとの受渡しを明示契約へ揃える |
| TC-04 | 未知PID marker、genericの不正適合を再現 | プロセス側でmarkerの解決と型変数の同一性を通常規則へ揃える |
| TC-05 | unary Hole入力の例外が残存 | 例外を除去し、標準署名・正本・成功／拒否テストを揃える |
| TC-06・Enum | 明示implなしのEqを再現 | 暗黙proof/dispatchを除去し、明示impl / deriveに従う |
| TC-06・PID | compiler-owned Eqが残存 | プロセス側で標準実装へ移す。`instance_of`のAPIとEqとの接続は未確定 |
| TC-10 | 文書修正済み | Resultの補助エラー名はドキュメント用で、返却kindの静的制限・網羅検査を行わないと正本へ明記 |
| TC-11 | 修正・検証済み | 型注釈producerの元TypeErrorをprobe rollback後も保持。候補依存失敗はcandidate情報を維持 |

プロセス以外で対応する確定項目はTC-05、TC-06のEnum側、TC-10、TC-11の4件。[runtime側のRT-11](runtime_audit_followup_20261010.md#rt-11-recoverの説明を現行の通常関数へ揃える)を合わせて5件となる。

## TC-05: Hole入力に限ったcallable適合

**対応が必要。** `crates/scar/src/checker/types.rs:2654-2677`の`value_types_compatible`は、actualの入力が単項`Ty::Hole`なら入力型を照合せず出力型だけを見る。`types_compatible`そのものではHoleは同じHoleとだけ一致する。2つの関係による例外が、通常値の検査で残っている。

現行CLIで次の差を再現した。

```surtr
mapper: (Int -> Int) = always(10)
print(inspect(mapper(1)))
# check成功、runは10。
```

```surtr
mappers: List<(Int -> Int)> = [always(10)]
# check拒否: expected (Int -> Int), got (_ -> Int)
```

後者は既に拒否されている。両方が現行で成功するとの説明にはしない。groupingを介する通常引数・注釈・返り値・分岐の成功は、`crates/scar/tests/grouped_contextual_callables.rs`が現在の例外を固定しており、今回も全7テストが成功した。

利用者判断は、この例外をバグとして除去すること。実入力が`(_ -> Int)`のままならIntに適合させない。`always`を通常の推論で入力型まで確定できる設計へ変えるなら、その根拠と標準署名を先に定める。従来の成功を保つための別の特例やcontainer側の救済は追加しない。

修正先は`value_types_compatible`とそのcaller、`lib/function.srt`、`docs/dev/Trait_system_spec.md:82`、`docs/site/type-annotations.md:200`、`docs/dev/テスト方針.md`のHole contract、`grouped_contextual_callables`と既存のHole flow fixture。型規則を変えるためlevel4の契約整理と検証が必要。受入条件は、引数・binding・return・分岐・containerで同じ通常関係を使い、出力やarityの拒否を弱めないこと。明示的な入力型を持つ通常のclosureやcaptureは成功を保つ。

## TC-06: Enumの暗黙比較とPIDの標準実装

### Enum側

**対応が必要。** `crates/scar/src/checker/predeclare.rs:2482-2525`の`compiler_trait_impl_exists` / `compiler_trait_dispatch_target`は、標準EqでpayloadのないEnumにproofとBinOp dispatchを暗黙提供する。

```surtr
defenum Choice { One, Two }
print(inspect(Choice::One == Choice::One))
# 現行: check成功、runはTrue。
# 修正後: 明示Eq実装がないため拒否。
```

利用者判断は、通常の`impl Eq`、または明示`@derive Eq`で生成した実装に従うこと。payloadの有無だけで能力を付与しない。標準ではBoolean、Ordering、FileSystemEntryKind、IntBase、StringEncoding等に明示deriveが既にある一方、`lib/file.srt:153`のFileMode等にはない。全標準enumへ一律にderiveを追加せず、実際に比較を要求する利用先の依存を確認する。

修正先はScarのproof / dispatch、必要な標準宣言、`docs/dev/Trait_system_spec.md:711-715`、`docs/site/trait-impls.md:33-38`、暗黙Eqの既存成功例。受入条件は、明示implなしのpayload-free enumを拒否し、明示impl / deriveを通常の登録・証明・dispatchで使うこと。payloadありEnumでは各payloadのEq要求を維持する。trait_impl_policyの既存derive成功・禁止型拒否のテストは今回成功したが、暗黙Eq除去後のGreenを意味しない。

### PID側（プロセス作業へ引継ぎ）

現行は同じ関数で`Ty::Pid`へのcompiler-owned Eqを提供し、`predeclare.rs:2328`の`trait_impl_policy_for_ty`はPIDへの利用者実装を閉じている。`instance_of`の標準実装は現行の`lib/`に見つからない。

利用者判断は「PIDは`instance_of`として標準が実装する」。名称、公開署名、Eqとの接続、標準だけに必要な実装権限を実装前に具体化する必要がある。本調査で新しいAPIを決めない。

[Process Runtime 第3.12節](../docs/dev/ProcessRuntime_spec.md#312-worker-lifecycle)はsingletonの型単位／Workerの個体単位のEqを維持するが、標準実装へ移すタスクは持たない。比較の意味を変えず実装主体を移す別件として管理する。名前だけでcompilerが能力を補う経路へ戻さない。Enum側の除去とPID側の未確定APIを、一つの完了条件へ混ぜない。

## TC-10: 具象エラー名の列挙はドキュメント目的

**文書修正済み。実装変更なし。** 2026-10-10、`docs/site/type-annotations.md`、`docs/site/trait-impls.md`、`docs/dev/Trait_system_spec.md`、`docs/dev/Error_spec.md`、`docs/dev/Xldr_spec.md`を整合させた。指定したエラー名の存在・直接戻り値位置の検査と、ドキュメント用metadataを区別し、返却kindの静的制限・網羅検査を行わないと明記した。変更箇所を現行説明・標準実装と照合し、`git diff --check`は成功。文書のみのためコンパイラテストは再実行していない。

以下は修正前の調査記録。実装変更は不要。 次の入力は現行の型検査で成功し、実行結果は`Err(ZeroDivisionError("division by zero"))`だった。

```surtr
def other_error() -> Result<Int, NoneError> { Err(ZeroDivisionError) }
print(inspect(other_error()))
```

この成功は利用者判断どおりである。直接戻り値位置の`Result<T, E>`は説明用metadataで、値の型は`Result<T>`。異なるError kindを返すことだけを理由に拒否しない。

`docs/site/type-annotations.md:88-96`の「関数が返すエラーの契約」、`docs/dev/Trait_system_spec.md:843-845`の「固定する」、`docs/site/trait-impls.md:233-237`の「エラー契約」は静的なkind保証とも読める。ドキュメント目的であり、返却kindの静的制限・網羅検査ではないことを明記する。`docs/dev/Error_spec.md`と`docs/dev/Xldr_spec.md:91`の値型／宣言表示の説明も照合する。

受入条件は、具象エラー名の存在確認、指定可能な直接戻り値位置、成功型・引数型の通常検査を維持し、説明用metadataとの区別を揃えること。エラー列挙構文や新しい静的制約は追加しない。

## TC-11: probeから最終診断までの情報保持

**修正・検証済み（level2）。** 2026-10-10、型注釈を解決するproducerのエラーをconstructor probeで保持し、rollback後も元のTypeErrorを返すようにした。予備検査・候補検査・child checker・nested probeで同じ経路を使い、messageや構造化情報の有無で原因を推測しない。候補依存の型関係失敗は従来のcandidate情報を保持する。

`common_constructor_invocation`では未知型、不正arity、構造化情報を持つ専用型の拒否を、parameter・body・grouping・nested callbackと追加carrierの有無で比較した。既知carrierの診断との完全一致、正常成功、候補依存失敗を固定し、元spanの欠落でRedを確認した。局所のgrouped/constructor検証は10件成功。最終差分の独立レビューは指摘なし。共有の最終検証は`rtk cargo nextest run --profile ci --workspace --features rune/tui`が2,430件成功、`rtk proxy cargo run -- test --quiet --all`が終了コード0。quietのため標準テスト件数は記録していない。

以下は修正前の調査記録。成功への不正fallbackは調査で見つからなかった。 次の3入力を現行CLIで検査した。

```surtr
Monad::return(1) |>= {|x: NotAType| Ok(x)}
Monad::return(1) |>= {|x: List<Int, String>| Ok(x)}
Monad::return(1) |>= {|x: String| Ok(x)}
```

それぞれを独立したファイルで検査すると、最終reasonは`NoApplicableTraitImplementation`、originは`Operator`の`|>=`、primary spanは`[14, 15]`（左の値`1`）になる。未知型・Listの不正arity・Int/String不一致の本文はcandidateごとの`failures[].detail`に残るが、元の注釈span、reason、関連factは残らない。

比較のため、次の入力も検査した。

```surtr
Ok(1) |>= {|x: NotAType| Ok(x)}
Ok(1) |>= {|x: List<Int, String>| Ok(x)}
```

この2つは同じ原因をそれぞれ注釈の`[15, 23]`、`[15, 32]`で拒否する。unknownの診断文は`Unknown type: NotAType`、arityの診断文は`List<T> requires exactly 1 type argument`。元エラーの位置が失われるのはcarrier未確定の候補探索へ入る組合せである。

入口・情報の流れは`crates/scar/src/checker/expr.rs:7258-7443`の`try_constructor_invocation_from_arguments`。予備検査の`.ok()`でcarrier hintが得られず、候補ごとの`check_trait_invocation`をrollbackする。その後`7398-7405`で`TypeError`をcandidate名と`error.message`だけへ縮め、最終の能力不足診断を左の値spanで生成する。したがって`.ok()`だけを外せば全て解決するという原因説明にはしない。

標準候補では未知注釈の最終diagnosticに12 candidateが含まれた。既存テストのBoxed carrierのFunctor / Applicative / Monad実装を追加すると13になり、reasonと指す値`1`は同じままcandidate detailが増えた。登録数で拒否が成功になることはなかった。正常な`Monad::return(1) |>= {|x: Int| Ok(x)}`は成功した。`Applicative::pure(1)`は追加carrierの有無とも`AmbiguousReturnTypeArgument`で拒否した。

rollbackは`expr.rs:487-522`でenv、substitutions、bounds、pending obligations、capabilities、Lazy状態、warnings等を復元する。既存のconstructor capability checkpoint／serialize／rollbackテストも成功した。全候補順序と全rollback状態の網羅検証は今回していない。

修正先はprobe・candidate診断の生成責務と`docs/dev/diagnostics.md`。受入条件は、候補に依存しない未知／不正注釈を元の位置・構造化情報で返すこと、候補依存の型関係失敗は必要なcandidate情報を保持すること、候補数や登録順だけで成功を推測しないこと。単に最初のcandidateの失敗を採用する実装にはしない。成功経路と、carrier根拠がない入力のambiguity拒否を維持する。

## TC-03・04: プロセス側で対応する型規則

次の正当なWorker宣言を共通の前提として検査した。未知Counterで拒否されただけの再現とは区別する。

```surtr
defgenserver Counter {
  meta { instance: Worker init_policy: Eager state: Int }
  @init def init() -> Result<Int> { Ok(0) }
  @call def read(state: Int) -> Result<CallResult<Int, Int>> {
    Ok(CallResult::Reply(state, state))
  }
}
```

### TC-03: WorkerLease→PID

`checker/types.rs:2717-2728`は、期待PIDと実WorkerLeaseの型引数を比較して一般の適合を許す。次の双方が`surtr check`で終了コード0だった。

```surtr
def leak(lease: WorkerLease<Counter>) -> PID<Counter> { lease }
def leak_list(lease: WorkerLease<Counter>) -> List<PID<Counter>> { [lease] }
```

この例外は通常の`types_compatible`内にあるためcontainerにも入る。後続の受入条件は、通常引数・binding・return・container・比較・adoptを含む一般の適合を拒否すること。生成Worker helperやruntimeのlease受渡しが必要な箇所は、明示的な契約で接続する。runtimeがleaseからPIDを取り出せるという事実を、通常型の暗黙変換の根拠にしない。

停止・回収仕様はleaseからの新規メッセージを停止受付で拒否するが、WorkerLeaseからPIDへの一般的な型適合を除去するタスクではない。公開Workers::reserveで得た実leaseのEq/adopt投入の実行は今回は行っていない。

### TC-04: PID markerとgeneric同一性

`checker/types.rs:604-616`の`pid_marker_from_ast`はNamedの文字列を返し、process宣言の存在・種別を検査しない。`2711-2714`のPID比較は片側が`$`始まりなら一致させ、通常の型変数束縛を共有しない。

```surtr
def fake(value: PID<NotAProcess>) -> PID<NotAProcess> { value }
# 現行: check成功。

def forget(value: PID<$P>) -> PID<Other> { value }
# Counterと別の正当なOther Workerがある場合もcheck成功。
```

さらにCounterとOtherを同じ構造で宣言し、正当なsupervisorを使う次の入力もcheck成功した。

```surtr
defsupervisor Sup {
  meta {
    strategy: OneForOne
    max_restarts: 5
    max_seconds: 10
    child_restart_default: Transient
    allow_adopt: True
  }
}
supervisor_init { Sup {} }
def take(first: PID<$P>, second: PID<$P>) -> PID<$P> { first }
a =? Sup::spawn(Counter::init())
b =? Sup::spawn(Other::init())
value = take(a, b)
```

同じ`$P`の2回の出現に異なるprocessの実PIDを渡せる。`Other::read(forget(a))`も型検査を通り、誤ったmarkerを生成wrapperに渡せることまで確認した。これらはcheckのみで、停止・回収と重なるruntime実行は行っていない。

受入条件は、未知markerを拒否し、有効なprocess markerをcanonical identityへ解決すること、genericを宣言内の同じ型変数として扱い、別processへの返却と同じgenericの異種入力を拒否すること。handler / singleton / Workerの許可範囲、実PID、複数moduleの同名markerも検証する。停止・回収仕様のPID runtime identityの変更だけでScarのこの規則も解消済みとしない。

## INV-01〜07の再判定

以下は、確定した修正と未確認の内部不変条件を区別した結果である。全組合せ未実測という理由だけで修正件数を増やさない。

| ID | 現行の判定 | 根拠・残る範囲 |
|---|---|---|
| INV-01 | 通常入口でのidentity欠落の救済は確認されない | resolved署名はfallbackを無効化。raw経路は残るが、field・alias・nested型は事前の位置検査で拒否。下記参照 |
| INV-02 | 不正受理の証拠なし | 空fieldでもownerと型引数を先に照合。正当なgeneric型の不一致を拒否。破損した内部metadataの直接投入は未実施 |
| INV-03 | 内部エラーの一般化が残存 | canonicalization／復元エラーの一般化を内部診断候補とする |
| INV-07 | 独立した追加バグは未確認 | TC-03/04/06/11へ重複を統合。constructor helper・capture・checkpointの既存テストは成功。複数moduleのPID genericと実leaseのEq/adopt実行は未実測 |

### INV-01: raw / resolved署名とalias

`checker/types.rs:59`はraw Normal / Trait / Builtin / ImplHeadで名前からconstructor Traitを引ける。一方、`1824-1930`のresolved署名入口はcanonical IDがなければResolved modeへ切り替え、名前のfallbackを無効化する。IDがあるがregistryにない／別identityの場合も明示的に拒否する。既存unit test `direct_signature_constructor_trait_without_canonical_identity_fails_closed`は今回成功した。

raw callerは`predeclare.rs:893`のfield、`1143`のEnum payload、`1594`のconstructor署名、`definitions.rs:1881, 1920`のExtractor署名、`predeclare.rs:2853`のimpl target再構成、`types.rs:1765`の標準Lazy内側など。`resolve_signature_alias`（`types.rs:325-378`）は同じmodeを再帰へ渡し、resolvedからrawへ切り替えない。

加えて`checker/mod.rs:4998`の`validate_constructor_application_positions`とalias展開を含む`4658-4715`の検査がraw解決より前に許可位置を制限する。今回、Functor<Int>のfield／Enum payload／関数型alias／generic alias／nested List引数は全て`ConstructorTraitApplicationPosition`で拒否した。parameterized constructor Traitの直接署名も専用エラーで拒否された。

現時点では修正を必須にする通常入力の反例はない。全raw callerへcanonical metadataを付ける再設計やraw経路の一律削除を、調査結果から自動で追加しない。標準Lazy内側・内部再構成・複数moduleのshadowingまで全組合せを実測したわけではない。

### INV-02: 空nominal field

`types.rs:2833-2867`はStruct / Recordのfieldが片側空なら展開fieldの再比較を省くが、その前にcanonical ownerと`nominal_arguments_compatible`（`carriers.rs:515`）を比較する。後者は型引数の数と各引数を検査する。`env.rs:242-260`には宣言先取り時の空field表現があり、展開済みfieldだけがnominal identityではない。

正当なconstructorを持つBox<$T>で`def leak(value: Box<Int>) -> Box<String> { value }`を検査し、`ReturnTypeMismatch: expected Box<String>, got Box<Int>`を確認した。最初のprobeにconstructor不足のエラーが出たため、constructorを追加した入力で再検査している。owner違い・generic違いを空fieldだけで受理する証拠は得ていない。内部のfieldを任意に破損させた型の照合は未検証。

### INV-03: Rejected理由

残る`trait_selection.rs:3563-3569, 3694-3706`と`carriers.rs:919-925`は、canonical request／canonical→Ty復元の失敗を`Canonicalization`へ縮める。失う内容は未登録nominal宣言、nominal引数metadata不足、内部callable identity等の内部契約診断である。Rejectedを成功へ変える経路ではない。

この残件は内部不変条件エラーの情報保持に限定して調査を継続する。正常なSurtr入力から不正成功または新たな循環原因欠落は再現していない。修正する場合は、破損metadataを最も直接的な層で入力し、元原因を維持する回帰テストを先に置く。TC-11の公開入力での診断劣化を、この未再現の内部候補で代用しない。

## 次の作業単位と検証

プロセス作業と独立して進める順序は、文書のみのTC-10 / RT-11、局所診断のTC-11、通常Eqへの統一であるTC-06のEnum側、型規則整理が必要なTC-05を推奨する。TC-05とPID側の未確定APIは、必要な契約を具体化してから実装へ進む。今回の調査を実装承認や新APIの決定として扱わない。

実装時は各項目の再発ケースと成功・拒否境界を既存test targetへ追加／修正し、狙った理由のRedから修正する。最終差分が複数フェーズ・型規則へ及ぶ場合はlevel4の正本整合、全体検証、別エージェントレビューを適用する。文書だけの修正にはコンパイラ全件の再実行は不要。

今回の実行コマンドと結果は次のとおり。

```sh
rtk cargo nextest run -p forge -p eldr -p scar -E 'test(normalize_function_table) | test(error_constructor) | binary(grouped_contextual_callables) | binary(trait_impl_policy) | binary(common_constructor_invocation) | binary(structured_diagnostics)'
# 30 passed, 702 skipped

rtk cargo build -p rune
# 成功。以降の最小入力はこのtarget/debug/surtrでcheck / run。

rtk cargo nextest run -p scar -p eldr -E 'binary(type_constructor_carriers) | binary(nominal_constructor_parameters) | binary(parameterized_type_constructor_traits) | binary(constructor_capability_session) | binary(return_type_argument_capture_forwarding) | test(push_atomic_bootstrap_allows_type_entries) | test(frame_stack_underflow_is_runtime_error)'
# 37 passed, 593 skipped

rtk cargo nextest run -p scar -E 'test(direct_signature_constructor_trait_without_canonical_identity_fails_closed) | test(constructor_projection_distinguishes_deferred_from_rejected) | test(constructor_projection_reports_each_metadata_contract_failure)'
# 3 passed, 343 skipped
```

Rustの既存テストは計70件成功、失敗0件。件数はnextestの実行単位で、registry内の個別case総数ではない。skippedは選択外であり成功件数に含めない。

最小入力はリポジトリ外の一時ディレクトリに保存し、通常の標準環境で`target/debug/surtr check <input.srt>`を実行した。主要入力と診断は各節に記載した。runtime側のgeneric注入fixture、直接dedup、recover、Hole mapper、暗黙Enum Eq、異なるkindを返すResult注釈の6入力はrunでも確認した。generic注入fixtureは既存`.expected`と一致する。

全workspace CI、標準Surtr全件、複数moduleのPID generic、実WorkerLeaseのEq/adopt、停止・回収／補充失敗の実行、全candidate登録順と全rollback状態は未検証。正本文書の修正と実装は後続作業である。
