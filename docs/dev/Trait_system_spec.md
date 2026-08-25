# Trait System Implementation Spec

この文書は Trait system の実装者向け正本である。利用者が書く構文、`where` bound、impl の利用規則は
[`../site/trait-system.md`](../site/trait-system.md) と
[`../site/trait-impls.md`](../site/trait-impls.md) を正本とする。

## 1. パイプラインと phase ownership

Trait に関する情報は、構文から Forge まで argument、generic の対応、source span、qualified method
identity を失わずに運ぶ。

```text
Spire surface syntax
  -> Sigil resolved trait/type identity
  -> Scar TraitRef / TraitObligation / ImplPattern
  -> coherence | applicability | coverage
  -> concrete static dispatch
  -> Forge invariant
```

- Spire は where RHS を `Type<...>`、bare `Trait`、`TypeConstructorTrait.$Slot` に分類して保持する。通常 trait RHS の argument は保持しない。
- Sigil は Trait と type の canonical identity を解決する。block 内の callable 重複は map 登録前に拒否する。
- Scar は user source の overlap、不足 bound、未解決 dispatch を typecheck error として停止する。
- Forge は concrete dispatch だけを受け取る。ユーザ入力由来の trait conflict を `CodegenError` にしない。

## 2. 構造データと identity

Trait identity を display string や source generic 名で表してはならない。

```text
TraitRef {
  trait_id: TraitId,
  args: Vec<Ty>,
}

TraitObligation {
  trait_ref: TraitRef,
  subject: Ty,
  origin: DirectTraitCall | ImplWhereClause | ParentCoverage | DeclaredBound,
  span: Span,
}

ImplPattern {
  trait_ref: CanonicalTraitRef,
  target: CanonicalTy,
  where_constraints: CanonicalConstraintSet,
  constructor_slot_mapping,
  declaration_id,
  span,
}
```

`CanonicalTy` は primitive、rigid/inference/pattern variable、nominal application、その全 type argument、
tuple、function、constructor application などを再帰的に保持する。同じ source generic の再出現は同じ
canonical variable を使い、alpha-equivalence は source 名ではなく出現構造で判定する。

次を identity 判定に使用してはならない。

- `trait_name.contains('<')`、`split_once('<')`
- `ty_name`、`trait_display_name`、型の表示省略規則
- source generic 名や内部 variable 番号
- where の bare capability を expression の full obligation と取り違えること

表示名は診断生成にだけ用いる。

## 3. 独立した判定 API

共通の canonical type と unification primitive は再利用してよいが、次の量化を boolean helper や
「最初の一致候補」へまとめてはならない。

| 判定 | 量化 | 成功条件 |
|---|---|---|
| coherence | 存在 | 2 impl pattern を同時に満たす substitution が存在しない |
| applicability | 1 instance | candidate head と全 impl `where` obligation が成立する |
| parent coverage | 全称 | child の全 instance を 1 parent impl が cover し、parent `where` を証明できる |

### 3.1 Coherence

同じ base Trait の pattern は、trait argument 列と target を同一 unification 環境で再帰照合する。
左右の generic namespace は分離し、occurs check を行う。型 pattern が交差するなら `where` が異なっても
overlap である。V1 に specialization、most-specific dispatch、宣言順優先、negative bound、closed-world
disjointness proof はない。

`From` / `TryFrom` の排他は同じ canonical pattern unifier で、receiver target と変換元 Trait argument の
両方を検査する。disjoint な full pattern は同じ nominal target でも共存でき、Forge key も full identity を
保持する。

### 3.2 Applicability と obligation solver

candidate は requested `TraitRef` の全 argument と target を同じ fresh mapping で unify し、その mapping
で instantiate した impl `where` obligations を再帰的に証明する。head の一致だけでは適用可能ではない。

```text
ObligationResult =
  | Satisfied(Proof)
  | Deferred(DeferredObligation)
  | Unsatisfied(ObligationFailure)

DeferredObligation {
  obligation: TraitObligation,
  waiting_on: OrderedSet<InferenceVarId>,
}
```

`Deferred` を成功へ潰してはならない。candidate の obligation が一つでも `Unsatisfied` なら不適用、
一つでも `Deferred` なら candidate と依存する variables を保留し、全て `Satisfied` のときだけ dispatch を
確定する。

rigid generic は宣言済み bound（親 Trait closure を含む）からだけ証明する。direct call を理由に checker の
bound environment を変更してはならず、足りない場合は `MissingGenericBound` とする。inference variable の
obligation は deferred に登録する。

### 3.3 Proof environment と parent coverage

solver の仮定は checker-wide `tyvar_bounds` への一時書込みではなく、明示的な environment として渡す。

