# Trait System Implementation Spec

この文書はTrait systemの実装者向け正本であり、用語、受理構文、`where`分類、型推論、dispatchの契約を定める。
利用者向け説明は[`../site/trait-system.md`](../site/trait-system.md)と
[`../site/trait-impls.md`](../site/trait-impls.md)へ同期する。移行中に記述が衝突する場合はこの文書を優先する。

## 0. 用語、トークン、構文索引

この節は Trait、generic、TypeConstructor に関する正規語彙と surface syntax の索引である。同じ概念を
別名で呼ばず、AST、Resolved IR、Scar metadata、診断、テスト名もここで定める語へ揃える。

### 0.1 正規用語

| 用語 | 定義 |
|---|---|
| Trait | 型が提供する compile-time capability と、qualified method contract をまとめた宣言。runtime trait object や hidden dictionary ではない |
| Trait identity / `TraitId` | 表示名や型引数を含まないcanonical Trait識別子 |
| Trait head | `deftrait Trait<$P>` または `impl Trait<Arg> for Target` の `Trait<...>` 部分 |
| Trait-head type parameter | `deftrait Trait<$P>`の`$P`。Trait宣言側のbinder |
| Trait argument | `Trait<Int>`の`Int`。Trait-head type parameterを具体化する適用側の型 |
| `TraitRef` | `TraitId`と順序付きTrait argumentsの組 |
| parent Trait / parent closure | `where Self: Parent`で要求するTraitと、Trait argumentsを置換しながら推移的に導く全parent `TraitRef` |
| Trait method contract identity | `(trait_id, method_name)`。どのTrait contractかを表す |
| Trait method implementation identity | contract identityとimpl identityを組み合わせた具象callable識別子 |
| TypeConstructor | `List<_>`、`Option<_>`のように、型引数を受け取って具象型を作る型レベルのconstructor |
| TypeCtorTrait | `Self: Type<$A, ...>`でconstructor slotを宣言するTrait。`Functor`、`Applicative`、`Monad`など |
| TypeCtorTraitFamily | TypeCtorTrait間の継承関係が作る連結成分。同じfamily内のTrait slotは一つの具象carrierを共有する |
| family root | TypeCtorTraitFamily内で上位のTypeCtorTrait parentを持たないTrait。rootが複数あっても、継承で連結されるなら同じfamily |
| constructor slot | `Self: Type<$A, ...>`の`$A`。TypeCtorTraitが観測・置換するcontainer内部の型位置 |
| capturing TypeCtorTrait slot | `Functor<$A>`のようにcontainer内部型を型変数へ束縛して観測するdirect signature位置 |
| non-capturing TypeCtorTrait slot | bare `Functor`のようにcontainer内部型を束縛しないdirect signature位置。carrier同一性の検査には参加する |
| captured impl-target type parameter | impl targetの型引数のうちconstructor slotへmapされず、`Self<$...>`で保持される型位置 |
| carrier | TypeCtorTrait slotを満たす具象または部分適用済みTypeConstructor。`Either<String, _>`ではcaptureされた`String`もidentityに含む |
| generic | 型変数を含み複数の具象型へinstantiateできる性質。個々の入力を指す名詞には「型変数」を使う |
| 型変数 | `$A`、`$F`のような`$`付き識別子。同じ宣言内の同じ識別子は同じ型を表し、別の識別子は通常どおり独立する |
| value parameter | `def f(value: Ty)`の`value: Ty`。その型に現れる型変数を引数位置から導入する |
| ReturnTypeArgument（戻り値型引数） | `def f::<TYPE>(args) -> Return`の`TYPE`位置。値引数から導入できず、戻り値に現れる型入力を宣言する |
| call-site ReturnTypeArgument | `f::<TYPE>(args)`の`TYPE`位置。定義側ReturnTypeArgumentへ具象型を与える。generic一般を任意に指定する構文ではない |
| obligation subject | Trait capabilityを要求される型。runtime value receiverがない`Default::default`でも存在する |
| receiver | `self`などruntime/value-level receiverが実在するときの値と型。obligation subjectの同義語ではない |
| `Self` | Trait定義では実装対象、implではimpl targetを表す予約型。通常の名前付き型変数ではない |
| `Self` application | `Self<$A, ...>`。Trait/impl signatureで既知impl targetの型引数位置を置換するmarker。TypeCtorTraitではconstructor slotを置換する |
| trait constraint | `where $A: Eq`のように、型変数または許可されたsubjectへ要求するTrait capability |
| bare capability | `where`右辺の型引数なしTrait名。宣言scopeで利用可能なTrait capabilityを表す |
| full obligation | 式のdispatchが要求する`(TraitRef, obligation subject)`。bare capabilityから必要時に構築する |
| 型形状指定 | Trait定義だけで使える`Self: Type<$A, ...>`。通常のtrait constraintではない |
| slot mapping | TypeCtorTrait implだけで使える`$T: Trait.$Slot`。impl targetの型位置をconstructor slotへ対応付ける |
| impl target | `impl Trait for Target`または`impl Target`の`Target`。Traitの`Self`を具体化する型pattern |
| concrete dispatch target | obligation solverが選んだbuiltinまたはuser function。impl targetやReturnTypeArgumentとは別のcallable identity |
| rigid variable | 宣言済みgenericを検査中に任意の具象型へ確定させない内部型変数 |
| inference variable | call-site制約から具象型を推論する内部型変数 |
| pattern variable | impl headの構造照合で置換を受け取る内部型変数 |
| callable instantiation | generic callableを具象型substitutionでclone/具体化する処理。V1非対応のimpl specializationとは別概念 |

