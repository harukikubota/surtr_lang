# ReturnTypeArgument 構文・型検査仕様

## 1. 状態と正本関係

本書は `def f::<TYPE>(args) -> Return` と `f::<TYPE>(args)` における
ReturnTypeArgument（戻り値型引数）の surface syntax、well-formedness、型推論、診断、
実装移行を定める実装入力である。

用語と Trait 全体の規則は [`../docs/dev/Trait_system_spec.md`](../docs/dev/Trait_system_spec.md)、
診断の `message` / `labels` / `notes` / `help` 分類は
[`../docs/dev/diagnostics.md`](../docs/dev/diagnostics.md) を正本とする。
両文書と衝突する旧ドラフトの記述は採用しない。

## 2. 目的

ReturnTypeArgument は、値引数の型から取得できず、戻り値に現れる型入力を宣言する。
通常の generic parameter list、Trait argument、impl target、constructor slot とは別の入力チャネルである。

次を同じ規則へ載せる。

- 通常関数と private 関数
- Trait method と default method
- inherent impl method と Trait impl method
- 非 intrinsic builtin
- qualified / imported Trait helper
- callable capture

`do` 構文intrinsic、Trait定義の型形状指定、TypeCtorTrait implのslot mappingは本書の対象外とする。

## 3. 構文

### 3.1 定義側

```text
ReturnTypeArguments := "::" "<" ReturnTypeArgument ("," ReturnTypeArgument)* ">"

CallableDefinition :=
  "def" Name ReturnTypeArguments? "(" ValueParameters? ")" "->" ReturnType WhereClause? Body?
```

```surtr
def make::<$A>() -> $A

def try_from::<$To>(value: $From) -> Result<$To, Error>
where
  $From: TryFrom

def guard::<Alternative>(condition: Boolean) -> Alternative<Unit>
```

定義側の抽象ReturnTypeArgumentは、一項目で一つの型入力を導入する。通常関数、Trait method、
default method、inherent impl methodでは次だけを宣言項目として受理する。

- `$A`のような名前付き型変数
- `Self`（Traitまたはimpl contextだけ）
- `Alternative`のようなdirect TypeCtorTrait名

Trait impl method以外の定義側ReturnTypeArgumentへ、`Int`、`List<$T>`、`Option<Int>`などの具象型または
具象TypeConstructorを含む型式を書いてはならない。`::<$F<$A>>`のように一項目で複数の抽象型入力を
導入することも認めない。return-onlyの`$F`と`$A`が必要なら、それぞれを宣言する。

```surtr
def make_app::<$F, $A>() -> $F<$A>
where
  $F: Applicative
```

Trait impl methodでは、Trait contractのReturnTypeArgumentをTrait arguments、impl target、`Self` applicationで
代入した結果を記述する。この位置では`Int`、`List<$T>`、`Facet<$K, $S, $A, _, _>`などの具象または
部分具体化された型式を受理し、Trait contractとの構造一致を検査する。

通常関数で`List<$T>`のような型式をReturnTypeArgumentに許可すると、call-siteが`$T`だけを選択できる一方、
戻り値のownerを`List`へ固定した限定的な型入力構文になる。この用途は名前付き型変数、通常の戻り値型、
TypeCtorTraitで表現し、ReturnTypeArgumentの追加構文としては導入しない。

### 3.2 呼び出し側

```text
ReturnTypeArgumentApply :=
  Callable "::" "<" Type ("," Type)* ">"
```

```surtr
make::<Int>()
try_from::<Int>("42")
guard::<Option>(True)
Trait::method::<Int>(value)
&Trait::method::<Int>
```

call-site ReturnTypeArgumentは、定義側の各位置を順序どおり具体化する。定義側に対応位置がないcallableへ
`::<...>`を付けてはならない。通常の型変数を任意に指定するgeneric applicationではない。

call-siteで`::<...>`を書く場合、項目数は定義側と厳密に一致させる。各項目の`_`は既存の型hole規則に従い、
その位置だけを他の制約から推論する。一部だけ、または全項目を`_`にできる。

```surtr
convert::<_, Int>(value)
convert::<_, _>(value)
```