```text
ProofEnvironment {
  assumptions: OrderedSet<CanonicalObligationKey>,
}
```

parent coverage は child variables を rigid、parent variables を flexible として一方向 match を行う。
child `where` を仮定として、substitute 済み parent `where` obligations を同じ solver で証明する。`Self` は
child impl target に lower する。head coverage、constructor slot mapping、where entailment は別々に診断する。
複数の disjoint parent impl の和集合による coverage は V1 では行わない。

親 Trait closure は TraitRef の argument を substitution して導く。`Child<Int>` は `Parent<Int>` を導けても
`Parent<String>` は導かない。

## 4. Well-formedness と lifecycle

`where` bound を登録する前に、RHS の kind、resolved Trait / slot の存在、generic の scope、`Self` の
target substitution を検査する。通常 bound は bare trait family capability として proof environment に登録し、
where clause が未知の type variable を導入してはならない。`Type<...>` は trait definition where の `Self`
だけ、`Trait.$Slot` は TypeConstructor trait impl の slot map だけで受理する。

`Self<$...>` は declaration の既知 impl target への型位置 substitution であり、任意の generic application
ではない。`Self::...` と `Type::...` は value owner path として受理しない。

TypeConstructor trait application（例: `Applicative<$A>`）は通常関数または trait method signature の
direct parameter / return のみで受理する。parameter ごとに position-keyed の独立 witness を作り、return
には parameter witness と共有しない fresh concrete result witness を作る。nested type、field、local annotation、
closure signature では拒否する。return witness は本体検査の終了時に単一 concrete constructor へ確定しなければ
ならない。

bare capability は expression が trait call、operator lowering、または generic call の proof 引渡しで消費した
ときだけ full obligation を発行する。full obligation は `(trait_id, trait_args, receiver)` を構造化して保持する。
body / impl block の scope 終了時に未消費の bare capability は `UnusedTraitConstraint` TypeError とする。
trait parent、shape、slot map はこの判定から除外する。

### 4.1 Trait method の入力型スロット

Trait method の `Self` と `$...` 型変数は、型入力を導入するチャネルを 1 つだけ持つ。Scar は trait を
predeclare する前に、FunParams（`def method::<...>`）と value parameter の型を再帰走査して次を `TypeError`
として検査する。

- 同じ型変数が FunParams と value parameter の両方に現れてはならない。`Eq::eq::<Self>(self: Self, ...)`
  は不正であり、`Eq::eq(self: Self, ...)` と書く。
- FunParams に現れる型変数は戻り値にも現れなければならない。値引数から導入できない result/target slot を
  explicit specialization として observable に保つためである。
- 戻り値に現れる型変数は、FunParams または value parameter のどちらかで導入されなければならない。
  return-only slot は推論・dispatch の入力を持たないため不正とする。
- value parameter で導入した型変数は戻り値に現れてもよいが、現れる必要はない。

```surtr
deftrait Show {
  def to_string(self: Self) -> String
}

deftrait TryFrom<$To> {
  def try_from::<$To>(self: Self) -> Result<$To, Error>
}
```

trait impl method は trait head と impl target を代入・`Self` application を展開した後の FunParams、value
parameter、戻り値をまとめて alpha-normalize して比較する。FunParams の個数、順序、型構造、または他の
signature slot との関係が trait contract と異なれば incompatible signature とする。derive が生成する
`Show` / `Eq` / `Compare` method も、receiver/value parameter が `Self` を導入するため FunParams を生成しない。

### 4.2 `Default` derive の生成境界

`Default` trait の標準契約は次で固定する。

```surtr
deftrait Default {
  def default::<Self>() -> Self
}
```

`Self` は runtime value parameter ではなく、FunParams から導入する dispatch target である。したがって
`Default::default` の戻り値は常に `Self` でなければならず、`Result<Self, Error>` などへ変更してはならない。

`@derive Default` の resolver expansion は次の規則に従う。

- struct は各 field を `Default::default()` で埋めた struct literal を生成する。record は常に public な field を持つ
  unprotected aggregate として、同じ field-by-field construction を行う。
- 0 フィールド struct も有効な product とし、空の struct literal を生成する。`@derive Default` は inherent `new` を生成せず、struct のフィールド数に関係なく `new` 必須契約を緩和しない。
- enum は選択した variant を直接構築し、payload を `Default::default()` で埋める。
- structの生成で `Type(...)` や `Type::new(...)` の constructor surface を呼び出してはならない。
- struct literal は `impl Type` の同型メソッド本体内だけで許可されるため、struct の derive 生成コードは型所有者側の
  自動生成として扱う。record にはこの値保護境界を適用しない。
- derive は型固有の不変条件を検査・推論しない。各 field の default 値で構築してよいことを、型定義者が
  `@derive Default` によって明示的に許可する。

