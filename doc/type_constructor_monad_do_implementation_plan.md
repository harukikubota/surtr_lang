# TypeCtorTrait・標準Monad・MonadT・do・Generator 実装計画

## 1. 起点と状態

- 基準: `harukikubota/surtr_lang` / `f1986a27d84728e1e7a88de457a1d6b55cfe5ab8`。
- 引継ぎ前提: Git履歴に残る旧 Type Constructor Signature Unification 計画のTask 1–9は完了。
- 基準commitの旧計画にもTask 9の完了・追修正後の検証記録がある。
- 本計画は残作業を再編した新しい管理ファイル。旧Taskのチェックボックスを継続しない。
- 新Taskは `N01`–`N14` と呼び、旧Task 9等と混同しない。
- N01–N11は実装済み。N12以降を残作業として管理する。

旧Task 1–9の手順を再実装タスクとしてコピーしない。ただし新しい変更による退行を検出するため、既存テストは引き続き実行する。

本書は作業管理資料であり、言語意味論の唯一の正本ではない。仕様の詳細は下表の担当文書に置く。配置案は従来の引継ぎどおり `/doc` とし、文書種別をimplementation planと明示する。完了後は仕様として `/docs` に全文移動せず、必要な履歴はVCS/作業ログへ残す。

## 2. 入力仕様と実装済み正本

| ファイル | 責務 |
|---|---|
| `../docs/site/{identity,reader,state}.md` | 実装済み Identity / Reader / State の利用者向け契約 |
| `monadt_language_extension_spec.md` | nominal constructor parameter・parameterized TypeCtorTrait・MonadT契約 |
| `monadt_standard_types_spec.md` | OptionT / EitherT / ReaderT / StateTの意味とAPI。実装正本は`../lib/traits/monad_t.srt`と`../lib/types/monad_transformer/` |
| `diagnostics_cleanup_spec.md` | 旧Task 10のSafeBind・診断残作業 |
| 既存 `do_intrinsic_spec.md` | doの詳細仕様。新carrier規則と参照先を更新して利用 |
| `generator_spec.md` | 遅延・persistent GeneratorとAPI移行 |

実装済み共通契約は `/docs/dev/Trait_system_spec.md`、`/docs/dev/diagnostics.md` 等を利用する。削除する旧入力仕様は、必要な内容の移管が完了してから参照を切り替える。
N01 の旧入力 `type_constructor_trait_extension_spec.md` は実装と正本への移管後に削除した。
N02 の旧入力 `monad_instances_spec.md` も実装と `@doc`・利用者向け文書への移管後に削除した。

## 3. 作業分割の原則

通常Monad追加とMonadT追加を別Task、別差分、別完了条件にする。

- N02は、既存の通常型・関数・Traitの仕組みで完了させる。N03–N05を依存として要求しない。
- N03–N04は言語機能。標準4Transformerをすべて作らなくても、最小ユーザ型で検証できるようにする。
- N05は言語機能完成後の標準API。helper候補をcompiler特例で実現しない。
- GeneratorはMonadTやdo開始の技術的前提ではない。通常の実施順はdoの後とし、独立した変更として管理する。
- 仕様の追加と実装済み `/docs` の更新を混同しない。
- ExtractorはN06–N14でも現行のOption返却を前提にする。更改は`extractor_revision_draft.md`の独立した別タスクであり、N番号を付けず、相互の着手・完了条件に含めない。

## 4. 全体順序と依存

| 新Task | 作業 | 主な依存 | 状態 | 旧計画との対応 |
|---|---|---|---|---|
| N01 | direct carrier同一性改修 | 旧Task 9までの基盤 | [x] 完了 | 新規 |
| N02 | Identity / Reader / State | 既存通常型・Trait基盤 | [x] 完了 | 新規・独立 |
| N03 | nominal constructor parameterとdeclaration constraint | N01 | [x] 完了 | 新規言語機能 |
| N04 | parameterized TypeCtorTrait・MonadT契約 | N03 | [x] 完了 | 新規言語機能 |
| N05 | 標準Transformer | N04。Identityを使うテストはN02 | [x] 完了 | 新規標準機能 |
| N06 | SafeBind・診断残作業・do開始ゲート | N01–N05の検証済みrevision、実装済みSafeBind RHS訂正 | [x] 完了 | 旧Task 10を再編 |
| N07 | do compiler-owned contract | N06 | [x] 完了 | 旧Task 11 |
| N08 | do syntax・AST・resolver・scope | N07 | [x] 完了 | 旧Task 12 |
| N09 | do carrier推論・core lowering | N08 | [x] 完了 | 旧Task 13 |
| N10 | do SafeBind・Forge lowering | N09 | [x] 完了 | 旧Task 14 |
| N11 | do診断・全carrier統合検証 | N10 | [x] 完了 | 旧Task 15 |
| N12 | Generator core改修 | G-I01/G-I02確定。独立Task | [ ] 未着手 | 新規 |
| N13 | Generator adapter・言語機能接続 | N12。do確認はN11 | [ ] 未着手 | 新規 |
| N14 | 最終監査・文書移管確認 | 上記全Task | [ ] 未着手 | 旧Task 16を拡張 |

N06の「N01–N05完了」は、この会話で採用した統合順のゲートであり、doという機能が理論上MonadTを必要とするという意味ではない。MonadTのinterface判断で順序を変える場合は、本表とdo開始条件を明示的に改訂する。

## 5. 着手前の文書整理

[x] ローカルの変更を保護し、基準commitと最新remoteとの差分を確認する。

[x] 旧入力の節ごとに、実装済み・確定未実装・draftを分類する。ファイル名だけで完了と推定しない。

[x] 実装済みのRTA・dispatch・診断の内容を `/docs` の担当正本へ移管する。

[x] 未実装分を本パッケージの仕様と既存do仕様へ対応付ける。

[x] draft本文は変更せず保持する。現行との差異を埋める移行作業を行わない。

[x] 新計画を配置し、旧計画の生きた参照を更新してから旧計画を削除する。

この整理作業では言語実装を変更しない。実装前のinterface確認が残る事項を、実装済み `/docs` へ書かない。