call-siteでReturnTypeArgument全体を省略した場合は、定義側と同数の`_`を内部的に補ったものとして解決する。

```surtr
# 定義側に二つのReturnTypeArgumentがある場合、次の二つは同じ制約を生成する
convert(value)
convert::<_, _>(value)
```

末尾項目の省略など、項目数が異なるpartial listは受理しない。部分推論は項目を省略せず`_`で明示する。

### 3.3 `::<...>` と `<...>` の区別

| 位置 | 用語 | 役割 |
|---|---|---|
| `deftrait TryFrom<$To>` | Trait-head type parameter | Trait identityを構成する宣言側binder |
| `impl TryFrom<Int> for String` | Trait argument | Trait-head type parameterの具体化 |
| `def try_from::<$To>(...)` | ReturnTypeArgument | 値引数から取得できない戻り値側の型入力 |
| `try_from::<Int>(...)` | call-site ReturnTypeArgument | 定義側ReturnTypeArgumentの具体化 |
| `List<Int>` | named type argument | nominal TypeConstructorの適用 |

同じ型が複数の役割を持つことはあるが、構文上の位置と内部表現を混ぜてはならない。たとえば
`try_from::<Int>("1")`の`Int`はcall-site ReturnTypeArgumentであり、選択されるfull obligationでは
`TryFrom<Int>`のTrait argumentにもなる。

## 4. 型入力の導入規則

### 4.1 occurrence集合

callable signatureを再帰走査し、次を区別する。

```text
argument_inputs = value parameterの型に現れる型入力
return_inputs   = return typeに現れる型入力
declared_rtas   = ReturnTypeArgumentで宣言された型入力
```

「現れる」はdirect位置だけでなく、tuple、関数型、named type application、TypeCtorTrait applicationの内部を含む。
`mapper: ($A -> $B)`に現れる`$A`と`$B`は、どちらもvalue parameterから導入される。

### 4.2 基本不変条件

通常関数とTrait methodは次を満たさなければならない。

1. `argument_inputs`と`declared_rtas`は交差しない。
2. `declared_rtas`の各入力は`return_inputs`に現れる。
3. `return_inputs`のうち`argument_inputs`にない入力はReturnTypeArgumentで宣言する。
4. value parameterから導入した入力は戻り値に現れてもよいが、現れる必要はない。
5. `where`は未知の型入力を導入しない。

```surtr
# OK: $Aと$Bはmapperを含むvalue parameterから導入される
def map(
  value: $F<$A>,
  mapper: ($A -> $B)
) -> $F<$B>
where
  $F: Functor

# NG: $Fはvalue parameterから導入済み
def duplicate::<$F>(value: $F<$A>) -> $F<$A>
where
  $F: Functor

# NG: $Bは戻り値だけに現れるが宣言されていない
def missing(value: Int) -> $B

# NG: $Aは戻り値に現れない
def unused::<$A>() -> Int
```

Trait-head type parameter、`Self`、impl targetで既に名前がscopeへ入っていても、値引数から取得できず
methodの戻り値側を選択する入力ならReturnTypeArgumentに置く。

```surtr
deftrait Default {
  def default::<Self>() -> Self
}

deftrait TryFrom<$To> {
  def try_from::<$To>(self: Self) -> Result<$To, Error>
}
```

`Self`または`$To`がvalue parameterに現れる場合は、同じ入力をReturnTypeArgumentへ重ねない。

### 4.3 Trait impl method

Trait impl methodは、新しい抽象型入力を導入するのではなく、Trait contractを具体化する。
Scarは次を一つの順序付き型リストとしてalpha-normalizeする。

```text
[ReturnTypeArguments..., value parameter types..., return type]
```

Trait headとimpl targetを代入し、`Self` applicationを展開した後、次を構造比較する。

- ReturnTypeArgumentの個数と順序
- 各項目の型構造
- value parameterとの型変数同一性
- 戻り値との型変数同一性
- `where` constraint集合

型変数のsource名は一致条件にしない。表示文字列、owner名だけの比較、値引数だけからの置換導出を使ってはならない。

## 5. TypeCtorTraitとの統合

### 5.1 direct TypeCtorTrait

