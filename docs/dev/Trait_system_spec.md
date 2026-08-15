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

- Spire は parameterized `where` RHS を構文として保持する。
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
- `TypedWhereConstraintRhs::Trait` の `args` を捨てた base Trait 名

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

`where` bound を登録する前に、resolved Trait の存在、argument arity、argument 内 generic の scope、`Self` の
owner target への lowering、nested type 内までの再帰検査を行う。where clause が未知の type variable を導入してはならない。

pending obligation、substitution、declared bound、source generic name は次のすべてで整合して移動する。

- inference variable の unify / concrete bind（失敗時 rollback を含む）
- local callable scheme の generalize / instantiate
- specialization clone
- checker checkpoint / rollback と REPL state clone / restore
- definition boundary と program boundary

複数 variable を待つ obligation は、1 variable の bind 後も残りの variable へ再 home する。bind は substitution を
仮適用して関連 obligation を再実行し、失敗時は substitution と pending state の両方を rollback する。
definition boundary では pending を監査し、rigid generic は `MissingGenericBound`、concrete type は obligation
error、なお不明な型は ambiguity error として止める。scheme に制約を保存しない V1 経路で obligation を黙って落としてはならない。

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
`$A must implement Convert<Int>` と必要な `where` hint を示す。compile-fail contract は phase、error kind/message、
primary/related span を安定させ、宣言・file・map iteration 順に依存させない。

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
argument mismatch、deferred rehome/rollback、finite recursion、parent `Self` assumption、qualified diamond、
call-site scheme と expected propagation を unit と Rune fixture の両方で保持する。

workspace は既定 nextest profile で 2 回連続成功させる。timeout 引上げで性能問題を隠さず、新しい prelude-heavy
fixture は既存 bucket に集約する。

V1 の非目標は runtime trait object/dictionary dispatch、specialization/priority dispatch、negative trait bound、
closed-world proof、coinductive solving、任意 local value の全面的 let-polymorphism、effectful callable の自動
generalization である。