2026-09-09の取り込みでは`origin`をfetchし、基準commit
`f1986a27d84728e1e7a88de457a1d6b55cfe5ab8`が`origin/main`と一致し、ローカル`main`の祖先であることを確認した。
dirtyなメインworktreeは変更せず、専用worktreeで文書だけを整理した。compiler testは実装変更時の各Taskへ残す。

### 5.1 旧入力移管マトリクス

旧ファイル名は削除済み入力をGit履歴で特定するためのラベルであり、生きた正本へのリンクではない。
実装済み判定は旧チェックボックスだけでなく、現行source/testとHEAD祖先のtask commitを照合した。

| 旧入力・節（Git履歴） | 分類 | 実装・test根拠 | 現在の行き先 |
|---|---|---|---|
| `return_type_argument_rules.md` §§1–3 | 実装済み | `crates/spire/src/{ast.rs,parser}`、`crates/sigil/src/{resolved.rs,resolver}`、`crates/scar/tests/return_type_arguments.rs` | `docs/dev/Trait_system_spec.md` §§0.1–0.6, 4.1、`docs/site/type-annotations.md` |
| 同 §§4–5 | 実装済み | `crates/scar/src/checker/{predeclare.rs,signatures.rs,types.rs}`、`crates/scar/tests/{return_type_arguments,type_constructor_carriers}.rs` | `docs/dev/Trait_system_spec.md` §§0.4–0.5, 4.1 |
| 同 §§6–7 | 実装済み | `CallConstraintSet` / `SolveState`と`call_substitution`、`crates/scar/tests/return_type_argument_capture_forwarding.rs` | `docs/dev/Trait_system_spec.md` §7 |
| 同 §§8–12 | 実装済み | `crates/diagnostics/src/{data.rs,projection.rs,typecheck.rs}`、RTA pass/fail fixtures | `docs/dev/diagnostics.md`、`docs/dev/Trait_system_spec.md` §8 |
| `trait_method_type_list_dispatch.md` §§1–7 | 実装済み | `CanonicalTy` / role list / alpha environment、`crates/scar/tests/trait_method_type_lists.rs` | `docs/dev/Trait_system_spec.md` §§2.1–2.3, 4.1、`docs/site/trait-impls.md` |
| 同 §§8–10 | 実装済み | `CanonicalTraitImplPatternKey` / `TraitImplDeclarationKey`、`trait_selection.rs`、`specialize.rs`、trait applicability/instantiation tests | `docs/dev/Trait_system_spec.md` §§3.2, 7 |
| 同 §§11–14 | 実装済み | constructor carrier tests、default/derive/builtin dispatch tests、recursive Trait self-call fixture | `docs/dev/Trait_system_spec.md` §§1–2, 4–6、`docs/site/trait-impls.md` |
| 同 §§15–19 | 実装済み契約と履歴 | sourceの旧語彙・fallback監査、Scar/diagnostics/Rune fixtures | `docs/dev/{Trait_system_spec,diagnostics}.md`。実装手順と過去ログはVCS |
| `signature_diagnostics_unification.md` §§1–5.7 | 実装済み | `crates/diagnostics/src/{data.rs,projection.rs,render.rs,typecheck.rs}`、structured diagnostics tests | `docs/dev/diagnostics.md`のstructured contract・stable reason・typed data |
| 同 §§6–9 | 実装済み | SafeBindのResult一段射影、通常pattern優先、全phase producerのclosed reason、message heuristic撤去を実装 | `docs/dev/diagnostics.md`、実装入力・履歴は`diagnostics_cleanup_spec.md` |
| 同 §§10–11 | 実装済み | JSON typed projection、phase ownership、Rune/Xldr adapterをstructured producer入力へ統一 | `docs/dev/diagnostics.md` |
| 同 §§12–15 | 実装済み | Task 9とN06のSafeBind・diagnostics cleanup、およびN07–N11のdo実装・受け入れ検証を完了 | `docs/dev/diagnostics.md`、`do_intrinsic_spec.md`、N07–N11 |
| `type_constructor_signature_unification_task4_fallback_remediation_scope.md`の完了部分 | 実装済み | arity事前検査、canonical signature必須化、candidate rollback、RTA ambiguity tests | `docs/dev/Trait_system_spec.md` §§2.1, 3.2, 7、`docs/dev/diagnostics.md` |
| 同 canonical Trait / constructor application残件 | 実装済み | Sigilのcanonical Trait identityを保持し、constructor projectionは`Deferred`/`Rejected`の理由を保持。実行可能な未解決`SelfApp`はScar境界で構造化診断として拒否 | `docs/dev/Trait_system_spec.md` §§0.5–0.7, 2–3, 7–8 |
| 同 builtin / diagnostic残件 | 実装済み | source surfaceとcompiler-generated declaration identityを`BUILTIN_METAS`の構造データへ統合し、message heuristicと外部allowlistを撤去 | `docs/dev/diagnostics.md`、実装履歴は`diagnostics_cleanup_spec.md` §8.1・DC-14 |
| `type_constructor_signature_unification_implementation_plan.md` Task 1–9 | 完了履歴 | commits `b1805d75`, `6221dffd`, `d134c95a`, `256c4b79`, `65ba6172`, `25c53c4e`, `dfe4a2e1`, `c28d185c`, `f2c0affd`と各review fixがHEAD祖先 | 恒久契約は担当`/docs`、実装手順・検証ログはVCS |
| 同 Task 10 | 完了履歴 | SafeBind旧制限、diagnostic heuristic、builtin surface allowlistをN06で撤去 | `docs/dev/diagnostics.md`、`diagnostics_cleanup_spec.md` / N06 |
| 同 Task 11 | 実装済み | Sindrの`DoIntrinsicContract` / `DoBlock` / `IntrinsicId::Do`、標準surfaceの構造検証、reserved marker拒否を実装 | `do_intrinsic_spec.md` / N07 |
| 同 Task 12 | 実装済み | `Token::Do` / `Ast::Do` / `Resolved::Do`、RTA・statement span、RHS-first scope resolution | `do_intrinsic_spec.md` / N08 |
| 同 Task 13 | 実装済み | do-local carrier推論、Monad / 条件付きAlternative、core typed loweringを実装 | `do_intrinsic_spec.md` / N09 |
| 同 Task 14 | 実装済み | SafeBind failure targetとForge loweringを実装・検証 | `do_intrinsic_spec.md` / N10 |
| 同 Task 15 | 実装済み | 診断整備と全carrier受け入れ検証をN11で完了 | `do_intrinsic_spec.md` / N11 |
| 同 Task 16 | implementation plan | 最終監査はdo/Generator等の完了後 | N14 |
| `signature_level_type_constructor_inference_draft.md` | draft | import前後のblob hash一致を確認 | 本文を変更せず`doc/`に保持 |