内部名はconceptをそのまま表し、ReturnTypeArgumentには`ReturnTypeArgument`、
`return_type_argument`、`return_type_arguments`を使う。非正規な歴史的名称、generic parameter、曖昧なslot名へ
置き換えてはならない。`::<...>`を保持する全phaseと診断・テストも同じ名称へ更改する。

### 0.2 トークン索引

| トークン | この仕様での役割 |
|---|---|
| `deftrait` | Trait宣言を開始する |
| `impl` | inherent implまたはTrait implを開始する |
| `for` | Trait headとimpl targetを分ける |
| `where` | 宣言済みsubjectへ制約、型形状、slot mappingのいずれかを列挙する |
| `Self` | Traitの実装対象型、またはimpl targetの置換形 |
| `Type` | `Self: Type<...>`でだけ使うcompiler-special surface name。小文字`type`と同じlexer tokenへ分類されるが型alias keywordとは役割が異なる |
| `$` | 名前付き型変数、constructor slot、slot mapping対象を導入する |
| `<...>` | Trait-head type parameter、Trait argument、named type argument、型形状slot、型applicationを表す |
| `::<...>` | 定義またはcall-siteのReturnTypeArgument位置を表す |
| `:` | value parameterの型、または`where` subjectと右辺を分ける |
| `+` | 同じ`where` subjectに複数の右辺制約を課す |
| `.` | `Trait.$Slot`でTraitとconstructor slotを結ぶ |
| `::` | `Trait::method`、`Type::method`などqualified value pathを作る。`::<...>`ではReturnTypeArgument開始記号の一部 |
| `->` | callableの引数型と戻り値型を分ける |
| `_` | その位置の型を周囲から推論するhole。新しい名前付きgenericを宣言しない |
| `def` / `defp` | public / private method宣言。bodyのないpublic Trait methodは実装必須契約になる |
| `@autoimport` | Trait helper aliasをfile-local preludeへ入れるTrait単位のopt-in |
| `@derive` | 対応Trait implをresolverが生成する型宣言側annotator |

`Type`は型形状指定のcompiler-special surface name、`TypeConstructor`はcompiler内部のkind/identity分類、
`TypeCtorTrait`は`Self: Type<...>`を持つTraitの分類であり、相互に同義ではない。

### 0.3 宣言構文一覧