一つのconstructor variableに必要な明示constraintがTypeCtorTrait一つだけなら、parameter、return、
ReturnTypeArgumentへTrait名を直接書ける。

```surtr
def guard::<Alternative>(condition: Boolean) -> Alternative<Unit>
```

概念上、freshな名前付きconstructor variableと関数`where`へ正規化する。

```surtr
def guard::<$F>(condition: Boolean) -> $F<Unit>
where
  $F: Alternative
```

direct return TypeCtorTraitは、call-site制約から具象carrierを選ぶ`impl Trait`相当であり、runtime trait object、
dictionary、動的dispatchを生成しない。通常Trait名をparameter / return型として使うことはできない。

### 5.2 名前付きconstructor variable

複数constraintが必要なら名前付きconstructor variableを使い、関数定義の`where`へすべて列挙する。

```surtr
def stop::<$F>() -> $F<Unit>
where
  $F: Monad + Alternative
```

ReturnTypeArgument位置へconstraintを書いてはならない。

```surtr
# NG
def stop::<$F: Monad>() -> $F<Unit>
```

`$F<$A>`のようなconstructor variable applicationは、同じ関数定義の`where`で`$F`にTypeCtorTrait constraintが
ある場合だけ受理する。通常型変数を任意に高階適用する構文にはしない。

```surtr
# OK
def keep(value: $F<$A>) -> $F<$A>
where
  $F: Monad

# NG: $FをTypeConstructorとして使える根拠がない
def invalid(value: $F<$A>) -> $F<$A> {
  value
}
```

`where Applicative: Add`のようにTrait名をconstraint subjectにしてはならない。名前付き型変数を導入する。

```surtr
def make::<$F>() -> $F<Unit>
where
  $F: Applicative + Add
```

### 5.3 capturing / non-capturing

TypeCtorTraitがcontainer内部型を観測する場合は型引数を記述する。

```surtr
def keep(value: Monad<$A>) -> Monad<$A>
def transform(value: Functor<$A>, mapper: ($A -> $B)) -> Functor<$B>
```

内部型を観測せず、他の型との関係も表現しない場合は型引数を省略できる。

```surtr
def discard(value: Monad) -> Unit {
  ()
}
```

non-capturing位置は型変数を導入しない。mapped payload同士を暗黙に同一型とはみなさないが、
TypeCtorTraitFamilyのcarrier同一性検査には参加する。

### 5.4 TypeCtorTraitFamily

同じ関数定義内で同じTypeCtorTraitFamilyに属する全occurrenceは、一つのcarrierを共有する。
共有対象には次を含む。

- direct value parameter
- direct return
- direct TypeCtorTraitのReturnTypeArgument
- 同じconstructor variableの`$F<$A>` application
- callable schemeがbody内へ渡すproof requirement

carrier identityはnominal owner、constructor head、mapped slot以外に固定される型引数を含む。
mapped payloadは通常の型変数規則で比較し、同じ識別子が再出現した場合だけ同一型を要求する。

```surtr
def same(left: Functor, right: Monad) -> Unit {
  ()
}
```

`Monad`から`Functor`へ継承pathがあるなら、`left`と`right`へ異なるcarrierを渡せない。
ただしvalue parameter位置のcapability viewは表記されたTraitに制限する。

```surtr
def capabilities(left: Functor, right: Monad) -> Unit {
  bind(left, mapper)  # NG: leftのviewはFunctor
  bind(right, mapper) # OK
}
```

異なるTypeCtorTraitFamilyなら異なるcarrierを渡せる。同じcarrierを渡すことも禁止しない。

## 6. call-site型推論

### 6.1 制約源

call checkerは次を検査順に依存しないconstraint setとして収集する。

- call-site ReturnTypeArgument
- value argumentsの具象型
- call expressionへ与えられたexpected return type
- 既知のclosure parameter / return shape
- callable signatureのtrait obligations

呼び出し能力やimpl登録順をfallbackの型選択に使ってはならない。

### 6.2 省略と明示

定義側ReturnTypeArgumentは必須の宣言である。call-site指定は、他の制約から一意に推論できるなら省略できる。
省略時は全項目を`_`にした場合と同じconstraintを生成する。明示する場合は項目数を厳密一致させ、推論へ
残す位置を`_`で表す。