## 6. N01 — direct carrier同一性

担当: 通常signatureの正規化、carrier relation、signature/dispatch診断、関連stdlib helperとテスト。

同じdirect TypeCtorTrait名を単一の`$F` + bare capabilityと同じcarrier関係へ正規化する。
異なるdirect Trait名は同じfamilyでも独立させ、同じ `$F` / `Self` と明示契約による共有を維持する。
RTAとreturn-only carrierの既存関係を切断しない。

完了条件: `CI-01`–`CI-12`と`CI-15/16`。CI-13/14のdo確認はN11へ引き継ぐ。実装済み `/docs` とhelperのsignatureを新規則へ同期する。

状態: 完了。同じdirect TypeCtorTrait名を単一のconstructor variableとして共有し、異なるdirect Trait名は
同じfamilyでも独立させた。同じ`$F` / `Self`、return-only RTAの明示関係も維持した。
名前付き`$F<...> where $F: TypeCtorTrait`はsignature内のconstructor application自体でbare capabilityを
消費し、direct表記と同じ未使用constraint判定にした。
canonical Trait identity、captured / phantom nominal arguments、構造化診断、probe / REPL rollbackを含む
CI-01–CI-12・CI-15/16を実装とテストへ固定した。CI-13/14は予定どおりN11へ残す。

## 7. N02 — 通常Monadインスタンス

担当: `lib/types/{identity,reader,state}.srt` 相当、標準読込み、標準テスト、利用者向け `@doc`。

Identity / Reader / State、new/runと固有primitive、Functor/Applicative/Monadを追加する。通常型・関数型field・明示slot mapping・既存Trait dispatchだけで実装する。

Readerは関数入力Rを渡すデータ構造。Stateは次状態を返す純粋な状態遷移構造。Processへ接続する専用機能を作らない。

完了条件: MI-01–MI-14、MI-16。MI-15のdo比較はN11。MonadTがなくてもこのTaskを完了できる。

状態: 完了。Identity / Reader / Stateを通常のsource型として追加し、Functor / Applicative / Monad、
固有helper、標準読込み、利用者向け`@doc`を実装した。関数値fieldの呼出しは
`Function::apply`を使い、通常のclosure引数・ローカルclosureは通常callのまま扱う。
成功ケース、有限入力による各法則、Reader / Stateのcarrier不一致診断、REPLの具体化・ambiguity・
失敗後継続をテストへ固定した。MI-15は予定どおりN11へ残す。

## 8. N03 — nominal constructor parameter

担当: declaration `where` constraintの構文metadata、型式の適用、nominal well-formedness、型変更可能な再構築、REPL具体化。

`defstruct OptionT<$M, $A> where $M: Monad { inner: $M<Option<$A>> }` を最小ユーザ型で検証する。constructor slotと通常型parameterを明確に扱い、無制約の一般HKT推論を追加しない。

N03の初回範囲は、`Result` / `Option` / `List` と、既存のTypeCtorTrait implで
constructor slotが一意に定まるユーザ定義のbare nominal headに限定した。nominal型注釈の
`Either<String, _>`のような部分適用構文、associated type、型lambdaは追加しない。call-site RTAの
完全・部分型applicationと`_`はN04のMT-L18–MT-L21で扱い、`do`での実使用（MT-L22）はN07–N11で扱う。

MT-I03では、nominal headのdeclaration `where` constraintをScarの型定義metadataへ保持する。
field宣言ではそのconstraintから`$M<T>`のshapeを検査し、nominal適用、callable signature、
constructor、型変更可能なFacet再構築の各destinationで同じconstraintを既存solverへ渡す。
rigid genericは明示された`where` constraintだけをproofとし、型の利用箇所からconstraintを暗黙導入しない。
nominal well-formednessに使った明示`where` constraintは使用済みとして扱う。

完了条件: MT-L01–06、MT-L13/14/15の該当部分。field/return/annotation/Facetでdestinationのconstraintを検査する。

状態: 完了。`defstruct` / `defenum` の`where` constraint付きconstructor parameterを導入し、
field / payload のconstructor application、bare nominal head、nominal well-formedness、
constructor / callable / return / annotation、型変更可能なFacet destination、REPLの失敗後継続を
同じdeclaration `where` constraintと既存solverで検査するようにした。constructor slot以外の通常parameter、
rigid genericのproof、使用済みconstraintを区別し、部分適用・型lambda・runtime dictionaryは追加していない。

対応する受け入れ条件ID: MT-L01–06、MT-L13、MT-L14、MT-L15のN03該当部分。

検証（2026-09-09）:

- `rtk cargo nextest run -p scar --test nominal_constructor_parameters`: 20 passed。
- `rtk cargo nextest run -p spire`: 413 passed。
- `rtk cargo nextest run -p scar`: 251 passed。
- `rtk cargo nextest run -p xldr --test repl_core repl_core_bucket_3`: 1 passed、8 skipped。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。

実装commit: `b0c180cd`。N04以降、Rune integration、workspace / CI全体はN03の対象外として未実行。
入力仕様のN03状態は`doc/monadt_language_extension_spec.md`、実装済み仕様は`doc/要件定義v9.md`、`docs/dev/Trait_system_spec.md`、
`docs/site/{language-reference,structs,type-annotations,facet}.md`へ反映した。

## 9. N04 — parameterized TypeCtorTrait / MonadT契約

担当: captured Trait parameter、parent shapeの継承、TraitRef、method type list、applicability、static instantiation。

