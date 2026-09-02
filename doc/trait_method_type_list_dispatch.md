# Trait method 型リスト照合・dispatch仕様

## 1. 状態と正本関係

本書はTrait impl候補の照合、Trait method contractの具体化、static dispatch、callable instantiationを、
型変数を含む順序付き構造型リストで統一するための実装入力である。

Trait identity、coherence、applicability、parent coverageの全体契約は
[`../docs/dev/Trait_system_spec.md`](../docs/dev/Trait_system_spec.md)、ReturnTypeArgumentの構文と導入規則は
[`return_type_argument_rules.md`](return_type_argument_rules.md)、診断構造は
[`../docs/dev/diagnostics.md`](../docs/dev/diagnostics.md)を正本とする。

本書は型形状指定、constructor slot mapping、coherenceの意味を変更しない。それらが生成した構造metadataを
候補照合とdispatchで一貫して利用する。

## 2. 目的

Trait methodの検査とdispatchは、次のすべてを同じ構造的substitutionから導出しなければならない。

- requested Trait identityとTrait arguments
- obligation subject / impl target
- ReturnTypeArguments
- value parameter types / value argument types
- return type / expected return type
- impl `where` obligations
- method body内の型変数
- concrete dispatch target
- callable instantiation key

型名の表示文字列、nominal owner名だけのkey、値引数だけのmapping、impl登録順を正しさの根拠にしてはならない。

## 3. 動機

```surtr
defstruct Box<$T> {
  val: $T
}
```

`Box<$T>`を対象に宣言されたTrait methodを`Box<V>`へ適用するとき、宣言側の`$T`と呼び出し側の`V`を
同時に扱う必要がある。`Box`というowner名だけの比較では、field型、戻り値型、method bodyの`$T`を`V`へ
置換できない。逆に呼び出し側の`V`だけを保持すると、Trait contract内で同じ`$T`が再出現する関係を失う。

必要な照合は次である。

```text
declaration: Box<$T>, $T, ($T -> String), Result<$T, Error>
invocation:  Box<V>,  V,  (V  -> String), Result<V,  Error>

substitution: $T := V
```

`V`は具象型でも、call-site inference variableでも、外側のrigid genericでもよい。照合は型名の文字列ではなく、
`Ty` / canonical typeの再帰構造を使う。

## 4. 正規用語

| 用語 | 定義 |
|---|---|
| declaration type list | Trait / impl宣言から構築したrole付き順序型リスト |
| invocation type list | obligation、call-site ReturnTypeArguments、value arguments、expected returnから構築した対応型リスト |
| type-list role | `TraitArgument`、`ImplTarget`、`ReturnTypeArgument`、`ValueParameter`、`ReturnType`の区分 |
| structural type path | list roleとordinal、型内部の引数位置を結ぶ不一致箇所。診断生成に使う |
| impl pattern namespace | impl headごとにfresh化されるpattern variableの名前空間 |
| method namespace | Trait method contractまたはimpl methodごとにfresh化される型変数名前空間 |
| invocation namespace | call-site inference variable、rigid generic、具象型を保持する名前空間 |
| method instantiation | 一つのimpl候補と一つのmethod callを同じsubstitutionで具体化した結果 |
| contract identity | `(TraitId, method_name)` |
| implementation identity | contract identity、canonical impl head、method declaration identityの組 |

callable instantiationはgeneric callableを具象型でcloneする処理であり、V1非対応のimpl specializationとは別概念である。

## 5. 構造型リスト

### 5.1 role付きentry

型リストは単なる`Vec<Ty>`へ無造作にflattenせず、roleとordinalを保持する。

```text
TypeListEntry {
  role: TypeListRole,
  ordinal: u32,
  ty: CanonicalTy,
  origin: TypeOrigin,
}

TypeListRole =
  | TraitArgument
  | ImplTarget
  | ReturnTypeArgument
  | ValueParameter
  | ReturnType
```

`CanonicalTy`はnominal application、tuple、function、list、lazy、Result、Facet、`Self` application、
constructor application、rigid / inference / pattern variableを再帰的に保持する。nested型をentry列へflattenして
構造を失ってはならない。

