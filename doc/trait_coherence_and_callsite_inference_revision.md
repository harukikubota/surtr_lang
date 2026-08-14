# Trait coherence / call-site 型推論 改修案

## 1. 目的

本書は commit `78528ca3cc367a8515c92b42d9003e32c23c03e8` 以降の trait system 更改と generic 型推論強化を監査した結果に対する改修案である。

対象は次の 6 項目とする。

1. overlapping trait impl を宣言順に選択してしまう
2. nominal target が同じ specialization を Scar で区別できず CodegenError まで進む
3. trait impl block 内の同名 method が後勝ちになる
4. `From` / `TryFrom` の排他検査を generic の alpha-renaming で回避できる
5. local callable の型が call-site ごとに fresh にならず、行分割で型検査結果が変わる
6. 未束縛 generic 引数へ closure や literal shape を持つ式を直接渡せない

本改修では specialization と宣言順による優先順位を導入しない。impl pattern の交差は compile error とし、dispatch は常に一意にする。

本書と追従更新した `doc/要件定義v9.md` を目標仕様とする。実装と fixture が完了するまでは、本文中の「許可」「拒否」は現行挙動の説明ではなく改修後の contract である。

### 1.1 現行実装の主な到達点

| 問題 | 現行の主経路 | 影響 |
|---|---|---|
| generic impl の先勝ち | `crates/scar/src/checker/expr.rs` の candidate scan が最初の match を返す | 宣言順で dispatch 結果が変わる |
| textual duplicate 判定 | `crates/sigil/src/resolver/declarations.rs` / `resolver/expr.rs` が AST 文字列表現に依存する | alpha-renaming と nested specialization を同一視できない |
| nominal key への縮約 | `crates/scar/src/checker/predeclare.rs` と `checker/mod.rs` が target の外側 name を中心に登録する | disjoint/overlap の区別が Codegen まで失われる |
| Codegen での衝突 | `crates/forge/src/codegen.rs` の function index 生成で重複が顕在化する | user の impl 重複が `CodegenError` になる |
| impl method 後勝ち | `crates/scar/src/checker/predeclare.rs` が method name-keyed map へ上書き insert する | 先の method body が無言で消える |
| where obligation 未検査 | `crates/scar/src/checker/expr.rs` / `predeclare.rs` の candidate 判定が target/trait arguments だけを見る | bound を満たさない型にも impl が適用される |
| local callable の擬似多相 | `crates/scar/src/checker/expr.rs` / `checker/mod.rs` が statement ごとに substitution を clear する一方、local `Ty::Func` を instantiate しない | 改行と式 grouping で成否が変わる |
| expected shape の断絶 | `crates/scar/src/checker/expr.rs` / `checker/matching.rs` が list/tuple/branch/arm を bottom-up に検査する | 外側注釈が内側の helper/literal 推論に届かない |

調査時点の実装位置は変更で移動し得るため、行番号ではなく責務と file を追跡単位にする。

## 2. 用語

- **impl pattern**: trait head の型引数列と impl target 型の組
- **alpha-equivalent**: generic 変数名だけが異なり、変数の同一性と出現位置が等しいこと
- **overlap**: 2 つの impl pattern を同時に成立させる型代入が 1 つ以上存在すること
- **call-site instantiation**: callable の型 scheme を呼び出しごとの fresh inference variable へ置換すること
- **synthesis**: expected type を必要とせず、式自身の shape から型を得ること
- **checking**: expected type を式の内部へ渡して型を検査すること

## 3. 問題点と確定仕様

### 3.1 overlapping impl が宣言順で選ばれる

現在の generic impl dispatch は登録順に候補を走査し、最初に一致した候補を採用する。このため次のプログラムは宣言順で実行結果が変わる。

```surtr
deftrait Mark<$T> {
  def mark::<$T>(self: Self) -> String
}

impl Mark<$A> for String {
  def mark::<$A>(self: String) -> String { "any" }
}

impl Mark<List<$A>> for String {
  def mark::<List<$A>>(self: String) -> String { "list" }
}
```