MonadTの最小contractを追加し、Selfとbaseの独立したcarrierを保持する。Trait-head binderと`where` constraintを分離し、field layoutを発見する機能や、標準型の第1引数をbaseとする位置規則を作らない。

MT-I02/04を記録した上で、ユーザ定義の最小MonadT実装を用いて、ordinary call/RTA/expected typeだけで`lift`を解決する。TypeCtorTrait RTAの完全・部分型applicationと`_`はN04で確定するが、`do`での実使用確認（MT-L22）はN07–N11の後続受入へ残す。

完了条件: MT-L07–12、MT-L14–21、MT-L23–24、およびN03からの残り。MT-L22は`do`後続受入（N07–N11）で完了し、N05の標準4型完成をN04の唯一の検証方法にしない。

状態: 完了。Trait-head parameterとconstructor mapped slotを別metadataとして保持し、
parameterized TypeCtorTraitのparent shape、TraitRef、method role list、impl coherence / applicability、
static instantiationを既存のcanonical solverへ統合した。標準contractとして
`MonadT<$M> where $M: Monad, Self: Monad` と `lift::<Self>(value: $M<$A>) -> Self<$A>` を追加した。
base carrierと出力carrierは独立して解決し、impl数、登録順、標準型名、field構造から不足型を補完しない。

call-site TypeCtorTrait RTAでは完全なcarrier型、`_`を含む型application、bare constructor headを受理し、
mapped / captured argumentをvalue argument、expected return、型注釈、既存signature constraintだけから具体化する。
通常Trait methodのordinary RTAでも、generic targetのbare headとimpl source/targetが共有する型変数を
value argument / expected returnから完全に解ける場合は受理する。user-defined targetにも同じcanonical solverを使い、
標準型名、impl数、登録順による補完は行わない。通常関数のordinary RTAは完全型のままとする。
Trait methodを含むトップレベルRTAの`_`は省略と同じ推論入力とし、RTA内nominal applicationの`_`に付く
declaration constraintは後続constraintで具体化してから検査する。
通常型注釈の`_`はconstructor inferenceに流用せず、`Alternative::empty::<Self>() -> Self` は
具象carrier全体を単一RTAとして扱う。Trait-head / nominal binderのinline constraintは拒否し、
`where`だけを正規surfaceとした。

対応する受け入れ条件ID: MT-L07–12、MT-L14–21、MT-L23–24。MT-L22は予定どおりN07–N11へ残す。

検証（2026-09-10）:

- `rtk cargo nextest run -p spire`: 414 passed。
- `rtk cargo nextest run -p sigil`: 245 passed。
- `rtk cargo nextest run -p scar --test return_type_arguments`: 41 passed。
- `rtk cargo nextest run -p scar`: 267 passed。
- `rtk cargo nextest run -p forge`: 72 passed。
- `rtk cargo nextest run -p diagnostics`: 79 passed。
- `rtk cargo nextest run -p xldr`: 79 passed、78 skipped。
- `rtk cargo nextest run -p rune --test integration run_srt`: 11 passed、120 skipped。
- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`: 1893 passedを同一差分で2回連続成功。
- `cargo run -- test --quiet --all`: 成功（quietのため件数表示なし）。
- `cargo run -- test --all`: 476 passed（quiet実行と同じ標準test集合の件数確認）。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。

実装commit: 未作成。専用worktree `.worktrees/n04-type-constructor-monad-do` の未コミット差分として保持する。
実装済み契約は`doc/要件定義v9.md`、`docs/dev/{Trait_system_spec,diagnostics,テスト方針}.md`、
`docs/site/`の関連利用者文書、標準`MonadT`の`@doc`へ移管した。
N05へは標準OptionT / EitherT / ReaderT / StateT本体とAPI・法則テストだけを引き継ぎ、
N04のcompiler特例を追加しない。

## 10. N05 — 標準MonadT実装型

担当: 標準 OptionT / EitherT / ReaderT / StateT、個別Trait impl、API、法則テスト、field/Facet/REPL。

着手時にMT-SI01–04の採用一覧を残す。必須new/run/lift/固有primitiveと、任意のmap_t等を区別する。未確定helperは必要な契約を決めるまで追加しない。

Alternativeは各型で条件付き実装する。IdentityT、抽象Transformer引数API、ResultT特例を追加しない。

完了条件: MT-S01–13、MT-S15–18。doのMT-S14はN11へ引き継ぐ。

状態: 完了。MT-SI01–04は`monadt_standard_types_spec.md` §11の採用一覧どおり固定し、
`map_inner` / `map_t`は追加せず、EitherTには作用するslotを明示する
`map_left` / `map_right` / `bimap`を追加した。OptionT / EitherT / ReaderT / StateTを
標準moduleとして追加し、new / run / lift、各型固有helper、Functor / Applicative / Monad / MonadT、
仕様で定めた条件付きAlternativeを通常のSurtr定義として実装した。IdentityT、抽象Transformer引数API、
runtime辞書、標準型名に依存するcompiler特例は追加していない。

標準4型を通じて顕在化した一般的なconstructor provenance / captured boundの伝播をcanonical solver上で補い、
`MonadT::lift`の入力payload capabilityを保持するようにした。`Deferred`は後続制約へ残す一方、
`Rejected`を候補bindへ流すfallbackは削除した。Facetではplain sourceのResult-valued focusに余分な
`Ok`を重ねず、source自体がResultまたはpathがfallibleな場合だけ外側Resultを生成する契約へ揃えた。

対応する受け入れ条件ID: MT-S01–13、MT-S15–18。MT-S14は予定どおりN11へ残す。

検証（2026-09-12）:

- `rtk cargo nextest run -p scar`: 269 passed。
- `rtk cargo nextest run -p forge`: 72 passed。
- `rtk cargo nextest run -p xldr`: 79 passed、78 skipped。
- `rtk cargo nextest run -p rune --test integration run_srt`: 11 passed、120 skipped。
- `rtk cargo nextest run -p rune --test integration`: 131 passed。
- `cargo run -- test --quiet --all`: 成功（quietのため件数表示なし）。
- `cargo run -- test --all`: 489 passed（quiet実行と同じ標準test集合の件数確認）。
- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`: 1895 passedを同一差分で2回連続成功。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。