TypeCtorTrait carrierは単なるowner名や通常のzero-arity型へ潰さず、`CanonicalTy`から参照できる次の構造を持つ。

```text
CanonicalConstructorCarrier {
  family_id: TypeCtorTraitFamilyId,
  constructor: ConstructorHead,
  arity: u32,
  mapped_slots: Vec<CanonicalMappedSlot>,
  captured_arguments: Vec<CanonicalCapturedArgument>,
}

CanonicalMappedSlot {
  slot_id: ConstructorSlotId,
  position: u32,
}

CanonicalCapturedArgument {
  position: u32,
  ty: CanonicalTy,
}
```

`constructor`は宣言側のcarrier variable、call-site inference variable、具象`TypeCtorId`を区別する。
`family_id`はTypeCtorTrait継承graphの連結成分を表すcanonical identityである。family rootが複数ある場合も、
継承で連結されるなら同じ`family_id`になる。
各signature位置が要求する具体的なTrait能力はcarrier identityへ混ぜず、同じcarrier variableに対するobligationとして保持する。

### 5.2 impl head list

Trait impl宣言から次を構築する。

```text
ImplHeadTypeList = [
  TraitArgument(0),
  ...,
  TraitArgument(n - 1),
  ImplTarget(0),
]
```

call-site obligationから対応するrequested listを構築する。

```text
RequestedHeadTypeList = [
  requested_trait_argument(0),
  ...,
  requested_trait_argument(n - 1),
  obligation_subject,
]
```

二つのlistはrole、ordinal、個数を一致させ、一つのfresh substitution environmentで全entryを再帰unifyする。
Trait argumentsとimpl targetを別々のmappingで照合してはならない。

### 5.3 method signature list

Trait method contractとTrait impl methodは、次の同じ形へ正規化する。

```text
MethodSignatureTypeList = [
  ReturnTypeArgument(0),
  ...,
  ReturnTypeArgument(r - 1),
  ValueParameter(0),
  ...,
  ValueParameter(p - 1),
  ReturnType(0),
]
```

ReturnTypeArgumentsをvalue parametersへ混ぜず、return-only型入力をlistから落とさない。`where` constraintsは
型listとは別のcanonical setとして保持するが、同じ型変数namespaceとsubstitutionを参照する。

### 5.4 invocation list

method callから次を構築する。

```text
InvocationTypeList = [
  call-site ReturnTypeArgument(0),
  ...,
  call-site ReturnTypeArgument(r - 1),
  value argument type(0),
  ...,
  value argument type(p - 1),
  expected-or-fresh return type,
]
```

call-site ReturnTypeArgumentが省略された場合は、定義側と同数のfresh inference variableを置く。
明示list内の`_`も対応位置のfresh inference variableになる。expected returnがない場合もfresh variableを置き、
return typeを照合対象から除外しない。

明示listは定義側と項目数を厳密一致させる。末尾省略やpartial zipは認めず、推論へ残す位置を`_`で表す。

fresh variableは「call-site側の未確定入力」であり、登録済みimpl patternから既定値を得る穴ではない。値引数、明示
ReturnTypeArgument、expected return、外側signature constraintなどimpl集合と独立した制約を先に収集する。これらから
subjectまたはcarrierを決められない場合、候補が一つしかなくても`Deferred`のままにする。候補probeはpattern variableを
call-site型へbindできるが、candidate-localな具象型だけを根拠に未拘束call-site variableをbindして候補を選んではならない。

## 6. namespaceとalpha-equivalence

### 6.1 fresh namespace

次の変数ID空間を共有してはならない。

- Trait contract
- 各impl head
- 各impl method
- 各call-site
- coherenceで比較する左右のimpl

候補評価の開始時にimpl patternとmethod variablesをfresh化する。source上で同じ`$T`という名前を使っていても
別宣言なら別変数であり、`$A`と`$T`の名前が異なっていても出現構造が同じならalpha-equivalentになり得る。

### 6.2 repeated variable

一つのdeclaration list内で同じ型変数が再出現した場合、同じcanonical variableを使う。

