# TypeCtorTrait の carrier 同一性変更仕様

## 1. 状態・範囲・根拠

- 分類: `/doc` に置く、仕様決定・実装待ちの実装入力。
- 基準: `harukikubota/surtr_lang` の `f1986a27d84728e1e7a88de457a1d6b55cfe5ab8`。
- 着手前提: 旧 Type Constructor Signature Unification 計画の Task 9 まで完了。
- 本書の変更: 通常 callable の direct TypeCtorTrait 引数間にある、family 由来の暗黙 carrier 同一性を取り除く。
- 本書の対象外: MonadT の declaration bound、型引数に constructor を保持する拡張、標準 Monad 型の追加、Generator。

基準仕様は[`../docs/dev/Trait_system_spec.md`](../docs/dev/Trait_system_spec.md)の用語・構文・coherence・applicability・parent coverage、role付き型リスト、ReturnTypeArgument規則である。旧入力の実装済み契約は同正本へ移管済みであり、本書は実装待ちの差分だけを定める。

入力メモ「Surtr Monad Transformer 検討メモ」§1 と本会話で決めた「同じ family でも direct 引数は独立、同じ `$F` / `Self` で同一性を指定」を実装入力へ具体化する。旧ドラフトの本文は変更しない。

本書を `/doc` に追加しただけで、現在の `/docs` に記載された実装済み挙動を変更済みとして書き換えてはならない。実装が完了した時点で `/docs` へ反映する。

## 2. 設計意図

Trait family は能力の継承と constructor slot の対応を説明する。別の値引数が同じ具体的コンテナを使うかどうかまで、family の所属だけで決めない。

「どの能力を要求するか」と「どの型入力を共有するか」を分離する。

```surtr
# left と right は独立した carrier を受ける。
def inspect_pair(left: Functor, right: Monad) -> Unit {
  ()
}

# left と right は同じ carrier を受ける。
def combine(left: $F<$A>, right: $F<$B>) -> $F<($A, $B)>
where
  $F: Monad
{
  Monad::bind(left, {|a|
    Functor::fmap(right, {|b| (a, b)})
  })
}
```

上の第二例は同じ `$F` を両引数と戻り値で共有する。宣言 capability の利用は既存の `UnusedTraitConstraint` 規則でも検査し、本書では未使用 bound の例外を追加しない。

## 3. 同一性の生成規則

| 起点 | 規則 |
|---|---|
| 別々の direct TypeCtorTrait value parameter | それぞれ独立した carrier 入力を持つ |
| 同じ名前付き constructor variable `$F` の再出現 | 同じ carrier を要求する |
| Trait contract の `Self` / `Self<A>` の再出現 | 同じ実装対象と、検証済み mapped slot 関係を使う |
| 通常の型注釈・期待型・引数と parameter の照合 | 宣言にある型関係に従い通常の制約を生成する |
| `do` の monadic origin | intrinsic contract が所有する一つの do-local carrier へ結び付ける |
| 同じ TypeCtorTraitFamily に属すること | それだけでは別の carrier 入力を等置しない |

`Functor` と `Monad` の継承関係は維持する。異なる carrier を許可するために `Monad2` のような別 family を作る必要はない。

同じ direct Trait 名を二度書いても、別の value parameter なら独立である。同じ payload 型変数 `$A` を二度書くことは payload 型の一致を要求するだけであり、carrier の一致までは要求しない。

```text
宣言: left: Functor<A>, right: Functor<A>
正規化: left: F1<A>, right: F2<A>
関係: A は共有。F1 と F2 は独立。
```

## 4. capability view を維持する

carrier の統一と capability の昇格は別である。ある signature position で利用できる method は、その位置で宣言された capability に従う。

Functor のみを要求する入力を、偶然 Monad を実装している具象型で呼べるとしても、generic body がその入力へ無条件に `Monad::bind` を使ってよいことにはならない。

rigid generic は宣言済み bound と parent closure から証明する。呼び出しを見つけたことを理由に checker が bound を追加してはならない。

## 5. ReturnTypeArgument と戻り値の接続

本変更で ReturnTypeArgument の導入・省略規則を変更しない。

```surtr
def guard::<Alternative>(condition: Boolean) -> Alternative<Unit>
```

この宣言の ReturnTypeArgument と戻り値は、既存の return-only carrier 入力として接続する。引数独立化を理由に、この二つまで無関係な fresh carrier に分解してはならない。

一方、どの引数の carrier を戻り値へ保存するかを表したい通常関数は、名前付き constructor variable でその関係を書く。

```surtr
def map_value(value: $F<$A>, mapper: ($A -> $B)) -> $F<$B>
where
  $F: Functor
{
  Functor::fmap(value, mapper)
}
```

複数の direct 引数のうち最初・最後・同じ Trait 名のものを、戻り値の結び付け先として推測しない。direct 戻り値の導入元が既存の宣言規則で一意に定まらない場合は、名前付き `$F` または return-only の ReturnTypeArgument を要求する。

