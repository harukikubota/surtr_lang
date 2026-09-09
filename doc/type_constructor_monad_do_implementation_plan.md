# TypeCtorTrait・標準Monad・MonadT・do・Generator 実装計画

## 1. 起点と状態

- 基準: `harukikubota/surtr_lang` / `f1986a27d84728e1e7a88de457a1d6b55cfe5ab8`。
- 引継ぎ前提: Git履歴に残る旧 Type Constructor Signature Unification 計画のTask 1–9は完了。
- 基準commitの旧計画にもTask 9の完了・追修正後の検証記録がある。
- 本計画は残作業を再編した新しい管理ファイル。旧Taskのチェックボックスを継続しない。
- 新Taskは `N01`–`N14` と呼び、旧Task 9等と混同しない。
- この配布時点では、新Taskのローカル実装・テストは行っていない。全Taskを未着手で初期化する。

旧Task 1–9の手順を再実装タスクとしてコピーしない。ただし新しい変更による退行を検出するため、既存テストは引き続き実行する。

本書は作業管理資料であり、言語意味論の唯一の正本ではない。仕様の詳細は下表の担当文書に置く。配置案は従来の引継ぎどおり `/doc` とし、文書種別をimplementation planと明示する。完了後は仕様として `/docs` に全文移動せず、必要な履歴はVCS/作業ログへ残す。

## 2. 入力仕様

| ファイル | 責務 |
|---|---|
| `type_constructor_trait_extension_spec.md` | 通常callableのcarrier同一性変更 |
| `monad_instances_spec.md` | Identity / Reader / State追加 |
| `monadt_language_extension_spec.md` | nominal constructor parameter・parameterized TypeCtorTrait・MonadT契約 |
| `monadt_standard_types_spec.md` | OptionT / EitherT / ReaderT / StateTの意味とAPI |
| `diagnostics_cleanup_spec.md` | 旧Task 10のSafeBind・診断残作業 |
| 既存 `do_intrinsic_spec.md` | doの詳細仕様。新carrier規則と参照先を更新して利用 |
| `generator_spec.md` | 遅延・persistent GeneratorとAPI移行 |

実装済み共通契約は `/docs/dev/Trait_system_spec.md`、`/docs/dev/diagnostics.md` 等を利用する。削除する旧入力仕様は、必要な内容の移管が完了してから参照を切り替える。

## 3. 作業分割の原則

通常Monad追加とMonadT追加を別Task、別差分、別完了条件にする。

- N02は、既存の通常型・関数・Traitの仕組みで完了させる。N03–N05を依存として要求しない。
- N03–N04は言語機能。標準4Transformerをすべて作らなくても、最小ユーザ型で検証できるようにする。
- N05は言語機能完成後の標準API。helper候補をcompiler特例で実現しない。
- GeneratorはMonadTやdo開始の技術的前提ではない。通常の実施順はdoの後とし、独立した変更として管理する。
- 仕様の追加と実装済み `/docs` の更新を混同しない。

## 4. 全体順序と依存