```surtr
deftrait Same<$A> {
  def same(self: Self, left: $A, right: $A) -> $A
}
```

`left`、`right`、returnを別々にfresh化してはならない。invocation側でも三位置が一つの型へunifyされる。

### 6.3 occurs check

pattern / inference variableを型へbindするときはoccurs checkを行う。`$A := List<$A>`のような無限型を
候補成功として扱ってはならない。失敗候補のsubstitution、pending obligation、proof stateはすべてrollbackする。

## 7. Trait contractとimpl methodの一致

### 7.1 contract具体化

Trait implをpredeclareするとき、次の順序でcontract listを具体化する。

1. Trait-head type parametersをimpl headのTrait argumentsへ代入する。
2. `Self`をimpl targetへ代入する。
3. 事前検証済みconstructor slot mappingを使って`Self<$...>`を展開する。
4. ReturnTypeArguments、value parameters、return typeへ同じsubstitutionを適用する。
5. Trait method `where` constraintsを同じnamespaceでcanonicalizeする。

constructor slot mappingの妥当性判定自体は本書で変更しない。ここでは検証済みmappingを入力として使う。

### 7.2 impl method list

impl method側もReturnTypeArguments、value parameters、return typeを一つのnamespaceで解決する。
Trait impl methodのReturnTypeArgumentsは契約代入後の具象・部分具体化型式を受理するが、各entryのroleとordinalは
contract側と一致しなければならない。

### 7.3 equality

contract listとimpl method listをそれぞれalpha-normalizeし、次を検査する。

- role列の完全一致
- roleごとのarity一致
- 各型の再帰構造一致
- repeated variableの同一性
- ReturnTypeArgumentとreturn / value parameterの関係
- canonical `where` constraint集合の一致
- visibilityとmethod名など型list外のcontract属性

表示名や内部variable番号は一致条件にしない。不一致時は最初の表示順entryではなく、安定したrole / ordinal順で
structural type pathを選ぶ。

## 8. impl候補のapplicability

### 8.1 candidate discovery

候補探索用のsecondary indexは`TraitId`をkeyにしてよい。secondary indexは候補集合を狭めるためだけに使い、
applicability、優先順位、dispatch結果を決定してはならない。

次を禁止する。

- rendered Trait名の`<...>`を`split` / `contains`で解析する
- surface名の一致をcanonical identityの代わりに使う
- `(trait display name, target owner name)`だけでimplを一意に取得する
- storage iterationで最初に見つかった候補を選ぶ

### 8.2 canonical storage identity

Trait impl patternの構造identityと宣言追跡identityを分ける。

```text
CanonicalTraitImplPatternKey {
  trait_ref: CanonicalTraitRef,
  target: CanonicalTy,
}

TraitImplDeclarationKey {
  pattern: CanonicalTraitImplPatternKey,
  declaration_id: DeclarationId,
}
```

pattern keyはbinderをstable ordinalへalpha-normalizeし、source generic名、fresh variable ID、hash iteration順を含めない。
`declaration_id`はsource span、method body、診断originの追跡に使い、構造同一性の代用やoverlapするimplを共存させる
識別子にはしない。同じnominal targetでもfull patternがdisjointなら別patternになり、交差するならcoherence errorになる。

### 8.3 applicability手順

call-site固有constraintを候補集合と独立に収集した後、一候補ごとに一つのcheckpointとsubstitution environmentを作り、
次を行う。

1. impl pattern variablesをfresh化する。
2. `ImplHeadTypeList`と`RequestedHeadTypeList`を一つのenvironmentでunifyする。
3. head substitutionを保持したままTrait method contractとimpl methodを具体化する。
4. `MethodSignatureTypeList`と`InvocationTypeList`を同じenvironmentでunifyする。
5. headとmethodのsubstitutionをimpl / method `where` obligationsへ適用する。
6. proof environmentのdeclared boundsとconcrete implを使って全obligationを証明する。
7. candidate-local patternだけが未拘束call-site variableへ与えたbindをcandidate selectionには使わない。
8. `Unsatisfied`またはstructural mismatchなら候補から除外しcheckpointをrollbackする。
9. call-site側入力またはproofが未確定なら`Deferred`として待機variablesと候補を保持する。
10. 全型listと全obligationが解決した場合だけ`MethodInstantiation`を構築する。

