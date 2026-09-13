# MonadT N05 文書移管・境界テスト follow-up 仕様

## 1. 状態と目的

- 対象 branch: `codex/type-constructor-monad-do-n05`
- 状態: **実装・局所検証完了、統合前**
- level: **3**

N03–N05 の実装済み契約と、N07–N11へ残した未実装契約を明確に分離する。
その上で、削除予定の次の入力文書に残る必要情報を、責務に合う文書・標準source・テストへ移管する。

- `doc/monadt_language_extension_spec.md`
- `doc/monadt_standard_types_spec.md`

今回は入力文書を削除しない。両文書には実施済み・追加作業・後続Task待ち・対象外が
節および受け入れ条件単位で分かる状態表示を追加する。

level 3 とする理由は、文書整理だけでなく、Xldr REPL の Facet 成功・拒否境界と、Scar の
parameterized `MonadT::lift` 拒否境界を追加するためである。Facet の現行動作が nominal source
identityを守っていない場合は、既存仕様へ戻すcompiler修正まで含む。新しい言語仕様は追加しない。

## 2. 変更しないもの

- MonadT、OptionT、EitherT、ReaderT、StateT の公開APIと意味論を変更しない。
- `do`、SafeBind、IdentityT、ResultT、抽象Transformer引数APIを追加しない。
- `defrecord` にgeneric parameterやconstructor parameterを追加しない。
- `crates/xldr/src/loader.rs` の登録構造、ロード順、関連仕様・テストを変更しない。
  loaderは今後の別作業で登録元を一本化し、順序依存を除去する。
- `docs/dev/テスト方針.md` へN05固有の受け入れ条件一覧を追加しない。
- 入力文書をこの作業内では削除しない。

## 3. 入力文書の状態表示

### 3.1 状態分類

両入力文書の冒頭に移管状況表を追加し、各節と受け入れ条件を次のいずれかへ分類する。

| 状態 | 意味 |
|---|---|
| 実装済み・移管済み | 実装と恒久正本または標準sourceの`@doc`が一致している |
| 実装済み・追加移管あり | 実装済みだが、本書で定める利用者文書・作業記録・テストの補強が残る |
| 未実装・後続Task | N07–N11など、確定済みだが後続Taskで実装・検証する |
| 対象外・open issue | 今回のMonadT実装では採用せず、別の設計判断へ移す |

本文の過去形・現在形だけから状態を推測させない。状態表から、実装正本、利用者向け説明、
検証根拠、残作業の行き先を直接参照できるようにする。

### 3.2 `monadt_language_extension_spec.md`

- N03–N04で完了したMT-L項目は、`doc/要件定義v9.md`、
  `docs/dev/Trait_system_spec.md`、関連する利用者文書、実テストを移管先として示す。
- `MT-L22` は **未実装・後続Task** とし、N07–N11および
  `doc/do_intrinsic_spec.md`を参照する。
- `defrecord` のgeneric / constructor parameterは **対象外・open issue** とする。
- `defenum`、`defstruct`と`defrecord`を同じ実装済み範囲に見せる記述を残さない。

### 3.3 `monadt_standard_types_spec.md`

- N05の採用APIと`MT-S01–13`、`MT-S15–18`は実装済みとして、標準source、利用者文書、
  実テストを対応付ける。
- `MT-S14` は **未実装・後続Task** とし、N11へ残す。
- §11の見出しと導入文は、未確定のままではなく「N05で確定したinterface判断」として読める形にする。
- 未採用の`map_inner` / `map_t`は候補APIではなく、非採用境界として明示する。

受け入れ条件IDの定義は、入力文書を保持する間は入力文書を正本とする。将来削除するときは、
生きたID参照を実装計画内の自己完結した記述または正本・実テストへの直接参照へ置換してから削除する。

## 4. 追加作業

### 4.1 実施済み契約とテストの対応

`doc/type_constructor_monad_do_implementation_plan.md` のN03–N05完了記録を、削除予定の入力文書を
開かなくても検証範囲が分かる形へ補強する。