| 新Task | 作業 | 主な依存 | 状態 | 旧計画との対応 |
|---|---|---|---|---|
| N01 | direct carrier同一性改修 | 旧Task 9までの基盤 | [ ] 未着手 | 新規 |
| N02 | Identity / Reader / State | 既存通常型・Trait基盤 | [ ] 未着手 | 新規・独立 |
| N03 | nominal constructor parameterとbound | N01 | [ ] 未着手 | 新規言語機能 |
| N04 | parameterized TypeCtorTrait・MonadT契約 | N03 | [ ] 未着手 | 新規言語機能 |
| N05 | 標準Transformer | N04。Identityを使うテストはN02 | [ ] 未着手 | 新規標準機能 |
| N06 | SafeBind・診断残作業・do開始ゲート | N01–N05の検証済みrevision | [ ] 未着手 | 旧Task 10を再編 |
| N07 | do compiler-owned contract | N06 | [ ] 未着手 | 旧Task 11 |
| N08 | do syntax・AST・resolver・scope | N07 | [ ] 未着手 | 旧Task 12 |
| N09 | do carrier推論・core lowering | N08 | [ ] 未着手 | 旧Task 13 |
| N10 | do SafeBind・Forge lowering | N09 | [ ] 未着手 | 旧Task 14 |
| N11 | do診断・全carrier統合検証 | N10 | [ ] 未着手 | 旧Task 15 |
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
| 同 §§6–9 | 一部実装済み・一部未実装 | Task 9 reason producerは実装済み。SafeBindのOption拒否、message heuristic、policy familyは残存 | 実装済み境界は`docs/dev/diagnostics.md`、残作業は`diagnostics_cleanup_spec.md` §§3–9 |
| 同 §§10–11 | 実装済み範囲と未移行family | JSON typed projectionとphase ownershipは実装済み。全phase producer移行は未完 | `docs/dev/diagnostics.md`。残るproducer/adapterは`diagnostics_cleanup_spec.md` §§6–8 |
| 同 §§12–15 | 作業履歴と未実装契約 | Task 9 commits/testsを祖先確認。SafeBind・全heuristic撤去・doは未実装 | N06、`do_intrinsic_spec.md`、N07–N11 |
| `type_constructor_signature_unification_task4_fallback_remediation_scope.md`の完了部分 | 実装済み | arity事前検査、canonical signature必須化、candidate rollback、RTA ambiguity tests | `docs/dev/Trait_system_spec.md` §§2.1, 3.2, 7、`docs/dev/diagnostics.md` |
| 同 canonical Trait / constructor application残件 | 確定未実装 | unique-only short-name lookup、理由を捨てる`Option`、未解決`SelfApp`保持が現存 | `type_constructor_trait_extension_spec.md` CI-15/16を含むN01 |
| 同 builtin / diagnostic残件 | 確定未実装 | qualified builtin allowlist合成とmessage heuristicが現存 | `diagnostics_cleanup_spec.md` §8.1・DC-14を含むN06 |
| `type_constructor_signature_unification_implementation_plan.md` Task 1–9 | 完了履歴 | commits `b1805d75`, `6221dffd`, `d134c95a`, `256c4b79`, `65ba6172`, `25c53c4e`, `dfe4a2e1`, `c28d185c`, `f2c0affd`と各review fixがHEAD祖先 | 恒久契約は担当`/docs`、実装手順・検証ログはVCS |
| 同 Task 10 | 確定未実装 | SafeBind旧制限とdiagnostic heuristicが現存 | `diagnostics_cleanup_spec.md` / N06 |
| 同 Task 11–15 | 確定未実装 | `Ast::Do` / `Resolved::Do` / `TypedDo` / `DoBlock` / `IntrinsicId::Do`なし | `do_intrinsic_spec.md` / N07–N11 |
| 同 Task 16 | implementation plan | 最終監査はdo/Generator等の完了後 | N14 |
| `signature_level_type_constructor_inference_draft.md` | draft | import前後のblob hash一致を確認 | 本文を変更せず`doc/`に保持 |

## 6. N01 — direct carrier同一性

担当: 通常signatureの正規化、carrier relation、signature/dispatch診断、関連stdlib helperとテスト。

別direct引数を独立化し、同じ `$F` / `Self` と明示契約による共有を維持する。RTAとreturn-only carrierの既存関係を切断しない。

完了条件: `CI-01`–`CI-12`と`CI-15/16`。CI-13/14のdo確認はN11へ引き継ぐ。実装済み `/docs` とhelperのsignatureを新規則へ同期する。

## 7. N02 — 通常Monadインスタンス

担当: `lib/types/{identity,reader,state}.srt` 相当、標準読込み、標準テスト、利用者向け `@doc`。

Identity / Reader / State、new/runと固有primitive、Functor/Applicative/Monadを追加する。通常型・関数型field・明示slot mapping・既存Trait dispatchだけで実装する。

Readerは関数入力Rを渡すデータ構造。Stateは次状態を返す純粋な状態遷移構造。Processへ接続する専用機能を作らない。

完了条件: MI-01–MI-14、MI-16。MI-15のdo比較はN11。MonadTがなくてもこのTaskを完了できる。