Either / EitherT API 追加後の検証（2026-09-13）:

- `cargo run -- test --quiet either`: 成功（quietのため件数表示なし）。
- `cargo run -- test --quiet monad_transformers`: 成功（quietのため件数表示なし）。
- `cargo run -- test --quiet --all`: 成功（quietのため件数表示なし）。
- `target/debug/surtr repl --quiet --no-local-config`の実REPLで、変更したEither helper、
  From変換、EitherT mapping helperの代表24入力を確認: すべて成功。
- `git diff --check`とstaleな非採用記述の検索: 成功。

### N03–N05 受け入れ条件と実テスト対応

削除予定の入力文書を開かなくても完了契約を追跡できるよう、IDの意味と直接の検証先を次に固定する。

| Task / 受け入れ条件 | 完了した契約 | 主な実テスト・fixture |
|---|---|---|
| N03 / MT-L01–06 | `defstruct` / `defenum`のconstructor parameterとdeclaration `where` constraintを分離し、nested field、型形成、destination bound、rigid genericを同じwell-formedness経路で検査する | `crates/scar/tests/nominal_constructor_parameters.rs`、`crates/scar/tests/typecheck_surface.rs` |
| N03 / MT-L13–15のN03範囲 | nominal値をFacetで合法に再構築し、REPLには具象値だけを保存する。runtime dictionary、field探索、型lambdaを導入しない | `crates/scar/tests/nominal_constructor_parameters.rs`、`crates/xldr/tests/repl_core.rs`の`core_nominal_constructor_value_survives_failed_bound_check` |
| N04 / MT-L07–12 | Trait-head parameterとmapped slotを分離し、base / Selfを独立carrierとして通常のimpl matching、coherence、expected type、value argumentで解決する | `crates/scar/tests/{parameterized_type_constructor_traits,type_constructor_carriers,common_constructor_invocation}.rs` |
| N04 / MT-L16–21、MT-L23–24 | user-defined MonadT、完全・部分RTA、`_`、bare head、`Alternative::empty`をcanonical solverで扱い、impl数・順序・固定Trait argumentから補完しない | `crates/scar/tests/{parameterized_type_constructor_traits,return_type_arguments}.rs`、`tests/fixtures/script/pass/stdmod/monad_transformer_*`、`tests/fixtures/script/fail/typecheck/monad_transformer_*` |
| N05 / MT-S01–10 | 4 Transformerのrepresentation round-trip、Functor / Applicative / Monad / lift、短絡、base failure、State threading、条件付きAlternativeを観測する | `lib/tests/monad_transformers.srt`、`tests/fixtures/script/pass/stdmod/monad_transformers.srt`、`tests/fixtures/script/fail/typecheck/{either_t_has_no_alternative,option_t_requires_monad_base,reader_t_alternative_requires_base_capability,state_t_alternative_requires_base_capability}.*` |
| N05 / MT-S11–13、MT-S15 | 通常field・collection・Facet・REPLで具象値を扱い、ambiguityを拒否し、user-defined base / Transformerを実行する | `lib/tests/monad_transformers.srt`、`tests/fixtures/script/pass/stdmod/user_defined_monad_transformer.srt`、`tests/fixtures/script/fail/typecheck/monad_transformer_lift_ambiguous.*`、`crates/xldr/tests/repl_core.rs` |
| N05 / MT-S16–18 | 有限Monad / lift law、EitherT mapping helperのslot / base failure保持、非採用API・runtime経路の不在を固定する | `lib/tests/monad_transformers.srt`、`lib/tests/either.srt`、`tests/fixtures/script/pass/stdmod/user_defined_monad_transformer.srt` |
| N11 / MT-L22、MT-S14 | `do::<Either<String, _>>`を通常のTypeCtorTrait RTA経路へ接続し、pipelineと型・観測結果を比較する | `lib/tests/do.srt`でEitherと4 Transformerを含む全carrierのpipeline比較を実行 |

N14ではこの表を起点に受け入れ条件と実テストを再照合する。generic `defrecord` / constructor
parameterはN03–N05に含めず、`open-issues.md` OI-035で別仕様を待つ。

実装は専用worktree `.worktrees/type-constructor-monad-do-n05` で task-local commit に分割した。
標準APIと実装契約は各`.srt`の`@doc`、`docs/site/monad-transformers.md`、
`docs/site/{README,standard-library,standard-modules}.md`へ移管した。
N06へはN01–N05の検証済み差分を引き継ぎ、N05のための追加作業は残さない。
SafeBind RHS訂正は専用worktreeで`diagnostics_cleanup_spec.md`、本計画、`do_intrinsic_spec.md`、
`要件定義v9.md`、恒久文書、標準`@doc`、実装とテストへ反映した。旧non-Result全面pass-through契約を
生きた入力から除き、移管元の一時提案書も削除した。

## 11. N06 — SafeBind / diagnostics cleanup / do開始ゲート

担当: `diagnostics_cleanup_spec.md`。旧Task 10の残作業を引き継ぐ。

Result一段projection、partial non-Result pass-through、pattern型関係が成立するtotal non-Resultの
「非Monad」「Result以外のMonad」二診断、remaining phase producer、heuristic撤去、Rune/Xldr adapter、
qualified builtin surfaceの`BUILTIN_METAS`構造化を完了する。

旧Task 10に含まれるdraft削除、draft本文を含めた旧語彙ゼロ件検査、Task 9の再実装は行わない。

N06はlevel4の仕様変更であり、Option名による先行拒否・constructor patternのOk限定を撤去し、通常MatchBlockを再利用する。
`num: Int =? Option::Some(10)`のようなstatic pattern型不一致は通常TypeErrorを優先し、SafeBind固有説明を加えない。
通常pattern検査を通過したtotal non-Resultだけを二診断へ分類し、両方でResultだけがRHS分解能力を持つことを示す。
変換APIのhelpは出さず、`total =? value`をnon-Result pass-throughとして受理しない。
現行Option返却ExtractorのSome/Noneと単一評価を維持し、Extractor更改やdo symbol導入を含めない。
詳細な作業順・変更先・移管先は`diagnostics_cleanup_spec.md` §11.1、SafeBindの追加境界は同書§3.1を入力にする。