| 構文 | 許可位置 | 意味と制約 |
|---|---|---|
| `deftrait T { ... }` | top level | 型引数を持たないTraitを宣言する |
| `deftrait T<$P, ...> { ... }` | top level | Trait-head type parameterを持つTraitを宣言する。`$P`はTrait headで導入される |
| `deftrait T<$P: Bound> { ... }` | top level | Trait-head type parameterへ単一のdeclaration boundを付ける。複数constraintはTrait `where`へ置く |
| `deftrait T<$P> where Self: Type<$A> { ... }` | なし | directまたは継承でTypeCtorTraitに分類されるTraitはTrait-head type parameterを持たない |
| `defstruct S<$P, ...> { ... }` | top level | field型で使うtype parameterを持つnominal structを宣言する |
| `defenum E<$P, ...> { ... }` | top level | variant payload型で使うtype parameterを持つnominal enumを宣言する |
| `type F<$P, ...> = (Args -> Return)` | top level | type parameterを持つ関数型aliasを宣言する。任意のdata type aliasではない |
| `@builtin type T<$P, ...>` | 標準定義source | builtin TypeConstructorのsurface headを宣言する。追加・変更の実装正本ではない |
| `deftrait T where Self: Parent { ... }` | Trait定義where | bare parent capabilityを宣言する |
| `deftrait T where Self: Type<$A, ...> { ... }` | Trait定義whereだけ | TypeCtorTraitの型形状とconstructor slotを宣言する |
| `def method(args) -> Return` | Trait、Trait impl、inherent impl、通常関数 | 値引数の型から必要な型変数を導入する |
| `def method::<R, ...>(args) -> Return` | callable定義 | 値引数から導入できず戻り値に現れる型入力をReturnTypeArgumentとして宣言する |
| `def method<$P>(args) -> Return` | なし | 通常callableは`<...>`によるtype parameter listを宣言しないため構文エラー |
| `impl Target { ... }` | top level | `Target`のinherent method namespaceを宣言する。Trait-style引数や`for`を持たない |
| `impl Trait for Target { ... }` | top level | 型引数を持たないTraitの実装を宣言する |
| `impl Trait<Arg, ...> for Target { ... }` | top level | Trait argumentを持つTrait実装を宣言する |
| `impl Trait for Target where $A: Bound { ... }` | Trait impl where | impl headで導入済みの型変数へtrait constraintを課す |
| `impl TypeCtorTrait for Target where $T: Trait.$Slot { ... }` | TypeCtorTrait impl whereだけ | impl targetの型位置をconstructor slotへmapする |

通常関数、method、Trait head、impl head、`where`のどれも、未知の型変数を暗黙には導入しない。
型変数の導入元は、その構文で認められたTrait-head type parameter、impl target、value parameter、
ReturnTypeArgument、constructor slotのいずれかでなければならない。

### 0.4 `where`構文一覧

| 分類 | 構文 | 許可される宣言 | 意味 |
|---|---|---|---|
| trait constraint | `$A: Eq` | 関数、method、Trait、impl | `$A`へbare capabilityを一つ要求する |
| 複数trait constraint | `$A: Eq + Show` | 関数、method、Trait、impl | 同じ型変数へ複数capabilityを要求する |
| parent Trait | `Self: Parent` | Trait定義 | Trait実装にparent capabilityを要求する |
| 型形状指定 | `Self: Type<$A, ...>` | Trait定義だけ | TraitをTypeCtorTraitとして分類しconstructor slotを宣言する |
| slot mapping | `$T: Functor.$A` | TypeCtorTrait implだけ | impl targetの`$T`を`Functor`の`$A` slotへ対応付ける |

通常の関数定義`where`はtrait constraintだけを受理する。Trait定義`where`はtrait constraintと型形状指定、
Trait impl `where`はtrait constraint、TypeCtorTrait implに限ってslot mappingを受理する。`where`はsubjectを
導入せず、左辺はその宣言ですでに導入済みでなければならない。

次は不正である。

```surtr
# parameterized bound: full Trait identityはimpl headまたは式dispatchで指定する
where $A: Encode<String>

# Trait名は制約対象となる型変数ではない
where Applicative: Add

# ReturnTypeArgument位置へ制約を書かない
def stop::<$F: Monad>() -> $F<Unit>

# 型形状指定を関数whereへ置かない
def f(value: $F<$A>) -> $F<$A>
where $F: Type<$A>

# slot mappingを通常関数やTrait定義へ置かない
where $A: Functor.$A
```

複数制約が必要なら名前付き型変数を導入する。

```surtr
def stop::<$F>() -> $F<Unit>
where $F: Applicative + Add
```

### 0.5 型位置とReturnTypeArgument