```surtr
value: Option<Unit> = guard(True)
value = guard::<Option>(True)
```

TryFromのように値引数から目的型を取得できず、expected typeもない場合はcall-siteで明示する。

```surtr
number = try_from::<Int>("42")
```

明示指定もexpected typeもなく、必要な入力を一意に決められない場合はambiguity errorにする。

### 6.3 TypeConstructor headの明示

TypeCtorTrait carrierをcall-siteで明示する場合は、具象TypeConstructor headをReturnTypeArgumentへ与える。

```surtr
guard::<Option>(True)
guard::<Either>(True)
```

mapped slot以外の固定型引数はvalue arguments、expected type、他のsignature制約から取得する。
たとえば`Either<$L, $R>`の`$R`がmapped slotなら、`guard::<Either>`は`$L`を指定しない。
固定型引数が最後まで未確定なら、headが明示されていてもambiguity errorにする。

### 6.4 解決結果とboundary

ReturnTypeArgumentを含むcallの解決結果は次の三状態を持つ。

```text
Solved   : 全型入力、carrier、dispatchが確定
Deferred : 他のinference variableまたはexpected typeを待つ
Failed   : 制約が矛盾、能力不足、または型不一致
```

definition boundary、callable instantiation boundary、program boundaryでは`Deferred`を監査する。
入力が尽きても未確定ならambiguity errorとし、先頭implやbuiltin固有の既定型で成功させない。

## 7. callable種別間の共通化

通常関数、Trait method/helper、非 intrinsic builtinは、ReturnTypeArgument、value parameters、return type、
`where` constraintsを同じ`CallableSignature`へ正規化する。

```text
CallableSignature {
  return_type_arguments: Vec<CanonicalTy>,
  value_parameters: Vec<CanonicalTy>,
  return_type: CanonicalTy,
  where_constraints: CanonicalConstraintSet,
}
```

共通化後は次を禁止する。

- callable名やbuiltin名によるReturnTypeArgument推論分岐
- `pure`、`empty`、`return`などhelper名によるexpected type注入
- `Option`、`List`、`Result`、`Either`などdata type名によるcarrier選択
- builtin登録順、impl登録順によるfallback
- rendererによるmessage文字列解析からのfailure reason復元

runtime builtin ID、opcode、effect、値域検査は型推論後の別責任として保持できる。
`@intrinsic`は通常callable signatureではないため対象外とする。

## 8. 診断契約

### 8.1 構造化failure

最低限、次を独立したfailure reasonとして保持する。最終的なRust enum名は既存`TypeError`構造へ合わせてよいが、
自然言語からreasonを復元してはならない。

| failure | 条件 | primary span |
|---|---|---|
| `DuplicateReturnTypeArgumentInput` | 同じ型入力がvalue parameterとReturnTypeArgumentの両方に現れる | ReturnTypeArgument側 |
| `MissingReturnTypeArgument` | 戻り値だけの型入力が宣言されていない | 戻り値の最初の該当箇所 |
| `UnusedReturnTypeArgument` | 宣言項目が戻り値に現れない | ReturnTypeArgument項目 |
| `ConcreteReturnTypeArgumentInDefinition` | Trait impl method以外の定義側に具象型または複合型式を書いた | ReturnTypeArgument項目 |
| `InlineReturnTypeArgumentConstraint` | `::<$F: Monad>`と書いた | inline constraint |
| `InvalidTraitConstraintSubject` | `where Applicative: Add`のようにTrait名をsubjectにした | subject |
| `MissingTypeConstructorConstraint` | TypeCtorTrait constraintなしで`$F<$A>`を使った | `$F<$A>` |
| `ReturnTypeArgumentArityMismatch` | call-siteまたはimpl methodの項目数が契約と違う | `::<...>`全体 |
| `ReturnTypeArgumentMismatch` | 明示項目がvalue argumentまたはexpected typeと矛盾する | 明示項目 |
| `AmbiguousReturnTypeArgument` | boundaryまで一意に決まらない | callまたはcapture |
| `TypeConstructorFamilyMismatch` | 同じfamilyへ異なるcarrierが与えられた | 後から衝突したorigin |