head一致だけ、またはmethod value argumentsだけでapplicableにしてはならない。bare capabilityをarity 0のfull obligationとして
扱ってはならない。head probe後に別environmentでmethod mappingを作り直すことも禁止する。

### 8.4 結果集合

```text
CandidateResult =
  | Applicable(MethodInstantiation)
  | Deferred(DeferredCandidate)
  | Rejected(CandidateFailure)
```

未確定call-site入力のため複数候補が残る場合は`Deferred`であり、入力boundaryで`AmbiguousReturnTypeArgument`になる。
concrete inputに対して複数の`Applicable`が残る場合、宣言順やmore-specific推測で選ばずcompiler invariant違反として
停止する。通常はimpl predeclareのcoherence検査が先にoverlapを拒否する。候補内部のhead mismatchは候補除外理由であり、
単独でuser-facing errorにしない。

## 9. method instantiation

### 9.1 単一結果

applicable候補とmethod callから次を構築する。

```text
TraitMethodInstantiation {
  contract_identity,
  implementation_identity,
  requested_trait_ref,
  obligation_subject,
  declaration_type_list,
  invocation_type_list,
  substitution,
  proven_obligations,
  dispatch_target,
}
```

同じ`substitution`を次へ適用する。

- impl targetとTrait arguments
- ReturnTypeArguments
- value parameter types
- return type
- impl method body
- body内のfield / constructor / Trait obligations
- callable instantiation key

候補選択後に値引数だけから別mappingを作り直してはならない。

### 9.2 Box例

```surtr
defstruct Box<$T> {
  val: $T
}
```

宣言側と呼び出し側を次のように照合する。

```text
declaration head:   [ImplTarget = Box<$T>]
requested head:     [ImplTarget = Box<V>]
head substitution:  $T := V

declaration method: [ValueParameter = Box<$T>, ReturnType = $T]
invocation method:  [ValueParameter = Box<V>,  ReturnType = V]
```

method bodyが`self.val`を読む場合、field型の`$T`にも同じ`$T := V`を適用する。dispatch targetだけを選び、
body cloneへmappingを渡さない状態を許してはならない。

### 9.3 ReturnTypeArgumentだけから得る型

```surtr
deftrait Default {
  def default::<Self>() -> Self
}
```

runtime value argumentがないため、value argumentsだけをzipするmappingでは`Self`を取得できない。
call-site ReturnTypeArgumentまたはexpected returnを`InvocationTypeList`へ入れ、impl targetの`Self`とunifyする。

### 9.4 Trait argumentsから得る型

```surtr
deftrait TryFrom<$To> {
  def try_from::<$To>(self: Self) -> Result<$To, Error>
}
```

`try_from::<Int>(value)`では、requested `TryFrom<Int>`、ReturnTypeArgument `Int`、return `Result<Int, Error>`を
同じsubstitutionで具体化する。Trait argumentの`Int`をrendered Trait名から再解析してはならない。

## 10. dispatch targetとcallable instantiation

### 10.1 static dispatch

Trait methodのdispatch targetは、implementation identityから次のいずれかへ解決する。

```text
TraitDispatchTarget =
  | Builtin(BuiltinId)
  | UserFunction(FunctionId, FunIdx)
  | Operator(OperatorIdentity)
```

user-facing表示名はdispatch結果から生成してよいが、表示名を使って逆引きしてはならない。

### 10.2 instantiation key

generic user methodを具体化するkeyは、implementation identityと、解決済みの型入力列で構成する。

```text
CallableInstantiationKey {
  implementation_identity,
  type_arguments: Vec<CanonicalTyKey>,
}
```

型入力列は宣言で定めたstable ordinal順に並べ、`HashMap` iteration順に依存させない。少なくともimpl head variables、
method ReturnTypeArguments、method signatureからbodyへ残るrigid variablesを含む。不要なinference variable IDや
source generic名をkeyに含めない。

### 10.3 mapping completeness