`$A` は任意の well-formed type と一致するため、`Mark<$A>` と `Mark<List<$B>>` は `Mark<List<Int>>` などで交差する。この 2 impl は、どちらを先に宣言しても duplicate/overlap error とする。

「`$A` が勝つ」は runtime dispatch の優先順位を意味しない。coherence 検査において `$A` が具象型または型コンストラクタ適用全体を受け入れ、その結果として交差を検出することを意味する。

### 3.2 nominal target が同じ impl が CodegenError まで進む

target の外側 nominal name だけを key にすると、`List<Int>` と `List<String>`、`List<$A>` と `List<Int>` を区別できない。一方、すべてを同じ nominal target として禁止すると、本来 disjoint な具象 specialization も失われる。

V1 では target と trait argument を構造的かつ再帰的に照合する。

```surtr
# 許可: 同時に成立する型代入がない
impl Mark<Int> for List<Int> { ... }
impl Mark<Int> for List<String> { ... }

# 拒否: List<Int> で交差する
impl Mark<Int> for List<$A> { ... }
impl Mark<Int> for List<Int> { ... }

# 拒否: Pair<String, Int> で交差する
impl Mark<Int> for Pair<$A, Int> { ... }
impl Mark<Int> for Pair<String, $B> { ... }
```

この検査は Scar の impl predeclare 中、method body の型検査と Forge の function index 生成より前に完了しなければならない。ユーザ入力由来の重複を CodegenError にしてはならない。

### 3.3 impl block 内の同名 method が後勝ちになる

impl method を name-keyed map へ無条件に insert すると、先の定義が後の定義で上書きされる。

`defmod`、inherent `impl Type`、`impl Trait for Type` の各 block は、関数名単位で一意でなければならない。`def` と `defp` は同じ callable namespace を共有し、visibility、引数型、generic 構造、返り値型の違いによる overload は認めない。

default method の補完は explicit method の一意検査後に行う。明示 method が 1 つあれば default を override し、明示 method が複数なら default の有無にかかわらず resolve error とする。

### 3.4 `From` / `TryFrom` の generic 排他を alpha-renaming で回避できる

`From<$A> for Box<$A>` と `TryFrom<$T> for Box<$T>` は変数名が異なっても同じ型集合を表す。文字列表現や AST 上の変数名で比較してはならない。

`From` / `TryFrom` の排他は通常の impl overlap 判定と同じ canonical pattern/unifier で検査する。receiver target pattern と変換元 trait argument pattern の両方が交差する場合は拒否する。

```surtr
# 拒否: A := T で同時に成立する
impl From<$A> for Box<$A> { ... }
impl TryFrom<$T> for Box<$T> { ... }

# 拒否: T := Int で同時に成立する
impl From<$A> for Box<$A> { ... }
impl TryFrom<Int> for Box<Int> { ... }

# 許可: target または変換元が構造的に交差しない
impl From<Int> for Box<Int> { ... }
impl TryFrom<String> for Box<String> { ... }
```

### 3.5 local callable が行単位で擬似的に多相化される

現在は local closure を fresh instantiate せず、statement 境界で substitution を clear するため、同じ意味の式が行分割で異なる結果になる。

```surtr
id = {|x| x}
pair: (Int, String) = (id(1), id("s"))
```

```surtr
id = {|x| x}
i: Int = id(1)
s: String = id("s")
```

両方を成功させる。callable value の binding は型 scheme として保持し、各 call-site で fresh instantiate する。

一般化する inference variable は binding environment に自由出現しないものだけとする。capture した外部値の型、外側 signature の rigid generic、明示注釈で固定された型は一般化しない。これにより異なる call-site から substitution が漏れず、capture の健全性も保つ。

statement 終端の substitution clear は多相性の意味論に使わない。式の grouping と改行は型検査結果を変えてはならない。

### 3.6 未束縛 generic へ直接 closure / literal を渡せない

generic parameter がまだ未束縛のとき、引数側の closure をその expected type に対して直ちに check すると、expected が function shape でないという誤りになる。

```surtr
box = Box({|n: Int| n + 1})
```

未束縛 inference variable は「型がない」ことを意味しない。actual expression から型を synthesis し、その結果を expected variable と unify する。