### 8.2 message / labels / notes / help

#### value parameterとの二重導入

```text
message: type input `$F` is introduced more than once
label 1: `$F` is introduced by this value parameter
label 2: `$F` is declared again as a return type argument
help: remove `$F` from the return type arguments
```

二つのsource spanをAriadne labelとして表示する。primaryはReturnTypeArgument側、relatedはvalue parameter側とする。

#### return-only型入力の宣言漏れ

```text
message: return-only type input `$F` is not declared
label: `$F` appears only in this return type
note: return-only type inputs must be declared as return type arguments
help: declare it as `def name::<$F>(...)`
```

#### 定義側の具象型・複合型式

```text
message: return type arguments must declare abstract type inputs
label: `List<$T>` fixes a concrete type constructor in this declaration
note: concrete return type arguments are only used when a Trait impl method substitutes its contract
help: declare `$T` directly and keep `List<$T>` in the return type
```

たとえば`def zeros::<List<$T>>() -> List<$T>`は拒否し、必要なら
`def zeros::<$T>() -> List<$T>`と書く。

#### Trait名をconstraint subjectにした場合

```text
message: trait `Applicative` cannot be used as a constraint subject
label: this is a trait name, not a named type variable
help: introduce a type variable and write `where $F: Applicative + Add`
```

#### carrier不一致

二つの値または値と期待戻り値が衝突した場合、両originへ完全な具象型を表示する。
call-site ReturnTypeArgumentのlabelは型名だけでよい。

```text
message: values require different type-constructor carriers
label 1: left value has `Either<String, Int>`
label 2: right value has `Either<Error, Boolean>`
note: positions in the same TypeCtorTraitFamily must use one carrier
```

JSON利用者が必要とする`return_type_argument_ordinal`、`left_type`、`right_type`、`left_origin`、
`right_origin`、`required_trait`はtyped fieldとして追加し、message解析に依存させない。

## 9. phase ownershipと内部名

### 9.1 Spire

- すべてのcallable定義で定義側`::<...>`を同じparser routeから構築する。
- call-site `::<...>`を`ReturnTypeArgumentApply`として保持する。
- inline constraint、空list、重複applyなどidentity不要の違反をparse errorにする。
- abstract declaration項目とimpl substitution項目をcontextに応じて検査する。

### 9.2 Sigil

- ReturnTypeArgument内のTrait、type、`Self`をcanonical identityへ解決する。
- 定義側とcall-site側の順序、source span、originを保持する。
- Trait helper aliasを解決してもReturnTypeArgumentを通常のvalue argumentへ変換しない。

### 9.3 Scar

- value parameter、ReturnTypeArgument、return typeの導入関係を再帰検査する。
- direct TypeCtorTraitと名前付きconstructor variableを同じconstraint表現へ正規化する。
- call-site ReturnTypeArgument、value arguments、expected returnを同じconstraint setで解く。
- Trait contractとimpl methodを順序付き型リストで構造比較する。
- boundaryまで残った未解決入力とpending dispatchを拒否する。

### 9.4 Forge / Eldr

- 具体化済みcallとdispatchだけを受け取る。
- ReturnTypeArgument専用runtime metadata、dictionary、lookupを追加しない。

### 9.5 正規内部名

| concept | 正規名 |
|---|---|
| 一項目 | `ReturnTypeArgument` / `return_type_argument` |
| 項目列 | `return_type_arguments` |
| call-site node | `ReturnTypeArgumentApply` |
| call-site項目列 | `call_site_return_type_arguments` |
| value parameter | `ValueParameter` / `value_parameter` |
| Resolved value parameter | `ResolvedValueParameter` |
| Typed value parameter | `TypedValueParameter` |

旧内部名、generic type applicationに見える別名、互換用の二重fieldを残してはならない。
serialized cache、semantic metadata、diagnostic、test、rustdoc、site docsも一括更新し、旧形式を読み戻す
compatibility layerは設けない。

## 10. テストマトリクス

### 10.1 definition well-formedness