完了条件: DC-01–DC-17。既存全体ゲートどおり同じrevisionでworkspaceを2回連続成功させ、ログとcommitを記録する。do実装symbolがまだないことを確認する。

状態: 完了。SafeBindはcanonical Resultの外側一段だけを射影し、partial non-Resultは通常MatchBlockへ渡す。
static pattern検査を通過したtotal non-Resultはcanonical Monad proofに基づく二reasonで拒否し、RHSとExtractorの
単一評価、nearest callableのfailure target、Facet禁止、REPL継続を保持した。phase producer、Rune/Xldr adapter、
runtime failureをclosed reason / typed dataへ移し、自然言語message・source・label markerから意味を再構成する
heuristicを削除した。builtin source surfaceとcompiler-generated declaration identityは`BUILTIN_METAS`を正本とし、
通常source surfaceへの混入やowner/nameの外部allowlist合成を認めない。`do` symbolは導入していない。

実装は専用worktree `surtr-n06` の未commit差分として保持する。commitは利用者から明示された時点でtask-local
fileだけをstageして作成する。DC-01–DC-17のfocused / REPL / 全体ゲートと独立レビュー結果はN06完了時の
作業報告に記録する。

検証（2026-09-13）:

- `cargo check --workspace`: 成功。
- `cargo run -- test --quiet --all`: 成功（quietのため件数表示なし）。
- focused: diagnostics 45 passed、Sindr 90 passed、Sigil 245 passed、Xldr 81 passed / 78 skipped、
  Rune CI integration 131 passed。独立Lunaレビューの再検証もSindr 88 passed、Scar surface 8 passed / 1 skipped、
  Rune language-feature 8 passed / 123 skipped。
- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`: 1874 passedを最終差分で2回連続成功。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。旧heuristic実行参照、do実装symbol、
  DynamicSupervisor runtime対応のmetadata外allowlistはいずれも0件。
- Astra顧問の指摘したruntime名接頭辞による検証迂回と、独立Lunaレビューが指摘したcompiler-generated
  DynamicSupervisor対応の外部allowlistを修正し、最終レビューはfindings 0件。

未検証範囲はない。commitは未作成であり、本記録の「同じrevision」は同一の未commit worktree差分を指す。

## 12. N07–N11 — do

### N07: compiler-owned contract

既存 `do_intrinsic_spec.md` のcompiler-owned contract、DoBlock、surface validationを実装する。do-local carrierの単一性をcontract自体へ明示する。

状態: 完了。Sindrに`IntrinsicId::Do`、`DoIntrinsicContract`、canonical Trait / method / Result identity、
ReturnTypeArgument position 0に結び付く単一のdo-local carrier、条件付き`Alternative`、SafeBind input / failure policy、
lowering contractをclosed metadataとして追加した。`DoBlock` builtin identityは
`IntrinsicSignatureOnly(IntrinsicId::Do)`とし、標準sourceからidentityを生成しない。

Spireは`@intrinsic`のraw display textと構造化signatureを分離し、Sigilは`Bootstrap::do`のowner、
ReturnTypeArgument arity / role、parameter、return、反復`$Result`関係をSindr contractと構造比較する。
user sourceの`DoBlock`宣言・通常type position・inherent / Trait impl targetは用途別のclosed reasonで拒否する。
raw signatureの再解析、通常callable scheme化、runtime function / opcode、do構文・推論・loweringは追加していない。

検証（2026-09-13）:

- TDD Red: Spireの構造化signatureテストは旧`String` ASTでcompile failure、Sigilの不正surfaceは受理、
  `DoBlock`予約テストは汎用builtin診断となることを確認後、新契約でGreen化。
- `rtk cargo nextest run -p sindr -p spire -p sigil -p diagnostics -p scar`: 1080 passed。
- `cargo check --workspace`: 成功。
- `cargo run -- test --quiet --all`: 成功。新規worktree初回はGit管理外の`tmp/sandbox`不在で
  file I/O 5件だけが失敗し、先行filesystem testが同directoryを作成した後の同一コマンドで成功。
- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`: 1885 passed。AST変更後の初回は
  Project modeの8 language-feature bucketが別々cold prefixを並列構築し、15秒timeoutで8件失敗。
  同binaryで個別・逐次実行を成功させた後、並列8 passed / 4.096sと最終workspace 1885 passed / 70.430sを確認。
- 独立Lunaレビューは、具象payloadの誤受理、表示名だけのsurface検証、impl target span欠落の3件を検出。
  型変数種別、canonical owner / Trait / builtin identity、一般`Ast::ImplDef` target spanと診断label / note / helpを修正後、
  再レビューはfindings 0件。Astra顧問も一般AST span保持を採用すべきと確認し、CI初回timeoutを
  binary fingerprint変更後のProject prefix cold並列構築と切り分けた。

N10は完了。N11の診断・全carrier統合検証を実装し、全件検証を終えた。commitは未作成。

### N08: syntax / AST / resolver / scope

既存RTA parserを再利用し、doの `<-` とFacet bulk_updateの `<-` を構文所有者で区別する。RHSはLHS bindingより先に解決する。適用済みcarrier型を新たなdo文法にしない。

状態: 完了。strict / tolerant lexerで`do`を予約し、compiler-ownedな`@intrinsic def do`の宣言名だけを
canonical surface検証用に受理する。既存RTA parserからlist処理を共通化し、head、完全・部分適用carrier、`_`、
省略を保持する。空list / 複数項 / `do<...>` / 外側constructor variableは、項目またはRTA listのsource spanと
structured parse reasonを保って拒否する。

Spire / Sigilにはlower前の`Do` nodeと`Extract` / `SafeBind` / ordinary statementを追加し、`<-` / `=?`の
operator spanを保持する。do全体をchild scopeに置き、各pattern statementはRHSを先にresolveしてからpattern bindingを
後続statementへ公開し、block外へは漏らさない。capture、warning、parallel ID rebase、process owner / self、impl `Self`、
bulk_update operation検査など、silent fallbackを持つAST / resolved visitorもdo内部へ再帰させた。