| 型構文 | 許可条件 |
|---|---|
| `$A` | その宣言で導入済みの通常型変数 |
| `List<$A>` | named TypeConstructorへの通常の型application |
| `$F<$A>` | `$F`に対する`where $F: TypeCtorTrait`がある場合だけ。`TypeCtorTrait`は実際のTrait名を表す |
| `Self` | Trait signatureまたはimpl methodの型位置 |
| `Self<$A>` | Trait/impl methodの型位置。TypeCtorTraitではconstructor slotを、generic inherent implでは既知owner targetの型引数位置を置換する |
| `Functor<$A>` | TypeCtorTrait名を直接使えるparameter / returnの直下だけ。field、local、nested type、closure signatureでは不正 |
| `Functor` | container内部型を観測しないdirect TypeCtorTrait slot。parameter / returnの直下だけ |

通常Trait名はparameter / returnの型にならない。direct type syntaxを持つのはTypeCtorTraitだけである。
parameterのdirect TypeCtorTraitは、その位置で利用できるcapabilityを制限する名前付きでないcarrier slotとして扱う。
returnのdirect TypeCtorTraitは、具象carrierをcall-site制約で選ぶ`impl Trait`相当として扱い、trait objectにはしない。
一方、`$F<$A>`は名前付きconstructor variableなので、`where $F: Applicative + Add`のように複数constraintを
付与できる。この差を型表示やdiagnosticで失ってはならない。

値引数の型に現れる型変数はvalue parameterから導入され、ReturnTypeArgumentへ重ねて宣言してはならない。
戻り値にだけ現れる型変数はReturnTypeArgumentで宣言しなければならない。値引数から導入された型変数は
戻り値でも再利用できる。

```surtr
# OK: $Fと$Aはvalue parameterから導入される
def fmap_value(value: $F<$A>, mapper: ($A -> $B)) -> $F<$B>
where $F: Functor

# NG: $Fをvalue parameterとReturnTypeArgumentで二重に導入している
def bad::<$F>(value: $F<$A>) -> $F<$A>
where $F: Functor

# NG: 戻り値だけの$BがReturnTypeArgumentで宣言されていない
def missing() -> $B

# OK: 戻り値だけの$Bを宣言する
def make::<$B>() -> $B
```

`$F`の制約がTypeCtorTrait一つだけなら、parameter、return、ReturnTypeArgumentでTrait名を直接指定できる。
direct Trait syntaxはfreshな型変数と`where`制約へ正規化する。

```surtr
def guard::<Alternative>(cond: Boolean) -> Alternative<Unit>

# normalization model
def guard::<$F>(cond: Boolean) -> $F<Unit>
where $F: Alternative
```

同じ関数定義内で同じTypeCtorTraitFamilyに属するdirect TypeCtorTrait slotは、一つの具象carrierへ解決する。
`Monad`から`Functor`へ継承pathがあるなら異なるcarrierを渡せない。異なるfamilyなら別のcarrierを渡せる。
container内部の型変数は通常どおり識別子単位で分離し、parameter位置で利用できるmethod能力は、そこに書かれた
Traitまでに制限する。

```surtr
def same_family(left: Functor, right: Monad) -> Unit {
  # leftとrightは同じcarrier。leftにはFunctor能力、rightにはMonad能力だけを認める
  ()
}

def different_family(left: Monad, right: Monad2) -> Unit {
  # MonadとMonad2が異なるTypeCtorTraitFamilyなら異なるcarrierを渡せる
  ()
}
```

non-capturing TypeCtorTrait slotはcontainer内部型を公開しないが、二つの引数span、または引数と戻り値spanを
関連labelとしてcarrier不一致を診断できる。call-site ReturnTypeArgumentのlabelは型名だけでよく、値側の
二つのspanへactual carrier型と同一carrier要求を表示する。

### 0.6 呼び出し構文一覧

| 構文 | 意味 |
|---|---|
| `Trait::method(args)` | qualified Trait method call。Trait identityを明示する |
| `method(args)` | import / auto-import済みTrait helper aliasまたは通常callableの呼び出し |
| `method::<Type>(args)` | 定義側ReturnTypeArgumentへcall-siteから型を与える |
| `Trait::method::<Type>(args)` | Trait identityとReturnTypeArgumentをともに明示する |
| `Type::method(args)` | inherent owner method call。Trait impl methodをtarget inherent memberとして追加しない |
| `&Trait::method` | qualified method capture。dispatch identityを保持する |
| `&Trait::method::<Type>` | ReturnTypeArgumentを適用したqualified method capture |
| trait-dispatched operator | operatorごとに定めたTrait method callへloweringする |