この規則は closure 専用にしない。少なくとも次の式 shape に共通適用する。

- scalar literal: `Int`, `Float`, `Boolean`, `String`, `Unit`
- closure / capture
- tuple
- non-empty list / hash literal
- struct / record / enum constructor
- function call / trait helper call / operator expression
- `if` / `match` の全 branch

空 list/hash、引数注釈を欠く closure、branch だけでは一意にならない式は expected type または別の制約を引き続き必要とする。

通常 call、constructor call、trait helper call、Apply/PipeApply、Compose/KleisliCompose は同じ argument inference entry point を使う。既に成功している Apply / Compose の直接呼び出しを個別の例外として残さない。

## 4. impl pattern の canonicalization と overlap 判定

### 4.1 canonical pattern

impl predeclare 時に次を canonical form へ変換する。

```text
ImplPattern {
  trait_id,
  trait_args: [TypePattern],
  target: TypePattern,
  where_constraints: ConstraintSet,
  source_order,
  span,
}
```

- trait/type alias と path は resolver が確定した canonical identity を使う
- generic 変数名は最初の出現順に `Var(0)`, `Var(1)`, ... へ alpha-normalize する
- 同じ generic の再出現は同じ `Var(n)` を使う
- tuple、function、nominal type application、constructor slot を構造として保持する
- `where` constraint は対象と trait identity を canonicalize し、記述順によらない set として保持する

### 4.2 overlap algorithm

同じ base trait に対する 2 impl `L`, `R` は、次を同一 substitution/occurs-check 環境で満たすとき overlap する。

1. `L.trait_args` と `R.trait_args` の arity が等しく、全要素を再帰 unification できる
2. `L.target` と `R.target` を再帰 unification できる

再帰 unification の規則は次のとおり。

- `Var(n)` は occurs check に反しない任意の well-formed type pattern と一致する
- nominal head が異なる concrete type 同士は一致しない
- nominal head と arity が同じなら各 type argument を再帰照合する
- tuple arity、function arity が異なる場合は一致しない
- rigid builtin type は同じ identity の場合だけ一致する
- 左右の generic namespace は分離し、変数名の偶然の一致を同一変数として扱わない

V1 の coherence 検査では `where` constraint を disjointness の証明に使わない。負の trait bound と closed-world の非実装証明を持たないためである。型 pattern が交差するなら、異なる `where` を持っていても overlap error とする。

候補の宣言順、module load 順、map iteration 順は結果へ影響させない。複数の衝突がある場合は canonical declaration order で最初の pair を主診断にし、相手 span を副診断にする。

### 4.3 dispatch applicability

coherence と dispatch applicability は別の検査である。

- coherence: impl pattern 同士が交差しないことを登録時に保証する
- applicability: call-site の concrete target/trait arguments が pattern と一致し、impl の `where` obligations を満たすことを確認する

`where` obligation を満たさない generic impl は dispatch candidate にしてはならない。coherence が保証されていても、where clause の検査を省略してよいことにはならない。

## 5. call-site 型推論アルゴリズム

### 5.1 callable binding の一般化

local binding の右辺が callable value のとき、型検査後に次を行う。

1. inferred type を現在の substitution で normalize する
2. binding environment に自由出現しない inference variable を列挙する
3. capture、rigid generic、明示注釈由来の変数を除外する
4. 残りを quantified variable とする `TypeScheme` を environment に登録する
5. call ごとに quantified variable を fresh inference variable へ instantiate する

非 callable local value は V1 では従来どおり monomorphic とする。この境界は将来の全面的 let-polymorphism と区別する。

### 5.2 共通 argument inference

すべての callable surface は概念上、次の処理を共有する。

```text
infer_argument(actual, expected):
  expected' = resolve_substitution(expected)

  if expected' is an unbound inference variable:
    actual_ty = synthesize(actual)
    unify(expected', actual_ty)
    return actual_ty

  if actual supports bidirectional checking:
    return check(actual, expected')

  actual_ty = synthesize(actual)
  unify(expected', actual_ty)
  return actual_ty
```

expected が function type と確定している closure には引数・返り値 shape を内側へ伝播する。expected が未束縛なら closure 自身から function type を作ってから unify する。