N08では型推論・loweringを行わず、`Resolved::Do`はScar入口でstructured `CompilePolicyViolation`として明示拒否する。
この境界はN09でcarrier推論とcore loweringへ置き換える。

検証（2026-09-14）:

- TDD Red: `Token::Do` / `Ast::Do` / `Resolved::Do`がないcompile failureを確認後に実装した。
- `rtk cargo nextest run -p spire -p sigil -p diagnostics -p scar -p surtr-analysis do_`: 37 passed、1104 skipped。
- `rtk cargo nextest run -p surtr-analysis project_runner`: 14 passed、122 skipped。
- `rtk cargo nextest run -p spire -p sigil -p diagnostics -p scar -p surtr-analysis`: 1141 passed。
- `cargo check --workspace`: 成功。
- `cargo run -- test --quiet --all`: 成功。新規worktree初回だけGit管理外の`tmp/sandbox`不在でfile I/O 5件が失敗し、
  directory作成後の同一コマンドで成功した。
- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`: 1911 passed。
- 独立サブエージェントレビューでRTA形状、silent visitor、状態文言、pin capture、pattern identity testを補正し、
  最終再レビューはfindings 0件。

### N09: carrier inference / core lowering

N09時点では、monadic originを一つのdo-local carrierへ結び付け、payloadを別型として扱う。常時Monad、partial `<-` はAlternative。通常 `=` の値保存を自動bindしない。

状態: 完了。Scarはvalidated `DoIntrinsicContract`のReturnTypeArgument position 0、expected result、`<-` RHS、
bare monadic expression、通常callのRTA推論、最終式を、通常のTypeCtorTrait / Trait dispatch経路で一つのcarrierへ統一する。
mapped payloadは各bindで独立に変化でき、captured / fixed argumentは同じcarrier identityとして固定する。

N09時点ではtotal `<-` とbare monadic expressionをconcrete `Monad::bind`、partial `<-` をwildcard failure armを持つmatchと
同じcarrierのconcrete `Alternative::empty`へ、Scar内で既存のTraitCall / Closure / Block / Matchへlowerした。
現在のpartial `<-` failureは`ResultEffect > Alternative > Monad`で選択し、Result effectがあるcarrierではErrorを保持する。
生成closureのcaptureはSigilのresolved capture collectorを再利用する。通常 `=` は値保存のまま、最終式だけのdoにも
Monadを要求する。do内SafeBindはN10のfailure target / Forge loweringが入るまでstructured `CompilePolicyViolation`として
fail closedを維持する。

検証（2026-09-14）:

- TDD Red: N08のScar入口 `CompilePolicyViolation` により追加したcarrier / lowering surface case 6件が失敗することを確認後、
  N09 routeでGreen化した。
- `rtk cargo nextest run -p scar --test typecheck_surface`: 9 passed。
- `rtk cargo nextest run -p scar`: 276 passed。
- `cargo check --workspace`: 成功。
- `cargo run -- test --quiet --all`: 成功。新規worktree初回だけGit管理外の`tmp/sandbox`不在でfile I/O 5件が失敗し、
  directory作成後の同一コマンドで成功した。
- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`: 2回連続で各1911 passed。
- total / partial / failure / bareの手動scriptを`cargo run -- run`で実行し、Identity、Option::Some、
  Option::Noneの既存Forge / Eldr経路を確認した。検証用scriptは削除済み。
- 独立Lunaレビューはfindings 0件。指摘された`Either<String, _>`のcaptured carrier成功例と、
  compiler生成continuationの外側local captureを追加テストで補完した。Astra顧問を必要とするblocking issueは発生しなかった。

### N10: SafeBind / Forge lowering

N06で合法と確定したSafeBindを再利用する。N10時点ではdo carrierがcanonical ResultならError保存、それ以外は
同じcarrierのemptyへ失敗先を変更したが、現在は[`do_intrinsic_spec.md`](do_intrinsic_spec.md)と
[`Trait_system_spec.md`](../docs/dev/Trait_system_spec.md) の現行契約により `ResultEffect > Alternative > Monad`へ置換済みである。total non-Result RHSの入力判定を緩和・再実装しない。
Error生成・RHS評価回数・source originを維持する。
do専用VM opcodeを追加しない。

状態: 完了。Scarはdo内SafeBindを専用のboxed typed controlへ正規化し、RHS projection、
後続continuation、source origin、およびResult effectのerror保存または具体化済み`Alternative::empty` dispatchを保持する。
SafeBind RHSの型検査substitutionはcarrier推論へ混ぜず、後続文から確定したcarrierに対してfailure targetを選択する。
Forgeはtyped targetごとの式内joinへlowerし、enclosing functionの`Return`やtop-levelの`Halt`へ依存しない。
Result effectを持たないcarrierではfailure payloadを生成・観測せず、一つのfailure handlerから保存済みempty callへ進む。
do専用opcodeとEldr変更は追加していない。

実装中、inline typed payloadによるScarのdebug stack frame増大を既存回帰テストが検出した。
Astra顧問のframe比較を受け、payload全体をbox化しspecialization rewriteを専用helperへ分離した。
既存の8 MiB stack regressionはテスト条件を変更せずGreenへ復帰した。

独立レビューで検出したtyped visitor、Result payload specialization、session function-index再配置、
複数SafeBind / SafeBind後の`<-`におけるresult origin、semantic cache schema、未知Extractor tagのfailure吸収を
回帰テストから修正した。未知tagはnon-Result doの`empty`へ丸めず`InvalidMatchResult`としてfail closedにし、
旧typed IRを含むstdlib semantic cacheはschema mismatchで再構築する。`TypedInner::DoSafeBind`の保存済みoriginを
既存の末尾return診断へ投影するvisitor拡張は、N11のdiagnostics / acceptanceへ引き継ぐ。

検証（2026-09-14）:

- TDD Red: obligation未走査、Result全体へのpattern具体化、pin dispatchのstale function index、未知Extractor tagの
  `Alternative::empty`吸収を直接テストし、4件が狙った理由で失敗することを確認後にGreen化した。