特に constructor が `Result<Self, Error>` を返す型でも、derive 側は constructor の戻り値を unwrap / match
して `Self` を取り出す経路を作らない。constructor の検証契約と field default の妥当性は別責任であり、default
値が不正になり得る型は `Default` を derive してはならない。

user-defined `Struct` はフィールド数に関係なく inherent `new` を持たなければならない。したがって
`defstruct Empty {}` と `@derive Default` は両立するが、`impl Empty { def new() -> Self { Empty {} } }` などの
`new` は別途必要である。`Empty()` は `new` のシグネチャに依存する constructor sugar であり、Default derive や
フィールド数とは疎結合である。

pending obligation、substitution、declared bound、source generic name は次のすべてで整合して移動する。

- inference variable の unify / concrete bind（失敗時 rollback を含む）
- local callable scheme の generalize / instantiate
- specialization clone
- checker checkpoint / rollback と REPL state clone / restore
- definition boundary と program boundary

複数 variable を待つ obligation は、1 variable の bind 後も残りの variable へ再 home する。bind は substitution を
仮適用して関連 obligation を再実行し、失敗時は substitution と pending state の両方を rollback する。
definition boundary では pending を監査し、rigid generic は `MissingGenericBound`、concrete type は obligation
error、なお不明な型は ambiguity error として止める。scheme に制約を保存しない V1 経路で obligation を黙って落としてはならない。Scar は Forge 前に全 pending dispatch を監査し、concrete dispatch 以外を渡してはならない。

## 5. Cycle、cache、visitor

cycle/cache key は `trait_id`、canonicalized trait arguments、canonicalized subject、proof environment projection
で構成する。表示名、source span、generic struct の外側 name だけを key に含めない。`Visiting(key)` への再入だけを
cycle とし、完了後は visiting set から必ず削除する。異なる assumption environment の proof を cache 共有しない。

`TypedInner` の子ノード走査は一つの exhaustive visitor に集約する。pending dispatch と bound variable の収集は
同じ child traversal を使い、少なくとも If、Match（guard/arm を含む）、Closure、aggregate、field access、
tuple/list/map、call/capture/inject、bind/block、pipe/compose/unary/binary、return/error wrapper を網羅する。
specialization 後、Forge 前に全 `TypedNode` を監査し、`TraitDispatch::Pending` があれば Scar の
`UnresolvedTraitObligation` として source span 付きで失敗させる。

## 6. Qualified method identity と診断

Trait method の identity は `(trait_id, method_name)` であり、target inherent namespace の member ではない。
default/inherited method table、Scar typed dispatch、Forge function index はこの qualified identity を保持する。
diamond inheritance の同名 method は qualified call で区別し、bare alias の衝突は import 規則で処理する。

diagnostic は source generic name と declaration span を保持する。内部 ID を `$6257` のように見せず、
full obligation に必要な trait target を示す。位置規則は note、bare capability への書換えは help に置く。
compile-fail contract は phase、error kind/message、primary/related span を安定させ、宣言・file・map iteration順に依存させない。

## 7. Call-site inference の境界

local non-expansive callable value は、environment に自由出現しない inference variable だけを scheme として
generalize し、call-site ごとに fresh instantiate する。capture、outer rigid generic、明示注釈、effectful expression
の value を一般化してはならない。

call、constructor、Trait helper、Apply/PipeApply、Compose/KleisliCompose は共通の argument inference route を使う。
expected type が unbound variable なら actual を synthesize して unify し、既知なら closure を check して shape を
内側へ伝播する。tuple の既知 slot、list element、`if` の全 branch、`match` の全 arm に expected type を伝播する。
空 collection や引数注釈のない曖昧 closure は、別の制約または expected type を要する。

## 8. テストと非目標

テスト配置・fixture の phase/error assertion・visitor 変更時の配置 matrix は
[`テスト方針.md`](./テスト方針.md) の 3.6.1 を正本とする。coherence の正逆順、nested pattern、parameterized
impl-head / expression obligation の argument mismatch、deferred rehome/rollback、finite recursion、bare capability
consumption、position-keyed parameter witness、fresh result witness、parent `Self` assumption、qualified diamond、
call-site scheme と expected propagation を unit と Rune fixture の両方で保持する。

workspace は既定 nextest profile で 2 回連続成功させる。timeout 引上げで性能問題を隠さず、新しい prelude-heavy
fixture は既存 bucket に集約する。

V1 の非目標は runtime trait object/dictionary dispatch、specialization/priority dispatch、negative trait bound、
closed-world proof、coinductive solving、任意 local value の全面的 let-polymorphism、effectful callable の自動
generalization である。