call-site ReturnTypeArgumentは定義側に対応位置がある場合だけ指定できる。期待戻り値、型注釈、他のsignature制約から
一意に得られる場合は省略でき、引数位置からも期待型からも得られない場合は`::<Type>`で明示する。
`::<$F: Monad>`のようなcall-site制約指定は受理しない。

### 0.7 実装移行境界

0.1–0.6は移行後の正本契約である。実装に旧名称や旧slot規則が残っていても、それをsurface仕様として追認しない。
修正フェーズは次を一つの移行単位として完了させる。

- 通常関数、Trait method、inherent method、Trait impl methodの定義側`::<...>`を同じReturnTypeArgument parser routeへ載せる。
- Spire AST、Sigil Resolved IR、Scar metadata、callable instantiation、semantic metadataの対応field/typeを
  `return_type_argument(s)`へ改名し、旧内部名を残さない。
- value parameterを表す内部typeは`ValueParameter`、`ResolvedValueParameter`、`TypedValueParameter`へ揃え、
  ReturnTypeArgumentの内部名と文字列上も衝突させない。VM function tableなど無関係な物理slot名は対象外とする。
- call-siteの`::<...>`をgenericなtype applicationとして命名せず、expression nodeは
  `ReturnTypeArgumentApply`、保持fieldは`return_type_arguments`へ揃える。任意generic指定に見える内部名も残さない。
- source diagnostic、JSON diagnostic、unit test、fixture、rustdoc、公開文書をReturnTypeArgument用語へ揃える。
- `$F<$A, ...>`を、`$F`にTypeCtorTrait constraintがあるsignature型位置だけで受理する。
- direct TypeCtorTrait slotをpositionごとの独立witnessとして扱う経路を廃止し、TypeCtorTraitFamily単位の
  carrier substitutionへ置換する。
- user function、Trait helper、builtinのsignatureを同じwell-formedness、型推論、trait obligation routeへ載せる。
- Forgeへは具体化済みcall/dispatchだけを渡し、ReturnTypeArgument専用metadataを新設しない。
- 上記移行が完了する前にdo構文intrinsicを追加しない。

移行中に互換用の二重field、旧用語alias、旧経路fallbackを追加してはならない。serialized cacheやfixture更新が
必要な場合も一括更新し、旧形式を読み戻すcompatibility layerは設けない。

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

- Spire は where RHS を `Type<...>`、bare `Trait`、`TypeCtorTrait.$Slot` に分類して保持する。通常 trait RHS の argument は保持しない。
- SpireからScarまで、`def f::<...>`を`return_type_arguments`として独立に保持する。value parameterやTrait-head type parameterの配列へ混ぜない。
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