- value parameterから導入される型入力をReturnTypeArgumentへ置かない成功例
- value parameterとReturnTypeArgumentの二重導入
- return-only型変数の宣言あり／なし
- ReturnTypeArgumentが戻り値に現れない失敗
- Trait impl method以外の定義側にある具象型・複合型式の拒否
- Trait impl methodで代入済み具象型式を受理し、contractと構造比較すること
- closure型やnested type内に現れるvalue-parameter由来型変数の再帰検出
- `Self`とTrait-head type parameterのreturn-only規則
- abstract Trait methodと具象Trait impl methodの型リスト一致

### 10.2 TypeCtorTrait

- direct TypeCtorTraitのparameter / return / ReturnTypeArgument
- direct syntaxと名前付き`$F` + `where`の同値性
- 複数constraintで名前付き`$F`を要求
- inline ReturnTypeArgument constraintの拒否
- TypeCtorTrait constraintなしの`$F<$A>`拒否
- capturing / non-capturing slot
- same-family carrier一致／不一致
- different-family carrier分離
- mapped payloadと固定型引数の区別

### 10.3 call-site

- 明示ReturnTypeArgumentだけで決定
- value argumentだけで決定
- expected returnだけで決定
- `_`を含む明示listの推論
- 全項目を`_`にした明示listとcall-site list省略の同値性
- 三制約源が一致する成功
- 任意の二制約源が衝突する失敗
- 項目数不足／過剰
- 定義側にReturnTypeArgumentがないcallableへの適用拒否
- callable captureへの適用
- headは明示されたが固定型引数が未確定のambiguity
- impl登録順を変えても結果と診断が同じ

### 10.4 callable parity

- user function、Trait helper、非 intrinsic builtinが同じ成功結果を持つ
- 同じ入力に対して同じ構造化failure reasonを持つ
- builtin IDとconcrete dispatchが型推論後も維持される
- intrinsicが共通callable routeへ入らない

### 10.5 diagnostics

- 二重導入の二地点label
- 宣言漏れのreturn spanとrewrite help
- Trait subject誤用の名前付き型変数へのrewrite help
- carrier不一致の二地点と完全な型
- AriadneとJSONが同じtyped failureを参照する
- source generic名を表示し、内部inference variable番号を表示しない

## 11. 対象外

- Trait定義`where`の`Self: Type<...>`型形状指定
- TypeCtorTrait impl `where`の`Trait.$Slot` mapping
- constructor slotの決定規則
- Trait impl coherenceとparent coverage
- expected typeを各式shapeへ伝播する詳細
- `if`、`match`など複数block固有のheadline
- `do`構文intrinsic
- runtime trait object、dictionary、dynamic dispatch
- arbitrary higher-kinded type variableと一般kind system

本書の実装と検証を完了してから`do`構文intrinsicの追加へ進む。

## 12. 受け入れ基準

1. 定義側とcall-site側の`::<...>`がReturnTypeArgumentとして一意に説明・保持される。
2. Trait impl method以外の定義側ReturnTypeArgumentは抽象入力だけを宣言し、具象型・複合型式を拒否する。
3. call-site指定は厳密な項目数を要求し、`_`による部分・全体推論とlist全体の省略を同じ制約へ正規化する。
4. value parameter由来とreturn-onlyの型入力が構造的に分類される。
5. 二重導入、宣言漏れ、未使用ReturnTypeArgumentがsource span付きで拒否される。
6. direct TypeCtorTraitが名前付きconstructor variableと単一constraintへ正規化される。
7. 複数constraintは名前付き型変数と関数`where`だけで表現される。
8. 同じTypeCtorTraitFamilyは一つのcarrierを共有し、capability viewは位置ごとに保持される。
9. call-site指定は任意で、他の制約から得られない場合だけ明示を要求する。
10. 通常関数、Trait helper、非 intrinsic builtinが同じ検査・推論経路を使う。
11. Trait contractとimpl methodがReturnTypeArgumentを含む順序付き型リストで一致検査される。
12. 旧内部名と旧互換経路が残らず、Forgeへ抽象ReturnTypeArgumentを渡さない。
13. Ariadneは関係する二地点を示し、JSONは必要な情報をtyped fieldで保持する。
14. 本書の修正フェーズが完了するまで`do`構文を追加しない。