実装前の signature matrix では、少なくとも「direct 引数だけ」「同じ `$F` の引数と戻り値」「direct return-only RTA」「`Self` の Trait method」を別々に確認する。旧 family 共有に依存していた通常 helper は、意図する関係を `$F` で明記して移行する。

## 6. carrier identity

既存の canonical carrier 表現を再利用する。少なくとも constructor head、arity、全 mapped slot とその対応、captured/fixed arguments を保持する。

payload 型と carrier identity を混ぜない。標準例では次の関係になる。

| 比較 | 結果 |
|---|---|
| `Either<String, Int>` と `Either<String, Boolean>` | 同じ unary carrier、異なる payload |
| `Either<String, Int>` と `Either<Int, Boolean>` | captured argument が異なる carrier |
| `Reader<Config, Int>` と `Reader<Config, String>` | Reader 導入後は同じ carrier |
| `State<Int, Int>` と `State<String, Int>` | State 導入後は異なる carrier |

`Reader<Config, _>` 等はこの説明における carrier の表記であり、call-site や field に新しい部分適用構文を許可する宣言ではない。`_` の surface の意味は既存の型位置規則を維持する。

標準型の最後の型引数を暗黙に mapped slot と決めない。ユーザ定義型では検証済み slot mapping を用いる。

## 7. 通常呼び出し・演算子・関数 capture

`defmod` 内の通常関数、Trait method、inherent method、Trait impl method、通常 builtin、import 済み helper は同じ callable signature と solver を使う。

演算子は既存の Trait method へ lower し、その method contract の `Self` / `$F` の関係を使う。演算子だけ、または直接呼び出しだけが別 carrier を混在可能になる経路を作らない。

`&Trait::method` は既存の qualified method identity と通常の callable instantiation に従う。constructor value、dyn Trait、runtime dictionary を新設しない。

## 8. do との接続

`do` は通常関数の引数独立化とは別に、一つの do-local carrier 入力を所有する。

- `<-` RHS、bare monadic expression、最終式、および block の期待型をその carrier へ接続する。
- payload は文ごとに変化してよい。
- captured arguments は同一でなければならない。
- `=?` RHS は do carrier の推論源にしない。
- `=` で別のコンテナ値自体を保存することを monadic origin とみなさない。

`doc/do_intrinsic_spec.md` の変更は「family 所属だけで同一」を除き「intrinsic が宣言する同一性」を明記することに限定する。partial `<-`、SafeBind、Alternative、Result の既存の失敗方針をこの変更と一緒に再設計しない。

## 9. REPL・エラー・状態保存

成功した実行単位から公開されるデータ値は具象型を持つ。未確定 carrier は既存の ambiguity boundary でエラーにする。後続の別 REPL 入力が、すでに保存されたデータ値の型を後から決めることはない。

generic callable の宣言 scheme や compiler-managed な deferred Facet metadata と、未確定型を持つ runtime data value を混同しない。前者の既存機能を一括禁止したり、後者の抜け道にしたりしない。

candidate probe の失敗時は型置換、carrier、capability provenance、pending obligations をまとめて rollback する。REPL の checkpoint / restore でも carrier の独立性と宣言由来の共有関係を保存する。

## 10. 受け入れ条件

| ID | 検証 |
|---|---|
| CI-01 | 別 direct 引数に `Option<Int>` と `List<Int>` を渡せる |
| CI-02 | 同じ direct Trait 名でも別 value parameter は独立する |
| CI-03 | 同じ `$F` の引数へ異なる carrier を渡すと拒否する |
| CI-04 | 同じ `$A` は payload 同一性だけを要求する |
| CI-05 | `Self<A>` / `Self<B>` は同じ captured arguments を保持する |
| CI-06 | return-only direct RTA と戻り値の既存接続を維持する |
| CI-07 | `$F<A> -> $F<B>` の helper で carrier が保存される |
| CI-08 | capability view を越えた generic method 利用を拒否する |
| CI-09 | 引数・impl の登録順を入れ替えても判定が変わらない |
| CI-10 | 唯一の impl から未確定 carrier を逆決定しない |
| CI-11 | helper・演算子・capture で同じ型関係と dispatch を得る |
| CI-12 | 失敗 probe と REPL checkpoint が関係を漏洩しない |
| CI-13 | do 導入後、異なる carrier の monadic origin を拒否する |
| CI-14 | do 導入後、別 carrier の通常 `=` 束縛はそれだけでは拒否しない |
| CI-15 | direct TypeCtorTrait occurrenceがSigilで解決したcanonical Trait identityを保持し、short/display name検索を正しさの経路に使わない |
| CI-16 | constructor applicationのmetadata不足を`Deferred`と`Rejected`に区別し、実行可能な未解決`SelfApp`や理由を捨てる`Option`で継続しない |

## 11. 移管先

実装完了後は `docs/dev/Trait_system_spec.md`、`docs/site/trait-system.md`、`docs/site/trait-impls.md`、必要な型注釈ガイドへ現行規則として統合する。本書の契約を残したまま同じ説明を二重管理しない。

通常 Monad の追加は `monad_instances_spec.md`、MonadT の型機能は `monadt_language_extension_spec.md` を参照する。本書の実装完了を MonadT 実装完了とは数えない。