- `rtk cargo nextest run -p spire -p sigil -p scar -p forge -p xldr`: 1121 passed、78 skipped。
- `rtk cargo nextest run -p rune --test integration language_features`: 8 passed、123 skipped。
- `rtk cargo nextest run -p scar --test typecheck_surface typecheck_surface`: 8 passed、1 skipped。
- `cargo check --workspace`、`cargo run -- test --quiet --all`: 成功。
- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`: 最終差分で2回連続各1919 passed。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。
- 独立サブエージェントの最終再レビューはN10 blocker findings 0件。Astra顧問のstack frame比較を採用した後も、
  do専用opcode / Eldr変更、旧failure経路、曖昧なcarrier fallbackは追加していない。

N10の未検証範囲はない。commitは未作成。

### N11: diagnostics / acceptance

既存Option/List/Result/Either、N02のIdentity/Reader/State、N05のTransformer、ユーザ定義carrierを検証する。通常関数・pipeline・doの型と結果を比較する。

MI-15、CI-13/14、MT-S14をここで完了する。通常Identity/Reader/StateへAlternativeを追加しないまま、必要能力不足の拒否も確認する。Facetのsource scopeとcompiler生成closureを混同して合法な操作を誤拒否しない。

実装: `lib/tests/do.srt`でOption / Result / List / applied `Either<String, _>`、Identity / Reader / State、
OptionT / EitherT / ReaderT / StateT、ユーザ定義Monad carrierについて、通常pipelineと`do`の実行値を比較する。
Scarのsurface testsでIdentity / Reader / Stateのpartial `<-` とSafeBindがAlternativeなしで拒否されること、
SafeBind末尾値の型不一致が保存済みresult spanを使うこと、生成bind closure内のFacet path消費が許可されることを固定する。
`tail_expr_span`は`TypedDoSafeBind.origins.result_span`を直接参照し、node span配置へ依存しない。

検証（2026-09-15）:

- `rtk cargo nextest run -p scar`: 281 passed。
- `rtk cargo nextest run -p diagnostics`: 47 passed。
- `rtk cargo nextest run -p rune --test integration run_srt`: 11 passed、120 skipped。
- `cargo run -- test --quiet do`: 成功（12 cases）。
- `SURTR_TEST_CACHE=1 rtk cargo nextest run --profile ci --workspace`: 最終差分で2回連続各1922 passed。
- `cargo run -- test --quiet --all`: 成功。
- `cargo fmt --all -- --check`、`git diff --check`: 成功。
- `cargo nextest run -p rune --bin surtr do_return_diagnostic_keeps_the_same_source_facts_in_human_and_json`: 1 passed。human表示とJSONのdiagnostic reason / message / primary span、source factsを照合。
- 独立レビュー: 初回のdiagnostic owner / span、具象non-Monad reason、生成partial matchとuser branch優先順位の指摘を修正し、最終blocking findings 0件。human/JSON診断の直接比較テストも追加。

## 13. N12 — Generator core

公開型をGenerator<A>へ移し、hidden state/stepを保持して遅延生成する。既存nextの「次Generatorを返す」値意味論は維持する。

G-I01/G-I02を先に固定し、unfoldのcallbackアリティと終端/実エラーを曖昧にしたままruntimeを作らない。旧eager列構築経路を監査し、表現変更に必要なcode/metadata/cache境界を更新する。

完了条件: G-01–04、G-06/07、G-09、G-12–15のcore範囲。

## 14. N13 — Generator adapters / integration

map/filter、scan/map_accum、take/to_list、idx/with_indexの採用APIを一覧化する。G-I03–05を固定してから実装する。

旧takeがListを返す契約を、説明だけでlazy Generatorへ変えない。Generator<Result<A>>のitem失敗とproducerの終端を分離する。

Facet・field・collection・SafeBind・pipeline・REPL・do内の通常関数呼び出しを検証する。GeneratorをMonad carrierとして扱うための特例は追加しない。

完了条件: G-05、G-08、G-10/11、およびN12から残った全G条件。

## 15. N14 — 最終監査

各受け入れ条件IDを実際のtest/fixtureへ対応付ける。仕様例が書かれているだけの項目を完了扱いにしない。

最終不変条件:

- 型入力の導入元・RTAの意味が既存パターンから外れていない。
- 具象runtimeデータへ未確定carrierやTrait dictionaryを残していない。
- impl数・順序・表示名・field探索から型を逆決定していない。
- Monadインスタンス追加と言語機能追加のcommit/試験/移管先が分離している。
- doはTransformerの内部構造を知らず、Generatorにobservableなcursor mutationがない。
- 旧正本への生きた参照がなく、draftは保持されている。
- ExtractorはOption返却契約で検証し、独立した更改ドラフトの採用・実装をN14完了条件へ混入させていない。
- 実装済み仕様を `/docs` へ移し、未実装・未採用の内容を混入させていない。
- qualified builtin surfaceを`BUILTIN_METAS`以外のallowlistや表示名fallbackから合成していない。

## 16. 検証方針

各Taskは最小の失敗test、実装、focused test、関連integrationの順で確認する。変更のない仕様書作成だけを理由にcompiler全体のtest実行を必須にしない。

実装時のコマンドはリポジトリの最新AGENTS/テスト方針に従う。候補例:

```bash
cargo fmt --all -- --check
rtk cargo nextest run -p scar
rtk cargo nextest run -p rune --test integration run_srt
rtk cargo nextest run -p xldr --test repl_core
git diff --check
```

全体ゲートは既存計画の2回連続workspace成功を維持する。使用profile、環境変数、test数、skipped、timeout、exit statusを記録し、timeoutや未実行を成功と読み替えない。

REPLの手動検証環境に固有の起動方法はローカル担当の既存規約に従う。本書作成環境でREPLを操作したという記録は作らない。

## 17. 進捗の記録欄

Taskごとに次の形式で追記する。

```text
Task ID:
状態: 未着手 / 実施中 / 実装済み検証待ち / 完了
採用interface・仕様revision:
実装commit:
対応する受け入れ条件ID:
実行コマンド・結果・ログ:
未検証範囲:
docs移管先:
次Taskへの引継ぎ:
```

仕様変更が必要になったら担当仕様書を先に更新し、影響するTaskと受け入れ条件を記録する。進捗欄だけで言語仕様を変更しない。