callable instantiation前に、bodyへ自由出現する全bound variableがsubstitutionに含まれることを検査する。
値引数に現れない変数も、次から取得できる。

- obligation subject
- Trait arguments
- ReturnTypeArguments
- expected return
- impl targetのcaptured type parameters

不足がある場合は未具体化methodを黙って使わず、他のconstraintを待つかambiguity errorにする。

## 11. TypeCtorTrait implとの接続

TypeCtorTrait implでは、事前検証済みconstructor slot positionsとcaptured impl-target type parametersを
`ImplHeadTypeList`内のtarget構造として保持する。

```text
Either<$L, $R>
  captured: $L
  mapped:   $R -> Functor.$A
```

`Either<String, Int>`と`Either<String, Boolean>`はmapped payloadが異なる同一carrierとして扱える。
`Either<String, Int>`と`Either<Error, Boolean>`はcaptured argumentが異なる。method instantiationはhead substitutionを
return / value parameter / bodyへ適用し、各mapped slotを対応する出力slot型へ置換する。

direct TypeCtorTrait syntaxはfresh constructor variable、`family_id`、その位置が要求するcapability obligationへ
正規化する。call-site `::<Either>`は`ConstructorHead = Either`だけを明示し、mapped positionsはimpl metadataから、
captured argumentsはvalue arguments、expected return、他のsignature constraintから得る。たとえば`Either<$L, $R>`で
`$R`がmapped slotなら、`::<Either>`だけを根拠に`$L`をimpl一覧から選ばない。boundaryまで未確定なら
`AmbiguousReturnTypeArgument`にする。

同じ`family_id`の全signature位置は一つの`CanonicalConstructorCarrier`へunifyする。Functor位置とMonad位置の
capability viewが異なってもcarrierは分離しない。異なるfamilyだけが別carrierを持てる。複数constructor slotを
持つTraitも`mapped_slots`の各`slot_id` / `position`を個別に照合し、先頭または最後の一枠を暗黙選択しない。

Functor、Applicative、Monadごとの個別dispatch helperへ型照合を複製せず、同じimpl head list、method signature list、
obligation solverを使う。operator loweringもfull obligationを生成した後は同じ候補経路へ載せる。

## 12. default method、derive、builtin

### 12.1 default method

Trait default methodはcontract identityを保ったまま、選択されたimpl head substitutionで具体化する。
default body内の`Self`、Trait-head type parameters、ReturnTypeArgumentsへ同じmappingを適用する。
method originは`DefaultTraitMethod(TraitMethodDeclarationId)`とし、選択implのdeclaration identityと組み合わせて
implementation identityを作る。

### 12.2 derive

derive生成methodも通常Trait impl methodと同じtype listを生成する。receiverまたはvalue parameterから導入される
型入力をReturnTypeArgumentへ重複生成してはならない。`Default::default`の`Self`などcontract上必要な
ReturnTypeArgumentは代入後のimpl method listへ保持する。
method originはderive site、target、contract identityから作るstable `SyntheticMethodId`とする。

### 12.3 builtin

builtin Trait methodは`BUILTIN_METAS`を起点に解決した`BuiltinId`をmethod originとしてimplementation identityへ関連付ける。
名前解決後のcandidate applicability、method type list、ReturnTypeArgument、return inference、dispatch target選択、診断を
Trait名、method名、data type名で分岐してはならない。`dispatch_override`を残す場合もcanonical implementation identityから
resolved `BuiltinId`を取得するmetadata lookupに限定し、名前分岐のescape hatchにしない。

## 13. pending、rollback、cache

### 13.1 Deferred

headまたはmethod listに未確定inference variableがある場合、候補と待機variable集合を保持する。
一つのvariableがbindされても他の待機variableが残るなら、残りへre-homeする。

### 13.2 rollback

候補probeは次をcheckpointし、失敗時にすべて戻す。

- type substitutions
- pending Trait obligations
- constructor carrier bindings
- proof assumptions
- candidate-local callable instantiation state

候補Aの失敗mappingを候補Bへ漏らしてはならない。

### 13.3 cache key

applicability / proof cacheはcanonical `TraitRef`、canonical obligation subject、proof environment projectionを含む。
method instantiation cacheはimplementation identityと解決済みtype listを含む。display name、source span、
nominal owner名だけのkeyを使わない。