CallableSignature {
  return_type_arguments: Vec<CanonicalTy>,
  value_parameters: Vec<CanonicalTy>,
  return_type: CanonicalTy,
  where_constraints: CanonicalConstraintSet,
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

`From` / `TryFrom` の排他は同じ canonical pattern unifier で、obligation subjectと変換元Trait argumentの
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

TypeCtorTrait application（例: `Applicative<$A>`）は通常関数またはTrait method signatureのdirect
parameter / returnだけで受理する。nested type、field、local annotation、closure signatureでは拒否する。
direct Trait syntaxは名前付きconstructor variableとbare capabilityへ正規化し、同じTypeCtorTraitFamilyに属する
全parameterとreturnを一つの具象carrierへunifyする。異なるfamilyだけが同じ関数定義内で異なるcarrierを
取れる。container内部を観測しないdirect slotは型引数を省略できるが、carrier同一性の検査には参加する。
returnだけに現れるcarrierはReturnTypeArgumentから導入し、本体検査の終了時に単一の具象constructorへ
確定しなければならない。

bare capability は expression が trait call、operator lowering、または generic call の proof 引渡しで消費した
ときだけ full obligation を発行する。full obligation は `(TraitRef, obligation subject)` を構造化して保持する。
body / impl block の scope 終了時に未消費の bare capability は `UnusedTraitConstraint` TypeError とする。
trait parent、shape、slot map はこの判定から除外する。

### 4.1 ReturnTypeArgumentのwell-formedness

通常関数とTrait methodの`Self`、`$...`型変数、direct TypeCtorTrait carrierは、型入力を導入するチャネルを
一つだけ持つ。Scarはcallableをpredeclareする前にReturnTypeArgument（`def method::<...>`）、value parameter、
戻り値を再帰走査して次を`TypeError`として検査する。

- 同じ型変数がReturnTypeArgumentとvalue parameterの両方に現れてはならない。
  `Eq::eq::<Self>(self: Self, ...)`は不正であり、`Eq::eq(self: Self, ...)`と書く。
- ReturnTypeArgumentに現れる型変数は戻り値にも現れなければならない。値引数から導入できないresult/targetを
  call-site、期待型、型注釈から決める入力としてobservableに保つためである。
- 戻り値だけに現れる型変数はReturnTypeArgumentで宣言しなければならない。宣言されていないreturn-only型変数を
  暗黙にgeneralizeしてはならない。
- value parameterで導入した型変数は戻り値に現れてもよいが、現れる必要はない。
- ReturnTypeArgumentにTypeCtorTrait名を直接書いた場合はfreshなconstructor variableと単一のbare capabilityへ
  正規化する。複数constraintが必要なら名前付き型変数と関数`where`を使う。

```surtr
deftrait Show {
  def to_string(self: Self) -> String
}

deftrait TryFrom<$To> {
  def try_from::<$To>(self: Self) -> Result<$To, Error>
}

def guard::<Alternative>(cond: Boolean) -> Alternative<Unit>
```

trait impl methodはTrait headとimpl targetを代入・`Self` applicationを展開した後のReturnTypeArgument、value
parameter、戻り値を一つの順序付き型リストとしてalpha-normalizeして比較する。ReturnTypeArgumentの個数、順序、
型構造、または他のsignature位置との同一変数関係がTrait contractと異なればincompatible signatureとする。
候補検査、method contract具体化、dispatch、callable instantiationは同じ構造的型リストとsubstitutionを使い、表示文字列、
nominal owner名、値引数だけから別々に導出してはならない。deriveが生成する`Show` / `Eq` / `Compare` methodも、
receiver/value parameterが`Self`を導入するためReturnTypeArgumentを生成しない。

structなどの内部型を使うimplでは、宣言側型変数と呼び出し側具象型を同じ順序付き型リストで照合する。

```surtr
defstruct Box<$T> {
  val: $T
}
```

`Box<$T>`に対するmethodを`Box<V>`で呼ぶ場合、宣言側`[Box<$T>, $T, ...]`と具象側`[Box<V>, V, ...]`を
再帰unifyして`$T := V`を得る。この置換をreceiver、Trait arguments、ReturnTypeArgument、value parameter、
期待戻り値へ一貫して適用する。型変数を型リストから落としたり、`Box`というowner名だけでdispatchしてはならない。

well-formedness診断は少なくとも次のmessage、label、helpを構築できなければならない。

- ReturnTypeArgumentとvalue parameterで同じ型変数を導入した場合、両方のsource spanを示し、
  「value parameterから導入済みなのでReturnTypeArgumentから取り除く」と案内する。
- 戻り値だけに現れる型変数が未宣言の場合、戻り値の出現をlabelし、
  `def name::<$T>(...)`の形でReturnTypeArgumentへ追加するhelpを出す。
- `where Applicative: Add`のようにTrait名をsubjectにした場合、そのTrait名をlabelし、
  `where $F: Applicative + Add`のように名前付き型変数を通じて複数constraintを宣言するhelpを出す。

### 4.2 `Default` derive の生成境界

`Default` trait の標準契約は次で固定する。

```surtr
deftrait Default {
  def default::<Self>() -> Self
}
```

`Self`はruntime value parameterではなく、ReturnTypeArgumentから導入するdispatch targetである。したがって
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
- callable instantiation clone
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
callable instantiation後、Forge前に全`TypedNode`を監査し、`TraitDispatch::Pending`があればScarの
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
consumption、ReturnTypeArgumentの導入規則、same-family carrier一致、different-family carrier分離、non-capturing slot、
parent `Self` assumption、qualified diamond、call-site scheme と expected propagation を unit と Rune fixture の両方で保持する。

workspace は既定 nextest profile で 2 回連続成功させる。timeout 引上げで性能問題を隠さず、新しい prelude-heavy
fixture は既存 bucket に集約する。

V1 の非目標は runtime trait object/dictionary dispatch、specialization/priority dispatch、negative trait bound、
closed-world proof、coinductive solving、任意 local value の全面的 let-polymorphism、effectful callable の自動
generalization である。