list は expected が `List<T>` なら `T` を全 element へ、tuple は各 slot を対応要素へ、`if` / `match` は result expected type を全 branch/arm へ伝播する。最初の branch だけを結果型の基準にしてはならない。

### 5.3 common route の適用箇所

最低限、次を共通 route の回帰対象とする。

- ordinary positional/named call
- struct/record/enum constructor call
- trait helper call と explicit trait argument call
- Apply / PipeApply
- Compose / KleisliCompose
- operator lowering 後の trait call
- higher-order callable argument

surface ごとに generic argument と closure の順序を変えたり、特定の演算子だけ先に actual を synthesis したりしない。

## 6. エラー契約

追加・整理するエラーは次のとおり。

| phase | kind（名称は実装時に既存命名へ合わせてよい） | 条件 |
|---|---|---|
| resolve | `DuplicateFunction` | 同一 block に同名 `def` / `defp` がある |
| typecheck | `OverlappingTraitImpl` | 同じ trait の impl pattern が再帰的に交差する |
| typecheck | `ConflictingConversionImpl` | `From` / `TryFrom` pattern が交差する |
| typecheck | `UnsatisfiedImplConstraint` | target は一致するが impl `where` obligation を満たさない |
| typecheck | 既存 inference error | actual/expected の unify 後も型が一意に定まらない |

overlap 診断には両 impl の span、canonicalized trait/target pattern、交差を生んだ型位置を含める。宣言順を入れ替えても phase と error kind は同じでなければならない。

Forge は duplicate function index を内部不変条件違反として防御してよいが、正しい user program validation route では到達不能にする。

## 7. 実装方針

### 7.1 Spire / Sigil

- impl/trait/defmod block ごとの callable name set を作り、map へ格納する前に重複を拒否する
- 既存の完全一致 duplicate 検査は早期診断として残してよい
- generic 変数名の文字列一致を semantic duplicate/coherence の正本にしない
- resolved type identity と source span を Scar へ渡す

### 7.2 Scar

- impl target と trait arguments を含む `ImplPattern` を構築する
- alpha-normalization と recursive pattern unifier を 1 実装に集約する
- 通常 trait overlap、同一 nominal target specialization、`From` / `TryFrom` 排他で同じ unifier を使う
- impl predeclare 完了前に pairwise overlap を検査し、method/function index 登録前に停止する
- dispatch 時は pattern match に加えて impl-level `where` obligation を検査する
- local callable environment entry に monotype と type scheme を区別して保持する
- lookup では scheme だけを call-site ごとに fresh instantiate する
- statement 境界の substitution clear に多相性を依存しない
- call/constructor/operator 用の argument inference を共通化する
- `if` / `match` / list / tuple へ expected result shape を伝播する

### 7.3 Forge

- Scar が確定した impl identity を full canonical pattern identity で受け取る
- nominal target name だけで specialization を潰さない
- duplicate function index は compiler invariant error として残し、通常の重複診断には使わない

## 8. テスト観点

### 8.1 overlap / coherence

成功系:

- `Trait<Int> for List<Int>` と `Trait<Int> for List<String>`
- nominal head が異なる target
- trait argument pattern が構造的に disjoint な impl
- alpha-renaming されたが disjoint な pattern

失敗系:

- `$A` と `Int` を両宣言順で登録
- `List<$A>` と `List<Int>` を両宣言順で登録
- `Result<List<$A>, $E>` と `Result<List<Int>, String>`
- `Pair<$A, Int>` と `Pair<String, $B>` のように 1 点で交差する pattern
- target は同じで trait argument が `$A` と `List<$B>`
- trait argument は同じで target が nested generic と concrete
- 異なる `where` constraint を持つが型 pattern が交差する impl
- 同じ入力を file/module 順だけ変え、同じ phase/kind で失敗すること
- overlap が Forge の CodegenError へ進まないこと

### 8.2 method uniqueness

- `defmod` 内の同名 `def`
- inherent impl 内の同名 `def`
- trait impl 内の同名 `def`
- `def` と `defp` の同名衝突
- signature、visibility、generic 名だけを変えた同名衝突
- explicit override 1 件と default method の正常な組み合わせ