## 14. phase ownership

### 14.1 Spire

- Trait head、impl target、ReturnTypeArguments、value parameters、return typeの構文とsource spanを保持する。
- 型の表示文字列をstorage identityとして生成しない。

### 14.2 Sigil

- Trait、type owner、method declarationをcanonical identityへ解決する。
- Trait contract identityとimplementation identityを区別する。
- Trait arguments、impl target、method signatureの構造を失わずScarへ渡す。

### 14.3 Scar predeclare

- impl headとmethod signatureのrole付きtype listを構築する。
- 左右をfresh namespaceにしてcoherenceとcontract一致を検査する。
- canonical impl storage keyとTraitId secondary indexを構築する。
- derive / default / builtinも同じmetadataへ載せる。

### 14.4 Scar expression checking

- full `TraitObligation`からrequested head listを構築する。
- candidate head、`where` obligations、method invocation listを一つのenvironmentで解く。
- applicable method instantiationとtyped dispatchを出力する。
- TypeCtorTrait helper名ごとのcandidate scanを共通routeへ移行する。

### 14.5 Scar callable instantiation

- method instantiationが持つsubstitutionをbody cloneへそのまま適用する。
- value argumentsだけからmappingを再構築しない。
- full canonical type listからstable instantiation keyを作る。

### 14.6 Forge / Eldr

- concrete `TraitDispatchTarget`と具体化済みfunctionだけを受け取る。
- Trait argumentsの再解析、runtime candidate selection、dictionary lookupを行わない。

## 15. 現行実装からの移行対象

### 15.1 metadata

- Spire AST、Sigil IR、Scar metadata / helper、derive、診断、fixtureから旧`FunParams` / `fun_params`を完全廃止する。
  ReturnTypeArgumentは`return_type_argument(s)`、value parameterは`value_parameter(s)`で表し、旧field、型、関数、
  compatibility alias、二重保持を残さない。`doc/要件定義v9.md`などに残る旧表記も正本更新時に除去する。
- `TraitObligation`のTrait identityを表示用`String`ではなくcanonical `TraitId`として保持する。
- `receiver`をruntime receiverとobligation subjectで使い回さず、構造上は`subject`として保持する。
- `TraitImplInfo`のAST型、resolved型、表示名を正しさのkeyとして重複保持せず、canonical impl headを正本にする。
- method metadataへReturnTypeArgumentsを含むrole付きtype listを持たせる。

### 15.2 storage / index

- string tupleのimpl storage keyを`CanonicalTraitImplPatternKey`と`TraitImplDeclarationKey`へ置換する。
- `split_once('<')`、`contains('<')`、surface名fallbackによるcandidate identity判定を削除する。
- TraitId secondary indexは候補列挙だけに限定する。

### 15.3 contract comparison

- ReturnTypeArguments、value parameters、returnをまとめたalpha-normalizationをrole付きtype listへ移す。
- mismatch pathと両宣言spanを保持する。
- `where` canonicalizationがmethod type variablesと同じnamespaceを使うようにする。

### 15.4 dispatch

- rendered Trait instance名の一致比較を削除し、`TraitRef.args`を構造比較する。
- `(trait name, target owner)`のdirect lookupを正しさの経路から外す。
- operator、TypeCtorTrait、通常Trait callの候補applicabilityを共通化する。
- method名やdata type名で型mappingを分岐しない。

### 15.5 callable instantiation

- value parameter typesとvalue argumentsだけをzipするmappingを廃止する。
- obligation subject、Trait arguments、ReturnTypeArguments、expected returnを含むmethod instantiation substitutionを使う。
- bodyの全bound variablesがmapping済みかboundaryで監査する。

## 16. 診断契約

### 16.1 candidate-local failure

候補probe内部では次を構造化して保持する。これらは候補除外、関連note選択、compiler invariant監査の入力であり、
一候補のfailure kindをそのまま最終`TypeError`に昇格させない。