- 受け入れ条件の意味を、単なる`MT-Lxx` / `MT-Sxx`の列挙ではなく短い説明とともに残す。
- 主な実テスト・fixtureを対応付ける。
- `MT-L22` / `MT-S14`だけはN11の未完了項目として残す。
- N14の最終監査で、受け入れ条件と実テストを再照合する。

この対応表は作業計画・完了記録であり、`docs/dev/テスト方針.md`には追加しない。

### 4.2 `defrecord` の範囲外記録

`doc/open-issues.md`へ、generic `defrecord` / constructor parameterを別issueとして追加する。

- N03はMonadTに必要な`defstruct`と`defenum`までを対象にしたことを記録する。
- record grammar、値表現、constructor surface、Facet再構築への影響を、実装前に別仕様で決める。
- MonadT完了条件やN05の残作業には含めない。
- 仕様未確定のままparserだけを`defstruct`へ合わせない。

### 4.3 Xldr Facet境界

`crates/xldr/tests/repl_core.rs`の
`core_nominal_constructor_value_survives_failed_bound_check`を修正する。

成功境界:

```surtr
value: ReplOptionT<Result, Int> = ReplOptionT(Ok(Option::Some(1)))
Facet::view(ReplOptionT.inner, value)
```

拒否境界:

```surtr
Facet::view(OptionT.inner, value)
```

後者は同じ名前・形状のfieldを持っていても、Facet pathのsource ownerが異なるため拒否する。
field名やrepresentation形状によるcross-nominal互換を追加しない。

実装時は拒否テストを先に確認する。現行compilerがすでに拒否するなら誤った成功期待だけを修正する。
現行compilerが受理するなら、Facet pathのsource type identityを最初に失うphaseを特定し、既存のnominal
identity契約へ戻す最小修正を行う。owner名allowlistやOptionT固有判定は追加しない。

### 4.4 ユーザ定義 MonadT の文書

`docs/site/monad-transformers.md`へ、標準型以外のbase MonadとTransformerを定義できることを追記する。

- 最小のuser-defined base / Transformer / `MonadT<$M>` implを示す。
- `lift`の出力はexpected typeまたは明示RTAで決める。
- compilerは標準型名、impl数・登録順、field名・representationからbaseや出力carrierを推測しない。
- runtime Trait dictionary、暗黙lift、Transformer専用runtime値を作らない。

例は既存の`tests/fixtures/script/pass/stdmod/user_defined_monad_transformer.srt`と意味を共有し、
文書専用の別surfaceを作らない。

### 4.5 Alternative の利用者向け意味論

`docs/site/monad-transformers.md`と該当標準sourceの`@doc`へ、次を追記する。

- `OptionT<List, A>`の`empty`は`[None]`であり、baseの空List`[]`ではない。
- `OptionT::choose`はbase由来のmultiplicityを保持し、分岐を集約しない。
- ReaderT / StateTの`choose`は両branchの通常評価を取り消さない。
- StateTでは両branchへ同じ初期状態を渡すが、外部作用をrollbackする契約ではない。
- Transformer内部の短絡と、通常の関数引数式のeager評価を区別する。

### 4.6 `MonadT::lift` のcaptured base拒否

`crates/scar/tests/parameterized_type_constructor_traits.rs`へ、完全なcarrier RTAがcaptured baseを
固定する拒否テストを追加する。

概念上の境界は次である。

```surtr
source: Base<Int> = Base::Base(1)
value = MonadT::lift::<Wrap<Other, _>>(source) # NG
```

payloadだけでなく、Trait argumentのbase carrierとimpl target内のcaptured baseが同じcanonical relationを
共有する場合にだけ成功させる。impl数・登録順・期待型から固定Trait argumentを書き換えない。

### 4.7 loaderは別作業へ分離

今回の変更ではloaderの仕様・テストを追加しない。現在の`STDLIB_MODULE_SPECS`、個別`include_str!`、
file name、module orderの重複は既知の設計負債として扱う。

後続のloader再設計では、単一の登録元からsource、file name、module identity、依存順を導出し、
可能な範囲で順序非依存にする。今回その将来構造を前提に局所的な固定テストを増やさない。

## 5. 入力文書を参照する文書の整理

参照元の性質に応じて修正する。