### 8.3 `From` / `TryFrom`

- 同一 concrete pair の排他
- `$A` / `$T` と変数名だけを変えた alpha-equivalent pair
- generic と concrete が交差する pair
- nested target/trait argument で交差する pair
- declaration 順を逆にした pair
- target または変換元が disjoint な許可例

### 8.4 call-site polymorphism

- `id(1)` と `id("s")` を同じ tuple 内で呼ぶ
- 同じ 2 call を別 statement に分ける
- 同じ callable を list/tuple/constructor 引数の内外で呼ぶ
- capture を持つ closure の外部型が call ごとに不正に一般化されないこと
- 明示注釈で monomorphic にした callable は異型 call を拒否すること
- outer rigid generic が fresh variable へ置換されないこと

### 8.5 literal-first / bidirectional inference

- generic constructor へ closure を直接渡す
- generic function へ `Int` / `String` / tuple / non-empty list を直接渡す
- `List<Option<Int>>` の element として expected type を必要とする trait helper を使う
- `Option<Int>` を expected type とする `if` の両 branch で trait helper を使う
- `match` の全 arm へ同じ expected result type を伝える
- ordinary call、Apply、PipeApply、Compose で同じ actual expression が同じ型になる
- expected type のない空 list/hash が適切な inference error になる
- literal kind は維持し、`Int` literal を暗黙に `Float` へ coercion しない

### 8.6 テスト配置と実行

- parser/resolver の block 内重複は Sigil unit test
- pattern canonicalization/unification、scheme generalization は Scar unit test
- user-visible 成功例は `tests/fixtures/script/pass/`
- user-visible 失敗例は `tests/fixtures/script/fail/typecheck/` または resolve 用 fail fixture
- module/file 順不変性は `tests/fixtures/modules/fail/`

最低確認コマンド:

```bash
cargo nextest run -p sigil -p scar
cargo nextest run -p rune --test integration run_srt
cargo nextest run -p rune --test integration module_import_fixtures
cargo nextest run --workspace
```

## 9. ドキュメント追従

本改修と同時に次を正本へ反映する。

- `doc/要件定義v9.md`: coherence、recursive overlap、method uniqueness、conversion 排他、call-site scheme、actual synthesis と expected propagation
- `docs/dev/テスト方針.md`: overlap の正逆順/nested pattern、method 重複、call grouping 不変性、common call route の fixture 方針
- `docs/site/trait-system.md` / `docs/site/trait-impls.md` / `docs/site/language-reference.md`: impl の交差規則、conversion 排他、method 一意性と「宣言順・specialization なし」を利用者向けに説明
- `docs/site/callables.md`: local callable の call-site polymorphism と closure/literal 引数推論を説明
- `docs/site/type-annotations.md`: expected type の内向き伝播と空 literal の制約を説明
- `doc/要件定義v9.md`: constructor witness の concrete specialization と overlapping impl の優先順位を区別する

## 10. 実装順と完了条件

実装順は次のとおりとする。

1. canonical `TypePattern` と recursive unifier
2. trait impl coherence と `From` / `TryFrom` 排他
3. impl/defmod method name uniqueness
4. callable `TypeScheme` の generalize/instantiate
5. common argument inference と複合式への expected propagation
6. Forge key の full impl identity 化と防御 assertion
7. fixture、診断 snapshot、正本ドキュメントの最終同期

完了条件:

- overlapping impl の採否が宣言順・file 順に依存しない
- `$A` を含む pattern とその instance は再帰的に overlap error になる
- disjoint な同一 nominal target specialization は区別して codegen できる
- impl block 内の同名 method は上書きされず resolve error になる
- generic の名前を変えても `From` / `TryFrom` 排他を回避できない
- local callable の同じ call 群は改行・tuple grouping によらず同じ型検査結果になる
- closure を含む literal-shaped actual を未束縛 generic へ直接渡せる
- ordinary call、constructor、trait call、Apply、Compose が同じ inference contract を満たす
- impl-level `where` obligation を満たさない call は typecheck で拒否される
- workspace test が成功し、ユーザ起因の impl 重複で CodegenError に到達しない