| failure | 条件 | primary span |
|---|---|---|
| `TraitImplHeadMismatch` | Trait argumentsまたはtargetがrequested obligationと不一致 | call-site obligation |
| `TraitImplWhereUnsatisfied` | head一致後にimpl `where`を証明できない | impl constraintまたはcall-site |
| `TraitMethodInvocationMismatch` | method signature listとinvocation listが不一致 | call-siteの該当origin |

最終user-facing failureは次を区別する。

| failure | 条件 | primary span |
|---|---|---|
| `TraitMethodTypeListArityMismatch` | roleごとのentry数がcontractと違う | impl method |
| `TraitMethodTypeListMismatch` | 対応entryの構造または変数関係が違う | impl methodの該当型 |
| `TraitMethodConstraintMismatch` | canonical method `where`集合が違う | impl method `where` |
| `NoApplicableTraitImplementation` | call-site constraint確定後も適用可能implがない | call-site |
| `AmbiguousReturnTypeArgument` | call-site入力が未確定で複数候補またはcarrierが残る | call-site |
| `UnresolvedTraitMethodInstantiation` | bodyに必要な型入力がboundaryまで未確定 | call-siteまたはmethod declaration |
| `MissingTraitDispatchTarget` | applicable implにconcrete targetがない | impl method declaration |

overlapするimpl宣言はpredeclare時のcoherence error、concrete inputで複数`Applicable`が残る状態はinternal invariantとし、
`AmbiguousReturnTypeArgument`へ変換しない。

### 16.2 incompatible signature

Trait contractとimpl methodの両方をsource labelにする。

```text
message: Trait impl method has an incompatible type signature
label 1: contract requires `Box<$T> -> $T`
label 2: implementation provides `Box<$T> -> String`
note: return type differs at `ReturnType`
help: make the impl method preserve the contract's type relationship
```

structural type pathはuser-facingにはrole名、parameter ordinal、nested type argument位置へ変換する。
内部variable IDやhash iteration順を表示しない。

### 16.3 no applicable impl

```text
message: no Trait implementation matches the requested types
label: requested `TryFrom<Int>` for `String`
note: Trait arguments and implementation target are matched together
```

候補ごとの全内部失敗を列挙せず、最も関連するhead mismatchまたはunsatisfied `where`をrelated label / noteへ載せる。

### 16.4 callable instantiation ambiguity

```text
message: Trait method type inputs could not be determined
label: this call leaves `$T` unresolved
note: `$T` is used by the selected implementation body
help: provide a return type argument or an expected return type
```

JSONで必要な`trait_id`、`trait_arguments`、`subject_type`、`method_name`、`type_list_role`、`ordinal`、
`expected_type`、`actual_type`、`impl_declaration`はtyped fieldとして保持する。

## 17. テストマトリクス

### 17.1 impl head

- 同じnominal targetで異なるdisjoint type arguments
- nested nominal pattern
- tuple / function target
- parameterized Trait argumentsとtargetの同時照合
- targetだけ一致、Trait argument不一致
- Trait argumentだけ一致、target不一致
- source generic名を変更しても同じ結果
- impl宣言順を反転しても同じ結果

### 17.2 method contract

- ReturnTypeArguments、value parameters、returnの完全一致
- ReturnTypeArgument arity / order / nested structure不一致
- value parameter arity不一致
- repeated variable関係の保持
- alpha-renamed variablesの成功
- `Self`と`Self<$A>`の展開
- canonical `where`集合の順序非依存一致

### 17.3 Box / internal type

- `Box<$T>`を`Box<Int>`へdispatchし、field / return / bodyを`Int`へ具体化
- actual側がouter rigid generic `V`
- `Box<List<$T>>`などnested mapping
- 同じownerで異なるtype argumentsのcallable instantiation key分離
- method body内だけで使うimpl target variableのmapping保持

### 17.4 ReturnTypeArgument

- value argumentに現れない`Default::default::<Self>`
- `TryFrom<$To>`のTrait argumentとReturnTypeArgumentの一致
- call-site list省略と全`_`の同値性
- expected returnだけからmethod instantiationを完成
- explicit ReturnTypeArgumentとexpected returnの衝突
- `Default::default()`でimplが一つだけでも型をimpl集合から逆決定しないこと
- `guard(True)`でcarrier implが一つだけでもcarrierをimpl集合から逆決定しないこと
- 明示listの項目不足／過剰をpartial zipせずarity errorにすること