## 8. N03 — nominal constructor parameter

担当: declaration boundの構文metadata、型式の適用、nominal well-formedness、型変更可能な再構築、REPL具体化。

`defstruct OptionT<$M: Monad,$A>` と `$M<Option<$A>>` を最小ユーザ型で検証する。constructor slotと通常型parameterを明確に扱い、無制約の一般HKT推論を追加しない。

実装前にMT-I01/03の初回範囲を明記する。新しい部分適用構文・associated type・型lambdaを勝手に導入しない。

完了条件: MT-L01–06、MT-L13/14/15の該当部分。field/return/annotation/Facetでdestinationのboundを検査する。

## 9. N04 — parameterized TypeCtorTrait / MonadT契約

担当: captured Trait parameter、parent shapeの継承、TraitRef、method type list、applicability、static instantiation。

MonadTの最小contractを追加し、Selfとbaseの独立したcarrierを保持する。field layoutを発見する機能や、標準型の第1引数をbaseとする位置規則を作らない。

MT-I02/04を記録した上で、ユーザ定義の最小MonadT実装を用いて、ordinary call/RTA/expected typeだけでliftを解決する。

完了条件: MT-L07–12、MT-L14–16、およびN03からの残り。N05の標準4型完成をこのTaskの唯一の検証方法にしない。

## 10. N05 — 標準MonadT実装型

担当: 標準 OptionT / EitherT / ReaderT / StateT、個別Trait impl、API、法則テスト、field/Facet/REPL。

着手時にMT-SI01–04の採用一覧を残す。必須new/run/lift/固有primitiveと、任意のmap_t等を区別する。未確定helperは必要な契約を決めるまで追加しない。

Alternativeは各型で条件付き実装する。IdentityT、抽象Transformer引数API、ResultT特例を追加しない。

完了条件: MT-S01–13、MT-S15–17。doのMT-S14はN11へ引き継ぐ。

## 11. N06 — SafeBind / diagnostics cleanup / do開始ゲート

担当: `diagnostics_cleanup_spec.md`。旧Task 10の残作業を引き継ぐ。

Result一段projection、non-Result pass-through、remaining phase producer、heuristic撤去、Rune/Xldr adapter、
qualified builtin surfaceの`BUILTIN_METAS`構造化を完了する。

旧Task 10に含まれるdraft削除、draft本文を含めた旧語彙ゼロ件検査、Task 9の再実装は行わない。

完了条件: DC-01–DC-14。既存全体ゲートどおり同じrevisionでworkspaceを2回連続成功させ、ログとcommitを記録する。do実装symbolがまだないことを確認する。

## 12. N07–N11 — do

### N07: compiler-owned contract

既存 `do_intrinsic_spec.md` のcompiler-owned contract、DoBlock、surface validationを実装する。do-local carrierの単一性をcontract自体へ明示する。

### N08: syntax / AST / resolver / scope

既存RTA parserを再利用し、doの `<-` とFacet bulk_updateの `<-` を構文所有者で区別する。RHSはLHS bindingより先に解決する。適用済みcarrier型を新たなdo文法にしない。

### N09: carrier inference / core lowering

monadic originを一つのdo-local carrierへ結び付け、payloadを別型として扱う。常時Monad、partial `<-` はAlternative。通常 `=` の値保存を自動bindしない。

### N10: SafeBind / Forge lowering

N06のSafeBindを再利用し、canonical ResultならError保存、それ以外は同じcarrierのemptyへ失敗先を変更する。Error生成・RHS評価回数・source originを維持する。do専用VM opcodeを追加しない。

### N11: diagnostics / acceptance

既存Option/List/Result/Either、N02のIdentity/Reader/State、N05のTransformer、ユーザ定義carrierを検証する。通常関数・pipeline・doの型と結果を比較する。

MI-15、CI-13/14、MT-S14をここで完了する。通常Identity/Reader/StateへAlternativeを追加しないまま、必要能力不足の拒否も確認する。Facetのsource scopeとcompiler生成closureを混同して合法な操作を誤拒否しない。

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