| 参照元 | 方針 |
|---|---|
| `doc/README.md` | 入力文書の実施済み／後続Task状態を表示する。将来削除時は恒久正本とN11入力へ置換する |
| `doc/type_constructor_monad_do_implementation_plan.md` | N03–N05の完了根拠を自己完結させ、N11の未完了条件だけを残す |
| `docs/superpowers/plans/*`などの履歴文書 | 履歴本文は書き換えず、冒頭の現行正本リンクだけを`docs/dev/Trait_system_spec.md`等へ向ける |
| `docs/dev/*` / `docs/site/*` | 削除予定の入力文書を正本として参照せず、恒久正本同士で閉じる |

activeな仕様・利用者文書と、historical planを同じ強さで書き換えない。履歴文書の古い設計本文は
履歴として保持し、現行契約に見えるリンクと注記だけを訂正する。

## 6. 実施順序

1. 両入力文書へ状態表を追加する。
2. `defrecord`を`doc/open-issues.md`へ分離する。
3. Facetの成功・拒否テストをTDDで確認し、必要な場合だけcompilerを修正する。
4. `MonadT::lift`のcaptured base mismatch拒否テストを追加する。
5. user-defined MonadTとAlternative意味論を利用者文書・`@doc`へ追加する。
6. 実装計画へN03–N05の契約・実テスト対応を追加する。
7. 入力文書を参照する文書を、それぞれのlevelに応じて更新する。
8. 全差分と状態表を再照合する。入力文書は削除しない。

## 7. 受け入れ条件

- 両入力文書の全節と`MT-L*` / `MT-SI*` / `MT-S*`が、実装済み、追加移管、後続Task、
  対象外のいずれかに分類されている。
- `MT-L22`と`MT-S14`がN11の未完了条件として明示され、完了扱いされていない。
- generic `defrecord`がN03–N05の実装済み契約から外れ、`doc/open-issues.md`に残る。
- `ReplOptionT.inner`を`ReplOptionT`値へ適用する成功と、`OptionT.inner`を同値へ適用する拒否が対になっている。
- full carrier RTAとvalue argumentのcaptured base不一致がScarで拒否される。
- `docs/site/monad-transformers.md`からuser-defined MonadTの最小経路と推論境界が分かる。
- OptionT / ReaderT / StateTのAlternativeについて、multiplicity、eager評価、同一初期状態、
  rollback非保証の境界が利用者文書または該当`@doc`から分かる。
- 実装計画のN03–N05完了記録が、削除予定文書のIDだけに依存せず実テストへ辿れる。
- activeな`docs/dev` / `docs/site`が削除予定文書を正本として参照していない。
- loaderのsource・test・仕様に今回由来の変更がない。
- 入力文書2件が削除されていない。

## 8. 検証

文書変更:

```bash
git diff --check
rg -n "monadt_language_extension_spec|monadt_standard_types_spec" doc docs
```

Facet境界:

```bash
rtk cargo nextest run -p xldr --test repl_core repl_core_bucket_3
```

MonadT RTA境界:

```bash
rtk cargo nextest run -p scar --test parameterized_type_constructor_traits
```

Facetのcompiler実装を変更した場合は、直接影響するScarのFacetテストとRune script fixtureを追加実行する。
最終的にlevel 3の同一最終差分で次を1回成功させる。

```bash
rtk cargo nextest run --workspace
cargo run -- test --quiet --all
cargo fmt --all -- --check
git diff --check
```

実行件数、skip、除外、失敗、未実行範囲を最終報告に記録する。timeout延長、ignored化、
旧経路fallbackでGreenを作らない。

## 9. 将来の入力文書削除ゲート

今回の受け入れ条件完了後も、削除は別の明示指示で行う。削除時は次を同一変更で満たす。

- `MT-L22` / `MT-S14`の定義と状態がN11側に残っている。
- N03–N05の完了根拠が実装計画、恒久正本、標準source、実テストだけで追跡できる。
- `doc/README.md`とactiveなMarkdown linkに削除対象への参照がない。
- 新規`docs/site/monad-transformers.md`、`lib/traits/monad_t.srt`、
  `lib/types/monad_transformer/*.srt`が追跡・統合対象に含まれている。
- 入力文書の削除、参照更新、移管先追加を分割せず、欠落状態を作らない。