### 17.5 TypeCtorTrait

- captured argumentとmapped payloadの分離
- `Either<String, Int>`から`Either<String, Boolean>`へのmapping
- captured argumentが異なるcarrier mismatch
- `::<Either>`だけを指定しcaptured argumentが未確定ならambiguityになること
- 同じfamily IDのFunctor位置とMonad位置が同一carrierを要求すること
- 複数rootを持つ一つのfamilyでもcarrierを分離しないこと
- 複数mapped slotをslot ID / positionごとに照合すること
- user-defined TypeCtorTrait impl
- Functor / Applicative / Monad helperが同じcandidate routeを利用
- operator lowering後のfull obligationが同じ結果を持つ

### 17.6 dispatch / instantiation

- user method、default method、derive method、builtin dispatch
- value argumentsだけでは不足し、subject / Trait argument / returnからmappingを得るケース
- bodyの全bound variablesが具体化されること
- pending candidateのre-homeとrollback
- cache keyがfull canonical type listを区別
- alpha-renamed impl patternが同じpattern keyを持ち、別宣言identityを持つこと
- builtin targetをcanonical implementation identityから解決し、名前分岐しないこと
- concrete inputで複数Applicableが残ればuser ambiguityではなくinvariant failureになること
- REPL state clone / restore後にstale `fun_idx`を使わないこと

### 17.7 diagnostics

- contract / implの二地点label
- mismatch roleとordinalの安定性
- no applicable implのfull TraitRefとsubject
- unsatisfied impl `where`のconstraint span
- ambiguityでimpl登録順を表示しないこと
- 内部variable IDとrendered storage keyを表示しないこと

## 18. 対象外

- Trait定義`where`の型形状指定の変更
- TypeCtorTrait implのconstructor slot mapping規則の変更
- coherence、applicability、parent coverageの量化変更
- impl specialization、priority dispatch、negative bound
- runtime Trait object、dictionary、dynamic dispatch
- source generic名をpublic identityにする変更
- data typeやmethod名固有のdispatch経路追加
- `do`構文intrinsic

## 19. 受け入れ基準

1. Trait impl headはTrait argumentsとtargetを一つのstructural list / environmentで照合する。
2. Trait contractとimpl methodはReturnTypeArguments、value parameters、returnを含むrole付きtype listで比較する。
3. impl / method / call-siteの型変数namespaceが分離され、repeated variable関係が保持される。
4. 候補applicability、method具体化、dispatch、body clone、callable instantiation keyが同じsubstitutionを使う。
5. obligation subject、Trait arguments、ReturnTypeArguments、expected returnから型mappingを取得できる。
6. value argumentsだけから別mappingを作り直す経路がない。
7. impl storageとcandidate identityがcanonical type structureを使い、rendered文字列を解析しない。
8. TraitId indexは候補列挙だけに使われ、登録順が結果へ影響しない。
9. TypeCtorTrait helper、operator、通常Trait callが同じapplicability routeを使う。
10. `Box<$T>`の`$T := V`がfield、return、body、dispatch targetへ一貫して適用される。
11. default、derive、builtin methodが同じtype-list契約に従う。
12. pending / rollback / cacheがfull canonical identityとproof environmentを保持する。
13. 診断がcontractとimpl、または衝突した二つのoriginを示し、内部IDや表示keyへ依存しない。
14. Forgeへpending dispatch、未具体化body、abstract impl candidateを渡さない。
15. 未拘束call-site型またはcarrierを、登録済みimplの個数や内容から逆決定しない。
16. TypeCtorTrait carrierがfamily ID、constructor head、全mapped slots、captured argumentsを構造的に保持する。
17. default、derive、builtinのmethod originがstable identityを持ち、builtin dispatchを名前分岐しない。
18. 旧`FunParams` / `fun_params`を互換表現なしで完全廃止する。
19. 本書の修正フェーズが完了するまで`do`構文intrinsicを追加しない。
